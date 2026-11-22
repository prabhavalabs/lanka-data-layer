//! Resolution of a document against the catalogue (10 §2.3): the
//! `resolved` block a renderer uses and never edits — dataset versions
//! (pinned release, exact `{dsv}` or latest), data URLs, attribution,
//! notices and repair problems. Pure over a catalogue snapshot plus the
//! release/version rows `load_context` reads, so the same function serves
//! `lanka map resolve`, the app plane (WP-28) and tests.
//!
//! Data shapes served today: frames (`indicator` sources, `/v2/frame`) and
//! boundaries. `features`, `grid` and `analysis` sources resolve to
//! `shape: "unavailable"` with a `NOT_SERVED` problem until WP-27/WP-29.

use std::collections::{BTreeMap, BTreeSet};

use lanka_core::Catalog;
use lanka_core::frame::{PinnedVersion, dsv_versions, release_versions};
use lanka_core::{CoreResult, Period};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::PgPool;

use crate::doc::{
    Extent, IndicatorSource, LevelRef, MapDocument, PeriodRef, Pin, Source, VersionRef,
};
use crate::refs::{host_of, is_workspace};
use crate::validate::ptr;

/// Release and version rows a resolution needs.
#[derive(Debug, Clone, Default)]
pub struct ResolveContext {
    /// The document pin's release and its versions (`None` = latest, or
    /// the release does not exist).
    pub release: Option<(String, Vec<PinnedVersion>)>,
    /// The pinned release was not found.
    pub release_missing: bool,
    /// Versions named by `{dsv}` sources.
    pub dsvs: Vec<PinnedVersion>,
}

/// Reads what `resolve` needs from Postgres.
pub async fn load_context(pool: &PgPool, doc: &MapDocument) -> CoreResult<ResolveContext> {
    let mut ctx = ResolveContext::default();
    if let Some(r) = doc.pin.release() {
        match release_versions(pool, r).await? {
            Some(v) => ctx.release = Some((r.to_owned(), v)),
            None => ctx.release_missing = true,
        }
    }
    let wanted: Vec<String> = doc
        .sources
        .values()
        .filter_map(|s| match s {
            Source::Indicator(i) => i.version.as_ref(),
            Source::Boundaries(b) => b.version.as_ref(),
            Source::Features(f) => f.version.as_ref(),
            Source::Grid(g) => g.version.as_ref(),
            _ => None,
        })
        .filter_map(|v| match v {
            VersionRef::Dsv { dsv } => Some(dsv.clone()),
            VersionRef::Latest(_) => None,
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if !wanted.is_empty() {
        ctx.dsvs = dsv_versions(pool, &wanted).await?;
    }
    Ok(ctx)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VersionUsed {
    pub dataset: String,
    pub dsv: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ZoomFrame {
    /// From this zoom on (until the next entry).
    pub zoom: f64,
    pub level: u8,
    pub frame: String,
    pub source_layer: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum Shape {
    Frame {
        frame: String,
        tileset: String,
        tiles: String,
        source_layer: String,
        join: String,
        centroids_layer: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        by_zoom: Option<Vec<ZoomFrame>>,
    },
    Boundaries {
        tiles: String,
        source_layer: String,
    },
    Inline,
    External {
        url: String,
    },
    /// Not drawable here: denied, not found, withdrawn or not served yet;
    /// the reason is in `problems`.
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedSource {
    /// public | granted | denied
    pub access: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dsv: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub versions: Vec<VersionUsed>,
    /// official | third_party | derived | estimated (the least official input).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub basis: Option<Vec<String>>,
    #[serde(flatten)]
    pub shape: Shape,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedAnalysis {
    pub id: Option<String>,
    /// queued | running | ready | failed | blocked | unavailable
    pub state: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Attribution {
    pub source_id: String,
    pub publisher: String,
    pub text: String,
    pub licence: String,
    pub licence_url: Option<String>,
    /// open | attribution | permission_unclear | restricted
    pub licence_status: String,
    /// bulk | api | link | internal
    pub redistribution: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Notice {
    /// ESTIMATE | COMMUNITY_DATA | PRIVATE_DATA | SYNTHETIC | EXTERNAL | LICENCE_UNCLEAR
    pub code: &'static str,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolveProblem {
    pub path: String,
    /// WITHDRAWN | NOT_FOUND | RENAMED | LEVEL_UNAVAILABLE | PERIOD_UNAVAILABLE | NOT_SERVED
    pub code: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Resolved {
    pub data_version: Option<String>,
    pub sources: BTreeMap<String, ResolvedSource>,
    pub analyses: BTreeMap<String, ResolvedAnalysis>,
    pub attribution: Vec<Attribution>,
    pub notices: Vec<Notice>,
    pub problems: Vec<ResolveProblem>,
}

/// The document with its `resolved` block (what the renderer receives).
#[derive(Debug, Clone, Serialize)]
pub struct ResolvedMap<'a> {
    #[serde(flatten)]
    pub doc: &'a MapDocument,
    pub resolved: Resolved,
}

/// Percent-encodes a query value (RFC 3986 unreserved plus `,` kept).
fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b',') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `level` → the indicator's `geo_levels` vocabulary.
fn level_name(level: u8) -> String {
    if level == 0 {
        "national".to_owned()
    } else {
        format!("adm{level}")
    }
}

fn admin_tiles() -> String {
    // The admin tileset route of 05 §9 (built by WP-04).
    "/v2/tiles/admin/{z}/{x}/{y}.mvt".to_owned()
}

struct Builder<'a> {
    cat: &'a Catalog,
    ctx: &'a ResolveContext,
    doc: &'a MapDocument,
    problems: Vec<ResolveProblem>,
    notices: BTreeSet<Notice>,
    datasets: BTreeSet<String>,
    external: Vec<(String, String)>,
}

impl Builder<'_> {
    fn problem(&mut self, path: String, code: &'static str, hint: Option<Value>) {
        self.problems.push(ResolveProblem { path, code, hint });
    }

    /// The version of `dataset` a source reads: `(seq-free dsv, pinned?)`;
    /// `Err(())` after recording a problem.
    fn version_of(
        &mut self,
        at: &str,
        dataset: &str,
        v: Option<&VersionRef>,
    ) -> Result<(String, VersionMode), ()> {
        let current = self
            .cat
            .datasets
            .get(dataset)
            .and_then(|d| d.current.as_ref());
        match v {
            Some(VersionRef::Dsv { dsv }) => match self.ctx.dsvs.iter().find(|p| &p.dsv == dsv) {
                Some(p) if p.dataset_id != dataset => {
                    self.problem(format!("{at}/version"), "NOT_FOUND", Some(json!({ "reason": format!("{dsv} is a version of {}, not {dataset}", p.dataset_id) })));
                    Err(())
                }
                Some(p) if p.status == "withdrawn" => {
                    let latest = current.map(|c| c.dsv.clone());
                    self.problem(
                        format!("{at}/version"),
                        "WITHDRAWN",
                        Some(json!({ "successor": latest })),
                    );
                    Err(())
                }
                Some(p) => Ok((p.dsv.clone(), VersionMode::Dsv(p.dsv.clone()))),
                None => {
                    self.problem(
                        format!("{at}/version"),
                        "NOT_FOUND",
                        Some(json!({ "dsv": dsv, "latest": current.map(|c| c.dsv.clone()) })),
                    );
                    Err(())
                }
            },
            Some(VersionRef::Latest(_)) => current
                .map(|c| (c.dsv.clone(), VersionMode::Latest))
                .ok_or_else(|| {
                    self.problem(
                        at.to_owned(),
                        "NOT_FOUND",
                        Some(json!({ "reason": format!("{dataset} has no published version") })),
                    );
                }),
            None => match &self.ctx.release {
                Some((r, rows)) => match rows.iter().find(|p| p.dataset_id == dataset) {
                    Some(p) => Ok((p.dsv.clone(), VersionMode::Release(r.clone()))),
                    None => {
                        self.problem(at.to_owned(), "NOT_FOUND", Some(json!({ "reason": format!("{dataset} is not in release {r}"), "release": r })));
                        Err(())
                    }
                },
                None => current
                    .map(|c| (c.dsv.clone(), VersionMode::Latest))
                    .ok_or_else(|| {
                        self.problem(
                            at.to_owned(),
                            "NOT_FOUND",
                            Some(
                                json!({ "reason": format!("{dataset} has no published version") }),
                            ),
                        );
                    }),
            },
        }
    }

    #[allow(clippy::too_many_lines)]
    fn indicator(&mut self, id: &str, s: &IndicatorSource) -> ResolvedSource {
        let at = format!("/sources/{}", ptr(id));
        let ids = s.indicator_ids();
        let unavailable = |class: Option<String>| ResolvedSource {
            access: "public",
            dsv: None,
            versions: Vec::new(),
            class,
            basis: None,
            shape: Shape::Unavailable,
        };
        if ids.iter().any(|i| is_workspace(i)) {
            self.notices.insert(Notice { code: "PRIVATE_DATA", text: "Some layers use private workspace data; they are shown only to members of that workspace.".to_owned() });
            return ResolvedSource {
                access: "denied",
                ..unavailable(None)
            };
        }
        let levels = s.level.levels();
        let mut versions: Vec<VersionUsed> = Vec::new();
        let mut modes: Vec<VersionMode> = Vec::new();
        let mut classes: Vec<String> = Vec::new();
        let mut ok = true;
        for (k, ind_id) in ids.iter().enumerate() {
            let iat = if s.indicators.is_some() {
                format!("{at}/indicators/{k}")
            } else {
                format!("{at}/indicator")
            };
            let Some(ind) = self.cat.indicators.get(*ind_id) else {
                self.problem(iat, "NOT_FOUND", Some(json!({ "indicator": ind_id })));
                ok = false;
                continue;
            };
            for l in &levels {
                if !ind.geo_levels.contains(&level_name(*l)) {
                    let avail: Vec<u8> = (0..=9)
                        .filter(|x| ind.geo_levels.contains(&level_name(*x)))
                        .collect();
                    self.problem(
                        format!("{at}/level"),
                        "LEVEL_UNAVAILABLE",
                        Some(json!({ "indicator": ind_id, "available_levels": avail })),
                    );
                    ok = false;
                }
            }
            if let Ok(freq) = ind.frequency()
                && let Ok(profile) = self.cat.profile()
            {
                let codes: Vec<&str> = match &s.period {
                    PeriodRef::Code(c) if c != "latest" => vec![c.as_str()],
                    PeriodRef::List(v) => v.iter().map(String::as_str).collect(),
                    PeriodRef::Range { from, to } => vec![from.as_str(), to.as_str()],
                    _ => Vec::new(),
                };
                for c in codes {
                    if Period::parse(freq, c, &profile.calendars).is_err() {
                        self.problem(
                            format!("{at}/period"),
                            "PERIOD_UNAVAILABLE",
                            Some(json!({ "indicator": ind_id, "period": c, "freq": ind.freq })),
                        );
                        ok = false;
                    }
                }
            }
            classes.push(ind.class.clone());
            match self.version_of(&at, &ind.dataset_id, s.version.as_ref()) {
                Ok((dsv, mode)) => {
                    if !versions.iter().any(|v| v.dataset == ind.dataset_id) {
                        versions.push(VersionUsed {
                            dataset: ind.dataset_id.clone(),
                            dsv,
                        });
                        self.datasets.insert(ind.dataset_id.clone());
                    }
                    modes.push(mode);
                }
                Err(()) => ok = false,
            }
        }
        let parent = s.parent.clone().or_else(|| match &self.doc.extent {
            Some(Extent::Geo { geo }) => Some(geo.clone()),
            _ => None,
        });
        if let (Some(p), Ok(profile)) = (&parent, self.cat.profile()) {
            match profile.admin.parse(p) {
                Ok(pc) if levels.iter().all(|l| pc.level() < *l) => {}
                Ok(_) => {
                    self.problem(
                        format!("{at}/parent"),
                        "LEVEL_UNAVAILABLE",
                        Some(json!({ "reason": format!("{p} is not above level {levels:?}") })),
                    );
                    ok = false;
                }
                Err(e) => {
                    self.problem(
                        format!("{at}/parent"),
                        "NOT_FOUND",
                        Some(json!({ "reason": e.to_string() })),
                    );
                    ok = false;
                }
            }
        }
        let class = least_official(&classes);
        if class.as_deref() == Some("estimated") {
            self.notices.insert(Notice { code: "ESTIMATE", text: "Some layers show modelled estimates, not official statistics; their legends carry the estimate badge.".to_owned() });
        }
        if !ok {
            return unavailable(class);
        }
        // One prefix for the whole source: a release when every dataset is
        // read at the document's release, else the current release, with
        // `dsv=` naming exact versions.
        let release = modes.iter().find_map(|m| match m {
            VersionMode::Release(r) => Some(r.clone()),
            _ => None,
        });
        let all_release = release.is_some()
            && modes
                .iter()
                .all(|m| matches!(m, VersionMode::Release(_) | VersionMode::Dsv(_)));
        let prefix = match (&release, all_release) {
            (Some(r), true) => format!("/v2/r/{r}/frame"),
            _ => "/v2/frame".to_owned(),
        };
        let mut dsvs: Vec<String> = modes
            .iter()
            .filter_map(|m| match m {
                VersionMode::Dsv(d) => Some(d.clone()),
                _ => None,
            })
            .collect();
        if release.is_some() && !all_release {
            // Mixed pins: name every release-pinned version exactly.
            for (m, v) in modes.iter().zip(&versions) {
                if matches!(m, VersionMode::Release(_)) {
                    dsvs.push(v.dsv.clone());
                }
            }
        }
        dsvs.sort();
        dsvs.dedup();
        let url = |level: u8| {
            let mut q = vec![
                format!("indicators={}", enc(&ids.join(","))),
                format!("level={level}"),
            ];
            if let Some(p) = &parent {
                q.push(format!("parent={}", enc(p)));
            }
            match &s.period {
                PeriodRef::Code(c) if c == "latest" => {}
                PeriodRef::Code(c) => q.push(format!("periods={}", enc(c))),
                PeriodRef::List(v) => q.push(format!("periods={}", enc(&v.join(",")))),
                PeriodRef::Range { from, to } => {
                    q.push(format!("from={}", enc(from)));
                    q.push(format!("to={}", enc(to)));
                }
                PeriodRef::Bind { .. } => {
                    // The frame carries every period of the time range (10 §2.7).
                    if let Some(t) = &self.doc.time {
                        q.push(format!("from={}", enc(&t.range[0])));
                        q.push(format!("to={}", enc(&t.range[1])));
                    }
                }
            }
            if let Some(d) = s.dims.as_ref().filter(|d| !d.is_empty()) {
                let canon: Vec<String> = d.iter().map(|(k, v)| format!("{k}={v}")).collect();
                q.push(format!("dims={}", enc(&canon.join(";"))));
            }
            if !dsvs.is_empty() {
                q.push(format!("dsv={}", enc(&dsvs.join(","))));
            }
            format!("{prefix}?{}", q.join("&"))
        };
        let first = levels.first().copied().unwrap_or(0);
        let by_zoom = match &s.level {
            LevelRef::ByZoom { by_zoom } => Some(
                by_zoom
                    .iter()
                    .map(|(z, l)| ZoomFrame {
                        zoom: *z,
                        level: *l,
                        frame: url(*l),
                        source_layer: format!("adm{l}"),
                    })
                    .collect(),
            ),
            LevelRef::Level(_) => None,
        };
        self.datasets
            .extend(self.cat.boundary_dataset().map(str::to_owned));
        ResolvedSource {
            access: "public",
            dsv: (versions.len() == 1).then(|| versions[0].dsv.clone()),
            versions,
            basis: (class.as_deref() == Some("estimated")).then(|| vec!["estimated".to_owned()]),
            class,
            shape: Shape::Frame {
                frame: url(first),
                tileset: "admin".to_owned(),
                tiles: admin_tiles(),
                source_layer: format!("adm{first}"),
                join: "pcode".to_owned(),
                centroids_layer: "centroids".to_owned(),
                by_zoom,
            },
        }
    }

    fn not_served(&mut self, at: String, what: &str, wp: &str) -> ResolvedSource {
        self.problem(
            at,
            "NOT_SERVED",
            Some(json!({ "reason": format!("{what} are served from {wp}") , "work_package": wp })),
        );
        ResolvedSource {
            access: "public",
            dsv: None,
            versions: Vec::new(),
            class: None,
            basis: None,
            shape: Shape::Unavailable,
        }
    }

    fn source(&mut self, id: &str, s: &Source) -> ResolvedSource {
        let at = format!("/sources/{}", ptr(id));
        let denied = || ResolvedSource {
            access: "denied",
            dsv: None,
            versions: Vec::new(),
            class: None,
            basis: None,
            shape: Shape::Unavailable,
        };
        let private_notice = Notice { code: "PRIVATE_DATA", text: "Some layers use private workspace data; they are shown only to members of that workspace.".to_owned() };
        match s {
            Source::Indicator(i) => self.indicator(id, i),
            Source::Boundaries(b) => {
                let Some(ds) = self.cat.boundary_dataset().map(str::to_owned) else {
                    self.problem(
                        at,
                        "NOT_FOUND",
                        Some(json!({ "reason": "no boundary dataset in the country profile" })),
                    );
                    return ResolvedSource {
                        shape: Shape::Unavailable,
                        ..denied()
                    };
                };
                let scheme_ok = self
                    .cat
                    .profile()
                    .is_ok_and(|p| p.admin_scheme() == b.scheme);
                if !scheme_ok {
                    return self.not_served(
                        format!("{at}/scheme"),
                        "boundaries of other schemes",
                        "WP-10",
                    );
                }
                match self.version_of(&at, &ds, b.version.as_ref()) {
                    Ok((dsv, _)) => {
                        self.datasets.insert(ds.clone());
                        ResolvedSource {
                            access: "public",
                            dsv: Some(dsv.clone()),
                            versions: vec![VersionUsed { dataset: ds, dsv }],
                            class: Some("official".to_owned()),
                            basis: None,
                            shape: Shape::Boundaries {
                                tiles: admin_tiles(),
                                source_layer: format!("adm{}", b.level),
                            },
                        }
                    }
                    Err(()) => ResolvedSource {
                        access: "public",
                        ..denied()
                    },
                }
            }
            Source::Features(f) if is_workspace(&f.dataset) => {
                self.notices.insert(private_notice);
                denied()
            }
            Source::Grid(g) if is_workspace(&g.grid) => {
                self.notices.insert(private_notice);
                denied()
            }
            Source::Features(f) => {
                if self.cat.datasets.contains_key(&f.dataset) {
                    self.not_served(at, "feature layers", "WP-04/WP-27")
                } else {
                    self.problem(
                        format!("{at}/dataset"),
                        "NOT_FOUND",
                        Some(json!({ "dataset": f.dataset })),
                    );
                    ResolvedSource {
                        access: "public",
                        ..denied()
                    }
                }
            }
            Source::Grid(_) => self.not_served(at, "grid value tiles", "WP-05/WP-27"),
            Source::Analysis(_) => self.not_served(at, "analysis outputs", "WP-29"),
            Source::Inline(_) => ResolvedSource {
                access: "public",
                dsv: None,
                versions: Vec::new(),
                class: Some("user".to_owned()),
                basis: None,
                shape: Shape::Inline,
            },
            Source::External(e) => {
                let host = host_of(&e.url).unwrap_or_else(|| e.url.clone());
                self.notices.insert(Notice { code: "EXTERNAL", text: format!("Data from {host} is loaded by your browser directly; Lanka Data Layer does not check or cache it.") });
                self.external.push((host, e.attribution.clone()));
                ResolvedSource {
                    access: "public",
                    dsv: None,
                    versions: Vec::new(),
                    class: Some("third_party".to_owned()),
                    basis: None,
                    shape: Shape::External { url: e.url.clone() },
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum VersionMode {
    Latest,
    Release(String),
    Dsv(String),
}

/// The least official class among inputs (estimated < derived < third_party < official).
fn least_official(classes: &[String]) -> Option<String> {
    let rank = |c: &str| match c {
        "estimated" => 0,
        "derived" => 1,
        "third_party" => 2,
        _ => 3,
    };
    classes.iter().min_by_key(|c| rank(c)).cloned()
}

/// Resolves a validated document.
#[must_use]
pub fn resolve(doc: &MapDocument, cat: &Catalog, ctx: &ResolveContext) -> Resolved {
    let mut b = Builder {
        cat,
        ctx,
        doc,
        problems: Vec::new(),
        notices: BTreeSet::new(),
        datasets: BTreeSet::new(),
        external: Vec::new(),
    };
    if ctx.release_missing
        && let Pin::Release { release } = &doc.pin
    {
        b.problem(
            "/pin".to_owned(),
            "NOT_FOUND",
            Some(json!({ "release": release, "fallback": "latest", "latest": cat.data_version })),
        );
    }
    let sources: BTreeMap<String, ResolvedSource> = doc
        .sources
        .iter()
        .map(|(id, s)| (id.clone(), b.source(id, s)))
        .collect();
    let mut analyses = BTreeMap::new();
    for (k, a) in &doc.analyses {
        analyses.insert(
            k.clone(),
            ResolvedAnalysis {
                id: a.get("result").and_then(Value::as_str).map(str::to_owned),
                state: "unavailable",
            },
        );
        b.problem(
            format!("/analyses/{}", ptr(k)),
            "NOT_SERVED",
            Some(json!({ "reason": "analyses are computed from WP-29", "work_package": "WP-29" })),
        );
    }

    // Attribution: every source behind every dataset read, then external hosts.
    let mut attribution: BTreeSet<Attribution> = BTreeSet::new();
    for d in &b.datasets {
        let Some(ds) = cat.datasets.get(d) else {
            continue;
        };
        for sid in &ds.source_ids {
            let Some(src) = cat.sources.get(sid) else {
                continue;
            };

            let lic = cat.licences.get(&src.licence_id);
            let licence = lic
                .and_then(|l| l.spdx.clone())
                .unwrap_or_else(|| src.licence_id.clone());
            let status = lic.map_or_else(|| "permission_unclear".to_owned(), |l| l.status.clone());
            if status == "permission_unclear" {
                b.notices.insert(Notice {
                    code: "LICENCE_UNCLEAR",
                    text: format!("{} has not stated a licence for this data; reuse beyond viewing may need its permission.", src.publisher),
                });
            }
            attribution.insert(Attribution {
                source_id: src.id.clone(),
                publisher: src.publisher.clone(),
                text: format!("{} — {}", src.publisher, src.title),
                licence,
                licence_url: lic.and_then(|l| l.url.clone()),
                licence_status: status,
                redistribution: lic.map_or_else(|| "link".to_owned(), |l| l.redistribution.clone()),
            });
        }
    }
    let mut attribution: Vec<Attribution> = attribution.into_iter().collect();
    for (host, text) in std::mem::take(&mut b.external) {
        attribution.push(Attribution {
            source_id: format!("external:{host}"),
            publisher: host,
            text,
            licence: "unknown".to_owned(),
            licence_url: None,
            licence_status: "permission_unclear".to_owned(),
            redistribution: "link".to_owned(),
        });
    }
    let data_version = match &ctx.release {
        Some((r, _)) => Some(r.clone()),
        None => cat.data_version.clone(),
    };
    Resolved {
        data_version,
        sources,
        analyses,
        attribution,
        notices: b.notices.into_iter().collect(),
        problems: b.problems,
    }
}

/// Freezes the pin to the current release (10 §2.3). Class breaks and
/// analysis fingerprints are written by WP-29 (`lanka-stats`); until then a
/// frozen document keeps the breaks it carries. Returns `false` when there
/// is no release to pin to.
pub fn freeze(doc: &mut MapDocument, cat: &Catalog) -> bool {
    match &cat.data_version {
        Some(dv) => {
            doc.pin = Pin::Release {
                release: dv.clone(),
            };
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_values_are_encoded() {
        assert_eq!(enc("sex=total;age=0-14"), "sex%3Dtotal%3Bage%3D0-14");
        assert_eq!(enc("a.b,c_d"), "a.b,c_d");
        assert_eq!(enc("Y65+"), "Y65%2B");
    }

    #[test]
    fn least_official_wins() {
        let c = |v: &[&str]| least_official(&v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>());
        assert_eq!(c(&["official", "estimated"]).as_deref(), Some("estimated"));
        assert_eq!(
            c(&["official", "third_party"]).as_deref(),
            Some("third_party")
        );
        assert_eq!(c(&[]), None);
    }
}
