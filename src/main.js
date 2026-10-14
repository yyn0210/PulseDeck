import { invoke } from "@tauri-apps/api/core";
import {
  esc,
  fmtBytes,
  fmtClock,
  fmtDuration,
  fmtFan,
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

/* 当前平台是否为 Windows：从快照的 osName 判断（Windows 上 sysinfo 报 "Windows"，
 * Linux 报发行版名）。Windows 专属功能（PawnIO 高级采集、联想风扇号）整套 UI 与
 * 命令调用都由它门控——非 Windows 上那几个命令根本不存在，invoke 会直接抛错。
 * 未知（osName 缺失）时保守按 Windows 处理：宁可显示一个读不到状态的区块，
 * 也不要在一个能用的平台上把功能藏起来。 */
let isWindows = true;
let platformApplied = false;

function applyPlatform(snap) {
  const os = snap.system && snap.system.osName;
  if (os) isWindows = /windows/i.test(os);
  if (platformApplied) return;
  platformApplied = true;

  const advCard = document.getElementById("advanced-card");
  if (advCard) advCard.classList.toggle("hidden", !isWindows);
  const src = document.getElementById("status-right");
  if (src && !isWindows) {
    src.textContent = "数据来源：sysinfo + 平台原生接口（Linux: sysfs / procfs）";
  }
}

async function tick() {
  if (state.paused) return;
  try {
    const snap = await invoke("get_snapshot");
    state.latest = snap;
    state.failures = 0;
    banner.classList.add("hidden");
    applyPlatform(snap);
    // 低频刷新 PawnIO / 高级采集状态（每 10 拍一次）：
    // 用户可能在外部装上或卸掉驱动，提权桥接也可能刚刚起来或退出
    if (state.ticks++ % 10 === 0) {
      await refreshPawnioStatus();
      await refreshHardwareStatus();
    }
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
  renderThermal(s.thermal, s.cpu, s.gpu);
  renderDisks(s.disks);
  renderProcs(s.cpu);

  chart.push({ cpu: s.cpu.usageTotal, mem: s.memory.usagePct });
  chart.draw();
  netChart.push({ rx: s.net.rxBytesPerSec, tx: s.net.txBytesPerSec });
  netChart.draw();

  // 设置页可见时才刷新高级采集状态，避免在监控页做无谓 DOM 写入
  if (!viewSettings.classList.contains("hidden")) {
    renderAdvanced();
    refreshPetStatus();
    refreshTaskbarStatus();
  } else {
    renderPawnioTip();
  }
}

function renderHeader(s) {
  const parts = [s.system.hostName, s.system.osName, s.system.osVersion, s.system.cpuArch].filter(
    Boolean,
  );
  setText("machine-line", parts.join(" · "));
  setText("uptime", fmtUptime(s.system.uptimeSecs));
  setText("proc-count", s.system.processCount.toLocaleString("zh-CN"));
  setText("sample-interval", `${s.sampleIntervalMs} ms`);
  setText("sample-tag", `${s.sampleIntervalMs} ms`);
  setText("status-left", `已更新 ${fmtClock(s.tsMs)}`);
}

function setRing(id, percent) {
  const el = document.getElementById(id);
  if (!el) return;
  const p = Math.max(0, Math.min(100, percent || 0));
  el.style.strokeDashoffset = String(CIRCUMFERENCE * (1 - p / 100));
}

/** 给风扇数值挂一个来源提示（鼠标悬停可见）。
 * 来源字符串来自后端 `FanReading::source`，例如「联想 EC（上限 4500 RPM）」。
 * 空字符串或缺失时清掉 title，避免出现空白浮层。 */
function setFanSource(id, source) {
  const el = document.getElementById(id);
  if (!el) return;
  const text = typeof source === "string" ? source.trim() : "";
  if (text) {
    el.title = `来源：${text}`;
  } else {
    el.removeAttribute("title");
  }
}

function renderCpu(cpu) {  setRing("cpu-ring", cpu.usageTotal);
  setText("cpu-value", cpu.usageTotal.toFixed(1));
  setText("cpu-brand", cpu.brand);
  setText(
    "cpu-cores",
    `${cpu.physicalCores} 核 / ${cpu.logicalCores} 线程`,
  );
  setText("cpu-freq", fmtFreq(cpu.freqMhz));
  setText("cpu-fan", fmtFan(cpu.fanRpm, cpu.fanPct));
  setFanSource("cpu-fan", cpu.fanSource);
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
    setText("gpu-fan", "--");
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
  // 风扇：优先真实 RPM，其次驱动给的占空比；拿不到就是 --
  setText("gpu-fan", fmtFan(gpu.fanRpm, gpu.fanPct));
  setFanSource("gpu-fan", gpu.fanSource);

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
 * 未安装时后端不会自动提权，而是在这里引导用户点一下按钮。
 *
 * 联想机型读「风扇转速」同样需要管理员权限（走同一个提权子进程），
 * 所以下面的「启用」按钮复用它——一次 UAC 同时搞定温度与风扇。 */
let pawnio = null;
let pawnioBusy = false;
/* 高级采集通道状态（联想风扇 / PawnIO），由 hardware_status 返回 */
let hwStatus = null;
/* 风扇号偏好：厂商的 FanID 约定（0 = CPU）在个别机型上对不上时，让用户手动纠正 */
let fanPref = null;

async function refreshPawnioStatus() {
  // 非 Windows 没有 PawnIO 这个概念（命令也不存在，invoke 会抛错）
  if (!isWindows) {
    pawnio = null;
    return;
  }
  try {
    pawnio = await invoke("pawnio_status");
  } catch (e) {
    // 拿不到状态就不显示安装入口，避免误报
    pawnio = null;
  }
}

async function refreshHardwareStatus() {
  if (isWindows) {
    try {
      hwStatus = await invoke("hardware_status");
    } catch (e) {
      hwStatus = null;
    }
  } else {
    hwStatus = null;
  }
  // 风扇偏好是跨平台命令（Linux 也用同一份 fan.json），照常刷新
  try {
    fanPref = await invoke("get_fan_pref");
  } catch (e) {
    fanPref = null;
  }
}

/** 设置页「CPU 风扇」下拉：备选项是 hardware_status 报上来的真实风扇列表。
 * 只有厂商通道（当前是联想）才有可辨识的风扇号，其它机型整行隐藏。 */
function renderFanCpuSelect() {
  const row = document.getElementById("fan-cpu-row");
  const sel = document.getElementById("fan-cpu-select");
  if (!row || !sel) return;

  const fans = (hwStatus && hwStatus.lenovoFans) || [];
  row.classList.toggle("hidden", !(hwStatus && hwStatus.isLenovo) || fans.length === 0);
  if (fans.length === 0) return;

  const current = fanPref ? fanPref.cpuFanId : null;
  const known = fans.some((f) => f.fanId === current);
  // 手动指定的号当前没读到（风扇列表变了 / 桥接没起来）时也要留着这个选项，
  // 否则下拉会静默跳回「自动」，用户以为设置丢了
  const extra = current != null && !known ? [{ fanId: current, rpm: null }] : [];

  sel.innerHTML = [`<option value="">自动</option>`]
    .concat(
      fans
        .concat(extra)
        .map(
          (f) =>
            `<option value="${f.fanId}">风扇 ${f.fanId}${
              f.rpm == null ? " · 未检测到" : ` · ${Math.round(f.rpm)} RPM`
            }</option>`,
        ),
    )
    .join("");
  sel.value = current == null ? "" : String(current);
}

/** 设置页「高级采集」区块：把各采集通道的当前状态摆出来（只读，操作按钮在下方 hint 里）。
 * 数据来自 pawnio_status / hardware_status，采样时每 10 拍才更新一次，这里仅做映射。 */
function renderAdvanced() {
  const box = document.getElementById("adv-status");
  if (!box) return;

  const items = [];
  if (pawnio) {
    items.push(
      pawnio.installed
        ? { state: "ok", label: "PawnIO 驱动", value: "已安装 · 可读取 CPU 核心温度" }
        : {
            state: "warn",
            label: "PawnIO 驱动",
            value: pawnio.reason || "未安装 · CPU 核心温度不可用",
          },
    );
  }
  if (hwStatus && hwStatus.isLenovo) {
    items.push(
      hwStatus.lenovoFanReady
        ? { state: "ok", label: "厂商风扇通道", value: "已启用 · 可读取风扇转速" }
        : { state: "warn", label: "厂商风扇通道", value: "待启用 · 读取风扇转速需要管理员权限" },
    );
  } else if (hwStatus) {
    items.push({ state: "idle", label: "厂商风扇通道", value: "非联想机型 · 走通用通道" });
  }

  const rows = items.length
    ? items
    : [{ state: "idle", label: "检测中", value: "正在读取各采集通道状态…" }];

  box.innerHTML = rows
    .map(
      (it) => `<div class="adv-item ${it.state}">
        <i class="adv-dot" aria-hidden="true"></i>
        <span class="adv-text"><b>${esc(it.label)}</b><small>${esc(it.value)}</small></span>
      </div>`,
    )
    .join("");

  const pending = items.filter((it) => it.state === "warn").length;
  setText("adv-tag", items.length ? (pending ? `${pending} 项待处理` : "全部就绪") : "检测中…");

  // 「永久关闭」了主页面提醒时，在这里给一条改回来的入口，避免变成单向门
  const mutedNote = document.getElementById("tip-muted-note");
  if (mutedNote) mutedNote.classList.toggle("hidden", !tipMutedForever);

  renderFanCpuSelect();
}

/* 改「CPU 风扇」下拉：后端与采样线程共用同一份配置，下一拍读数就换过来 */
const fanCpuSelect = document.getElementById("fan-cpu-select");
if (fanCpuSelect) {
  fanCpuSelect.addEventListener("change", async () => {
    const raw = fanCpuSelect.value;
    const cpuFanId = raw === "" ? null : Number(raw);
    try {
      // 回显以后端返回值为准（越界的号会被清洗成「自动」）
      fanPref = await invoke("set_fan_pref", { pref: { cpuFanId } });
      renderFanCpuSelect();
    } catch (e) {
      console.error("保存 CPU 风扇设置失败:", e);
    }
  });
}

/* ---------------- 主页面顶部提示条 ---------------- */

/* PawnIO 未安装时，在监控页顶部提醒用户去设置页安装/启用。
 * 只提醒、不代劳——真正的安装入口在设置页「高级采集」，避免主页面弹 UAC。
 * 点「×」后由用户选：仅本次（不落盘）/ 永久关闭（落 localStorage）。 */
const TIP_MUTE_KEY = "pd.pawnioTipMuted";

function readTipMuted() {
  try {
    return localStorage.getItem(TIP_MUTE_KEY) === "1";
  } catch (e) {
    return false; // 隐私模式等场景不可读，按「未永久关闭」处理
  }
}

function writeTipMuted(muted) {
  try {
    if (muted) localStorage.setItem(TIP_MUTE_KEY, "1");
    else localStorage.removeItem(TIP_MUTE_KEY);
  } catch (e) {
    /* 不可写就只在本次运行内生效（tipMutedForever 已改） */
  }
}

/** 用户选了「永久关闭」——跨启动生效 */
let tipMutedForever = readTipMuted();
/** 用户选了「仅本次」——只挡本次运行 */
let tipClosedThisRun = false;

/** 展开 / 收起确认态（点「×」之后出现的那三个按钮） */
function setTipConfirm(on) {
  const el = document.getElementById("tipbar");
  if (el) el.classList.toggle("tipbar--confirm", on);
}

function renderPawnioTip() {
  const el = document.getElementById("tipbar");
  if (!el) return;
  // 已经在设置页了就没必要再提醒（那里的「高级采集」直接给了状态与入口）
  const onMonitorPage = viewSettings.classList.contains("hidden");
  const missing = !!pawnio && !pawnio.installed;
  const show = missing && !tipMutedForever && !tipClosedThisRun && onMonitorPage;
  el.classList.toggle("hidden", !show);
  // 提示条收起时把确认态一并复位，下次出现仍是「去设置 / ×」
  if (!show) setTipConfirm(false);
}

function renderThermal(thermal, cpu, gpu) {
  setText("cpu-temp", thermal.cpuTempC != null ? `${thermal.cpuTempC.toFixed(0)} °C` : "--");
  const el = document.getElementById("thermal-hint");

  // 温度不可用 → 优先走原来那套提示（可能带「安装 PawnIO」按钮）
  if (!thermal.available && thermal.cpuTempC == null) {
    el.classList.remove("hidden");
    renderThermalUnavailable(el, thermal);
    return;
  }

  // 温度正常，但风扇读不到 → 给一条风扇说明（不挡温度）
  const cpuFanMissing = cpu && cpu.fanRpm == null && cpu.fanPct == null;
  const gpuFanMissing =
    gpu && gpu.available && gpu.fanRpm == null && gpu.fanPct == null;
  if (cpuFanMissing && gpuFanMissing) {
    el.classList.remove("hidden");
    renderFanHint(el, gpuFanMissing);
    return;
  }
  if (cpuFanMissing) {
    el.classList.remove("hidden");
    renderFanHint(el, false);
    return;
  }

  el.classList.add("hidden");
}

/* 风扇读不到时的说明。CPU 与 GPU 的成因不同，分开写清楚。
 * 联想机型另有一套说明 + 「启用」按钮（需要管理员权限）。 */
const FAN_HINT_CPU =
  "CPU 风扇转速读不到：转速由主板 EC / Super I/O 控制，Windows 未向用户态程序公开；" +
  "装 OpenHardwareMonitor（或旧版 LibreHardwareMonitor ≤ 0.9.4）后本工具会自动读到。";
const FAN_HINT_GPU =
  "GPU 风扇转速读不到：显卡驱动未上报（笔记本独显的风扇通常由 EC 控制，NVIDIA 驱动读数为 N/A）。";
const FAN_HINT_BOTH = `${FAN_HINT_CPU} ${FAN_HINT_GPU}`;
const FAN_HINT_LENOVO =
  "本机是联想设备，风扇转速可通过厂商 ACPI 通道读取，但需要管理员权限。点击「启用」并按提示确认一次即可。";

function renderFanHint(el, both) {
  // 联想机型：给出「启用」按钮（读风扇要走提权子进程）
  if (hwStatus && hwStatus.isLenovo && hwStatus.needsEnable) {
    el.innerHTML =
      `<span class="hint-text">${esc(FAN_HINT_LENOVO)}</span>` +
      `<button class="hint-btn" id="hw-enable"${pawnioBusy ? " disabled" : ""}>` +
      `${pawnioBusy ? "启用中…" : "启用"}</button>`;
    document.getElementById("hw-enable").addEventListener("click", enableElevated);
    return;
  }
  el.innerHTML = `<span class="hint-text">${esc(both ? FAN_HINT_BOTH : FAN_HINT_CPU)}</span>`;
}

function renderThermalUnavailable(el, thermal) {
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
  if (state.latest) renderThermal(state.latest.thermal, state.latest.cpu, state.latest.gpu);
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
  if (state.latest) renderThermal(state.latest.thermal, state.latest.cpu, state.latest.gpu);
}

/** 请求后端启用高级采集（联想机型读风扇转速需要管理员权限）。
 * 与安装 PawnIO 一样走提权子进程，用户只需在 UAC 上点一次「是」。 */
async function enableElevated() {
  if (pawnioBusy) return;
  pawnioBusy = true;
  if (state.latest) renderThermal(state.latest.thermal, state.latest.cpu, state.latest.gpu);
  try {
    await invoke("enable_elevated");
  } catch (e) {
    console.error("请求启用高级采集失败:", e);
  }
  // 轮询直到桥接把风扇读数写出来（或超时）
  const deadline = Date.now() + 120000;
  while (Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, 2000));
    await refreshHardwareStatus();
    if (hwStatus && hwStatus.lenovoFanReady) break;
  }
  await refreshPawnioStatus();
  pawnioBusy = false;
  if (state.latest) renderThermal(state.latest.thermal, state.latest.cpu, state.latest.gpu);
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

/* ---------------- 设置：桌面宠物 ---------------- */

/* 宠物配置三件套：开不开、显示哪些指标、整体不透明度。
 *
 * 分工：前端只负责「改配置 + 本地缓存」，窗口的创建/销毁由后端 `set_pet_config` 完成——
 * 这样主面板最小化到托盘时宠物依然在桌面上，也不存在两个窗口抢窗口句柄的问题。
 *
 * 后端会清洗配置（去掉非法指标、至少保留一项、透明度夹到 0.3~1.0），
 * 所以每次提交都用**返回值**回显，避免界面与实际状态不一致。 */
const PET_KEY = "pd.petConfig";
const PET_FALLBACK = { enabled: false, metrics: ["cpu", "mem", "gpu"], opacity: 1 };

const petSwitch = document.getElementById("opt-pet");
const petMetricBox = document.getElementById("pet-metrics");
const petOpacity = document.getElementById("pet-opacity");
const petOpacityOut = document.getElementById("pet-opacity-out");

const petStatusRow = document.getElementById("pet-status");
const petStatusText = document.getElementById("pet-status-text");

let petCfg = { ...PET_FALLBACK };
/** 宠物窗口的运行状态（由 pet_status 每拍刷新，仅设置页可见时查） */
let petAlive = null;

/** 窗口状态行：把「开着」和「真的在渲染」分开说——
 * 透明无边框窗口一旦没渲染出来，屏幕上就是「什么都没有」，用户很难判断是关了还是坏了。 */
function renderPetStatus() {
  if (!petStatusRow) return;
  const on = !!petCfg.enabled;
  let state = "idle";
  let text = "已关闭";
  if (on && petAlive) {
    const { windowOpen, alive, heartbeatAgeMs } = petAlive;
    if (!windowOpen) {
      state = "warn";
      text = "窗口未创建 · 保存设置或重启一次应用即可";
    } else if (alive) {
      state = "ok";
      text = `运行中 · ${Math.max(1, Math.round((heartbeatAgeMs ?? 1000) / 1000))}s 内心跳`;
    } else {
      state = "warn";
      text = "窗口在，但页面没有心跳 · 多为 WebView 未渲染，可重启应用试试";
    }
  } else if (on) {
    text = "检测中…";
  }
  petStatusRow.className = `adv-item ${state}`;
  setText("pet-status-text", text);
}

async function refreshPetStatus() {
  try {
    petAlive = await invoke("pet_status");
  } catch (e) {
    petAlive = null;
  }
  renderPetStatus();
}

function petMetricInputs() {
  return Array.from(petMetricBox.querySelectorAll('input[type="checkbox"]'));
}

/** 配置 → 控件（包括后端清洗后的回显） */
function paintPetCfg() {
  petSwitch.checked = !!petCfg.enabled;
  petMetricInputs().forEach((input) => {
    input.checked = (petCfg.metrics || []).includes(input.value);
    const chip = input.closest(".chip");
    if (chip) chip.classList.toggle("is-on", input.checked);
  });
  const pct = Math.round((petCfg.opacity ?? 1) * 100);
  petOpacity.value = String(pct);
  petOpacityOut.textContent = `${pct}%`;
  setText("pet-tag", petCfg.enabled ? "已开启" : "已关闭");
  renderPetStatus();
}

/** 控件 → 配置 */
function readPetForm() {
  return {
    enabled: petSwitch.checked,
    metrics: petMetricInputs()
      .filter((i) => i.checked)
      .map((i) => i.value),
    opacity: Number(petOpacity.value) / 100,
  };
}

async function pushPetCfg(cfg) {
  try {
    petCfg = await invoke("set_pet_config", { config: cfg });
  } catch (e) {
    console.error("保存桌面宠物配置失败:", e);
    return;
  }
  try {
    localStorage.setItem(PET_KEY, JSON.stringify(petCfg));
  } catch (e) {
    /* 隐私模式等场景不可写：本次运行内仍生效，下次启动回落到后端 pet.json */
  }
  paintPetCfg();
}

async function initPetSettings() {
  let stored = null;
  try {
    stored = JSON.parse(localStorage.getItem(PET_KEY) || "null");
  } catch (e) {
    stored = null;
  }
  if (stored && Array.isArray(stored.metrics)) {
    // 本地缓存优先（用户上次的选择），顺带把窗口恢复出来
    await pushPetCfg(stored);
  } else {
    try {
      petCfg = await invoke("get_pet_config");
    } catch (e) {
      console.error("读取桌面宠物配置失败:", e);
      petCfg = { ...PET_FALLBACK };
    }
  }
  paintPetCfg();
}

petSwitch.addEventListener("change", () => pushPetCfg(readPetForm()));

petMetricInputs().forEach((input) => {
  input.addEventListener("change", () => pushPetCfg(readPetForm()));
});

// 拖动时只更新数字，松手才提交（避免滑杆每动一格就打一次后端）
petOpacity.addEventListener("input", () => {
  petOpacityOut.textContent = `${petOpacity.value}%`;
});
petOpacity.addEventListener("change", () => pushPetCfg(readPetForm()));

/* ---------------- 设置：任务栏显示 ---------------- */

/* 与桌面宠物同一套路：前端只负责「改配置 + 本地缓存」，窗口的创建 / 销毁 / 贴到任务栏哪
 * 全由后端 `set_taskbar_config` 完成（后端会清洗配置：剔除非法指标、至少保留一项），
 * 所以每次提交都用**返回值**回显。挂件没有「位置」也没有「透明度」——
 * 它是贴在任务栏上的一条，位置由后端按任务栏算，不透明度由任务栏主题决定。 */
const TASKBAR_KEY = "pd.taskbarConfig";
const TASKBAR_FALLBACK = { enabled: false, metrics: ["cpu", "mem", "net"] };

const taskbarSwitch = document.getElementById("opt-taskbar");
const taskbarMetricBox = document.getElementById("taskbar-metrics");
const taskbarStatusRow = document.getElementById("taskbar-status");

let taskbarCfg = { ...TASKBAR_FALLBACK };
/** 挂件窗口的运行状态（由 taskbar_status 每拍刷新，仅设置页可见时查） */
let taskbarAlive = null;

/** 状态行：把「开着」「藏起来了」「没建出来」分开说。
 * 全屏应用在前台时挂件是**故意**藏起来的，不能跟「坏了」混为一谈。 */
function renderTaskbarStatus() {
  if (!taskbarStatusRow) return;
  const on = !!taskbarCfg.enabled;
  let state = "idle";
  let text = "已关闭";
  if (on && taskbarAlive) {
    const { windowOpen, alive, heartbeatAgeMs, hiddenFullscreen } = taskbarAlive;
    if (!windowOpen) {
      state = "warn";
      text = "窗口未创建 · 保存设置或重启一次应用即可";
    } else if (hiddenFullscreen) {
      text = "全屏应用在前台，已暂时隐藏";
    } else if (alive) {
      state = "ok";
      text = `运行中 · ${Math.max(1, Math.round((heartbeatAgeMs ?? 1000) / 1000))}s 内心跳`;
    } else {
      state = "warn";
      text = "窗口在，但页面没有心跳 · 多为 WebView 未渲染，可重启应用试试";
    }
  } else if (on) {
    text = "检测中…";
  }
  taskbarStatusRow.className = `adv-item ${state}`;
  setText("taskbar-status-text", text);
}

async function refreshTaskbarStatus() {
  try {
    taskbarAlive = await invoke("taskbar_status");
  } catch (e) {
    taskbarAlive = null;
  }
  renderTaskbarStatus();
}

function taskbarMetricInputs() {
  return Array.from(taskbarMetricBox.querySelectorAll('input[type="checkbox"]'));
}

/** 配置 → 控件（包括后端清洗后的回显） */
function paintTaskbarCfg() {
  taskbarSwitch.checked = !!taskbarCfg.enabled;
  taskbarMetricInputs().forEach((input) => {
    input.checked = (taskbarCfg.metrics || []).includes(input.value);
    const chip = input.closest(".chip");
    if (chip) chip.classList.toggle("is-on", input.checked);
  });
  setText("taskbar-tag", taskbarCfg.enabled ? "已开启" : "已关闭");
  renderTaskbarStatus();
}

/** 控件 → 配置 */
function readTaskbarForm() {
  return {
    enabled: taskbarSwitch.checked,
    metrics: taskbarMetricInputs()
      .filter((i) => i.checked)
      .map((i) => i.value),
  };
}

async function pushTaskbarCfg(cfg) {
  try {
    taskbarCfg = await invoke("set_taskbar_config", { config: cfg });
  } catch (e) {
    console.error("保存任务栏显示配置失败:", e);
    return;
  }
  try {
    localStorage.setItem(TASKBAR_KEY, JSON.stringify(taskbarCfg));
  } catch (e) {
    /* 隐私模式等场景不可写：本次运行内仍生效，下次启动回落到后端 taskbar.json */
  }
  paintTaskbarCfg();
  refreshTaskbarStatus();
}

async function initTaskbarSettings() {
  let stored = null;
  try {
    stored = JSON.parse(localStorage.getItem(TASKBAR_KEY) || "null");
  } catch (e) {
    stored = null;
  }
  if (stored && Array.isArray(stored.metrics)) {
    // 本地缓存优先（用户上次的选择），顺带把挂件恢复出来
    await pushTaskbarCfg(stored);
  } else {
    try {
      taskbarCfg = await invoke("get_taskbar_config");
    } catch (e) {
      console.error("读取任务栏显示配置失败:", e);
      taskbarCfg = { ...TASKBAR_FALLBACK };
    }
  }
  paintTaskbarCfg();
  await refreshTaskbarStatus();
}

taskbarSwitch.addEventListener("change", () => pushTaskbarCfg(readTaskbarForm()));

taskbarMetricInputs().forEach((input) => {
  input.addEventListener("change", () => pushTaskbarCfg(readTaskbarForm()));
});

/* ---------------- 页面切换：监控 / 设置 ---------------- */

const viewMonitor = document.getElementById("view-monitor");
const viewSettings = document.getElementById("view-settings");
const navButtons = Array.from(document.querySelectorAll(".nav-btn"));
const VIEW_KEY = "pd.activeView";

function switchView(name) {
  const toSettings = name === "settings";
  viewMonitor.classList.toggle("hidden", toSettings);
  viewSettings.classList.toggle("hidden", !toSettings);
  navButtons.forEach((b) => b.classList.toggle("active", b.dataset.view === name));
  // 进入设置页时立刻补一次采集通道 / 宠物窗口状态（不必等下一拍）
  if (toSettings) {
    renderAdvanced();
    refreshPetStatus();
    refreshTaskbarStatus();
  }
  // 提示条只在监控页出现，切页后立即同步
  renderPawnioTip();
  try {
    localStorage.setItem(VIEW_KEY, toSettings ? "settings" : "monitor");
  } catch (e) {
    /* 隐私模式等场景不可写，忽略即可 */
  }
}

navButtons.forEach((b) => b.addEventListener("click", () => switchView(b.dataset.view)));

/* 提示条交互：
 * 「去设置」跳设置页；「×」展开确认态；确认态里选「永久关闭 / 仅本次 / 取消」 */
function onTipClick(id, fn) {
  const el = document.getElementById(id);
  if (el) el.addEventListener("click", fn);
}

onTipClick("tipbar-action", () => switchView("settings"));
onTipClick("tipbar-close", () => setTipConfirm(true));
onTipClick("tipbar-forget", () => {
  tipMutedForever = true;
  writeTipMuted(true);
  renderPawnioTip();
});
onTipClick("tipbar-once", () => {
  tipClosedThisRun = true;
  renderPawnioTip();
});
onTipClick("tipbar-cancel", () => setTipConfirm(false));

// 设置页「高级采集」里可以把「永久关闭」改回来
onTipClick("tip-unmute", () => {
  tipMutedForever = false;
  writeTipMuted(false);
  renderAdvanced();
});

// 记住上次停留的页面（默认监控页）
(function restoreView() {
  let saved = "monitor";
  try {
    saved = localStorage.getItem(VIEW_KEY) || "monitor";
  } catch (e) {
    /* 忽略 */
  }
  switchView(saved === "settings" ? "settings" : "monitor");
})();

/* 启动：设置异步加载，同时先渲染首屏再开启轮询
 * （不用 top-level await，保持构建目标兼容） */
initSettings();
initPetSettings();
initTaskbarSettings();
tick().then(schedule);
