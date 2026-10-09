//! PawnIO 直连：借 PawnIO 内核驱动读 Intel CPU 的 MSR，取回核心 / 封装温度
//!
//! Windows 没有公开的 CPU 温度接口——温度只存在于片上 DTS 寄存器里，必须经内核驱动读 MSR。
//! PawnIO（<https://pawnio.eu>）是一个通用 IO 驱动：它自己不含硬件逻辑，而是执行**已签名的
//! 模块字节码**。这里沿用 LibreHardwareMonitor 同款的官方模块 `IntelMSR.bin`
//! （来源与许可见 `resources/pawnio/NOTICE.md`），只用到其中一个函数：
//!
//! ```text
//! "ioctl_read_msr"(msr_index) -> eax | (edx << 32)
//! ```
//!
//! 三个关键 MSR：
//! * `0x1A2` IA32_TEMPERATURE_TARGET —— bits 23:16 = TjMax（温度上限）
//! * `0x1B1` IA32_PACKAGE_THERM_STATUS —— bit31 = 读数有效，bits 22:16 = 封装 DTS
//! * `0x19C` IA32_THERM_STATUS —— 同上，但只代表**当前所在核心**
//!
//! 温度 = TjMax − DTS。DTS 的含义是「距上限还有多少度」，所以必须做减法。
//!
//! 驱动设备只对管理员开放，所以读取实际发生在提权子进程里，见 `msr_bridge.rs`。

use std::ffi::c_void;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Threading::{GetCurrentThread, SetThreadAffinityMask};

/// PawnIO 驱动设备名（`GLOBALROOT` 前缀可绕过 Win32 路径规范化与目录重定向）
const DEVICE_PATH: &str = r"\\?\GLOBALROOT\Device\PawnIO";
/// DEVICE_TYPE 由 PawnIO 头文件固化（`PawnIOLib.h` 里就是 41394）
const DEVICE_TYPE: u32 = 41394 << 16;
/// 载入模块字节码
const IOCTL_PIO_LOAD_BINARY: u32 = (0x821 << 2) | DEVICE_TYPE;
/// 执行模块内函数
const IOCTL_PIO_EXECUTE_FN: u32 = (0x841 << 2) | DEVICE_TYPE;
/// 函数名固定占 32 字节，其后紧接 i64 参数数组
const FN_NAME_LENGTH: usize = 32;

const MSR_TEMPERATURE_TARGET: u32 = 0x1A2;
const MSR_PACKAGE_THERM_STATUS: u32 = 0x1B1;
const MSR_THERM_STATUS: u32 = 0x19C;

/// 合理温度区间：越界说明解码错了，宁可报错也不显示垃圾值
const TEMP_MIN: f32 = 1.0;
const TEMP_MAX: f32 = 125.0;
/// TjMax 读不到（或读到明显不合理的值）时的兜底，Intel 近代平台基本都按 100 °C
const TJMAX_FALLBACK: f32 = 100.0;
/// 逻辑核心遍历上限，防止亲和性探测死循环
const MAX_LOGICAL_CPU: u32 = 64;

/// 已打开的 PawnIO 设备（模块已载入）。Drop 时关闭句柄。
pub struct PawnIo {
    handle: HANDLE,
}

impl PawnIo {
    /// 打开设备并载入模块字节码
    pub fn open(module: &[u8]) -> Result<Self, String> {
        let path: Vec<u16> = DEVICE_PATH.encode_utf16().chain(Some(0)).collect();
        // GENERIC_READ(0x80000000) | GENERIC_WRITE(0x40000000)
        let handle = unsafe {
            CreateFileW(
                PCWSTR(path.as_ptr()),
                0xC000_0000,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
        }
        .map_err(|e| format!("打开 PawnIO 设备失败（未安装 PawnIO，或进程未提权）：{e}"))?;

        let dev = Self { handle };
        unsafe {
            DeviceIoControl(
                dev.handle,
                IOCTL_PIO_LOAD_BINARY,
                Some(module.as_ptr() as *const c_void),
                module.len() as u32,
                None,
                0,
                None,
                None,
            )
        }
        .map_err(|e| format!("载入 PawnIO 模块失败（模块未签名或与驱动版本不匹配）：{e}"))?;

        Ok(dev)
    }

    /// 调用模块内函数。入参是 i64 数组，返回也是 i64 数组。
    fn execute(&self, name: &str, args: &[i64], out_len: usize) -> Result<Vec<i64>, String> {
        let mut input = vec![0u8; FN_NAME_LENGTH + args.len() * 8];
        let bytes = name.as_bytes();
        let n = bytes.len().min(FN_NAME_LENGTH - 1);
        input[..n].copy_from_slice(&bytes[..n]);
        for (i, v) in args.iter().enumerate() {
            let at = FN_NAME_LENGTH + i * 8;
            input[at..at + 8].copy_from_slice(&v.to_le_bytes());
        }

        let mut output = vec![0u8; out_len * 8];
        let mut read: u32 = 0;
        unsafe {
            DeviceIoControl(
                self.handle,
                IOCTL_PIO_EXECUTE_FN,
                Some(input.as_ptr() as *const c_void),
                input.len() as u32,
                Some(output.as_mut_ptr() as *mut c_void),
                output.len() as u32,
                Some(&mut read),
                None,
            )
        }
        .map_err(|e| format!("执行 {name} 失败：{e}"))?;

        let got = (read as usize / 8).min(out_len);
        Ok((0..got)
            .map(|i| {
                let at = i * 8;
                i64::from_le_bytes(output[at..at + 8].try_into().unwrap_or([0; 8]))
            })
            .collect())
    }

    /// 读一个 MSR，返回 64 位原值（低 32 位 eax、高 32 位 edx）
    pub fn read_msr(&self, index: u32) -> Result<u64, String> {
        let out = self.execute("ioctl_read_msr", &[index as i64], 1)?;
        out.first()
            .map(|v| *v as u64)
            .ok_or_else(|| "ioctl_read_msr 未返回数据".to_string())
    }
}

impl Drop for PawnIo {
    fn drop(&mut self) {
        if !self.handle.is_invalid() {
            let _ = unsafe { CloseHandle(self.handle) };
        }
    }
}

/// Intel CPU 温度读取器（持有 PawnIO 句柄 + TjMax）
pub struct IntelTempReader {
    dev: PawnIo,
    tjmax: f32,
}

impl IntelTempReader {
    pub fn open(module: &[u8]) -> Result<Self, String> {
        let dev = PawnIo::open(module)?;
        let tjmax = match dev.read_msr(MSR_TEMPERATURE_TARGET) {
            Ok(v) => {
                let t = ((v >> 16) & 0xFF) as f32;
                if t >= 60.0 {
                    t
                } else {
                    TJMAX_FALLBACK
                }
            }
            // 读不到 TjMax 不算致命：封装温度仍可由 0x1B1 给出
            Err(_) => TJMAX_FALLBACK,
        };
        Ok(Self { dev, tjmax })
    }

    pub fn tjmax(&self) -> f32 {
        self.tjmax
    }

    /// CPU 温度：优先整包（0x1B1），取不到再逐核心读 0x19C 取最高
    pub fn package_temp(&self) -> Result<f32, String> {
        match self.dev.read_msr(MSR_PACKAGE_THERM_STATUS) {
            Ok(v) => {
                if let Some(t) = decode_dts(v, self.tjmax) {
                    return Ok(t);
                }
            }
            Err(e) => {
                // 0x1B1 不被支持（老平台）属正常，继续走核心遍历
                log_debug(&format!("读 0x1B1 失败：{e}"));
            }
        }
        self.max_core_temp()
    }

    /// 逐逻辑核心读 0x19C。
    ///
    /// MSR 是**每核心**的：不切线程亲和性的话，读到的永远是当前那颗核。
    /// `SetThreadAffinityMask` 返回上一个掩码，返回 0 表示绑核失败（该核不存在）。
    fn max_core_temp(&self) -> Result<f32, String> {
        let thread = unsafe { GetCurrentThread() };
        let original = unsafe { SetThreadAffinityMask(thread, 1) };
        if original == 0 {
            return Err("设置线程亲和性失败".to_string());
        }

        let mut best: Option<f32> = None;
        let mut saw_valid_msr = false;
        for i in 0..MAX_LOGICAL_CPU {
            let prev = unsafe { SetThreadAffinityMask(thread, 1usize << i) };
            if prev == 0 {
                break; // 第 i 颗逻辑核不存在
            }
            if let Ok(v) = self.dev.read_msr(MSR_THERM_STATUS) {
                saw_valid_msr = true;
                if let Some(t) = decode_dts(v, self.tjmax) {
                    best = Some(best.map_or(t, |b: f32| b.max(t)));
                }
            }
            unsafe { SetThreadAffinityMask(thread, prev) };
        }
        unsafe { SetThreadAffinityMask(thread, original) };

        match (best, saw_valid_msr) {
            (Some(t), _) => Ok(t),
            (None, true) => Err("MSR 读到了，但 DTS 读数位显示无效".to_string()),
            (None, false) => Err("读 0x19C 失败：PawnIO 模块不可用".to_string()),
        }
    }
}

/// 解 IA32_*_THERM_STATUS：bit31 = 读数有效，bits 22:16 = DTS
fn decode_dts(value: u64, tjmax: f32) -> Option<f32> {
    if value & (1 << 31) == 0 {
        return None;
    }
    let dts = ((value >> 16) & 0x7F) as f32;
    let t = tjmax - dts;
    (TEMP_MIN..=TEMP_MAX).contains(&t).then_some(t)
}

/// 排错用流水账：仅在 `WB_PAWNIO_DIAG=1` 时输出
fn log_debug(msg: &str) {
    if std::env::var("WB_PAWNIO_DIAG").as_deref() == Ok("1") {
        eprintln!("[pawnio] {msg}");
    }
}
