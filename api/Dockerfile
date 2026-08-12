# Build from the repository root so the pnpm workspace (api + shared) is
# available, e.g.:
#   docker build -f api/Dockerfile -t geopub-api .
#
# The API ships and runs its TypeScript source directly (via tsx) rather than
# compiling to JS — @geopub/shared does the same (see shared/package.json),
# so this image never needs an emitting build step, only a typecheck.

# ---- deps: resolve the workspace strictly from the lockfile -----------------
FROM node:22-slim AS deps
RUN corepack enable && corepack prepare pnpm@10.0.0 --activate
WORKDIR /app
COPY package.json pnpm-workspace.yaml pnpm-lock.yaml ./
COPY shared/package.json shared/package.json
COPY api/package.json api/package.json
# better-sqlite3 ships prebuilt N-API binaries for linux-x64/arm64 — no
# build-essential/python3 toolchain needed in this image.
RUN pnpm install --frozen-lockfile --filter @geopub/api...

# ---- build: bring in source, typecheck as a build-time gate ------------------
FROM deps AS build
COPY shared shared
COPY api api
RUN pnpm --filter @geopub/api exec tsc --noEmit

# ---- runtime: slim image, non-root, only what's needed to run ----------------
FROM node:22-slim AS runtime
ENV NODE_ENV=production
WORKDIR /app
COPY --from=build --chown=node:node /app/node_modules ./node_modules
COPY --from=build --chown=node:node /app/shared ./shared
COPY --from=build --chown=node:node /app/api ./api
WORKDIR /app/api

ENV PORT=8600
# Points at the rsync'd/baked-in artifact inside the container — override at
# deploy time if the mount path differs (docs/architecture.md §3, §6).
ENV GEOPUB_DB=../foundry/data/artifacts/geopub.sqlite

EXPOSE 8600
# The official node:22-slim image already has a non-root "node" user (uid 1000).
USER node

CMD ["node_modules/.bin/tsx", "src/index.ts"]
