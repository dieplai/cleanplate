"""Ground-truth benchmark for still images.
Clean sources: a real Gemini image flipped so a mark-free quadrant sits under the mark position. The real V1 diamond
(alpha 0.50, captured on black) is composited at the generator's placement (96 px, margin 64), then delivered as
  png    : lossless, as downloaded from Gemini
  jpg90  : re-saved JPEG q90
  small  : resized to 60 % and JPEG q85 (a shared/forwarded copy; the mark is now ~58 px)
GT = clean image through the same geometry (no codec); floor = clean image through the same geometry AND codec.
usage: img_bench.py build SRC.png OUTDIR | score OUTDIR name=pattern ...   (pattern: {case} -> output file)"""
import sys, os, json, glob, numpy as np, cv2
def composite(I, A, x, y):
    J = I.astype(np.float32).copy(); a = A[..., None]; h, w = A.shape
    J[y:y + h, x:x + w] = (1 - a) * J[y:y + h, x:x + w] + a * 255.0
    return np.clip(J + 0.5, 0, 255).astype(np.uint8)
if sys.argv[1] == "build":
    src, od = sys.argv[2], sys.argv[3]; os.makedirs(od, exist_ok=True)
    S = cv2.imread(src)[..., :3]
    A = cv2.imread(os.path.join(os.path.dirname(__file__), "..", "assets", "marks", "google", "gemini-diamond-v1-96.png"), 0).astype(np.float32) / 255
    for tag, flip in (("brick", -1), ("kitchen", 0), ("neon", 1)):
        I = cv2.flip(S, flip); H, W = I.shape[:2]; x, y = W - 64 - 96, H - 64 - 96
        J = composite(I, A, x, y)
        for cond in ("png", "jpg90", "small"):
            case = f"{tag}_{cond}"
            if cond == "png":
                cv2.imwrite(f"{od}/{case}_marked.png", J); cv2.imwrite(f"{od}/{case}_gt.png", I); cv2.imwrite(f"{od}/{case}_floor.png", I)
                box = [x, y, 96, 96]; ext = "png"
            elif cond == "jpg90":
                q = [cv2.IMWRITE_JPEG_QUALITY, 90]
                cv2.imwrite(f"{od}/{case}_marked.jpg", J, q); cv2.imwrite(f"{od}/{case}_gt.png", I); cv2.imwrite(f"{od}/{case}_floor.jpg", I, q)
                box = [x, y, 96, 96]; ext = "jpg"
            else:
                s = 0.6; sz = (round(W * s), round(H * s)); q = [cv2.IMWRITE_JPEG_QUALITY, 85]
                Js, Is = cv2.resize(J, sz, interpolation=cv2.INTER_AREA), cv2.resize(I, sz, interpolation=cv2.INTER_AREA)
                cv2.imwrite(f"{od}/{case}_marked.jpg", Js, q); cv2.imwrite(f"{od}/{case}_gt.png", Is); cv2.imwrite(f"{od}/{case}_floor.jpg", Is, q)
                box = [int(x * s), int(y * s), int(np.ceil(96 * s)) + 1, int(np.ceil(96 * s)) + 1]; ext = "jpg"
            json.dump({"box": box, "ext": ext}, open(f"{od}/{case}_meta.json", "w"))
            print(case, box)
else:
    od = sys.argv[2]; methods = [a.split("=", 1) for a in sys.argv[3:]]
    try:
        import torch, lpips; net = lpips.LPIPS(net="alex", verbose=False)
        lp = lambda a, b: float(net(*(torch.from_numpy(x.astype(np.float32) / 127.5 - 1).permute(2, 0, 1)[None] for x in (a, b))))
    except Exception: lp = None
    print(f"{'case':16} {'method':12} {'PSNR':>7} {'excess':>7} {'LPIPS':>7}")
    for mf in sorted(glob.glob(f"{od}/*_meta.json")):
        case = os.path.basename(mf)[:-10]; m = json.load(open(mf)); x, y, w, h = m["box"]
        GT = cv2.imread(f"{od}/{case}_gt.png").astype(np.float32)
        P = 4; L = 16
        def sc(path):
            if not os.path.exists(path): return None
            V = cv2.imread(path)[..., :3].astype(np.float32)
            d = (V - GT)[y - P:y + h + P, x - P:x + w + P]; ps = 10 * np.log10(255 ** 2 / max((d ** 2).mean(), 1e-6))
            l = lp(V[y - L:y + h + L, x - L:x + w + L], GT[y - L:y + h + L, x - L:x + w + L]) if lp else float("nan")
            return ps, l
        fl = sc(f"{od}/{case}_floor.{m['ext']}"); fl = (min(fl[0], 60.0), fl[1])
        for name, pat in [("floor", None), ("marked", f"{od}/{case}_marked.{m['ext']}")] + methods:
            r = fl if name == "floor" else sc(pat.format(case=case) if "{" in pat else pat)
            if r is None: print(f"{case:16} {name:12} {'(no output)':>23}"); continue
            print(f"{case:16} {name:12} {r[0]:7.2f} {fl[0] - r[0]:7.2f} {r[1]:7.4f}")
