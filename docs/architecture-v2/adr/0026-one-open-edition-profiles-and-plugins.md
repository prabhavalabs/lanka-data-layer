# ADR 0026 — One open-source edition, deployment profiles, compiled-in plugins

Status: proposed · 2026-11-22 · Detail: [12 §8–§10](../12-enterprise-and-byod.md#8-plugin-points)

## Context

The platform must serve individuals, teams and enterprises (including on-premises installations) while being operated by one or two people.

## Decision

There is one codebase under Apache-2.0 and no enterprise edition. Differences between installations are **profiles** (compose override + environment): `public`, `mirror` (read-only, app plane off), `private` (app plane on, corporate OIDC, optional closed `/v2`, upstream release sync, registry overlay) and `laptop` (offline pack). Limits are rows in `app.plans`; there is no billing code. Extension is by data where possible (YAML connectors and recipes, ramps, aliases, codelists in a registry overlay) and by Rust traits otherwise (`Connector`, `Method`, `AnalysisMethod`, `Exporter`, `BlobStore`), linked into a **distribution binary** that registers extra implementations. No runtime plugin loader now; sandboxed WebAssembly components for analysis methods are the named growth path. Outbound integration remains poll-only ([ADR 0016](0016-status-apis-only.md)). A tamper-evident audit log is part of the app plane.

## Consequences

- One code path to test; the fork test covers the entire product.
- A public body can run the full system without a licence purchase.
- Revenue, if wanted, comes from hosting, managed instances, onboarding and support — an owner decision, not an architectural one.
- Custom Rust methods require building a binary.

## Alternatives rejected

- **Open core with closed enterprise features** (SSO, audit, private data): splits the codebase and the test matrix; contradicts the project's openness goal.
- **Dynamic libraries or a scripting runtime for plugins**: ABI and sandboxing work with no current user.
- **Multi-tenant SaaS only**: excludes organisations that cannot place data off-premises.
