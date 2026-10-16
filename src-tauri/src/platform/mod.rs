//! 跨平台指标采集抽象层
//!
//! 采样链路分两半：
//!
//! * **天然可移植的一半**（CPU / 内存基础 / 磁盘 / 网络 / 进程榜）直接走 sysinfo，
//!   留在 `sampler.rs`，不进本层——抽象 OS 绑定不了已经可移植的东西。
//! * **OS 绑定的一半**（GPU / 温度 / 风扇 / 电池 / 内存附加 / CPU 附加指标）没有跨平台库
//!   兜底，全部收敛为本模块的 6 个 trait，由各平台的子模块给出实现：
//!
//! | trait | Windows 实现（`windows.rs`） | Linux 实现（`linux.rs`） | 其他平台（`unsupported.rs`） |
//! | --- | --- | --- | --- |
//! | [`GpuSource`] | NVML + PDH 计数器合并（`gpu.rs`，身份取自 DXGI） | NVML + 驱动 sysfs（amdgpu / i915 / xe） | 诚实报「暂不支持」 |
//! | [`ThermalSource`] | MSR 桥接 / LHM / ACPI / PDH 四级降级（`thermal.rs`） | hwmon + RAPL 功耗 | 同上 |
//! | [`FanSource`] | 厂商私有 / LHM / ACPI 三级降级（`fan.rs`） | hwmon fan/pwm | 同上 |
//! | [`BatterySource`] | CallNtPowerInformation / GetSystemPowerStatus（`win32.rs`） | `/sys/class/power_supply` | 同上 |
//! | [`MemoryExtras`] | GlobalMemoryStatusEx 提交量（`win32.rs`） | `/proc/meminfo` 提交量 | 同上 |
//! | [`CpuExtras`] | PDH `Actual Frequency`（实时主频，`cpu_extras.rs`） | 无需补充（sysinfo 已是实时口径） | 同上 |
//!
//! ## 接入新平台的步骤
//!
//! 1. 新建 `platform/<os>.rs`，为 6 个 trait 各写一个实现（能读到就报、读不到
//!    给出该平台的具体原因，遵循「数据源诚实降级」）；
//! 2. 在 [`sources`] 的 cfg 分派里加一个分支；
//! 3. `unsupported.rs` 对新平台不再生效，其余公共层（sampler / battery / model）
//!    无需任何改动。
//!
//! ## 硬性约束
//!
//! * 公共层（`sampler.rs` / `battery.rs` / `model.rs` / 本模块）**不得**直接
//!   `use` Windows 专属模块（gpu / thermal / fan / vendor / win32 / wmi / pdh），
//!   一律通过这里的 trait——这是 cfg 分区能成立的边界。
//! * trait 都要求 `Send`：实现体要进采样线程的 `Mutex<Inner>`，靠 `Box<dyn>` 持有。
//! * 各实现**自带跨拍状态**（PDH 句柄、WMI 节流时刻、缓存），所以是「有状态的
//!   源对象」而非无状态函数；`FanSource::set_pref` 用于注入与设置页共享的风扇偏好句柄。

use crate::fan_pref::FanPrefStore;
use crate::model::{BatteryInfo, GpuDevice, ThermalInfo};

/* ---------------- 跨边界数据类型 ---------------- */
/* 以下类型同时被 trait 签名与 sampler 公共层使用，必须留在 OS 中立的位置。 */

/// 一次采样拿到的风扇读数（自 `fan.rs` 迁入）
#[derive(Debug, Clone, Default)]
pub struct FanReading {
    /// 转速（RPM）。仅当数据源可信为 RPM 时有值。
    pub rpm: Option<f32>,
    /// 风扇占空比（%）。ACPI 的「相对速度」和 NVML 的百分比都归到这里。
    pub pct: Option<f32>,
    /// 数据来源描述，直接展示在界面上
    pub source: String,
}

impl FanReading {
    /// 取到任何可用读数
    pub fn any(&self) -> bool {
        self.rpm.is_some() || self.pct.is_some()
    }
}

/// 整个应用一份的整体风扇报告（自 `fan.rs` 迁入）
#[derive(Debug, Clone, Default)]
pub struct FanReport {
    /// CPU 风扇（优先厂商通道，其次 LHM/OHM 的 cpu 归属传感器或 ACPI）
    pub cpu: FanReading,
    /// 各 GPU 风扇，按 GPU 名称关键字匹配
    pub gpus: Vec<FanReading>,
    /// 拿不到任何风扇时展示给用户的说明
    pub hint: String,
    /// 识别到的整机厂商（供界面标注数据来源）
    pub vendor: Option<Vendor>,
}

/// 已识别的整机厂商（枚举自 `vendor.rs` 迁入；`detect()` 依赖 WMI，留在原处）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vendor {
    Lenovo,
    /// 非联想，或未能识别
    Other,
}

impl Vendor {
    /// 界面上展示的厂商名。
    ///
    /// 当前界面用风扇的 `source` 文案（如「联想 EC（上限 4500 RPM）」）表达来源，
    /// 所以这个短名暂时没有调用点；保留它是为了让界面文案集中在一处，
    /// 将来加「设备信息」区块时直接用。
    #[allow(dead_code)]
    pub fn label(self) -> &'static str {
        match self {
            Vendor::Lenovo => "联想",
            Vendor::Other => "通用",
        }
    }
}

/// Windows GlobalMemoryStatusEx 的读数（自 `win32.rs` 迁入）。
/// 仅含 sysinfo 覆盖不到的「已提交内存」与系统内存负载。
#[derive(Clone, Copy, Debug, Default)]
pub struct MemoryStatus {
    /// dwMemoryLoad
    pub load_pct: u32,
    pub phys_total_bytes: u64,
    pub phys_avail_bytes: u64,
    /// 提交上限 = 物理内存 + 页面文件大小
    pub commit_limit_bytes: u64,
    /// 当前已提交量
    pub commit_used_bytes: u64,
}

/// CPU 的附加读数：sysinfo 覆盖不到的一项（**实时**主频）。
///
/// 为什么单独一路：Windows 上 sysinfo 的 `Cpu::frequency()` 取自
/// `CallNtPowerInformation` 的 `PROCESSOR_POWER_INFORMATION.CurrentMhz`，实测**恒为
/// 标称频率**（本机 2300 MHz，打满也不动），实时值只能从 PDH `Actual Frequency` 拿。
/// 占用不在这里补充——实测任务管理器显示的与 sysinfo 的忙时占比**同口径**
/// （`% Processor Utility` 恒为「忙时 × 频率系数」，与任务管理器对不上，
/// 见 `cpu_extras.rs` 模块文档与 AGENTS.md），所以占用没有第二条通道。
#[derive(Clone, Copy, Debug, Default)]
pub struct CpuExtrasReading {
    /// 平台自己给的**实时**主频（MHz）。给出时优先于 sysinfo 的 `frequency()`
    pub live_freq_mhz: Option<u32>,
    /// sysinfo 的 `frequency()` 在本平台**是否已经是实时值**：
    /// Windows 只给标称频率（false），Linux 读的是 cpufreq `scaling_cur_freq`（true）。
    /// 界面据此决定写「当前主频」还是「基准主频」——不能拿标称值冒充实时值。
    pub sysinfo_freq_live: bool,
}

/// 电池原始读数（自 `win32.rs` 迁入）。
/// 各平台的 [`BatterySource`] 把原生接口的读数整理成这个统一形状，
/// 再交给公共层 `battery::from_raw` 做加工（文案、单位换算全部复用）。
#[derive(Clone, Copy, Debug, Default)]
pub struct RawBattery {
    pub present: bool,
    pub ac_online: bool,
    pub charging: bool,
    pub discharging: bool,
    /// 满充容量，单位 mWh
    pub max_capacity_mwh: u32,
    /// 剩余容量，单位 mWh
    pub remaining_capacity_mwh: u32,
    /// 充/放电功率，单位 mW（恒为正值，方向由 charging/discharging 决定）
    pub rate_mw: u32,
    /// 剩余估算时间，单位秒
    pub estimated_time_s: u32,
    pub source: &'static str,
}

/* ---------------- 采集源 trait ---------------- */

/// GPU 采样源：多卡分别读，返回 (设备列表, 来源标识)。
/// 来源标识沿用 model 的约定：nvml / pdh / pdh-warming / unavailable。
pub trait GpuSource: Send {
    fn sample(&mut self) -> (Vec<GpuDevice>, String);
}

/// 温度 / CPU 封装功耗采样源
pub trait ThermalSource: Send {
    fn sample(&mut self) -> ThermalInfo;
}

/// 风扇采样源。`gpu_count` 用于把读数按序分配给各 GPU。
pub trait FanSource: Send {
    /// 注入与设置页共享的风扇偏好句柄（「CPU 风扇号」手动指定，两边必须同一份）
    fn set_pref(&mut self, pref: FanPrefStore);
    fn sample(&mut self, gpu_count: usize) -> FanReport;
}

/// 电池采样源。实现应把原生读数交给 `battery::from_raw` 加工后返回。
pub trait BatterySource: Send {
    fn collect(&mut self) -> BatteryInfo;
}

/// sysinfo 覆盖不到的内存附加指标（Windows 的「已提交内存」/ 内存负载）。
/// 平台没有对应概念时返回 `None`，前端显示 `--`。
pub trait MemoryExtras: Send {
    fn memory_status(&self) -> Option<MemoryStatus>;
}

/// sysinfo 覆盖不到的 CPU 附加指标（**实时**主频）。
/// 平台没有对应通道时各项给 `None` + `sysinfo_freq_live` 如实回答，前端相应降级。
pub trait CpuExtras: Send {
    fn sample(&mut self) -> CpuExtrasReading;
}

/* ---------------- 平台工厂 ---------------- */

/// 一套平台采集源。由 [`sources`] 按编译目标组装，sampler 整体持有。
pub struct PlatformSources {
    pub gpu: Box<dyn GpuSource>,
    pub thermal: Box<dyn ThermalSource>,
    pub fan: Box<dyn FanSource>,
    pub battery: Box<dyn BatterySource>,
    pub memory_extras: Box<dyn MemoryExtras>,
    pub cpu_extras: Box<dyn CpuExtras>,
}

/// 组装当前操作系统的采集源。
/// 新增平台时在这里加 cfg 分支（见模块文档「接入新平台的步骤」）。
pub fn sources() -> PlatformSources {
    #[cfg(windows)]
    {
        windows::sources()
    }
    #[cfg(target_os = "linux")]
    {
        linux::sources()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        unsupported::sources()
    }
}

/// 尚未适配的平台上，各指标统一的「读不到」原因（占位实现使用）
pub const UNSUPPORTED: &str = "当前操作系统暂不支持该指标的采集";

#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
mod linux;

/// Linux 采集的解析与选择纯函数：**所有平台都参与编译**（理由同 unsupported.rs）——
/// 本仓库在 Windows 上开发，这样 `cargo test` 就能直接跑 Linux 的解析单测。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod linux_parse;

/// 尚未适配平台（macOS 等）的占位实现**在所有平台都参与编译**：它只依赖公共类型，
/// 让 Windows / Linux 上的 `cargo test` 就能发现 trait 签名与占位实现的漂移，
/// 而不是等到某个新平台编译时才炸。
#[allow(dead_code)]
mod unsupported;

/// NVML 读数探针：Windows（gpu.rs）与 Linux（linux.rs）共用。
/// 其余平台（macOS 等）暂无使用者，先允许闲置。
#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
pub(crate) mod gpu_nvml;

#[cfg(test)]
mod tests {
    use super::*;

    /// 占位实现必须「诚实降级」：所有通道给出空读数 + 明确原因，
    /// 绝不编造数值。在 Windows 上跑，守护 trait 签名与占位实现不漂移。
    #[test]
    fn unsupported_sources_report_unavailable() {
        let mut s = unsupported::sources();

        let (devices, _) = s.gpu.sample();
        assert!(devices.is_empty(), "占位 GPU 不应报出任何设备");

        let thermal = s.thermal.sample();
        assert!(!thermal.available);
        assert!(thermal.cpu_temp_c.is_none());
        assert!(thermal.hint.contains("不支持"), "hint 应说明平台不支持");

        let fans = s.fan.sample(2);
        assert!(!fans.cpu.any());
        assert_eq!(fans.gpus.len(), 2, "GPU 风扇槽位应按 gpu_count 预留");
        assert!(!fans.gpus.iter().any(|f| f.any()));
        assert!(fans.hint.contains("不支持"));

        let battery = s.battery.collect();
        assert!(!battery.has_battery);
        assert!(!battery.status_text.is_empty(), "电池应给出说明而非空文案");

        assert!(s.memory_extras.memory_status().is_none());

        let cpu_extras = s.cpu_extras.sample();
        assert!(
            cpu_extras.live_freq_mhz.is_none(),
            "占位实现不应编造实时主频"
        );
        assert!(
            !cpu_extras.sysinfo_freq_live,
            "占位平台不得把 sysinfo 的主频当实时值宣称"
        );
    }
}
