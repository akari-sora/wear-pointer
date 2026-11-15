import React from "react";
import { AbsoluteFill, Img, OffthreadVideo, interpolate, spring, useCurrentFrame, useVideoConfig } from "remotion";
import type { Media } from "./assets";
import { C, Lang, MONO, rgba } from "./theme";

// --------------------------------------------------------------------------- math

export type P = { x: number; y: number };

export const clamp01 = (v: number) => Math.max(0, Math.min(1, v));
export const easeInOut = (t: number) => (t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2);
export const easeOutExpo = (t: number) => (t >= 1 ? 1 : 1 - Math.pow(2, -10 * t));

/** Centripetal-ish Catmull-Rom through pts, u in [0,1]. */
export const spline = (pts: P[], u: number): P => {
  const n = pts.length - 1;
  const s = clamp01(u) * n;
  const i = Math.min(n - 1, Math.floor(s));
  const t = s - i;
  const p0 = pts[Math.max(0, i - 1)];
  const p1 = pts[i];
  const p2 = pts[i + 1];
  const p3 = pts[Math.min(n, i + 2)];
  const t2 = t * t;
  const t3 = t2 * t;
  const f = (a: number, b: number, c: number, d: number) =>
    0.5 * (2 * b + (-a + c) * t + (2 * a - 5 * b + 4 * c - d) * t2 + (-a + 3 * b - 3 * c + d) * t3);
  return { x: f(p0.x, p1.x, p2.x, p3.x), y: f(p0.y, p1.y, p2.y, p3.y) };
};

export const lerpP = (a: P, b: P, t: number): P => ({ x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t });

export const useSpring = (delay = 0, damping = 200, durationInFrames?: number) => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  return spring({ frame: frame - delay, fps, config: { damping }, durationInFrames });
};

// --------------------------------------------------------------------------- global css

export const GLOBAL_CSS = `
.wp-grid{position:absolute;inset:0;background-image:linear-gradient(rgba(148,163,184,.07) 1px,transparent 1px),linear-gradient(90deg,rgba(148,163,184,.07) 1px,transparent 1px);
 background-size:60px 60px;-webkit-mask-image:radial-gradient(ellipse 70% 62% at 50% 45%,#000 0%,rgba(0,0,0,.5) 45%,transparent 82%)}
.wp-watch{position:absolute;width:var(--d);height:var(--d)}
.wp-watch .halo{position:absolute;left:50%;top:50%;width:calc(var(--d)*1.45);height:calc(var(--d)*1.45);transform:translate(-50%,-50%);
 border-radius:50%;background:radial-gradient(circle,color-mix(in srgb,var(--glow) 40%,transparent) 0%,color-mix(in srgb,var(--glow) 12%,transparent) 42%,transparent 68%)}
.wp-watch .strap{position:absolute;left:50%;width:calc(var(--d)*.50);height:calc(var(--d)*.34);transform:translateX(-50%);
 background:repeating-linear-gradient(0deg,rgba(255,255,255,.035) 0 2px,transparent 2px 9px),linear-gradient(90deg,#0b0e13 0%,#1c222b 14%,#272e39 50%,#1c222b 86%,#0b0e13 100%)}
.wp-watch .strap.top{bottom:calc(var(--d)*.84);border-radius:calc(var(--d)*.06) calc(var(--d)*.06) 0 0}
.wp-watch .strap.bottom{top:calc(var(--d)*.84);border-radius:0 0 calc(var(--d)*.06) calc(var(--d)*.06)}
.wp-watch .strap::after{content:"";position:absolute;inset:0;border-radius:inherit;box-shadow:inset 0 0 0 1.5px rgba(255,255,255,.05)}
.wp-watch .strap.top::after{background:linear-gradient(to bottom,rgba(0,0,0,.45),transparent 60%)}
.wp-watch .strap.bottom::after{background:linear-gradient(to top,rgba(0,0,0,.45),transparent 60%)}
.wp-watch .btn{position:absolute;width:calc(var(--d)*.07);height:calc(var(--d)*.115);border-radius:calc(var(--d)*.018);
 background:linear-gradient(90deg,#20252d,#59606c 55%,#2a3038);transform:translate(-50%,-50%) rotate(var(--r));box-shadow:0 2px 6px rgba(0,0,0,.6)}
.wp-watch .case{position:absolute;inset:0;border-radius:50%;
 background:conic-gradient(from 200deg,#4a515c,#1b2028 14%,#626a77 30%,#232830 47%,#565e6a 63%,#191d24 80%,#4a515c);
 box-shadow:0 0 0 1.5px #06080c,0 calc(var(--d)*.06) calc(var(--d)*.12) rgba(0,0,0,.65),inset 0 2px 2px rgba(255,255,255,.35),inset 0 -3px 5px rgba(0,0,0,.7)}
.wp-watch .bezel{position:absolute;inset:calc(var(--d)*.038);border-radius:50%;background:radial-gradient(circle,#0d1016 60%,#151a21 100%);
 box-shadow:inset 0 2px 6px rgba(0,0,0,.9),0 0 0 1px rgba(0,0,0,.6),0 1px 0 rgba(255,255,255,.12)}
.wp-watch .bezel::before{content:"";position:absolute;inset:0;border-radius:50%;
 background:repeating-conic-gradient(from -.5deg,rgba(226,232,240,.16) 0 .5deg,transparent .5deg 6deg);
 -webkit-mask-image:radial-gradient(circle closest-side,transparent 88%,#000 89%,#000 97%,transparent 98%)}
.wp-watch .screen{position:absolute;inset:calc(var(--d)*.085);border-radius:50%;overflow:hidden;background:#000;box-shadow:0 0 0 2px #000,0 0 0 3px rgba(255,255,255,.06)}
.wp-watch .screen .layer{position:absolute;inset:0;width:100%;height:100%;object-fit:cover}
.wp-watch .glass{position:absolute;inset:calc(var(--d)*.085);border-radius:50%;
 background:linear-gradient(140deg,rgba(255,255,255,.13) 0%,rgba(255,255,255,.03) 30%,transparent 46%),radial-gradient(circle at 50% 50%,transparent 66%,rgba(0,0,0,.35) 100%)}
.wp-laptop{position:absolute;width:var(--w)}
.wp-laptop .lid{position:relative;width:var(--w);height:calc(var(--w)*.6415);border-radius:calc(var(--w)*.032) calc(var(--w)*.032) calc(var(--w)*.012) calc(var(--w)*.012);
 background:#05070b;padding:calc(var(--w)*.022);box-shadow:0 0 0 2px #39404b,0 0 0 3px #11151b,0 calc(var(--w)*.05) calc(var(--w)*.1) rgba(0,0,0,.55),0 0 calc(var(--w)*.18) rgba(56,189,248,.10)}
.wp-laptop .cam{position:absolute;top:calc(var(--w)*.008);left:50%;width:calc(var(--w)*.008);height:calc(var(--w)*.008);border-radius:50%;background:#1e2530;transform:translateX(-50%)}
.wp-laptop .display{position:relative;width:100%;height:100%;border-radius:calc(var(--w)*.006);overflow:hidden;background:#0b1424}
.wp-laptop .display svg{position:absolute;inset:0;width:100%;height:100%;display:block}
.wp-laptop .display::after{content:"";position:absolute;inset:0;background:linear-gradient(125deg,rgba(255,255,255,.07) 0%,transparent 35%)}
.wp-laptop .base{position:relative;margin-left:calc(var(--w)*-.06);width:calc(var(--w)*1.12);height:calc(var(--w)*.032);
 background:linear-gradient(180deg,#5a626e 0%,#2b313a 35%,#151920 100%);border-radius:0 0 calc(var(--w)*.05) calc(var(--w)*.05)/0 0 calc(var(--w)*.03) calc(var(--w)*.03);box-shadow:0 calc(var(--w)*.03) calc(var(--w)*.05) rgba(0,0,0,.6)}
.wp-laptop .base::before{content:"";position:absolute;left:50%;top:0;width:18%;height:38%;transform:translateX(-50%);background:#20252d;border-radius:0 0 12px 12px}
.wp-card{background:linear-gradient(180deg,rgba(30,41,59,.66),rgba(15,23,42,.66));border:1.5px solid rgba(148,163,184,.16);box-shadow:0 20px 50px rgba(0,0,0,.35),inset 0 1px 0 rgba(255,255,255,.06)}
`;

// --------------------------------------------------------------------------- background

export const Background: React.FC<{ accent: string; W: number; H: number }> = ({ accent, W, H }) => {
  const frame = useCurrentFrame();
  const drift = Math.sin(frame / 70) * 40;
  const big = Math.max(W, H);
  return (
    <AbsoluteFill style={{ background: `linear-gradient(180deg,${C.bg0} 0%,${C.bg1} 55%,#0A101C 100%)`, overflow: "hidden" }}>
      <div className="wp-grid" />
      <div style={glow(W * 0.05 + drift, H * 0.45, big * 0.62, rgba(accent, 0.2))} />
      <div style={glow(W * 0.62 - drift, H * 0.02, big * 0.55, "rgba(56,189,248,0.13)")} />
      <div style={glow(W * 0.3, -big * 0.25, big * 0.45, rgba(accent, 0.08))} />
      <AbsoluteFill style={{ background: "radial-gradient(ellipse 85% 75% at 50% 45%,transparent 55%,rgba(3,6,12,.6) 100%)" }} />
    </AbsoluteFill>
  );
};

const glow = (x: number, y: number, s: number, color: string): React.CSSProperties => ({
  position: "absolute",
  left: x - s / 2,
  top: y - s / 2,
  width: s,
  height: s,
  borderRadius: "50%",
  background: `radial-gradient(circle, ${color} 0%, transparent 65%)`,
});

// --------------------------------------------------------------------------- watch

export type ScreenLayer = { media: Media; opacity?: number };

export const ScreenMedia: React.FC<{ media: Media; opacity?: number }> = ({ media, opacity = 1 }) =>
  media.kind === "video" ? (
    <OffthreadVideo className="layer" src={media.src} muted style={{ opacity }} />
  ) : (
    <Img className="layer" src={media.src} style={{ opacity }} />
  );

/**
 * Round Galaxy-Watch-like case with the screen layers masked inside.
 * `overlay` is drawn in the screenshot's 480x480 coordinate space.
 */
export const Watch: React.FC<{
  d: number;
  cx: number;
  cy: number;
  layers: ScreenLayer[];
  glow: string;
  tiltX?: number;
  tiltY?: number;
  scale?: number;
  opacity?: number;
  overlay?: React.ReactNode;
}> = ({ d, cx, cy, layers, glow: g, tiltX = 0, tiltY = 0, scale = 1, opacity = 1, overlay }) => {
  const btns = [28, -28].map((ang) => {
    const a = (ang * Math.PI) / 180;
    return { x: d / 2 + d * 0.488 * Math.cos(a), y: d / 2 - d * 0.488 * Math.sin(a), r: -ang };
  });
  return (
    <div
      className="wp-watch"
      style={
        {
          "--d": `${d}px`,
          "--glow": g,
          left: cx - d / 2,
          top: cy - d / 2,
          opacity,
          transform: `perspective(${d * 3}px) rotateX(${tiltX}deg) rotateY(${tiltY}deg) scale(${scale})`,
        } as React.CSSProperties
      }
    >
      <div className="halo" />
      <div className="strap top" />
      <div className="strap bottom" />
      {btns.map((b, i) => (
        <div key={i} className="btn" style={{ left: b.x, top: b.y, ["--r" as string]: `${b.r}deg` } as React.CSSProperties} />
      ))}
      <div className="case" />
      <div className="bezel" />
      <div className="screen">
        {layers.map((l, i) => (
          <ScreenMedia key={`${l.media.file}-${i}`} media={l.media} opacity={l.opacity ?? 1} />
        ))}
        {overlay ? (
          <svg viewBox="0 0 480 480" style={{ position: "absolute", inset: 0, width: "100%", height: "100%" }}>
            {overlay}
          </svg>
        ) : null}
      </div>
      <div className="glass" />
    </div>
  );
};

// --------------------------------------------------------------------------- laptop + desktop

export const Laptop: React.FC<{ w: number; left: number; top: number; children: React.ReactNode; style?: React.CSSProperties }> = ({
  w,
  left,
  top,
  children,
  style,
}) => (
  <div className="wp-laptop" style={{ ["--w" as string]: `${w}px`, left, top, ...style } as React.CSSProperties}>
    <div className="lid">
      <div className="cam" />
      <div className="display">{children}</div>
    </div>
    <div className="base" />
  </div>
);

const CURSOR_PATH = "M0 0 L0 46 L11 35.5 L18.5 52 L26 48.5 L18.8 32.5 L33 32.5 Z";

export const Cursor: React.FC<{ p: P; scale?: number; opacity?: number; halo?: string; press?: number; id: string }> = ({
  p,
  scale = 1.7,
  opacity = 1,
  halo,
  press = 0,
  id,
}) => (
  <g>
    {halo ? (
      <>
        <circle cx={p.x} cy={p.y} r={60} fill={halo} opacity={0.32} filter={`url(#${id}-b18)`} />
        <circle cx={p.x} cy={p.y} r={30 + press * 22} fill="none" stroke={halo} strokeWidth={3 + press * 3} opacity={0.75 - press * 0.2} />
      </>
    ) : null}
    <g transform={`translate(${p.x} ${p.y}) scale(${scale * (1 - press * 0.12)})`} opacity={opacity}>
      <path d={CURSOR_PATH} fill="#000" opacity={0.35} transform="translate(2 4)" />
      <path d={CURSOR_PATH} fill="#F8FAFC" stroke="#0B1220" strokeWidth={2.6} strokeLinejoin="round" />
    </g>
  </g>
);

const WindowShape: React.FC<{ x: number; y: number; w: number; h: number; kind: "editor" | "gallery" | "chart"; accent: string; border?: string; id: string }> = ({
  x,
  y,
  w,
  h,
  kind,
  accent,
  border,
  id,
}) => {
  const body: React.ReactNode[] = [];
  if (kind === "editor") {
    const widths = [0.62, 0.48, 0.74, 0.36, 0.58, 0.68, 0.3, 0.52, 0.44];
    const cols = [C.sky, "rgba(255,255,255,.22)", "rgba(255,255,255,.16)", accent, "rgba(255,255,255,.22)", "rgba(255,255,255,.16)", C.amber, "rgba(255,255,255,.18)", "rgba(255,255,255,.14)"];
    widths.forEach((fw, i) => {
      const ly = y + 80 + i * 38;
      if (ly > y + h - 40) return;
      const indent = [2, 3, 5, 6].includes(i) ? 34 : 0;
      body.push(<rect key={i} x={x + 34 + indent} y={ly} width={(w - 90) * fw} height={14} rx={7} fill={cols[i]} opacity={0.85} />);
    });
  } else if (kind === "gallery") {
    const gw = (w - 68 - 40) / 3;
    const gh = (h - 80 - 34 - 20) / 2;
    const tints = [C.emerald, C.sky, "#6366F1", C.amber, C.sky, C.emerald];
    for (let r = 0; r < 2; r++)
      for (let c = 0; c < 3; c++) {
        const gx = x + 34 + c * (gw + 20);
        const gy = y + 72 + r * (gh + 20);
        body.push(
          <g key={`${r}${c}`}>
            <rect x={gx} y={gy} width={gw} height={gh} rx={12} fill={tints[r * 3 + c]} opacity={0.22} />
            <rect x={gx + 16} y={gy + gh - 30} width={gw * 0.5} height={10} rx={5} fill="rgba(255,255,255,.35)" />
          </g>,
        );
      }
  } else {
    const bars = [0.35, 0.55, 0.42, 0.7, 0.6, 0.85, 0.66, 0.92];
    const bw = (w - 68) / bars.length;
    bars.forEach((b, i) => {
      const bh = (h - 140) * b;
      const last = i === bars.length - 1;
      body.push(<rect key={i} x={x + 34 + i * bw + 6} y={y + h - 40 - bh} width={bw - 12} height={bh} rx={8} fill={last ? accent : C.sky} opacity={last ? 0.9 : 0.35} />);
    });
  }
  return (
    <g>
      <rect x={x} y={y + 18} width={w} height={h} rx={20} fill="#000" opacity={0.45} filter={`url(#${id}-b18)`} />
      <rect x={x} y={y} width={w} height={h} rx={20} fill="#111a2b" stroke={border ?? "rgba(255,255,255,.10)"} strokeWidth={border ? 4 : 2} />
      <path d={`M${x} ${y + 20} a20 20 0 0 1 20 -20 h${w - 40} a20 20 0 0 1 20 20 v28 h-${w} z`} fill="#18233a" />
      {[C.rose, C.amber, C.emerald].map((c, i) => (
        <circle key={c} cx={x + 30 + i * 24} cy={y + 24} r={7} fill={c} />
      ))}
      <rect x={x + w / 2 - 80} y={y + 18} width={160} height={12} rx={6} fill="rgba(255,255,255,.14)" />
      {body}
    </g>
  );
};

export type DesktopProps = {
  id: string;
  accent: string;
  cursor: P;
  trail?: P[];
  press?: number;
  /** Drag scene: moving window position (top-left), ghost outline at the original spot. */
  drag?: { pos: P; ghost: P; active: number };
  laser?: { opacity: number };
  pulse?: { p: P; t: number; color: string };
  badge?: { text: string; color: string; opacity: number };
  cursorOpacity?: number;
};

/** Minimal 1600x1000 desktop scene with windows, a dock and an animated cursor. */
export const Desktop: React.FC<DesktopProps> = ({ id, accent, cursor, trail = [], press = 0, drag, laser, pulse, badge, cursorOpacity = 1 }) => {
  const n = trail.length;
  return (
    <svg viewBox="0 0 1600 1000" preserveAspectRatio="xMidYMid slice">
      <defs>
        <linearGradient id={`${id}-wp`} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#0f1b31" />
          <stop offset="1" stopColor="#081020" />
        </linearGradient>
        <filter id={`${id}-b120`} x="-50%" y="-50%" width="200%" height="200%">
          <feGaussianBlur stdDeviation={120} />
        </filter>
        <filter id={`${id}-b18`} x="-50%" y="-50%" width="200%" height="200%">
          <feGaussianBlur stdDeviation={18} />
        </filter>
        <filter id={`${id}-b8`} x="-50%" y="-50%" width="200%" height="200%">
          <feGaussianBlur stdDeviation={8} />
        </filter>
      </defs>
      <rect width={1600} height={1000} fill={`url(#${id}-wp)`} />
      <circle cx={260} cy={860} r={420} fill={accent} opacity={0.2} filter={`url(#${id}-b120)`} />
      <circle cx={1380} cy={160} r={380} fill={C.sky} opacity={0.15} filter={`url(#${id}-b120)`} />
      <rect width={1600} height={40} fill="rgba(255,255,255,.06)" />
      <circle cx={34} cy={20} r={9} fill="rgba(255,255,255,.55)" />
      {[60, 46, 52, 70].map((w, i) => (
        <rect key={i} x={66 + i * 90} y={14} width={w} height={12} rx={6} fill="rgba(255,255,255,.28)" />
      ))}
      {[28, 28, 90].map((w, i) => (
        <rect key={i} x={1340 + i * 60} y={14} width={w} height={12} rx={6} fill="rgba(255,255,255,.28)" />
      ))}
      <rect x={520} y={902} width={560} height={72} rx={24} fill="rgba(255,255,255,.08)" stroke="rgba(255,255,255,.10)" strokeWidth={2} />
      {[C.sky, C.emerald, "#6366F1", C.amber, C.rose, "#94A3B8", C.sky, C.emerald].map((c, i) => (
        <rect key={i} x={544 + i * 66} y={914} width={48} height={48} rx={13} fill={c} opacity={0.55} />
      ))}

      {drag ? (
        <>
          <WindowShape id={id} x={980} y={120} w={520} h={360} kind="gallery" accent={accent} />
          <rect x={drag.ghost.x} y={drag.ghost.y} width={600} height={400} rx={20} fill={accent} fillOpacity={0.05 * drag.active} stroke={accent} strokeOpacity={0.55 * drag.active} strokeWidth={3} strokeDasharray="14 12" />
          <WindowShape id={id} x={drag.pos.x} y={drag.pos.y} w={600} h={400} kind="chart" accent={accent} border={drag.active > 0.05 ? accent : undefined} />
          {drag.active > 0.05 ? (
            <rect x={drag.pos.x - 6} y={drag.pos.y - 6} width={612} height={412} rx={24} fill="none" stroke={accent} strokeWidth={10} opacity={0.28 * drag.active} filter={`url(#${id}-b8)`} />
          ) : null}
        </>
      ) : (
        <>
          <WindowShape id={id} x={120} y={110} w={760} h={500} kind="editor" accent={accent} />
          <WindowShape id={id} x={960} y={250} w={520} h={380} kind="gallery" accent={accent} />
        </>
      )}

      {laser && laser.opacity > 0 ? (
        <g opacity={laser.opacity}>
          <line x1={0} y1={1000} x2={cursor.x} y2={cursor.y} stroke={accent} strokeWidth={4} strokeDasharray="4 16" strokeLinecap="round" opacity={0.6} />
          <circle cx={cursor.x} cy={cursor.y} r={70} fill={accent} opacity={0.25} filter={`url(#${id}-b18)`} />
          <circle cx={cursor.x} cy={cursor.y} r={46} fill="none" stroke={accent} strokeWidth={2.5} opacity={0.6} />
          <circle cx={cursor.x} cy={cursor.y} r={12} fill={accent} />
          <circle cx={cursor.x} cy={cursor.y} r={5} fill="#ECFDF5" />
        </g>
      ) : null}

      {trail.slice(0, -1).map((a, i) => {
        const b = trail[i + 1];
        const k = (i + 1) / n;
        return <line key={i} x1={a.x} y1={a.y} x2={b.x} y2={b.y} stroke={accent} strokeWidth={3 + k * 8} strokeLinecap="round" opacity={k * 0.9} />;
      })}
      {n > 4 ? <polyline points={trail.map((p) => `${p.x},${p.y}`).join(" ")} fill="none" stroke={accent} strokeWidth={30} opacity={0.18} filter={`url(#${id}-b18)`} /> : null}

      {pulse && pulse.t > 0 && pulse.t < 1 ? (
        <g>
          {[0, 0.25].map((off) => {
            const t = clamp01((pulse.t - off) / (1 - off));
            return <circle key={off} cx={pulse.p.x} cy={pulse.p.y} r={20 + t * 160} fill="none" stroke={pulse.color} strokeWidth={6 * (1 - t) + 1} opacity={(1 - t) * 0.9} />;
          })}
        </g>
      ) : null}

      {laser && laser.opacity > 0.95 ? null : <Cursor id={id} p={cursor} halo={accent} press={press} opacity={cursorOpacity} />}

      {badge && badge.opacity > 0 ? (
        <g opacity={badge.opacity} transform={`translate(${cursor.x + 70} ${cursor.y - 10})`}>
          <rect x={0} y={-30} width={badge.text.length * 22 + 74} height={60} rx={30} fill="rgba(15,23,42,.92)" stroke={badge.color} strokeWidth={3} />
          <rect x={26} y={-12} width={8} height={24} rx={2} fill={badge.color} />
          <rect x={40} y={-12} width={8} height={24} rx={2} fill={badge.color} />
          <text x={62} y={10} fill={badge.color} fontFamily={MONO} fontSize={28} fontWeight={700} letterSpacing={2}>
            {badge.text}
          </text>
        </g>
      ) : null}
    </svg>
  );
};

/** Sample a time-parametrised path backwards to build a cursor trail. */
export const trailOf = (pos: (f: number) => P, frame: number, len = 18, step = 1, minFrame = 0): P[] => {
  const pts: P[] = [];
  for (let k = len; k >= 0; k--) {
    const f = frame - k * step;
    if (f < minFrame) continue;
    pts.push(pos(f));
  }
  return pts;
};

// --------------------------------------------------------------------------- text

type Token = { t: string; accent: boolean; space: boolean };

const tokenize = (text: string, lang: Lang): Token[] => {
  const out: Token[] = [];
  text.split(/(\*[^*]+\*)/).forEach((seg) => {
    if (!seg) return;
    const accent = seg.startsWith("*") && seg.endsWith("*");
    const body = accent ? seg.slice(1, -1) : seg;
    if (lang === "zh") {
      // CJK: one token per character, keep Latin/number runs together.
      const parts = body.match(/[A-Za-z0-9.~\-–+%·]+|\s+|./gu) ?? [];
      parts.forEach((p) => out.push({ t: p, accent, space: /^\s+$/.test(p) }));
    } else {
      body.split(/(\s+)/).forEach((p) => p && out.push({ t: p, accent, space: /^\s+$/.test(p) }));
    }
  });
  return out;
};

/** Headline with a per-word (per-character for zh) spring reveal. */
export const Reveal: React.FC<{
  text: string;
  lang: Lang;
  delay?: number;
  stagger?: number;
  accent: string;
  style?: React.CSSProperties;
}> = ({ text, lang, delay = 0, stagger, accent, style }) => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const tokens = tokenize(text, lang);
  const st = stagger ?? (lang === "zh" ? 1.6 : 3);
  let idx = 0;
  return (
    <div style={{ textWrap: "balance", ...style } as React.CSSProperties}>
      {tokens.map((tk, i) => {
        if (tk.t.includes("\n")) return <br key={i} />;
        if (tk.space) return <span key={i}> </span>;
        const s = spring({ frame: frame - delay - idx++ * st, fps, config: { damping: 18, mass: 0.6, stiffness: 140 } });
        return (
          <span
            key={i}
            style={{
              display: "inline-block",
              color: tk.accent ? accent : undefined,
              opacity: clamp01(s * 1.4),
              transform: `translateY(${(1 - s) * 0.45}em)`,
              filter: `blur(${(1 - clamp01(s)) * 8}px)`,
            }}
          >
            {tk.t}
          </span>
        );
      })}
    </div>
  );
};

export const Rise: React.FC<{ delay?: number; children: React.ReactNode; style?: React.CSSProperties; dist?: number }> = ({ delay = 0, children, style, dist = 30 }) => {
  const s = useSpring(delay, 200);
  return <div style={{ opacity: s, transform: `translateY(${(1 - s) * dist}px)`, ...style }}>{children}</div>;
};

export const Eyebrow: React.FC<{ text: string; accent: string; size?: number; lang: Lang }> = ({ text, accent, size = 24, lang }) => (
  <div
    style={{
      display: "inline-flex",
      alignItems: "center",
      gap: size * 0.6,
      padding: `${size * 0.5}px ${size}px ${size * 0.5}px ${size * 0.85}px`,
      borderRadius: 999,
      border: `1.5px solid ${rgba(accent, 0.45)}`,
      background: rgba(accent, 0.1),
      color: accent,
      fontFamily: MONO,
      fontSize: size,
      fontWeight: 600,
      letterSpacing: lang === "zh" ? "0.2em" : "0.16em",
      whiteSpace: "nowrap",
    }}
  >
    <i style={{ width: size * 0.42, height: size * 0.42, borderRadius: "50%", background: accent, boxShadow: `0 0 14px ${accent}` }} />
    {text}
  </div>
);

// --------------------------------------------------------------------------- widgets

export const ICONS: Record<string, string> = {
  command: '<path d="M9 6a3 3 0 1 0-3 3h12a3 3 0 1 0-3-3v12a3 3 0 1 0 3-3H6a3 3 0 1 0 3 3V6z"/>',
  window: '<rect x="3" y="4.5" width="18" height="15" rx="2.5"/><path d="M3 9h18"/>',
  terminal: '<rect x="3" y="4.5" width="18" height="15" rx="2.5"/><path d="M7 10l3 2.5L7 15M12.5 15.5H17"/>',
  tap: '<circle cx="12" cy="12" r="3"/><circle cx="12" cy="12" r="7.5" opacity=".55"/>',
  hold: '<circle cx="12" cy="12" r="3" fill="currentColor"/><circle cx="12" cy="12" r="8"/><path d="M12 4a8 8 0 0 1 8 8" stroke-width="3"/>',
  pause: '<path d="M9 6v12M15 6v12" stroke-width="3"/>',
  center: '<circle cx="12" cy="12" r="6"/><circle cx="12" cy="12" r="1.6" fill="currentColor"/><path d="M12 2.5v4M12 17.5v4M2.5 12h4M17.5 12h4"/>',
  check: '<path d="M5 12.5l4.5 4.5L19 7.5" stroke-width="2.6"/>',
  gyro: '<circle cx="12" cy="12" r="8"/><path d="M12 4v16M4 12h16" opacity=".5"/><circle cx="12" cy="12" r="2.5" fill="currentColor"/>',
  bolt: '<path d="M13 3L5 13.5h6L10 21l8-10.5h-6L13 3z"/>',
  dpi: '<path d="M4 18h16M6 18V9M10 18V6M14 18v-8M18 18V4"/>',
};

export const Icon: React.FC<{ name: string; size: number; color: string; stroke?: number }> = ({ name, size, color, stroke = 2 }) => (
  <svg
    width={size}
    height={size}
    viewBox="0 0 24 24"
    fill="none"
    stroke={color}
    strokeWidth={stroke}
    strokeLinecap="round"
    strokeLinejoin="round"
    style={{ color, display: "block" }}
    dangerouslySetInnerHTML={{ __html: ICONS[name] ?? ICONS.tap }}
  />
);

export const RowCard: React.FC<{ icon: string; color: string; title: string; sub: string; active?: number; width: number; scale?: number }> = ({
  icon,
  color,
  title,
  sub,
  active = 0,
  width,
  scale = 1,
}) => (
  <div
    className="wp-card"
    style={{
      width,
      display: "flex",
      alignItems: "center",
      gap: 20 * scale,
      padding: `${20 * scale}px ${22 * scale}px`,
      borderRadius: 28 * scale,
      borderColor: active > 0 ? rgba(color, 0.2 + 0.5 * active) : undefined,
      boxShadow: active > 0 ? `0 0 ${40 * active}px ${rgba(color, 0.35 * active)}, inset 0 1px 0 rgba(255,255,255,.06)` : undefined,
      transform: `scale(${1 + 0.03 * active})`,
    }}
  >
    <div
      style={{
        width: 62 * scale,
        height: 62 * scale,
        borderRadius: 18 * scale,
        flex: "none",
        display: "grid",
        placeItems: "center",
        background: rgba(color, 0.13 + 0.12 * active),
        border: `1.5px solid ${rgba(color, 0.4)}`,
      }}
    >
      <Icon name={icon} size={36 * scale} color={color} stroke={2.1} />
    </div>
    <div style={{ minWidth: 0 }}>
      <div style={{ fontSize: 32 * scale, fontWeight: 700, lineHeight: 1.1, whiteSpace: "nowrap" }}>{title}</div>
      <div style={{ marginTop: 6 * scale, fontSize: 19 * scale, color: C.muted, fontFamily: MONO, whiteSpace: "nowrap" }}>{sub}</div>
    </div>
  </div>
);

export const Chip: React.FC<{ text: string; accent: string; size?: number }> = ({ text, accent, size = 24 }) => (
  <span
    style={{
      display: "inline-flex",
      alignItems: "center",
      gap: size * 0.45,
      padding: `${size * 0.5}px ${size * 0.95}px`,
      borderRadius: 999,
      border: "1.5px solid rgba(148,163,184,.25)",
      background: "rgba(15,23,42,.6)",
      color: "#E2E8F0",
      fontSize: size,
      fontWeight: 600,
      whiteSpace: "nowrap",
    }}
  >
    <i style={{ width: size * 0.36, height: size * 0.36, borderRadius: "50%", background: accent, boxShadow: `0 0 10px ${accent}` }} />
    {text}
  </span>
);

/** Watch-style segmented pill: LASER / AIR / TOUCH with a sliding emerald thumb. */
export const Segmented: React.FC<{ labels: string[]; pos: number; width: number; height: number; accent: string; lang: Lang }> = ({
  labels,
  pos,
  width,
  height,
  accent,
  lang,
}) => {
  const pad = 8;
  const segW = (width - pad * 2) / labels.length;
  return (
    <div
      style={{
        position: "relative",
        width,
        height,
        borderRadius: height / 2,
        background: "rgba(10,14,22,.92)",
        border: "2px solid rgba(148,163,184,.22)",
        boxShadow: "0 24px 60px rgba(0,0,0,.45), inset 0 1px 0 rgba(255,255,255,.06)",
      }}
    >
      <div
        style={{
          position: "absolute",
          top: pad,
          bottom: pad,
          left: pad + pos * segW,
          width: segW,
          borderRadius: (height - pad * 2) / 2,
          background: rgba(accent, 0.2),
          border: `2.5px solid ${accent}`,
          boxShadow: `0 0 30px ${rgba(accent, 0.45)}`,
        }}
      />
      {labels.map((l, i) => {
        const on = clamp01(1 - Math.abs(pos - i));
        return (
          <div
            key={l}
            style={{
              position: "absolute",
              top: 0,
              bottom: 0,
              left: pad + i * segW,
              width: segW,
              display: "grid",
              placeItems: "center",
              fontFamily: lang === "zh" ? undefined : MONO,
              fontSize: height * (lang === "zh" ? 0.34 : 0.32),
              fontWeight: 700,
              letterSpacing: lang === "zh" ? "0.06em" : "0.14em",
              color: on > 0.5 ? accent : "#CBD5E1",
              opacity: 0.6 + 0.4 * on,
              whiteSpace: "nowrap",
            }}
          >
            {l}
          </div>
        );
      })}
    </div>
  );
};

export const DpiSlider: React.FC<{ value: number; width: number; accent: string; caption: string; scale?: number }> = ({ value, width, accent, caption, scale = 1 }) => {
  const lo = 400;
  const hi = 3200;
  const pct = ((value - lo) / (hi - lo)) * 100;
  const shown = Math.round(value / 200) * 200;
  const ticks = [400, 800, 1200, 1600, 2000, 2400, 2800, 3200];
  return (
    <div className="wp-card" style={{ width, borderRadius: 34 * scale, padding: `${36 * scale}px ${48 * scale}px ${32 * scale}px` }}>
      <div style={{ position: "relative", height: 14 * scale, borderRadius: 7 * scale, background: "rgba(148,163,184,.18)", marginTop: 76 * scale }}>
        <div style={{ position: "absolute", left: 0, top: 0, bottom: 0, width: `${pct}%`, borderRadius: 7 * scale, background: `linear-gradient(90deg,${rgba(accent, 0.35)},${accent})`, boxShadow: `0 0 24px ${rgba(accent, 0.5)}` }} />
        <div
          style={{
            position: "absolute",
            left: `${pct}%`,
            bottom: 40 * scale,
            transform: "translateX(-50%)",
            padding: `${10 * scale}px ${20 * scale}px`,
            borderRadius: 14 * scale,
            background: accent,
            color: "#04120c",
            fontFamily: MONO,
            fontSize: 28 * scale,
            fontWeight: 800,
            whiteSpace: "nowrap",
          }}
        >
          {shown} DPI
        </div>
        <div style={{ position: "absolute", left: `${pct}%`, top: "50%", width: 40 * scale, height: 40 * scale, borderRadius: "50%", background: "#F8FAFC", transform: "translate(-50%,-50%)", boxShadow: `0 0 0 ${8 * scale}px ${rgba(accent, 0.35)},0 0 30px ${accent}` }} />
      </div>
      <div style={{ display: "flex", justifyContent: "space-between", marginTop: 26 * scale, fontFamily: MONO, fontSize: 21 * scale, color: C.muted }}>
        {ticks.map((t) => (
          <span key={t} style={{ color: t === shown ? C.ink : undefined, fontWeight: t === shown ? 700 : 400 }}>
            {t}
          </span>
        ))}
      </div>
      <div style={{ marginTop: 16 * scale, textAlign: "center", fontFamily: MONO, fontSize: 19 * scale, letterSpacing: "0.16em", color: C.dim }}>{caption}</div>
    </div>
  );
};

export const interp = (f: number, input: number[], output: number[]) =>
  interpolate(f, input, output, { extrapolateLeft: "clamp", extrapolateRight: "clamp" });
