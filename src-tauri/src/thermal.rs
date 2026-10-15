//! CPU 温度采集 —— 多源探测 + 有效性过滤
//!
//! Windows 不像 Linux 有统一的 hwmon 接口，CPU 温度只能这样拿，按优先级：
//! 1. **CPU 片内温度直读（经 PawnIO 驱动）**——Intel 读 MSR 的 DTS 寄存器、AMD 读
//!    SMN 的 `THM_TCON_CUR_TMP`，都是片内传感器给的原始读数，最准；
//!    两者都需要管理员权限，故实际读取在提权桥接子进程里做，这里只读它写出的状态文件
//!    （见 `msr_bridge.rs` / `pawnio.rs`）；
//! 2. LibreHardwareMonitor / OpenHardwareMonitor 的 WMI 传感器
//!    —— **注意**：LHM 自 0.9.5 起已删除 WMI 提供程序，这条实际只对 OHM（或 LHM ≤ 0.9.4）有效；
//! 3. ACPI 热区（WMI `root\wmi:MSAcpi_ThermalZoneTemperature`），厂商工具（联想 LegionZone 等）
//!    也拿它当通用兜底——联想机型上往往能直接读到 CPU 热区，且无需安装任何软件；
//! 4. ACPI 温度区（PDH `Thermal Zone Information`），**很多机器上报的是无意义的恒定值**，
//!    例如本机实测恒为 273（对应 0°C），必须靠范围校验剔除。

use std::time::{Duration, Instant};

use windows::Win32::System::Performance::PDH_HCOUNTER;

use crate::model::ThermalInfo;
use crate::pdh::Query;
use crate::wmi::HwTemp;

const THERMAL_PATH: &str = r"\Thermal Zone Information(*)\Temperature";
/// Intel RAPL 的包功耗（PDH Energy Meter，单位为 mW）。
///
/// 这个 PDH 计数器只在 Intel 平台存在；**AMD 没有对应的用户态计数器**，其封装功耗
/// 改由提权桥接子进程读 MSR 能量计数器（`MSR_PKG_ENERGY_STAT`）差分得出，见
/// `pawnio.rs` 的 `AmdTempReader::package_power_w`。
/// 老平台或虚拟机连 Intel 计数器也没有，取不到时只是少一个读数，不影响温度。
const POWER_PATH: &str = r"\Energy Meter(*)\Power";

/// WMI 查询较慢，失败后至少隔这么久再试
const RETRY_AFTER: Duration = Duration::from_secs(60);

struct PdhThermal {
    query: Query,
    temp_counter: PDH_HCOUNTER,
    /// 可选：部分机器没有 Energy Meter 计数器
    power_counter: Option<PDH_HCOUNTER>,
}

/// 一次 PDH 采样结果：温度 + CPU 封装功耗
#[derive(Default)]
struct PdhSample {
    temp_c: Option<f32>,
    cpu_power_w: Option<f32>,
}

/// 提权桥接进程本拍给出的读数（状态文件里有什么就是什么）
#[derive(Default)]
struct BridgeSample {
    /// (温度, 来源描述)——来源由桥接按真实后端写明，界面直接展示
    temp: Option<(f32, String)>,
    /// CPU 封装功耗（W）。目前只有 AMD 会填（Intel 侧由 PDH RAPL 提供）
    power_w: Option<f32>,
}

// PDH 句柄不被跨线程共享，仅随 probe 整体移交，标记为 Send 即可
unsafe impl Send for PdhThermal {}

pub struct ThermalProbe {
    pdh: Option<PdhThermal>,
    pdh_tried: bool,
    /// 上一次成功读到的硬件监控传感器
    hw: Option<(String, Vec<HwTemp>)>,
    next_wmi_probe: Instant,
    /// ACPI 热区（WMI `MSAcpi_ThermalZoneTemperature`）的有效读数，缓存避免每次慢查询
    acpi_wmi: Option<Vec<f32>>,
    next_acpi_wmi_probe: Instant,
    /// 上一次的失败原因，用于给前端展示
    last_hint: String,
    /// 温度直读通道（提权桥接进程）的失败原因，优先级高于 last_hint
    msr_hint: String,
}

impl Default for ThermalProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl ThermalProbe {
    pub fn new() -> Self {
        Self {
            pdh: None,
            pdh_tried: false,
            hw: None,
            next_wmi_probe: Instant::now(),
            acpi_wmi: None,
            next_acpi_wmi_probe: Instant::now(),
            last_hint: String::new(),
            msr_hint: String::new(),
        }
    }

    pub fn sample(&mut self) -> ThermalInfo {
        self.refresh_hw_if_due();
        self.refresh_acpi_if_due();

        // CPU 封装功耗（RAPL）走同一条 PDH 查询，温度无论用哪个源都要先取一次
        let pdh = self.sample_pdh();
        // 提权桥接的状态文件本拍只读一次（温度 + AMD 封装功耗都在里面）
        let bridge = self.sample_bridge();
        // 功耗来源：PDH（Intel RAPL）优先；AMD 没有 PDH 计数器，用桥接差分出的那个
        let cpu_power_w = pdh.cpu_power_w.or(bridge.power_w);

        // 优先级 1：片内温度直读（PawnIO 提权桥接进程供数），Intel 走 MSR、AMD 走 SMN
        if let Some((temp, source)) = bridge.temp {
            return ThermalInfo {
                cpu_temp_c: Some(temp),
                cpu_source: source,
                gpu_temp_c: self.gpu_temp_from_hw(),
                gpu_source: "hwmonitor".to_string(),
                available: true,
                cpu_power_w,
                hint: String::new(),
            };
        }

        // 优先级 2：硬件监控软件（OHM / 旧版 LHM）的 WMI 传感器
        if let Some((source, temps)) = &self.hw {
            if let Some(t) = pick_cpu_temp(temps) {
                return ThermalInfo {
                    cpu_temp_c: Some(t),
                    cpu_source: source.clone(),
                    gpu_temp_c: pick_gpu_temp(temps),
                    gpu_source: source.clone(),
                    available: true,
                    cpu_power_w,
                    hint: String::new(),
                };
            }
        }

        // 优先级 3：ACPI 热区（WMI `MSAcpi_ThermalZoneTemperature`），联想等厂商工具的通用兜底路径，
        // 在其机型上往往能直接读到 CPU 热区，且无需安装任何软件
        if let Some(temps) = &self.acpi_wmi {
            if let Some(t) = temps.iter().copied().reduce(f32::max) {
                return ThermalInfo {
                    cpu_temp_c: Some(t),
                    cpu_source: "ACPI 温度区 (WMI)".to_string(),
                    gpu_temp_c: self.gpu_temp_from_hw(),
                    gpu_source: "hwmonitor".to_string(),
                    available: true,
                    cpu_power_w,
                    hint: String::new(),
                };
            }
        }

        // 优先级 4：ACPI 温度区（PDH）
        if let Some(t) = pdh.temp_c {
            return ThermalInfo {
                cpu_temp_c: Some(t),
                cpu_source: "ACPI 温度区".to_string(),
                gpu_temp_c: self.gpu_temp_from_hw(),
                gpu_source: "hwmonitor".to_string(),
                available: true,
                cpu_power_w,
                hint: String::new(),
            };
        }

        // GPU 温度可能单独从硬件监控软件拿到，即便 CPU 没有
        let gpu = self.gpu_temp_from_hw();
        ThermalInfo {
            cpu_temp_c: None,
            cpu_source: "不可用".to_string(),
            gpu_temp_c: gpu,
            gpu_source: if gpu.is_some() {
                "hwmonitor".to_string()
            } else {
                "不可用".to_string()
            },
            available: false,
            cpu_power_w,
            // 直读通道的原因更具体（未提权 / 未装 PawnIO / CPU 不在支持范围），优先展示
            hint: if self.msr_hint.is_empty() {
                self.last_hint.clone()
            } else {
                self.msr_hint.clone()
            },
        }
    }

    /// 读提权桥接进程写出的状态。文件读得很轻（几百字节），每次采样都读即可。
    ///
    /// 来源描述也取自状态文件：桥接按**真实后端**写明是 `Intel MSR (PawnIO)`
    /// 还是 `AMD SMN (PawnIO)`，这里不硬编码，免得界面上把 AMD 的读数标成 Intel。
    fn sample_bridge(&mut self) -> BridgeSample {
        let Some(state) = crate::msr_bridge::read_fresh_state() else {
            return BridgeSample::default();
        };
        match state.cpu_temp_c {
            Some(t) => {
                self.msr_hint.clear();
                BridgeSample {
                    temp: Some((t, state.source)),
                    power_w: state.cpu_power_w,
                }
            }
            None => {
                if let Some(err) = state.error {
                    self.msr_hint = format!("CPU 温度未取到：{err}");
                }
                BridgeSample {
                    power_w: state.cpu_power_w,
                    ..Default::default()
                }
            }
        }
    }

    /// 只有 GPU 温度需要用到硬件监控软件数据时的取值入口
    fn gpu_temp_from_hw(&self) -> Option<f32> {
        self.hw.as_ref().and_then(|(_, t)| pick_gpu_temp(t))
    }

    fn refresh_hw_if_due(&mut self) {
        if Instant::now() < self.next_wmi_probe {
            return;
        }
        // 无论成败都排下一次探测：成功则 30s 后刷新，失败则等更久
        match crate::wmi::hardware_temperatures() {
            Some((source, temps)) => {
                self.hw = Some((source, temps));
                self.last_hint.clear();
                self.next_wmi_probe = Instant::now() + Duration::from_secs(30);
            }
            None => {
                self.hw = None;
                self.last_hint = HINT_NO_SENSOR.to_string();
                self.next_wmi_probe = Instant::now() + RETRY_AFTER;
            }
        }
    }

    /// 按节流间隔刷新 ACPI 热区（WMI）读数。WMI 跨进程查询较慢，故缓存并复用。
    fn refresh_acpi_if_due(&mut self) {
        if Instant::now() < self.next_acpi_wmi_probe {
            return;
        }
        // 无论成败都排下一次探测：成功则 30s 后刷新，失败则等更久
        match crate::wmi::acpi_thermal_zone_raw() {
            Some(raw) => {
                let temps: Vec<f32> = raw.iter().filter_map(|v| to_celsius(*v)).collect();
                self.acpi_wmi = if temps.is_empty() { None } else { Some(temps) };
                self.next_acpi_wmi_probe = Instant::now() + Duration::from_secs(30);
            }
            None => {
                self.acpi_wmi = None;
                self.next_acpi_wmi_probe = Instant::now() + RETRY_AFTER;
            }
        }
    }

    fn sample_pdh(&mut self) -> PdhSample {
        if self.pdh.is_none() {
            if self.pdh_tried {
                return PdhSample::default();
            }
            self.pdh_tried = true;
            let Some(query) = Query::new() else {
                return PdhSample::default();
            };
            let Some(temp_counter) = query.add(THERMAL_PATH) else {
                return PdhSample::default();
            };
            // 功耗是可选增强：加不上只是少一个读数，不影响温度
            let power_counter = query.add(POWER_PATH);
            query.collect();
            self.pdh = Some(PdhThermal {
                query,
                temp_counter,
                power_counter,
            });
        }
        let Some(p) = self.pdh.as_ref() else {
            return PdhSample::default();
        };
        p.query.collect();

        let mut best: Option<f32> = None;
        for (_, raw) in p.query.double_array(p.temp_counter) {
            if let Some(c) = to_celsius(raw) {
                best = Some(match best {
                    Some(v) => v.max(c),
                    None => c,
                });
            }
        }
        let cpu_power_w = p
            .power_counter
            .and_then(|c| pick_cpu_power(&p.query.double_array(c)));

        PdhSample {
            temp_c: best,
            cpu_power_w,
        }
    }
}

/// 从 Energy Meter 的实例列表里挑出 CPU 封装功耗（mW → W）
///
/// 这是 **Intel** 侧的功耗来源（AMD 的实例名不在这里，其功耗走桥接读 MSR）。
/// 实测 Intel 的实例形如 `rapl_package0_pkg`（整包）、`rapl_package0_pp0`（核心）、
/// `rapl_package0_pp1`、`rapl_package0_dram`；`_total` 恒为 0，不能直接取。
/// 优先 `*_pkg`，没有时退回所有非 `_total` 实例里的最大值。
fn pick_cpu_power(items: &[(String, f64)]) -> Option<f32> {
    let pick = |only_pkg: bool| -> Option<f64> {
        items
            .iter()
            .filter(|(name, v)| {
                let n = name.to_ascii_lowercase();
                v.is_finite() && *v > 0.0 && !n.contains("_total") && (!only_pkg || n.contains("pkg"))
            })
            .map(|(_, v)| *v)
            .fold(None, |acc: Option<f64>, v| Some(acc.map_or(v, |a| a.max(v))))
    };
    let mw = pick(true).or_else(|| pick(false))?;
    // 单位是毫瓦，超过 1 kW 一律视为异常读数
    if mw > 1_000_000.0 {
        return None;
    }
    Some((mw / 1000.0) as f32)
}

/// 把 ACPI 原始值换算成摄氏度，顺便剔掉无效读数
///
/// 单位可能是开尔文或十分之一开尔文（取决于固件实现），
/// 换算后再用合理温度区间过滤掉「传感器没实现时返回的 0°C」这类假值。
fn to_celsius(raw: f64) -> Option<f32> {
    let c = if (200.0..=500.0).contains(&raw) {
        raw - 273.15
    } else if (2000.0..=5000.0).contains(&raw) {
        raw / 10.0 - 273.15
    } else {
        return None;
    };
    if (1.0..=120.0).contains(&c) {
        Some(c as f32)
    } else {
        None
    }
}

/// 多个 CPU 读数里挑最能代表整体的那个：优先封装 Tctl/Tdie 或 Package/Core 合计
fn pick_cpu_temp(temps: &[HwTemp]) -> Option<f32> {
    let cpus: Vec<&HwTemp> = temps.iter().filter(|t| t.kind == "cpu").collect();
    if cpus.is_empty() {
        return None;
    }
    let preferred = ["package", "total", "tctl", "tdie", "cpu (total)"];
    let lower_has = |t: &HwTemp, keys: &[&str]| {
        let n = t.name.to_ascii_lowercase();
        keys.iter().any(|k| n.contains(k))
    };
    if let Some(t) = cpus.iter().find(|t| lower_has(t, &preferred)) {
        return Some(t.value_c);
    }
    cpus.first().map(|t| t.value_c)
}

fn pick_gpu_temp(temps: &[HwTemp]) -> Option<f32> {
    temps
        .iter()
        .filter(|t| t.kind == "gpu")
        .map(|t| t.value_c)
        .reduce(f32::max)
}

/// 兜底提示：连片内直读通道都没拿到时的通用说明（Intel / AMD 两条路都适用）
const HINT_NO_SENSOR: &str = "未检测到可用的温度传感器。可在设置页「高级采集」里启用 CPU 温度直读（Intel 读 MSR、AMD 读 SMN，需要一次管理员授权），或安装 OpenHardwareMonitor 等硬件监控软件补齐。";
