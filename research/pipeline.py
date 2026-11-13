#!/usr/bin/env python3
"""cleanplate reference pipeline (Python) — video with a static visible mark in, clean full video out.

The Rust implementation must reproduce this pipeline; this file is the executable spec.

  pass 1  decode once; keep only (a) a crop around the bottom-right corner (uint8 YUV) and (b) a tiny
          thumbnail per frame for shot detection. Memory ~ crop size x frames, independent of resolution.
  detect  multi-template, multi-scale NCC on the time-averaged high-pass of the corner. Low confidence ->
          refuse (never damage a clip).
  shots   hard cuts from thumbnail differences; temporal gathering never crosses a cut.
  rebuild per frame, per mark pixel: invert the overlay in native YUV, then gather motion-compensated
          candidates from neighbouring frames of the same shot (DIS flow, re-filled inside the mark,
          forward/backward checked). Candidates that land outside the mark are real clean pixels;
          the rest are inversions weighted by (1-a)^2. Robust weighted mean. Static shots get a wider
          window and a shape-constrained removal of the codec's static ghost.
  pass 2  decode again, paste the rebuilt region (feathered), encode; audio and colour tags are copied.

usage: pipeline.py in.mp4 out.(mp4|mov|mkv) [--codec auto|prores|av1|h264|ffv1] [--K 8] [--marks DIR]
"""
import sys, os, json, glob, argparse, subprocess, time
import numpy as np, cv2

ap = argparse.ArgumentParser()
ap.add_argument("inp"); ap.add_argument("out")
ap.add_argument("--codec", default="auto")
ap.add_argument("--K", type=int, default=8)
ap.add_argument("--marks", default=os.path.join(os.path.dirname(__file__), "..", "assets", "marks"))
ap.add_argument("--min-ncc", type=float, default=0.5)
ap.add_argument("--lam", type=float, default=-1.0,
                help="TV prior strength; -1 = automatic per shot (16 for static/slow shots, 0 for moving ones)")
ap.add_argument("--reach", action="store_true", help="research: far, sparse neighbours for slow motion (worse on GT)")
ap.add_argument("--no-edge-tv", dest="edge_tv", action="store_false",
                help="disable the TV prior on the mark's edge ring where no clean evidence landed (on by default: removes the "
                     "hairline outline in fast motion; costs <=0.25 dB on slow textured GT clips)")
ap.add_argument("--chroma-siting", default="left", choices=["left", "center"])
ap.add_argument("--opaque", type=int, default=-1,
                help="treat the mark as fully covering its (dilated by N px) support: no inversion, rebuild from other "
                     "frames + TV fill. -1 = auto (on for marks whose look depends on the background, e.g. Flow 1080p)")
ap.add_argument("--guide", default=None,
                help="opaque marks: a cleaned lower-resolution version of the SAME clip (e.g. cleanplate's output of the Flow "
                     "720p download). Its upscaled pixels fill the mark's strokes instead of a spatial guess")
ap.add_argument("--interp", default="cubic", choices=["cubic", "lanczos"], help="warp interpolation for luma candidates")
ap.add_argument("--deghost", action="store_true",
                help="research only: shape-basis static-ghost subtraction. OFF by default — it mistakes a real background/fill "
                     "difference for a ghost (painted a grey diamond on a black static shot); the TV refinement covers it")
args = ap.parse_args()
T0 = time.time()

def log(*a): print(f"[{time.time() - T0:6.1f}s]", *a, flush=True)

# ---------------------------------------------------------------- probe
pr = json.loads(subprocess.check_output(["ffprobe", "-v", "error", "-show_streams", "-show_format", "-of", "json", args.inp]))
vs = next(s for s in pr["streams"] if s["codec_type"] == "video")
W, H, FPS = vs["width"], vs["height"], vs["r_frame_rate"]
has_audio = any(s["codec_type"] == "audio" for s in pr["streams"])
color = {k: vs.get(k) for k in ("color_range", "color_space", "color_primaries", "color_transfer")}
src_bitrate = int(vs.get("bit_rate") or pr["format"].get("bit_rate") or 8_000_000)
log(f"{W}x{H} @ {FPS}, {vs.get('pix_fmt')}, audio={has_audio}, colour={color}")
FS = W * H * 3 // 2

def decode():
    p = subprocess.Popen(["ffmpeg", "-v", "error", "-i", args.inp, "-f", "rawvideo", "-pix_fmt", "yuv420p", "-"],
                         stdout=subprocess.PIPE, bufsize=FS * 2)
    while True:
        b = p.stdout.read(FS)
        if len(b) < FS: break
        f = np.frombuffer(b, np.uint8)
        yield f[:W * H].reshape(H, W), f[W * H:W * H * 5 // 4].reshape(H // 2, W // 2), f[W * H * 5 // 4:].reshape(H // 2, W // 2)
    p.wait()

# ---------------------------------------------------------------- pass 1
CW, CH = min(W // 2, 480) & ~1, min(H // 2, 320) & ~1          # corner searched for the mark
cx0, cy0 = W - CW, H - CH
Yc, Uc, Vc, thumbs = [], [], [], []
for y, u, v in decode():
    Yc.append(y[cy0:, cx0:].copy()); Uc.append(u[cy0 // 2:, cx0 // 2:].copy()); Vc.append(v[cy0 // 2:, cx0 // 2:].copy())
    thumbs.append(cv2.resize(y, (64, 36), interpolation=cv2.INTER_AREA).astype(np.float32))
Yc, Uc, Vc = np.array(Yc), np.array(Uc), np.array(Vc)
N = len(Yc)
log(f"pass 1: {N} frames, corner crop {CW}x{CH}")

# ---------------------------------------------------------------- shots
d = np.array([0.0] + [np.abs(thumbs[i] - thumbs[i - 1]).mean() for i in range(1, N)])
cuts = [0]
for i in range(1, N):
    local = np.median(d[max(1, i - 12):i + 12]) + 1.0
    if d[i] > max(10.0, 5.0 * local) and i - cuts[-1] >= 4:
        cuts.append(i)
shots = list(zip(cuts, cuts[1:] + [N]))
log(f"shots: {len(shots)} -> {shots}")

# ---------------------------------------------------------------- detect
step = max(1, N // 120)
samp = Yc[::step].astype(np.float32)
hp = (samp - np.array([cv2.GaussianBlur(s, (0, 0), 3) for s in samp])).mean(0)
best = None
for f in sorted(glob.glob(os.path.join(args.marks, "*", "*.png"))):
    A0 = cv2.imread(f, cv2.IMREAD_GRAYSCALE).astype(np.float32) / 255.0
    ys, xs = np.where(A0 > 0); A0 = A0[ys.min():ys.max() + 1, xs.min():xs.max() + 1]
    # only physically plausible scales: native, and the resolution ratio to the 720p reference (a 14x6 down-scaled
    # template matched random texture on CLEAN clips at ncc~0.5 — never let a tiny template hunt freely)
    for sc in sorted({1.0, round(min(W, H) / 720, 3)}):
        A = cv2.resize(A0, None, fx=sc, fy=sc, interpolation=cv2.INTER_AREA if sc < 1 else cv2.INTER_CUBIC) if sc != 1 else A0
        if A.shape[0] >= CH or A.shape[1] >= CW or min(A.shape) < 6: continue
        t = A - cv2.GaussianBlur(A, (0, 0), 3)
        r = cv2.matchTemplate(hp, t, cv2.TM_CCOEFF_NORMED)
        _, sc_, _, loc = cv2.minMaxLoc(r)
        if best is None or sc_ > best[0]:
            best = (sc_, os.path.relpath(f, args.marks), sc, loc, A)
ncc, mname, mscale, (mx, my), A = best
# generators stamp marks at fixed placements; a match exactly there is far less likely to be chance, so a faint mark
# (white text on a bright, busy background — real teacup clip scored 0.55) may pass a lower bar. (right, bottom) margins
# in pixels at the template's native scale.
KNOWN = {"veo-small-720p.png": [(16, 16)], "veo-small-1080p.png": [(24, 24)], "gemini-diamond-48.png": [(72, 72), (96, 96)],
         "gemini-diamond-96.png": [(64, 64), (192, 192)], "gemini-diamond-36.png": [(32, 32)],
         "veo-legacy-68.png": [(17, 18)]}
mr, mb = CW - (mx + A.shape[1]), CH - (my + A.shape[0])
at_known = any(abs(mr - r * mscale) <= 3 and abs(mb - b * mscale) <= 3 for r, b in KNOWN.get(os.path.basename(mname), []))
# unknown placement needs a strong match; a known placement may be faint. Clean clips peaked at 0.48-0.52 elsewhere.
floor_ = 0.45 if at_known else max(args.min_ncc, 0.75)   # weakest real mark seen: 0.55 (teacup)
log(f"detected {mname} x{mscale:.2f} at ({mx + cx0},{my + cy0}) ncc={ncc:.3f}"
    f"{' (known placement)' if at_known else ''}, threshold {floor_:.2f}")
if ncc < floor_:
    sys.exit(f"no mark found with confidence (best ncc {ncc:.2f} < {floor_:.2f}) — leaving the video untouched")
# Flow's 1080p download is an AI upscale of the marked 720p: the mark is re-drawn per background (opaque white on
# dark, white + dark ringing on bright; implied-alpha corr with the template 0.83 vs 0.24). No fixed alpha fits, so
# the stroke support is treated as unknown pixels.
OPAQUE = {"veo-small-1080p.png": 1}
# on bright backgrounds the re-drawn 1080p mark can match the 1.5x 720p glyph better; at its known placement it is the
# same mark, so use the native 1080p support
if os.path.basename(mname) == "veo-small-720p.png" and abs(mscale - 1.5) < 0.01 and at_known:
    mname = "google/veo-small-1080p.png"
    A = cv2.imread(os.path.join(args.marks, mname), cv2.IMREAD_GRAYSCALE).astype(np.float32) / 255.0
    ys, xs = np.where(A > 0); A = A[ys.min():ys.max() + 1, xs.min():xs.max() + 1]
    mx, my = CW - 24 - A.shape[1], CH - 24 - A.shape[0]
    log(f"1.5x 720p glyph at the Flow 1080p placement -> {mname} at ({mx + cx0},{my + cy0})")
op = args.opaque if args.opaque >= 0 else OPAQUE.get(os.path.basename(mname), -1)
if args.guide and args.opaque < 0 and op >= 0:
    op = 3   # with real guide content, also replace the upscaler's ringing/blocky halo around the strokes (+1px left specks)
if op >= 0:
    A = np.pad(A, op + 1); mx -= op + 1; my -= op + 1
    A = 0.95 * cv2.dilate((A > 0.08).astype(np.uint8), np.ones((2 * op + 1, 2 * op + 1), np.uint8)).astype(np.float32)
    log(f"opaque mode: support dilated {op}px, {int((A > 0).sum())} px rebuilt without inversion")
th, tw = A.shape

# working region: mark + context margin for motion estimation, even-aligned, inside the corner crop
MG = 40
rx0, ry0 = max(0, mx - MG) & ~1, max(0, my - MG) & ~1
rx1, ry1 = min(CW, (mx + tw + MG + 1) & ~1), min(CH, (my + th + MG + 1) & ~1)
RW, RH = rx1 - rx0, ry1 - ry0
Ay = np.zeros((RH, RW), np.float32); Ay[my - ry0:my - ry0 + th, mx - rx0:mx - rx0 + tw] = np.clip(A, 0, 0.95)
# chroma alpha with H.264's default 4:2:0 siting ("left": co-sited with even luma columns, centred vertically).
# Probe on real Veo clips: 13-22% less mark-shaped chroma leftover than a centred 2x2 average.
_h = cv2.filter2D(Ay, -1, np.array([[0.25, 0.5, 0.25]], np.float32), borderType=cv2.BORDER_REPLICATE)
Ac = _h.reshape(RH // 2, 2, RW).mean(axis=1)[:, ::2] if args.chroma_siting == "left" else Ay.reshape(RH // 2, 2, RW // 2, 2).mean(axis=(1, 3))
Ly = 235.0 if (color["color_range"] in (None, "tv", "unknown")) else 255.0   # white in the stored range
Lc = 128.0

Jy = Yc[:, ry0:ry1, rx0:rx1].astype(np.float32)
Ju = Uc[:, ry0 // 2:ry1 // 2, rx0 // 2:rx1 // 2].astype(np.float32)
Jv = Vc[:, ry0 // 2:ry1 // 2, rx0 // 2:rx1 // 2].astype(np.float32)
del Yc, Uc, Vc

# ---------------------------------------------------------------- rebuild
Ry = (Jy - Ay * Ly) / (1 - Ay)
Ru = (Ju - Ac * Lc) / (1 - Ac)
Rv = (Jv - Ac * Lc) / (1 - Ac)
M = cv2.dilate((Ay > 0.01).astype(np.uint8), np.ones((5, 5), np.uint8)).astype(bool)
Mflow = cv2.dilate(M.astype(np.uint8), np.ones((5, 5), np.uint8)).astype(bool)
fy_, fx_ = np.where(Mflow)
FW = (slice(max(fy_.min() - 6, 0), fy_.max() + 7), slice(max(fx_.min() - 6, 0), fx_.max() + 7))
Mflow_w = Mflow[FW]

def harmonic(F, hole, iters=150):
    G = F.copy(); G[hole] = F[~hole].mean()
    for _ in range(iters):
        Pd = np.pad(G, 1, mode="edge")
        G[hole] = ((Pd[:-2, 1:-1] + Pd[2:, 1:-1] + Pd[1:-1, :-2] + Pd[1:-1, 2:]) / 4)[hole]
    return G

if op >= 0:
    # no usable inversion under an opaque mark: each frame's own estimate is a harmonic fill from its surroundings
    # (sane values for flow + a weak fallback; clean pixels from other frames win wherever they exist)
    hole_y, hole_c = Ay > 0.5, Ac > 0.3
    Ry = np.array([harmonic(f, hole_y) for f in Jy]); Ru = np.array([harmonic(f, hole_c) for f in Ju])
    Rv = np.array([harmonic(f, hole_c) for f in Jv])
    if args.guide:
        # the guide is the same clip, frame for frame (Flow's 1080p only drops the last 2 frames), at lower resolution.
        # Its cleaned pixels are REAL content; upscale them onto this frame's grid (pixel-centre convention).
        gs_ = next(x for x in json.loads(subprocess.check_output(["ffprobe", "-v", "error", "-show_streams", "-of", "json",
                                                                    args.guide]))["streams"] if x["codec_type"] == "video")
        GW, GH = gs_["width"], gs_["height"]; sg = W / GW; GFS = GW * GH * 3 // 2
        X0, Y0 = cx0 + rx0, cy0 + ry0
        gx0 = max(0, int((X0 + 0.5) / sg) - 4) & ~1; gy0 = max(0, int((Y0 + 0.5) / sg) - 4) & ~1
        gx1 = min(GW, int((X0 + RW) / sg) + 6); gy1 = min(GH, int((Y0 + RH) / sg) + 6)
        gp = subprocess.Popen(["ffmpeg", "-v", "error", "-i", args.guide, "-f", "rawvideo", "-pix_fmt", "yuv420p", "-"], stdout=subprocess.PIPE)
        GY, GU, GV = [], [], []
        while len(GY) < N:
            b = gp.stdout.read(GFS)
            if len(b) < GFS: break
            f = np.frombuffer(b, np.uint8)
            GY.append(f[:GW * GH].reshape(GH, GW)[gy0:gy1, gx0:gx1].astype(np.float32))
            GU.append(f[GW * GH:GW * GH * 5 // 4].reshape(GH // 2, GW // 2)[gy0 // 2:gy1 // 2, gx0 // 2:gx1 // 2].astype(np.float32))
            GV.append(f[GW * GH * 5 // 4:].reshape(GH // 2, GW // 2)[gy0 // 2:gy1 // 2, gx0 // 2:gx1 // 2].astype(np.float32))
        gp.kill()
        my_, mx_ = np.mgrid[0:RH, 0:RW].astype(np.float32)
        mapx, mapy = (X0 + mx_ + 0.5) / sg - 0.5 - gx0, (Y0 + my_ + 0.5) / sg - 0.5 - gy0
        cy_, cx_ = np.mgrid[0:RH // 2, 0:RW // 2].astype(np.float32)
        cmapx, cmapy = (X0 // 2 + cx_ + 0.5) / sg - 0.5 - gx0 // 2, (Y0 // 2 + cy_ + 0.5) / sg - 0.5 - gy0 // 2
        # brightness match per frame on a thin ring just outside the strokes (guide went through another encode)
        rg_y = cv2.dilate(hole_y.astype(np.uint8), np.ones((7, 7), np.uint8)).astype(bool) & ~cv2.dilate(
            (Ay > 0.01).astype(np.uint8), np.ones((3, 3), np.uint8)).astype(bool)
        rg_c = rg_y[::2, ::2] & ~hole_c
        for t in range(N):
            k = min(t, len(GY) - 1)
            for R_, J_, G_, h_, rg, mxy in ((Ry, Jy, GY, hole_y, rg_y, (mapx, mapy)), (Ru, Ju, GU, hole_c, rg_c, (cmapx, cmapy)),
                                           (Rv, Jv, GV, hole_c, rg_c, (cmapx, cmapy))):
                g = cv2.remap(G_[k], *mxy, cv2.INTER_CUBIC, borderMode=cv2.BORDER_REPLICATE)
                R_[t][h_] = (g + np.median(J_[t][rg] - g[rg]))[h_]
        # guide-filled strokes count as a half-weight clean observation: native clean pixels from other frames (moving
        # shots) still win, and the veto checks them against the guide
        Ay, Ac = (0.29 * hole_y).astype(np.float32), (0.29 * hole_c).astype(np.float32)
        log(f"guide {GW}x{GH} ({len(GY)} frames, x{sg:.2f}) fills the strokes")

def tv_refine(mu, w, mask, lam, iters=300, eps=1.0):
    """MAP refinement: min sum w (I-mu)^2 + lam*TV_eps(I) over mask pixels (gradient descent).
    w = per-pixel data confidence; where only noisy inversions exist (high alpha, static) the edge-sparsity prior
    suppresses the codec's glyph-shaped edges, where clean candidates exist the data keeps real detail."""
    I = mu.copy()
    # per-pixel (Jacobi-preconditioned) step at the explicit-scheme stability bound: w reaches ~48 next to the mark in
    # static shots (a single global step made lam=4 diverge there) while glyph pixels have w~0.1 and need big steps
    tau = 0.9 / (2 * w + 8 * lam / eps)
    for _ in range(iters):
        gx = np.diff(I, axis=1, append=I[:, -1:]); gy = np.diff(I, axis=0, append=I[-1:, :])
        n = np.sqrt(gx ** 2 + gy ** 2 + eps ** 2)
        px, py = gx / n, gy / n
        div = px - np.roll(px, 1, axis=1) + py - np.roll(py, 1, axis=0)
        I[mask] -= (tau * (2 * w * (I - mu) - lam * div))[mask]
    return I

def shape_basis(a):
    b = [a, np.hypot(cv2.Sobel(a, cv2.CV_32F, 1, 0), cv2.Sobel(a, cv2.CV_32F, 0, 1)), cv2.Laplacian(a, cv2.CV_32F),
         cv2.GaussianBlur(a, (0, 0), 1.0) - a, cv2.GaussianBlur(a, (0, 0), 2.0) - a]
    return np.stack([x / (np.abs(x).max() + 1e-6) for x in b], -1)

dis = cv2.DISOpticalFlow_create(cv2.DISOPTICAL_FLOW_PRESET_MEDIUM)
R8 = np.clip(Ry, 0, 255).astype(np.uint8)
def flow(a, b):
    F = dis.calc(R8[a], R8[b], None)
    for k in range(2):
        F[..., k][FW] = harmonic(F[..., k][FW], Mflow_w)
    return F

_ga = np.hypot(cv2.Sobel(Ay, cv2.CV_32F, 1, 0), cv2.Sobel(Ay, cv2.CV_32F, 0, 1))
EDGE = cv2.dilate((_ga > 0.25 * _ga.max()).astype(np.uint8), np.ones((3, 3), np.uint8)).astype(bool)
INTERP = cv2.INTER_LANCZOS4 if args.interp == "lanczos" else cv2.INTER_CUBIC
gy, gx = np.mgrid[0:RH, 0:RW].astype(np.float32)
Oy, Ou, Ov = Ry.copy(), Ru.copy(), Rv.copy()
ring = cv2.dilate(Mflow.astype(np.uint8), np.ones((9, 9), np.uint8)).astype(bool) & ~Mflow
for (s0, s1) in shots:
    L_ = s1 - s0
    # how much does the background move under the mark in this shot? (median ring flow, a few samples)
    probes = [(t, min(t + 2, s1 - 1)) for t in range(s0, s1 - 2, max(1, L_ // 6))][:6]
    motion = float(np.median([np.median(np.hypot(*flow(a, b)[ring].T)) for a, b in probes])) if probes else 0.0
    if motion > 0.3 and args.reach:
        # research flag: reach far enough that the scene slides fully out from under the mark (dense +-K, then every
        # 3rd frame). Measured worse on ground truth (long-gap flow is unreliable), so off by default.
        reach = int(min(45, np.ceil(1.3 * max(tw, th) / motion)))
        offs = sorted(set(range(-args.K, args.K + 1)) | set(range(-reach, reach + 1, 3)))
        K = max(args.K, reach)
    elif motion > 0.3:
        K = args.K; offs = list(range(-K, K + 1))
    else:
        # static: nothing ever slides out; average many inversions (and let TV-MAP handle the codec ghost)
        K = min(3 * args.K, L_)
        offs = list(range(-K, K + 1))
        if op >= 0 and args.guide:
            # the guide already did the temporal work; mixing neighbours of a static shot erased a foot stepping
            # through the mark (majority = empty pavement) -> use this frame's guide fill only
            offs = [0]
    # GT grid v4: TV-MAP wins on static/slow shots (<=0.8 px/frame), hurts textured moving ones (>=1.3 px/frame)
    lam = args.lam if args.lam >= 0 else 16.0 * float(np.clip((1.2 - motion) / 0.6, 0, 1))
    clean_w = tot_w = 0.0
    for t in range(s0, s1):
        cy, cu, cv_, ws, cl = [Ry[t]], [Ru[t]], [Rv[t]], [(1 - Ay) ** 2], [Ay < 0.02]
        for o in offs:
            s = t + o
            if o == 0 or s < s0 or s >= s1: continue
            Fts, Fst = flow(t, s), flow(s, t)
            px, py = gx + Fts[..., 0], gy + Fts[..., 1]
            back = np.stack([cv2.remap(Fst[..., k], px, py, cv2.INTER_LINEAR, borderMode=cv2.BORDER_REPLICATE) for k in range(2)], -1)
            fb = np.hypot(Fts[..., 0] + back[..., 0], Fts[..., 1] + back[..., 1])
            inb = (px >= 0) & (px <= RW - 1) & (py >= 0) & (py <= RH - 1)
            a_src = cv2.remap(Ay, px, py, cv2.INTER_LINEAR, borderMode=cv2.BORDER_CONSTANT, borderValue=0)
            w = (1 - a_src) ** 2 * np.exp(-(fb / 0.7) ** 2) * inb * np.exp(-abs(s - t) / (2.0 * K))
            cy.append(cv2.remap(Ry[s], px, py, INTERP, borderMode=cv2.BORDER_REPLICATE))
            hx, hy = px[::2, ::2] / 2, py[::2, ::2] / 2
            cu.append(cv2.remap(Ru[s], hx, hy, cv2.INTER_LINEAR, borderMode=cv2.BORDER_REPLICATE))
            cv_.append(cv2.remap(Rv[s], hx, hy, cv2.INTER_LINEAR, borderMode=cv2.BORDER_REPLICATE))
            ws.append(w); cl.append(a_src < 0.02)
            core = Ay > 0.05   # count only pixels that actually carry the mark
            clean_w += float(((a_src < 0.02) * w)[core].sum()); tot_w += float(w[core].sum())
        C, Wt = np.array(cy), np.array(ws)
        # anchor: a candidate must agree with THIS frame's own inversion (noisy, but the right content at the right
        # place) within ~3 sigma of that inversion's known noise. Rejects wrong-place pixels that a failed flow
        # imports consistently in both directions (yellow dots from a yellow stripe, cartoon at 20 px/frame).
        # tolerance must exceed the own inversion's STATIC codec error (~20 levels at a=0.5 — a tight anchor pulled the
        # result back onto it: ghost 2.9 -> 15.9 on the flower GT), yet stay far below wrong-place content (~80 levels).
        # Quartic fall-off: flat inside the tolerance, steep outside.
        tol_y = 25.0 + 25.0 * Ay / (1 - Ay)   # a gross-outlier veto, not a pull toward the noisy own inversion
        anc = np.exp(-((C - Ry[t]) / tol_y) ** 4); anc[0] = 1.0
        Wt = Wt * anc
        mu = (Wt * C).sum(0) / (Wt.sum(0) + 1e-6)
        Wr = Wt
        for _ in range(2):
            dev = np.abs(C - mu); sc = np.median(dev, 0) * 1.4826 + 2.0
            Wr = Wt * np.minimum(1.0, 1.5 * sc / (dev + 1e-6))
            mu = (Wr * C).sum(0) / (Wr.sum(0) + 1e-6)
        # per-pixel data confidence for the TV-MAP step:
        #   clean candidates are independent observations -> their weights ADD;
        #   inversions of a static point share one codec error -> together they count as ONE observation
        conf = (Wr * np.array(cl)).sum(0) + 1.0 / (1.0 + 9.0 * (Ay / (1 - Ay)) ** 2)   # (sigma_codec/sigma_0)^2 = 9
        lam_px = lam
        clean_sum = (Wr * np.array(cl)).sum(0) - (Ay < 0.02) * Wr[0]
        if op >= 0:
            # opaque: TV only on stroke pixels nothing clean landed on (never smooth the real pixels between strokes);
            # with a guide the strokes hold real content -> no TV at all
            lam_px = (0.0 if args.guide else 16.0) * (hole_y & (clean_sum < 0.5)).astype(np.float32)
        elif args.edge_tv:
            # thin ring along the mark's edges, only where no clean observation landed (fast motion): the codec smears
            # the overlay's edge each frame differently, leaving a hairline outline after inversion
            lam_px = np.maximum(lam, 16.0 * (EDGE & (clean_sum < 0.5))).astype(np.float32)
        Oy[t] = tv_refine(mu, conf, M, lam_px) if np.max(lam_px) > 0 else np.where(M, mu, Oy[t])
        # chroma: same geometry weights, but its OWN anchor and robust step (colour errors are invisible to luma)
        Wg, Mc, confc = Wt[:, ::2, ::2], M[::2, ::2], conf[::2, ::2]
        tol_c = 20.0 + 20.0 * Ac / (1 - Ac)
        for O, cands, Rc in ((Ou, cu, Ru), (Ov, cv_, Rv)):
            Cc = np.array(cands)
            Wc = Wg * np.exp(-((Cc - Rc[t]) / tol_c) ** 4); Wc[0] = Wg[0]
            muc = (Wc * Cc).sum(0) / (Wc.sum(0) + 1e-6)
            for _ in range(2):
                dev = np.abs(Cc - muc); sc = np.median(dev, 0) * 1.4826 + 1.5
                Wcr = Wc * np.minimum(1.0, 1.5 * sc / (dev + 1e-6))
                muc = (Wcr * Cc).sum(0) / (Wcr.sum(0) + 1e-6)
            lam_c = lam_px[::2, ::2] if op >= 0 else lam   # opaque: no inversion to fall back on
            O[t] = tv_refine(muc, confc, Mc, lam_c) if np.max(lam_c) > 0 else np.where(Mc, muc, O[t])
    share = clean_w / max(tot_w, 1e-6)
    # static leftover (codec ghost) — only matters when the shot relied on inversions, i.e. little clean data
    if share < 0.5 and L_ >= 8 and args.deghost:
        for O, a, Mk in ((Oy, Ay, M), (Ou, Ac, M[::2, ::2]), (Ov, Ac, M[::2, ::2])):
            seg = O[s0:s1]
            g = (seg - np.array([harmonic(x, Mk) for x in seg])).mean(0) * Mk
            B = shape_basis(a)[Mk]
            c, *_ = np.linalg.lstsq(B, g[Mk], rcond=None)
            gs = np.zeros_like(g); gs[Mk] = B @ c
            O[s0:s1] -= (1 - share) * gs
    log(f"shot {s0}-{s1}: motion={motion:.2f}px K={K} lambda={lam:.1f} clean-share={share:.2f}")

# feathered paste mask (inside M: rebuilt, outside: untouched source)
feather = np.clip(cv2.GaussianBlur(M.astype(np.float32), (0, 0), 1.0) * 1.5, 0, 1)
feather_c = feather[::2, ::2]

# ---------------------------------------------------------------- pass 2: encode
ext = os.path.splitext(args.out)[1].lower()
codec = args.codec
if codec == "auto":
    have = subprocess.run(["ffmpeg", "-hide_banner", "-encoders"], capture_output=True, text=True).stdout
    codec = "prores" if ext == ".mov" else ("h264x" if "libx264" in have else "av1")
venc = {
    "prores": ["-c:v", "prores_ks", "-profile:v", "3", "-pix_fmt", "yuv422p10le", "-vendor", "apl0"],   # ProRes 422 HQ
    "av1": ["-c:v", "libsvtav1", "-crf", "14", "-preset", "5", "-svtav1-params", "tune=0", "-pix_fmt", "yuv420p10le"],
    "h264x": ["-c:v", "libx264", "-crf", "12", "-preset", "slow", "-pix_fmt", "yuv420p"],
    "h264": ["-c:v", "libopenh264", "-b:v", str(max(3 * src_bitrate, 12_000_000)), "-pix_fmt", "yuv420p"],
    "ffv1": ["-c:v", "ffv1", "-level", "3", "-pix_fmt", "yuv420p"],
}[codec]
tags = []
for k, opt in (("color_range", "-color_range"), ("color_space", "-colorspace"), ("color_primaries", "-color_primaries"), ("color_transfer", "-color_trc")):
    if color[k] and color[k] != "unknown": tags += [opt, color[k]]
if ".crop." in args.out:   # research: only the bottom-right 256x128, lossless, for the ground-truth scorer
    venc = ["-vf", f"crop=384:256:{W - 384}:{H - 256}", "-c:v", "ffv1", "-pix_fmt", "yuv420p"]; codec = "ffv1-crop"
cmd = ["ffmpeg", "-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "yuv420p", "-s", f"{W}x{H}", "-r", FPS, *tags[:2], "-i", "-",
       "-i", args.inp, "-map", "0:v", "-map", "1:a?", "-c:a", "copy", *venc, *tags, args.out]
enc = subprocess.Popen(cmd, stdin=subprocess.PIPE)
Y0, X0 = cy0 + ry0, cx0 + rx0
for t, (y, u, v) in enumerate(decode()):
    y, u, v = y.copy(), u.copy(), v.copy()
    sy = y[Y0:Y0 + RH, X0:X0 + RW].astype(np.float32)
    y[Y0:Y0 + RH, X0:X0 + RW] = np.clip(feather * Oy[t] + (1 - feather) * sy + 0.5, 0, 255)
    for P_, O in ((u, Ou), (v, Ov)):
        sl = (slice(Y0 // 2, Y0 // 2 + RH // 2), slice(X0 // 2, X0 // 2 + RW // 2))
        P_[sl] = np.clip(feather_c * O[t] + (1 - feather_c) * P_[sl].astype(np.float32) + 0.5, 0, 255)
    enc.stdin.write(y.tobytes() + u.tobytes() + v.tobytes())
enc.stdin.close(); enc.wait()
log(f"done -> {args.out} ({codec}), {os.path.getsize(args.out) / 1e6:.1f} MB")
