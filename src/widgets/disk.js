// 硬盘小组件：各卷容量条 + 读写速率 / 累计量
import { esc, fmtBytes, fmtRate } from "../modules/format.js";
import { binder } from "./helpers.js";

const TEMPLATE = `
<header class="card-head">
  <h2>硬盘</h2>
  <span class="tag" id="disk-tag">--</span>
</header>
<div class="disk-list" id="disk-list"></div>`;

export default {
  type: "disk",
  title: "硬盘",
  accent: "disk",
  minW: 4,
  minH: 3,
  maxW: 12,
  maxH: 12,
  defW: 5,
  defH: 8,
  mount(root) {
    root.innerHTML = TEMPLATE;
    const set = binder(root);
    const list = root.querySelector("#disk-list");
    return {
      update(snap) {
        const disks = snap.disks;
        if (!disks.length) {
          list.innerHTML = `<div class="disk">未检测到任何卷</div>`;
          set("#disk-tag", "0 个卷");
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
        set("#disk-tag", `${disks.length} 个卷`);
      },
    };
  },
};
