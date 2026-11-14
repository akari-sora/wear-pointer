import React from "react";
import { Composition } from "remotion";
import { FPS, Promo, totalFrames } from "./Promo";

export const RemotionRoot: React.FC = () => (
  <>
    <Composition
      id="Horizontal-zh"
      component={Promo}
      durationInFrames={totalFrames()}
      fps={FPS}
      width={1920}
      height={1080}
      defaultProps={{ orientation: "horizontal" as const, lang: "zh" as const }}
    />
    <Composition
      id="Vertical-zh"
      component={Promo}
      durationInFrames={totalFrames()}
      fps={FPS}
      width={1080}
      height={1920}
      defaultProps={{ orientation: "vertical" as const, lang: "zh" as const }}
    />
    <Composition
      id="Horizontal-en"
      component={Promo}
      durationInFrames={totalFrames()}
      fps={FPS}
      width={1920}
      height={1080}
      defaultProps={{ orientation: "horizontal" as const, lang: "en" as const }}
    />
    <Composition
      id="Vertical-en"
      component={Promo}
      durationInFrames={totalFrames()}
      fps={FPS}
      width={1080}
      height={1920}
      defaultProps={{ orientation: "vertical" as const, lang: "en" as const }}
    />
  </>
);
