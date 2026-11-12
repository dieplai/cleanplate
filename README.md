# cleanplate

**Clean-plate restoration for AI-generated video and images.** Removes the *visible* marks that generators stamp on
their output (Google Veo text, Gemini ✦ diamond, …) and gives back footage that looks like it never had one — no
blurred patch, no ghost, no shimmer.

> Status: research phase. The method lives in `research/` (Python reference pipeline + ground-truth benchmark).
> The Rust core and CLI come next. See `docs/method.md` for how it works.

## Why another remover

Every existing tool inverts the overlay frame by frame and then hides what is left with a spatial blur or a learned
denoiser. On video that leaves a soft, slightly smeared patch where the mark was. cleanplate uses what a video has
and an image does not: **the scene moves under the mark, so every hidden pixel is visible, clean, in some other
frame.** It rebuilds the mark area from those real pixels (optical flow, inverse-variance fusion) and falls back to a
principled MAP estimate only where no frame ever saw the background.

## Results (ground truth, Veo "Veo" mark)

Measured inside the mark box against the original clean footage. *Floor* = the same clean clip through the same
encoder: the best any remover can possibly do.

| Clip | GWT 0.6.5 | cleanplate | Floor |
|---|---|---|---|
| Flower market (moving, textured) | 28.5 dB · sharpness 0.87 | **32.0 dB · 0.94** | 32.5 dB · 0.96 |
| Night street (dark) | 33.5 dB | **39.9 dB** | 40.5 dB |
| Snow (near-white; GWT skips the real clip) | 35.8 dB | **41.4 dB** | 45.5 dB |
| Kitchen portrait, static wall | 31.8 dB | **36.2 dB** | 41.9 dB |
| Gemini ✦ on flower market | not detected | **33.8 dB** | 34.2 dB |

Full tables, more clips and the Gemini ✦ diamond: `docs/benchmark.md`.

## What it does not do

- It removes **visible** marks only. Invisible watermarks (SynthID) and provenance metadata (C2PA) are left intact,
  on purpose. Label AI-generated content as your platform requires.
- It does not invent content with generative models. Where the background was never visible and the codec threw its
  detail away, cleanplate returns the most likely image, not the lost detail.

## Credits

- Mark templates for the Gemini diamond and legacy Veo text: [froggeric/gemini-watermark-and-synthid-remover](https://github.com/froggeric/gemini-watermark-and-synthid-remover) (MIT). See `assets/marks/google/ATTRIBUTION.md`.
- Method references: Dekel et al., CVPR 2017; E2FGVI (CVPR 2022) and ProPainter (ICCV 2023); DIS optical flow (ECCV 2016).

## License

MIT
