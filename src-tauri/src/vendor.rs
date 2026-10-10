//! 厂商专有的硬件读数通道
//!
//! Windows 上有一类指标**没有任何通用用户态接口**，只能靠厂商在 BIOS / EC 侧
//! 通过私有方式暴露。风扇转速就是典型：
//!
//! * 通用通道（NVML / ACPI `Win32_Fan` / PDH）在笔记本上基本拿不到 CPU 风扇；
//! * 主板 Super I/O 通道在笔记本上根本不存在（风扇由 EC 直接管理）；
//! * EC 私有 RAM 读写危险（会触发 EC 复位保护），不值得做。
//!
//! 但厂商自己会开一个受控的口子。本模块把这类**机型相关**的通道收拢到一处，
//! 让 `fan.rs` 不必关心厂商差异——新增厂商时只加一个 `Vendor` 分支即可。
//!
//! ## 当前支持的厂商
//!
//! | 厂商 | 通道 | 说明 |
//! | --- | --- | --- |
//! | 联想 | ACPI-WMI 映射（`root\WMI:LENOVO_FAN_METHOD`） | 由 BIOS/EC 实现，无需驱动 |
//!
//! ### ⚠️ 关键约束：联想通道**需要管理员权限**（实测，2026-10-10）
//!
//! 非提权时 `ExecMethod` 返回 `WBEM_E_ACCESS_DENIED(0x80041003)`，且与 COM
//! impersonation 级别无关（IMPERSONATE / DELEGATE / IDENTIFY 都一样失败）。
//! 提权后同一调用立即成功（`CurrentFanSpeed = 3003`）。
//!
//! 因此本模块**不在主进程里直接调 WMI**，而是读**提权桥接进程**写出的
//! `%LOCALAPPDATA%\WinMonitor\pawnio.json`（见 [`crate::msr_bridge`]）。
//! 那个桥接进程本来就要为 PawnIO MSR 温度提权——风扇搭它的便车，
//! **一次 UAC 同时搞定温度与风扇**。
//!
//! ### 刷新节奏（踩过坑，别再改回节流）
//!
//! 桥接进程 **1Hz** 写文件，本模块就**每拍读一次**、不设任何时间窗缓存。
//! 曾有一版 30 秒节流，结果风扇比同屏的温度/CPU/内存慢 30 倍——看起来就是「数值卡住不动」。
//! 读一个几百字节的本地文件是微秒级开销，没有任何节流的必要。
//!
//! 联想通道的实现细节见 `wmi.rs` 的「厂商通道」一节，分析方法见
//! `docs/LegionZone采集机制分析.md`。

use crate::msr_bridge;
use crate::wmi;

/// 厂商风扇读数：转速 + 来源描述
#[derive(Debug, Clone, PartialEq)]
pub struct VendorFan {
    /// 转速（RPM）
    pub rpm: f32,
    /// 来源描述，直接展示在界面上
    pub source: String,
}

/// 已识别的整机厂商
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vendor {
    Lenovo,
    /// 非联想，或未能识别
    Other,
}

impl Vendor {
    /// 识别整机厂商。机型不会变，内部已缓存结果。
    pub fn detect() -> Self {
        if wmi::is_lenovo() {
            Vendor::Lenovo
        } else {
            Vendor::Other
        }
    }

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

/// 厂商通道探针。
///
/// ⚠️ **无节流：每拍都读**。读数由提权桥接进程写进 `%LOCALAPPDATA%\WinMonitor\pawnio.json`，
/// 桥接本身是 1Hz 刷新的，而这里读的只是一个几百字节的本地文件（微秒级），
/// 所以直接跟采样节拍对齐即可。
///
/// 这里曾有一版 **30 秒**节流（当时的理由是「风扇转速变化慢」）。后果是风扇的刷新节奏
/// 比温度/CPU/内存等指标**慢 30 倍**——温度每秒都在动，风扇却半分钟才跳一次，
/// 看起来就像数值卡住了。风扇必须与其它指标**同频刷新**：桥接每秒给一次，界面就该每秒更新一次。
///
/// 让「每拍都读」变得廉价的配套改动：`wmi::lenovo_fan_max_speed()` 已改为进程内缓存
/// （该值由 BIOS 固定），否则每拍都会触发一次 WMI 查询。
pub struct VendorProbe {
    vendor: Vendor,
    /// 测试替身：`Some` 时跳过桥接文件、直接返回该读数。
    /// 生产路径恒为 `None`（只有测试构造器会设置它）。
    stub: Option<(Option<VendorFan>, Option<VendorFan>)>,
}

impl Default for VendorProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl VendorProbe {
    pub fn new() -> Self {
        Self {
            vendor: Vendor::detect(),
            stub: None,
        }
    }

    /// 识别到的厂商（供界面展示）
    pub fn vendor(&self) -> Vendor {
        self.vendor
    }

    /// 取当前厂商风扇读数：`(CPU, GPU)`。两者都可能为 `None`。
    ///
    /// 只在**联想机型**上工作；其它厂商直接返回 `(None, None)`，
    /// 上层会自然降级到通用通道。
    pub fn sample(&mut self) -> (Option<VendorFan>, Option<VendorFan>) {
        if self.vendor != Vendor::Lenovo {
            return (None, None);
        }
        if let Some(stub) = &self.stub {
            return stub.clone();
        }
        // 桥接没在跑 / 读数过期（>8s）/ 本机没暴露该通道 → 一律 None，上层降级。
        // 不缓存上一次的读数：宁可在桥接停掉后如实回到 `--`，也不要留一个冻住不动的旧值。
        match msr_bridge::read_fresh_fans() {
            Some(fans) => map_fans(&fans, wmi::lenovo_fan_max_speed()),
            None => (None, None),
        }
    }
}

/// 把桥接给出的 `(FanID, RPM)` 列表映射成 `(CPU, GPU)` 两个读数。
///
/// 联想的 FanID 约定：**0 = CPU，1 = GPU**（来自 `LENOVO_FAN_TABLE_DATA` 的 `Fan_Id`
/// 取值，本机实测为 0/0/1）。取不到 1 号时按「第一个非 0 号」兜底。
fn map_fans(fans: &[msr_bridge::LenovoFan], max: Option<f32>) -> (Option<VendorFan>, Option<VendorFan>) {
    let pairs: Vec<(u8, f32)> = fans.iter().map(|f| (f.fan_id, f.rpm)).collect();
    let cpu = pick(&pairs, 0, max);
    let gpu = pick(&pairs, 1, max).or_else(|| {
        pairs
            .iter()
            .find(|(id, _)| *id != 0)
            .map(|(_, rpm)| make(*rpm, max))
    });
    (cpu, gpu)
}

/// 从 `(fan_id, rpm)` 列表里取指定号，换算成 `VendorFan`
fn pick(all: &[(u8, f32)], id: u8, max: Option<f32>) -> Option<VendorFan> {
    all.iter()
        .find(|(fid, _)| *fid == id)
        .map(|(_, rpm)| make(*rpm, max))
}

/// 来源描述里带的「上限」是同一次采样的旁证，拿不到就退化为不带数字
fn make(rpm: f32, max: Option<f32>) -> VendorFan {
    let source = match max {
        Some(m) => format!("联想 EC（上限 {:.0} RPM）", m),
        None => "联想 EC".to_string(),
    };
    VendorFan { rpm, source }
}

#[cfg(test)]
impl VendorProbe {
    /// 测试替身：跳过桥接文件、注入固定读数，用于验证优先级
    pub fn with_fans_for_test(cpu_rpm: Option<f32>, gpu_rpm: Option<f32>) -> Self {
        let mk = |r: Option<f32>| {
            r.map(|rpm| VendorFan {
                rpm,
                source: "联想 EC".to_string(),
            })
        };
        Self {
            vendor: Vendor::Lenovo,
            stub: Some((mk(cpu_rpm), mk(gpu_rpm))),
        }
    }

    /// 测试替身：厂商通道完全沉默（返回 (None, None)）
    pub fn silent_for_test() -> Self {
        Self {
            vendor: Vendor::Lenovo,
            stub: Some((None, None)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn other_vendor_yields_nothing() {
        let mut p = VendorProbe {
            vendor: Vendor::Other,
            stub: None,
        };
        assert_eq!(p.sample(), (None, None));
    }

    #[test]
    fn vendor_labels() {
        assert_eq!(Vendor::Lenovo.label(), "联想");
        assert_eq!(Vendor::Other.label(), "通用");
    }

    #[test]
    fn injected_fans_are_returned_verbatim() {
        let cpu = VendorFan {
            rpm: 2400.0,
            source: "联想 EC".to_string(),
        };
        let mut p = VendorProbe {
            vendor: Vendor::Lenovo,
            stub: Some((Some(cpu), None)),
        };
        let (c, g) = p.sample();
        assert_eq!(c.map(|f| f.rpm), Some(2400.0));
        assert!(g.is_none());
    }

    /// **回归：风扇不再被节流。**
    ///
    /// 旧实现用一个 30s 的 `next_probe` 门控刷新，导致桥接明明每秒都在更新、
    /// 界面却半分钟才跳一次。这里验证「连续两次采样读到的是同一份输入」——
    /// 即 `sample()` 每次都重新映射当前输入，没有任何时间窗缓存。
    #[test]
    fn sampling_has_no_time_window_cache() {
        let mut p = VendorProbe {
            vendor: Vendor::Lenovo,
            stub: Some((Some(make(1000.0, None)), None)),
        };
        assert_eq!(p.sample().0.map(|f| f.rpm), Some(1000.0));
        // 换一份输入，立刻就要反映出来（旧实现这里会返回 30s 前的旧值）
        p.stub = Some((Some(make(2000.0, None)), None));
        assert_eq!(p.sample().0.map(|f| f.rpm), Some(2000.0));
    }

    /// FanID 0 → CPU、1 → GPU
    #[test]
    fn map_fans_splits_cpu_and_gpu() {
        let fans = vec![
            msr_bridge::LenovoFan { fan_id: 0, rpm: 2900.0 },
            msr_bridge::LenovoFan { fan_id: 1, rpm: 2750.0 },
        ];
        let (cpu, gpu) = map_fans(&fans, Some(4500.0));
        assert_eq!(cpu.as_ref().map(|f| f.rpm), Some(2900.0));
        assert_eq!(gpu.as_ref().map(|f| f.rpm), Some(2750.0));
        // 来源文案带上曲线上限
        assert!(cpu.unwrap().source.contains("4500"));
    }

    /// 没有 1 号风扇时，GPU 兜底到「第一个非 0 号」
    #[test]
    fn map_fans_falls_back_for_gpu() {
        let fans = vec![
            msr_bridge::LenovoFan { fan_id: 0, rpm: 2900.0 },
            msr_bridge::LenovoFan { fan_id: 2, rpm: 1800.0 },
        ];
        let (cpu, gpu) = map_fans(&fans, None);
        assert_eq!(cpu.as_ref().map(|f| f.rpm), Some(2900.0));
        assert_eq!(gpu.as_ref().map(|f| f.rpm), Some(1800.0));
        assert_eq!(gpu.unwrap().source, "联想 EC");
    }

    /// 空列表（桥接在跑但一个风扇都没读到）→ 两个读数都是 None
    #[test]
    fn map_fans_handles_empty() {
        assert_eq!(map_fans(&[], Some(4500.0)), (None, None));
    }

    /// CPU/GPU 的选择逻辑：FanID 0 → CPU，FanID 1 → GPU
    #[test]
    fn pick_maps_fan_ids() {
        let pairs = [(0u8, 3000.0f32), (1, 2500.0)];
        assert_eq!(pick(&pairs, 0, Some(4500.0)).map(|f| f.rpm), Some(3000.0));
        assert_eq!(pick(&pairs, 1, Some(4500.0)).map(|f| f.rpm), Some(2500.0));
        assert!(pick(&pairs, 2, Some(4500.0)).is_none());
    }

    /// 来源文案带上曲线表上限（本机实测 4500）
    #[test]
    fn source_mentions_max_speed() {
        let f = make(3000.0, Some(4500.0));
        assert!(f.source.contains("4500"));
        assert!(f.source.contains("联想"));
        // 拿不到上限时退化为不带数字
        assert_eq!(make(3000.0, None).source, "联想 EC");
    }

    /// 桥接写出的 JSON 能被主进程解析出风扇（字段名 camelCase 对齐）
    #[test]
    fn bridge_json_fans_parse() {
        let text = r#"{
            "ts": 4000000000,
            "parentPid": 1234,
            "cpuTempC": 62.0,
            "tjmaxC": 100.0,
            "source": "Intel MSR (PawnIO)",
            "error": null,
            "lenovoFans": [{"fanId": 0, "rpm": 3003.0}, {"fanId": 1, "rpm": 2800.0}]
        }"#;
        let st: crate::msr_bridge::MsrState = serde_json::from_str(text).unwrap();
        let fans = st.lenovo_fans.expect("应解析出 lenovoFans");
        assert_eq!(fans.len(), 2);
        assert_eq!(fans[0].fan_id, 0);
        assert_eq!(fans[0].rpm, 3003.0);
        assert_eq!(fans[1].fan_id, 1);
    }

    /// 非联想机型不写 `lenovoFans` 字段，解析后应为 None（而不是空列表）
    #[test]
    fn bridge_json_without_fans() {
        let text = r#"{"ts":1,"parentPid":9,"cpuTempC":null,"tjmaxC":null,"source":"x","error":"y"}"#;
        let st: crate::msr_bridge::MsrState = serde_json::from_str(text).unwrap();
        assert!(st.lenovo_fans.is_none());
    }
}
