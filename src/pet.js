/**
 * 桌面宠物窗口的前端。
 *
 * 与主窗口解耦：这里只向后端要两样东西
 *   - `get_pet_config`  显示哪些指标、整体不透明度
 *   - `get_pet_metrics` CPU / 内存 / GPU 占用 + CPU 温度（只有这几个数，不做整份快照搬运）
 *
 * 形象是 `src/assets/pet/` 下预处理好的一套逐帧贴图（`tools/copy_pet_frames.py` 生成）：
 * **6 个温度档 × 每个档位 8 帧**。档位由 CPU 温度决定（读不到温度时退回 CPU 占用），
 * 档位内 8 帧循环播放；**跨档位时整段换成新档位的 8 帧序列**（从第 0 帧重新起播）。
 * 色标 `SCALE` 与设置页的色带、以及后端 `colorSource` 的取舍保持一致。
 */
import { invoke } from "@tauri-apps/api/core";
import "./pet.css";

/* ---------------- 温度 → 形象 ---------------- */

// 色标：温度越高越偏红。与设置页色带同源，同时用来给宠物加一圈同色投影。
const SCALE = [
  { v: 38, c: "#3b9dff" }, // 凉爽
  { v: 50, c: "#22d3a5" }, // 舒适
  { v: 63, c: "#a3d94a" }, // 温热
  { v: 74, c: "#f5c451" }, // 偏热
  { v: 84, c: "#ff9f43" }, // 烫
  { v: 94, c: "#ff5f6d" }, // 过热
];

// 形象档位（= 贴图文件名里的数字），与 SCALE 的刻度一一对应
const TIERS = SCALE.map((s) => s.v);

// 达到这个档位就算「过热」：脚下的温度文字变橙、动画节奏加快
const HOT_TIER = 84;

// 每帧停留时长；过热时加快（视觉上更「喘」），但绝不能快到看不清动作。
// 之前 130/95ms 显得太碎（≈7.7/10.5fps），调慢到 260/200ms 后整段动画能
// 完整播完 8 帧（约 2.1s / 1.6s）、各档姿态差异也能看清。
const FRAME_MS = 260;
const FRAME_MS_HOT = 200;

// 逐帧贴图：`t<档位>_<帧号>.png`，构建期由 vite 收集成 URL 表
// 之前出 WebP 会出现色偏（alpha 边缘和 RGB 错位），改为 PNG 走 RGBA 通道
const FRAME_MODULES = import.meta.glob("./assets/pet/t*_*.png", {
  eager: true,
  query: "?url",
  import: "default",
});

/** 档位 → 按帧号排好序的 URL 数组 */
const TIER_FRAMES = new Map();
for (const [path, url] of Object.entries(FRAME_MODULES)) {
  const m = /t(\d+)_(\d+)\.png$/.exec(path);
  if (!m) continue;
  const tier = Number(m[1]);
  const frame = Number(m[2]);
  if (!TIER_FRAMES.has(tier)) TIER_FRAMES.set(tier, []);
  TIER_FRAMES.get(tier)[frame] = url;
}
// 只保留确实有图的档位，避免某档缺图时切过去是空白
for (const [tier, list] of [...TIER_FRAMES]) {
  const frames = list.filter(Boolean);
  if (frames.length) TIER_FRAMES.set(tier, frames);
  else TIER_FRAMES.delete(tier);
}
// 贴图一张都没收到时，窗口会表现为「纯透明 = 什么都没有」。这种失败没有任何 UI 反馈，
// 所以至少要在控制台留一句（排查时配合 PULSEDECK_PET_TRACE 用）
if (!TIER_FRAMES.size) {
  console.error(
    "宠物贴图为空：src/assets/pet/ 下应有 t<档位>_<帧>.png。先跑 python tools/copy_pet_frames.py 生成。",
  );
}

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

/** 同色半透明（给投影用） */
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
      return rgbToHex(hexToRgb(a.c).map((c, k) => c + (hexToRgb(b.c)[k] - c) * t));
    }
  }
  return last.c;
}

/** 数值 → 戏偶档位：取最接近的刻度（越界时落在两端档位） */
function tierAt(value) {
  const v = Number.isFinite(value) ? value : TIERS[0];
  let best = TIERS[0];
  for (const t of TIERS) {
    if (Math.abs(t - v) < Math.abs(best - v)) best = t;
  }
  return best;
}

/* ---------------- DOM ---------------- */

const root = document.getElementById("pet");
const img = document.getElementById("pet-img");
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

/** 当前档位的颜色写进 CSS 变量：`--pet` 记录当前档位色，`--pet-glow` 给贴图加同色投影 */
function applyColor(hex) {
  root.style.setProperty("--pet", hex);
  root.style.setProperty("--pet-glow", rgba(hex, 0.45));
}

/* ---------------- 逐帧播放 ---------------- */

// 当前档位与已播到第几帧。跨档位时重置为第 0 帧并立刻换图。
let curTier = null;
let curFrame = 0;
let hot = false;

function paintFrame() {
  const list = TIER_FRAMES.get(curTier);
  if (!list || !list.length) return;
  img.src = list[curFrame % list.length];
}

function setTier(tier) {
  if (tier === curTier) return;
  curTier = tier;
  curFrame = 0;
  paintFrame();
}

function stepFrame() {
  const list = TIER_FRAMES.get(curTier);
  if (!list || !list.length) return;
  curFrame = (curFrame + 1) % list.length;
  paintFrame();
}

// 帧循环独立于读数节拍：读数每秒一次，动画不能跟着卡成 1fps。
// 间隔按档位在定时器里取，避免跨档位时重建定时器。
(function loop() {
  stepFrame();
  setTimeout(loop, hot ? FRAME_MS_HOT : FRAME_MS);
})();

// 首帧之前先把 48 张贴图全部预解码，避免切档位那一下闪白
if (img) {
  for (const list of TIER_FRAMES.values()) {
    for (const url of list) {
      const pre = new Image();
      pre.src = url;
    }
  }
  // 贴图就绪前先摆一张最冷档，避免窗口刚出来是空白
  setTier(TIERS[0]);
}

function render(m, cfg) {
  // 温度是首选依据；读不到就退回 CPU 占用（后端已把取舍写进 colorSource）
  const temp = m && Number.isFinite(m.cpuTempC) ? m.cpuTempC : null;
  const byTemp = temp != null;
  const heat = byTemp ? temp : m ? Number(m.cpuPct) || 0 : 0;

  applyColor(colorAt(heat));
  const tier = tierAt(heat);
  hot = tier >= HOT_TIER;
  root.classList.toggle("is-hot", hot);
  if (m) setTier(tier);

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
    root.title = `CPU 温度 ${temp.toFixed(0)}°C · 形象随温度变化，双击打开主窗口`;
  } else {
    foot.textContent = "温度 -- · 按占用切换形象";
    root.title = `CPU 温度不可用，形象暂按 CPU 占用切换 · 双击打开主窗口`;
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
    invoke("show_main_window").catch((err) => console.error("打开主窗口失败：", err));
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
