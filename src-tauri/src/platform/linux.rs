//! Linux 平台的采集源实现：sysfs / procfs 的真实读取
//!
//! 本文件只做「IO + trait 实现」；一切解析 / 换算 / 选择逻辑在
//! [`super::linux_parse`]（纯函数，所有平台共编译，fixture 单测在那）。
//! GPU 的 NVML 走与 Windows 共用的 [`super::gpu_nvml`]。
//!
//! | trait | 通道 |
//! | --- | --- |
//! | [`GpuSource`] | NVML（NVIDIA）→ amdgpu sysfs（busy% / VRAM / 温度 / 功耗 / 风扇）|
//! | [`ThermalSource`] | hwmon（coretemp / k10temp / zenpower / 显卡 hwmon）+ RAPL 功耗 |
//! | [`FanSource`] | hwmon `fanN_input`（RPM）/ `pwmN`（占空比）|
//! | [`BatterySource`] | `/sys/class/power_supply/`（charge / energy 两种量纲）|
//! | [`MemoryExtras`] | `/proc/meminfo`（MemAvailable + Committed_AS / CommitLimit）|

use std::path::{Path, PathBuf};
use std::time::Instant;

use super::gpu_nvml::NvmlSource;
use super::linux_parse::{
    amd_device, battery_convert, chip_reading, energy_delta_w, gpu_fan_readings, parse_meminfo,
    pick_cpu_fan_chip, pick_cpu_temp, pick_gpu_temp, plausible_temp, temp_relevant_chip, AmdCardRaw,
    BatteryRaw, FanPick, HwmonChip,
};
use super::{
    BatterySource, FanReading, FanReport, FanSource, GpuSource, MemoryExtras, MemoryStatus,
    PlatformSources, RawBattery, ThermalSource,
};
use crate::battery;
use crate::fan_pref::FanPrefStore;
use crate::model::{BatteryInfo, GpuDevice, ThermalInfo};

/* ------------------------------ 通用小工具 ------------------------------ */

/// 读一个 sysfs 文件并去掉首尾空白；不可读时 None
fn read_str(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 读一个数值文件
fn read_num<T: std::str::FromStr>(path: &Path) -> Option<T> {
    read_str(path)?.parse().ok()
}

/* ------------------------------ hwmon 扫描 ------------------------------ */

/// 按 hwmon 编号顺序扫描 `/sys/class/hwmon/`
fn scan_hwmon() -> Vec<HwmonChip> {
    let mut dirs: Vec<(u32, PathBuf)> = Vec::new();
    let Ok(rd) = std::fs::read_dir("/sys/class/hwmon") else {
        return Vec::new();
    };
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some(idx) = name
            .strip_prefix("hwmon")
            .and_then(|n| n.parse::<u32>().ok())
        {
            dirs.push((idx, entry.path()));
        }
    }
    dirs.sort_by_key(|(idx, _)| *idx);
    dirs.into_iter()
        .map(|(_, path)| scan_chip(&path))
        .filter(|c| !c.name.is_empty())
        .collect()
}

fn scan_chip(dir: &Path) -> HwmonChip {
    let mut chip = HwmonChip {
        name: read_str(&dir.join("name")).unwrap_or_default(),
        ..HwmonChip::default()
    };

    // 传感器按 tempN / fanN / pwmN 编号，读目录项再排序（不依赖 read_dir 顺序）
    let mut temp_idx: Vec<(u32, PathBuf)> = Vec::new();
    let mut fan_idx: Vec<(u32, PathBuf)> = Vec::new();
    let mut pwm_idx: Vec<(u32, PathBuf)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for entry in rd.flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();
            if let Some(n) = fname
                .strip_prefix("temp")
                .and_then(|r| r.strip_suffix("_input"))
                .and_then(|r| r.parse::<u32>().ok())
            {
                temp_idx.push((n, entry.path()));
            } else if let Some(n) = fname
                .strip_prefix("fan")
                .and_then(|r| r.strip_suffix("_input"))
                .and_then(|r| r.parse::<u32>().ok())
            {
                fan_idx.push((n, entry.path()));
            } else if let Some(n) = fname.strip_prefix("pwm").and_then(|r| r.parse::<u32>().ok()) {
                pwm_idx.push((n, entry.path()));
            }
        }
    }
    temp_idx.sort_by_key(|(n, _)| *n);
    fan_idx.sort_by_key(|(n, _)| *n);
    pwm_idx.sort_by_key(|(n, _)| *n);

    // 温度只读「可能被选中」的芯片（CPU / 显卡，见 temp_relevant_chip 的成本说明）——
    // 其余芯片（nvme、wifi、电池……）的温度读了也不会显示，白等 IO
    if temp_relevant_chip(&chip.name) {
        for (n, path) in temp_idx {
            let Some(milli) = read_num::<i64>(&path) else {
                continue;
            };
            let Some(c) = plausible_temp(milli as f32 / 1000.0) else {
                continue;
            };
            let label = read_str(&dir.join(format!("temp{n}_label"))).unwrap_or_default();
            chip.temps.push((label, c));
        }
    }
    for (_, path) in fan_idx {
        chip.fans.push(read_num::<f32>(&path).unwrap_or(0.0));
    }
    for (_, path) in pwm_idx {
        chip.pwms.push(read_num::<f32>(&path).unwrap_or(0.0));
    }
    chip
}

/* ------------------------------ RAPL 功耗 ------------------------------ */

/// `intel-rapl` 能量计数器的差分状态（µJ → W）
struct RaplState {
    path: PathBuf,
    last_uj: u64,
    max_range: Option<u64>,
    last_at: Instant,
}

/// 找 RAPL 的 package 级能量文件（`/sys/class/powercap/intel-rapl:N/energy_uJ`）。
///
/// 只取顶层 zone（`intel-rapl:0` 这种；带子域后缀的 `intel-rapl:0:1` 是子域，跳过），
/// 优先 `name` 以 `package` 开头的那个。
fn find_rapl_energy() -> Option<PathBuf> {
    let rd = std::fs::read_dir("/sys/class/powercap").ok()?;
    let mut top: Vec<(String, PathBuf)> = Vec::new();
    for entry in rd.flatten() {
        let dirname = entry.file_name().to_string_lossy().to_string();
        let parts: Vec<&str> = dirname.split(':').collect();
        if parts.len() == 2 && parts[0].starts_with("intel-rapl") {
            let energy = entry.path().join("energy_uJ");
            if energy.is_file() {
                let name = read_str(&entry.path().join("name")).unwrap_or_default();
                top.push((name, entry.path()));
            }
        }
    }
    let pick = top
        .iter()
        .find(|(name, _)| name.starts_with("package"))
        .or_else(|| top.first())?;
    Some(pick.1.join("energy_uJ"))
}

/* ------------------------------ GPU ------------------------------ */

/// 扫 amdgpu 的 cardN（只认 vendor 0x1002 的），返回按卡号排序的原始读数
fn scan_amd_cards() -> Vec<AmdCardRaw> {
    let mut cards: Vec<(u32, AmdCardRaw)> = Vec::new();
    let Ok(rd) = std::fs::read_dir("/sys/class/drm") else {
        return Vec::new();
    };
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        // card0-DP-1 / renderD128 之类不是整卡目录（parse 失败自然跳过）
        let Some(idx) = name.strip_prefix("card").and_then(|n| n.parse::<u32>().ok()) else {
            continue;
        };
        let dev = entry.path().join("device");
        // 只处理 AMD（0x1002）；Intel 核显的 hwmon 只有温度、没有 busy% / VRAM sysfs，诚实跳过
        if read_str(&dev.join("vendor")).as_deref() != Some("0x1002") {
            continue;
        }
        let label =
            read_str(&dev.join("product_name")).unwrap_or_else(|| format!("AMD GPU card{idx}"));
        let (temp_c, power_uw, fan_rpm, fan_pwm) = amd_hwmon(&entry.path());
        cards.push((
            idx,
            AmdCardRaw {
                name: label,
                busy_pct: read_num::<f32>(&dev.join("gpu_busy_percent")),
                vram_used: read_num::<u64>(&dev.join("mem_info_vram_used")),
                vram_total: read_num::<u64>(&dev.join("mem_info_vram_total")),
                temp_c: temp_c.and_then(plausible_temp),
                power_uw,
                fan_rpm,
                fan_pwm,
            },
        ));
    }
    cards.sort_by_key(|(idx, _)| *idx);
    cards.into_iter().map(|(_, c)| c).collect()
}

/// 一张显卡自己的 hwmon（`/sys/class/drm/cardN/device/hwmon/hwmonM`）里的
/// 温度 / 功耗 / 风扇。显卡总有且只有一个 hwmon，取第一个。
fn amd_hwmon(card_dir: &Path) -> (Option<f32>, Option<f64>, Option<f32>, Option<f32>) {
    let Ok(rd) = std::fs::read_dir(card_dir.join("device/hwmon")) else {
        return (None, None, None, None);
    };
    let Some(hwmon) = rd.flatten().map(|e| e.path()).find(|p| p.is_dir()) else {
        return (None, None, None, None);
    };
    (
        read_num::<i64>(&hwmon.join("temp1_input")).map(|m| m as f32 / 1000.0),
        read_num::<f64>(&hwmon.join("power1_average")),
        read_num::<f32>(&hwmon.join("fan1_input")),
        read_num::<f32>(&hwmon.join("pwm1")),
    )
}

/// GPU：NVML 优先（N 卡），其次 amdgpu sysfs（A 卡）
struct LinuxGpu {
    nvml: NvmlSource,
}

impl GpuSource for LinuxGpu {
    fn sample(&mut self) -> (Vec<GpuDevice>, String) {
        if let Some(devices) = self.nvml.sample() {
            if !devices.is_empty() {
                return (devices, "nvml".to_string());
            }
        }
        let devices: Vec<GpuDevice> = scan_amd_cards()
            .iter()
            .enumerate()
            .filter_map(|(i, raw)| amd_device(i as u32, raw))
            .collect();
        if devices.is_empty() {
            (devices, "unavailable".to_string())
        } else {
            (devices, "amdgpu".to_string())
        }
    }
}

/* ------------------------------ 温度 ------------------------------ */

struct LinuxThermal {
    rapl: Option<RaplState>,
}

impl LinuxThermal {
    fn new() -> Self {
        let mut rapl = None;
        if let Some(path) = find_rapl_energy() {
            let max_range = path
                .parent()
                .and_then(|p| read_num::<u64>(&p.join("max_energy_range_uJ")));
            if let Some(uj) = read_num::<u64>(&path) {
                rapl = Some(RaplState {
                    path,
                    last_uj: uj,
                    max_range,
                    last_at: Instant::now(),
                });
            }
        }
        Self { rapl }
    }

    /// RAPL 差分求 CPU 封装功耗；首拍没有基线，返回 None
    fn cpu_power_w(&mut self) -> Option<f32> {
        let st = self.rapl.as_mut()?;
        let cur = read_num::<u64>(&st.path)?;
        let elapsed = st.last_at.elapsed().as_secs_f64();
        let prev = st.last_uj;
        st.last_uj = cur;
        st.last_at = Instant::now();
        energy_delta_w(prev, cur, st.max_range, elapsed).map(|w| w as f32)
    }
}

impl ThermalSource for LinuxThermal {
    fn sample(&mut self) -> ThermalInfo {
        let chips = scan_hwmon();
        let cpu = pick_cpu_temp(&chips);
        let gpu = pick_gpu_temp(&chips);
        let power = self.cpu_power_w();

        let (cpu_temp_c, cpu_source) = match cpu {
            Some((t, s)) => (Some(t), s),
            None => (None, "不支持".to_string()),
        };
        let (gpu_temp_c, gpu_source) = match gpu {
            Some((t, s)) => (Some(t), s),
            None => (None, "不支持".to_string()),
        };
        let available = cpu_temp_c.is_some() || gpu_temp_c.is_some();
        let hint = if available {
            String::new()
        } else {
            "内核 hwmon 未暴露 CPU / GPU 温度传感器：Intel 需 coretemp、AMD 需 k10temp（或 zenpower），\
             虚拟机中通常没有。可用 `sensors` 命令确认。"
                .to_string()
        };
        ThermalInfo {
            cpu_temp_c,
            cpu_source,
            gpu_temp_c,
            gpu_source,
            available,
            cpu_power_w: power,
            hint,
        }
    }
}

/* ------------------------------ 风扇 ------------------------------ */

fn to_fan_reading(pick: FanPick) -> FanReading {
    FanReading {
        rpm: pick.rpm,
        pct: pick.pct,
        source: pick.source,
    }
}

struct LinuxFan {
    /// 保留设置页注入的偏好句柄。FanID 是联想 ACPI-WMI 的概念，hwmon 通道暂不使用——
    /// 这里只持有它，将来做「hwmon 风扇序号手动指定」时直接用。
    #[allow(dead_code)]
    pref: Option<FanPrefStore>,
}

impl FanSource for LinuxFan {
    fn set_pref(&mut self, pref: FanPrefStore) {
        self.pref = Some(pref);
    }

    fn sample(&mut self, gpu_count: usize) -> FanReport {
        let chips = scan_hwmon();
        let cpu = pick_cpu_fan_chip(&chips).map(chip_reading).unwrap_or_default();
        let gpus = gpu_fan_readings(&chips, gpu_count);
        let hint = if cpu.any() {
            String::new()
        } else {
            "内核 hwmon 未暴露 CPU 风扇转速：笔记本风扇多由 EC 管理，需 thinkpad / nct 等专用\
             内核模块暴露；可用 `sensors` 命令确认。"
                .to_string()
        };
        FanReport {
            cpu: to_fan_reading(cpu),
            gpus: gpus.into_iter().map(to_fan_reading).collect(),
            hint,
            vendor: None,
        }
    }
}

/* ------------------------------ 电池 ------------------------------ */

/// 读一块电池目录的全部字段
fn read_battery(dir: &Path, ac_online: bool) -> BatteryRaw {
    BatteryRaw {
        status: read_str(&dir.join("status")).unwrap_or_default(),
        ac_online,
        charge_now_uah: read_num(&dir.join("charge_now")),
        charge_full_uah: read_num(&dir.join("charge_full")),
        energy_now_uwh: read_num(&dir.join("energy_now")),
        energy_full_uwh: read_num(&dir.join("energy_full")),
        current_now_ua: read_num(&dir.join("current_now")),
        voltage_now_uv: read_num(&dir.join("voltage_now")),
        power_now_uw: read_num(&dir.join("power_now")),
        capacity_pct: read_num::<f32>(&dir.join("capacity")),
    }
}

/// 扫 `/sys/class/power_supply/`：返回（第一块电池的读数, 适配器是否在线）。
///
/// 无电池时返回 `(None, ac_online)` —— 台式机仍要如实报告适配器状态。
fn scan_power_supply() -> (Option<BatteryRaw>, bool) {
    let mut ac_online = false;
    let mut entries: Vec<PathBuf> = Vec::new();
    if let Ok(rd) = std::fs::read_dir("/sys/class/power_supply") {
        for entry in rd.flatten() {
            entries.push(entry.path());
        }
    }
    entries.sort();

    // 第一遍：找电池目录 + 汇总适配器在线状态（与目录名的字母序无关）
    let mut battery_dir: Option<PathBuf> = None;
    for path in &entries {
        match read_str(&path.join("type")).as_deref() {
            Some("Mains") | Some("USB") | Some("USB_C") => {
                if read_num::<u8>(&path.join("online")) == Some(1) {
                    ac_online = true;
                }
            }
            Some("Battery") => {
                if battery_dir.is_none() {
                    battery_dir = Some(path.clone());
                }
            }
            _ => {}
        }
    }

    // 第二遍：电池字段读取（此时 ac 已确定）
    (battery_dir.map(|dir| read_battery(&dir, ac_online)), ac_online)
}

struct LinuxBattery;

impl BatterySource for LinuxBattery {
    fn collect(&mut self) -> BatteryInfo {
        match scan_power_supply() {
            (Some(raw_b), _ac) => {
                let c = battery_convert(&raw_b);
                let raw = RawBattery {
                    present: true,
                    ac_online: raw_b.ac_online,
                    charging: c.charging,
                    discharging: c.discharging,
                    max_capacity_mwh: c.max_capacity_mwh.unwrap_or(0).min(u32::MAX as u64) as u32,
                    remaining_capacity_mwh: c
                        .remaining_capacity_mwh
                        .unwrap_or(0)
                        .min(u32::MAX as u64) as u32,
                    rate_mw: c.rate_mw.unwrap_or(0),
                    // sysfs 没有标准的剩余时间接口，如实置 0（from_raw 会跳过）
                    estimated_time_s: 0,
                    source: "sysfs (/sys/class/power_supply)",
                };
                battery::from_raw(raw, raw_b.capacity_pct)
            }
            // 没有电池（台式机 / UPS 供电）：如实报告适配器状态
            (None, ac_online) => battery::from_raw(
                RawBattery {
                    present: false,
                    ac_online,
                    source: "sysfs (/sys/class/power_supply)",
                    ..RawBattery::default()
                },
                None,
            ),
        }
    }
}

/* ------------------------------ 内存附加 ------------------------------ */

struct LinuxMemoryExtras;

impl MemoryExtras for LinuxMemoryExtras {
    fn memory_status(&self) -> Option<MemoryStatus> {
        let text = std::fs::read_to_string("/proc/meminfo").ok()?;
        let p = parse_meminfo(&text)?;
        Some(MemoryStatus {
            load_pct: p.load_pct,
            phys_total_bytes: p.total_kb.saturating_mul(1024),
            phys_avail_bytes: p.avail_kb.saturating_mul(1024),
            commit_limit_bytes: p.commit_limit_kb.unwrap_or(0).saturating_mul(1024),
            commit_used_bytes: p.committed_kb.unwrap_or(0).saturating_mul(1024),
        })
    }
}

/* ------------------------------ 工厂 ------------------------------ */

pub(super) fn sources() -> PlatformSources {
    PlatformSources {
        gpu: Box::new(LinuxGpu {
            nvml: NvmlSource::new(),
        }),
        thermal: Box::new(LinuxThermal::new()),
        fan: Box::new(LinuxFan { pref: None }),
        battery: Box::new(LinuxBattery),
        memory_extras: Box::new(LinuxMemoryExtras),
    }
}
