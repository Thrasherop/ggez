//! Drives the window the way a user would, with a stand-in for the mouse.

use std::sync::{Arc, Mutex};

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use ggez_service::macros::Mode;

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
        .with_size(egui::vec2(1080.0, 760.0))
        .build_state(
            |ctx, app: &mut App| {
                // The real program installs the theme before the first frame.
                // Fonts set during a frame only exist from the next one, so
                // spend the first frame on that and draw nothing.
                let themed = egui::Id::new("theme-installed");
                if !ctx.data(|data| data.get_temp::<bool>(themed).unwrap_or(false)) {
                    crate::theme::apply(ctx);
                    ctx.data_mut(|data| data.insert_temp(themed, true));
                    ctx.request_repaint();
                    return;
                }
                app.ui(ctx)
            },
            app,
        );
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
fn a_mouse_that_will_not_open_is_not_called_unplugged() {
    let (mut harness, fake, _) = window("blocked");
    reply(
        &fake,
        Reply::Status(Status::Blocked("not permitted".into())),
    );
    harness.run();
    harness.get_by_label("Cannot open the mouse");
    // The system's own words are passed on, not swallowed.
    harness.get_by_label_contains("It said: not permitted");
    assert!(harness.query_by_label("Not connected").is_none());
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

#[test]
fn first_run_says_the_window_shows_factory_settings() {
    let (mut harness, _, _) = window("first-run");
    assert!(harness.state().first_run);
    click(&mut harness, "Got it");
    assert!(!harness.state().first_run);
    assert!(harness
        .query_by_role_and_label(egui::accesskit::Role::Button, "Got it")
        .is_none());
}

#[test]
fn applying_ends_the_first_run_notice() {
    let (mut harness, _, _) = window("first-run-apply");
    click(&mut harness, "Apply to mouse");
    assert!(!harness.state().first_run);
}

#[test]
fn an_existing_profile_means_no_first_run_notice() {
    let path = temp_dir("not-first").join("profile.toml");
    std::fs::write(&path, "polling = 500\n").unwrap();
    let fake = Fake::default();
    let app = App::new(Box::new(fake.clone()), Box::new(fake), Some(path), false);
    assert!(!app.first_run);
}

#[test]
fn a_running_gg_is_called_out() {
    let (mut harness, fake, _) = window("gg");
    assert!(harness.query_by_label(ggez_service::gg::ADVICE).is_none());
    reply(&fake, Reply::GgRunning(true));
    harness.run();
    harness.get_by_label(ggez_service::gg::ADVICE);
    reply(&fake, Reply::GgRunning(false));
    harness.run();
    assert!(harness.query_by_label(ggez_service::gg::ADVICE).is_none());
}

#[test]
fn clicking_the_drawing_selects_a_button() {
    let (mut harness, _, _) = window("diagram");
    click(&mut harness, "Buttons");
    for (label, button) in [
        ("Side button 5", Button::Grid(5)),
        ("Left button", Button::Left),
        ("Wheel click", Button::Middle),
        ("Wheel tilt right", Button::TiltRight),
        ("Scroll down", Button::ScrollDown),
        ("CPI button", Button::Dpi),
        ("Side button 12", Button::Grid(12)),
    ] {
        click(&mut harness, label);
        assert_eq!(harness.state().selected_button, button, "{label}");
    }
    // The editor now works on the button that was picked.
    click(&mut harness, "Ctrl");
    assert_eq!(
        harness
            .state()
            .draft
            .buttons
            .get(Button::Grid(12))
            .to_string(),
        "key:ctrl+equals"
    );
}

#[test]
fn every_button_in_the_drawing_can_be_reached() {
    let (mut harness, _, _) = window("diagram-all");
    click(&mut harness, "Buttons");
    for button in Button::ALL {
        harness.state_mut().select_button(Button::Grid(1));
        harness.state_mut().select_button(button);
        harness.run();
        assert_eq!(harness.state().selected_button, button);
    }
    // Twenty controls in the drawing, each exposed by name.
    for name in [
        "Right button",
        "Wheel tilt left",
        "Scroll up",
        "Side button 1",
    ] {
        harness.get_by_role_and_label(egui::accesskit::Role::Button, name);
    }
}

#[test]
fn the_active_cpi_level_can_be_chosen() {
    let (mut harness, _, _) = window("active-level");
    assert_eq!(harness.state().draft.dpi_selected, 1);
    harness.get_by_label("Level 4").click();
    harness.run();
    assert_eq!(harness.state().draft.dpi_selected, 3);
}

#[test]
fn click_flash_can_be_switched_on() {
    let (mut harness, fake, _) = window("flash");
    click(&mut harness, "Lighting");
    requests(&fake);
    harness.get_by_label("Flash on click").click();
    // The rainbow preview animates, so the window never comes to rest.
    harness.run_steps(4);
    assert_eq!(harness.state().draft.reactive, Some([255, 255, 255]));
    assert_eq!(
        requests(&fake).len(),
        1,
        "the change is previewed on the mouse"
    );
}

/// The custom-drawn controls must work without a pointer: reachable with
/// Tab, operated with Enter or Space.
#[test]
fn custom_controls_work_from_the_keyboard() {
    let (mut harness, _, _) = window("keyboard");

    // A sidebar entry.
    harness
        .get_by_role_and_label(egui::accesskit::Role::Button, "Buttons")
        .focus();
    harness.run();
    harness.press_key(egui::Key::Enter);
    harness.run();
    assert_eq!(harness.state().tab, Tab::Buttons);

    // A control in the mouse drawing.
    harness
        .get_by_role_and_label(egui::accesskit::Role::Button, "Side button 7")
        .focus();
    harness.run();
    harness.press_key(egui::Key::Space);
    harness.run();
    assert_eq!(harness.state().selected_button, Button::Grid(7));

    // A segmented choice and the main button.
    harness.state_mut().tab = Tab::Sensitivity;
    harness.run();
    harness
        .get_by_role_and_label(egui::accesskit::Role::Button, "500 Hz")
        .focus();
    harness.run();
    harness.press_key(egui::Key::Enter);
    harness.run();
    assert_eq!(harness.state().draft.polling, 500);

    // An on/off switch.
    harness.state_mut().tab = Tab::Settings;
    harness.run();
    harness
        .get_by_label("Start the helper when I log in")
        .focus();
    harness.run();
    harness.press_key(egui::Key::Space);
    harness.run();
    assert_eq!(harness.state().autostart, Ok(true));
}

#[test]
fn settings_can_be_saved_as_named_profiles_and_switched() {
    let (mut harness, fake, profile) = window("profiles");
    let store = Store::new(profile.parent().unwrap().to_path_buf());

    // Save the factory settings as "Work".
    harness.state_mut().save_profile_as("Work");
    assert_eq!(harness.state().current_profile.as_deref(), Some("Work"));
    assert_eq!(store.current().as_deref(), Some("Work"));
    assert!(
        requests(&fake).is_empty(),
        "saving sends nothing to the mouse"
    );

    // Change something and save that as "Games".
    click(&mut harness, "250 Hz");
    harness.state_mut().save_profile_as("Games");
    assert_eq!(harness.state().profile_names, ["Games", "Work"]);
    assert_eq!(store.load("Games").unwrap().polling, Some(250));
    assert_eq!(store.load("Work").unwrap().polling, Some(1000));

    // Switching loads the profile and sends it to the mouse.
    harness.state_mut().switch_profile("Work");
    harness.run_steps(2);
    assert_eq!(harness.state().draft.polling, 1000);
    assert_eq!(
        requests(&fake),
        [Request::Apply(store.load("Work").unwrap())]
    );
    assert_eq!(store.current().as_deref(), Some("Work"));
}

#[test]
fn applying_updates_the_profile_in_use() {
    let (mut harness, _, profile) = window("profile-follows");
    let store = Store::new(profile.parent().unwrap().to_path_buf());
    harness.state_mut().save_profile_as("Work");
    click(&mut harness, "500 Hz");
    assert_eq!(store.load("Work").unwrap().polling, Some(1000));
    click(&mut harness, "Apply to mouse");
    assert_eq!(store.load("Work").unwrap().polling, Some(500));
}

#[test]
fn the_picker_lists_profiles_and_switches_on_click() {
    let (mut harness, fake, _) = window("picker");
    harness.state_mut().save_profile_as("Work");
    click(&mut harness, "125 Hz");
    harness.state_mut().save_profile_as("Games");
    harness.run();
    requests(&fake);

    // The picker shows the profile in use; open it and choose the other.
    harness.get_by_label("Games").click();
    harness.run();
    harness.get_by_label("Work").click();
    harness.run_steps(4);
    assert_eq!(harness.state().current_profile.as_deref(), Some("Work"));
    assert_eq!(harness.state().draft.polling, 1000);
    assert_eq!(requests(&fake).len(), 1);
}

#[test]
fn a_new_profile_can_be_saved_from_the_picker() {
    let (mut harness, _, _) = window("picker-save");
    harness.get_by_label("Not saved").click();
    harness.run();
    harness.state_mut().new_profile_name = "  Weekend  ".into();
    harness.run();
    click(&mut harness, "Save");
    assert_eq!(harness.state().current_profile.as_deref(), Some("Weekend"));
    assert_eq!(harness.state().profile_names, ["Weekend"]);
    harness.get_by_label("Saved as Weekend.");
}

#[test]
fn a_bad_profile_name_is_explained() {
    let (mut harness, _, _) = window("bad-name");
    harness.state_mut().save_profile_as("work/play");
    harness.run();
    assert!(harness.state().profile_names.is_empty());
    assert!(
        matches!(&harness.state().notice, Notice::Bad(text) if text.contains("cannot contain"))
    );
}

#[test]
fn deleting_a_profile_asks_first_and_leaves_the_settings() {
    let (mut harness, fake, _) = window("delete-profile");
    harness.state_mut().save_profile_as("Work");
    harness.state_mut().confirm_delete_profile = Some("Work".into());
    harness.run();
    click(&mut harness, "Keep it");
    assert_eq!(harness.state().profile_names, ["Work"]);

    harness.state_mut().confirm_delete_profile = Some("Work".into());
    harness.run();
    click(&mut harness, "Delete profile");
    assert!(harness.state().profile_names.is_empty());
    assert_eq!(harness.state().current_profile, None);
    assert!(requests(&fake).is_empty(), "the mouse is not touched");
}

#[test]
fn the_profile_in_use_is_remembered_between_runs() {
    let (mut harness, _, profile) = window("remembered");
    harness.state_mut().save_profile_as("Games");
    let fake = Fake::default();
    let app = App::new(Box::new(fake.clone()), Box::new(fake), Some(profile), false);
    assert_eq!(app.current_profile.as_deref(), Some("Games"));
    assert_eq!(app.profile_names, ["Games"]);
}

#[test]
fn brightness_and_smart_lighting_are_previewed_and_saved() {
    let (mut harness, fake, _) = window("brightness");
    click(&mut harness, "Lighting");
    requests(&fake);
    harness
        .get_by_label("Turn the lights off while the mouse is moving")
        .click();
    harness.run_steps(4);
    assert!(harness.state().draft.smart_lighting);
    let previews = requests(&fake);
    assert!(
        matches!(previews.as_slice(), [Request::Preview(p)] if p.smart_lighting == Some(true)),
        "{previews:?}"
    );

    harness.state_mut().draft.brightness = 40;
    click(&mut harness, "Apply to mouse");
    let applied = requests(&fake);
    assert!(
        matches!(applied.as_slice(), [Request::Apply(p)] if p.brightness == Some(40)),
        "{applied:?}"
    );
}

#[test]
fn battery_saver_explains_itself_on_the_pages_it_overrides() {
    let (mut harness, fake, _) = window("saver");
    click(&mut harness, "Power");
    harness.get_by_label("Use battery saver").click();
    harness.run_steps(4);
    assert!(harness.state().draft.battery_saver);
    click(&mut harness, "Apply to mouse");
    let applied = requests(&fake);
    assert!(
        matches!(applied.as_slice(), [.., Request::Apply(p)] if p.battery_saver == Some(true)),
        "{applied:?}"
    );

    click(&mut harness, "Sensitivity");
    harness.get_by_label_contains("so the mouse uses 125 Hz");
    click(&mut harness, "Lighting");
    harness.get_by_label_contains("so the lights stay off");
}

#[test]
fn extra_mouse_buttons_can_be_chosen() {
    let (mut harness, _, _) = window("button6");
    click(&mut harness, "Buttons");
    click(&mut harness, "Side button 5");
    harness.state_mut().set_action(Action::MouseButton(4));
    harness.run_steps(2);
    click(&mut harness, "Button 7");
    assert_eq!(
        harness.state().draft.buttons.get(Button::Grid(5)),
        Action::MouseButton(7)
    );
    harness.get_by_label_contains("Windows only passes the first five");
    assert_eq!(view::describe(Action::MouseButton(7)), "Button 7");
}

#[test]
fn a_shortcut_can_follow_the_computer() {
    let (mut harness, _, _) = window("primary");
    click(&mut harness, "Buttons");
    click(&mut harness, "Side button 5");
    // Named for this computer's own key first.
    let name = if cfg!(target_os = "macos") {
        "Cmd/Ctrl"
    } else {
        "Ctrl/Cmd"
    };
    click(&mut harness, name);
    let action = harness.state().draft.buttons.get(Button::Grid(5));
    assert_eq!(action, "key:primary+5".parse().unwrap());
    assert_eq!(view::describe(action), format!("{name} + 5"));
    // It is written to the profile as itself, not as this computer's key.
    let profile = harness.state().draft.to_profile();
    assert_eq!(profile.buttons.unwrap()["g5"], "key:primary+5");
    // And the editor says what relying on it takes.
    harness.get_by_label_contains("has to be running on each computer");
    // Taking it off again takes the note away.
    click(&mut harness, name);
    assert_eq!(
        harness.state().draft.buttons.get(Button::Grid(5)),
        "key:5".parse().unwrap()
    );
    assert!(harness
        .query_by_label_contains("has to be running on each computer")
        .is_none());
}

#[test]
fn the_note_on_a_following_shortcut_knows_about_start_at_login() {
    assert!(view::primary_note(true).contains("taken care of"));
    assert!(view::primary_note(false).contains("Start the helper when I log in"));
}

#[test]
fn a_profile_changed_outside_the_window_is_shown() {
    let (mut harness, fake, profile) = window("outside");
    assert_eq!(harness.state().draft.polling, 1000);
    std::fs::write(&profile, "polling = 250\n").unwrap();
    reply(&fake, Reply::Status(CONNECTED));
    harness.run_steps(2);
    assert_eq!(harness.state().draft.polling, 250);
    assert!(!harness.state().is_dirty(), "nothing is left to apply");
    assert!(
        requests(&fake).is_empty(),
        "the helper sends it, not the window"
    );
    harness.get_by_label_contains("changed outside this window");
}

#[test]
fn edits_in_progress_survive_a_change_outside_the_window() {
    let (mut harness, fake, profile) = window("outside-dirty");
    click(&mut harness, "500 Hz");
    std::fs::write(&profile, "polling = 250\n").unwrap();
    reply(&fake, Reply::Status(CONNECTED));
    harness.run_steps(2);
    assert_eq!(harness.state().draft.polling, 500);
}

/// The sliders are drawn by ggez, so they handle the keyboard themselves.
#[test]
fn sliders_work_from_the_keyboard() {
    let (mut harness, _, _) = window("slider-keys");
    click(&mut harness, "Power");
    assert_eq!(harness.state().draft.sleep_minutes, 5);
    harness.get_by_label("Minutes before sleep").focus();
    harness.run();
    harness.press_key(egui::Key::ArrowRight);
    harness.run();
    assert_eq!(harness.state().draft.sleep_minutes, 6);
    harness.press_key(egui::Key::ArrowLeft);
    harness.run();
    harness.press_key(egui::Key::ArrowLeft);
    harness.run();
    assert_eq!(harness.state().draft.sleep_minutes, 4);
    // The arrow keys moved the slider, not the focus.
    assert!(harness.get_by_label("Minutes before sleep").is_focused());

    // The dim timer moves in small steps below a minute and larger ones
    // above, in both directions.
    assert_eq!(harness.state().draft.dim_seconds, 30);
    harness
        .get_by_label("Seconds before the lights dim")
        .focus();
    harness.run();
    for expected in [35, 40, 45, 50, 55, 60, 90] {
        harness.press_key(egui::Key::ArrowRight);
        harness.run();
        assert_eq!(harness.state().draft.dim_seconds, expected);
    }
    for expected in [60, 55] {
        harness.press_key(egui::Key::ArrowLeft);
        harness.run();
        assert_eq!(harness.state().draft.dim_seconds, expected);
    }
    assert!(harness.state().is_dirty());
}

#[test]
fn a_slider_stops_at_the_ends_of_its_range() {
    let (mut harness, _, _) = window("slider-ends");
    click(&mut harness, "Power");
    harness.state_mut().draft.sleep_minutes = 20;
    harness.state_mut().draft.dim_seconds = 0;
    harness.run();
    harness.get_by_label("Minutes before sleep").focus();
    harness.run();
    harness.press_key(egui::Key::ArrowRight);
    harness.run();
    assert_eq!(harness.state().draft.sleep_minutes, 20);
    harness
        .get_by_label("Seconds before the lights dim")
        .focus();
    harness.run();
    harness.press_key(egui::Key::ArrowLeft);
    harness.run();
    assert_eq!(harness.state().draft.dim_seconds, 0);
    harness.press_key(egui::Key::ArrowRight);
    harness.run();
    assert_eq!(harness.state().draft.dim_seconds, 5);
}

#[test]
fn a_slider_follows_the_pointer_and_reports_changes() {
    let (mut harness, fake, _) = window("slider-click");
    click(&mut harness, "Power");
    // The middle of a slider from 0 to 20.
    harness
        .get_by_label("Minutes before sleep")
        .simulate_click();
    harness.run();
    assert_eq!(harness.state().draft.sleep_minutes, 10);

    // On the lighting page a change of brightness is previewed.
    click(&mut harness, "Lighting");
    requests(&fake);
    harness
        .get_by_role_and_label(egui::accesskit::Role::Slider, "Brightness")
        .focus();
    // Not `run`: the rainbow in the preview never stops moving.
    harness.run_steps(4);
    harness.press_key(egui::Key::ArrowLeft);
    harness.run_steps(4);
    assert_eq!(harness.state().draft.brightness, 95);
    let previews = requests(&fake);
    assert!(
        matches!(previews.as_slice(), [Request::Preview(p)] if p.brightness == Some(95)),
        "{previews:?}"
    );
}

#[test]
fn a_button_can_be_given_a_macro_and_applied() {
    let (mut harness, fake, profile) = window("macro");
    click(&mut harness, "Buttons");
    click(&mut harness, "Side button 7");
    // Choose it from the list of what a button can do.
    harness
        .get_by_role_and_label(egui::accesskit::Role::ComboBox, "What it does")
        .click();
    harness.run();
    harness.get_by_label("Macro: several steps").click();
    harness.run();
    assert!(harness.state().draft.has_macro(Button::Grid(7)));
    assert!(harness.state().is_dirty());
    // The note under the steps says where macros can run.
    harness.get_by_label_contains(if ggez_service::macro_host::Host::supported() {
        "works only while ggez is running"
    } else {
        "on Windows only so far"
    });

    // What is typed is checked as it is typed.
    harness
        .state_mut()
        .draft
        .macros
        .get_mut("g7")
        .unwrap()
        .steps = "key:ctrl+c\nhop".into();
    harness.run();
    harness.get_by_label_contains("Line 2:");
    harness
        .state_mut()
        .draft
        .macros
        .get_mut("g7")
        .unwrap()
        .steps = "key:ctrl+c\nwait:80\nkey:ctrl+v".into();
    click(&mut harness, "Held");
    assert_eq!(harness.state().draft.macros["g7"].mode, Mode::Repeat);

    click(&mut harness, "Apply to mouse");
    let saved = Profile::load(&profile).unwrap();
    assert_eq!(requests(&fake), [Request::Apply(saved.clone())]);
    let spec = &saved.macros.as_ref().unwrap()["g7"];
    assert_eq!(spec.mode.as_deref(), Some("repeat"));
    assert_eq!(spec.steps, ["key:ctrl+c", "wait:80", "key:ctrl+v"]);
    saved.validate().unwrap();
}

#[test]
fn resetting_a_button_takes_its_macro_away() {
    let (mut harness, _, _) = window("macro-reset");
    click(&mut harness, "Buttons");
    click(&mut harness, "Side button 3");
    harness.state_mut().set_macro(true);
    harness.run();
    click(&mut harness, "Reset this button");
    assert!(!harness.state().draft.has_macro(Button::Grid(3)));
    assert_eq!(
        harness.state().draft.buttons.get(Button::Grid(3)),
        ButtonMap::default().get(Button::Grid(3))
    );
}

#[test]
fn a_macro_with_a_mistake_is_not_sent() {
    let (mut harness, fake, _) = window("macro-bad");
    harness.state_mut().tab = Tab::Buttons;
    harness.state_mut().set_macro(true);
    harness
        .state_mut()
        .draft
        .macros
        .get_mut("g1")
        .unwrap()
        .steps = "hop".into();
    harness.run();
    click(&mut harness, "Apply to mouse");
    assert!(requests(&fake).is_empty());
    harness.get_by_label_contains("the macro on g1");
}
