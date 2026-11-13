"""Ghost map: temporal mean of the mark box, high-passed and x4 contrast -> a faint mark-shaped leftover becomes visible.
usage: ghostmap.py out.png x y w h label=video ...   (box in full-frame pixels)"""
import sys, subprocess, json, numpy as np, cv2
outp, x, y, w, h = sys.argv[1], *map(int, sys.argv[2:6]); P = 12
tiles = []
for lab, path in (a.split("=", 1) for a in sys.argv[6:]):
    s = json.loads(subprocess.check_output(["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height", "-of", "json", path]))["streams"][0]
    W, H = s["width"], s["height"]
    p = subprocess.Popen(["ffmpeg", "-v", "error", "-i", path, "-f", "rawvideo", "-pix_fmt", "gray", "-"], stdout=subprocess.PIPE)
    acc, n = 0, 0
    while True:
        b = p.stdout.read(W * H)
        if len(b) < W * H: break
        acc = acc + np.frombuffer(b, np.uint8).reshape(H, W)[y - P:y + h + P, x - P:x + w + P].astype(np.float32); n += 1
    m = acc / n
    hp = np.clip(128 + 4 * (m - cv2.GaussianBlur(m, (0, 0), 4)), 0, 255).astype(np.uint8)
    z = max(2, 300 // hp.shape[1])
    t = cv2.cvtColor(cv2.resize(np.hstack([m.astype(np.uint8), hp]), None, fx=z, fy=z, interpolation=cv2.INTER_NEAREST), cv2.COLOR_GRAY2BGR)
    cv2.putText(t, lab, (6, 20), 0, 0.6, (0, 0, 255), 2); tiles += [t, np.full((6, t.shape[1], 3), 255, np.uint8)]
cv2.imwrite(outp, np.vstack(tiles[:-1])); print(outp)
