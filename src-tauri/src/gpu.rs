//! GPU 采集：NVML（NVIDIA 原生）与 PDH（Windows 内建性能计数器）**同时采样、合并展示**
//!
//! 为什么要两条一起走：
//! - **NVML** 覆盖 NVIDIA 且指标最全（占用 / 显存 / 温度 / 功耗 / 风扇），
//!   但只认 NVIDIA；探针本体与 Linux 共用，见 [`crate::platform::gpu_nvml`]。
//! - **PDH** 的 `GPU Engine` / `GPU Adapter Memory` 是 Windows 内建计数器，
//!   对 Intel 核显、AMD 都有效（占用 + 专用/共享内存），但没有温度与功耗
//!   （Windows 没有公开的跨厂商 GPU 温度接口，温度交给 `thermal.rs` 的
//!   硬件监控软件通道）。
//!
//! 早先的版本只在 NVML 不可用时才走 PDH，于是「N 卡 + Intel 核显」的机器上
//! 核显永远不出现；PDH 那条的显卡名还是按「PDH 实例顺序 == WMI 显卡列表顺序」
//! 猜的 —— 两者毫无关系，多显卡机器上必然张冠李戴。现在：
//!
//! * 两条通道都跑，NVML 已经报过的适配器不再由 PDH 重复列出；
//! * 适配器身份（名字 / 厂商 / 显存总量 / 是否软件适配器）向 DXGI 要
//!   （[`crate::gpu_dxgi`]），并按 **LUID** 与 PDH 实例精确对上。

use std::collections::{HashMap, HashSet};

use windows::Win32::System::Performance::PDH_HCOUNTER;

use crate::gpu_dxgi::{self, AdapterInfo};
use crate::model::GpuDevice;
use crate::pdh::Query;
use crate::platform::gpu_nvml::{clamp_pct, NvmlSource};

/// GPU Engine 各进程实例占用
const GPU_UTIL_PATH: &str = r"\GPU Engine(*)\Utilization Percentage";
/// 各适配器专用显存占用（byte）
const GPU_DEDICATED_PATH: &str = r"\GPU Adapter Memory(*)\Dedicated Usage";
/// 各适配器共享内存占用（byte）—— 核显的显存主要在这里（专用段只是个固定的小 carve-out）
const GPU_SHARED_PATH: &str = r"\GPU Adapter Memory(*)\Shared Usage";

/// `WB_GPU_DIAG=1` 时把 PDH 原始实例与 DXGI 身份打到 stderr（与 WB_WMI_DIAG 同一套习惯）
fn diag_on() -> bool {
    std::env::var("WB_GPU_DIAG").map(|v| v == "1").unwrap_or(false)
}

/// PDH 兜底路径的计数器集合 + DXGI 给出的适配器身份
struct PdhGpu {
    query: Query,
    util: PDH_HCOUNTER,
    dedicated: PDH_HCOUNTER,
    /// 可选：少数系统没有 `Shared Usage` 计数器（缺了只是核显显存口径退化为专用段）
    shared: Option<PDH_HCOUNTER>,
    /// PDH 速率型计数器需要连续两次 Collect 才有值，首拍可能为空，用于给出说明文案
    warmed: bool,
    /// DXGI 一次枚举的适配器身份（缓存；DXGI 不可用时为空表）
    adapters: Vec<AdapterInfo>,
}

// PDH 句柄只在同一个线程内使用，跨线程移交所有权是安全的
unsafe impl Send for PdhGpu {}

pub struct GpuProbe {
    /// NVML 有状态探针（Windows / Linux 共用实现）
    nvml: NvmlSource,
    pdh: Option<PdhGpu>,
}

impl Default for GpuProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl GpuProbe {
    pub fn new() -> Self {
        Self {
            nvml: NvmlSource::new(),
            pdh: None,
        }
    }

    /// 采样一次。返回 (设备列表, 数据来源标识)
    pub fn sample(&mut self) -> (Vec<GpuDevice>, String) {
        // NVML：N 卡一条龙（占用 / 显存 / 温度 / 功耗 / 风扇）
        let mut devices = self.nvml.sample().unwrap_or_default();
        // PDH + DXGI：其余适配器（Intel 核显 / A 卡……），N 卡已经报过的不重复列
        let (others, warmed) = self.sample_adapters(&devices);
        devices.extend(others);

        // 序号按最终列表顺序重排：列表前半是 NVML、后半是 PDH，序号必须跟着走
        for (i, dev) in devices.iter_mut().enumerate() {
            dev.index = i as u32;
        }

        if devices.is_empty() {
            let source = if warmed { "unavailable" } else { "pdh-warming" };
            return (devices, source.to_string());
        }
        // 有设备时以各设备的来源为准（`GpuInfo::aggregate` 会重新算一遍）
        let source = crate::model::device_sources(&devices);
        (devices, source)
    }

    /* ------------------------------ PDH ------------------------------ */

    fn ensure_pdh(&mut self) -> Option<&mut PdhGpu> {
        if self.pdh.is_none() {
            let (query, util, dedicated, shared) = {
                let query = Query::new()?;
                let util = query.add(GPU_UTIL_PATH)?;
                let dedicated = query.add(GPU_DEDICATED_PATH)?;
                // 共享内存计数器不是所有系统都有：加不上就只是少一个字段
                let shared = query.add(GPU_SHARED_PATH);
                (query, util, dedicated, shared)
            };
            let warm = query.collect();
            // PDH 实例名只有 LUID，显卡名 / 显存总量 / 厂商都要向 DXGI 要一次并缓存
            let adapters = gpu_dxgi::enumerate();
            if diag_on() {
                for a in &adapters {
                    let luid = a.luid;
                    eprintln!(
                        "[gpu] dxgi luid=0x{luid:016X} vendor=0x{:04X} dev=0x{:04X} \
                         ded={} shared={} sw={} {}",
                        a.vendor_id,
                        a.device_id,
                        a.dedicated_video_memory,
                        a.shared_system_memory,
                        a.is_software,
                        a.name
                    );
                }
            }
            self.pdh = Some(PdhGpu {
                query,
                util,
                dedicated,
                shared,
                warmed: warm,
                adapters,
            });
        }
        self.pdh.as_mut()
    }

    /// 产出「NVML 没报过的那些显示适配器」的设备条目。
    /// 返回 (设备列表, PDH 是否已积累到可用读数)
    fn sample_adapters(&mut self, nvml: &[GpuDevice]) -> (Vec<GpuDevice>, bool) {
        let Some(p) = self.ensure_pdh() else {
            return (Vec::new(), false);
        };
        p.query.collect();
        p.warmed = true;

        let util_rows = p.query.double_array(p.util);
        let ded_rows = p.query.double_array(p.dedicated);
        let shared_rows = p
            .shared
            .map(|c| p.query.double_array(c))
            .unwrap_or_default();

        if diag_on() {
            for (tag, rows) in [
                ("util", &util_rows),
                ("dedicated", &ded_rows),
                ("shared", &shared_rows),
            ] {
                for (instance, value) in rows {
                    eprintln!("[gpu] pdh {tag} {value:.1} {instance}");
                }
            }
            for d in nvml {
                eprintln!("[gpu] nvml {} util={:?}", d.name, d.usage_pct);
            }
        }

        // 同一 LUID 可能有多条实例（多进程引擎、phys_0/1），占用取最大、内存取最大
        let util_by_luid = max_by_luid(&util_rows);
        let ded_by_luid = max_by_luid(&ded_rows);
        let shared_by_luid = max_by_luid(&shared_rows);

        let nvml_names: Vec<String> = nvml.iter().map(|d| normalize_name(&d.name)).collect();
        // DXGI 会把同一块物理适配器重复枚举（见 dedup_adapters），先折叠再判覆盖
        let adapters = dedup_adapters(&p.adapters, &|luid| {
            util_by_luid.contains_key(&luid)
                || ded_by_luid.contains_key(&luid)
                || shared_by_luid.contains_key(&luid)
        });
        // 软件适配器（WARP）只在「一块真显卡都没有」时才需要列出来
        let real_adapters = adapters.iter().filter(|a| !a.is_software).count();
        let nvidia_adapters = adapters
            .iter()
            .filter(|a| a.is_nvidia() && !a.is_software)
            .count();

        let mut devices = Vec::new();
        let mut seen: HashSet<u64> = HashSet::new();
        for adapter in &adapters {
            seen.insert(adapter.luid);
            if adapter.is_software && real_adapters > 0 {
                continue;
            }
            if nvml_covers(adapter, &nvml_names, nvidia_adapters, nvml.len()) {
                continue;
            }
            let Some(dev) = adapter_device(
                adapter,
                util_by_luid.get(&adapter.luid).copied(),
                ded_by_luid.get(&adapter.luid).copied().unwrap_or(0.0),
                shared_by_luid.get(&adapter.luid).copied().unwrap_or(0.0),
                ded_by_luid.contains_key(&adapter.luid) || shared_by_luid.contains_key(&adapter.luid),
            ) else {
                continue;
            };
            devices.push(dev);
        }

        // DXGI 没枚举到、但 PDH 有读数的 LUID：枚举失败或外接显卡坞热插拔的残影。
        // 此时没有身份可查，用回退标签列出来（有读数就展示，不因为查不到名字就丢掉）。
        // 例外：DXGI 整表为空而 NVML 有卡时一律不列 —— 无法区分那些 LUID 是哪块卡，
        // 宁可只显示 NVML 那一份，也不要同一块卡重复出现两行。
        if !(p.adapters.is_empty() && !nvml.is_empty()) {
            let mut leftover: Vec<u64> = util_by_luid
                .keys()
                .chain(ded_by_luid.keys())
                .chain(shared_by_luid.keys())
                .copied()
                .filter(|luid| !seen.contains(luid))
                .collect();
            leftover.sort_unstable();
            leftover.dedup();
            for (i, luid) in leftover.iter().enumerate() {
                let used = ded_by_luid.get(luid).copied().unwrap_or(0.0);
                devices.push(GpuDevice {
                    index: 0,
                    name: format!("图形适配器 #{i}"),
                    usage_pct: util_by_luid.get(luid).copied().map(|v| clamp_pct(v as f32)),
                    mem_used_bytes: used.max(0.0) as u64,
                    // 没有身份就不知道显存总量，如实留 0（界面只显示已用量）
                    mem_total_bytes: 0,
                    mem_pct: None,
                    temp_c: None,
                    power_w: None,
                    fan_pct: None,
                    fan_rpm: None,
                    source: "pdh".to_string(),
                });
            }
        }

        (devices, true)
    }
}

/// 折叠 DXGI 重复枚举的适配器
///
/// DXGI 会把同一块物理适配器按「输出 / 混合显卡组」重复列好几条、LUID 各不相同
/// （本机实测 Intel Iris Xe 3 条、RTX 3060 2 条，其中只有一条有 PDH 读数）。
/// 按「厂商 + 设备号 + 型号 + 显存」分组，组内**有读数的优先**：
/// 该组只要有 LUID 拿得到 PDH 读数，就只保留拿到读数的那几条（真·双卡同型号
/// 两块都会各自有读数，因此都会留下；重复的残影没有读数，会被丢掉）。
fn dedup_adapters(adapters: &[AdapterInfo], has_reading: &dyn Fn(u64) -> bool) -> Vec<AdapterInfo> {
    let mut out: Vec<AdapterInfo> = Vec::new();
    for a in adapters {
        let same_group = |b: &AdapterInfo| {
            b.vendor_id == a.vendor_id
                && b.device_id == a.device_id
                && b.name == a.name
                && b.dedicated_video_memory == a.dedicated_video_memory
                && b.shared_system_memory == a.shared_system_memory
                && b.is_software == a.is_software
        };
        let group_has_reading = adapters.iter().filter(|b| same_group(b)).any(|b| has_reading(b.luid));
        if group_has_reading && !has_reading(a.luid) {
            continue;
        }
        out.push(a.clone());
    }
    out
}

/// 由 DXGI 身份 + PDH 读数组装一条设备记录；连占用与显存都没有就不报这块卡
fn adapter_device(
    adapter: &AdapterInfo,
    util: Option<f64>,
    ded_used: f64,
    shared_used: f64,
    has_mem: bool,
) -> Option<GpuDevice> {
    if util.is_none() && !has_mem {
        return None;
    }
    let ded_used = ded_used.max(0.0) as u64;
    let shared_used = shared_used.max(0.0) as u64;
    // 核显报「专用 + 共享」：专用段只是个固定的小 carve-out（Intel 实测 128 MiB），
    // 单看它会读出「128 MiB 里塞了 300 MiB」这种荒谬值；独显报专用段，
    // 与 NVML 的口径一致，也符合「显存」的直觉。
    let (mem_used, mem_total) = if adapter.is_integrated() {
        (
            ded_used + shared_used,
            adapter.dedicated_video_memory + adapter.shared_system_memory,
        )
    } else {
        (ded_used, adapter.dedicated_video_memory)
    };
    let mem_pct = (mem_total > 0).then(|| clamp_pct(mem_used as f32 / mem_total as f32 * 100.0));
    Some(GpuDevice {
        index: 0,
        name: adapter.name.clone(),
        usage_pct: util.map(|v| clamp_pct(v as f32)),
        mem_used_bytes: mem_used,
        mem_total_bytes: mem_total,
        mem_pct,
        // Windows 没有公开的跨厂商 GPU 温度 / 功耗 / 风扇接口，
        // 温度由 thermal.rs 的硬件监控软件通道单独给（OHM / 旧版 LHM）
        temp_c: None,
        power_w: None,
        fan_pct: None,
        fan_rpm: None,
        source: "pdh".to_string(),
    })
}

/// NVML 是否已经报过这块适配器（两条通道不能把同一块卡列两遍）
///
/// ① 归一化名字相同即认定同一块卡：DXGI 的 `Description` 与 NVML 的 `name`
///    对 NVIDIA 卡通常是同一个型号串（本机实测逐字相同）；
/// ② 名字写法有差异时退一步看数量：NVML 报出的设备数不少于本机 NVIDIA
///    适配器数，就认为 NVIDIA 的卡都已覆盖。
fn nvml_covers(
    adapter: &AdapterInfo,
    nvml_names: &[String],
    nvidia_adapters: usize,
    nvml_devices: usize,
) -> bool {
    if !adapter.is_nvidia() {
        return false;
    }
    if nvml_names.iter().any(|n| *n == normalize_name(&adapter.name)) {
        return true;
    }
    nvml_devices >= nvidia_adapters
}

/// 型号串归一化（只用于比较：统一小写、压缩空白）
fn normalize_name(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// 从 PDH 实例名里取出适配器 LUID（u64，与 [`gpu_dxgi::AdapterInfo::luid`] 同序）
///
/// GPU Engine 的实例名形如：
/// `pid_11212_luid_0x00000000_0x00014E56_phys_0_eng_0_engtype_3D`
/// `GPU Adapter Memory` 的形如：
/// `luid_0x00000000_0x00014E56_phys_0`
/// 两处的 LUID 都是「先高位后低位」的两个十六进制数。
fn parse_luid(instance: &str) -> Option<u64> {
    let start = instance.find("luid_")?;
    let mut parts = instance[start + "luid_".len()..].split('_');
    let high = u32::from_str_radix(parts.next()?.trim_start_matches("0x"), 16).ok()?;
    let low = u32::from_str_radix(parts.next()?.trim_start_matches("0x"), 16).ok()?;
    Some(gpu_dxgi::pack_luid(low, high as i32))
}

/// 按 LUID 归并读数：同一 LUID 的多个实例取最大值
/// （占用是「该适配器上最大引擎」的口径，显存同 LUID 只会有一条，取最大即取那条）
fn max_by_luid(rows: &[(String, f64)]) -> HashMap<u64, f64> {
    let mut out: HashMap<u64, f64> = HashMap::new();
    for (instance, value) in rows {
        let Some(luid) = parse_luid(instance) else {
            continue;
        };
        let entry = out.entry(luid).or_insert(f64::MIN);
        *entry = entry.max(*value);
    }
    out
}

/// 列表级来源标识（`nvml` / `pdh` / `nvml+pdh`…）由 `model::device_sources` 统一给出
/// —— Windows 与 Linux 两条通道都要用，放在公共层，别在本文件里再实现一遍。

#[cfg(test)]
mod tests {
    use super::*;

    fn adapter(luid: u64, name: &str, vendor: u32, ded: u64, shared: u64) -> AdapterInfo {
        AdapterInfo {
            luid,
            name: name.to_string(),
            vendor_id: vendor,
            device_id: 0,
            dedicated_video_memory: ded,
            shared_system_memory: shared,
            is_software: false,
        }
    }

    #[test]
    fn luid_parsed_from_real_pdh_instances() {
        // 占用计数器的实例名（带 pid 与引擎后缀）
        assert_eq!(
            parse_luid("pid_11212_luid_0x00000000_0x00014E56_phys_0_eng_0_engtype_3D"),
            Some(0x0001_4E56)
        );
        // 显存计数器的实例名（没有 pid / 引擎后缀）
        assert_eq!(
            parse_luid("luid_0x00000000_0x00014E56_phys_0"),
            Some(0x0001_4E56)
        );
        // 高位不为 0 的机器
        assert_eq!(
            parse_luid("pid_4_luid_0x00000002_0x00000001_phys_0_eng_1_engtype_Copy"),
            Some(0x0000_0002_0000_0001)
        );
        assert_eq!(parse_luid("pid_4_eng_0_engtype_3D"), None, "没有 LUID 的实例");
        assert_eq!(parse_luid("luid_zzzz_0x1_phys_0"), None, "非法十六进制");
    }

    #[test]
    fn max_by_luid_keeps_largest_per_adapter() {
        let rows = vec![
            ("pid_1_luid_0x0_0xAA_phys_0_eng_0_engtype_3D".to_string(), 12.0),
            ("pid_2_luid_0x0_0xAA_phys_0_eng_1_engtype_Copy".to_string(), 40.0),
            ("pid_1_luid_0x0_0xBB_phys_0_eng_0_engtype_3D".to_string(), 3.0),
        ];
        let m = max_by_luid(&rows);
        assert_eq!(m.get(&0xAA), Some(&40.0));
        assert_eq!(m.get(&0xBB), Some(&3.0));
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn nvidia_coverage_by_name_then_count() {
        let names = vec![normalize_name("NVIDIA GeForce RTX 3060 Laptop GPU")];
        let nv = adapter(1, "NVIDIA GeForce RTX 3060 Laptop GPU", gpu_dxgi::VENDOR_NVIDIA, 0, 0);
        assert!(nvml_covers(&nv, &names, 1, 1), "型号串逐字相同即同一块卡");

        // 型号串写法有差异（驱动版本不同）时按数量兜底
        let other = adapter(2, "NVIDIA GeForce RTX 3060 Laptop GPU (rev.2)", gpu_dxgi::VENDOR_NVIDIA, 0, 0);
        assert!(nvml_covers(&other, &names, 1, 1));

        // NVML 漏了第二块 N 卡（设备数少于适配器数）→ 不能算覆盖，PDH 要把它列出来
        assert!(!nvml_covers(&other, &names, 2, 1));
        // 非 N 卡永远不由 NVML 覆盖
        let igpu = adapter(3, "Intel(R) UHD Graphics", gpu_dxgi::VENDOR_INTEL, 0, 0);
        assert!(!nvml_covers(&igpu, &names, 1, 1));
    }

    #[test]
    fn integrated_reports_dedicated_plus_shared() {
        let igpu = adapter(0xAA, "Intel(R) UHD Graphics", gpu_dxgi::VENDOR_INTEL, 128 << 20, 8 << 30);
        let dev = adapter_device(&igpu, Some(7.0), (300 << 20) as f64, (900 << 20) as f64, true)
            .expect("有读数就报卡");
        assert_eq!(dev.mem_used_bytes, (300 + 900) << 20);
        assert_eq!(dev.mem_total_bytes, (128 << 20) + (8 << 30));
        assert_eq!(dev.usage_pct, Some(7.0));
        assert_eq!(dev.source, "pdh");
    }

    #[test]
    fn discrete_reports_dedicated_only() {
        let dgpu = adapter(0xBB, "AMD Radeon RX 6800", gpu_dxgi::VENDOR_AMD, 16 << 30, 8 << 30);
        let dev = adapter_device(&dgpu, None, (2u64 << 30) as f64, (1u64 << 30) as f64, true)
            .expect("有显存读数就报卡");
        assert_eq!(dev.mem_used_bytes, 2 << 30, "独显只看专用段，不把共享内存算进去");
        assert_eq!(dev.mem_total_bytes, 16 << 30);
        assert_eq!(dev.usage_pct, None);
    }

    #[test]
    fn adapter_without_any_reading_is_not_reported() {
        let igpu = adapter(0xAA, "Intel(R) UHD Graphics", gpu_dxgi::VENDOR_INTEL, 128 << 20, 8 << 30);
        assert!(adapter_device(&igpu, None, 0.0, 0.0, false).is_none());
    }

    #[test]
    fn dedup_keeps_only_the_luid_with_readings() {
        // 本机实测：同一块 Intel 核显被 DXGI 列了 3 条，只有一条拿得到 PDH 读数
        let dup = |luid: u64| adapter(luid, "Intel(R) Iris(R) Xe Graphics", gpu_dxgi::VENDOR_INTEL, 128 << 20, 24 << 30);
        let list = vec![dup(0x171FA), dup(0xB794E), dup(0x3A656)];
        let with_reading = |luid: u64| luid == 0x171FA;
        let out = dedup_adapters(&list, &with_reading);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].luid, 0x171FA);

        // 同型号的两块真卡各自都有读数 → 都要留下
        let dual = vec![dup(0x1), dup(0x2)];
        let both = |_: u64| true;
        assert_eq!(dedup_adapters(&dual, &both).len(), 2);

        // 全组都没有读数（未激活的适配器）→ 原样保留，交给「无读数不报」那条过滤
        let none = |_: u64| false;
        assert_eq!(dedup_adapters(&dual, &none).len(), 2);
    }

    #[test]
    fn sources_are_reported_from_devices() {
        let mk = |source: &str| GpuDevice {
            source: source.to_string(),
            ..GpuDevice::default()
        };
        let devs = [mk("nvml"), mk("pdh"), mk("nvml")];
        assert_eq!(crate::model::device_sources(&devs), "nvml+pdh");
        assert_eq!(crate::model::device_sources(&[mk("amdgpu")]), "amdgpu");
    }
}
