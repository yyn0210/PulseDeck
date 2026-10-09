//! PawnIO 的探测与安装
//!
//! 读 CPU 核心温度必须先有 **PawnIO 内核驱动**（见 `pawnio.rs`）。驱动不是普通依赖——
//! 它是内核组件，安装会弹 UAC，所以本模块只做两件事，且**安装一律由用户显式触发**：
//!
//! * [`probe`] —— 用「能不能打开 PawnIO 设备」判断驱动是否可用；
//! * [`run_installer`] —— 运行**随包原样分发**的官方安装器（`-install -silent`）。
//!
//! 为什么「打开设备」就能判断是否已安装：设备对象不存在时 `CreateFileW` 报
//! `ERROR_FILE_NOT_FOUND`(2)，设备存在但进程权限不足时报 `ERROR_ACCESS_DENIED`(5)。
//! 本机实测确认了这两种失败码的区分度（见项目记忆 2026-10-09）。
//!
//! 安装器来自 <https://github.com/namazso/PawnIO.Setup> 发布版 2.2.0，
//! 按 <https://pawnio.eu> 的许可原样再分发（「can be redistributed unmodified」），
//! 详见 `resources/pawnio/NOTICE.md`。

use std::path::{Path, PathBuf};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HWND, WAIT_OBJECT_0};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
use windows::Win32::UI::Shell::{
    ShellExecuteExW, SHELLEXECUTEINFOW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS,
};

/// PawnIO 驱动设备名（与 `pawnio.rs` 保持一致）
const DEVICE_PATH: &str = r"\\?\GLOBALROOT\Device\PawnIO";
/// 随包分发的官方安装器文件名。**不要改名 / 改动内容**，见 NOTICE.md
pub const SETUP_FILE_NAME: &str = "PawnIO_setup.exe";
/// `-silent` 让安装器完全不画界面；`-install` 是官方唯一的安装指令
const INSTALL_ARGS: &str = "-install -silent";

/// 安装器正常结束的等待上限：官方安装只需数秒，留足余量即可
const INSTALL_TIMEOUT_MS: u32 = 180_000;

// Win32 错误码（`ERROR_*`）
const ERR_FILE_NOT_FOUND: u32 = 2;
const ERR_PATH_NOT_FOUND: u32 = 3;
const ERR_ACCESS_DENIED: u32 = 5;
const ERR_CANCELLED: u32 = 1223;
/// 安装类操作「成功，但需要重启才生效」
const ERR_SUCCESS_REBOOT_REQUIRED: u32 = 3010;

/// PawnIO 设备的探测结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    /// 设备打开成功：已安装，且当前进程有管理员权限
    Ready,
    /// 设备存在但拒绝访问：已安装，只是当前进程权限不够（普通权限主进程的常态）
    Installed,
    /// 设备不存在：未安装
    Missing,
    /// 其他失败原因（附带原文，便于排错）
    Failed(String),
}

impl Probe {
    /// `Ready` 与 `Installed` 都代表驱动已装好，只是权限不同
    pub fn installed(&self) -> bool {
        matches!(self, Probe::Ready | Probe::Installed)
    }

    /// 面向界面的一句话说明
    pub fn describe(&self) -> String {
        match self {
            Probe::Ready => "PawnIO 已安装且当前进程可直接访问".to_string(),
            Probe::Installed => "PawnIO 已安装".to_string(),
            Probe::Missing => "未安装 PawnIO（读取 CPU 核心温度所需的内核驱动）".to_string(),
            Probe::Failed(e) => format!("PawnIO 设备访问失败：{e}"),
        }
    }
}

/// 探测 PawnIO 是否可用。不要缓存结果——用户随时可能装上或卸掉驱动。
pub fn probe() -> Probe {
    let path: Vec<u16> = DEVICE_PATH.encode_utf16().chain(std::iter::once(0)).collect();
    // GENERIC_READ(0x8000_0000) | GENERIC_WRITE(0x4000_0000)：与读取时用同样的权限试
    match unsafe {
        CreateFileW(
            PCWSTR(path.as_ptr()),
            0xC000_0000,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    } {
        Ok(handle) => {
            // 只是探测，拿到句柄即可关掉
            let _ = unsafe { CloseHandle(handle) };
            Probe::Ready
        }
        Err(e) => match win32_code(&e) {
            Some(ERR_ACCESS_DENIED) => Probe::Installed,
            Some(ERR_FILE_NOT_FOUND) | Some(ERR_PATH_NOT_FOUND) => Probe::Missing,
            _ => Probe::Failed(e.to_string()),
        },
    }
}

/// 随包安装器的绝对路径（`<resource_dir>/pawnio/PawnIO_setup.exe`）
pub fn setup_path(resource_dir: &Path) -> PathBuf {
    resource_dir.join("pawnio").join(SETUP_FILE_NAME)
}

/// 安装器运行结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOutcome {
    /// 安装成功，立即可用
    Done,
    /// 安装成功，但需重启系统才完全生效
    RebootRequired,
    /// 用户在 UAC 弹窗上点了「否」
    Cancelled,
    /// 安装器自己报错（随附原始退出码）
    Failed(u32),
}

impl InstallOutcome {
    /// 面向界面的一句话说明
    pub fn describe(&self) -> String {
        match self {
            InstallOutcome::Done => "PawnIO 已安装".to_string(),
            InstallOutcome::RebootRequired => "PawnIO 已安装，重启系统后生效".to_string(),
            InstallOutcome::Cancelled => "已取消安装 PawnIO".to_string(),
            InstallOutcome::Failed(code) => format!(
                "PawnIO 安装失败（安装器退出码 {code}）；若系统已装旧版 PawnIO，需先卸载旧版再安装"
            ),
        }
    }
}

/// 运行随包安装器并等待结束。
///
/// * `elevate = true` —— 用 `runas` 拉起，会弹 UAC（主进程普通权限时用）；
/// * `elevate = false` —— 直接运行（已经在提权进程里时用，避免二次 UAC）。
pub fn run_installer(setup: &Path, elevate: bool) -> Result<InstallOutcome, String> {
    use std::os::windows::ffi::OsStrExt;

    if !setup.is_file() {
        return Err(format!("未找到随包 PawnIO 安装器：{}", setup.display()));
    }

    let file_w: Vec<u16> = setup
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let params_w: Vec<u16> = INSTALL_ARGS
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let verb: Vec<u16> = if elevate { "runas" } else { "open" }
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        // NOASYNC：保证返回时进程已创建；NOCLOSEPROCESS：拿句柄以便等它结束
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        hwnd: HWND(std::ptr::null_mut()),
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(file_w.as_ptr()),
        lpParameters: PCWSTR(params_w.as_ptr()),
        lpDirectory: PCWSTR::null(),
        // SW_HIDE：`-silent` 已让安装器不画界面，这里再兜一层
        nShow: 0,
        ..Default::default()
    };

    if let Err(e) = unsafe { ShellExecuteExW(&mut info) } {
        // 用户在 UAC 上点「否」时是这个错误码，属正常分支而非故障
        if win32_code(&e) == Some(ERR_CANCELLED) {
            return Ok(InstallOutcome::Cancelled);
        }
        return Err(format!("启动 PawnIO 安装器失败：{e}"));
    }
    if info.hProcess.is_invalid() {
        return Err("启动 PawnIO 安装器失败：未取到进程句柄".to_string());
    }

    let waited = unsafe { WaitForSingleObject(info.hProcess, INSTALL_TIMEOUT_MS) };
    let mut code: u32 = 0;
    let got_code = unsafe { GetExitCodeProcess(info.hProcess, &mut code) }.is_ok();
    let _ = unsafe { CloseHandle(info.hProcess) };

    if waited != WAIT_OBJECT_0 {
        return Err("PawnIO 安装器超时未结束".to_string());
    }
    if !got_code {
        return Err("读取 PawnIO 安装器退出码失败".to_string());
    }

    Ok(match code {
        0 => InstallOutcome::Done,
        ERR_SUCCESS_REBOOT_REQUIRED => InstallOutcome::RebootRequired,
        other => InstallOutcome::Failed(other),
    })
}

/// 从 `windows` crate 的错误里取出底层 Win32 错误码。
///
/// 这些 API 的失败是以 `HRESULT_FROM_WIN32(code)`（`0x8007_0000 | code`）上报的，
/// 所以低 16 位就是原始 `ERROR_*` 码。
fn win32_code(e: &windows::core::Error) -> Option<u32> {
    let hr = e.code().0 as u32;
    (hr & 0xFFFF_0000 == 0x8007_0000).then_some(hr & 0xFFFF)
}
