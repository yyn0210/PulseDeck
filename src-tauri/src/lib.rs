//! 应用入口：后台采样线程 + Tauri 命令 + 托盘常驻

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/* ---------------- 模块分区 ----------------
 *
 * 跨平台公共层：sampler（采样编排）、platform（OS 绑定通道的 trait 与分派）、
 * battery（电池读数纯加工）、model、fan_pref、pet / taskbar（两个挂件窗口在
 * 三平台上都是 tauri 通用 API，只有细节处的平台差异加了 cfg）。
 * ⚠️ 公共层不得直接 use 下面的 Windows 模块，OS 绑定的读取一律走 platform
 * 的 trait（见 platform/mod.rs 的「硬性约束」）——这是 cfg 分区能成立的边界。
 *
 * Windows 专属：采样通道的具体实现（gpu/thermal/fan/vendor/win32/wmi/pdh/cpu_extras）、
 * PawnIO 提权链（pawnio/pawnio_setup/msr_bridge）。
 */

#[cfg(windows)]
mod gpu;
#[cfg(windows)]
mod gpu_dxgi;
#[cfg(windows)]
mod thermal;
#[cfg(windows)]
mod fan;
#[cfg(windows)]
mod vendor;
#[cfg(windows)]
mod win32;
#[cfg(windows)]
mod wmi;
#[cfg(windows)]
mod pdh;
/// CPU 的**实时**主频（PDH `Processor Information\Actual Frequency`）
#[cfg(windows)]
mod cpu_extras;
#[cfg(windows)]
mod pawnio;
#[cfg(windows)]
mod pawnio_setup;
#[cfg(windows)]
mod msr_bridge;

/// 电池纯加工层：Windows 与 Linux 采集源都在调用（`battery::from_raw`）；
/// macOS 等未适配平台暂时闲置，先允许。
#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
mod battery;
/// CPU 厂商识别（跨平台）：温度直读通道按厂商分流（Intel 走 MSR、AMD 走 SMN），
/// 判断只依赖 sysinfo 的品牌串，所以放在公共层、各平台都能编译与自测。
/// 目前只有 Windows 的提权桥接真的用它（Linux 侧 hwmon 不需要分厂商），先允许闲置。
#[cfg_attr(not(windows), allow(dead_code))]
mod cpu_vendor;
mod fan_pref;
mod model;
mod pet;
mod platform;
mod push;
mod sampler;
mod taskbar;

use model::Snapshot;
use tauri::menu::{CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, State};

pub struct AppState {
    sampler: Arc<sampler::Sampler>,
    paused: Arc<AtomicBool>,
    interval_ms: Arc<AtomicU32>,
    /// 关闭主窗口时是「隐藏到托盘」还是直接退出
    close_to_tray: Arc<AtomicBool>,
    /// 本会话是否已尝试拉起「MSR 温度桥接」进程（防止反复弹 UAC，Windows 专属）
    #[cfg(windows)]
    bridge_prompted: Arc<AtomicBool>,
    /// 桌面宠物的配置（窗口是否存在的依据，落盘在 pet.json）
    pet: pet::PetStore,
    /// 任务栏挂件的配置（窗口是否存在的依据，落盘在 taskbar.json）
    taskbar: taskbar::TaskbarStore,
    /// 风扇的用户手动指定（落盘在 fan.json）。**必须与采样线程共用同一份**，
    /// 否则设置页改了而采样读的是启动时那份。
    fan_pref: fan_pref::FanPrefStore,
    /// 数据推送的配置（落盘在 push.json）
    push: push::PushStore,
    /// 数据推送的运行时（TCP 监听 / BLE 广播的 worker 生命周期）
    push_hub: Arc<push::PushHub>,
}

#[tauri::command]
fn get_snapshot(state: State<'_, AppState>) -> Snapshot {
    state.sampler.latest()
}

#[tauri::command]
fn set_paused(state: State<'_, AppState>, paused: bool) -> bool {
    state.paused.store(paused, Ordering::Relaxed);
    paused
}

#[tauri::command]
fn set_interval(state: State<'_, AppState>, millis: u32) -> u32 {
    let ms = millis.clamp(300, 10_000);
    state.interval_ms.store(ms, Ordering::Relaxed);
    ms
}

#[tauri::command]
fn set_close_to_tray(state: State<'_, AppState>, enabled: bool) -> bool {
    state.close_to_tray.store(enabled, Ordering::Relaxed);
    enabled
}

#[tauri::command]
fn get_runtime_config(state: State<'_, AppState>) -> serde_json::Value {
    serde_json::json!({
        "paused": state.paused.load(Ordering::Relaxed),
        "intervalMs": state.interval_ms.load(Ordering::Relaxed),
        "closeToTray": state.close_to_tray.load(Ordering::Relaxed),
    })
}

/* ---------------- 桌面宠物（Windows 专属挂件） ---------------- */

/// 当前宠物配置（前端设置页据此回显）
#[tauri::command]
fn get_pet_config(state: State<'_, AppState>) -> pet::PetConfig {
    state.pet.get()
}

/// 保存宠物配置并同步窗口（开就建、关就销毁）。
///
/// 返回清洗后的配置——非法指标会被剔除、至少保留一项、透明度夹到 0.3~1.0，
/// 前端直接用返回值回显，避免界面与真实状态不一致。
///
/// ⚠⚠ **`async` 是硬性要求，不要改回同步命令**（2026-10-12 修的「启用宠物后整个进程
/// 卡死、退出都关不掉，只能任务管理器杀」就是它引起的）。
///
/// 同步命令跑在**主线程的 WebView2 IPC 回调里**，而 `WebviewWindowBuilder::build()`
/// 在 Windows 上会在这个上下文里**死锁**（官方 "Known issues" 明文写着）：它内联执行
/// `CreateCoreWebView2Controller`，wry 用 `wait_with_pump()` 起一个嵌套 `GetMessage`
/// 循环等 COM 完成回调，可线程正卡在 WebView2 的 `WebMessageReceived` 回调里，
/// 完成回调永远投递不进来。结果就是主线程永久阻塞：界面冻结、托盘「退出」也没人处理，
/// 只能任务管理器杀进程。
///
/// 改成 async 后命令跑在 tokio 线程上，`build()` 只把创建请求 post 给事件循环，
/// 由主线程在正常的消息循环上下文里建窗口；`create_window` 内部仍然会阻塞等待结果，
/// 所以失败照样能返回给前端。回归测试见 `pet::tests::set_pet_config_is_async_command`。
#[tauri::command]
async fn set_pet_config(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    config: pet::PetConfig,
) -> Result<pet::PetConfig, String> {
    let cfg = state.pet.set(config);
    pet::sync_window(&app, &cfg).map_err(|e| format!("打开/关闭桌面宠物失败：{e}"))?;
    Ok(cfg)
}

/// 宠物窗口专用读数：只给它会显示的几个数，不搬整份快照
#[tauri::command]
fn get_pet_metrics(state: State<'_, AppState>) -> pet::PetMetrics {
    pet::metrics_from(&state.sampler.latest())
}

/// 宠物页面每秒报一次心跳。
///
/// 用途见 `pet::LAST_BEAT_MS`：设置页据此显示「运行中 / 未响应」，
/// 排查「窗口在但没画出来」时可开 `PULSEDECK_PET_TRACE=1` 落盘确认。
///
/// 顺带把拖动出来的位置落盘：`WindowEvent::Moved` 只改内存（拖动时事件太密），
/// 而窗口位置没有「拖动结束」事件可用——这条现成的 1Hz 节拍正好当写盘时机，
/// 拖动停止后 1s 内必定写进 `pet.json`。
#[tauri::command]
fn pet_heartbeat(app: tauri::AppHandle, state: State<'_, AppState>) {
    pet::record_beat(&app);
    state.pet.flush_position();
}

/// 宠物窗口的就绪状态：是否开着、页面心跳是否新鲜
#[tauri::command]
fn pet_status(app: tauri::AppHandle) -> serde_json::Value {
    let open = app.get_webview_window(pet::WINDOW_LABEL).is_some();
    let age = pet::beat_age_ms();
    // 采样间隔最大 5s，宠物页面固定 1s 心跳，3s 内没消息就认为没在跑
    let alive = open && age.is_some_and(|ms| ms < 3000);
    serde_json::json!({
        "windowOpen": open,
        "heartbeatAgeMs": age,
        "alive": alive,
    })
}

/// 双击宠物时呼出主窗口（主窗口可能正藏在托盘里）
#[tauri::command]
fn show_main_window(app: tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/* ---------------- 任务栏显示（Windows 专属挂件） ---------------- */

/// 当前任务栏挂件配置（设置页据此回显）
#[tauri::command]
fn get_taskbar_config(state: State<'_, AppState>) -> taskbar::TaskbarConfig {
    state.taskbar.get()
}

/// 保存挂件配置并同步窗口（开就建、关就销毁）。
///
/// 返回清洗后的配置——非法指标会被剔除、至少保留一项，前端直接用返回值回显。
///
/// ⚠⚠ **`async` 是硬性要求，不要改回同步命令**：同步命令跑在主线程的 WebView2 IPC 回调里，
/// `WebviewWindowBuilder::build()` 在那里会死锁（机理见本文件 `set_pet_config` 的注释）。
#[tauri::command]
async fn set_taskbar_config(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    config: taskbar::TaskbarConfig,
) -> Result<taskbar::TaskbarConfig, String> {
    let cfg = state.taskbar.set(config);
    taskbar::sync_window(&app, &cfg).map_err(|e| format!("打开/关闭任务栏显示失败：{e}"))?;
    Ok(cfg)
}

/// 挂件每秒一问：读数 + 任务栏朝向 / 主题（顺带记心跳）。
///
/// 位置的跟随不靠这条命令——那是看护线程的活（窗口被全屏应用藏起来时页面会被节流）。
#[tauri::command]
fn taskbar_tick(app: tauri::AppHandle, state: State<'_, AppState>) -> taskbar::TaskbarTick {
    taskbar::tick(&app, &state.sampler.latest())
}

/// 挂件页面把量出来的内容尺寸回传，后端据此调整窗口大小与落点
#[tauri::command]
fn taskbar_fit(app: tauri::AppHandle, width: f64, height: f64) {
    let _ = taskbar::fit(&app, width, height);
}

/// 挂件窗口的就绪状态：是否开着、页面心跳是否新鲜、是否正被全屏应用挡着藏起来
#[tauri::command]
fn taskbar_status(app: tauri::AppHandle) -> serde_json::Value {
    taskbar::status(&app)
}

/* ---------------- 数据推送（TCP + BLE，外部设备的第二屏） ---------------- */

/// 当前推送配置（前端设置页据此回显）
#[tauri::command]
fn get_push_config(state: State<'_, AppState>) -> push::PushConfig {
    state.push.get()
}

/// 保存推送配置并按它重启推送子系统（改端口 / 间隔 / 指标 / 通道都是重启一代 worker）。
/// 返回清洗后的配置，前端以返回值回显。同步命令即可——不建窗，只起后台线程。
#[tauri::command]
fn set_push_config(state: State<'_, AppState>, config: push::PushConfig) -> push::PushConfig {
    let cfg = state.push.set(config);
    state.push_hub.sync(&cfg);
    cfg
}

/// 推送子系统状态：监听地址 / 已连接客户端数 / BLE 广播与订阅 / 最近一拍
#[tauri::command]
fn push_status(state: State<'_, AppState>) -> push::PushStatus {
    state.push_hub.status()
}

/// 是否已写进开机自启
#[tauri::command]
fn get_autostart(app: tauri::AppHandle) -> bool {
    let Some(manager) = app.try_state::<tauri_plugin_autostart::AutoLaunchManager>() else {
        return false;
    };
    manager.is_enabled().unwrap_or(false)
}

#[tauri::command]
fn set_autostart(app: tauri::AppHandle, enabled: bool) -> Result<bool, String> {
    let manager = app
        .try_state::<tauri_plugin_autostart::AutoLaunchManager>()
        .ok_or_else(|| "自启插件未初始化".to_string())?;
    match if enabled {
        manager.enable()
    } else {
        manager.disable()
    } {
        Ok(()) => Ok(enabled),
        Err(e) => Err(format!("设置开机自启失败：{e}")),
    }
}

/// PawnIO（读 CPU 温度所需的内核驱动）是否已就位。
///
/// 前端据此决定是否在温度不可用时显示「安装 PawnIO」按钮：
/// 只有「没装」且「随包安装器在」时才引导安装——已经装上却读不到温度是另一类问题（多半是
/// 用户取消了 UAC），此时按钮没有意义。
#[cfg(windows)]
#[tauri::command]
fn pawnio_status(app: tauri::AppHandle) -> serde_json::Value {
    let probe = pawnio_setup::probe();
    let installer_ready = app
        .path()
        .resource_dir()
        .map(|dir| pawnio_setup::setup_path(&dir).is_file())
        .unwrap_or(false);
    serde_json::json!({
        "installed": probe.installed(),
        "reason": probe.describe(),
        "installerAvailable": installer_ready,
    })
}

/// 高级采集通道的就位情况（CPU 温度 / 风扇转速都需要提权子进程）。
///
/// 前端据此决定提示文案：
/// * `elevatedActive`：提权桥接是否正在提供新鲜读数（说明用户已授权且正常工作）；
/// * `missing`：还差什么（`pawnio` = 没装驱动；`lenovoFan` = 联想通道未启用）。
#[cfg(windows)]
#[tauri::command]
fn hardware_status(app: tauri::AppHandle, state: State<'_, AppState>) -> serde_json::Value {
    let probe = pawnio_setup::probe();
    let installer_ready = app
        .path()
        .resource_dir()
        .map(|dir| pawnio_setup::setup_path(&dir).is_file())
        .unwrap_or(false);

    // 桥接是否在跑：以「有没有属于本进程的新鲜读数」为准
    let fresh = msr_bridge::read_fresh_state().filter(|st| st.parent_pid == std::process::id());
    let elevated_active = fresh.is_some();
    let fan_ready = fresh.as_ref().is_some_and(|st| st.lenovo_fans.is_some());

    serde_json::json!({
        "isLenovo": wmi::is_lenovo(),
        "pawnioInstalled": probe.installed(),
        "pawnioReason": probe.describe(),
        "installerAvailable": installer_ready,
        "elevatedActive": elevated_active,
        "lenovoFanReady": fan_ready,
        // 需要用户点「启用」的场景：联想机型但还没拿到风扇
        "needsEnable": wmi::is_lenovo() && !fan_ready,
        // 设置页「CPU 风扇」下拉的备选项：当前能读到的风扇（号 + 转速）
        "lenovoFans": fresh
            .as_ref()
            .and_then(|st| st.lenovo_fans.clone())
            .unwrap_or_default(),
        // 当前生效的选择（null = 自动）
        "cpuFanId": state.fan_pref.get().cpu_fan_id,
    })
}

/// 读取风扇偏好（设置页「CPU 风扇」下拉的当前值）
#[tauri::command]
fn get_fan_pref(state: State<'_, AppState>) -> fan_pref::FanPref {
    state.fan_pref.get()
}

/// 保存风扇偏好，返回清洗后的配置（前端回显以这份为准）。
///
/// 采样线程与这里共用同一个 `FanPrefStore`，所以改完**下一拍**就生效，
/// 不需要重启应用或重建桥接。
#[tauri::command]
fn set_fan_pref(state: State<'_, AppState>, pref: fan_pref::FanPref) -> fan_pref::FanPref {
    state.fan_pref.set(pref)
}

/// 用户点击「安装 PawnIO」。
///
/// 真正的安装在**提权子进程**里完成（它顺带读温度），所以全过程只弹一次 UAC。
/// 这里立刻返回，前端靠轮询 `pawnio_status` 观察结果。
#[cfg(windows)]
#[tauri::command]
fn install_pawnio(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| format!("取不到应用资源目录：{e}"))?;
    if !pawnio_setup::setup_path(&resource_dir).is_file() {
        return Err("随包分发的 PawnIO 安装器缺失，请重新安装本工具".to_string());
    }

    // 重置「本会话已尝试」标志：这次点击必须真的触发一次提权
    state.bridge_prompted.store(false, Ordering::Relaxed);
    // 提权走 ShellExecuteExW，会一直卡到用户在 UAC 上做出选择，别占住命令线程
    std::thread::spawn(move || msr_bridge::install_and_start(&resource_dir));

    Ok("已请求安装 PawnIO，请在 Windows 弹窗中点「是」，随后 CPU 温度会自动出现".to_string())
}

/// 用户点击「启用」（联想机型读风扇转速需要管理员权限）。
///
/// 不装 PawnIO、只拉起提权桥接读风扇；若 PawnIO 也缺失且安装器在，顺带装上——
/// 反正已经弹过一次 UAC 了，不必让用户点两次。
#[cfg(windows)]
#[tauri::command]
fn enable_elevated(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| format!("取不到应用资源目录：{e}"))?;

    state.bridge_prompted.store(false, Ordering::Relaxed);
    // 只有安装器确实在、且驱动确实缺失时，才授权桥接安装
    let install = pawnio_setup::setup_path(&resource_dir).is_file()
        && !pawnio_setup::probe().installed();
    std::thread::spawn(move || msr_bridge::launch_with_install(&resource_dir, install));

    Ok("已请求启用高级采集，请在 Windows 弹窗中点「是」".to_string())
}

/// 托盘 Id：用于后续找回这个托盘实例
const TRAY_ID: &str = "main";
/// 依据当前状态重建托盘菜单
///
/// CheckMenuItem 内部含平台裸指针，不适合长期跨线程持有，
/// 所以状态变化时整体重建菜单，而不是去改某一个菜单项。
fn rebuild_tray_menu(app: &tauri::AppHandle) -> tauri::Result<()> {
    let paused = app
        .try_state::<AppState>()
        .map(|s| s.paused.load(Ordering::Relaxed))
        .unwrap_or(false);
    let autostart_enabled = app
        .try_state::<tauri_plugin_autostart::AutoLaunchManager>()
        .and_then(|m| m.is_enabled().ok())
        .unwrap_or(false);

    let menu = MenuBuilder::new(app)
        .item(&MenuItemBuilder::with_id("show", "显示面板").build(app)?)
        .separator()
        .item(
            &CheckMenuItemBuilder::with_id("pause", "暂停刷新")
                .checked(paused)
                .build(app)?,
        )
        .item(
            &CheckMenuItemBuilder::with_id("autostart", "开机自启")
                .checked(autostart_enabled)
                .build(app)?,
        )
        .separator()
        .item(&MenuItemBuilder::with_id("quit", "退出").build(app)?)
        .build()?;

    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_menu(Some(menu))?;
    }
    Ok(())
}

/// 托盘图标 + 右键菜单 + 左键呼出主窗口
fn build_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(app.default_window_icon().cloned().expect("缺少应用图标"))
        .tooltip("脉盘 PulseDeck")
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
        })
        .build(app)?;

    rebuild_tray_menu(app)?;

    app.on_menu_event(move |app, event| match event.id().as_ref() {
        "show" => {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }
        "pause" => {
            let Some(state) = app.try_state::<AppState>() else {
                return;
            };
            let next = !state.paused.load(Ordering::Relaxed);
            state.paused.store(next, Ordering::Relaxed);
            let _ = rebuild_tray_menu(app);
        }
        "autostart" => {
            let Some(manager) = app.try_state::<tauri_plugin_autostart::AutoLaunchManager>() else {
                return;
            };
            let next = !manager.is_enabled().unwrap_or(false);
            let result = if next {
                manager.enable()
            } else {
                manager.disable()
            };
            if result.is_ok() {
                let _ = rebuild_tray_menu(app);
            }
        }
        "quit" => app.exit(0),
        _ => {}
    });

    Ok(())
}

/// 把崩溃信息落盘
///
/// release 走 `windows_subsystem = "windows"`，没有控制台，panic 信息会直接消失，
/// 出问题时全靠这个文件定位。位置：%LOCALAPPDATA%\PulseDeck\crash.log
fn install_crash_logger() {
    let dir = app_dir();
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let line = format!(
            "[{:?}] {info}\n  location: {}\n",
            std::time::SystemTime::now(),
            info.location()
                .map(|l| format!("{}:{}", l.file(), l.line()))
                .unwrap_or_else(|| "?".to_string())
        );
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("crash.log"))
        {
            use std::io::Write as _;
            let _ = f.write_all(line.as_bytes());
        }
        eprintln!("{line}");
        default_hook(info);
    }));
}

/// 应用数据目录：崩溃日志、桌面宠物配置等运行时文件都放这里。
///
/// 按各平台惯例选址（Windows 上与原实现逐字节一致，不迁移现有用户）：
/// * Windows：`%LOCALAPPDATA%\PulseDeck`
/// * macOS：`~/Library/Application Support/PulseDeck`
/// * Linux：`$XDG_CONFIG_HOME/pulsedeck`（未设则 `~/.config/pulsedeck`）
fn app_dir() -> std::path::PathBuf {
    #[cfg(windows)]
    {
        let dir = std::env::var_os("LOCALAPPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("PulseDeck");
        migrate_legacy_app_dir(&dir);
        dir
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        home.join("Library/Application Support/PulseDeck")
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
            .unwrap_or_else(std::env::temp_dir);
        base.join("pulsedeck")
    }
    #[cfg(not(any(windows, unix)))]
    {
        std::env::temp_dir().join("PulseDeck")
    }
}

/// 品牌从 WinMonitor 改成 PulseDeck 时，把旧目录里的用户配置搬过来，
/// 免得桌面宠物开关 / 手动指定的风扇号一夜回到默认值。
///
/// 只在 PulseDeck 目录**还不存在**时跑一次；任何一个环节出错都静默跳过——
/// 迁移失败最多是读默认值，不该拖垮启动。
#[cfg(windows)]
fn migrate_legacy_app_dir(new_dir: &std::path::Path) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        if new_dir.exists() {
            return;
        }
        let Some(parent) = new_dir.parent() else { return };
        let old_dir = parent.join("WinMonitor");
        if !old_dir.is_dir() {
            return;
        }
        if std::fs::create_dir_all(new_dir).is_err() {
            return;
        }
        // 只搬用户配置，日志类文件（crash.log / pawnio.log）不搬
        for name in ["pet.json", "fan.json"] {
            let src = old_dir.join(name);
            if src.is_file() {
                let _ = std::fs::copy(&src, new_dir.join(name));
            }
        }
    });
}

/// 子进程模式入口：被提权拉起时（带 `--pawnio-bridge`）只跑温度桥接循环，不建窗口。
/// 返回值表示「本次进程是不是桥接进程」。
#[cfg(windows)]
pub fn run_bridge_if_requested() -> bool {
    msr_bridge::run_if_requested()
}

/// 非 Windows 没有 PawnIO / UAC 提权桥接的概念，进程一定不是桥接进程。
#[cfg(not(windows))]
pub fn run_bridge_if_requested() -> bool {
    false
}

/* ---------------- 窗口外壳：主窗口的标题栏 ---------------- */

pub fn run() {
    install_crash_logger();
    // 采样器独立于 UI 运行，多个窗口 / 前端刷新共用同一份数据
    let sampler = Arc::new(sampler::Sampler::new());
    sampler.warmup();

    let running = Arc::new(AtomicBool::new(true));
    let paused = Arc::new(AtomicBool::new(false));
    let interval_ms = Arc::new(AtomicU32::new(1000));
    // 默认开启「关闭后常驻托盘」
    let close_to_tray = Arc::new(AtomicBool::new(true));
    // 宠物配置从磁盘恢复：上次开着这次就直接出现在桌面上，不必等主窗口推配置过来
    let pet = pet::PetStore::load();
    let pet_at_startup = pet.get();
    // 任务栏挂件同理：配置里开着就在启动时贴回任务栏
    let taskbar = taskbar::TaskbarStore::load();
    let taskbar_at_startup = taskbar.get();

    {
        let sampler = sampler.clone();
        let running = running.clone();
        let paused = paused.clone();
        let interval_ms = interval_ms.clone();
        thread::spawn(move || {
            while running.load(Ordering::Relaxed) {
                let target = interval_ms.load(Ordering::Relaxed);
                let mut waited = 0u32;
                while waited < target && running.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(100));
                    waited += 100;
                }
                if !running.load(Ordering::Relaxed) {
                    break;
                }
                if paused.load(Ordering::Relaxed) {
                    continue;
                }
                sampler.refresh();
            }
        });
    }

    let close_flag = close_to_tray.clone();

    // 温度桥接（读 MSR 需要管理员）的「本会话是否已尝试提权」标志，
    // 防止启动过程中反复弹 UAC。
    #[cfg(windows)]
    let bridge_prompted = Arc::new(AtomicBool::new(false));

    // 风扇偏好只加载一次，同时交给采样线程与设置页——两边必须是同一个句柄，
    // 否则「改了 CPU 风扇号」只在设置页生效、读数纹丝不动。
    let fan_pref = fan_pref::FanPrefStore::load();
    sampler.set_fan_pref(fan_pref.clone());

    // 数据推送（TCP + BLE）：配置从磁盘恢复，开着就立刻起 worker 开始监听/广播。
    // hub 持有采样器与全局存活位——推送的每拍数据来自 sampler.latest()，
    // 应用退出（running=false）时 worker 随之一并收尾。
    let push = push::PushStore::load();
    let push_hub = Arc::new(push::PushHub::new(sampler.clone(), running.clone()));
    push_hub.sync(&push.get());

    // AppState 的字段组成随平台略有不同（提权标志仅 Windows）。
    // 结构体字面量的字段位置不支持 cfg 属性，所以在构建处分叉两份。
    #[cfg(windows)]
    let app_state = AppState {
        sampler,
        paused,
        interval_ms,
        close_to_tray,
        bridge_prompted: bridge_prompted.clone(),
        pet,
        taskbar,
        fan_pref,
        push,
        push_hub,
    };
    #[cfg(not(windows))]
    let app_state = AppState {
        sampler,
        paused,
        interval_ms,
        close_to_tray,
        pet,
        taskbar,
        fan_pref,
        push,
        push_hub,
    };

    // 窗口外壳：主窗口无原生装饰条（tauri.conf.json 的 decorations: false），
    // 标题栏 / 拖动 / 四边缩放由页面自绘，见 index.html 的 .titlebar 与 src/main.js
    tauri::Builder::default()
        // 单实例闸门必须**第一个**注册（插件文档要求）：第二次启动时新进程会立刻退出，
        // 这里只负责把已有实例的主窗口拉到前台。
        //
        // 提权桥接子进程不受影响：它带 `--pawnio-bridge`，在 `main.rs` 里
        // 走 `run_bridge_if_requested()` 直接返回，根本不会走到这个 Builder。
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None::<Vec<&str>>,
        ))
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            set_paused,
            set_interval,
            set_close_to_tray,
            get_runtime_config,
            get_autostart,
            set_autostart,
            // 以下命令依赖 Windows 专属模块（PawnIO 提权链），
            // generate_handler! 会随 cfg 剔除条目（2026-10-14 实验确认）
            #[cfg(windows)]
            pawnio_status,
            #[cfg(windows)]
            hardware_status,
            get_fan_pref,
            set_fan_pref,
            #[cfg(windows)]
            install_pawnio,
            #[cfg(windows)]
            enable_elevated,
            get_pet_config,
            set_pet_config,
            get_pet_metrics,
            pet_heartbeat,
            pet_status,
            get_taskbar_config,
            set_taskbar_config,
            taskbar_tick,
            taskbar_fit,
            taskbar_status,
            get_push_config,
            set_push_config,
            push_status,
            show_main_window
        ])
        .setup({
            #[cfg(windows)]
            let setup_prompted = bridge_prompted.clone();
            move |app| {
                build_tray(app.handle())?;

                // 先把主窗口的 WebView2 环境缓存下来（仅 Windows 有此机制）：宠物 / 任务栏挂件
                // 两个窗口都必须复用它，而 `with_webview` 只在主线程同步有效——这里（setup）正好
                // 是主线程。放在建窗口**之前**：之后建窗口的动作都在非主线程上，现取是取不到的。
                #[cfg(windows)]
                pet::capture_main_environment(app.handle());

                // 恢复桌面宠物窗口（配置里 enabled=true 时才建）。
                // setup() 里直接建即可——曾怀疑「事件循环未就绪导致渲染冻结」，
                // 2026-10-11 排查确认那只是测试环境假象（WorkBuddy 沙箱注入
                // tsbx.dll 导致 WebView2 browser 进程崩溃，与创建时机无关）。
                if pet_at_startup.enabled {
                    if let Err(e) = pet::sync_window(&app.handle(), &pet_at_startup) {
                        eprintln!("恢复桌面宠物失败：{e}");
                    }
                }

                // 恢复任务栏挂件（同样按配置决定要不要建）
                if taskbar_at_startup.enabled {
                    if let Err(e) = taskbar::sync_window(&app.handle(), &taskbar_at_startup) {
                        eprintln!("恢复任务栏显示失败：{e}");
                    }
                }

                // 后台拉起「CPU 温度直读桥接」子进程（提权 + 隐藏窗口），不阻塞主窗口启动（Windows）。
                // 资源目录来自 Tauri 的 Resource 目录（由 bundle.resources 打进安装包），
                // 温度模块位于 <resource_dir>/pawnio/{IntelMSR,AMDFamily17}.bin，
                // 载哪个由 CPU 厂商决定（见 msr_bridge.rs 的 launch_bridge）。
                // 只有当读数缺失/过期时才拉，且一个会话只尝试一次（避免 UAC 轰炸）。
                #[cfg(windows)]
                if let Ok(resource_dir) = app.path().resource_dir() {
                    let prompted = setup_prompted.clone();
                    std::thread::spawn(move || {
                        msr_bridge::ensure_started(&resource_dir, &prompted);
                    });
                }
                Ok(())
            }
        })
        .on_window_event(move |window, event| match event {
            // 「关闭主窗口 = 隐藏到托盘」只适用于**主窗口**。
            //
            // 宠物窗口绝不能走这条：它是无边框挂件，`hide()` 之后桌面上空无一物，
            // 可窗口对象还在、配置里 `enabled` 也还是 true——设置页于是显示
            // 「运行中」或「窗口在但页面没有心跳」，而用户面对的是一个
            // 桌面上什么都没有、设置页却说一切正常的界面，完全无从判断。
            // （更糟的是隐藏页会被 Chromium 节流，心跳必然变旧，状态行给出的
            // 「WebView 未渲染」是个错误方向的诊断。）
            //
            // 让它按默认行为真的关掉：状态行就会如实说
            // 「窗口未创建 · 保存设置或重启一次应用即可」，而那正是恢复的正确做法
            // （`set_pet_config` 每次都调 `sync_window`，配置里还开着就会重建）。
            tauri::WindowEvent::CloseRequested { api, .. } if window.label() == "main" => {
                if close_flag.load(Ordering::Relaxed) {
                    // 隐藏到托盘，采样线程继续在后台跑
                    let _ = window.hide();
                    api.prevent_close();
                }
            }
            // 记住宠物窗口被拖到哪了。拖动期间每秒几十个事件，这里**只改内存**，
            // 落盘见 `PetStore::set_position`（心跳与销毁时各写一次）。
            tauri::WindowEvent::Moved(position) if window.label() == pet::WINDOW_LABEL => {
                if let Some(state) = window.app_handle().try_state::<AppState>() {
                    // 存的坐标与 `WebviewWindowBuilder::position` 一致（逻辑像素）
                    let scale = window.scale_factor().unwrap_or(1.0).max(0.1);
                    state
                        .pet
                        .set_position([position.x as f64 / scale, position.y as f64 / scale]);
                }
            }
            // 宠物窗口被销毁（关掉开关 / 在桌面上关掉它）时立刻落盘，
            // 免得「刚拖好就关掉」还要等下一次心跳
            tauri::WindowEvent::Destroyed if window.label() == pet::WINDOW_LABEL => {
                if let Some(state) = window.app_handle().try_state::<AppState>() {
                    state.pet.flush_position();
                }
            }
            _ => {}
        })
        .build(tauri::generate_context!())
        .expect("构建 Tauri 应用失败")
        .run(move |_app_handle, event| {
            if matches!(
                event,
                tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. }
            ) {
                running.store(false, Ordering::Relaxed);
                // 提权桥接进程每秒自查父进程存活，父进程一退它自行收尾；
                // 不需要（也不该）在这里跨权限 TerminateProcess——那大概率会失败。
            }
        });
}

/* ---------------- 分段诊断：定位原生数据源问题的工具 ----------------
 *
 * 新增指标或某台机器读不到数据时，先跑这些用例逐层确认：
 *   cargo test diag_probe_gpu -- --nocapture      # NVML / PDH GPU
 *   cargo test diag_probe_thermal -- --nocapture  # 温度（含 WMI 硬件监控）
 *   cargo test diag_cpu_extras -- --nocapture     # 实时主频（PDH Actual Frequency，附标称频率对照）
 *   cargo test diag_wmi -- --nocapture            # WMI 连通性（可加 WB_WMI_DIAG=1 看逐步 HRESULT）
 */

// 诊断用例全部直连 Windows 原生数据源（PDH / WMI / PawnIO / NVML），随平台一起门控。
#[cfg(test)]
#[cfg(windows)]
mod diag {
    fn flush(tag: &str) {
        use std::io::Write;
        print!("[{tag}] ");
        let _ = std::io::stdout().flush();
    }

    /// 是否运行在提升（管理员）权限下
    fn is_elevated() -> bool {
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::Security::{
            GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
        };
        use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        unsafe {
            let mut token = HANDLE::default();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
                return false;
            }
            let mut elev = TOKEN_ELEVATION::default();
            let mut ret_len = 0u32;
            let ok = GetTokenInformation(
                token,
                TokenElevation,
                Some(&mut elev as *mut _ as *mut core::ffi::c_void),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut ret_len,
            );
            let _ = windows::Win32::Foundation::CloseHandle(token);
            ok.is_ok() && elev.TokenIsElevated != 0
        }
    }

    /// 把诊断结果同时写到 `%TEMP%\wb_lenovo_diag.txt`，便于提权运行后回读
    fn dump_to_file(lines: &[String]) {
        use std::io::Write;
        let path = std::env::temp_dir().join("wb_lenovo_diag.txt");
        if let Ok(mut f) = std::fs::File::create(&path) {
            let _ = writeln!(f, "elevated = {}", is_elevated());
            for l in lines {
                let _ = writeln!(f, "{l}");
            }
        }
        println!(">>> 诊断已写入 {}", path.display());
    }

    /// 端到端验证「风扇**每拍都刷新**」（这是修过的 bug，别再退回节流）。
    ///
    /// 连续采样若干次并打印每拍读数：修好之后相邻两拍的风扇转速几乎总在变
    /// （真机风扇每秒有几个 RPM 的抖动）；而带 30s 节流的旧实现会让这里**次次相同**。
    ///
    /// 需要提权桥接正在运行（界面上点过「启用」）才有数据；桥接没起来时只打印 `--`，
    /// 此时不做断言。
    ///
    /// 用法：`cargo test --lib diag::fan_refresh -- --nocapture`
    #[test]
    fn fan_refresh() {
        const N: usize = 6;
        let mut probe = super::fan::FanProbe::new();
        let mut seen = Vec::new();
        for i in 0..N {
            let r = probe.sample(1);
            let cur = (r.cpu.rpm, r.gpus.first().and_then(|g| g.rpm));
            println!("[{i}] cpu={:?} gpu={:?}", cur.0, cur.1);
            seen.push(cur);
            if i + 1 < N {
                // 比桥接的写入周期（1s）略长，确保每拍都跨过一次写入
                std::thread::sleep(std::time::Duration::from_millis(1100));
            }
        }
        let has_data = seen.iter().any(|(c, _)| c.is_some());
        let changes = seen.windows(2).filter(|w| w[0] != w[1]).count();
        println!("{N} 次采样，拿到读数 = {has_data}，相邻两拍发生变化 = {changes} 次");
        if has_data {
            assert!(
                changes > 0,
                "风扇读数在 {N} 拍内一次都没变——多半是被节流回退了（旧实现固定 30s 缓存）"
            );
        }
    }

    /// WMI 建连开销对照：逐个调用（每风扇一次连接）vs 批量调用（共用一次连接）。
    ///
    /// 非提权下 `ExecMethod` 会被拒，但**建连 / 取类定义 / 取方法签名**这几步照走，
    /// 而它们正是这条链路里最贵的部分（`ConnectServer`），所以这个对照足以说明问题。
    ///
    /// 用法：`cargo test --lib diag::lenovo_conn_cost -- --nocapture`
    #[test]
    fn lenovo_conn_cost() {
        const ROUNDS: u32 = 10;
        let t = std::time::Instant::now();
        for _ in 0..ROUNDS {
            for id in 0..=3u8 {
                let _ = super::wmi::lenovo_fan_rpm(id);
            }
        }
        let old = t.elapsed();
        let t = std::time::Instant::now();
        for _ in 0..ROUNDS {
            let _ = super::wmi::lenovo_fans();
        }
        let new = t.elapsed();
        println!(
            "逐个(每风扇一次连接) = {old:?} / 批量(共用一次连接) = {new:?}  →  x{:.2}",
            old.as_secs_f64() / new.as_secs_f64().max(f64::MIN_POSITIVE)
        );
    }

    /// 联想通道诊断（可提权运行，结果落盘到 %TEMP%\wb_lenovo_diag.txt）
    ///
    /// 用法：`cargo test --lib diag::lenovo_elev -- --nocapture`
    /// 提权：`Start-Process <test-exe> -ArgumentList 'diag::lenovo_elev','--nocapture' -Verb RunAs`
    #[test]
    fn lenovo_elev() {
        let mut lines = Vec::new();
        flush("elev-check");
        lines.push(format!("is_elevated = {}", is_elevated()));
        lines.push(format!("is_lenovo = {}", super::wmi::is_lenovo()));

        for (label, res) in super::wmi::probe_exec_paths(
            "root\\WMI",
            "LENOVO_FAN_METHOD",
            "Fan_GetCurrentFanSpeed",
            &[("FanID", 0u8)],
        ) {
            lines.push(format!("{label}: {res}"));
        }
        lines.push(format!("lenovo_fans = {:?}", super::wmi::lenovo_fans()));

        // 端到端：走一遍桥接写状态 + 主进程读回，确认 JSON 往返没问题
        let fans = super::msr_bridge::sample_lenovo_fans_for_test();
        lines.push(format!("bridge sample_lenovo_fans = {fans:?}"));
        let json = serde_json::to_string(&super::msr_bridge::MsrState::for_test(fans.clone()))
            .unwrap_or_default();
        lines.push(format!("state json = {json}"));

        dump_to_file(&lines);
        for l in &lines {
            println!("{l}");
        }
    }

    #[test]
    fn nvml() {
        flush("init");
        let nvml = nvml_wrapper::Nvml::init();
        println!("ok={}", nvml.is_ok());
        let Some(nvml) = nvml.ok() else {
            println!("NVML 不可用（非 NVIDIA 或未装驱动）");
            return;
        };
        println!("device_count = {:?}", nvml.device_count());
        if let Ok(dev) = nvml.device_by_index(0) {
            println!("name = {:?}", dev.name());
            println!("util = {:?}", dev.utilization_rates().map(|u| u.gpu));
            println!("mem  = {:?}", dev.memory_info().map(|m| (m.used, m.total)));
            println!(
                "temp = {:?}",
                dev.temperature(nvml_wrapper::enum_wrappers::device::TemperatureSensor::Gpu)
            );
            println!("power= {:?}", dev.power_usage());
        }
        drop(nvml);
        println!("dropped nvml OK");
    }

    #[test]
    fn pdh_thermal() {
        flush("open");
        let Some(q) = super::pdh::Query::new() else {
            println!("PDH 查询创建失败");
            return;
        };
        let Some(c) = q.add(r"\Thermal Zone Information(*)\Temperature") else {
            println!("添加温度计数器失败");
            return;
        };
        q.collect();
        std::thread::sleep(std::time::Duration::from_millis(300));
        q.collect();
        println!("thermal = {:?}", q.double_array(c));
        drop(q);
        println!("dropped pdh OK");
    }

    #[test]
    fn wmi() {
        flush("video rows");
        let rows = super::wmi::query(
            "root\\cimv2",
            "SELECT Name FROM Win32_VideoController",
            &["Name"],
        );
        match rows {
            Some(r) => println!("rows={} -> {:?}", r.len(), r),
            None => println!("query returned None"),
        }
        flush("temps");
        println!("{:?}", super::wmi::hardware_temperatures().map(|(s, _)| s));
        println!("wmi dropped OK");
    }

    #[test]
    fn probe_gpu() {
        let mut probe = super::gpu::GpuProbe::new();
        let (d, s) = probe.sample();
        println!(
            "gpu devices={:?} source={s}",
            d.iter().map(|x| &x.name).collect::<Vec<_>>()
        );
        for dev in &d {
            println!(
                "  {} util={:?} temp={:?} mem={}/{}",
                dev.name, dev.usage_pct, dev.temp_c, dev.mem_used_bytes, dev.mem_total_bytes
            );
        }
        println!("gpu probe dropped OK");
    }

    #[test]
    fn probe_thermal() {
        let mut probe = super::thermal::ThermalProbe::new();
        println!("thermal={:?}", probe.sample());
        println!("thermal probe dropped OK");
    }

    /// PawnIO 直连是否可用：设备能否打开、模块能否载入、温度能否读出
    ///
    /// 需要有管理员权限才有意义——未提权时会在「打开设备」这一步就报访问被拒绝。
    /// 两个模块都会试着载一次：载错厂商的那个会被模块自己以 `STATUS_NOT_SUPPORTED`
    /// 拒绝（IntelMSR 校验 CPUID 厂商、AMDFamily17 校验厂商与 family 17h~1Ah），
    /// 所以这条输出同时验证了「模块与 CPU 匹配」这件事。
    #[test]
    fn pawnio_msr() {
        let vendor = super::cpu_vendor::detect();
        println!("cpu vendor    = {}", vendor.label());
        println!(
            "expected file = {}",
            super::pawnio::module_file(vendor).unwrap_or("（该厂商没有直读通道）")
        );

        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("pawnio");
        for file in ["IntelMSR.bin", "AMDFamily17.bin"] {
            let module = dir.join(file);
            println!(
                "\n--- {file}（{}）---",
                if module.exists() { "存在" } else { "缺失" }
            );
            match std::fs::read(&module) {
                Ok(blob) => match super::pawnio::CpuTempReader::open(vendor, &blob) {
                    Ok(reader) => {
                        println!("source        = {}", reader.source());
                        if let Some(t) = reader.tjmax() {
                            println!("tjmax         = {t:.0} °C");
                        }
                        println!("package temp  = {:?} °C", reader.package_temp());
                        let mut reader = reader;
                        println!("package power = {:?} W（首拍无差分，属正常）", reader.package_power_w());
                        std::thread::sleep(std::time::Duration::from_millis(1000));
                        println!("package power = {:?} W（1 s 后）", reader.package_power_w());
                    }
                    Err(e) => println!("打开 / 载入失败：{e}"),
                },
                Err(e) => println!("读取模块文件失败：{e}"),
            }
        }

        match super::msr_bridge::read_fresh_state() {
            Some(st) => println!("\nbridge state  = {st:?}"),
            None => println!("\nbridge state  = 无新鲜读数（桥接进程未在运行）"),
        }
    }

    /// PawnIO 探测与随包安装器是否就位
    ///
    /// 未提权时预期 `Installed`（设备在、但拒绝访问）；未安装 PawnIO 时是 `Missing`。
    #[test]
    fn pawnio_setup() {
        let probe = super::pawnio_setup::probe();
        println!("probe            = {probe:?}");
        println!("installed        = {}", probe.installed());
        println!("describe         = {}", probe.describe());

        let installer = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("pawnio")
            .join(super::pawnio_setup::SETUP_FILE_NAME);
        println!(
            "installer        = {} ({})",
            installer.display(),
            if installer.is_file() { "存在" } else { "缺失" }
        );
        println!("installer 大小   = {:?}", installer.metadata().map(|m| m.len()));
    }

    #[test]
    fn sampler_full() {
        let s = super::sampler::Sampler::new();
        s.warmup();
        std::thread::sleep(std::time::Duration::from_millis(1200));
        s.refresh();
        let snap = s.latest();
        println!(
            "cpu={} gpu={:?} net_rx={} net_tx={}",
            snap.cpu.usage_total,
            snap.gpu.usage_pct,
            snap.net.rx_bytes_per_sec,
            snap.net.tx_bytes_per_sec
        );
        println!("sampler dropped OK");
    }

    /// 风扇转速：各通道分别探测，确认到底是哪一层缺数据
    ///
    /// 预期（2026-10-09 本机实测）：NVML `fan_speed` 为 N/A（笔记本独显由 EC 控制）、
    /// LHM/OHM 命名空间不存在、`Win32_Fan` 有实例但 `DesiredSpeed` 为空。
    ///
    /// 2026-10-10 追加：联想机型应能通过 `LENOVO_FAN_METHOD` 读到 RPM。
    /// 联想 `LENOVO_FAN_METHOD` 类定义 dump：确认方法名与入参签名
    #[test]
    fn probe_lenovo_class() {
        flush("mof");
        match super::wmi::dump_class_mof("root\\WMI", "LENOVO_FAN_METHOD") {
            Some(mof) => println!("{mof}"),
            None => println!("取不到 LENOVO_FAN_METHOD 的 MOF"),
        }
        flush("props");
        println!(
            "Fan_GetCurrentFanSpeed in-params = {:?}",
            super::wmi::dump_prop_names(
                "root\\WMI",
                "LENOVO_FAN_METHOD",
                Some("Fan_GetCurrentFanSpeed")
            )
        );
        println!(
            "Fan_GetCurrentSensorTemperature in-params = {:?}",
            super::wmi::dump_prop_names(
                "root\\WMI",
                "LENOVO_FAN_METHOD",
                Some("Fan_GetCurrentSensorTemperature")
            )
        );
        flush("exec-paths");
        for (label, res) in super::wmi::probe_exec_paths(
            "root\\WMI",
            "LENOVO_FAN_METHOD",
            "Fan_GetCurrentFanSpeed",
            &[("FanID", 0u8)],
        ) {
            println!("  {label}: {res}");
        }

        flush("instances");
        for cls in ["LENOVO_FAN_TABLE_DATA", "LENOVO_FAN_METHOD"] {
            let rows = super::wmi::query(
                "root\\WMI",
                &format!("SELECT InstanceName, Active FROM {cls}"),
                &["InstanceName", "Active"],
            );
            println!("  {cls} 实例 = {rows:?}");
        }
    }

    #[test]
    fn probe_fan() {
        flush("nvml-fan");
        if let Ok(nvml) = nvml_wrapper::Nvml::init() {
            if let Ok(dev) = nvml.device_by_index(0) {
                println!("fan_speed(pct) = {:?}", dev.fan_speed(0));
                println!("fan_speed_rpm  = {:?}", dev.fan_speed_rpm(0));
            }
        } else {
            println!("NVML 不可用");
        }

        flush("vendor");
        println!("是否联想设备   = {}", super::wmi::is_lenovo());
        println!("ACPI 实例名    = {:?}", super::wmi::lenovo_fan_instance());
        println!("联想风扇上限   = {:?}", super::wmi::lenovo_fan_max_speed());
        println!("联想风扇号     = {:?}", super::wmi::lenovo_fan_ids());
        for id in super::wmi::lenovo_fan_ids() {
            println!("  FanID={id} rpm = {:?}", super::wmi::lenovo_fan_rpm(id));
        }
        for id in 0..=5u8 {
            println!("  SensorID={id} temp = {:?}", super::wmi::lenovo_sensor_temp(id));
        }
        println!("lenovo_fans    = {:?}", super::wmi::lenovo_fans());
        println!("桥接风扇读数   = {:?}", super::msr_bridge::read_fresh_fans());

        flush("wmi-fans");
        println!("hardware_fans  = {:?}", super::wmi::hardware_fans());
        println!("acpi_fans      = {:?}", super::wmi::acpi_fan_speeds());

        flush("probe");
        let mut probe = super::fan::FanProbe::new();
        println!("report = {:?}", probe.sample(1));
    }

    /// 电池原始读数逐字段打印：判断「适配器功率读不到」到底卡在哪一层
    #[test]
    fn probe_battery() {
        use windows_sys::Win32::System::Power::{
            CallNtPowerInformation, SystemBatteryState, SYSTEM_BATTERY_STATE,
        };
        flush("nt-power");
        unsafe {
            let mut st: SYSTEM_BATTERY_STATE = std::mem::zeroed();
            let ret = CallNtPowerInformation(
                SystemBatteryState,
                std::ptr::null(),
                0,
                &mut st as *mut _ as *mut core::ffi::c_void,
                std::mem::size_of::<SYSTEM_BATTERY_STATE>() as u32,
            );
            println!("ret={ret}");
            println!(
                "  AcOnLine={} BatteryPresent={} Charging={} Discharging={}",
                st.AcOnLine, st.BatteryPresent, st.Charging, st.Discharging
            );
            println!(
                "  Rate={} (as i32={}) MaxCapacity={} RemainingCapacity={} EstimatedTime={}",
                st.Rate,
                st.Rate as i32,
                st.MaxCapacity,
                st.RemainingCapacity,
                st.EstimatedTime
            );
        }
        flush("wmi-battery");
        let rows = super::wmi::query(
            "root\\wmi",
            "SELECT InstanceName, PowerOnline, Charging, Discharging, ChargeRate, DischargeRate, Voltage, RemainingCapacity FROM BatteryStatus",
            &[
                "InstanceName",
                "PowerOnline",
                "Charging",
                "Discharging",
                "ChargeRate",
                "DischargeRate",
                "Voltage",
                "RemainingCapacity",
            ],
        );
        match rows {
            Some(r) => println!("{r:?}"),
            None => println!("WMI BatteryStatus 查询返回 None"),
        }
        flush("parsed");
        println!("raw   = {:?}", super::win32::battery());
        println!(
            "info  = {:?}",
            super::battery::from_raw(super::win32::battery(), super::win32::battery_percent())
        );
    }

    /// 电源类 PDH 计数器是否有实例、单位与量级
    #[test]
    fn probe_power_counters() {
        flush("open");
        let Some(q) = super::pdh::Query::new() else {
            println!("PDH 查询创建失败");
            return;
        };
        let pm = q.add(r"\Power Meter(*)\Power");
        let em = q.add(r"\Energy Meter(*)\Power");
        println!("add: power_meter={} energy_meter={}", pm.is_some(), em.is_some());
        q.collect();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        q.collect();
        if let Some(c) = pm {
            println!("power meter  = {:?}", q.double_array(c));
        }
        if let Some(c) = em {
            println!("energy meter = {:?}", q.double_array(c));
        }
    }

    /// CPU 实时主频（`Actual Frequency`）：确认它能读出实时值，并打出标称频率做对照
    ///
    /// 用法：`cargo test --lib diag::cpu_extras -- --nocapture`
    ///
    /// 两个对照量：`Processor Frequency` 是标称频率（与 sysinfo 报的同一路，实测恒定不动），
    /// `% Processor Performance` 是实时频率相对标称的百分比（能解释任务管理器的「速度」
    /// 为什么会超过「基准速度」）。注意**「利用率」不在这一路**：任务管理器显示的是忙时占比
    /// （与本工具总占用同口径），`% Processor Utility` 实测与它对不上，见 AGENTS.md
    /// 「Windows 上的已知坑」与 `cpu_extras.rs` 的模块文档。
    #[test]
    fn cpu_extras() {
        flush("probe");
        let mut probe = super::cpu_extras::CpuExtrasProbe::new();
        println!("reading[0] = {:?}", probe.sample());
        std::thread::sleep(std::time::Duration::from_millis(1000));
        println!("reading[1] = {:?}", probe.sample());

        flush("sysinfo");
        let mut sys = sysinfo::System::new();
        sys.refresh_cpu_all();
        println!(
            "sysinfo 主频 = {:?} MHz（标称；实时值不会动，实时主频看 Actual Frequency）",
            sys.cpus().iter().map(|c| c.frequency()).max()
        );

        flush("raw");
        let Some(q) = super::pdh::Query::new() else {
            println!("PDH 查询创建失败");
            return;
        };
        let counters = [
            (
                "Actual Frequency",
                q.add(r"\Processor Information(*)\Actual Frequency"),
            ),
            (
                "Processor Frequency",
                q.add(r"\Processor Information(*)\Processor Frequency"),
            ),
            (
                "% Processor Performance",
                q.add(r"\Processor Information(*)\% Processor Performance"),
            ),
        ];
        q.collect();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        q.collect();
        for (label, counter) in counters {
            let Some(c) = counter else {
                println!("{label:<22} 计数器不可用");
                continue;
            };
            let rows = q.double_array(c);
            let total = rows
                .iter()
                .find(|(name, _)| name == "_Total")
                .map(|(_, v)| *v);
            println!("{label:<22} _Total = {total:?}（{} 个实例）", rows.len());
        }
    }
}

#[cfg(test)]
mod smoke {
    /// 冒烟测试：连续两次采样后打印真实结果，用于校验各指标是否取到值
    #[test]
    fn collect_once() {
        let sampler = super::sampler::Sampler::new();
        sampler.warmup();
        // PDH 类计数器需要第二拍才有值
        std::thread::sleep(std::time::Duration::from_millis(1200));
        sampler.refresh();
        let snap = sampler.latest();
        println!("{}", serde_json::to_string_pretty(&snap).unwrap());

        assert!(snap.cpu.logical_cores > 0, "未取到 CPU 核心数");
        assert!(snap.memory.total_bytes > 0, "未取到内存总量");
        assert!(!snap.disks.is_empty(), "未取到任何磁盘卷");
        assert!(snap.system.process_count > 0, "未取到进程列表");
    }
}
