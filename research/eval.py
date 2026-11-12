"""Compare watermark removal outputs. usage: eval.py orig.mp4 name=clean.mp4 ... -> prints metrics, writes montage png."""
import sys, cv2, numpy as np

def frames(p):
    # ponytail: only keep the bottom-right third (watermark lives there) as uint8 — full float frames OOM'd a 15GB box
    c = cv2.VideoCapture(p); out = []
    while True:
        ok, f = c.read()
        if not ok: return np.array(out)
        # every mark we target sits in the bottom-right 256x128; '*.crop.mkv' outputs are already exactly that
        out.append(f[-128:, -256:].copy())   # crops are bottom-right aligned, so this works for full and crop files

orig_p, outs = sys.argv[1], [a.split("=", 1) for a in sys.argv[2:] if "=" in a]
FIXED = "--fixed" in sys.argv  # known Veo-small box in the 256x128 corner crop (219,103,21,9); change-based bbox fails on faint marks
O = frames(orig_p).astype(np.float32); H, W = O.shape[1:3]  # H, W are crop dims from here on
C = {n: frames(p)[:len(O)].astype(np.float32) for n, p in outs}
# watermark bbox = where any tool changed pixels on average (re-encode noise is small, logo change is large)
chg = sum(np.abs(O[:len(v)] - v).mean((0, 3)) for v in C.values())
if FIXED:
    x0, y0, x1, y1 = 219, 103, 240, 112
else:
    ys, xs = np.where(chg > chg.max() * 0.3)
    y0, y1, x0, x1 = ys.min(), ys.max() + 1, xs.min(), xs.max() + 1
print(f"{W}x{H} frames={len(O)} watermark bbox x={x0} y={y0} w={x1-x0} h={y1-y0}")
pad = 24
Y0, Y1, X0, X1 = max(0, y0 - pad), min(H, y1 + pad), max(0, x0 - pad), min(W, x1 + pad)

# scored per B,G,R channel, worst channel reported — averaging channels hid a magenta/green chroma failure
def residual(V):
    # static leftover: temporal mean minus its blur, inside bbox vs ring around it
    m = V.mean(0)[Y0:Y1, X0:X1]
    inner = np.zeros(m.shape[:2], bool); inner[y0 - Y0:y1 - Y0, x0 - X0:x1 - X0] = True
    out = []
    for c in range(3):
        hp = m[..., c] - cv2.GaussianBlur(m[..., c], (0, 0), 6)
        out.append(np.abs(hp[inner]).mean() / (np.abs(hp[~inner]).mean() + 1e-6))
    return max(out)

def flicker(V):
    # temporal jitter inside bbox vs ring (high = flashing patch)
    d = np.abs(np.diff(V[:, Y0:Y1, X0:X1], axis=0)).mean(0)
    inner = np.zeros(d.shape[:2], bool); inner[y0 - Y0:y1 - Y0, x0 - X0:x1 - X0] = True
    return max(d[..., c][inner].mean() / (d[..., c][~inner].mean() + 1e-6) for c in range(3))

def color_shift(V):
    # mean colour offset of the restored bbox vs its ring, worst channel (0 = no tint/box)
    m = V.mean(0)[Y0:Y1, X0:X1]
    inner = np.zeros(m.shape[:2], bool); inner[y0 - Y0:y1 - Y0, x0 - X0:x1 - X0] = True
    ring = cv2.dilate(inner.astype(np.uint8), np.ones((9, 9), np.uint8)).astype(bool) & ~inner
    return max(abs(m[..., c][inner].mean() - m[..., c][ring].mean()) for c in range(3))

print(f"{'name':12} {'residual(1=clean)':>18} {'flicker(1=clean)':>17} {'tint(0=clean)':>14} {'frames':>7}")
for n, V in [("original", O)] + list(C.items()):
    print(f"{n:12} {residual(V):18.3f} {flicker(V):17.3f} {color_shift(V):14.2f} {len(V):7d}")

# montage: rows = 3 sample frames + temporal mean (contrast-boosted), cols = videos, 4x zoom
idx = [len(O) // 6, len(O) // 2, 5 * len(O) // 6]
cols = []
for n, V in [("original", O)] + list(C.items()):
    tiles = [V[i, Y0:Y1, X0:X1] for i in idx]
    m = V.mean(0)[Y0:Y1, X0:X1]
    tiles.append(np.clip((m - cv2.GaussianBlur(m, (0, 0), 6)) * 6 + 128, 0, 255))
    col = np.vstack([cv2.resize(t, None, fx=4, fy=4, interpolation=cv2.INTER_NEAREST) for t in tiles])
    lab = np.full((40, col.shape[1], 3), 255, np.float32)
    cv2.putText(lab, n, (8, 28), cv2.FONT_HERSHEY_SIMPLEX, 0.9, (0, 0, 0), 2)
    cols.append(np.vstack([lab, col])); cols.append(np.full((cols[-1].shape[0], 8, 3), 255, np.float32))
out = sys.argv[1].rsplit(".", 1)[0].split("/")[-1] + "_montage.png"
cv2.imwrite("out/" + out, np.hstack(cols[:-1]).astype(np.uint8)); print("montage -> out/" + out)
