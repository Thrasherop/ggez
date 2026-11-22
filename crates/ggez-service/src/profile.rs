//! A profile is a TOML file describing how the mouse should be set up.
//! Every key is optional; anything left out is not touched, except buttons:
//! the mouse only accepts a full set, so unlisted buttons get their factory
//! binding whenever a `[buttons]` table is present.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use ggez_core::protocol::{Illumination, MAX_BRIGHTNESS};
use ggez_core::{
    Action, Aerox9, Button, ButtonMap, DefaultLighting, PollingRate, Rgb, Transport, Zone,
};
use serde::{Deserialize, Serialize};

use crate::macros::{MacroSpec, Plan};

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
    /// Seconds of idle before the lights dim (0 = never).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dim_seconds: Option<u32>,
    /// How bright the lights are, 0 to 100 percent. The mouse has 16 steps.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brightness: Option<u32>,
    /// Turn the lights off while the mouse is moving, to save battery.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smart_lighting: Option<bool>,
    /// Lights off, 125 Hz polling and the mouse's own power-saving flag, for
    /// the longest battery life. Overrides `polling` and the lighting keys
    /// above while it is on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub battery_saver: Option<bool>,
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
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "in_physical_order"
    )]
    pub buttons: Option<BTreeMap<String, String>>,
    /// Buttons that run a list of steps on the computer instead of doing
    /// one thing on the mouse. They need the helper running. See
    /// [`crate::macros`].
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "in_physical_order"
    )]
    pub macros: Option<BTreeMap<String, MacroSpec>>,
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

/// Write bindings in the order the buttons sit on the mouse (`g2` before
/// `g10`), not in alphabetical order.
fn in_physical_order<S: serde::Serializer, V: Serialize>(
    buttons: &Option<BTreeMap<String, V>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let position = |name: &str| {
        name.parse::<Button>()
            .ok()
            .and_then(|button| Button::ALL.iter().position(|b| *b == button))
            .unwrap_or(usize::MAX)
    };
    let mut entries: Vec<(&String, &V)> = buttons.iter().flatten().collect();
    entries.sort_by_key(|(name, _)| position(name));
    serializer.collect_map(entries)
}

impl Colors {
    fn is_empty(&self) -> bool {
        *self == Colors::default()
    }
}

/// The nearest of the mouse's 16 brightness steps to a percentage.
pub fn brightness_level(percent: u32) -> u8 {
    ((percent.min(100) * u32::from(MAX_BRIGHTNESS) + 50) / 100) as u8
}

pub const DEFAULT_LIGHTING_MODES: [&str; 4] = ["off", "reactive", "rainbow", "reactive-rainbow"];

/// A profile whose every value has been checked and converted.
struct Checked {
    dpi: Option<(Vec<u32>, usize)>,
    polling: Option<PollingRate>,
    sleep_minutes: Option<u32>,
    illumination: Option<Illumination>,
    high_efficiency: Option<bool>,
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
        Ok(toml::to_string(self)?)
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
        let saving = self.battery_saver == Some(true);
        // Brightness, smart lighting and the dim timer travel in one command,
        // so naming any of them sets all three. Turning the battery saver
        // off sends them too, to bring the lights back.
        let illumination = if saving {
            Some(Illumination {
                brightness: 0,
                smart: true,
                dim_seconds: 0,
            })
        } else if self.dim_seconds.is_some()
            || self.brightness.is_some()
            || self.smart_lighting.is_some()
            || self.battery_saver.is_some()
        {
            let usual = Illumination::default();
            let percent = self.brightness.unwrap_or(100);
            if percent > 100 {
                bail!("brightness must be 0-100 percent, got {percent}");
            }
            Some(Illumination {
                brightness: brightness_level(percent),
                smart: self.smart_lighting.unwrap_or(usual.smart),
                dim_seconds: self.dim_seconds.unwrap_or(usual.dim_seconds),
            })
        } else {
            None
        };
        if let Some(lights) = illumination {
            ggez_core::protocol::set_illumination(ggez_core::Link::Wired, lights)?;
        }
        let polling = if saving {
            Some(PollingRate::Hz125)
        } else {
            polling
        };
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
        let mut buttons = self
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
        // A macro button sends its trigger key from the mouse; the helper
        // does the rest. So macros mean a button map is sent, even when the
        // profile names no other binding.
        let plan = self.macro_plan()?;
        if !plan.is_empty() {
            let listed = |button: Button| {
                self.buttons
                    .iter()
                    .flatten()
                    .any(|(name, _)| name.parse::<Button>().ok() == Some(button))
            };
            let map = buttons.get_or_insert_with(ButtonMap::default);
            for button in Button::ALL {
                let Action::Keys(keys) = map.get(button) else {
                    continue;
                };
                let taken = plan
                    .macros
                    .iter()
                    .find(|(_, trigger, _)| keys.contains(trigger));
                if let Some((owner, trigger, _)) = taken {
                    bail!(
                        "{button} is bound to {}, which the macro on {owner} needs to tell ggez it is held. Use another key for {button}",
                        Action::key(*trigger)
                    );
                }
            }
            for (button, trigger, _) in &plan.macros {
                if listed(*button) {
                    bail!("{button} has both a binding under [buttons] and a macro; remove one");
                }
                map.set(*button, Action::key(*trigger));
            }
        }
        Ok(Checked {
            dpi,
            polling,
            sleep_minutes: self.sleep_minutes,
            illumination,
            high_efficiency: self.battery_saver,
            default_lighting,
            reactive,
            rainbow,
            zones,
            buttons,
        })
    }

    /// The macros in the profile, each with the key the mouse sends for it.
    pub fn macro_plan(&self) -> Result<Plan> {
        Plan::new(self.macros.as_ref())
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
        if let Some(on) = checked.high_efficiency {
            mouse.set_high_efficiency(on)?;
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

    /// Send only the lighting: zone colors and the reactive color, which the
    /// mouse forgets when it powers off, and brightness. Returns whether
    /// anything was sent.
    pub fn apply_volatile<T: Transport>(&self, mouse: &mut Aerox9<T>) -> Result<bool> {
        Self::send_lighting(&self.check()?, mouse)
    }

    /// Whether the profile has anything [`Profile::apply_volatile`] would send.
    pub fn has_volatile(&self) -> bool {
        self.reactive.is_some()
            || self.effect.is_some()
            || !self.colors.is_empty()
            || self.dim_seconds.is_some()
            || self.brightness.is_some()
            || self.smart_lighting.is_some()
            || self.battery_saver.is_some()
    }

    fn send_lighting<T: Transport>(checked: &Checked, mouse: &mut Aerox9<T>) -> Result<bool> {
        let mut sent = false;
        if let Some(lights) = checked.illumination {
            mouse.set_illumination(lights)?;
            sent = true;
        }
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
    fn the_three_light_settings_travel_in_one_command() {
        let sent = |text| commands(|p, m| p.apply(m), text);
        // Naming one fills in the usual values for the others.
        assert_eq!(
            sent("dim_seconds = 60"),
            [vec![0x00, 0x63, 0x0F, 1, 0, 0, 0x60, 0xEA, 0]]
        );
        assert_eq!(
            sent("brightness = 50\nsmart_lighting = true"),
            [vec![0x00, 0x63, 0x08, 1, 1, 0, 0x30, 0x75, 0]]
        );
        assert_eq!(brightness_level(0), 0);
        assert_eq!(brightness_level(100), 15);
        assert!(Profile::parse("brightness = 101")
            .unwrap()
            .validate()
            .is_err());
        // Brightness is lighting, so the helper restores it with the colors.
        assert_eq!(
            commands(|p, m| p.apply_volatile(m), "brightness = 100").len(),
            1
        );
    }

    #[test]
    fn battery_saver_overrides_polling_and_lights_and_gives_them_back() {
        let sent = |text| commands(|p, m| p.apply(m), text);
        // What SteelSeries GG sends for High-Efficiency Mode: 125 Hz, the
        // mouse's own flag, lights off.
        assert_eq!(
            sent("polling = 1000\nbrightness = 80\nbattery_saver = true"),
            [
                vec![0x00, 0x6B, 0x03],
                vec![0x00, 0x68, 1],
                vec![0x00, 0x63, 0, 1, 1, 0, 0, 0, 0]
            ]
        );
        assert_eq!(
            sent("polling = 1000\nbrightness = 80\nbattery_saver = false"),
            [
                vec![0x00, 0x6B, 0x00],
                vec![0x00, 0x68, 0],
                vec![0x00, 0x63, 0x0C, 1, 0, 0, 0x30, 0x75, 0]
            ]
        );
    }

    #[test]
    fn buttons_are_written_in_physical_order() {
        let text = "[buttons]
g10 = \"left\"
g2 = \"left\"
left = \"left\"
scroll-up = \"left\"
";
        let written = Profile::parse(text).unwrap().to_toml().unwrap();
        let order: Vec<&str> = written
            .lines()
            .filter_map(|l| l.split(' ').next())
            .collect();
        assert_eq!(order, ["[buttons]", "left", "g2", "g10", "scroll-up"]);
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

    #[test]
    fn a_macro_button_sends_its_trigger_key_from_the_mouse() {
        let profile = Profile::parse(
            r#"
            [buttons]
            g1 = "key:ctrl+z"

            [macros.g7]
            steps = ["key:ctrl+c", "wait:80", "key:ctrl+v"]

            [macros.g2]
            mode = "repeat"
            steps = ["click:left", "wait:40"]
            "#,
        )
        .unwrap();
        let plan = profile.macro_plan().unwrap();
        let triggers: Vec<(Button, u8)> =
            plan.macros.iter().map(|(b, key, _)| (*b, *key)).collect();
        assert_eq!(triggers, [(Button::Grid(2), 0x68), (Button::Grid(7), 0x69)]);

        let buttons = profile.check().unwrap().buttons.unwrap();
        assert_eq!(buttons.get(Button::Grid(2)), Action::key(0x68), "F13");
        assert_eq!(buttons.get(Button::Grid(7)), Action::key(0x69), "F14");
        assert_eq!(buttons.get(Button::Grid(1)), "key:ctrl+z".parse().unwrap());
        assert_eq!(
            buttons.get(Button::Left),
            ButtonMap::default().get(Button::Left)
        );
    }

    #[test]
    fn macros_alone_still_send_a_button_map() {
        let profile = Profile::parse("[macros.g3]\nsteps = [\"text:hello\"]").unwrap();
        let buttons = profile.check().unwrap().buttons.expect("a button map");
        assert_eq!(buttons.get(Button::Grid(3)), Action::key(0x68));
        // Without macros, and without [buttons], the mouse's map is left alone.
        assert!(Profile::parse("polling = 500")
            .unwrap()
            .check()
            .unwrap()
            .buttons
            .is_none());
    }

    #[test]
    fn macro_mistakes_are_refused_with_a_reason() {
        let fails = |text: &str, expected: &str| {
            let error = format!(
                "{:#}",
                Profile::parse(text).unwrap().validate().unwrap_err()
            );
            assert!(error.contains(expected), "{error:?} lacks {expected:?}");
        };
        fails(
            "[buttons]\ng3 = \"key:a\"\n[macros.g3]\nsteps = [\"key:b\"]",
            "both a binding under [buttons] and a macro",
        );
        fails(
            "[buttons]\ng1 = \"key:f13\"\n[macros.g3]\nsteps = [\"key:b\"]",
            "g1 is bound to key:f13",
        );
        fails("[macros.g3]\nsteps = [\"hop\"]", "the macro on g3");
        fails("[macros.thumb]\nsteps = [\"key:a\"]", "thumb");
        // F13 is free to bind when no macro needs it.
        Profile::parse("[buttons]\ng1 = \"key:f13\"")
            .unwrap()
            .validate()
            .unwrap();
    }

    #[test]
    fn macros_are_written_in_the_order_of_the_buttons() {
        let profile = Profile::parse(
            "[macros.g10]\nsteps = [\"key:a\"]\n[macros.g2]\nmode = \"toggle\"\nsteps = [\"key:b\"]",
        )
        .unwrap();
        let text = profile.to_toml().unwrap();
        assert!(
            text.find("[macros.g2]").unwrap() < text.find("[macros.g10]").unwrap(),
            "{text}"
        );
        assert_eq!(Profile::parse(&text).unwrap(), profile);
    }
}
