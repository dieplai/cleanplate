# How cleanplate removes a visible mark

*Research reference: `research/pipeline.py` is the executable specification of this document.*

## 1. The problem, precisely

A generator composites a static, semi-transparent white mark onto each rendered frame and then encodes the video:

```
J_t = encode( (1 - a) · I_t + a · L )
```

`I_t` is the frame we want back, `a(p)` the mark's alpha (≤ 0.55 for every Google mark measured so far), `L` the mark
colour (white: luma 235 in TV range, chroma 128). Two things make naive removal leave a trace:

1. **Noise amplification.** Inverting, `I = (J − aL)/(1 − a)`, divides the codec's error by `(1 − a)`: up to ×2.2 on
   the mark's strokes. Every frame gets a slightly different error, so a single-frame inversion shimmers.
2. **Codec ghost.** The encoder spends its bits on the mark's sharp strokes; its error is glyph-shaped. On a static
   background it repeats identically every frame, so a time average keeps it.

Hiding both with a spatial blur (what most tools do, often with a learned denoiser) leaves a soft patch — the
"still a bit blurry there" look.

## 2. The idea: a video is not one image

The mark is fixed; the scene moves. A point of the scene that sits under the mark in frame *t* is, a few frames
earlier or later, **out from under it** — fully visible, uncorrupted. cleanplate rebuilds each mark pixel from the
same scene point in other frames, found by optical flow. Nothing is invented and nothing is blurred: the pixels are
the video's own.

## 3. Pipeline

| Step | What | Why |
|---|---|---|
| Detect | Multi-template, multi-scale NCC on the time-averaged high-pass of the corner; refuse below a confidence floor | Never damage a clip; works on bright/white backgrounds where other tools skip |
| Shots | Hard-cut detection on per-frame thumbnails | Never borrow pixels across a cut |
| Invert (native YUV) | `R_t = (J_t − aL)/(1 − a)` per plane; chroma uses the 2×2-averaged alpha and `L = 128` | Avoids a lossy RGB round trip; the 4:2:0 chroma sees a blurred mark |
| Gather | For each neighbour `s` within the shot: DIS flow `t→s` (re-filled inside the mark from its surroundings), forward/backward consistency check, warp `R_s` | Collect every observation of the same scene point |
| Weigh | Clean candidates (landing outside the mark) ≈ weight 1; inversions `(1 − a_src)²` (inverse noise variance); × flow confidence × temporal decay | Inverse-variance fusion: the cleanest evidence wins per pixel |
| Veto | Each candidate must agree with this frame's own inversion within a gross-outlier tolerance (25 + 25·a/(1−a) luma, 20 + 20·a/(1−a) chroma; quartic fall-off); luma and each chroma plane are vetoed and robust-reweighted **separately** | A failed flow can import wrong-place content consistently in both directions (yellow dots from a yellow stripe at 20 px/frame); the tolerance sits above the codec's own error (~20–40) and below wrong content (~80) |
| Refine | Weighted-TV MAP: `min Σ w (I − μ)² + λ TV(I)`, `w` = clean evidence (adds) + one inversion-equivalent `1/(1 + 9·(a/(1−a))²)`; λ per shot = 16 on static/slow shots → 0 on moving ones; plus λ = 16 on the mark's thin edge ring wherever no clean observation landed | Static shots: the edge-sparsity prior removes the glyph-shaped codec ghost. Moving shots: real detail is kept. Fast motion: removes the hairline outline the codec leaves on the mark's edge |
| Paste & encode | Feathered paste into the untouched frame; audio and colour tags copied; ProRes / AV1 / H.264 / FFV1 | Everything outside the mark is the source |

## 4. Evidence

Measured on ground-truth clips (a real Veo clip, flipped so a clean corner sits under the mark, re-marked with the
real template and re-encoded at the source bitrate; "floor" = the same clean clip through the same encoder — no
remover can beat it). See `docs/benchmark.md` for the full tables and `research/` to reproduce.

## 5. Known limits

- A static scene point hidden under the mark for the whole shot is never observed clean; its detail is whatever
  the codec kept. cleanplate returns the most likely image (TV-MAP), not the lost detail.
- Non-rigid motion that flow cannot follow (steam, water) gets low flow confidence and falls back to inversions.
- Only visible marks. SynthID and C2PA provenance are not touched, by design.

## References

- T. Dekel, M. Rubinstein, C. Liu, W. T. Freeman. *On the Effectiveness of Visible Watermarks.* CVPR 2017 — multi-image
  matting of a consistent watermark.
- Z. Li et al. *Towards an End-to-End Framework for Flow-Guided Video Inpainting* (E2FGVI), CVPR 2022, and
  S. Zhou et al. *ProPainter*, ICCV 2023 — flow-guided pixel propagation (here without any learned hallucination).
- T. Kroeger et al. *Fast Optical Flow using Dense Inverse Search* (DIS), ECCV 2016.
- L. Rudin, S. Osher, E. Fatemi. *Nonlinear total variation based noise removal algorithms*, Physica D 1992.
