"""Static-shot experiment: weighted-TV (MAP) refinement of the temporally averaged inversion.

In a static shot the codec repeats the same error around the mark every frame, so averaging cannot remove it.
Solve per frame   min_I  sum w (I - mu)^2 + lam * TV_eps(I)   inside the mark, w = (1-a)^2
(mu = robust temporal mean of the inversions over +-K frames). Where alpha is high the data is unreliable and the
edge-sparsity prior wins (kills glyph-shaped codec edges); where alpha is low the data keeps real detail.
usage: static_tv.py in.mp4 out.crop.mkv template.png lam [K]
"""
import sys, subprocess, json, numpy as np, cv2

inp, outp, tpl_p = sys.argv[1:4]
LAM = float(sys.argv[4]); K = int(sys.argv[5]) if len(sys.argv) > 5 else 12
s = json.loads(subprocess.check_output(["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries",
                                        "stream=width,height,r_frame_rate", "-of", "json", inp]))["streams"][0]
W, H, FPS = s["width"], s["height"], s["r_frame_rate"]
CW, CH = 256, 128; cx0, cy0 = W - CW, H - CH
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
Ys, Us, Vs = np.array(Ys), np.array(Us), np.array(Vs); N = len(Ys)
A = cv2.imread(tpl_p, cv2.IMREAD_GRAYSCALE).astype(np.float32) / 255.0
ys, xs = np.where(A > 0); A = A[ys.min():ys.max() + 1, xs.min():xs.max() + 1]; th, tw = A.shape
mx, my = CW - 16 - tw, CH - 16 - th
Ay = np.zeros((CH, CW), np.float32); Ay[my:my + th, mx:mx + tw] = A
Ac = Ay.reshape(CH // 2, 2, CW // 2, 2).mean(axis=(1, 3))

def tv_refine(mu, a, mask, lam, iters=300, eps=1.0):
    """gradient descent on sum w (I-mu)^2 + lam*sqrt(|grad I|^2+eps^2), only pixels in mask move"""
    I = mu.copy(); w = (1 - a) ** 2
    tau = 0.2 / (1 + lam)
    for _ in range(iters):
        gx = np.diff(I, axis=1, append=I[:, -1:]); gy = np.diff(I, axis=0, append=I[-1:, :])
        n = np.sqrt(gx ** 2 + gy ** 2 + eps ** 2)
        px, py = gx / n, gy / n
        div = px - np.roll(px, 1, axis=1) + py - np.roll(py, 1, axis=0)
        g = 2 * w * (I - mu) - lam * div
        I[mask] -= tau * g[mask]
    return I

out = []
for P_, a, L in ((Ys, Ay, 235.0), (Us, Ac, 128.0), (Vs, Ac, 128.0)):
    R = (P_ - a * L) / (1 - a)
    mask = cv2.dilate((a > 0.01).astype(np.uint8), np.ones((3, 3), np.uint8)).astype(bool)
    O = R.copy()
    cs = np.cumsum(np.concatenate([np.zeros_like(R[:1]), R]), 0)
    for t in range(N):
        t0, t1 = max(0, t - K), min(N, t + K + 1)
        mu = (cs[t1] - cs[t0]) / (t1 - t0)
        O[t] = tv_refine(mu, a, mask, LAM) if LAM > 0 else mu
    out.append(O)
enc = subprocess.Popen(["ffmpeg", "-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "yuv420p", "-s", f"{CW}x{CH}", "-r", FPS,
                        "-i", "-", "-c:v", "ffv1", outp], stdin=subprocess.PIPE)
for t in range(N):
    enc.stdin.write(b"".join(np.clip(o[t] + 0.5, 0, 255).astype(np.uint8).tobytes() for o in out))
enc.stdin.close(); enc.wait(); print("->", outp)
