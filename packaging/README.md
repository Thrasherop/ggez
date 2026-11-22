# Packaging and releases

## Making a release

1. Set the new version in `Cargo.toml` (`[workspace.package]`), commit, and
   wait for CI to pass.
2. Tag that commit and push the tag:

   ```
   git tag v0.2.0
   git push origin v0.2.0
   ```

3. The [Release workflow](../.github/workflows/release.yml) builds the
   downloads and attaches them to a **draft** release. It refuses a tag
   that does not match the version in `Cargo.toml`.
4. Open the draft on the repository's Releases page, install at least one
   of the downloads, edit the notes, and publish.

To see what a release would contain without making one, run the workflow by
hand from the Actions tab; the files are kept as artifacts of that run.

## What is built

| System | File | Made by |
| --- | --- | --- |
| Windows | `ggez-<version>-windows-x64-setup.exe` | [windows/ggez.iss](windows/ggez.iss), with Inno Setup |
| macOS | `ggez-<version>-macos.dmg` | [macos/bundle.sh](macos/bundle.sh) |
| Linux | `ggez-<version>-linux-x64.tar.gz` | the workflow itself |

**Windows.** The installer installs for the current user, so it does not
ask for administrator rights. It adds a Start menu entry and an uninstaller.
Uninstalling stops the helper and removes it from the programs that start
at login; settings in `%APPDATA%\ggez` are kept. The command-line program
`ggez.exe` is installed next to the app but is not added to `PATH`.

**macOS.** `ggez.app` holds both the Intel and the Apple Silicon program
and needs macOS 11 or newer. The command-line program is inside it, at
`ggez.app/Contents/MacOS/ggez`.

**Linux.** The two programs and the udev rule, to be copied into place by
hand. A `.deb` and a desktop entry are still to do.

## Signing

Unsigned downloads work, but the system warns before running them.

### macOS

With these repository secrets (Settings → Secrets and variables → Actions)
the workflow signs the app with a Developer ID and has Apple notarize the
disk image, so it opens without a warning:

| Secret | What it is |
| --- | --- |
| `MACOS_CERTIFICATE` | A "Developer ID Application" certificate with its private key, exported from Keychain Access as a `.p12` file and base64-encoded (`base64 -i certificate.p12 \| pbcopy`) |
| `MACOS_CERTIFICATE_PASSWORD` | The password chosen when exporting the `.p12` |
| `MACOS_SIGN_IDENTITY` | The certificate's full name, such as `Developer ID Application: Jane Doe (AB12CD34EF)` |
| `MACOS_NOTARY_APPLE_ID` | The Apple ID of the developer account |
| `MACOS_NOTARY_TEAM_ID` | The ten-character team ID |
| `MACOS_NOTARY_PASSWORD` | An app-specific password for that Apple ID, made at account.apple.com |

Without them the app gets an ad-hoc signature. It runs, but on first launch
macOS refuses it until the user allows it under System Settings → Privacy
& Security.

### Windows

Not set up. Without a signature, SmartScreen shows "Windows protected your
PC" and the user has to choose More info → Run anyway. Signing needs a
code-signing certificate or a signing service, which costs money; see the
[releases issue](https://github.com/Thrasherop/ggez/issues/14).
