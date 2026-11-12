"""Extract a reference alpha template for a mark from a clip whose mark sits on a near-black background.
On black, J = (1-a)*I + a*L with I ~ 0 => a ~ (J - I)/(L - I). Averaged over the darkest, flattest frames.
usage: make_template.py clip.mp4 x0 y0 w h out.png   (box generously around the mark, in pixels)
"""
import sys, subprocess, json, numpy as np, cv2

clip, X0, Y0, BW, BH, outp = sys.argv[1], *map(int, sys.argv[2:6]), sys.argv[6]
s = json.loads(subprocess.check_output(["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries",
                                        "stream=width,height", "-of", "json", clip]))["streams"][0]
W, H = s["width"], s["height"]
p = subprocess.Popen(["ffmpeg", "-v", "error", "-i", clip, "-f", "rawvideo", "-pix_fmt", "rgb24", "-"], stdout=subprocess.PIPE)
F = []
while True:
    b = p.stdout.read(W * H * 3)
    if len(b) < W * H * 3: break
    F.append(np.frombuffer(b, np.uint8).reshape(H, W, 3)[Y0:Y0 + BH, X0:X0 + BW].astype(np.float32))
F = np.array(F)
R = 4  # border ring used as the local background estimate
ring = np.ones((BH, BW), bool); ring[R:-R, R:-R] = False
lvl = F[:, ring].mean((1, 2)); flat = F[:, ring].std((1, 2))
pick = np.argsort(lvl + 2 * flat)[: max(10, len(F) // 4)]
print(f"frames={len(F)} using {len(pick)} darkest/flattest, ring level={lvl[pick].mean():.1f} std={flat[pick].mean():.2f}")
L = 255.0
A = []
for t in pick:
    bg = np.median(F[t][ring], 0)                      # per-channel background
    a = (F[t] - bg) / (L - bg)                          # per-channel alpha estimate
    A.append(a.mean(2))                                 # mark is achromatic: average channels
A = np.clip(np.median(np.array(A), 0), 0, 1)
A[A < 0.02] = 0
print(f"alpha max={A.max():.3f} support px={(A > 0.05).sum()} bbox={cv2.boundingRect((A > 0.05).astype(np.uint8))} (rel to box)")
cv2.imwrite(outp, np.round(A * 255).astype(np.uint8))
print("->", outp)
