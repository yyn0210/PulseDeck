//! 桌面宠物的全局键鼠监听、光标看护与鼠标穿透。
//!
//! 设计要点（与 KuroCat 的 `input_hook.rs` 同源，按 PulseDeck 的需要裁剪）：
//! * **只读虚拟键码与按键状态**：钩子始终 `CallNextHookEx`，不注入、不拦截、不读字符、
//!   不记录日志。密码框命中期间由 [`set_password_active`] 抑制转发（见 `pet_privacy.rs`）。
//! * **钩子按需装卸**：只有「宠物开启且为打字猫形象」时安装（[`sync`]），
//!   关闭宠物 / 切回经典贴图时卸载；线程本身常驻但空闲时只睡不做事。
//! * **低层钩子必须跑在有消息循环的线程上**：安装/卸载都由该线程通过
//!   `PostThreadMessageW` 的自定义消息驱动，不能在别的线程直接 `SetWindowsHookExW`。
//! * **按住去重与漏抬起对账**：低层钩子会重复上报按住不放的键；`GetAsyncKeyState`
//!   每 750ms 对账一次，避免切窗/丢事件导致键位永久按住。
//! * **跨平台**：非 Windows 上本模块是诚实空实现（不拦截、不转发），
//!   打字猫仍正常渲染与刷新指标。
//!
//! 事件格式（前端 `src/pet.js` 消费）：
//! `pet-input` = `{kind:"keyDown",vk,scan,repeat}` / `{kind:"keyUp",vk,scan}` /
//! `{kind:"mouseDown",button}` / `{kind:"mouseUp",button}` /
//! `{kind:"mouseWheel",delta}` / `{kind:"mouseMove",x,y,nx,ny}` / `{kind:"privacyTyping"}`
//! `pet-cursor` = `{relX,relY,inside,altOverride}`（相对宠物窗口，0~1）

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

/// 宠物窗口 label（与 `pet::WINDOW_LABEL` 保持一致；这里复制一份避免循环依赖）
const PET_LABEL: &str = "pet";

/// 是否处于「宠物开启 + 打字猫」状态；关闭时所有转发停止
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// 是否锁定鼠标穿透（Alt 临时解锁）
static CLICK_THROUGH_LOCKED: AtomicBool = AtomicBool::new(false);
/// 是否开启密码框防窥（由 `pet_privacy` 读取）
static PRIVACY_GUARD: AtomicBool = AtomicBool::new(true);
/// 密码框命中：键盘事件不再转发（由 `pet_privacy` 写入）
static PASSWORD_ACTIVE: AtomicBool = AtomicBool::new(false);

/// 钩子线程是否已经装上钩子（只有钩子线程读写）
static HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);
/// 当前实际下发给窗口的「忽略鼠标」状态（光标线程与 deactivate 共用，避免状态漂移）
static CLICK_THROUGH_APPLIED: AtomicBool = AtomicBool::new(false);
/// 钩子线程 id（用于 `PostThreadMessageW`）
static HOOK_TID: AtomicU32 = AtomicU32::new(0);
/// 运行期线程是否已经创建（只创建一次，随后靠 ACTIVE 开关）
static STARTED: AtomicBool = AtomicBool::new(false);

static HOOK_TX: Mutex<Option<Sender<RawHookMsg>>> = Mutex::new(None);

/// 钩子线程控制消息（`PostThreadMessageW` 的 msg）
#[cfg(windows)]
const WM_PET_INSTALL: u32 = 0x8000 + 1;
#[cfg(windows)]
const WM_PET_UNINSTALL: u32 = 0x8000 + 2;

#[allow(dead_code)]
#[derive(Debug, Clone)]
enum RawHookMsg {
    KeyDown { vk: u32, scan: u32 },
    KeyUp { vk: u32, scan: u32 },
    MouseDown(&'static str),
    MouseUp(&'static str),
    MouseWheel(i16),
    MouseMove { x: i32, y: i32 },
}

/// 发给前端的输入事件（字段名与 KuroCat 的 `kuro-input` 一致）
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PetInputEvent {
    KeyDown { vk: u32, scan: u32, repeat: bool },
    KeyUp { vk: u32, scan: u32 },
    MouseDown { button: String },
    MouseUp { button: String },
    MouseWheel { delta: i16 },
    MouseMove { x: i32, y: i32, nx: f64, ny: f64 },
    PrivacyTyping,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PetCursorInfo {
    pub rel_x: f64,
    pub rel_y: f64,
    pub inside: bool,
    pub alt_override: bool,
}

/// 当前平台是否支持全局键鼠监听（设置页据此给出说明）
pub fn is_supported() -> bool {
    cfg!(windows)
}

/// 是否需要全局钩子：只有「宠物开启 + 打字猫」需要；其余情况必须卸载（不拦截任何输入）。
pub fn wants_hooks(enabled: bool, mode: &str) -> bool {
    enabled && mode == "cat"
}

/// 鼠标穿透判定：锁定且未按 Alt 时才穿透；Alt 是临时解锁键。
pub fn should_ignore_cursor(locked: bool, alt: bool) -> bool {
    locked && !alt
}

pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

pub fn privacy_guard_enabled() -> bool {
    PRIVACY_GUARD.load(Ordering::Relaxed)
}

/// 密码框命中状态由 `pet_privacy` 写入；钩子转发层只读。
pub fn set_password_active(active: bool) {
    PASSWORD_ACTIVE.store(active, Ordering::Relaxed);
}

/// 按宠物配置同步监听状态。
///
/// 只有 `enabled && mode == "cat"` 才需要钩子；其余情况立即卸载（不拦截任何输入）。
pub fn sync(app: &AppHandle, enabled: bool, mode: &str, click_through: bool, privacy_guard: bool) {
    CLICK_THROUGH_LOCKED.store(click_through, Ordering::Relaxed);
    PRIVACY_GUARD.store(privacy_guard, Ordering::Relaxed);
    let want = wants_hooks(enabled, mode);
    if want {
        ensure_started(app);
        ACTIVE.store(true, Ordering::Relaxed);
        request_install(true);
    } else {
        ACTIVE.store(false, Ordering::Relaxed);
        PASSWORD_ACTIVE.store(false, Ordering::Relaxed);
        request_install(false);
        // 切回经典 / 关闭宠物后，窗口必须恢复可交互（否则鼠标穿透会让人以为窗口卡死）
        if let Some(win) = app.get_webview_window(PET_LABEL) {
            let _ = win.set_ignore_cursor_events(false);
        }
        // 同步「已下发状态」，否则 cat→classic→cat 往返后光标线程以为穿透已生效而不再下发
        CLICK_THROUGH_APPLIED.store(false, Ordering::Relaxed);
    }
}

/// 立即停用并卸载钩子（宠物窗口被销毁时调用；配置下次同步会重新启用）
pub fn deactivate(app: &AppHandle) {
    ACTIVE.store(false, Ordering::Relaxed);
    PASSWORD_ACTIVE.store(false, Ordering::Relaxed);
    request_install(false);
    if let Some(win) = app.get_webview_window(PET_LABEL) {
        let _ = win.set_ignore_cursor_events(false);
    }
    CLICK_THROUGH_APPLIED.store(false, Ordering::Relaxed);
}

fn request_install(install: bool) {
    #[cfg(windows)]
    {
        let tid = HOOK_TID.load(Ordering::Relaxed);
        if tid == 0 {
            return;
        }
        use windows::Win32::UI::WindowsAndMessaging::PostThreadMessageW;
        let msg = if install {
            WM_PET_INSTALL
        } else {
            WM_PET_UNINSTALL
        };
        unsafe {
            let _ = PostThreadMessageW(tid, msg, Default::default(), Default::default());
        }
    }
    #[cfg(not(windows))]
    {
        let _ = install;
    }
}

fn ensure_started(app: &AppHandle) {
    // 非 Windows 没有全局钩子可装：连线程都不启动（打字猫照常渲染，只是没有键鼠反应）
    if !cfg!(windows) {
        return;
    }
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let (tx, rx) = mpsc::channel::<RawHookMsg>();
    if let Ok(mut guard) = HOOK_TX.lock() {
        *guard = Some(tx);
    }

    #[cfg(windows)]
    spawn_hook_thread();

    spawn_dispatcher(app.clone(), rx);
    spawn_cursor_watcher(app.clone());
    crate::pet_privacy::spawn(app.clone());
}

/* ------------------------------ 事件派发 ------------------------------ */

fn emit_input(app: &AppHandle, ev: PetInputEvent) {
    if !ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    let _ = app.emit_to(PET_LABEL, "pet-input", &ev);
}

fn spawn_dispatcher(app: AppHandle, rx: Receiver<RawHookMsg>) {
    std::thread::spawn(move || {
        let mut key_down: HashSet<u32> = HashSet::new();
        let mut mouse_down = [false; 3]; // left / right / middle
        let mut last_move = Instant::now() - Duration::from_secs(1);
        let mut last_move_xy: Option<(i32, i32)> = None;
        let mut last_reconcile = Instant::now();
        let mut last_privacy_typing = Instant::now() - Duration::from_secs(1);

        loop {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(msg) => {
                    if !ACTIVE.load(Ordering::Relaxed) {
                        continue;
                    }
                    dispatch_one(
                        &app,
                        msg,
                        &mut key_down,
                        &mut mouse_down,
                        &mut last_move,
                        &mut last_move_xy,
                        &mut last_privacy_typing,
                    );
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }

            if ACTIVE.load(Ordering::Relaxed) && last_reconcile.elapsed() >= Duration::from_millis(750)
            {
                last_reconcile = Instant::now();
                #[cfg(windows)]
                reconcile_windows(&app, &mut key_down, &mut mouse_down);
            }
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn dispatch_one(
    app: &AppHandle,
    msg: RawHookMsg,
    key_down: &mut HashSet<u32>,
    mouse_down: &mut [bool; 3],
    last_move: &mut Instant,
    last_move_xy: &mut Option<(i32, i32)>,
    last_privacy_typing: &mut Instant,
) {
    let password = PASSWORD_ACTIVE.load(Ordering::Relaxed);
    match msg {
        RawHookMsg::KeyDown { vk, scan } => {
            if password {
                if last_privacy_typing.elapsed() >= Duration::from_millis(120) {
                    *last_privacy_typing = Instant::now();
                    emit_input(app, PetInputEvent::PrivacyTyping);
                }
                return;
            }
            // 用完整 VK 作为键：`vk & 0xff` 会让 VK_RETURN(0x0D) 与 VK_NUMPAD_ENTER(0x10D) 撞一起
            let repeat = !key_down.insert(vk);
            emit_input(app, PetInputEvent::KeyDown { vk, scan, repeat });
        }
        RawHookMsg::KeyUp { vk, scan } => {
            if password {
                key_down.remove(&vk);
                return;
            }
            if !key_down.remove(&vk) {
                return;
            }
            emit_input(app, PetInputEvent::KeyUp { vk, scan });
        }
        RawHookMsg::MouseDown(button) => {
            let i = button_index(button);
            if let Some(i) = i {
                if mouse_down[i] {
                    return;
                }
                mouse_down[i] = true;
            }
            emit_input(
                app,
                PetInputEvent::MouseDown {
                    button: button.to_string(),
                },
            );
        }
        RawHookMsg::MouseUp(button) => {
            let i = button_index(button);
            if let Some(i) = i {
                if !mouse_down[i] {
                    return;
                }
                mouse_down[i] = false;
            }
            emit_input(
                app,
                PetInputEvent::MouseUp {
                    button: button.to_string(),
                },
            );
        }
        RawHookMsg::MouseWheel(delta) => {
            emit_input(app, PetInputEvent::MouseWheel { delta });
        }
        RawHookMsg::MouseMove { x, y } => {
            if last_move.elapsed() < Duration::from_millis(16) {
                return;
            }
            *last_move = Instant::now();
            if *last_move_xy == Some((x, y)) {
                return;
            }
            *last_move_xy = Some((x, y));
            let (nx, ny) = normalized_virtual_screen(x, y);
            emit_input(app, PetInputEvent::MouseMove { x, y, nx, ny });
        }
    }
}

fn button_index(button: &str) -> Option<usize> {
    match button {
        "left" => Some(0),
        "right" => Some(1),
        "middle" => Some(2),
        _ => None,
    }
}

#[cfg(windows)]
fn normalized_virtual_screen(x: i32, y: i32) -> (f64, f64) {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };
    unsafe {
        let vx = GetSystemMetrics(SM_XVIRTUALSCREEN) as f64;
        let vy = GetSystemMetrics(SM_YVIRTUALSCREEN) as f64;
        let vw = (GetSystemMetrics(SM_CXVIRTUALSCREEN) as f64).max(1.0);
        let vh = (GetSystemMetrics(SM_CYVIRTUALSCREEN) as f64).max(1.0);
        (((x as f64 - vx) / vw).clamp(0.0, 1.0), ((y as f64 - vy) / vh).clamp(0.0, 1.0))
    }
}

#[cfg(not(windows))]
fn normalized_virtual_screen(_x: i32, _y: i32) -> (f64, f64) {
    (0.5, 0.5)
}

/// 每 750ms 用 `GetAsyncKeyState` 对账：低层钩子漏掉的抬起要补发，否则键位永久按住。
#[cfg(windows)]
fn reconcile_windows(app: &AppHandle, key_down: &mut HashSet<u32>, mouse_down: &mut [bool; 3]) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON,
    };
    unsafe {
        let stale: Vec<u32> = key_down
            .iter()
            .copied()
            .filter(|vk| (GetAsyncKeyState(*vk as i32) as u16 & 0x8000) == 0)
            .collect();
        for vk in stale {
            key_down.remove(&vk);
            emit_input(app, PetInputEvent::KeyUp { vk, scan: 0 });
        }
        let buttons = [
            (0usize, "left", VK_LBUTTON),
            (1usize, "right", VK_RBUTTON),
            (2usize, "middle", VK_MBUTTON),
        ];
        for (i, name, vk) in buttons {
            if mouse_down[i] && (GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000) == 0 {
                mouse_down[i] = false;
                emit_input(
                    app,
                    PetInputEvent::MouseUp {
                        button: name.to_string(),
                    },
                );
            }
        }
    }
}

/* ------------------------------ 光标看护 ------------------------------ */

fn spawn_cursor_watcher(app: AppHandle) {
    std::thread::spawn(move || {
        loop {
            if !ACTIVE.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(150));
                continue;
            }
            #[cfg(windows)]
            {
                if let Some((rel_x, rel_y, inside, alt)) = cursor_state(&app) {
                    let locked = CLICK_THROUGH_LOCKED.load(Ordering::Relaxed);
                    let ignore = should_ignore_cursor(locked, alt);
                    if ignore != CLICK_THROUGH_APPLIED.load(Ordering::Relaxed) {
                        if let Some(win) = app.get_webview_window(PET_LABEL) {
                            let _ = win.set_ignore_cursor_events(ignore);
                        }
                        CLICK_THROUGH_APPLIED.store(ignore, Ordering::Relaxed);
                    }
                    let _ = app.emit_to(
                        PET_LABEL,
                        "pet-cursor",
                        &PetCursorInfo {
                            rel_x,
                            rel_y,
                            inside,
                            alt_override: alt,
                        },
                    );
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    });
}

#[cfg(windows)]
fn cursor_state(app: &AppHandle) -> Option<(f64, f64, bool, bool)> {
    use windows::Win32::Foundation::{POINT, RECT};
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_MENU};
    use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetWindowRect};

    let win = app.get_webview_window(PET_LABEL)?;
    let hwnd = win.hwnd().ok()?;
    unsafe {
        let mut pt = POINT::default();
        GetCursorPos(&mut pt).ok()?;
        let mut rc = RECT::default();
        GetWindowRect(hwnd, &mut rc).ok()?;
        let w = (rc.right - rc.left).max(1) as f64;
        let h = (rc.bottom - rc.top).max(1) as f64;
        let rel_x = (pt.x - rc.left) as f64 / w;
        let rel_y = (pt.y - rc.top) as f64 / h;
        let inside = (0.0..=1.0).contains(&rel_x) && (0.0..=1.0).contains(&rel_y);
        let alt = (GetAsyncKeyState(VK_MENU.0 as i32) as u16 & 0x8000) != 0;
        Some((rel_x, rel_y, inside, alt))
    }
}

/* ------------------------------ 钩子线程 ------------------------------ */

#[cfg(windows)]
fn spawn_hook_thread() {
    std::thread::spawn(|| {
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetMessageW, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, MSG,
            WH_KEYBOARD_LL, WH_MOUSE_LL,
        };

        unsafe {
            let tid = windows::Win32::System::Threading::GetCurrentThreadId();
            HOOK_TID.store(tid, Ordering::SeqCst);
            // SetWindowsHookExW 要 HINSTANCE，GetModuleHandleW 给 HMODULE：两者都是模块基址句柄
            let hmod = Some(windows::Win32::Foundation::HINSTANCE(
                GetModuleHandleW(None).unwrap_or_default().0,
            ));
            let mut kb: Option<HHOOK> = None;
            let mut ms: Option<HHOOK> = None;
            let mut msg = MSG::default();
            // 启动竞态：本线程还没发布 tid 时，调用方可能已经发过 Install 请求（会丢）。
            // 这里补一次——只要 ACTIVE 已置位就先装上。
            if ACTIVE.load(Ordering::SeqCst) {
                kb = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook_proc), hmod, 0).ok();
                ms = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook_proc), hmod, 0).ok();
                HOOK_INSTALLED.store(kb.is_some() || ms.is_some(), Ordering::SeqCst);
            }

            loop {
                let ret = GetMessageW(&mut msg, None, 0, 0).0;
                if ret <= 0 {
                    // 0 = WM_QUIT；-1 = 出错。两种情况都退出消息循环：
                    // 卸载由进程退出兜底，不会留下装着的钩子。
                    break;
                }
                match msg.message {
                    WM_PET_INSTALL => {
                        if !HOOK_INSTALLED.load(Ordering::SeqCst) {
                            if kb.is_none() {
                                kb = SetWindowsHookExW(
                                    WH_KEYBOARD_LL,
                                    Some(keyboard_hook_proc),
                                    hmod,
                                    0,
                                )
                                .ok();
                            }
                            if ms.is_none() {
                                ms = SetWindowsHookExW(
                                    WH_MOUSE_LL,
                                    Some(mouse_hook_proc),
                                    hmod,
                                    0,
                                )
                                .ok();
                            }
                            HOOK_INSTALLED.store(kb.is_some() || ms.is_some(), Ordering::SeqCst);
                            crate::pet::plog(format!(
                                "pet_input: hooks installed kb={} ms={}",
                                kb.is_some(),
                                ms.is_some()
                            ));
                        }
                    }
                    WM_PET_UNINSTALL => {
                        if let Some(h) = kb.take() {
                            let _ = UnhookWindowsHookEx(h);
                        }
                        if let Some(h) = ms.take() {
                            let _ = UnhookWindowsHookEx(h);
                        }
                        HOOK_INSTALLED.store(false, Ordering::SeqCst);
                        crate::pet::plog("pet_input: hooks uninstalled".to_string());
                    }
                    _ => {}
                }
            }

            if kb.is_some() || ms.is_some() {
                let _ = PostThreadMessageW(tid, 0, Default::default(), Default::default());
            }
        }
    });
}

#[cfg(windows)]
fn send_raw(msg: RawHookMsg) {
    if let Ok(guard) = HOOK_TX.lock() {
        if let Some(tx) = guard.as_ref() {
            let _ = tx.send(msg);
        }
    }
}

#[cfg(windows)]
unsafe extern "system" fn keyboard_hook_proc(
    code: i32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, KBDLLHOOKSTRUCT, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
    };
    if code >= 0 && HOOK_INSTALLED.load(Ordering::Relaxed) {
        let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let msg = wparam.0 as u32;
        if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
            send_raw(RawHookMsg::KeyDown {
                vk: kb.vkCode,
                scan: kb.scanCode,
            });
        } else if msg == WM_KEYUP || msg == WM_SYSKEYUP {
            send_raw(RawHookMsg::KeyUp {
                vk: kb.vkCode,
                scan: kb.scanCode,
            });
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

#[cfg(windows)]
unsafe extern "system" fn mouse_hook_proc(
    code: i32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, MSLLHOOKSTRUCT, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN,
        WM_MBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONDOWN, WM_RBUTTONUP,
    };
    if code >= 0 && HOOK_INSTALLED.load(Ordering::Relaxed) {
        let ms = &*(lparam.0 as *const MSLLHOOKSTRUCT);
        match wparam.0 as u32 {
            WM_MOUSEMOVE => send_raw(RawHookMsg::MouseMove {
                x: ms.pt.x,
                y: ms.pt.y,
            }),
            WM_LBUTTONDOWN => send_raw(RawHookMsg::MouseDown("left")),
            WM_LBUTTONUP => send_raw(RawHookMsg::MouseUp("left")),
            WM_RBUTTONDOWN => send_raw(RawHookMsg::MouseDown("right")),
            WM_RBUTTONUP => send_raw(RawHookMsg::MouseUp("right")),
            WM_MBUTTONDOWN => send_raw(RawHookMsg::MouseDown("middle")),
            WM_MBUTTONUP => send_raw(RawHookMsg::MouseUp("middle")),
            WM_MOUSEWHEEL => {
                let delta = ((ms.mouseData >> 16) & 0xffff) as u16 as i16;
                send_raw(RawHookMsg::MouseWheel(delta));
            }
            _ => {}
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn button_mapping_is_stable() {
        assert_eq!(button_index("left"), Some(0));
        assert_eq!(button_index("right"), Some(1));
        assert_eq!(button_index("middle"), Some(2));
        assert_eq!(button_index("back"), None);
    }

    #[test]
    fn support_flag_matches_platform() {
        assert_eq!(is_supported(), cfg!(windows));
    }

    #[test]
    fn hooks_only_wanted_for_cat_pet() {
        assert!(wants_hooks(true, "cat"));
        assert!(!wants_hooks(false, "cat"));
        assert!(!wants_hooks(true, "classic"));
    }

    #[test]
    fn cursor_passthrough_requires_lock_without_alt() {
        assert!(should_ignore_cursor(true, false));
        assert!(!should_ignore_cursor(true, true));
        assert!(!should_ignore_cursor(false, false));
        assert!(!should_ignore_cursor(false, true));
    }
}
