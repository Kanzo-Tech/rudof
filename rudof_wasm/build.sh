#!/usr/bin/env bash
# Build the rudof_wasm bindings for the browser (wasm-bindgen --target web) into
# ./pkg. Self-contained: run from anywhere. Requires the wasm32 target,
# wasm-bindgen-cli, and (optionally) wasm-opt for size.
set -euo pipefail
cd "$(dirname "$0")"

cargo build -p rudof_wasm --target wasm32-unknown-unknown --profile wasm-release
wasm-bindgen ../target/wasm32-unknown-unknown/wasm-release/rudof_wasm.wasm \
  --out-dir pkg --target web
wasm-opt -Oz pkg/rudof_wasm_bg.wasm -o pkg/rudof_wasm_bg.wasm \
  || echo 'wasm-opt not found — skipping size optimization'
node -e "require('fs').writeFileSync('pkg/package.json', JSON.stringify({name:'rudof-wasm',version:'0.0.0',type:'module',main:'rudof_wasm.js',module:'rudof_wasm.js',types:'rudof_wasm.d.ts',sideEffects:['./rudof_wasm.js','./snippets/*'],files:['rudof_wasm.js','rudof_wasm.d.ts','rudof_wasm_bg.wasm','rudof_wasm_bg.wasm.d.ts']},null,2))"
echo "rudof_wasm → $(pwd)/pkg"
