//! Button remapping.
//!
//! Worked out from USB captures of SteelSeries GG talking to an Aerox 9
//! Wireless through its 2.4 GHz dongle (firmware 1.2.4). The mouse holds a
//! table of 20 slots, five bytes each: `[kind, parameter, 0, 0, 0]`. GG
//! always rewrites the whole table, as three 64-byte zero-padded packets
//! `6a <part> <slots...>` holding 6, 6 and 8 slots.
//!
//! Key bindings are performed by the mouse itself and follow the button: the
//! key goes down on press and up on release, so they can be held. A slot
//! may name up to four keys, which is how GG stores Ctrl+Shift+K.
//!
//! GG's OS shortcuts and macros are not in this table. GG disables the slot
//! and acts in software, which is why they fire once and need GG running.
//! It can do that because the ordinary mouse input report (12 bytes on the
//! dongle) carries the physical state of every button in bytes 7-9, whatever
//! the binding: side button 1 was seen as bit 0x40 of byte 8 for exactly as
//! long as it was held.
//!
//! The mouse has no known "read bindings" command, so a [`ButtonMap`] starts
//! from the factory layout and every write replaces all bindings.

use std::fmt;
use std::str::FromStr;

use crate::protocol::{Link, ProtoError};

const SLOT_COUNT: usize = 20;
const SLOT_LEN: usize = 5;
/// Slots carried by each of the three packets.
const PARTS: [std::ops::Range<usize>; 3] = [0..6, 6..12, 12..20];
/// Command 0x2A with the wireless flag.
const COMMAND_WIRELESS: u8 = 0x6A;
const PACKET_LEN: usize = 64;

const KIND_DISABLED: u8 = 0x00;
const KIND_DPI: u8 = 0x30;
const KIND_SCROLL_UP: u8 = 0x31;
const KIND_SCROLL_DOWN: u8 = 0x32;
const KIND_KEY: u8 = 0x51;
const KIND_MEDIA: u8 = 0x61;

/// A physical control that can be rebound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
    /// Scroll wheel pushed left; mouse button 4 (back) from the factory.
    TiltLeft,
    /// Scroll wheel pushed right; mouse button 5 (forward) from the factory.
    TiltRight,
    /// The button behind the scroll wheel.
    Dpi,
    /// Side grid button 1-12, numbered as printed on the mouse.
    Grid(u8),
    ScrollUp,
    ScrollDown,
}

/// Table slot of each grid button, indexed by grid number - 1. The grid is
/// three buttons wide and the table stores it column by column, right
/// column first. All twelve were confirmed by pressing them on a real mouse.
const GRID_SLOTS: [usize; 12] = [14, 10, 6, 15, 11, 7, 16, 12, 8, 17, 13, 9];

impl Button {
    fn slot(self) -> usize {
        match self {
            Button::Left => 0,
            Button::Right => 1,
            Button::Middle => 2,
            Button::TiltLeft => 3,
            Button::TiltRight => 4,
            Button::Dpi => 5,
            Button::Grid(n) => GRID_SLOTS[usize::from(n) - 1],
            Button::ScrollUp => 18,
            Button::ScrollDown => 19,
        }
    }
}

impl FromStr for Button {
    type Err = ProtoError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let lower = s.to_ascii_lowercase();
        Ok(match lower.as_str() {
            "left" => Button::Left,
            "right" => Button::Right,
            "middle" => Button::Middle,
            "tilt-left" => Button::TiltLeft,
            "tilt-right" => Button::TiltRight,
            "dpi" => Button::Dpi,
            "scroll-up" => Button::ScrollUp,
            "scroll-down" => Button::ScrollDown,
            other => match other.strip_prefix('g').and_then(|n| n.parse::<u8>().ok()) {
                Some(n @ 1..=12) => Button::Grid(n),
                _ => return Err(ProtoError::BadButton(s.to_string())),
            },
        })
    }
}

/// What a button does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Disabled,
    /// Mouse button 1-5 (1 left, 2 right, 3 middle, 4 back, 5 forward).
    MouseButton(u8),
    /// Cycle through the DPI presets.
    DpiCycle,
    ScrollUp,
    ScrollDown,
    /// Up to four keyboard keys pressed together, as USB HID keyboard usage
    /// codes; unused positions are 0. Modifiers are ordinary codes (0xE0-0xE7).
    Keys([u8; 4]),
    /// A media key, as a USB HID consumer usage code.
    Media(u8),
}

impl Action {
    /// A single keyboard key.
    pub fn key(code: u8) -> Self {
        Action::Keys([code, 0, 0, 0])
    }

    fn encode(self) -> [u8; SLOT_LEN] {
        let (kind, parameter) = match self {
            Action::Disabled => (KIND_DISABLED, 0),
            Action::MouseButton(n) => (n, 0),
            Action::DpiCycle => (KIND_DPI, 0),
            Action::ScrollUp => (KIND_SCROLL_UP, 0),
            Action::ScrollDown => (KIND_SCROLL_DOWN, 0),
            Action::Keys(k) => return [KIND_KEY, k[0], k[1], k[2], k[3]],
            Action::Media(code) => (KIND_MEDIA, code),
        };
        [kind, parameter, 0, 0, 0]
    }
}

/// Parses `disabled`, `left`, `right`, `middle`, `back`, `forward`, `dpi`,
/// `scroll-up`, `scroll-down`, `key:<name>[+<name>...]` and `media:<name>`.
impl FromStr for Action {
    type Err = ProtoError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let lower = s.to_ascii_lowercase();
        let bad = || ProtoError::BadAction(s.to_string());
        Ok(match lower.as_str() {
            "disabled" | "off" | "none" => Action::Disabled,
            "left" => Action::MouseButton(1),
            "right" => Action::MouseButton(2),
            "middle" => Action::MouseButton(3),
            "back" => Action::MouseButton(4),
            "forward" => Action::MouseButton(5),
            "dpi" => Action::DpiCycle,
            "scroll-up" => Action::ScrollUp,
            "scroll-down" => Action::ScrollDown,
            other => match other.split_once(':') {
                Some(("key", combo)) => Action::Keys(key_combo(combo).ok_or_else(bad)?),
                Some(("media", name)) => Action::Media(media_code(name).ok_or_else(bad)?),
                _ => return Err(bad()),
            },
        })
    }
}

/// One to four key names joined by `+`, e.g. `ctrl+shift+k`.
fn key_combo(combo: &str) -> Option<[u8; 4]> {
    let mut codes = [0u8; 4];
    for (i, name) in combo.split('+').enumerate() {
        *codes.get_mut(i)? = key_code(name)?;
    }
    Some(codes)
}

/// USB HID keyboard usage for a key name: a letter, a digit, `f1`-`f24`, a
/// named key, or a raw code such as `0x68`.
fn key_code(name: &str) -> Option<u8> {
    if let Some(hex) = name.strip_prefix("0x") {
        return u8::from_str_radix(hex, 16).ok().filter(|&c| c != 0);
    }
    if let [c] = name.as_bytes() {
        return match c {
            b'a'..=b'z' => Some(0x04 + (c - b'a')),
            b'1'..=b'9' => Some(0x1E + (c - b'1')),
            b'0' => Some(0x27),
            b'-' => Some(0x2D),
            b'=' => Some(0x2E),
            _ => None,
        };
    }
    if let Some(n) = name.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
        return match n {
            1..=12 => Some(0x3A + (n - 1)),
            13..=24 => Some(0x68 + (n - 13)),
            _ => None,
        };
    }
    let name = match name {
        "esc" => "escape",
        "cmd" | "super" => "win",
        "rcmd" | "rsuper" => "rwin",
        other => other,
    };
    NAMED_KEYS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, code)| *code)
}

/// USB HID consumer usage for a media key name. Only `play-pause` has been
/// observed in GG's traffic; the rest are the standard codes on that page.
fn media_code(name: &str) -> Option<u8> {
    if let Some(hex) = name.strip_prefix("0x") {
        return u8::from_str_radix(hex, 16).ok().filter(|&c| c != 0);
    }
    MEDIA_KEYS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, code)| *code)
}

/// A complete set of bindings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ButtonMap {
    slots: [Action; SLOT_COUNT],
}

impl Default for ButtonMap {
    /// The factory layout: grid buttons type 1-9, 0, minus and equals.
    fn default() -> Self {
        let mut slots = [Action::Disabled; SLOT_COUNT];
        for (i, slot) in slots.iter_mut().take(5).enumerate() {
            *slot = Action::MouseButton(i as u8 + 1);
        }
        let mut map = Self { slots };
        map.set(Button::Dpi, Action::DpiCycle);
        map.set(Button::ScrollUp, Action::ScrollUp);
        map.set(Button::ScrollDown, Action::ScrollDown);
        const GRID_KEYS: [u8; 12] = [
            0x1E, 0x1F, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x2D, 0x2E,
        ];
        for (n, code) in (1..).zip(GRID_KEYS) {
            map.set(Button::Grid(n), Action::key(code));
        }
        map
    }
}

impl ButtonMap {
    pub fn set(&mut self, button: Button, action: Action) {
        self.slots[button.slot()] = action;
    }

    pub fn get(&self, button: Button) -> Action {
        self.slots[button.slot()]
    }

    /// The packets (without report ID) that write this map to the mouse.
    pub fn packets(&self, link: Link) -> Result<Vec<Vec<u8>>, ProtoError> {
        if link == Link::Wired {
            // The cable's framing for this command has not been captured.
            return Err(ProtoError::ButtonsNeedDongle);
        }
        Ok(PARTS
            .iter()
            .enumerate()
            .map(|(part, range)| {
                let mut packet = vec![COMMAND_WIRELESS, part as u8];
                for action in &self.slots[range.clone()] {
                    packet.extend(action.encode());
                }
                packet.resize(PACKET_LEN, 0);
                packet
            })
            .collect())
    }
}

impl Button {
    /// Every rebindable control, in the order a user would list them.
    pub const ALL: [Button; 20] = [
        Button::Left,
        Button::Right,
        Button::Middle,
        Button::TiltLeft,
        Button::TiltRight,
        Button::Dpi,
        Button::Grid(1),
        Button::Grid(2),
        Button::Grid(3),
        Button::Grid(4),
        Button::Grid(5),
        Button::Grid(6),
        Button::Grid(7),
        Button::Grid(8),
        Button::Grid(9),
        Button::Grid(10),
        Button::Grid(11),
        Button::Grid(12),
        Button::ScrollUp,
        Button::ScrollDown,
    ];
}

/// The name [`Button::from_str`] accepts.
impl fmt::Display for Button {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Button::Left => f.write_str("left"),
            Button::Right => f.write_str("right"),
            Button::Middle => f.write_str("middle"),
            Button::TiltLeft => f.write_str("tilt-left"),
            Button::TiltRight => f.write_str("tilt-right"),
            Button::Dpi => f.write_str("dpi"),
            Button::Grid(n) => write!(f, "g{n}"),
            Button::ScrollUp => f.write_str("scroll-up"),
            Button::ScrollDown => f.write_str("scroll-down"),
        }
    }
}

/// The text [`Action::from_str`] accepts.
impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Action::Disabled => f.write_str("disabled"),
            Action::MouseButton(1) => f.write_str("left"),
            Action::MouseButton(2) => f.write_str("right"),
            Action::MouseButton(3) => f.write_str("middle"),
            Action::MouseButton(4) => f.write_str("back"),
            Action::MouseButton(_) => f.write_str("forward"),
            Action::DpiCycle => f.write_str("dpi"),
            Action::ScrollUp => f.write_str("scroll-up"),
            Action::ScrollDown => f.write_str("scroll-down"),
            Action::Keys(codes) => {
                f.write_str("key:")?;
                let names: Vec<String> = codes
                    .iter()
                    .filter(|&&c| c != 0)
                    .map(|&c| key_name(c))
                    .collect();
                f.write_str(&names.join("+"))
            }
            Action::Media(code) => write!(f, "media:{}", media_name(*code)),
        }
    }
}

/// Names and codes of keys that are not a single letter, digit or F-key.
const NAMED_KEYS: [(&str, u8); 27] = [
    ("ctrl", 0xE0),
    ("shift", 0xE1),
    ("alt", 0xE2),
    ("win", 0xE3),
    ("rctrl", 0xE4),
    ("rshift", 0xE5),
    ("ralt", 0xE6),
    ("rwin", 0xE7),
    ("enter", 0x28),
    ("escape", 0x29),
    ("backspace", 0x2A),
    ("tab", 0x2B),
    ("space", 0x2C),
    ("minus", 0x2D),
    ("equals", 0x2E),
    ("insert", 0x49),
    ("home", 0x4A),
    ("page-up", 0x4B),
    ("delete", 0x4C),
    ("end", 0x4D),
    ("page-down", 0x4E),
    ("right-arrow", 0x4F),
    ("left-arrow", 0x50),
    ("down-arrow", 0x51),
    ("up-arrow", 0x52),
    ("print-screen", 0x46),
    ("caps-lock", 0x39),
];

const MEDIA_KEYS: [(&str, u8); 7] = [
    ("play-pause", 0xCD),
    ("next", 0xB5),
    ("previous", 0xB6),
    ("stop", 0xB7),
    ("mute", 0xE2),
    ("volume-up", 0xE9),
    ("volume-down", 0xEA),
];

/// Every key name [`Action`] understands besides raw `0x..` codes, for
/// pickers in a user interface.
pub fn key_names() -> Vec<String> {
    let mut names: Vec<String> = (b'a'..=b'z').map(|c| (c as char).to_string()).collect();
    names.extend((0..=9).map(|d| d.to_string()));
    names.extend((1..=24).map(|n| format!("f{n}")));
    names.extend(NAMED_KEYS.iter().map(|(name, _)| name.to_string()));
    names
}

/// Every media key name [`Action`] understands.
pub fn media_names() -> Vec<&'static str> {
    MEDIA_KEYS.iter().map(|(name, _)| *name).collect()
}

fn key_name(code: u8) -> String {
    match code {
        0x04..=0x1D => ((b'a' + code - 0x04) as char).to_string(),
        0x1E..=0x26 => ((b'1' + code - 0x1E) as char).to_string(),
        0x27 => "0".to_string(),
        0x3A..=0x45 => format!("f{}", code - 0x3A + 1),
        0x68..=0x73 => format!("f{}", code - 0x68 + 13),
        _ => match NAMED_KEYS.iter().find(|(_, c)| *c == code) {
            Some((name, _)) => name.to_string(),
            None => format!("0x{code:02x}"),
        },
    }
}

fn media_name(code: u8) -> String {
    match MEDIA_KEYS.iter().find(|(_, c)| *c == code) {
        Some((name, _)) => name.to_string(),
        None => format!("0x{code:02x}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unpadded(packet: &[u8]) -> &[u8] {
        let len = packet.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
        &packet[..len]
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// Byte-for-byte what GG sent after binding grid 1 to Play/Pause and
    /// grid 2 to K, on a mouse whose owner had also customized other buttons.
    #[test]
    fn matches_captured_gg_packets() {
        let mut map = ButtonMap::default();
        map.set(Button::Grid(1), "media:play-pause".parse().unwrap());
        map.set(Button::Grid(2), "key:k".parse().unwrap());
        for (n, key) in [
            (3, "6"),
            (4, "7"),
            (5, "8"),
            (6, "9"),
            (7, "i"),
            (8, "o"),
            (9, "p"),
        ] {
            map.set(Button::Grid(n), format!("key:{key}").parse().unwrap());
        }
        // GG leaves the old key code behind when it disables a button; the
        // captured packets had 2e, 2d and 27 there. Compare those as zero.
        map.set(Button::Grid(10), Action::Disabled);
        map.set(Button::Grid(11), Action::Disabled);
        map.set(Button::Grid(12), Action::Disabled);

        let packets = map.packets(Link::Wireless).unwrap();
        assert!(packets.iter().all(|p| p.len() == 64));
        assert_eq!(
            unpadded(&packets[0]),
            hex("6a000100000000020000000003000000000400000000050000000030")
        );
        assert_eq!(
            unpadded(&packets[1]),
            hex("6a015123000000512600000051130000000000000000510e0000005125")
        );
        assert_eq!(
            unpadded(&packets[2]),
            hex("6a025112000000000000000061cd0000005124000000510c0000000000000000310000000032")
        );
    }

    #[test]
    fn default_grid_types_its_printed_labels() {
        let map = ButtonMap::default();
        assert_eq!(map.get(Button::Grid(1)), Action::key(0x1E));
        assert_eq!(map.get(Button::Grid(10)), Action::key(0x27));
        assert_eq!(map.get(Button::Grid(12)), Action::key(0x2E));
        assert_eq!(map.get(Button::Left), Action::MouseButton(1));
    }

    #[test]
    fn parses_names() {
        assert_eq!("G12".parse::<Button>().unwrap(), Button::Grid(12));
        assert!("g13".parse::<Button>().is_err());
        assert_eq!("key:f13".parse::<Action>().unwrap(), Action::key(0x68));
        assert_eq!("key:0x04".parse::<Action>().unwrap(), Action::key(0x04));
        // What GG sent for Ctrl+Shift+K: 51 e1 e0 0e 00.
        assert_eq!(
            "key:shift+ctrl+k".parse::<Action>().unwrap().encode(),
            [0x51, 0xE1, 0xE0, 0x0E, 0x00]
        );
        assert!("key:a+b+c+d+e".parse::<Action>().is_err());
        assert!("key:nope".parse::<Action>().is_err());
        assert!("media:".parse::<Action>().is_err());
    }

    #[test]
    fn names_round_trip() {
        for button in Button::ALL {
            assert_eq!(button.to_string().parse::<Button>().unwrap(), button);
        }
        let default = ButtonMap::default();
        for button in Button::ALL {
            let action = default.get(button);
            assert_eq!(action.to_string().parse::<Action>().unwrap(), action);
        }
        for text in [
            "key:ctrl+shift+k",
            "key:f13",
            "key:0x64",
            "media:play-pause",
            "disabled",
        ] {
            assert_eq!(text.parse::<Action>().unwrap().to_string(), text);
        }
        for name in key_names() {
            assert!(format!("key:{name}").parse::<Action>().is_ok(), "{name}");
        }
    }

    #[test]
    fn cable_is_refused() {
        assert!(ButtonMap::default().packets(Link::Wired).is_err());
    }
}
