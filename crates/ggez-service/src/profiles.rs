//! Named profiles: several saved setups to switch between, such as one
//! for work and one for games.
//!
//! Each is a profile file in a `profiles` folder beside the active
//! `profile.toml`. Switching copies one over the active profile and sends
//! it to the mouse; the name in use is remembered in a small text file.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::profile::Profile;

/// Where named profiles live, given the folder that holds `profile.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    config_dir: PathBuf,
}

impl Store {
    pub fn new(config_dir: PathBuf) -> Self {
        Self { config_dir }
    }

    /// The store beside the default profile.
    pub fn default_location() -> Result<Self> {
        Ok(Self::new(crate::paths::config_dir()?))
    }

    fn dir(&self) -> PathBuf {
        self.config_dir.join("profiles")
    }

    fn path(&self, name: &str) -> Result<PathBuf> {
        Ok(self.dir().join(format!("{}.toml", valid_name(name)?)))
    }

    fn current_file(&self) -> PathBuf {
        self.config_dir.join("current-profile.txt")
    }

    /// Every saved profile's name, in alphabetical order.
    pub fn list(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(self.dir()) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.is_file() && path.extension().is_some_and(|e| e == "toml"))
            .filter_map(|path| Some(path.file_stem()?.to_str()?.to_owned()))
            .filter(|name| valid_name(name).is_ok())
            .collect();
        names.sort_by_key(|name| name.to_lowercase());
        names
    }

    pub fn exists(&self, name: &str) -> bool {
        self.path(name).is_ok_and(|path| path.exists())
    }

    pub fn load(&self, name: &str) -> Result<Profile> {
        let path = self.path(name)?;
        if !path.exists() {
            bail!("there is no profile called {name:?}");
        }
        Profile::load(&path)
    }

    pub fn save(&self, name: &str, profile: &Profile) -> Result<()> {
        profile.save(&self.path(name)?)
    }

    /// Remove a saved profile. If it was the one in use, nothing is in use
    /// afterwards; the mouse and the active profile are left as they are.
    pub fn delete(&self, name: &str) -> Result<()> {
        let path = self.path(name)?;
        // Ask before the file goes: afterwards it no longer counts as current.
        let was_current = self.current().as_deref() == Some(name);
        std::fs::remove_file(&path)
            .with_context(|| format!("could not delete {}", path.display()))?;
        if was_current {
            self.set_current(None)?;
        }
        Ok(())
    }

    /// The name of the profile in use, if it still exists.
    pub fn current(&self) -> Option<String> {
        let name = std::fs::read_to_string(self.current_file()).ok()?;
        let name = name.trim();
        self.exists(name).then(|| name.to_owned())
    }

    pub fn set_current(&self, name: Option<&str>) -> Result<()> {
        let file = self.current_file();
        match name {
            Some(name) => {
                valid_name(name)?;
                write_creating_folder(&file, name)
            }
            None => match std::fs::remove_file(&file) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e).with_context(|| format!("could not remove {}", file.display())),
            },
        }
    }
}

fn write_creating_folder(path: &Path, text: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("could not create {}", dir.display()))?;
    }
    std::fs::write(path, text).with_context(|| format!("could not write {}", path.display()))
}

/// Check that `name` can be a file name on every system ggez runs on, and
/// return it trimmed.
pub fn valid_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty() {
        bail!("a profile needs a name");
    }
    if name.chars().count() > 40 {
        bail!("a profile name can be at most 40 characters");
    }
    if let Some(bad) = name
        .chars()
        .find(|c| c.is_control() || r#"/\:*?"<>|"#.contains(*c))
    {
        bail!("a profile name cannot contain {bad:?}");
    }
    if name.starts_with('.') || name.ends_with('.') {
        bail!("a profile name cannot start or end with a dot");
    }
    // Windows reserves these device names, with or without an extension.
    const RESERVED: [&str; 8] = ["con", "prn", "aux", "nul", "com1", "com2", "lpt1", "lpt2"];
    if RESERVED.contains(&name.to_lowercase().as_str()) {
        bail!("{name:?} is a name the system reserves");
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(name: &str) -> Store {
        let dir = std::env::temp_dir().join(format!("ggez-profiles-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Store::new(dir)
    }

    fn profile(polling: u32) -> Profile {
        Profile::parse(&format!("polling = {polling}")).unwrap()
    }

    #[test]
    fn saved_profiles_can_be_listed_loaded_and_deleted() {
        let store = store("basic");
        assert!(store.list().is_empty());
        store.save("Work", &profile(500)).unwrap();
        store.save("games", &profile(1000)).unwrap();
        store.save("Art", &profile(250)).unwrap();
        assert_eq!(store.list(), ["Art", "games", "Work"]);
        assert_eq!(store.load("Work").unwrap(), profile(500));

        store.delete("Work").unwrap();
        assert_eq!(store.list(), ["Art", "games"]);
        assert!(store.load("Work").is_err());
        assert!(store.delete("Work").is_err());
    }

    #[test]
    fn the_profile_in_use_is_remembered_until_it_is_deleted() {
        let store = store("current");
        assert_eq!(store.current(), None);
        store.save("Work", &profile(500)).unwrap();
        store.set_current(Some("Work")).unwrap();
        assert_eq!(store.current().as_deref(), Some("Work"));

        store.delete("Work").unwrap();
        assert_eq!(store.current(), None);
        assert!(
            !store.current_file().exists(),
            "no stale note of it is left"
        );
        store.set_current(None).unwrap();
    }

    #[test]
    fn a_remembered_name_without_a_file_counts_as_none() {
        let store = store("stale");
        store.set_current(Some("Gone")).unwrap();
        assert_eq!(store.current(), None);
    }

    #[test]
    fn names_that_cannot_be_file_names_are_refused() {
        for bad in [
            "",
            "   ",
            "a/b",
            r"a\b",
            "a:b",
            "what?",
            "..",
            ".hidden",
            "trailing.",
            "NUL",
            "con",
        ] {
            assert!(valid_name(bad).is_err(), "{bad:?}");
        }
        assert!(valid_name(&"x".repeat(41)).is_err());
        assert_eq!(
            valid_name("  Work & play (2)  ").unwrap(),
            "Work & play (2)"
        );
        assert_eq!(valid_name("Jeux vidéo").unwrap(), "Jeux vidéo");
    }

    #[test]
    fn stray_files_in_the_folder_are_ignored() {
        let store = store("stray");
        store.save("Work", &profile(500)).unwrap();
        std::fs::write(store.dir().join("notes.txt"), "hello").unwrap();
        std::fs::create_dir_all(store.dir().join("folder.toml")).unwrap();
        assert_eq!(store.list(), ["Work"]);
    }
}
