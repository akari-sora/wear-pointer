// Design tokens + copy for the Launch video. Matches tools/store-assets.
export type Lang = "en" | "zh";
export type Orientation = "horizontal" | "vertical";

export const C = {
  bg0: "#09111E",
  bg1: "#0B1220",
  emerald: "#10B981",
  sky: "#38BDF8",
  amber: "#F59E0B",
  rose: "#F43F5E",
  ink: "#F8FAFC",
  muted: "#9AA8BC",
  dim: "#64748B",
};

// System fonts only (no network). Remotion's headless Chrome on macOS resolves
// -apple-system to SF Pro and falls back per glyph to PingFang TC for Chinese.
export const SANS =
  '-apple-system, BlinkMacSystemFont, "SF Pro Display", "Helvetica Neue", "PingFang TC", "Heiti TC", sans-serif';
export const SANS_ZH = '-apple-system, BlinkMacSystemFont, "PingFang TC", "Heiti TC", sans-serif';
export const MONO = 'ui-monospace, "SF Mono", Menlo, Monaco, monospace';

export const VERSION = "v0.2.4";
export const REPO = "gitlab.com/aa22396584/wear-pointer";

export const rgba = (hex: string, a: number) => {
  const h = hex.replace("#", "");
  const r = parseInt(h.slice(0, 2), 16);
  const g = parseInt(h.slice(2, 4), 16);
  const b = parseInt(h.slice(4, 6), 16);
  return `rgba(${r},${g},${b},${a})`;
};

export type ModeKey = "laser" | "air" | "touch";

type Copy = {
  brand: string;
  hook: { eyebrow: string; headline: string; sub: string; chips: string[] };
  point: { eyebrow: string; headline: string; sub: string; stats: { v: string; l: string }[] };
  drag: { eyebrow: string; headline: string; sub: string; tap: [string, string]; hold: [string, string] };
  modes: { eyebrow: string; headline: string; items: { key: ModeKey; label: string; short: string; caption: string }[] };
  sens: { eyebrow: string; headline: string; sub: string; steps: string };
  pause: { eyebrow: string; headline: string; sub: string; paused: string; center: string; pauseCard: [string, string]; centerCard: [string, string] };
  outro: { items: string[]; tagline: string };
};

export const COPY: Record<Lang, Copy> = {
  en: {
    brand: "Wear Pointer",
    hook: {
      eyebrow: "WEAR OS AIR MOUSE",
      headline: "Your watch is now a *mouse*",
      sub: "Point at any computer and move the cursor.",
      chips: ["macOS", "Windows", "Linux"],
    },
    point: {
      eyebrow: "SPATIAL AIM",
      headline: "Point with your *wrist*",
      sub: "100 Hz gyro, about 10 ms over Bluetooth LE.",
      stats: [
        { v: "100 Hz", l: "GYRO" },
        { v: "~10 ms", l: "BLE LATENCY" },
        { v: "1600", l: "DEFAULT DPI" },
      ],
    },
    drag: {
      eyebrow: "GESTURES",
      headline: "Tap to click, *hold to drag*",
      sub: "When the ring fills, the drag locks.",
      tap: ["Tap", "Left click"],
      hold: ["Hold", "Drag & drop"],
    },
    modes: {
      eyebrow: `NEW IN ${VERSION}`,
      headline: "Three ways to *point*",
      items: [
        { key: "laser", label: "LASER", short: "Absolute pointing", caption: "Absolute. The cursor goes where the watch aims." },
        { key: "air", label: "AIR", short: "Relative + accel", caption: "Air mouse with acceleration. No recentering." },
        { key: "touch", label: "TOUCH", short: "Finger touchpad", caption: "Slide a finger on the watch like a touchpad." },
      ],
    },
    sens: {
      eyebrow: "LIVE SENSITIVITY",
      headline: "Tune sensitivity *on the bezel*",
      sub: "400–3200 DPI, synced to the computer instantly.",
      steps: "200 DPI STEPS",
    },
    pause: {
      eyebrow: "CLUTCH",
      headline: "Pause is a clutch. *Recenter* in one tap.",
      sub: "Move your arm freely. The cursor stays put.",
      paused: "PAUSED",
      center: "CENTER",
      pauseCard: ["Pause", "Cursor holds still"],
      centerCard: ["Center", "Snap to mid-screen"],
    },
    outro: {
      items: ["Open source", "No accounts", "No tracking"],
      tagline: "Wear OS air mouse for macOS, Windows and Linux",
    },
  },
  zh: {
    brand: "Wear Pointer",
    hook: {
      eyebrow: "WEAR OS 空中滑鼠",
      headline: "手錶秒變*空中滑鼠*",
      sub: "對準電腦，手腕一轉游標就動。",
      chips: ["macOS", "Windows", "Linux"],
    },
    point: {
      eyebrow: "空間指向",
      headline: "轉動*手腕*就能指",
      sub: "100 Hz 陀螺儀，藍牙延遲約 10 ms。",
      stats: [
        { v: "100 Hz", l: "陀螺儀" },
        { v: "~10 ms", l: "藍牙延遲" },
        { v: "1600", l: "預設 DPI" },
      ],
    },
    drag: {
      eyebrow: "手勢操作",
      headline: "點一下按，*長按拖曳*",
      sub: "外圈填滿，就鎖定拖曳。",
      tap: ["輕點", "左鍵點擊"],
      hold: ["長按", "拖曳放開"],
    },
    modes: {
      eyebrow: `${VERSION} 新功能`,
      headline: "三種*指向方式*",
      items: [
        { key: "laser", label: "雷射指向", short: "絕對指向", caption: "絕對指向：手錶指哪，游標就到哪。" },
        { key: "air", label: "空中滑鼠", short: "相對＋加速度", caption: "帶加速度的空中滑鼠，不必回中。" },
        { key: "touch", label: "觸控板", short: "手指滑動", caption: "手指在錶面滑動，就像觸控板。" },
      ],
    },
    sens: {
      eyebrow: "即時靈敏度",
      headline: "*錶圈*直接調靈敏度",
      sub: "400–3200 DPI，電腦端即時同步。",
      steps: "每格 200 DPI",
    },
    pause: {
      eyebrow: "離合控制",
      headline: "暫停當離合，\n*一鍵回中*",
      sub: "手臂隨意移動，游標停在原地。",
      paused: "已暫停",
      center: "回中",
      pauseCard: ["暫停", "游標停住不動"],
      centerCard: ["回中", "瞬間回到中央"],
    },
    outro: {
      items: ["開源", "免帳號", "零追蹤"],
      tagline: "macOS、Windows、Linux 都能用的 Wear OS 空中滑鼠",
    },
  },
};
