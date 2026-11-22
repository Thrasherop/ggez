//! Button remapping.
//!
//! Worked out from USB captures of SteelSeries GG talking to an Aerox 9
//! Wireless through its 2.4 GHz dongle (firmware 1.2.4). The mouse holds a
//! table of 20 slots, five bytes each: `[kind, parameter, 0, 0, 0]`. GG
//! always rewrites the whole table, as three 64-byte zero-padded packets
//! `6a <part> <slots...>` holding 6, 6 and 8 slots.
//!
//! The mouse has no known "read bindings" command, so a [`ButtonMap`] starts
//! from the factory layout and every write replaces all bindings.

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
    /// The button behind the scroll wheel.
    Dpi,
    /// Side grid button 1-12, numbered as printed on the mouse.
    Grid(u8),
    ScrollUp,
    ScrollDown,
}

/// Table slot of each grid button, indexed by grid number - 1. The grid is
/// three buttons wide and the table stores it column by column, right
/// column first. Buttons 1-6 were confirmed by pressing them on a real
/// mouse; 7-12 follow the same pattern.
const GRID_SLOTS: [usize; 12] = [14, 10, 6, 15, 11, 7, 16, 12, 8, 17, 13, 9];

impl Button {
    fn slot(self) -> usize {
        match self {
            Button::Left => 0,
            Button::Right => 1,
            Button::Middle => 2,
            // Slots 3 and 4 are mouse buttons 4 and 5, which this mouse has
            // no physical control for; they keep their factory values.
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
    /// A keyboard key, as a USB HID keyboard usage code.
    Key(u8),
    /// A media key, as a USB HID consumer usage code.
    Media(u8),
}

impl Action {
    fn encode(self) -> [u8; SLOT_LEN] {
        let (kind, parameter) = match self {
            Action::Disabled => (KIND_DISABLED, 0),
            Action::MouseButton(n) => (n, 0),
            Action::DpiCycle => (KIND_DPI, 0),
            Action::ScrollUp => (KIND_SCROLL_UP, 0),
            Action::ScrollDown => (KIND_SCROLL_DOWN, 0),
            Action::Key(code) => (KIND_KEY, code),
            Action::Media(code) => (KIND_MEDIA, code),
        };
        [kind, parameter, 0, 0, 0]
    }
}

/// Parses `disabled`, `left`, `right`, `middle`, `back`, `forward`, `dpi`,
/// `scroll-up`, `scroll-down`, `key:<name>` and `media:<name>`.
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
                Some(("key", name)) => Action::Key(key_code(name).ok_or_else(bad)?),
                Some(("media", name)) => Action::Media(media_code(name).ok_or_else(bad)?),
                _ => return Err(bad()),
            },
        })
    }
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
    Some(match name {
        "enter" => 0x28,
        "escape" | "esc" => 0x29,
        "backspace" => 0x2A,
        "tab" => 0x2B,
        "space" => 0x2C,
        "minus" => 0x2D,
        "equals" => 0x2E,
        "insert" => 0x49,
        "home" => 0x4A,
        "page-up" => 0x4B,
        "delete" => 0x4C,
        "end" => 0x4D,
        "page-down" => 0x4E,
        "right-arrow" => 0x4F,
        "left-arrow" => 0x50,
        "down-arrow" => 0x51,
        "up-arrow" => 0x52,
        _ => return None,
    })
}

/// USB HID consumer usage for a media key name. Only `play-pause` has been
/// observed in GG's traffic; the rest are the standard codes on that page.
fn media_code(name: &str) -> Option<u8> {
    Some(match name {
        "play-pause" => 0xCD,
        "next" => 0xB5,
        "previous" => 0xB6,
        "stop" => 0xB7,
        "mute" => 0xE2,
        "volume-up" => 0xE9,
        "volume-down" => 0xEA,
        _ => return None,
    })
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
            map.set(Button::Grid(n), Action::Key(code));
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
        for (n, key) in [(3, "6"), (4, "7"), (5, "8"), (6, "9"), (7, "i"), (8, "o"), (9, "p")] {
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
        assert_eq!(map.get(Button::Grid(1)), Action::Key(0x1E));
        assert_eq!(map.get(Button::Grid(10)), Action::Key(0x27));
        assert_eq!(map.get(Button::Grid(12)), Action::Key(0x2E));
        assert_eq!(map.get(Button::Left), Action::MouseButton(1));
    }

    #[test]
    fn parses_names() {
        assert_eq!("G12".parse::<Button>().unwrap(), Button::Grid(12));
        assert!("g13".parse::<Button>().is_err());
        assert_eq!("key:f13".parse::<Action>().unwrap(), Action::Key(0x68));
        assert_eq!("key:0x04".parse::<Action>().unwrap(), Action::Key(0x04));
        assert!("key:nope".parse::<Action>().is_err());
        assert!("media:".parse::<Action>().is_err());
    }

    #[test]
    fn cable_is_refused() {
        assert!(ButtonMap::default().packets(Link::Wired).is_err());
    }
}
