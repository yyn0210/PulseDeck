/**
 * 任务栏挂件窗口的前端。
 *
 * 与主窗口解耦，只向后端要两样东西：
 *   - `get_taskbar_config` 显示哪些指标（设置页改了这里 3 秒内跟上）
 *   - `taskbar_tick`       读数 + 任务栏朝向 / 主题（顺带当心跳）
 *
 * 再加上一件反向的事：把量出来的**内容尺寸**回传后端（`taskbar_fit`）。
 * 挂件多宽完全由内容决定——与其在 Rust 里硬编码「每个指标占多少像素」（换字体 / 改文案就全错），
 * 不如让浏览器量自己。数字变宽时窗口往左长，贴着通知区域的右端不动。
 *
 * 挂件的位置不归这里管：贴到哪、什么时候藏起来（全屏应用在前台）都由 Rust 侧的看护线程做，
 * 因为窗口一旦被藏起来，Chromium 会把页面定时器节流到分钟级，靠页面自己回不来。
 */
import { invoke } from "@tauri-apps/api/core";
import { fmtBytes } from "./modules/format.js";
import "./taskbar.css";

/* ---------------- 指标定义 ---------------- */

/* 竖排任务栏（Win10 把任务栏放到屏幕左右侧）那一竖条只有 40~60px 宽，
 * 去掉内边距后留给数字的地方不到 30px —— `↓1.24MB/s` 必然被裁掉。
 * 所以竖排时改用紧凑格式：只留一位有效单位（`↓1.2M`），功率不留小数。 */
let compact = false;

/** 每秒速率：`1.2MB/s`。单位里不留空格——任务栏上每一像素都要省，`fmtBytes` 的 "1.2 MB" 太占地方 */
function rate(bytesPerSec) {
  if (!Number.isFinite(bytesPerSec)) return "--";
  if (compact) {
    const units = ["B", "K", "M", "G", "T"];
    let v = Number(bytesPerSec);
    let i = 0;
    while (v >= 1024 && i < units.length - 1) {
      v /= 1024;
      i += 1;
    }
    return `${v >= 100 || i === 0 ? Math.round(v) : v.toFixed(1)}${units[i]}`;
  }
  const text = fmtBytes(bytesPerSec);
  return text === "--" ? "--" : `${text.replace(" ", "")}/s`;
}

function pct(v) {
  return Number.isFinite(v) ? `${Math.round(v)}%` : "--";
}

function tempText(v) {
  return Number.isFinite(v) ? `${Math.round(v)}℃` : "--";
}

function watts(v) {
  if (!Number.isFinite(v)) return "--";
  if (compact) return `${v >= 10 ? Math.round(v) : v.toFixed(1)}W`;
  return v >= 100 ? `${Math.round(v)}W` : `${v.toFixed(1)}W`;
}

/** 占用类阈值：60% 起黄、85% 起红（与监控页每核负载的配色一致） */
function pctState(v) {
  if (!Number.isFinite(v)) return "";
  if (v >= 85) return "is-hot";
  if (v >= 60) return "is-warn";
  return "";
}

/** 温度阈值：74℃ 起黄、84℃ 起红（与宠物分档的色标同源） */
function tempState(v) {
  if (!Number.isFinite(v)) return "";
  if (v >= 84) return "is-hot";
  if (v >= 74) return "is-warn";
  return "";
}

/** 电量低了才需要看：≤30% 黄、≤15% 红 */
function batState(v) {
  if (!Number.isFinite(v)) return "";
  if (v <= 15) return "is-hot";
  if (v <= 30) return "is-warn";
  return "";
}

/** 每项指标：label 可以为空（速率类自带箭头/单位），body 给出数值区的 HTML，
 *  state 给出整项的告警档位，missing 决定整项是否压暗（读不到的诚实降级）。 */
const METRIC_DEFS = {
  cpu: {
    label: "CPU",
    body: (m) => `<b class="tb-v">${pct(m?.cpuPct)}</b>`,
    state: (m) => pctState(m?.cpuPct),
  },
  mem: {
    label: "内存",
    body: (m) => `<b class="tb-v">${pct(m?.memPct)}</b>`,
    state: (m) => pctState(m?.memPct),
  },
  gpu: {
    label: "GPU",
    body: (m) => `<b class="tb-v">${pct(m?.gpuPct)}</b>`,
    state: (m) => pctState(m?.gpuPct),
    missing: (m) => !Number.isFinite(m?.gpuPct),
  },
  net: {
    label: "",
    body: (m) =>
      `<b class="tb-rx">↓${rate(m?.netRxBps)}</b><b class="tb-tx">↑${rate(m?.netTxBps)}</b>`,
    state: () => "",
  },
  disk: {
    label: "",
    body: (m) =>
      `<i class="tb-k">读</i><b class="tb-io">${rate(m?.diskReadBps)}</b>` +
      `<i class="tb-k">写</i><b class="tb-io">${rate(m?.diskWriteBps)}</b>`,
    state: () => "",
  },
  bat: {
    label: "电池",
    body: (m) => `<b class="tb-v">${pct(m?.batPct)}</b>`,
    state: (m) => batState(m?.batPct),
    missing: (m) => !Number.isFinite(m?.batPct),
  },
  temp: {
    label: "温度",
    body: (m) => `<b class="tb-v">${tempText(m?.cpuTempC)}</b>`,
    state: (m) => tempState(m?.cpuTempC),
    missing: (m) => !Number.isFinite(m?.cpuTempC),
  },
  power: {
    label: "功率",
    body: (m) => `<b class="tb-v">${watts(m?.powerW)}</b>`,
    state: () => "",
    missing: (m) => !Number.isFinite(m?.powerW),
  },
};

/* ---------------- DOM ---------------- */

const card = document.getElementById("tb");
const itemsBox = document.getElementById("tb-items");

/** 指标项按配置构建一次，之后每拍只换数值。
 * 只在「勾选项真的变了」时重建——否则每 3 秒轮询一次配置就白重建一遍 DOM。 */
const cells = [];
let builtMetrics = null;

function buildItems(metrics) {
  const signature = metrics.join(",");
  if (signature === builtMetrics) return;
  builtMetrics = signature;
  cells.length = 0;
  itemsBox.innerHTML = "";
  metrics.forEach((key) => {
    const def = METRIC_DEFS[key];
    if (!def) return;
    const item = document.createElement("span");
    item.className = "tb-item";
    item.dataset.key = key;
    item.innerHTML = def.label ? `<i class="tb-k">${def.label}</i>` : "";
    const body = document.createElement("span");
    body.className = "tb-body";
    item.appendChild(body);
    itemsBox.appendChild(item);
    cells.push({ def, item, body });
  });
  fit();
}

function render(m) {
  cells.forEach(({ def, item, body }) => {
    body.innerHTML = def.body(m);
    item.classList.toggle("is-missing", def.missing ? !!def.missing(m) : false);
    const state = def.state ? def.state(m) : "";
    item.classList.toggle("is-warn", state === "is-warn");
    item.classList.toggle("is-hot", state === "is-hot");
  });
}

/* ---------------- 尺寸回传 ---------------- */

/* 量出的是 `.tb` 的**自然尺寸**（CSS 像素）：它用 max-content 撑开，不受窗口大小影响，
 * 所以这里量出来的始终是「内容要多大」，不会和 set_size 形成互相追赶的循环。 */
let lastFit = "";

function fit() {
  const rect = card.getBoundingClientRect();
  const width = Math.ceil(rect.width);
  const height = Math.ceil(rect.height);
  if (width <= 0 || height <= 0) return;
  const key = `${width}x${height}`;
  if (key === lastFit) return;
  lastFit = key;
  invoke("taskbar_fit", { width, height }).catch((e) => console.error("调整挂件尺寸失败:", e));
}

// 内容跟着数字变宽变窄（速率单位会跳），ResizeObserver 兜住所有没显式调 fit 的场合
if (typeof ResizeObserver === "function") {
  new ResizeObserver(() => fit()).observe(card);
}

/* ---------------- 主循环 ---------------- */

let cfg = { enabled: true, metrics: ["cpu", "mem", "net"] };

async function loadConfig() {
  try {
    const next = await invoke("get_taskbar_config");
    if (next && Array.isArray(next.metrics)) {
      cfg = next;
      buildItems(cfg.metrics);
    }
  } catch (e) {
    console.error("读取任务栏显示配置失败:", e);
  }
}

async function tick() {
  try {
    const payload = await invoke("taskbar_tick");
    const m = payload && payload.metrics;
    // 主题（Windows 模式）与任务栏朝向都从后端拿：一个要读注册表，一个是 Rust 侧算的
    card.classList.toggle("tb--light", payload?.theme === "light");
    card.classList.toggle("tb--vertical", !!payload?.vertical);
    compact = !!payload?.vertical;
    render(m);
    fit();
    // 首拍之后才露出来（见 CSS 里 .tb 的 opacity）——先画默认观感再改成浅色主题会闪一下
    card.classList.add("is-ready");
  } catch (e) {
    // 后端还没就绪（启动瞬间）时先别显示半截内容
    console.error("读取任务栏读数失败:", e);
  }
}

// 单击整条挂件呼出主面板（主面板可能正藏在托盘里）。
// 窗口是不可激活的（WS_EX_NOACTIVATE），所以点击不会抢走当前应用的焦点。
document.addEventListener("click", () => {
  invoke("show_main_window").catch((e) => console.error("打开主窗口失败：", e));
});

// 重新可见时立刻补一拍：被全屏应用藏起来过（或长时间被遮挡）之后，
// 不等到下一个定时器就刷新，避免恢复瞬间显示的还是藏起来之前的旧数。
document.addEventListener("visibilitychange", () => {
  if (!document.hidden) tick();
});

(async function start() {
  await loadConfig();
  await tick();
  setInterval(tick, 1000);
  // 配置可能在设置页被改（指标增减），每 3 拍同步一次
  setInterval(loadConfig, 3000);
})();
