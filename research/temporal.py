"""Motion-compensated temporal reconstruction of the area under a static mark (no spatial blur, no invented pixels).

1. invert the overlay per frame in native YUV  -> R_t  (correct on average, but codec noise amplified by 1/(1-a))
2. optical flow (DIS) between nearby frames, computed on R and re-filled inside the mark from its surroundings
3. every mark pixel gathers candidates from up to 2K neighbours:
     - candidates landing OUTSIDE the mark are real, clean pixels of the same scene point  -> high weight
     - candidates landing INSIDE the mark are other noisy inversions                      -> weight (1-a_src)^2
     - forward/backward flow check rejects occlusions; robust re-weighting rejects outliers
   static scenes degrade gracefully into a multi-frame average of inversions (noise / sqrt(N), still sharp)
Works on the bottom-right 256x128 crop only; writes a '*.crop.mkv' (lossless) for the scorers.

usage: temporal.py in.mp4 out.crop.mkv template.png [K]
"""
import sys, subprocess, json, numpy as np, cv2

inp, outp, tpl_p = sys.argv[1:4]
K = int(sys.argv[4]) if len(sys.argv) > 4 else 8
s = json.loads(subprocess.check_output(["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries",
                                        "stream=width,height,r_frame_rate", "-of", "json", inp]))["streams"][0]
W, H, FPS = s["width"], s["height"], s["r_frame_rate"]
CW, CH = 256, 128
cx0, cy0 = W - CW, H - CH

# ---- read crops (Y full-res, U/V half-res)
fs = W * H * 3 // 2
p = subprocess.Popen(["ffmpeg", "-v", "error", "-i", inp, "-f", "rawvideo", "-pix_fmt", "yuv420p", "-"], stdout=subprocess.PIPE)
Ys, Us, Vs = [], [], []
while True:
    b = p.stdout.read(fs)
    if len(b) < fs: break
    f = np.frombuffer(b, np.uint8)
    Ys.append(f[:W * H].reshape(H, W)[cy0:, cx0:].astype(np.float32))
    Us.append(f[W * H:W * H * 5 // 4].reshape(H // 2, W // 2)[cy0 // 2:, cx0 // 2:].astype(np.float32))
    Vs.append(f[W * H * 5 // 4:].reshape(H // 2, W // 2)[cy0 // 2:, cx0 // 2:].astype(np.float32))
Ys, Us, Vs = np.array(Ys), np.array(Us), np.array(Vs)
N = len(Ys)

# ---- place template (known Veo-small position; NCC sanity check)
A = cv2.imread(tpl_p, cv2.IMREAD_GRAYSCALE).astype(np.float32) / 255.0
ys, xs = np.where(A > 0); A = A[ys.min():ys.max() + 1, xs.min():xs.max() + 1]; th, tw = A.shape
mx, my = CW - 16 - tw, CH - 16 - th
hp = (Ys - np.array([cv2.GaussianBlur(y, (0, 0), 3) for y in Ys])).mean(0)
r = cv2.matchTemplate(hp, A - cv2.GaussianBlur(A, (0, 0), 3), cv2.TM_CCOEFF_NORMED)
_, score, _, (bx, by) = cv2.minMaxLoc(r)
if score >= 0.5 and abs(bx - mx) <= 3 and abs(by - my) <= 3:
    mx, my = bx, by
print(f"mark at crop ({mx},{my}) ncc={score:.3f}")
if score < 0.3:
    sys.exit("low confidence — refusing to modify")
Ay = np.zeros((CH, CW), np.float32); Ay[my:my + th, mx:mx + tw] = A
Ac = Ay.reshape(CH // 2, 2, CW // 2, 2).mean(axis=(1, 3))

# ---- 1. per-frame inversion
Ry = (Ys - Ay * 235.0) / (1 - Ay)
Ru = (Us - Ac * 128.0) / (1 - Ac)
Rv = (Vs - Ac * 128.0) / (1 - Ac)

M = cv2.dilate((Ay > 0.01).astype(np.uint8), np.ones((5, 5), np.uint8)).astype(bool)   # area we rebuild
Mflow = cv2.dilate(M.astype(np.uint8), np.ones((5, 5), np.uint8)).astype(bool)          # flow re-fill area

def harmonic(F, hole, iters=150):
    G = F.copy(); G[hole] = F[~hole].mean()
    for _ in range(iters):
        Pd = np.pad(G, 1, mode="edge")
        G[hole] = ((Pd[:-2, 1:-1] + Pd[2:, 1:-1] + Pd[1:-1, :-2] + Pd[1:-1, 2:]) / 4)[hole]
    return G

dis = cv2.DISOpticalFlow_create(cv2.DISOPTICAL_FLOW_PRESET_MEDIUM)
R8 = np.clip(Ry, 0, 255).astype(np.uint8)
fy_, fx_ = np.where(Mflow)
FW = (slice(max(fy_.min() - 6, 0), fy_.max() + 7), slice(max(fx_.min() - 6, 0), fx_.max() + 7))
Mflow_w = Mflow[FW]
def flow(a, b):
    F = dis.calc(R8[a], R8[b], None)
    # the mark area is unreliable for matching: re-fill it from the surrounding motion field (window only: fast)
    for k in range(2):
        F[..., k][FW] = harmonic(F[..., k][FW], Mflow_w)
    return F

gy, gx = np.mgrid[0:CH, 0:CW].astype(np.float32)
# work only on the rows/cols around the mark to keep it fast
ys_, xs_ = np.where(M)
wy0, wy1, wx0, wx1 = max(ys_.min() - 4, 0), min(ys_.max() + 5, CH), max(xs_.min() - 4, 0), min(xs_.max() + 5, CW)

out_y, out_u, out_v = Ry.copy(), Ru.copy(), Rv.copy()
stats = {"clean": 0.0, "total": 0.0}
for t in range(N):
    cand_y, cand_u, cand_v, wts = [Ry[t]], [Ru[t]], [Rv[t]], [(1 - Ay) ** 2]
    for s_ in range(max(0, t - K), min(N, t + K + 1)):
        if s_ == t: continue
        Fts, Fst = flow(t, s_), flow(s_, t)
        mxp, myp = gx + Fts[..., 0], gy + Fts[..., 1]
        # forward-backward consistency: occluded / unreliable correspondences get no weight
        back = np.stack([cv2.remap(Fst[..., k], mxp, myp, cv2.INTER_LINEAR, borderMode=cv2.BORDER_REPLICATE) for k in range(2)], -1)
        fb = np.hypot(Fts[..., 0] + back[..., 0], Fts[..., 1] + back[..., 1])
        inside = (mxp >= 0) & (mxp <= CW - 1) & (myp >= 0) & (myp <= CH - 1)
        a_src = cv2.remap(Ay, mxp, myp, cv2.INTER_LINEAR, borderMode=cv2.BORDER_CONSTANT, borderValue=0)
        w = (1 - a_src) ** 2 * np.exp(-(fb / 0.7) ** 2) * inside * np.exp(-abs(s_ - t) / (2.0 * K))
        cand_y.append(cv2.remap(Ry[s_], mxp, myp, cv2.INTER_CUBIC, borderMode=cv2.BORDER_REPLICATE))
        hx, hy = mxp[::2, ::2] / 2, myp[::2, ::2] / 2
        cand_u.append(cv2.remap(Ru[s_], hx, hy, cv2.INTER_LINEAR, borderMode=cv2.BORDER_REPLICATE))
        cand_v.append(cv2.remap(Rv[s_], hx, hy, cv2.INTER_LINEAR, borderMode=cv2.BORDER_REPLICATE))
        wts.append(w)
        stats["clean"] += float(((a_src < 0.02) * w)[M].sum()); stats["total"] += float(w[M].sum())
    Cy, Wt = np.array(cand_y), np.array(wts)
    # robust weighted mean: start from the weighted mean, down-weight candidates far from it (Huber)
    mu = (Wt * Cy).sum(0) / (Wt.sum(0) + 1e-6)
    for _ in range(2):
        dev = np.abs(Cy - mu); sc = np.median(dev, 0) * 1.4826 + 2.0
        Wr = Wt * np.minimum(1.0, 1.5 * sc / (dev + 1e-6))
        mu = (Wr * Cy).sum(0) / (Wr.sum(0) + 1e-6)
    out_y[t][M] = mu[M]
    Wc = Wr[:, ::2, ::2]   # same weights on the chroma grid (4:2:0)
    Mc = M[::2, ::2]
    for C, cands in ((out_u, cand_u), (out_v, cand_v)):
        Cc = np.array(cands)
        C[t][Mc] = ((Wc * Cc).sum(0) / (Wc.sum(0) + 1e-6))[Mc]
print(f"share of weight from clean (outside-mark) pixels: {stats['clean'] / max(stats['total'], 1e-6):.2f}")

enc = subprocess.Popen(["ffmpeg", "-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "yuv420p", "-s", f"{CW}x{CH}", "-r", FPS,
                        "-i", "-", "-c:v", "ffv1", outp], stdin=subprocess.PIPE)
for t in range(N):
    enc.stdin.write(np.clip(out_y[t] + 0.5, 0, 255).astype(np.uint8).tobytes()
                    + np.clip(out_u[t] + 0.5, 0, 255).astype(np.uint8).tobytes()
                    + np.clip(out_v[t] + 0.5, 0, 255).astype(np.uint8).tobytes())
enc.stdin.close(); enc.wait()
print("->", outp)
