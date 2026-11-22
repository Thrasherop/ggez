//! A profile is a TOML file describing how the mouse should be set up.
//! Every key is optional; anything left out is not touched, except buttons:
//! the mouse only accepts a full set, so unlisted buttons get their factory
//! binding whenever a `[buttons]` table is present.

use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use ggez_core::{Aerox9, Button, ButtonMap, DefaultLighting, PollingRate, Rgb, Transport, Zone};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// 1 to 5 DPI presets.
    dpi: Option<Vec<u32>>,
    /// Which preset is active, counting from 1.
    dpi_select: Option<usize>,
    /// Polling rate in Hz.
    polling: Option<u32>,
    sleep_minutes: Option<u32>,
    dim_seconds: Option<u32>,
    /// Lighting shown when the mouse powers on: off, reactive, rainbow or
    /// reactive-rainbow.
    default_lighting: Option<String>,
    /// Color flashed on click, or "off".
    reactive: Option<String>,
    #[serde(default)]
    colors: Colors,
    buttons: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Colors {
    all: Option<String>,
    top: Option<String>,
    middle: Option<String>,
    bottom: Option<String>,
}

impl Profile {
    pub fn parse(text: &str) -> Result<Self> {
        Ok(toml::from_str(text)?)
    }

    /// Send every setting in the profile. Nothing is sent if any value is
    /// invalid. Returns whether anything was sent.
    pub fn apply<T: Transport>(&self, mouse: &mut Aerox9<T>) -> Result<bool> {
        // Validate everything before touching the mouse.
        let polling = self
            .polling
            .map(|hz| PollingRate::from_hz(hz).with_context(|| format!("unsupported polling rate {hz}")))
            .transpose()?;
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
        let color = |own: &Option<String>| -> Result<Option<Rgb>> {
            Ok(own.as_ref().or(self.colors.all.as_ref()).map(|c| c.parse()).transpose()?)
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
        if self.dpi.is_none() && self.dpi_select.is_some() {
            bail!("dpi_select needs dpi");
        }
        let select = self.dpi_select.unwrap_or(1);
        if select == 0 {
            bail!("dpi_select counts from 1");
        }

        let mut sent = false;
        if let Some(presets) = &self.dpi {
            mouse.set_dpi(presets, select - 1)?;
            sent = true;
        }
        if let Some(rate) = polling {
            mouse.set_polling_rate(rate)?;
            sent = true;
        }
        if let Some(minutes) = self.sleep_minutes {
            mouse.set_sleep_timer(minutes)?;
            sent = true;
        }
        if let Some(seconds) = self.dim_seconds {
            mouse.set_dim_timer(seconds)?;
            sent = true;
        }
        if let Some(mode) = default_lighting {
            mouse.set_default_lighting(mode)?;
            sent = true;
        }
        if let Some(color) = reactive {
            mouse.set_reactive_color(color)?;
            sent = true;
        }
        for (zone, color) in zones {
            if let Some(color) = color {
                mouse.set_zone_color(zone, color)?;
                sent = true;
            }
        }
        if let Some(map) = &buttons {
            mouse.set_buttons(map)?;
            sent = true;
        }
        Ok(sent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ggez_core::{Link, MockTransport};

    fn apply(text: &str) -> Result<Vec<Vec<u8>>> {
        let mut mouse = Aerox9::new(MockTransport::accepting(), Link::Wireless);
        Profile::parse(text)?.apply(&mut mouse)?;
        Ok(mouse.transport().written.clone())
    }

    #[test]
    fn full_profile_sends_each_setting() {
        let packets = apply(
            r##"
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
            "##,
        )
        .unwrap();
        let commands: Vec<u8> = packets.iter().map(|p| p[1]).collect();
        assert_eq!(
            commands,
            [0x6D, 0x6B, 0x69, 0x67, 0x66, 0x61, 0x61, 0x61, 0x6A, 0x6A, 0x6A]
        );
        assert_eq!(packets[0][2..6], [2, 1, 0x09, 0x12]);
        assert_eq!(packets[7][2..7], [0x01, 0x02, 0, 0, 0xFF]);
    }

    #[test]
    fn empty_profile_sends_nothing() {
        assert!(apply("").unwrap().is_empty());
    }

    #[test]
    fn a_bad_value_sends_nothing() {
        let mut mouse = Aerox9::new(MockTransport::accepting(), Link::Wireless);
        let profile = Profile::parse("polling = 1000\n[buttons]\ng99 = \"left\"").unwrap();
        assert!(profile.apply(&mut mouse).is_err());
        assert!(mouse.transport().written.is_empty());
    }

    #[test]
    fn unknown_keys_are_rejected() {
        assert!(Profile::parse("poling = 1000").is_err());
    }
}
