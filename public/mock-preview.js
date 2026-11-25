/**
 * 仅开发预览用的 Tauri 运行时替身。
 *
 * 浏览器里打开 index.html?mock=1 时会注入 window.__TAURI_INTERNALS__，
 * 让前端在脱离 Tauri 外壳的情况下也能渲染出完整界面，方便调布局与样式。
 * 正常使用（Tauri 环境）不会加载到这套替身，因为没有 ?mock 参数。
 */
(function () {
  const params = new URLSearchParams(location.search);
  if (!params.has("mock")) return;

  // `?mock=1&pawnio=missing` 用来预览「未装 PawnIO、引导用户安装」这一状态
  const pawnioMissing = params.get("pawnio") === "missing";
  // `?mock=1&fan=on` 用来预览「风扇能读到」的排版（默认是读不到的诚实降级态）
  const fanOn = params.get("fan") === "on";
  // `?mock=1&lenovo=1` 用来预览「联想机型、风扇待启用」的提示与「启用」按钮
  const lenovo = params.get("lenovo") === "1";
  // 桌面宠物：`?mock=1&hot=1` 预览过热态（红色档）；`?mock=1&nogpu=1` 预览读不到 GPU 占用；
  // `?mock=1&temp=74` 固定温度，用来逐档核对形象（档位：38/50/63/74/84/94）
  const hot = params.get("hot") === "1";
  const noGpu = params.get("nogpu") === "1";
  // `?mock=1&cpupdh=missing` 预览 CPU 的 PDH 通道读不到（非 Windows 或老系统）：
  // 主频退回标称值、标签显示「基准主频」
  const noCpuPdh = params.get("cpupdh") === "missing";
  const fixedTemp = params.has("temp") ? Number(params.get("temp")) : null;
  // 窗口外壳：真实主窗口是无装饰条窗口（自绘标题栏），最大化状态由这颗开关模拟，
  // 便于在预览里核对「最大化 → 图标变还原、四边热区收起」这一套
  let maximized = false;
  // 当前页面是哪个窗口：真实环境由 Tauri 注入，getCurrentWindow() 要读它
  const windowLabel = location.pathname.includes("pet")
    ? "pet"
    : location.pathname.includes("taskbar")
      ? "taskbar"
      : "main";

  let tick = 0;
  let callbackId = 0;
  const jitter = (base, amp) => Math.max(0, base + (Math.random() - 0.5) * amp);
  const rnd = (base, amp) => Math.round(jitter(base, amp));

  function snapshot() {
    tick += 1;
    const perCore = Array.from({ length: 16 }, () => rnd(22, 46));
    const cpuTotal = perCore.reduce((a, b) => a + b, 0) / perCore.length;
    const gpuUsage = jitter(18, 26);
    const memTotal = 16 * 1024 * 1024 * 1024;
    const memUsed = 9.4 * 1024 * 1024 * 1024 + tick * 1024 * 1024;
    return {
      tsMs: Date.now(),
      sampleIntervalMs: 1000,
      system: {
        hostName: "DLK-YANGYN-PC",
        osName: "Windows",
        osVersion: "11 (24H2)",
        longOsVersion: "Windows 11 专业版",
        kernelVersion: "10.0.26100",
        cpuArch: "x86_64",
        uptimeSecs: 31234,
        bootTimeSecs: 1759000000,
        processCount: 268,
      },
      cpu: {
        brand: "12th Gen Intel(R) Core(TM) i7-12700H",
        usageTotal: cpuTotal,
        logicalCores: 20,
        physicalCores: 14,
        perCore,
        freqMhz: noCpuPdh ? 2300 : rnd(2980, 700),
        // 实时主频标记（真机上由 Windows 的 PDH `Actual Frequency` 给出；
        // 预览里按「有读数 / 恒标称」两种状态编造）
        freqLive: !noCpuPdh,
        fanRpm: fanOn ? rnd(2400, 260) : null,
        fanPct: null,
        fanSource: fanOn ? "root\\OpenHardwareMonitor" : "",
        topCpu: [
          { pid: 11212, name: "chrome.exe", cpuUsage: 18.4, memoryBytes: 812 * 1024 * 1024 },
          { pid: 4512, name: "Code.exe", cpuUsage: 12.1, memoryBytes: 640 * 1024 * 1024 },
          { pid: 8836, name: "nvcontainer.exe", cpuUsage: 4.2, memoryBytes: 96 * 1024 * 1024 },
          { pid: 220, name: "explorer.exe", cpuUsage: 3.1, memoryBytes: 148 * 1024 * 1024 },
          { pid: 7712, name: "WeChat.exe", cpuUsage: 2.6, memoryBytes: 420 * 1024 * 1024 },
        ],
        topMem: [
          { pid: 11212, name: "chrome.exe", cpuUsage: 18.4, memoryBytes: 3120 * 1024 * 1024 },
          { pid: 4512, name: "Code.exe", cpuUsage: 12.1, memoryBytes: 1480 * 1024 * 1024 },
          { pid: 7712, name: "WeChat.exe", cpuUsage: 2.6, memoryBytes: 620 * 1024 * 1024 },
          { pid: 220, name: "explorer.exe", cpuUsage: 3.1, memoryBytes: 248 * 1024 * 1024 },
          { pid: 9004, name: "MsMpEng.exe", cpuUsage: 1.2, memoryBytes: 208 * 1024 * 1024 },
        ],
      },
      memory: {
        totalBytes: memTotal,
        usedBytes: memUsed,
        availableBytes: memTotal - memUsed,
        usagePct: ((memUsed / memTotal) * 100).toFixed(1) * 1,
        swapTotalBytes: 4 * 1024 * 1024 * 1024,
        swapUsedBytes: 1.2 * 1024 * 1024 * 1024,
        swapUsagePct: 30,
        commitUsedBytes: 11.6 * 1024 * 1024 * 1024,
        commitLimitBytes: 22 * 1024 * 1024 * 1024,
        commitPct: 52.7,
        systemLoadPct: 61,
      },
      disks: [
        {
          name: "NVMe KIOXIA 1024GB",
          mountPoint: "C:\\",
          fileSystem: "NTFS",
          diskKind: "SSD",
          removable: false,
          totalBytes: 1024 * 1024 * 1024 * 1024,
          usedBytes: 612 * 1024 * 1024 * 1024,
          freeBytes: 412 * 1024 * 1024 * 1024,
          usagePct: 59.8,
          readBytesPerSec: rnd(2 * 1024 * 1024, 6 * 1024 * 1024),
          writeBytesPerSec: rnd(600 * 1024, 3 * 1024 * 1024),
          totalReadBytes: 412 * 1024 * 1024 * 1024,
          totalWrittenBytes: 208 * 1024 * 1024 * 1024,
        },
        {
          name: "Samsung SSD 870",
          mountPoint: "D:\\",
          fileSystem: "NTFS",
          diskKind: "SSD",
          removable: false,
          totalBytes: 2000 * 1024 * 1024 * 1024,
          usedBytes: 1420 * 1024 * 1024 * 1024,
          freeBytes: 580 * 1024 * 1024 * 1024,
          usagePct: 71,
          readBytesPerSec: rnd(80 * 1024, 400 * 1024),
          writeBytesPerSec: rnd(20 * 1024, 200 * 1024),
          totalReadBytes: 92 * 1024 * 1024 * 1024,
          totalWrittenBytes: 41 * 1024 * 1024 * 1024,
        },
      ],
      battery: {
        hasBattery: true,
        acOnline: true,
        charging: false,
        discharging: true,
        percent: 78,
        fullChargeWh: 84.2,
        remainingWh: 65.7,
        chargeRateW: null,
        dischargeRateW: 0.1,
        secondsToFull: null,
        secondsToEmpty: 14400,
        source: "CallNtPowerInformation(SystemBatteryState)",
        statusText: "适配器已接入，当前未充电",
      },
      gpu: {
        available: true,
        source: "nvml+pdh",
        mainIndex: 0,
        usagePct: gpuUsage,
        memUsedBytes: (1.8 + tick * 0.01) * 1024 * 1024 * 1024,
        memTotalBytes: 6 * 1024 * 1024 * 1024,
        memPct: 30,
        tempC: jitter(48, 6),
        powerW: jitter(11.4, 8),
        fanPct: fanOn ? rnd(38, 12) : null,
        fanRpm: null,
        fanSource: fanOn ? "NVIDIA 驱动" : "",
        // 双显卡：独显（NVML，主卡）+ 核显（Windows 计数器），
        // 核显只出现在「其它显卡」那一行（temp/power 在 Windows 上读不到，如实为 null）
        devices: [
          {
            index: 0,
            name: "NVIDIA GeForce RTX 3060 Laptop GPU",
            usagePct: gpuUsage,
            memUsedBytes: 1.8 * 1024 * 1024 * 1024,
            memTotalBytes: 6 * 1024 * 1024 * 1024,
            memPct: 30,
            tempC: jitter(48, 6),
            powerW: jitter(11.4, 8),
            fanPct: fanOn ? rnd(38, 12) : null,
            fanRpm: null,
            source: "nvml",
          },
          {
            index: 1,
            name: "Intel(R) Iris(R) Xe Graphics",
            usagePct: jitter(4, 6),
            memUsedBytes: 0.6 * 1024 * 1024 * 1024,
            memTotalBytes: 24 * 1024 * 1024 * 1024,
            memPct: 2.5,
            tempC: null,
            powerW: null,
            fanPct: null,
            fanRpm: null,
            source: "pdh",
          },
        ],
      },
      net: {
        rxBytesPerSec: rnd(1.6 * 1024 * 1024, 3 * 1024 * 1024),
        txBytesPerSec: rnd(320 * 1024, 700 * 1024),
        totalReceived: 650387632,
        totalTransmitted: 234517557,
        interfaces: [
          {
            name: "WLAN",
            mac: "08:8E:90:07:39:E7",
            isVirtual: false,
            rxBytesPerSec: rnd(1.6 * 1024 * 1024, 3 * 1024 * 1024),
            txBytesPerSec: rnd(320 * 1024, 700 * 1024),
            totalReceived: 650387632,
            totalTransmitted: 234517557,
          },
          {
            name: "以太网",
            mac: "8C:16:45:22:A1:0B",
            isVirtual: false,
            rxBytesPerSec: rnd(40 * 1024, 300 * 1024),
            txBytesPerSec: rnd(8 * 1024, 90 * 1024),
            totalReceived: 18345678901,
            totalTransmitted: 2145678901,
          },
          {
            name: "vEthernet (Default Switch)",
            mac: "00:15:5D:83:84:88",
            isVirtual: true,
            rxBytesPerSec: 0,
            txBytesPerSec: 0,
            totalReceived: 0,
            totalTransmitted: 318758,
          },
          {
            name: "VMware Network Adapter VMnet8",
            mac: "00:50:56:C0:00:08",
            isVirtual: true,
            rxBytesPerSec: 0,
            txBytesPerSec: 0,
            totalReceived: 9,
            totalTransmitted: 1959,
          },
        ],
      },
      thermal: pawnioMissing
        ? {
            cpuTempC: null,
            cpuSource: "不可用",
            gpuTempC: null,
            gpuSource: "不可用",
            available: false,
            cpuPowerW: jitter(44, 12),
            hint: "未安装 PawnIO 内核驱动，无法读取 CPU 温度；请在界面上点击「安装 PawnIO」",
          }
        : {
            cpuTempC: jitter(62, 8),
            cpuSource: "root\\LibreHardwareMonitor",
            gpuTempC: jitter(48, 6),
            gpuSource: "nvml",
            available: true,
            cpuPowerW: jitter(44, 12),
            hint: "",
          },
    };
  }

  const settings = { autostart: false, closeToTray: true };
  // 桌面宠物：预览模式下不真的开窗，只把配置存起来，供 pet.html?mock=1 读取。
  // `?mock=1&pet=cat&render=3d&skin=black&petlayout=108&view=viewer&petscale=0.8&collar=red&headphones=on`
  // 可预览打字猫的各个形态；不带参数时保持旧默认（关闭 + 经典配置）。
  const petMode = params.get("pet") === "classic" ? "classic" : "cat";
  const petRender = params.get("render") === "3d" ? "3d" : "2d";
  const petSkin = params.get("skin") || "black";
  const petLayout = params.get("petlayout") === "108" ? "108" : "65";
  const petOrientation = params.get("view") === "viewer" ? "viewer" : "cat";
  const petScale = Number(params.get("petscale")) || 1;
  const petCollar = params.get("collar") || "none";
  const petHeadphones = params.get("headphones") === "on" ? "on" : "off";
  const petPrivate = params.get("private") === "1";
  let petConfig = {
    enabled: false,
    metrics: ["cpu", "mem", "gpu"],
    opacity: 1,
    mode: petMode,
    renderMode: petRender,
    skin: petSkin,
    layout: petLayout,
    orientation: petOrientation,
    scale: petScale,
    sound: true,
    clickThrough: false,
    privacyGuard: true,
    collar: petCollar,
    headphones: petHeadphones,
  };
  // 任务栏挂件：同上，供 taskbar.html?mock=1 读取。
  // `?mock=1&theme=light` 预览浅色任务栏、`?mock=1&vertical=1` 预览竖排任务栏（贴屏幕左右侧）
  let taskbarConfig = { enabled: false, metrics: ["cpu", "mem", "net"] };
  const taskbarTheme = params.get("theme") === "light" ? "light" : "dark";
  const taskbarVertical = params.get("vertical") === "1";
  // 数据推送：同上。`?mock=1&push=on` 预览「运行中」状态行（无视开关，专看文案排版）
  const pushOn = params.get("push") === "on";
  let pushConfig = {
    enabled: pushOn,
    tcpEnabled: true,
    bleEnabled: pushOn,
    port: 17432,
    intervalMs: 1000,
    metrics: ["cpu", "mem", "gpu", "net"],
  };
  // 移动 App 布局（mobile-layout.json 的后端镜像）：每个设备两份布局
  // （流式 flow / 固定一屏 grid）+ 当前激活模式。mock 里在内存中持久，刷新回默认。
  let mobileLayout = {
    phone: {
      mode: "flow",
      flow: {
        v: 1,
        widgets: [
          { type: "cpu", x: 0, y: 0, w: 2, h: 3 },
          { type: "mem", x: 2, y: 0, w: 2, h: 3 },
          { type: "gpu", x: 0, y: 3, w: 2, h: 3 },
          { type: "bat", x: 2, y: 3, w: 2, h: 3 },
          { type: "net", x: 0, y: 6, w: 2, h: 3 },
          { type: "disk", x: 2, y: 6, w: 2, h: 3 },
        ],
      },
      grid: {
        v: 1,
        widgets: [
          { type: "cpu", x: 0, y: 0, w: 1, h: 2 },
          { type: "mem", x: 1, y: 0, w: 1, h: 2 },
          { type: "gpu", x: 2, y: 0, w: 1, h: 2 },
          { type: "bat", x: 3, y: 0, w: 1, h: 2 },
          { type: "power", x: 0, y: 2, w: 1, h: 2 },
          { type: "net", x: 1, y: 2, w: 1, h: 2 },
          { type: "disk", x: 2, y: 2, w: 1, h: 2 },
          { type: "trend", x: 3, y: 2, w: 1, h: 2 },
        ],
      },
    },
    pad: {
      mode: "flow",
      flow: {
        v: 1,
        widgets: [
          { type: "cpu", x: 0, y: 0, w: 3, h: 3 },
          { type: "mem", x: 3, y: 0, w: 3, h: 3 },
          { type: "gpu", x: 0, y: 3, w: 3, h: 3 },
          { type: "bat", x: 3, y: 3, w: 3, h: 3 },
          { type: "net", x: 0, y: 6, w: 3, h: 3 },
          { type: "disk", x: 3, y: 6, w: 3, h: 3 },
        ],
      },
      grid: {
        v: 1,
        widgets: [
          { type: "cpu", x: 0, y: 0, w: 2, h: 2 },
          { type: "mem", x: 2, y: 0, w: 2, h: 2 },
          { type: "gpu", x: 4, y: 0, w: 2, h: 2 },
          { type: "bat", x: 0, y: 2, w: 2, h: 2 },
          { type: "power", x: 2, y: 2, w: 2, h: 2 },
          { type: "net", x: 4, y: 2, w: 2, h: 2 },
          { type: "disk", x: 0, y: 4, w: 2, h: 2 },
          { type: "trend", x: 2, y: 4, w: 2, h: 2 },
          { type: "cores", x: 4, y: 4, w: 2, h: 2 },
        ],
      },
    },
  };
  const mobileLayoutOf = (device) => (device === "pad" ? mobileLayout.pad : mobileLayout.phone);
  // 风扇号手动指定：null = 自动（与后端 FanPref 同构，越界的号会被清洗成 null）
  let fanPrefCpuId = null;

  window.__TAURI_INTERNALS__ = {
    // 窗口 API（getCurrentWindow）从这里读当前窗口 label
    metadata: {
      currentWindow: { label: windowLabel },
      currentWebview: { label: windowLabel, windowLabel },
    },
    // 事件 API（listen / onResized）注册回调时要先拿个 id；预览里没有事件通道，
    // 给个自增 id 让调用方能正常返回即可（不会有事件真被投递）
    transformCallback: () => ++callbackId,
    invoke(cmd, args) {
      switch (cmd) {
        // 窗口外壳：主窗口无装饰条（标题栏是页面自绘的），预览里照实回答
        case "plugin:window|is_decorated":
          return Promise.resolve(false);
        case "plugin:window|is_maximized":
          return Promise.resolve(maximized);
        case "plugin:window|toggle_maximize":
          maximized = !maximized;
          // 预览里没有事件通道（真实窗口靠 resize 事件驱动页面），直接同步一下 class
          document.documentElement.classList.toggle("is-maximized", maximized);
          return Promise.resolve(null);
        case "get_snapshot":
          return Promise.resolve(snapshot());
        case "get_runtime_config":
          return Promise.resolve({
            paused: false,
            intervalMs: 1000,
            closeToTray: settings.closeToTray,
          });
        case "get_autostart":
          return Promise.resolve(settings.autostart);
        case "set_autostart":
          settings.autostart = !!args.enabled;
          return Promise.resolve(settings.autostart);
        case "set_close_to_tray":
          settings.closeToTray = !!args.enabled;
          return Promise.resolve(settings.closeToTray);
        case "set_paused":
          return Promise.resolve(args.paused);
        case "set_interval":
          return Promise.resolve(args.millis);
        case "pawnio_status":
          return Promise.resolve({
            installed: !pawnioMissing,
            reason: pawnioMissing
              ? "未安装 PawnIO（读取 CPU 核心温度所需的内核驱动）"
              : "PawnIO 已安装",
            installerAvailable: pawnioMissing,
          });
        case "hardware_status":
          return Promise.resolve({
            isLenovo: lenovo,
            pawnioInstalled: !pawnioMissing,
            pawnioReason: pawnioMissing ? "未安装 PawnIO" : "PawnIO 已安装",
            installerAvailable: pawnioMissing,
            // 预演「已启用」的状态：风扇能读到就说明提权桥接在工作
            elevatedActive: fanOn,
            lenovoFanReady: lenovo && fanOn,
            needsEnable: lenovo && !fanOn,
            // 「CPU 风扇」下拉的备选项
            lenovoFans:
              lenovo && fanOn
                ? [
                    { fanId: 0, rpm: rnd(2900, 300) },
                    { fanId: 1, rpm: rnd(2700, 300) },
                  ]
                : [],
            cpuFanId: fanPrefCpuId,
          });
        case "get_fan_pref":
          return Promise.resolve({ cpuFanId: fanPrefCpuId });
        case "set_fan_pref": {
          const id = args.pref && args.pref.cpuFanId;
          // 复刻后端的清洗规则：越界的号退回「自动」
          fanPrefCpuId = id == null || id > 7 ? null : id;
          return Promise.resolve({ cpuFanId: fanPrefCpuId });
        }
        case "get_pet_config":
          return Promise.resolve(petConfig);
        case "set_pet_config": {
          const next = Object.assign({}, petConfig, args.config || {});
          // 复刻后端的清洗规则：过滤非法指标、至少保留一项、透明度夹到 0.3~1.0
          const keys = ["cpu", "mem", "gpu"];
          next.metrics = (next.metrics || []).filter((m) => keys.includes(m));
          if (!next.metrics.length) next.metrics = ["cpu"];
          next.opacity = Math.min(1, Math.max(0.3, Number(next.opacity) || 1));
          const skins = [
            "black",
            "ragdoll",
            "ginger",
            "calico",
            "tuxedo",
            "siamese",
            "dafeiyu",
          ];
          if (!skins.includes(next.skin)) next.skin = "black";
          if (next.mode !== "classic") next.mode = "cat";
          next.renderMode = next.skin === "dafeiyu" || next.renderMode !== "3d" ? "2d" : "3d";
          if (!["65", "108"].includes(next.layout)) next.layout = "65";
          if (!["cat", "viewer"].includes(next.orientation)) next.orientation = "cat";
          next.scale = Math.min(1.5, Math.max(0.2, Number(next.scale) || 1));
          if (!["none", "red", "blue", "pink", "green"].includes(next.collar)) next.collar = "none";
          if (!["off", "on"].includes(next.headphones)) next.headphones = "off";
          petConfig = next;
          return Promise.resolve(petConfig);
        }
        case "get_pet_metrics": {
          const temp = pawnioMissing
            ? null
            : fixedTemp != null
              ? fixedTemp
              : hot
                ? jitter(93, 3)
                : jitter(58, 26);
          return Promise.resolve({
            cpuPct: jitter(28, 30),
            memPct: jitter(62, 8),
            gpuPct: noGpu ? null : jitter(24, 30),
            hasGpu: !noGpu,
            cpuTempC: temp,
            colorSource: temp == null ? "usage" : "temp",
          });
        }
        case "pet_heartbeat":
          return Promise.resolve(null);
        case "pet_status":
          return Promise.resolve({
            windowOpen: petConfig.enabled,
            heartbeatAgeMs: petConfig.enabled ? 400 : null,
            alive: petConfig.enabled,
            inputSupported: true,
          });
        case "get_taskbar_config":
          return Promise.resolve(taskbarConfig);
        case "set_taskbar_config": {
          const next = Object.assign({}, taskbarConfig, args.config || {});
          // 复刻后端的清洗规则：过滤非法指标、至少保留一项
          const keys = ["cpu", "mem", "gpu", "net", "disk", "bat", "temp", "power"];
          next.metrics = (next.metrics || []).filter((m) => keys.includes(m));
          if (!next.metrics.length) next.metrics = ["cpu"];
          taskbarConfig = next;
          return Promise.resolve(taskbarConfig);
        }
        case "taskbar_tick": {
          const s = snapshot();
          return Promise.resolve({
            metrics: {
              cpuPct: s.cpu.usageTotal,
              memPct: s.memory.usagePct,
              gpuPct: noGpu ? null : s.gpu.usagePct,
              netRxBps: s.net.rxBytesPerSec,
              netTxBps: s.net.txBytesPerSec,
              diskReadBps: s.disks.reduce((a, d) => a + d.readBytesPerSec, 0),
              diskWriteBps: s.disks.reduce((a, d) => a + d.writeBytesPerSec, 0),
              batPct: s.battery.hasBattery ? s.battery.percent : null,
              cpuTempC: s.thermal.cpuTempC,
              // 与后端同构：电池功率优先，没充没放时退回 CPU 封装功耗
              powerW: s.battery.chargeRateW || s.battery.dischargeRateW || s.thermal.cpuPowerW,
            },
            vertical: taskbarVertical,
            theme: taskbarTheme,
          });
        }
        case "taskbar_fit":
          return Promise.resolve(null);
        case "taskbar_status":
          return Promise.resolve({
            windowOpen: taskbarConfig.enabled,
            heartbeatAgeMs: taskbarConfig.enabled ? 300 : null,
            alive: taskbarConfig.enabled,
            hiddenFullscreen: false,
          });
        case "get_push_config":
          return Promise.resolve(pushConfig);
        case "set_push_config": {
          const next = Object.assign({}, pushConfig, args.config || {});
          // 复刻后端的清洗规则：端口夹 1~65535（0 回默认）、间隔夹 300~10000、
          // 指标走白名单且至少留一项、通道至少留一个（全关强制回 TCP）
          const port = Number(next.port);
          if (!Number.isFinite(port) || port < 1 || port > 65535) next.port = 17432;
          next.intervalMs = Math.min(10000, Math.max(300, Number(next.intervalMs) || 1000));
          const keys = ["cpu", "mem", "gpu", "bat", "net", "disk", "cores", "procs"];
          next.metrics = (next.metrics || []).filter((m) => keys.includes(m));
          if (!next.metrics.length) next.metrics = ["cpu", "mem", "gpu", "net"];
          if (!next.tcpEnabled && !next.bleEnabled) next.tcpEnabled = true;
          pushConfig = next;
          return Promise.resolve(pushConfig);
        }
        case "push_status": {
          // `&push=on` 无视开关强制「运行中」，专看状态行文案排版
          const run = pushOn || pushConfig.enabled;
          return Promise.resolve({
            enabled: run,
            tcpListening: run && pushConfig.tcpEnabled,
            tcpAddr: "192.168.1.100:17432",
            tcpClients: run && pushConfig.tcpEnabled ? 1 : 0,
            tcpError: "",
            bleAdvertising: run && pushConfig.bleEnabled,
            bleClients: run && pushConfig.bleEnabled ? 1 : 0,
            bleError: "",
            lastPushMs: run ? Date.now() : 0,
          });
        }
        case "get_mobile_layout":
          return Promise.resolve(mobileLayoutOf(args.device));
        case "set_mobile_layout": {
          // 复刻后端清洗：类型白名单 11 种、按 type 去重、w 夹 1~cols、x 夹 0~cols-w
          // （cols：手机 4 / 平板 6）、flow 的 h 夹 1~8 / y 夹 0~2000；grid 的
          // h 夹 1~rows、y 夹 0~rows-h（rows：手机 4 / 平板 6），排序后丢掉重叠项。
          // 与 mobile_layout.rs 的 sanitize 同规则。
          const TYPES = [
            "cpu", "mem", "bat", "gpu", "power", "net",
            "trend", "cores", "disk", "procCpu", "procMem",
          ];
          const isPad = args.device === "pad";
          const cols = isPad ? 6 : 4;
          const rows = isPad ? 6 : 4;
          const mode = args.mode === "grid" ? "grid" : "flow";
          const seen = new Set();
          const widgets = (args.layout && args.layout.widgets) || [];
          const cleanWidgets = widgets
            .filter((w) => w && TYPES.includes(w.type) && !seen.has(w.type) && seen.add(w.type))
            .map((w) => {
              const cw = Math.min(cols, Math.max(1, Math.round(Number(w.w) || 1)));
              const hMax = mode === "grid" ? rows : 8;
              const ch = Math.min(hMax, Math.max(1, Math.round(Number(w.h) || 1)));
              const yMax = mode === "grid" ? Math.max(0, rows - ch) : 2000;
              return {
                type: w.type,
                w: cw,
                x: Math.min(cols - cw, Math.max(0, Math.round(Number(w.x) || 0))),
                y: Math.min(yMax, Math.max(0, Math.round(Number(w.y) || 0))),
                h: ch,
              };
            })
            .sort((a, b) => a.y - b.y || a.x - b.x);
          const kept = [];
          for (const w of cleanWidgets) {
            if (mode === "grid" && kept.some((p) =>
              w.x < p.x + p.w && p.x < w.x + w.w && w.y < p.y + p.h && p.y < w.y + w.h)) {
              continue;
            }
            kept.push(w);
          }
          const clean = { v: 1, widgets: kept };
          const d = mobileLayoutOf(args.device);
          if (mode === "grid") d.grid = clean;
          else d.flow = clean;
          return Promise.resolve(clean);
        }
        case "set_mobile_layout_mode": {
          const d = mobileLayoutOf(args.device);
          d.mode = args.mode === "grid" ? "grid" : "flow";
          return Promise.resolve(d.mode);
        }
        case "show_main_window":
          return Promise.resolve(null);
        case "install_pawnio":
          return Promise.resolve("（预览模式）已请求安装 PawnIO");
        case "enable_elevated":
          return Promise.resolve("（预览模式）已请求启用高级采集");
        default:
          return Promise.resolve(null);
      }
    },
  };
})();
