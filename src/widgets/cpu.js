// 处理器小组件：环形总占用 + 型号 / 主频 / 温度 / 风扇明细
import { fmtFan, fmtFreq } from "../modules/format.js";
import { binder, setFanSource } from "./helpers.js";
import { ringSetter } from "./ring.js";

const TEMPLATE = `
<header class="card-head">
  <h2>处理器</h2>
  <span class="tag" id="cpu-tag">--</span>
</header>
<div class="ring-wrap">
  <div class="ring">
    <svg viewBox="0 0 120 120">
      <circle class="ring-bg" cx="60" cy="60" r="52"></circle>
      <circle class="ring-fg" id="cpu-ring" cx="60" cy="60" r="52"></circle>
    </svg>
    <div class="ring-center">
      <span class="ring-value" id="cpu-value">0<i>%</i></span>
      <span class="ring-label">总占用</span>
    </div>
  </div>
  <ul class="kv">
    <li><span>型号</span><b id="cpu-brand">--</b></li>
    <li><span>核心 / 线程</span><b id="cpu-cores">--</b></li>
    <li><span id="cpu-freq-label">当前主频</span><b id="cpu-freq">--</b></li>
    <li><span>温度</span><b id="cpu-temp">--</b></li>
    <li><span>风扇</span><b id="cpu-fan">--</b></li>
  </ul>
</div>`;

export default {
  type: "cpu",
  title: "处理器",
  accent: "cpu",
  minW: 3,
  minH: 4,
  maxW: 6,
  maxH: 12,
  defW: 3,
  defH: 4,
  mount(root) {
    root.innerHTML = TEMPLATE;
    const setRing = ringSetter(root, "cpu-ring");
    const set = binder(root);
    return {
      update(snap) {
        const { cpu, thermal } = snap;
        setRing(cpu.usageTotal);
        set("#cpu-value", cpu.usageTotal.toFixed(1));
        set("#cpu-brand", cpu.brand);
        set("#cpu-cores", `${cpu.physicalCores} 核 / ${cpu.logicalCores} 线程`);
        set("#cpu-freq", fmtFreq(cpu.freqMhz));
        // 标签跟着数据口径走：能读到实时主频才敢写「当前」，否则如实写「基准主频」
        set("#cpu-freq-label", cpu.freqLive ? "当前主频" : "基准主频");
        set("#cpu-fan", fmtFan(cpu.fanRpm, cpu.fanPct));
        setFanSource(root, "#cpu-fan", cpu.fanSource);
        set("#cpu-tag", `峰值 ${Math.max(...cpu.perCore, 0).toFixed(0)}%`);
        // 温度行：来源与后端 thermal 通道（PawnIO / ACPI / PDH 多级降级）
        set(
          "#cpu-temp",
          thermal && thermal.cpuTempC != null ? `${thermal.cpuTempC.toFixed(0)} °C` : "--",
        );
      },
    };
  },
};
