//! 桌面宠物：一个置顶、无边框、透明的小窗口
//!
//! 设计要点：
//! * **窗口由 Rust 管**：设置页只负责改配置（`set_pet_config`），窗口的创建/销毁发生在这里，
//!   因此「最小化到托盘」时宠物依然在桌面上，也不需要前端持有窗口句柄。
//! * **配置落盘**：应用数据目录（见 `app_dir()`）下的 `pet.json`，下次启动自动恢复
//!   （不依赖主窗口先起来）。
//! * **读数走专用命令**：宠物窗口只需要 3~4 个数，不值得每拍搬一整份 `Snapshot`
//!   （里面还有进程榜与磁盘列表），所以单独给一个 `pet_metrics`。
//!
//! 跨平台：窗口生命周期、配置、位置持久化、心跳全部是 tauri 通用 API，
//! 三平台一致；只有「复用主窗口 WebView2 环境」是 Windows 专属机制（见下方注释），
//! 其余平台建窗即独立环境，无需处理。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
#[cfg(windows)]
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

use crate::model::Snapshot;

/// 宠物窗口的 label。
///
/// **必须与 `capabilities/default.json` 的 `windows` 列表对应**——没有匹配到 capability 的
/// 窗口里，`invoke` 会被直接拒绝（宠物窗口就拿不到读数了）。
pub const WINDOW_LABEL: &str = "pet";

/// 配置文件名（放在应用数据目录下）
const FILE_NAME: &str = "pet.json";

/// 窗口尺寸：按 3 行指标 + 一只宠物的高度设计；行数变少时内容垂直居中，留白而不是改窗口大小
///
/// 高度跟着贴图走：形象是 15:16 的逐帧贴图（`src/assets/pet/`），118px 宽 → 约 125px 高，
/// 比原来的内联 SVG（118x106）高约 19px，所以窗口高度从 202 提到 224。
/// **换贴图比例时这里要跟着改**，否则底部指标条会被裁掉。
const WIN_W: f64 = 190.0;
const WIN_H: f64 = 224.0;

/// 可选指标，取值与前端 `index.html` 里 checkbox 的 value 一致
pub const METRIC_KEYS: [&str; 3] = ["cpu", "mem", "gpu"];

/// 皮肤白名单：6 款猫 + 鲸鱼娘（鲸鱼娘只有 2D 渲染器）
pub const SKIN_IDS: [&str; 7] = [
    "black",
    "ragdoll",
    "ginger",
    "calico",
    "tuxedo",
    "siamese",
    "dafeiyu",
];
/// 键盘布局白名单
pub const LAYOUT_IDS: [&str; 2] = ["65", "108"];
/// 键盘视角白名单
pub const ORIENTATION_IDS: [&str; 2] = ["cat", "viewer"];
/// 3D 项圈颜色白名单
pub const COLLAR_IDS: [&str; 5] = ["none", "red", "blue", "pink", "green"];
/// 3D 耳机白名单
pub const HEADPHONE_IDS: [&str; 2] = ["off", "on"];

/// 打字猫模式指标条的逻辑高度（画布之外固定占位，保证小屏也读得清）
pub const CAT_METRICS_BAR_H: f64 = 44.0;

/// 传给 WebView2 的附加命令行参数。**当前为空 = 两个窗口都不带附加参数**。
///
/// ⚠️ 历史教训一（0x8007139F）：Windows 下同一份 WebView2 用户数据目录只能对应
/// 「一组」附加参数。主窗口用 A、宠物窗口用 B（包括 B 是 wry 默认串），建第二个
/// WebView 时 WebView2 直接返回 `HRESULT(0x8007139F)`(ERROR_INVALID_STATE)，
/// 没有任何 UI 提示，桌面表现就是「宠物开关打开了但什么都不出现」。
///
/// ⚠️ 历史教训二（0x80000003）：曾经为了对抗「后台节流」给两个窗口统一加了
/// `--disable-backgrounding-occluded-windows --disable-renderer-backgrounding
/// --disable-background-timer-throttling` 等参数，结果 WebView2 的 **browser 进程**
/// 启动数秒后在 msedge.dll 内 CHECK 失败（int3，Crashpad dump 落在
/// `%LOCALAPPDATA%\com.jianluo.pulsedeck\EBWebView\Crashpad\reports\`），
/// 渲染器被整体带走——表现出来的「定时器只走一拍就冻结」恰恰是这次崩溃的
/// 症状，而当初加参数想治的「节流」根本不是根因。**不要轻易再加命令行参数。**
///
/// 如果未来确实需要附加参数：主窗口（`tauri.conf.json` 的 `additionalBrowserArgs`）
/// 与宠物窗口（本常量）必须逐字符一致，由 `browser_args_match_conf` 单测卡住。
///
/// 非 Windows 上 `additional_browser_args` 不是 tauri 的 API，本常量只有测试在用。
#[cfg_attr(not(windows), allow(dead_code))]
pub const BROWSER_ARGS: &str = "";

/* ------------------------------ 配置 ------------------------------ */

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PetConfig {
    /// 是否显示宠物窗口
    pub enabled: bool,
    /// 要显示的指标（顺序即展示顺序），取值见 `METRIC_KEYS`
    pub metrics: Vec<String>,
    /// 整体不透明度 0.3 ~ 1.0
    pub opacity: f32,
    /// 形象模式：`cat`（打字猫）/ `classic`（6 档贴图）
    #[serde(default = "default_mode")]
    pub mode: String,
    /// 打字猫渲染引擎：`2d` / `3d`（鲸鱼娘强制 2d）
    #[serde(default = "default_render_mode")]
    pub render_mode: String,
    /// 皮肤 id，取值见 `SKIN_IDS`
    #[serde(default = "default_skin")]
    pub skin: String,
    /// 键盘布局：`65` / `108`
    #[serde(default = "default_layout")]
    pub layout: String,
    /// 键盘视角：`cat`（面向猫咪）/ `viewer`（面向观众）
    #[serde(default = "default_orientation")]
    pub orientation: String,
    /// 打字猫尺寸倍率（0.6 ~ 1.5）
    #[serde(default = "default_scale")]
    pub scale: f64,
    /// 摸头呼噜声开关
    #[serde(default = "default_true")]
    pub sound: bool,
    /// 锁定鼠标穿透（按住 Alt 临时解锁）
    #[serde(default)]
    pub click_through: bool,
    /// 密码框防窥开关（Windows）
    #[serde(default = "default_true")]
    pub privacy_guard: bool,
    /// 3D 项圈颜色，取值见 `COLLAR_IDS`
    #[serde(default = "default_collar")]
    pub collar: String,
    /// 3D 耳机：`off` / `on`
    #[serde(default = "default_headphones")]
    pub headphones: String,
    /// 窗口左上角，**逻辑像素**（与 `WebviewWindowBuilder::position` 同一套坐标）；
    /// `None` = 用户没拖过，用默认落点（主显示器右下角）。
    ///
    /// 位置由**拖动**产生（`WindowEvent::Moved` → [`PetStore::set_position`]），
    /// 不由设置页决定——前端提交的配置里根本没有这个字段，所以：
    /// * `#[serde(default)]` 是必需的，否则前端一提交就反序列化失败；
    /// * 老版本的 `pet.json` 也没有这个字段，缺了它**必须**还能读出来
    ///   （读不出来就退回默认配置 = 宠物被静默关掉，见 `old_config_without_position_still_parses`）；
    /// * [`PetStore::set`] 会用内存里那份覆盖前端传来的值，免得设置页一保存就清空位置。
    #[serde(default)]
    pub position: Option<[f64; 2]>,
}

fn default_mode() -> String {
    "cat".to_string()
}
fn default_render_mode() -> String {
    "2d".to_string()
}
fn default_skin() -> String {
    "black".to_string()
}
fn default_layout() -> String {
    "65".to_string()
}
fn default_orientation() -> String {
    "cat".to_string()
}
fn default_scale() -> f64 {
    1.0
}
fn default_true() -> bool {
    true
}
fn default_collar() -> String {
    "none".to_string()
}
fn default_headphones() -> String {
    "off".to_string()
}

impl Default for PetConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            metrics: METRIC_KEYS.iter().map(|s| s.to_string()).collect(),
            opacity: 1.0,
            position: None,
            mode: default_mode(),
            render_mode: default_render_mode(),
            skin: default_skin(),
            layout: default_layout(),
            orientation: default_orientation(),
            scale: default_scale(),
            sound: true,
            click_through: false,
            privacy_guard: true,
            collar: default_collar(),
            headphones: default_headphones(),
        }
    }
}

impl PetConfig {
    /// 清洗外部传入的配置：过滤非法指标、去重保序、兜底至少一项、透明度夹取到合法区间
    fn sanitize(&mut self) {
        let mut seen: Vec<String> = Vec::new();
        for m in std::mem::take(&mut self.metrics) {
            if METRIC_KEYS.contains(&m.as_str()) && !seen.contains(&m) {
                seen.push(m);
            }
        }
        if seen.is_empty() {
            seen.push(METRIC_KEYS[0].to_string());
        }
        self.metrics = seen;

        if !self.opacity.is_finite() {
            self.opacity = 1.0;
        }
        self.opacity = self.opacity.clamp(0.3, 1.0);

        // 位置必须是有限数：NaN / inf 会让建窗定位算出乱七八糟的坐标
        if let Some([x, y]) = self.position {
            if !x.is_finite() || !y.is_finite() {
                self.position = None;
            }
        }

        if self.mode != "classic" {
            self.mode = "cat".to_string();
        }
        if !SKIN_IDS.contains(&self.skin.as_str()) {
            self.skin = default_skin();
        }
        if !LAYOUT_IDS.contains(&self.layout.as_str()) {
            self.layout = default_layout();
        }
        if !ORIENTATION_IDS.contains(&self.orientation.as_str()) {
            self.orientation = default_orientation();
        }
        if !COLLAR_IDS.contains(&self.collar.as_str()) {
            self.collar = default_collar();
        }
        if !HEADPHONE_IDS.contains(&self.headphones.as_str()) {
            self.headphones = default_headphones();
        }
        // 鲸鱼娘只有 2D 渲染器；3D 在小尺寸下也看不清，未知值一律回到 2D
        if self.skin == "dafeiyu" || self.render_mode != "3d" {
            self.render_mode = "2d".to_string();
        }
        if !self.scale.is_finite() {
            self.scale = default_scale();
        }
        self.scale = self.scale.clamp(0.2, 1.5);
    }
}

/// 合成真正生效的配置：「前端提交的那份」+「已保存的位置」。
///
/// 位置归拖动所有（见 [`PetStore::set_position`]），而前端只提交
/// `enabled` / `metrics` / `opacity` 三个字段——但整份配置会经 localStorage
/// 转一圈再回来（`main.js` 把返回值原样存下来，启动时又推给后端），
/// 所以这里必须丢掉前端那份位置，否则设置页每保存一次（哪怕只是拉一下不透明度）
/// 位置就被清空，宠物下次从默认落点冒出来。
fn adopt_position(mut submitted: PetConfig, stored: Option<[f64; 2]>) -> PetConfig {
    submitted.position = stored;
    submitted
}

/// 配置的共享句柄（主线程与命令线程共用一份）
#[derive(Clone)]
pub struct PetStore {
    config: Arc<Mutex<PetConfig>>,
    /// 位置改过但还没落盘（`Moved` 事件只改内存，见 `set_position`）
    position_dirty: Arc<AtomicBool>,
    /// 串行化写盘。`save()` 现在会被三个线程调到：设置页命令（tokio 线程池）、
    /// 宠物页心跳（每秒一次）、窗口销毁事件（主线程）。它们共用同一个
    /// `pet.json.tmp`，并发时会出现「A 还在写、B 把它 rename 走」——
    /// 于是配置里落进半个 JSON，下次启动解析失败、静默退回默认值。
    save_lock: Arc<Mutex<()>>,
}

/// 排障日志：只有设了 `PULSEDECK_PET_TRACE` 才落盘，正常使用时零开销。
///
/// 宠物窗口是「无边框 + 跳过任务栏」的，一旦没建出来，桌面上什么都不显示，
/// 肉眼完全分不清「配置没读到」还是「建窗失败」还是「页面没跑起来」。
/// 这条链路上的每一步都记一笔 `pet.log`，出问题时先看它。
pub(crate) fn plog(msg: impl AsRef<str>) {
    if std::env::var_os("PULSEDECK_PET_TRACE").is_none() {
        return;
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(crate::app_dir().join("pet.log"))
    {
        use std::io::Write;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let _ = writeln!(f, "[{ts}] {}", msg.as_ref());
    }
}

impl PetStore {
    /// 从磁盘恢复；文件不存在或损坏时退回默认配置（宠物默认关闭，不做任何打扰）
    pub fn load() -> Self {
        let path = config_path();
        let parsed = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<PetConfig>(&s).ok());
        let mut cfg = parsed.clone().unwrap_or_default();
        cfg.sanitize();
        plog(format!(
            "load path={} parsed={} -> enabled={} metrics={:?} position={:?}",
            path.display(),
            parsed.is_some(),
            cfg.enabled,
            cfg.metrics,
            cfg.position
        ));
        Self {
            config: Arc::new(Mutex::new(cfg)),
            position_dirty: Arc::new(AtomicBool::new(false)),
            save_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn get(&self) -> PetConfig {
        self.lock().clone()
    }

    /// 落盘并返回清洗后的配置（前端回显以这份为准）
    pub fn set(&self, mut config: PetConfig) -> PetConfig {
        config.sanitize();
        let mut guard = self.lock();
        let merged = adopt_position(config, guard.position);
        *guard = merged.clone();
        drop(guard);
        self.save(&merged);
        merged
    }

    /// 记录窗口当前位置（**只改内存，不落盘**）。
    ///
    /// 拖动时 Windows 连续发几十上百个 `WM_MOVE`，每个都写一次盘既无谓、
    /// 也可能在模态拖动循环里拖慢响应。落盘交给两个时机：
    /// * [`flush_position`]：宠物页每秒一次的心跳顺带写盘（拖动停止后 1s 内必定落盘），
    ///   以及宠物窗口被销毁时（刚拖好就关掉的情形）；
    /// * [`set`]：设置页保存配置时本来就要写盘。
    ///
    /// 覆盖不到的只剩「拖完直接杀进程」——位置是纯偏好，不值得为它加常驻定时器。
    pub fn set_position(&self, position: [f64; 2]) {
        if !position[0].is_finite() || !position[1].is_finite() {
            return;
        }
        let mut guard = self.lock();
        if guard.position == Some(position) {
            return;
        }
        guard.position = Some(position);
        self.position_dirty.store(true, Ordering::Relaxed);
    }

    /// 把待落盘的位置写进 `pet.json`；没有变化时直接返回（心跳每秒钟都会调它）。
    pub fn flush_position(&self) {
        if !self.position_dirty.swap(false, Ordering::Relaxed) {
            return;
        }
        let config = self.get();
        self.save(&config);
        plog(format!("flush_position 落盘 position={:?}", config.position));
    }

    fn save(&self, config: &PetConfig) {
        // 同一份 tmp 只能有一个写者（见 `save_lock` 的注释）。
        // 锁中毒不该把应用带崩，所以直接用 `into_inner` 忽略中毒。
        let _serialize_writes = self.save_lock.lock().unwrap_or_else(|e| e.into_inner());
        let path = config_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // 原子写：临时文件 + rename，避免写到一半被关机打断留下坏配置
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
    fn lock(&self) -> MutexGuard<'_, PetConfig> {
        self.config.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn config_path() -> std::path::PathBuf {
    crate::app_dir().join(FILE_NAME)
}

/* ------------------------------ 窗口尺寸 ------------------------------ */

/// 打字猫画布的逻辑基准尺寸（随键盘布局变化）。
///
/// 与 KuroCat 的 `base_dimensions` 对齐：65% 键 420×440、108 键 540×456。
/// 窗口 = 画布 × scale + 固定高度指标条（见 [`window_size`]）。
pub fn base_canvas_size(layout: &str) -> (f64, f64) {
    if layout == "108" {
        (540.0, 456.0)
    } else {
        (420.0, 440.0)
    }
}

/// 按配置算出窗口的逻辑尺寸。
///
/// 经典贴图保持 190×224；打字猫把指标条算进高度，宽度只随画布缩放。
pub fn window_size(config: &PetConfig) -> (f64, f64) {
    if config.mode == "classic" {
        return (WIN_W, WIN_H);
    }
    let (bw, bh) = base_canvas_size(&config.layout);
    let scale = config.scale.clamp(0.2, 1.5);
    (
        (bw * scale).round().max(1.0),
        (bh * scale).round() + CAT_METRICS_BAR_H,
    )
}

/// 窗口尺寸变化时保持右下角锚定，并把窗口夹回当前显示器工作区。
///
/// 尺寸没变时什么都不做（`set_pet_config` 每次都会走到这里）。
fn apply_window_geometry(win: &tauri::WebviewWindow, w: f64, h: f64) {
    let scale = win.scale_factor().unwrap_or(1.0).max(0.1);
    let cur = win.inner_size().ok();
    if let Some(cur) = cur {
        if (cur.width as f64 / scale - w).abs() < 0.5
            && (cur.height as f64 / scale - h).abs() < 0.5
        {
            return;
        }
    }
    let old_pos = win.outer_position().ok();
    let old_size = win.outer_size().ok();
    plog(format!("apply_window_geometry -> {w}x{h}"));
    let _ = win.set_size(tauri::Size::Logical(tauri::LogicalSize::new(w, h)));
    let (Some(pos), Some(size)) = (old_pos, old_size) else {
        return;
    };
    let dx = size.width as f64 - w * scale;
    let dy = size.height as f64 - h * scale;
    let mut nx = pos.x as f64 + dx;
    let mut ny = pos.y as f64 + dy;
    if let Ok(Some(monitor)) = win.current_monitor() {
        let area = monitor.work_area();
        let min_x = area.position.x as f64;
        let min_y = area.position.y as f64;
        let max_x = (min_x + area.size.width as f64 - w * scale).max(min_x);
        let max_y = (min_y + area.size.height as f64 - h * scale).max(min_y);
        nx = nx.clamp(min_x, max_x);
        ny = ny.clamp(min_y, max_y);
    }
    let _ = win.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
        nx.round() as i32,
        ny.round() as i32,
    )));
}

/* ------------------------------ 窗口 ------------------------------ */

/// 按配置同步窗口的存在与否：开 → 没有就建；关 → 有就销毁
///
/// ⚠⚠ **绝不能在「同步 Tauri 命令」或事件回调里调用本函数**（Windows）。
///
/// `WebviewWindowBuilder::build()` 的官方 "Known issues" 写着：Windows 下它在同步命令和
/// 事件处理器里会**死锁**。机理：同步命令跑在**主线程的 WebView2 IPC 回调里**
/// （`WebMessageReceived`），此时 `build()` 会内联执行 `CreateCoreWebView2Controller`，
/// wry 用 `wait_with_pump()` 起一个嵌套 `GetMessage` 循环等 COM 完成回调；
/// 但线程还卡在 WebView2 的回调里，COM 完成回调投递不进来 → 永远等不到 →
/// **整个进程卡死**（界面无响应，连托盘「退出」都不生效，只能任务管理器杀进程）。
///
/// 所以调用方（`set_pet_config`）必须是 **async 命令**：命令跑在 tokio 线程上，
/// `build()` 只是把创建请求 post 给事件循环，由主线程在正常的消息循环上下文里建窗口。
/// 回归测试：`set_pet_config_is_async_command`（lib.rs 里改回同步会直接编译失败/测试红）。
pub fn sync_window(app: &tauri::AppHandle, config: &PetConfig) -> tauri::Result<()> {
    let exists = app.get_webview_window(WINDOW_LABEL).is_some();
    plog(format!(
        "sync_window enabled={} exists={}",
        config.enabled, exists
    ));
    if config.enabled {
        if !exists {
            create_window(app, config)?;
        } else if let Some(win) = app.get_webview_window(WINDOW_LABEL) {
            // 模式 / 键位 / 尺寸变化：只调整几何，不重建 WebView（重建会闪一下）
            let (w, h) = window_size(config);
            apply_window_geometry(&win, w, h);
        }
    } else if let Some(win) = app.get_webview_window(WINDOW_LABEL) {
        plog("sync_window destroy");
        win.destroy()?;
    }
    plog(format!(
        "sync_window done -> exists={}",
        app.get_webview_window(WINDOW_LABEL).is_some()
    ));
    // 全局键鼠钩子 / 密码防窥 / 鼠标穿透只在「宠物开启 + 打字猫」时运行；
    // 关闭宠物或切回经典贴图会在这里立即卸载钩子（不拦截任何输入）。
    crate::pet_input::sync(
        app,
        config.enabled,
        &config.mode,
        config.click_through,
        config.privacy_guard,
    );
    Ok(())
}

/* ------------------- 主窗口的 WebView2 环境（复用） ------------------- */

/// 主窗口的 `ICoreWebView2Environment`，**只在主线程取一次，之后一直复用**。
///
/// 为什么必须缓存：`with_webview` 只有在**主线程**上调用才会同步拿到环境——
/// `send_user_message` 在非主线程时只是把消息 post 给事件循环就返回了，回调根本还没跑
/// （见 tauri-runtime-wry 的 `send_user_message`）。而宠物窗口的创建/销毁**必须离开
/// 同步命令**（原因见 `sync_window` 上的注释），那一刻已经不在主线程上了，`with_webview`
/// 只会拿到 `None` → 宠物窗口退回「独立环境」→ 以 `HRESULT(0x8007139F)` 静默建不出来。
/// 所以趁启动阶段（`setup()`：主线程、且不在任何 WebView2 回调里）先把环境存下来。
#[cfg(windows)]
static MAIN_ENV: OnceLock<Option<EnvHolder>> = OnceLock::new();

#[cfg(windows)]
#[derive(Clone)]
struct EnvHolder(webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment);

// SAFETY: 只在主线程写入（setup 里 set 一次）、只在主线程读取（建宠物窗口时）。
// 持有的是一份强引用，保证环境对象在整个进程存活期内不被释放。
#[cfg(windows)]
unsafe impl Send for EnvHolder {}
#[cfg(windows)]
unsafe impl Sync for EnvHolder {}

/// 在**主线程**上取一次主窗口的 WebView2 环境并缓存下来。
///
/// 必须从 `setup()` 里调用（那时主窗口已建好，且不在任何 WebView2 回调内部）。
#[cfg(windows)]
pub fn capture_main_environment(app: &tauri::AppHandle) {
    let env = get_main_environment(app);
    plog(if env.is_some() {
        "capture_main_environment: 已缓存主窗口的 WebView2 环境"
    } else {
        "capture_main_environment: ⚠ 取不到主窗口环境，宠物窗口将退回独立环境（可能建不出来）"
    });
    let _ = MAIN_ENV.set(env.map(EnvHolder));
}

/// 主窗口的 WebView2 环境，供主窗口之外的窗口（桌面宠物 / 任务栏挂件）复用。
///
/// 每个「第二个 WebView」窗口都必须复用主窗口的环境（同一个浏览器进程、同一份用户数据目录），
/// 否则 WebView2 会以 `HRESULT(0x8007139F)` 静默建不出来——桌面表现就是
/// 「开关打开了但什么都不出现」，且没有任何提示。仅 Windows 有这层机制。
#[cfg(windows)]
pub fn shared_webview_environment(
    app: &tauri::AppHandle,
) -> Option<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment> {
    main_environment(app)
}

/// 取主窗口环境：优先用启动时的缓存；没缓存到就现场再取一次（此刻若在主线程上仍有效）
#[cfg(windows)]
fn main_environment(
    app: &tauri::AppHandle,
) -> Option<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment> {
    if let Some(slot) = MAIN_ENV.get() {
        return slot.as_ref().map(|h| h.0.clone());
    }
    let env = get_main_environment(app);
    if env.is_some() {
        let _ = MAIN_ENV.set(env.clone().map(EnvHolder));
    }
    env
}

/// 从主窗口获取 WebView2 环境（用于共享给宠物窗口）。
///
/// ⚠ **只有主线程调用才有返回值**：`with_webview` 走 `send_user_message`，
/// 主线程上同步执行回调，非主线程上只是 post 一条消息（返回时回调还没跑）。
///
/// 用 `with_webview` 在主线程同步拿环境。虽然回调要求 `Send`，
/// 但 `send_user_message` 在主线程是同步执行的（见 tauri-runtime-wry 的
/// `send_user_message`：当前线程 == 主线程时直接 `handle_user_message`），
/// 所以这里用 `unsafe impl Send` 绕过类型约束是安全的——回调根本不会跨线程。
#[cfg(windows)]
fn get_main_environment(
    app: &tauri::AppHandle,
) -> Option<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment> {
    use std::sync::{Arc, Mutex};
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment;

    struct EnvHolder(Option<ICoreWebView2Environment>);
    // SAFETY: 回调在主线程同步执行，不会跨线程移动 ICoreWebView2Environment。
    unsafe impl Send for EnvHolder {}

    let holder = Arc::new(Mutex::new(EnvHolder(None)));
    let holder_clone = Arc::clone(&holder);

    let main = app.get_webview_window("main")?;
    plog("get_main_environment: 主窗口存在，调用 with_webview");
    main.with_webview(move |wv| {
        plog("get_main_environment: with_webview 回调执行");
        let env = wv.environment();
        if let Ok(mut h) = holder_clone.lock() {
            h.0 = Some(env);
        }
    })
    .ok()?;

    let env = holder.lock().ok()?.0.take();
    env
}

fn create_window(app: &tauri::AppHandle, config: &PetConfig) -> tauri::Result<()> {
    let (w, h) = window_size(config);
    let (x, y) = restore_position(app, config);
    let url = WebviewUrl::App("pet.html".into());
    plog(format!("create_window url={url:?} size={w}x{h} at ({x},{y})"));

    let mut builder = WebviewWindowBuilder::new(app, WINDOW_LABEL, url)
        .title("桌面宠物")
        .inner_size(w, h)
        .position(x, y);

    // Windows 上多个 WebView 共享同一个 ICoreWebView2Environment（官方推荐做法：
    // 同一浏览器进程、同一数据目录，避免环境选项不一致之类的隐性冲突）。
    //
    // 实现细节：用 `with_webview` 从主窗口拿环境。虽然它的回调要求 `Send`，
    // 但 `send_user_message` 在主线程是同步执行的（见 tauri-runtime-wry 的
    // `send_user_message` 实现：当前线程 == 主线程时直接 `handle_user_message`），
    // 所以这里用 `unsafe impl Send` 绕过类型约束是安全的——回调根本不会跨线程。
    // 环境取**缓存**的那份，不要在这里现取：本函数可能不在主线程上跑
    // （见 sync_window 的注释），那时 `with_webview` 是拿不到环境的。
    //
    // Linux（webkit2gtk）/ macOS（WKWebView）没有这层环境共享机制，跳过整块逻辑。
    #[cfg(windows)]
    {
        let environment = main_environment(app);
        if environment.is_some() {
            plog("复用主窗口的 WebView2 环境");
        } else {
            plog("⚠ 没有主窗口环境，宠物窗口将使用独立环境（多半会以 0x8007139F 建不出来）");
        }
        if let Some(env) = environment {
            builder = builder.with_environment(env);
        }
    }

    builder = builder
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .always_on_top(true)
        .skip_taskbar(true);

    // 附加参数必须与主窗口一致（原因见 BROWSER_ARGS 的注释）；当前 BROWSER_ARGS 为空，
    // 空串就不下发（避免给 WebView2 传一个空参数）。
    #[cfg(windows)]
    if !BROWSER_ARGS.is_empty() {
        builder = builder.additional_browser_args(BROWSER_ARGS);
    }

    let built = builder
        // 直接建成可见窗口：WebView2 在「隐藏窗口」里创建时，出现过整窗空白不渲染的情况，
        // 而宠物页面自带深色卡片底（见 pet.css），不存在白底闪一下的问题。
        .visible(true)
        .build();
    match built {
        Ok(_) => {
            plog("create_window ok");
            Ok(())
        }
        Err(e) => {
            plog(format!("create_window FAILED: {e}"));
            Err(e)
        }
    }
}

/// 默认落点：主显示器工作区右下角（避开任务栏）
fn default_position(app: &tauri::AppHandle, config: &PetConfig) -> (f64, f64) {
    let (win_w, win_h) = window_size(config);
    let mut x = 160.0;
    let mut y = 160.0;
    if let Ok(Some(m)) = app.primary_monitor() {
        let scale = m.scale_factor().max(0.1);
        let area = m.work_area();
        let w = area.size.width as f64 / scale;
        let h = area.size.height as f64 / scale;
        if w > win_w && h > win_h {
            x = area.position.x as f64 / scale + w - win_w - 24.0;
            y = area.position.y as f64 / scale + h - win_h - 12.0;
        }
    }
    (x.max(0.0), y.max(0.0))
}

/// 建窗落点：优先用上次拖动保存的位置，其次才是默认落点。
///
/// 保存的位置已经不在任何显示器上时（拔掉外接屏、改了显示器布局）回退默认落点——
/// 否则宠物会落在看不见的地方，而且「关掉再打开」只会复用同一个坏位置，
/// 用户没有任何自救手段（窗口无边框、不进任务栏，看不见就是彻底看不见）。
fn restore_position(app: &tauri::AppHandle, config: &PetConfig) -> (f64, f64) {
    if let Some([x, y]) = config.position {
        if position_on_some_monitor(app, x, y) {
            let (w, h) = window_size(config);
            if let Some((left, top, right, bottom)) = monitor_work_area_containing(app, x, y) {
                let (cx, cy) = clamp_position_to_rect(x, y, w, h, left, top, right, bottom);
                plog(format!("恢复保存的位置 ({x}, {y}) -> 夹取为 ({cx}, {cy})"));
                return (cx, cy);
            }
            plog(format!("恢复保存的位置 ({x}, {y})"));
            return (x, y);
        }
        plog(format!("保存的位置 ({x}, {y}) 不在任何显示器上，回退默认落点"));
    }
    default_position(app, config)
}

/// 逻辑坐标点是否落在某个显示器范围内。
///
/// 把窗口左上角夹进一块矩形（全部逻辑像素）。
///
/// 升级自旧版的 `pet.json` 时，窗口可能从 190×224 变成 420×484：旧位置贴在屏幕右下角，
/// 直接照搬会让窗口下半截/右边超出工作区。这里把窗口整体夹回可见范围。
fn clamp_position_to_rect(
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
) -> (f64, f64) {
    let max_x = (right - w).max(left);
    let max_y = (bottom - h).max(top);
    (x.clamp(left, max_x), y.clamp(top, max_y))
}

/// 包含逻辑坐标点 `(x,y)` 的显示器工作区（逻辑像素）
fn monitor_work_area_containing(
    app: &tauri::AppHandle,
    x: f64,
    y: f64,
) -> Option<(f64, f64, f64, f64)> {
    let monitors = app.available_monitors().ok()?;
    for m in monitors.iter() {
        let scale = m.scale_factor().max(0.1);
        let area = m.work_area();
        let left = area.position.x as f64 / scale;
        let top = area.position.y as f64 / scale;
        let right = left + area.size.width as f64 / scale;
        let bottom = top + area.size.height as f64 / scale;
        if x >= left && x < right && y >= top && y < bottom {
            return Some((left, top, right, bottom));
        }
    }
    None
}

/// 显示器给的是**物理像素**（各屏 `scale_factor` 还可能不同），而保存的位置是逻辑像素，
/// 所以按每个显示器自己的缩放换算后再比。查不到显示器列表（极少见）时返回 true——
/// 那属于异常路径，宁可相信保存的位置，也不要无故把宠物挪回默认角落。
fn position_on_some_monitor(app: &tauri::AppHandle, x: f64, y: f64) -> bool {
    let Ok(monitors) = app.available_monitors() else {
        return true;
    };
    monitors.iter().any(|m| {
        let scale = m.scale_factor().max(0.1);
        let pos = m.position();
        let size = m.size();
        let left = pos.x as f64 / scale;
        let top = pos.y as f64 / scale;
        let right = left + size.width as f64 / scale;
        let bottom = top + size.height as f64 / scale;
        x >= left && x < right && y >= top && y < bottom
    })
}

/* ------------------------------ 心跳 ------------------------------ */

/// 宠物页面最后一次心跳的时刻（Unix 毫秒）。
///
/// 宠物窗口是「透明无边框」的，一旦 WebView 没渲染出来，界面上表现就是「什么都没有」，
/// 用户与开发者都看不出是没开、还是开了但没画出来。所以让页面每秒报一次心跳：
/// * 设置页可以显示「运行中 / 未响应」，用户自己就能判断；
/// * 排查时把 `PULSEDECK_PET_TRACE=1` 打开，心跳会落盘到 `%LOCALAPPDATA%\PulseDeck\pet.trace`，
///   直接回答「页面到底跑起来没有」。
static LAST_BEAT_MS: AtomicU64 = AtomicU64::new(0);

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn record_beat(app: &tauri::AppHandle) {
    let ts = now_ms();
    LAST_BEAT_MS.store(ts, Ordering::Relaxed);
    if std::env::var_os("PULSEDECK_PET_TRACE").is_some() {
        // 覆盖写：每次只留最新一行，避免长时间运行把文件撑大
        let _ = std::fs::write(
            crate::app_dir().join("pet.trace"),
            format!(
                "beat ts_ms={ts} window_open={}\n",
                app.get_webview_window(WINDOW_LABEL).is_some()
            ),
        );
        plog("beat");
    }
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

/// 宠物专用的轻量读数（只含界面真正要显示的几个数）
#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct PetMetrics {
    pub cpu_pct: f32,
    pub mem_pct: f32,
    /// GPU 占用；无 GPU / 驱动没上报时为 None
    pub gpu_pct: Option<f32>,
    pub has_gpu: bool,
    /// CPU 温度（°C）；读不到时为 None —— 此时颜色退回按 CPU 占用着色
    pub cpu_temp_c: Option<f32>,
    /// 颜色来源：`temp` = CPU 温度；`usage` = 温度不可用，退回 CPU 占用
    pub color_source: String,
}

pub fn metrics_from(s: &Snapshot) -> PetMetrics {
    let cpu_temp_c = s.thermal.cpu_temp_c.filter(|v| v.is_finite());
    PetMetrics {
        cpu_pct: s.cpu.usage_total,
        mem_pct: s.memory.usage_pct,
        gpu_pct: s.gpu.usage_pct.filter(|v| v.is_finite()),
        has_gpu: s.gpu.available,
        cpu_temp_c,
        color_source: if cpu_temp_c.is_some() {
            "temp".to_string()
        } else {
            "usage".to_string()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_drops_unknown_and_dedups() {
        let mut cfg = PetConfig {
            enabled: true,
            metrics: vec!["gpu".into(), "cpu".into(), "gpu".into(), "disk".into()],
            opacity: 5.0,
            ..PetConfig::default()
        };
        cfg.sanitize();
        assert_eq!(cfg.metrics, vec!["gpu".to_string(), "cpu".to_string()]);
        assert_eq!(cfg.opacity, 1.0);
    }

    #[test]
    fn sanitize_keeps_at_least_one_metric() {
        let mut cfg = PetConfig {
            enabled: true,
            metrics: vec!["nope".into()],
            opacity: 0.1,
            ..PetConfig::default()
        };
        cfg.sanitize();
        assert_eq!(cfg.metrics, vec!["cpu".to_string()]);
        assert_eq!(cfg.opacity, 0.3);
    }

    /// 老版本的 `pet.json` 里没有 `position` 字段，**必须**还能反序列化。
    ///
    /// 缺 `#[serde(default)]` 时 `from_str` 直接失败，而 `PetStore::load` 对解析失败
    /// 的处理是「退回默认配置」= 宠物被静默关掉、指标回默认——
    /// 也就是说升级一次就把所有人的宠物设置清空，且没有任何提示。
    #[test]
    fn old_config_without_position_still_parses() {
        let cfg: PetConfig =
            serde_json::from_str(r#"{"enabled":true,"metrics":["gpu"],"opacity":0.8}"#)
                .expect("缺少 position 字段的老配置必须能解析");
        assert!(cfg.enabled);
        assert_eq!(cfg.metrics, vec!["gpu".to_string()]);
        assert_eq!(cfg.position, None);
    }

    /// 位置由拖动产生、不由设置页决定：前端提交（无 position）不能把已保存的位置清掉。
    #[test]
    fn submitted_config_does_not_clear_saved_position() {
        let mut submitted = PetConfig {
            enabled: true,
            metrics: vec!["cpu".into()],
            opacity: 0.9,
            ..PetConfig::default()
        };
        submitted.sanitize();
        let merged = adopt_position(submitted, Some([1234.0, 567.0]));
        assert_eq!(merged.position, Some([1234.0, 567.0]));
        assert_eq!(merged.opacity, 0.9);
    }

    /// NaN / inf 位置要被清洗掉，合法位置原样保留
    #[test]
    fn sanitize_drops_non_finite_position() {
        let base = PetConfig {
            enabled: true,
            metrics: vec!["cpu".into()],
            opacity: 1.0,
            ..PetConfig::default()
        };

        let mut nan = PetConfig {
            position: Some([f64::NAN, 10.0]),
            ..base.clone()
        };
        nan.sanitize();
        assert_eq!(nan.position, None, "NaN 位置必须被清掉");

        let mut inf = PetConfig {
            position: Some([10.0, f64::NEG_INFINITY]),
            ..base.clone()
        };
        inf.sanitize();
        assert_eq!(inf.position, None, "inf 位置必须被清掉");

        let mut ok = PetConfig {
            position: Some([12.0, 34.0]),
            ..base
        };
        ok.sanitize();
        assert_eq!(ok.position, Some([12.0, 34.0]));
    }

    /// 温度缺失时颜色来源应切到 CPU 占用，前端据此给不同的说明文案
    #[test]
    fn color_source_falls_back_to_usage() {
        let mut s = Snapshot::default();
        s.cpu.usage_total = 42.0;
        s.thermal.cpu_temp_c = None;
        let m = metrics_from(&s);
        assert_eq!(m.color_source, "usage");
        assert_eq!(m.cpu_temp_c, None);

        s.thermal.cpu_temp_c = Some(61.5);
        let m = metrics_from(&s);
        assert_eq!(m.color_source, "temp");
        assert_eq!(m.cpu_temp_c, Some(61.5));
    }

    /// 旧位置在屏幕右下角、窗口变大后要整体夹回工作区，不能只校验左上角
    #[test]
    fn clamp_position_keeps_window_inside_work_area() {
        // 1920x1080 工作区，窗口 420x484，旧位置 (1600, 800) 会超出右下角
        let (x, y) = clamp_position_to_rect(1600.0, 800.0, 420.0, 484.0, 0.0, 0.0, 1920.0, 1080.0);
        assert_eq!(x, 1500.0);
        assert_eq!(y, 596.0);
        // 窗口比工作区还大时贴左上角，不出现负坐标
        let (x, y) = clamp_position_to_rect(100.0, 100.0, 3000.0, 2000.0, -100.0, 0.0, 1920.0, 1080.0);
        assert_eq!((x, y), (-100.0, 0.0));
    }

    /// 新字段默认值：旧配置缺字段时落到打字猫 2D + 黑猫，而不是关闭宠物
    #[test]
    fn new_fields_have_safe_defaults() {
        let cfg: PetConfig =
            serde_json::from_str(r#"{"enabled":true,"metrics":["cpu"],"opacity":1.0}"#)
                .expect("旧配置必须能解析");
        assert_eq!(cfg.mode, "cat");
        assert_eq!(cfg.render_mode, "2d");
        assert_eq!(cfg.skin, "black");
        assert_eq!(cfg.layout, "65");
        assert_eq!(cfg.orientation, "cat");
        assert!((cfg.scale - 1.0).abs() < 1e-9);
        assert!(cfg.sound);
        assert!(!cfg.click_through);
        assert!(cfg.privacy_guard);
        assert_eq!(cfg.collar, "none");
        assert_eq!(cfg.headphones, "off");
    }

    /// 非法新字段要被清洗：未知皮肤/键位/视角/配饰回落白名单首项，
    /// 鲸鱼娘强制 2D，比例夹到 0.2~1.5
    #[test]
    fn sanitize_normalizes_new_fields() {
        let mut cfg = PetConfig {
            mode: "weird".into(),
            render_mode: "3d".into(),
            skin: "dafeiyu".into(),
            layout: "999".into(),
            orientation: "sideways".into(),
            scale: 9.0,
            collar: "gold".into(),
            headphones: "auto".into(),
            ..PetConfig::default()
        };
        cfg.sanitize();
        assert_eq!(cfg.mode, "cat");
        assert_eq!(cfg.render_mode, "2d", "鲸鱼娘只有 2D 渲染器");
        assert_eq!(cfg.skin, "dafeiyu");
        assert_eq!(cfg.layout, "65");
        assert_eq!(cfg.orientation, "cat");
        assert!((cfg.scale - 1.5).abs() < 1e-9);
        assert_eq!(cfg.collar, "none");
        assert_eq!(cfg.headphones, "off");

        let mut tiny = PetConfig {
            scale: 0.01,
            ..PetConfig::default()
        };
        tiny.sanitize();
        assert!((tiny.scale - 0.2).abs() < 1e-9);
    }

    /// 窗口尺寸：经典 190×224；打字猫随键位与比例变化，并额外加指标条高度
    #[test]
    fn window_size_tracks_mode_layout_and_scale() {
        let classic = PetConfig {
            mode: "classic".into(),
            ..PetConfig::default()
        };
        assert_eq!(window_size(&classic), (190.0, 224.0));

        let cat = PetConfig::default();
        assert_eq!(window_size(&cat), (420.0, 440.0 + CAT_METRICS_BAR_H));

        let cat108 = PetConfig {
            layout: "108".into(),
            ..PetConfig::default()
        };
        assert_eq!(window_size(&cat108), (540.0, 456.0 + CAT_METRICS_BAR_H));

        let small = PetConfig {
            scale: 0.2,
            ..PetConfig::default()
        };
        assert_eq!(window_size(&small), (84.0, 88.0 + CAT_METRICS_BAR_H));
    }

    /// 鼠标穿透锁定依赖 `set_ignore_cursor_events`，capability 必须显式授权
    #[test]
    fn pet_capability_allows_ignore_cursor_events() {
        let cap = include_str!("../capabilities/default.json");
        assert!(
            cap.contains("\"core:window:allow-set-ignore-cursor-events\""),
            "capabilities/default.json 必须包含 core:window:allow-set-ignore-cursor-events，             否则「锁定鼠标穿透」会被 ACL 静默拒绝。"
        );
    }

    /// 宠物窗口的**拖动**必须显式授权，否则 Tauri 的 drag.js 调
    /// `plugin:window|start_dragging` 会被 ACL 静默拒绝——表现是
    /// 「鼠标按下去窗口纹丝不动」，而且 HTML 属性怎么写都没用（曾排查半天）。
    ///
    /// `core:window:default` 只含 getter，`start_dragging` 在 tauri 的 build.rs 里
    /// 标的是 `("start_dragging", false)`（非默认授权），必须由 capability 显式给出。
    #[test]
    fn pet_capability_allows_start_dragging() {
        let cap = include_str!("../capabilities/default.json");
        assert!(
            cap.contains("\"core:window:allow-start-dragging\""),
            "capabilities/default.json 必须包含 core:window:allow-start-dragging，\
             否则宠物窗口完全无法拖动（ACL 静默拒绝，UI 无任何提示）。"
        );
        assert!(
            cap.contains("\"pet\""),
            "capabilities/default.json 的 windows 列表必须含 pet，否则宠物窗口的 \
             invoke 全部被拒。"
        );
    }

    /// **`set_pet_config` 必须是 async 命令**：改回同步会在 Windows 上死锁。
    ///
    /// 同步命令跑在主线程的 WebView2 IPC 回调里，`WebviewWindowBuilder::build()`
    /// 在那里会永久等不到 `CreateCoreWebView2Controller` 的 COM 完成回调
    /// （机理见 `sync_window` 的注释）→ 进程卡死、退出都关不掉。
    ///
    /// 只卡「是不是 async」，不管其它：这是唯一能让「首次启用宠物卡死」复发的改动。
    #[test]
    fn set_pet_config_is_async_command() {
        let lib = include_str!("lib.rs");
        assert!(
            lib.contains("async fn set_pet_config"),
            "lib.rs 的 set_pet_config 必须是 async 命令。\
             同步命令 = 在主线程的 WebView2 IPC 回调里建窗口 = Windows 死锁\
             （整个进程卡住、退出关不掉，只能任务管理器杀进程）。"
        );
    }

    /// 启动时必须在**主线程**把主窗口的 WebView2 环境缓存下来，
    /// 否则宠物窗口会因为拿不到环境而退回独立环境（`0x8007139F`，静默建不出来）。
    #[test]
    fn setup_captures_main_environment_before_window_sync() {
        let lib = include_str!("lib.rs");
        let cap = lib
            .find("pet::capture_main_environment")
            .unwrap_or_else(|| panic!("setup() 必须调用 pet::capture_main_environment"));
        // 取**最后一次**出现：`set_pet_config` 里也有一处 `pet::sync_window`，
        // 而我们要卡的是 setup 里那一处（它在文件更靠后的位置）。
        let sync = lib
            .rfind("pet::sync_window")
            .unwrap_or_else(|| panic!("lib.rs 里找不到 pet::sync_window"));
        assert!(
            cap < sync,
            "capture_main_environment 必须在 sync_window 之前：\
             先缓存环境，再建宠物窗口（否则同步阶段现取不到环境）。"
        );
    }

    /// 主窗口与宠物窗口的 WebView2 附加参数必须一致（不一致 → 宠物窗口
    /// 以 HRESULT(0x8007139F) 静默建不出来，现象跟「功能没开」一模一样）。
    ///
    /// 两种合法状态：① conf 不写 `additionalBrowserArgs` 且 `BROWSER_ARGS` 为空（当前状态）；
    /// ② conf 里的值与 `BROWSER_ARGS` 逐字符相等。其余组合都是漂移，直接失败。
    #[test]
    fn browser_args_match_conf() {
        let conf = include_str!("../tauri.conf.json");
        let re_key = conf.contains("\"additionalBrowserArgs\"");
        if BROWSER_ARGS.is_empty() {
            assert!(
                !re_key,
                "BROWSER_ARGS 为空时 tauri.conf.json 不得配置 additionalBrowserArgs，\
                 否则宠物窗口会以 HRESULT(0x8007139F) 建不出来。"
            );
        } else {
            let needle = format!("\"additionalBrowserArgs\": \"{BROWSER_ARGS}\"");
            assert!(
                conf.contains(&needle),
                "tauri.conf.json 必须包含与 BROWSER_ARGS 完全一致的字符串：\n  {needle}\n\
                 不一致会导致宠物窗口以 HRESULT(0x8007139F) 建不出来。"
            );
        }
    }
}
