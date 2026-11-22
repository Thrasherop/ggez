//! The window.

use std::path::PathBuf;

use eframe::egui::{self, Color32, RichText};
use ggez_core::{key_names, media_names, Action, Button, ButtonMap, Link};
use ggez_service::profile::DEFAULT_LIGHTING_MODES;
use ggez_service::Profile;

use crate::device::{Device, Reply, Request, Status};
use crate::draft::{snap_dpi, Draft, Effect, MAX_DPI_PRESETS, POLLING_RATES};

const ACCENT: Color32 = Color32::from_rgb(45, 212, 191);
const WARNING: Color32 = Color32::from_rgb(250, 190, 80);
const DANGER: Color32 = Color32::from_rgb(248, 113, 113);
const GOOD: Color32 = Color32::from_rgb(74, 222, 128);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Sensitivity,
    Buttons,
    Lighting,
    Power,
    Settings,
}

impl Tab {
    const ALL: [(Tab, &'static str); 5] = [
        (Tab::Sensitivity, "Sensitivity"),
        (Tab::Buttons, "Buttons"),
        (Tab::Lighting, "Lighting"),
        (Tab::Power, "Power"),
        (Tab::Settings, "Settings"),
    ];
}

/// The one-line message in the bottom bar.
#[derive(Debug, Clone, PartialEq)]
enum Notice {
    None,
    Working(String),
    Good(String),
    Bad(String),
}

/// Things the window asks the surrounding program to do, so the window
/// itself stays free of file dialogs and registry access and can be tested.
pub trait Host {
    fn autostart_enabled(&self) -> Result<bool, String>;
    fn set_autostart(&mut self, enabled: bool) -> Result<(), String>;
    /// Ask where to save a profile; `None` if the user cancels.
    fn pick_save_path(&mut self) -> Option<PathBuf>;
    fn pick_open_path(&mut self) -> Option<PathBuf>;
    fn reveal(&mut self, path: &std::path::Path);
}

pub struct App {
    pub tab: Tab,
    /// What the window shows and edits.
    pub draft: Draft,
    /// What was last sent to the mouse (or loaded at start).
    pub applied: Draft,
    /// The draft as of the previous frame, to notice edits.
    seen: Draft,
    status: Status,
    notice: Notice,
    applying: bool,
    /// The last apply failed, so the mouse may not match `applied`.
    apply_failed: bool,
    selected_button: Button,
    /// Text box contents for the selected button's action.
    action_text: String,
    /// The next key press becomes the selected button's key.
    capturing_key: bool,
    confirm_no_left_click: bool,
    confirm_reset: bool,
    live_preview: bool,
    profile_path: Option<PathBuf>,
    autostart: Result<bool, String>,
    helper_running_here: bool,
    device: Box<dyn Device>,
    host: Box<dyn Host>,
}

impl App {
    pub fn new(
        device: Box<dyn Device>,
        host: Box<dyn Host>,
        profile_path: Option<PathBuf>,
        helper_running_here: bool,
    ) -> Self {
        let mut notice = Notice::None;
        let draft = match profile_path
            .as_deref()
            .filter(|p| p.exists())
            .map(Profile::load)
        {
            Some(Ok(profile)) => Draft::from_profile(&profile),
            Some(Err(e)) => {
                notice = Notice::Bad(format!("Could not read your saved profile: {e:#}"));
                Draft::default()
            }
            None => Draft::default(),
        };
        let autostart = host.autostart_enabled();
        let selected_button = Button::Grid(1);
        Self {
            tab: Tab::Sensitivity,
            action_text: draft.buttons.get(selected_button).to_string(),
            applied: draft.clone(),
            seen: draft.clone(),
            draft,
            status: Status::Unknown,
            notice,
            applying: false,
            apply_failed: false,
            selected_button,
            capturing_key: false,
            confirm_no_left_click: false,
            confirm_reset: false,
            live_preview: true,
            profile_path,
            autostart,
            helper_running_here,
            device,
            host,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.draft != self.applied || self.apply_failed
    }

    fn connected(&self) -> bool {
        matches!(self.status, Status::Connected { .. })
    }

    fn handle_replies(&mut self) {
        for reply in self.device.poll() {
            match reply {
                Reply::Status(status) => self.status = status,
                Reply::Applied(Ok(())) => {
                    self.applying = false;
                    self.apply_failed = false;
                    self.notice = Notice::Good("Applied and saved to the mouse.".into());
                }
                Reply::Applied(Err(e)) => {
                    self.applying = false;
                    self.apply_failed = true;
                    self.notice = Notice::Bad(format!("Could not apply: {e}"));
                }
            }
        }
    }

    /// Send everything to the mouse and remember it as the user's profile.
    fn apply(&mut self) {
        let profile = self.draft.to_profile();
        if let Err(e) = profile.validate() {
            self.notice = Notice::Bad(format!("These settings are not valid: {e:#}"));
            return;
        }
        if let Some(path) = &self.profile_path {
            if let Err(e) = profile.save(path) {
                self.notice = Notice::Bad(format!("{e:#}"));
                return;
            }
        }
        self.applied = self.draft.clone();
        self.applying = true;
        self.notice = Notice::Working("Applying…".into());
        self.device.send(Request::Apply(profile));
    }

    fn request_apply(&mut self) {
        if self.draft.has_left_click() {
            self.apply();
        } else {
            self.confirm_no_left_click = true;
        }
    }

    fn preview_lighting(&mut self) {
        if self.live_preview && self.connected() {
            self.device.send(Request::Preview(self.draft.to_profile()));
        }
    }

    fn select_button(&mut self, button: Button) {
        self.selected_button = button;
        self.action_text = self.draft.buttons.get(button).to_string();
        self.capturing_key = false;
    }

    fn set_action(&mut self, action: Action) {
        self.draft.buttons.set(self.selected_button, action);
        self.action_text = action.to_string();
    }

    pub fn ui(&mut self, ctx: &egui::Context) {
        self.handle_replies();
        self.capture_key(ctx);
        // A success message describes the settings as they were; once they
        // change it is stale, and the "not on the mouse yet" hint takes over.
        if self.draft != self.seen {
            if matches!(self.notice, Notice::Good(_)) {
                self.notice = Notice::None;
            }
            self.seen = self.draft.clone();
        }

        egui::SidePanel::left("sidebar")
            .exact_width(230.0)
            .resizable(false)
            .show(ctx, |ui| self.sidebar(ui));
        egui::TopBottomPanel::bottom("actions")
            .exact_height(56.0)
            .show(ctx, |ui| self.action_bar(ui));
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(8.0);
                    match self.tab {
                        Tab::Sensitivity => self.sensitivity(ui),
                        Tab::Buttons => self.buttons(ui),
                        Tab::Lighting => self.lighting(ui),
                        Tab::Power => self.power(ui),
                        Tab::Settings => self.settings(ui),
                    }
                    ui.add_space(16.0);
                });
        });
        self.dialogs(ctx);
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(14.0);
        ui.label(RichText::new("ggez").size(26.0).strong().color(ACCENT));
        ui.label(RichText::new("Aerox 9 Wireless").weak());
        ui.add_space(14.0);

        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let (dot, title, detail) = match &self.status {
                Status::Unknown => (WARNING, "Looking for your mouse…", String::new()),
                Status::NoDevice => (
                    DANGER,
                    "Not connected",
                    "Plug in the USB cable or the 2.4 GHz dongle.".to_string(),
                ),
                Status::Asleep => (
                    WARNING,
                    "Mouse is asleep or off",
                    "The dongle is plugged in. Move or switch on the mouse.".to_string(),
                ),
                Status::Connected { link, .. } => (
                    GOOD,
                    "Connected",
                    match link {
                        Link::Wired => "USB cable".to_string(),
                        Link::Wireless => "2.4 GHz dongle".to_string(),
                    },
                ),
            };
            ui.horizontal(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 5.0, dot);
                ui.label(RichText::new(title).strong());
            });
            if !detail.is_empty() {
                ui.label(RichText::new(detail).weak());
            }
            if let Status::Connected {
                battery, firmware, ..
            } = &self.status
            {
                ui.add_space(6.0);
                match battery {
                    Some((percent, charging)) => {
                        let text = if *charging {
                            format!("{percent}% · charging")
                        } else {
                            format!("{percent}%")
                        };
                        let color = if *percent <= 15 && !*charging {
                            DANGER
                        } else {
                            ACCENT
                        };
                        ui.add(
                            egui::ProgressBar::new(f32::from(*percent) / 100.0)
                                .text(text)
                                .fill(color.gamma_multiply(0.55)),
                        )
                        .on_hover_text("Battery level, in steps of 5%");
                    }
                    None => {
                        ui.label(RichText::new("Battery level unavailable").weak());
                    }
                }
                if let Some(version) = firmware {
                    ui.label(RichText::new(format!("Firmware {version}")).weak());
                }
            }
        });
        ui.add_space(14.0);

        for (tab, name) in Tab::ALL {
            let selected = self.tab == tab;
            let button = egui::Button::new(RichText::new(name).size(15.0))
                .selected(selected)
                .min_size(egui::vec2(ui.available_width(), 34.0));
            if ui.add(button).clicked() {
                self.tab = tab;
                self.capturing_key = false;
            }
        }

        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.add_space(8.0);
            ui.label(
                RichText::new(concat!("ggez ", env!("CARGO_PKG_VERSION")))
                    .weak()
                    .small(),
            );
        });
    }

    fn action_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            match &self.notice {
                Notice::None if self.is_dirty() => {
                    ui.label(
                        RichText::new("You have changes that are not on the mouse yet.")
                            .color(WARNING),
                    );
                }
                Notice::None => {}
                Notice::Working(text) => {
                    ui.spinner();
                    ui.label(text);
                }
                Notice::Good(text) => {
                    ui.label(RichText::new(text).color(GOOD));
                }
                Notice::Bad(text) => {
                    ui.label(RichText::new(text).color(DANGER));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let can_apply = self.connected() && !self.applying;
                let apply = egui::Button::new(
                    RichText::new("Apply to mouse")
                        .strong()
                        .color(Color32::BLACK),
                )
                .fill(ACCENT)
                .min_size(egui::vec2(140.0, 34.0));
                let response =
                    ui.add_enabled(can_apply, apply)
                        .on_disabled_hover_text(if self.applying {
                            "Still applying the previous changes."
                        } else {
                            "Connect the mouse first."
                        });
                if response.clicked() {
                    self.request_apply();
                }
                let revert = egui::Button::new("Revert").min_size(egui::vec2(80.0, 34.0));
                if ui
                    .add_enabled(self.draft != self.applied, revert)
                    .on_hover_text("Discard changes made since the last apply.")
                    .clicked()
                {
                    self.draft = self.applied.clone();
                    self.select_button(self.selected_button);
                    self.notice = Notice::None;
                    self.preview_lighting();
                }
            });
        });
    }

    fn sensitivity(&mut self, ui: &mut egui::Ui) {
        heading(
            ui,
            "Sensitivity",
            "Up to five CPI levels. The button behind the wheel cycles through them.",
        );
        let mut remove = None;
        let count = self.draft.dpi.len();
        for (index, dpi) in self.draft.dpi.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.radio_value(
                    &mut self.draft.dpi_selected,
                    index,
                    format!("Level {}", index + 1),
                )
                .on_hover_text("Make this the active level.");
                let mut value = *dpi;
                ui.spacing_mut().slider_width = 340.0;
                // Same width for every value box so the rows line up.
                ui.spacing_mut().interact_size.x = 96.0;
                let slider = egui::Slider::new(&mut value, 100..=18_000)
                    .logarithmic(true)
                    .suffix(" CPI")
                    .step_by(100.0);
                if ui.add(slider).changed() {
                    *dpi = snap_dpi(value);
                }
                if ui
                    .add_enabled(count > 1, egui::Button::new("Remove"))
                    .on_disabled_hover_text("At least one level is needed.")
                    .clicked()
                {
                    remove = Some(index);
                }
            });
        }
        if let Some(index) = remove {
            self.draft.dpi.remove(index);
            if self.draft.dpi_selected >= index && self.draft.dpi_selected > 0 {
                self.draft.dpi_selected -= 1;
            }
            self.draft.dpi_selected = self.draft.dpi_selected.min(self.draft.dpi.len() - 1);
        }
        ui.add_space(4.0);
        if ui
            .add_enabled(count < MAX_DPI_PRESETS, egui::Button::new("Add level"))
            .on_disabled_hover_text("The mouse stores at most five levels.")
            .clicked()
        {
            let last = *self.draft.dpi.last().unwrap_or(&800);
            self.draft.dpi.push(snap_dpi(last.saturating_mul(2)));
        }

        ui.add_space(22.0);
        heading(
            ui,
            "Polling rate",
            "How often the mouse reports its position. Higher is smoother and uses more battery.",
        );
        ui.horizontal(|ui| {
            for hz in POLLING_RATES {
                if choice(ui, self.draft.polling == hz, &format!("{hz} Hz")) {
                    self.draft.polling = hz;
                }
            }
        });
    }

    fn buttons(&mut self, ui: &mut egui::Ui) {
        heading(
            ui,
            "Buttons",
            "Bindings are stored on the mouse: they work on any computer, and a key stays held for as long as you hold the button.",
        );
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(390.0);
                ui.label(RichText::new("Top").weak());
                self.button_row(ui, &[Button::Left, Button::Middle, Button::Right]);
                self.button_row(ui, &[Button::TiltLeft, Button::Dpi, Button::TiltRight]);
                self.button_row(ui, &[Button::ScrollUp, Button::ScrollDown]);
                ui.add_space(10.0);
                ui.label(RichText::new("Side grid").weak());
                for row in 0..4u8 {
                    let buttons: Vec<Button> =
                        (1..=3).map(|col| Button::Grid(row * 3 + col)).collect();
                    self.button_row(ui, &buttons);
                }
                ui.add_space(10.0);
                if ui
                    .button("Reset all buttons")
                    .on_hover_text("Put every button back to its factory binding.")
                    .clicked()
                {
                    self.confirm_reset = true;
                }
            });
            ui.add_space(14.0);
            ui.vertical(|ui| self.button_editor(ui));
        });
    }

    fn button_row(&mut self, ui: &mut egui::Ui, buttons: &[Button]) {
        let factory = ButtonMap::default();
        ui.horizontal(|ui| {
            for &button in buttons {
                let action = self.draft.buttons.get(button);
                let changed = action != factory.get(button);
                let text = format!("{}\n{}", button_title(button), describe(action));
                let color = if changed {
                    ACCENT
                } else {
                    ui.visuals().text_color()
                };
                let widget = egui::Button::new(RichText::new(text).color(color))
                    .selected(self.selected_button == button)
                    .min_size(egui::vec2(122.0, 46.0));
                if ui.add(widget).clicked() {
                    self.select_button(button);
                }
            }
        });
    }

    fn button_editor(&mut self, ui: &mut egui::Ui) {
        let button = self.selected_button;
        let action = self.draft.buttons.get(button);
        ui.label(RichText::new(button_title(button)).size(18.0).strong());
        ui.label(RichText::new(format!("Currently: {}", describe(action))).weak());
        ui.add_space(10.0);

        let kind = Kind::of(action);
        ui.horizontal(|ui| {
            ui.label("Does");
            egui::ComboBox::from_id_salt("action-kind")
                .selected_text(kind.label())
                .width(220.0)
                .height(320.0)
                .show_ui(ui, |ui| {
                    for candidate in Kind::ALL {
                        if ui
                            .selectable_label(kind == candidate, candidate.label())
                            .clicked()
                            && kind != candidate
                        {
                            self.set_action(candidate.default_action());
                        }
                    }
                });
        });
        ui.add_space(8.0);

        match action {
            Action::Keys(codes) => self.keys_editor(ui, codes),
            Action::MouseButton(current) => {
                ui.horizontal_wrapped(|ui| {
                    for (number, name) in (1u8..).zip([
                        "Left click",
                        "Right click",
                        "Middle click",
                        "Back",
                        "Forward",
                    ]) {
                        if ui.selectable_label(current == number, name).clicked() {
                            self.set_action(Action::MouseButton(number));
                        }
                    }
                });
            }
            Action::Media(current) => {
                ui.horizontal_wrapped(|ui| {
                    for name in media_names() {
                        let candidate: Action =
                            format!("media:{name}").parse().expect("known media key");
                        if ui
                            .selectable_label(
                                candidate == Action::Media(current),
                                media_title(name),
                            )
                            .clicked()
                        {
                            self.set_action(candidate);
                        }
                    }
                });
            }
            Action::Disabled => {
                ui.label(RichText::new("The button does nothing.").weak());
            }
            Action::DpiCycle => {
                ui.label(RichText::new("Switches to the next CPI level.").weak());
            }
            Action::ScrollUp | Action::ScrollDown => {
                ui.label(RichText::new("Scrolls one step.").weak());
            }
        }

        ui.add_space(14.0);
        ui.collapsing("Edit as text", |ui| {
            ui.label(
                RichText::new("For anything the controls above cannot express, such as key:f13 or key:ctrl+alt+shift+k.")
                    .weak(),
            );
            let response = ui.add(egui::TextEdit::singleline(&mut self.action_text).desired_width(260.0));
            match self.action_text.parse::<Action>() {
                Ok(parsed) => {
                    if response.changed() {
                        self.draft.buttons.set(button, parsed);
                    }
                }
                Err(e) => {
                    ui.label(RichText::new(e.to_string()).color(DANGER));
                }
            }
        });

        ui.add_space(10.0);
        let factory = ButtonMap::default().get(button);
        if ui
            .add_enabled(action != factory, egui::Button::new("Reset this button"))
            .on_hover_text(format!("Factory binding: {}", describe(factory)))
            .clicked()
        {
            self.set_action(factory);
        }
    }

    /// Modifier toggles plus one key, which covers ordinary shortcuts. More
    /// unusual combinations go through "Edit as text".
    fn keys_editor(&mut self, ui: &mut egui::Ui, codes: [u8; 4]) {
        const MODIFIERS: [(&str, u8); 4] = [
            ("Ctrl", 0xE0),
            ("Shift", 0xE1),
            ("Alt", 0xE2),
            ("Win", 0xE3),
        ];
        let is_modifier = |code: u8| MODIFIERS.iter().any(|(_, m)| *m == code);
        let held: Vec<u8> = codes.iter().copied().filter(|&c| is_modifier(c)).collect();
        let others: Vec<u8> = codes
            .iter()
            .copied()
            .filter(|&c| c != 0 && !is_modifier(c))
            .collect();
        if others.len() > 1 {
            ui.label(
                RichText::new(
                    "This binding presses several keys at once. Use \"Edit as text\" to change it.",
                )
                .weak(),
            );
            return;
        }
        let key = others.first().copied();
        let rebuild = |held: &[u8], key: Option<u8>| {
            let mut codes = [0u8; 4];
            for (slot, code) in codes.iter_mut().zip(held.iter().copied().chain(key)) {
                *slot = code;
            }
            Action::Keys(codes)
        };

        ui.horizontal(|ui| {
            ui.label("Hold");
            for (name, code) in MODIFIERS {
                let turning_on = !held.contains(&code);
                // A binding holds at most four keys in total.
                let full = held.len() + usize::from(key.is_some()) >= 4;
                if choice(ui, !turning_on, name) && !(turning_on && full) {
                    let mut held = held.clone();
                    if turning_on {
                        held.push(code);
                        held.sort_unstable();
                    } else {
                        held.retain(|&c| c != code);
                    }
                    self.set_action(rebuild(&held, key));
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Key");
            let current = key.map(|code| Action::key(code).to_string()["key:".len()..].to_string());
            egui::ComboBox::from_id_salt("key-name")
                .selected_text(current.as_deref().map_or("(none)".to_string(), key_title))
                .width(150.0)
                .height(320.0)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(key.is_none(), "(none)").clicked() && !held.is_empty() {
                        self.set_action(rebuild(&held, None));
                    }
                    for name in key_names().into_iter().filter(|n| !is_modifier_name(n)) {
                        let selected = current.as_deref() == Some(name.as_str());
                        if ui.selectable_label(selected, key_title(&name)).clicked() {
                            let parsed: Action = format!("key:{name}").parse().expect("known key");
                            if let Action::Keys([code, ..]) = parsed {
                                self.set_action(rebuild(&held, Some(code)));
                            }
                        }
                    }
                });
            let label = if self.capturing_key {
                "Press a key…"
            } else {
                "Press to set"
            };
            if ui
                .add(egui::Button::new(label).selected(self.capturing_key))
                .on_hover_text("Click, then press the key (with any modifiers) on your keyboard.")
                .clicked()
            {
                self.capturing_key = !self.capturing_key;
            }
        });
        if held.is_empty() && key.is_none() {
            ui.label(RichText::new("Choose a key or a modifier.").color(WARNING));
        }
    }

    /// While "Press to set" is active, the next key press becomes the binding.
    fn capture_key(&mut self, ctx: &egui::Context) {
        if !self.capturing_key || self.tab != Tab::Buttons {
            return;
        }
        let pressed = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    repeat: false,
                    ..
                } => Some((*key, *modifiers)),
                // The windowing layer reports these three shortcuts as
                // clipboard events instead of key presses.
                egui::Event::Copy => Some((egui::Key::C, clipboard_modifier())),
                egui::Event::Cut => Some((egui::Key::X, clipboard_modifier())),
                egui::Event::Paste(_) => Some((egui::Key::V, clipboard_modifier())),
                _ => None,
            })
        });
        let Some((key, modifiers)) = pressed else {
            return;
        };
        let Some(name) = egui_key_name(key) else {
            return;
        };
        let mut text = String::from("key:");
        for (down, modifier) in [
            (modifiers.ctrl, "ctrl"),
            (modifiers.shift, "shift"),
            (modifiers.alt, "alt"),
            (modifiers.mac_cmd, "win"),
        ] {
            if down {
                text.push_str(modifier);
                text.push('+');
            }
        }
        text.push_str(&name);
        if let Ok(action) = text.parse::<Action>() {
            self.set_action(action);
        }
        self.capturing_key = false;
    }

    fn lighting(&mut self, ui: &mut egui::Ui) {
        heading(
            ui,
            "Lighting",
            "What the three light zones show while this computer is driving them.",
        );
        let mut changed = false;
        ui.horizontal(|ui| {
            for (effect, name) in [
                (Effect::Static, "Static colors"),
                (Effect::Rainbow, "Rainbow"),
            ] {
                if choice(ui, self.draft.effect == effect, name) && self.draft.effect != effect {
                    self.draft.effect = effect;
                    changed = true;
                }
            }
        });
        ui.add_space(8.0);
        match self.draft.effect {
            Effect::Static => {
                egui::Grid::new("zones")
                    .num_columns(2)
                    .spacing([16.0, 10.0])
                    .show(ui, |ui| {
                        for (index, name) in ["Front", "Palm", "Wrist"].into_iter().enumerate() {
                            ui.label(name);
                            changed |= ui
                                .color_edit_button_srgb(&mut self.draft.zones[index])
                                .changed();
                            ui.end_row();
                        }
                    });
                ui.add_space(6.0);
                if ui.button("Use the front color everywhere").clicked() {
                    self.draft.zones = [self.draft.zones[0]; 3];
                    changed = true;
                }
                ui.add_space(8.0);
                note(
                    ui,
                    "The mouse does not remember static colors. ggez sends them again whenever the mouse connects or wakes, so they need ggez running (see Settings).",
                );
            }
            Effect::Rainbow => {
                ui.label(RichText::new("The mouse cycles through colors by itself.").weak());
            }
        }

        ui.add_space(22.0);
        heading(ui, "Click flash", "Flash a color every time you click.");
        let mut flash = self.draft.reactive.is_some();
        ui.horizontal(|ui| {
            if ui.checkbox(&mut flash, "Flash on click").changed() {
                self.draft.reactive = flash.then_some([255, 255, 255]);
                changed = true;
            }
            if let Some(color) = &mut self.draft.reactive {
                changed |= ui.color_edit_button_srgb(color).changed();
            }
        });

        ui.add_space(22.0);
        heading(
            ui,
            "Without ggez",
            "What the mouse shows on its own, from power-on until a computer running ggez takes over. This is stored on the mouse.",
        );
        egui::ComboBox::from_id_salt("default-lighting")
            .selected_text(default_lighting_title(self.draft.default_lighting))
            .width(260.0)
            .show_ui(ui, |ui| {
                for index in 0..DEFAULT_LIGHTING_MODES.len() {
                    ui.selectable_value(
                        &mut self.draft.default_lighting,
                        index,
                        default_lighting_title(index),
                    );
                }
            });

        ui.add_space(22.0);
        ui.checkbox(
            &mut self.live_preview,
            "Show lighting changes on the mouse as I make them",
        );
        if changed {
            self.preview_lighting();
        }
    }

    fn power(&mut self, ui: &mut egui::Ui) {
        heading(
            ui,
            "Sleep",
            "How long the mouse waits, untouched, before it goes to sleep to save battery.",
        );
        ui.spacing_mut().slider_width = 340.0;
        ui.add(
            egui::Slider::new(&mut self.draft.sleep_minutes, 0..=20).custom_formatter(|v, _| {
                if v == 0.0 {
                    "Never".to_string()
                } else {
                    format!("{v} min")
                }
            }),
        );
        if self.draft.sleep_minutes == 0 {
            note(ui, "With sleep off, the mouse stays awake until you switch it off, which drains the battery.");
        }

        ui.add_space(22.0);
        heading(
            ui,
            "Dim the lights",
            "How long the mouse waits, untouched, before it dims its lights.",
        );
        ui.add(
            egui::Slider::new(&mut self.draft.dim_seconds, 0..=1200)
                .logarithmic(true)
                .custom_formatter(|v, _| match v as u32 {
                    0 => "Never".to_string(),
                    s if s < 60 => format!("{s} s"),
                    s if s % 60 == 0 => format!("{} min", s / 60),
                    s => format!("{} min {} s", s / 60, s % 60),
                }),
        );
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        heading(
            ui,
            "Keep my settings applied",
            "A small helper re-sends your colors whenever the mouse connects or wakes. It does nothing else and is not involved in mouse movement or clicks.",
        );
        match self.autostart.clone() {
            Ok(mut enabled) => {
                if ui
                    .checkbox(&mut enabled, "Start the helper when I log in")
                    .changed()
                {
                    self.autostart = self.host.set_autostart(enabled).map(|()| enabled);
                }
            }
            Err(e) => {
                ui.label(
                    RichText::new(format!("Could not check the login setting: {e}")).color(DANGER),
                );
                if ui.button("Try again").clicked() {
                    self.autostart = self.host.autostart_enabled();
                }
            }
        }
        ui.label(
            RichText::new(if self.helper_running_here {
                "The helper is running inside this window for now."
            } else {
                "The helper is already running in the background."
            })
            .weak(),
        );

        ui.add_space(22.0);
        heading(
            ui,
            "Profile file",
            "Your settings are kept in a plain text file that you can copy to another computer.",
        );
        if let Some(path) = self.profile_path.clone() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(path.display().to_string()).monospace());
                if ui.button("Show in folder").clicked() {
                    self.host.reveal(&path);
                }
            });
        }
        ui.horizontal(|ui| {
            if ui.button("Export…").on_hover_text("Save the settings shown in this window to a file.").clicked() {
                if let Some(path) = self.host.pick_save_path() {
                    self.notice = match self.draft.to_profile().save(&path) {
                        Ok(()) => Notice::Good(format!("Exported to {}", path.display())),
                        Err(e) => Notice::Bad(format!("{e:#}")),
                    };
                }
            }
            if ui
                .button("Import…")
                .on_hover_text("Load settings from a file into this window. Nothing is sent until you apply.")
                .clicked()
            {
                if let Some(path) = self.host.pick_open_path() {
                    match Profile::load(&path).and_then(|p| p.validate().map(|()| p)) {
                        Ok(profile) => {
                            self.draft = Draft::from_profile(&profile);
                            self.select_button(self.selected_button);
                            self.notice = Notice::None;
                        }
                        Err(e) => self.notice = Notice::Bad(format!("{e:#}")),
                    }
                }
            }
            if ui
                .button("Factory settings")
                .on_hover_text("Load the mouse's factory settings into this window. Nothing is sent until you apply.")
                .clicked()
            {
                self.draft = Draft::default();
                self.select_button(self.selected_button);
                self.notice = Notice::None;
            }
        });

        ui.add_space(22.0);
        heading(ui, "SteelSeries GG", "");
        note(
            ui,
            "Quit SteelSeries GG while you use ggez. GG keeps sending its own settings to the mouse, which overrides yours and makes commands fail.",
        );

        ui.add_space(22.0);
        heading(ui, "About", "");
        ui.label(concat!(
            "ggez ",
            env!("CARGO_PKG_VERSION"),
            " · open source under the MIT license"
        ));
        ui.hyperlink_to(
            "github.com/Thrasherop/ggez",
            "https://github.com/Thrasherop/ggez",
        );
        ui.label(RichText::new("Not made by or affiliated with SteelSeries.").weak());
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if self.confirm_no_left_click {
            egui::Modal::new(egui::Id::new("no-left-click")).show(ctx, |ui| {
                ui.set_width(380.0);
                ui.label(RichText::new("No button does a left click").size(17.0).strong());
                ui.add_space(6.0);
                ui.label("With these bindings nothing on the mouse will left-click, so you would need another mouse or the keyboard to undo it.");
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Go back").clicked() {
                        self.confirm_no_left_click = false;
                    }
                    if ui.button(RichText::new("Apply anyway").color(DANGER)).clicked() {
                        self.confirm_no_left_click = false;
                        self.apply();
                    }
                });
            });
        }
        if self.confirm_reset {
            egui::Modal::new(egui::Id::new("reset-buttons")).show(ctx, |ui| {
                ui.set_width(340.0);
                ui.label(RichText::new("Reset all buttons?").size(17.0).strong());
                ui.add_space(6.0);
                ui.label("Every button goes back to its factory binding in this window. Nothing is sent until you apply.");
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        self.confirm_reset = false;
                    }
                    if ui.button("Reset").clicked() {
                        self.confirm_reset = false;
                        self.draft.buttons = ButtonMap::default();
                        self.select_button(self.selected_button);
                    }
                });
            });
        }
    }
}

/// The broad kinds of action offered in the "Does" menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Keyboard,
    Mouse,
    Media,
    DpiCycle,
    ScrollUp,
    ScrollDown,
    Disabled,
}

impl Kind {
    const ALL: [Kind; 7] = [
        Kind::Keyboard,
        Kind::Mouse,
        Kind::Media,
        Kind::DpiCycle,
        Kind::ScrollUp,
        Kind::ScrollDown,
        Kind::Disabled,
    ];

    fn of(action: Action) -> Self {
        match action {
            Action::Keys(_) => Kind::Keyboard,
            Action::MouseButton(_) => Kind::Mouse,
            Action::Media(_) => Kind::Media,
            Action::DpiCycle => Kind::DpiCycle,
            Action::ScrollUp => Kind::ScrollUp,
            Action::ScrollDown => Kind::ScrollDown,
            Action::Disabled => Kind::Disabled,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Kind::Keyboard => "Keyboard key or shortcut",
            Kind::Mouse => "Mouse button",
            Kind::Media => "Media key",
            Kind::DpiCycle => "Next CPI level",
            Kind::ScrollUp => "Scroll up",
            Kind::ScrollDown => "Scroll down",
            Kind::Disabled => "Nothing (disabled)",
        }
    }

    fn default_action(self) -> Action {
        match self {
            Kind::Keyboard => Action::key(0x04),
            Kind::Mouse => Action::MouseButton(1),
            Kind::Media => Action::Media(0xCD),
            Kind::DpiCycle => Action::DpiCycle,
            Kind::ScrollUp => Action::ScrollUp,
            Kind::ScrollDown => Action::ScrollDown,
            Kind::Disabled => Action::Disabled,
        }
    }
}

/// One option in a row of mutually visible choices. Returns whether it was
/// clicked.
fn choice(ui: &mut egui::Ui, selected: bool, text: &str) -> bool {
    ui.add(
        egui::Button::new(text)
            .selected(selected)
            .min_size(egui::vec2(64.0, 28.0)),
    )
    .clicked()
}

fn heading(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.label(RichText::new(title).size(20.0).strong());
    if !subtitle.is_empty() {
        ui.label(RichText::new(subtitle).weak());
    }
    ui.add_space(10.0);
}

fn note(ui: &mut egui::Ui, text: &str) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_max_width(560.0);
        ui.label(RichText::new(text).color(WARNING));
    });
}

fn button_title(button: Button) -> String {
    match button {
        Button::Left => "Left button".into(),
        Button::Right => "Right button".into(),
        Button::Middle => "Wheel click".into(),
        Button::TiltLeft => "Wheel tilt left".into(),
        Button::TiltRight => "Wheel tilt right".into(),
        Button::Dpi => "CPI button".into(),
        Button::Grid(n) => format!("Side button {n}"),
        Button::ScrollUp => "Scroll up".into(),
        Button::ScrollDown => "Scroll down".into(),
    }
}

fn is_modifier_name(name: &str) -> bool {
    matches!(
        name,
        "ctrl" | "shift" | "alt" | "win" | "rctrl" | "rshift" | "ralt" | "rwin"
    )
}

/// A key name as shown to the user: `page-up` becomes `Page Up`.
fn key_title(name: &str) -> String {
    match name {
        "ctrl" => "Ctrl".into(),
        "rctrl" => "Right Ctrl".into(),
        "rshift" => "Right Shift".into(),
        "ralt" => "Right Alt".into(),
        "rwin" => "Right Win".into(),
        "win" => "Win".into(),
        "minus" => "-".into(),
        "equals" => "=".into(),
        "escape" => "Esc".into(),
        name if name.len() == 1 || (name.starts_with('f') && name[1..].parse::<u8>().is_ok()) => {
            name.to_ascii_uppercase()
        }
        name => name
            .split('-')
            .map(|word| {
                let mut chars = word.chars();
                chars.next().map_or(String::new(), |first| {
                    first.to_ascii_uppercase().to_string() + chars.as_str()
                })
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

fn media_title(name: &str) -> &'static str {
    match name {
        "play-pause" => "Play / Pause",
        "next" => "Next track",
        "previous" => "Previous track",
        "stop" => "Stop",
        "mute" => "Mute",
        "volume-up" => "Volume up",
        "volume-down" => "Volume down",
        _ => "Media key",
    }
}

fn default_lighting_title(index: usize) -> &'static str {
    [
        "Lights off",
        "Dark, flash on click",
        "Rainbow",
        "Rainbow, flash on click",
    ][index]
}

/// A short human description of an action: `Ctrl + V`, `Left click`.
pub fn describe(action: Action) -> String {
    match action {
        Action::Disabled => "Disabled".into(),
        Action::MouseButton(1) => "Left click".into(),
        Action::MouseButton(2) => "Right click".into(),
        Action::MouseButton(3) => "Middle click".into(),
        Action::MouseButton(4) => "Back".into(),
        Action::MouseButton(_) => "Forward".into(),
        Action::DpiCycle => "Next CPI level".into(),
        Action::ScrollUp => "Scroll up".into(),
        Action::ScrollDown => "Scroll down".into(),
        Action::Keys(_) => {
            let text = action.to_string();
            let names: Vec<String> = text["key:".len()..]
                .split('+')
                .filter(|n| !n.is_empty())
                .map(key_title)
                .collect();
            if names.is_empty() {
                "No key".into()
            } else {
                names.join(" + ")
            }
        }
        Action::Media(_) => {
            let text = action.to_string();
            media_title(&text["media:".len()..]).into()
        }
    }
}

/// The modifier that makes C, X and V into copy, cut and paste here.
fn clipboard_modifier() -> egui::Modifiers {
    if cfg!(target_os = "macos") {
        egui::Modifiers::MAC_CMD
    } else {
        egui::Modifiers::CTRL
    }
}

/// The ggez name of a key reported by the windowing system.
fn egui_key_name(key: egui::Key) -> Option<String> {
    use egui::Key;
    Some(
        match key {
            Key::Enter => "enter",
            Key::Escape => "escape",
            Key::Backspace => "backspace",
            Key::Tab => "tab",
            Key::Space => "space",
            Key::Minus => "minus",
            Key::Equals => "equals",
            Key::Insert => "insert",
            Key::Home => "home",
            Key::PageUp => "page-up",
            Key::Delete => "delete",
            Key::End => "end",
            Key::PageDown => "page-down",
            Key::ArrowRight => "right-arrow",
            Key::ArrowLeft => "left-arrow",
            Key::ArrowDown => "down-arrow",
            Key::ArrowUp => "up-arrow",
            other => {
                // Letters, digits and F-keys: egui names them "A", "5", "F7".
                let name = other.name().to_ascii_lowercase();
                let known = key_names().contains(&name);
                return known.then_some(name);
            }
        }
        .to_string(),
    )
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
