// GPU 小组件：环形占用 + 型号 / 显存 / 温度 / 功耗 / 风扇明细
import { fmtBytes, fmtFan, fmtWatts } from "../modules/format.js";
import { binder, setFanSource, toggleRow } from "./helpers.js";
import { ringSetter } from "./ring.js";

const TEMPLATE = `
<header class="card-head">
  <h2>GPU</h2>
  <span class="tag" id="gpu-tag">--</span>
</header>
<div class="ring-wrap">
  <div class="ring">
    <svg viewBox="0 0 120 120">
      <circle class="ring-bg" cx="60" cy="60" r="52"></circle>
      <circle class="ring-fg" id="gpu-ring" cx="60" cy="60" r="52"></circle>
    </svg>
    <div class="ring-center">
      <span class="ring-value" id="gpu-value">--</span>
      <span class="ring-label" id="gpu-label">显卡占用</span>
    </div>
  </div>
  <ul class="kv">
    <li><span>型号</span><b id="gpu-name">--</b></li>
    <li><span>显存</span><b id="gpu-mem">--</b></li>
    <li><span>温度</span><b id="gpu-temp">--</b></li>
    <li><span>功耗</span><b id="gpu-power">--</b></li>
    <li><span>风扇</span><b id="gpu-fan">--</b></li>
    <!-- 主卡之外的显卡（笔记本核显 / 多卡机器的第二块），只有一块卡时整行隐藏 -->
    <li id="gpu-others-row" class="hidden"><span>其它显卡</span><b>--</b></li>
  </ul>
</div>`;

// 各数据源在界面上的名字。Windows：NVML（N 卡）/ 性能计数器（核显、A 卡）；
// Linux：NVML（N 卡）/ 显卡驱动 sysfs（amdgpu、i915、xe）
const GPU_SOURCE_LABEL = {
  nvml: "NVIDIA 驱动",
  pdh: "Windows 计数器",
  "pdh-warming": "正在初始化",
  amdgpu: "AMD 驱动",
  amd: "AMD 驱动",
  i915: "Intel 驱动",
  xe: "Intel 驱动",
  intel: "Intel 驱动",
};

function gpuSourceText(gpu) {
  if (!gpu.available) return GPU_SOURCE_LABEL[gpu.source] || "无可用数据源";
  const labels = [...new Set(gpu.devices.map((d) => GPU_SOURCE_LABEL[d.source] || d.source))];
  const multi = gpu.devices.length > 1 ? ` · ${gpu.devices.length} 块显卡` : "";
  return `${labels.join(" + ")}${multi}`;
}

/** 一块卡在「其它显卡」行里的短描述：名字 + 占用（读不到就 --） */
function gpuBrief(d) {
  const usage = d.usagePct != null ? `${d.usagePct.toFixed(0)}%` : "--";
  return `${d.name} ${usage}`;
}

export default {
  type: "gpu",
  title: "GPU",
  accent: "gpu",
  minW: 3,
  minH: 4,
  maxW: 6,
  maxH: 12,
  defW: 3,
  defH: 4,
  mount(root) {
    root.innerHTML = TEMPLATE;
    const setRing = ringSetter(root, "gpu-ring");
    const set = binder(root);
    return {
      update(snap) {
        const gpu = snap.gpu;
        const usage = gpu.usagePct;
        if (!gpu.available) {
          setRing(0);
          set("#gpu-value", "--");
          set("#gpu-label", "未检测到");
          set("#gpu-name", "--");
          set("#gpu-mem", "--");
          set("#gpu-temp", "--");
          set("#gpu-power", "--");
          set("#gpu-fan", "--");
          toggleRow(root, "gpu-others-row", []);
          set("#gpu-tag", "无可用数据源");
          return;
        }

        setRing(usage ?? 0);
        set("#gpu-value", usage != null ? usage.toFixed(0) : "--");
        set("#gpu-label", usage != null ? "显卡占用" : "占用未知");

        // 型号显示**主卡**（后端按 mainIndex 指明）—— 上面的数字就是这块卡的，
        // 别拿列表第一块充数：多显卡时两者常常不是同一块（详见 model.rs 的挑卡规则）
        const main = gpu.devices.find((d) => d.index === gpu.mainIndex) || gpu.devices[0];
        set("#gpu-name", main ? main.name : "--");
        set(
          "#gpu-mem",
          gpu.memTotalBytes
            ? `${fmtBytes(gpu.memUsedBytes)} / ${fmtBytes(gpu.memTotalBytes)}`
            : gpu.memUsedBytes
              ? fmtBytes(gpu.memUsedBytes)
              : "--",
        );
        set("#gpu-temp", gpu.tempC != null ? `${gpu.tempC.toFixed(0)} °C` : "--");
        set("#gpu-power", fmtWatts(gpu.powerW));
        // 风扇：优先真实 RPM，其次驱动给的占空比；拿不到就是 --
        set("#gpu-fan", fmtFan(gpu.fanRpm, gpu.fanPct));
        setFanSource(root, "#gpu-fan", gpu.fanSource);

        // 主卡之外的卡单列一行（笔记本的核显、多卡机器的第二块），不然它们只体现在
        // 「N 块显卡」这几个字里，等于没展示
        const others = gpu.devices.filter((d) => main && d.index !== main.index);
        toggleRow(root, "gpu-others-row", others.map(gpuBrief));

        set("#gpu-tag", gpuSourceText(gpu));
      },
    };
  },
};
