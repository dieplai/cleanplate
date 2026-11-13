#!/usr/bin/env sh
# Rebuild the two WebAssembly builds of crates/cleanplate-wasm with the official rust Docker image:
#   web/cleanplate.wasm     one thread (wasm32-unknown-unknown), the fallback
#   web/cleanplate-mt.wasm  all cores (wasm32-wasip1-threads + rayon), used when the page is cross-origin isolated
set -e
cd "$(dirname "$0")/.."
docker run --rm -v "$PWD":/src:ro -v "$PWD/web":/out -w /src -e CARGO_TARGET_DIR=/tmp/t rust:1.88-slim sh -c '
  rustup target add wasm32-unknown-unknown wasm32-wasip1-threads >/dev/null &&
  RUSTFLAGS="-C target-feature=+simd128" \
    cargo build --release --locked -p cleanplate-wasm --target wasm32-unknown-unknown &&
  RUSTFLAGS="-C target-feature=+simd128 -C link-arg=--max-memory=4294967296 -C link-arg=--export=__wasi_init_tp -C link-arg=--export=__wasm_call_ctors" \
    cargo build --release --locked -p cleanplate-wasm --features parallel --target wasm32-wasip1-threads &&
  cp /tmp/t/wasm32-unknown-unknown/release/cleanplate_wasm.wasm /out/cleanplate.wasm &&
  cp /tmp/t/wasm32-wasip1-threads/release/cleanplate_wasm.wasm /out/cleanplate-mt.wasm &&
  chown '"$(id -u):$(id -g)"' /out/cleanplate.wasm /out/cleanplate-mt.wasm'
ls -l web/cleanplate.wasm web/cleanplate-mt.wasm
