# Mark templates - provenance

| File | Source | License |
|---|---|---|
| `veo-small-720p.png` | Extracted by cleanplate (`research/make_template.py`) from a Veo 3.1 clip whose mark sits on a near-black background | MIT (this repo) |
| `gemini-diamond-36.png`, `gemini-diamond-48.png`, `gemini-diamond-96.png`, `veo-legacy-68.png`, `veo-legacy-99.png` | [froggeric/gemini-watermark-and-synthid-remover](https://github.com/froggeric/gemini-watermark-and-synthid-remover) `assets/watermark-masks/` | MIT, © 2026 Frédéric Guigand |

Convention: single-channel PNG, pixel value = alpha × 255 (255 = opaque mark, 0 = no mark). The mark colour is white.
| `gemini-diamond-v1-48.png`, `gemini-diamond-v1-96.png` | The "V1" (legacy, alpha ≈ 0.50) diamond captures embedded in [froggeric/gemini-watermark-and-synthid-remover](https://github.com/froggeric/gemini-watermark-and-synthid-remover) `assets/embedded_assets.hpp` (`bg_48_png`, `bg_96_png`), captured on black: value = alpha × 255 | MIT, © 2026 Frédéric Guigand |
