//! Detecting SteelSeries GG.
//!
//! GG keeps sending its own settings and lighting to the mouse, which
//! overrides ggez's settings and makes ggez's commands go unanswered. ggez
//! cannot fix that, but it can say so.

use std::process::{Command, Stdio};

/// Whether SteelSeries GG's engine, the part that talks to the mouse, is
/// running. `false` if that cannot be determined.
pub fn is_running() -> bool {
    let mut command = if cfg!(windows) {
        let mut c = Command::new("tasklist");
        c.args([
            "/FI",
            "IMAGENAME eq SteelSeriesEngine.exe",
            "/FO",
            "CSV",
            "/NH",
        ]);
        c
    } else {
        let mut c = Command::new("pgrep");
        c.args(["-f", "SteelSeriesEngine|SteelSeries GG|SteelSeriesGG"]);
        c
    };
    command.stdin(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        // Do not flash a console window when called from the app.
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    match command.output() {
        // tasklist succeeds either way and prints an "INFO: no tasks" line
        // when nothing matches; pgrep prints matching process ids.
        Ok(output) => {
            let text = String::from_utf8_lossy(&output.stdout);
            if cfg!(windows) {
                text.contains("SteelSeriesEngine.exe")
            } else {
                output.status.success() && !text.trim().is_empty()
            }
        }
        Err(_) => false,
    }
}

/// What to tell someone whose command failed while GG is running.
pub const ADVICE: &str =
    "SteelSeries GG is running. It keeps sending its own settings to the mouse, \
which overrides ggez and makes commands fail. Quit GG (right-click its tray icon and choose Exit) \
and try again.";
