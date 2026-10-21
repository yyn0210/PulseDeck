// 移动 App 端布局的**预览卡**注册表：桌面「编辑布局 → 手机 / 平板」目标里挂的就是这些。
// 形态刻意复刻 App 端卡片而非复用桌面组件——编辑器要 WYSIWYG 地告诉用户「设备上会长这样」。
// 卡片形态与 PC 主面板对齐（环形图 + 明细行 / 双速率 + 迷你趋势 / 逐卷磁盘 / 每核条），
// 但只用 App 端可用的原语实现：环是「两半遮罩 + 旋转边框圆」（conic-gradient 在
// uni-app x 的 Android 原生端不可用）、趋势是 CSS 细条、不引 canvas——预览里看到的
// 就是 App 上能渲染的。
// 组件类型白名单与网格列数（手机 4 / 平板 6）同后端 mobile_layout.rs、App 端
// protocol.uts 三处同步；这里的 maxW 取两形态的宽松值 6（手机目标会被
// layout.clampRect 的 profile.cols 夹回 4）。

import {
  fmtPercent,
  fmtFreq,
  fmtFan,
  fmtDuration,
  esc,
} from "../modules/format.js";

/** 详细行的显示门槛：h ≥ 3 才显示明细（与 App 端的精简/详细口径一致） */
export const DETAIL_H = 3;

/** 趋势 / 网络迷你趋势的点数（App 端同为环形历史，这里取一屏可读的量） */
const SPARK_POINTS = 60;

/** 逐卷磁盘条数上限与挂载点排序——与推送的静态信息消息（schema 3）同口径 */
const VOL_CAP = 4;

const GIB = 1024 * 1024 * 1024;

/** 与后端 info 消息同一套截断（brand 36 / 其它显卡 / 挂载点 16，按字符）——
 * 预览所见即 App 所得，不截就会骗用户 */
function trunc(s, n) {
  if (!s) return "";
  return [...String(s)].length > n ? [...String(s)].slice(0, n).join("") : String(s);
}

/** 与 App 端 fmtPairBytes 同款：共用单位、各 1 位小数（10.2/17.0 GB）——
 * 手机卡窄，两边各带单位会被省略号截掉 */
function fmtPairBytes(used, total) {
  if (used == null || total == null || used < 0 || total <= 0) return "--";
  const gib = 1024 * 1024 * 1024;
  const mib = 1024 * 1024;
  if (total >= gib) return `${(used / gib).toFixed(1)}/${(total / gib).toFixed(1)} GB`;
  if (total >= mib) return `${(used / mib).toFixed(1)}/${(total / mib).toFixed(1)} MB`;
  return `${fmtBytesApp(used)} / ${fmtBytesApp(total)}`;
}

/** 与 App 端 fmtBytes 同款（KB / MB / GB 一律 1 位小数）——
 * 桌面主面板的 fmtBytes 在 <10 时给 2 位、≥100 时给 0 位，移动卡按 App 口径来 */
function fmtBytesApp(v) {
  if (v == null || v <= 0) return "--";
  if (v < 1024) return `${v.toFixed(0)} B`;
  if (v < 1024 * 1024) return `${(v / 1024).toFixed(1)} KB`;
  if (v < 1024 * 1024 * 1024) return `${(v / 1024 / 1024).toFixed(1)} MB`;
  return `${(v / 1024 / 1024 / 1024).toFixed(1)} GB`;
}

/** 与 App 端 fmtBps 同款（速率一律 1 位小数，单位随量级）——
 * 桌面主面板的 fmtRate 在 <10 MB 时给 2 位小数，移动卡必须按 App 的口径来 */
function fmtBps(v) {
  if (v == null || v < 0) return "--";
  if (v < 1024) return `${v.toFixed(0)} B/s`;
  if (v < 1024 * 1024) return `${(v / 1024).toFixed(1)} KB/s`;
  if (v < 1024 * 1024 * 1024) return `${(v / 1024 / 1024).toFixed(1)} MB/s`;
  return `${(v / 1024 / 1024 / 1024).toFixed(1)} GB/s`;
}

/**
 * 两半遮罩环的弧角度（与 App 端同一套数学）：进度 p% → 右/左两段弧的旋转角。
 * p ≤ 50 只有右半出弧（rotate = p×3.6 − 135°，弧端点恰好落在 p×3.6° 处）；
 * p > 50 右半固定满半（45°），左半接着转（p×3.6 − 315°）。p = 0 时右弧
 * 自然转出遮罩（不可见），不需要额外的隐藏分支。
 */
export function ringAngles(pct) {
  const p = Math.max(0, Math.min(100, pct));
  return {
    r: p <= 50 ? p * 3.6 - 135 : 45,
    l: p > 50 ? p * 3.6 - 315 : 0,
  };
}

/** 组件定义的公共尺寸段（11 种卡一致，省得每处抄一遍）。
 * minH=2：一行（92px）连环都装不下，缩到 1 行只会把卡内容裁掉；
 * h=2 是精简形态（h<3）里环能完整显示的最小高度。 */
function baseDef(type, title, accent) {
  return { type, title, accent, minW: 1, minH: 2, maxW: 6, maxH: 6, defW: 2, defH: 3 };
}

/* ---- 环形卡工厂（cpu / 内存 / GPU / 电池）：环 + 中心大数字 + 明细行 ---- */

/**
 * @param {(snap: object) => {pct: number|null, big: string, label: string, rows: [string, string][]}} main
 *   每拍取数：pct 是环的百分比（null = 读不到，环与数值一起降级）；rows 是详细行。
 */
function ringCard(type, title, accent, main) {
  return {
    ...baseDef(type, title, accent),
    mount(root) {
      root.classList.add("mob-card");
      root.innerHTML = `
        <header class="mob-head"><h2>${title}</h2></header>
        <div class="mob-ring">
          <i class="mob-ring-track"></i>
          <span class="mob-ring-half r"><i></i></span>
          <span class="mob-ring-half l"><i></i></span>
          <div class="mob-ring-center"><b class="mob-big">--</b><span class="mob-ring-label"></span></div>
        </div>
        <ul class="mob-kv"></ul>`;
      const big = root.querySelector(".mob-big");
      const label = root.querySelector(".mob-ring-label");
      const halfR = root.querySelector(".mob-ring-half.r");
      const halfL = root.querySelector(".mob-ring-half.l");
      const arcR = halfR.firstElementChild;
      const arcL = halfL.firstElementChild;
      const kv = root.querySelector(".mob-kv");
      return {
        update(snap, ctx) {
          root.classList.toggle("mob-compact", (ctx?.h ?? 3) < DETAIL_H);
          const { pct, big: b, label: lb, rows } = main(snap);
          big.textContent = b;
          label.textContent = lb;
          const has = pct != null && Number.isFinite(pct);
          halfR.classList.toggle("hidden", !has);
          // 左半只在过半后出现（见 ringAngles：≤50% 的角度遮不出空弧，直接藏）
          halfL.classList.toggle("hidden", !has || pct <= 50);
          if (has) {
            const a = ringAngles(pct);
            arcR.style.transform = `rotate(${a.r}deg)`;
            arcL.style.transform = `rotate(${a.l}deg)`;
          }
          kv.innerHTML = rows.map(([k, v]) => `<li><span>${k}</span><b>${esc(v)}</b></li>`).join("");
        },
      };
    },
  };
}

/** 电池实时功率（与 PC 电源功率卡同口径）：充电取充入、电池供电取放电，
 * 已接入未充电（满电续充 / 养护模式）如实给 0；没有电池为 null。 */
function batteryRateW(b) {
  if (!b?.hasBattery) return null;
  if (b.charging) return b.chargeRateW ?? null;
  if (!b.acOnline) return b.dischargeRateW ?? null;
  return 0;
}

/* ---- 各环形卡取数：字段口径与 PC 卡片一致（功耗在电源卡、不在 CPU 卡） ---- */

const cpuCard = ringCard("cpu", "处理器", "cpu", (snap) => {
  const { cpu, thermal } = snap;
  return {
    pct: cpu.usageTotal,
    // 与 App 端 fmtPct 同为 1 位小数（预览不能再四舍五入，否则数字都不同）
    big: fmtPercent(cpu.usageTotal),
    label: "总占用",
    rows: [
      ["型号", trunc(String(cpu.brand ?? "").trim(), 36) || "--"],
      ["核心", `${cpu.physicalCores} 核 / ${cpu.logicalCores} 线程`],
      [cpu.freqLive ? "当前主频" : "基准主频", fmtFreq(cpu.freqMhz)],
      ["温度", thermal?.cpuTempC != null ? `${thermal.cpuTempC.toFixed(1)} °C` : "--"],
      ["风扇", fmtFan(cpu.fanRpm, cpu.fanPct)],
    ],
  };
});

const memCard = ringCard("mem", "内存", "mem", (snap) => {
  const m = snap.memory;
  return {
    pct: m.usagePct,
    big: fmtPercent(m.usagePct),
    label: "已使用",
    rows: [
      ["已用", fmtPairBytes(m.usedBytes, m.totalBytes)],
      // 推送不带 availableBytes（预算）：App 端用 total−used 计算，sysinfo 口径下是精确值
      ["可用", fmtBytesApp(Math.max(0, m.totalBytes - m.usedBytes))],
    ],
  };
});

const gpuCard = ringCard("gpu", "GPU", "gpu", (snap) => {
  const g = snap.gpu;
  if (!g?.available) {
    return { pct: null, big: "--", label: "未检测到", rows: [["型号", "--"]] };
  }
  const dev = g.devices?.[g.mainIndex];
  const others = (g.devices ?? [])
    .filter((d) => d.index !== g.mainIndex)
    .map((d) => trunc(d.name, 16))
    .slice(0, 2); // info 消息最多带 2 个其它显卡（与后端 INFO_OTHERS_CAP 同口径）
  // 显存与 App 同一口径：总量在 info 消息里取整到 GB，已用 = 总量 × 每拍 memPct
  // （App 拿不到 memUsedBytes；预览若直接报字节会多出两位小数，和真机不一样）
  const totalGB = g.memTotalBytes > 0 ? Math.round(g.memTotalBytes / GIB) : 0;
  const memText =
    totalGB > 0 && g.memPct != null
      ? `${((totalGB * g.memPct) / 100).toFixed(1)} / ${totalGB.toFixed(0)} GB`
      : "--";
  return {
    pct: g.usagePct,
    big: g.usagePct != null ? fmtPercent(g.usagePct) : "--",
    label: g.usagePct != null ? "显卡占用" : "占用未知",
    rows: [
      ["型号", dev?.name ?? "GPU"],
      ["显存", memText],
      ["温度", g.tempC != null ? `${g.tempC.toFixed(1)} °C` : "--"],
      ["功耗", g.powerW != null ? `${g.powerW.toFixed(1)} W` : "--"],
      ["风扇", fmtFan(g.fanRpm, g.fanPct)],
      ...(others.length ? [["其它显卡", others.join(" · ")]] : []),
    ],
  };
});

const batCard = ringCard("bat", "电池", "bat", (snap) => {
  const b = snap.battery;
  if (!b?.hasBattery) return { pct: null, big: "--", label: "无电池", rows: [] };
  const cap =
    b.remainingWh != null && b.fullChargeWh != null
      ? // info 消息按整数 Wh 下发，预览同口径（所见即所得）
        `${Math.round(b.remainingWh)} / ${Math.round(b.fullChargeWh)} Wh`
      : null;
  const eta = b.charging
    ? b.secondsToFull != null
      ? ["充满需要", fmtDuration(b.secondsToFull)]
      : null
    : b.secondsToEmpty != null
      ? ["剩余续航", fmtDuration(b.secondsToEmpty)]
      : null;
  return {
    pct: b.percent,
    big: b.percent != null ? fmtPercent(b.percent) : "--",
    label: b.charging ? "充电中" : b.acOnline ? "已接入未充电" : "放电中",
    rows: [...(cap ? [["容量", cap]] : []), ...(eta ? [eta] : [])],
  };
});

/* ---- 电源功率：大数字（电池实时功率，PC 同口径）+ 三行副指标 ---- */

const powerCard = {
  ...baseDef("power", "电源功率", "power"),
  mount(root) {
    root.classList.add("mob-card");
    root.innerHTML = `
      <header class="mob-head"><h2>电源功率</h2></header>
      <div class="mob-power"><b class="mob-big">--</b><i>W</i></div>
      <ul class="mob-kv"></ul>`;
    const big = root.querySelector(".mob-big");
    const kv = root.querySelector(".mob-kv");
    return {
      update(snap, ctx) {
        root.classList.toggle("mob-compact", (ctx?.h ?? 3) < DETAIL_H);
        const { thermal, battery } = snap;
        const w = batteryRateW(battery);
        big.textContent = w != null ? (w === 0 ? "0.0" : w.toFixed(1)) : "--";
        kv.innerHTML = [
          ["CPU 封装", thermal?.cpuPowerW != null ? `${thermal.cpuPowerW.toFixed(1)} W` : "--"],
          ["电池", w != null ? `${w.toFixed(1)} W` : "--"],
          ["适配器", battery?.acOnline ? "已接入" : battery?.hasBattery ? "未接入" : "--"],
        ]
          .map(([k, v]) => `<li><span>${k}</span><b>${esc(v)}</b></li>`)
          .join("");
      },
    };
  },
};

/* ---- 网络：下行/上行双速率 + 迷你趋势（按历史峰值归一）+ 累计行 ---- */

const netCard = {
  ...baseDef("net", "网络", null),
  mount(root) {
    root.classList.add("mob-card");
    root.innerHTML = `
      <header class="mob-head"><h2>网络</h2></header>
      <div class="mob-rates">
        <div class="mob-rate rx"><span class="arrow">↓</span><b>--</b></div>
        <div class="mob-rate tx"><span class="arrow">↑</span><b>--</b></div>
      </div>
      <div class="mob-spark"></div>
      <ul class="mob-kv"></ul>`;
    const down = root.querySelector(".mob-rate.rx b");
    const up = root.querySelector(".mob-rate.tx b");
    const body = root.querySelector(".mob-spark");
    const kv = root.querySelector(".mob-kv");
    const hist = [];
    const bars = Array.from({ length: SPARK_POINTS }, () => {
      const el = document.createElement("i");
      el.innerHTML = "<b class='r'></b><b class='t'></b>";
      body.append(el);
      return el;
    });
    return {
      update(snap, ctx) {
        root.classList.toggle("mob-compact", (ctx?.h ?? 3) < DETAIL_H);
        const n = snap.net;
        down.textContent = fmtBps(n.rxBytesPerSec);
        up.textContent = fmtBps(n.txBytesPerSec);
        hist.push({ r: n.rxBytesPerSec, t: n.txBytesPerSec });
        if (hist.length > SPARK_POINTS) hist.shift();
        // 速率不是百分比：按两条历史里的最大值归一，否则几 MB/s 的波在 100% 标尺下看不到
        let max = 1;
        for (const p of hist) max = Math.max(max, p.r, p.t);
        bars.forEach((el, i) => {
          const p = hist[i];
          el.querySelector(".r").style.height = p ? `${(p.r / max) * 100}%` : "0%";
          el.querySelector(".t").style.height = p ? `${(p.t / max) * 100}%` : "0%";
        });
        // 累计流量按 MiB 取整（推送字段是整数 MiB），预览同口径；
        // 直接报字节会多出几位，和 App 上看到的数不一样
        const mib = 1024 * 1024;
        const recv = Math.floor(n.totalReceived / mib) * mib;
        const sent = Math.floor(n.totalTransmitted / mib) * mib;
        kv.innerHTML = `<li><span>累计</span><b>↓ ${fmtBytesApp(recv)} · ↑ ${fmtBytesApp(sent)}</b></li>`;
      },
    };
  },
};

/* ---- 硬盘：逐卷容量面板（与 schema 3 同口径：按挂载点排序、cap 4）+ 合计速率行 ---- */

const diskCard = {
  ...baseDef("disk", "硬盘", "disk"),
  mount(root) {
    root.classList.add("mob-card");
    root.innerHTML = `
      <header class="mob-head"><h2>硬盘</h2></header>
      <div class="mob-vols"></div>
      <ul class="mob-kv"></ul>`;
    const vols = root.querySelector(".mob-vols");
    const kv = root.querySelector(".mob-kv");
    return {
      update(snap, ctx) {
        root.classList.toggle("mob-compact", (ctx?.h ?? 3) < DETAIL_H);
        const list = (snap.disks ?? [])
          .filter((d) => d.totalBytes > 0)
          .sort((a, b) => (a.mountPoint || "").localeCompare(b.mountPoint || ""))
          .slice(0, VOL_CAP);
        vols.innerHTML = list
          .map((d) => {
            const pct = Math.max(0, Math.min(100, Math.round(d.usagePct)));
            // info 消息按整数百分比与 GiB 下发（容量是整除，不是四舍五入），
            // 预览同口径（所见即所得）
            return `
          <div class="mob-vol">
            <div class="mob-vol-head"><b>${esc(trunc(d.mountPoint || d.name, 16))}</b><span>${pct}%</span></div>
            <div class="mob-vol-bar"><i style="width:${pct}%"></i></div>
            <div class="mob-vol-meta">${Math.floor(d.usedBytes / GIB)} / ${Math.floor(d.totalBytes / GIB)} GB</div>
          </div>`;
          })
          .join("");
        let read = 0;
        let write = 0;
        for (const d of snap.disks ?? []) {
          read += d.readBytesPerSec;
          write += d.writeBytesPerSec;
        }
        kv.innerHTML = [
          ["读", fmtBps(read)],
          ["写", fmtBps(write)],
        ]
          .map(([k, v]) => `<li><span>${k}</span><b>${esc(v)}</b></li>`)
          .join("");
      },
    };
  },
};

/* ---- CPU 与内存趋势：图例 + 细条 sparkline + 当前值行（App 端同款，不引 canvas） ---- */

const trendCard = {
  ...baseDef("trend", "趋势", null),
  defW: 4,
  defH: 2,
  mount(root) {
    root.classList.add("mob-card");
    root.innerHTML = `
      <header class="mob-head"><h2>CPU 与内存趋势</h2></header>
      <div class="mob-legend"><i class="c"></i>CPU<i class="m"></i>内存</div>
      <div class="mob-spark"></div>
      <div class="mob-sub">CPU -- · 内存 --</div>`;
    const body = root.querySelector(".mob-spark");
    const sub = root.querySelector(".mob-sub");
    const hist = [];
    const bars = Array.from({ length: SPARK_POINTS }, () => {
      const el = document.createElement("i");
      el.innerHTML = "<b class='c'></b><b class='m'></b>";
      body.append(el);
      return el;
    });
    return {
      update(snap, ctx) {
        root.classList.toggle("mob-compact", (ctx?.h ?? 3) < DETAIL_H);
        hist.push({ c: snap.cpu?.usageTotal ?? 0, m: snap.memory?.usagePct ?? 0 });
        if (hist.length > SPARK_POINTS) hist.shift();
        bars.forEach((el, i) => {
          const p = hist[i];
          el.querySelector(".c").style.height = p ? `${Math.max(0, Math.min(100, p.c))}%` : "0%";
          el.querySelector(".m").style.height = p ? `${Math.max(0, Math.min(100, p.m))}%` : "0%";
        });
        const last = hist[hist.length - 1];
        sub.textContent = `CPU ${fmtPercent(last?.c)} · 内存 ${fmtPercent(last?.m)}`;
      },
      destroy() {
        hist.length = 0;
      },
    };
  },
};

/* ---- 每核负载：每核「序号 + 横条 + 数值」两列网格（PC 端同形态，三档变色） ---- */

const coresCard = {
  ...baseDef("cores", "每核负载", "cpu"),
  mount(root) {
    root.classList.add("mob-card");
    root.innerHTML = `
      <header class="mob-head"><h2>每核负载</h2></header>
      <b class="mob-big mob-peak">--</b>
      <div class="mob-core-grid"></div>`;
    const grid = root.querySelector(".mob-core-grid");
    const big = root.querySelector(".mob-peak");
    let cells = [];
    const tierOf = (v) => (v >= 85 ? "t3" : v >= 60 ? "t2" : "");
    return {
      update(snap, ctx) {
        root.classList.toggle("mob-compact", (ctx?.h ?? 3) < DETAIL_H);
        const per = snap.cpu?.perCore ?? [];
        // 核数不变时只更新条宽与数值；变了才重建（首拍 / 远程唤醒核心数变化的情形）
        if (cells.length !== per.length) {
          grid.innerHTML = per
            .map((_, i) => `<div class="mob-core"><span>${i + 1}</span><div class="mob-core-bar"><i></i></div><b></b></div>`)
            .join("");
          cells = [...grid.querySelectorAll(".mob-core")];
        }
        let peak = 0;
        per.forEach((v, i) => {
          // 推送的 cores.load 是四舍五入后的整数（后端 round），预览同口径——
          // 直接用原始浮点会让峰值 / 条宽和 App 差零点几个百分点
          const pct = Math.max(0, Math.min(100, Math.round(v)));
          peak = Math.max(peak, pct);
          const cell = cells[i];
          cell.className = `mob-core ${tierOf(pct)}`.trim();
          cell.querySelector("i").style.width = `${pct}%`;
          cell.querySelector("b").textContent = `${pct}%`;
        });
        big.textContent = per.length ? fmtPercent(peak) : "--";
      },
    };
  },
};

/* ---- 进程榜：序号 + 名字 + 双指标（排序列提亮，PC 表格的移动版） ---- */

function procCard(type, title, byMem) {
  return {
    ...baseDef(type, title, null),
    mount(root) {
      root.classList.add("mob-card");
      root.innerHTML = `
        <header class="mob-head"><h2>${title}</h2></header>
        <ol class="mob-list"></ol>`;
      const list = root.querySelector(".mob-list");
      return {
        update(snap, ctx) {
          root.classList.toggle("mob-compact", (ctx?.h ?? 3) < DETAIL_H);
          // App 只收到「按 CPU 排的 top5」这一份列表（schema 1 的 procs 组）：
          // 内存榜是同一份列表在端上按 memMiB 重排，不是 PC 的 topMem 榜。
          // 预览必须照同一口径来，否则进程名/数值会和真机不同。
          const base = (snap.cpu?.topCpu ?? []).slice(0, 5);
          const rows = byMem
            ? [...base].sort((a, b) => Math.floor(b.memoryBytes / 1024 / 1024) - Math.floor(a.memoryBytes / 1024 / 1024))
            : base;
          list.innerHTML = rows
            .map((p, i) => {
              const cpu = fmtPercent(p.cpuUsage, 1);
              // 推送的 memMiB 是整数 MiB（整除）；展示口径与 App 端一致：
              // <1 GiB 报整数 MB，否则保留 1 位小数的 GB
              const mib = Math.floor(p.memoryBytes / 1024 / 1024);
              const mem = mib >= 1024 ? `${(mib / 1024).toFixed(1)} GB` : `${mib} MB`;
              // 推送把进程名截到 16 字符（按字符），预览同口径
              const name = trunc(p.name, 16);
              // 两榜列序与 PC 一致：CPU 榜「CPU、内存」，内存榜「内存、CPU」，排序列在前且提亮
              const [a, b] = byMem ? [mem, cpu] : [cpu, mem];
              return `<li><i>${i + 1}</i><span class="mob-name">${esc(name)}</span><b class="lead">${a}</b><b>${b}</b></li>`;
            })
            .join("");
        },
      };
    },
  };
}

/** 「编辑布局 → 手机 / 平板」chips 的展示序（与后端 MOBILE_WIDGET_TYPES 同序） */
export const MOBILE_ORDER = [
  "cpu",
  "mem",
  "bat",
  "gpu",
  "power",
  "net",
  "trend",
  "cores",
  "disk",
  "procCpu",
  "procMem",
];

const WIDGETS = {
  cpu: cpuCard,
  mem: memCard,
  bat: batCard,
  gpu: gpuCard,
  power: powerCard,
  net: netCard,
  trend: trendCard,
  disk: diskCard,
  cores: coresCard,
  procCpu: procCard("procCpu", "进程 · CPU", false),
  procMem: procCard("procMem", "进程 · 内存", true),
};

/** 按 type 取移动端预览卡定义；未知类型返回 null（布局清洗据此过滤）。 */
export function mobileDef(type) {
  return Object.prototype.hasOwnProperty.call(WIDGETS, type) ? WIDGETS[type] : null;
}
