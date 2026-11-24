# ADR 0027: A separate GitHub organisation for data: captures, archives and release mirrors

Status: proposed · 2026-11-24 · Detail: [13 §1.4](../13-civic-record.md#14-off-site-copy-on-github-needs-the-owners-go-ahead), [07 §6.1](../07-operations.md#61-single-host-form-built-the-off-site-copy-above-is-still-to-come)

## Context

The platform's primary store is a single disk: the external SSD today, the VPS later. That disk needs an off-site copy, and some sources are better captured somewhere other than our own host. River levels every 30 minutes, disaster bulletins and weather are examples. They need redundancy, and their history should be public and verifiable by anyone.

Nuwan Senaratna's 410 repositories (`github.com/nuuuwan`) show the pattern works at scale. About 25 of them push fresh Sri Lankan data every day from GitHub Actions. They also show its weak points:
- scrapers stop silently (`lk_weather_3h` since 2026-11-11, `lk_police_press_releases` since 2026-09);
- there are no schema checks and almost no tests;
- one JSON file is written per reading (40,000 files in `lk_irrigation`);
- repositories grow to 14 to 38 GB of git history;
- a personal account and token act as the bot;
- MIT covers the code while the source terms of the data are unstated.

The owner wants data repositories kept apart from `prabhavalabs`, which holds the product code.

## Decision

A dedicated GitHub organisation (name chosen by the owner; `lanka-data-layer` is proposed) holds every data repository. The engine repository can stay in `prabhavalabs`; moving it is a separate decision. The organisation has four kinds of repository:

| Kind | Name | Holds | Written by |
|---|---|---|---|
| Capture | `capture-<source>` (for example `capture-irrigation-levels`) | Raw upstream responses for high-frequency sources, as one compressed NDJSON file per day on a `data` branch, validated against the JSON Schema in the repository | A scheduled Action in the repository |
| Archive | `archive-<collection>` (for example `archive-gazettes`) | Document metadata as `metadata/<year>.jsonl` in git, so its history can be diffed. Files go in Release assets, one release per month (`2026-11`), named by sha256, each with a `manifest.jsonl` (id, language, sha256, bytes, source URL). | `lanka docs mirror` from the primary store |
| Releases | `releases` | Each data release's distributions (CSV, Parquet, GeoJSON, PMTiles) and its DCAT catalogue as Release assets, tagged by `data_version` | `lanka release publish --mirror github` after a train |
| Shared | `.github`, `capture-template` | Org profile, reusable workflows (capture, freshness watchdog, mirror) and the template every capture starts from | Maintainers |

Rules, each fixing a weakness seen above:

1. **A capture fails loudly.** A response that fails its schema, or an empty result where the source has never been empty, fails the job. Nothing is committed and the run is red.
2. **Freshness watchdog.** A daily workflow in `.github` opens or updates an issue for every capture with no data commit in three times its cadence. The engine's `/v2/status` checks the same thing when it reads a capture.
3. **Data is kept compact.** One compressed NDJSON file per day per stream; no file per reading. Git history stays under 2 GB per repository, and anything larger or binary goes in Release assets. Git LFS is not used, because of its quota.
4. **A bot, not a person.** Commits come from a GitHub App or a dedicated machine account with repository-scoped permissions, never a personal token.
5. **Provenance by commit.** Engine connectors read a capture at a pinned commit SHA, and that SHA is recorded in the dataset version's inputs. Anyone can check out exactly what we ingested.
6. **Licences stated per repository.**
   - Code and workflows: Apache-2.0.
   - Our metadata: CC BY 4.0.
   - Captured data: the source's terms, written in the README. Official texts are not under copyright (Intellectual Property Act No. 36 of 2003, s. 8).
   - Data with unclear terms is captured only for internal processing unless the review gate approves redistribution.
7. **No personal data.** No news article text, no obituaries, and no court or police documents without a redaction policy approved first.
8. **Restore runs both ways.** `lanka docs restore --from github` and the release mirror can rebuild a store on a fresh host. The quarterly drill restores from GitHub as well as from local backups.

Third-party repositories such as nuuuwan's are not forked into the organisation. Our connectors keep reading them upstream at pinned commits, and we cross-check them against our own captures of the same source. Fixes found that way are offered back upstream.

## Consequences

- There is an off-site, public and verifiable copy of everything that can be redistributed, at no hosting cost. GitHub's limits are 2 GiB per Release asset, about 5 GB recommended per repository, and the Actions limits for public repositories.
- High-frequency capture keeps running when our host is down, and its full history is auditable through git.
- Many small repositories have to be kept consistent. The template and the reusable workflows carry that burden, and drift is caught by the watchdog.
- Creating the organisation, choosing its name and installing the bot are owner actions. GitHub has no API for creating organisations.
