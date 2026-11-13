"""Self-check for research/image.py (no network, no ffmpeg): a synthetic textured image gets the real V1 diamond at
the Gemini still placement; the remover must restore it near-exactly and must refuse the clean image.
run: python3 tests/test_image.py"""
import os, subprocess, sys, tempfile
import numpy as np, cv2

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
rng = np.random.default_rng(7)
I = cv2.GaussianBlur(rng.uniform(0, 255, (1024, 1024, 3)).astype(np.float32), (0, 0), 2.0)
I = np.clip((I - I.mean()) * 3 + 120, 0, 255)
A = cv2.imread(os.path.join(ROOT, "assets/marks/google/gemini-diamond-v1-48.png"), 0).astype(np.float32) / 255
x = y = 1024 - 32 - 48                                    # 48 px at margin 32 (2/3 of the size)
J = I.copy(); J[y:y + 48, x:x + 48] = (1 - A[..., None]) * J[y:y + 48, x:x + 48] + A[..., None] * 255
with tempfile.TemporaryDirectory() as d:
    clean, marked, out = (os.path.join(d, n) for n in ("clean.png", "marked.png", "out.png"))
    cv2.imwrite(clean, np.round(I).astype(np.uint8)); cv2.imwrite(marked, np.round(J).astype(np.uint8))
    run = lambda a, b: subprocess.run([sys.executable, os.path.join(ROOT, "research/image.py"), a, b], capture_output=True, text=True)
    r = run(marked, out); assert r.returncode == 0, r.stdout + r.stderr
    R = cv2.imread(out).astype(np.float32); G = np.round(I)
    psnr = 10 * np.log10(255 ** 2 / ((R - G)[y - 4:y + 52, x - 4:x + 52] ** 2).mean())
    print(f"marked image: restored, PSNR in the mark box {psnr:.1f} dB")
    assert psnr > 45, psnr
    r = run(clean, os.path.join(d, "clean_out.png"))
    assert r.returncode != 0 and not os.path.exists(os.path.join(d, "clean_out.png")), "clean image must be refused"
    print("clean image: refused, untouched")
print("ok")
