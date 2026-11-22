//! `osm_pbf` driver (03 §2.2): one Geofabrik extract, many feature datasets.
//!
//! ```yaml
//! source: osm-geofabrik
//! driver: osm_pbf
//! version: 1.0.0
//! discover:
//!   url: https://download.geofabrik.de/asia/sri-lanka/sri-lanka-latest.osm.pbf
//!   file: sri-lanka-latest.osm.pbf        # name under --from (else the only *.osm.pbf there)
//! extract:
//!   relation_types: [multipolygon, boundary, route]
//! standardise:
//!   outputs: [ {dataset, rules: [...]} ]  # see feature_rules.rs
//! ```
//!
//! **discover** asks for the `-latest` URL without following its redirect:
//! Geofabrik answers with the dated daily file (`…-261120.osm.pbf`), whose
//! name is both the edition and the fingerprint — an unchanged day costs one
//! HEAD and nothing is downloaded. Without a redirect the fingerprint is the
//! ETag/Last-Modified/length of the file.
//!
//! **extract** reads the PBF in three parallel passes (relations, ways,
//! nodes) and writes one `features` extract: every element that matches a
//! rule of any output, with all its tags and its geometry at OSM precision
//! (7 decimals) — nodes as points, ways as line strings (closed or not),
//! multipolygon/boundary relations assembled into (multi)polygons, route
//! relations as multi-line strings. A header row (`osm/header`) carries the
//! replication timestamp (the edition).
//!
//! **standardise** applies each output's rules (first match wins) to the
//! extract and emits features with 6-decimal geometry, names
//! (`name:en`/`name`, `name:si`, `name:ta`), typed attributes and flags.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use async_trait::async_trait;
use osmpbf::{BlobDecode, BlobReader, Element, RelMemberType};
use rayon::iter::{ParallelBridge, ParallelIterator};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::connector::{
    Candidate, CellProv, Connector, DiscoverCtx, Discovery, ExtractCtx, FeatureIn, Fetched,
    OutputHeader, OutputKind, OutputSink, StdCtx,
};
use crate::drivers::feature_rules::{Compiled, OutputSpec, Tags, attrs_of, first_tag};
use crate::extract::{ExtractOut, ExtractRef, Feature};
use crate::geom::{Coord, Geometry, WayLine, assemble_rings, polygons_from_rings};
use crate::registry::{ConnectorSpec, FieldSpec, Loaded};

pub const HEADER_KEY: &str = "osm/header";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub source: String,
    pub driver: String,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    pub discover: DiscoverSpec,
    #[serde(default)]
    pub extract: ExtractSpec,
    pub standardise: StandardiseSpec,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoverSpec {
    pub url: String,
    #[serde(default)]
    pub file: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtractSpec {
    #[serde(default = "default_relation_types")]
    pub relation_types: Vec<String>,
}

impl Default for ExtractSpec {
    fn default() -> Self {
        Self {
            relation_types: default_relation_types(),
        }
    }
}

fn default_relation_types() -> Vec<String> {
    ["multipolygon", "boundary", "route"]
        .map(str::to_owned)
        .to_vec()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StandardiseSpec {
    pub outputs: Vec<OutputSpec>,
}

/// One output with compiled rules.
#[derive(Debug, Clone)]
pub struct Output {
    pub spec: OutputSpec,
    pub rules: Vec<Compiled>,
}

pub struct OsmPbf {
    spec: Spec,
    outputs: Vec<Output>,
    sha256: [u8; 32],
}

impl OsmPbf {
    pub fn from_spec(l: &Loaded<ConnectorSpec>) -> Result<Self> {
        let spec: Spec = serde_json::from_value(l.json.clone())
            .with_context(|| format!("{}: invalid osm_pbf spec", l.path.display()))?;
        Self::new(spec, l.sha256)
    }

    pub fn new(spec: Spec, sha256: [u8; 32]) -> Result<Self> {
        let mut outputs = Vec::new();
        let mut seen = HashSet::new();
        for o in &spec.standardise.outputs {
            ensure!(
                seen.insert(o.dataset.clone()),
                "dataset {:?} listed twice",
                o.dataset
            );
            ensure!(!o.rules.is_empty(), "{}: no rules", o.dataset);
            let rules = o
                .rules
                .iter()
                .cloned()
                .map(|r| r.compile())
                .collect::<Result<Vec<_>>>()
                .with_context(|| format!("rules of {}", o.dataset))?;
            outputs.push(Output {
                spec: o.clone(),
                rules,
            });
        }
        Ok(Self {
            spec,
            outputs,
            sha256,
        })
    }

    #[must_use]
    pub fn outputs(&self) -> &[Output] {
        &self.outputs
    }

    fn rules(&self) -> impl Iterator<Item = &Compiled> {
        self.outputs.iter().flat_map(|o| o.rules.iter())
    }

    /// Local input under `--from`: the configured file name, else the only
    /// (or, if several, the lexically last) `*.osm.pbf`.
    fn local_file(&self, dir: &Path) -> Result<PathBuf> {
        if let Some(f) = &self.spec.discover.file {
            let p = dir.join(f);
            if p.is_file() {
                return Ok(p);
            }
        }
        let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
            .with_context(|| format!("reading {}", dir.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.to_string_lossy().ends_with(".osm.pbf"))
            .collect();
        found.sort();
        found
            .pop()
            .with_context(|| format!("no *.osm.pbf in {}", dir.display()))
    }
}

/// `…-YYMMDD.osm.pbf` → `20YY-MM-DD`.
#[must_use]
pub fn edition_of(url: &str) -> Option<String> {
    let name = url.rsplit('/').next()?;
    let stem = name.strip_suffix(".osm.pbf")?;
    let d = stem.rsplit('-').next()?;
    (d.len() == 6 && d.bytes().all(|b| b.is_ascii_digit()))
        .then(|| format!("20{}-{}-{}", &d[0..2], &d[2..4], &d[4..6]))
}

// ───────────────────────────── extract ─────────────────────────────────────

/// Decimicro-degree coordinates of the nodes ways need.
struct NodeIndex {
    ids: Vec<i64>,
    pos: Vec<[i32; 2]>,
}

impl NodeIndex {
    fn get(&self, id: i64) -> Option<Coord> {
        let i = self.ids.binary_search(&id).ok()?;
        let [lat, lon] = self.pos[i];
        Some([f64::from(lon) * 1e-7, f64::from(lat) * 1e-7])
    }
}

type TagVec = Vec<(String, String)>;

struct WayIn {
    refs: Vec<i64>,
    /// Present when the way matched a rule itself.
    tags: Option<TagVec>,
}

struct RelIn {
    id: i64,
    tags: TagVec,
    rtype: String,
    /// (way id, role)
    members: Vec<(i64, String)>,
}

struct TagSlice<'a>(&'a [(String, String)]);

impl Tags for TagSlice<'_> {
    fn tag(&self, k: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(kk, _)| kk == k)
            .map(|(_, v)| v.as_str())
    }
}

/// Rules and the keys they read (cheap prefilter before matching).
struct Matcher<'a> {
    rules: Vec<&'a Compiled>,
    keys: HashSet<&'a str>,
}

impl<'a> Matcher<'a> {
    fn new(rules: impl Iterator<Item = &'a Compiled>) -> Self {
        let rules: Vec<&Compiled> = rules.collect();
        let keys = rules.iter().flat_map(|r| r.match_keys()).collect();
        Self { rules, keys }
    }

    fn wants<'t>(
        &self,
        etype: &str,
        tags: impl Iterator<Item = (&'t str, &'t str)>,
    ) -> Option<TagVec> {
        let mut any = false;
        let mut v: TagVec = Vec::new();
        for (k, val) in tags {
            any |= self.keys.contains(k);
            v.push((k.to_owned(), val.to_owned()));
        }
        if !any {
            return None;
        }
        let t = TagSlice(&v);
        self.rules
            .iter()
            .any(|r| r.matches_tags(etype, &t))
            .then_some(v)
    }

    fn has_key<'t>(&self, mut tags: impl Iterator<Item = (&'t str, &'t str)>) -> bool {
        tags.any(|(k, _)| self.keys.contains(k))
    }
}

/// Extract statistics (written to the run log by the caller).
#[derive(Debug, Default, Clone)]
pub struct ExtractStats {
    pub nodes: usize,
    pub ways: usize,
    pub relations: usize,
    pub broken_relations: usize,
    pub missing_nodes: usize,
}

fn thread_pool() -> Result<rayon::ThreadPool> {
    let n = std::env::var("LANKA_EXTRACT_THREADS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map_or(2, std::num::NonZeroUsize::get)
                .min(4)
        });
    Ok(rayon::ThreadPoolBuilder::new().num_threads(n).build()?)
}

/// Runs `f` over every data block of the file in parallel and reduces.
fn par_blocks<T, F, R>(path: &Path, f: F, reduce: R) -> Result<T>
where
    T: Default + Send,
    F: Fn(&osmpbf::PrimitiveBlock, &mut T) + Sync + Send,
    R: Fn(T, T) -> T + Sync + Send,
{
    BlobReader::from_path(path)?
        .par_bridge()
        .map(|blob| -> Result<T> {
            let mut acc = T::default();
            if let BlobDecode::OsmData(block) = blob?.decode()? {
                f(&block, &mut acc);
            }
            Ok(acc)
        })
        .try_reduce(T::default, |a, b| Ok(reduce(a, b)))
}

/// Replication timestamp of the file header (seconds since the epoch).
fn replication_timestamp(path: &Path) -> Result<Option<i64>> {
    for blob in BlobReader::from_path(path)? {
        if let BlobDecode::OsmHeader(h) = blob?.decode()? {
            return Ok(h.osmosis_replication_timestamp());
        }
    }
    Ok(None)
}

/// Reads one PBF into feature rows (sorted node → way → relation, by id).
pub fn extract_pbf(
    path: &Path,
    rules: impl Iterator<Item = impl std::ops::Deref<Target = Compiled>>,
    relation_types: &[String],
    artifact_idx: u32,
) -> Result<(Vec<Feature>, ExtractStats)> {
    let owned: Vec<Compiled> = rules.map(|r| r.clone()).collect();
    let m = Matcher::new(owned.iter());
    let rel_types: HashSet<&str> = relation_types.iter().map(String::as_str).collect();
    let pool = thread_pool()?;
    pool.install(|| extract_inner(path, &m, &rel_types, artifact_idx))
}

#[allow(clippy::too_many_lines)]
fn extract_inner(
    path: &Path,
    m: &Matcher<'_>,
    rel_types: &HashSet<&str>,
    artifact_idx: u32,
) -> Result<(Vec<Feature>, ExtractStats)> {
    // Pass 1: relations of the configured types that match a rule.
    let rels: Vec<RelIn> = par_blocks(
        path,
        |block, acc: &mut Vec<RelIn>| {
            for el in block.elements() {
                if let Element::Relation(r) = el {
                    if !m.has_key(r.tags()) {
                        continue;
                    }
                    let rtype = r
                        .tags()
                        .find(|(k, _)| *k == "type")
                        .map(|(_, v)| v.to_owned())
                        .unwrap_or_default();
                    if !rel_types.contains(rtype.as_str()) {
                        continue;
                    }
                    let Some(tags) = m.wants("relation", r.tags()) else {
                        continue;
                    };
                    let members = r
                        .members()
                        .filter(|mm| mm.member_type == RelMemberType::Way)
                        .map(|mm| (mm.member_id, mm.role().unwrap_or_default().to_owned()))
                        .collect();
                    acc.push(RelIn {
                        id: r.id(),
                        tags,
                        rtype,
                        members,
                    });
                }
            }
        },
        |mut a, mut b| {
            a.append(&mut b);
            a
        },
    )?;
    let member_ways: HashSet<i64> = rels
        .iter()
        .flat_map(|r| r.members.iter().map(|(id, _)| *id))
        .collect();

    // Pass 2: ways that match a rule or are relation members.
    let ways: Vec<(i64, WayIn)> = par_blocks(
        path,
        |block, acc: &mut Vec<(i64, WayIn)>| {
            for el in block.elements() {
                if let Element::Way(w) = el {
                    let tags = if m.has_key(w.tags()) {
                        m.wants("way", w.tags())
                    } else {
                        None
                    };
                    if tags.is_none() && !member_ways.contains(&w.id()) {
                        continue;
                    }
                    acc.push((
                        w.id(),
                        WayIn {
                            refs: w.refs().collect(),
                            tags,
                        },
                    ));
                }
            }
        },
        |mut a, mut b| {
            a.append(&mut b);
            a
        },
    )?;
    let mut needed: Vec<i64> = ways
        .iter()
        .flat_map(|(_, w)| w.refs.iter().copied())
        .collect();
    needed.sort_unstable();
    needed.dedup();

    // Pass 3: matching nodes, and the coordinates of every needed node.
    #[derive(Default)]
    struct NodePass {
        hits: Vec<(i64, [i32; 2])>,
        points: Vec<(i64, TagVec, Coord)>,
    }
    let pass: NodePass = par_blocks(
        path,
        |block, acc: &mut NodePass| {
            let mut visit = |id: i64, lat: i32, lon: i32, wanted: Option<TagVec>, c: Coord| {
                if needed.binary_search(&id).is_ok() {
                    acc.hits.push((id, [lat, lon]));
                }
                if let Some(t) = wanted {
                    acc.points.push((id, t, c));
                }
            };
            for el in block.elements() {
                match el {
                    Element::Node(n) => {
                        let w = if m.has_key(n.tags()) {
                            m.wants("node", n.tags())
                        } else {
                            None
                        };
                        visit(
                            n.id(),
                            n.decimicro_lat(),
                            n.decimicro_lon(),
                            w,
                            [n.lon(), n.lat()],
                        );
                    }
                    Element::DenseNode(n) => {
                        let w = if m.has_key(n.tags()) {
                            m.wants("node", n.tags())
                        } else {
                            None
                        };
                        visit(
                            n.id(),
                            n.decimicro_lat(),
                            n.decimicro_lon(),
                            w,
                            [n.lon(), n.lat()],
                        );
                    }
                    _ => {}
                }
            }
        },
        |mut a, mut b| {
            a.hits.append(&mut b.hits);
            a.points.append(&mut b.points);
            a
        },
    )?;
    let mut hits = pass.hits;
    hits.sort_unstable_by_key(|h| h.0);
    let idx = NodeIndex {
        ids: hits.iter().map(|h| h.0).collect(),
        pos: hits.iter().map(|h| h.1).collect(),
    };
    drop(hits);

    let mut stats = ExtractStats::default();
    let mut out: Vec<Feature> = Vec::new();
    let mut points = pass.points;
    points.sort_unstable_by_key(|p| p.0);
    for (id, tags, c) in points {
        stats.nodes += 1;
        out.push(Feature {
            source_key: format!("node/{id}"),
            kind: "node".to_owned(),
            tags: tags.into_iter().collect(),
            geojson: Geometry::Point(c).to_geojson7(),
            artifact_idx,
        });
    }
    // Way coordinates (missing nodes are skipped; < 2 left = no geometry).
    let mut way_coords: HashMap<i64, (Vec<i64>, Vec<Coord>)> = HashMap::with_capacity(ways.len());
    let mut sorted_ways = ways;
    sorted_ways.sort_unstable_by_key(|w| w.0);
    for (id, w) in sorted_ways {
        let mut ids = Vec::with_capacity(w.refs.len());
        let mut cs = Vec::with_capacity(w.refs.len());
        for r in &w.refs {
            if let Some(c) = idx.get(*r) {
                ids.push(*r);
                cs.push(c);
            } else {
                stats.missing_nodes += 1;
            }
        }
        if let Some(tags) = w.tags
            && cs.len() >= 2
        {
            stats.ways += 1;
            out.push(Feature {
                source_key: format!("way/{id}"),
                kind: "way".to_owned(),
                tags: tags.into_iter().collect(),
                geojson: Geometry::LineString(cs.clone()).to_geojson7(),
                artifact_idx,
            });
        }
        if member_ways.contains(&id) {
            way_coords.insert(id, (ids, cs));
        }
    }
    let mut rels = rels;
    rels.sort_unstable_by_key(|r| r.id);
    for r in rels {
        let geom = if r.rtype == "route" {
            let ls: Vec<Vec<Coord>> = r
                .members
                .iter()
                .filter_map(|(id, _)| way_coords.get(id))
                .filter(|(_, cs)| cs.len() >= 2)
                .map(|(_, cs)| cs.clone())
                .collect();
            (!ls.is_empty()).then_some(Geometry::MultiLineString(ls))
        } else {
            let lines = |inner: bool| -> Vec<WayLine<'_>> {
                r.members
                    .iter()
                    .filter(|(_, role)| (role == "inner") == inner)
                    .filter_map(|(id, _)| way_coords.get(id))
                    .map(|(ids, cs)| WayLine {
                        nodes: ids,
                        coords: cs,
                    })
                    .collect()
            };
            match (assemble_rings(&lines(false)), assemble_rings(&lines(true))) {
                (Ok(outer), Ok(inner)) if !outer.is_empty() => {
                    Some(polygons_from_rings(outer, inner))
                }
                _ => None,
            }
        };
        let Some(g) = geom else {
            stats.broken_relations += 1;
            continue;
        };
        stats.relations += 1;
        out.push(Feature {
            source_key: format!("relation/{}", r.id),
            kind: "relation".to_owned(),
            tags: r.tags.into_iter().collect(),
            geojson: g.to_geojson7(),
            artifact_idx,
        });
    }
    Ok((out, stats))
}

// ───────────────────────────── standardise ─────────────────────────────────

/// Applies every output's rules to extracted features; returns the features
/// per output (in `outputs` order) and the count of attribute values that
/// did not fit their declared type.
pub fn standardise_features<'a>(
    outputs: &[Output],
    fields_of: impl Fn(&str) -> Vec<FieldSpec>,
    features: impl Iterator<Item = (usize, u32, &'a Feature)>,
) -> Result<(Vec<Vec<FeatureIn>>, usize)> {
    let fields: Vec<Vec<FieldSpec>> = outputs.iter().map(|o| fields_of(&o.spec.dataset)).collect();
    let field_maps: Vec<BTreeMap<&str, &FieldSpec>> = fields
        .iter()
        .map(|fs| fs.iter().map(|f| (f.name.as_str(), f)).collect())
        .collect();
    let mut per: Vec<Vec<FeatureIn>> = outputs.iter().map(|_| Vec::new()).collect();
    let mut dropped = 0usize;
    for (extract, row, f) in features {
        if f.source_key == HEADER_KEY {
            continue;
        }
        let tags = &f.tags;
        let mut geom: Option<Geometry> = None;
        for (oi, o) in outputs.iter().enumerate() {
            let name_en = first_tag(tags, &o.spec.name_en, &["name:en", "name"]);
            let Some(rule) = o
                .rules
                .iter()
                .find(|r| r.matches_tags(&f.kind, tags) && r.matches_name(name_en.as_deref()))
            else {
                continue;
            };
            if geom.is_none() {
                geom = Some(
                    Geometry::parse(&f.geojson)
                        .with_context(|| format!("{}: extract geometry", f.source_key))?,
                );
            }
            let Some(g) = geom.as_ref().and_then(|g| rule.shape(g, name_en.is_some())) else {
                continue;
            };
            let Some(rep) = g.rep_point() else {
                continue;
            };
            let kind = rule.kind(tags);
            if kind.is_empty() {
                continue;
            }
            per[oi].push(FeatureIn {
                key: f.source_key.clone(),
                kind,
                name_si: first_tag(tags, &o.spec.name_si, &["name:si"]),
                name_ta: first_tag(tags, &o.spec.name_ta, &["name:ta"]),
                name_en,
                geometry: g.to_geojson6(),
                rep: [crate::geom::round6(rep[0]), crate::geom::round6(rep[1])],
                attrs: attrs_of(rule, tags, &field_maps[oi], &mut dropped)
                    .with_context(|| format!("dataset {}", o.spec.dataset))?,
                flags: rule.flags,
                entity_id: None,
                artifact: f.artifact_idx as usize,
                prov: Some(CellProv { extract, row }),
            });
        }
    }
    Ok((per, dropped))
}

// async_trait marks the boxed futures #[must_use]; clippy then flags that.
#[allow(clippy::double_must_use)]
#[async_trait]
impl Connector for OsmPbf {
    fn source_id(&self) -> &str {
        &self.spec.source
    }

    fn version(&self) -> &str {
        &self.spec.version
    }

    fn extractor(&self) -> &str {
        "osm_pbf"
    }

    fn spec_sha256(&self) -> Option<[u8; 32]> {
        Some(self.sha256)
    }

    async fn discover(&self, ctx: &DiscoverCtx<'_>) -> Result<Discovery> {
        let url = &self.spec.discover.url;
        if let Some(dir) = &ctx.opts.from_dir {
            let p = self.local_file(dir)?;
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned());
            return Ok(Discovery {
                candidates: vec![Candidate {
                    key: "pbf".to_owned(),
                    url: url.clone(),
                    edition: name.as_deref().and_then(edition_of),
                    local_path: Some(p),
                    media_type: Some("application/x-protobuf".to_owned()),
                    ..Candidate::default()
                }],
                fingerprint: None,
            });
        }
        // The -latest URL redirects to the dated daily file: one HEAD tells
        // whether there is anything new.
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!(
                "lanka-data-layer-ingest/",
                env!("CARGO_PKG_VERSION")
            ))
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        let resp = http
            .head(url)
            .send()
            .await
            .with_context(|| format!("HEAD {url}"))?;
        let header = |n: reqwest::header::HeaderName| {
            resp.headers()
                .get(n)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        let (target, fp_text) = if resp.status().is_redirection() {
            let loc = header(reqwest::header::LOCATION).context("redirect without Location")?;
            let abs = reqwest::Url::parse(url)?.join(&loc)?.to_string();
            (abs.clone(), abs)
        } else if resp.status().is_success() {
            let fp = format!(
                "{}\u{1f}{}\u{1f}{}",
                header(reqwest::header::ETAG).unwrap_or_default(),
                header(reqwest::header::LAST_MODIFIED).unwrap_or_default(),
                header(reqwest::header::CONTENT_LENGTH).unwrap_or_default()
            );
            (url.clone(), fp)
        } else {
            bail!("HEAD {url}: HTTP {}", resp.status());
        };
        Ok(Discovery {
            candidates: vec![Candidate {
                key: "pbf".to_owned(),
                edition: edition_of(&target),
                url: target,
                media_type: Some("application/x-protobuf".to_owned()),
                ..Candidate::default()
            }],
            fingerprint: Some(Sha256::digest(fp_text.as_bytes()).into()),
        })
    }

    async fn extract(&self, _ctx: &ExtractCtx<'_>, fetched: &[Fetched]) -> Result<Vec<ExtractOut>> {
        ensure!(fetched.len() == 1, "osm_pbf reads exactly one file");
        let f = fetched[0].clone();
        let rules: Vec<Compiled> = self.rules().cloned().collect();
        let rel_types = self.spec.extract.relation_types.clone();
        let (mut rows, stats) = tokio::task::spawn_blocking(move || -> Result<_> {
            let ts = replication_timestamp(&f.path)?;
            let (mut rows, stats) = extract_pbf(&f.path, rules.iter(), &rel_types, 0)?;
            let mut tags = BTreeMap::new();
            if let Some(ts) = ts {
                tags.insert("replication_timestamp".to_owned(), ts.to_string());
            }
            tags.insert("nodes".to_owned(), stats.nodes.to_string());
            tags.insert("ways".to_owned(), stats.ways.to_string());
            tags.insert("relations".to_owned(), stats.relations.to_string());
            tags.insert(
                "broken_relations".to_owned(),
                stats.broken_relations.to_string(),
            );
            tags.insert("missing_nodes".to_owned(), stats.missing_nodes.to_string());
            rows.insert(
                0,
                Feature {
                    source_key: HEADER_KEY.to_owned(),
                    kind: "header".to_owned(),
                    tags,
                    geojson: String::new(),
                    artifact_idx: 0,
                },
            );
            Ok((rows, stats))
        })
        .await??;
        rows.shrink_to_fit();
        tracing::info!(
            nodes = stats.nodes,
            ways = stats.ways,
            relations = stats.relations,
            broken_relations = stats.broken_relations,
            missing_nodes = stats.missing_nodes,
            "osm_pbf extract"
        );
        Ok(vec![ExtractOut::Features(rows)])
    }

    fn standardise(
        &self,
        ctx: &StdCtx<'_>,
        extracts: &[ExtractRef],
        sink: &mut dyn OutputSink,
    ) -> Result<()> {
        let mut vintage: Option<String> = None;
        for e in extracts {
            if let Some(h) = e
                .data
                .features()
                .first()
                .filter(|h| h.source_key == HEADER_KEY)
            {
                vintage = h
                    .tags
                    .get("replication_timestamp")
                    .and_then(|t| t.parse::<i64>().ok())
                    .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
                    .map(|d| d.format("%Y-%m-%d").to_string());
                if let Some(b) = h.tags.get("broken_relations").filter(|b| *b != "0") {
                    ctx.warn(format!(
                        "{b} relation(s) could not be assembled (open rings)"
                    ));
                }
            }
        }
        let rows = extracts.iter().enumerate().flat_map(|(ei, e)| {
            e.data
                .features()
                .iter()
                .enumerate()
                .map(move |(ri, f)| (ei, u32::try_from(ri).unwrap_or(u32::MAX), f))
        });
        let (per, dropped) = standardise_features(
            &self.outputs,
            |ds| {
                ctx.registry
                    .dataset(ds)
                    .map(|d| d.fields.clone())
                    .unwrap_or_default()
            },
            rows,
        )?;
        if dropped > 0 {
            ctx.warn(format!(
                "{dropped} attribute value(s) did not fit their declared field type and were left out"
            ));
        }
        for (o, feats) in self.outputs.iter().zip(per) {
            ensure!(
                ctx.registry.dataset(&o.spec.dataset).is_some(),
                "output dataset {:?} is not in the registry",
                o.spec.dataset
            );
            sink.begin(OutputHeader {
                dataset_id: o.spec.dataset.clone(),
                kind: OutputKind::Features,
                vintage: vintage.clone(),
            })?;
            for f in feats {
                sink.feature(f)?;
            }
            sink.end()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edition_from_dated_name() {
        assert_eq!(
            edition_of("https://download.geofabrik.de/asia/sri-lanka/sri-lanka-261120.osm.pbf"),
            Some("2026-11-20".into())
        );
        assert_eq!(edition_of("https://x/sri-lanka-latest.osm.pbf"), None);
    }

    #[test]
    fn standardises_by_first_matching_rule() {
        let spec: Spec = serde_yaml_ng::from_str(
            r#"
source: osm-x
driver: osm_pbf
version: 1.0.0
discover: { url: "https://example.org/x-latest.osm.pbf" }
standardise:
  outputs:
    - dataset: roads
      rules:
        - { match: { highway: [construction], construction: [motorway] }, geometry: line, kind: motorway,
            attrs: { status: "=under_construction" } }
        - { match: { highway: [motorway] }, geometry: line, kind: motorway, attrs: { status: "=open", ref: ref } }
    - dataset: pois
      rules:
        - { match: { amenity: [hospital] }, geometry: point, kind: "{amenity}", attrs: { beds: beds } }
"#,
        )
        .expect("spec");
        let c = OsmPbf::new(spec, [0; 32]).expect("compile");
        let feat = |key: &str, kind: &str, tags: &[(&str, &str)], g: &str| Feature {
            source_key: key.into(),
            kind: kind.into(),
            tags: tags
                .iter()
                .map(|(k, v)| ((*k).into(), (*v).into()))
                .collect(),
            geojson: g.into(),
            artifact_idx: 0,
        };
        let rows = [
            feat(HEADER_KEY, "header", &[], ""),
            feat(
                "way/1",
                "way",
                &[("highway", "motorway"), ("ref", "E01")],
                r#"{"type":"LineString","coordinates":[[79.9,6.9],[80.0,7.0]]}"#,
            ),
            feat(
                "way/2",
                "way",
                &[("highway", "construction"), ("construction", "motorway")],
                r#"{"type":"LineString","coordinates":[[80.1,7.1],[80.2,7.2]]}"#,
            ),
            feat(
                "way/3",
                "way",
                &[
                    ("amenity", "hospital"),
                    ("beds", "12;30"),
                    ("name", "Base Hospital"),
                ],
                r#"{"type":"LineString","coordinates":[[80,7],[80.01,7],[80.01,7.01],[80,7.01],[80,7]]}"#,
            ),
        ];
        let fields = |ds: &str| -> Vec<FieldSpec> {
            let f = |name: &str, ty| FieldSpec {
                name: name.into(),
                ty,
                label: None,
                unit: None,
                measure_type: None,
                values: Vec::new(),
            };
            match ds {
                "roads" => vec![
                    f("status", crate::registry::FieldType::String),
                    f("ref", crate::registry::FieldType::String),
                ],
                _ => vec![f("beds", crate::registry::FieldType::Integer)],
            }
        };
        let (per, dropped) = standardise_features(
            c.outputs(),
            fields,
            rows.iter().enumerate().map(|(i, f)| (0, i as u32, f)),
        )
        .expect("standardise");
        assert_eq!(per[0].len(), 2);
        assert_eq!(per[0][0].attrs["status"], "open");
        assert_eq!(per[0][0].attrs["ref"], "E01");
        assert_eq!(per[0][1].attrs["status"], "under_construction");
        assert_eq!(per[1].len(), 1);
        let h = &per[1][0];
        assert_eq!(h.kind, "hospital");
        assert!(
            h.geometry.starts_with(r#"{"type":"Point""#),
            "closed way → interior point"
        );
        assert_eq!(h.name_en.as_deref(), Some("Base Hospital"));
        assert!(h.attrs.is_empty());
        assert_eq!(dropped, 1, "beds=12;30 is not an integer");
        assert_eq!(h.prov, Some(CellProv { extract: 0, row: 3 }));
    }

    /// discover: one HEAD of `-latest`, the redirect target is the candidate
    /// and the fingerprint; nothing is downloaded.
    #[tokio::test]
    async fn discover_follows_nothing_but_reads_the_redirect() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let server = tokio::spawn(async move {
            let mut methods = Vec::new();
            for day in ["261120", "261121"] {
                let (mut s, _) = listener.accept().await.expect("accept");
                let mut buf = vec![0u8; 4096];
                let n = s.read(&mut buf).await.expect("read");
                methods.push(
                    String::from_utf8_lossy(&buf[..n])
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .to_owned(),
                );
                let resp = format!(
                    "HTTP/1.1 307 Temporary Redirect\r\nLocation: /asia/sri-lanka/sri-lanka-{day}.osm.pbf\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                s.write_all(resp.as_bytes()).await.expect("write");
            }
            methods
        });
        let spec: Spec = serde_yaml_ng::from_str(&format!(
            "source: osm-x\ndriver: osm_pbf\nversion: 1.0.0\n\
             discover: {{ url: 'http://127.0.0.1:{port}/asia/sri-lanka/sri-lanka-latest.osm.pbf' }}\n\
             standardise: {{ outputs: [ {{ dataset: d, rules: [ {{ match: {{ amenity: x }}, geometry: point, kind: x }} ] }} ] }}\n"
        ))
        .expect("spec");
        let c = OsmPbf::new(spec, [0; 32]).expect("connector");
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../registry");
        let profile = lanka_core::Profile::discover(&root)
            .and_then(lanka_core::Profile::load)
            .expect("profile");
        let reg = crate::Registry::default();
        let http = reqwest::Client::new();
        let opts = crate::RunOptions::default();
        let ctx = DiscoverCtx {
            registry: &reg,
            profile: &profile,
            http: &http,
            opts: &opts,
            run: None,
        };
        let a = c.discover(&ctx).await.expect("discover");
        let b = c.discover(&ctx).await.expect("discover");
        assert_eq!(
            a.candidates[0].url,
            format!("http://127.0.0.1:{port}/asia/sri-lanka/sri-lanka-261120.osm.pbf")
        );
        assert_eq!(a.candidates[0].edition.as_deref(), Some("2026-11-20"));
        assert_ne!(
            a.fingerprint, b.fingerprint,
            "a new daily file changes the fingerprint"
        );
        let methods = server.await.expect("server");
        assert!(
            methods.iter().all(|m| m.starts_with("HEAD ")),
            "{methods:?}"
        );
    }

    /// Reads the real extract when `LANKA_TEST_OSM_PBF` points at one.
    #[test]
    fn real_extract_smoke() {
        let Ok(p) = std::env::var("LANKA_TEST_OSM_PBF") else {
            return;
        };
        let spec_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../registry/connectors/osm-geofabrik.yaml");
        let spec: Spec =
            serde_yaml_ng::from_str(&std::fs::read_to_string(spec_path).expect("spec file"))
                .expect("spec");
        let c = OsmPbf::new(spec, [0; 32]).expect("compile");
        let t = std::time::Instant::now();
        let (rows, stats) =
            extract_pbf(Path::new(&p), c.rules(), &c.spec.extract.relation_types, 0)
                .expect("extract");
        eprintln!("{} rows in {:?}: {stats:?}", rows.len(), t.elapsed());
        assert!(rows.len() > 10_000);
        let reg =
            crate::Registry::load(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../registry"))
                .expect("registry");
        let t = std::time::Instant::now();
        let (per, dropped) = standardise_features(
            c.outputs(),
            |ds| {
                reg.dataset(ds)
                    .map(|d| d.fields.clone())
                    .unwrap_or_default()
            },
            rows.iter().enumerate().map(|(i, f)| (0, i as u32, f)),
        )
        .expect("standardise");
        eprintln!(
            "standardised in {:?}; {dropped} attribute values dropped",
            t.elapsed()
        );
        for (o, feats) in c.outputs().iter().zip(&per) {
            let mut by: BTreeMap<(String, &str), usize> = BTreeMap::new();
            let mut bytes = 0usize;
            for f in feats {
                let ty = f.key.split('/').next().unwrap_or_default();
                *by.entry((f.kind.clone(), ty)).or_default() += 1;
                bytes += f.geometry.len();
            }
            eprintln!(
                "{} {} features, {bytes} geometry bytes",
                o.spec.dataset,
                feats.len()
            );
            for ((k, ty), n) in by {
                eprintln!("    {k:<24} {ty:<9} {n}");
            }
        }
    }
}
