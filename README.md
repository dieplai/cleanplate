<div align="center">

# cleanplate

### Remove the visible mark, keep the picture. Nothing invented, nothing blurred.

[![CI](https://github.com/dieplai/cleanplate/actions/workflows/ci.yml/badge.svg)](https://github.com/dieplai/cleanplate/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)
[![Rust](https://img.shields.io/badge/rust-core%20%2B%20CLI-B7410E.svg)](./crates)
[![Status: research preview](https://img.shields.io/badge/status-research%20preview-orange.svg)](./CHANGELOG.md)

**English** · [Tiếng Việt](./README.vi.md)

</div>

<p align="center"><img src="docs/assets/hero_veo_legacy.gif" width="720" alt="Veo text mark: original, GWT, wmr, cleanplate"></p>

**1. What it is.** cleanplate removes the *visible* marks that AI generators stamp on their output (Google Veo's
"Veo" text and Gemini's ✦ diamond) from **videos and still images**, and gives back footage that looks like it never
had one. It is built for creators who make non-commercial work with these tools and want a clean plate for a film
scene, an edit or a thumbnail.

**2. Why another remover.** Existing tools invert the overlay frame by frame and then hide what is left with a blur or
a learned inpainter, which leaves a soft patch. A video has something an image does not: **the scene moves under the
mark, so almost every hidden pixel is visible, clean, in some other frame.** cleanplate rebuilds the mark area from
those real pixels and falls back to a principled MAP estimate only where no frame ever saw the background. On stills
it measures the mark's opacity on the image itself instead of trusting a template.

| Ground truth, PSNR inside the mark (dB) | GWT 0.6.5 | wmr 1.17 | **cleanplate** | ceiling |
|---|---|---|---|---|
| Video, Veo 720p, 5 clips (mean) | 32.6 | — | **37.3** | 39.9 |
| Still, Gemini ✦ PNG (brick / neon) | 43.4 / 40.2 | 56.4 / 56.4 | **56.4 / 56.5** | 60 |
| Still, re-saved JPEG (brick / neon) | 32.7 / 36.0 | 33.0 / 37.3 | **33.0 / 40.8** | 35.4 / 42.3 |
| Still, resized + JPEG, like a shared copy | not detected | not detected / wrong | **30.4 / 34.1** | 32.3 / 43.9 |

<sub>Ceiling / floor = the clean footage through the same codec: no remover can beat it. Full tables, every clip and
the method behind each number: <a href="./docs/benchmark.md">docs/benchmark.md</a>.</sub>

> **Visible marks only.** cleanplate never touches SynthID (the invisible watermark) or C2PA provenance metadata.
> Label AI-generated content as your platform and your audience expect.

## Results at a glance

![Results overview](./docs/assets/overview.png)

<table>
<tr>
<td width="50%"><img src="docs/assets/hero_veo.gif" alt="Veo mark on a moving flower market"><br><sub>Veo 3.1, moving shot: original · GWT · cleanplate</sub></td>
<td width="50%"><img src="docs/assets/hero_gemini.gif" alt="Gemini diamond on a fast cartoon"><br><sub>Gemini ✦ on fast motion (20 px/frame): original · cleanplate</sub></td>
</tr>
</table>

<p align="center"><img src="docs/assets/hero_still.jpg" width="860" alt="Flow stills: GWT edits the wrong spot, wmr does not detect, cleanplate removes"><br>
<sub>Real Flow (Nano Banana) stills, faint 48 px ✦. GWT paints a dark diamond at the wrong spot and leaves the mark; wmr does not detect it.</sub></p>

## What it handles

| Case | How |
|---|---|
| Moving shots | Flow-guided gathering of real pixels from neighbouring frames (DIS optical flow, forward/backward check, inverse-variance fusion) |
| Static shots | Inversion averaged over the shot + weighted-TV MAP; λ is chosen per shot from the measured motion |
| Fast motion | Gross-outlier veto against each frame's own inversion; TV on the mark's edge ring where no clean pixel landed |
| Cuts | Shot detection; pixels are never borrowed across a cut |
| Colour | Works in the video's native YUV with H.264 chroma siting; no RGB round trip |
| Stills | Opacity, sub-pixel offset and (for resized copies) fractional size fitted on the image; texture-adaptive TV for JPEG |
| Mark variants | Veo text (720p, legacy 68 px), Gemini ✦ 36/48/96 px in both renderings (opacity 0.30 and 0.50), Flow stills |
| Safety | A mark must be found with confidence or the file is left untouched (14/14 clean inputs refused) |
| Output | Audio and colour tags copied; ProRes 422 HQ, AV1 10-bit, H.264 or lossless FFV1; stills keep their format |

## Quick start

```bash
git clone https://github.com/dieplai/cleanplate.git && cd cleanplate
cargo install --path crates/cleanplate-cli        # Rust ≥ 1.82; video also needs ffmpeg on PATH

cleanplate photo.png photo_clean.png
cleanplate clip.mp4 clip_clean.mov                # .mov = ProRes 422 HQ; or --codec av1 / h264 / ffv1
cleanplate clip_1080p.mp4 out.mov --guide clip_720p_clean.mov    # AI-upscaled download + its cleaned 720p
```

An 8-second 720p clip takes **3–9 s** (a 9:16 static shot 18 s), a 2528×1696 still **2 s**, on a 12-core laptop —
**16× faster than the Python reference at the same quality** (ground-truth gate: mean 36.65 vs 36.64 dB over 8 clips;
stills identical; `bench/rust_vs_python.sh`). A clean input is never modified: the tool exits with code 2 and writes
nothing.

**Best quality:** download the **native resolution** (Flow: 720p "original size"). Flow's 1080p and 4K downloads are
AI upscales of the *marked* video, so the mark is re-drawn and partly destroys what is under it; if you only have the
1080p, pass the same clip's 720p with `--guide`.

`crates/cleanplate-core` holds every algorithm with no file or OS access (ready for WebAssembly);
`crates/cleanplate-cli` does the IO. The Python code in `research/` stays as the reference specification and the
research bench (`pip install numpy opencv-python`).

## How it works

```
video ─► detect (multi-template NCC, known placements, refuse if unsure)
      ─► shots (hard cuts)
      ─► per frame, per mark pixel:
            invert the overlay in native YUV            I = (J − a·L) / (1 − a)
            gather the same scene point from ±K frames  (optical flow + consistency check)
            weigh: clean pixels ≫ inversions (1 − a)², × flow confidence × time decay
            veto gross outliers, robust mean
            weighted-TV MAP only where no clean pixel exists
      ─► feathered paste into the untouched frame, encode, copy audio + colour tags
```

Details, evidence and the ideas that were tried and rejected: [`docs/method.md`](./docs/method.md),
[`docs/benchmark.md`](./docs/benchmark.md).

## Known limits

- **Static shots are information-limited.** If the background under the mark is never visible, cleanplate returns the
  most likely image, not the detail the codec threw away (worst case: kitchen 9:16, 36.2 vs 41.9 dB floor).
- **AI-upscaled downloads** (Flow 1080p/4K) re-draw the mark per background; without the 720p `--guide`, static shots
  keep a faint smoothed patch.
- **A mark on pure white** (adds ≤ 7 levels on 253) is not detected by any tool, cleanplate included; it is also
  practically invisible.

## Roadmap

- [x] Video reference pipeline + ground-truth benchmark (8 clips)
- [x] Still images, self-calibrated opacity, resized / re-encoded copies
- [x] Flow 1080p (`--guide`), Gemini-app and Flow stills, Zalo round trip
- [x] Rust core + single-file CLI (Linux, macOS, Windows) — same quality as the reference, ~16× faster
- [ ] In-browser drag-and-drop page (WebAssembly + WebCodecs; files never leave your device)
- [ ] Optional upscaling of the clean result

## License

| Part | License |
|---|---|
| Code (`research/`, `tests/`, `docs/*.py`) | [MIT](./LICENSE) |
| Veo 720p / 1080p mark templates (extracted by cleanplate) | MIT |
| Gemini diamond and legacy Veo templates | MIT, © Frédéric Guigand, from [froggeric/gemini-watermark-and-synthid-remover](https://github.com/froggeric/gemini-watermark-and-synthid-remover); see [ATTRIBUTION](./assets/marks/google/ATTRIBUTION.md) |

Method references: Dekel et al., *On the Effectiveness of Visible Watermarks*, CVPR 2017 · E2FGVI (CVPR 2022) and
ProPainter (ICCV 2023) for flow-guided propagation · DIS optical flow (ECCV 2016) · Rudin–Osher–Fatemi TV (1992).

Contributions are welcome: read [CONTRIBUTING.md](./CONTRIBUTING.md). Every claim of "better" needs a number from
`research/score_gt.py` or `research/img_bench.py` and a visual sheet.
