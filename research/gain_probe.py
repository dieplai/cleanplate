"""Probe: per-video mark gain from the SIGNED correlation between the restored frames and the mark shape.
Under-removal leaves a +glyph, over-removal a -glyph; codec noise is not glyph-shaped, so it does not bias
the zero-crossing (unlike edge-energy minimisation, which noise amplification pushes toward small g).
usage: gain_probe.py clip.mp4 template.png
"""
import sys, subprocess, json, numpy as np, cv2

inp, tpl_p = sys.argv[1:3]
s = json.loads(subprocess.check_output(["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries",
                                        "stream=width,height", "-of", "json", inp]))["streams"][0]
W, H = s["width"], s["height"]
A = cv2.imread(tpl_p, cv2.IMREAD_GRAYSCALE).astype(np.float32) / 255.0
ys, xs = np.where(A > 0); A = A[ys.min():ys.max() + 1, xs.min():xs.max() + 1]; th, tw = A.shape
mx, my = W - 16 - tw, H - 16 - th          # known Veo-small position (bottom-right margin 16 px)
P = 4
X0, Y0 = mx - P, my - P
Af = np.zeros((th + 2 * P, tw + 2 * P), np.float32); Af[P:P + th, P:P + tw] = A
p = subprocess.Popen(["ffmpeg", "-v", "error", "-i", inp, "-f", "rawvideo", "-pix_fmt", "yuv420p", "-"], stdout=subprocess.PIPE)
J = []
fs = W * H * 3 // 2
while True:
    b = p.stdout.read(fs)
    if len(b) < fs: break
    J.append(np.frombuffer(b, np.uint8)[:W * H].reshape(H, W)[Y0:Y0 + th + 2 * P, X0:X0 + tw + 2 * P].astype(np.float32))
J = np.array(J)
hole = cv2.dilate((Af > 0.01).astype(np.uint8), np.ones((3, 3), np.uint8)).astype(bool)

def harmonic(F, iters=200):
    G = F.copy(); G[hole] = F[~hole].mean()
    for _ in range(iters):
        Pd = np.pad(G, 1, mode="edge")
        G[hole] = ((Pd[:-2, 1:-1] + Pd[2:, 1:-1] + Pd[1:-1, :-2] + Pd[1:-1, 2:]) / 4)[hole]
    return G

Ac = Af[hole] - Af[hole].mean()
def signed_corr(g):
    a = np.clip(g * Af, 0, 0.95)
    I = (J - a * 235.0) / (1 - a)
    c = [((r - harmonic(r))[hole] * Ac).sum() / (Ac ** 2).sum() for r in I]
    return float(np.median(c))   # median over frames: robust to frames where an edge crosses the mark

gs = np.arange(0.7, 1.31, 0.05)
cs = [signed_corr(g) for g in gs]
i = np.where(np.diff(np.sign(cs)) != 0)[0]
g0 = gs[i[0]] - cs[i[0]] * (gs[i[0] + 1] - gs[i[0]]) / (cs[i[0] + 1] - cs[i[0]]) if len(i) else float("nan")
print(f"zero-crossing g={g0:.3f}   curve: " + " ".join(f"{g:.2f}:{c:+.1f}" for g, c in zip(gs, cs)))
