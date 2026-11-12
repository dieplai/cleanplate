"""Per-clip mark registration: gain g, sub-pixel shift (dx, dy), extra blur s.

For each parameter k, the restored frames' residual (I_t - smooth fill) is projected onto that parameter's
alpha-error shape dA_k. A correct parameter gives a median projection of 0 (no glyph-shaped leftover of that
kind). We root-find each parameter in turn (coordinate descent on zero-crossings), using the MEDIAN over frames:
robust, and — validated for g on ground truth — unbiased, unlike the de-meaned regression that diverged
(errors-in-variables via the interpolated background).

usage: fit_mark.py clip.mp4 template.png out_template.png
"""
import sys, subprocess, json, numpy as np, cv2

inp, tpl_p, outp = sys.argv[1:4]
s = json.loads(subprocess.check_output(["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries",
                                        "stream=width,height", "-of", "json", inp]))["streams"][0]
W, H = s["width"], s["height"]
A0 = cv2.imread(tpl_p, cv2.IMREAD_GRAYSCALE).astype(np.float32) / 255.0
ys, xs = np.where(A0 > 0); A0 = A0[ys.min():ys.max() + 1, xs.min():xs.max() + 1]; th, tw = A0.shape
mx, my = W - 16 - tw, H - 16 - th
P = 4
X0, Y0 = mx - P, my - P
Ab = np.zeros((th + 2 * P, tw + 2 * P), np.float32); Ab[P:P + th, P:P + tw] = A0
fs = W * H * 3 // 2
p = subprocess.Popen(["ffmpeg", "-v", "error", "-i", inp, "-f", "rawvideo", "-pix_fmt", "yuv420p", "-"], stdout=subprocess.PIPE)
J = []
while True:
    b = p.stdout.read(fs)
    if len(b) < fs: break
    J.append(np.frombuffer(b, np.uint8)[:W * H].reshape(H, W)[Y0:Y0 + Ab.shape[0], X0:X0 + Ab.shape[1]].astype(np.float32))
J = np.array(J)[::2]   # every other frame is plenty for 4 parameters
hole = cv2.dilate((Ab > 0.01).astype(np.uint8), np.ones((5, 5), np.uint8)).astype(bool)

def harmonic(F, iters=150):
    G = F.copy(); G[hole] = F[~hole].mean()
    for _ in range(iters):
        Pd = np.pad(G, 1, mode="edge")
        G[hole] = ((Pd[:-2, 1:-1] + Pd[2:, 1:-1] + Pd[1:-1, :-2] + Pd[1:-1, 2:]) / 4)[hole]
    return G

def render(g, dx, dy, sg):
    a = cv2.warpAffine(Ab, np.float32([[1, 0, dx], [0, 1, dy]]), Ab.shape[::-1], flags=cv2.INTER_CUBIC)
    if sg > 1e-3: a = cv2.GaussianBlur(a, (0, 0), float(np.sqrt(sg)))
    return np.clip(a * g, 0, 0.95).astype(np.float32)

L = 235.0
def shapes(a):
    return [a, cv2.Sobel(a, cv2.CV_32F, 1, 0, ksize=3) / 8, cv2.Sobel(a, cv2.CV_32F, 0, 1, ksize=3) / 8, cv2.Laplacian(a, cv2.CV_32F) / 2]

def projection(prm, k):
    a = render(*prm)
    I = (J - a * L) / (1 - a)
    d = shapes(a)[k][hole]; d = (d - d.mean()) / (np.linalg.norm(d - d.mean()) + 1e-6)
    return float(np.median([((r - harmonic(r))[hole] * d).sum() for r in I]))

prm = [1.0, 0.0, 0.0, 0.0]
grids = [np.arange(0.8, 1.21, 0.05), np.arange(-1.0, 1.01, 0.25), np.arange(-1.0, 1.01, 0.25), np.array([0, 0.1, 0.25, 0.5, 0.8, 1.2])]
for rnd in range(2):
    for k in range(4):
        vals = grids[k]; pr = []
        for v in vals:
            q = list(prm); q[k] = float(v); pr.append(projection(q, k))
        pr = np.array(pr)
        i = np.where(np.diff(np.sign(pr)) != 0)[0]
        if len(i):  # nearest crossing to the current value, linear interpolation
            j = i[np.argmin(np.abs(vals[i] - prm[k]))]
            prm[k] = float(vals[j] - pr[j] * (vals[j + 1] - vals[j]) / (pr[j + 1] - pr[j]))
        else:       # no crossing: smallest |projection|
            prm[k] = float(vals[np.argmin(np.abs(pr))])
    print(f"round {rnd}: g={prm[0]:.3f} dx={prm[1]:+.3f} dy={prm[2]:+.3f} blur_var={prm[3]:.3f}")
cv2.imwrite(outp, np.round(render(*prm) * 255).clip(0, 255).astype(np.uint8))
print(f"template written (padded {P}px), box top-left in frame {X0},{Y0}")
