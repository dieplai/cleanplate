"""Experiment 1 — blind multi-frame matting of a static overlay, in native YUV planes.

usage: python3 blind_matte.py in.mp4 out.mkv [--fuse]

Idea: a watermark is a static overlay J_t = (1-a) I_t + a L on a moving background.
For each pixel, with I_hat_t = background guessed from the surrounding ring,
    y_t = J_t - I_hat_t = a*L - a*I_hat_t      (linear in unknowns aL and a)
Hundreds of frames -> robust per-pixel regression gives a and L for THIS video,
no template needed. Then invert: I_t = (J_t - aL) / (1 - a).
Memory: keeps only a small crop around the mark for all frames (KBs per frame).
"""
import sys, subprocess, json
import numpy as np, cv2

inp, outp = sys.argv[1], sys.argv[2]
FUSE = "--fuse" in sys.argv

def probe(p):
    s = json.loads(subprocess.check_output(["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries",
        "stream=width,height,r_frame_rate", "-of", "json", p]))["streams"][0]
    n, d = s["r_frame_rate"].split("/")
    return s["width"], s["height"], s["r_frame_rate"]

W, H, FPS = probe(inp)
FS = W * H * 3 // 2  # yuv420p frame bytes

def frames(p):
    f = subprocess.Popen(["ffmpeg", "-v", "error", "-i", p, "-f", "rawvideo", "-pix_fmt", "yuv420p", "-"], stdout=subprocess.PIPE)
    while True:
        b = f.stdout.read(FS)
        if len(b) < FS: break
        a = np.frombuffer(b, np.uint8)
        y = a[:W * H].reshape(H, W)
        u = a[W * H:W * H * 5 // 4].reshape(H // 2, W // 2)
        v = a[W * H * 5 // 4:].reshape(H // 2, W // 2)
        yield y, u, v
    f.wait()

# ---- pass 1: collect bottom-right luma crop to discover the static overlay
CX, CY = W * 3 // 4, H * 3 // 4          # search window: bottom-right quarter
Ys = np.array([y[CY:, CX:].astype(np.float32) for y, _, _ in frames(inp)])
N = len(Ys)
hp = (Ys - np.array([cv2.GaussianBlur(f, (0, 0), 3) for f in Ys])).mean(0)
# static structure survives temporal averaging; moving texture cancels
sig = np.median(np.abs(hp)) * 1.4826 + 1e-6
m = (np.abs(hp) > 6 * sig).astype(np.uint8)
n_lbl, lbl, st, _ = cv2.connectedComponentsWithStats(m)
# keep components clustered near the strongest one (a logo can be several glyphs)
order = np.argsort(-st[1:, cv2.CC_STAT_AREA]) + 1
x0, y0, w0, h0 = st[order[0], :4]
keep = [i for i in range(1, n_lbl) if st[i, 4] >= 2 and
        abs((st[i, 0] + st[i, 2] / 2) - (x0 + w0 / 2)) < 60 and abs((st[i, 1] + st[i, 3] / 2) - (y0 + h0 / 2)) < 30]
sup = np.isin(lbl, keep)
ys_, xs_ = np.where(sup)
bx0, by0, bx1, by1 = xs_.min() + CX, ys_.min() + CY, xs_.max() + 1 + CX, ys_.max() + 1 + CY
print(f"{W}x{H} N={N} discovered mark bbox x={bx0} y={by0} w={bx1-bx0} h={by1-by0}")

# working region: bbox + margin, even-aligned for chroma
PAD = 8
RX0, RY0 = (bx0 - PAD) & ~1, (by0 - PAD) & ~1
RX1, RY1 = min(W, (bx1 + PAD + 1) & ~1), min(H, (by1 + PAD + 1) & ~1)
del Ys

crops = {"y": [], "u": [], "v": []}
for y, u, v in frames(inp):
    crops["y"].append(y[RY0:RY1, RX0:RX1].astype(np.float32))
    crops["u"].append(u[RY0 // 2:RY1 // 2, RX0 // 2:RX1 // 2].astype(np.float32))
    crops["v"].append(v[RY0 // 2:RY1 // 2, RX0 // 2:RX1 // 2].astype(np.float32))
crops = {k: np.array(v) for k, v in crops.items()}

def harmonic_fill(F, hole, iters=400):
    """Laplace-fill hole pixels from their surroundings, batched over frames."""
    G = F.copy()
    G[:, hole] = F[:, ~hole].mean(1, keepdims=True)
    for _ in range(iters):
        P = np.pad(G, ((0, 0), (1, 1), (1, 1)), mode="edge")
        avg = (P[:, :-2, 1:-1] + P[:, 2:, 1:-1] + P[:, 1:-1, :-2] + P[:, 1:-1, 2:]) / 4
        G[:, hole] = avg[:, hole]
    return G

def solve_alpha(J, Ih, L, support):
    """Per-pixel robust 1-unknown fit  y_t = a (L - x_t)  with y = J - I_hat, x = I_hat (L shared by the whole mark)."""
    y, d = J - Ih, L - Ih
    w = np.ones_like(J)
    for _ in range(6):  # IRLS/Huber: frames where the ring guess is wrong get down-weighted
        a = np.clip((w * y * d).sum(0) / ((w * d * d).sum(0) + 1e-6), 0, 0.95)
        r = y - a * d
        s = np.median(np.abs(r), 0) * 1.4826 + 1.0
        w = np.minimum(1.0, 1.5 * s / (np.abs(r) + 1e-6))
    a[~support] = 0
    return a

def solve_luma(J, Ih, support):
    """Per-pixel robust 2-unknown fit y_t = aL - a x_t. The Veo mark is white text + a dark drop shadow,
    so L differs per pixel (~255 on glyphs, ~0 in the halo) — one shared L over-subtracts."""
    y, x = J - Ih, Ih
    w = np.ones_like(J)
    for _ in range(6):
        sw, sx, sy = w.sum(0), (w * x).sum(0), (w * y).sum(0)
        sxx, sxy = (w * x * x).sum(0), (w * x * y).sum(0)
        slope = (sw * sxy - sx * sy) / (sw * sxx - sx * sx + 1e-6)
        aL = (sy - slope * sx) / sw
        a = np.clip(-slope, 0, 0.95)
        r = y - (aL - a * x)
        s = np.median(np.abs(r), 0) * 1.4826 + 1.0
        w = np.minimum(1.0, 1.5 * s / (np.abs(r) + 1e-6))
    keep = support & (a > 0.02)
    a[~keep] = 0; aL[~keep] = 0
    return a, aL

def estimate_L(J, Ih, support):
    """Shared logo value for the luma plane: per-pixel 2-unknown regression, then median over strong pixels."""
    y, x = J - Ih, Ih
    sx, sy, sxx, sxy, n = x.sum(0), y.sum(0), (x * x).sum(0), (x * y).sum(0), len(J)
    slope = (n * sxy - sx * sy) / (n * sxx - sx * sx + 1e-6)
    a, aL = -slope, (sy - slope * sx) / n
    strong = support & (a > 0.25)
    return float(np.median(aL[strong] / a[strong])) if strong.any() else 255.0

hole_y = np.zeros(crops["y"].shape[1:], bool)
hole_y[by0 - RY0 - 2:by1 - RY0 + 2, bx0 - RX0 - 2:bx1 - RX0 + 2] = True
# chroma is 2x subsampled: the mark bleeds a bit wider there, so grow the hole by one chroma pixel
hc, wc = crops["u"].shape[1:]
hole_c = hole_y.reshape(hc, 2, wc, 2).any(axis=(1, 3))
hole_c = cv2.dilate(hole_c.astype(np.uint8), np.ones((3, 3), np.uint8)).astype(bool)

# alpha support: discovered glyph pixels + 1px (anti-aliased edges); everything else in the hole is alpha=0
sup_crop = np.zeros_like(hole_y)
sup_crop[ys_ + CY - RY0, xs_ + CX - RX0] = True
sup_crop = cv2.dilate(sup_crop.astype(np.uint8), np.ones((7, 7), np.uint8)).astype(bool) & hole_y  # cover the soft shadow halo

Jy = crops["y"]; Ihy = harmonic_fill(Jy, hole_y)
A0, _ = solve_luma(Jy, Ihy, sup_crop)           # shape of the mark, discovered from this video

def edge_energy(g, L, A):
    """Gradient-prior cost: a wrong alpha leaves the SAME glyph-shaped edges in every frame,
    real background edges change frame to frame — so minimise robust edge energy summed over time."""
    a = np.clip(g * A, 0, 0.95)
    I = (Jy - a * L) / (1 - a)
    gx, gy = np.diff(I, axis=2)[:, :-1, :], np.diff(I, axis=1)[:, :, :-1]
    m = sup_crop[:-1, :-1]
    return np.sqrt(gx[:, m] ** 2 + gy[:, m] ** 2 + 1.0).mean()

if "--prior" in sys.argv:
    best = min((edge_energy(g, L, A0), g, L) for g in np.arange(0.5, 1.51, 0.05) for L in np.arange(200, 301, 5))
    _, g0, L0 = best
    best = min((edge_energy(g, L, A0), g, L) for g in np.arange(g0 - 0.05, g0 + 0.051, 0.01) for L in np.arange(L0 - 5, L0 + 5.1, 1))
    _, g, L = best
    print(f"gradient-prior fit: gain={g:.2f} L={L:.0f}")
    a_y = np.clip(g * A0, 0, 0.95); aL_y = a_y * L
else:
    a_y, aL_y = solve_luma(Jy, Ihy, sup_crop)
# chroma: same physical alpha, seen through 4:2:0 subsampling (2x2 average); an achromatic mark (white text,
# black shadow) pulls U,V toward 128 whatever its luma, so chroma needs no L estimate of its own
a_c = a_y.reshape(hc, 2, wc, 2).mean(axis=(1, 3))
L_map = np.where(a_y > 0.05, aL_y / np.maximum(a_y, 1e-3), np.nan)
print(f"max alpha_y={a_y.max():.3f}  max alpha_c={a_c.max():.3f}  L range(a>0.1)={np.nanpercentile(L_map[a_y > 0.1], [5, 50, 95]).round(1)}")
np.save(outp + ".alpha_y.npy", a_y)

res = {}
for k, J, a, aL, Ih in (("y", Jy, a_y, aL_y, Ihy),
                        ("u", crops["u"], a_c, a_c * 128.0, None), ("v", crops["v"], a_c, a_c * 128.0, None)):
    I = (J - aL) / (1 - a)
    if FUSE:
        Ih = Ih if Ih is not None else harmonic_fill(J, hole_c)
        # noise-aware fusion: inversion amplifies codec noise by 1/(1-a); lean on the smooth guess where a is large
        wi = (1 - a) ** 2 / ((1 - a) ** 2 + 0.15 * a)
        I = wi * I + (1 - wi) * Ih
    res[k] = I

# ---- pass 2: stream again, patch region, encode lossless (isolates removal quality from codec choice)
enc = subprocess.Popen(["ffmpeg", "-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "yuv420p", "-s", f"{W}x{H}", "-r", FPS,
                        "-i", "-", "-i", inp, "-map", "0:v", "-map", "1:a?", "-c:a", "copy", "-c:v", "ffv1", outp],
                       stdin=subprocess.PIPE)
for t, (y, u, v) in enumerate(frames(inp)):
    y, u, v = y.copy(), u.copy(), v.copy()
    y[RY0:RY1, RX0:RX1] = np.clip(res["y"][t] + 0.5, 0, 255)
    u[RY0 // 2:RY1 // 2, RX0 // 2:RX1 // 2] = np.clip(res["u"][t] + 0.5, 0, 255)
    v[RY0 // 2:RY1 // 2, RX0 // 2:RX1 // 2] = np.clip(res["v"][t] + 0.5, 0, 255)
    enc.stdin.write(y.tobytes() + u.tobytes() + v.tobytes())
enc.stdin.close(); enc.wait()
print("->", outp)
