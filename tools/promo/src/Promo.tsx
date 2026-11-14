import React from "react";
import {
  AbsoluteFill,
  Easing,
  Img,
  interpolate,
  spring,
  staticFile,
  useCurrentFrame,
  useVideoConfig,
} from "remotion";
import { Video } from "@remotion/media";
import { TransitionSeries, linearTiming } from "@remotion/transitions";
import { fade } from "@remotion/transitions/fade";
import { loadFont as loadSans } from "@remotion/google-fonts/NotoSansTC";
import { loadFont as loadMono } from "@remotion/google-fonts/JetBrainsMono";

// Opening scene starts settled so frame 0 (social auto-thumbnail) is a complete hero poster.
const Settled = React.createContext(0);

export const { fontFamily: sansFont } = loadSans("normal", {
  weights: ["400", "500", "700", "900"],
  subsets: ["chinese-traditional", "latin"],
  ignoreTooManyRequestsWarning: true,
});

export const { fontFamily: monoFont } = loadMono("normal", {
  weights: ["500", "700"],
  subsets: ["latin"],
});

export type Orientation = "vertical" | "horizontal";
export type Lang = "zh" | "en";

// Keyframe inside the 1920x1080 desktop capture:
// x = center X in capture px, y = top Y in capture px, z = zoom >= 1.0 (never < 1)
export type DeskKey = { t: number; x: number; y: number; z?: number };

export type SceneSpec = {
  id: string;
  deskStart: number;
  watchStart: number;
  rawSeconds: number;
  speed: number;
  accentColor: string;
  glowA: string;
  glowB: string;
  packetTag: string;
  metricLabel: string;
  metricValue: string;
  zh: {
    chip: string;
    title: string;
    accent: string;
    sub: string;
  };
  en: {
    chip: string;
    title: string;
    accent: string;
    sub: string;
  };
  keysH: DeskKey[];
  keysV: DeskKey[];
};

const DESK_W = 1920;
const DESK_H = 1080;
export const FPS = 30;
export const XFADE = 10;
export const END_FRAMES = 96; // 3.2s punchy end card

// Sync offset: watch_sync.mp4 and desk_sync.mp4 are pre-aligned at t=0 via clapperboard!
export const SCENES: SceneSpec[] = [
  {
    id: "aim-click",
    deskStart: 0.0,
    watchStart: 0.0,
    rawSeconds: 6.4,
    speed: 1.08,
    accentColor: "#10B981",
    glowA: "rgba(16, 185, 129, 0.28)",
    glowB: "rgba(56, 189, 248, 0.22)",
    packetTag: "0x00 AIM · 0x01 L_CLICK",
    metricLabel: "CURSOR LOOP",
    metricValue: "100→240 Hz",
    zh: {
      chip: "開源專案 · Wear OS × Hyprland",
      title: "把智慧手錶變成",
      accent: "100 Hz 空中滑鼠",
      sub: "陀螺儀＋重力感測直連 BLE・流暢點擊 Wayland 桌面",
    },
    en: {
      chip: "Open Source · Wear OS × Hyprland",
      title: "Your Watch as a",
      accent: "100 Hz Air Mouse",
      sub: "Gyro + gravity over 12-byte BLE GATT · Native Wayland pointer",
    },
    keysH: [
      { t: 0.0, x: 620, y: 0, z: 1.12 },
      { t: 3.2, x: 650, y: 0, z: 1.14 },
      { t: 6.4, x: 640, y: 30, z: 1.12 },
    ],
    keysV: [
      { t: 0.0, x: 420, y: 0, z: 1.48 },
      { t: 1.6, x: 340, y: 0, z: 1.52 },
      { t: 3.3, x: 640, y: 0, z: 1.52 },
      { t: 5.0, x: 930, y: 0, z: 1.52 },
      { t: 6.4, x: 520, y: 120, z: 1.45 },
    ],
  },
  {
    id: "drag-rclick",
    deskStart: 7.06,
    watchStart: 7.06,
    rawSeconds: 5.92,
    speed: 1.05,
    accentColor: "#38BDF8",
    glowA: "rgba(56, 189, 248, 0.28)",
    glowB: "rgba(16, 185, 129, 0.24)",
    packetTag: "0x07 DRAG · 0x02 R_CLICK",
    metricLabel: "HOLD GESTURE",
    metricValue: "350 ms LOCK",
    zh: {
      chip: "單手直覺手勢",
      title: "長按 350 ms",
      accent: "鎖定拖曳・右鍵選單",
      sub: "進度環滿格即鎖定按住，轉動手腕輕鬆拖放視窗物件",
    },
    en: {
      chip: "One-Handed Ergonomics",
      title: "350 ms Hold",
      accent: "Drag Lock & Right-Click",
      sub: "Radial progress locks button-down for effortless window dragging",
    },
    keysH: [
      { t: 0.0, x: 610, y: 60, z: 1.12 },
      { t: 3.0, x: 740, y: 80, z: 1.14 },
      { t: 5.2, x: 680, y: 60, z: 1.14 },
    ],
    keysV: [
      { t: 0.0, x: 380, y: 310, z: 1.52 },
      { t: 2.9, x: 880, y: 310, z: 1.52 },
      { t: 4.8, x: 720, y: 290, z: 1.52 },
    ],
  },
  {
    id: "clutch-center",
    deskStart: 13.0,
    watchStart: 13.0,
    rawSeconds: 4.9,
    speed: 1.04,
    accentColor: "#F59E0B",
    glowA: "rgba(245, 158, 11, 0.26)",
    glowB: "rgba(56, 189, 248, 0.24)",
    packetTag: "0x05 PAUSE · 0x04 CENTER",
    metricLabel: "CLUTCH MODE",
    metricValue: "ZERO DRIFT",
    zh: {
      chip: "防誤觸離合器",
      title: "一鍵暫停離合",
      accent: "瞬間歸零校正",
      sub: "Pause 轉動手腕游標紋絲不動・Center 立即對準螢幕中央",
    },
    en: {
      chip: "Clutch & Calibration",
      title: "Instant Clutch",
      accent: "& One-Tap Recenter",
      sub: "Move your wrist freely while paused · Snap dead-center anytime",
    },
    keysH: [
      { t: 0.0, x: 880, y: 40, z: 1.12 },
      { t: 2.6, x: 920, y: 40, z: 1.14 },
      { t: 4.9, x: 960, y: 50, z: 1.16 },
    ],
    keysV: [
      { t: 0.0, x: 630, y: 210, z: 1.5 },
      { t: 2.8, x: 650, y: 210, z: 1.5 },
      { t: 4.9, x: 920, y: 240, z: 1.52 },
    ],
  },
  {
    id: "telemetry-rust",
    deskStart: 17.92,
    watchStart: 17.92,
    rawSeconds: 5.4,
    speed: 1.08,
    accentColor: "#A78BFA",
    glowA: "rgba(167, 139, 250, 0.28)",
    glowB: "rgba(16, 185, 129, 0.24)",
    packetTag: "20B MAX GATT · RUST CORE",
    metricLabel: "PACKET DROP",
    metricValue: "0 DROP/S",
    zh: {
      chip: "極致輕量協定",
      title: "即時封包監控",
      accent: "Rust 原生接收端",
      sub: "錶端即時顯示 100 pkt/s 零掉包・支援 Hyprland 與 uinput",
    },
    en: {
      chip: "Compact Binary Protocol",
      title: "Live Telemetry",
      accent: "& Rust BLE Receiver",
      sub: "100 pkt/s zero-drop HUD · Hyprland & Linux uinput backends",
    },
    keysH: [
      { t: 0.0, x: 1340, y: 0, z: 1.12 },
      { t: 2.8, x: 1380, y: 10, z: 1.15 },
      { t: 5.4, x: 1360, y: 10, z: 1.14 },
    ],
    keysV: [
      { t: 0.0, x: 1480, y: 0, z: 1.48 },
      { t: 2.8, x: 1540, y: 10, z: 1.52 },
      { t: 5.4, x: 1500, y: 10, z: 1.48 },
    ],
  },
];

export const sceneFrames = (s: SceneSpec) =>
  Math.ceil((s.rawSeconds / s.speed) * FPS);

export const totalFrames = () =>
  SCENES.reduce((acc, s) => acc + sceneFrames(s), 0) +
  END_FRAMES -
  XFADE * SCENES.length;

const ease = Easing.inOut(Easing.cubic);

const trackDesk = (keys: DeskKey[], t: number) => {
  if (keys.length === 1) {
    return { x: keys[0].x, y: keys[0].y, z: Math.max(1, keys[0].z ?? 1) };
  }
  const ts = keys.map((k) => k.t);
  const opts = {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
    easing: ease,
  } as const;
  return {
    x: interpolate(t, ts, keys.map((k) => k.x), opts),
    y: interpolate(t, ts, keys.map((k) => k.y), opts),
    z: Math.max(1, interpolate(t, ts, keys.map((k) => k.z ?? 1), opts)),
  };
};

const Background: React.FC<{ glowA: string; glowB: string }> = ({
  glowA,
  glowB,
}) => {
  const frame = useCurrentFrame();
  const drift = Math.sin(frame / 75) * 32;
  return (
    <AbsoluteFill
      style={{
        background: "linear-gradient(140deg, #05080F 0%, #09101E 55%, #060B14 100%)",
      }}
    >
      <AbsoluteFill
        style={{
          background: `radial-gradient(circle at ${76 + drift / 18}% ${20 + drift / 28}%, ${glowA} 0%, transparent 46%),
            radial-gradient(circle at ${18 - drift / 22}% ${82 - drift / 28}%, ${glowB} 0%, transparent 48%)`,
        }}
      />
      <AbsoluteFill
        style={{
          backgroundImage:
            "radial-gradient(rgba(255,255,255,0.06) 1.2px, transparent 1.3px)",
          backgroundSize: "40px 40px",
        }}
      />
    </AbsoluteFill>
  );
};

const Rise: React.FC<{
  delay: number;
  children: React.ReactNode;
  style?: React.CSSProperties;
}> = ({ delay, children, style }) => {
  const frame = useCurrentFrame() + React.useContext(Settled);
  const { fps } = useVideoConfig();
  const p = spring({ frame: frame - delay, fps, config: { damping: 200 } });
  return (
    <div
      style={{
        ...style,
        opacity: p,
        transform: `translateY(${(1 - p) * 24}px)`,
      }}
    >
      {children}
    </div>
  );
};

const Chip: React.FC<{ text: string; accent: string; size: number }> = ({
  text,
  accent,
  size,
}) => (
  <span
    style={{
      display: "inline-flex",
      alignItems: "center",
      gap: 10,
      padding: `${Math.round(size * 0.34)}px ${Math.round(size * 0.85)}px`,
      borderRadius: 999,
      background: "rgba(15, 23, 42, 0.82)",
      border: `1.5px solid ${accent}66`,
      color: "#F8FAFC",
      fontSize: size,
      fontWeight: 700,
      letterSpacing: 0.6,
    }}
  >
    <span
      style={{
        width: 10,
        height: 10,
        borderRadius: "50%",
        background: accent,
        boxShadow: `0 0 12px ${accent}`,
      }}
    />
    {text}
  </span>
);

const DesktopWindowCard: React.FC<{
  scene: SceneSpec;
  box: { x: number; y: number; w: number; h: number; r: number };
  keys: DeskKey[];
}> = ({ scene, box, keys }) => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const localSec = (frame / fps) * scene.speed;
  const { x, y, z } = trackDesk(keys, localSec);

  // Never zoom below 1: guarantee scaled capture covers both box.w and box.h
  const minScale = Math.max(box.w / DESK_W, box.h / DESK_H);
  const s = minScale * Math.max(1, z);
  const visibleW = box.w / s;
  const visibleH = box.h / s;
  const left = Math.min(Math.max(x - visibleW / 2, 0), DESK_W - visibleW);
  const top = Math.min(Math.max(y, 0), DESK_H - visibleH);

  const enter = spring({
    frame: frame + React.useContext(Settled) - 3,
    fps,
    config: { damping: 200 },
  });

  return (
    <div
      style={{
        position: "absolute",
        left: box.x,
        top: box.y,
        width: box.w,
        height: box.h,
        opacity: enter,
        transform: `translateY(${(1 - enter) * 36}px)`,
      }}
    >
      <div
        style={{
          position: "absolute",
          inset: 0,
          borderRadius: box.r,
          overflow: "hidden",
          background: "#070B14",
          boxShadow:
            "0 36px 84px rgba(0,0,0,0.62), 0 8px 24px rgba(0,0,0,0.4)",
        }}
      >
        <div
          style={{
            position: "absolute",
            left: -left * s,
            top: -top * s,
            width: DESK_W * s,
            height: DESK_H * s,
          }}
        >
          <Video
            src={staticFile("desk_sync.mp4")}
            playbackRate={scene.speed}
            trimBefore={Math.round(scene.deskStart * fps)}
            muted
            objectFit="fill"
            style={{ width: "100%", height: "100%" }}
          />
        </div>
      </div>
      {/* Border drawn on the exact same DOM box so it can never drift */}
      <div
        style={{
          position: "absolute",
          inset: 0,
          borderRadius: box.r,
          border: `2px solid ${scene.accentColor}66`,
          pointerEvents: "none",
        }}
      />
      {/* Top-right live badge */}
      <div
        style={{
          position: "absolute",
          right: 20,
          bottom: 18,
          padding: "8px 16px",
          borderRadius: 999,
          background: "rgba(6, 10, 19, 0.88)",
          border: "1px solid rgba(255,255,255,0.16)",
          fontFamily: monoFont,
          fontSize: 18,
          fontWeight: 700,
          color: "#E2E8F0",
          letterSpacing: 0.5,
        }}
      >
        Hyprland 0.57 · Wayland Live
      </div>
    </div>
  );
};

const WatchRoundBezel: React.FC<{
  scene: SceneSpec;
  x: number;
  y: number;
  size: number;
}> = ({ scene, x, y, size }) => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const enter = spring({
    frame: frame + React.useContext(Settled) - 6,
    fps,
    config: { damping: 200 },
  });

  return (
    <div
      style={{
        position: "absolute",
        left: x,
        top: y,
        width: size,
        height: size,
        opacity: enter,
        transform: `scale(${0.92 + 0.08 * enter})`,
      }}
    >
      {/* Subtle ambient glow behind round watch */}
      <div
        style={{
          position: "absolute",
          inset: -18,
          borderRadius: "50%",
          background: `radial-gradient(circle, ${scene.accentColor}44 0%, transparent 70%)`,
        }}
      />
      {/* Machined outer watch case */}
      <div
        style={{
          position: "absolute",
          inset: -10,
          borderRadius: "50%",
          background: "linear-gradient(145deg, #1E293B 0%, #090D16 65%, #111827 100%)",
          boxShadow: "0 28px 64px rgba(0,0,0,0.72)",
          border: "1.5px solid rgba(255,255,255,0.16)",
        }}
      />
      {/* Exact circular screen container + overlay border on same DOM box */}
      <div
        style={{
          position: "absolute",
          inset: 0,
          borderRadius: "50%",
          overflow: "hidden",
          background: "#000000",
        }}
      >
        <Video
          src={staticFile("watch_sync.mp4")}
          playbackRate={scene.speed}
          trimBefore={Math.round(scene.watchStart * fps)}
          muted
          objectFit="cover"
          style={{ width: "100%", height: "100%" }}
        />
      </div>
      <div
        style={{
          position: "absolute",
          inset: 0,
          borderRadius: "50%",
          border: `2.5px solid ${scene.accentColor}AA`,
          pointerEvents: "none",
        }}
      />
    </div>
  );
};

const BlePulseBridge: React.FC<{
  scene: SceneSpec;
  x: number;
  y: number;
  w: number;
}> = ({ scene, x, y, w }) => {
  const frame = useCurrentFrame();
  const dotPos = (frame % 24) / 24;
  return (
    <div
      style={{
        position: "absolute",
        left: x,
        top: y,
        width: w,
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        gap: 8,
        fontFamily: monoFont,
      }}
    >
      <div
        style={{
          padding: "6px 14px",
          borderRadius: 999,
          background: "rgba(9, 14, 26, 0.92)",
          border: `1.5px solid ${scene.accentColor}88`,
          color: scene.accentColor,
          fontSize: 16,
          fontWeight: 700,
          whiteSpace: "nowrap",
        }}
      >
        {scene.metricValue}
      </div>
      <div
        style={{
          position: "relative",
          width: "100%",
          height: 4,
          borderRadius: 4,
          background: "rgba(255,255,255,0.14)",
          overflow: "hidden",
        }}
      >
        <div
          style={{
            position: "absolute",
            left: `${dotPos * 85}%`,
            top: 0,
            width: "22%",
            height: "100%",
            borderRadius: 4,
            background: scene.accentColor,
            boxShadow: `0 0 10px ${scene.accentColor}`,
          }}
        />
      </div>
      <div
        style={{
          fontSize: 13,
          color: "#94A3B8",
          fontWeight: 500,
          whiteSpace: "nowrap",
        }}
      >
        12B GATT ⇢
      </div>
    </div>
  );
};

const SceneView: React.FC<{
  scene: SceneSpec;
  o: Orientation;
  lang: Lang;
}> = ({ scene, o, lang }) => {
  const copy = scene[lang];
  const isV = o === "vertical";

  if (!isV) {
    // 16:9 Horizontal (1920x1080): Left info + large round watch, Right large Hyprland desktop
    return (
      <AbsoluteFill style={{ fontFamily: sansFont }}>
        <Background glowA={scene.glowA} glowB={scene.glowB} />

        {/* Top-left brand header */}
        <div
          style={{
            position: "absolute",
            left: 72,
            top: 52,
            display: "flex",
            alignItems: "center",
            gap: 14,
          }}
        >
          <Img
            src={staticFile("icon.png")}
            style={{ width: 46, height: 46, borderRadius: 12 }}
          />
          <span
            style={{
              fontFamily: monoFont,
              fontSize: 22,
              fontWeight: 700,
              color: "#F8FAFC",
              letterSpacing: 0.4,
            }}
          >
            wear-pointer
          </span>
          <span
            style={{
              fontFamily: monoFont,
              fontSize: 16,
              fontWeight: 700,
              color: "#10B981",
              padding: "3px 10px",
              borderRadius: 999,
              background: "rgba(16, 185, 129, 0.14)",
              border: "1px solid rgba(16, 185, 129, 0.35)",
            }}
          >
            v0.1.0
          </span>
        </div>

        {/* Left headline block */}
        <div
          style={{
            position: "absolute",
            left: 72,
            top: 126,
            width: 596,
            display: "flex",
            flexDirection: "column",
            alignItems: "flex-start",
            color: "#FFFFFF",
          }}
        >
          <Rise delay={2}>
            <Chip
              text={copy.chip}
              accent={scene.accentColor}
              size={lang === "zh" ? 23 : 21}
            />
          </Rise>
          <Rise
            delay={6}
            style={{
              fontSize: lang === "zh" ? 60 : 58,
              fontWeight: 900,
              lineHeight: 1.15,
              marginTop: 18,
              letterSpacing: -0.8,
              whiteSpace: "nowrap",
            }}
          >
            {copy.title}
          </Rise>
          <Rise
            delay={10}
            style={{
              fontSize: lang === "zh" ? 60 : 58,
              fontWeight: 900,
              lineHeight: 1.15,
              color: scene.accentColor,
              letterSpacing: -0.8,
              whiteSpace: "nowrap",
            }}
          >
            {copy.accent}
          </Rise>
          <Rise
            delay={14}
            style={{
              fontSize: lang === "zh" ? 27 : 25,
              fontWeight: 500,
              color: "#CBD5E1",
              lineHeight: 1.45,
              marginTop: 16,
              maxWidth: 570,
            }}
          >
            {copy.sub}
          </Rise>
        </div>

        {/* Bottom-left live round Wear OS Watch (416x416 -> 92% native 454px clarity!) */}
        <WatchRoundBezel scene={scene} x={80} y={584} size={416} />

        {/* Animated BLE packet bridge between Watch and Hyprland Desktop */}
        <BlePulseBridge scene={scene} x={514} y={754} w={172} />

        {/* Right Hyprland Wayland Desktop Card */}
        <DesktopWindowCard
          scene={scene}
          box={{ x: 704, y: 56, w: 1152, h: 968, r: 34 }}
          keys={scene.keysH}
        />
      </AbsoluteFill>
    );
  }

  // 9:16 Vertical (1080x1920) — strict Reels / Shorts / 4:5 feed crop safe zones:
  // Top text >= 260, right clear >= 110px (content width <= 956), bottom <= 1625 (<= 1635)!
  return (
    <AbsoluteFill style={{ fontFamily: sansFont }}>
      <Background glowA={scene.glowA} glowB={scene.glowB} />

      {/* Caption safe zone: y=262..552, x=60..960 */}
      <div
        style={{
          position: "absolute",
          left: 60,
          top: 262,
          width: 904,
          display: "flex",
          flexDirection: "column",
          alignItems: "flex-start",
          color: "#FFFFFF",
        }}
      >
        <Rise delay={2}>
          <Chip
            text={copy.chip}
            accent={scene.accentColor}
            size={lang === "zh" ? 26 : 24}
          />
        </Rise>
        <Rise
          delay={6}
          style={{
            fontSize: lang === "zh" ? 72 : 64,
            fontWeight: 900,
            lineHeight: 1.12,
            marginTop: 14,
            letterSpacing: -0.8,
          }}
        >
          {copy.title}
        </Rise>
        <Rise
          delay={10}
          style={{
            fontSize: lang === "zh" ? 72 : 64,
            fontWeight: 900,
            lineHeight: 1.12,
            color: scene.accentColor,
            letterSpacing: -0.8,
          }}
        >
          {copy.accent}
        </Rise>
        <Rise
          delay={14}
          style={{
            fontSize: lang === "zh" ? 31 : 28,
            fontWeight: 500,
            color: "#CBD5E1",
            lineHeight: 1.38,
            marginTop: 12,
          }}
        >
          {copy.sub}
        </Rise>
      </div>

      {/* Hyprland Desktop Window: y=578..1202, x=60..964 (right 116px clear for Reels buttons!) */}
      <DesktopWindowCard
        scene={scene}
        box={{ x: 60, y: 578, w: 904, h: 616, r: 32 }}
        keys={scene.keysV}
      />

      {/* Bottom Dock: Watch (x=68, y=1228, size=388 -> bottom=1616 <= 1635!) + Live Spec Card */}
      <WatchRoundBezel scene={scene} x={68} y={1228} size={388} />

      <Rise
        delay={12}
        style={{
          position: "absolute",
          left: 484,
          top: 1264,
          width: 480,
          height: 318,
          borderRadius: 28,
          background: "rgba(9, 15, 28, 0.90)",
          border: `1.5px solid ${scene.accentColor}55`,
          padding: "28px 30px",
          display: "flex",
          flexDirection: "column",
          justifyContent: "space-between",
          boxShadow: "0 20px 50px rgba(0,0,0,0.45)",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
          <Img
            src={staticFile("icon.png")}
            style={{ width: 42, height: 42, borderRadius: 10 }}
          />
          <div>
            <div
              style={{
                fontFamily: monoFont,
                fontSize: 22,
                fontWeight: 700,
                color: "#F8FAFC",
              }}
            >
              wear-pointer
            </div>
            <div
              style={{
                fontFamily: monoFont,
                fontSize: 15,
                color: "#94A3B8",
              }}
            >
              SYNCED LIVE CAPTURE
            </div>
          </div>
        </div>

        <div>
          <div
            style={{
              fontFamily: monoFont,
              fontSize: 16,
              color: "#94A3B8",
              letterSpacing: 1,
            }}
          >
            {scene.metricLabel}
          </div>
          <div
            style={{
              fontFamily: monoFont,
              fontSize: 42,
              fontWeight: 700,
              color: scene.accentColor,
              marginTop: 4,
            }}
          >
            {scene.metricValue}
          </div>
        </div>

        <div
          style={{
            fontFamily: monoFont,
            fontSize: 17,
            color: "#E2E8F0",
            padding: "8px 14px",
            borderRadius: 12,
            background: "rgba(255,255,255,0.06)",
          }}
        >
          {scene.packetTag}
        </div>
      </Rise>
    </AbsoluteFill>
  );
};

const END_COPY = {
  zh: {
    tagline: "Wear OS 100 Hz BLE 空中滑鼠",
    features: "Kotlin 圓錶介面 × Rust 原生接收端 × Hyprland / Linux uinput",
    cta: "GitLab / GitHub 開源釋出 v0.1.0",
    repo: "gitlab.com/aa22396584/wear-pointer",
    note: "Wear OS 模擬器與 Hyprland 0.57 雙端同步實錄 · MIT 開源授權 · Wear OS 為 Google LLC 商標",
  },
  en: {
    tagline: "100 Hz BLE Air Mouse for Hyprland & Linux",
    features: "Kotlin Compose Watch App · Rust Receiver · 240 Hz 1€ Filter",
    cta: "Open Source v0.1.0 on GitLab & GitHub",
    repo: "gitlab.com/aa22396584/wear-pointer",
    note: "Synchronized live capture on Wear OS + Hyprland 0.57 · MIT License · Wear OS is a trademark of Google LLC",
  },
};

const EndCard: React.FC<{ o: Orientation; lang: Lang }> = ({ o, lang }) => {
  const v = o === "vertical";
  const t = END_COPY[lang];
  return (
    <AbsoluteFill style={{ fontFamily: sansFont }}>
      <Background
        glowA="rgba(16, 185, 129, 0.32)"
        glowB="rgba(56, 189, 248, 0.28)"
      />
      <AbsoluteFill
        style={{
          justifyContent: "center",
          alignItems: "center",
          textAlign: "center",
          color: "#FFFFFF",
          padding: v ? "0 132px 210px 64px" : "0 120px",
        }}
      >
        <Rise delay={0}>
          <Img
            src={staticFile("icon.png")}
            style={{
              width: v ? 184 : 168,
              height: v ? 184 : 168,
              borderRadius: v ? 42 : 38,
              boxShadow: "0 28px 64px rgba(0,0,0,0.6)",
            }}
          />
        </Rise>
        <Rise
          delay={4}
          style={{
            fontFamily: monoFont,
            fontSize: v ? 80 : 92,
            fontWeight: 700,
            marginTop: 22,
            letterSpacing: -1.5,
          }}
        >
          wear-pointer
        </Rise>
        <Rise
          delay={8}
          style={{
            fontSize: v ? 42 : 46,
            fontWeight: 800,
            marginTop: 10,
            color: "#10B981",
          }}
        >
          {t.tagline}
        </Rise>
        <Rise
          delay={12}
          style={{
            fontSize: v ? 27 : 30,
            color: "#E2E8F0",
            marginTop: 16,
            fontWeight: 500,
            lineHeight: 1.45,
            maxWidth: v ? 820 : 1100,
          }}
        >
          {t.features}
        </Rise>
        <Rise delay={16} style={{ marginTop: v ? 40 : 34 }}>
          <div
            style={{
              display: "inline-flex",
              flexDirection: "column",
              alignItems: "center",
              gap: 8,
              padding: v ? "20px 42px" : "18px 44px",
              borderRadius: 24,
              background: "rgba(16, 185, 129, 0.16)",
              border: "2px solid rgba(16, 185, 129, 0.55)",
            }}
          >
            <span style={{ fontSize: v ? 34 : 32, fontWeight: 800 }}>
              {t.cta}
            </span>
            <span
              style={{
                fontFamily: monoFont,
                fontSize: v ? 25 : 24,
                color: "#6EE7B7",
                fontWeight: 700,
              }}
            >
              {t.repo}
            </span>
          </div>
        </Rise>
        <Rise
          delay={20}
          style={{
            marginTop: v ? 40 : 32,
            fontSize: v ? 21 : 20,
            color: "rgba(226, 232, 240, 0.68)",
            lineHeight: 1.5,
            maxWidth: v ? 860 : 1200,
          }}
        >
          {t.note}
        </Rise>
      </AbsoluteFill>
    </AbsoluteFill>
  );
};

export const Promo: React.FC<{ orientation: Orientation; lang?: Lang }> = ({
  orientation,
  lang = "zh",
}) => (
  <TransitionSeries>
    {SCENES.flatMap((scene, i) => [
      <TransitionSeries.Sequence
        key={`s-${scene.id}`}
        durationInFrames={sceneFrames(scene)}
      >
        <Settled.Provider value={i === 0 ? 45 : 0}>
          <SceneView scene={scene} o={orientation} lang={lang} />
        </Settled.Provider>
      </TransitionSeries.Sequence>,
      <TransitionSeries.Transition
        key={`t-${scene.id}`}
        presentation={fade()}
        timing={linearTiming({ durationInFrames: XFADE })}
      />,
    ])}
    <TransitionSeries.Sequence durationInFrames={END_FRAMES}>
      <EndCard o={orientation} lang={lang} />
    </TransitionSeries.Sequence>
  </TransitionSeries>
);
