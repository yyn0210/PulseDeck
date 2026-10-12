"""端到端验证：桥接进程会在父进程结束后自行收尾（残留进程修复的验收脚本）。

不弹 UAC、不建窗口、不碰真实的 pawnio.json —— 只以 `--pawnio-bridge` 身份直接拉起
刚构建出来的 release exe，用一个自己控制的「假父进程」做靶子。

跑法（项目根目录）：
    python tools/verify_bridge_exit.py
退出码 0 = 通过；非 0 = 失败（同时会尽量把桥接进程清掉）。
"""

from __future__ import annotations

import os
import subprocess
import sys
import time
from pathlib import Path

import ctypes
import ctypes.wintypes as wintypes

ROOT = Path(__file__).resolve().parent.parent
EXE = ROOT / "src-tauri" / "target" / "release" / "pulse-deck.exe"
MODULE = ROOT / "src-tauri" / "resources" / "pawnio" / "IntelMSR.bin"
OUT = Path(os.environ.get("TEMP", ".")) / "wb_bridge_exit_test.json"

CREATE_NO_WINDOW = 0x08000000
PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
SYNCHRONIZE = 0x00100000
WAIT_TIMEOUT = 258

k32 = ctypes.WinDLL("kernel32", use_last_error=True)
k32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
k32.OpenProcess.restype = wintypes.HANDLE
k32.CloseHandle.argtypes = [wintypes.HANDLE]
k32.WaitForSingleObject.argtypes = [wintypes.HANDLE, wintypes.DWORD]
k32.WaitForSingleObject.restype = wintypes.DWORD


def open_process(pid: int, access: int):
    """返回句柄（int / None）。"""
    handle = k32.OpenProcess(access, False, pid)
    return handle if handle else None


def alive_by_handle(pid: int) -> bool:
    """修复后用的判据：能等到句柄超时才算「还活着」。"""
    handle = open_process(pid, SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION)
    if not handle:
        return False
    try:
        return k32.WaitForSingleObject(handle, 0) == WAIT_TIMEOUT
    finally:
        k32.CloseHandle(handle)


def alive_by_naive_openprocess(pid: int) -> bool:
    """修复前的旧判据：能打开就算活着 —— 用来证明它为什么不可靠。"""
    handle = open_process(pid, PROCESS_QUERY_LIMITED_INFORMATION)
    if not handle:
        return False
    k32.CloseHandle(handle)
    return True


def wait_until(predicate, timeout_s: float, interval_s: float = 0.2) -> bool:
    deadline = time.monotonic() + timeout_s
    while time.monotonic() < deadline:
        if predicate():
            return True
        time.sleep(interval_s)
    return predicate()


def main() -> int:
    if not EXE.is_file():
        print(f"[FAIL] 找不到构建产物：{EXE}（先跑 npm run build）")
        return 2
    if not MODULE.is_file():
        print(f"[FAIL] 找不到温度模块：{MODULE}")
        return 2

    OUT.unlink(missing_ok=True)

    # 1) 假父进程：一个只会傻等 60 秒的 ping，PID 就是我们传的 --parent-pid
    parent = subprocess.Popen(
        [r"C:\Windows\System32\PING.EXE", "-n", "60", "127.0.0.1"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        creationflags=CREATE_NO_WINDOW,
    )
    print(f"[1] 假父进程 PID={parent.pid}")

    # 2) 用 release 产物以桥接身份拉起（非提权：读不到 MSR 没关系，本条只验生命周期）
    bridge = subprocess.Popen(
        [
            str(EXE),
            "--pawnio-bridge",
            "--module", str(MODULE),
            "--out", str(OUT),
            "--parent-pid", str(parent.pid),
        ],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        creationflags=CREATE_NO_WINDOW,
    )
    print(f"[2] 桥接进程 PID={bridge.pid}")

    failures: list[str] = []
    try:
        # 3) 桥接必须活着，并且真的在写状态文件
        if not wait_until(lambda: alive_by_handle(bridge.pid), 5.0):
            failures.append("桥接进程没能启动")
        if not wait_until(lambda: OUT.is_file() and OUT.stat().st_size > 0, 5.0):
            failures.append("桥接进程没有写出状态文件")
        else:
            print(f"[3] 状态文件已写出：{OUT.stat().st_size} 字节")

        # 4) 父进程还活着时必须继续保持（别误杀自己）
        if not alive_by_handle(bridge.pid):
            failures.append("父进程还在，桥接进程却已退出")
        else:
            print("[4] 父进程仍在运行 → 桥接进程保持存活 ✔")

        # 5) 结束父进程，桥接应在 1 拍（1s）+ 余量内自行收尾
        parent.kill()
        parent.wait()
        print(f"[5] 已结束假父进程 PID={parent.pid}")

        # 负向对照：旧的「OpenProcess 能打开就算活着」判据此刻仍然会说「活着」
        # （parent 这个 Popen 对象还持有它的进程句柄，进程对象没被回收）
        if alive_by_naive_openprocess(parent.pid):
            print("[i] 对照：旧的 OpenProcess 判据对已退出的 PID 仍返回「活着」——这正是 bug 的根因")

        if not wait_until(lambda: not alive_by_handle(bridge.pid), 6.0):
            failures.append("父进程已结束，桥接进程仍未退出（残留进程 bug 复发）")
        else:
            print("[6] 父进程结束 → 桥接进程已自行退出 ✔")
    finally:
        for proc, name in ((bridge, "桥接进程"), (parent, "假父进程")):
            if proc.poll() is None:
                print(f"[cleanup] {name} 仍在运行，强制结束")
                proc.kill()
                proc.wait()

    if failures:
        for msg in failures:
            print(f"[FAIL] {msg}")
        return 1
    print("\n全部通过：桥接进程会跟着父进程一起收尾。")
    return 0


if __name__ == "__main__":
    sys.exit(main())
