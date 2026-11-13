"""Render docs/assets/overview.png — the one-page results overview used in the README.
Every number comes from docs/benchmark.md (ground-truth tables) or the real-clip checks listed there.
usage: python3 docs/make_overview.py"""
import os
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.patches import FancyBboxPatch

plt.rcParams.update({"font.family": ["Liberation Serif", "DejaVu Sans"], "font.size": 10.5, "axes.spines.top": False,
                     "axes.spines.right": False, "axes.edgecolor": "#444", "axes.linewidth": 0.8})
OTHER, OTHER2, CP, GOOD, BAD, INK, MUTED = "#C9CED6", "#9AA3AF", "#2B7BD6", "#1E9E61", "#D64545", "#1F2328", "#6B7280"

fig = plt.figure(figsize=(16, 13.2), dpi=130)
fig.text(0.5, 0.972, "cleanplate  ·  results overview", ha="center", fontsize=25, weight="bold", color=INK)
fig.text(0.5, 0.947, "ground truth = the clean footage the mark was composited on   ·   "
         "floor / ceiling = the same clean footage through the same codec (no remover can beat it)",
         ha="center", fontsize=11, color=MUTED)

def title(ax, n, s, y=1.12):
    ax.text(-0.02, y, f"{n}  {s}", transform=ax.transAxes, fontsize=15.5, weight="bold", color=INK, ha="left")

# ① video, ground truth (PSNR in the mark box, dB)
ax = fig.add_axes([0.07, 0.585, 0.40, 0.30])
clips = ["Flower market\n(moving)", "Night street\n(dark)", "Snow\n(near-white)", "Teacup\n(static)", "Kitchen 9:16\n(static wall)"]
gwt = [28.48, 33.50, 35.79, 33.18, 31.80]; cp = [31.93, 39.92, 41.53, 36.94, 36.15]; floor_ = [32.52, 40.50, 45.53, 38.84, 41.89]
y = range(len(clips)); h = 0.36
ax.barh([i + h / 2 for i in y], gwt, h, color=OTHER, label="GWT 0.6.5")
ax.barh([i - h / 2 for i in y], cp, h, color=CP, label="cleanplate")
for i in y:
    ax.plot([floor_[i]] * 2, [i - 0.45, i + 0.45], color=INK, lw=1.2, ls=(0, (2, 2)))
    ax.text(gwt[i] - 0.3, i + h / 2, f"{gwt[i]:.1f}", va="center", ha="right", fontsize=9, color=INK)
    ax.text(cp[i] - 0.3, i - h / 2, f"{cp[i]:.1f}", va="center", ha="right", fontsize=9, color="white", weight="bold")
    ax.text(47.2, i - h / 2, f"+{cp[i] - gwt[i]:.1f} dB", va="center", fontsize=11, color=GOOD, weight="bold")
ax.set_yticks(list(y)); ax.set_yticklabels(clips); ax.invert_yaxis(); ax.set_xlim(25, 47)
ax.set_xlabel("PSNR inside the mark (dB) — dashed line: floor (clean clip, same encoder)", color=MUTED)
ax.legend(loc="lower right", bbox_to_anchor=(1.0, 1.0), ncol=2, frameon=False)
title(ax, "①", "Video, Veo mark, 720p: ground truth")

# ② stills, ground truth
ax = fig.add_axes([0.58, 0.585, 0.38, 0.30])
cases = ["Brick\nPNG", "Neon\nPNG", "Brick\nJPEG q90", "Neon\nJPEG q90", "Brick\nresized+JPEG", "Neon\nresized+JPEG"]
G = [43.4, 40.2, 32.7, 36.0, None, None]; Wm = [56.4, 56.4, 33.0, 37.3, None, 15.5]; C = [56.4, 56.5, 33.0, 40.8, 30.4, 34.1]
ceil = [60, 60, 35.4, 42.3, 32.3, 43.9]
w = 0.26
for i in range(len(cases)):
    for k, (v, col, lab) in enumerate(((G[i], OTHER, "GWT"), (Wm[i], OTHER2, "wmr"), (C[i], CP, "cleanplate"))):
        x = i + (k - 1) * w
        if v is None:
            ax.text(x, 1.0, "✗", ha="center", va="bottom", fontsize=12, color=BAD, weight="bold")
        else:
            ax.bar(x, v, w, color=col, label=lab if i == 0 else None)
    ax.plot([i - 0.42, i + 0.42], [ceil[i]] * 2, color=INK, lw=1.2, ls=(0, (2, 2)))
ax.text(5, 16.2, "wrong\nmark", ha="center", va="bottom", fontsize=7.5, color=BAD)
ax.text(0.99, 0.80, "✗ = mark not detected", transform=ax.transAxes, ha="right", fontsize=9, color=BAD)
ax.set_xticks(range(len(cases))); ax.set_xticklabels(cases, fontsize=9); ax.set_ylim(0, 63)
ax.set_ylabel("PSNR inside the mark (dB)", color=MUTED)
ax.legend(loc="upper right", ncol=3, frameon=False, fontsize=9.5)
title(ax, "②", "Stills, Gemini ✦: ground truth")

# ③ real files, no ground truth: removed cleanly / total
ax = fig.add_axes([0.07, 0.11, 0.47, 0.37]); ax.axis("off")
title(ax, "③", "Real files: removed with no visible trace", y=1.02)
rows = [("Veo 720p clips (flower, night, snow, teacup, kitchen)", "3/5", "—", "5/5", "GWT skips snow, leaves the teacup mark"),
        ("Film-style Veo 720p + Gemini-app ✦ video", "5/5", "—", "5/5", "GWT: faint ✦ outline in the time average"),
        ("Legacy \"Veo\" text sample (68×30)", "0/1", "0/1", "1/1*", "*faint glyph texture on one static shot; wmr leaves more, GWT fails"),
        ("Flow 1080p downloads (AI-upscaled mark)", "0/3", "—", "3/3*", "*with the clip's 720p as --guide"),
        ("Gemini still (V1 ✦, PNG)", "1/1", "1/1", "1/1", "all three clean"),
        ("Flow stills (Nano Banana, faint ✦ 48 px)", "0/4", "0/4", "4/4", "GWT: 2 skipped, 2 wrong / outline"),
        ("Same Flow stills after a Zalo round trip", "0/4", "0/4", "4/4", "re-encoded JPEG ≈ q85")]
cols = [0.0, 0.53, 0.62, 0.71, 0.80]
for j, hd in enumerate(["", "GWT", "wmr", "clean-\nplate", ""]):
    ax.text(cols[j] + 0.035, 0.95, hd, ha="center", va="bottom", fontsize=10.5, weight="bold", color=INK)
for r, (name, g, wm, c, note) in enumerate(rows):
    yy = 0.86 - r * 0.125
    if r % 2 == 0: ax.add_patch(FancyBboxPatch((-0.01, yy - 0.05), 1.02, 0.1, boxstyle="round,pad=0.003", fc="#F4F6F9", ec="none"))
    ax.text(0, yy, name, va="center", fontsize=10.2, color=INK)
    for j, v in ((1, g), (2, wm), (3, c)):
        full = v not in ("—",) and v.split("/")[0].rstrip("*") == v.split("/")[1].rstrip("*")
        col = MUTED if v == "—" else (CP if j == 3 and full else (GOOD if full else BAD))
        ax.text(cols[j] + 0.035, yy, v, ha="center", va="center", fontsize=11, color=col, weight="bold")
    ax.text(0.0, yy - 0.042, note, va="center", fontsize=8.3, color=MUTED)
ax.text(0, -0.05, "— = not run.  Judged on 4× contrast-stretched zooms and time-averaged ghost maps; sheets in docs/benchmark.md.",
        fontsize=8.3, color=MUTED, transform=ax.transAxes)

# ④ + ⑤ safety and verification checklist
ax = fig.add_axes([0.60, 0.11, 0.37, 0.37]); ax.axis("off")
title(ax, "④", "Safety and checks", y=1.02)
items = [("✓", GOOD, "14 / 14 clean inputs left untouched", "5 clean videos + 9 clean stills: detector refuses"),
         ("✓", GOOD, "Opacity measured, not assumed", "0.498–0.504 measured vs 0.498 true (Gemini ships 0.30 and 0.50)"),
         ("✓", GOOD, "Native YUV, H.264 chroma siting", "no RGB round trip; 13–22 % less colour leftover"),
         ("+", CP, "Audio, colour tags, ProRes / AV1 out", "everything outside the mark is the source"),
         ("!", BAD, "Static shots are information-limited", "nothing ever sees the background: MAP estimate"),
         ("!", BAD, "AI-upscaled 1080p re-draws the mark", "use the native 720p download for the best result")]
for i, (sym, col, head, sub) in enumerate(items):
    yy = 0.88 - i * 0.155
    ax.text(0.0, yy, sym, fontsize=17, color=col, weight="bold", va="center")
    ax.text(0.07, yy + 0.012, head, fontsize=11.5, color=INK, va="center", weight="bold")
    ax.text(0.07, yy - 0.05, sub, fontsize=9, color=MUTED, va="center")

# footer: the boundary
fig.patches.append(FancyBboxPatch((0.06, 0.022), 0.88, 0.045, boxstyle="round,pad=0.006", transform=fig.transFigure,
                                  fc="#F4F6F9", ec="#C9CED6", lw=1))
fig.text(0.5, 0.0445, "Boundary:  visible marks only   ·   SynthID and C2PA provenance are never touched   ·   "
         "nothing is generated: every pixel comes from the video or the image itself",
         ha="center", va="center", fontsize=11, color=INK)

out = os.path.join(os.path.dirname(__file__), "assets", "overview.png")
os.makedirs(os.path.dirname(out), exist_ok=True)
fig.savefig(out, facecolor="white"); print(out)
