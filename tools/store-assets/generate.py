#!/usr/bin/env python3
"""Render Wear Pointer store / README marketing images.

HTML + CSS composed here, rasterised by headless Chrome/Chromium at 2x, then
downscaled with Pillow to the exact store size as RGB (no alpha).

    python3 tools/store-assets/generate.py              # everything
    python3 tools/store-assets/generate.py --only phone # phone | feature | hero | sheet
    python3 tools/store-assets/generate.py --config other.json --out /tmp/x

Requirements: Python 3.9+, Pillow, and Google Chrome or a Chromium build
(set CHROME_BIN to override discovery). No network access needed.
"""
from __future__ import annotations

import argparse
import base64
import glob
import html
import io
import json
import math
import os
import re
import shutil
import subprocess
import sys
import tempfile
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

HERE = Path(__file__).resolve().parent

COLORS = {
    "emerald": "#10B981",
    "sky": "#38BDF8",
    "amber": "#F59E0B",
    "rose": "#F43F5E",
    "white": "#F8FAFC",
}

# --------------------------------------------------------------------------- utils


def color(name: str) -> str:
    return COLORS.get(name, name)


def rgba(hex_color: str, a: float) -> str:
    h = hex_color.lstrip("#")
    r, g, b = int(h[0:2], 16), int(h[2:4], 16), int(h[4:6], 16)
    return f"rgba({r},{g},{b},{a})"


def loc(value, locale: str):
    """Pick the locale string from a str | {locale: str} field."""
    if isinstance(value, dict):
        if locale in value:
            return value[locale]
        if "en-US" in value:
            return value["en-US"]
        return next(iter(value.values()))
    return value


def esc(text: str) -> str:
    return html.escape(text or "")


def rich(text: str) -> str:
    """Escape, then turn *word* into an accent-tinted <em>."""
    out = esc(text)
    out = re.sub(r"\*(.+?)\*", r"<em>\1</em>", out)
    # Tighten the full-width middle dot (Chrome ignores "palt" for PingFang).
    out = out.replace("・", '<span class="cjk-dot">・</span>')
    return out.replace("\n", "<br>")


def find_chrome() -> str:
    env = os.environ.get("CHROME_BIN")
    if env and Path(env).exists():
        return env
    candidates = [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
    ]
    home = Path.home()
    candidates += sorted(
        glob.glob(str(home / "Library/Caches/ms-playwright/chromium-*/chrome-mac*/*.app/Contents/MacOS/*")),
        reverse=True,
    )
    candidates += sorted(glob.glob(str(home / ".cache/ms-playwright/chromium-*/chrome-linux*/chrome")), reverse=True)
    for name in ("google-chrome", "google-chrome-stable", "chromium", "chromium-browser", "chrome"):
        p = shutil.which(name)
        if p:
            candidates.append(p)
    for c in candidates:
        if c and Path(c).exists():
            return c
    sys.exit("No Chrome/Chromium found. Install Google Chrome or set CHROME_BIN.")


_img_cache: dict[tuple, str] = {}


def data_uri(path: Path, size: int | None = None, circle: bool = False) -> str:
    key = (str(path), size, circle)
    if key in _img_cache:
        return _img_cache[key]
    im = Image.open(path).convert("RGB")
    if size:
        im = im.resize((size, size), Image.LANCZOS)
    buf = io.BytesIO()
    im.save(buf, "PNG", optimize=False)
    uri = "data:image/png;base64," + base64.b64encode(buf.getvalue()).decode()
    _img_cache[key] = uri
    return uri


# --------------------------------------------------------------------------- icons

ICONS = {
    # 24x24 stroke icons (neutral glyphs, not vendor logos)
    "command": '<path d="M9 6a3 3 0 1 0-3 3h12a3 3 0 1 0-3-3v12a3 3 0 1 0 3-3H6a3 3 0 1 0 3 3V6z"/>',
    "window": '<rect x="3" y="4.5" width="18" height="15" rx="2.5"/><path d="M3 9h18"/><path d="M6.5 6.8h.01M9 6.8h.01"/>',
    "terminal": '<rect x="3" y="4.5" width="18" height="15" rx="2.5"/><path d="M7 10l3 2.5L7 15M12.5 15.5H17"/>',
    "tap": '<circle cx="12" cy="12" r="3"/><circle cx="12" cy="12" r="7.5" opacity=".55"/>',
    "hold": '<circle cx="12" cy="12" r="3" fill="currentColor"/><circle cx="12" cy="12" r="8"/><path d="M12 4a8 8 0 0 1 8 8" stroke-width="3"/>',
    "bezel": '<path d="M20 12a8 8 0 1 1-2.34-5.66"/><path d="M20 4v4h-4"/><path d="M12 9v6M9.5 11.5 12 9l2.5 2.5"/>',
    "pause": '<path d="M9 6v12M15 6v12" stroke-width="3"/>',
    "center": '<circle cx="12" cy="12" r="6"/><circle cx="12" cy="12" r="1.6" fill="currentColor"/><path d="M12 2.5v4M12 17.5v4M2.5 12h4M17.5 12h4"/>',
    "gyro": '<ellipse cx="12" cy="12" rx="9" ry="4" transform="rotate(-25 12 12)"/><ellipse cx="12" cy="12" rx="4" ry="9" transform="rotate(-25 12 12)"/><circle cx="12" cy="12" r="1.8" fill="currentColor"/>',
    "stop": '<rect x="6.5" y="6.5" width="11" height="11" rx="2.5" fill="currentColor"/>',
    "check": '<path d="M5 12.5l4.5 4.5L19 7.5" stroke-width="2.6"/>',
    "lock": '<rect x="5" y="11" width="14" height="9.5" rx="2.5"/><path d="M8 11V8a4 4 0 0 1 8 0v3"/>',
}


def icon(name: str, size: int, col: str, stroke: float = 2.0) -> str:
    body = ICONS.get(name, ICONS["tap"])
    return (
        f'<svg width="{size}" height="{size}" viewBox="0 0 24 24" fill="none" stroke="{col}" '
        f'stroke-width="{stroke}" stroke-linecap="round" stroke-linejoin="round" style="color:{col}">{body}</svg>'
    )


# --------------------------------------------------------------------------- base css

BASE_CSS = """
:root{--em:#10B981;--sky:#38BDF8;--amber:#F59E0B;--rose:#F43F5E;--ink:#F8FAFC;--muted:#9AA8BC;--dim:#64748B;}
*{box-sizing:border-box;margin:0;padding:0}
html,body{width:var(--W);height:var(--H);overflow:hidden}
body{position:relative;color:var(--ink);background:linear-gradient(180deg,#09111E 0%,#0B1220 55%,#0A101C 100%);
 font-family:-apple-system,BlinkMacSystemFont,"SF Pro Display","Inter","Helvetica Neue","PingFang TC",sans-serif;
 -webkit-font-smoothing:antialiased;text-rendering:geometricPrecision}
body.zh{font-family:-apple-system,BlinkMacSystemFont,"PingFang TC","Heiti TC",sans-serif}
.mono{font-family:ui-monospace,"SF Mono",Menlo,monospace}
.abs{position:absolute}
.grid{position:absolute;inset:0;background-image:linear-gradient(rgba(148,163,184,.07) 1px,transparent 1px),linear-gradient(90deg,rgba(148,163,184,.07) 1px,transparent 1px);
 background-size:var(--gs,54px) var(--gs,54px);background-position:center top;
 -webkit-mask-image:radial-gradient(ellipse 70% 60% at 50% 45%,#000 0%,rgba(0,0,0,.55) 45%,transparent 80%)}
.glow{position:absolute;border-radius:50%;filter:blur(var(--b,90px));pointer-events:none}
.vignette{position:absolute;inset:0;background:radial-gradient(ellipse 85% 75% at 50% 45%,transparent 55%,rgba(3,6,12,.55) 100%);pointer-events:none}
em{font-style:normal;color:var(--accent);}
.cjk-dot{display:inline-block;margin:0 -.2em}

/* ---------- header ---------- */
.header{position:absolute;left:var(--mx);right:var(--mx);top:var(--top);text-align:center}
.eyebrow{display:inline-flex;align-items:center;gap:14px;padding:12px 24px 12px 20px;border-radius:999px;
 border:1.5px solid color-mix(in srgb,var(--accent) 45%,transparent);background:color-mix(in srgb,var(--accent) 10%,transparent);
 color:var(--accent);font-size:23px;font-weight:600;letter-spacing:.16em}
.eyebrow i{width:10px;height:10px;border-radius:50%;background:var(--accent);box-shadow:0 0 14px var(--accent)}
.zh .eyebrow{letter-spacing:.22em;font-size:24px}
.headline{margin-top:34px;font-size:92px;line-height:1.05;font-weight:800;letter-spacing:-.024em;text-wrap:balance}
.zh .headline{font-size:94px;font-weight:600;letter-spacing:.01em;line-height:1.14;font-feature-settings:"palt"}
.sub{margin-top:26px;font-size:36px;line-height:1.35;color:var(--muted);font-weight:450;letter-spacing:-.01em;text-wrap:balance}
.zh .sub{font-size:36px;letter-spacing:.03em;font-weight:400}

/* ---------- watch ---------- */
.watch{position:absolute;width:var(--d);height:var(--d)}
.watch .halo{position:absolute;left:50%;top:50%;width:calc(var(--d)*1.35);height:calc(var(--d)*1.35);transform:translate(-50%,-50%);
 border-radius:50%;background:radial-gradient(circle,color-mix(in srgb,var(--glow) 42%,transparent) 0%,color-mix(in srgb,var(--glow) 12%,transparent) 42%,transparent 68%);filter:blur(10px)}
.watch .strap{position:absolute;left:50%;width:calc(var(--d)*.50);height:calc(var(--d)*.34);transform:translateX(-50%);
 background:
  repeating-linear-gradient(0deg,rgba(255,255,255,.035) 0 2px,transparent 2px 9px),
  linear-gradient(90deg,#0b0e13 0%,#1c222b 14%,#272e39 50%,#1c222b 86%,#0b0e13 100%);
 box-shadow:inset 0 0 0 1px rgba(255,255,255,.04)}
.watch .strap.top{bottom:calc(var(--d)*.84);border-radius:calc(var(--d)*.06) calc(var(--d)*.06) 0 0}
.watch .strap.bottom{top:calc(var(--d)*.84);border-radius:0 0 calc(var(--d)*.06) calc(var(--d)*.06)}
.watch .strap::after{content:"";position:absolute;inset:0;border-radius:inherit;box-shadow:inset 0 0 0 1.5px rgba(255,255,255,.05)}
.watch .strap.top::after{background:linear-gradient(to bottom,rgba(0,0,0,.45),transparent 60%),linear-gradient(to bottom,transparent 0,transparent 18%,rgba(255,255,255,.05) 18.5%,transparent 20%)}
.watch .strap.bottom::after{background:linear-gradient(to top,rgba(0,0,0,.45),transparent 60%),linear-gradient(to top,transparent 0,transparent 18%,rgba(255,255,255,.05) 18.5%,transparent 20%)}
.watch .lug{position:absolute;left:50%;width:calc(var(--d)*.62);height:calc(var(--d)*.16);transform:translateX(-50%);
 background:linear-gradient(90deg,#1a1f27,#3c434e 30%,#4b525e 50%,#3c434e 70%,#1a1f27)}
.watch .lug.top{top:calc(var(--d)*.0);border-radius:40% 40% 0 0/80% 80% 0 0}
.watch .lug.bottom{bottom:calc(var(--d)*.0);border-radius:0 0 40% 40%/0 0 80% 80%}
.watch .btn{position:absolute;width:calc(var(--d)*.07);height:calc(var(--d)*.115);border-radius:calc(var(--d)*.018);
 background:linear-gradient(90deg,#20252d,#59606c 55%,#2a3038);transform:translate(-50%,-50%) rotate(var(--r));box-shadow:0 2px 6px rgba(0,0,0,.6)}
.watch .case{position:absolute;inset:0;border-radius:50%;
 background:conic-gradient(from 200deg,#4a515c,#1b2028 14%,#626a77 30%,#232830 47%,#565e6a 63%,#191d24 80%,#4a515c);
 box-shadow:0 0 0 1.5px #06080c,0 calc(var(--d)*.06) calc(var(--d)*.12) rgba(0,0,0,.65),
  inset 0 2px 2px rgba(255,255,255,.35),inset 0 -3px 5px rgba(0,0,0,.7)}
.watch .bezel{position:absolute;inset:calc(var(--d)*.038);border-radius:50%;
 background:radial-gradient(circle,#0d1016 60%,#151a21 100%);box-shadow:inset 0 2px 6px rgba(0,0,0,.9),0 0 0 1px rgba(0,0,0,.6),0 1px 0 rgba(255,255,255,.12)}
.watch .bezel::before{content:"";position:absolute;inset:0;border-radius:50%;
 background:repeating-conic-gradient(from -.5deg,rgba(226,232,240,.16) 0 .5deg,transparent .5deg 6deg);
 -webkit-mask-image:radial-gradient(circle closest-side,transparent 88%,#000 89%,#000 97%,transparent 98%)}
.watch .screen{position:absolute;inset:calc(var(--d)*.085);border-radius:50%;overflow:hidden;background:#000;
 box-shadow:0 0 0 2px #000,0 0 0 3px rgba(255,255,255,.06)}
.watch .screen img{width:100%;height:100%;display:block}
.watch .glass{position:absolute;inset:calc(var(--d)*.085);border-radius:50%;
 background:linear-gradient(140deg,rgba(255,255,255,.13) 0%,rgba(255,255,255,.03) 30%,transparent 46%),
  radial-gradient(circle at 50% 50%,transparent 66%,rgba(0,0,0,.35) 100%);pointer-events:none}

/* ---------- laptop ---------- */
.laptop{position:absolute;width:var(--w)}
.laptop .lid{position:relative;width:var(--w);height:calc(var(--w)*.6415);border-radius:calc(var(--w)*.032) calc(var(--w)*.032) calc(var(--w)*.012) calc(var(--w)*.012);
 background:#05070b;padding:calc(var(--w)*.022);
 box-shadow:0 0 0 2px #39404b,0 0 0 3px #11151b,0 calc(var(--w)*.05) calc(var(--w)*.1) rgba(0,0,0,.55),0 0 calc(var(--w)*.18) rgba(56,189,248,.10)}
.laptop .lid .cam{position:absolute;top:calc(var(--w)*.008);left:50%;width:calc(var(--w)*.008);height:calc(var(--w)*.008);border-radius:50%;background:#1e2530;transform:translateX(-50%)}
.laptop .display{position:relative;width:100%;height:100%;border-radius:calc(var(--w)*.006);overflow:hidden;background:#0b1424}
.laptop .display svg{position:absolute;inset:0;width:100%;height:100%;display:block}
.laptop .display::after{content:"";position:absolute;inset:0;background:linear-gradient(125deg,rgba(255,255,255,.07) 0%,transparent 35%);pointer-events:none}
.laptop .base{position:relative;margin-left:calc(var(--w)*-.06);width:calc(var(--w)*1.12);height:calc(var(--w)*.032);
 background:linear-gradient(180deg,#5a626e 0%,#2b313a 35%,#151920 100%);border-radius:0 0 calc(var(--w)*.05) calc(var(--w)*.05)/0 0 calc(var(--w)*.03) calc(var(--w)*.03);
 box-shadow:0 calc(var(--w)*.03) calc(var(--w)*.05) rgba(0,0,0,.6)}
.laptop .base::before{content:"";position:absolute;left:50%;top:0;width:18%;height:38%;transform:translateX(-50%);background:#20252d;border-radius:0 0 12px 12px}

/* ---------- chips / cards ---------- */
.card{background:linear-gradient(180deg,rgba(30,41,59,.62),rgba(15,23,42,.62));border:1.5px solid rgba(148,163,184,.16);
 box-shadow:0 20px 50px rgba(0,0,0,.35),inset 0 1px 0 rgba(255,255,255,.06);backdrop-filter:blur(6px)}
.pill{display:inline-flex;align-items:center;gap:12px;border-radius:999px}
"""


def page(w: int, h: int, body: str, locale: str, accent: str, extra_css: str = "") -> str:
    zh = locale.lower().startswith("zh")
    return f"""<!doctype html><html lang="{locale}"><head><meta charset="utf-8">
<style>{BASE_CSS}
:root{{--W:{w}px;--H:{h}px;--accent:{accent}}}
{extra_css}</style></head><body class="{'zh' if zh else 'en'}">{body}</body></html>"""


# --------------------------------------------------------------------------- components


def watch_html(screen_uri: str, d: int, cx: float, cy: float, glow: str, halo: bool = True) -> str:
    left, top = cx - d / 2, cy - d / 2
    btns = ""
    for ang in (28, -28):
        a = math.radians(ang)
        bx = d / 2 + d * 0.488 * math.cos(a)
        by = d / 2 - d * 0.488 * math.sin(a)
        btns += f'<div class="btn" style="left:{bx:.1f}px;top:{by:.1f}px;--r:{-ang}deg"></div>'
    return f"""<div class="watch" style="--d:{d}px;--glow:{glow};left:{left:.1f}px;top:{top:.1f}px">
 {'<div class="halo"></div>' if halo else ''}
 <div class="strap top"></div><div class="strap bottom"></div>
 {btns}
 <div class="case"></div><div class="bezel"></div>
 <div class="screen"><img src="{screen_uri}"></div><div class="glass"></div>
</div>"""


def _bez(p0, p1, p2, p3, t):
    u = 1 - t
    x = u**3 * p0[0] + 3 * u * u * t * p1[0] + 3 * u * t * t * p2[0] + t**3 * p3[0]
    y = u**3 * p0[1] + 3 * u * u * t * p1[1] + 3 * u * t * t * p2[1] + t**3 * p3[1]
    return x, y


CURSOR_PATH = "M0 0 L0 46 L11 35.5 L18.5 52 L26 48.5 L18.8 32.5 L33 32.5 Z"


def cursor_svg(x, y, scale=1.5, opacity=1.0, glow=None, uid="c"):
    halo = ""
    if glow:
        halo = (
            f'<circle cx="{x}" cy="{y}" r="58" fill="{glow}" opacity=".35" filter="url(#blur18{uid})"/>'
            f'<circle cx="{x}" cy="{y}" r="34" fill="none" stroke="{glow}" stroke-width="3" opacity=".75"/>'
        )
    return (
        f'{halo}<g transform="translate({x} {y}) scale({scale})" opacity="{opacity}">'
        f'<path d="{CURSOR_PATH}" fill="#000" opacity=".35" transform="translate(2 4)"/>'
        f'<path d="{CURSOR_PATH}" fill="#F8FAFC" stroke="#0B1220" stroke-width="2.6" stroke-linejoin="round"/></g>'
    )


def window_svg(x, y, w, h, kind="editor", accent="#10B981", border=None, uid="w", shadow=True):
    out = []
    if shadow:
        out.append(f'<rect x="{x}" y="{y+18}" width="{w}" height="{h}" rx="20" fill="#000" opacity=".45" filter="url(#blur18{uid})"/>')
    stroke = border or "rgba(255,255,255,.10)"
    sw = 4 if border else 2
    out.append(f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="20" fill="#111a2b" stroke="{stroke}" stroke-width="{sw}"/>')
    out.append(f'<path d="M{x} {y+20} a20 20 0 0 1 20 -20 h{w-40} a20 20 0 0 1 20 20 v28 h-{w} z" fill="#18233a"/>')
    for i, c in enumerate(("#F43F5E", "#F59E0B", "#10B981")):
        out.append(f'<circle cx="{x+30+i*24}" cy="{y+24}" r="7" fill="{c}"/>')
    out.append(f'<rect x="{x+w/2-80}" y="{y+18}" width="160" height="12" rx="6" fill="rgba(255,255,255,.14)"/>')
    cy = y + 80
    if kind == "editor":
        widths = [0.62, 0.48, 0.74, 0.36, 0.58, 0.68, 0.30, 0.52, 0.44]
        cols = ["#38BDF8", "rgba(255,255,255,.22)", "rgba(255,255,255,.16)", accent, "rgba(255,255,255,.22)", "rgba(255,255,255,.16)", "#F59E0B", "rgba(255,255,255,.18)", "rgba(255,255,255,.14)"]
        for i, (fw, c) in enumerate(zip(widths, cols)):
            ly = cy + i * 38
            if ly > y + h - 40:
                break
            indent = 34 if i in (2, 3, 5, 6) else 0
            out.append(f'<rect x="{x+34+indent}" y="{ly}" width="{(w-90)*fw}" height="14" rx="7" fill="{c}" opacity=".85"/>')
    elif kind == "gallery":
        cols, rows = 3, 2
        gw = (w - 34 * 2 - 20 * (cols - 1)) / cols
        gh = (h - 80 - 34 - 20 * (rows - 1)) / rows
        tints = ["#10B981", "#38BDF8", "#6366F1", "#F59E0B", "#38BDF8", "#10B981"]
        for r in range(rows):
            for c in range(cols):
                gx = x + 34 + c * (gw + 20)
                gy = y + 72 + r * (gh + 20)
                t = tints[(r * cols + c) % len(tints)]
                out.append(f'<rect x="{gx}" y="{gy}" width="{gw}" height="{gh}" rx="12" fill="{t}" opacity=".22"/>')
                out.append(f'<rect x="{gx+16}" y="{gy+gh-30}" width="{gw*0.5}" height="10" rx="5" fill="rgba(255,255,255,.35)"/>')
    elif kind == "chart":
        bars = [0.35, 0.55, 0.42, 0.7, 0.6, 0.85, 0.66, 0.92]
        bw = (w - 68) / len(bars)
        base_y = y + h - 40
        for i, b in enumerate(bars):
            bh = (h - 140) * b
            out.append(f'<rect x="{x+34+i*bw+6}" y="{base_y-bh}" width="{bw-12}" height="{bh}" rx="8" fill="{accent if i==len(bars)-1 else "#38BDF8"}" opacity="{0.9 if i==len(bars)-1 else 0.35}"/>')
    return "".join(out)


def desktop_svg(variant: str, accent: str, uid: str = "d") -> str:
    """1600x1000 desktop scene."""
    defs = f"""<defs>
<linearGradient id="wp{uid}" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#0f1b31"/><stop offset="1" stop-color="#081020"/></linearGradient>
<filter id="blur120{uid}" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="120"/></filter>
<filter id="blur18{uid}" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="18"/></filter>
<filter id="blur8{uid}" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="8"/></filter>
</defs>"""
    s = [defs, f'<rect width="1600" height="1000" fill="url(#wp{uid})"/>']
    s.append(f'<circle cx="260" cy="860" r="420" fill="{accent}" opacity=".22" filter="url(#blur120{uid})"/>')
    s.append(f'<circle cx="1380" cy="160" r="380" fill="#38BDF8" opacity=".16" filter="url(#blur120{uid})"/>')
    s.append(f'<circle cx="900" cy="560" r="300" fill="#6366F1" opacity=".08" filter="url(#blur120{uid})"/>')
    # menubar
    s.append('<rect width="1600" height="40" fill="rgba(255,255,255,.06)"/>')
    s.append('<circle cx="34" cy="20" r="9" fill="rgba(255,255,255,.55)"/>')
    for i, wdt in enumerate((60, 46, 52, 70)):
        s.append(f'<rect x="{66+i*90}" y="14" width="{wdt}" height="12" rx="6" fill="rgba(255,255,255,.28)"/>')
    for i, wdt in enumerate((28, 28, 90)):
        s.append(f'<rect x="{1340+i*60 + (0 if i<2 else 0)}" y="14" width="{wdt}" height="12" rx="6" fill="rgba(255,255,255,.28)"/>')
    # dock
    s.append('<rect x="520" y="902" width="560" height="72" rx="24" fill="rgba(255,255,255,.08)" stroke="rgba(255,255,255,.10)" stroke-width="2"/>')
    dock_cols = ["#38BDF8", "#10B981", "#6366F1", "#F59E0B", "#F43F5E", "#94A3B8", "#38BDF8", "#10B981"]
    for i, c in enumerate(dock_cols):
        s.append(f'<rect x="{544+i*66}" y="914" width="48" height="48" rx="13" fill="{c}" opacity=".55"/>')

    if variant == "drag":
        s.append(window_svg(980, 120, 520, 360, "gallery", accent, uid=uid))
        # ghost (original position)
        gx, gy, gw, gh = 140, 150, 600, 400
        s.append(f'<rect x="{gx}" y="{gy}" width="{gw}" height="{gh}" rx="20" fill="{accent}" fill-opacity=".05" stroke="{accent}" stroke-opacity=".55" stroke-width="3" stroke-dasharray="14 12"/>')
        nx, ny = 700, 440
        p0 = (gx + 250, gy + 24)
        p3 = (nx + 250, ny + 24)
        p1 = (p0[0] + 160, p0[1] - 40)
        p2 = (p3[0] - 260, p3[1] - 60)
        s.append(trail_svg(p0, p1, p2, p3, accent, uid))
        s.append(window_svg(nx, ny, gw, gh, "chart", accent, border=accent, uid=uid))
        s.append(f'<rect x="{nx-6}" y="{ny-6}" width="{gw+12}" height="{gh+12}" rx="24" fill="none" stroke="{accent}" stroke-width="10" opacity=".25" filter="url(#blur8{uid})"/>')
        s.append(cursor_svg(p3[0], p3[1], 1.6, 1, accent, uid))
    else:
        s.append(window_svg(120, 110, 760, 500, "editor", accent, uid=uid))
        s.append(window_svg(960, 250, 520, 380, "gallery", accent, uid=uid))
        p0, p1, p2, p3 = (230, 900), (420, 760), (560, 420), (1130, 330)
        s.append(trail_svg(p0, p1, p2, p3, accent, uid))
        s.append(cursor_svg(p3[0], p3[1], 1.7, 1, accent, uid))
    return f'<svg viewBox="0 0 1600 1000" preserveAspectRatio="xMidYMid slice" xmlns="http://www.w3.org/2000/svg">{"".join(s)}</svg>'


def trail_svg(p0, p1, p2, p3, accent, uid):
    d = f"M{p0[0]} {p0[1]} C{p1[0]} {p1[1]} {p2[0]} {p2[1]} {p3[0]} {p3[1]}"
    gid = f"tr{uid}{int(p0[0])}"
    out = [
        f'<linearGradient id="{gid}" gradientUnits="userSpaceOnUse" x1="{p0[0]}" y1="{p0[1]}" x2="{p3[0]}" y2="{p3[1]}">'
        f'<stop offset="0" stop-color="{accent}" stop-opacity="0"/><stop offset=".55" stop-color="{accent}" stop-opacity=".55"/>'
        f'<stop offset="1" stop-color="{accent}" stop-opacity="1"/></linearGradient>',
        f'<path d="{d}" fill="none" stroke="url(#{gid})" stroke-width="34" stroke-linecap="round" opacity=".45" filter="url(#blur18{uid})"/>',
        f'<path d="{d}" fill="none" stroke="url(#{gid})" stroke-width="9" stroke-linecap="round"/>',
    ]
    for t, op, sc in ((0.5, 0.12, 1.25), (0.68, 0.2, 1.35), (0.84, 0.32, 1.45)):
        x, y = _bez(p0, p1, p2, p3, t)
        out.append(cursor_svg(round(x, 1), round(y, 1), sc, op, None, uid))
    for t in (0.2, 0.34):
        x, y = _bez(p0, p1, p2, p3, t)
        out.append(f'<circle cx="{x:.1f}" cy="{y:.1f}" r="6" fill="{accent}" opacity=".5"/>')
    return "".join(out)


def laptop_html(w: int, left: float, top: float, variant: str, accent: str, uid: str = "d") -> str:
    return f"""<div class="laptop" style="--w:{w}px;left:{left}px;top:{top}px">
 <div class="lid"><div class="cam"></div><div class="display">{desktop_svg(variant, accent, uid)}</div></div>
 <div class="base"></div></div>"""


def background(w: int, h: int, accent: str, variant: str = "phone") -> str:
    if variant == "phone":
        return f"""<div class="grid"></div>
<div class="glow" style="left:{-w*0.25}px;top:{h*0.42}px;width:{w*0.95}px;height:{w*0.95}px;background:{rgba(accent,.20) if accent.startswith('#') else accent};--b:140px"></div>
<div class="glow" style="left:{w*0.45}px;top:{h*0.18}px;width:{w*0.8}px;height:{w*0.8}px;background:rgba(56,189,248,.12);--b:150px"></div>
<div class="glow" style="left:{w*0.15}px;top:{-w*0.45}px;width:{w*0.7}px;height:{w*0.6}px;background:{rgba(accent,.10)};--b:120px"></div>
<div class="vignette"></div>"""
    return f"""<div class="grid" style="--gs:48px"></div>
<div class="glow" style="left:{w*0.40}px;top:{h*0.05}px;width:{h*1.1}px;height:{h*1.1}px;background:{rgba(accent,.18)};--b:140px"></div>
<div class="glow" style="left:{w*0.70}px;top:{-h*0.3}px;width:{h*0.9}px;height:{h*0.9}px;background:rgba(56,189,248,.14);--b:140px"></div>
<div class="glow" style="left:{-w*0.1}px;top:{h*0.55}px;width:{h*0.9}px;height:{h*0.9}px;background:rgba(99,102,241,.08);--b:140px"></div>
<div class="vignette"></div>"""


# --------------------------------------------------------------------------- phone slides

PHONE_CSS = """
body{display:flex;flex-direction:column;padding:118px 0 56px}
.header{position:relative;left:auto;right:auto;top:auto;margin:0 70px;flex:none}
.stage{position:relative;flex:1;display:flex;align-items:center;justify-content:center}
.comp{position:relative;flex:none;width:1080px;height:1300px}
.rowlist{position:absolute;display:flex;flex-direction:column;gap:22px}
.row{display:flex;align-items:center;gap:20px;padding:22px 22px;border-radius:30px}
.row .ic{width:64px;height:64px;border-radius:19px;display:grid;place-items:center;flex:none}
.row .t{font-size:34px;font-weight:700;letter-spacing:-.01em;line-height:1.1;white-space:nowrap}
.row .s{margin-top:7px;font-size:19px;color:var(--muted);letter-spacing:0;white-space:nowrap}
.zh .row .t{font-weight:600}
.link{position:absolute;display:inline-flex;align-items:center;gap:14px;padding:14px 24px;border-radius:999px;font-size:21px;font-weight:600;white-space:nowrap;
 letter-spacing:.04em;color:#E2E8F0;background:rgba(15,23,42,.8);border:1.5px solid color-mix(in srgb,var(--lc) 50%,transparent);box-shadow:0 0 30px color-mix(in srgb,var(--lc) 22%,transparent)}
.link i{width:10px;height:10px;border-radius:50%;background:var(--lc);box-shadow:0 0 12px var(--lc)}
.stats{position:absolute;left:60px;right:60px;display:grid;grid-template-columns:repeat(3,1fr);gap:28px}
.stat{border-radius:32px;padding:34px 20px 30px;text-align:center}
.stat .v{font-size:64px;font-weight:800;letter-spacing:-.03em;line-height:1}
.stat .v small{font-size:30px;font-weight:700;color:var(--accent);margin-left:6px;letter-spacing:0}
.stat .l{margin-top:16px;font-size:20px;letter-spacing:.14em;color:var(--muted)}
.zh .stat .l{letter-spacing:.12em;font-size:23px}
.actions{position:absolute;left:60px;right:60px;display:grid;grid-template-columns:repeat(3,1fr);gap:28px}
.action{border-radius:32px;padding:32px 26px 32px;text-align:left}
.action .ic{width:72px;height:72px;border-radius:22px;display:grid;place-items:center}
.action .t{margin-top:24px;font-size:36px;font-weight:700}
.action .d{margin-top:10px;font-size:23px;line-height:1.35;color:var(--muted);text-wrap:balance}
.zh .action .t{font-weight:600}
.dpi{position:absolute;left:60px;right:60px;border-radius:34px;padding:40px 52px 36px}
.dpi .track{position:relative;height:14px;border-radius:7px;background:rgba(148,163,184,.18);margin:78px 0 0}
.dpi .fill{position:absolute;left:0;top:0;bottom:0;border-radius:7px;background:linear-gradient(90deg,rgba(16,185,129,.35),var(--accent));box-shadow:0 0 24px rgba(16,185,129,.5)}
.dpi .knob{position:absolute;top:50%;width:40px;height:40px;border-radius:50%;background:#F8FAFC;transform:translate(-50%,-50%);box-shadow:0 0 0 8px rgba(16,185,129,.35),0 0 30px var(--accent)}
.dpi .bubble{position:absolute;bottom:42px;transform:translateX(-50%);padding:10px 20px;border-radius:14px;background:var(--accent);color:#04120c;font-size:28px;font-weight:800;white-space:nowrap}
.dpi .ticks{display:flex;justify-content:space-between;margin-top:26px;font-size:22px;color:var(--muted)}
.dpi .ticks b{color:var(--ink);font-weight:600}
.dpi .cap{margin-top:18px;text-align:center;font-size:20px;letter-spacing:.16em;color:var(--dim)}
.os{position:absolute;left:60px;right:60px;display:grid;grid-template-columns:repeat(3,1fr);gap:28px}
.osc{border-radius:34px;padding:38px 24px 34px;text-align:center}
.osc .ic{width:96px;height:96px;margin:0 auto;border-radius:28px;display:grid;place-items:center;background:rgba(56,189,248,.12);border:1.5px solid rgba(56,189,248,.35)}
.osc .t{margin-top:24px;font-size:38px;font-weight:700}
.osc .s{margin-top:12px;font-size:19px;line-height:1.5;color:var(--muted)}
.points{position:absolute;left:110px;right:110px;display:flex;flex-direction:column;gap:26px}
.point{display:flex;align-items:center;gap:24px;font-size:33px;font-weight:500;color:#E2E8F0}
.point .ck{width:52px;height:52px;border-radius:50%;flex:none;display:grid;place-items:center;background:rgba(16,185,129,.15);border:1.5px solid rgba(16,185,129,.5)}
.foot{position:absolute;left:0;right:0;text-align:center;font-size:22px;letter-spacing:.24em;color:var(--dim)}
.appicon{position:absolute;border-radius:22.5%;overflow:hidden;box-shadow:0 0 0 2px rgba(16,185,129,.35),0 30px 80px rgba(0,0,0,.6),0 0 140px rgba(16,185,129,.35)}
.appicon img{width:100%;height:100%;display:block}
.arcs{position:absolute;pointer-events:none}
"""


def header_html(text: dict, top: int = 120, mx: int = 70) -> str:
    sub = text.get("sub") or ""
    return f"""<div class="header" style="--top:{top}px;--mx:{mx}px">
 <div class="eyebrow mono"><i></i>{esc(text.get('eyebrow',''))}</div>
 <div class="headline">{rich(text.get('headline',''))}</div>
 {f'<div class="sub">{rich(sub)}</div>' if sub else ''}
</div>"""


def rows_html(rows, locale, left, top, width):
    items = []
    for r in rows:
        c = color(r.get("color", "sky"))
        items.append(
            f'<div class="row card"><div class="ic" style="background:{rgba(c,.13)};border:1.5px solid {rgba(c,.4)}">{icon(r.get("icon","tap"),38,c,2.1)}</div>'
            f'<div><div class="t">{esc(loc(r["title"],locale))}</div><div class="s mono">{esc(loc(r.get("sub",""),locale))}</div></div></div>'
        )
    return f'<div class="rowlist" style="left:{left}px;top:{top}px;width:{width}px">{"".join(items)}</div>'


def arcs_svg(kind: str, cx: float, cy: float, r: float, accent: str) -> str:
    size = r * 2 + 200
    ox, oy = cx - size / 2, cy - size / 2
    c = size / 2
    out = [f'<defs><marker id="ah{kind}" viewBox="0 0 10 10" refX="5" refY="5" markerWidth="4.2" markerHeight="4.2" orient="auto-start-reverse">'
           f'<path d="M0 0 L10 5 L0 10 z" fill="{accent}"/></marker></defs>']

    def arc(a0, a1, rad, col, sw, op, both=True, dash=None):
        x0, y0 = c + rad * math.cos(math.radians(a0)), c - rad * math.sin(math.radians(a0))
        x1, y1 = c + rad * math.cos(math.radians(a1)), c - rad * math.sin(math.radians(a1))
        large = 1 if abs(a1 - a0) > 180 else 0
        sweep = 0 if a1 > a0 else 1
        m = f'marker-end="url(#ah{kind})"' + (f' marker-start="url(#ah{kind})"' if both else "")
        da = f'stroke-dasharray="{dash}"' if dash else ""
        return (f'<path d="M{x0:.1f} {y0:.1f} A{rad} {rad} 0 {large} {sweep} {x1:.1f} {y1:.1f}" fill="none" stroke="{col}" '
                f'stroke-width="{sw}" stroke-linecap="round" opacity="{op}" {m} {da}/>')

    if kind == "motion":
        out.append(arc(205, 155, r + 40, "#38BDF8", 7, 0.9).replace(f"url(#ah{kind})", f"url(#ahs{kind})"))
        out.append(arc(-25, 25, r + 40, "#38BDF8", 7, 0.9).replace(f"url(#ah{kind})", f"url(#ahs{kind})"))
        out.insert(1, f'<defs><marker id="ahs{kind}" viewBox="0 0 10 10" refX="5" refY="5" markerWidth="4.2" markerHeight="4.2" orient="auto-start-reverse"><path d="M0 0 L10 5 L0 10 z" fill="#38BDF8"/></marker></defs>')
    elif kind == "bezel":
        out.append(arc(70, 18, r + 40, accent, 8, 0.95, False))
        out.append(arc(110, 162, r + 40, accent, 8, 0.95, False))
    return f'<svg class="arcs" style="left:{ox:.1f}px;top:{oy:.1f}px" width="{size:.0f}" height="{size:.0f}" viewBox="0 0 {size:.0f} {size:.0f}">{"".join(out)}</svg>'


def phone_slide(slide: dict, locale: str, cfg: dict, base: Path) -> str:
    W, H = cfg["phone"]["size"]
    accent = color(slide.get("accent", "emerald"))
    text = loc(slide["text"], locale)
    layout = slide.get("layout", "watch-hero")
    shot = lambda key: data_uri(resolve_screen(cfg, base, key), 1200)

    has_sub = bool(text.get("sub"))
    # The composition box sits under the header. Header bottom depends on line count;
    # the box is anchored to the bottom of the canvas so spacing stays consistent.
    body = [background(W, H, accent)]
    body.append(header_html(text, top=118))
    comp = []
    if layout == "watch-laptop":
        lw = 720
        comp.append(laptop_html(lw, 262, 40, slide.get("desktop", "point"), accent, uid=slide["id"].replace("-", "")))
        d = 610
        cx, cy = 370, 800
        comp.append(watch_html(shot(slide["watch"]), d, cx, cy, accent))
        link = loc(slide.get("link", ""), locale)
        rows = slide.get("rows", [])
        list_top = 640
        if link:
            lc = color(slide.get("link_color", "sky"))
            comp.append(f'<div class="link mono" style="--lc:{lc};left:706px;top:{list_top-92}px"><i></i>{esc(link)}</div>')
        comp.append(rows_html(rows, locale, 706, list_top, 320))
    elif layout == "watch-hero":
        d = 680
        cx, cy = 540, 470
        decor = slide.get("decor", "none")
        if decor in ("motion", "bezel"):
            comp.append(arcs_svg(decor, cx, cy, d / 2, accent))
        comp.append(watch_html(shot(slide["watch"]), d, cx, cy, accent))
        extra = slide.get("extra") or {}
        et = extra.get("type")
        y0 = 1000
        if et == "stats":
            cells = []
            for it in extra["items"]:
                unit = f'<small>{esc(it["unit"])}</small>' if it.get("unit") else ""
                cells.append(f'<div class="stat card"><div class="v">{esc(it["value"])}{unit}</div><div class="l mono">{esc(loc(it["label"],locale))}</div></div>')
            comp.append(f'<div class="stats" style="top:{y0+40}px">{"".join(cells)}</div>')
        elif et == "actions":
            cells = []
            for it in extra["items"]:
                c = color(it.get("color", "emerald"))
                cells.append(
                    f'<div class="action card" style="border-color:{rgba(c,.35)}"><div class="ic" style="background:{rgba(c,.14)};border:1.5px solid {rgba(c,.45)}">{icon(it["icon"],38,c,2.2)}</div>'
                    f'<div class="t" style="color:{c}">{esc(loc(it["title"],locale))}</div><div class="d">{esc(loc(it["desc"],locale))}</div></div>'
                )
            comp.append(f'<div class="actions" style="top:{y0+10}px">{"".join(cells)}</div>')
        elif et == "dpi-scale":
            lo, hi, val, step = extra["min"], extra["max"], extra["value"], extra.get("step", 400)
            pct = (val - lo) / (hi - lo) * 100
            ticks = "".join(
                f'<span class="mono">{"<b>" if v == val else ""}{v}{"</b>" if v == val else ""}</span>' for v in range(lo, hi + 1, step)
            )
            comp.append(f"""<div class="dpi card" style="top:{y0+20}px">
  <div class="track"><div class="fill" style="width:{pct}%"></div><div class="bubble mono" style="left:{pct}%">{val} DPI</div><div class="knob" style="left:{pct}%"></div></div>
  <div class="ticks">{ticks}</div>
  <div class="cap mono">{esc(loc(extra.get('label',''),locale))}</div></div>""")
    elif layout == "open-source":
        brand = cfg["brand"]
        icon_uri = data_uri(resolve(base, brand["icon"]), 640)
        isz = 300
        comp.append(f'<div class="glow" style="left:{540-330}px;top:{-120}px;width:660px;height:600px;background:{rgba(accent,.25)};--b:110px"></div>')
        comp.append(f'<div class="appicon" style="left:{540-isz/2}px;top:20px;width:{isz}px;height:{isz}px"><img src="{icon_uri}"></div>')
        comp.append(f'<div class="abs" style="left:0;right:0;top:{20+isz+40}px;text-align:center;font-size:54px;font-weight:800;letter-spacing:-.02em">{esc(brand["name"])}</div>')
        cards = []
        for o in slide.get("os", []):
            sub = esc(loc(o.get("sub", ""), locale)).replace("\n", "<br>")
            cards.append(f'<div class="osc card"><div class="ic">{icon(o["icon"],50,"#38BDF8",2)}</div><div class="t">{esc(o["title"])}</div><div class="s mono">{sub}</div></div>')
        comp.append(f'<div class="os" style="top:520px">{"".join(cards)}</div>')
        pts = "".join(
            f'<div class="point"><div class="ck">{icon("check",28,"#10B981",2.6)}</div><div>{esc(loc(p,locale))}</div></div>' for p in slide.get("points", [])
        )
        comp.append(f'<div class="points" style="top:900px">{pts}</div>')
        comp.append(f'<div class="foot mono" style="top:1200px">{esc(slide.get("footnote",""))}</div>')
    body.append(f'<div class="stage"><div class="comp">{"".join(comp)}</div></div>')
    return page(W, H, "".join(body), locale, accent, PHONE_CSS)


# --------------------------------------------------------------------------- feature graphic & hero

WIDE_CSS = """
.brandrow{display:flex;align-items:center;gap:22px}
.brandrow .ai{border-radius:22.5%;overflow:hidden;box-shadow:0 0 0 1.5px rgba(16,185,129,.4),0 0 50px rgba(16,185,129,.35);flex:none}
.brandrow .ai img{width:100%;height:100%;display:block}
.bname{font-weight:800;letter-spacing:-.03em;line-height:1}
.zh .bname{font-weight:700}
.tag{color:#CBD5E1;line-height:1.3;letter-spacing:-.01em;text-wrap:balance}
.zh .tag{letter-spacing:.02em}
.chiprow{display:flex;gap:12px;flex-wrap:wrap}
.chip{display:inline-flex;align-items:center;gap:10px;border-radius:999px;border:1.5px solid rgba(148,163,184,.25);background:rgba(15,23,42,.6);color:#E2E8F0}
.chip i{width:8px;height:8px;border-radius:50%;background:var(--accent);box-shadow:0 0 10px var(--accent)}
"""


def feature_graphic(cfg: dict, locale: str, base: Path) -> str:
    W, H = cfg["feature_graphic"]["size"]
    fg = cfg["feature_graphic"]
    t = loc(fg["text"], locale)
    accent = COLORS["emerald"]
    icon_uri = data_uri(resolve(base, cfg["brand"]["icon"]), 256)
    shot = data_uri(resolve_screen(cfg, base, fg["watch"]), 800)
    zh = locale.startswith("zh")
    left = f"""<div class="abs" style="left:112px;top:0;bottom:0;width:390px;display:flex;flex-direction:column;justify-content:center;gap:26px">
  <div class="brandrow"><div class="ai" style="width:84px;height:84px"><img src="{icon_uri}"></div>
   <div class="bname" style="font-size:{48 if not zh else 48}px">{esc(cfg['brand']['name'])}</div></div>
  <div class="tag" style="font-size:{30 if not zh else 30}px;font-weight:{600 if not zh else 500}">{rich(t['tagline'])}</div>
  <div class="chiprow"><span class="chip mono" style="padding:9px 16px;font-size:16px;letter-spacing:.08em"><i></i>{esc(t.get('chips',''))}</span></div>
 </div>"""
    right = laptop_html(318, 584, 92, "point", accent, uid="fg") + watch_html(shot, 232, 606, 322, accent)
    body = background(W, H, accent, "wide") + right + left
    css = WIDE_CSS + ".grid{--gs:40px} .laptop .lid{box-shadow:0 0 0 1.5px #39404b,0 20px 40px rgba(0,0,0,.55)}"
    return page(W, H, body, locale, accent, css)


def hero(cfg: dict, locale: str, base: Path) -> str:
    W, H = cfg["hero"]["size"]
    hc = cfg["hero"]
    t = loc(hc["text"], locale)
    accent = COLORS["emerald"]
    icon_uri = data_uri(resolve(base, cfg["brand"]["icon"]), 256)
    shot = data_uri(resolve_screen(cfg, base, hc["watch"]), 1000)
    chips = "".join(f'<span class="chip" style="padding:12px 22px;font-size:22px;font-weight:600"><i></i>{esc(c)}</span>' for c in t.get("chips", []))
    left = f"""<div class="abs" style="left:120px;top:0;bottom:0;width:640px;display:flex;flex-direction:column;justify-content:center">
  <div class="brandrow"><div class="ai" style="width:64px;height:64px"><img src="{icon_uri}"></div>
    <div class="bname" style="font-size:34px">{esc(cfg['brand']['name'])}</div></div>
  <div style="margin-top:44px;font-size:100px;line-height:1;font-weight:800;letter-spacing:-.034em;text-wrap:balance">{rich(t['headline'])}</div>
  <div class="tag" style="margin-top:30px;font-size:27px;color:#A7B3C6;font-weight:450;max-width:600px">{rich(t.get('sub',''))}</div>
  <div class="chiprow" style="margin-top:38px">{chips}</div>
  <div class="mono" style="margin-top:30px;font-size:17px;letter-spacing:.12em;color:#64748B">{esc(t.get('footnote',''))}</div>
 </div>"""
    right = laptop_html(640, 850, 150, "point", accent, uid="hero") + watch_html(shot, 410, 890, 620, accent)
    body = background(W, H, accent, "wide") + right + left
    css = WIDE_CSS + """.vignette{background:radial-gradient(ellipse 80% 80% at 60% 50%,transparent 50%,rgba(2,4,9,.75) 100%)}
    body::after{content:"";position:absolute;left:0;right:0;bottom:0;height:140px;background:linear-gradient(transparent,rgba(5,9,17,.65))}"""
    return page(W, H, body, locale, accent, css)


# --------------------------------------------------------------------------- pipeline


def resolve(base: Path, p: str) -> Path:
    path = Path(os.path.expanduser(p))
    return path if path.is_absolute() else (base / path).resolve()


def resolve_screen(cfg: dict, base: Path, key: str) -> Path:
    p = cfg.get("screens", {}).get(key, key)
    path = resolve(base, p)
    if not path.exists():
        sys.exit(f"Screenshot not found for '{key}': {path}")
    return path


def render(chrome: str, html_text: str, html_path: Path, png_path: Path, size, scale: int):
    W, H = size
    html_path.parent.mkdir(parents=True, exist_ok=True)
    png_path.parent.mkdir(parents=True, exist_ok=True)
    html_path.write_text(html_text, encoding="utf-8")
    raw = png_path.with_suffix(".raw.png")
    with tempfile.TemporaryDirectory(prefix="wp-chrome-") as prof:
        cmd = [
            chrome, "--headless=new", "--disable-gpu", "--hide-scrollbars", "--no-first-run",
            "--no-default-browser-check", "--disable-extensions", "--mute-audio",
            f"--user-data-dir={prof}", f"--force-device-scale-factor={scale}",
            f"--window-size={W},{H}", "--virtual-time-budget=4000", "--default-background-color=09111EFF",
            f"--screenshot={raw}", html_path.as_uri(),
        ]
        res = subprocess.run(cmd, capture_output=True, text=True, timeout=120)
    if not raw.exists():
        raise RuntimeError(f"Chrome failed for {html_path.name}:\n{res.stderr[-2000:]}")
    im = Image.open(raw).convert("RGB")
    if im.size != (W, H):
        im = im.resize((W, H), Image.LANCZOS)
    im.save(png_path, "PNG", optimize=True)
    raw.unlink()
    return png_path


def contact_sheet(groups, out_path: Path):
    pad, gap, label_h, title_h = 70, 36, 80, 70
    th_phone = 620
    try:
        font = ImageFont.truetype("/System/Library/Fonts/Helvetica.ttc", 22)
        font_b = ImageFont.truetype("/System/Library/Fonts/Helvetica.ttc", 34)
    except Exception:
        font = font_b = ImageFont.load_default()
    rows = []
    for title, files in groups:
        thumbs = []
        for f in files:
            im = Image.open(f).convert("RGB")
            h = th_phone if im.height > im.width else 400
            w = round(im.width * h / im.height)
            thumbs.append((f, im.resize((w, h), Image.LANCZOS)))
        rows.append((title, thumbs))
    width = max(sum(t.width for _, t in th) + gap * (len(th) - 1) for _, th in rows) + pad * 2
    height = pad + sum(title_h + max(t.height for _, t in th) + label_h + gap for _, th in rows) + pad - gap
    sheet = Image.new("RGB", (width, height), (9, 17, 30))
    dr = ImageDraw.Draw(sheet)
    y = pad
    for title, thumbs in rows:
        dr.text((pad, y), title, fill=(248, 250, 252), font=font_b)
        y += title_h
        x = pad
        rh = max(t.height for _, t in thumbs)
        for f, t in thumbs:
            sheet.paste(t, (x, y))
            dr.rectangle([x - 1, y - 1, x + t.width, y + t.height], outline=(51, 65, 85), width=2)
            sz = Image.open(f).size
            name = Path(f).stem
            if name in ("feature_graphic",):
                name = f"{Path(f).parent.name} feature"
            dr.text((x, y + t.height + 10), name[:28], fill=(203, 213, 225), font=font)
            dr.text((x, y + t.height + 40), f"{sz[0]}x{sz[1]}", fill=(100, 116, 139), font=font)
            x += t.width + gap
        y += rh + label_h + gap
    sheet.save(out_path, "PNG", optimize=True)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--config", default=str(HERE / "config.json"))
    ap.add_argument("--out", default=str(HERE / "out"))
    ap.add_argument("--only", choices=["phone", "feature", "hero", "sheet"], action="append")
    ap.add_argument("--locale", action="append", help="limit to these locales")
    ap.add_argument("--keep-html", action="store_true", help="keep intermediate HTML in out/_html")
    ap.add_argument("-j", "--jobs", type=int, default=4)
    args = ap.parse_args()

    cfg_path = Path(args.config).resolve()
    cfg = json.loads(cfg_path.read_text(encoding="utf-8"))
    base = cfg_path.parent
    out = Path(args.out).resolve()
    html_dir = out / "_html"
    scale = int(cfg.get("render_scale", 2))
    only = set(args.only or ["phone", "feature", "hero", "sheet"])
    locales = args.locale or cfg.get("locales", ["en-US"])
    chrome = find_chrome()
    print(f"chrome: {chrome}")

    jobs = []  # (html, html_path, png_path, size)
    phone_files, feature_files, hero_files = {}, [], []
    if "phone" in only:
        slides = [s for s in cfg["phone"]["slides"] if s.get("enabled", True)]
        for lc in locales:
            for i, s in enumerate(slides, 1):
                name = f"phone_{i:02d}_{s['id']}"
                png = out / "play" / lc / f"{name}.png"
                jobs.append((phone_slide(s, lc, cfg, base), html_dir / lc / f"{name}.html", png, tuple(cfg["phone"]["size"])))
                phone_files.setdefault(lc, []).append(png)
    if "feature" in only:
        for lc in locales:
            png = out / "play" / lc / "feature_graphic.png"
            jobs.append((feature_graphic(cfg, lc, base), html_dir / lc / "feature_graphic.html", png, tuple(cfg["feature_graphic"]["size"])))
            feature_files.append(png)
    if "hero" in only:
        for lc in cfg["hero"].get("locales", ["en-US"]):
            png = out / ("hero.png" if lc == "en-US" else f"hero.{lc}.png")
            jobs.append((hero(cfg, lc, base), html_dir / lc / "hero.html", png, tuple(cfg["hero"]["size"])))
            hero_files.append(png)

    def run(job):
        h, hp, pp, size = job
        render(chrome, h, hp, pp, size, scale)
        im = Image.open(pp)
        assert im.size == size and im.mode == "RGB", (pp, im.size, im.mode)
        return pp

    with ThreadPoolExecutor(max_workers=max(1, args.jobs)) as ex:
        for p in ex.map(run, jobs):
            print(f"  wrote {p.relative_to(out)}  {Image.open(p).size[0]}x{Image.open(p).size[1]}")

    if "sheet" in only:
        groups = []
        for lc in locales:
            files = phone_files.get(lc) or sorted((out / "play" / lc).glob("phone_*.png"))
            if files:
                groups.append((f"Google Play phone screenshots — {lc}", files))
        wide = feature_files or sorted((out / "play").glob("*/feature_graphic.png"))
        wide += hero_files or sorted(out.glob("hero*.png"))
        if wide:
            groups.append(("Feature graphic + README hero", wide))
        if groups:
            contact_sheet(groups, out / "contact-sheet.png")
            print("  wrote contact-sheet.png")
    if not args.keep_html and html_dir.exists():
        shutil.rmtree(html_dir)


if __name__ == "__main__":
    main()
