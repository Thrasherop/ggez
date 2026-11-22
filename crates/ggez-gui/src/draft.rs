//! The settings being edited in the window.
//!
//! A [`Profile`] leaves anything unspecified alone, which suits hand-written
//! files. The window shows every setting, so it edits a [`Draft`] in which
//! everything has a value, and converts to and from profiles at the edges.

use std::collections::BTreeMap;

use ggez_core::{Action, Button, ButtonMap, Rgb};
use ggez_service::profile::{Colors, DEFAULT_LIGHTING_MODES};
use ggez_service::Profile;

pub const MAX_DPI_PRESETS: usize = 5;
pub const POLLING_RATES: [u32; 4] = [125, 250, 500, 1000];

/// What the LEDs show while the computer is driving them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// One color per zone, re-sent by the helper whenever the mouse wakes.
    Static,
    /// The mouse's built-in color cycle.
    Rainbow,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    pub dpi: Vec<u32>,
    /// Index into `dpi` of the active preset.
    pub dpi_selected: usize,
    pub polling: u32,
    pub sleep_minutes: u32,
    pub dim_seconds: u32,
    /// Index into [`DEFAULT_LIGHTING_MODES`].
    pub default_lighting: usize,
    pub effect: Effect,
    /// Front, palm, wrist.
    pub zones: [[u8; 3]; 3],
    pub reactive: Option<[u8; 3]>,
    pub buttons: ButtonMap,
}

impl Default for Draft {
    /// The mouse's factory settings.
    fn default() -> Self {
        Self {
            dpi: vec![400, 800, 1200, 2400, 3200],
            dpi_selected: 1,
            polling: 1000,
            sleep_minutes: 5,
            dim_seconds: 30,
            default_lighting: 2,
            effect: Effect::Rainbow,
            zones: [[255, 80, 0], [255, 80, 0], [255, 80, 0]],
            reactive: None,
            buttons: ButtonMap::default(),
        }
    }
}

fn color_text(c: [u8; 3]) -> String {
    Rgb(c[0], c[1], c[2]).to_string()
}

fn parse_color(text: &str) -> Option<[u8; 3]> {
    text.parse::<Rgb>().ok().map(|Rgb(r, g, b)| [r, g, b])
}

impl Draft {
    /// Start from the factory settings and overlay whatever the profile
    /// specifies. Values the profile gets wrong are ignored rather than
    /// refused: the window then shows, and can fix, the rest.
    pub fn from_profile(profile: &Profile) -> Self {
        let mut draft = Self::default();
        if let Some(dpi) = &profile.dpi {
            let presets: Vec<u32> = dpi
                .iter()
                .take(MAX_DPI_PRESETS)
                .map(|&d| snap_dpi(d))
                .collect();
            if !presets.is_empty() {
                draft.dpi_selected = profile
                    .dpi_select
                    .unwrap_or(1)
                    .saturating_sub(1)
                    .min(presets.len() - 1);
                draft.dpi = presets;
            }
        }
        if let Some(hz) = profile.polling.filter(|hz| POLLING_RATES.contains(hz)) {
            draft.polling = hz;
        }
        if let Some(minutes) = profile.sleep_minutes {
            draft.sleep_minutes = minutes.min(20);
        }
        if let Some(seconds) = profile.dim_seconds {
            draft.dim_seconds = seconds.min(1200);
        }
        if let Some(mode) = profile
            .default_lighting
            .as_deref()
            .and_then(|m| DEFAULT_LIGHTING_MODES.iter().position(|known| *known == m))
        {
            draft.default_lighting = mode;
        }
        draft.reactive = profile.reactive.as_deref().and_then(parse_color);

        let colors = &profile.colors;
        let zone = |own: &Option<String>| own.as_ref().or(colors.all.as_ref()).and_then(|c| parse_color(c));
        let zones = [zone(&colors.top), zone(&colors.middle), zone(&colors.bottom)];
        if zones.iter().any(Option::is_some) {
            draft.effect = Effect::Static;
        }
        for (slot, color) in draft.zones.iter_mut().zip(zones) {
            if let Some(color) = color {
                *slot = color;
            }
        }
        if profile.effect.as_deref() == Some("rainbow") {
            draft.effect = Effect::Rainbow;
        }

        if let Some(bindings) = &profile.buttons {
            for (button, action) in bindings {
                if let (Ok(button), Ok(action)) = (button.parse::<Button>(), action.parse::<Action>()) {
                    draft.buttons.set(button, action);
                }
            }
        }
        draft
    }

    /// A profile that sets everything shown in the window.
    pub fn to_profile(&self) -> Profile {
        let factory = ButtonMap::default();
        // Only bindings that differ from the factory layout are written, so
        // the file stays short and readable.
        let buttons: BTreeMap<String, String> = Button::ALL
            .into_iter()
            .filter(|&b| self.buttons.get(b) != factory.get(b))
            .map(|b| (b.to_string(), self.buttons.get(b).to_string()))
            .collect();
        let (effect, colors) = match self.effect {
            Effect::Rainbow => ("rainbow", Colors::default()),
            Effect::Static => (
                "static",
                Colors {
                    all: None,
                    top: Some(color_text(self.zones[0])),
                    middle: Some(color_text(self.zones[1])),
                    bottom: Some(color_text(self.zones[2])),
                },
            ),
        };
        Profile {
            dpi: Some(self.dpi.clone()),
            dpi_select: Some(self.dpi_selected + 1),
            polling: Some(self.polling),
            sleep_minutes: Some(self.sleep_minutes),
            dim_seconds: Some(self.dim_seconds),
            default_lighting: Some(DEFAULT_LIGHTING_MODES[self.default_lighting].to_string()),
            effect: Some(effect.to_string()),
            reactive: Some(self.reactive.map_or_else(|| "off".to_string(), color_text)),
            colors,
            // An empty table still means "write the factory layout".
            buttons: Some(buttons),
        }
    }

    /// Whether some button still produces a left click. Without one the
    /// mouse cannot click anything, including this window.
    pub fn has_left_click(&self) -> bool {
        Button::ALL
            .into_iter()
            .any(|b| self.buttons.get(b) == Action::MouseButton(1))
    }
}

/// Round to the nearest value the sensor supports.
pub fn snap_dpi(dpi: u32) -> u32 {
    ((dpi.clamp(100, 18_000) + 50) / 100) * 100
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_draft_round_trips_and_is_valid() {
        let draft = Draft::default();
        let profile = draft.to_profile();
        profile.validate().unwrap();
        assert_eq!(Draft::from_profile(&profile), draft);
        assert!(draft.has_left_click());
    }

    #[test]
    fn edited_draft_round_trips_through_the_file_format() {
        let mut draft = Draft::default();
        draft.dpi = vec![800, 1600];
        draft.dpi_selected = 1;
        draft.polling = 500;
        draft.sleep_minutes = 0;
        draft.dim_seconds = 600;
        draft.default_lighting = 0;
        draft.effect = Effect::Static;
        draft.zones = [[1, 2, 3], [4, 5, 6], [7, 8, 9]];
        draft.reactive = Some([255, 0, 0]);
        draft.buttons.set(Button::Grid(12), "key:ctrl+v".parse().unwrap());
        draft.buttons.set(Button::TiltLeft, Action::Disabled);

        let text = draft.to_profile().to_toml().unwrap();
        let reread = Profile::parse(&text).unwrap();
        reread.validate().unwrap();
        assert_eq!(Draft::from_profile(&reread), draft);
        assert!(text.contains("g12 = \"key:ctrl+v\""), "{text}");
        assert!(!text.contains("g1 ="), "factory bindings are left out: {text}");
    }

    #[test]
    fn hand_written_profile_fills_the_gaps_with_factory_settings() {
        let profile = Profile::parse("polling = 250\n[colors]\nall = \"blue\"\n").unwrap();
        let draft = Draft::from_profile(&profile);
        assert_eq!(draft.polling, 250);
        assert_eq!(draft.effect, Effect::Static);
        assert_eq!(draft.zones, [[0, 0, 255]; 3]);
        assert_eq!(draft.dpi, Draft::default().dpi);
    }

    #[test]
    fn out_of_range_values_are_clamped_not_fatal() {
        let profile = Profile::parse(
            "dpi = [50, 123456, 1234]\ndpi_select = 9\npolling = 7\nsleep_minutes = 99\n",
        )
        .unwrap();
        let draft = Draft::from_profile(&profile);
        assert_eq!(draft.dpi, [100, 18_000, 1200]);
        assert_eq!(draft.dpi_selected, 2);
        assert_eq!(draft.polling, 1000);
        assert_eq!(draft.sleep_minutes, 20);
        draft.to_profile().validate().unwrap();
    }

    #[test]
    fn losing_left_click_is_detected() {
        let mut draft = Draft::default();
        draft.buttons.set(Button::Left, Action::Disabled);
        assert!(!draft.has_left_click());
        draft.buttons.set(Button::Grid(1), Action::MouseButton(1));
        assert!(draft.has_left_click());
    }
}
