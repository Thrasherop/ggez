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
| Polling rate | `2B n` | 1000 Hz = 0, 500 = 1, 250 = 2, 125 = 3. ☑️ GG's capture showed 1 ms reports at 1000 Hz |
| Zone color | `21 01 zone R G B` | Zone 0 front, 1 palm, 2 wrist. ✅ Not stored by the mouse |
| Reactive color | `26 01 00 R G B`, off `26 00 00 00 00 00` | Flash on click. ☑️ |
| Rainbow effect | `22 FF` | The built-in color cycle. ☑️ |
| Power-on lighting | `27 a b` | off `00 00`, reactive `00 01`, rainbow `01 00`, reactive-rainbow `01 01`. ✅ for off and rainbow. Stored by the mouse |
| Sleep timer | `29` + milliseconds, 3 bytes little-endian | 0-20 minutes; 0 = never ❓. ☑️ |
| Dim timer | `23 0F 01 00 00` + milliseconds, 3 bytes little-endian | 0-1200 s. ☑️ See "Open questions" for the leading bytes |
| Button table | `2A part slot…` | See below. ✅ |
| Save | `11 00` | Writes current settings to onboard memory. ✅ for button bindings and power-on lighting |
| Battery | `92` | Reply `[cmd, b]`: bit 7 = charging, level = ((b & 0x7F) - 1) × 5 %. ☑️ plausible readings; 0 means no mouse |
| Firmware version | `90` (dongle `D0`) | Reply is ASCII text such as `1.2.4` and does **not** echo the command. `90` sent to the dongle unflagged returns the dongle's own version ❓. ☑️ |
| Link status | `BC` (to the dongle, no flag) | Reply `[BC, 1]` linked, `[BC, 0]` not. Answered by the dongle itself. ☑️ |
| Unknown | `28` | GG sends it, with no arguments seen, in its start-up sequence. ❓ |

GG's start-up sequence to the dongle, in order: `2D` CPI, `26` reactive,
`29` sleep, `2B` polling, `27` power-on lighting, `28`, `23` dim, `2A` ×3
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
| `30` | Next CPI level | none ☑️ |
| `31`, `32` | Scroll up, scroll down | none ☑️ |
| `51` | Keyboard keys | Up to four USB HID keyboard usage codes pressed together. Modifiers are ordinary codes `E0`-`E7`. ✅ including Ctrl+V |
| `61` | Media key | One USB HID consumer usage code (`CD` = play/pause). ☑️ |
| `06`-`08`, `33`, `34`, `62` | Listed in rivalcfg pull request 243 as mouse buttons 6-8, pan left/right, and "meta" | ❓ |

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
pointing into a bank written with command `5E`). Whether the Aerox 9
firmware has anything similar is ❓. The mouse acknowledges a slot of kind
`71`, but it acknowledges everything.

## Input reports

Over the dongle, the ordinary mouse report (interrupt endpoint `0x81`) is 12
bytes. Besides movement it carries the physical state of every button in
bytes 7-9, whatever each button is bound to: side button 1 was seen as bit
`0x40` of byte 8 for exactly as long as it was held. ✅ for that one bit;
the full bit map is ❓. This is presumably how GG learns that a "disabled"
button was pressed.

Key bindings are sent as ordinary keyboard reports on endpoint `0x86`.

## Open questions

- **Smart illumination** (lights off while the mouse moves) and
  **brightness.** GG sends the dim timer as `23 0F 01 01 00 <ms>` with
  Smart Mode on, where rivalcfg sends `23 0F 01 00 00 <ms>`. OpenRGB uses
  `23 <0-10>` as a brightness command. So the bytes after `23` may be
  brightness, an enable flag and the smart-mode flag.
- **Angle snapping, acceleration/deceleration, High-Efficiency Mode.** GG
  offers them; their commands have not been captured.
- **Command `28`.**
- **Lighting effects** beyond rainbow. GG animates them from the computer.
- **Bluetooth.** Untested.
- **Reading settings back.** None known.

## How to add to this

1. Close ggez's helper and start a capture: `tools\capture.ps1 -Name
   what-i-changed -Seconds 60`.
2. Change one setting in GG.
3. `tools\decode.ps1 what-i-changed` and compare with the tables above.

Do not send commands that are not listed here to see what they do. An
unknown command could erase or reflash the mouse.
