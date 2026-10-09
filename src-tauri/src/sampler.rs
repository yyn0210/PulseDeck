//! 后台采样：所有数据在这里统一刷新，buf/命令只读取最后一次快照。

use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sysinfo::{DiskKind, Disks, Networks, ProcessesToUpdate, System};

use crate::model::{
    percent_of, BatteryInfo, CpuInfo, DiskInfo, GpuInfo, MemoryInfo, NetIface, NetInfo, ProcInfo,
    Snapshot, SystemInfo, ThermalInfo,
};
use crate::{battery, gpu, thermal, win32};

/// 进程榜单长度
const TOP_N: usize = 8;
/// sysinfo 两次 CPU 刷新之间的最小间隔（官方常量约 200ms）
const CPU_WARMUP_MS: u64 = 220;

struct Inner {
    sys: System,
    disks: Disks,
    net: Networks,
    gpu: gpu::GpuProbe,
    thermal: thermal::ThermalProbe,
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
                gpu: gpu::GpuProbe::new(),
                thermal: thermal::ThermalProbe::new(),
            }),
            latest: Mutex::new(Snapshot::default()),
            last_tick: Mutex::new(Instant::now()),
        }
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
            gpu,
            thermal,
        } = &mut *inner;

        sys.refresh_cpu_all();
        sys.refresh_memory();
        sys.refresh_processes(ProcessesToUpdate::All, true);
        disks.refresh(true);
        net.refresh(true);

        let cpu = collect_cpu(sys);
        let memory = collect_memory(sys);
        let disks_out = collect_disks(disks, elapsed_secs);
        let battery_info: BatteryInfo = battery::collect();
        let net_out = collect_net(net, elapsed_secs);
        let mut gpu_out = collect_gpu(gpu);
        let thermal_out: ThermalInfo = thermal.sample();
        // NVML 拿不到温度时（Intel / AMD 显卡）回退到硬件监控软件的读数
        if gpu_out.temp_c.is_none() {
            gpu_out.temp_c = thermal_out.gpu_temp_c;
        }

        Snapshot {
            ts_ms: now_ms(),
            sample_interval_ms: elapsed_ms,
            system: collect_system(sys),
            cpu,
            memory,
            disks: disks_out,
            battery: battery_info,
            gpu: gpu_out,
            net: net_out,
            thermal: thermal_out,
        }
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
        top_cpu,
        top_mem,
    };
    info.normalize();
    info
}

fn collect_memory(sys: &System) -> MemoryInfo {
    let mut info = MemoryInfo {
        total_bytes: sys.total_memory(),
        used_bytes: sys.used_memory(),
        available_bytes: sys.available_memory(),
        swap_total_bytes: sys.total_swap(),
        swap_used_bytes: sys.used_swap(),
        ..MemoryInfo::default()
    };

    if let Some(status) = win32::memory_status() {
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
fn collect_gpu(probe: &mut gpu::GpuProbe) -> GpuInfo {
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
