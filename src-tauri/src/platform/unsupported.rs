//! 尚未适配平台的占位实现
//!
//! 与项目「数据源诚实降级」的哲学一致：读不到就报「读不到 + 具体原因」，
//! 绝不编造数值。所有通道统一引用 [`super::UNSUPPORTED`] 文案，前端显示 `--`
//! 并展示原因。
//!
//! 本模块在**所有平台**都参与编译（见 `mod.rs` 的说明），但只在非 Windows
//! 目标被 [`super::sources`] 选中。将来某平台接入真实实现后，该平台自然
//! 不再走到这里。

use super::{
    BatterySource, FanReading, FanReport, FanSource, GpuSource, MemoryExtras, MemoryStatus,
    PlatformSources, ThermalSource, UNSUPPORTED,
};
use crate::fan_pref::FanPrefStore;
use crate::model::{BatteryInfo, GpuDevice, ThermalInfo};

struct UnsupportedGpu;

impl GpuSource for UnsupportedGpu {
    fn sample(&mut self) -> (Vec<GpuDevice>, String) {
        // 空设备列表：model::GpuInfo::aggregate 会把 source 归一为 "unavailable"
        (Vec::new(), "unavailable".to_string())
    }
}

struct UnsupportedThermal;

impl ThermalSource for UnsupportedThermal {
    fn sample(&mut self) -> ThermalInfo {
        ThermalInfo {
            cpu_source: "不支持".to_string(),
            gpu_source: "不支持".to_string(),
            available: false,
            hint: UNSUPPORTED.to_string(),
            ..ThermalInfo::default()
        }
    }
}

struct UnsupportedFan;

impl FanSource for UnsupportedFan {
    fn set_pref(&mut self, _pref: FanPrefStore) {}

    fn sample(&mut self, gpu_count: usize) -> FanReport {
        // GPU 槽位照常预留（sampler 按下标回填），只是全部没有读数
        FanReport {
            gpus: vec![FanReading::default(); gpu_count],
            hint: UNSUPPORTED.to_string(),
            ..FanReport::default()
        }
    }
}

struct UnsupportedBattery;

impl BatterySource for UnsupportedBattery {
    fn collect(&mut self) -> BatteryInfo {
        BatteryInfo {
            has_battery: false,
            source: "unavailable".to_string(),
            status_text: UNSUPPORTED.to_string(),
            ..BatteryInfo::default()
        }
    }
}

struct UnsupportedMemoryExtras;

impl MemoryExtras for UnsupportedMemoryExtras {
    fn memory_status(&self) -> Option<MemoryStatus> {
        None
    }
}

pub(super) fn sources() -> PlatformSources {
    PlatformSources {
        gpu: Box::new(UnsupportedGpu),
        thermal: Box::new(UnsupportedThermal),
        fan: Box::new(UnsupportedFan),
        battery: Box::new(UnsupportedBattery),
        memory_extras: Box::new(UnsupportedMemoryExtras),
    }
}
