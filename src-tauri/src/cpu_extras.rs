//! CPU 附加读数：**实时**主频（Windows 专属）
//!
//! Windows 上 sysinfo 的 `Cpu::frequency()` 取自 `CallNtPowerInformation` 的
//! `PROCESSOR_POWER_INFORMATION.CurrentMhz`，实测**恒为标称频率**（本机 i7-12700H
//! 恒定 2300 MHz，打满也不动，与 PDH `Processor Frequency` 同值），
//! 真实运行频率要从 PDH 的 `Actual Frequency` 读（实测 2.4~3.5 GHz 随负载变化，
//! 与任务管理器显示的「速度」同量级、同趋势）。
//!
//! **这里曾经还有一路 `% Processor Utility`（当时当成「任务管理器口径」）**：
//! 2026-10-16 用受控负载 + 任务管理器截图实测推翻了那个前提——任务管理器的读数
//! 跟的是**忙时占比**（与本工具的总占用同口径：空载 TM 20% vs 忙时 12~15% vs
//! utility 24.5%；16 线程满载 TM 94% vs 忙时 96~99% vs utility 103~106%），
//! 而 `% Processor Utility` 恒等于「忙时占比 × 实时频率 / 标称频率」，稳定高出
//! 5%~50%（睿频越猛差得越多），**不是任务管理器口径**。windows_exporter 文档里
//! 「utility 对应任务管理器」的说法在本机实测不成立，界面上不再摆这个读数
//! （结论与数据见 AGENTS.md「Windows 上的已知坑」）。
//!
//! 计数器在 `Processor Information` 集合里（`Processor` 集合连这个集合名下的
//! 计数器都没有，实测加不上）。实例是每个逻辑处理器一行再加合计行 `_Total`，
//! 这里只取 `_Total`：主频看整机。

use windows::Win32::System::Performance::PDH_HCOUNTER;

use crate::pdh::Query;
use crate::platform::CpuExtrasReading;

/// 实时主频（MHz）
const FREQ_PATH: &str = r"\Processor Information(*)\Actual Frequency";

/// `Actual Frequency` 的 PDH 查询
struct PdhCpu {
    query: Query,
    /// 加不上就是读不到（老系统未必有这个计数器），如实降级
    freq: Option<PDH_HCOUNTER>,
}

// PDH 句柄只在同一个线程内使用，跨线程移交所有权是安全的（同 gpu.rs 的 PdhGpu）
unsafe impl Send for PdhCpu {}

pub struct CpuExtrasProbe {
    /// 建不起来就是读不到：如实降级，不做重试——PDH 的可用性在运行期不会变
    pdh: Option<PdhCpu>,
}

impl Default for CpuExtrasProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl CpuExtrasProbe {
    pub fn new() -> Self {
        Self { pdh: build() }
    }

    pub fn sample(&mut self) -> CpuExtrasReading {
        let Some(p) = self.pdh.as_ref() else {
            return CpuExtrasReading::default();
        };
        // `Actual Frequency` 是即时量（不是需要两拍求差的速率型），
        // 单拍即可读；能读是因为 `build()` 已预收过一拍（首拍无效）
        p.query.collect();
        let freq = p.freq.and_then(|c| instance_total(&p.query.double_array(c)));

        CpuExtrasReading {
            live_freq_mhz: freq.map(|v| v.round() as u32).filter(|mhz| *mhz > 0),
            // sysinfo 在 Windows 上给的是标称频率，不是实时值
            sysinfo_freq_live: false,
        }
    }
}

fn build() -> Option<PdhCpu> {
    let query = Query::new()?;
    let freq = query.add(FREQ_PATH);
    // 预收一拍：PDH 计数器首拍读不出有效值（实测首拍 CStatus 无效、第二拍才有数），
    // 先空收一次能让界面第一帧就是实时值，而不是先摆一拍标称频率再跳过去
    query.collect();
    Some(PdhCpu { query, freq })
}

/// 从 PDH 实例表里挑出合计行 `_Total`（每个逻辑处理器各一行，外加这一行合计）
fn instance_total(items: &[(String, f64)]) -> Option<f64> {
    items
        .iter()
        .find(|(name, _)| name == "_Total")
        .map(|(_, value)| *value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, value: f64) -> (String, f64) {
        (name.to_string(), value)
    }

    #[test]
    fn instance_total_picks_the_total_row_only() {
        let rows = vec![row("0", 3.0), row("1", 9.0), row("_Total", 42.5)];
        assert_eq!(instance_total(&rows), Some(42.5));
        // 只有单核实例时不能拿第一行充合计——宁可报读不到，也不把单核当整机
        assert_eq!(instance_total(&[row("0", 3.0)]), None);
        assert_eq!(instance_total(&[]), None);
    }
}
