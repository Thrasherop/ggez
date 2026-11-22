//! The notification-area icon on Windows, straight on the Win32 API: an
//! icon with a tooltip, a right-click menu and pop-up messages. It needs
//! nothing but a hidden window to receive the icon's clicks.

use std::cell::{Cell, RefCell};
use std::io;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIIF_WARNING, NIM_ADD,
    NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, FindWindowW, GetCursorPos, GetSystemMetrics, IsIconic, LoadIconW, LoadImageW,
    MsgWaitForMultipleObjectsEx, PeekMessageW, PostMessageW, RegisterClassW,
    RegisterWindowMessageW, SetForegroundWindow, SetMenuDefaultItem, ShowWindow, TrackPopupMenu,
    TranslateMessage, HICON, IDI_APPLICATION, IMAGE_ICON, MF_CHECKED, MF_GRAYED, MF_SEPARATOR,
    MF_STRING, MSG, MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT, SM_CXSMICON, SM_CYSMICON,
    SW_RESTORE, TPM_BOTTOMALIGN, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP,
    WM_CONTEXTMENU, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_NULL, WM_QUIT, WM_RBUTTONUP, WNDCLASSW,
};

use crate::tray::{Choice, Item, Popup};

/// The message Windows sends our window when the icon is clicked.
const ICON_MESSAGE: u32 = WM_APP + 1;
const ICON_ID: u32 = 1;
/// The icon winresource embeds in the program.
const PROGRAM_ICON: usize = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Click {
    /// A plain click: do the default thing.
    Primary,
    /// A right click: show the menu.
    Menu,
}

thread_local! {
    /// Clicks the window procedure has seen and the loop has not handled.
    static CLICKS: RefCell<Vec<Click>> = const { RefCell::new(Vec::new()) };
    /// Explorer restarted and lost every icon; ours must be added again.
    static EXPLORER_RESTARTED: Cell<bool> = const { Cell::new(false) };
    /// The number Windows gave the "TaskbarCreated" message this session.
    static TASKBAR_CREATED: Cell<u32> = const { Cell::new(0) };
}

/// A string as Windows wants it: UTF-16 with a terminating zero.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Copy `text` into a fixed-size field, cut short if need be, always
/// leaving a terminating zero.
fn fill(field: &mut [u16], text: &str) {
    let room = field.len() - 1;
    let mut written = 0;
    for unit in text.encode_utf16().take(room) {
        field[written] = unit;
        written += 1;
    }
    // Do not leave half of a two-unit character at the cut.
    if written > 0 && (0xD800..0xDC00).contains(&field[written - 1]) {
        written -= 1;
    }
    field[written..].fill(0);
}

unsafe extern "system" fn window_procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == ICON_MESSAGE {
        let click = match lparam as u32 {
            WM_LBUTTONUP | WM_LBUTTONDBLCLK => Some(Click::Primary),
            WM_RBUTTONUP | WM_CONTEXTMENU => Some(Click::Menu),
            _ => None,
        };
        if let Some(click) = click {
            CLICKS.with(|clicks| clicks.borrow_mut().push(click));
        }
        return 0;
    }
    if message != 0 && message == TASKBAR_CREATED.get() {
        EXPLORER_RESTARTED.set(true);
        return 0;
    }
    // SAFETY: forwarding the arguments Windows gave us, unchanged.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

pub struct Icon {
    window: HWND,
    icon: HICON,
    tooltip: String,
}

impl Icon {
    /// Put the icon in the notification area. Must be called on the thread
    /// that will go on to call [`Icon::wait`].
    pub fn new(tooltip: &str) -> io::Result<Self> {
        let class = wide("ggez-tray");
        // Not "ggez": that is how the main window is found.
        let title = wide("ggez notification icon");
        // SAFETY: plain Win32 calls with pointers to locals that outlive
        // them; every result is checked before use.
        unsafe {
            let program = GetModuleHandleW(null());
            let registration = WNDCLASSW {
                lpfnWndProc: Some(window_procedure),
                hInstance: program,
                lpszClassName: class.as_ptr(),
                ..std::mem::zeroed()
            };
            if RegisterClassW(&registration) == 0 {
                return Err(io::Error::last_os_error());
            }
            // An ordinary window that is never shown. A message-only window
            // would not be told when Explorer restarts.
            let window = CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                0,
                0,
                0,
                0,
                0,
                null_mut(),
                null_mut(),
                program,
                null(),
            );
            if window.is_null() {
                return Err(io::Error::last_os_error());
            }
            TASKBAR_CREATED.set(RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()));
            let mut icon = LoadImageW(
                program,
                PROGRAM_ICON as *const u16,
                IMAGE_ICON,
                GetSystemMetrics(SM_CXSMICON),
                GetSystemMetrics(SM_CYSMICON),
                0,
            );
            if icon.is_null() {
                icon = LoadIconW(null_mut(), IDI_APPLICATION);
            }
            let tray = Self {
                window,
                icon,
                tooltip: tooltip.to_owned(),
            };
            if !tray.send(NIM_ADD, NIF_MESSAGE | NIF_ICON | NIF_TIP, None) {
                let error = io::Error::last_os_error();
                DestroyWindow(window);
                return Err(error);
            }
            Ok(tray)
        }
    }

    /// Add or change the icon. `popup` also raises a message beside it.
    fn send(&self, action: u32, flags: u32, popup: Option<&Popup>) -> bool {
        // SAFETY: NOTIFYICONDATAW is plain data; zero is a valid start, and
        // the call only reads it.
        unsafe {
            let mut data: NOTIFYICONDATAW = std::mem::zeroed();
            data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
            data.hWnd = self.window;
            data.uID = ICON_ID;
            data.uFlags = flags;
            data.uCallbackMessage = ICON_MESSAGE;
            data.hIcon = self.icon;
            fill(&mut data.szTip, &self.tooltip);
            if let Some(popup) = popup {
                fill(&mut data.szInfoTitle, &popup.title);
                fill(&mut data.szInfo, &popup.text);
                data.dwInfoFlags = if popup.warning {
                    NIIF_WARNING
                } else {
                    NIIF_INFO
                };
            }
            Shell_NotifyIconW(action, &data) != 0
        }
    }

    pub fn set_tooltip(&mut self, tooltip: &str) {
        if self.tooltip != tooltip {
            self.tooltip = tooltip.to_owned();
            self.send(NIM_MODIFY, NIF_TIP, None);
        }
    }

    pub fn notify(&self, popup: &Popup) {
        self.send(NIM_MODIFY, NIF_INFO, Some(popup));
    }

    /// Let Windows deliver what it has for us, waiting up to `timeout_ms`
    /// if there is nothing yet, and return the clicks on the icon. `None`
    /// means the program was asked to end.
    pub fn wait(&mut self, timeout_ms: u32) -> Option<Vec<Click>> {
        // SAFETY: `message` is a plain struct Windows fills in.
        unsafe {
            MsgWaitForMultipleObjectsEx(0, null(), timeout_ms, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
            let mut message: MSG = std::mem::zeroed();
            while PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) != 0 {
                if message.message == WM_QUIT {
                    return None;
                }
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        if EXPLORER_RESTARTED.replace(false) {
            self.send(NIM_ADD, NIF_MESSAGE | NIF_ICON | NIF_TIP, None);
        }
        Some(CLICKS.with(|clicks| std::mem::take(&mut *clicks.borrow_mut())))
    }

    /// Show the menu at the pointer and wait for a choice.
    pub fn show_menu(&self, items: &[Item]) -> Option<Choice> {
        // SAFETY: the menu handle is created, used and destroyed here, and
        // every string outlives the call that reads it.
        unsafe {
            let menu = CreatePopupMenu();
            if menu.is_null() {
                return None;
            }
            for (index, item) in items.iter().enumerate() {
                // Zero means "nothing chosen", so entries count from one.
                let id = index + 1;
                match item {
                    Item::Separator => {
                        AppendMenuW(menu, MF_SEPARATOR, 0, null());
                    }
                    Item::Label(text) => {
                        AppendMenuW(menu, MF_STRING | MF_GRAYED, id, wide(text).as_ptr());
                    }
                    Item::Entry {
                        text,
                        checked,
                        default,
                        ..
                    } => {
                        // An ampersand would otherwise underline a letter.
                        let text = wide(&text.replace('&', "&&"));
                        let ticked = if *checked { MF_CHECKED } else { 0 };
                        AppendMenuW(menu, MF_STRING | ticked, id, text.as_ptr());
                        if *default {
                            SetMenuDefaultItem(menu, id as u32, 0);
                        }
                    }
                }
            }
            let mut pointer = POINT { x: 0, y: 0 };
            GetCursorPos(&mut pointer);
            // Without this the menu does not close when the user clicks
            // elsewhere; the empty message afterwards is the other half of
            // the same long-standing Windows requirement.
            SetForegroundWindow(self.window);
            let chosen = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
                pointer.x,
                pointer.y,
                0,
                self.window,
                null(),
            );
            PostMessageW(self.window, WM_NULL, 0, 0);
            DestroyMenu(menu);
            match items.get((chosen as usize).checked_sub(1)?)? {
                Item::Entry { choice, .. } => Some(choice.clone()),
                _ => None,
            }
        }
    }
}

impl Drop for Icon {
    fn drop(&mut self) {
        self.send(NIM_DELETE, 0, None);
        // SAFETY: the window was created by `new` on this thread.
        unsafe {
            DestroyWindow(self.window);
        }
    }
}

/// Bring the window with this exact title to the front, restoring it if it
/// is minimized. Returns whether there was one.
pub fn bring_to_front(title: &str) -> bool {
    // SAFETY: the title is a valid zero-terminated string for the call.
    unsafe {
        // The class is the one winit gives its windows; without it a folder
        // window that happens to be called the same would match.
        let window = FindWindowW(wide("Window Class").as_ptr(), wide(title).as_ptr());
        if window.is_null() {
            return false;
        }
        if IsIconic(window) != 0 {
            ShowWindow(window, SW_RESTORE);
        }
        SetForegroundWindow(window);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_text_is_cut_to_fit_with_a_terminator() {
        let mut field = [0xFFFFu16; 8];
        fill(&mut field, "ggez");
        assert_eq!(field, [103, 103, 101, 122, 0, 0, 0, 0]);
        fill(&mut field, "a very long tooltip");
        assert_eq!(field[7], 0);
        assert_eq!(String::from_utf16_lossy(&field[..7]), "a very ");
        // A character that takes two units is not cut in half.
        let mut field = [0u16; 4];
        fill(&mut field, "ab\u{1F5B1}");
        assert_eq!(field, [97, 98, 0, 0]);
    }
}
