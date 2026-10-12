//! 风扇的**用户手动指定**配置（落盘 `%LOCALAPPDATA%\PulseDeck\fan.json`）
//!
//! ## 为什么需要它
//!
//! 风扇转速没有通用接口，各家（包括本工具的联想 ACPI-WMI 通道）都只能靠
//! 「厂商给的编号约定」猜哪个风扇是 CPU 的。约定对不上时，用户比我们更清楚
//! 自己机器上哪个是 CPU 风扇——所以给一个手动指定的口子，让人能纠正自动判断。
//!
//! 这个思路来自竞品 LiteMonitor（`FanMapper.cs`）：它把「用户手动指定」放在
//! 映射链的最高层，只在没指定时才走启发式猜测。本模块保留同样的优先级，
//! 但只暴露**一个**选项（CPU 风扇号），因为本工具当前的通道里只有 FanID 这一
//! 个可辨识维度——等将来接进带名字的传感器通道，再按需扩展。
//!
//! ## 存储
//!
//! 与 `pet.json` 同目录、同写法（临时文件 + rename 的原子写）。
//! 读不到/损坏时静默退回默认（自动），不做任何打扰。

use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

const FILE_NAME: &str = "fan.json";

/// FanID 是 `u8`，但一台机器不可能挂 8 个以上可报告转速的风扇。
/// 超出范围的取值视为脏数据，清洗成「自动」。
const MAX_FAN_ID: u8 = 7;

/// 风扇相关的用户偏好（前端 JSON 用 camelCase）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FanPref {
    /// 手动指定的 CPU 风扇号；`None` = 自动（按 FanID 约定取 0 号）
    pub cpu_fan_id: Option<u8>,
}

impl FanPref {
    /// 清洗：越界的号退回「自动」，永远不返回非法配置
    fn sanitize(&mut self) {
        if self.cpu_fan_id.is_some_and(|id| id > MAX_FAN_ID) {
            self.cpu_fan_id = None;
        }
    }
}

/// 配置的共享句柄（采样线程与命令线程共用一份）
#[derive(Clone)]
pub struct FanPrefStore(Arc<Mutex<FanPref>>);

impl FanPrefStore {
    /// 从磁盘恢复；文件不存在或损坏时退回「自动」
    pub fn load() -> Self {
        let parsed = std::fs::read_to_string(config_path())
            .ok()
            .and_then(|s| serde_json::from_str::<FanPref>(&s).ok());
        let mut pref = parsed.unwrap_or_default();
        pref.sanitize();
        Self(Arc::new(Mutex::new(pref)))
    }

    /// 纯内存实例：**只给测试用**，不落盘也不读盘
    #[cfg(test)]
    pub fn in_memory(cpu_fan_id: Option<u8>) -> Self {
        Self(Arc::new(Mutex::new(FanPref { cpu_fan_id })))
    }

    pub fn get(&self) -> FanPref {
        *self.lock()
    }

    /// 落盘并返回清洗后的配置（前端回显以这份为准）
    pub fn set(&self, mut pref: FanPref) -> FanPref {
        pref.sanitize();
        *self.lock() = pref;
        self.save(&pref);
        pref
    }

    fn save(&self, pref: &FanPref) {
        let path = config_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // 原子写：临时文件 + rename，避免写到一半被关机打断留下坏配置
        let tmp = path.with_extension("json.tmp");
        if serde_json::to_string_pretty(pref)
            .ok()
            .and_then(|s| std::fs::write(&tmp, s).ok())
            .is_some()
        {
            let _ = std::fs::rename(&tmp, &path);
        }
    }

    /// 锁中毒不该把整个应用带崩（release 是 panic=abort），所以直接取回内部数据
    fn lock(&self) -> MutexGuard<'_, FanPref> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn config_path() -> std::path::PathBuf {
    crate::app_dir().join(FILE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_auto() {
        assert_eq!(FanPref::default().cpu_fan_id, None);
    }

    /// 越界的风扇号退回「自动」，而不是拿去匹配一个不存在的风扇
    #[test]
    fn set_sanitizes_out_of_range_id() {
        let store = FanPrefStore::in_memory(None);
        let got = store.set(FanPref {
            cpu_fan_id: Some(99),
        });
        assert_eq!(got.cpu_fan_id, None);
        assert_eq!(store.get().cpu_fan_id, None);
    }

    #[test]
    fn set_keeps_valid_id() {
        let store = FanPrefStore::in_memory(None);
        let got = store.set(FanPref {
            cpu_fan_id: Some(1),
        });
        assert_eq!(got.cpu_fan_id, Some(1));
        // 内存实例也应当立刻生效（采样线程读的是同一份）
        assert_eq!(store.get().cpu_fan_id, Some(1));
    }

    /// 边界：MAX_FAN_ID 本身合法
    #[test]
    fn max_id_is_allowed() {
        let mut pref = FanPref {
            cpu_fan_id: Some(MAX_FAN_ID),
        };
        pref.sanitize();
        assert_eq!(pref.cpu_fan_id, Some(MAX_FAN_ID));
    }

    /// 前端传来的 JSON key 是 camelCase，字段名要对齐
    #[test]
    fn deserializes_camel_case() {
        let p: FanPref = serde_json::from_str(r#"{"cpuFanId": 2}"#).unwrap();
        assert_eq!(p.cpu_fan_id, Some(2));
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("cpuFanId"), "实际序列化结果：{json}");
    }
}
