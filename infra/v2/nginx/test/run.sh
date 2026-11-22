#!/usr/bin/env bash
# Tests the real infra/v2/nginx/lanka-v2.conf in nginx (Docker, pinned image):
#
#   1. `nginx -t` on the real file inside an http {} block built from the
#      http-level lines documented in its header comment;
#   2. a stub upstream answers every request with
#      `Cache-Control: public, max-age=3600` and a per-response timestamp:
#      /v2/ responses are stored (second GET is a HIT with the same body),
#      /app/ responses never are (new body every time, no cache file whose
#      key is an /app/ URL);
#   3. X-Forwarded-For reaches the upstream with the client address last;
#   4. the snapshot write has its own rate zone (429 past the burst).
#
# Optional: UPSTREAM=host.docker.internal:8628 proxies to a running
# `lanka serve` instead of the stub (checks 1, 2 for /app/ and 3 only).
#
#   infra/v2/nginx/test/run.sh
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
conf="${CONF:-$here/../lanka-v2.conf}"
image="${NGINX_IMAGE:-nginx:1.27-alpine}"
port="${PORT:-18628}"
name="lanka-nginx-test-$$"
upstream="${UPSTREAM:-127.0.0.1:9000}"
work="$(mktemp -d)"
trap 'docker rm -f "$name" >/dev/null 2>&1 || true; rm -rf "$work"' EXIT

# http-level lines from the real file's header comment ("#   upstream …").
http_lines="$(sed -nE 's/^#   (upstream|limit_req_zone|limit_conn_zone|proxy_cache_path) /\1 /p' "$conf")"
[ -n "$http_lines" ] || { echo "no http-level lines found in $conf" >&2; exit 1; }

cat >"$work/nginx.conf" <<EOF
worker_processes 1;
error_log /dev/stderr warn;
pid /tmp/nginx.pid;
events {}
http {
  access_log off;
$http_lines
  server {
    listen 8080;
    include /etc/nginx/lanka-v2.conf;
  }
  # stub upstream: every response asks to be cached
  server {
    listen 9000;
    location / {
      default_type application/json;
      add_header Cache-Control "public, max-age=3600" always;
      return 200 '{"uri":"\$request_uri","method":"\$request_method","t":"\$msec\$request_id","xff":"\$http_x_forwarded_for"}';
    }
  }
}
EOF
echo "server $upstream;" >"$work/upstream.conf"

mounts=(-v "$work/nginx.conf:/etc/nginx/nginx.conf:ro"
        -v "$conf:/etc/nginx/lanka-v2.conf:ro"
        -v "$work/upstream.conf:/etc/nginx/lanka-v2-upstream.conf:ro")

echo "== nginx -t ($image)"
docker run --rm "${mounts[@]}" "$image" nginx -t

docker run -d --name "$name" --add-host host.docker.internal:host-gateway \
  -p "127.0.0.1:$port:8080" "${mounts[@]}" "$image" >/dev/null
base="http://127.0.0.1:$port"
# readiness: curl's own retry on refused connections (no sleep)
curl -fsS -o /dev/null --retry 30 --retry-connrefused --retry-all-errors --retry-max-time 30 "$base/v2/health" \
  || curl -sS -o /dev/null --retry 30 --retry-connrefused --retry-max-time 30 "$base/v2/health"

fail=0
check() { if eval "$2"; then echo "ok   $1"; else echo "FAIL $1"; fail=1; fi; }
hdr() { curl -sS -D - -o /dev/null "$@" | tr -d '\r'; }
body() { curl -sS "$@"; }

if [ "$upstream" = "127.0.0.1:9000" ]; then
  echo "== /v2/ is cached (control)"
  a="$(body "$base/v2/catalog?x=1")"; b="$(body "$base/v2/catalog?x=1")"
  check "/v2 second GET returns the stored body" '[ "$a" = "$b" ]'
  check "/v2 second GET is X-Cache: HIT" 'hdr "$base/v2/catalog?x=1" | grep -qi "^x-cache: HIT"'

  echo "== /app/ is never stored"
  for path in /app/v1/maps/ms_0123456789abcdef /app/v1/maps/ms_0123456789abcdef/resolved /app/v1/maps/ms_0123456789abcdef/package; do
    a="$(body "$base$path")"; b="$(body "$base$path")"; c="$(body "$base$path")"
    check "GET $path: a new upstream response every time" '[ "$a" != "$b" ] && [ "$b" != "$c" ]'
    check "GET $path: no X-Cache status" '! hdr "$base$path" | grep -qi "^x-cache:"'
  done
  a="$(body -X POST -H 'content-type: application/json' -d '{}' "$base/app/v1/maps/resolve")"
  b="$(body -X POST -H 'content-type: application/json' -d '{}' "$base/app/v1/maps/resolve")"
  check "POST /app/v1/maps/resolve: never served from cache" '[ "$a" != "$b" ]'
  keys="$(docker exec "$name" sh -c 'find /var/cache/nginx/lanka -type f -exec grep -ah "^KEY: " {} + 2>/dev/null' || true)"
  check "the cache holds /v2 entries" 'printf "%s\n" "$keys" | grep -q "/v2/"'
  check "the cache holds no /app/ entry" '! printf "%s\n" "$keys" | grep -q "/app/"'

  echo "== client address"
  x="$(body -H 'X-Forwarded-For: 10.1.1.1' "$base/app/v1/maps/ms_0123456789abcdef")"
  check "X-Forwarded-For ends with the address nginx saw" 'printf "%s" "$x" | grep -Eq "\"xff\":\"10\.1\.1\.1, [0-9.]+\""'

  echo "== snapshot rate zone"
  codes=""
  for _ in $(seq 1 12); do
    codes="$codes $(curl -sS -o /dev/null -w '%{http_code}' -X POST -H 'content-type: application/json' -d '{}' "$base/app/v1/maps/snapshots")"
  done
  echo "     codes:$codes"
  check "past the burst the snapshot write is 429" 'printf "%s" "$codes" | grep -q 429'
  check "the burst itself passes" 'printf "%s" "$codes" | grep -q 200'
  code="$(curl -sS -o /dev/null -w '%{http_code}' "$base/app/v1/maps/ms_0123456789abcdef")"
  check "other /app/ routes keep their own zone (200)" '[ "$code" = 200 ]'
else
  echo "== real upstream $upstream"
  for path in /app/v1/openapi.json /app/v1/maps/ms_0123456789abcdef; do
    check "GET $path: no X-Cache status" '! hdr "$base$path" | grep -qi "^x-cache:"'
  done
  body "$base/v2/health" >/dev/null; body "$base/v2/catalog" >/dev/null
  keys="$(docker exec "$name" sh -c 'find /var/cache/nginx/lanka -type f -exec grep -ah "^KEY: " {} + 2>/dev/null' || true)"
  check "the cache holds no /app/ entry" '! printf "%s\n" "$keys" | grep -q "/app/"'
fi

exit $fail
