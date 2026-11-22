//! Where ggez keeps its files.

use std::path::PathBuf;

use anyhow::{Context, Result};

/// The per-user configuration directory:
/// `%APPDATA%\ggez`, `~/Library/Application Support/ggez` or
/// `$XDG_CONFIG_HOME/ggez` (default `~/.config/ggez`).
pub fn config_dir() -> Result<PathBuf> {
    let var = |name: &str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let base = if cfg!(windows) {
        var("APPDATA").context("APPDATA is not set")?
    } else if cfg!(target_os = "macos") {
        var("HOME")
            .context("HOME is not set")?
            .join("Library/Application Support")
    } else {
        match var("XDG_CONFIG_HOME") {
            Some(dir) => dir,
            None => var("HOME").context("HOME is not set")?.join(".config"),
        }
    };
    Ok(base.join("ggez"))
}

/// The profile the background helper keeps applied.
pub fn profile_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("profile.toml"))
}

pub(crate) fn lock_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("helper.lock"))
}
