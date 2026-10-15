// 组件间共用的小工具——都是「查找范围限定在组件根元素内」的版本，
// 组件模板沿用原 index.html 里的元素 id（每种组件至多一个实例，id 不会重复）。

import { setTextIn } from "../modules/format.js";

/** setTextIn 的柯里化版：mount 里 `const set = binder(root)`，之后 `set("#id", v)`。 */
export function binder(root) {
  return (selector, value) => setTextIn(root, selector, value);
}

/** 给风扇数值挂一个来源提示（鼠标悬停可见）。
 * 来源字符串来自后端 `FanReading::source`，例如「联想 EC（上限 4500 RPM）」。
 * 空字符串或缺失时清掉 title，避免出现空白浮层。 */
export function setFanSource(root, selector, source) {
  const el = root.querySelector(selector);
  if (!el) return;
  const text = typeof source === "string" ? source.trim() : "";
  if (text) {
    el.title = `来源：${text}`;
  } else {
    el.removeAttribute("title");
  }
}

/** 切换一行「附加信息」：items 为空就把整行藏起来。
 * 值里的每段用 ` · ` 连接（`.kv li b` 是单行 + 省略号，不会撑破卡片）。 */
export function toggleRow(root, rowId, items) {
  const row = root.querySelector(`#${rowId}`);
  if (!row) return;
  const value = row.querySelector("b");
  if (value) value.textContent = items.join(" · ");
  row.classList.toggle("hidden", items.length === 0);
}
