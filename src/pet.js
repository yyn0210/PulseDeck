/**
 * 桌面宠物窗口的前端入口。
 *
 * 形象分派：
 *   - `mode=classic` → `src/pet/classic.js`（原有 6 档 × 8 帧贴图）
 *   - `mode=cat`     → `src/pet/cat.js`（打字猫：2D 手绘 / 3D 手办 / 鲸鱼娘）
 *
 * 共用的只有四件事：读配置、每秒拉一次 `get_pet_metrics`、心跳、双击呼出主窗口。
 * 指标读数只在 Rust 后台线程采样，这里不重复采样。
 */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import "./pet.css";
import { createClassicPet } from "./pet/classic.js";
import { createCatPet } from "./pet/cat.js";

const root = document.getElementById("pet");

let config = null;
let renderer = null;
let rendererKey = "";
let mountToken = 0;

function keyOf(cfg) {
  if (!cfg) return "";
  if (cfg.mode === "classic") return "classic";
  return `cat|${cfg.renderMode || "2d"}|${cfg.skin || "black"}`;
}

function applyChrome(cfg) {
  if (!cfg) return;
  document.documentElement.style.setProperty("--pet-opacity", String(cfg.opacity ?? 1));
}

async function mount(cfg) {
  const token = ++mountToken;
  if (renderer?.destroy) {
    try {
      renderer.destroy();
    } catch (e) {
      console.error("卸载宠物渲染器失败:", e);
    }
  }
  renderer = null;
  root.innerHTML = "";
  root.classList.remove("pet--cat");

  let next = null;
  try {
    next = cfg.mode === "classic" ? createClassicPet(root) : await createCatPet(root, cfg);
  } catch (e) {
    // 打字猫渲染器整体起不来（极少见：canvas/WebGL 初始化失败）时退回经典贴图，
    // 至少保证宠物窗口不是一片空白——透明窗口里「什么都没有」无法排查。
    console.error("打字猫渲染器创建失败，退回经典贴图:", e);
    root.innerHTML = "";
    root.classList.remove("pet--cat");
    next = createClassicPet(root);
  }
  if (token !== mountToken) {
    // 挂载期间配置又变了：丢弃这次结果，避免旧引擎抢回画面
    next?.destroy?.();
    return;
  }
  renderer = next;
  rendererKey = keyOf(cfg);
  renderer.setConfig?.(cfg);
  applyChrome(cfg);
}

async function applyConfig(next) {
  if (!next || typeof next !== "object") return;
  config = next;
  const bare = { ...next };
  delete bare.position; // 位置归拖动所有，前端只读不用

  if (keyOf(bare) !== rendererKey) {
    await mount(bare);
  } else {
    renderer?.setConfig?.(bare);
    applyChrome(bare);
  }
}

async function loadConfig() {
  try {
    const next = await invoke("get_pet_config");
    if (next && typeof next === "object") await applyConfig(next);
  } catch (e) {
    console.error("读取桌面宠物配置失败:", e);
  }
}

async function tick() {
  try {
    const m = await invoke("get_pet_metrics");
    renderer?.update?.(m, config);
    // 心跳：设置页据此显示「运行中」，也是排查「窗口在但没画出来」的唯一凭据
    invoke("pet_heartbeat").catch(() => {});
  } catch (e) {
    renderer?.update?.(null, config);
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
document.addEventListener(
  "mousedown",
  (e) => {
    if (e.button !== 0 || e.detail !== 2) return;
    e.stopImmediatePropagation();
    e.preventDefault();
    invoke("show_main_window").catch((err) => console.error("打开主窗口失败：", err));
  },
  { capture: true },
);

// 重新可见时立刻补一拍（被别的窗口盖住时 Chromium 可能节流定时器）
document.addEventListener("visibilitychange", () => {
  if (!document.hidden) tick();
});

(async function start() {
  await loadConfig();
  // 设置页保存后由后端 `set_pet_config` 立即广播，避免等 3s 轮询才换形象/键位
  try {
    await listen("pet-config", (ev) => {
      applyConfig(ev.payload).catch((e) => console.error("应用宠物配置失败:", e));
    });
  } catch {
    /* 浏览器 mock 预览没有 Tauri 事件系统 */
  }
  await tick();
  setInterval(tick, 1000);
  // 兜底轮询：事件丢失或旧后端也能在 3s 内收敛
  setInterval(loadConfig, 3000);
})();
