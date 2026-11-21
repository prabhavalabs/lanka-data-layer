# ADR 0016 — Monitoring: emit health, status and run events; no alert delivery

Status: proposed · 2026-11-21 · Detail: [07 §4](../07-operations.md#4-health-and-status-apis)

## Context

The foundation implements webhook alerts (`LANKA_ALERT_WEBHOOK`), a heartbeat ping (`LANKA_HEARTBEAT_URL`) and `ops.alerts` with delivery state, and plans a GitHub Actions watchdog. The owner will build external uptime monitoring as a separate system and wants the platform to expose status only.

## Decision

The engine exposes `/v2/health`, `/v2/status`, `/v2/status/{sources,datasets,runs,incidents}` and an append-only, cursor-read event feed `/v2/status/events`, plus an internal Prometheus endpoint on localhost. Persistent conditions are `ops.incidents`, opened and closed automatically. `/v2/status` always returns 200 with a computed `overall` state and includes worker tick age, queue age, disk and backup freshness. Webhook, heartbeat and `ops.alerts` delivery are removed.

## Consequences

- No outbound notification dependencies or secrets in the engine.
- Detecting that the VPS itself is down is entirely the external monitor's job.
- The `overall` rule and the event schema become a contract the monitor depends on (versioned with the API).

## Alternatives rejected

- **Keep webhooks as an option**: two alerting paths to maintain; explicitly out of scope.
- **Push events to a queue/webhook stream**: needs delivery guarantees; polling a monotonic id is simpler and loss-free.
- **GitHub Actions watchdog**: an external scheduler doing the monitor's job.
