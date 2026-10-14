//! Linux 采集的解析与选择逻辑（纯函数，零 IO、零平台依赖）
//!
//! 与 `linux.rs`（扫描 / trait 实现）拆开的原因：本模块**在所有平台都参与编译**
//! （同 `unsupported.rs` 的理由）——本仓库在 Windows 上开发，这样 `cargo test`
//! 就能直接验证 Linux 的解析 / 选择逻辑（fixture 驱动），而不是等 CI 才知道算错。
//!
//! 数据流：`linux.rs` 把 sysfs / procfs 的原始字符串装进这里的 `*Raw` / `HwmonChip`
//! 结构，本模块负责一切换算与选择。

use crate::model::GpuDevice;

/* ------------------------------ 数值区间 ------------------------------ */

/// 温度合理区间（与 Windows 侧 thermal.rs 的口径一致）
pub fn plausible_temp(c: f32) -> Option<f32> {
    (1.0..=120.0).contains(&c).then_some(c)
}

/// 风扇转速合理区间（0 = 传感器没在转/不可信；上限参照 Windows 侧 12000）
pub fn plausible_rpm(rpm: f32) -> Option<f32> {
    (rpm > 0.0 && rpm < 12_000.0).then_some(rpm)
}

/* ------------------------------ hwmon ------------------------------ */

/// 一个 hwmon 芯片的原始读数
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HwmonChip {
    pub name: String,
    /// (label, °C)，label 缺失时为空串
    pub temps: Vec<(String, f32)>,
    /// fanN_input（RPM），按索引顺序
    pub fans: Vec<f32>,
    /// pwmN（0-255 原始值），按索引顺序
    pub pwms: Vec<f32>,
}

/// 判定芯片是否「CPU 温度」的候选（数字越小优先级越高）
fn cpu_chip_rank(name: &str) -> Option<u8> {
    match name {
        "coretemp" => Some(0),
        "k10temp" | "zenpower" | "k8temp" => Some(1),
        "cpu_thermal" => Some(2),
        _ => None,
    }
}

/// 判定芯片是否 GPU 温度/风扇的来源
pub fn is_gpu_chip(name: &str) -> bool {
    matches!(name, "amdgpu" | "nouveau" | "radeon" | "i915" | "xe" | "nvidia")
}

/// 从 hwmon 芯片里挑 CPU 温度。
///
/// coretemp 优先取 "Package id 0"（封装温度），否则取该芯片最高温；
/// k10temp / zenpower 取 Tctl（标签匹配，否则取最高温）；cpu_thermal 取最高温。
pub fn pick_cpu_temp(chips: &[HwmonChip]) -> Option<(f32, String)> {
    let mut candidates: Vec<&HwmonChip> = chips
        .iter()
        .filter(|c| cpu_chip_rank(&c.name).is_some() && !c.temps.is_empty())
        .collect();
    candidates.sort_by_key(|c| cpu_chip_rank(&c.name).unwrap_or(9));
    let chip = *candidates.first()?;

    let pick = chip
        .temps
        .iter()
        .find(|(label, _)| label == "Package id 0")
        .or_else(|| {
            chip.temps
                .iter()
                .find(|(label, _)| label.starts_with("Tctl") || label.starts_with("Tccd1"))
        })
        .or_else(|| chip.temps.iter().max_by(|a, b| a.1.total_cmp(&b.1)));
    let (label, temp) = pick?;
    let source = if label.is_empty() {
        format!("hwmon:{}", chip.name)
    } else {
        format!("hwmon:{} ({label})", chip.name)
    };
    Some((*temp, source))
}

/// 从 hwmon 芯片里挑 GPU 温度（取第一块 GPU 芯片的最高温）
pub fn pick_gpu_temp(chips: &[HwmonChip]) -> Option<(f32, String)> {
    let chip = chips
        .iter()
        .find(|c| is_gpu_chip(&c.name) && !c.temps.is_empty())?;
    let (_, temp) = chip.temps.iter().max_by(|a, b| a.1.total_cmp(&b.1))?;
    Some((*temp, format!("hwmon:{}", chip.name)))
}

/// 主板/EC 侧的风扇芯片（CPU 风扇候选），按优先级
fn mobo_chip_rank(name: &str) -> Option<u8> {
    if name == "thinkpad" || name == "asus" || name == "dell_smm" {
        return Some(0); // 笔记本 EC
    }
    if name.starts_with("nct") || name.starts_with("it87") || name.starts_with("f71") {
        return Some(1); // 台式机 Super I/O
    }
    if name.starts_with("acpi") || name.starts_with("pwm") {
        return Some(2);
    }
    if is_gpu_chip(name) {
        return None; // 显卡风扇归 GPU 侧
    }
    Some(3) // 其余芯片（含未知）兜底尝试
}

/// 从 hwmon 芯片挑 CPU 风扇：按 [`mobo_chip_rank`] 优先级取第一个带风扇的芯片
pub fn pick_cpu_fan_chip(chips: &[HwmonChip]) -> Option<&HwmonChip> {
    let mut candidates: Vec<(u8, &HwmonChip)> = chips
        .iter()
        .filter(|c| c.fans.iter().any(|f| plausible_rpm(*f).is_some()))
        .filter_map(|c| mobo_chip_rank(&c.name).map(|rank| (rank, c)))
        .collect();
    candidates.sort_by_key(|(rank, _)| *rank);
    candidates.first().map(|(_, c)| *c)
}

/// 一次风扇选择结果（由 `linux.rs` 转换成 platform 层的 `FanReading`）
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FanPick {
    pub rpm: Option<f32>,
    pub pct: Option<f32>,
    pub source: String,
}

impl FanPick {
    pub fn any(&self) -> bool {
        self.rpm.is_some() || self.pct.is_some()
    }
}

/// 单芯片 → 风扇读数：RPM 优先（取可信的最大值）；没有 RPM 时退回第一路 pwm 占空比
pub fn chip_reading(chip: &HwmonChip) -> FanPick {
    let rpm = chip
        .fans
        .iter()
        .filter_map(|f| plausible_rpm(*f))
        .max_by(|a, b| a.total_cmp(b));
    if let Some(rpm) = rpm {
        return FanPick {
            rpm: Some(rpm),
            pct: None,
            source: format!("hwmon:{}", chip.name),
        };
    }
    let pct = chip.pwms.first().map(|v| (v / 255.0 * 100.0).clamp(0.0, 100.0));
    FanPick {
        rpm: None,
        pct,
        source: if pct.is_some() {
            format!("hwmon:{} (PWM)", chip.name)
        } else {
            String::new()
        },
    }
}

/// 各 GPU 芯片的风扇读数（按 hwmon 顺序，与 GPU 设备顺序对齐）
pub fn gpu_fan_readings(chips: &[HwmonChip], gpu_count: usize) -> Vec<FanPick> {
    let gpu_chips: Vec<&HwmonChip> = chips
        .iter()
        .filter(|c| is_gpu_chip(&c.name) && (!c.fans.is_empty() || !c.pwms.is_empty()))
        .collect();
    let mut out = vec![FanPick::default(); gpu_count];
    for (i, r) in out.iter_mut().enumerate() {
        if let Some(chip) = gpu_chips.get(i) {
            *r = chip_reading(chip);
        }
    }
    out
}

/* ------------------------------ RAPL 功耗 ------------------------------ */

/// 能量差分 → 功率（W）。计数回绕（新值 < 旧值）时按量程补偿；
/// 拿不到量程就本次放弃（返回 None，下拍重新建立基线）。
pub fn energy_delta_w(
    prev_uj: u64,
    cur_uj: u64,
    max_range_uj: Option<u64>,
    elapsed_s: f64,
) -> Option<f64> {
    if elapsed_s <= 0.0 {
        return None;
    }
    let delta = if cur_uj >= prev_uj {
        cur_uj - prev_uj
    } else {
        max_range_uj? - prev_uj + cur_uj
    };
    Some(delta as f64 / 1_000_000.0 / elapsed_s)
}

/* ------------------------------ amdgpu ------------------------------ */

/// amdgpu sysfs 的一张卡的原始读数
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AmdCardRaw {
    pub name: String,
    pub busy_pct: Option<f32>,
    pub vram_used: Option<u64>,
    pub vram_total: Option<u64>,
    pub temp_c: Option<f32>,
    pub power_uw: Option<f64>,
    pub fan_rpm: Option<f32>,
    pub fan_pwm: Option<f32>,
}

/// 由原始读数组装 GpuDevice；什么都没读到（无 busy%/VRAM/温度）时返回 None
pub fn amd_device(index: u32, card: &AmdCardRaw) -> Option<GpuDevice> {
    let has_any = card.busy_pct.is_some() || card.vram_total.is_some() || card.temp_c.is_some();
    if !has_any {
        return None;
    }
    let (mem_used, mem_total, mem_pct) = (
        card.vram_used.unwrap_or(0),
        card.vram_total.unwrap_or(0),
        match (card.vram_used, card.vram_total) {
            (Some(u), Some(t)) if t > 0 => Some((u as f32 / t as f32 * 100.0).clamp(0.0, 100.0)),
            _ => None,
        },
    );
    Some(GpuDevice {
        index,
        name: card.name.clone(),
        usage_pct: card.busy_pct.map(|v| v.clamp(0.0, 100.0)),
        mem_used_bytes: mem_used,
        mem_total_bytes: mem_total,
        mem_pct,
        temp_c: card.temp_c,
        power_w: card.power_uw.map(|uw| (uw / 1_000_000.0) as f32),
        fan_pct: card.fan_pwm.map(|v| (v / 255.0 * 100.0).clamp(0.0, 100.0)),
        fan_rpm: card.fan_rpm.and_then(plausible_rpm),
        source: "amdgpu".to_string(),
    })
}

/* ------------------------------ 电池 ------------------------------ */

/// 一块电池的 sysfs 原始读数（µAh / µWh 两种量纲都保留，由换算层决定用哪套）
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BatteryRaw {
    pub status: String,
    pub ac_online: bool,
    /// 充电域：µAh
    pub charge_now_uah: Option<u64>,
    pub charge_full_uah: Option<u64>,
    /// 能量域：µWh
    pub energy_now_uwh: Option<u64>,
    pub energy_full_uwh: Option<u64>,
    /// µA（充电域）
    pub current_now_ua: Option<u64>,
    /// µV
    pub voltage_now_uv: Option<u64>,
    /// µW（有则最可信）
    pub power_now_uw: Option<u64>,
    /// 电量百分比（capacity 文件）
    pub capacity_pct: Option<f32>,
}

/// 换算结果（由 `linux.rs` 装进 `platform::RawBattery`）
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BatteryConversion {
    pub charging: bool,
    pub discharging: bool,
    pub max_capacity_mwh: Option<u64>,
    pub remaining_capacity_mwh: Option<u64>,
    /// mW；超过 1 kW 的误报已被过滤为 None
    pub rate_mw: Option<u32>,
}

/// 把 sysfs 读数换算成统一量纲（mWh / mW）。
///
/// 两个量纲的处理规则（内核文档规定）：
/// * 有 `energy_*` 的电池按 µWh 报，功率优先 `power_now`（µW）；
/// * 只有 `charge_*` 的电池按 µAh 报，容量与功率都要乘 `voltage_now`（µV）；
///   拿不到电压就放弃（不猜量纲），宁可显示 `--`。
pub fn battery_convert(b: &BatteryRaw) -> BatteryConversion {
    let (max_mwh, now_mwh) = if let (Some(f), Some(n)) = (b.energy_full_uwh, b.energy_now_uwh) {
        (Some(f / 1000), Some(n / 1000))
    } else if let (Some(f), Some(n), Some(v)) =
        (b.charge_full_uah, b.charge_now_uah, b.voltage_now_uv)
    {
        // µAh × µV / 1e9 = mWh
        (
            Some(f.saturating_mul(v) / 1_000_000_000),
            Some(n.saturating_mul(v) / 1_000_000_000),
        )
    } else {
        (None, None)
    };

    // 功率（mW）：power_now(µW) 优先；否则 current_now(µA) × voltage(µV) / 1e9 = mW
    let rate_mw_raw: Option<u32> = if let Some(uw) = b.power_now_uw {
        Some((uw / 1000) as u32)
    } else if let (Some(i), Some(v)) = (b.current_now_ua, b.voltage_now_uv) {
        Some((i.saturating_mul(v) / 1_000_000_000) as u32)
    } else {
        None
    };

    BatteryConversion {
        charging: b.status == "Charging",
        discharging: b.status == "Discharging",
        max_capacity_mwh: max_mwh,
        remaining_capacity_mwh: now_mwh,
        // 上限保护：笔记本功耗超过 1 kW 一律视为量纲误报（与 win32 侧同样的口径）
        rate_mw: rate_mw_raw.filter(|mw| *mw <= 1_000_000),
    }
}

/* ------------------------------ meminfo ------------------------------ */

/// `/proc/meminfo` 的解析结果（由 `linux.rs` 转成 `platform::MemoryStatus`）
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MeminfoParsed {
    pub total_kb: u64,
    pub avail_kb: u64,
    pub committed_kb: Option<u64>,
    pub commit_limit_kb: Option<u64>,
    pub load_pct: u32,
}

/// 解析 `/proc/meminfo` 文本。MemTotal 缺失视为不可用（返回 None）。
///
/// Committed_AS / CommitLimit 对应 Windows 的「已提交内存 / 提交上限」——
/// 同一个概念在 Linux 上的名字，也是 sysinfo 覆盖不到的部分。
pub fn parse_meminfo(text: &str) -> Option<MeminfoParsed> {
    let mut total_kb = None;
    let mut avail_kb = None;
    let mut committed_kb = None;
    let mut limit_kb = None;
    for line in text.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let value: Option<u64> = rest.split_whitespace().next().and_then(|v| v.parse().ok());
        match key {
            "MemTotal" => total_kb = value,
            "MemAvailable" => avail_kb = value,
            "Committed_AS" => committed_kb = value,
            "CommitLimit" => limit_kb = value,
            _ => {}
        }
    }
    let total = total_kb?;
    let avail = avail_kb.unwrap_or(0);
    let load_pct = if total > 0 {
        ((total.saturating_sub(avail)) as f64 / total as f64 * 100.0).round() as u32
    } else {
        0
    };
    Some(MeminfoParsed {
        total_kb: total,
        avail_kb: avail,
        committed_kb,
        commit_limit_kb: limit_kb,
        load_pct,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /* ---------- hwmon 选择 ---------- */

    fn chip(name: &str, temps: &[(&str, f32)], fans: &[f32]) -> HwmonChip {
        HwmonChip {
            name: name.to_string(),
            temps: temps.iter().map(|(l, t)| (l.to_string(), *t)).collect(),
            fans: fans.to_vec(),
            pwms: Vec::new(),
        }
    }

    #[test]
    fn cpu_temp_prefers_coretemp_package() {
        let chips = vec![
            chip(
                "coretemp",
                &[("Core 0", 60.0), ("Package id 0", 65.0), ("Core 1", 62.0)],
                &[],
            ),
            chip("nvme", &[("Composite", 40.0)], &[]),
        ];
        let (t, source) = pick_cpu_temp(&chips).expect("coretemp 应被选中");
        assert_eq!(t, 65.0, "优先取 Package id 0");
        assert!(source.contains("coretemp"));
    }

    #[test]
    fn cpu_temp_k10temp_tctl() {
        let chips = vec![chip("k10temp", &[("Tctl", 55.5), ("Tccd1", 50.0)], &[])];
        let (t, source) = pick_cpu_temp(&chips).expect("k10temp 应被选中");
        assert_eq!(t, 55.5);
        assert!(source.contains("Tctl"));
    }

    #[test]
    fn cpu_temp_none_when_no_known_chip() {
        let chips = vec![chip("nvme", &[("Composite", 40.0)], &[])];
        assert!(pick_cpu_temp(&chips).is_none(), "nvme 温度不能冒充 CPU 温度");
    }

    #[test]
    fn gpu_temp_from_gpu_chip() {
        let chips = vec![
            chip("coretemp", &[("Package id 0", 60.0)], &[]),
            chip("amdgpu", &[("edge", 48.0)], &[]),
        ];
        let (t, source) = pick_gpu_temp(&chips).expect("amdgpu 应被选中");
        assert_eq!(t, 48.0);
        assert!(source.contains("amdgpu"));
    }

    #[test]
    fn cpu_fan_skips_gpu_chips_and_prefers_ec() {
        let gpu = chip("amdgpu", &[], &[1200.0]);
        let ec = chip("thinkpad", &[], &[3000.0]);
        let superio = chip("nct6775", &[], &[1500.0]);
        let chips = [gpu, superio, ec];
        let pick = pick_cpu_fan_chip(&chips).expect("应有 CPU 风扇候选");
        assert_eq!(pick.name, "thinkpad", "EC 芯片优先于 Super I/O，且不取显卡芯片");
    }

    #[test]
    fn chip_reading_prefers_rpm_over_pwm() {
        let mut c = chip("nct6775", &[], &[1800.0]);
        c.pwms = vec![128.0];
        let r = chip_reading(&c);
        assert_eq!(r.rpm, Some(1800.0));
        assert_eq!(r.pct, None);
    }

    #[test]
    fn chip_reading_falls_back_to_pwm_pct() {
        let mut c = chip("nct6775", &[], &[0.0]);
        c.pwms = vec![127.5];
        let r = chip_reading(&c);
        assert_eq!(r.rpm, None);
        assert_eq!(r.pct, Some(50.0));
    }

    /* ---------- RAPL 差分 ---------- */

    #[test]
    fn rapl_delta_basic_and_wrap() {
        // 2 秒涨 20 J = 10 W
        assert_eq!(energy_delta_w(1_000_000, 21_000_000, None, 2.0), Some(10.0));
        // 回绕：max=100，prev=90，cur=15 → delta=25
        assert_eq!(energy_delta_w(90, 15, Some(100), 1.0), Some(25e-6));
        // 回绕但不知道量程 → 本次放弃
        assert_eq!(energy_delta_w(90, 15, None, 1.0), None);
    }

    /* ---------- amdgpu ---------- */

    #[test]
    fn amd_device_maps_all_fields() {
        let raw = AmdCardRaw {
            name: "Radeon RX 6800".to_string(),
            busy_pct: Some(37.0),
            vram_used: Some(2_000_000_000),
            vram_total: Some(16_000_000_000),
            temp_c: Some(55.0),
            power_uw: Some(120_000_000.0),
            fan_rpm: Some(1500.0),
            fan_pwm: Some(128.0),
        };
        let dev = amd_device(0, &raw).expect("有 busy% 就应产出设备");
        assert_eq!(dev.usage_pct, Some(37.0));
        assert_eq!(dev.mem_pct, Some(12.5));
        assert_eq!(dev.power_w, Some(120.0));
        assert!((dev.fan_pct.unwrap() - 50.196).abs() < 0.01, "128/255 ≈ 50.2%");
        assert_eq!(dev.source, "amdgpu");
    }

    #[test]
    fn amd_device_absent_when_no_readings() {
        let raw = AmdCardRaw {
            name: "AMD GPU card0".to_string(),
            ..AmdCardRaw::default()
        };
        assert!(amd_device(0, &raw).is_none(), "什么都读不到就不该报设备");
    }

    /* ---------- 电池 ---------- */

    #[test]
    fn battery_energy_domain_uses_power_now() {
        let b = BatteryRaw {
            status: "Discharging".to_string(),
            energy_now_uwh: Some(30_000_000),  // 30 Wh
            energy_full_uwh: Some(50_000_000), // 50 Wh
            power_now_uw: Some(12_000_000),    // 12 W
            capacity_pct: Some(60.0),
            ..BatteryRaw::default()
        };
        let c = battery_convert(&b);
        assert!(c.discharging && !c.charging);
        assert_eq!(c.remaining_capacity_mwh, Some(30_000));
        assert_eq!(c.max_capacity_mwh, Some(50_000));
        assert_eq!(c.rate_mw, Some(12_000));
    }

    #[test]
    fn battery_charge_domain_converts_with_voltage() {
        let b = BatteryRaw {
            status: "Charging".to_string(),
            ac_online: true,
            charge_now_uah: Some(2_000_000),  // 2000 mAh
            charge_full_uah: Some(4_000_000), // 4000 mAh
            current_now_ua: Some(1_000_000),  // 1000 mA
            voltage_now_uv: Some(12_000_000), // 12 V → 12 W 充电
            capacity_pct: Some(50.0),
            ..BatteryRaw::default()
        };
        let c = battery_convert(&b);
        assert!(c.charging);
        assert_eq!(c.remaining_capacity_mwh, Some(24_000)); // 2000mAh × 12V
        assert_eq!(c.rate_mw, Some(12_000)); // 12 W
    }

    #[test]
    fn battery_charge_domain_without_voltage_is_dropped() {
        let b = BatteryRaw {
            status: "Discharging".to_string(),
            charge_now_uah: Some(1_000_000),
            charge_full_uah: Some(4_000_000),
            current_now_ua: Some(500_000),
            // 没有电压：不猜量纲，容量与功率都放弃
            ..BatteryRaw::default()
        };
        let c = battery_convert(&b);
        assert_eq!(c.rate_mw, None);
        assert_eq!(c.remaining_capacity_mwh, None);
        assert!(c.discharging, "方向仍然可信");
    }

    #[test]
    fn battery_absurd_rate_is_filtered() {
        let b = BatteryRaw {
            status: "Charging".to_string(),
            power_now_uw: Some(5_000_000_000), // 5000 W，量纲误报
            ..BatteryRaw::default()
        };
        assert_eq!(battery_convert(&b).rate_mw, None, "超过 1 kW 一律视为误报");
    }

    /* ---------- meminfo ---------- */

    #[test]
    fn meminfo_parses_commit_and_load() {
        let text = "\
MemTotal:       16384000 kB
MemFree:         1234000 kB
MemAvailable:    8192000 kB
Buffers:          123000 kB
Committed_AS:    9876543 kB
CommitLimit:    20480000 kB
";
        let st = parse_meminfo(text).expect("meminfo 应能解析");
        assert_eq!(st.total_kb, 16_384_000);
        assert_eq!(st.avail_kb, 8_192_000);
        assert_eq!(st.load_pct, 50);
        assert_eq!(st.committed_kb, Some(9_876_543));
        assert_eq!(st.commit_limit_kb, Some(20_480_000));
    }

    #[test]
    fn meminfo_missing_total_is_none() {
        assert!(parse_meminfo("MemFree: 100 kB\n").is_none());
    }
}
