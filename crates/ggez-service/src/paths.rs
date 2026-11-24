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

/// A per-user folder for things that can be thrown away and fetched again:
/// `%LOCALAPPDATA%\ggez`, ``/Library/Caches/ggez` or
/// `$XDG_CACHE_HOME/ggez` (default ``/.cache/ggez`).
pub fn cache_dir() -> Result<PathBuf> {
    let var = |name: &str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let base = if cfg!(windows) {
        var("LOCALAPPDATA").context("LOCALAPPDATA is not set")?
    } else if cfg!(target_os = "macos") {
        var("HOME")
            .context("HOME is not set")?
            .join("Library/Caches")
    } else {
        match var("XDG_CACHE_HOME") {
            Some(dir) => dir,
            None => var("HOME").context("HOME is not set")?.join(".cache"),
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

/// Written to tell the helper that is running to stop.
pub(crate) fn stop_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("helper.stop"))
}
