# ggez

A small, native replacement for SteelSeries GG/Engine for the **Aerox 9 Wireless**,
written in Rust. One binary, no background services, and the same code on
Windows, macOS and Linux.

Status: early. Every command has been accepted by a real Aerox 9 Wireless
through its 2.4 GHz dongle (firmware 1.2.4), including with SteelSeries GG
running. Button bindings were checked by pressing all twelve side buttons, and zone
colors by eye. The USB cable path is untested.

## What works (in code)

| Feature | Command |
| --- | --- |
| Battery level / charging | `ggez battery` |
| DPI presets (1-5, 100-18000) | `ggez dpi 800,1600,3200 --select 2` |
| Polling rate | `ggez polling 1000` |
| LED zone colors | `ggez color --all "#ff8000" --bottom blue` |
| Reactive (click) color | `ggez reactive red` / `ggez reactive off` |
| Rainbow effect | `ggez rainbow` |
| Power-on lighting | `ggez default-lighting rainbow` |
| Sleep / dim timers | `ggez sleep 5`, `ggez dim 30` |
| List SteelSeries HID interfaces | `ggez list` |

Settings are saved to the mouse's onboard memory unless you pass `--no-save`.

Both USB cable (`1038:185a`) and the 2.4 GHz dongle (`1038:1858`) are
supported, plus the WoW Edition IDs. Bluetooth is not.

## Not done yet

- Button remapping over the USB cable (the dongle works: `ggez buttons
  g1=key:f13 g2=media:play-pause`), key combinations with modifiers, and
  macros.
- Reading current settings back from the mouse.
- A GUI.

## Testing without hardware

`--dry-run wired|wireless` prints the exact packets instead of sending them:

```
$ ggez --dry-run wireless dpi 800,1600,3200 --select 2
00 6d 03 01 09 12 26
00 51 00
```

`cargo test --workspace` checks every packet builder against known-good bytes
using a mock transport.

## Building

```
cargo build --release
```

Needs a C compiler for `hidapi`. On Linux you also need `libudev-dev`, and a
udev rule to access the device without root.

## Layout

- `crates/ggez-core` - protocol encoding (pure), transport trait + mock +
  hidapi, and the `Aerox9` device API.
- `crates/ggez-cli` - the `ggez` binary.

## Credits

Protocol details come from [rivalcfg](https://github.com/flozz/rivalcfg).
Not affiliated with SteelSeries.
