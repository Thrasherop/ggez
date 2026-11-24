//! The window: what it knows and what it decides. Drawing is in `view`.

use std::path::PathBuf;

use ggez_core::{Action, Button};
use ggez_service::profiles::Store;
use ggez_service::update::{Record, Relaunch, Version};
use ggez_service::Profile;

use crate::device::{Device, Reply, Request, Status};
use crate::draft::Draft;
use crate::updates::{self, Updater};
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
    /// Where macros stand on this computer, as of now.
    fn macros_here(&self) -> MacrosHere;
    /// Have the system ask its user to let ggez press keys, and show them
    /// where that is allowed.
    fn ask_to_run_macros(&mut self);
    /// Whether this copy of ggez can replace itself with a newer one. If
    /// not, why not, in words for the person using it.
    fn can_update(&self) -> Result<(), String>;
    /// Show a web page in the person's browser.
    fn open_page(&mut self, url: &str);
    /// Start this once the window has closed: an update takes over.
    fn restart_with(&mut self, relaunch: Relaunch);
}

/// Where the window stands with a newer version of ggez.
#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    /// None is known of.
    None,
    Available(Version),
    /// Being fetched; the amounts are in bytes.
    Downloading {
        version: Version,
        received: u64,
        total: Option<u64>,
    },
    /// Fetched and checked. It takes over when ggez restarts, and that
    /// waits while there are changes that are not on the mouse.
    Ready(Version),
    Failed {
        version: Version,
        problem: String,
    },
}

/// Whether this computer can run macros, and how many.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacrosHere {
    /// ggez knows how to run macros on this kind of system.
    pub supported: bool,
    /// The system lets ggez catch and press keys. A Mac asks its user.
    pub permitted: bool,
    /// How many buttons can have a macro here.
    pub limit: usize,
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
    /// When the profile file was last written, as far as the window knows.
    /// A different time means something else changed it: the icon's menu,
    /// the command line, or an editor.
    profile_stamp: Option<std::time::SystemTime>,
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
    /// The version of ggez this is.
    running: Version,
    pub update: Update,
    /// The banner about a newer version was put away for now.
    update_dismissed: bool,
    looking_for_update: bool,
    /// The look that is under way was asked for, so its failure is told.
    look_was_asked: bool,
    look_problem: Option<String>,
    /// What is remembered about updates between runs, and where.
    update_record: Record,
    update_record_path: Option<PathBuf>,
    can_update: Result<(), String>,
    /// What to start once the window has closed, when an update is ready.
    relaunch: Option<Relaunch>,
    /// The window is to close so that an update can take over.
    closing_for_update: bool,
    device: Box<dyn Device>,
    host: Box<dyn Host>,
    updater: Box<dyn Updater>,
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
        let profile_stamp = profile_path.as_deref().and_then(modified);
        let update_record_path = profile_path
            .as_deref()
            .and_then(|path| path.parent())
            .map(|dir| dir.join("updates.toml"));
        let update_record = update_record_path
            .as_deref()
            .map(Record::load)
            .unwrap_or_default();
        let running = Version::current();
        // What an earlier look found is offered straight away.
        let update = update_record
            .newer_than(running)
            .map_or(Update::None, Update::Available);
        let can_update = host.can_update();
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
            profile_stamp,
            store,
            profile_names,
            current_profile,
            new_profile_name: String::new(),
            confirm_delete_profile: None,
            autostart,
            helper_running_here,
            running,
            update,
            update_dismissed: false,
            looking_for_update: false,
            look_was_asked: false,
            look_problem: None,
            update_record,
            update_record_path,
            can_update,
            relaunch: None,
            closing_for_update: false,
            device,
            host,
            updater: Box::new(updates::Offline),
        }
    }

    /// Give the window its way to the releases page, and have it look for
    /// a newer version if it is time to.
    pub fn with_updater(mut self, updater: Box<dyn Updater>) -> Self {
        self.updater = updater;
        if self.update_record.due(std::time::SystemTime::now()) {
            self.look_for_update(false);
        }
        self
    }

    /// Find out what the newest version is. `asked` says the person asked,
    /// and so wants to hear if it could not be found out.
    fn look_for_update(&mut self, asked: bool) {
        self.look_was_asked |= asked;
        if self.looking_for_update {
            return;
        }
        self.looking_for_update = true;
        self.look_was_asked = asked;
        self.look_problem = None;
        self.updater.send(updates::Request::Check);
    }

    /// Fetch and install the newer version that is on offer.
    fn start_update(&mut self) {
        let version = match &self.update {
            Update::Available(version) | Update::Failed { version, .. } => *version,
            _ => return,
        };
        self.update = Update::Downloading {
            version,
            received: 0,
            total: None,
        };
        self.update_dismissed = false;
        self.updater.send(updates::Request::Install);
    }

    /// Close the window so that the update that is ready can take over.
    fn restart_for_update(&mut self) {
        if let Some(relaunch) = self.relaunch.take() {
            self.host.restart_with(relaunch);
            self.closing_for_update = true;
        }
    }

    fn save_update_record(&mut self) {
        let Some(path) = &self.update_record_path else {
            return;
        };
        if let Err(e) = self.update_record.save(path) {
            self.notice = Notice::Bad(format!("{e:#}"));
        }
    }

    fn handle_update_replies(&mut self) {
        for reply in self.updater.poll() {
            match reply {
                updates::Reply::Latest(Ok(latest)) => {
                    self.looking_for_update = false;
                    self.update_record
                        .found(latest, std::time::SystemTime::now());
                    self.save_update_record();
                    // A look changes what is on offer, never an update
                    // that is already under way or has just failed.
                    if matches!(self.update, Update::None | Update::Available(_)) {
                        let offer = if latest > self.running {
                            Update::Available(latest)
                        } else {
                            Update::None
                        };
                        if offer != self.update {
                            self.update_dismissed = false;
                        }
                        self.update = offer;
                    }
                }
                updates::Reply::Latest(Err(problem)) => {
                    self.looking_for_update = false;
                    if self.look_was_asked {
                        self.look_problem = Some(problem);
                    }
                }
                updates::Reply::Progress { received, total } => {
                    if let Update::Downloading {
                        received: so_far,
                        total: in_all,
                        ..
                    } = &mut self.update
                    {
                        *so_far = received;
                        *in_all = total;
                    }
                }
                updates::Reply::Ready(Ok((version, relaunch))) => {
                    self.relaunch = Some(relaunch);
                    self.update = Update::Ready(version);
                    self.update_dismissed = false;
                    // Restarting throws away what has not been applied,
                    // so that is left for the person to decide.
                    if !self.is_dirty() && !self.applying {
                        self.restart_for_update();
                    }
                }
                updates::Reply::Ready(Err(problem)) => {
                    if let Update::Downloading { version, .. } = self.update {
                        self.update = Update::Failed { version, problem };
                    }
                }
            }
        }
    }

    /// If the profile file was changed by something other than this window,
    /// show what it now holds. Edits in progress here are never thrown
    /// away for it.
    fn follow_outside_changes(&mut self) {
        let Some(path) = self.profile_path.clone() else {
            return;
        };
        let stamp = modified(&path);
        if stamp == self.profile_stamp {
            return;
        }
        self.profile_stamp = stamp;
        if let Some(store) = &self.store {
            self.profile_names = store.list();
            self.current_profile = store.current();
        }
        if self.is_dirty() || self.applying {
            return;
        }
        let Ok(profile) = Profile::load(&path) else {
            return;
        };
        let draft = Draft::from_profile(&profile);
        if draft == self.applied {
            return;
        }
        self.applied = draft.clone();
        self.seen = draft.clone();
        self.draft = draft;
        self.first_run = false;
        self.select_button(self.selected_button);
        self.notice =
            Notice::Good("Your settings were changed outside this window. Showing them.".into());
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
                Reply::Status(status) => {
                    self.status = status;
                    self.follow_outside_changes();
                }
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
        self.profile_stamp = self.profile_path.as_deref().and_then(modified);
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

    /// Give the selected button a macro, or take its macro away.
    fn set_macro(&mut self, on: bool) {
        let name = self.selected_button.to_string();
        if on {
            self.draft.macros.entry(name).or_default();
        } else {
            self.draft.macros.remove(&name);
        }
        self.capturing_key = false;
    }

    fn set_action(&mut self, action: Action) {
        self.draft.buttons.set(self.selected_button, action);
        self.action_text = action.to_string();
    }
}

/// When a file was last written, if it exists.
fn modified(path: &std::path::Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

mod view;

#[cfg(test)]
use view::describe;

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
