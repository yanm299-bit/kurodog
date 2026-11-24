use crate::config::AppSettings;
use crate::{
    apply_keyboard_layout_and_save, apply_keyboard_orientation_and_save,
    apply_scale_and_save, reset_window_position, AppState,
};
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

fn default_tray_tooltip() -> &'static str {
    #[cfg(windows)]
    {
        if crate::win_is_elevated() {
            return "小黑狗（管理员 · 游戏模式就绪）";
        }
    }
    "小黑狗"
}

pub fn setup_tray(app: &AppHandle, initial: &AppSettings) -> tauri::Result<()> {
    let toggle_show = MenuItem::with_id(app, "toggle_show", "显示 / 隐藏小黑狗", true, None::<&str>)?;
    let pause_guard = CheckMenuItem::with_id(
        app,
        "pause_guard",
        "暂停监听并隐藏（游戏反作弊护航）",
        true,
        false,
        None::<&str>,
    )?;
    #[cfg(windows)]
    let restart_as_admin = {
        let elevated = crate::win_is_elevated();
        let label = if elevated {
            "✓ 已管理员运行（游戏键鼠就绪）"
        } else {
            "以管理员身份重启（捕获游戏键鼠）"
        };
        MenuItem::with_id(app, "restart_as_admin", label, !elevated, None::<&str>)?
    };
    let click_through = CheckMenuItem::with_id(
        app,
        "click_through",
        "锁定鼠标穿透（按住 Alt 可临时操作）",
        true,
        initial.click_through_locked,
        None::<&str>,
    )?;
    let key_bubbles = CheckMenuItem::with_id(
        app,
        "key_bubbles",
        "显示悬浮按键气泡",
        true,
        initial.show_key_bubbles,
        None::<&str>,
    )?;
    let privacy_guard = CheckMenuItem::with_id(
        app,
        "privacy_guard",
        "输入密码自动闭眼防偷窥",
        true,
        initial.privacy_password_guard,
        None::<&str>,
    )?;
    let always_on_top = CheckMenuItem::with_id(
        app,
        "always_on_top",
        "始终置顶显示",
        true,
        initial.always_on_top,
        None::<&str>,
    )?;
    let autostart = CheckMenuItem::with_id(
        app,
        "autostart",
        "开机自动启动",
        true,
        initial.autostart,
        None::<&str>,
    )?;
    let mcp_toggle = CheckMenuItem::with_id(
        app,
        "mcp_toggle",
        "开启 MCP 扩展服务",
        true,
        initial.mcp_enabled,
        None::<&str>,
    )?;

    let scale_25 = MenuItem::with_id(app, "scale_25", "25%（迷你）", true, None::<&str>)?;
    let scale_50 = MenuItem::with_id(app, "scale_50", "50%（小巧）", true, None::<&str>)?;
    let scale_75 = MenuItem::with_id(app, "scale_75", "75%（适中）", true, None::<&str>)?;
    let scale_100 = MenuItem::with_id(app, "scale_100", "100%（默认）", true, None::<&str>)?;
    let scale_125 = MenuItem::with_id(app, "scale_125", "125%（偏大）", true, None::<&str>)?;
    let scale_150 = MenuItem::with_id(app, "scale_150", "150%（大号）", true, None::<&str>)?;
    let scale_menu = Submenu::with_items(
        app,
        "缩放大小",
        true,
        &[&scale_25, &scale_50, &scale_75, &scale_100, &scale_125, &scale_150],
    )?;

    let kb_65 = MenuItem::with_id(app, "kb_65", "65% 紧凑键盘", true, None::<&str>)?;
    let kb_108 = MenuItem::with_id(app, "kb_108", "108 键全尺寸（含小键盘）", true, None::<&str>)?;
    let kb_sep = PredefinedMenuItem::separator(app)?;
    let kb_cat = MenuItem::with_id(app, "kb_cat", "面向小黑狗（180° 真实视角）", true, None::<&str>)?;
    let kb_viewer = MenuItem::with_id(app, "kb_viewer", "面向观众（正向字母易读）", true, None::<&str>)?;
    let kb_menu = Submenu::with_items(
        app,
        "键盘配列与方向",
        true,
        &[&kb_65, &kb_108, &kb_sep, &kb_cat, &kb_viewer],
    )?;

    let reset_pos = MenuItem::with_id(app, "reset_pos", "重置位置到右下角", true, None::<&str>)?;
    #[cfg(windows)]
    let desktop_shortcut =
        MenuItem::with_id(app, "desktop_shortcut", "创建桌面快捷方式 (小黑狗)", true, None::<&str>)?;
    #[cfg(target_os = "macos")]
    let mac_input_menu = {
        let mac_reconnect = MenuItem::with_id(
            app,
            "mac_reconnect_input",
            "重新连接键盘监听（兼容 Karabiner）",
            true,
            None::<&str>,
        )?;
        let mac_diag = MenuItem::with_id(
            app,
            "mac_diagnose_input",
            "查看键盘监听与权限诊断",
            true,
            None::<&str>,
        )?;
        let mac_sep = PredefinedMenuItem::separator(app)?;
        let mac_open_listen = MenuItem::with_id(
            app,
            "mac_open_input_monitoring",
            "打开「输入监控」隐私设置",
            true,
            None::<&str>,
        )?;
        let mac_open_ax = MenuItem::with_id(
            app,
            "mac_open_accessibility",
            "打开「辅助功能」隐私设置",
            true,
            None::<&str>,
        )?;
        Submenu::with_items(
            app,
            "键盘监听与权限",
            true,
            &[
                &mac_reconnect,
                &mac_diag,
                &mac_sep,
                &mac_open_listen,
                &mac_open_ax,
            ],
        )?
    };
    let check_update = MenuItem::with_id(
        app,
        "check_update",
        format!("检查更新 (v{})", crate::updater::CURRENT_VERSION),
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "退出 小黑狗", true, None::<&str>)?;

    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;

    #[cfg(windows)]
    let menu = Menu::with_items(
        app,
        &[
            &toggle_show,
            &pause_guard,
            &restart_as_admin,
            &sep1,
            &click_through,
            &privacy_guard,
            &key_bubbles,
            &always_on_top,
            &kb_menu,
            &scale_menu,
            &desktop_shortcut,
            &reset_pos,
            &autostart,
            &mcp_toggle,
            &check_update,
            &sep2,
            &quit,
        ],
    )?;

    #[cfg(target_os = "macos")]
    let menu = Menu::with_items(
        app,
        &[
            &toggle_show,
            &pause_guard,
            &sep1,
            &click_through,
            &privacy_guard,
            &key_bubbles,
            &always_on_top,
            &kb_menu,
            &scale_menu,
            &mac_input_menu,
            &reset_pos,
            &autostart,
            &mcp_toggle,
            &check_update,
            &sep2,
            &quit,
        ],
    )?;

    #[cfg(all(not(windows), not(target_os = "macos")))]
    let menu = Menu::with_items(
        app,
        &[
            &toggle_show,
            &pause_guard,
            &sep1,
            &click_through,
            &privacy_guard,
            &key_bubbles,
            &always_on_top,
            &kb_menu,
            &scale_menu,
            &reset_pos,
            &autostart,
            &mcp_toggle,
            &check_update,
            &sep2,
            &quit,
        ],
    )?;

    let icon = Image::from_bytes(include_bytes!("../icons/tray-icon.png"))?;
    let pause_guard_for_click = pause_guard.clone();

    TrayIconBuilder::with_id("kuro-tray")
        .icon(icon)
        .tooltip(default_tray_tooltip())
        .menu(&menu)
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_tray_icon_event(move |tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                let state = app.state::<AppState>();
                let on_top = state.settings.lock().unwrap().always_on_top;
                if state.paused_guard.load(std::sync::atomic::Ordering::SeqCst) {
                    state
                        .paused_guard
                        .store(false, std::sync::atomic::Ordering::SeqCst);
                    crate::input_hook::set_hooks_paused(false);
                    let _ = pause_guard_for_click.set_checked(false);
                    let _ = tray.set_tooltip(Some(default_tray_tooltip()));
                    if let Some(win) = app.get_webview_window("main") {
                        let _ = win.show();
                        #[cfg(windows)]
                        if on_top {
                            crate::win_gentle_reassert_topmost(&win);
                        }
                    }
                } else if let Some(win) = app.get_webview_window("main") {
                    if win.is_visible().unwrap_or(true) {
                        let _ = win.hide();
                    } else {
                        let _ = win.show();
                        #[cfg(windows)]
                        if on_top {
                            crate::win_gentle_reassert_topmost(&win);
                        }
                    }
                }
            }
        })
        .on_menu_event(move |app, event| {
            let state = app.state::<AppState>();
            match event.id().as_ref() {
                "toggle_show" => {
                    let on_top = state.settings.lock().unwrap().always_on_top;
                    if state.paused_guard.load(std::sync::atomic::Ordering::SeqCst) {
                        state
                            .paused_guard
                            .store(false, std::sync::atomic::Ordering::SeqCst);
                        crate::input_hook::set_hooks_paused(false);
                        let _ = pause_guard.set_checked(false);
                        if let Some(tray) = app.tray_by_id("kuro-tray") {
                            let _ = tray.set_tooltip(Some(default_tray_tooltip()));
                        }
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.show();
                            #[cfg(windows)]
                            if on_top {
                                crate::win_gentle_reassert_topmost(&win);
                            }
                        }
                    } else if let Some(win) = app.get_webview_window("main") {
                        if win.is_visible().unwrap_or(true) {
                            let _ = win.hide();
                        } else {
                            let _ = win.show();
                            #[cfg(windows)]
                            if on_top {
                                crate::win_gentle_reassert_topmost(&win);
                            }
                        }
                    }
                }
                "pause_guard" => {
                    let next = !state
                        .paused_guard
                        .load(std::sync::atomic::Ordering::SeqCst);
                    state
                        .paused_guard
                        .store(next, std::sync::atomic::Ordering::SeqCst);
                    crate::input_hook::set_hooks_paused(next);
                    let _ = pause_guard.set_checked(next);
                    if let Some(tray) = app.tray_by_id("kuro-tray") {
                        let tip = if next {
                            "小黑狗（已暂停监听 · 反作弊护航中）"
                        } else {
                            default_tray_tooltip()
                        };
                        let _ = tray.set_tooltip(Some(tip));
                    }
                    if let Some(win) = app.get_webview_window("main") {
                        if next {
                            let _ = win.hide();
                        } else {
                            let _ = win.show();
                            #[cfg(windows)]
                            if state.settings.lock().unwrap().always_on_top {
                                crate::win_gentle_reassert_topmost(&win);
                            }
                        }
                    }
                }
                #[cfg(windows)]
                "restart_as_admin" => {
                    if !crate::win_is_elevated() {
                        crate::win_restart_as_admin(app.clone());
                    }
                }
                "click_through" => {
                    let mut s = state.settings.lock().unwrap();
                    s.click_through_locked = !s.click_through_locked;
                    let locked = s.click_through_locked;
                    crate::config::save_settings(&s);
                    let _ = click_through.set_checked(locked);
                    let _ = app.emit("kuro-settings", s.clone());
                }
                "key_bubbles" => {
                    let mut s = state.settings.lock().unwrap();
                    s.show_key_bubbles = !s.show_key_bubbles;
                    let enabled = s.show_key_bubbles;
                    crate::config::save_settings(&s);
                    let _ = key_bubbles.set_checked(enabled);
                    let _ = app.emit("kuro-settings", s.clone());
                }
                "privacy_guard" => {
                    let mut s = state.settings.lock().unwrap();
                    s.privacy_password_guard = !s.privacy_password_guard;
                    let enabled = s.privacy_password_guard;
                    crate::config::save_settings(&s);
                    let _ = privacy_guard.set_checked(enabled);
                    let _ = app.emit("kuro-settings", s.clone());
                }
                "always_on_top" => {
                    let mut s = state.settings.lock().unwrap();
                    s.always_on_top = !s.always_on_top;
                    let on_top = s.always_on_top;
                    crate::config::save_settings(&s);
                    let _ = always_on_top.set_checked(on_top);
                    if let Some(win) = app.get_webview_window("main") {
                        let _ = win.set_always_on_top(on_top);
                        #[cfg(target_os = "macos")]
                        crate::configure_macos_fullscreen_overlay(&win, on_top);
                    }
                    let _ = app.emit("kuro-settings", s.clone());
                }
                "autostart" => {
                    let mut s = state.settings.lock().unwrap();
                    let next = !s.autostart;
                    if crate::config::set_windows_autostart(next).is_ok() {
                        s.autostart = next;
                        crate::config::save_settings(&s);
                        let _ = autostart.set_checked(next);
                        let _ = app.emit("kuro-settings", s.clone());
                    }
                }
                "mcp_toggle" => {
                    let mut s = state.settings.lock().unwrap();
                    s.mcp_enabled = !s.mcp_enabled;
                    let enabled = s.mcp_enabled;
                    state
                        .mcp_enabled
                        .store(enabled, std::sync::atomic::Ordering::Relaxed);
                    crate::config::save_settings(&s);
                    let _ = mcp_toggle.set_checked(enabled);
                    let _ = app.emit("kuro-settings", s.clone());
                }
                "scale_25" => apply_scale_and_save(app, 0.25),
                "scale_50" => apply_scale_and_save(app, 0.5),
                "scale_75" => apply_scale_and_save(app, 0.75),
                "scale_100" => apply_scale_and_save(app, 1.0),
                "scale_125" => apply_scale_and_save(app, 1.25),
                "scale_150" => apply_scale_and_save(app, 1.5),
                "kb_65" => apply_keyboard_layout_and_save(app, "65"),
                "kb_108" => apply_keyboard_layout_and_save(app, "108"),
                "kb_cat" => apply_keyboard_orientation_and_save(app, "cat"),
                "kb_viewer" => apply_keyboard_orientation_and_save(app, "viewer"),
                "desktop_shortcut" => {
                    let _ = crate::config::create_desktop_shortcut();
                }
                #[cfg(target_os = "macos")]
                "mac_reconnect_input" => {
                    crate::input_hook::macos_hook::trigger_macos_reconnect();
                }
                #[cfg(target_os = "macos")]
                "mac_diagnose_input" => {
                    crate::input_hook::macos_hook::show_macos_diagnostics_dialog();
                }
                #[cfg(target_os = "macos")]
                "mac_open_input_monitoring" => {
                    crate::input_hook::macos_hook::open_macos_privacy_pane("input_monitoring");
                }
                #[cfg(target_os = "macos")]
                "mac_open_accessibility" => {
                    crate::input_hook::macos_hook::open_macos_privacy_pane("accessibility");
                }
                "check_update" => {
                    let app_clone = app.clone();
                    if let Some(win) = app.get_webview_window("main") {
                        let _ = win.show();
                    }
                    let _ = app_clone.emit(
                        "kuro-update-status",
                        crate::updater::UpdateProgressEvent {
                            stage: "checking".to_string(),
                            message: "正在检查新版本...".to_string(),
                            info: None,
                        },
                    );
                    std::thread::spawn(move || match crate::updater::check_latest_update() {
                        Ok(info) => {
                            let (stage, msg) = if info.has_update {
                                ("available", format!("发现新版本 v{}", info.latest_version))
                            } else {
                                ("up_to_date", format!("已是最新版本 v{}", info.current_version))
                            };
                            let _ = app_clone.emit(
                                "kuro-update-status",
                                crate::updater::UpdateProgressEvent {
                                    stage: stage.to_string(),
                                    message: msg,
                                    info: Some(info),
                                },
                            );
                        }
                        Err(err) => {
                            let _ = app_clone.emit(
                                "kuro-update-status",
                                crate::updater::UpdateProgressEvent {
                                    stage: "error".to_string(),
                                    message: err,
                                    info: None,
                                },
                            );
                        }
                    });
                }
                "reset_pos" => reset_window_position(app),
                "quit" => {
                    app.exit(0);
                }
                _ => {}
            }
        })
        .build(app)?;

    Ok(())
}
