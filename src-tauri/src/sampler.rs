//! 后台采样：所有数据在这里统一刷新，buf/命令只读取最后一次快照。

use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sysinfo::{DiskKind, Disks, Networks, ProcessesToUpdate, System};

use crate::model::{
    percent_of, BatteryInfo, CpuInfo, DiskInfo, GpuInfo, MemoryInfo, NetIface, NetInfo, ProcInfo,
    Snapshot, SystemInfo, ThermalInfo,
};
use crate::platform::{self, CpuExtrasReading, FanReport, GpuSource, MemoryExtras};

/// 进程榜单长度
const TOP_N: usize = 8;
/// sysinfo 两次 CPU 刷新之间的最小间隔（官方常量约 200ms）
const CPU_WARMUP_MS: u64 = 220;

/// 进程刷新口径：只要进程榜用到的「占用率 + 内存」。
///
/// 默认的 `System::refresh_processes` 会额外做三件本工具一件都用不上的事，而它们在
/// 大机器上非常贵（本机 i7-8750H / 3000+ 线程的桌面实测，单拍 70ms，占整拍耗时 3/4）：
///
/// * `tasks`：Linux 上把**每个线程**当成独立进程枚举（/proc/<pid>/task/*），
///   本机 425 个进程被展开成 3177 条，每条都要读 stat / statm 并留一份结构体；
/// * `disk_usage`：读每个进程的 /proc/<pid>/io（进程磁盘 IO 界面上没有这一项）；
/// * `exe`：读 /proc/<pid>/exe 链接（进程名来自 stat/status，用不到可执行路径）。
///
/// 结果：单拍从 70ms 降到 10ms 量级，顺带把常驻的 Process 条目和打开的 procfs 文件
/// 数量降到 1/7。⚠️ `nothing()` 的默认值里 `tasks` 仍是 **true**，必须显式关掉。
fn process_refresh_kind() -> sysinfo::ProcessRefreshKind {
    sysinfo::ProcessRefreshKind::nothing()
        .with_cpu()
        .with_memory()
        .without_tasks()
}

/// 采样分阶段计时：设置 `PULSEDECK_TIMING=1` 时把每阶段耗时打到 stderr。
///
/// 用途是「怀疑某一拍变贵了」时定位到具体数据源（当年就是靠它发现进程刷新占了 3/4 的
/// 采样耗时、NVMe 的 hwmon 一个芯片比其余芯片加起来还贵）。要拿有意义的数字得跑 release：
///
/// ```text
/// PULSEDECK_TIMING=1 cargo test --release --lib smoke::collect_once -- --nocapture
/// ```
macro_rules! stage {
    ($label:expr, $body:expr) => {{
        let __t = Instant::now();
        let __v = $body;
        if timing_on() {
            eprintln!(
                "    {:<22}{:>8.2} ms",
                $label,
                __t.elapsed().as_secs_f64() * 1000.0
            );
        }
        __v
    }};
}

/// 分阶段计时的开关。环境变量只读一次：采样线程每拍要问十几次，
/// 每次都去查环境（`getenv`）没必要。
fn timing_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("PULSEDECK_TIMING").is_some())
}

struct Inner {
    sys: System,
    disks: Disks,
    net: Networks,
    /// OS 绑定的采集通道（GPU / 温度 / 风扇 / 电池 / 内存附加），
    /// 实现按编译目标分派（Windows = 原生探针，其余 = 诚实占位），见 `platform`
    sources: platform::PlatformSources,
}

pub struct Sampler {
    inner: Mutex<Inner>,
    latest: Mutex<Snapshot>,
    last_tick: Mutex<Instant>,
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Sampler {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                sys: System::new_all(),
                disks: Disks::new_with_refreshed_list(),
                net: Networks::new_with_refreshed_list(),
                sources: platform::sources(),
            }),
            latest: Mutex::new(Snapshot::default()),
            last_tick: Mutex::new(Instant::now()),
        }
    }

    /// 注入风扇偏好句柄：设置页改了「CPU 风扇号」，采样线程必须立刻用上同一份。
    /// 不做注入的话，两边各自 `load()` 一次 `fan.json`，改动就只在设置页生效。
    pub fn set_fan_pref(&self, pref: crate::fan_pref::FanPrefStore) {
        self.inner.lock().unwrap().sources.fan.set_pref(pref);
    }

    /// 启动时预热：CPU 占用需要两次采样才有值，先跑一轮避免首屏全 0
    pub fn warmup(&self) {
        {
            let mut inner = self.inner.lock().unwrap();
            inner.sys.refresh_cpu_all();
        }
        std::thread::sleep(Duration::from_millis(CPU_WARMUP_MS));
        self.refresh();
    }

    pub fn refresh(&self) {
        let snap = self.collect();
        *self.latest.lock().unwrap() = snap;
        *self.last_tick.lock().unwrap() = Instant::now();
    }

    pub fn latest(&self) -> Snapshot {
        self.latest.lock().unwrap().clone()
    }

    fn collect(&self) -> Snapshot {
        let tick_start = Instant::now();
        // 两次采样之间的真实间隔，用于把增量换算成速率（比理论采样间隔更准）
        let elapsed_ms = {
            let last = self.last_tick.lock().unwrap();
            tick_start.duration_since(*last).as_millis() as u64
        };
        let elapsed_secs = (elapsed_ms as f64 / 1000.0).max(0.05);

        let mut inner = self.inner.lock().unwrap();
        let Inner {
            sys,
            disks,
            net,
            sources,
        } = &mut *inner;

        sys.refresh_cpu_all();
        stage!("refresh_memory", sys.refresh_memory());
        stage!(
            "refresh_processes",
            sys.refresh_processes_specifics(ProcessesToUpdate::All, true, process_refresh_kind())
        );
        stage!("disks.refresh", disks.refresh(true));
        stage!("net.refresh", net.refresh(true));

        let mut cpu = stage!("collect_cpu", collect_cpu(sys));
        // **实时**主频。Windows 要另外读（PDH `Actual Frequency`：sysinfo 只给标称频率）；
        // Linux 的 sysinfo 本来就是实时值、未适配平台如实报「没有」，都走同一个 trait。
        let cpu_extras = stage!("cpu_extras", sources.cpu_extras.sample());
        let memory = stage!("memory_extras", collect_memory(sys, &*sources.memory_extras));
        let disks_out = stage!("collect_disks", collect_disks(disks, elapsed_secs));
        let battery_info: BatteryInfo = stage!("battery", sources.battery.collect());
        let net_out = stage!("collect_net", collect_net(net, elapsed_secs));
        let mut gpu_out = stage!("gpu", collect_gpu(&mut *sources.gpu));
        let thermal_out: ThermalInfo = stage!("thermal", sources.thermal.sample());
        // NVML 拿不到温度时（Intel / AMD 显卡）回退到硬件监控软件的读数
        if gpu_out.temp_c.is_none() {
            gpu_out.temp_c = thermal_out.gpu_temp_c;
        }

        // 风扇：NVML/PDH 都没有时，用 WMI（OHM/旧版 LHM、ACPI）兜底。
        // 注意风扇读数与温度是两套独立通道，不能因为温度有值就假定风扇也有。
        let fans = stage!("fan", sources.fan.sample(gpu_out.devices.len()));
        apply_cpu_fan(&mut cpu, &fans);
        apply_cpu_extras(&mut cpu, &cpu_extras);
        apply_gpu_fan(&mut gpu_out, &fans);

        let snap = Snapshot {
            ts_ms: now_ms(),
            sample_interval_ms: elapsed_ms,
            system: stage!("collect_system", collect_system(sys)),
            cpu,
            memory,
            disks: disks_out,
            battery: battery_info,
            gpu: gpu_out,
            net: net_out,
            thermal: thermal_out,
        };
        if timing_on() {
            eprintln!(
                "    {:<22}{:>8.2} ms  (总耗时)\n",
                "collect",
                tick_start.elapsed().as_secs_f64() * 1000.0
            );
        }
        snap
    }
}

/* ------------------------------ 各维度采集 ------------------------------ */

fn collect_system(sys: &System) -> SystemInfo {
    SystemInfo {
        host_name: System::host_name(),
        os_name: System::name(),
        os_version: System::os_version(),
        long_os_version: System::long_os_version(),
        kernel_version: System::kernel_version(),
        cpu_arch: System::cpu_arch(),
        uptime_secs: System::uptime(),
        boot_time_secs: System::boot_time(),
        process_count: sys.processes().len(),
    }
}

fn collect_cpu(sys: &System) -> CpuInfo {
    let cpus = sys.cpus();
    let brand = cpus
        .first()
        .map(|c| c.brand().trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| cpus.first().map(|c| c.name().to_string()))
        .unwrap_or_else(|| "未知处理器".to_string());

    let per_core: Vec<f32> = cpus.iter().map(|c| c.cpu_usage()).collect();
    let freq_mhz = cpus.iter().map(|c| c.frequency()).max().unwrap_or(0);

    // sysinfo 的 Process::cpu_usage() 是「每核 100%」口径（单进程跑满一核 = 100%，多线程可超 100%），
    // 与全局 usage_total 的「全核合计 100%」口径不一致。这里除以逻辑核心数，换算成
    // 「占总 CPU 算力」口径，使进程百分比与全局 CPU 一致、也与任务管理器默认列一致。
    let logical_cores_f = cpus.len().max(1) as f32;

    let processes: Vec<ProcInfo> = sys
        .processes()
        .values()
        .map(|p| ProcInfo {
            pid: p.pid().as_u32() as usize,
            name: p.name().to_string_lossy().to_string(),
            cpu_usage: p.cpu_usage() / logical_cores_f,
            memory_bytes: p.memory(),
        })
        .collect();

    let mut top_cpu = processes.clone();
    top_cpu.sort_by(|a, b| b.cpu_usage.partial_cmp(&a.cpu_usage).unwrap_or(std::cmp::Ordering::Equal));
    top_cpu.truncate(TOP_N);

    let mut top_mem = processes;
    top_mem.sort_by(|a, b| b.memory_bytes.cmp(&a.memory_bytes));
    top_mem.truncate(TOP_N);

    let mut info = CpuInfo {
        brand,
        usage_total: sys.global_cpu_usage(),
        logical_cores: cpus.len(),
        physical_cores: System::physical_core_count().unwrap_or(cpus.len()),
        per_core,
        freq_mhz,
        // 主频的「实时与否」由 apply_cpu_extras 在采样后期按平台通道补齐
        freq_live: false,
        // 风扇由 fan.rs 在采样后期补齐（WMI 查询慢、且独立于 CPU 采集）
        fan_rpm: None,
        fan_pct: None,
        fan_source: String::new(),
        top_cpu,
        top_mem,
    };
    info.normalize();
    info
}

/// 把风扇报告里的 CPU 部分写进 `CpuInfo`
fn apply_cpu_fan(cpu: &mut CpuInfo, fans: &FanReport) {
    cpu.fan_rpm = fans.cpu.rpm;
    cpu.fan_pct = fans.cpu.pct;
    cpu.fan_source = fans.cpu.source.clone();
}

/// 把平台侧的 CPU 附加读数写进 `CpuInfo`（**实时**主频）
///
/// 取舍：平台能给实时值就用实时值（Windows 走 PDH `Actual Frequency`，与任务管理器的
/// 「速度」同量级）；给不出来时保留 sysinfo 的值，但**如实标注它不是实时读数**
/// （Windows 的 sysinfo 主频是恒定标称值），界面据此写「基准主频」。
fn apply_cpu_extras(cpu: &mut CpuInfo, extras: &CpuExtrasReading) {
    match extras.live_freq_mhz {
        Some(mhz) => {
            cpu.freq_mhz = mhz as u64;
            cpu.freq_live = true;
        }
        None => cpu.freq_live = extras.sysinfo_freq_live,
    }
}

/// 把风扇报告里的 GPU 部分写进 `GpuInfo` 与各设备（仅在原生通道没给值时才覆盖）
fn apply_gpu_fan(gpu: &mut GpuInfo, fans: &FanReport) {
    for (i, dev) in gpu.devices.iter_mut().enumerate() {
        let Some(r) = fans.gpus.get(i) else { continue };
        if dev.fan_rpm.is_none() {
            dev.fan_rpm = r.rpm;
        }
        if dev.fan_pct.is_none() {
            dev.fan_pct = r.pct;
        }
    }
    // 汇总口径与 aggregate 一致：取第一个真正有风扇读数的设备。
    // （aggregate 已按主卡填过一遍，这里只补它没填到的情形。）
    let Some(m) = gpu
        .devices
        .iter()
        .find(|d| d.fan_rpm.is_some() || d.fan_pct.is_some())
    else {
        return;
    };
    if gpu.fan_rpm.is_none() {
        gpu.fan_rpm = m.fan_rpm;
    }
    if gpu.fan_pct.is_none() {
        gpu.fan_pct = m.fan_pct;
    }
    if gpu.fan_source.is_empty() {
        gpu.fan_source = if m.source == "nvml" {
            "NVIDIA 驱动".to_string()
        } else {
            fans.gpus
                .iter()
                .find(|r| r.any())
                .map(|r| r.source.clone())
                .unwrap_or_default()
        };
    }
}

fn collect_memory(sys: &System, extras: &dyn MemoryExtras) -> MemoryInfo {
    let mut info = MemoryInfo {
        total_bytes: sys.total_memory(),
        used_bytes: sys.used_memory(),
        available_bytes: sys.available_memory(),
        swap_total_bytes: sys.total_swap(),
        swap_used_bytes: sys.used_swap(),
        ..MemoryInfo::default()
    };

    // 平台附加读数（Windows = GlobalMemoryStatusEx 的提交量 / 内存负载）
    if let Some(status) = extras.memory_status() {
        info.system_load_pct = status.load_pct as f32;
        info.commit_used_bytes = status.commit_used_bytes;
        info.commit_limit_bytes = status.commit_limit_bytes;
        // sysinfo 在某些机器上报不全时用 Win32 兜底
        if info.total_bytes == 0 {
            info.total_bytes = status.phys_total_bytes;
            info.available_bytes = status.phys_avail_bytes;
            info.used_bytes = status.phys_total_bytes.saturating_sub(status.phys_avail_bytes);
        }
    }

    info.recompute();
    info
}

fn collect_disks(disks: &Disks, elapsed_secs: f64) -> Vec<DiskInfo> {
    let mut list: Vec<DiskInfo> = disks
        .list()
        .iter()
        .map(|d| {
            let total = d.total_space();
            let free = d.available_space();
            let used = total.saturating_sub(free);
            let u = d.usage();
            DiskInfo {
                name: d.name().to_string_lossy().to_string(),
                mount_point: d.mount_point().to_string_lossy().to_string(),
                file_system: d.file_system().to_string_lossy().to_string(),
                disk_kind: disk_kind_label(d.kind()),
                removable: d.is_removable(),
                total_bytes: total,
                used_bytes: used,
                free_bytes: free,
                usage_pct: percent_of(used, total),
                read_bytes_per_sec: (u.read_bytes as f64 / elapsed_secs).round() as u64,
                write_bytes_per_sec: (u.written_bytes as f64 / elapsed_secs).round() as u64,
                total_read_bytes: u.total_read_bytes,
                total_written_bytes: u.total_written_bytes,
            }
        })
        .collect();

    list.sort_by(|a, b| a.mount_point.cmp(&b.mount_point));
    list
}

/// GPU：多卡分别采样后再汇总出一览数据
fn collect_gpu(probe: &mut dyn GpuSource) -> GpuInfo {
    let (devices, source) = probe.sample();
    let mut info = GpuInfo {
        devices,
        source,
        ..GpuInfo::default()
    };
    info.aggregate();
    info
}

/// 网络：sysinfo 的 received/transmitted 自上一次 refresh 起算增量，除以实测间隔得到速率。
///
/// 注意 Windows 上同一张物理网卡会同时出现「真实接口」和「过滤驱动」两个条目
/// （实测本机 WLAN 与 WLAN-Huorong NDIS Filter 的累计流量完全相同），
/// 不去重的话汇总速率会翻倍。
fn collect_net(net: &Networks, elapsed_secs: f64) -> NetInfo {
    use std::collections::HashSet;

    let mut seen_mac: HashSet<String> = HashSet::new();
    let mut rows: Vec<NetIface> = Vec::new();

    for (name, data) in net.iter() {
        let mac = format_mac(&data.mac_address().0);
        let total_received = data.total_received();
        let total_transmitted = data.total_transmitted();

        // 同一 MAC 出现两次 = 物理网卡 + 它的 NDIS 过滤驱动，只保留第一次
        if !mac.is_empty() && !seen_mac.insert(mac.clone()) {
            continue;
        }
        // 没有 MAC 的条目（部分过滤驱动），若累计流量与已有条目完全一致，判为重复
        if mac.is_empty()
            && (total_received > 0 || total_transmitted > 0)
            && rows.iter().any(|r| {
                r.total_received == total_received && r.total_transmitted == total_transmitted
            })
        {
            continue;
        }

        rows.push(NetIface {
            name: name.clone(),
            mac,
            is_virtual: is_virtual_iface(name),
            rx_bytes_per_sec: (data.received() as f64 / elapsed_secs).round() as u64,
            tx_bytes_per_sec: (data.transmitted() as f64 / elapsed_secs).round() as u64,
            total_received,
            total_transmitted,
        });
    }

    rows.sort_by(|a, b| {
        // 有流量的排前面，其次按名称
        let a_active = a.total_received + a.total_transmitted;
        let b_active = b.total_received + b.total_transmitted;
        b_active.cmp(&a_active).then_with(|| a.name.cmp(&b.name))
    });

    NetInfo {
        rx_bytes_per_sec: rows.iter().map(|i| i.rx_bytes_per_sec).sum(),
        tx_bytes_per_sec: rows.iter().map(|i| i.tx_bytes_per_sec).sum(),
        total_received: rows.iter().map(|i| i.total_received).sum(),
        total_transmitted: rows.iter().map(|i| i.total_transmitted).sum(),
        interfaces: rows,
    }
}

/// 虚拟网卡识别：VMware / VirtualBox / Hyper-V / WSL / Docker 等
fn is_virtual_iface(name: &str) -> bool {
    const KEYWORDS: [&str; 10] = [
        "vmware",
        "vmnet",
        "virtualbox",
        "hyper-v",
        "vethernet",
        "ndis filter",
        "pseudo",
        "wsl",
        "docker",
        "loopback",
    ];
    let lower = name.to_ascii_lowercase();
    KEYWORDS.iter().any(|k| lower.contains(k))
}

fn format_mac(b: &[u8; 6]) -> String {
    if b.iter().all(|x| *x == 0) {
        return String::new();
    }
    b.iter()
        .map(|x| format!("{:02X}", x))
        .collect::<Vec<_>>()
        .join(":")
}

fn disk_kind_label(kind: DiskKind) -> String {
    match kind {
        DiskKind::SSD => "SSD".to_string(),
        DiskKind::HDD => "HDD".to_string(),
        _ => "未知".to_string(),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一个「只有 sysinfo 给的标称主频」的 CpuInfo（Windows 上 sysinfo 就是给这个）
    fn cpu_with_sysinfo_freq() -> CpuInfo {
        CpuInfo {
            freq_mhz: 2300,
            ..CpuInfo::default()
        }
    }

    #[test]
    fn cpu_extras_prefers_live_frequency_and_flags_it() {
        let mut cpu = cpu_with_sysinfo_freq();
        apply_cpu_extras(
            &mut cpu,
            &CpuExtrasReading {
                live_freq_mhz: Some(3416),
                sysinfo_freq_live: false,
            },
        );
        assert_eq!(cpu.freq_mhz, 3416, "平台给了实时主频就用它");
        assert!(cpu.freq_live, "实时值要如实标记，界面才敢写「当前主频」");
    }

    #[test]
    fn cpu_extras_without_live_channel_degrades_honestly() {
        // Windows 上 PDH 拿不到：保留 sysinfo 的标称频率，但不得宣称它是实时值
        let mut cpu = cpu_with_sysinfo_freq();
        apply_cpu_extras(&mut cpu, &CpuExtrasReading::default());
        assert_eq!(cpu.freq_mhz, 2300);
        assert!(!cpu.freq_live, "标称频率不能被当成实时主频");

        // Linux：sysinfo 的主频本来就是实时值（cpufreq），原样保留并标记
        let mut cpu = cpu_with_sysinfo_freq();
        apply_cpu_extras(
            &mut cpu,
            &CpuExtrasReading {
                sysinfo_freq_live: true,
                ..CpuExtrasReading::default()
            },
        );
        assert_eq!(cpu.freq_mhz, 2300);
        assert!(cpu.freq_live);
    }
}
