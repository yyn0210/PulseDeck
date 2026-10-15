//! PawnIO 直连：借 PawnIO 内核驱动读 CPU 温度（Intel 走 MSR、AMD 走 SMN）
//!
//! Windows 没有公开的 CPU 温度接口——Intel 的温度只存在于片内 DTS 寄存器（MSR），
//! AMD 的温度只在 SMN（System Management Network）空间里，都必须经内核驱动读。
//! PawnIO（<https://pawnio.eu>）是一个通用 IO 驱动：它自己不含硬件逻辑，而是执行
//! **已签名的模块字节码**。本工具随包分发官方发布的两个模块（来源与许可见
//! `resources/pawnio/NOTICE.md`）：
//!
//! | 厂商 | 模块 | 用到的函数 | 取温度的寄存器 |
//! | --- | --- | --- | --- |
//! | Intel | `IntelMSR.bin` | `ioctl_read_msr` | `0x1B1` 封装 / `0x19C` 逐核心（DTS） |
//! | AMD | `AMDFamily17.bin` | `ioctl_read_smn` | SMN `0x00059800` THM_TCON_CUR_TMP |
//!
//! ## Intel：MSR
//!
//! 三个关键 MSR：
//! * `0x1A2` IA32_TEMPERATURE_TARGET —— bits 23:16 = TjMax（温度上限）
//! * `0x1B1` IA32_PACKAGE_THERM_STATUS —— bit31 = 读数有效，bits 22:16 = 封装 DTS
//! * `0x19C` IA32_THERM_STATUS —— 同上，但只代表**当前所在核心**
//!
//! 温度 = TjMax − DTS。DTS 的含义是「距上限还有多少度」，所以必须做减法。
//!
//! ## AMD：SMN
//!
//! AMD 这条路没有对应的 MSR：温度在 SMN 空间（`0x00059800` = THM_TCON_CUR_TMP，
//! `CUR_TEMP [31:21]` 以 0.125 °C 为单位），而 SMN 在 PCI 配置空间里用一对
//! 索引 / 数据寄存器间接访问（模块内部写 0:0.0 的 `0x60`、读 `0x64`）——
//! 那是**全机共享**的资源，所以读之前必须先拿 `Global\Access_PCI` 互斥体
//! （见 [`PciBusGuard`]）。
//!
//! 模块只支持 **Zen 架构（family 17h~1Ah）**：`AMDFamily17.bin` 载入时自己会用
//! CPUID 校验，非 AMD 或家族不符一律以 `STATUS_NOT_SUPPORTED` 拒绝载入。
//! 封装功耗（RAPL）也走同一个模块：`MSR_PWR_UNIT (0xC0010299)` 给出能量单位、
//! `MSR_PKG_ENERGY_STAT (0xC001029B)` 是不断累加的能量计数器，两拍求差即得瓦数
//! （Windows 的 PDH `Energy Meter` 只存在于 Intel 平台，AMD 上拿不到）。
//!
//! 驱动设备只对管理员开放，所以读取实际发生在提权子进程里，见 `msr_bridge.rs`。

use std::ffi::c_void;
use std::time::Instant;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Threading::{
    CreateMutexW, GetCurrentThread, OpenMutexW, ReleaseMutex, SetThreadAffinityMask,
    WaitForSingleObject, MUTEX_ALL_ACCESS,
};

use crate::cpu_vendor::CpuVendor;

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

/// Intel 专用：温度相关的三个 MSR
const MSR_TEMPERATURE_TARGET: u32 = 0x1A2;
const MSR_PACKAGE_THERM_STATUS: u32 = 0x1B1;
const MSR_THERM_STATUS: u32 = 0x19C;

/// AMD 专用：温度寄存器（SMN 空间）与 RAPL 封装功耗的两个 MSR
const SMN_THM_TCON_CUR_TMP: u32 = 0x0005_9800;
const MSR_PWR_UNIT: u32 = 0xC001_0299;
const MSR_PKG_ENERGY_STAT: u32 = 0xC001_029B;

/// 合理温度区间：越界说明解码错了，宁可报错也不显示垃圾值
const TEMP_MIN: f32 = 1.0;
const TEMP_MAX: f32 = 125.0;
/// TjMax 读不到（或读到明显不合理的值）时的兜底，Intel 近代平台基本都按 100 °C
const TJMAX_FALLBACK: f32 = 100.0;
/// 逻辑核心遍历上限，防止亲和性探测死循环
const MAX_LOGICAL_CPU: u32 = 64;

/// AMD `THM_TCON_CUR_TMP` 的 `CUR_TEMP` 字段：bits 31:21，单位 0.125 °C
const AMD_CUR_TEMP_SHIFT: u32 = 21;
const AMD_CUR_TEMP_MASK: u32 = 0x7FF;
/// bit19 = RANGE_SEL；bits 17:16 = TJ_SEL（两者任一命中都表示读数已含 49 °C 偏置）
const AMD_TEMP_RANGE_SEL: u32 = 0x0008_0000;
const AMD_TEMP_TJ_SEL: u32 = 0x0003_0000;
/// 命中上面两个标志时要减回去的固定偏置
const AMD_TEMP_OFFSET_C: f32 = 49.0;

/// 封装功耗的合理性上限（W）：越界说明能量单位或计数器解读错了
const POWER_MAX_W: f32 = 1500.0;

/// `Global\Access_PCI`：串行化 PCI 配置空间（含 AMD 的 SMN 间接访问）的全局互斥体。
///
/// 这是各家监控软件的既有约定（HWiNFO、LibreHardwareMonitor、厂商工具都用它），
/// PawnIO 的 `AMDFamily17` 模块文档也明确要求：`You should acquire the
/// "\BaseNamedObjects\Access_PCI" mutant before calling this`——`Global\` 前缀
/// 指的就是同一个全局对象。名字写错就等于没加锁，所以照抄公开实现的写法。
const PCI_MUTEX_NAME: &str = "Global\\Access_PCI";
/// 拿不到锁时的等待上限：正常无人竞争时零等待，真被别的软件占着也不该卡住采样节拍
const PCI_MUTEX_WAIT_MS: u32 = 50;

/// 已打开的 PawnIO 设备（模块已载入）。Drop 时关闭句柄。
pub struct PawnIo {
    handle: HANDLE,
}

impl PawnIo {
    /// 打开设备并载入模块字节码
    pub fn open(module: &[u8]) -> Result<Self, String> {
        let path: Vec<u16> = DEVICE_PATH.encode_utf16().chain(Some(0)).collect();
        // GENERIC_READ(0x8000_0000) | GENERIC_WRITE(0x4000_0000)
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
        .map_err(|e| format!("载入 PawnIO 模块失败（模块未签名、与驱动版本不匹配，或与 CPU 不符）：{e}"))?;

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

    /// 读一个 SMN 地址（AMD 专用），返回 32 位寄存器值。
    ///
    /// SMN 不是内存也不是 MSR，而是经 PCI 配置空间的一对索引 / 数据寄存器间接访问的
    /// 一片地址空间；那对寄存器是全机共享的，所以调用方**必须**先持有
    /// `Global\Access_PCI` 互斥体（见 [`PciBusGuard`]）。
    pub fn read_smn(&self, offset: u32) -> Result<u32, String> {
        let out = self.execute("ioctl_read_smn", &[offset as i64], 1)?;
        out.first()
            .map(|v| *v as u32)
            .ok_or_else(|| "ioctl_read_smn 未返回数据".to_string())
    }
}

impl Drop for PawnIo {
    fn drop(&mut self) {
        if !self.handle.is_invalid() {
            let _ = unsafe { CloseHandle(self.handle) };
        }
    }
}

/* ------------------------------ Access_PCI 互斥体 ------------------------------ */

/// `Global\Access_PCI` 的持有者，Drop 时释放并关句柄。
///
/// 拿不到锁**不阻止**读取（读中断的成本由调用方的范围校验兜住），只是记一条调试日志——
/// 否则一旦有别的软件崩在临界区里（或把 ACL 设得别人打不开），温度就会永远读不到，
/// 而用户完全看不出为什么。这与「不做静默错误」并不冲突：真正的失败仍会照实上报。
struct PciBusGuard {
    handle: Option<HANDLE>,
    /// 是否真的拿到了锁（决定 Drop 时要不要 `ReleaseMutex`）
    held: bool,
}

impl PciBusGuard {
    fn acquire() -> Self {
        let name: Vec<u16> = PCI_MUTEX_NAME.encode_utf16().chain(Some(0)).collect();
        let name = PCWSTR(name.as_ptr());
        // 先建（已存在则等价于打开）：本进程是提权进程，建得出全局对象。
        // 建的默认安全描述符只给创建者/管理员，所以别人先建好时这里会失败，
        // 再退回 OpenMutexW——公开实现（LHM）创建时就给 Everyone 完全控制，
        // 正是为了让后来者打得开。
        let handle = unsafe { CreateMutexW(None, false, name) }
            .or_else(|_| unsafe { OpenMutexW(MUTEX_ALL_ACCESS, false, name) });
        let Ok(handle) = handle else {
            log_debug("打不开 Access_PCI 互斥体，本次 SMN 读取未加锁");
            return Self {
                handle: None,
                held: false,
            };
        };
        // WAIT_ABANDONED：上一个持有者没释放就退出了，按约定此时锁归我们
        let held = matches!(
            unsafe { WaitForSingleObject(handle, PCI_MUTEX_WAIT_MS) },
            WAIT_OBJECT_0 | WAIT_ABANDONED
        );
        if !held {
            log_debug("等 Access_PCI 互斥体超时，本次 SMN 读取可能与其它监控软件交错");
        }
        Self {
            handle: Some(handle),
            held,
        }
    }
}

impl Drop for PciBusGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.handle {
            if self.held {
                let _ = unsafe { ReleaseMutex(handle) };
            }
            let _ = unsafe { CloseHandle(handle) };
        }
    }
}

/* ------------------------------ Intel：MSR ------------------------------ */

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

/* ------------------------------ AMD：SMN ------------------------------ */

/// AMD CPU 温度 / 封装功耗读取器（SMN 直读 + RAPL 能量计数器）
pub struct AmdTempReader {
    dev: PawnIo,
    /// 能量单位指数 ESU（`MSR_PWR_UNIT` bits 12:8）：每增量 = 1/2^ESU 焦耳
    esu: u32,
    /// 上一次的能量计数与时刻——功耗是差分，单拍读数没有意义
    last_energy: Option<(u32, Instant)>,
}

impl AmdTempReader {
    pub fn open(module: &[u8]) -> Result<Self, String> {
        let dev = PawnIo::open(module)?;
        // ESU 读不到也不致命（温度照样能读），只是没有封装功耗
        let esu = dev
            .read_msr(MSR_PWR_UNIT)
            .map(|v| ((v >> 8) & 0x1F) as u32)
            .unwrap_or(0);
        log_debug(&format!("AMD 能量单位 ESU = {esu}"));
        Ok(Self {
            dev,
            esu,
            last_energy: None,
        })
    }

    /// CPU 温度（Tctl）
    pub fn package_temp(&self) -> Result<f32, String> {
        // 先拿 PCI 互斥体再读：SMN 经 0:0.0 的索引 / 数据寄存器访问，是全机共享资源
        let _pci = PciBusGuard::acquire();
        let raw = self.dev.read_smn(SMN_THM_TCON_CUR_TMP)?;
        decode_amd_temp(raw).ok_or_else(|| {
            format!("AMD 温度读数越界（SMN 0x{SMN_THM_TCON_CUR_TMP:08X} = 0x{raw:08X}）")
        })
    }

    /// CPU 封装功耗（W）：`MSR_PKG_ENERGY_STAT` 是不断累加的能量计数器，
    /// 与上一拍求差后按实测间隔换算。第一次调用没有上一拍，返回 `None`。
    pub fn package_power_w(&mut self) -> Option<f32> {
        let now = Instant::now();
        let energy = self.dev.read_msr(MSR_PKG_ENERGY_STAT).ok()? as u32;
        let (prev_energy, prev_at) = self.last_energy.replace((energy, now))?;

        // 计数器只有 32 位、会回绕；wrapping_sub 在「一拍内绕了不到一圈」时正好还原真值
        // （1/2^21 焦耳的单位下，绕一圈需要 ≈ 2048 W 的功耗，现实中不会发生）
        let delta = energy.wrapping_sub(prev_energy);
        energy_delta_to_watts(delta, self.esu, now.duration_since(prev_at).as_secs_f64())
    }
}

/// 解码 AMD 的 `THM_TCON_CUR_TMP`（SMN `0x00059800`）。
///
/// `CUR_TEMP [31:21]` 是以 0.125 °C 为单位的读数；bit19（RANGE_SEL）或 bits17:16
/// 全 1（TJ_SEL，较新的 Zen 用它表达同一件事）都表示固件**已经**把 49 °C 的偏置算进
/// 去了，要减回来。这段逻辑与 Linux `k10temp`（`drivers/hwmon/k10temp.c`）和
/// LibreHardwareMonitor 的 AMD 实现逐句一致。
///
/// 得到的就是 AMD 暴露的 **Tctl**：Zen2 及以后 Tctl == Tdie（真实结温）；少数
/// Zen / Zen+ 型号（1600X / 1700X / 1800X / 2700X / Threadripper 1x/2x）的 Tctl
/// 会再高出一个固定偏置——那是固件定义的口径，本工具照实显示 Tctl，不替用户
/// 按型号猜测（`k10temp` 会另给一路 Tdie 供对照，本工具只报一路）。
fn decode_amd_temp(raw: u32) -> Option<f32> {
    let mut t = ((raw >> AMD_CUR_TEMP_SHIFT) & AMD_CUR_TEMP_MASK) as f32 * 0.125;
    if raw & AMD_TEMP_RANGE_SEL != 0 || raw & AMD_TEMP_TJ_SEL == AMD_TEMP_TJ_SEL {
        t -= AMD_TEMP_OFFSET_C;
    }
    (TEMP_MIN..=TEMP_MAX).contains(&t).then_some(t)
}

/// 把两拍之间的能量增量换算成瓦数（`delta` 单位 = 1/2^ESU 焦耳），并过滤掉不可能的值。
///
/// 抽取成纯函数是为了能单测：真机上这两拍要隔着 1 秒，测试里没法等。
/// `delta == 0` 一律判无效——计数器没推进说明这个平台根本没给数，此时显示 `--`
/// 比给一个假的 `0.0 W` 诚实（哪怕 CPU 真的近乎空闲，1/2^21 焦耳的单位下
/// 0.5 W 也是每秒约百万次增量，不可能恰好为 0）。
fn energy_delta_to_watts(delta: u32, esu: u32, seconds: f64) -> Option<f32> {
    if !(seconds > 0.0) {
        return None;
    }
    let watts = (f64::from(delta) / f64::from(1u32 << esu) / seconds) as f32;
    (watts.is_finite() && watts > 0.0 && watts <= POWER_MAX_W).then_some(watts)
}

/* ------------------------------ 按厂商分派 ------------------------------ */

/// CPU 温度读取器：按厂商选后端（Intel MSR / AMD SMN）
pub enum CpuTempReader {
    Intel(IntelTempReader),
    Amd(AmdTempReader),
}

impl CpuTempReader {
    /// 载入该厂商对应的 PawnIO 模块。模块字节码由调用方读入（模块随资源目录分发）。
    pub fn open(vendor: CpuVendor, module: &[u8]) -> Result<Self, String> {
        match vendor {
            CpuVendor::Intel => IntelTempReader::open(module).map(Self::Intel),
            CpuVendor::Amd => AmdTempReader::open(module).map(Self::Amd),
            CpuVendor::Other => Err(format!(
                "{} CPU 没有可用的 MSR / SMN 温度直读通道",
                vendor.label()
            )),
        }
    }

    /// CPU 温度
    pub fn package_temp(&self) -> Result<f32, String> {
        match self {
            Self::Intel(r) => r.package_temp(),
            Self::Amd(r) => r.package_temp(),
        }
    }

    /// CPU 封装功耗。Intel 侧返回 `None`——Intel 的功耗有更好的来源（PDH RAPL 在
    /// 主进程里就能读，不必绕到提权子进程），AMD 侧 Windows 没有 PDH 计数器，
    /// 只能走这里的 MSR。
    pub fn package_power_w(&mut self) -> Option<f32> {
        match self {
            Self::Intel(_) => None,
            Self::Amd(r) => r.package_power_w(),
        }
    }

    /// 数据来源描述，直接写进状态文件、在界面上展示
    pub fn source(&self) -> &'static str {
        match self {
            Self::Intel(_) => source_label(CpuVendor::Intel),
            Self::Amd(_) => source_label(CpuVendor::Amd),
        }
    }

    /// Intel 特有的 TjMax；AMD 的 SMN 温度寄存器不提供该上限
    pub fn tjmax(&self) -> Option<f32> {
        match self {
            Self::Intel(r) => Some(r.tjmax()),
            Self::Amd(_) => None,
        }
    }
}

/// 该厂商应当载入的 PawnIO 模块文件名（位于随包的 `resources/pawnio/`）。
/// 没有直读通道的厂商返回 `None`——此时桥接进程不载模块，只做联想风扇。
pub fn module_file(vendor: CpuVendor) -> Option<&'static str> {
    match vendor {
        CpuVendor::Intel => Some("IntelMSR.bin"),
        CpuVendor::Amd => Some("AMDFamily17.bin"),
        CpuVendor::Other => None,
    }
}

/// 该厂商的数据来源描述（界面上展示；读不到时也用同一组文案，便于对照）
pub fn source_label(vendor: CpuVendor) -> &'static str {
    match vendor {
        CpuVendor::Intel => "Intel MSR (PawnIO)",
        CpuVendor::Amd => "AMD SMN (PawnIO)",
        CpuVendor::Other => "不可用",
    }
}

/// 排错用流水账：仅在 `WB_PAWNIO_DIAG=1` 时输出
fn log_debug(msg: &str) {
    if std::env::var("WB_PAWNIO_DIAG").as_deref() == Ok("1") {
        eprintln!("[pawnio] {msg}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 厂家给的原始值 -> 摄氏度。构造方式：读数 = CUR_TEMP 字段值 << 21。
    fn smn_raw(cur_temp: u32, flags: u32) -> u32 {
        (cur_temp << AMD_CUR_TEMP_SHIFT) | flags
    }

    #[test]
    fn amd_decode_plain_value() {
        // 45.0 °C（无任何标志位）——即 Zen2 及以后的 Tctl == Tdie
        assert_eq!(decode_amd_temp(smn_raw(360, 0)), Some(45.0));
        // 0.125 °C 的步进要保留
        assert_eq!(decode_amd_temp(smn_raw(361, 0)), Some(45.125));
    }

    #[test]
    fn amd_decode_subtracts_49_when_flagged() {
        // RANGE_SEL（bit19）命中：字段里已含 49 °C 偏置
        assert_eq!(decode_amd_temp(smn_raw(360 + 392, AMD_TEMP_RANGE_SEL)), Some(45.0));
        // TJ_SEL 全 1（bits17:16）表达同一件事
        assert_eq!(decode_amd_temp(smn_raw(360 + 392, AMD_TEMP_TJ_SEL)), Some(45.0));
        // 只置了 TJ_SEL 的一位时不算命中（须两位全 1）
        let one_bit = AMD_TEMP_TJ_SEL & !(1 << 16);
        assert_eq!(decode_amd_temp(smn_raw(360, one_bit)), Some(45.0));
    }

    #[test]
    fn amd_decode_rejects_impossible_values() {
        // 全 1（读失败时的常见残值）
        assert_eq!(decode_amd_temp(0xFFFF_FFFF), None);
        // 0 °C：传感器没实现时固件常给的值，按无效处理
        assert_eq!(decode_amd_temp(0), None);
        // 超过上限（125 °C）
        assert_eq!(decode_amd_temp(smn_raw(2047, AMD_TEMP_RANGE_SEL)), None);
    }

    #[test]
    fn module_file_matches_vendor() {
        assert_eq!(module_file(CpuVendor::Intel), Some("IntelMSR.bin"));
        assert_eq!(module_file(CpuVendor::Amd), Some("AMDFamily17.bin"));
        assert_eq!(module_file(CpuVendor::Other), None);
        // 两个模块都必须随包分发（NOTICE.md 里逐个列了来源与校验值）
        for v in [CpuVendor::Intel, CpuVendor::Amd] {
            let file = module_file(v).expect("Intel / AMD 都应有模块");
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("resources")
                .join("pawnio")
                .join(file);
            assert!(path.is_file(), "缺少随包模块：{}", path.display());
        }
    }

    #[test]
    fn amd_power_conversion_and_sanity_filter() {
        // ESU=21（增量 = 1/2^21 焦耳）时，1 秒内走 2^21 个增量 = 1 W
        assert_eq!(energy_delta_to_watts(1 << 21, 21, 1.0), Some(1.0));
        // 45 W：1 秒内 45 * 2^21 个增量
        assert_eq!(energy_delta_to_watts(45 * (1 << 21), 21, 1.0), Some(45.0));
        // 同一段能量用 2 秒走完 → 减半
        assert_eq!(energy_delta_to_watts(45 * (1 << 21), 21, 2.0), Some(22.5));
        // 计数器没推进：不给假的 0.0 W，直接判无效（界面显示 `--`）
        assert_eq!(energy_delta_to_watts(0, 21, 1.0), None);
        // 离谱增量（64 W 级机器读到 >1500 W）也判无效
        assert_eq!(energy_delta_to_watts(u32::MAX, 21, 1.0), None);
        // 时间间隔为 0（同一时刻读两次）不做除法
        assert_eq!(energy_delta_to_watts(1234, 21, 0.0), None);
    }

    /// Intel 的解码回归（与既有行为一致）
    #[test]
    fn intel_dts_decode_unchanged() {
        // bit31 置位 + DTS=20，TjMax=100 → 80 °C
        let v = (1u64 << 31) | (20 << 16);
        assert_eq!(decode_dts(v, 100.0), Some(80.0));
        // 读数有效位为 0
        assert_eq!(decode_dts(20 << 16, 100.0), None);
    }
}
