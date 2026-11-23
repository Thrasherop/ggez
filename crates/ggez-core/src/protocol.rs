//! Pure packet encoding for the Aerox 9 Wireless. No I/O happens here.
//!
//! Every builder returns the command payload *without* the leading HID
//! report ID; [`crate::device::Aerox9`] prepends `0x00` when sending.
//!
//! The 2.4 GHz dongle speaks the same protocol as the USB cable, except the
//! first command byte has bit 6 set (see [`Link`]).

use std::str::FromStr;

use crate::dpi_table::{DPI_MAX, DPI_MIN, DPI_STEP, TRUEMOVE_AIR};

pub const VENDOR_ID: u16 = 0x1038;
/// The HID interface that accepts configuration commands.
pub const CONFIG_INTERFACE: i32 = 3;
/// The usage page that interface declares. Some systems cannot say which
/// interface number a HID device has, and it is then known by this.
pub const CONFIG_USAGE_PAGE: u16 = 0xFFC0;
pub const MAX_DPI_PRESETS: usize = 5;
pub const MAX_SLEEP_MINUTES: u32 = 20;
pub const MAX_DIM_SECONDS: u32 = 1200;
pub const MAX_BRIGHTNESS: u8 = 15;

const WIRELESS_FLAG: u8 = 0x40;
const BATTERY_CHARGING_FLAG: u8 = 0x80;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProtoError {
    #[error("DPI {0} is out of range ({DPI_MIN}-{DPI_MAX})")]
    DpiOutOfRange(u32),
    #[error("need 1 to {MAX_DPI_PRESETS} DPI presets, got {0}")]
    DpiPresetCount(usize),
    #[error("selected preset {selected} does not exist (have {count})")]
    DpiSelection { selected: usize, count: usize },
    #[error("sleep timer must be 0-{MAX_SLEEP_MINUTES} minutes, got {0}")]
    SleepOutOfRange(u32),
    #[error("dim timer must be 0-{MAX_DIM_SECONDS} seconds, got {0}")]
    DimOutOfRange(u32),
    #[error("brightness must be 0-{MAX_BRIGHTNESS}, got {0}")]
    BrightnessOutOfRange(u8),
    #[error("invalid color {0:?} (use #rrggbb or a basic color name)")]
    BadColor(String),
    #[error("unexpected battery response {0:02x?}")]
    BadBatteryResponse(Vec<u8>),
    #[error("unknown button {0:?} (use left, right, middle, tilt-left, tilt-right, dpi, g1-g12, scroll-up, scroll-down)")]
    BadButton(String),
    #[error("unknown action {0:?} (use disabled, left, right, middle, back, forward, dpi, scroll-up, scroll-down, key:<name>, media:<name>)")]
    BadAction(String),
}

/// How the mouse is connected. Determines the command flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    /// USB cable.
    Wired,
    /// 2.4 GHz dongle.
    Wireless,
}

impl Link {
    /// Identify a supported device from its USB product ID.
    pub fn from_product_id(pid: u16) -> Option<Link> {
        match pid {
            0x185A | 0x1876 => Some(Link::Wired), // Aerox 9 / WoW Edition
            0x1858 | 0x1874 => Some(Link::Wireless),
            _ => None,
        }
    }

    fn apply(self, mut payload: Vec<u8>) -> Vec<u8> {
        if self == Link::Wireless {
            payload[0] |= WIRELESS_FLAG;
        }
        payload
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// `#rrggbb`, which [`Rgb::from_str`] accepts.
impl std::fmt::Display for Rgb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
}

impl FromStr for Rgb {
    type Err = ProtoError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let named = match s.to_ascii_lowercase().as_str() {
            "black" => Some(Rgb(0, 0, 0)),
            "white" => Some(Rgb(255, 255, 255)),
            "red" => Some(Rgb(255, 0, 0)),
            "green" | "lime" => Some(Rgb(0, 255, 0)),
            "blue" => Some(Rgb(0, 0, 255)),
            "yellow" => Some(Rgb(255, 255, 0)),
            "cyan" | "aqua" => Some(Rgb(0, 255, 255)),
            "magenta" | "fuchsia" | "pink" => Some(Rgb(255, 0, 255)),
            "orange" => Some(Rgb(255, 128, 0)),
            "purple" => Some(Rgb(128, 0, 255)),
            _ => None,
        };
        if let Some(c) = named {
            return Ok(c);
        }
        let hex = s.strip_prefix('#').unwrap_or(s);
        let bad = || ProtoError::BadColor(s.to_string());
        if hex.len() != 6 || !hex.is_ascii() {
            return Err(bad());
        }
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| bad());
        Ok(Rgb(byte(0)?, byte(2)?, byte(4)?))
    }
}

/// The three LED zones along the body of the mouse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Top = 0,
    Middle = 1,
    Bottom = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PollingRate {
    Hz125,
    Hz250,
    Hz500,
    Hz1000,
}

impl PollingRate {
    pub fn from_hz(hz: u32) -> Option<Self> {
        match hz {
            125 => Some(Self::Hz125),
            250 => Some(Self::Hz250),
            500 => Some(Self::Hz500),
            1000 => Some(Self::Hz1000),
            _ => None,
        }
    }

    fn code(self) -> u8 {
        match self {
            Self::Hz1000 => 0x00,
            Self::Hz500 => 0x01,
            Self::Hz250 => 0x02,
            Self::Hz125 => 0x03,
        }
    }
}

/// Lighting the mouse shows on power-up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefaultLighting {
    Off,
    Reactive,
    Rainbow,
    ReactiveRainbow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Battery {
    /// Charge in percent, in steps of 5.
    pub percent: u8,
    pub charging: bool,
}

fn le24(v: u32) -> [u8; 3] {
    let b = v.to_le_bytes();
    [b[0], b[1], b[2]]
}

/// Encode one DPI value, rounding to the nearest 100 (ties round down).
pub fn encode_dpi(dpi: u32) -> Result<u8, ProtoError> {
    if dpi < DPI_MIN as u32 || dpi > DPI_MAX as u32 {
        return Err(ProtoError::DpiOutOfRange(dpi));
    }
    let step = DPI_STEP as u32;
    let index = ((dpi + step / 2 - 1) / step).max(1) - 1;
    Ok(TRUEMOVE_AIR[index as usize])
}

/// Set the DPI presets and which one (0-based) is active.
pub fn set_dpi(link: Link, presets: &[u32], selected: usize) -> Result<Vec<u8>, ProtoError> {
    if presets.is_empty() || presets.len() > MAX_DPI_PRESETS {
        return Err(ProtoError::DpiPresetCount(presets.len()));
    }
    if selected >= presets.len() {
        return Err(ProtoError::DpiSelection {
            selected,
            count: presets.len(),
        });
    }
    let mut p = vec![0x2D, presets.len() as u8, selected as u8];
    for &dpi in presets {
        p.push(encode_dpi(dpi)?);
    }
    Ok(link.apply(p))
}

pub fn set_polling_rate(link: Link, rate: PollingRate) -> Vec<u8> {
    link.apply(vec![0x2B, rate.code()])
}

pub fn set_zone_color(link: Link, zone: Zone, c: Rgb) -> Vec<u8> {
    link.apply(vec![0x21, 0x01, zone as u8, c.0, c.1, c.2])
}

/// Color flashed on button click; `None` turns the reactive effect off.
pub fn set_reactive_color(link: Link, color: Option<Rgb>) -> Vec<u8> {
    let p = match color {
        Some(c) => vec![0x26, 0x01, 0x00, c.0, c.1, c.2],
        None => vec![0x26, 0x00, 0x00, 0x00, 0x00, 0x00],
    };
    link.apply(p)
}

/// Idle time before the mouse sleeps. 0 disables sleep.
pub fn set_sleep_timer(link: Link, minutes: u32) -> Result<Vec<u8>, ProtoError> {
    if minutes > MAX_SLEEP_MINUTES {
        return Err(ProtoError::SleepOutOfRange(minutes));
    }
    let mut p = vec![0x29];
    p.extend(le24(minutes * 60_000));
    Ok(link.apply(p))
}

/// How the LEDs behave as a whole. The mouse takes all three in one command,
/// so changing one means sending the others too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Illumination {
    /// 0 (off) to [`MAX_BRIGHTNESS`].
    pub brightness: u8,
    /// Turn the LEDs off while the mouse is moving, to save battery.
    pub smart: bool,
    /// Idle time before the LEDs dim. 0 disables dimming.
    pub dim_seconds: u32,
}

impl Default for Illumination {
    /// Full brightness, dimming after half a minute, lit while moving.
    fn default() -> Self {
        Self {
            brightness: MAX_BRIGHTNESS,
            smart: false,
            dim_seconds: 30,
        }
    }
}

pub fn set_illumination(link: Link, lights: Illumination) -> Result<Vec<u8>, ProtoError> {
    if lights.dim_seconds > MAX_DIM_SECONDS {
        return Err(ProtoError::DimOutOfRange(lights.dim_seconds));
    }
    if lights.brightness > MAX_BRIGHTNESS {
        return Err(ProtoError::BrightnessOutOfRange(lights.brightness));
    }
    // The third byte is 1 in everything SteelSeries GG sends; its meaning
    // is not known.
    let mut p = vec![0x23, lights.brightness, 0x01, u8::from(lights.smart), 0x00];
    p.extend(le24(lights.dim_seconds * 1_000));
    Ok(link.apply(p))
}

/// The mouse's own power-saving flag. SteelSeries GG sets it together with
/// brightness 0 and 125 Hz polling for its "High-Efficiency Mode"; what the
/// flag changes by itself has not been measured.
pub fn set_high_efficiency(link: Link, on: bool) -> Vec<u8> {
    link.apply(vec![0x28, u8::from(on)])
}

/// Start the rainbow effect. Setting any zone color stops it.
pub fn rainbow_effect(link: Link) -> Vec<u8> {
    link.apply(vec![0x22, 0xFF])
}

pub fn set_default_lighting(link: Link, mode: DefaultLighting) -> Vec<u8> {
    let (rainbow, reactive) = match mode {
        DefaultLighting::Off => (0, 0),
        DefaultLighting::Reactive => (0, 1),
        DefaultLighting::Rainbow => (1, 0),
        DefaultLighting::ReactiveRainbow => (1, 1),
    };
    link.apply(vec![0x27, rainbow, reactive])
}

/// Persist the current settings to the mouse's onboard memory.
pub fn save(link: Link) -> Vec<u8> {
    link.apply(vec![0x11, 0x00])
}

/// Ask the 2.4 GHz dongle whether it currently has a radio link to the
/// mouse. The reply is `[0xBC, 1]` when linked and `[0xBC, 0]` when the mouse
/// is off, asleep or out of range. The dongle answers this itself, so it
/// works even with no mouse. Not meaningful over the cable.
pub fn link_status_query() -> Vec<u8> {
    vec![0xBC]
}

/// Ask for the firmware version. The reply is the version as ASCII text
/// (for example `1.2.4`) and, unlike other replies, does not echo the command.
pub fn firmware_query(link: Link) -> Vec<u8> {
    link.apply(vec![0x90])
}

/// Extract the version from a reply to [`firmware_query`], if it is one.
pub fn parse_firmware(response: &[u8]) -> Option<String> {
    let end = response
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(response.len());
    let text = &response[..end];
    let plausible = text.len() >= 3
        && text[0].is_ascii_digit()
        && text.contains(&b'.')
        && text.iter().all(|&b| b.is_ascii_digit() || b == b'.');
    plausible.then(|| String::from_utf8_lossy(text).into_owned())
}

pub fn battery_query(link: Link) -> Vec<u8> {
    link.apply(vec![0x92])
}

/// Decode the response to [`battery_query`].
pub fn parse_battery(response: &[u8]) -> Result<Battery, ProtoError> {
    let bad = || ProtoError::BadBatteryResponse(response.to_vec());
    let raw = *response.get(1).ok_or_else(bad)?;
    let level = raw & !BATTERY_CHARGING_FLAG;
    // Level 0 means the dongle has no mouse to report on (off or asleep).
    let percent = level.checked_sub(1).ok_or_else(bad)? as u32 * 5;
    if percent > 100 {
        return Err(bad());
    }
    Ok(Battery {
        percent: percent as u8,
        charging: raw & BATTERY_CHARGING_FLAG != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use Link::{Wired, Wireless};

    #[test]
    fn dpi_table_lookup_and_rounding() {
        assert_eq!(encode_dpi(100), Ok(0x00));
        assert_eq!(encode_dpi(800), Ok(0x09));
        assert_eq!(encode_dpi(18000), Ok(0xD6));
        assert_eq!(encode_dpi(849), Ok(0x09));
        assert_eq!(encode_dpi(850), Ok(0x09), "ties round down");
        assert_eq!(encode_dpi(851), Ok(0x0A));
        assert_eq!(encode_dpi(99), Err(ProtoError::DpiOutOfRange(99)));
        assert_eq!(encode_dpi(18001), Err(ProtoError::DpiOutOfRange(18001)));
    }

    #[test]
    fn dpi_table_is_strictly_increasing() {
        assert!(TRUEMOVE_AIR.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn dpi_presets() {
        assert_eq!(
            set_dpi(Wired, &[800, 1600], 0).unwrap(),
            [0x2D, 0x02, 0x00, 0x09, 0x12]
        );
        assert_eq!(
            set_dpi(Wireless, &[400, 800, 1200, 2400, 3200], 2).unwrap(),
            [0x6D, 0x05, 0x02, 0x04, 0x09, 0x0D, 0x1B, 0x26]
        );
        assert!(set_dpi(Wired, &[], 0).is_err());
        assert!(set_dpi(Wired, &[800; 6], 0).is_err());
        assert!(set_dpi(Wired, &[800], 1).is_err());
    }

    #[test]
    fn simple_commands() {
        assert_eq!(set_polling_rate(Wired, PollingRate::Hz1000), [0x2B, 0x00]);
        assert_eq!(set_polling_rate(Wireless, PollingRate::Hz125), [0x6B, 0x03]);
        assert_eq!(
            set_zone_color(Wired, Zone::Middle, Rgb(1, 2, 3)),
            [0x21, 0x01, 0x01, 1, 2, 3]
        );
        assert_eq!(
            set_zone_color(Wireless, Zone::Bottom, Rgb(255, 0, 0)),
            [0x61, 0x01, 0x02, 255, 0, 0]
        );
        assert_eq!(
            set_reactive_color(Wired, Some(Rgb(9, 8, 7))),
            [0x26, 0x01, 0x00, 9, 8, 7]
        );
        assert_eq!(set_reactive_color(Wired, None), [0x26, 0, 0, 0, 0, 0]);
        assert_eq!(rainbow_effect(Wireless), [0x62, 0xFF]);
        assert_eq!(
            set_default_lighting(Wired, DefaultLighting::ReactiveRainbow),
            [0x27, 0x01, 0x01]
        );
        assert_eq!(save(Wired), [0x11, 0x00]);
        assert_eq!(save(Wireless), [0x51, 0x00]);
        assert_eq!(battery_query(Wired), [0x92]);
        assert_eq!(battery_query(Wireless), [0xD2]);
    }

    #[test]
    fn timers() {
        // 5 min = 300_000 ms = 0x0493E0
        assert_eq!(set_sleep_timer(Wired, 5).unwrap(), [0x29, 0xE0, 0x93, 0x04]);
        // 20 min = 0x124F80
        assert_eq!(
            set_sleep_timer(Wireless, 20).unwrap(),
            [0x69, 0x80, 0x4F, 0x12]
        );
        assert!(set_sleep_timer(Wired, 21).is_err());
        // 30 s = 30_000 ms = 0x7530
        assert_eq!(
            set_illumination(Wired, Illumination::default()).unwrap(),
            [0x23, 0x0F, 0x01, 0x00, 0x00, 0x30, 0x75, 0x00]
        );
        let lights = |brightness, smart, dim_seconds| Illumination {
            brightness,
            smart,
            dim_seconds,
        };
        assert!(set_illumination(Wired, lights(15, false, 1201)).is_err());
        assert!(set_illumination(Wired, lights(16, false, 30)).is_err());
    }

    #[test]
    fn illumination_matches_what_gg_sends() {
        // Captured from SteelSeries GG over the dongle, dim timer 30 s:
        // Smart Mode on, then the brightness slider at its middle and lowest.
        let lights = |brightness, smart| {
            set_illumination(
                Wireless,
                Illumination {
                    brightness,
                    smart,
                    dim_seconds: 30,
                },
            )
            .unwrap()
        };
        assert_eq!(lights(15, true), [0x63, 0x0F, 1, 1, 0, 0x30, 0x75, 0]);
        assert_eq!(lights(4, true), [0x63, 0x04, 1, 1, 0, 0x30, 0x75, 0]);
        assert_eq!(lights(0, true), [0x63, 0x00, 1, 1, 0, 0x30, 0x75, 0]);
        // High-Efficiency Mode on and off.
        assert_eq!(set_high_efficiency(Wireless, true), [0x68, 1]);
        assert_eq!(set_high_efficiency(Wireless, false), [0x68, 0]);
    }

    #[test]
    fn battery() {
        assert_eq!(
            parse_battery(&[0x92, 0x0B]),
            Ok(Battery {
                percent: 50,
                charging: false
            })
        );
        assert_eq!(
            parse_battery(&[0x92, 0x95]),
            Ok(Battery {
                percent: 100,
                charging: true
            })
        );
        assert!(parse_battery(&[0x92, 0x00]).is_err());
        assert!(parse_battery(&[0x92, 0x7F]).is_err());
        assert!(parse_battery(&[0x92]).is_err());
    }

    #[test]
    fn colors() {
        assert_eq!("#ff8000".parse(), Ok(Rgb(255, 128, 0)));
        assert_eq!("00FF7f".parse(), Ok(Rgb(0, 255, 127)));
        assert_eq!("Red".parse(), Ok(Rgb(255, 0, 0)));
        assert!("#fff".parse::<Rgb>().is_err());
        assert!("nope".parse::<Rgb>().is_err());
    }

    #[test]
    fn product_ids() {
        assert_eq!(Link::from_product_id(0x185A), Some(Wired));
        assert_eq!(Link::from_product_id(0x1858), Some(Wireless));
        assert_eq!(Link::from_product_id(0x1234), None);
    }
}
