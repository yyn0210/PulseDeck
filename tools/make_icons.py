"""生成 PulseDeck 应用图标（32/128/256 PNG + 多尺寸 ICO）。

用法：  python tools/make_icons.py
依赖：  Pillow（本机系统 Python 3.11 已带）
"""

from pathlib import Path

from PIL import Image, ImageDraw

OUT = Path(__file__).resolve().parent.parent / "src-tauri" / "icons"
SUPER = 1024  # 超采样，缩小时更干净


def gradient(size, top, bottom):
    img = Image.new("RGBA", (size, size))
    px = img.load()
    for y in range(size):
        t = y / max(size - 1, 1)
        px_y = tuple(round(top[i] + (bottom[i] - top[i]) * t) for i in range(3)) + (255,)
        for x in range(size):
            px[x, y] = px_y
    return img


def rounded_mask(size, radius_ratio=0.22):
    mask = Image.new("L", (size, size), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        [0, 0, size - 1, size - 1], radius=int(size * radius_ratio), fill=255
    )
    return mask


def build(size):
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    grad = gradient(size, (79, 140, 255), (169, 123, 255))
    canvas.paste(grad, (0, 0), rounded_mask(size))

    layer = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)

    # 监控折线（心跳/负载曲线），相对坐标
    pts = [
        (0.14, 0.56),
        (0.28, 0.56),
        (0.37, 0.30),
        (0.50, 0.74),
        (0.62, 0.42),
        (0.71, 0.56),
        (0.86, 0.56),
    ]
    pts = [(round(x * size), round(y * size)) for x, y in pts]
    w = max(size // 14, 3)
    d.line(pts, fill=(255, 255, 255, 245), width=w, joint="curve")

    canvas = Image.alpha_composite(canvas, layer)
    canvas.putalpha(rounded_mask(size))
    return canvas


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    big = build(SUPER)

    sizes = {
        "32x32.png": 32,
        "128x128.png": 128,
        "128x128@2x.png": 256,
    }
    for name, s in sizes.items():
        big.resize((s, s), Image.LANCZOS).save(OUT / name)
        print("wrote", name)

    ico_path = OUT / "icon.ico"
    big.resize((256, 256), Image.LANCZOS).save(
        ico_path,
        format="ICO",
        sizes=[(256, 256), (128, 128), (64, 64), (48, 48), (32, 32), (16, 16)],
    )
    print("wrote", ico_path.name)


if __name__ == "__main__":
    main()
