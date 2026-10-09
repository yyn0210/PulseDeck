import { invoke } from "@tauri-apps/api/core";
import {
  esc,
  fmtBytes,
  fmtClock,
  fmtDuration,
  fmtFreq,
  fmtRate,
  fmtUptime,
  fmtWatts,
  setText,
} from "./modules/format.js";
import { LineChart } from "./modules/charts.js";

/* ---------------- 常量 / 状态 ---------------- */

const RING_RADIUS = 52;
const CIRCUMFERENCE = 2 * Math.PI * RING_RADIUS;
const HISTORY_MAX = 240;

const state = {
  paused: false,
  intervalMs: 1000,
  timer: null,
  coreCells: [],
  coreCount: 0,
  failures: 0,
  latest: null,
  // 是否把无流量的网卡也列出来
  showIdleIface: false,
  // 采样拍数：用来给「低频刷新」的调用（如 PawnIO 状态）计数
  ticks: 0,
};

const banner = document.getElementById("banner");

/* ---------------- 图表 ---------------- */

const chart = new LineChart(document.getElementById("trend-chart"), {
  max: HISTORY_MAX,
  ceiling: 100,
  series: [
    { key: "cpu", color: "#4f8cff", fill: true },
    { key: "mem", color: "#22d3a5" },
  ],
});

const netChart = new LineChart(document.getElementById("net-chart"), {
  max: HISTORY_MAX,
  autoCeiling: true,
  // 至少保留 256 KB/s 的量程，空闲时不至于把零星毛刺放大成锯齿
  minCeiling: 256 * 1024,
  series: [
    { key: "rx", color: "#3ddc84", fill: true },
    { key: "tx", color: "#ffd166" },
  ],
});

/* ---------------- 主循环 ---------------- */

async function tick() {
  if (state.paused) return;
  try {
    const snap = await invoke("get_snapshot");
    state.latest = snap;
    state.failures = 0;
    banner.classList.add("hidden");
    // 低频刷新 PawnIO 状态（每 10 拍一次）：用户可能在外部装上或卸掉驱动
    if (state.ticks++ % 10 === 0) await refreshPawnioStatus();
    render(snap);
  } catch (err) {
    state.failures += 1;
    if (state.failures >= 2) {
      banner.textContent = `采集失败：${err}（请确认以桌面会话运行，而非服务/无窗口环境）`;
      banner.classList.remove("hidden");
    }
    console.error("get_snapshot failed:", err);
  }
}

function schedule() {
  if (state.timer) clearInterval(state.timer);
  state.timer = setInterval(tick, state.intervalMs);
}

/* ---------------- 渲染 ---------------- */

function render(s) {
  renderHeader(s);
  renderCpu(s.cpu);
  renderMemory(s.memory);
  renderGpu(s.gpu, s.thermal);
  renderBattery(s.battery, s.thermal);
  renderNet(s.net);
  renderThermal(s.thermal);
  renderDisks(s.disks);
  renderProcs(s.cpu);

  chart.push({ cpu: s.cpu.usageTotal, mem: s.memory.usagePct });
  chart.draw();
  netChart.push({ rx: s.net.rxBytesPerSec, tx: s.net.txBytesPerSec });
  netChart.draw();
}

function renderHeader(s) {
  const parts = [s.system.hostName, s.system.osName, s.system.osVersion, s.system.cpuArch].filter(
    Boolean,
  );
  setText("machine-line", parts.join(" · "));
  setText("uptime", fmtUptime(s.system.uptimeSecs));
  setText("proc-count", s.system.processCount.toLocaleString("zh-CN"));
  setText("sample-interval", `${s.sampleIntervalMs} ms`);
  setText("status-left", `已更新 ${fmtClock(s.tsMs)}`);
}

function setRing(id, percent) {
  const el = document.getElementById(id);
  if (!el) return;
  const p = Math.max(0, Math.min(100, percent || 0));
  el.style.strokeDashoffset = String(CIRCUMFERENCE * (1 - p / 100));
}

function renderCpu(cpu) {
  setRing("cpu-ring", cpu.usageTotal);
  setText("cpu-value", cpu.usageTotal.toFixed(1));
  setText("cpu-brand", cpu.brand);
  setText(
    "cpu-cores",
    `${cpu.physicalCores} 核 / ${cpu.logicalCores} 线程`,
  );
  setText("cpu-freq", fmtFreq(cpu.freqMhz));
  setText("cpu-tag", `峰值 ${Math.max(...cpu.perCore, 0).toFixed(0)}%`);
  renderCores(cpu.perCore);
}

function renderCores(perCore) {
  const grid = document.getElementById("core-grid");
  if (perCore.length !== state.coreCount) {
    state.coreCount = perCore.length;
    state.coreCells = [];
    grid.innerHTML = perCore
      .map((_, i) => {
        const name = perCore.length > 64 ? `${i}` : `CPU${i}`;
        return `<div class="core"><span class="core-name">${esc(name)}</span>
          <div class="core-bar"><div class="core-fill"></div></div>
          <span class="core-val">0%</span></div>`;
      })
      .join("");
    state.coreCells = Array.from(grid.querySelectorAll(".core")).map((el) => ({
      fill: el.querySelector(".core-fill"),
      val: el.querySelector(".core-val"),
    }));
  }
  perCore.forEach((v, i) => {
    const cell = state.coreCells[i];
    if (!cell) return;
    const p = Math.max(0, Math.min(100, v));
    cell.fill.style.width = `${p}%`;
    cell.fill.style.background = p >= 85 ? "#ff5f6d" : p >= 60 ? "#f5c451" : "#4f8cff";
    cell.val.textContent = `${v.toFixed(0)}%`;
  });
}

function renderMemory(mem) {
  setRing("mem-ring", mem.usagePct);
  setText("mem-value", mem.usagePct.toFixed(1));
  setText("mem-used", `${fmtBytes(mem.usedBytes)} / ${fmtBytes(mem.totalBytes)}`);
  setText("mem-avail", fmtBytes(mem.availableBytes));
  setText(
    "mem-commit",
    mem.commitLimitBytes
      ? `${fmtBytes(mem.commitUsedBytes)} / ${fmtBytes(mem.commitLimitBytes)}`
      : "--",
  );
  const swap = mem.swapTotalBytes
    ? ` · 页面文件 ${fmtBytes(mem.swapUsedBytes)}/${fmtBytes(mem.swapTotalBytes)}`
    : "";
  setText("mem-tag", `${fmtBytes(mem.totalBytes)}${swap}`);
}

/* ---------------- GPU ---------------- */

function renderGpu(gpu, thermal) {
  const usage = gpu.usagePct;
  if (!gpu.available) {
    setRing("gpu-ring", 0);
    setText("gpu-value", "--");
    setText("gpu-label", "未检测到");
    setText("gpu-name", "--");
    setText("gpu-mem", "--");
    setText("gpu-temp", "--");
    setText("gpu-power", "--");
    setText("gpu-tag", "无可用数据源");
    return;
  }

  setRing("gpu-ring", usage ?? 0);
  setText("gpu-value", usage != null ? usage.toFixed(0) : "--");
  setText("gpu-label", usage != null ? "显卡占用" : "占用未知");

  const main = gpu.devices.find((d) => d.name) || gpu.devices[0];
  setText("gpu-name", main ? main.name : "--");
  setText(
    "gpu-mem",
    gpu.memTotalBytes
      ? `${fmtBytes(gpu.memUsedBytes)} / ${fmtBytes(gpu.memTotalBytes)}`
      : gpu.memUsedBytes
        ? fmtBytes(gpu.memUsedBytes)
        : "--",
  );
  setText("gpu-temp", gpu.tempC != null ? `${gpu.tempC.toFixed(0)} °C` : "--");
  setText("gpu-power", fmtWatts(gpu.powerW));

  const sourceLabel = { nvml: "NVIDIA 驱动", pdh: "Windows 计数器", "pdh-warming": "正在初始化" };
  const multi = gpu.devices.length > 1 ? ` · ${gpu.devices.length} 块显卡` : "";
  setText("gpu-tag", `${sourceLabel[gpu.source] || gpu.source}${multi}`);
}

/* ---------------- 网络 ---------------- */

function renderNet(net) {
  setText("net-rx", fmtBytes(net.rxBytesPerSec));
  setText("net-tx", fmtBytes(net.txBytesPerSec));
  setText(
    "net-total",
    `↓ ${fmtBytes(net.totalReceived)} · ↑ ${fmtBytes(net.totalTransmitted)}`,
  );

  const list = net.interfaces.filter(
    (i) => state.showIdleIface || i.isVirtual === false,
  );
  const box = document.getElementById("iface-list");
  box.innerHTML = list.length
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
}

/* ---------------- 温度 ---------------- */

/* PawnIO 是读 CPU 核心温度必需的内核驱动（需用户授权安装），
 * 未安装时后端不会自动提权，而是在这里引导用户点一下按钮。 */
let pawnio = null;
let pawnioBusy = false;

async function refreshPawnioStatus() {
  try {
    pawnio = await invoke("pawnio_status");
  } catch (e) {
    // 拿不到状态就不显示安装入口，避免误报
    pawnio = null;
  }
}

function renderThermal(thermal) {
  setText("cpu-temp", thermal.cpuTempC != null ? `${thermal.cpuTempC.toFixed(0)} °C` : "--");
  // GPU 温度由 GPU 卡片负责展示，这里只在不可用时补一次提示
  const el = document.getElementById("thermal-hint");
  if (thermal.available || thermal.cpuTempC != null) {
    el.classList.add("hidden");
    return;
  }
  el.classList.remove("hidden");

  const msg = thermal.hint || "未检测到温度传感器";
  // 只有「确实没装 PawnIO」且「随包安装器在」才给安装入口：
  // 已经装上却读不到温度属于另一类问题（多半是用户取消了 UAC），按钮没有意义。
  const canInstall = !!pawnio && !pawnio.installed && pawnio.installerAvailable;
  if (!canInstall) {
    el.textContent = msg;
    return;
  }

  el.innerHTML =
    `<span class="hint-text">${esc(msg)}</span>` +
    `<button class="hint-btn" id="pawnio-install"${pawnioBusy ? " disabled" : ""}>` +
    `${pawnioBusy ? "安装中…" : "安装 PawnIO"}</button>`;
  document.getElementById("pawnio-install").addEventListener("click", installPawnIo);
}

/** 请求后端安装 PawnIO。
 * 真正的安装在提权子进程里跑（用户会在 UAC 上点一次「是」），这里轮询状态直到装上。 */
async function installPawnIo() {
  if (pawnioBusy) return;
  pawnioBusy = true;
  if (state.latest) renderThermal(state.latest.thermal);
  try {
    await invoke("install_pawnio");
  } catch (e) {
    console.error("请求安装 PawnIO 失败:", e);
  }
  const deadline = Date.now() + 180000;
  while (Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, 2000));
    await refreshPawnioStatus();
    if (pawnio && pawnio.installed) break;
  }
  pawnioBusy = false;
  if (state.latest) renderThermal(state.latest.thermal);
}

function renderBattery(bat, thermal) {
  const percent = bat.percent ?? 0;
  if (!bat.hasBattery) {
    setRing("bat-ring", 0);
    setText("bat-value", "--");
    setText("bat-label", "无电池");
    setText("bat-energy", "--");
    setText("bat-remain", "--");
    setText("bat-tofull", "--");
    setText("bat-status", bat.acOnline ? "交流供电" : "未检测到");
    setRingValue0();
  } else {
    setRing("bat-ring", percent);
    setText("bat-value", percent.toFixed(0));
    setText("bat-label", bat.charging ? "充电中" : bat.discharging ? "放电中" : "待机");
    setText(
      "bat-energy",
      bat.remainingWh != null
        ? `${bat.remainingWh.toFixed(1)} / ${(bat.fullChargeWh ?? 0).toFixed(1)} Wh`
        : "--",
    );
    setText("bat-remain", fmtDuration(bat.secondsToEmpty));
    setText("bat-tofull", fmtDuration(bat.secondsToFull));
    setText("bat-status", bat.statusText || "--");
  }

  // 电源功率：优先「充入电池」的功率，其次「电池输出」的功率。
  // 适配器在供电但电池既不充也不放（多数笔记本满电后的常态）时明确给 0，
  // 避免整块卡片只有一个 “--”；CPU 封装功耗作为独立的实时功耗参考放在副行。
  const liveW = bat.chargeRateW ?? bat.dischargeRateW ?? (bat.acOnline ? 0 : null);
  setText("power-value", liveW != null ? liveW.toFixed(1) : "--");
  setText("cpu-power-value", fmtWatts(thermal?.cpuPowerW));
  setText("bat-power-state", batteryStateLabel(bat));
  setText("ac-status", bat.acOnline ? "已接入适配器" : "电池供电");
  setText("power-source", bat.source || "--");
  setText("power-hint", powerHint(bat, thermal));
}

// 电池侧状态：任何情况都能给出文案，不会留空
function batteryStateLabel(bat) {
  if (!bat.hasBattery) return "无电池";
  if (bat.charging) return "充电中";
  if (bat.discharging) return bat.acOnline ? "微放电" : "放电中";
  return bat.acOnline ? "未充电" : "待机";
}

// 电源卡片说明：覆盖「充电 / 已接入未充电 / 电池供电」全部状态
function powerHint(bat, thermal) {
  const cpu =
    thermal?.cpuPowerW != null ? `；CPU 封装功耗 ${thermal.cpuPowerW.toFixed(1)} W` : "";
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

function setRingValue0() {
  const el = document.getElementById("bat-ring");
  if (el) el.style.strokeDashoffset = String(CIRCUMFERENCE);
}

function renderDisks(disks) {
  const list = document.getElementById("disk-list");
  if (!disks.length) {
    list.innerHTML = `<div class="disk">未检测到任何卷</div>`;
    setText("disk-tag", "0 个卷");
    return;
  }
  list.innerHTML = disks
    .map(
      (d) => `
      <div class="disk">
        <div class="disk-head">
          <span class="disk-title">${esc(d.mountPoint)} ${esc(d.name)}</span>
          <span class="disk-kind">${esc(d.fileSystem)} · ${esc(d.diskKind)}${
            d.removable ? " · 可移动" : ""
          }</span>
        </div>
        <div class="disk-bar"><div class="disk-fill" style="width:${Math.min(
          100,
          d.usagePct,
        )}%"></div></div>
        <div class="disk-meta">
          <span>已用 <b>${fmtBytes(d.usedBytes)}</b></span>
          <span>可用 <b>${fmtBytes(d.freeBytes)}</b></span>
          <span>共 <b>${fmtBytes(d.totalBytes)}</b></span>
          <span class="io-read">读 <b>${fmtRate(d.readBytesPerSec)}</b></span>
          <span class="io-write">写 <b>${fmtRate(d.writeBytesPerSec)}</b></span>
          <span>累计 <b>${fmtBytes(d.totalReadBytes)} / ${fmtBytes(d.totalWrittenBytes)}</b></span>
        </div>
      </div>`,
    )
    .join("");
  setText("disk-tag", `${disks.length} 个卷`);
}

function procRows(rows, highlight) {
  if (!rows.length) return `<tr><td colspan="5">暂无数据</td></tr>`;
  return rows
    .map(
      (p, i) => `
      <tr>
        <td>${i + 1}</td>
        <td class="pname">${esc(p.name)}</td>
        <td>${p.pid}</td>
        <td class="${highlight === "cpuUsage" ? "lead" : ""}">${p.cpuUsage.toFixed(1)}%</td>
        <td class="${highlight === "memoryBytes" ? "lead" : ""}">${fmtBytes(p.memoryBytes)}</td>
      </tr>`,
    )
    .join("");
}

function renderProcs(cpu) {
  document.getElementById("top-cpu-body").innerHTML = procRows(cpu.topCpu, "cpuUsage");
  document.getElementById("top-mem-body").innerHTML = procRows(cpu.topMem, "memoryBytes");
  setText("proc-tag", `Top ${Math.max(cpu.topCpu.length, cpu.topMem.length)} · 取占用最高者`);
}

/* ---------------- 交互 ---------------- */

const pauseBtn = document.getElementById("pause-btn");
pauseBtn.addEventListener("click", async () => {
  state.paused = !state.paused;
  pauseBtn.textContent = state.paused ? "继续" : "暂停";
  pauseBtn.classList.toggle("active", state.paused);
  try {
    await invoke("set_paused", { paused: state.paused });
  } catch (e) {
    console.error(e);
  }
  if (!state.paused) tick();
});

const intervalSelect = document.getElementById("interval-select");
intervalSelect.addEventListener("change", async () => {
  const ms = Number(intervalSelect.value);
  try {
    await invoke("set_interval", { millis: ms });
  } catch (e) {
    console.error(e);
  }
  state.intervalMs = ms;
  chart.max = ms >= 2000 ? 120 : 240;
  netChart.max = chart.max;
  schedule();
});

/* ---------------- 设置：开机自启 / 托盘 / 网卡显示 ---------------- */

async function initSettings() {
  const autostart = document.getElementById("opt-autostart");
  const tray = document.getElementById("opt-tray");
  const idleIface = document.getElementById("opt-idle-iface");

  // 读取运行期配置与自启状态
  try {
    const cfg = await invoke("get_runtime_config");
    tray.checked = cfg.closeToTray !== false;
    await invoke("set_close_to_tray", { enabled: tray.checked });
  } catch (e) {
    console.error("读取运行配置失败:", e);
  }
  try {
    autostart.checked = await invoke("get_autostart");
  } catch (e) {
    console.error("读取自启状态失败:", e);
  }

  autostart.addEventListener("change", async () => {
    try {
      await invoke("set_autostart", { enabled: autostart.checked });
    } catch (e) {
      autostart.checked = !autostart.checked;
      console.error("设置开机自启失败:", e);
    }
  });

  tray.addEventListener("change", async () => {
    try {
      await invoke("set_close_to_tray", { enabled: tray.checked });
    } catch (e) {
      console.error("设置托盘驻留失败:", e);
    }
  });

  idleIface.addEventListener("change", () => {
    state.showIdleIface = idleIface.checked;
    if (state.latest) renderNet(state.latest.net);
  });
}

/* 启动：设置异步加载，同时先渲染首屏再开启轮询
 * （不用 top-level await，保持构建目标兼容） */
initSettings();
tick().then(schedule);
