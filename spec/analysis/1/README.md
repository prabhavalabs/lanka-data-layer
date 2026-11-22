# ldl-analysis/1

The open analysis request and result contract of Lanka Data Layer
(design: `docs/architecture-v2/11-analysis-engine.md` §5–§6, ADR 0021/0022).

| File | Is |
|---|---|
| `request.schema.json` | an analysis request (`POST /v2/analyses`, `POST /v2/analyses/plan`, `analyses` in an `ldl-map/1` document) |
| `result.schema.json` | an analysis result (`GET /v2/analyses/{id}`) — guards, estimates, outputs, provenance, citation |
| `plan.schema.json` | the dry run (`POST /v2/analyses/plan`) |

Rules a client must follow (binding, UI contract §6.4): render `warn` and `block` guards above any number,
chart or layer from a result; a `blocked` result has no numbers; a `p` that is absent is not computed
client-side; use `interpretation.summary` and `reading` strings verbatim. Method parameter schemas, minimum
sample sizes and the guards each method can raise are served by `GET /v2/analysis/methods/{id}`.

The reference implementation is `engine/crates/lanka-analysis` (copies of these schemas live in its
`schema/` directory and a test keeps them byte-identical). Licence: CC BY 4.0.
