/**
 * 经典贴图宠物（`mode=classic`）：行为与 KuroCat 移植前的版本完全一致。
 *
 * 形象是 `src/assets/pet/` 下预处理好的一套逐帧贴图（`tools/copy_pet_frames.py` 生成）：
 * **6 个温度档 × 每个档位 8 帧**。档位由 CPU 温度决定（读不到温度时退回 CPU 占用），
 * 档位内 8 帧循环播放；跨档位时整段换成新档位的 8 帧序列（从第 0 帧重新起播）。
 *
 * 本模块只管渲染；指标读数、心跳、双击呼出主窗口由 `src/pet.js` 统一负责。
 */

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
const FRAME_MS = 260;
const FRAME_MS_HOT = 200;

// 逐帧贴图：`t<档位>_<帧号>.png`，构建期由 vite 收集成 URL 表
const FRAME_MODULES = import.meta.glob("../assets/pet/t*_*.png", {
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
export function rgba(hex, alpha) {
  const [r, g, b] = hexToRgb(hex);
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}

/** 按色标插值出温度对应的颜色 */
export function colorAt(value) {
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
export function tierAt(value) {
  const v = Number.isFinite(value) ? value : TIERS[0];
  let best = TIERS[0];
  for (const t of TIERS) {
    if (Math.abs(t - v) < Math.abs(best - v)) best = t;
  }
  return best;
}

export { HOT_TIER, TIERS };

export function createClassicPet(host) {
  host.innerHTML = `
    <div class="pet-stage" data-tauri-drag-region="deep">
      <img class="pet-img" id="pet-img" alt="" draggable="false" data-tauri-drag-region="deep" />
    </div>
    <div class="pet-panel" data-tauri-drag-region="deep">
      <div class="pet-rows" id="pet-rows"></div>
      <div class="pet-foot" id="pet-foot">温度 --</div>
    </div>
  `;
  const img = host.querySelector(".pet-img");
  const rowsBox = host.querySelector(".pet-rows");
  const foot = host.querySelector(".pet-foot");

  const rowCache = new Map();
  let builtMetrics = null;
  let curTier = null;
  let curFrame = 0;
  let hot = false;
  let timer = null;
  let destroyed = false;

  function buildRows(metrics) {
    const signature = (metrics || []).join(",");
    if (signature === builtMetrics) return;
    builtMetrics = signature;
    rowCache.clear();
    rowsBox.innerHTML = "";
    (metrics || []).forEach((key) => {
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
    document.documentElement.style.setProperty("--pet", hex);
    document.documentElement.style.setProperty("--pet-glow", rgba(hex, 0.45));
  }

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
  function loop() {
    if (destroyed) return;
    stepFrame();
    timer = setTimeout(loop, hot ? FRAME_MS_HOT : FRAME_MS);
  }

  // 首帧之前先把所有贴图预解码，避免切档位那一下闪白
  for (const list of TIER_FRAMES.values()) {
    for (const url of list) {
      const pre = new Image();
      pre.src = url;
    }
  }
  // 贴图就绪前先摆一张最冷档，避免窗口刚出来是空白
  setTier(TIERS[0]);
  loop();

  return {
    setConfig(cfg) {
      buildRows(cfg?.metrics);
    },

    update(m, cfg) {
      buildRows(cfg?.metrics);
      const temp = m && Number.isFinite(m.cpuTempC) ? m.cpuTempC : null;
      const byTemp = temp != null;
      const heat = byTemp ? temp : m ? Number(m.cpuPct) || 0 : 0;

      applyColor(colorAt(heat));
      const tier = tierAt(heat);
      hot = tier >= HOT_TIER;
      host.classList.toggle("is-hot", hot);
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
        host.title = `CPU 温度 ${temp.toFixed(0)}°C · 形象随温度变化，双击打开主窗口`;
      } else {
        foot.textContent = "温度 -- · 按占用切换形象";
        host.title = `CPU 温度不可用，形象暂按 CPU 占用切换 · 双击打开主窗口`;
      }
    },

    destroy() {
      destroyed = true;
      if (timer !== null) clearTimeout(timer);
      timer = null;
    },
  };
}
