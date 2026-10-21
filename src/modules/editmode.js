// 编辑布局模式：拖拽换位（与其他组件重叠则交换位置）、右下角缩放（不许压到别人）、
// 添加 / 移除 / 重置。进出由顶栏「编辑布局」按钮切换，全部视觉开关挂在
// body.editing 上（虚线框 / 手柄 / 占位框的显隐都走 CSS）。
//
// 一套交互驱动**五套布局目标**：桌面主面板（12 列 #grid）与移动 App 的
// 「手机 / 平板」×「流式 / 固定一屏」四份预览。工具条上的「桌面 / 手机 / 平板」
// 分段按钮选设备，「流式 / 固定一屏」选该设备的激活模式（同时决定 App 显示哪一份）；
// body.edit-mobile 控制预览框显隐与桌面网格让位，body.edit-phone / edit-pad +
// edit-flow / edit-grid 再挑显示哪个网格。五个网格都挂了事件监听，非当前目标的
// 直接忽略。
//
// 坐标换算：网格等宽（fr）+ 行高。桌面 44px、流式预览 92px（profile.rowUnit）；
// 固定一屏用 profile.rows 把预览框可用高度等分（与 App 端按 rows 的百分比定位
// 同一套几何），列宽仍由容器实时量出。落位后的合法性（去重 / 夹边界 / 有界清洗 /
// 持久化）统一交给 dashboard.settle()；固定一屏装不下时回滚本次拖拽。

import * as layout from "./layout.js";

function clamp(v, lo, hi) {
  return Math.max(lo, Math.min(hi, v));
}

/**
 * @param {{
 *   targets: {
 *     desk: {dashboard: import("./dashboard.js").Dashboard, grid: HTMLElement},
 *     phone: {flow: {dashboard, grid}, grid: {dashboard, grid}},
 *     pad: {flow: {dashboard, grid}, grid: {dashboard, grid}},
 *   },
 *   modes: {phone: "flow"|"grid", pad: "flow"|"grid"},
 *   onModeChange?: (device: "phone"|"pad", mode: "flow"|"grid") => void,
 *   editBtn: HTMLButtonElement,
 *   editbar: HTMLElement,
 *   chipsEl: HTMLElement,
 *   resetBtn: HTMLButtonElement,
 *   noticeEl?: HTMLElement,
 * }} refs
 */
export function initEditmode(refs) {
  const { targets, modes, onModeChange, editBtn, editbar, chipsEl, resetBtn, noticeEl } = refs;
  let editing = false;
  let device = "desk";
  /** @type {null | {item, mode, startX, startY, grabX, grabY, cellW, cellH, gap, startW, startH, ph, target, snapshot}} */
  let drag = null;
  let noticeTimer = -1;

  /* ---- 当前目标 ---- */

  let cur = targets.desk;
  const cols = () => cur.dashboard.profile.cols;
  const profile = () => cur.dashboard.profile;

  const targetBtns = [...editbar.querySelectorAll("#editbar-target .editbar-target-btn")];
  const modeBtns = [...editbar.querySelectorAll(".editbar-mode-btn")];
  const flowBtn = editbar.querySelector("#edit-flowview");
  targetBtns.forEach((btn) =>
    btn.addEventListener("click", () => setDevice(btn.dataset.target)),
  );
  modeBtns.forEach((btn) =>
    btn.addEventListener("click", () => setMode(btn.dataset.mode)),
  );

  function targetOf(dev, mode) {
    if (dev === "desk") return targets.desk;
    return targets[dev] ? targets[dev][mode] || targets[dev].flow : targets.desk;
  }

  function applySelection() {
    const mobile = device === "phone" || device === "pad";
    cur = targetOf(device, modes[device] || "flow");
    for (const btn of targetBtns) btn.classList.toggle("active", btn.dataset.target === device);
    const activeMode = mobile ? modes[device] || "flow" : "flow";
    for (const btn of modeBtns) btn.classList.toggle("active", btn.dataset.mode === activeMode);
    // 预览框与网格的显隐交给 CSS（body.editing + edit-mobile/edit-phone/edit-pad
    // + edit-flow/edit-grid 组合）
    document.body.classList.toggle("edit-mobile", mobile);
    document.body.classList.toggle("edit-phone", device === "phone");
    document.body.classList.toggle("edit-pad", device === "pad");
    document.body.classList.toggle("edit-flow", mobile && activeMode === "flow");
    document.body.classList.toggle("edit-grid", mobile && activeMode === "grid");
    const preview = mobile && activeMode === "flow" && cur.dashboard.flowView;
    document.body.classList.toggle("flow-preview", preview);
    if (flowBtn) {
      flowBtn.classList.toggle("hidden", !(mobile && activeMode === "flow"));
      flowBtn.textContent = preview ? "调整顺序" : "流式预览";
      flowBtn.classList.toggle("active", preview);
    }
    renderChips();
  }

  function setDevice(next) {
    // 只接受 desk / phone / pad：模式按钮也带 editbar-target-btn 样式类，
    // 事件选择器已限定在 #editbar-target 内，这里再兜一层防 dataset 缺失
    if (next !== "desk" && !targets[next]) return;
    if (next === device) return;
    if (drag) finishDrag(true);
    device = next;
    if (device === "desk") {
      exitFlowPreview();
    } else if (cur.dashboard.flowView) {
      // 切设备时保留预览状态没有意义（不同网格），统一回到编辑视图
      exitFlowPreview();
    }
    applySelection();
  }

  function setMode(next) {
    if (device === "desk" || !targets[device]) return;
    if (next !== "flow" && next !== "grid") return;
    if (modes[device] === next) return;
    if (drag) finishDrag(true);
    exitFlowPreview();
    modes[device] = next;
    if (onModeChange) onModeChange(device, next);
    applySelection();
  }

  function exitFlowPreview() {
    for (const dev of ["phone", "pad"]) {
      if (!targets[dev]) continue;
      for (const md of ["flow", "grid"]) {
        const t = targets[dev][md];
        if (t && t.dashboard.flowView) t.dashboard.setFlowView(false);
      }
    }
    document.body.classList.remove("flow-preview");
    if (flowBtn) {
      flowBtn.textContent = "流式预览";
      flowBtn.classList.remove("active");
    }
  }

  function showNotice(msg) {
    if (!noticeEl) return;
    noticeEl.textContent = msg;
    noticeEl.classList.add("show");
    clearTimeout(noticeTimer);
    noticeTimer = setTimeout(() => noticeEl.classList.remove("show"), 2600);
  }

  /* ---- 网格度量（按当前目标的容器量） ---- */

  function metrics() {
    // 移动预览网格的 gap 是 0（卡片间距由 .mob-card 的固定边距画），
    // 不能写成 `parseFloat(...) || 14`——0 是合法值，会被或成桌面默认的 14
    const rawGap = parseFloat(getComputedStyle(cur.grid).columnGap);
    const gap = Number.isFinite(rawGap) ? rawGap : 14;
    const c = cols();
    const cellW = (cur.grid.clientWidth - (c - 1) * gap) / c;
    // 固定一屏：预览框高度被 rows 等分（与 App 端按 rows 的百分比定位一致）；
    // 桌面 / 流式编辑：用 profile.rowUnit（44 / 92）。
    const p = profile();
    const cellH = p.rows
      ? Math.max(1, cur.grid.clientHeight / p.rows)
      : p.rowUnit || layout.ROW_UNIT;
    return { gap, cellW, cellH };
  }

  /** 拖动落点：卡片左上角（指针位置 - 抓住点偏移）换算成网格格坐标。
   * 加半格距再取整：像素坐标由浮点几何算出（273.4999…），直接 floor 会偏左一格。 */
  function targetCell(e, m) {
    const gridRect = cur.grid.getBoundingClientRect();
    const px = e.clientX - gridRect.left - drag.grabX;
    const py = e.clientY - gridRect.top + cur.grid.scrollTop - drag.grabY;
    const p = profile();
    return {
      x: clamp(
        Math.floor((px + m.gap / 2) / (m.cellW + m.gap)),
        0,
        cols() - drag.item.w,
      ),
      y: clamp(
        Math.floor((py + m.gap / 2) / (m.cellH + m.gap)),
        0,
        p.rows ? Math.max(0, p.rows - drag.item.h) : 2000,
      ),
    };
  }

  /* ---- 缩放：目标尺寸 → 不压到别人的最大可行尺寸 ---- */

  function pickAllowedSize(item, desiredW, desiredH) {
    const def = item.def;
    const c = cols();
    const p = profile();
    const maxW = Math.min(def.maxW, c - item.x);
    const minW = Math.min(def.minW, c);
    const maxH = p.rows ? Math.min(def.maxH, p.rows - item.y) : def.maxH;
    const dw = clamp(desiredW, minW, Math.max(minW, maxW));
    const dh = clamp(desiredH, Math.min(def.minH, maxH), maxH);
    const others = cur.dashboard.items.filter((o) => o !== item);
    for (let w = dw; w >= minW; w -= 1) {
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
    for (const o of cur.dashboard.items) {
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
    const { order, def } = cur.dashboard.profile;
    const mounted = new Set(cur.dashboard.items.map((i) => i.type));
    chipsEl.innerHTML = order
      .filter((t) => !mounted.has(t))
      .map((t) => {
        const d = def(t);
        return `<button type="button" class="editbar-chip" data-type="${t}">＋ ${d.title}</button>`;
      })
      .join("");
  }

  function setEditing(on) {
    if (editing === on) return;
    if (drag) finishDrag(true);
    editing = on;
    document.body.classList.toggle("editing", on);
    if (on) {
      applySelection();
    } else {
      exitFlowPreview();
      document.body.classList.remove("edit-mobile", "edit-phone", "edit-pad", "edit-flow", "edit-grid");
    }
    editbar.classList.toggle("hidden", !on);
    editBtn.classList.toggle("active", on);
    editBtn.textContent = on ? "完成" : "编辑布局";
    if (on) renderChips();
  }

  editBtn.addEventListener("click", () => setEditing(!editing));
  resetBtn.addEventListener("click", () => {
    cur.dashboard.reset();
    renderChips();
  });
  chipsEl.addEventListener("click", (e) => {
    const chip = e.target.closest(".editbar-chip");
    if (!chip) return;
    const added = cur.dashboard.add(chip.dataset.type);
    if (!added && profile().rows) showNotice(`固定一屏（${profile().cols}×${profile().rows}）已满，先移除一个组件`);
    renderChips();
  });
  if (flowBtn) {
    flowBtn.addEventListener("click", () => {
      if (device === "desk" || modes[device] !== "flow") return;
      const on = !cur.dashboard.flowView;
      cur.dashboard.setFlowView(on);
      applySelection();
    });
  }

  // 移除按钮：走 click（不与拖拽抢 pointerdown）。五个网格共用一套委托逻辑。
  for (const dev of Object.values(targets)) {
    const entries = dev.flow ? [dev.flow, dev.grid] : [dev];
    for (const { dashboard, grid } of entries) {
      grid.addEventListener("click", (e) => {
        if (!editing || grid !== cur.grid || cur.dashboard.flowView) return;
        const btn = e.target.closest(".w-remove");
        if (!btn) return;
        const item = dashboard.items.find((i) => i.el === btn.closest(".widget"));
        if (item) {
          dashboard.remove(item.type);
          renderChips();
        }
      });

      /* ---- 拖拽 / 缩放（Pointer Events；非当前目标的事件直接忽略） ---- */

      grid.addEventListener("pointerdown", (e) => {
        if (!editing || grid !== cur.grid || cur.dashboard.flowView || e.button !== 0 || drag) return;
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
          // 固定一屏装不下时整组回滚（含被交换的受害卡）
          snapshot: dashboard.getLayout(),
        };
        if (drag.mode === "move") {
          card.classList.add("dragging");
          drag.ph = document.createElement("div");
          drag.ph.className = "grid-ph";
          cur.grid.append(drag.ph);
          movePlaceholder(e);
        } else {
          card.classList.add("resizing");
        }
        card.setPointerCapture(e.pointerId);
      });
    }
  }

  function movePlaceholder(e) {
    const cell = targetCell(e, drag);
    drag.target = cell;
    drag.ph.style.gridColumn = `${cell.x + 1} / span ${drag.item.w}`;
    drag.ph.style.gridRow = `${cell.y + 1} / span ${drag.item.h}`;
  }

  function restoreSnapshot(snapshot) {
    for (const rect of snapshot) {
      const it = cur.dashboard.item(rect.type);
      if (it) Object.assign(it, rect);
    }
    cur.dashboard.sync();
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
    const { item, mode, ph, snapshot } = drag;
    item.el.classList.remove("dragging", "resizing");
    item.el.style.transform = "";
    if (ph) ph.remove();

    if (!cancelled) {
      if (mode === "move" && drag.target) {
        const old = { x: item.x, y: item.y };
        const target = { x: drag.target.x, y: drag.target.y };
        if (target.x !== old.x || target.y !== old.y) {
          // 落点压到别人 → 与「被压面积最大」的那块交换（尺寸不同时换过去可能撞到
          // 第三块，残余重叠由 settle 的清洗兜底）
          const victim = pickVictim(item, target);
          if (victim) {
            victim.x = Math.min(old.x, cols() - victim.w);
            victim.y = old.y;
          }
          item.x = target.x;
          item.y = target.y;
          if (cur.dashboard.settle() === false) {
            restoreSnapshot(snapshot);
            showNotice("固定一屏放不下这次调整，已还原");
          }
        }
      } else if (mode === "resize") {
        if (cur.dashboard.settle() === false) {
          restoreSnapshot(snapshot);
          showNotice("固定一屏放不下这次调整，已还原");
        }
      }
    } else {
      restoreSnapshot(snapshot);
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

  // 启动时按调用方给定的设备 / 模式回显（默认桌面）
  device = "desk";
  applySelection();
  return {
    /** 异步读到后端 / 本地存档后刷新某设备的激活模式 */
    setModes(next) {
      if (!next) return;
      if (next.phone === "flow" || next.phone === "grid") modes.phone = next.phone;
      if (next.pad === "flow" || next.pad === "grid") modes.pad = next.pad;
      if (editing) applySelection();
    },
  };
}
