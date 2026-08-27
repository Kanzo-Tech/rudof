#!/usr/bin/env bash
# Build the rudof_wasm bindings for the browser into ./pkg using the pinned
# wasm-bindgen CLI (NOT wasm-pack, which is archived): cargo wasm32 build +
# wasm-bindgen --target web + wasm-opt -Oz, then stamp the publishable npm identity.
# Self-contained: run from anywhere. Requires the wasm32-unknown-unknown target and
# wasm-bindgen-cli (wasm-opt optional but recommended).
#
# Pinned tooling:
#   wasm-bindgen-cli: 0.2.120 (must match the `wasm-bindgen = "=0.2.120"` lib pin)
#   Install: cargo install --version 0.2.120 wasm-bindgen-cli
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
command -v wasm-bindgen >/dev/null 2>&1 || {
  echo "error: wasm-bindgen CLI not found." >&2
  echo "  cargo install --version 0.2.120 wasm-bindgen-cli" >&2
  exit 1
}
WB_VERSION=$(wasm-bindgen --version | awk '{print $2}')
[ "$WB_VERSION" = "0.2.120" ] || echo "warning: wasm-bindgen CLI $WB_VERSION != 0.2.120 (pinned); output may be ABI-mismatched." >&2

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
mkdir -p "$PKG_DIR"
echo "[build] wasm-bindgen --target web → $PKG_DIR"
wasm-bindgen "$WASM_INPUT" --target web --out-dir "$PKG_DIR" --out-name rudof_wasm

# ---- 3. wasm-opt -Oz (size; parity with the previous wasm-pack profile) ----
if [ "$HAS_WASM_OPT" -eq 1 ]; then
  echo "[build] wasm-opt -Oz"
  wasm-opt -Oz "$PKG_DIR/rudof_wasm_bg.wasm" -o "$PKG_DIR/rudof_wasm_bg.wasm"
fi

# ---- 4. Stamp the publishable npm identity ----
# wasm-bindgen names the package after the crate (rudof_wasm @ workspace version) and
# omits `repository`; stamp the chosen scoped npm name + the Kanzo fork repo.
cd "$PKG_DIR"
npm pkg set name="$PKG_NAME"
[ -n "${PKG_VERSION:-}" ] && npm pkg set version="$PKG_VERSION"
npm pkg set repository.type="git" repository.url="git+https://github.com/Kanzo-Tech/rudof.git"

RAW=$(wc -c < "$PKG_DIR/rudof_wasm_bg.wasm")
echo "rudof_wasm → $PKG_DIR  ($(npm pkg get name | tr -d '\"')@$(npm pkg get version | tr -d '\"'), ${RAW} bytes)"
