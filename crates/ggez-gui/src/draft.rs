//! The settings being edited in the window.
//!
//! A [`Profile`] leaves anything unspecified alone, which suits hand-written
//! files. The window shows every setting, so it edits a [`Draft`] in which
//! everything has a value, and converts to and from profiles at the edges.

use std::collections::BTreeMap;

use ggez_core::{Action, Button, ButtonMap, Rgb};
use ggez_service::macros::{MacroSpec, Mode};
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
    /// Brightness of the lights in percent.
    pub brightness: u32,
    /// Lights off while the mouse is moving.
    pub smart_lighting: bool,
    /// Lights off, 125 Hz and the mouse's own power-saving flag.
    pub battery_saver: bool,
    /// Index into [`DEFAULT_LIGHTING_MODES`].
    pub default_lighting: usize,
    pub effect: Effect,
    /// Front, palm, wrist.
    pub zones: [[u8; 3]; 3],
    pub reactive: Option<[u8; 3]>,
    pub buttons: ButtonMap,
    /// Buttons that run a macro, by the button's name. A button listed
    /// here does that instead of its binding in `buttons`, which is
    /// kept for when the macro is removed.
    pub macros: BTreeMap<String, MacroDraft>,
}

/// A macro as the window edits it: its steps are text, one step to a line,
/// and may be wrong while they are being typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroDraft {
    pub mode: Mode,
    pub steps: String,
}

impl Default for MacroDraft {
    /// Something that works, to start from.
    fn default() -> Self {
        Self {
            mode: Mode::Once,
            steps: "key:ctrl+c".to_string(),
        }
    }
}

impl MacroDraft {
    /// The steps as a profile lists them: every line that is not blank.
    pub fn lines(&self) -> Vec<String> {
        self.steps
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(str::to_string)
            .collect()
    }

    /// What is wrong with the steps, if anything, naming the line.
    pub fn problem(&self) -> Option<String> {
        let lines = self.lines();
        if lines.is_empty() {
            return Some("Add at least one step.".to_string());
        }
        lines.iter().enumerate().find_map(|(index, line)| {
            ggez_service::macros::check_step(line)
                .err()
                .map(|e| format!("Line {}: {e:#}", index + 1))
        })
    }
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
            brightness: 100,
            smart_lighting: false,
            battery_saver: false,
            default_lighting: 2,
            effect: Effect::Rainbow,
            zones: [[255, 80, 0], [255, 80, 0], [255, 80, 0]],
            reactive: None,
            buttons: ButtonMap::default(),
            macros: BTreeMap::new(),
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
        if let Some(percent) = profile.brightness {
            draft.brightness = percent.min(100);
        }
        draft.smart_lighting = profile.smart_lighting.unwrap_or(draft.smart_lighting);
        draft.battery_saver = profile.battery_saver.unwrap_or(draft.battery_saver);
        if let Some(mode) = profile
            .default_lighting
            .as_deref()
            .and_then(|m| DEFAULT_LIGHTING_MODES.iter().position(|known| *known == m))
        {
            draft.default_lighting = mode;
        }
        draft.reactive = profile.reactive.as_deref().and_then(parse_color);

        let colors = &profile.colors;
        let zone = |own: &Option<String>| {
            own.as_ref()
                .or(colors.all.as_ref())
                .and_then(|c| parse_color(c))
        };
        let zones = [
            zone(&colors.top),
            zone(&colors.middle),
            zone(&colors.bottom),
        ];
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
                if let (Ok(button), Ok(action)) =
                    (button.parse::<Button>(), action.parse::<Action>())
                {
                    draft.buttons.set(button, action);
                }
            }
        }
        for (name, spec) in profile.macros.iter().flatten() {
            let Ok(button) = name.parse::<Button>() else {
                continue;
            };
            let mode = Mode::ALL
                .into_iter()
                .find(|mode| Some(mode.name()) == spec.mode.as_deref())
                .unwrap_or_default();
            draft.macros.insert(
                button.to_string(),
                MacroDraft {
                    mode,
                    steps: spec.steps.join("\n"),
                },
            );
        }
        draft
    }

    /// Whether a button runs a macro.
    pub fn has_macro(&self, button: Button) -> bool {
        self.macros.contains_key(&button.to_string())
    }

    /// A profile that sets everything shown in the window.
    pub fn to_profile(&self) -> Profile {
        let factory = ButtonMap::default();
        // Only bindings that differ from the factory layout are written, so
        // the file stays short and readable.
        let buttons: BTreeMap<String, String> = Button::ALL
            .into_iter()
            .filter(|&b| self.buttons.get(b) != factory.get(b) && !self.has_macro(b))
            .map(|b| (b.to_string(), self.buttons.get(b).to_string()))
            .collect();
        let macros: BTreeMap<String, MacroSpec> = self
            .macros
            .iter()
            .map(|(name, entry)| {
                let spec = MacroSpec {
                    mode: (entry.mode != Mode::Once).then(|| entry.mode.name().to_string()),
                    steps: entry.lines(),
                };
                (name.clone(), spec)
            })
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
            brightness: Some(self.brightness),
            smart_lighting: Some(self.smart_lighting),
            battery_saver: Some(self.battery_saver),
            default_lighting: Some(DEFAULT_LIGHTING_MODES[self.default_lighting].to_string()),
            effect: Some(effect.to_string()),
            reactive: Some(self.reactive.map_or_else(|| "off".to_string(), color_text)),
            colors,
            // An empty table still means "write the factory layout".
            buttons: Some(buttons),
            macros: (!macros.is_empty()).then_some(macros),
        }
    }

    /// Whether some button still produces a left click. Without one the
    /// mouse cannot click anything, including this window.
    pub fn has_left_click(&self) -> bool {
        Button::ALL
            .into_iter()
            .any(|b| self.buttons.get(b) == Action::MouseButton(1) && !self.has_macro(b))
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
        let mut draft = Draft {
            dpi: vec![800, 1600],
            dpi_selected: 1,
            polling: 500,
            sleep_minutes: 0,
            dim_seconds: 600,
            brightness: 40,
            smart_lighting: true,
            battery_saver: true,
            default_lighting: 0,
            effect: Effect::Static,
            zones: [[1, 2, 3], [4, 5, 6], [7, 8, 9]],
            reactive: Some([255, 0, 0]),
            ..Draft::default()
        };
        draft
            .buttons
            .set(Button::Grid(12), "key:ctrl+v".parse().unwrap());
        draft.buttons.set(Button::TiltLeft, Action::Disabled);

        let text = draft.to_profile().to_toml().unwrap();
        let reread = Profile::parse(&text).unwrap();
        reread.validate().unwrap();
        assert_eq!(Draft::from_profile(&reread), draft);
        assert!(text.contains("g12 = \"key:ctrl+v\""), "{text}");
        assert!(
            !text.contains("g1 ="),
            "factory bindings are left out: {text}"
        );
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

    #[test]
    fn macros_survive_the_trip_to_a_profile_and_back() {
        let mut draft = Draft::default();
        draft
            .buttons
            .set(Button::Grid(4), "key:ctrl+z".parse().unwrap());
        draft.macros.insert(
            "g4".into(),
            MacroDraft {
                mode: Mode::Repeat,
                steps: "click:left\n\nwait:40\n".into(),
            },
        );
        draft.macros.insert("g9".into(), MacroDraft::default());
        let profile = draft.to_profile();
        profile.validate().unwrap();
        let macros = profile.macros.as_ref().unwrap();
        assert_eq!(macros["g4"].mode.as_deref(), Some("repeat"));
        assert_eq!(
            macros["g4"].steps,
            ["click:left", "wait:40"],
            "blank lines dropped"
        );
        assert_eq!(
            macros["g9"].mode, None,
            "once is the default and is left out"
        );
        // The binding under a macro is not written: the macro replaces it.
        assert!(!profile.buttons.as_ref().unwrap().contains_key("g4"));

        let back = Draft::from_profile(&profile);
        assert!(back.has_macro(Button::Grid(4)) && back.has_macro(Button::Grid(9)));
        assert_eq!(back.macros["g4"].mode, Mode::Repeat);
        assert_eq!(back.macros["g4"].steps, "click:left\nwait:40");
    }

    #[test]
    fn a_macro_being_typed_says_what_is_wrong_with_it() {
        let mut entry = MacroDraft::default();
        assert_eq!(entry.problem(), None);
        entry.steps = "key:ctrl+c\nhop".into();
        assert!(
            entry.problem().unwrap().starts_with("Line 2:"),
            "{:?}",
            entry.problem()
        );
        entry.steps = "  \n".into();
        assert_eq!(entry.problem().as_deref(), Some("Add at least one step."));
    }

    #[test]
    fn a_macro_on_the_only_left_click_button_counts_as_losing_left_click() {
        let mut draft = Draft::default();
        assert!(draft.has_left_click());
        draft.macros.insert("left".into(), MacroDraft::default());
        assert!(!draft.has_left_click());
    }
}
