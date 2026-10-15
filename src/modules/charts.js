// 手绘 canvas 折线图（无第三方依赖），用于 CPU / 内存占用趋势

export class LineChart {
  /**
   * @param {HTMLCanvasElement} canvas
   * @param {{ series: {key:string,color:string,fill?:boolean}[], max?: number, ceiling?: number }} opts
   */
  constructor(canvas, opts) {
    this.canvas = canvas;
    this.ctx = canvas.getContext("2d");
    this.series = opts.series;
    this.max = opts.max ?? 180;
    this.ceiling = opts.ceiling ?? 100;
    /** 为 true 时纵轴量程跟随数据自动伸缩（网络速率这类量级不稳定的数据需要） */
    this.autoCeiling = opts.autoCeiling ?? false;
    this.minCeiling = opts.minCeiling ?? 1;
    this.smoothCeiling = this.minCeiling;
    this.data = [];
    this.dpr = 1;

    this._resize = () => this.resize();
    window.addEventListener("resize", this._resize);
    if (typeof ResizeObserver !== "undefined") {
      this.observer = new ResizeObserver(this._resize);
      this.observer.observe(canvas);
    }
    this.resize();
  }

  resize() {
    const rect = this.canvas.getBoundingClientRect();
    if (!rect.width || !rect.height) return;
    this.dpr = window.devicePixelRatio || 1;
    this.canvas.width = Math.round(rect.width * this.dpr);
    this.canvas.height = Math.round(rect.height * this.dpr);
    this.draw();
  }

  push(point) {
    this.data.push(point);
    if (this.data.length > this.max) this.data.shift();
  }

  /** 卸载：解掉全局监听与尺寸观察。组件被移除时必须调用，否则 observer 一直持有 canvas。 */
  destroy() {
    window.removeEventListener("resize", this._resize);
    if (this.observer) {
      this.observer.disconnect();
      this.observer = null;
    }
  }

  /**
   * 自适应量程：取当前窗口内的最大值留 25% 余量，并做阻尼平滑，
   * 避免纵轴在流量抖动时反复跳动。
   */
  computeCeiling() {
    let max = 0;
    for (const point of this.data) {
      for (const s of this.series) {
        const v = point[s.key];
        if (typeof v === "number" && Number.isFinite(v) && v > max) max = v;
      }
    }
    const target = Math.max(this.minCeiling, max * 1.25);
    this.smoothCeiling = this.smoothCeiling * 0.82 + target * 0.18;
    return this.smoothCeiling;
  }

  draw() {
    const { ctx, canvas } = this;
    if (!canvas.width || !canvas.height) return;
    const w = canvas.width;
    const h = canvas.height;
    const pad = 8 * this.dpr;
    const ceiling = this.autoCeiling ? this.computeCeiling() : this.ceiling;

    ctx.clearRect(0, 0, w, h);
    if (this.data.length < 2) return;

    ctx.save();
    ctx.scale(1, 1);

    // 横向网格线
    ctx.strokeStyle = "rgba(255,255,255,0.07)";
    ctx.lineWidth = 1;
    for (let i = 1; i < 4; i += 1) {
      const y = pad + ((h - pad * 2) * i) / 4;
      ctx.beginPath();
      ctx.moveTo(pad, y);
      ctx.lineTo(w - pad, y);
      ctx.stroke();
    }

    const stepX = (w - pad * 2) / (this.max - 1);
    const startX = w - pad - stepX * (this.data.length - 1);
    const yOf = (v) => {
      const clamped = Math.max(0, Math.min(ceiling, v));
      return pad + (1 - clamped / ceiling) * (h - pad * 2);
    };

    for (const s of this.series) {
      const pts = this.data.map((d, i) => [startX + i * stepX, yOf(d[s.key])]);

      if (s.fill) {
        const grad = ctx.createLinearGradient(0, pad, 0, h - pad);
        grad.addColorStop(0, `${s.color}44`);
        grad.addColorStop(1, `${s.color}00`);
        ctx.beginPath();
        ctx.moveTo(pts[0][0], h - pad);
        for (const [x, y] of pts) ctx.lineTo(x, y);
        ctx.lineTo(pts[pts.length - 1][0], h - pad);
        ctx.closePath();
        ctx.fillStyle = grad;
        ctx.fill();
      }

      ctx.beginPath();
      pts.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
      ctx.strokeStyle = s.color;
      ctx.lineWidth = 1.8 * this.dpr;
      ctx.lineJoin = "round";
      ctx.lineCap = "round";
      ctx.stroke();

      // 末端发光圆点
      const last = pts[pts.length - 1];
      ctx.beginPath();
      ctx.arc(last[0], last[1], 2.6 * this.dpr, 0, Math.PI * 2);
      ctx.fillStyle = s.color;
      ctx.fill();
    }

    ctx.restore();
  }
}
