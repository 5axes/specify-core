#!/usr/bin/env bash
# Packages specify-core for crates.io with the built app and engine in bundle/ (ADR P12), if the
# crate stays under crates.io's 10 MB; otherwise without them (its `open` then asks for --app).
# Run from the repository root after bundle/app and bundle/wasm are in place (bundle.yml).
# bundle/ is not committed, so packaging allows a dirty tree.
set -euo pipefail
limit=10485760
version=$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | select(.name == "specify-core") | .version')
crate="target/package/specify-core-$version.crate"
for file in app/index.html app/specify-ui-app.json wasm/specify_core_wasm.js wasm/specify_core_wasm_bg.wasm; do
  [ -f "bundle/$file" ] || { echo "::error::bundle/$file is missing: build the app and engine first (bundle.yml)"; exit 1; }
done
cargo package --locked --allow-dirty --no-verify -p specify-core
size=$(wc -c < "$crate" | tr -d ' ')
echo "$crate: $size bytes with the app and engine (crates.io allows $limit)"
if [ "$size" -ge "$limit" ]; then
  echo "::warning::the crate with the app and engine is $size bytes, over crates.io's $limit: it is published without them, and its open asks for --app"
  rm -rf bundle "$crate"
  cargo package --locked --allow-dirty --no-verify -p specify-core
  echo "$crate: $(wc -c < "$crate" | tr -d ' ') bytes without them"
else
  cargo package --list --locked --allow-dirty -p specify-core | grep -qx bundle/wasm/specify_core_wasm_bg.wasm
fi
