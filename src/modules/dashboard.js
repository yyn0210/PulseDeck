// 监控页小组件面板：持有布局状态与组件实例，向渲染循环分发快照。
// 坐标 / 尺寸的合法性由 layout.js 保证，这里只做挂载与 DOM 同步；
// 拖拽 / 缩放等交互由 editmode.js 驱动，落位后回调 settle()。
// 同一个引擎驱动两套布局目标（profile：桌面 12 列 / 手机预览 4 列），
// 各自独立实例化（main.js 各建一份，存储键与组件注册表都不同）。

import * as layout from "./layout.js";

export class Dashboard {
  /**
   * @param {HTMLElement} gridEl .grid 容器
   * @param {object} profile 布局目标（layout.js 的 DESK / MOBILE：cols / 存储键 / 注册表 / 默认布局）
   */
  constructor(gridEl, profile) {
    this.grid = gridEl;
    this.profile = profile;
    /** @type {{type:string,x:number,y:number,w:number,h:number,el:HTMLElement,inst:object,def:object}[]} */
    this.items = [];
    /** 布局每次变更后的回调，参数是纯数据列表（持久化挂这里） */
    this.onchange = null;
    /** 流式预览：把显式网格坐标换成「按 DOM 顺序 + span w」的自动行高排版。
     * 只影响渲染，不改变 items 里的坐标；退出预览恢复显式落位。 */
    this.flowView = false;
    this.emptyHint = gridEl.querySelector(".grid-empty");
  }

  item(type) {
    return this.items.find((i) => i.type === type) || null;
  }

  getLayout() {
    return this.items.map(({ type, x, y, w, h }) => ({ type, x, y, w, h }));
  }

  /** 全量重挂（启动 / 重置布局用）。传入列表假定已 sanitize。 */
  mountAll(list) {
    for (const it of [...this.items]) this.unmount(it, false);
    this.items = [];
    for (const rect of list) this.mount(rect, false);
    this.commit();
  }

  /** 挂一个组件（type 去重）。rect 形如 {type,x,y,w,h}。 */
  mount(rect, commit = true) {
    const def = this.profile.def(rect.type);
    if (!def || this.item(rect.type)) return null;
    const el = document.createElement("section");
    el.className = "card widget";
    if (def.accent) el.dataset.accent = def.accent;
    el.dataset.type = rect.type;
    const inst = def.mount(el);
    // 编辑模式手柄常驻 DOM（显隐交给 CSS 的 .editing 开关），避免进出编辑模式反复插拔
    const remove = document.createElement("button");
    remove.type = "button";
    remove.className = "w-remove";
    remove.title = "移除组件";
    remove.setAttribute("aria-label", "移除组件");
    remove.textContent = "×";
    const resize = document.createElement("i");
    resize.className = "w-resize";
    resize.title = "调整大小";
    el.append(remove, resize);
    this.grid.append(el);
    const item = { type: rect.type, x: rect.x, y: rect.y, w: rect.w, h: rect.h, el, inst, def };
    this.items.push(item);
    if (commit) this.commit();
    return item;
  }

  unmount(item, commit = true) {
    const idx = this.items.indexOf(item);
    if (idx >= 0) this.items.splice(idx, 1);
    try {
      if (item.inst && typeof item.inst.destroy === "function") item.inst.destroy();
    } catch (e) {
      console.error(`组件 ${item.type} 卸载失败:`, e);
    }
    item.el.remove();
    if (commit) this.commit();
  }

  /** 把 items 的坐标写回 DOM（显式 grid 放置），并同步空状态提示。
   * 流式目标每次同步按 (y,x) 重排 DOM——App 与推送消息都按这个顺序消费，
   * 预览必须与真机一致（拖拽换位后 items 数组顺序不会自动变）。 */
  sync() {
    if (this.profile.mode === "flow") this.reorderByGrid();
    for (const it of this.items) {
      if (this.flowView) {
        it.el.style.gridColumn = `span ${it.w}`;
        it.el.style.gridRow = "auto";
      } else {
        it.el.style.gridColumn = `${it.x + 1} / span ${it.w}`;
        it.el.style.gridRow = `${it.y + 1} / span ${it.h}`;
      }
    }
    if (this.emptyHint) this.emptyHint.classList.toggle("hidden", this.items.length > 0);
  }

  /** 按 (y,x) 重排 items 与 DOM 顺序（flow 的阅读顺序 / 流式预览顺序）。 */
  reorderByGrid() {
    this.items.sort((a, b) => a.y - b.y || a.x - b.x);
    for (const it of this.items) this.grid.append(it.el);
  }

  /** 切换流式预览：只改渲染方式，不动坐标；预览时编辑交互由 editmode 关闭。 */
  setFlowView(on) {
    this.flowView = !!on;
    this.grid.classList.toggle("flow-view", this.flowView);
    this.sync();
  }

  /** 写 DOM + 触发 onchange（保存布局）。 */
  commit() {
    this.sync();
    if (this.onchange) this.onchange(this.getLayout());
  }

  /** 添加组件：落在第一个放得下默认尺寸的空位。固定一屏（profile.rows）找不到
   * 空位时返回 null，由调用方提示「一屏已满」，绝不追加到屏幕外。 */
  add(type) {
    const def = this.profile.def(type);
    if (!def || this.item(type)) return null;
    const spot = layout.findFreeSpot(
      this.getLayout(),
      def.defW,
      def.defH,
      this.profile.cols,
      this.profile.rows || null,
    );
    if (!spot) return null;
    return this.mount({ type, x: spot.x, y: spot.y, w: def.defW, h: def.defH });
  }

  remove(type) {
    const item = this.item(type);
    if (item) this.unmount(item);
  }

  /** 恢复默认布局（整组重挂，组件实例全部重建）。 */
  reset() {
    this.mountAll(this.profile.defaults());
  }

  /**
   * 编辑落位后调用：以 sanitize 的结果修正坐标（交换产生的残余重叠会被下移），
   * 再写回 DOM 并保存。固定一屏装不下全部组件时返回 false 且不改动当前布局，
   * 由 editmode 回滚本次拖拽——绝不静默丢卡。
   */
  settle() {
    const before = this.getLayout();
    const fixed = layout.sanitize(before, this.profile);
    if (this.profile.rows && fixed.length < before.length) return false;
    for (const rect of fixed) {
      const it = this.item(rect.type);
      if (it) Object.assign(it, rect);
    }
    this.commit();
    return true;
  }

  /** 渲染循环入口：逐组件分发快照（附该项的 h，手机预览卡按它切精简/详细）；
   * 单个组件抛错不拖垮整页。 */
  updateAll(snap, ctx) {
    for (const it of this.items) {
      try {
        it.inst.update(snap, { ...ctx, h: it.h });
      } catch (e) {
        console.error(`组件 ${it.type} 更新失败:`, e);
      }
    }
  }

  /** 采样间隔变更：转发给关心横轴长度的图表组件。 */
  setInterval(ms) {
    for (const it of this.items) {
      try {
        if (typeof it.inst.setInterval === "function") it.inst.setInterval(ms);
      } catch (e) {
        console.error(`组件 ${it.type} 调整采样间隔失败:`, e);
      }
    }
  }
}
