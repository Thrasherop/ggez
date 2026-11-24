//! Runs macros for real: catches the trigger keys the mouse sends, feeds
//! them to the [`Engine`], and has the computer press what it asks for.
//!
//! Windows and macOS so far.
//!
//! On Windows the trigger keys are caught by a low-level keyboard hook,
//! which sees every key before any program does and can swallow it, and
//! keys are sent with `SendInput`. Neither needs administrator rights or a
//! driver. Two limits come with that: a program running as administrator
//! does not receive what ggez sends unless ggez runs as administrator too,
//! and a few games ignore keys that were not typed on a real keyboard.
//!
//! On a Mac they are caught by an event tap and sent as Quartz events.
//! Both need the user to allow ggez under Privacy & Security →
//! Accessibility, so [`Host::start`] fails until they have. And a Mac knows
//! the keys F13 to F20 only, so eight buttons can have a macro there, not
//! twelve.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::Instant;

use crate::macros::{Engine, Output, Plan};

#[cfg(target_os = "macos")]
use macos as system;
#[cfg(windows)]
use windows as system;

#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
enum Message {
    Plan(Plan),
    /// A trigger key went down or came up.
    Trigger(u8, bool),
    Stop,
}

/// The running macro engine. Dropping it stops it and lets go of every key
/// a macro was holding.
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
pub struct Host {
    messages: Sender<Message>,
    engine: Option<JoinHandle<()>>,
    #[cfg(any(windows, target_os = "macos"))]
    catcher: Option<system::Catcher>,
}

impl Host {
    /// Whether macros can run on this system at all.
    pub fn supported() -> bool {
        cfg!(any(windows, target_os = "macos"))
    }

    /// How many buttons can have a macro on this system: one for each
    /// trigger key it can tell apart.
    pub fn max_macros() -> usize {
        if cfg!(windows) {
            crate::macros::MAX_MACROS
        } else if cfg!(target_os = "macos") {
            8
        } else {
            0
        }
    }

    /// Whether the system lets ggez catch and press keys. Only a Mac asks
    /// its user about that.
    pub fn permitted() -> bool {
        #[cfg(target_os = "macos")]
        {
            macos::permitted()
        }
        #[cfg(not(target_os = "macos"))]
        {
            true
        }
    }

    /// Have the system ask its user for that permission, and show them
    /// where it is given. Does nothing where none is needed.
    pub fn ask_permission() {
        #[cfg(target_os = "macos")]
        macos::ask_permission();
    }

    /// Start the engine with no macros. `None` where macros cannot run, or
    /// cannot run yet: on a Mac, until the user has given permission.
    pub fn start() -> Option<Self> {
        #[cfg(any(windows, target_os = "macos"))]
        {
            let (messages, inbox) = std::sync::mpsc::channel();
            let catcher = system::Catcher::install(messages.clone())?;
            let engine = std::thread::spawn(move || system::run(inbox));
            Some(Self {
                messages,
                engine: Some(engine),
                catcher: Some(catcher),
            })
        }
        #[cfg(not(any(windows, target_os = "macos")))]
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
        #[cfg(any(windows, target_os = "macos"))]
        drop(self.catcher.take());
    }
}

/// Which trigger keys to catch, for the code that the system calls when a
/// key is pressed: bit 0 is F13.
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
mod watched {
    use std::sync::atomic::{AtomicU16, Ordering};

    use crate::macros::TRIGGER_KEYS;

    static WATCHED: AtomicU16 = AtomicU16::new(0);

    pub fn set(triggers: &[u8]) {
        let bits = triggers
            .iter()
            .filter(|key| TRIGGER_KEYS.contains(key))
            .fold(0u16, |bits, key| bits | 1 << (key - TRIGGER_KEYS.start()));
        WATCHED.store(bits, Ordering::Relaxed);
    }

    pub fn has(trigger: u8) -> bool {
        TRIGGER_KEYS.contains(&trigger)
            && WATCHED.load(Ordering::Relaxed) & 1 << (trigger - TRIGGER_KEYS.start()) != 0
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn only_the_named_trigger_keys_are_watched() {
            set(&[0x68, 0x6A, 0x04]);
            assert!(has(0x68) && has(0x6A));
            assert!(!has(0x69) && !has(0x04));
            set(&[0x73]);
            assert!(has(0x73) && !has(0x68));
            set(&[]);
            assert!(TRIGGER_KEYS.clone().all(|key| !has(key)));
        }
    }
}

/// The engine's loop: wait for a message or for the next step to fall due,
/// then send whatever is due. `send` presses or types one thing; `watch` is
/// told which trigger keys to catch.
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
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
    use std::sync::mpsc::{self, Receiver, Sender};
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

    use super::{watched, Message};
    use crate::macros::{scan_code, Output, TRIGGER_KEYS};

    /// The engine's thread.
    pub fn run(inbox: Receiver<Message>) {
        super::drive(inbox, send, watched::set);
    }

    thread_local! {
        /// Where the hook, which runs on the thread that installed it,
        /// reports the keys it catches.
        static CAUGHT: RefCell<Option<Sender<Message>>> = const { RefCell::new(None) };
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
                let trigger = TRIGGER_KEYS.start() + (key - u32::from(VK_F13)) as u8;
                if watched::has(trigger) {
                    let down = matches!(wparam as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
                    CAUGHT.with(|caught| {
                        if let Some(messages) = caught.borrow().as_ref() {
                            let _ = messages.send(Message::Trigger(trigger, down));
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
    pub struct Catcher {
        thread: Option<JoinHandle<()>>,
        thread_id: u32,
    }

    impl Catcher {
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

    impl Drop for Catcher {
        fn drop(&mut self) {
            watched::set(&[]);
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

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::sync::Arc;
    use std::thread::JoinHandle;
    use std::time::Duration;

    use core_foundation::base::TCFType;
    use core_foundation::runloop::{kCFRunLoopCommonModes, kCFRunLoopDefaultMode, CFRunLoop};
    use core_graphics::event::{
        CGEvent, CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions,
        CGEventTapPlacement, CGEventType, CGMouseButton, CallbackResult, EventField,
    };
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    use super::{watched, Message};
    use crate::macros::{mac_key_code, mac_trigger, Output};

    /// Written on every event ggez sends ("ggez" in ASCII), so that the
    /// tap can tell them from the mouse's: a macro that presses Print
    /// Screen sends what a Mac calls F13, which is also a trigger key.
    const OURS: i64 = 0x6767_657A;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventTapEnable(tap: *mut c_void, enable: bool);
        fn CGRequestPostEventAccess() -> bool;
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> u8;
    }

    pub fn permitted() -> bool {
        // SAFETY: takes nothing and returns a Boolean.
        unsafe { AXIsProcessTrusted() != 0 }
    }

    pub fn ask_permission() {
        // SAFETY: takes nothing. The first time, the system shows its own
        // window asking the user; after that it does nothing.
        unsafe { CGRequestPostEventAccess() };
        // The list where the permission is given, since the system's
        // window only appears once.
        let _ = std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .spawn();
    }

    /// The mouse's trigger key that a key event stands for, and whether it
    /// went down, if it is one to catch.
    fn caught(kind: CGEventType, key_code: i64, ours: bool) -> Option<(u8, bool)> {
        let down = match kind {
            CGEventType::KeyDown => true,
            CGEventType::KeyUp => false,
            _ => return None,
        };
        let trigger = mac_trigger(u16::try_from(key_code).ok()?)?;
        (!ours && watched::has(trigger)).then_some((trigger, down))
    }

    /// The event tap and the thread whose run loop it is called on.
    pub struct Catcher {
        thread: Option<JoinHandle<()>>,
        stop: Arc<AtomicBool>,
    }

    impl Catcher {
        /// `None` if the system will not allow a tap: the user has not
        /// given ggez the Accessibility permission.
        pub fn install(messages: Sender<Message>) -> Option<Self> {
            let (ready, installed) = mpsc::channel();
            let stop = Arc::new(AtomicBool::new(false));
            let stopped = stop.clone();
            let thread = std::thread::spawn(move || {
                // The system switches a tap off if it is ever slow to
                // answer, and says so through the tap itself. The callback
                // needs the tap's port to switch it back on, and the port
                // exists only once the tap does.
                let port = Arc::new(AtomicPtr::new(std::ptr::null_mut::<c_void>()));
                let port_for_callback = port.clone();
                let tap = CGEventTap::new(
                    CGEventTapLocation::HID,
                    CGEventTapPlacement::HeadInsertEventTap,
                    CGEventTapOptions::Default,
                    vec![CGEventType::KeyDown, CGEventType::KeyUp],
                    move |_proxy, kind, event| {
                        if matches!(
                            kind,
                            CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput
                        ) {
                            let port = port_for_callback.load(Ordering::Relaxed);
                            if !port.is_null() {
                                // SAFETY: the port of this tap, which is
                                // alive while its callback is being called.
                                unsafe { CGEventTapEnable(port, true) };
                            }
                            return CallbackResult::Keep;
                        }
                        let key_code =
                            event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
                        let ours = event
                            .get_integer_value_field(EventField::EVENT_SOURCE_USER_DATA)
                            == OURS;
                        match caught(kind, key_code, ours) {
                            Some((trigger, down)) => {
                                let _ = messages.send(Message::Trigger(trigger, down));
                                // Swallow it: no program sees the key.
                                CallbackResult::Drop
                            }
                            None => CallbackResult::Keep,
                        }
                    },
                );
                let source = tap
                    .as_ref()
                    .ok()
                    .and_then(|tap| tap.mach_port().create_runloop_source(0).ok());
                let (Ok(tap), Some(source)) = (tap, source) else {
                    let _ = ready.send(false);
                    return;
                };
                // SAFETY: a constant of the system's.
                CFRunLoop::get_current().add_source(&source, unsafe { kCFRunLoopCommonModes });
                port.store(
                    tap.mach_port().as_concrete_TypeRef().cast::<c_void>(),
                    Ordering::Relaxed,
                );
                tap.enable();
                let _ = ready.send(true);
                // The tap is called from this thread's run loop, so the
                // loop has to keep turning. It comes back a few times a
                // second to see whether it is time to stop.
                while !stopped.load(Ordering::Relaxed) {
                    // SAFETY: a constant of the system's.
                    CFRunLoop::run_in_mode(
                        unsafe { kCFRunLoopDefaultMode },
                        Duration::from_millis(250),
                        false,
                    );
                }
                port.store(std::ptr::null_mut(), Ordering::Relaxed);
                drop(tap);
            });
            if !installed.recv().unwrap_or(false) {
                let _ = thread.join();
                return None;
            }
            Some(Self {
                thread: Some(thread),
                stop,
            })
        }
    }

    impl Drop for Catcher {
        fn drop(&mut self) {
            watched::set(&[]);
            self.stop.store(true, Ordering::Relaxed);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    /// The engine's thread.
    pub fn run(inbox: Receiver<Message>) {
        let mut keyboard = Keyboard::default();
        super::drive(inbox, |output| keyboard.send(output), watched::set);
    }

    const MODIFIERS: CGEventFlags = CGEventFlags::CGEventFlagShift
        .union(CGEventFlags::CGEventFlagControl)
        .union(CGEventFlags::CGEventFlagAlternate)
        .union(CGEventFlags::CGEventFlagCommand);

    /// What a held modifier key adds to the events sent while it is held.
    fn flag(usage: u8) -> Option<CGEventFlags> {
        Some(match usage {
            0xE0 | 0xE4 => CGEventFlags::CGEventFlagControl,
            0xE1 | 0xE5 => CGEventFlags::CGEventFlagShift,
            0xE2 | 0xE6 => CGEventFlags::CGEventFlagAlternate,
            0xE3 | 0xE7 => CGEventFlags::CGEventFlagCommand,
            _ => return None,
        })
    }

    /// Presses keys and mouse buttons on the Mac.
    #[derive(Default)]
    struct Keyboard {
        /// The modifier keys macros are holding. An event says for itself
        /// which modifiers go with it, so each one sent is told.
        held: Vec<u8>,
    }

    impl Keyboard {
        fn flags(&self) -> CGEventFlags {
            self.held
                .iter()
                .filter_map(|&usage| flag(usage))
                .fold(CGEventFlags::CGEventFlagNull, CGEventFlags::union)
        }

        fn source() -> Option<CGEventSource> {
            CGEventSource::new(CGEventSourceStateID::HIDSystemState).ok()
        }

        /// The events that carry out one thing a macro asks for.
        fn events(&mut self, output: &Output) -> Vec<CGEvent> {
            let mut events = Vec::new();
            match output {
                Output::KeyDown(usage) | Output::KeyUp(usage) => {
                    let down = matches!(output, Output::KeyDown(_));
                    if flag(*usage).is_some() {
                        self.held.retain(|held| held != usage);
                        if down {
                            self.held.push(*usage);
                        }
                    }
                    let event =
                        mac_key_code(*usage)
                            .zip(Self::source())
                            .and_then(|(code, source)| {
                                CGEvent::new_keyboard_event(source, code, down).ok()
                            });
                    events.extend(event);
                }
                Output::MouseDown(button) | Output::MouseUp(button) => {
                    let down = matches!(output, Output::MouseDown(_));
                    let (kind, which) = match (button, down) {
                        (1, true) => (CGEventType::LeftMouseDown, CGMouseButton::Left),
                        (1, false) => (CGEventType::LeftMouseUp, CGMouseButton::Left),
                        (2, true) => (CGEventType::RightMouseDown, CGMouseButton::Right),
                        (2, false) => (CGEventType::RightMouseUp, CGMouseButton::Right),
                        (3..=5, true) => (CGEventType::OtherMouseDown, CGMouseButton::Center),
                        (3..=5, false) => (CGEventType::OtherMouseUp, CGMouseButton::Center),
                        _ => return events,
                    };
                    // A click happens where the pointer is.
                    let place = Self::source()
                        .and_then(|source| CGEvent::new(source).ok())
                        .map(|event| event.location());
                    let event = place.zip(Self::source()).and_then(|(place, source)| {
                        CGEvent::new_mouse_event(source, kind, place, which).ok()
                    });
                    if let Some(event) = &event {
                        // Left, right and middle are buttons 0 to 2; back
                        // and forward are 3 and 4.
                        event.set_integer_value_field(
                            EventField::MOUSE_EVENT_BUTTON_NUMBER,
                            i64::from(*button) - 1,
                        );
                        event.set_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE, 1);
                    }
                    events.extend(event);
                }
                // Typed as characters, whatever the keyboard layout: each
                // event carries its character with it.
                Output::Text(text) => {
                    let mut units = [0u16; 2];
                    for character in text.chars() {
                        let units = character.encode_utf16(&mut units);
                        for down in [true, false] {
                            let event = Self::source().and_then(|source| {
                                CGEvent::new_keyboard_event(source, 0, down).ok()
                            });
                            if let Some(event) = &event {
                                event.set_string_from_utf16_unchecked(units);
                            }
                            events.extend(event);
                        }
                    }
                }
            }
            // Text is typed as it is written, whatever is held.
            let modifiers = match output {
                Output::Text(_) => CGEventFlags::CGEventFlagNull,
                _ => self.flags(),
            };
            for event in &events {
                event.set_flags(event.get_flags().difference(MODIFIERS).union(modifiers));
                event.set_integer_value_field(EventField::EVENT_SOURCE_USER_DATA, OURS);
            }
            events
        }

        fn send(&mut self, output: &Output) {
            for event in self.events(output) {
                event.post(CGEventTapLocation::HID);
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn only_the_mouses_own_watched_trigger_keys_are_caught() {
            watched::set(&[0x68, 0x6F]);
            // F13 and F20, as a Mac numbers them.
            assert_eq!(
                caught(CGEventType::KeyDown, 0x69, false),
                Some((0x68, true))
            );
            assert_eq!(caught(CGEventType::KeyUp, 0x5A, false), Some((0x6F, false)));
            // F14 has no macro; A is not a trigger; and what ggez itself
            // sent is not the mouse's.
            assert_eq!(caught(CGEventType::KeyDown, 0x6B, false), None);
            assert_eq!(caught(CGEventType::KeyDown, 0x00, false), None);
            assert_eq!(caught(CGEventType::KeyDown, 0x69, true), None);
            assert_eq!(caught(CGEventType::FlagsChanged, 0x69, false), None);
            assert_eq!(caught(CGEventType::KeyDown, -1, false), None);
            watched::set(&[]);
            assert_eq!(caught(CGEventType::KeyDown, 0x69, false), None);
        }

        fn field(event: &CGEvent, field: u32) -> i64 {
            event.get_integer_value_field(field)
        }

        #[test]
        fn outputs_become_the_right_events() {
            let mut keyboard = Keyboard::default();
            // Cmd down, C down and up, Cmd up: C carries the Cmd flag, and
            // nothing does once Cmd is let go.
            let cmd = keyboard.events(&Output::KeyDown(0xE3));
            assert_eq!(cmd.len(), 1);
            assert_eq!(field(&cmd[0], EventField::KEYBOARD_EVENT_KEYCODE), 0x37);
            let c = keyboard.events(&Output::KeyDown(0x06));
            assert_eq!(field(&c[0], EventField::KEYBOARD_EVENT_KEYCODE), 0x08);
            assert!(matches!(c[0].get_type(), CGEventType::KeyDown));
            assert_eq!(
                c[0].get_flags().intersection(MODIFIERS),
                CGEventFlags::CGEventFlagCommand
            );
            assert_eq!(field(&c[0], EventField::EVENT_SOURCE_USER_DATA), OURS);
            let up = keyboard.events(&Output::KeyUp(0x06));
            assert!(matches!(up[0].get_type(), CGEventType::KeyUp));
            keyboard.events(&Output::KeyUp(0xE3));
            let plain = keyboard.events(&Output::KeyDown(0x06));
            assert!(plain[0].get_flags().intersection(MODIFIERS).is_empty());

            // Text: a press and a release for each character, with no
            // modifiers even while one is held.
            keyboard.events(&Output::KeyDown(0xE1));
            let text = keyboard.events(&Output::Text("hé😀".into()));
            assert_eq!(text.len(), 6);
            assert!(text
                .iter()
                .all(|event| event.get_flags().intersection(MODIFIERS).is_empty()));

            // Shift is still held for a click, and forward is button 4.
            let forward = keyboard.events(&Output::MouseDown(5));
            assert!(matches!(forward[0].get_type(), CGEventType::OtherMouseDown));
            assert_eq!(field(&forward[0], EventField::MOUSE_EVENT_BUTTON_NUMBER), 4);
            assert_eq!(
                forward[0].get_flags().intersection(MODIFIERS),
                CGEventFlags::CGEventFlagShift
            );
            let left = keyboard.events(&Output::MouseUp(1));
            assert!(matches!(left[0].get_type(), CGEventType::LeftMouseUp));
            assert_eq!(field(&left[0], EventField::MOUSE_EVENT_BUTTON_NUMBER), 0);

            // Nothing for what cannot be done.
            assert!(keyboard.events(&Output::KeyDown(0x70)).is_empty(), "F21");
            assert!(keyboard.events(&Output::MouseDown(9)).is_empty());
        }

        #[test]
        fn asking_whether_it_is_allowed_is_safe() {
            // The answer depends on the machine; that it can be asked at
            // all is what is checked.
            let _ = permitted();
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
