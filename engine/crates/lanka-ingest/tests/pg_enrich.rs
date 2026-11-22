//! WP-06 enrichment against a real PostGIS server, on a throwaway database
//! (gated like `pg_integration.rs` by `LANKA_TEST_DATABASE_URL`).
//!
//! `enrichment_cycle` uses a synthetic country (`ZZ`: 1 country, 2 regions,
//! 3 districts) and five recipes over fixture inputs:
//!
//! - `derived-it-admin`: unit_metrics → `admin.area_km2` / `admin.child_count`
//! - `derived-it-rollup`: rollup of a district census to regions (+ QA: regions
//!   sum to the printed national total), with a provisional/`INTERPOLATED_SPACE`
//!   district and a suppressed one
//! - `derived-it-density`: ratio of the census and the roll-up over the areas,
//!   two recipes downstream (`/km2`)
//! - `derived-it-facilities`: features_zonal over a fixture `geo.features` table
//!   in WP-04's 0007 shape (`UNOFFICIAL_GEOM`/`DISAGGREGATED` features) and
//!   per 10,000 population (`DENOM_MISMATCH`)
//! - `it-official-sum`: a roll-up of an estimated input into an `official`
//!   indicator — the validator must reject it, the recipe fails with a
//!   `recipe_failed` incident and nothing else is held back
//!
//! and checks: first versions wait for review, approved outputs are
//! released by the train that produced them and are visible downstream in
//! the same train, a train with nothing new runs nothing (fingerprint
//! reuse), an input change reruns exactly the dependent recipes, status and
//! flags (bits 16/17 included) propagate across recipe boundaries, derived
//! rows have no obs_provenance, and `explain` returns the contributing
//! input rows with their obs_ids.

use std::path::PathBuf;

use lanka_core::{Basis, Flags, Method, Profile, Status};
use lanka_enrich::provenance::{GeoContext, explain_obs};
use lanka_enrich::{Methods, Recipes};
use lanka_ingest::Registry;
use lanka_ingest::connector::{AdminUnitIn, BoundarySet, ObsRow};
use lanka_ingest::derive::{self, DeriveEnv};
use lanka_ingest::geo_load;
use lanka_ingest::publish::{self, StageInput, publish_observations};
use lanka_ingest::run::RunTracker;
use lanka_ingest::train::{TrainReport, run_train_with};
use serde_json::json;
use sqlx::{Connection, Executor, PgConnection, PgPool};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

const PROFILE: &str = r#"
id: zz
name: { en: Testland }
iso3166: { alpha2: ZZ, alpha3: ZZZ }
site: { base_url: https://example.org }
languages: [en]
timezone: UTC
currency: XXX
grid: { lat_max: 10.0, lon_min: 79.4, step_deg: 0.001, rows: 4200, cols: 2700 }
admin:
  scheme: admin
  pcode_pattern: "^ZZ[0-9]*$"
  root_code: ZZ
  levels:
    - { level: 0, digits: 0, name: { en: Country } }
    - { level: 1, digits: 1, name: { en: Region } }
    - { level: 2, digits: 2, name: { en: District } }
  reference_boundary_set: it-set
  boundary_dataset: it-geo
metric_crs: EPSG:4326
calendars: { fiscal: { start_month: 1 }, agri: { start_month: 1 } }
schemes: []
"#;

fn write(dir: &std::path::Path, rel: &str, text: &str) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
    std::fs::write(p, text).expect("write");
}

fn ind(id: &str, unit: &str, mt: &str, agg: &str, class: &str, dims: &str, levels: &str) -> String {
    format!(
        "  - {{id: {id}, label: {id}, unit: '{unit}', measure_type: {mt}, aggregation: {agg}, class: {class},\n\
         \x20     dims: [{dims}], geo_levels: [{levels}], period_type: snapshot}}\n"
    )
}

fn dataset(id: &str, source: &str, kind: Option<&str>, indicators: &[String]) -> String {
    format!(
        "id: {id}\ntitle: {id}\ndescription: Fixture dataset for the enrichment tests.\nsource_ids: [{source}]\nindicators:\n{}\
         time_coverage: {{}}\nmethod: Fixture.\nlimits: [Not real data.]\npublication_class: A\n{}",
        if indicators.is_empty() {
            "  []\n".to_owned()
        } else {
            indicators.concat()
        },
        kind.map(|k| format!("kind: {k}\n")).unwrap_or_default()
    )
}

fn fixture_registry() -> (PathBuf, Profile) {
    let dir = std::env::temp_dir().join(format!("lanka-it-enrich-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for f in [
        "source.schema.json",
        "dataset.schema.json",
        "codelist.schema.json",
        "units.schema.json",
        "profile.schema.json",
        "recipe.schema.json",
    ] {
        std::fs::create_dir_all(dir.join("schema")).expect("mkdir");
        std::fs::copy(
            repo_root().join("registry/schema").join(f),
            dir.join("schema").join(f),
        )
        .expect("copy schema");
    }
    for c in ["sex", "amenity_type"] {
        std::fs::create_dir_all(dir.join("codelists")).expect("mkdir");
        std::fs::copy(
            repo_root().join(format!("registry/codelists/{c}.yaml")),
            dir.join(format!("codelists/{c}.yaml")),
        )
        .expect("copy codelist");
    }
    write(
        &dir,
        "units.yaml",
        "units:\n  - { code: '{person}', label: persons, kind: count }\n  - { code: '{count}', label: count, kind: count }\n\
         \x20 - { code: km2, label: km2, kind: physical }\n  - { code: '/km2', label: per km2, kind: ratio }\n\
         \x20 - { code: '/10*4', label: per 10000, kind: ratio }\n",
    );
    let src = |id: &str, datasets: &str| {
        format!(
            "id: {id}\npublisher: Test\ntitle: {id}\nhomepage: https://example.org\n\
             access: {{method: api, endpoint: https://example.org, auth: none}}\n\
             licence: {{id: cc-by-4.0, status: open}}\ncadence: {{frequency: annual}}\n\
             geo: {{levels: [national, adm2], join: pcode}}\nthemes: [ECON]\ndatasets: [{datasets}]\n\
             priority: P3\neffort: S\nstatus: active\n"
        )
    };
    write(
        &dir,
        "sources/it-src.yaml",
        &src("it-src", "it-geo, it-census, it-est, it-pois, it-official"),
    );
    write(
        &dir,
        "sources/it-derived.yaml",
        &src(
            "it-derived",
            "derived-it-admin, derived-it-rollup, derived-it-density, derived-it-facilities",
        ),
    );
    let lv = "national, adm1, adm2";
    write(
        &dir,
        "datasets/it-geo.yaml",
        &dataset("it-geo", "it-src", Some("boundaries"), &[]),
    );
    write(
        &dir,
        "datasets/it-census.yaml",
        &dataset(
            "it-census",
            "it-src",
            None,
            &[ind(
                "it.pop", "{person}", "count", "sum", "official", "sex", lv,
            )],
        ),
    );
    write(
        &dir,
        "datasets/it-est.yaml",
        &dataset(
            "it-est",
            "it-src",
            None,
            &[ind(
                "it.est_pop",
                "{person}",
                "count",
                "sum",
                "third_party",
                "",
                lv,
            )],
        ),
    );
    write(
        &dir,
        "datasets/it-pois.yaml",
        &dataset("it-pois", "it-src", Some("features"), &[]),
    );
    write(
        &dir,
        "datasets/it-official.yaml",
        &dataset(
            "it-official",
            "it-src",
            None,
            &[
                ind("it.est_pop", "{person}", "count", "sum", "official", "", lv)
                    .replace("it.est_pop", "it.official_sum"),
            ],
        ),
    );
    write(
        &dir,
        "datasets/derived-it-admin.yaml",
        &dataset(
            "derived-it-admin",
            "it-derived",
            None,
            &[
                ind("admin.area_km2", "km2", "sum", "sum", "derived", "", lv),
                ind(
                    "admin.child_count",
                    "{count}",
                    "count",
                    "sum",
                    "derived",
                    "",
                    "national, adm1",
                ),
            ],
        ),
    );
    write(
        &dir,
        "datasets/derived-it-rollup.yaml",
        &dataset(
            "derived-it-rollup",
            "it-derived",
            None,
            &[ind(
                "rollup.it.pop",
                "{person}",
                "count",
                "sum",
                "derived",
                "sex",
                "adm1",
            )],
        ),
    );
    write(
        &dir,
        "datasets/derived-it-density.yaml",
        &dataset(
            "derived-it-density",
            "it-derived",
            None,
            &[ind(
                "it.density",
                "/km2",
                "rate",
                "none",
                "derived",
                "sex",
                lv,
            )],
        ),
    );
    write(
        &dir,
        "datasets/derived-it-facilities.yaml",
        &dataset(
            "derived-it-facilities",
            "it-derived",
            None,
            &[
                ind(
                    "it.fac_count",
                    "{count}",
                    "count",
                    "sum",
                    "derived",
                    "amenity_type",
                    lv,
                ),
                ind(
                    "it.fac_per_10k",
                    "/10*4",
                    "rate",
                    "none",
                    "derived",
                    "amenity_type",
                    lv,
                ),
            ],
        ),
    );
    write(
        &dir,
        "recipes/derived-it-admin.yaml",
        "id: derived-it-admin\noutput: derived-it-admin\nstability: stable\ninputs: { admin: { dataset: it-geo } }\n\
         steps:\n  - { id: m, method: unit_metrics@^1, with: { boundaries: admin, period: '2024', area: admin.area_km2,\n\
         \x20     child_count: admin.child_count, levels: [0, 1, 2] } }\nemit: [ { from: m } ]\n",
    );
    write(
        &dir,
        "recipes/derived-it-rollup.yaml",
        "id: derived-it-rollup\noutput: derived-it-rollup\nstability: stable\ninputs: { census: { dataset: it-census } }\n\
         steps: [ { id: up, method: rollup@^1, with: { input: census, levels: [1] } } ]\n\
         emit: [ { from: up, map: { it.pop: rollup.it.pop } } ]\n\
         qa:\n  - { check: sum_matches, indicator: rollup.it.pop, level: 1, input: census, input_indicator: it.pop,\n\
         \x20     input_level: 0, tol_pct: 0.5 }\n",
    );
    write(
        &dir,
        "recipes/derived-it-density.yaml",
        "id: derived-it-density\noutput: derived-it-density\nstability: stable\n\
         inputs: { census: { dataset: it-census }, up: { dataset: derived-it-rollup }, admin: { dataset: derived-it-admin } }\n\
         steps:\n  - id: d\n    method: ratio@^1\n    with:\n      terms:\n\
         \x20       pop: { from: [ { port: census, indicator: it.pop }, { port: up, indicator: rollup.it.pop } ] }\n\
         \x20       area: { from: [ { port: admin, indicator: admin.area_km2 } ] }\n\
         \x20     formula: pop / area\n      indicator: it.density\n      levels: [1, 2]\n\
         emit: [ { from: d } ]\n",
    );
    write(
        &dir,
        "recipes/derived-it-facilities.yaml",
        "id: derived-it-facilities\noutput: derived-it-facilities\nstability: stable\n\
         inputs: { pois: { dataset: it-pois }, census: { dataset: it-census }, up: { dataset: derived-it-rollup } }\n\
         steps:\n  - id: n\n    method: features_zonal@^1\n    with: { features: pois, measure: count, kinds: [hospital, school],\n\
         \x20     dim: { id: amenity_type, from: kind, total: total }, indicator: it.fac_count, period: '2026', levels: [0, 1, 2] }\n\
         \x20 - id: r\n    method: ratio@^1\n    with:\n      terms:\n        n: { from: [ { port: n, indicator: it.fac_count } ] }\n\
         \x20       pop: { from: [ { port: census, indicator: it.pop }, { port: up, indicator: rollup.it.pop } ], select: sex=total }\n\
         \x20     formula: n / pop * 10000\n      indicator: it.fac_per_10k\n      match_period: { nearest: { max_years: 5 } }\n\
         \x20     levels: [1, 2]\nemit: [ { from: n }, { from: r } ]\n",
    );
    let p = dir.join("profile.yaml");
    std::fs::write(&p, PROFILE).expect("write profile");
    let profile = Profile::load(&p).expect("fixture profile");
    (dir, profile)
}

fn square(x0: f64, y0: f64, d: f64) -> Box<serde_json::value::RawValue> {
    let g = json!({"type": "Polygon", "coordinates": [[[x0, y0], [x0 + d, y0], [x0 + d, y0 + d], [x0, y0 + d], [x0, y0]]]});
    serde_json::value::to_raw_value(&g).expect("raw geometry")
}

fn unit(pcode: &str, level: i16, parent: Option<&str>, x: f64, d: f64) -> AdminUnitIn {
    AdminUnitIn {
        pcode: pcode.into(),
        level,
        name_en: pcode.into(),
        name_si: None,
        name_ta: None,
        parent_pcode: parent.map(Into::into),
        geometry: square(x, 7.0, d),
        artifact: 0,
    }
}

fn census_row(ind: &str, geo: &str, sex: &str, v: Option<f64>) -> ObsRow {
    ObsRow {
        indicator_id: ind.into(),
        geo_code: geo.into(),
        period: "2024".into(),
        dims: if sex.is_empty() {
            String::new()
        } else {
            format!("sex={sex}")
        },
        value: v,
        lo: None,
        hi: None,
        basis: Basis::Reported,
        status: Status::Normal,
        flags: Flags::EMPTY,
        artifact: 0,
        prov: None,
        locator: json!({"sheet": "T", "cell": format!("{geo}{sex}")}),
        verbatim: v.map(|v| v.to_string()),
        method: Method::SheetCell,
        confidence: Some(1.0),
    }
}

/// District census: ZZ11 provisional + INTERPOLATED_SPACE, ZZ21 female
/// suppressed. `bump` changes ZZ12's population (an input revision).
fn census(bump: f64) -> Vec<ObsRow> {
    let mut rows = Vec::new();
    for (g, m, f) in [
        ("ZZ11", 100.0, 110.0),
        ("ZZ12", 200.0 + bump, 190.0),
        ("ZZ21", 50.0, 60.0),
    ] {
        let mut t = census_row("it.pop", g, "total", Some(m + f));
        let mut mm = census_row("it.pop", g, "male", Some(m));
        let mut ff = census_row("it.pop", g, "female", Some(f));
        if g == "ZZ11" {
            for r in [&mut t, &mut mm, &mut ff] {
                r.status = Status::Provisional;
                r.flags = Flags::INTERPOLATED_SPACE;
            }
        }
        if g == "ZZ21" {
            ff.status = Status::Suppressed;
            ff.value = None;
            ff.verbatim = None;
        }
        rows.extend([t, mm, ff]);
    }
    rows.push(census_row("it.pop", "ZZ", "total", Some(710.0 + bump)));
    rows
}

struct Ctx {
    pool: PgPool,
    reg: Registry,
    profile: Profile,
    recipes: Recipes,
    methods: Methods,
}

impl Ctx {
    fn env(&self) -> DeriveEnv<'_> {
        DeriveEnv {
            pool: &self.pool,
            registry: &self.reg,
            profile: &self.profile,
            recipes: &self.recipes,
            methods: &self.methods,
        }
    }

    async fn train(&self) -> (TrainReport, Rep) {
        let env = self.env();
        let t = run_train_with(&self.pool, "test", None, None, Some(&env))
            .await
            .expect("train");
        let r = Rep(t.recipes.clone());
        assert!(r.0.get("error").is_none(), "recompute failed: {}", r.0);
        (t, r)
    }

    async fn stage(&self, dataset: &str, rows: &[ObsRow]) -> publish::PublishResult {
        let run = RunTracker::start(&self.pool, "it-src", "test")
            .await
            .expect("run");
        let art: i64 = sqlx::query_scalar(
            "INSERT INTO ops.artifacts (sha256, source_id, url, fetched_at, bytes, archive_path)
             VALUES (sha256(convert_to($1, 'UTF8')), 'it-src', 'https://example.org', now(), 1, 'x')
             ON CONFLICT (sha256) DO UPDATE SET sha256 = EXCLUDED.sha256 RETURNING id",
        )
        .bind(format!("{dataset}-{}", rows.len()))
        .fetch_one(&self.pool)
        .await
        .expect("artifact");
        let r = publish_observations(
            &self.pool,
            &self.reg,
            &self.profile,
            StageInput {
                run_id: run.run_id,
                dataset_id: dataset,
                vintage: None,
                rows,
                artifact_ids: &[art],
                extract_ids: &[],
            },
        )
        .await
        .expect("stage");
        run.finish(lanka_ingest::run::RunStatus::Succeeded, "fixture")
            .await
            .expect("finish");
        r
    }

    /// Approves every version in review (a reviewer reading the reports).
    async fn approve_all(&self) -> usize {
        let pending = publish::pending_reviews(&self.pool).await.expect("pending");
        for p in &pending {
            publish::approve(&self.pool, &p.dsv, "it-reviewer", Some("checked"))
                .await
                .expect("approve");
        }
        pending.len()
    }

    async fn value(
        &self,
        ind: &str,
        geo: &str,
        dims: &str,
    ) -> Option<(Option<f64>, i16, i16, i32)> {
        sqlx::query_as(
            "SELECT o.value, o.basis, o.status, o.flags FROM data.observations o
             JOIN catalog.indicators i ON i.key = o.indicator_key JOIN geo.units u ON u.key = o.geo_key
             JOIN catalog.dim_sets ds ON ds.key = o.dims_key
             JOIN data.current_versions cv ON cv.dataset_key = o.dataset_key
             WHERE i.id = $1 AND u.code = $2 AND ds.dims = $3 AND o.vfrom <= cv.seq AND o.vto >= cv.seq",
        )
        .bind(ind)
        .bind(geo)
        .bind(dims)
        .fetch_optional(&self.pool)
        .await
        .expect("value")
    }

    async fn obs_id(&self, ind: &str, geo: &str, dims: &str) -> i64 {
        sqlx::query_scalar(
            "SELECT o.obs_id FROM data.observations o
             JOIN catalog.indicators i ON i.key = o.indicator_key JOIN geo.units u ON u.key = o.geo_key
             JOIN catalog.dim_sets ds ON ds.key = o.dims_key
             WHERE i.id = $1 AND u.code = $2 AND ds.dims = $3 AND o.vto = 2147483647",
        )
        .bind(ind)
        .bind(geo)
        .bind(dims)
        .fetch_one(&self.pool)
        .await
        .expect("obs id")
    }
}

/// The train's recompute report (`TrainReport.recipes`).
#[derive(Debug)]
struct Rep(serde_json::Value);

impl Rep {
    fn n(&self, k: &str) -> u64 {
        self.0[k].as_u64().unwrap_or(u64::MAX)
    }
    fn get(&self, recipe: &str) -> &serde_json::Value {
        self.0["results"]
            .as_array()
            .and_then(|a| a.iter().find(|x| x["recipe"] == recipe))
            .unwrap_or(&serde_json::Value::Null)
    }
}

fn outcome<'a>(r: &'a Rep, recipe: &str) -> &'a str {
    r.get(recipe)["outcome"].as_str().unwrap_or("absent")
}

async fn with_database<F, Fut>(tag: &str, f: F)
where
    F: FnOnce(String) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let Ok(admin_url) = std::env::var("LANKA_TEST_DATABASE_URL") else {
        eprintln!("skipping: LANKA_TEST_DATABASE_URL not set");
        return;
    };
    let db = format!("lanka_it_{tag}_{}", std::process::id());
    let mut admin = PgConnection::connect(&admin_url)
        .await
        .expect("connect admin");
    let drop = format!("DROP DATABASE IF EXISTS {db} WITH (FORCE)");
    admin
        .execute(sqlx::raw_sql(sqlx::AssertSqlSafe(drop.clone())))
        .await
        .expect("drop");
    admin
        .execute(sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "CREATE DATABASE {db}"
        ))))
        .await
        .expect("create");
    let base = admin_url
        .rsplit_once('/')
        .map_or(admin_url.as_str(), |(b, _)| b);
    let url = format!("{base}/{db}");
    let result = tokio::spawn(f(url)).await;
    admin
        .execute(sqlx::raw_sql(sqlx::AssertSqlSafe(drop)))
        .await
        .expect("drop test database");
    if let Err(e) = result {
        std::panic::resume_unwind(e.into_panic());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn enrichment_cycle() {
    with_database("enrich", run_cycle).await;
}

#[allow(clippy::too_many_lines)]
async fn run_cycle(url: String) {
    let pool = lanka_core::db::connect(&url, lanka_core::db::PoolRole::Ingest)
        .await
        .expect("pool");
    lanka_core::db::migrate(&pool).await.expect("migrate");
    let (dir, profile) = fixture_registry();
    let reg = Registry::load(&dir).expect("fixture registry");
    reg.sync(&pool, &profile).await.expect("sync");
    let methods = Methods::builtin();
    let recipes = Recipes::load(&dir).expect("fixture recipes");
    let check = recipes.check(&derive::registry_view(&reg), &methods);
    assert!(check.errors.is_empty(), "{:?}", check.errors);
    // The official-sum recipe deliberately writes into a non-derived dataset
    // (the check refuses that; the runner must still be safe): added after
    // the check, as an operator mistake would be.
    let mut recipes = recipes;
    recipes.list.push(lanka_enrich::recipe::LoadedRecipe {
        spec: serde_json::from_value(json!({
            "id": "it-official-sum", "output": "it-official", "stability": "stable",
            "inputs": { "est": { "dataset": "it-est" } },
            "steps": [ { "id": "up", "method": "rollup@^1", "with": { "input": "est", "levels": [0] } } ],
            "emit": [ { "from": "up", "map": { "it.est_pop": "it.official_sum" } } ]
        }))
        .expect("spec"),
        path: dir.join("recipes/it-official-sum.yaml"),
    });
    let ctx = Ctx {
        pool: pool.clone(),
        reg,
        profile,
        recipes,
        methods,
    };

    // ── inputs: boundaries, census, an estimated series, fixture features ──
    let run = RunTracker::start(&pool, "it-src", "test")
        .await
        .expect("run");
    let units = vec![
        unit("ZZ", 0, None, 80.0, 1.0),
        unit("ZZ1", 1, Some("ZZ"), 80.0, 0.5),
        unit("ZZ2", 1, Some("ZZ"), 80.5, 0.5),
        unit("ZZ11", 2, Some("ZZ1"), 80.0, 0.25),
        unit("ZZ12", 2, Some("ZZ1"), 80.25, 0.25),
        unit("ZZ21", 2, Some("ZZ2"), 80.5, 0.5),
    ];
    let set = BoundarySet {
        id: "it-set".into(),
        title: "Fixture".into(),
        vintage: None,
        notes: None,
    };
    geo_load::publish_admin_units(
        &pool,
        &ctx.profile,
        run.run_id,
        "it-geo",
        &set,
        &units,
        &[],
        &[],
    )
    .await
    .expect("units");
    run.finish(lanka_ingest::run::RunStatus::Succeeded, "units")
        .await
        .expect("finish");
    assert_eq!(
        ctx.stage("it-census", &census(0.0)).await.outcome,
        "approved"
    );
    let mut est = Vec::new();
    for (g, v) in [("ZZ1", 300.0), ("ZZ2", 200.0)] {
        let mut r = census_row("it.est_pop", g, "", Some(v));
        r.basis = Basis::Estimated;
        r.lo = Some(v * 0.9);
        r.hi = Some(v * 1.1);
        est.push(r);
    }
    assert_eq!(ctx.stage("it-est", &est).await.outcome, "approved");
    // geo.features in the WP-04 shape (05 §4; migration 0007 delivers it).
    pool.execute(sqlx::raw_sql(
        "DROP TABLE geo.features CASCADE;
         CREATE TABLE geo.features (dataset_key int4 NOT NULL, feature_key text NOT NULL, kind text NOT NULL,
           unit_key int4, geom geometry(Geometry, 4326) NOT NULL, attrs jsonb, flags int4 NOT NULL DEFAULT 0,
           vfrom int4 NOT NULL, vto int4 NOT NULL DEFAULT 2147483647);",
    ))
    .await
    .expect("fixture features table");
    sqlx::query(
        "INSERT INTO data.dataset_versions (dataset_id, dsv, content_sha256, row_count, status, publication_class, seq)
         VALUES ('it-pois', 'dsv_fixture_pois', '\\x00', 4, 'approved', 'A', 1)",
    )
    .execute(&pool)
    .await
    .expect("features version");
    sqlx::query(
        "INSERT INTO geo.features (dataset_key, feature_key, kind, unit_key, geom, flags, vfrom)
         SELECT d.key, f.k, f.kind, u.key, ST_SetSRID(ST_MakePoint(80.1, 7.1), 4326), f.flags, 1
         FROM (VALUES ('n/1', 'hospital', 'ZZ11', 0), ('n/2', 'school', 'ZZ11', $1),
                      ('n/3', 'school', 'ZZ12', $2), ('n/4', 'school', 'ZZ21', 0)) f(k, kind, unit, flags)
         JOIN geo.units u ON u.code = f.unit, catalog.datasets d WHERE d.id = 'it-pois'",
    )
    .bind(Flags::UNOFFICIAL_GEOM.to_db())
    .bind(Flags::DISAGGREGATED.to_db())
    .execute(&pool)
    .await
    .expect("features");

    // ── train 1: inputs released; recipes run; first versions wait for review ──
    let (t1, r1) = ctx.train().await;
    assert_eq!(t1.outcome, "released");
    assert_eq!(outcome(&r1, "derived-it-admin"), "in_review", "{:?}", r1);
    assert_eq!(outcome(&r1, "derived-it-rollup"), "in_review");
    assert_eq!(outcome(&r1, "derived-it-density"), "waiting_inputs");
    assert_eq!(outcome(&r1, "derived-it-facilities"), "waiting_inputs");
    // estimated → official: the validator rejects it; nothing else is affected
    assert_eq!(outcome(&r1, "it-official-sum"), "failed");
    let err = r1.get("it-official-sum")["detail"].to_string();
    assert!(
        err.contains("basis estimated is not allowed for a official indicator"),
        "{err}"
    );
    let incidents: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ops.incidents WHERE kind = 'recipe_failed' AND subject = 'it-official-sum' AND closed_at IS NULL",
    )
    .fetch_one(&pool)
    .await
    .expect("incidents");
    assert_eq!(incidents, 1);
    assert_eq!(ctx.approve_all().await, 2);

    // ── train 2: admin + rollup released; density and facilities run ──
    let (t2, r2) = ctx.train().await;
    assert_eq!(t2.outcome, "released");
    assert_eq!(outcome(&r2, "derived-it-admin"), "reused");
    assert_eq!(outcome(&r2, "derived-it-rollup"), "reused");
    assert_eq!(outcome(&r2, "derived-it-density"), "in_review");
    assert_eq!(outcome(&r2, "derived-it-facilities"), "in_review");
    assert_eq!(ctx.approve_all().await, 2);
    let (t3, r3) = ctx.train().await;
    assert_eq!(t3.outcome, "released");
    assert_eq!(r3.n("ran"), 0, "{r3:?}");

    // ── fingerprint reuse: a train with nothing new runs nothing ──
    let (t4, r4) = ctx.train().await;
    assert_eq!(t4.outcome, "no_change");
    assert_eq!(
        (r4.n("ran"), r4.n("reused"), r4.n("failed")),
        (0, 4, 1),
        "{r4:?}"
    );

    // ── values ──
    let v = |x: Option<(Option<f64>, i16, i16, i32)>| x.expect("row");
    let (zz1, b, st, fl) = v(ctx.value("rollup.it.pop", "ZZ1", "sex=total").await);
    assert_eq!(zz1, Some(600.0));
    assert_eq!(b, Basis::Computed.code());
    assert_eq!(
        st,
        Status::Provisional.code(),
        "provisional district → provisional region"
    );
    assert_eq!(
        fl & (1 << 16),
        1 << 16,
        "INTERPOLATED_SPACE (bit 16) propagates"
    );
    let (zz2f, _, st, _) = v(ctx.value("rollup.it.pop", "ZZ2", "sex=female").await);
    assert_eq!(
        (zz2f, st),
        (None, Status::Suppressed.code()),
        "suppressed child → suppressed parent"
    );
    let (zz2t, _, st, fl) = v(ctx.value("rollup.it.pop", "ZZ2", "sex=total").await);
    assert_eq!((zz2t, st, fl), (Some(110.0), Status::Normal.code(), 0));
    let (area, ..) = v(ctx.value("admin.area_km2", "ZZ1", "").await);
    let area = area.expect("area");
    assert!(area > 1000.0, "geodesic km2 of a 0.5° square: {area}");
    assert_eq!(
        v(ctx.value("admin.child_count", "ZZ1", "").await).0,
        Some(2.0)
    );
    assert_eq!(
        v(ctx.value("admin.child_count", "ZZ", "").await).0,
        Some(2.0)
    );
    // density two recipes downstream: region from the roll-up, district from the census
    let (d1, _, st, fl) = v(ctx.value("it.density", "ZZ1", "sex=total").await);
    assert!((d1.expect("d") - 600.0 / area).abs() < 1e-9);
    assert_eq!(
        st,
        Status::Provisional.code(),
        "status crosses recipe boundaries"
    );
    assert_eq!(fl & (1 << 16), 1 << 16, "bit 16 crosses recipe boundaries");
    // facilities: counts, per 10k, flags from features (bits 14, 17)
    assert_eq!(
        v(ctx.value("it.fac_count", "ZZ", "amenity_type=total").await).0,
        Some(4.0)
    );
    assert_eq!(
        v(ctx
            .value("it.fac_count", "ZZ1", "amenity_type=school")
            .await)
        .0,
        Some(2.0)
    );
    let (n, _, _, fl) = v(ctx.value("it.fac_count", "ZZ1", "amenity_type=total").await);
    assert_eq!(n, Some(3.0));
    assert!(Flags::from_db_lossy(fl).contains(Flags::UNOFFICIAL_GEOM.union(Flags::DISAGGREGATED)));
    assert_eq!(
        fl & (1 << 17),
        1 << 17,
        "DISAGGREGATED (bit 17) propagates from features"
    );
    let (r, _, _, fl) = v(ctx
        .value("it.fac_per_10k", "ZZ1", "amenity_type=total")
        .await);
    assert!((r.expect("rate") - 3.0 / 600.0 * 10_000.0).abs() < 1e-9);
    assert!(
        Flags::from_db_lossy(fl).contains(Flags::DENOM_MISMATCH),
        "2026 features / 2024 census"
    );
    assert!(Flags::from_db_lossy(fl).contains(Flags::DISAGGREGATED));
    let (r, _, _, _) = v(ctx
        .value("it.fac_per_10k", "ZZ21", "amenity_type=school")
        .await);
    assert!((r.expect("rate") - 1.0 / 110.0 * 10_000.0).abs() < 1e-9);

    // ── lineage: no obs_provenance; derivations + explain ──
    let prov: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM data.obs_provenance p JOIN catalog.datasets d ON d.key = p.dataset_key
         WHERE d.recipe_id IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .expect("prov");
    assert_eq!(prov, 0, "derived rows have no obs_provenance (ADR 0005)");
    let deps: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM data.version_inputs vi JOIN data.dataset_versions v ON v.id = vi.version_id
         WHERE v.dataset_id = 'derived-it-density'",
    )
    .fetch_one(&pool)
    .await
    .expect("version_inputs");
    assert_eq!(deps, 3);
    let geo = GeoContext {
        calendars: ctx.profile.calendars.clone(),
        reference_set: "it-set".into(),
        scheme: "admin".into(),
    };
    let id = ctx.obs_id("rollup.it.pop", "ZZ1", "sex=total").await;
    let e = explain_obs(&pool, &geo, &ctx.methods, id, 1)
        .await
        .expect("explain")
        .expect("derived");
    assert_eq!(e["derivation"]["recipe"], "derived-it-rollup");
    let contributors = e["contributors"].as_array().expect("contributors");
    assert_eq!(contributors.len(), 2);
    let want: Vec<i64> = [
        ctx.obs_id("it.pop", "ZZ11", "sex=total").await,
        ctx.obs_id("it.pop", "ZZ12", "sex=total").await,
    ]
    .into();
    let got: Vec<i64> = contributors
        .iter()
        .filter_map(|c| c["obs_id"].as_i64())
        .collect();
    assert_eq!(got, want, "explain names the input rows by obs_id");
    let id = ctx.obs_id("it.density", "ZZ1", "sex=total").await;
    let e = explain_obs(&pool, &geo, &ctx.methods, id, 2)
        .await
        .expect("explain")
        .expect("derived");
    let cs = e["contributors"].as_array().expect("contributors");
    assert_eq!(cs.len(), 2);
    assert!(
        cs.iter().all(|c| c.get("provenance").is_some()),
        "depth 2 explains the derived contributors too: {e}"
    );
    let census_id = ctx.obs_id("it.pop", "ZZ11", "sex=total").await;
    assert!(
        explain_obs(&pool, &geo, &ctx.methods, census_id, 1)
            .await
            .expect("explain")
            .is_none()
    );

    // ── an input revision reruns exactly the dependent recipes ──
    let r = ctx.stage("it-census", &census(10.0)).await;
    assert!(
        matches!(r.outcome, "approved" | "in_review"),
        "{}",
        r.outcome
    );
    ctx.approve_all().await;
    let (_, r5) = ctx.train().await;
    assert_eq!(outcome(&r5, "derived-it-admin"), "reused");
    assert!(
        matches!(outcome(&r5, "derived-it-rollup"), "approved" | "in_review"),
        "{r5:?}"
    );
    for _ in 0..3 {
        ctx.approve_all().await;
        ctx.train().await;
    }
    assert_eq!(
        v(ctx.value("rollup.it.pop", "ZZ1", "sex=total").await).0,
        Some(610.0)
    );
    let (d1, ..) = v(ctx.value("it.density", "ZZ1", "sex=total").await);
    assert!(
        (d1.expect("d") - 610.0 / area).abs() < 1e-9,
        "the revision reached the downstream recipe"
    );
    let (_, r6) = ctx.train().await;
    assert_eq!((r6.n("ran"), r6.n("reused")), (0, 4), "{r6:?}");

    // the failed recipe keeps failing (deterministically) without blocking anything
    assert_eq!(outcome(&r6, "it-official-sum"), "failed");
}

#[test]
fn repository_recipes_validate() {
    let root = repo_root().join("registry");
    let reg = Registry::load(&root).expect("registry");
    let recipes =
        Recipes::load(&root).expect("recipes validate against registry/schema/recipe.schema.json");
    assert!(recipes.list.len() >= 4);
    let rep = recipes.check(&derive::registry_view(&reg), &Methods::builtin());
    assert!(rep.errors.is_empty(), "{:?}", rep.errors);
    let pos = |id: &str| rep.order.iter().position(|x| x == id).expect(id);
    assert!(pos("derived-census-2024-rollups") < pos("derived-per-capita-indicators"));
    assert!(pos("derived-admin-metrics") < pos("derived-per-capita-indicators"));
    assert!(pos("derived-census-2024-rollups") < pos("derived-facility-counts"));
}
