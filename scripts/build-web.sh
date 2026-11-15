#!/usr/bin/env bash
# Builds the web demo into web/pkg and web/assets. Same script locally and in CI.
# Usage: scripts/build-web.sh [mozc-dir]   (or MOZC_DIR=...; default target/mozc)
set -euo pipefail

cd "$(dirname "$0")/.."
mozc_dir="${1:-${MOZC_DIR:-target/mozc}}"
commit="$(grep -oE 'MOZC_COMMIT: &str = "[0-9a-f]{40}"' tools/nullime-dict/src/main.rs | grep -oE '[0-9a-f]{40}')"
step() { printf '\n==> %s\n' "$*"; }
start=$SECONDS

step "wasm: cargo build nullime-wasm"
cargo build -p nullime-wasm --release --target wasm32-unknown-unknown

step "wasm-bindgen -> web/pkg"
# mise.toml pins wasm-bindgen-cli; fall back to PATH when mise is absent.
if command -v mise >/dev/null 2>&1; then bindgen=(mise exec -- wasm-bindgen); else bindgen=(wasm-bindgen); fi
"${bindgen[@]}" --target web --out-dir web/pkg target/wasm32-unknown-unknown/release/nullime_wasm.wasm

step "Mozc dictionary_oss @ ${commit} -> ${mozc_dir}"
if [ "$(cat "$mozc_dir/.mozc-commit" 2>/dev/null || true)" = "$commit" ]; then
  echo "cache hit, skipping fetch"
else
  rm -f "$mozc_dir/.mozc-commit"
  cargo run --release -p nullime-dict -- fetch --out "$mozc_dir"
  echo "$commit" > "$mozc_dir/.mozc-commit"
fi

step "dictionary assets -> web/assets"
mkdir -p web/assets
cargo run --release -p nullime-dict -- build --mozc "$mozc_dir" --out web/assets/nullime-ja.dict
cargo run --release -p nullime-dict -- build --mozc "$mozc_dir" --max-cost 6000 --out web/assets/nullime-ja.small.dict

step "sizes"
ls -l web/assets/*.dict web/pkg/*.wasm | awk '{printf "%12d  %s\n", $5, $NF}'
echo "done in $((SECONDS - start))s"
