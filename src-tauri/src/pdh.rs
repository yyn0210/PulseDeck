//! PDH（Performance Data Helper）轻量封装
//!
//! 用途：在没有厂商 SDK 的情况下读取 Windows 内建性能计数器。
//! 句柄长期持有（counter 是速率型，需要连续两次 Collect 才有值），不要每次采样重建。

use std::mem;

use windows::core::PCWSTR;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Performance::{
    PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
};

/// 查询句柄包装：Drop 时自动 PdhCloseQuery
pub struct Query {
    h: PDH_HQUERY,
}

unsafe impl Send for Query {}

impl Query {
    pub fn new() -> Option<Self> {
        let mut h = PDH_HQUERY::default();
        let rc = unsafe { windows::Win32::System::Performance::PdhOpenQueryW(None, 0, &mut h) };
        if rc != ERROR_SUCCESS.0 {
            return None;
        }
        Some(Self { h })
    }

    /// 添加计数器。路径用**英文**名（PdhAddEnglishCounterW），避免中文系统上 localized 路径失效。
    /// 支持通配符：例如 `\GPU Engine(*)\Utilization Percentage`
    pub fn add(&self, path: &str) -> Option<PDH_HCOUNTER> {
        let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
        let mut hc = PDH_HCOUNTER::default();
        let rc = unsafe {
            windows::Win32::System::Performance::PdhAddEnglishCounterW(
                self.h,
                PCWSTR(wide.as_ptr()),
                0,
                &mut hc,
            )
        };
        if rc != ERROR_SUCCESS.0 {
            return None;
        }
        Some(hc)
    }

    /// 抓一次快照。速率型 counter 至少需要调用两次（间隔 > 采样周期）
    pub fn collect(&self) -> bool {
        let rc = unsafe { windows::Win32::System::Performance::PdhCollectQueryData(self.h) };
        rc == ERROR_SUCCESS.0
    }

    /// 读取该计数器下所有实例的值；返回 (实例名, double 值) 列表
    pub fn double_array(&self, counter: PDH_HCOUNTER) -> Vec<(String, f64)> {
        let mut needed: u32 = 0;
        let mut count: u32 = 0;
        // 第一次调用拿所需缓冲区大小
        let rc = unsafe {
            windows::Win32::System::Performance::PdhGetFormattedCounterArrayW(
                counter,
                PDH_FMT_DOUBLE,
                &mut needed,
                &mut count,
                None,
            )
        };
        if rc != PDH_MORE_DATA && rc != ERROR_SUCCESS.0 {
            return Vec::new();
        }

        let unit = mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
        if (needed as usize) < unit {
            return Vec::new();
        }

        // 多试几次：两次调用之间实例数可能变化（GPU Engine 每进程一个实例、
        // 随进程启停实时增减），PDH 会返回 PDH_MORE_DATA 并给出新的所需大小。
        for _ in 0..3 {
            // 向上取整 + 留冗余，避免 PDH 写出我们没分配的空间（它会按]。
            // lpdwbuffersize 的上限写，所以这里必须回传 buf 的真实字节数）
            let cap_items = (needed as usize).div_ceil(unit) + 8;
            // 上限保护：单个对象的数据超过 8 MB 就放弃，避免异常数据吃内存
            if cap_items * unit > 8 * 1024 * 1024 {
                return Vec::new();
            }
            let mut buf: Vec<PDH_FMT_COUNTERVALUE_ITEM_W> =
                vec![PDH_FMT_COUNTERVALUE_ITEM_W::default(); cap_items];
            let mut buf_bytes = (cap_items * unit) as u32;

            let rc = unsafe {
                windows::Win32::System::Performance::PdhGetFormattedCounterArrayW(
                    counter,
                    PDH_FMT_DOUBLE,
                    &mut buf_bytes,
                    &mut count,
                    Some(buf.as_mut_ptr()),
                )
            };
            if rc == PDH_MORE_DATA {
                needed = buf_bytes;
                continue;
            }
            if rc != ERROR_SUCCESS.0 {
                return Vec::new();
            }

            let mut out = Vec::with_capacity(buf.len().min(count as usize));
            for item in buf.iter().take(count as usize) {
                if item.FmtValue.CStatus != ERROR_SUCCESS.0 {
                    continue;
                }
                let name = unsafe { item.szName.to_string() }.unwrap_or_default();
                // union 里此刻存的是 doubleValue（PDH_FMT_DOUBLE）
                let value = unsafe { item.FmtValue.Anonymous.doubleValue };
                if value.is_finite() {
                    out.push((name, value));
                }
            }
            return out;
        }
        Vec::new()
    }
}

impl Drop for Query {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::System::Performance::PdhCloseQuery(self.h);
        }
    }
}
