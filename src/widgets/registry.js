// 监控小组件注册表：布局配置里的 type 字符串 → 组件定义。
// WIDGET_ORDER 是「编辑布局 → 添加组件」chips 的展示顺序（按常用程度排）。

import cpu from "./cpu.js";
import mem from "./mem.js";
import bat from "./bat.js";
import gpu from "./gpu.js";
import power from "./power.js";
import net from "./net.js";
import trend from "./trend.js";
import cores from "./cores.js";
import disk from "./disk.js";
import { procCpu, procMem } from "./procs.js";

export const WIDGET_ORDER = [
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
  cpu,
  mem,
  bat,
  gpu,
  power,
  net,
  trend,
  cores,
  disk,
  procCpu,
  procMem,
};

/** 按 type 取组件定义；未知类型返回 null（布局清洗据此过滤）。 */
export function widgetDef(type) {
  return Object.prototype.hasOwnProperty.call(WIDGETS, type) ? WIDGETS[type] : null;
}
