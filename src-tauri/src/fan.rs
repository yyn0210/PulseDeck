//! 风扇转速采集 —— 尽力而为 + 诚实降级
//!
//! Windows 上「风扇转速」是个比温度更封闭的指标，没有任何**通用**的用户态接口：
//!
//! | 通道 | 覆盖情况 | 单位 |
//! | --- | --- | --- |
//! | **厂商私有（联想 ACPI-WMI）** | **联想机型有效；⚠️ 需管理员权限**（走提权桥接，非提权调用返回访问被拒） | **RPM（可信）** |
//! | NVML（NVIDIA） | 桌面卡常有，**笔记本独显普遍为 N/A**（风扇由 EC 控制） | 百分比 / RPM |
//! | LHM / OHM 的 WMI `Sensor(SensorType='Fan')` | 需用户装 OpenHardwareMonitor 或旧版 LHM ≤ 0.9.4 | **RPM（可信）** |
//! | ACPI `Win32_Fan.DesiredSpeed` | 类存在但**绝大多数机器字段为空**；有值也不保证是 RPM | 相对速度 |
//! | PDH | **没有**任何 Fan 计数器（实测只有 `Thermal Zone Information`） | — |
//!
//! 所以本模块的定位是「**能读到就报、读不到就明确说读不到**」，绝不编造数值。
//! 前端在拿不到时显示 `--`，并通过 [`FanReport::hint`] 说明原因。
//!
//! ## 优先级
//!
//! 1. **厂商私有通道**（[`crate::vendor`]）——机型相关，联想机型上是唯一能拿到
//!    CPU 风扇 RPM 的途径。数据经**提权桥接进程**（[`crate::msr_bridge`]）中转，
//!    因为直接调 `LENOVO_FAN_METHOD` 需要管理员权限。
//! 2. LHM / OpenHardwareMonitor 的 WMI 风扇传感器（单位可信为 RPM）
//! 3. ACPI `Win32_Fan`（单位不可信，只当占空比）
//!
//! CPU 侧的历史现状：CPU 风扇挂在主板的 Super I/O / EC 上，而**笔记本根本不用
//! Super I/O**（风扇由 EC 直接管理，Linux 7.1 的 `yogafan` 文档有明确说明）。
//! 本工具已有的 PawnIO + `IntelMSR.bin` 只能读 MSR，**不含** EC 读取函数，
//! 因此无法复用。联想机型改走其 ACPI-WMI 私有通道（见上表第一行）。

use std::time::{Duration, Instant};

use crate::platform::{FanReading, FanReport, Vendor};
use crate::vendor::VendorProbe;
use crate::wmi::{self, HwFan};

/// WMI 查询较慢，失败后至少隔这么久再试（与 thermal.rs 的节流策略一致）
const RETRY_AFTER: Duration = Duration::from_secs(60);
/// 成功后的刷新间隔：风扇转速变化比温度慢，30s 足够
const REFRESH_AFTER: Duration = Duration::from_secs(30);

pub struct FanProbe {
    /// 厂商私有通道（联想 ACPI-WMI）。机型相关，是联想机型上 CPU 风扇的唯一来源。
    vendor: VendorProbe,
    /// 上一次成功的硬件监控软件风扇读数
    hw: Option<(String, Vec<HwFan>)>,
    next_hw_probe: Instant,
    /// 上一次成功的 ACPI 风扇读数（通常为空）
    acpi: Vec<HwFan>,
    next_acpi_probe: Instant,
}

impl Default for FanProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl FanProbe {
    pub fn new() -> Self {
        Self {
            vendor: VendorProbe::new(),
            hw: None,
            next_hw_probe: Instant::now(),
            acpi: Vec::new(),
            next_acpi_probe: Instant::now(),
        }
    }

    /// 注入风扇偏好句柄（与设置页共用同一份，避免各读各的 `fan.json`）
    pub fn set_pref(&mut self, pref: crate::fan_pref::FanPrefStore) {
        self.vendor.set_pref(pref);
    }

    /// 采样一次。`gpu_count` 用于按顺序分配 GPU 风扇读数。
    pub fn sample(&mut self, gpu_count: usize) -> FanReport {
        self.refresh_hw_if_due();
        self.refresh_acpi_if_due();

        let mut report = FanReport {
            vendor: Some(self.vendor.vendor()),
            ..FanReport::default()
        };

        // 优先级 1：厂商私有通道（联想 ACPI-WMI）。RPM 可信，且是联想机型上
        // 唯一能拿到 CPU 风扇 RPM 的途径，所以放在最前。
        let (vendor_cpu, vendor_gpu) = self.vendor.sample();
        if let Some(f) = vendor_cpu {
            report.cpu = FanReading {
                rpm: Some(f.rpm),
                pct: None,
                source: f.source,
            };
        }

        // 优先级 2：硬件监控软件（单位是可信的 RPM）
        if let Some((source, fans)) = &self.hw {
            if !report.cpu.any() {
                report.cpu = pick_cpu_fan(fans, source);
            }
            for i in 0..gpu_count {
                report.gpus.push(pick_gpu_fan(fans, source, i));
            }
        } else {
            report.gpus = vec![FanReading::default(); gpu_count];
        }

        // GPU 侧厂商通道兜底（联想的 FanID=1）；只补通用通道没给到的位置
        if let Some(f) = vendor_gpu {
            if let Some(first) = report.gpus.first_mut() {
                if !first.any() {
                    *first = FanReading {
                        rpm: Some(f.rpm),
                        pct: None,
                        source: f.source,
                    };
                }
            }
        }

        // 优先级 3：ACPI（单位不可信，只当占空比，且通常为空）
        if !report.cpu.any() {
            if let Some(f) = self.acpi.iter().find(|f| f.kind == "cpu").or_else(|| self.acpi.first())
            {
                report.cpu = FanReading {
                    rpm: None,
                    pct: Some(f.rpm),
                    source: format!("ACPI ({})", f.name),
                };
            }
        }

        if !report.cpu.any() {
            report.hint = cpu_hint(report.vendor);
        }
        report
    }

    fn refresh_hw_if_due(&mut self) {
        if Instant::now() < self.next_hw_probe {
            return;
        }
        match wmi::hardware_fans() {
            Some((source, fans)) => {
                self.hw = Some((source, fans));
                self.next_hw_probe = Instant::now() + REFRESH_AFTER;
            }
            None => {
                self.hw = None;
                self.next_hw_probe = Instant::now() + RETRY_AFTER;
            }
        }
    }

    fn refresh_acpi_if_due(&mut self) {
        if Instant::now() < self.next_acpi_probe {
            return;
        }
        self.acpi = wmi::acpi_fan_speeds();
        self.next_acpi_probe = Instant::now()
            + if self.acpi.is_empty() {
                RETRY_AFTER
            } else {
                REFRESH_AFTER
            };
    }
}

/// 液冷/水泵传感器的名字关键字。
///
/// 一体式水冷（AIO）的水泵在 OHM/LHM 里**也是 `Fan` 传感器**，名字通常带
/// Pump / Water / AIO / Liquid / Coolant。水泵转速往往比 CPU 风扇还高，
/// 若不做排除，「按转速取最大」就会把泵速显示成 CPU 风扇转速。
const COOLANT_WORDS: [&str; 5] = ["pump", "water", "aio", "liquid", "coolant"];

/// 传感器是不是水泵/液冷（而不是风扇）
fn is_coolant(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    COOLANT_WORDS.iter().any(|w| lower.contains(w))
}

/// 从传感器列表里挑 CPU 风扇：优先封装/CPU 标识，其次按名字含 cpu 的
fn pick_cpu_fan(fans: &[HwFan], source: &str) -> FanReading {
    let pick = fans
        .iter()
        .filter(|f| !is_coolant(&f.name))
        .filter(|f| f.kind == "cpu")
        .max_by(|a, b| a.rpm.partial_cmp(&b.rpm).unwrap_or(std::cmp::Ordering::Equal))
        .or_else(|| {
            fans.iter()
                .filter(|f| !is_coolant(&f.name))
                .find(|f| f.name.to_ascii_lowercase().contains("cpu"))
        });
    match pick {
        Some(f) => FanReading {
            rpm: if f.is_rpm { Some(f.rpm) } else { None },
            pct: if f.is_rpm { None } else { Some(f.rpm) },
            source: source.to_string(),
        },
        None => FanReading::default(),
    }
}

/// 从传感器列表里挑第 `index` 块 GPU 的风扇
fn pick_gpu_fan(fans: &[HwFan], source: &str, index: usize) -> FanReading {
    // 同 CPU 侧：显卡水冷的泵速也不算显卡风扇
    let gpu_fans: Vec<&HwFan> = fans
        .iter()
        .filter(|f| f.kind == "gpu" && !is_coolant(&f.name))
        .collect();
    let Some(f) = gpu_fans
        .iter()
        .filter(|f| f.name.to_ascii_lowercase().contains("#0") || index == 0)
        .max_by(|a, b| a.rpm.partial_cmp(&b.rpm).unwrap_or(std::cmp::Ordering::Equal))
    else {
        return FanReading::default();
    };
    FanReading {
        rpm: if f.is_rpm { Some(f.rpm) } else { None },
        pct: if f.is_rpm { None } else { Some(f.rpm) },
        source: source.to_string(),
    }
}

/// 拿不到 CPU 风扇时的说明。写得具体一点，用户才知道这不是 bug。
pub const CPU_HINT: &str = "CPU 风扇转速无法从 Windows 公开接口读取：转速由主板 EC / Super I/O 控制，\
系统未暴露给用户态程序。安装 OpenHardwareMonitor 或旧版 LibreHardwareMonitor（≤ 0.9.4）后本工具可自动读到。";

/// 联想机型上拿不到风扇的说明——原因通常是提权桥接没在跑，或 BIOS 太老
pub const CPU_HINT_LENOVO: &str = "本机是联想设备，风扇转速需要管理员权限读取\
（联想 ACPI-WMI 私有通道）。请在界面上点击「启用」授权一次；\
若已授权仍无数据，可能是 BIOS 版本较老未暴露该接口。";

/// 按厂商挑选提示文案
fn cpu_hint(vendor: Option<Vendor>) -> String {
    match vendor {
        Some(Vendor::Lenovo) => CPU_HINT_LENOVO.to_string(),
        _ => CPU_HINT.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fan(kind: &str, name: &str, rpm: f32, is_rpm: bool) -> HwFan {
        HwFan {
            kind: kind.to_string(),
            name: name.to_string(),
            rpm,
            is_rpm,
        }
    }

    #[test]
    fn picks_cpu_fan_as_rpm() {
        let fans = vec![
            fan("cpu", "CPU Fan", 2100.0, true),
            fan("gpu", "GPU Fan", 1800.0, true),
        ];
        let r = pick_cpu_fan(&fans, "root\\OpenHardwareMonitor");
        assert_eq!(r.rpm, Some(2100.0));
        assert_eq!(r.pct, None);
    }

    /// 单位不可信为 RPM 的读数（ACPI 的「相对速度」）只能落到 pct，不许冒充转速
    #[test]
    fn non_rpm_becomes_percent() {
        let fans = vec![fan("cpu", "冷却设备", 40.0, false)];
        let r = pick_cpu_fan(&fans, "ACPI");
        assert_eq!(r.rpm, None);
        assert_eq!(r.pct, Some(40.0));
        assert_eq!(r.source, "ACPI");
    }

    /// ACPI 走的是 sample() 里的兜底分支：hardware_fans() 为空时才会用到
    #[test]
    fn acpi_is_fallback_only() {
        let mut probe = FanProbe {
            vendor: VendorProbe::silent_for_test(),
            hw: None,
            next_hw_probe: Instant::now() + Duration::from_secs(3600),
            acpi: vec![fan("other", "冷却设备", 55.0, false)],
            next_acpi_probe: Instant::now() + Duration::from_secs(3600),
        };
        let report = probe.sample(1);
        assert_eq!(report.cpu.rpm, None);
        assert_eq!(report.cpu.pct, Some(55.0));
        assert!(report.cpu.source.starts_with("ACPI"));
        // ACPI 不提供 GPU 风扇
        assert!(!report.gpus[0].any());
    }

    /// 厂商通道给出读数时，应当**优先于** LHM/OHM 的通用通道
    #[test]
    fn vendor_takes_precedence_over_hw() {
        let mut probe = FanProbe {
            vendor: VendorProbe::with_fans_for_test(Some(2600.0), None),
            hw: Some((
                "root\\OpenHardwareMonitor".to_string(),
                vec![fan("cpu", "CPU Fan", 1800.0, true)],
            )),
            next_hw_probe: Instant::now() + Duration::from_secs(3600),
            acpi: Vec::new(),
            next_acpi_probe: Instant::now() + Duration::from_secs(3600),
        };
        let report = probe.sample(0);
        // 厂商的 2600 应该赢过 OHM 的 1800
        assert_eq!(report.cpu.rpm, Some(2600.0));
        assert!(report.cpu.source.contains("联想"));
    }

    /// 厂商通道沉默时应回落到 LHM/OHM
    #[test]
    fn falls_back_to_hw_when_vendor_silent() {
        let mut probe = FanProbe {
            vendor: VendorProbe::silent_for_test(),
            hw: Some((
                "root\\OpenHardwareMonitor".to_string(),
                vec![fan("cpu", "CPU Fan", 1800.0, true)],
            )),
            next_hw_probe: Instant::now() + Duration::from_secs(3600),
            acpi: Vec::new(),
            next_acpi_probe: Instant::now() + Duration::from_secs(3600),
        };
        let report = probe.sample(0);
        assert_eq!(report.cpu.rpm, Some(1800.0));
        assert_eq!(report.cpu.source, "root\\OpenHardwareMonitor");
    }

    #[test]
    fn lenovo_hint_differs_from_generic() {
        assert!(cpu_hint(Some(Vendor::Lenovo)).contains("联想"));
        assert!(cpu_hint(Some(Vendor::Other)).contains("公开接口"));
        assert!(cpu_hint(None).contains("公开接口"));
    }

    /// 水泵/液冷传感器不能被当成 CPU 风扇——AIO 的泵速往往比风扇还高，
    /// 按转速取最大时会顶掉真正的风扇读数
    #[test]
    fn coolant_sensor_is_not_cpu_fan() {
        let fans = vec![
            fan("cpu", "CPU Fan", 1800.0, true),
            fan("cpu", "Water Pump", 3200.0, true),
        ];
        let r = pick_cpu_fan(&fans, "root\\OpenHardwareMonitor");
        assert_eq!(r.rpm, Some(1800.0));
    }

    /// 只有水泵时也不能拿泵速顶 CPU 风扇的位置（宁可显示 `--`）
    #[test]
    fn coolant_only_gives_no_cpu_fan() {
        let fans = vec![fan("cpu", "AIO Pump", 3200.0, true)];
        assert!(!pick_cpu_fan(&fans, "src").any());
    }

    /// GPU 侧同理：显卡水冷的泵速不算显卡风扇
    #[test]
    fn coolant_sensor_is_not_gpu_fan() {
        let fans = vec![
            fan("gpu", "GPU Fan", 1500.0, true),
            fan("gpu", "Liquid Pump", 4000.0, true),
        ];
        let r = pick_gpu_fan(&fans, "src", 0);
        assert_eq!(r.rpm, Some(1500.0));
    }

    #[test]
    fn no_fan_gives_empty_reading() {
        let fans = vec![fan("gpu", "GPU Fan", 1800.0, true)];
        let r = pick_cpu_fan(&fans, "src");
        assert!(!r.any());
        assert_eq!(r.source, "");
    }
}
