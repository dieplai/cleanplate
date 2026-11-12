"""Which 4:2:0 chroma siting does the generator's encoder use? Invert U,V with the chroma alpha derived both ways and
measure the leftover mark-shaped energy in the chroma residual (smaller = the right model).
usage: chroma_siting_probe.py clip.mp4 template.png"""
import sys, subprocess, json, numpy as np, cv2
inp, tpl = sys.argv[1:3]
s = json.loads(subprocess.check_output(["ffprobe","-v","error","-select_streams","v:0","-show_entries","stream=width,height","-of","json",inp]))["streams"][0]
W, H = s["width"], s["height"]
A = cv2.imread(tpl, 0).astype(np.float32) / 255; ys, xs = np.where(A > 0); A = A[ys.min():ys.max()+1, xs.min():xs.max()+1]; th, tw = A.shape
mx, my = W - 16 - tw, H - 16 - th; P = 6
X0, Y0 = (mx - P) & ~1, (my - P) & ~1; RW, RH = (tw + 2*P + 2) & ~1, (th + 2*P + 2) & ~1
Ay = np.zeros((RH, RW), np.float32); Ay[my-Y0:my-Y0+th, mx-X0:mx-X0+tw] = A
center = Ay.reshape(RH//2, 2, RW//2, 2).mean(axis=(1, 3))
h = cv2.filter2D(Ay, -1, np.array([[0.25, 0.5, 0.25]], np.float32), borderType=cv2.BORDER_REPLICATE)
left = h.reshape(RH//2, 2, RW).mean(axis=1)[:, ::2]
fs = W*H*3//2; p = subprocess.Popen(["ffmpeg","-v","error","-i",inp,"-f","rawvideo","-pix_fmt","yuv420p","-"], stdout=subprocess.PIPE)
U, V = [], []
while True:
    b = p.stdout.read(fs)
    if len(b) < fs: break
    f = np.frombuffer(b, np.uint8)
    U.append(f[W*H:W*H*5//4].reshape(H//2, W//2)[Y0//2:Y0//2+RH//2, X0//2:X0//2+RW//2].astype(np.float32))
    V.append(f[W*H*5//4:].reshape(H//2, W//2)[Y0//2:Y0//2+RH//2, X0//2:X0//2+RW//2].astype(np.float32))
U, V = np.array(U), np.array(V)
hole = cv2.dilate((center > 0.005).astype(np.uint8), np.ones((3, 3), np.uint8)).astype(bool)
def harmonic(F, it=100):
    G = F.copy(); G[hole] = F[~hole].mean()
    for _ in range(it):
        Pd = np.pad(G, 1, mode="edge"); G[hole] = ((Pd[:-2,1:-1]+Pd[2:,1:-1]+Pd[1:-1,:-2]+Pd[1:-1,2:])/4)[hole]
    return G
for name, ac in (("center", center), ("left", left)):
    e = []
    for C in (U, V):
        R = (C - ac * 128) / (1 - ac)
        res = np.array([(r - harmonic(r))[hole] for r in R])
        e.append(np.abs(res.mean(0)).mean())     # static (mark-locked) leftover
    print(f"{name:6} static chroma leftover U={e[0]:.3f} V={e[1]:.3f}")
