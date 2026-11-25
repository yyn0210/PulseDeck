/**
 * Ported from KuroCat (https://github.com/qianyiai/kurocat), v5.1.0.
 * Copyright (c) 2026 KuroCat Contributors, MIT License.
 * See src-tauri/resources/licenses/KUROCAT-MIT.txt.
 */
import * as THREE from 'three';
import type { CatSkinConfig, CatSpot } from './catSkins';

const MAX_CUSTOM_SPOTS = 8;

export type RenderStyle3D = '3d' | '2d';

/**
 * Push shoulder vertices slightly away from the camera along view ray
 * so sleek 3D arms emerge seamlessly from the continuous plush torso with zero seam.
 */
export const ARM_ROOT_PUSH_GLSL = `
  float rootK = 1.0 - smoothstep(0.0, 0.28, along);
  float rootPush = 0.34 * rootK * rootK;
  mv.xyz += normalize(mv.xyz) * rootPush;
`;

export function withArmRootPush<T extends THREE.Material>(mat: T): T {
  mat.onBeforeCompile = (s) => {
    s.vertexShader = 'attribute float along;\n' + s.vertexShader;
    s.vertexShader = s.vertexShader.replace(
      '#include <project_vertex>',
      `vec4 mvPosition = vec4(transformed, 1.0);
      #ifdef USE_INSTANCING
        mvPosition = instanceMatrix * mvPosition;
      #endif
      mvPosition = modelViewMatrix * mvPosition;
      {
        vec4 mv = mvPosition;
        ${ARM_ROOT_PUSH_GLSL}
        mvPosition = mv;
      }
      gl_Position = projectionMatrix * mvPosition;`,
    );
  };
  return mat;
}

export interface FurPalette3D {
  id: string;
  fur: string;
  light: string;
  dark: string;
  patchA: string;
  patchB: string;
  point: string;
  earOuter: string | null;
  tailBase: string | null;
  inner: string;
  stripes: number;
  muzzle: number;
  bib: number;
  calico: number;
  tuxedo: number;
  points: number;
  innerAmt: number;
  shade: number;
  paw: string;
  pawTuckLine: string | null;
  toe: string;
  whisker: string;
  whiskerOpacity: number;
  nose: string | null;
  mouth: string | null;
  eyeTop: string;
  eyeBottom: string;
  pupil: string;
  lid: string;
  squintLine: string;
  eyeCatchlight: boolean;
  isDarkFur: boolean;
  lightContour: string | null;
  accent: string;
  accentRight: string;
  accentSide: string;
}

export function resolveFurPalette3D(skin: CatSkinConfig, style: RenderStyle3D): FurPalette3D {
  const is3D = style === '3d';
  const accent = skin.accentColor || '#d4f044';
  const accentSide = skin.accentSideColor || '#92ba12';
  const isWarmAccent = /^#(f[a-f0-9][89a-c]|ff[789a-c]|e[89a-f][789a])/i.test(accent);
  const accentRight = isWarmAccent ? '#58c7ff' : '#ff7b3b';

  switch (skin.id) {
    case 'black':
      return {
        id: 'black',
        // Pure deep velvety obsidian black matching original KuroCat (#141314) & app icon
        fur: '#0d0d0f',
        light: '#0d0d0f',
        dark: '#0d0d0f',
        patchA: '#0d0d0f',
        patchB: '#0d0d0f',
        point: '#0d0d0f',
        earOuter: null,
        tailBase: null,
        inner: '#1a1b22',
        stripes: 0,
        muzzle: 0,
        bib: 0,
        calico: 0,
        tuxedo: 0,
        points: 0,
        // Subtle dark slate inner ear scoop in 3D mode (like kurocat-icon.png), pure black in 2D flat mode
        innerAmt: is3D ? 0.42 : 0,
        shade: is3D ? 1.0 : 0,
        paw: '#0e0e11',
        pawTuckLine: null,
        toe: '#23252d',
        whisker: '#d4d8e0',
        whiskerOpacity: 0.56,
        nose: null,
        mouth: null,
        eyeTop: '#d7f346',
        eyeBottom: '#bde62c',
        pupil: '#0c0d0f',
        lid: '#d4f044',
        squintLine: '#d4f044',
        eyeCatchlight: false,
        isDarkFur: true,
        lightContour: null,
        accent,
        accentRight,
        accentSide,
      };
    case 'ragdoll':
      return {
        id: 'ragdoll',
        fur: '#f7f3ea',
        light: '#fdfbf7',
        dark: '#7e6c5e',
        patchA: '#f7f3ea',
        patchB: '#f7f3ea',
        point: '#8a786a',
        earOuter: null,
        tailBase: null,
        inner: '#dec4be',
        stripes: 0,
        muzzle: 1,
        bib: 1,
        calico: 0,
        tuxedo: 1,
        points: 1,
        innerAmt: 0.92,
        shade: is3D ? 0.85 : 0,
        paw: '#fdfbf7',
        pawTuckLine: 'rgba(116, 102, 90, 0.28)',
        toe: '#d4c8bc',
        whisker: '#7e6f64',
        whiskerOpacity: 0.56,
        nose: '#ee8e9e',
        mouth: '#5c524c',
        eyeTop: '#2c5594',
        eyeBottom: '#a3d2f2',
        pupil: '#101624',
        lid: '#222736',
        squintLine: '#222736',
        eyeCatchlight: true,
        isDarkFur: false,
        lightContour: '#685b50',
        accent,
        accentRight,
        accentSide,
      };
    case 'orange-tabby':
      return {
        id: 'orange-tabby',
        fur: '#ef9e4b',
        light: '#fff4e0',
        dark: '#b75816',
        patchA: '#ef9e4b',
        patchB: '#ef9e4b',
        point: '#ef9e4b',
        earOuter: null,
        tailBase: null,
        inner: '#f4b8ae',
        stripes: 1,
        muzzle: 1,
        bib: 1,
        calico: 0,
        tuxedo: 0,
        points: 0,
        innerAmt: 0.95,
        shade: is3D ? 0.88 : 0,
        paw: '#fff5e4',
        pawTuckLine: 'rgba(155, 84, 30, 0.26)',
        toe: '#d8ad84',
        whisker: '#fff9ee',
        whiskerOpacity: 0.68,
        nose: '#ee8d98',
        mouth: '#86502d',
        eyeTop: '#b96c14',
        eyeBottom: '#ffd66b',
        pupil: '#15120e',
        lid: '#6b3b15',
        squintLine: '#6b3b15',
        eyeCatchlight: true,
        isDarkFur: false,
        lightContour: '#a65c20',
        accent,
        accentRight,
        accentSide,
      };
    case 'calico':
      return {
        id: 'calico',
        fur: '#fcfaf4',
        light: '#fcfaf4',
        dark: '#35302d',
        patchA: '#ea9445',
        patchB: '#262324',
        point: '#fcfaf4',
        earOuter: null,
        tailBase: null,
        inner: '#f2bcc1',
        stripes: 0,
        muzzle: 0,
        bib: 1,
        calico: 1,
        tuxedo: 0,
        points: 0,
        innerAmt: 0.95,
        shade: is3D ? 0.82 : 0,
        paw: '#fcfaf4',
        pawTuckLine: 'rgba(110, 98, 90, 0.28)',
        toe: '#d8c5be',
        whisker: '#85786f',
        whiskerOpacity: 0.56,
        nose: '#ef95a2',
        mouth: '#6b5e57',
        eyeTop: '#568d3a',
        eyeBottom: '#cbe87a',
        pupil: '#101412',
        lid: '#403632',
        squintLine: '#403632',
        eyeCatchlight: true,
        isDarkFur: false,
        lightContour: '#7c7067',
        accent,
        accentRight,
        accentSide,
      };
    case 'tuxedo':
      return {
        id: 'tuxedo',
        fur: '#191a1f',
        light: '#faf9f5',
        dark: '#191a1f',
        patchA: '#191a1f',
        patchB: '#191a1f',
        point: '#191a1f',
        earOuter: null,
        tailBase: null,
        inner: '#dfb1b8',
        stripes: 0,
        muzzle: 1,
        bib: 1,
        calico: 0,
        tuxedo: 1,
        points: 0,
        innerAmt: 0.82,
        shade: is3D ? 0.96 : 0,
        paw: '#faf9f5',
        pawTuckLine: 'rgba(92, 96, 108, 0.28)',
        toe: '#c7c5bd',
        whisker: '#e6e9f0',
        whiskerOpacity: 0.64,
        nose: '#ee9aa6',
        mouth: '#6b6f7a',
        eyeTop: '#6ab83d',
        eyeBottom: '#daf66a',
        pupil: '#0d1014',
        lid: '#c8f052',
        squintLine: '#c8f052',
        eyeCatchlight: true,
        isDarkFur: true,
        lightContour: null,
        accent,
        accentRight,
        accentSide,
      };
    case 'siamese':
      return {
        id: 'siamese',
        fur: '#efe3cf',
        light: '#f7efe0',
        dark: '#3b2c23',
        patchA: '#efe3cf',
        patchB: '#efe3cf',
        point: '#3d2c22',
        earOuter: null,
        tailBase: null,
        inner: '#7f675b',
        stripes: 0,
        muzzle: 0,
        bib: 0,
        calico: 0,
        tuxedo: 0,
        points: 1,
        innerAmt: 0.62,
        shade: is3D ? 0.86 : 0,
        paw: '#3d2c22',
        pawTuckLine: 'rgba(180, 158, 140, 0.30)',
        toe: '#695244',
        whisker: '#6c5a4f',
        whiskerOpacity: 0.56,
        nose: '#2d201a',
        mouth: '#5b453a',
        eyeTop: '#2462be',
        eyeBottom: '#8ed6ff',
        pupil: '#0c1220',
        lid: '#2c2019',
        squintLine: '#2c2019',
        eyeCatchlight: true,
        isDarkFur: false,
        lightContour: '#6d5848',
        accent,
        accentRight,
        accentSide,
      };
    default: {
      const pat = skin.pattern || 'solid';
      const c = new THREE.Color(skin.furColor || '#f2a452');
      const isDark = 0.299 * c.r + 0.587 * c.g + 0.114 * c.b < 0.16;
      return {
        id: skin.id || 'custom',
        fur: skin.furColor || '#f2a452',
        light: skin.bellyBibColor || '#faf8f2',
        dark: skin.stripeColor || skin.furSecondaryColor || skin.furColor || '#664422',
        patchA: skin.maskColor || '#ea9445',
        patchB: skin.calicoPatchColor || '#2e2927',
        point: skin.maskColor || skin.earOuterColor || '#5a4436',
        earOuter:
          skin.earOuterColor && skin.earOuterColor.toLowerCase() !== (skin.furColor || '').toLowerCase()
            ? skin.earOuterColor
            : null,
        tailBase:
          skin.tailColor && skin.tailColor.toLowerCase() !== (skin.furColor || '').toLowerCase()
            ? skin.tailColor
            : null,
        inner: skin.earInnerColor || '#f0bcc0',
        stripes: pat === 'tabby' ? 1 : 0,
        muzzle: pat === 'tabby' || pat === 'bicolor' || pat === 'tuxedo' ? 1 : 0,
        bib: skin.bellyBibColor || pat === 'bicolor' || pat === 'tuxedo' ? 1 : 0,
        calico: pat === 'calico' ? 1 : 0,
        tuxedo: pat === 'tuxedo' || pat === 'bicolor' ? 1 : 0,
        points: pat === 'siamese' || pat === 'bicolor' ? 1 : 0,
        innerAmt: skin.earInnerColor ? 0.9 : 0,
        shade: is3D ? 0.86 : 0,
        paw: skin.pawColor || skin.furColor || '#faf8f2',
        pawTuckLine: isDark ? 'rgba(182, 188, 198, 0.34)' : 'rgba(110, 98, 90, 0.28)',
        toe: '#bdb5aa',
        whisker: isDark ? '#d8dce4' : '#7f7268',
        whiskerOpacity: 0.6,
        nose: skin.noseColor || '#ee8e9e',
        mouth: skin.mouthColor || '#5e5550',
        eyeTop: skin.eyeColorTop || '#4a88d8',
        eyeBottom: skin.eyeColorBottom || '#a3d2f2',
        pupil: skin.pupilColor || '#10141c',
        lid: skin.squintColor || '#282c36',
        squintLine: skin.squintColor || '#282c36',
        eyeCatchlight: skin.eyeCatchlight !== false,
        isDarkFur: isDark,
        lightContour: isDark ? null : skin.outlineColor || '#6b5e54',
        accent,
        accentRight,
        accentSide,
      };
    }
  }
}

export type FurPart = 'CAT_BODY' | 'EAR' | 'TAIL' | 'ARM' | 'PAW';

/**
 * Unified rest-space GLSL fur shader crafted specifically for KuroCat's
 * continuous one-piece mochi-pear silhouette & soft designer-vinyl studio lighting (NO KeyCat white sticker rim!).
 */
export class FurShaderSystem {
  public readonly uniforms: Record<string, THREE.IUniform>;

  constructor() {
    this.uniforms = {
      uFur: { value: new THREE.Color('#141315') },
      uLight: { value: new THREE.Color('#f7f5ee') },
      uDark: { value: new THREE.Color('#141315') },
      uPatchA: { value: new THREE.Color('#141315') },
      uPatchB: { value: new THREE.Color('#141315') },
      uPoint: { value: new THREE.Color('#141315') },
      uInner: { value: new THREE.Color('#272931') },
      uStripes: { value: 0 },
      uMuzzle: { value: 0 },
      uBib: { value: 0 },
      uCalico: { value: 0 },
      uTuxedo: { value: 0 },
      uPoints: { value: 0 },
      uInnerAmt: { value: 0.5 },
      uShade: { value: 1.0 },
      uIsDarkFur: { value: 1.0 },
      uPawColor: { value: new THREE.Color('#151417') },
      uEarOuter: { value: new THREE.Color('#141315') },
      uEarOuterAmt: { value: 0 },
      uTailBase: { value: new THREE.Color('#141315') },
      uTailBaseAmt: { value: 0 },
      uSpotCount: { value: 0 },
      uSpotPos: { value: Array.from({ length: MAX_CUSTOM_SPOTS }, () => new THREE.Vector4()) },
      uSpotCol: { value: Array.from({ length: MAX_CUSTOM_SPOTS }, () => new THREE.Vector4()) },
      uSpotMeta: { value: Array.from({ length: MAX_CUSTOM_SPOTS }, () => new THREE.Vector4()) },
      // GPU body+head deformation uniforms (eliminates 4,745-vert CPU deformation + computeVertexNormals + PCIe upload every frame!)
      uBend: { value: new THREE.Vector4(0, 0, 0, 0) }, // (x, z, squash, bellySwell)
      uHeadRot: { value: new THREE.Vector3(-0.04, 0, 0) }, // (pitch, yaw, roll) Euler YXZ
    };
  }

  public setBodyDeform(
    bx: number,
    bz: number,
    squash: number,
    bellySwell: number,
    pitch: number,
    yaw: number,
    roll: number,
  ): void {
    (this.uniforms.uBend.value as THREE.Vector4).set(bx, bz, squash, bellySwell);
    (this.uniforms.uHeadRot.value as THREE.Vector3).set(pitch, yaw, roll);
  }

  public createMaterial(part: FurPart, side = 0): THREE.ShaderMaterial {
    const defines: Record<string, number> = { [part]: 1 };
    if (side < 0) defines.LEFT = 1;

    const vertexShader = `
      attribute vec3 rest;
      attribute float along;
      #ifdef CAT_BODY
        uniform vec4 uBend;
        uniform vec3 uHeadRot;
      #endif
      varying vec3 vRest;
      varying vec3 vNormal;
      varying vec3 vViewDir;
      varying float vAlong;
      void main() {
        #if defined(CAT_BODY) || defined(EAR)
          vRest = rest;
        #else
          vRest = position;
        #endif
        vAlong = along;

        vec3 localPos = position;
        vec3 localNrm = normal;

        #ifdef CAT_BODY
          const float HEAD_CY = 1.56;
          const float BODY_H = 1.98;
          const float W_HEAD = 0.8226517; // pow(smoothstep(0.14, 1.75, 1.56), 1.45)
          const float S_HEAD = 0.8588995; // smoothstep(0.18, 1.98, 1.56)

          float x0 = position.x;
          float y0 = position.y;
          float z0 = position.z;

          float headW = smoothstep(1.14, 1.44, y0);
          float sSpine = smoothstep(0.14, 1.75, y0);
          float wTorso = pow(max(0.0, sSpine), 1.45);
          float w = mix(wTorso, W_HEAD, headW);
          float sw = smoothstep(0.18, BODY_H, y0);
          float headSinkY = HEAD_CY * (1.0 - uBend.z * S_HEAD);
          float swell = 1.0 + uBend.w * smoothstep(1.30, 0.65, y0) * smoothstep(0.0, 0.25, y0);

          vec3 pDef = vec3(x0 * swell, y0, z0 * swell);

          if (headW > 0.001) {
            float ax = uHeadRot.x * headW;
            float ay = uHeadRot.y * headW;
            float az = uHeadRot.z * headW;
            float cx = cos(ax), sx = sin(ax);
            float cy = cos(ay), sy = sin(ay);
            float cz = cos(az), sz = sin(az);
            // Exact column-major R = Ry(ay) * Rx(ax) * Rz(az) matching THREE.Euler(..., 'YXZ')
            mat3 rot = mat3(
              cy * cz + sy * sx * sz,   cx * sz,   -sy * cz + cy * sx * sz,
              -cy * sz + sy * sx * cz,  cx * cz,    sy * sz + cy * sx * cz,
              sy * cx,                 -sx,         cy * cx
            );
            vec3 rel = vec3(pDef.x, pDef.y - HEAD_CY, pDef.z);
            rel = rot * rel;
            pDef = vec3(rel.x, rel.y + HEAD_CY, rel.z);
            localNrm = rot * localNrm;
          }

          // Subtle analytical normal tilt for torso spine shear + squash
          float torsoFactor = 1.0 - headW;
          if (torsoFactor > 0.001) {
            float dwApprox = 0.68 * torsoFactor;
            localNrm.y -= (uBend.x * localNrm.x + uBend.y * localNrm.z) * dwApprox;
            localNrm.y /= max(0.75, 1.0 - uBend.z * sw * torsoFactor);
          }

          float torsoY = pDef.y * (1.0 - uBend.z * sw);
          float headRigidY = pDef.y - HEAD_CY + headSinkY;
          localPos = vec3(pDef.x + uBend.x * w, mix(torsoY, headRigidY, headW), pDef.z + uBend.y * w);
        #endif

        vNormal = normalize(normalMatrix * localNrm);
        vec4 mv = modelViewMatrix * vec4(localPos, 1.0);
        #ifdef ARM
          ${ARM_ROOT_PUSH_GLSL}
        #endif
        vViewDir = normalize(-mv.xyz);
        gl_Position = projectionMatrix * mv;
      }
    `;

    const fragmentShader = `
      uniform vec3 uFur, uLight, uDark, uPatchA, uPatchB, uPoint, uInner, uPawColor, uEarOuter, uTailBase;
      uniform float uStripes, uMuzzle, uBib, uCalico, uTuxedo, uPoints, uInnerAmt, uShade, uIsDarkFur, uEarOuterAmt, uTailBaseAmt;
      uniform int uSpotCount;
      uniform vec4 uSpotPos[${MAX_CUSTOM_SPOTS}];
      uniform vec4 uSpotCol[${MAX_CUSTOM_SPOTS}];
      uniform vec4 uSpotMeta[${MAX_CUSTOM_SPOTS}];
      varying vec3 vRest;
      varying vec3 vNormal;
      varying vec3 vViewDir;
      varying float vAlong;

      float blob(vec2 p, vec2 c, vec2 r) {
        return 1.0 - smoothstep(0.92, 1.0, length((p - c) / r));
      }
      float blob3(vec3 p, vec3 c, float r) {
        return 1.0 - smoothstep(r * 0.92, r, length(p - c));
      }
      float stroke(vec2 p, vec2 a, vec2 b, float w, float taper) {
        vec2 ab = b - a;
        float h = clamp(dot(p - a, ab) / dot(ab, ab), 0.0, 1.0);
        float r = w * mix(1.0, taper, h);
        return 1.0 - smoothstep(r * 0.84, r, length(p - a - ab * h));
      }
      float rings(float x, float n, float fill) {
        float f = fract(x * n);
        float d = abs(f - 0.5);
        return 1.0 - smoothstep(fill * 0.5 - 0.03, fill * 0.5, d);
      }

      void main() {
        vec3 col = uFur;
        vec3 p = vRest;
        float light = 0.0, dark = 0.0, inner = 0.0, point = 0.0;
        float pa = 0.0, pb = 0.0;

        #ifdef CAT_BODY
          // Unified one-piece KuroCat body+head rest coordinates (y: 0.0 desk -> 1.98 crown, head center at y=1.56)
          vec3 hp = vec3(p.x / 0.62, (p.y - 1.56) / 0.44, p.z / 0.50);
          float headZone = smoothstep(1.22, 1.42, p.y);
          float torsoZone = 1.0 - smoothstep(1.18, 1.42, p.y);

          // Uniform-coherent early skips: solid Black Cat skips 100% of procedural pattern ALU ops!
          float hasHeadMark = uPoints + uMuzzle + uTuxedo + uStripes;
          if (hasHeadMark > 0.01 && hp.z > -0.10) {
            vec2 q = hp.xy;
            if (uPoints > 0.01) {
              // Colourpoint face mask (Siamese & Ragdoll upper cap/temples)
              float maskDist = length((q - vec2(0.0, 0.08)) / vec2(0.95, 0.86));
              float facePoint = (1.0 - smoothstep(0.62, 1.04, maskDist)) * smoothstep(-0.35, 0.05, q.y) * smoothstep(-0.08, 0.25, hp.z);
              point = max(point, facePoint * headZone);
            }

            if (uMuzzle > 0.01 || uTuxedo > 0.01) {
              // Soft pale cat muzzle pads & chin
              float m = max(
                max(blob(q, vec2(-0.18, -0.34), vec2(0.28, 0.24)), blob(q, vec2(0.18, -0.34), vec2(0.28, 0.24))),
                blob(q, vec2(0.0, -0.54), vec2(0.32, 0.24))
              );
              light = max(light, m * uMuzzle * headZone);

              if (uTuxedo > 0.01) {
                // Wide fluffy inverted-V Ragdoll / Tuxedo white blaze flowing smoothly down into the chest bib!
                float blazePeak = (1.0 - smoothstep(0.0, 0.14, abs(q.x) - max(0.0, (0.34 - q.y) * 0.72)))
                                * smoothstep(-0.75, -0.25, q.y)
                                * (1.0 - smoothstep(0.26, 0.42, q.y))
                                * smoothstep(0.12, 0.36, hp.z);
                light = max(light, max(m, blazePeak) * uTuxedo * headZone);
              }
            }

            if (uStripes > 0.01) {
              // Tabby forehead "M" & cheek whiskers-line streaks
              float mk = stroke(q, vec2(0.0, 0.36), vec2(0.0, 0.82), 0.072, 0.52);
              mk = max(mk, stroke(vec2(abs(q.x), q.y), vec2(0.16, 0.34), vec2(0.30, 0.80), 0.062, 0.44));
              mk = max(mk, stroke(vec2(abs(q.x), q.y), vec2(0.66, 0.04), vec2(0.96, 0.14), 0.058, 0.42));
              mk = max(mk, stroke(vec2(abs(q.x), q.y), vec2(0.68, -0.18), vec2(0.96, -0.12), 0.052, 0.42));
              dark = max(dark, mk * uStripes * headZone * smoothstep(0.05, 0.30, hp.z));
            }
          }

          // 2) Seamless chest bib & lower torso markings (skip atan() entirely on solid breeds!)
          float bib = 0.0;
          if (uBib > 0.01 || uTuxedo > 0.01 || uStripes > 0.01 || uCalico > 0.01 || (uPoints > 0.5 && uTuxedo > 0.5)) {
            float ang = abs(atan(p.x, p.z));
            if (uBib > 0.01 || uTuxedo > 0.01 || uCalico > 0.01) {
              float bibW = mix(0.46, 0.74, smoothstep(0.45, 1.35, p.y));
              bib = (1.0 - smoothstep(bibW * 0.88, bibW, ang)) * (1.0 - smoothstep(1.30, 1.48, p.y));
              light = max(light, bib * max(uBib, uTuxedo));
            }

            // Ragdoll soft warm taupe flanks along outer thighs
            if (uPoints > 0.5 && uTuxedo > 0.5) {
              float flank = smoothstep(0.75, 1.35, ang) * smoothstep(0.08, 0.45, p.y) * (1.0 - smoothstep(0.85, 1.25, p.y));
              point = max(point, flank * 0.62);
            }

            if (uStripes > 0.01) {
              // Tabby continuous flank & back rings
              float wave = p.y * 0.88 + 0.05 * sin(ang * 3.0);
              float reach = smoothstep(0.55, 1.05, ang);
              float band = rings(wave, 5.2, 0.36 * reach) * step(0.18, p.y) * torsoZone;
              dark = max(dark, band * uStripes);
            }
          }

          if (uCalico > 0.01) {
            // Calico organic head + side flank patches wrapping seamlessly into 3D shoulders!
            pa = max(
              blob3(p, vec3(-0.38, 1.82, 0.14), 0.44),
              blob3(p, vec3(-0.54, 0.76, 0.08), 0.62) * (1.0 - bib * 0.85)
            );
            pb = max(
              blob3(p, vec3(0.44, 1.76, 0.06), 0.40),
              blob3(p, vec3(0.56, 0.72, 0.06), 0.62) * (1.0 - bib * 0.85)
            );
          }
        #endif

        #ifdef EAR
          point = smoothstep(0.02, 0.15, p.y);
          float rMax = mix(0.24, 0.0, clamp((p.y + 0.05) / 0.36, 0.0, 1.0));
          inner = (1.0 - smoothstep(0.46, 0.58, abs(p.x) / max(rMax, 0.001)))
                * smoothstep(0.04, 0.08, p.y)
                * (1.0 - smoothstep(0.22, 0.25, p.y))
                * smoothstep(-0.02, 0.04, p.z);
          #ifdef LEFT
            pa = 1.0; pb = 0.0;
          #else
            pa = 0.0; pb = 1.0;
          #endif
        #endif

        #ifdef TAIL
          dark = max(dark, rings(vAlong, 5.0, 0.40) * step(0.16, vAlong) * uStripes);
          dark = max(dark, smoothstep(0.88, 0.91, vAlong) * uStripes);
          pa = rings(vAlong + 0.14, 1.0, 0.32);
          pb = 1.0 - pa;
          point = smoothstep(0.0, 0.34, vAlong);
          light = max(light, smoothstep(0.85, 0.90, vAlong) * uTuxedo * (1.0 - uPoints));
        #endif

        #ifdef ARM
          dark = max(dark, rings(vAlong, 3.0, 0.34) * step(0.32, vAlong) * uStripes);
          // Calico: upper shoulder flows seamlessly out of left orange / right charcoal flank patch into snow-white kitten mittens!
          #ifdef LEFT
            pa = (1.0 - smoothstep(0.24, 0.48, vAlong));
          #else
            pb = (1.0 - smoothstep(0.24, 0.48, vAlong));
          #endif
          // Siamese (uPoints && !uTuxedo): smooth toasted-cream flank -> chocolate stockings;
          // Ragdoll (uPoints && uTuxedo): snow-white mitten arms + signature warm taupe oval spot on viewer's RIGHT forearm!
          if (uPoints > 0.5 && uTuxedo < 0.5) {
            point = smoothstep(0.20, 0.78, vAlong);
          } else if (uPoints > 0.5 && uTuxedo > 0.5) {
            light = smoothstep(0.18, 0.48, vAlong);
            #ifndef LEFT
              float spotBand = 1.0 - smoothstep(0.05, 0.14, abs(vAlong - 0.62));
              float frontFace = smoothstep(-0.15, 0.45, normalize(vNormal).z);
              point = spotBand * frontFace * 0.82;
              light *= (1.0 - point);
            #endif
          }
        #endif

        #ifdef PAW
          col = uPawColor;
        #else
          #ifdef EAR
            col = mix(col, uEarOuter, uEarOuterAmt * smoothstep(0.0, 0.14, p.y));
          #endif
          #ifdef TAIL
            col = mix(col, uTailBase, uTailBaseAmt * smoothstep(0.0, 0.24, vAlong));
          #endif
          col = mix(col, uPatchA, pa * uCalico);
          col = mix(col, uPatchB, pb * uCalico);
          col = mix(col, uPoint, point * uPoints);
          col = mix(col, uDark, dark);
          col = mix(col, uLight, light);
          col = mix(col, uInner, inner * uInnerAmt);

          // Full MCP custom spots/birthmarks support on head, body, leftArm, and rightArm!
          if (uSpotCount > 0) {
            for (int si = 0; si < ${MAX_CUSTOM_SPOTS}; si++) {
              if (si >= uSpotCount) break;
              vec4 spPos = uSpotPos[si];
              vec4 spCol = uSpotCol[si];
              vec4 spMeta = uSpotMeta[si];
              float partCode = spMeta.x;
              float rot = spMeta.y;
              float smask = 0.0;

              #ifdef CAT_BODY
                if (partCode < 0.5) {
                  // Head spot (matches 2D head spot coordinates: +x right, -y up in 3D)
                  vec2 d = vec2(hp.x - spPos.x, hp.y - spPos.y);
                  float ca = cos(rot), sa = sin(rot);
                  vec2 rd = vec2(d.x * ca - d.y * sa, d.x * sa + d.y * ca);
                  float dist = length(rd / max(spPos.zw, vec2(0.04)));
                  smask = (1.0 - smoothstep(0.84, 1.0, dist)) * headZone * smoothstep(-0.05, 0.25, hp.z);
                } else if (partCode < 1.5) {
                  // Body spot (front torso)
                  vec2 bp = vec2(p.x / 0.68, (p.y - 0.66) / 0.56);
                  vec2 d = vec2(bp.x - spPos.x, bp.y - spPos.y);
                  float ca = cos(rot), sa = sin(rot);
                  vec2 rd = vec2(d.x * ca - d.y * sa, d.x * sa + d.y * ca);
                  float dist = length(rd / max(spPos.zw, vec2(0.04)));
                  smask = (1.0 - smoothstep(0.84, 1.0, dist)) * torsoZone * smoothstep(-0.05, 0.22, p.z);
                }
              #endif

              #ifdef ARM
                #ifdef LEFT
                  if (partCode > 1.5 && partCode < 2.5) {
                    float dAlong = abs(vAlong - spPos.x) / max(spPos.z, 0.05);
                    float fFace = smoothstep(-0.20, 0.35, normalize(vNormal).z);
                    smask = (1.0 - smoothstep(0.75, 1.0, dAlong)) * fFace;
                  }
                #else
                  if (partCode > 2.5) {
                    float dAlong = abs(vAlong - spPos.x) / max(spPos.z, 0.05);
                    float fFace = smoothstep(-0.20, 0.35, normalize(vNormal).z);
                    smask = (1.0 - smoothstep(0.75, 1.0, dAlong)) * fFace;
                  }
                #endif
              #endif

              if (smask > 0.001) {
                col = mix(col, spCol.rgb, clamp(smask * spCol.a, 0.0, 1.0));
              }
            }
          }
        #endif

        // Designer-vinyl / soft matte-velvet 3D studio lighting (inspired by KuroCat app icon — ZERO KeyCat white outline!)
        vec3 n = normalize(vNormal);
        vec3 v = normalize(vViewDir);
        float ndv = clamp(dot(n, v), 0.0, 1.0);
        // Pre-normalized (-0.28, 0.74, 0.62) and (0.58, 0.24, 0.45)
        float keyLit = dot(n, vec3(-0.2785, 0.7361, 0.6167));
        float fillLit = dot(n, vec3(0.7510, 0.3108, 0.5827));

        // Keep crown-to-ear join seamlessly unified in value
        #ifdef CAT_BODY
          float earJoin = smoothstep(1.66, 1.94, p.y);
          keyLit = mix(keyLit, max(keyLit, 0.56), earJoin * 0.55);
          // Subtle cozy contact AO right at desk base
          float baseAO = mix(0.84, 1.0, smoothstep(0.0, 0.24, p.y));
        #else
          float baseAO = 1.0;
        #endif
        #ifdef EAR
          keyLit = mix(keyLit, 0.56, 0.48);
        #endif

        if (uIsDarkFur > 0.5) {
          // True deep velvety jet-black in linear space (gamma-correct subtle sheen + crisp dark-velvet silhouette rim!)
          float velvetKey = pow(max(0.0, keyLit), 1.65);
          float velvetFill = max(0.0, fillLit) * 0.16;
          vec3 sheen = vec3(0.0115, 0.0125, 0.0165) * (velvetKey + velvetFill);
          vec3 rimGlow = vec3(0.022, 0.025, 0.034) * pow(1.0 - ndv, 2.7);
          col += (sheen + rimGlow) * uShade;
          col *= mix(1.0, baseAO, uShade);
        } else {
          // Soft sculptural plush illumination for Ragdoll, Orange Tabby, Calico & Siamese
          float wrap = keyLit * 0.5 + 0.5;
          float shadeMul = mix(0.84, 1.08, smoothstep(0.12, 0.92, wrap));
          col *= mix(1.0, shadeMul * baseAO, uShade);
          col += vec3(0.036, 0.032, 0.028) * max(0.0, keyLit) * uShade * 0.45;
          // Gentle warm-cool grazing edge softness
          col *= 1.0 - uShade * 0.14 * pow(1.0 - ndv, 2.2);
        }

        gl_FragColor = vec4(col, 1.0);
        #include <colorspace_fragment>
      }
    `;

    return new THREE.ShaderMaterial({
      uniforms: this.uniforms,
      vertexShader,
      fragmentShader,
      defines,
    });
  }

  public applyPalette(p: FurPalette3D, spots: CatSpot[] = []): void {
    const setColor = (key: string, hex: string) => {
      (this.uniforms[key].value as THREE.Color).set(hex);
    };
    setColor('uFur', p.fur);
    setColor('uLight', p.light);
    setColor('uDark', p.dark);
    setColor('uPatchA', p.patchA);
    setColor('uPatchB', p.patchB);
    setColor('uPoint', p.point);
    setColor('uInner', p.inner);
    setColor('uPawColor', p.paw);
    this.uniforms.uStripes.value = p.stripes;
    this.uniforms.uMuzzle.value = p.muzzle;
    this.uniforms.uBib.value = p.bib;
    this.uniforms.uCalico.value = p.calico;
    this.uniforms.uTuxedo.value = p.tuxedo;
    this.uniforms.uPoints.value = p.points;
    this.uniforms.uInnerAmt.value = p.innerAmt;
    this.uniforms.uShade.value = p.shade;
    this.uniforms.uIsDarkFur.value = p.isDarkFur ? 1.0 : 0.0;
    if (p.earOuter) {
      setColor('uEarOuter', p.earOuter);
      this.uniforms.uEarOuterAmt.value = 1.0;
    } else {
      this.uniforms.uEarOuterAmt.value = 0.0;
    }
    if (p.tailBase) {
      setColor('uTailBase', p.tailBase);
      this.uniforms.uTailBaseAmt.value = 1.0;
    } else {
      this.uniforms.uTailBaseAmt.value = 0.0;
    }

    const list = Array.isArray(spots) ? spots.slice(0, MAX_CUSTOM_SPOTS) : [];
    const posArr = this.uniforms.uSpotPos.value as THREE.Vector4[];
    const colArr = this.uniforms.uSpotCol.value as THREE.Vector4[];
    const metaArr = this.uniforms.uSpotMeta.value as THREE.Vector4[];
    const tmpCol = new THREE.Color();
    this.uniforms.uSpotCount.value = list.length;

    for (let i = 0; i < list.length; i++) {
      const sp = list[i];
      const partCode =
        sp.part === 'head' ? 0 : sp.part === 'body' ? 1 : sp.part === 'leftArm' ? 2 : 3;
      if (partCode === 0) {
        posArr[i].set(sp.x, -sp.y, Math.max(0.06, sp.rx / 55), Math.max(0.06, sp.ry / 45));
      } else if (partCode === 1) {
        posArr[i].set(sp.x, -sp.y, Math.max(0.06, sp.rx / 65), Math.max(0.06, sp.ry / 65));
      } else {
        posArr[i].set(
          Math.max(0.08, Math.min(0.92, sp.x)),
          sp.y || 0,
          Math.max(0.06, sp.rx / 95),
          Math.max(0.06, sp.ry / 30),
        );
      }
      try {
        tmpCol.set(sp.color || '#3c3634');
      } catch {
        tmpCol.set('#3c3634');
      }
      colArr[i].set(tmpCol.r, tmpCol.g, tmpCol.b, sp.opacity ?? 0.85);
      metaArr[i].set(partCode, -(sp.rotation || 0), 0, 0);
    }
  }
}
