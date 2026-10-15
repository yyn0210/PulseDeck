//! CPU 厂商识别（跨平台）
//!
//! CPU 温度的直读通道按厂商分流：**Intel 读 MSR**（`IntelMSR.bin`）、
//! **AMD 读 SMN**（`AMDFamily17.bin`，经 PCI 配置空间间接访问），见 `pawnio.rs`。
//! 选哪个模块、甚至要不要拉起提权桥接，都取决于这里的判断。
//!
//! 判断只依赖 sysinfo 的品牌串：不需要特权、也不碰任何平台 API，因此本模块
//! **在所有平台参与编译**——Linux CI 上也能跑它的解析单测（与
//! `platform/linux_parse.rs` 同样的思路，接口漂移在本机就能发现）。
//!
//! 为什么不查 CPUID 的厂商 ID（`GenuineIntel` / `AuthenticAMD`）：那需要平台相关的
//! 内联汇编或额外依赖（Windows 上还得处理 `__cpuid` intrinsic 的 cfg），而品牌串
//! 由固件原样给出、sysinfo 已经取好了，足够支撑这个判断。

/// CPU 厂商。只区分「有专门温度直读通道的两家」与「其它」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuVendor {
    Intel,
    Amd,
    /// 既不是 Intel 也不是 AMD（Apple Silicon、骁龙 X、部分虚拟机等）
    Other,
}

impl CpuVendor {
    /// 日志 / 界面用的短名
    pub fn label(self) -> &'static str {
        match self {
            CpuVendor::Intel => "Intel",
            CpuVendor::Amd => "AMD",
            CpuVendor::Other => "其它",
        }
    }

    /// 从品牌串判断厂商。
    ///
    /// 实测品牌串形如 `12th Gen Intel(R) Core(TM) i9-12900K`、
    /// `AMD Ryzen 7 5800H with Radeon Graphics`、`AMD FX(tm)-8350 Eight-Core Processor`。
    /// 品牌串是各厂商自己填的自由文本（虚拟机里尤其随意），所以判断只做小写包含匹配，
    /// 越简单越不容易被某个型号的措辞带偏。
    pub fn from_brand(brand: &str) -> Self {
        let brand = brand.to_ascii_lowercase();
        if brand.contains("amd") {
            CpuVendor::Amd
        } else if brand.contains("intel") {
            CpuVendor::Intel
        } else {
            CpuVendor::Other
        }
    }
}

/// 当前机器的 CPU 厂商。品牌串取不到时按 [`CpuVendor::Other`] 处理（= 不走直读通道）。
pub fn detect() -> CpuVendor {
    let mut sys = sysinfo::System::new();
    sys.refresh_cpu_all();
    sys.cpus()
        .first()
        .map(|cpu| CpuVendor::from_brand(cpu.brand()))
        .unwrap_or(CpuVendor::Other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brand_strings_map_to_expected_vendor() {
        // 本项目开发机实测品牌串
        assert_eq!(
            CpuVendor::from_brand("12th Gen Intel(R) Core(TM) i9-12900K"),
            CpuVendor::Intel
        );
        assert_eq!(
            CpuVendor::from_brand("Intel(R) Xeon(R) CPU E5-2670 0 @ 2.60GHz"),
            CpuVendor::Intel
        );
        // AMD 侧：Ryzen 台式 / 笔记本 / 老 FX 系列
        assert_eq!(
            CpuVendor::from_brand("AMD Ryzen 7 5800H with Radeon Graphics"),
            CpuVendor::Amd
        );
        assert_eq!(CpuVendor::from_brand("AMD Ryzen 5 5600X 6-Core Processor"), CpuVendor::Amd);
        assert_eq!(
            CpuVendor::from_brand("AMD FX(tm)-8350 Eight-Core Processor"),
            CpuVendor::Amd
        );
        // 「AMD Ryzen 7 9800X3D」这类不含 Radeon 的也要认出来
        assert_eq!(CpuVendor::from_brand("AMD Ryzen 7 9800X3D 8-Core Processor"), CpuVendor::Amd);
        // 既非 Intel 也非 AMD：不该被误判成任何一家
        assert_eq!(CpuVendor::from_brand("Apple M3 Pro"), CpuVendor::Other);
        assert_eq!(CpuVendor::from_brand("Snapdragon(R) X Elite"), CpuVendor::Other);
        assert_eq!(CpuVendor::from_brand(""), CpuVendor::Other);
        // 大小写不敏感
        assert_eq!(CpuVendor::from_brand("amd ryzen 3 2200g"), CpuVendor::Amd);
    }

    /// `detect()` 至少不能崩，且结果必须与品牌串的解析一致（本机自证）。
    #[test]
    fn detect_matches_brand_parsing() {
        let mut sys = sysinfo::System::new();
        sys.refresh_cpu_all();
        let expected = sys
            .cpus()
            .first()
            .map(|c| CpuVendor::from_brand(c.brand()))
            .unwrap_or(CpuVendor::Other);
        assert_eq!(detect(), expected);
        // x86_64 的实体机 / 主流虚拟机一定是 Intel 或 AMD；识别不出来说明解析在这台机器上失效了。
        // （ARM 上的品牌串措辞差异大，不做这条断言。）
        if cfg!(target_arch = "x86_64") {
            assert_ne!(detect(), CpuVendor::Other, "品牌串识别与本机不符");
        }
    }
}
