// 监控页布局：网格坐标模型（12 列 × 固定行高）与 localStorage 持久化。
// 这里只放纯函数——DOM 挂载在 dashboard.js，拖拽交互在 editmode.js。
// localStorage 键沿用项目的 pd. 前缀（与 pd.activeView 同级的主窗口 UI 状态，不进后端）。

import { widgetDef } from "../widgets/registry.js";

/** 网格列数（与 style.css 里 .grid 的 repeat(12, …) 对应） */
export const COLS = 12;
/** 行高单元（px）。与 style.css 的 --row-unit 是同一个值，改要两处一起改。 */
export const ROW_UNIT = 44;

const LAYOUT_KEY = "pd.layout";

function clampInt(v, lo, hi) {
  const n = Math.round(Number(v));
  if (!Number.isFinite(n)) return lo;
  return Math.max(lo, Math.min(hi, n));
}

/** 两块组件是否重叠（网格坐标，闭开区间） */
export function overlaps(a, b) {
  return a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
}

/** 夹到组件允许的尺寸与网格边界内；未知类型返回 null。 */
export function clampRect(raw) {
  const def = widgetDef(raw.type);
  if (!def) return null;
  const w = clampInt(raw.w ?? def.defW, def.minW, Math.min(def.maxW, COLS));
  const h = clampInt(raw.h ?? def.defH, def.minH, def.maxH);
  const x = clampInt(raw.x ?? 0, 0, COLS - w);
  const y = clampInt(raw.y ?? 0, 0, 2000);
  return { type: raw.type, x, y, w, h };
}

/**
 * 清洗：滤掉未知类型、按类型去重（每种组件至多一个实例）、夹到边界，
 * 最后消除重叠——按 (y, x) 排序逐个落位，与已放置的重叠就下移到挡路者正下方。
 * 拖拽换位（交换后可能撞到第三块）与加载旧配置两条路径都靠它兜底。
 */
export function sanitize(list) {
  const seen = new Set();
  const out = [];
  for (const raw of Array.isArray(list) ? list : []) {
    if (!raw || seen.has(raw.type)) continue;
    const rect = clampRect(raw);
    if (!rect) continue;
    seen.add(rect.type);
    out.push(rect);
  }
  out.sort((a, b) => a.y - b.y || a.x - b.x);
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
  return out;
}

/** 默认布局：旧版固定排布翻译成网格坐标（观感用 mock 预览校准，改这里不用迁存量用户）。 */
export function defaultLayout() {
  return sanitize([
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
  ]);
}

/**
 * 从左上角起扫描第一个放得下 w×h 的空位（「添加组件」用）。
 * list 需已 sanitize（无重叠）。兜底：追加到最底部。
 */
export function findFreeSpot(list, w, h) {
  for (let y = 0; y <= 200; y += 1) {
    for (let x = 0; x + w <= COLS; x += 1) {
      if (!list.some((o) => overlaps({ x, y, w, h }, o))) return { x, y };
    }
  }
  const maxY = list.reduce((m, o) => Math.max(m, o.y + o.h), 0);
  return { x: 0, y: maxY };
}

/** 读取持久化布局。存档结构合法（哪怕组件全被移除）就照用；否则回默认布局。 */
export function load() {
  let stored = null;
  try {
    stored = JSON.parse(localStorage.getItem(LAYOUT_KEY) || "null");
  } catch (e) {
    stored = null;
  }
  if (stored && Array.isArray(stored.widgets)) return sanitize(stored.widgets);
  return defaultLayout();
}

export function save(list) {
  try {
    localStorage.setItem(LAYOUT_KEY, JSON.stringify({ v: 1, widgets: list }));
  } catch (e) {
    /* 隐私模式等场景不可写：本次运行内仍生效，下次启动回到默认布局 */
  }
}
