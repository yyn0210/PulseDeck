// 进程榜小组件：CPU 榜与内存榜各一个组件（列序不同——排序依据那一列在前）
import { esc, fmtBytes } from "../modules/format.js";

/**
 * 表格行。两榜的列序不同——排序依据那一列在前（CPU 榜「CPU、内存」，
 * 内存榜「内存、CPU」），表头就是这么写的，这里必须同序输出，
 * 否则数值会跟表头错位；排序列加 `.lead` 提亮。
 */
function procRows(rows, sortKey) {
  if (!rows.length) return `<tr><td colspan="5">暂无数据</td></tr>`;
  const cells = {
    cpuUsage: (p, lead) => `<td class="${lead ? "lead" : ""}">${p.cpuUsage.toFixed(1)}%</td>`,
    memoryBytes: (p, lead) =>
      `<td class="${lead ? "lead" : ""}">${fmtBytes(p.memoryBytes)}</td>`,
  };
  const otherKey = sortKey === "cpuUsage" ? "memoryBytes" : "cpuUsage";
  return rows
    .map(
      (p, i) => `
      <tr>
        <td>${i + 1}</td>
        <td class="pname">${esc(p.name)}</td>
        <td>${p.pid}</td>
        ${cells[sortKey](p, true)}
        ${cells[otherKey](p, false)}
      </tr>`,
    )
    .join("");
}

function makeProcWidget(type, title, sortKey, headCells) {
  return {
    type,
    title,
    minW: 4,
    minH: 3,
    maxW: 12,
    maxH: 12,
    defW: 6,
    defH: 6,
    mount(root) {
      root.innerHTML = `
<header class="card-head">
  <h2>${esc(title)}</h2>
  <span class="tag">--</span>
</header>
<div class="proc-scroll">
  <table class="proc-table">
    <thead>
      <tr>${headCells.map((h) => `<th>${h}</th>`).join("")}</tr>
    </thead>
    <tbody></tbody>
  </table>
</div>`;
      const tbody = root.querySelector("tbody");
      const tag = root.querySelector(".tag");
      return {
        update(snap) {
          const rows = sortKey === "cpuUsage" ? snap.cpu.topCpu : snap.cpu.topMem;
          tbody.innerHTML = procRows(rows, sortKey);
          tag.textContent = `Top ${rows.length} · ${sortKey === "cpuUsage" ? "按 CPU" : "按内存"}`;
        },
      };
    },
  };
}

export const procCpu = makeProcWidget("procCpu", "Top 进程 · CPU", "cpuUsage", [
  "#",
  "进程",
  "PID",
  "CPU",
  "内存",
]);

export const procMem = makeProcWidget("procMem", "Top 进程 · 内存", "memoryBytes", [
  "#",
  "进程",
  "PID",
  "内存",
  "CPU",
]);
