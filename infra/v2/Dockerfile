# syntax=docker/dockerfile:1.7
#
# The `lanka` image: one static-ish binary for `lanka serve`, `lanka worker`
# and `lanka db migrate` (07 §1). Build context is engine/:
#
#   docker build -f infra/v2/Dockerfile -t lanka:dev --build-arg GIT_REV=$(git rev-parse --short HEAD) engine
#
# Runtime: distroless cc (glibc, libssl3, CA certificates, tzdata; no shell,
# no package manager), uid 10001. TLS is rustls, except for the hosts the
# country profile lists under fetch.legacy_tls_hosts (servers offering only
# TLS 1.2 CBC suites): those use OpenSSL, linked dynamically against the
# libssl3 the distroless cc image already ships. Nothing is added to it.

ARG RUST_IMAGE=rust:1.94-slim-trixie
ARG RUNTIME_IMAGE=gcr.io/distroless/cc-debian13:nonroot

FROM ${RUST_IMAGE} AS build
# openssl-sys (reqwest native-tls) links the system libssl3 at build time.
RUN apt-get update \
 && apt-get install -y --no-install-recommends pkg-config libssl-dev \
 && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY . .
# Registry and target caches survive between builds; the binary is copied
# out of the cache mount so the final stage can see it.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/src/target,sharing=locked \
    cargo build --release --locked -p lanka-cli \
 && install -D -m 0755 target/release/lanka /out/lanka

FROM ${RUNTIME_IMAGE}
ARG GIT_REV=unknown
LABEL org.opencontainers.image.title="lanka" \
      org.opencontainers.image.description="Lanka Data Layer v2 engine: serve, worker, migrate" \
      org.opencontainers.image.source="https://github.com/prabhavalabs/lanka-data-layer" \
      org.opencontainers.image.revision="${GIT_REV}"
COPY --from=build /out/lanka /usr/local/bin/lanka
ENV LANKA_GIT_REV=${GIT_REV} \
    LANKA_LOG_JSON=true \
    LANKA_STORE_DIR=/store \
    LANKA_REGISTRY_DIR=/app/registry \
    RUST_LOG=info,sqlx=warn
USER 10001:10001
WORKDIR /app
ENTRYPOINT ["/usr/local/bin/lanka"]
CMD ["--help"]
