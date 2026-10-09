//! 应用入口：后台采样线程 + Tauri 命令 + 托盘常驻

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

mod battery;
mod gpu;
mod model;
mod msr_bridge;
mod pawnio;
mod pawnio_setup;
mod pdh;
mod sampler;
mod thermal;
mod win32;
mod wmi;

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
    /// 本会话是否已尝试拉起「MSR 温度桥接」进程（防止反复弹 UAC）
    bridge_prompted: Arc<AtomicBool>,
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

/// 用户点击「安装 PawnIO」。
///
/// 真正的安装在**提权子进程**里完成（它顺带读温度），所以全过程只弹一次 UAC。
/// 这里立刻返回，前端靠轮询 `pawnio_status` 观察结果。
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
        .tooltip("Win 系统监控")
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
/// 出问题时全靠这个文件定位。位置：%LOCALAPPDATA%\WinMonitor\crash.log
fn install_crash_logger() {
    let dir = crash_dir();
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

fn crash_dir() -> std::path::PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("WinMonitor")
}

/// 子进程模式入口：被提权拉起时（带 `--pawnio-bridge`）只跑温度桥接循环，不建窗口。
/// 返回值表示「本次进程是不是桥接进程」。
pub fn run_bridge_if_requested() -> bool {
    msr_bridge::run_if_requested()
}

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
    let bridge_prompted = Arc::new(AtomicBool::new(false));

    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None::<Vec<&str>>,
        ))
        .manage(AppState {
            sampler,
            paused,
            interval_ms,
            close_to_tray,
            bridge_prompted: bridge_prompted.clone(),
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            set_paused,
            set_interval,
            set_close_to_tray,
            get_runtime_config,
            get_autostart,
            set_autostart,
            pawnio_status,
            install_pawnio
        ])
        .setup({
            let setup_prompted = bridge_prompted.clone();
            move |app| {
                build_tray(app.handle())?;

                // 后台拉起「MSR 温度桥接」子进程（提权 + 隐藏窗口），不阻塞主窗口启动。
                // 资源目录来自 Tauri 的 Resource 目录（由 bundle.resources 打进安装包），
                // 温度模块位于 <resource_dir>/pawnio/IntelMSR.bin。
                // 只有当读数缺失/过期时才拉，且一个会话只尝试一次（避免 UAC 轰炸）。
                if let Ok(resource_dir) = app.path().resource_dir() {
                    let prompted = setup_prompted.clone();
                    std::thread::spawn(move || {
                        msr_bridge::ensure_started(&resource_dir, &prompted);
                    });
                }
                Ok(())
            }
        })
        .on_window_event(move |window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if close_flag.load(Ordering::Relaxed) {
                    // 隐藏到托盘，采样线程继续在后台跑
                    let _ = window.hide();
                    api.prevent_close();
                }
            }
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
 *   cargo test diag_wmi -- --nocapture            # WMI 连通性（可加 WB_WMI_DIAG=1 看逐步 HRESULT）
 */

#[cfg(test)]
mod diag {
    fn flush(tag: &str) {
        use std::io::Write;
        print!("[{tag}] ");
        let _ = std::io::stdout().flush();
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

    /// PawnIO 直连是否可用：设备能否打开、模块能否载入、MSR 温度能否读出
    ///
    /// 需要有管理员权限才有意义——未提权时会在「打开设备」这一步就报访问被拒绝。
    #[test]
    fn pawnio_msr() {
        let module = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("pawnio")
            .join("IntelMSR.bin");
        println!(
            "module = {} ({})",
            module.display(),
            if module.exists() { "存在" } else { "缺失" }
        );

        match std::fs::read(&module) {
            Ok(blob) => match super::pawnio::IntelTempReader::open(&blob) {
                Ok(reader) => {
                    println!("tjmax         = {:.0} °C", reader.tjmax());
                    println!("package temp  = {:?} °C", reader.package_temp());
                }
                Err(e) => println!("打开 / 载入失败：{e}"),
            },
            Err(e) => println!("读取模块文件失败：{e}"),
        }

        match super::msr_bridge::read_fresh_state() {
            Some(st) => println!("bridge state  = {st:?}"),
            None => println!("bridge state  = 无新鲜读数（桥接进程未在运行）"),
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
        println!("info  = {:?}", super::battery::collect());
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
