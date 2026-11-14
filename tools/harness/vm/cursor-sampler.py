#!/usr/bin/env python3
"""Sample Hyprland's cursor position every 10 ms over its IPC socket.

Writes "<unix seconds> <x> <y>" per line. Reads the compositor's own state,
so it shows where the cursor really is, after Hyprland's clamping.
Usage: cursor-sampler.py OUT_FILE
"""
import os
import socket
import sys
import time

runtime = os.environ.get("XDG_RUNTIME_DIR", f"/run/user/{os.getuid()}")
sig = os.environ.get("HYPRLAND_INSTANCE_SIGNATURE") or sorted(os.listdir(f"{runtime}/hypr"))[0]
path = f"{runtime}/hypr/{sig}/.socket.sock"


def cursorpos():
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
        s.connect(path)
        s.sendall(b"cursorpos")
        data = b""
        while True:
            chunk = s.recv(256)
            if not chunk:
                break
            data += chunk
    x, y = data.decode().strip().split(",")
    return float(x), float(y)


with open(sys.argv[1], "w", buffering=1) as out:
    while True:
        try:
            x, y = cursorpos()
            out.write(f"{time.time():.6f} {x:.1f} {y:.1f}\n")
        except (OSError, ValueError):
            pass
        time.sleep(0.01)
