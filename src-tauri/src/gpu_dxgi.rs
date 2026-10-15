//! DXGI 适配器枚举 —— GPU 兜底通道（`gpu.rs`）的「适配器身份」来源
//!
//! PDH 的 GPU 计数器（`GPU Engine` / `GPU Adapter Memory`）对 Intel 核显、AMD
//! 都有效，但实例名里只有 **LUID**（形如 `luid_0x00000000_0x00014E56_phys_0`）——
//! 没有名字、没有显存总量。DXGI 枚举的是同一批适配器（同一个 Windows 图形栈），
//! 自带 LUID 与厂商 / 设备号 / 专用显存 / 共享内存上限，所以：
//!
//! * **按 LUID 精确对上** PDH 的读数。早先的版本按「PDH 实例顺序 == WMI 显卡
//!   列表顺序」猜名字（`Win32_VideoController` 换个顺序就张冠李戴），已废弃；
//! * 显存总量取 `DedicatedVideoMemory`（核显另见 `shared_system_memory`）；
//! * `Flags` 里的 `DXGI_ADAPTER_FLAG_SOFTWARE` 用来剔除微软基础渲染驱动
//!   （WARP）—— 它不是真显卡，任务管理器也不列它。
//!
//! 只做枚举，不做采样：调用方（`gpu.rs`）缓存一次结果即可，适配器型号与显存
//! 总量不会变（外接显卡坞热插拔属罕见场景，新适配器走「无身份」的回退分支）。

use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE,
};

/// 各家的 PCI 厂商号。目前只有 NVIDIA 那个参与逻辑（覆盖判定），
/// AMD / Intel 两个用于阅读与单测对照（诊断日志里按十六进制打印）。
pub const VENDOR_NVIDIA: u32 = 0x10DE;
#[allow(dead_code)]
pub const VENDOR_AMD: u32 = 0x1002;
#[allow(dead_code)]
pub const VENDOR_INTEL: u32 = 0x8086;

/// 一块显示适配器的静态身份
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdapterInfo {
    /// 适配器 LUID，打包成 u64（高 32 位在前，与 PDH 实例名里的两个十六进制数同序）
    pub luid: u64,
    pub name: String,
    pub vendor_id: u32,
    pub device_id: u32,
    /// 专用显存总量（byte）
    pub dedicated_video_memory: u64,
    /// 可借用的系统内存上限（byte）——核显的显存几乎全在这里
    pub shared_system_memory: u64,
    /// 微软基础渲染驱动（WARP）之类的软件适配器，不是真显卡
    pub is_software: bool,
}

impl AdapterInfo {
    /// 是不是 NVIDIA 的卡（决定「NVML 是否已经报过它」的覆盖判定）
    pub fn is_nvidia(&self) -> bool {
        self.vendor_id == VENDOR_NVIDIA
    }

    /// 是不是集成显卡（核显）。DXGI 没有官方标志位，只能用「专用显存很小、
    /// 主要靠共享内存」这个经验判据 —— 核显的专用段是固定的小 carve-out
    /// （Intel 实测 128 MiB），而独显的专用显存在 GB 量级。
    /// 判错的代价只是显存口径变成「专用 + 共享」（见 `gpu.rs` 的说明），不影响其它指标。
    pub fn is_integrated(&self) -> bool {
        const ONE_GIB: u64 = 1024 * 1024 * 1024;
        self.dedicated_video_memory < ONE_GIB
            && self.shared_system_memory > self.dedicated_video_memory.saturating_mul(4)
    }
}

/// 枚举本机全部显示适配器（失败返回空表，调用方退回「无身份」的标签）
pub fn enumerate() -> Vec<AdapterInfo> {
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    // EnumAdapters1 在越界时返回 DXGI_ERROR_NOT_FOUND，即结束条件；
    // 上限只是防御性的（正常机器不会超过几块）
    for i in 0..16u32 {
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(i) }) else {
            break;
        };
        let Ok(desc) = (unsafe { adapter.GetDesc1() }) else {
            continue;
        };
        out.push(AdapterInfo {
            luid: pack_luid(desc.AdapterLuid.LowPart, desc.AdapterLuid.HighPart),
            name: decode_name(&desc.Description),
            vendor_id: desc.VendorId,
            device_id: desc.DeviceId,
            dedicated_video_memory: desc.DedicatedVideoMemory as u64,
            shared_system_memory: desc.SharedSystemMemory as u64,
            is_software: desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0,
        });
    }
    out
}

/// LUID 打包成 u64：高 32 位放 HighPart，与 PDH 实例名里
/// `luid_0x<HighPart>_0x<LowPart>` 的书写顺序一致。
pub fn pack_luid(low: u32, high: i32) -> u64 {
    ((high as u32 as u64) << 32) | low as u64
}

/// 定长 UTF-16 缓冲（NUL 结尾）→ String
fn decode_name(utf16: &[u16]) -> String {
    let end = utf16.iter().position(|c| *c == 0).unwrap_or(utf16.len());
    String::from_utf16_lossy(&utf16[..end]).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn luid_packs_high_first() {
        // PDH 实例名 luid_0x00000000_0x00014E56 → u64 0x0000_0000_0001_4E56
        assert_eq!(pack_luid(0x0001_4E56, 0), 0x0000_0000_0001_4E56);
        // 高位不为 0 的情形（多适配器 / 部分虚拟机）同样按 高 | 低 排列
        assert_eq!(pack_luid(0x0000_0001, 2), 0x0000_0002_0000_0001);
    }

    #[test]
    fn name_decodes_until_nul() {
        let mut buf = [0u16; 128];
        for (i, c) in "Intel(R) UHD Graphics".encode_utf16().enumerate() {
            buf[i] = c;
        }
        assert_eq!(decode_name(&buf), "Intel(R) UHD Graphics");
        assert_eq!(decode_name(&[0u16; 128]), "");
    }

    #[test]
    fn integrated_heuristic_separates_igpu_from_dgpu() {
        let igpu = AdapterInfo {
            luid: 0,
            name: "Intel(R) UHD Graphics".to_string(),
            vendor_id: VENDOR_INTEL,
            device_id: 0x9A60,
            dedicated_video_memory: 128 * 1024 * 1024,
            shared_system_memory: 8 * 1024 * 1024 * 1024,
            is_software: false,
        };
        let dgpu = AdapterInfo {
            name: "NVIDIA GeForce RTX 3060 Laptop GPU".to_string(),
            vendor_id: VENDOR_NVIDIA,
            dedicated_video_memory: 6 * 1024 * 1024 * 1024,
            shared_system_memory: 8 * 1024 * 1024 * 1024,
            ..igpu.clone()
        };
        let apu = AdapterInfo {
            name: "AMD Radeon(TM) Graphics".to_string(),
            vendor_id: VENDOR_AMD,
            dedicated_video_memory: 512 * 1024 * 1024,
            shared_system_memory: 8 * 1024 * 1024 * 1024,
            ..igpu.clone()
        };
        assert!(igpu.is_integrated(), "专用 128MiB + 共享 8GiB = 核显");
        assert!(apu.is_integrated(), "AMD APU 的 512MiB carve-out 也算核显");
        assert!(!dgpu.is_integrated(), "6GiB 专用显存是独显");
        assert!(dgpu.is_nvidia());
    }
}
