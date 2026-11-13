"""Zoomed sheet around the detected mark: original | GWT | cleanplate, a few frames per clip.
usage: sheet2.py out.png run.log dir clip...   (reads 'detected ... at (x,y)' + template size from run.log)"""
import subprocess, sys, os, re, numpy as np, cv2
outp, log, d, clips = sys.argv[1], open(sys.argv[2]).read(), sys.argv[3], sys.argv[4:]
def frame(path, t):
    p = subprocess.run(["ffmpeg", "-v", "error", "-i", path, "-vf", f"select=eq(n\\,{t})", "-frames:v", "1",
                        "-f", "image2pipe", "-vcodec", "png", "-"], capture_output=True).stdout
    return cv2.imdecode(np.frombuffer(p, np.uint8), cv2.IMREAD_COLOR)
rows = []
for c in clips:
    m = re.search(rf"== {c}\n(?:.*\n)*?.*detected (\S+) x([\d.]+) at \((\d+),(\d+)\)", log)
    tpl, sc, x, y = m.group(1), float(m.group(2)), int(m.group(3)), int(m.group(4))
    th, tw = (np.array(cv2.imread(f"cleanplate/assets/marks/{tpl}").shape[:2]) * sc).astype(int)
    pad = max(8, th // 2); x0, y0, x1, y1 = x - pad, y - pad, x + tw + pad, y + th + pad
    z = max(2, 360 // (x1 - x0))
    for t in (24, 100, 170):
        cols = []
        for lab, path in [("original", f"ref/{c}.mp4"), ("GWT", f"{d}/{c}_gwt.mp4"), ("cleanplate", f"{d}/{c}_cleanplate.mp4")]:
            f = frame(path, t) if os.path.exists(path) else None
            if f is None: tile = np.full(((y1 - y0) * z, (x1 - x0) * z, 3), 40, np.uint8); lab += " (none)"
            else: tile = cv2.resize(f[y0:y1, x0:x1], None, fx=z, fy=z, interpolation=cv2.INTER_NEAREST)
            cv2.putText(tile, f"{c} {lab} t={t}", (6, 20), 0, 0.5, (255, 255, 255), 3); cv2.putText(tile, f"{c} {lab} t={t}", (6, 20), 0, 0.5, (0, 0, 0), 1)
            cols += [tile, np.full((tile.shape[0], 6, 3), 255, np.uint8)]
        r = np.hstack(cols[:-1]); rows.append(r)
W = max(r.shape[1] for r in rows)
rows = [np.pad(r, ((0, 6), (0, W - r.shape[1]), (0, 0)), constant_values=255) for r in rows]
cv2.imwrite(outp, np.vstack(rows)); print(outp)
