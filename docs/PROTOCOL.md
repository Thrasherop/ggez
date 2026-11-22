# Aerox 9 Wireless protocol notes

What ggez knows about how the SteelSeries Aerox 9 Wireless is configured, and
how sure it is. Everything here was learned by observing the device's USB
traffic (with [`tools/capture.ps1`](../tools/capture.ps1)), by sending
commands and reading the replies, or from the
[rivalcfg](https://github.com/flozz/rivalcfg) project. Nothing comes from
SteelSeries' software itself.

Tested on firmware **1.2.4**. Each fact carries one of these marks:

| Mark | Meaning |
| --- | --- |
| ✅ | Seen working on a real mouse (effect observed by a person) |
| ☑️ | Accepted by the mouse, byte-identical to what GG sends, or both; effect not observed |
| ❓ | Inferred or reported elsewhere; not yet checked |

## Devices

| Product | USB ID |
| --- | --- |
| Aerox 9 Wireless, USB cable | `1038:185a` |
| Aerox 9 Wireless, 2.4 GHz dongle | `1038:1858` |
| Aerox 9 Wireless WoW Edition, cable / dongle | `1038:1876` / `1038:1874` ❓ |

Both the mouse (on its cable) and the dongle present six HID interfaces.
Configuration uses **interface 3** (usage page `0xFFC0`). Interface 4
(`0xFFC1`) is also vendor-defined; nothing has been seen on it.

Plugging in the cable takes the mouse off the radio: the dongle then reports
no link, and the mouse must be configured through the cable. GG does not
configure a cabled mouse at all; ggez does.

## Framing

- A command is an HID output report: report ID `0x00`, then the command
  byte, then its arguments. GG pads to 64 bytes with zeros; short reports
  work too. ✅
- Through the dongle, `0x40` is added to the command byte (`0x2B` becomes
  `0x6B`). ✅
- The mouse answers every settings command with an input report
  `[command, status]`. Status `0x00` means accepted. This happens on the
  cable as well as the dongle. ✅ The bytes after the status are leftovers
  from earlier replies and mean nothing.
- When the dongle has no link to the mouse it answers `40 FF` instead. ✅
- Replies normally arrive within 20 ms. Saving takes up to 450 ms.
- The mouse does **not** check argument values: a button table full of
  `0xFF` is acknowledged like any other. An acknowledgement only means the
  command arrived.
- Other software talking to the same dongle (GG sends about 66 lighting
  commands a second) makes commands go unanswered. ggez resends; closing GG
  is the real fix.

## Commands

Command bytes are given as sent over the cable; add `0x40` for the dongle
unless noted.

| Command | Bytes | Notes |
| --- | --- | --- |
| CPI levels | `2D count active level…` | 1-5 levels, one byte each from the table in [`dpi_table.rs`](../crates/ggez-core/src/dpi_table.rs). 100-18000 in steps of 100. ☑️ byte-identical to GG for 400/800/1400/2400/3200 |
| Polling rate | `2B n` | 1000 Hz = 0, 500 = 1, 250 = 2, 125 = 3. ☑️ GG's capture showed 1 ms reports at 1000 Hz. Over the dongle a change of rate drops the radio link; see "Radio link" |
| Zone color | `21 01 zone R G B` | Zone 0 front, 1 palm, 2 wrist. ✅ Not stored by the mouse |
| Reactive color | `26 01 00 R G B`, off `26 00 00 00 00 00` | Flash on click. ☑️ |
| Rainbow effect | `22 FF` | The built-in color cycle. ☑️ |
| Power-on lighting | `27 a b` | off `00 00`, reactive `00 01`, rainbow `01 00`, reactive-rainbow `01 01`. ✅ for off and rainbow. Stored by the mouse |
| Sleep timer | `29` + milliseconds, 3 bytes little-endian | 0-20 minutes; 0 = never ❓. ☑️ |
| Illumination | `23 brightness 01 smart 00` + dim timer in milliseconds, 3 bytes little-endian | Brightness 0-15 (GG's slider sent `00`, `04` and `0F` at its lowest, middle and highest). Smart = 1 turns the lights off while the mouse moves. Dim timer 0-1200 s. The `01` is in everything GG sends; meaning ❓. ☑️ byte-identical to GG; effect not yet watched |
| High-Efficiency flag | `28 n` | 1 on, 0 off. GG sends it between `23 00 01 01` (lights off) and `2B 03` (125 Hz) for its "High-Efficiency Mode", and the reverse to leave it. What the flag changes by itself is ❓. ☑️ |
| Button table | `2A part slot…` | See below. ✅ |
| Save | `11 00` | Writes current settings to onboard memory. ✅ for button bindings and power-on lighting |
| Battery | `92` | Reply `[cmd, b]`: bit 7 = charging, level = ((b & 0x7F) - 1) × 5 %. ☑️ plausible readings; 0 means no mouse |
| Firmware version | `90` (dongle `D0`) | Reply is ASCII text such as `1.2.4` and does **not** echo the command. `90` sent to the dongle unflagged returns the dongle's own version ❓. ☑️ |
| Link status | `BC` (to the dongle, no flag) | Reply `[BC, 1]` linked, `[BC, 0]` not. Answered by the dongle itself. ☑️ |

GG's start-up sequence to the dongle, in order: `2D` CPI, `26` reactive,
`29` sleep, `2B` polling, `27` power-on lighting, `28` High-Efficiency flag,
`23` illumination, `2A` ×3
buttons, `11` save. It then streams `21` zone colors continuously to animate
them.

## What the mouse stores

Saved by `11 00` and kept through a power cycle:

- Button bindings ✅
- Power-on lighting mode ✅
- CPI levels, polling rate, sleep and dim timers ❓ (expected, not yet
  power-cycled)

Not stored:

- Static zone colors ✅. After power-on the mouse shows its power-on
  lighting mode until software sends colors. ggez's background helper
  re-sends them whenever the mouse connects or wakes.

## Button table

Twenty slots of five bytes each, sent as three packets:

```
2A 00 <slots 0-5>     left, right, middle, tilt left, tilt right, CPI button
2A 01 <slots 6-11>    side 3, 6, 9, 12, 2, 5
2A 02 <slots 12-19>   side 8, 11, 1, 4, 7, 10, scroll up, scroll down
```

The side grid is three buttons wide; the table stores it one column at a
time, right-hand column first. All twelve positions ✅.

A slot is `[kind, a, b, c, d]`:

| Kind | Meaning | Arguments |
| --- | --- | --- |
| `00` | Disabled | GG leaves the old arguments in place; they are ignored ✅ |
| `01`-`05` | Mouse button: left, right, middle, back, forward | none ✅ for the defaults |
| `06`-`08` | Mouse buttons 6, 7, 8 | none ✅ They set bits `0x20`, `0x40`, `0x80` of the first byte of the mouse report for as long as the button is held. Windows passes only buttons 1-5 on to programs |
| `30` | Next CPI level | none ☑️ |
| `31`, `32` | Scroll up, scroll down | none ☑️ |
| `51` | Keyboard keys | Up to four USB HID keyboard usage codes pressed together. Modifiers are ordinary codes `E0`-`E7`. ✅ including Ctrl+V |
| `61` | Media key | One USB HID consumer usage code (`CD` = play/pause). ☑️ |
| `33`, `34`, `62`, `71` | `33`, `34`, `62` are listed in rivalcfg pull request 243 as pan left/right and "meta"; `71` is the Prime's macro kind | With all-zero arguments, pressing the button changed nothing in the mouse report beyond its raw-state bit. Whether they need arguments, or report elsewhere, is ❓ |

Key bindings are performed by the mouse and follow the button: the key goes
down when the button does and comes up when it is released. ✅ A side
button bound to a key can therefore be held.

The whole table is written every time. There is no known way to read it
back.

## Macros

No on-device macro storage has been found. GG's "OS shortcut" and macro
bindings are done in software: GG sets the slot to `00` (disabled) and acts
when it sees the button pressed. That is why they fire once, cannot be held,
and need GG running.

The related SteelSeries Prime stores macros on the device (button kind `71`
pointing into a bank written with command `5E`). On the Aerox 9 a slot of
kind `71` with zero arguments did nothing when pressed, and GG never uses
it. Command `5E` has not been sent, and will not be without a capture of GG
sending it. The most the mouse is known to do by itself is press up to four
keys together for as long as the button is held.

## Input reports

Over the dongle, the ordinary mouse report (interrupt endpoint `0x81`) is 12
bytes. Besides movement it carries the physical state of every button in
bytes 7-9, whatever each button is bound to, for exactly as long as it is
held. Seen ✅: left `01` of byte 7; side 3 `40` and side 6 `80` of byte 7;
side 2 `04`, side 5 `08`, side 1 `40` and side 4 `80` of byte 8; side 7
`01` of byte 9. The rest of the map is ❓. This is presumably how GG learns
that a "disabled" button was pressed, and it is what a software macro
engine in ggez would read.

Byte 0 of the same report is the ordinary button bitmap (bit 0 left, up to
bit 7 for button 8).

Key bindings are sent as ordinary keyboard reports on endpoint `0x86`.

## Radio link

The dongle reports the link on its own: `BC 01` when the mouse joins and
`BC 00` when it leaves, unasked, on endpoint `0x85`. ☑️

While there is no link the dongle answers every command meant for the mouse
with `40 FF`. ✅

Changing the polling rate makes the mouse leave and rejoin. ✅ GG's own
lighting stream got `40 FF` for about 2 seconds after each change with the
mouse beside the dongle; from 15 feet and two walls away ggez saw no link
for more than 9 seconds. A command sent in that window is lost, so ggez
keeps resending the command that follows a polling change for up to 20
seconds.

Replies carry leftovers: a short reply such as `61 00` arrives with bytes
of an earlier, longer reply after it (`61 00 32 2E 34`, the tail of the
firmware text). Only the first two bytes of an acknowledgement mean
anything. ✅

## Not done by the mouse

GG offers these, but sent the mouse nothing when they were changed, live or
on Save, so it does them in software on the computer: ✅

- **Angle snapping**
- **Acceleration and deceleration**
- **Lighting effects** other than the built-in rainbow (Steady with
  several zones aside): ColorShift and Multi Color Breathe are a stream of
  `21` zone colors, about 32 a second.

ggez does not copy them: it stays out of the path that movement takes.

## Open questions

- **What the High-Efficiency flag (`28 01`) does by itself**, and the
  third byte of `23`.
- **Button kinds `33`, `34`, `62`, `71`.**
- **The unasked `12 14` report** the dongle sends a tenth of a second
  after the mouse rejoins.
- **Bluetooth.** Untested.
- **Reading settings back.** None known.

## How to add to this

1. Close ggez's helper and start a capture: `tools\capture.ps1 -Name
   what-i-changed -Seconds 60`.
2. Change one setting in GG.
3. `tools\decode.ps1 what-i-changed` and compare with the tables above.

Do not send commands that are not listed here to see what they do. An
unknown command could erase or reflash the mouse.
