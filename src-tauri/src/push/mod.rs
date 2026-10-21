//! 数据推送：把监控指标按设置的节拍推给外部设备（TCP + BLE GATT 串口）
//!
//! 面向「第二屏」场景——DIY 硬件小屏、手机 / 平板、另一台电脑（树莓派状态屏等）
//! 连上来后持续接收一份紧凑 JSON（协议规范见 `docs/推送协议.md`）。两条通道：
//!
//! * **TCP**：本机做服务端监听 `0.0.0.0:<port>`（默认 17432）。同一端口收
//!   两种客户端——裸 TCP 按 NDJSON 帧（每行一个 JSON）；WebSocket（`ws.rs`，
//!   浏览器 / 手机网页直连）按每拍一条文本消息，accept 后在 250ms 判定窗口内
//!   peek 首字节分流（见 [`sniff_proto`]）。服务**只出不进**——从不解析客户端
//!   发来的任何字节，掉线判定只靠写失败 / 写超时（1s），因此不存在
//!   「外部输入触发行为」的面。
//! * **BLE GATT 串口**（Nordic UART Service，`ble.rs`）：广播 `PulseDeck`、
//!   等设备订阅 notify；载荷按 20 字节安全帧切发（协商 MTU 不可知，见 [`ble_frames`]）。
//!
//! 配置落盘 `push.json`（[`PushStore`]，与 taskbar.json 同一套原子写模式）；
//! 运行时是 [`PushHub`]：`sync()` 按配置「停旧起新」地重启推送 worker——
//! 改任何一项（端口 / 间隔 / 指标 / 通道）都等于重启推送子系统，代价可忽略。
//! 重启时新一代会先等旧一代放开监听端口再 `bind`（[`Released`]），
//! 否则同一端口上会以 10048 抢跑成「监听失败」。
//! worker 是裸 OS 线程（与采样线程同款 100ms 步进睡法），同时受全局
//! `running` 与自身代际 stop 控制，应用退出时一并收尾。
//!
//! 每拍**有连接才干活**：`sampler.latest()` 拉最后一次快照 → 组装载荷 →
//! 序列化一次 → 扇出给所有 TCP 客户端与 BLE 订阅者；无连接时零开销。
//!
//! 除每拍载荷外还有一组**一次性消息**：移动端布局（schema 2，见
//! [`PushHub::set_layout`]）——手机 / 平板两种设备形态各一条（带 `device` 字段，
//! 先 pad 后 phone 连发），连接建立后必达、布局每次变更立即全量重发，
//! 由 `crate::mobile_layout` 供数（桌面端「编辑布局 → 手机 / 平板」目标写的就是它）。
//! 另有一条**静态信息消息**（schema 3，[`build_host_info`]）：CPU 型号 / 逐卷磁盘
//! 容量这类「PC 卡片要、但每拍预算塞不下」的慢变数据——连接后必达、内容变化重发、
//! 60 拍心跳兜底（BLE 丢连接突发时的自愈路径），先于两条布局消息发出。

mod ble;
mod ws;

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::model::Snapshot;

/// 配置文件名（放在应用数据目录下）
const FILE_NAME: &str = "push.json";

/// 可推送的指标组（与设置页 chip 的 value 一致）。cores / procs 是移动端布局
/// 功能引入的可选组（默认不勾选，存量 DIY 屏设备的载荷一个字节都不变）。
pub const METRIC_KEYS: [&str; 8] = [
    "cpu", "mem", "gpu", "bat", "net", "disk", "cores", "procs",
];

/// 默认推送：CPU / 内存 / GPU / 网络（温度随 CPU 组、电池组默认不开——台式机没有）
const DEFAULT_METRICS: [&str; 4] = ["cpu", "mem", "gpu", "net"];

/// TCP 默认监听端口（避开常用服务段，且好记）
pub const DEFAULT_PORT: u16 = 17432;

/// 推送间隔的合理区间（与主面板采样间隔同一范围）
pub const MIN_INTERVAL_MS: u32 = 300;
pub const MAX_INTERVAL_MS: u32 = 10_000;
const DEFAULT_INTERVAL_MS: u32 = 1000;

/// 「停旧起新」时新一代等上一代放开端口的上限。worker 是 100ms 步进的轮询循环，
/// 正常几毫秒到一百毫秒就等到；等超时也照样往下走，由 bind 重试兜底。
const RELEASE_WAIT_MS: u64 = 1500;
/// 绑定端口的小步重试窗口与步长（交接等待超时后守住最后一道，见 [`bind_listener`]）
const BIND_RETRY_WINDOW_MS: u64 = 1500;
const BIND_RETRY_STEP_MS: u64 = 60;

/// 载荷协议版本（设备端按它决定字段解释；只在破坏性变更时才动）
pub const SCHEMA_VERSION: u8 = 1;

/* ------------------------------ 配置 ------------------------------ */

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PushConfig {
    /// 总开关
    #[serde(default)]
    pub enabled: bool,
    /// TCP 通道（局域网设备连 `ip:port` 收 NDJSON）
    #[serde(default = "default_tcp_enabled")]
    pub tcp_enabled: bool,
    /// BLE GATT 串口通道（Nordic UART Service）
    #[serde(default)]
    pub ble_enabled: bool,
    /// TCP 监听端口（0 = 未设置，sanitize 时回默认值）
    #[serde(default = "default_port")]
    pub port: u16,
    /// 推送间隔（毫秒），与主面板采样间隔相互独立
    #[serde(default = "default_interval")]
    pub interval_ms: u32,
    /// 要推送的指标组，取值见 `METRIC_KEYS`
    #[serde(default = "default_metrics")]
    pub metrics: Vec<String>,
}

fn default_tcp_enabled() -> bool {
    true
}
fn default_port() -> u16 {
    DEFAULT_PORT
}
fn default_interval() -> u32 {
    DEFAULT_INTERVAL_MS
}
fn default_metrics() -> Vec<String> {
    DEFAULT_METRICS.iter().map(|s| s.to_string()).collect()
}

impl Default for PushConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            tcp_enabled: true,
            ble_enabled: false,
            port: DEFAULT_PORT,
            interval_ms: DEFAULT_INTERVAL_MS,
            metrics: default_metrics(),
        }
    }
}

impl PushConfig {
    /// 清洗外部传入的配置：夹边界、过滤非法指标、通道至少留一个
    fn sanitize(&mut self) {
        if self.port == 0 {
            self.port = DEFAULT_PORT;
        }
        self.interval_ms = self.interval_ms.clamp(MIN_INTERVAL_MS, MAX_INTERVAL_MS);
        let mut seen: Vec<String> = Vec::new();
        for m in std::mem::take(&mut self.metrics) {
            if METRIC_KEYS.contains(&m.as_str()) && !seen.contains(&m) {
                seen.push(m);
            }
        }
        if seen.is_empty() {
            seen = default_metrics();
        }
        self.metrics = seen;
        // 两个通道全关没有意义（等效于 enabled=false，却还让界面亮着）——强制回 TCP。
        // 前端提交后以返回值回显，用户会看到勾被弹回来，与宠物「至少保留一项」同套路。
        if !self.tcp_enabled && !self.ble_enabled {
            self.tcp_enabled = true;
        }
    }
}

/// 配置的共享句柄（命令线程与推送 worker 共用一份），原子写 + 写盘串行化，
/// 与 `TaskbarStore` 同一套路（理由见 PetStore::save 的注释）。
#[derive(Clone)]
pub struct PushStore {
    config: Arc<Mutex<PushConfig>>,
    save_lock: Arc<Mutex<()>>,
}

impl PushStore {
    /// 从磁盘恢复；文件不存在或损坏时退回默认配置（推送默认关闭）
    pub fn load() -> Self {
        let path = config_path();
        let parsed = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<PushConfig>(&s).ok());
        let mut cfg = parsed.unwrap_or_default();
        cfg.sanitize();
        Self {
            config: Arc::new(Mutex::new(cfg)),
            save_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn get(&self) -> PushConfig {
        self.lock().clone()
    }

    /// 落盘并返回清洗后的配置（前端回显以这份为准）
    pub fn set(&self, mut config: PushConfig) -> PushConfig {
        config.sanitize();
        *self.lock() = config.clone();
        self.save(&config);
        config
    }

    fn save(&self, config: &PushConfig) {
        let _serialize_writes = self.save_lock.lock().unwrap_or_else(|e| e.into_inner());
        let path = config_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = path.with_extension("json.tmp");
        if serde_json::to_string_pretty(config)
            .ok()
            .and_then(|s| std::fs::write(&tmp, s).ok())
            .is_some()
        {
            let _ = std::fs::rename(&tmp, &path);
        }
    }

    /// 锁中毒不该把整个应用带崩（release 是 panic=abort），直接取回内部数据
    fn lock(&self) -> MutexGuard<'_, PushConfig> {
        self.config.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn config_path() -> std::path::PathBuf {
    crate::app_dir().join(FILE_NAME)
}

/* ------------------------------ 载荷（纯函数） ------------------------------ */

fn finite(v: &f32) -> bool {
    v.is_finite()
}

/// 载荷里的浮点统一 1 位小数（f64 域取整）：与主面板口径一致，也避免 f32 精度伪影
/// （45.2 的 f32 最近邻是 45.20000076293945，直接序列化会把 JSON 吹大）——BLE 按字节计费。
fn round1(v: f32) -> f64 {
    ((v as f64) * 10.0).round() / 10.0
}

/// 推给外部设备的一拍数据（协议 schema 1）。
///
/// 只含勾选的组、组内读不到的字段省略——BLE 通道按 20 字节帧切发，
/// 每一个字节都是空口开销，能省则省。字段口径与主面板一致
/// （CPU 占用 = 忙时占比、GPU 取主卡、速率用实测间隔换算）。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PushPayload {
    pub schema: u8,
    /// 快照采样时刻（Unix 毫秒）——如实反映数据年龄，不是推送时刻
    pub ts_ms: u64,
    /// 快照的实测采样间隔（毫秒）
    pub sample_interval_ms: u64,
    /// 主机名（手机 App 等据此标注这是哪台电脑）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<PushCpu>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mem: Option<PushMem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu: Option<PushGpu>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bat: Option<PushBat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net: Option<PushNet>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk: Option<PushDisk>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cores: Option<PushCores>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub procs: Option<PushProcs>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PushCpu {
    pub usage_pct: f64,
    pub freq_mhz: u64,
    /// `freq_mhz` 是否实时值（false = 平台只能给标称频率，见 AGENTS「实时主频」）
    pub freq_live: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temp_c: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power_w: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fan_rpm: Option<f64>,
    /// 风扇占空比（%）：没有真实 RPM 的机型靠它显示风扇状态
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fan_pct: Option<f64>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PushMem {
    pub usage_pct: f64,
    pub used_bytes: u64,
    pub total_bytes: u64,
    // 「可用」不单独推：sysinfo 的 used 恒等于 total−available（两平台同口径），
    // 设备端 total−used 即精确值——每拍预算装不下第三个 12 位字节数
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PushGpu {
    /// 主卡型号（`gpu.mainIndex` 指哪块就写哪块）
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mem_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temp_c: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power_w: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fan_rpm: Option<f64>,
    /// 风扇占空比（%）：笔记本独显常见只有占空比、没有真实 RPM
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fan_pct: Option<f64>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PushBat {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pct: Option<f64>,
    pub charging: bool,
    pub ac_online: bool,
    /// 电池功率（W，正值）：充电取充入功率、否则取放电功率，方向看 `charging`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_w: Option<f64>,
    /// 剩余续航（秒）；充电中 / 固件未提供时省略
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seconds_to_empty: Option<u64>,
    /// 充满还需（秒）；未在充电 / 固件未提供时省略
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seconds_to_full: Option<u64>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PushNet {
    pub down_bps: u64,
    pub up_bps: u64,
    /// 开机以来累计收 / 发（MiB 取整——字节计数是 13~15 位数字，BLE 按字节计费；
    /// 1MB 粒度对「累计流量」展示毫无损失）。显式 rename 保留 MiB 的大写，与 memMiB 同款
    #[serde(rename = "totalReceivedMiB")]
    pub total_received_mib: u64,
    #[serde(rename = "totalTransmittedMiB")]
    pub total_transmitted_mib: u64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PushDisk {
    pub read_bps: u64,
    pub write_bps: u64,
}

/// 每核负载。取整数 0~100：百分比数组按字符计费，1 位小数会让 16 核多花
/// 一倍字节（BLE 空口按字节计费，手机端进度条也用不着小数）。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PushCores {
    pub load: Vec<u8>,
}

/// 进程榜条目（按 CPU 排序的 top N；内存榜由设备端按 `memMiB` 重排同一条列表）
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PushProc {
    /// 截到 16 字符（按字符不按字节，进程名可能含非 ASCII）
    pub name: String,
    pub cpu_pct: f64,
    /// 显式 rename：serde camelCase 会把 `mem_mib` 转成 `memMib`，单位写法要保留 MiB 的大写
    #[serde(rename = "memMiB")]
    pub mem_mib: u64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PushProcs {
    pub list: Vec<PushProc>,
}

/// 进程榜条数与名字上限（BLE 预算：5 条约 250 字节）
const PROC_TOP_N: usize = 5;
const PROC_NAME_CHARS: usize = 16;

/// 按字符数截断（不能按字节切：会把多字节字符拦腰截断成非法 UTF-8）
fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    s.chars().take(max_chars).collect()
}

/// 从快照组装载荷：`metrics` 决定包含哪些组（读不到的组如实省略，
/// 无 GPU / 无电池的机器不会出现空组）。
pub fn build_payload(s: &Snapshot, metrics: &[String]) -> PushPayload {
    let has = |k: &str| metrics.iter().any(|m| m == k);
    let gpu_name = || {
        s.gpu
            .devices
            .get(s.gpu.main_index as usize)
            .map(|d| d.name.clone())
            .unwrap_or_default()
    };
    PushPayload {
        schema: SCHEMA_VERSION,
        ts_ms: s.ts_ms,
        sample_interval_ms: s.sample_interval_ms,
        host: s.system.host_name.clone().filter(|h| !h.is_empty()),
        cpu: has("cpu").then(|| PushCpu {
            usage_pct: round1(s.cpu.usage_total),
            freq_mhz: s.cpu.freq_mhz,
            freq_live: s.cpu.freq_live,
            temp_c: s.thermal.cpu_temp_c.filter(finite).map(round1),
            power_w: s.thermal.cpu_power_w.filter(finite).map(round1),
            fan_rpm: s.cpu.fan_rpm.filter(finite).map(round1),
            fan_pct: s.cpu.fan_pct.filter(finite).map(round1),
        }),
        mem: has("mem").then(|| PushMem {
            usage_pct: round1(s.memory.usage_pct),
            used_bytes: s.memory.used_bytes,
            total_bytes: s.memory.total_bytes,
        }),
        gpu: (has("gpu") && s.gpu.available).then(|| PushGpu {
            name: gpu_name(),
            usage_pct: s.gpu.usage_pct.filter(finite).map(round1),
            mem_pct: s.gpu.mem_pct.filter(finite).map(round1),
            // 主卡自带温度优先；GpuInfo 汇总层为 None 时再看硬件监控软件通道
            temp_c: s
                .gpu
                .temp_c
                .or(s.thermal.gpu_temp_c)
                .filter(finite)
                .map(round1),
            power_w: s.gpu.power_w.filter(finite).map(round1),
            fan_rpm: s.gpu.fan_rpm.filter(finite).map(round1),
            fan_pct: s.gpu.fan_pct.filter(finite).map(round1),
        }),
        bat: (has("bat") && s.battery.has_battery).then(|| PushBat {
            pct: s.battery.percent.filter(finite).map(round1),
            charging: s.battery.charging,
            ac_online: s.battery.ac_online,
            rate_w: if s.battery.charging {
                s.battery.charge_rate_w
            } else if s.battery.ac_online {
                // 已接入未充电（满电续充 / 养护模式）如实给 0——与 PC 电源卡同一口径。
                // 不能落到 discharge_rate_w：那种状态下它通常是 None，App 端会因此
                // 把「电池」明细行整行隐藏，和桌面预览对不上。
                Some(0.0)
            } else {
                s.battery.discharge_rate_w
            }
            .filter(finite)
            .map(round1),
            seconds_to_empty: s.battery.seconds_to_empty,
            seconds_to_full: s.battery.seconds_to_full,
        }),
        net: has("net").then(|| PushNet {
            down_bps: s.net.rx_bytes_per_sec,
            up_bps: s.net.tx_bytes_per_sec,
            total_received_mib: s.net.total_received / MIB,
            total_transmitted_mib: s.net.total_transmitted / MIB,
        }),
        disk: has("disk").then(|| {
            let (read_bps, write_bps) = s.disks.iter().fold((0u64, 0u64), |(r, w), d| {
                (
                    r.saturating_add(d.read_bytes_per_sec),
                    w.saturating_add(d.write_bytes_per_sec),
                )
            });
            PushDisk {
                read_bps,
                write_bps,
            }
        }),
        // 首拍等场景 per_core 可能还是空的——如实省略整组（设备端按「未勾选」处理）
        cores: (has("cores") && !s.cpu.per_core.is_empty()).then(|| PushCores {
            load: s
                .cpu
                .per_core
                .iter()
                .map(|v| v.round().clamp(0.0, 100.0) as u8)
                .collect(),
        }),
        procs: has("procs").then(|| PushProcs {
            list: s
                .cpu
                .top_cpu
                .iter()
                .take(PROC_TOP_N)
                .map(|p| PushProc {
                    name: truncate_chars(&p.name, PROC_NAME_CHARS),
                    cpu_pct: round1(p.cpu_usage),
                    mem_mib: p.memory_bytes / (1024 * 1024),
                })
                .collect(),
        }),
    }
}

/* ---------------- 静态信息消息（schema 3，纯函数） ---------------- */

/// 静态信息消息的协议号（与每拍载荷的 1、布局消息的 2 区分）
pub const INFO_SCHEMA: u8 = 3;

/// GiB 换算（info 消息里的磁盘容量按 GiB 取整——整数量级让「内容变了」极少触发重发）
const GIB: u64 = 1024 * 1024 * 1024;
/// MiB 换算（网络累计流量的取整粒度）
const MIB: u64 = 1024 * 1024;
/// 逐卷磁盘条数上限（info 消息自己的体积闸门：4 卷全量约 240B）
const INFO_VOLUME_CAP: usize = 4;
/// 挂载点 / 型号 / 其它显卡名的截断长度（按字符，与进程名截断同款理由）。
/// brand 取 36：主流型号串（"12th Gen Intel(R) Core(TM) i7-12700H" 36 字符、
/// "AMD Ryzen 7 7745HX 8-Core Processor" 35）都能完整放下，不会切在型号中间。
const INFO_MOUNT_CHARS: usize = 16;
const INFO_BRAND_CHARS: usize = 36;
const INFO_OTHERS_CAP: usize = 2;
/// info 心跳重发间隔（拍）。BLE 通道容量 2 且不排队，连接建立时的
/// 「info + 两条布局 + 首拍」突发可能无声丢掉 info——它不像布局有
/// 「PC 端每次编辑都重发」的自愈路径，靠周期重发兜底（60 拍 ≈ 1 分钟 @1Hz）。
const INFO_HEARTBEAT_TICKS: u32 = 60;

/// 静态 / 慢变信息（schema 3）：承载 PC 卡片要、但每拍载荷预算塞不下的数据——
/// CPU 型号与核心数、主卡显存总量、其它显卡名、电池 Wh、逐卷磁盘容量。
/// 组随 metrics 勾选（与每拍载荷同款缺席语义）；设备端按「schema≠1 忽略」处理，
/// 旧设备零影响。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HostInfo {
    pub schema: u8,
    #[serde(rename = "type")]
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<InfoCpu>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu: Option<InfoGpu>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bat: Option<InfoBat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk: Option<InfoDisk>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InfoCpu {
    pub brand: String,
    pub physical_cores: usize,
    pub logical_cores: usize,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InfoGpu {
    /// 主卡显存总量（byte）；拿不到（核显走 PDH 无显存计数等）时省略
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mem_total_bytes: Option<u64>,
    /// 主卡之外的设备名（App 端「其它显卡」行；占用%是每拍数据，预算不够不搬）
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub others: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InfoBat {
    /// 剩余 / 满充容量（Wh，取整——整数量级让重发极少触发）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remaining_wh: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_charge_wh: Option<u64>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InfoDisk {
    /// 逐卷容量（按挂载点排序、cap [`INFO_VOLUME_CAP`]；每卷读写速率不搬——
    /// 每拍预算装不下，App 端用整卡合计速率行）
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub volumes: Vec<InfoVolume>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InfoVolume {
    pub mount: String,
    /// 已用百分比（整数）
    pub pct: u8,
    /// 显式 rename：camelCase 会把 `used_gib` 转成 `usedGib`，GiB 的大写要保留（与 memMiB 同款）
    #[serde(rename = "usedGiB")]
    pub used_gib: u64,
    #[serde(rename = "totalGiB")]
    pub total_gib: u64,
}

/// 从快照组装静态信息消息。纯函数 + 产出可序列化结构：worker 每拍序列化成
/// 字符串与上次比对，变了才重发（brand / 核心数 / 容量 GiB / 整数 Wh 都是
/// 慢变量，正常运行几乎不会变）。
pub fn build_host_info(s: &Snapshot, metrics: &[String]) -> HostInfo {
    let has = |k: &str| metrics.iter().any(|m| m == k);
    let main_gpu = || s.gpu.devices.get(s.gpu.main_index as usize);
    HostInfo {
        schema: INFO_SCHEMA,
        kind: "info",
        cpu: has("cpu").then(|| InfoCpu {
            brand: truncate_chars(s.cpu.brand.trim(), INFO_BRAND_CHARS),
            physical_cores: s.cpu.physical_cores,
            logical_cores: s.cpu.logical_cores,
        }),
        gpu: (has("gpu") && s.gpu.available)
            .then(|| InfoGpu {
                mem_total_bytes: main_gpu()
                    .map(|d| d.mem_total_bytes)
                    .filter(|t| *t > 0),
                others: s
                    .gpu
                    .devices
                    .iter()
                    .filter(|d| d.index != s.gpu.main_index)
                    .take(INFO_OTHERS_CAP)
                    .map(|d| truncate_chars(&d.name, PROC_NAME_CHARS))
                    .collect(),
            }),
        bat: (has("bat") && s.battery.has_battery).then(|| InfoBat {
            remaining_wh: s.battery.remaining_wh.map(|v| v.round() as u64),
            full_charge_wh: s.battery.full_charge_wh.map(|v| v.round() as u64),
        }),
        disk: has("disk").then(|| InfoDisk {
            volumes: {
                let mut vs: Vec<InfoVolume> = s
                    .disks
                    .iter()
                    .filter(|d| d.total_bytes > 0)
                    .map(|d| InfoVolume {
                        mount: truncate_chars(&d.mount_point, INFO_MOUNT_CHARS),
                        pct: d.usage_pct.round().clamp(0.0, 100.0) as u8,
                        used_gib: d.used_bytes / GIB,
                        total_gib: d.total_bytes / GIB,
                    })
                    .collect();
                vs.sort_by(|a, b| a.mount.cmp(&b.mount));
                vs.truncate(INFO_VOLUME_CAP);
                vs
            },
        }),
    }
}

/// 序列化好的 info JSON（worker 用它做逐拍字符串比对与扇出）
fn host_info_json(s: &Snapshot, metrics: &[String]) -> Option<String> {
    serde_json::to_string(&build_host_info(s, metrics)).ok()
}

/* ---------------- BLE 帧格式（纯函数，文档口径见 docs/推送协议.md） ---------------- */


/// BLE 单帧数据段上限：2 字节帧头 + 18 字节 = 20 字节，落在最小协商 MTU（23−3）之内。
pub const BLE_FRAME_DATA: usize = 18;

/// 把一段 UTF-8 JSON 切成 BLE notify 帧：`[idx:u8][total:u8][≤18 字节数据]`。
///
/// 协商 MTU 在各平台都拿不到，只能按最保守的 20 字节切——1Hz 全组载荷
/// 约 20 帧/拍，BLE 完全吃得下。设备端按 (idx, total) 重组后按 UTF-8 解析。
pub fn ble_frames(payload: &[u8]) -> Vec<Vec<u8>> {
    if payload.is_empty() {
        return Vec::new();
    }
    let total = payload.len().div_ceil(BLE_FRAME_DATA).min(u8::MAX as usize);
    // 超预算的载荷属于配置异常（单拍 JSON 不该超过 4.5KB），截断保协议不崩
    let bytes = &payload[..(total * BLE_FRAME_DATA).min(payload.len())];
    let mut frames = Vec::with_capacity(total);
    for (idx, chunk) in bytes.chunks(BLE_FRAME_DATA).enumerate() {
        let mut f = Vec::with_capacity(2 + chunk.len());
        f.push(idx as u8);
        f.push(total as u8);
        f.extend_from_slice(chunk);
        frames.push(f);
    }
    frames
}

/// 帧重组（设备端参考实现，也是单测的对照）：帧序错、缺帧、total 不一致都判失败。
/// 生产侧不用它（只有设备端需要重组），但保留并由单测守护——它是协议的判定规则
/// （与 `taskbar::covers_monitor` 同款处理）。
#[cfg_attr(not(test), allow(dead_code))]
pub fn ble_reassemble(frames: &[Vec<u8>]) -> Option<Vec<u8>> {
    let total = frames.first()?.get(1).map(|t| *t as usize)?;
    if total == 0 || frames.len() != total {
        return None;
    }
    let mut slots: Vec<Option<Vec<u8>>> = vec![None; total];
    for f in frames {
        let (Some(idx), Some(t)) = (f.first().map(|i| *i as usize), f.get(1)) else {
            return None;
        };
        if idx >= total || *t as usize != total {
            return None;
        }
        slots[idx] = Some(f[2..].to_vec());
    }
    let mut out = Vec::new();
    for s in slots {
        out.extend_from_slice(&s?);
    }
    Some(out)
}

/* ---------------- 协议分流：同一端口收裸 TCP 与 WebSocket ---------------- */

/// 连接判定窗口。裸 TCP（NDJSON）客户端连上后一言不发等着收，而 WebSocket
/// 客户端会立刻发 HTTP 升级请求——窗口内等到首字节且长得像请求行就按
/// WebSocket 处理，超窗仍无字节按 NDJSON 处理。裸 TCP 客户端的首拍推送
/// 因此最多延后一个窗口（默认 1Hz 下无感），协议文档如实写明。
const SNIFF_WINDOW_MS: u64 = 250;

#[derive(PartialEq, Eq, Debug)]
enum Proto {
    /// 裸 TCP：按 NDJSON 帧（每行一个 JSON）推
    Raw,
    /// WebSocket：先握手，之后每拍一条文本消息（`ws.rs`）
    Ws,
}

/// 首字节像不像 HTTP 请求行（方法名 + 空格）——WebSocket 握手必以之开头。
/// 方法名大小写敏感（HTTP 规范如此）；不足 4 字节（最短方法 GET + 空格）
/// 判不出来，由 [`sniff_proto`] 继续等。
fn looks_like_http(prefix: &[u8]) -> bool {
    const METHODS: [&[u8]; 8] = [
        b"GET ", b"POST ", b"PUT ", b"DELETE ", b"HEAD ", b"OPTIONS ", b"PATCH ", b"CONNECT ",
    ];
    METHODS.iter().any(|m| prefix.len() >= m.len() && &prefix[..m.len()] == *m)
}

/// 在判定窗口内非阻塞 peek 首字节做协议分流。peek 不消费字节：WebSocket
/// 握手照常读到完整请求；裸 TCP 侧收到的任何字节照旧不读（只出不进，
/// 留在内核缓冲里无害）。此函数会占用调用线程至多一个窗口，只在 accept
/// 新连接时跑一次。
fn sniff_proto(s: &TcpStream) -> Proto {
    let _ = s.set_nonblocking(true);
    let mut buf = [0u8; 8];
    let mut waited = 0u64;
    loop {
        let n = s.peek(&mut buf).unwrap_or(0);
        // 攒够 4 字节才判定，避免把分段到达的半个请求行误判成非 HTTP
        if n >= 4 {
            return if looks_like_http(&buf[..n]) { Proto::Ws } else { Proto::Raw };
        }
        if waited >= SNIFF_WINDOW_MS {
            // 超窗：要么一字不发（裸 TCP 客户端的常态），要么字节不足以判成 HTTP
            return if n > 0 && looks_like_http(&buf[..n]) { Proto::Ws } else { Proto::Raw };
        }
        std::thread::sleep(Duration::from_millis(10));
        waited += 10;
    }
}

/* ------------------------------ 运行时 ------------------------------ */

/// 推送子系统的状态快照（设置页状态行据此渲染）
#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct PushStatus {
    pub enabled: bool,
    /// BLE 已启动但尚未广播成功时（advertising=false 且 error 为空）= 启动中
    pub tcp_listening: bool,
    /// `局域网IP:端口`（拿不到局域网 IP 时是 `0.0.0.0:端口`）
    pub tcp_addr: String,
    pub tcp_clients: usize,
    pub tcp_error: String,
    pub ble_advertising: bool,
    /// 当前订阅 notify 的 BLE 设备数
    pub ble_clients: usize,
    pub ble_error: String,
    /// 最近一次成功扇出的时刻（Unix 毫秒）；0 = 从未推过
    pub last_push_ms: u64,
}

/// 「上一代已放开监听端口」的信号：worker 收尾（关闭监听 socket）时置位并唤醒等待者。
///
/// 「停旧起新」**不能只看停止位**：worker 是 100ms 步进的轮询循环，从看到停止位到
/// 真正 drop 监听 socket 之间有一个节拍的延迟。新一代若抢在这段里 bind 同一个端口，
/// Windows 会以 10048（每个套接字地址只允许使用一次）拒绝——端口其实是空的，只是旧
/// socket 还没关，界面却显示「监听失败」。应用启动时必撞这条路：lib.rs 先按 push.json
/// 起一代，前端页面起来后又把 localStorage 里的同一份配置推给 `set_push_config`
/// （等于再 sync 一次）。等待放在**新 worker 线程里**（而非 `sync()` 里），
/// 是因为 `set_push_config` 是同步命令，跑在 WebView2 的 IPC 回调线程上，不能在那里阻塞。
#[derive(Default)]
struct Released {
    flag: Mutex<bool>,
    cv: Condvar,
}

impl Released {
    fn mark(&self) {
        *self.flag.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.cv.notify_all();
    }

    /// 等旧一代放开端口；到点还没等到返回 false（交 bind 重试兜底）
    fn wait(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut set = self.flag.lock().unwrap_or_else(|e| e.into_inner());
        while !*set {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            let (guard, _) = self
                .cv
                .wait_timeout(set, left)
                .unwrap_or_else(|e| e.into_inner());
            set = guard;
        }
        true
    }
}

/// 保证 [`Released`] 在 worker 退场时必定置位（含 panic 路径）。
/// 它在 `run()` 里声明得比 `listener` 早，drop 得比它晚——置位时监听 socket 一定已关。
struct ReleaseGuard(Arc<Released>);

impl Drop for ReleaseGuard {
    fn drop(&mut self) {
        self.0.mark();
    }
}

/// 推送运行时：按配置「停旧起新」地管理 worker 线程。
///
/// 每一代 worker 拿一份**独占的** `Arc<Mutex<PushStatus>>`——`sync()` 重启时
/// 换新的一份挂到槽位上，旧 worker 收尾时的迟写只会落到自己那份（已无人读），
/// 不会把新 worker 刚写的状态覆盖掉。
pub struct PushHub {
    sampler: Arc<crate::sampler::Sampler>,
    /// 全局存活位（lib.rs 的 `running`）：应用退出时一并带走 worker
    app_running: Arc<AtomicBool>,
    inner: Mutex<HubInner>,
    status: Mutex<Arc<Mutex<PushStatus>>>,
    /// 移动端布局消息槽（Hub 级、跨 worker 代际共享）：手机 / 平板两种设备
    /// 形态各一份序列化好的 schema 2 JSON。`set_layout` 换内容并 bump 世代号，
    /// worker 每拍比对世代决定要不要（重）发——连接建立后必达、每次变更立即全量重发。
    layout: Arc<Mutex<LayoutSlot>>,
}

/// 布局消息槽。世代号从 1 起（0 = 从未设置过），worker 初值取 0，
/// 因此每一代 worker 的首个连接都会拿到当前布局。任一设备形态变化都 bump
/// 同一个世代号——worker 的触发逻辑不区分是谁变了，重发就两条一起发。
#[derive(Default)]
struct LayoutSlot {
    gen: u64,
    phone: Option<String>,
    pad: Option<String>,
}

struct HubInner {
    /// 当前一代 worker 的停止位；没有在跑的 worker 时为 None
    stop: Option<Arc<AtomicBool>>,
    /// 当前一代 worker 的退场信号（下一代 bind 之前等它放开端口）
    released: Option<Arc<Released>>,
    _gen: AtomicU64,
}

impl PushHub {
    pub fn new(sampler: Arc<crate::sampler::Sampler>, app_running: Arc<AtomicBool>) -> Self {
        Self {
            sampler,
            app_running,
            inner: Mutex::new(HubInner {
                stop: None,
                released: None,
                _gen: AtomicU64::new(0),
            }),
            status: Mutex::new(Arc::new(Mutex::new(PushStatus::default()))),
            layout: Arc::new(Mutex::new(LayoutSlot::default())),
        }
    }

    /// 更新某一设备形态的布局消息（`device` 取 `mobile_layout::DEVICE_PHONE` /
    /// `DEVICE_PAD`，未知串按手机处理；None = 清空，如布局组件被全删时也照发空表）。
    /// 内容没变就直接跳过——前端每次编辑落位都会调一次，拖回原位的情形不必重发。
    pub fn set_layout(&self, device: &str, msg: Option<String>) {
        let device = crate::mobile_layout::canonical_device(device);
        let mut slot = self.layout.lock().unwrap_or_else(|e| e.into_inner());
        let slot_msg = if device == crate::mobile_layout::DEVICE_PAD {
            &mut slot.pad
        } else {
            &mut slot.phone
        };
        if *slot_msg == msg {
            return;
        }
        *slot_msg = msg;
        slot.gen += 1;
    }

    /// 按配置重启推送子系统：先停旧 worker，`enabled` 时起新的一代。
    /// 注意这里**不做 sanitize**——`store.set()` 已经洗过，测试直连时自己保证合法性
    /// （port=0 是合法输入：交给内核选临时端口，端到端测试靠它避开端口冲突）。
    pub fn sync(&self, cfg: &PushConfig) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(stop) = inner.stop.take() {
            stop.store(true, Ordering::Relaxed);
        }
        // 旧一代的退场信号交给新 worker：它 bind 之前先等端口真的空出来
        let prev_released = inner.released.take();
        let fresh = Arc::new(Mutex::new(PushStatus {
            enabled: cfg.enabled,
            ..Default::default()
        }));
        *self.status.lock().unwrap_or_else(|e| e.into_inner()) = fresh.clone();
        if !cfg.enabled {
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        inner.stop = Some(stop.clone());
        let released = Arc::new(Released::default());
        inner.released = Some(released.clone());
        let worker = Worker {
            cfg: cfg.clone(),
            stop,
            app_running: self.app_running.clone(),
            sampler: self.sampler.clone(),
            status: fresh,
            released,
            prev_released,
            layout: self.layout.clone(),
        };
        std::thread::Builder::new()
            .name("pd-push".into())
            .spawn(move || worker.run())
            .ok();
    }

    pub fn status(&self) -> PushStatus {
        let slot = self.status.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let snapshot = slot.lock().unwrap_or_else(|e| e.into_inner()).clone();
        snapshot
    }
}

/// 一代推送 worker 的全部输入：配置快照 + 停止位 + 数据源 + 状态出口
struct Worker {
    cfg: PushConfig,
    stop: Arc<AtomicBool>,
    app_running: Arc<AtomicBool>,
    sampler: Arc<crate::sampler::Sampler>,
    status: Arc<Mutex<PushStatus>>,
    /// 本代退场信号（置位 = 监听 socket 已关，端口可以让给下一代了）
    released: Arc<Released>,
    /// 上一代的退场信号：bind 之前先等它放开端口（见 [`Released`]）
    prev_released: Option<Arc<Released>>,
    /// 移动端布局消息槽（Hub 级共享，跨代不换）
    layout: Arc<Mutex<LayoutSlot>>,
}

impl Worker {
    fn alive(&self) -> bool {
        !self.stop.load(Ordering::Relaxed) && self.app_running.load(Ordering::Relaxed)
    }

    fn lock_status(&self) -> MutexGuard<'_, PushStatus> {
        self.status.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn run(self) {
        // 退场信号必须置位（含 panic 路径）：声明在 listener 之前 → drop 在它之后
        let _release_guard = ReleaseGuard(self.released.clone());
        let mut listener: Option<TcpListener> = None;
        if self.cfg.tcp_enabled {
            // 先等上一代真正关闭监听 socket，再 bind——否则同一端口上「停旧起新」
            // 会以 10048 抢跑成失败（详见 [`Released`] 的说明）
            if let Some(prev) = &self.prev_released {
                prev.wait(Duration::from_millis(RELEASE_WAIT_MS));
            }
            match bind_listener(self.cfg.port) {
                Ok(l) => {
                    // 只让 accept 非阻塞轮询；收下来的连接要显式恢复阻塞模式，
                    // 否则写超时（剔除慢消费者的判据）不生效
                    let _ = l.set_nonblocking(true);
                    let port = l.local_addr().map(|a| a.port()).unwrap_or(self.cfg.port);
                    let mut st = self.lock_status();
                    st.tcp_listening = true;
                    st.tcp_addr = format!("{}:{}", local_ip(), port);
                    drop(st);
                    listener = Some(l);
                }
                Err(e) => {
                    self.lock_status().tcp_error = format!("监听 {} 端口失败：{e}", self.cfg.port);
                }
            }
        }
        let ble_server = self
            .cfg
            .ble_enabled
            .then(ble::BleServer::start);

        let mut clients: Vec<TcpStream> = Vec::new();
        let mut ws_clients: Vec<ws::WsClient> = Vec::new();
        let interval = self.cfg.interval_ms.max(100);
        let mut waited = 0u32;
        // 新客户端连上立即推一拍，不必干等一个间隔
        let mut force_push = false;
        // 布局消息的发送状态：已发到哪个世代 + 上一拍的 BLE 订阅数（0→N 判「新连接」）
        let mut layout_seen = 0u64;
        let mut prev_ble_subs = 0usize;
        // 静态信息（schema 3）的发送状态：最近一次发出的内容 + 心跳计数
        let mut last_info: Option<String> = None;
        let mut info_ticks: u32 = 0;
        loop {
            if !self.alive() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
            waited = waited.saturating_add(100);

            // BLE 侧状态合进 PushStatus（广播中 / 错误 / 订阅数），设置页状态行用
            if let Some(b) = &ble_server {
                let (adv, err, subs) = b.snapshot();
                let mut st = self.lock_status();
                st.ble_advertising = adv;
                st.ble_error = err;
                st.ble_clients = subs;
            }

            if let Some(l) = &listener {
                loop {
                    match l.accept() {
                        Ok((s, _)) => {
                            // 同端口双协议：先嗅探分流（至多占用本线程一个判定窗口）
                            match sniff_proto(&s) {
                                Proto::Ws => {
                                    // 嗅探时是非阻塞模式，握手前恢复阻塞（读写超时才有意义）
                                    let _ = s.set_nonblocking(false);
                                    ws_clients.push(ws::WsClient::spawn(s));
                                }
                                Proto::Raw => {
                                    let _ = s.set_nonblocking(false);
                                    let _ = s.set_nodelay(true);
                                    let _ = s.set_write_timeout(Some(Duration::from_secs(1)));
                                    clients.push(s);
                                }
                            }
                            force_push = true;
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(_) => break,
                    }
                }
            }

            // 布局消息（schema 2，移动端布局，手机/平板各一条）：连接建立后必达、
            // 每次变更立即全量重发。走独立判断、不受推送间隔节流（100ms 一查）；
            // BLE 订阅是异步事件（无 accept 路径），用订阅数 0→N 当「新连接」。
            // BLE 通道容量 2 且 try_send 丢满不排队——两条布局背靠背推送时若与
            // 一拍载荷挤在一起可能被无声丢弃，由「App 端按设备独立持久化 +
            // 每次编辑都重发」自愈，不加队列。
            let ble_subs_now = ble_server
                .as_ref()
                .map(ble::BleServer::subscribers)
                .unwrap_or(0);
            if !clients.is_empty() || !ws_clients.is_empty() || ble_subs_now > 0 {
                let (send_any, outgoing) = {
                    let slot = self.layout.lock().unwrap_or_else(|e| e.into_inner());
                    let changed = slot.gen != layout_seen;
                    let ble_joined = ble_subs_now > 0 && prev_ble_subs == 0;
                    if changed || ble_joined || force_push {
                        layout_seen = slot.gen;
                        (true, (slot.pad.clone(), slot.phone.clone()))
                    } else {
                        (false, (None, None))
                    }
                };
                // 静态信息（schema 3）先于两条布局消息：App 要先拿到型号 / 逐卷容量
                // 这类静态数据，随后渲染布局卡片才不缺行。触发点与布局同款
                // （新客户端连上 / 布局变更），平时不拍拍重发——变化检测在数据拍里做。
                if send_any {
                    if let Some(info) = host_info_json(&self.sampler.latest(), &self.cfg.metrics)
                    {
                        fanout_text(&mut clients, &mut ws_clients, &ble_server, &info);
                        last_info = Some(info);
                    }
                }
                // 先 pad 后 phone：旧版 App 不认 device 字段，两条都会应用、
                // 后者覆盖前者——最终停在 phone，与单布局时代行为一致
                if let Some(msg) = outgoing.0 {
                    fanout_text(&mut clients, &mut ws_clients, &ble_server, &msg);
                }
                if let Some(msg) = outgoing.1 {
                    fanout_text(&mut clients, &mut ws_clients, &ble_server, &msg);
                }
            }
            prev_ble_subs = ble_subs_now;

            if !force_push && waited < interval {
                continue;
            }
            waited = 0;
            force_push = false;

            let ble_subs = ble_subs_now;
            if clients.is_empty() && ws_clients.is_empty() && ble_subs == 0 {
                // 无连接不干活；顺带把状态里的客户端数清零（上次连接可能刚断）
                let mut st = self.lock_status();
                st.tcp_clients = 0;
                drop(st);
                continue;
            }

            let snap = self.sampler.latest();
            let payload = build_payload(&snap, &self.cfg.metrics);
            let Ok(mut line) = serde_json::to_string(&payload) else {
                continue;
            };
            let ble_bytes = line.clone().into_bytes();
            // WebSocket 侧用不带换行的同一份 JSON（每拍一条文本消息）
            let ws_json = line.clone();
            line.push('\n');

            // TCP 扇出：写失败 / 超时（1s）视为掉线，就地剔除
            let mut i = 0;
            while i < clients.len() {
                if clients[i].write_all(line.as_bytes()).is_ok() {
                    i += 1;
                } else {
                    clients.remove(i);
                }
            }
            // WS 扇出：队列满（慢消费者）或连接已关（线程退出后通道断连）
            // 都剔除——与 NDJSON 的「写超时 1s 剔除」同一哲学
            let mut i = 0;
            while i < ws_clients.len() {
                if ws_clients[i].send(&ws_json) {
                    i += 1;
                } else {
                    ws_clients.remove(i);
                }
            }
            if let Some(b) = &ble_server {
                b.push(&ble_bytes);
            }

            // 静态信息（schema 3）的变化重发 + 心跳兜底。GiB / 整数 Wh 粒度让
            // 「内容变了」极少触发；心跳是为 BLE 准的——连接突发时容量 2 的通道
            // 可能无声丢掉 info，它没有布局消息「编辑即重发」的自愈路径。
            if let Some(info) = host_info_json(&snap, &self.cfg.metrics) {
                info_ticks = info_ticks.saturating_add(1);
                if info_ticks >= INFO_HEARTBEAT_TICKS || last_info.as_deref() != Some(&info) {
                    fanout_text(&mut clients, &mut ws_clients, &ble_server, &info);
                    last_info = Some(info);
                    info_ticks = 0;
                }
            }

            let mut st = self.lock_status();
            st.tcp_clients = clients.len() + ws_clients.len();
            st.last_push_ms = now_ms();
        }
        // 收尾：监听 socket 与客户端连接随 drop 关闭；WS 句柄 drop 后各连接
        // 线程经通道断连感知退出（礼貌 Close）；BLE 在 Drop 里停广播
        drop(listener);
        drop(clients);
        drop(ws_clients);
        drop(ble_server);
        // 这里之后 `_release_guard` 才 drop 并置位退场信号——「端口已放开」的
        // 判据就建立在「显式 drop(listener) 在它之前」这个顺序上，别调整。
    }
}

/// 三路扇出一条已序列化好的文本消息（布局消息用）：裸 TCP 补换行成 NDJSON 行、
/// WS 原文文本消息（不带换行）、BLE 交 `BleServer` 内部分帧。掉线剔除规则与
/// 每拍载荷的扇出完全一致。
fn fanout_text(
    clients: &mut Vec<TcpStream>,
    ws_clients: &mut Vec<ws::WsClient>,
    ble_server: &Option<ble::BleServer>,
    msg: &str,
) {
    let mut line = msg.to_string();
    line.push('\n');
    let mut i = 0;
    while i < clients.len() {
        if clients[i].write_all(line.as_bytes()).is_ok() {
            i += 1;
        } else {
            clients.remove(i);
        }
    }
    let mut i = 0;
    while i < ws_clients.len() {
        if ws_clients[i].send(msg) {
            i += 1;
        } else {
            ws_clients.remove(i);
        }
    }
    if let Some(b) = ble_server {
        b.push(msg.as_bytes());
    }
}

/// 绑定监听端口，带上限的小步重试。新一代在 [`Released`] 上等到超时（上一代卡在
/// 写超时 / BLE 交接里，迟迟没关掉监听 socket）时，这一层是最后的兜底——占位者往往
/// 几十毫秒后就撒手。窗口内仍未成功就把最后一个错误如实交给状态行（真被别的程序
/// 长期占着时不该假装没事）。
///
/// 不用 SO_REUSEADDR 兜底：2026-10-17 实测（Win11）服务端主动关闭留下的 TIME_WAIT
/// 并不拦重新 bind，压根没有需要绕过的场景，而 SO_REUSEADDR 在 Windows 上会让
/// **别的进程**也能绑上同一端口（抢流量），代价远大于收益。
fn bind_listener(port: u16) -> std::io::Result<TcpListener> {
    let deadline = Instant::now() + Duration::from_millis(BIND_RETRY_WINDOW_MS);
    loop {
        match TcpListener::bind(("0.0.0.0", port)) {
            Err(_) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(BIND_RETRY_STEP_MS));
            }
            result => return result,
        }
    }
}

/// 本机对外的局域网 IP（主路由出口）。UDP connect 不会真的发包，只是让协议栈选路。
/// 拿不到（无网络 / 离线）回退 0.0.0.0——状态行如实显示，用户自己知道内网 IP 也行。
fn local_ip() -> String {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .ok()
        .and_then(|s| s.connect("8.8.8.8:80").ok().map(|_| s))
        .and_then(|s| s.local_addr().ok())
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|| "0.0.0.0".to_string())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/* ------------------------------ 测试 ------------------------------ */

#[cfg(test)]
mod tests {
    use super::*;

    /* ---------- 配置 ---------- */

    #[test]
    fn sanitize_fixes_port_interval_metrics_transports() {
        let mut cfg = PushConfig {
            enabled: true,
            tcp_enabled: false,
            ble_enabled: false,
            port: 0,
            interval_ms: 10,
            metrics: vec!["gpu".into(), "cpu".into(), "gpu".into(), "温度".into()],
        };
        cfg.sanitize();
        assert_eq!(cfg.port, DEFAULT_PORT);
        assert_eq!(cfg.interval_ms, MIN_INTERVAL_MS);
        assert_eq!(cfg.metrics, vec!["gpu".to_string(), "cpu".to_string()]);
        assert!(cfg.tcp_enabled, "通道全关时强制回 TCP");

        cfg.interval_ms = 999_999;
        cfg.sanitize();
        assert_eq!(cfg.interval_ms, MAX_INTERVAL_MS);
    }

    #[test]
    fn sanitize_falls_back_to_default_metrics() {
        let mut cfg = PushConfig {
            metrics: vec!["nope".into()],
            ..Default::default()
        };
        cfg.sanitize();
        assert_eq!(cfg.metrics, default_metrics());
    }

    /// 缺字段的配置文件必须还能解析（老版本文件 / 手改），否则等于悄悄把开关关掉
    #[test]
    fn partial_config_still_parses() {
        let cfg: PushConfig = serde_json::from_str("{}").expect("空配置解析成默认值");
        assert!(!cfg.enabled);
        assert!(cfg.tcp_enabled);
        assert!(!cfg.ble_enabled);
        assert_eq!(cfg.port, DEFAULT_PORT);
        assert_eq!(cfg.interval_ms, DEFAULT_INTERVAL_MS);
        assert_eq!(cfg.metrics, default_metrics());
    }

    /* ---------- 载荷 ---------- */

    fn sample_snapshot() -> Snapshot {
        let mut s = Snapshot::default();
        s.ts_ms = 1_729_000_000_000;
        s.sample_interval_ms = 1000;
        s.system.host_name = Some("desktop".into());
        s.cpu.brand = "12th Gen Intel(R) Core(TM) i7-12700H".into();
        s.cpu.usage_total = 23.5;
        s.cpu.physical_cores = 14;
        s.cpu.logical_cores = 20;
        s.cpu.freq_mhz = 2900;
        s.cpu.freq_live = true;
        s.cpu.per_core = vec![
            12.0, 45.5, 100.0, 3.2, 8.0, 27.5, 66.6, 0.4, 12.0, 45.5, 100.0, 3.2, 8.0, 27.5, 66.6,
            0.4,
        ];
        s.cpu.top_cpu = vec![
            crate::model::ProcInfo {
                pid: 1,
                name: "pulse-deck.exe".into(),
                cpu_usage: 12.5,
                memory_bytes: 36 * 1024 * 1024,
            },
            crate::model::ProcInfo {
                pid: 2,
                name: "一个名字特别长的进程名字会被截断.exe".into(),
                cpu_usage: 3.3,
                memory_bytes: 500 * 1024 * 1024,
            },
        ];
        s.thermal.cpu_temp_c = Some(61.5);
        s.thermal.cpu_power_w = Some(12.25);
        s.cpu.fan_rpm = Some(2400.0);
        s.memory.usage_pct = 58.3;
        s.memory.used_bytes = 9_500_000_000;
        s.memory.total_bytes = 17_000_000_000;
        s.gpu.available = true;
        s.gpu.main_index = 1;
        s.gpu.devices = vec![
            crate::model::GpuDevice {
                index: 0,
                name: "Intel(R) Iris(R) Xe Graphics".into(),
                usage_pct: Some(6.0),
                source: "pdh".into(),
                ..Default::default()
            },
            crate::model::GpuDevice {
                index: 1,
                name: "NVIDIA GeForce RTX 3060 Laptop GPU".into(),
                usage_pct: Some(37.0),
                mem_pct: Some(44.0),
                mem_total_bytes: 6 * GIB,
                temp_c: Some(66.0),
                power_w: Some(75.5),
                source: "nvml".into(),
                ..Default::default()
            },
        ];
        s.gpu.usage_pct = Some(37.0);
        s.gpu.mem_pct = Some(44.0);
        s.gpu.temp_c = Some(66.0);
        s.gpu.power_w = Some(75.5);
        s.gpu.mem_total_bytes = 6 * GIB;
        s.battery.has_battery = true;
        s.battery.charging = true;
        s.battery.ac_online = true;
        s.battery.percent = Some(82.0);
        s.battery.charge_rate_w = Some(45.2);
        s.battery.discharge_rate_w = None;
        s.battery.remaining_wh = Some(67.3);
        s.battery.full_charge_wh = Some(82.1);
        s.battery.seconds_to_full = Some(3_600);
        s.net.rx_bytes_per_sec = 1_234_567;
        s.net.tx_bytes_per_sec = 98_765;
        // 累计取整 MiB 后的期望值要能对上：12_582_912_000 B = 12000 MiB、2_097_152_000 B = 2000 MiB
        s.net.total_received = 12_000 * MIB;
        s.net.total_transmitted = 2_000 * MIB;
        s.disks = vec![crate::model::DiskInfo {
            mount_point: "C:\\".into(),
            total_bytes: 932 * GIB,
            used_bytes: 775 * GIB,
            usage_pct: 83.1,
            read_bytes_per_sec: 300_000,
            write_bytes_per_sec: 1_200_000,
            ..Default::default()
        }];
        s
    }

    #[test]
    fn payload_only_contains_selected_groups() {
        let s = sample_snapshot();
        let only_mem = build_payload(&s, &["mem".to_string()]);
        let json = serde_json::to_string(&only_mem).unwrap();
        assert!(!json.contains("\"cpu\""));
        assert!(!json.contains("\"gpu\""));
        assert!(!json.contains("\"bat\""));
        assert!(!json.contains("\"net\""));
        assert!(!json.contains("\"disk\""));
        assert!(json.contains("\"usedBytes\":9500000000"));
    }

    #[test]
    fn payload_shape_and_units() {
        let s = sample_snapshot();
        let all = build_payload(
            &s,
            &METRIC_KEYS.iter().map(|k| k.to_string()).collect::<Vec<_>>(),
        );
        let v: serde_json::Value = serde_json::to_value(&all).unwrap();
        assert_eq!(v["schema"], 1);
        assert_eq!(v["tsMs"], 1_729_000_000_000_u64);
        assert_eq!(v["host"], "desktop");
        assert_eq!(v["cpu"]["usagePct"], 23.5);
        // 注意 serde camelCase 把 freq_mhz 转成 freqMhz（与主快照同款），不是 freqMHz
        assert_eq!(v["cpu"]["freqMhz"], 2900);
        assert_eq!(v["cpu"]["tempC"], 61.5);
        assert_eq!(v["gpu"]["name"], "NVIDIA GeForce RTX 3060 Laptop GPU");
        assert_eq!(v["gpu"]["powerW"], 75.5);
        assert_eq!(v["bat"]["rateW"], 45.2);
        assert!(v["bat"]["charging"].as_bool().unwrap());
        assert_eq!(v["bat"]["secondsToFull"], 3_600_u64);
        assert!(
            v["bat"]["secondsToEmpty"].is_null(),
            "充电中不给剩余续航"
        );
        assert_eq!(v["net"]["downBps"], 1_234_567_u64);
        assert_eq!(v["net"]["totalReceivedMiB"], 12_000_u64);
        assert_eq!(v["net"]["totalTransmittedMiB"], 2_000_u64);
        assert_eq!(v["disk"]["readBps"], 300_000_u64);
    }

    /// cores 组：整数数组（round 而非 truncate——45.5 → 46）；procs 组：top N、
    /// 名字按**字符**截 16 个（多字节字符不能拦腰截断成非法 UTF-8）
    #[test]
    fn payload_cores_and_procs_shape() {
        let s = sample_snapshot();
        let all = build_payload(
            &s,
            &METRIC_KEYS.iter().map(|k| k.to_string()).collect::<Vec<_>>(),
        );
        let v: serde_json::Value = serde_json::to_value(&all).unwrap();
        assert_eq!(v["cores"]["load"].as_array().unwrap().len(), 16);
        assert_eq!(v["cores"]["load"][0], 12);
        assert_eq!(v["cores"]["load"][1], 46);
        assert_eq!(v["cores"]["load"][2], 100);
        let list = v["procs"]["list"].as_array().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0]["name"], "pulse-deck.exe");
        assert_eq!(list[0]["cpuPct"], 12.5);
        assert_eq!(list[0]["memMiB"], 36);
        let long_name = list[1]["name"].as_str().unwrap();
        assert_eq!(long_name.chars().count(), 16, "名字截到 16 个字符");
        assert_eq!(list[1]["memMiB"], 500, "内存换算成 MiB 取整");
    }

    /// per_core 还是空的（首拍等场景）→ cores 组整组缺席，不出现空组
    #[test]
    fn payload_omits_empty_cores() {
        let s = Snapshot::default();
        let p = build_payload(&s, &["cores".to_string()]);
        assert!(p.cores.is_none());
        let json = serde_json::to_string(&p).unwrap();
        assert!(!json.contains("\"cores\""));
    }

    #[test]
    fn truncate_chars_keeps_valid_utf8() {
        assert_eq!(truncate_chars("short", 16), "short");
        let n = "名字截断必须按字符".to_string();
        assert_eq!(truncate_chars(&n, 4), "名字截断");
        // 截断点落在多字节字符中间也不产生非法 UTF-8（按字符取天然保证）
        let mixed = "aé漢字b";
        let cut = truncate_chars(mixed, 3);
        assert_eq!(cut, "aé漢");
    }

    /// 蓝牙按 20 字节帧切发，单拍载荷必须有预算上限——加字段时这里是闸门
    #[test]
    fn payload_with_all_groups_stays_small() {
        let s = sample_snapshot();
        let all = build_payload(
            &s,
            &METRIC_KEYS.iter().map(|k| k.to_string()).collect::<Vec<_>>(),
        );
        let json = serde_json::to_string(&all).unwrap();
        assert!(
            json.len() < 1024,
            "全组载荷 {} 字节，超出 1KB 预算（BLE 帧数会失控）",
            json.len()
        );
    }

    /// 无电池 / 无 GPU 的机器不出现空组（设备端不用判空）
    #[test]
    fn payload_omits_unavailable_groups() {
        let s = Snapshot::default();
        let all = build_payload(
            &s,
            &METRIC_KEYS.iter().map(|k| k.to_string()).collect::<Vec<_>>(),
        );
        let json = serde_json::to_string(&all).unwrap();
        assert!(!json.contains("\"gpu\""), "无 GPU 不出 gpu 组");
        assert!(!json.contains("\"bat\""), "无电池不出 bat 组");
        assert!(json.contains("\"cpu\""));
    }

    /// 放电时 rateW 取放电功率（整机电耗）；两者都没有就是没有
    #[test]
    fn payload_battery_rate_prefers_direction() {
        let mut s = sample_snapshot();
        s.battery.charging = false;
        s.battery.ac_online = false;
        s.battery.charge_rate_w = None;
        s.battery.discharge_rate_w = Some(30.1);
        let p = build_payload(&s, &["bat".to_string()]);
        assert_eq!(p.bat.unwrap().rate_w, Some(30.1));
    }

    /// 已接入未充电（满电续充 / 养护模式）如实给 0：与 PC 电源卡同口径，
    /// 否则 App 端拿到 None 会把「电池」明细行整行隐藏，和桌面预览对不上
    #[test]
    fn payload_battery_rate_zero_when_ac_idle() {
        let mut s = sample_snapshot();
        s.battery.charging = false;
        s.battery.ac_online = true;
        s.battery.charge_rate_w = None;
        s.battery.discharge_rate_w = None;
        let p = build_payload(&s, &["bat".to_string()]);
        assert_eq!(p.bat.unwrap().rate_w, Some(0.0));
    }

    /* ---------- 静态信息消息（schema 3） ---------- */

    /// info 消息的形状：schema/type、截断（brand 36 / 挂载点 16 / 其它显卡 16）、
    /// 取整（整数 Wh / GiB / 整数百分比）、逐卷排序与 cap、其它显卡 cap
    #[test]
    fn host_info_shape_caps_and_rounding() {
        let mut s = sample_snapshot();
        s.gpu.devices.push(crate::model::GpuDevice {
            index: 2,
            name: "AMD Radeon RX 6800 Extra Long Name".into(),
            ..Default::default()
        });
        s.disks = ["A:\\", "F:\\", "B:\\", "E:\\", "C:\\", "D:\\"]
            .iter()
            .enumerate()
            .map(|(i, m)| crate::model::DiskInfo {
                mount_point: (*m).to_string(),
                total_bytes: 500 * GIB + i as u64 * GIB,
                used_bytes: 100 * GIB,
                usage_pct: 20.4 + i as f32,
                ..Default::default()
            })
            .collect();
        let all: Vec<String> = METRIC_KEYS.iter().map(|k| k.to_string()).collect();
        let v: serde_json::Value = serde_json::to_value(build_host_info(&s, &all)).unwrap();

        assert_eq!(v["schema"], 3);
        assert_eq!(v["type"], "info");
        assert_eq!(
            v["cpu"]["brand"].as_str().unwrap().chars().count(),
            INFO_BRAND_CHARS,
            "brand 截到 {} 字符",
            INFO_BRAND_CHARS
        );
        assert_eq!(v["cpu"]["physicalCores"], 14);
        assert_eq!(v["cpu"]["logicalCores"], 20);
        // 主卡（RTX 3060）显存总量；others = Intel 核显 + AMD，各截 16 字符
        assert_eq!(v["gpu"]["memTotalBytes"], 6 * GIB);
        let others = v["gpu"]["others"].as_array().unwrap();
        assert_eq!(others.len(), INFO_OTHERS_CAP);
        assert_eq!(others[0].as_str().unwrap().chars().count(), 16);
        // Wh 取整
        assert_eq!(v["bat"]["remainingWh"], 67);
        assert_eq!(v["bat"]["fullChargeWh"], 82);
        // 逐卷：按挂载点排序、cap 4、pct/GiB 取整
        let vols = v["disk"]["volumes"].as_array().unwrap();
        assert_eq!(vols.len(), INFO_VOLUME_CAP);
        assert_eq!(vols[0]["mount"], "A:\\");
        assert_eq!(vols[3]["mount"], "D:\\");
        assert_eq!(vols[0]["pct"], 20);
        assert_eq!(vols[0]["usedGiB"], 100);
        assert_eq!(vols[0]["totalGiB"], 500);
    }

    /// info 的组跟随 metrics 勾选：没勾的组整组缺席（与每拍载荷同款语义）
    #[test]
    fn host_info_respects_metric_selection() {
        let s = sample_snapshot();
        let only_mem = build_host_info(&s, &["mem".to_string()]);
        let v: serde_json::Value = serde_json::to_value(only_mem).unwrap();
        assert!(v["cpu"].is_null());
        assert!(v["gpu"].is_null());
        assert!(v["bat"].is_null());
        assert!(v["disk"].is_null());
        // 无电池 / 无 GPU 的机器同样不出空组
        let empty = Snapshot::default();
        let v2: serde_json::Value =
            serde_json::to_value(build_host_info(&empty, &["gpu".into(), "bat".into()])).unwrap();
        assert!(v2["gpu"].is_null());
        assert!(v2["bat"].is_null());
        assert_eq!(v2["schema"], 3, "没有组时也仍是合法的 info 消息");
    }

    /// 最坏情况每拍载荷闸门：全组 + 5 条 16 字符进程 + 24 核 + 15 字符主机名
    /// （Windows NetBIOS 上限）+ 处处最长数字，仍必须 <1KB（BLE 帧数预算）。
    /// 预算装不下而砍掉的：内存「可用」（sysinfo 口径 total−used 即精确值，
    /// 设备端自己算）、网络累计的字节精度（MiB 取整）——加字段时这里是闸门，超了继续砍。
    #[test]
    fn payload_worst_case_stays_under_ble_budget() {
        let mut s = sample_snapshot();
        s.system.host_name = Some("workstation-host".into());
        s.cpu.per_core = vec![100.0; 24];
        s.cpu.top_cpu = (0..5)
            .map(|i| crate::model::ProcInfo {
                pid: 10_000 + i,
                name: "procname-16-chars!".into(),
                cpu_usage: 99.9,
                memory_bytes: 2_048 * 1024 * 1024,
            })
            .collect();
        s.memory.total_bytes = 137_438_953_472;
        s.memory.used_bytes = 68_719_476_736;
        s.net.total_received = 123_456_789_012_345;
        s.net.total_transmitted = 12_345_678_901_234;
        let all: Vec<String> = METRIC_KEYS.iter().map(|k| k.to_string()).collect();
        let json = serde_json::to_string(&build_payload(&s, &all)).unwrap();
        assert!(
            json.len() < 1024,
            "最坏载荷 {} 字节，超出 1KB 预算（BLE 帧数会失控）",
            json.len()
        );
    }

    /* ---------- BLE 帧 ---------- */

    #[test]
    fn ble_frames_roundtrip_various_sizes() {
        for size in [1usize, 18, 19, 37, 200, 750] {
            let payload: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
            let frames = ble_frames(&payload);
            assert!(!frames.is_empty());
            for f in &frames {
                assert!(f.len() <= 20, "单帧不得超过 20 字节");
            }
            assert_eq!(frames.len(), size.div_ceil(BLE_FRAME_DATA));
            assert_eq!(
                ble_reassemble(&frames).as_deref(),
                Some(&payload[..]),
                "{size} 字节载荷切帧后必须能无损重组"
            );
        }
    }

    #[test]
    fn ble_reassemble_rejects_broken_sequences() {
        let payload: Vec<u8> = (0..100).collect();
        let frames = ble_frames(&payload);
        // 缺一帧
        assert!(ble_reassemble(&frames[..frames.len() - 1]).is_none());
        // total 不一致
        let mut bad_total = frames.clone();
        bad_total[0][1] += 1;
        assert!(ble_reassemble(&bad_total).is_none());
        // idx 越界
        let mut bad_idx = frames.clone();
        bad_idx[0][0] = 200;
        assert!(ble_reassemble(&bad_idx).is_none());
        // 空序列
        assert!(ble_reassemble(&[]).is_none());
    }

    /* ---------- 设置页一致性 ---------- */

    /// 指标键与通道键在几处必须一致：Rust 白名单、设置页 chip 的 value。
    /// 少一处就是「勾了存不下去 / 存了推不出来」的半坏状态。
    #[test]
    fn metric_and_transport_keys_exist_in_settings_html() {
        let html = include_str!("../../../index.html");
        // 按字符截窗口（HTML 里有中文，字节切片会切进多字节字符中间）
        let zone = |anchor: &str| {
            let start = html.find(anchor).unwrap_or(usize::MAX);
            html[start..].chars().take(1400).collect::<String>()
        };
        let metrics_zone = zone("id=\"push-metrics\"");
        for key in METRIC_KEYS {
            assert!(
                metrics_zone.contains(&format!("value=\"{key}\"")),
                "设置页推送指标 chips 缺少 {key}"
            );
        }
        let transports_zone = zone("id=\"push-transports\"");
        for key in ["tcp", "ble"] {
            assert!(
                transports_zone.contains(&format!("value=\"{key}\"")),
                "设置页推送通道 chips 缺少 {key}"
            );
        }
    }

    /* ---------- 协议分流 ---------- */

    #[test]
    fn http_prefix_detection() {
        assert!(looks_like_http(b"GET / HTTP/1.1\r\nHost: x"));
        assert!(looks_like_http(b"PUT /x HTTP/1.1"));
        assert!(looks_like_http(b"CONNECT host:443"));
        assert!(!looks_like_http(b"{\"schema\":1}"), "NDJSON 客户端不巧发了数据也不是 HTTP");
        assert!(!looks_like_http(b"GET"), "不足 4 字节判不出来（嗅探会继续等）");
        assert!(!looks_like_http(b""));
        assert!(!looks_like_http(b"get / HTTP/1.1"), "方法名大小写敏感（HTTP 规范）");
    }

    /* ---------- TCP / WebSocket 端到端 ---------- */

    /// 等监听就位并返回实际端口（port=0 交内核选临时端口，避开端口冲突）
    fn wait_listening(hub: &PushHub) -> u16 {
        for _ in 0..50 {
            let st = hub.status();
            if st.tcp_listening {
                return st
                    .tcp_addr
                    .rsplit(':')
                    .next()
                    .and_then(|p| p.parse::<u16>().ok())
                    .expect("地址里必须有端口");
            }
            assert!(st.tcp_error.is_empty(), "监听不该失败：{}", st.tcp_error);
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("500ms 内必须监听就位");
    }

    fn wait_clients(hub: &PushHub, want: usize) {
        for _ in 0..20 {
            if hub.status().tcp_clients == want {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("状态里应能看到 {want} 台已连接客户端");
    }

    fn e2e_config() -> PushConfig {
        PushConfig {
            enabled: true,
            tcp_enabled: true,
            ble_enabled: false,
            port: 0,
            interval_ms: 300,
            metrics: default_metrics(),
        }
    }

    /// 让内核选一个当前空闲的端口：测试要**复用同一个固定端口**（重启不换端口
    /// 才是出问题的那条路），port=0 每代都换端口，撞不上这个竞态。
    fn free_port() -> u16 {
        let l = TcpListener::bind(("0.0.0.0", 0)).expect("拿到空闲端口");
        l.local_addr().expect("内核给了地址").port() // drop 即释放
    }

    /// 同一端口上的「停旧起新」不能失败：旧 worker 从看到停止位到真正关闭监听 socket
    /// 有一个轮询节拍的延迟（100ms），新一代若抢在这之前 bind，Windows 会以
    /// 10048（每个地址只允许用一次）拒绝——状态行于是显示「监听失败」，而端口其实是空的。
    /// 应用启动时必走这条路：lib.rs 先按 push.json sync 一代，前端页面起来后又把
    /// localStorage 里的同一份配置推给 set_push_config（等于再 sync 一次）。
    #[test]
    fn restart_on_the_same_port_keeps_listening() {
        let mut cfg = e2e_config();
        cfg.port = free_port();
        let sampler = Arc::new(crate::sampler::Sampler::new());
        let running = Arc::new(AtomicBool::new(true));
        let hub = PushHub::new(sampler, running.clone());

        hub.sync(&cfg);
        let port = wait_listening(&hub);
        assert_eq!(port, cfg.port, "用的是我们指定的端口");

        // 紧接着重启（前端推配置就是这么近）
        hub.sync(&cfg);
        assert_eq!(wait_listening(&hub), cfg.port, "新一代接管同一个端口");

        running.store(false, Ordering::Relaxed);
        hub.sync(&PushConfig::default());
    }

    /// 端口还被占着时（上一代卡在写超时 / BLE 交接里，交接等待超时了）要**小步重试**，
    /// 而不是立刻把 10048 摆到状态行上——占位者往往几十毫秒后就撒手了。
    /// 窗口过后仍未绑上才如实报错（真被别的程序长期占着时不该假装没事）。
    #[test]
    fn bind_listener_retries_until_the_squatter_releases() {
        let squatter = TcpListener::bind(("0.0.0.0", 0)).expect("占住一个端口");
        let port = squatter.local_addr().expect("内核给了地址").port();

        let binder = std::thread::spawn(move || bind_listener(port).map(|l| l.local_addr()));
        std::thread::sleep(Duration::from_millis(300));
        assert!(!binder.is_finished(), "窗口内应还在重试，而不是已经失败退出");

        drop(squatter);
        let bound = binder
            .join()
            .expect("绑定线程不 panic")
            .expect("占位者撒手后应绑上")
            .expect("绑上的地址可读");
        assert_eq!(bound.port(), port, "绑的是同一个端口");

        // 一直占着不放：窗口用尽后必须回错误，不能无限重试
        let held = TcpListener::bind(("0.0.0.0", port)).expect("重新占住");
        let err = bind_listener(port).expect_err("端口被长期占用时应如实报错");
        assert_eq!(err.kind(), std::io::ErrorKind::AddrInUse);
        drop(held);
    }

    /// 端到端往返（裸 TCP）：起 hub（临时端口）→ 客户端连上 → 收到合法 JSON 行
    /// → 状态可见。监听、accept、协议分流、组载荷、扇出、掉线剔除全链路都在内。
    #[test]
    fn tcp_push_roundtrip() {
        use std::io::BufRead;

        let sampler = Arc::new(crate::sampler::Sampler::new());
        let running = Arc::new(AtomicBool::new(true));
        let hub = PushHub::new(sampler, running.clone());
        hub.sync(&e2e_config());
        let port = wait_listening(&hub);

        // 裸 TCP 客户端连上后一言不发（NDJSON 客户端的常态，也是嗅探判定的前提）；
        // 首拍会被 250ms 判定窗口延后，5s 读超时内稳过。连接后先到的可能是
        // 静态信息 / 布局消息（schema 2/3）——读到第一条数据拍（schema 1）为止
        let stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("连上推送服务");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut reader = std::io::BufReader::new(stream);

        let mut line = String::new();
        loop {
            line.clear();
            reader.read_line(&mut line).expect("读到一行推送");
            if line.contains("\"schema\":1") {
                break;
            }
        }
        assert!(line.ends_with('\n'), "NDJSON 帧以换行结尾");
        let v: serde_json::Value = serde_json::from_str(line.trim()).expect("推送行是合法 JSON");
        assert_eq!(v["schema"], 1);
        assert!(v["tsMs"].is_number(), "载荷带采样时刻");
        assert!(v["cpu"].is_object(), "默认指标集含 CPU");

        // 状态行能看到这台客户端
        wait_clients(&hub, 1);

        // 收工：置停止位，worker 在 100ms 内退出
        running.store(false, Ordering::Relaxed);
        hub.sync(&PushConfig::default());
    }

    /// 端到端往返（WebSocket）：tungstenite 客户端连临时端口 → 握手成功 →
    /// 收到一条文本消息，内容与 NDJSON 通道同一份 JSON（无换行）→ 状态行
    /// 把 WS 客户端计入 tcpClients。
    #[test]
    fn ws_push_roundtrip() {
        let sampler = Arc::new(crate::sampler::Sampler::new());
        let running = Arc::new(AtomicBool::new(true));
        let hub = PushHub::new(sampler, running.clone());
        hub.sync(&e2e_config());
        let port = wait_listening(&hub);

        // 用裸 TcpStream 建客户端（connect() 的 TLS 兼容包装流拿不到 set_read_timeout）
        let stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("连上推送服务");
        let (mut ws, _resp) =
            tungstenite::client(format!("ws://127.0.0.1:{port}/"), stream).expect("WS 握手成功");
        let _ = ws.get_ref().set_read_timeout(Some(Duration::from_secs(5)));

        // 首条可能是静态信息 / 布局消息（schema 2/3）——读到第一条数据拍为止
        let mut text = String::new();
        loop {
            text = ws.read().expect("收到一条推送").into_text().unwrap().to_string();
            if text.contains("\"schema\":1") {
                break;
            }
        }
        assert!(!text.contains('\n'), "WS 文本消息不带换行");
        let v: serde_json::Value = serde_json::from_str(&text).expect("推送是合法 JSON");
        assert_eq!(v["schema"], 1);
        assert!(v["tsMs"].is_number(), "载荷带采样时刻");
        assert!(v["cpu"].is_object(), "默认指标集含 CPU");

        wait_clients(&hub, 1);

        running.store(false, Ordering::Relaxed);
        hub.sync(&PushConfig::default());
    }

    /// 同一端口双协议并存：裸 TCP 与 WebSocket 客户端同时连，各自按各自的
    /// 帧格式收到推送，tcpClients 合计 2——分流不误伤是本功能的核心回归。
    #[test]
    fn tcp_and_ws_clients_share_the_same_port() {
        use std::io::BufRead;

        let sampler = Arc::new(crate::sampler::Sampler::new());
        let running = Arc::new(AtomicBool::new(true));
        let hub = PushHub::new(sampler, running.clone());
        hub.sync(&e2e_config());
        let port = wait_listening(&hub);

        // 先连 WS（握手即时到达，判定窗口毫秒级通过），再连裸 TCP（等窗口超时）
        let ws_stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("连上推送服务");
        let (mut ws, _resp) = tungstenite::client(format!("ws://127.0.0.1:{port}/"), ws_stream)
            .expect("WS 握手成功");
        let _ = ws.get_ref().set_read_timeout(Some(Duration::from_secs(5)));

        let raw = std::net::TcpStream::connect(("127.0.0.1", port)).expect("连上推送服务");
        raw.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut reader = std::io::BufReader::new(raw);

        // 首行可能是静态信息（schema 3）——跳过它取第一条数据拍
        let mut line = String::new();
        loop {
            line.clear();
            reader.read_line(&mut line).expect("裸 TCP 收到 NDJSON 行");
            if line.contains("\"schema\":1") {
                break;
            }
        }
        let mut text = String::new();
        loop {
            text = ws.read().expect("WS 收到文本消息").into_text().unwrap().to_string();
            if text.contains("\"schema\":1") {
                break;
            }
        }

        let v_line: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        let v_ws: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v_line["schema"], 1);
        assert_eq!(v_ws["schema"], 1);
        assert!(v_ws["cpu"].is_object(), "WS 收到的也是同一份载荷");

        wait_clients(&hub, 2);

        running.store(false, Ordering::Relaxed);
        hub.sync(&PushConfig::default());
    }

    /// 布局消息（schema 2）的端到端：连接建立后两种设备形态（pad/phone）各必达
    /// 一条且 pad 在前（旧版 App 不认 device 字段，后者覆盖前者最终停在 phone）、
    /// `set_layout` 变更后在读超时窗口内重发新全量。TCP 侧验证（WS 走同一
    /// 个扇出函数、BLE 走同一条 push 路径，无线电部分按仓库惯例真机验收）。
    #[test]
    fn layout_message_reaches_client_and_resends_on_change() {
        use std::io::BufRead;

        let sampler = Arc::new(crate::sampler::Sampler::new());
        let running = Arc::new(AtomicBool::new(true));
        let hub = PushHub::new(sampler, running.clone());
        hub.set_layout(
            crate::mobile_layout::DEVICE_PAD,
            Some(
                r#"{"schema":2,"type":"layout","device":"pad","cols":6,"widgets":[{"type":"cpu","x":0,"y":0,"w":3,"h":3}]}"#
                    .into(),
            ),
        );
        hub.set_layout(
            crate::mobile_layout::DEVICE_PHONE,
            Some(
                r#"{"schema":2,"type":"layout","device":"phone","cols":4,"widgets":[{"type":"cpu","x":0,"y":0,"w":2,"h":3}]}"#
                    .into(),
            ),
        );
        hub.sync(&e2e_config());
        let port = wait_listening(&hub);

        let stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("连上推送服务");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut reader = std::io::BufReader::new(stream);

        // 连接后前几行里必须同时见到两条布局消息（pad 在 phone 前）与数据拍
        // （与数据拍的相对顺序不保证：布局走 100ms 快查、首拍还带 250ms 嗅探窗口）
        let (mut saw_pad, mut saw_phone, mut phone_after_pad, mut saw_tick) =
            (false, false, false, false);
        for _ in 0..8 {
            let mut line = String::new();
            reader.read_line(&mut line).expect("读到一行");
            if line.contains("\"device\":\"pad\"") {
                saw_pad = true;
                assert!(line.contains("\"cols\":6"), "pad 布局带自己的列数");
            } else if line.contains("\"device\":\"phone\"") {
                saw_phone = true;
                phone_after_pad = saw_pad;
            } else if line.contains("\"schema\":1") {
                saw_tick = true;
            }
            if saw_pad && saw_phone && saw_tick {
                break;
            }
        }
        assert!(saw_pad, "连接建立后必达 pad 布局消息");
        assert!(saw_phone, "连接建立后必达 phone 布局消息");
        assert!(phone_after_pad, "phone 布局必须在 pad 之后（旧 App 兼容顺序）");
        assert!(saw_tick, "数据拍照常推送");

        // 变更即重发：换了内容（同内容会被跳过）后读超时窗口内必须再收到。
        // 改任一设备都会两条一起重发，这里改 phone 验证新内容到达
        hub.set_layout(
            crate::mobile_layout::DEVICE_PHONE,
            Some(
                r#"{"schema":2,"type":"layout","device":"phone","cols":4,"widgets":[]}"#
                    .into(),
            ),
        );
        let mut got_new = false;
        for _ in 0..10 {
            let mut line = String::new();
            reader.read_line(&mut line).expect("读到一行");
            if line.contains("\"device\":\"phone\"") && line.contains("\"widgets\":[]") {
                got_new = true;
                break;
            }
        }
        assert!(got_new, "布局变更后必须重发全量");

        running.store(false, Ordering::Relaxed);
        hub.sync(&PushConfig::default());
    }

    /// 静态信息（schema 3）端到端：连接建立后必达，且**先于**两条布局消息
    /// （App 要先拿到型号 / 逐卷容量这类静态数据，随后渲染布局卡片才不缺行）。
    #[test]
    fn info_message_precedes_layouts_on_connect() {
        use std::io::BufRead;

        let sampler = Arc::new(crate::sampler::Sampler::new());
        let running = Arc::new(AtomicBool::new(true));
        let hub = PushHub::new(sampler, running.clone());
        hub.set_layout(
            crate::mobile_layout::DEVICE_PAD,
            Some(r#"{"schema":2,"type":"layout","device":"pad","cols":6,"widgets":[]}"#.into()),
        );
        hub.set_layout(
            crate::mobile_layout::DEVICE_PHONE,
            Some(r#"{"schema":2,"type":"layout","device":"phone","cols":4,"widgets":[]}"#.into()),
        );
        hub.sync(&e2e_config());
        let port = wait_listening(&hub);

        let stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("连上推送服务");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut reader = std::io::BufReader::new(stream);

        // 连接后前几行内记下各自出现的行序，info 必须排在两条布局之前
        let (mut info_at, mut pad_at, mut phone_at) = (usize::MAX, usize::MAX, usize::MAX);
        for i in 0..8 {
            let mut line = String::new();
            reader.read_line(&mut line).expect("读到一行");
            if line.contains("\"schema\":3") {
                info_at = info_at.min(i);
            } else if line.contains("\"device\":\"pad\"") {
                pad_at = pad_at.min(i);
            } else if line.contains("\"device\":\"phone\"") {
                phone_at = phone_at.min(i);
            }
            if info_at != usize::MAX && pad_at != usize::MAX && phone_at != usize::MAX {
                break;
            }
        }
        assert_ne!(info_at, usize::MAX, "连接建立后必达 info 消息");
        assert_ne!(pad_at, usize::MAX);
        assert_ne!(phone_at, usize::MAX);
        assert!(
            info_at < pad_at && info_at < phone_at,
            "info 必须先于两条布局消息（实际行序 info={info_at} pad={pad_at} phone={phone_at}）"
        );

        running.store(false, Ordering::Relaxed);
        hub.sync(&PushConfig::default());
    }
}
