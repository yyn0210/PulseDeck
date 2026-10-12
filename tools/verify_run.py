"""实机验证脚本：启动应用 -> 等窗口出现 -> 置前 -> 截屏 -> 报告存活状态。

因为沙箱会在命令结束时回收子进程，所以启动、观察、截图必须放在同一次执行里。
"""

import ctypes
import subprocess
import sys
import time
from ctypes import wintypes
from pathlib import Path

from PIL import ImageGrab

ROOT = Path(__file__).resolve().parent.parent
EXE = str(ROOT / "src-tauri" / "target" / "release" / "pulse-deck.exe")
SHOT_DIR = ROOT / "shots"
OUT_SHOT = str(SHOT_DIR / "shot_app.png")
OUT_APP_SHOT = str(SHOT_DIR / "shot_app_crop.png")

user32 = ctypes.windll.user32

WNDENUMPROC = ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.c_void_p, ctypes.c_void_p)


def find_windows(title_part: str):
    found = []

    def cb(hwnd, _lparam):
        buf = ctypes.create_unicode_buffer(512)
        user32.GetWindowTextW(hwnd, buf, 512)
        if title_part in buf.value:
            rect = wintypes.RECT()
            user32.GetWindowRect(hwnd, ctypes.byref(rect))
            found.append(
                {
                    "hwnd": hwnd,
                    "title": buf.value,
                    "visible": bool(user32.IsWindowVisible(hwnd)),
                    "iconic": bool(user32.IsIconic(hwnd)),
                    "rect": (rect.left, rect.top, rect.right, rect.bottom),
                }
            )
        return True

    user32.EnumWindows(WNDENUMPROC(cb), 0)
    return found


def main():
    proc = subprocess.Popen([EXE])
    time.sleep(18)

    alive = proc.poll() is None
    wins = find_windows("脉盘 PulseDeck")
    print(f"alive={alive} pid={proc.pid}")
    for w in wins:
        print(f"  win {w}")

    if wins:
        target = next((w for w in wins if w["visible"]), wins[0])
        hwnd = target["hwnd"]
        user32.ShowWindow(hwnd, 5)  # SW_SHOW
        user32.SetForegroundWindow(hwnd)
        time.sleep(3)

    img = ImageGrab.grab()  # 主屏
    img.save(OUT_SHOT)
    print(f"screenshot={img.size} -> {OUT_SHOT}")

    # 再等一会儿，确认没有在采样若干轮之后退出
    time.sleep(25)
    still = proc.poll() is None
    print(f"alive_after_45s={still}")
    wins2 = find_windows("脉盘 PulseDeck")
    print(f"windows_after={len(wins2)}")

    if wins2:
        w = wins2[0]
        left, top, right, bottom = w["rect"]
        crop = img.crop(
            (
                max(0, left),
                max(0, top),
                min(img.size[0], right),
                min(img.size[1], bottom),
            )
        )
        crop.save(OUT_APP_SHOT)
        print(f"window crop {crop.size} -> {OUT_APP_SHOT}")

    sys.stdout.flush()


if __name__ == "__main__":
    main()
