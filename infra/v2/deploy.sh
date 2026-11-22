#!/usr/bin/env bash
# infra/v2/deploy.sh <tag> — blue/green deploy of the v2 engine on the host
# (07 §2, §7). Run on the VPS from the repository checkout:
#
#   infra/v2/deploy.sh <tag>          # image ghcr.io/prabhavalabs/lanka:<tag>
#   LANKA_IMAGE=lanka:dev infra/v2/deploy.sh local   # a locally built image
#
# Steps: pull → migrate (admin role, forward-only) → start the idle colour of
# `serve` → health-check it → point nginx at it → stop the old colour →
# recreate the worker (SIGTERM: running jobs finish or are re-queued).
#
# Environment: ENV_FILE (default /etc/lanka/v2.env), STATE_DIR (default
# /etc/lanka; holds `serve-colour`), NGINX_UPSTREAM (default
# /etc/nginx/lanka-v2-upstream.conf; nginx is skipped when not installed).
set -euo pipefail

TAG=${1:?usage: deploy.sh <image tag>}
HERE=$(cd "$(dirname "$0")" && pwd)
ENV_FILE=${ENV_FILE:-/etc/lanka/v2.env}
STATE_DIR=${STATE_DIR:-/etc/lanka}
NGINX_UPSTREAM=${NGINX_UPSTREAM:-/etc/nginx/lanka-v2-upstream.conf}
export LANKA_IMAGE=${LANKA_IMAGE:-ghcr.io/prabhavalabs/lanka:${TAG}}
export LANKA_GIT_REV=${LANKA_GIT_REV:-$TAG}

compose() { docker compose -f "$HERE/docker-compose.yml" --env-file "$ENV_FILE" --profile green "$@"; }
env_get() { grep -E "^$1=" "$ENV_FILE" | tail -1 | cut -d= -f2- || true; }

BLUE_PORT=$(env_get LANKA_SERVE_PORT); BLUE_PORT=${BLUE_PORT:-8601}
GREEN_PORT=$(env_get LANKA_SERVE_GREEN_PORT); GREEN_PORT=${GREEN_PORT:-8611}
ACTIVE=$(cat "$STATE_DIR/serve-colour" 2>/dev/null || echo blue)
if [[ $ACTIVE == blue ]]; then
  NEXT=green NEXT_SVC=serve-green NEXT_PORT=$GREEN_PORT OLD_SVC=serve
else
  NEXT=blue NEXT_SVC=serve NEXT_PORT=$BLUE_PORT OLD_SVC=serve-green
fi

echo "==> image $LANKA_IMAGE (active: $ACTIVE → $NEXT on 127.0.0.1:$NEXT_PORT)"
if [[ $LANKA_IMAGE == */* ]]; then
  compose pull migrate "$NEXT_SVC" worker
fi

echo "==> migrate"
compose run --rm --no-deps migrate

echo "==> start $NEXT_SVC"
compose up -d --no-deps --force-recreate "$NEXT_SVC"
curl -fsS --retry 30 --retry-all-errors --retry-delay 2 "http://127.0.0.1:$NEXT_PORT/v2/health" >/dev/null
echo "    $NEXT_SVC healthy"

if command -v nginx >/dev/null 2>&1; then
  echo "==> switch nginx upstream → 127.0.0.1:$NEXT_PORT"
  printf 'server 127.0.0.1:%s;\n' "$NEXT_PORT" >"$NGINX_UPSTREAM.new"
  mv "$NGINX_UPSTREAM.new" "$NGINX_UPSTREAM"
  nginx -t && nginx -s reload
else
  echo "==> nginx not installed here: upstream switch skipped"
fi

echo "==> stop $OLD_SVC"
compose stop "$OLD_SVC"
mkdir -p "$STATE_DIR"
echo "$NEXT" >"$STATE_DIR/serve-colour"

echo "==> recreate worker"
compose up -d --no-deps --force-recreate worker

echo "==> deploy OK: $LANKA_IMAGE serving on $NEXT ($NEXT_PORT)"
