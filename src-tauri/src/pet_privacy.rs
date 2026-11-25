//! 桌面宠物的「输密码自动防窥」：检测焦点是否在密码输入框，命中时让猫闭眼/捂眼
//! 并停止向宠物转发按键（密码按键不产生打字动画）。
//!
//! 检测双通道（与 KuroCat 同源，均为 Windows 官方能力）：
//! * **UI Automation** `CurrentIsPassword`：要求控件类型是 `Edit` 且高度在 8–140px，
//!   过滤第三方 UIA/MSAA 提供程序把文档/面板误报成密码框的情况；
//! * **Win32** `ES_PASSWORD`：只对 Edit / RichEdit 类窗口生效（其他控件里 0x20 位含义不同）。
//!
//! 时序：约 140ms 轮询；离开密码框需连续 2 拍（约 280ms）确认，避免登录表单内
//! 焦点闪动导致遮罩闪烁。开关由 `pet.json` 的 `privacyGuard` 控制；
//! 非 Windows 上不启动线程、不误报。

use std::time::Duration;

use tauri::{AppHandle, Emitter};

use crate::pet_input;

/// 向宠物页面通知隐私模式（true = 进入闭眼/捂眼状态）
fn emit_privacy(app: &AppHandle, active: bool) {
    let _ = app.emit_to("pet", "pet-privacy", active);
}

#[cfg(windows)]
pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let uia = win_impl::UiaDetector::new();
        let mut active = false;
        let mut clear_streak: u8 = 0;
        loop {
            std::thread::sleep(Duration::from_millis(140));
            let guard_enabled = pet_input::is_active() && pet_input::privacy_guard_enabled();
            if !guard_enabled {
                if active {
                    active = false;
                    clear_streak = 0;
                    pet_input::set_password_active(false);
                    emit_privacy(&app, false);
                }
                continue;
            }
            let is_pwd = win_impl::is_focused_password_field(uia.as_ref());
            if is_pwd {
                clear_streak = 0;
                if !active {
                    active = true;
                    pet_input::set_password_active(true);
                    emit_privacy(&app, true);
                }
            } else if active {
                clear_streak = clear_streak.saturating_add(1);
                if clear_streak >= 2 {
                    active = false;
                    clear_streak = 0;
                    pet_input::set_password_active(false);
                    emit_privacy(&app, false);
                }
            }
        }
    });
}

#[cfg(not(windows))]
pub fn spawn(_app: AppHandle) {
    // 非 Windows 没有 UIA / ES_PASSWORD 的等价能力：不启动线程、不误报。
}

#[cfg(windows)]
mod win_impl {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_MULTITHREADED,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, UIA_EditControlTypeId,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GetGUIThreadInfo, GetWindowLongPtrW,
        GetWindowThreadProcessId, GUITHREADINFO, GWL_STYLE,
    };

    const ES_MULTILINE: isize = 0x0004;
    const ES_PASSWORD: isize = 0x0020;
    const MAX_PASSWORD_BOX_HEIGHT_PX: i32 = 140;

    pub(super) fn is_plausible_password_rect(top: i32, bottom: i32) -> bool {
        let h = bottom.saturating_sub(top);
        // 真实可见的单行密码框高度在 8~140px 之间；0px 是未实现的 UIA 代理，
        // 300~1000px 是文档编辑器（WPS Word / Notepad 画布），都要排除。
        (8..=MAX_PASSWORD_BOX_HEIGHT_PX).contains(&h)
    }

    pub(super) fn is_classic_edit_class(class_name: &str) -> bool {
        let lower = class_name.to_ascii_lowercase();
        lower == "edit"
            || lower.starts_with("richedit")
            || lower == "tedit"
            || lower == "tmaskedit"
            || lower == "wedit"
            || (lower.starts_with("thunder") && lower.contains("textbox"))
    }

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
                let is_pwd = match el.CurrentIsPassword() {
                    Ok(b) => b.as_bool(),
                    Err(_) => false,
                };
                if !is_pwd {
                    return false;
                }
                let Ok(ctrl_type) = el.CurrentControlType() else {
                    return false;
                };
                if ctrl_type != UIA_EditControlTypeId {
                    return false;
                }
                let Ok(rc) = el.CurrentBoundingRectangle() else {
                    return false;
                };
                if !is_plausible_password_rect(rc.top, rc.bottom) {
                    return false;
                }
                true
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
            if fg == HWND::default() {
                return false;
            }
            let tid = GetWindowThreadProcessId(fg, None);
            if tid == 0 {
                return false;
            }
            let mut info = GUITHREADINFO::default();
            info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
            if GetGUIThreadInfo(tid, &mut info).is_err() || info.hwndFocus == HWND::default() {
                return false;
            }
            let mut cls_buf = [0u16; 128];
            let cls_len = GetClassNameW(info.hwndFocus, &mut cls_buf);
            if cls_len <= 0 {
                return false;
            }
            let cls_name = String::from_utf16_lossy(&cls_buf[..cls_len as usize]);
            if !is_classic_edit_class(&cls_name) {
                return false;
            }
            let style = GetWindowLongPtrW(info.hwndFocus, GWL_STYLE);
            (style & ES_PASSWORD) != 0 && (style & ES_MULTILINE) == 0
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

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    #[test]
    fn password_rect_filter_rejects_editors_and_empty_proxies() {
        use super::win_impl::is_plausible_password_rect;
        assert!(is_plausible_password_rect(100, 124)); // 24px 单行密码框
        assert!(!is_plausible_password_rect(100, 100)); // 0px 未实现代理
        assert!(!is_plausible_password_rect(100, 500)); // 文档编辑器
    }

    #[cfg(windows)]
    #[test]
    fn classic_edit_class_filter_is_narrow() {
        use super::win_impl::is_classic_edit_class;
        assert!(is_classic_edit_class("Edit"));
        assert!(is_classic_edit_class("RichEdit20W"));
        assert!(!is_classic_edit_class("SysTreeView32"));
        assert!(!is_classic_edit_class("Chrome_WidgetWin_1"));
    }
}
