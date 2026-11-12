"""Template-based remover variants for the research benchmark (lossless FFV1 output to isolate removal quality).

usage: methods.py in.mp4 out.mkv template.png MODE [beta]
  rgb   : invert J=(1-a)I+aL in RGB                       (what wmr/GWT do)
  yuv   : invert in native yuv420p planes; chroma uses 2x2-averaged alpha and L=128 (achromatic mark)
  fuse  : yuv + noise-aware fusion: where 1/(1-a) amplifies codec noise, blend toward a smooth fill
          weight w = 1 / (1 + beta * (a/(1-a))^2)
Mark position: NCC of the template against the time-averaged high-pass of the bottom-right corner.
"""
import sys, subprocess, json, numpy as np, cv2

inp, outp, tpl_p, MODE = sys.argv[1:5]
BETA = float(sys.argv[5]) if len(sys.argv) > 5 and not sys.argv[5].startswith("--") else 1.0
DEGHOST = "--deghost" in sys.argv   # subtract the static (time-averaged) leftover the codec leaves on the overlay edges
s = json.loads(subprocess.check_output(["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries",
                                        "stream=width,height,r_frame_rate", "-of", "json", inp]))["streams"][0]
W, H, FPS = s["width"], s["height"], s["r_frame_rate"]
A = cv2.imread(tpl_p, cv2.IMREAD_GRAYSCALE).astype(np.float32) / 255.0
ys, xs = np.where(A > 0); A = A[ys.min():ys.max() + 1, xs.min():xs.max() + 1]; th, tw = A.shape

def frames(pix):
    fs = W * H * (3 if pix == "rgb24" else 1.5)
    fs = int(fs)
    p = subprocess.Popen(["ffmpeg", "-v", "error", "-i", inp, "-f", "rawvideo", "-pix_fmt", pix, "-"], stdout=subprocess.PIPE)
    while True:
        b = p.stdout.read(fs)
        if len(b) < fs: break
        yield np.frombuffer(b, np.uint8)

# --- locate: integer NCC on time-mean high-pass of the bottom-right corner (luma)
CX, CY = W - 160, H - 100
acc = np.zeros((H - CY, W - CX), np.float64); n = 0
for f in frames("yuv420p"):
    y = f[:W * H].reshape(H, W)[CY:, CX:].astype(np.float32)
    acc += y - cv2.GaussianBlur(y, (0, 0), 3); n += 1
hp = (acc / n).astype(np.float32)
r = cv2.matchTemplate(hp, A - cv2.GaussianBlur(A, (0, 0), 3), cv2.TM_CCOEFF_NORMED)
_, score, _, (bx, by) = cv2.minMaxLoc(r)
mx, my = CX + bx, CY + by
mx -= mx & 1; my -= my & 1  # keep the box chroma-aligned; template is re-placed at exact offset below
ox, oy = CX + bx - mx, CY + by - my
print(f"located mark at ({CX+bx},{CY+by}) ncc={score:.3f}")
if score < 0.5:
    sys.exit("low confidence — refusing to modify (anti-goal: never damage a clip)")

P = 4  # work box with margin, even-aligned
bw, bh = ((tw + ox + 2 * P + 1) & ~1), ((th + oy + 2 * P + 1) & ~1)
X0, Y0 = mx - P, my - P
Afull = np.zeros((bh, bw), np.float32); Afull[P + oy:P + oy + th, P + ox:P + ox + tw] = A
Ac = Afull.reshape(bh // 2, 2, bw // 2, 2).mean(axis=(1, 3))

def harmonic(F, hole, iters=200):
    G = F.copy(); G[hole] = F[~hole].mean()
    for _ in range(iters):
        Pd = np.pad(G, 1, mode="edge")
        G[hole] = ((Pd[:-2, 1:-1] + Pd[2:, 1:-1] + Pd[1:-1, :-2] + Pd[1:-1, 2:]) / 4)[hole]
    return G

def restore(J, a, L):
    I = (J - a * L) / (1 - a)
    if MODE in ("fuse", "afuse"):
        hole = cv2.dilate((a > 0.01).astype(np.uint8), np.ones((3, 3), np.uint8)).astype(bool)
        S = harmonic(J, hole)
        if MODE == "fuse":
            w = 1.0 / (1.0 + BETA * (a / (1 - a)) ** 2)
        else:
            # adaptive inverse-variance fusion, per frame:
            #   inversion error  ~ sigma_codec * a/(1-a)    (codec noise amplified by the division)
            #   smooth-fill error ~ local texture of the surrounding ring in THIS frame
            ring = cv2.dilate(hole.astype(np.uint8), np.ones((7, 7), np.uint8)).astype(bool) & ~hole
            tex = np.std((J - cv2.GaussianBlur(J, (0, 0), 1.5))[ring]) + 0.5
            e_inv2 = (BETA * a / (1 - a)) ** 2
            w = tex ** 2 / (tex ** 2 + e_inv2)
        I = w * I + (1 - w) * S
    return I

def shape_basis(a):
    """Shapes a codec leaves around an overlay: the mark itself, its edges, ringing at 2 scales."""
    b = [a, np.hypot(cv2.Sobel(a, cv2.CV_32F, 1, 0), cv2.Sobel(a, cv2.CV_32F, 0, 1)), cv2.Laplacian(a, cv2.CV_32F),
         cv2.GaussianBlur(a, (0, 0), 1.0) - a, cv2.GaussianBlur(a, (0, 0), 2.0) - a]
    return np.stack([x / (np.abs(x).max() + 1e-6) for x in b], -1)

if MODE == "rgb":
    enc_pix = "rgb24"
else:
    enc_pix = "yuv420p"
# '*.crop.mkv' = keep only the bottom-right 256x128 (all the scorer needs; full lossless clips filled the disk)
CROPF = ["-vf", f"crop=256:128:{W-256}:{H-128}"] if ".crop." in outp else []
enc = subprocess.Popen(["ffmpeg", "-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", enc_pix, "-s", f"{W}x{H}", "-r", FPS,
                        "-i", "-", "-i", inp, "-map", "0:v", "-map", "1:a?", "-c:a", "copy", *CROPF, "-c:v", "ffv1", "-pix_fmt", "yuv420p", outp],
                       stdin=subprocess.PIPE)
def planes(f):
    return (f[:W * H].reshape(H, W), f[W * H:W * H * 5 // 4].reshape(H // 2, W // 2), f[W * H * 5 // 4:].reshape(H // 2, W // 2))

SL = [(slice(Y0, Y0 + bh), slice(X0, X0 + bw))] + [(slice(Y0 // 2, Y0 // 2 + bh // 2), slice(X0 // 2, X0 // 2 + bw // 2))] * 2
AL = [(Afull, 235.0), (Ac, 128.0), (Ac, 128.0)]

ghost = None
if DEGHOST and MODE != "rgb":
    # pass A: restore crops only, then estimate the STATIC leftover (codec-distorted overlay edges):
    # background moves, the ghost doesn't -> time-mean of (restored - smooth guess) isolates it
    rest = [[], [], []]
    for f in frames("yuv420p"):
        for k, (P_, sl, (a, L)) in enumerate(zip(planes(f), SL, AL)):
            rest[k].append(restore(P_[sl].astype(np.float32), a, L))
    ghost = []
    for k, (a, _) in enumerate(AL):
        R = np.array(rest[k])
        m = cv2.dilate((a > 0.01).astype(np.uint8), np.ones((3, 3), np.uint8)).astype(bool)
        S = np.array([harmonic(r, m) for r in R])
        g = (R - S).mean(0) * m
        # static scenes: real texture is static too -> only trust the free-form ghost estimate if the ring moves
        ring = cv2.dilate(m.astype(np.uint8), np.ones((5, 5), np.uint8)).astype(bool) & ~m
        motion = np.abs(np.diff(R[:, ring], axis=0)).mean()
        conf = float(np.clip((motion - 0.5) / 2.0, 0, 1))
        # shape-constrained ghost: project onto a few mark-shaped components -> separable even when static,
        # because real background texture does not look like the mark's edges
        B = shape_basis(a)[m]
        c, *_ = np.linalg.lstsq(B, g[m], rcond=None)
        g_shape = np.zeros_like(g); g_shape[m] = B @ c
        ghost.append(conf * g + (1 - conf) * g_shape)
        print(f"plane {k}: ghost max={np.abs(g).max():.2f} shape-fit max={np.abs(g_shape).max():.2f} ring motion={motion:.2f} -> free-form weight {conf:.2f}")
    del rest

for f in frames(enc_pix):
    f = f.copy()
    if MODE == "rgb":
        img = f.reshape(H, W, 3)
        reg = img[Y0:Y0 + bh, X0:X0 + bw].astype(np.float32)
        img[Y0:Y0 + bh, X0:X0 + bw] = np.clip(restore(reg, Afull[..., None], 255.0) + 0.5, 0, 255)
    else:
        for k, (P_, sl, (a, L)) in enumerate(zip(planes(f), SL, AL)):
            I = restore(P_[sl].astype(np.float32), a, L)
            if ghost is not None: I -= ghost[k]
            P_[sl] = np.clip(I + 0.5, 0, 255)
    enc.stdin.write(f.tobytes())
enc.stdin.close(); enc.wait()
print("->", outp)
