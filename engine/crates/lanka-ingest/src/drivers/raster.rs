//! `raster` driver (03 §2.2, WP-05): GeoTIFFs → canonical grid layers →
//! zonal observations, configured by `registry/connectors/<source>.yaml`.
//!
//! ```yaml
//! source: worldpop
//! driver: raster
//! version: 1.0.0
//! dataset: worldpop-pop-grid          # the dataset the zonal observations belong to
//! discover:
//!   files:                            # one artifact each (`--from DIR` reads DIR/<file name>)
//!     - { key: pop-2025, url: https://…/lka_pop_2025_UC_1km_R2024B_UA_v1.tif }
//! layers:                             # grid layers (geo.grid_layers), one array each
//!   - id: worldpop.population
//!     period: "2025"
//!     indicator: pop.grid_total       # optional; dims: "k=v"
//!     resample: split_sum             # split_sum | area_sum | centre_mean | centre_sample | class_count | class_mode | slope_mean | slope_share | centre_max | centre_share
//!     band: 1                         # 1-based (default 1); files: [keys] (default all)
//!     encoding: { dtype: f32 }        # + scale, offset, nodata
//!     unit: "{person}"
//!     zonal: sum                      # geo.grid_layers.zonal
//!     overview: sum                   # mean | sum | max | mode
//!     source_res_m: 1000
//!     serve: true                     # mapped by `lanka serve` (point/radius/tiles)
//!     style: { ramp: viridis.magma, scale: log, domain: [0.1, 300] }
//! zonal:                              # observations computed from the layers
//!   - { indicator: pop.grid_total, layer: worldpop.population, stat: sum, basis: estimated }
//! ```
//!
//! `class_count` takes `class: <value>` (pixels of that class per cell, u8),
//! `class_mode` takes `classes: [...]`, `slope_share` takes `threshold_deg`
//! (share of fine pixels steeper than it), `centre_max` keeps the deepest
//! fine pixel per cell and `centre_share` takes `threshold` (share of the
//! fine pixels in the cell that are valid and above it; nodata counts as
//! not above — flood maps store only the wet area). `class_*`,
//! `centre_mean`, `centre_max` and `centre_share` groups are read chunk by
//! chunk, only the chunks over the grid. Zonal `stat`: `sum`, `mean`
//! (area-weighted), `area_sum` (Σ value · cell area, m², × `factor`) and
//! `share` (this layer's `area_sum` over the sum of every layer listed in
//! `of`, × `factor`). A zonal entry with `layers: {"<dims>": <layer>, …}`
//! emits one series per dimension combination.
//!
//! Decoding is pure Rust (`lanka_grid::geotiff`); each file is read once for
//! all the layers built from it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use anyhow::{Context, Result, bail, ensure};
use async_trait::async_trait;
use lanka_core::grid::Grid;
use lanka_grid::array::{Dtype, LayerEncoding};
use lanka_grid::cog::Overview;
use lanka_grid::geotiff::GeoTiff;
use lanka_grid::resample::{
    CentreClasses, CentreMax, CentreMean, CentreShare, area_sum, centre_sample, split_sum,
};
use lanka_grid::tile::Style;
use lanka_grid::zonal::Stat;
use serde::Deserialize;

use crate::connector::{
    Candidate, Connector, DiscoverCtx, Discovery, ExtractCtx, Fetched, OutputSink, StdCtx,
};
use crate::extract::{ExtractOut, ExtractRef};
use crate::registry::{ConnectorSpec, Loaded};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RasterSpec {
    pub source: String,
    pub driver: String,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    pub dataset: String,
    pub discover: DiscoverSpec,
    pub layers: Vec<LayerSpec>,
    #[serde(default)]
    pub zonal: Vec<ZonalSpec>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoverSpec {
    pub files: Vec<FileSpec>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileSpec {
    pub key: String,
    pub url: String,
    #[serde(default)]
    pub media_type: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resample {
    SplitSum,
    /// Area-weighted redistribution of counts (similar or finer rasters).
    AreaSum,
    CentreMean,
    CentreSample,
    ClassCount,
    ClassMode,
    SlopeMean,
    SlopeShare,
    /// Maximum of the fine pixels whose centre is in the cell.
    CentreMax,
    /// Share of the fine pixels whose centre is in the cell that are valid
    /// and above `threshold` (nodata = not above).
    CentreShare,
}

impl Resample {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SplitSum => "split_sum",
            Self::AreaSum => "area_sum",
            Self::CentreMean => "centre_mean",
            Self::CentreSample => "centre_sample",
            Self::ClassCount => "class_count",
            Self::ClassMode => "class_mode",
            Self::SlopeMean => "slope_mean",
            Self::SlopeShare => "slope_share",
            Self::CentreMax => "centre_max",
            Self::CentreShare => "centre_share",
        }
    }

    /// Accumulated chunk by chunk (no whole-image read).
    #[must_use]
    pub const fn chunkable(self) -> bool {
        matches!(
            self,
            Self::ClassCount
                | Self::ClassMode
                | Self::CentreMax
                | Self::CentreShare
                | Self::CentreMean
        )
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EncodingSpec {
    pub dtype: String,
    #[serde(default)]
    pub scale: Option<f64>,
    #[serde(default)]
    pub offset: Option<f64>,
    #[serde(default)]
    pub nodata: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerSpec {
    pub id: String,
    pub period: String,
    #[serde(default)]
    pub indicator: Option<String>,
    #[serde(default)]
    pub dims: String,
    pub resample: Resample,
    #[serde(default)]
    pub files: Option<Vec<String>>,
    #[serde(default)]
    pub band: Option<u16>,
    #[serde(default)]
    pub class: Option<u8>,
    #[serde(default)]
    pub classes: Option<Vec<u8>>,
    #[serde(default)]
    pub threshold_deg: Option<f64>,
    /// `centre_share`: pixels with a value above it count (default 0).
    #[serde(default)]
    pub threshold: Option<f64>,
    pub encoding: EncodingSpec,
    pub unit: String,
    pub zonal: String,
    pub overview: String,
    pub source_res_m: f64,
    #[serde(default)]
    pub serve: bool,
    #[serde(default)]
    pub style: Option<Style>,
}

impl LayerSpec {
    pub fn encoding(&self) -> Result<LayerEncoding> {
        Ok(LayerEncoding {
            dtype: Dtype::parse(&self.encoding.dtype)?,
            scale: self.encoding.scale.unwrap_or(1.0),
            offset: self.encoding.offset.unwrap_or(0.0),
            nodata: self.encoding.nodata,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZonalStat {
    Sum,
    Mean,
    AreaSum,
    Share,
    Max,
    /// Σ v per km² of the unit's valid cells (counts → density).
    Density,
}

impl ZonalStat {
    /// The accumulator statistic behind it.
    #[must_use]
    pub const fn acc_stat(self) -> Stat {
        match self {
            Self::Sum | Self::Density => Stat::Sum,
            Self::Mean => Stat::Mean,
            Self::AreaSum | Self::Share => Stat::AreaSum,
            Self::Max => Stat::Max,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ZonalSpec {
    pub indicator: String,
    pub stat: ZonalStat,
    /// One layer (with `dims`) …
    #[serde(default)]
    pub layer: Option<String>,
    #[serde(default)]
    pub dims: String,
    /// … or one layer per dimension combination.
    #[serde(default)]
    pub layers: BTreeMap<String, String>,
    /// Denominator layers of `share`.
    #[serde(default)]
    pub of: Vec<String>,
    #[serde(default)]
    pub factor: Option<f64>,
    /// `computed` (default) or `estimated` (modelled inputs, 04 §2).
    #[serde(default)]
    pub basis: Option<String>,
    /// Admin levels to publish (default 1–4).
    #[serde(default)]
    pub levels: Option<Vec<i16>>,
    /// Period of the observations (default: the layer's).
    #[serde(default)]
    pub period: Option<String>,
    /// Cells counted: `all` assigned cells (default for `sum`, v1's
    /// population roll-up) or only those whose centre is `inside` a unit
    /// (default otherwise: near-shore cells assigned by the 2 km fallback
    /// would add sea to areas and means).
    #[serde(default)]
    pub cells: Option<String>,
}

impl ZonalSpec {
    /// Whether only cells with their centre inside a unit count.
    pub fn inside_only(&self) -> anyhow::Result<bool> {
        match self.cells.as_deref() {
            None => Ok(self.stat != ZonalStat::Sum),
            Some("inside") => Ok(true),
            Some("all") => Ok(false),
            Some(c) => anyhow::bail!("zonal {}: cells {c:?} (all | inside)", self.indicator),
        }
    }
}

impl ZonalSpec {
    /// `(dims, layer id)` pairs.
    #[must_use]
    pub fn series(&self) -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = self
            .layers
            .iter()
            .map(|(d, l)| (d.clone(), l.clone()))
            .collect();
        if let Some(l) = &self.layer {
            v.push((self.dims.clone(), l.clone()));
        }
        v
    }
}

/// The driver.
#[derive(Debug, Clone)]
pub struct Raster {
    pub spec: RasterSpec,
    sha256: [u8; 32],
}

impl Raster {
    pub fn from_spec(l: &Loaded<ConnectorSpec>) -> Result<Self> {
        let spec: RasterSpec = serde_json::from_value(l.json.clone())
            .with_context(|| format!("{}: invalid raster spec", l.path.display()))?;
        ensure!(spec.driver == "raster", "not a raster spec");
        let mut ids = BTreeSet::new();
        let keys: BTreeSet<&str> = spec.discover.files.iter().map(|f| f.key.as_str()).collect();
        ensure!(
            keys.len() == spec.discover.files.len(),
            "{}: duplicate file keys",
            l.path.display()
        );
        for lay in &spec.layers {
            ensure!(
                ids.insert((lay.id.as_str(), lay.period.as_str())),
                "{}: duplicate layer {} {}",
                l.path.display(),
                lay.id,
                lay.period
            );
            lay.encoding()?;
            Overview::parse(&lay.overview)?;
            for f in lay.files.iter().flatten() {
                ensure!(
                    keys.contains(f.as_str()),
                    "{}: layer {} names unknown file {f}",
                    l.path.display(),
                    lay.id
                );
            }
            match lay.resample {
                Resample::ClassCount => ensure!(
                    lay.class.is_some(),
                    "layer {}: class_count needs `class`",
                    lay.id
                ),
                Resample::ClassMode => ensure!(
                    lay.classes.is_some(),
                    "layer {}: class_mode needs `classes`",
                    lay.id
                ),
                Resample::SlopeShare => {
                    ensure!(
                        lay.threshold_deg.is_some(),
                        "layer {}: slope_share needs `threshold_deg`",
                        lay.id
                    );
                }
                _ => {}
            }
        }
        let layer_ids: BTreeSet<&str> = spec.layers.iter().map(|l| l.id.as_str()).collect();
        for z in &spec.zonal {
            let s = z.series();
            ensure!(!s.is_empty(), "zonal {}: no layer", z.indicator);
            for (_, lay) in s.iter().chain(
                z.of.iter()
                    .map(|o| ("".to_owned(), o.clone()))
                    .collect::<Vec<_>>()
                    .iter(),
            ) {
                ensure!(
                    layer_ids.contains(lay.as_str()),
                    "zonal {}: unknown layer {lay}",
                    z.indicator
                );
            }
            if z.stat == ZonalStat::Share {
                ensure!(!z.of.is_empty(), "zonal {}: share needs `of`", z.indicator);
            }
        }
        Ok(Self {
            spec,
            sha256: l.sha256,
        })
    }

    /// The archived file for each spec file key.
    pub fn paths(&self, fetched: &[Fetched]) -> Result<BTreeMap<String, PathBuf>> {
        let mut m = BTreeMap::new();
        for f in &self.spec.discover.files {
            let p = fetched
                .iter()
                .find(|x| x.key == f.key)
                .with_context(|| format!("file {} was not fetched", f.key))?;
            m.insert(f.key.clone(), p.path.clone());
        }
        Ok(m)
    }
}

// async_trait marks the boxed futures #[must_use]; clippy then flags that.
#[allow(clippy::double_must_use)]
#[async_trait]
impl Connector for Raster {
    fn source_id(&self) -> &str {
        &self.spec.source
    }
    fn version(&self) -> &str {
        &self.spec.version
    }
    fn extractor(&self) -> &str {
        "raster"
    }
    fn spec_sha256(&self) -> Option<[u8; 32]> {
        Some(self.sha256)
    }
    fn grid(&self) -> Option<&Raster> {
        Some(self)
    }

    async fn discover(&self, ctx: &DiscoverCtx<'_>) -> Result<Discovery> {
        let mut candidates = Vec::new();
        for f in &self.spec.discover.files {
            let local_path = match &ctx.opts.from_dir {
                Some(d) => {
                    let name = f
                        .url
                        .rsplit('/')
                        .next()
                        .filter(|s| !s.is_empty())
                        .with_context(|| format!("url of {} has no file name", f.key))?;
                    let p = d.join(name);
                    ensure!(p.is_file(), "--from: {} not found", p.display());
                    Some(p)
                }
                None => None,
            };
            candidates.push(Candidate {
                key: f.key.clone(),
                url: f.url.clone(),
                local_path,
                media_type: Some(
                    f.media_type
                        .clone()
                        .unwrap_or_else(|| "image/tiff".to_owned()),
                ),
                ..Candidate::default()
            });
        }
        // Static URLs: the upstream state is the artifact set (conditional
        // GET + the artifact-set short-circuit decide whether to rebuild).
        Ok(Discovery {
            candidates,
            fingerprint: None,
        })
    }

    async fn extract(
        &self,
        _ctx: &ExtractCtx<'_>,
        _fetched: &[Fetched],
    ) -> Result<Vec<ExtractOut>> {
        bail!("raster sources are extracted by the grid stage (lanka_ingest::grid)")
    }

    fn standardise(
        &self,
        _ctx: &StdCtx<'_>,
        _extracts: &[ExtractRef],
        _sink: &mut dyn OutputSink,
    ) -> Result<()> {
        bail!(
            "{}: zonal observations are computed from the grid layers; re-run `lanka ingest run {} --force` \
             instead of restandardise",
            self.spec.source,
            self.spec.source
        )
    }
}

// ───────────────────────────── building ────────────────────────────────────

/// A layer's values on the canonical grid (physical, NaN = nodata).
#[derive(Debug)]
pub struct BuiltLayer {
    pub spec: LayerSpec,
    pub values: Vec<f32>,
    /// Source pixels / files that fed it (for the run log).
    pub note: String,
}

fn merge_sum(acc: &mut Option<Vec<f32>>, v: Vec<f32>) {
    match acc {
        None => *acc = Some(v),
        Some(a) => {
            for (x, y) in a.iter_mut().zip(v) {
                if y.is_nan() {
                    continue;
                }
                *x = if x.is_nan() { y } else { *x + y };
            }
        }
    }
}

fn merge_first(acc: &mut Option<Vec<f32>>, v: Vec<f32>) {
    match acc {
        None => *acc = Some(v),
        Some(a) => {
            for (x, y) in a.iter_mut().zip(v) {
                if x.is_nan() {
                    *x = y;
                }
            }
        }
    }
}

/// Does a raster's footprint touch the grid?
fn touches(g: &Grid, t: &GeoTiff) -> bool {
    let [w, s, e, n] = t.georef.bounds();
    e > g.lon_min && w < g.lon_max() && n > g.lat_min() && s < g.lat_max
}

/// Builds every layer of the spec from the archived files (CPU-bound; run
/// it on a blocking thread).
pub fn build_layers(
    spec: &RasterSpec,
    g: &Grid,
    paths: &BTreeMap<String, PathBuf>,
) -> Result<Vec<BuiltLayer>> {
    let all: Vec<String> = spec.discover.files.iter().map(|f| f.key.clone()).collect();
    // Group layers by the files they read.
    let mut groups: BTreeMap<Vec<String>, Vec<usize>> = BTreeMap::new();
    for (i, l) in spec.layers.iter().enumerate() {
        let mut files = l.files.clone().unwrap_or_else(|| all.clone());
        files.sort();
        groups.entry(files).or_default().push(i);
    }
    let mut out: Vec<Option<BuiltLayer>> = (0..spec.layers.len()).map(|_| None).collect();
    for (files, members) in groups {
        let layers: Vec<&LayerSpec> = members.iter().map(|&i| &spec.layers[i]).collect();
        let classes: Vec<u8> = {
            let mut s = BTreeSet::new();
            for l in &layers {
                s.extend(l.class);
                s.extend(l.classes.iter().flatten().copied());
            }
            s.into_iter().collect()
        };
        let needs_classes = !classes.is_empty();
        let only_classes = layers.iter().all(|l| l.resample.chunkable());
        let mut cc = needs_classes.then(|| CentreClasses::new(*g, &classes));
        let mut means: BTreeMap<usize, CentreMean> = BTreeMap::new();
        let mut maxes: BTreeMap<usize, CentreMax> = BTreeMap::new();
        let mut shares: BTreeMap<usize, CentreShare> = BTreeMap::new();
        let mut arrays: BTreeMap<usize, Option<Vec<f32>>> = BTreeMap::new();
        let mut pixels = 0u64;
        let mut used = 0usize;
        for key in &files {
            let path = paths
                .get(key)
                .with_context(|| format!("no archived file for {key}"))?;
            let mut t = GeoTiff::open(path)?;
            if !touches(g, &t) {
                continue;
            }
            used += 1;
            if only_classes {
                // Large categorical tiles: chunk by chunk, only chunks over the grid.
                let geo = t.georef;
                for ci in 0..t.chunk_count()? {
                    let (x, y, w, h) = t.chunk_rect(ci);
                    let west = geo.x0 + f64::from(x) * geo.dx;
                    let east = geo.x0 + f64::from(x + w) * geo.dx;
                    let north = geo.y0 - f64::from(y) * geo.dy;
                    let south = geo.y0 - f64::from(y + h) * geo.dy;
                    if east <= g.lon_min
                        || west >= g.lon_max()
                        || south >= g.lat_max
                        || north <= g.lat_min()
                    {
                        continue;
                    }
                    let block = t.read_chunk(ci)?;
                    pixels += u64::from(w) * u64::from(h);
                    if let Some(c) = cc.as_mut() {
                        c.add_block(&geo, &block);
                    }
                    let is_nd = |v: f32| t.is_nodata(v);
                    for (&li, l) in members.iter().zip(&layers) {
                        let band = l
                            .band
                            .unwrap_or(1)
                            .checked_sub(1)
                            .context("bands are 1-based")?;
                        ensure!(
                            band < block.bands,
                            "layer {}: band {} of {}",
                            l.id,
                            band + 1,
                            block.bands
                        );
                        match l.resample {
                            Resample::CentreMean => means
                                .entry(li)
                                .or_insert_with(|| CentreMean::new(*g))
                                .add_block(&geo, &block, band, is_nd),
                            Resample::CentreMax => maxes
                                .entry(li)
                                .or_insert_with(|| CentreMax::new(*g))
                                .add_block(&geo, &block, band, is_nd),
                            Resample::CentreShare => shares
                                .entry(li)
                                .or_insert_with(|| CentreShare::new(*g, l.threshold.unwrap_or(0.0)))
                                .add_block(&geo, &block, band, is_nd),
                            _ => {}
                        }
                    }
                }
                continue;
            }
            let block = t.read_all()?;
            pixels += u64::from(block.width) * u64::from(block.height);
            let geo = t.georef;
            let nodata = t.nodata;
            let is_nd = |v: f32| t.is_nodata(v);
            if let Some(c) = cc.as_mut() {
                c.add_block(&geo, &block);
            }
            let mut slope: Option<Vec<f32>> = None;
            for (&li, l) in members.iter().zip(&layers) {
                let band = l
                    .band
                    .unwrap_or(1)
                    .checked_sub(1)
                    .context("bands are 1-based")?;
                ensure!(
                    band < block.bands,
                    "layer {}: band {} of {}",
                    l.id,
                    band + 1,
                    block.bands
                );
                match l.resample {
                    Resample::SplitSum => {
                        let (v, _) = split_sum(g, &geo, &block, band, nodata);
                        merge_sum(arrays.entry(li).or_default(), v);
                    }
                    Resample::AreaSum => {
                        let (v, _) = area_sum(g, &geo, &block, band, nodata);
                        merge_sum(arrays.entry(li).or_default(), v);
                    }
                    Resample::CentreSample => {
                        let v = centre_sample(g, &geo, &block, band, is_nd);
                        merge_first(arrays.entry(li).or_default(), v);
                    }
                    Resample::CentreMean => {
                        means
                            .entry(li)
                            .or_insert_with(|| CentreMean::new(*g))
                            .add_block(&geo, &block, band, is_nd);
                    }
                    Resample::SlopeMean | Resample::SlopeShare => {
                        let s = slope.get_or_insert_with(|| {
                            lanka_grid::slope::horn_slope_deg(&geo, &block, is_nd)
                        });
                        let m = means.entry(li).or_insert_with(|| CentreMean::new(*g));
                        let thr = l.threshold_deg;
                        for row in 0..block.height {
                            for col in 0..block.width {
                                let v = s[(row * block.width + col) as usize];
                                if v.is_nan() {
                                    continue;
                                }
                                let (lon, lat) = geo.centre(col, row);
                                let x = match thr {
                                    Some(t) if l.resample == Resample::SlopeShare => {
                                        f64::from(u8::from(f64::from(v) > t))
                                    }
                                    _ => f64::from(v),
                                };
                                m.add(lon, lat, x);
                            }
                        }
                    }
                    Resample::ClassCount
                    | Resample::ClassMode
                    | Resample::CentreMax
                    | Resample::CentreShare => {}
                }
            }
        }
        ensure!(
            used > 0,
            "no input of layers {:?} touches the grid",
            layers.iter().map(|l| &l.id).collect::<Vec<_>>()
        );
        for (&li, l) in members.iter().zip(&layers) {
            let values = match l.resample {
                Resample::SplitSum | Resample::AreaSum | Resample::CentreSample => arrays
                    .remove(&li)
                    .flatten()
                    .context("layer produced nothing")?,
                Resample::CentreMean | Resample::SlopeMean | Resample::SlopeShare => {
                    means.get(&li).context("layer produced nothing")?.finish()
                }
                Resample::CentreMax => maxes
                    .remove(&li)
                    .context("layer produced nothing")?
                    .finish(),
                Resample::CentreShare => {
                    shares.get(&li).context("layer produced nothing")?.finish()
                }
                Resample::ClassCount => {
                    let c = cc.as_ref().context("classes")?;
                    let cls = l.class.context("class")?;
                    let s = c
                        .classes
                        .iter()
                        .position(|&x| x == cls)
                        .context("class slot")?;
                    c.counts[s]
                        .iter()
                        .zip(&c.valid)
                        .map(|(&n, &v)| if v == 0 { f32::NAN } else { f32::from(n) })
                        .collect()
                }
                Resample::ClassMode => {
                    let c = cc.as_ref().context("classes")?;
                    c.mode()
                        .into_iter()
                        .map(|m| if m == 0 { f32::NAN } else { f32::from(m) })
                        .collect()
                }
            };
            out[li] = Some(BuiltLayer {
                spec: (*l).clone(),
                values,
                note: format!("{used} file(s), {pixels} pixels, {}", l.resample.as_str()),
            });
        }
    }
    out.into_iter()
        .enumerate()
        .map(|(i, b)| b.with_context(|| format!("layer {} not built", spec.layers[i].id)))
        .collect()
}
