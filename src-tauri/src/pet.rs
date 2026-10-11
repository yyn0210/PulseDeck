//! 桌面宠物：一个置顶、无边框、透明的小窗口
//!
//! 设计要点：
//! * **窗口由 Rust 管**：设置页只负责改配置（`set_pet_config`），窗口的创建/销毁发生在这里，
//!   因此「最小化到托盘」时宠物依然在桌面上，也不需要前端持有窗口句柄。
//! * **配置落盘**：`%LOCALAPPDATA%\WinMonitor\pet.json`，下次启动自动恢复（不依赖主窗口先起来）。
//! * **读数走专用命令**：宠物窗口只需要 3~4 个数，不值得每拍搬一整份 `Snapshot`
//!   （里面还有进程榜与磁盘列表），所以单独给一个 `pet_metrics`。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

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
/// `%LOCALAPPDATA%\com.jianluo.winmonitor\EBWebView\Crashpad\reports\`），
/// 渲染器被整体带走——表现出来的「定时器只走一拍就冻结」恰恰是这次崩溃的
/// 症状，而当初加参数想治的「节流」根本不是根因。**不要轻易再加命令行参数。**
///
/// 如果未来确实需要附加参数：主窗口（`tauri.conf.json` 的 `additionalBrowserArgs`）
/// 与宠物窗口（本常量）必须逐字符一致，由 `browser_args_match_conf` 单测卡住。
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
}

impl Default for PetConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            metrics: METRIC_KEYS.iter().map(|s| s.to_string()).collect(),
            opacity: 1.0,
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
    }
}

/// 配置的共享句柄（主线程与命令线程共用一份）
#[derive(Clone)]
pub struct PetStore(Arc<Mutex<PetConfig>>);

/// 排障日志：只有设了 `WINMON_PET_TRACE` 才落盘，正常使用时零开销。
///
/// 宠物窗口是「无边框 + 跳过任务栏」的，一旦没建出来，桌面上什么都不显示，
/// 肉眼完全分不清「配置没读到」还是「建窗失败」还是「页面没跑起来」。
/// 这条链路上的每一步都记一笔 `pet.log`，出问题时先看它。
fn plog(msg: impl AsRef<str>) {
    if std::env::var_os("WINMON_PET_TRACE").is_none() {
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
            "load path={} parsed={} -> enabled={} metrics={:?}",
            path.display(),
            parsed.is_some(),
            cfg.enabled,
            cfg.metrics
        ));
        Self(Arc::new(Mutex::new(cfg)))
    }

    pub fn get(&self) -> PetConfig {
        self.lock().clone()
    }

    /// 落盘并返回清洗后的配置（前端回显以这份为准）
    pub fn set(&self, mut config: PetConfig) -> PetConfig {
        config.sanitize();
        *self.lock() = config.clone();
        self.save(&config);
        config
    }

    fn save(&self, config: &PetConfig) {
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
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn config_path() -> std::path::PathBuf {
    crate::app_dir().join(FILE_NAME)
}

/* ------------------------------ 窗口 ------------------------------ */

/// 按配置同步窗口的存在与否：开 → 没有就建；关 → 有就销毁
pub fn sync_window(app: &tauri::AppHandle, config: &PetConfig) -> tauri::Result<()> {
    let exists = app.get_webview_window(WINDOW_LABEL).is_some();
    plog(format!(
        "sync_window enabled={} exists={}",
        config.enabled, exists
    ));
    if config.enabled {
        if !exists {
            create_window(app)?;
        }
    } else if let Some(win) = app.get_webview_window(WINDOW_LABEL) {
        plog("sync_window destroy");
        win.destroy()?;
    }
    plog(format!(
        "sync_window done -> exists={}",
        app.get_webview_window(WINDOW_LABEL).is_some()
    ));
    Ok(())
}

/// 从主窗口获取 WebView2 环境（用于共享给宠物窗口）。
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

fn create_window(app: &tauri::AppHandle) -> tauri::Result<()> {
    let (x, y) = default_position(app);
    let url = WebviewUrl::App("pet.html".into());
    plog(format!("create_window url={url:?} size={WIN_W}x{WIN_H} at ({x},{y})"));

    // Windows 上多个 WebView 共享同一个 ICoreWebView2Environment（官方推荐做法：
    // 同一浏览器进程、同一数据目录，避免环境选项不一致之类的隐性冲突）。
    //
    // 实现细节：用 `with_webview` 从主窗口拿环境。虽然它的回调要求 `Send`，
    // 但 `send_user_message` 在主线程是同步执行的（见 tauri-runtime-wry 的
    // `send_user_message` 实现：当前线程 == 主线程时直接 `handle_user_message`），
    // 所以这里用 `unsafe impl Send` 绕过类型约束是安全的——回调根本不会跨线程。
    let environment = get_main_environment(app);
    if environment.is_some() {
        plog("复用主窗口的 WebView2 环境");
    } else {
        plog("⚠ 主窗口不存在，宠物窗口将使用独立环境");
    }

    let mut builder = WebviewWindowBuilder::new(app, WINDOW_LABEL, url)
        .title("桌面宠物")
        .inner_size(WIN_W, WIN_H)
        .position(x, y);
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
        .always_on_top(true)
        .skip_taskbar(true);
    // 附加参数必须与主窗口一致（原因见 BROWSER_ARGS 的注释）；当前 BROWSER_ARGS 为空，
    // 空串就不下发（避免给 WebView2 传一个空参数）。
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
fn default_position(app: &tauri::AppHandle) -> (f64, f64) {
    let mut x = 160.0;
    let mut y = 160.0;
    if let Ok(Some(m)) = app.primary_monitor() {
        let scale = m.scale_factor().max(0.1);
        let area = m.work_area();
        let w = area.size.width as f64 / scale;
        let h = area.size.height as f64 / scale;
        if w > WIN_W && h > WIN_H {
            x = area.position.x as f64 / scale + w - WIN_W - 24.0;
            y = area.position.y as f64 / scale + h - WIN_H - 12.0;
        }
    }
    (x.max(0.0), y.max(0.0))
}

/* ------------------------------ 心跳 ------------------------------ */

/// 宠物页面最后一次心跳的时刻（Unix 毫秒）。
///
/// 宠物窗口是「透明无边框」的，一旦 WebView 没渲染出来，界面上表现就是「什么都没有」，
/// 用户与开发者都看不出是没开、还是开了但没画出来。所以让页面每秒报一次心跳：
/// * 设置页可以显示「运行中 / 未响应」，用户自己就能判断；
/// * 排查时把 `WINMON_PET_TRACE=1` 打开，心跳会落盘到 `%LOCALAPPDATA%\WinMonitor\pet.trace`，
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
    if std::env::var_os("WINMON_PET_TRACE").is_some() {
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
        };
        cfg.sanitize();
        assert_eq!(cfg.metrics, vec!["cpu".to_string()]);
        assert_eq!(cfg.opacity, 0.3);
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
