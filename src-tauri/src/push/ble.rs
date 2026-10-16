//! BLE GATT 串口服务端（Nordic UART Service），基于 `ble-peripheral-rust`
//!
//! 选型说明（2026-10-17 定）：**btleplug 只有 central 角色，做不了 GATT 服务端**；
//! 本模块用的 ble-peripheral-rust 是 Rust 生态里唯一跨平台的 BLE 外设角色库
//! （Windows = WinRT `GattServiceProvider`，Linux = bluer / BlueZ D-Bus，macOS =
//! CoreBluetooth），需要 tokio 运行时——由本模块自带的 current-thread 运行时满足，
//! 不占用也不依赖 tauri 的全局运行时。
//!
//! 线程模型：`BleServer::start()` 起一条专用线程，线程内建 tokio 运行时并跑
//! 异步循环（建服务 → 广播 → `select!` 收发）。同步侧（推送 worker）通过
//! tokio mpsc 的 `try_send` 把载荷塞进来——BLE 跟不上节拍时丢弃本拍，绝不
//! 反压采样线程。订阅者计数来自平台的 `CharacteristicSubscriptionUpdate` 事件，
//! 状态行据此如实显示「已订阅 N 台设备」。
//!
//! 广播名固定 `PulseDeck`，服务用 NUS 标准 UUID——现成的 BLE 串口客户端
//! （nRF Connect、SerialBluetoothTerminal、ESP32 NimBLE 等）无需适配即可连。

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ble_peripheral_rust::gatt::characteristic::Characteristic;
use ble_peripheral_rust::gatt::peripheral_event::{
    PeripheralEvent, ReadRequestResponse, RequestResponse, WriteRequestResponse,
};
use ble_peripheral_rust::gatt::properties::{AttributePermission, CharacteristicProperty};
use ble_peripheral_rust::gatt::service::Service;
use ble_peripheral_rust::{Peripheral, PeripheralImpl};
use uuid::Uuid;

/// 广播名（设备端扫描时看到的名字）
pub const BLE_NAME: &str = "PulseDeck";

/// Nordic UART Service 的三个标准 UUID
const NUS_SERVICE: &str = "6e400001-b5a3-f393-e0a9-e50e24dcca9e";
/// 设备 → 本机（write）。协议只出不进：收到的内容一律不解析，仅按成功应答
const NUS_RX: &str = "6e400002-b5a3-f393-e0a9-e50e24dcca9e";
/// 本机 → 设备（notify）：推送载荷按 20 字节安全帧切发（见 `super::ble_frames`）
const NUS_TX: &str = "6e400003-b5a3-f393-e0a9-e50e24dcca9e";

fn uuid_of(s: &str) -> Uuid {
    Uuid::parse_str(s).expect("内置 NUS UUID 必须合法")
}

fn nus_service() -> Service {
    Service {
        uuid: uuid_of(NUS_SERVICE),
        primary: true,
        characteristics: vec![
            Characteristic {
                uuid: uuid_of(NUS_RX),
                properties: vec![
                    CharacteristicProperty::Write,
                    CharacteristicProperty::WriteWithoutResponse,
                ],
                permissions: vec![AttributePermission::Writeable],
                value: None,
                descriptors: Vec::new(),
            },
            Characteristic {
                uuid: uuid_of(NUS_TX),
                properties: vec![CharacteristicProperty::Notify],
                permissions: Vec::new(),
                value: None,
                descriptors: Vec::new(),
            },
        ],
    }
}

/// BLE 侧的活状态（异步循环写、worker 读）
#[derive(Default)]
struct BleState {
    advertising: bool,
    error: String,
    /// 订阅了 notify 的远端设备（平台事件驱动增删，client 是平台给的设备标识）
    subscribers: HashSet<String>,
}

/// 同步侧持有的 BLE 服务端句柄：线程 + 状态 + 载荷通道。
///
/// `Drop` 即停（置停止位；线程最长 ~150ms 自行退出，广播随之停掉）。
/// 旧新两代服务端的交接靠 [`BLE_TURNSTILE`] 串行化，见其注释。
pub struct BleServer {
    tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    state: Arc<Mutex<BleState>>,
    stop: Arc<AtomicBool>,
}

impl BleServer {
    /// 起后台线程建 NUS 服务并广播。**不会等启动完成**——结果异步落到
    /// `snapshot()`（advertising=false 且 error 为空 = 还在启动中）。
    pub fn start() -> Self {
        let (tx, rx) = tokio::sync::mpsc::channel(2);
        let state = Arc::new(Mutex::new(BleState::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_state = state.clone();
        let thread_stop = stop.clone();
        std::thread::Builder::new()
            .name("pd-push-ble".into())
            .spawn(move || run_thread(thread_stop, thread_state, rx))
            .expect("起 BLE 线程");
        Self { tx, state, stop }
    }

    /// 推一段载荷（UTF-8 JSON，不带换行）。BLE 侧按 20 字节安全帧切发 notify；
    /// 通道满（BLE 跟不上节拍）时丢弃本拍——推送尽力而为，不阻塞采样线程。
    pub fn push(&self, payload: &[u8]) {
        let _ = self.tx.try_send(payload.to_vec());
    }

    /// 当前订阅 notify 的远端设备数
    pub fn subscribers(&self) -> usize {
        self.lock_state().subscribers.len()
    }

    /// (广播中, 错误信息, 订阅数)——worker 每拍合进 PushStatus
    pub fn snapshot(&self) -> (bool, String, usize) {
        let st = self.lock_state();
        (st.advertising, st.error.clone(), st.subscribers.len())
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, BleState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Drop for BleServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // 不 join：线程最长 ~150ms 后自行退出；与新一代服务端的串行交接
        // 由 BLE_TURNSTILE 保证（它退出时才释放闸门）
    }
}

/// 全局闸门：同一时刻只允许一代 BLE 服务端持有蓝牙（WinRT 上两个
/// GattServiceProvider 用同一个服务 UUID 会互相打架）。
/// 「停旧起新」时旧线程 ≤150ms 退出现身，新线程在闸门外最多等 5s。
static BLE_TURNSTILE: AtomicBool = AtomicBool::new(false);

struct Turnstile;

impl Turnstile {
    /// 拿不到（停止位已置 / 超时）返回 None
    fn acquire(stop: &AtomicBool) -> Option<Turnstile> {
        for _ in 0..50 {
            if !BLE_TURNSTILE.swap(true, Ordering::AcqRel) {
                return Some(Turnstile);
            }
            if stop.load(Ordering::Relaxed) {
                return None;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        None
    }
}

impl Drop for Turnstile {
    fn drop(&mut self) {
        BLE_TURNSTILE.store(false, Ordering::Release);
    }
}

fn run_thread(
    stop: Arc<AtomicBool>,
    state: Arc<Mutex<BleState>>,
    payload_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
) {
    let _gate = match Turnstile::acquire(&stop) {
        Some(g) => g,
        // 停止位已置：还没开跑就该退场，不留言
        None => return,
    };
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            state.lock().unwrap_or_else(|e| e.into_inner()).error =
                format!("创建异步运行时失败：{e}");
            return;
        }
    };
    if let Err(e) = rt.block_on(ble_loop(&stop, &state, payload_rx)) {
        state.lock().unwrap_or_else(|e| e.into_inner()).error = e;
    }
}

async fn ble_loop(
    stop: &AtomicBool,
    state: &Mutex<BleState>,
    mut payload_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
) -> Result<(), String> {
    let es = |e: ble_peripheral_rust::error::Error| format!("{e}");

    let (evt_tx, mut evt_rx) = tokio::sync::mpsc::channel(64);
    let mut peripheral = Peripheral::new(evt_tx).await.map_err(es)?;

    // 等射频上电（蓝牙可能被关着 / 适配器正在初始化），最多 5s
    let mut powered = false;
    for _ in 0..25 {
        if peripheral.is_powered().await.map_err(es)? {
            powered = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    if !powered {
        return Err("蓝牙适配器未就绪（可能已关闭或不存在）".into());
    }

    peripheral.add_service(&nus_service()).await.map_err(es)?;
    peripheral
        .start_advertising(BLE_NAME, &[uuid_of(NUS_SERVICE)])
        .await
        .map_err(|e| format!("启动广播失败：{e}"))?;
    state.lock().unwrap_or_else(|e| e.into_inner()).advertising = true;

    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(150)) => {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
            }
            ev = evt_rx.recv() => {
                match ev {
                    None => break,
                    Some(PeripheralEvent::CharacteristicSubscriptionUpdate { request, subscribed }) => {
                        let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
                        if subscribed {
                            st.subscribers.insert(request.client);
                        } else {
                            st.subscribers.remove(&request.client);
                        }
                    }
                    // 协议只出不进：任何写入按成功应答、内容一律不解析
                    Some(PeripheralEvent::WriteRequest { responder, .. }) => {
                        let _ = responder.send(WriteRequestResponse {
                            response: RequestResponse::Success,
                        });
                    }
                    Some(PeripheralEvent::ReadRequest { responder, .. }) => {
                        let _ = responder.send(ReadRequestResponse {
                            value: Vec::new(),
                            response: RequestResponse::RequestNotSupported,
                        });
                    }
                    _ => {}
                }
            }
            p = payload_rx.recv() => {
                match p {
                    None => break,
                    Some(bytes) => {
                        if !state.lock().unwrap_or_else(|e| e.into_inner()).subscribers.is_empty() {
                            for frame in super::ble_frames(&bytes) {
                                // 单帧失败不打断后续帧（设备可能刚断开），下一拍自然见分晓
                                let _ = peripheral.update_characteristic(uuid_of(NUS_TX), frame).await;
                            }
                        }
                    }
                }
            }
        }
    }

    state.lock().unwrap_or_else(|e| e.into_inner()).advertising = false;
    let _ = peripheral.stop_advertising().await;
    Ok(())
}

/* ------------------------------ 测试 ------------------------------ */

#[cfg(test)]
mod tests {
    use super::*;

    /// NUS UUID 串必须能解析（防手滑改错一个字符，运行时才炸）
    #[test]
    fn nus_uuids_parse() {
        let _ = uuid_of(NUS_SERVICE);
        let _ = uuid_of(NUS_RX);
        let _ = uuid_of(NUS_TX);
    }
}
