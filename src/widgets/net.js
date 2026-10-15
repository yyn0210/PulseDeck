// 网络小组件：上下行速率 + 速率曲线（canvas 手绘）+ 网卡明细列表
import { LineChart } from "../modules/charts.js";
import { esc, fmtBytes, fmtRate } from "../modules/format.js";
import { binder } from "./helpers.js";

// 图表横轴的默认窗口点数；采样间隔 ≥2s 时缩到一半（与「采样」设置联动，见 setInterval）
const HISTORY_MAX = 240;

const TEMPLATE = `
<header class="card-head">
  <h2>网络</h2>
  <div class="legend">
    <span><i class="dot rx"></i>下行</span>
    <span><i class="dot tx"></i>上行</span>
  </div>
</header>
<div class="net-head">
  <div class="net-rate">
    <span class="net-arrow down">↓</span>
    <span class="net-value" id="net-rx">--</span>
    <span class="net-unit">/s</span>
  </div>
  <div class="net-rate">
    <span class="net-arrow up">↑</span>
    <span class="net-value" id="net-tx">--</span>
    <span class="net-unit">/s</span>
  </div>
  <div class="net-total">
    <span class="net-total-label">累计流量</span>
    <b id="net-total">--</b>
  </div>
</div>
<canvas id="net-chart" class="chart net-chart"></canvas>
<div class="iface-list" id="iface-list"></div>`;

export default {
  type: "net",
  title: "网络",
  accent: "net",
  minW: 5,
  minH: 4,
  maxW: 12,
  maxH: 12,
  defW: 9,
  defH: 5,
  mount(root) {
    root.innerHTML = TEMPLATE;
    const set = binder(root);
    const ifaceList = root.querySelector("#iface-list");
    const chart = new LineChart(root.querySelector("#net-chart"), {
      max: HISTORY_MAX,
      autoCeiling: true,
      // 至少保留 256 KB/s 的量程，空闲时不至于把零星毛刺放大成锯齿
      minCeiling: 256 * 1024,
      series: [
        { key: "rx", color: "#3ddc84", fill: true },
        { key: "tx", color: "#ffd166" },
      ],
    });
    return {
      update(snap, ctx) {
        const net = snap.net;
        set("#net-rx", fmtBytes(net.rxBytesPerSec));
        set("#net-tx", fmtBytes(net.txBytesPerSec));
        set("#net-total", `↓ ${fmtBytes(net.totalReceived)} · ↑ ${fmtBytes(net.totalTransmitted)}`);

        // 「列出无流量网卡」是设置页开关，经 ctx 传进来
        const showIdle = !!(ctx && ctx.showIdleIface);
        const list = net.interfaces.filter((i) => showIdle || i.isVirtual === false);
        ifaceList.innerHTML = list.length
          ? list
              .map(
                (i) => `
      <div class="iface ${i.rxBytesPerSec + i.txBytesPerSec > 0 ? "active" : ""}">
        <span class="iface-name">
          <span title="${esc(i.name)}">${esc(i.name)}</span>
          ${i.isVirtual ? '<i class="iface-vtag">虚拟</i>' : ""}
        </span>
        <span class="iface-rate">
          <b class="rx">↓${fmtRate(i.rxBytesPerSec)}</b>
          <b class="tx">↑${fmtRate(i.txBytesPerSec)}</b>
        </span>
        <span class="iface-total">↓${fmtBytes(i.totalReceived)} ↑${fmtBytes(i.totalTransmitted)}</span>
      </div>`,
              )
              .join("")
          : `<div class="iface"><span class="iface-name">未检测到网卡</span></div>`;

        chart.push({ rx: net.rxBytesPerSec, tx: net.txBytesPerSec });
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
