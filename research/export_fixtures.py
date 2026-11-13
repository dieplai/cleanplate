"""Write golden fixtures (.npy) for the Rust core's primitive tests: the exact OpenCV calls research/pipeline.py and
research/image.py make, on small deterministic inputs. usage: python3 research/export_fixtures.py"""
import os, numpy as np, cv2
OUT = os.path.join(os.path.dirname(__file__), "..", "crates", "cleanplate-core", "tests", "fixtures")
os.makedirs(OUT, exist_ok=True)
def save(name, a): np.save(os.path.join(OUT, name + ".npy"), np.ascontiguousarray(a))
rng = np.random.default_rng(1)
a = cv2.GaussianBlur(rng.uniform(0, 255, (29, 37)).astype(np.float32), (0, 0), 1.2)   # smooth-ish, h=29 w=37
b = rng.uniform(0, 255, (48, 64)).astype(np.float32)
save("in_a", a); save("in_b", b)
for s in (1.0, 3.0):
    save(f"gauss_a_{s:g}", cv2.GaussianBlur(a, (0, 0), s)); save(f"gauss_b_{s:g}", cv2.GaussianBlur(b, (0, 0), s))
save("sobel_x_b", cv2.Sobel(b, cv2.CV_32F, 1, 0)); save("sobel_y_b", cv2.Sobel(b, cv2.CV_32F, 0, 1))
m = (rng.uniform(0, 1, (29, 37)) > 0.93).astype(np.uint8); save("mask_a", m)
for k in (3, 5, 9):
    save(f"dilate_{k}", cv2.dilate(m, np.ones((k, k), np.uint8)))
def harmonic(F, hole, iters=150):   # verbatim from research/pipeline.py
    G = F.copy(); G[hole] = F[~hole].mean()
    for _ in range(iters):
        Pd = np.pad(G, 1, mode="edge")
        G[hole] = ((Pd[:-2, 1:-1] + Pd[2:, 1:-1] + Pd[1:-1, :-2] + Pd[1:-1, 2:]) / 4)[hole]
    return G
hole = np.zeros((29, 37), bool); hole[8:20, 10:27] = True; save("hole_a", hole.astype(np.uint8))
save("harmonic_a", harmonic(a, hole))
save("area_b_32x24", cv2.resize(b, (32, 24), interpolation=cv2.INTER_AREA))
t = rng.uniform(0, 1, (96, 96)).astype(np.float32); save("in_t96", t)
save("area_t_58", cv2.resize(t, (58, 58), interpolation=cv2.INTER_AREA))
save("cubic_a_56x44", cv2.resize(a, (56, 44), interpolation=cv2.INTER_CUBIC))
mx = (np.mgrid[0:29, 0:37][1] + rng.uniform(-3, 3, (29, 37))).astype(np.float32)
my = (np.mgrid[0:29, 0:37][0] + rng.uniform(-3, 3, (29, 37))).astype(np.float32)
save("map_x", mx); save("map_y", my)
for interp, iname in ((cv2.INTER_LINEAR, "linear"), (cv2.INTER_CUBIC, "cubic")):
    save(f"remap_{iname}_rep", cv2.remap(a, mx, my, interp, borderMode=cv2.BORDER_REPLICATE))
    save(f"remap_{iname}_const", cv2.remap(a, mx, my, interp, borderMode=cv2.BORDER_CONSTANT, borderValue=0))
print("fixtures ->", os.path.abspath(OUT), len(os.listdir(OUT)), "files")

# ---- Task 2: template matching, resize by factor, video detection (inputs: 3 corner frames per clip)
tpl = b[5:17, 6:26].copy(); save("tpl_b", tpl)
save("match_b", cv2.matchTemplate(b, tpl, cv2.TM_CCOEFF_NORMED))
v = cv2.imread(os.path.join(os.path.dirname(__file__), "..", "assets", "marks", "google", "veo-small-720p.png"), 0).astype(np.float32) / 255
ys, xs = np.where(v > 0); v = v[ys.min():ys.max() + 1, xs.min():xs.max() + 1]; save("veo_trim", v)
save("veo_fx1_5", cv2.resize(v, None, fx=1.5, fy=1.5, interpolation=cv2.INTER_CUBIC))
save("veo_fx0_667", cv2.resize(v, None, fx=0.667, fy=0.667, interpolation=cv2.INTER_AREA))
import glob, subprocess
BENCH = os.path.join(os.path.dirname(__file__), "..", "..")
MARKS = os.path.join(os.path.dirname(__file__), "..", "assets", "marks")
KNOWN = {"veo-small-720p.png": [(16, 16)], "veo-small-1080p.png": [(24, 24)], "gemini-diamond-48.png": [(72, 72), (96, 96)],
         "gemini-diamond-96.png": [(64, 64), (192, 192)], "gemini-diamond-v1-48.png": [(32, 32), (72, 72), (96, 96)],
         "gemini-diamond-v1-96.png": [(64, 64), (192, 192)], "gemini-diamond-36.png": [(32, 32)],
         "veo-legacy-68.png": [(17, 18)]}
def detect(Yc, W, H, CW, CH, min_ncc=0.5):   # verbatim logic of research/pipeline.py lines 93-125
    N = len(Yc); step = max(1, N // 120); samp = Yc[::step].astype(np.float32)
    hp = (samp - np.array([cv2.GaussianBlur(s, (0, 0), 3) for s in samp])).mean(0)
    best = None
    for f in sorted(glob.glob(os.path.join(MARKS, "*", "*.png"))):
        A0 = cv2.imread(f, cv2.IMREAD_GRAYSCALE).astype(np.float32) / 255.0
        ys, xs = np.where(A0 > 0); A0 = A0[ys.min():ys.max() + 1, xs.min():xs.max() + 1]
        for sc in sorted({1.0, round(min(W, H) / 720, 3)}):
            A = cv2.resize(A0, None, fx=sc, fy=sc, interpolation=cv2.INTER_AREA if sc < 1 else cv2.INTER_CUBIC) if sc != 1 else A0
            if A.shape[0] >= CH or A.shape[1] >= CW or min(A.shape) < 6: continue
            t = A - cv2.GaussianBlur(A, (0, 0), 3)
            r = cv2.matchTemplate(hp, t, cv2.TM_CCOEFF_NORMED); _, sc_, _, loc = cv2.minMaxLoc(r)
            if best is None or sc_ > best[0]: best = (sc_, os.path.basename(f), sc, loc, A)
    ncc, name, sc, (mx, my), A = best
    mr, mb = CW - (mx + A.shape[1]), CH - (my + A.shape[0])
    known = any(abs(mr - r * sc) <= 3 and abs(mb - b * sc) <= 3 for r, b in KNOWN.get(name, []))
    floor_ = 0.45 if known else max(min_ncc, 0.75)
    return dict(name=name, scale=sc, x=mx, y=my, ncc=float(ncc), known=known, floor=floor_, ok=ncc >= floor_)
SEL3 = "select='eq(n\\,0)+eq(n\\,60)+eq(n\\,120)'"
SEL8 = "select='not(mod(n\\,24))'"
for tag, clip, sel in (("veo", "bench/veo31fast_flower_720p_marked.mp4", SEL3), ("diamond", "bench_diamond/veo31fast_flower_720p_marked.mp4", SEL8),
                       ("clean", "bench/veo31fast_flower_720p_floor.mp4", SEL3)):
    raw = subprocess.run(["ffmpeg", "-v", "error", "-i", os.path.join(BENCH, clip), "-vf", sel,
                          "-vsync", "0", "-f", "rawvideo", "-pix_fmt", "gray", "-"], capture_output=True).stdout
    fr = np.frombuffer(raw, np.uint8).reshape(-1, 720, 1280)[:, 720 - 320:, 1280 - 480:]
    save(f"corners_{tag}", fr)
    d = detect(fr, 1280, 720, 480, 320)
    save(f"det_{tag}", np.array([d["x"], d["y"], d["ncc"], d["scale"], float(d["ok"]), float(d["known"])], np.float32))
    print(tag, d)

# ---- Task 3: DIS optical flow (OpenCV PRESET_MEDIUM) on real region-sized crops + a synthetic translation
dis = cv2.DISOpticalFlow_create(cv2.DISOPTICAL_FLOW_PRESET_MEDIUM)
def luma(clip, frames):
    sel = "+".join(f"eq(n\\,{f})" for f in frames)
    raw = subprocess.run(["ffmpeg", "-v", "error", "-i", os.path.join(BENCH, clip), "-vf", f"select='{sel}'", "-vsync", "0",
                          "-f", "rawvideo", "-pix_fmt", "gray", "-"], capture_output=True).stdout
    return np.frombuffer(raw, np.uint8).reshape(-1, 720, 1280)
k = 0
for clip in ("bench/veo31fast_flower_720p_floor.mp4", "bench/teacup_static_720p_floor.mp4"):
    fr = luma(clip, [50, 51, 54, 62])
    for s in (1, 2, 3):                    # offsets 1, 4, 12
        a0 = np.ascontiguousarray(fr[0, 620:720, 1140:1280]); a1 = np.ascontiguousarray(fr[s, 620:720, 1140:1280])
        save(f"flow_i0_{k}", a0); save(f"flow_i1_{k}", a1); save(f"flow_cv_{k}", dis.calc(a0, a1, None)); k += 1
tex = cv2.GaussianBlur(np.random.default_rng(5).uniform(0, 255, (100, 140)).astype(np.float32), (0, 0), 2.0)
tex = np.clip((tex - tex.mean()) * 4 + 128, 0, 255)
gy_, gx_ = np.mgrid[0:100, 0:140].astype(np.float32)
sh = cv2.remap(tex, gx_ - 1.3, gy_ + 0.7, cv2.INTER_CUBIC, borderMode=cv2.BORDER_REFLECT)   # I1(x) = I0(x - d), d = (1.3, -0.7)
t0, t1 = np.round(tex).astype(np.uint8), np.round(np.clip(sh, 0, 255)).astype(np.uint8)
save("shift_i0", t0); save("shift_i1", t1)
f = dis.calc(t0, t1, None); e = np.hypot(f[..., 0] - 1.3, f[..., 1] + 0.7)[10:-10, 10:-10]
print("opencv synthetic EPE median %.4f p90 %.4f" % (np.median(e), np.percentile(e, 90)))

# ---- Task 4: weighted-TV MAP refinement (verbatim research/pipeline.py tv_refine)
def tv_refine(mu, w, mask, lam, iters=300, eps=1.0):
    I = mu.copy()
    tau = 0.9 / (2 * w + 8 * lam / eps)
    for _ in range(iters):
        gx = np.diff(I, axis=1, append=I[:, -1:]); gy = np.diff(I, axis=0, append=I[-1:, :])
        n = np.sqrt(gx ** 2 + gy ** 2 + eps ** 2)
        px, py = gx / n, gy / n
        div = px - np.roll(px, 1, axis=1) + py - np.roll(py, 1, axis=0)
        I[mask] -= (tau * (2 * w * (I - mu) - lam * div))[mask]
    return I
r4 = np.random.default_rng(4)
tmu = r4.uniform(0, 255, (30, 40)).astype(np.float32); tw = r4.uniform(0.05, 48, (30, 40)).astype(np.float32)
tmask = np.zeros((30, 40), bool); tmask[5:25, 6:34] = True
tlam = (16.0 * (r4.uniform(0, 1, (30, 40)) > 0.5)).astype(np.float32)
save("tv_mu", tmu); save("tv_w", tw); save("tv_mask", tmask.astype(np.uint8)); save("tv_lam", tlam)
save("tv_out", tv_refine(tmu, tw, tmask, tlam).astype(np.float32))
