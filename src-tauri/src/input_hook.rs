use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum InputEvent {
    KeyDown {
        vk: u32,
        scan: u32,
        repeat: bool,
    },
    KeyUp {
        vk: u32,
        scan: u32,
    },
    MouseDown {
        button: String,
    },
    MouseUp {
        button: String,
    },
    MouseWheel {
        delta: i16,
    },
    MouseMove {
        x: i32,
        y: i32,
        nx: f64,
        ny: f64,
    },
    PrivacyTyping,
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
enum KeySource {
    Primary = 0, // Windows WH_KEYBOARD_LL or macOS CGEventTap
    Hid = 1,     // macOS IOHIDManager (Karabiner VirtualHIDKeyboard / Secure Input resilient)
}

enum RawHookMsg {
    KeyDown {
        vk: u32,
        scan: u32,
        source: KeySource,
    },
    KeyUp {
        vk: u32,
        scan: u32,
        source: KeySource,
    },
    MouseDown(&'static str),
    MouseUp(&'static str),
    MouseWheel(i16),
    MouseMove {
        x: i32,
        y: i32,
    },
}

static HOOK_TX: OnceLock<mpsc::Sender<RawHookMsg>> = OnceLock::new();
static KEY_EVENTS_RECEIVED: AtomicU64 = AtomicU64::new(0);
static ALT_HELD: AtomicBool = AtomicBool::new(false);
static HOOKS_PAUSED: AtomicBool = AtomicBool::new(false);
static PASSWORD_PRIVACY_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn set_password_privacy_active(active: bool) {
    PASSWORD_PRIVACY_ACTIVE.store(active, Ordering::SeqCst);
}
#[cfg(windows)]
static WIN_HOOK_TID: AtomicU32 = AtomicU32::new(0);
#[cfg(windows)]
const WM_KURO_PAUSE_TOGGLE: u32 = 0x8007; // WM_APP + 7

fn is_alt_vk(vk: u32) -> bool {
    matches!(vk, 0x12 | 0xA4 | 0xA5)
}

pub fn is_alt_pressed() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_MENU};
        let async_down = unsafe { (GetAsyncKeyState(VK_MENU as i32) as u16 & 0x8000) != 0 };
        if async_down {
            return true;
        }
    }
    ALT_HELD.load(Ordering::Relaxed)
}

pub fn set_hooks_paused(paused: bool) {
    HOOKS_PAUSED.store(paused, Ordering::SeqCst);
    if paused {
        ALT_HELD.store(false, Ordering::Relaxed);
    }
    #[cfg(windows)]
    {
        let tid = WIN_HOOK_TID.load(Ordering::SeqCst);
        if tid != 0 {
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::PostThreadMessageW(
                    tid,
                    WM_KURO_PAUSE_TOGGLE,
                    paused as usize,
                    0,
                );
            }
        }
    }
}

/// Deduplicates held key repeats AND cross-source duplicates when macOS CGEventTap
/// and IOHIDManager both report the same physical or Karabiner-synthesized keystroke.
struct KeyDeduplicator {
    logical_pressed: [bool; 512],
    last_down: [Option<(Instant, KeySource)>; 512],
}

impl KeyDeduplicator {
    fn new() -> Self {
        Self {
            logical_pressed: [false; 512],
            last_down: [None; 512],
        }
    }

    /// Returns `Some(repeat)` if a `KeyDown` should be emitted, or `None` if suppressed
    /// as a cross-source duplicate.
    fn on_key_down(&mut self, vk: u32, source: KeySource, now: Instant) -> Option<bool> {
        let idx = vk as usize;
        if idx >= 512 {
            return Some(false);
        }
        if self.logical_pressed[idx] {
            if let Some((_, prev_src)) = self.last_down[idx] {
                if prev_src != source {
                    // Shadow event from the parallel capture channel while key is already held.
                    return None;
                }
            }
            return Some(true);
        }
        // Guard against micro-tap race: channel A delivered Down+Up, then channel B delivers
        // the same keystroke's Down a few ms later.
        if let Some((prev_at, prev_src)) = self.last_down[idx] {
            if prev_src != source && now.duration_since(prev_at) < Duration::from_millis(40) {
                return None;
            }
        }
        self.logical_pressed[idx] = true;
        self.last_down[idx] = Some((now, source));
        Some(false)
    }

    /// Returns `true` if a `KeyUp` should be emitted.
    fn on_key_up(&mut self, vk: u32, _source: KeySource) -> bool {
        let idx = vk as usize;
        if idx >= 512 {
            return true;
        }
        if !self.logical_pressed[idx] {
            return false;
        }
        self.logical_pressed[idx] = false;
        true
    }
}

/// Maps USB HID Usage ID (Usage Page 0x07: Keyboard/Keypad) to Windows VK codes
/// used by the KuroDog frontend keyboard layout.
#[allow(dead_code)]
pub fn hid_usage_to_vk(usage: u32) -> Option<u32> {
    let vk = match usage {
        // Letters A..Z (0x04..=0x1D)
        0x04..=0x1D => 0x41 + (usage - 0x04),
        // Digits 1..9 (0x1E..=0x26), 0 (0x27)
        0x1E..=0x26 => 0x31 + (usage - 0x1E),
        0x27 => 0x30,
        // Control & editing
        0x28 => 0x0D, // Return / Enter
        0x29 => 0x1B, // Escape
        0x2A => 0x08, // Backspace (Delete)
        0x2B => 0x09, // Tab
        0x2C => 0x20, // Space
        // Punctuation
        0x2D => 0xBD, // -
        0x2E => 0xBB, // =
        0x2F => 0xDB, // [
        0x30 => 0xDD, // ]
        0x31 | 0x32 => 0xDC, // \ and Non-US #
        0x33 => 0xBA, // ;
        0x34 => 0xDE, // '
        0x35 => 0xC0, // `
        0x36 => 0xBC, // ,
        0x37 => 0xBE, // .
        0x38 => 0xBF, // /
        0x39 => 0x14, // CapsLock
        // Function keys F1..F12 (0x3A..=0x45)
        0x3A..=0x45 => 0x70 + (usage - 0x3A),
        // Navigation
        0x49 => 0x2D, // Insert
        0x4A => 0x24, // Home
        0x4B => 0x21, // PageUp
        0x4C => 0x2E, // Forward Delete
        0x4D => 0x23, // End
        0x4E => 0x22, // PageDown
        0x4F => 0x27, // Right
        0x50 => 0x25, // Left
        0x51 => 0x28, // Down
        0x52 => 0x26, // Up
        // Numpad
        0x53 => 0x90,  // NumLock / Clear
        0x54 => 0x6F,  // Numpad /
        0x55 => 0x6A,  // Numpad *
        0x56 => 0x6D,  // Numpad -
        0x57 => 0x6B,  // Numpad +
        0x58 => 0x10D, // Numpad Enter
        0x59..=0x61 => 0x61 + (usage - 0x59), // Numpad 1..9
        0x62 => 0x60,  // Numpad 0
        0x63 => 0x6E,  // Numpad .
        // Modifiers
        0xE0 => 0xA2, // Left Control
        0xE1 => 0xA0, // Left Shift
        0xE2 => 0xA4, // Left Option / Alt
        0xE3 => 0x5B, // Left Command / GUI
        0xE4 => 0xA3, // Right Control
        0xE5 => 0xA1, // Right Shift
        0xE6 => 0xA5, // Right Option / Alt
        0xE7 => 0x5C, // Right Command / GUI
        _ => return None,
    };
    Some(vk)
}

pub fn start_global_hooks(app: AppHandle) {
    let (tx, rx) = mpsc::channel::<RawHookMsg>();
    let _ = HOOK_TX.set(tx);

    // Dispatcher thread: deduplicates held key repeats & dual-channel macOS events,
    // and throttles mouse moves to ~60Hz
    std::thread::spawn(move || {
        let mut dedup = KeyDeduplicator::new();
        let mut last_mouse_emit = Instant::now() - Duration::from_secs(1);

        while let Ok(msg) = rx.recv() {
            if HOOKS_PAUSED.load(Ordering::Relaxed) {
                continue;
            }
            match msg {
                RawHookMsg::KeyDown { vk, scan, source } => {
                    if is_alt_vk(vk) {
                        ALT_HELD.store(true, Ordering::Relaxed);
                    }
                    if PASSWORD_PRIVACY_ACTIVE.load(Ordering::Relaxed) {
                        // Zero password keystrokes leave Rust — only emit a content-less privacy pulse!
                        if let Some(repeat) = dedup.on_key_down(vk, source, Instant::now()) {
                            if !repeat {
                                let _ = app.emit("kuro-input", InputEvent::PrivacyTyping);
                            }
                        }
                        continue;
                    }
                    if let Some(repeat) = dedup.on_key_down(vk, source, Instant::now()) {
                        if !repeat {
                            KEY_EVENTS_RECEIVED.fetch_add(1, Ordering::Relaxed);
                        }
                        let _ = app.emit("kuro-input", InputEvent::KeyDown { vk, scan, repeat });
                    }
                }
                RawHookMsg::KeyUp { vk, scan, source } => {
                    if is_alt_vk(vk) {
                        ALT_HELD.store(false, Ordering::Relaxed);
                    }
                    if PASSWORD_PRIVACY_ACTIVE.load(Ordering::Relaxed) {
                        let _ = dedup.on_key_up(vk, source);
                        continue;
                    }
                    if dedup.on_key_up(vk, source) {
                        let _ = app.emit("kuro-input", InputEvent::KeyUp { vk, scan });
                    }
                }
                RawHookMsg::MouseDown(btn) => {
                    let _ = app.emit(
                        "kuro-input",
                        InputEvent::MouseDown {
                            button: btn.to_string(),
                        },
                    );
                }
                RawHookMsg::MouseUp(btn) => {
                    let _ = app.emit(
                        "kuro-input",
                        InputEvent::MouseUp {
                            button: btn.to_string(),
                        },
                    );
                }
                RawHookMsg::MouseWheel(delta) => {
                    let _ = app.emit("kuro-input", InputEvent::MouseWheel { delta });
                }
                RawHookMsg::MouseMove { x, y } => {
                    let now = Instant::now();
                    if now.duration_since(last_mouse_emit) >= Duration::from_millis(16) {
                        last_mouse_emit = now;
                        let (nx, ny) = normalize_screen_point(x, y);
                        let _ = app.emit("kuro-input", InputEvent::MouseMove { x, y, nx, ny });
                    }
                }
            }
        }
    });

    #[cfg(windows)]
    std::thread::spawn(|| {
        run_windows_hook_loop();
    });

    #[cfg(target_os = "macos")]
    std::thread::spawn(|| {
        macos_hook::run_macos_hook_loop();
    });
}

#[cfg(windows)]
fn normalize_screen_point(x: i32, y: i32) -> (f64, f64) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };
    unsafe {
        let vx = GetSystemMetrics(SM_XVIRTUALSCREEN) as f64;
        let vy = GetSystemMetrics(SM_YVIRTUALSCREEN) as f64;
        let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1) as f64;
        let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1) as f64;
        let nx = ((x as f64 - vx) / vw).clamp(0.0, 1.0);
        let ny = ((y as f64 - vy) / vh).clamp(0.0, 1.0);
        (nx, ny)
    }
}

#[cfg(target_os = "macos")]
fn normalize_screen_point(x: i32, y: i32) -> (f64, f64) {
    let (w, h) = macos_hook::main_display_size();
    let nx = (x as f64 / w.max(1.0)).clamp(0.0, 1.0);
    let ny = (y as f64 / h.max(1.0)).clamp(0.0, 1.0);
    (nx, ny)
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn normalize_screen_point(_x: i32, _y: i32) -> (f64, f64) {
    (0.5, 0.5)
}

#[cfg(windows)]
unsafe extern "system" fn keyboard_hook_proc(code: i32, wparam: usize, lparam: isize) -> isize {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, KBDLLHOOKSTRUCT, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    if code >= 0 {
        let kb = &*(lparam as *const KBDLLHOOKSTRUCT);
        let msg = wparam as u32;
        // Distinguish Numpad Enter (VK_RETURN with LLKHF_EXTENDED=0x01) from main Enter for 108-key layouts
        let mapped_vk = if kb.vkCode == 0x0d && (kb.flags & 0x01) != 0 {
            0x10d
        } else {
            kb.vkCode
        };
        if let Some(tx) = HOOK_TX.get() {
            match msg {
                WM_KEYDOWN | WM_SYSKEYDOWN => {
                    let _ = tx.send(RawHookMsg::KeyDown {
                        vk: mapped_vk,
                        scan: kb.scanCode,
                        source: KeySource::Primary,
                    });
                }
                WM_KEYUP | WM_SYSKEYUP => {
                    let _ = tx.send(RawHookMsg::KeyUp {
                        vk: mapped_vk,
                        scan: kb.scanCode,
                        source: KeySource::Primary,
                    });
                }
                _ => {}
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

#[cfg(windows)]
unsafe extern "system" fn mouse_hook_proc(code: i32, wparam: usize, lparam: isize) -> isize {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, MSLLHOOKSTRUCT, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN,
        WM_MBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONDOWN, WM_RBUTTONUP,
    };

    if code >= 0 {
        let ms = &*(lparam as *const MSLLHOOKSTRUCT);
        let msg = wparam as u32;
        if let Some(tx) = HOOK_TX.get() {
            match msg {
                WM_MOUSEMOVE => {
                    let _ = tx.send(RawHookMsg::MouseMove {
                        x: ms.pt.x,
                        y: ms.pt.y,
                    });
                }
                WM_LBUTTONDOWN => {
                    let _ = tx.send(RawHookMsg::MouseDown("left"));
                }
                WM_LBUTTONUP => {
                    let _ = tx.send(RawHookMsg::MouseUp("left"));
                }
                WM_RBUTTONDOWN => {
                    let _ = tx.send(RawHookMsg::MouseDown("right"));
                }
                WM_RBUTTONUP => {
                    let _ = tx.send(RawHookMsg::MouseUp("right"));
                }
                WM_MBUTTONDOWN => {
                    let _ = tx.send(RawHookMsg::MouseDown("middle"));
                }
                WM_MBUTTONUP => {
                    let _ = tx.send(RawHookMsg::MouseUp("middle"));
                }
                WM_MOUSEWHEEL => {
                    let delta = ((ms.mouseData >> 16) & 0xffff) as i16;
                    let _ = tx.send(RawHookMsg::MouseWheel(delta));
                }
                _ => {}
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

#[cfg(windows)]
fn run_windows_hook_loop() {
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetMessageW, PeekMessageW, SetWindowsHookExW, UnhookWindowsHookEx, MSG, PM_NOREMOVE,
        WH_KEYBOARD_LL, WH_MOUSE_LL,
    };

    unsafe {
        // Ensure the thread message queue exists before publishing WIN_HOOK_TID
        let mut init_msg: MSG = std::mem::zeroed();
        PeekMessageW(&mut init_msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
        WIN_HOOK_TID.store(GetCurrentThreadId(), Ordering::SeqCst);

        let hmod = GetModuleHandleW(std::ptr::null());
        let mut kb_hook = if HOOKS_PAUSED.load(Ordering::SeqCst) {
            std::ptr::null_mut()
        } else {
            SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook_proc), hmod, 0)
        };
        let mut ms_hook = if HOOKS_PAUSED.load(Ordering::SeqCst) {
            std::ptr::null_mut()
        } else {
            SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook_proc), hmod, 0)
        };

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            if msg.message == WM_KURO_PAUSE_TOGGLE {
                let pause = msg.wParam != 0;
                if pause {
                    if !kb_hook.is_null() {
                        UnhookWindowsHookEx(kb_hook);
                        kb_hook = std::ptr::null_mut();
                    }
                    if !ms_hook.is_null() {
                        UnhookWindowsHookEx(ms_hook);
                        ms_hook = std::ptr::null_mut();
                    }
                } else {
                    if kb_hook.is_null() {
                        kb_hook =
                            SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook_proc), hmod, 0);
                    }
                    if ms_hook.is_null() {
                        ms_hook = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook_proc), hmod, 0);
                    }
                }
            }
        }

        if !kb_hook.is_null() {
            UnhookWindowsHookEx(kb_hook);
        }
        if !ms_hook.is_null() {
            UnhookWindowsHookEx(ms_hook);
        }
    }
}

#[cfg(target_os = "macos")]
pub mod macos_hook {
    use super::{hid_usage_to_vk, KeySource, RawHookMsg, HOOK_TX, KEY_EVENTS_RECEIVED};
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    type CGEventTapProxy = *mut c_void;
    type CGEventRef = *mut c_void;
    type CFMachPortRef = *mut c_void;
    type CFRunLoopSourceRef = *mut c_void;
    type CFRunLoopTimerRef = *mut c_void;
    type CFRunLoopRef = *mut c_void;
    type CFStringRef = *const c_void;
    type CFDictionaryRef = *const c_void;
    type CFArrayRef = *const c_void;
    type CFAllocatorRef = *const c_void;
    type CFBooleanRef = *const c_void;
    type CGEventType = u32;
    type CGEventMask = u64;
    type CGDirectDisplayID = u32;

    type IOHIDManagerRef = *mut c_void;
    type IOHIDValueRef = *mut c_void;
    type IOHIDElementRef = *mut c_void;
    type IOReturn = i32;
    type IOOptionBits = u32;

    #[repr(C)]
    #[derive(Copy, Clone, Debug)]
    struct CGPoint {
        x: f64,
        y: f64,
    }

    #[repr(C)]
    #[derive(Copy, Clone, Debug)]
    struct CGSize {
        width: f64,
        height: f64,
    }

    #[repr(C)]
    #[derive(Copy, Clone, Debug)]
    struct CGRect {
        origin: CGPoint,
        size: CGSize,
    }

    const K_CG_EVENT_LEFT_MOUSE_DOWN: u32 = 1;
    const K_CG_EVENT_LEFT_MOUSE_UP: u32 = 2;
    const K_CG_EVENT_RIGHT_MOUSE_DOWN: u32 = 3;
    const K_CG_EVENT_RIGHT_MOUSE_UP: u32 = 4;
    const K_CG_EVENT_MOUSE_MOVED: u32 = 5;
    const K_CG_EVENT_LEFT_MOUSE_DRAGGED: u32 = 6;
    const K_CG_EVENT_RIGHT_MOUSE_DRAGGED: u32 = 7;
    const K_CG_EVENT_KEY_DOWN: u32 = 10;
    const K_CG_EVENT_KEY_UP: u32 = 11;
    const K_CG_EVENT_FLAGS_CHANGED: u32 = 12;
    const K_CG_EVENT_SCROLL_WHEEL: u32 = 22;
    const K_CG_EVENT_OTHER_MOUSE_DOWN: u32 = 25;
    const K_CG_EVENT_OTHER_MOUSE_UP: u32 = 26;
    const K_CG_EVENT_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFFFFFE;
    const K_CG_EVENT_TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFFFFFF;

    const K_CG_KEYBOARD_EVENT_KEYCODE: u32 = 9;
    const K_CG_SCROLL_WHEEL_EVENT_DELTA_AXIS_1: u32 = 11;

    const K_CG_SESSION_EVENT_TAP: u32 = 1;
    const K_CG_HEAD_INSERT_EVENT_TAP: u32 = 0;
    const K_CG_EVENT_TAP_OPTION_DEFAULT: u32 = 0;
    const K_CG_EVENT_TAP_OPTION_LISTEN_ONLY: u32 = 1;

    const K_CF_STRING_ENCODING_UTF8: u32 = 0x08000100;
    const K_CF_NUMBER_SINT32_TYPE: isize = 3;

    const K_IO_HID_OPTIONS_TYPE_NONE: IOOptionBits = 0;
    const K_IO_RETURN_SUCCESS: IOReturn = 0;
    const K_HID_PAGE_GENERIC_DESKTOP: i32 = 0x01;
    const K_HID_USAGE_GD_KEYBOARD: i32 = 0x06;
    const K_HID_USAGE_GD_KEYPAD: i32 = 0x07;
    const K_HID_PAGE_KEYBOARD_OR_KEYPAD: u32 = 0x07;

    // CGEventFlags modifier masks
    const FLAG_CAPS_LOCK: u64 = 0x00010000;
    const FLAG_SHIFT: u64 = 0x00020000;
    const FLAG_CONTROL: u64 = 0x00040000;
    const FLAG_OPTION: u64 = 0x00080000;
    const FLAG_COMMAND: u64 = 0x00100000;

    type CGEventTapCallBack = extern "C" fn(
        proxy: CGEventTapProxy,
        event_type: CGEventType,
        event: CGEventRef,
        user_info: *mut c_void,
    ) -> CGEventRef;

    type CFRunLoopTimerCallBack = extern "C" fn(timer: CFRunLoopTimerRef, info: *mut c_void);

    type IOHIDValueCallback = extern "C" fn(
        context: *mut c_void,
        result: IOReturn,
        sender: *mut c_void,
        value: IOHIDValueRef,
    );

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGPreflightListenEventAccess() -> bool;
        fn CGRequestListenEventAccess() -> bool;
        fn CGEventTapCreate(
            tap: u32,
            place: u32,
            options: u32,
            events_of_interest: CGEventMask,
            callback: CGEventTapCallBack,
            user_info: *mut c_void,
        ) -> CFMachPortRef;
        fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
        fn CGEventTapIsEnabled(tap: CFMachPortRef) -> bool;
        fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
        fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
        fn CGEventGetFlags(event: CGEventRef) -> u64;
        fn CGMainDisplayID() -> CGDirectDisplayID;
        fn CGDisplayBounds(display: CGDirectDisplayID) -> CGRect;
        fn CGEventCreate(source: *const c_void) -> CGEventRef;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFRunLoopCommonModes: CFStringRef;
        static kCFBooleanTrue: CFBooleanRef;
        static kCFTypeDictionaryKeyCallBacks: u8;
        static kCFTypeDictionaryValueCallBacks: u8;
        static kCFTypeArrayCallBacks: u8;

        fn CFStringCreateWithBytes(
            alloc: CFAllocatorRef,
            bytes: *const u8,
            num_bytes: isize,
            encoding: u32,
            is_external_representation: u8,
        ) -> CFStringRef;
        fn CFNumberCreate(
            allocator: CFAllocatorRef,
            the_type: isize,
            value_ptr: *const c_void,
        ) -> *const c_void;
        fn CFArrayCreate(
            allocator: CFAllocatorRef,
            values: *const *const c_void,
            num_values: isize,
            call_backs: *const c_void,
        ) -> CFArrayRef;
        fn CFDictionaryCreate(
            allocator: CFAllocatorRef,
            keys: *const *const c_void,
            values: *const *const c_void,
            num_values: isize,
            key_callbacks: *const c_void,
            value_callbacks: *const c_void,
        ) -> CFDictionaryRef;
        fn CFMachPortCreateRunLoopSource(
            allocator: CFAllocatorRef,
            port: CFMachPortRef,
            order: isize,
        ) -> CFRunLoopSourceRef;
        fn CFMachPortInvalidate(port: CFMachPortRef);
        fn CFRunLoopGetCurrent() -> CFRunLoopRef;
        fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
        fn CFRunLoopRemoveSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
        fn CFRunLoopRun();
        fn CFRunLoopStop(rl: CFRunLoopRef);
        fn CFAbsoluteTimeGetCurrent() -> f64;
        fn CFRunLoopTimerCreate(
            allocator: CFAllocatorRef,
            fire_date: f64,
            interval: f64,
            flags: usize,
            order: isize,
            callout: CFRunLoopTimerCallBack,
            context: *mut c_void,
        ) -> CFRunLoopTimerRef;
        fn CFRunLoopAddTimer(rl: CFRunLoopRef, timer: CFRunLoopTimerRef, mode: CFStringRef);
        fn CFRunLoopTimerInvalidate(timer: CFRunLoopTimerRef);
        fn CFRelease(cf: *const c_void);
    }

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOHIDManagerCreate(allocator: CFAllocatorRef, options: IOOptionBits) -> IOHIDManagerRef;
        fn IOHIDManagerSetDeviceMatchingMultiple(manager: IOHIDManagerRef, multiple: CFArrayRef);
        fn IOHIDManagerRegisterInputValueCallback(
            manager: IOHIDManagerRef,
            callback: Option<IOHIDValueCallback>,
            context: *mut c_void,
        );
        fn IOHIDManagerScheduleWithRunLoop(
            manager: IOHIDManagerRef,
            run_loop: CFRunLoopRef,
            run_loop_mode: CFStringRef,
        );
        fn IOHIDManagerUnscheduleFromRunLoop(
            manager: IOHIDManagerRef,
            run_loop: CFRunLoopRef,
            run_loop_mode: CFStringRef,
        );
        fn IOHIDManagerOpen(manager: IOHIDManagerRef, options: IOOptionBits) -> IOReturn;
        fn IOHIDManagerClose(manager: IOHIDManagerRef, options: IOOptionBits) -> IOReturn;
        fn IOHIDValueGetElement(value: IOHIDValueRef) -> IOHIDElementRef;
        fn IOHIDValueGetIntegerValue(value: IOHIDValueRef) -> isize;
        fn IOHIDElementGetUsagePage(element: IOHIDElementRef) -> u32;
        fn IOHIDElementGetUsage(element: IOHIDElementRef) -> u32;
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        static kAXTrustedCheckOptionPrompt: CFStringRef;
        fn AXIsProcessTrusted() -> bool;
        fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    }

    static TAP_PORT: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
    static TAP_HAS_KEYBOARD: AtomicBool = AtomicBool::new(false);
    static HID_ACTIVE: AtomicBool = AtomicBool::new(false);
    static SNAPSHOT_AX: AtomicBool = AtomicBool::new(false);
    static SNAPSHOT_LISTEN: AtomicBool = AtomicBool::new(false);
    static RECONNECT_REQUESTED: AtomicBool = AtomicBool::new(false);
    static LAST_TICK_WALL_MS: AtomicU64 = AtomicU64::new(0);
    static LAST_HID_RETRY_WALL_MS: AtomicU64 = AtomicU64::new(0);

    #[derive(Debug, Clone)]
    pub struct MacInputStatus {
        pub accessibility: bool,
        pub input_monitoring: bool,
        pub tap_active: bool,
        pub tap_has_keyboard: bool,
        pub hid_active: bool,
        pub key_events_received: u64,
    }

    fn wall_clock_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    pub fn get_macos_input_status() -> MacInputStatus {
        let tap = TAP_PORT.load(Ordering::Acquire);
        let tap_active = !tap.is_null() && unsafe { CGEventTapIsEnabled(tap) };
        MacInputStatus {
            accessibility: unsafe { AXIsProcessTrusted() },
            input_monitoring: unsafe { CGPreflightListenEventAccess() },
            tap_active,
            tap_has_keyboard: TAP_HAS_KEYBOARD.load(Ordering::Acquire),
            hid_active: HID_ACTIVE.load(Ordering::Acquire),
            key_events_received: KEY_EVENTS_RECEIVED.load(Ordering::Relaxed),
        }
    }

    pub fn trigger_macos_reconnect() {
        unsafe {
            if !CGPreflightListenEventAccess() {
                let _ = CGRequestListenEventAccess();
            }
        }
        RECONNECT_REQUESTED.store(true, Ordering::Release);
    }

    pub fn open_macos_privacy_pane(pane: &str) {
        let url = match pane {
            "input_monitoring" => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"
            }
            _ => "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
        };
        let _ = std::process::Command::new("open").arg(url).spawn();
    }

    pub fn show_macos_diagnostics_dialog() {
        let st = get_macos_input_status();
        let ax_txt = if st.accessibility {
            "已授权"
        } else {
            "未生效"
        };
        let listen_txt = if st.input_monitoring {
            "已授权"
        } else {
            "未生效"
        };
        let tap_txt = if st.tap_active && st.tap_has_keyboard {
            "运行中（键鼠）"
        } else if st.tap_active {
            "仅鼠标（缺少键盘权限）"
        } else {
            "未运行"
        };
        let hid_txt = if st.hid_active {
            "运行中（兼容 Karabiner）"
        } else {
            "未运行（需输入监控权限）"
        };
        let tip = if !st.accessibility || !st.input_monitoring {
            "若系统设置里开关亮着但这里显示「未生效」，说明更新版本后系统缓存过期：请在「辅助功能」和「输入监控」中选中 KuroDog 点左下角「-」号删除，再点「+」号重新添加并重启。"
        } else if st.key_events_received == 0 {
            "权限均已生效！请先随便敲几下键盘测试；若使用 Karabiner-Elements 仍无响应，可点击托盘「重新连接键盘监听」。"
        } else {
            "键盘监听工作正常！"
        };

        let msg = format!(
            "辅助功能权限：{}\n输入监控权限：{}\nCGEventTap 通道：{}\nIOHIDManager 通道：{}\n已收到全局按键：{} 次\n\n{}",
            ax_txt, listen_txt, tap_txt, hid_txt, st.key_events_received, tip
        );
        let escaped = msg
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n");
        let script = format!(
            "display dialog \"{}\" with title \"KuroDog 键盘监听诊断\" buttons {{\"确定\"}} default button \"确定\"",
            escaped
        );
        let _ = std::process::Command::new("osascript")
            .args(["-e", &script])
            .spawn();
    }

    pub fn main_display_size() -> (f64, f64) {
        unsafe {
            let bounds = CGDisplayBounds(CGMainDisplayID());
            (bounds.size.width.max(1.0), bounds.size.height.max(1.0))
        }
    }

    pub fn cursor_pos_points() -> Option<(f64, f64)> {
        unsafe {
            let ev = CGEventCreate(std::ptr::null());
            if ev.is_null() {
                return None;
            }
            let pt = CGEventGetLocation(ev);
            CFRelease(ev as *const c_void);
            Some((pt.x, pt.y))
        }
    }

    fn request_permissions_once() {
        unsafe {
            if !AXIsProcessTrusted() {
                let keys = [kAXTrustedCheckOptionPrompt as *const c_void];
                let vals = [kCFBooleanTrue as *const c_void];
                let opts = CFDictionaryCreate(
                    std::ptr::null(),
                    keys.as_ptr(),
                    vals.as_ptr(),
                    1,
                    std::ptr::null(),
                    std::ptr::null(),
                );
                let _ = AXIsProcessTrustedWithOptions(opts);
                if !opts.is_null() {
                    CFRelease(opts as *const c_void);
                }
            }
            if !CGPreflightListenEventAccess() {
                let _ = CGRequestListenEventAccess();
            }
        }
    }

    fn macos_keycode_to_vk(kc: u16) -> Option<u32> {
        let vk = match kc {
            // Letters
            0x00 => 0x41, // A
            0x0B => 0x42, // B
            0x08 => 0x43, // C
            0x02 => 0x44, // D
            0x0E => 0x45, // E
            0x03 => 0x46, // F
            0x05 => 0x47, // G
            0x04 => 0x48, // H
            0x22 => 0x49, // I
            0x26 => 0x4A, // J
            0x28 => 0x4B, // K
            0x25 => 0x4C, // L
            0x2E => 0x4D, // M
            0x2D => 0x4E, // N
            0x1F => 0x4F, // O
            0x23 => 0x50, // P
            0x0C => 0x51, // Q
            0x0F => 0x52, // R
            0x01 => 0x53, // S
            0x11 => 0x54, // T
            0x20 => 0x55, // U
            0x09 => 0x56, // V
            0x0D => 0x57, // W
            0x07 => 0x58, // X
            0x10 => 0x59, // Y
            0x06 => 0x5A, // Z
            // Digits
            0x1D => 0x30, // 0
            0x12 => 0x31, // 1
            0x13 => 0x32, // 2
            0x14 => 0x33, // 3
            0x15 => 0x34, // 4
            0x17 => 0x35, // 5
            0x16 => 0x36, // 6
            0x1A => 0x37, // 7
            0x1C => 0x38, // 8
            0x19 => 0x39, // 9
            // Punctuation
            0x1B => 0xBD, // -
            0x18 => 0xBB, // =
            0x21 => 0xDB, // [
            0x1E => 0xDD, // ]
            0x2A => 0xDC, // \
            0x29 => 0xBA, // ;
            0x27 => 0xDE, // '
            0x2B => 0xBC, // ,
            0x2F => 0xBE, // .
            0x2C => 0xBF, // /
            0x32 => 0xC0, // `
            // Control & editing
            0x35 => 0x1B,  // Esc
            0x30 => 0x09,  // Tab
            0x31 => 0x20,  // Space
            0x24 => 0x0D,  // Return
            0x4C => 0x10D, // Numpad Enter
            0x33 => 0x08,  // Backspace (Delete)
            0x75 => 0x2E,  // Forward Delete
            0x39 => 0x14,  // CapsLock
            // Numpad
            0x47 => 0x90, // Clear / NumLock
            0x4B => 0x6F, // Numpad /
            0x43 => 0x6A, // Numpad *
            0x4E => 0x6D, // Numpad -
            0x45 => 0x6B, // Numpad +
            0x41 => 0x6E, // Numpad .
            0x52 => 0x60, // Numpad 0
            0x53 => 0x61, // Numpad 1
            0x54 => 0x62, // Numpad 2
            0x55 => 0x63, // Numpad 3
            0x56 => 0x64, // Numpad 4
            0x57 => 0x65, // Numpad 5
            0x58 => 0x66, // Numpad 6
            0x59 => 0x67, // Numpad 7
            0x5B => 0x68, // Numpad 8
            0x5C => 0x69, // Numpad 9
            // Modifiers
            0x38 => 0xA0, // LShift
            0x3C => 0xA1, // RShift
            0x3B => 0xA2, // LCtrl
            0x3E => 0xA3, // RCtrl
            0x3A => 0xA4, // LOption (Alt)
            0x3D => 0xA5, // ROption (Alt)
            0x37 => 0x5B, // LCommand (Win)
            0x36 => 0x5C, // RCommand (Win)
            // Arrows & navigation
            0x7B => 0x25, // Left
            0x7C => 0x27, // Right
            0x7D => 0x28, // Down
            0x7E => 0x26, // Up
            0x73 => 0x24, // Home
            0x77 => 0x23, // End
            0x74 => 0x21, // PgUp
            0x79 => 0x22, // PgDn
            // Function keys
            0x7A => 0x70, // F1
            0x78 => 0x71, // F2
            0x63 => 0x72, // F3
            0x76 => 0x73, // F4
            0x60 => 0x74, // F5
            0x61 => 0x75, // F6
            0x62 => 0x76, // F7
            0x64 => 0x77, // F8
            0x65 => 0x78, // F9
            0x6D => 0x79, // F10
            0x67 => 0x7A, // F11
            0x6F => 0x7B, // F12
            _ => return None,
        };
        Some(vk)
    }

    extern "C" fn tap_callback(
        _proxy: CGEventTapProxy,
        event_type: CGEventType,
        event: CGEventRef,
        _user_info: *mut c_void,
    ) -> CGEventRef {
        if event_type == K_CG_EVENT_TAP_DISABLED_BY_TIMEOUT
            || event_type == K_CG_EVENT_TAP_DISABLED_BY_USER_INPUT
        {
            let tap = TAP_PORT.load(Ordering::Acquire);
            if !tap.is_null() {
                unsafe { CGEventTapEnable(tap, true) };
            }
            return event;
        }

        if let Some(tx) = HOOK_TX.get() {
            unsafe {
                match event_type {
                    K_CG_EVENT_KEY_DOWN => {
                        let kc =
                            CGEventGetIntegerValueField(event, K_CG_KEYBOARD_EVENT_KEYCODE) as u16;
                        if let Some(vk) = macos_keycode_to_vk(kc) {
                            let _ = tx.send(RawHookMsg::KeyDown {
                                vk,
                                scan: kc as u32,
                                source: KeySource::Primary,
                            });
                        }
                    }
                    K_CG_EVENT_KEY_UP => {
                        let kc =
                            CGEventGetIntegerValueField(event, K_CG_KEYBOARD_EVENT_KEYCODE) as u16;
                        if let Some(vk) = macos_keycode_to_vk(kc) {
                            let _ = tx.send(RawHookMsg::KeyUp {
                                vk,
                                scan: kc as u32,
                                source: KeySource::Primary,
                            });
                        }
                    }
                    K_CG_EVENT_FLAGS_CHANGED => {
                        let kc =
                            CGEventGetIntegerValueField(event, K_CG_KEYBOARD_EVENT_KEYCODE) as u16;
                        let flags = CGEventGetFlags(event);
                        let is_down = match kc {
                            0x38 | 0x3C => (flags & FLAG_SHIFT) != 0,
                            0x3B | 0x3E => (flags & FLAG_CONTROL) != 0,
                            0x3A | 0x3D => (flags & FLAG_OPTION) != 0,
                            0x37 | 0x36 => (flags & FLAG_COMMAND) != 0,
                            0x39 => (flags & FLAG_CAPS_LOCK) != 0,
                            _ => false,
                        };
                        if let Some(vk) = macos_keycode_to_vk(kc) {
                            if is_down {
                                let _ = tx.send(RawHookMsg::KeyDown {
                                    vk,
                                    scan: kc as u32,
                                    source: KeySource::Primary,
                                });
                            } else {
                                let _ = tx.send(RawHookMsg::KeyUp {
                                    vk,
                                    scan: kc as u32,
                                    source: KeySource::Primary,
                                });
                            }
                        }
                    }
                    K_CG_EVENT_MOUSE_MOVED
                    | K_CG_EVENT_LEFT_MOUSE_DRAGGED
                    | K_CG_EVENT_RIGHT_MOUSE_DRAGGED => {
                        let pt = CGEventGetLocation(event);
                        let _ = tx.send(RawHookMsg::MouseMove {
                            x: pt.x.round() as i32,
                            y: pt.y.round() as i32,
                        });
                    }
                    K_CG_EVENT_LEFT_MOUSE_DOWN => {
                        let _ = tx.send(RawHookMsg::MouseDown("left"));
                    }
                    K_CG_EVENT_LEFT_MOUSE_UP => {
                        let _ = tx.send(RawHookMsg::MouseUp("left"));
                    }
                    K_CG_EVENT_RIGHT_MOUSE_DOWN => {
                        let _ = tx.send(RawHookMsg::MouseDown("right"));
                    }
                    K_CG_EVENT_RIGHT_MOUSE_UP => {
                        let _ = tx.send(RawHookMsg::MouseUp("right"));
                    }
                    K_CG_EVENT_OTHER_MOUSE_DOWN => {
                        let _ = tx.send(RawHookMsg::MouseDown("middle"));
                    }
                    K_CG_EVENT_OTHER_MOUSE_UP => {
                        let _ = tx.send(RawHookMsg::MouseUp("middle"));
                    }
                    K_CG_EVENT_SCROLL_WHEEL => {
                        let delta = CGEventGetIntegerValueField(
                            event,
                            K_CG_SCROLL_WHEEL_EVENT_DELTA_AXIS_1,
                        ) as i16;
                        let _ = tx.send(RawHookMsg::MouseWheel(delta));
                    }
                    _ => {}
                }
            }
        }
        event
    }

    extern "C" fn hid_input_callback(
        _context: *mut c_void,
        _result: IOReturn,
        _sender: *mut c_void,
        value: IOHIDValueRef,
    ) {
        if value.is_null() {
            return;
        }
        unsafe {
            let elem = IOHIDValueGetElement(value);
            if elem.is_null() {
                return;
            }
            if IOHIDElementGetUsagePage(elem) != K_HID_PAGE_KEYBOARD_OR_KEYPAD {
                return;
            }
            let usage = IOHIDElementGetUsage(elem);
            if !(0x04..=0xE7).contains(&usage) {
                return;
            }
            let Some(vk) = hid_usage_to_vk(usage) else {
                return;
            };
            let pressed = IOHIDValueGetIntegerValue(value) != 0;
            if let Some(tx) = HOOK_TX.get() {
                if pressed {
                    let _ = tx.send(RawHookMsg::KeyDown {
                        vk,
                        scan: usage,
                        source: KeySource::Hid,
                    });
                } else {
                    let _ = tx.send(RawHookMsg::KeyUp {
                        vk,
                        scan: usage,
                        source: KeySource::Hid,
                    });
                }
            }
        }
    }

    unsafe fn cf_string_from_str(s: &str) -> CFStringRef {
        CFStringCreateWithBytes(
            std::ptr::null(),
            s.as_ptr(),
            s.len() as isize,
            K_CF_STRING_ENCODING_UTF8,
            0,
        )
    }

    unsafe fn make_hid_matching_dict(usage_page: i32, usage: i32) -> CFDictionaryRef {
        let k_page = cf_string_from_str("DeviceUsagePage");
        let k_usage = cf_string_from_str("DeviceUsage");
        let v_page = CFNumberCreate(
            std::ptr::null(),
            K_CF_NUMBER_SINT32_TYPE,
            &usage_page as *const i32 as *const c_void,
        );
        let v_usage = CFNumberCreate(
            std::ptr::null(),
            K_CF_NUMBER_SINT32_TYPE,
            &usage as *const i32 as *const c_void,
        );
        let keys = [k_page as *const c_void, k_usage as *const c_void];
        let vals = [v_page, v_usage];
        let dict = CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            vals.as_ptr(),
            2,
            &kCFTypeDictionaryKeyCallBacks as *const u8 as *const c_void,
            &kCFTypeDictionaryValueCallBacks as *const u8 as *const c_void,
        );
        if !k_page.is_null() {
            CFRelease(k_page);
        }
        if !k_usage.is_null() {
            CFRelease(k_usage);
        }
        if !v_page.is_null() {
            CFRelease(v_page);
        }
        if !v_usage.is_null() {
            CFRelease(v_usage);
        }
        dict
    }

    /// Creates a non-exclusive read-only IOHIDManager subscribed to all Keyboard & Keypad
    /// devices (including Karabiner-Elements DriverKit VirtualHIDKeyboard).
    unsafe fn start_hid_keyboard_listener(rl: CFRunLoopRef) -> IOHIDManagerRef {
        let mgr = IOHIDManagerCreate(std::ptr::null(), K_IO_HID_OPTIONS_TYPE_NONE);
        if mgr.is_null() {
            return std::ptr::null_mut();
        }
        let d_kb = make_hid_matching_dict(K_HID_PAGE_GENERIC_DESKTOP, K_HID_USAGE_GD_KEYBOARD);
        let d_kp = make_hid_matching_dict(K_HID_PAGE_GENERIC_DESKTOP, K_HID_USAGE_GD_KEYPAD);
        if !d_kb.is_null() && !d_kp.is_null() {
            let dicts = [d_kb as *const c_void, d_kp as *const c_void];
            let arr = CFArrayCreate(
                std::ptr::null(),
                dicts.as_ptr(),
                2,
                &kCFTypeArrayCallBacks as *const u8 as *const c_void,
            );
            if !arr.is_null() {
                IOHIDManagerSetDeviceMatchingMultiple(mgr, arr);
                CFRelease(arr);
            }
        }
        if !d_kb.is_null() {
            CFRelease(d_kb);
        }
        if !d_kp.is_null() {
            CFRelease(d_kp);
        }

        IOHIDManagerRegisterInputValueCallback(mgr, Some(hid_input_callback), std::ptr::null_mut());
        IOHIDManagerScheduleWithRunLoop(mgr, rl, kCFRunLoopCommonModes);

        let ret = IOHIDManagerOpen(mgr, K_IO_HID_OPTIONS_TYPE_NONE);
        if ret != K_IO_RETURN_SUCCESS {
            IOHIDManagerRegisterInputValueCallback(mgr, None, std::ptr::null_mut());
            IOHIDManagerUnscheduleFromRunLoop(mgr, rl, kCFRunLoopCommonModes);
            CFRelease(mgr as *const c_void);
            return std::ptr::null_mut();
        }
        mgr
    }

    /// Periodic self-healing watchdog running on the exact same CFRunLoop thread.
    /// Automatically tears down & rebuilds listeners when:
    /// 1. The user grants Input Monitoring or Accessibility after launch (no restart needed!)
    /// 2. The Mac wakes from sleep (wall-clock jump > 12s, where Karabiner/WindowServer taps often stale)
    /// 3. The CGEventTap gets disabled and cannot be re-enabled in place
    /// 4. The user clicks "重新连接键盘监听" in the menu bar
    extern "C" fn health_timer_callback(_timer: CFRunLoopTimerRef, _info: *mut c_void) {
        unsafe {
            let now_ms = wall_clock_ms();
            let prev_ms = LAST_TICK_WALL_MS.swap(now_ms, Ordering::AcqRel);
            let woke_from_sleep = prev_ms > 0 && now_ms.saturating_sub(prev_ms) > 12_000;

            let ax_now = AXIsProcessTrusted();
            let listen_now = CGPreflightListenEventAccess();
            let ax_upgraded = !SNAPSHOT_AX.load(Ordering::Acquire) && ax_now;
            let listen_upgraded = !SNAPSHOT_LISTEN.load(Ordering::Acquire) && listen_now;
            let manual_reconnect = RECONNECT_REQUESTED.swap(false, Ordering::AcqRel);

            let tap = TAP_PORT.load(Ordering::Acquire);
            let mut tap_dead = false;
            if !tap.is_null() {
                if !CGEventTapIsEnabled(tap) {
                    CGEventTapEnable(tap, true);
                    if !CGEventTapIsEnabled(tap) {
                        tap_dead = true;
                    }
                }
            } else if ax_now || listen_now {
                tap_dead = true;
            }

            let hid_needs_retry = !HID_ACTIVE.load(Ordering::Acquire)
                && listen_now
                && now_ms.saturating_sub(LAST_HID_RETRY_WALL_MS.load(Ordering::Acquire)) > 15_000;

            if manual_reconnect
                || ax_upgraded
                || listen_upgraded
                || woke_from_sleep
                || tap_dead
                || hid_needs_retry
            {
                if hid_needs_retry {
                    LAST_HID_RETRY_WALL_MS.store(now_ms, Ordering::Release);
                }
                CFRunLoopStop(CFRunLoopGetCurrent());
            }
        }
    }

    pub fn run_macos_hook_loop() {
        // Trigger both Accessibility and Input Monitoring prompts once on first launch
        request_permissions_once();

        let mouse_mask: CGEventMask = (1u64 << K_CG_EVENT_LEFT_MOUSE_DOWN)
            | (1u64 << K_CG_EVENT_LEFT_MOUSE_UP)
            | (1u64 << K_CG_EVENT_RIGHT_MOUSE_DOWN)
            | (1u64 << K_CG_EVENT_RIGHT_MOUSE_UP)
            | (1u64 << K_CG_EVENT_MOUSE_MOVED)
            | (1u64 << K_CG_EVENT_LEFT_MOUSE_DRAGGED)
            | (1u64 << K_CG_EVENT_RIGHT_MOUSE_DRAGGED)
            | (1u64 << K_CG_EVENT_SCROLL_WHEEL)
            | (1u64 << K_CG_EVENT_OTHER_MOUSE_DOWN)
            | (1u64 << K_CG_EVENT_OTHER_MOUSE_UP);

        let full_mask: CGEventMask = mouse_mask
            | (1u64 << K_CG_EVENT_KEY_DOWN)
            | (1u64 << K_CG_EVENT_KEY_UP)
            | (1u64 << K_CG_EVENT_FLAGS_CHANGED);

        loop {
            unsafe {
                let ax_ok = AXIsProcessTrusted();
                let listen_ok = CGPreflightListenEventAccess();
                if !ax_ok && !listen_ok {
                    TAP_PORT.store(std::ptr::null_mut(), Ordering::Release);
                    TAP_HAS_KEYBOARD.store(false, Ordering::Release);
                    HID_ACTIVE.store(false, Ordering::Release);
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                }

                SNAPSHOT_AX.store(ax_ok, Ordering::Release);
                SNAPSHOT_LISTEN.store(listen_ok, Ordering::Release);
                LAST_TICK_WALL_MS.store(wall_clock_ms(), Ordering::Release);

                let rl = CFRunLoopGetCurrent();

                // Channel 1: CGEventTap (mouse + keyboard; prefers passive LISTEN_ONLY, falls back
                // to pass-through DEFAULT tap if ListenEvent TCC is lagging behind Accessibility,
                // or mouse-only tap if keyboard is exclusively served via IOHIDManager)
                let mut tap_has_kb = true;
                let mut tap = CGEventTapCreate(
                    K_CG_SESSION_EVENT_TAP,
                    K_CG_HEAD_INSERT_EVENT_TAP,
                    K_CG_EVENT_TAP_OPTION_LISTEN_ONLY,
                    full_mask,
                    tap_callback,
                    std::ptr::null_mut(),
                );
                if tap.is_null() {
                    tap = CGEventTapCreate(
                        K_CG_SESSION_EVENT_TAP,
                        K_CG_HEAD_INSERT_EVENT_TAP,
                        K_CG_EVENT_TAP_OPTION_DEFAULT,
                        full_mask,
                        tap_callback,
                        std::ptr::null_mut(),
                    );
                }
                if tap.is_null() {
                    tap_has_kb = false;
                    tap = CGEventTapCreate(
                        K_CG_SESSION_EVENT_TAP,
                        K_CG_HEAD_INSERT_EVENT_TAP,
                        K_CG_EVENT_TAP_OPTION_LISTEN_ONLY,
                        mouse_mask,
                        tap_callback,
                        std::ptr::null_mut(),
                    );
                }

                let mut tap_src: CFRunLoopSourceRef = std::ptr::null_mut();
                if !tap.is_null() {
                    tap_src = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
                    if !tap_src.is_null() {
                        TAP_PORT.store(tap, Ordering::Release);
                        TAP_HAS_KEYBOARD.store(tap_has_kb, Ordering::Release);
                        CFRunLoopAddSource(rl, tap_src, kCFRunLoopCommonModes);
                        CGEventTapEnable(tap, true);
                    } else {
                        CFMachPortInvalidate(tap);
                        CFRelease(tap as *const c_void);
                        tap = std::ptr::null_mut();
                        TAP_PORT.store(std::ptr::null_mut(), Ordering::Release);
                        TAP_HAS_KEYBOARD.store(false, Ordering::Release);
                    }
                } else {
                    TAP_PORT.store(std::ptr::null_mut(), Ordering::Release);
                    TAP_HAS_KEYBOARD.store(false, Ordering::Release);
                }

                // Channel 2: Non-exclusive read-only IOHIDManager for Keyboard & Keypad.
                // Receives events directly from physical keyboards AND Karabiner-Elements
                // DriverKit VirtualHIDKeyboard, even when CGEventTap is blocked by Secure Input
                // or disrupted by third-party event taps.
                let hid_mgr = if listen_ok {
                    start_hid_keyboard_listener(rl)
                } else {
                    std::ptr::null_mut()
                };
                HID_ACTIVE.store(!hid_mgr.is_null(), Ordering::Release);
                if hid_mgr.is_null() && listen_ok {
                    LAST_HID_RETRY_WALL_MS.store(wall_clock_ms(), Ordering::Release);
                }

                if tap.is_null() && hid_mgr.is_null() {
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                }

                // 2-second watchdog timer on the same RunLoop for zero-restart permission upgrades & sleep recovery
                let timer = CFRunLoopTimerCreate(
                    std::ptr::null(),
                    CFAbsoluteTimeGetCurrent() + 2.0,
                    2.0,
                    0,
                    0,
                    health_timer_callback,
                    std::ptr::null_mut(),
                );
                if !timer.is_null() {
                    CFRunLoopAddTimer(rl, timer, kCFRunLoopCommonModes);
                }

                CFRunLoopRun();

                // Clean teardown before re-initializing
                if !timer.is_null() {
                    CFRunLoopTimerInvalidate(timer);
                    CFRelease(timer as *const c_void);
                }
                if !hid_mgr.is_null() {
                    HID_ACTIVE.store(false, Ordering::Release);
                    IOHIDManagerRegisterInputValueCallback(hid_mgr, None, std::ptr::null_mut());
                    IOHIDManagerUnscheduleFromRunLoop(hid_mgr, rl, kCFRunLoopCommonModes);
                    IOHIDManagerClose(hid_mgr, K_IO_HID_OPTIONS_TYPE_NONE);
                    CFRelease(hid_mgr as *const c_void);
                }
                if !tap.is_null() {
                    TAP_PORT.store(std::ptr::null_mut(), Ordering::Release);
                    TAP_HAS_KEYBOARD.store(false, Ordering::Release);
                    if !tap_src.is_null() {
                        CFRunLoopRemoveSource(rl, tap_src, kCFRunLoopCommonModes);
                        CFRelease(tap_src as *const c_void);
                    }
                    CFMachPortInvalidate(tap);
                    CFRelease(tap as *const c_void);
                }
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hid_usage_maps_letters_digits_modifiers_and_numpad() {
        assert_eq!(hid_usage_to_vk(0x04), Some(0x41)); // A
        assert_eq!(hid_usage_to_vk(0x1D), Some(0x5A)); // Z
        assert_eq!(hid_usage_to_vk(0x1E), Some(0x31)); // 1
        assert_eq!(hid_usage_to_vk(0x27), Some(0x30)); // 0
        assert_eq!(hid_usage_to_vk(0x28), Some(0x0D)); // Enter
        assert_eq!(hid_usage_to_vk(0x29), Some(0x1B)); // Esc
        assert_eq!(hid_usage_to_vk(0x2C), Some(0x20)); // Space
        assert_eq!(hid_usage_to_vk(0x58), Some(0x10D)); // Numpad Enter
        assert_eq!(hid_usage_to_vk(0xE0), Some(0xA2)); // LCtrl
        assert_eq!(hid_usage_to_vk(0xE3), Some(0x5B)); // LCommand
        assert_eq!(hid_usage_to_vk(0xE7), Some(0x5C)); // RCommand
        assert_eq!(hid_usage_to_vk(0x00), None);
        assert_eq!(hid_usage_to_vk(0x01), None);
    }

    #[test]
    fn dedup_suppresses_parallel_hid_and_cgtap_events_without_blocking_fast_repress() {
        let mut dedup = KeyDeduplicator::new();
        let t0 = Instant::now();

        // Key pressed: HID arrives first, CGEventTap arrives 2ms later -> only 1 non-repeat KeyDown
        assert_eq!(dedup.on_key_down(0x41, KeySource::Hid, t0), Some(false));
        assert_eq!(
            dedup.on_key_down(0x41, KeySource::Primary, t0 + Duration::from_millis(2)),
            None
        );

        // Held key repeat from the active source still reports repeat=true
        assert_eq!(
            dedup.on_key_down(0x41, KeySource::Hid, t0 + Duration::from_millis(350)),
            Some(true)
        );

        // Key released: HID Up emits once, trailing CGEventTap Up is suppressed
        assert!(dedup.on_key_up(0x41, KeySource::Hid));
        assert!(!dedup.on_key_up(0x41, KeySource::Primary));

        // Micro-tap race: HID Down+Up in 5ms, delayed CGEventTap Down arrives at +10ms -> suppressed
        let t1 = t0 + Duration::from_millis(500);
        assert_eq!(dedup.on_key_down(0x42, KeySource::Hid, t1), Some(false));
        assert!(dedup.on_key_up(0x42, KeySource::Hid));
        assert_eq!(
            dedup.on_key_down(0x42, KeySource::Primary, t1 + Duration::from_millis(10)),
            None
        );

        // Same-source rapid double-tap (even 20ms apart) is NEVER blocked
        let t2 = t1 + Duration::from_millis(200);
        assert_eq!(dedup.on_key_down(0x43, KeySource::Hid, t2), Some(false));
        assert!(dedup.on_key_up(0x43, KeySource::Hid));
        assert_eq!(
            dedup.on_key_down(0x43, KeySource::Hid, t2 + Duration::from_millis(20)),
            Some(false)
        );
    }
}
