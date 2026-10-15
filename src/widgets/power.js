// 电源功率小组件：电池实时功率大数字 + CPU 封装功耗 / 供电状态副行
import { fmtWatts } from "../modules/format.js";
import { binder } from "./helpers.js";

const TEMPLATE = `
<header class="card-head">
  <h2>电源功率</h2>
  <span class="tag" id="power-source">--</span>
</header>
<div class="power-body">
  <div class="power-main">
    <span class="power-value" id="power-value">0.0</span>
    <span class="power-unit">W</span>
  </div>
  <p class="power-hint" id="power-hint">等待数据…</p>
  <div class="power-row">
    <div>
      <span class="power-sub-label">CPU 功耗</span>
      <b id="cpu-power-value">--</b>
    </div>
    <div>
      <span class="power-sub-label">电池</span>
      <b id="bat-power-state">--</b>
    </div>
    <div>
      <span class="power-sub-label">电源</span>
      <b id="ac-status">--</b>
    </div>
  </div>
</div>`;

// 电池侧状态：任何情况都能给出文案，不会留空
function batteryStateLabel(bat) {
  if (!bat.hasBattery) return "无电池";
  if (bat.charging) return "充电中";
  if (bat.discharging) return bat.acOnline ? "微放电" : "放电中";
  return bat.acOnline ? "未充电" : "待机";
}

// 说明文案：覆盖「充电 / 已接入未充电 / 电池供电」全部状态
function powerHint(bat, thermal) {
  const cpu =
    thermal && thermal.cpuPowerW != null ? `；CPU 封装功耗 ${thermal.cpuPowerW.toFixed(1)} W` : "";
  if (!bat.hasBattery) return "本机未检测到电池，无法读取充放电功率";
  if (bat.charging && bat.chargeRateW != null) {
    return `适配器正在以 ${bat.chargeRateW.toFixed(1)} W 向电池充电${cpu}`;
  }
  if (bat.charging) return `正在充电，但固件未上报充入功率${cpu}`;
  if (bat.acOnline) {
    const out =
      bat.dischargeRateW != null ? `，电池轻微放电 ${bat.dischargeRateW.toFixed(1)} W` : "";
    return `适配器已接入，当前未向电池充电${out}${cpu}`;
  }
  if (bat.dischargeRateW != null) {
    return `未接入适配器，电池放电 ${bat.dischargeRateW.toFixed(1)} W${cpu}`;
  }
  return `电池供电中，固件未上报放电功率${cpu}`;
}

export default {
  type: "power",
  title: "电源功率",
  accent: "power",
  minW: 3,
  minH: 4,
  maxW: 6,
  maxH: 12,
  defW: 3,
  defH: 4,
  mount(root) {
    root.innerHTML = TEMPLATE;
    const set = binder(root);
    return {
      update(snap) {
        const { battery: bat, thermal } = snap;
        // 主数值取「电池实时功率」：充电时用充入功率，未接入时用放电功率；
        // 已接入但未充电（多数笔记本满电后的常态）时明确给 0，避免整卡只有 “--”
        const liveW = bat.chargeRateW ?? bat.dischargeRateW ?? (bat.acOnline ? 0 : null);
        set("#power-value", liveW != null ? liveW.toFixed(1) : "--");
        set("#cpu-power-value", fmtWatts(thermal && thermal.cpuPowerW));
        set("#bat-power-state", batteryStateLabel(bat));
        set("#ac-status", bat.acOnline ? "已接入适配器" : "电池供电");
        set("#power-source", bat.source || "--");
        set("#power-hint", powerHint(bat, thermal));
      },
    };
  },
};
