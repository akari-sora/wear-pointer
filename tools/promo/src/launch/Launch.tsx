import React from "react";
import { AbsoluteFill } from "remotion";
import { TransitionSeries, linearTiming } from "@remotion/transitions";
import { fade } from "@remotion/transitions/fade";
import { GLOBAL_CSS } from "./components";
import { Drag, Hook, Modes, Outro, Pause, Point, SceneProps, Sensitivity } from "./scenes";

export const LAUNCH_FPS = 30;
const XF = 12;

const BEATS: { id: string; frames: number; C: React.FC<SceneProps> }[] = [
  { id: "hook", frames: 120, C: Hook },
  { id: "point", frames: 122, C: Point },
  { id: "drag", frames: 142, C: Drag },
  { id: "modes", frames: 152, C: Modes },
  { id: "sensitivity", frames: 112, C: Sensitivity },
  { id: "pause", frames: 128, C: Pause },
  { id: "outro", frames: 130, C: Outro },
];

export const launchFrames = () => BEATS.reduce((a, b) => a + b.frames, 0) - XF * (BEATS.length - 1);

/** Start frame of each beat in the final timeline (used for the GIF preview cut list). */
export const beatStarts = () => {
  let t = 0;
  return BEATS.map((b) => {
    const s = t;
    t += b.frames - XF;
    return { id: b.id, start: s, frames: b.frames };
  });
};

export const Launch: React.FC<SceneProps> = ({ o, lang }) => (
  <AbsoluteFill style={{ backgroundColor: "#09111E" }}>
    <style>{GLOBAL_CSS}</style>
    <TransitionSeries>
      {BEATS.flatMap((b, i) => {
        const seq = (
          <TransitionSeries.Sequence key={b.id} durationInFrames={b.frames}>
            <b.C o={o} lang={lang} />
          </TransitionSeries.Sequence>
        );
        if (i === 0) return [seq];
        return [<TransitionSeries.Transition key={`${b.id}-t`} presentation={fade()} timing={linearTiming({ durationInFrames: XF })} />, seq];
      })}
    </TransitionSeries>
  </AbsoluteFill>
);
