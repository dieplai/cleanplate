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
