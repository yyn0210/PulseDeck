# -*- coding: utf-8 -*-
"""把 `src/assets/img/pet/` 下的原图做成宠物贴图（320 宽 PNG + alpha）。

为什么必须"抠底"而不能直接用原图：原图是**不透明**的浅灰底（约 RGB 242/241/243，
带一点蓝紫渐变），直接当贴图会带一个大白方块。

抠底的核心难点：**小狐狸的白色毛（口鼻/耳内/胸口）和背景几乎同色**
（实测色差只有 0.01~0.07），单靠"像素颜色离背景多远"的阈值一定会把白毛一起抠掉
（用户反馈的"把嘴抠没了"就是这个）。

所以这里用 **连通性** 而不是纯颜色：

1. **局部背景模型**：背景是渐变（角落能偏出全局中位数 0.10），先用粗阈值从画面
   边缘洪水填充得到可信背景，再按网格中位数建一张"局部背景色"图并平滑上采样。
2. **软 alpha**：按"离局部背景色的最大通道差"做 6%→10% 的线性羽化。
3. **洪水填充定内外**：从画面四条边出发，沿 `dist < FLOOD_GATE` 的像素蔓延。
   蔓延得到的是"外面"（背景）；蔓延不到的像素一律 **alpha=1**。
   白色口鼻虽然颜色像背景，但被橘色毛/深色线围住、从边缘蔓延不进去 → 保住。
4. RGB 全程保持美术原色：**不反预乘、不调色、不模糊、不做形态学**。

用法：

    python tools/copy_pet_frames.py
"""

from __future__ import annotations

import glob
import os
import re

import numpy as np
from PIL import Image
from scipy import ndimage

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC_DIR = os.path.join(ROOT, "src", "assets", "img", "pet")
OUT_DIR = os.path.join(ROOT, "src", "assets", "pet")
OUT_W = 320

# —— 背景/前景判定 ——
# 可用环境变量临时覆盖，方便调参：PULSEDECK_PET_BG_FULL / _BG_EDGE / _FLOOD_GATE / _COARSE_TOL
def _env_float(name: str, default: float) -> float:
    raw = os.environ.get(name)
    if raw is None or raw == "":
        return default
    try:
        return float(raw)
    except ValueError:
        print(f"[WARN] 环境变量 {name}={raw!r} 不是数字，用默认值 {default}")
        return default


COARSE_TOL = _env_float("PULSEDECK_PET_COARSE_TOL", 0.15)  # 第 1 步粗判"像背景"（只需大致分得开，后面还会用局部模型重算）
BG_FULL = _env_float("PULSEDECK_PET_BG_FULL", 0.06)        # 离局部背景色 < 6%  → alpha 0
BG_EDGE = _env_float("PULSEDECK_PET_BG_EDGE", 0.10)        # 离局部背景色 > 10% → alpha 1；中间线性羽化
FLOOD_GATE = _env_float("PULSEDECK_PET_FLOOD_GATE", 0.10)  # 洪水填充只能走"离背景色 < 10%"的像素；实测等于 BG_EDGE 时内部空洞最少，且不得低于 BG_EDGE
GRID = 24   # 局部背景模型的网格密度（粗网格中位数 → 双线性上采样）


def estimate_bg(rgb: np.ndarray) -> np.ndarray:
    """四条边各 8px 带的中位数当初始背景色。"""
    b = 8
    edges = np.concatenate([
        rgb[:b].reshape(-1, 3), rgb[-b:].reshape(-1, 3),
        rgb[:, :b].reshape(-1, 3), rgb[:, -b:].reshape(-1, 3),
    ])
    return np.median(edges, axis=0)


def flood_from_border(mask: np.ndarray) -> np.ndarray:
    """从画面四条边出发、在 mask 为 True 的像素上做 4 连通洪水填充。"""
    seeds = np.zeros_like(mask)
    seeds[0, :] = mask[0, :]
    seeds[-1, :] = mask[-1, :]
    seeds[:, 0] = mask[:, 0]
    seeds[:, -1] = mask[:, -1]
    if not seeds.any():
        return np.zeros_like(mask)
    return ndimage.binary_propagation(seeds, mask=mask)


def local_bg_model(rgb: np.ndarray, bg_mask: np.ndarray, grid: int = GRID) -> np.ndarray:
    """用可信背景像素按粗网格取中位数，空洞用最近邻补齐，再双线性上采样成平滑背景图。"""
    h, w = rgb.shape[:2]
    gh = max(1, grid)
    gw = max(1, round(grid * w / h))
    ys = np.linspace(0, h, gh + 1).round().astype(int)
    xs = np.linspace(0, w, gw + 1).round().astype(int)
    cell = np.zeros((gh, gw, 3), np.float32)
    ok = np.zeros((gh, gw), bool)
    for i in range(gh):
        for j in range(gw):
            block = rgb[ys[i]:ys[i + 1], xs[j]:xs[j + 1]]
            m = bg_mask[ys[i]:ys[i + 1], xs[j]:xs[j + 1]]
            if m.any():
                cell[i, j] = np.median(block[m], axis=0)
                ok[i, j] = True
    if not ok.any():                       # 极端兜底：整幅都判成前景
        cell[:] = estimate_bg(rgb)
        ok[:] = True
    if not ok.all():                       # 空缺格子 → 用最近的已知格子补
        idx = ndimage.distance_transform_edt(~ok, return_distances=False, return_indices=True)
        cell = cell[idx[0], idx[1]]
    # 双线性上采样（逐通道用 PIL 的浮点图）
    out = np.empty((h, w, 3), np.float32)
    for c in range(3):
        im = Image.fromarray(cell[..., c], mode="F")
        out[..., c] = np.asarray(im.resize((w, h), Image.BILINEAR))
    return out


def process(rgb: np.ndarray, debug: bool = False):
    """RGB float 0~1 → RGBA uint8。"""
    bg0 = estimate_bg(rgb)
    dist0 = np.abs(rgb - bg0).max(axis=2)
    coarse = flood_from_border(dist0 < COARSE_TOL)

    bg_local = local_bg_model(rgb, coarse)
    dist = np.abs(rgb - bg_local).max(axis=2)
    alpha_soft = np.clip((dist - BG_FULL) / (BG_EDGE - BG_FULL), 0.0, 1.0)

    outside = flood_from_border(dist < FLOOD_GATE)
    alpha = np.where(outside, alpha_soft, 1.0)   # 蔓延不到的内部 → 一律不透明

    rgba = np.concatenate([rgb, alpha[..., None]], axis=2)
    im = Image.fromarray((rgba * 255.0).round().astype(np.uint8), "RGBA")
    if debug:
        return im, dict(bg0=bg0, dist=dist, alpha_soft=alpha_soft, outside=outside)
    return im


def main() -> int:
    pat = re.compile(r"pet_(\d+)_(\d+)\.png$")
    files = sorted(glob.glob(os.path.join(SRC_DIR, "pet_*.png")))
    if not files:
        print(f"未找到源图：{SRC_DIR}\\pet_*.png")
        return 1

    os.makedirs(OUT_DIR, exist_ok=True)
    total = 0
    for f in files:
        m = pat.search(os.path.basename(f))
        if not m:
            continue
        tier, frame = int(m.group(1)), int(m.group(2))
        im = Image.open(f).convert("RGB")
        w, h = im.size
        small = im.resize((OUT_W, round(h * OUT_W / w)), Image.LANCZOS)
        rgb = np.asarray(small).astype(np.float32) / 255.0
        rgba_im = process(rgb)
        name = f"t{tier}_{frame}.png"
        dst = os.path.join(OUT_DIR, name)
        rgba_im.save(dst, "PNG", optimize=True)
        total += os.path.getsize(dst)

    tiers = sorted({int(pat.search(os.path.basename(f)).group(1)) for f in files})
    per = sum(1 for f in files if pat.search(os.path.basename(f)).group(1) == str(tiers[0]))
    print(f"源图 {len(files)} 张，档位 {tiers}，每档 {per} 帧")
    print(f"输出 {len(files)} 张，合计 {total/1024/1024:.2f} MB → {OUT_DIR}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
