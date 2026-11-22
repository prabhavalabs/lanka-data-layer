//! `http_json`: REST/JSON APIs, one request per key (03 §2.2).
//!
//! ```yaml
//! source: worldbank-wdi
//! driver: http_json
//! version: 1.0.0
//! discover:
//!   url: https://api.example.org/{alpha3}/series/{key}?format=json
//!   keys:
//!     list: [A.B, C.D]                       # fixed keys …
//!     from_dataset:                          # … plus one per named indicator
//!       dataset: example-ds
//!       exclude: [ex.series]
//!       code: { strip_prefix: "ex.", replace: [["_", "."]], case: upper }   # upstream_id wins
//!   fingerprint: { url: https://api.example.org/meta, select: ["$[1][0].lastupdated"] }
//! extract:
//!   meta: { path: "$[0]", fields: { page: page, pages: pages }, require: [page] }
//!   single_page: { page: page, pages: pages }
//!   records: { path: "$[1]", fields: { date: date, value: value, label: indicator.value } }
//! standardise:
//!   dataset: example-ds
//!   vintage: "Example lastupdated {max:meta.lastupdated}"
//!   indicator: { by_key: true, generic: { indicator: ex.series, dim: series_code } }
//!   geo: { root: true }
//!   period: { field: date, format: year }
//!   value: { field: value }
//!   status: { field: obs_status, map: { E: [estimated, normal] }, default: [reported, normal] }
//!   locator: { url: "{url}", path: "{path}.value", series: "{key}", label: "{field.label}" }
//!   verbatim: "{field.value}"
//!   method: api
//! ```
//!
//! Extract writes one `records` extract: per artifact a `meta` record and one
//! `data` record per element of the records array, each field as its exact
//! JSON token (numbers) or unescaped string. Standardise maps records to
//! observations: a record without a value is skipped (the API's `null`),
//! keys with a named indicator go to it, others to the generic indicator
//! with the key as a dimension. Templates: `{url}`, `{key}`, `{path}`,
//! `{record_idx}`, `{field.X}`, `{meta.X}`; a template whose placeholder has
//! no value renders as JSON `null`.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use anyhow::{Context, Result, bail, ensure};
use async_trait::async_trait;
use lanka_core::{Basis, Flags, Method, Status};
use serde::Deserialize;
use serde_json::value::RawValue;
use sha2::{Digest, Sha256};

use super::json_path::{elements, select, text, truncate};
use crate::connector::{
    Candidate, CellProv, Connector, DiscoverCtx, Discovery, ExtractCtx, Fetched, ObsRow,
    OutputHeader, OutputKind, OutputSink, StdCtx, dims_key,
};
use crate::extract::{ExtractOut, ExtractRef, Record};
use crate::registry::{ConnectorSpec, Loaded, Registry};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub source: String,
    pub driver: String,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    pub discover: DiscoverSpec,
    pub extract: ExtractSpec,
    pub standardise: StdSpec,
    /// Expectations (expect.rs); read by the pipeline, not the driver.
    #[serde(default)]
    pub expect: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoverSpec {
    pub url: String,
    #[serde(default)]
    pub keys: KeysSpec,
    #[serde(default)]
    pub media_type: Option<String>,
    #[serde(default)]
    pub fingerprint: Option<FingerprintSpec>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeysSpec {
    #[serde(default)]
    pub list: Vec<String>,
    #[serde(default)]
    pub from_dataset: Option<FromDataset>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FromDataset {
    pub dataset: String,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub code: CodeRule,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Case {
    Upper,
    Lower,
}

/// Indicator id → upstream code, when the indicator has no `upstream_id`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodeRule {
    #[serde(default)]
    pub strip_prefix: Option<String>,
    #[serde(default)]
    pub replace: Vec<[String; 2]>,
    #[serde(default)]
    pub case: Option<Case>,
}

impl CodeRule {
    #[must_use]
    pub fn apply(&self, id: &str) -> String {
        let mut s = self
            .strip_prefix
            .as_deref()
            .and_then(|p| id.strip_prefix(p))
            .unwrap_or(id)
            .to_owned();
        for [from, to] in &self.replace {
            s = s.replace(from.as_str(), to);
        }
        match self.case {
            Some(Case::Upper) => s.to_ascii_uppercase(),
            Some(Case::Lower) => s.to_ascii_lowercase(),
            None => s,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FingerprintSpec {
    pub url: String,
    pub select: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableSel {
    pub path: String,
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
    /// Fields that must be present (else the response is an upstream error).
    #[serde(default)]
    pub require: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageCheck {
    pub page: String,
    pub pages: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtractSpec {
    #[serde(default)]
    pub meta: Option<TableSel>,
    /// Meta fields naming the current page and page count; more than one
    /// page fails the extract (raise the page size in the URL).
    #[serde(default)]
    pub single_page: Option<PageCheck>,
    pub records: TableSel,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Generic {
    pub indicator: String,
    pub dim: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndicatorRoute {
    /// Route by the request key through `discover.keys.from_dataset`.
    #[serde(default)]
    pub by_key: bool,
    #[serde(default)]
    pub generic: Option<Generic>,
    #[serde(default)]
    pub fixed: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeoSel {
    /// The profile's root unit (national series).
    #[serde(default)]
    pub root: bool,
    #[serde(default)]
    pub field: Option<String>,
    #[serde(default)]
    pub fixed: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PeriodFormat {
    /// Exactly four digits.
    Year,
    /// Already a canonical period code.
    AsIs,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeriodSel {
    pub field: String,
    pub format: PeriodFormat,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValueSel {
    pub field: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusSel {
    pub field: String,
    #[serde(default)]
    pub map: BTreeMap<String, (Basis, Status)>,
    pub default: (Basis, Status),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StdSpec {
    pub dataset: String,
    #[serde(default)]
    pub vintage: Option<String>,
    pub indicator: IndicatorRoute,
    pub geo: GeoSel,
    pub period: PeriodSel,
    pub value: ValueSel,
    #[serde(default)]
    pub status: Option<StatusSel>,
    #[serde(default)]
    pub locator: BTreeMap<String, String>,
    #[serde(default)]
    pub verbatim: Option<String>,
    pub method: Method,
    /// Warn when a key delivered no values.
    #[serde(default)]
    pub warn_empty: bool,
}

/// The `http_json` driver for one source.
#[derive(Debug, Clone)]
pub struct HttpJson {
    spec: Spec,
    sha256: [u8; 32],
}

const META: &str = "meta";
const DATA: &str = "data";

impl HttpJson {
    pub fn from_spec(l: &Loaded<ConnectorSpec>) -> Result<Self> {
        let spec: Spec = serde_json::from_value(l.json.clone())
            .with_context(|| format!("{}: invalid http_json spec", l.path.display()))?;
        Self::new(spec, l.sha256)
    }

    pub fn new(spec: Spec, sha256: [u8; 32]) -> Result<Self> {
        let r = &spec.standardise.indicator;
        ensure!(
            r.fixed.is_some() != (r.by_key || r.generic.is_some()),
            "standardise.indicator: give either `fixed` or `by_key`/`generic`"
        );
        ensure!(
            !r.by_key || spec.discover.keys.from_dataset.is_some(),
            "standardise.indicator.by_key needs discover.keys.from_dataset"
        );
        let g = &spec.standardise.geo;
        ensure!(
            usize::from(g.root) + usize::from(g.field.is_some()) + usize::from(g.fixed.is_some())
                == 1,
            "standardise.geo: give exactly one of root, field, fixed"
        );
        Ok(Self { spec, sha256 })
    }

    #[must_use]
    pub const fn spec(&self) -> &Spec {
        &self.spec
    }

    /// Upstream code → named indicator id, from the registry dataset.
    fn named(&self, registry: &Registry) -> Result<HashMap<String, String>> {
        let Some(fd) = &self.spec.discover.keys.from_dataset else {
            return Ok(HashMap::new());
        };
        let ds = registry
            .dataset(&fd.dataset)
            .with_context(|| format!("registry has no dataset {}", fd.dataset))?;
        Ok(ds
            .indicators
            .iter()
            .filter(|i| !fd.exclude.contains(&i.id))
            .map(|i| {
                let code = i
                    .upstream_id
                    .clone()
                    .unwrap_or_else(|| fd.code.apply(&i.id));
                (code, i.id.clone())
            })
            .collect())
    }

    fn keys(&self, registry: &Registry) -> Result<Vec<String>> {
        let mut keys: BTreeSet<String> = self.spec.discover.keys.list.iter().cloned().collect();
        keys.extend(self.named(registry)?.into_keys());
        Ok(keys.into_iter().collect())
    }
}

/// Substitutes `{name}` placeholders; `None` when any has no value.
fn render(tpl: &str, lookup: &dyn Fn(&str) -> Option<String>) -> Result<Option<String>> {
    let mut out = String::with_capacity(tpl.len() + 16);
    let mut rest = tpl;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let j = rest[i..]
            .find('}')
            .with_context(|| format!("unclosed placeholder in {tpl:?}"))?;
        let name = &rest[i + 1..i + j];
        match lookup(name) {
            Some(v) => out.push_str(&v),
            None => return Ok(None),
        }
        rest = &rest[i + j + 1..];
    }
    out.push_str(rest);
    Ok(Some(out))
}

fn profile_vars<'a>(
    ctx_alpha: (&'a str, &'a str),
    key: &'a str,
) -> impl Fn(&str) -> Option<String> + 'a {
    move |n| match n {
        "key" => Some(key.to_owned()),
        "alpha3" => Some(ctx_alpha.0.to_owned()),
        "alpha2" => Some(ctx_alpha.1.to_owned()),
        _ => None,
    }
}

#[async_trait]
impl Connector for HttpJson {
    fn source_id(&self) -> &str {
        &self.spec.source
    }

    fn version(&self) -> &str {
        &self.spec.version
    }

    fn extractor(&self) -> &str {
        "http_json"
    }

    fn spec_sha256(&self) -> Option<[u8; 32]> {
        Some(self.sha256)
    }

    async fn discover(&self, ctx: &DiscoverCtx<'_>) -> Result<Discovery> {
        let iso = &ctx.profile.spec.iso3166;
        let alpha = (iso.alpha3.as_str(), iso.alpha2.as_str());
        let mut candidates = Vec::new();
        for key in self.keys(ctx.registry)? {
            let url = render(&self.spec.discover.url, &profile_vars(alpha, &key))?.with_context(
                || {
                    format!(
                        "discover.url: unknown placeholder in {:?}",
                        self.spec.discover.url
                    )
                },
            )?;
            candidates.push(Candidate {
                key,
                url,
                media_type: self.spec.discover.media_type.clone(),
                ..Candidate::default()
            });
        }
        ensure!(!candidates.is_empty(), "http_json: no keys to fetch");
        let fingerprint = match &self.spec.discover.fingerprint {
            None => None,
            Some(fp) => upstream_fingerprint(ctx, fp, &candidates, alpha).await,
        };
        Ok(Discovery {
            candidates,
            fingerprint,
        })
    }

    async fn extract(&self, _ctx: &ExtractCtx<'_>, fetched: &[Fetched]) -> Result<Vec<ExtractOut>> {
        let x = &self.spec.extract;
        let mut records = Vec::new();
        for (ai, f) in fetched.iter().enumerate() {
            let ai = u32::try_from(ai)?;
            let bytes = f.read()?;
            let root: &RawValue = serde_json::from_slice(&bytes)
                .with_context(|| format!("{}: response is not JSON", f.key))?;
            if let Some(m) = &x.meta {
                let meta = select(root, &m.path)?;
                let mut fields = BTreeMap::new();
                for (name, path) in &m.fields {
                    let v = match meta {
                        Some(mv) => text(select(mv, path).ok().flatten())?,
                        None => None,
                    };
                    if let Some(v) = v {
                        fields.insert(name.clone(), v);
                    }
                }
                if let Some(missing) = m.require.iter().find(|r| !fields.contains_key(*r)) {
                    bail!(
                        "{}: upstream error (no {missing:?} in the response): {}",
                        f.key,
                        truncate(meta.map_or(root.get(), RawValue::get), 300)
                    );
                }
                if let Some(pc) = &x.single_page {
                    let num = |k: &str| fields.get(k).and_then(|v| v.parse::<u64>().ok());
                    let (page, pages) = (num(&pc.page), num(&pc.pages));
                    if page != Some(1) || pages.is_none_or(|p| p > 1) {
                        bail!(
                            "{}: response is paginated (page {page:?} of {pages:?}); raise the page size",
                            f.key
                        );
                    }
                }
                records.push(Record {
                    table_id: META.to_owned(),
                    record_idx: 0,
                    path: m.path.clone(),
                    fields,
                    artifact_idx: ai,
                });
            }
            let arr = elements(select(root, &x.records.path)?)
                .with_context(|| format!("{}: {}", f.key, x.records.path))?;
            for (i, rec) in arr.into_iter().enumerate() {
                let mut fields = BTreeMap::new();
                for (name, path) in &x.records.fields {
                    if let Some(v) = text(select(rec, path)?)? {
                        fields.insert(name.clone(), v);
                    }
                }
                records.push(Record {
                    table_id: DATA.to_owned(),
                    record_idx: u32::try_from(i)?,
                    path: format!("{}[{i}]", x.records.path),
                    fields,
                    artifact_idx: ai,
                });
            }
        }
        Ok(vec![ExtractOut::Records(records)])
    }

    fn standardise(
        &self,
        ctx: &StdCtx<'_>,
        extracts: &[ExtractRef],
        sink: &mut dyn OutputSink,
    ) -> Result<()> {
        let s = &self.spec.standardise;
        let named = self.named(ctx.registry)?;
        let ds = ctx
            .registry
            .dataset(&s.dataset)
            .with_context(|| format!("registry has no dataset {}", s.dataset))?;
        if let Some(g) = &s.indicator.generic {
            ensure!(
                ds.indicators.iter().any(|i| i.id == g.indicator),
                "{} must declare the generic indicator {}",
                s.dataset,
                g.indicator
            );
        }
        let mut max_meta: BTreeMap<String, String> = BTreeMap::new();
        let mut rows = Vec::new();
        let mut delivered: BTreeMap<String, usize> = BTreeMap::new();
        for (ei, x) in extracts.iter().enumerate() {
            for a in &x.artifacts {
                delivered.entry(a.key.clone()).or_default();
            }
            let recs = x.data.records();
            let mut meta_of: HashMap<u32, &BTreeMap<String, String>> = HashMap::new();
            for r in recs.iter().filter(|r| r.table_id == META) {
                meta_of.insert(r.artifact_idx, &r.fields);
                for (k, v) in &r.fields {
                    if max_meta.get(k).is_none_or(|m| v > m) {
                        max_meta.insert(k.clone(), v.clone());
                    }
                }
            }
            for (ri, r) in recs.iter().enumerate().filter(|(_, r)| r.table_id == DATA) {
                let art = x.artifact(r.artifact_idx)?;
                delivered.entry(art.key.clone()).or_default();
                let Some(token) = r.fields.get(&s.value.field) else {
                    continue; // the API's null: no observation
                };
                let period = r
                    .fields
                    .get(&s.period.field)
                    .with_context(|| format!("{} {}: no period", art.key, r.path))?;
                let value: f64 = token
                    .parse()
                    .with_context(|| format!("{} {period}: non-numeric value {token}", art.key))?;
                if s.period.format == PeriodFormat::Year
                    && (period.len() != 4 || !period.bytes().all(|b| b.is_ascii_digit()))
                {
                    bail!("{}: unexpected non-annual period {period:?}", art.key);
                }
                let (indicator_id, dims) = match (&s.indicator.fixed, named.get(&art.key)) {
                    (Some(id), _) => (id.clone(), String::new()),
                    (None, Some(id)) if s.indicator.by_key => (id.clone(), String::new()),
                    (None, _) => {
                        let g =
                            s.indicator.generic.as_ref().with_context(|| {
                                format!("{}: no indicator for this key", art.key)
                            })?;
                        (
                            g.indicator.clone(),
                            dims_key(&[(g.dim.as_str(), art.key.as_str())]),
                        )
                    }
                };
                let geo_code = if s.geo.root {
                    ctx.profile.admin.root().to_owned()
                } else if let Some(g) = &s.geo.fixed {
                    g.clone()
                } else {
                    let f = s.geo.field.as_deref().unwrap_or_default();
                    r.fields
                        .get(f)
                        .cloned()
                        .with_context(|| format!("{} {}: no geo field {f:?}", art.key, r.path))?
                };
                let (basis, status) = match &s.status {
                    None => (Basis::Reported, Status::Normal),
                    Some(st) => r
                        .fields
                        .get(&st.field)
                        .and_then(|c| st.map.get(c))
                        .copied()
                        .unwrap_or(st.default),
                };
                let meta = meta_of.get(&r.artifact_idx);
                let vars = |n: &str| -> Option<String> {
                    match n {
                        "url" => Some(art.url.clone()),
                        "key" => Some(art.key.clone()),
                        "path" => Some(r.path.clone()),
                        "record_idx" => Some(r.record_idx.to_string()),
                        _ => n
                            .strip_prefix("field.")
                            .and_then(|f| r.fields.get(f).cloned())
                            .or_else(|| {
                                n.strip_prefix("meta.")
                                    .and_then(|f| meta.and_then(|m| m.get(f).cloned()))
                            }),
                    }
                };
                let mut locator = serde_json::Map::new();
                for (k, tpl) in &s.locator {
                    locator.insert(
                        k.clone(),
                        render(tpl, &vars)?
                            .map_or(serde_json::Value::Null, serde_json::Value::String),
                    );
                }
                let verbatim = match &s.verbatim {
                    Some(t) => render(t, &vars)?,
                    None => None,
                };
                *delivered.entry(art.key.clone()).or_default() += 1;
                rows.push(ObsRow {
                    indicator_id,
                    geo_code,
                    period: period.clone(),
                    dims,
                    value: Some(value),
                    lo: None,
                    hi: None,
                    basis,
                    status,
                    flags: Flags::EMPTY,
                    artifact: r.artifact_idx as usize,
                    prov: Some(CellProv {
                        extract: ei,
                        row: u32::try_from(ri)?,
                    }),
                    locator: serde_json::Value::Object(locator),
                    verbatim,
                    method: s.method,
                    confidence: Some(1.0),
                });
            }
        }
        if s.warn_empty {
            for (k, n) in &delivered {
                if *n == 0 {
                    ctx.warn(format!("{k}: no values for {}", ctx.profile.spec.name.en));
                }
            }
        }
        let vintage = match &s.vintage {
            Some(t) => render(t, &|n: &str| {
                n.strip_prefix("max:meta.")
                    .and_then(|f| max_meta.get(f).cloned())
            })?,
            None => None,
        };
        sink.begin(OutputHeader {
            dataset_id: s.dataset.clone(),
            kind: OutputKind::Observations,
            vintage,
        })?;
        for r in rows {
            sink.obs(r)?;
        }
        sink.end()
    }
}

/// sha256 over the candidate URLs and the selected upstream values; `None`
/// (= always fetch) when the fingerprint endpoint cannot be read.
async fn upstream_fingerprint(
    ctx: &DiscoverCtx<'_>,
    fp: &FingerprintSpec,
    candidates: &[Candidate],
    alpha: (&str, &str),
) -> Option<[u8; 32]> {
    let url = render(&fp.url, &profile_vars(alpha, "")).ok().flatten()?;
    let resp = ctx
        .http
        .get(&url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status);
    let body = match resp {
        Ok(r) => r.bytes().await.ok()?,
        Err(e) => {
            if let Some(run) = ctx.run {
                run.warn(
                    crate::run::Stage::Discover,
                    &format!("fingerprint endpoint {url} failed ({e}); fetching everything"),
                    None,
                )
                .await;
            }
            return None;
        }
    };
    let root: &RawValue = serde_json::from_slice(&body).ok()?;
    let mut h = Sha256::new();
    for c in candidates {
        h.update(c.url.as_bytes());
        h.update(b"\n");
    }
    for p in &fp.select {
        let v = text(select(root, p).ok()?).ok()??;
        h.update(p.as_bytes());
        h.update(b"=");
        h.update(v.as_bytes());
        h.update(b"\n");
    }
    Some(h.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connector::{CollectSink, Lookup, Output};
    use crate::extract::ExtractArtifact;
    use std::path::PathBuf;
    use std::sync::Arc;

    #[test]
    fn code_rule_and_templates() {
        let r = CodeRule {
            strip_prefix: Some("wdi.".into()),
            replace: vec![["_".into(), ".".into()]],
            case: Some(Case::Upper),
        };
        assert_eq!(r.apply("wdi.ny_gdp_mktp_cd"), "NY.GDP.MKTP.CD");
        let v = |n: &str| (n == "a").then(|| "x".to_owned());
        assert_eq!(render("p{a}.v", &v).expect("ok").as_deref(), Some("px.v"));
        assert_eq!(render("{b}", &v).expect("ok"), None);
        assert!(render("{a", &v).is_err());
    }

    // ── golden test: the WDI spec reproduces the retired world_bank.rs ──

    const SAMPLE: &str = r#"[{"page":1,"pages":1,"per_page":500,"total":3,"sourceid":"2","lastupdated":"2026-11-11"},
      [{"indicator":{"id":"SP.POP.TOTL","value":"Population, total"},"country":{"id":"LK","value":"Sri Lanka"},"countryiso3code":"LKA","date":"2025","value":21756000,"unit":"","obs_status":"","decimal":0},
       {"indicator":{"id":"SP.POP.TOTL","value":"Population, total"},"country":{"id":"LK","value":"Sri Lanka"},"countryiso3code":"LKA","date":"2024","value":null,"unit":"","obs_status":"","decimal":0},
       {"indicator":{"id":"SP.POP.TOTL","value":"Population, total"},"country":{"id":"LK","value":"Sri Lanka"},"countryiso3code":"LKA","date":"2023","value":2.5e7,"unit":"","obs_status":"E","decimal":0}]]"#;

    const GDP: &str = r#"[{"page":1,"pages":1,"per_page":500,"total":1,"lastupdated":"2026-10-28"},
      [{"indicator":{"id":"NY.GDP.MKTP.CD","value":"GDP (current US$)"},"date":"2024","value":98963000000.5,"obs_status":"P"}]]"#;

    fn registry() -> (Registry, lanka_core::Profile) {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../registry");
        let reg = Registry::load(&root).expect("registry");
        let p = lanka_core::Profile::load(lanka_core::Profile::discover(&root).expect("profile"))
            .expect("profile loads");
        (reg, p)
    }

    fn wdi(reg: &Registry) -> HttpJson {
        HttpJson::from_spec(&reg.connectors["worldbank-wdi"]).expect("spec")
    }

    fn fetched(tag: &str, body: &[(&str, &str)]) -> Vec<Fetched> {
        let dir = std::env::temp_dir().join(format!("lanka-httpjson-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        body.iter()
            .enumerate()
            .map(|(i, (key, text))| {
                let path = dir.join(format!("{i}.json"));
                std::fs::write(&path, text).expect("write");
                Fetched {
                    key: (*key).to_owned(),
                    url: format!("https://api.worldbank.org/v2/country/LKA/indicator/{key}?format=json&per_page=500"),
                    artifact_id: 100 + i64::try_from(i).expect("i"),
                    sha256: [u8::try_from(i).expect("i"); 32],
                    path,
                    bytes: text.len() as u64,
                    media_type: None,
                }
            })
            .collect()
    }

    async fn run(
        reg: &Registry,
        p: &lanka_core::Profile,
        f: &[Fetched],
    ) -> Result<(Vec<ObsRow>, Option<String>, Vec<String>)> {
        let d = wdi(reg);
        let out = d
            .extract(
                &ExtractCtx {
                    registry: reg,
                    profile: p,
                },
                f,
            )
            .await?;
        // Round-trip through Parquet: standardise reads what the store holds.
        let out: Vec<ExtractOut> = out
            .iter()
            .map(|o| {
                crate::extract::from_parquet(
                    crate::extract::to_parquet(o, &[]).expect("enc").into(),
                )
            })
            .collect::<Result<_>>()?;
        let refs: Vec<ExtractRef> = out
            .into_iter()
            .enumerate()
            .map(|(i, data)| ExtractRef {
                id: i64::try_from(i).expect("i") + 1,
                sha256: [0; 32],
                extractor: "http_json".into(),
                extractor_ver: "1.0.0".into(),
                artifacts: f
                    .iter()
                    .map(|x| ExtractArtifact {
                        artifact_id: x.artifact_id,
                        key: x.key.clone(),
                        url: x.url.clone(),
                        sha256: x.sha256,
                    })
                    .collect(),
                data: Arc::new(data),
            })
            .collect();
        let lookup = Lookup::default();
        let ctx = StdCtx::new(reg, p, &lookup);
        let mut sink = CollectSink::default();
        d.standardise(&ctx, &refs, &mut sink)?;
        let warnings = ctx.take_warnings();
        match sink.finish()?.pop() {
            Some(Output::Observations { rows, vintage, .. }) => Ok((rows, vintage, warnings)),
            _ => bail!("no observations output"),
        }
    }

    #[tokio::test]
    async fn wdi_spec_parses_values_with_provenance() {
        let (reg, p) = registry();
        let f = fetched(
            "ok",
            &[
                ("SP.POP.TOTL", SAMPLE),
                ("NY.GDP.MKTP.CD", GDP),
                (
                    "X.EMPTY",
                    r#"[{"page":1,"pages":0,"lastupdated":"2026-01-01"},null]"#,
                ),
            ],
        );
        let (rows, vintage, warnings) = run(&reg, &p, &f).await.expect("runs");
        assert_eq!(vintage.as_deref(), Some("WDI lastupdated 2026-11-11"));
        assert_eq!(
            warnings,
            vec!["X.EMPTY: no values for Sri Lanka".to_owned()]
        );
        assert_eq!(rows.len(), 3, "null values are skipped");
        let r0 = &rows[0];
        assert_eq!(
            (
                r0.indicator_id.as_str(),
                r0.dims.as_str(),
                r0.geo_code.as_str(),
                r0.period.as_str()
            ),
            ("wdi.series", "series_code=SP.POP.TOTL", "LK", "2025")
        );
        assert_eq!(r0.value, Some(21_756_000.0));
        assert_eq!(r0.verbatim.as_deref(), Some("21756000"));
        assert_eq!(
            r0.locator,
            serde_json::json!({
                "url": f[0].url, "path": "$[1][0].value", "series": "SP.POP.TOTL",
                "series_label": "Population, total",
            }),
            "locator identical to the retired world_bank.rs"
        );
        assert_eq!(rows[1].locator["path"], "$[1][2].value");
        assert_eq!(rows[1].verbatim.as_deref(), Some("2.5e7"));
        assert_eq!(
            (rows[1].basis, rows[1].status),
            (Basis::Estimated, Status::Normal)
        );
        assert_eq!(rows[1].artifact, 0);
        // A named indicator wins over the generic one; P = provisional.
        assert_eq!(
            (
                rows[2].indicator_id.as_str(),
                rows[2].dims.as_str(),
                rows[2].artifact
            ),
            ("wdi.ny_gdp_mktp_cd", "", 1)
        );
        assert_eq!(
            (rows[2].basis, rows[2].status),
            (Basis::Reported, Status::Provisional)
        );
        assert_eq!(rows[2].verbatim.as_deref(), Some("98963000000.5"));
        assert!(
            rows.iter()
                .all(|r| r.method == Method::Api && r.prov.is_some())
        );
    }

    #[tokio::test]
    async fn wdi_spec_surfaces_api_errors_and_pagination() {
        let (reg, p) = registry();
        let err = run(
            &reg,
            &p,
            &fetched(
                "err",
                &[(
                    "X.Y",
                    r#"[{"message":[{"id":"120","key":"Invalid value"}]}]"#,
                )],
            ),
        )
        .await
        .expect_err("must fail");
        assert!(format!("{err:#}").contains("Invalid value"), "{err:#}");
        let paged = r#"[{"page":1,"pages":2,"lastupdated":"x"},[]]"#;
        let err = run(&reg, &p, &fetched("page", &[("X.Y", paged)]))
            .await
            .expect_err("must fail");
        assert!(format!("{err:#}").contains("paginated"), "{err:#}");
        let quarterly = r#"[{"page":1,"pages":1},[{"date":"2024Q1","value":1}]]"#;
        let err = run(&reg, &p, &fetched("q", &[("X.Y", quarterly)]))
            .await
            .expect_err("must fail");
        assert!(format!("{err:#}").contains("non-annual"), "{err:#}");
    }

    #[test]
    fn wdi_keys_cover_core_and_named_series() {
        let (reg, _) = registry();
        let keys = wdi(&reg).keys(&reg).expect("keys");
        for k in [
            "SP.POP.TOTL",
            "NY.GDP.MKTP.CD",
            "NY.GDP.PCAP.PP.KD",
            "AG.LND.FRST.ZS",
        ] {
            assert!(keys.contains(&k.to_owned()), "{k}");
        }
        assert_eq!(
            keys.len(),
            27,
            "26 headline series + NY.GDP.PCAP.PP.KD (named only)"
        );
    }
}
