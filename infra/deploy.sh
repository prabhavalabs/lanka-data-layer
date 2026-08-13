#!/usr/bin/env bash
# Server-side deploy: run on the VPS as /opt/lanka-data-layer/infra/deploy.sh.
# Invoked by the GitHub Actions workflow on every merge to main, and safe to
# run by hand. Deploys CODE only — data artifacts are shipped separately
# (see infra/README.md "Data releases").
set -euo pipefail

REPO_DIR=/opt/lanka-data-layer
BRANCH=${1:-main}

cd "$REPO_DIR"

echo "==> Updating to origin/$BRANCH"
git fetch origin "$BRANCH"
git reset --hard "origin/$BRANCH"

echo "==> Building web bundle (throwaway node container)"
docker run --rm \
  -v "$REPO_DIR":/repo -w /repo \
  -e CI=1 \
  node:22-slim \
  sh -c "corepack enable && corepack prepare pnpm@10.0.0 --activate \
    && pnpm install --frozen-lockfile \
    && pnpm --filter @lanka-data-layer/web run build"

echo "==> Publishing static bundle"
mkdir -p web-dist
rsync -a --delete web/dist/ web-dist/

echo "==> Building and starting the API"
docker compose -f infra/docker-compose.yml build api
docker compose -f infra/docker-compose.yml up -d api

echo "==> Health check"
curl -fsS --retry 15 --retry-connrefused --retry-delay 2 http://127.0.0.1:8600/v1/health >/dev/null
echo "==> Deploy OK: $(git rev-parse --short HEAD)"
