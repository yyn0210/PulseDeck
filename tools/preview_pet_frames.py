# -*- coding: utf-8 -*-
"""宠物贴图预览：把所有档位 × 所有帧拼成联络表，落到 `shots/`，供人眼验收抠图。

自动自检（`tools/check_pet_frames.py`）只能抓"数值上明显不对"的情况；轮廓生硬、
光晕断开、档位之间角色跳动这类问题还是得靠眼睛。这里**同时出深色底和洋红底两张**：

* **深色底**：模拟深色壁纸下的真实观感（也是检查"贴到深色桌面上发不发灰"的正路）。
* **洋红底**：洋红 = 被抠掉/透明，一眼能看出哪里抠破了。

用法：

    python tools/preview_pet_frames.py
"""

from __future__ import annotations

import glob
import os
import re

from PIL import Image, ImageDraw

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT_DIR = os.path.join(ROOT, "src", "assets", "pet")
SHOTS = os.path.join(ROOT, "shots")
CELL_W = 188
BACKGROUNDS = {"dark": (26, 27, 32), "magenta": (255, 0, 255)}
PAT = re.compile(r"t(\d+)_(\d+)\.png$")


def main() -> int:
    files = glob.glob(os.path.join(OUT_DIR, "t*.png"))
    if not files:
        print(f"找不到贴图：{OUT_DIR}\\t*_*.png（先跑 python tools/copy_pet_frames.py）")
        return 1

    grid: dict[int, dict[int, str]] = {}
    for p in sorted(files):
        m = PAT.search(os.path.basename(p))
        if m:
            grid.setdefault(int(m.group(1)), {})[int(m.group(2))] = p
    tiers = sorted(grid)
    frames = sorted({f for v in grid.values() for f in v})
    print(f"档位 {tiers}，帧 {frames}")

    os.makedirs(SHOTS, exist_ok=True)
    sample = Image.open(grid[tiers[0]][frames[0]])
    cell_h = round(sample.height * CELL_W / sample.width)

    for tag, bg in BACKGROUNDS.items():
        cols, rows = len(frames), len(tiers)
        pad, lbl = 3, 16
        sheet = Image.new(
            "RGB",
            ((CELL_W + pad) * cols + pad, (cell_h + lbl + pad) * rows + 20),
            (245, 245, 245),
        )
        d = ImageDraw.Draw(sheet)
        head = "洋红底：品红 = 被抠掉/透明" if tag == "magenta" else "深色底：看轮廓是否生硬、发不发灰"
        d.text((6, 4), f"宠物贴图总览（{head}）", fill=(0, 0, 0))
        for ri, tier in enumerate(tiers):
            y = 18 + ri * (cell_h + lbl + pad)
            d.text((6, y), f"t{tier}", fill=(170, 0, 0))
            for ci, frame in enumerate(frames):
                src = grid.get(tier, {}).get(frame)
                if src is None:
                    continue
                im = Image.open(src).convert("RGBA").resize((CELL_W, cell_h), Image.LANCZOS)
                base = Image.new("RGBA", im.size, bg + (255,))
                sheet.paste(Image.alpha_composite(base, im).convert("RGB"),
                            (pad + ci * (CELL_W + pad), y + lbl))
        dst = os.path.join(SHOTS, f"pet_frames_{tag}.png")
        sheet.save(dst)
        print("saved", dst, sheet.size)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
