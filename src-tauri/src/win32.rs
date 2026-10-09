//! Win32 原生接口薄封装（仅用于 sysinfo 覆盖不到的指标）
//!
//! 只有两处需要绕过 sysinfo 直接调 Win32：
//!   1. 电池：CallNtPowerInformation(SystemBatteryState) 才能拿到「充/放电功率」
//!   2. 内存提交量：GlobalMemoryStatusEx

use std::mem::{size_of, zeroed};

use windows_sys::Win32::System::Power::{
    CallNtPowerInformation, GetSystemPowerStatus, SystemBatteryState, SYSTEM_BATTERY_STATE,
    SYSTEM_POWER_STATUS,
};
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

/// NTSTATUS 成功码
const STATUS_SUCCESS: i32 = 0;

/// Windows 用全 1 表示该字段「无效/未提供」，必须在边界处拦掉，
/// 否则会被当成真实数值（例如 0xFFFFFFFF mWh 会被算成 4.29 MWh 的容量）
const U32_UNKNOWN: u32 = 0xFFFF_FFFF;

/// 笔记本充电功率的上限保护：超过 1 kW 一律视为固件误报
const MAX_PLAUSIBLE_MW: u32 = 1_000_000;

fn known(v: u32) -> u32 {
    if v == U32_UNKNOWN {
        0
    } else {
        v
    }
}

/* ------------------------------ 内存 ------------------------------ */

#[derive(Clone, Copy, Debug, Default)]
pub struct MemoryStatus {
    /// dwMemoryLoad
    pub load_pct: u32,
    pub phys_total_bytes: u64,
    pub phys_avail_bytes: u64,
    /// 提交上限 = 物理内存 + 页面文件大小
    pub commit_limit_bytes: u64,
    /// 当前已提交量
    pub commit_used_bytes: u64,
}

pub fn memory_status() -> Option<MemoryStatus> {
    unsafe {
        let mut m: MEMORYSTATUSEX = zeroed();
        m.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
        if GlobalMemoryStatusEx(&mut m) == 0 {
            return None;
        }
        Some(MemoryStatus {
            load_pct: m.dwMemoryLoad,
            phys_total_bytes: m.ullTotalPhys,
            phys_avail_bytes: m.ullAvailPhys,
            commit_limit_bytes: m.ullTotalPageFile,
            commit_used_bytes: m.ullTotalPageFile.saturating_sub(m.ullAvailPageFile),
        })
    }
}

/* ------------------------------ 电池 ------------------------------ */

#[derive(Clone, Copy, Debug, Default)]
pub struct RawBattery {
    pub present: bool,
    pub ac_online: bool,
    pub charging: bool,
    pub discharging: bool,
    /// 满充容量，单位 mWh
    pub max_capacity_mwh: u32,
    /// 剩余容量，单位 mWh
    pub remaining_capacity_mwh: u32,
    /// 充/放电功率，单位 mW（恒为正值，方向由 charging/discharging 决定）
    pub rate_mw: u32,
    /// 剩余估算时间，单位秒
    pub estimated_time_s: u32,
    pub source: &'static str,
}

/// SYSTEM_BATTERY_STATE::Rate 声明为 DWORD，实机上却按**有符号**返回：
///   正值 = 充电功率(mW)，负值 = 放电功率(mW)，0 / ±1 = 固件未提供。
/// 这里先按符号解出方向与大小，解不出来才退回 Charging/Discharging 标志位。
fn decode_rate(signed: i32, flag_charging: bool, flag_discharging: bool) -> (bool, bool, u32) {
    match signed {
        v if v >= 2 => {
            let mw = v as u32;
            (true, false, if mw > MAX_PLAUSIBLE_MW { 0 } else { mw })
        }
        v if v <= -2 => {
            let mw = v.unsigned_abs();
            (false, true, if mw > MAX_PLAUSIBLE_MW { 0 } else { mw })
        }
        _ => (flag_charging, flag_discharging, 0),
    }
}

/// 读取电池状态：优先 CallNtPowerInformation(SystemBatteryState)，
/// 失败或无电池时回退到 GetSystemPowerStatus。
pub fn battery() -> RawBattery {
    if let Some(raw) = battery_state_precise() {
        return raw;
    }
    battery_status()
}

/// SYSTEM_BATTERY_STATE：唯一能公开拿到 ChargeRate / DischargeRate 的接口
fn battery_state_precise() -> Option<RawBattery> {
    unsafe {
        let mut st: SYSTEM_BATTERY_STATE = zeroed();
        let ret = CallNtPowerInformation(
            SystemBatteryState,
            std::ptr::null(),
            0,
            &mut st as *mut _ as *mut core::ffi::c_void,
            size_of::<SYSTEM_BATTERY_STATE>() as u32,
        );
        if ret != STATUS_SUCCESS || st.BatteryPresent == 0 {
            return None;
        }
        let (charging, discharging, rate_mw) =
            decode_rate(st.Rate as i32, st.Charging != 0, st.Discharging != 0);
        Some(RawBattery {
            present: true,
            ac_online: st.AcOnLine != 0,
            charging,
            discharging,
            max_capacity_mwh: known(st.MaxCapacity),
            remaining_capacity_mwh: known(st.RemainingCapacity),
            rate_mw,
            estimated_time_s: known(st.EstimatedTime),
            source: "CallNtPowerInformation(SYSTEM_BATTERY_STATE)",
        })
    }
}

fn battery_status() -> RawBattery {
    unsafe {
        let mut s: SYSTEM_POWER_STATUS = zeroed();
        if GetSystemPowerStatus(&mut s) == 0 {
            return RawBattery {
                source: "unavailable",
                ..RawBattery::default()
            };
        }
        // BatteryFlag bit7 = 无电池；0xff = 未知状态
        let present = s.BatteryFlag & 0x80 == 0 && s.BatteryFlag != 0xff;
        let ac_online = s.ACLineStatus == 1;
        RawBattery {
            present,
            ac_online,
            charging: present && ac_online,
            discharging: present && s.ACLineStatus == 0,
            source: "GetSystemPowerStatus",
            ..RawBattery::default()
        }
    }
}

/// 只取电量百分比：SYSTEM_BATTERY_STATE 的容量读数不可信时用它补充
pub fn battery_percent() -> Option<f32> {
    unsafe {
        let mut s: SYSTEM_POWER_STATUS = zeroed();
        if GetSystemPowerStatus(&mut s) == 0 || s.BatteryLifePercent == 0xff {
            return None;
        }
        Some(s.BatteryLifePercent as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_is_signed() {
        // 实机观测：-840 mW 表示电池在微放电
        assert_eq!(decode_rate(-840, true, false), (false, true, 840));
        assert_eq!(decode_rate(15_000, false, false), (true, false, 15_000));
        // 0 / ±1 视为固件未提供，退回标志位
        assert_eq!(decode_rate(-1, false, true), (false, true, 0));
        assert_eq!(decode_rate(0, true, false), (true, false, 0));
        // 荒谬值兜底
        assert_eq!(decode_rate(-2_000_000_000, false, true), (false, true, 0));
    }
}
