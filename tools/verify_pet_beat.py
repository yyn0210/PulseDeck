#!/usr/bin/env python3
"""桌面宠物心跳验证（release 专用）。

⚠️ **必须从沙箱外运行**（用户自己在终端里跑，或由用户双击启动后观察）：
从 WorkBuddy 等 AI 工具的沙箱 shell 里启动 pulse-deck.exe，沙箱会把
tsbx.dll 注入进程树，WebView2 的 browser 进程启动 ~5.5s 后 FAIL_FAST
(0x80000003) 崩溃、渲染器被带走——心跳必然「只走一拍就冻结」，
这是测试环境假象，不是应用 bug（2026-10-11 排查确认，Crashpad dump 里
能看到注入的 tsbx.dll；别家 Tauri 应用同样中招）。在沙箱里跑本脚本
必然 FAIL，结果不可信。

判据：真实窗口里跑着的 pet.js 必须持续写 pet.trace。
只要 ts_ms 每拍都在前进，说明页面定时器活着、IPC 正常。

用法：PULSEDECK_PET_TRACE=1 由本脚本自己注入，无需外部设置。
      --keep 结束后不杀进程（方便人肉看桌面）。
"""

import ctypes
import ctypes.wintypes as wt
import json
import os
import subprocess
import sys
import time

APP_DIR = os.path.join(os.environ["LOCALAPPDATA"], "PulseDeck")
EXE = sys.argv[1] if len(sys.argv) > 1 else os.path.join(
    os.path.dirname(os.path.abspath(__file__)),
    "..", "src-tauri", "target", "release", "pulse-deck.exe",
)
EXE = os.path.normpath(EXE)
TRACE = os.path.join(APP_DIR, "pet.trace")
PET_JSON = os.path.join(APP_DIR, "pet.json")
KEEP = "--keep" in sys.argv

EnumWindows = ctypes.windll.user32.EnumWindows
EnumWindowsProc = ctypes.WINFUNCTYPE(ctypes.c_bool, wt.HWND, wt.LPARAM)
GetWindowTextW = ctypes.windll.user32.GetWindowTextW
GetWindowTextLengthW = ctypes.windll.user32.GetWindowTextLengthW
IsWindowVisible = ctypes.windll.user32.IsWindowVisible
GetWindowThreadProcessId = ctypes.windll.user32.GetWindowThreadProcessId


def find_windows():
    """按进程 pid 找出它创建的所有可见窗口标题。"""
    out = []
    pid = ctypes.wintypes.DWORD()

    def cb(hwnd, _l):
        n = GetWindowTextLengthW(hwnd)
        if n == 0:
            return True
        buf = ctypes.create_unicode_buffer(n + 1)
        GetWindowTextW(hwnd, buf, n + 1)
        GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        out.append((hwnd, pid.value, buf.value))
        return True

    EnumWindows(EnumWindowsProc(cb), 0)
    return out


def read_beat():
    if not os.path.exists(TRACE):
        return None
    try:
        txt = open(TRACE, encoding="utf-8").read().strip()
    except OSError:
        return None
    if "ts_ms=" not in txt:
        return None
    try:
        ts = int(txt.split("ts_ms=")[1].split()[0])
    except (ValueError, IndexError):
        return None
    return ts, txt


def kill_stale():
    subprocess.run(
        ["taskkill", "/F", "/IM", "pulse-deck.exe"],
        capture_output=True,
    )
    time.sleep(1.0)


def main():
    if not os.path.exists(EXE):
        print(f"[FAIL] 找不到 exe: {EXE}")
        return 2
    print(f"[exe] {EXE}  ({os.path.getsize(EXE)} bytes, "
          f"{time.strftime('%Y-%m-%d %H:%M:%S', time.localtime(os.path.getmtime(EXE)))})")

    kill_stale()

    # 确保启用：这个 exe 才是被测对象，配置由它自己落盘。
    cfg = {"enabled": True, "metrics": ["cpu", "mem", "gpu"], "opacity": 1.0}
    os.makedirs(APP_DIR, exist_ok=True)
    with open(PET_JSON, "w", encoding="utf-8") as f:
        json.dump(cfg, f, indent=2)
    if os.path.exists(TRACE):
        os.remove(TRACE)
    print(f"[cfg] {json.dumps(cfg)}")

    env = dict(os.environ)
    env["PULSEDECK_PET_TRACE"] = "1"
    proc = subprocess.Popen([EXE], env=env)
    print(f"[run] pid={proc.pid}  trace=1")
    print("      等待 10s 让 WebView2 起来…")
    time.sleep(10)

    samples = []
    for i in range(4):
        b = read_beat()
        wins = [t for (_h, p, t) in find_windows() if p == proc.pid]
        pet = [t for t in wins if "宠物" in t or "pet" in t.lower()]
        if b is None:
            samples.append((i, None, pet, proc.poll()))
            print(f"  #{i}  心跳=<无>            窗口={pet or wins}  exit={proc.poll()}")
        else:
            samples.append((i, b[0], pet, proc.poll()))
            print(f"  #{i}  心跳={b[0]}  窗口={pet or '?'}  exit={proc.poll()}")
            print(f"        {b[1]}")
        if i < 3:
            time.sleep(15)

    print()
    ts = [s[1] for s in samples]
    got = [t for t in ts if t is not None]
    ok = True
    if len(got) < 2:
        print("[FAIL] 心跳样本不足（少于 2 次）→ 页面 JS 没跑起来或窗口没创建")
        ok = False
    elif len(set(got)) == 1:
        print(f"[FAIL] 心跳冻结在 {got[0]} → Chromium 仍在节流后台页")
        ok = False
    else:
        deltas = [got[i + 1] - got[i] for i in range(len(got) - 1)]
        print(f"[ OK ] 心跳持续前进，间隔(ms) = {deltas}")
        # 间隔不应远超 1s 的采样节拍；超 5s 说明被降频了。
        if max(deltas) > 5000:
            print(f"[WARN] 最大间隔 {max(deltas)}ms 偏大，可能被降频")

    if not KEEP:
        subprocess.run(["taskkill", "/PID", str(proc.pid), "/T", "/F"], capture_output=True)
        print(f"[kill] pid={proc.pid}")
    else:
        print(f"[keep] pid={proc.pid} 未杀，请手动结束")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
