//! WMI（COM）查询封装
//!
//! 用到的地方都是 sysinfo / Win32 / PDH 拿不到、但又希望尽量取到的信息：
//! 1. 硬件温度与风扇传感器 —— LibreHardwareMonitor / OpenHardwareMonitor 的 `Sensor` 表；
//! 2. 风扇 —— `Win32_Fan`（ACPI）与联想的私有 ACPI-WMI 方法。
//!
//! （显卡名称曾走 `Win32_VideoController`，现改由 DXGI 提供：见 `gpu_dxgi.rs`。）
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
    VariantClear, VARIANT, VT_BSTR, VT_I4, VT_R4, VT_R8, VT_UI1, VT_UI2, VT_UI4,
};
use windows::Win32::System::Wmi::{
    IWbemClassObject, IWbemLocator, IWbemServices, WBEM_FLAG_FORWARD_ONLY, WBEM_GENERIC_FLAG_TYPE,
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

/// 连接一个 WMI 命名空间，并完成代理认证层设置。
///
/// `query()` 与 `call_method_u8()` 共用：WMI 是跨进程 COM，不设置 `CoSetProxyBlanket`
/// 的话后续调用会直接报 `WBEM_E_ACCESS_DENIED(0x80041003)`。
///
/// ⚠️ **返回顺序是 `(COM 作用域, 服务指针)`，不能对调**：
/// Rust 局部变量按声明顺序的**逆序**析构，所以先声明的 `ComScope` 会最后释放
/// （`CoUninitialize`），保证 `IWbemServices` 及其派生对象都在 COM 仍有效时 `Release`。
/// 若把服务指针放在前面，`CoUninitialize` 会先执行，随后 Release 触发
/// `STATUS_ACCESS_VIOLATION(0xc0000005)`——这是实测踩过的坑。
fn connect(namespace: &str) -> Option<(ComScope, IWbemServices)> {
    let com = ComScope::new()?;

    let locator = unsafe {
        CoCreateInstance::<_, IWbemLocator>(&wbem_locator_clsid(), None, CLSCTX_INPROC_SERVER)
    }
    .map_err(|e| diag(&format!("创建 IWbemLocator 失败: {e:?}")))
    .ok()?;

    let ns = windows::core::BSTR::from(namespace);
    let empty = windows::core::BSTR::new();
    let svc = unsafe { locator.ConnectServer(&ns, &empty, &empty, &empty, 0, &empty, None) }
        .map_err(|e| diag(&format!("连接命名空间 {namespace} 失败: {e:?}")))
        .ok()?;

    let proxy = windows::core::Interface::cast::<windows::core::IUnknown>(&svc)
        .map_err(|e| diag(&format!("无法取得 IUnknown 代理: {e:?}")))
        .ok()?;
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

    Some((com, svc))
}

/// 调用一个 **ACPI-WMI 映射类**上的方法（如联想的 `LENOVO_FAN_METHOD`）。
///
/// 为什么需要它：厂商的读数接口（联想 `Fan_GetCurrentFanSpeed`）挂在 WMI **方法类**上，
/// 而不是数据类。方法类没有实例，只能先生成入参实例、再 `ExecMethod` 调用。
///
/// 踩坑记录（2026-10-10，逐条实测，**勿再走弯路**）：
///
/// 1. **不能**用 `SELECT * FROM LENOVO_FAN_METHOD` 取类定义——方法类不可枚举，恒返回 0 行。
///    （这解释了 `Get-WmiObject` 报告「0 个实例」，别误判为「通道不可用」。）
/// 2. 取类定义用**裸类名** `LENOVO_FAN_METHOD`（`GetObject`）。
/// 3. **`ExecMethod` 的对象路径必须是 ACPI 实例路径**：
///    `LENOVO_FAN_METHOD.InstanceName="ACPI\PNP0C14\GMZN_0"`。
///    * 裸类名 → `WBEM_E_INVALID_METHOD_PARAMETERS(0x8004102F)`（误导性错误码）
///    * `LENOVO_FAN_METHOD=@` → `WBEM_E_INVALID_OBJECT_PATH(0x8004103A)`
///    * `InstanceName="GMZN_0"`（短名）→ `WBEM_E_NOT_FOUND(0x80041002)`
/// 4. **必须管理员权限**。非提权时同一调用返回 `WBEM_E_ACCESS_DENIED(0x80041003)`，
///    且**与 `CoSetProxyBlanket` 的 impersonation 级别无关**（IMPERSONATE/DELEGATE/IDENTIFY
///    三级都一样失败）——因为拦在 ACPI-WMI 提供程序侧，不是 COM 认证层。
///    实测提权后立即成功（`CurrentFanSpeed = 3003`）。
///
/// * `in_args`：`(参数名, 值)` 列表，值作为 WMI `CIM_UINT8` 写入
/// * 返回：输出参数的 `(名字, 值)` 列表；调用失败或 `ReturnValue != 0` 时返回 `None`
///
/// 入参一律按 `uint8` 处理——联想的接口只用到 `FanID` / `SensorID` 这类小整数。
pub fn call_method_u8(
    namespace: &str,
    class: &str,
    instance: &str,
    method: &str,
    in_args: &[(&str, u8)],
) -> Option<Vec<(String, f64)>> {
    // 注意绑定顺序：`_com` 必须声明在 `svc` 之前，才能最后析构（见 `connect` 的说明）
    let (_com, svc) = connect(namespace)?;
    let class_obj = get_class(&svc, class)?;
    exec_method(&svc, &class_obj, class, instance, method, in_args)
}

/// 取一个 WMI **类定义**对象。
///
/// `GetObject` 一律用**裸类名**（如 `LENOVO_FAN_METHOD`）。`=@` 之类的路径形态会返回
/// `WBEM_E_INVALID_OBJECT_PATH(0x8004103A)`——实测踩过（详见 [`call_method_u8`] 的说明）。
fn get_class(svc: &IWbemServices, class: &str) -> Option<IWbemClassObject> {
    let cls_bstr = windows::core::BSTR::from(class);
    let mut class_obj: Option<IWbemClassObject> = None;
    unsafe {
        svc.GetObject(
            &cls_bstr,
            WBEM_GENERIC_FLAG_TYPE(0),
            None,
            Some(&mut class_obj),
            None,
        )
    }
    .map_err(|e| diag(&format!("GetObject({class}) 失败: {e:?}")))
    .ok()?;
    match class_obj {
        Some(o) => Some(o),
        None => {
            diag(&format!("GetObject({class}) 未返回类对象"));
            None
        }
    }
}

/// 在一个**已建好的会话**（WMI 连接 + 类定义）上调用方法，并读回输出参数。
///
/// 之所以把「建连 + 取类定义」留在外面：`ConnectServer` 是这一串调用里最贵的一步，
/// 而联想的风扇轮询要对 `FanID` 0..=3 各调一次同一方法。若每次都走一遍
/// [`connect`]，桥接进程每秒就要建 4 次 WMI 连接（实测稳定吃掉约 5% 单核）。
/// 见 [`lenovo_fans`]。
fn exec_method(
    svc: &IWbemServices,
    class_obj: &IWbemClassObject,
    class: &str,
    instance: &str,
    method: &str,
    in_args: &[(&str, u8)],
) -> Option<Vec<(String, f64)>> {
    // 1) 由方法名取 InParameters 签名
    let method_name = windows::core::BSTR::from(method);
    let mut in_sig: Option<IWbemClassObject> = None;
    let mut out_sig: Option<IWbemClassObject> = None;
    unsafe {
        class_obj
            .GetMethod(&method_name, 0, &mut in_sig, &mut out_sig)
            .map_err(|e| diag(&format!("GetMethod({method}) 失败: {e:?}")))
            .ok()?;
    }
    let Some(in_sig) = in_sig else {
        diag(&format!("方法 {class}.{method} 没有入参签名"));
        return None;
    };

    // 2) 按签名生成入参实例，逐个填值
    let in_obj = unsafe {
        in_sig
            .SpawnInstance(0)
            .map_err(|e| diag(&format!("SpawnInstance 失败: {e:?}")))
            .ok()?
    };
    for (name, value) in in_args {
        let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        let mut v = VARIANT::default();
        set_u8_variant(&mut v, *value);
        let put = unsafe { in_obj.Put(PCWSTR(wide.as_ptr()), 0, &v, 0) };
        // Put 是浅拷贝，VARIANT 本体可以立刻清掉
        let _ = unsafe { VariantClear(&mut v) };
        if put.is_err() {
            diag(&format!("Put({name}) 失败: {put:?}"));
            return None;
        }
    }

    // 3) ExecMethod：对象路径必须是 ACPI 实例路径（见文档注释第 3 条）
    let path = windows::core::BSTR::from(&format!(
        "{class}.InstanceName=\"{}\"",
        instance.replace('\\', "\\\\")
    ));
    let mut out_obj: Option<IWbemClassObject> = None;
    let exec = unsafe {
        svc.ExecMethod(
            &path,
            &method_name,
            WBEM_GENERIC_FLAG_TYPE(0),
            None,
            &in_obj,
            Some(&mut out_obj),
            None,
        )
    };
    if let Err(e) = exec {
        diag(&format!("ExecMethod({method} @ {instance}) 失败: {e:?}"));
        return None;
    }
    let Some(out_obj) = out_obj else {
        diag(&format!("ExecMethod({method}) 未返回输出对象"));
        return None;
    };

    // 4) 读回输出参数；ReturnValue != 0 视为方法内部失败
    let mut out = Vec::new();
    let mut ret_code: Option<f64> = None;

    for key in out_param_names(in_args) {
        let wide: Vec<u16> = key.encode_utf16().chain(Some(0)).collect();
        let mut v = VARIANT::default();
        let got = unsafe { out_obj.Get(PCWSTR(wide.as_ptr()), 0, &mut v, None, None) };
        if got.is_ok() {
            if let Prop::Num(n) = read_variant(&v) {
                if key == "ReturnValue" {
                    ret_code = Some(n);
                }
                out.push((key.clone(), n));
            }
        }
        let _ = unsafe { VariantClear(&mut v) };
    }

    if let Some(code) = ret_code {
        if code != 0.0 {
            diag(&format!("{class}.{method} 返回失败码 {code}"));
            return None;
        }
    }
    Some(out)
}

/// 需要探测的输出参数名。
///
/// 无法枚举 `IWbemClassObject` 的所有属性名（需额外的枚举接口），所以用保守候选列表：
/// 联想的接口输出就叫 `CurrentFanSpeed` / `CurrentSensorTemperature`，加 `ReturnValue`。
///
/// 说明：联想的这些方法在 MOF 里声明为 `void`，**没有 `ReturnValue`**，
/// 所以 `ReturnValue` 只是防御性候选（取不到就不会触发失败判定）。
fn out_param_names(in_args: &[(&str, u8)]) -> Vec<String> {
    let mut names = vec![
        "ReturnValue".to_string(),
        "CurrentFanSpeed".to_string(),
        "CurrentSensorTemperature".to_string(),
    ];
    for (n, _) in in_args {
        names.push((*n).to_string());
    }
    names
}

/// 往 VARIANT 里写一个 WMI `CIM_UINT8`（VT_UI1）。
///
/// 注意 `VARIANT.Anonymous` 是 `ManuallyDrop<VARIANT_0_0>`：Rust 不会在联合体字段上
/// 自动 Deref，必须显式 `&mut *` 才能取到内层结构。
fn set_u8_variant(v: &mut VARIANT, value: u8) {
    unsafe {
        let inner = &mut *v.Anonymous.Anonymous;
        inner.vt = VT_UI1;
        inner.Anonymous.bVal = value;
    }
}

/// 排错用：把某个 WMI 类的 MOF 文本 dump 出来（仅 `#[cfg(test)]` 使用）。
///
/// 用来确认方法类上到底挂了哪些方法、每个方法的入参名字与类型。
/// 返回 `None` 表示类不存在或取不到。
#[cfg(test)]
pub fn dump_class_mof(namespace: &str, class: &str) -> Option<String> {
    let (_com, svc) = connect(namespace)?;
    let cls_bstr = windows::core::BSTR::from(class);
    let mut class_obj: Option<IWbemClassObject> = None;
    unsafe {
        svc.GetObject(
            &cls_bstr,
            WBEM_GENERIC_FLAG_TYPE(0),
            None,
            Some(&mut class_obj),
            None,
        )
    }
    .map_err(|e| diag(&format!("GetObject({class}) 失败: {e:?}")))
    .ok()?;
    let class_obj = class_obj?;
    let mof = unsafe { class_obj.GetObjectText(0) };
    match mof {
        Ok(b) => Some(b.to_string()),
        Err(e) => {
            diag(&format!("GetObjectText({class}) 失败: {e:?}"));
            None
        }
    }
}

/// 排错用：枚举一个类对象（或签名对象）的**属性名**。
#[cfg(test)]
pub fn dump_prop_names(obj_namespace: &str, class: &str, method: Option<&str>) -> Vec<String> {
    let Some((_com, svc)) = connect(obj_namespace) else {
        return Vec::new();
    };
    let cls_bstr = windows::core::BSTR::from(class);
    let mut class_obj: Option<IWbemClassObject> = None;
    if unsafe {
        svc.GetObject(
            &cls_bstr,
            WBEM_GENERIC_FLAG_TYPE(0),
            None,
            Some(&mut class_obj),
            None,
        )
    }
    .is_err()
    {
        return Vec::new();
    }
    let Some(class_obj) = class_obj else {
        return Vec::new();
    };

    // 指定方法时，改枚举该方法的 InParameters 签名属性
    let target = if let Some(m) = method {
        let mn = windows::core::BSTR::from(m);
        let mut in_sig: Option<IWbemClassObject> = None;
        let mut out_sig: Option<IWbemClassObject> = None;
        if unsafe { class_obj.GetMethod(&mn, 0, &mut in_sig, &mut out_sig) }.is_err() {
            return Vec::new();
        }
        match in_sig {
            Some(s) => s,
            None => return Vec::new(),
        }
    } else {
        class_obj
    };

    let mut names = Vec::new();
    if unsafe { target.BeginEnumeration(0) }.is_err() {
        return names;
    }
    loop {
        let mut name = windows::core::BSTR::new();
        let mut val = VARIANT::default();
        let mut ptype: i32 = 0;
        let mut flavor: i32 = 0;
        let r = unsafe { target.Next(0, &mut name, &mut val, &mut ptype, &mut flavor) };
        let _ = unsafe { VariantClear(&mut val) };
        if r.is_err() {
            break;
        }
        let s = name.to_string();
        if s.is_empty() {
            break;
        }
        names.push(s);
    }
    let _ = unsafe { target.EndEnumeration() };
    names
}

/// 排错用：用多种对象路径格式依次尝试 `ExecMethod`，打印各自的 HRESULT。
///
/// 用来定位 `WBEM_E_INVALID_METHOD_PARAMETERS(0x8004102F)` 到底卡在路径格式
/// 还是参数上。返回 `(路径描述, 结果描述)` 列表。
#[cfg(test)]
pub fn probe_exec_paths(namespace: &str, class: &str, method: &str, in_args: &[(&str, u8)]) -> Vec<(String, String)> {
    let mut results = Vec::new();
    let Some((_com, svc)) = connect(namespace) else {
        return results;
    };

    // 准备类定义 + 入参对象
    let cls_bstr = windows::core::BSTR::from(class);
    let mut class_obj: Option<IWbemClassObject> = None;
    if unsafe {
        svc.GetObject(
            &cls_bstr,
            WBEM_GENERIC_FLAG_TYPE(0),
            None,
            Some(&mut class_obj),
            None,
        )
    }
    .is_err()
    {
        return results;
    }
    let Some(class_obj) = class_obj else { return results };

    let method_name = windows::core::BSTR::from(method);
    let mut in_sig: Option<IWbemClassObject> = None;
    let mut out_sig: Option<IWbemClassObject> = None;
    if unsafe { class_obj.GetMethod(&method_name, 0, &mut in_sig, &mut out_sig) }.is_err() {
        return results;
    }
    let Some(in_sig) = in_sig else { return results };
    let Ok(in_obj) = (unsafe { in_sig.SpawnInstance(0) }) else {
        return results;
    };
    for (name, value) in in_args {
        let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        let mut v = VARIANT::default();
        set_u8_variant(&mut v, *value);
        let _ = unsafe { in_obj.Put(PCWSTR(wide.as_ptr()), 0, &v, 0) };
        let _ = unsafe { VariantClear(&mut v) };
    }

    // 实例名候选（从 LENOVO_FAN_TABLE_DATA 观察到的 ACPI 路径形态）
    let candidates: Vec<(String, String)> = vec![
        ("裸类名".to_string(), class.to_string()),
        ("类=@".to_string(), format!("{class}=@")),
        (
            "InstanceName=GMZN_0".to_string(),
            format!("{class}.InstanceName=\"ACPI\\\\PNP0C14\\\\GMZN_0\""),
        ),
        (
            "InstanceName 裸".to_string(),
            format!("{class}.InstanceName=\"GMZN_0\""),
        ),
    ];

    for (label, path) in candidates {
        let p = windows::core::BSTR::from(&path);
        let mut out_obj: Option<IWbemClassObject> = None;
        let r = unsafe {
            svc.ExecMethod(
                &p,
                &method_name,
                WBEM_GENERIC_FLAG_TYPE(0),
                None,
                &in_obj,
                Some(&mut out_obj),
                None,
            )
        };
        let desc = match r {
            Ok(()) if out_obj.is_some() => {
                let o = out_obj.as_ref().unwrap();
                let mut txt = String::new();
                if let Ok(t) = unsafe { o.GetObjectText(0) } {
                    txt = t.to_string().replace('\n', " ").replace("  ", " ");
                }
                format!("OK {txt}")
            }
            Ok(()) => "OK 但 out 为 None".to_string(),
            Err(e) => format!("{e:?}"),
        };
        results.push((format!("{label} -> {path}"), desc));
    }

    // 变量：给 in_obj 也单独设置代理认证层（部分 ACPI-WMI 提供程序要求）
    for imp in [
        ("IMPERSONATE", RPC_C_IMP_LEVEL_IMPERSONATE),
        ("DELEGATE", RPC_C_IMP_LEVEL_DELEGATE),
        ("IDENTIFY", RPC_C_IMP_LEVEL_IDENTIFY),
    ] {
        if let Ok(unk) = windows::core::Interface::cast::<windows::core::IUnknown>(&svc) {
            let _ = unsafe {
                CoSetProxyBlanket(
                    &unk,
                    RPC_C_AUTHN_DEFAULT,
                    RPC_C_AUTHZ_DEFAULT,
                    None,
                    RPC_C_AUTHN_LEVEL_CALL,
                    imp.1,
                    None,
                    EOAC_DEFAULT,
                )
            };
        }
        let path = format!("{class}.InstanceName=\"ACPI\\\\PNP0C14\\\\GMZN_0\"");
        let p = windows::core::BSTR::from(&path);
        let mut out_obj: Option<IWbemClassObject> = None;
        let r = unsafe {
            svc.ExecMethod(
                &p,
                &method_name,
                WBEM_GENERIC_FLAG_TYPE(0),
                None,
                &in_obj,
                Some(&mut out_obj),
                None,
            )
        };
        let desc = match r {
            Ok(()) if out_obj.is_some() => {
                let o = out_obj.as_ref().unwrap();
                let mut txt = String::new();
                if let Ok(t) = unsafe { o.GetObjectText(0) } {
                    txt = t.to_string().replace('\n', " ").replace("  ", " ");
                }
                format!("OK {txt}")
            }
            Ok(()) => "OK 但 out 为 None".to_string(),
            Err(e) => format!("{e:?}"),
        };
        results.push((format!("imp={}", imp.0), desc));
    }
    results
}

#[cfg(test)]
use windows::Win32::System::Com::{
    RPC_C_IMP_LEVEL_DELEGATE, RPC_C_IMP_LEVEL_IDENTIFY,
};

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
        // 联想的 CurrentFanSpeed / CurrentSensorTemperature 是 uint16（VT_UI2）
        VT_UI2 => Prop::Num(unsafe { v.Anonymous.Anonymous.Anonymous.uiVal } as f64),
        VT_UI1 => Prop::Num(unsafe { v.Anonymous.Anonymous.Anonymous.bVal } as f64),
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
/// 「PawnIO 直读片内传感器」（Intel 读 MSR、AMD 读 SMN，见 `pawnio.rs`）。
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

/// 一条风扇读数（来源自硬件监控软件或 ACPI）
#[derive(Debug, Clone)]
pub struct HwFan {
    /// 归属：cpu / gpu / other（由 Identifier 推断，与 HwTemp 同规则）
    pub kind: String,
    pub name: String,
    /// 转速。单位取决于数据源：LHM/OHM 的 Fan 传感器是 **RPM**；
    /// ACPI `Win32_Fan` 的 `DesiredSpeed` 多为百分比或「相对速度」，不一定真是 RPM。
    pub rpm: f32,
    /// 该读数的单位是否可信为 RPM
    pub is_rpm: bool,
}

/// 从 LHM / OpenHardwareMonitor 读一次风扇传感器（有软件才读得到，否则返回 None）。
///
/// 与 `hardware_temperatures()` 走同一套命名空间（见上方说明：**LHM ≥ 0.9.5 没有 WMI
/// 提供程序**，所以这条实际只对 OpenHardwareMonitor 或旧版 LHM 有效）。
/// LHM/OHM 的 Fan 传感器值单位就是 RPM，可信度最高。
pub fn hardware_fans() -> Option<(String, Vec<HwFan>)> {
    for ns in THERMAL_NAMESPACES {
        let cols = ["Identifier", "Name", "Value", "SensorType"];
        let wql = "SELECT Identifier, Name, Value, SensorType FROM Sensor WHERE SensorType = 'Fan'";
        let Some(rows) = query(ns, wql, &cols) else {
            continue;
        };
        if rows.is_empty() {
            continue;
        }
        let mut fans = Vec::new();
        for r in &rows {
            let Some(value) = r.get("Value").and_then(|p| p.as_f64()) else {
                continue;
            };
            // 0 表示风扇停转或传感器无实现，二者都无法区分，一律当作「读不到」
            if !value.is_finite() || value <= 0.0 {
                continue;
            }
            let identifier = r
                .get("Identifier")
                .and_then(|p| p.as_string())
                .unwrap_or_default();
            let name = r.get("Name").and_then(|p| p.as_string()).unwrap_or_default();
            fans.push(HwFan {
                kind: identifier_kind(&identifier),
                name,
                rpm: value as f32,
                is_rpm: true,
            });
        }
        if !fans.is_empty() {
            return Some((ns.to_string(), fans));
        }
    }
    None
}

/// ACPI 风扇（WMI `root\cimv2:Win32_Fan`）。
///
/// ⚠️ 现实情况：这个类**在绝大多数消费级机器上存在但没数据** —— 本机实测
/// `Win32_Fan` 有实例（Name=「冷却设备」、ActiveCooling=True），但 `DesiredSpeed`
/// 为空。厂商把转速放在私有的 EC / WMI 提供程序里，ACPI 只暴露「有无风扇」。
/// 即便 `DesiredSpeed` 有值，其单位也是「相对速度」而**不保证是 RPM**，故这里
/// 标记 `is_rpm = false`，交给上层决定展示口径。
pub fn acpi_fan_speeds() -> Vec<HwFan> {
    let Some(rows) = query(
        "root\\cimv2",
        "SELECT Name, DesiredSpeed, ActiveCooling, VariableSpeed FROM Win32_Fan",
        &["Name", "DesiredSpeed", "ActiveCooling", "VariableSpeed"],
    ) else {
        return Vec::new();
    };
    let mut fans = Vec::new();
    for r in &rows {
        let Some(speed) = r.get("DesiredSpeed").and_then(|p| p.as_f64()) else {
            continue; // 本机的常态：字段为空
        };
        if !speed.is_finite() || speed <= 0.0 {
            continue;
        }
        let name = r
            .get("Name")
            .and_then(|p| p.as_string())
            .unwrap_or_else(|| "ACPI 风扇".to_string());
        fans.push(HwFan {
            kind: "other".to_string(),
            name,
            rpm: speed as f32,
            // ACPI 的 DesiredSpeed 单位不可靠，不能当 RPM 展示
            is_rpm: false,
        });
    }
    fans
}

/// ACPI 热区温度原始读数（WMI `root\wmi:MSAcpi_ThermalZoneTemperature`）。
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

/* --------------------- 厂商通道：联想 ACPI-WMI 风扇 --------------------- */

/// 联想风扇读数所用的 WMI 方法与命名空间。
///
/// 背景（2026-10-10 分析 LegionZone 后确认）：联想**没有**把风扇转速放在 Super I/O 上，
/// 而是通过 **ACPI-WMI 映射设备（`ACPI\PNP0C14`）** 暴露到 `root\WMI`：
///
/// ```text
/// Fan_GetCurrentFanSpeed(FanID: u8) -> CurrentFanSpeed: u16   // 单位 RPM
/// ```
///
/// ⚠️ 与最初判断不同，**实测发现调用该方法是需要管理员权限的**：
/// 非提权时 `ExecMethod` 返回 `WBEM_E_ACCESS_DENIED(0x80041003)`，
/// 提权后同一调用立即成功（`CurrentFanSpeed = 3003`，本机 2026-10-10 实测）。
/// 因此本通道**不能**在主进程里直接调，必须走提权桥接子进程
/// （`msr_bridge.rs`，与 PawnIO 共用一次 UAC）。
///
/// 该类是**方法类（实例数为 0）**，且方法必须挂在 ACPI 实例路径上执行
/// （`InstanceName` 取自 `LENOVO_FAN_TABLE_DATA` 的实例，如 `ACPI\PNP0C14\GMZN_0`）。
const LENOVO_NS: &str = "root\\WMI";
const LENOVO_FAN_CLASS: &str = "LENOVO_FAN_METHOD";
/// 提供 ACPI 实例名的数据类（本机有 3 个实例：GMZN_0 / GMZN_1 / GMZN_2）
const LENOVO_TABLE_CLASS: &str = "LENOVO_FAN_TABLE_DATA";

/// 单次风扇读取的合理上限（RPM）。超过这个值说明解码错了或机器异常，
/// 宁可判为「读不到」也不要显示垃圾数据。12000 足以覆盖消费级笔记本风扇量级。
const FAN_RPM_SANE_MAX: f64 = 12_000.0;

/// 是否是联想设备。
///
/// 判定依据是**机型**而非「是否装了 LegionZone」——因为本通道来自 BIOS/EC，
/// 与联想软件是否安装无关。读取 `root\cimv2:Win32_ComputerSystem.Manufacturer`，
/// 大小写不敏感地匹配 `LENOVO`。
///
/// 结果在进程内缓存：机型不会变，且 WMI 查询不便宜。
pub fn is_lenovo() -> bool {
    use std::sync::OnceLock;
    static CACHE: OnceLock<bool> = OnceLock::new();
    *CACHE.get_or_init(|| {
        let Some(rows) = query(
            "root\\cimv2",
            "SELECT Manufacturer FROM Win32_ComputerSystem",
            &["Manufacturer"],
        ) else {
            diag("读取 Win32_ComputerSystem.Manufacturer 失败，按非联想处理");
            return false;
        };
        let hit = rows
            .iter()
            .filter_map(|r| r.get("Manufacturer").and_then(|p| p.as_string()))
            .any(|m| m.to_ascii_lowercase().contains("lenovo"));
        diag(&format!("是否联想设备: {hit}"));
        hit
    })
}

/// 解析 `LENOVO_FAN_METHOD` 方法该挂到哪个 ACPI 实例路径上。
///
/// 方法类本身没有实例，`ExecMethod` 却要求一个实例路径。实测该路径的可用形态是
/// `LENOVO_FAN_METHOD.InstanceName="ACPI\PNP0C14\GMZN_0"`——实例名从
/// `LENOVO_FAN_TABLE_DATA` 取（它与方法类同属一个 ACPI-WMI 设备）。
///
/// 返回 `None` 表示该机型没暴露 `LENOVO_FAN_TABLE_DATA`（老 BIOS 常见）。
///
/// 结果缓存：实例名不会变。
pub fn lenovo_fan_instance() -> Option<String> {
    use std::sync::OnceLock;
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            if !is_lenovo() {
                return None;
            }
            let rows = query(
                LENOVO_NS,
                &format!("SELECT InstanceName FROM {LENOVO_TABLE_CLASS}"),
                &["InstanceName"],
            )?;
            let name = rows
                .iter()
                .filter_map(|r| r.get("InstanceName").and_then(|p| p.as_string()))
                .find(|s| !s.trim().is_empty());
            diag(&format!("联想风扇 ACPI 实例: {name:?}"));
            name
        })
        .clone()
}

/// 读**某一个**风扇的当前转速（RPM）。
///
/// `fan_id`：联想的逻辑风扇号。实测本机 `Fan_Id` 取值为 0（CPU）与 1（GPU）。
/// 返回 `None` 表示通道不可用（含**未提权**）、方法返回失败码、或数值超出合理区间。
///
/// 生产路径不用这个函数——它自己会建一次 WMI 连接，逐个调很浪费；
/// 轮询一律走 [`lenovo_fans`]（一次连接读全部）。这里保留是给
/// `diag::probe_fan` 逐个 FanID 打印读数用。
#[allow(dead_code)]
pub fn lenovo_fan_rpm(fan_id: u8) -> Option<f32> {
    let instance = lenovo_fan_instance()?;
    let out = call_method_u8(
        LENOVO_NS,
        LENOVO_FAN_CLASS,
        &instance,
        "Fan_GetCurrentFanSpeed",
        &[("FanID", fan_id)],
    )?;
    fan_rpm_from(&out, fan_id)
}

/// 从 `Fan_GetCurrentFanSpeed` 的输出里取转速。
///
/// `0` 表示风扇停转或该风扇号不存在，两者在联想的接口里**无法区分** → 一律视为「读不到」，
/// 宁可显示 `--` 也不把 0 当成真实转速。
fn fan_rpm_from(out: &[(String, f64)], fan_id: u8) -> Option<f32> {
    let v = out
        .iter()
        .find(|(k, _)| k == "CurrentFanSpeed")
        .map(|(_, v)| *v)?;
    if !v.is_finite() || v <= 0.0 || v > FAN_RPM_SANE_MAX {
        diag(&format!("FanID={fan_id} 读数不合理: {v}"));
        return None;
    }
    Some(v as f32)
}

/// 读联想的传感器温度（摄氏度）。
///
/// 联想的 `Fan_GetCurrentSensorTemperature` 返回的是**整数值**，实测其量纲为
/// 摄氏度（曲线表里的阈值 32~110 与之对应）。仅作交叉校验用，不作为温度主源。
///
/// 当前 CPU 温度主源是 PawnIO 直读片内传感器（Intel MSR / AMD SMN，见 `thermal.rs`），
/// 本函数**尚未接线**；
/// 保留是因为它与风扇同属一次提权调用，将来要做温度交叉校验时可直接用
/// （`diag::probe_fan` 会打印各 SensorID 的读数，便于判断可用性）。
#[allow(dead_code)]
pub fn lenovo_sensor_temp(sensor_id: u8) -> Option<f32> {
    let instance = lenovo_fan_instance()?;
    let out = call_method_u8(
        LENOVO_NS,
        LENOVO_FAN_CLASS,
        &instance,
        "Fan_GetCurrentSensorTemperature",
        &[("SensorID", sensor_id)],
    )?;
    let v = out
        .iter()
        .find(|(k, _)| k == "CurrentSensorTemperature")
        .map(|(_, v)| *v)?;
    // 合理温度区间：低于 -20 或高于 150 一律判为无效
    if !(-20.0..=150.0).contains(&v) {
        return None;
    }
    Some(v as f32)
}

/// 本机实际存在的风扇号（升序、去重）。**保证至少含 `[0, 1]`**（联想的 CPU / GPU 编号约定）。
///
/// 来源是 `LENOVO_FAN_TABLE_DATA.Fan_Id`（本机 3 个实例的取值为 0/0/1 → 去重后 `[0, 1]`，
/// 与 `Fan_GetCurrentFanSpeed(FanID)` 是同一套编号）。
///
/// 这样做是为了**不再盲探 0..=3**：每一个不存在的风扇号都要白白付一次 `ExecMethod` 往返，
/// 而提权桥接进程每秒都跑一趟。表里读不到风扇号时退回 `[0, 1]`，与旧行为等效。
///
/// 结果缓存：机型不变。
pub fn lenovo_fan_ids() -> Vec<u8> {
    use std::sync::OnceLock;
    static CACHE: OnceLock<Vec<u8>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let mut ids: Vec<u8> = query(
                LENOVO_NS,
                &format!("SELECT Fan_Id FROM {LENOVO_TABLE_CLASS}"),
                &["Fan_Id"],
            )
            .map(|rows| {
                rows.iter()
                    .filter_map(|r| r.get("Fan_Id").and_then(|p| p.as_f64()))
                    .filter(|n| (0.0..=255.0).contains(n))
                    .map(|n| n as u8)
                    .collect()
            })
            .unwrap_or_default();
            // CPU / GPU 是固定约定，无论表里有没有都探一下；最坏情况也只多两次调用
            for id in [0u8, 1] {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
            ids.sort_unstable();
            ids.dedup();
            diag(&format!("联想风扇号: {ids:?}"));
            ids
        })
        .clone()
}

/// 一次拿到联想机型上所有可用的风扇读数。
///
/// 只探测 [`lenovo_fan_ids`] 给出的风扇号（本机 `[0, 1]`），跳过读不到的。
/// 返回按 `FanID` 升序的 `(fan_id, rpm)` 列表。
///
/// ⚠️ **只建一次 WMI 连接**：这里刻意不逐个调 [`lenovo_fan_rpm`]，因为那个函数每次都会
/// `connect()`（内含 `ConnectServer`，是本链路最贵的一步）。提权桥接进程每秒都要跑一趟，
/// 逐个调用等于每秒建 4 次连接——实测稳定占用约 5% 单核，对一个常驻监控工具完全是浪费。
pub fn lenovo_fans() -> Vec<(u8, f32)> {
    let Some(instance) = lenovo_fan_instance() else {
        return Vec::new();
    };
    let Some((_com, svc)) = connect(LENOVO_NS) else {
        return Vec::new();
    };
    let Some(class_obj) = get_class(&svc, LENOVO_FAN_CLASS) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for id in lenovo_fan_ids() {
        let Some(vals) = exec_method(
            &svc,
            &class_obj,
            LENOVO_FAN_CLASS,
            &instance,
            "Fan_GetCurrentFanSpeed",
            &[("FanID", id)],
        ) else {
            // 该风扇号读不到（可能未提权、也可能是停转/不存在）。静默跳过，交由上层降级。
            continue;
        };
        if let Some(rpm) = fan_rpm_from(&vals, id) {
            out.push((id, rpm));
        }
    }
    out
}

/// 联想风扇曲线的最大转速（RPM），用于把读数标注成「上限 N RPM」。
///
/// 数据来自 `LENOVO_FAN_TABLE_DATA.CurrentFanMaxSpeed`（实测本机 4500）。
/// 该值由 BIOS 固定，**读一次即可**——结果在进程内缓存，避免每拍都去查 WMI
/// （主进程现在每拍都要用它拼来源文案，不缓存等于每秒一次 WMI 查询）。
///
/// 非提权也能读（数据类不受 ACPI-WMI 方法调用的权限限制），实测确认。
pub fn lenovo_fan_max_speed() -> Option<f32> {
    use std::sync::OnceLock;
    static CACHE: OnceLock<Option<f32>> = OnceLock::new();
    *CACHE.get_or_init(|| {
        let rows = query(
            LENOVO_NS,
            "SELECT CurrentFanMaxSpeed FROM LENOVO_FAN_TABLE_DATA",
            &["CurrentFanMaxSpeed"],
        )?;
        rows.iter()
            .filter_map(|r| r.get("CurrentFanMaxSpeed").and_then(|p| p.as_f64()))
            .find(|v| *v > 0.0 && *v <= FAN_RPM_SANE_MAX)
            .map(|v| v as f32)
    })
}

