# nuuuwan repositories: what to ingest

Survey of `github.com/nuuuwan` (Nuwan Senaratna), 2026-11-24. There are 410 repositories: 331 MIT and 79 without a licence. 80 were pushed since 2026-06, and about 25 are refreshed by bots every day. About 140 are tools or experiments and about 90 are stubs. The rest hold data. MIT covers his code; the terms of the underlying government data are unstated, which the registry records as `permission_unclear` (see [ADR 0027](../architecture-v2/adr/0027-data-organisation-on-github.md)).

## Already in the registry

| Repository | Source id |
|---|---|
| `lk_irrigation` | `nuuuwan-lk-irrigation` |
| `lk_dmc` | `nuuuwan-lk-dmc` |
| `weather_lk` | `nuuuwan-weather-lk` (planned) |
| `lk_dengue` | `nuuuwan-lk-dengue` |
| `lk_dcs` | `nuuuwan-lk-dcs` |
| `lk_census_2024`, `_2012`, `_2001`, `_1971` | `nuuuwan-lk-census-*` |
| `lk_admin_regions`, `gig-data` | `nuuuwan-lk-admin-regions`, `nuuuwan-gig-data` |
| `lk_moh_regions` | `nuuuwan-lk-moh-regions` |
| `lk_cabinet_decisions` | `nuuuwan-lk-cabinet-decisions` |
| `lk_tourism` | `nuuuwan-lk-tourism` |
| `lk_eroc` | `nuuuwan-lk-eroc` |
| `lg_election_lk_2025` | `nuuuwan-lg-election-2025` |
| `lk_elections` | `nuuuwan-lk-elections` |
| `cmb_bus_network`, `cmb_rail` | `nuuuwan-cmb-transit` |
| `lk_metro` | `nuuuwan-lk-metro` |

## Not yet covered, in order of value

| Repository | Data | Cadence | Plan |
|---|---|---|---|
| `lk_dmc_landslides` | Landslide warning level 1–3 per DS division | every 3 h | Connector. His `LK-xxxx` ids go through `gnd-id-crosswalk` to p-codes. Mapped warnings per DSD. |
| `lanka_data_timeseries` | 4,361 national series: CBSL, World Bank, IMF, ADB, DMT, SLTDA | hourly | Cross-check against our direct CBSL and WDI connectors. Ingest only the series we lack (ADB, DMT). |
| `lk_hansard` | Parliamentary Hansard, three languages, 2023 onwards | every 8 h | A `docs` collection (13 §1). Speakers link to `parliament-members`. |
| `lk_legal_docs` | 3,979 acts, bills, extraordinary gazettes since the 1980s (114,000 documents) | every 12 h | Our own `lk-acts` and `lk-bills` collections from documents.gov.lk. His archive is a cross-check, and backfill for gazettes older than the site lists. |
| `lk_fisheries` | Weekly fish prices, monthly production and trade | every 12 h | Connector for the published statistics (not scraped listings, so allowed by ADR 0025). |
| `lk_tsunamis` | USGS earthquakes near Sri Lanka | every 15 min | Connect to USGS directly instead. |
| `lk_air_travel` | Colombo airport inbound flights | every 30 min | Low priority. |
| `lk_treasury`, `lk_pmd` | Treasury and PM office press releases | hourly, daily | `docs` collections. `lk_treasury` data stops at 2025-07. |
| `lk_appeal_court_judgements`, `lk_supreme_court_judgements` | 18,827 and 2,729 judgements | every 12 h | Only after a redaction policy is in place (party names, minors). |
| `lk_edupub` | 1,426 school textbooks (9.4 GB) | every 10 days | Out of scope for now. |

## Not ingested, by policy

- `lk_food`: scraped supermarket shelf prices. [ADR 0025](../architecture-v2/adr/0025-price-data-and-point-sample-policy.md) rules out scraped listings.
- `lk_news`, `news_long_lk`, `news_lk*`: commercial publishers' articles.
- `lk_obituaries`, `lk_police_press_releases`, `politicians_lk`: personal data with no redaction policy.
- `lk_religion`, `lk_diversity`, `lk_urban`, `lk_dengue_weather_model`: his own analyses. We derive our own from the census, with methods.

## What we take from how he builds them

**Kept:**
- A scheduled pipeline per repository, with separate `update_latest` and `back_populate` jobs.
- A data branch per dataset.
- The raw file kept beside its extracted text, with text coverage stated.
- Generated READMEs that make staleness visible.

**Done differently** ([ADR 0027](../architecture-v2/adr/0027-data-organisation-on-github.md)):
- Schema checks that fail loudly.
- A freshness watchdog.
- Compact daily NDJSON instead of a file per reading.
- Binaries in Release assets, not git history.
- A bot identity.
- Licences stated per repository.
- p-codes rather than private ids.
- LLM-extracted tables marked as lower confidence.
- Tests.

Watch list: `lk_weather_3h` has been silent since 2026-11-11, `lk_police_press_releases` since 2026-09, and `lk_treasury` data stops at 2025-07.
