#!/usr/bin/env python3
"""Automated tap-sequence smoke test for touchHLE/HyperHLE (Windows host).

Launches the emulator with --print-fps, performs a sequence of taps at
coordinates relative to the emulator window, captures screenshots and checks
that frames keep being presented and the process stays alive.

Usage:
    python dev-scripts/ai-tap-sequence.py APP [--exe PATH] [--out DIR]
        [--step "WAIT:X,Y"]... [--final-wait SECONDS] [-- extra emulator args]

Each --step waits WAIT seconds, takes a screenshot, then taps at (X, Y),
where X and Y are fractions (0..1) of the window's client area. Use "-" for
X,Y to take a screenshot without tapping.

Requires: Pillow, pywin32.
"""

import argparse
import re
import subprocess
import sys
import threading
import time
from pathlib import Path

import win32api
import win32con
import win32gui
from PIL import ImageGrab

FPS_RE = re.compile(r"FPS: ([0-9.]+)")


def find_window(pid_hint_title="touchHLE"):
    found = []

    def cb(hwnd, _):
        if win32gui.IsWindowVisible(hwnd) and pid_hint_title in win32gui.GetWindowText(hwnd):
            found.append(hwnd)

    win32gui.EnumWindows(cb, None)
    return found[0] if found else None


def client_rect(hwnd):
    left, top, right, bottom = win32gui.GetClientRect(hwnd)
    x0, y0 = win32gui.ClientToScreen(hwnd, (left, top))
    x1, y1 = win32gui.ClientToScreen(hwnd, (right, bottom))
    return x0, y0, x1, y1


def tap(hwnd, fx, fy):
    x0, y0, x1, y1 = client_rect(hwnd)
    x = int(x0 + (x1 - x0) * fx)
    y = int(y0 + (y1 - y0) * fy)
    try:
        win32gui.SetForegroundWindow(hwnd)
    except Exception:
        pass
    win32api.SetCursorPos((x, y))
    time.sleep(0.05)
    win32api.mouse_event(win32con.MOUSEEVENTF_LEFTDOWN, 0, 0)
    time.sleep(0.12)
    win32api.mouse_event(win32con.MOUSEEVENTF_LEFTUP, 0, 0)


def screenshot(hwnd, path):
    ImageGrab.grab(bbox=client_rect(hwnd), all_screens=True).save(path)


def main():
    argv = sys.argv[1:]
    extra = []
    if "--" in argv:
        i = argv.index("--")
        argv, extra = argv[:i], argv[i + 1:]
    p = argparse.ArgumentParser()
    p.add_argument("app")
    p.add_argument("--exe", default=str(Path("target/release/touchHLE.exe").resolve()))
    p.add_argument("--out", default="tap-test-out")
    p.add_argument("--step", action="append", default=[])
    p.add_argument("--final-wait", type=float, default=10.0)
    args = p.parse_args(argv)

    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    log_path = out / "emulator.log"
    log = open(log_path, "w", encoding="utf-8", errors="replace")
    proc = subprocess.Popen(
        [args.exe, "--print-fps", *extra, args.app],
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
    )
    fps_samples = []

    start = time.time()

    def reader():
        for raw in proc.stdout:
            line = raw.decode("utf-8", "replace")
            log.write(f"[{time.time() - start:7.2f}] {line}")
            log.flush()
            m = FPS_RE.search(line)
            if m:
                fps_samples.append((time.time(), float(m.group(1))))

    threading.Thread(target=reader, daemon=True).start()

    hwnd = None
    for _ in range(60):
        hwnd = find_window()
        if hwnd:
            break
        time.sleep(0.5)
    if not hwnd:
        print("FAIL: emulator window never appeared")
        proc.kill()
        return 1

    ok = True
    for n, step in enumerate(args.step):
        wait, _, pos = step.partition(":")
        time.sleep(float(wait))
        if proc.poll() is not None:
            print(f"FAIL: emulator exited (code {proc.returncode}) before step {n}")
            ok = False
            break
        shot = out / f"step{n:02d}.png"
        screenshot(hwnd, shot)
        before = len(fps_samples)
        if pos != "-":
            fx, fy = (float(v) for v in pos.split(","))
            tap(hwnd, fx, fy)
            print(f"step {n}: screenshot {shot.name}, tap ({fx:.2f},{fy:.2f})")
        else:
            print(f"step {n}: screenshot {shot.name}")

    if ok:
        time.sleep(args.final_wait)
        screenshot(hwnd, out / "final.png")
        alive = proc.poll() is None
        tail = [f for t, f in fps_samples[-5:]]
        print(f"FPS reports: {len(fps_samples)}, last: {tail}")
        if not alive:
            print(f"FAIL: emulator exited with code {proc.returncode}")
            ok = False
        elif len(fps_samples) < 3 or all(f == 0 for f in tail):
            print("FAIL: frames are not being presented")
            ok = False
    if proc.poll() is None:
        proc.kill()
    print("PASS" if ok else "FAIL")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
