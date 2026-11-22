//! The background helper: keeps the user's profile applied.
//!
//! The mouse remembers most settings itself, but not zone colors or the
//! reactive color. The helper re-sends those every time the mouse becomes
//! reachable (plugged in, switched on, woken from sleep), and applies and
//! saves the whole profile whenever the profile file changes.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use ggez_core::{Aerox9, Transport};

use crate::profile::Profile;

/// How often to look for the mouse and for profile changes.
pub const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// What the helper did on one pass, for logs and tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The mouse became reachable and the volatile settings were re-sent.
    Reconnected,
    /// The profile file changed and the whole profile was applied and saved.
    ProfileApplied,
    /// The mouse stopped being reachable.
    Disconnected,
    /// The profile file could not be used; the previous one stays in effect.
    BadProfile(String),
    /// Talking to the mouse failed; it will be retried on the next pass.
    Failed(String),
}

/// Tracks what has already been applied so that each pass does only what
/// is new. Separate from the loop so it can be tested without a mouse.
#[derive(Debug, Default)]
pub struct Helper {
    profile_path: PathBuf,
    profile: Option<Profile>,
    /// Modification time of the profile file when it was last read.
    loaded_at: Option<SystemTime>,
    /// The loaded profile still has to be applied in full.
    pending_full_apply: bool,
    online: bool,
}

impl Helper {
    pub fn new(profile_path: PathBuf) -> Self {
        Self {
            profile_path,
            ..Self::default()
        }
    }

    /// Re-read the profile file if it changed. A missing file means no
    /// profile; an invalid one is reported and the last good one kept.
    fn refresh_profile(&mut self) -> Option<Event> {
        let modified = std::fs::metadata(&self.profile_path)
            .and_then(|m| m.modified())
            .ok();
        if modified == self.loaded_at {
            return None;
        }
        // The first load only restores volatile settings: the mouse already
        // holds everything else, and rewriting it on every start is needless.
        let first_load = self.loaded_at.is_none() && self.profile.is_none();
        self.loaded_at = modified;
        if modified.is_none() {
            self.profile = None;
            return None;
        }
        match Profile::load(&self.profile_path).and_then(|p| p.validate().map(|()| p)) {
            Ok(profile) => {
                self.pending_full_apply = !first_load;
                self.profile = Some(profile);
                None
            }
            Err(e) => Some(Event::BadProfile(format!("{e:#}"))),
        }
    }

    /// One pass with the mouse's current state. `mouse` is `None` when no
    /// supported device is present at all.
    pub fn tick<T: Transport>(&mut self, mouse: Option<&mut Aerox9<T>>) -> Vec<Event> {
        let mut events = Vec::new();
        events.extend(self.refresh_profile());

        let Some(mouse) = mouse else {
            if std::mem::take(&mut self.online) {
                events.push(Event::Disconnected);
            }
            return events;
        };
        let online = match mouse.is_online() {
            Ok(online) => online,
            Err(e) => {
                events.push(Event::Failed(e.to_string()));
                return events;
            }
        };
        let was_online = std::mem::replace(&mut self.online, online);
        if !online {
            if was_online {
                events.push(Event::Disconnected);
            }
            return events;
        }
        let Some(profile) = &self.profile else {
            return events;
        };

        let result = if self.pending_full_apply {
            profile
                .apply(mouse)
                .and_then(|sent| if sent { Ok(mouse.save()?) } else { Ok(()) })
                .map(|()| Some(Event::ProfileApplied))
        } else if !was_online {
            profile
                .apply_volatile(mouse)
                .map(|sent| sent.then_some(Event::Reconnected))
        } else {
            Ok(None)
        };
        match result {
            Ok(event) => {
                self.pending_full_apply = false;
                events.extend(event);
            }
            Err(e) => {
                // Treat the mouse as gone so the next pass starts over.
                self.online = false;
                events.push(Event::Failed(format!("{e:#}")));
            }
        }
        events
    }
}

/// Holds the helper's lock file; only one helper runs per user.
pub struct InstanceLock {
    _guard: fd_lock::RwLockWriteGuard<'static, std::fs::File>,
}

impl InstanceLock {
    /// `Ok(None)` means another helper is already running.
    pub fn acquire() -> Result<Option<Self>> {
        let path = crate::paths::lock_path()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .with_context(|| format!("could not open {}", path.display()))?;
        // The lock lives as long as the process; leaking it keeps the guard
        // valid without a self-referential struct.
        let lock: &'static mut fd_lock::RwLock<std::fs::File> =
            Box::leak(Box::new(fd_lock::RwLock::new(file)));
        match lock.try_write() {
            Ok(guard) => Ok(Some(Self { _guard: guard })),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e).with_context(|| format!("could not lock {}", path.display())),
        }
    }
}

/// Run the helper until `stop` is set. `on_event` receives everything the
/// helper does, for logging.
pub fn run(profile_path: &Path, stop: Arc<AtomicBool>, mut on_event: impl FnMut(&Event)) {
    let mut helper = Helper::new(profile_path.to_path_buf());
    while !stop.load(Ordering::Relaxed) {
        // Open the device afresh each pass: it is cheap, and it copes with
        // the dongle or cable being unplugged and replugged.
        let mut mouse = Aerox9::open().ok();
        for event in helper.tick(mouse.as_mut()) {
            on_event(&event);
        }
        drop(mouse);
        std::thread::sleep(POLL_INTERVAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ggez_core::{Link, MockTransport};

    /// A dongle whose link state the test controls. Acknowledges everything.
    struct FakeDongle {
        linked: bool,
        written: Vec<Vec<u8>>,
        replies: std::collections::VecDeque<Vec<u8>>,
    }

    impl Transport for FakeDongle {
        fn write(&mut self, packet: &[u8]) -> std::result::Result<(), ggez_core::TransportError> {
            self.written.push(packet.to_vec());
            let command = packet[1];
            self.replies.push_back(if command == 0xBC {
                vec![0xBC, u8::from(self.linked)]
            } else {
                vec![command, 0x00]
            });
            Ok(())
        }

        fn read(
            &mut self,
            buf: &mut [u8],
            timeout_ms: i32,
        ) -> std::result::Result<usize, ggez_core::TransportError> {
            if timeout_ms == 0 {
                return Ok(0);
            }
            Ok(self.replies.pop_front().map_or(0, |r| {
                buf[..r.len()].copy_from_slice(&r);
                r.len()
            }))
        }
    }

    fn dongle(linked: bool) -> Aerox9<FakeDongle> {
        let t = FakeDongle {
            linked,
            written: Vec::new(),
            replies: Default::default(),
        };
        Aerox9::new(t, Link::Wireless)
    }

    /// Commands sent since the last call, without link-status queries.
    fn sent(mouse: &mut Aerox9<FakeDongle>) -> Vec<u8> {
        // The transport is only reachable immutably; rebuild from a copy.
        let commands: Vec<u8> = mouse
            .transport()
            .written
            .iter()
            .map(|p| p[1])
            .filter(|&c| c != 0xBC)
            .collect();
        let linked = mouse.transport().linked;
        *mouse = dongle(linked);
        commands
    }

    fn profile_file(name: &str, text: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ggez-test-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("profile.toml");
        std::fs::write(&path, text).unwrap();
        path
    }

    const COLORS: &str = "polling = 500\n[colors]\nall = \"red\"\n";

    #[test]
    fn start_restores_only_volatile_settings() {
        let mut helper = Helper::new(profile_file("start", COLORS));
        let mut mouse = dongle(true);
        assert_eq!(helper.tick(Some(&mut mouse)), [Event::Reconnected]);
        assert_eq!(sent(&mut mouse), [0x61, 0x61, 0x61]);
        // Nothing more while it stays connected.
        assert!(helper.tick(Some(&mut mouse)).is_empty());
        assert!(sent(&mut mouse).is_empty());
    }

    #[test]
    fn colors_are_resent_after_sleep_or_replug() {
        let mut helper = Helper::new(profile_file("wake", COLORS));
        let mut mouse = dongle(true);
        helper.tick(Some(&mut mouse));
        sent(&mut mouse);

        let mut asleep = dongle(false);
        assert_eq!(helper.tick(Some(&mut asleep)), [Event::Disconnected]);
        assert!(sent(&mut asleep).is_empty());
        assert_eq!(helper.tick(Some(&mut mouse)), [Event::Reconnected]);
        assert_eq!(sent(&mut mouse), [0x61, 0x61, 0x61]);

        assert_eq!(
            helper.tick(None::<&mut Aerox9<FakeDongle>>),
            [Event::Disconnected]
        );
        assert_eq!(helper.tick(Some(&mut mouse)), [Event::Reconnected]);
    }

    #[test]
    fn edited_profile_is_applied_in_full_and_saved() {
        let path = profile_file("edit", COLORS);
        let mut helper = Helper::new(path.clone());
        let mut mouse = dongle(true);
        helper.tick(Some(&mut mouse));
        sent(&mut mouse);

        std::fs::write(&path, "polling = 1000\n").unwrap();
        // Make sure the modification time differs even on coarse clocks.
        helper.loaded_at = Some(SystemTime::UNIX_EPOCH);
        assert_eq!(helper.tick(Some(&mut mouse)), [Event::ProfileApplied]);
        assert_eq!(sent(&mut mouse), [0x6B, 0x51]);
    }

    #[test]
    fn edit_made_while_asleep_is_applied_on_wake() {
        let path = profile_file("asleep-edit", COLORS);
        let mut helper = Helper::new(path.clone());
        let mut asleep = dongle(false);
        helper.tick(Some(&mut asleep));

        std::fs::write(&path, "polling = 1000\n").unwrap();
        helper.loaded_at = Some(SystemTime::UNIX_EPOCH);
        assert!(helper.tick(Some(&mut asleep)).is_empty());
        let mut mouse = dongle(true);
        assert_eq!(helper.tick(Some(&mut mouse)), [Event::ProfileApplied]);
        assert_eq!(sent(&mut mouse), [0x6B, 0x51]);
    }

    #[test]
    fn bad_profile_keeps_the_previous_one() {
        let path = profile_file("bad", COLORS);
        let mut helper = Helper::new(path.clone());
        let mut mouse = dongle(true);
        helper.tick(Some(&mut mouse));
        sent(&mut mouse);

        std::fs::write(&path, "polling = 7\n").unwrap();
        helper.loaded_at = Some(SystemTime::UNIX_EPOCH);
        let events = helper.tick(Some(&mut mouse));
        assert!(
            matches!(events.as_slice(), [Event::BadProfile(_)]),
            "{events:?}"
        );
        assert!(sent(&mut mouse).is_empty());
        assert!(helper.profile.as_ref().unwrap().has_volatile());
    }

    #[test]
    fn no_profile_file_means_nothing_is_sent() {
        let mut helper = Helper::new(std::env::temp_dir().join("ggez-test-missing/profile.toml"));
        let mut mouse = dongle(true);
        assert!(helper.tick(Some(&mut mouse)).is_empty());
        assert!(sent(&mut mouse).is_empty());
    }

    #[test]
    fn mock_transport_type_checks() {
        // The helper is generic over the transport used in dry runs too.
        let mut helper = Helper::new(PathBuf::from("missing.toml"));
        let mut mouse = Aerox9::new(MockTransport::accepting(), Link::Wired);
        assert!(helper.tick(Some(&mut mouse)).is_empty());
    }
}
