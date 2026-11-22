# ADR 0009 — Sandboxed document tools; class C is operator-run and human-signed

Status: proposed · 2026-11-21 · Detail: [03 §3](../03-ingestion.md#3-documents-pdf-and-xlsx-with-locators), [07 §5](../07-operations.md#5-security)

## Context

~70 % of parity effort is PDF tables from ~170 upstream sites; the tools that read them (poppler, tesseract, GDAL, tippecanoe) are C/C++ with a history of memory-safety bugs. Nine datasets need LLM-assisted extraction. The engine otherwise forbids `unsafe`.

## Decision

All C/C++ document, raster and tile tools run in a `tools` sidecar container with no network, read-only root filesystem, dropped capabilities, memory/pid limits and per-request rlimits and timeouts, driven over a Unix socket by `lanka-extractd` with fixed argv allow-lists. Pure-Rust parsers run in-process with size caps. Class C: an operator runs `lanka extract propose`; two model readings must be anchored to text-layer/OCR tokens; the result is an L1 extract that requires per-cell sign-off below confidence 1.0 plus a 10 % sample. The worker never holds LLM credentials and never calls a model. Preferred use of the LLM is proposing a deterministic template, not numbers.

## Consequences

- A malicious PDF can at worst crash a process with no network, secrets or database access.
- GDAL and tippecanoe are CLI subprocesses, so the `lanka` binary stays free of C dependencies.
- Class C throughput is bounded by reviewer time (intended).
- LLM provider choice is configuration (hosted or local).

## Alternatives rejected

- **Link poppler/GDAL into the engine**: C code in the process holding database credentials.
- **`docker run` per extraction from the worker**: needs the Docker socket (root-equivalent).
- **bubblewrap/nsjail inside the worker container**: fragile under Docker's default seccomp/userns settings.
- **LLM extraction in the scheduled path**: non-deterministic, unreviewed numbers and a prompt-injection path.
- **Pure-Rust PDF parsing**: current crates do not give reliable word-level bounding boxes across government PDFs.
