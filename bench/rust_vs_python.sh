#!/bin/bash
# Acceptance gate for the Rust port: same ground-truth clips as bench/baseline_python.txt.
# Gates (docs/superpowers/plans/2026-11-13-rust-core-cli.md): per clip PSNR >= python - 0.10 dB, mean >= python mean,
# ghost <= python + 0.5; flower and teacup <= 15 s wall; peak RSS <= 1 GB. Exit 1 on any violation.
set -u
REPO="$(cd "$(dirname "$0")/.." && pwd)"
cargo build --release -q -p cleanplate --manifest-path "$REPO/Cargo.toml" || exit 1
BIN="$REPO/target/release/cleanplate"
cd "$REPO/.."
P=cleanplate/research
R=out/bench/rust_vs_python.txt; : > $R
for sc in bench/veo31fast_flower_720p bench/night_720p bench/snow_720p bench/teacup_static_720p bench/kitchen_9x16 \
          bench_diamond/veo31fast_flower_720p bench_diamond/night_720p bench_diamond/kitchen_9x16; do
  set_=${sc%%/*}; c=${sc#*/}
  /usr/bin/time -f "resources ${set_}/${c} %e s %M KB" -o out/bench/time.tmp "$BIN" ${sc}_marked.mp4 out/bench/${set_}_${c}_rs.crop.mkv > out/bench/${set_}_${c}_rs.log 2>&1
  cat out/bench/time.tmp >> $R
  python3 $P/score_gt.py $sc python=out/bench/${set_}_${c}_py.crop.mkv rust=out/bench/${set_}_${c}_rs.crop.mkv | sed "1s|^|${set_}/|" >> $R
done
# stills: 9 ground-truth cases (bench_img), same scorer as the Python baseline
O=out/img_bench
for m in bench_img/*_meta.json; do
  c=$(basename $m _meta.json); ext=$(python3 -c "import json;print(json.load(open('$m'))['ext'])")
  # both write lossless PNG so the comparison is the algorithm, not two JPEG libraries (libjpeg vs pure-Rust encoders
  # differ by up to 0.3 dB on textured content; with identical decoded input and PNG output Rust == Python exactly)
  rm -f $O/${c}_rsp.png $O/${c}_pyp.png
  "$BIN" bench_img/${c}_marked.$ext $O/${c}_rsp.png > /dev/null 2>&1
  python3 $P/image.py bench_img/${c}_marked.$ext $O/${c}_pyp.png > /dev/null 2>&1
done
tools/venv/bin/python $P/img_bench.py score bench_img python=$O/{case}_pyp.png rust=$O/{case}_rsp.png 2>/dev/null | grep -E "_png|jpg90|small" >> $R
cat $R
python3 - "$R" <<'PY'
import re, sys
txt = open(sys.argv[1]).read(); bad = []; py, rs = [], []
for blk in re.split(r"\n(?=bench)", txt):
    if not blk.startswith("bench"): continue
    name = blk.split()[0]
    m = {k: (float(a), float(g)) for k, a, g in re.findall(r"^\s+(python|rust)\s+([\d.]+)\s+[\d.-]+\s+([\d.]+)", blk, re.M)}
    if "python" not in m or "rust" not in m: bad.append(f"{name}: missing scores"); continue
    (pp, pg), (rp, rg) = m["python"], m["rust"]; py.append(pp); rs.append(rp)
    if rp < pp - 0.10: bad.append(f"{name}: PSNR {rp:.2f} < python {pp:.2f} - 0.10")
    if rg > pg + 0.5: bad.append(f"{name}: ghost {rg:.2f} > python {pg:.2f} + 0.5")
for name, sec, kb in re.findall(r"resources (\S+) ([\d.]+) s (\d+) KB", txt):
    if int(kb) > 1_000_000: bad.append(f"{name}: peak RSS {int(kb)/1e3:.0f} MB > 1 GB")
    if ("flower" in name and name.startswith("bench/")) or "teacup" in name:
        if float(sec) > 15: bad.append(f"{name}: {sec} s > 15 s")
st = {}
for case, meth, rest in re.findall(r"^(\w+_(?:png|jpg90|small))\s+(python|rust)\s+(.*)$", txt, re.M):
    st.setdefault(case, {})[meth] = None if "no output" in rest else float(rest.split()[0])
for case, d in sorted(st.items()):
    p_, r_ = d.get("python"), d.get("rust")
    if (p_ is None) != (r_ is None): bad.append(f"still {case}: python {p_} vs rust {r_} (refusal mismatch)")
    elif p_ is not None and r_ < p_ - 0.05: bad.append(f"still {case}: {r_:.2f} < python {p_:.2f} - 0.05")
print(f"stills compared: {len(st)}")
if py and sum(rs) / len(rs) < sum(py) / len(py): bad.append(f"mean PSNR {sum(rs)/len(rs):.2f} < python {sum(py)/len(py):.2f}")
print(f"\nmean PSNR: python {sum(py)/max(len(py),1):.2f}  rust {sum(rs)/max(len(rs),1):.2f}")
print("GATE " + ("PASS" if not bad else "FAIL:\n  " + "\n  ".join(bad)))
sys.exit(1 if bad else 0)
PY
