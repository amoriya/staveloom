#!/bin/bash
set -e

echo "Building WASM module..."
cd crates/staveloom-wasm
npx wasm-pack build --target web --out-dir ../../web/pkg --release

echo "Done! Output in web/pkg/"
ls -lh ../../web/pkg/*.wasm
