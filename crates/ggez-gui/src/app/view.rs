//! What the window draws. State and decisions live in the parent module.

use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, CornerRadius, FontId, Layout, Rect, RichText, Sense,
    Stroke, StrokeKind, Ui, WidgetInfo, WidgetType,
};
use ggez_core::{key_names, media_names, Action, Button, ButtonMap, Link};
use ggez_service::profile::DEFAULT_LIGHTING_MODES;
use ggez_service::Profile;

use super::{App, Notice, Tab};
use crate::device::Status;
use crate::draft::{snap_dpi, Draft, Effect, MAX_DPI_PRESETS, POLLING_RATES};
use crate::mouse_view::{self, Lights};
use crate::theme::{
    self, ACCENT, ACCENT_DEEP, BG, CARD, DANGER, GOOD, LINE, RAISED, SIDEBAR, TEXT, TEXT_DIM,
    TEXT_FAINT, WARNING,
};

const FIRST_RUN: &str = "These are the mouse's factory settings, not what is on your mouse now: the mouse cannot report its settings. Applying replaces everything on it, button bindings included.";

impl App {
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
            .exact_width(236.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(SIDEBAR)
                    .inner_margin(egui::Margin::same(16)),
            )
            .show(ctx, |ui| self.sidebar(ui));
        egui::TopBottomPanel::bottom("actions")
            .exact_height(66.0)
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(BG)
                    .inner_margin(egui::Margin::symmetric(28, 0)),
            )
            .show(ctx, |ui| self.action_bar(ui));
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(BG)
                    .inner_margin(egui::Margin::symmetric(28, 0)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_max_width(theme::PAGE_WIDTH);
                        ui.add_space(16.0);
                        self.banners(ui);
                        match self.tab {
                            Tab::Sensitivity => self.sensitivity(ui),
                            Tab::Buttons => self.buttons(ui),
                            Tab::Lighting => self.lighting(ui),
                            Tab::Power => self.power(ui),
                            Tab::Settings => self.settings(ui),
                        }
                        ui.add_space(20.0);
                    });
            });
        self.dialogs(ctx);
    }

    fn sidebar(&mut self, ui: &mut Ui) {
        // The device card keeps to the bottom whatever the window height.
        egui::TopBottomPanel::bottom("sidebar-foot")
            .frame(egui::Frame::NONE)
            .show_separator_line(false)
            .show_inside(ui, |ui| {
                self.device_card(ui);
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(concat!("Version ", env!("CARGO_PKG_VERSION")))
                            .color(TEXT_FAINT)
                            .size(11.5),
                    );
                });
            });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            ui.label(
                RichText::new("ggez")
                    .font(FontId::new(28.0, theme::strong()))
                    .color(ACCENT),
            );
        });
        ui.add_space(18.0);

        for (tab, name) in Tab::ALL {
            if nav_item(ui, self.tab == tab, tab, name) {
                self.tab = tab;
                self.capturing_key = false;
            }
        }
    }

    /// Which mouse this is and how it is doing.
    fn device_card(&mut self, ui: &mut Ui) {
        let (color, title, detail) = match &self.status {
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
        egui::Frame::new()
            .fill(CARD)
            .stroke(Stroke::new(1.0, LINE))
            .corner_radius(CornerRadius::same(theme::CARD_RADIUS))
            .inner_margin(egui::Margin::same(14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                {
                    ui.spacing_mut().item_spacing.y = 5.0;
                    ui.label(
                        RichText::new("Aerox 9 Wireless").font(FontId::new(14.5, theme::strong())),
                    );
                    ui.horizontal(|ui| {
                        theme::dot(ui, color);
                        ui.label(RichText::new(title).size(13.5));
                    });
                    if !detail.is_empty() {
                        ui.label(RichText::new(detail).color(TEXT_DIM).size(12.5));
                    }
                    if let Status::Connected {
                        battery, firmware, ..
                    } = &self.status
                    {
                        ui.add_space(4.0);
                        match battery {
                            Some((percent, charging)) => battery_bar(ui, *percent, *charging),
                            None => {
                                ui.label(
                                    RichText::new("Battery level unavailable")
                                        .color(TEXT_DIM)
                                        .size(12.5),
                                );
                            }
                        }
                        if let Some(version) = firmware {
                            ui.label(
                                RichText::new(format!("Firmware {version}"))
                                    .color(TEXT_FAINT)
                                    .size(12.0),
                            );
                        }
                    }
                }
            });
    }

    fn action_bar(&mut self, ui: &mut Ui) {
        let line_y = ui.max_rect().top();
        ui.painter()
            .hline(ui.max_rect().x_range(), line_y, Stroke::new(1.0, LINE));
        ui.horizontal_centered(|ui| {
            match &self.notice {
                Notice::None if self.is_dirty() => {
                    theme::dot(ui, WARNING);
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
                    theme::dot(ui, GOOD);
                    ui.label(RichText::new(text).color(GOOD));
                }
                Notice::Bad(text) => {
                    theme::dot(ui, DANGER);
                    ui.label(RichText::new(text).color(DANGER));
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let can_apply = self.connected() && !self.applying;
                let apply = theme::primary_button(ui, can_apply, "Apply to mouse");
                let apply = if can_apply {
                    apply
                } else {
                    apply.on_hover_text(if self.applying {
                        "Still applying the previous changes."
                    } else {
                        "Connect the mouse first."
                    })
                };
                if can_apply && apply.clicked() {
                    self.request_apply();
                }
                let revert = egui::Button::new("Revert").min_size(vec2(84.0, 38.0));
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

    /// Things the user should know whatever page they are on.
    fn banners(&mut self, ui: &mut Ui) {
        if self.gg_running {
            theme::callout(ui, WARNING, ggez_service::gg::ADVICE);
            ui.add_space(6.0);
        }
        if self.first_run {
            egui::Frame::new()
                .fill(ACCENT.gamma_multiply(0.07))
                .stroke(Stroke::new(1.0, ACCENT.gamma_multiply(0.4)))
                .corner_radius(CornerRadius::same(theme::CONTROL_RADIUS))
                .inner_margin(egui::Margin::symmetric(14, 12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(FIRST_RUN).size(13.5));
                    if theme::button(ui, "Got it").clicked() {
                        self.first_run = false;
                    }
                });
            ui.add_space(6.0);
        }
    }

    fn sensitivity(&mut self, ui: &mut Ui) {
        theme::page_title(
            ui,
            "Sensitivity",
            "How far the pointer travels when you move the mouse.",
        );
        theme::card(ui, |ui| {
            theme::card_title(
                ui,
                "CPI levels",
                "The mouse holds up to five. The button behind the wheel steps through them.",
            );
            let mut remove = None;
            let count = self.draft.dpi.len();
            for index in 0..count {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 14.0;
                    let active = self.draft.dpi_selected == index;
                    if level_marker(ui, active, index + 1) {
                        self.draft.dpi_selected = index;
                    }
                    let mut value = self.draft.dpi[index];
                    let slider = egui::Slider::new(&mut value, 100..=18_000)
                        .logarithmic(true)
                        .show_value(false)
                        .step_by(100.0);
                    ui.add(slider);
                    ui.add_sized(
                        [96.0, 32.0],
                        egui::DragValue::new(&mut value)
                            .range(100..=18_000)
                            .speed(20)
                            .suffix(" CPI"),
                    )
                    .on_hover_text("Drag, or double-click to type a value.");
                    self.draft.dpi[index] = snap_dpi(value);
                    if ui
                        .add_enabled(
                            count > 1,
                            egui::Button::new(RichText::new("Remove").color(TEXT_DIM).size(13.0))
                                .frame(false),
                        )
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
            ui.add_space(2.0);
            if ui
                .add_enabled(
                    count < MAX_DPI_PRESETS,
                    egui::Button::new("Add level").min_size(vec2(0.0, 34.0)),
                )
                .on_disabled_hover_text("The mouse stores at most five levels.")
                .clicked()
            {
                let last = *self.draft.dpi.last().unwrap_or(&800);
                self.draft.dpi.push(snap_dpi(last.saturating_mul(2)));
            }
        });

        ui.add_space(6.0);
        theme::card(ui, |ui| {
            theme::card_title(
                ui,
                "Polling rate",
                "How often the mouse reports its position. Higher is smoother and uses more battery.",
            );
            let options: Vec<(u32, String)> = POLLING_RATES
                .iter()
                .map(|&hz| (hz, format!("{hz} Hz")))
                .collect();
            let options: Vec<(u32, &str)> =
                options.iter().map(|(hz, s)| (*hz, s.as_str())).collect();
            if let Some(hz) = theme::segmented(ui, self.draft.polling, &options) {
                self.draft.polling = hz;
            }
        });
    }

    fn buttons(&mut self, ui: &mut Ui) {
        theme::page_title(
            ui,
            "Buttons",
            "Stored on the mouse: bindings work on any computer, and a key stays held for as long as you hold the button.",
        );
        theme::card(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 22.0;
                let clicked = mouse_view::buttons(
                    ui,
                    &self.draft.buttons,
                    self.selected_button,
                    &describe,
                    &button_title,
                );
                if let Some(button) = clicked {
                    self.select_button(button);
                }
                ui.vertical(|ui| self.button_editor(ui));
            });
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if theme::button(ui, "Reset all buttons")
                .on_hover_text("Put every button back to its factory binding.")
                .clicked()
            {
                self.confirm_reset = true;
            }
            let changed = Button::ALL
                .into_iter()
                .filter(|&b| self.draft.buttons.get(b) != ButtonMap::default().get(b))
                .count();
            if changed > 0 {
                ui.label(
                    RichText::new(format!(
                        "{changed} of 20 differ from the factory layout, shown in teal."
                    ))
                    .color(TEXT_DIM)
                    .size(13.0),
                );
            }
        });
    }

    fn button_editor(&mut self, ui: &mut Ui) {
        let button = self.selected_button;
        let action = self.draft.buttons.get(button);
        ui.label(RichText::new(button_title(button)).font(FontId::new(18.0, theme::strong())));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            action_keycaps(ui, action);
        });
        ui.add_space(8.0);

        let kind = Kind::of(action);
        ui.label(RichText::new("What it does").color(TEXT_DIM).size(12.5));
        egui::ComboBox::from_id_salt("action-kind")
            .selected_text(kind.label())
            .width(ui.available_width().min(250.0))
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
        ui.add_space(4.0);

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
                        if choice(ui, current == number, name) {
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
                        if choice(ui, candidate == Action::Media(current), media_title(name)) {
                            self.set_action(candidate);
                        }
                    }
                });
            }
            Action::Disabled => hint(ui, "The button does nothing."),
            Action::DpiCycle => hint(ui, "Switches to the next CPI level."),
            Action::ScrollUp | Action::ScrollDown => hint(ui, "Scrolls one step."),
        }

        ui.add_space(6.0);
        ui.collapsing("Edit as text", |ui| {
            hint(
                ui,
                "For anything the controls cannot express, such as key:f13 or key:ctrl+alt+shift+k.",
            );
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.action_text)
                    .desired_width(ui.available_width().min(240.0)),
            );
            match self.action_text.parse::<Action>() {
                Ok(parsed) => {
                    if response.changed() {
                        self.draft.buttons.set(button, parsed);
                    }
                }
                Err(e) => {
                    ui.label(RichText::new(e.to_string()).color(DANGER).size(12.5));
                }
            }
        });

        let factory = ButtonMap::default().get(button);
        if ui
            .add_enabled(
                action != factory,
                egui::Button::new("Reset this button").min_size(vec2(0.0, 32.0)),
            )
            .on_hover_text(format!("Factory binding: {}", describe(factory)))
            .clicked()
        {
            self.set_action(factory);
        }
    }

    /// Modifier toggles plus one key, which covers ordinary shortcuts. More
    /// unusual combinations go through "Edit as text".
    fn keys_editor(&mut self, ui: &mut Ui, codes: [u8; 4]) {
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
            hint(
                ui,
                "This binding presses several keys at once. Use \"Edit as text\" to change it.",
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

        ui.label(RichText::new("Held with").color(TEXT_DIM).size(12.5));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
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
        ui.label(RichText::new("Key").color(TEXT_DIM).size(12.5));
        ui.horizontal(|ui| {
            let current = key.map(|code| Action::key(code).to_string()["key:".len()..].to_string());
            egui::ComboBox::from_id_salt("key-name")
                .selected_text(current.as_deref().map_or("(none)".to_string(), key_title))
                .width(104.0)
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
                .add(
                    egui::Button::new(label)
                        .selected(self.capturing_key)
                        .min_size(vec2(0.0, 32.0)),
                )
                .on_hover_text("Click, then press the key (with any modifiers) on your keyboard.")
                .clicked()
            {
                self.capturing_key = !self.capturing_key;
            }
        });
        if held.is_empty() && key.is_none() {
            ui.label(
                RichText::new("Choose a key or a modifier.")
                    .color(WARNING)
                    .size(12.5),
            );
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

    fn lighting(&mut self, ui: &mut Ui) {
        theme::page_title(
            ui,
            "Lighting",
            "What the three light zones show while this computer is driving them.",
        );
        let mut changed = false;
        theme::card(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 30.0;
                let lights = match self.draft.effect {
                    Effect::Static => Lights::Zones(self.draft.zones),
                    Effect::Rainbow => Lights::Rainbow,
                };
                mouse_view::lighting(ui, &lights);
                ui.vertical(|ui| {
                    // Everything in this column is placed by hand so that each
                    // zone's controls sit level with that zone in the drawing.
                    ui.spacing_mut().item_spacing.y = 0.0;
                    const SWITCH: f32 = 36.0;
                    const ROW: f32 = 44.0;
                    let options = [
                        (Effect::Static, "Static colors"),
                        (Effect::Rainbow, "Rainbow"),
                    ];
                    if let Some(effect) = theme::segmented(ui, self.draft.effect, &options) {
                        if self.draft.effect != effect {
                            self.draft.effect = effect;
                            changed = true;
                        }
                    }
                    let mut used = SWITCH;
                    match self.draft.effect {
                        Effect::Static => {
                            for (index, name) in ["Front", "Palm", "Wrist"].into_iter().enumerate()
                            {
                                let (start, end) = mouse_view::ZONE_BANDS[index];
                                let center = (start + end) / 2.0 * mouse_view::LIGHTING_HEIGHT;
                                ui.add_space((center - ROW / 2.0 - used).max(8.0));
                                used = center + ROW / 2.0;
                                ui.allocate_ui(vec2(ui.available_width(), ROW), |ui| {
                                    ui.horizontal_centered(|ui| {
                                        ui.spacing_mut().item_spacing.x = 14.0;
                                        ui.spacing_mut().interact_size = vec2(58.0, 36.0);
                                        changed |= ui
                                            .color_edit_button_srgb(&mut self.draft.zones[index])
                                            .on_hover_text("Choose a color")
                                            .changed();
                                        ui.vertical(|ui| {
                                            ui.add_space(4.0);
                                            ui.label(
                                                RichText::new(name)
                                                    .font(FontId::new(14.5, theme::strong())),
                                            );
                                            let [r, g, b] = self.draft.zones[index];
                                            ui.label(
                                                RichText::new(format!("#{r:02x}{g:02x}{b:02x}"))
                                                    .monospace()
                                                    .color(TEXT_DIM)
                                                    .size(12.0),
                                            );
                                        });
                                    });
                                });
                            }
                            ui.add_space(14.0);
                            if theme::button(ui, "Use the front color everywhere").clicked() {
                                self.draft.zones = [self.draft.zones[0]; 3];
                                changed = true;
                            }
                        }
                        Effect::Rainbow => {
                            ui.add_space(22.0);
                            ui.label(
                                RichText::new("The mouse cycles through colors by itself.")
                                    .font(FontId::new(15.0, theme::strong())),
                            );
                            ui.add_space(6.0);
                            hint(
                                ui,
                                "This effect is built into the mouse, so it keeps running without ggez. Its speed and colors are fixed.",
                            );
                        }
                    }
                });
            });
            if self.draft.effect == Effect::Static {
                ui.add_space(4.0);
                theme::callout(
                    ui,
                    WARNING,
                    "The mouse does not remember static colors. ggez sends them again whenever the mouse connects or wakes, so they need ggez running (see Settings).",
                );
            }
            ui.add_space(4.0);
            theme::toggle(
                ui,
                &mut self.live_preview,
                "Show lighting changes on the mouse as I make them",
            );
        });

        ui.add_space(6.0);
        ui.columns(2, |columns| {
            theme::card(&mut columns[0], |ui| {
                theme::card_title(ui, "Click flash", "Flash a color every time you click.");
                let mut flash = self.draft.reactive.is_some();
                ui.horizontal(|ui| {
                    if theme::toggle(ui, &mut flash, "Flash on click") {
                        self.draft.reactive = flash.then_some([255, 255, 255]);
                        changed = true;
                    }
                    if let Some(color) = &mut self.draft.reactive {
                        changed |= ui.color_edit_button_srgb(color).changed();
                    }
                });
            });
            theme::card(&mut columns[1], |ui| {
                theme::card_title(
                    ui,
                    "Without ggez",
                    "What the mouse shows on its own after power-on. Stored on the mouse.",
                );
                egui::ComboBox::from_id_salt("default-lighting")
                    .selected_text(default_lighting_title(self.draft.default_lighting))
                    .width(ui.available_width().min(260.0))
                    .show_ui(ui, |ui| {
                        for index in 0..DEFAULT_LIGHTING_MODES.len() {
                            ui.selectable_value(
                                &mut self.draft.default_lighting,
                                index,
                                default_lighting_title(index),
                            );
                        }
                    });
            });
        });
        if changed {
            self.preview_lighting();
        }
    }

    fn power(&mut self, ui: &mut Ui) {
        theme::page_title(ui, "Power", "Trade a little convenience for battery life.");
        theme::card(ui, |ui| {
            theme::card_title(ui, "Battery", "");
            match &self.status {
                Status::Connected {
                    battery: Some((percent, charging)),
                    ..
                } => {
                    let (percent, charging) = (*percent, *charging);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("{percent}%"))
                                .font(FontId::new(34.0, theme::strong())),
                        );
                        ui.vertical(|ui| {
                            ui.add_space(12.0);
                            hint(
                                ui,
                                if charging {
                                    "Charging over USB."
                                } else {
                                    "Reported by the mouse in steps of 5%."
                                },
                            );
                        });
                    });
                    let color = if percent <= 15 && !charging {
                        DANGER
                    } else {
                        ACCENT
                    };
                    let (rect, _) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 8.0), Sense::hover());
                    ui.painter()
                        .rect_filled(rect, CornerRadius::same(4), RAISED);
                    let mut filled = rect;
                    filled.set_width(rect.width() * f32::from(percent.min(100)) / 100.0);
                    ui.painter()
                        .rect_filled(filled, CornerRadius::same(4), color);
                }
                Status::Connected { .. } => hint(ui, "The mouse did not report its battery level."),
                _ => hint(ui, "Connect the mouse to see its battery level."),
            }
        });
        ui.add_space(6.0);
        theme::card(ui, |ui| {
            theme::card_title(
                ui,
                "Sleep",
                "How long the mouse waits, untouched, before it goes to sleep.",
            );
            ui.horizontal(|ui| {
                ui.add(egui::Slider::new(&mut self.draft.sleep_minutes, 0..=20).show_value(false));
                let text = match self.draft.sleep_minutes {
                    0 => "Never".to_string(),
                    1 => "1 minute".to_string(),
                    n => format!("{n} minutes"),
                };
                ui.label(RichText::new(text).font(FontId::new(15.0, theme::strong())));
            });
            if self.draft.sleep_minutes == 0 {
                theme::callout(
                    ui,
                    WARNING,
                    "With sleep off, the mouse stays awake until you switch it off, which drains the battery.",
                );
            }
        });
        ui.add_space(6.0);
        theme::card(ui, |ui| {
            theme::card_title(
                ui,
                "Dim the lights",
                "How long the mouse waits, untouched, before it dims its lights.",
            );
            ui.horizontal(|ui| {
                ui.add(
                    egui::Slider::new(&mut self.draft.dim_seconds, 0..=1200)
                        .logarithmic(true)
                        .show_value(false),
                );
                let text = match self.draft.dim_seconds {
                    0 => "Never".to_string(),
                    s if s < 60 => format!("{s} seconds"),
                    s if s % 60 == 0 => format!("{} min", s / 60),
                    s => format!("{} min {} s", s / 60, s % 60),
                };
                ui.label(RichText::new(text).font(FontId::new(15.0, theme::strong())));
            });
        });
    }

    fn settings(&mut self, ui: &mut Ui) {
        theme::page_title(ui, "Settings", "How ggez itself behaves.");
        theme::card(ui, |ui| {
            theme::card_title(
                ui,
                "Keep my settings applied",
                "A small helper re-sends your colors whenever the mouse connects or wakes. It does nothing else and is not involved in mouse movement or clicks.",
            );
            match self.autostart.clone() {
                Ok(mut enabled) => {
                    if theme::toggle(ui, &mut enabled, "Start the helper when I log in") {
                        self.autostart = self.host.set_autostart(enabled).map(|()| enabled);
                    }
                }
                Err(e) => {
                    ui.label(
                        RichText::new(format!("Could not check the login setting: {e}"))
                            .color(DANGER),
                    );
                    if theme::button(ui, "Try again").clicked() {
                        self.autostart = self.host.autostart_enabled();
                    }
                }
            }
            hint(
                ui,
                if self.helper_running_here {
                    "The helper is running inside this window for now."
                } else {
                    "The helper is already running in the background."
                },
            );
        });

        ui.add_space(6.0);
        theme::card(ui, |ui| {
            theme::card_title(
                ui,
                "Profile file",
                "Your settings are kept in a plain text file that you can copy to another computer.",
            );
            if let Some(path) = self.profile_path.clone() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(path.display().to_string())
                            .monospace()
                            .color(TEXT_DIM),
                    );
                    if ui.link("Show in folder").clicked() {
                        self.host.reveal(&path);
                    }
                });
            }
            ui.horizontal(|ui| {
                if theme::button(ui, "Export…")
                    .on_hover_text("Save the settings shown in this window to a file.")
                    .clicked()
                {
                    if let Some(path) = self.host.pick_save_path() {
                        self.notice = match self.draft.to_profile().save(&path) {
                            Ok(()) => Notice::Good(format!("Exported to {}", path.display())),
                            Err(e) => Notice::Bad(format!("{e:#}")),
                        };
                    }
                }
                if theme::button(ui, "Import…")
                    .on_hover_text(
                        "Load settings from a file into this window. Nothing is sent until you apply.",
                    )
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
                if theme::button(ui, "Factory settings")
                    .on_hover_text(
                        "Load the mouse's factory settings into this window. Nothing is sent until you apply.",
                    )
                    .clicked()
                {
                    self.draft = Draft::default();
                    self.select_button(self.selected_button);
                    self.notice = Notice::None;
                }
            });
        });

        ui.add_space(6.0);
        theme::card(ui, |ui| {
            theme::card_title(ui, "SteelSeries GG", "");
            ui.label(
                RichText::new(
                    "Quit SteelSeries GG while you use ggez. GG keeps sending its own settings to the mouse, which overrides yours and makes commands fail.",
                )
                .color(TEXT_DIM),
            );
        });

        ui.add_space(6.0);
        theme::card(ui, |ui| {
            theme::card_title(ui, "About", "");
            ui.label(concat!(
                "ggez ",
                env!("CARGO_PKG_VERSION"),
                " · open source under the MIT license"
            ));
            ui.hyperlink_to(
                "github.com/Thrasherop/ggez",
                "https://github.com/Thrasherop/ggez",
            );
            hint(ui, "Not made by or affiliated with SteelSeries.");
        });
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if self.confirm_no_left_click {
            egui::Modal::new(egui::Id::new("no-left-click")).show(ctx, |ui| {
                ui.set_width(400.0);
                ui.label(
                    RichText::new("No button does a left click")
                        .font(FontId::new(18.0, theme::strong())),
                );
                ui.label(
                    RichText::new(
                        "With these bindings nothing on the mouse will left-click, so you would need another mouse or the keyboard to undo it.",
                    )
                    .color(TEXT_DIM),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if theme::button(ui, "Go back").clicked() {
                        self.confirm_no_left_click = false;
                    }
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Apply anyway").color(DANGER))
                                .min_size(vec2(0.0, 34.0)),
                        )
                        .clicked()
                    {
                        self.confirm_no_left_click = false;
                        self.apply();
                    }
                });
            });
        }
        if self.confirm_reset {
            egui::Modal::new(egui::Id::new("reset-buttons")).show(ctx, |ui| {
                ui.set_width(360.0);
                ui.label(
                    RichText::new("Reset all buttons?").font(FontId::new(18.0, theme::strong())),
                );
                ui.label(
                    RichText::new(
                        "Every button goes back to its factory binding in this window. Nothing is sent until you apply.",
                    )
                    .color(TEXT_DIM),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if theme::button(ui, "Cancel").clicked() {
                        self.confirm_reset = false;
                    }
                    if theme::button(ui, "Reset").clicked() {
                        self.confirm_reset = false;
                        self.draft.buttons = ButtonMap::default();
                        self.select_button(self.selected_button);
                    }
                });
            });
        }
    }
}

/// One entry in the sidebar. Returns whether it was clicked.
fn nav_item(ui: &mut Ui, selected: bool, tab: Tab, name: &str) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
    response.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, selected, name));
    let (fill, ink) = if selected {
        (Some(ACCENT_DEEP.gamma_multiply(0.55)), TEXT)
    } else if response.hovered() {
        (Some(RAISED), TEXT)
    } else {
        (None, TEXT_DIM)
    };
    if let Some(fill) = fill {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(theme::CONTROL_RADIUS), fill);
    }
    if selected {
        let bar = Rect::from_min_size(rect.left_top() + vec2(0.0, 10.0), vec2(3.0, 20.0));
        ui.painter().rect_filled(bar, CornerRadius::same(2), ACCENT);
    }
    let icon_color = if selected { ACCENT } else { ink };
    nav_icon(
        ui,
        tab,
        pos2(rect.left() + 24.0, rect.center().y),
        icon_color,
    );
    let font = if selected {
        FontId::new(14.5, theme::strong())
    } else {
        FontId::proportional(14.5)
    };
    ui.painter().text(
        pos2(rect.left() + 46.0, rect.center().y),
        Align2::LEFT_CENTER,
        name,
        font,
        ink,
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

/// A small line drawing for each page.
fn nav_icon(ui: &Ui, tab: Tab, c: egui::Pos2, color: Color32) {
    let p = ui.painter();
    let stroke = Stroke::new(1.6, color);
    match tab {
        Tab::Sensitivity => {
            p.circle_stroke(c, 6.0, stroke);
            p.circle_filled(c, 1.8, color);
            for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                p.line_segment([c + vec2(dx, dy) * 6.0, c + vec2(dx, dy) * 9.0], stroke);
            }
        }
        Tab::Buttons => {
            for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                let cell = Rect::from_center_size(c + vec2(dx, dy) * 4.5, vec2(6.0, 6.0));
                p.rect_stroke(cell, CornerRadius::same(2), stroke, StrokeKind::Inside);
            }
        }
        Tab::Lighting => {
            p.circle_stroke(c, 4.0, stroke);
            for i in 0..8 {
                let angle = i as f32 * std::f32::consts::TAU / 8.0;
                let dir = vec2(angle.cos(), angle.sin());
                p.line_segment([c + dir * 6.5, c + dir * 9.0], stroke);
            }
        }
        Tab::Power => {
            let body = Rect::from_center_size(c - vec2(1.0, 0.0), vec2(15.0, 9.0));
            p.rect_stroke(body, CornerRadius::same(2), stroke, StrokeKind::Inside);
            p.line_segment(
                [
                    pos2(body.right() + 2.0, c.y - 2.0),
                    pos2(body.right() + 2.0, c.y + 2.0),
                ],
                stroke,
            );
            p.rect_filled(
                Rect::from_min_max(
                    body.min + vec2(2.5, 2.5),
                    pos2(body.center().x + 1.0, body.max.y - 2.5),
                ),
                CornerRadius::same(1),
                color,
            );
        }
        Tab::Settings => {
            for (dy, knob) in [(-5.0, 3.0), (0.0, -3.0), (5.0, 1.0)] {
                p.line_segment([c + vec2(-8.0, dy), c + vec2(8.0, dy)], stroke);
                p.circle_filled(c + vec2(knob, dy), 2.6, color);
            }
        }
    }
}

fn battery_bar(ui: &mut Ui, percent: u8, charging: bool) {
    let color = if percent <= 15 && !charging {
        DANGER
    } else {
        ACCENT
    };
    let text = if charging {
        format!("{percent}% · charging")
    } else {
        format!("{percent}%")
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new("Battery").color(TEXT_DIM).size(12.5));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(text).size(12.5));
        });
    });
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(3), RAISED);
    let mut filled = rect;
    filled.set_width(rect.width() * f32::from(percent.min(100)) / 100.0);
    ui.painter()
        .rect_filled(filled, CornerRadius::same(3), color);
    response.on_hover_text("Battery level, in steps of 5%");
}

/// The marker at the start of a CPI row: shows which level is active and
/// makes one active when clicked.
fn level_marker(ui: &mut Ui, active: bool, number: usize) -> bool {
    let label = format!("Level {number}");
    let (rect, response) = ui.allocate_exact_size(vec2(86.0, 32.0), Sense::click());
    response.widget_info(|| WidgetInfo::selected(WidgetType::RadioButton, true, active, &label));
    let center = pos2(rect.left() + 10.0, rect.center().y);
    if active {
        ui.painter()
            .circle_stroke(center, 7.0, Stroke::new(1.6, ACCENT));
        ui.painter().circle_filled(center, 3.6, ACCENT);
    } else {
        let color = if response.hovered() {
            TEXT_DIM
        } else {
            TEXT_FAINT
        };
        ui.painter()
            .circle_stroke(center, 7.0, Stroke::new(1.6, color));
    }
    let font = if active {
        FontId::new(14.5, theme::strong())
    } else {
        FontId::proportional(14.5)
    };
    ui.painter().text(
        pos2(rect.left() + 26.0, rect.center().y),
        Align2::LEFT_CENTER,
        &label,
        font,
        if active { TEXT } else { TEXT_DIM },
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("Make this the active level.")
        .clicked()
}

/// One option among a few shown side by side. Returns whether it was
/// clicked.
fn choice(ui: &mut Ui, selected: bool, text: &str) -> bool {
    ui.add(
        egui::Button::new(text)
            .selected(selected)
            .min_size(vec2(52.0, 32.0)),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
    .clicked()
}

fn hint(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).color(TEXT_DIM).size(13.0));
}

/// A binding drawn as key caps: `Ctrl` `+` `V`.
fn action_keycaps(ui: &mut Ui, action: Action) {
    match action {
        Action::Keys(_) => {
            let text = action.to_string();
            let names: Vec<&str> = text["key:".len()..]
                .split('+')
                .filter(|n| !n.is_empty())
                .collect();
            if names.is_empty() {
                theme::keycap(ui, "No key");
            }
            for (i, name) in names.iter().enumerate() {
                if i > 0 {
                    ui.label(RichText::new("+").color(TEXT_FAINT));
                }
                theme::keycap(ui, &key_title(name));
            }
        }
        other => theme::keycap(ui, &describe(other)),
    }
}

/// The broad kinds of action offered in the "What it does" menu.
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
