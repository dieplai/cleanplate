"""Side-by-side sheet: original | GWT | cleanplate, one frame per clip, 5x zoom around the mark."""
import subprocess, numpy as np, cv2, os
clips = ["veo31fast_flower_720p", "night_720p", "snow_720p", "teacup_static_720p", "kitchen_9x16"]
def frame(path, t=96):
    p = subprocess.run(["ffmpeg", "-v", "error", "-i", path, "-vf", f"select=eq(n\\,{t})", "-frames:v", "1",
                        "-f", "image2pipe", "-vcodec", "png", "-"], capture_output=True).stdout
    f = cv2.imdecode(np.frombuffer(p, np.uint8), cv2.IMREAD_COLOR)
    return f if ".crop." in path else f[-128:, -256:]
rows = []
for c in clips:
    cols = []
    for lab, path in [("original", f"ref/{c}.mp4"), ("GWT", f"out/real/{c}_gwt.mp4"), ("cleanplate", f"out/real/{c}_afuse3dg.crop.mkv")]:
        if os.path.exists(path):
            tile = cv2.resize(frame(path)[88:128, 184:256], None, fx=5, fy=5, interpolation=cv2.INTER_NEAREST)
        else:
            tile = np.full((200, 360, 3), 40, np.uint8); cv2.putText(tile, "skipped clip", (60, 110), 0, 0.9, (200, 200, 200), 2)
        cv2.putText(tile, lab, (6, 22), 0, 0.6, (255, 255, 255), 2); cv2.putText(tile, lab, (6, 22), 0, 0.6, (0, 0, 0), 1)
        cols += [tile, np.full((tile.shape[0], 6, 3), 255, np.uint8)]
    rows += [np.hstack(cols[:-1]), np.full((6, sum(x.shape[1] for x in cols[:-1]), 3), 255, np.uint8)]
cv2.imwrite("out/real/sheet.png", np.vstack(rows[:-1])); print("out/real/sheet.png")
