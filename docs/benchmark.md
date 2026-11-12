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
