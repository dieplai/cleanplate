#!/usr/bin/env python3
"""cleanplate for still images — remove a visible generator mark (Gemini diamond) from one image.

  detect     high-pass NCC of each mark SHAPE over a range of sizes in the bottom-right corner. Generators place
             the mark at a margin of 2/3 or 2x its size, so "known placement" is scale-free and survives a resized
             copy. Low confidence -> refuse, image untouched.
  calibrate  never trust a template's opacity: Gemini has shipped the same diamond at alpha 0.30 and 0.50. The
             template gives the SHAPE; the opacity g and a sub-pixel offset are fitted on the image itself so that
             the mark's edge disappears: across a thin ring on both sides of the edge, the restored image must
             not step (zero-crossing per angular sector; unbiased, unlike minimising energy/TV).
  invert     I = (J - a*255) / (1 - a) per RGB channel (stills are full-range RGB; the mark is white).
  refine     only for lossy inputs (JPEG/WebP): the codec's error is amplified by 1/(1-a); a weighted-TV MAP
             step on the mark (data weight ~ (1-a)^2) removes the mark-shaped ringing, PNG is left exact.

usage: image.py in.(png|jpg|webp) out.(png|jpg|webp) [--marks DIR] [--force-shape NAME]
"""
import sys, os, glob, argparse
import numpy as np, cv2

ap = argparse.ArgumentParser()
ap.add_argument("inp"); ap.add_argument("out")
ap.add_argument("--marks", default=os.path.join(os.path.dirname(__file__), "..", "assets", "marks", "google"))
ap.add_argument("--min-ncc", type=float, default=0.3, help="detection floor at a known placement (0.6 elsewhere)")
ap.add_argument("--lam", type=float, default=-1, help="TV strength for lossy inputs; -1 = auto (0 for PNG)")
args = ap.parse_args()

raw = cv2.imread(args.inp, cv2.IMREAD_UNCHANGED)
if raw is None: sys.exit(f"cannot read {args.inp}")
alpha_ch = raw[..., 3] if raw.ndim == 3 and raw.shape[2] == 4 else None
J = (raw[..., :3] if raw.ndim == 3 else cv2.cvtColor(raw, cv2.COLOR_GRAY2BGR)).astype(np.float32)
H, W = J.shape[:2]
lossy = os.path.splitext(args.inp)[1].lower() in (".jpg", ".jpeg", ".webp")

def trim(A):
    ys, xs = np.where(A > 0.01); return A[ys.min():ys.max() + 1, xs.min():xs.max() + 1]

# shapes: normalised to plateau 1 (median of the core), so opacity is fitted, not assumed
shapes, native, nsize = {}, {}, {}
for f in sorted(glob.glob(os.path.join(args.marks, "gemini-diamond*.png"))):
    A = trim(cv2.imread(f, cv2.IMREAD_GRAYSCALE).astype(np.float32) / 255.0)
    k = os.path.basename(f); native[k] = float(np.median(A[A > 0.5 * A.max()])); shapes[k] = A / native[k]
    nsize[k] = max(A.shape)

# ---------------------------------------------------------------- detect
CS = min(H, W, max(320, min(H, W) // 3))
cx0, cy0 = W - CS, H - CS
g = cv2.cvtColor(J[cy0:, cx0:], cv2.COLOR_BGR2GRAY)
hp = g - cv2.GaussianBlur(g, (0, 0), 3)
# margin / mark size at which generators stamp: Gemini stills 2/3, Gemini video 2, Flow (Nano Banana) stills ~1.6
# (48 px at 73-78 px on 1024x1024, 1376x768, 1200x896, 768x1376)
PLACE = (2 / 3, 1.6, 2.0)
best = None
for name, S in shapes.items():
    for size in range(24, min(200, CS // 2), 2):
        A = cv2.resize(S, (size, size), interpolation=cv2.INTER_AREA if size < S.shape[0] else cv2.INTER_CUBIC)
        t = A - cv2.GaussianBlur(A, (0, 0), 3)
        r = cv2.matchTemplate(hp, t, cv2.TM_CCOEFF_NORMED)
        _, sc, _, (mx, my) = cv2.minMaxLoc(r)
        mr, mb = CS - mx - size, CS - my - size
        known = any(abs(mr - k * size) <= max(3, 0.1 * size) and abs(mb - k * size) <= max(3, 0.1 * size) for k in PLACE)
        key = sc + (0.15 if known else 0)          # prefer the generator's placement among near-equal peaks
        if best is None or key > best[0]:
            best = (key, sc, name, size, mx + cx0, my + cy0, known)
        for k in PLACE:                             # the global peak can miss a faint mark: probe the placements
            px, py = CS - size - round(k * size), CS - size - round(k * size)
            if 0 <= px < r.shape[1] and 0 <= py < r.shape[0]:
                v = float(r[py - 1:py + 2, px - 1:px + 2].max()) if py > 0 and px > 0 else float(r[py, px])
                if v + 0.15 > best[0]:
                    yy_, xx_ = np.unravel_index(np.argmax(r[max(py - 1, 0):py + 2, max(px - 1, 0):px + 2]), (3, 3))
                    best = (v + 0.15, v, name, size, max(px - 1, 0) + xx_ + cx0, max(py - 1, 0) + yy_ + cy0, True)
_, ncc, mname, msize, mx, my, known = best
floor_ = args.min_ncc if known else 0.6
print(f"detected {mname} {msize}px at ({mx},{my}) ncc={ncc:.3f}{' (known placement)' if known else ''}, floor {floor_}")
if ncc < floor_:
    sys.exit(f"no mark found with confidence (ncc {ncc:.2f} < {floor_}) — image left untouched")

# ---------------------------------------------------------------- calibrate (opacity + sub-pixel offset)
P = 6
x0, y0, x1, y1 = max(0, mx - P), max(0, my - P), min(W, mx + msize + P), min(H, my + msize + P)
j = J[y0:y1, x0:x1]
def shape_at(name, dx, dy, ds=0.0):
    """shape rendered at size msize+ds (a resized copy has a fractional mark size) and sub-pixel offset (dx, dy)"""
    S = cv2.resize(shapes[name], (msize, msize), interpolation=cv2.INTER_AREA if msize < shapes[name].shape[0] else cv2.INTER_CUBIC)
    Sp = np.zeros(j.shape[:2], np.float32); Sp[my - y0:my - y0 + msize, mx - x0:mx - x0 + msize] = S
    k = (msize + ds) / msize; ox, oy = mx - x0, my - y0          # scale about the mark's top-left corner
    return cv2.warpAffine(Sp, np.float32([[k, 0, ox * (1 - k) + dx], [0, k, oy * (1 - k) + dy]]),
                          (Sp.shape[1], Sp.shape[0]), flags=cv2.INTER_LINEAR)
def restore(a):
    return (j - a[..., None] * 255.0) / np.maximum(1 - a[..., None], 0.05)
def grad(X):
    return cv2.Sobel(X, cv2.CV_32F, 1, 0, ksize=3), cv2.Sobel(X, cv2.CV_32F, 0, 1, ksize=3)
def fit(S):
    """opacity = zero-crossing of the signed correlation between the restored image's gradient and the mark's
    gradient (a left-over mark is edge-aligned with it: + if under-removed, - if over-removed; texture averages out)"""
    sx, sy = grad(S); band = np.hypot(sx, sy) > 0.1 * np.hypot(sx, sy).max()
    def corr(g):
        I = restore(g * S); c = 0.0
        for k in range(3):
            ix, iy = grad(np.ascontiguousarray(I[..., k])); c += float((ix * sx + iy * sy)[band].sum())
        return c
    lo, hi = 0.05, 0.9
    for _ in range(22):
        m = (lo + hi) / 2
        if corr(m) > 0: lo = m
        else: hi = m
    I = restore(m * S); e = 0.0                      # left-over edge energy on the band: picks shape + offset
    for k in range(3):
        ix, iy = grad(np.ascontiguousarray(I[..., k])); e += float((ix ** 2 + iy ** 2)[band].mean())
    return m, e
cands = []
for name in shapes:                                 # the same diamond exists in several renderings
    for dy in np.arange(-1, 1.01, 0.5):
        for dx in np.arange(-1, 1.01, 0.5):
            gm, e = fit(shape_at(name, dx, dy)); cands.append((e, name, dx, dy, gm))
# generators render a mark at a fixed opacity: also try each shape at its native level (exact when it is the truth)
def energy(S, g):
    I = restore(g * S); sx, sy = grad(S); band = np.hypot(sx, sy) > 0.1 * np.hypot(sx, sy).max(); e = 0.0
    for k in range(3):
        ix, iy = grad(np.ascontiguousarray(I[..., k])); e += float((ix ** 2 + iy ** 2)[band].mean())
    return e
snapped = []
for e_, name, dx_, dy_, gm in cands:
    if abs(gm - native[name]) < 0.04 * max(1.0, native[name] / 0.3):
        snapped.append((energy(shape_at(name, dx_, dy_), native[name]), name, dx_, dy_, native[name]))
# a shape whose measured opacity matches the level it is rendered at is strong evidence for that shape (on busy
# texture the edge energies of two renderings can tie: synthetic test picked the 0.30 shape at 0.49 otherwise)
cands = sorted(snapped) if snapped else sorted(cands)
# generators render each size from its own artwork: a template captured at the detected size beats a resampled one
exact = [c for c in cands if abs(nsize[c[1]] - msize) <= 1]
cands = exact or cands
e0, sname, cx, cy, gain = cands[0]
dx, dy = cx, cy
for ddy in (-0.25, 0, 0.25):                        # refine the offset around the best
    for ddx in (-0.25, 0, 0.25):
        S_ = shape_at(sname, cx + ddx, cy + ddy)
        for gm, e in (fit(S_), (gain, energy(S_, gain))):
            if e < e0: e0, gain, dx, dy = e, gm, cx + ddx, cy + ddy
ds = 0.0
for dds in ((-1.0, -0.75, -0.5, -0.25, 0.25, 0.5, 0.75, 1.0) if msize not in (36, 48, 96) else ()):
    # fractional size + offset, only for a resized copy (native sizes are exact; texture noise would mislead it)
    for ddy in (-0.5, -0.25, 0, 0.25, 0.5):
        for ddx in (-0.5, -0.25, 0, 0.25, 0.5):
            S_ = shape_at(sname, dx + ddx, dy + ddy, dds)
            for gm, e in (fit(S_), (gain, energy(S_, gain))):
                if e < e0: e0, gain, nx, ny, ds = e, gm, dx + ddx, dy + ddy, dds
if ds != 0.0: dx, dy = nx, ny
other = next((c for c in cands if c[1] != sname), None)
print(f"calibrated: shape {sname}, opacity {gain:.3f}, offset ({dx:+.2f},{dy:+.2f}), edge energy {e0:.1f}"
      + (f" (best other shape {other[1]}: {other[0]:.1f})" if other else ""))
a = np.clip(gain * shape_at(sname, dx, dy, ds), 0, 0.95)
print(f"  size {msize + ds:.2f}px")

# ---------------------------------------------------------------- invert + (lossy only) refine
I = restore(a)
# lossy inputs: TV strength from the texture around the mark (GT: brick wants ~1, a smooth dark neon scene ~6)
ring_ = (cv2.dilate((a > 0.01).astype(np.uint8), np.ones((9, 9), np.uint8)) > 0) & ~(a > 0.01)
gj = cv2.cvtColor(j, cv2.COLOR_BGR2GRAY)
tex = float((np.hypot(cv2.Sobel(gj, cv2.CV_32F, 1, 0), cv2.Sobel(gj, cv2.CV_32F, 0, 1)) / 8)[ring_].mean())
lam = args.lam if args.lam >= 0 else (float(np.clip(6 - 5 * (tex - 3) / 9, 1, 6)) if lossy else 0.0)
print(f"texture {tex:.1f} -> TV lambda {lam:.1f}{' (lossless: exact inversion)' if not lossy else ''}")
if lam > 0:
    w = (1 - a) ** 2 + 0.02                         # data confidence: inversions of strong alpha are noisy
    mask = cv2.dilate((a > 0.01).astype(np.uint8), np.ones((3, 3), np.uint8)) > 0
    for c in range(3):
        u = I[..., c].copy(); mu = u.copy(); tau = 0.9 / (2 * w + 8 * lam)
        for _ in range(200):
            gx = np.diff(u, axis=1, append=u[:, -1:]); gy = np.diff(u, axis=0, append=u[-1:, :])
            n = np.sqrt(gx ** 2 + gy ** 2 + 1.0)
            div = gx / n - np.roll(gx / n, 1, axis=1) + gy / n - np.roll(gy / n, 1, axis=0)
            u[mask] -= (tau * (2 * w * (u - mu) - lam * div))[mask]
        I[..., c] = u
out = J.copy(); out[y0:y1, x0:x1] = I
out = np.clip(out + 0.5, 0, 255).astype(np.uint8)
if alpha_ch is not None: out = np.dstack([out, alpha_ch])
ext = os.path.splitext(args.out)[1].lower()
params = [cv2.IMWRITE_JPEG_QUALITY, 95] if ext in (".jpg", ".jpeg") else [cv2.IMWRITE_WEBP_QUALITY, 95] if ext == ".webp" else []
cv2.imwrite(args.out, out, params)
print(f"done -> {args.out}")
