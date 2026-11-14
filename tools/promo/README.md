# `wear-pointer` Showcase Video (Remotion)

Builds the 16:9 (`1920×1080`) and 9:16 (`1080×1920`) showcase videos in Traditional Chinese (`zh`) and English (`en`) from synchronized real captures of the Wear OS app (`public/watch_sync.mp4`) and the Hyprland 0.57 Wayland desktop (`public/desk_sync.mp4`).

`capture/stage.py` (native Wayland GTK3/Cairo `1920×1080` interactive stage + live `trace.log` packet decoder) and `capture/choreograph.py` (60 Hz rigid-body wrist IMU choreographer over the emulator console) drive the dual-hardware recording on top of `tools/harness/`.

## Preview & Render

```bash
cd tools/promo
npm ci
npx remotion studio

npx remotion render src/index.ts Horizontal-zh ../../docs/assets/demo-16x9-zh.mp4 --codec=h264 --crf=20
npx remotion render src/index.ts Horizontal-en ../../docs/assets/demo-16x9-en.mp4 --codec=h264 --crf=20
npx remotion render src/index.ts Vertical-zh ../../docs/assets/demo-9x16-zh.mp4 --codec=h264 --crf=20
npx remotion render src/index.ts Vertical-en ../../docs/assets/demo-9x16-en.mp4 --codec=h264 --crf=20
```
