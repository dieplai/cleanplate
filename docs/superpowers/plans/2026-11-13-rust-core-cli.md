# cleanplate Rust core + CLI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port the Python reference (`research/pipeline.py` for video, `research/image.py` for stills) to a Rust
workspace — a pure-compute core crate and a single-binary CLI — with quality **equal to or better than** the Python
reference on the existing ground-truth benchmarks and an 8-second 720p clip processed in seconds on all cores.

**Architecture:** `cleanplate-core` holds every algorithm on in-memory planes (no file, process or OS access, so it
can later compile to wasm32). `cleanplate-cli` does all IO: ffprobe/ffmpeg subprocess pipes for video (decode to
yuv420p, encode with audio + colour tags copied), the `image` crate for stills. Parallelism is rayon behind the
core's default feature `parallel`. OpenCV is replaced by small, tested ports of exactly the primitives the Python
code calls, including DIS optical flow with OpenCV's PRESET_MEDIUM parameters.

**Tech Stack:** Rust 1.99 (edition 2021), rayon, png (template decoding), image (CLI stills), clap (CLI args),
anyhow (CLI errors). Dev-only: npyz (reading golden fixtures), approx.

**Spec:** `docs/superpowers/specs/2026-11-12-cleanplate-design.md` (§5 constraints, §7 method, §8 layout) and
`docs/method.md`. The executable specification of every number below is `research/pipeline.py` and
`research/image.py` at commit `4b2aeb7`; when this plan and that code disagree, the code wins.

## Global Constraints

- License MIT; template PNGs keep their attribution (`assets/marks/google/ATTRIBUTION.md`).
- Core crate dependencies limited to: `rayon` (optional, feature `parallel`, default on) and `png`. No `std::fs`,
  `std::process`, `std::thread` in core.
- Quality gate (per clip, ground truth, `research/score_gt.py`): Rust PSNR ≥ Python PSNR − 0.05 dB on every one of the
  8 clips (`bench/` × 5 Veo, `bench_diamond/` × 3), and the mean over the 8 ≥ the Python mean. Ghost (`score_gt` col 4)
  not more than 0.5 above Python on any clip.
- Still gate (`research/img_bench.py score`): every case ≥ Python − 0.05 dB; refusals stay refusals.
- Safety gate: the 5 clean video floors and the 9 clean still GTs are refused (process exits non-zero, no output
  file written).
- Speed gate: `veo31fast_flower_720p` (190 frames) and `teacup_static_720p` (static, K = 24) each ≤ 15 s wall time on
  this 12-core machine, release build.
- Memory: never hold full decoded frames for the whole clip — only the bottom-right corner crop (≤ 480×320 luma) per
  frame, as the Python pass 1 does. Peak RSS ≤ 1 GB on a 720p clip.
- The dev machine's ffmpeg has **no libx264**; encoders used: `prores_ks`, `libsvtav1`, `libopenh264`, `ffv1`.
- Commits: author `dieplai`, **no `Co-Authored-By` or tool attribution lines**.
- Defaults only: `--deghost`, `--reach`, `--interp lanczos` and `--chroma-siting center` are research flags and are
  NOT ported.

## Review Focus

1. **Odd frame sizes and margins** (e.g. 720×1280 portrait, 1920×1080, a mark 1 px from the crop edge): the region
   bounds are even-aligned and clamped like Python's `& ~1` and `min(CW, …)`; chroma planes stay exactly half.
   Test in Task 5: `region_bounds_portrait_and_1080p`.
2. **Clips shorter than the temporal window** (a 10-frame shot with K = 24): offsets outside the shot are skipped,
   probes list may be empty → motion 0. Test in Task 5: `short_shot_does_not_panic`.
3. **No audio stream / unknown colour tags**: encoding must still succeed and must not pass `-color_range unknown`.
   Test in Task 6: `encode_args_without_audio_or_tags`.
4. **A clean input** must never be modified (exit code ≠ 0, no output file). Tests in Task 2 (`detect_refuses_clean`)
   and Task 7 (`still_refuses_clean`).
5. **Stills with an alpha channel / grayscale / JPEG**: alpha channel re-attached unchanged, grayscale expanded, JPEG
   written at quality 95. Test in Task 7: `still_keeps_alpha_channel`.

---

## File Structure

```
Cargo.toml                                   workspace: members crates/*
crates/cleanplate-core/
  Cargo.toml
  src/lib.rs            re-exports; feature "parallel"
  src/plane.rs          Plane (f32 image, row-major) + crop/pad/map helpers
  src/filter.rs         gaussian_blur, sobel3, dilate_rect, harmonic_fill
  src/geom.rs           resize_area, resize_cubic, remap (Linear | Cubic, Replicate | Constant)
  src/ncc.rs            match_template_ccoeff_normed, min_max_loc
  src/flow.rs           DIS optical flow (PRESET_MEDIUM)
  src/marks.rs          embedded template catalog, KNOWN placements, OPAQUE set, still placement ratios
  src/detect.rs         video detection, shot cuts
  src/tv.rs             tv_refine (weighted-TV MAP)
  src/video.rs          RegionJob → rebuild (gather, veto, robust mean, TV, chroma) + feather
  src/still.rs          still detection, calibration, inversion, texture-adaptive TV
  tests/primitives.rs   golden tests vs OpenCV fixtures
  tests/fixtures/       small .npy files written by research/export_fixtures.py (committed, < 2 MB total)
crates/cleanplate-cli/
  Cargo.toml
  src/main.rs           clap CLI: `cleanplate <in> <out> [--codec …] [--K 8] [--min-ncc 0.5] [--lam -1]
                        [--no-edge-tv] [--opaque -1] [--guide PATH]` — dispatch by extension
  src/ffmpeg.rs         probe, decode (yuv420p frames iterator), encoder args, encode writer
  src/video_io.rs       pass 1 (corner crops + thumbs), call core, pass 2 (paste + encode)
  src/still_io.rs       read/write stills, call core::still
  tests/cli.rs          end-to-end on tiny generated files
research/export_fixtures.py                  writes crates/cleanplate-core/tests/fixtures/*.npy via OpenCV
bench/rust_vs_python.sh                      acceptance: runs both on 8 GT clips + 9 still cases, checks gates
bench/baseline_python.txt                    frozen Python numbers + timings (Task 0)
```

---

### Task 0: Freeze the Python baseline

**Files:**
- Create: `bench/baseline_python.sh`, `bench/baseline_python.txt`

**Interfaces:**
- Produces: `bench/baseline_python.txt` — one block per clip exactly as printed by `research/score_gt.py`
  (`method PSNR excess ghost sharp`, method name `python`), followed by `time <clip> <seconds>` lines; then the
  `img_bench.py score` table with method name `python`.

- [ ] **Step 1:** Write `bench/baseline_python.sh`: for each of `bench/{veo31fast_flower_720p,night_720p,snow_720p,teacup_static_720p,kitchen_9x16}`
  and `bench_diamond/{veo31fast_flower_720p,night_720p,kitchen_9x16}` run
  `research/pipeline.py <set>/<c>_marked.mp4 out/bench/<set>_<c>_py.crop.mkv` under
  `systemd-run --user --scope -q -p MemoryMax=4G`, time it with `date +%s.%N`, then
  `score_gt.py <set>/<c> python=out/bench/<set>_<c>_py.crop.mkv`; then run `research/image.py` on every
  `bench_img/*_marked.*` and `img_bench.py score bench_img python=…`. Paths are relative to the parent directory
  `gemini-wm/` (the bench data lives there, not in the repo); the script `cd`s there.
- [ ] **Step 2:** Run it (`bash bench/baseline_python.sh > bench/baseline_python.txt`, ~30 min). Expected: 8 score
  blocks, 8 time lines, 9 still rows (3 "no output").
- [ ] **Step 3:** Commit `bench/baseline_python.sh bench/baseline_python.txt` — "bench: freeze Python baseline".

---

### Task 1: Workspace, Plane and the image primitives

**Files:**
- Create: `Cargo.toml`, `crates/cleanplate-core/{Cargo.toml,src/lib.rs,src/plane.rs,src/filter.rs,src/geom.rs}`,
  `crates/cleanplate-core/tests/primitives.rs`, `research/export_fixtures.py`, `crates/cleanplate-core/tests/fixtures/`
- Modify: `.github/workflows/ci.yml` (add a `rust` job: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`)
- Modify: `.gitignore` (add `target/`)

**Interfaces:**
- Produces:
  - `pub struct Plane { pub w: usize, pub h: usize, pub data: Vec<f32> }` with `new(w,h)`, `from_u8(w,h,&[u8])`,
    `at(x,y)`, `set(x,y,v)`, `crop(x,y,w,h) -> Plane`, `pad_const(n, v) -> Plane`.
  - `pub fn gaussian_blur(p: &Plane, sigma: f32) -> Plane` — OpenCV `GaussianBlur(src,(0,0),sigma)` for CV_32F:
    ksize = round(sigma·8 + 1) | 1, border REFLECT_101, separable.
  - `pub fn sobel3(p: &Plane) -> (Plane, Plane)` — ksize 3, border REFLECT_101, unnormalised (OpenCV scale 1).
  - `pub fn dilate_rect(m: &[bool], w: usize, h: usize, kw: usize, kh: usize) -> Vec<bool>` — anchor centre, pixels
    outside the image ignored.
  - `pub fn harmonic_fill(p: &Plane, hole: &[bool], iters: usize) -> Plane` — exactly `pipeline.harmonic` (init hole
    to mean of non-hole, Jacobi 4-neighbour average with edge padding, 150 iterations by default).
  - `pub fn resize_area(p: &Plane, w: usize, h: usize) -> Plane`, `pub fn resize_cubic(p: &Plane, w: usize, h: usize) -> Plane`
    — OpenCV INTER_AREA / INTER_CUBIC (a = −0.75, pixel-centre convention, REPLICATE border).
  - `pub enum Interp { Linear, Cubic }`, `pub enum Border { Replicate, Constant(f32) }`,
    `pub fn remap(src: &Plane, mapx: &Plane, mapy: &Plane, interp: Interp, border: Border) -> Plane` — OpenCV `remap`.

- [ ] **Step 1: Write `research/export_fixtures.py`.** Deterministic inputs (`np.random.default_rng(1)`, a 37×29 float32
  plane in [0,255] and a 64×48 one), run the OpenCV call for each primitive above with the parameters the pipeline
  uses (blur σ 3 and 1.0, Sobel, dilate 5×5/3×3/9×9, INTER_AREA 0.5×, INTER_CUBIC 1.5×, remap with a random
  sub-pixel map ±3 px for Linear/Cubic × Replicate/Constant(0)), save inputs and outputs as `.npy` into
  `crates/cleanplate-core/tests/fixtures/`. Also save `harmonic` from `pipeline.py` (copy the function verbatim).
- [ ] **Step 2: Write the failing tests** in `tests/primitives.rs`, one per primitive, loading fixtures with `npyz`:
  `gaussian_matches_opencv` (max abs err ≤ 1e-3), `sobel_matches_opencv` (≤ 1e-3), `dilate_matches_opencv` (exact),
  `harmonic_matches_python` (≤ 1e-3), `resize_area_matches_opencv` (≤ 1e-3), `resize_cubic_matches_opencv` (≤ 1e-2),
  `remap_{linear,cubic}_{replicate,constant}_matches_opencv` (≤ 1e-2).
- [ ] **Step 3:** `cargo test -p cleanplate-core --test primitives` → FAIL (functions missing).
- [ ] **Step 4:** Implement the five modules.
- [ ] **Step 5:** `cargo test -p cleanplate-core` → PASS; `cargo clippy -- -D warnings` clean.
- [ ] **Step 6:** Commit — "core: Plane and OpenCV-equivalent primitives with golden tests".

---

### Task 2: Mark catalog, template matching, video detection, shot cuts

**Files:**
- Create: `crates/cleanplate-core/src/{ncc.rs,marks.rs,detect.rs}`; extend fixtures + `tests/primitives.rs`
- Test: `crates/cleanplate-core/tests/detect.rs`

**Interfaces:**
- Consumes: `Plane`, `gaussian_blur`, `resize_area`, `resize_cubic` (Task 1).
- Produces:
  - `pub fn match_template_ccoeff_normed(img: &Plane, tpl: &Plane) -> Plane` and `pub fn max_loc(p: &Plane) -> (f32, usize, usize)`.
  - `pub struct Mark { pub name: &'static str, pub alpha: Plane /* trimmed to alpha>0, values 0..1 */, pub known: &'static [(u32, u32)], pub opaque: Option<u32> }`
    and `pub fn catalog() -> &'static [Mark]` — every PNG in `assets/marks/google/` embedded with `include_bytes!`,
    decoded once (`std::sync::OnceLock`), sorted by file name like Python's `sorted(glob)`. `known` = Python `KNOWN`;
    `opaque` = Python `OPAQUE` (`veo-small-1080p.png` → 1).
  - `pub struct Detection { pub mark: &'static Mark, pub scale: f32, pub x: usize, pub y: usize /* in the corner crop */, pub ncc: f32, pub at_known: bool, pub alpha: Plane /* scaled */ }`
  - `pub fn detect_video(corner_y: &[&Plane], frame_w: usize, frame_h: usize, min_ncc: f32) -> Result<Detection, Refusal>`
    where `pub struct Refusal { pub best_ncc: f32, pub floor: f32 }` — exactly `pipeline.py` lines 93–125 + the 1.5×
    720p → 1080p swap (lines 132–137): sample every `max(1, N/120)`-th frame, mean of (frame − blur σ 3), scales
    `{1.0, round(min(W,H)/720, 3)}`, skip if template ≥ crop or min side < 6, floor 0.45 at a known placement
    (|margin − k·scale| ≤ 3) else max(min_ncc, 0.75).
  - `pub fn shot_cuts(thumbs: &[Plane]) -> Vec<(usize, usize)>` — lines 83–89 (64×36 thumbnails, threshold
    `max(10, 5·(median(d[i−12..i+12]) + 1))`, min gap 4).

- [ ] **Step 1:** Extend `export_fixtures.py`: matchTemplate on a fixture pair; and for detection, the sampled corner
  luma crops (every 4th of 40 frames, u8) of `bench/veo31fast_flower_720p_marked.mp4`, of
  `bench_diamond/veo31fast_flower_720p_marked.mp4` and of `bench/veo31fast_flower_720p_floor.mp4`, plus the Python
  detection result for each (name, scale, x, y, ncc). Keep each fixture ≤ 400 KB (store u8).
- [ ] **Step 2: Failing tests:** `match_template_matches_opencv` (≤ 1e-4); `detect_finds_veo_small` (same name,
  same x/y, |ncc − py| ≤ 0.01); `detect_finds_diamond` (same); `detect_refuses_clean` (Err, floor 0.75);
  `shot_cuts_on_concatenation` (two synthetic 20-frame shots of different constant brightness → `[(0,20),(20,40)]`).
- [ ] **Step 3:** Run → FAIL. **Step 4:** Implement. **Step 5:** Run → PASS.
- [ ] **Step 6:** Commit — "core: template catalog, safe video detection, shot cuts".

---

### Task 3: DIS optical flow (OpenCV PRESET_MEDIUM)

**Files:**
- Create: `crates/cleanplate-core/src/flow.rs`; Test: `crates/cleanplate-core/tests/flow.rs`; fixtures.

**Interfaces:**
- Produces: `pub struct Flow { pub u: Plane, pub v: Plane }` and
  `pub fn dis_flow(i0: &Plane, i1: &Plane) -> Flow` (inputs are u8-valued planes, i.e. `clip(R,0,255)` rounded down,
  like `R8` in Python). Parameters fixed: finest_scale 1, patch 8, stride 3, gradient-descent iterations 25,
  variational refinement iterations 5, α 20, δ 5, γ 10, mean normalisation on, spatial propagation on; coarsest
  scale `floor(log2(2·min(w,h) / (4·patch)))` clamped ≥ 0 (OpenCV `autoSelectCoarsestScale`).

The algorithm (Kroeger et al., ECCV 2016, as in OpenCV `dis_flow.cpp`) per scale, coarse to fine: inverse-search
each patch on a stride-3 grid (spatial propagation forward then backward pass, then 25 Gauss–Newton iterations on
the inverse compositional SSD with mean normalisation), densify by weighting each overlapping patch's displacement by
`1/max(1, |photometric error|)`, then variational refinement (5 SOR iterations of the brightness + gradient constancy
+ smoothness energy with the α/δ/γ weights), upscale ×2 to the next scale.

- [ ] **Step 1:** Fixtures: 6 pairs of real region crops `R8[t], R8[s]` (|t−s| ∈ {1, 4, 12}) from the flower and the
  teacup GT clips, and OpenCV's DIS output for each.
- [ ] **Step 2: Failing tests:** `dis_recovers_subpixel_translation` (a smooth random texture shifted by (1.3, −0.7)
  with cubic remap: median end-point error over the interior < 0.05 px); `dis_matches_opencv_on_real_pairs` (median
  EPE vs OpenCV < 0.15 px, 90th percentile < 0.6 px on each fixture pair); `dis_zero_on_identical` (|flow| < 0.01).
- [ ] **Step 3:** Run → FAIL. **Step 4:** Implement. **Step 5:** Run → PASS.
- [ ] **Step 6:** Commit — "core: DIS optical flow".

---

### Task 4: TV-MAP refinement

**Files:**
- Create: `crates/cleanplate-core/src/tv.rs`; Test in `tests/primitives.rs`; fixture.

**Interfaces:**
- Produces: `pub fn tv_refine(mu: &Plane, w: &Plane, mask: &[bool], lam: &Plane, iters: usize, eps: f32) -> Plane`
  — verbatim semantics of `pipeline.tv_refine` (forward differences with replicated last row/col, `np.roll`
  wrap-around in the divergence, per-pixel step `0.9 / (2w + 8·lam/eps)`, update only inside `mask`); callers pass
  `iters = 300`, `eps = 1.0`; a scalar λ is a constant `Plane`.

- [ ] **Step 1:** Fixture: random μ, w ∈ [0.05, 48], a mask, per-pixel λ ∈ {0, 16}; Python output.
- [ ] **Step 2:** Failing test `tv_matches_python` (max abs err ≤ 1e-2 after 300 iterations).
- [ ] **Step 3–5:** Run → FAIL, implement, run → PASS.
- [ ] **Step 6:** Commit — "core: weighted-TV MAP refinement".

---

### Task 5: Video region rebuild (the method) + parallel flow cache

**Files:**
- Create: `crates/cleanplate-core/src/video.rs`; Test: `crates/cleanplate-core/tests/video.rs`

**Interfaces:**
- Consumes: Tasks 1–4.
- Produces:
  - `pub fn prepare_alpha(det: &Detection, opaque_flag: i32, has_guide: bool) -> Prepared` with
    `pub struct Prepared { pub alpha: Plane, pub mx: i64, pub my: i64, pub op: i32 /* -1 = alpha overlay */ }` —
    lines 138–145 (OPAQUE lookup, guide ⇒ op 3, pad by op+1, `0.95·dilate(alpha > 0.08, 2·op+1)`).
  - `pub struct Region { pub x0: usize, pub y0: usize, pub w: usize, pub h: usize }` (luma coords inside the corner crop,
    even-aligned) and `pub fn region_bounds(p: &Prepared, crop_w: usize, crop_h: usize) -> Region` — lines 147–151 with MG = 40.
  - `pub struct Params { pub k: usize /*8*/, pub lam: f32 /*-1 = auto*/, pub edge_tv: bool /*true*/, pub opaque: i32 /*-1 = auto*/, pub tv_range_white: f32 /*235 or 255*/ }`
  - `pub struct Guide { pub y: Vec<Plane>, pub u: Vec<Plane>, pub v: Vec<Plane> }` — guide planes already resampled onto the
    region grid (CLI does the resampling; core never reads files).
  - `pub struct Rebuilt { pub y: Vec<Plane>, pub u: Vec<Plane>, pub v: Vec<Plane>, pub feather_y: Plane, pub feather_c: Plane, pub shot_logs: Vec<ShotLog> }`
    with `pub struct ShotLog { pub s0: usize, pub s1: usize, pub motion: f32, pub k: usize, pub lam: f32, pub clean_share: f32 }`.
  - `pub fn rebuild(jy: &[Plane], ju: &[Plane], jv: &[Plane], alpha_y: &Plane /* Prepared.alpha placed in the region */, op: i32, shots: &[(usize, usize)], params: &Params, guide: Option<&Guide>) -> Rebuilt`
    — exactly `pipeline.py` lines 152–362 (left chroma siting only; opaque mode + guide; edge-ring TV; per-plane veto
    and robust reweighting; `conf`; feather σ 1.0 × 1.5 clipped).
- **Performance design (decided):** for each shot, compute every ordered flow pair `(t, s)` with `0 < |t−s| ≤ K`
  **once** into a cache (`Vec<Flow>` indexed by (t, offset)), in parallel with rayon (`par_iter` over t); the Python
  code recomputes both directions per t, i.e. 2× the work. Then fuse frames in parallel over t (each t is
  independent given the cache). Region flow is harmonic-refilled inside `FW`/`Mflow` exactly as `flow()` in Python.
  Memory: K = 24 static shots → 48 flows × 190 frames × region (≈ 140×100 × 2 × 4 B) ≈ 1 GB worst case — therefore
  compute the cache in sliding windows of 64 frames (pairs needed by frames t ∈ [a, a+64) only) and drop it per window.

- [ ] **Step 1: Failing tests:** `region_bounds_portrait_and_1080p` (720×1280 diamond at (576,1136) and 1920×1080
  1080p Veo at (1864,1040): bounds even, inside the crop, mark fully inside); `short_shot_does_not_panic` (10-frame
  synthetic clip with a composited mark, K = 24 → returns 10 frames); `static_synthetic_restores` (40-frame static
  textured scene with the `veo-small-720p` alpha composited at Ly 235 and 2-level Gaussian noise: PSNR in the mark box
  ≥ 40 dB); `moving_synthetic_uses_clean_pixels` (scene translating 2 px/frame: `clean_share ≥ 0.5` and PSNR ≥ 42 dB).
- [ ] **Step 2:** Run → FAIL. **Step 3:** Implement. **Step 4:** Run → PASS.
- [ ] **Step 5:** Commit — "core: video region rebuild with cached parallel flow".

---

### Task 6: CLI video path (ffmpeg IO, encoders, paste) + the quality gate

**Files:**
- Create: `crates/cleanplate-cli/{Cargo.toml,src/main.rs,src/ffmpeg.rs,src/video_io.rs}`, `crates/cleanplate-cli/tests/cli.rs`,
  `bench/rust_vs_python.sh`

**Interfaces:**
- Consumes: `detect_video`, `shot_cuts`, `prepare_alpha`, `region_bounds`, `rebuild`, `resize_area` (thumbnails), `remap` (guide).
- Produces:
  - `ffmpeg::Probe { w, h, fps: String, has_audio: bool, color: [Option<String>; 4] /*range, space, primaries, transfer*/, bitrate: u64 }`,
    `ffmpeg::probe(path) -> anyhow::Result<Probe>`, `ffmpeg::decode_yuv420(path, w, h) -> impl Iterator<Item = Vec<u8>>`,
    `ffmpeg::encoder_args(codec: &str, out: &Path, probe: &Probe) -> Vec<String>` (codec table, `auto`, `.crop.`
    research mode = lossless FFV1 of the bottom-right 384×256, tags only when known and ≠ "unknown").
  - CLI: `cleanplate IN OUT [--codec auto|prores|av1|h264|ffv1] [--k 8] [--min-ncc 0.5] [--lam -1] [--no-edge-tv] [--opaque -1] [--guide PATH] [--threads N]`;
    log lines with the same wording as Python (`detected …`, `shots: …`, `shot a-b: motion=… K=… lambda=… clean-share=…`, `done -> …`);
    refusal prints the Python message and exits with code 2 without creating OUT.

- [ ] **Step 1: Failing tests** (`tests/cli.rs`, skipped when `ffmpeg` is absent): `encode_args_without_audio_or_tags`
  (unit test on `encoder_args` with an all-`None` probe: no `-color_range`, contains `-map 1:a?`);
  `video_roundtrip_small` (generate a 48-frame 640×360 clip with `ffmpeg -f lavfi testsrc2`, composite the Veo
  template with a 10-line Rust helper, run the binary to `.mkv` with `--codec ffv1`, check exit 0, same frame count,
  audio absent is fine); `video_refuses_clean` (the clean testsrc2 clip → exit 2, no output file).
- [ ] **Step 2:** Run → FAIL. **Step 3:** Implement (pass 1 streams frames: keep corner crops u8 + 64×36 thumbnails;
  pass 2 re-decodes and pastes `feather·O + (1−feather)·src` with +0.5 rounding, as Python lines 385–393).
- [ ] **Step 4:** Run → PASS.
- [ ] **Step 5:** Write `bench/rust_vs_python.sh`: build release, run the binary on the 8 GT clips to
  `out/bench/<set>_<c>_rs.crop.mkv` with `/usr/bin/time -f "%e s %M KB"`, score with `score_gt.py <set>/<c> python=… rust=…`,
  and fail (exit 1) on any violated gate from Global Constraints using the numbers in `bench/baseline_python.txt`.
- [ ] **Step 6:** Run `bash bench/rust_vs_python.sh`. Expected: all gates PASS, flower ≤ 15 s, teacup ≤ 15 s,
  peak RSS ≤ 1 GB. If a clip fails the PSNR gate, diff the per-shot log lines against Python first (motion, K,
  λ, clean-share) — a mismatch there points at flow or detection, not at the fusion.
- [ ] **Step 7:** Commit — "cli: video path; Rust ≥ Python on all 8 ground-truth clips".

---

### Task 7: Stills (core + CLI) + the still gate

**Files:**
- Create: `crates/cleanplate-core/src/still.rs`, `crates/cleanplate-cli/src/still_io.rs`; Test:
  `crates/cleanplate-core/tests/still.rs`; extend `bench/rust_vs_python.sh`.

**Interfaces:**
- Consumes: Tasks 1–2 (`gaussian_blur`, `sobel3`, `resize_*`, `remap`/affine warp via `remap`, `match_template_*`, `catalog`).
- Produces: `pub struct StillResult { pub rgb: Vec<f32> /*w*h*3, BGR order like OpenCV*/, pub log: Vec<String> }`,
  `pub fn remove_still(rgb: &[u8], w: usize, h: usize, lossy: bool, lam: f32 /*-1 auto*/) -> Result<StillResult, Refusal>`
  — exactly `research/image.py`: placements `PLACE = (2/3, 1.6, 2.0)` with tolerance `max(3, 0.1·size)`, sizes
  24..min(200, CS/2) step 2, +0.15 placement bonus and placement probes, floors 0.3 / 0.6; calibration by the
  signed-gradient-correlation zero crossing (22 bisections on [0.05, 0.9]), shape/offset grid ±1 step 0.5 then
  ±0.25, snap to native opacity (tolerance `0.04·max(1, native/0.3)`), opacity-consistent candidates first,
  exact-size templates first, fractional size ±1 step 0.25 only when size ∉ {36, 48, 96}; lossy-only TV with
  `λ = clip(6 − 5·(tex − 3)/9, 1, 6)`, 200 iterations, w = (1 − a)² + 0.02.
  CLI: same binary; extensions `.png .jpg .jpeg .webp` route to stills; alpha channel re-attached; JPEG/WebP q 95.

- [ ] **Step 1: Failing tests:** `still_synthetic_v1_48` (port of `tests/test_image.py`: PSNR ≥ 45 dB in the box);
  `still_refuses_clean` (Err); `still_keeps_alpha_channel` (CLI: RGBA PNG in → RGBA PNG out, alpha bytes identical).
- [ ] **Step 2:** Run → FAIL. **Step 3:** Implement. **Step 4:** Run → PASS.
- [ ] **Step 5:** Extend `bench/rust_vs_python.sh` with the 9 still cases (`img_bench.py score bench_img python=… rust=…`)
  and the still gate; run it. Expected: PASS, the 3 kitchen cases refused by both.
- [ ] **Step 6:** Commit — "core+cli: stills; Rust ≥ Python on the still bench".

---

### Task 8: Docs, CI and release binary

**Files:**
- Modify: `README.md`, `README.vi.md` (Quick start: `cargo install --path crates/cleanplate-cli` and
  `cleanplate in.mp4 out.mov --codec prores`; Python kept as the reference), `CHANGELOG.md`, `docs/benchmark.md`
  (Rust vs Python table and timings from `bench/rust_vs_python.sh`), `.github/workflows/ci.yml`
- Create: `.github/workflows/release.yml` (on tag `v*`: build `cleanplate` for x86_64-linux, x86_64/aarch64-macos,
  x86_64-windows with `cargo build --release -p cleanplate-cli`, attach to the GitHub release)

- [ ] **Step 1:** Update docs with the measured numbers (no estimates).
- [ ] **Step 2:** `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` → PASS; push; CI green.
- [ ] **Step 3:** Commit — "docs: Rust CLI quick start, Rust vs Python benchmark; release workflow".
