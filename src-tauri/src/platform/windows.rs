//! Windows 平台的采集源实现
//!
//! 采样逻辑全在原模块里（gpu / thermal / fan / win32），本文件只做薄适配：
//! 让现有探针满足 [`super`] 的 trait，并把电池 / 内存附加这两个「函数式」通道
//! 包成源对象。Windows 构建的行为与抽象化之前完全一致。

use super::{
    BatterySource, FanReport, FanSource, GpuSource, MemoryExtras, MemoryStatus,
    PlatformSources, ThermalSource,
};
use crate::battery;
use crate::fan_pref::FanPrefStore;
use crate::model::{BatteryInfo, GpuDevice, ThermalInfo};
use crate::win32;

impl GpuSource for crate::gpu::GpuProbe {
    fn sample(&mut self) -> (Vec<GpuDevice>, String) {
        // 转发到同名固有方法（路径解析优先固有方法，不会递归回 trait 实现）
        crate::gpu::GpuProbe::sample(self)
    }
}

impl ThermalSource for crate::thermal::ThermalProbe {
    fn sample(&mut self) -> ThermalInfo {
        crate::thermal::ThermalProbe::sample(self)
    }
}

impl FanSource for crate::fan::FanProbe {
    fn set_pref(&mut self, pref: FanPrefStore) {
        crate::fan::FanProbe::set_pref(self, pref)
    }

    fn sample(&mut self, gpu_count: usize) -> FanReport {
        crate::fan::FanProbe::sample(self, gpu_count)
    }
}

/// 电池：Win32 原始读数 → 公共层 `battery::from_raw` 加工
/// （百分比回退、单位换算、状态文案全部复用公共实现）
struct WinBattery;

impl BatterySource for WinBattery {
    fn collect(&mut self) -> BatteryInfo {
        battery::from_raw(win32::battery(), win32::battery_percent())
    }
}

/// 内存附加指标：GlobalMemoryStatusEx（提交量 / dwMemoryLoad）
struct WinMemoryExtras;

impl MemoryExtras for WinMemoryExtras {
    fn memory_status(&self) -> Option<MemoryStatus> {
        win32::memory_status()
    }
}

pub(super) fn sources() -> PlatformSources {
    PlatformSources {
        gpu: Box::new(crate::gpu::GpuProbe::new()),
        thermal: Box::new(crate::thermal::ThermalProbe::new()),
        fan: Box::new(crate::fan::FanProbe::new()),
        battery: Box::new(WinBattery),
        memory_extras: Box::new(WinMemoryExtras),
    }
}
