//! ggez: configure a SteelSeries Aerox 9 Wireless.
//!
//! `ggez-gui` opens the window. `ggez-gui --background` runs only the
//! helper that keeps the saved profile applied, with no window; that is
//! what starts at login.

// No console window on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod device;
mod draft;

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use eframe::egui;
use ggez_service::{autostart, helper, paths};

struct DesktopHost;

impl app::Host for DesktopHost {
    fn autostart_enabled(&self) -> Result<bool, String> {
        autostart::location().map(|l| l.is_some()).map_err(|e| format!("{e:#}"))
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
        rfd::FileDialog::new().add_filter("ggez profile", &["toml"]).pick_file()
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

/// Run the helper on this thread until the process exits.
fn run_helper_forever(profile: &Path) {
    helper::run(profile, Arc::new(AtomicBool::new(false)), |_event| {});
}

struct Window(app::App);

impl eframe::App for Window {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.0.ui(ctx);
    }
}

fn style(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
        style.spacing.interact_size.y = 28.0;
        for (_, font) in style.text_styles.iter_mut() {
            font.size *= 1.12;
        }
        let selection = egui::Color32::from_rgb(20, 110, 100);
        style.visuals.selection.bg_fill = selection;
        style.visuals.selection.stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
        style.visuals.hyperlink_color = egui::Color32::from_rgb(45, 212, 191);
    });
}

fn main() -> anyhow::Result<()> {
    let profile_path = paths::profile_path().ok();
    let background = std::env::args().any(|arg| arg == "--background");

    // Only one helper per user; whoever holds the lock runs it.
    let lock = helper::InstanceLock::acquire().ok().flatten();
    if background {
        if let (Some(_lock), Some(path)) = (&lock, &profile_path) {
            run_helper_forever(path);
        }
        return Ok(());
    }
    let helper_running_here = lock.is_some();
    if let (true, Some(path)) = (helper_running_here, profile_path.clone()) {
        std::thread::spawn(move || run_helper_forever(&path));
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("ggez")
            .with_inner_size([1000.0, 700.0])
            .with_min_inner_size([860.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native(
        "ggez",
        options,
        Box::new(move |cc| {
            style(&cc.egui_ctx);
            let ctx = cc.egui_ctx.clone();
            let worker = device::Worker::spawn(move || ctx.request_repaint());
            Ok(Box::new(Window(app::App::new(
                Box::new(worker),
                Box::new(DesktopHost),
                profile_path,
                helper_running_here,
            ))))
        }),
    )
    .map_err(|e| anyhow::anyhow!("could not open the window: {e}"))?;
    // Keep the lock until the window closes.
    drop(lock);
    Ok(())
}
