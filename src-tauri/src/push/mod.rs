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
//! worker 是裸 OS 线程（与采样线程同款 100ms 步进睡法），同时受全局
//! `running` 与自身代际 stop 控制，应用退出时一并收尾。
//!
//! 每拍**有连接才干活**：`sampler.latest()` 拉最后一次快照 → 组装载荷 →
//! 序列化一次 → 扇出给所有 TCP 客户端与 BLE 订阅者；无连接时零开销。

mod ble;
mod ws;

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::model::Snapshot;

/// 配置文件名（放在应用数据目录下）
const FILE_NAME: &str = "push.json";

/// 可推送的指标组（与设置页 chip 的 value 一致）
pub const METRIC_KEYS: [&str; 6] = ["cpu", "mem", "gpu", "bat", "net", "disk"];

/// 默认推送：CPU / 内存 / GPU / 网络（温度随 CPU 组、电池组默认不开——台式机没有）
const DEFAULT_METRICS: [&str; 4] = ["cpu", "mem", "gpu", "net"];

/// TCP 默认监听端口（避开常用服务段，且好记）
pub const DEFAULT_PORT: u16 = 17432;

/// 推送间隔的合理区间（与主面板采样间隔同一范围）
pub const MIN_INTERVAL_MS: u32 = 300;
pub const MAX_INTERVAL_MS: u32 = 10_000;
const DEFAULT_INTERVAL_MS: u32 = 1000;

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
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PushMem {
    pub usage_pct: f64,
    pub used_bytes: u64,
    pub total_bytes: u64,
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
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PushNet {
    pub down_bps: u64,
    pub up_bps: u64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PushDisk {
    pub read_bps: u64,
    pub write_bps: u64,
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
        }),
        bat: (has("bat") && s.battery.has_battery).then(|| PushBat {
            pct: s.battery.percent.filter(finite).map(round1),
            charging: s.battery.charging,
            ac_online: s.battery.ac_online,
            rate_w: if s.battery.charging {
                s.battery.charge_rate_w
            } else {
                s.battery.discharge_rate_w
            }
            .filter(finite)
            .map(round1),
        }),
        net: has("net").then(|| PushNet {
            down_bps: s.net.rx_bytes_per_sec,
            up_bps: s.net.tx_bytes_per_sec,
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
    }
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
}

struct HubInner {
    /// 当前一代 worker 的停止位；没有在跑的 worker 时为 None
    stop: Option<Arc<AtomicBool>>,
    _gen: AtomicU64,
}

impl PushHub {
    pub fn new(sampler: Arc<crate::sampler::Sampler>, app_running: Arc<AtomicBool>) -> Self {
        Self {
            sampler,
            app_running,
            inner: Mutex::new(HubInner {
                stop: None,
                _gen: AtomicU64::new(0),
            }),
            status: Mutex::new(Arc::new(Mutex::new(PushStatus::default()))),
        }
    }

    /// 按配置重启推送子系统：先停旧 worker，`enabled` 时起新的一代。
    /// 注意这里**不做 sanitize**——`store.set()` 已经洗过，测试直连时自己保证合法性
    /// （port=0 是合法输入：交给内核选临时端口，端到端测试靠它避开端口冲突）。
    pub fn sync(&self, cfg: &PushConfig) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(stop) = inner.stop.take() {
            stop.store(true, Ordering::Relaxed);
        }
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
        let worker = Worker {
            cfg: cfg.clone(),
            stop,
            app_running: self.app_running.clone(),
            sampler: self.sampler.clone(),
            status: fresh,
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
}

impl Worker {
    fn alive(&self) -> bool {
        !self.stop.load(Ordering::Relaxed) && self.app_running.load(Ordering::Relaxed)
    }

    fn lock_status(&self) -> MutexGuard<'_, PushStatus> {
        self.status.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn run(self) {
        let mut listener: Option<TcpListener> = None;
        if self.cfg.tcp_enabled {
            match TcpListener::bind(("0.0.0.0", self.cfg.port)) {
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

            if !force_push && waited < interval {
                continue;
            }
            waited = 0;
            force_push = false;

            let ble_subs = ble_server
                .as_ref()
                .map(ble::BleServer::subscribers)
                .unwrap_or(0);
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
        s.cpu.usage_total = 23.5;
        s.cpu.freq_mhz = 2900;
        s.cpu.freq_live = true;
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
        s.battery.has_battery = true;
        s.battery.charging = true;
        s.battery.ac_online = true;
        s.battery.percent = Some(82.0);
        s.battery.charge_rate_w = Some(45.2);
        s.battery.discharge_rate_w = None;
        s.net.rx_bytes_per_sec = 1_234_567;
        s.net.tx_bytes_per_sec = 98_765;
        s.disks = vec![crate::model::DiskInfo {
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
        assert_eq!(v["net"]["downBps"], 1_234_567_u64);
        assert_eq!(v["disk"]["readBps"], 300_000_u64);
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
        let zone = |anchor: &str| {
            let start = html.find(anchor).unwrap_or(usize::MAX);
            &html[start..(start + 1400).min(html.len())]
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
        // 首拍会被 250ms 判定窗口延后，5s 读超时内稳过
        let stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("连上推送服务");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut reader = std::io::BufReader::new(stream);

        let mut line = String::new();
        reader.read_line(&mut line).expect("读到一行推送");
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

        let text = ws.read().expect("收到一条推送").into_text().unwrap().to_string();
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

        let mut line = String::new();
        reader.read_line(&mut line).expect("裸 TCP 收到 NDJSON 行");
        let text = ws.read().expect("WS 收到文本消息").into_text().unwrap().to_string();

        let v_line: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        let v_ws: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v_line["schema"], 1);
        assert_eq!(v_ws["schema"], 1);
        assert!(v_ws["cpu"].is_object(), "WS 收到的也是同一份载荷");

        wait_clients(&hub, 2);

        running.store(false, Ordering::Relaxed);
        hub.sync(&PushConfig::default());
    }
}
