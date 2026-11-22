//! Drives the window the way a user would, with a stand-in for the mouse.

use std::sync::{Arc, Mutex};

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use super::*;

#[derive(Default)]
struct Shared {
    requests: Vec<Request>,
    replies: Vec<Reply>,
    autostart: bool,
    save_path: Option<PathBuf>,
    open_path: Option<PathBuf>,
}

#[derive(Clone, Default)]
struct Fake(Arc<Mutex<Shared>>);

impl Device for Fake {
    fn send(&mut self, request: Request) {
        self.0.lock().unwrap().requests.push(request);
    }
    fn poll(&mut self) -> Vec<Reply> {
        std::mem::take(&mut self.0.lock().unwrap().replies)
    }
}

impl Host for Fake {
    fn autostart_enabled(&self) -> Result<bool, String> {
        Ok(self.0.lock().unwrap().autostart)
    }
    fn set_autostart(&mut self, enabled: bool) -> Result<(), String> {
        self.0.lock().unwrap().autostart = enabled;
        Ok(())
    }
    fn pick_save_path(&mut self) -> Option<PathBuf> {
        self.0.lock().unwrap().save_path.clone()
    }
    fn pick_open_path(&mut self) -> Option<PathBuf> {
        self.0.lock().unwrap().open_path.clone()
    }
    fn reveal(&mut self, _path: &std::path::Path) {}
}

const CONNECTED: Status = Status::Connected {
    link: Link::Wireless,
    battery: Some((80, false)),
    firmware: None,
};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ggez-gui-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A window with a connected mouse and its profile in a scratch folder.
fn window(name: &str) -> (Harness<'static, App>, Fake, PathBuf) {
    let fake = Fake::default();
    fake.0
        .lock()
        .unwrap()
        .replies
        .push(Reply::Status(CONNECTED));
    let profile = temp_dir(name).join("profile.toml");
    let app = App::new(
        Box::new(fake.clone()),
        Box::new(fake.clone()),
        Some(profile.clone()),
        true,
    );
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .build_state(|ctx, app: &mut App| app.ui(ctx), app);
    harness.run();
    (harness, fake, profile)
}

fn click(harness: &mut Harness<'static, App>, label: &str) {
    harness
        .get_by_role_and_label(egui::accesskit::Role::Button, label)
        .click();
    // Not `run`: that waits for the window to stop animating, and the
    // spinner shown while applying never does.
    harness.run_steps(4);
}

fn requests(fake: &Fake) -> Vec<Request> {
    std::mem::take(&mut fake.0.lock().unwrap().requests)
}

fn reply(fake: &Fake, reply: Reply) {
    fake.0.lock().unwrap().replies.push(reply);
}

#[test]
fn every_tab_opens() {
    let (mut harness, _, _) = window("tabs");
    for (tab, name) in Tab::ALL {
        click(&mut harness, name);
        assert_eq!(harness.state().tab, tab);
    }
}

#[test]
fn apply_sends_the_draft_and_writes_the_profile_file() {
    let (mut harness, fake, profile) = window("apply");
    assert!(!harness.state().is_dirty());
    click(&mut harness, "250 Hz");
    assert_eq!(harness.state().draft.polling, 250);
    assert!(harness.state().is_dirty());

    click(&mut harness, "Apply to mouse");
    let expected = harness.state().draft.to_profile();
    assert_eq!(requests(&fake), [Request::Apply(expected.clone())]);
    assert_eq!(Profile::load(&profile).unwrap(), expected);
    assert!(!harness.state().is_dirty());

    reply(&fake, Reply::Applied(Ok(())));
    harness.run();
    harness.get_by_label("Applied and saved to the mouse.");
}

#[test]
fn failed_apply_is_shown_and_stays_pending() {
    let (mut harness, fake, _) = window("fail");
    click(&mut harness, "500 Hz");
    click(&mut harness, "Apply to mouse");
    reply(&fake, Reply::Applied(Err("the dongle has no link".into())));
    harness.run();
    harness.get_by_label("Could not apply: the dongle has no link");
    assert!(harness.state().is_dirty());
}

#[test]
fn apply_is_unavailable_without_a_mouse() {
    let (mut harness, fake, _) = window("offline");
    reply(&fake, Reply::Status(Status::Asleep));
    harness.run();
    harness.get_by_label("Mouse is asleep or off");
    click(&mut harness, "125 Hz");
    click(&mut harness, "Apply to mouse");
    assert!(requests(&fake).is_empty());
}

#[test]
fn revert_discards_changes() {
    let (mut harness, _, _) = window("revert");
    click(&mut harness, "125 Hz");
    click(&mut harness, "Revert");
    assert_eq!(harness.state().draft.polling, 1000);
    assert!(!harness.state().is_dirty());
}

#[test]
fn cpi_levels_can_be_removed_and_added() {
    let (mut harness, _, _) = window("cpi");
    harness
        .get_all_by_role_and_label(egui::accesskit::Role::Button, "Remove")
        .next()
        .unwrap()
        .click();
    harness.run();
    assert_eq!(harness.state().draft.dpi, [800, 1200, 2400, 3200]);
    assert_eq!(
        harness.state().draft.dpi_selected,
        0,
        "the active level keeps pointing at 800"
    );
    click(&mut harness, "Add level");
    assert_eq!(harness.state().draft.dpi, [800, 1200, 2400, 3200, 6400]);
    harness.state().draft.to_profile().validate().unwrap();
}

#[test]
fn removing_left_click_needs_confirmation() {
    let (mut harness, fake, _) = window("left");
    click(&mut harness, "Buttons");
    harness.state_mut().select_button(Button::Left);
    harness.state_mut().set_action(Action::Disabled);
    harness.run();

    click(&mut harness, "Apply to mouse");
    harness.get_by_label("No button does a left click");
    assert!(requests(&fake).is_empty());
    click(&mut harness, "Go back");
    assert!(requests(&fake).is_empty());

    click(&mut harness, "Apply to mouse");
    click(&mut harness, "Apply anyway");
    assert_eq!(requests(&fake).len(), 1);
}

#[test]
fn a_key_press_becomes_the_binding() {
    let (mut harness, _, _) = window("capture");
    click(&mut harness, "Buttons");
    click(&mut harness, "Press to set");
    harness.press_key_modifiers(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::K);
    harness.run();
    let action = harness.state().draft.buttons.get(Button::Grid(1));
    assert_eq!(action.to_string(), "key:ctrl+shift+k");
    assert_eq!(describe(action), "Ctrl + Shift + K");
    assert!(!harness.state().capturing_key);
}

#[test]
fn modifier_toggles_build_a_shortcut() {
    let (mut harness, _, _) = window("modifiers");
    click(&mut harness, "Buttons");
    click(&mut harness, "Ctrl");
    click(&mut harness, "Alt");
    let grid1 =
        |harness: &Harness<'static, App>| harness.state().draft.buttons.get(Button::Grid(1));
    assert_eq!(grid1(&harness).to_string(), "key:ctrl+alt+1");
    click(&mut harness, "Ctrl");
    assert_eq!(grid1(&harness).to_string(), "key:alt+1");
    click(&mut harness, "Reset this button");
    assert_eq!(grid1(&harness), ButtonMap::default().get(Button::Grid(1)));
}

#[test]
fn reset_all_buttons_asks_first() {
    let (mut harness, _, _) = window("reset");
    click(&mut harness, "Buttons");
    click(&mut harness, "Shift");
    click(&mut harness, "Reset all buttons");
    click(&mut harness, "Cancel");
    assert_ne!(harness.state().draft.buttons, ButtonMap::default());
    click(&mut harness, "Reset all buttons");
    click(&mut harness, "Reset");
    assert_eq!(harness.state().draft.buttons, ButtonMap::default());
}

#[test]
fn lighting_changes_are_previewed_only_when_asked() {
    let (mut harness, fake, _) = window("preview");
    click(&mut harness, "Lighting");
    click(&mut harness, "Static colors");
    assert_eq!(
        requests(&fake),
        [Request::Preview(harness.state().draft.to_profile())]
    );

    harness.state_mut().live_preview = false;
    click(&mut harness, "Rainbow");
    assert!(requests(&fake).is_empty());
}

#[test]
fn export_then_import_restores_the_settings() {
    let (mut harness, fake, _) = window("export");
    let file = temp_dir("export-file").join("mine.toml");
    fake.0.lock().unwrap().save_path = Some(file.clone());
    fake.0.lock().unwrap().open_path = Some(file.clone());

    click(&mut harness, "250 Hz");
    let exported = harness.state().draft.clone();
    click(&mut harness, "Settings");
    click(&mut harness, "Export…");
    assert!(file.exists());

    click(&mut harness, "Factory settings");
    assert_eq!(harness.state().draft, Draft::default());
    click(&mut harness, "Import…");
    assert_eq!(harness.state().draft, exported);
}

#[test]
fn importing_a_broken_file_changes_nothing() {
    let (mut harness, fake, _) = window("bad-import");
    let file = temp_dir("bad-import-file").join("broken.toml");
    std::fs::write(&file, "polling = 7\n").unwrap();
    fake.0.lock().unwrap().open_path = Some(file);
    click(&mut harness, "Settings");
    click(&mut harness, "Import…");
    assert_eq!(harness.state().draft, Draft::default());
    assert!(matches!(harness.state().notice, Notice::Bad(_)));
}

#[test]
fn login_checkbox_reaches_the_host() {
    let (mut harness, fake, _) = window("autostart");
    click(&mut harness, "Settings");
    harness
        .get_by_label("Start the helper when I log in")
        .click();
    harness.run();
    assert!(fake.0.lock().unwrap().autostart);
}

#[test]
fn saved_profile_is_loaded_at_start() {
    let path = temp_dir("load").join("profile.toml");
    std::fs::write(&path, "polling = 500\n[buttons]\ng3 = \"key:f13\"\n").unwrap();
    let fake = Fake::default();
    let app = App::new(Box::new(fake.clone()), Box::new(fake), Some(path), false);
    assert_eq!(app.draft.polling, 500);
    assert_eq!(
        app.draft.buttons.get(Button::Grid(3)).to_string(),
        "key:f13"
    );
    assert!(!app.is_dirty());
}

#[test]
fn descriptions_read_naturally() {
    for (text, expected) in [
        ("key:ctrl+v", "Ctrl + V"),
        ("key:f13", "F13"),
        ("key:page-up", "Page Up"),
        ("key:minus", "-"),
        ("left", "Left click"),
        ("media:play-pause", "Play / Pause"),
        ("disabled", "Disabled"),
    ] {
        assert_eq!(describe(text.parse().unwrap()), expected, "{text}");
    }
}

/// Found by hand: the windowing layer delivers Ctrl+C, Ctrl+X and Ctrl+V as
/// clipboard events, not key presses, so they need their own handling.
#[test]
fn clipboard_shortcuts_can_be_captured() {
    let modifier = if cfg!(target_os = "macos") {
        "win"
    } else {
        "ctrl"
    };
    for (event, key) in [
        (egui::Event::Copy, "c"),
        (egui::Event::Cut, "x"),
        (egui::Event::Paste("clipboard text".into()), "v"),
    ] {
        let (mut harness, _, _) = window(&format!("clipboard-{key}"));
        click(&mut harness, "Buttons");
        click(&mut harness, "Press to set");
        harness.input_mut().events.push(event);
        harness.run();
        assert_eq!(
            harness
                .state()
                .draft
                .buttons
                .get(Button::Grid(1))
                .to_string(),
            format!("key:{modifier}+{key}")
        );
    }
}

/// Found by hand: the export confirmation was hidden behind the "unsaved
/// changes" hint whenever there were pending edits.
#[test]
fn export_is_confirmed_even_with_pending_changes() {
    let (mut harness, fake, _) = window("export-notice");
    let file = temp_dir("export-notice-file").join("mine.toml");
    fake.0.lock().unwrap().save_path = Some(file.clone());
    click(&mut harness, "250 Hz");
    click(&mut harness, "Settings");
    click(&mut harness, "Export…");
    harness.get_by_label(&format!("Exported to {}", file.display()));

    // Editing afterwards makes the message stale, so it goes away.
    click(&mut harness, "Sensitivity");
    click(&mut harness, "500 Hz");
    harness.get_by_label("You have changes that are not on the mouse yet.");
}
