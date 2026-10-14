//! GPU 采集：NVML（NVIDIA 原生）优先，失败降级到 Windows PDH 性能计数器
//!
//! 为什么分两级：
//! - NVML 能给全部指标（占用 / 显存 / 温度 / 功耗 / 风扇），但只覆盖 NVIDIA；
//!   NVML 探针本体与 Linux 共用，见 [`crate::platform::gpu_nvml`]。
//! - PDH 的 `GPU Engine` 与 `GPU Adapter Memory` 是 Windows 内建计数器，
//!   对 Intel 核显、AMD 都有效，但拿不到温度（温度交给 `thermal.rs`）。

use std::collections::HashMap;

use windows::Win32::System::Performance::PDH_HCOUNTER;

use crate::model::GpuDevice;
use crate::pdh::Query;
use crate::platform::gpu_nvml::{clamp_pct, NvmlSource};

/// GPU Engine 各进程实例占用
const GPU_UTIL_PATH: &str = r"\GPU Engine(*)\Utilization Percentage";
/// 各适配器专用显存占用（byte）
const GPU_DEDICATED_PATH: &str = r"\GPU Adapter Memory(*)\Dedicated Usage";

/// PDH 兜底路径的计数器集合
struct PdhGpu {
    query: Query,
    util: PDH_HCOUNTER,
    dedicated: PDH_HCOUNTER,
    /// PDH 速率型计数器需要连续两次 Collect 才有值，首拍必然为空，用于给出说明文案
    warmed: bool,
    /// 显卡名（WMI 一次查询后缓存，PDH 本身不提供名称）
    names: Vec<String>,
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
        if let Some(devices) = self.nvml.sample() {
            if !devices.is_empty() {
                return (devices, "nvml".to_string());
            }
        }
        let (devices, warmed) = self.sample_pdh();
        (devices, if warmed { "pdh".to_string() } else { "pdh-warming".to_string() })
    }

    /* ------------------------------ PDH ------------------------------ */

    fn ensure_pdh(&mut self) -> Option<&mut PdhGpu> {
        if self.pdh.is_none() {
            let (query, util, dedicated) = {
                let query = Query::new()?;
                let util = query.add(GPU_UTIL_PATH)?;
                let dedicated = query.add(GPU_DEDICATED_PATH)?;
                (query, util, dedicated)
            };
            let warm = query.collect();
            // PDH 拿不到显卡名，向 WMI 要一次并缓存
            let names = crate::wmi::video_controller_names();
            self.pdh = Some(PdhGpu {
                query,
                util,
                dedicated,
                warmed: warm,
                names,
            });
        }
        self.pdh.as_mut()
    }

    fn sample_pdh(&mut self) -> (Vec<GpuDevice>, bool) {
        let Some(p) = self.ensure_pdh() else {
            return (Vec::new(), false);
        };
        p.query.collect();
        p.warmed = true;

        // key = 适配器标识（luid + phys），value = 该适配器上最大的引擎占用
        let mut util_by_adapter: HashMap<String, f32> = HashMap::new();
        for (instance, value) in p.query.double_array(p.util) {
            let Some(adapter) = adapter_key(&instance) else {
                continue;
            };
            let entry = util_by_adapter.entry(adapter).or_insert(0.0);
            if value > 0.0 {
                *entry = entry.max(value as f32);
            }
        }

        let mut mem_by_adapter: HashMap<String, u64> = HashMap::new();
        for (instance, value) in p.query.double_array(p.dedicated) {
            let Some(adapter) = adapter_key(&instance) else {
                continue;
            };
            let bytes = value.max(0.0) as u64;
            let entry = mem_by_adapter.entry(adapter).or_insert(0);
            *entry = (*entry).max(bytes);
        }

        if util_by_adapter.is_empty() && mem_by_adapter.is_empty() {
            return (Vec::new(), false);
        }

        let mut keys: Vec<String> = util_by_adapter.keys().cloned().collect();
        for k in mem_by_adapter.keys() {
            if !keys.contains(k) {
                keys.push(k.clone());
            }
        }
        keys.sort();

        // 提前取出显卡名，避免闭包里二次借用 self
        let names = p.names.clone();
        let total_named = names.len();

        let devices: Vec<GpuDevice> = keys
            .iter()
            .enumerate()
            .map(|(i, key)| {
                let usage = util_by_adapter.get(key).copied().map(clamp_pct);
                let used = mem_by_adapter.get(key).copied().unwrap_or(0);
                let name = if i < total_named {
                    names[i].clone()
                } else {
                    adapter_label(key, i)
                };
                GpuDevice {
                    index: i as u32,
                    name,
                    usage_pct: usage,
                    mem_used_bytes: used,
                    mem_total_bytes: 0,
                    mem_pct: None,
                    temp_c: None,
                    power_w: None,
                    // PDH 没有任何风扇计数器（实测只有 GPU Engine / GPU Adapter Memory），
                    // 兜底路径交给 fan.rs 的 WMI 通道，这里保持 None。
                    fan_pct: None,
                    fan_rpm: None,
                    source: "pdh".to_string(),
                }
            })
            .collect();

        (devices, true)
    }
}

/// 从 PDH 实例名里提取适配器标识
///
/// GPU Engine 的实例名形如：
/// `pid_11212_luid_0x00000000_0x00014E56_phys_0_eng_0_engtype_3D`
/// 这里取出中间的 `luid_..._phys_N` 作为适配器 key。
fn adapter_key(instance: &str) -> Option<String> {
    let start = instance.find("luid_")?;
    let rest = &instance[start..];
    let end_rel = rest.find("_eng_")?;
    Some(rest[..end_rel].to_string())
}

fn adapter_label(key: &str, index: usize) -> String {
    // key 形如 luid_0x00000000_0x00014E56_phys_0
    if let Some(pos) = key.find("_phys_") {
        let phys = &key[(pos + 6)..];
        return format!("图形适配器 {phys}(#{index})");
    }
    format!("图形适配器 #{index}")
}
