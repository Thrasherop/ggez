//! Talks to the mouse on a background thread so the window never freezes
//! while a command is waiting for the radio.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use ggez_core::{Aerox9, Link};
use ggez_service::Profile;

/// How often the status shown in the sidebar is refreshed.
const REFRESH_EVERY: Duration = Duration::from_secs(5);
/// Looking for SteelSeries GG means starting another program, so do it less
/// often than the status refresh.
const GG_CHECK_EVERY: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// Send the whole profile and store it in the mouse's memory.
    Apply(Profile),
    /// Send only the lighting, without storing anything, so color changes
    /// can be seen while they are being made.
    Preview(Profile),
    Refresh,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    Status(Status),
    Applied(Result<(), String>),
    /// Whether SteelSeries GG is running and fighting over the mouse.
    GgRunning(bool),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum Status {
    /// Nothing has been heard from the worker yet.
    #[default]
    Unknown,
    /// No cable or dongle is plugged in.
    NoDevice,
    /// The dongle is present but the mouse is off, asleep or out of range.
    Asleep,
    Connected {
        link: Link,
        /// Percent and whether it is charging; `None` if it could not be read.
        battery: Option<(u8, bool)>,
        firmware: Option<String>,
    },
}

/// The window's view of the mouse. A trait so tests can stand in for it.
pub trait Device {
    fn send(&mut self, request: Request);
    /// Replies that have arrived since the last call.
    fn poll(&mut self) -> Vec<Reply>;
}

/// The real mouse, driven from a worker thread.
pub struct Worker {
    requests: Sender<Request>,
    replies: Receiver<Reply>,
}

impl Worker {
    /// `wake` is called after every reply so the window repaints.
    pub fn spawn(wake: impl Fn() + Send + 'static) -> Self {
        let (requests, request_rx) = mpsc::channel();
        let (reply_tx, replies) = mpsc::channel();
        std::thread::spawn(move || run(request_rx, reply_tx, wake));
        Self { requests, replies }
    }
}

impl Device for Worker {
    fn send(&mut self, request: Request) {
        // The worker only stops when the window closes.
        let _ = self.requests.send(request);
    }

    fn poll(&mut self) -> Vec<Reply> {
        self.replies.try_iter().collect()
    }
}

fn run(requests: Receiver<Request>, replies: Sender<Reply>, wake: impl Fn()) {
    let mut firmware: Option<String> = None;
    let mut gg_checked: Option<std::time::Instant> = None;
    let mut next = Some(Request::Refresh);
    loop {
        let request = match next.take() {
            Some(request) => request,
            None => match requests.recv_timeout(REFRESH_EVERY) {
                Ok(request) => request,
                Err(RecvTimeoutError::Timeout) => Request::Refresh,
                Err(RecvTimeoutError::Disconnected) => return,
            },
        };
        // While colors are being dragged, previews arrive faster than the
        // mouse takes them. Only the newest matters.
        let request = match request {
            Request::Preview(mut latest) => {
                let mut other = None;
                for queued in requests.try_iter() {
                    match queued {
                        Request::Preview(newer) => latest = newer,
                        queued => {
                            other = Some(queued);
                            break;
                        }
                    }
                }
                next = other;
                Request::Preview(latest)
            }
            request => request,
        };

        let reply = match request {
            Request::Refresh => {
                if gg_checked.is_none_or(|at| at.elapsed() >= GG_CHECK_EVERY) {
                    gg_checked = Some(std::time::Instant::now());
                    let gg = Reply::GgRunning(ggez_service::gg::is_running());
                    if replies.send(gg).is_err() {
                        return;
                    }
                }
                Reply::Status(status(&mut firmware))
            }
            Request::Preview(profile) => {
                // A failed preview is not worth interrupting the user for;
                // the status line already shows when the mouse is away.
                if let Ok(mut mouse) = Aerox9::open() {
                    let _ = profile.apply_volatile(&mut mouse);
                }
                continue;
            }
            Request::Apply(profile) => Reply::Applied(apply(&profile)),
        };
        if replies.send(reply).is_err() {
            return;
        }
        wake();
    }
}

fn apply(profile: &Profile) -> Result<(), String> {
    let mut mouse = Aerox9::open().map_err(|e| e.to_string())?;
    profile.apply(&mut mouse).map_err(|e| format!("{e:#}"))?;
    mouse.save().map_err(|e| e.to_string())
}

fn status(firmware: &mut Option<String>) -> Status {
    let Ok(mut mouse) = Aerox9::open() else {
        *firmware = None;
        return Status::NoDevice;
    };
    if !mouse.is_online().unwrap_or(false) {
        return Status::Asleep;
    }
    // The version cannot change while the mouse stays connected.
    if firmware.is_none() {
        *firmware = mouse.firmware_version().ok();
    }
    Status::Connected {
        link: mouse.link(),
        battery: mouse.battery().ok().map(|b| (b.percent, b.charging)),
        firmware: firmware.clone(),
    }
}
