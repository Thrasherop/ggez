//! Looks for a newer ggez, and fetches one, on a background thread so the
//! window never waits on the network.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use ggez_service::update::{self, Https, Relaunch, Version};

/// How long after opening to try again to clear away a download that
/// could not be deleted at once.
const TIDY_AGAIN_AFTER: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// Find out what the newest version is.
    Check,
    /// Download the newest version, check it, and install it as far as
    /// that can be done while ggez is open.
    Install,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    /// The newest version there is, or why that could not be found out.
    Latest(Result<Version, String>),
    /// How much of the download has arrived, in bytes.
    Progress { received: u64, total: Option<u64> },
    /// The new version is ready: start this once the window has closed.
    Ready(Result<(Version, Relaunch), String>),
}

/// The window's way to the releases page. A trait so tests can stand in
/// for it.
pub trait Updater {
    fn send(&mut self, request: Request);
    /// Replies that have arrived since the last call.
    fn poll(&mut self) -> Vec<Reply>;
}

/// For a window that has not been given a way to the releases page: it
/// asks nothing and hears nothing.
pub struct Offline;

impl Updater for Offline {
    fn send(&mut self, _request: Request) {}
    fn poll(&mut self) -> Vec<Reply> {
        Vec::new()
    }
}

pub struct Worker {
    requests: Sender<Request>,
    replies: Receiver<Reply>,
}

/// Delete what the last update downloaded. Returns whether nothing is
/// left.
fn tidy() -> bool {
    update::download_folder().is_ok_and(|folder| {
        let _ = std::fs::remove_dir_all(&folder);
        !folder.exists()
    })
}

impl Worker {
    /// `wake` is called whenever there is a reply to collect.
    pub fn spawn(wake: impl Fn() + Send + 'static) -> Self {
        let (requests, incoming) = mpsc::channel();
        let (outgoing, replies) = mpsc::channel();
        std::thread::spawn(move || {
            let problem = |e: anyhow::Error| format!("{e:#}");
            // An update has just been installed if this is its first run.
            // On Windows the installer opens ggez while it is still
            // finishing, and its own file cannot be deleted until it has,
            // so what is left is tried once more a little later. It is
            // done on this thread, between requests, so that it can never
            // delete a download that is under way.
            let mut left_over = !tidy();
            loop {
                let request = if left_over {
                    match incoming.recv_timeout(TIDY_AGAIN_AFTER) {
                        Ok(request) => request,
                        Err(RecvTimeoutError::Timeout) => {
                            tidy();
                            left_over = false;
                            continue;
                        }
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                } else {
                    match incoming.recv() {
                        Ok(request) => request,
                        Err(_) => break,
                    }
                };
                let reply = match request {
                    Request::Check => Reply::Latest(
                        update::latest(&Https)
                            .map(|release| release.version)
                            .map_err(problem),
                    ),
                    Request::Install => {
                        // Fetching starts by emptying the folder itself.
                        left_over = false;
                        let installed =
                            update::download_and_install(&Https, &mut |received, total| {
                                let _ = outgoing.send(Reply::Progress { received, total });
                                wake();
                            });
                        // Where the new programs are already in place, a
                        // helper still running is the old one. It is told
                        // to finish now, whatever happens to the window;
                        // the next ggez to start brings the new one.
                        if installed
                            .as_ref()
                            .is_ok_and(|(_, relaunch)| !relaunch.installs)
                        {
                            ggez_service::helper::stop_running(Duration::ZERO);
                        }
                        Reply::Ready(installed.map_err(problem))
                    }
                };
                if outgoing.send(reply).is_err() {
                    break;
                }
                wake();
            }
        });
        Self { requests, replies }
    }
}

impl Updater for Worker {
    fn send(&mut self, request: Request) {
        let _ = self.requests.send(request);
    }

    fn poll(&mut self) -> Vec<Reply> {
        self.replies.try_iter().collect()
    }
}
