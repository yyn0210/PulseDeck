/**
 * 打字猫渲染器（`mode=cat`）：2D 手绘 / 3D 手办 / 鲸鱼娘三种引擎的统一挂载层。
 *
 * 职责：
 * - 挂 canvas 与指标条，按配置选择引擎（3D 按需动态导入 three，失败回退 2D）；
 * - 接 Rust 的 `pet-input` / `pet-cursor` / `pet-privacy` 事件，转给引擎；
 * - 系统信息融合：温度档位色 → 脚下光晕 + 指标条温度强调色，过热档 → 脸红/汗滴；
 * - 摸头眯眼、呼噜声开关、鼠标穿透时的防误触。
 *
 * 引擎本身是 KuroCat 的原样移植（见 `src/pet/engine/`），这里只做 PulseDeck 侧的装配。
 */
import { listen } from "@tauri-apps/api/event";

import { KuroCatEngine } from "./engine/catEngine";
import { DEFAULT_BUILTIN_CATS } from "./engine/catSkins";
import { catAudio } from "./engine/audio";
import { colorAt, HOT_TIER, rgba } from "./classic.js";
import { createMetricsBar } from "./metrics.js";

/** 与 KuroCat 的窗口相对命中区同一套椭圆近似（猫头/身体 + 键盘） */
function overInteractive(rx, ry, is108) {
  const catCx = is108 ? 0.51 : 0.48;
  const catRx = is108 ? 0.21 : 0.25;
  const dx = (rx - catCx) / catRx;
  const dy = (ry - 0.49) / 0.27;
  const overCat = dx * dx + dy * dy <= 1;
  return { overCat };
}

function findSkin(id) {
  return DEFAULT_BUILTIN_CATS.find((s) => s.id === id) || DEFAULT_BUILTIN_CATS[0];
}

/**
 * @param {HTMLElement} host 宠物根容器 `#pet`
 * @param {object} cfg 后端清洗后的宠物配置
 */
export async function createCatPet(host, cfg) {
  host.classList.add("pet--cat");
  host.innerHTML = `
    <div class="cat-stage" id="cat-stage" data-tauri-drag-region="deep">
      <div class="cat-glow" id="cat-glow" aria-hidden="true"></div>
      <canvas class="cat-canvas" id="cat-canvas-2d" data-tauri-drag-region="deep"></canvas>
      <canvas class="cat-canvas" id="cat-canvas-3d" hidden data-tauri-drag-region="deep"></canvas>
      <div class="cat-heat" id="cat-heat" aria-hidden="true">
        <i class="cat-blush cat-blush-l"></i>
        <i class="cat-blush cat-blush-r"></i>
        <i class="cat-sweat cat-sweat-a"></i>
        <i class="cat-sweat cat-sweat-b"></i>
      </div>
      <div class="cat-fallback" id="cat-fallback" hidden></div>
    </div>
    <div class="cat-metrics" id="cat-metrics" data-tauri-drag-region="deep"></div>
  `;

  const stage = host.querySelector("#cat-stage");
  const glow = host.querySelector("#cat-glow");
  const canvas2d = host.querySelector("#cat-canvas-2d");
  const canvas3d = host.querySelector("#cat-canvas-3d");
  const heatEl = host.querySelector("#cat-heat");
  const fallbackEl = host.querySelector("#cat-fallback");
  const metricsHost = host.querySelector("#cat-metrics");
  const metrics = createMetricsBar(metricsHost);

  let engine = null;
  let is3d = false;
  let privacyOn = false;
  let config = cfg;
  let destroyed = false;
  let fallbackReason = "";
  const unlisteners = [];

  /* ------------------------------ 引擎挂载 ------------------------------ */

  async function mountEngine() {
    const want3d = config.renderMode === "3d" && config.skin !== "dafeiyu";
    if (want3d) {
      try {
        const { KuroCatEngine3D } = await import("./engine/catEngine3d");
        engine = new KuroCatEngine3D(canvas3d, { embedded: true });
        is3d = true;
        canvas3d.hidden = false;
        canvas2d.hidden = true;
        canvas3d.addEventListener("webglcontextlost", onContextLost, { once: true });
        try {
          localStorage.removeItem("pd.cat3dError");
        } catch {
          /* 隐私模式下降级为仅本次运行 */
        }
      } catch (err) {
        fallbackReason = "当前环境不支持 WebGL2，已回退 2D 手绘渲染";
        console.warn("3D 引擎创建失败，回退 2D：", err);
        try {
          localStorage.setItem("pd.cat3dError", fallbackReason);
        } catch {
          /* ignore */
        }
        engine = null;
        is3d = false;
      }
    }
    if (!engine) {
      engine = new KuroCatEngine(canvas2d, { embedded: true });
      is3d = false;
      canvas2d.hidden = false;
      canvas3d.hidden = true;
    }

    fallbackEl.hidden = !fallbackReason;
    fallbackEl.textContent = fallbackReason;
    applyConfigToEngine();
    engine.resize();
    engine.start();
    publishInfo();
  }

  /** 调试/回归钩子：不参与渲染，只暴露当前引擎与关键配置 */
  function publishInfo() {
    window.__pdPetInfo = {
      engine: is3d ? "3d" : "2d",
      fallback: fallbackReason || null,
      skin: config.skin,
      layout: config.layout,
      orientation: config.orientation,
      privacy: privacyOn,
      collar: config.collar,
      headphones: config.headphones,
    };
  }

  /** 3D 创建成功后发现上下文不可用/丢失：释放 3D 并在同一窗口内重建 2D 引擎 */
  function fallbackTo2D(reason) {
    if (destroyed) return;
    fallbackReason = reason;
    try {
      localStorage.setItem("pd.cat3dError", reason);
    } catch {
      /* ignore */
    }
    try {
      engine?.stop?.();
      engine?.dispose?.();
    } catch {
      /* ignore */
    }
    engine = null;
    is3d = false;
    canvas3d.hidden = true;
    canvas2d.hidden = false;
    fallbackEl.hidden = false;
    fallbackEl.textContent = reason;
    engine = new KuroCatEngine(canvas2d, { embedded: true });
    applyConfigToEngine();
    engine.resize();
    engine.start();
    publishInfo();
  }

  function onContextLost(event) {
    event.preventDefault();
    console.warn("3D WebGL 上下文丢失，回退 2D");
    fallbackTo2D("3D 上下文丢失，已回退 2D 手绘渲染");
  }

  function applyConfigToEngine() {
    if (!engine) return;
    const skin = findSkin(config.skin);
    if (is3d) {
      engine.setCatSkin(skin);
      engine.setCollar(config.collar || "none");
      engine.setHeadphones(config.headphones === "on");
    } else {
      engine.setCatAppearance(skin.id, skin);
    }
    engine.setKeyboardConfig(config.layout || "65", config.orientation || "cat");
    engine.showKeyBubbles = true;
    engine.setPrivacyMode(privacyOn);
    engine.setGameActive(false);
    catAudio.setEnabled(Boolean(config.sound));
  }

  /* ------------------------------ 输入接线 ------------------------------ */

  function onInput(p) {
    if (!engine || !p) return;
    switch (p.kind) {
      case "keyDown":
        engine.onKeyDown(p.vk, Boolean(p.repeat));
        break;
      case "keyUp":
        engine.onKeyUp(p.vk);
        break;
      case "mouseMove":
        engine.onGlobalMouseMove(p.nx, p.ny);
        break;
      case "mouseDown":
        engine.onMouseDown(p.button);
        break;
      case "mouseUp":
        engine.onMouseUp(p.button);
        break;
      case "mouseWheel":
        engine.onMouseWheel();
        break;
      case "privacyTyping":
        engine.onPrivacyTyping();
        break;
      default:
        break;
    }
  }

  function onCursor(c) {
    if (!engine || !c) return;
    const canvas = is3d ? canvas3d : canvas2d;
    const rect = canvas.getBoundingClientRect();
    const pageW = window.innerWidth || 1;
    const pageH = window.innerHeight || 1;
    const cx = ((c.relX * pageW) - rect.left) / Math.max(1, rect.width);
    const cy = ((c.relY * pageH) - rect.top) / Math.max(1, rect.height);
    const is108 = (config.layout || "65") === "108";
    let overCat = c.inside && overInteractive(cx, cy, is108).overCat;
    // 穿透锁定且没有按 Alt 时窗口根本不接收鼠标：不要触发摸头，避免「隔着窗口摸猫」
    if (config.clickThrough && !c.altOverride) overCat = false;
    host.classList.toggle("cat-hover", overCat);
    engine.setHover?.(overCat);
    engine.onWindowCursor(cx, cy, overCat);
  }

  function setPrivacy(active) {
    privacyOn = Boolean(active);
    host.classList.toggle("is-private", privacyOn);
    engine?.setPrivacyMode(privacyOn);
    if (privacyOn) catAudio.setPurrIntensity(0);
    if (engine) publishInfo();
  }

  async function wireEvents() {
    const safeListen = async (event, handler) => {
      try {
        const un = await listen(event, (ev) => handler(ev.payload));
        if (typeof un === "function") unlisteners.push(un);
      } catch {
        // 浏览器 mock 预览没有 Tauri 事件系统：忽略即可，猫仍能渲染与刷新指标
      }
    };
    await safeListen("pet-input", onInput);
    await safeListen("pet-cursor", onCursor);
    await safeListen("pet-privacy", setPrivacy);
  }

  /* ------------------------------ 温度融合 ------------------------------ */

  function update(m) {
    const temp = m && Number.isFinite(m.cpuTempC) ? m.cpuTempC : null;
    const byTemp = temp != null;
    const heat = byTemp ? temp : m ? Number(m.cpuPct) || 0 : 0;
    const color = colorAt(heat);
    // 验收写的是「温度达到 ≥84°C」才出现过热表现；温度不可用时不冒汗（只按占用选色）
    const hot = byTemp && temp >= HOT_TIER;

    glow.style.setProperty("--cat-glow-color", rgba(color, 0.5));
    heatEl.classList.toggle("is-hot", hot);
    host.classList.toggle("is-hot", hot);
    metrics.update(m, { color });
    if (byTemp) {
      host.title = `CPU 温度 ${temp.toFixed(0)}°C · 双击打开主窗口`;
    } else {
      host.title = "CPU 温度不可用，形象暂按 CPU 占用切换 · 双击打开主窗口";
    }
  }

  /* ------------------------------ 生命周期 ------------------------------ */

  const handleResize = () => engine?.resize();
  window.addEventListener("resize", handleResize);

  // 窗内光标：不依赖 Rust 的 pet-cursor 轮询（非 Windows 没有那段线程），
  // 直接按 canvas 局部坐标算 overCat，保证三平台悬停摸头都成立。
  function handlePointerMove(e) {
    if (!engine) return;
    const canvas = is3d ? canvas3d : canvas2d;
    const rect = canvas.getBoundingClientRect();
    const cx = (e.clientX - rect.left) / Math.max(1, rect.width);
    const cy = (e.clientY - rect.top) / Math.max(1, rect.height);
    const is108 = (config.layout || "65") === "108";
    const overCat = overInteractive(cx, cy, is108).overCat;
    host.classList.toggle("cat-hover", overCat);
    engine.setHover?.(overCat);
    engine.onWindowCursor(cx, cy, overCat);
  }

  function handlePointerLeave() {
    host.classList.remove("cat-hover");
    engine?.setHover?.(false);
    engine?.onWindowCursor(0.5, 0.5, false);
  }

  stage.addEventListener("mousemove", handlePointerMove);
  stage.addEventListener("mouseleave", handlePointerLeave);

  metrics.build(config.metrics);
  await mountEngine();
  await wireEvents();

  // 回归钩子（无 UI 依赖）：mock 检查脚本用它注入合成键鼠事件，验证前端映射链路。
  // 真实输入来自 Rust 的 `pet-input`，与这里走的是同一个 onInput/onCursor。
  window.__pdPetTest = {
    input: onInput,
    cursor: onCursor,
    setPrivacy,
    engine: () => (is3d ? "3d" : "2d"),
    state: () => engine?.debugState?.() || null,
  };

  // 开发预览：`pet.html?mock=1&private=1` 直接进入隐私模式（真实环境由 Rust 事件触发）
  try {
    if (new URLSearchParams(location.search).get("private") === "1") setPrivacy(true);
  } catch {
    /* ignore */
  }

  return {
    setConfig(next) {
      const prev = config;
      config = next;
      metrics.build(next.metrics);
      if (!engine) return;
      const skinChanged = prev.skin !== next.skin;
      const layoutChanged = prev.layout !== next.layout;
      const orientationChanged = prev.orientation !== next.orientation;
      if (skinChanged) {
        const skin = findSkin(next.skin);
        if (is3d) engine.setCatSkin(skin);
        else engine.setCatAppearance(skin.id, skin);
      }
      if (layoutChanged || orientationChanged) {
        engine.setKeyboardConfig(next.layout || "65", next.orientation || "cat");
      }
      if (is3d) {
        engine.setCollar(next.collar || "none");
        engine.setHeadphones(next.headphones === "on");
      }
      catAudio.setEnabled(Boolean(next.sound));
      engine.resize();
      publishInfo();
    },

    update,
    setPrivacy,

    destroy() {
      destroyed = true;
      window.removeEventListener("resize", handleResize);
      stage.removeEventListener("mousemove", handlePointerMove);
      stage.removeEventListener("mouseleave", handlePointerLeave);
      for (const un of unlisteners.splice(0)) {
        try {
          un();
        } catch {
          /* ignore */
        }
      }
      catAudio.setPurrIntensity(0);
      catAudio.setEnabled(false);
      engine?.stop?.();
      engine?.dispose?.();
      engine = null;
      host.classList.remove("pet--cat", "is-hot", "is-private", "cat-hover");
    },
  };
}
