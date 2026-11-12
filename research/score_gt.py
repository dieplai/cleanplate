"""Score removers against ground truth from synth.py.
usage: score_gt.py bench/<name> method=out.mp4 ...
Reports, inside the mark box (+2px): PSNR vs GT, and excess = PSNR(floor) - PSNR(method) in dB (0 = as good as
a clip that never had a mark), plus temporal-mean error (static ghost) which is what the eye notices on playback.
"""
import sys, json, subprocess, numpy as np

base = sys.argv[1]
meta = json.load(open(base + "_meta.json")); W, H = meta["W"], meta["H"]
cx0, cy0, cx1, cy1 = meta["crop"]; mx, my, tw, th = meta["mark"]
GT = np.load(base + "_gt.npy").astype(np.float32)
by0, by1, bx0, bx1 = my - cy0 - 2, my - cy0 + th + 2, mx - cx0 - 2, mx - cx0 + tw + 2

def crops(path):
    # '*.crop.mkv' files hold only the bottom-right 256x128 of the frame
    # '*.crop.*' files hold only a bottom-right corner; its size is read from the file
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
        out.append(np.frombuffer(b, np.uint8).reshape(fh, fw, 3)[cy0 - oy:cy1 - oy, cx0 - ox:cx1 - ox].astype(np.float32))
    return np.array(out)

import cv2
def hf(X):   # high-frequency energy inside the box, per frame
    return np.array([np.abs(f - cv2.GaussianBlur(f, (0, 0), 1.0))[by0:by1, bx0:bx1].mean() for f in X])

GT_HF = None
def score(V):
    global GT_HF
    n = min(len(V), len(GT))
    d = (V[:n] - GT[:n])[:, by0:by1, bx0:bx1]
    mse = (d ** 2).mean()
    ghost = np.abs(d.mean(0)).max()            # worst static (time-averaged) error, any pixel/channel
    if GT_HF is None: GT_HF = hf(GT)
    sharp = hf(V[:n]).mean() / (GT_HF[:n].mean() + 1e-6)   # 1 = as detailed as the truth, <1 = blurry patch
    return 10 * np.log10(255 ** 2 / max(mse, 1e-9)), ghost, sharp

floor_psnr, floor_ghost, floor_sharp = score(crops(base + "_floor.mp4"))
rows = [("floor(no mark)", floor_psnr, floor_ghost, floor_sharp), ("marked(input)", *score(crops(base + "_marked.mp4")))]
for arg in sys.argv[2:]:
    n, p = arg.split("=", 1)
    rows.append((n, *score(crops(p))))
print(f"{base.split('/')[-1]}  (box {tw}x{th}+2px)")
print(f"  {'method':16} {'PSNR':>7} {'excess dB':>10} {'ghost':>7} {'sharp':>6}")
for n, ps, gh, sh in rows:
    print(f"  {n:16} {ps:7.2f} {floor_psnr - ps:10.2f} {gh:7.2f} {sh:6.2f}")
