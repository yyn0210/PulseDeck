/**
 * 桌面宠物窗口的前端。
 *
 * 与主窗口解耦：这里只向后端要两样东西
 *   - `get_pet_config`  显示哪些指标、整体不透明度
 *   - `get_pet_metrics` CPU / 内存 / GPU 占用 + CPU 温度（只有这几个数，不做整份快照搬运）
 *
 * 身体颜色由 CPU 温度驱动；温度读不到时（多数机器需要装 PawnIO 驱动才有）退回按 CPU 占用着色，
 * 并在悬浮提示里说明，避免出现「颜色变了但不知道为什么」。
 */
import { invoke } from "@tauri-apps/api/core";
import "./pet.css";

/* ---------------- 温度 → 颜色 ---------------- */

// 色标：温度越高越偏红。改动这里即可调整配色，两端之外的温度取端点色。
const SCALE = [
  { v: 38, c: "#3b9dff" }, // 凉爽
  { v: 50, c: "#22d3a5" }, // 舒适
  { v: 63, c: "#a3d94a" }, // 温热
  { v: 74, c: "#f5c451" }, // 偏热
  { v: 84, c: "#ff9f43" }, // 烫
  { v: 94, c: "#ff5f6d" }, // 过热
];

// 超过这个温度开始冒汗、呼吸加快
const HOT_AT = 80;

const METRIC_DEFS = {
  cpu: { label: "CPU", color: "#4f8cff", pick: (m) => m.cpuPct },
  mem: { label: "内存", color: "#22d3a5", pick: (m) => m.memPct },
  gpu: { label: "GPU", color: "#35d6c4", pick: (m) => m.gpuPct },
};

function hexToRgb(hex) {
  const h = hex.replace("#", "");
  const n = parseInt(
    h.length === 3
      ? h
          .split("")
          .map((c) => c + c)
          .join("")
      : h,
    16,
  );
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

function rgbToHex([r, g, b]) {
  const c = (v) => Math.max(0, Math.min(255, Math.round(v))).toString(16).padStart(2, "0");
  return `#${c(r)}${c(g)}${c(b)}`;
}

/** amount > 0 往白色混（提亮），< 0 往黑色混（压暗） */
function shade(hex, amount) {
  const rgb = hexToRgb(hex);
  const target = amount > 0 ? 255 : 0;
  const t = Math.abs(amount);
  return rgbToHex(rgb.map((v) => v + (target - v) * t));
}

function rgba(hex, alpha) {
  const [r, g, b] = hexToRgb(hex);
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}

/** 按色标插值出温度对应的颜色 */
function colorAt(value) {
  const v = Number.isFinite(value) ? value : SCALE[0].v;
  if (v <= SCALE[0].v) return SCALE[0].c;
  const last = SCALE[SCALE.length - 1];
  if (v >= last.v) return last.c;
  for (let i = 0; i < SCALE.length - 1; i += 1) {
    const a = SCALE[i];
    const b = SCALE[i + 1];
    if (v <= b.v) {
      const t = (v - a.v) / (b.v - a.v);
      return rgbToHex(
        hexToRgb(a.c).map((c, k) => c + (hexToRgb(b.c)[k] - c) * t),
      );
    }
  }
  return last.c;
}

/* ---------------- DOM ---------------- */

const root = document.getElementById("pet");
const rowsBox = document.getElementById("pet-rows");
const foot = document.getElementById("pet-foot");

/** 指标行按配置构建一次，之后每拍只改数字与进度条。
 * 只在「勾选项真的变了」时重建——否则每 3 秒轮询一次配置就会白重建一遍 DOM。 */
const rowCache = new Map();
let builtMetrics = null;

function buildRows(metrics) {
  const signature = metrics.join(",");
  if (signature === builtMetrics) return;
  builtMetrics = signature;
  rowCache.clear();
  rowsBox.innerHTML = "";
  metrics.forEach((key) => {
    const def = METRIC_DEFS[key];
    if (!def) return;
    const row = document.createElement("div");
    row.className = "pet-row";
    row.innerHTML =
      `<span class="pet-k">${def.label}</span>` +
      `<span class="pet-bar"><i></i></span>` +
      `<b class="pet-v">--</b>`;
    const bar = row.querySelector(".pet-bar > i");
    bar.style.background = def.color;
    rowsBox.appendChild(row);
    rowCache.set(key, { row, bar, value: row.querySelector(".pet-v") });
  });
  if (!rowCache.size) {
    rowsBox.innerHTML = `<div class="pet-row pet-row--empty">未选择指标</div>`;
  }
}

function applyColor(hex) {
  root.style.setProperty("--pet", hex);
  root.style.setProperty("--pet-dark", shade(hex, -0.35));
  root.style.setProperty("--pet-soft", shade(hex, 0.55));
  root.style.setProperty("--pet-glow", rgba(hex, 0.45));
}

function render(m, cfg) {
  // 温度是首选着色依据；读不到就退回 CPU 占用（后端已把取舍写进 colorSource）
  const temp = m && Number.isFinite(m.cpuTempC) ? m.cpuTempC : null;
  const byTemp = temp != null;
  const heat = byTemp ? temp : m ? Number(m.cpuPct) || 0 : 0;

  applyColor(colorAt(heat));
  root.classList.toggle("is-hot", heat >= HOT_AT);

  for (const [key, cell] of rowCache) {
    const def = METRIC_DEFS[key];
    const raw = m ? def.pick(m) : null;
    const v = Number.isFinite(raw) ? Math.max(0, Math.min(100, raw)) : null;
    cell.bar.style.width = `${v ?? 0}%`;
    cell.value.textContent = v == null ? "--" : `${v.toFixed(0)}%`;
    cell.row.classList.toggle("is-missing", v == null);
  }

  if (byTemp) {
    foot.textContent = `温度 ${temp.toFixed(0)}°C`;
    root.title = `CPU 温度 ${temp.toFixed(0)}°C · 身体颜色随温度变化，双击打开主窗口`;
  } else {
    foot.textContent = "温度 -- · 按占用着色";
    root.title = `CPU 温度不可用，身体颜色暂按 CPU 占用着色 · 双击打开主窗口`;
  }

  if (cfg) root.style.setProperty("--pet-opacity", String(cfg.opacity ?? 1));
}

/* ---------------- 主循环 ---------------- */

let cfg = { enabled: true, metrics: ["cpu", "mem", "gpu"], opacity: 1 };

async function loadConfig() {
  try {
    const next = await invoke("get_pet_config");
    if (next && Array.isArray(next.metrics)) {
      cfg = next;
      buildRows(cfg.metrics);
    }
  } catch (e) {
    console.error("读取桌面宠物配置失败:", e);
  }
}

async function tick() {
  try {
    const m = await invoke("get_pet_metrics");
    render(m, cfg);
    // 心跳：设置页据此显示「运行中」，也是排查「窗口在但没画出来」的唯一凭据
    // （透明无边框窗口一旦没渲染，界面上就是「什么都没有」，只能靠这个区分）
    invoke("pet_heartbeat").catch(() => {});
  } catch (e) {
    // 后端还没就绪（启动瞬间）时先给个占位，不要弹错误打断视觉
    render(null, cfg);
    console.error("读取宠物读数失败:", e);
  }
}

// 双击呼出主窗口。
//
// 两个坑叠在一起，所以既不能用原生 `dblclick`，也不能用「mouseup 自己合成」：
//   1. Tauri 的 drag.js 在 mousedown 上 `preventDefault` + `stopImmediatePropagation`，
//      浏览器不会再合成 click / dblclick —— 原生 dblclick 永远收不到；
//   2. 拖动走的是 Windows 的模态拖动循环（WM_NCLBUTTONDOWN/HTCAPTION），
//      **mouseup 根本不派发到页面**，靠 mouseup 判双击也不成立。
//
// 唯一可靠信号是 mousedown 事件自带的 `e.detail`（第几次连击）——Tauri 自己
// 也是用 `e.detail === 2` 判双击的。这里在 **capture 阶段**监听：
//   * capture 一定早于 Tauri 的 bubble 监听，能先拿到事件；
//   * 双击时顺手 stopImmediatePropagation，阻止 Tauri 把宠物窗口
//     internal_toggle_maximize（该命令是默认授权的，不拦会被它吃掉这次双击）。
// 单击（detail === 1）保持沉默，交给 Tauri 启动窗口拖动。
document.addEventListener(
  "mousedown",
  (e) => {
    if (e.button !== 0 || e.detail !== 2) return;
    e.stopImmediatePropagation(); // 别让 Tauri 用这次双击去 toggle maximize
    e.preventDefault();
    invoke("show_main_window").catch((err) =>
      console.error("打开主窗口失败：", err),
    );
  },
  { capture: true },
);

// 重新可见时立刻补一拍。
// 桌面宠物经常被别的窗口盖住，理论上 Chromium 可能把「被遮挡且非前台」的窗口
// 当后台页处理（暂停渲染、节流定时器），这里兜一层：只要页面重新可见，
// 马上刷新，不等下一个定时器。
document.addEventListener("visibilitychange", () => {
  if (!document.hidden) tick();
});

(async function start() {
  await loadConfig();
  await tick();
  setInterval(tick, 1000);
  // 配置可能在设置页被改（指标增减 / 透明度），每 3 拍同步一次
  setInterval(loadConfig, 3000);
})();
