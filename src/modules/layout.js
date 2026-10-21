// 布局纯函数：网格坐标模型与 localStorage 持久化。
// 布局目标（profile）：桌面主面板（12 列，`pd.layout`），以及移动 App 的
// 4 份布局——每个设备形态（手机 / 平板）× 两种模式（流式 flow / 固定一屏 grid）：
// - flow：App 按 (y,x) 阅读顺序排列、w 决定宽度、h 只决定精简/详细，卡高随内容；
// - grid：手机 4×4 / 平板 6×6，App 把行高按可用高度均分、绝对定位，整屏不滚动。
// 后四套经数据推送给 App（见 widgets/mobile.js 与 src-tauri/src/mobile_layout.rs）。
// 这里只放纯函数——DOM 挂载在 dashboard.js，拖拽交互在 editmode.js。
// localStorage 键沿用项目的 pd. 前缀（与 pd.activeView 同级的主窗口 UI 状态）。

import { widgetDef, WIDGET_ORDER } from "../widgets/registry.js";
import { mobileDef, MOBILE_ORDER } from "../widgets/mobile.js";

/** 桌面网格的行高单元（px），与 style.css :root 的 --row-unit 同值。
 * 移动端流式预览的行单元见各 profile 的 rowUnit（与 .grid-mobile / .grid-pad
 * 上的 --row-unit 同步）——手机卡是环 + 明细形态，用 44px 的行装不下。 */
export const ROW_UNIT = 44;

/**
 * 布局目标画像：列数 / 行数 / 存储键 / 注册表 / 默认布局。
 * `cols` 与 style.css 里容器上的 `--cols` 变量是同一个值（#grid 是 12、
 * .grid-mobile 是 4、.grid-pad 是 6），改必须两处一起改。移动端四份的
 * `device` / `mode` 字段是推送消息里的取值（后端 set_mobile_layout 的参数同款）。
 * `rows` 只对固定一屏（grid）有意义：编辑器与 App 都用它在纵向分格；
 * flow 没有行数上限（y 可到 2000），rowUnit 仅用于编辑网格的行高。
 */
export const DESK = {
  key: "pd.layout",
  cols: 12,
  rowUnit: ROW_UNIT,
  def: widgetDef,
  order: WIDGET_ORDER,
  defaults: () => defaultLayout(),
};

export const MOBILE_PHONE_FLOW = {
  key: "pd.mlayout",
  modeKey: "pd.mlayoutMode",
  cols: 4,
  rowUnit: 92,
  device: "phone",
  mode: "flow",
  def: mobileDef,
  order: MOBILE_ORDER,
  defaults: () => defaultMobileFlowLayout(),
};

export const MOBILE_PHONE_GRID = {
  key: "pd.mlayoutGrid",
  modeKey: "pd.mlayoutMode",
  cols: 4,
  rows: 4,
  device: "phone",
  mode: "grid",
  def: mobileDef,
  order: MOBILE_ORDER,
  defaults: () => defaultMobileGridLayout(),
};

export const MOBILE_PAD_FLOW = {
  key: "pd.mlayoutPad",
  modeKey: "pd.mlayoutModePad",
  cols: 6,
  rowUnit: 92,
  device: "pad",
  mode: "flow",
  def: mobileDef,
  order: MOBILE_ORDER,
  defaults: () => defaultPadFlowLayout(),
};

export const MOBILE_PAD_GRID = {
  key: "pd.mlayoutPadGrid",
  modeKey: "pd.mlayoutModePad",
  cols: 6,
  rows: 6,
  device: "pad",
  mode: "grid",
  def: mobileDef,
  order: MOBILE_ORDER,
  defaults: () => defaultPadGridLayout(),
};

/** 兼容旧名字：默认指流式那一份（现有用户的布局都从流式延续）。 */
export const MOBILE_PHONE = MOBILE_PHONE_FLOW;
export const MOBILE_PAD = MOBILE_PAD_FLOW;

function clampInt(v, lo, hi) {
  const n = Math.round(Number(v));
  if (!Number.isFinite(n)) return lo;
  return Math.max(lo, Math.min(hi, n));
}

/** 两块组件是否重叠（网格坐标，闭开区间） */
export function overlaps(a, b) {
  return a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
}

/** 夹到组件允许的尺寸与网格边界内；未知类型返回 null。
 * grid 模式额外受行数约束（h ≤ rows、y ≤ rows-h），flow 的 y 只防御性夹到 2000。 */
export function clampRect(raw, profile) {
  const def = profile.def(raw.type);
  if (!def) return null;
  // minW 防御性夹到列数内：列数小于组件最小宽时宁可溢出注册表约束也不溢出网格
  const minW = Math.min(def.minW, profile.cols);
  const w = clampInt(raw.w ?? def.defW, minW, Math.min(def.maxW, profile.cols));
  const hMax = profile.rows ? Math.min(def.maxH, profile.rows) : def.maxH;
  const h = clampInt(raw.h ?? def.defH, Math.min(def.minH, hMax), hMax);
  const x = clampInt(raw.x ?? 0, 0, profile.cols - w);
  const yMax = profile.rows ? Math.max(0, profile.rows - h) : 2000;
  const y = clampInt(raw.y ?? 0, 0, yMax);
  return { type: raw.type, x, y, w, h };
}

/**
 * 清洗：滤掉未知类型、按类型去重（每种组件至多一个实例）、夹到边界。
 * - flow：按 (y, x) 排序逐个落位，与已放置的重叠就下移到挡路者正下方（无行数上限）；
 * - grid：固定一屏——优先保留原位置，放不下就找第一个空位，仍放不下则丢弃该项。
 *   加载手改/旧存档时宁可少一张卡，也不让卡片叠在一起或伸出屏幕。
 * 拖拽换位（交换后可能撞到第三块）与加载旧配置两条路径都靠它兜底。
 */
export function sanitize(list, profile) {
  const seen = new Set();
  const out = [];
  for (const raw of Array.isArray(list) ? list : []) {
    if (!raw || seen.has(raw.type)) continue;
    const rect = clampRect(raw, profile);
    if (!rect) continue;
    seen.add(rect.type);
    out.push(rect);
  }
  out.sort((a, b) => a.y - b.y || a.x - b.x);
  if (profile.rows) {
    const placed = [];
    for (const item of out) {
      if (!placed.some((p) => overlaps(item, p))) {
        placed.push(item);
        continue;
      }
      const spot = findFreeSpot(placed, item.w, item.h, profile.cols, profile.rows);
      if (spot) {
        item.x = spot.x;
        item.y = spot.y;
        placed.push(item);
      }
    }
    return placed;
  }
  const placed = [];
  for (const item of out) {
    let moved = true;
    while (moved) {
      moved = false;
      for (const p of placed) {
        if (overlaps(item, p)) {
          item.y = p.y + p.h;
          moved = true;
        }
      }
    }
    placed.push(item);
  }
  return placed;
}

/** 桌面默认布局：旧版固定排布翻译成网格坐标（观感用 mock 预览校准，改这里不用迁存量用户）。 */
export function defaultLayout() {
  return sanitize(
    [
      { type: "cpu", x: 0, y: 0, w: 3, h: 4 },
      { type: "mem", x: 3, y: 0, w: 3, h: 4 },
      { type: "bat", x: 6, y: 0, w: 3, h: 4 },
      { type: "gpu", x: 9, y: 0, w: 3, h: 4 },
      { type: "power", x: 0, y: 4, w: 3, h: 4 },
      { type: "net", x: 3, y: 4, w: 9, h: 5 },
      { type: "trend", x: 0, y: 9, w: 7, h: 4 },
      { type: "cores", x: 0, y: 13, w: 7, h: 4 },
      { type: "disk", x: 7, y: 9, w: 5, h: 8 },
      { type: "procCpu", x: 0, y: 17, w: 6, h: 6 },
      { type: "procMem", x: 6, y: 17, w: 6, h: 6 },
    ],
    DESK,
  );
}

/** 手机流式默认布局：六张基础卡两列排布（与后端 MobileLayout::default_flow_phone 同构）。 */
export function defaultMobileFlowLayout() {
  return sanitize(
    [
      { type: "cpu", x: 0, y: 0, w: 2, h: 3 },
      { type: "mem", x: 2, y: 0, w: 2, h: 3 },
      { type: "gpu", x: 0, y: 3, w: 2, h: 3 },
      { type: "bat", x: 2, y: 3, w: 2, h: 3 },
      { type: "net", x: 0, y: 6, w: 2, h: 3 },
      { type: "disk", x: 2, y: 6, w: 2, h: 3 },
    ],
    MOBILE_PHONE_FLOW,
  );
}

/** 平板流式默认布局：六张基础卡一排两张（3 列宽），与手机版同构。 */
export function defaultPadFlowLayout() {
  return sanitize(
    [
      { type: "cpu", x: 0, y: 0, w: 3, h: 3 },
      { type: "mem", x: 3, y: 0, w: 3, h: 3 },
      { type: "gpu", x: 0, y: 3, w: 3, h: 3 },
      { type: "bat", x: 3, y: 3, w: 3, h: 3 },
      { type: "net", x: 0, y: 6, w: 3, h: 3 },
      { type: "disk", x: 3, y: 6, w: 3, h: 3 },
    ],
    MOBILE_PAD_FLOW,
  );
}

/** 手机固定一屏默认：8 张紧凑卡（w1 h2），两排正好填满 4×4（与后端同构）。 */
export function defaultMobileGridLayout() {
  return sanitize(
    [
      { type: "cpu", x: 0, y: 0, w: 1, h: 2 },
      { type: "mem", x: 1, y: 0, w: 1, h: 2 },
      { type: "gpu", x: 2, y: 0, w: 1, h: 2 },
      { type: "bat", x: 3, y: 0, w: 1, h: 2 },
      { type: "power", x: 0, y: 2, w: 1, h: 2 },
      { type: "net", x: 1, y: 2, w: 1, h: 2 },
      { type: "disk", x: 2, y: 2, w: 1, h: 2 },
      { type: "trend", x: 3, y: 2, w: 1, h: 2 },
    ],
    MOBILE_PHONE_GRID,
  );
}

/** 平板固定一屏默认：9 张紧凑卡（w2 h2），三排正好填满 6×6（与后端同构）。 */
export function defaultPadGridLayout() {
  return sanitize(
    [
      { type: "cpu", x: 0, y: 0, w: 2, h: 2 },
      { type: "mem", x: 2, y: 0, w: 2, h: 2 },
      { type: "gpu", x: 4, y: 0, w: 2, h: 2 },
      { type: "bat", x: 0, y: 2, w: 2, h: 2 },
      { type: "power", x: 2, y: 2, w: 2, h: 2 },
      { type: "net", x: 4, y: 2, w: 2, h: 2 },
      { type: "disk", x: 0, y: 4, w: 2, h: 2 },
      { type: "trend", x: 2, y: 4, w: 2, h: 2 },
      { type: "cores", x: 4, y: 4, w: 2, h: 2 },
    ],
    MOBILE_PAD_GRID,
  );
}

/** 兼容旧名字：都指流式那一份。 */
export function defaultMobileLayout() {
  return defaultMobileFlowLayout();
}

export function defaultPadLayout() {
  return defaultPadFlowLayout();
}

/**
 * 从左上角起扫描第一个放得下 w×h 的空位（「添加组件」用）。
 * list 需已 sanitize（无重叠）。传 rows 时只在固定一屏范围内找，无空位返回 null；
 * 不传 rows 时兜底追加到最底部（流式 / 桌面）。
 */
export function findFreeSpot(list, w, h, cols, rows = null) {
  const yMax = rows ? Math.max(0, rows - h) : 200;
  for (let y = 0; y <= yMax; y += 1) {
    for (let x = 0; x + w <= cols; x += 1) {
      if (!list.some((o) => overlaps({ x, y, w, h }, o))) return { x, y };
    }
  }
  if (rows) return null;
  const maxY = list.reduce((m, o) => Math.max(m, o.y + o.h), 0);
  return { x: 0, y: maxY };
}

/** 读取持久化布局。存档结构合法（哪怕组件全被移除）就照用；否则回该目标的默认布局。 */
export function load(profile) {
  let stored = null;
  try {
    stored = JSON.parse(localStorage.getItem(profile.key) || "null");
  } catch (e) {
    stored = null;
  }
  if (stored && Array.isArray(stored.widgets)) return sanitize(stored.widgets, profile);
  return profile.defaults();
}

export function save(list, profile) {
  try {
    localStorage.setItem(profile.key, JSON.stringify({ v: 1, widgets: list }));
  } catch (e) {
    /* 隐私模式等场景不可写：本次运行内仍生效，下次启动回到默认布局 */
  }
}

/** 读取某设备的激活模式（"flow" / "grid"，缺省 flow）。传任一该设备的 profile 即可。 */
export function loadMode(profile) {
  try {
    return localStorage.getItem(profile.modeKey) === "grid" ? "grid" : "flow";
  } catch (e) {
    return "flow";
  }
}

export function saveMode(profile, mode) {
  try {
    localStorage.setItem(profile.modeKey, mode === "grid" ? "grid" : "flow");
  } catch (e) {
    /* 隐私模式等场景不可写 */
  }
}
