// Every watch screen in the Launch video comes from this map.
//
// Drop fresh 480x480 captures (PNG) or screen recordings (MP4/WebM, square,
// round UI) into tools/store-assets/screens/ using the names below, then run
// `npm run launch:assets` (the render script does this for you). For each key
// the first candidate that exists wins, so a new capture replaces the
// placeholder without code changes. Recordings are played with
// <OffthreadVideo>, muted, from their first frame.
import { staticFile } from "remotion";
import { AVAILABLE } from "./available.generated";

export const SCREENS = {
  /** Main pointer UI, idle. */
  main: ["main.mp4", "main.png"],
  /** Main UI in each pointing mode (mode selector visible). */
  mainLaser: ["main_laser.mp4", "main_laser.png", "main.png"],
  mainAir: ["main_air.mp4", "main_air.png", "main.png"],
  mainTouch: ["main_touch.mp4", "main_touch.png", "main.png"],
  /** Mode picker sheet. No placeholder: the beat skips it until it exists. */
  sheetMode: ["sheet_mode.mp4", "sheet_mode.png"],
  /** Sensitivity / telemetry sheet. */
  sensitivity: ["sheet_sensitivity.mp4", "sheet_sensitivity.png", "sensitivity.png"],
  /** Hold ring filled, drag locked (emerald). */
  drag: ["drag.mp4", "drag.png"],
  /** Paused (amber). */
  paused: ["paused.mp4", "paused.png"],
  stopped: ["stopped.png"],
} as const;

export const ICON = ["icon.png"];

export type ScreenKey = keyof typeof SCREENS;
export type Media = { src: string; kind: "image" | "video"; file: string };

const VIDEO = /\.(mp4|webm|mov)$/i;

export const pick = (candidates: readonly string[]): Media | null => {
  const file = candidates.find((c) => AVAILABLE.includes(c));
  if (!file) return null;
  return { src: staticFile(`launch/${file}`), kind: VIDEO.test(file) ? "video" : "image", file };
};

export const screen = (key: ScreenKey): Media | null => pick(SCREENS[key]);

/** Like screen(), but never null: falls back to the main UI. */
export const screenOrMain = (key: ScreenKey): Media =>
  screen(key) ?? screen("main") ?? { src: staticFile("launch/main.png"), kind: "image", file: "main.png" };

export const iconSrc = (): string => pick(ICON)?.src ?? staticFile("icon.png");
