"""Build a ground-truth benchmark clip from a real clip.

  clean = hflip+vflip(clip)            -> bottom-right now shows the clip's clean top-left
  marked = blend(clean, template)       -> J = (1-a) I + a*255 in RGB, at the real mark position
  encode marked AND clean with the same H.264 settings (libopenh264, source bitrate)
GT = clean before encoding. 'floor' = clean after encoding: the best any remover can do through this codec.
Only a crop around the mark is stored for GT (memory-safe streaming).

usage: synth.py clip.mp4 template.png right_margin bottom_margin outdir
"""
import sys, os, subprocess, json, numpy as np, cv2

clip, tpl_p, MR, MB, outdir = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4]), sys.argv[5]
os.makedirs(outdir, exist_ok=True)
name = os.path.splitext(os.path.basename(clip))[0]
s = json.loads(subprocess.check_output(["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries",
                                        "stream=width,height,r_frame_rate,bit_rate", "-of", "json", clip]))["streams"][0]
W, H, FPS, BR = s["width"], s["height"], s["r_frame_rate"], int(s["bit_rate"])
A = cv2.imread(tpl_p, cv2.IMREAD_GRAYSCALE).astype(np.float32) / 255.0
ys, xs = np.where(A > 0)
A = A[ys.min():ys.max() + 1, xs.min():xs.max() + 1]; th, tw = A.shape
mx, my = W - MR - tw, H - MB - th                      # mark top-left in frame
PAD = 16
cx0, cy0, cx1, cy1 = mx - PAD, my - PAD, mx + tw + PAD, my + th + PAD

def enc(path):
    return subprocess.Popen(["ffmpeg", "-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{W}x{H}", "-r", FPS,
                             "-i", "-", "-c:v", "libopenh264", "-b:v", str(BR), "-pix_fmt", "yuv420p", path], stdin=subprocess.PIPE)

dec = subprocess.Popen(["ffmpeg", "-v", "error", "-i", clip, "-vf", "hflip,vflip", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"],
                       stdout=subprocess.PIPE)
e_mark, e_clean = enc(f"{outdir}/{name}_marked.mp4"), enc(f"{outdir}/{name}_floor.mp4")
gt = []
a3 = A[..., None]
while True:
    b = dec.stdout.read(W * H * 3)
    if len(b) < W * H * 3: break
    I = np.frombuffer(b, np.uint8).reshape(H, W, 3)
    gt.append(I[cy0:cy1, cx0:cx1].copy())
    J = I.copy()
    region = J[my:my + th, mx:mx + tw].astype(np.float32)
    J[my:my + th, mx:mx + tw] = np.clip((1 - a3) * region + a3 * 255.0 + 0.5, 0, 255).astype(np.uint8)
    e_mark.stdin.write(J.tobytes()); e_clean.stdin.write(I.tobytes())
for e in (e_mark, e_clean): e.stdin.close(); e.wait()
np.save(f"{outdir}/{name}_gt.npy", np.array(gt))
json.dump({"W": W, "H": H, "mark": [int(mx), int(my), tw, th], "crop": [int(cx0), int(cy0), int(cx1), int(cy1)]},
          open(f"{outdir}/{name}_meta.json", "w"))
print(f"{name}: {W}x{H} frames={len(gt)} mark at ({mx},{my}) {tw}x{th}, bitrate={BR}")
