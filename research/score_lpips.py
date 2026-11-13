"""Perceptual score (LPIPS, AlexNet) of removers against ground truth from synth.py, on the GT crop around the mark.
LPIPS tolerates a different-but-plausible texture far better than PSNR and penalises blur and ghosts — closer to what
the eye notices. usage: score_lpips.py bench/<name> method=out.mp4 ...   (needs torch + lpips; see tools/venv)
Prints one line per method: LPIPS (lower = closer to the truth) and excess over the floor (the clean clip encoded)."""
import sys, json, subprocess, numpy as np, torch, lpips
base = sys.argv[1]
meta = json.load(open(base + "_meta.json")); W, H = meta["W"], meta["H"]; cx0, cy0, cx1, cy1 = meta["crop"]
GT = np.load(base + "_gt.npy")
def crops(path):
    fw, fh = W, H
    if ".crop." in path:
        st = json.loads(subprocess.check_output(["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries",
                                                 "stream=width,height", "-of", "json", path]))["streams"][0]
        fw, fh = st["width"], st["height"]
    ox, oy = W - fw, H - fh
    p = subprocess.Popen(["ffmpeg", "-v", "error", "-i", path, "-f", "rawvideo", "-pix_fmt", "rgb24", "-"], stdout=subprocess.PIPE)
    out = []
    while True:
        b = p.stdout.read(fw * fh * 3)
        if len(b) < fw * fh * 3: break
        out.append(np.frombuffer(b, np.uint8).reshape(fh, fw, 3)[cy0 - oy:cy1 - oy, cx0 - ox:cx1 - ox].copy())
    return np.array(out)
net = lpips.LPIPS(net="alex", verbose=False)
def t(x): return torch.from_numpy(x.astype(np.float32) / 127.5 - 1).permute(0, 3, 1, 2)
def score(V):
    n = min(len(V), len(GT)); idx = np.arange(0, n, 2)
    with torch.no_grad(): return float(net(t(V[idx]), t(GT[idx])).mean())
fl = score(crops(base + "_floor.mp4"))
name = base.split("/")[-1]
print(f"{name} floor {fl:.4f}")
for arg in ["marked=" + base + "_marked.mp4"] + sys.argv[2:]:
    n, p = arg.split("=", 1); s = score(crops(p))
    print(f"{name} {n} {s:.4f} excess {s - fl:+.4f}")
