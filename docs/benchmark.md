# Benchmark

## Ground truth protocol

There is no "clean" version of a real Veo clip, so ground truth is built from one (`research/synth.py`):

1. take a real Veo 3.1 clip and flip it (h+v): its clean top-left corner now sits where the mark goes;
2. composite the mark exactly as the generator does — `J = (1 − a)·I + a·255` in RGB, real template, real position;
3. encode with H.264 at the source clip's bitrate (`libopenh264`), the same way the clean clip is encoded for the
   **floor**: the same clean frames through the same encoder. No remover can beat the floor.

Scores are inside the mark box (+2 px), against the pre-encoding clean frames (`research/score_gt.py`):

- **PSNR** (dB, higher is better) and **excess** = floor PSNR − method PSNR (0 = indistinguishable from a clip that
  never had a mark);
- **ghost** = worst time-averaged error of any pixel/channel (what you see as a faint static shape on playback).

Caveat: the synthetic mark is our own extracted template, which favours cleanplate (GWT ships its own template).
Real-clip comparisons (below and `out/real/sheet.png`) do not have this bias.

## Results — cleanplate (final defaults) vs GWT Video 0.6.5-demo

Veo "Veo" text mark (21×9 px at 720p):

| Clip | Motion | GWT PSNR / ghost | cleanplate PSNR / ghost | Floor PSNR / ghost |
|---|---|---|---|---|
| Flower market (dolly, textured) | 1.7 px/f | 28.48 / 9.99 | **31.93 / 3.16** | 32.52 / 3.06 |
| Night street (dark) | 0.8 px/f | 33.50 / 7.51 | **39.92 / 2.54** | 40.50 / 1.89 |
| Snow (near-white) | 0.4 px/f | 35.79 / 4.84 | **41.53 / 2.12** | 45.53 / 1.11 |
| Teacup (tripod, steam) | 0.6 px/f | 33.18 / 8.03 | **36.94 / 5.04** | 38.84 / 3.81 |
| Kitchen portrait 9:16 (static wall) | 0.0 px/f | 31.80 / **11.51** | **36.15** / 16.05 | 41.89 / 5.43 |

Gemini ✦ diamond (48×48 px):

| Clip | GWT PSNR / ghost | cleanplate PSNR / ghost | Floor PSNR / ghost |
|---|---|---|---|
| Flower market | not detected | **33.90 / 5.49** | 34.19 / 3.07 |
| Night street | not detected | **36.11 / 6.66** | 36.62 / 6.28 |
| Kitchen portrait 9:16 | 28.40 / 28.04 | **36.65 / 25.42** | 40.29 / 8.47 |

Reading it: on moving shots cleanplate is within **0.3–0.6 dB of the floor** — the mark area is as faithful as
the rest of the frame. Static shots are bounded by information: the background under the mark is never seen
clean, so the remaining gap is what the codec destroyed; cleanplate's MAP estimate still beats GWT by 1.4–8 dB.

## Ablations that shaped the method (all on the same ground truth)

| Change | Effect |
|---|---|
| Invert in native YUV instead of RGB | +0.3 to +1.2 dB on all clips |
| Chroma alpha with H.264 "left" 4:2:0 siting | +0.7 dB (kitchen); 13–22 % less mark-shaped chroma leftover on real Veo clips |
| Flow-guided temporal gathering (vs single-frame inversion + fusion) | flower 29.8 → 32.0 dB, ghost 9.6 → 2.9; night 37.2 → 39.9 dB |
| Weighted-TV MAP on static/slow shots | kitchen 32.4 → 36.2 dB; snow 37.8 → 41.4 dB; hurts textured moving shots → disabled there automatically |
| Static "deghost" by shape-basis subtraction | **removed**: painted a grey diamond on a black static shot (mistakes a real background/fill difference for a ghost) |
| Gross-outlier veto against the frame's own inversion, per plane | teacup 34.5 → 36.9 dB; removes wrong-place colour dots at 20 px/frame; a tight veto (tol 6–15) pulled results onto the codec error (ghost 2.9 → 15.9) — tolerance 25 + 25·a/(1−a) |
| TV on the mark's edge ring where no clean evidence | removes the hairline outline in fast motion; −0.25 dB on the slow Veo flower GT, neutral/better elsewhere |
| Per-pixel TV wherever no clean evidence (whole mark) | **rejected**: −0.8 to −1.3 dB on textured GT clips |
| Far, sparse neighbours for slow motion | **rejected**: snow 41.4 → 36.7 dB (long-gap flow is unreliable) |
| Per-clip gain by de-meaned regression | **rejected**: biased (errors-in-variables), 0.65 for truth 1.0 |
| Per-clip gain / sub-pixel shift / blur by signed-correlation zero-crossing | unbiased (0.97–0.99 for truth 1.0); showed the extracted Veo template already matches real clips — kept as a diagnostic |

## Real clips (no ground truth; no-reference metrics, final AV1 outputs)

`residual` = mark-shaped static leftover in the time-averaged frame vs its surroundings (1 = indistinguishable);
`tint` = mean colour offset of the mark box vs its ring (0 = none). Visual sheet: `out/real/sheet_final.png`.

| Clip (mark) | GWT residual / tint | cleanplate residual / tint | Note |
|---|---|---|---|
| Flower market (Veo) | 4.27 / 2.59 | **1.43 / 0.67** | |
| Night street (Veo) | 1.37 / 0.98 | **0.95 / 0.29** | |
| Snow (Veo) | skipped (no detection) | **0.46 / 0.17** | GWT leaves the clip untouched |
| Teacup (Veo, faint on bright) | 1.11 / 14.72 | 1.11 / **3.70** | GWT reports OK but the mark stays; tint shows it |
| Kitchen 9:16 (Veo) | 0.87 / 8.34 | **0.75 / 4.23** | |
| Gemini sample, 5 shots (✦ 48) | 0.85 / **6.18** (wmr 0.85 / 7.36) | **0.84** / 7.07 | wmr leaves a diamond outline, GWT a faint square on black; cleanplate frames are clean incl. 20 px/frame motion |
| Legacy "Veo" text sample (68×30) | 10.07 / 32.0 (GWT fails; 240 of 432 frames) | **0.66 / 4.87** (wmr 1.22 / 5.55) | faint glyph texture remains on one static shot |

## Detection safety

Clean clips (the five floors) are refused: best match 0.34–0.44 against a bar of 0.75 at unknown placements
(0.45 at a generator's known placement). All eight marked real clips are detected (0.55–0.98, all at known
placements). An earlier, looser detector (six free scales) accepted two clean clips at ncc 0.50–0.52 — the reason
for scale restriction and placement priors.

## Round 2: film-style scenes, a Gemini-app clip, Flow 1080p (2026-11-13)

Four new Veo 3.1 Fast clips (static dinner dialogue, very slow drone push-in, static sidewalk with passers-by, rain
on a window with heavy grain), one Gemini-app video (✦ 48 on a static wooden cabinet) and Flow's 1080p downloads of
three of them. All detected at known placements (ncc 0.72–0.98). Sheets: `out/round2/sheet_*.png`, zoomed videos:
`out/compare2/`.

| Clip | GWT | cleanplate |
|---|---|---|
| Gemini app, ✦ on a static cabinet (720p) | clean frames; faint diamond outline in the time-averaged ghost map | clean; no diamond shape in the ghost map |
| Dinner / sidewalk / hills / rain (720p) | clean | clean; sidewalk crack line stays continuous |
| Dinner, mark on black letterbox (1080p) | **"Veo" clearly visible** | clean |
| Sidewalk, bright static concrete (1080p) | **"Veo" clearly visible** | mark gone; one or two faint fill smudges |
| Hills, moving grass (1080p) | faint "e" remains | clean (83 % of the mark rebuilt from clean pixels) |

**Flow 1080p is not the 720p mark scaled up.** It is an AI upscale of the *marked* 720p video: on dark backgrounds
the glyph becomes near-opaque white (alpha ≈ 1, 1–3 px strokes), on bright ones it gains dark ringing. Alpha implied
by a known background matches the extracted 1080p template on a dark clip (corr 0.83) but not on a bright one (0.24),
so no fixed alpha can be inverted. cleanplate detects the 1080p mark (`veo-small-1080p.png`, extracted from the black
letterbox of the dinner clip; also accepts the 1.5× 720p glyph at the same placement) and switches to **opaque mode**:
the stroke support (+1 px) is treated as unknown — each frame's own estimate is a harmonic fill, clean pixels from
other frames win wherever the scene moved, TV-MAP fills the rest. A first version that kept the a = 0.95 inversion as
a weak candidate left white specks and a dark glyph-shaped fill (values ×20 the codec error) — replaced.

Advice for film work: if you can choose, **download 720p and clean that**; the 720p mark is a true alpha overlay and
comes out at the ground-truth floor on moving shots. Upscale after cleaning if you need 1080p.

## Still images (Gemini ✦) — `research/image.py`

Ground truth: a real 2528×1696 Gemini image, flipped so a mark-free quadrant (brick wall, white kitchen, neon
portrait) sits under the mark; the real "V1" diamond (96 px, margin 64, alpha 0.50) composited, then delivered as the
PNG Gemini gives you, a JPEG q90 re-save, or a 60 % resized JPEG q85 (a shared copy). PSNR inside the mark box (dB);
the PNG ceiling is the 8-bit inversion limit (~60). `research/img_bench.py`.

| Case | GWT | wmr | cleanplate | ceiling |
|---|---|---|---|---|
| Brick, PNG | 43.4 | 56.4 | **56.4** | 60 |
| Neon, PNG | 40.2 | 56.4 | **56.5** | 60 |
| Brick, JPEG q90 | 32.7 | 33.0 | **33.0** | 35.4 |
| Neon, JPEG q90 | 36.0 | 37.3 | **40.8** | 42.3 |
| Brick, resized 60 % + JPEG | not detected | not detected | **30.4** | 32.3 |
| Neon, resized 60 % + JPEG | not detected | 15.5 (wrong mark) | **36.1** | 43.9 (exact-alpha inversion: 36.6) |
| White kitchen (mark adds ≤ 7 levels on 253) | not detected | not detected | not detected (refused) | — |

How: the template gives only the SHAPE; opacity is fitted on the image (zero-crossing of the signed correlation
between the restored image's gradient and the mark's gradient — 0.498–0.504 measured vs 0.498 true), snapped to the
shape's native level when within reach; size is searched continuously only for resized copies; JPEG inputs get a
weighted-TV step whose strength follows the texture around the mark (brick wants ~1, a smooth neon scene ~6).
Detection is scale-free (generator margins are 2/3× or 2× the mark size). The 9 clean versions are all refused.
Gemini has shipped the same diamond at alpha 0.30 ("V2") and 0.50 ("V1", this image); a fixed-opacity template
under-removes one of them — the reason opacity is measured, not assumed.

### Real Flow (Nano Banana 2.1) stills

Four Flow images (1024², 1376×768, 1200×896, 768×1376) carry a fainter diamond: 48 px at a margin of 73–78 px
(≈1.6× the size, a placement neither GWT nor wmr knows) and opacity 0.28–0.30 (measured by cleanplate).
cleanplate: 4/4 removed, no visible residue in the 4× high-pass view. GWT: 2/4 skipped; on the fruit image it reported
OK but edited the wrong spot (the diamond stays, a dark arc is added); on the cat image a diamond outline remains.
wmr: 0/4 detected. Sheet: `out/img_flow/cmp.png`.

### The same Flow stills after a Zalo round trip

Sent as photos to Zalo "My Documents" (HD mode: size kept, re-encoded JPEG ≈ q85) and downloaded back.
cleanplate: 4/4 removed, measured opacity 0.28–0.30 (same as on the originals). GWT: same failures as on the originals
(2 skipped, wrong spot on the fruit, outline on the cat). wmr: 0/4. Sheet: `out/img_zalo/cmp.png`.
