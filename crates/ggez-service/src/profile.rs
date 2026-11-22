//! A profile is a TOML file describing how the mouse should be set up.
//! Every key is optional; anything left out is not touched, except buttons:
//! the mouse only accepts a full set, so unlisted buttons get their factory
//! binding whenever a `[buttons]` table is present.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use ggez_core::{Aerox9, Button, ButtonMap, DefaultLighting, PollingRate, Rgb, Transport, Zone};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// 1 to 5 DPI presets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dpi: Option<Vec<u32>>,
    /// Which preset is active, counting from 1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dpi_select: Option<usize>,
    /// Polling rate in Hz.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub polling: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sleep_minutes: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dim_seconds: Option<u32>,
    /// Lighting shown when the mouse powers on: off, reactive, rainbow or
    /// reactive-rainbow.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_lighting: Option<String>,
    /// Color flashed on click, or "off".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reactive: Option<String>,
    /// What the LEDs show: "static" (the zone colors below, the default) or
    /// "rainbow" (the mouse's built-in color cycle; zone colors are ignored).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effect: Option<String>,
    #[serde(default, skip_serializing_if = "Colors::is_empty")]
    pub colors: Colors,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buttons: Option<BTreeMap<String, String>>,
}

/// Zone colors as `#rrggbb` or a color name. `all` fills in any zone that
/// has no color of its own.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Colors {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub all: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub middle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bottom: Option<String>,
}

impl Colors {
    fn is_empty(&self) -> bool {
        *self == Colors::default()
    }
}

pub const DEFAULT_LIGHTING_MODES: [&str; 4] = ["off", "reactive", "rainbow", "reactive-rainbow"];

/// A profile whose every value has been checked and converted.
struct Checked {
    dpi: Option<(Vec<u32>, usize)>,
    polling: Option<PollingRate>,
    sleep_minutes: Option<u32>,
    dim_seconds: Option<u32>,
    default_lighting: Option<DefaultLighting>,
    reactive: Option<Option<Rgb>>,
    rainbow: bool,
    zones: [(Zone, Option<Rgb>); 3],
    buttons: Option<ButtonMap>,
}

impl Profile {
    pub fn parse(text: &str) -> Result<Self> {
        Ok(toml::from_str(text)?)
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("could not read {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("{} is not a valid profile", path.display()))
    }

    pub fn to_toml(&self) -> Result<String> {
        Ok(toml::to_string_pretty(self)?)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("could not create {}", dir.display()))?;
        }
        std::fs::write(path, self.to_toml()?)
            .with_context(|| format!("could not write {}", path.display()))
    }

    /// Check every value without touching a mouse.
    pub fn validate(&self) -> Result<()> {
        self.check().map(|_| ())
    }

    fn check(&self) -> Result<Checked> {
        let dpi = match (&self.dpi, self.dpi_select) {
            (None, Some(_)) => bail!("dpi_select needs dpi"),
            (None, None) => None,
            (Some(presets), select) => {
                let select = select.unwrap_or(1);
                if select == 0 {
                    bail!("dpi_select counts from 1");
                }
                // Encoding checks range, count and selection.
                ggez_core::protocol::set_dpi(ggez_core::Link::Wired, presets, select - 1)?;
                Some((presets.clone(), select - 1))
            }
        };
        let polling = self
            .polling
            .map(|hz| {
                PollingRate::from_hz(hz).with_context(|| {
                    format!("unsupported polling rate {hz} (use 125, 250, 500 or 1000)")
                })
            })
            .transpose()?;
        if let Some(minutes) = self.sleep_minutes {
            ggez_core::protocol::set_sleep_timer(ggez_core::Link::Wired, minutes)?;
        }
        if let Some(seconds) = self.dim_seconds {
            ggez_core::protocol::set_dim_timer(ggez_core::Link::Wired, seconds)?;
        }
        let default_lighting = self
            .default_lighting
            .as_deref()
            .map(|mode| match mode {
                "off" => Ok(DefaultLighting::Off),
                "reactive" => Ok(DefaultLighting::Reactive),
                "rainbow" => Ok(DefaultLighting::Rainbow),
                "reactive-rainbow" => Ok(DefaultLighting::ReactiveRainbow),
                other => bail!("unknown default_lighting {other:?}"),
            })
            .transpose()?;
        let reactive = self
            .reactive
            .as_deref()
            .map(|c| match c {
                "off" | "none" => Ok(None),
                c => c.parse::<Rgb>().map(Some),
            })
            .transpose()?;
        let rainbow = match self.effect.as_deref() {
            None | Some("static") => false,
            Some("rainbow") => true,
            Some(other) => bail!("unknown effect {other:?} (use static or rainbow)"),
        };
        let color = |own: &Option<String>| -> Result<Option<Rgb>> {
            Ok(own
                .as_ref()
                .or(self.colors.all.as_ref())
                .map(|c| c.parse())
                .transpose()?)
        };
        let zones = [
            (Zone::Top, color(&self.colors.top)?),
            (Zone::Middle, color(&self.colors.middle)?),
            (Zone::Bottom, color(&self.colors.bottom)?),
        ];
        let buttons = self
            .buttons
            .as_ref()
            .map(|bindings| -> Result<ButtonMap> {
                let mut map = ButtonMap::default();
                for (button, action) in bindings {
                    map.set(button.parse::<Button>()?, action.parse()?);
                }
                Ok(map)
            })
            .transpose()?;
        Ok(Checked {
            dpi,
            polling,
            sleep_minutes: self.sleep_minutes,
            dim_seconds: self.dim_seconds,
            default_lighting,
            reactive,
            rainbow,
            zones,
            buttons,
        })
    }

    /// Send every setting in the profile. Nothing is sent if any value is
    /// invalid. Returns whether anything was sent.
    pub fn apply<T: Transport>(&self, mouse: &mut Aerox9<T>) -> Result<bool> {
        let checked = self.check()?;
        let mut sent = false;
        if let Some((presets, selected)) = &checked.dpi {
            mouse.set_dpi(presets, *selected)?;
            sent = true;
        }
        if let Some(rate) = checked.polling {
            mouse.set_polling_rate(rate)?;
            sent = true;
        }
        if let Some(minutes) = checked.sleep_minutes {
            mouse.set_sleep_timer(minutes)?;
            sent = true;
        }
        if let Some(seconds) = checked.dim_seconds {
            mouse.set_dim_timer(seconds)?;
            sent = true;
        }
        if let Some(mode) = checked.default_lighting {
            mouse.set_default_lighting(mode)?;
            sent = true;
        }
        if let Some(map) = &checked.buttons {
            mouse.set_buttons(map)?;
            sent = true;
        }
        sent |= Self::send_lighting(&checked, mouse)?;
        Ok(sent)
    }

    /// Send only what the mouse forgets when it powers off: zone colors and
    /// the reactive color. Returns whether anything was sent.
    pub fn apply_volatile<T: Transport>(&self, mouse: &mut Aerox9<T>) -> Result<bool> {
        Self::send_lighting(&self.check()?, mouse)
    }

    /// Whether the profile has anything [`Profile::apply_volatile`] would send.
    pub fn has_volatile(&self) -> bool {
        self.reactive.is_some() || self.effect.is_some() || !self.colors.is_empty()
    }

    fn send_lighting<T: Transport>(checked: &Checked, mouse: &mut Aerox9<T>) -> Result<bool> {
        let mut sent = false;
        if let Some(color) = checked.reactive {
            mouse.set_reactive_color(color)?;
            sent = true;
        }
        if checked.rainbow {
            mouse.rainbow_effect()?;
            return Ok(true);
        }
        for (zone, color) in checked.zones {
            if let Some(color) = color {
                mouse.set_zone_color(zone, color)?;
                sent = true;
            }
        }
        Ok(sent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ggez_core::{Link, MockTransport};

    const FULL: &str = r##"
        dpi = [800, 1600]
        dpi_select = 2
        polling = 1000
        sleep_minutes = 5
        default_lighting = "off"
        reactive = "off"

        [colors]
        all = "#ff0000"
        bottom = "blue"

        [buttons]
        g12 = "key:ctrl+v"
        "##;

    fn commands(
        send: impl FnOnce(&Profile, &mut Aerox9<MockTransport>) -> Result<bool>,
        text: &str,
    ) -> Vec<Vec<u8>> {
        let mut mouse = Aerox9::new(MockTransport::accepting(), Link::Wireless);
        send(&Profile::parse(text).unwrap(), &mut mouse).unwrap();
        mouse.transport().written.clone()
    }

    #[test]
    fn full_profile_sends_each_setting() {
        let packets = commands(|p, m| p.apply(m), FULL);
        let sent: Vec<u8> = packets.iter().map(|p| p[1]).collect();
        assert_eq!(
            sent,
            [0x6D, 0x6B, 0x69, 0x67, 0x6A, 0x6A, 0x6A, 0x66, 0x61, 0x61, 0x61]
        );
        assert_eq!(packets[0][2..6], [2, 1, 0x09, 0x12]);
        assert_eq!(packets[10][2..7], [0x01, 0x02, 0, 0, 0xFF]);
    }

    #[test]
    fn volatile_apply_sends_only_lighting() {
        let packets = commands(|p, m| p.apply_volatile(m), FULL);
        let sent: Vec<u8> = packets.iter().map(|p| p[1]).collect();
        assert_eq!(sent, [0x66, 0x61, 0x61, 0x61]);
        assert!(Profile::parse(FULL).unwrap().has_volatile());
        assert!(!Profile::parse("polling = 500").unwrap().has_volatile());
    }

    #[test]
    fn rainbow_effect_replaces_zone_colors() {
        let text = "effect = \"rainbow\"
[colors]
all = \"red\"
";
        let packets = commands(|p, m| p.apply_volatile(m), text);
        assert_eq!(packets, [vec![0x00, 0x62, 0xFF]]);
        assert!(Profile::parse("effect = \"disco\"")
            .unwrap()
            .validate()
            .is_err());
    }

    #[test]
    fn empty_profile_sends_nothing() {
        assert!(commands(|p, m| p.apply(m), "").is_empty());
    }

    #[test]
    fn a_bad_value_sends_nothing() {
        for text in [
            "polling = 1000\n[buttons]\ng99 = \"left\"",
            "dpi = [50]",
            "dpi = [800]\ndpi_select = 2",
            "sleep_minutes = 99",
            "[colors]\ntop = \"nope\"",
        ] {
            let mut mouse = Aerox9::new(MockTransport::accepting(), Link::Wireless);
            let profile = Profile::parse(text).unwrap();
            assert!(profile.apply(&mut mouse).is_err(), "{text}");
            assert!(mouse.transport().written.is_empty(), "{text}");
        }
    }

    #[test]
    fn unknown_keys_are_rejected() {
        assert!(Profile::parse("poling = 1000").is_err());
    }

    #[test]
    fn survives_a_round_trip_through_toml() {
        let profile = Profile::parse(FULL).unwrap();
        assert_eq!(
            Profile::parse(&profile.to_toml().unwrap()).unwrap(),
            profile
        );
        assert_eq!(Profile::default().to_toml().unwrap().trim(), "");
    }
}
