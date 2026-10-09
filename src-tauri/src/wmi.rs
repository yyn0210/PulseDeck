//! WMI（COM）查询封装
//!
//! 只有两处会用到，都是 sysinfo / Win32 拿不到、但又希望尽量取到的信息：
//! 1. 硬件温度传感器 —— LibreHardwareMonitor / OpenHardwareMonitor 的 `Sensor` 表；
//! 2. 显卡名称 —— `Win32_VideoController`（PDH 只给 luid，不给可读名称）。
//!
//! COM 初始化的作用域必须包住整个查询过程：一旦提前 CoUninitialize，
//! 拿到的 IWbemServices 指针就失效了，所以这里刻意用「一次性查询」而不是返回句柄。

use std::collections::HashMap;

use windows::core::{GUID, PCWSTR};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoInitializeSecurity, CoSetProxyBlanket, CoUninitialize,
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, EOAC_DEFAULT, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL,
    RPC_C_AUTHN_LEVEL_DEFAULT, RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::Variant::{
    VariantClear, VARIANT, VT_BSTR, VT_I4, VT_R4, VT_R8, VT_UI4,
};
use windows::Win32::System::Wmi::{
    IWbemClassObject, IWbemLocator, WBEM_FLAG_FORWARD_ONLY,
    WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_INFINITE,
};

/// CLSID_WbemLocator（windows crate 未导出该常量，手工固化）
fn wbem_locator_clsid() -> GUID {
    GUID::from_u128(0x4590f811_1d3a_11d0_891f_00aa004b2e24)
}

/// 一个单元格的值
#[derive(Debug, Clone)]
pub enum Prop {
    Str(String),
    Num(f64),
    Empty,
}

pub type Row = HashMap<String, Prop>;

impl Prop {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Prop::Num(v) => Some(*v),
            Prop::Str(s) => s.parse::<f64>().ok(),
            Prop::Empty => None,
        }
    }

    pub fn as_string(&self) -> Option<String> {
        match self {
            Prop::Str(s) => Some(s.clone()),
            Prop::Num(v) => Some(v.to_string()),
            Prop::Empty => None,
        }
    }
}

/// COM 初始化作用域。Drop 时配对 CoUninitialize。
struct ComScope {
    uninit: bool,
}

impl ComScope {
    fn new() -> Option<Self> {
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        // S_OK = 本线程首次初始化；S_FALSE = 已初始化（引用计数 +1，仍需配对释放）
        if hr.is_ok() {
            // 进程级 COM 安全描述符：不设置这一步，跨进程调用 WMI 服务会被拒
            // （WBEM_E_ACCESS_DENIED）。已被别人设置过时返回 RPC_E_TOO_LATE，可忽略。
            let sec = unsafe {
                CoInitializeSecurity(
                    None,
                    -1,
                    None,
                    None,
                    RPC_C_AUTHN_LEVEL_DEFAULT,
                    RPC_C_IMP_LEVEL_IMPERSONATE,
                    None,
                    EOAC_NONE,
                    None,
                )
            };
            if let Err(e) = sec {
                diag(&format!("CoInitializeSecurity: {e:?}"));
            }
            Some(Self { uninit: true })
        } else {
            // RPC_E_CHANGED_MODE 之类：本线程 COM 已被初始化为其它模式，
            // 这时不能 Uninitialize，否则会破坏别人的引用计数。
            None
        }
    }
}

impl Drop for ComScope {
    fn drop(&mut self) {
        if self.uninit {
            unsafe { CoUninitialize() };
        }
    }
}

/// 执行一次 WQL 查询，返回列名 -> 值 的行集合。失败返回 None。
pub fn query(namespace: &str, wql: &str, cols: &[&str]) -> Option<Vec<Row>> {
    let Some(_com) = ComScope::new() else {
        diag("COM 初始化失败");
        return None;
    };

    let Ok(locator) = (unsafe {
        CoCreateInstance::<_, IWbemLocator>(&wbem_locator_clsid(), None, CLSCTX_INPROC_SERVER)
    }) else {
        diag("创建 IWbemLocator 失败");
        return None;
    };

    let ns = windows::core::BSTR::from(namespace);
    let empty = windows::core::BSTR::new();
    let Ok(svc) =
        (unsafe { locator.ConnectServer(&ns, &empty, &empty, &empty, 0, &empty, None) }) else {
            diag(&format!("连接命名空间 {namespace} 失败"));
            return None;
        };

    // WMI 是跨进程 COM：不显式设置代理认证层，ExecQuery/Next 会直接报
    // WBEM_E_ACCESS_DENIED(0x80041003)。C# / PowerShell 的封装内置了这一步，
    // 手写 COM 必须自己补上。
    let Ok(proxy) = windows::core::Interface::cast::<windows::core::IUnknown>(&svc) else {
        diag("无法取得 IUnknown 代理");
        return None;
    };
    let blanket = unsafe {
        CoSetProxyBlanket(
            &proxy,
            RPC_C_AUTHN_DEFAULT,
            RPC_C_AUTHZ_DEFAULT,
            None,
            RPC_C_AUTHN_LEVEL_CALL,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_DEFAULT,
        )
    };
    if blanket.is_err() {
        diag("CoSetProxyBlanket 失败");
    }

    let lang = windows::core::BSTR::from("WQL");
    let wql_bstr = windows::core::BSTR::from(wql);
    let Ok(en) = (unsafe {
        svc.ExecQuery(
            &lang,
            &wql_bstr,
            WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
            None,
        )
    }) else {
        diag(&format!("执行查询失败: {wql}"));
        return None;
    };

    let mut rows = Vec::new();
    let mut first = true;
    loop {
        let mut objs: [Option<IWbemClassObject>; 1] = [None; 1];
        let mut returned: u32 = 0;
        let hr = unsafe { en.Next(WBEM_INFINITE, &mut objs, &mut returned) };
        if first {
            first = false;
            diag(&format!("Next hr={hr:?} returned={returned}"));
        }
        if hr.is_err() || returned == 0 {
            break;
        }
        let Some(obj) = objs[0].take() else {
            break;
        };
        let mut row = Row::new();
        for col in cols {
            let wide: Vec<u16> = col.encode_utf16().chain(Some(0)).collect();
            let mut v = VARIANT::default();
            let got = unsafe { obj.Get(PCWSTR(wide.as_ptr()), 0, &mut v, None, None) };
            if got.is_ok() {
                let p = read_variant(&v);
                diag(&format!("  Get({col}) -> {p:?}"));
                row.insert(col.to_string(), p);
            } else {
                diag(&format!("  Get({col}) failed: {got:?}"));
            }
            // VARIANT 没有 Drop 实现，BSTR 之类的变体需要显式释放
            let _ = unsafe { VariantClear(&mut v) };
        }
        rows.push(row);
    }
    Some(rows)
}

/// RPC 的「取默认值」哨兵常量，值与方法二一致
const RPC_C_AUTHN_DEFAULT: u32 = 0xffff_ffff;
const RPC_C_AUTHZ_DEFAULT: u32 = 0xffff_ffff;

/// 排错用：仅在设置 WB_WMI_DIAG=1 时输出，避免污染正常日志
fn diag(msg: &str) {
    if std::env::var("WB_WMI_DIAG").as_deref() == Ok("1") {
        eprintln!("[wmi] {msg}");
    }
}

fn read_variant(v: &VARIANT) -> Prop {
    let vt = unsafe { v.Anonymous.Anonymous.vt };
    match vt {
        VT_R8 => Prop::Num(unsafe { v.Anonymous.Anonymous.Anonymous.dblVal }),
        VT_R4 => Prop::Num(unsafe { v.Anonymous.Anonymous.Anonymous.fltVal } as f64),
        VT_I4 => Prop::Num(unsafe { v.Anonymous.Anonymous.Anonymous.lVal } as f64),
        VT_UI4 => Prop::Num(unsafe { v.Anonymous.Anonymous.Anonymous.ulVal } as f64),
        VT_BSTR => {
            let bstr = unsafe { &v.Anonymous.Anonymous.Anonymous.bstrVal };
            // BSTR::to_string() 已经返回一个 String（不是 Result）
            Prop::Str(bstr.to_string())
        }
        _ => Prop::Empty,
    }
}

/* ------------------------------ 具体业务查询 ------------------------------ */

/// 硬件监控软件的命名空间，按优先级尝试。
///
/// ⚠️ 现实情况：**LibreHardwareMonitor 自 0.9.5 起删除了自带的 WMI 提供程序**
/// （0.9.0~0.9.4 才有 `WMI/WmiProvider.cs`），所以 `root\LibreHardwareMonitor` 在
/// 新版 LHM 上永远不存在。这里保留这两个命名空间只是为了兼容
/// **OpenHardwareMonitor** 以及**旧版 LHM（≤ 0.9.4）**；本工具自己的 CPU 温度主通道是
/// 「PawnIO + Intel MSR」（见 `pawnio.rs`）。
const THERMAL_NAMESPACES: [&str; 2] = ["root\\LibreHardwareMonitor", "root\\OpenHardwareMonitor"];

/// 一条温度传感器读数
#[derive(Debug, Clone)]
pub struct HwTemp {
    /// 归属：cpu / gpu / other
    pub kind: String,
    pub name: String,
    pub value_c: f32,
}

/// 从 LHM/OHM 读一次温度传感器（有软件才读得到，否则返回 None）
pub fn hardware_temperatures() -> Option<(String, Vec<HwTemp>)> {
    for ns in THERMAL_NAMESPACES {
        let cols = ["Identifier", "Name", "Value", "SensorType"];
        let wql = "SELECT Identifier, Name, Value, SensorType FROM Sensor WHERE SensorType = 'Temperature'";
        let Some(rows) = query(ns, wql, &cols) else {
            continue;
        };
        if rows.is_empty() {
            continue;
        }
        let mut temps = Vec::new();
        for r in &rows {
            let Some(value) = r.get("Value").and_then(|p| p.as_f64()) else {
                continue;
            };
            let identifier = r
                .get("Identifier")
                .and_then(|p| p.as_string())
                .unwrap_or_default();
            let name = r.get("Name").and_then(|p| p.as_string()).unwrap_or_default();
            temps.push(HwTemp {
                kind: identifier_kind(&identifier),
                name,
                value_c: value as f32,
            });
        }
        if !temps.is_empty() {
            return Some((ns.to_string(), temps));
        }
    }
    None
}

/// 从 Identifier 判断硬件归属。LHM/OHM 的 identifier 形如
/// `/intelcpu/0/temperature/3`、`/gpu-nvidia/0/temperature/0`。
fn identifier_kind(identifier: &str) -> String {
    let second = identifier.trim_start_matches('/').split('/').next().unwrap_or("");
    let lower = second.to_ascii_lowercase();
    if lower.contains("gpu") {
        "gpu".to_string()
    } else if lower.contains("cpu") {
        "cpu".to_string()
    } else {
        "other".to_string()
    }
}

/// 显卡名称列表（Win32_VideoController 的 Name 字段）
pub fn video_controller_names() -> Vec<String> {
    let Some(rows) = query(
        "root\\cimv2",
        "SELECT Name FROM Win32_VideoController",
        &["Name"],
    ) else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|r| r.get("Name").and_then(|p| p.as_string()))
        .filter(|s| !s.trim().is_empty())
        .collect()
}

/// ACPI 热区温度原始读数（WMI `root\wmi:MSAcpi_ThermalZoneTemperature`）。
///
/// 这是 Windows 暴露 CPU / 主板热区温度的通用接口，厂商工具（如联想 LegionZone）
/// 也拿它当作通用的兜底温度源。属性 `CurrentTemperature` 的单位是「十分之一开尔文」，
/// 换算与有效性过滤交给 `thermal` 模块的 `to_celsius` 处理。
///
/// 本机若没有 ACPI 热区实例（多数台式机 / 虚拟机不暴露），`query` 返回空行 → 这里返回 None。
pub fn acpi_thermal_zone_raw() -> Option<Vec<f64>> {
    const NS: &str = "root\\wmi";
    const WQL: &str = "SELECT CurrentTemperature FROM MSAcpi_ThermalZoneTemperature";
    let Some(rows) = query(NS, WQL, &["CurrentTemperature"]) else {
        return None;
    };
    let mut out = Vec::new();
    for r in &rows {
        if let Some(v) = r.get("CurrentTemperature").and_then(|p| p.as_f64()) {
            out.push(v);
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}
