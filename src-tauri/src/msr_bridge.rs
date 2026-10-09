//! CPU 温度提权桥接进程
//!
//! 读 MSR 需要管理员权限（PawnIO 设备只对管理员开放），但主程序是普通权限运行的。
//! 所以这里把「读 MSR」拆成一个**独立的提权子进程**：仍然是本 exe，只是带上
//! `--pawnio-bridge` 参数用 `runas` 重新拉起（隐藏窗口），它每秒读一次温度，
//! 写进一个几百字节的 JSON 文件；主进程（普通权限）直接读该文件即可。
//!
//! 为什么用文件而不是管道：`ShellExecuteExW` + `runas` **无法重定向 stdout**，
//! 传文件是最省事、也最不容易出问题的跨权限数据通道（覆盖写 + 原子改名）。
//!
//! 生命周期：子进程每秒检查父进程是否还活着（`OpenProcess`），父进程一退出就自行收尾，
//! 因此**不需要**跨权限 `TerminateProcess`——那在普通权限下大概率会失败。
//!
//! 关于 PawnIO 的安装（`--install-if-missing`）：读温度必须先有 PawnIO 内核驱动，
//! 而装驱动要弹 UAC。为了**不白弹 UAC、也绝不静默装驱动**，规则是：
//! * 启动时若探测到驱动缺失 → **不拉起**桥接进程，改由界面引导用户点击「安装 PawnIO」；
//! * 用户点击后，本次拉起会带上 `--install-if-missing`（= 已获用户授权），
//!   桥接进程**此刻已经是管理员**，于是直接跑随包安装器（`-install -silent`），
//!   装完继续读温度——**整个过程只弹一次 UAC**。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::pawnio::IntelTempReader;
use crate::pawnio_setup::{self, Probe};

/// 子进程模式开关
pub const BRIDGE_FLAG: &str = "--pawnio-bridge";
/// 「已获用户授权，可静默安装 PawnIO」——只由 `install_and_start` 带上
const INSTALL_FLAG: &str = "--install-if-missing";
/// 读数超过这个时长没更新就视为「桥接已停」
const STALE_AFTER: Duration = Duration::from_secs(8);

/// 桥接进程写出的状态（主进程只读）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MsrState {
    /// Unix 秒，用于判断读数是否还新鲜
    pub ts: u64,
    /// 写出这份状态的桥接进程所服务的父进程 PID（用来识别上一轮的残留文件）
    pub parent_pid: u32,
    pub cpu_temp_c: Option<f32>,
    pub tjmax_c: Option<f32>,
    /// 数据来源描述，直接展示在界面上
    pub source: String,
    /// 失败原因（成功时为 None）
    pub error: Option<String>,
}

impl MsrState {
    fn ok(temp: f32, tjmax: f32) -> Self {
        Self {
            ts: now_secs(),
            parent_pid: parent_pid_from_env(),
            cpu_temp_c: Some(temp),
            tjmax_c: Some(tjmax),
            source: "Intel MSR (PawnIO)".to_string(),
            error: None,
        }
    }

    fn err(msg: String) -> Self {
        Self {
            ts: now_secs(),
            parent_pid: parent_pid_from_env(),
            cpu_temp_c: None,
            tjmax_c: None,
            source: "Intel MSR (PawnIO)".to_string(),
            error: Some(msg),
        }
    }
}

/// 子进程把父进程 PID 记在环境变量里，供 `MsrState` 填字段（避免到处传参）
const PARENT_ENV: &str = "WB_PAWNIO_PARENT";

fn parent_pid_from_env() -> u32 {
    std::env::var(PARENT_ENV)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 主进程侧的共享目录
fn local_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("WinMonitor")
}

/// 桥接状态文件路径
pub fn state_path() -> PathBuf {
    local_dir().join("pawnio.json")
}

/// 读一份**新鲜**的状态；文件不存在、解析失败或已过期都返回 None
pub fn read_fresh_state() -> Option<MsrState> {
    let text = std::fs::read_to_string(state_path()).ok()?;
    let st: MsrState = serde_json::from_str(&text).ok()?;
    let age = now_secs().saturating_sub(st.ts);
    (age <= STALE_AFTER.as_secs()).then_some(st)
}

/* ------------------------------ 子进程模式 ------------------------------ */

/// 若命令行带 `--pawnio-bridge` 就进桥接模式（不启动 Tauri），返回值表示是否已接管
pub fn run_if_requested() -> bool {
    let args: Vec<String> = std::env::args().collect();
    if !args.iter().any(|a| a == BRIDGE_FLAG) {
        return false;
    }
    let get = |key: &str| -> Option<String> {
        args.iter()
            .position(|a| a == key)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let module = get("--module").map(PathBuf::from).unwrap_or_default();
    let out = get("--out").map(PathBuf::from).unwrap_or_else(state_path);
    // 先写进环境变量，让 MsrState 能把 parentPid 一起带上
    if let Some(pid) = get("--parent-pid") {
        std::env::set_var(PARENT_ENV, &pid);
    }
    let parent = get("--parent-pid")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);
    // 带这个开关意味着「用户已同意安装 PawnIO」，见文件开头的说明
    let install_if_missing = args.iter().any(|a| a == INSTALL_FLAG);

    run_bridge(&module, &out, parent, install_if_missing);
    true
}

/// 桥接主循环：读温度 → 写状态 → 父进程没了就退出
fn run_bridge(module: &Path, out: &Path, parent_pid: u32, install_if_missing: bool) {
    // 非 Intel 平台读这些 MSR 没有意义，直接给出明确结论，避免反复无效尝试
    if !is_intel_cpu() {
        let st = MsrState::err("当前 CPU 不是 Intel，MSR 直读暂不支持（已回退 ACPI / PDH 温度源）".to_string());
        write_state(out, &st);
    }

    // 用户点了「安装 PawnIO」才会走到这里装；此刻本进程已是管理员，无需二次 UAC
    install_pawnio_if_needed(module, install_if_missing);

    let mut reader: Option<IntelTempReader> = None;
    let mut last_err = String::from("尚未开始读取");

    loop {
        if !process_alive(parent_pid) {
            break;
        }

        if reader.is_none() && is_intel_cpu() {
            match std::fs::read(module) {
                Ok(blob) => match IntelTempReader::open(&blob) {
                    Ok(r) => {
                        log_err(&format!("模块已载入，TjMax = {:.0} °C", r.tjmax()));
                        reader = Some(r);
                    }
                    Err(e) => last_err = explain_open_failure(e),
                },
                Err(e) => {
                    last_err = format!("读取温度模块失败：{e}（{}）", module.display());
                }
            }
        }

        let state = match reader.as_ref() {
            Some(r) => match r.package_temp() {
                Ok(t) => MsrState::ok(t, r.tjmax()),
                Err(e) => {
                    last_err = e;
                    MsrState::err(last_err.clone())
                }
            },
            None => MsrState::err(last_err.clone()),
        };
        write_state(out, &state);

        std::thread::sleep(Duration::from_millis(1000));
    }
}

/// 设备打开失败时，用探测结果把原因说具体——「没装驱动」是可操作的，值得单独提示
fn explain_open_failure(raw: String) -> String {
    match pawnio_setup::probe() {
        Probe::Missing => {
            "未安装 PawnIO 内核驱动，无法读取 CPU 温度；请在界面上点击「安装 PawnIO」".to_string()
        }
        _ => raw,
    }
}

/// 仅在「用户已授权」且驱动确实缺失时安装 PawnIO。
///
/// 本进程已经提权，所以安装器以 `elevate = false` 直接运行，不会再有第二次 UAC。
/// 安装器就在温度模块旁边（`<resource_dir>/pawnio/`），不用额外传参。
fn install_pawnio_if_needed(module: &Path, allowed: bool) {
    if !allowed || pawnio_setup::probe().installed() {
        return;
    }
    let Some(setup) = module
        .parent()
        .map(|dir| dir.join(pawnio_setup::SETUP_FILE_NAME))
    else {
        log_err("无法定位随包 PawnIO 安装器：温度模块路径没有父目录");
        return;
    };
    log_err(&format!("未检测到 PawnIO，开始静默安装：{}", setup.display()));
    match pawnio_setup::run_installer(&setup, false) {
        Ok(outcome) => log_err(&format!("PawnIO 安装结果：{}", outcome.describe())),
        Err(e) => log_err(&format!("PawnIO 安装失败：{e}")),
    }
}

/// 覆盖写状态文件：先写临时文件再原子改名，主进程不会读到半截内容
fn write_state(out: &Path, state: &MsrState) {
    let Some(dir) = out.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let Ok(text) = serde_json::to_string(state) else {
        return;
    };
    let tmp = out.with_extension("json.tmp");
    if std::fs::write(&tmp, text).is_err() {
        return;
    }
    // Windows 上 std 的 rename 走 MOVEFILE_REPLACE_EXISTING，可覆盖已有文件
    if let Err(e) = std::fs::rename(&tmp, out) {
        log_err(&format!("写入状态文件失败：{e}"));
    }
}

/// 父进程是否还活着（PID 为 0 时无从判断，按「活着」处理）
fn process_alive(pid: u32) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    if pid == 0 {
        return true;
    }
    // 提权进程去打开普通权限的父进程是允许的（高完整性级别可以打开低级别）
    match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) } {
        Ok(handle) => {
            let _ = unsafe { CloseHandle(handle) };
            true
        }
        Err(_) => false,
    }
}

/// 依据 CPU 品牌粗判是否 Intel（`brand` 形如 "12th Gen Intel(R) Core(TM) i9-12900K"）
fn is_intel_cpu() -> bool {
    let mut sys = sysinfo::System::new_all();
    sys.refresh_cpu_all();
    sys.cpus()
        .first()
        .map(|c| c.brand().to_ascii_lowercase().contains("intel"))
        .unwrap_or(false)
}

/* ------------------------------ 主进程侧控制 ------------------------------ */

/// 启动时按需拉起桥接进程：**一个会话只尝试一次**（避免反复弹 UAC）。
///
/// 未装 PawnIO 时**不拉起**——拉起来只会报错、还白弹一次 UAC；
/// 这种情况改由界面引导用户点「安装 PawnIO」（见 [`install_and_start`]）。
pub fn ensure_started(resource_dir: &Path, prompted: &AtomicBool) {
    if prompted.swap(true, Ordering::Relaxed) {
        return;
    }
    // 已经有一份属于**本次运行**的新鲜读数 → 说明桥接已在工作，不必再拉
    if let Some(st) = read_fresh_state() {
        if st.parent_pid == std::process::id() {
            return;
        }
    }
    if pawnio_setup::probe() == Probe::Missing {
        log_err("未检测到 PawnIO，暂不拉起桥接进程（等待用户在界面上确认安装）");
        return;
    }
    launch_bridge(resource_dir, false);
}

/// 用户点了「安装 PawnIO」：强制拉起桥接进程，并**授权它顺手静默安装驱动**。
///
/// 安装与读取由同一个提权子进程完成，所以全程只弹一次 UAC。
pub fn install_and_start(resource_dir: &Path) {
    // 已经装好（例如用户自己装过）就按普通路径拉起，别再去跑安装器
    launch_bridge(resource_dir, !pawnio_setup::probe().installed());
}

/// 用 `runas` 提权 + 隐藏窗口拉起自己（`nShow = 0` 即 SW_HIDE）
fn launch_bridge(resource_dir: &Path, install_if_missing: bool) {
    let Ok(exe) = std::env::current_exe() else {
        log_err("取不到自身路径，无法拉起桥接进程");
        return;
    };
    let module = resource_dir.join("pawnio").join("IntelMSR.bin");
    let out = state_path();
    let extra = if install_if_missing {
        format!(" {INSTALL_FLAG}")
    } else {
        String::new()
    };
    let params = format!(
        "{BRIDGE_FLAG} --module \"{}\" --out \"{}\" --parent-pid {}{extra}",
        module.display(),
        out.display(),
        std::process::id()
    );

    // 用户点了「否」时 ShellExecuteExW 返回 ERROR_CANCELLED，此处静默放过：
    // 温度会回退到 ACPI / PDH，界面另有提示。
    if let Err(e) = launch_elevated_hidden(&exe, &params) {
        log_err(&format!("拉起提权桥接进程失败：{e}"));
    }
}

/// 用 `runas` 提权 + 隐藏窗口拉起自己（`nShow = 0` 即 SW_HIDE）
fn launch_elevated_hidden(exe: &Path, params: &str) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Shell::{ShellExecuteExW, SHELLEXECUTEINFOW, SEE_MASK_NOCLOSEPROCESS};

    let exe_w: Vec<u16> = exe
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let verb: Vec<u16> = "runas".encode_utf16().chain(std::iter::once(0)).collect();
    let args_w: Vec<u16> = params.encode_utf16().chain(std::iter::once(0)).collect();

    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        hwnd: HWND(std::ptr::null_mut()),
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(exe_w.as_ptr()),
        lpParameters: PCWSTR(args_w.as_ptr()),
        lpDirectory: PCWSTR::null(),
        // Default 里 nShow = 0（SW_HIDE）：子进程不弹窗口
        ..Default::default()
    };

    unsafe { ShellExecuteExW(&mut info) }.map_err(|e| e.to_string())?;
    if !info.hProcess.is_invalid() {
        let _ = unsafe { windows::Win32::Foundation::CloseHandle(info.hProcess) };
    }
    Ok(())
}

/* ------------------------------ 落盘日志 ------------------------------ */

/// 桥接进程没有控制台，出错只能落盘：%LOCALAPPDATA%\WinMonitor\pawnio.log
fn log_err(msg: &str) {
    let dir = local_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("pawnio.log"))
    {
        use std::io::Write;
        let _ = writeln!(f, "[{}] {msg}", now_secs());
    }
}
