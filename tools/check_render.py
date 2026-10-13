#!/usr/bin/env python3
"""截图主窗口和宠物窗口区域，检查页面是否真的渲染了（非空白）。

⚠️ 同 verify_pet_beat.py：从沙箱 shell 启动的 pulse-deck.exe 会因
tsbx.dll 注入导致 WebView2 browser 进程 ~5.5s 后崩溃，截出来的必然是
「没渲染」——结果不可信。要从沙箱外运行（需系统 Python 的 Pillow）。

排查用：window 存在 ≠ 页面渲染了。渲染过的页面有丰富色彩，
空白/未渲染的页面是全白或全黑。注意：宠物窗口是透明窗口，
截图截到的是它后面的桌面，判读时以「有没有宠物形象」为准而非标准差。
"""

import ctypes
import ctypes.wintypes as wt
import os
import subprocess
import sys
import time

u = ctypes.windll.user32

# 默认路径从脚本位置推出来，别写死绝对路径——仓库改过名（system-monitor → PulseDeck），
# 写死的默认值会静默指向一个不存在的 exe。
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

EXE = sys.argv[1] if len(sys.argv) > 1 else os.path.join(
    ROOT, "src-tauri", "target", "release", "pulse-deck.exe"
)
WAIT = int(sys.argv[2]) if len(sys.argv) > 2 else 12
OUT = sys.argv[3] if len(sys.argv) > 3 else os.path.join(ROOT, "shots", "_render_check.png")


def windows(pid):
    out = []

    def cb(hwnd, _l):
        p = wt.DWORD()
        u.GetWindowThreadProcessId(hwnd, ctypes.byref(p))
        if p.value == pid:
            b = ctypes.create_unicode_buffer(256)
            u.GetWindowTextW(hwnd, b, 256)
            r = wt.RECT()
            u.GetWindowRect(hwnd, ctypes.byref(r))
            if b.value and (r.right - r.left) > 50:
                out.append((b.value, (r.left, r.top, r.right, r.bottom)))
        return True

    u.EnumWindows(ctypes.WINFUNCTYPE(ctypes.c_bool, wt.HWND, wt.LPARAM)(cb), 0)
    return out


def main():
    subprocess.run(["taskkill", "/F", "/IM", "pulse-deck.exe"], capture_output=True)
    time.sleep(1)
    p = subprocess.Popen([EXE])
    print(f"pid={p.pid}, 等 {WAIT}s")
    time.sleep(WAIT)

    wins = windows(p.pid)
    print("窗口:", wins)
    if not wins:
        print("[FAIL] 没有窗口")
        p.kill()
        return

    from PIL import Image, ImageStat
    # 逻辑坐标 -> 物理坐标（本机 150% 缩放）
    scale = ctypes.windll.shcore.GetScaleFactorForDevice(0) / 100 if hasattr(ctypes.windll, 'shcore') else 1.5
    print(f"DPI 缩放: {scale}")

    imgs = []
    for title, (l, t, r, b) in wins:
        box = (int(l * scale), int(t * scale), int(r * scale), int(b * scale))
        im = Image.Image()
        try:
            from PIL import ImageGrab
            im = ImageGrab.grab(bbox=box, all_screens=True)
        except Exception as e:
            print(f"  {title}: 截图失败 {e}")
            continue
        st = ImageStat.Stat(im.convert("L"))
        print(f"  {title}: 均值={st.mean[0]:.1f} 标准差={st.stddev[0]:.1f} "
              f"({'有内容' if st.stddev[0] > 8 else '★空白/未渲染★'})")
        imgs.append((title, im))

    if imgs:
        w = max(i.width for _, i in imgs)
        h = sum(i.height for _, i in imgs)
        canvas = Image.new("RGB", (w, h), (30, 30, 30))
        y = 0
        for _, im in imgs:
            canvas.paste(im, (0, y))
            y += im.height
        out_dir = os.path.dirname(os.path.abspath(OUT))
        os.makedirs(out_dir, exist_ok=True)
        canvas.save(OUT)
        print(f"已保存 {OUT}")

    p.kill()


if __name__ == "__main__":
    main()
