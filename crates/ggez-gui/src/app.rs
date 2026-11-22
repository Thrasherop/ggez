//! The window: what it knows and what it decides. Drawing is in `view`.

use std::path::PathBuf;

use ggez_core::{Action, Button};
use ggez_service::profiles::Store;
use ggez_service::Profile;

use crate::device::{Device, Reply, Request, Status};
use crate::draft::Draft;
#[cfg(test)]
use {
    eframe::egui,
    ggez_core::{ButtonMap, Link},
};

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
    gg_running: bool,
    /// No saved profile existed at start, so the window shows factory
    /// settings rather than anything known about this mouse.
    first_run: bool,
    notice: Notice,
    applying: bool,
    /// The last apply failed, so the mouse may not match `applied`.
    apply_failed: bool,
    selected_button: Button,
    /// Text box contents for the selected button's action.
    action_text: String,
    /// The next key press becomes the selected button's key.
    capturing_key: bool,
    /// What has been typed into the key list's search box.
    key_filter: String,
    confirm_no_left_click: bool,
    confirm_reset: bool,
    live_preview: bool,
    profile_path: Option<PathBuf>,
    /// Saved, named profiles; `None` if there is nowhere to keep them.
    store: Option<Store>,
    profile_names: Vec<String>,
    /// The named profile the window is editing, if any.
    current_profile: Option<String>,
    /// What has been typed as the name for a new profile.
    new_profile_name: String,
    confirm_delete_profile: Option<String>,
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
        let first_run = profile_path.as_deref().is_some_and(|p| !p.exists());
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
        let store = profile_path
            .as_deref()
            .and_then(|path| path.parent())
            .map(|dir| Store::new(dir.to_path_buf()));
        let profile_names = store.as_ref().map(Store::list).unwrap_or_default();
        let current_profile = store.as_ref().and_then(Store::current);
        let selected_button = Button::Grid(1);
        Self {
            tab: Tab::Sensitivity,
            action_text: draft.buttons.get(selected_button).to_string(),
            applied: draft.clone(),
            seen: draft.clone(),
            draft,
            status: Status::Unknown,
            gg_running: false,
            first_run,
            notice,
            applying: false,
            apply_failed: false,
            selected_button,
            capturing_key: false,
            key_filter: String::new(),
            confirm_no_left_click: false,
            confirm_reset: false,
            live_preview: true,
            profile_path,
            store,
            profile_names,
            current_profile,
            new_profile_name: String::new(),
            confirm_delete_profile: None,
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
                Reply::GgRunning(running) => self.gg_running = running,
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
        // The named profile in use follows what is on the mouse.
        let named = match (&self.store, &self.current_profile) {
            (Some(store), Some(name)) => store.save(name, &profile),
            _ => Ok(()),
        };
        if let Err(e) = named {
            self.notice = Notice::Bad(format!("{e:#}"));
            return;
        }
        self.applied = self.draft.clone();
        self.first_run = false;
        self.applying = true;
        self.notice = Notice::Working("Applying…".into());
        self.device.send(Request::Apply(profile));
    }

    /// Switch to a saved profile: load it into the window and, if the mouse
    /// is there, send it straight away.
    pub fn switch_profile(&mut self, name: &str) {
        let Some(store) = self.store.clone() else {
            return;
        };
        match store.load(name).and_then(|p| p.validate().map(|()| p)) {
            Ok(profile) => {
                self.draft = Draft::from_profile(&profile);
                self.current_profile = Some(name.to_owned());
                let _ = store.set_current(Some(name));
                self.select_button(self.selected_button);
                self.notice = Notice::None;
                if self.connected() && !self.applying {
                    self.request_apply();
                }
            }
            Err(e) => self.notice = Notice::Bad(format!("Could not load {name}: {e:#}")),
        }
    }

    /// Save what the window shows as a named profile and make it the one
    /// in use. Nothing is sent to the mouse.
    pub fn save_profile_as(&mut self, name: &str) {
        let Some(store) = self.store.clone() else {
            return;
        };
        let saved = ggez_service::profiles::valid_name(name)
            .map(str::to_owned)
            .and_then(|name| {
                store.save(&name, &self.draft.to_profile())?;
                store.set_current(Some(&name))?;
                Ok(name)
            });
        match saved {
            Ok(name) => {
                self.notice = Notice::Good(format!("Saved as {name}."));
                self.current_profile = Some(name);
                self.new_profile_name.clear();
                self.profile_names = store.list();
            }
            Err(e) => self.notice = Notice::Bad(format!("{e:#}")),
        }
    }

    /// Remove a saved profile. The mouse keeps its settings.
    pub fn delete_profile(&mut self, name: &str) {
        let Some(store) = self.store.clone() else {
            return;
        };
        match store.delete(name) {
            Ok(()) => {
                if self.current_profile.as_deref() == Some(name) {
                    self.current_profile = None;
                }
                self.profile_names = store.list();
                self.notice = Notice::Good(format!("Deleted {name}."));
            }
            Err(e) => self.notice = Notice::Bad(format!("{e:#}")),
        }
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
}

mod view;

#[cfg(test)]
use view::describe;

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
