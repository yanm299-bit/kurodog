use crate::config::AppSettings;
use crate::input_hook;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// Starts a lightweight background watcher that detects when the OS focus is inside a
/// password field (Windows UI Automation `IsPassword` + Win32 `ES_PASSWORD`; macOS native
/// `IsSecureEventInputEnabled` + `AXSecureTextField`), immediately suppressing keystroke
/// forwarding in `input_hook` and notifying the frontend to close the cat's eyes / cover BigFish's eyes.
pub fn start_password_privacy_watcher(
    app: AppHandle,
    settings: Arc<Mutex<AppSettings>>,
    paused_guard: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        #[cfg(windows)]
        let uia = win_impl::UiaDetector::new();

        let mut active = false;
        let mut clear_streak: u8 = 0;

        loop {
            std::thread::sleep(Duration::from_millis(140));

            let guard_enabled = !paused_guard.load(Ordering::Relaxed)
                && settings
                    .lock()
                    .map(|s| s.privacy_password_guard)
                    .unwrap_or(true);

            if !guard_enabled {
                if active {
                    active = false;
                    clear_streak = 0;
                    input_hook::set_password_privacy_active(false);
                    let _ = app.emit("kuro-privacy", false);
                }
                continue;
            }

            #[cfg(windows)]
            let is_pwd = win_impl::is_focused_password_field(uia.as_ref());
            #[cfg(target_os = "macos")]
            let is_pwd = mac_impl::is_focused_password_field();
            #[cfg(all(not(windows), not(target_os = "macos")))]
            let is_pwd = false;

            if is_pwd {
                clear_streak = 0;
                if !active {
                    active = true;
                    input_hook::set_password_privacy_active(true);
                    let _ = app.emit("kuro-privacy", true);
                }
            } else if active {
                // Brief 2-tick (~280ms) debounce before exiting so focus flickers inside a login form don't flash open
                clear_streak = clear_streak.saturating_add(1);
                if clear_streak >= 2 {
                    active = false;
                    clear_streak = 0;
                    input_hook::set_password_privacy_active(false);
                    let _ = app.emit("kuro-privacy", false);
                }
            }
        }
    });
}

#[cfg(windows)]
mod win_impl {
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_MULTITHREADED,
    };
    use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetGUIThreadInfo, GetWindowLongPtrW, GetWindowThreadProcessId,
        GUITHREADINFO, GWL_STYLE,
    };

    const ES_PASSWORD: isize = 0x0020;

    pub struct UiaDetector {
        uia: IUIAutomation,
        com_inited: bool,
    }

    impl UiaDetector {
        pub fn new() -> Option<Self> {
            unsafe {
                let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
                let com_inited = hr.is_ok();
                let uia: IUIAutomation =
                    CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
                Some(Self { uia, com_inited })
            }
        }

        pub fn focused_is_password(&self) -> bool {
            unsafe {
                let Ok(el) = self.uia.GetFocusedElement() else {
                    return false;
                };
                match el.CurrentIsPassword() {
                    Ok(b) => b.as_bool(),
                    Err(_) => false,
                }
            }
        }
    }

    impl Drop for UiaDetector {
        fn drop(&mut self) {
            if self.com_inited {
                unsafe {
                    CoUninitialize();
                }
            }
        }
    }

    fn classic_win32_is_password() -> bool {
        unsafe {
            let fg = GetForegroundWindow();
            if fg.is_null() {
                return false;
            }
            let tid = GetWindowThreadProcessId(fg, std::ptr::null_mut());
            if tid == 0 {
                return false;
            }
            let mut info: GUITHREADINFO = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
            if GetGUIThreadInfo(tid, &mut info) == 0 || info.hwndFocus.is_null() {
                return false;
            }
            let style = GetWindowLongPtrW(info.hwndFocus, GWL_STYLE);
            (style & ES_PASSWORD) != 0
        }
    }

    pub fn is_focused_password_field(uia: Option<&UiaDetector>) -> bool {
        if classic_win32_is_password() {
            return true;
        }
        if let Some(det) = uia {
            return det.focused_is_password();
        }
        false
    }
}

#[cfg(target_os = "macos")]
mod mac_impl {
    use std::ffi::c_void;

    const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

    #[link(name = "Carbon", kind = "framework")]
    extern "C" {
        fn IsSecureEventInputEnabled() -> u8;
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
        fn AXUIElementCreateSystemWide() -> *mut c_void;
        fn AXUIElementCopyAttributeValue(
            element: *mut c_void,
            attribute: *const c_void,
            value: *mut *mut c_void,
        ) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: *const c_void);
        fn CFStringCreateWithBytes(
            alloc: *const c_void,
            bytes: *const u8,
            num_bytes: isize,
            encoding: u32,
            is_external_rep: u8,
        ) -> *mut c_void;
        fn CFGetTypeID(cf: *const c_void) -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFEqual(cf1: *const c_void, cf2: *const c_void) -> u8;
    }

    unsafe fn make_cf_string(s: &str) -> *mut c_void {
        CFStringCreateWithBytes(
            std::ptr::null(),
            s.as_ptr(),
            s.len() as isize,
            K_CF_STRING_ENCODING_UTF8,
            0,
        )
    }

    unsafe fn ax_focused_is_secure_text_field() -> bool {
        if !AXIsProcessTrusted() {
            return false;
        }
        let sys_wide = AXUIElementCreateSystemWide();
        if sys_wide.is_null() {
            return false;
        }
        let attr_focused = make_cf_string("AXFocusedUIElement");
        let attr_subrole = make_cf_string("AXSubrole");
        let secure_subrole = make_cf_string("AXSecureTextField");

        let mut is_secure = false;
        if !attr_focused.is_null() && !attr_subrole.is_null() && !secure_subrole.is_null() {
            let mut focused: *mut c_void = std::ptr::null_mut();
            let err1 = AXUIElementCopyAttributeValue(sys_wide, attr_focused, &mut focused);
            if err1 == 0 && !focused.is_null() {
                let mut subrole: *mut c_void = std::ptr::null_mut();
                let err2 = AXUIElementCopyAttributeValue(focused, attr_subrole, &mut subrole);
                if err2 == 0 && !subrole.is_null() {
                    if CFGetTypeID(subrole) == CFStringGetTypeID()
                        && CFEqual(subrole, secure_subrole) != 0
                    {
                        is_secure = true;
                    }
                    CFRelease(subrole);
                }
                CFRelease(focused);
            }
        }

        if !secure_subrole.is_null() {
            CFRelease(secure_subrole);
        }
        if !attr_subrole.is_null() {
            CFRelease(attr_subrole);
        }
        if !attr_focused.is_null() {
            CFRelease(attr_focused);
        }
        CFRelease(sys_wide);
        is_secure
    }

    pub fn is_focused_password_field() -> bool {
        unsafe {
            if IsSecureEventInputEnabled() != 0 {
                return true;
            }
            ax_focused_is_secure_text_field()
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    #[test]
    fn windows_password_detector_initializes_and_probes_safely() {
        let det = super::win_impl::UiaDetector::new();
        assert!(det.is_some(), "CUIAutomation COM object should initialize cleanly on Windows");
        let _ = super::win_impl::is_focused_password_field(det.as_ref());
    }
}
