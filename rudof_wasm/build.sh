#!/usr/bin/env bash
# Build the rudof_wasm bindings for the browser into ./pkg using the pinned
# wasm-bindgen CLI (NOT wasm-pack, which is archived): cargo wasm32 build +
# wasm-bindgen --target web + wasm-opt -Oz, then stamp the publishable npm identity.
# Self-contained: run from anywhere. Requires the wasm32-unknown-unknown target and
# wasm-bindgen-cli (wasm-opt optional but recommended).
#
# Pinned tooling:
#   wasm-bindgen-cli: exactly the `wasm-bindgen` version Cargo.lock resolves
#   (rudof_wasm pins the lib with `=`). The script reads it from the lockfile.
#
# Package identity is overridable for publishing:
#   PKG_NAME     npm name     (default: @kanzo-tech/rudof-wasm)
#   PKG_VERSION  npm version  (default: the crate version; CI sets the release version)
set -euo pipefail

SCRIPT_DIR="$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
REPO_ROOT="$( cd -- "$SCRIPT_DIR/.." &> /dev/null && pwd )"
PKG_DIR="$SCRIPT_DIR/pkg"
PKG_NAME="${PKG_NAME:-@kanzo-tech/rudof-wasm}"

# ---- Preflight ----
command -v cargo >/dev/null 2>&1 || { echo "error: cargo not found (install rustup)." >&2; exit 1; }
WB_LOCKED=$(cd "$REPO_ROOT" && cargo pkgid -p wasm-bindgen | sed 's/.*@//')
command -v wasm-bindgen >/dev/null 2>&1 || {
  echo "error: wasm-bindgen CLI not found." >&2
  echo "  cargo install --version $WB_LOCKED wasm-bindgen-cli" >&2
  exit 1
}
WB_VERSION=$(wasm-bindgen --version | awk '{print $2}')
[ "$WB_VERSION" = "$WB_LOCKED" ] || echo "warning: wasm-bindgen CLI $WB_VERSION != $WB_LOCKED (Cargo.lock); output may be ABI-mismatched." >&2

HAS_WASM_OPT=0
command -v wasm-opt >/dev/null 2>&1 && HAS_WASM_OPT=1 || \
  echo "warning: wasm-opt not found (install binaryen); skipping -Oz size pass." >&2

# ---- 1. cargo build → wasm32 ----
cd "$REPO_ROOT"
echo "[build] cargo build --release --target wasm32-unknown-unknown -p rudof_wasm"
cargo build --release --target wasm32-unknown-unknown -p rudof_wasm

WASM_INPUT="$REPO_ROOT/target/wasm32-unknown-unknown/release/rudof_wasm.wasm"
[ -f "$WASM_INPUT" ] || { echo "error: missing build artefact $WASM_INPUT" >&2; exit 1; }

# ---- 2. wasm-bindgen --target web ----
# --target web (NOT bundler): emits a small JS shim where the consumer passes the
# resolved .wasm URL via the default init — predictable across every bundler.
# Wipe pkg/ first. wasm-bindgen overwrites what it emits but removes nothing, so a
# stale tree survives across builds — which is exactly how the missing package.json
# below stayed invisible locally while failing on every clean checkout.
rm -rf "$PKG_DIR"
mkdir -p "$PKG_DIR"
echo "[build] wasm-bindgen --target web → $PKG_DIR"
wasm-bindgen "$WASM_INPUT" --target web --out-dir "$PKG_DIR" --out-name rudof_wasm

# ---- 3. wasm-opt -Oz (size; parity with the previous wasm-pack profile) ----
if [ "$HAS_WASM_OPT" -eq 1 ]; then
  echo "[build] wasm-opt -Oz"
  wasm-opt -Oz "$PKG_DIR/rudof_wasm_bg.wasm" -o "$PKG_DIR/rudof_wasm_bg.wasm"
fi

# ---- 4. Write the publishable npm identity ----
# wasm-pack used to emit pkg/package.json and copy the crate README; `wasm-bindgen`
# emits NEITHER — it writes only the .js/.d.ts/.wasm. So the manifest is authored
# here rather than patched with `npm pkg set`, which needs a file to edit and fails
# with ENOENT on a clean checkout.
#
# The field set reproduces what 0.3.4 shipped, deliberately: `files` lists three
# entries (npm adds README.md on its own), `main`/`types` point at the shim, and
# `sideEffects` scopes to snippets. Keep it that way unless the change is intended —
# this is the package's public surface.
CRATE_VERSION="$(sed -n '/^\[workspace\.package\]/,/^\[/p' "$REPO_ROOT/Cargo.toml" |
                 sed -n 's/^version *= *"\(.*\)"/\1/p' | head -1)"
PKG_VERSION="${PKG_VERSION:-$CRATE_VERSION}"
[ -n "$PKG_VERSION" ] || { echo "error: could not resolve a version (set PKG_VERSION)." >&2; exit 1; }

cat > "$PKG_DIR/package.json" <<JSON
{
  "name": "$PKG_NAME",
  "type": "module",
  "description": "wasm-bindgen bindings exposing rudof's SHACL/ShEx stack to JavaScript",
  "version": "$PKG_VERSION",
  "license": "MIT OR Apache-2.0",
  "files": [
    "rudof_wasm_bg.wasm",
    "rudof_wasm.js",
    "rudof_wasm.d.ts"
  ],
  "main": "rudof_wasm.js",
  "types": "rudof_wasm.d.ts",
  "sideEffects": [
    "./snippets/*"
  ],
  "repository": {
    "type": "git",
    "url": "git+https://github.com/Kanzo-Tech/rudof.git"
  }
}
JSON

# wasm-pack copied this too; npm ships a README whether or not `files` names it.
cp "$SCRIPT_DIR/README.md" "$PKG_DIR/README.md"

RAW=$(wc -c < "$PKG_DIR/rudof_wasm_bg.wasm")
echo "rudof_wasm → $PKG_DIR  ($PKG_NAME@$PKG_VERSION, ${RAW} bytes)"
