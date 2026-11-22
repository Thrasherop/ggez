//! Runs macros for real: catches the trigger keys the mouse sends, feeds
//! them to the [`Engine`], and has the computer press what it asks for.
//!
//! Only Windows so far. There the trigger keys are caught by a low-level
//! keyboard hook, which sees every key before any program does and can
//! swallow it, and keys are sent with `SendInput`. Neither needs
//! administrator rights or a driver. Two limits come with that: a program
//! running as administrator does not receive what ggez sends unless ggez
//! runs as administrator too, and a few games ignore keys that were not
//! typed on a real keyboard.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::Instant;

use crate::macros::{Engine, Output, Plan};

#[cfg_attr(not(windows), allow(dead_code))]
enum Message {
    Plan(Plan),
    /// A trigger key went down or came up.
    Trigger(u8, bool),
    Stop,
}

/// The running macro engine. Dropping it stops it and lets go of every key
/// a macro was holding.
#[cfg_attr(not(windows), allow(dead_code))]
pub struct Host {
    messages: Sender<Message>,
    engine: Option<JoinHandle<()>>,
    #[cfg(windows)]
    hook: Option<windows::Hook>,
}

impl Host {
    /// Whether macros can run on this system at all.
    pub fn supported() -> bool {
        cfg!(windows)
    }

    /// Start the engine with no macros. `None` where macros cannot run.
    pub fn start() -> Option<Self> {
        #[cfg(windows)]
        {
            let (messages, inbox) = std::sync::mpsc::channel();
            let hook = windows::Hook::install(messages.clone())?;
            let engine = std::thread::spawn(move || drive(inbox, windows::send, windows::watch));
            Some(Self {
                messages,
                engine: Some(engine),
                hook: Some(hook),
            })
        }
        #[cfg(not(windows))]
        {
            None
        }
    }

    /// Use these macros from now on.
    pub fn set_plan(&self, plan: Plan) {
        let _ = self.messages.send(Message::Plan(plan));
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.messages.send(Message::Stop);
        if let Some(engine) = self.engine.take() {
            let _ = engine.join();
        }
        #[cfg(windows)]
        drop(self.hook.take());
    }
}

/// The engine's loop: wait for a message or for the next step to fall due,
/// then send whatever is due. `send` presses or types one thing; `watch` is
/// told which trigger keys to catch.
#[cfg_attr(not(windows), allow(dead_code))]
fn drive(inbox: Receiver<Message>, mut send: impl FnMut(&Output), mut watch: impl FnMut(&[u8])) {
    let mut engine = Engine::default();
    loop {
        let message = match engine.next_due() {
            Some(due) => match inbox.recv_timeout(due.saturating_duration_since(Instant::now())) {
                Ok(message) => Some(message),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            },
            None => match inbox.recv() {
                Ok(message) => Some(message),
                Err(_) => break,
            },
        };
        let now = Instant::now();
        match message {
            Some(Message::Plan(plan)) => {
                engine.set_plan(&plan).iter().for_each(&mut send);
                watch(&engine.triggers());
            }
            Some(Message::Trigger(key, down)) => engine.trigger(key, down, now),
            Some(Message::Stop) => break,
            None => {}
        }
        engine.advance(now).iter().for_each(&mut send);
    }
    // Never leave a key held down behind us.
    watch(&[]);
    engine.stop_all().iter().for_each(&mut send);
}

#[cfg(windows)]
mod windows {
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicU16, Ordering};
    use std::sync::mpsc::{self, Sender};
    use std::thread::JoinHandle;

    use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
        KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, KEYEVENTF_UNICODE, MOUSEEVENTF_LEFTDOWN,
        MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN,
        MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT, VK_F13, VK_F24,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetMessageW, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx,
        HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_QUIT,
        WM_SYSKEYDOWN,
    };

    use super::Message;
    use crate::macros::{scan_code, Output, TRIGGER_KEYS};

    /// Which of F13 to F24 to catch: bit 0 is F13.
    static WATCHED: AtomicU16 = AtomicU16::new(0);

    thread_local! {
        /// Where the hook, which runs on the thread that installed it,
        /// reports the keys it catches.
        static CAUGHT: RefCell<Option<Sender<Message>>> = const { RefCell::new(None) };
    }

    pub fn watch(triggers: &[u8]) {
        let bits = triggers
            .iter()
            .filter(|key| TRIGGER_KEYS.contains(key))
            .fold(0u16, |bits, key| bits | 1 << (key - TRIGGER_KEYS.start()));
        WATCHED.store(bits, Ordering::Relaxed);
    }

    unsafe extern "system" fn on_key(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32 {
            // SAFETY: for HC_ACTION the system passes a pointer to a
            // KBDLLHOOKSTRUCT that is valid for the length of this call.
            let event = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
            // Keys that ggez itself sent are not the mouse's.
            let real = event.flags & LLKHF_INJECTED == 0;
            let key = event.vkCode;
            if real && (u32::from(VK_F13)..=u32::from(VK_F24)).contains(&key) {
                let index = (key - u32::from(VK_F13)) as u8;
                if WATCHED.load(Ordering::Relaxed) & 1 << index != 0 {
                    let down = matches!(wparam as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
                    CAUGHT.with(|caught| {
                        if let Some(messages) = caught.borrow().as_ref() {
                            let _ =
                                messages.send(Message::Trigger(TRIGGER_KEYS.start() + index, down));
                        }
                    });
                    // Swallow it: no program sees the key.
                    return 1;
                }
            }
        }
        // SAFETY: passing the call on unchanged, as the hook contract asks.
        unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
    }

    /// The keyboard hook and the thread whose message loop it runs on.
    pub struct Hook {
        thread: Option<JoinHandle<()>>,
        thread_id: u32,
    }

    impl Hook {
        pub fn install(messages: Sender<Message>) -> Option<Self> {
            let (ready, installed) = mpsc::channel();
            let thread = std::thread::spawn(move || {
                CAUGHT.with(|caught| *caught.borrow_mut() = Some(messages));
                // SAFETY: plain Win32 calls. The hook procedure is a
                // function in this program, and the hook is removed before
                // the thread ends.
                unsafe {
                    let hook = SetWindowsHookExW(
                        WH_KEYBOARD_LL,
                        Some(on_key),
                        GetModuleHandleW(std::ptr::null()),
                        0,
                    );
                    let _ = ready.send((!hook.is_null()).then(|| GetCurrentThreadId()));
                    if hook.is_null() {
                        return;
                    }
                    // A low-level hook is called through this thread's
                    // message loop, so the loop has to keep turning.
                    let mut message: MSG = std::mem::zeroed();
                    while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {}
                    UnhookWindowsHookEx(hook);
                }
            });
            let thread_id = installed.recv().ok().flatten()?;
            Some(Self {
                thread: Some(thread),
                thread_id,
            })
        }
    }

    impl Drop for Hook {
        fn drop(&mut self) {
            watch(&[]);
            // SAFETY: posting a message to a thread of this process.
            unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0) };
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    fn key(scan: u16, flags: u32) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: 0,
                    wScan: scan,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    fn mouse(flags: u32, data: u32) -> INPUT {
        INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: data as _,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    /// The inputs that carry out one thing a macro asks for.
    fn inputs(output: &Output) -> Vec<INPUT> {
        let pressed = |usage: u8, up: u32| {
            scan_code(usage).map(|(scan, extended)| {
                let extended = if extended { KEYEVENTF_EXTENDEDKEY } else { 0 };
                key(scan, KEYEVENTF_SCANCODE | extended | up)
            })
        };
        let button = |button: u8, down: bool| match (button, down) {
            (1, true) => Some(mouse(MOUSEEVENTF_LEFTDOWN, 0)),
            (1, false) => Some(mouse(MOUSEEVENTF_LEFTUP, 0)),
            (2, true) => Some(mouse(MOUSEEVENTF_RIGHTDOWN, 0)),
            (2, false) => Some(mouse(MOUSEEVENTF_RIGHTUP, 0)),
            (3, true) => Some(mouse(MOUSEEVENTF_MIDDLEDOWN, 0)),
            (3, false) => Some(mouse(MOUSEEVENTF_MIDDLEUP, 0)),
            // Back and forward are the first and second "X" buttons.
            (4 | 5, true) => Some(mouse(MOUSEEVENTF_XDOWN, u32::from(button) - 3)),
            (4 | 5, false) => Some(mouse(MOUSEEVENTF_XUP, u32::from(button) - 3)),
            _ => None,
        };
        match output {
            Output::KeyDown(usage) => pressed(*usage, 0).into_iter().collect(),
            Output::KeyUp(usage) => pressed(*usage, KEYEVENTF_KEYUP).into_iter().collect(),
            Output::MouseDown(which) => button(*which, true).into_iter().collect(),
            Output::MouseUp(which) => button(*which, false).into_iter().collect(),
            // Typed as characters, whatever the keyboard layout.
            Output::Text(text) => text
                .encode_utf16()
                .flat_map(|unit| {
                    [
                        key(unit, KEYEVENTF_UNICODE),
                        key(unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP),
                    ]
                })
                .collect(),
        }
    }

    pub fn send(output: &Output) {
        let inputs = inputs(output);
        if inputs.is_empty() {
            return;
        }
        // SAFETY: the pointer and count describe the vector above.
        unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            );
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn only_the_named_trigger_keys_are_watched() {
            watch(&[0x68, 0x6A, 0x04]);
            assert_eq!(WATCHED.load(Ordering::Relaxed), 0b101);
            watch(&[0x73]);
            assert_eq!(WATCHED.load(Ordering::Relaxed), 1 << 11);
            watch(&[]);
            assert_eq!(WATCHED.load(Ordering::Relaxed), 0);
        }

        #[test]
        fn outputs_become_the_right_inputs() {
            // Ctrl down: a scan code, not extended.
            let ctrl = inputs(&Output::KeyDown(0xE0));
            assert_eq!(ctrl.len(), 1);
            // SAFETY: these inputs were made as keyboard inputs.
            let ki = unsafe { ctrl[0].Anonymous.ki };
            assert_eq!((ki.wScan, ki.dwFlags), (0x1D, KEYEVENTF_SCANCODE));
            // Delete up: extended.
            let delete = inputs(&Output::KeyUp(0x4C));
            let ki = unsafe { delete[0].Anonymous.ki };
            assert_eq!(
                (ki.wScan, ki.dwFlags),
                (
                    0x53,
                    KEYEVENTF_SCANCODE | KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP
                )
            );
            // Text: a press and a release for each UTF-16 unit.
            assert_eq!(inputs(&Output::Text("hé".into())).len(), 4);
            assert_eq!(inputs(&Output::Text("😀".into())).len(), 4);
            // Forward is the second X button.
            let forward = inputs(&Output::MouseDown(5));
            let mi = unsafe { forward[0].Anonymous.mi };
            assert_eq!((mi.dwFlags, mi.mouseData as u32), (MOUSEEVENTF_XDOWN, 2));
            // Nothing for what cannot be done.
            assert!(inputs(&Output::KeyDown(0x99)).is_empty());
            assert!(inputs(&Output::MouseDown(9)).is_empty());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::*;
    use crate::macros::MacroSpec;

    fn plan(mode: &str, steps: &[&str]) -> Plan {
        let spec = MacroSpec {
            mode: Some(mode.to_string()),
            steps: steps.iter().map(|step| step.to_string()).collect(),
        };
        Plan::new(Some(&BTreeMap::from([("g1".to_string(), spec)]))).unwrap()
    }

    /// The loop with a pretend keyboard: returns the way in, what was sent,
    /// and which keys were asked to be watched.
    #[allow(clippy::type_complexity)]
    fn running() -> (
        Sender<Message>,
        Arc<Mutex<Vec<Output>>>,
        Arc<Mutex<Vec<Vec<u8>>>>,
        JoinHandle<()>,
    ) {
        let (messages, inbox) = mpsc::channel();
        let sent = Arc::new(Mutex::new(Vec::new()));
        let watched = Arc::new(Mutex::new(Vec::new()));
        let (sent_by_loop, watched_by_loop) = (sent.clone(), watched.clone());
        let thread = std::thread::spawn(move || {
            drive(
                inbox,
                |output| sent_by_loop.lock().unwrap().push(output.clone()),
                |keys| watched_by_loop.lock().unwrap().push(keys.to_vec()),
            )
        });
        (messages, sent, watched, thread)
    }

    #[test]
    fn a_press_runs_the_macro_in_real_time() {
        let (messages, sent, watched, thread) = running();
        messages
            .send(Message::Plan(plan(
                "once",
                &["key:a", "wait:30", "text:ok"],
            )))
            .unwrap();
        messages.send(Message::Trigger(0x68, true)).unwrap();
        messages.send(Message::Trigger(0x68, false)).unwrap();
        std::thread::sleep(Duration::from_millis(250));
        assert_eq!(
            *sent.lock().unwrap(),
            [
                Output::KeyDown(0x04),
                Output::KeyUp(0x04),
                Output::Text("ok".into())
            ]
        );
        messages.send(Message::Stop).unwrap();
        thread.join().unwrap();
        assert_eq!(*watched.lock().unwrap(), [vec![0x68], vec![]]);
    }

    #[test]
    fn stopping_lets_go_of_held_keys() {
        let (messages, sent, _, thread) = running();
        messages
            .send(Message::Plan(plan("toggle", &["down:shift", "wait:5000"])))
            .unwrap();
        messages.send(Message::Trigger(0x68, true)).unwrap();
        std::thread::sleep(Duration::from_millis(80));
        assert_eq!(*sent.lock().unwrap(), [Output::KeyDown(0xE1)]);
        // The same as the helper shutting down.
        drop(messages);
        thread.join().unwrap();
        assert_eq!(
            *sent.lock().unwrap(),
            [Output::KeyDown(0xE1), Output::KeyUp(0xE1)]
        );
    }
}
