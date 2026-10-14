//! NVML（NVIDIA Management Library）读数探针 —— Windows 与 Linux 共用。
//!
//! `nvml-wrapper` 在两个平台上加载的是同一套 C API（Windows 的 nvml.dll /
//! Linux 的 libnvidia-ml.so），字段含义完全一致，所以这一份有状态探针直接共用：
//!
//! * Windows（`gpu.rs`）：NVML 优先，失败降级 PDH 性能计数器；
//! * Linux（`platform/linux.rs`）：NVML 优先，失败降级 amdgpu sysfs。

use crate::model::GpuDevice;

/// 有状态的 NVML 探针（初始化失败的尝试只做一次）
pub struct NvmlSource {
    /// 故意泄漏持有：NVML 内部有后台线程，进程退出时若卸载 NVML 动态库
    /// （`Nvml` 的 Drop 会释放动态库）偶发 `STATUS_ACCESS_VIOLATION`（Windows 实机复现）。
    /// 泄漏后动态库随进程正常退出由系统回收。
    nvml: Option<&'static nvml_wrapper::Nvml>,
    /// 初始化失败后不再重试，避免每拍都去 LoadLibrary / dlopen
    tried: bool,
}

impl Default for NvmlSource {
    fn default() -> Self {
        Self::new()
    }
}

impl NvmlSource {
    pub fn new() -> Self {
        Self {
            nvml: None,
            tried: false,
        }
    }

    /// 采样一次。返回 `None` = NVML 不可用（非 N 卡 / 未装驱动 / 初始化失败）；
    /// 返回 `Some(vec![])` = NVML 可用但没有设备。
    pub fn sample(&mut self) -> Option<Vec<GpuDevice>> {
        if self.nvml.is_none() {
            if self.tried {
                return None;
            }
            self.tried = true;
            // 没有 NVIDIA 驱动时这里会返回 Err（动态加载 NVML 库失败），属正常情况
            let instance = nvml_wrapper::Nvml::init().ok()?;
            self.nvml = Some(Box::leak(Box::new(instance)));
        }
        let nvml = self.nvml.as_ref()?;

        let count = nvml.device_count().ok()?;
        let mut out = Vec::new();
        for i in 0..count {
            let Ok(dev) = nvml.device_by_index(i) else {
                continue;
            };
            let name = dev.name().unwrap_or_else(|_| format!("NVIDIA GPU #{i}"));

            let usage_pct = dev
                .utilization_rates()
                .ok()
                .map(|u| clamp_pct(u.gpu as f32));

            let (mem_used, mem_total, mem_pct) = match dev.memory_info() {
                Ok(m) => {
                    let pct = if m.total > 0 {
                        Some(clamp_pct(m.used as f32 / m.total as f32 * 100.0))
                    } else {
                        None
                    };
                    (m.used, m.total, pct)
                }
                Err(_) => (0, 0, None),
            };

            let temp_c = dev
                .temperature(nvml_wrapper::enum_wrappers::device::TemperatureSensor::Gpu)
                .ok()
                .map(|t| t as f32);
            // NVML 的功耗单位是 mW
            let power_w = dev.power_usage().ok().map(|mw| mw as f32 / 1000.0);
            // fan_speed 返回的是**占空比百分比**（0-100），不是 RPM。
            // 桌面卡上通常有值；笔记本独显风扇由 EC 控制，驱动返回 N/A（这里就是 Err → None）。
            let fan_pct = dev.fan_speed(0).ok().map(|v| v as f32);
            // 个别型号/驱动能直接给 RPM；拿不到就是 None，不拿百分比硬凑成「转速」。
            let fan_rpm = dev
                .fan_speed_rpm(0)
                .ok()
                .map(|v| v as f32)
                .filter(|v| *v > 0.0);

            out.push(GpuDevice {
                index: i,
                name,
                usage_pct,
                mem_used_bytes: mem_used,
                mem_total_bytes: mem_total,
                mem_pct,
                temp_c,
                power_w,
                fan_pct,
                fan_rpm,
                source: "nvml".to_string(),
            });
        }
        Some(out)
    }
}

/// 百分比合理区间清洗（NVML 与 PDH 两条通道共用）
pub(crate) fn clamp_pct(v: f32) -> f32 {
    if !v.is_finite() {
        0.0
    } else {
        v.clamp(0.0, 100.0)
    }
}
