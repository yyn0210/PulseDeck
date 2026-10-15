// 环形进度组件的共享件：几何常量与「跳变才补间」的写值逻辑。
// 原来这坨在 main.js 模块级（ringPrev Map 按全局 id 记状态），组件化后跟着实例走。

export const RING_RADIUS = 52;
export const CIRCUMFERENCE = 2 * Math.PI * RING_RADIUS;

/* 补间阈值（百分点）：低于它的变化直接落值，不补间。
 *
 * stroke-dashoffset 不是可合成属性——补间期间浏览器每帧都要重画这个 SVG，
 * 实测 4 个环一起补间 ≈ 一个 CPU 核的 11%（同样工况下整页 1 秒一拍的全部开销约 2%）。
 * 而 1 秒一拍的采样里，环的读数通常只挪几度，那种幅度本来也看不出补间，
 * 所以只在「明显跳变」（打开大程序、跑分之类的突变）时才把补间放出来。 */
const RING_ANIM_MIN_DELTA = 8;

/** 造一个作用于 root 内指定环的 setRing(percent)。每实例一份 prev，避免跨组件串状态。 */
export function ringSetter(root, ringId) {
  const el = root.querySelector(`#${ringId}`);
  let prev = null;
  return (percent) => {
    if (!el) return;
    const p = Math.max(0, Math.min(100, percent || 0));
    const animate = prev == null || Math.abs(p - prev) >= RING_ANIM_MIN_DELTA;
    // 先定补间开关再写值：同一次样式重算里生效，"none" 会让本次更新立即到位
    el.style.transition = animate ? "" : "none";
    el.style.strokeDashoffset = String(CIRCUMFERENCE * (1 - p / 100));
    prev = p;
  };
}
