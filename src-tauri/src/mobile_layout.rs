//! 移动 App 端监控页的布局配置：桌面端「编辑布局 → 手机 / 平板」目标编辑的就是这份。
//!
//! 每个设备形态（手机 4 列 / 平板 6 列）各存**两份布局**：
//! - `flow`（流式）：App 按 `(y,x)` 阅读顺序排列，`w` 决定宽度、`h` 只决定精简/详细，
//!   卡片高度随内容增长（瀑布流）；
//! - `grid`（固定一屏）：手机 4 行 / 平板 6 行，App 把行高按可用高度均分、绝对定位，
//!   整块棋盘一屏不滚动。
//!
//! 每份布局还有一个**激活模式**（`mode`）：桌面编辑器切换模式后随布局消息下发，
//! App 只按激活模式渲染（协议「只出不进」，设备端没有上行通道）。每个设备仍然只发
//! **一条**布局消息（phone / pad 各一条），不新增消息条数。
//!
//! 双存储（与宠物 / 任务栏挂件同套路）：前端 `localStorage["pd.mlayout"]`（手机流式）/
//! `["pd.mlayoutPad"]`（平板流式）/ `["pd.mlayoutGrid"]`（手机固定）/
//! `["pd.mlayoutPadGrid"]`（平板固定）是编辑过程中的快速持久化；这里的
//! `mobile-layout.json` 是**推送 worker 的数据源**——worker 在 Rust 线程里读不到
//! localStorage，布局必须落到后端一份才能下发。前端提交后以 `set_mobile_layout`
//! 的返回值回显（后端会清洗）。
//!
//! 布局消息经推送通道三路下发（TCP/WS/BLE），结构见 `docs/推送协议.md`：
//! `{"schema":2,"type":"layout","device":"phone","mode":"flow","cols":4,"rowUnit":92,"widgets":[…]}`
//! （grid 模式额外带 `"rows":4`）。**cols（手机 4 / 平板 6）、rows（4 / 6）与组件类型
//! 白名单在前端 `src/modules/layout.js`、这里的 sanitize、App 端 `utils/protocol.uts`
//! 三处同步**（`public/mock-preview.js` 里还有第四份清洗副本）——改任何一处都要同步
//! 其余各处（与桌面布局「ROW_UNIT 两处同步」同款约定）。

use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Deserializer, Serialize};

/// 配置文件名（应用数据目录下，两种设备形态合一个文件）
const FILE_NAME: &str = "mobile-layout.json";

/// 手机形态网格列数
pub const PHONE_COLS: i32 = 4;
/// 平板形态网格列数
pub const PAD_COLS: i32 = 6;
/// 手机「固定一屏」的行数（4 列 × 4 行，像手机桌面小组件）
pub const PHONE_ROWS: i32 = 4;
/// 平板「固定一屏」的行数（6 列 × 6 行）
pub const PAD_ROWS: i32 = 6;
/// 流式编辑 / 旧客户端兜底用的行单元（逻辑像素）。固定一屏的新 App 不按它算高度，
/// 而是用 `rows` 把可用高度等分；这个值保留给旧 App 与 flow 网格编辑器。
pub const ROW_UNIT: i32 = 92;

/// 设备形态标识（消息 `device` 字段的取值，也是命令参数的取值）
pub const DEVICE_PHONE: &str = "phone";
pub const DEVICE_PAD: &str = "pad";

/// 布局模式：流式 / 固定一屏。缺省与未知值一律按流式处理——
/// 旧存档与旧桌面消息里没有这个字段，历史上就是流式渲染。
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LayoutMode {
    Flow,
    Grid,
}

impl Default for LayoutMode {
    fn default() -> Self {
        Self::Flow
    }
}

impl LayoutMode {
    /// 宽容解析：只认 "grid"（大小写不敏感），其余全部回退到 Flow。
    pub fn from_str_loose(raw: &str) -> Self {
        if raw.eq_ignore_ascii_case("grid") {
            Self::Grid
        } else {
            Self::Flow
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Flow => "flow",
            Self::Grid => "grid",
        }
    }
}

/// 手写反序列化而不是 derive：手改存档写了未知 mode 时，derive 会让整份
/// `mobile-layout.json` 解析失败、手机和平板布局一起回默认；这里只把该字段回退到 flow。
impl<'de> Deserialize<'de> for LayoutMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer).unwrap_or_default();
        Ok(Self::from_str_loose(&raw))
    }
}

/// 未知设备串一律按手机处理（命令参数防御；消息缺 `device` 字段时 App 端同款约定）
pub fn canonical_device(device: &str) -> &'static str {
    if device == DEVICE_PAD {
        DEVICE_PAD
    } else {
        DEVICE_PHONE
    }
}

/// 设备形态对应的网格列数
pub fn cols_of(device: &str) -> i32 {
    if canonical_device(device) == DEVICE_PAD {
        PAD_COLS
    } else {
        PHONE_COLS
    }
}

/// 设备形态对应的「固定一屏」行数
pub fn rows_of(device: &str) -> i32 {
    if canonical_device(device) == DEVICE_PAD {
        PAD_ROWS
    } else {
        PHONE_ROWS
    }
}

/// 组件类型白名单（与桌面端 11 种同款，两种设备形态共用）。顺序即桌面编辑器
/// chips 的展示序，与前端 `MOBILE_ORDER` 一致。
pub const MOBILE_WIDGET_TYPES: [&str; 11] = [
    "cpu",
    "mem",
    "bat",
    "gpu",
    "power",
    "net",
    "trend",
    "cores",
    "disk",
    "procCpu",
    "procMem",
];

/// 单个小组件。flow 模式下 `(y,x)` 只决定阅读顺序、`w` 决定宽度、`h` 只决定
/// 精简/详细；grid 模式下 `x/y/w/h` 是固定网格坐标（App 按 `x/cols`、
/// `y/rows`、`w/cols`、`h/rows` 的百分比绝对定位）。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MobileWidget {
    #[serde(rename = "type")]
    pub kind: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MobileLayout {
    #[serde(default = "default_v")]
    pub v: u8,
    #[serde(default)]
    pub widgets: Vec<MobileWidget>,
}

fn default_v() -> u8 {
    1
}

/// 一种设备的双布局 + 激活模式（`mobile-layout.json` 里 phone / pad 各一份）
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct DeviceLayouts {
    pub mode: LayoutMode,
    pub flow: MobileLayout,
    pub grid: MobileLayout,
}

/// 两种设备形态的布局合集（`mobile-layout.json` 的存档结构）
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct MobileLayouts {
    pub v: u8,
    pub phone: DeviceLayouts,
    pub pad: DeviceLayouts,
}

impl MobileLayout {
    /// 手机流式默认布局：六张基础卡两列排布（与 App 端内置默认一致；cores / procs
    /// 等需要额外推送组的组件不进默认——不勾组也能开箱即用）
    fn default_flow_phone() -> Self {
        Self {
            v: 1,
            widgets: vec![
                MobileWidget { kind: "cpu".into(), x: 0, y: 0, w: 2, h: 3 },
                MobileWidget { kind: "mem".into(), x: 2, y: 0, w: 2, h: 3 },
                MobileWidget { kind: "gpu".into(), x: 0, y: 3, w: 2, h: 3 },
                MobileWidget { kind: "bat".into(), x: 2, y: 3, w: 2, h: 3 },
                MobileWidget { kind: "net".into(), x: 0, y: 6, w: 2, h: 3 },
                MobileWidget { kind: "disk".into(), x: 2, y: 6, w: 2, h: 3 },
            ],
        }
    }

    /// 平板流式默认布局：六张基础卡一排两张（3 列宽），与手机版同构
    fn default_flow_pad() -> Self {
        Self {
            v: 1,
            widgets: vec![
                MobileWidget { kind: "cpu".into(), x: 0, y: 0, w: 3, h: 3 },
                MobileWidget { kind: "mem".into(), x: 3, y: 0, w: 3, h: 3 },
                MobileWidget { kind: "gpu".into(), x: 0, y: 3, w: 3, h: 3 },
                MobileWidget { kind: "bat".into(), x: 3, y: 3, w: 3, h: 3 },
                MobileWidget { kind: "net".into(), x: 0, y: 6, w: 3, h: 3 },
                MobileWidget { kind: "disk".into(), x: 3, y: 6, w: 3, h: 3 },
            ],
        }
    }

    /// 手机固定一屏默认：8 张紧凑卡（w1 h2），两排正好填满 4×4。
    fn default_grid_phone() -> Self {
        Self {
            v: 1,
            widgets: vec![
                MobileWidget { kind: "cpu".into(), x: 0, y: 0, w: 1, h: 2 },
                MobileWidget { kind: "mem".into(), x: 1, y: 0, w: 1, h: 2 },
                MobileWidget { kind: "gpu".into(), x: 2, y: 0, w: 1, h: 2 },
                MobileWidget { kind: "bat".into(), x: 3, y: 0, w: 1, h: 2 },
                MobileWidget { kind: "power".into(), x: 0, y: 2, w: 1, h: 2 },
                MobileWidget { kind: "net".into(), x: 1, y: 2, w: 1, h: 2 },
                MobileWidget { kind: "disk".into(), x: 2, y: 2, w: 1, h: 2 },
                MobileWidget { kind: "trend".into(), x: 3, y: 2, w: 1, h: 2 },
            ],
        }
    }

    /// 平板固定一屏默认：9 张紧凑卡（w2 h2），三排正好填满 6×6。
    fn default_grid_pad() -> Self {
        Self {
            v: 1,
            widgets: vec![
                MobileWidget { kind: "cpu".into(), x: 0, y: 0, w: 2, h: 2 },
                MobileWidget { kind: "mem".into(), x: 2, y: 0, w: 2, h: 2 },
                MobileWidget { kind: "gpu".into(), x: 4, y: 0, w: 2, h: 2 },
                MobileWidget { kind: "bat".into(), x: 0, y: 2, w: 2, h: 2 },
                MobileWidget { kind: "power".into(), x: 2, y: 2, w: 2, h: 2 },
                MobileWidget { kind: "net".into(), x: 4, y: 2, w: 2, h: 2 },
                MobileWidget { kind: "disk".into(), x: 0, y: 4, w: 2, h: 2 },
                MobileWidget { kind: "trend".into(), x: 2, y: 4, w: 2, h: 2 },
                MobileWidget { kind: "cores".into(), x: 4, y: 4, w: 2, h: 2 },
            ],
        }
    }

    fn default_flow(device: &str) -> Self {
        if canonical_device(device) == DEVICE_PAD {
            Self::default_flow_pad()
        } else {
            Self::default_flow_phone()
        }
    }

    fn default_grid(device: &str) -> Self {
        if canonical_device(device) == DEVICE_PAD {
            Self::default_grid_pad()
        } else {
            Self::default_grid_phone()
        }
    }

    /// 清洗：滤未知类型、按类型去重（每种组件至多一个，与桌面布局同规则）、
    /// 夹边界（列数按设备形态传入）。flow 只夹列/行上限并按 `(y,x)` 排序；
    /// grid 额外把 `h` 夹到 `rows`、`y` 夹到 `rows-h`，排序后丢弃仍与已保留项
    /// 重叠的组件（防手改存档把一屏布局弄重叠）。
    ///
    /// 前端 `layout.sanitize` 已经做过一轮完整清洗；这里只是后端最后一道边界校验。
    pub fn sanitize(&mut self, cols: i32, mode: LayoutMode, rows: i32) {
        let cols = cols.max(1);
        let rows = rows.max(1);
        let mut seen: Vec<String> = Vec::new();
        let mut out = Vec::new();
        for w in std::mem::take(&mut self.widgets) {
            let mut w = w;
            if !MOBILE_WIDGET_TYPES.contains(&w.kind.as_str()) || seen.contains(&w.kind) {
                continue;
            }
            w.w = w.w.clamp(1, cols);
            w.x = w.x.clamp(0, cols - w.w);
            w.h = if mode == LayoutMode::Grid {
                w.h.clamp(1, rows)
            } else {
                w.h.clamp(1, 8)
            };
            w.y = if mode == LayoutMode::Grid {
                w.y.clamp(0, (rows - w.h).max(0))
            } else {
                w.y.clamp(0, 2000)
            };
            seen.push(w.kind.clone());
            out.push(w);
        }
        out.sort_by(|a, b| a.y.cmp(&b.y).then_with(|| a.x.cmp(&b.x)));
        if mode == LayoutMode::Grid {
            let mut kept: Vec<MobileWidget> = Vec::new();
            for w in out {
                if kept.iter().any(|p| overlaps(&w, p)) {
                    continue;
                }
                kept.push(w);
            }
            self.widgets = kept;
        } else {
            self.widgets = out;
        }
    }
}

fn overlaps(a: &MobileWidget, b: &MobileWidget) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

fn default_device(device: &str) -> DeviceLayouts {
    DeviceLayouts {
        mode: LayoutMode::Flow,
        flow: MobileLayout::default_flow(device),
        grid: MobileLayout::default_grid(device),
    }
}

fn default_layouts() -> MobileLayouts {
    MobileLayouts {
        v: 2,
        phone: default_device(DEVICE_PHONE),
        pad: default_device(DEVICE_PAD),
    }
}

fn sanitize_device(device: &mut DeviceLayouts, cols: i32, rows: i32) {
    device.flow.sanitize(cols, LayoutMode::Flow, rows);
    device.grid.sanitize(cols, LayoutMode::Grid, rows);
}

/// 布局消息（协议 schema 2）。独立于每拍载荷（schema 1 体积闸门不含它），
/// 连接建立后必达、每次变更立即全量重发。`device` 缺省时 App 端按手机处理
/// （旧桌面 → 新 App 方向的兼容）；`mode` 缺省按 flow；grid 模式额外带 `rows`，
/// App 端用 `x/cols`、`y/rows`、`w/cols`、`h/rows` 做百分比定位。
#[derive(Serialize)]
struct LayoutMsg<'a> {
    schema: u8,
    #[serde(rename = "type")]
    kind: &'static str,
    device: &'static str,
    mode: LayoutMode,
    cols: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    rows: Option<i32>,
    #[serde(rename = "rowUnit")]
    row_unit: i32,
    widgets: &'a [MobileWidget],
}

/// 组装推送用的布局消息文本。空布局（组件全删）也是合法消息——App 端清空卡片。
/// 消息按激活模式生成：flow 不发 `rows`。
pub fn to_message(device: &str, mode: LayoutMode, layout: &MobileLayout) -> Option<String> {
    let device = canonical_device(device);
    let rows = rows_of(device);
    let mut clean = layout.clone();
    clean.sanitize(cols_of(device), mode, rows);
    serde_json::to_string(&LayoutMsg {
        schema: 2,
        kind: "layout",
        device,
        mode,
        cols: cols_of(device),
        rows: if mode == LayoutMode::Grid {
            Some(rows)
        } else {
            None
        },
        row_unit: ROW_UNIT,
        widgets: &clean.widgets,
    })
    .ok()
}

/// 解析存档文本。兼容三代格式：
/// 1. 顶层带 `widgets` 的单布局（只有手机形态的时代）→ 手机流式，平板默认；
/// 2. `{v, phone:{v,widgets}, pad:{...}}` 双设备单布局（上一版）→ 两份都作为流式，
///    固定一屏用默认布局，激活模式 flow；
/// 3. 新的 `{v, phone:{mode,flow,grid}, pad:{...}}`。
fn parse_file(text: &str) -> Option<MobileLayouts> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    if v.get("widgets").is_some() {
        let phone: MobileLayout = serde_json::from_value(v).ok()?;
        return Some(MobileLayouts {
            v: 2,
            phone: DeviceLayouts {
                mode: LayoutMode::Flow,
                flow: phone,
                grid: MobileLayout::default_grid(DEVICE_PHONE),
            },
            pad: default_device(DEVICE_PAD),
        });
    }
    let parse_device = |key: &str, device: &'static str| -> DeviceLayouts {
        let Some(obj) = v.get(key) else {
            return default_device(device);
        };
        let mode = obj
            .get("mode")
            .and_then(|m| m.as_str())
            .map(LayoutMode::from_str_loose)
            .unwrap_or_default();
        // 上一版把布局直接放在设备对象下（{v,widgets}）；新格式放在 flow 里。
        let flow = if obj.get("widgets").is_some() {
            serde_json::from_value::<MobileLayout>(obj.clone())
                .unwrap_or_else(|_| MobileLayout::default_flow(device))
        } else if let Some(f) = obj.get("flow") {
            serde_json::from_value::<MobileLayout>(f.clone())
                .unwrap_or_else(|_| MobileLayout::default_flow(device))
        } else {
            MobileLayout::default_flow(device)
        };
        let grid = if let Some(g) = obj.get("grid") {
            serde_json::from_value::<MobileLayout>(g.clone())
                .unwrap_or_else(|_| MobileLayout::default_grid(device))
        } else {
            MobileLayout::default_grid(device)
        };
        DeviceLayouts { mode, flow, grid }
    };
    Some(MobileLayouts {
        v: 2,
        phone: parse_device("phone", DEVICE_PHONE),
        pad: parse_device("pad", DEVICE_PAD),
    })
}

/// 配置的共享句柄（命令线程写、启动装配读），原子写 + 写盘串行化，
/// 与 `PushStore` / `TaskbarStore` 同一套路。
#[derive(Clone)]
pub struct MobileLayoutStore {
    layouts: Arc<Mutex<MobileLayouts>>,
    save_lock: Arc<Mutex<()>>,
}

impl MobileLayoutStore {
    /// 从磁盘恢复；文件不存在或损坏时回默认（每个设备各一份流式 + 固定一屏）
    pub fn load() -> Self {
        let path = crate::app_dir().join(FILE_NAME);
        let parsed = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| parse_file(&s));
        let mut layouts = parsed.unwrap_or_else(default_layouts);
        sanitize_device(&mut layouts.phone, PHONE_COLS, PHONE_ROWS);
        sanitize_device(&mut layouts.pad, PAD_COLS, PAD_ROWS);
        Self {
            layouts: Arc::new(Mutex::new(layouts)),
            save_lock: Arc::new(Mutex::new(())),
        }
    }

    /// 某设备的双布局 + 激活模式（前端编辑器启动时回显）
    pub fn get_device(&self, device: &str) -> DeviceLayouts {
        let layouts = self.lock();
        if canonical_device(device) == DEVICE_PAD {
            layouts.pad.clone()
        } else {
            layouts.phone.clone()
        }
    }

    /// 某设备某一份布局（推送预载用）
    pub fn get(&self, device: &str, mode: LayoutMode) -> MobileLayout {
        let layouts = self.lock();
        let d = if canonical_device(device) == DEVICE_PAD {
            &layouts.pad
        } else {
            &layouts.phone
        };
        match mode {
            LayoutMode::Flow => d.flow.clone(),
            LayoutMode::Grid => d.grid.clone(),
        }
    }

    /// 某设备当前激活的模式
    pub fn mode_of(&self, device: &str) -> LayoutMode {
        let layouts = self.lock();
        if canonical_device(device) == DEVICE_PAD {
            layouts.pad.mode
        } else {
            layouts.phone.mode
        }
    }

    /// 保存某设备某一份布局并返回清洗结果（前端回显以这份为准）
    pub fn set(&self, device: &str, mode: LayoutMode, mut layout: MobileLayout) -> MobileLayout {
        let device = canonical_device(device);
        let rows = rows_of(device);
        layout.sanitize(cols_of(device), mode, rows);
        {
            let mut layouts = self.lock();
            let d = if device == DEVICE_PAD {
                &mut layouts.pad
            } else {
                &mut layouts.phone
            };
            match mode {
                LayoutMode::Flow => d.flow = layout.clone(),
                LayoutMode::Grid => d.grid = layout.clone(),
            }
            self.save(&layouts);
        }
        layout
    }

    /// 切换某设备激活的布局模式并返回最新双布局
    pub fn set_mode(&self, device: &str, mode: LayoutMode) -> DeviceLayouts {
        let device = canonical_device(device);
        let mut layouts = self.lock();
        let d = if device == DEVICE_PAD {
            &mut layouts.pad
        } else {
            &mut layouts.phone
        };
        d.mode = mode;
        let snapshot = d.clone();
        self.save(&layouts);
        snapshot
    }

    fn save(&self, layouts: &MobileLayouts) {
        let _serialize_writes = self.save_lock.lock().unwrap_or_else(|e| e.into_inner());
        let path = crate::app_dir().join(FILE_NAME);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = path.with_extension("json.tmp");
        if serde_json::to_string_pretty(layouts)
            .ok()
            .and_then(|s| std::fs::write(&tmp, s).ok())
            .is_some()
        {
            let _ = std::fs::rename(&tmp, &path);
        }
    }

    /// 锁中毒不该把整个应用带崩（release 是 panic=abort），直接取回内部数据
    fn lock(&self) -> MutexGuard<'_, MobileLayouts> {
        self.layouts.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widget(kind: &str, x: i32, y: i32, w: i32, h: i32) -> MobileWidget {
        MobileWidget { kind: kind.into(), x, y, w, h }
    }

    #[test]
    fn mode_defaults_to_flow_and_tolerates_unknown() {
        assert_eq!(LayoutMode::default(), LayoutMode::Flow);
        let grid: LayoutMode = serde_json::from_str("\"grid\"").unwrap();
        assert_eq!(grid, LayoutMode::Grid);
        let unknown: LayoutMode = serde_json::from_str("\"something\"").unwrap();
        assert_eq!(unknown, LayoutMode::Flow, "未知 mode 回退 flow，不能整份存档失效");
        let upper: LayoutMode = serde_json::from_str("\"GRID\"").unwrap();
        assert_eq!(upper, LayoutMode::Grid, "大小写不敏感");
    }

    #[test]
    fn sanitize_filters_dedupes_and_clamps() {
        // flow：沿用旧的夹取规则
        let mut layout = MobileLayout {
            v: 1,
            widgets: vec![
                widget("nope", 0, 0, 2, 3),
                widget("cpu", -3, 0, 9, 99),
                widget("cpu", 2, 1, 2, 3),
                widget("mem", 2, 5, 2, 0),
            ],
        };
        layout.sanitize(PHONE_COLS, LayoutMode::Flow, PHONE_ROWS);
        assert_eq!(layout.widgets.len(), 2);
        assert_eq!(layout.widgets[0].kind, "cpu");
        assert_eq!(layout.widgets[0].w, 4, "手机形态 w 夹到 4");
        assert_eq!(layout.widgets[0].x, 0, "x 夹到下限");
        assert_eq!(layout.widgets[0].h, 8, "flow 的 h 上限仍是 8");
        assert_eq!(layout.widgets[1].kind, "mem");
        assert_eq!(layout.widgets[1].y, 5);
        assert_eq!(layout.widgets[1].h, 1, "h 夹到下限");

        // grid：h/y 夹到一屏，重叠项丢弃
        let mut grid = MobileLayout {
            v: 1,
            widgets: vec![widget("cpu", 0, 0, 2, 99), widget("mem", 0, 0, 2, 2)],
        };
        grid.sanitize(PAD_COLS, LayoutMode::Grid, PAD_ROWS);
        let kinds: Vec<&str> = grid.widgets.iter().map(|w| w.kind.as_str()).collect();
        assert_eq!(kinds, vec!["cpu"], "整列占满后，重叠的 mem 被丢弃");
        assert_eq!(grid.widgets[0].h, 6, "grid 的 h 夹到 6");
        assert_eq!(grid.widgets[0].y, 0);

        let mut clamped = MobileLayout {
            v: 1,
            widgets: vec![widget("bat", 4, 5, 9, 2)],
        };
        clamped.sanitize(PAD_COLS, LayoutMode::Grid, PAD_ROWS);
        assert_eq!(clamped.widgets[0].w, 6, "w 夹到列数");
        assert_eq!(clamped.widgets[0].x, 0, "x 夹到 cols-w");
        assert_eq!(clamped.widgets[0].y, 4, "y 夹到 rows-h");
    }

    /// 空存档（组件全删）是合法状态：sanitize 后仍为空，消息照发空表
    #[test]
    fn empty_layout_stays_empty() {
        let mut layout = MobileLayout { v: 1, widgets: vec![] };
        layout.sanitize(PHONE_COLS, LayoutMode::Flow, PHONE_ROWS);
        assert!(layout.widgets.is_empty());
        assert_eq!(
            to_message(DEVICE_PHONE, LayoutMode::Flow, &layout).unwrap(),
            r#"{"schema":2,"type":"layout","device":"phone","mode":"flow","cols":4,"rowUnit":92,"widgets":[]}"#
        );
        assert_eq!(
            to_message(DEVICE_PHONE, LayoutMode::Grid, &layout).unwrap(),
            r#"{"schema":2,"type":"layout","device":"phone","mode":"grid","cols":4,"rows":4,"rowUnit":92,"widgets":[]}"#
        );
        assert_eq!(
            to_message(DEVICE_PAD, LayoutMode::Grid, &layout).unwrap(),
            r#"{"schema":2,"type":"layout","device":"pad","mode":"grid","cols":6,"rows":6,"rowUnit":92,"widgets":[]}"#
        );
    }

    /// 布局消息的体积闸门：全 11 种组件也必须留在 BLE 64 帧（≈1152B）预算内，
    /// 取 1024 与每拍载荷同一档——加字段/组件类型时这里是闸门；设备 × 模式四条都过。
    #[test]
    fn layout_message_stays_within_ble_budget() {
        for device in [DEVICE_PHONE, DEVICE_PAD] {
            for mode in [LayoutMode::Flow, LayoutMode::Grid] {
                let cols = cols_of(device);
                let mut layout = MobileLayout::default_flow(device);
                // grid 用 1×1 铺开，保证 11 个都放得下且不重叠；flow 的 h 用 3 更贴近实际。
                layout.widgets = MOBILE_WIDGET_TYPES
                    .iter()
                    .enumerate()
                    .map(|(i, t)| {
                        let i = i as i32;
                        if mode == LayoutMode::Grid {
                            MobileWidget {
                                kind: t.to_string(),
                                x: i % cols,
                                y: i / cols,
                                w: 1,
                                h: 1,
                            }
                        } else {
                            MobileWidget {
                                kind: t.to_string(),
                                x: (i % 2) * 2,
                                y: i * 3,
                                w: 2,
                                h: 3,
                            }
                        }
                    })
                    .collect();
                layout.sanitize(cols, mode, rows_of(device));
                assert_eq!(layout.widgets.len(), 11, "{device}/{mode:?} 11 个组件都要留下");
                let msg = to_message(device, mode, &layout).unwrap();
                assert!(
                    msg.len() < 1024,
                    "{device}/{mode:?} 全组件布局消息 {} 字节，超出预算",
                    msg.len()
                );
            }
        }
    }

    #[test]
    fn message_shape_matches_protocol_doc() {
        let flow = to_message(DEVICE_PHONE, LayoutMode::Flow, &MobileLayout::default_flow(DEVICE_PHONE))
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&flow).unwrap();
        assert_eq!(v["schema"], 2);
        assert_eq!(v["type"], "layout");
        assert_eq!(v["device"], "phone");
        assert_eq!(v["mode"], "flow");
        assert_eq!(v["cols"], 4);
        assert!(v.get("rows").is_none(), "flow 消息不带 rows");
        assert_eq!(v["rowUnit"], 92);
        assert_eq!(v["widgets"].as_array().unwrap().len(), 6);
        assert_eq!(v["widgets"][0]["type"], "cpu");
        assert_eq!(v["widgets"][0]["w"], 2);

        let grid = to_message(DEVICE_PHONE, LayoutMode::Grid, &MobileLayout::default_grid(DEVICE_PHONE))
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&grid).unwrap();
        assert_eq!(v["mode"], "grid");
        assert_eq!(v["rows"], 4);
        assert_eq!(v["widgets"].as_array().unwrap().len(), 8);
        assert_eq!(v["widgets"][0]["w"], 1);

        let grid_pad = to_message(DEVICE_PAD, LayoutMode::Grid, &MobileLayout::default_grid(DEVICE_PAD))
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&grid_pad).unwrap();
        assert_eq!(v["device"], "pad");
        assert_eq!(v["mode"], "grid");
        assert_eq!(v["cols"], 6);
        assert_eq!(v["rows"], 6);
        assert_eq!(v["widgets"].as_array().unwrap().len(), 9);
        assert_eq!(v["widgets"][0]["w"], 2);
    }

    /// 两套固定一屏默认布局必须恰好装进 rows，且互不重叠
    #[test]
    fn grid_defaults_fill_one_screen() {
        for (device, count) in [(DEVICE_PHONE, 8), (DEVICE_PAD, 9)] {
            let layout = MobileLayout::default_grid(device);
            assert_eq!(layout.widgets.len(), count);
            let rows = rows_of(device);
            for w in &layout.widgets {
                assert!(w.x >= 0 && w.x + w.w <= cols_of(device));
                assert!(w.y >= 0 && w.y + w.h <= rows);
            }
            for (i, a) in layout.widgets.iter().enumerate() {
                for b in layout.widgets.iter().skip(i + 1) {
                    assert!(!overlaps(a, b), "{device} 默认布局 {} 与 {} 重叠", a.kind, b.kind);
                }
            }
        }
    }

    /// 缺字段的存档（老版本文件 / 手改）必须还能解析，否则等于悄悄重置布局
    #[test]
    fn partial_layout_still_parses() {
        let layouts = parse_file("{}").expect("空存档解析成默认值");
        assert_eq!(layouts.phone, default_device(DEVICE_PHONE));
        assert_eq!(layouts.pad, default_device(DEVICE_PAD));

        let layouts = parse_file(
            r#"{"phone":{"widgets":[{"type":"cpu","x":0,"y":0,"w":2,"h":3}]}}"#,
        )
        .expect("上一版双设备格式可解析");
        assert_eq!(layouts.phone.mode, LayoutMode::Flow);
        assert_eq!(layouts.phone.flow.widgets.len(), 1);
        assert_eq!(layouts.phone.grid, MobileLayout::default_grid(DEVICE_PHONE));
        assert_eq!(layouts.pad, default_device(DEVICE_PAD));

        let layouts = parse_file(
            r#"{"phone":{"mode":"grid","flow":{"widgets":[]},"grid":{"widgets":[{"type":"mem","x":0,"y":0,"w":1,"h":1}]}}}"#,
        )
        .expect("新格式可解析");
        assert_eq!(layouts.phone.mode, LayoutMode::Grid);
        assert!(layouts.phone.flow.widgets.is_empty());
        assert_eq!(layouts.phone.grid.widgets.len(), 1);
    }

    /// 旧单布局格式（顶层 widgets，只有手机形态的时代）迁移成双布局：
    /// 手机流式原样保留，固定一屏与平板回默认
    #[test]
    fn legacy_single_layout_migrates_to_phone() {
        let legacy = r#"{"v":1,"widgets":[{"type":"cpu","x":0,"y":0,"w":2,"h":3},
{"type":"gpu","x":0,"y":3,"w":2,"h":3}]}"#;
        let layouts = parse_file(legacy).expect("旧格式可迁移");
        assert_eq!(layouts.phone.flow.widgets.len(), 2);
        assert_eq!(layouts.phone.flow.widgets[0].kind, "cpu");
        assert_eq!(layouts.phone.mode, LayoutMode::Flow);
        assert_eq!(layouts.phone.grid, MobileLayout::default_grid(DEVICE_PHONE));
        assert_eq!(layouts.pad, default_device(DEVICE_PAD));

        let fresh = r#"{"v":1,"phone":{"widgets":[]},"pad":{"widgets":[]}}"#;
        let layouts = parse_file(fresh).expect("上一版双设备格式照常解析");
        assert!(layouts.phone.flow.widgets.is_empty());
        assert!(layouts.pad.flow.widgets.is_empty());
        assert_eq!(layouts.phone.grid, MobileLayout::default_grid(DEVICE_PHONE));
        assert_eq!(layouts.pad.grid, MobileLayout::default_grid(DEVICE_PAD));
    }
}
