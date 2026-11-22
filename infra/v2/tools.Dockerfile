# syntax=docker/dockerfile:1.7
#
# The `tools` sidecar (ADR 0009, 07 §5): poppler + tesseract behind
# `lanka-extractd`, which serves a fixed set of operations on a Unix socket
# in a volume shared with the worker. Build context is engine/:
#
#   docker build -f infra/v2/tools.Dockerfile -t lanka-tools:dev engine
#
# The compose service runs it with no network (`network_mode: none`), a
# read-only root, all capabilities dropped, no-new-privileges, memory and
# pid limits and a tmpfs work directory; every tool call additionally runs
# under ulimits and a wall-clock kill (lanka-extractd::ops::Limits). The
# image holds no credentials and the container sees no database.

ARG RUST_IMAGE=rust:1.94-slim-trixie
ARG RUNTIME_IMAGE=debian:trixie-slim

FROM ${RUST_IMAGE} AS build
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/src/target-tools,sharing=locked \
    CARGO_TARGET_DIR=/src/target-tools cargo build --release --locked -p lanka-extractd \
 && install -D -m 0755 /src/target-tools/release/lanka-extractd /out/lanka-extractd

FROM ${RUNTIME_IMAGE}
ARG GIT_REV=unknown
LABEL org.opencontainers.image.title="lanka-tools" \
      org.opencontainers.image.description="Lanka Data Layer document sandbox: poppler + tesseract behind lanka-extractd" \
      org.opencontainers.image.source="https://github.com/prabhavalabs/lanka-data-layer" \
      org.opencontainers.image.revision="${GIT_REV}"
# Only the tools the allow-list uses: pdfinfo/pdftotext/pdftoppm, tesseract
# with English, Sinhala and Tamil (03 §3.1).
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      poppler-utils tesseract-ocr tesseract-ocr-eng tesseract-ocr-sin tesseract-ocr-tam \
 && rm -rf /var/lib/apt/lists/* \
 && groupadd -g 10001 lanka && useradd -u 10001 -g 10001 -M -s /usr/sbin/nologin lanka \
 && install -d -o 10001 -g 10001 -m 0770 /run/lanka
COPY --from=build /out/lanka-extractd /usr/local/bin/lanka-extractd
ENV LANKA_EXTRACTD_SOCKET=/run/lanka/extractd.sock \
    LANKA_EXTRACTD_WORK=/tmp \
    RUST_LOG=info
USER 10001:10001
ENTRYPOINT ["/usr/local/bin/lanka-extractd"]
CMD ["serve"]
