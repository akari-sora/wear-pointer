import React from "react";
import { AbsoluteFill, Img, useCurrentFrame, useVideoConfig, spring } from "remotion";
import { iconSrc, screen, screenOrMain } from "./assets";
import {
  Background,
  Chip,
  Desktop,
  DpiSlider,
  Eyebrow,
  Icon,
  Laptop,
  P,
  Reveal,
  Rise,
  RowCard,
  Segmented,
  Watch,
  clamp01,
  easeInOut,
  easeOutExpo,
  interp,
  lerpP,
  spline,
  trailOf,
  useSpring,
} from "./components";
import { C, COPY, Lang, MONO, Orientation, REPO, SANS, SANS_ZH, VERSION, rgba } from "./theme";

export type SceneProps = { o: Orientation; lang: Lang };

// --------------------------------------------------------------------------- layout

const useLayout = ({ o, lang }: SceneProps, dy = 0) => {
  const { width: W, height: H } = useVideoConfig();
  const isV = o === "vertical";
  return {
    W,
    H,
    isV,
    lang,
    copy: COPY[lang],
    font: lang === "zh" ? SANS_ZH : SANS,
    laptop: isV ? { w: 840, left: 180, top: 620 + dy } : { w: 880, left: 900, top: 150 },
    watch: isV ? { d: 590, cx: 355, cy: 1400 + dy } : { d: 460, cx: 960, cy: 700 },
    col: isV ? { left: 700, top: 1262 + dy, width: 330 } : null,
    text: isV
      ? ({ position: "absolute", left: 70, right: 70, top: 150, textAlign: "center" } as React.CSSProperties)
      : ({ position: "absolute", left: 120, top: 210, width: 660, textAlign: "left" } as React.CSSProperties),
    headSize: isV ? 92 : 88,
    subSize: isV ? 36 : 32,
  };
};

type L = ReturnType<typeof useLayout>;

const headStyle = (l: L, size = l.headSize): React.CSSProperties => ({
  fontSize: l.lang === "zh" ? size * 1.02 : size,
  lineHeight: l.lang === "zh" ? 1.16 : 1.05,
  fontWeight: l.lang === "zh" ? 600 : 800,
  letterSpacing: l.lang === "zh" ? "0.01em" : "-0.025em",
  marginTop: 30,
});

const subStyle = (l: L): React.CSSProperties => ({
  marginTop: 24,
  fontSize: l.subSize,
  lineHeight: 1.35,
  color: C.muted,
  letterSpacing: l.lang === "zh" ? "0.03em" : "-0.01em",
  textWrap: "balance",
} as React.CSSProperties);

const TextBlock: React.FC<{ l: L; accent: string; eyebrow?: string; headline: string; sub?: string; children?: React.ReactNode; headSize?: number }> = ({
  l,
  accent,
  eyebrow,
  headline,
  sub,
  children,
  headSize,
}) => (
  <div style={l.text}>
    {eyebrow ? (
      <Rise delay={0} dist={16}>
        <Eyebrow text={eyebrow} accent={accent} lang={l.lang} size={l.isV ? 24 : 22} />
      </Rise>
    ) : null}
    <Reveal text={headline} lang={l.lang} accent={accent} delay={4} style={headStyle(l, headSize)} />
    {sub ? (
      <Rise delay={16} dist={20}>
        <div style={subStyle(l)}>{sub}</div>
      </Rise>
    ) : null}
    {children}
  </div>
);

const Frame: React.FC<{ l: L; accent: string; children: React.ReactNode }> = ({ l, accent, children }) => (
  <AbsoluteFill style={{ fontFamily: l.font, color: C.ink, WebkitFontSmoothing: "antialiased" } as React.CSSProperties}>
    <Background accent={accent} W={l.W} H={l.H} />
    {children}
  </AbsoluteFill>
);

/** Shared laptop+watch entrance so device scenes feel continuous. */
const useDeviceIn = () => {
  const lap = useSpring(2, 200, 26);
  const wat = useSpring(6, 200, 26);
  return { lap, wat };
};

const tiltFrom = (p: P, k = 1) => ({ tiltY: ((p.x - 800) / 800) * 16 * k, tiltX: (-(p.y - 500) / 500) * 11 * k });

const Column: React.FC<{ l: L; children: React.ReactNode; top?: number }> = ({ l, children, top }) =>
  l.col ? (
    <div style={{ position: "absolute", left: l.col.left, top: top ?? l.col.top, width: l.col.width, display: "flex", flexDirection: "column", gap: 20 }}>{children}</div>
  ) : null;

// --------------------------------------------------------------------------- 1 hook

export const Hook: React.FC<SceneProps> = (props) => {
  const l = useLayout(props, 20);
  const f = useCurrentFrame();
  const accent = C.emerald;
  const { lap, wat } = useDeviceIn();
  const icon = useSpring(0, 12);
  const route: P[] = [
    { x: 260, y: 860 },
    { x: 520, y: 560 },
    { x: 860, y: 420 },
    { x: 1160, y: 330 },
    { x: 1270, y: 540 },
    { x: 1000, y: 650 },
    { x: 760, y: 520 },
  ];
  const pos = (fr: number) => spline(route, easeInOut(clamp01((fr - 10) / 105)));
  const cur = pos(f);
  const t = tiltFrom(cur, 0.8);
  const c = l.copy.hook;
  return (
    <Frame l={l} accent={accent}>
      <Laptop w={l.laptop.w} left={l.laptop.left + (1 - lap) * 160} top={l.laptop.top} style={{ opacity: lap }}>
        <Desktop id="hook" accent={accent} cursor={cur} trail={trailOf(pos, f, 16, 1, 10)} />
      </Laptop>
      <Watch d={l.watch.d} cx={l.watch.cx} cy={l.watch.cy + (1 - wat) * 140} layers={[{ media: screenOrMain("main") }]} glow={accent} opacity={wat} {...t} />
      <div style={l.text}>
        <div style={{ display: "flex", alignItems: "center", gap: 20, justifyContent: l.isV ? "center" : "flex-start" }}>
          <Img
            src={iconSrc()}
            style={{
              width: l.isV ? 92 : 84,
              height: l.isV ? 92 : 84,
              borderRadius: "22.5%",
              transform: `scale(${0.6 + 0.4 * icon})`,
              opacity: clamp01(icon * 1.5),
              boxShadow: `0 0 0 1.5px ${rgba(accent, 0.4)}, 0 0 60px ${rgba(accent, 0.4)}`,
            }}
          />
          <Rise delay={3} dist={14}>
            <div style={{ fontSize: l.isV ? 44 : 40, fontWeight: 800, letterSpacing: "-0.02em" }}>{l.copy.brand}</div>
          </Rise>
        </div>
        <Reveal text={c.headline} lang={l.lang} accent={accent} delay={8} style={headStyle(l, l.isV ? 96 : 96)} />
        <Rise delay={22} dist={20}>
          <div style={subStyle(l)}>{c.sub}</div>
        </Rise>
        {!l.isV ? (
          <div style={{ display: "flex", gap: 14, marginTop: 40 }}>
            {c.chips.map((ch, i) => (
              <Rise key={ch} delay={30 + i * 4} dist={16}>
                <Chip text={ch} accent={accent} size={24} />
              </Rise>
            ))}
          </div>
        ) : null}
      </div>
      <Column l={l}>
        {[
          { icon: "command", title: "macOS", sub: "CGEventPost" },
          { icon: "window", title: "Windows", sub: "SendInput" },
          { icon: "terminal", title: "Linux", sub: "Hyprland · uinput" },
        ].map((r, i) => (
          <Rise key={r.title} delay={30 + i * 5} dist={24}>
            <RowCard icon={r.icon} color={C.sky} title={r.title} sub={r.sub} width={l.col?.width ?? 300} />
          </Rise>
        ))}
      </Column>
    </Frame>
  );
};

// --------------------------------------------------------------------------- 2 point

export const Point: React.FC<SceneProps> = (props) => {
  const l = useLayout(props);
  const f = useCurrentFrame();
  const accent = C.emerald;
  const { lap, wat } = useDeviceIn();
  const route: P[] = [
    { x: 330, y: 780 },
    { x: 640, y: 330 },
    { x: 1180, y: 240 },
    { x: 1360, y: 620 },
    { x: 960, y: 760 },
    { x: 620, y: 470 },
  ];
  const pos = (fr: number) => spline(route, easeInOut(clamp01((fr - 6) / 110)));
  const cur = pos(f);
  const t = tiltFrom(cur, 1.35);
  const c = l.copy.point;
  const statIcons = ["gyro", "bolt", "dpi"];
  return (
    <Frame l={l} accent={accent}>
      <Laptop w={l.laptop.w} left={l.laptop.left} top={l.laptop.top} style={{ opacity: Math.max(lap, 0.001) }}>
        <Desktop id="point" accent={accent} cursor={cur} trail={trailOf(pos, f, 20, 1, 6)} />
      </Laptop>
      <Watch d={l.watch.d} cx={l.watch.cx} cy={l.watch.cy} layers={[{ media: screenOrMain("main") }]} glow={accent} scale={0.94 + 0.06 * wat} {...t} />
      <TextBlock l={l} accent={accent} eyebrow={c.eyebrow} headline={c.headline} sub={c.sub}>
        {!l.isV ? (
          <div style={{ display: "flex", gap: 16, marginTop: 40 }}>
            {c.stats.map((s, i) => (
              <Rise key={s.v} delay={26 + i * 5} dist={18}>
                <div className="wp-card" style={{ borderRadius: 24, padding: "20px 24px", minWidth: 190 }}>
                  <div style={{ fontSize: 40, fontWeight: 800, letterSpacing: "-0.02em" }}>{s.v}</div>
                  <div style={{ marginTop: 8, fontFamily: MONO, fontSize: 16, letterSpacing: "0.14em", color: C.muted }}>{s.l}</div>
                </div>
              </Rise>
            ))}
          </div>
        ) : null}
      </TextBlock>
      <Column l={l}>
        {c.stats.map((s, i) => (
          <Rise key={s.v} delay={24 + i * 5} dist={24}>
            <RowCard icon={statIcons[i]} color={i === 1 ? C.sky : accent} title={s.v} sub={s.l} width={l.col?.width ?? 300} />
          </Rise>
        ))}
      </Column>
    </Frame>
  );
};

// --------------------------------------------------------------------------- 3 tap / hold / drag

export const Drag: React.FC<SceneProps> = (props) => {
  const l = useLayout(props);
  const f = useCurrentFrame();
  const accent = C.emerald;
  const c = l.copy.drag;
  const TAP = 20;
  const HOLD0 = 48;
  const HOLD1 = 74;
  const DRAG0 = 78;
  const DRAG1 = 124;
  const ghost = { x: 140, y: 150 };
  const end = { x: 760, y: 430 };
  const grip = { x: 250, y: 24 };
  const winAt = (fr: number): P => lerpP(ghost, end, easeInOut(clamp01((fr - DRAG0) / (DRAG1 - DRAG0))));
  const pos = (fr: number): P => {
    if (fr < 16) return lerpP({ x: 760, y: 700 }, { x: ghost.x + grip.x, y: ghost.y + grip.y }, easeInOut(clamp01(fr / 16)));
    const w = winAt(fr);
    return { x: w.x + grip.x, y: w.y + grip.y };
  };
  const press = Math.max(interp(f, [TAP, TAP + 4, TAP + 10], [0, 1, 0]), interp(f, [HOLD0, HOLD0 + 5], [0, 0.7]) * interp(f, [DRAG1 + 6, DRAG1 + 12], [1, 0]));
  const holdP = clamp01((f - HOLD0) / (HOLD1 - HOLD0));
  const locked = interp(f, [HOLD1, HOLD1 + 6], [0, 1]);
  const ripple = clamp01((f - TAP) / 18);
  const tapActive = interp(f, [TAP - 6, TAP, TAP + 18, TAP + 26], [0, 1, 1, 0]);
  const holdActive = interp(f, [HOLD0 - 4, HOLD0 + 2], [0, 1]);
  const R = 50;
  const circ = 2 * Math.PI * R;
  const overlay = (
    <g>
      {ripple > 0 && ripple < 1 ? (
        <>
          <circle cx={240} cy={185} r={20 + ripple * 150} fill={accent} opacity={(1 - ripple) * 0.25} />
          <circle cx={240} cy={185} r={20 + ripple * 150} fill="none" stroke={accent} strokeWidth={4} opacity={(1 - ripple) * 0.9} />
        </>
      ) : null}
      {holdP > 0 && locked < 1 ? (
        <g opacity={1 - locked}>
          <circle cx={240} cy={160} r={R} fill="none" stroke={rgba(accent, 0.25)} strokeWidth={6} />
          <circle
            cx={240}
            cy={160}
            r={R}
            fill="none"
            stroke={accent}
            strokeWidth={6}
            strokeLinecap="round"
            strokeDasharray={`${circ * holdP} ${circ}`}
            transform="rotate(-90 240 160)"
            style={{ filter: `drop-shadow(0 0 6px ${accent})` }}
          />
        </g>
      ) : null}
    </g>
  );
  return (
    <Frame l={l} accent={accent}>
      <Laptop w={l.laptop.w} left={l.laptop.left} top={l.laptop.top}>
        <Desktop
          id="drag"
          accent={accent}
          cursor={pos(f)}
          press={press}
          trail={f > DRAG0 ? trailOf(pos, f, 16, 1, DRAG0) : []}
          drag={{ pos: winAt(f), ghost, active: locked }}
        />
      </Laptop>
      <Watch
        d={l.watch.d}
        cx={l.watch.cx}
        cy={l.watch.cy}
        layers={[{ media: screenOrMain("main") }, { media: screenOrMain("drag"), opacity: locked }]}
        glow={accent}
        tiltY={interp(f, [DRAG0, DRAG1], [0, 12])}
        tiltX={interp(f, [DRAG0, DRAG1], [0, -5])}
        scale={1 - press * 0.012}
        overlay={overlay}
      />
      <TextBlock l={l} accent={accent} eyebrow={c.eyebrow} headline={c.headline} sub={c.sub}>
        {!l.isV ? (
          <div style={{ display: "flex", gap: 16, marginTop: 40 }}>
            <RowCard icon="tap" color={accent} title={c.tap[0]} sub={c.tap[1]} width={284} active={tapActive} scale={0.9} />
            <RowCard icon="hold" color={accent} title={c.hold[0]} sub={c.hold[1]} width={284} active={holdActive} scale={0.9} />
          </div>
        ) : null}
      </TextBlock>
      <Column l={l}>
        <RowCard icon="tap" color={accent} title={c.tap[0]} sub={c.tap[1]} width={l.col?.width ?? 300} active={tapActive} />
        <RowCard icon="hold" color={accent} title={c.hold[0]} sub={c.hold[1]} width={l.col?.width ?? 300} active={holdActive} />
      </Column>
    </Frame>
  );
};

// --------------------------------------------------------------------------- 4 three modes

export const Modes: React.FC<SceneProps> = (props) => {
  const l = useLayout(props, 40);
  const f = useCurrentFrame();
  const { fps } = useVideoConfig();
  const accent = C.emerald;
  const c = l.copy.modes;
  const S1 = 56;
  const S2 = 102;
  const s1 = spring({ frame: f - S1, fps, config: { damping: 16, mass: 0.7 } });
  const s2 = spring({ frame: f - S2, fps, config: { damping: 16, mass: 0.7 } });
  const sel = s1 + s2;

  // LASER: absolute jumps to targets.
  const targets: P[] = [
    { x: 760, y: 560 },
    { x: 420, y: 300 },
    { x: 1180, y: 260 },
    { x: 820, y: 640 },
  ];
  const laserPos = (fr: number): P => {
    const hops = [12, 26, 40];
    let p = targets[0];
    hops.forEach((h, i) => {
      p = lerpP(p, targets[i + 1], easeOutExpo(clamp01((fr - h) / 7)));
    });
    return p;
  };
  // AIR: smooth relative motion with acceleration.
  const airRoute: P[] = [targets[3], { x: 520, y: 480 }, { x: 760, y: 270 }, { x: 1240, y: 330 }, { x: 900, y: 620 }];
  const airPos = (fr: number): P => spline(airRoute, easeInOut(clamp01((fr - S1 - 2) / 40)));
  // TOUCH: finger on the watch face drives the cursor.
  const finger: P[] = [
    { x: 170, y: 250 },
    { x: 215, y: 170 },
    { x: 320, y: 145 },
    { x: 315, y: 240 },
    { x: 235, y: 225 },
  ];
  const fingerAt = (fr: number): P => spline(finger, easeInOut(clamp01((fr - S2 - 4) / 40)));
  const airEnd = airRoute[airRoute.length - 1];
  const touchPos = (fr: number): P => {
    const q = fingerAt(fr);
    return { x: airEnd.x + (q.x - finger[0].x) * 2.8, y: airEnd.y + (q.y - finger[0].y) * 2.8 };
  };
  const pos = (fr: number): P => (fr < S1 ? laserPos(fr) : fr < S2 ? airPos(fr) : touchPos(fr));
  const cur = pos(f);
  const mode = f < S1 ? 0 : f < S2 ? 1 : 2;
  const trailFrom = mode === 0 ? 1e9 : mode === 1 ? S1 : S2;
  const trail = mode === 0 ? [] : trailOf(pos, f, mode === 1 ? 22 : 14, 1, trailFrom);
  const t = mode === 2 ? { tiltX: 0, tiltY: 0 } : tiltFrom(cur, mode === 0 ? 1.2 : 1.1);
  const tiltK = 1 - s2;

  const sheet = screen("sheetMode");
  const sheetOp = sheet ? interp(f, [0, 10, 16], [1, 1, 0]) : 0;
  const layers = [
    { media: screenOrMain("mainLaser"), opacity: 1 - s1 },
    { media: screenOrMain("mainAir"), opacity: s1 * (1 - s2) },
    { media: screenOrMain("mainTouch"), opacity: s2 },
    ...(sheet ? [{ media: sheet, opacity: sheetOp }] : []),
  ];
  const fp = fingerAt(f);
  const fingerOverlay =
    mode === 2 ? (
      <g opacity={interp(f, [S2, S2 + 6], [0, 1])}>
        {trailOf(fingerAt, f, 10, 1, S2).map((p, i, a) => (
          <circle key={i} cx={p.x} cy={p.y} r={6 + (i / a.length) * 10} fill="#F8FAFC" opacity={(i / a.length) * 0.35} />
        ))}
        <circle cx={fp.x} cy={fp.y} r={34} fill="rgba(248,250,252,.28)" stroke="#F8FAFC" strokeWidth={3} />
        <circle cx={fp.x} cy={fp.y} r={46} fill="none" stroke={accent} strokeWidth={3} opacity={0.7} />
      </g>
    ) : null;

  const captions = c.items.map((it, i) => {
    const on = clamp01(1 - Math.abs(sel - i) * 1.6);
    return (
      <div
        key={it.key}
        style={{
          position: "absolute",
          left: 0,
          right: 0,
          top: 0,
          opacity: on,
          transform: `translateY(${(sel - i) * -18}px)`,
          fontSize: l.isV ? 34 : 32,
          lineHeight: 1.35,
          color: "#CBD5E1",
          textAlign: l.isV ? "center" : "left",
          textWrap: "balance",
        } as React.CSSProperties}
      >
        {it.caption}
      </div>
    );
  });
  const selW = l.isV ? 900 : 580;
  const selH = l.isV ? 100 : 88;
  const selIn = useSpring(14, 200);
  const modeIcons = ["center", "gyro", "tap"];
  return (
    <Frame l={l} accent={accent}>
      <Laptop w={l.laptop.w} left={l.laptop.left} top={l.laptop.top}>
        <Desktop id="modes" accent={accent} cursor={cur} trail={trail} laser={{ opacity: 1 - s1 }} />
      </Laptop>
      <Watch
        d={l.watch.d}
        cx={l.watch.cx}
        cy={l.watch.cy}
        layers={layers}
        glow={accent}
        tiltX={t.tiltX * tiltK}
        tiltY={t.tiltY * tiltK}
        overlay={fingerOverlay}
      />
      <TextBlock l={l} accent={accent} eyebrow={c.eyebrow} headline={c.headline} headSize={l.isV ? 88 : 88}>
        <div
          style={{
            marginTop: l.isV ? 44 : 48,
            display: "flex",
            justifyContent: l.isV ? "center" : "flex-start",
            opacity: selIn,
            transform: `translateY(${(1 - selIn) * 24}px)`,
          }}
        >
          <Segmented labels={c.items.map((i) => i.label)} pos={sel} width={selW} height={selH} accent={accent} lang={l.lang} />
        </div>
        <div style={{ position: "relative", marginTop: 34, height: l.isV ? 60 : 100, opacity: selIn }}>{captions}</div>
      </TextBlock>
      <Column l={l}>
        {c.items.map((it, i) => (
          <RowCard
            key={it.key}
            icon={modeIcons[i]}
            color={accent}
            title={it.label}
            sub={it.short}
            width={l.col?.width ?? 300}
            active={clamp01(1 - Math.abs(sel - i) * 1.5)}
          />
        ))}
      </Column>
    </Frame>
  );
};

// --------------------------------------------------------------------------- 5 sensitivity

export const Sensitivity: React.FC<SceneProps> = (props) => {
  const l = useLayout(props);
  const f = useCurrentFrame();
  const accent = C.emerald;
  const c = l.copy.sens;
  const value = f < 46 ? interp(f, [14, 44], [1600, 3200]) : interp(f, [52, 80], [3200, 2400]);
  const eased = value;
  const wat = useSpring(0, 200, 24);
  const d = l.isV ? 640 : 600;
  const cx = l.isV ? 540 : 520;
  const cy = l.isV ? 980 : 560;
  const rot = (eased - 1600) / 8;
  const arcR = d / 2 + 46;
  const arc = (a0: number, a1: number) => {
    const p = (a: number) => ({ x: arcR * Math.cos((a * Math.PI) / 180), y: -arcR * Math.sin((a * Math.PI) / 180) });
    const s = p(a0);
    const e = p(a1);
    return `M${s.x} ${s.y} A${arcR} ${arcR} 0 0 ${a1 < a0 ? 1 : 0} ${e.x} ${e.y}`;
  };
  return (
    <Frame l={l} accent={accent}>
      <svg
        style={{ position: "absolute", left: cx - arcR - 40, top: cy - arcR - 40, overflow: "visible" }}
        width={arcR * 2 + 80}
        height={arcR * 2 + 80}
        viewBox={`${-arcR - 40} ${-arcR - 40} ${arcR * 2 + 80} ${arcR * 2 + 80}`}
      >
        <defs>
          <marker id="sens-ah" viewBox="0 0 10 10" refX="5" refY="5" markerWidth="4" markerHeight="4" orient="auto-start-reverse">
            <path d="M0 0 L10 5 L0 10 z" fill={accent} />
          </marker>
        </defs>
        <g transform={`rotate(${rot})`} opacity={wat}>
          <path d={arc(70, 20)} fill="none" stroke={accent} strokeWidth={8} strokeLinecap="round" markerEnd="url(#sens-ah)" />
          <path d={arc(250, 200)} fill="none" stroke={accent} strokeWidth={8} strokeLinecap="round" markerEnd="url(#sens-ah)" />
          <circle r={arcR} fill="none" stroke={accent} strokeOpacity={0.25} strokeWidth={2} strokeDasharray="2 16" />
        </g>
      </svg>
      <Watch d={d} cx={cx} cy={cy + (1 - wat) * 80} opacity={wat} layers={[{ media: screenOrMain("sensitivity") }]} glow={accent} tiltY={Math.sin(f / 18) * 4} />
      <div
        style={
          l.isV
            ? l.text
            : ({ position: "absolute", left: 1010, top: 250, width: 800, textAlign: "left" } as React.CSSProperties)
        }
      >
        <Rise delay={0} dist={16}>
          <Eyebrow text={c.eyebrow} accent={accent} lang={l.lang} size={l.isV ? 24 : 22} />
        </Rise>
        <Reveal text={c.headline} lang={l.lang} accent={accent} delay={4} style={headStyle(l)} />
        <Rise delay={16} dist={20}>
          <div style={subStyle(l)}>{c.sub}</div>
        </Rise>
      </div>
      <Rise delay={12} dist={30} style={{ position: "absolute", left: l.isV ? 70 : 1010, top: l.isV ? 1450 : 640 }}>
        <DpiSlider value={eased} width={l.isV ? 940 : 790} accent={accent} caption={c.steps} />
      </Rise>
    </Frame>
  );
};

// --------------------------------------------------------------------------- 6 pause / recenter

export const Pause: React.FC<SceneProps> = (props) => {
  const l = useLayout(props);
  const f = useCurrentFrame();
  const accent = C.amber;
  const c = l.copy.pause;
  const TAP = 66;
  const frozen: P = { x: 1150, y: 330 };
  const center: P = { x: 800, y: 500 };
  const cur = lerpP(frozen, center, easeOutExpo(clamp01((f - (TAP + 4)) / 12)));
  const wrist = interp(f, [8, TAP - 4], [0, 1]) * interp(f, [TAP - 4, TAP + 4], [1, 0]);
  const tiltY = Math.sin(f / 7) * 16 * wrist;
  const tiltX = Math.cos(f / 9) * 8 * wrist;
  const badge = interp(f, [6, 14, TAP, TAP + 6], [0, 1, 1, 0]);
  const tap = clamp01((f - TAP) / 18);
  const pauseActive = interp(f, [0, 6, TAP - 2, TAP + 4], [0, 1, 1, 0]);
  const centerActive = interp(f, [TAP - 2, TAP + 4], [0, 1]);
  const overlay = (
    <g>
      {f >= TAP - 2 ? (
        <rect
          x={182}
          y={294}
          width={116}
          height={92}
          rx={30}
          fill={rgba(C.emerald, 0.18 * (1 - tap))}
          stroke={C.emerald}
          strokeWidth={5}
          opacity={interp(f, [TAP - 2, TAP + 2, TAP + 30, TAP + 40], [0, 1, 1, 0.35])}
        />
      ) : null}
      {tap > 0 && tap < 1 ? <circle cx={240} cy={340} r={20 + tap * 120} fill="none" stroke={C.emerald} strokeWidth={4} opacity={1 - tap} /> : null}
    </g>
  );
  return (
    <Frame l={l} accent={accent}>
      <Laptop w={l.laptop.w} left={l.laptop.left} top={l.laptop.top}>
        <Desktop
          id="pause"
          accent={f < TAP ? accent : C.emerald}
          cursor={cur}
          badge={{ text: c.paused, color: accent, opacity: badge }}
          pulse={{ p: center, t: clamp01((f - (TAP + 10)) / 26), color: C.emerald }}
        />
      </Laptop>
      <Watch d={l.watch.d} cx={l.watch.cx} cy={l.watch.cy} layers={[{ media: screenOrMain("paused") }]} glow={accent} tiltX={tiltX} tiltY={tiltY} overlay={overlay} />
      <TextBlock l={l} accent={accent} eyebrow={c.eyebrow} headline={c.headline} sub={c.sub}>
        {!l.isV ? (
          <div style={{ display: "flex", gap: 16, marginTop: 40 }}>
            <RowCard icon="pause" color={accent} title={c.pauseCard[0]} sub={c.pauseCard[1]} width={284} active={pauseActive} scale={0.9} />
            <RowCard icon="center" color={C.emerald} title={c.centerCard[0]} sub={c.centerCard[1]} width={284} active={centerActive} scale={0.9} />
          </div>
        ) : null}
      </TextBlock>
      <Column l={l}>
        <RowCard icon="pause" color={accent} title={c.pauseCard[0]} sub={c.pauseCard[1]} width={l.col?.width ?? 300} active={pauseActive} />
        <RowCard icon="center" color={C.emerald} title={c.centerCard[0]} sub={c.centerCard[1]} width={l.col?.width ?? 300} active={centerActive} />
      </Column>
    </Frame>
  );
};

// --------------------------------------------------------------------------- 7 outro

export const Outro: React.FC<SceneProps> = (props) => {
  const l = useLayout(props);
  const accent = C.emerald;
  const c = l.copy.outro;
  const icon = useSpring(0, 12);
  const k = l.isV ? 1.12 : 1;
  return (
    <Frame l={l} accent={accent}>
      <AbsoluteFill style={{ display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", textAlign: "center", padding: 80 }}>
        <div style={{ position: "relative" }}>
          <div style={{ position: "absolute", left: "50%", top: "50%", width: 520 * k, height: 520 * k, transform: "translate(-50%,-50%)", borderRadius: "50%", background: `radial-gradient(circle, ${rgba(accent, 0.35)} 0%, transparent 65%)` }} />
          <Img
            src={iconSrc()}
            style={{
              position: "relative",
              width: 200 * k,
              height: 200 * k,
              borderRadius: "22.5%",
              transform: `scale(${0.5 + 0.5 * icon})`,
              opacity: clamp01(icon * 1.4),
              boxShadow: `0 0 0 2px ${rgba(accent, 0.4)}, 0 30px 80px rgba(0,0,0,.6), 0 0 120px ${rgba(accent, 0.35)}`,
            }}
          />
        </div>
        <Rise delay={6} dist={20}>
          <div style={{ marginTop: 44 * k, fontSize: 92 * k, fontWeight: 800, letterSpacing: "-0.03em", lineHeight: 1 }}>{l.copy.brand}</div>
        </Rise>
        <Rise delay={10} dist={16}>
          <div style={{ marginTop: 22 * k, fontSize: 32 * k, color: C.muted, maxWidth: l.isV ? 900 : 1200, textWrap: "balance" } as React.CSSProperties}>{c.tagline}</div>
        </Rise>
        <div style={{ display: "flex", gap: 22 * k, marginTop: 52 * k, flexWrap: "wrap", justifyContent: "center" }}>
          {c.items.map((it, i) => (
            <Rise key={it} delay={18 + i * 6} dist={18}>
              <div style={{ display: "flex", alignItems: "center", gap: 14 * k, padding: `${14 * k}px ${26 * k}px ${14 * k}px ${16 * k}px`, borderRadius: 999, background: rgba(accent, 0.1), border: `1.5px solid ${rgba(accent, 0.45)}` }}>
                <div style={{ width: 40 * k, height: 40 * k, borderRadius: "50%", display: "grid", placeItems: "center", background: rgba(accent, 0.2) }}>
                  <Icon name="check" size={24 * k} color={accent} stroke={2.8} />
                </div>
                <span style={{ fontSize: 32 * k, fontWeight: 650 }}>{it}</span>
              </div>
            </Rise>
          ))}
        </div>
        <Rise delay={38} dist={16}>
          <div style={{ display: "flex", alignItems: "center", gap: 18 * k, marginTop: 56 * k, flexWrap: "wrap", justifyContent: "center" }}>
            <span style={{ fontFamily: MONO, fontSize: 30 * k, color: "#E2E8F0", padding: `${14 * k}px ${28 * k}px`, borderRadius: 18 * k, background: "rgba(15,23,42,.75)", border: "1.5px solid rgba(148,163,184,.25)" }}>
              {REPO}
            </span>
            <span style={{ fontFamily: MONO, fontSize: 28 * k, fontWeight: 700, color: "#04120c", padding: `${14 * k}px ${22 * k}px`, borderRadius: 18 * k, background: accent }}>{VERSION}</span>
          </div>
        </Rise>
        <Rise delay={46} dist={12}>
          <div style={{ marginTop: 34 * k, fontFamily: MONO, fontSize: 20 * k, letterSpacing: "0.24em", color: C.dim }}>MIT · macOS · WINDOWS · LINUX</div>
        </Rise>
      </AbsoluteFill>
    </Frame>
  );
};
