# Store and README marketing images

`generate.py` builds the Google Play listing images and the README hero from real watch screenshots. It writes each layout as HTML/CSS (CSS watch case, CSS laptop with an SVG desktop and cursor trail), renders it in headless Chrome at 2x, and scales it down with Pillow to the exact size as RGB PNG with no alpha.

## Run

From the repository root:

```sh
python3 tools/store-assets/generate.py
```

The full set renders in about 15 s. Needs Python 3.9+, Pillow (`pip install pillow`) and Google Chrome or a Chromium build. No network access is needed. Chrome is found in `/Applications`, in the Playwright cache (`~/Library/Caches/ms-playwright`) or on `PATH`. Set `CHROME_BIN=/path/to/chrome` to use a different one.

Useful flags:

| Flag | Effect |
| --- | --- |
| `--only phone` / `feature` / `hero` / `sheet` | Render one group (repeatable). `sheet` alone rebuilds the contact sheet from existing PNGs. |
| `--locale zh-TW` | Render one locale (repeatable). |
| `--config path.json` | Use another config. |
| `--out dir` | Write somewhere else (default `tools/store-assets/out`). |
| `--keep-html` | Keep the intermediate HTML in `out/_html/` for tweaking in a browser. |

## Output

```
out/
  play/en-US/phone_01_mouse.png … phone_06_open-source.png   1080x1920
  play/en-US/feature_graphic.png                               1024x500
  play/zh-TW/…                                                 same set
  hero.png                                                     1600x900 (README, en)
  contact-sheet.png                                            preview of everything
```

Phone screenshots are numbered in config order, so upload them in filename order.

## Swapping screenshots or text

Everything is in [`config.json`](config.json):

- `screens`: name → PNG path (relative to the config or absolute). Watch screenshots should be square captures of the round display (the Watch7 gives 480x480). The image is masked to a circle and upscaled with Lanczos. To refresh, overwrite the files in `screens/`, or point the names at new captures, and run the command again.
- `phone.slides[]`: one entry per screenshot, in upload order.
  - `layout`: `watch-laptop` (watch in front of a laptop, with a list on the right), `watch-hero` (big watch plus a `stats`, `actions` or `dpi-scale` block) or `open-source`.
  - `watch`: a key from `screens`. `accent`: `emerald` | `sky` | `amber` | `rose` | `#hex`.
  - `desktop` (watch-laptop only): `point` (cursor trail) or `drag` (window dragged from a dashed outline).
  - `decor` (watch-hero only): `motion` (tilt arrows), `bezel` (rotate arrows) or `none`.
  - `text.<locale>`: `eyebrow`, `headline`, `sub`. Wrap words in `*asterisks*` to colour them with the accent. `\n` forces a line break. Leave `sub` empty when the headline needs two lines.
  - `"enabled": false` keeps a slide in the config without rendering it. The `relative-mode` slide ("Air-mouse or absolute — your choice") is parked this way for the coming control-mode setting. Add a screenshot of the mode picker under `screens`, point `watch` at it, and set `enabled` to `true`.
- `feature_graphic` and `hero`: watch screen plus text per locale. Add `"zh-TW"` to `hero.locales` to also get `hero.zh-TW.png`.

To add a locale, add it to `locales` and add a matching key to every localised field. A field with no entry for the locale falls back to `en-US`.

## Notes

- Text uses the macOS system font (SF Pro) with PingFang TC for Chinese and SF Mono for labels. On Linux, Chrome substitutes other fonts, so line breaks can shift. Check the contact sheet after rendering.
- The OS badges use neutral glyphs (⌘, a window, a terminal prompt), not the Apple, Microsoft or Linux logos.
- The feature graphic keeps the text, icon, watch case and laptop inside the central 80%. Only the strap ends reach toward the edges.
