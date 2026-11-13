#!/bin/bash
# Freeze the Python reference numbers (quality + wall time) that the Rust port is gated against.
# Bench data lives next to the repo (gemini-wm/bench*, built by research/synth.py and research/img_bench.py build).
set -u
cd "$(dirname "$0")/../.."
C="systemd-run --user --scope -q -p MemoryMax=4G -p MemorySwapMax=0"
P=cleanplate/research
for sc in bench/veo31fast_flower_720p bench/night_720p bench/snow_720p bench/teacup_static_720p bench/kitchen_9x16 \
          bench_diamond/veo31fast_flower_720p bench_diamond/night_720p bench_diamond/kitchen_9x16; do
  set_=${sc%%/*}; c=${sc#*/}; out=out/bench/${set_}_${c}_py.crop.mkv
  t0=$(date +%s.%N)
  $C python3 $P/pipeline.py ${sc}_marked.mp4 $out > /dev/null 2>&1
  t1=$(date +%s.%N)
  python3 $P/score_gt.py $sc python=$out | sed "1s|^|${set_}/|"
  echo "time ${set_}/${c} $(python3 -c "print(round($t1-$t0,1))")"
done
O=out/img_bench
for m in bench_img/*_meta.json; do
  c=$(basename $m _meta.json); ext=$(python3 -c "import json;print(json.load(open('$m'))['ext'])")
  python3 $P/image.py bench_img/${c}_marked.$ext $O/${c}_py.$ext > /dev/null 2>&1
done
tools/venv/bin/python $P/img_bench.py score bench_img python=$O/{case}_py.png 2>/dev/null | grep "_png"
tools/venv/bin/python $P/img_bench.py score bench_img python=$O/{case}_py.jpg 2>/dev/null | grep -E "jpg90|small"
