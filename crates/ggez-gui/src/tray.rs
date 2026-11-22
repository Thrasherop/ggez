//! What the notification-area icon says and offers. The drawing of the icon
//! and its menu is in `tray_windows`; this part runs, and is tested, on
//! every system.

use ggez_service::battery::{BatteryNotice, BatteryWatch};
use ggez_service::helper::Event;

/// One line of the icon's menu.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    /// Greyed-out text that only informs.
    Label(String),
    Separator,
    Entry {
        text: String,
        checked: bool,
        /// Shown in bold: what a plain click on the icon does.
        default: bool,
        choice: Choice,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Choice {
    OpenWindow,
    UseProfile(String),
    Quit,
}

/// Something to tell the user in a pop-up.
#[derive(Debug, Clone, PartialEq)]
pub struct Popup {
    pub title: String,
    pub text: String,
    pub warning: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum Mouse {
    /// Nothing heard from the helper yet.
    #[default]
    Unknown,
    Away,
    Here(Option<(u8, bool)>),
}

/// Follows the helper's events so the icon can describe the mouse.
#[derive(Debug, Default)]
pub struct Tray {
    mouse: Mouse,
    watch: BatteryWatch,
    /// The last profile problem shown, so it is not repeated every pass.
    bad_profile: Option<String>,
}

impl Tray {
    /// Take in one helper event; returns a pop-up if it deserves one.
    pub fn on_event(&mut self, event: &Event) -> Option<Popup> {
        match event {
            Event::Battery { percent, charging } => {
                self.mouse = Mouse::Here(Some((*percent, *charging)));
                match self.watch.update(*percent, *charging)? {
                    BatteryNotice::Low(percent) => Some(Popup {
                        title: "Mouse battery low".into(),
                        text: format!("The Aerox 9 is at {percent}%. Plug it in soon."),
                        warning: true,
                    }),
                    BatteryNotice::Full => Some(Popup {
                        title: "Mouse charged".into(),
                        text: "The Aerox 9 is at 100%.".into(),
                        warning: false,
                    }),
                }
            }
            Event::Reconnected | Event::ProfileApplied => {
                if !matches!(self.mouse, Mouse::Here(_)) {
                    self.mouse = Mouse::Here(None);
                }
                self.bad_profile = None;
                None
            }
            Event::Disconnected => {
                self.mouse = Mouse::Away;
                None
            }
            Event::BadProfile(problem) => {
                if self.bad_profile.as_deref() == Some(problem) {
                    return None;
                }
                self.bad_profile = Some(problem.clone());
                Some(Popup {
                    title: "ggez cannot use your profile".into(),
                    text: problem.clone(),
                    warning: true,
                })
            }
            // Retried on the next pass; `Disconnected` covers a mouse
            // that has really gone.
            Event::Failed(_) | Event::Macros(_) => None,
            Event::MacrosUnavailable => Some(Popup {
                title: "Macros are not running".into(),
                text: "Your profile has macros, but ggez can only run them on Windows so far."
                    .into(),
                warning: true,
            }),
        }
    }

    /// The mouse in a few words, for the tooltip and the top of the menu.
    pub fn status(&self) -> String {
        match self.mouse {
            Mouse::Unknown => "Looking for the mouse…".into(),
            Mouse::Away => "Mouse asleep, off or unplugged".into(),
            Mouse::Here(None) => "Aerox 9 connected".into(),
            Mouse::Here(Some((percent, false))) => format!("Aerox 9: {percent}%"),
            Mouse::Here(Some((percent, true))) => format!("Aerox 9: {percent}%, charging"),
        }
    }

    pub fn tooltip(&self) -> String {
        format!("ggez\n{}", self.status())
    }

    pub fn menu(&self, profiles: &[String], current: Option<&str>) -> Vec<Item> {
        let mut items = vec![
            Item::Label(self.status()),
            Item::Separator,
            Item::Entry {
                text: "Open ggez".into(),
                checked: false,
                default: true,
                choice: Choice::OpenWindow,
            },
        ];
        if !profiles.is_empty() {
            items.push(Item::Separator);
            items.push(Item::Label("Profile".into()));
            items.extend(profiles.iter().map(|name| Item::Entry {
                text: name.clone(),
                checked: current == Some(name.as_str()),
                default: false,
                choice: Choice::UseProfile(name.clone()),
            }));
        }
        items.push(Item::Separator);
        items.push(Item::Entry {
            text: "Quit ggez".into(),
            checked: false,
            default: false,
            choice: Choice::Quit,
        });
        items
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn battery(percent: u8, charging: bool) -> Event {
        Event::Battery { percent, charging }
    }

    #[test]
    fn the_status_follows_the_mouse() {
        let mut tray = Tray::default();
        assert_eq!(tray.status(), "Looking for the mouse…");
        tray.on_event(&Event::Reconnected);
        assert_eq!(tray.status(), "Aerox 9 connected");
        tray.on_event(&battery(85, false));
        assert_eq!(tray.status(), "Aerox 9: 85%");
        // Re-sending colors does not forget the level.
        tray.on_event(&Event::ProfileApplied);
        assert_eq!(tray.tooltip(), "ggez\nAerox 9: 85%");
        tray.on_event(&battery(85, true));
        assert_eq!(tray.status(), "Aerox 9: 85%, charging");
        tray.on_event(&Event::Disconnected);
        assert_eq!(tray.status(), "Mouse asleep, off or unplugged");
    }

    #[test]
    fn a_low_battery_pops_up_once() {
        let mut tray = Tray::default();
        assert_eq!(tray.on_event(&battery(50, false)), None);
        let popup = tray.on_event(&battery(15, false)).unwrap();
        assert!(popup.warning && popup.text.contains("15%"), "{popup:?}");
        assert_eq!(tray.on_event(&battery(10, false)), None);
    }

    #[test]
    fn a_broken_profile_is_reported_once_until_it_changes() {
        let mut tray = Tray::default();
        let bad = Event::BadProfile("polling 7 is not supported".into());
        assert!(tray.on_event(&bad).is_some());
        assert_eq!(tray.on_event(&bad), None);
        tray.on_event(&Event::ProfileApplied);
        assert!(tray.on_event(&bad).is_some());
    }

    #[test]
    fn the_menu_lists_profiles_and_ticks_the_one_in_use() {
        let tray = Tray::default();
        let plain = tray.menu(&[], None);
        assert_eq!(plain.len(), 5, "{plain:?}");
        assert!(matches!(
            &plain[2],
            Item::Entry {
                default: true,
                choice: Choice::OpenWindow,
                ..
            }
        ));
        assert!(matches!(
            plain.last(),
            Some(Item::Entry {
                choice: Choice::Quit,
                ..
            })
        ));

        let names = ["Games".to_string(), "Work".to_string()];
        let menu = tray.menu(&names, Some("Work"));
        let ticked: Vec<&str> = menu
            .iter()
            .filter_map(|item| match item {
                Item::Entry {
                    text,
                    checked: true,
                    ..
                } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(ticked, ["Work"]);
        assert!(menu.contains(&Item::Entry {
            text: "Games".into(),
            checked: false,
            default: false,
            choice: Choice::UseProfile("Games".into()),
        }));
    }
}
