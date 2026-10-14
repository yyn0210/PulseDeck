//! 电池：把各平台采集源给的原始读数（[`platform::RawBattery`]）整理成前端用的 BatteryInfo
//!
//! 本模块是**纯加工层**（无任何 OS 依赖）：单位换算、功率方向分派、状态文案
//! 全部在这里，各平台的 `BatterySource` 实现只负责把原生接口的读数装进 RawBattery。

use crate::model::BatteryInfo;
use crate::platform::RawBattery;

/// 把原始电池读数加工成 BatteryInfo。
///
/// `percent_fallback`：主接口容量读数不可信时的电量百分比回退
/// （Windows 上来自 GetSystemPowerStatus，由平台采集源一并传入）。
pub fn from_raw(raw: RawBattery, percent_fallback: Option<f32>) -> BatteryInfo {
    if !raw.present {
        return BatteryInfo {
            has_battery: false,
            ac_online: raw.ac_online,
            source: raw.source.to_string(),
            status_text: if raw.ac_online {
                "未检测到电池（台式机 / UPS 供电）".to_string()
            } else {
                "未检测到电池".to_string()
            },
            ..BatteryInfo::default()
        };
    }

    let mut info = BatteryInfo {
        has_battery: true,
        ac_online: raw.ac_online,
        charging: raw.charging,
        discharging: raw.discharging,
        source: raw.source.to_string(),
        ..BatteryInfo::default()
    };

    // 容量
    if raw.max_capacity_mwh > 0 {
        info.full_charge_wh = Some(mwh_to_wh(raw.max_capacity_mwh));
        info.remaining_wh = Some(mwh_to_wh(raw.remaining_capacity_mwh));
        info.percent = Some(round1(raw.remaining_capacity_mwh as f32 * 100.0 / raw.max_capacity_mwh as f32));
    }

    // 百分比回退：主接口容量无效时用采集源给的备用百分比
    if info.percent.is_none() {
        info.percent = percent_fallback;
    }

    // 适配充电功率 / 放电功率
    if raw.rate_mw > 0 {
        let rate_w = round1(raw.rate_mw as f32 / 1000.0);
        if raw.charging {
            info.charge_rate_w = Some(rate_w);
        } else if raw.discharging {
            info.discharge_rate_w = Some(rate_w);
        }
    }

    // 预估时间（哨兵值已在 win32 层过滤）
    if raw.estimated_time_s > 0 {
        if raw.charging {
            info.seconds_to_full = Some(raw.estimated_time_s as u64);
        } else {
            info.seconds_to_empty = Some(raw.estimated_time_s as u64);
        }
    }

    info.status_text = status_text(&info);
    info
}

fn status_text(info: &BatteryInfo) -> String {
    match (info.ac_online, info.charging, info.discharging) {
        (true, true, _) => "正在充电（适配器已接入）".to_string(),
        (true, false, _) => match info.percent {
            Some(p) if p >= 99.5 => "已充满（适配器持续供电）".to_string(),
            Some(p) if p < 20.0 => "适配器已接入，但未充电（低电量保护/限充）".to_string(),
            // 多数笔记本接电后维持电量、电池不充不放，个别机型还会有极小电流的「微放电」，
            // 两者都属于「适配器在供电、但没在充电」，分开表述避免用户误以为在充电
            _ if info.discharging => "适配器已接入，电池未充电（轻微放电）".to_string(),
            _ => "适配器已接入，当前未充电".to_string(),
        },
        (false, _, true) => "电池供电中".to_string(),
        _ => "电池状态未知".to_string(),
    }
}

fn mwh_to_wh(mwh: u32) -> f32 {
    round1(mwh as f32 / 1000.0)
}

fn round1(v: f32) -> f32 {
    (v * 10.0).round() / 10.0
}
