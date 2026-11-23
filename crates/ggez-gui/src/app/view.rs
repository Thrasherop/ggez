//! What the window draws. State and decisions live in the parent module.

use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, CornerRadius, FontId, Layout, Rect, RichText, Sense,
    Stroke, Ui, WidgetInfo, WidgetType,
};
use ggez_core::{key_names, media_names, Action, Button, ButtonMap, Link};
use ggez_service::macro_host::Host;
use ggez_service::macros::{Mode, MAX_MACROS};
use ggez_service::profile::DEFAULT_LIGHTING_MODES;
use ggez_service::Profile;

use super::{App, Notice, Tab};
use crate::device::Status;
use crate::draft::{snap_dpi, Draft, Effect, MAX_DPI_PRESETS, POLLING_RATES};
use crate::mouse_view::{self, Lights, Place};
use crate::theme::{self, ACCENT, CARD, DANGER, GOOD, LINE, TEXT, TEXT_DIM, TEXT_FAINT, WARNING};

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

        // The stage is lit in the mouse's own colors. The rainbow only moves
        // on the lighting page, where it is what the user is looking at.
        let moving = self.tab == Tab::Lighting
            && self.draft.effect == Effect::Rainbow
            && self.light_level() > 0.0;
        let time = if moving {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
            ctx.input(|input| input.time) as f32
        } else {
            5.6
        };
        let lights = self.lights().colors(self.light_level(), time);
        theme::backdrop(
            &ctx.layer_painter(egui::LayerId::background()),
            ctx.screen_rect(),
            mouse_view::take_focus(ctx),
            lights,
        );
        let margin = egui::Margin::symmetric(24, 0);
        egui::TopBottomPanel::top("top")
            .exact_height(68.0)
            .show_separator_line(false)
            .frame(egui::Frame::new().inner_margin(margin))
            .show(ctx, |ui| self.top_bar(ui));
        egui::TopBottomPanel::bottom("actions")
            .exact_height(80.0)
            .show_separator_line(false)
            .frame(egui::Frame::new().inner_margin(margin))
            .show(ctx, |ui| self.action_bar(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(margin))
            .show(ctx, |ui| {
                let view_height = ui.available_height();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let top = ui.cursor().top();
                        column(ui, |ui| self.banners(ui));
                        // What is left of the view for a page that fills it.
                        let room = view_height - (ui.cursor().top() - top);
                        match self.tab {
                            Tab::Buttons => self.buttons(ui, room, lights),
                            Tab::Lighting => self.lighting(ui, room, lights),
                            Tab::Sensitivity => column(ui, |ui| self.sensitivity(ui)),
                            Tab::Power => column(ui, |ui| self.power(ui)),
                            Tab::Settings => column(ui, |ui| self.settings(ui)),
                        }
                    });
            });
        self.dialogs(ctx);
    }

    /// The name, the pages, and at the far end the profile and the mouse.
    fn top_bar(&mut self, ui: &mut Ui) {
        let bar = ui.max_rect();
        let part = |ui: &mut Ui, name: &str, rect: Rect, layout: Layout| {
            ui.new_child(
                egui::UiBuilder::new()
                    .id_salt(name)
                    .max_rect(rect)
                    .layout(layout),
            )
        };

        let mut name = part(ui, "name", bar, Layout::left_to_right(Align::Center));
        name.spacing_mut().item_spacing.x = 9.0;
        theme::logo(&mut name, 24.0);
        name.label(RichText::new("ggez").font(FontId::new(22.0, theme::strong())));

        let middle = Rect::from_center_size(bar.center(), vec2(bar.width(), 40.0));
        let mut pages = part(ui, "pages", middle, Layout::top_down(Align::Center));
        if let Some(tab) = theme::tabs(&mut pages, self.tab, &Tab::ALL) {
            self.tab = tab;
            self.capturing_key = false;
        }

        let mut end = part(ui, "end", bar, Layout::right_to_left(Align::Center));
        end.spacing_mut().item_spacing.x = 14.0;
        self.profile_picker(&mut end);
        self.device_chip(&mut end);
    }

    /// The saved profiles: which one is in use, switching to another, and
    /// saving the current settings under a new name.
    fn profile_picker(&mut self, ui: &mut Ui) {
        if self.store.is_none() {
            return;
        }
        let shown = self
            .current_profile
            .clone()
            .unwrap_or_else(|| "Not saved".to_string());
        let opener = theme::dropdown_button(ui, &shown, 150.0)
            .on_hover_text("Profile: switch between saved setups, or save this one.");
        let popup = ui.id().with("profile-picker");
        if opener.clicked() {
            ui.memory_mut(|memory| memory.toggle_popup(popup));
        }
        let mut switch_to = None;
        let mut save_as = None;
        egui::popup_below_widget(
            ui,
            popup,
            &opener,
            egui::PopupCloseBehavior::CloseOnClickOutside,
            |ui| {
                ui.set_min_width(220.0);
                if self.profile_names.is_empty() {
                    hint(ui, "No saved profiles yet.");
                }
                for name in &self.profile_names {
                    let current = self.current_profile.as_deref() == Some(name.as_str());
                    ui.horizontal(|ui| {
                        if ui
                            .selectable_label(current, name)
                            .on_hover_text("Load this profile and send it to the mouse.")
                            .clicked()
                        {
                            switch_to = Some(name.clone());
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Delete").color(TEXT_FAINT).size(12.0),
                                    )
                                    .frame(false),
                                )
                                .on_hover_text(format!("Delete {name}"))
                                .clicked()
                            {
                                self.confirm_delete_profile = Some(name.clone());
                            }
                        });
                    });
                }
                ui.separator();
                ui.label(
                    RichText::new("Save these settings as")
                        .color(TEXT_DIM)
                        .size(12.5),
                );
                ui.horizontal(|ui| {
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut self.new_profile_name)
                            .hint_text("Name, such as Work")
                            .desired_width(140.0),
                    );
                    let entered =
                        field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                    let wanted = !self.new_profile_name.trim().is_empty();
                    if (ui.add_enabled(wanted, egui::Button::new("Save")).clicked() || entered)
                        && wanted
                    {
                        save_as = Some(self.new_profile_name.clone());
                    }
                });
            },
        );
        if let Some(name) = switch_to {
            ui.memory_mut(|memory| memory.close_popup());
            self.switch_profile(&name);
        }
        if let Some(name) = save_as {
            ui.memory_mut(|memory| memory.close_popup());
            self.save_profile_as(&name);
        }
        if self.confirm_delete_profile.is_some() {
            ui.memory_mut(|memory| memory.close_popup());
        }
    }

    /// Which mouse this is and how it is doing, in a few words; the rest
    /// is shown when the pointer rests on it.
    fn device_chip(&mut self, ui: &mut Ui) {
        let (color, title, detail) = match &self.status {
            Status::Unknown => (WARNING, "Looking for your mouse…", String::new()),
            Status::NoDevice => (
                DANGER,
                "Not connected",
                "Plug in the USB cable or the 2.4 GHz dongle.".to_string(),
            ),
            Status::Blocked(reason) => (DANGER, "Found, but cannot be opened", reason.clone()),
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
        let mut more = format!("Aerox 9 Wireless\n{title}");
        if !detail.is_empty() {
            more.push_str(&format!("\n{detail}"));
        }
        let (text, color) = match &self.status {
            Status::Connected {
                battery, firmware, ..
            } => {
                if let Some(version) = firmware {
                    more.push_str(&format!("\nFirmware {version}"));
                }
                match battery {
                    Some((percent, true)) => (format!("Aerox 9 · {percent}%, charging"), color),
                    Some((percent, false)) => (
                        format!("Aerox 9 · {percent}%"),
                        if *percent <= 15 { DANGER } else { color },
                    ),
                    None => {
                        more.push_str("\nBattery level unavailable");
                        ("Aerox 9".to_string(), color)
                    }
                }
            }
            _ => (title.to_string(), color),
        };
        // Laid out from the right: the words, then the light before them.
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.label(RichText::new(text).color(TEXT_DIM).size(13.5))
                .on_hover_text(more);
            theme::dot(ui, color);
        });
    }

    /// Apply and Revert, floating in the middle, with what the window has
    /// to say beside them.
    fn action_bar(&mut self, ui: &mut Ui) {
        let bar = ui.max_rect();
        let width = 92.0 + 10.0 + theme::primary_width(ui, "Apply to mouse");
        let middle = Rect::from_center_size(bar.center(), vec2(width, 42.0));
        let mut buttons = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("buttons")
                .max_rect(middle)
                .layout(Layout::left_to_right(Align::Center)),
        );
        {
            let ui = &mut buttons;
            ui.spacing_mut().item_spacing.x = 10.0;
            let revert = egui::Button::new("Revert")
                .corner_radius(CornerRadius::same(21))
                .min_size(vec2(92.0, 42.0));
            let revert = ui
                .add_enabled(self.draft != self.applied, revert)
                .on_hover_text("Discard changes made since the last apply.");
            theme::focus_ring(ui, &revert, revert.rect, 21);
            if revert.clicked() {
                self.draft = self.applied.clone();
                self.select_button(self.selected_button);
                self.notice = Notice::None;
                self.preview_lighting();
            }
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
        }

        // A long message wraps in the space beside the buttons.
        let beside = Rect::from_min_max(bar.left_top(), pos2(middle.left() - 20.0, bar.bottom()));
        let mut note = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("notice")
                .max_rect(beside)
                .layout(Layout::left_to_right(Align::Center)),
        );
        let ui = &mut note;
        let message = |ui: &mut Ui, color, text: &str| {
            theme::dot(ui, color);
            ui.add(egui::Label::new(RichText::new(text).color(color)).wrap());
        };
        match &self.notice {
            Notice::None if self.is_dirty() => message(
                ui,
                WARNING,
                "You have changes that are not on the mouse yet.",
            ),
            Notice::None => {}
            Notice::Working(text) => {
                ui.spinner();
                ui.label(text);
            }
            Notice::Good(text) => message(ui, GOOD, text),
            Notice::Bad(text) => message(ui, DANGER, text),
        }
    }

    /// Things the user should know whatever page they are on.
    fn banners(&mut self, ui: &mut Ui) {
        if let Status::Blocked(reason) = &self.status {
            theme::callout(ui, DANGER, &blocked_advice(reason));
            ui.add_space(6.0);
        }
        if matches!(self.status, Status::NoDevice | Status::Asleep) {
            theme::callout(
                ui,
                TEXT_DIM,
                "The mouse is not reachable right now. You can keep editing; Apply becomes available when it is back.",
            );
            ui.add_space(6.0);
        }
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

    /// What the mouse's lights are set to show.
    fn lights(&self) -> Lights {
        match self.draft.effect {
            Effect::Static => Lights::Zones(self.draft.zones),
            Effect::Rainbow => Lights::Rainbow,
        }
    }

    /// How bright the drawings show the lights, from 0 to 1. Even the
    /// lowest setting is drawn bright enough to tell the colors apart.
    fn light_level(&self) -> f32 {
        if self.draft.battery_saver || self.draft.brightness == 0 {
            0.0
        } else {
            0.35 + 0.65 * self.draft.brightness as f32 / 100.0
        }
    }

    fn sensitivity(&mut self, ui: &mut Ui) {
        ui.add_space(4.0);
        theme::card(ui, |ui| {
            theme::card_title(
                ui,
                "CPI levels",
                "The mouse holds up to five. The button behind the wheel steps through them.",
            );
            cpi_scale(ui, &self.draft.dpi, self.draft.dpi_selected);
            ui.add_space(4.0);
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
                    theme::slider(
                        ui,
                        &mut value,
                        100..=18_000,
                        |_| 100,
                        true,
                        &format!("Level {} sensitivity", index + 1),
                    );
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
            if self.draft.battery_saver {
                ui.add_space(4.0);
                theme::callout(
                    ui,
                    WARNING,
                    "Battery saver is on (see Power), so the mouse uses 125 Hz until you turn it off.",
                );
            }
        });
    }

    /// The mouse on the stage with every button to click, and beside it a
    /// panel for the one that is selected. `room` is the height there is
    /// to fill.
    fn buttons(&mut self, ui: &mut Ui, room: f32, lights: [Color32; 3]) {
        let height = room.max(476.0);
        let (stage, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
        // The drawing is 356 tall at its natural size and grows with the
        // window, up to a point.
        let k = ((height - 60.0) / 356.0).clamp(1.0, 1.25);
        let center = pos2(stage.left() + stage.width() * 0.43, stage.center().y);
        let factory = ButtonMap::default();
        let draft = &self.draft;
        let binding = |button: Button| {
            if draft.has_macro(button) {
                ("Macro".to_string(), true)
            } else {
                let action = draft.buttons.get(button);
                (describe(action), action != factory.get(button))
            }
        };
        let clicked = mouse_view::buttons(
            ui,
            Place { center, scale: k },
            self.selected_button,
            lights,
            &binding,
            &button_title,
        );
        if let Some(button) = clicked {
            self.select_button(button);
        }

        let beside = Rect::from_min_max(
            pos2(stage.right() - 284.0, stage.top() + 10.0),
            stage.right_bottom(),
        );
        let mut editor = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("editor")
                .max_rect(beside)
                .layout(Layout::top_down(Align::Min)),
        );
        theme::panel(&mut editor, 18, |ui| self.button_editor(ui));

        let corner = Rect::from_min_max(
            pos2(stage.left(), stage.bottom() - 36.0),
            pos2(center.x + 60.0, stage.bottom()),
        );
        let mut reset = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("reset")
                .max_rect(corner)
                .layout(Layout::left_to_right(Align::Center)),
        );
        if theme::button(&mut reset, "Reset all buttons")
            .on_hover_text("Put every button back to its factory binding.")
            .clicked()
        {
            self.confirm_reset = true;
        }
        let changed = Button::ALL
            .into_iter()
            .filter(|&b| self.draft.has_macro(b) || self.draft.buttons.get(b) != factory.get(b))
            .count();
        if changed > 0 {
            reset.label(
                RichText::new(format!("{changed} of 20 changed, shown in teal."))
                    .color(TEXT_DIM)
                    .size(13.0),
            );
        }
        // A short window scrolls to whatever did not fit on the stage.
        let below = editor.min_rect().bottom() - stage.bottom();
        if below > 0.0 {
            ui.add_space(below);
        }
    }

    fn button_editor(&mut self, ui: &mut Ui) {
        let button = self.selected_button;
        let action = self.draft.buttons.get(button);
        ui.label(RichText::new(button_title(button)).font(FontId::new(18.0, theme::strong())));
        let is_macro = self.draft.has_macro(button);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            if is_macro {
                theme::keycap(ui, "Macro");
            } else {
                action_keycaps(ui, action);
            }
        });
        ui.add_space(8.0);

        let kind = if is_macro {
            Kind::Macro
        } else {
            Kind::of(action)
        };
        theme::caption(ui, "What it does");
        egui::ComboBox::from_id_salt("action-kind")
            .icon(list_mark)
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
                        self.set_macro(candidate == Kind::Macro);
                        // Leaving a macro for the kind of binding the
                        // button had underneath keeps that binding.
                        if candidate != Kind::Macro && Kind::of(action) != candidate {
                            self.set_action(candidate.default_action());
                        }
                    }
                }
            })
            .response
            .widget_info(|| named_list("What it does", kind.label()));
        ui.add_space(4.0);

        if is_macro {
            self.macro_editor(ui);
            self.reset_button(ui);
            return;
        }
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
                        "Button 6",
                        "Button 7",
                        "Button 8",
                    ]) {
                        if choice(ui, current == number, name) {
                            self.set_action(Action::MouseButton(number));
                        }
                    }
                });
                if current > 5 {
                    hint(
                        ui,
                        "The mouse sends buttons 6 to 8 as real mouse buttons, but Windows only passes the first five on to programs. They are useful on Linux.",
                    );
                }
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

        self.reset_button(ui);
    }

    /// Puts the selected button back to what it does out of the box.
    fn reset_button(&mut self, ui: &mut Ui) {
        let button = self.selected_button;
        let factory = ButtonMap::default().get(button);
        let changed = self.draft.buttons.get(button) != factory || self.draft.has_macro(button);
        if ui
            .add_enabled(
                changed,
                egui::Button::new("Reset this button").min_size(vec2(0.0, 32.0)),
            )
            .on_hover_text(format!("Factory binding: {}", describe(factory)))
            .clicked()
        {
            self.set_macro(false);
            self.set_action(factory);
        }
    }

    /// The steps of the selected button's macro and when it runs.
    fn macro_editor(&mut self, ui: &mut Ui) {
        let name = self.selected_button.to_string();
        let count = self.draft.macros.len();
        let Some(entry) = self.draft.macros.get_mut(&name) else {
            return;
        };
        theme::caption(ui, "Runs");
        let options = [
            (Mode::Once, "Once"),
            (Mode::Repeat, "Held"),
            (Mode::Toggle, "Toggle"),
        ];
        if let Some(mode) = theme::segmented(ui, entry.mode, &options) {
            entry.mode = mode;
        }
        hint(
            ui,
            match entry.mode {
                Mode::Once => "Once each time you press the button.",
                Mode::Repeat => "Again and again for as long as you hold the button.",
                Mode::Toggle => "Again and again from one press until the next.",
            },
        );
        ui.add_space(2.0);
        theme::caption(ui, "Steps, one per line");
        ui.add(
            egui::TextEdit::multiline(&mut entry.steps)
                .font(egui::TextStyle::Monospace)
                .desired_rows(5)
                .desired_width(f32::INFINITY)
                .hint_text("key:ctrl+c"),
        );
        match entry.problem() {
            Some(problem) => {
                ui.label(RichText::new(problem).color(DANGER).size(12.5));
            }
            None => hint(
                ui,
                "key:ctrl+c   down:shift   up:shift   text:hello   click:left   wait:50 (milliseconds)",
            ),
        }
        if count > MAX_MACROS {
            ui.label(
                RichText::new(format!(
                    "{count} buttons have a macro, but at most {MAX_MACROS} can."
                ))
                .color(DANGER)
                .size(12.5),
            );
        }
        ui.add_space(2.0);
        if Host::supported() {
            theme::callout(
                ui,
                TEXT_DIM,
                "A macro runs on this computer, so it works only while ggez is running. Other bindings live on the mouse.",
            );
        } else {
            theme::callout(
                ui,
                WARNING,
                "ggez can run macros on Windows only so far. On this computer the button will do nothing.",
            );
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

        ui.add_space(2.0);
        theme::caption(ui, "Held with");
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
        ui.add_space(2.0);
        theme::caption(ui, "Key");
        ui.horizontal(|ui| {
            let current = key.map(|code| Action::key(code).to_string()["key:".len()..].to_string());
            let shown = current.as_deref().map_or("(none)".to_string(), key_title);
            let opener = theme::dropdown_button(ui, &shown, 116.0);
            let popup = ui.id().with("key-picker");
            if opener.clicked() {
                self.key_filter.clear();
                ui.memory_mut(|memory| memory.toggle_popup(popup));
            }
            // `Some(None)` means "(none)" was chosen.
            let mut picked: Option<Option<String>> = None;
            egui::popup_below_widget(
                ui,
                popup,
                &opener,
                egui::PopupCloseBehavior::CloseOnClickOutside,
                |ui| {
                    ui.set_min_width(190.0);
                    let search = ui.add(
                        egui::TextEdit::singleline(&mut self.key_filter)
                            .hint_text("Search keys")
                            .desired_width(f32::INFINITY),
                    );
                    if opener.clicked() {
                        search.request_focus();
                    }
                    let filter = self.key_filter.trim().to_lowercase();
                    let matches: Vec<String> = key_names()
                        .into_iter()
                        .filter(|name| !is_modifier_name(name))
                        .filter(|name| {
                            name.contains(&filter)
                                || key_title(name).to_lowercase().contains(&filter)
                        })
                        .collect();
                    // Enter takes the first match, so a key can be chosen
                    // without leaving the keyboard.
                    if search.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter))
                    {
                        if let Some(first) = matches.first() {
                            picked = Some(Some(first.clone()));
                        }
                    }
                    egui::ScrollArea::vertical()
                        .max_height(270.0)
                        .show(ui, |ui| {
                            ui.set_min_width(170.0);
                            if filter.is_empty()
                                && ui.selectable_label(key.is_none(), "(none)").clicked()
                            {
                                picked = Some(None);
                            }
                            for name in &matches {
                                let selected = current.as_deref() == Some(name.as_str());
                                if ui.selectable_label(selected, key_title(name)).clicked() {
                                    picked = Some(Some(name.clone()));
                                }
                            }
                            if matches.is_empty() {
                                hint(ui, "No key by that name.");
                            }
                        });
                },
            );
            if let Some(choice) = picked {
                let code = choice.and_then(|name| match format!("key:{name}").parse() {
                    Ok(Action::Keys([code, ..])) => Some(code),
                    _ => None,
                });
                // A binding needs at least one key or modifier.
                if code.is_some() || !held.is_empty() {
                    self.set_action(rebuild(&held, code));
                }
                ui.memory_mut(|memory| memory.close_popup());
            }
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

    /// The mouse on the stage, glowing as it will; each zone's color beside
    /// that zone, and the rest of the lighting settings in a panel.
    fn lighting(&mut self, ui: &mut Ui, room: f32, lights: [Color32; 3]) {
        let height = room.max(480.0);
        let (stage, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
        let part = |ui: &mut Ui, name: &str, rect: Rect, layout: Layout| {
            ui.new_child(
                egui::UiBuilder::new()
                    .id_salt(name)
                    .max_rect(rect)
                    .layout(layout),
            )
        };
        let mut changed = false;

        // Which effect, above the mouse.
        let top = Rect::from_min_size(stage.left_top(), vec2(stage.width(), 40.0));
        let mut switch = part(ui, "effect", top, Layout::top_down(Align::Center));
        let options = [
            (Effect::Static, "Static colors"),
            (Effect::Rainbow, "Rainbow"),
        ];
        if let Some(effect) = theme::segmented(&mut switch, self.draft.effect, &options) {
            if self.draft.effect != effect {
                self.draft.effect = effect;
                changed = true;
            }
        }

        let k = ((height - 84.0) / mouse_view::LIGHTING_HEIGHT).clamp(1.0, 1.3);
        let center = pos2(
            stage.center().x,
            stage.top() + 58.0 + mouse_view::LIGHTING_HEIGHT / 2.0 * k,
        );
        let zones = mouse_view::lighting(ui, Place { center, scale: k }, lights);

        // Beside the mouse: each zone's color, level with that zone.
        let column = zones[1].x + 54.0;
        match self.draft.effect {
            Effect::Static => {
                for (index, name) in ["Front", "Palm", "Wrist"].into_iter().enumerate() {
                    let zone = zones[index];
                    ui.painter().line_segment(
                        [zone + vec2(8.0, 0.0), pos2(column - 12.0, zone.y)],
                        Stroke::new(1.0, Color32::WHITE.gamma_multiply(0.3)),
                    );
                    ui.painter().circle_filled(
                        zone + vec2(8.0, 0.0),
                        2.6,
                        Color32::WHITE.gamma_multiply(0.6),
                    );
                    let row = Rect::from_min_max(
                        pos2(column, zone.y - 22.0),
                        pos2(stage.right(), zone.y + 22.0),
                    );
                    let mut row = part(ui, name, row, Layout::left_to_right(Align::Center));
                    row.spacing_mut().item_spacing.x = 14.0;
                    changed |= theme::swatch(
                        &mut row,
                        &mut self.draft.zones[index],
                        &format!("{name} color"),
                    );
                    row.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.add_space(4.0);
                        ui.label(RichText::new(name).font(FontId::new(14.5, theme::strong())));
                        let [r, g, b] = self.draft.zones[index];
                        ui.label(
                            RichText::new(format!("#{r:02x}{g:02x}{b:02x}"))
                                .monospace()
                                .color(TEXT_DIM)
                                .size(12.0),
                        );
                    });
                }
                let under = Rect::from_min_max(
                    pos2(column, zones[2].y + 32.0),
                    pos2(stage.right(), zones[2].y + 70.0),
                );
                let mut under = part(ui, "same", under, Layout::left_to_right(Align::Center));
                if theme::button(&mut under, "Use the front color everywhere").clicked() {
                    self.draft.zones = [self.draft.zones[0]; 3];
                    changed = true;
                }
            }
            Effect::Rainbow => {
                let words = Rect::from_min_max(
                    pos2(column, zones[1].y - 44.0),
                    pos2((column + 290.0).min(stage.right()), stage.bottom()),
                );
                let mut words = part(ui, "rainbow", words, Layout::top_down(Align::Min));
                words.label(
                    RichText::new("The mouse cycles through colors by itself.")
                        .font(FontId::new(15.0, theme::strong())),
                );
                hint(
                    &mut words,
                    "This effect is built into the mouse, so it keeps running without ggez. Its speed and colors are fixed.",
                );
            }
        }

        // Everything else about the lights, in a panel on the other side.
        let side = Rect::from_min_max(
            stage.left_top() + vec2(0.0, 10.0),
            pos2(stage.left() + 316.0, stage.bottom()),
        );
        let mut side = part(ui, "settings", side, Layout::top_down(Align::Min));
        theme::panel(&mut side, 18, |ui| {
            if self.draft.battery_saver {
                theme::callout(
                    ui,
                    WARNING,
                    "Battery saver is on (see Power), so the lights stay off until you turn it off.",
                );
            }
            theme::caption(ui, "Brightness");
            ui.horizontal(|ui| {
                ui.spacing_mut().slider_width = 214.0;
                changed |= theme::slider(
                    ui,
                    &mut self.draft.brightness,
                    0..=100,
                    |_| 5,
                    false,
                    "Brightness",
                )
                .on_hover_text("Stored on the mouse, which has 16 brightness steps.")
                .changed();
                let text = match self.draft.brightness {
                    0 => "Off".to_string(),
                    percent => format!("{percent}%"),
                };
                ui.label(RichText::new(text).font(FontId::new(14.5, theme::strong())));
            });
            ui.add_space(2.0);
            changed |= theme::toggle(
                ui,
                &mut self.draft.smart_lighting,
                "Turn the lights off while the mouse is moving",
            );
            ui.add_space(8.0);

            theme::caption(ui, "Click flash");
            let mut flash = self.draft.reactive.is_some();
            ui.horizontal(|ui| {
                if theme::toggle(ui, &mut flash, "Flash on click") {
                    self.draft.reactive = flash.then_some([255, 255, 255]);
                    changed = true;
                }
                if let Some(color) = &mut self.draft.reactive {
                    changed |= theme::swatch(ui, color, "Flash color");
                }
            });
            ui.add_space(8.0);

            theme::caption(ui, "Without ggez");
            egui::ComboBox::from_id_salt("default-lighting")
                .icon(list_mark)
                .selected_text(default_lighting_title(self.draft.default_lighting))
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for index in 0..DEFAULT_LIGHTING_MODES.len() {
                        ui.selectable_value(
                            &mut self.draft.default_lighting,
                            index,
                            default_lighting_title(index),
                        );
                    }
                })
                .response
                .on_hover_text(
                    "What the mouse shows on its own after power-on. Stored on the mouse.",
                )
                .widget_info(|| {
                    named_list(
                        "Without ggez",
                        default_lighting_title(self.draft.default_lighting),
                    )
                });
            ui.add_space(8.0);

            theme::toggle(
                ui,
                &mut self.live_preview,
                "Show lighting changes on the mouse as I make them",
            );
            if self.draft.effect == Effect::Static {
                ui.add_space(2.0);
                hint(
                    ui,
                    "The mouse does not remember static colors. ggez sends them again whenever the mouse connects or wakes, so they need ggez running (see Settings).",
                );
            }
        });
        // A short window scrolls to whatever did not fit on the stage.
        let below = side.min_rect().bottom() - stage.bottom();
        if below > 0.0 {
            ui.add_space(below);
        }
        if changed {
            self.preview_lighting();
        }
    }

    fn power(&mut self, ui: &mut Ui) {
        ui.add_space(4.0);
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
                    theme::meter(ui, 8.0, f32::from(percent) / 100.0, color);
                }
                Status::Connected { .. } => hint(ui, "The mouse did not report its battery level."),
                _ => hint(ui, "Connect the mouse to see its battery level."),
            }
        });
        ui.add_space(6.0);
        theme::card(ui, |ui| {
            theme::card_title(
                ui,
                "Battery saver",
                "For the longest battery life: lights off, 125 Hz polling, and the mouse's own power-saving mode.",
            );
            theme::toggle(ui, &mut self.draft.battery_saver, "Use battery saver");
            if self.draft.battery_saver {
                hint(
                    ui,
                    "Your polling rate and lighting settings are kept and come back when you turn this off.",
                );
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
                theme::slider(
                    ui,
                    &mut self.draft.sleep_minutes,
                    0..=20,
                    |_| 1,
                    false,
                    "Minutes before sleep",
                );
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
                // Fine steps where seconds matter, coarser ones beyond.
                let step = |seconds: u32| match seconds {
                    0..=59 => 5,
                    60..=299 => 30,
                    _ => 60,
                };
                theme::slider(
                    ui,
                    &mut self.draft.dim_seconds,
                    0..=1200,
                    step,
                    true,
                    "Seconds before the lights dim",
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
        ui.add_space(4.0);
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
                if !self.helper_running_here {
                    "The helper is already running in the background."
                } else if cfg!(windows) && matches!(self.autostart, Ok(true)) {
                    "The helper is running inside this window. When you close the window it moves to an icon in the notification area, by the clock."
                } else {
                    "The helper is running inside this window and stops when you close it."
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
        if let Some(name) = self.confirm_delete_profile.clone() {
            egui::Modal::new(egui::Id::new("delete-profile")).show(ctx, |ui| {
                ui.set_width(360.0);
                ui.label(
                    RichText::new(format!("Delete {name}?"))
                        .font(FontId::new(18.0, theme::strong())),
                );
                ui.label(
                    RichText::new(
                        "The saved profile is removed. The mouse keeps whatever settings it has now.",
                    )
                    .color(TEXT_DIM),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if theme::button(ui, "Keep it").clicked() {
                        self.confirm_delete_profile = None;
                    }
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Delete profile").color(DANGER))
                                .min_size(vec2(0.0, 34.0)),
                        )
                        .clicked()
                    {
                        self.confirm_delete_profile = None;
                        self.delete_profile(&name);
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

/// A column in the middle of the page for a page's cards, no wider than
/// is comfortable to read.
fn column(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui)) {
    let width = ui.available_width().min(theme::PAGE_WIDTH);
    let left = ui.max_rect().center().x - width / 2.0;
    let rect = Rect::from_min_max(
        pos2(left, ui.cursor().top()),
        pos2(left + width, ui.max_rect().bottom()),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::top_down(Align::Min)),
    );
    add_contents(&mut child);
    let used = child.min_rect();
    if used.height() > 0.0 {
        ui.allocate_rect(used, Sense::hover());
        ui.add_space(16.0);
    }
}

/// All the CPI levels on one line, so their spread can be seen at a glance.
/// The line is logarithmic, like the sliders: each doubling is the same
/// distance.
fn cpi_scale(ui: &mut Ui, levels: &[u32], active: usize) {
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width().min(640.0), 58.0), Sense::hover());
    let rail = rect.left() + 14.0..=rect.right() - 14.0;
    let x_of = |cpi: u32| {
        let t = ((cpi as f32).ln() - 100f32.ln()) / (18_000f32.ln() - 100f32.ln());
        egui::lerp(rail.clone(), t.clamp(0.0, 1.0))
    };
    let y = rect.top() + 18.0;
    ui.painter().hline(rail.clone(), y, Stroke::new(2.0, LINE));
    for tick in [100u32, 200, 400, 800, 1600, 3200, 6400, 18_000] {
        let x = x_of(tick);
        ui.painter().line_segment(
            [pos2(x, y + 5.0), pos2(x, y + 10.0)],
            Stroke::new(1.0, LINE),
        );
        ui.painter().text(
            pos2(x, y + 22.0),
            Align2::CENTER_CENTER,
            tick.to_string(),
            FontId::proportional(10.5),
            TEXT_FAINT,
        );
    }
    // Draw the active level last so it sits on top of any it overlaps.
    let mut order: Vec<usize> = (0..levels.len()).collect();
    order.sort_by_key(|&i| i == active);
    for i in order {
        let centre = pos2(x_of(levels[i]), y);
        let (fill, ink) = if i == active {
            (ACCENT, theme::ON_ACCENT)
        } else {
            (theme::RAISED_HOVER, TEXT)
        };
        ui.painter().circle_filled(centre, 10.0, CARD);
        ui.painter().circle_filled(centre, 8.5, fill);
        ui.painter().text(
            centre,
            Align2::CENTER_CENTER,
            (i + 1).to_string(),
            FontId::new(11.0, theme::strong()),
            ink,
        );
    }
}

/// The marker at the start of a CPI row: shows which level is active and
/// makes one active when clicked.
fn level_marker(ui: &mut Ui, active: bool, number: usize) -> bool {
    let label = format!("Level {number}");
    let (rect, response) = ui.allocate_exact_size(vec2(86.0, 32.0), Sense::click());
    response.widget_info(|| WidgetInfo::selected(WidgetType::RadioButton, true, active, &label));
    theme::focus_ring(ui, &response, rect, theme::CONTROL_RADIUS);
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

/// What a list control tells assistive technology: its name, which is
/// written above it and not part of it, and the choice it shows.
fn named_list(name: &str, chosen: &str) -> WidgetInfo {
    let mut info = WidgetInfo::labeled(WidgetType::ComboBox, true, name);
    info.current_text_value = Some(chosen.to_string());
    info
}

/// What to tell someone whose mouse is plugged in but will not open.
pub(super) fn blocked_advice(reason: &str) -> String {
    let mut text = format!(
        "Your mouse is plugged in, but this computer would not let ggez open it. It said: {reason}"
    );
    if cfg!(target_os = "macos") {
        text.push_str(
            "\nIf macOS is refusing, allow ggez under System Settings, Privacy & Security, Input Monitoring, then open ggez again.",
        );
    } else if cfg!(target_os = "linux") {
        text.push_str(
            "\nOn Linux this usually means the udev rule from the README is not installed yet.",
        );
    } else {
        text.push_str(
            "\nAnother program may be holding it. Closing SteelSeries GG is worth trying.",
        );
    }
    text
}

/// The mark on the built-in list controls, to match the custom ones.
fn list_mark(
    ui: &Ui,
    rect: Rect,
    _visuals: &egui::style::WidgetVisuals,
    _is_open: bool,
    _above_or_below: egui::AboveOrBelow,
) {
    theme::chevron(ui.painter(), rect.center(), TEXT_DIM);
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
    Macro,
    DpiCycle,
    ScrollUp,
    ScrollDown,
    Disabled,
}

impl Kind {
    const ALL: [Kind; 8] = [
        Kind::Keyboard,
        Kind::Mouse,
        Kind::Media,
        Kind::Macro,
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
            Kind::Macro => "Macro: several steps",
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
            // A macro is not a binding on the mouse; ggez picks the key
            // the button sends.
            Kind::Macro => Action::Disabled,
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
        Action::MouseButton(5) => "Forward".into(),
        Action::MouseButton(n) => format!("Button {n}"),
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
