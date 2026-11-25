/**
 * Ported from KuroCat (https://github.com/qianyiai/kurocat), v5.1.0.
 * Copyright (c) 2026 KuroCat Contributors, MIT License.
 * See src-tauri/resources/licenses/KUROCAT-MIT.txt.
 */
import * as THREE from 'three';
import {
  getKeyboardLayout,
  resolveKeyByVk,
  type KeyboardLayoutId,
  type KeySpec,
} from './keyboardLayout';
import type { KeyboardOrientation } from './catEngine';

export class Spring1D {
  public x: number;
  public v = 0;
  public hz: number;
  public damping: number;

  constructor(x = 0, hz = 2, damping = 0.7) {
    this.x = x;
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

  public kick(v: number): void {
    this.v += v;
  }

  public set(x: number): void {
    this.x = x;
    this.v = 0;
  }
}

export class Spring3D {
  public x: THREE.Vector3;
  public v = new THREE.Vector3();
  public hz: number;
  public damping: number;

  constructor(x: THREE.Vector3, hz = 3.2, damping = 0.78) {
    this.x = x.clone();
    this.hz = hz;
    this.damping = damping;
  }

  public step(target: THREE.Vector3, dt: number): THREE.Vector3 {
    const w = 2 * Math.PI * this.hz;
    const n = Math.max(1, Math.ceil(dt * 240));
    const h = dt / n;
    for (let i = 0; i < n; i++) {
      this.v.x += (w * w * (target.x - this.x.x) - 2 * this.damping * w * this.v.x) * h;
      this.v.y += (w * w * (target.y - this.x.y) - 2 * this.damping * w * this.v.y) * h;
      this.v.z += (w * w * (target.z - this.x.z) - 2 * this.damping * w * this.v.z) * h;
      this.x.x += this.v.x * h;
      this.x.y += this.v.y * h;
      this.x.z += this.v.z * h;
    }
    return this.x;
  }
}

export const approach = (rate: number, dt: number): number => 1 - Math.exp(-rate * dt);
export const clamp = (v: number, a: number, b: number): number => Math.max(a, Math.min(b, v));
export const mix = (a: number, b: number, t: number): number => a + (b - a) * t;
export const smoothstep = (a: number, b: number, x: number): number => {
  const t = clamp((x - a) / (b - a), 0, 1);
  return t * t * (3 - 2 * t);
};

/**
 * Lightweight static BufferGeometry merger (position + normal + optional vertex color)
 * to collapse static multi-mesh props (headset, keyboard chassis, whiskers, mouse seams) into single draw calls.
 */
export function mergeStaticGeometries(
  parts: Array<{ geo: THREE.BufferGeometry; matrix?: THREE.Matrix4; color?: string }>,
): THREE.BufferGeometry {
  let totalVerts = 0;
  let totalIndices = 0;
  let hasColor = false;

  const prepared = parts.map(({ geo, matrix, color }) => {
    const g = geo.clone();
    if (matrix) g.applyMatrix4(matrix);
    if (!g.attributes.normal) g.computeVertexNormals();
    if (color !== undefined) hasColor = true;
    const pos = g.attributes.position;
    const idx = g.getIndex();
    totalVerts += pos.count;
    totalIndices += idx ? idx.count : pos.count;
    return { g, color };
  });

  const positions = new Float32Array(totalVerts * 3);
  const normals = new Float32Array(totalVerts * 3);
  const colors = hasColor ? new Float32Array(totalVerts * 3) : null;
  const indices = totalVerts > 65535 ? new Uint32Array(totalIndices) : new Uint16Array(totalIndices);
  const tmpCol = new THREE.Color();

  let vOffset = 0;
  let iOffset = 0;
  for (const { g, color } of prepared) {
    const pos = g.attributes.position;
    const nrm = g.attributes.normal;
    const count = pos.count;
    for (let i = 0; i < count; i++) {
      const dst = (vOffset + i) * 3;
      positions[dst] = pos.getX(i);
      positions[dst + 1] = pos.getY(i);
      positions[dst + 2] = pos.getZ(i);
      normals[dst] = nrm.getX(i);
      normals[dst + 1] = nrm.getY(i);
      normals[dst + 2] = nrm.getZ(i);
    }
    if (colors) {
      tmpCol.set(color ?? '#ffffff');
      for (let i = 0; i < count; i++) {
        const dst = (vOffset + i) * 3;
        colors[dst] = tmpCol.r;
        colors[dst + 1] = tmpCol.g;
        colors[dst + 2] = tmpCol.b;
      }
    }
    const idx = g.getIndex();
    if (idx) {
      for (let i = 0; i < idx.count; i++) {
        indices[iOffset++] = idx.getX(i) + vOffset;
      }
    } else {
      for (let i = 0; i < count; i++) {
        indices[iOffset++] = vOffset + i;
      }
    }
    vOffset += count;
    g.dispose();
  }

  const merged = new THREE.BufferGeometry();
  merged.setAttribute('position', new THREE.BufferAttribute(positions, 3));
  merged.setAttribute('normal', new THREE.BufferAttribute(normals, 3));
  if (colors) {
    merged.setAttribute('color', new THREE.BufferAttribute(colors, 3));
  }
  merged.setIndex(new THREE.BufferAttribute(indices, 1));
  return merged;
}

/** Dynamic variable-radius 3D tube rebuilt cleanly in place each frame (zero GC churn, precomputed trig). */
export class TaperTube3D {
  public readonly mesh: THREE.Mesh;
  private readonly n: number;
  private readonly m: number;
  private readonly pts: THREE.Vector3[];
  private readonly cosTable: Float32Array;
  private readonly sinTable: Float32Array;
  private readonly T = new THREE.Vector3();
  private readonly N = new THREE.Vector3();
  private readonly B = new THREE.Vector3();
  private readonly tmp = new THREE.Vector3();

  constructor(segments: number, radial: number, material: THREE.Material, renderOrder = 0) {
    this.n = segments;
    this.m = radial;
    this.cosTable = new Float32Array(radial);
    this.sinTable = new Float32Array(radial);
    for (let j = 0; j < radial; j++) {
      const a = (j / radial) * Math.PI * 2;
      this.cosTable[j] = Math.cos(a);
      this.sinTable[j] = Math.sin(a);
    }

    const geo = new THREE.BufferGeometry();
    const count = (segments + 1) * radial;
    geo.setAttribute('position', new THREE.BufferAttribute(new Float32Array(count * 3), 3));
    geo.setAttribute('normal', new THREE.BufferAttribute(new Float32Array(count * 3), 3));
    geo.setAttribute(
      'along',
      new THREE.BufferAttribute(
        Float32Array.from({ length: count }, (_, k) => Math.floor(k / radial) / segments),
        1,
      ),
    );
    const index: number[] = [];
    for (let i = 0; i < segments; i++) {
      for (let j = 0; j < radial; j++) {
        const a = i * radial + j;
        const b = i * radial + ((j + 1) % radial);
        const c = a + radial;
        const d = b + radial;
        index.push(a, b, c, b, d, c);
      }
    }
    geo.setIndex(index);
    this.mesh = new THREE.Mesh(geo, material);
    this.mesh.renderOrder = renderOrder;
    this.mesh.frustumCulled = false;
    this.pts = Array.from({ length: segments + 1 }, () => new THREE.Vector3());
  }

  public update(
    point: (t: number, out: THREE.Vector3) => void,
    radius: (t: number) => number,
    grow: number | ((t: number) => number) = 0,
  ): void {
    const { n, m, pts, cosTable, sinTable, T, N, B, tmp } = this;
    for (let i = 0; i <= n; i++) point(i / n, pts[i]);
    const pos = this.mesh.geometry.attributes.position.array as Float32Array;
    const nrm = this.mesh.geometry.attributes.normal.array as Float32Array;
    const isGrowFn = typeof grow === 'function';
    for (let i = 0; i <= n; i++) {
      T.subVectors(pts[Math.min(n, i + 1)], pts[Math.max(0, i - 1)]).normalize();
      if (i === 0) {
        tmp.set(0, 1, 0);
        if (Math.abs(T.dot(tmp)) > 0.9) tmp.set(1, 0, 0);
        N.crossVectors(T, tmp).normalize();
      } else {
        B.crossVectors(N, T);
        N.crossVectors(T, B).normalize();
      }
      B.crossVectors(T, N).normalize();
      const u = i / n;
      const gVal = isGrowFn ? (grow as (t: number) => number)(u) : (grow as number);
      const r = Math.max(0.0008, radius(u) + gVal);
      const px = pts[i].x;
      const py = pts[i].y;
      const pz = pts[i].z;
      const nx = N.x;
      const ny = N.y;
      const nz = N.z;
      const bx = B.x;
      const by = B.y;
      const bz = B.z;
      for (let j = 0; j < m; j++) {
        const ca = cosTable[j];
        const sa = sinTable[j];
        const c = ca * r;
        const s = sa * r;
        const k = (i * m + j) * 3;
        pos[k] = px + nx * c + bx * s;
        pos[k + 1] = py + ny * c + by * s;
        pos[k + 2] = pz + nz * c + bz * s;
        nrm[k] = nx * ca + bx * sa;
        nrm[k + 1] = ny * ca + by * sa;
        nrm[k + 2] = nz * ca + bz * sa;
      }
    }
    this.mesh.geometry.attributes.position.needsUpdate = true;
    this.mesh.geometry.attributes.normal.needsUpdate = true;
  }
}

interface KeyEntry3D {
  spec: KeySpec;
  index: number;
  x: number;
  z: number;
  scaleX: number;
  scaleY: number;
  scaleZ: number;
  down: boolean;
  depth: number;
  glow: number;
}

interface KeyAtlasData {
  tex: THREE.CanvasTexture;
  rects: Map<string, [number, number, number, number]>;
}

const keyAtlasCache = new Map<string, KeyAtlasData>();
const miscTextureCache = new Map<string, THREE.CanvasTexture>();

/**
 * Packs all unique keycap legends for a layout + orientation into ONE single crisp power-of-2
 * CanvasTexture atlas (128x128 per tile with 2px inner bleed padding), collapsing 67-108 separate
 * keycap textures and 201-324 draw calls into just 2 InstancedMesh draw calls total!
 */
function getKeycapAtlas(
  layoutId: KeyboardLayoutId,
  keys: readonly KeySpec[],
  rotated180: boolean,
): KeyAtlasData {
  const cacheKey = `${layoutId}|${rotated180 ? 1 : 0}`;
  const cached = keyAtlasCache.get(cacheKey);
  if (cached) return cached;

  const unique: Array<{ key: string; label: string; accent: 'base' | 'mod' | 'orange' }> = [];
  const seen = new Set<string>();
  for (const spec of keys) {
    const accent: 'base' | 'mod' | 'orange' =
      spec.accent === 'orange' ? 'orange' : spec.accent === 'mod' ? 'mod' : 'base';
    const label = spec.id === 'space' ? '' : spec.id === 'esc' ? 'Esc' : spec.label;
    const tk = `${label}|${accent}`;
    if (!seen.has(tk)) {
      seen.add(tk);
      unique.push({ key: tk, label, accent });
    }
  }

  // Place orange accent tiles on their own isolated bottom row so mipmaps NEVER bleed orange onto cream keys!
  const nonOrange = unique.filter((u) => u.accent !== 'orange');
  const orangeOnly = unique.filter((u) => u.accent === 'orange');

  const CELL = 128;
  const cols = nonOrange.length <= 56 ? 8 : 16;
  const nonOrangeRows = Math.ceil(nonOrange.length / cols);
  const totalRows = nonOrangeRows + (orangeOnly.length > 0 ? 1 : 0);
  const atlasW = cols * CELL;
  const atlasH = totalRows <= 8 ? 1024 : 2048;

  const c = document.createElement('canvas');
  c.width = atlasW;
  c.height = atlasH;
  const g = c.getContext('2d')!;
  g.fillStyle = '#f9f8f3';
  g.fillRect(0, 0, atlasW, atlasH);

  const rects = new Map<string, [number, number, number, number]>();
  const PAD = 10.0;

  const paintTile = (
    item: { key: string; label: string; accent: 'base' | 'mod' | 'orange' },
    col: number,
    row: number,
  ) => {
    const ox = col * CELL;
    const oy = row * CELL;

    g.fillStyle =
      item.accent === 'orange' ? '#f59b2e' : item.accent === 'mod' ? '#e6e4db' : '#f9f8f3';
    g.fillRect(ox, oy, CELL, CELL);

    // Sculpted concave PBT keycap sheen
    const grad = g.createLinearGradient(0, oy + PAD, 0, oy + CELL - PAD);
    grad.addColorStop(0, 'rgba(255,255,255,0.28)');
    grad.addColorStop(1, 'rgba(0,0,0,0.08)');
    g.fillStyle = grad;
    g.fillRect(ox, oy, CELL, CELL);

    if (item.label) {
      g.save();
      g.translate(ox + CELL * 0.5, oy + CELL * 0.5);
      if (rotated180) g.rotate(Math.PI);
      const len = item.label.length;
      const fontSize = len >= 4 ? 29 : len === 3 ? 35 : len === 2 ? 42 : 52;
      g.font = `700 ${fontSize}px "Segoe UI", system-ui, sans-serif`;
      g.fillStyle = item.accent === 'orange' ? '#ffffff' : '#2b2d34';
      g.textAlign = 'center';
      g.textBaseline = 'middle';
      g.fillText(item.label, 0, 2);
      g.restore();
    }

    // CanvasTexture uses flipY = true: canvas y=0 -> v=1, y=atlasH -> v=0
    const u0 = (ox + PAD) / atlasW;
    const du = (CELL - PAD * 2) / atlasW;
    const v0 = 1.0 - (oy + CELL - PAD) / atlasH;
    const dv = (CELL - PAD * 2) / atlasH;
    rects.set(item.key, [u0, v0, du, dv]);
  };

  for (let i = 0; i < nonOrange.length; i++) {
    paintTile(nonOrange[i], i % cols, Math.floor(i / cols));
  }
  if (orangeOnly.length > 0) {
    // Place orange tiles on the bottom-most atlas row for a full-row mipmap buffer + 10px per-tile padding!
    const orangeRow = Math.floor(atlasH / CELL) - 1;
    g.fillStyle = '#f59b2e';
    g.fillRect(0, orangeRow * CELL, atlasW, CELL);
    for (let i = 0; i < orangeOnly.length; i++) {
      paintTile(orangeOnly[i], i % cols, orangeRow);
    }
  }

  const tex = new THREE.CanvasTexture(c);
  tex.anisotropy = 4;
  tex.colorSpace = THREE.SRGBColorSpace;
  tex.needsUpdate = true;
  const entry: KeyAtlasData = { tex, rects };
  keyAtlasCache.set(cacheKey, entry);
  return entry;
}

function makeSoftShadowTexture(): THREE.CanvasTexture {
  const hit = miscTextureCache.get('shadow:desk');
  if (hit) return hit;
  const c = document.createElement('canvas');
  c.width = 256;
  c.height = 256;
  const g = c.getContext('2d')!;
  const grad = g.createRadialGradient(128, 128, 22, 128, 128, 122);
  grad.addColorStop(0, 'rgba(0, 0, 0, 0.44)');
  grad.addColorStop(0.55, 'rgba(0, 0, 0, 0.20)');
  grad.addColorStop(1, 'rgba(0, 0, 0, 0)');
  g.fillStyle = grad;
  g.fillRect(0, 0, 256, 256);
  const tex = new THREE.CanvasTexture(c);
  tex.colorSpace = THREE.SRGBColorSpace;
  miscTextureCache.set('shadow:desk', tex);
  return tex;
}

function makePrivacyTexture(): THREE.CanvasTexture {
  const hit = miscTextureCache.get('privacy:shield');
  if (hit) return hit;
  const pCanvas = document.createElement('canvas');
  pCanvas.width = 512;
  pCanvas.height = 160;
  const pg = pCanvas.getContext('2d')!;
  pg.fillStyle = 'rgba(15, 20, 32, 0.88)';
  pg.fillRect(0, 0, 512, 160);
  pg.strokeStyle = 'rgba(244, 114, 182, 0.65)';
  pg.lineWidth = 6;
  pg.strokeRect(6, 6, 500, 148);
  pg.font = 'bold 34px "Segoe UI", "PingFang SC", sans-serif';
  pg.fillStyle = '#fbcfe8';
  pg.textAlign = 'center';
  pg.textBaseline = 'middle';
  pg.fillText('🔒 密码防偷窥 · 已隐藏按键', 256, 80);
  const pTex = new THREE.CanvasTexture(pCanvas);
  pTex.colorSpace = THREE.SRGBColorSpace;
  miscTextureCache.set('privacy:shield', pTex);
  return pTex;
}

function makeMouseLabelTexture(char: 'L' | 'R', rotated180: boolean): THREE.CanvasTexture {
  const key = `mouse:${char}:${rotated180 ? 1 : 0}`;
  const hit = miscTextureCache.get(key);
  if (hit) return hit;
  const c = document.createElement('canvas');
  c.width = 64;
  c.height = 64;
  const g = c.getContext('2d')!;
  g.save();
  g.translate(32, 32);
  if (rotated180) g.rotate(Math.PI);
  g.font = '800 36px "Segoe UI", system-ui, sans-serif';
  g.fillStyle = 'rgba(42, 46, 56, 0.56)';
  g.textAlign = 'center';
  g.textBaseline = 'middle';
  g.fillText(char, 0, 2);
  g.restore();
  const tex = new THREE.CanvasTexture(c);
  tex.colorSpace = THREE.SRGBColorSpace;
  miscTextureCache.set(key, tex);
  return tex;
}

function makeRoundedSlabGeo(w: number, d: number, h: number, r: number, y: number): THREE.ExtrudeGeometry {
  const s = new THREE.Shape();
  const x0 = -w / 2;
  const z0 = -d / 2;
  s.moveTo(x0 + r, z0);
  s.lineTo(x0 + w - r, z0);
  s.quadraticCurveTo(x0 + w, z0, x0 + w, z0 + r);
  s.lineTo(x0 + w, z0 + d - r);
  s.quadraticCurveTo(x0 + w, z0 + d, x0 + w - r, z0 + d);
  s.lineTo(x0 + r, z0 + d);
  s.quadraticCurveTo(x0, z0 + d, x0, z0 + d - r);
  s.lineTo(x0, z0 + r);
  s.quadraticCurveTo(x0, z0, x0 + r, z0);
  const g = new THREE.ExtrudeGeometry(s, { depth: h, bevelEnabled: false, curveSegments: 6 });
  g.rotateX(Math.PI / 2);
  g.translate(0, y + h, 0);
  return g;
}

function createInstancedKeyTopMaterial(atlasTex: THREE.CanvasTexture): THREE.ShaderMaterial {
  return new THREE.ShaderMaterial({
    uniforms: {
      uAtlas: { value: atlasTex },
    },
    vertexShader: `
      attribute vec4 aUvRect;
      attribute vec3 aTint;
      varying vec2 vAtlasUv;
      varying vec3 vTint;
      void main() {
        vAtlasUv = aUvRect.xy + uv * aUvRect.zw;
        vTint = aTint;
        vec4 mvPosition = vec4(position, 1.0);
        #ifdef USE_INSTANCING
          mvPosition = instanceMatrix * mvPosition;
        #endif
        mvPosition = modelViewMatrix * mvPosition;
        gl_Position = projectionMatrix * mvPosition;
      }
    `,
    fragmentShader: `
      uniform sampler2D uAtlas;
      varying vec2 vAtlasUv;
      varying vec3 vTint;
      void main() {
        vec4 texel = texture2D(uAtlas, vAtlasUv);
        gl_FragColor = vec4(texel.rgb * vTint, 1.0);
        #include <colorspace_fragment>
      }
    `,
  });
}

/**
 * KuroCat's iconic clean desktop gear in real 3D:
 * - Slightly canted matte-charcoal mechanical keyboard rendered with 2 InstancedMeshes + 1 Texture Atlas
 * - Cordless oval split-plate L/R mini mouse with engraved L/R & dual-color highlight
 */
export class KeyboardAndMouse3D {
  public readonly boardGroup = new THREE.Group();
  public readonly mouseGroup = new THREE.Group();
  public readonly deskGroup = new THREE.Group();

  private readonly capSideGeo: THREE.CylinderGeometry;
  private readonly capTopBaseGeo: THREE.PlaneGeometry;
  private readonly capSideMat = new THREE.MeshBasicMaterial({ color: '#ffffff' });
  private readonly chassisMat = new THREE.MeshBasicMaterial({ vertexColors: true });
  private readonly kbShadowMat: THREE.MeshBasicMaterial;
  private readonly shadowTex = makeSoftShadowTexture();

  private readonly sideColorBase = new THREE.Color('#dad7cd');
  private readonly sideColorMod = new THREE.Color('#c4c0b4');
  private readonly sideColorOrange = new THREE.Color('#c4741a');
  private readonly whiteColor = new THREE.Color('#ffffff');

  private keys = new Map<string, KeyEntry3D>();
  private activeKeys = new Set<KeyEntry3D>();
  private sideInstanced: THREE.InstancedMesh | null = null;
  private topInstanced: THREE.InstancedMesh | null = null;
  private topTintAttr: THREE.InstancedBufferAttribute | null = null;
  private topTintArray: Float32Array | null = null;
  private boardDisposables: Array<{ dispose(): void }> = [];

  private layoutId: KeyboardLayoutId = '65';
  private orientation: KeyboardOrientation = 'cat';
  private hitColor = new THREE.Color('#d4f044');
  private rightBtnColor = new THREE.Color('#ff7b3b');
  private boardH = 0.115;
  private capH = 0.056;
  private boardWidth = 2.4;

  // Scratch objects (zero per-frame GC allocations)
  private readonly tmpPos = new THREE.Vector3();
  private readonly tmpScale = new THREE.Vector3();
  private readonly tmpQuat = new THREE.Quaternion();
  private readonly tmpMat = new THREE.Matrix4();
  private readonly tmpCol = new THREE.Color();

  // Cordless 3D KuroCat Mini Mouse
  private readonly mouseShape = new THREE.Group();
  private readonly mouseBodyMat: THREE.MeshBasicMaterial;
  private readonly leftBtnMat: THREE.MeshBasicMaterial;
  private readonly rightBtnMat: THREE.MeshBasicMaterial;
  private readonly wheelMat: THREE.MeshBasicMaterial;
  private readonly mouseLabelLMat: THREE.MeshBasicMaterial;
  private readonly mouseLabelRMat: THREE.MeshBasicMaterial;
  private readonly mouseHome = new THREE.Vector3(-1.32, 0.015, 1.04);
  private readonly mouseOffset = new THREE.Vector3();
  private readonly mouseTarget = new THREE.Vector3();
  private readonly mouseFollow = new Spring3D(new THREE.Vector3(), 5.5, 0.78);
  private leftPress = 0;
  private rightPress = 0;
  private wheelGlow = 0;
  private lastDeskHopY = Number.NaN;

  private privacyPlate: THREE.Mesh | null = null;
  private privacyMat: THREE.MeshBasicMaterial;

  constructor(scene: THREE.Scene) {
    // Open-ended 4-sided beveled keycap frustum (no invisible underside!)
    this.capSideGeo = new THREE.CylinderGeometry(Math.SQRT1_2 * 0.80, Math.SQRT1_2, 1, 4, 1, true);
    this.capSideGeo.rotateY(Math.PI / 4);
    this.capSideGeo.translate(0, 0.5, 0);

    // Exact square top face matching the frustum top corners (+-0.40 at y=1.0)
    this.capTopBaseGeo = new THREE.PlaneGeometry(0.80, 0.80);
    this.capTopBaseGeo.rotateX(-Math.PI / 2);
    this.capTopBaseGeo.translate(0, 1.0, 0);

    this.kbShadowMat = new THREE.MeshBasicMaterial({
      map: this.shadowTex,
      transparent: true,
      depthWrite: false,
      opacity: 0.72,
    });
    this.privacyMat = new THREE.MeshBasicMaterial({
      map: makePrivacyTexture(),
      transparent: true,
      opacity: 0,
      depthTest: false,
    });

    // Subtle playful KuroCat keyboard tilt matching original catEngine.ts
    this.boardGroup.position.set(0.24, 0.015, 1.08);
    this.boardGroup.rotation.set(0.06, -0.042, 0);
    this.deskGroup.add(this.boardGroup);

    // Build Cordless KuroCat Oval Split-Plate Mini Mouse
    this.mouseGroup.position.copy(this.mouseHome);
    this.mouseShape.rotation.y = -0.10;
    this.mouseGroup.add(this.mouseShape);

    const MW = 0.165;
    const MH = 0.098;
    const ML = 0.228;
    const domeGeo = new THREE.SphereGeometry(1, 28, 16, 0, Math.PI * 2, 0, Math.PI / 2);
    const q = domeGeo.attributes.position;
    const colors: number[] = [];
    const topC = new THREE.Color('#f7f7f2');
    const sideC = new THREE.Color('#cbc8be');
    const c = new THREE.Color();
    for (let i = 0; i < q.count; i++) {
      const x = q.getX(i);
      const y = q.getY(i);
      const z = q.getZ(i);
      q.setXYZ(i, x * (1 - 0.07 * z), y * (1 - 0.12 * z), z);
      c.copy(sideC).lerp(topC, smoothstep(0.08, 0.76, y));
      colors.push(c.r, c.g, c.b);
    }
    domeGeo.setAttribute('color', new THREE.Float32BufferAttribute(colors, 3));

    const mouseShadow = new THREE.Mesh(
      new THREE.PlaneGeometry(MW * 3.1, ML * 2.8),
      new THREE.MeshBasicMaterial({ map: this.shadowTex, transparent: true, depthWrite: false, opacity: 0.78 }),
    );
    mouseShadow.rotation.x = -Math.PI / 2;
    mouseShadow.position.set(0, 0.002, 0.012);

    const baseSkirt = new THREE.Mesh(
      new THREE.CylinderGeometry(MW * 0.96, MW * 0.92, 0.018, 24),
      new THREE.MeshBasicMaterial({ color: '#b7b5ac' }),
    );
    baseSkirt.scale.set(1, 1, ML / MW);
    baseSkirt.position.y = 0.008;

    this.mouseBodyMat = new THREE.MeshBasicMaterial({ vertexColors: true, color: '#ffffff' });
    const dome = new THREE.Mesh(domeGeo, this.mouseBodyMat);
    dome.scale.set(MW, MH, ML);
    dome.position.y = 0.012;

    const onTop = (x: number, z: number) =>
      0.012 +
      MH *
        (1 - (0.12 * z) / ML) *
        Math.sqrt(Math.max(0, 1 - (x / (MW * (1 - (0.07 * z) / ML))) ** 2 - (z / ML) ** 2));

    // Merge vertical & horizontal mouse seams into a single static draw call
    const seamMat = new THREE.MeshBasicMaterial({ color: '#8e9099', depthTest: true });
    const vSeam = new TaperTube3D(12, 4, seamMat, 6);
    vSeam.update(
      (t, out) => {
        const z = mix(-0.01, ML * 0.95, t);
        out.set(0, onTop(0, z) + 0.003, z);
      },
      () => 0.0038,
    );
    const hSeam = new TaperTube3D(14, 4, seamMat, 6);
    hSeam.update(
      (t, out) => {
        const x = mix(-MW * 0.94, MW * 0.94, t);
        const z = -0.01 + 0.012 * Math.cos(Math.PI * (t - 0.5));
        out.set(x, onTop(x, z) + 0.003, z);
      },
      () => 0.0036,
    );
    const mergedSeamsGeo = mergeStaticGeometries([
      { geo: vSeam.mesh.geometry },
      { geo: hSeam.mesh.geometry },
    ]);
    vSeam.mesh.geometry.dispose();
    hSeam.mesh.geometry.dispose();
    const seamsMesh = new THREE.Mesh(mergedSeamsGeo, seamMat);
    seamsMesh.renderOrder = 6;

    this.wheelMat = new THREE.MeshBasicMaterial({ color: '#555966', depthTest: true });
    const wheel = new THREE.Mesh(new THREE.SphereGeometry(1, 14, 8), this.wheelMat);
    wheel.scale.set(0.016, 0.013, 0.036);
    wheel.position.set(0, onTop(0, ML * 0.44) + 0.005, ML * 0.44);
    wheel.renderOrder = 7;

    // Curved surface-hugging LMB (+X in cat-facing) & RMB (-X in cat-facing) highlight plates
    const makeBtnPatch = (sideSign: -1 | 1) => {
      const geo = new THREE.PlaneGeometry(1, 1, 8, 8);
      const pos = geo.attributes.position;
      for (let i = 0; i < pos.count; i++) {
        const u = pos.getX(i) + 0.5;
        const v = pos.getY(i) + 0.5;
        const ang = (1 - v) * (Math.PI * 0.46);
        const rad = 0.08 + u * 0.85;
        const x = sideSign * Math.max(0.010, Math.cos(ang) * rad * MW * 0.92);
        const z = Math.max(0.004, Math.sin(ang) * rad * ML * 0.92);
        pos.setXYZ(i, x, onTop(x, z) + 0.004, z);
      }
      geo.computeVertexNormals();
      return geo;
    };

    this.leftBtnMat = new THREE.MeshBasicMaterial({
      color: '#d4f044',
      transparent: true,
      opacity: 0,
      depthTest: true,
      depthWrite: false,
      side: THREE.DoubleSide,
    });
    const leftBtn = new THREE.Mesh(makeBtnPatch(1), this.leftBtnMat);
    leftBtn.renderOrder = 5;
    leftBtn.visible = false;

    this.rightBtnMat = new THREE.MeshBasicMaterial({
      color: '#ff7b3b',
      transparent: true,
      opacity: 0,
      depthTest: true,
      depthWrite: false,
      side: THREE.DoubleSide,
    });
    const rightBtn = new THREE.Mesh(makeBtnPatch(-1), this.rightBtnMat);
    rightBtn.renderOrder = 5;
    rightBtn.visible = false;

    this.mouseLabelLMat = new THREE.MeshBasicMaterial({
      map: makeMouseLabelTexture('L', true),
      transparent: true,
      depthTest: true,
      depthWrite: false,
    });
    this.mouseLabelRMat = new THREE.MeshBasicMaterial({
      map: makeMouseLabelTexture('R', true),
      transparent: true,
      depthTest: true,
      depthWrite: false,
    });
    const lblGeo = new THREE.PlaneGeometry(0.068, 0.068);
    lblGeo.rotateX(-Math.PI / 2);
    const lblL = new THREE.Mesh(lblGeo, this.mouseLabelLMat);
    lblL.position.set(MW * 0.42, onTop(MW * 0.42, ML * 0.48) + 0.009, ML * 0.48);
    lblL.renderOrder = 8;
    const lblR = new THREE.Mesh(lblGeo, this.mouseLabelRMat);
    lblR.position.set(-MW * 0.42, onTop(-MW * 0.42, ML * 0.48) + 0.009, ML * 0.48);
    lblR.renderOrder = 8;

    this.mouseShape.add(
      mouseShadow,
      baseSkirt,
      dome,
      leftBtn,
      rightBtn,
      seamsMesh,
      wheel,
      lblL,
      lblR,
    );
    this.mouseShape.userData.leftBtn = leftBtn;
    this.mouseShape.userData.rightBtn = rightBtn;
    this.deskGroup.add(this.mouseGroup);

    scene.add(this.deskGroup);
    this.rebuildBoard('65', 'cat');
  }

  public setAccentColors(primaryHex: string, rightHex: string): void {
    this.hitColor.set(primaryHex);
    this.rightBtnColor.set(rightHex);
    this.leftBtnMat.color.set(primaryHex);
    this.rightBtnMat.color.set(rightHex);
  }

  public setStyle(style: '3d' | '2d'): void {
    const is3D = style === '3d';
    this.mouseBodyMat.vertexColors = is3D;
    this.mouseBodyMat.color.set(is3D ? '#ffffff' : '#f4f4f0');
    this.mouseBodyMat.needsUpdate = true;
  }

  public rebuildBoard(layoutId: KeyboardLayoutId, orientation: KeyboardOrientation): void {
    // Skip redundant rebuild if nothing changed and meshes already exist
    if (this.sideInstanced && this.layoutId === layoutId && this.orientation === orientation) {
      return;
    }

    this.layoutId = layoutId;
    this.orientation = orientation;
    this.keys.clear();
    this.activeKeys.clear();

    while (this.boardGroup.children.length > 0) {
      this.boardGroup.remove(this.boardGroup.children[0]);
    }
    for (const d of this.boardDisposables) {
      d.dispose();
    }
    this.boardDisposables.length = 0;

    const layout = getKeyboardLayout(layoutId);
    const is108 = layoutId === '108';
    const isCatFacing = orientation === 'cat';

    // Sync mouse orientation (180° cat-facing vs 0° viewer-facing)
    this.mouseShape.rotation.y = isCatFacing ? -0.10 : Math.PI - 0.10;
    this.mouseLabelLMat.map = makeMouseLabelTexture('L', isCatFacing);
    this.mouseLabelRMat.map = makeMouseLabelTexture('R', isCatFacing);
    this.mouseLabelLMat.needsUpdate = true;
    this.mouseLabelRMat.needsUpdate = true;

    const U = is108 ? 0.122 : 0.150;
    const CAP = U * 0.87;
    const W = layout.totalUnits * U + 0.14;
    const D = layout.totalRows * U + 0.14;
    this.boardWidth = W;
    this.boardH = 0.112;
    this.capH = is108 ? 0.048 : 0.056;

    this.boardGroup.position.set(0.24, 0.015, 1.08);
    this.mouseHome.set(is108 ? -1.46 : -1.16, 0.015, 0.96);

    // Soft grounded desk shadow under keyboard
    const kbShadowGeo = new THREE.PlaneGeometry(W * 1.18, D * 1.38);
    const kbShadow = new THREE.Mesh(kbShadowGeo, this.kbShadowMat);
    kbShadow.rotation.x = -Math.PI / 2;
    kbShadow.position.set(0, 0.002, 0.025);
    this.boardDisposables.push(kbShadowGeo);

    // Merge baseChassis + topBezel + plate into 1 single vertex-colored draw call!
    const baseGeo = makeRoundedSlabGeo(W, D, this.boardH, 0.068, 0);
    const bezelGeo = makeRoundedSlabGeo(W - 0.014, D - 0.014, 0.014, 0.060, this.boardH - 0.008);
    const plateGeo = makeRoundedSlabGeo(W - 0.062, D - 0.062, 0.008, 0.044, this.boardH + 0.004);
    const chassisGeo = mergeStaticGeometries([
      { geo: baseGeo, color: '#202229' },
      { geo: bezelGeo, color: '#2c2f38' },
      { geo: plateGeo, color: '#181a20' },
    ]);
    baseGeo.dispose();
    bezelGeo.dispose();
    plateGeo.dispose();
    const chassisMesh = new THREE.Mesh(chassisGeo, this.chassisMat);
    this.boardDisposables.push(chassisGeo);
    this.boardGroup.add(kbShadow, chassisMesh);

    // Single Texture Atlas + 2 InstancedMeshes for ALL 67 or 108 keycaps (2 draw calls total!)
    const keyCount = layout.keys.length;
    const atlas = getKeycapAtlas(layoutId, layout.keys, isCatFacing);

    const sideInst = new THREE.InstancedMesh(this.capSideGeo, this.capSideMat, keyCount);
    sideInst.frustumCulled = false;

    const topGeo = this.capTopBaseGeo.clone();
    const uvRects = new Float32Array(keyCount * 4);
    const tints = new Float32Array(keyCount * 3);
    const uvAttr = new THREE.InstancedBufferAttribute(uvRects, 4);
    const tintAttr = new THREE.InstancedBufferAttribute(tints, 3);
    tintAttr.setUsage(THREE.DynamicDrawUsage);
    topGeo.setAttribute('aUvRect', uvAttr);
    topGeo.setAttribute('aTint', tintAttr);

    const topMat = createInstancedKeyTopMaterial(atlas.tex);
    const topInst = new THREE.InstancedMesh(topGeo, topMat, keyCount);
    topInst.frustumCulled = false;
    this.boardDisposables.push(topGeo, topMat);

    const baseY = this.boardH + 0.008;
    for (let i = 0; i < keyCount; i++) {
      const spec = layout.keys[i];
      const rY = spec.rowY ?? spec.row;
      const rSpan = spec.rowSpan ?? 1;
      const uCenter = spec.uStart + spec.uWidth * 0.5;
      const rCenter = rY + rSpan * 0.5;

      const screenU = isCatFacing ? layout.totalUnits - uCenter : uCenter;
      const screenR = isCatFacing ? layout.totalRows - rCenter : rCenter;

      const x = (screenU - layout.totalUnits * 0.5) * U;
      const z = (screenR - layout.totalRows * 0.5) * U;
      const scaleX = Math.max(0.06, spec.uWidth * U - (U - CAP));
      const scaleY = this.capH;
      const scaleZ = Math.max(0.06, rSpan * U - (U - CAP));

      this.tmpPos.set(x, baseY, z);
      this.tmpScale.set(scaleX, scaleY, scaleZ);
      this.tmpMat.compose(this.tmpPos, this.tmpQuat, this.tmpScale);
      sideInst.setMatrixAt(i, this.tmpMat);
      topInst.setMatrixAt(i, this.tmpMat);

      const accent: 'base' | 'mod' | 'orange' =
        spec.accent === 'orange' ? 'orange' : spec.accent === 'mod' ? 'mod' : 'base';
      sideInst.setColorAt(
        i,
        accent === 'orange'
          ? this.sideColorOrange
          : accent === 'mod'
            ? this.sideColorMod
            : this.sideColorBase,
      );

      const legendText = spec.id === 'space' ? '' : spec.id === 'esc' ? 'Esc' : spec.label;
      const rect = atlas.rects.get(`${legendText}|${accent}`) ?? [0, 0, 0.1, 0.1];
      uvRects[i * 4] = rect[0];
      uvRects[i * 4 + 1] = rect[1];
      uvRects[i * 4 + 2] = rect[2];
      uvRects[i * 4 + 3] = rect[3];

      tints[i * 3] = 1;
      tints[i * 3 + 1] = 1;
      tints[i * 3 + 2] = 1;

      this.keys.set(spec.id, {
        spec,
        index: i,
        x,
        z,
        scaleX,
        scaleY,
        scaleZ,
        down: false,
        depth: 0,
        glow: 0,
      });
    }

    sideInst.instanceMatrix.needsUpdate = true;
    if (sideInst.instanceColor) sideInst.instanceColor.needsUpdate = true;
    topInst.instanceMatrix.needsUpdate = true;
    this.sideInstanced = sideInst;
    this.topInstanced = topInst;
    this.topTintAttr = tintAttr;
    this.topTintArray = tints;
    this.boardGroup.add(sideInst, topInst);

    // Privacy dark frosted shield over the 3D keyboard
    const privGeo = new THREE.PlaneGeometry(W - 0.08, D - 0.08);
    this.boardDisposables.push(privGeo);
    this.privacyPlate = new THREE.Mesh(privGeo, this.privacyMat);
    this.privacyPlate.rotation.x = -Math.PI / 2;
    this.privacyPlate.position.set(0, this.boardH + this.capH + 0.025, 0);
    this.privacyPlate.renderOrder = 30;
    this.privacyPlate.visible = false;
    this.boardGroup.add(this.privacyPlate);

    this.deskGroup.updateMatrixWorld(false);
    this.boardGroup.updateMatrixWorld(false);
  }

  public getLayoutId(): KeyboardLayoutId {
    return this.layoutId;
  }

  public getOrientation(): KeyboardOrientation {
    return this.orientation;
  }

  public getBoardWidth(): number {
    return this.boardWidth;
  }

  public resolveKey(vk: number): KeyEntry3D | null {
    const spec = resolveKeyByVk(vk, this.layoutId);
    if (!spec) return null;
    return this.keys.get(spec.id) || null;
  }

  public getKeyById(id: string): KeyEntry3D | null {
    return this.keys.get(id) || null;
  }

  public setKeyDown(id: string, down: boolean): void {
    const k = this.keys.get(id);
    if (!k) return;
    if (k.down !== down) {
      k.down = down;
      this.activeKeys.add(k);
    } else if (down) {
      this.activeKeys.add(k);
    }
  }

  public releaseAllKeys(): void {
    for (const k of this.keys.values()) {
      if (k.down) {
        k.down = false;
        this.activeKeys.add(k);
      }
    }
  }

  public keyTopWorld(id: string, out = new THREE.Vector3()): THREE.Vector3 {
    const k = this.keys.get(id);
    this.deskGroup.updateMatrixWorld(false);
    this.boardGroup.updateMatrixWorld(false);
    if (!k) return out.set(0.24, this.boardH + this.capH, 1.08);
    return out
      .set(k.x, this.boardH + 0.008 + this.capH - k.depth, k.z)
      .applyMatrix4(this.boardGroup.matrixWorld);
  }

  public isRightEdgeTailKey(id: string): boolean {
    const k = this.keys.get(id);
    if (!k) return false;
    return k.x >= this.boardWidth * 0.22;
  }

  public setMouseNormalized(nx: number, ny: number): void {
    this.mouseTarget.set((nx - 0.5) * 0.22, 0, (ny - 0.5) * 0.14);
  }

  public pulseScrollWheel(): void {
    this.wheelGlow = 1;
  }

  public mouseTopWorld(out = new THREE.Vector3()): THREE.Vector3 {
    this.deskGroup.updateMatrixWorld(false);
    this.mouseGroup.updateMatrixWorld(false);
    this.mouseShape.updateMatrixWorld(false);
    return out.set(0, 0.115, -0.05).applyMatrix4(this.mouseShape.matrixWorld);
  }

  public update(
    dt: number,
    leftDown: boolean,
    rightDown: boolean,
    deskHopY: number,
    privacyAlpha: number,
  ): void {
    if (deskHopY !== this.lastDeskHopY) {
      this.deskGroup.position.y = deskHopY;
      this.lastDeskHopY = deskHopY;
    }

    // Only animate keys that are actually pressed or rebounding/fading (0-3 keys instead of 67-108!)
    if (this.activeKeys.size > 0 && this.sideInstanced && this.topInstanced && this.topTintAttr && this.topTintArray) {
      let matrixDirty = false;
      let tintDirty = false;
      const baseY = this.boardH + 0.008;
      const kDownDepth = approach(62, dt);
      const kUpDepth = approach(24, dt);
      const kDownGlow = approach(44, dt);
      const kUpGlow = approach(6.5, dt);

      for (const k of this.activeKeys) {
        const targetDepth = k.down ? 0.032 : 0;
        k.depth += (targetDepth - k.depth) * (k.down ? kDownDepth : kUpDepth);
        k.glow += ((k.down ? 1 : 0) - k.glow) * (k.down ? kDownGlow : kUpGlow);

        const settled = !k.down && k.depth < 0.00025 && k.glow < 0.002;
        if (settled) {
          k.depth = 0;
          k.glow = 0;
          this.activeKeys.delete(k);
        }

        this.tmpPos.set(k.x, baseY - k.depth, k.z);
        this.tmpScale.set(k.scaleX, k.scaleY, k.scaleZ);
        this.tmpMat.compose(this.tmpPos, this.tmpQuat, this.tmpScale);
        this.sideInstanced.setMatrixAt(k.index, this.tmpMat);
        this.topInstanced.setMatrixAt(k.index, this.tmpMat);
        matrixDirty = true;

        this.tmpCol.copy(this.whiteColor).lerp(this.hitColor, k.glow * 0.90);
        const off = k.index * 3;
        this.topTintArray[off] = this.tmpCol.r;
        this.topTintArray[off + 1] = this.tmpCol.g;
        this.topTintArray[off + 2] = this.tmpCol.b;
        tintDirty = true;
      }

      if (matrixDirty) {
        this.sideInstanced.instanceMatrix.needsUpdate = true;
        this.topInstanced.instanceMatrix.needsUpdate = true;
      }
      if (tintDirty) {
        this.topTintAttr.needsUpdate = true;
      }
    }

    if (this.privacyPlate) {
      this.privacyPlate.visible = privacyAlpha > 0.02;
      this.privacyMat.opacity = privacyAlpha;
    }

    this.mouseTarget.multiplyScalar(1 - approach(1.6, dt));
    this.mouseOffset.copy(this.mouseFollow.step(this.mouseTarget, dt));
    this.mouseGroup.position.copy(this.mouseHome).add(this.mouseOffset);

    this.leftPress += ((leftDown ? 1 : 0) - this.leftPress) * approach(32, dt);
    this.rightPress += ((rightDown ? 1 : 0) - this.rightPress) * approach(32, dt);
    if (this.leftPress < 0.002 && !leftDown) this.leftPress = 0;
    if (this.rightPress < 0.002 && !rightDown) this.rightPress = 0;
    this.wheelGlow = Math.max(0, this.wheelGlow - dt * 3.8);

    // Hide transparent L/R click highlight patches completely when not pressed (saves 2 alpha draw calls!)
    const leftBtn = this.mouseShape.userData.leftBtn as THREE.Mesh | undefined;
    const rightBtn = this.mouseShape.userData.rightBtn as THREE.Mesh | undefined;
    this.leftBtnMat.opacity = 0.88 * this.leftPress;
    this.rightBtnMat.opacity = 0.88 * this.rightPress;
    if (leftBtn) leftBtn.visible = this.leftPress > 0.005;
    if (rightBtn) rightBtn.visible = this.rightPress > 0.005;

    this.wheelMat.color.set('#555966').lerp(this.hitColor, this.wheelGlow);

    const anyPress = Math.max(this.leftPress, this.rightPress);
    this.mouseShape.scale.set(1 + 0.03 * anyPress, 1 - 0.10 * anyPress, 1);
  }
}
