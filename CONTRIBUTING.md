# Contributing to ggez

Thanks for helping. The [roadmap](https://github.com/Thrasherop/ggez/issues/23)
says what needs doing and in what order; the issues it links are the place
to start.

## Building and testing

You need Rust 1.88 or newer (`rust-toolchain.toml` pins it). On Linux also
install the libraries listed in the README.

```
cargo build
cargo test
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
```

CI runs those four on Windows, macOS and Linux and must be green.

None of the tests need a mouse. `ggez --dry-run wired <command>` prints the
bytes a command would send, which is the quickest way to see what a change
does.

## What "tested" means here

ggez talks to hardware that acknowledges every command whether or not it
makes sense, so a passing test and a happy mouse prove less than they seem
to. Three things are expected of a change:

1. **Automated tests** for the logic. The window has tests that click
   through it (`crates/ggez-gui/src/app_tests.rs`); the command line has
   tests that run the real program (`crates/ggez-cli/tests/cli.rs`).
2. **A look at the real thing** for anything visible. Run the app and click
   through what you changed. Two bugs in the first version of the window
   passed every automated test and were found in the first minute of use.
3. **An honest note of what was verified.** If you changed how a setting is
   sent and could only confirm that the mouse acknowledged it, say so. In
   `docs/PROTOCOL.md`, mark it accepted, not seen working.

## Adding to what is known about the mouse

Everything ggez knows about the protocol is in
[docs/PROTOCOL.md](docs/PROTOCOL.md), each fact marked as seen working,
accepted by the mouse, or a guess. New knowledge comes from watching what
SteelSeries GG sends:

1. `tools\capture.ps1 -Name what-i-changed -Seconds 60` (Windows, needs
   USBPcap and Wireshark; it asks for administrator rights).
2. Change one setting in GG while it records.
3. `tools\decode.ps1 what-i-changed`.

Captures record every device on the same USB hub while they run. The
scripts keep only the SteelSeries traffic and delete the rest, and
`captures/` is ignored by git; never commit a capture.

**Do not send the mouse commands that are not in `docs/PROTOCOL.md` to see
what happens.** An unknown command could erase or reflash it. Learn the
command from a capture first.

Do not use SteelSeries' own files or code as a source. Observing the
device's USB traffic is the method.

## Where things go

| | |
| --- | --- |
| `crates/ggez-core` | The protocol and the device. No files, no user interface. |
| `crates/ggez-service` | Profiles, the background helper, start at login. |
| `crates/ggez-cli` | The `ggez` command. |
| `crates/ggez-gui` | The app. `app.rs` holds what the window knows and decides, `app/view.rs` draws it, `theme.rs` is the look, `mouse_view.rs` is the drawing of the mouse. |

A new setting usually touches each layer once: an encoder in
`ggez-core::protocol` with a test of its exact bytes, a method on `Aerox9`,
a key in the profile, a CLI command, and a control in the app.

## Style

- Write for the person who uses the mouse, not the person who wrote the
  code. Text in the app and in errors says what happened and what to do, in
  plain words.
- Prefer the mouse. If the mouse can hold a setting, store it there, so it
  works without ggez running.
- Comments explain why, not what.
- Keep ggez out of the path that movement and clicks take.

## License

By contributing you agree that your work is released under the MIT license
in [LICENSE](LICENSE).
