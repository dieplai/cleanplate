#!/usr/bin/env sh
# Rebuild web/cleanplate.wasm from crates/cleanplate-wasm. Uses the local toolchain when it has the wasm32 target,
# otherwise the official rust Docker image (no install needed).
set -e
cd "$(dirname "$0")/.."
FLAGS='-C target-feature=+simd128'
if rustc --print target-libdir --target wasm32-unknown-unknown >/dev/null 2>&1 &&
  [ -d "$(rustc --print target-libdir --target wasm32-unknown-unknown)" ]; then
  RUSTFLAGS="$FLAGS" cargo build --release -p cleanplate-wasm --target wasm32-unknown-unknown
  cp target/wasm32-unknown-unknown/release/cleanplate_wasm.wasm web/cleanplate.wasm
else
  docker run --rm -v "$PWD":/src:ro -v "$PWD/web":/out -w /src -e CARGO_TARGET_DIR=/tmp/t rust:1.88-slim sh -c \
    "rustup target add wasm32-unknown-unknown >/dev/null && RUSTFLAGS='$FLAGS' cargo build --release --locked -p cleanplate-wasm --target wasm32-unknown-unknown && cp /tmp/t/wasm32-unknown-unknown/release/cleanplate_wasm.wasm /out/cleanplate.wasm && chown $(id -u):$(id -g) /out/cleanplate.wasm"
fi
ls -l web/cleanplate.wasm
