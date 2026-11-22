#!/usr/bin/env bash
# WP-27 browser smoke test: the 12-layer reference document (ref12.json) is
# resolved with `lanka map resolve` and drawn by MapLibre GL 4.7 in headless
# Chromium (agent-browser) against a running `lanka serve`: dem hillshade,
# two ldl-v16 value-tile rasters (decoded in the page), three frame
# choropleths (feature-state on the admin tileset), road vector tiles,
# GeoJSON hospitals, Arrow points (schools) and heatmap (places of worship),
# province lines and district labels. Passes when all 12 layers draw data
# and MapLibre reports no error.
#
#   API=http://127.0.0.1:8627 LANKA=engine/target/release/lanka ./run.sh
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
api="${API:-http://127.0.0.1:8627}"
lanka="${LANKA:-lanka}"
port="${PORT:-8629}"
work="$(mktemp -d)"
cp "$here/index.html" "$here/ref12.json" "$work/"
"$lanka" map validate "$work/ref12.json"
"$lanka" map resolve "$work/ref12.json" > "$work/ref12.resolved.json"
python3 -m http.server "$port" --bind 127.0.0.1 --directory "$work" > "$work/http.log" 2>&1 &
srv=$!
trap 'kill $srv 2>/dev/null; rm -rf "$work"' EXIT
s=wp27-smoke-$$
agent-browser --session "$s" set viewport 1280 900 > /dev/null
agent-browser --session "$s" open "http://127.0.0.1:$port/index.html?api=$api" > /dev/null
agent-browser --session "$s" wait --fn "document.title.startsWith('SMOKE')" --timeout 60000 > /dev/null
out="$(agent-browser --session "$s" eval 'JSON.stringify({ok: __smoke.ok, drawn: __smoke.drawn, idle_ms: __smoke.idle_ms, layers: __smoke.layers, errors: __smoke.errors})')"
agent-browser --session "$s" screenshot "${SHOT:-$here/../../../../target/wp27-smoke.png}" > /dev/null || true
agent-browser --session "$s" close > /dev/null || true
echo "$out"
case "$out" in *'\"ok\":true'*|*'"ok":true'*) exit 0 ;; *) exit 1 ;; esac
