// 每核负载小组件：逻辑核占用的网格条
import { esc } from "../modules/format.js";

const TEMPLATE = `
<header class="card-head">
  <h2>每核负载</h2>
</header>
<div class="core-grid" id="core-grid"></div>`;

export default {
  type: "cores",
  title: "每核负载",
  accent: "cpu",
  minW: 4,
  minH: 3,
  maxW: 12,
  maxH: 12,
  defW: 7,
  defH: 4,
  mount(root) {
    root.innerHTML = TEMPLATE;
    const grid = root.querySelector("#core-grid");
    // 核数变化时才重建 DOM，平时只更新缓存好的引用（与旧版 renderCores 同一套优化）
    let coreCells = [];
    let coreCount = 0;
    return {
      update(snap) {
        const perCore = snap.cpu.perCore;
        if (perCore.length !== coreCount) {
          coreCount = perCore.length;
          grid.innerHTML = perCore
            .map((_, i) => {
              const name = perCore.length > 64 ? `${i}` : `CPU${i}`;
              return `<div class="core"><span class="core-name">${esc(name)}</span>
          <div class="core-bar"><div class="core-fill"></div></div>
          <span class="core-val">0%</span></div>`;
            })
            .join("");
          coreCells = Array.from(grid.querySelectorAll(".core")).map((el) => ({
            fill: el.querySelector(".core-fill"),
            val: el.querySelector(".core-val"),
          }));
        }
        perCore.forEach((v, i) => {
          const cell = coreCells[i];
          if (!cell) return;
          const p = Math.max(0, Math.min(100, v));
          // 长度走 transform 而不是 width（改 width 每次都要重排），详见 style.css 的 .core-fill
          cell.fill.style.transform = `scaleX(${p / 100})`;
          cell.fill.style.background = p >= 85 ? "#ff5f6d" : p >= 60 ? "#f5c451" : "#4f8cff";
          cell.val.textContent = `${v.toFixed(0)}%`;
        });
      },
    };
  },
};
