// 展示层格式化工具（Windows 下按 1024 进制 + 中文 GB 习惯展示）

const BYTE_UNITS = ["B", "KB", "MB", "GB", "TB", "PB"];

export function fmtBytes(n) {
  if (n === null || n === undefined) return "--";
  let v = Number(n);
  let i = 0;
  while (v >= 1024 && i < BYTE_UNITS.length - 1) {
    v /= 1024;
    i += 1;
  }
  if (i === 0) return `${Math.round(v)} B`;
  const digits = v >= 100 ? 0 : v >= 10 ? 1 : 2;
  return `${v.toFixed(digits)} ${BYTE_UNITS[i]}`;
}

export function fmtRate(bytesPerSec) {
  if (bytesPerSec === null || bytesPerSec === undefined) return "--";
  return `${fmtBytes(bytesPerSec)}/s`;
}

export function fmtWatts(w, digits = 1) {
  if (w === null || w === undefined) return "--";
  const v = Number(w);
  if (v === 0) return "0 W";
  return v >= 100 ? `${v.toFixed(0)} W` : `${v.toFixed(digits)} W`;
}

export function fmtPercent(v, digits = 1) {
  if (v === null || v === undefined) return "--";
  return `${Number(v).toFixed(digits)}%`;
}

export function fmtDuration(seconds) {
  if (!seconds || seconds <= 0 || seconds >= 0xffffffff) return "--";
  const s = Math.round(seconds);
  const d = Math.floor(s / 86400);
  const h = Math.floor((s % 86400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (d > 0) return `${d} 天 ${h} 小时`;
  if (h > 0) return `${h} 小时 ${m} 分`;
  if (m > 0) return `${m} 分 ${s % 60} 秒`;
  return `${s} 秒`;
}

export function fmtUptime(seconds) {
  if (!seconds) return "--";
  const s = Math.round(seconds);
  const d = Math.floor(s / 86400);
  const h = Math.floor((s % 86400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const pad = (x) => String(x).padStart(2, "0");
  return d > 0 ? `${d} 天 ${pad(h)}:${pad(m)}` : `${pad(h)}:${pad(m)}:${pad(sec)}`;
}

export function fmtClock(ms) {
  if (!ms) return "--";
  return new Date(ms).toLocaleTimeString("zh-CN", { hour12: false });
}

export function fmtFreq(mhz) {
  if (!mhz) return "--";
  return mhz >= 1000 ? `${(mhz / 1000).toFixed(2)} GHz` : `${mhz} MHz`;
}

export function esc(value) {
  return String(value ?? "").replace(/[&<>"']/g, (c) => {
    switch (c) {
      case "&":
        return "&amp;";
      case "<":
        return "&lt;";
      case ">":
        return "&gt;";
      case '"':
        return "&quot;";
      default:
        return "&#39;";
    }
  });
}

export function setText(id, value) {
  const el = document.getElementById(id);
  if (el) el.textContent = value;
}
