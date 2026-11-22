# ADR 0022 — Guard rails are part of the analysis result contract

Status: proposed · 2026-11-22 · Detail: [11 §6](../11-analysis-engine.md#6-guard-rails)

## Context

Analyses are run by people without statistical training and by language models, and are not reviewed. Areal data invites the modifiable areal unit problem and the ecological fallacy; neighbouring units are not independent; several platform datasets share inputs; correlation will be read as causation.

## Decision

Every result carries a list of structured **guards** (`code`, `severity: note | warn | block`, message from a fixed template, optional corrective action). Guards are evaluated by the engine, not the client. `block` returns no estimates. Under `SMALL_N`, *p*-values are not returned at all. For areal supports the headline significance is the spatially adjusted one. `NOT_CAUSAL` is attached to every relationship result and message templates are scanned in CI for causal vocabulary. `MAUP` comes with an automatic scale-sensitivity table when adjacent levels are available. `SHARED_LINEAGE` is raised from the derivation graph when two variables share an upstream dataset. Interpretation text is templated; no free-text generation. Clients, embeds and MCP text must render `warn` and `block` guards next to any number or layer from the result.

## Consequences

- A shared map or an agent's answer cannot carry the estimate without its caveats.
- Some requests return "blocked" with an explanation instead of a number; documented overrides are recorded in the result and citation.
- The guard registry is a maintained artefact with a positive and a negative test per code.

## Alternatives rejected

- **Documentation-only caveats**: not present where the number is shown.
- **Client-side warnings**: every client would re-implement them; MCP and exports would have none.
- **Refusing all areal correlation**: removes the most requested capability; the honest version with guards is more useful than none.
