/**
 * Ported from KuroCat (https://github.com/qianyiai/kurocat), v5.1.0.
 * Copyright (c) 2026 KuroCat Contributors, MIT License.
 * See src-tauri/resources/licenses/KUROCAT-MIT.txt.
 */
import * as THREE from 'three';
import type { CatSkinConfig } from './catSkins';
import type { KeyboardLayoutId } from './keyboardLayout';
import type { KeyboardOrientation, TailKeyMode } from './catEngine';
import { DEFAULT_TAIL_KEY_IDS } from './catEngine';
import { catAudio } from './audio';
import {
  FurShaderSystem,
  resolveFurPalette3D,
  type FurPalette3D,
  type RenderStyle3D,
} from './furShader3d';
import {
  KeyboardAndMouse3D,
  Spring1D,
  Spring3D,
  TaperTube3D,
  approach,
  clamp,
  mergeStaticGeometries,
  mix,
  smoothstep,
} from './keyboard3d';

export type CollarColorId = 'none' | 'red' | 'blue' | 'pink' | 'green';

const COLLAR_COLORS: Record<Exclude<CollarColorId, 'none'>, string> = {
  red: '#d9483b',
  blue: '#3d7cc9',
  pink: '#e57b98',
  green: '#4fa368',
};

const EYE_MAX_RX = 0.175;
const EYE_MAX_RY = 0.160;
const Z_SCALE = 0.82;
const BODY_H = 1.98;
const HEAD_CY = 1.56;

interface PawTrack3D {
  side: -1 | 1;
  pos: THREE.Vector3;
  from: THREE.Vector3;
  last: THREE.Vector3;
  spring: Spring3D;
  keyId: string | null;
  t0: number;
  dur: number;
  down: boolean;
  lastActive: number;
  tuckWeight: number;
}

interface Popup3D {
  keyId: string;
  sprite: THREE.Sprite;
  baseW: number;
  baseH: number;
  t0: number;
  upAt: number | null;
  side: -1 | 1;
}

interface Fx3D {
  sprite: THREE.Sprite;
  kind: 'bang' | 'dust' | 'z';
  vel: THREE.Vector3;
  t0: number;
  dur: number;
  size: number;
  baseX: number;
}

interface IdleAct3D {
  kind: 'tilt' | 'ear' | 'slowBlink' | 'look' | 'tail';
  t0: number;
  dur: number;
  side: -1 | 1;
}

interface SimpleLimb3D {
  fill: TaperTube3D;
  caps: THREE.Mesh[];
  update(point: (t: number, out: THREE.Vector3) => void, radius: (t: number) => number): void;
}

const CAP_GEO = new THREE.SphereGeometry(1, 10, 8);

function createCleanLimb(
  parent: THREE.Object3D,
  opts: {
    segments: number;
    radial: number;
    fill: THREE.Material;
    order?: number;
    capStart?: boolean;
    capEnd?: boolean;
  },
): SimpleLimb3D {
  const order = opts.order ?? 0;
  const fill = new TaperTube3D(opts.segments, opts.radial, opts.fill, order);
  parent.add(fill.mesh);

  const caps: THREE.Mesh[] = [];
  const capTs: number[] = [];
  if (opts.capStart !== false) capTs.push(0);
  if (opts.capEnd !== false) capTs.push(1);
  for (let i = 0; i < capTs.length; i++) {
    const cap = new THREE.Mesh(CAP_GEO, opts.fill);
    cap.renderOrder = order;
    parent.add(cap);
    caps.push(cap);
  }
  const tmp = new THREE.Vector3();

  return {
    fill,
    caps,
    update(point, radius) {
      fill.update(point, radius);
      capTs.forEach((t, idx) => {
        point(t, tmp);
        const r = Math.max(0.002, radius(t));
        caps[idx].position.copy(tmp);
        caps[idx].scale.setScalar(r);
      });
    },
  };
}

function withTowardEye(mat: THREE.Material, lift: number): THREE.Material {
  mat.onBeforeCompile = (s) => {
    s.vertexShader = s.vertexShader.replace(
      '#include <project_vertex>',
      `vec4 mvPosition = vec4(transformed, 1.0);
      #ifdef USE_INSTANCING
        mvPosition = instanceMatrix * mvPosition;
      #endif
      mvPosition = modelViewMatrix * mvPosition;
      mvPosition.xyz -= normalize(mvPosition.xyz) * ${lift.toFixed(4)};
      gl_Position = projectionMatrix * mvPosition;`,
    );
  };
  return mat;
}

const spriteTexCache = new Map<string, { tex: THREE.CanvasTexture; aspect: number }>();

function getPopupTexture(text: string): { tex: THREE.CanvasTexture; aspect: number } {
  const key = `pop:${text}`;
  const hit = spriteTexCache.get(key);
  if (hit) return hit;

  const h = 110;
  const font = '700 48px "Segoe UI", system-ui, sans-serif';
  const probe = document.createElement('canvas').getContext('2d')!;
  probe.font = font;
  const w = Math.max(114, Math.ceil(probe.measureText(text).width) + 52);

  const c = document.createElement('canvas');
  c.width = w;
  c.height = h;
  const g = c.getContext('2d')!;
  const r = 24;
  g.fillStyle = '#fbfaf3';
  g.strokeStyle = '#24262e';
  g.lineWidth = 6;
  g.beginPath();
  g.moveTo(r + 4, 5);
  g.lineTo(w - r - 4, 5);
  g.quadraticCurveTo(w - 4, 5, w - 4, r + 5);
  g.lineTo(w - 4, h - r - 5);
  g.quadraticCurveTo(w - 4, h - 5, w - r - 4, h - 5);
  g.lineTo(r + 4, h - 5);
  g.quadraticCurveTo(4, h - 5, 4, h - r - 5);
  g.lineTo(4, r + 5);
  g.quadraticCurveTo(4, 5, r + 4, 5);
  g.closePath();
  g.fill();
  g.stroke();

  g.fillStyle = '#181a20';
  g.font = font;
  g.textAlign = 'center';
  g.textBaseline = 'middle';
  g.fillText(text, w / 2, h / 2 + 2);

  const tex = new THREE.CanvasTexture(c);
  tex.colorSpace = THREE.SRGBColorSpace;
  const entry = { tex, aspect: w / h };
  spriteTexCache.set(key, entry);
  return entry;
}

function getFxTexture(kind: 'z' | 'anger' | 'bang' | 'dust'): THREE.CanvasTexture {
  const key = `fx:${kind}`;
  const hit = spriteTexCache.get(key);
  if (hit) return hit.tex;

  const c = document.createElement('canvas');
  c.width = 128;
  c.height = 128;
  const g = c.getContext('2d')!;
  if (kind === 'z') {
    g.font = '800 84px "Segoe UI", system-ui, sans-serif';
    g.textAlign = 'center';
    g.textBaseline = 'middle';
    g.lineJoin = 'round';
    g.lineWidth = 14;
    g.strokeStyle = '#15161c';
    g.strokeText('Z', 64, 68);
    g.fillStyle = '#f8f8f2';
    g.fillText('Z', 64, 68);
  } else if (kind === 'anger') {
    g.lineCap = 'round';
    const drawArcs = () => {
      for (const [cx, cy, a0, a1] of [
        [24, 24, 0.1, 1.45],
        [104, 24, 1.7, 3.05],
        [104, 104, 3.25, 4.6],
        [24, 104, 4.82, 6.18],
      ]) {
        g.beginPath();
        g.arc(cx, cy, 31, a0, a1);
        g.stroke();
      }
    };
    g.strokeStyle = '#14151a';
    g.lineWidth = 28;
    drawArcs();
    g.stroke();
    g.strokeStyle = '#e5483d';
    g.lineWidth = 14;
    drawArcs();
    g.stroke();
  } else if (kind === 'bang') {
    g.beginPath();
    for (let i = 0; i < 20; i++) {
      const a = (i / 20) * Math.PI * 2;
      const r = i % 2 ? 26 : 58;
      if (i === 0) g.moveTo(64 + Math.cos(a) * r, 64 + Math.sin(a) * r);
      else g.lineTo(64 + Math.cos(a) * r, 64 + Math.sin(a) * r);
    }
    g.closePath();
    g.fillStyle = '#fbfaf3';
    g.fill();
    g.strokeStyle = '#1d1d1b';
    g.lineWidth = 7;
    g.stroke();
  } else {
    g.beginPath();
    g.arc(64, 64, 44, 0, Math.PI * 2);
    g.fillStyle = '#f4f1e6';
    g.fill();
    g.strokeStyle = '#b9b4a4';
    g.lineWidth = 8;
    g.stroke();
  }
  const tex = new THREE.CanvasTexture(c);
  tex.colorSpace = THREE.SRGBColorSpace;
  spriteTexCache.set(key, { tex, aspect: 1 });
  return tex;
}

function makeSoftShadowTex(): THREE.CanvasTexture {
  const key = 'shadow:cat';
  const hit = spriteTexCache.get(key);
  if (hit) return hit.tex;
  const c = document.createElement('canvas');
  c.width = 256;
  c.height = 256;
  const g = c.getContext('2d')!;
  const grad = g.createRadialGradient(128, 128, 28, 128, 128, 122);
  grad.addColorStop(0, 'rgba(0, 0, 0, 0.50)');
  grad.addColorStop(0.55, 'rgba(0, 0, 0, 0.22)');
  grad.addColorStop(1, 'rgba(0, 0, 0, 0)');
  g.fillStyle = grad;
  g.fillRect(0, 0, 256, 256);
  const tex = new THREE.CanvasTexture(c);
  tex.colorSpace = THREE.SRGBColorSpace;
  spriteTexCache.set(key, { tex, aspect: 1 });
  return tex;
}

function makeBlushTexture(): THREE.CanvasTexture {
  const key = 'blush:soft';
  const hit = spriteTexCache.get(key);
  if (hit) return hit.tex;
  const c = document.createElement('canvas');
  c.width = 64;
  c.height = 64;
  const g = c.getContext('2d')!;
  const grad = g.createRadialGradient(32, 32, 4, 32, 32, 30);
  grad.addColorStop(0, 'rgba(244, 136, 158, 1)');
  grad.addColorStop(0.55, 'rgba(244, 136, 158, 0.52)');
  grad.addColorStop(1, 'rgba(244, 136, 158, 0)');
  g.fillStyle = grad;
  g.fillRect(0, 0, 64, 64);
  const tex = new THREE.CanvasTexture(c);
  tex.colorSpace = THREE.SRGBColorSpace;
  spriteTexCache.set(key, { tex, aspect: 1 });
  return tex;
}

/**
 * Analytical anti-aliased GLSL eye shader reproducing original KuroCat's signature eyes:
 * - Luminous lemon-chartreuse almond-oval with subtle cheeky outward tilt (±0.10 rad)
 * - Clean calm Black Cat stare (no distracting permanent dots), popping starlight catchlights in Curious mode
 * - Zero-seam slanted grumpy upper eyelid clip revealing real 3D shaded fur/mask underneath
 * - Strict inside-iris pupil clipping & real 3D depthTest occlusion when orbiting behind the cat
 */
function createEyeShaderMaterial(side: -1 | 1): THREE.ShaderMaterial {
  return new THREE.ShaderMaterial({
    uniforms: {
      uSide: { value: side },
      uScale: { value: new THREE.Vector2(0.156 / EYE_MAX_RX, 0.136 / EYE_MAX_RY) },
      uTilt: { value: side * -0.09 },
      uEyeTop: { value: new THREE.Color('#d7f346') },
      uEyeBottom: { value: new THREE.Color('#bde62c') },
      uPupilCol: { value: new THREE.Color('#0c0d0f') },
      uLashCol: { value: new THREE.Color('#0b0c10') },
      uPupilCenter: { value: new THREE.Vector2(0, 0) },
      uPupilRad: { value: new THREE.Vector2(0.43, 0.67) },
      uCross: { value: 0 },
      uCurious: { value: 0 },
      uShineAmt: { value: 0 },
      uIs3D: { value: 1 },
      uLimbalAmt: { value: 0.28 },
    },
    transparent: true,
    depthTest: true,
    depthWrite: false,
    polygonOffset: true,
    polygonOffsetFactor: -2,
    polygonOffsetUnits: -2,
    side: THREE.FrontSide,
    vertexShader: `
      varying vec2 vEye;
      void main() {
        vEye = uv * 2.0 - 1.0;
        gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
      }
    `,
    fragmentShader: `
      uniform float uSide;
      uniform vec2 uScale;
      uniform float uTilt;
      uniform vec3 uEyeTop;
      uniform vec3 uEyeBottom;
      uniform vec3 uPupilCol;
      uniform vec3 uLashCol;
      uniform vec2 uPupilCenter;
      uniform vec2 uPupilRad;
      uniform float uCross;
      uniform float uCurious;
      uniform float uShineAmt;
      uniform float uIs3D;
      uniform float uLimbalAmt;
      varying vec2 vEye;

      void main() {
        // Apply KuroCat's subtle cheeky outward eye tilt
        float ct = cos(uTilt);
        float st = sin(uTilt);
        vec2 rotEye = vec2(vEye.x * ct - vEye.y * st, vEye.x * st + vEye.y * ct);
        vec2 p = rotEye / max(uScale, vec2(0.05));

        float r = length(p);
        float aa = max(fwidth(r), 0.016);
        float outerMask = 1.0 - smoothstep(1.0 - aa, 1.0 + aa, r);
        if (outerMask < 0.004) discard;

        // Zero-seam grumpy slanted upper eyelid: discards above angled brow so real 3D fur shows cleanly
        float lidAlpha = 1.0;
        float lashMask = 0.0;
        float lidShadow = 0.0;
        if (uCross > 0.01) {
          float lidY = mix(1.18, 0.22, uCross) + uSide * p.x * 0.34 * uCross;
          float lashH = 0.105 * smoothstep(0.05, 0.35, uCross);
          float lidDist = p.y - lidY;
          if (lidDist > lashH + aa) discard;
          lidAlpha = 1.0 - smoothstep(lashH - aa, lashH + aa, lidDist);
          lashMask = smoothstep(-aa, aa, lidDist);
          lidShadow = smoothstep(-0.24, 0.0, lidDist) * 0.25 * uCross;
        }

        // Luminous glass-cabochon iris gradient
        float ty = clamp(0.5 - p.y * 0.5, 0.0, 1.0);
        vec3 col = mix(uEyeTop, uEyeBottom, ty);
        if (uIs3D > 0.5) {
          col *= mix(1.0, 0.88, smoothstep(0.26, 0.94, p.y));
          float crescent = smoothstep(0.08, 0.85, -p.y) * (1.0 - smoothstep(0.68, 0.94, r));
          col = mix(col, min(col * 1.18 + vec3(0.04), vec3(1.0)), crescent * 0.36);
        }
        float limbal = smoothstep(0.84, 0.98, r);
        col = mix(col, col * 0.50 + vec3(0.01), limbal * uLimbalAmt);

        // Crisp elliptical pupil strictly clipped inside iris boundary
        vec2 pd = (p - uPupilCenter) / max(uPupilRad, vec2(0.05));
        float pr = length(pd);
        float paa = max(fwidth(pr), 0.032);
        float pupilMask = (1.0 - smoothstep(1.0 - paa, 1.0 + paa, pr)) * (1.0 - smoothstep(0.90, 0.96, r));
        col = mix(col, uPupilCol, pupilMask);

        col *= (1.0 - lidShadow);

        // Specular catchlights (off on calm Black Cat for iconic KuroCat gaze; sparkles in Curious & light breeds!)
        if (uShineAmt > 0.02) {
          float sAmt = clamp(uShineAmt, 0.0, 1.35);
          vec2 d1 = p - (uPupilCenter * 0.42 + vec2(-0.28, 0.32));
          float r1 = 0.205 * sAmt;
          float h1 = 1.0 - smoothstep(r1 - 0.026, r1 + 0.020, length(d1));

          vec2 d2 = p - (uPupilCenter * 0.34 + vec2(0.29, -0.30));
          float r2 = 0.10 * sAmt;
          float h2 = 1.0 - smoothstep(r2 - 0.022, r2 + 0.018, length(d2));

          float h3 = 0.0;
          if (uCurious > 0.15) {
            vec2 d3 = p - (uPupilCenter * 0.30 + vec2(-0.36, -0.17));
            float r3 = 0.085 * uCurious;
            h3 = 1.0 - smoothstep(r3 - 0.020, r3 + 0.016, length(d3));
          }
          float hi = clamp(max(h1, max(h2, h3)), 0.0, 1.0) * (1.0 - smoothstep(0.90, 0.97, r));
          col = mix(col, vec3(1.0), hi * min(1.0, uShineAmt * 1.15));
        }

        col = mix(col, uLashCol, lashMask);

        float alpha = outerMask * lidAlpha;
        if (alpha < 0.008) discard;
        gl_FragColor = vec4(col, alpha);
        #include <colorspace_fragment>
      }
    `,
  });
}

/**
 * Lightweight studio-lit plastic / plush / brushed-metal shader for 3D gaming headphones.
 * Matches KuroCat's volumetric fur key light in `3d` mode and switches cleanly to flat cel colors in `3d-flat`.
 */
function createHeadsetMaterial(
  hexColor: string,
  specStrength = 0.22,
  rimStrength = 0.24,
  rimHex = '#8ea2c4',
): THREE.ShaderMaterial {
  return new THREE.ShaderMaterial({
    uniforms: {
      uBase: { value: new THREE.Color(hexColor) },
      uRim: { value: new THREE.Color(rimHex) },
      uSpec: { value: specStrength },
      uRimAmt: { value: rimStrength },
      uIs3D: { value: 1 },
    },
    vertexShader: `
      varying vec3 vNormalV;
      varying vec3 vViewPos;
      void main() {
        vec4 mvPos = modelViewMatrix * vec4(position, 1.0);
        vViewPos = -mvPos.xyz;
        vNormalV = normalize(normalMatrix * normal);
        gl_Position = projectionMatrix * mvPos;
      }
    `,
    fragmentShader: `
      uniform vec3 uBase;
      uniform vec3 uRim;
      uniform float uSpec;
      uniform float uRimAmt;
      uniform float uIs3D;
      varying vec3 vNormalV;
      varying vec3 vViewPos;
      void main() {
        vec3 col = uBase;
        if (uIs3D > 0.5) {
          vec3 N = normalize(vNormalV);
          vec3 V = normalize(vViewPos);
          vec3 L = normalize(vec3(-0.35, 0.72, 0.60));
          float ndl = dot(N, L);
          float wrap = clamp(ndl * 0.52 + 0.58, 0.22, 1.15);
          col = uBase * mix(0.62, 1.18, wrap);
          vec3 H = normalize(L + V);
          float spec = pow(max(dot(N, H), 0.0), 26.0) * uSpec;
          float fres = pow(1.0 - max(dot(N, V), 0.0), 2.4) * uRimAmt;
          col += vec3(spec) + uRim * fres;
        }
        gl_FragColor = vec4(col, 1.0);
        #include <colorspace_fragment>
      }
    `,
  });
}

/**
 * Original KuroCat 3D Engine:
 * Sculpted directly from KuroCat's iconic continuous BODY_SLICES mochi-pear silhouette
 * (NO KeyCat white sticker rim, NO bobblehead neck seam, NO tall KeyCat ears, NO wired mouse).
 */
export class KuroCatEngine3D {
  public showKeyBubbles = true;
  public privacyMode = false;

  private readonly canvas: HTMLCanvasElement;
  private readonly renderer: THREE.WebGLRenderer;
  private readonly scene = new THREE.Scene();
  private readonly camera: THREE.PerspectiveCamera;
  private readonly desk: KeyboardAndMouse3D;
  private readonly furSystem = new FurShaderSystem();

  private readonly catRoot = new THREE.Group();
  private readonly homePos = new THREE.Vector3(0.02, 0, -0.02);
  private readonly bodyGeo: THREE.LatheGeometry;
  private readonly bodyMesh: THREE.Mesh;
  private readonly outlineSamplePts: THREE.Vector3[];

  // Head mount tracks exact head pivot inside the continuous one-piece body mesh
  private readonly headMount = new THREE.Group();
  private readonly headsetGroup = new THREE.Group();
  private readonly headsetShaderMats: THREE.ShaderMaterial[] = [];
  private readonly headsetLedMat = new THREE.MeshBasicMaterial({ color: '#d4f53d' });
  private readonly headsetMicLedMat = new THREE.MeshBasicMaterial({ color: '#52f7d4' });
  private readonly sHeadset = new Spring1D(0, 5.4, 0.70);
  private headsetOn = false;
  private readonly collarGroup = new THREE.Group();
  private readonly collarMat: THREE.MeshBasicMaterial;
  private readonly bellGroup = new THREE.Group();
  private readonly collarY = 1.32;
  private readonly collarR: number;

  private readonly ears: Array<{ group: THREE.Group; mesh: THREE.Mesh; s: -1 | 1; tilt: number }> = [];
  private readonly eyes: Array<{
    s: -1 | 1;
    mesh: THREE.Mesh;
    mat: THREE.ShaderMaterial;
    closedArc: THREE.Mesh;
    squintSlit: THREE.Mesh;
    blush: THREE.Mesh;
  }> = [];
  private readonly faceGroup = new THREE.Group();

  // Shared materials updated on skin switch
  private readonly pawMat: THREE.ShaderMaterial;
  private readonly toeMat = new THREE.MeshBasicMaterial({ color: '#343640', depthTest: true });
  private readonly lidArcMat = new THREE.MeshBasicMaterial({
    color: '#d4f044',
    depthTest: true,
    polygonOffset: true,
    polygonOffsetFactor: -2,
    polygonOffsetUnits: -2,
  });
  private readonly squintMat = new THREE.MeshBasicMaterial({
    color: '#d4f044',
    depthTest: true,
    polygonOffset: true,
    polygonOffsetFactor: -2,
    polygonOffsetUnits: -2,
  });
  private readonly whiskerMat = new THREE.MeshBasicMaterial({
    color: '#d4d8e0',
    transparent: true,
    opacity: 0.62,
    depthTest: true,
    depthWrite: false,
  });
  private readonly noseMat = new THREE.MeshBasicMaterial({
    color: '#ee8e9e',
    depthTest: true,
    polygonOffset: true,
    polygonOffsetFactor: -2,
    polygonOffsetUnits: -2,
  });
  private readonly mouthMat = new THREE.MeshBasicMaterial({
    color: '#5c524c',
    depthTest: true,
    polygonOffset: true,
    polygonOffsetFactor: -2,
    polygonOffsetUnits: -2,
  });
  private readonly blushMat = new THREE.MeshBasicMaterial({
    map: makeBlushTexture(),
    transparent: true,
    opacity: 0,
    depthTest: true,
    depthWrite: false,
    polygonOffset: true,
    polygonOffsetFactor: -1,
    polygonOffsetUnits: -1,
  });
  private readonly tuckLineMat = new THREE.MeshBasicMaterial({
    color: '#aeb4c0',
    transparent: true,
    opacity: 0.36,
    depthTest: true,
    depthWrite: false,
  });

  // Articulated sleek arms & graceful tail
  private readonly arms: Array<{
    s: -1 | 1;
    limb: SimpleLimb3D;
    paw: THREE.Mesh;
    tuckArc: THREE.Mesh;
    shoulderLocal: THREE.Vector3;
    shoulder: THREE.Vector3;
    c1: THREE.Vector3;
    c2: THREE.Vector3;
    hand: THREE.Vector3;
  }> = [];
  private readonly tailLimb: SimpleLimb3D;
  private readonly tailCurve: THREE.CatmullRomCurve3;
  // Sleek graceful KuroCat tail curving smoothly out to the right & gently curling up at tip
  private readonly tailRestPts = [
    new THREE.Vector3(0.56, 0.24, -0.14),
    new THREE.Vector3(0.84, 0.16, -0.18),
    new THREE.Vector3(1.10, 0.24, -0.20),
    new THREE.Vector3(1.26, 0.42, -0.18),
    new THREE.Vector3(1.36, 0.56, -0.16),
    new THREE.Vector3(1.48, 0.62, -0.14),
  ];
  private readonly tailSleepPts = [
    new THREE.Vector3(0.56, 0.12, -0.14),
    new THREE.Vector3(0.96, 0.10, 0.02),
    new THREE.Vector3(1.14, 0.10, 0.30),
    new THREE.Vector3(1.04, 0.10, 0.58),
    new THREE.Vector3(0.82, 0.10, 0.72),
    new THREE.Vector3(0.56, 0.10, 0.76),
  ];
  private readonly tailThumpPts = [
    new THREE.Vector3(0.56, 0.24, -0.14),
    new THREE.Vector3(0.84, 0.12, -0.12),
    new THREE.Vector3(1.08, 0.06, -0.02),
    new THREE.Vector3(1.26, 0.05, 0.10),
    new THREE.Vector3(1.42, 0.05, 0.18),
    new THREE.Vector3(1.56, 0.06, 0.22),
  ];

  private readonly angerSprite: THREE.Sprite;
  private readonly popups: Popup3D[] = [];
  private readonly fxList: Fx3D[] = [];
  private readonly petMeshes: THREE.Object3D[] = [];
  private readonly raycaster = new THREE.Raycaster();

  // 240Hz Springs
  private readonly sShift = new Spring1D(0, 1.85, 0.72);
  private readonly sHunch = new Spring1D(0, 1.75, 0.78);
  private readonly sSquash = new Spring1D(0, 2.3, 0.54);
  private readonly sYaw = new Spring1D(0, 2.8, 0.72);
  private readonly sPitch = new Spring1D(-0.04, 2.6, 0.70);
  private readonly sRoll = new Spring1D(0, 2.4, 0.62);
  private readonly sGazeX = new Spring1D(0, 9.2, 0.94);
  private readonly sGazeY = new Spring1D(0, 9.2, 0.94);
  private readonly sEars = new Spring1D(0, 5.2, 0.36);
  private readonly sCross = new Spring1D(0, 1.8, 0.88);
  private readonly sCurious = new Spring1D(0, 3.6, 0.74);
  private readonly sDeskHop = new Spring1D(0, 9.2, 0.34);

  // Turntable 360° orbit camera springs (0 = iconic KuroCat elevated front angle)
  private orbitAzimuthTarget = 0;
  private orbitElevTarget = 24.5;
  private readonly sOrbitAz = new Spring1D(0, 4.2, 0.86);
  private readonly sOrbitEl = new Spring1D(24.5, 4.2, 0.86);
  private readonly camAnchor = new THREE.Vector3(0.06, 0.82, 0.36);

  // State machine
  private currentSkin: CatSkinConfig | null = null;
  private currentPalette: FurPalette3D | null = null;
  private renderStyle: RenderStyle3D = '3d';
  private collarId: CollarColorId = 'none';
  private embedded = false;
  private renderScale = 1.0;
  private tailKeyMode: TailKeyMode = 'smart';
  private tailKeyIdSet = new Set<string>(DEFAULT_TAIL_KEY_IDS);
  private bend = { x: 0, z: 0, squash: 0, bellySwell: 0 };
  private headRot = { pitch: -0.04, yaw: 0, roll: 0 };

  private paws: { L: PawTrack3D; R: PawTrack3D };
  private tailTap = {
    keyId: null as string | null,
    t0: -100,
    dur: 0.17,
    from: 0,
    down: false,
    landAt: -100,
    reach: 0,
    hover: 0.12,
    tip: new THREE.Vector3(),
  };

  private rafId: number | null = null;
  private idleTimer: number | null = null;
  private isIdlePacing = false;
  private lastFrameMs = 0;
  private directRafPrevMs = 0;
  private vsyncMs = 16.67;
  private vsyncSamples = 0;
  private gameActive = false;
  private lastCamKey = '';
  private elapsed = 0;
  private lastInput = 0;
  private lastKey = -100;
  private lastMouse = -100;
  private lastMouseNX = 0.5;
  private lastMouseNY = 0.5;
  private lastCursorRelX = 0.5;
  private lastCursorRelY = 0.5;
  private leftMouseDown = false;
  private rightMouseDown = false;
  private cursorNdc = new THREE.Vector2(0, 0);
  private cursorActiveAt = -100;
  private pointerSamples: Array<{ t: number; dist: number }> = [];
  private lastSampleX: number | null = null;
  private lastSampleY: number | null = null;
  private lastSampleT = -100;

  private petting = false;
  private petUntil = -100;
  /** PulseDeck：鼠标悬停猫身即眯眼（不要求来回抚摸） */
  private hovering = false;
  private petX = 0;
  private rub = 0;
  private squintAmt = 0;
  private asleep = false;
  private sleepAmt = 0;
  private wakeAt = -100;
  private nextBlink = 2.8;
  private nextZ = 0;
  private privacyClose = 0;

  private strokes: number[] = [];
  private excite = 0;
  private tailPhase = 0;
  private annoy = 0;
  private lastPoke = -100;
  private isCross = false;
  private crossSince = -100;
  private crossHuffDir: -1 | 1 = -1;
  private thumpPhase = 0;
  private thumpLift = 0;
  private thumpLash = 0;
  private lastSlam = -100;
  private curiousUntil = -100;
  private idleAct: IdleAct3D | null = null;
  private nextIdleAct = 4.5;

  private readonly tmpV1 = new THREE.Vector3();
  private readonly tmpV2 = new THREE.Vector3();
  private readonly tmpV3 = new THREE.Vector3();
  private readonly tmpTailAttached = new THREE.Vector3();
  private readonly tmpTailLoose = new THREE.Vector3();
  private readonly tmpTailScratch = new THREE.Vector3();
  private readonly tmpTailPath = [
    new THREE.Vector3(),
    new THREE.Vector3(),
    new THREE.Vector3(),
    new THREE.Vector3(),
  ];
  private readonly tmpPawLocal = new THREE.Vector3();
  private readonly tmpArmOff = new THREE.Vector3();
  private readonly tmpFxPos = new THREE.Vector3();

  constructor(canvas: HTMLCanvasElement, options?: { embedded?: boolean }) {
    this.canvas = canvas;
    this.embedded = Boolean(options?.embedded);
    this.renderer = new THREE.WebGLRenderer({
      canvas,
      antialias: true,
      alpha: true,
      stencil: false,
      premultipliedAlpha: true,
      powerPreference: 'low-power',
    });
    this.renderer.setPixelRatio(Math.min(1.5, window.devicePixelRatio || 1));
    this.renderer.setClearColor(0x000000, 0);
    this.renderer.outputColorSpace = THREE.SRGBColorSpace;

    this.camera = new THREE.PerspectiveCamera(22, 1.35, 0.1, 100);
    this.desk = new KeyboardAndMouse3D(this.scene);
    this.pawMat = this.furSystem.createMaterial('PAW');

    // 1) Original KuroCat Continuous One-Piece Mochi-Pear Silhouette (exact BODY_SLICES proportions!)
    this.catRoot.position.copy(this.homePos);
    this.catRoot.scale.setScalar(0.90);
    this.scene.add(this.catRoot);

    // Soft studio contact shadow underneath the plush sitting body
    const catShadow = new THREE.Mesh(
      new THREE.PlaneGeometry(2.15, 1.75),
      new THREE.MeshBasicMaterial({
        map: makeSoftShadowTex(),
        transparent: true,
        depthWrite: false,
        opacity: 0.78,
      }),
    );
    catShadow.rotation.x = -Math.PI / 2;
    catShadow.position.set(0, 0.003, 0.02);
    this.catRoot.add(catShadow);

    // Exact measured KuroCat BODY_SLICES (forehead 49 -> cheeks 60 -> plush neck 52 -> chest 62 -> belly 70 -> thighs 73 -> base 46)
    const bodyCurve = new THREE.CatmullRomCurve3(
      [
        [0.0, 0.0],
        [0.46, 0.006],
        [0.60, 0.06],
        [0.74, 0.19],
        [0.80, 0.43], // widest soft thighs!
        [0.76, 0.67], // plump belly
        [0.68, 0.91], // mid chest
        [0.60, 1.12], // upper chest
        [0.57, 1.32], // gentle plush neck (NO skinny KeyCat neck!)
        [0.65, 1.50], // chubby cheeks widest
        [0.61, 1.69], // eye line
        [0.52, 1.87], // forehead
        [0.34, 1.948], // crown shoulder
        [0.0, BODY_H], // smooth rounded crown peak
      ].map(([r, y]) => new THREE.Vector3(r, y, 0)),
      false,
      'centripetal',
    );
    this.outlineSamplePts = bodyCurve.getSpacedPoints(360);
    const profile = bodyCurve
      .getSpacedPoints(48)
      .map((q) => new THREE.Vector2(Math.max(0, q.x), q.y));
    profile[0].x = 0;
    profile[profile.length - 1].x = 0;

    this.bodyGeo = new THREE.LatheGeometry(profile, 56);
    // Bake Z_SCALE directly into geometry so 3D head-tilt vertex rotation has ZERO ellipse distortion!
    const bPos = this.bodyGeo.attributes.position;
    for (let i = 0; i < bPos.count; i++) {
      bPos.setZ(i, bPos.getZ(i) * Z_SCALE);
    }
    this.bodyGeo.computeVertexNormals();
    this.bodyGeo.setAttribute('rest', this.bodyGeo.attributes.position.clone());

    this.bodyMesh = new THREE.Mesh(this.bodyGeo, this.furSystem.createMaterial('CAT_BODY'));
    this.bodyMesh.frustumCulled = false;
    this.catRoot.add(this.bodyMesh);
    this.petMeshes.push(this.bodyMesh);

    // 2) Subtle Grounded Side Hind Paws tucked cleanly beside lower thighs (zero white KeyCat outline!)
    const pawGeo = this.makePlushPawGeo();
    for (const s of [-1, 1] as const) {
      const hp = new THREE.Mesh(pawGeo, this.pawMat);
      hp.scale.set(0.185, 0.11, 0.165);
      hp.position.set(s * 0.56, 0.065, 0.34);
      hp.rotation.y = s * 0.24;
      this.catRoot.add(hp);
    }

    // 3) Optional 3D Collar + Swinging Gold Bell (off by default to stay 100% pure original KuroCat!)
    this.collarR = this.radiusAtY(this.collarY);
    const COLLAR_LIFT = 0.055;
    this.collarMat = withTowardEye(
      new THREE.MeshBasicMaterial({ color: '#d9483b' }),
      COLLAR_LIFT,
    ) as THREE.MeshBasicMaterial;
    const bellMat = withTowardEye(new THREE.MeshBasicMaterial({ color: '#edb938' }), COLLAR_LIFT);
    const band = createCleanLimb(this.collarGroup, {
      segments: 38,
      radial: 8,
      fill: this.collarMat,
      order: 2,
    });
    band.update(
      (t, out) => {
        const a = mix(-1.35, 1.35, t);
        out.set(
          Math.sin(a) * (this.collarR + 0.012),
          0.012 * (1 - Math.cos(a)),
          Math.cos(a) * (this.collarR + 0.012) * Z_SCALE,
        );
      },
      (t) => 0.032 * (0.28 + 0.72 * smoothstep(0, 0.22, Math.min(t, 1 - t))),
    );
    {
      const RB = 0.068;
      const ball = new THREE.Mesh(new THREE.SphereGeometry(RB, 20, 14), bellMat);
      const makeDecal = (color: string) =>
        withTowardEye(new THREE.MeshBasicMaterial({ color, depthTest: false }), COLLAR_LIFT + 0.01);
      const slit = new THREE.Mesh(new THREE.PlaneGeometry(0.068, 0.012), makeDecal('#1d1d1b'));
      slit.position.set(0, -0.020, Math.sqrt(RB * RB - 0.02 * 0.02) + 0.002);
      slit.rotation.x = -0.33;
      const glint = new THREE.Mesh(new THREE.CircleGeometry(0.014, 12), makeDecal('#fff6d8'));
      glint.position.set(-0.020, 0.022, Math.sqrt(RB * RB - 0.02 * 0.02 - 0.022 * 0.022) + 0.002);
      glint.rotation.set(-0.35, -0.35, 0);
      const loop = new THREE.Mesh(
        new THREE.TorusGeometry(0.019, 0.006, 8, 16),
        withTowardEye(new THREE.MeshBasicMaterial({ color: '#1d1d1b' }), COLLAR_LIFT),
      );
      loop.position.y = RB + 0.009;
      const hang = new THREE.Group();
      hang.position.y = -(RB + 0.016);
      hang.add(ball, slit, glint, loop);
      this.bellGroup.add(hang);
      this.bellGroup.position.set(0, -0.012, (this.collarR + 0.044) * Z_SCALE);
    }
    this.collarGroup.add(this.bellGroup);
    this.collarGroup.visible = false;
    this.catRoot.add(this.collarGroup);

    // 4) Head Mount (tracks exact head center y=HEAD_CY inside continuous body mesh!)
    this.headMount.position.set(0, HEAD_CY, 0);
    this.catRoot.add(this.headMount);

    // 5) Compact Rounded KuroCat 3D Ears (snug, slightly upright triangular cat tips matching Panel 1!)
    const earPts: Array<[number, number]> = [
      [0.23, -0.05],
      [0.21, 0.03],
      [0.18, 0.12],
      [0.135, 0.21],
      [0.082, 0.28],
      [0.036, 0.322],
      [0.012, 0.336],
      [0.0, 0.340],
    ];
    const earSplinePts = [
      new THREE.Vector2(0, -0.05),
      ...earPts.map(([r, y]) => new THREE.Vector2(r, y)),
    ];
    const earGeo = new THREE.LatheGeometry(new THREE.SplineCurve(earSplinePts).getSpacedPoints(20), 26);
    earGeo.setAttribute('rest', earGeo.attributes.position.clone());
    for (const s of [-1, 1] as const) {
      const g = new THREE.Group();
      g.position.set(s * 0.295, 0.255, -0.015);
      g.rotation.set(-0.06, 0, -s * 0.06);
      g.scale.set(1.0, 1.04, 1.0);
      const mesh = new THREE.Mesh(earGeo, this.furSystem.createMaterial('EAR', s));
      mesh.scale.set(1, 1, 0.56);
      g.add(mesh);
      this.headMount.add(g);
      this.ears.push({ group: g, mesh, s, tilt: 0.06 });
      this.petMeshes.push(mesh);
    }

    // 6) Curved Surface-Conforming KuroCat Eyes + Petting Lazy Slits + Closed Sleep Arcs + Soft Blush
    for (const s of [-1, 1] as const) {
      const lx = s * 0.232;
      const ly = 0.105; // headMount local -> world y = 1.665 (exact KuroCat eye line!)

      const eyeGeo = this.makeCurvedFacePatchGeo(lx, ly, EYE_MAX_RX, EYE_MAX_RY, 0.024, 20);
      const eyeMat = createEyeShaderMaterial(s);
      const eyeMesh = new THREE.Mesh(eyeGeo, eyeMat);
      eyeMesh.renderOrder = 10;

      // Gentle U-shaped closed arc for sleep / password privacy / blink
      const closedArc = new TaperTube3D(18, 6, this.lidArcMat, 14);
      closedArc.update(
        (t, out) => {
          const x = lx + mix(-0.132, 0.132, t);
          const y = ly - 0.042 * Math.sin(t * Math.PI);
          out.set(x, y, this.surfaceZ(x, y) + 0.028);
        },
        (t) => 0.012 + 0.005 * Math.sin(t * Math.PI),
      );
      closedArc.mesh.visible = false;

      // Original KuroCat sleek thin lazy-slanted horizontal squint slits when petting!
      const squintSlit = new TaperTube3D(18, 6, this.squintMat, 14);
      squintSlit.update(
        (t, out) => {
          const x = lx + mix(-0.132, 0.132, t);
          const y = ly - 0.012 * Math.sin(t * Math.PI) - s * (t - 0.5) * 0.016;
          out.set(x, y, this.surfaceZ(x, y) + 0.028);
        },
        (t) => 0.011 + 0.004 * Math.sin(t * Math.PI),
      );
      squintSlit.mesh.visible = false;

      const blushGeo = this.makeCurvedFacePatchGeo(lx + s * 0.06, ly - 0.155, 0.10, 0.056, 0.016, 8);
      const blush = new THREE.Mesh(blushGeo, this.blushMat);
      blush.renderOrder = 9;
      blush.visible = false;

      this.headMount.add(blush, eyeMesh, closedArc.mesh, squintSlit.mesh);
      this.eyes.push({
        s,
        mesh: eyeMesh,
        mat: eyeMat,
        closedArc: closedArc.mesh,
        squintSlit: squintSlit.mesh,
        blush,
      });
    }

    // 7) Dainty Short KuroCat Cheek Whiskers merged into 1 single static draw call (was 6 draw calls!)
    {
      const whiskerGeos: Array<{ geo: THREE.BufferGeometry }> = [];
      for (const s of [-1, 1] as const) {
        for (let i = 0; i < 3; i++) {
          const tube = new TaperTube3D(8, 4, this.whiskerMat, 12);
          const y0 = -0.085 - i * 0.048;
          const x0 = s * 0.38;
          const z0 = this.surfaceZ(x0, y0) + 0.012;
          const a = new THREE.Vector3(x0, y0, z0);
          const c = new THREE.Vector3(s * 0.48, y0 - 0.008 - i * 0.006, z0 - 0.03);
          const b = new THREE.Vector3(s * 0.58, y0 - 0.022 - i * 0.016, z0 - 0.09);
          tube.update(
            (t, out) => {
              const u = 1 - t;
              out.set(
                u * u * a.x + 2 * u * t * c.x + t * t * b.x,
                u * u * a.y + 2 * u * t * c.y + t * t * b.y,
                u * u * a.z + 2 * u * t * c.z + t * t * b.z,
              );
            },
            (t) => 0.0042 * (1 - 0.35 * t),
          );
          whiskerGeos.push({ geo: tube.mesh.geometry });
        }
      }
      const mergedWhiskers = mergeStaticGeometries(whiskerGeos);
      for (const w of whiskerGeos) w.geo.dispose();
      const whiskerMesh = new THREE.Mesh(mergedWhiskers, this.whiskerMat);
      whiskerMesh.renderOrder = 12;
      this.headMount.add(whiskerMesh);
    }

    // 8) Delicate 3D Nose & "ω" Mouth (shown on Ragdoll / Tabby / Calico / Tuxedo / Siamese; hidden on pure Black Cat!)
    {
      const NY = -0.042;
      const noseShape = new THREE.Shape();
      noseShape.moveTo(-0.040, 0.011);
      noseShape.quadraticCurveTo(0, 0.023, 0.040, 0.011);
      noseShape.quadraticCurveTo(0.044, 0.003, 0.011, -0.022);
      noseShape.quadraticCurveTo(0, -0.028, -0.011, -0.022);
      noseShape.quadraticCurveTo(-0.044, 0.003, -0.040, 0.011);
      const noseMesh = new THREE.Mesh(new THREE.ShapeGeometry(noseShape, 10), this.noseMat);
      noseMesh.position.set(0, NY, this.surfaceZ(0, NY) + 0.024);
      noseMesh.renderOrder = 12;

      const mouth = new TaperTube3D(24, 4, this.mouthMat, 12);
      mouth.update(
        (t, out) => {
          const x = mix(-0.064, 0.064, t);
          const y = NY - 0.032 - 0.020 * Math.sin((Math.abs(x) / 0.064) * Math.PI);
          out.set(x, y, this.surfaceZ(x, y) + 0.024);
        },
        () => 0.0050,
      );
      this.faceGroup.add(noseMesh, mouth.mesh);
      this.headMount.add(this.faceGroup);
    }

    // 8b) Sculpted 3D Gaming Headset (auto-equipped on PC game detection or manual toggle!)
    this.buildGamingHeadset();

    // 9) Graceful Tapered 3D KuroCat Tail (ZERO white border!)
    this.tailCurve = new THREE.CatmullRomCurve3(
      this.tailRestPts.map((q) => q.clone()),
      false,
      'catmullrom',
      0.5,
    );
    this.tailLimb = createCleanLimb(this.scene, {
      segments: 22,
      radial: 10,
      fill: this.furSystem.createMaterial('TAIL'),
      capStart: false,
    });

    // 10) Sleek Unified KuroCat Arms + Soft Sculpted Plush Paws (tuck neatly onto front belly when idle!)
    const singleToeGeo = new THREE.PlaneGeometry(0.024, 0.28);
    singleToeGeo.rotateX(-Math.PI / 2);
    const mergedToesGeo = mergeStaticGeometries([
      { geo: singleToeGeo, matrix: new THREE.Matrix4().makeTranslation(-0.28, 0.68, 0.70) },
      { geo: singleToeGeo, matrix: new THREE.Matrix4().makeTranslation(0.28, 0.68, 0.70) },
    ]);
    singleToeGeo.dispose();

    for (const s of [-1, 1] as const) {
      const limb = createCleanLimb(this.scene, {
        segments: 16,
        radial: 10,
        fill: this.furSystem.createMaterial('ARM', s),
        order: 18,
      });
      const paw = new THREE.Mesh(pawGeo, this.pawMat);
      paw.renderOrder = 20;
      const toes = new THREE.Mesh(mergedToesGeo, this.toeMat);
      toes.renderOrder = 21;
      paw.add(toes);

      const tuckArc = new THREE.Mesh();
      tuckArc.visible = false;
      this.scene.add(paw);
      this.arms.push({
        s,
        limb,
        paw,
        tuckArc,
        // Cozy mid-lower plush flank shoulder (matches approved 2D 0.65/±43 proportions — shorter, cuter arms!)
        shoulderLocal: new THREE.Vector3(s * 0.54, 0.78, 0.22),
        shoulder: new THREE.Vector3(),
        c1: new THREE.Vector3(),
        c2: new THREE.Vector3(),
        hand: new THREE.Vector3(),
      });
    }

    const mkTrack = (side: -1 | 1): PawTrack3D => {
      const pos = new THREE.Vector3(side * 0.28, 0.34, 0.62);
      return {
        side,
        pos,
        from: pos.clone(),
        last: pos.clone(),
        spring: new Spring3D(pos, 3.6, 0.78),
        keyId: null,
        t0: -100,
        dur: 0.08,
        down: false,
        lastActive: -100,
        tuckWeight: 1,
      };
    };
    this.paws = { L: mkTrack(-1), R: mkTrack(1) };

    // 11) Comic anger mark 💢 sprite
    this.angerSprite = new THREE.Sprite(
      new THREE.SpriteMaterial({ map: getFxTexture('anger'), depthTest: false, transparent: true }),
    );
    this.angerSprite.renderOrder = 45;
    this.angerSprite.visible = false;
    this.scene.add(this.angerSprite);

    this.resize();
  }

  /**
   * Builds sculpted over-ear gaming headphones on `headMount`:
   * - Arched padded headband curves cleanly behind the cat's ears (zero ear clipping!)
   * - Plush memory-foam cushions hug the chubby cheeks with brushed-titanium yokes
   * - Luminous KuroCat-lime / cyan-RGB breathing LED rings + curved boom microphone
   */
  private buildGamingHeadset(): void {
    const shellMat = createHeadsetMaterial('#242934', 0.28, 0.28, '#8da4c9');
    const padMat = createHeadsetMaterial('#384050', 0.12, 0.16, '#7f8da6');
    const metalMat = createHeadsetMaterial('#a2afc4', 0.58, 0.34, '#e6f0ff');
    const boomMat = createHeadsetMaterial('#202530', 0.20, 0.20, '#7889a6');
    this.headsetShaderMats.push(shellMat, padMat, metalMat, boomMat);

    // Collect static headset meshes by material and merge into 6 single draw calls (was 23 draw calls / 10.2k tris!)
    const shellParts: Array<{ geo: THREE.BufferGeometry; matrix?: THREE.Matrix4 }> = [];
    const padParts: Array<{ geo: THREE.BufferGeometry; matrix?: THREE.Matrix4 }> = [];
    const metalParts: Array<{ geo: THREE.BufferGeometry; matrix?: THREE.Matrix4 }> = [];
    const ledParts: Array<{ geo: THREE.BufferGeometry; matrix?: THREE.Matrix4 }> = [];
    const disposeLater: THREE.BufferGeometry[] = [];

    const makeLocalMat = (
      pos: [number, number, number],
      rot: [number, number, number],
      scl: [number, number, number],
    ) => {
      const m = new THREE.Matrix4();
      const q = new THREE.Quaternion().setFromEuler(new THREE.Euler(rot[0], rot[1], rot[2]));
      m.compose(new THREE.Vector3(...pos), q, new THREE.Vector3(...scl));
      return m;
    };

    // 1) Main outer headband arch curving smoothly behind the upright cat ears!
    const archCurve = new THREE.CatmullRomCurve3(
      [
        new THREE.Vector3(-0.675, 0.09, -0.015),
        new THREE.Vector3(-0.635, 0.33, -0.068),
        new THREE.Vector3(-0.36, 0.495, -0.106),
        new THREE.Vector3(0.0, 0.535, -0.114),
        new THREE.Vector3(0.36, 0.495, -0.106),
        new THREE.Vector3(0.635, 0.33, -0.068),
        new THREE.Vector3(0.675, 0.09, -0.015),
      ],
      false,
      'centripetal',
    );
    const bandGeo = new THREE.TubeGeometry(archCurve, 28, 0.046, 10, false);
    const bPos = bandGeo.attributes.position;
    for (let i = 0; i < bPos.count; i++) {
      const z = bPos.getZ(i);
      const y = bPos.getY(i);
      const crownW = smoothstep(0.18, 0.52, y);
      bPos.setZ(i, z - crownW * 0.012);
    }
    bandGeo.computeVertexNormals();
    shellParts.push({ geo: bandGeo });
    disposeLater.push(bandGeo);

    // Soft plush inner headband cushion under the crown
    const padPts: THREE.Vector3[] = [];
    for (let i = 0; i <= 12; i++) {
      const u = mix(0.24, 0.76, i / 12);
      const p = archCurve.getPoint(u);
      padPts.push(new THREE.Vector3(p.x * 0.96, p.y - 0.036, p.z + 0.012));
    }
    const innerPadGeo = new THREE.TubeGeometry(
      new THREE.CatmullRomCurve3(padPts, false, 'centripetal'),
      16,
      0.032,
      8,
      false,
    );
    padParts.push({ geo: innerPadGeo });
    disposeLater.push(innerPadGeo);

    // Brushed-titanium headband slider arms
    for (const s of [-1, 1] as const) {
      const sliderPts = [
        archCurve.getPoint(s < 0 ? 0.02 : 0.98),
        archCurve.getPoint(s < 0 ? 0.09 : 0.91),
        archCurve.getPoint(s < 0 ? 0.16 : 0.84),
      ];
      const sliderGeo = new THREE.TubeGeometry(
        new THREE.CatmullRomCurve3(sliderPts),
        8,
        0.052,
        8,
        false,
      );
      metalParts.push({ geo: sliderGeo });
      disposeLater.push(sliderGeo);
    }

    // 2) Sculpted Over-Ear Plush Earcups (Left -1 & Right +1)
    const cushionGeo = new THREE.TorusGeometry(0.145, 0.058, 12, 20);
    cushionGeo.rotateY(Math.PI / 2);

    const shellGeo = new THREE.SphereGeometry(0.185, 16, 12);
    const rimGeo = new THREE.TorusGeometry(0.155, 0.022, 10, 20);
    rimGeo.rotateY(Math.PI / 2);

    const ledRingGeo = new THREE.TorusGeometry(0.112, 0.015, 8, 22);
    ledRingGeo.rotateY(Math.PI / 2);

    const centerCapGeo = new THREE.CylinderGeometry(0.068, 0.078, 0.032, 14);
    centerCapGeo.rotateZ(Math.PI / 2);

    const dotGeo = new THREE.SphereGeometry(0.024, 10, 8);
    const yokeGeo = new THREE.CylinderGeometry(0.036, 0.042, 0.09, 10);
    disposeLater.push(cushionGeo, shellGeo, rimGeo, ledRingGeo, centerCapGeo, dotGeo, yokeGeo);

    for (const s of [-1, 1] as const) {
      const cupMat = makeLocalMat([s * 0.652, 0.045, 0.012], [0, s * 0.08, -s * 0.10], [1, 1, 1]);
      const pushCup = (
        arr: Array<{ geo: THREE.BufferGeometry; matrix?: THREE.Matrix4 }>,
        geo: THREE.BufferGeometry,
        pos: [number, number, number],
        rot: [number, number, number],
        scl: [number, number, number],
      ) => {
        arr.push({ geo, matrix: cupMat.clone().multiply(makeLocalMat(pos, rot, scl)) });
      };

      pushCup(padParts, cushionGeo, [0, 0, 0], [0, 0, 0], [0.95, 1.18, 0.94]);
      pushCup(shellParts, shellGeo, [s * 0.048, 0, 0], [0, 0, 0], [0.56, 1.12, 0.90]);
      pushCup(metalParts, rimGeo, [s * 0.068, 0, 0], [0, 0, 0], [0.85, 1.12, 0.91]);
      pushCup(metalParts, centerCapGeo, [s * 0.132, 0, 0], [0, 0, 0], [1.0, 1.08, 0.92]);
      pushCup(ledParts, ledRingGeo, [s * 0.138, 0, 0], [0, 0, 0], [1.0, 1.10, 0.92]);
      pushCup(ledParts, dotGeo, [s * 0.150, 0, 0], [0, 0, 0], [0.45, 1.0, 1.0]);
      pushCup(metalParts, yokeGeo, [s * 0.032, 0.195, -0.025], [0, 0, s * 0.16], [1, 1, 1]);
    }

    // 3) Sleek Curved Gaming Boom Microphone extending from the left earcup (-X)
    const micCurve = new THREE.CatmullRomCurve3([
      new THREE.Vector3(-0.64, -0.065, 0.075),
      new THREE.Vector3(-0.585, -0.148, 0.31),
      new THREE.Vector3(-0.415, -0.168, 0.495),
      new THREE.Vector3(-0.235, -0.132, 0.562),
    ]);
    const micArm = new THREE.Mesh(new THREE.TubeGeometry(micCurve, 16, 0.017, 8, false), boomMat);
    this.headsetGroup.add(micArm);

    const micJointGeo = new THREE.SphereGeometry(0.032, 10, 8);
    metalParts.push({
      geo: micJointGeo,
      matrix: makeLocalMat([-0.645, -0.062, 0.072], [0, 0, 0], [1, 1, 1]),
    });
    disposeLater.push(micJointGeo);

    const micTipGeo = new THREE.SphereGeometry(0.044, 12, 10);
    padParts.push({
      geo: micTipGeo,
      matrix: makeLocalMat([-0.222, -0.128, 0.568], [0, -0.35, 0.22], [1.32, 0.88, 0.88]),
    });
    disposeLater.push(micTipGeo);

    this.headsetGroup.add(
      new THREE.Mesh(mergeStaticGeometries(shellParts), shellMat),
      new THREE.Mesh(mergeStaticGeometries(padParts), padMat),
      new THREE.Mesh(mergeStaticGeometries(metalParts), metalMat),
      new THREE.Mesh(mergeStaticGeometries(ledParts), this.headsetLedMat),
    );
    for (const g of disposeLater) g.dispose();

    const micLed = new THREE.Mesh(new THREE.SphereGeometry(0.014, 8, 6), this.headsetMicLedMat);
    micLed.position.set(-0.172, -0.118, 0.588);
    this.headsetGroup.add(micLed);

    this.headsetGroup.visible = false;
    this.headMount.add(this.headsetGroup);
  }

  private makePlushPawGeo(): THREE.SphereGeometry {
    const g = new THREE.SphereGeometry(1, 18, 12);
    const q = g.attributes.position;
    for (let i = 0; i < q.count; i++) {
      const x = q.getX(i);
      const y = q.getY(i);
      const z = q.getZ(i);
      q.setXYZ(i, x * (1 + 0.10 * z), y < 0 ? y * 0.48 : y, z);
    }
    g.computeVertexNormals();
    return g;
  }

  /** Exact piecewise-linear radius of our continuous KuroCat Lathe profile at body-local height y ∈ [0, BODY_H]. */
  private radiusAtY(y: number): number {
    const cy = clamp(y, 0, BODY_H);
    const pts = this.outlineSamplePts;
    for (let i = 0; i < pts.length - 1; i++) {
      const a = pts[i];
      const b = pts[i + 1];
      if ((cy >= a.y && cy <= b.y) || (cy <= a.y && cy >= b.y)) {
        const span = b.y - a.y;
        const t = Math.abs(span) > 1e-6 ? clamp((cy - a.y) / span, 0, 1) : 0;
        return mix(a.x, b.x, t);
      }
    }
    return pts[pts.length - 1].x;
  }

  /** Mathematically exact Z depth on the front face of our Lathe body at headMount-local (x, y). */
  private surfaceZ(x: number, y: number): number {
    const ly = HEAD_CY + y;
    const R = Math.max(0.05, this.radiusAtY(ly));
    const ux = clamp(x / R, -0.985, 0.985);
    return R * Z_SCALE * Math.sqrt(Math.max(0, 1 - ux * ux));
  }

  private makeCurvedFacePatchGeo(
    cx: number,
    cy: number,
    rx: number,
    ry: number,
    zOffset: number,
    segs = 16,
  ): THREE.PlaneGeometry {
    const geo = new THREE.PlaneGeometry(2, 2, segs, segs);
    const pos = geo.attributes.position;
    for (let i = 0; i < pos.count; i++) {
      const u = pos.getX(i);
      const v = pos.getY(i);
      const x = cx + u * rx;
      const y = cy + v * ry;
      pos.setXYZ(i, x, y, this.surfaceZ(x, y) + zOffset);
    }
    geo.computeVertexNormals();
    geo.computeBoundingSphere();
    return geo;
  }

  private spineWeight(y: number): number {
    return smoothstep(0.14, 1.75, y) ** 1.45;
  }

  private sinkWeight(y: number): number {
    return smoothstep(0.18, BODY_H, y);
  }

  private attachLocal(local: THREE.Vector3, out: THREE.Vector3): THREE.Vector3 {
    const w = this.spineWeight(local.y);
    const swell =
      1 +
      this.bend.bellySwell * smoothstep(1.30, 0.65, local.y) * smoothstep(0.0, 0.25, local.y);
    return out
      .set(
        local.x * swell + this.bend.x * w,
        local.y * (1 - this.bend.squash * this.sinkWeight(local.y)),
        local.z * swell + this.bend.z * w,
      )
      .applyMatrix4(this.catRoot.matrixWorld);
  }

  /**
   * Pushes spine bend + rigid head rotation to the GPU CAT_BODY vertex shader (zero CPU loop,
   * zero computeVertexNormals, zero per-frame PCIe vertex upload!).
   */
  private deformCatBody(): void {
    this.furSystem.setBodyDeform(
      this.bend.x,
      this.bend.z,
      this.bend.squash,
      this.bend.bellySwell,
      this.headRot.pitch,
      this.headRot.yaw,
      this.headRot.roll,
    );
  }

  private placeHeadAndCollar(): void {
    const wHead = this.spineWeight(HEAD_CY);
    const sHead = this.sinkWeight(HEAD_CY);
    this.headMount.position.set(
      this.bend.x * wHead,
      HEAD_CY * (1 - this.bend.squash * sHead),
      this.bend.z * wHead,
    );
    this.headMount.rotation.set(this.headRot.pitch, this.headRot.yaw, this.headRot.roll, 'YXZ');

    if (!this.collarGroup.visible) return;
    const y = this.collarY - 0.18 * clamp(this.headRot.pitch + 0.06, 0, 0.45);
    const s = this.radiusAtY(y) / Math.max(0.05, this.collarR);
    this.collarGroup.scale.set(s, 1, s);
    const w = this.spineWeight(y);
    const dw = (this.spineWeight(y + 0.01) - this.spineWeight(y - 0.01)) / 0.02;
    this.collarGroup.position.set(
      this.bend.x * w,
      y * (1 - this.bend.squash * this.sinkWeight(y)),
      this.bend.z * w,
    );
    const tx = clamp(Math.atan(this.bend.z * dw) * 0.45, -0.16, 0.16);
    const tz = clamp(-Math.atan(this.bend.x * dw) * 0.45, -0.16, 0.16);
    this.collarGroup.rotation.set(tx, 0, tz);
    this.bellGroup.rotation.set(-tx, 0, -tz);
  }

  // ---- Public API ----

  public resize(): void {
    const parent = this.canvas.parentElement;
    const w = Math.max(1, parent ? parent.clientWidth : this.canvas.clientWidth || 640);
    const h = Math.max(1, parent ? parent.clientHeight : this.canvas.clientHeight || 420);
    this.renderer.setSize(w, h, false);
    this.camera.aspect = w / h;
    this.camera.updateProjectionMatrix();
    this.lastCamKey = '';
  }

  public start(): void {
    if (this.rafId !== null || this.idleTimer !== null) return;
    this.lastFrameMs = performance.now();
    this.directRafPrevMs = 0;
    this.isIdlePacing = false;
    this.rafId = requestAnimationFrame(this.renderFrame);
  }

  public stop(): void {
    if (this.rafId !== null) {
      cancelAnimationFrame(this.rafId);
      this.rafId = null;
    }
    if (this.idleTimer !== null) {
      window.clearTimeout(this.idleTimer);
      this.idleTimer = null;
    }
    this.isIdlePacing = false;
  }

  /** 卸载：停止渲染循环并释放 WebGL 资源（PulseDeck 切回 2D/经典模式时调用） */
  public dispose(): void {
    this.stop();
    try {
      this.renderer.dispose();
    } catch {
      /* ignore */
    }
    try {
      this.renderer.forceContextLoss();
    } catch {
      /* ignore */
    }
  }

  /** 调试/回归用：悬停与眯眼状态（不参与渲染） */
  public debugState(): { hover: boolean; squint: number; petting: boolean } {
    return { hover: this.hovering, squint: this.squintAmt, petting: this.petting };
  }

  public setGameActive(active: boolean): void {
    this.gameActive = Boolean(active);
  }

  private readonly renderFrame = (nowMs: number): void => {
    const frameId = this.rafId;
    const workStart = performance.now();

    // Auto-detect native display VSync period during the first few direct rAF frames
    if (this.directRafPrevMs > 0 && this.vsyncSamples < 8) {
      const gap = nowMs - this.directRafPrevMs;
      if (gap >= 3.2 && gap <= 25) {
        this.vsyncMs = this.vsyncSamples === 0 ? gap : Math.min(this.vsyncMs, gap);
        this.vsyncSamples += 1;
      }
    }

    const dt = Math.min(0.05, Math.max(0.001, (nowMs - this.lastFrameMs) / 1000));
    this.lastFrameMs = nowMs;
    this.update(dt);
    this.renderer.render(this.scene, this.camera);

    if (this.rafId !== frameId) return;

    const headsetSettling =
      Math.abs(this.sHeadset.v) > 0.02 ||
      Math.abs(this.sHeadset.x - (this.headsetOn ? 1 : 0)) > 0.01;

    // Sleep particles continue drawing at the idle rate.
    const busy =
      this.elapsed - this.lastInput < 3.0 ||
      this.elapsed - this.cursorActiveAt < 2.4 ||
      this.isCross ||
      this.petting ||
      this.hovering ||
      this.elapsed < this.curiousUntil ||
      this.popups.length > 0 ||
      this.fxList.some((fx) => fx.kind !== 'z') ||
      (this.sleepAmt > 0.02 && this.sleepAmt < 0.98) ||
      headsetSettling ||
      Math.abs(this.sOrbitAz.v) > 0.05 ||
      Math.abs(this.sOrbitEl.v) > 0.05;

    // Smart adaptive frame pacing:
    // - Calibrate VSync over first 8 active frames
    // - On 60Hz monitors in normal mode: native direct rAF (0 timer jitter)
    // - On 120-240Hz monitors in normal mode: cap cleanly at 60 FPS (cuts 240Hz GPU/CPU by 4x!)
    // - In Game Eco Mode (foreground PC game detected): cap at 30 FPS (cuts 240Hz GPU/CPU by 8x!)
    // - Idle awake: ~18 FPS (55ms); Deep sleep: ~12 FPS (85ms)
    if (busy && (this.vsyncSamples < 8 || (!this.gameActive && this.vsyncMs >= 13.5))) {
      this.isIdlePacing = false;
      this.directRafPrevMs = nowMs;
      this.rafId = requestAnimationFrame(this.renderFrame);
      return;
    }

    this.directRafPrevMs = 0;
    const workMs = performance.now() - workStart;
    const targetMs = busy
      ? this.gameActive
        ? 33.33
        : 16.67
      : this.sleepAmt > 0.95
        ? 85
        : 55;
    const sleepMs = Math.max(4, Math.round(targetMs - workMs - this.vsyncMs * 0.35));

    this.rafId = null;
    this.isIdlePacing = !busy;
    this.idleTimer = window.setTimeout(() => {
      this.idleTimer = null;
      this.isIdlePacing = false;
      this.rafId = requestAnimationFrame(this.renderFrame);
    }, sleepMs);
  };

  public setCatSkin(skin: CatSkinConfig): void {
    this.currentSkin = skin;
    const p = resolveFurPalette3D(skin, this.renderStyle);
    this.currentPalette = p;
    this.furSystem.applyPalette(p, skin.spots || []);
    this.desk.setAccentColors(p.accent, p.accentRight);
    this.desk.setStyle(this.renderStyle);

    this.toeMat.color.set(p.toe);
    this.whiskerMat.color.set(p.whisker);
    this.whiskerMat.opacity = p.whiskerOpacity;
    this.lidArcMat.color.set(p.lid);
    this.squintMat.color.set(p.squintLine);
    this.tuckLineMat.color.set(p.isDarkFur ? '#b2b8c4' : '#7b6e64');
    this.tuckLineMat.opacity = p.isDarkFur ? 0.36 : 0.24;

    this.faceGroup.visible = Boolean(p.nose);
    if (p.nose) this.noseMat.color.set(p.nose);
    if (p.mouth) this.mouthMat.color.set(p.mouth);

    const lashCol = p.isDarkFur ? '#0b0c10' : p.lid;
    for (const e of this.eyes) {
      (e.mat.uniforms.uEyeTop.value as THREE.Color).set(p.eyeTop);
      (e.mat.uniforms.uEyeBottom.value as THREE.Color).set(p.eyeBottom);
      (e.mat.uniforms.uPupilCol.value as THREE.Color).set(p.pupil);
      (e.mat.uniforms.uLashCol.value as THREE.Color).set(lashCol);
      e.mat.uniforms.uIs3D.value = this.renderStyle === '3d' ? 1.0 : 0.0;
      e.mat.uniforms.uLimbalAmt.value = p.isDarkFur ? 0.22 : 0.56;
    }
  }

  public setRenderStyle(style: RenderStyle3D): void {
    this.renderStyle = style;
    const is3d = style === '3d' ? 1 : 0;
    for (const m of this.headsetShaderMats) {
      m.uniforms.uIs3D.value = is3d;
    }
    if (this.currentSkin) {
      this.setCatSkin(this.currentSkin);
    }
  }

  public setCollar(id: CollarColorId): void {
    this.collarId = id;
    if (id === 'none') {
      this.collarGroup.visible = false;
    } else {
      this.collarGroup.visible = true;
      this.collarMat.color.set(COLLAR_COLORS[id]);
      this.placeHeadAndCollar();
    }
  }

  public setHeadphones(active: boolean): void {
    const next = Boolean(active);
    if (next !== this.headsetOn) {
      this.headsetOn = next;
      if (next) {
        this.sHeadset.kick(1.8);
      }
      this.wakeRendering();
    }
  }

  public setKeyboardConfig(layout: KeyboardLayoutId, orient: KeyboardOrientation): void {
    this.desk.rebuildBoard(layout, orient);
    this.paws.L.keyId = null;
    this.paws.R.keyId = null;
    this.tailTap.keyId = null;
  }

  public setTailKeyConfig(mode: TailKeyMode, keyIds: readonly string[]): void {
    this.tailKeyMode =
      mode === 'custom' || mode === 'edge' || mode === 'off' ? mode : 'smart';
    this.tailKeyIdSet = new Set(
      (Array.isArray(keyIds) ? keyIds : []).map((id) => String(id).trim().toLowerCase()).filter(Boolean),
    );
    if (this.tailKeyMode === 'off') {
      this.tailTap.down = false;
      this.tailTap.keyId = null;
    }
  }

  public setRenderScale(scale: number): void {
    this.renderScale = clamp(scale, 0.2, 2.0);
  }

  public onPrivacyTyping(): void {
    if (this.privacyMode) {
      this.sSquash.kick(0.22);
      this.wakeRendering();
    }
  }

  public getSleepStatus(): { asleep: boolean; sleepAmt: number; idleSec: number; sleepAfterSec: number } {
    return {
      asleep: this.asleep,
      sleepAmt: this.sleepAmt,
      idleSec: Math.max(0, this.elapsed - this.lastInput),
      sleepAfterSec: this.embedded ? 18 : 28,
    };
  }

  public setPrivacyMode(active: boolean): void {
    this.privacyMode = active;
    if (active) {
      this.desk.releaseAllKeys();
      this.paws.L.down = false;
      this.paws.R.down = false;
      this.tailTap.down = false;
      this.isCross = false;
      this.curiousUntil = -100;
    }
  }

  public addOrbitDelta(dAzDeg: number, dElDeg: number): void {
    this.orbitAzimuthTarget = clamp(this.orbitAzimuthTarget + dAzDeg, -165, 165);
    this.orbitElevTarget = clamp(this.orbitElevTarget + dElDeg, 6, 58);
    this.wakeRendering();
  }

  public resetOrbit(): void {
    this.orbitAzimuthTarget = 0;
    this.orbitElevTarget = 24.5;
    this.wakeRendering();
  }

  public triggerCuriousDemo(durSec = 3.6): void {
    this.wake();
    this.isCross = false;
    this.annoy = 0;
    this.curiousUntil = this.elapsed + durSec;
    this.sCurious.kick(2.4);
    this.sEars.kick(2.2);
  }

  public triggerCrossDemo(): void {
    this.wake();
    this.curiousUntil = -100;
    this.annoy = 0.96;
    this.lastPoke = this.elapsed;
    this.isCross = true;
    this.crossSince = this.elapsed;
    this.crossHuffDir = this.cursorNdc.x >= 0 ? -1 : 1;
    this.thumpPhase = 0;
  }

  public triggerSleepDemo(): void {
    this.isCross = false;
    this.annoy = 0;
    this.curiousUntil = -100;
    this.idleAct = null;
    this.asleep = true;
    this.lastInput = this.elapsed - 90;
    this.nextZ = this.elapsed + 0.45;
  }

  public triggerTailPatDemo(): void {
    this.wake();
    this.isCross = false;
    const targetId = this.desk.getKeyById('enter') ? 'enter' : 'space';
    this.triggerTailKey(targetId, targetId === 'enter' ? 'Enter' : 'Space');
    window.setTimeout(() => {
      if (this.tailTap.keyId === targetId) this.tailTap.down = false;
    }, 180);
  }

  public onKeyDown(vk: number, repeat = false): void {
    if (this.privacyMode) {
      if (!repeat) {
        this.sSquash.kick(0.25);
        this.wakeRendering();
      }
      return;
    }
    const k = this.desk.resolveKey(vk);
    if (!k) return;
    this.wake();
    if (repeat) return;

    this.lastKey = this.elapsed;
    this.strokes.push(this.elapsed);
    const burst = this.strokes.filter((s) => this.elapsed - s < 0.18).length;
    if (burst >= 4) {
      this.pokeAnnoy(0.32);
    }

    const useTail =
      this.tailKeyMode === 'custom'
        ? this.tailKeyIdSet.has(k.spec.id)
        : !this.isCross &&
          this.tailKeyMode !== 'off' &&
          (this.desk.isRightEdgeTailKey(k.spec.id) ||
            (this.tailKeyMode === 'smart' && (k.spec.id === 'space' || k.spec.id === 'enter')));
    if (useTail) {
      this.triggerTailKey(k.spec.id, k.spec.bubble || k.spec.label || k.spec.id);
      return;
    }

    const mousingNow = this.isMousing();
    const isLeftSideScreen = k.x < 0.05;
    const pawKey: 'L' | 'R' = mousingNow ? 'R' : isLeftSideScreen ? 'L' : 'R';
    const paw = this.paws[pawKey];
    paw.from.copy(paw.pos);
    paw.keyId = k.spec.id;
    paw.t0 = this.elapsed;
    paw.lastActive = this.elapsed;
    const keyWorld = this.desk.keyTopWorld(k.spec.id, this.tmpV1);
    paw.dur = clamp(paw.pos.distanceTo(keyWorld) * 0.085, 0.048, 0.095);
    paw.down = true;
    this.desk.setKeyDown(k.spec.id, true);

    this.sSquash.kick(0.32);
    this.sPitch.kick(0.75);
    if (this.showKeyBubbles) {
      this.spawnKeyPopup(k.spec.id, k.spec.bubble || k.spec.label || k.spec.id, paw.side);
    }
  }

  public onKeyUp(vk: number): void {
    const k = this.desk.resolveKey(vk);
    if (!k) return;
    if (this.tailTap.keyId === k.spec.id) {
      this.tailTap.down = false;
    }
    this.desk.setKeyDown(k.spec.id, false);
    if (this.paws.L.keyId === k.spec.id) this.paws.L.down = false;
    if (this.paws.R.keyId === k.spec.id) this.paws.R.down = false;
    for (const p of this.popups) {
      if (p.keyId === k.spec.id && p.upAt === null) p.upAt = this.elapsed;
    }
  }

  public onGlobalMouseMove(nx: number, ny: number): void {
    const moveDelta = Math.hypot(nx - this.lastMouseNX, ny - this.lastMouseNY);
    // Ignore 0px synthetic repeats & tiny optical-sensor desk vibration so sleep countdown never falsely resets!
    const jitterThreshold = this.asleep || this.sleepAmt > 0.35 ? 0.006 : 0.0028;
    if (moveDelta < jitterThreshold) {
      return;
    }

    this.lastMouseNX = nx;
    this.lastMouseNY = ny;

    const typingNow = this.elapsed - this.lastKey < 0.5 || this.paws.L.down || this.paws.R.down;
    if (typingNow && moveDelta < 0.015) {
      return;
    }

    this.wake();
    const gap = this.elapsed - this.lastSampleT;
    if (this.lastSampleX !== null && this.lastSampleY !== null && gap > 0 && gap <= 0.14) {
      const dist = Math.hypot(nx - this.lastSampleX, ny - this.lastSampleY);
      if (dist > 0.0015 && dist < 0.45) {
        this.pointerSamples.push({ t: this.elapsed, dist });
      }
    }
    this.lastSampleX = nx;
    this.lastSampleY = ny;
    this.lastSampleT = this.elapsed;

    this.desk.setMouseNormalized(clamp(nx, 0, 1), clamp(ny, 0, 1));
    this.lastMouse = this.elapsed;
    this.paws.L.lastActive = this.elapsed;
  }

  public onWindowCursor(relX: number, relY: number, overCat: boolean): void {
    const delta = Math.hypot(relX - this.lastCursorRelX, relY - this.lastCursorRelY);
    this.lastCursorRelX = relX;
    this.lastCursorRelY = relY;
    if (overCat !== this.hovering) {
      this.hovering = overCat;
      this.wakeRendering();
    }
    if (delta > 0.0015) {
      this.cursorNdc.set(clamp(relX * 2 - 1, -1.6, 1.6), clamp(1 - relY * 2, -1.4, 1.4));
      this.cursorActiveAt = this.elapsed;
      this.wakeRendering();
    }
    if (overCat && !this.leftMouseDown && !this.rightMouseDown && delta > 0.006) {
      this.petting = true;
      this.petUntil = this.elapsed + 0.36;
      this.petX = clamp(this.cursorNdc.x * 2.0, -1, 1);
      this.wake();
    }
  }

  public onMouseDown(btn: string): void {
    this.wake();
    this.lastMouse = this.elapsed;
    this.paws.L.lastActive = this.elapsed;
    if (btn === 'left') this.leftMouseDown = true;
    if (btn === 'right') this.rightMouseDown = true;
    if (btn === 'middle') this.desk.pulseScrollWheel();
    this.sSquash.kick(0.24);
  }

  public onMouseUp(btn: string): void {
    if (btn === 'left') this.leftMouseDown = false;
    if (btn === 'right') this.rightMouseDown = false;
  }

  public onMouseWheel(): void {
    this.wake();
    this.lastMouse = this.elapsed;
    this.paws.L.lastActive = this.elapsed;
    this.desk.pulseScrollWheel();
  }

  public onPointerMove(
    clientX: number,
    clientY: number,
    insideCanvas: boolean,
    allowPetting: boolean,
  ): boolean {
    const rect = this.canvas.getBoundingClientRect();
    if (rect.width < 4 || rect.height < 4) return false;

    const nx = (clientX - rect.left) / rect.width;
    const ny = (clientY - rect.top) / rect.height;
    this.cursorNdc.set(nx * 2 - 1, -(ny * 2 - 1));
    this.cursorActiveAt = this.elapsed;
    this.wakeRendering();

    const sx = clientX / 1440;
    const sy = clientY / 1440;
    const gap = this.elapsed - this.lastSampleT;
    if (this.lastSampleX !== null && this.lastSampleY !== null && gap > 0 && gap <= 0.14) {
      const dist = Math.hypot(sx - this.lastSampleX, sy - this.lastSampleY);
      if (dist > 0.0015 && dist < 0.45) {
        this.pointerSamples.push({ t: this.elapsed, dist });
      }
    }
    this.lastSampleX = sx;
    this.lastSampleY = sy;
    this.lastSampleT = this.elapsed;

    if (insideCanvas) {
      this.desk.setMouseNormalized(clamp(nx, 0, 1), clamp(ny, 0, 1));
      if (this.elapsed - this.lastKey > 0.45) {
        this.lastMouse = this.elapsed;
        this.paws.L.lastActive = this.elapsed;
      }
    }

    let overHead = false;
    if (insideCanvas && allowPetting && !this.leftMouseDown && !this.rightMouseDown) {
      this.raycaster.setFromCamera(this.cursorNdc, this.camera);
      const hits = this.raycaster.intersectObjects(this.petMeshes, false);
      overHead = hits.length > 0 && hits[0].point.y >= 1.15;
      if (overHead) {
        this.petting = true;
        this.petUntil = this.elapsed + 0.30;
        this.petX = clamp(this.cursorNdc.x * 2.5, -1, 1);
        this.wake();
      }
    }
    return overHead;
  }

  public getStateSummary(): {
    mood: string;
    orbitAz: number;
    orbitEl: number;
    collar: CollarColorId;
    style: RenderStyle3D;
  } {
    const mood = this.privacyMode
      ? '隐私防偷窥'
      : this.sleepAmt > 0.5
        ? '打瞌睡 Zzz'
        : this.isCross
          ? '生气飞机耳 💢'
          : this.elapsed < this.curiousUntil
            ? '好奇大圆瞳 ✨'
            : this.petting
              ? '被摸头呼噜中 💛'
              : '原版黑猫 3D 态';
    return {
      mood,
      orbitAz: Math.round(this.orbitAzimuthTarget),
      orbitEl: Math.round(this.orbitElevTarget),
      collar: this.collarId,
      style: this.renderStyle,
    };
  }

  // ---- Internal helpers ----

  private wakeRendering(): void {
    // Only interrupt idle/sleep timers so active 60 FPS / 30 FPS pacing is never defeated by high-Hz mouse storms!
    if (this.idleTimer === null || !this.isIdlePacing) return;
    window.clearTimeout(this.idleTimer);
    this.idleTimer = null;
    this.isIdlePacing = false;
    this.rafId = requestAnimationFrame(this.renderFrame);
  }

  private wake(): void {
    this.lastInput = this.elapsed;
    this.wakeRendering();
    if (this.asleep) {
      this.asleep = false;
      this.wakeAt = this.elapsed;
      this.sSquash.kick(-1.1);
    }
  }

  private isMousing(): boolean {
    return (
      !this.privacyMode &&
      !this.asleep &&
      (this.leftMouseDown || this.rightMouseDown || this.elapsed - this.lastMouse < 1.35)
    );
  }

  private pokeAnnoy(amount: number): void {
    this.annoy = Math.min(1, this.annoy + amount);
    this.lastPoke = this.elapsed;
  }

  private triggerTailKey(keyId: string, bubbleLabel: string): void {
    this.tailTap.keyId = keyId;
    this.tailTap.from = this.tailTap.reach;
    this.tailTap.t0 = this.elapsed;
    this.tailTap.dur = 0.135;
    this.tailTap.down = true;
    this.tailTap.landAt = this.elapsed;
    this.desk.setKeyDown(keyId, true);
    this.sSquash.kick(0.24);
    if (this.showKeyBubbles) {
      this.spawnKeyPopup(keyId, bubbleLabel, 1);
    }
    window.setTimeout(() => {
      if (!this.tailTap.down) this.desk.setKeyDown(keyId, false);
    }, 170);
  }

  private spawnKeyPopup(keyId: string, label: string, side: -1 | 1): void {
    const clean = label.length > 6 ? label.slice(0, 6) : label;
    const { tex, aspect } = getPopupTexture(clean);
    const mat = new THREE.SpriteMaterial({ map: tex, transparent: true, depthTest: false });
    const sprite = new THREE.Sprite(mat);
    const baseH = 0.25;
    const baseW = baseH * aspect;
    sprite.scale.set(baseW, baseH, 1);
    sprite.renderOrder = 40;
    const kp = this.desk.keyTopWorld(keyId, this.tmpV2);
    sprite.position.set(kp.x, kp.y + 0.14, kp.z);
    this.scene.add(sprite);
    this.popups.push({
      keyId,
      sprite,
      baseW,
      baseH,
      t0: this.elapsed,
      upAt: null,
      side,
    });
    if (this.popups.length > 5) {
      const old = this.popups.shift()!;
      this.scene.remove(old.sprite);
      old.sprite.material.dispose();
    }
  }

  private spawnFx(kind: 'bang' | 'dust' | 'z', pos: THREE.Vector3, vel: THREE.Vector3, size: number, dur: number): void {
    const mat = new THREE.SpriteMaterial({
      map: getFxTexture(kind),
      transparent: true,
      depthTest: false,
    });
    const sprite = new THREE.Sprite(mat);
    sprite.position.copy(pos);
    sprite.scale.set(size, size, 1);
    sprite.renderOrder = 44;
    this.scene.add(sprite);
    this.fxList.push({ sprite, kind, vel, t0: this.elapsed, dur, size, baseX: pos.x });
  }

  private spawnSlamFx(): void {
    this.lastSlam = this.elapsed;
    this.sDeskHop.kick(0.70);
    this.sSquash.kick(0.32);
    const impact = this.tailCurve.points[this.tailCurve.points.length - 1];
    this.spawnFx('bang', new THREE.Vector3(impact.x, impact.y + 0.08, impact.z + 0.04), new THREE.Vector3(0, 0.08, 0), 0.34, 0.22);
    for (const side of [-1, 1] as const) {
      this.spawnFx(
        'dust',
        new THREE.Vector3(impact.x + side * 0.07, impact.y + 0.05, impact.z + 0.06),
        new THREE.Vector3(side * 0.45, 0.36, 0.16),
        0.13,
        0.38,
      );
    }
  }

  private update(dt: number): void {
    this.elapsed += dt;
    const t = this.elapsed;

    // 1) Camera smooth 360° turntable framing (skip redundant lookAt matrix rebuild when static)
    const az = (this.sOrbitAz.step(this.orbitAzimuthTarget, dt) * Math.PI) / 180;
    const el = (this.sOrbitEl.step(this.orbitElevTarget, dt) * Math.PI) / 180;
    const is108 = this.desk.getLayoutId() === '108';
    const camKey = `${az.toFixed(4)}|${el.toFixed(4)}|${is108 ? 1 : 0}|${this.camera.aspect.toFixed(4)}|${this.renderScale.toFixed(3)}`;
    if (camKey !== this.lastCamKey) {
      this.lastCamKey = camKey;
      const baseDist = is108 ? 9.7 : 8.1;
      const reqHalfW = is108 ? 1.98 : 1.56;
      const tanHalfFov = Math.tan((this.camera.fov * Math.PI) / 360);
      const aspectFitDist = reqHalfW / (tanHalfFov * Math.max(0.55, this.camera.aspect));
      const dist = Math.max(baseDist, aspectFitDist) / Math.max(0.25, this.renderScale);
      this.camera.position.set(
        this.camAnchor.x + Math.sin(az) * Math.cos(el) * dist,
        this.camAnchor.y + Math.sin(el) * dist,
        this.camAnchor.z + Math.cos(az) * Math.cos(el) * dist,
      );
      this.camera.lookAt(this.camAnchor);
    }

    // 2) Emotional state machine
    this.strokes = this.strokes.filter((s) => t - s < 1.0);
    const cps = this.strokes.length;
    this.excite += (Math.min(1, cps / 7.5) - this.excite) * approach(7.5, dt);

    const sleepAfterSec = this.embedded ? 18 : 28;
    const idleSec = t - this.lastInput;
    if (!this.asleep && !this.privacyMode && !this.petting && !this.hovering && idleSec > sleepAfterSec) {
      this.asleep = true;
      this.isCross = false;
      this.idleAct = null;
      this.nextZ = t + 0.9;
    }
    const wantSleep = this.asleep && !this.privacyMode ? 1 : 0;
    this.sleepAmt += (wantSleep - this.sleepAmt) * approach(wantSleep ? 1.5 : 6.5, dt);
    if (this.sleepAmt < 0.002 && !wantSleep) this.sleepAmt = 0;

    this.privacyClose += ((this.privacyMode ? 1 : 0) - this.privacyClose) * approach(14, dt);

    if (this.petting && t > this.petUntil) {
      this.petting = false;
    }
    const petActive = (this.petting || this.hovering) && !this.privacyMode && !this.isCross;
    this.squintAmt += ((petActive ? 1 : 0) - this.squintAmt) * approach(12, dt);

    // Curious trigger from swift cursor sweep
    this.pointerSamples = this.pointerSamples.filter((s) => t - s.t <= 0.22);
    const pointerSpeed = this.pointerSamples.reduce((a, s) => a + s.dist, 0) / 0.22;
    const canCurious = !this.asleep && !this.privacyMode && !this.isCross && t - this.lastKey > 0.9;
    if (canCurious && this.pointerSamples.length >= 4 && pointerSpeed > 0.92) {
      this.curiousUntil = Math.max(this.curiousUntil, t + 2.2);
    }
    const curiousNow = !this.asleep && !this.privacyMode && !this.isCross && t < this.curiousUntil;
    const curious = clamp(this.sCurious.step(curiousNow ? 1 : 0, dt), 0, 1);

    // Grumpy annoyance state
    if (!this.privacyMode && !this.asleep) {
      if (cps > 8.5) {
        this.pokeAnnoy((cps - 8.5) * 0.055 * dt);
      } else if (this.isCross && t - this.lastKey < 0.9 && cps >= 4) {
        this.lastPoke = t;
      }
      if (t - this.lastPoke > 2.2) {
        this.annoy = Math.max(0, this.annoy - 0.15 * dt);
      }
      if (!this.isCross && this.annoy >= 0.62) {
        this.isCross = true;
        this.crossSince = t;
        this.crossHuffDir = this.cursorNdc.x >= 0 ? -1 : 1;
        this.thumpPhase = 0;
        this.curiousUntil = -100;
        this.idleAct = null;
      } else if (this.isCross && this.annoy < 0.26 && t - this.crossSince > 3.2) {
        this.isCross = false;
      }
    } else {
      this.isCross = false;
      this.annoy = Math.max(0, this.annoy - 0.35 * dt);
    }
    const cross = clamp(this.sCross.step(this.isCross ? 1 : 0, dt), 0, 1);

    // Calm idle fidgets
    if (this.idleAct && t > this.idleAct.t0 + this.idleAct.dur) this.idleAct = null;
    if (
      !this.idleAct &&
      t >= this.nextIdleAct &&
      idleSec > 2.6 &&
      !this.asleep &&
      !this.privacyMode &&
      !petActive &&
      !this.isCross &&
      !curiousNow
    ) {
      const kinds: IdleAct3D['kind'][] = ['tilt', 'tilt', 'ear', 'slowBlink', 'look', 'tail'];
      const kind = kinds[Math.floor(Math.random() * kinds.length)];
      const durMap: Record<IdleAct3D['kind'], number> = {
        tilt: 1.9,
        ear: 0.46,
        slowBlink: 1.45,
        look: 2.1,
        tail: 1.1,
      };
      this.idleAct = { kind, t0: t, dur: durMap[kind], side: Math.random() < 0.5 ? -1 : 1 };
      this.nextIdleAct = t + 3.8 + Math.random() * 4.5;
    }
    const act = this.idleAct;
    const actU = act ? clamp((t - act.t0) / act.dur, 0, 1) : 0;
    const actEnv = act ? smoothstep(0, 0.25, actU) * (1 - smoothstep(0.7, 1, actU)) : 0;

    // 3) Keyboard & cordless mouse update
    const deskHopY = this.sDeskHop.step(0, dt);
    this.desk.update(dt, this.leftMouseDown, this.rightMouseDown, deskHopY, this.privacyClose);

    // 4) Spine lean, hunch, squash & breathing on continuous one-piece body
    const mousing = this.isMousing();
    const typingActive = t - this.lastKey < 0.65 || this.paws.L.down || this.paws.R.down;
    const striking =
      this.paws.L.down && (!this.paws.R.down || this.paws.L.t0 >= this.paws.R.t0)
        ? this.paws.L
        : this.paws.R.down
          ? this.paws.R
          : null;

    let targetShift = 0;
    if (mousing) targetShift = -0.24;
    else if (striking && striking.keyId) {
      const kt = this.desk.keyTopWorld(striking.keyId, this.tmpV1);
      targetShift = clamp((kt.x - 0.14) * 0.15, -0.22, 0.22);
    } else if (this.tailTap.reach > 0.1) {
      targetShift = 0.08 * this.tailTap.reach;
    }

    const targetHunch = typingActive
      ? 0.09 + 0.08 * this.excite
      : curious * 0.07 + (petActive ? -0.05 : 0);
    const targetSquash =
      this.sleepAmt * 0.20 +
      (petActive ? 0.06 : 0) -
      curious * 0.04 +
      Math.sin(t * 2.0) * 0.014;

    this.bend.x = this.sShift.step(targetShift, dt);
    this.bend.z = this.sHunch.step(targetHunch, dt);
    this.bend.squash = this.sSquash.step(targetSquash, dt);
    this.bend.bellySwell = this.sleepAmt * 0.095 + (typingActive ? 0.035 : 0);

    // 5) Head pose & silky eye tracking
    const gx = Math.tanh(this.cursorNdc.x * 1.45);
    const gy = Math.tanh(this.cursorNdc.y * 1.35);
    const lookCursor = t - this.cursorActiveAt < 2.5;

    let targetYaw = 0;
    let targetPitch = -0.03;
    let targetRoll = 0;
    let targetGazeX = 0;
    let targetGazeY = 0;

    if (this.asleep || this.privacyMode) {
      targetPitch = 0.16;
      targetRoll = 0.08;
    } else if (petActive) {
      this.rub += dt * 4.5;
      targetYaw = this.petX * 0.16;
      targetPitch = -0.10;
      targetRoll = Math.sin(this.rub) * 0.14;
    } else if (lookCursor && (curiousNow || t - this.lastKey > 0.65)) {
      targetYaw = gx * (0.20 + curious * 0.14);
      targetPitch = -0.04 - gy * 0.14;
      targetRoll = gx * (0.05 + curious * 0.13);
      targetGazeX = gx;
      targetGazeY = gy;
    } else if (striking && striking.keyId) {
      const kt = this.desk.keyTopWorld(striking.keyId, this.tmpV1);
      const bias = clamp((kt.x - 0.12) * 0.42, -1, 1);
      targetYaw = bias * 0.18;
      targetPitch = 0.04;
      targetRoll = bias * 0.07;
      targetGazeX = bias * 0.68;
      targetGazeY = -0.52;
    }

    if (act) {
      if (act.kind === 'tilt') targetRoll += act.side * 0.19 * actEnv;
      if (act.kind === 'look') {
        targetYaw += act.side * 0.22 * actEnv;
        targetGazeX = mix(targetGazeX, act.side * 0.85, actEnv);
      }
    }
    if (cross > 0.05) {
      targetYaw = mix(targetYaw, this.crossHuffDir * 0.20, cross * 0.65);
      targetGazeX = mix(targetGazeX, this.crossHuffDir * 0.72, cross * 0.65);
    }

    const wakeEl = t - this.wakeAt;
    const wakeShake = wakeEl >= 0 && wakeEl < 0.55 ? Math.sin(wakeEl * 36) * 0.20 * (1 - wakeEl / 0.55) : 0;

    this.headRot.pitch = this.sPitch.step(targetPitch, dt);
    this.headRot.yaw = this.sYaw.step(targetYaw, dt);
    this.headRot.roll = this.sRoll.step(targetRoll, dt) + wakeShake;

    // Deform one-piece body+head smoothly & sync headMount + optional collar!
    this.deformCatBody();
    this.placeHeadAndCollar();

    // 5b) Smooth drop-on gaming headset animation + reactive KuroCat-lime/cyan RGB earcup glow!
    const hsVal = clamp(this.sHeadset.step(this.headsetOn ? 1 : 0, dt), 0, 1.18);
    if (hsVal > 0.015) {
      this.headsetGroup.visible = true;
      const settled = Math.min(1, hsVal);
      const dropOffset = (1 - settled) * 0.42;
      const sc = mix(0.84, 1.0, settled) + Math.max(0, hsVal - 1) * 0.14;
      this.headsetGroup.position.set(0, dropOffset, 0);
      this.headsetGroup.scale.setScalar(sc);
      const rgbMix = 0.5 + 0.5 * Math.sin(t * 2.3 + this.excite * 2.0);
      // Smoothly breathe between KuroCat neon-lime (#d4f53d) and electric gaming cyan (#3df2e8)
      const pulse = 0.88 + 0.12 * Math.sin(t * 5.2) + this.excite * 0.16;
      const r = clamp(mix(0.83, 0.24, rgbMix * 0.58) * pulse, 0, 1);
      const g = clamp(mix(0.96, 0.95, rgbMix * 0.58) * pulse, 0, 1);
      const b = clamp(mix(0.24, 0.92, rgbMix * 0.58) * pulse, 0, 1);
      this.headsetLedMat.color.setRGB(r, g, b);
      const micPulse = 0.78 + 0.22 * Math.sin(t * 3.8);
      this.headsetMicLedMat.color.setRGB(0.22 * micPulse, 0.97 * micPulse, 0.82 * micPulse);
    } else {
      this.headsetGroup.visible = false;
    }

    const gazeX = this.sGazeX.step(targetGazeX, dt);
    const gazeY = this.sGazeY.step(targetGazeY, dt);

    // 6) Compact ears: perk when curious, fold flat sideways into airplane ears when cross
    const earTarget =
      0.12 * this.excite * (1 - cross) +
      0.16 * curious -
      (petActive ? 0.15 : 0) -
      0.56 * cross -
      0.10 * this.sleepAmt;
    const earState = this.sEars.step(earTarget, dt);
    const earFlick = act && act.kind === 'ear' ? Math.sin(actU * Math.PI * 2) * 0.22 : 0;
    for (const e of this.ears) {
      const flick = act && act.kind === 'ear' && act.side === e.s ? earFlick : 0;
      const airplaneFold = cross * 0.56;
      e.group.rotation.z = -e.s * (e.tilt + airplaneFold - earState * 0.28 + flick);
      e.group.rotation.x = -0.06 - cross * 0.14 + curious * 0.10;
      e.group.position.set(e.s * (0.295 + cross * 0.06), 0.255 - cross * 0.045, -0.015);
    }

    // 7) Iconic KuroCat Eyes: open / curious / grumpy zero-seam / petting lazy slits / sleep arcs
    if (curiousNow && t > this.nextBlink - 0.25) this.nextBlink = t + 0.65;
    let blinkClose = 0;
    if (t >= this.nextBlink) {
      const bt = t - this.nextBlink;
      if (bt < 0.14) blinkClose = Math.sin((bt / 0.14) * Math.PI);
      else this.nextBlink = t + 2.8 + Math.random() * 3.8;
    }
    if (act && act.kind === 'slowBlink') {
      blinkClose = Math.max(blinkClose, Math.sin(actU * Math.PI));
    }

    const isSleepOrPrivacy = this.sleepAmt > 0.45 || this.privacyClose > 0.45;
    const isPetSquint = !isSleepOrPrivacy && this.squintAmt > 0.38 && blinkClose < 0.3;
    const isBlinkClosed = !isSleepOrPrivacy && !isPetSquint && blinkClose > 0.58;
    const showOpenEye = !isSleepOrPrivacy && !isPetSquint && !isBlinkClosed;

    this.blushMat.opacity += ((petActive ? 0.46 : 0) - this.blushMat.opacity) * approach(8, dt);
    if (this.blushMat.opacity < 0.004 && !petActive) this.blushMat.opacity = 0;
    const showBlush = this.blushMat.opacity > 0.004;
    catAudio.setPurrIntensity(petActive ? 0.85 : 0);

    const pal = this.currentPalette;
    // Match original KuroCat 2D: Black Cat shows NO permanent catchlights when calm, sparkling catchlights when Curious!
    const shineAmt = pal?.eyeCatchlight ? 1.0 + curious * 0.20 : Math.min(1.16, curious * 1.25);

    for (const e of this.eyes) {
      e.blush.visible = showBlush;
      e.mesh.visible = showOpenEye;
      e.closedArc.visible = isSleepOrPrivacy || isBlinkClosed;
      e.squintSlit.visible = isPetSquint;

      if (showOpenEye) {
        const irisRx = 0.156 * (1 + curious * 0.06);
        const irisRy = Math.max(0.02, 0.136 * (1 + curious * 0.09) * (1 - blinkClose * 0.8));

        const pRx = clamp(
          (0.064 + curious * 0.048 + this.excite * 0.008) * (1 - cross * 0.32),
          0.036,
          irisRx - 0.018,
        );
        const pRy = clamp((0.094 + curious * 0.020) * (1 - cross * 0.08), 0.052, irisRy - 0.010);

        const maxTx = Math.max(0.024, irisRx - pRx - 0.012);
        const maxTy = Math.max(0.020, irisRy - pRy - 0.010);
        const px = clamp(gazeX * maxTx, -maxTx, maxTx);
        const py = clamp(gazeY * maxTy, -maxTy, maxTy);

        (e.mat.uniforms.uScale.value as THREE.Vector2).set(irisRx / EYE_MAX_RX, irisRy / EYE_MAX_RY);
        e.mat.uniforms.uTilt.value = e.s * -(0.06 + 0.06 * (typingActive ? 1 : 0));
        (e.mat.uniforms.uPupilCenter.value as THREE.Vector2).set(px / irisRx, py / irisRy);
        (e.mat.uniforms.uPupilRad.value as THREE.Vector2).set(pRx / irisRx, pRy / irisRy);
        e.mat.uniforms.uCross.value = cross;
        e.mat.uniforms.uCurious.value = curious;
        e.mat.uniforms.uShineAmt.value = shineAmt;
      }
    }

    // 8) Graceful 3D KuroCat Tail (sway, sleep curl, tail key patting, 3-beat grumpy desk-thump)
    let targetThumpLift = 0;
    let targetThumpLash = 0;
    if (cross > 0.08 && this.tailTap.reach < 0.25) {
      const prev = this.thumpPhase;
      this.thumpPhase += dt * 2.55;
      const beat = Math.floor(this.thumpPhase);
      const u = this.thumpPhase - beat;
      const restBeat = beat % 4 === 3;
      if (!restBeat) {
        targetThumpLift = u < 0.58 ? smoothstep(0, 0.58, u) : u < 0.72 ? 1 - ((u - 0.58) / 0.14) ** 2 : 0;
      }
      targetThumpLash = Math.sin(this.thumpPhase * Math.PI * 0.5);
      const slamAt = beat + 0.72;
      if (!restBeat && this.isCross && prev < slamAt && this.thumpPhase >= slamAt) {
        this.spawnSlamFx();
      }
    }
    this.thumpLift += (targetThumpLift - this.thumpLift) * approach(38, dt);
    this.thumpLash += (targetThumpLash - this.thumpLash) * approach(18, dt);

    const swayRate = mix(1.0, 2.5, this.excite) * (1 - 0.75 * this.sleepAmt);
    this.tailPhase += dt * swayRate;
    const tailSway = (1 - 0.7 * this.sleepAmt) * (1 + 0.42 * curious) + (act && act.kind === 'tail' ? 1.35 * actEnv : 0);

    const attached = this.tmpTailAttached;
    const loose = this.tmpTailLoose;
    const tmpTail = this.tmpTailScratch;
    this.catRoot.updateMatrixWorld(false);
    for (let i = 0; i < this.tailRestPts.length; i++) {
      const w = i / (this.tailRestPts.length - 1);
      const pt = this.tailCurve.points[i].lerpVectors(this.tailRestPts[i], this.tailSleepPts[i], this.sleepAmt);
      if (cross > 0.01) {
        tmpTail.copy(this.tailThumpPts[i]);
        tmpTail.y += this.thumpLift * 1.02 * w ** 1.4;
        tmpTail.x -= this.thumpLift * 0.18 * w ** 2;
        tmpTail.z += this.thumpLash * 0.34 * w ** 1.5;
        pt.lerp(tmpTail, cross);
      }
      pt.x += Math.sin(this.tailPhase * 1.5 + i * 0.68) * 0.045 * w * tailSway;
      pt.y += Math.sin(this.tailPhase * 1.25 + i * 0.85) * 0.05 * w * w * tailSway;
      this.attachLocal(pt, attached);
      loose.copy(pt).add(this.homePos);
      pt.lerpVectors(attached, loose, smoothstep(0.1, 0.7, w));
    }

    if (this.tailTap.reach > 0.01 && this.tailTap.keyId) {
      const tipWorld = this.desk.keyTopWorld(this.tailTap.keyId, this.tailTap.tip);
      tipWorld.y += this.tailTap.hover + 0.045;
      const root = this.tailCurve.points[0];
      this.tmpTailPath[0].set(root.x + 0.36, 0.28, root.z + 0.22);
      this.tmpTailPath[1].set(tipWorld.x + 0.36, 0.30, mix(root.z, tipWorld.z, 0.52));
      this.tmpTailPath[2].set(tipWorld.x + 0.26, 0.32, tipWorld.z - 0.06);
      this.tmpTailPath[3].set(tipWorld.x + 0.12, mix(0.32, tipWorld.y + 0.02, 0.5), tipWorld.z + 0.02);
      for (let i = 1; i < this.tailCurve.points.length; i++) {
        const dst = i - 1 < 4 ? this.tmpTailPath[i - 1] : tipWorld;
        this.tailCurve.points[i].lerp(dst, this.tailTap.reach);
      }
    }

    const tailActive = this.tailTap.down || t - this.tailTap.landAt < 0.28;
    this.tailTap.reach += ((tailActive ? 1 : 0) - this.tailTap.reach) * approach(tailActive ? 24 : 9, dt);
    const tailHoverTarget = this.tailTap.down ? 0 : 0.095;
    this.tailTap.hover += (tailHoverTarget - this.tailTap.hover) * approach(34, dt);

    this.tailLimb.update(
      (u, out) => this.tailCurve.getPoint(u, out),
      (u) => mix(0.068, 0.044, u) * (1 - 0.12 * Math.sin(u * Math.PI)),
    );

    // 9) Sleek Unified KuroCat Arms + Cozy Tucked Idle Belly Paws!
    const updatePaw = (paw: PawTrack3D, isLeft: boolean) => {
      const activeForMouse = isLeft && mousing;
      const activeForKey = Boolean(paw.keyId && (paw.down || t - paw.t0 < 0.52)) && !this.asleep && !this.privacyMode;
      const wantTuck = !activeForMouse && !activeForKey ? 1 : 0;
      paw.tuckWeight += (wantTuck - paw.tuckWeight) * approach(wantTuck ? 6.5 : 22, dt);

      this.tmpPawLocal.set(paw.side * 0.24, 0.16 - this.sleepAmt * 0.06, 0.54);
      const tuckHome = this.attachLocal(this.tmpPawLocal, this.tmpV3);

      if (activeForMouse) {
        const mt = this.desk.mouseTopWorld(this.tmpV1);
        paw.pos.copy(paw.spring.step(mt, dt));
        paw.last.copy(paw.pos);
        return;
      }
      if (activeForKey && paw.keyId) {
        const kt = this.desk.keyTopWorld(paw.keyId, this.tmpV1);
        if (paw.down) {
          const u = clamp((t - paw.t0) / paw.dur, 0, 1);
          paw.pos.lerpVectors(paw.from, kt, smoothstep(0, 1, u));
          paw.pos.y += Math.sin(u * Math.PI) * 0.22;
          paw.spring.x.copy(paw.pos);
          paw.spring.v.set(0, 0, 0);
        } else {
          const hover = this.tmpV2.set(kt.x, kt.y + 0.14, kt.z - 0.05);
          paw.pos.copy(paw.spring.step(hover, dt));
        }
        paw.last.copy(paw.pos);
        return;
      }

      // Smoothly tuck sleek paws onto front tummy above the keyboard when calm/idle!
      paw.pos.copy(paw.spring.step(tuckHome, dt));
      paw.last.copy(paw.pos);
    };
    updatePaw(this.paws.L, true);
    updatePaw(this.paws.R, false);

    for (const arm of this.arms) {
      const pawTrack = arm.s < 0 ? this.paws.L : this.paws.R;
      this.attachLocal(arm.shoulderLocal, arm.shoulder);
      arm.hand.copy(pawTrack.pos);

      const tuck = pawTrack.tuckWeight;
      const onMouse = arm.s < 0 && mousing;
      const reach = arm.shoulder.distanceTo(arm.hand);
      const lift = mix(0.24, 0.06, clamp((reach - 0.85) / 0.85, 0, 1)) * (1 - tuck * 0.65);

      this.tmpArmOff.set(
        arm.s * (onMouse ? 0.09 : 0.14 * (1 - tuck * 0.85)),
        -0.06 + lift * 0.32,
        mix(0.18, 0.02, tuck),
      );
      arm.c1.copy(arm.shoulder).lerp(arm.hand, 0.36).add(this.tmpArmOff);

      this.tmpArmOff.set(
        arm.s * (onMouse ? 0.05 : 0.05 * (1 - tuck)),
        lift * 0.85,
        mix(0.12, 0.02, tuck),
      );
      arm.c2.copy(arm.shoulder).lerp(arm.hand, 0.72).add(this.tmpArmOff);

      arm.limb.update(
        (u, out) => {
          const m = 1 - u;
          const m2 = m * m;
          const u2 = u * u;
          out.set(
            m2 * m * arm.shoulder.x + 3 * m2 * u * arm.c1.x + 3 * m * u2 * arm.c2.x + u2 * u * arm.hand.x,
            m2 * m * arm.shoulder.y + 3 * m2 * u * arm.c1.y + 3 * m * u2 * arm.c2.y + u2 * u * arm.hand.y,
            m2 * m * arm.shoulder.z + 3 * m2 * u * arm.c1.z + 3 * m * u2 * arm.c2.z + u2 * u * arm.hand.z,
          );
        },
        // Cute chubby plush kitten arms (matches approved 2D arm optimization — shorter & fuller!)
        (u) => mix(0.118, 0.082, smoothstep(0.0, 0.88, u)) * mix(1.0, 0.76, tuck * (1 - u * 0.4)),
      );

      // Soft sculpted plush paw resting cleanly right behind top edge of keyboard when calm
      const pawScale = mix(1.0, 0.94, tuck);
      arm.paw.position.set(arm.hand.x, arm.hand.y - 0.010, arm.hand.z + 0.014);
      arm.paw.scale.set(0.126 * pawScale, 0.080 * pawScale, 0.118 * pawScale);
      arm.paw.rotation.y = Math.atan2(arm.hand.x - arm.c2.x, Math.max(0.1, arm.hand.z - arm.c2.z)) * 0.42;
    }

    // 10) Floating Key Bubbles, Anger Mark 💢 & Sleep Zzz FX
    for (let i = this.popups.length - 1; i >= 0; i--) {
      const p = this.popups[i];
      const age = t - p.t0;
      const tailAge = p.upAt !== null ? t - p.upAt : 0;
      if (age > 1.2 || tailAge > 0.55) {
        this.scene.remove(p.sprite);
        p.sprite.material.dispose();
        this.popups.splice(i, 1);
        continue;
      }
      const pop = smoothstep(0, 0.12, age);
      const fade = p.upAt !== null ? 1 - smoothstep(0.18, 0.55, tailAge) : 1;
      p.sprite.position.y += dt * 0.20;
      const s = 0.72 + 0.28 * pop;
      p.sprite.scale.set(p.baseW * s, p.baseH * s, 1);
      p.sprite.material.opacity = fade;
    }

    if (cross > 0.25) {
      this.angerSprite.visible = true;
      const pulse = 1 + 0.10 * Math.sin(t * 14) + (t - this.lastSlam < 0.22 ? 0.16 : 0);
      this.headMount.updateMatrixWorld(false);
      this.tmpFxPos.set(-0.52, 0.44, 0.18).applyMatrix4(this.headMount.matrixWorld);
      this.angerSprite.position.copy(this.tmpFxPos);
      this.angerSprite.scale.setScalar(0.30 * pulse * smoothstep(0.25, 0.65, cross));
    } else {
      this.angerSprite.visible = false;
    }

    if (this.sleepAmt > 0.65 && t >= this.nextZ) {
      this.nextZ = t + 1.35;
      this.headMount.updateMatrixWorld(false);
      const zPos = this.tmpFxPos.set(0.46, 0.42, 0.16).applyMatrix4(this.headMount.matrixWorld);
      this.spawnFx('z', zPos, new THREE.Vector3(0.08, 0.36, 0), 0.22, 2.3);
    }

    for (let i = this.fxList.length - 1; i >= 0; i--) {
      const fx = this.fxList[i];
      const u = (t - fx.t0) / fx.dur;
      if (u >= 1) {
        this.scene.remove(fx.sprite);
        fx.sprite.material.dispose();
        this.fxList.splice(i, 1);
        continue;
      }
      fx.sprite.position.addScaledVector(fx.vel, dt);
      if (fx.kind === 'z') {
        fx.sprite.position.x = fx.baseX + Math.sin(u * Math.PI * 2.2) * 0.08 + u * 0.14;
        const sc = fx.size * (0.65 + 0.65 * u);
        fx.sprite.scale.set(sc, sc, 1);
        fx.sprite.material.opacity = smoothstep(0, 0.18, u) * (1 - smoothstep(0.62, 1, u));
      } else if (fx.kind === 'bang') {
        const sc = fx.size * (0.55 + 0.65 * smoothstep(0, 0.4, u));
        fx.sprite.scale.set(sc, sc, 1);
        fx.sprite.material.opacity = 1 - smoothstep(0.35, 1, u);
      } else {
        fx.vel.multiplyScalar(1 - approach(3.2, dt));
        const sc = fx.size * (0.8 + 0.9 * u);
        fx.sprite.scale.set(sc, sc, 1);
        fx.sprite.material.opacity = (1 - u) * 0.85;
      }
    }
  }
}
