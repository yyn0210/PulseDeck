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
//! `%LOCALAPPDATA%\PulseDeck\pawnio.json`（见 [`crate::msr_bridge`]）。
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

use crate::fan_pref::FanPrefStore;
use crate::msr_bridge;
use crate::platform::Vendor;
use crate::wmi;

/// 厂商风扇读数：转速 + 来源描述
#[derive(Debug, Clone, PartialEq)]
pub struct VendorFan {
    /// 转速（RPM）
    pub rpm: f32,
    /// 来源描述，直接展示在界面上
    pub source: String,
}

impl Vendor {
    /// 识别整机厂商。机型不会变，内部已缓存结果。
    /// （枚举本体与 `label()` 在 `platform` 层——它们要进跨平台的数据类型；
    /// 识别手段才是 Windows 专属的，所以留在这里。）
    pub fn detect() -> Self {
        if wmi::is_lenovo() {
            Vendor::Lenovo
        } else {
            Vendor::Other
        }
    }
}

/// 厂商通道探针。
///
/// ⚠️ **无节流：每拍都读**。读数由提权桥接进程写进 `%LOCALAPPDATA%\PulseDeck\pawnio.json`，
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
    /// 用户手动指定的 CPU 风扇号（落盘在 `fan.json`，与设置页共用同一份句柄）
    pref: FanPrefStore,
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
            pref: FanPrefStore::load(),
            stub: None,
        }
    }

    /// 换成外部传入的偏好句柄。
    ///
    /// 必须注入而不是各读各的：`fan.json` 若被两个实例分别加载，
    /// 设置页改了配置而采样线程读的是启动时那份，改动就不会生效。
    pub fn set_pref(&mut self, pref: FanPrefStore) {
        self.pref = pref;
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
            Some(fans) => map_fans(
                &fans,
                wmi::lenovo_fan_max_speed(),
                self.pref.get().cpu_fan_id,
            ),
            None => (None, None),
        }
    }
}

/// 联想的 FanID 约定（来自 `LENOVO_FAN_TABLE_DATA` 的 `Fan_Id`，本机实测 0/0/1）
const CPU_FAN_ID: u8 = 0;
const GPU_FAN_ID: u8 = 1;

/// 拿不到曲线上限时的机械常识上限：民用风扇不可能转过 2 万 RPM
const RPM_CEILING_FALLBACK: f32 = 20_000.0;
/// 已知曲线上限时的容忍倍数（允许短时超速）
const RPM_OVERDRIVE: f32 = 1.5;
/// 固件常见的「未提供」哨兵：`u16` 全 1
const RPM_SENTINEL: f32 = 65_535.0;

/// 转速读数是否可信——剔除哨兵值与明显离谱的脏数据。
///
/// ⚠️ **故意不设下限门槛**：竞品（LiteMonitor）对主板 Super I/O 通道设了
/// `> 200 RPM` 的底噪过滤，因为那条通道上**没接风扇的风扇头会返回垃圾值**。
/// 我们这条是 BIOS 在 `LENOVO_FAN_TABLE_DATA` 里声明过的真实风扇，不存在那种
/// 空接口噪声；而 `0 RPM` 是风扇真停转的合法状态，砍掉它等于把「停转」显示成
/// 「读不到」。所以这里只防**明显不可信**的值，不防低转速。
pub(crate) fn plausible(rpm: f32, max: Option<f32>) -> bool {
    if !rpm.is_finite() || rpm < 0.0 {
        return false;
    }
    if rpm >= RPM_SENTINEL {
        return false;
    }
    let ceiling = match max {
        Some(m) if m > 0.0 => m * RPM_OVERDRIVE,
        _ => RPM_CEILING_FALLBACK,
    };
    rpm <= ceiling
}

/// 把桥接给出的 `(FanID, RPM)` 列表映射成 `(CPU, GPU)` 两个读数。
///
/// * `pref`：用户手动指定的 CPU 风扇号（`None` = 自动）。自动时按 FanID 约定取 0 号；
/// * 同一个风扇号不会被同时当成 CPU 和 GPU（`cpu_id` 先占位，GPU 从剩下的里挑）；
/// * 读数不可信的号直接跳过——宁可显示 `--`，也不拿别的风扇的数值冒充。
fn map_fans(
    fans: &[msr_bridge::LenovoFan],
    max: Option<f32>,
    pref: Option<u8>,
) -> (Option<VendorFan>, Option<VendorFan>) {
    let all: Vec<(u8, f32)> = fans.iter().map(|f| (f.fan_id, f.rpm)).collect();
    let valid: Vec<(u8, f32)> = all
        .iter()
        .copied()
        .filter(|(_, rpm)| plausible(*rpm, max))
        .collect();

    let cpu_id = choose_cpu_id(&all, &valid, pref);
    let cpu = cpu_id.and_then(|id| pick(&valid, id, max));
    let gpu = pick(&valid, GPU_FAN_ID, max)
        .filter(|_| Some(GPU_FAN_ID) != cpu_id)
        .or_else(|| {
            valid
                .iter()
                .filter(|(id, _)| Some(*id) != cpu_id)
                .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(_, rpm)| make(*rpm, max))
        });
    (cpu, gpu)
}

/// 挑 CPU 风扇号：**手动指定 > FanID 0 > 「压根没有 0 号」时取转得最快的**。
///
/// 最后那级兜底只在整机**没有 0 号风扇**时启用（说明 BIOS 没按 0=CPU 约定编号，
/// 此时每个号都是猜）。若 0 号存在但读数不可信，则如实返回 `None`
/// ——把 GPU 风扇的转速显示成 CPU 风扇属于编造，本项目不做。
fn choose_cpu_id(all: &[(u8, f32)], valid: &[(u8, f32)], pref: Option<u8>) -> Option<u8> {
    if let Some(id) = pref {
        if valid.iter().any(|(i, _)| *i == id) {
            return Some(id);
        }
    }
    if valid.iter().any(|(i, _)| *i == CPU_FAN_ID) {
        return Some(CPU_FAN_ID);
    }
    if all.iter().any(|(i, _)| *i == CPU_FAN_ID) {
        return None;
    }
    valid
        .iter()
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| *i)
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
            pref: FanPrefStore::in_memory(None),
            stub: Some((mk(cpu_rpm), mk(gpu_rpm))),
        }
    }

    /// 测试替身：厂商通道完全沉默（返回 (None, None)）
    pub fn silent_for_test() -> Self {
        Self {
            vendor: Vendor::Lenovo,
            pref: FanPrefStore::in_memory(None),
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
            pref: FanPrefStore::in_memory(None),
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
            pref: FanPrefStore::in_memory(None),
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
            pref: FanPrefStore::in_memory(None),
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
        let (cpu, gpu) = map_fans(&fans, Some(4500.0), None);
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
        let (cpu, gpu) = map_fans(&fans, None, None);
        assert_eq!(cpu.as_ref().map(|f| f.rpm), Some(2900.0));
        assert_eq!(gpu.as_ref().map(|f| f.rpm), Some(1800.0));
        assert_eq!(gpu.unwrap().source, "联想 EC");
    }

    /// 手动指定优先于 FanID 约定：把 1 号指定成 CPU 后，0 号让给 GPU
    #[test]
    fn map_fans_honors_manual_cpu_choice() {
        let fans = vec![
            msr_bridge::LenovoFan { fan_id: 0, rpm: 2900.0 },
            msr_bridge::LenovoFan { fan_id: 1, rpm: 2750.0 },
        ];
        let (cpu, gpu) = map_fans(&fans, Some(4500.0), Some(1));
        assert_eq!(cpu.as_ref().map(|f| f.rpm), Some(2750.0));
        assert_eq!(gpu.as_ref().map(|f| f.rpm), Some(2900.0));
    }

    /// 指定了一个不存在的号 → 退回自动，而不是什么都不显示
    #[test]
    fn map_fans_ignores_missing_manual_choice() {
        let fans = vec![msr_bridge::LenovoFan { fan_id: 0, rpm: 2900.0 }];
        let (cpu, _) = map_fans(&fans, Some(4500.0), Some(5));
        assert_eq!(cpu.as_ref().map(|f| f.rpm), Some(2900.0));
    }

    /// 同一个风扇不许同时当 CPU 和 GPU（只有一个风扇时 GPU 必须为空）
    #[test]
    fn map_fans_never_reuses_one_fan_for_both() {
        let fans = vec![msr_bridge::LenovoFan { fan_id: 0, rpm: 2900.0 }];
        let (cpu, gpu) = map_fans(&fans, Some(4500.0), None);
        assert_eq!(cpu.as_ref().map(|f| f.rpm), Some(2900.0));
        assert!(gpu.is_none());
    }

    /// 压根没有 0 号（BIOS 没按约定编号）→ 取转得最快的那个当 CPU
    #[test]
    fn map_fans_falls_back_to_fastest_without_zero() {
        let fans = vec![
            msr_bridge::LenovoFan { fan_id: 3, rpm: 1200.0 },
            msr_bridge::LenovoFan { fan_id: 4, rpm: 3100.0 },
        ];
        let (cpu, gpu) = map_fans(&fans, Some(4500.0), None);
        assert_eq!(cpu.as_ref().map(|f| f.rpm), Some(3100.0));
        assert_eq!(gpu.as_ref().map(|f| f.rpm), Some(1200.0));
    }

    /// 0 号存在但读数不可信 → 如实为空，**不拿 GPU 风扇冒充 CPU 风扇**
    #[test]
    fn untrusted_cpu_reading_is_not_substituted() {
        let fans = vec![
            msr_bridge::LenovoFan { fan_id: 0, rpm: 65535.0 },
            msr_bridge::LenovoFan { fan_id: 1, rpm: 2800.0 },
        ];
        let (cpu, gpu) = map_fans(&fans, Some(4500.0), None);
        assert!(cpu.is_none(), "CPU 读数不可信时应为 None，不能拿 1 号顶替");
        assert_eq!(gpu.as_ref().map(|f| f.rpm), Some(2800.0));
    }

    /// 脏值校验：哨兵 / 负数 / 超限 / 正常
    #[test]
    fn plausible_rejects_garbage() {
        assert!(!plausible(65535.0, Some(4500.0)));
        assert!(!plausible(-1.0, Some(4500.0)));
        assert!(!plausible(f32::NAN, Some(4500.0)));
        // 超过曲线上限 1.5 倍
        assert!(!plausible(9000.0, Some(4500.0)));
        assert!(plausible(5000.0, Some(4500.0)));
        // 停转是合法状态，不许被过滤掉
        assert!(plausible(0.0, Some(4500.0)));
        // 没有上限时退到机械常识上限
        assert!(!plausible(25000.0, None));
        assert!(plausible(3000.0, None));
    }

    /// 空列表（桥接在跑但一个风扇都没读到）→ 两个读数都是 None
    #[test]
    fn map_fans_handles_empty() {
        assert_eq!(map_fans(&[], Some(4500.0), None), (None, None));
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
