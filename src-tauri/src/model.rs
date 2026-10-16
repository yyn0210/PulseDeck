//! 与前端交换的数据模型（JSON key 统一 camelCase）

use serde::Serialize;

fn round1(v: f32) -> f32 {
    (v * 10.0).round() / 10.0
}

/// 单次采样得到的完整快照
#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// 采样时刻（Unix 毫秒）
    pub ts_ms: u64,
    /// 两组采样之间的实测间隔（毫秒），前端据此把「本次增量」换算成速率
    pub sample_interval_ms: u64,
    pub system: SystemInfo,
    pub cpu: CpuInfo,
    pub memory: MemoryInfo,
    pub disks: Vec<DiskInfo>,
    pub battery: BatteryInfo,
    pub gpu: GpuInfo,
    pub net: NetInfo,
    pub thermal: ThermalInfo,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub host_name: Option<String>,
    pub os_name: Option<String>,
    pub os_version: Option<String>,
    pub long_os_version: Option<String>,
    pub kernel_version: Option<String>,
    pub cpu_arch: String,
    pub uptime_secs: u64,
    pub boot_time_secs: u64,
    pub process_count: usize,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct CpuInfo {
    pub brand: String,
    pub usage_total: f32,
    pub logical_cores: usize,
    pub physical_cores: usize,
    /// 当前各核心占用（%），顺序与 sysinfo 的 cpus() 一致
    pub per_core: Vec<f32>,
    /// 当前主频（MHz）：平台能读到实时值就给实时值（Windows 走 PDH 的
    /// `Actual Frequency`），否则退回 sysinfo 的标称频率（此时 `freq_live = false`）
    pub freq_mhz: u64,
    /// `freq_mhz` 是否为**实时**读数。false = 平台只能给标称频率，界面写「基准主频」
    pub freq_live: bool,
    /// CPU 风扇转速（RPM）。Windows 无公开通道，仅装 OHM/旧版 LHM 时才有值。
    pub fan_rpm: Option<f32>,
    /// CPU 风扇占空比（%）。ACPI 的「相对速度」归到这里（单位不可信为 RPM）。
    pub fan_pct: Option<f32>,
    /// CPU 风扇数据来源；读不到时为空字符串
    pub fan_source: String,
    pub top_cpu: Vec<ProcInfo>,
    pub top_mem: Vec<ProcInfo>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProcInfo {
    pub pid: usize,
    pub name: String,
    pub cpu_usage: f32,
    pub memory_bytes: u64,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct MemoryInfo {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub usage_pct: f32,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
    pub swap_usage_pct: f32,
    /// Windows 已提交内存：当前已提交字节数与提交上限（物理内存 + 所有页面文件）
    pub commit_used_bytes: u64,
    pub commit_limit_bytes: u64,
    pub commit_pct: f32,
    /// Kernel32 GlobalMemoryStatusEx 给出的内存负载百分比
    pub system_load_pct: f32,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct DiskInfo {
    pub name: String,
    pub mount_point: String,
    pub file_system: String,
    pub disk_kind: String,
    pub removable: bool,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
    pub usage_pct: f32,
    /// 速率按两次采样的实测间隔换算，单位 byte/s
    pub read_bytes_per_sec: u64,
    pub write_bytes_per_sec: u64,
    /// 开机以来的累计读写量（byte），通过重启不清零，可用于总量统计
    pub total_read_bytes: u64,
    pub total_written_bytes: u64,
}

/* ------------------------------ GPU ------------------------------ */

/// 单块 GPU 的读数
#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct GpuDevice {
    pub index: u32,
    pub name: String,
    /// 占用百分比；拿不到时为 None
    pub usage_pct: Option<f32>,
    pub mem_used_bytes: u64,
    pub mem_total_bytes: u64,
    pub mem_pct: Option<f32>,
    pub temp_c: Option<f32>,
    /// 整卡功耗（W）
    pub power_w: Option<f32>,
    /// 风扇占空比（%）。NVML 的 `fan_speed` 返回的就是百分比。
    pub fan_pct: Option<f32>,
    /// 风扇真实转速（RPM）。仅当驱动/传感器给出可信 RPM 时才有值，否则为 None。
    pub fan_rpm: Option<f32>,
    /// 数据来源：nvml = NVIDIA 驱动；pdh = Windows 性能计数器；
    /// amdgpu / i915 / xe = Linux 的显卡驱动 sysfs
    pub source: String,
}

/// GPU 汇总：概览大数字用主卡（见 [`GpuInfo::aggregate`] 的挑卡规则），明细走 devices
#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct GpuInfo {
    pub available: bool,
    pub devices: Vec<GpuDevice>,
    /// 被选作主卡的那块设备在 `devices` 里的 `index`（界面据此显示型号，
    /// 保证「显示的型号」与「显示的数字」是同一块卡）
    pub main_index: u32,
    pub usage_pct: Option<f32>,
    pub mem_used_bytes: u64,
    pub mem_total_bytes: u64,
    pub mem_pct: Option<f32>,
    pub temp_c: Option<f32>,
    pub power_w: Option<f32>,
    /// 主卡风扇占空比（%），来源同 devices
    pub fan_pct: Option<f32>,
    /// 主卡风扇转速（RPM）
    pub fan_rpm: Option<f32>,
    /// GPU 风扇数据来源；读不到时为空字符串
    pub fan_source: String,
    /// 列表级来源标识，由 [`GpuInfo::aggregate`] 从各设备的 `source` 汇总而来：
    /// 去重后按出现顺序用 `+` 连接（如 `nvml+pdh`）；没有任何设备时是 `unavailable`
    pub source: String,
}

impl GpuInfo {
    /// 按设备列表重新计算汇总值与主卡
    ///
    /// 挑卡分两步：先按**数据源分档**（驱动直读优先于通用计数器，见 [`source_rank`]），
    /// 再在最高档里取占用最高的那块。只按占用挑是不行的 —— 笔记本上核显常年被
    /// 桌面合成占着几个百分点，插着独显的机器主卡会一直在核显与独显之间跳。
    pub fn aggregate(&mut self) {
        self.available = !self.devices.is_empty();
        self.main_index = 0;
        if !self.available {
            self.source = "unavailable".to_string();
            return;
        }
        self.source = device_sources(&self.devices);
        let rank = self
            .devices
            .iter()
            .map(|d| source_rank(&d.source))
            .min()
            .unwrap_or(0);
        let best = || self.devices.iter().filter(move |d| source_rank(&d.source) == rank);
        let main = best()
            .filter(|d| d.usage_pct.is_some())
            .max_by(|a, b| {
                a.usage_pct
                    .unwrap_or(0.0)
                    .partial_cmp(&b.usage_pct.unwrap_or(0.0))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .or_else(|| best().max_by_key(|d| d.mem_used_bytes));

        if let Some(m) = main {
            self.main_index = m.index;
            self.usage_pct = m.usage_pct;
            self.mem_used_bytes = m.mem_used_bytes;
            self.mem_total_bytes = m.mem_total_bytes;
            self.mem_pct = m.mem_pct;
            self.temp_c = m.temp_c;
            self.power_w = m.power_w;
            self.fan_pct = m.fan_pct;
            self.fan_rpm = m.fan_rpm;
        } else {
            self.usage_pct = None;
        }
    }
}

/// 列表级来源标识：各设备的来源去重后按出现顺序用 `+` 连接（如 `nvml+pdh`）
pub fn device_sources(devices: &[GpuDevice]) -> String {
    let mut out: Vec<&str> = Vec::new();
    for d in devices {
        if !out.contains(&d.source.as_str()) {
            out.push(&d.source);
        }
    }
    out.join("+")
}

/// 数据源分档（数字越小越优先）：**厂商驱动直读**优于通用计数器兜底。
/// 只影响「多显卡时谁当主卡」，不影响任何一块卡上报什么值。
fn source_rank(source: &str) -> u8 {
    match source {
        // 厂商驱动直读：NVML（NVIDIA）、amdgpu / i915 / xe（Linux 显卡驱动 sysfs，
        // 读不到驱动名时退化成 amd / intel 这两个厂商名）
        "nvml" | "amdgpu" | "amd" | "i915" | "xe" | "intel" => 0,
        // Windows 的通用性能计数器（所有适配器都能看，但没有温度 / 功耗）
        "pdh" => 1,
        _ => 2,
    }
}

/* ------------------------------ 网络 ------------------------------ */

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct NetIface {
    pub name: String,
    pub mac: String,
    /// 虚拟网卡（VMware / Hyper-V / WSL 等），前端可折叠
    pub is_virtual: bool,
    /// 下行速率（byte/s）—— 自上次采样增量换算
    pub rx_bytes_per_sec: u64,
    /// 上行速率（byte/s）
    pub tx_bytes_per_sec: u64,
    pub total_received: u64,
    pub total_transmitted: u64,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct NetInfo {
    pub interfaces: Vec<NetIface>,
    /// 全部网卡合计的下行速率（byte/s）
    pub rx_bytes_per_sec: u64,
    pub tx_bytes_per_sec: u64,
    pub total_received: u64,
    pub total_transmitted: u64,
}

/* ------------------------------ 温度 ------------------------------ */

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ThermalInfo {
    pub cpu_temp_c: Option<f32>,
    pub cpu_source: String,
    pub gpu_temp_c: Option<f32>,
    pub gpu_source: String,
    pub available: bool,
    /// CPU 封装功耗（W），来自 PDH `\Energy Meter(*)\Power`（Intel/AMD RAPL）。
    /// 拿不到时为 None —— 老平台没有这个计数器。
    pub cpu_power_w: Option<f32>,
    /// 读不到时的排错/处置提示，前端在不显示数值时展示
    pub hint: String,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct BatteryInfo {
    /// 是否存在电池（台式机/无电池设备为 false）
    pub has_battery: bool,
    pub ac_online: bool,
    pub charging: bool,
    pub discharging: bool,
    /// 剩余电量百分比；未知时为 None
    pub percent: Option<f32>,
    /// 满充容量（Wh）
    pub full_charge_wh: Option<f32>,
    /// 当前剩余电量（Wh）
    pub remaining_wh: Option<f32>,
    /// 充电功率 = 适配器当前充入电池的功率（W）
    pub charge_rate_w: Option<f32>,
    /// 放电功率 = 整机当前功耗（W）
    pub discharge_rate_w: Option<f32>,
    /// 充满所需时间（秒）
    pub seconds_to_full: Option<u64>,
    /// 剩余续航时间（秒）
    pub seconds_to_empty: Option<u64>,
    /// 数据来源，便于排错
    pub source: String,
    pub status_text: String,
}

impl MemoryInfo {
    pub fn recompute(&mut self) {
        self.usage_pct = percent_of(self.used_bytes, self.total_bytes);
        self.swap_usage_pct = percent_of(self.swap_used_bytes, self.swap_total_bytes);
        self.commit_pct = percent_of(self.commit_used_bytes, self.commit_limit_bytes);
    }
}

impl CpuInfo {
    pub fn normalize(&mut self) {
        self.usage_total = round1(self.usage_total);
        for v in &mut self.per_core {
            *v = round1(*v);
        }
        for v in &mut self.top_cpu {
            v.cpu_usage = round1(v.cpu_usage).clamp(0.0, 100.0);
        }
        for v in &mut self.top_mem {
            v.cpu_usage = round1(v.cpu_usage).clamp(0.0, 100.0);
        }
    }
}

pub fn percent_of(used: u64, total: u64) -> f32 {
    if total == 0 {
        return 0.0;
    }
    round1((used as f64 / total as f64 * 100.0) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_guard() {
        assert_eq!(percent_of(0, 0), 0.0);
        assert_eq!(percent_of(512, 1024), 50.0);
    }

    fn dev(index: u32, name: &str, source: &str, usage: Option<f32>, mem: u64) -> GpuDevice {
        GpuDevice {
            index,
            name: name.to_string(),
            usage_pct: usage,
            mem_used_bytes: mem,
            source: source.to_string(),
            ..GpuDevice::default()
        }
    }

    #[test]
    fn main_card_prefers_driver_source_over_higher_igpu_usage() {
        // 插着独显的笔记本：核显被桌面合成占着 20%，独显在 3%，主卡应仍是独显
        let mut info = GpuInfo {
            devices: vec![
                dev(0, "Intel(R) UHD Graphics", "pdh", Some(20.0), 1 << 30),
                dev(1, "NVIDIA GeForce RTX 3060 Laptop GPU", "nvml", Some(3.0), 2 << 30),
            ],
            ..GpuInfo::default()
        };
        info.aggregate();
        assert!(info.available);
        assert_eq!(info.main_index, 1, "主卡是驱动直读的那块");
        assert_eq!(info.usage_pct, Some(3.0));
        assert_eq!(info.mem_used_bytes, 2 << 30);
    }

    #[test]
    fn main_card_takes_highest_usage_within_one_source() {
        let mut info = GpuInfo {
            devices: vec![
                dev(0, "Radeon 780M", "amdgpu", Some(8.0), 1 << 30),
                dev(1, "Radeon RX 7900 XTX", "amdgpu", Some(61.0), 6 << 30),
            ],
            ..GpuInfo::default()
        };
        info.aggregate();
        assert_eq!(info.main_index, 1);
        assert_eq!(info.usage_pct, Some(61.0));
    }

    #[test]
    fn main_card_falls_back_to_memory_when_no_usage_anywhere() {
        let mut info = GpuInfo {
            devices: vec![
                dev(0, "Intel(R) UHD Graphics", "pdh", None, 1 << 30),
                dev(1, "AMD Radeon RX 6800", "pdh", None, 3 << 30),
            ],
            ..GpuInfo::default()
        };
        info.aggregate();
        assert_eq!(info.main_index, 1, "都没占用数据时看显存占用");
        assert_eq!(info.usage_pct, None);
    }

    #[test]
    fn aggregate_without_devices_reports_unavailable() {
        let mut info = GpuInfo::default();
        info.aggregate();
        assert!(!info.available);
        assert_eq!(info.source, "unavailable");
        assert_eq!(info.main_index, 0);
    }
}
