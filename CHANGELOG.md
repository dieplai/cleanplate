# Changelog

## 2.0.0 - 2026-11-14

### Many files at once
- CLI: `cleanplate IN... -o DIR` cleans any number of files and folders (sub-folders kept) into `<name>_clean.<ext>`,
  two at a time on 8+ cores within the available memory (17 clips: 245 s -> 198 s). A failing or crashing file
  never stops the others. Outputs are written under a hidden partial name and renamed when complete, existing
  results are skipped so an interrupted run resumes, and `cleanplate-report.json` records every file.
- Browser: a batch workspace. Drop files or whole folders, follow each file's progress, compare any result, then
  download it, download everything as a ZIP, or save straight into a folder as results finish (Chrome, Edge). The
  same scheduling rule as the CLI, one worker per file, and no decoded pixels kept between files.

### Changed
- The page now lives at https://cleanplate.dieplai.io.vn. The old GitHub Pages address redirects there.
- One-file mode no longer replaces an existing output unless `--overwrite` is given.


## 1.0.0 - 2026-11-14

First stable release. The method and its quality are unchanged from 0.1.0 (ground-truth gate: 36.65 dB mean vs the
Python reference 36.64).

### Browser
- Uses every CPU core: a multi-threaded WebAssembly build (wasm32-wasip1-threads + rayon) with a single-thread
  fallback and a watchdog. Chrome, 12 threads: a 1696x2528 still 9 s -> 2.6 s, an 8 s 720p clip 21 s -> 8 s, a static
  1080p shot ~150 s -> 14 s. GitHub Pages is cross-origin isolated by coi-serviceworker.
- Stays within each device's memory: jobs are estimated before any work (stills ~62 B/px, video ~0.35 MB/frame plus
  input and output) against a budget of 40% of the device's RAM (1-3 GB). Larger jobs go to the server or are
  refused with a clear message instead of crashing the tab.
- Fixed: 23.976 fps (any non-integer rate) videos failed when writing the MP4.
- New look: Plus Jakarta Sans (self-hosted), soft gradient, before/after tags moved off the mark.
- Feedback from the page: report a problem, a mark that was not removed, or an idea. Users see and can leave out
  the technical details. Files and file names are never sent.

### Server (optional, `deploy/`)
- Self-hosting kit: a small service that cleans files with the native CLI, nginx site with cross-origin isolation,
  fair-use limits (6 files/min per address, 2 at a time) and a password-protected feedback dashboard at `/admin/`.

### Core and CLI
- `core::session`: the two-pass video logic shared by the CLI and the browser (it was duplicated before).
- Errors end with a link to the issue tracker. Messages follow one punctuation style.
- `LICENSE-THIRD-PARTY.md`: every third-party work and its license, shipped with the page and the binaries.


## 0.1.0 - 2026-11-13

First public release: prebuilt binaries for Windows (x64), macOS (universal) and Linux (x86_64), and a browser page.

### Browser
- `web/` + `crates/cleanplate-wasm`: the Rust core as WebAssembly. Images bit-identical to the CLI, video through
  WebCodecs (H.264 out, AAC audio copied). Published at https://dieplai.github.io/cleanplate/.

### CLI robustness
- Clear refusals instead of panics for tiny stills (< 64 px) and videos (< 240 px, a 64×64 pattern used to match an
  8 px ✦) and for videos with no decodable frame. The refusal says why (confidence, opacity or size).
- `--codec` is validated. `auto` picks from the output extension and the encoders this ffmpeg offers (libx264 →
  SVT-AV1 → OpenH264), and fails fast when none fits. Missing ffmpeg gives an install hint for the OS.
- Rotated (phone) videos are read at their upright size. Image format detected from content. 16-bit inputs noted.
  Video-only flags on stills are reported. Usage errors exit 1 (2 stays "no mark, nothing written").
- Windows binary links the C runtime statically. CI runs the full suite on Linux, Windows and macOS with ffmpeg.

## Research preview

### Video
- Flow-guided temporal reconstruction in native YUV with H.264 chroma siting. Gross-outlier veto per plane.
  Weighted-TV MAP with per-shot λ and an edge ring for fast motion. Shot detection. Safe detection with known placements.
- Flow 1080p (AI-upscaled) support: native 1080p template, opaque mode, `--guide` with the clip's 720p.
- Outputs: ProRes 422 HQ, AV1 10-bit, H.264, FFV1. Audio and colour tags copied.

### Stills
- `research/image.py`: scale-free detection (Gemini, Gemini-video and Flow placements), opacity / offset / fractional
  size fitted on the image, preference for opacity-consistent and exact-size templates, texture-adaptive TV for JPEG.
- Gemini ✦ "V1" (alpha 0.50) templates added alongside the 0.30 rendering.

### Evaluation
- Ground-truth video bench (`research/synth.py`, `score_gt.py`), resolution bench, LPIPS scorer.
- Ground-truth still bench (`research/img_bench.py`): PNG, JPEG, resized copies.
- Real-file checks: Veo 720p/1080p, Gemini-app video, Gemini and Flow stills, a Zalo round trip.

### Rust core + CLI
- `crates/cleanplate-core` (no IO, wasm-ready) and the `cleanplate` binary: video (ffmpeg pipes) and stills (PNG/JPEG/WebP).
- Ports of the OpenCV primitives used (golden-tested), DIS optical flow matched to OpenCV, FFT template matching.
- Ground-truth gate vs the Python reference: mean 36.65 vs 36.64 dB, ~16× faster (3-18 s per 8 s clip), stills 2× faster.
- Stills: refuse a fitted opacity < 0.15 (also in `research/image.py`).
