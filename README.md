<div align="center">

# cleanplate

### Remove the visible AI mark, keep the picture. Nothing invented, nothing blurred.

[![CI](https://github.com/dieplai/cleanplate/actions/workflows/ci.yml/badge.svg)](https://github.com/dieplai/cleanplate/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/dieplai/cleanplate?color=0f766e)](https://github.com/dieplai/cleanplate/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/dieplai/cleanplate/total?color=0f766e)](https://github.com/dieplai/cleanplate/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)
[![Platforms](https://img.shields.io/badge/Windows%20%C2%B7%20macOS%20%C2%B7%20Linux-supported-555.svg)](#install)
[![Try it in the browser](https://img.shields.io/badge/try%20it-in%20your%20browser-14b8a6.svg)](https://dieplai.github.io/cleanplate/)

**English** · [Tiếng Việt](./README.vi.md)

</div>

<p align="center"><img src="docs/assets/hero_veo_legacy.gif" width="720" alt="Veo text mark: original, GWT, wmr, cleanplate"></p>

cleanplate removes the **visible** marks that AI generators stamp on their output (Google Veo's "Veo" text and
Gemini / Nano Banana's ✦ diamond) from **videos and still images**, and gives back footage that looks like it never
had one. It is made for creators who use these tools for non-commercial work and need a clean plate for a scene, an
edit or a thumbnail.

Other removers invert the overlay frame by frame, then hide what is left with a blur or an AI inpainter, which leaves
a soft patch. cleanplate uses what a video already has: **the scene moves under the mark, so almost every hidden pixel
is visible, clean, in another frame.** It rebuilds the mark area from those real pixels. On stills it measures the
mark's exact opacity and position on your image instead of assuming them.

| Ground truth, PSNR inside the mark (dB, higher is better) | GWT 0.6.5 | wmr 1.17 | **cleanplate** | ceiling |
|---|---|---|---|---|
| Video, Veo 720p, 5 clips (mean) | 32.6 | - | **37.3** | 39.9 |
| Still, Gemini ✦ PNG (brick / neon) | 43.4 / 40.2 | 56.4 / 56.4 | **56.4 / 56.5** | 60 |
| Still, re-saved JPEG (brick / neon) | 32.7 / 36.0 | 33.0 / 37.3 | **33.0 / 40.8** | 35.4 / 42.3 |
| Still, resized + JPEG, like a shared copy | not detected | not detected / wrong | **30.4 / 34.1** | 32.3 / 43.9 |

<sub>Ceiling = the clean footage through the same codec. No remover can beat it. Every clip and how each number was
measured: <a href="./docs/benchmark.md">docs/benchmark.md</a>.</sub>

> [!IMPORTANT]
> **Visible marks only.** cleanplate does not touch SynthID (the invisible watermark) or C2PA provenance. Label
> AI-generated content as your platform and your audience expect.

## Contents

[Results](#results) · [Install](#install) · [Usage](#usage) · [Best quality](#best-quality) · [Method](#method) ·
[Performance](#performance) · [Limits](#known-limits) · [Support](#-support) · [License](#license) ·
[Credits](#credits) · [References](#references)

## Results

![Results overview](./docs/assets/overview.png)

<table>
<tr>
<td width="50%"><img src="docs/assets/hero_veo.gif" alt="Veo mark on a moving flower market"><br><sub>Veo 3.1, moving shot: original · GWT · cleanplate</sub></td>
<td width="50%"><img src="docs/assets/hero_gemini.gif" alt="Gemini diamond on a fast cartoon"><br><sub>Gemini ✦ on fast motion (20 px/frame): original · cleanplate</sub></td>
</tr>
</table>

<p align="center"><img src="docs/assets/hero_still.jpg" width="860" alt="Flow stills: GWT edits the wrong spot, wmr does not detect, cleanplate removes"><br>
<sub>Real Flow (Nano Banana) stills with a faint 48 px ✦. GWT paints a dark diamond at the wrong spot, wmr does not detect it.</sub></p>

## Install

### Option 1 - in the browser (nothing to install)

Open **[dieplai.github.io/cleanplate](https://dieplai.github.io/cleanplate/)** in Chrome or Edge and drop a file.
Everything runs on your device, on all its CPU cores. The file is never uploaded. Images give exactly the same
result as the CLI. Video is re-encoded as H.264.

### Option 2 - download the command-line tool

| System | Download | Then |
|---|---|---|
| **Windows** 10/11 (x64) | [cleanplate-windows-x86_64.zip](https://github.com/dieplai/cleanplate/releases/latest/download/cleanplate-windows-x86_64.zip) | Unzip. Run `cleanplate.exe` from a terminal in that folder (unsigned: if SmartScreen asks, *More info → Run anyway*) |
| **macOS** (Apple Silicon and Intel) | [cleanplate-macos-universal.tar.gz](https://github.com/dieplai/cleanplate/releases/latest/download/cleanplate-macos-universal.tar.gz) | `tar xzf` it. Once: `xattr -d com.apple.quarantine cleanplate` |
| **Linux** x86_64 (glibc 2.35+) | [cleanplate-linux-x86_64.tar.gz](https://github.com/dieplai/cleanplate/releases/latest/download/cleanplate-linux-x86_64.tar.gz) | `tar xzf` it. Optionally move `cleanplate` into `~/.local/bin` |

**Video also needs [ffmpeg](https://ffmpeg.org) on your PATH** (images do not):

| System | Command |
|---|---|
| Windows | `winget install --id Gyan.FFmpeg -e`, then open a **new** terminal |
| macOS | `brew install ffmpeg` |
| Ubuntu / Debian | `sudo apt install ffmpeg` |
| Fedora | `sudo dnf install ffmpeg-free` (no x264: cleanplate writes AV1 instead) |

### Option 3 - build from source

```bash
git clone https://github.com/dieplai/cleanplate.git && cd cleanplate
cargo install --path crates/cleanplate-cli      # Rust ≥ 1.88 (https://rustup.rs); ~1–3 min
```

## Usage

```bash
cleanplate photo.png photo_clean.png            # still: PNG, JPG or WebP in; same or another format out
cleanplate clip.mp4 clip_clean.mp4              # video: H.264 (or AV1), audio and colour tags copied
cleanplate clip.mp4 clip_clean.mov              # ProRes 422 HQ for editing
cleanplate clip.mp4 clip_clean.mkv              # lossless FFV1
cleanplate clip_1080p.mp4 out.mov --guide clip_720p_clean.mov   # AI-upscaled download + its cleaned 720p
```

On Windows type `.\cleanplate.exe` instead of `cleanplate` when you run it from the folder you unzipped.

| Option | Meaning |
|---|---|
| `--codec auto\|h264x\|av1\|h264\|prores\|ffv1` | Video encoder. `auto` picks from the output extension and what your ffmpeg offers |
| `--guide FILE` | For Flow 1080p / 4K downloads: the same clip at 720p, already cleaned |
| `--threads N` | Worker threads (default: all cores) |
| `--lam`, `--k`, `--min-ncc`, `--opaque`, `--no-edge-tv` | Expert tuning. See `cleanplate --help` |

**Exit codes:** `0` cleaned · `2` no mark found with confidence, **nothing written** (the input is never modified) ·
`1` error (the message says what to do, e.g. how to install ffmpeg).

## Best quality

- **Download the original resolution.** In Flow choose 720p "original size". Flow's 1080p and 4K downloads are AI
  upscales of the *marked* video: the mark is re-drawn and partly destroys what is under it. If you only have the
  1080p, clean the 720p too and pass it with `--guide`.
- **Use the file as downloaded.** Every re-encode (messaging apps, social networks, screen recording) blurs the detail
  the method recovers. cleanplate still handles resized and re-compressed copies, at lower fidelity.
- **Save stills as PNG.** It is lossless. JPEG output re-compresses the whole picture.
- **For editing, write `.mov` (ProRes) or `.mkv` (FFV1)** so the next tool starts from a lossless file.

## Method

**Problem.** A generator composites a static, semi-transparent mark onto every frame and then encodes it:
`J_t = C((1 − α)·I_t + α·L)`, with opacity `α ≤ 0.55` for every Google mark measured and `L` white. Inverting a single
frame, `Î_t = (J_t − α·L) / (1 − α)` [1], divides the codec error of `C` by `1 − α` (up to ×2.2 on the strokes):
frames shimmer, and on static backgrounds a glyph-shaped residue remains that a spatial blur or a learned inpainter
can only hide.

**Observation.** The mark is fixed in the frame while the scene moves, so a point hidden at time *t* is usually seen
unobstructed at some other time *s*. Cleanplate estimates every covered pixel from those observations, found with
optical flow, instead of synthesising it: the propagation idea of flow-guided video inpainting [5, 6], without any
learned component.

**Video pipeline.**

1. **Detection.** Zero-mean normalised cross-correlation [7] between the mark templates and the temporal mean of the
   high-passed bottom-right corner, at the generator's known placements and scales. Below a confidence floor the
   input is returned untouched.
2. **Shots.** Hard cuts from per-frame thumbnails. No observation crosses a cut.
3. **Inversion in native YUV** with H.264 chroma siting (chroma uses the 2×2-averaged α and `L = 128`), avoiding an
   RGB round trip.
4. **Temporal gathering.** For every neighbour *s* within ±K frames: Dense Inverse Search optical flow [2] with a
   variational refinement whose data terms are normalised as in [3], re-filled inside the mark from its
   surroundings, validated by a forward-backward consistency check [8]. Each candidate is weighted by its inverse
   noise variance (a clean observation ≈ 1, an inversion `(1 − α)²`), the flow confidence and a temporal decay.
5. **Robust fusion.** Candidates that disagree with the frame's own inversion beyond the codec's error are vetoed,
   separately for luma and each chroma plane, then robustly reweighted.
6. **MAP refinement.** Where no clean observation exists, a weighted total-variation estimate [4],
   `min_I Σ w·(I − μ)² + λ·TV(I)`, with λ set per shot from the measured motion (16 on static shots, 0 on moving
   ones) and on the mark's edge ring.
7. **Composition.** The rebuilt region is feathered into the untouched frame. Audio and colour metadata are copied.

**Stills.** The ✦ is detected at any size by the same correlation. Its opacity, sub-pixel offset and fractional size
are then *fitted on the image* (the zero crossing of the signed gradient correlation across the mark's edge), because
Google has shipped the same shape at α = 0.30 and 0.50. The overlay is inverted exactly [1]. JPEG and WebP inputs
get a texture-adaptive weighted-TV step [4] on the mark only.

**Safety.** No confident match, a fitted opacity below 0.15, or an input too small to hold a mark: nothing is
written (14 / 14 clean inputs refused).

The executable specification is [`research/`](./research). Evidence for every step and the ideas that were tried and
rejected are in [`docs/method.md`](./docs/method.md) and [`docs/benchmark.md`](./docs/benchmark.md).

## Performance

On a 12-core laptop: an 8 s 720p clip in **3-9 s** (a 9:16 static shot 18 s), a 2528×1696 still in **2 s**: **about
16× faster than the Python reference at the same quality** (ground-truth gate over 8 clips: 36.65 vs 36.64 dB mean,
stills bit-identical, [`bench/rust_vs_python.sh`](./bench/rust_vs_python.sh)). The browser version runs the same
core as WebAssembly on all cores (a 12-thread laptop in Chrome: 2.6 s for that still, 8 s for that clip, 14 s for a
static 1080p shot). Where the browser cannot share memory between threads it falls back to one thread.

The Rust code is split into [`cleanplate-core`](./crates/cleanplate-core) (every algorithm, no file or OS access) and
[`cleanplate-cli`](./crates/cleanplate-cli) (files and ffmpeg). [`cleanplate-wasm`](./crates/cleanplate-wasm) +
[`web/`](./web) is the browser page. The Python in [`research/`](./research) is the reference specification and the
benchmark harness.

## Known limits

- **Static shots are information-limited.** If the background under the mark is never visible, cleanplate returns the
  most likely image, not detail the codec already threw away (worst case: a 9:16 kitchen shot, 36.2 vs 41.9 dB).
- **AI-upscaled downloads** (Flow 1080p / 4K) re-draw the mark per background. Without the 720p `--guide`, static
  shots keep a faint smoothed patch.
- **A mark on pure white** (≤ 7 levels on 253) is not detected by any tool, cleanplate included. It is also practically
  invisible.
- Video in the browser needs Chrome or Edge and an H.264 MP4 / MOV. Other containers and codecs: use the CLI.

## 💖 Support

If cleanplate saved you time, consider giving it a ⭐ on GitHub or sponsoring its development:

<a href="https://github.com/sponsors/dieplai"><img src="https://img.shields.io/badge/Sponsor-dieplai-EA4AAA?style=for-the-badge&logo=githubsponsors&logoColor=white" alt="Sponsor dieplai on GitHub" height="40"></a>

## License

MIT. Code, templates, fonts and libraries from other projects, with their licenses, are listed in
[`LICENSE-THIRD-PARTY.md`](./LICENSE-THIRD-PARTY.md).

## Credits

Built on research and code from:

- [froggeric/gemini-watermark-and-synthid-remover](https://github.com/froggeric/gemini-watermark-and-synthid-remover)
  (Frédéric Guigand): the Gemini ✦ and legacy Veo mark templates (MIT). Its `wmr` 1.17 is a baseline in the benchmark
- [allenk/GeminiWatermarkTool](https://github.com/allenk/GeminiWatermarkTool) (AllenK): single-frame reverse
  alpha-blend with NCC detection. GWT 0.6.5 is the main baseline in the benchmark
- [ishara-madu/gemini-watermark-remover](https://github.com/ishara-madu/gemini-watermark-remover) (Ishara Madushanka):
  layout of the browser page and its logo (MIT). Its unblending core was ported for comparison only
- [OpenCV](https://github.com/opencv/opencv): Dense Inverse Search optical flow and the image primitives,
  re-implemented in Rust from OpenCV's implementation (Apache-2.0)
- [rayon](https://github.com/rayon-rs/rayon), [RustFFT](https://github.com/ejmahler/RustFFT),
  [image](https://github.com/image-rs/image), [jpeg-encoder](https://github.com/vstroebel/jpeg-encoder),
  [clap](https://github.com/clap-rs/clap): parallelism, FFT, image codecs and the command line
- [mp4box.js](https://github.com/gpac/mp4box.js), [mp4-muxer](https://github.com/Vanilagy/mp4-muxer),
  [coi-serviceworker](https://github.com/gzuidhof/coi-serviceworker), Plus Jakarta Sans (Tokotype): the browser page
- The papers in [References](#references): the method stands on them

Contributions are welcome: read [CONTRIBUTING.md](./CONTRIBUTING.md). A claim of "better" needs a number from
`research/score_gt.py` or `research/img_bench.py` and a visual comparison.

## References

1. T. Dekel, M. Rubinstein, C. Liu, W. T. Freeman. *On the Effectiveness of Visible Watermarks.* CVPR 2017.
2. T. Kroeger, R. Timofte, D. Dai, L. Van Gool. *Fast Optical Flow using Dense Inverse Search.* ECCV 2016.
   [arXiv:1603.03590](https://arxiv.org/abs/1603.03590)
3. H. Zimmer, A. Bruhn, J. Weickert. *Optic Flow in Harmony.* International Journal of Computer Vision 93(3), 2011.
4. L. I. Rudin, S. Osher, E. Fatemi. *Nonlinear total variation based noise removal algorithms.* Physica D 60, 1992.
5. Z. Li, C.-Z. Lu, J. Qin, C.-L. Guo, M.-M. Cheng. *Towards An End-to-End Framework for Flow-Guided Video
   Inpainting.* CVPR 2022. [arXiv:2204.02663](https://arxiv.org/abs/2204.02663)
6. S. Zhou, C. Li, K. C. K. Chan, C. C. Loy. *ProPainter: Improving Propagation and Transformer for Video Inpainting.*
   ICCV 2023. [arXiv:2309.03897](https://arxiv.org/abs/2309.03897)
7. J. P. Lewis. *Fast Normalized Cross-Correlation.* Vision Interface 1995.
8. N. Sundaram, T. Brox, K. Keutzer. *Dense Point Trajectories by GPU-Accelerated Large Displacement Optical Flow.*
   ECCV 2010.
