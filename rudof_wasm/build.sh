#!/usr/bin/env bash
# Build the rudof_wasm bindings for the browser into ./pkg with wasm-pack
# (cargo wasm32 + wasm-bindgen + wasm-opt -Oz via [package.metadata.wasm-pack]),
# then stamp the publishable npm identity. Self-contained: run from anywhere.
# Requires wasm-pack and the wasm32-unknown-unknown target.
#
# Package identity is overridable for publishing:
#   PKG_NAME     npm name     (default: @kanzo-tech/rudof-wasm)
#   PKG_VERSION  npm version  (default: the crate version; CI sets the release version)
set -euo pipefail
cd "$(dirname "$0")"

PKG_NAME="${PKG_NAME:-@kanzo-tech/rudof-wasm}"

wasm-pack build --target web --out-dir pkg --out-name rudof_wasm --release

# wasm-pack names the package after the crate (rudof_wasm @ workspace version) and
# omits `repository`; stamp the chosen scoped npm name + the Kanzo fork repo.
cd pkg
npm pkg set name="$PKG_NAME"
[ -n "${PKG_VERSION:-}" ] && npm pkg set version="$PKG_VERSION"
npm pkg set repository.type="git" repository.url="git+https://github.com/Kanzo-Tech/rudof.git"
echo "rudof_wasm → $(pwd)  ($(npm pkg get name | tr -d '\"')@$(npm pkg get version | tr -d '\"'))"
