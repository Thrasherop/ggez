//! ggez: configure a SteelSeries Aerox 9 Wireless.
//!
//! `ggez-gui` opens the window. `ggez-gui --background` runs only the
//! helper that keeps the saved profile applied, with no window; that is
//! what starts at login. A window that closes while "start at login" is
//! on hands the helper over to a background process, so that the helper
//! is around from then on as it would be after logging in. On Windows the
//! helper shows an icon in the notification area.

// No console window on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod device;
mod draft;
mod icon;
mod mouse_view;
mod theme;
// Only Windows has an icon to drive so far; elsewhere this is tested but
// not yet called.
#[cfg_attr(not(windows), allow(dead_code))]
mod tray;
#[cfg(windows)]
mod tray_windows;
mod updates;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui;
use ggez_service::update::{self, Relaunch};
use ggez_service::{autostart, helper, paths};

/// The window's title. The notification-area icon finds the window by it.
const WINDOW_TITLE: &str = "ggez";

struct DesktopHost {
    /// Where the window leaves what is to be started once it has closed.
    relaunch: Arc<Mutex<Option<Relaunch>>>,
}

impl app::Host for DesktopHost {
    fn autostart_enabled(&self) -> Result<bool, String> {
        autostart::location()
            .map(|l| l.is_some())
            .map_err(|e| format!("{e:#}"))
    }

    fn set_autostart(&mut self, enabled: bool) -> Result<(), String> {
        let result = if enabled {
            autostart::Launch::for_current_exe().and_then(|launch| autostart::enable(&launch))
        } else {
            autostart::disable()
        };
        result.map_err(|e| format!("{e:#}"))
    }

    fn pick_save_path(&mut self) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .add_filter("ggez profile", &["toml"])
            .set_file_name("ggez-profile.toml")
            .save_file()
    }

    fn pick_open_path(&mut self) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .add_filter("ggez profile", &["toml"])
            .pick_file()
    }

    fn macros_here(&self) -> app::MacrosHere {
        use ggez_service::macro_host::Host;
        app::MacrosHere {
            supported: Host::supported(),
            permitted: Host::permitted(),
            limit: Host::max_macros(),
        }
    }

    fn ask_to_run_macros(&mut self) {
        ggez_service::macro_host::Host::ask_permission();
    }

    fn can_update(&self) -> Result<(), String> {
        update::can_install()
    }

    fn open_page(&mut self, url: &str) {
        let mut opener = if cfg!(windows) {
            let mut opener = std::process::Command::new("rundll32");
            opener.arg("url.dll,FileProtocolHandler");
            opener
        } else if cfg!(target_os = "macos") {
            std::process::Command::new("open")
        } else {
            std::process::Command::new("xdg-open")
        };
        let _ = opener.arg(url).spawn();
    }

    fn restart_with(&mut self, relaunch: Relaunch) {
        if let Ok(mut place) = self.relaunch.lock() {
            *place = Some(relaunch);
        }
    }

    fn reveal(&mut self, path: &Path) {
        let folder = path.parent().unwrap_or(path);
        let _ = std::fs::create_dir_all(folder);
        let opener = if cfg!(windows) {
            "explorer"
        } else if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        let _ = std::process::Command::new(opener).arg(folder).spawn();
    }
}

/// Run the helper on this thread until it is told to stop.
#[cfg(not(windows))]
fn run_helper_forever(profile: &Path) {
    helper::run(profile, Arc::new(AtomicBool::new(false)), |_event| {});
}

/// `ggez-gui --background`: the helper with no window.
fn run_background(profile: Option<PathBuf>) {
    // A window that is closing hands the helper over to this process and
    // needs a moment to let go of it. Only one helper runs per user, so if
    // the lock stays taken there is nothing to do.
    let lock = (0..20).find_map(|attempt| {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        helper::InstanceLock::acquire().ok().flatten()
    });
    let (Some(_lock), Some(profile)) = (lock, profile) else {
        return;
    };
    #[cfg(windows)]
    run_with_icon(&profile);
    #[cfg(not(windows))]
    run_helper_forever(&profile);
}

/// The helper on its own thread, and on this one the notification-area
/// icon that shows it is running, until the user chooses Quit.
#[cfg(windows)]
fn run_with_icon(profile: &Path) {
    use ggez_service::profiles::Store;
    use tray::{Choice, Popup};
    use tray_windows::{Click, Icon};

    let (sender, events) = std::sync::mpsc::channel();
    let helper_profile = profile.to_path_buf();
    std::thread::spawn(move || {
        helper::run(&helper_profile, Arc::new(AtomicBool::new(false)), |event| {
            let _ = sender.send(event.clone());
        });
    });
    let (found, newer_versions) = std::sync::mpsc::channel();
    watch_for_updates(found);

    let mut state = tray::Tray::default();
    let Ok(mut icon) = Icon::new(&state.tooltip()) else {
        // No icon, but the helper still has its job to do.
        loop {
            std::thread::park();
        }
    };
    let store = profile.parent().map(|dir| Store::new(dir.to_path_buf()));
    while let Some(clicks) = icon.wait(500) {
        loop {
            match events.try_recv() {
                Ok(event) => {
                    if let Some(popup) = state.on_event(&event) {
                        icon.notify(&popup);
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                // The helper has stopped: it was told to, so that an
                // update can replace this program. The icon goes with it.
                Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
            }
        }
        for version in newer_versions.try_iter() {
            if let Some(popup) = state.on_newer_version(version) {
                icon.notify(&popup);
            }
        }
        icon.set_tooltip(&state.tooltip());
        for click in clicks {
            let choice = match click {
                Click::Primary => Some(Choice::OpenWindow),
                Click::Menu => {
                    let names = store.as_ref().map(Store::list).unwrap_or_default();
                    let current = store.as_ref().and_then(Store::current);
                    icon.show_menu(&state.menu(&names, current.as_deref()))
                }
            };
            match choice {
                Some(Choice::OpenWindow) => open_window(),
                Some(Choice::UseProfile(name)) => {
                    // The helper notices the changed file and sends it.
                    let switched = store
                        .as_ref()
                        .map_or(Ok(()), |store| store.activate(&name, profile));
                    if let Err(e) = switched {
                        icon.notify(&Popup {
                            title: format!("Could not switch to {name}"),
                            text: format!("{e:#}"),
                            warning: true,
                        });
                    }
                }
                Some(Choice::Quit) => return,
                None => {}
            }
        }
    }
}

/// Look for a newer ggez now and then while only the icon is running, and
/// report one that is found. The record is shared with the window, so
/// between them they look once a day, and not at all if that is switched
/// off in Settings.
#[cfg(windows)]
fn watch_for_updates(found: std::sync::mpsc::Sender<update::Version>) {
    use std::time::{Duration, SystemTime};
    use update::{Https, Record, Version};

    std::thread::spawn(move || loop {
        if let Ok(path) = Record::path() {
            let mut record = Record::load(&path);
            if record.due(SystemTime::now()) {
                if let Ok(release) = update::latest(&Https) {
                    // Read again: the setting may have changed meanwhile.
                    record = Record::load(&path);
                    record.found(release.version, SystemTime::now());
                    let _ = record.save(&path);
                }
            }
            if let Some(version) = record.newer_than(Version::current()) {
                if found.send(version).is_err() {
                    return;
                }
            }
        }
        std::thread::sleep(Duration::from_secs(60 * 60));
    });
}

/// Show the ggez window: the one already open, or a new one.
#[cfg(windows)]
fn open_window() {
    if tray_windows::bring_to_front(WINDOW_TITLE) {
        return;
    }
    if let Ok(program) = std::env::current_exe() {
        let _ = std::process::Command::new(program).spawn();
    }
}

/// Whether closing the window should leave the helper running: only if
/// the user asked for the helper to be around by turning on "start at
/// login". It is then what would be running had they just logged in, with
/// an icon on Windows and without one elsewhere.
fn stays_after_close(helper_running_here: bool) -> bool {
    helper_running_here && autostart::location().is_ok_and(|location| location.is_some())
}

struct Window(app::App);

impl eframe::App for Window {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.0.ui(ctx);
    }
}

fn main() -> anyhow::Result<()> {
    let profile_path = paths::profile_path().ok();
    let background = std::env::args().any(|arg| arg == "--background");

    if background {
        run_background(profile_path);
        return Ok(());
    }
    // Only one helper per user; whoever holds the lock runs it.
    let lock = helper::InstanceLock::acquire().ok().flatten();
    let helper_running_here = lock.is_some();
    let stop_helper = Arc::new(AtomicBool::new(false));
    let helper_thread = match (helper_running_here, profile_path.clone()) {
        (true, Some(path)) => {
            let stop = stop_helper.clone();
            Some(std::thread::spawn(move || {
                helper::run(&path, stop, |_event| {})
            }))
        }
        _ => None,
    };
    let relaunch = Arc::new(Mutex::new(None));
    let host = DesktopHost {
        relaunch: relaunch.clone(),
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(WINDOW_TITLE)
            .with_icon(Arc::new(egui::IconData {
                rgba: icon::rgba(128),
                width: 128,
                height: 128,
            }))
            .with_inner_size([1080.0, 760.0])
            .with_min_inner_size([1020.0, 640.0]),
        ..Default::default()
    };
    eframe::run_native(
        "ggez",
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx);
            let ctx = cc.egui_ctx.clone();
            let worker = device::Worker::spawn(move || ctx.request_repaint());
            let ctx = cc.egui_ctx.clone();
            let updater = updates::Worker::spawn(move || ctx.request_repaint());
            Ok(Box::new(Window(
                app::App::new(
                    Box::new(worker),
                    Box::new(host),
                    profile_path,
                    helper_running_here,
                )
                .with_updater(Box::new(updater)),
            )))
        }),
    )
    .map_err(|e| anyhow::anyhow!("could not open the window: {e}"))?;
    let relaunch = relaunch.lock().ok().and_then(|mut place| place.take());
    if let Some(relaunch) = relaunch {
        // An update takes over from here. The helper is the old program,
        // so it finishes first: the one in this process, or one in
        // another, which is told to stop and waited for.
        stop_helper.store(true, Ordering::Relaxed);
        if let Some(thread) = helper_thread {
            let _ = thread.join();
        }
        drop(lock);
        helper::stop_running(std::time::Duration::from_secs(15));
        return relaunch.start();
    }
    // Keep the lock until the window closes.
    drop(lock);
    if stays_after_close(helper_running_here) {
        if let Ok(program) = std::env::current_exe() {
            let _ = std::process::Command::new(program)
                .arg("--background")
                .spawn();
        }
    }
    Ok(())
}
