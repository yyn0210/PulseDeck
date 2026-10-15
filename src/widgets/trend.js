// CPU·内存趋势小组件：双序列占用率曲线（canvas 手绘）
import { LineChart } from "../modules/charts.js";

// 图表横轴的默认窗口点数；采样间隔 ≥2s 时缩到一半（与「采样」设置联动，见 setInterval）
const HISTORY_MAX = 240;

const TEMPLATE = `
<header class="card-head">
  <h2>CPU 与内存趋势</h2>
  <div class="legend">
    <span><i class="dot cpu"></i>CPU</span>
    <span><i class="dot mem"></i>内存</span>
  </div>
</header>
<canvas id="trend-chart" class="chart"></canvas>`;

export default {
  type: "trend",
  title: "CPU 与内存趋势",
  minW: 4,
  minH: 3,
  maxW: 12,
  maxH: 12,
  defW: 7,
  defH: 4,
  mount(root) {
    root.innerHTML = TEMPLATE;
    const chart = new LineChart(root.querySelector("#trend-chart"), {
      max: HISTORY_MAX,
      ceiling: 100,
      series: [
        { key: "cpu", color: "#4f8cff", fill: true },
        { key: "mem", color: "#22d3a5" },
      ],
    });
    return {
      update(snap) {
        chart.push({ cpu: snap.cpu.usageTotal, mem: snap.memory.usagePct });
        chart.draw();
      },
      setInterval(ms) {
        chart.max = ms >= 2000 ? 120 : 240;
      },
      destroy() {
        chart.destroy();
      },
    };
  },
};
