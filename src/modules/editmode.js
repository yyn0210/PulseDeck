// 编辑布局模式：拖拽换位（与其他组件重叠则交换位置）、右下角缩放（不许压到别人）、
// 添加 / 移除 / 重置。只作用于监控页；进出由顶栏「编辑布局」按钮切换，
// 全部视觉开关挂在 body.editing 上（虚线框 / 手柄 / 占位框的显隐都走 CSS）。
//
// 坐标换算：网格 12 列等宽（fr）+ 固定行高（layout.ROW_UNIT），列宽由容器实时量出。
// 落位后的合法性（去重 / 夹边界 / 消除残余重叠 / 持久化）统一交给 dashboard.settle()。

import * as layout from "./layout.js";
import { WIDGET_ORDER, widgetDef } from "../widgets/registry.js";

function clamp(v, lo, hi) {
  return Math.max(lo, Math.min(hi, v));
}

/**
 * @param {{
 *   dashboard: import("./dashboard.js").Dashboard,
 *   grid: HTMLElement,
 *   editBtn: HTMLButtonElement,
 *   editbar: HTMLElement,
 *   chipsEl: HTMLElement,
 *   resetBtn: HTMLButtonElement,
 * }} refs
 */
export function initEditmode(refs) {
  const { dashboard, grid, editBtn, editbar, chipsEl, resetBtn } = refs;
  let editing = false;
  /** @type {null | {item, mode, startX, startY, grabX, grabY, cellW, cellH, gap, startW, startH, ph, target}} */
  let drag = null;

  /* ---- 网格度量 ---- */

  function metrics() {
    const gap = parseFloat(getComputedStyle(grid).columnGap) || 14;
    const cellW = (grid.clientWidth - (layout.COLS - 1) * gap) / layout.COLS;
    return { gap, cellW, cellH: layout.ROW_UNIT };
  }

  /** 拖动落点：卡片左上角（指针位置 - 抓住点偏移）换算成网格格坐标。
   * 加半格距再取整：像素坐标由浮点几何算出（273.4999…），直接 floor 会偏左一格。 */
  function targetCell(e, m) {
    const gridRect = grid.getBoundingClientRect();
    const px = e.clientX - gridRect.left - drag.grabX;
    const py = e.clientY - gridRect.top + grid.scrollTop - drag.grabY;
    return {
      x: clamp(
        Math.floor((px + m.gap / 2) / (m.cellW + m.gap)),
        0,
        layout.COLS - drag.item.w,
      ),
      y: Math.max(0, Math.floor((py + m.gap / 2) / (m.cellH + m.gap))),
    };
  }

  /* ---- 缩放：目标尺寸 → 不压到别人的最大可行尺寸 ---- */

  function pickAllowedSize(item, desiredW, desiredH) {
    const def = item.def;
    const maxW = Math.min(def.maxW, layout.COLS - item.x);
    const dw = clamp(desiredW, def.minW, maxW);
    const dh = clamp(desiredH, def.minH, def.maxH);
    const others = dashboard.items.filter((o) => o !== item);
    for (let w = dw; w >= def.minW; w -= 1) {
      for (let h = dh; h >= def.minH; h -= 1) {
        // 比当前尺寸小的候选被当前矩形完全包含，缩小不可能制造重叠，直接可行
        if (w <= item.w && h <= item.h) return { w, h };
        if (!others.some((o) => layout.overlaps({ x: item.x, y: item.y, w, h }, o))) {
          return { w, h };
        }
      }
    }
    return { w: item.w, h: item.h };
  }

  /* ---- 落点交换对象：与拖动组件重叠面积最大的那块（落点可能同时压住多块的边角） ---- */

  function pickVictim(item, target) {
    let victim = null;
    let bestArea = 0;
    for (const o of dashboard.items) {
      if (o === item) continue;
      const ox = Math.min(target.x + item.w, o.x + o.w) - Math.max(target.x, o.x);
      const oy = Math.min(target.y + item.h, o.y + o.h) - Math.max(target.y, o.y);
      if (ox > 0 && oy > 0 && ox * oy > bestArea) {
        bestArea = ox * oy;
        victim = o;
      }
    }
    return victim;
  }

  /* ---- 工具条 ---- */

  function renderChips() {
    const mounted = new Set(dashboard.items.map((i) => i.type));
    chipsEl.innerHTML = WIDGET_ORDER.filter((t) => !mounted.has(t))
      .map((t) => {
        const def = widgetDef(t);
        return `<button type="button" class="editbar-chip" data-type="${t}">＋ ${def.title}</button>`;
      })
      .join("");
  }

  function setEditing(on) {
    if (editing === on) return;
    if (drag) finishDrag(true);
    editing = on;
    document.body.classList.toggle("editing", on);
    editbar.classList.toggle("hidden", !on);
    editBtn.classList.toggle("active", on);
    editBtn.textContent = on ? "完成" : "编辑布局";
    if (on) renderChips();
  }

  editBtn.addEventListener("click", () => setEditing(!editing));
  resetBtn.addEventListener("click", () => {
    dashboard.reset();
    renderChips();
  });
  chipsEl.addEventListener("click", (e) => {
    const chip = e.target.closest(".editbar-chip");
    if (!chip) return;
    dashboard.add(chip.dataset.type);
    renderChips();
  });

  // 移除按钮：走 click（不与拖拽抢 pointerdown）
  grid.addEventListener("click", (e) => {
    if (!editing) return;
    const btn = e.target.closest(".w-remove");
    if (!btn) return;
    const item = dashboard.items.find((i) => i.el === btn.closest(".widget"));
    if (item) {
      dashboard.remove(item.type);
      renderChips();
    }
  });

  /* ---- 拖拽 / 缩放（Pointer Events） ---- */

  grid.addEventListener("pointerdown", (e) => {
    if (!editing || e.button !== 0 || drag) return;
    const card = e.target.closest(".widget");
    if (!card) return; // 点在空白处 / 滚动条上
    const item = dashboard.items.find((i) => i.el === card);
    if (!item) return;
    const handle = e.target.closest(".w-resize");
    if (!handle && e.target.closest(".w-remove")) return;

    e.preventDefault();
    const m = metrics();
    const rect = card.getBoundingClientRect();
    drag = {
      item,
      mode: handle ? "resize" : "move",
      startX: e.clientX,
      startY: e.clientY,
      grabX: e.clientX - rect.left,
      grabY: e.clientY - rect.top,
      startW: item.w,
      startH: item.h,
      cellW: m.cellW,
      cellH: m.cellH,
      gap: m.gap,
      ph: null,
      target: null,
    };
    if (drag.mode === "move") {
      card.classList.add("dragging");
      drag.ph = document.createElement("div");
      drag.ph.className = "grid-ph";
      grid.append(drag.ph);
      movePlaceholder(e);
    } else {
      card.classList.add("resizing");
    }
    card.setPointerCapture(e.pointerId);
  });

  function movePlaceholder(e) {
    const cell = targetCell(e, drag);
    drag.target = cell;
    drag.ph.style.gridColumn = `${cell.x + 1} / span ${drag.item.w}`;
    drag.ph.style.gridRow = `${cell.y + 1} / span ${drag.item.h}`;
  }

  window.addEventListener("pointermove", (e) => {
    if (!drag) return;
    if (drag.mode === "move") {
      const dx = e.clientX - drag.startX;
      const dy = e.clientY - drag.startY;
      drag.item.el.style.transform = `translate(${dx}px, ${dy}px)`;
      movePlaceholder(e);
    } else {
      const pitchW = drag.cellW + drag.gap;
      const pitchH = drag.cellH + drag.gap;
      const desiredW = drag.startW + Math.round((e.clientX - drag.startX) / pitchW);
      const desiredH = drag.startH + Math.round((e.clientY - drag.startY) / pitchH);
      const { item } = drag;
      const size = pickAllowedSize(item, desiredW, desiredH);
      item.w = size.w;
      item.h = size.h;
      item.el.style.gridColumn = `${item.x + 1} / span ${item.w}`;
      item.el.style.gridRow = `${item.y + 1} / span ${item.h}`;
    }
  });

  /** 结束拖拽。cancelled = true 时还原到拖拽前状态（Esc / pointercancel）。 */
  function finishDrag(cancelled) {
    if (!drag) return;
    const { item, mode, ph } = drag;
    item.el.classList.remove("dragging", "resizing");
    item.el.style.transform = "";
    if (ph) ph.remove();

    if (!cancelled) {
      if (mode === "move" && drag.target) {
        const old = { x: item.x, y: item.y };
        const target = { x: drag.target.x, y: drag.target.y };
        if (target.x !== old.x || target.y !== old.y) {
          // 落点压到别人 → 与「被压面积最大」的那块交换（尺寸不同时换过去可能撞到
          // 第三块，残余重叠由 settle 的「向下推」兜底）
          const victim = pickVictim(item, target);
          if (victim) {
            victim.x = Math.min(old.x, layout.COLS - victim.w);
            victim.y = old.y;
          }
          item.x = target.x;
          item.y = target.y;
          dashboard.settle();
        }
      } else if (mode === "resize") {
        dashboard.settle();
      }
    } else {
      if (mode === "resize") {
        item.w = drag.startW;
        item.h = drag.startH;
      }
      dashboard.sync();
    }
    drag = null;
  }

  window.addEventListener("pointerup", () => finishDrag(false));
  window.addEventListener("pointercancel", () => finishDrag(true));
  // Esc 取消进行中的拖拽 / 缩放（落位前反悔）
  window.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && drag) {
      e.preventDefault();
      finishDrag(true);
    }
  });
}
