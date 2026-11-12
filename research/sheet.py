"""Side-by-side sheet: original | GWT | cleanplate variants, two frames per clip, 5x zoom around the mark.
usage: sheet.py out.png label=suffix ...   (suffix -> out/real/<clip>_<suffix>)"""
import subprocess, sys, os, numpy as np, cv2
outp = sys.argv[1]
variants = [a.split("=", 1) for a in sys.argv[2:]]
clips = ["veo31fast_flower_720p", "night_720p", "snow_720p", "teacup_static_720p", "kitchen_9x16"]
def frame(path, t):
    p = subprocess.run(["ffmpeg", "-v", "error", "-i", path, "-vf", f"select=eq(n\\,{t})", "-frames:v", "1",
                        "-f", "image2pipe", "-vcodec", "png", "-"], capture_output=True).stdout
    f = cv2.imdecode(np.frombuffer(p, np.uint8), cv2.IMREAD_COLOR)
    return f[-128:, -256:]   # crops are bottom-right aligned
rows = []
for c in clips:
    for t in (30, 96):
        cols = []
        for lab, path in [("original", f"ref/{c}.mp4"), ("GWT", f"out/real/{c}_gwt.mp4")] + [(l, f"out/real/{c}_{s}") for l, s in variants]:
            if os.path.exists(path):
                tile = cv2.resize(frame(path, t)[93:123, 196:256], None, fx=5, fy=5, interpolation=cv2.INTER_NEAREST)
            else:
                tile = np.full((150, 300, 3), 40, np.uint8); cv2.putText(tile, "GWT skipped", (50, 85), 0, 0.8, (200, 200, 200), 2)
            cv2.putText(tile, lab, (6, 22), 0, 0.6, (255, 255, 255), 3); cv2.putText(tile, lab, (6, 22), 0, 0.6, (0, 0, 0), 1)
            cols += [tile, np.full((tile.shape[0], 6, 3), 255, np.uint8)]
        rows += [np.hstack(cols[:-1]), np.full((6, sum(x.shape[1] for x in cols[:-1]), 3), 255, np.uint8)]
cv2.imwrite(outp, np.vstack(rows[:-1])); print(outp)
