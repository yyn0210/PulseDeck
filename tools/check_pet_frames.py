# -*- coding: utf-8 -*-
"""宠物贴图自检：拿"源图"当基准，查 `src/assets/pet/t<档位>_<帧>.png` 有没有抠错。

为什么需要它：抠底最典型的失败是**白色毛被当成背景抠掉** —— 小狐狸的口鼻 / 耳内 / 胸口白毛
和浅灰背景几乎同色（实测色差只有 0.01~0.07），一旦按纯颜色阈值抠，这些地方会变成透明窟窿，
肉眼看就是"嘴没了"。缩略图上很容易漏掉，必须逐像素比。

两类错误，都用「源图像素离局部背景色多远」来判（都复用 `copy_pet_frames` 里的局部背景模型）：

* **前景被抠破**（真正致命）：输出里 `alpha<60` 且**不连到画面四边**的连通块，
  其源图像素却明显不是背景色（dist > `--fg`，默认 0.15）→ 白毛 / 嘴被抠掉了。
* **背景没抠净**（雾气）：源图明显是背景（dist < `--bg`，默认 0.05）的像素，输出里 `alpha>128`
  → 桌面上会看到一圈浅色方块边。

`dist` 小的内部透明块（浅色光晕 / 泡泡本身）**不算问题** —— 那些本来就应该透。

用法：

    python tools/check_pet_frames.py                # 默认阈值，直接当 CI 用（有问题非零退出）
    python tools/check_pet_frames.py --verbose      # 逐张打印
    python tools/check_pet_frames.py --min-px 40    # 更严格（默认 100）
"""

from __future__ import annotations

import argparse
import glob
import os
import re
import sys

import numpy as np
from PIL import Image
from scipy import ndimage

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from copy_pet_frames import (  # noqa: E402
    COARSE_TOL,
    OUT_W,
    SRC_DIR,
    estimate_bg,
    flood_from_border,
    local_bg_model,
)

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT_DIR = os.path.join(ROOT, "src", "assets", "pet")
EXPECTED_TIERS = {38, 50, 63, 74, 84, 94}   # 与 src/pet.js 的 SCALE 一致
PAT = re.compile(r"t(\d+)_(\d+)\.png$")
TRANSPARENT = 60     # alpha 低于此算"透明"
OPAQUE = 128         # alpha 高于此算"不透明"
_RING = 10           # "画面边缘"的宽度（像素）


def source_planes(tier: int, frame: int):
    """读源图并缩到与输出同尺寸，返回 (rgb 0~1, 离局部背景色的 dist)。"""
    p = os.path.join(SRC_DIR, f"pet_{tier:02d}_{frame:02d}.png")
    if not os.path.exists(p):
        return None, None
    im = Image.open(p).convert("RGB")
    w, h = im.size
    small = im.resize((OUT_W, round(h * OUT_W / w)), Image.LANCZOS)
    rgb = np.asarray(small).astype(np.float32) / 255.0
    coarse = flood_from_border(np.abs(rgb - estimate_bg(rgb)).max(axis=2) < COARSE_TOL)
    dist = np.abs(rgb - local_bg_model(rgb, coarse)).max(axis=2)
    return rgb, dist


def interior_components(alpha: np.ndarray, dist, min_px: int):
    """alpha<TRANSPARENT 且不连到画面四边的连通块，按面积降序。

    每项附带该块在源图上的「离背景距离中位数」`med`：med 大 → 源图是前景却被抠透了（真问题）。
    """
    trans = alpha < TRANSPARENT
    lbl, n = ndimage.label(trans)  # 4 连通
    on_border = set(lbl[0, :]) | set(lbl[-1, :]) | set(lbl[:, 0]) | set(lbl[:, -1])
    on_border.discard(0)
    out = []
    for i in range(1, n + 1):
        if i in on_border:
            continue
        size = int((lbl == i).sum())
        if size >= min_px:
            ys, xs = np.nonzero(lbl == i)
            med = float(np.median(dist[ys, xs])) if dist is not None else None
            out.append((size, (int(ys.min()), int(xs.min()), int(ys.max()), int(xs.max())), med))
    out.sort(reverse=True)
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--min-px", type=int, default=100, help="内部透明块超过这个像素数才检查（默认 100）")
    ap.add_argument("--fg", type=float, default=0.15, help="源图像素离背景 > 此值即认定是前景（默认 0.15）")
    ap.add_argument("--bg", type=float, default=0.05, help="源图像素离背景 < 此值即认定是背景（默认 0.05）")
    ap.add_argument("--haze-px", type=int, default=80, help="画面边缘允许的不透明背景色像素上限（默认 80）")
    ap.add_argument("--haze-ratio", type=float, default=0.25,
                    help="不透明像素里背景色的占比上限（默认 0.25，超过说明背景整块没抠）")
    ap.add_argument("--verbose", action="store_true")
    args = ap.parse_args()

    files = sorted(glob.glob(os.path.join(OUT_DIR, "t*.png")))
    if not files:
        print(f"[FAIL] 找不到贴图：{OUT_DIR}\\t*_*.png（先跑 python tools/copy_pet_frames.py）")
        return 1

    tiers: dict[int, list[int]] = {}
    sizes, punched, hazy, empty, no_src = [], [], [], [], []
    for p in files:
        name = os.path.basename(p)
        m = PAT.search(name)
        if not m:
            print(f"[WARN] 文件名不合规（应为 t<档位>_<帧>.png）：{name}")
            continue
        tier, frame = int(m.group(1)), int(m.group(2))
        tiers.setdefault(tier, []).append(frame)

        im = Image.open(p).convert("RGBA")
        sizes.append(im.size)
        alpha = np.asarray(im)[..., 3]
        if int((alpha > 0).sum()) == 0:
            empty.append(name)
            continue

        _, dist = source_planes(tier, frame)
        if dist is None:
            no_src.append(name)

        # ① 背景没抠净。
        #   注意：软光晕 / 地面倒影本来就接近背景色，判定为"前景"（不透明）是**对的**，
        #   所以不能一看到"背景色像素不透明"就报错。真正的错误形态只有两种：
        #     a. 画**边缘**出现不透明的背景色 → 桌面上是一圈浅色方块边（硬伤，阈值很严）
        #     b. 背景整体没抠（例如阈值配错导致什么都没透明）→ 不透明像素里背景色占比极高
        if dist is not None:
            inside = np.ones_like(alpha, bool)
            inside[:_RING] = inside[-_RING:] = False
            inside[:, :_RING] = inside[:, -_RING:] = False
            bg_like = dist < args.bg
            ring_px = int((bg_like & (alpha > OPAQUE) & ~inside).sum())
            n_bg_opaque = int((bg_like & (alpha > OPAQUE)).sum())
            n_opaque = int((alpha > OPAQUE).sum())
            ratio = n_bg_opaque / max(1, n_opaque)
            if ring_px >= args.haze_px:
                hazy.append((name, f"画面边缘有 {ring_px}px 不透明背景色", "ring"))
            if ratio > args.haze_ratio:
                hazy.append((name, f"不透明像素里背景色占 {ratio:.0%}（疑似整块背景没抠）", "ratio"))

        # ② 前景被抠破（白毛 / 嘴被抠掉）
        for size, bb, med in interior_components(alpha, dist, args.min_px):
            if med is None or med > args.fg:
                punched.append((name, size, bb, med))

        if args.verbose:
            print(f"[ ok ] {name}  尺寸={im.size} 前景像素={int((alpha > OPAQUE).sum())}")

    print()
    print(f"贴图 {len(files)} 张；档位 {sorted(tiers)}；"
          f"每档帧数 { {t: len(set(v)) for t, v in sorted(tiers.items())} }；尺寸 {sorted(set(sizes))}")
    if no_src:
        print(f"[WARN] {len(no_src)} 张找不到对应源图，跳过破坏检查：{no_src[:5]}")

    problems = []
    if set(tiers) != EXPECTED_TIERS:
        problems.append(f"档位不匹配：缺 {sorted(EXPECTED_TIERS - set(tiers))}，"
                        f"多 {sorted(set(tiers) - EXPECTED_TIERS)}")
    if len(set(sizes)) > 1:
        problems.append(f"各帧尺寸不一致：{sorted(set(sizes))}（换帧会跳，必须统一）")
    if empty:
        problems.append(f"{len(empty)} 张整张全透明：{empty[:5]}")
    if punched:
        problems.append(f"{len(punched)} 处**前景被抠破**（源图明显不是背景，却被抠成透明）：")
        for name, size, bb, med in punched[:8]:
            problems.append(f"        {name}  {size}px  bbox(y0,x0,y1,x1)={bb}  源图离背景 {med:.3f}")
    if hazy:
        problems.append(f"{len(hazy)} 张**背景没抠净**（浅色雾气会变成方块边 / 背景整块残留）：")
        for name, why, _kind in hazy[:8]:
            problems.append(f"        {name}  {why}")

    if problems:
        print()
        for line in problems:
            print(f"[FAIL] {line}")
        return 1

    print("[ OK ] 没有把前景抠破，也没有背景残留")
    return 0


if __name__ == "__main__":
    sys.exit(main())
