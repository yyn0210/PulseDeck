/**
 * Ported from KuroCat (https://github.com/qianyiai/kurocat), v5.1.0.
 * Copyright (c) 2026 KuroCat Contributors, MIT License.
 * See src-tauri/resources/licenses/KUROCAT-MIT.txt.
 */
export interface CatSpot {
  part: 'head' | 'body' | 'leftArm' | 'rightArm';
  x: number;
  y: number;
  rx: number;
  ry: number;
  rotation?: number;
  color: string;
  opacity?: number;
}

export type CatPattern = 'solid' | 'bicolor' | 'tuxedo' | 'tabby' | 'calico' | 'siamese';

export interface CatSkinConfig {
  id: string;
  name: string;
  emoji?: string;
  builtin?: boolean;

  furColor: string;
  furSecondaryColor?: string;
  outlineColor?: string;
  legLineColor?: string;

  pattern?: CatPattern;
  maskColor?: string;
  foreheadBridgeColor?: string;
  flankColor?: string;
  bellyBibColor?: string;
  stripeColor?: string;
  calicoPatchColor?: string;

  earOuterColor?: string;
  earTipColor?: string;
  earInnerColor?: string;
  earTuftColor?: string;

  tailColor?: string;
  tailTipColor?: string;
  tailFluffy?: boolean;
  tailStripes?: boolean;

  eyeColorTop: string;
  eyeColorBottom?: string;
  pupilColor?: string;
  eyeCatchlight?: boolean;
  eyeRimColor?: string;
  squintColor?: string;
  squintGlowColor?: string;
  noseColor?: string;
  mouthColor?: string;
  blushColor?: string;
  whiskerColor?: string;

  armColor?: string;
  pawColor?: string;
  pawOutlineColor?: string;
  armCreaseColor?: string;

  accentColor?: string;
  accentSideColor?: string;

  spots?: CatSpot[];
}

export interface ResolvedSkin {
  id: string;
  name: string;
  isDarkBase: boolean;
  furColor: string;
  furSecondaryColor: string;
  outlineColor: string | null;
  legLineColor: string;
  pattern: CatPattern;
  maskColor: string | null;
  foreheadBridgeColor: string | null;
  flankColor: string | null;
  bellyBibColor: string | null;
  stripeColor: string | null;
  calicoPatchColor: string | null;
  earOuterColor: string;
  earTipColor: string | null;
  earInnerColor: string | null;
  earTuftColor: string | null;
  tailColor: string;
  tailTipColor: string;
  tailFluffy: boolean;
  tailStripes: boolean;
  eyeColorTop: string;
  eyeColorBottom: string;
  pupilColor: string;
  eyeCatchlight: boolean;
  eyeRimColor: string | null;
  squintColor: string;
  squintGlowColor: string | null;
  noseColor: string | null;
  mouthColor: string | null;
  blushColor: string | null;
  whiskerColor: string;
  armColor: string;
  pawColor: string;
  pawOutlineColor: string | null;
  armCreaseColor: string | null;
  accentColor: string;
  accentSideColor: string;
  spots: CatSpot[];
}

function parseHexLuminance(hex: string): number {
  const m = hex.trim().match(/^#([0-9a-f]{3}|[0-9a-f]{6})$/i);
  if (!m) return 0.5;
  let h = m[1];
  if (h.length === 3) h = h[0] + h[0] + h[1] + h[1] + h[2] + h[2];
  const r = parseInt(h.slice(0, 2), 16) / 255;
  const g = parseInt(h.slice(2, 4), 16) / 255;
  const b = parseInt(h.slice(4, 6), 16) / 255;
  return 0.299 * r + 0.587 * g + 0.114 * b;
}

export function isLightColor(hex: string): boolean {
  return parseHexLuminance(hex) > 0.48;
}

export function resolveCustomSkin(raw: CatSkinConfig): ResolvedSkin {
  const fur = raw.furColor || '#f2a452';
  const darkBase = !isLightColor(fur);
  const pattern: CatPattern = raw.pattern || 'solid';
  const defaultOutline = darkBase ? null : '#524840';
  const outlineColor = raw.outlineColor !== undefined ? (raw.outlineColor || null) : defaultOutline;
  const pawColor = raw.pawColor || (pattern === 'tuxedo' ? '#faf9f5' : fur);
  const armColor = raw.armColor || fur;
  const pawIsLight = isLightColor(pawColor);

  return {
    id: raw.id,
    name: raw.name || raw.id,
    isDarkBase: darkBase,
    furColor: fur,
    furSecondaryColor: raw.furSecondaryColor || fur,
    outlineColor,
    legLineColor:
      raw.legLineColor || (darkBase ? 'rgba(214, 218, 224, 0.62)' : 'rgba(82, 72, 64, 0.62)'),
    pattern,
    maskColor: raw.maskColor || (pattern === 'siamese' ? '#4a382c' : pattern === 'bicolor' ? '#9e8e7f' : null),
    foreheadBridgeColor: raw.foreheadBridgeColor || null,
    flankColor: raw.flankColor || null,
    bellyBibColor:
      raw.bellyBibColor || (pattern === 'tuxedo' ? '#faf9f5' : null),
    stripeColor:
      raw.stripeColor || (pattern === 'tabby' ? '#9e5318' : null),
    calicoPatchColor:
      raw.calicoPatchColor || (pattern === 'calico' ? '#3a3533' : null),
    earOuterColor: raw.earOuterColor || raw.maskColor || fur,
    earTipColor: raw.earTipColor || null,
    earInnerColor: raw.earInnerColor || (darkBase ? null : '#efc4c8'),
    earTuftColor: raw.earTuftColor || null,
    tailColor: raw.tailColor || raw.earOuterColor || raw.maskColor || fur,
    tailTipColor: raw.tailTipColor || raw.earTipColor || raw.tailColor || fur,
    tailFluffy: Boolean(raw.tailFluffy),
    tailStripes: Boolean(raw.tailStripes || pattern === 'tabby'),
    eyeColorTop: raw.eyeColorTop || '#5a94d6',
    eyeColorBottom: raw.eyeColorBottom || raw.eyeColorTop || '#a3d2f2',
    pupilColor: raw.pupilColor || '#10141e',
    eyeCatchlight: raw.eyeCatchlight ?? !darkBase,
    eyeRimColor: raw.eyeRimColor || (darkBase ? null : 'rgba(42, 46, 58, 0.78)'),
    squintColor: raw.squintColor || (darkBase ? raw.eyeColorTop || '#d4f044' : '#2c2928'),
    squintGlowColor: raw.squintGlowColor || null,
    noseColor: raw.noseColor !== undefined ? (raw.noseColor || null) : (darkBase ? null : '#ee8e9e'),
    mouthColor: raw.mouthColor || (darkBase ? null : 'rgba(72, 68, 76, 0.78)'),
    blushColor: raw.blushColor || (darkBase ? null : '#f492a6'),
    whiskerColor:
      raw.whiskerColor || (darkBase ? 'rgba(210, 214, 220, 0.56)' : 'rgba(84, 76, 70, 0.56)'),
    armColor,
    pawColor,
    pawOutlineColor:
      raw.pawOutlineColor !== undefined
        ? (raw.pawOutlineColor || null)
        : (pawIsLight ? outlineColor || '#524840' : null),
    armCreaseColor:
      raw.armCreaseColor !== undefined
        ? (raw.armCreaseColor || null)
        : (!pawIsLight ? 'rgba(216, 218, 222, 0.62)' : null),
    accentColor: raw.accentColor || '#d2f244',
    accentSideColor: raw.accentSideColor || '#8cb014',
    spots: Array.isArray(raw.spots) ? raw.spots : [],
  };
}

export const DEFAULT_BUILTIN_CATS: CatSkinConfig[] = [
  {
    id: 'black',
    name: '小黑猫',
    emoji: '🐈‍⬛',
    builtin: true,
    furColor: '#141314',
    eyeColorTop: '#d4f044',
    accentColor: '#d2f244',
    accentSideColor: '#8cb014',
  },
  {
    id: 'ragdoll',
    name: '布偶猫',
    emoji: '🐱',
    builtin: true,
    furColor: '#fbf9f3',
    eyeColorTop: '#2c5594',
    accentColor: '#7ac7ff',
    accentSideColor: '#4690d8',
  },
  {
    id: 'ginger',
    name: '大橘猫',
    emoji: '🍊',
    builtin: true,
    furColor: '#f2a452',
    furSecondaryColor: '#e38c38',
    outlineColor: '#6b4322',
    legLineColor: 'rgba(107, 67, 34, 0.60)',
    pattern: 'tabby',
    bellyBibColor: '#fdf6e8',
    stripeColor: '#b85d19',
    earOuterColor: '#e38c38',
    earInnerColor: '#f3b9b5',
    tailColor: '#eb9640',
    tailTipColor: '#b85d19',
    tailFluffy: false,
    tailStripes: true,
    eyeColorTop: '#b86b14',
    eyeColorBottom: '#ffd369',
    pupilColor: '#141210',
    eyeCatchlight: true,
    eyeRimColor: 'rgba(82, 52, 24, 0.78)',
    squintColor: '#523318',
    noseColor: '#ee8d98',
    mouthColor: 'rgba(92, 62, 36, 0.80)',
    blushColor: '#f596a2',
    whiskerColor: 'rgba(107, 67, 34, 0.56)',
    armColor: '#f2a452',
    pawColor: '#fdf6e8',
    pawOutlineColor: '#6b4322',
    accentColor: '#ffb347',
    accentSideColor: '#cc7a16',
  },
  {
    id: 'calico',
    name: '三花猫',
    emoji: '🌸',
    builtin: true,
    furColor: '#fcfaf5',
    furSecondaryColor: '#f1ece2',
    outlineColor: '#4f4640',
    legLineColor: 'rgba(79, 70, 64, 0.60)',
    pattern: 'calico',
    maskColor: '#ea9446',
    calicoPatchColor: '#3c3634',
    earOuterColor: '#3c3634',
    earInnerColor: '#f2bfc4',
    tailColor: '#3c3634',
    tailTipColor: '#ea9446',
    tailFluffy: false,
    tailStripes: false,
    eyeColorTop: '#5a8f3d',
    eyeColorBottom: '#c9e87c',
    pupilColor: '#101412',
    eyeCatchlight: true,
    eyeRimColor: 'rgba(46, 44, 42, 0.80)',
    squintColor: '#383230',
    noseColor: '#ef95a2',
    mouthColor: 'rgba(76, 68, 66, 0.80)',
    blushColor: '#f498a8',
    whiskerColor: 'rgba(86, 76, 70, 0.56)',
    armColor: '#fcfaf5',
    pawColor: '#fcfaf5',
    pawOutlineColor: '#4f4640',
    accentColor: '#ffa6c1',
    accentSideColor: '#cc6c8b',
  },
  {
    id: 'tuxedo',
    name: '奶牛猫',
    emoji: '🐄',
    builtin: true,
    furColor: '#1d1f24',
    furSecondaryColor: '#15171b',
    outlineColor: '#141519',
    legLineColor: 'rgba(120, 126, 138, 0.58)',
    pattern: 'tuxedo',
    bellyBibColor: '#faf9f5',
    earOuterColor: '#1d1f24',
    earInnerColor: '#dfb6bc',
    tailColor: '#1d1f24',
    tailTipColor: '#faf9f5',
    tailFluffy: false,
    tailStripes: false,
    eyeColorTop: '#6db840',
    eyeColorBottom: '#d8f568',
    pupilColor: '#0e1012',
    eyeCatchlight: true,
    squintColor: '#c8f052',
    noseColor: '#ee9aa6',
    mouthColor: 'rgba(56, 60, 68, 0.78)',
    blushColor: '#f492a6',
    whiskerColor: 'rgba(214, 218, 224, 0.62)',
    armColor: '#1d1f24',
    pawColor: '#faf9f5',
    pawOutlineColor: '#2c2f38',
    armCreaseColor: 'rgba(216, 218, 222, 0.56)',
    accentColor: '#7ee8a6',
    accentSideColor: '#3aa666',
  },
  {
    id: 'siamese',
    name: '暹罗猫',
    emoji: '🤎',
    builtin: true,
    furColor: '#f3eadb',
    furSecondaryColor: '#e2d4be',
    outlineColor: '#4c3b30',
    legLineColor: 'rgba(76, 59, 48, 0.58)',
    pattern: 'siamese',
    maskColor: '#4a382c',
    flankColor: '#cbb9a0',
    earOuterColor: '#3e2e24',
    earTipColor: '#2c2018',
    earInnerColor: '#bfa494',
    tailColor: '#4a382c',
    tailTipColor: '#2c2018',
    tailFluffy: false,
    tailStripes: false,
    eyeColorTop: '#245ab8',
    eyeColorBottom: '#8ad4ff',
    pupilColor: '#0e1320',
    eyeCatchlight: true,
    eyeRimColor: 'rgba(28, 22, 18, 0.80)',
    squintColor: '#8ad4ff',
    noseColor: '#3c2b22',
    mouthColor: 'rgba(56, 42, 34, 0.78)',
    blushColor: '#e89aa6',
    whiskerColor: 'rgba(98, 82, 70, 0.58)',
    armColor: '#5a4638',
    pawColor: '#3c2c22',
    pawOutlineColor: '#2c2018',
    armCreaseColor: 'rgba(220, 208, 194, 0.45)',
    accentColor: '#72c4ff',
    accentSideColor: '#3a84c8',
  },
  {
    id: 'dafeiyu',
    name: '大肥鱼',
    emoji: '🐳',
    builtin: true,
    furColor: '#2957a4',
    eyeColorTop: '#2563eb',
    accentColor: '#5cc8ff',
    accentSideColor: '#2563eb',
  },
];
