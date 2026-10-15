// 内存小组件：环形占用 + 用量 / 可用 / 已提交明细
import { fmtBytes } from "../modules/format.js";
import { binder } from "./helpers.js";
import { ringSetter } from "./ring.js";

const TEMPLATE = `
<header class="card-head">
  <h2>内存</h2>
  <span class="tag" id="mem-tag">--</span>
</header>
<div class="ring-wrap">
  <div class="ring">
    <svg viewBox="0 0 120 120">
      <circle class="ring-bg" cx="60" cy="60" r="52"></circle>
      <circle class="ring-fg" id="mem-ring" cx="60" cy="60" r="52"></circle>
    </svg>
    <div class="ring-center">
      <span class="ring-value" id="mem-value">0<i>%</i></span>
      <span class="ring-label">已使用</span>
    </div>
  </div>
  <ul class="kv">
    <li><span>已用 / 总量</span><b id="mem-used">--</b></li>
    <li><span>可用</span><b id="mem-avail">--</b></li>
    <li><span>已提交</span><b id="mem-commit">--</b></li>
  </ul>
</div>`;

export default {
  type: "mem",
  title: "内存",
  accent: "mem",
  minW: 3,
  minH: 4,
  maxW: 6,
  maxH: 12,
  defW: 3,
  defH: 4,
  mount(root) {
    root.innerHTML = TEMPLATE;
    const setRing = ringSetter(root, "mem-ring");
    const set = binder(root);
    return {
      update(snap) {
        const mem = snap.memory;
        setRing(mem.usagePct);
        set("#mem-value", mem.usagePct.toFixed(1));
        set("#mem-used", `${fmtBytes(mem.usedBytes)} / ${fmtBytes(mem.totalBytes)}`);
        set("#mem-avail", fmtBytes(mem.availableBytes));
        set(
          "#mem-commit",
          mem.commitLimitBytes
            ? `${fmtBytes(mem.commitUsedBytes)} / ${fmtBytes(mem.commitLimitBytes)}`
            : "--",
        );
        const swap = mem.swapTotalBytes
          ? ` · 页面文件 ${fmtBytes(mem.swapUsedBytes)}/${fmtBytes(mem.swapTotalBytes)}`
          : "";
        set("#mem-tag", `${fmtBytes(mem.totalBytes)}${swap}`);
      },
    };
  },
};
