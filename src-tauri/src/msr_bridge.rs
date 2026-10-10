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
//! 生命周期：子进程**启动时打开一次父进程句柄并一直持有**，之后每秒用
//! `WaitForSingleObject` 等待它——父进程一退出（进程对象被置为有信号态）就自行收尾，
//! 因此**不需要**跨权限 `TerminateProcess`——那在普通权限下大概率会失败。
//!
//! ⚠️ **不要退回「`OpenProcess` 打不开 = 父进程已退出」的判据**：进程退出后只要有别的
//! 句柄还引用它的进程对象（WebView2 之类很常见），`OpenProcess(父PID)` 会**继续成功**，
//! 于是子进程永远以为父进程还活着、应用关了它还在后台跑（本项目 2026-10-10 修的
//! 「残留进程」就是这个）。详见 [`ParentWatch`]。
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

use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_TIMEOUT};
use windows::Win32::System::Threading::{
    OpenProcess, WaitForSingleObject, PROCESS_ACCESS_RIGHTS, PROCESS_QUERY_LIMITED_INFORMATION,
};

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
    /// 联想机型上的风扇转速：`FanID -> RPM`。
    ///
    /// 之所以搭桥接的便车：联想的 `LENOVO_FAN_METHOD` 实测**也要管理员权限**
    /// （非提权调用返回 `WBEM_E_ACCESS_DENIED`），正好与 MSR 温度共用同一次 UAC。
    /// 非联想机型为 `None`（不写字段，省得 JSON 里塞空对象）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lenovo_fans: Option<Vec<LenovoFan>>,
}

/// 一个联想风扇读数（桥接 → 主进程）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LenovoFan {
    /// 联想逻辑风扇号：0 = CPU，1 = GPU
    pub fan_id: u8,
    /// 转速（RPM）
    pub rpm: f32,
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
            lenovo_fans: None,
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
            lenovo_fans: None,
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

/// 只取联想风扇读数（`FanID -> RPM`），供 `vendor.rs` 使用。
///
/// 返回 `None` 表示「桥接没在跑 / 读数过期 / 本机不是联想」——三种情况上层
/// 都按「厂商通道无数据」处理，不需要区分。
pub fn read_fresh_fans() -> Option<Vec<LenovoFan>> {
    read_fresh_state()?.lenovo_fans
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
    // 先守望父进程：拿不到句柄（父进程已经不在了 / 权限不足）就立刻收尾。
    // 看不住父进程的桥接进程只会变成没人管的孤儿，不如不跑。
    let Some(parent) = ParentWatch::attach(parent_pid) else {
        log_err(&format!(
            "无法打开父进程句柄（pid={parent_pid}），桥接进程立即退出"
        ));
        return;
    };

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

        let mut state = match reader.as_ref() {
            Some(r) => match r.package_temp() {
                Ok(t) => MsrState::ok(t, r.tjmax()),
                Err(e) => {
                    last_err = e;
                    MsrState::err(last_err.clone())
                }
            },
            None => MsrState::err(last_err.clone()),
        };
        // 风扇与温度彼此独立，各自采各自的；合进同一份状态文件（同一次 UAC 的收益）
        state.lenovo_fans = sample_lenovo_fans();
        write_state(out, &state);

        // 这一等既当采样节拍（1 Hz），也是「父进程是否还活着」的唯一判据：
        // 父进程一结束，它的进程对象即被置为有信号态，这里立刻返回 false 并收尾。
        if !parent.alive_after(1000) {
            break;
        }
    }
}

/* --------------------------- 父进程守望（句柄等待法） --------------------------- */

/// `SYNCHRONIZE`（0x0010_0000）：`windows` crate 只导出了 FileSystem 的版本，
/// 进程访问权里没有，这里自己补一个——有了它才能对进程句柄 `WaitForSingleObject`。
const SYNCHRONIZE: PROCESS_ACCESS_RIGHTS = PROCESS_ACCESS_RIGHTS(0x0010_0000);

/// 父进程的守望句柄。
///
/// **为什么不能每秒 `OpenProcess` 试探**：Windows 上进程退出后，只要还有任何句柄引用它的
/// 进程对象（实测：被 WebView2 之类留下句柄时很常见），`OpenProcess(pid)` 会**继续成功**，
/// 「打不开 = 已退出」这个判据就失效了——子进程会永远以为父进程还活着，应用关了它还赖在
/// 后台。本机实测：应用退出后 `OpenProcess(旧父PID, PROCESS_QUERY_LIMITED_INFORMATION)`
/// 仍返回有效句柄（err=0），而对一个不存在的 PID 才返回 `ERROR_INVALID_PARAMETER`(87)。
///
/// 正确做法是**启动时打开一次句柄并一直持有**：
/// * 句柄一直在手 → 进程对象不会被回收，PID 也不会被系统复用给别人，不存在「PID 复用」误判；
/// * 进程一结束，进程对象即被置为有信号态 → `WaitForSingleObject` 立刻返回
///   `WAIT_OBJECT_0`，与进程对象是否已被回收无关（正好覆盖上面的「僵尸句柄」情形）。
struct ParentWatch(HANDLE);

impl ParentWatch {
    /// 打开父进程句柄。返回 `None` 表示此刻就打不开（父进程已不在 / 权限不足）。
    fn attach(pid: u32) -> Option<Self> {
        if pid == 0 {
            return None;
        }
        let access = SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION;
        match unsafe { OpenProcess(access, false, pid) } {
            Ok(handle) if !handle.is_invalid() => Some(Self(handle)),
            _ => None,
        }
    }

    /// 至多等 `timeout_ms`：父进程**仍在运行**返回 `true`，已结束返回 `false`。
    ///
    /// 等待出错也返回 `false`——宁可少跑一个后台进程，也不要留孤儿。
    fn alive_after(&self, timeout_ms: u32) -> bool {
        matches!(
            unsafe { WaitForSingleObject(self.0, timeout_ms) },
            WAIT_TIMEOUT
        )
    }
}

impl Drop for ParentWatch {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// 桥接主循环（风扇部分）：联想机型上读一次所有风扇转速。
///
/// 单独抽出来是因为它**与 MSR 温度彼此独立**——非 Intel 平台照样可能有联想风扇，
/// 反之 Intel 机型若是其它品牌则没有联想风扇。
///
/// 说明：`wmi::lenovo_fans()` 内部依赖 `is_lenovo()` 与 `lenovo_fan_instance()`，
/// 它们各自有 `OnceLock` 缓存，不会每秒都去查 `Win32_ComputerSystem`。
fn sample_lenovo_fans() -> Option<Vec<LenovoFan>> {
    if !crate::wmi::is_lenovo() {
        return None;
    }
    let fans = crate::wmi::lenovo_fans();
    if fans.is_empty() {
        return None;
    }
    Some(
        fans.into_iter()
            .map(|(fan_id, rpm)| LenovoFan { fan_id, rpm })
            .collect(),
    )
}

#[cfg(test)]
impl MsrState {
    /// 测试用：构造一份只带风扇读数的最小状态（其余字段填占位值）
    pub fn for_test(fans: Option<Vec<LenovoFan>>) -> Self {
        Self {
            ts: now_secs(),
            parent_pid: 0,
            cpu_temp_c: None,
            tjmax_c: None,
            source: "test".to_string(),
            error: None,
            lenovo_fans: fans,
        }
    }
}

/// 测试用入口：暴露私有的 `sample_lenovo_fans`（提权诊断需要）
#[cfg(test)]
pub fn sample_lenovo_fans_for_test() -> Option<Vec<LenovoFan>> {
    sample_lenovo_fans()
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
///
/// 联想机型例外：即便没装 PawnIO，风扇转速也**必须**靠提权桥接才能读。
/// 但同样不在启动路径上弹 UAC（用户没点任何东西就弹框很唐突），
/// 改由界面提示里的「启用」按钮触发 [`launch_with_install`]。
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

/// 用户点了界面上的「启用」：无条件拉起桥接进程。
///
/// `install_if_missing` 为 `true` 时授权桥接顺手静默安装 PawnIO——
/// 只在调用方确认「安装器在 + 驱动缺失」时传 true，避免无谓地跑安装器。
pub fn launch_with_install(resource_dir: &Path, install_if_missing: bool) {
    launch_bridge(resource_dir, install_if_missing);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    /// 回归：被守望的进程结束之后，`ParentWatch` 必须立刻发现——**即使它的进程对象还没被回收**。
    ///
    /// 这个用例正是 2026-10-10 那个「应用关了还有残留进程」的复现条件：`std::process::Child`
    /// 自己持有一个进程句柄，所以子进程被杀掉之后它的进程对象**依然存在**，
    /// 「`OpenProcess` 打不开 = 已退出」的旧判据在这里恒为「还活着」；而句柄等待会立刻返回
    /// `WAIT_OBJECT_0`。**不要**把它改回 OpenProcess 试探。
    #[test]
    fn parent_watch_notices_exit_even_when_object_survives() {
        let mut child = Command::new("cmd")
            .args(["/c", "ping", "-n", "30", "127.0.0.1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("启动测试子进程失败");
        let pid = child.id();

        let watch = ParentWatch::attach(pid).expect("应能打开子进程句柄");
        assert!(watch.alive_after(0), "子进程仍在运行时应判定为存活");

        child.kill().expect("结束测试子进程失败");
        let _ = child.wait(); // 回收进程；此时进程对象尚未被回收（句柄还在我们手里）

        assert!(
            !watch.alive_after(1000),
            "子进程已结束，守望必须发现（否则就是「残留进程」bug 复发）"
        );
    }

    /// `attach` 对不存在的 PID 必须失败——否则桥接会对着一个空 PID 无限守望。
    #[test]
    fn attach_fails_for_missing_pid() {
        assert!(ParentWatch::attach(0).is_none());
        // 999999 在本机一定不存在（`OpenProcess` 返回 ERROR_INVALID_PARAMETER）
        assert!(ParentWatch::attach(999_999).is_none());
    }
}
