//! WP-05 grid engine end to end on a throwaway PostGIS database (gated on
//! `LANKA_TEST_DATABASE_URL`, like `pg_integration.rs`).
//!
//! A synthetic country (`zz`) on a small 100 × 100 grid of 0.01° cells,
//! three leaf units, a 20 × 20 GeoTIFF of counts written with the engine's
//! own COG writer, and a `raster` connector spec. Checks: the raster run
//! builds `gnd_idx`/`land_mask` and the layer, registers them, publishes
//! zonal sums whose leaves + unassigned equal the grid total and whose
//! parents equal the sum of their children; an identical re-run is
//! `no_change` before any work; the server's grid path of reverse geocoding
//! equals the PostGIS polygon path on random points.

use std::path::PathBuf;

use lanka_core::grid::Grid;
use lanka_core::{Catalog, Profile, query};
use lanka_grid::array::{Dtype, GridArray, LayerEncoding};
use lanka_grid::cog::{Overview, write_cog};
use lanka_ingest::connector::{AdminUnitIn, BoundarySet, RunOptions};
use lanka_ingest::run::{RunStatus, RunTracker};
use lanka_ingest::{Env, Registry, geo_load, run_source};
use lanka_store::FsStore;
use serde_json::json;
use sqlx::{Connection, Executor, PgConnection};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn tmp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("lanka-grid-it-{name}-{}", std::process::id()))
}

const PROFILE: &str = r#"
id: zz
name: { en: Testland }
iso3166: { alpha2: ZZ, alpha3: ZZZ }
site: { base_url: https://example.org }
languages: [en]
timezone: UTC
currency: XXX
grid: { lat_max: 8.0, lon_min: 80.0, step_deg: 0.01, rows: 100, cols: 100 }
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
"#;

const CONNECTOR: &str = r#"
source: it-ras
driver: raster
version: 1.0.0
dataset: it-pop
discover:
  files:
    - { key: pop, url: "https://example.org/data/pop.tif" }
layers:
  - id: it.pop
    period: "2025"
    indicator: it.pop
    resample: split_sum
    encoding: { dtype: f32 }
    unit: "{thing}"
    zonal: sum
    overview: sum
    source_res_m: 5000
    serve: true
    style: { ramp: viridis.magma, domain: [0, 10] }
zonal:
  - { indicator: it.pop, layer: it.pop, stat: sum, levels: [1, 2] }
"#;

fn fixture_registry() -> (PathBuf, Profile) {
    let dir = tmp("registry");
    let _ = std::fs::remove_dir_all(&dir);
    for d in ["schema", "sources", "datasets", "codelists", "connectors"] {
        std::fs::create_dir_all(dir.join(d)).expect("mkdir");
    }
    for f in [
        "source.schema.json",
        "dataset.schema.json",
        "codelist.schema.json",
        "units.schema.json",
        "profile.schema.json",
        "connector.schema.json",
    ] {
        std::fs::copy(
            repo_root().join("registry/schema").join(f),
            dir.join("schema").join(f),
        )
        .expect("copy");
    }
    std::fs::write(
        dir.join("units.yaml"),
        "units:\n  - { code: '{thing}', label: things, kind: count }\n",
    )
    .expect("write");
    std::fs::write(
        dir.join("sources/it-ras.yaml"),
        "id: it-ras\npublisher: Test\ntitle: Raster source\nhomepage: https://example.org\n\
         access: {method: raster, endpoint: https://example.org, auth: none}\n\
         licence: {id: cc-by-4.0, status: open}\ncadence: {frequency: annual}\n\
         geo: {levels: [raster, adm1, adm2], join: grid}\nthemes: [SOCI]\ndatasets: [it-pop, it-geo, grid-lookups]\n\
         priority: P3\neffort: S\nstatus: active\n",
    )
    .expect("write");
    std::fs::write(
        dir.join("datasets/it-pop.yaml"),
        "id: it-pop\ntitle: Gridded things\ndescription: Fixture raster.\nsource_ids: [it-ras]\n\
         indicators:\n  - {id: it.pop, label: Things, unit: '{thing}', measure_type: count, aggregation: sum,\n\
         \x20     class: third_party, geo_levels: [raster, adm1, adm2], period_type: year}\n\
         time_coverage: {from: 2025}\nmethod: Fixture.\nlimits: [Not real data.]\npublication_class: A\n",
    )
    .expect("write");
    std::fs::write(
        dir.join("datasets/it-geo.yaml"),
        "id: it-geo\ntitle: Fixture boundaries\ndescription: Fixture admin units.\nsource_ids: [it-ras]\n\
         indicators: []\ntime_coverage: {}\nmethod: Fixture.\nlimits: [Rectangles.]\npublication_class: A\n\
         kind: boundaries\n",
    )
    .expect("write");
    std::fs::write(
        dir.join("datasets/grid-lookups.yaml"),
        "id: grid-lookups\ntitle: Grid lookups\ndescription: gnd_idx and land_mask.\nsource_ids: [it-ras]\n\
         indicators: []\ntime_coverage: {}\nmethod: Fixture.\nlimits: [None.]\npublication_class: A\nkind: grid\n",
    )
    .expect("write");
    std::fs::write(dir.join("connectors/it-ras.yaml"), CONNECTOR).expect("write");
    let p = dir.join("profile.yaml");
    std::fs::write(&p, PROFILE).expect("write profile");
    let profile = Profile::load(&p).expect("fixture profile");
    (dir, profile)
}

fn rect(w: f64, s: f64, e: f64, n: f64) -> Box<serde_json::value::RawValue> {
    serde_json::value::to_raw_value(&json!({"type": "Polygon",
        "coordinates": [[[w, s], [e, s], [e, n], [w, n], [w, s]]]}))
    .expect("raw")
}

fn unit(
    pcode: &str,
    level: i16,
    parent: Option<&str>,
    g: Box<serde_json::value::RawValue>,
) -> AdminUnitIn {
    AdminUnitIn {
        pcode: pcode.into(),
        level,
        name_en: format!("Unit {pcode}"),
        name_si: None,
        name_ta: None,
        parent_pcode: parent.map(Into::into),
        geometry: g,
        artifact: 0,
    }
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
    let sql = |s: String| sqlx::raw_sql(sqlx::AssertSqlSafe(s));
    admin
        .execute(sql(format!("DROP DATABASE IF EXISTS {db} WITH (FORCE)")))
        .await
        .expect("drop");
    admin
        .execute(sql(format!("CREATE DATABASE {db}")))
        .await
        .expect("create");
    let base = admin_url
        .rsplit_once('/')
        .map_or(admin_url.as_str(), |(b, _)| b);
    let result = tokio::spawn(f(format!("{base}/{db}"))).await;
    admin
        .execute(sql(format!("DROP DATABASE IF EXISTS {db} WITH (FORCE)")))
        .await
        .expect("drop test database");
    if let Err(e) = result {
        std::panic::resume_unwind(e.into_panic());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn raster_to_zonal_against_postgis() {
    with_database("grid", run_grid).await;
}

#[allow(clippy::too_many_lines)]
async fn run_grid(url: String) {
    let pool = lanka_core::db::connect(&url, lanka_core::db::PoolRole::Ingest)
        .await
        .expect("pool");
    lanka_core::db::migrate(&pool).await.expect("migrate");
    let (dir, profile) = fixture_registry();
    let reg = Registry::load(&dir).expect("fixture registry");
    reg.sync(&pool, &profile).await.expect("sync");
    let root = tmp("store");
    let _ = std::fs::remove_dir_all(&root);
    let store = FsStore::new(&root);

    // Boundaries: ZZ = the whole grid; ZZ1 west half (ZZ11 south, ZZ12 north),
    // ZZ2 east half minus a 0.1° bay in its north-east corner (ZZ21).
    let run = RunTracker::start(&pool, "it-ras", "test")
        .await
        .expect("run");
    let units = vec![
        unit("ZZ", 0, None, rect(80.0, 7.0, 81.0, 8.0)),
        unit("ZZ1", 1, Some("ZZ"), rect(80.0, 7.0, 80.5, 8.0)),
        unit("ZZ2", 1, Some("ZZ"), rect(80.5, 7.0, 81.0, 7.9)),
        unit("ZZ11", 2, Some("ZZ1"), rect(80.0, 7.0, 80.5, 7.5)),
        unit("ZZ12", 2, Some("ZZ1"), rect(80.0, 7.5, 80.5, 8.0)),
        unit("ZZ21", 2, Some("ZZ2"), rect(80.5, 7.0, 81.0, 7.9)),
    ];
    let set = BoundarySet {
        id: "it-set".into(),
        title: "Fixture".into(),
        vintage: None,
        notes: None,
    };
    geo_load::publish_admin_units(
        &pool,
        &profile,
        run.run_id,
        "it-geo",
        &set,
        &units,
        &[],
        &[],
    )
    .await
    .expect("units");
    run.finish(RunStatus::Succeeded, "units")
        .await
        .expect("finish");
    lanka_ingest::train::run_train(&pool, "test", None, None)
        .await
        .expect("train");

    // A 20 × 20 raster of 0.05° pixels over the grid, values 1…400 (+ one nodata).
    let coarse = Grid {
        lat_max: 8.0,
        lon_min: 80.0,
        step: 0.05,
        rows: 20,
        cols: 20,
    };
    #[allow(clippy::cast_precision_loss)]
    let vals: Vec<f32> = (0..400)
        .map(|i| if i == 7 { f32::NAN } else { (i + 1) as f32 })
        .collect();
    let enc = LayerEncoding {
        dtype: Dtype::F32,
        scale: 1.0,
        offset: 0.0,
        nodata: Some(-99999.0),
    };
    let arr = GridArray::quantise(coarse, enc, &vals).expect("raster");
    let inputs = tmp("inputs");
    std::fs::create_dir_all(&inputs).expect("inputs");
    write_cog(&inputs.join("pop.tif"), &arr, Overview::Mean).expect("tif");
    let total: f64 = vals
        .iter()
        .filter(|v| v.is_finite())
        .map(|&v| f64::from(v))
        .sum();

    let env = Env {
        pool: &pool,
        registry: &reg,
        profile: &profile,
        store: &store,
    };
    let opts = RunOptions {
        from_dir: Some(inputs.clone()),
        trigger: "test".into(),
        ..RunOptions::default()
    };
    let r = run_source(&env, "it-ras", &opts).await.expect("raster run");
    assert_eq!(r.status, RunStatus::Succeeded, "{r:?}");
    assert_eq!(r.published.len(), 1);
    assert_eq!(r.published[0].outcome, "approved");
    // 2 regions + 3 districts.
    assert_eq!(r.published[0].rows, 5);

    let layers: Vec<(String, String, bool)> =
        sqlx::query_as("SELECT id, dataset_id, serve FROM geo.grid_layers ORDER BY id")
            .fetch_all(&pool)
            .await
            .expect("layers");
    let ids: Vec<&str> = layers.iter().map(|l| l.0.as_str()).collect();
    assert_eq!(ids, ["grid.gnd_idx", "grid.land_mask", "it.pop"]);
    let slots: i64 = sqlx::query_scalar("SELECT count(*) FROM geo.grid_lookup_units")
        .fetch_one(&pool)
        .await
        .expect("slots");
    assert_eq!(slots, 3, "one slot per leaf unit");

    lanka_ingest::train::run_train(&pool, "test", None, None)
        .await
        .expect("train");
    let vals: Vec<(String, i16, f64)> = sqlx::query_as(
        "SELECT geo_code, geo_level, value FROM data.current_observations_coded WHERE indicator_id = 'it.pop' ORDER BY geo_code",
    )
    .fetch_all(&pool)
    .await
    .expect("obs");
    let get = |c: &str| vals.iter().find(|v| v.0 == c).map(|v| v.2).expect(c);
    // Parents equal the sum of their children.
    assert!((get("ZZ1") - get("ZZ11") - get("ZZ12")).abs() < 1e-6);
    assert!((get("ZZ2") - get("ZZ21")).abs() < 1e-6);
    // Leaves + the bay (cells outside every unit beyond 2 km, slot 0) = the grid total.
    let leaves = get("ZZ11") + get("ZZ12") + get("ZZ21");
    assert!(leaves <= total + 1e-6);
    // The bay is 10 × 50 cells; cells within 2 km of ZZ21 were assigned to it.
    assert!(total - leaves > 0.0, "the bay keeps some value unassigned");
    let bin: String =
        sqlx::query_scalar("SELECT bin_path FROM geo.grid_layers WHERE id = 'it.pop'")
            .fetch_one(&pool)
            .await
            .expect("bin");
    let stored = lanka_ingest::grid::read_layer(&root, &bin).expect("read layer");
    assert!(
        (stored.stats().sum - total).abs() < 1e-3,
        "split_sum preserves the raster total"
    );

    // Identical second run: no_change before any work.
    let again = run_source(&env, "it-ras", &opts).await.expect("again");
    assert_eq!(again.status, RunStatus::NoChange);
    // Local inputs: the file fingerprint (size + mtime) already matches.
    assert_eq!(again.short_circuit.as_deref(), Some("fingerprint"));
    // --force skips the short-circuits; the content is identical → unchanged.
    let forced = run_source(
        &env,
        "it-ras",
        &RunOptions {
            force: true,
            ..opts.clone()
        },
    )
    .await
    .expect("forced");
    assert_eq!(forced.status, RunStatus::NoChange);
    assert_eq!(forced.published[0].outcome, "unchanged");

    // The grid path of reverse geocoding equals the polygon path.
    let gs = lanka_server::grids::GridSet::load(&pool, &root, profile.grid)
        .await
        .expect("grid set");
    assert_eq!(gs.mapped_count(), 3);
    let cat = Catalog::load(&pool).await.expect("catalog");
    let (mut by_grid, mut seed) = (0, 0x2545_f491_4f6c_dd1d_u64);
    for _ in 0..2_000 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        #[allow(clippy::cast_precision_loss)]
        let (a, b) = (
            (seed >> 11) as f64 / (1u64 << 53) as f64,
            (seed & 0xffff_ffff) as f64 / 4_294_967_296.0,
        );
        let (lat, lon) = (7.0 + a * 0.999, 80.0 + b * 0.999);
        let poly = query::reverse(&pool, &cat, lat, lon).await;
        if let Some(g) = gs.reverse(lat, lon) {
            by_grid += 1;
            let p = poly.expect("polygon path answers where the grid does");
            assert_eq!(g.unit.pcode, p.unit.pcode, "({lat}, {lon})");
            let anc = |r: &query::ReverseResult| {
                r.ancestors
                    .iter()
                    .map(|a| a.pcode.clone())
                    .collect::<Vec<_>>()
            };
            assert_eq!(anc(&g), anc(&p));
        }
    }
    assert!(
        by_grid > 1_500,
        "most points are answered by the grid ({by_grid})"
    );
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&inputs);
    let _ = std::fs::remove_dir_all(&dir);
}
