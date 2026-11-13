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
if py and sum(rs) / len(rs) < sum(py) / len(py): bad.append(f"mean PSNR {sum(rs)/len(rs):.2f} < python {sum(py)/len(py):.2f}")
print(f"\nmean PSNR: python {sum(py)/max(len(py),1):.2f}  rust {sum(rs)/max(len(rs),1):.2f}")
print("GATE " + ("PASS" if not bad else "FAIL:\n  " + "\n  ".join(bad)))
sys.exit(1 if bad else 0)
PY
