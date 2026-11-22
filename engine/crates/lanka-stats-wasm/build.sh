#!/usr/bin/env sh
# Builds the WebAssembly module, records its size and refreshes the parity
# fixture (needs the wasm32-unknown-unknown target and node ≥ 18).
#   crates/lanka-stats-wasm/build.sh            (from engine/)
set -eu
here=$(cd "$(dirname "$0")" && pwd)
cd "$here/../.."
cargo build --release --target wasm32-unknown-unknown -p lanka-stats-wasm
wasm=target/wasm32-unknown-unknown/release/lanka_stats_wasm.wasm
raw=$(wc -c < "$wasm" | tr -d ' ')
gz=$(gzip -9 -c "$wasm" | wc -c | tr -d ' ')
echo "lanka_stats_wasm.wasm: $raw bytes, $gz bytes gzip -9"
node "$here/js/parity.mjs" "$wasm" crates/lanka-stats/tests/fixtures/reference.json > "$here/tests/fixtures/wasm_bits.json"
echo "parity fixture: $here/tests/fixtures/wasm_bits.json (cargo test -p lanka-stats-wasm compares it with the native build)"
