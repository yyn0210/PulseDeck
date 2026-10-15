// 电池小组件：环形电量 + 剩余容量 / 续航预估
import { fmtDuration } from "../modules/format.js";
import { binder } from "./helpers.js";
import { ringSetter } from "./ring.js";

const TEMPLATE = `
<header class="card-head">
  <h2>电池</h2>
  <span class="tag" id="bat-status">--</span>
</header>
<div class="ring-wrap">
  <div class="ring">
    <svg viewBox="0 0 120 120">
      <circle class="ring-bg" cx="60" cy="60" r="52"></circle>
      <circle class="ring-fg" id="bat-ring" cx="60" cy="60" r="52"></circle>
    </svg>
    <div class="ring-center">
      <span class="ring-value" id="bat-value">--</span>
      <span class="ring-label" id="bat-label">暂无电池</span>
    </div>
  </div>
  <ul class="kv">
    <li><span>剩余 / 满充</span><b id="bat-energy">--</b></li>
    <li><span>剩余续航</span><b id="bat-remain">--</b></li>
    <li><span>充满需要</span><b id="bat-tofull">--</b></li>
  </ul>
</div>`;

export default {
  type: "bat",
  title: "电池",
  accent: "bat",
  minW: 3,
  minH: 4,
  maxW: 6,
  maxH: 12,
  defW: 3,
  defH: 4,
  mount(root) {
    root.innerHTML = TEMPLATE;
    const setRing = ringSetter(root, "bat-ring");
    const set = binder(root);
    return {
      update(snap) {
        const bat = snap.battery;
        const percent = bat.percent ?? 0;
        if (!bat.hasBattery) {
          setRing(0);
          set("#bat-value", "--");
          set("#bat-label", "无电池");
          set("#bat-energy", "--");
          set("#bat-remain", "--");
          set("#bat-tofull", "--");
          set("#bat-status", bat.acOnline ? "交流供电" : "未检测到");
          return;
        }
        setRing(percent);
        set("#bat-value", percent.toFixed(0));
        set("#bat-label", bat.charging ? "充电中" : bat.discharging ? "放电中" : "待机");
        set(
          "#bat-energy",
          bat.remainingWh != null
            ? `${bat.remainingWh.toFixed(1)} / ${(bat.fullChargeWh ?? 0).toFixed(1)} Wh`
            : "--",
        );
        set("#bat-remain", fmtDuration(bat.secondsToEmpty));
        set("#bat-tofull", fmtDuration(bat.secondsToFull));
        set("#bat-status", bat.statusText || "--");
      },
    };
  },
};
