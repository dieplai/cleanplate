# Changelog

## Unreleased — research preview

### Video
- Flow-guided temporal reconstruction in native YUV with H.264 chroma siting; gross-outlier veto per plane;
  weighted-TV MAP with per-shot λ and an edge ring for fast motion; shot detection; safe detection with known placements.
- Flow 1080p (AI-upscaled) support: native 1080p template, opaque mode, `--guide` with the clip's 720p.
- Outputs: ProRes 422 HQ, AV1 10-bit, H.264, FFV1; audio and colour tags copied.

### Stills
- `research/image.py`: scale-free detection (Gemini, Gemini-video and Flow placements), opacity / offset / fractional
  size fitted on the image, preference for opacity-consistent and exact-size templates, texture-adaptive TV for JPEG.
- Gemini ✦ "V1" (alpha 0.50) templates added alongside the 0.30 rendering.

### Evaluation
- Ground-truth video bench (`research/synth.py`, `score_gt.py`), resolution bench, LPIPS scorer.
- Ground-truth still bench (`research/img_bench.py`): PNG, JPEG, resized copies.
- Real-file checks: Veo 720p/1080p, Gemini-app video, Gemini and Flow stills, a Zalo round trip.
