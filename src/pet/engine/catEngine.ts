/**
 * Ported from KuroCat (https://github.com/qianyiai/kurocat), v5.1.0.
 * Copyright (c) 2026 KuroCat Contributors, MIT License.
 * See src-tauri/resources/licenses/KUROCAT-MIT.txt.
 */
import {
  KEYBOARD_KEYS_65,
  KEYBOARD_KEYS_108,
  getKeyboardLayout,
  type KeySpec,
  type KeyboardLayoutId,
  formatBubbleForVk,
  resolveKeyByVk,
} from './keyboardLayout';
import { catAudio } from './audio';
import { resolveCustomSkin, type CatSkinConfig, type ResolvedSkin } from './catSkins';
import { DaFeiYuRenderer, type DaFeiYuLayout } from './dafeiyuRenderer';

export const LOGICAL_W = 420;
export const LOGICAL_H = 440;
export type CatBreed = 'black' | 'ragdoll' | 'dafeiyu';
export type KeyboardOrientation = 'cat' | 'viewer';
export type TailKeyMode = 'smart' | 'custom' | 'edge' | 'off';

export const DEFAULT_TAIL_KEY_IDS: readonly string[] = ['enter', 'space', 'bs', 'esc'];

export const TAIL_KEY_PRESETS = {
  office: {
    label: '回车空格',
    ids: ['enter', 'space', 'bs', 'esc'] as readonly string[],
  },
  rightFn: {
    label: '方向/小键盘',
    ids: [
      'up', 'down', 'left', 'right', 'enter', 'bs', 'rshift',
      'ins', 'home', 'pgup', 'del', 'end', 'pgdn',
      'numlk', 'np_div', 'np_mul', 'np_sub', 'np_add', 'np_ent', 'np_dot',
      'np_0', 'np_1', 'np_2', 'np_3', 'np_4', 'np_5', 'np_6', 'np_7', 'np_8', 'np_9',
    ] as readonly string[],
  },
  edgeMods: {
    label: '外侧功能键',
    ids: ['esc', 'tab', 'caps', 'lshift', 'lctrl', 'rshift', 'enter', 'bs'] as readonly string[],
  },
} as const;

const EDGE_PROTECTED_WASD_IDS = new Set(['q', 'w', 'e', 'a', 's', 'd', 'z', 'x', 'c', 'space']);

interface Vec2 {
  x: number;
  y: number;
}

interface SmearFrame {
  sx: number;
  sy: number;
  px: number;
  py: number;
  alpha: number;
}

type PawMode = 'idle' | 'curled' | 'poke' | 'mouse';

interface PawState {
  side: 'left' | 'right';
  shoulderNormY: number; // vertical fraction along spine (0=ear tips, 1=base)
  shoulderHalfX: number;
  pos: Vec2;
  vel: Vec2;
  target: Vec2;
  mode: PawMode;
  idleWeight: number;
  curlWeight: number;
  pokeTime: number;
  impactSquash: number;
  lastActiveAt: number;
  smears: SmearFrame[];
}

interface KeyBubble {
  id: number;
  label: string;
  isSpace: boolean;
  x: number;
  targetX: number;
  y: number;
  targetY: number;
  rot: number;
  createdAt: number;
  lifetime: number;
}

interface KeyAnim {
  pressed: boolean;
  pressAmount: number;
  lastHitAt: number;
}

/**
 * 240Hz sub-stepped second-order damped harmonic spring.
 * Damping < 1 creates subtle organic weight & follow-through overshoot at any frame rate.
 */
class Spring {
  public x: number;
  public v = 0;
  public hz: number;
  public damping: number;

  constructor(initial = 0, hz = 2.4, damping = 0.72) {
    this.x = initial;
    this.hz = hz;
    this.damping = damping;
  }

  public step(target: number, dt: number): number {
    const w = 2 * Math.PI * this.hz;
    const n = Math.max(1, Math.ceil(dt * 240));
    const h = dt / n;
    for (let i = 0; i < n; i++) {
      this.v += (w * w * (target - this.x) - 2 * this.damping * w * this.v) * h;
      this.x += this.v * h;
    }
    return this.x;
  }

  public kick(dv: number): void {
    this.v += dv;
  }

  public set(val: number): void {
    this.x = val;
    this.v = 0;
  }
}

function parseHexRgb(color: string): [number, number, number] | null {
  const c = color.trim();
  const hex = c.match(/^#([0-9a-f]{3}|[0-9a-f]{6})$/i);
  if (hex) {
    let h = hex[1];
    if (h.length === 3) h = h[0] + h[0] + h[1] + h[1] + h[2] + h[2];
    return [
      parseInt(h.slice(0, 2), 16),
      parseInt(h.slice(2, 4), 16),
      parseInt(h.slice(4, 6), 16),
    ];
  }
  const rgb = c.match(/^rgba?\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)/i);
  if (rgb) {
    return [parseFloat(rgb[1]), parseFloat(rgb[2]), parseFloat(rgb[3])];
  }
  return null;
}

function blendHex(colorA: string, colorB: string, t: number): string {
  const a = parseHexRgb(colorA);
  const b = parseHexRgb(colorB);
  if (!a || !b) return colorA;
  const u = Math.max(0, Math.min(1, t));
  const r = Math.round(a[0] + (b[0] - a[0]) * u);
  const g = Math.round(a[1] + (b[1] - a[1]) * u);
  const bl = Math.round(a[2] + (b[2] - a[2]) * u);
  return `rgb(${r}, ${g}, ${bl})`;
}

function colorWithAlpha(color: string, alpha: number): string {
  const a = Math.max(0, Math.min(1, alpha));
  const c = color.trim();
  const hex = c.match(/^#([0-9a-f]{3}|[0-9a-f]{6})$/i);
  if (hex) {
    let h = hex[1];
    if (h.length === 3) h = h[0] + h[0] + h[1] + h[1] + h[2] + h[2];
    const r = parseInt(h.slice(0, 2), 16);
    const g = parseInt(h.slice(2, 4), 16);
    const b = parseInt(h.slice(4, 6), 16);
    return `rgba(${r}, ${g}, ${b}, ${a.toFixed(3)})`;
  }
  const rgb = c.match(/^rgba?\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)(?:\s*,\s*([\d.]+))?\s*\)$/i);
  if (rgb) {
    const baseA = rgb[4] !== undefined ? parseFloat(rgb[4]) : 1;
    return `rgba(${rgb[1]}, ${rgb[2]}, ${rgb[3]}, ${(baseA * a).toFixed(3)})`;
  }
  return c;
}

interface CatFxParticle {
  kind: 'bang' | 'dust' | 'z';
  x: number;
  y: number;
  vx: number;
  vy: number;
  size: number;
  createdAt: number;
  lifetime: number;
}

interface IdleFidget {
  kind: 'tilt' | 'ear' | 'slowBlink' | 'look' | 'tail';
  t0: number;
  dur: number;
  side: -1 | 1;
}

// Exact measured pixel silhouette half-width profile from original video 0.3s (compact chubby sitting cat!)
// t = 0.0 (ear tips) -> t = 1.0 (bottom base flush on keyboard)
const BODY_SLICES: Array<{ t: number; hw: number }> = [
  { t: 0.12, hw: 49 }, // forehead / ear base
  { t: 0.21, hw: 56 }, // eye line
  { t: 0.30, hw: 60 }, // chubby cheeks widest
  { t: 0.38, hw: 52 }, // gentle neck indent
  { t: 0.47, hw: 54 }, // upper chest
  { t: 0.57, hw: 62 }, // mid chest
  { t: 0.68, hw: 70 }, // plump belly
  { t: 0.79, hw: 73 }, // widest soft thighs!
  { t: 0.90, hw: 69 }, // lower thigh curve
  { t: 0.97, hw: 58 }, // tapers softly inward at base!
  { t: 1.00, hw: 46 }, // cozy bottom rest
];

const IDLE_RENDER_AFTER_MS = 3000;
const IDLE_RENDER_PAUSE_MS = 40;
const KEYBOARD_HIT_HALF_WIDTH_RATIO = 0.54;
const KEYBOARD_HIT_HALF_HEIGHT_RATIO = 0.56;

export class KuroCatEngine {
  private canvas: HTMLCanvasElement;
  private ctx: CanvasRenderingContext2D;
  private rafId: number | null = null;
  private idleFrameTimer: number | null = null;
  private isIdlePacing = false;
  private directRafPrevMs = 0;
  private vsyncMs = 16.67;
  private vsyncSamples = 0;
  private gameActive = false;
  private kbCacheCanvas: HTMLCanvasElement | null = null;
  private kbCacheKey = '';
  private lastActivityAt = 0;
  private lastTime = 0;
  private elapsed = 0;

  public showKeyBubbles = true;
  public privacyMode = false;
  public onAfterRender: (() => void) | null = null;
  public catBreed: CatBreed = 'black';
  public customSkin: ResolvedSkin | null = null;
  public keyboardLayout: KeyboardLayoutId = '65';
  public keyboardOrientation: KeyboardOrientation = 'cat';
  public tailKeyMode: TailKeyMode = 'smart';
  private tailKeyIdSet = new Set<string>(DEFAULT_TAIL_KEY_IDS);
  private tailPickerActive = false;
  private tailPickerSelection: ReadonlySet<string> | null = null;
  private tailPickerHoverId: string | null = null;
  private renderScale = 1.0;
  private embedded = false;
  private dafeiyu = new DaFeiYuRenderer();
  private readonly handleWindowResize = () => this.resize();
  private catTail = {
    heldVks: new Set<number>(),
    targetX: 320,
    targetY: 345,
    tipX: 320,
    tipY: 250,
    reach: 0,
    hover: 0,
    tapAt: -10000,
    releaseAt: -10000,
    impactSquash: 0,
    active: false,
  };
  private privacyClose = 0;
  private privacyTypingAt = -10000;
  private lastMouseNX = 0.5;
  private lastDt = 1 / 60;

  public setPrivacyMode(active: boolean): void {
    this.wakeRendering();
    this.privacyMode = active;
    this.dafeiyu.onPrivacyModeChanged(active);
    if (active) {
      this.heldKeys.clear();
      this.catTail.heldVks.clear();
      this.catTailHeldAt.clear();
      this.catTail.active = false;
      this.bubbles.length = 0;
      for (const anim of this.keyAnims.values()) {
        anim.pressed = false;
        anim.pressAmount = 0;
      }
    }
  }

  public onPrivacyTyping(): void {
    this.privacyTypingAt = performance.now();
    this.wakeRendering(this.privacyTypingAt);
    if (this.catBreed === 'dafeiyu') {
      this.dafeiyu.onPrivacyTyping();
    }
  }

  public setRenderScale(scale: number): void {
    this.renderScale = Math.max(0.2, Math.min(1.5, scale));
    this.wakeRendering();
  }

  public getLogicalSize(): { w: number; h: number } {
    return { w: this.logicalW, h: this.logicalH };
  }

  /** 调试/回归用：当前按住键数与爪子/鼠标状态（不参与渲染） */
  public debugState(): {
    pressed: number;
    leftPaw: string;
    rightPaw: string;
    mouseLeft: boolean;
    mouseRight: boolean;
    hover: boolean;
    squint: number;
  } {
    let pressed = 0;
    for (const anim of this.keyAnims.values()) {
      if (anim.pressed) pressed += 1;
    }
    return {
      pressed,
      leftPaw: this.leftPaw.mode,
      rightPaw: this.rightPaw.mode,
      mouseLeft: this.mouseLeftDown,
      mouseRight: this.mouseRightDown,
      hover: this.hoverPet,
      squint: this.squintAmount,
    };
  }

  public setTailKeyConfig(mode: TailKeyMode, keyIds: readonly string[]): void {
    const valid: TailKeyMode =
      mode === 'custom' || mode === 'edge' || mode === 'off' ? mode : 'smart';
    this.tailKeyMode = valid;
    this.tailKeyIdSet = new Set(
      (Array.isArray(keyIds) ? keyIds : []).map((id) => String(id).trim().toLowerCase()).filter(Boolean),
    );
    if (this.tailKeyMode === 'off') {
      this.catTail.active = false;
      this.catTail.heldVks.clear();
      this.catTailHeldAt.clear();
    }
    this.wakeRendering();
  }

  public setTailPickerActive(active: boolean, selection?: ReadonlySet<string>): void {
    this.tailPickerActive = Boolean(active);
    this.tailPickerSelection = this.tailPickerActive ? selection ?? null : null;
    if (!this.tailPickerActive) {
      this.tailPickerHoverId = null;
    }
    this.wakeRendering();
  }

  public setTailPickerHover(specId: string | null): void {
    if (this.tailPickerHoverId !== specId) {
      this.tailPickerHoverId = specId;
      this.wakeRendering();
    }
  }

  private getTailTargetPoint(spec: KeySpec): Vec2 {
    if (spec.id === 'space') {
      const isCatFacing = this.keyboardOrientation === 'cat';
      const spaceU = isCatFacing
        ? spec.uStart + spec.uWidth * 0.24
        : spec.uStart + spec.uWidth * 0.76;
      const rY = spec.rowY ?? spec.row;
      return this.clampToKeyboardSurface(this.projectKeyboardPoint(spaceU, rY + 0.50));
    }
    return this.clampToKeyboardSurface(this.getKeyWorldCenter(spec));
  }

  public previewTailTap(specId: string): void {
    const layout = getKeyboardLayout(this.keyboardLayout);
    const spec = layout.keys.find((k) => k.id === specId);
    if (!spec) return;
    const now = performance.now();
    this.wakeCat(now);
    const anim = this.keyAnims.get(spec.id);
    if (anim) {
      anim.pressAmount = 1;
      anim.lastHitAt = now;
    }
    const targetPt = this.getTailTargetPoint(spec);
    if (this.catBreed === 'dafeiyu') {
      this.dafeiyu.triggerTailPreview(targetPt, this.buildDaFeiYuLayout());
      return;
    }
    if (!this.catTail.active && this.catTail.reach < 0.08) {
      const hipRight = this.spinePoint(0.84, 0);
      this.catTail.tipX = hipRight.x + 126;
      this.catTail.tipY = hipRight.y - 37;
    }
    this.catTail.active = true;
    this.catTail.targetX = targetPt.x;
    this.catTail.targetY = targetPt.y;
    this.catTail.tapAt = now;
    this.catTail.releaseAt = now + 220;
    this.catTail.impactSquash = 1.0;
    this.kbBounce = Math.min(1, this.kbBounce + 0.35);
    this.squashSpring.kick(0.9);
  }

  private getViewportTransform(): { offsetX: number; offsetY: number; uniformScale: number } {
    const cw = this.canvas.width;
    const ch = this.canvas.height;
    if (this.embedded) {
      const baseFit = Math.min((cw * 0.86) / this.logicalW, (ch * 0.86) / this.logicalH);
      const effectiveMul =
        this.renderScale <= 1.0 ? this.renderScale : 1.0 + (this.renderScale - 1.0) * 0.30;
      const uniformScale = baseFit * effectiveMul;
      const drawW = this.logicalW * uniformScale;
      const drawH = this.logicalH * uniformScale;
      return {
        offsetX: (cw - drawW) * 0.5,
        offsetY: (ch - drawH) * 0.52,
        uniformScale,
      };
    }
    const cssW = Math.max(1, window.innerWidth);
    const dpr = cw / cssW;
    const fitScale = Math.min(cw / this.logicalW, ch / this.logicalH);
    const uniformScale = Math.min(fitScale, this.renderScale * dpr);
    const drawW = this.logicalW * uniformScale;
    const drawH = this.logicalH * uniformScale;
    return {
      offsetX: cw - drawW,
      offsetY: ch - drawH,
      uniformScale,
    };
  }

  public getKeyboardClientBounds(): DOMRect {
    const rect = this.canvas.getBoundingClientRect();
    const { offsetX, offsetY, uniformScale } = this.getViewportTransform();
    const scaleX = rect.width / this.canvas.width;
    const scaleY = rect.height / this.canvas.height;
    const sin = Math.abs(Math.sin(this.kbTiltRad));
    const cos = Math.abs(Math.cos(this.kbTiltRad));
    const halfWidth = this.kbWidth * KEYBOARD_HIT_HALF_WIDTH_RATIO;
    const halfHeight = this.kbHeight * KEYBOARD_HIT_HALF_HEIGHT_RATIO;
    const boundX = (halfWidth * cos + halfHeight * sin) * uniformScale;
    const boundY = (halfWidth * sin + halfHeight * cos) * uniformScale;
    const centerX = offsetX + this.kbCenterX * uniformScale;
    const centerY = offsetY + this.kbCenterY * uniformScale;
    return new DOMRect(
      rect.left + (centerX - boundX) * scaleX,
      rect.top + (centerY - boundY) * scaleY,
      2 * boundX * scaleX,
      2 * boundY * scaleY,
    );
  }

  public hitTestKeyAtClientPoint(clientX: number, clientY: number): KeySpec | null {
    const rect = this.canvas.getBoundingClientRect();
    if (rect.width < 2 || rect.height < 2) return null;
    const scaleX = this.canvas.width / rect.width;
    const scaleY = this.canvas.height / rect.height;
    const canvasX = (clientX - rect.left) * scaleX;
    const canvasY = (clientY - rect.top) * scaleY;
    const { offsetX, offsetY, uniformScale } = this.getViewportTransform();
    if (uniformScale <= 0.001) return null;

    const lx = (canvasX - offsetX) / uniformScale;
    const ly = (canvasY - offsetY) / uniformScale;

    const dx = lx - this.kbCenterX;
    const dy = ly - (this.kbCenterY + this.kbBounce * 1.1 + this.deskHopY);
    const cos = Math.cos(-this.kbTiltRad);
    const sin = Math.sin(-this.kbTiltRad);
    const kx = dx * cos - dy * sin;
    const ky = dx * sin + dy * cos;

    const padX = 7;
    const padY = 6;
    const innerW = this.kbWidth - padX * 2;
    const innerH = this.kbHeight - padY * 2;
    if (
      Math.abs(kx) > this.kbWidth * KEYBOARD_HIT_HALF_WIDTH_RATIO ||
      Math.abs(ky) > this.kbHeight * KEYBOARD_HIT_HALF_HEIGHT_RATIO
    ) {
      return null;
    }

    const layout = getKeyboardLayout(this.keyboardLayout);
    const isCatFacing = this.keyboardOrientation === 'cat';
    const is108 = this.keyboardLayout === '108';
    const rowH = innerH / layout.totalRows;
    const unitW = innerW / layout.totalUnits;
    const gap = is108 ? 0.85 : 0.9;
    const hitPad = 0.75;

    for (const key of layout.keys) {
      const rY = key.rowY ?? key.row;
      const rSpan = key.rowSpan ?? 1;
      const screenRowTop = isCatFacing ? layout.totalRows - (rY + rSpan) : rY;
      const screenUStart = isCatFacing
        ? layout.totalUnits - (key.uStart + key.uWidth)
        : key.uStart;
      const rowCenterFrac = (screenRowTop + rSpan * 0.5) / layout.totalRows;
      const depthScale = 0.94 + rowCenterFrac * 0.09;

      const rawX = -innerW / 2 + screenUStart * unitW + gap * 0.5;
      const rawW = key.uWidth * unitW - gap;
      const x = rawX * depthScale;
      const w = rawW * depthScale;
      const y = -innerH / 2 + screenRowTop * rowH + gap * 0.5;
      const h = rSpan * rowH - gap - 0.8;

      if (
        kx >= x - hitPad &&
        kx <= x + w + hitPad &&
        ky >= y - hitPad &&
        ky <= y + h + hitPad
      ) {
        return key;
      }
    }
    return null;
  }

  private shouldUseTailForKey(
    spec: KeySpec,
    targetPoint: Vec2,
    activelyMousing: boolean,
    prevAnyKeyAt: number,
    now: number,
  ): boolean {
    if (this.tailKeyMode === 'off') return false;
    if (this.tailKeyMode === 'custom') {
      return this.tailKeyIdSet.has(spec.id);
    }
    if (this.tailKeyMode === 'edge') {
      if (EDGE_PROTECTED_WASD_IDS.has(spec.id)) return false;
      const edgeThresholdX =
        this.keyboardLayout === '108'
          ? this.kbCenterX + this.kbWidth * 0.22
          : this.kbCenterX + this.kbWidth * 0.34;
      return targetPoint.x >= edgeThresholdX;
    }

    // 'smart' (第三只手 · 智能忙碌时出尾):
    // 1) Both hands occupied -> tail leaps in as a real third hand for any extra key!
    let leftHeld = 0;
    let rightHeld = 0;
    let rightBusy = false;
    if (this.catBreed === 'dafeiyu') {
      const busy = this.dafeiyu.getHandBusyState(now);
      leftHeld = busy.leftHeld;
      rightHeld = busy.rightHeld;
      rightBusy = busy.rightBusy;
    } else {
      for (const h of this.heldKeys.values()) {
        if (h.side === 'left') leftHeld += 1;
        else rightHeld += 1;
      }
      rightBusy =
        rightHeld > 0 || (this.rightPaw.mode === 'poke' && now - this.rightPaw.lastActiveAt < 135);
    }

    const thirdHandBusy =
      (activelyMousing && rightBusy) ||
      (leftHeld > 0 && rightHeld > 0) ||
      leftHeld + rightHeld >= 2;
    if (thirdHandBusy) return true;

    // 2) Satisfying punctuation flourish: tail slaps Enter / Numpad Enter right after typing a burst!
    if (
      (spec.id === 'enter' || spec.id === 'np_ent') &&
      (this.typingEnergy > 0.24 || (now - prevAnyKeyAt < 450 && this.recentKeyTimes.length >= 2))
    ) {
      return true;
    }
    return false;
  }

  private syncLayoutPositions(): void {
    const isBigFish = this.catBreed === 'dafeiyu';
    if (this.keyboardLayout === '108') {
      this.logicalW = 540;
      this.logicalH = 456;
      this.catCenterX = 278;
      this.catTopY = 66;
      this.catBaseY = 292;
      this.kbCenterX = 290;
      this.kbCenterY = 356;
      this.kbWidth = 398;
      this.kbHeight = 96;
      this.kbTiltRad = (2.4 * Math.PI) / 180;
      this.mouseBaseX = isBigFish ? 54 : 46;
      this.mouseBaseY = isBigFish ? 332 : 342;
    } else {
      this.logicalW = 420;
      this.logicalH = 440;
      this.catCenterX = 196;
      this.catTopY = 82;
      this.catBaseY = 308;
      this.kbCenterX = isBigFish ? 234 : 214;
      this.kbCenterY = 353;
      this.kbWidth = 266;
      this.kbHeight = 74;
      this.kbTiltRad = (3.4 * Math.PI) / 180;
      this.mouseBaseX = isBigFish ? 72 : 54;
      this.mouseBaseY = isBigFish ? 322 : 334;
    }
  }

  public setCatAppearance(breedId: string, skin?: CatSkinConfig | null): void {
    if (breedId === 'dafeiyu') {
      this.catBreed = 'dafeiyu';
      this.customSkin = null;
      catAudio.setPurrIntensity(0);
    } else if ((breedId === 'black' || breedId === 'ragdoll') && (!skin || skin.builtin)) {
      this.catBreed = breedId;
      this.customSkin = null;
    } else if (skin) {
      this.catBreed = 'black';
      this.customSkin = resolveCustomSkin(skin);
    } else {
      this.catBreed = 'black';
      this.customSkin = null;
    }
    this.syncLayoutPositions();
    this.wakeRendering();
  }

  public setHeadphones(active: boolean): void {
    const next = Boolean(active);
    if (next !== this.headphonesOn) {
      this.headphonesOn = next;
      if (next) {
        this.headsetSpring.v += 2.2;
      }
      this.wakeRendering();
    }
  }

  public setKeyboardConfig(layout: KeyboardLayoutId, orientation: KeyboardOrientation): void {
    const nextLayout: KeyboardLayoutId = layout === '108' ? '108' : '65';
    const nextOrientation: KeyboardOrientation = orientation === 'viewer' ? 'viewer' : 'cat';
    const layoutChanged = this.keyboardLayout !== nextLayout;
    this.keyboardLayout = nextLayout;
    this.keyboardOrientation = nextOrientation;

    this.syncLayoutPositions();

    if (layoutChanged) {
      this.heldKeys.clear();
      this.catTail.heldVks.clear();
      this.catTailHeldAt.clear();
      this.leftPaw.pos = { x: this.catCenterX - 18, y: this.catBaseY };
      this.leftPaw.target = { x: this.catCenterX - 18, y: this.catBaseY };
      this.rightPaw.pos = { x: this.catCenterX + 18, y: this.catBaseY };
      this.rightPaw.target = { x: this.catCenterX + 18, y: this.catBaseY };
    }
    this.wakeRendering();
  }

  private buildDaFeiYuLayout(): DaFeiYuLayout {
    const mx = this.mouseBaseX + this.mouseOffsetX;
    const my = this.mouseBaseY + this.mouseOffsetY;
    const isCatFacing = this.keyboardOrientation === 'cat';
    const localBtnX = this.mouseLeftDown ? -6.5 : this.mouseRightDown ? 6.5 : 0;
    const localBtnY = -10.5;
    const ang = this.kbTiltRad + (isCatFacing ? Math.PI : 0);
    const c = Math.cos(ang);
    const s = Math.sin(ang);
    return {
      logicalW: this.logicalW,
      kbCenterX: this.kbCenterX,
      kbCenterY: this.kbCenterY,
      kbWidth: this.kbWidth,
      kbHeight: this.kbHeight,
      kbTiltRad: this.kbTiltRad,
      mouseBtnX: mx + localBtnX * c - localBtnY * s,
      mouseBtnY: my + localBtnX * s + localBtnY * c,
      mouseNX: this.lastMouseNX,
      mouseLeft: this.mouseLeftDown,
      mouseRight: this.mouseRightDown,
      mouseActiveAt: this.lastMouseMoveAt,
      mouseClickAt: this.lastMouseClickAt,
      petEnergy: this.petEnergy,
      hoverSquint: this.hoverPet,
      privacyMode: this.privacyMode,
      privacyTypingAt: this.privacyTypingAt,
      is108: this.keyboardLayout === '108',
    };
  }

  // Dynamic logical proportions (65% compact = 420x440; 108-key full-size = 540x456)
  private logicalW = 420;
  private logicalH = 440;
  private catCenterX = 196;
  private catTopY = 82;
  private catBaseY = 308;
  private readonly catHeight = 226;

  // Mechanical keyboard deck below the sitting cat
  private kbCenterX = 214;
  private kbCenterY = 353;
  private kbWidth = 266;
  private kbHeight = 74;
  private kbTiltRad = (3.4 * Math.PI) / 180;

  // True-scale white oval mouse to the left of keyboard
  private mouseBaseX = 54;
  private mouseBaseY = 334;
  private mouseOffsetX = 0;
  private mouseOffsetY = 0;
  private mouseTargetOffsetX = 0;
  private mouseTargetOffsetY = 0;
  private mouseLeftDown = false;
  private mouseRightDown = false;
  private mouseMiddleDown = false;
  private mouseDownAt: Record<'left' | 'right' | 'middle', number> = {
    left: 0,
    right: 0,
    middle: 0,
  };
  private mouseLeftPress = 0;
  private mouseRightPress = 0;
  private mouseMiddlePress = 0;
  private lastMouseClickAt = -10000;
  private mouseScrollFlash = 0;
  private lastMouseMoveAt = -10000;
  private lastMouseNY = 0.5;
  private lastCursorEyeAt = -10000;
  private mouseBadges: Array<{
    id: number;
    label: string;
    color: string;
    x: number;
    y: number;
    createdAt: number;
    lifetime: number;
  }> = [];

  // Cursor & 100% live eye tracking + 2.5D head turn
  private cursorRelX = 0.5;
  private cursorRelY = 0.5;
  private cursorOverCat = false;
  /** PulseDeck：悬停摸头（鼠标停在猫身上即眯眼，不要求来回抚摸；见 src/pet/cat.js） */
  private hoverPet = false;
  private eyeLookX = 0;
  private eyeLookY = 0;
  private faceTurnX = 0;

  // Staggered 240Hz spring hierarchy: eyes fastest (9.5Hz near-critically damped -> liquid silky!) -> ears (5.2Hz) -> head (2.6Hz) -> spine (1.9Hz)
  private eyeXSpring = new Spring(0, 9.5, 0.96);
  private eyeYSpring = new Spring(0, 9.5, 0.96);
  private earPerkSpring = new Spring(0, 5.2, 0.36);
  private headTiltSpring = new Spring(0, 2.6, 0.62);
  private faceTurnSpring = new Spring(0, 2.9, 0.74);
  private spineLeanSpring = new Spring(0, 1.9, 0.72);
  private hunchSpring = new Spring(0, 2.0, 0.68);
  private squashSpring = new Spring(0, 2.6, 0.54);
  private curiousSpring = new Spring(0, 3.8, 0.72);
  private crossSpring = new Spring(0, 1.9, 0.86);
  private deskHopSpring = new Spring(0, 9.2, 0.34);
  private headsetSpring = new Spring(0, 5.4, 0.70);
  private headphonesOn = false;

  // Alive cat emotional state machine: curious (laser-pointer eyes), sleepy loaf (Zzz), idle fidgets, cross (airplane ears + tail desk-thump!)
  private lastInputAt = performance.now();
  private asleep = false;
  private sleepAmount = 0;
  private wakeAt = -10000;
  private nextZAt = 0;
  private pointerSamples: Array<{ t: number; dist: number }> = [];
  private lastPointerSampleX: number | null = null;
  private lastPointerSampleY: number | null = null;
  private lastPointerSampleAt = -10000;
  private curiousUntil = -10000;
  private curiousAmount = 0;
  private annoy = 0;
  private lastPokeAt = -10000;
  private isCross = false;
  private crossSince = -10000;
  private crossAmount = 0;
  private crossHuffDir: -1 | 1 = -1;
  private thumpPhase = 0;
  private thumpLift = 0;
  private thumpLash = 0;
  private lastSlamAt = -10000;
  private deskHopY = 0;
  private idleAct: IdleFidget | null = null;
  private nextIdleActAt = performance.now() + 4500;
  private fxParticles: CatFxParticle[] = [];

  // Spine-shear deformation state (exact measured flexible cat spine + playful typing body/head sway!)
  private spineLean = 0;    // -1 (mouse lean: head shifts -46px left while hips stay anchored!) .. +1
  private headTilt = 0;
  private typingHunch = 0;  // 0..1 lowers head & rounds posture slightly when typing
  private typingSway = 0;   // -1..+1 lively left-right torso & head sway while typing!
  private typingSwayVel = 0;
  private typingGroovePhase = 0;
  private lastKeyBias = 0;
  private strikeRock = 0;
  private strikeRockVel = 0;
  private bodyBob = 0;
  private kbBounce = 0;

  // Blinking, petting, ear micro-twitch, typing rhythm
  private nextBlinkAt = 2.8;
  private blinkProgress = 0;
  private isBlinking = false;

  private lastPetCursorX: number | null = null;
  private lastPetCursorY: number | null = null;
  private petAnchorX: number | null = null;
  private petAnchorY: number | null = null;
  private petStrokeDirX = 0;
  private petStrokeDirY = 0;
  private petStrokeTravel = 0;
  private petReverseTravel = 0;
  private petStrokeCount = 0;
  private lastPetMoveAt = -10000;
  private petEnergy = 0;
  private squintAmount = 0;
  private earTwitchLeft = 0;
  private earTwitchRight = 0;
  private recentKeyTimes: number[] = [];
  private typingEnergy = 0;
  private lastAnyKeyAt = -10000;

  // Paws & continuous held-key tracking (Fixes #3)
  private leftPaw: PawState;
  private rightPaw: PawState;
  private lastSpacePaw: 'left' | 'right' = 'left';
  private heldKeys = new Map<
    number,
    { side: 'left' | 'right'; specId: string; x: number; y: number; at: number }
  >();
  private catTailHeldAt = new Map<number, number>();

  // Keys & bubbles
  private keyAnims = new Map<string, KeyAnim>();
  private bubbles: KeyBubble[] = [];
  private nextBubbleId = 1;

  constructor(canvas: HTMLCanvasElement, options?: { embedded?: boolean }) {
    this.canvas = canvas;
    this.embedded = Boolean(options?.embedded);
    const ctx = canvas.getContext('2d');
    if (!ctx) throw new Error('Could not initialize 2D canvas context');
    this.ctx = ctx;

    for (const k of KEYBOARD_KEYS_65) {
      this.keyAnims.set(k.id, { pressed: false, pressAmount: 0, lastHitAt: -10000 });
    }
    for (const k of KEYBOARD_KEYS_108) {
      if (!this.keyAnims.has(k.id)) {
        this.keyAnims.set(k.id, { pressed: false, pressAmount: 0, lastHitAt: -10000 });
      }
    }

    this.leftPaw = {
      side: 'left',
      shoulderNormY: 0.65,
      shoulderHalfX: -43,
      pos: { x: this.catCenterX - 18, y: this.catBaseY },
      vel: { x: 0, y: 0 },
      target: { x: this.catCenterX - 18, y: this.catBaseY },
      mode: 'idle',
      idleWeight: 1,
      curlWeight: 0,
      pokeTime: 1,
      impactSquash: 0,
      lastActiveAt: -10000,
      smears: [],
    };

    this.rightPaw = {
      side: 'right',
      shoulderNormY: 0.65,
      shoulderHalfX: 43,
      pos: { x: this.catCenterX + 18, y: this.catBaseY },
      vel: { x: 0, y: 0 },
      target: { x: this.catCenterX + 18, y: this.catBaseY },
      mode: 'idle',
      idleWeight: 1,
      curlWeight: 0,
      pokeTime: 1,
      impactSquash: 0,
      lastActiveAt: -10000,
      smears: [],
    };

    this.resize();
    window.addEventListener('resize', this.handleWindowResize);
  }

  public resize(): void {
    const dpr = Math.max(window.devicePixelRatio || 1, 1);
    const cssW = Math.max(1, this.embedded ? (this.canvas.clientWidth || 420) : window.innerWidth);
    const cssH = Math.max(1, this.embedded ? (this.canvas.clientHeight || 440) : window.innerHeight);
    this.canvas.width = Math.round(cssW * dpr);
    this.canvas.height = Math.round(cssH * dpr);
    this.wakeRendering();
  }

  public start(): void {
    if (this.rafId !== null || this.idleFrameTimer !== null) return;
    this.lastTime = this.lastActivityAt = performance.now();
    this.directRafPrevMs = 0;
    this.isIdlePacing = false;
    this.rafId = requestAnimationFrame(this.renderFrame);
  }

  public stop(): void {
    if (this.rafId !== null) cancelAnimationFrame(this.rafId);
    if (this.idleFrameTimer !== null) window.clearTimeout(this.idleFrameTimer);
    this.rafId = null;
    this.idleFrameTimer = null;
    this.isIdlePacing = false;
  }

  /** 卸载：停止渲染循环并移除 window resize 监听（PulseDeck 切换形象/引擎时调用） */
  public dispose(): void {
    this.stop();
    window.removeEventListener('resize', this.handleWindowResize);
  }

  public setGameActive(active: boolean): void {
    this.gameActive = Boolean(active);
  }

  /**
   * PulseDeck：悬停摸头开关。
   *
   * 原版 KuroCat 只认「来回抚摸」（petEnergy 需要两次反向行程），验收要求
   * 「鼠标移到猫头区域即眯眼」，所以由 `src/pet/cat.js` 在 overCat 时直接置位；
   * 呼噜仍由 petEnergy/抚摸强度驱动（不强求悬停就呼噜）。
   */
  public setHover(active: boolean): void {
    const next = Boolean(active);
    if (next !== this.hoverPet) {
      this.hoverPet = next;
      this.wakeRendering();
    }
  }

  private readonly renderFrame = (now: number): void => {
    const frameId = this.rafId;
    const workStart = performance.now();

    if (this.directRafPrevMs > 0 && this.vsyncSamples < 8) {
      const gap = now - this.directRafPrevMs;
      if (gap >= 3.2 && gap <= 25) {
        this.vsyncMs = this.vsyncSamples === 0 ? gap : Math.min(this.vsyncMs, gap);
        this.vsyncSamples += 1;
      }
    }

    const dt = Math.min(0.05, Math.max(0.001, (now - this.lastTime) / 1000));
    this.lastTime = now;
    this.update(dt, now);
    this.render();
    this.onAfterRender?.();

    if (this.rafId !== frameId) return;
    const active = now - this.lastActivityAt < IDLE_RENDER_AFTER_MS;

    // Smart adaptive frame pacing:
    // - 60Hz normal mode: native direct rAF
    // - 120-240Hz normal mode: 60 FPS cap
    // - Game Eco Mode (foreground PC game detected): 30 FPS cap
    // - Idle awake: ~18 FPS (55ms); Deep sleep: ~12 FPS (85ms)
    if (active && (this.vsyncSamples < 8 || (!this.gameActive && this.vsyncMs >= 13.5))) {
      this.isIdlePacing = false;
      this.directRafPrevMs = now;
      this.rafId = requestAnimationFrame(this.renderFrame);
      return;
    }

    this.directRafPrevMs = 0;
    const workMs = performance.now() - workStart;
    const targetMs = active
      ? this.gameActive
        ? 33.33
        : 16.67
      : this.sleepAmount > 0.95
        ? 85
        : Math.max(IDLE_RENDER_PAUSE_MS, 55);
    const sleepMs = Math.max(4, Math.round(targetMs - workMs - this.vsyncMs * 0.35));

    // Pause after drawing so even a slow frame leaves CPU headroom on older hardware (Fixes #9).
    this.rafId = null;
    this.isIdlePacing = !active;
    this.idleFrameTimer = window.setTimeout(() => {
      this.idleFrameTimer = null;
      this.isIdlePacing = false;
      this.rafId = requestAnimationFrame(this.renderFrame);
    }, sleepMs);
  };

  private wakeRendering(now = performance.now()): void {
    this.lastActivityAt = now;
    if (this.idleFrameTimer === null || !this.isIdlePacing) return;
    window.clearTimeout(this.idleFrameTimer);
    this.idleFrameTimer = null;
    this.isIdlePacing = false;
    this.rafId = requestAnimationFrame(this.renderFrame);
  }

  private pokeAnnoy(amount: number, now: number): void {
    if (this.catBreed === 'dafeiyu' || this.privacyMode) return;
    this.annoy = Math.max(0, Math.min(1, this.annoy + amount));
    this.lastPokeAt = now;
  }

  private wakeCat(now = performance.now()): void {
    this.wakeRendering(now);
    if (this.asleep || this.sleepAmount > 0.35) {
      this.wakeAt = now;
      this.pokeAnnoy(0.28, now);
    }
    this.asleep = false;
    this.lastInputAt = now;
    this.idleAct = null;
    this.nextIdleActAt = now + 4200 + Math.random() * 4200;
  }

  private recordPointerSpeed(nx: number, ny: number, now: number): void {
    if (this.catBreed === 'dafeiyu' || this.privacyMode) return;
    const gapMs = now - this.lastPointerSampleAt;
    if (
      this.lastPointerSampleX !== null &&
      this.lastPointerSampleY !== null &&
      gapMs > 0 &&
      gapMs <= 140
    ) {
      const dist = Math.hypot(nx - this.lastPointerSampleX, ny - this.lastPointerSampleY);
      if (dist > 0.0015 && dist < 0.45) {
        this.pointerSamples.push({ t: now, dist });
      }
    }
    this.lastPointerSampleX = nx;
    this.lastPointerSampleY = ny;
    this.lastPointerSampleAt = now;
  }

  /** Dedicated pointer-speed feed (e.g., full-window CSS-pixel/1440 space on web preview) for laser-pointer Curious mode. */
  public onPointerSpeedSample(sx: number, sy: number): void {
    const now = performance.now();
    this.recordPointerSpeed(sx, sy, now);
  }

  /** Interactive demo trigger: immediately shows laser-pointer curious big round pupils & perked ears! */
  public triggerCuriousDemo(durationMs = 3500): void {
    const now = performance.now();
    this.wakeCat(now);
    this.isCross = false;
    this.annoy = 0;
    this.curiousUntil = now + durationMs;
    this.curiousSpring.kick(2.4);
    this.earPerkSpring.kick(2.2);
  }

  /** Interactive demo trigger: immediately shows airplane ears + 3-beat grumpy tail desk-thump! */
  public triggerCrossDemo(): void {
    const now = performance.now();
    this.wakeCat(now);
    this.curiousUntil = -10000;
    this.annoy = 0.95;
    this.lastPokeAt = now;
    this.isCross = true;
    this.crossSince = now;
    this.crossHuffDir = this.cursorRelX >= 0.5 ? -1 : 1;
    this.thumpPhase = 0;
  }

  /** Interactive demo trigger: immediately curls into sleepy loaf with floating Zzz! */
  public triggerSleepDemo(): void {
    const now = performance.now();
    this.wakeRendering(now);
    this.isCross = false;
    this.annoy = 0;
    this.curiousUntil = -10000;
    this.idleAct = null;
    this.asleep = true;
    this.lastInputAt = now - 90_000;
    this.nextZAt = now + 400;
  }

  // ---- Flexible spine coordinate system (exact match to measured 0.3s -> 2.2s shear!) ----

  private spinePoint(t: number, breath: number): Vec2 {
    // t in [0..1]: 0 = ear tips, 1 = seated base on desk
    const u = Math.max(0, Math.min(1, 1 - t)); // 0 at hips, 1 at head
    const shearCurve = u * u * (3 - 2 * u); // smoothstep from hips up to head!
    // Measured mouse-lean shift (-46px at head) + lively typing left-right body/head sway (+-25px)!
    const leanAmp = this.keyboardLayout === '108' ? 76 : 46;
    const leanX = (this.spineLean * leanAmp + this.typingSway * 25 + this.strikeRock * 10) * shearCurve;
    const swayDipY = Math.abs(this.typingSway) * 4.0 * shearCurve;
    const sleepSinkY = this.sleepAmount * 16.5 * shearCurve;
    const squashDipY = this.squashSpring.x * 6.5 * shearCurve;
    const hunchY = (this.typingHunch * 13 + this.bodyBob * 3.2 + breath) * shearCurve + swayDipY + sleepSinkY + squashDipY;
    // Slight forward drop when leaning deeply to the mouse
    const leanDropY = Math.abs(this.spineLean) * 6 * shearCurve;
    return {
      x: this.catCenterX + leanX,
      y: this.catTopY + t * this.catHeight + hunchY + leanDropY,
    };
  }

  private getLiveShoulder(paw: PawState, breath: number): Vec2 {
    const sp = this.spinePoint(paw.shoulderNormY, breath);
    return {
      x: sp.x + paw.shoulderHalfX,
      y: sp.y,
    };
  }

  private getLiveCurledPawPos(paw: PawState, breath: number): Vec2 {
    // Bent forearm elbow -> cute curled paw sitting near lower belly (matches 8.3s / 10.6s!)
    const sp = this.spinePoint(0.84, breath);
    const bob = Math.sin(this.elapsed * 12 + (paw.side === 'left' ? 0 : Math.PI)) * (1.5 + this.typingEnergy * 2.0);
    const xOff = paw.side === 'left' ? -24 : 24;
    return {
      x: sp.x + xOff,
      y: sp.y + bob,
    };
  }

  // ---- Input event handlers ----

  public onKeyDown(vk: number, repeat: boolean): void {
    if (this.privacyMode) {
      if (!repeat) this.onPrivacyTyping();
      return;
    }
    const now = performance.now();
    const spec = resolveKeyByVk(vk, this.keyboardLayout);
    if (!spec) return;
    this.wakeCat(now);

    const anim = this.keyAnims.get(spec.id);
    if (anim) {
      anim.pressed = true;
      anim.pressAmount = 1;
      anim.lastHitAt = now;
    }

    if (repeat) {
      this.lastAnyKeyAt = now;
      const held = this.heldKeys.get(vk);
      if (held) held.at = now;
      if (this.catTail.heldVks.has(vk)) {
        this.catTailHeldAt.set(vk, now);
      }
      return;
    }

    const prevAnyKeyAt = this.lastAnyKeyAt;
    this.lastAnyKeyAt = now;
    this.recentKeyTimes.push(now);
    if (this.recentKeyTimes.length > 26) this.recentKeyTimes.shift();

    // Key mashing (4+ keys within 180ms) irritates the cat slightly!
    let burstCount = 0;
    for (let i = this.recentKeyTimes.length - 1; i >= 0; i--) {
      if (now - this.recentKeyTimes[i] <= 180) burstCount++;
      else break;
    }
    if (burstCount >= 4) {
      this.pokeAnnoy(0.32, now);
    }

    this.bodyBob = Math.min(1.15, this.bodyBob + 0.65);
    this.kbBounce = Math.min(1, this.kbBounce + 0.45);
    this.squashSpring.kick(1.85);

    const activelyMousing =
      this.mouseLeftDown || this.mouseRightDown || now - this.lastMouseMoveAt < 320;
    const rawKeyPt = this.getKeyWorldCenter(spec);
    const useTail = this.shouldUseTailForKey(spec, rawKeyPt, activelyMousing, prevAnyKeyAt, now);

    if (this.catBreed === 'dafeiyu') {
      const dafeiyuLayout = this.buildDaFeiYuLayout();
      const dafeiyuMouseBusy =
        dafeiyuLayout.mouseLeft ||
        dafeiyuLayout.mouseRight ||
        now - dafeiyuLayout.mouseActiveAt < 900;
      const dafeiyuUseTail =
        useTail ||
        this.shouldUseTailForKey(spec, rawKeyPt, dafeiyuMouseBusy, prevAnyKeyAt, now);
      const pt = dafeiyuUseTail ? this.getTailTargetPoint(spec) : rawKeyPt;
      this.dafeiyu.onKeyDown(vk, pt, dafeiyuLayout, repeat, dafeiyuUseTail);
      if (this.showKeyBubbles) {
        const label = formatBubbleForVk(vk, spec);
        if (label) {
          this.spawnBubble(label, spec.id === 'space', pt.x, now);
        }
      }
      return;
    }

    if (Math.random() < 0.16) {
      if (Math.random() < 0.5) this.earTwitchLeft = 1;
      else this.earTwitchRight = 1;
    }

    const layout = getKeyboardLayout(this.keyboardLayout);
    const isCatFacing = this.keyboardOrientation === 'cat';
    const keyColCenter = spec.uStart + spec.uWidth * 0.5;
    const screenKeyCol = isCatFacing ? layout.totalUnits - keyColCenter : keyColCenter;
    const midCol = layout.totalUnits * 0.5;
    const keyBias = Math.max(-1, Math.min(1, (screenKeyCol - midCol) / (layout.totalUnits * 0.34)));
    this.lastKeyBias = keyBias;
    this.typingGroovePhase += 1.22;

    let targetPoint = rawKeyPt;
    let chosen: PawState;

    if (useTail) {
      const clampedTailPt = this.getTailTargetPoint(spec);
      if (!this.catTail.active && this.catTail.reach < 0.08) {
        const hipRight = this.spinePoint(0.84, 0);
        this.catTail.tipX = hipRight.x + 126;
        this.catTail.tipY = hipRight.y - 37;
      }
      this.catTail.active = true;
      this.catTail.heldVks.add(vk);
      this.catTailHeldAt.set(vk, now);
      this.catTail.targetX = clampedTailPt.x;
      this.catTail.targetY = clampedTailPt.y;
      this.catTail.tapAt = now;
      this.catTail.impactSquash = 1.0;

      // Subtle playful hip shift & rock as the tail pats the chosen key!
      const tailSideSign = clampedTailPt.x >= this.catCenterX ? 1 : -1;
      this.strikeRockVel += tailSideSign * (0.95 + this.typingEnergy * 0.65);
      this.squashSpring.kick(0.95);
      this.headTiltSpring.kick(tailSideSign * 0.32);
      if (!activelyMousing) {
        this.typingSwayVel +=
          (keyBias * 1.15 + Math.cos(this.typingGroovePhase) * 0.75) *
          (0.85 + this.typingEnergy * 0.35);
      }

      if (this.showKeyBubbles) {
        const label = formatBubbleForVk(vk, spec);
        if (label) {
          this.spawnBubble(label, spec.id === 'space', clampedTailPt.x, now);
        }
      }
      return;
    }

    const splitCol =
      this.keyboardLayout === '108'
        ? isCatFacing
          ? 12.2
          : 8.4
        : 7.5;

    if (spec.id === 'space') {
      if (activelyMousing) {
        chosen = this.rightPaw;
      } else {
        this.lastSpacePaw = this.lastSpacePaw === 'left' ? 'right' : 'left';
        chosen = this.lastSpacePaw === 'left' ? this.leftPaw : this.rightPaw;
      }
      const spaceU = isCatFacing
        ? chosen.side === 'left'
          ? spec.uStart + spec.uWidth * 0.68
          : spec.uStart + spec.uWidth * 0.32
        : chosen.side === 'left'
          ? spec.uStart + spec.uWidth * 0.32
          : spec.uStart + spec.uWidth * 0.68;
      const rY = spec.rowY ?? spec.row;
      targetPoint = this.projectKeyboardPoint(spaceU, rY + 0.50);
    } else if (activelyMousing) {
      chosen = this.rightPaw;
    } else if (screenKeyCol < splitCol) {
      chosen = this.leftPaw;
    } else {
      chosen = this.rightPaw;
    }

    const sideSign = chosen.side === 'left' ? -1 : 1;
    this.strikeRockVel += sideSign * (1.75 + this.typingEnergy * 1.25);
    this.headTiltSpring.kick(sideSign * 0.42);
    if (!activelyMousing) {
      // Lively groove impulse: rocks head & upper body back and forth as the cat types!
      this.typingSwayVel += (keyBias * 2.1 + Math.cos(this.typingGroovePhase) * 1.65) * (0.9 + this.typingEnergy * 0.45);
    }

    const prevShoulder = this.getLiveShoulder(chosen, 0);
    if (chosen.mode === 'poke' && Math.hypot(targetPoint.x - chosen.pos.x, targetPoint.y - chosen.pos.y) > 22) {
      chosen.smears.push({
        sx: prevShoulder.x,
        sy: prevShoulder.y,
        px: chosen.pos.x,
        py: chosen.pos.y,
        alpha: 0.28,
      });
      if (chosen.smears.length > 3) chosen.smears.shift();
    }

    if (chosen.mode === 'curled' || chosen.mode === 'idle') {
      const startPt = this.getLiveCurledPawPos(chosen, 0);
      chosen.pos = { x: startPt.x, y: startPt.y };
      chosen.vel = { x: 0, y: 0 };
    }
    const clampedTarget = this.clampToKeyboardSurface(targetPoint);
    chosen.mode = 'poke';
    chosen.target = clampedTarget;
    chosen.pokeTime = 0;
    chosen.impactSquash = 0.85;
    chosen.lastActiveAt = now;
    this.heldKeys.set(vk, {
      side: chosen.side,
      specId: spec.id,
      x: clampedTarget.x,
      y: clampedTarget.y,
      at: now,
    });

    const other = chosen.side === 'left' ? this.rightPaw : this.leftPaw;
    if (other.mode === 'idle' && !activelyMousing) {
      other.mode = 'curled';
      other.lastActiveAt = now;
    }

    if (this.showKeyBubbles) {
      const label = formatBubbleForVk(vk, spec);
      if (label) {
        this.spawnBubble(label, spec.id === 'space', targetPoint.x, now);
      }
    }
  }

  public onKeyUp(vk: number): void {
    this.dafeiyu.onKeyUp(vk);
    this.heldKeys.delete(vk);
    if (this.catTail.heldVks.has(vk)) {
      this.catTail.heldVks.delete(vk);
      this.catTailHeldAt.delete(vk);
      if (this.catTail.heldVks.size === 0) {
        this.catTail.releaseAt = performance.now();
      }
    }
    if (this.privacyMode) return;
    const spec = resolveKeyByVk(vk, this.keyboardLayout);
    if (!spec) return;
    this.wakeRendering();
    let stillHeldSameSpec = false;
    for (const h of this.heldKeys.values()) {
      if (h.specId === spec.id) {
        stillHeldSameSpec = true;
        break;
      }
    }
    if (!stillHeldSameSpec) {
      const anim = this.keyAnims.get(spec.id);
      if (anim) anim.pressed = false;
    }
  }

  public onGlobalMouseMove(nx: number, ny: number): void {
    const now = performance.now();
    const moveDelta = Math.hypot(nx - this.lastMouseNX, ny - this.lastMouseNY);

    // Ignore 0px synthetic repeats & tiny optical-sensor micro-jitter (~3-4px; ~7-8px when napping)
    // so the sleep countdown is never silently reset and a desk bump never wakes a sleeping cat!
    const jitterThreshold = this.asleep || this.sleepAmount > 0.35 ? 0.006 : 0.0028;
    if (moveDelta < jitterThreshold) {
      return;
    }

    this.lastMouseNX = nx;
    this.lastMouseNY = ny;

    // Ignore micro-jitter while actively typing so a tiny desk bump never hijacks the typing paw
    const typingNow = now - this.lastAnyKeyAt < 500 || this.heldKeys.size > 0;
    if (typingNow && moveDelta < 0.015) {
      return;
    }

    this.wakeCat(now);
    // On desktop, onGlobalMouseMove carries true full-screen 0..1 coords; on embedded web, window pointermove feeds onPointerSpeedSample directly
    if (!this.embedded) {
      this.recordPointerSpeed(nx, ny, now);
    }
    this.mouseTargetOffsetX = (nx - 0.5) * (this.catBreed === 'dafeiyu' ? 14 : 20);
    this.mouseTargetOffsetY = (ny - 0.5) * (this.catBreed === 'dafeiyu' ? 11 : 18);
    this.lastMouseMoveAt = now;

    this.leftPaw.mode = 'mouse';
    this.leftPaw.lastActiveAt = now;
  }

  private getMouseButtonColors(): { left: string; right: string; middle: string } {
    const primary = this.customSkin
      ? this.customSkin.accentColor
      : this.catBreed === 'dafeiyu'
        ? '#5cc8ff'
        : this.catBreed === 'ragdoll'
          ? '#7ac7ff'
          : '#d4f044';
    const scroll = this.customSkin
      ? this.customSkin.accentSideColor
      : this.catBreed === 'dafeiyu'
        ? '#2563eb'
        : this.catBreed === 'ragdoll'
          ? '#4690d8'
          : '#92ba12';

    // Pick a high-contrast complementary color for RMB so Left vs Right click are unmistakable on every skin
    const isWarmAccent = /^#(f[a-f0-9][89a-c]|ff[789a-c]|e[89a-f][789a])/i.test(primary);
    const rightColor = isWarmAccent ? '#58c7ff' : '#ff7b3b';
    return { left: primary, right: rightColor, middle: scroll };
  }

  private spawnMouseBadge(button: 'left' | 'right' | 'middle', now: number): void {
    if (!this.showKeyBubbles) return;
    const colors = this.getMouseButtonColors();
    const label = button === 'left' ? 'LMB' : button === 'right' ? 'RMB' : 'MMB';
    const color = button === 'left' ? colors.left : button === 'right' ? colors.right : colors.middle;
    const mx = this.mouseBaseX + this.mouseOffsetX;
    const my = this.mouseBaseY + this.mouseOffsetY;
    const isCatFacing = this.keyboardOrientation === 'cat';
    const sideShift =
      button === 'left'
        ? isCatFacing
          ? 10
          : -10
        : button === 'right'
          ? isCatFacing
            ? -10
            : 10
          : 0;

    if (this.mouseBadges.length >= 2) {
      this.mouseBadges.shift();
    }
    this.mouseBadges.push({
      id: this.nextBubbleId++,
      label,
      color,
      x: Math.max(24, mx + sideShift),
      y: my - 30,
      createdAt: now,
      lifetime: 560,
    });
  }

  public onMouseDown(button: string): void {
    const now = performance.now();
    this.wakeCat(now);
    this.squashSpring.kick(1.2);
    if (button === 'left') {
      this.mouseLeftDown = true;
      this.mouseDownAt.left = now;
      this.mouseLeftPress = 1;
      this.spawnMouseBadge('left', now);
    } else if (button === 'right') {
      this.mouseRightDown = true;
      this.mouseDownAt.right = now;
      this.mouseRightPress = 1;
      this.spawnMouseBadge('right', now);
    } else if (button === 'middle') {
      this.mouseMiddleDown = true;
      this.mouseDownAt.middle = now;
      this.mouseMiddlePress = 1;
      this.spawnMouseBadge('middle', now);
    }

    this.lastMouseMoveAt = now;
    this.lastMouseClickAt = now;
    this.dafeiyu.resetLeftHandForMouse();
    this.dafeiyu.onMouseAction(now);
    this.leftPaw.mode = 'mouse';
    this.leftPaw.impactSquash = 0.72;
    this.leftPaw.lastActiveAt = now;
  }

  public onMouseUp(button: string): void {
    this.wakeRendering();
    if (button === 'left') {
      this.mouseLeftDown = false;
      this.mouseDownAt.left = 0;
    } else if (button === 'right') {
      this.mouseRightDown = false;
      this.mouseDownAt.right = 0;
    } else if (button === 'middle') {
      this.mouseMiddleDown = false;
      this.mouseDownAt.middle = 0;
    }
  }

  public onMouseWheel(): void {
    const now = performance.now();
    this.wakeCat(now);
    this.mouseScrollFlash = 1;
    this.lastMouseMoveAt = now;
    this.lastMouseClickAt = now;
    this.dafeiyu.resetLeftHandForMouse();
    this.dafeiyu.onMouseAction(now);
    this.leftPaw.mode = 'mouse';
    this.leftPaw.lastActiveAt = now;
  }

  private resetPetStrokes(): void {
    this.lastPetCursorX = null;
    this.lastPetCursorY = null;
    this.petAnchorX = null;
    this.petAnchorY = null;
    this.petStrokeDirX = 0;
    this.petStrokeDirY = 0;
    this.petStrokeTravel = 0;
    this.petReverseTravel = 0;
    this.petStrokeCount = 0;
  }

  public onWindowCursor(relX: number, relY: number, overCat: boolean): void {
    const now = performance.now();
    const cursorDelta = Math.hypot(relX - this.cursorRelX, relY - this.cursorRelY);
    if (relX !== this.cursorRelX || relY !== this.cursorRelY || overCat !== this.cursorOverCat) {
      this.wakeRendering(now);
      if (cursorDelta > 0.0015) {
        this.lastCursorEyeAt = now;
      }
    }
    this.cursorRelX = relX;
    this.cursorRelY = relY;
    this.cursorOverCat = overCat;

    if (overCat && !this.mouseLeftDown && !this.mouseRightDown) {
      const now = performance.now();
      const px = relX * this.logicalW;
      const py = relY * this.logicalH;
      if (this.lastPetCursorX !== null && this.lastPetCursorY !== null) {
        const dx = px - this.lastPetCursorX;
        const dy = py - this.lastPetCursorY;
        const dist = Math.hypot(dx, dy);
        if (dist > 1.5 && dist < 80) {
          this.lastPetMoveAt = now;
          const MIN_STROKE_PX = 14;
          const REQUIRED_STROKES = 2;

          if (this.petAnchorX === null || this.petAnchorY === null) {
            this.petAnchorX = this.lastPetCursorX;
            this.petAnchorY = this.lastPetCursorY;
          }

          if (this.petStrokeDirX === 0 && this.petStrokeDirY === 0) {
            const vx = px - this.petAnchorX;
            const vy = py - this.petAnchorY;
            const segLen = Math.hypot(vx, vy);
            if (segLen >= MIN_STROKE_PX) {
              this.petStrokeDirX = vx / segLen;
              this.petStrokeDirY = vy / segLen;
              this.petStrokeTravel = segLen;
              this.petReverseTravel = 0;
            }
          } else {
            const ux = dx / dist;
            const uy = dy / dist;
            const dot = ux * this.petStrokeDirX + uy * this.petStrokeDirY;
            if (dot >= 0.35) {
              // Continuing along the same straight pass across the cat
              this.petStrokeTravel += dist;
              this.petReverseTravel = Math.max(0, this.petReverseTravel - dist * 0.5);
            } else {
              // Reversing or curving (back-and-forth petting / circular rub)
              this.petReverseTravel += dist;
              if (this.petStrokeTravel >= MIN_STROKE_PX && this.petReverseTravel >= MIN_STROKE_PX) {
                this.petStrokeCount += 1;
                this.petStrokeDirX = ux;
                this.petStrokeDirY = uy;
                this.petStrokeTravel = this.petReverseTravel;
                this.petReverseTravel = 0;
                this.petAnchorX = this.lastPetCursorX;
                this.petAnchorY = this.lastPetCursorY;
              }
            }
          }

          // Only unlock petting energy & purring after intentional back-and-forth / circular strokes (Fixes #13)
          if (this.petStrokeCount >= REQUIRED_STROKES) {
            this.petEnergy = Math.min(1, this.petEnergy + dist * 0.015);
          }
        }
      } else {
        this.petAnchorX = px;
        this.petAnchorY = py;
      }
      this.lastPetCursorX = px;
      this.lastPetCursorY = py;
    } else {
      this.resetPetStrokes();
    }
  }

  // ---- Keyboard projection ----

  private projectKeyboardPoint(uCol: number, rowPos: number): Vec2 {
    const layout = getKeyboardLayout(this.keyboardLayout);
    const isCatFacing = this.keyboardOrientation === 'cat';
    const screenU = isCatFacing ? layout.totalUnits - uCol : uCol;
    const screenRow = isCatFacing ? layout.totalRows - rowPos : rowPos;

    const padX = 7;
    const padY = 6;
    const innerW = this.kbWidth - padX * 2;
    const innerH = this.kbHeight - padY * 2;
    const localX = -innerW / 2 + (screenU / layout.totalUnits) * innerW;
    const localY = -innerH / 2 + (screenRow / layout.totalRows) * innerH;

    const depthFactor = 0.94 + (screenRow / layout.totalRows) * 0.09;
    const px = localX * depthFactor;
    const py = localY;

    const cos = Math.cos(this.kbTiltRad);
    const sin = Math.sin(this.kbTiltRad);
    return {
      x: this.kbCenterX + px * cos - py * sin,
      y: this.kbCenterY + px * sin + py * cos,
    };
  }

  private getKeyWorldCenter(spec: KeySpec): Vec2 {
    const layout = getKeyboardLayout(this.keyboardLayout);
    const isCatFacing = this.keyboardOrientation === 'cat';
    const uClamped = Math.max(0.65, Math.min(layout.totalUnits - 0.65, spec.uStart + spec.uWidth * 0.5));
    const rY = spec.rowY ?? spec.row;
    const rSpan = spec.rowSpan ?? 1;
    let rowPos = rY + rSpan * 0.5;
    if (isCatFacing && rY <= 0.1) {
      rowPos = rY + 0.68;
    } else if (!isCatFacing && rY + rSpan >= layout.totalRows - 0.1) {
      rowPos = rY + rSpan - 0.68;
    }
    return this.projectKeyboardPoint(uClamped, rowPos);
  }

  private clampToKeyboardSurface(pt: Vec2): Vec2 {
    // Transform into keyboard-local space and clamp so the plump paw pad NEVER extends past the keyboard bezel!
    const dx = pt.x - this.kbCenterX;
    const dy = pt.y - this.kbCenterY;
    const cos = Math.cos(-this.kbTiltRad);
    const sin = Math.sin(-this.kbTiltRad);
    const lx = dx * cos - dy * sin;
    const ly = dx * sin + dy * cos;

    const maxX = this.kbWidth * 0.5 - 15;
    const minY = -this.kbHeight * 0.5 + 8;
    const maxY = this.kbHeight * 0.5 - 14;
    const cx = Math.max(-maxX, Math.min(maxX, lx));
    const cy = Math.max(minY, Math.min(maxY, ly));

    const rcos = Math.cos(this.kbTiltRad);
    const rsin = Math.sin(this.kbTiltRad);
    return {
      x: this.kbCenterX + cx * rcos - cy * rsin,
      y: this.kbCenterY + cx * rsin + cy * rcos,
    };
  }

  private spawnBubble(label: string, isSpace: boolean, keyX: number, now: number): void {
    if (this.bubbles.length >= 6) {
      this.bubbles.shift();
    }
    const minX = this.keyboardLayout === '108' ? 145 : 125;
    const maxX = this.logicalW - 105;
    const baseX = Math.max(minX, Math.min(maxX, keyX * 0.45 + this.kbCenterX * 0.55));
    const staggerY = 258 + ((this.nextBubbleId % 3) - 1) * 10 + (Math.random() - 0.5) * 6;
    this.bubbles.push({
      id: this.nextBubbleId++,
      label,
      isSpace,
      x: baseX,
      targetX: baseX,
      y: 292,
      targetY: staggerY,
      rot: ((Math.random() - 0.5) * 4.5 * Math.PI) / 180,
      createdAt: now,
      lifetime: 940,
    });
    this.layoutBubbles();
  }

  private layoutBubbles(): void {
    const count = this.bubbles.length;
    if (count === 0) return;

    const widths = this.bubbles.map((b) => (b.isSpace ? 40 : Math.max(25, b.label.length * 7.5 + 14)));
    const gap = 3.5;
    const totalW = widths.reduce((a, b) => a + b, 0) + gap * (count - 1);
    const anchor = this.kbCenterX + 12;
    const leftBound = this.keyboardLayout === '108' ? 140 : 126;
    const rightBound = this.logicalW - 82;
    let cursor = Math.max(leftBound, Math.min(rightBound - totalW, anchor - totalW * 0.5));

    for (let i = 0; i < count; i++) {
      const w = widths[i];
      this.bubbles[i].targetX = cursor + w * 0.5;
      cursor += w + gap;
    }
  }

  // ---- Simulation update ----

  private update(dt: number, now: number): void {
    this.elapsed += dt;

    // Cross-platform lost-MouseUp safety fallback (Fixes #4 on macOS/Linux/web; Windows also reconciles via GetAsyncKeyState)
    const MOUSE_STUCK_FALLBACK_MS = 10_000;
    const lastMouseActivity = Math.max(this.lastMouseMoveAt, this.lastMouseClickAt);
    if (
      this.mouseLeftDown &&
      now - Math.max(lastMouseActivity, this.mouseDownAt.left) > MOUSE_STUCK_FALLBACK_MS
    ) {
      this.mouseLeftDown = false;
      this.mouseDownAt.left = 0;
    }
    if (
      this.mouseRightDown &&
      now - Math.max(lastMouseActivity, this.mouseDownAt.right) > MOUSE_STUCK_FALLBACK_MS
    ) {
      this.mouseRightDown = false;
      this.mouseDownAt.right = 0;
    }
    if (
      this.mouseMiddleDown &&
      now - Math.max(lastMouseActivity, this.mouseDownAt.middle) > MOUSE_STUCK_FALLBACK_MS
    ) {
      this.mouseMiddleDown = false;
      this.mouseDownAt.middle = 0;
    }

    // Cross-platform lost-KeyUp safety fallback (Fixes #3)
    const KEY_HOLD_STALE_MS = 15_000;
    for (const [vk, held] of this.heldKeys) {
      if (now - held.at > KEY_HOLD_STALE_MS) {
        this.heldKeys.delete(vk);
        const anim = this.keyAnims.get(held.specId);
        if (anim) anim.pressed = false;
      }
    }
    for (const [vk, heldAt] of this.catTailHeldAt) {
      if (now - heldAt > KEY_HOLD_STALE_MS) {
        this.catTailHeldAt.delete(vk);
        this.catTail.heldVks.delete(vk);
        if (this.catTail.heldVks.size === 0) {
          this.catTail.releaseAt = now;
        }
      }
    }

    if (this.petStrokeCount > 0 && now - this.lastPetMoveAt > 2200) {
      this.resetPetStrokes();
    }

    const mLerp = 1 - Math.exp(-28 * dt);
    this.mouseOffsetX += (this.mouseTargetOffsetX - this.mouseOffsetX) * mLerp;
    this.mouseOffsetY += (this.mouseTargetOffsetY - this.mouseOffsetY) * mLerp;
    if (!this.mouseLeftDown && this.mouseLeftPress > 0) {
      this.mouseLeftPress = Math.max(0, this.mouseLeftPress - dt * 5.8);
    }
    if (!this.mouseRightDown && this.mouseRightPress > 0) {
      this.mouseRightPress = Math.max(0, this.mouseRightPress - dt * 5.8);
    }
    if (!this.mouseMiddleDown && this.mouseMiddlePress > 0) {
      this.mouseMiddlePress = Math.max(0, this.mouseMiddlePress - dt * 5.8);
    }
    this.mouseScrollFlash = Math.max(0, this.mouseScrollFlash - dt * 4.5);
    this.mouseBadges = this.mouseBadges.filter((b) => now - b.createdAt < b.lifetime);

    while (this.recentKeyTimes.length > 0 && now - this.recentKeyTimes[0] > 950) {
      this.recentKeyTimes.shift();
    }
    const targetTypingEnergy = Math.min(1, this.recentKeyTimes.length / 7);
    this.typingEnergy += (targetTypingEnergy - this.typingEnergy) * (1 - Math.exp(-9 * dt));

    const isTypingRecently =
      now - this.lastAnyKeyAt < 650 ||
      this.heldKeys.size > 0 ||
      this.catTail.heldVks.size > 0;
    const isCatMode = this.catBreed !== 'dafeiyu';

    // 1) Alive cat emotional states: sleep (loaf + Zzz), curious (laser-pointer eyes), annoy -> cross (airplane ears + tail desk-thump), idle fidgets!
    const sleepAfterMs = this.embedded ? 18_000 : 28_000;
    const idleMs = now - this.lastInputAt;
    if (isCatMode && !this.privacyMode && !this.asleep && this.petEnergy < 0.15 && idleMs > sleepAfterMs) {
      this.asleep = true;
      this.isCross = false;
      this.idleAct = null;
      this.nextZAt = now + 900;
    }
    const wantSleep = isCatMode && this.asleep && !this.privacyMode ? 1 : 0;
    this.sleepAmount += (wantSleep - this.sleepAmount) * (1 - Math.exp(-(wantSleep ? 1.5 : 6.5) * dt));
    if (this.sleepAmount < 0.002 && !wantSleep) this.sleepAmount = 0;

    // Fast pointer speed over last 220ms triggers Curious (laser-pointer big round eyes!)
    // Never false-trigger Curious while grumpy (isCross) or actively typing, so moving the mouse never snaps back typing/grumpy mood!
    while (this.pointerSamples.length > 0 && now - this.pointerSamples[0].t > 220) {
      this.pointerSamples.shift();
    }
    const pointerSpeed =
      this.pointerSamples.reduce((acc, s) => acc + s.dist, 0) / 0.22;
    const canTriggerCurious =
      isCatMode &&
      !this.asleep &&
      !this.privacyMode &&
      !this.isCross &&
      now - this.lastAnyKeyAt > 900;
    if (canTriggerCurious && this.pointerSamples.length >= 4 && pointerSpeed > 0.92) {
      this.curiousUntil = Math.max(this.curiousUntil, now + 2200);
    }
    const curiousNow = isCatMode && !this.asleep && !this.privacyMode && !this.isCross && now < this.curiousUntil;
    this.curiousAmount = Math.max(0, Math.min(1.2, this.curiousSpring.step(curiousNow ? 1 : 0, dt)));
    const curiousClamped = Math.min(1, this.curiousAmount);

    // Annoyance accumulates from sustained furious typing (>8.5 CPS) or key mashing (in onKeyDown), cools down after 2.2s quiet
    if (isCatMode && !this.privacyMode && !this.asleep) {
      const cps = this.recentKeyTimes.length / 0.95;
      if (cps > 8.5) {
        this.pokeAnnoy((cps - 8.5) * 0.055 * dt, now);
      } else if (this.isCross && now - this.lastAnyKeyAt < 900 && this.recentKeyTimes.length >= 4) {
        // Keep grumpy mood alive while user continues typing continuously
        this.lastPokeAt = now;
      }
      if (now - this.lastPokeAt > 2200) {
        this.annoy = Math.max(0, this.annoy - 0.15 * dt);
      }
      if (!this.isCross && this.annoy >= 0.62) {
        this.isCross = true;
        this.crossSince = now;
        this.crossHuffDir = this.cursorRelX >= 0.5 ? -1 : 1;
        this.thumpPhase = 0;
        this.curiousUntil = -10000;
        this.idleAct = null;
      } else if (this.isCross && this.annoy < 0.26 && now - this.crossSince > 3200) {
        this.isCross = false;
      }
    } else {
      this.isCross = false;
      this.annoy = Math.max(0, this.annoy - 0.35 * dt);
    }
    // Moving the mouse never cancels grumpy mode; only cooling down naturally (or demo button) clears it
    const crossTarget = this.isCross ? 1 : 0;
    this.crossAmount = Math.max(0, Math.min(1, this.crossSpring.step(crossTarget, dt)));

    // Idle fidgets when calmly awake and idle > 2.6s (tilt, single-ear flick, affectionate slowBlink, look around, tail flick)
    if (this.idleAct && now > this.idleAct.t0 + this.idleAct.dur) {
      this.idleAct = null;
    }
    if (
      isCatMode &&
      !this.idleAct &&
      now >= this.nextIdleActAt &&
      idleMs > 2600 &&
      !this.asleep &&
      !this.privacyMode &&
      this.petEnergy < 0.15 &&
      !this.isCross &&
      !curiousNow
    ) {
      const kinds: IdleFidget['kind'][] = ['tilt', 'tilt', 'ear', 'slowBlink', 'look', 'tail'];
      const kind = kinds[Math.floor(Math.random() * kinds.length)];
      const durMap: Record<IdleFidget['kind'], number> = {
        tilt: 1900,
        ear: 460,
        slowBlink: 1450,
        look: 2100,
        tail: 1100,
      };
      this.idleAct = {
        kind,
        t0: now,
        dur: durMap[kind],
        side: Math.random() < 0.5 ? -1 : 1,
      };
      this.nextIdleActAt = now + 3800 + Math.random() * 4800;
      this.wakeRendering(now);
    }
    const act = this.idleAct;
    const actU = act ? Math.max(0, Math.min(1, (now - act.t0) / act.dur)) : 0;
    const smoothstep = (a: number, b: number, x: number) => {
      const t = Math.max(0, Math.min(1, (x - a) / (b - a)));
      return t * t * (3 - 2 * t);
    };
    const actEnv = act ? smoothstep(0, 0.25, actU) * (1 - smoothstep(0.7, 1, actU)) : 0;

    // 2) Staggered 240Hz spring posture (body & hunch follow a beat behind head & eyes!)
    const targetHunch = isTypingRecently
      ? 0.45 + this.typingEnergy * 0.55
      : curiousClamped * 0.28;
    this.typingHunch = Math.max(0, Math.min(1.2, this.hunchSpring.step(targetHunch, dt)));
    this.squashSpring.step(this.sleepAmount * 0.35 - curiousClamped * 0.25, dt);

    const rockAcc = -this.strikeRock * 140 - this.strikeRockVel * 14;
    this.strikeRockVel += rockAcc * dt;
    this.strikeRock = Math.max(-1, Math.min(1, this.strikeRock + this.strikeRockVel * dt));

    this.bodyBob = Math.max(0, this.bodyBob - dt * 11.5);
    this.kbBounce = Math.max(0, this.kbBounce - dt * 15);

    // Playful left-right body & head groove sway during pure typing
    const pureTypingActive = isTypingRecently && this.leftPaw.mode !== 'mouse';
    if (pureTypingActive) {
      this.typingGroovePhase += dt * (4.8 + this.typingEnergy * 5.2);
    }
    const grooveWave = Math.sin(this.typingGroovePhase) * (0.58 + this.typingEnergy * 0.44);
    const targetTypingSway = pureTypingActive
      ? Math.max(-1.1, Math.min(1.1, grooveWave * 0.72 + this.lastKeyBias * 0.55))
      : this.leftPaw.mode === 'mouse' && isTypingRecently
        ? Math.sin(this.typingGroovePhase) * 0.16
        : 0;
    const swayAcc = (targetTypingSway - this.typingSway) * 118 - this.typingSwayVel * 12.5;
    this.typingSwayVel += swayAcc * dt;
    this.typingSway = Math.max(-1.2, Math.min(1.2, this.typingSway + this.typingSwayVel * dt));

    // Measured spine-shear lean via 240Hz spring (1.9 Hz -> organic weighty follow-through!)
    let targetLean = 0;
    if (this.leftPaw.mode === 'mouse') {
      const mouseSweepBias = (this.mouseOffsetX / 18) * 0.22;
      targetLean = this.rightPaw.mode === 'poke'
        ? -0.52 + mouseSweepBias
        : -0.96 + mouseSweepBias;
    } else if (this.catTail.reach > 0.05) {
      const tailDir = Math.max(-1, Math.min(1, (this.catTail.tipX - this.catCenterX) / (this.kbWidth * 0.5)));
      targetLean = tailDir * 0.14 * this.catTail.reach;
    }
    this.spineLean = this.spineLeanSpring.step(targetLean, dt);

    // Stable neutral eye reference (no feedback wobble when spine leans toward the mouse!)
    const eyeRefRelX = this.catCenterX / this.logicalW;
    const eyeRefRelY = (this.catTopY + 0.24 * this.catHeight) / this.logicalH;
    const rawDx = this.cursorRelX - eyeRefRelX;
    const rawDy = this.cursorRelY - eyeRefRelY;
    // Smooth asymptotic curve (-1..+1): responsive near the face, buttery easing across the whole screen (zero hard kink!)
    const gx = Math.tanh(rawDx * 2.15);
    const gy = Math.tanh(rawDy * 2.05);

    const wakeElapsed = (now - this.wakeAt) / 1000;
    const wakeShake =
      wakeElapsed >= 0 && wakeElapsed < 0.55
        ? Math.sin(wakeElapsed * 36) * 0.18 * (1 - wakeElapsed / 0.55)
        : 0;
    const fidgetTilt = act && act.kind === 'tilt' ? act.side * 0.19 * actEnv : 0;
    // Continuous smooth head cock when curious (never snaps left/right across dx=0!)
    const curiousCock = curiousClamped * 0.16 * gx;
    const sleepTilt = this.sleepAmount * 0.08;
    const desiredHeadTilt =
      this.typingSway * 0.18 +
      this.typingHunch * (this.strikeRock * 0.065 - 0.02) +
      this.spineLean * 0.085 +
      fidgetTilt +
      curiousCock +
      sleepTilt;
    this.headTilt = this.headTiltSpring.step(desiredHeadTilt, dt) + wakeShake;

    this.petEnergy = Math.max(0, this.petEnergy - dt * (this.cursorOverCat ? 0.22 : 1.15));
    this.lastDt = dt;
    const wantSquint = (this.petEnergy > 0.3 || this.hoverPet) && !this.isCross ? 1 : 0;
    this.squintAmount += (wantSquint - this.squintAmount) * (1 - Math.exp(-12 * dt));
    const wantPrivacy = this.privacyMode ? 1 : 0;
    this.privacyClose += (wantPrivacy - this.privacyClose) * (1 - Math.exp(-14 * dt));
    catAudio.setPurrIntensity(
      this.catBreed !== 'dafeiyu' && !this.privacyMode && this.squintAmount > 0.25 ? this.squintAmount : 0,
    );

    // Dynamic ear spring (5.2Hz, damping 0.36): perks up when curious/excited, flattens into airplane ears when cross!
    const earTarget = isCatMode
      ? 0.12 * this.typingEnergy * (1 - this.crossAmount) +
        0.16 * curiousClamped -
        0.18 * this.squintAmount -
        0.58 * this.crossAmount -
        0.12 * this.sleepAmount
      : 0;
    this.earPerkSpring.step(earTarget, dt);

    if (this.catBreed === 'dafeiyu') {
      this.dafeiyu.update(dt, now, this.buildDaFeiYuLayout());
    } else {
      // Update plush cat tail key-patting physics for keys near the tail
      if (this.privacyMode || this.asleep) {
        this.catTail.active = false;
        this.catTail.heldVks.clear();
      }
      const tailAge = Math.max(0, now - this.catTail.tapAt);
      const tailPressing = this.catTail.active && (this.catTail.heldVks.size > 0 || tailAge < 155);
      const sinceTailRelease =
        this.catTail.heldVks.size > 0
          ? 0
          : Math.max(0, now - Math.max(this.catTail.tapAt + 155, this.catTail.releaseAt));
      if (this.catTail.active && !tailPressing && sinceTailRelease > 460) {
        this.catTail.active = false;
      }

      const targetReach = this.catTail.active ? 1 : 0;
      const reachRate = this.catTail.active ? 28 : 9.5;
      this.catTail.reach += (targetReach - this.catTail.reach) * (1 - Math.exp(-reachRate * dt));
      if (this.catTail.reach < 0.002 && !this.catTail.active) this.catTail.reach = 0;

      let targetHover = 0;
      if (this.catTail.active) {
        if (tailAge < 75) {
          targetHover = Math.sin((tailAge / 75) * Math.PI) * 24;
        } else if (tailPressing) {
          targetHover = 0;
        } else {
          targetHover = 13 + Math.sin(now * 0.012) * 2.2;
        }
      }
      const hoverRate = tailAge < 75 ? 45 : tailPressing ? 52 : 16;
      this.catTail.hover += (targetHover - this.catTail.hover) * (1 - Math.exp(-hoverRate * dt));

      const tipFollow = 1 - Math.exp(-34 * dt);
      this.catTail.tipX += (this.catTail.targetX - this.catTail.tipX) * tipFollow;
      this.catTail.tipY += (this.catTail.targetY - this.catTail.tipY) * tipFollow;
      this.catTail.impactSquash = Math.max(0, this.catTail.impactSquash - dt * 6.5);

      // Rhythmic 3-beat grumpy tail desk-thump when cross (lift slowly -> slam hard -> bounce keyboard & dust puffs!)
      let targetThumpLift = 0;
      let targetThumpLash = 0;
      if (this.crossAmount > 0.08 && this.catTail.reach < 0.25) {
        const prevPhase = this.thumpPhase;
        this.thumpPhase += dt * 2.55;
        const beat = Math.floor(this.thumpPhase);
        const u = this.thumpPhase - beat;
        const restingBeat = beat % 4 === 3;
        if (!restingBeat) {
          targetThumpLift =
            u < 0.58
              ? smoothstep(0, 0.58, u)
              : u < 0.72
                ? 1 - ((u - 0.58) / 0.14) ** 2
                : 0;
        }
        targetThumpLash = Math.sin(this.thumpPhase * Math.PI * 0.5);
        const slamAt = beat + 0.72;
        if (!restingBeat && this.isCross && prevPhase < slamAt && this.thumpPhase >= slamAt) {
          this.onTailDeskSlam(now);
        }
      }
      this.thumpLift += (targetThumpLift - this.thumpLift) * (1 - Math.exp(-38 * dt));
      this.thumpLash += (targetThumpLash - this.thumpLash) * (1 - Math.exp(-18 * dt));
    }

    this.deskHopY = this.deskHopSpring.step(0, dt);
    this.headsetSpring.step(this.headphonesOn ? 1 : 0, dt);

    // Floating sleepy comic "Z" particles when asleep
    if (isCatMode && this.sleepAmount > 0.68 && now >= this.nextZAt) {
      this.nextZAt = now + 1350;
      const headPt = this.spinePoint(0.18, 0);
      this.fxParticles.push({
        kind: 'z',
        x: headPt.x + 42,
        y: headPt.y - 18,
        vx: 9,
        vy: -24,
        size: 13,
        createdAt: now,
        lifetime: 2400,
      });
    }

    // Update FX particles (impact starburst, dust puffs, sleep Zzz)
    for (let i = this.fxParticles.length - 1; i >= 0; i--) {
      const fx = this.fxParticles[i];
      if (now - fx.createdAt >= fx.lifetime) {
        this.fxParticles.splice(i, 1);
        continue;
      }
      fx.x += fx.vx * dt;
      fx.y += fx.vy * dt;
      if (fx.kind === 'dust') {
        fx.vx *= Math.exp(-2.2 * dt);
        fx.vy *= Math.exp(-1.5 * dt);
      }
    }

    // Keep active animations smooth. Sleep particles continue drawing at the idle rate.
    if (
      isCatMode &&
      (curiousNow ||
        this.crossAmount > 0.03 ||
        this.idleAct !== null ||
        this.fxParticles.some((fx) => fx.kind !== 'z') ||
        (this.sleepAmount > 0.02 && this.sleepAmount < 0.98) ||
        (wakeElapsed >= 0 && wakeElapsed < 0.65) ||
        Math.abs(this.deskHopY) > 0.15 ||
        Math.abs(this.eyeXSpring.v) > 0.02 ||
        Math.abs(this.eyeYSpring.v) > 0.02 ||
        Math.abs(this.faceTurnSpring.v) > 0.05)
    ) {
      this.lastActivityAt = Math.max(this.lastActivityAt, now);
    }

    this.earTwitchLeft = Math.max(0, this.earTwitchLeft - dt * 8);
    this.earTwitchRight = Math.max(0, this.earTwitchRight - dt * 8);

    // Pause blinking while staring curiously at fast cursor!
    if (curiousNow && this.elapsed >= this.nextBlinkAt - 0.2) {
      this.nextBlinkAt = this.elapsed + 0.6;
    }
    if (!this.isBlinking && this.elapsed >= this.nextBlinkAt) {
      this.isBlinking = true;
      this.blinkProgress = 0;
    }
    if (this.isBlinking) {
      this.blinkProgress += dt * 12;
      if (this.blinkProgress >= 2) {
        this.isBlinking = false;
        this.blinkProgress = 0;
        this.nextBlinkAt = this.elapsed + 2.8 + Math.random() * 3.6;
      }
    }

    // 3) Silky normalized eye tracking (9.5Hz critically-damped -> liquid follow!) & 2.5D face turn (2.9Hz -> follows a beat behind!)
    const cursorRecentlyMoved =
      now - this.lastCursorEyeAt < 2400 || now - this.lastMouseMoveAt < 2400;
    const pureTypingGlance = cursorRecentlyMoved || curiousNow ? 0 : this.typingHunch;
    let targetGazeX =
      gx * (1 - pureTypingGlance * 0.45) +
      pureTypingGlance * Math.max(-1, Math.min(1, this.typingSway * 0.55 + this.lastKeyBias * 0.32));
    let targetGazeY =
      gy * (1 - pureTypingGlance * 0.45) + pureTypingGlance * 0.72;

    if (act && act.kind === 'look') {
      targetGazeX = targetGazeX * (1 - actEnv) + act.side * 0.85 * actEnv;
      targetGazeY = targetGazeY * (1 - actEnv) - 0.22 * actEnv;
    }
    if (this.crossAmount > 0.05) {
      // Stable huffy side-glance (locked direction -> zero sudden left/right snap when cursor crosses center!)
      const huffX = this.crossHuffDir * 0.72 * this.crossAmount;
      targetGazeX = targetGazeX * (1 - this.crossAmount * 0.62) + huffX * 0.62;
    }
    this.eyeLookX = Math.max(-1.05, Math.min(1.05, this.eyeXSpring.step(targetGazeX, dt)));
    this.eyeLookY = Math.max(-1.05, Math.min(1.05, this.eyeYSpring.step(targetGazeY, dt)));

    const huffFace = this.crossAmount * this.crossHuffDir * 7.5;
    const lookFace = act && act.kind === 'look' ? act.side * 5.5 * actEnv : 0;
    const targetFaceTurn =
      gx * (5.2 + curiousClamped * 3.8) * (1 - this.crossAmount * 0.75) +
      huffFace +
      lookFace +
      this.spineLean * 8.0 +
      this.typingSway * 8.5 +
      pureTypingGlance * 1.5;
    this.faceTurnX = this.faceTurnSpring.step(targetFaceTurn, dt);

    for (const anim of this.keyAnims.values()) {
      if (!anim.pressed && anim.pressAmount > 0) {
        anim.pressAmount = Math.max(0, anim.pressAmount - dt * 5.0);
      }
    }

    this.updatePaw(this.leftPaw, dt, now);
    this.updatePaw(this.rightPaw, dt, now);

    const beforeLen = this.bubbles.length;
    this.bubbles = this.bubbles.filter((b) => now - b.createdAt < b.lifetime);
    if (this.bubbles.length !== beforeLen) {
      this.layoutBubbles();
    }
    for (const b of this.bubbles) {
      const age = (now - b.createdAt) / b.lifetime;
      b.x += (b.targetX - b.x) * (1 - Math.exp(-18 * dt));
      const desiredY = b.targetY - age * 10;
      b.y += (desiredY - b.y) * (1 - Math.exp(-16 * dt));
    }
  }

  private latestHeldKeyFor(
    side: 'left' | 'right',
  ): { side: 'left' | 'right'; specId: string; x: number; y: number; at: number } | null {
    let best: { side: 'left' | 'right'; specId: string; x: number; y: number; at: number } | null =
      null;
    for (const entry of this.heldKeys.values()) {
      if (entry.side === side && (!best || entry.at >= best.at)) {
        best = entry;
      }
    }
    return best;
  }

  private updatePaw(paw: PawState, dt: number, now: number): void {
    const liveMouseX = this.mouseBaseX + this.mouseOffsetX;
    const liveMouseY = this.mouseBaseY + this.mouseOffsetY;
    const heldKey = this.latestHeldKeyFor(paw.side);

    if (paw.mode === 'mouse') {
      const isCatFacing = this.keyboardOrientation === 'cat';
      const lrBias = this.mouseLeftPress - this.mouseRightPress;
      const anyClickPress = Math.max(this.mouseLeftPress, this.mouseRightPress, this.mouseMiddlePress);
      if (isCatFacing) {
        // 180° cat-facing: palm heel at TOP, LMB = inner/+X, RMB = outer/-X
        paw.target = {
          x: liveMouseX + 1.2 + lrBias * 6.4,
          y: liveMouseY - 11.5 + anyClickPress * 3.6,
        };
      } else {
        // 0° viewer-facing: palm heel at BOTTOM, LMB = left/-X, RMB = right/+X
        paw.target = {
          x: liveMouseX + 0.5 - lrBias * 6.4,
          y: liveMouseY + 10.0 - anyClickPress * 3.4,
        };
      }
      const stillHolding =
        this.mouseLeftDown ||
        this.mouseRightDown ||
        this.mouseMiddleDown ||
        now - this.lastMouseMoveAt < 2200;
      if (!stillHolding) {
        paw.mode = heldKey ? 'poke' : 'idle';
      }
    } else if (heldKey) {
      // Keep paw pressed down onto the held keycap until keyUp (Fixes #3)
      paw.mode = 'poke';
      paw.target = { x: heldKey.x, y: heldKey.y };
      paw.pokeTime = Math.min(paw.pokeTime + dt, 0.10);
      paw.lastActiveAt = now;
    } else if (paw.mode === 'poke') {
      paw.pokeTime += dt;
      if (paw.pokeTime > 0.13) {
        if (paw.side === 'left' && (this.mouseLeftDown || this.mouseRightDown || now - this.lastMouseMoveAt < 450)) {
          paw.mode = 'mouse';
        } else if (now - this.lastAnyKeyAt < 650 || this.heldKeys.size > 0) {
          paw.mode = 'curled';
        } else {
          paw.mode = 'idle';
        }
      }
    } else if (paw.mode === 'curled') {
      if (now - this.lastAnyKeyAt > 680 && this.heldKeys.size === 0) {
        paw.mode = 'idle';
      }
    }

    if (paw.mode === 'curled') {
      paw.target = this.getLiveCurledPawPos(paw, 0);
    } else if (paw.mode === 'idle') {
      const baseSp = this.spinePoint(1.0, 0);
      paw.target = { x: baseSp.x + (paw.side === 'left' ? -19 : 19), y: baseSp.y };
    }

    const wantIdle = paw.mode === 'idle' ? 1 : 0;
    const wantCurl = paw.mode === 'curled' ? 1 : 0;
    const blendRate = 1 - Math.exp(-20 * dt);
    paw.idleWeight += (wantIdle - paw.idleWeight) * blendRate;
    paw.curlWeight += (wantCurl - paw.curlWeight) * blendRate;

    // Unconditionally stable, framerate-independent tracking (ZERO Euler spring overshoot at any FPS!)
    const followRate = paw.mode === 'poke' ? 42 : paw.mode === 'mouse' ? 32 : 22;
    const follow = 1 - Math.exp(-followRate * dt);
    paw.pos.x += (paw.target.x - paw.pos.x) * follow;
    paw.pos.y += (paw.target.y - paw.pos.y) * follow;
    paw.vel.x = 0;
    paw.vel.y = 0;

    // Hard guarantee: while poking a key, paw tip stays strictly on top of the keyboard surface
    if (paw.mode === 'poke') {
      const clamped = this.clampToKeyboardSurface(paw.pos);
      if (clamped.y < paw.pos.y || Math.abs(clamped.x - paw.pos.x) > 0.01) {
        paw.pos.x = clamped.x;
        paw.pos.y = Math.min(paw.pos.y, clamped.y);
      }
    }

    paw.impactSquash = Math.max(0, paw.impactSquash - dt * 7.5);

    for (const s of paw.smears) {
      s.alpha -= dt * 3.8;
    }
    paw.smears = paw.smears.filter((s) => s.alpha > 0.02);
  }

  // ---- High-fidelity true-scale rendering ----

  private render(): void {
    const ctx = this.ctx;
    const cw = this.canvas.width;
    const ch = this.canvas.height;
    ctx.clearRect(0, 0, cw, ch);

    ctx.save();
    const { offsetX, offsetY, uniformScale } = this.getViewportTransform();
    ctx.translate(offsetX, offsetY);
    ctx.scale(uniformScale, uniformScale);

    const breathFreq = 2.1 - this.sleepAmount * 0.95;
    const breath = Math.sin(this.elapsed * breathFreq) * (1.0 + this.sleepAmount * 0.65);
    const purrShift = this.squintAmount > 0.4 ? Math.sin(this.elapsed * 22) * 0.45 * this.squintAmount : 0;

    if (this.catBreed === 'dafeiyu') {
      const now = performance.now();
      const layout = this.buildDaFeiYuLayout();
      this.dafeiyu.renderBody(ctx, now, layout);
      this.drawMiniMouse(ctx);
      this.drawKeyboard(ctx);
      this.dafeiyu.renderForeground(ctx, this.lastDt, now, layout);
      if (this.showKeyBubbles && !this.privacyMode) {
        this.drawKeyBubbles(ctx);
        this.drawMouseBadges(ctx);
      }
      ctx.restore();
      return;
    }

    // Layer 1: Mini mouse & mechanical keyboard (both hop organically on grumpy tail desk-slam!)
    this.drawMiniMouse(ctx);
    this.drawKeyboard(ctx);

    // Layer 2: Plush cat tail (above keyboard so near-tail key pats & angry desk-thumps land cleanly, yet tucked behind hip!)
    this.drawTail(ctx);

    // Layer 3: Exact measured spine-deformable cat silhouette (ears, cheeks, neck, pear belly, grounded hind feet, eyes & tiny whiskers)
    this.drawCatBody(ctx, breath, purrShift);

    // Layer 4: True-scale velvety cat paws (tapered idle legs / planted diagonal mouse support leg / bent curled chest paw / plump round poking paw!)
    this.drawPaw(ctx, this.leftPaw, breath);
    this.drawPaw(ctx, this.rightPaw, breath);

    // Layer 4.5: Alive cat comic FX (anger 💢 mark, tail-slam starburst & dust puffs, sleepy Zzz bubbles)
    this.drawCatAliveFx(ctx, performance.now(), breath);

    // Layer 5: Organic bouncing 3D keycap bubbles & crisp mouse click badges
    if (this.showKeyBubbles && !this.privacyMode) {
      this.drawKeyBubbles(ctx);
      this.drawMouseBadges(ctx);
    }

    ctx.restore();
  }

  private onTailDeskSlam(now: number): void {
    this.lastSlamAt = now;
    const hipRight = this.spinePoint(0.84, 0);
    const slamX = Math.min(this.logicalW - 22, hipRight.x + 132);
    const slamY = hipRight.y + 30;

    // Comic white starburst impact + 3 playful dust puffs!
    this.fxParticles.push({
      kind: 'bang',
      x: slamX,
      y: slamY - 4,
      vx: 0,
      vy: -8,
      size: 16,
      createdAt: now,
      lifetime: 240,
    });
    for (const s of [-1, 0, 1]) {
      this.fxParticles.push({
        kind: 'dust',
        x: slamX + s * 6,
        y: slamY,
        vx: s * 36 + (Math.random() - 0.5) * 14,
        vy: -24 - Math.random() * 18,
        size: 6.5 + Math.random() * 2.5,
        createdAt: now,
        lifetime: 460,
      });
    }
    this.deskHopSpring.kick(-38);
    this.squashSpring.kick(2.4);
  }

  private drawCatAliveFx(ctx: CanvasRenderingContext2D, now: number, breath: number): void {
    // 1) Pulsing comic anger vein (💢) beside upper head when grumpy/cross
    if (this.crossAmount > 0.12 && !this.privacyMode) {
      const headPt = this.spinePoint(0.18, breath);
      const slamPulse = 1 + 0.24 * Math.max(0, 1 - (now - this.lastSlamAt) / 250);
      const sc = this.crossAmount * slamPulse;
      ctx.save();
      ctx.translate(headPt.x - 57 + this.faceTurnX * 0.4, headPt.y - 34);
      ctx.rotate(-0.14);
      ctx.scale(sc, sc);
      ctx.globalAlpha = Math.min(1, this.crossAmount * 1.1);
      ctx.lineCap = 'round';
      const drawVein = (color: string, width: number) => {
        ctx.strokeStyle = color;
        ctx.lineWidth = width;
        const r = 5.2;
        const off = 6.0;
        ctx.beginPath();
        ctx.arc(-off, -off, r, 0.05 * Math.PI, 0.45 * Math.PI);
        ctx.stroke();
        ctx.beginPath();
        ctx.arc(off, -off, r, 0.55 * Math.PI, 0.95 * Math.PI);
        ctx.stroke();
        ctx.beginPath();
        ctx.arc(off, off, r, 1.05 * Math.PI, 1.45 * Math.PI);
        ctx.stroke();
        ctx.beginPath();
        ctx.arc(-off, off, r, 1.55 * Math.PI, 1.95 * Math.PI);
        ctx.stroke();
      };
      drawVein('#fbfaf3', 5.2);
      drawVein('#e2483d', 2.6);
      ctx.restore();
    }

    // 2) Impact starburst ('bang'), dust puffs ('dust'), and sleepy 'Z' bubbles ('z')
    for (const fx of this.fxParticles) {
      const age = now - fx.createdAt;
      const u = Math.max(0, Math.min(1, age / fx.lifetime));
      ctx.save();
      if (fx.kind === 'bang') {
        const s = fx.size * (0.55 + 0.65 * Math.min(1, u / 0.35));
        const alpha = u < 0.45 ? 1 : Math.max(0, 1 - (u - 0.45) / 0.55);
        ctx.globalAlpha = alpha;
        ctx.translate(fx.x, fx.y);
        ctx.beginPath();
        for (let i = 0; i < 16; i++) {
          const a = (i / 16) * Math.PI * 2;
          const rad = i % 2 === 0 ? s : s * 0.46;
          if (i === 0) ctx.moveTo(Math.cos(a) * rad, Math.sin(a) * rad);
          else ctx.lineTo(Math.cos(a) * rad, Math.sin(a) * rad);
        }
        ctx.closePath();
        ctx.fillStyle = '#fbfaf3';
        ctx.strokeStyle = '#1d1d1b';
        ctx.lineWidth = 2.0;
        ctx.fill();
        ctx.stroke();
      } else if (fx.kind === 'dust') {
        const r = fx.size * (0.65 + 0.85 * u);
        ctx.globalAlpha = 0.85 * (1 - u);
        ctx.fillStyle = '#f4f1e6';
        ctx.strokeStyle = '#b9b4a4';
        ctx.lineWidth = 1.4;
        ctx.beginPath();
        ctx.arc(fx.x, fx.y, r, 0, Math.PI * 2);
        ctx.fill();
        ctx.stroke();
      } else if (fx.kind === 'z') {
        const swayX = Math.sin(age * 0.0028) * 6.5;
        const sc = 0.72 + u * 0.55;
        const alpha = Math.min(1, u / 0.16) * Math.max(0, Math.min(1, (1 - u) / 0.25));
        ctx.globalAlpha = alpha;
        ctx.translate(fx.x + swayX, fx.y);
        ctx.scale(sc, sc);
        ctx.font = '800 18px "Comic Sans MS", "Segoe UI", system-ui, sans-serif';
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.lineJoin = 'round';
        ctx.lineWidth = 4.2;
        ctx.strokeStyle = '#1d1d1b';
        ctx.strokeText('Z', 0, 0);
        ctx.fillStyle = '#fbfaf3';
        ctx.fillText('Z', 0, 0);
      }
      ctx.restore();
    }
  }

  private drawTail(ctx: CanvasRenderingContext2D): void {
    // Exact ORIG 0.3s resting tail S-hook, smoothly morphing into:
    //  1) playful arched swan-neck pat onto near-tail keys (catTail.reach),
    //  2) grumpy 3-beat rhythmic desk-thump (crossAmount),
    //  3) cozy tucked sleeping curl (sleepAmount), and
    //  4) subtle curious tip twitch / idle fidget flick!
    const now = performance.now();
    const act = this.idleAct;
    const actU = act ? Math.max(0, Math.min(1, (now - act.t0) / act.dur)) : 0;
    const tailFlick = act && act.kind === 'tail' ? Math.sin(actU * Math.PI * 2) * 7.5 : 0;
    const curiousTwitch =
      this.curiousAmount > 0.05
        ? Math.sin(this.elapsed * 14.5) * 4.2 * Math.min(1, this.curiousAmount)
        : 0;
    const swayAmp = (1.4 + this.typingEnergy * 2.2) * (1 - this.sleepAmount * 0.85);
    const gentleDrift = Math.sin(this.elapsed * (0.9 + this.typingEnergy * 1.4)) * swayAmp;
    const hipRight = this.spinePoint(0.84, 0);

    const startX = hipRight.x + 56;
    const startY = hipRight.y - 2 + this.sleepAmount * 14;
    let restCp1X = startX + 34;
    let restCp1Y = startY + 4 + gentleDrift * 0.25;
    let restCp2X = startX + 38;
    let restCp2Y = startY - 34 + gentleDrift * 0.45;
    let restEndX = startX + 70 + gentleDrift * 0.35 + tailFlick * 0.4;
    let restEndY = startY - 35 + gentleDrift + tailFlick + curiousTwitch;

    // Sleepy cozy curl: tucks low and forward beside the cat's hip
    if (this.sleepAmount > 0.01) {
      const sl = this.sleepAmount;
      const slCp1X = startX + 42;
      const slCp1Y = startY + 14;
      const slCp2X = startX + 54;
      const slCp2Y = startY + 28;
      const slEndX = startX + 26;
      const slEndY = startY + 26;
      restCp1X += (slCp1X - restCp1X) * sl;
      restCp1Y += (slCp1Y - restCp1Y) * sl;
      restCp2X += (slCp2X - restCp2X) * sl;
      restCp2Y += (slCp2Y - restCp2Y) * sl;
      restEndX += (slEndX - restEndX) * sl;
      restEndY += (slEndY - restEndY) * sl;
    }

    // Grumpy 3-beat desk-thump pose when cross (lifts up smoothly, slams down onto desk!)
    const r = Math.max(0, Math.min(1, this.catTail.reach));
    const smoothR = r * r * (3 - 2 * r);
    const thumpBlend = this.crossAmount * (1 - smoothR);
    if (thumpBlend > 0.01) {
      const lift = this.thumpLift;
      const lash = this.thumpLash;
      const maxEdge = this.logicalW - 18;
      const thCp1X = Math.min(maxEdge, startX + 36);
      const thCp1Y = startY + 14 - lift * 16;
      const thCp2X = Math.min(maxEdge, startX + 64 - lift * 10);
      const thCp2Y = startY + 24 - lift * 52;
      const thEndX = Math.min(maxEdge, startX + 78 - lift * 16 + lash * 6);
      const thEndY = startY + 30 - lift * 76;
      restCp1X += (thCp1X - restCp1X) * thumpBlend;
      restCp1Y += (thCp1Y - restCp1Y) * thumpBlend;
      restCp2X += (thCp2X - restCp2X) * thumpBlend;
      restCp2Y += (thCp2Y - restCp2Y) * thumpBlend;
      restEndX += (thEndX - restEndX) * thumpBlend;
      restEndY += (thEndY - restEndY) * thumpBlend;
    }

    const patEndX = this.catTail.tipX;
    const patEndY = this.catTail.tipY - this.catTail.hover - 2;
    const maxEdgeX = this.logicalW - 14;
    // Near-right keys keep the iconic high swan-neck arch; far-left keys sweep smoothly around the
    // right hip and glide across the front keyboard deck below the cat's belly (zero body clipping!).
    const crossLeft = Math.max(0, Math.min(1, (startX - 10 - patEndX) / (this.kbWidth * 0.65)));
    const nearCp1X = Math.min(maxEdgeX, Math.max(startX + 44, patEndX + 32));
    const nearCp1Y = startY - 12 - this.catTail.hover * 0.35;
    const nearCp2X = Math.min(maxEdgeX, Math.max(startX + 54, patEndX + 40));
    const nearCp2Y = Math.min(startY + 24, (startY + patEndY) * 0.5 - 30 - this.catTail.hover * 0.55);

    const farCp1X = Math.min(maxEdgeX, startX + 56);
    const farCp1Y = startY + 36 - this.catTail.hover * 0.25;
    const farCp2X = (startX + patEndX) * 0.5 + 24;
    const farCp2Y = Math.max(this.kbCenterY - this.kbHeight * 0.34, patEndY - 22 - this.catTail.hover * 0.45);

    const patCp1X = nearCp1X + (farCp1X - nearCp1X) * crossLeft;
    const patCp1Y = nearCp1Y + (farCp1Y - nearCp1Y) * crossLeft;
    const patCp2X = nearCp2X + (farCp2X - nearCp2X) * crossLeft;
    const patCp2Y = nearCp2Y + (farCp2Y - nearCp2Y) * crossLeft;

    const cp1X = restCp1X + (patCp1X - restCp1X) * smoothR;
    const cp1Y = restCp1Y + (patCp1Y - restCp1Y) * smoothR;
    const cp2X = restCp2X + (patCp2X - restCp2X) * smoothR;
    const cp2Y = restCp2Y + (patCp2Y - restCp2Y) * smoothR;
    const endX = restEndX + (patEndX - restEndX) * smoothR;
    const endY = restEndY + (patEndY - restEndY) * smoothR;
    const tipAngle = Math.atan2(endY - cp2Y, endX - cp2X);
    const tipSquash = this.catTail.impactSquash * smoothR;

    ctx.save();
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';

    // Subtle tactile contact shadow directly on the keycap when the tail pats down!
    if (smoothR > 0.35) {
      const prox = Math.max(0, 1 - this.catTail.hover / 28);
      const shadowA = smoothR * prox * 0.34;
      if (shadowA > 0.02) {
        ctx.fillStyle = `rgba(18, 22, 32, ${shadowA.toFixed(3)})`;
        ctx.beginPath();
        ctx.ellipse(
          this.catTail.tipX,
          this.catTail.tipY + 3.5,
          12.5 + tipSquash * 3.5,
          6.2 + tipSquash * 1.4,
          0,
          0,
          Math.PI * 2,
        );
        ctx.fill();
      }
    }

    if (this.customSkin) {
      const s = this.customSkin;
      const tailW = s.tailFluffy ? 20.0 : 16.0;
      if (s.outlineColor) {
        ctx.shadowColor = 'rgba(30, 32, 40, 0.22)';
        ctx.shadowBlur = 4;
        ctx.shadowOffsetY = 1.5;
        ctx.strokeStyle = s.outlineColor;
        ctx.lineWidth = tailW + 3.4;
        ctx.beginPath();
        ctx.moveTo(startX, startY);
        ctx.bezierCurveTo(cp1X, cp1Y, cp2X, cp2Y, endX, endY);
        ctx.stroke();
        ctx.shadowBlur = 0;
        ctx.shadowOffsetY = 0;
      } else {
        ctx.shadowColor = 'rgba(18, 17, 18, 0.42)';
        ctx.shadowBlur = 1.6;
      }

      const tailGrad = ctx.createLinearGradient(startX, startY, endX, endY);
      tailGrad.addColorStop(0.0, s.tailColor);
      tailGrad.addColorStop(1.0, s.tailTipColor);
      ctx.strokeStyle = tailGrad;
      ctx.lineWidth = tailW;
      ctx.beginPath();
      ctx.moveTo(startX, startY);
      ctx.bezierCurveTo(cp1X, cp1Y, cp2X, cp2Y, endX, endY);
      ctx.stroke();

      if (s.tailFluffy) {
        ctx.fillStyle = s.tailTipColor;
        ctx.beginPath();
        ctx.ellipse(
          endX + Math.cos(tipAngle) * 1.2,
          endY + Math.sin(tipAngle) * 1.2,
          11.8 * (1 + tipSquash * 0.14),
          10.2 * (1 - tipSquash * 0.10),
          tipAngle,
          0,
          Math.PI * 2,
        );
        ctx.fill();
      } else if (tipSquash > 0.05) {
        ctx.fillStyle = s.tailTipColor;
        ctx.beginPath();
        ctx.ellipse(
          endX,
          endY,
          tailW * 0.54 * (1 + tipSquash * 0.16),
          tailW * 0.50 * (1 - tipSquash * 0.12),
          tipAngle,
          0,
          Math.PI * 2,
        );
        ctx.fill();
      }

      // Cute tabby tail rings when tailStripes is enabled!
      if (s.tailStripes && (s.stripeColor || s.tailTipColor)) {
        const ringColor = s.stripeColor || s.tailTipColor;
        ctx.strokeStyle = ringColor;
        ctx.lineWidth = 3.2;
        ctx.shadowBlur = 0;
        for (const t of [0.24, 0.44, 0.64, 0.82]) {
          const mt = 1 - t;
          const bx =
            mt * mt * mt * startX +
            3 * mt * mt * t * cp1X +
            3 * mt * t * t * cp2X +
            t * t * t * endX;
          const by =
            mt * mt * mt * startY +
            3 * mt * mt * t * cp1Y +
            3 * mt * t * t * cp2Y +
            t * t * t * endY;
          const tx =
            3 * mt * mt * (cp1X - startX) +
            6 * mt * t * (cp2X - cp1X) +
            3 * t * t * (endX - cp2X);
          const ty =
            3 * mt * mt * (cp1Y - startY) +
            6 * mt * t * (cp2Y - cp1Y) +
            3 * t * t * (endY - cp2Y);
          const tLen = Math.max(1, Math.hypot(tx, ty));
          const nx = (-ty / tLen) * (tailW * 0.42);
          const ny = (tx / tLen) * (tailW * 0.42);
          ctx.beginPath();
          ctx.moveTo(bx - nx, by - ny);
          ctx.lineTo(bx + nx, by + ny);
          ctx.stroke();
        }
      }

      ctx.restore();
      return;
    }

    if (this.catBreed === 'ragdoll') {
      // Plush bushy warm cocoa-taupe Ragdoll plume tail matching the real cat photo!
      ctx.shadowColor = 'rgba(48, 38, 30, 0.22)';
      ctx.shadowBlur = 4;
      ctx.shadowOffsetY = 1.5;
      ctx.strokeStyle = '#4e4239';
      ctx.lineWidth = 23.5;
      ctx.beginPath();
      ctx.moveTo(startX, startY);
      ctx.bezierCurveTo(cp1X, cp1Y, cp2X, cp2Y, endX, endY);
      ctx.stroke();

      ctx.shadowBlur = 0;
      ctx.shadowOffsetY = 0;
      const tailGrad = ctx.createLinearGradient(startX, startY, endX, endY);
      tailGrad.addColorStop(0.0, '#8d7e6f');
      tailGrad.addColorStop(0.5, '#726356');
      tailGrad.addColorStop(1.0, '#584b40');
      ctx.strokeStyle = tailGrad;
      ctx.lineWidth = 20.2;
      ctx.beginPath();
      ctx.moveTo(startX, startY);
      ctx.bezierCurveTo(cp1X, cp1Y, cp2X, cp2Y, endX, endY);
      ctx.stroke();

      // Bushy warm cocoa-taupe plume tip puff
      ctx.fillStyle = '#584b40';
      ctx.beginPath();
      ctx.ellipse(
        endX + Math.cos(tipAngle) * 1.2,
        endY + Math.sin(tipAngle) * 1.2,
        12.2 * (1 + tipSquash * 0.14),
        10.6 * (1 - tipSquash * 0.10),
        tipAngle,
        0,
        Math.PI * 2,
      );
      ctx.fill();

      // Soft warm creamy sheen along upper fluff curve
      ctx.strokeStyle = 'rgba(246, 240, 232, 0.26)';
      ctx.lineWidth = 5.0;
      ctx.beginPath();
      ctx.moveTo(startX + 4, startY - 4);
      ctx.bezierCurveTo(cp1X - 2, cp1Y - 4, cp2X - 3, cp2Y - 4, endX - 3, endY - 4);
      ctx.stroke();
    } else {
      ctx.shadowColor = 'rgba(18, 17, 18, 0.45)';
      ctx.shadowBlur = 1.6;
      ctx.strokeStyle = '#141314';
      ctx.lineWidth = 16.0;
      ctx.beginPath();
      ctx.moveTo(startX, startY);
      ctx.bezierCurveTo(cp1X, cp1Y, cp2X, cp2Y, endX, endY);
      ctx.stroke();

      if (tipSquash > 0.05) {
        ctx.fillStyle = '#141314';
        ctx.beginPath();
        ctx.ellipse(
          endX,
          endY,
          8.8 * (1 + tipSquash * 0.16),
          8.0 * (1 - tipSquash * 0.12),
          tipAngle,
          0,
          Math.PI * 2,
        );
        ctx.fill();
      }
    }

    ctx.restore();
  }

  private drawCatBody(ctx: CanvasRenderingContext2D, breath: number, purrShift: number): void {
    if (this.customSkin) {
      this.drawCustomCatBody(ctx, this.customSkin, breath, purrShift);
      return;
    }
    const isRagdoll = this.catBreed === 'ragdoll';
    const catBlack = '#141314';
    const inkOutline = '#5d5148';

    // Evaluate measured left & right silhouette points deformed by flexible cat spine + unified head tilt!
    const leftPts: Vec2[] = [];
    const rightPts: Vec2[] = [];
    const bellySwell = 1 + this.typingHunch * 0.04 + this.sleepAmount * 0.085;
    const headCenter = this.spinePoint(0.24, breath);
    const hcx = headCenter.x + purrShift;
    const hcy = headCenter.y;

    for (const slice of BODY_SLICES) {
      const sp = this.spinePoint(slice.t, breath);
      const widen = slice.t > 0.45 ? bellySwell : 1;
      const hw = slice.hw * widen;

      if (slice.t <= 0.38) {
        const tiltW = slice.t <= 0.30 ? 1.0 : (0.42 - slice.t) / 0.12;
        const ang = this.headTilt * tiltW;
        const cosA = Math.cos(ang);
        const sinA = Math.sin(ang);
        const relY = sp.y - hcy;
        const lx0 = sp.x + purrShift - hw - hcx;
        const rx0 = sp.x + purrShift + hw - hcx;
        leftPts.push({
          x: hcx + lx0 * cosA - relY * sinA,
          y: hcy + lx0 * sinA + relY * cosA,
        });
        rightPts.push({
          x: hcx + rx0 * cosA - relY * sinA,
          y: hcy + rx0 * sinA + relY * cosA,
        });
      } else {
        leftPts.push({ x: sp.x + purrShift - hw, y: sp.y });
        rightPts.push({ x: sp.x + purrShift + hw, y: sp.y });
      }
    }

    const crownRelY = this.spinePoint(0.11, breath).y - hcy;
    const crownPt = {
      x: hcx - crownRelY * Math.sin(this.headTilt),
      y: hcy + crownRelY * Math.cos(this.headTilt),
    };
    const basePt = this.spinePoint(1.0, breath);

    ctx.save();
    if (isRagdoll) {
      ctx.shadowColor = 'rgba(26, 30, 40, 0.24)';
      ctx.shadowBlur = 5;
      ctx.shadowOffsetY = 2;
    } else {
      ctx.shadowColor = 'rgba(18, 17, 18, 0.42)';
      ctx.shadowBlur = 1.6;
    }

    // 1) Cute rounded grounded hind paws always rendered in seated posture (no idle disappear/blink; Fixes #1)
    ctx.fillStyle = isRagdoll ? '#f7f5f0' : catBlack;
    ctx.strokeStyle = inkOutline;
    ctx.lineWidth = 1.9;
    for (const [fx, fy, rx, ry] of [
      [basePt.x - 42, basePt.y - 3, 14.5, 11.5],
      [basePt.x + 32, basePt.y - 1, 15.0, 11.5],
    ]) {
      ctx.beginPath();
      ctx.ellipse(fx, fy, rx, ry, 0, 0, Math.PI * 2);
      ctx.fill();
      if (isRagdoll) {
        ctx.save();
        ctx.shadowBlur = 0;
        ctx.shadowOffsetY = 0;
        ctx.stroke();
        ctx.restore();
      }
    }

    // 2) Build exact smooth spline through measured silhouette slices (left top -> right top -> right down -> bottom -> left up)!
    const traceBodyPath = () => {
      ctx.beginPath();
      const crownCos = Math.cos(this.headTilt);
      const crownSin = Math.sin(this.headTilt);
      ctx.moveTo(leftPts[0].x + 14 * crownCos + 6 * crownSin, leftPts[0].y + 14 * crownSin - 6 * crownCos);
      ctx.quadraticCurveTo(crownPt.x, crownPt.y - 2, rightPts[0].x - 14 * crownCos + 6 * crownSin, rightPts[0].y - 14 * crownSin - 6 * crownCos);

      ctx.lineTo(rightPts[0].x, rightPts[0].y);
      for (let i = 0; i < rightPts.length - 1; i++) {
        const p0 = rightPts[i];
        const p1 = rightPts[i + 1];
        const mx = (p0.x + p1.x) * 0.5;
        const my = (p0.y + p1.y) * 0.5;
        ctx.quadraticCurveTo(p0.x, p0.y, mx, my);
      }
      const lastR = rightPts[rightPts.length - 1];
      ctx.lineTo(lastR.x, lastR.y);

      const lastL = leftPts[leftPts.length - 1];
      ctx.quadraticCurveTo(basePt.x + purrShift, basePt.y + 6, lastL.x, lastL.y);

      for (let i = leftPts.length - 1; i > 0; i--) {
        const p0 = leftPts[i];
        const p1 = leftPts[i - 1];
        const mx = (p0.x + p1.x) * 0.5;
        const my = (p0.y + p1.y) * 0.5;
        ctx.quadraticCurveTo(p0.x, p0.y, mx, my);
      }
      ctx.lineTo(leftPts[0].x, leftPts[0].y);
      ctx.closePath();
    };

    traceBodyPath();
    if (isRagdoll) {
      // Warm creamy ivory base coat matching the real fluffy Ragdoll photo!
      const furGrad = ctx.createRadialGradient(hcx, hcy + 68, 18, basePt.x, basePt.y - 60, 148);
      furGrad.addColorStop(0.0, '#fbf9f3');
      furGrad.addColorStop(0.68, '#f4f0e6');
      furGrad.addColorStop(1.0, '#e8e2d6');
      ctx.fillStyle = furGrad;
      ctx.fill();

      // Paint custom warm milk-tea taupe Ragdoll markings clipped cleanly inside the body silhouette!
      ctx.save();
      ctx.shadowBlur = 0;
      ctx.shadowOffsetY = 0;
      ctx.clip();

      // 2a) Plush warm taupe-brown lower flanks / hips wrapping around both outer thighs (matches photo!)
      const midTorso = this.spinePoint(0.72, breath);
      for (const sgn of [-1, 1]) {
        const flankGrad = ctx.createRadialGradient(
          midTorso.x + purrShift + sgn * 70,
          midTorso.y + 8,
          14,
          midTorso.x + purrShift + sgn * 50,
          midTorso.y + 10,
          72,
        );
        flankGrad.addColorStop(0.0, 'rgba(130, 115, 100, 0.78)');
        flankGrad.addColorStop(0.50, 'rgba(156, 141, 126, 0.45)');
        flankGrad.addColorStop(0.82, 'rgba(178, 165, 150, 0.14)');
        flankGrad.addColorStop(1.0, 'rgba(190, 178, 164, 0.0)');
        ctx.fillStyle = flankGrad;
        ctx.beginPath();
        ctx.ellipse(midTorso.x + purrShift + sgn * 60, midTorso.y + 8, 52, 84, sgn * -0.14, 0, Math.PI * 2);
        ctx.fill();
      }

      // 2b) Subtle fluffy creamy-white chest lion-mane fur tufts under chin
      const neckSp = this.spinePoint(0.47, breath);
      ctx.strokeStyle = 'rgba(182, 170, 156, 0.36)';
      ctx.lineWidth = 1.8;
      ctx.lineCap = 'round';
      ctx.beginPath();
      ctx.arc(neckSp.x + purrShift, neckSp.y - 2, 34, 0.22 * Math.PI, 0.78 * Math.PI);
      ctx.stroke();
      ctx.beginPath();
      ctx.arc(neckSp.x + purrShift - 16, neckSp.y + 18, 14, 0.18 * Math.PI, 0.72 * Math.PI);
      ctx.arc(neckSp.x + purrShift + 16, neckSp.y + 18, 14, 0.28 * Math.PI, 0.82 * Math.PI);
      ctx.stroke();

      // 2c) Custom warm milk-tea taupe crown cap & wide fluffy creamy-white blaze (1:1 match to user's real Ragdoll photos!)
      ctx.save();
      ctx.translate(hcx, hcy);
      ctx.rotate(this.headTilt);

      const maskShift = this.faceTurnX * 0.72;
      // Rich warm latte-taupe head mask covering crown, ears base, temples & outer cheeks
      const capGrad = ctx.createLinearGradient(0, -40, 0, 22);
      capGrad.addColorStop(0.0, '#8d7d6e');
      capGrad.addColorStop(0.42, '#9e8e7f');
      capGrad.addColorStop(0.84, '#b3a495');
      capGrad.addColorStop(1.0, 'rgba(186, 173, 158, 0.14)');

      ctx.fillStyle = capGrad;
      ctx.shadowColor = 'rgba(138, 122, 108, 0.45)';
      ctx.shadowBlur = 4.8;

      // Full warm milk-tea head mask curving around outer eyes & cheeks, opening into a wide soft fluffy white blaze!
      ctx.beginPath();
      ctx.moveTo(-74, -44);
      ctx.lineTo(74, -44);
      ctx.lineTo(70, 20);
      ctx.quadraticCurveTo(maskShift + 42, 18, maskShift + 26, 9);
      ctx.quadraticCurveTo(maskShift + 17, 0, maskShift + 13.5, -13);
      // Soft wide fluffy dome peak of the creamy-white blaze between the brows (warm latte bridge stays connected across forehead!)
      ctx.quadraticCurveTo(maskShift + 7, -21.5, maskShift, -22.5);
      ctx.quadraticCurveTo(maskShift - 7, -21.5, maskShift - 13.5, -13);
      ctx.quadraticCurveTo(maskShift - 17, 0, maskShift - 26, 9);
      ctx.quadraticCurveTo(maskShift - 42, 18, -70, 20);
      ctx.closePath();
      ctx.fill();

      // Lighter warm champagne-beige soft forehead bridge highlight right between the ears (exact match to close-up photo!)
      ctx.shadowBlur = 5.5;
      ctx.fillStyle = 'rgba(208, 194, 176, 0.58)';
      ctx.beginPath();
      ctx.ellipse(maskShift, -32, 18, 9, 0, 0, Math.PI * 2);
      ctx.fill();

      // Deeper warm taupe-brown shading along outer temples below both ears
      ctx.fillStyle = 'rgba(116, 100, 86, 0.40)';
      ctx.beginPath();
      ctx.ellipse(-44 + maskShift * 0.4, -10, 21, 22, -0.18, 0, Math.PI * 2);
      ctx.ellipse(44 + maskShift * 0.4, -10, 21, 22, 0.18, 0, Math.PI * 2);
      ctx.fill();

      ctx.restore(); // end mask head-space
      ctx.restore(); // end body clip

      // Clean refined soft warm-brown body outline so creamy fur pops crisply on BOTH light & dark wallpapers!
      ctx.save();
      ctx.shadowBlur = 0;
      ctx.shadowOffsetY = 0;
      traceBodyPath();
      ctx.strokeStyle = inkOutline;
      ctx.lineWidth = 1.8;
      ctx.lineJoin = 'round';
      ctx.stroke();
      ctx.restore();
    } else {
      ctx.fillStyle = catBlack;
      ctx.fill();
    }

    // 3) Compact plush rounded cat ears matching ORIG 0.3s exact height (shorter & cuter, no tall rabbit ears!)
    ctx.save();
    ctx.translate(hcx, hcy);
    ctx.rotate(this.headTilt);

    const nowEar = performance.now();
    const earAct = this.idleAct;
    const earActU = earAct && earAct.kind === 'ear' ? Math.max(0, Math.min(1, (nowEar - earAct.t0) / earAct.dur)) : 0;
    const lFlick = earAct && earAct.kind === 'ear' && earAct.side === -1 ? Math.sin(earActU * Math.PI * 3) * 0.24 * (1 - earActU) : 0;
    const rFlick = earAct && earAct.kind === 'ear' && earAct.side === 1 ? Math.sin(earActU * Math.PI * 3) * 0.24 * (1 - earActU) : 0;
    const lTwitch = Math.sin(this.earTwitchLeft * Math.PI) * 0.12 + lFlick;
    const rTwitch = Math.sin(this.earTwitchRight * Math.PI) * 0.12 + rFlick;
    const earPerk = this.earPerkSpring.x; // +0.16 curious/perky, -0.58 airplane ears!
    const airplaneDrop = Math.max(0, -earPerk) * 11;
    const airplaneSpread = Math.max(0, -earPerk) * 6;

    const drawOneEar = (sideSign: -1 | 1, twitch: number) => {
      ctx.save();
      ctx.translate(sideSign * (33 + airplaneSpread) + this.faceTurnX * 0.18, -28 + airplaneDrop);
      ctx.rotate(sideSign * (0.08 - earPerk + twitch));

      const earPath = () => {
        ctx.beginPath();
        ctx.moveTo(sideSign * 19, 20);
        ctx.bezierCurveTo(sideSign * 19, -8, sideSign * 12, -23, sideSign * 3, -21);
        ctx.bezierCurveTo(sideSign * -8, -18, sideSign * -16, -6, sideSign * -21, 8);
        ctx.closePath();
      };

      if (isRagdoll) {
        // Rich dark espresso-taupe ear rims fading to soft warm latte base (1:1 match to user's real cat!)
        const earGrad = ctx.createLinearGradient(0, -23, 0, 18);
        earGrad.addColorStop(0.0, '#463b33');
        earGrad.addColorStop(0.42, '#625449');
        earGrad.addColorStop(0.82, '#8b7c6e');
        earGrad.addColorStop(1.0, '#9c8d7e');
        ctx.fillStyle = earGrad;
        earPath();
        ctx.fill();

        // Stroke outer ear rim cleanly (open at bottom skull merge so there is zero seam line!)
        ctx.save();
        ctx.shadowBlur = 0;
        ctx.shadowOffsetY = 0;
        ctx.strokeStyle = '#473c34';
        ctx.lineWidth = 1.9;
        ctx.lineCap = 'round';
        ctx.beginPath();
        ctx.moveTo(sideSign * 19, 17);
        ctx.bezierCurveTo(sideSign * 19, -8, sideSign * 12, -23, sideSign * 3, -21);
        ctx.bezierCurveTo(sideSign * -8, -18, sideSign * -16, -6, sideSign * -19, 5);
        ctx.stroke();

        // Soft warm dusty-blush cream inner ear
        ctx.fillStyle = '#dec4be';
        ctx.beginPath();
        ctx.moveTo(sideSign * 11, 10);
        ctx.bezierCurveTo(sideSign * 11, -3, sideSign * 7, -13, sideSign * 2, -11);
        ctx.bezierCurveTo(sideSign * -4, -9, sideSign * -9, -1, sideSign * -10, 8);
        ctx.closePath();
        ctx.fill();

        // Fluffy creamy-white Ragdoll ear furnishings
        ctx.strokeStyle = 'rgba(252, 249, 242, 0.90)';
        ctx.lineWidth = 1.6;
        ctx.beginPath();
        ctx.moveTo(sideSign * -7, 6);
        ctx.quadraticCurveTo(sideSign * -1, 3, sideSign * 5, 6);
        ctx.moveTo(sideSign * -4, 10);
        ctx.quadraticCurveTo(sideSign * 2, 7.5, sideSign * 8, 10);
        ctx.stroke();
        ctx.restore();
      } else {
        ctx.fillStyle = catBlack;
        earPath();
        ctx.fill();
      }
      ctx.restore();
    };

    this.drawHeadphonesBack(ctx);
    drawOneEar(-1, lTwitch);
    drawOneEar(1, rTwitch);

    ctx.shadowBlur = 0;
    ctx.shadowOffsetY = 0;

    const wShift = this.faceTurnX * 0.72;

    // 4) Ragdoll rosy petting blush + adorable tiny pink nose & gentle kitten smile!
    if (isRagdoll) {
      if (this.squintAmount > 0.12) {
        ctx.fillStyle = `rgba(244, 146, 166, ${(0.42 * this.squintAmount).toFixed(3)})`;
        ctx.beginPath();
        ctx.ellipse(wShift - 30, 13, 8.5, 4.6, -0.08, 0, Math.PI * 2);
        ctx.ellipse(wShift + 30, 13, 8.5, 4.6, 0.08, 0, Math.PI * 2);
        ctx.fill();
      }

      // Cute rosy-pink Ragdoll nose centered neatly inside the snow-white inverted-V blaze
      ctx.fillStyle = '#ee8e9e';
      ctx.beginPath();
      ctx.moveTo(wShift - 4.1, 9.6);
      ctx.quadraticCurveTo(wShift, 8.5, wShift + 4.1, 9.6);
      ctx.quadraticCurveTo(wShift + 3.2, 13.8, wShift, 14.3);
      ctx.quadraticCurveTo(wShift - 3.2, 13.8, wShift - 4.1, 9.6);
      ctx.closePath();
      ctx.fill();

      // Delicate cute kitten "w" mouth
      ctx.strokeStyle = 'rgba(72, 77, 92, 0.80)';
      ctx.lineWidth = 1.35;
      ctx.lineCap = 'round';
      ctx.beginPath();
      ctx.moveTo(wShift - 5.8, 16.6);
      ctx.quadraticCurveTo(wShift - 2.9, 19.6, wShift, 16.4);
      ctx.quadraticCurveTo(wShift + 2.9, 19.6, wShift + 5.8, 16.6);
      ctx.stroke();
    }

    // 5) Delicate wispy cheek whiskers adapting to 2.5D head turn
    ctx.strokeStyle = isRagdoll ? 'rgba(98, 86, 76, 0.52)' : 'rgba(205, 208, 214, 0.54)';
    ctx.lineWidth = isRagdoll ? 1.15 : 1.05;
    ctx.lineCap = 'round';
    const leftLen = Math.max(8, 12 - this.faceTurnX * 0.55);
    const rightLen = Math.max(8, 12 + this.faceTurnX * 0.55);
    for (let i = 0; i < 3; i++) {
      const wy = 16 + i * 5.0;
      const slant = 2.0 + i * 1.1;
      ctx.beginPath();
      ctx.moveTo(-34 + wShift, wy);
      ctx.lineTo(-34 + wShift - leftLen, wy + slant);
      ctx.stroke();

      ctx.beginPath();
      ctx.moveTo(34 + wShift, wy);
      ctx.lineTo(34 + wShift + rightLen, wy + slant);
      ctx.stroke();
    }

    // 6) Expressive eyes (lemon-chartreuse for Black Cat / luminous sapphire-blue for Ragdoll!)
    this.drawEyes(ctx, wShift, -1);
    this.drawHeadphonesFront(ctx);

    ctx.restore(); // end head

    ctx.restore();
  }

  private getHeadphoneLedColors(): { ring: string; glow: string; mic: string } {
    const rgbMix = 0.5 + 0.5 * Math.sin(this.elapsed * 2.3 + this.typingEnergy * 2.0);
    const pulse = 0.88 + 0.12 * Math.sin(this.elapsed * 5.2) + this.typingEnergy * 0.14;
    const t = rgbMix * 0.58;
    const r = Math.min(255, Math.round((212 * (1 - t) + 61 * t) * pulse));
    const g = Math.min(255, Math.round((245 * (1 - t) + 242 * t) * pulse));
    const b = Math.min(255, Math.round((61 * (1 - t) + 232 * t) * pulse));
    return {
      ring: `rgb(${r}, ${g}, ${b})`,
      glow: `rgba(${r}, ${g}, ${b}, 0.58)`,
      mic: `rgba(82, 247, 212, ${(0.82 + 0.18 * Math.sin(this.elapsed * 3.8)).toFixed(2)})`,
    };
  }

  /** Pass 1 (behind ears): sleek arched padded gaming headband + titanium sliders! */
  private drawHeadphonesBack(ctx: CanvasRenderingContext2D): void {
    const hs = Math.max(0, Math.min(1.18, this.headsetSpring.x));
    if (hs <= 0.015) return;

    const settled = Math.min(1, hs);
    const dropY = -(1 - settled) * 38;
    const sc = 0.86 + 0.14 * settled + Math.max(0, hs - 1) * 0.12;

    ctx.save();
    ctx.globalAlpha = Math.min(1, settled * 1.45);
    ctx.translate(0, dropY);
    ctx.scale(sc, sc);
    ctx.shadowBlur = 0;
    ctx.shadowOffsetY = 0;
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';

    const traceArch = (inset = 0) => {
      ctx.beginPath();
      ctx.moveTo(-64 + inset * 0.7, 4);
      ctx.bezierCurveTo(-66 + inset * 0.7, -34 + inset, -34, -56 + inset, 0, -56 + inset);
      ctx.bezierCurveTo(34, -56 + inset, 66 - inset * 0.7, -34 + inset, 64 - inset * 0.7, 4);
    };

    // Subtle light specular outline so headband reads crisply even on pure Black Cat!
    traceArch(0);
    ctx.strokeStyle = '#6f7e99';
    ctx.lineWidth = 12.2;
    ctx.stroke();

    // Matte-charcoal outer headband shell
    traceArch(0);
    ctx.strokeStyle = '#242a36';
    ctx.lineWidth = 10.0;
    ctx.stroke();

    // Soft top satin highlight along crown
    ctx.beginPath();
    ctx.moveTo(-26, -54.2);
    ctx.quadraticCurveTo(0, -58.5, 26, -54.2);
    ctx.strokeStyle = 'rgba(216, 228, 248, 0.36)';
    ctx.lineWidth = 2.0;
    ctx.stroke();

    // Plush under-crown inner foam pad
    ctx.beginPath();
    ctx.moveTo(-28, -48.5);
    ctx.quadraticCurveTo(0, -52.5, 28, -48.5);
    ctx.strokeStyle = '#3b4454';
    ctx.lineWidth = 5.4;
    ctx.stroke();

    // Brushed-titanium lower side slider brackets
    for (const s of [-1, 1] as const) {
      ctx.beginPath();
      ctx.moveTo(s * 62.5, -14);
      ctx.lineTo(s * 64.0, 2);
      ctx.strokeStyle = '#a2afc4';
      ctx.lineWidth = 6.2;
      ctx.stroke();
    }

    ctx.restore();
  }

  /** Pass 2 (in front of cheeks): plush over-ear cups, glowing LED rings & curved boom mic! */
  private drawHeadphonesFront(ctx: CanvasRenderingContext2D): void {
    const hs = Math.max(0, Math.min(1.18, this.headsetSpring.x));
    if (hs <= 0.015) return;

    const settled = Math.min(1, hs);
    const dropY = -(1 - settled) * 38;
    const sc = 0.86 + 0.14 * settled + Math.max(0, hs - 1) * 0.12;
    const led = this.getHeadphoneLedColors();
    const turnShift = this.faceTurnX * 0.25;

    ctx.save();
    ctx.globalAlpha = Math.min(1, settled * 1.45);
    ctx.translate(0, dropY);
    ctx.scale(sc, sc);
    ctx.shadowBlur = 0;
    ctx.shadowOffsetY = 0;

    for (const s of [-1, 1] as const) {
      ctx.save();
      ctx.translate(s * 60 + turnShift * 0.5, 5);
      ctx.rotate(s * 0.09);

      // Inner plush memory-foam cheek cushion
      ctx.fillStyle = '#384050';
      ctx.strokeStyle = '#1b1f29';
      ctx.lineWidth = 1.5;
      ctx.beginPath();
      ctx.ellipse(-s * 3.5, 0, 10.5, 19.5, 0, 0, Math.PI * 2);
      ctx.fill();
      ctx.stroke();

      // Domed matte-charcoal outer earcup shell
      const cupGrad = ctx.createLinearGradient(-10, -18, 10, 18);
      cupGrad.addColorStop(0.0, '#363e4f');
      cupGrad.addColorStop(0.55, '#242934');
      cupGrad.addColorStop(1.0, '#191d26');
      ctx.fillStyle = cupGrad;
      ctx.strokeStyle = '#8fa0bd';
      ctx.lineWidth = 1.6;
      ctx.beginPath();
      ctx.ellipse(s * 4.2, 0, 10.2, 18.2, 0, 0, Math.PI * 2);
      ctx.fill();
      ctx.stroke();

      // Reactive KuroCat-lime / electric-cyan gaming LED ring!
      ctx.save();
      ctx.shadowColor = led.glow;
      ctx.shadowBlur = 7;
      ctx.strokeStyle = led.ring;
      ctx.lineWidth = 2.4;
      ctx.beginPath();
      ctx.ellipse(s * 6.0, 0, 5.4, 11.4, 0, 0, Math.PI * 2);
      ctx.stroke();

      ctx.fillStyle = led.ring;
      ctx.beginPath();
      ctx.ellipse(s * 6.4, 0, 1.9, 2.8, 0, 0, Math.PI * 2);
      ctx.fill();
      ctx.restore();

      ctx.restore();
    }

    // Sleek curved flexible boom microphone from left earcup (-X) curving beside lower cheek
    const mx0 = -60 + turnShift * 0.5;
    const mxTip = -22 + this.faceTurnX * 0.55;
    ctx.save();
    ctx.lineCap = 'round';
    ctx.strokeStyle = '#5c6980';
    ctx.lineWidth = 4.6;
    ctx.beginPath();
    ctx.moveTo(mx0, 14);
    ctx.quadraticCurveTo(mx0 + 6, 29, mxTip, 23);
    ctx.stroke();

    ctx.strokeStyle = '#1f242f';
    ctx.lineWidth = 2.8;
    ctx.beginPath();
    ctx.moveTo(mx0, 14);
    ctx.quadraticCurveTo(mx0 + 6, 29, mxTip, 23);
    ctx.stroke();

    // Metallic swivel joint on earcup
    ctx.fillStyle = '#a2afc4';
    ctx.beginPath();
    ctx.arc(mx0, 14, 3.0, 0, Math.PI * 2);
    ctx.fill();

    // Plush charcoal foam windscreen capsule + glowing mic-live indicator dot
    ctx.fillStyle = '#333b4a';
    ctx.strokeStyle = '#73829c';
    ctx.lineWidth = 1.2;
    ctx.beginPath();
    ctx.ellipse(mxTip + 2.5, 22.4, 5.6, 3.8, -0.22, 0, Math.PI * 2);
    ctx.fill();
    ctx.stroke();

    ctx.fillStyle = led.mic;
    ctx.shadowColor = led.mic;
    ctx.shadowBlur = 5;
    ctx.beginPath();
    ctx.arc(mxTip + 6.2, 21.5, 1.7, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();

    ctx.restore();
  }

  private drawEyes(ctx: CanvasRenderingContext2D, cx: number, eyeY: number): void {
    const isRagdoll = this.catBreed === 'ragdoll';
    const nowEye = performance.now();
    const slowBlinkVal =
      this.idleAct && this.idleAct.kind === 'slowBlink'
        ? 0.92 * Math.sin(Math.max(0, Math.min(1, (nowEye - this.idleAct.t0) / this.idleAct.dur)) * Math.PI)
        : 0;
    const blinkVal = Math.max(this.isBlinking ? 1 - Math.abs(this.blinkProgress - 1) : 0, slowBlinkVal);
    const closedness = Math.max(blinkVal, this.squintAmount * 0.85, this.privacyClose, this.sleepAmount);
    const curious = Math.min(1, this.curiousAmount);
    const cross = this.crossAmount;

    // Exact measured eye spacing & size from original 0.3s & 10.6s!
    const eyeXs = [cx - 23.0, cx + 23.0];
    const rx = (14.8 + this.typingHunch * 1.0) * (1 + curious * 0.06);
    const ry = (13.4 - this.typingHunch * 0.5) * (1 + curious * 0.09);

    for (let idx = 0; idx < 2; idx++) {
      const ex = eyeXs[idx];
      ctx.save();
      ctx.translate(ex, eyeY);

      if (this.privacyClose > 0.45 || this.sleepAmount > 0.52) {
        ctx.strokeStyle = isRagdoll ? '#212533' : '#d4f044';
        ctx.lineWidth = 2.9;
        ctx.lineCap = 'round';
        ctx.beginPath();
        ctx.moveTo(-13.5, -0.5);
        ctx.quadraticCurveTo(0, 5.2, 13.5, -0.5);
        ctx.stroke();
        ctx.restore();
        continue;
      }

      if (this.squintAmount > 0.38 && blinkVal < 0.3) {
        if (isRagdoll) {
          // Sleek lazy drooping slits with a subtle crystal-blue underside shimmer!
          ctx.strokeStyle = 'rgba(174, 224, 255, 0.72)';
          ctx.lineWidth = 4.4;
          ctx.lineCap = 'round';
          ctx.beginPath();
          ctx.moveTo(-12.5, 0.2);
          ctx.quadraticCurveTo(0, 2.2, 12.5, 1.8);
          ctx.stroke();

          ctx.strokeStyle = '#212533';
          ctx.lineWidth = 2.7;
          ctx.beginPath();
          ctx.moveTo(-13.5, -0.5);
          ctx.quadraticCurveTo(0, 1.4, 13.5, 1.2);
          ctx.stroke();
        } else {
          // Exact sleek thin horizontal lazy-slanted lemon-chartreuse slits from user's reference photo (NO grey lid blob!)
          ctx.strokeStyle = '#d4f044';
          ctx.lineWidth = 2.9;
          ctx.lineCap = 'round';
          ctx.beginPath();
          ctx.moveTo(-13.5, -0.5);
          ctx.quadraticCurveTo(0, 1.4, 13.5, 1.2);
          ctx.stroke();
        }
        ctx.restore();
        continue;
      }

      const openRy = Math.max(1.2, ry * (1 - closedness));
      const curiousTilt = (idx === 0 ? -0.1 : 0.1) * (0.25 + this.typingHunch * 0.75);
      ctx.rotate(curiousTilt);

      const hasGrumpyLid = cross > 0.04;
      const innerDrop = -openRy + openRy * 0.90 * cross;
      const outerDrop = -openRy + openRy * 0.56 * cross;
      const leftDrop = idx === 0 ? outerDrop : innerDrop;
      const rightDrop = idx === 0 ? innerDrop : outerDrop;

      // Clip the eye cleanly below the slanted grumpy eyelid so underlying fur / bicolor masks show through seamlessly!
      ctx.save();
      if (hasGrumpyLid) {
        ctx.beginPath();
        ctx.moveTo(-rx - 4, leftDrop);
        ctx.lineTo(rx + 4, rightDrop);
        ctx.lineTo(rx + 4, openRy + 4);
        ctx.lineTo(-rx - 4, openRy + 4);
        ctx.closePath();
        ctx.clip();
      }

      if (isRagdoll) {
        // Gentle soulful sky-cerulean Ragdoll eyes (warm & lifelike, matching user's Ragdoll!)
        const irisGrad = ctx.createLinearGradient(0, -openRy, 0, openRy);
        irisGrad.addColorStop(0.0, '#2c5594');
        irisGrad.addColorStop(0.50, '#5a94d6');
        irisGrad.addColorStop(1.0, '#a3d2f2');
        ctx.fillStyle = irisGrad;
      } else {
        ctx.fillStyle = '#d4f044';
      }

      ctx.beginPath();
      ctx.ellipse(0, 0, rx, openRy, 0, 0, Math.PI * 2);
      ctx.fill();

      ctx.save();
      ctx.clip();

      if (openRy > 3.2) {
        // Dilated huge round pupils when Curious (laser-pointer mode), sharp narrow slits when Cross!
        const pupilRx = Math.min(rx - 2.4, (6.2 + curious * 4.2 + this.typingEnergy * 0.8) * (1 - cross * 0.34));
        const pupilRy = Math.min(openRy - 1.4, (8.8 + curious * 1.9) * (1 - cross * 0.08));
        // Scale normalized gaze (-1..+1) smoothly into available travel room (never sticks on a clamp wall!)
        const maxTravelX = Math.max(2.6, rx - pupilRx - 1.0);
        const maxTravelY = Math.max(2.2, openRy - pupilRy - 0.6);
        const px = Math.max(-maxTravelX, Math.min(maxTravelX, this.eyeLookX * maxTravelX));
        const py = Math.max(-maxTravelY, Math.min(maxTravelY, this.eyeLookY * maxTravelY));

        ctx.fillStyle = isRagdoll ? '#101624' : '#0e0e10';
        ctx.beginPath();
        ctx.ellipse(px, py, pupilRx, pupilRy, 0, 0, Math.PI * 2);
        ctx.fill();

        if (isRagdoll || curious > 0.25) {
          // Crisp glossy starlight catchlights (always on Ragdoll, plus sparkles on Black Cat when curious!)
          const ca = isRagdoll ? 0.94 : Math.min(0.92, curious * 0.95);
          ctx.fillStyle = `rgba(255, 255, 255, ${ca.toFixed(2)})`;
          ctx.beginPath();
          ctx.arc(px - 3.6 - curious * 0.8, py - 4.0 - curious * 0.5, 2.5 + curious * 0.7, 0, Math.PI * 2);
          ctx.fill();

          ctx.fillStyle = `rgba(255, 255, 255, ${(ca * 0.76).toFixed(2)})`;
          ctx.beginPath();
          ctx.arc(px + 3.2, py + 3.2, 1.2 + curious * 0.4, 0, Math.PI * 2);
          ctx.fill();
        }
      }

      ctx.restore();

      if (isRagdoll && openRy > 2.2) {
        // Delicate velvety eye rim so sapphire iris frames cleanly inside the blue-grey mask
        ctx.strokeStyle = 'rgba(34, 39, 54, 0.85)';
        ctx.lineWidth = 1.25;
        ctx.beginPath();
        ctx.ellipse(0, 0, rx, openRy, 0, 0, Math.PI * 2);
        ctx.stroke();
      }

      ctx.restore(); // end grumpy eyelid clip

      if (hasGrumpyLid && isRagdoll && openRy > 2.2) {
        ctx.save();
        ctx.beginPath();
        ctx.ellipse(0, 0, rx + 0.9, openRy + 0.9, 0, 0, Math.PI * 2);
        ctx.clip();
        ctx.strokeStyle = 'rgba(34, 39, 54, 0.88)';
        ctx.lineWidth = 1.9;
        ctx.lineCap = 'round';
        ctx.beginPath();
        ctx.moveTo(-rx - 2, leftDrop);
        ctx.lineTo(rx + 2, rightDrop);
        ctx.stroke();
        ctx.restore();
      }

      ctx.restore();
    }
  }

  private drawMiniMouse(ctx: CanvasRenderingContext2D): void {
    const mx = this.mouseBaseX + this.mouseOffsetX;
    const my = this.mouseBaseY + this.mouseOffsetY + this.deskHopY * 1.35;
    const rx = 18.5;
    const ry = 23.0;
    const lPress = this.mouseLeftPress;
    const rPress = this.mouseRightPress;
    const mPress = Math.max(this.mouseMiddlePress, this.mouseScrollFlash);
    const colors = this.getMouseButtonColors();

    ctx.save();
    ctx.translate(mx, my);
    ctx.rotate(-0.06);

    // Soft 3D base rim / drop shadow
    ctx.save();
    ctx.shadowColor = 'rgba(0, 0, 0, 0.28)';
    ctx.shadowBlur = 7;
    ctx.shadowOffsetY = 3.5;
    ctx.fillStyle = '#c8cac4';
    ctx.beginPath();
    ctx.ellipse(0, 1.8, rx, ry, 0, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();

    // Main mouse palm shell (rear/palm at TOP closest to cat; split L/R buttons at BOTTOM pointing away from cat)
    const palmGrad = ctx.createLinearGradient(0, -ry, 0, 2);
    palmGrad.addColorStop(0.0, '#ecece7');
    palmGrad.addColorStop(1.0, '#f4f4f0');
    ctx.fillStyle = palmGrad;
    ctx.beginPath();
    ctx.ellipse(0, 0, rx, ry, 0, 0, Math.PI * 2);
    ctx.fill();

    // Clip inside mouse silhouette to render strictly separated Left / Right front button plates!
    const isCatFacing = this.keyboardOrientation === 'cat';
    ctx.save();
    ctx.beginPath();
    ctx.ellipse(0, 0, rx - 0.5, ry - 0.5, 0, 0, Math.PI * 2);
    ctx.clip();

    // Orientation-aware button/wheel geometry:
    // - 'cat' (180°): palm at TOP, buttons at BOTTOM, L = +X (inner), R = -X (outer)
    // - 'viewer' (0°): buttons at TOP, palm at BOTTOM, L = -X (left), R = +X (right)
    ctx.save();
    if (!isCatFacing) {
      ctx.scale(-1, -1);
    }

    const traceSeam = () => {
      ctx.moveTo(-rx - 2, -0.8);
      ctx.quadraticCurveTo(0, 1.8, rx + 2, -0.8);
    };

    // 1) Left Mouse Button plate (+X half in local coords -> inner-bottom in cat mode, top-left in viewer mode)
    ctx.beginPath();
    ctx.moveTo(0, 0.5);
    ctx.quadraticCurveTo(rx * 0.52, 0.4, rx + 2, -0.8);
    ctx.lineTo(rx + 2, ry + 2);
    ctx.lineTo(0, ry + 2);
    ctx.closePath();
    ctx.fillStyle = lPress > 0.08 ? colors.left : '#fafaf7';
    ctx.fill();
    if (lPress > 0.08) {
      ctx.fillStyle = `rgba(18, 22, 28, ${(0.18 * lPress).toFixed(3)})`;
      ctx.fillRect(0, -1, rx + 2, 3.8);
      ctx.fillRect(0, -1, 2.2, ry + 2);
    }

    // 2) Right Mouse Button plate (-X half in local coords -> outer-bottom in cat mode, top-right in viewer mode)
    ctx.beginPath();
    ctx.moveTo(-rx - 2, -0.8);
    ctx.quadraticCurveTo(-rx * 0.52, 0.4, 0, 0.5);
    ctx.lineTo(0, ry + 2);
    ctx.lineTo(-rx - 2, ry + 2);
    ctx.closePath();
    ctx.fillStyle = rPress > 0.08 ? colors.right : '#fafaf7';
    ctx.fill();
    if (rPress > 0.08) {
      ctx.fillStyle = `rgba(18, 22, 28, ${(0.18 * rPress).toFixed(3)})`;
      ctx.fillRect(-rx - 2, -1, rx + 2, 3.8);
      ctx.fillRect(-2.2, -1, 2.2, ry + 2);
    }

    // Crisp horizontal palm-to-button seam & vertical center L/R split line
    ctx.strokeStyle = 'rgba(42, 45, 52, 0.56)';
    ctx.lineWidth = 1.25;
    ctx.beginPath();
    traceSeam();
    ctx.stroke();

    ctx.strokeStyle = 'rgba(42, 45, 52, 0.66)';
    ctx.lineWidth = 1.3;
    ctx.beginPath();
    ctx.moveTo(0, 0.6);
    ctx.lineTo(0, ry + 1);
    ctx.stroke();

    // Recessed scroll wheel housing + interactive wheel (middle click & scroll glow)
    ctx.fillStyle = '#2d2f36';
    this.roundRect(ctx, -2.5, 4.2, 5.0, 9.4, 2.5);
    ctx.fill();

    ctx.fillStyle = mPress > 0.12 ? colors.middle : '#6b6f78';
    this.roundRect(ctx, -1.7, 5.0 + this.mouseMiddlePress * 0.8, 3.4, 7.6, 1.7);
    ctx.fill();

    ctx.strokeStyle = 'rgba(255, 255, 255, 0.45)';
    ctx.lineWidth = 0.8;
    ctx.beginPath();
    ctx.moveTo(-1.0, 8.8 + this.mouseMiddlePress * 0.8);
    ctx.lineTo(1.0, 8.8 + this.mouseMiddlePress * 0.8);
    ctx.stroke();

    ctx.restore(); // end local orientation transform

    // Always draw upright crisp L / R button legends in unobstructed front tips!
    ctx.font = '800 7.6px "Segoe UI", system-ui, sans-serif';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    const lx = isCatFacing ? 7.4 : -7.4;
    const rxLbl = isCatFacing ? -7.4 : 7.4;
    const lyBase = isCatFacing ? 14.0 : -14.0;
    const pressDir = isCatFacing ? 0.9 : -0.9;
    ctx.fillStyle = lPress > 0.12 ? '#14161a' : 'rgba(72, 76, 86, 0.56)';
    ctx.fillText('L', lx, lyBase + lPress * pressDir);
    ctx.fillStyle = rPress > 0.12 ? '#ffffff' : 'rgba(72, 76, 86, 0.56)';
    ctx.fillText('R', rxLbl, lyBase + rPress * pressDir);

    ctx.restore(); // end mouse clip

    // Crisp outer silhouette stroke
    ctx.strokeStyle = '#2b2d34';
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    ctx.ellipse(0, 0, rx, ry, 0, 0, Math.PI * 2);
    ctx.stroke();

    ctx.restore();
  }

  private drawKeyboard(ctx: CanvasRenderingContext2D): void {
    const layout = getKeyboardLayout(this.keyboardLayout);
    const isCatFacing = this.keyboardOrientation === 'cat';
    const is108 = this.keyboardLayout === '108';
    const padX = 7;
    const padY = 6;
    const innerW = this.kbWidth - padX * 2;
    const innerH = this.kbHeight - padY * 2;

    ctx.save();
    ctx.translate(this.kbCenterX, this.kbCenterY + this.kbBounce * 1.1 + this.deskHopY);
    ctx.rotate(this.kbTiltRad);

    // True 3D desk trapezoid perspective: far edge (top) is narrower, near edge (bottom) is wider,
    // cleanly re-projected for BOTH cat-facing (Spacebar at narrow top) and viewer-facing (Esc/F-row at narrow top)!
    const topHalfW = (this.kbWidth * 0.955) / 2;
    const botHalfW = (this.kbWidth * 1.035) / 2;
    const halfH = this.kbHeight / 2;
    const rowH = innerH / layout.totalRows;
    const unitW = innerW / layout.totalUnits;
    const gap = is108 ? 0.85 : 0.9;

    const drawSingleKey = (
      targetCtx: CanvasRenderingContext2D,
      key: KeySpec,
      press: number,
      isTailSelected: boolean,
      isTailHovered: boolean,
      pickerAccentTop: string,
      pickerAccentSide: string,
    ) => {
      const rY = key.rowY ?? key.row;
      const rSpan = key.rowSpan ?? 1;
      const screenRowTop = isCatFacing ? layout.totalRows - (rY + rSpan) : rY;
      const screenUStart = isCatFacing
        ? layout.totalUnits - (key.uStart + key.uWidth)
        : key.uStart;

      const rowCenterFrac = (screenRowTop + rSpan * 0.5) / layout.totalRows;
      const depthScale = 0.94 + rowCenterFrac * 0.09;

      const rawX = -innerW / 2 + screenUStart * unitW + gap * 0.5;
      const rawW = key.uWidth * unitW - gap;
      const x = rawX * depthScale;
      const w = rawW * depthScale;
      const y = -innerH / 2 + screenRowTop * rowH + gap * 0.5 + press * 2.0;
      const h = rSpan * rowH - gap - 0.8;

      let topColor = '#f8f8f5';
      let sideColor = '#d0d2cc';
      if (press > 0.14) {
        topColor = this.customSkin
          ? this.customSkin.accentColor
          : this.catBreed === 'dafeiyu'
            ? '#5cc8ff'
            : this.catBreed === 'ragdoll'
              ? '#7ac7ff'
              : '#d2f244';
        sideColor = this.customSkin
          ? this.customSkin.accentSideColor
          : this.catBreed === 'dafeiyu'
            ? '#2563eb'
            : this.catBreed === 'ragdoll'
              ? '#4690d8'
              : '#8cb014';
      } else if (isTailSelected) {
        topColor = pickerAccentTop;
        sideColor = pickerAccentSide;
      } else if (key.accent === 'orange') {
        topColor = '#f4a524';
        sideColor = '#c27a10';
      } else if (key.accent === 'mod') {
        topColor = '#efefeb';
        sideColor = '#c7c9c3';
      }

      // When overdrawing an animated key onto the cached base, paint the inner keybed strip behind it
      if (press > 0.01) {
        targetCtx.fillStyle = '#d4d6d1';
        targetCtx.fillRect(x - 0.4, -innerH / 2 + screenRowTop * rowH + gap * 0.5 - 0.4, w + 0.8, h + 3.8);
      }

      targetCtx.fillStyle = sideColor;
      this.roundRect(targetCtx, x, y + 1.4, w, h, 1.8);
      targetCtx.fill();

      targetCtx.fillStyle = topColor;
      this.roundRect(targetCtx, x, y, w, h - 0.9, 1.8);
      targetCtx.fill();

      if (isTailHovered) {
        targetCtx.save();
        targetCtx.strokeStyle = isTailSelected ? '#111214' : pickerAccentTop;
        targetCtx.lineWidth = 1.65;
        this.roundRect(targetCtx, x - 0.3, y - 0.3, w + 0.6, h - 0.3, 2.0);
        targetCtx.stroke();
        targetCtx.restore();
      } else if (isTailSelected) {
        targetCtx.save();
        targetCtx.strokeStyle = 'rgba(17, 18, 20, 0.55)';
        targetCtx.lineWidth = 0.85;
        this.roundRect(targetCtx, x + 0.25, y + 0.25, w - 0.5, h - 1.4, 1.6);
        targetCtx.stroke();
        targetCtx.restore();
      }

      const legend = key.id === 'space' ? (isTailSelected ? '␣' : '') : key.id === 'esc' ? 'Esc' : key.label;
      if (legend) {
        const isWideWord = legend.length >= 3;
        const fontSize = is108
          ? isWideWord
            ? 5.0
            : legend.length === 2
              ? 5.8
              : 7.2
          : isWideWord
            ? 5.6
            : legend.length === 2
              ? 6.4
              : 7.8;
        targetCtx.save();
        targetCtx.translate(x + w * 0.5, y + (h - 0.9) * 0.50);
        if (isCatFacing) {
          targetCtx.rotate(Math.PI);
        }
        targetCtx.font = `700 ${fontSize}px "Segoe UI", system-ui, sans-serif`;
        targetCtx.fillStyle = press > 0.14 || isTailSelected
          ? '#111214'
          : key.accent === 'orange'
            ? '#ffffff'
            : '#2c2f36';
        targetCtx.textAlign = 'center';
        targetCtx.textBaseline = 'middle';
        targetCtx.fillText(legend, 0, 0);
        targetCtx.restore();
      }
    };

    const pickerAccentTop =
      this.catBreed === 'dafeiyu' ? '#5cc8ff' : this.catBreed === 'ragdoll' ? '#7ac7ff' : '#c8f538';
    const pickerAccentSide =
      this.catBreed === 'dafeiyu' ? '#2563eb' : this.catBreed === 'ragdoll' ? '#4690d8' : '#8cb014';

    if (!this.tailPickerActive) {
      // Cache the entire static keyboard (chassis + Gaussian shadowBlur + all 65/108 unpressed keycaps & legends)
      // on an offscreen canvas so each active frame is just 1 drawImage() + 0-3 pressed keys!
      const { uniformScale } = this.getViewportTransform();
      const cacheScale = Math.min(2.5, Math.max(1.5, Math.ceil(uniformScale * 2) / 2));
      const padMargin = 20;
      const cacheLogW = Math.ceil(botHalfW * 2 + padMargin * 2);
      const cacheLogH = Math.ceil(this.kbHeight + padMargin * 2);
      const nextKey = `${this.keyboardLayout}|${this.keyboardOrientation}|${this.kbWidth}|${this.kbHeight}|${cacheScale}`;

      if (!this.kbCacheCanvas || this.kbCacheKey !== nextKey) {
        if (!this.kbCacheCanvas) this.kbCacheCanvas = document.createElement('canvas');
        this.kbCacheCanvas.width = Math.round(cacheLogW * cacheScale);
        this.kbCacheCanvas.height = Math.round(cacheLogH * cacheScale);
        const cg = this.kbCacheCanvas.getContext('2d');
        if (cg) {
          cg.clearRect(0, 0, this.kbCacheCanvas.width, this.kbCacheCanvas.height);
          cg.save();
          cg.scale(cacheScale, cacheScale);
          cg.translate(cacheLogW * 0.5, cacheLogH * 0.5);

          cg.save();
          cg.shadowColor = 'rgba(0, 0, 0, 0.34)';
          cg.shadowBlur = 11;
          cg.shadowOffsetY = 5;
          cg.fillStyle = '#191a1e';
          this.trapezoidPath(cg, topHalfW, botHalfW, -halfH + 3.5, halfH + 5.5, 6.5);
          cg.fill();
          cg.restore();

          cg.fillStyle = '#24262c';
          cg.strokeStyle = '#363840';
          cg.lineWidth = 1.1;
          this.trapezoidPath(cg, topHalfW, botHalfW, -halfH, halfH, 6.5);
          cg.fill();
          cg.stroke();

          cg.fillStyle = '#d4d6d1';
          this.trapezoidPath(cg, topHalfW - 5, botHalfW - 5, -halfH + 4, halfH - 4, 3.5);
          cg.fill();

          for (const key of layout.keys) {
            drawSingleKey(cg, key, 0, false, false, pickerAccentTop, pickerAccentSide);
          }
          cg.restore();
          this.kbCacheKey = nextKey;
        }
      }

      if (this.kbCacheCanvas) {
        ctx.drawImage(this.kbCacheCanvas, -cacheLogW * 0.5, -cacheLogH * 0.5, cacheLogW, cacheLogH);
        for (const key of layout.keys) {
          const anim = this.keyAnims.get(key.id);
          const press = anim ? anim.pressAmount : 0;
          if (press > 0.01) {
            drawSingleKey(ctx, key, press, false, false, pickerAccentTop, pickerAccentSide);
          }
        }
      }
    } else {
      // Interactive tail-picker mode: draw live highlights & hover outlines
      ctx.save();
      ctx.shadowColor = 'rgba(0, 0, 0, 0.34)';
      ctx.shadowBlur = 11;
      ctx.shadowOffsetY = 5;
      ctx.fillStyle = '#191a1e';
      this.trapezoidPath(ctx, topHalfW, botHalfW, -halfH + 3.5, halfH + 5.5, 6.5);
      ctx.fill();
      ctx.restore();

      ctx.fillStyle = '#24262c';
      ctx.strokeStyle = pickerAccentTop;
      ctx.lineWidth = 2.0;
      this.trapezoidPath(ctx, topHalfW, botHalfW, -halfH, halfH, 6.5);
      ctx.fill();
      ctx.stroke();

      ctx.fillStyle = '#d4d6d1';
      this.trapezoidPath(ctx, topHalfW - 5, botHalfW - 5, -halfH + 4, halfH - 4, 3.5);
      ctx.fill();

      const selectedTailKeys = this.tailPickerSelection ?? this.tailKeyIdSet;
      for (const key of layout.keys) {
        const anim = this.keyAnims.get(key.id);
        const press = anim ? anim.pressAmount : 0;
        const isTailSelected = selectedTailKeys.has(key.id);
        const isTailHovered = this.tailPickerHoverId === key.id;
        drawSingleKey(ctx, key, press, isTailSelected, isTailHovered, pickerAccentTop, pickerAccentSide);
      }
    }

    if (this.privacyClose > 0.08) {
      ctx.save();
      ctx.globalAlpha = this.privacyClose;
      ctx.fillStyle = 'rgba(15, 20, 32, 0.74)';
      this.trapezoidPath(ctx, topHalfW - 2, botHalfW - 2, -halfH + 1.5, halfH - 1.5, 5.5);
      ctx.fill();
      ctx.strokeStyle = 'rgba(244, 114, 182, 0.45)';
      ctx.lineWidth = 1.4;
      ctx.stroke();
      ctx.font = '700 11.5px "Segoe UI", "PingFang SC", sans-serif';
      ctx.fillStyle = '#fbcfe8';
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      const dots = performance.now() - this.privacyTypingAt < 350 ? ' • • •' : '';
      const label = this.catBreed === 'dafeiyu' ? '🔒 非礼勿视 · 已隐藏按键' : '🔒 密码输入中 · 已隐藏按键';
      ctx.fillText(label + dots, 0, 0);
      ctx.restore();
    }

    ctx.restore();
  }

  /**
   * True-scale plush velvety cat arm & chubby rounded paw ball (1:1 match to reference photo & 8.3s / 13.2s / 31.0s!)
   */
  private drawPlumpCatArm(
    ctx: CanvasRenderingContext2D,
    side: 'left' | 'right',
    sx: number,
    sy: number,
    px: number,
    py: number,
    squash: number,
    withCrease: boolean,
    isSmear = false,
  ): void {
    const dx = px - sx;
    const dy = py - sy;
    const len = Math.max(1, Math.hypot(dx, dy));
    const ux = dx / len;
    const uy = dy / len;
    const nx = -uy;
    const ny = ux;

    const wristDist = Math.max(6, len - 8);
    const wx = sx + ux * wristDist;
    const wy = sy + uy * wristDist;

    // Cute chubby plush proportions!
    const rShoulder = 15.0;
    const rWrist = 10.6;
    const pawRx = 14.8 * (1 + squash * 0.14);
    const pawRy = 12.0 * (1 - squash * 0.12);

    if (this.customSkin) {
      this.drawCustomPlumpArm(ctx, this.customSkin, side, sx, sy, px, py, squash, withCrease, isSmear);
      return;
    }

    const isRagdoll = this.catBreed === 'ragdoll';
    ctx.save();

    // Subtle organic plush kitten-arm bow (avoids stiff straight broomstick look!)
    const bowSide = side === 'left' ? -1 : 1;
    const bowAmp = Math.min(7.5, len * 0.052);
    const midX = (sx + wx) * 0.5 + bowSide * bowAmp * Math.abs(uy);
    const midY = (sy + wy) * 0.5 + bowAmp * 0.45 * Math.abs(ux);

    if (isRagdoll) {
      // Seamless pure creamy snow-white Ragdoll mitten arm melting smoothly out of the white chest!
      const armGrad = ctx.createLinearGradient(sx, sy, px, py);
      if (isSmear) {
        armGrad.addColorStop(0.0, 'rgba(249, 246, 239, 0.0)');
        armGrad.addColorStop(0.45, 'rgba(249, 246, 239, 0.0)');
        armGrad.addColorStop(0.75, 'rgba(249, 246, 239, 0.58)');
        armGrad.addColorStop(1.0, 'rgba(252, 250, 244, 0.85)');
      } else {
        armGrad.addColorStop(0.0, 'rgba(226, 219, 208, 0.0)');
        armGrad.addColorStop(0.11, 'rgba(232, 226, 216, 1.0)');
        armGrad.addColorStop(0.32, '#f5f1e8');
        armGrad.addColorStop(0.62, '#f9f6ef');
        armGrad.addColorStop(1.0, '#fcfaf4');
      }

      ctx.fillStyle = armGrad;

      // Tapered snow-white arm silhouette (NO dark shadow smudge over the white chest!)
      const traceArmPath = () => {
        ctx.beginPath();
        ctx.moveTo(sx + nx * rShoulder, sy + ny * rShoulder);
        ctx.quadraticCurveTo(
          midX + nx * (rShoulder * 0.86),
          midY + ny * (rShoulder * 0.86),
          wx + nx * rWrist,
          wy + ny * rWrist,
        );
        ctx.lineTo(wx - nx * rWrist, wy - ny * rWrist);
        ctx.quadraticCurveTo(
          midX - nx * (rShoulder * 0.86),
          midY - ny * (rShoulder * 0.86),
          sx - nx * rShoulder,
          sy - ny * rShoulder,
        );
        ctx.closePath();
      };
      traceArmPath();
      ctx.fill();

      // Signature unique marking from user's real Ragdoll photo: adorable warm taupe-brown oval patch on viewer's RIGHT front forearm!
      if (!isSmear && side === 'right' && len > 26) {
        ctx.save();
        traceArmPath();
        ctx.clip();
        const spotX = sx + ux * (len * 0.64) + nx * 2.0;
        const spotY = sy + uy * (len * 0.64) + ny * 2.0;
        const armAngle = Math.atan2(dy, dx);
        ctx.fillStyle = 'rgba(134, 118, 102, 0.84)';
        ctx.shadowColor = 'rgba(134, 118, 102, 0.45)';
        ctx.shadowBlur = 2.5;
        ctx.beginPath();
        ctx.ellipse(spotX, spotY, Math.min(13.5, len * 0.16), 8.2, armAngle, 0, Math.PI * 2);
        ctx.fill();
        ctx.restore();
      }

      // Soft subtle contact shadow ONLY under the plump round white mitten paw ball
      ctx.save();
      if (!isSmear) {
        ctx.shadowColor = 'rgba(46, 38, 30, 0.22)';
        ctx.shadowBlur = 4;
        ctx.shadowOffsetY = 1.5;
      }
      ctx.fillStyle = '#fcfaf4';
      ctx.beginPath();
      ctx.ellipse(px, py, pawRx, pawRy, 0, 0, Math.PI * 2);
      ctx.fill();
      ctx.restore();

      if (!isSmear) {
        // Refined warm-taupe arm side strokes that fade gently in ONLY near lower torso/forearm (exact de Casteljau curve!)
        ctx.lineWidth = 1.65;
        ctx.lineCap = 'round';
        const startFrac = withCrease ? 0.22 : 0.18;
        for (const sgn of [-1, 1]) {
          const p0x = sx + nx * sgn * rShoulder;
          const p0y = sy + ny * sgn * rShoulder;
          const p1x = midX + nx * sgn * (rShoulder * 0.86);
          const p1y = midY + ny * sgn * (rShoulder * 0.86);
          const p2x = wx + nx * sgn * rWrist;
          const p2y = wy + ny * sgn * rWrist;

          const q0x = p0x + (p1x - p0x) * startFrac;
          const q0y = p0y + (p1y - p0y) * startFrac;
          const q1x = p1x + (p2x - p1x) * startFrac;
          const q1y = p1y + (p2y - p1y) * startFrac;
          const esx = q0x + (q1x - q0x) * startFrac;
          const esy = q0y + (q1y - q0y) * startFrac;

          const edgeGrad = ctx.createLinearGradient(esx, esy, p2x, p2y);
          edgeGrad.addColorStop(0.0, 'rgba(93, 81, 72, 0.0)');
          edgeGrad.addColorStop(0.28, 'rgba(93, 81, 72, 0.74)');
          edgeGrad.addColorStop(1.0, 'rgba(93, 81, 72, 0.90)');

          ctx.strokeStyle = edgeGrad;
          ctx.beginPath();
          ctx.moveTo(esx, esy);
          ctx.quadraticCurveTo(q1x, q1y, p2x, p2y);
          ctx.stroke();
        }

        // Crisp rounded mitten paw outline arc so the white paw reads clearly against white keyboard keys!
        const pawAng = Math.atan2(dy, dx);
        ctx.strokeStyle = 'rgba(93, 81, 72, 0.92)';
        ctx.beginPath();
        ctx.ellipse(px, py, pawRx, pawRy, 0, pawAng - Math.PI * 0.62, pawAng + Math.PI * 0.62);
        ctx.stroke();
      }

      ctx.restore();
      return;
    }

    ctx.shadowColor = 'rgba(18, 17, 18, 0.42)';
    ctx.shadowBlur = isSmear ? 0 : 1.6;
    if (isSmear) {
      const smearGrad = ctx.createLinearGradient(sx, sy, px, py);
      smearGrad.addColorStop(0.0, 'rgba(20, 19, 20, 0.0)');
      smearGrad.addColorStop(0.40, 'rgba(20, 19, 20, 0.0)');
      smearGrad.addColorStop(0.72, 'rgba(20, 19, 20, 0.65)');
      smearGrad.addColorStop(1.0, 'rgba(20, 19, 20, 1.0)');
      ctx.fillStyle = smearGrad;
    } else {
      ctx.fillStyle = '#141314';
    }

    // Arm silhouette tapering gently from shoulder to wrist
    ctx.beginPath();
    ctx.moveTo(sx + nx * rShoulder, sy + ny * rShoulder);
    ctx.quadraticCurveTo(
      midX + nx * (rShoulder * 0.86),
      midY + ny * (rShoulder * 0.86),
      wx + nx * rWrist,
      wy + ny * rWrist,
    );
    ctx.lineTo(wx - nx * rWrist, wy - ny * rWrist);
    ctx.quadraticCurveTo(
      midX - nx * (rShoulder * 0.86),
      midY - ny * (rShoulder * 0.86),
      sx - nx * rShoulder,
      sy - ny * rShoulder,
    );
    ctx.closePath();
    ctx.fill();

    // Seamless shoulder merge (not needed on smears)
    if (!isSmear) {
      ctx.beginPath();
      ctx.arc(sx, sy, rShoulder, 0, Math.PI * 2);
      ctx.fill();
    }

    // Plump, soft rounded black velvet paw ball pressing firmly onto the key!
    ctx.fillStyle = '#141314';
    ctx.beginPath();
    ctx.ellipse(px, py, pawRx, pawRy, 0, 0, Math.PI * 2);
    ctx.fill();

    ctx.shadowBlur = 0;

    // Clean crisp subtle light-grey side separation lines ONLY where the arm crosses the black body (exact 2.2s / 8.3s / 13.2s!)
    if (!isSmear && withCrease && len > 28) {
      const startFrac = 0.18;
      const endFrac = 0.72;
      for (const sgn of [-1, 1]) {
        const csx = sx + ux * (len * startFrac) + nx * sgn * (rShoulder * 0.86);
        const csy = sy + uy * (len * startFrac) + ny * sgn * (rShoulder * 0.86);
        const cex = sx + ux * (len * endFrac) + nx * sgn * (rWrist * 1.02);
        const cey = sy + uy * (len * endFrac) + ny * sgn * (rWrist * 1.02);

        const creaseGrad = ctx.createLinearGradient(csx, csy, cex, cey);
        creaseGrad.addColorStop(0.0, 'rgba(216, 218, 222, 0.0)');
        creaseGrad.addColorStop(0.22, 'rgba(216, 218, 222, 0.65)');
        creaseGrad.addColorStop(0.84, 'rgba(216, 218, 222, 0.56)');
        creaseGrad.addColorStop(1.0, 'rgba(216, 218, 222, 0.0)');

        ctx.strokeStyle = creaseGrad;
        ctx.lineWidth = 1.3;
        ctx.lineCap = 'round';
        ctx.beginPath();
        ctx.moveTo(csx, csy);
        ctx.lineTo(cex, cey);
        ctx.stroke();
      }
    }

    ctx.restore();
  }

  /**
   * Real 2-segment articulated bent kitten arm (shoulder -> elbow -> curled plump paw on chest!)
   * Matches original video 8.3s (left paw), 10.6s (both paws), 23.2s, 26.2s!
   */
  private drawBentCurledArm(ctx: CanvasRenderingContext2D, paw: PawState, breath: number, alpha: number): void {
    const sh = this.getLiveShoulder(paw, breath);
    const cp = this.getLiveCurledPawPos(paw, breath);
    const sign = paw.side === 'left' ? -1 : 1;
    // Natural elbow bends slightly outward & down below shoulder before forearm curves inward-up!
    const elbowX = sh.x + sign * 8;
    const elbowY = cp.y + 8;

    if (this.customSkin) {
      this.drawCustomBentArm(ctx, this.customSkin, paw, breath, alpha);
      return;
    }

    const isRagdoll = this.catBreed === 'ragdoll';
    ctx.save();
    ctx.globalAlpha = alpha;
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';

    // Continuous bent velvety arm + plump curled paw
    ctx.strokeStyle = isRagdoll ? '#f6f2ea' : '#141314';
    ctx.lineWidth = 20;
    ctx.beginPath();
    ctx.moveTo(sh.x, sh.y);
    ctx.quadraticCurveTo(elbowX, elbowY, cp.x, cp.y);
    ctx.stroke();

    // Signature warm taupe-brown patch on viewer's RIGHT curled forearm too!
    if (isRagdoll && paw.side === 'right') {
      ctx.save();
      ctx.fillStyle = 'rgba(134, 118, 102, 0.78)';
      ctx.beginPath();
      ctx.ellipse((elbowX + cp.x) * 0.5 + 2, (elbowY + cp.y) * 0.5 + 1, 7.5, 5.2, -0.35, 0, Math.PI * 2);
      ctx.fill();
      ctx.restore();
    }

    ctx.fillStyle = isRagdoll ? '#fcfaf4' : '#141314';
    ctx.beginPath();
    ctx.ellipse(cp.x, cp.y, 12.5, 11.0, sign * 0.2, 0, Math.PI * 2);
    ctx.fill();

    // Clear crisp contour along the bent curled forearm & kitten paw
    const grad = ctx.createLinearGradient(elbowX, elbowY - 12, cp.x, cp.y + 12);
    if (isRagdoll) {
      grad.addColorStop(0.0, 'rgba(93, 81, 72, 0.0)');
      grad.addColorStop(0.28, 'rgba(93, 81, 72, 0.68)');
      grad.addColorStop(0.86, 'rgba(93, 81, 72, 0.78)');
      grad.addColorStop(1.0, 'rgba(93, 81, 72, 0.35)');
    } else {
      grad.addColorStop(0.0, 'rgba(214, 216, 220, 0.0)');
      grad.addColorStop(0.30, 'rgba(214, 216, 220, 0.65)');
      grad.addColorStop(0.85, 'rgba(220, 222, 226, 0.72)');
      grad.addColorStop(1.0, 'rgba(220, 222, 226, 0.25)');
    }

    ctx.strokeStyle = grad;
    ctx.lineWidth = isRagdoll ? 1.65 : 1.4;
    ctx.beginPath();
    ctx.moveTo(elbowX + sign * 8, elbowY - 16);
    ctx.quadraticCurveTo(elbowX + sign * 4, elbowY + 12, cp.x + sign * 2, cp.y + 10);
    ctx.quadraticCurveTo(cp.x - sign * 11, cp.y + 8, cp.x - sign * 12, cp.y - 4);
    ctx.stroke();

    ctx.restore();
  }

  private drawPaw(ctx: CanvasRenderingContext2D, paw: PawState, breath: number): void {
    // 1) Seated idle vertical front legs (EXACT 4 clean lines of ORIG 0.3s!) OR planted diagonal supporting leg (ORIG 2.2s!)
    if (paw.idleWeight > 0.05) {
      ctx.save();
      ctx.globalAlpha = paw.idleWeight;

      if (paw.side === 'right' && this.leftPaw.mode === 'mouse') {
        const sh = this.getLiveShoulder(paw, breath);
        const baseSp = this.spinePoint(1.0, breath);
        this.drawPlumpCatArm(ctx, 'right', sh.x, sh.y + 4, baseSp.x + 30, baseSp.y - 2, 0, true);
      } else {
        // Exact 4-line seated leg silhouette from ORIG 0.3s (outer curved leg line + crisp inner center-gap line!)
        const topSp = this.spinePoint(0.66, breath);
        const midSp = this.spinePoint(0.84, breath);
        const botSp = this.spinePoint(1.0, breath);
        const sign = paw.side === 'left' ? -1 : 1;

        const outerTopX = topSp.x + sign * 41;
        const outerBotX = botSp.x + sign * 35;
        const innerTopX = topSp.x + sign * 6.5;
        const innerBotX = botSp.x + sign * 9.5;

        if (this.customSkin) {
          this.drawCustomIdleLegDetails(ctx, this.customSkin, paw, topSp, midSp, botSp, sign, outerTopX, outerBotX, innerTopX, innerBotX);
          ctx.restore();
          return;
        }

        // Signature warm taupe-brown oval leg patch on viewer's RIGHT seated front leg (1:1 match to user's real Ragdoll!)
        const isRagdoll = this.catBreed === 'ragdoll';
        if (isRagdoll && paw.side === 'right') {
          ctx.save();
          ctx.fillStyle = 'rgba(134, 118, 102, 0.80)';
          ctx.shadowColor = 'rgba(134, 118, 102, 0.40)';
          ctx.shadowBlur = 3.0;
          ctx.beginPath();
          ctx.ellipse(midSp.x + 21, midSp.y + 12, 9.5, 13.5, 0.22, 0, Math.PI * 2);
          ctx.fill();
          ctx.restore();
        }

        // Crisp subtle outer leg curve & inner leg-gap line (4 total lines across both legs — exact ORIG 0.3s!)
        const lineGrad = ctx.createLinearGradient(topSp.x, topSp.y, botSp.x, botSp.y);
        if (isRagdoll) {
          lineGrad.addColorStop(0.0, 'rgba(93, 81, 72, 0.0)');
          lineGrad.addColorStop(0.18, 'rgba(93, 81, 72, 0.62)');
          lineGrad.addColorStop(0.92, 'rgba(93, 81, 72, 0.68)');
          lineGrad.addColorStop(1.0, 'rgba(93, 81, 72, 0.28)');
        } else {
          lineGrad.addColorStop(0.0, 'rgba(210, 212, 216, 0.0)');
          lineGrad.addColorStop(0.18, 'rgba(210, 212, 216, 0.64)');
          lineGrad.addColorStop(0.92, 'rgba(210, 212, 216, 0.62)');
          lineGrad.addColorStop(1.0, 'rgba(210, 212, 216, 0.25)');
        }

        ctx.strokeStyle = lineGrad;
        ctx.lineWidth = isRagdoll ? 1.55 : 1.4;
        ctx.lineCap = 'round';

        // Outer front-leg separation line (gently curves inward toward paw!)
        ctx.beginPath();
        ctx.moveTo(outerTopX, topSp.y);
        ctx.quadraticCurveTo(midSp.x + sign * 41, midSp.y, outerBotX, botSp.y - 1);
        ctx.stroke();

        // Inner front-leg gap line (tapers slightly outward toward paws, exact inverted narrow V from ORIG 0.3s!)
        ctx.beginPath();
        ctx.moveTo(innerTopX, topSp.y + 10);
        ctx.lineTo(innerBotX, botSp.y - 1);
        ctx.stroke();
      }
      ctx.restore();
    }

    // 2) Continuous bent articulated curled chest arm (8.3s / 10.6s / 23.2s / 26.2s!)
    if (paw.curlWeight > 0.05) {
      this.drawBentCurledArm(ctx, paw, breath, paw.curlWeight);
    }

    // 3) Plump velvety poking / mouse-holding cat arm!
    const activeArmWeight = Math.max(0, 1 - paw.idleWeight - paw.curlWeight);
    if (activeArmWeight > 0.05) {
      const sh = this.getLiveShoulder(paw, breath);
      const px = paw.pos.x;
      const mouseClickDrop =
        paw.mode === 'mouse'
          ? Math.max(this.mouseLeftPress, this.mouseRightPress, this.mouseMiddlePress) * 1.8
          : 0;
      const py = paw.pos.y + mouseClickDrop;

      ctx.save();
      ctx.globalAlpha = Math.min(1, activeArmWeight * 1.2);

      for (const sm of paw.smears) {
        ctx.save();
        const smearMult = this.customSkin || this.catBreed === 'ragdoll' ? 0.58 : 1.0;
        ctx.globalAlpha = Math.max(0, sm.alpha * activeArmWeight * smearMult);
        this.drawPlumpCatArm(ctx, paw.side, sm.sx, sm.sy, sm.px, sm.py, 0, false, true);
        ctx.restore();
      }

      // Keep mouse-reaching arm pure velvet black in open air (ORIG 2.2s); show subtle separation lines when poking across black torso!
      this.drawPlumpCatArm(ctx, paw.side, sh.x, sh.y, px, py, paw.impactSquash, paw.mode !== 'mouse', false);
      ctx.restore();
    }
  }

  private drawKeyBubbles(ctx: CanvasRenderingContext2D): void {
    const now = performance.now();

    for (const b of this.bubbles) {
      const age = now - b.createdAt;
      const t = Math.min(1, age / b.lifetime);

      let scale = 1;
      if (t < 0.11) {
        const p = t / 0.11;
        scale = 0.62 + Math.sin(p * Math.PI * 0.65) * 0.44;
      } else if (t > 0.76) {
        scale = 1 - ((t - 0.76) / 0.24) * 0.1;
      }
      const alpha = t < 0.72 ? 1 : Math.max(0, 1 - (t - 0.72) / 0.28);

      ctx.save();
      ctx.globalAlpha = alpha;
      ctx.translate(b.x, b.y);
      ctx.rotate(b.rot);
      ctx.scale(scale, scale);

      ctx.font = '700 12px "Segoe UI", system-ui, sans-serif';
      const textW = ctx.measureText(b.label).width;
      const boxW = b.isSpace ? 40 : Math.max(24, textW + 13);
      const boxH = 24;
      const bx = -boxW / 2;
      const by = -boxH / 2;

      ctx.shadowColor = 'rgba(0, 0, 0, 0.22)';
      ctx.shadowBlur = 5;
      ctx.shadowOffsetY = 2;

      ctx.fillStyle = '#c8cac6';
      this.roundRect(ctx, bx, by + 2.2, boxW, boxH, 5.5);
      ctx.fill();

      ctx.shadowBlur = 0;
      ctx.fillStyle = '#ffffff';
      ctx.strokeStyle = 'rgba(38, 40, 46, 0.42)';
      ctx.lineWidth = 1.05;
      this.roundRect(ctx, bx, by, boxW, boxH - 1.3, 5.5);
      ctx.fill();
      ctx.stroke();

      if (b.isSpace) {
        ctx.strokeStyle = '#222429';
        ctx.lineWidth = 1.55;
        ctx.lineCap = 'round';
        ctx.lineJoin = 'round';
        ctx.beginPath();
        ctx.moveTo(-9, -1);
        ctx.lineTo(-9, 2.0);
        ctx.lineTo(9, 2.0);
        ctx.lineTo(9, -1);
        ctx.stroke();
      } else {
        ctx.fillStyle = '#1e2025';
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.fillText(b.label, 0, -0.5);
      }

      ctx.restore();
    }
  }

  private drawMouseBadges(ctx: CanvasRenderingContext2D): void {
    const now = performance.now();
    for (let i = 0; i < this.mouseBadges.length; i++) {
      const b = this.mouseBadges[i];
      const t = Math.min(1, (now - b.createdAt) / b.lifetime);
      let scale = 1;
      if (t < 0.14) {
        scale = 0.65 + Math.sin((t / 0.14) * Math.PI * 0.65) * 0.42;
      } else if (t > 0.72) {
        scale = 1 - ((t - 0.72) / 0.28) * 0.12;
      }
      const alpha = t < 0.68 ? 1 : Math.max(0, 1 - (t - 0.68) / 0.32);
      const liftY = t * 14 + (this.mouseBadges.length - 1 - i) * 18;

      ctx.save();
      ctx.globalAlpha = alpha;
      ctx.translate(b.x, b.y - liftY);
      ctx.scale(scale, scale);

      const boxW = 34;
      const boxH = 19;
      const bx = -boxW / 2;
      const by = -boxH / 2;

      ctx.shadowColor = 'rgba(0, 0, 0, 0.24)';
      ctx.shadowBlur = 4;
      ctx.shadowOffsetY = 1.5;

      ctx.fillStyle = '#22242a';
      this.roundRect(ctx, bx, by + 1.6, boxW, boxH, 5);
      ctx.fill();

      ctx.shadowBlur = 0;
      ctx.fillStyle = b.color;
      ctx.strokeStyle = 'rgba(20, 22, 28, 0.45)';
      ctx.lineWidth = 1.0;
      this.roundRect(ctx, bx, by, boxW, boxH - 1.0, 5);
      ctx.fill();
      ctx.stroke();

      ctx.font = '800 9.5px "Segoe UI", system-ui, sans-serif';
      ctx.fillStyle = b.label === 'RMB' && b.color === '#ff7b3b' ? '#ffffff' : '#14161a';
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      ctx.fillText(b.label, 0, -0.3);

      ctx.restore();
    }
  }

  private drawCustomCatBody(
    ctx: CanvasRenderingContext2D,
    s: ResolvedSkin,
    breath: number,
    purrShift: number,
  ): void {
    const leftPts: Vec2[] = [];
    const rightPts: Vec2[] = [];
    const bellySwell = 1 + this.typingHunch * 0.04 + this.sleepAmount * 0.085;
    const headCenter = this.spinePoint(0.24, breath);
    const hcx = headCenter.x + purrShift;
    const hcy = headCenter.y;

    for (const slice of BODY_SLICES) {
      const sp = this.spinePoint(slice.t, breath);
      const widen = slice.t > 0.45 ? bellySwell : 1;
      const hw = slice.hw * widen;

      if (slice.t <= 0.38) {
        const tiltW = slice.t <= 0.30 ? 1.0 : (0.42 - slice.t) / 0.12;
        const ang = this.headTilt * tiltW;
        const cosA = Math.cos(ang);
        const sinA = Math.sin(ang);
        const relY = sp.y - hcy;
        const lx0 = sp.x + purrShift - hw - hcx;
        const rx0 = sp.x + purrShift + hw - hcx;
        leftPts.push({ x: hcx + lx0 * cosA - relY * sinA, y: hcy + lx0 * sinA + relY * cosA });
        rightPts.push({ x: hcx + rx0 * cosA - relY * sinA, y: hcy + rx0 * sinA + relY * cosA });
      } else {
        leftPts.push({ x: sp.x + purrShift - hw, y: sp.y });
        rightPts.push({ x: sp.x + purrShift + hw, y: sp.y });
      }
    }

    const crownRelY = this.spinePoint(0.11, breath).y - hcy;
    const crownPt = {
      x: hcx - crownRelY * Math.sin(this.headTilt),
      y: hcy + crownRelY * Math.cos(this.headTilt),
    };
    const basePt = this.spinePoint(1.0, breath);

    ctx.save();
    ctx.shadowColor = s.isDarkBase ? 'rgba(18, 17, 18, 0.42)' : 'rgba(30, 32, 42, 0.22)';
    ctx.shadowBlur = s.isDarkBase ? 1.6 : 5;
    if (!s.isDarkBase) ctx.shadowOffsetY = 2;

    // 1) Grounded hind paws always rendered in seated posture (no idle disappear/blink; Fixes #1)
    ctx.fillStyle = s.pawColor;
    ctx.strokeStyle = s.pawOutlineColor || s.outlineColor || 'transparent';
    ctx.lineWidth = 1.85;
    for (const [fx, fy, rx, ry] of [
      [basePt.x - 42, basePt.y - 3, 14.5, 11.5],
      [basePt.x + 32, basePt.y - 1, 15.0, 11.5],
    ]) {
      ctx.beginPath();
      ctx.ellipse(fx, fy, rx, ry, 0, 0, Math.PI * 2);
      ctx.fill();
      if (s.pawOutlineColor || s.outlineColor) {
        ctx.save();
        ctx.shadowBlur = 0;
        ctx.shadowOffsetY = 0;
        ctx.stroke();
        ctx.restore();
      }
    }

    // 2) Unified flexible body path
    const traceBodyPath = () => {
      ctx.beginPath();
      const crownCos = Math.cos(this.headTilt);
      const crownSin = Math.sin(this.headTilt);
      ctx.moveTo(leftPts[0].x + 14 * crownCos + 6 * crownSin, leftPts[0].y + 14 * crownSin - 6 * crownCos);
      ctx.quadraticCurveTo(crownPt.x, crownPt.y - 2, rightPts[0].x - 14 * crownCos + 6 * crownSin, rightPts[0].y - 14 * crownSin - 6 * crownCos);
      ctx.lineTo(rightPts[0].x, rightPts[0].y);
      for (let i = 0; i < rightPts.length - 1; i++) {
        const p0 = rightPts[i];
        const p1 = rightPts[i + 1];
        ctx.quadraticCurveTo(p0.x, p0.y, (p0.x + p1.x) * 0.5, (p0.y + p1.y) * 0.5);
      }
      ctx.lineTo(rightPts[rightPts.length - 1].x, rightPts[rightPts.length - 1].y);
      ctx.quadraticCurveTo(basePt.x + purrShift, basePt.y + 6, leftPts[leftPts.length - 1].x, leftPts[leftPts.length - 1].y);
      for (let i = leftPts.length - 1; i > 0; i--) {
        const p0 = leftPts[i];
        const p1 = leftPts[i - 1];
        ctx.quadraticCurveTo(p0.x, p0.y, (p0.x + p1.x) * 0.5, (p0.y + p1.y) * 0.5);
      }
      ctx.lineTo(leftPts[0].x, leftPts[0].y);
      ctx.closePath();
    };

    traceBodyPath();
    const furGrad = ctx.createRadialGradient(hcx, hcy + 65, 18, basePt.x, basePt.y - 60, 150);
    furGrad.addColorStop(0.0, s.furColor);
    furGrad.addColorStop(1.0, s.furSecondaryColor);
    ctx.fillStyle = furGrad;
    ctx.fill();

    // 3) Clipped coat pattern markings & custom spots
    ctx.save();
    ctx.shadowBlur = 0;
    ctx.shadowOffsetY = 0;
    ctx.clip();

    const midTorso = this.spinePoint(0.70, breath);
    const chestSp = this.spinePoint(0.56, breath);

    if (s.flankColor) {
      for (const sgn of [-1, 1]) {
        ctx.save();
        ctx.globalAlpha = 0.62;
        ctx.fillStyle = s.flankColor;
        ctx.shadowColor = s.flankColor;
        ctx.shadowBlur = 10;
        ctx.beginPath();
        ctx.ellipse(midTorso.x + purrShift + sgn * 62, midTorso.y + 6, 48, 82, sgn * -0.14, 0, Math.PI * 2);
        ctx.fill();
        ctx.restore();
      }
    }

    if (s.bellyBibColor) {
      ctx.fillStyle = s.bellyBibColor;
      ctx.beginPath();
      ctx.ellipse(chestSp.x + purrShift, chestSp.y + 38, 36, 62, 0, 0, Math.PI * 2);
      ctx.fill();
    }

    if (s.pattern === 'tabby' && s.stripeColor) {
      ctx.strokeStyle = s.stripeColor;
      ctx.lineCap = 'round';
      ctx.lineWidth = 4.6;
      ctx.globalAlpha = 0.65;
      for (const sgn of [-1, 1]) {
        for (let i = 0; i < 4; i++) {
          const ty = midTorso.y - 26 + i * 20;
          ctx.beginPath();
          ctx.moveTo(midTorso.x + purrShift + sgn * 74, ty);
          ctx.quadraticCurveTo(midTorso.x + purrShift + sgn * 56, ty + 3, midTorso.x + purrShift + sgn * 42, ty + 7);
          ctx.stroke();
        }
      }
      ctx.globalAlpha = 1;
    }

    if (s.pattern === 'calico') {
      const orangePatch = s.maskColor || '#ea9446';
      const darkPatch = s.calicoPatchColor || '#3c3634';
      ctx.save();
      ctx.shadowBlur = 4;
      ctx.shadowColor = orangePatch;
      ctx.fillStyle = orangePatch;
      ctx.beginPath();
      ctx.ellipse(midTorso.x + purrShift - 54, midTorso.y + 10, 38, 52, -0.2, 0, Math.PI * 2);
      ctx.fill();

      ctx.shadowColor = darkPatch;
      ctx.fillStyle = darkPatch;
      ctx.beginPath();
      ctx.ellipse(midTorso.x + purrShift + 56, midTorso.y - 4, 36, 56, 0.18, 0, Math.PI * 2);
      ctx.fill();
      ctx.restore();
    }

    // Body custom spots
    for (const spot of s.spots) {
      if (spot.part !== 'body') continue;
      ctx.save();
      ctx.globalAlpha = spot.opacity ?? 0.85;
      ctx.fillStyle = spot.color;
      ctx.beginPath();
      ctx.ellipse(
        midTorso.x + purrShift + spot.x * 65,
        midTorso.y + spot.y * 65,
        Math.max(2, spot.rx),
        Math.max(2, spot.ry),
        spot.rotation || 0,
        0,
        Math.PI * 2,
      );
      ctx.fill();
      ctx.restore();
    }

    // Head-space patterns (rotate seamlessly with headTilt!)
    ctx.save();
    ctx.translate(hcx, hcy);
    ctx.rotate(this.headTilt);
    const maskShift = this.faceTurnX * 0.72;

    if (s.pattern === 'bicolor' && s.maskColor) {
      ctx.save();
      ctx.fillStyle = s.maskColor;
      ctx.shadowColor = s.maskColor;
      ctx.shadowBlur = 4.5;
      ctx.beginPath();
      ctx.moveTo(-74, -44);
      ctx.lineTo(74, -44);
      ctx.lineTo(70, 20);
      ctx.quadraticCurveTo(maskShift + 42, 18, maskShift + 26, 9);
      ctx.quadraticCurveTo(maskShift + 17, 0, maskShift + 13.5, -13);
      ctx.quadraticCurveTo(maskShift + 7, -21.5, maskShift, -22.5);
      ctx.quadraticCurveTo(maskShift - 7, -21.5, maskShift - 13.5, -13);
      ctx.quadraticCurveTo(maskShift - 17, 0, maskShift - 26, 9);
      ctx.quadraticCurveTo(maskShift - 42, 18, -70, 20);
      ctx.closePath();
      ctx.fill();
      if (s.foreheadBridgeColor) {
        ctx.fillStyle = s.foreheadBridgeColor;
        ctx.shadowBlur = 5.5;
        ctx.beginPath();
        ctx.ellipse(maskShift, -32, 18, 9, 0, 0, Math.PI * 2);
        ctx.fill();
      }
      ctx.restore();
    } else if (s.pattern === 'tuxedo' && s.bellyBibColor) {
      ctx.fillStyle = s.bellyBibColor;
      ctx.beginPath();
      ctx.ellipse(maskShift, 22, 26, 15, 0, 0, Math.PI * 2);
      ctx.fill();
    } else if (s.pattern === 'siamese' && s.maskColor) {
      ctx.save();
      ctx.fillStyle = s.maskColor;
      ctx.shadowColor = s.maskColor;
      ctx.shadowBlur = 9;
      ctx.beginPath();
      ctx.ellipse(maskShift, 3, 42, 27, 0, 0, Math.PI * 2);
      ctx.fill();
      ctx.restore();
    } else if (s.pattern === 'calico') {
      const orangePatch = s.maskColor || '#ea9446';
      const darkPatch = s.calicoPatchColor || '#3c3634';
      ctx.save();
      ctx.shadowBlur = 4;
      ctx.fillStyle = orangePatch;
      ctx.shadowColor = orangePatch;
      ctx.beginPath();
      ctx.ellipse(-34 + maskShift * 0.4, -18, 34, 28, -0.25, 0, Math.PI * 2);
      ctx.fill();
      ctx.fillStyle = darkPatch;
      ctx.shadowColor = darkPatch;
      ctx.beginPath();
      ctx.ellipse(36 + maskShift * 0.4, -16, 32, 26, 0.25, 0, Math.PI * 2);
      ctx.fill();
      ctx.restore();
    } else if (s.pattern === 'tabby' && s.stripeColor) {
      // Iconic tabby forehead "M" & cheek stripes!
      ctx.strokeStyle = s.stripeColor;
      ctx.lineWidth = 3.2;
      ctx.lineCap = 'round';
      ctx.lineJoin = 'round';
      ctx.globalAlpha = 0.78;
      // Center M stripes on forehead
      ctx.beginPath();
      ctx.moveTo(maskShift - 11, -30);
      ctx.lineTo(maskShift - 8, -19);
      ctx.moveTo(maskShift, -32);
      ctx.lineTo(maskShift, -18);
      ctx.moveTo(maskShift + 11, -30);
      ctx.lineTo(maskShift + 8, -19);
      ctx.stroke();
      // Cheek tiger stripes
      ctx.lineWidth = 2.8;
      for (const sgn of [-1, 1]) {
        ctx.beginPath();
        ctx.moveTo(sgn * 58, 6);
        ctx.lineTo(sgn * 42, 9);
        ctx.moveTo(sgn * 56, 14);
        ctx.lineTo(sgn * 41, 16);
        ctx.stroke();
      }
      ctx.globalAlpha = 1;
    }

    // Head custom spots
    for (const spot of s.spots) {
      if (spot.part !== 'head') continue;
      ctx.save();
      ctx.globalAlpha = spot.opacity ?? 0.85;
      ctx.fillStyle = spot.color;
      ctx.beginPath();
      ctx.ellipse(
        maskShift + spot.x * 52,
        spot.y * 36,
        Math.max(2, spot.rx),
        Math.max(2, spot.ry),
        spot.rotation || 0,
        0,
        Math.PI * 2,
      );
      ctx.fill();
      ctx.restore();
    }

    ctx.restore(); // end head-space patterns
    ctx.restore(); // end body clip

    if (s.outlineColor) {
      ctx.save();
      ctx.shadowBlur = 0;
      ctx.shadowOffsetY = 0;
      traceBodyPath();
      ctx.strokeStyle = s.outlineColor;
      ctx.lineWidth = 1.8;
      ctx.lineJoin = 'round';
      ctx.stroke();
      ctx.restore();
    }

    // 4) Ears, face features & eyes in head-space
    ctx.save();
    ctx.translate(hcx, hcy);
    ctx.rotate(this.headTilt);

    const nowEar = performance.now();
    const earAct = this.idleAct;
    const earActU = earAct && earAct.kind === 'ear' ? Math.max(0, Math.min(1, (nowEar - earAct.t0) / earAct.dur)) : 0;
    const lFlick = earAct && earAct.kind === 'ear' && earAct.side === -1 ? Math.sin(earActU * Math.PI * 3) * 0.24 * (1 - earActU) : 0;
    const rFlick = earAct && earAct.kind === 'ear' && earAct.side === 1 ? Math.sin(earActU * Math.PI * 3) * 0.24 * (1 - earActU) : 0;
    const lTwitch = Math.sin(this.earTwitchLeft * Math.PI) * 0.12 + lFlick;
    const rTwitch = Math.sin(this.earTwitchRight * Math.PI) * 0.12 + rFlick;
    const earPerk = this.earPerkSpring.x;
    const airplaneDrop = Math.max(0, -earPerk) * 11;
    const airplaneSpread = Math.max(0, -earPerk) * 6;

    const drawCustomEar = (sideSign: -1 | 1, twitch: number) => {
      ctx.save();
      ctx.translate(sideSign * (33 + airplaneSpread) + this.faceTurnX * 0.18, -28 + airplaneDrop);
      ctx.rotate(sideSign * (0.08 - earPerk + twitch));

      const earPath = () => {
        ctx.beginPath();
        ctx.moveTo(sideSign * 19, 20);
        ctx.bezierCurveTo(sideSign * 19, -8, sideSign * 12, -23, sideSign * 3, -21);
        ctx.bezierCurveTo(sideSign * -8, -18, sideSign * -16, -6, sideSign * -21, 8);
        ctx.closePath();
      };

      const baseEarCol =
        s.pattern === 'calico' && sideSign === -1
          ? s.maskColor || s.earOuterColor
          : s.earOuterColor;
      const earGrad = ctx.createLinearGradient(0, -23, 0, 18);
      earGrad.addColorStop(0.0, s.earTipColor || baseEarCol);
      earGrad.addColorStop(1.0, baseEarCol);
      ctx.fillStyle = earGrad;
      earPath();
      ctx.fill();

      ctx.shadowBlur = 0;
      ctx.shadowOffsetY = 0;

      if (s.outlineColor) {
        ctx.strokeStyle = s.outlineColor;
        ctx.lineWidth = 1.85;
        ctx.lineCap = 'round';
        ctx.beginPath();
        ctx.moveTo(sideSign * 19, 17);
        ctx.bezierCurveTo(sideSign * 19, -8, sideSign * 12, -23, sideSign * 3, -21);
        ctx.bezierCurveTo(sideSign * -8, -18, sideSign * -16, -6, sideSign * -19, 5);
        ctx.stroke();
      }

      if (s.earInnerColor) {
        ctx.fillStyle = s.earInnerColor;
        ctx.beginPath();
        ctx.moveTo(sideSign * 11, 10);
        ctx.bezierCurveTo(sideSign * 11, -3, sideSign * 7, -13, sideSign * 2, -11);
        ctx.bezierCurveTo(sideSign * -4, -9, sideSign * -9, -1, sideSign * -10, 8);
        ctx.closePath();
        ctx.fill();
      }

      if (s.earTuftColor) {
        ctx.strokeStyle = s.earTuftColor;
        ctx.lineWidth = 1.5;
        ctx.lineCap = 'round';
        ctx.beginPath();
        ctx.moveTo(sideSign * -6, 6);
        ctx.quadraticCurveTo(sideSign * -1, 3, sideSign * 5, 6);
        ctx.moveTo(sideSign * -3, 10);
        ctx.quadraticCurveTo(sideSign * 2, 7.5, sideSign * 7, 10);
        ctx.stroke();
      }
      ctx.restore();
    };

    this.drawHeadphonesBack(ctx);
    drawCustomEar(-1, lTwitch);
    drawCustomEar(1, rTwitch);

    ctx.shadowBlur = 0;
    ctx.shadowOffsetY = 0;
    const wShift = this.faceTurnX * 0.72;

    // Blush on petting
    if (s.blushColor && this.squintAmount > 0.12) {
      ctx.save();
      ctx.globalAlpha = 0.42 * this.squintAmount;
      ctx.fillStyle = s.blushColor;
      ctx.beginPath();
      ctx.ellipse(wShift - 30, 13, 8.5, 4.6, -0.08, 0, Math.PI * 2);
      ctx.ellipse(wShift + 30, 13, 8.5, 4.6, 0.08, 0, Math.PI * 2);
      ctx.fill();
      ctx.restore();
    }

    // Nose & mouth
    if (s.noseColor) {
      ctx.fillStyle = s.noseColor;
      ctx.beginPath();
      ctx.moveTo(wShift - 4.1, 9.6);
      ctx.quadraticCurveTo(wShift, 8.5, wShift + 4.1, 9.6);
      ctx.quadraticCurveTo(wShift + 3.2, 13.8, wShift, 14.3);
      ctx.quadraticCurveTo(wShift - 3.2, 13.8, wShift - 4.1, 9.6);
      ctx.closePath();
      ctx.fill();
    }
    if (s.mouthColor) {
      ctx.strokeStyle = s.mouthColor;
      ctx.lineWidth = 1.35;
      ctx.lineCap = 'round';
      ctx.beginPath();
      ctx.moveTo(wShift - 5.8, 16.6);
      ctx.quadraticCurveTo(wShift - 2.9, 19.6, wShift, 16.4);
      ctx.quadraticCurveTo(wShift + 2.9, 19.6, wShift + 5.8, 16.6);
      ctx.stroke();
    }

    // Whiskers
    ctx.strokeStyle = s.whiskerColor;
    ctx.lineWidth = 1.1;
    ctx.lineCap = 'round';
    const leftLen = Math.max(8, 12 - this.faceTurnX * 0.55);
    const rightLen = Math.max(8, 12 + this.faceTurnX * 0.55);
    for (let i = 0; i < 3; i++) {
      const wy = 16 + i * 5.0;
      const slant = 2.0 + i * 1.1;
      ctx.beginPath();
      ctx.moveTo(-34 + wShift, wy);
      ctx.lineTo(-34 + wShift - leftLen, wy + slant);
      ctx.stroke();
      ctx.beginPath();
      ctx.moveTo(34 + wShift, wy);
      ctx.lineTo(34 + wShift + rightLen, wy + slant);
      ctx.stroke();
    }

    this.drawCustomEyes(ctx, s, wShift, -1);
    this.drawHeadphonesFront(ctx);

    ctx.restore();
    ctx.restore();
  }

  private drawCustomEyes(ctx: CanvasRenderingContext2D, s: ResolvedSkin, cx: number, eyeY: number): void {
    const nowEye = performance.now();
    const slowBlinkVal =
      this.idleAct && this.idleAct.kind === 'slowBlink'
        ? 0.92 * Math.sin(Math.max(0, Math.min(1, (nowEye - this.idleAct.t0) / this.idleAct.dur)) * Math.PI)
        : 0;
    const blinkVal = Math.max(this.isBlinking ? 1 - Math.abs(this.blinkProgress - 1) : 0, slowBlinkVal);
    const closedness = Math.max(blinkVal, this.squintAmount * 0.85, this.privacyClose, this.sleepAmount);
    const curious = Math.min(1, this.curiousAmount);
    const cross = this.crossAmount;
    const eyeXs = [cx - 23.0, cx + 23.0];
    const rx = (14.8 + this.typingHunch * 1.0) * (1 + curious * 0.06);
    const ry = (13.4 - this.typingHunch * 0.5) * (1 + curious * 0.09);

    for (let idx = 0; idx < 2; idx++) {
      ctx.save();
      ctx.translate(eyeXs[idx], eyeY);

      if (this.privacyClose > 0.45 || this.sleepAmount > 0.52) {
        ctx.strokeStyle = s.squintColor;
        ctx.lineWidth = 2.9;
        ctx.lineCap = 'round';
        ctx.beginPath();
        ctx.moveTo(-13.5, -0.5);
        ctx.quadraticCurveTo(0, 5.2, 13.5, -0.5);
        ctx.stroke();
        ctx.restore();
        continue;
      }

      if (this.squintAmount > 0.38 && blinkVal < 0.3) {
        if (s.squintGlowColor) {
          ctx.strokeStyle = s.squintGlowColor;
          ctx.lineWidth = 4.4;
          ctx.lineCap = 'round';
          ctx.beginPath();
          ctx.moveTo(-12.5, 0.2);
          ctx.quadraticCurveTo(0, 2.2, 12.5, 1.8);
          ctx.stroke();
        }
        ctx.strokeStyle = s.squintColor;
        ctx.lineWidth = 2.8;
        ctx.lineCap = 'round';
        ctx.beginPath();
        ctx.moveTo(-13.5, -0.5);
        ctx.quadraticCurveTo(0, 1.4, 13.5, 1.2);
        ctx.stroke();
        ctx.restore();
        continue;
      }

      const openRy = Math.max(1.2, ry * (1 - closedness));
      const curiousTilt = (idx === 0 ? -0.1 : 0.1) * (0.25 + this.typingHunch * 0.75);
      ctx.rotate(curiousTilt);

      const hasGrumpyLid = cross > 0.04;
      const innerDrop = -openRy + openRy * 0.90 * cross;
      const outerDrop = -openRy + openRy * 0.56 * cross;
      const leftDrop = idx === 0 ? outerDrop : innerDrop;
      const rightDrop = idx === 0 ? innerDrop : outerDrop;

      // Clip the eye cleanly below the slanted grumpy eyelid so bicolor masks, tabby stripes & spots stay 100% seamless!
      ctx.save();
      if (hasGrumpyLid) {
        ctx.beginPath();
        ctx.moveTo(-rx - 4, leftDrop);
        ctx.lineTo(rx + 4, rightDrop);
        ctx.lineTo(rx + 4, openRy + 4);
        ctx.lineTo(-rx - 4, openRy + 4);
        ctx.closePath();
        ctx.clip();
      }

      const irisGrad = ctx.createLinearGradient(0, -openRy, 0, openRy);
      irisGrad.addColorStop(0.0, s.eyeColorTop);
      irisGrad.addColorStop(1.0, s.eyeColorBottom);
      ctx.fillStyle = irisGrad;
      ctx.beginPath();
      ctx.ellipse(0, 0, rx, openRy, 0, 0, Math.PI * 2);
      ctx.fill();

      ctx.save();
      ctx.clip();
      if (openRy > 3.2) {
        const pupilRx = Math.min(rx - 2.4, (6.2 + curious * 4.2 + this.typingEnergy * 0.8) * (1 - cross * 0.34));
        const pupilRy = Math.min(openRy - 1.4, (8.8 + curious * 1.9) * (1 - cross * 0.08));
        const maxTravelX = Math.max(2.6, rx - pupilRx - 1.0);
        const maxTravelY = Math.max(2.2, openRy - pupilRy - 0.6);
        const px = Math.max(-maxTravelX, Math.min(maxTravelX, this.eyeLookX * maxTravelX));
        const py = Math.max(-maxTravelY, Math.min(maxTravelY, this.eyeLookY * maxTravelY));
        ctx.fillStyle = s.pupilColor;
        ctx.beginPath();
        ctx.ellipse(px, py, pupilRx, pupilRy, 0, 0, Math.PI * 2);
        ctx.fill();

        if (s.eyeCatchlight || curious > 0.25) {
          const ca = s.eyeCatchlight ? 0.94 : Math.min(0.92, curious * 0.95);
          ctx.fillStyle = `rgba(255, 255, 255, ${ca.toFixed(2)})`;
          ctx.beginPath();
          ctx.arc(px - 3.6 - curious * 0.8, py - 4.0 - curious * 0.5, 2.5 + curious * 0.7, 0, Math.PI * 2);
          ctx.fill();
          ctx.fillStyle = `rgba(255, 255, 255, ${(ca * 0.76).toFixed(2)})`;
          ctx.beginPath();
          ctx.arc(px + 3.2, py + 3.2, 1.2 + curious * 0.4, 0, Math.PI * 2);
          ctx.fill();
        }
      }
      ctx.restore();

      if (s.eyeRimColor && openRy > 2.2) {
        ctx.strokeStyle = s.eyeRimColor;
        ctx.lineWidth = 1.25;
        ctx.beginPath();
        ctx.ellipse(0, 0, rx, openRy, 0, 0, Math.PI * 2);
        ctx.stroke();
      }

      ctx.restore(); // end grumpy eyelid clip

      if (hasGrumpyLid && s.eyeRimColor && openRy > 2.2) {
        ctx.save();
        ctx.beginPath();
        ctx.ellipse(0, 0, rx + 0.9, openRy + 0.9, 0, 0, Math.PI * 2);
        ctx.clip();
        ctx.strokeStyle = s.eyeRimColor;
        ctx.lineWidth = 1.9;
        ctx.lineCap = 'round';
        ctx.beginPath();
        ctx.moveTo(-rx - 2, leftDrop);
        ctx.lineTo(rx + 2, rightDrop);
        ctx.stroke();
        ctx.restore();
      }

      ctx.restore();
    }
  }

  private drawCustomPlumpArm(
    ctx: CanvasRenderingContext2D,
    s: ResolvedSkin,
    side: 'left' | 'right',
    sx: number,
    sy: number,
    px: number,
    py: number,
    squash: number,
    withCrease: boolean,
    isSmear = false,
  ): void {
    // Keep dark-coat tuxedo shoulders cleanly outside the white chest bib
    if (s.isDarkBase && s.bellyBibColor) {
      sx += side === 'left' ? -5 : 5;
    }

    const dx = px - sx;
    const dy = py - sy;
    const len = Math.max(1, Math.hypot(dx, dy));
    const ux = dx / len;
    const uy = dy / len;
    const nx = -uy;
    const ny = ux;
    const wristDist = Math.max(6, len - 8);
    const wx = sx + ux * wristDist;
    const wy = sy + uy * wristDist;
    const rShoulder = s.isDarkBase && s.bellyBibColor ? 13.8 : 15.2;
    const rWrist = 10.8;
    const pawRx = 14.8 * (1 + squash * 0.14);
    const pawRy = 12.0 * (1 - squash * 0.12);

    // Subtle organic plush kitten-arm bow (eliminates stiff straight-stick look!)
    const bowSide = side === 'left' ? -1 : 1;
    const bowAmp = Math.min(7.5, len * 0.052);
    const midX = (sx + wx) * 0.5 + bowSide * bowAmp * Math.abs(uy);
    const midY = (sy + wy) * 0.5 + bowAmp * 0.45 * Math.abs(ux);

    ctx.save();
    const traceArmPath = () => {
      ctx.beginPath();
      ctx.moveTo(sx + nx * rShoulder, sy + ny * rShoulder);
      ctx.quadraticCurveTo(
        midX + nx * (rShoulder * 0.86),
        midY + ny * (rShoulder * 0.86),
        wx + nx * rWrist,
        wy + ny * rWrist,
      );
      ctx.lineTo(wx - nx * rWrist, wy - ny * rWrist);
      ctx.quadraticCurveTo(
        midX - nx * (rShoulder * 0.86),
        midY - ny * (rShoulder * 0.86),
        sx - nx * rShoulder,
        sy - ny * rShoulder,
      );
      ctx.closePath();
    };

    // Match the true rendered flank coat tone (toasted beige on Siamese; orange/charcoal flank patches on Calico!)
    const calicoPatchTone =
      s.pattern === 'calico'
        ? side === 'left'
          ? (s.maskColor || '#ea9446')
          : (s.calicoPatchColor || '#3c3634')
        : null;
    const flankTone = calicoPatchTone
      ? calicoPatchTone
      : s.flankColor
        ? blendHex(s.furSecondaryColor, s.flankColor, 0.45)
        : s.furColor;
    const contrastingArm = s.armColor.toLowerCase() !== s.furColor.toLowerCase();
    const armGrad = ctx.createLinearGradient(sx, sy, px, py);
    if (isSmear) {
      // Soft motion trail ONLY behind moving lower forearm & paw — never a ghost stick on the chest!
      armGrad.addColorStop(0.0, colorWithAlpha(s.armColor, 0.0));
      armGrad.addColorStop(0.45, colorWithAlpha(s.armColor, 0.0));
      armGrad.addColorStop(0.75, colorWithAlpha(s.armColor, 0.56));
      armGrad.addColorStop(1.0, colorWithAlpha(s.pawColor, 0.84));
    } else if (calicoPatchTone) {
      // Calico: upper shoulder flows seamlessly out of the side patch (orange left / charcoal right) into cute white kitten mittens!
      armGrad.addColorStop(0.0, colorWithAlpha(calicoPatchTone, 0.0));
      armGrad.addColorStop(0.09, colorWithAlpha(calicoPatchTone, 1.0));
      armGrad.addColorStop(0.28, colorWithAlpha(calicoPatchTone, 1.0));
      armGrad.addColorStop(0.46, colorWithAlpha(s.armColor, 1.0));
      armGrad.addColorStop(1.0, colorWithAlpha(s.armColor, 1.0));
    } else if (contrastingArm) {
      // Authentic colourpoint transition (e.g. Siamese): melts invisibly out of toasted flank into smooth espresso stockings & chocolate paws!
      const midMocha = blendHex(flankTone, s.armColor, 0.55);
      armGrad.addColorStop(0.0, colorWithAlpha(flankTone, 0.0));
      armGrad.addColorStop(0.11, colorWithAlpha(flankTone, 1.0));
      armGrad.addColorStop(0.24, colorWithAlpha(flankTone, 1.0));
      armGrad.addColorStop(0.56, colorWithAlpha(midMocha, 1.0));
      armGrad.addColorStop(0.84, colorWithAlpha(s.armColor, 1.0));
      armGrad.addColorStop(1.0, colorWithAlpha(s.pawColor, 1.0));
    } else if (s.isDarkBase) {
      // Dark-coat skins (e.g. Tuxedo): crisp clean shoulder on black flank without soft grey feathering onto white bib!
      armGrad.addColorStop(0.0, colorWithAlpha(s.armColor, 0.0));
      armGrad.addColorStop(0.04, colorWithAlpha(s.armColor, 1.0));
      armGrad.addColorStop(1.0, colorWithAlpha(s.armColor, 1.0));
    } else {
      // Same-coat light skins (Orange Tabby, solid): brisk seamless shoulder merge with ZERO body-outline bleed!
      armGrad.addColorStop(0.0, colorWithAlpha(s.armColor, 0.0));
      armGrad.addColorStop(0.10, colorWithAlpha(s.armColor, 1.0));
      armGrad.addColorStop(1.0, colorWithAlpha(s.armColor, 1.0));
    }

    ctx.fillStyle = armGrad;
    traceArmPath();
    ctx.fill();

    // Custom arm spots
    const armPart = side === 'left' ? 'leftArm' : 'rightArm';
    const armSpots = isSmear ? [] : s.spots.filter((sp) => sp.part === armPart);
    if (armSpots.length > 0 && len > 24) {
      const armAng = Math.atan2(dy, dx);
      ctx.save();
      traceArmPath();
      ctx.clip();
      for (const sp of armSpots) {
        const frac = Math.max(0.1, Math.min(0.9, sp.x));
        ctx.fillStyle = colorWithAlpha(sp.color, sp.opacity ?? 0.85);
        ctx.beginPath();
        ctx.ellipse(
          sx + ux * (len * frac) + nx * sp.y,
          sy + uy * (len * frac) + ny * sp.y,
          Math.max(2, sp.rx),
          Math.max(2, sp.ry),
          armAng + (sp.rotation || 0),
          0,
          Math.PI * 2,
        );
        ctx.fill();
      }
      ctx.restore();
    }

    // Mitten paw ball
    ctx.save();
    if (!isSmear && !s.isDarkBase) {
      ctx.shadowColor = 'rgba(36, 32, 30, 0.22)';
      ctx.shadowBlur = 4;
      ctx.shadowOffsetY = 1.5;
    }
    ctx.fillStyle = s.pawColor;
    ctx.beginPath();
    ctx.ellipse(px, py, pawRx, pawRy, 0, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();

    const edgeColor = s.pawOutlineColor || s.outlineColor;
    if (!isSmear && edgeColor) {
      ctx.lineWidth = 1.65;
      ctx.lineCap = 'round';
      const startFrac = withCrease ? 0.22 : 0.16;
      for (const sgn of [-1, 1]) {
        // Exact de Casteljau subdivision of bowed quadratic arm contour from startFrac..1.0
        const p0x = sx + nx * sgn * rShoulder;
        const p0y = sy + ny * sgn * rShoulder;
        const p1x = midX + nx * sgn * (rShoulder * 0.86);
        const p1y = midY + ny * sgn * (rShoulder * 0.86);
        const p2x = wx + nx * sgn * rWrist;
        const p2y = wy + ny * sgn * rWrist;

        const q0x = p0x + (p1x - p0x) * startFrac;
        const q0y = p0y + (p1y - p0y) * startFrac;
        const q1x = p1x + (p2x - p1x) * startFrac;
        const q1y = p1y + (p2y - p1y) * startFrac;
        const esx = q0x + (q1x - q0x) * startFrac;
        const esy = q0y + (q1y - q0y) * startFrac;

        const edgeGrad = ctx.createLinearGradient(esx, esy, p2x, p2y);
        edgeGrad.addColorStop(0.0, colorWithAlpha(edgeColor, 0.0));
        edgeGrad.addColorStop(0.28, colorWithAlpha(edgeColor, 0.74));
        edgeGrad.addColorStop(1.0, colorWithAlpha(edgeColor, 0.90));

        ctx.strokeStyle = edgeGrad;
        ctx.beginPath();
        ctx.moveTo(esx, esy);
        ctx.quadraticCurveTo(q1x, q1y, p2x, p2y);
        ctx.stroke();
      }
      const pawAng = Math.atan2(dy, dx);
      ctx.strokeStyle = colorWithAlpha(edgeColor, 0.92);
      ctx.beginPath();
      ctx.ellipse(px, py, pawRx, pawRy, 0, pawAng - Math.PI * 0.62, pawAng + Math.PI * 0.62);
      ctx.stroke();
    } else if (!isSmear && s.armCreaseColor && withCrease && len > 28) {
      for (const sgn of [-1, 1]) {
        const csx = sx + ux * (len * 0.20) + nx * sgn * (rShoulder * 0.86);
        const csy = sy + uy * (len * 0.20) + ny * sgn * (rShoulder * 0.86);
        const cex = sx + ux * (len * 0.78) + nx * sgn * (rWrist * 1.02);
        const cey = sy + uy * (len * 0.78) + ny * sgn * (rWrist * 1.02);
        const creaseGrad = ctx.createLinearGradient(csx, csy, cex, cey);
        creaseGrad.addColorStop(0.0, colorWithAlpha(s.armCreaseColor, 0.0));
        creaseGrad.addColorStop(0.26, colorWithAlpha(s.armCreaseColor, 0.85));
        creaseGrad.addColorStop(0.84, colorWithAlpha(s.armCreaseColor, 0.72));
        creaseGrad.addColorStop(1.0, colorWithAlpha(s.armCreaseColor, 0.0));
        ctx.strokeStyle = creaseGrad;
        ctx.lineWidth = 1.3;
        ctx.lineCap = 'round';
        ctx.beginPath();
        ctx.moveTo(csx, csy);
        ctx.lineTo(cex, cey);
        ctx.stroke();
      }
    }

    ctx.restore();
  }

  private drawCustomBentArm(
    ctx: CanvasRenderingContext2D,
    s: ResolvedSkin,
    paw: PawState,
    breath: number,
    alpha: number,
  ): void {
    const sh = this.getLiveShoulder(paw, breath);
    const cp = this.getLiveCurledPawPos(paw, breath);
    const sign = paw.side === 'left' ? -1 : 1;
    const elbowX = sh.x + sign * 8;
    const elbowY = cp.y + 8;

    ctx.save();
    ctx.globalAlpha = alpha;
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';

    const calicoPatchTone =
      s.pattern === 'calico'
        ? paw.side === 'left'
          ? (s.maskColor || '#ea9446')
          : (s.calicoPatchColor || '#3c3634')
        : null;
    const flankTone = calicoPatchTone
      ? calicoPatchTone
      : s.flankColor
        ? blendHex(s.furSecondaryColor, s.flankColor, 0.45)
        : s.furColor;
    const contrastingArm = s.armColor.toLowerCase() !== s.furColor.toLowerCase();
    const bentGrad = ctx.createLinearGradient(sh.x, sh.y, cp.x, cp.y);
    if (calicoPatchTone) {
      bentGrad.addColorStop(0.0, colorWithAlpha(calicoPatchTone, 0.0));
      bentGrad.addColorStop(0.16, colorWithAlpha(calicoPatchTone, 1.0));
      bentGrad.addColorStop(0.45, colorWithAlpha(s.armColor, 1.0));
      bentGrad.addColorStop(1.0, colorWithAlpha(s.armColor, 1.0));
    } else if (contrastingArm) {
      const midMocha = blendHex(flankTone, s.armColor, 0.55);
      bentGrad.addColorStop(0.0, colorWithAlpha(flankTone, 0.0));
      bentGrad.addColorStop(0.18, colorWithAlpha(flankTone, 1.0));
      bentGrad.addColorStop(0.56, colorWithAlpha(midMocha, 1.0));
      bentGrad.addColorStop(0.86, colorWithAlpha(s.armColor, 1.0));
      bentGrad.addColorStop(1.0, colorWithAlpha(s.pawColor, 1.0));
    } else {
      bentGrad.addColorStop(0.0, colorWithAlpha(s.armColor, 0.0));
      bentGrad.addColorStop(0.16, colorWithAlpha(s.armColor, 1.0));
      bentGrad.addColorStop(1.0, colorWithAlpha(s.armColor, 1.0));
    }

    ctx.strokeStyle = bentGrad;
    ctx.lineWidth = 21;
    ctx.beginPath();
    ctx.moveTo(sh.x, sh.y);
    ctx.quadraticCurveTo(elbowX, elbowY, cp.x, cp.y);
    ctx.stroke();

    const armPart = paw.side === 'left' ? 'leftArm' : 'rightArm';
    for (const sp of s.spots) {
      if (sp.part !== armPart) continue;
      ctx.save();
      ctx.fillStyle = colorWithAlpha(sp.color, sp.opacity ?? 0.82);
      ctx.beginPath();
      ctx.ellipse((elbowX + cp.x) * 0.5, (elbowY + cp.y) * 0.5, Math.min(8, sp.rx), Math.min(6, sp.ry), -0.3 * sign, 0, Math.PI * 2);
      ctx.fill();
      ctx.restore();
    }

    ctx.fillStyle = s.pawColor;
    ctx.beginPath();
    ctx.ellipse(cp.x, cp.y, 12.8, 11.2, sign * 0.2, 0, Math.PI * 2);
    ctx.fill();

    const outlineCol = s.pawOutlineColor || s.legLineColor;
    const outlineGrad = ctx.createLinearGradient(elbowX, elbowY - 12, cp.x, cp.y + 12);
    outlineGrad.addColorStop(0.0, colorWithAlpha(outlineCol, 0.0));
    outlineGrad.addColorStop(0.28, colorWithAlpha(outlineCol, 0.68));
    outlineGrad.addColorStop(0.86, colorWithAlpha(outlineCol, 0.78));
    outlineGrad.addColorStop(1.0, colorWithAlpha(outlineCol, 0.35));

    ctx.strokeStyle = outlineGrad;
    ctx.lineWidth = 1.6;
    ctx.beginPath();
    ctx.moveTo(elbowX + sign * 8, elbowY - 16);
    ctx.quadraticCurveTo(elbowX + sign * 4, elbowY + 12, cp.x + sign * 2, cp.y + 10);
    ctx.quadraticCurveTo(cp.x - sign * 11, cp.y + 8, cp.x - sign * 12, cp.y - 4);
    ctx.stroke();
    ctx.restore();
  }

  private drawCustomIdleLegDetails(
    ctx: CanvasRenderingContext2D,
    s: ResolvedSkin,
    paw: PawState,
    topSp: Vec2,
    midSp: Vec2,
    botSp: Vec2,
    sign: -1 | 1,
    outerTopX: number,
    outerBotX: number,
    innerTopX: number,
    innerBotX: number,
  ): void {
    const armPart = paw.side === 'left' ? 'leftArm' : 'rightArm';
    for (const sp of s.spots) {
      if (sp.part !== armPart) continue;
      ctx.save();
      ctx.globalAlpha = sp.opacity ?? 0.82;
      ctx.fillStyle = sp.color;
      ctx.beginPath();
      ctx.ellipse(midSp.x + sign * 21, midSp.y + (sp.x - 0.5) * 28, Math.max(3, sp.rx * 0.8), Math.max(3, sp.ry * 1.1), 0.2 * sign, 0, Math.PI * 2);
      ctx.fill();
      ctx.restore();
    }

    // Optional cute white mittens at the bottom of idle seated legs when pawColor differs from furColor!
    if (s.pawColor !== s.furColor) {
      ctx.save();
      ctx.fillStyle = s.pawColor;
      ctx.beginPath();
      ctx.ellipse(botSp.x + sign * 22, botSp.y - 4, 13.5, 9.5, 0, 0, Math.PI * 2);
      ctx.fill();
      ctx.restore();
    }

    ctx.strokeStyle = s.legLineColor;
    ctx.lineWidth = 1.5;
    ctx.lineCap = 'round';
    ctx.beginPath();
    ctx.moveTo(outerTopX, topSp.y);
    ctx.quadraticCurveTo(midSp.x + sign * 41, midSp.y, outerBotX, botSp.y - 1);
    ctx.stroke();

    ctx.beginPath();
    ctx.moveTo(innerTopX, topSp.y + 10);
    ctx.lineTo(innerBotX, botSp.y - 1);
    ctx.stroke();
  }

  private roundRect(
    ctx: CanvasRenderingContext2D,
    x: number,
    y: number,
    w: number,
    h: number,
    r: number,
  ): void {
    const rad = Math.min(r, w / 2, h / 2);
    ctx.beginPath();
    ctx.moveTo(x + rad, y);
    ctx.arcTo(x + w, y, x + w, y + h, rad);
    ctx.arcTo(x + w, y + h, x, y + h, rad);
    ctx.arcTo(x, y + h, x, y, rad);
    ctx.arcTo(x, y, x + w, y, rad);
    ctx.closePath();
  }

  private trapezoidPath(
    ctx: CanvasRenderingContext2D,
    topHalfW: number,
    botHalfW: number,
    topY: number,
    botY: number,
    r: number,
  ): void {
    ctx.beginPath();
    ctx.moveTo(-topHalfW + r, topY);
    ctx.lineTo(topHalfW - r, topY);
    ctx.quadraticCurveTo(topHalfW, topY, topHalfW + 1, topY + r);
    ctx.lineTo(botHalfW, botY - r);
    ctx.quadraticCurveTo(botHalfW, botY, botHalfW - r, botY);
    ctx.lineTo(-botHalfW + r, botY);
    ctx.quadraticCurveTo(-botHalfW, botY, -botHalfW, botY - r);
    ctx.lineTo(-topHalfW - 1, topY + r);
    ctx.quadraticCurveTo(-topHalfW, topY, -topHalfW + r, topY);
    ctx.closePath();
  }
}
