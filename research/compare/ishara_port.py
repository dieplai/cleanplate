"""(MIT-licensed upstream: https://github.com/ishara-madu/gemini-watermark-remover)
Faithful Python port of the processing core of ishara-madu/gemini-watermark-remover (main.js, MIT), for benchmarking
with our ground truth without running the ad-carrying web page. Ported 1:1: getWatermarkInfo, getVeoWatermark,
getAlphaTemplateData/evaluateCandidateMatch, detectWatermarkCandidate / detectVideoWatermarkCandidate (with refinement
and fallback), resolveBox/getRoi/buildAlpha/removeWatermark. Canvas 'high' smoothing is approximated by OpenCV
INTER_AREA (down) / INTER_CUBIC (up). Video: detection on the first frame (their preview frame), the same box for every
frame, output written LOSSLESS here (their app re-encodes H.264), which can only flatter them.
usage: ishara_port.py image in.png out.png | video in.mp4 out.mkv"""
import sys, subprocess, json, numpy as np, cv2

import os
BG96 = cv2.imread(os.environ.get("ISHARA_BG96", "repos/ishara-madu__gemini-watermark-remover/assets/bg_96.png"))
BG96A = BG96.max(2).astype(np.float32) / 255.0

def resize_alpha(size):
    return cv2.resize(BG96A, (size, size), interpolation=cv2.INTER_AREA if size < 96 else cv2.INTER_CUBIC)

def get_wm_info(w, h):
    r = min(w, h) / 1536; size = max(16, round(96 * r)); m = max(8, round(64 * r))
    return dict(size=size, x=max(0, w - m - size), y=max(0, h - m - size))

def get_veo_wm(w, h):
    b = min(w, h); size = max(24, min(round(b / 15), b)); m = round(b / 10)
    return dict(size=size, x=max(0, w - m - size), y=max(0, h - m - size))

def evaluate(img, w, h, box):
    x, y, size = box
    if x < 0 or y < 0 or x + size > w or y + size > h or size <= 0: return -1
    A = resize_alpha(size); gA = np.zeros_like(A)
    gx = np.zeros_like(A); gy = np.zeros_like(A)
    gx[:, 1:-1] = A[:, 2:] - A[:, :-2]; gy[1:-1, :] = A[2:, :] - A[:-2, :]; gA = np.hypot(gx, gy)
    step = 2 if size > 80 else 1
    L = (0.299 * img[..., 2] + 0.587 * img[..., 1] + 0.114 * img[..., 0]).astype(np.float64)
    rr = np.arange(0, size, step); cc = np.arange(0, size, step)
    Lp = L[y + rr[:, None], x + cc[None, :]]; Ap = A[rr[:, None], cc[None, :]].astype(np.float64)
    n = Lp.size; mL, mA = Lp.mean(), Ap.mean(); vL = max(0, (Lp ** 2).mean() - mL ** 2); vA = max(0, (Ap ** 2).mean() - mA ** 2)
    if vA <= 1e-4: return 0
    nw = ((Lp * Ap).mean() - mL * mA) / np.sqrt(vL * vA) if vL > 0.5 else 0
    IL = 255 - Lp; mI = IL.mean(); vI = max(0, (IL ** 2).mean() - mI ** 2)
    nd = ((IL * Ap).mean() - mI * mA) / np.sqrt(vI * vA) if (vI > 0.5 and mL > 160) else 0
    nl = max(nw, nd)
    # gradient part (inner pixels only)
    R, C = np.meshgrid(rr, cc, indexing="ij"); m = (R > 0) & (R < size - 1) & (C > 0) & (C < size - 1)
    IR, IC = y + R[m], x + C[m]; ok = (IR > 0) & (IR < h - 1) & (IC > 0) & (IC < w - 1); IR, IC = IR[ok], IC[ok]
    G = np.hypot(L[IR, IC + 1] - L[IR, IC - 1], L[IR + 1, IC] - L[IR - 1, IC]); GA = gA[R[m][ok], C[m][ok]].astype(np.float64)
    ng = 0
    if G.size > 10:
        vG = max(0, (G ** 2).mean() - G.mean() ** 2); vGA = max(0, (GA ** 2).mean() - GA.mean() ** 2)
        if vG > 0.5 and vGA > 1e-4: ng = max(0, ((G * GA).mean() - G.mean() * GA.mean()) / np.sqrt(vG * vGA))
    f = nl * 0.65 + ng * 0.35
    if vL < 20: f = max(f, nl * 0.40 + ng * 0.60)
    return max(0, f)

def refine(img, w, h, best, prior, fine):
    rx, ry, rs, sc = best["x"], best["y"], best["size"], best["score"]
    for ts in sorted(set(fine)):
        for dy in range(-16, 17, 4):
            for dx in range(-16, 17, 4):
                tx = max(0, min(w - ts, best["x"] + dx)); ty = max(0, min(h - ts, best["y"] + dy))
                s = evaluate(img, w, h, (tx, ty, ts)) * prior
                if s > sc: rx, ry, rs, sc = tx, ty, ts, s
    return rx, ry, rs, sc

def detect_image(img):
    h, w = img.shape[:2]; md = min(w, h); br = md / 1536; base = get_wm_info(w, h)
    fams = [(base["size"], lambda s: (max(8, round(192 * br)),) * 2, 0.6, 1.08),
            (base["size"], lambda s: (max(8, round(64 * br)),) * 2, 1.0, 1.04),
            (96, lambda s: ((192 if md >= 1400 else round(128 * max(0.5, md / 1024))),) * 2, 0.6, 1.02),
            (96, lambda s: ((64 if md >= 1024 else 32),) * 2, 1.0, 1.01)]
    best = None
    for bs, mp, gain, prior in fams:
        for scl in [0.55, 0.70, 0.85, 1.00, 1.15, 1.30, 1.50, 1.70]:
            s = max(16, min(round(bs * scl), md - 8)); m = mp(s)[0]; x, y = max(0, w - m - s), max(0, h - m - s)
            sc = evaluate(img, w, h, (x, y, s)) * prior
            if best is None or sc > best["score"]: best = dict(size=s, x=x, y=y, score=sc, gain=gain, prior=prior)
    if best["score"] > 0.05:
        fine = [max(16, round(best["size"] * k)) for k in (0.90, 0.95)] + [best["size"]] + [min(md - 8, round(best["size"] * k)) for k in (1.05, 1.10)]
        rx, ry, rs, sc = refine(img, w, h, best, best["prior"], fine)
        return dict(x=rx, y=ry, size=rs, gain=best["gain"], score=sc, found=sc >= 0.10)
    off = round(-128 * br)                       # fallback: they still clean
    return dict(x=base["x"] + off, y=base["y"] + off, size=base["size"], gain=0.6, score=0, found=False)

def detect_video(img):
    h, w = img.shape[:2]; bd = min(w, h); vb = get_veo_wm(w, h); bx, by = vb["x"], vb["y"]
    ao = round(-24 * (bd / 720))
    fams = [(vb["size"], lambda s: (max(0, min(w - s, bx + ao)), max(0, min(h - s, by + ao))), 1.06),
            (vb["size"], lambda s: (max(0, min(w - s, bx)), max(0, min(h - s, by))), 1.02),
            (max(24, round(96 * (bd / 1536))), lambda s: (max(0, w - max(16, round(192 * (bd / 1536))) - s),
                                                          max(0, h - max(16, round(192 * (bd / 1536))) - s)), 1.01)]
    best = None
    for bs, pos, prior in fams:
        for scl in [0.65, 0.85, 1.00, 1.20, 1.45]:
            s = max(16, min(round(bs * scl), bd - 8)); x, y = pos(s)
            sc = evaluate(img, w, h, (x, y, s)) * prior
            if best is None or sc > best["score"]: best = dict(size=s, x=x, y=y, score=sc, prior=prior)
    if best["score"] > 0.05:
        fine = [max(16, round(best["size"] * 0.92)), best["size"], min(bd - 8, round(best["size"] * 1.08))]
        rx, ry, rs, sc = refine(img, w, h, best, best["prior"], fine)
        return dict(x=rx, y=ry, size=rs, gain=0.6, score=sc, found=sc >= 0.08)
    return dict(x=bx + ao, y=by + ao, size=vb["size"], gain=0.6, score=0, found=False)

def clean(img, d):
    out = img.astype(np.float32).copy(); s, x, y = d["size"], d["x"], d["y"]
    a = np.minimum(resize_alpha(s) * d["gain"], 0.99); m = a >= 0.002
    reg = out[y:y + s, x:x + s]; a3 = a[..., None]
    rec = np.clip(np.round((reg - a3 * 255.0) / (1 - a3)), 0, 255)
    reg[m] = rec[m]; return out.astype(np.uint8)

if __name__ == "__main__":
    mode, inp, outp = sys.argv[1:4]
    if mode == "image":
        img = cv2.imread(inp)[..., :3]; d = detect_image(img); print("detect", d)
        cv2.imwrite(outp, clean(img, d), [cv2.IMWRITE_JPEG_QUALITY, 95] if outp.endswith(".jpg") else [])
    else:
        s = json.loads(subprocess.check_output(["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height,r_frame_rate", "-of", "json", inp]))["streams"][0]
        W, H = s["width"], s["height"]
        dec = subprocess.Popen(["ffmpeg", "-v", "error", "-i", inp, "-f", "rawvideo", "-pix_fmt", "bgr24", "-"], stdout=subprocess.PIPE)
        enc = subprocess.Popen(["ffmpeg", "-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "bgr24", "-s", f"{W}x{H}", "-r", s["r_frame_rate"], "-i", "-",
                                "-c:v", "ffv1", "-pix_fmt", "yuv420p", outp], stdin=subprocess.PIPE)
        d = None
        while True:
            b = dec.stdout.read(W * H * 3)
            if len(b) < W * H * 3: break
            f = np.frombuffer(b, np.uint8).reshape(H, W, 3)
            if d is None: d = detect_video(f); print("detect", d)
            enc.stdin.write(clean(f, d).tobytes())
        enc.stdin.close(); enc.wait()
