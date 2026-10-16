//! WebSocket 客户端连接（数据推送的浏览器通道），基于同步的 `tungstenite`
//!
//! 与裸 TCP（NDJSON）客户端**共用同一个监听端口**：worker accept 后先做
//! 250ms 的协议判定（见 `super::sniff_proto`），首字节长得像 HTTP 请求行的
//! 连接交到这里的 [`WsClient::spawn`]。浏览器 / 手机网页因此无需任何原生
//! App，`new WebSocket("ws://<ip>:17432/")` 直接收推送。
//!
//! 线程模型与 BLE（`ble.rs`）同一哲学：**worker 持句柄、连接各自一条线程、
//! `try_send` 不反压**——同步侧把序列化好的 JSON 塞进每条连接的通道，
//! 连接线程负责握手 / 推帧 / 答 Ping / 觉察对端关闭。停止信号不需要额外的
//! 标志位：worker 退出时 drop 全部句柄，通道断连（`Disconnected`）就是
//! 「礼貌收尾」的信号，与 BLE 的 Drop-置位异曲同工。
//!
//! 只出不进的边界与 NDJSON 通道一致：客户端发来的**内容一律不解析**——
//! 读入向只是为了答 Ping（tungstenite 自动排队 Pong，下一次 read/send 顺带
//! flush）与察觉 Close。无子协议、路径忽略。
//!
//! 掉线 / 慢消费者的判定（对齐 NDJSON 通道「写超时 1s 剔除」）：
//! 写失败 / 写超时、队列积压 [`QUEUE`] 拍未消费、对端断开 → 连接线程退出
//! → 接收端 drop → worker 下一次 `send` 得 `Disconnected` → 剔除句柄。

use std::io::ErrorKind;
use std::net::TcpStream;
use std::sync::mpsc::sync_channel;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::SyncSender;
use std::time::Duration;

use tungstenite::Message;

/// 每条连接的排队能力（拍）：慢消费者容忍度。最小间隔 300ms 下即 ~2.4s，
/// 再消化不掉就断开——对齐 NDJSON 通道 1s 写超时的「不养慢消费者」取舍，
/// 客户端（浏览器）重连成本极低。
const QUEUE: usize = 8;

/// 步进节拍：出向等待与入向读各一次，最坏一拍载荷延迟 ~200ms，
/// 与 worker / BLE 的 100~150ms 步进同一量级。
const STEP: Duration = Duration::from_millis(100);

/// worker 侧持有的连接句柄：往这条连接泵载荷，连接死了 [`Self::send`]
/// 返回 false，由调用方就地剔除。
pub(super) struct WsClient {
    tx: SyncSender<String>,
}

impl WsClient {
    /// 在这条（已判定为 WebSocket 的）连接上起服务线程。握手在后台完成，
    /// 失败（非 WebSocket 的 HTTP 探测者 / 残缺请求）时连接静默关闭——
    /// worker 下一次 `send` 才会经 `Disconnected` 发现。
    pub(super) fn spawn(stream: TcpStream) -> Self {
        let (tx, rx) = sync_channel(QUEUE);
        std::thread::Builder::new()
            .name("pd-push-ws".into())
            .spawn(move || serve(stream, rx))
            .ok(); // 起线程失败：通道无人接，send 恒 false，下一拍被剔除
        Self { tx }
    }

    /// 泵一拍载荷（UTF-8 JSON，无换行）。队列满（慢消费者）或连接已关都返回
    /// false = 该剔除了；推送尽力而为，绝不反压采样线程。
    pub(super) fn send(&self, payload: &str) -> bool {
        self.tx.try_send(payload.to_string()).is_ok()
    }
}

/// 一条 WebSocket 连接的服务循环：握手 → 出向推帧 / 入向答 Ping。
fn serve(stream: TcpStream, rx: Receiver<String>) {
    // 握手阶段给足时间：慢客户端的 HTTP 头可能分多个包到（5s）
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
    let mut ws = match tungstenite::accept(stream) {
        Ok(ws) => ws,
        Err(_) => return, // 不是合法的 WebSocket 升级请求：直接断，不解释
    };
    // 建立后转入步进节拍：读超时让「无入向数据」不阻塞，写超时保住剔除判据
    let _ = ws.get_ref().set_read_timeout(Some(STEP));

    loop {
        match rx.recv_timeout(STEP) {
            Ok(json) => {
                if ws.send(Message::text(json)).is_err() {
                    break; // 写失败 / 超时（1s）= 掉线或慢消费者
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            // worker 已退出（句柄全部 drop）：礼貌关闭后收尾
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
        match ws.read() {
            // 对端主动关闭：回敬 Close 后断开（RFC 6455 的礼貌收尾）
            Ok(Message::Close(_)) => break,
            // Ping 由 tungstenite 自动排队 Pong（下一次 read/send 顺带发出）；
            // Text / Binary / Pong 是客户端发来的内容——按只出不进原则不解析
            Ok(_) => {}
            Err(tungstenite::Error::Io(ref e)) if reversible(e) => {}
            // 协议错误 / 连接重置 / 其余 IO 错误：判掉线
            Err(_) => break,
        }
    }
    let _ = ws.send(Message::Close(None)); // 尽力而为的收尾（已断时无效果）
}

/// 步进读入向时，「没有数据」类的 IO 错误可以继续，其余一律视为掉线。
/// tungstenite 文档口径：WouldBlock 视为可重试；read 超时在阻塞 + 超时模式下
/// 表现为同族错误，同样只是「这一拍没数据」。
fn reversible(e: &std::io::Error) -> bool {
    e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut
}

/* ------------------------------ 测试 ------------------------------ */

#[cfg(test)]
mod tests {
    use super::*;

    /// 最小链路：起一条本地回环的 WS 连接对（客户端 tungstenite::client +
    /// 服务端 serve 线程），验证握手、推帧、礼貌关闭全链路成立。
    /// 端口嗅探 / accept 分流由 mod.rs 的 e2e 测试覆盖，这里只测 serve 本身。
    #[test]
    fn ws_serve_pushes_and_closes_politely() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();

        let client_side = std::thread::spawn(move || {
            // URL 只决定 Host 头，连接用的是下面这条已建好的流
            let stream = TcpStream::connect(addr).unwrap();
            let (mut ws, _) = tungstenite::client("ws://127.0.0.1/", stream).unwrap();
            let _ = ws.get_ref().set_read_timeout(Some(Duration::from_secs(5)));
            let text = ws.read().unwrap().into_text().unwrap().to_string();
            // 服务端 drop 句柄后应收到 Close
            let second = ws.read().unwrap();
            (text, second)
        });

        let (server_stream, _) = listener.accept().unwrap();
        let handle = WsClient::spawn(server_stream);
        assert!(handle.send("{\"schema\":1}"), "活连接的第一次 send 必须成功");
        // 给客户端一点时间收到帧，然后 drop 句柄触发礼貌关闭
        std::thread::sleep(Duration::from_millis(300));
        drop(handle);

        let (text, second) = client_side.join().unwrap();
        assert_eq!(text, "{\"schema\":1}");
        assert!(
            matches!(second, Message::Close(_)),
            "drop 句柄后应收到 Close，实际 {second:?}"
        );
    }
}
