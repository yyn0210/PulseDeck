/**
 * 打字猫模式的紧凑指标条。
 *
 * 与经典贴图模式的竖向指标行不同，这里把指标排成一行（名称 + 迷你条 + 数值），
 * 温度作为独立项跟在后面；数值口径与 `src/pet/classic.js` 完全一致：
 * 百分比取整、越界夹到 0~100、读不到显示 `--` 并标灰。
 */

const DEFS = {
  cpu: { label: "CPU", color: "#4f8cff", pick: (m) => m.cpuPct },
  mem: { label: "内存", color: "#22d3a5", pick: (m) => m.memPct },
  gpu: { label: "GPU", color: "#35d6c4", pick: (m) => m.gpuPct },
};

export function createMetricsBar(host) {
  let built = null;
  const cells = new Map();
  let tempEl = null;
  let tempValue = null;

  /** 只在勾选项真的变了时重建 DOM（每 3 秒轮询配置会反复调用） */
  function build(keys) {
    const list = (keys || []).filter((k) => DEFS[k]);
    const sig = list.join(",");
    if (sig === built) return;
    built = sig;
    cells.clear();
    host.innerHTML = "";

    for (const key of list) {
      const def = DEFS[key];
      const el = document.createElement("div");
      el.className = "cat-metric";
      el.innerHTML =
        `<span class="cat-metric-k">${def.label}</span>` +
        `<span class="cat-metric-bar"><i></i></span>` +
        `<b class="cat-metric-v">--</b>`;
      el.querySelector(".cat-metric-bar > i").style.background = def.color;
      host.appendChild(el);
      cells.set(key, {
        el,
        bar: el.querySelector(".cat-metric-bar > i"),
        value: el.querySelector(".cat-metric-v"),
      });
    }

    if (!cells.size) {
      const empty = document.createElement("div");
      empty.className = "cat-metric cat-metric-empty";
      empty.textContent = "未选择指标";
      host.appendChild(empty);
    }

    tempEl = document.createElement("div");
    tempEl.className = "cat-metric cat-metric-temp";
    tempEl.innerHTML = `<span class="cat-metric-k">温度</span><b class="cat-metric-v">--</b>`;
    tempValue = tempEl.querySelector(".cat-metric-v");
    host.appendChild(tempEl);
  }

  /** 每拍更新数字与进度条；`color` 是当前温度档位色 */
  function update(m, { color } = {}) {
    for (const [key, cell] of cells) {
      const raw = m ? DEFS[key].pick(m) : null;
      const v = Number.isFinite(raw) ? Math.max(0, Math.min(100, raw)) : null;
      cell.bar.style.width = `${v ?? 0}%`;
      cell.value.textContent = v == null ? "--" : `${v.toFixed(0)}%`;
      cell.el.classList.toggle("is-missing", v == null);
    }

    if (!tempEl) return;
    const temp = m && Number.isFinite(m.cpuTempC) ? m.cpuTempC : null;
    tempValue.textContent = temp == null ? "--" : `${temp.toFixed(0)}°C`;
    tempEl.classList.toggle("is-missing", temp == null);
    if (color) tempEl.style.setProperty("--temp-accent", color);
  }

  return { build, update };
}
