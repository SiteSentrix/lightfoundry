#!/bin/sh
# Builds the browser version into apps/lightfoundry-web/pkg. Needs the wasm target and a
# wasm-bindgen CLI matching the wasm-bindgen crate version in Cargo.lock:
#   rustup target add wasm32-unknown-unknown
#   cargo install wasm-bindgen-cli --version <version from Cargo.lock>
# Then serve this folder over HTTP (for example `python3 -m http.server`) and open index.html.
set -eu
cd "$(dirname "$0")/../.."
cargo build --release --target wasm32-unknown-unknown -p lightfoundry-web
wasm-bindgen --target web --no-typescript --out-dir apps/lightfoundry-web/pkg \
  target/wasm32-unknown-unknown/release/lightfoundry_web.wasm
echo "built apps/lightfoundry-web/pkg; serve apps/lightfoundry-web and open index.html"
