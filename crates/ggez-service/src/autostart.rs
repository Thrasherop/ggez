//! Start the background helper when the user logs in.
//!
//! Each platform's ordinary per-user mechanism is used, so nothing needs
//! administrator rights and everything is easy to inspect and undo:
//! - Windows: a value under `HKCU\...\CurrentVersion\Run`
//! - macOS: a LaunchAgent in `~/Library/LaunchAgents`
//! - Linux: an XDG autostart entry in `~/.config/autostart`

use std::path::PathBuf;

use anyhow::{bail, Context, Result};

/// The command line the login item runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<String>,
}

impl Launch {
    /// How to start the helper from the running program. Prefers the
    /// windowed `ggez-gui` next to the current executable, because on
    /// Windows a console program started at login shows a console window.
    pub fn for_current_exe() -> Result<Self> {
        let exe = std::env::current_exe().context("could not find the running program")?;
        let gui = exe.with_file_name(format!("ggez-gui{}", std::env::consts::EXE_SUFFIX));
        Ok(if gui.exists() {
            Self {
                program: gui,
                args: vec!["--background".into()],
            }
        } else {
            Self {
                program: exe,
                args: vec!["daemon".into()],
            }
        })
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::process::Command;

    const KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE: &str = "ggez";

    fn reg(args: &[&str]) -> Result<std::process::Output> {
        let mut command = Command::new("reg");
        command.args(args);
        // Do not flash a console window when called from the GUI.
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        command.output().context("could not run reg.exe")
    }

    pub fn enable(launch: &Launch) -> Result<()> {
        let mut line = format!("\"{}\"", launch.program.display());
        for arg in &launch.args {
            line.push(' ');
            line.push_str(arg);
        }
        let out = reg(&["add", KEY, "/v", VALUE, "/t", "REG_SZ", "/d", &line, "/f"])?;
        if !out.status.success() {
            bail!(
                "reg add failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(())
    }

    pub fn disable() -> Result<()> {
        if location()?.is_some() {
            let out = reg(&["delete", KEY, "/v", VALUE, "/f"])?;
            if !out.status.success() {
                bail!(
                    "reg delete failed: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                );
            }
        }
        Ok(())
    }

    pub fn location() -> Result<Option<String>> {
        Ok(reg(&["query", KEY, "/v", VALUE])?
            .status
            .success()
            .then(|| format!(r"{KEY}\{VALUE}")))
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    fn home() -> Result<PathBuf> {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .context("HOME is not set")
    }

    #[cfg(target_os = "macos")]
    fn entry() -> Result<(PathBuf, fn(&Launch) -> String)> {
        Ok((
            home()?.join("Library/LaunchAgents/io.github.thrasherop.ggez.plist"),
            launch_agent,
        ))
    }

    #[cfg(not(target_os = "macos"))]
    fn entry() -> Result<(PathBuf, fn(&Launch) -> String)> {
        let config = match std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
            Some(dir) => PathBuf::from(dir),
            None => home()?.join(".config"),
        };
        Ok((config.join("autostart/ggez.desktop"), desktop_entry))
    }

    pub fn enable(launch: &Launch) -> Result<()> {
        let (path, render) = entry()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, render(launch))
            .with_context(|| format!("could not write {}", path.display()))
    }

    pub fn disable() -> Result<()> {
        let (path, _) = entry()?;
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e).with_context(|| format!("could not remove {}", path.display())),
        }
    }

    pub fn location() -> Result<Option<String>> {
        let (path, _) = entry()?;
        Ok(path.exists().then(|| path.display().to_string()))
    }
}

/// A macOS LaunchAgent that starts the helper at login.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn launch_agent(launch: &Launch) -> String {
    let escape = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let mut arguments = format!(
        "    <string>{}</string>\n",
        escape(&launch.program.to_string_lossy())
    );
    for arg in &launch.args {
        arguments.push_str(&format!("    <string>{}</string>\n", escape(arg)));
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>io.github.thrasherop.ggez</string>
  <key>ProgramArguments</key>
  <array>
{arguments}  </array>
  <key>RunAtLoad</key>
  <true/>
</dict>
</plist>
"#
    )
}

/// A freedesktop.org autostart entry that starts the helper at login.
#[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
fn desktop_entry(launch: &Launch) -> String {
    // Exec values quote arguments with double quotes and backslash-escape
    // the characters that are special inside them.
    let quote = |s: &str| {
        let mut quoted = String::from("\"");
        for c in s.chars() {
            if matches!(c, '"' | '`' | '$' | '\\') {
                quoted.push('\\');
            }
            quoted.push(c);
        }
        quoted.push('"');
        quoted
    };
    let mut exec = quote(&launch.program.to_string_lossy());
    for arg in &launch.args {
        exec.push(' ');
        exec.push_str(&quote(arg));
    }
    format!(
        "[Desktop Entry]\nType=Application\nName=ggez helper\nComment=Keeps your SteelSeries mouse profile applied\nExec={exec}\nX-GNOME-Autostart-enabled=true\n"
    )
}

/// Start the helper at every login.
pub fn enable(launch: &Launch) -> Result<()> {
    if !launch.program.exists() {
        bail!("{} does not exist", launch.program.display());
    }
    imp::enable(launch)
}

/// Stop starting the helper at login. Not an error if it was not enabled.
pub fn disable() -> Result<()> {
    imp::disable()
}

/// Where the login item is registered, if it is.
pub fn location() -> Result<Option<String>> {
    imp::location()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_entry_quotes_the_command() {
        let launch = Launch {
            program: PathBuf::from("/opt/my apps/ggez-gui"),
            args: vec!["--background".into()],
        };
        let entry = desktop_entry(&launch);
        assert!(
            entry.contains("Exec=\"/opt/my apps/ggez-gui\" \"--background\"\n"),
            "{entry}"
        );
        assert!(entry.starts_with("[Desktop Entry]\nType=Application\n"));
    }

    #[test]
    fn launch_agent_lists_program_and_arguments() {
        let launch = Launch {
            program: PathBuf::from("/a&b/ggez"),
            args: vec!["daemon".into()],
        };
        let plist = launch_agent(&launch);
        assert!(
            plist.contains("<string>/a&amp;b/ggez</string>\n    <string>daemon</string>"),
            "{plist}"
        );
        assert!(plist.contains("<key>RunAtLoad</key>\n  <true/>"));
    }

    #[test]
    fn enabling_a_missing_program_is_refused() {
        let missing = Launch {
            program: PathBuf::from("definitely/not/here"),
            args: vec![],
        };
        assert!(enable(&missing).is_err());
    }
}
