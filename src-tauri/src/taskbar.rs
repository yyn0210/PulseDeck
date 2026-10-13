//! 任务栏显示模式：把勾选的指标贴在 Windows 任务栏上的一条细挂件
//!
//! 与桌面宠物（`pet.rs`）同为「Rust 侧持有的第二个 WebView」，但有三处不同：
//!
//! 1. **位置是算出来的**，不是拖出来的：贴住任务栏条带、停在通知区域（托盘图标 / 时钟）左侧。
//!    所以配置里没有 `position` 字段，页面也不设 `data-tauri-drag-region`。
//! 2. **宽度按内容量**：勾几个指标、数字有多长，就决定挂件多宽。尺寸由页面量完回传
//!    （`taskbar_fit`）——不在 Rust 里硬编码「每个指标占多少像素」，否则换字体 / 改文案就全错。
//! 3. **任务栏是会变的**：换边、改厚度、自动隐藏、explorer 重启（`Shell_TrayWnd` 换句柄）。
//!    建窗时拉起的**看护线程**每秒重算一次落点与 topmost，始终跟着任务栏；
//!    全屏应用在前台时还会把挂件藏起来——否则一条挂件会压在游戏 / 全屏视频画面上。
//!
//! 窗口置顶、无边框、透明、不进任务栏，并且**不抢焦点**（`focusable(false)` → WS_EX_NOACTIVATE）：
//! 任务栏旁边的东西一旦能激活，用户点一下焦点就从当前应用飞走了。整条挂件点一下呼出主面板。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{
    Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{ERROR_SUCCESS, HWND, RECT, WIN32_ERROR};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, FindWindowW, GetDesktopWindow, GetForegroundWindow, GetShellWindow,
    GetWindowLongPtrW, GetWindowRect, SetWindowPos, GWL_STYLE, HWND_TOPMOST, SWP_ASYNCWINDOWPOS,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WS_MAXIMIZE,
};

use crate::model::Snapshot;

/// 挂件窗口的 label。
///
/// **必须与 `capabilities/default.json` 的 `windows` 列表对应**——没有匹配到 capability 的
/// 窗口里，`invoke` 会被直接拒绝（挂件就拿不到读数了）。
pub const WINDOW_LABEL: &str = "taskbar";

/// 配置文件名（放在应用数据目录下）
const FILE_NAME: &str = "taskbar.json";

/// 可选指标，取值与前端 `taskbar.html` 里 chip 的 value、`taskbar.js` 的 `METRIC_DEFS` 一致
pub const METRIC_KEYS: [&str; 8] = ["cpu", "mem", "gpu", "net", "disk", "bat", "temp", "power"];

/// 默认显示：CPU / 内存 / 网络——任务栏上最常盯的三样，且总宽度不占掉整条任务栏
const DEFAULT_METRICS: [&str; 3] = ["cpu", "mem", "net"];

/// 各处留白（逻辑像素）：挂件与任务栏上下边缘、与通知区域、与任务栏两端
const PAD_LOGICAL: f64 = 3.0;
const GAP_LOGICAL: f64 = 6.0;
const EDGE_LOGICAL: f64 = 8.0;

/// 找不到通知区域时的兜底预留宽度（逻辑像素）。
/// 时钟 + 托盘图标大致就这么宽；宁可多留一点，也不要把挂件压在时钟上。
const TRAY_RESERVE_LOGICAL: f64 = 210.0;

/// 任务栏厚度的合理区间（物理像素）：超出这个范围说明拿到的矩形不是任务栏
/// （比如自动隐藏时整个滑到屏幕外、或者拿到一个异常的小矩形），退回默认厚度。
const MIN_TH: i32 = 16;
const MAX_TH: i32 = 200;

/// 拿不到任务栏矩形时的默认厚度（逻辑像素；Windows 11 任务栏 48px，Win10 40px）
const DEFAULT_TH_LOGICAL: f64 = 48.0;

/* ------------------------------ 配置 ------------------------------ */

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskbarConfig {
    /// 是否显示任务栏挂件
    #[serde(default)]
    pub enabled: bool,
    /// 要显示的指标（顺序即展示顺序），取值见 `METRIC_KEYS`
    #[serde(default = "default_metrics")]
    pub metrics: Vec<String>,
}

fn default_metrics() -> Vec<String> {
    DEFAULT_METRICS.iter().map(|s| s.to_string()).collect()
}

impl Default for TaskbarConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            metrics: default_metrics(),
        }
    }
}

impl TaskbarConfig {
    /// 清洗外部传入的配置：过滤非法指标、去重保序、兜底至少一项
    fn sanitize(&mut self) {
        let mut seen: Vec<String> = Vec::new();
        for m in std::mem::take(&mut self.metrics) {
            if METRIC_KEYS.contains(&m.as_str()) && !seen.contains(&m) {
                seen.push(m);
            }
        }
        if seen.is_empty() {
            seen.push(DEFAULT_METRICS[0].to_string());
        }
        self.metrics = seen;
    }
}

/// 配置的共享句柄（命令线程与看护线程共用一份）。
///
/// 与 `PetStore` 同一套路：原子写（临时文件 + rename）、写盘串行化
/// （`save_lock` 防止「A 还在写、B 把它 rename 走」留下半个 JSON）。
/// 差别只在于挂件没有「拖动产生的位置」，所以没有 `position_dirty` 那一套。
#[derive(Clone)]
pub struct TaskbarStore {
    config: Arc<Mutex<TaskbarConfig>>,
    save_lock: Arc<Mutex<()>>,
}

impl TaskbarStore {
    /// 从磁盘恢复；文件不存在或损坏时退回默认配置（挂件默认关闭，不做任何打扰）
    pub fn load() -> Self {
        let path = config_path();
        let parsed = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<TaskbarConfig>(&s).ok());
        let mut cfg = parsed.unwrap_or_default();
        cfg.sanitize();
        Self {
            config: Arc::new(Mutex::new(cfg)),
            save_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn get(&self) -> TaskbarConfig {
        self.lock().clone()
    }

    /// 落盘并返回清洗后的配置（前端回显以这份为准）
    pub fn set(&self, mut config: TaskbarConfig) -> TaskbarConfig {
        config.sanitize();
        *self.lock() = config.clone();
        self.save(&config);
        config
    }

    fn save(&self, config: &TaskbarConfig) {
        // 同一份 tmp 只能有一个写者（理由见 PetStore::save 的注释）。
        // 锁中毒不该把应用带崩，所以直接用 into_inner 忽略中毒。
        let _serialize_writes = self.save_lock.lock().unwrap_or_else(|e| e.into_inner());
        let path = config_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = path.with_extension("json.tmp");
        if serde_json::to_string_pretty(config)
            .ok()
            .and_then(|s| std::fs::write(&tmp, s).ok())
            .is_some()
        {
            let _ = std::fs::rename(&tmp, &path);
        }
    }

    /// 锁中毒不该把整个应用带崩（release 是 panic=abort），所以直接取回内部数据
    fn lock(&self) -> MutexGuard<'_, TaskbarConfig> {
        self.config.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn config_path() -> std::path::PathBuf {
    crate::app_dir().join(FILE_NAME)
}

/* ------------------------------ 几何（纯函数） ------------------------------
 *
 * 全部按**物理像素**算：Win32 拿到的矩形、显示器矩形都是物理像素，
 * 混进逻辑像素会随 DPI 缩放比例出错。
 */

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    fn right(&self) -> i32 {
        self.x + self.w
    }
    fn bottom(&self) -> i32 {
        self.y + self.h
    }
}

/// 任务栏贴在哪条边
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

impl Edge {
    /// 横排（上下边）= 指标一行排开；竖排（左右边）= 一列排开
    pub fn is_horizontal(self) -> bool {
        matches!(self, Edge::Top | Edge::Bottom)
    }
}

/// 任务栏条带：沿屏幕某条边的整条带状区域
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stripe {
    pub edge: Edge,
    pub rect: Rect,
}

/// 由任务栏窗口矩形算出「沿屏边的条带」。
///
/// **必须做这层归一化**：任务栏自动隐藏时，`Shell_TrayWnd` 会整个滑到屏幕外
/// （底部任务栏的 y 从屏底开始往下），直接拿它当落点会把挂件也甩到屏幕外——界面上就是
/// 「开了但什么都看不见」。这里只取它的**厚度**和**在哪条边**，条带本身重新贴到显示器边缘。
pub fn stripe_of(monitor: Rect, taskbar: Option<Rect>, default_th: i32) -> Stripe {
    let th = taskbar
        .map(|r| r.w.min(r.h))
        .filter(|t| (MIN_TH..=MAX_TH).contains(t))
        .unwrap_or(default_th);

    let edge = match taskbar {
        // 横向矩形（宽 ≥ 高）看上下边，纵向矩形看左右边；用「中心落在显示器中心哪一侧」判断
        Some(r) if r.w >= r.h => {
            if r.y + r.h / 2 < monitor.y + monitor.h / 2 {
                Edge::Top
            } else {
                Edge::Bottom
            }
        }
        Some(r) => {
            if r.x + r.w / 2 < monitor.x + monitor.w / 2 {
                Edge::Left
            } else {
                Edge::Right
            }
        }
        None => Edge::Bottom,
    };

    let rect = match edge {
        Edge::Top => Rect {
            x: monitor.x,
            y: monitor.y,
            w: monitor.w,
            h: th,
        },
        Edge::Bottom => Rect {
            x: monitor.x,
            y: monitor.bottom() - th,
            w: monitor.w,
            h: th,
        },
        Edge::Left => Rect {
            x: monitor.x,
            y: monitor.y,
            w: th,
            h: monitor.h,
        },
        Edge::Right => Rect {
            x: monitor.right() - th,
            y: monitor.y,
            w: th,
            h: monitor.h,
        },
    };
    Stripe { edge, rect }
}

/// 挂件在条带里的落点。
///
/// * 横向任务栏：高度铺满条带（减去上下留白），右端锚在**通知区域左侧**——
///   这样数字变宽时是往左长，右边沿纹丝不动，看着才像钉在任务栏上；
/// * 竖向任务栏：宽度铺满条带，下端同样锚在通知区域上方（竖排任务栏的托盘在底部）。
///
/// `tray` 取不到时按 `TRAY_RESERVE_LOGICAL` 从屏幕右端（或下端）预留一段。
pub fn dock_rect(stripe: Stripe, want: (i32, i32), tray: Option<Rect>, scale: f64) -> Rect {
    let pad = px(PAD_LOGICAL, scale);
    let gap = px(GAP_LOGICAL, scale);
    let edge_margin = px(EDGE_LOGICAL, scale);
    let reserve = px(TRAY_RESERVE_LOGICAL, scale);
    let s = stripe.rect;

    if stripe.edge.is_horizontal() {
        let h = (s.h - pad * 2).max(1);
        let w = want.0.clamp(1, (s.w - edge_margin * 2).max(1));
        let anchor = match tray {
            Some(t) => t.x - gap,
            None => s.right() - reserve,
        };
        // 先贴住锚点，再整体夹进条带——小屏 + 一堆托盘图标时宁可压住图标，也不能跑出屏幕
        let x = (anchor - w)
            .max(s.x + edge_margin)
            .min(s.right() - edge_margin - w)
            .max(s.x);
        Rect { x, y: s.y + pad, w, h }
    } else {
        let w = (s.w - pad * 2).max(1);
        let h = want.1.clamp(1, (s.h - edge_margin * 2).max(1));
        let anchor = match tray {
            Some(t) => t.y - gap,
            None => s.bottom() - reserve,
        };
        let y = (anchor - h)
            .max(s.y + edge_margin)
            .min(s.bottom() - edge_margin - h)
            .max(s.y);
        Rect { x: s.x + pad, y, w, h }
    }
}

/// 前台窗口是不是真的「全屏」。
///
/// 两条都要满足：贴满整块显示器（容差 2px）、**且不是被最大化**。
/// 为什么必须排除最大化：任务栏自动隐藏时，最大化的窗口同样铺满整块屏幕，
/// 只按矩形判断的话，用户一最大化窗口挂件就消失了。
pub fn covers_monitor(fg: Rect, monitor: Rect, maximized: bool) -> bool {
    const TOL: i32 = 2;
    !maximized
        && fg.x <= monitor.x + TOL
        && fg.y <= monitor.y + TOL
        && fg.right() >= monitor.right() - TOL
        && fg.bottom() >= monitor.bottom() - TOL
}

fn px(logical: f64, scale: f64) -> i32 {
    (logical * scale).round().max(0.0) as i32
}

/* ------------------------------ 桌面信息 ------------------------------ */

struct Desk {
    /// 主显示器的物理矩形
    monitor: Rect,
    scale: f64,
}

/// 主显示器（主任务栏一定在它上面）。
///
/// 取不到显示器信息时退回 1920×1080 / 100%——异常路径上宁可把挂件放在一个合理的默认位置，
/// 也不要因为一次查询失败就整条挂件不显示。
fn desk_of(app: &tauri::AppHandle) -> Desk {
    if let Ok(Some(m)) = app.primary_monitor() {
        let pos = m.position();
        let size = m.size();
        return Desk {
            monitor: Rect {
                x: pos.x,
                y: pos.y,
                w: size.width as i32,
                h: size.height as i32,
            },
            scale: m.scale_factor().max(0.1),
        };
    }
    Desk {
        monitor: Rect {
            x: 0,
            y: 0,
            w: 1920,
            h: 1080,
        },
        scale: 1.0,
    }
}

/// 当前任务栏条带
fn current_stripe(desk: &Desk) -> Stripe {
    stripe_of(
        desk.monitor,
        window_rect_opt(taskbar_window()),
        px(DEFAULT_TH_LOGICAL, desk.scale),
    )
}

/* ------------------------------ Win32 查询 ------------------------------ */

fn rect_from(r: RECT) -> Rect {
    Rect {
        x: r.left,
        y: r.top,
        w: r.right - r.left,
        h: r.bottom - r.top,
    }
}

fn window_rect(hwnd: HWND) -> Option<Rect> {
    let mut r = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut r).ok()? };
    Some(rect_from(r))
}

fn window_rect_opt(hwnd: Option<HWND>) -> Option<Rect> {
    hwnd.and_then(window_rect)
}

/// 主任务栏窗口（`Shell_TrayWnd`）
fn taskbar_window() -> Option<HWND> {
    unsafe { FindWindowW(windows::core::w!("Shell_TrayWnd"), PCWSTR::null()).ok() }
}

/// 通知区域（托盘图标 + 时钟）的矩形：挂件停在它左边，不去压时钟。
/// 找不到（个别系统 / 精简版外壳）时返回 None，`dock_rect` 会退回固定预留宽度。
fn tray_rect() -> Option<Rect> {
    let bar = taskbar_window()?;
    let tray = unsafe {
        FindWindowExW(Some(bar), None, windows::core::w!("TrayNotifyWnd"), PCWSTR::null()).ok()?
    };
    window_rect(tray)
}

/// 任务栏是浅色还是深色（`light` / `dark`）。
///
/// 读的是 `SystemUsesLightTheme`（设置里的「Windows 模式」），**不是** `AppsUseLightTheme`
/// ——后者管应用窗口，和任务栏可以不一致（很多人正是「应用浅色 + 任务栏深色」）。
/// 读不到时按深色处理：深色任务栏是绝大多数截图里的默认观感。
fn taskbar_theme() -> &'static str {
    let mut value: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    let rc: WIN32_ERROR = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            windows::core::w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            windows::core::w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            Some(std::ptr::null_mut()),
            Some(&mut value as *mut u32 as *mut core::ffi::c_void),
            Some(&mut size),
        )
    };
    if rc == ERROR_SUCCESS && value != 0 {
        "light"
    } else {
        "dark"
    }
}

/// 前台窗口是否盖住了整块屏幕（全屏游戏 / 播放器 / 网页全屏视频）。判定逻辑见 [`covers_monitor`]。
fn foreground_covers(monitor: Rect) -> bool {
    unsafe {
        let fg = GetForegroundWindow();
        // 桌面 / 外壳窗口铺满屏幕是常态，不是「全屏应用」
        if fg.0.is_null() || fg == GetShellWindow() || fg == GetDesktopWindow() {
            return false;
        }
        let Some(r) = window_rect(fg) else {
            return false;
        };
        let maximized = GetWindowLongPtrW(fg, GWL_STYLE) as u32 & WS_MAXIMIZE.0 != 0;
        covers_monitor(r, monitor, maximized)
    }
}

/// 把挂件重新顶到置顶带最上面。
///
/// 不能只调用 `set_always_on_top(true)`：tao 只在标志位**发生变化**时才发 `SetWindowPos`
/// （`apply_diff` 里 diff 为空直接返回），已经是 topmost 的窗口再调一次什么都不会发生。
/// 而任务栏被点一下就会盖到普通置顶窗口上面（Windows 的既定行为），所以必须自己发一次
/// `SetWindowPos(HWND_TOPMOST)`——带 `SWP_NOACTIVATE`，不会抢焦点。
fn raise_topmost(win: &WebviewWindow) -> tauri::Result<()> {
    let hwnd = win.hwnd()?;
    unsafe {
        SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_ASYNCWINDOWPOS | SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
        .ok()
    };
    Ok(())
}

/* ------------------------------ 窗口 ------------------------------ */

/// 按配置同步窗口的存在与否：开 → 没有就建；关 → 有就销毁。
///
/// ⚠ 与 `pet::sync_window` 一样，**绝不能从同步命令或事件回调里调用**（Windows 下
/// `WebviewWindowBuilder::build()` 会死锁，机理见 `pet::sync_window` 的注释）。
/// 调用方 `set_taskbar_config` 必须是 async 命令。
pub fn sync_window(app: &tauri::AppHandle, config: &TaskbarConfig) -> tauri::Result<()> {
    if config.enabled {
        if app.get_webview_window(WINDOW_LABEL).is_none() {
            create_window(app, config)?;
        }
    } else if let Some(win) = app.get_webview_window(WINDOW_LABEL) {
        win.destroy()?;
    }
    Ok(())
}

fn create_window(app: &tauri::AppHandle, config: &TaskbarConfig) -> tauri::Result<()> {
    let desk = desk_of(app);
    let stripe = current_stripe(&desk);
    let vertical = !stripe.edge.is_horizontal();
    let want = {
        let logical = estimate_logical_size(&config.metrics, vertical);
        (px(logical.0, desk.scale), px(logical.1, desk.scale))
    };
    let dock = dock_rect(stripe, want, tray_rect(), desk.scale);

    // Windows 上多个 WebView 共享同一个 ICoreWebView2Environment（与宠物窗口同一份，
    // 见 pet.rs 的 MAIN_ENV 注释）：保持一致才不会有环境选项冲突。
    let environment = crate::pet::shared_webview_environment(app);

    let mut builder = WebviewWindowBuilder::new(app, WINDOW_LABEL, WebviewUrl::App("taskbar.html".into()))
        .title("任务栏显示")
        .inner_size(
            dock.w as f64 / desk.scale,
            dock.h as f64 / desk.scale,
        )
        .position(dock.x as f64 / desk.scale, dock.y as f64 / desk.scale);
    if let Some(env) = environment {
        builder = builder.with_environment(env);
    }
    builder = builder
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        // 不抢焦点（WS_EX_NOACTIVATE）：关掉「全屏应用」后再显示、或者用户误点到挂件时，
        // 都不该把焦点从他正在用的窗口上抢走
        .focusable(false)
        .focused(false);
    // 附加参数必须与主窗口 / 宠物窗口一致（见 pet::BROWSER_ARGS 的注释）；
    // 当前 BROWSER_ARGS 为空，空串就不下发（避免传一个空参数）。
    if !crate::pet::BROWSER_ARGS.is_empty() {
        builder = builder.additional_browser_args(crate::pet::BROWSER_ARGS);
    }
    // 建成可见窗口：WebView2 在「隐藏窗口」里创建时出现过整窗空白不渲染，
    // 而挂件页面本身就带底色，不存在白底闪一下的问题。
    let win = builder.visible(true).build()?;

    // 建窗时的逻辑坐标要经窗口自己的缩放因子换算，与主显示器的不一定完全一致；
    // 建完立刻用**物理**坐标再摆一次，位置不留误差。
    apply_dock(&win, dock)?;
    raise_topmost(&win)?;
    spawn_watcher(app.clone());
    Ok(())
}

/// 把窗口摆到算好的落点（物理像素）。
///
/// 用物理坐标而不是逻辑坐标：`set_position(LogicalPosition)` 是按窗口**当前**的缩放因子换算的，
/// 而落点是照着主显示器算的——多屏不同 DPI 时这两个不一定是同一个值。
fn apply_dock(win: &WebviewWindow, dock: Rect) -> tauri::Result<()> {
    // 尺寸没变就别设：set_size 会让 WebView 重新布局一次，每秒都设是白闪
    let cur = win.inner_size()?;
    if cur.width != dock.w.max(1) as u32 || cur.height != dock.h.max(1) as u32 {
        win.set_size(PhysicalSize::new(dock.w.max(1) as u32, dock.h.max(1) as u32))?;
    }
    let pos = win.outer_position()?;
    if pos.x != dock.x || pos.y != dock.y {
        win.set_position(PhysicalPosition::new(dock.x, dock.y))?;
    }
    Ok(())
}

/// 看护线程：窗口被销毁（关掉开关）时循环自己结束。
///
/// 每秒三件事：全屏应用在前台就藏起来、重算落点、把自己重新顶到最上面。
/// 这些**不能交给页面做**：窗口一旦被藏起来，Chromium 会把页面定时器节流到分钟级，
/// 那样「全屏应用退出后挂件迟迟不回来」就成了必然。
fn spawn_watcher(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let mut hidden = false;
        loop {
            let Some(win) = app.get_webview_window(WINDOW_LABEL) else {
                break;
            };
            let desk = desk_of(&app);
            let fullscreen = foreground_covers(desk.monitor);
            if fullscreen != hidden {
                hidden = fullscreen;
                HIDDEN_BY_FULLSCREEN.store(hidden, Ordering::Relaxed);
                let _ = if hidden { win.hide() } else { win.show() };
            }
            if !hidden {
                if let Ok(cur) = win.inner_size() {
                    let stripe = current_stripe(&desk);
                    let dock = dock_rect(
                        stripe,
                        (cur.width as i32, cur.height as i32),
                        tray_rect(),
                        desk.scale,
                    );
                    let _ = apply_dock(&win, dock);
                }
                let _ = raise_topmost(&win);
            }
            std::thread::sleep(Duration::from_millis(1000));
        }
        HIDDEN_BY_FULLSCREEN.store(false, Ordering::Relaxed);
    });
}

/// 挂件首屏尺寸估算（逻辑像素）。
///
/// 只是「首屏别太离谱」：落地后由页面量出的真实尺寸覆盖（[`fit`]）。
/// 所以这里的分档宁可略宽——窄了会当场把文字挤没，宽了下一拍就自己收回来。
fn estimate_logical_size(metrics: &[String], vertical: bool) -> (f64, f64) {
    const PAD: f64 = 18.0;
    const GAP: f64 = 10.0;

    fn width_of(key: &str) -> f64 {
        match key {
            "net" => 122.0,
            "disk" => 126.0,
            "bat" => 64.0,
            "temp" | "power" => 68.0,
            // cpu / mem / gpu 及一切未知档位
            _ => 60.0,
        }
    }

    if vertical {
        // 竖排：每项一行，行高按 12px 字 + 行距算
        (74.0, PAD + metrics.len() as f64 * GAP + metrics.len() as f64 * 16.0)
    } else {
        let w = PAD + metrics.iter().map(|m| width_of(m) + GAP).sum::<f64>();
        (w, 30.0)
    }
}

/// 页面把量出来的内容尺寸（CSS 像素）回传，挂件按它调整窗口大小与落点。
///
/// 宽度完全由内容决定：与其在 Rust 里硬编码「每个指标占多少像素」（换字体 / 改文案就全错），
/// 不如让浏览器量自己。数字变宽时窗口往左长——右端锚在通知区域，右边沿不动，看着像钉住的。
pub fn fit(app: &tauri::AppHandle, width: f64, height: f64) -> tauri::Result<()> {
    let Some(win) = app.get_webview_window(WINDOW_LABEL) else {
        return Ok(());
    };
    if !width.is_finite() || !height.is_finite() {
        return Ok(());
    }
    let scale = win.scale_factor().unwrap_or(1.0).max(0.1);
    let desk = desk_of(app);
    let stripe = current_stripe(&desk);
    let dock = dock_rect(stripe, (px(width, scale), px(height, scale)), tray_rect(), desk.scale);
    apply_dock(&win, dock)
}

/* ------------------------------ 心跳 ------------------------------ */

/// 挂件页面最后一次心跳的时刻（Unix 毫秒）。
///
/// 与宠物窗口同样的用途：设置页据此显示「运行中 / 未响应」，
/// 用户与开发者不必靠肉眼判断一条贴在任务栏上的细挂件是否正常。
static LAST_BEAT_MS: AtomicU64 = AtomicU64::new(0);

/// 是否正被「全屏应用在前台」藏着——设置页的状态行据此把「藏起来了」和「坏了」分开说
static HIDDEN_BY_FULLSCREEN: AtomicBool = AtomicBool::new(false);

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn record_beat() {
    LAST_BEAT_MS.store(now_ms(), Ordering::Relaxed);
}

/// 距离上次心跳过去多久（毫秒）；从未收到过心跳时为 None
pub fn beat_age_ms() -> Option<u64> {
    let last = LAST_BEAT_MS.load(Ordering::Relaxed);
    if last == 0 {
        None
    } else {
        Some(now_ms().saturating_sub(last))
    }
}

/* ------------------------------ 读数 ------------------------------ */

/// 挂件专用读数（只含界面真正要显示的几个数，不搬整份快照）
#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct TaskbarMetrics {
    pub cpu_pct: f32,
    pub mem_pct: f32,
    /// GPU 占用；无 GPU / 驱动没上报时为 None
    pub gpu_pct: Option<f32>,
    /// 全机合计数率（byte/s）
    pub net_rx_bps: u64,
    pub net_tx_bps: u64,
    /// 所有卷合计数率（byte/s）
    pub disk_read_bps: u64,
    pub disk_write_bps: u64,
    /// 电量百分比；无电池（台式机 / 无电池设备）时为 None
    pub bat_pct: Option<f32>,
    /// CPU 温度（°C）；读不到时为 None
    pub cpu_temp_c: Option<f32>,
    /// 功率（W）：优先电池充/放电功率，其次 CPU 封装功耗
    pub power_w: Option<f32>,
}

/// 挂件每秒一问的「一拍」：读数 + 影响排版的环境信息
#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct TaskbarTick {
    pub metrics: TaskbarMetrics,
    /// 任务栏竖排（贴在屏幕左右侧）——页面据此把指标排成一列
    pub vertical: bool,
    /// 任务栏是浅色还是深色（`light` / `dark`）——决定挂件的文字与底色
    pub theme: String,
}

pub fn metrics_from(s: &Snapshot) -> TaskbarMetrics {
    let (disk_read_bps, disk_write_bps) = s.disks.iter().fold((0u64, 0u64), |(r, w), d| {
        (
            r.saturating_add(d.read_bytes_per_sec),
            w.saturating_add(d.write_bytes_per_sec),
        )
    });
    TaskbarMetrics {
        cpu_pct: s.cpu.usage_total,
        mem_pct: s.memory.usage_pct,
        gpu_pct: s.gpu.usage_pct.filter(|v| v.is_finite()),
        net_rx_bps: s.net.rx_bytes_per_sec,
        net_tx_bps: s.net.tx_bytes_per_sec,
        disk_read_bps,
        disk_write_bps,
        bat_pct: s.battery.percent.filter(|v| v.is_finite()),
        cpu_temp_c: s.thermal.cpu_temp_c.filter(|v| v.is_finite()),
        power_w: power_of(s),
    }
}

/// 功率显示哪个数：电池的充/放电功率是「整机功耗」里最有意义的那个；
/// 没有电池、或固件给的是 0（适配器供电但没充电，笔记本满电后的常态）时，
/// 退回 CPU 封装功耗（PDH / RAPL）——总比在任务栏上长期挂一个 0.0W 有用。
fn power_of(s: &Snapshot) -> Option<f32> {
    let bat = s
        .battery
        .charge_rate_w
        .or(s.battery.discharge_rate_w)
        .filter(|v| v.is_finite() && *v > 0.0);
    bat.or_else(|| {
        s.thermal
            .cpu_power_w
            .filter(|v| v.is_finite() && *v > 0.0)
    })
}

/// 页面每秒一次的「一拍」：记心跳 + 回读数与排版信息
pub fn tick(app: &tauri::AppHandle, snapshot: &Snapshot) -> TaskbarTick {
    record_beat();
    let desk = desk_of(app);
    let stripe = current_stripe(&desk);
    TaskbarTick {
        metrics: metrics_from(snapshot),
        vertical: !stripe.edge.is_horizontal(),
        theme: taskbar_theme().to_string(),
    }
}

/// 挂件窗口的就绪状态：是否开着、页面心跳是否新鲜、是不是正被全屏应用挡着藏起来了
pub fn status(app: &tauri::AppHandle) -> serde_json::Value {
    let open = app.get_webview_window(WINDOW_LABEL).is_some();
    let age = beat_age_ms();
    // 页面固定 1s 心跳，3s 内没消息就认为没在跑
    let alive = open && age.is_some_and(|ms| ms < 3000);
    serde_json::json!({
        "windowOpen": open,
        "heartbeatAgeMs": age,
        "alive": alive,
        "hiddenFullscreen": HIDDEN_BY_FULLSCREEN.load(Ordering::Relaxed),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MON: Rect = Rect {
        x: 0,
        y: 0,
        w: 2560,
        h: 1440,
    };

    /* ---------- 条带归一化 ---------- */

    /// 任务栏自动隐藏时会整个滑到屏幕外，这时**不能**照搬它的矩形——
    /// 照搬就把挂件也甩到屏幕外，表现为「开了但桌面上什么都看不见」。
    #[test]
    fn stripe_normalizes_auto_hidden_taskbar() {
        let offscreen = Rect {
            x: 0,
            y: 1440,
            w: 2560,
            h: 48,
        };
        let s = stripe_of(MON, Some(offscreen), 48);
        assert_eq!(s.edge, Edge::Bottom);
        assert_eq!(s.rect, Rect { x: 0, y: 1440 - 48, w: 2560, h: 48 });
    }

    #[test]
    fn stripe_reads_vertical_taskbar() {
        let left = Rect { x: 0, y: 0, w: 60, h: 1440 };
        let s = stripe_of(MON, Some(left), 48);
        assert_eq!(s.edge, Edge::Left);
        assert_eq!(s.rect.w, 60);
        assert!(!s.edge.is_horizontal());

        let right = Rect { x: 2500, y: 0, w: 60, h: 1440 };
        let s = stripe_of(MON, Some(right), 48);
        assert_eq!(s.edge, Edge::Right);
    }

    #[test]
    fn stripe_handles_top_taskbar_and_missing_rect() {
        let top = Rect { x: 0, y: 0, w: 2560, h: 40 };
        let s = stripe_of(MON, Some(top), 48);
        assert_eq!(s.edge, Edge::Top);
        assert_eq!(s.rect.y, 0);

        // 拿不到任务栏（explorer 刚崩 / 精简外壳）：贴底 + 默认厚度
        let s = stripe_of(MON, None, 48);
        assert_eq!(s.edge, Edge::Bottom);
        assert_eq!(s.rect.h, 48);
        assert_eq!(s.rect.bottom(), MON.bottom());
    }

    /// 荒谬厚度（比如拿到一个 3px 的矩形）要退回默认值，别把挂件做成一条线
    #[test]
    fn stripe_rejects_absurd_thickness() {
        let weird = Rect { x: 0, y: 1437, w: 2560, h: 3 };
        let s = stripe_of(MON, Some(weird), 48);
        assert_eq!(s.rect.h, 48);
    }

    /* ---------- 落点 ---------- */

    #[test]
    fn dock_sits_left_of_tray() {
        let s = stripe_of(MON, Some(Rect { x: 0, y: 1392, w: 2560, h: 48 }), 48);
        let tray = Rect { x: 2300, y: 1392, w: 260, h: 48 };
        let d = dock_rect(s, (272, 42), Some(tray), 1.0);
        assert_eq!(d.x, 2300 - 6 - 272);
        assert_eq!(d.y, 1392 + 3);
        assert_eq!(d.h, 48 - 6);
        assert_eq!(d.w, 272);
    }

    /// 挂件再宽也不能越过任务栏左端（小屏 + 一堆托盘图标时宁可压住图标，也不能跑出屏幕）
    #[test]
    fn dock_clamps_into_stripe() {
        let s = stripe_of(MON, Some(Rect { x: 0, y: 1392, w: 2560, h: 48 }), 48);
        let tray = Rect { x: 2400, y: 1392, w: 160, h: 48 };
        let d = dock_rect(s, (2500, 42), Some(tray), 1.0);
        assert_eq!(d.x, 0 + 8);

        // 通知区域在很右边、挂件也不大时，右端留白同样生效
        let d = dock_rect(s, (100, 42), None, 1.0);
        assert_eq!(d.x, 2560 - 210 - 100);
    }

    #[test]
    fn dock_scales_with_dpi() {
        let s = stripe_of(MON, Some(Rect { x: 0, y: 1392, w: 2560, h: 72 }), 72);
        let tray = Rect { x: 2300, y: 1392, w: 260, h: 72 };
        let d = dock_rect(s, (400, 200), Some(tray), 1.5);
        // 留白按缩放算：6pt * 1.5 = 9px
        assert_eq!(d.x, 2300 - 9 - 400);
        // y 取的是**归一化后的条带**（贴屏幕底边）：1440 - 72 + 3*1.5 = 1373
        assert_eq!(d.y, 1373);
        assert_eq!(d.h, 72 - 10);
    }

    #[test]
    fn dock_vertical_stacks_above_tray() {
        let s = stripe_of(MON, Some(Rect { x: 0, y: 0, w: 60, h: 1440 }), 48);
        let tray = Rect { x: 0, y: 1280, w: 60, h: 160 };
        let d = dock_rect(s, (54, 90), Some(tray), 1.0);
        assert_eq!(d.x, 0 + 3);
        assert_eq!(d.y, 1280 - 6 - 90);
        assert_eq!(d.w, 60 - 6);
        // 宽度取条带（竖排任务栏就那么宽），页面量出来的宽度在这里不生效
        assert_eq!(d.h, 90);
    }

    /* ---------- 全屏判定 ---------- */

    #[test]
    fn covers_monitor_detects_fullscreen_only() {
        let full = Rect { x: 0, y: 0, w: 2560, h: 1440 };
        assert!(covers_monitor(full, MON, false));

        // 最大化窗口（任务栏自动隐藏时同样铺满整块屏幕）不算全屏应用
        assert!(!covers_monitor(full, MON, true));

        // 只是「很宽的窗口」不算
        assert!(!covers_monitor(
            Rect { x: 0, y: 0, w: 2560, h: 1360 },
            MON,
            false
        ));
    }

    #[test]
    fn covers_monitor_tolerates_edge_pixels() {
        let almost = Rect { x: -1, y: 0, w: 2561, h: 1440 };
        assert!(covers_monitor(almost, MON, false));
    }

    /* ---------- 配置 ---------- */

    #[test]
    fn sanitize_drops_unknown_and_dedups() {
        let mut cfg = TaskbarConfig {
            enabled: true,
            metrics: vec!["gpu".into(), "cpu".into(), "gpu".into(), "温度".into()],
        };
        cfg.sanitize();
        assert_eq!(cfg.metrics, vec!["gpu".to_string(), "cpu".to_string()]);
    }

    #[test]
    fn sanitize_keeps_at_least_one_metric() {
        let mut cfg = TaskbarConfig {
            enabled: true,
            metrics: vec!["nope".into()],
        };
        cfg.sanitize();
        assert_eq!(cfg.metrics, vec![DEFAULT_METRICS[0].to_string()]);
    }

    /// 缺字段的配置文件（手改过、或以后加了新字段）必须还能解析——
    /// 解析失败会被 `load` 当成「没有配置」静默退回默认值，等于把用户的开关悄悄关掉。
    #[test]
    fn partial_config_still_parses() {
        let cfg: TaskbarConfig = serde_json::from_str("{}").expect("空配置必须能解析成默认值");
        assert!(!cfg.enabled);
        assert_eq!(cfg.metrics, default_metrics());

        let cfg: TaskbarConfig =
            serde_json::from_str(r#"{"enabled":true}"#).expect("只有 enabled 也要能解析");
        assert!(cfg.enabled);
        assert_eq!(cfg.metrics, default_metrics());
    }

    #[test]
    fn default_metrics_are_valid() {
        for m in DEFAULT_METRICS {
            assert!(METRIC_KEYS.contains(&m), "{m} 不在 METRIC_KEYS 里");
        }
    }

    /// 指标键在几处必须一致：Rust 的 `METRIC_KEYS`、设置页的 chip、挂件页的渲染定义。
    /// 少一处就是「勾了存不下去 / 存了显示不出来」这种最难查的半坏状态。
    #[test]
    fn metric_keys_are_consistent_across_files() {
        let html = include_str!("../../index.html");
        let js = include_str!("../../src/taskbar.js");
        for key in METRIC_KEYS {
            assert!(
                html.contains(&format!("value=\"{key}\"")),
                "设置页 index.html 缺少 {key} 的勾选项"
            );
            assert!(
                js.contains(&format!("{key}: {{")),
                "挂件页 src/taskbar.js 的 METRIC_DEFS 缺少 {key}"
            );
        }
    }

    /* ---------- 读数 ---------- */

    #[test]
    fn metrics_sum_disks_and_fall_back_power() {
        let mut s = Snapshot::default();
        s.cpu.usage_total = 42.0;
        s.memory.usage_pct = 61.0;
        s.gpu.usage_pct = Some(18.0);
        s.net.rx_bytes_per_sec = 1_000;
        s.net.tx_bytes_per_sec = 200;
        s.disks = vec![
            crate::model::DiskInfo {
                read_bytes_per_sec: 300,
                write_bytes_per_sec: 40,
                ..Default::default()
            },
            crate::model::DiskInfo {
                read_bytes_per_sec: 700,
                write_bytes_per_sec: 60,
                ..Default::default()
            },
        ];
        // 电池既没充也没放（固件报 0）→ 退回 CPU 封装功耗
        s.battery.percent = Some(78.0);
        s.battery.charge_rate_w = Some(0.0);
        s.battery.discharge_rate_w = Some(0.0);
        s.thermal.cpu_temp_c = Some(62.0);
        s.thermal.cpu_power_w = Some(19.4);

        let m = metrics_from(&s);
        assert_eq!(m.cpu_pct, 42.0);
        assert_eq!(m.disk_read_bps, 1_000);
        assert_eq!(m.disk_write_bps, 100);
        assert_eq!(m.bat_pct, Some(78.0));
        assert_eq!(m.cpu_temp_c, Some(62.0));
        assert_eq!(m.power_w, Some(19.4));

        // 正在充电时优先用充入功率
        s.battery.charge_rate_w = Some(45.5);
        assert_eq!(metrics_from(&s).power_w, Some(45.5));
    }

    /// 无电池的机器上电量必须是 None（前端显示 --），不能悄悄当成 0%
    #[test]
    fn metrics_without_battery_report_none() {
        let s = Snapshot::default();
        let m = metrics_from(&s);
        assert_eq!(m.bat_pct, None);
        assert_eq!(m.gpu_pct, None);
        assert_eq!(m.power_w, None);
    }

    /* ---------- 与另一个 WebView 窗口的一致性（都是曾经踩过的坑） ---------- */

    /// 挂件窗口同样必须复用主窗口的 WebView2 环境、并且与主窗口带**同一份**附加参数。
    /// 不一致时的表现是 `HRESULT(0x8007139F)`：建窗静默失败，界面上就是「开关打开了但什么都
    /// 不出现」。这里卡住 `create_window` 里的两处引用，改了它们就红。
    #[test]
    fn window_reuses_main_environment_and_browser_args() {
        let src = include_str!("taskbar.rs");
        assert!(
            src.contains("crate::pet::shared_webview_environment"),
            "挂件窗口必须复用主窗口的 WebView2 环境（不共享会以 0x8007139F 建不出来）"
        );
        assert!(
            src.contains("crate::pet::BROWSER_ARGS"),
            "挂件窗口的附加参数必须来自 pet::BROWSER_ARGS（两个窗口参数不一致会以 \
             0x8007139F 建不出来）"
        );
    }

    /// `set_taskbar_config` 必须是 async 命令：同步命令跑在主线程的 WebView2 IPC 回调里，
    /// 在那里 `WebviewWindowBuilder::build()` 会永久等不到 COM 完成回调 → 整个进程卡死
    /// （机理见 `pet::sync_window` 的注释）。
    #[test]
    fn set_taskbar_config_is_async_command() {
        let lib = include_str!("lib.rs");
        assert!(
            lib.contains("async fn set_taskbar_config"),
            "lib.rs 的 set_taskbar_config 必须是 async 命令，否则启用挂件时整个进程会死锁"
        );
    }

    /// 窗口标签必须出现在 capability 的 windows 列表里，否则挂件页面里所有 invoke 都会被拒
    /// （表现是「贴纸在但全是 --」，没有任何报错提示）。
    #[test]
    fn capability_lists_taskbar_window() {
        let cap = include_str!("../capabilities/default.json");
        assert!(
            cap.contains("\"taskbar\""),
            "capabilities/default.json 的 windows 列表必须含 taskbar"
        );
    }
}
