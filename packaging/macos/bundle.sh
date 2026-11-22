#!/usr/bin/env bash
# Builds dist/ggez-<version>-macos.dmg: a ggez.app that runs on both Intel
# and Apple Silicon Macs, in a disk image with a shortcut to Applications.
#
#   packaging/macos/bundle.sh 0.1.0
#
# Run from the repository root, on a Mac, after building both targets:
#   cargo build --release --target aarch64-apple-darwin
#   cargo build --release --target x86_64-apple-darwin
#
# Signing is controlled by the environment:
#   SIGN_IDENTITY   "Developer ID Application: Name (TEAMID)". Without it the
#                   app gets an ad-hoc signature, which runs but makes macOS
#                   ask the user to approve it in Privacy & Security.
#   NOTARY_APPLE_ID, NOTARY_TEAM_ID, NOTARY_PASSWORD
#                   An Apple ID, its team, and an app-specific password. With
#                   all three and SIGN_IDENTITY, the disk image is notarized
#                   so that it opens without any warning.
set -euo pipefail

version="${1:?usage: bundle.sh <version>}"
app="dist/ggez.app"
image="dist/ggez-${version}-macos.dmg"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

rm -rf "$app" "$image"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

# One program file holding both processor types.
for program in ggez-gui ggez; do
    lipo -create -output "$app/Contents/MacOS/$program" \
        "target/aarch64-apple-darwin/release/$program" \
        "target/x86_64-apple-darwin/release/$program"
done

sed "s/@VERSION@/${version}/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
cp LICENSE README.md profile.example.toml "$app/Contents/Resources/"

# The icon, drawn by the program's own code at every size macOS asks for.
iconset="$work/ggez.iconset"
mkdir "$iconset"
for size in 16 32 128 256 512; do
    cargo run --quiet --release -p ggez-gui --example icon_png -- \
        "$iconset/icon_${size}x${size}.png" "$size"
    cargo run --quiet --release -p ggez-gui --example icon_png -- \
        "$iconset/icon_${size}x${size}@2x.png" "$((size * 2))"
done
iconutil --convert icns --output "$app/Contents/Resources/ggez.icns" "$iconset"

if [[ -n "${SIGN_IDENTITY:-}" ]]; then
    echo "Signing as: $SIGN_IDENTITY"
    codesign --force --options runtime --timestamp --sign "$SIGN_IDENTITY" \
        "$app/Contents/MacOS/ggez" "$app/Contents/MacOS/ggez-gui"
    codesign --force --options runtime --timestamp --sign "$SIGN_IDENTITY" "$app"
else
    echo "No SIGN_IDENTITY: using an ad-hoc signature."
    codesign --force --sign - "$app/Contents/MacOS/ggez" "$app/Contents/MacOS/ggez-gui"
    codesign --force --sign - "$app"
fi
codesign --verify --deep --strict "$app"

# The disk image: the app beside a shortcut to Applications to drag it to.
staging="$work/image"
mkdir "$staging"
cp -R "$app" "$staging/"
ln -s /Applications "$staging/Applications"
hdiutil create -volname "ggez" -srcfolder "$staging" -ov -format UDZO "$image"

if [[ -n "${SIGN_IDENTITY:-}" ]]; then
    codesign --force --timestamp --sign "$SIGN_IDENTITY" "$image"
    if [[ -n "${NOTARY_APPLE_ID:-}" && -n "${NOTARY_TEAM_ID:-}" && -n "${NOTARY_PASSWORD:-}" ]]; then
        echo "Notarizing (this can take a few minutes)."
        xcrun notarytool submit "$image" --wait \
            --apple-id "$NOTARY_APPLE_ID" \
            --team-id "$NOTARY_TEAM_ID" \
            --password "$NOTARY_PASSWORD"
        xcrun stapler staple "$image"
    else
        echo "Signed but not notarized: the NOTARY_* values are not all set."
    fi
fi

echo "Wrote $image"
