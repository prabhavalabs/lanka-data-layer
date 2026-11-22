# ADR 0023 — A separate app plane with optional accounts; anonymous read is untouched

Status: proposed · 2026-11-22 · Amends principle 8 and [06 §2.2](../06-serving.md#22-conventions), [07 §5](../07-operations.md#5-security) · Detail: [12 §2–§3](../12-enterprise-and-byod.md#2-two-planes)

## Context

The original design has no accounts, no writes from the API and no uploads. Saved maps, private data, API keys and audit need ownership.

## Decision

Two planes in one binary. The **public data plane** (`/v2`, `/mcp`, `/dl`) stays anonymous and world-cacheable (GET, plus `POST /v2/analyses`, which redirects to a cacheable GET); its handlers receive no data-access principal and its database roles cannot see private data. The **app plane** (`/app/v1`) carries credentials and writes; it uses a third role `lanka_app` that may write only schemas `app` and `ws`, with row-level security by workspace. nginx never caches `/app/`. Sign-in is OpenID Connect only (no passwords, no e-mail); sessions are opaque cookies scoped to `/app`. Anonymous users can still share maps through content-addressed, immutable **snapshots**. The app plane can be switched off (`LANKA_APP_PLANE=off`), which yields the original read-only system.

## Consequences

- Every guarantee of the open layer (cacheability, conformance, "the API role cannot write data") is preserved by construction.
- The platform now holds a small amount of personal data (handle, e-mail) and user content: a privacy notice, terms, deletion and takedown paths become prerequisites for enabling sign-in.
- Sign-in depends on external identity providers on the hosted instance.

## Alternatives rejected

- **Workspace columns on the public tables**: every existing query becomes a potential leak; public responses stop being cacheable without care.
- **A separate application service and database**: a second deployable and no joins between private and public data.
- **Passwords or e-mail magic links**: a credential store or a mail system to operate.
- **Accounts required for saving or sharing**: unnecessary friction; snapshots need no identity.
