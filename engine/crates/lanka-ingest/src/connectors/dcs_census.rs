//! Census of Population and Housing 2024 (Department of Census and
//! Statistics), final report tables A5–A7 at DS-division resolution.
//!
//! Row-level provenance: every value carries its workbook table, sheet and
//! A1 cell address plus the verbatim cell value, and points at the archived
//! XLSX (sha256). Parsing rules are ported from `foundry/src/lib/census.ts`
//! and `foundry/src/steps/census.ts` (v1): column B holds a trilingual name
//! ("si\nta\nen"), values start at column C; rows run in document order —
//! country, then each "<Name> District" followed by its DS divisions. The
//! tables' own additivity is enforced, so silent layout drift fails loudly.

use std::collections::HashMap;
use std::io::Cursor;

use anyhow::{Context, Result, bail, ensure};
use calamine::{Data, Reader, Xlsx};
use futures::FutureExt;
use futures::future::BoxFuture;
use serde_json::json;

use crate::connector::{
    Candidate, Connector, CrosswalkRow, Ctx, Fetched, ObsRow, Output, dims_key,
};

pub struct DcsCensus2024;

const SOURCE: &str = "dcs-census-2024";
const DS_POPULATION: &str = "dcs-census-2024-population";
const DS_ETH_REL: &str = "dcs-census-2024-ethnicity-religion";
const SEX_RATIO: &str = "census24.sex_ratio";
const PERIOD: &str = "2024";
const BASE: &str =
    "http://www.statistics.gov.lk/Population/StaticalInformation/CPH2024/PopulationA";
const BOUNDARY_SET: &str = "cod-ab-lka";
const NAME_SET: &str = "name:dcs-cph2024";
/// Census DS rows without a COD-AB ADM3 (2024: "Kalmunai North Sub").
const MAX_UNMATCHED_DS: usize = 5;

/// What each value column means: (indicator, dims, column label).
type ColSpec = (
    &'static str,
    &'static [(&'static str, &'static str)],
    &'static str,
);

const P: &str = "census24.pop_total";
const AGE: &str = "census24.pop_age_sex";
const A5_COLS: &[ColSpec] = &[
    (P, &[("sex", "total")], "Total"),
    (P, &[("sex", "male")], "Male"),
    (P, &[("sex", "female")], "Female"),
    (AGE, &[("age_group", "0-14"), ("sex", "total")], "Under 15"),
    (AGE, &[("age_group", "15-59"), ("sex", "total")], "15-59"),
    (AGE, &[("age_group", "60-64"), ("sex", "total")], "60-64"),
    (
        AGE,
        &[("age_group", "65+"), ("sex", "total")],
        "65 and over",
    ),
];

const E: &str = "census24.pop_by_ethnicity";
const A6_COLS: &[ColSpec] = &[
    ("", &[], "Total"),
    (E, &[("ethnicity", "sinhala")], "Sinhala"),
    (E, &[("ethnicity", "sri_lankan_tamil")], "Sri Lankan Tamil"),
    (E, &[("ethnicity", "indian_tamil")], "Indian Tamil"),
    (E, &[("ethnicity", "moor")], "Sri Lanka Yonaka/Moor"),
    (E, &[("ethnicity", "burgher")], "Burgher"),
    (E, &[("ethnicity", "malay")], "Malay"),
    (E, &[("ethnicity", "sri_lanka_chetty")], "Sri Lanka Chetty"),
    (E, &[("ethnicity", "bharatha")], "Bharatha"),
    (E, &[("ethnicity", "veddha")], "Veddha"),
    (E, &[("ethnicity", "other")], "Other"),
];

const R: &str = "census24.pop_by_religion";
const A7_COLS: &[ColSpec] = &[
    ("", &[], "Total"),
    (R, &[("religion", "buddhist")], "Buddhist"),
    (R, &[("religion", "hindu")], "Hindu"),
    (R, &[("religion", "muslim")], "Islam"),
    (R, &[("religion", "roman_catholic")], "Roman Catholic"),
    (R, &[("religion", "other_christian")], "Other Christian"),
    (R, &[("religion", "other")], "Other"),
];

/// (table, target dataset, column map).
const TABLES: &[(&str, &str, &[ColSpec])] = &[
    ("A5", DS_POPULATION, A5_COLS),
    ("A6", DS_ETH_REL, A6_COLS),
    ("A7", DS_ETH_REL, A7_COLS),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Country,
    District,
    Ds,
}

#[derive(Debug, Clone)]
pub struct SheetRow {
    pub kind: Kind,
    pub name_en: String,
    /// Absolute 0-based sheet row.
    pub row: u32,
    pub values: Vec<f64>,
    pub verbatim: Vec<String>,
}

/// Excel column letters for a 0-based column index.
#[must_use]
pub fn col_letters(mut c: u32) -> String {
    let mut s = Vec::new();
    loop {
        s.push(b'A' + u8::try_from(c % 26).unwrap_or(0));
        if c < 26 {
            break;
        }
        c = c / 26 - 1;
    }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
}

fn num(d: &Data) -> Option<(f64, String)> {
    match d {
        Data::Int(i) => Some((*i as f64, i.to_string())),
        Data::Float(f) => {
            let v = if f.fract() == 0.0 && f.abs() < 1e15 {
                format!("{f:.0}")
            } else {
                f.to_string()
            };
            Some((*f, v))
        }
        Data::String(s) => {
            let t = s.trim().replace(',', "");
            t.parse::<f64>().ok().map(|v| (v, s.trim().to_owned()))
        }
        _ => None,
    }
}

/// Extracts the data rows of one sheet (port of `parseCensusSheet`).
pub fn parse_sheet(bytes: &[u8], value_count: usize) -> Result<(String, Vec<SheetRow>)> {
    let mut wb: Xlsx<_> =
        calamine::open_workbook_from_rs(Cursor::new(bytes)).context("opening XLSX")?;
    let (sheet, range) = wb
        .worksheets()
        .into_iter()
        .next()
        .context("workbook has no sheets")?;
    let (r0, c0) = range.start().context("empty sheet")?;
    fn cell(row: &[Data], c0: u32, abs_col: u32) -> &Data {
        static EMPTY: Data = Data::Empty;
        abs_col
            .checked_sub(c0)
            .and_then(|i| row.get(i as usize))
            .unwrap_or(&EMPTY)
    }
    let (col_b, col_c) = (1u32, 2u32);
    let mut out = Vec::new();
    for (i, row) in range.rows().enumerate() {
        let abs_row = r0 + u32::try_from(i)?;
        let Data::String(name) = cell(row, c0, col_b) else {
            continue;
        };
        if name.trim().is_empty() || num(cell(row, c0, col_c)).is_none() {
            continue; // titles, headers, spacers
        }
        let name_en = name
            .split('\n')
            .map(str::trim)
            .rfind(|l| !l.is_empty())
            .unwrap_or_default()
            .to_owned();
        let mut values = Vec::with_capacity(value_count);
        let mut verbatim = Vec::with_capacity(value_count);
        for k in 0..value_count {
            let col = col_c + u32::try_from(k)?;
            let Some((v, raw)) = num(cell(row, c0, col)) else {
                bail!(
                    "row {:?} has a non-numeric value at {}{} (layout change?)",
                    name_en,
                    col_letters(col),
                    abs_row + 1
                );
            };
            values.push(v);
            verbatim.push(raw);
        }
        let kind = if name_en.contains("Sri Lanka") {
            Kind::Country
        } else if name_en.ends_with("District") {
            Kind::District
        } else {
            Kind::Ds
        };
        let name_en = if kind == Kind::Country {
            "Sri Lanka".to_owned()
        } else {
            name_en
        };
        out.push(SheetRow {
            kind,
            name_en,
            row: abs_row,
            values,
            verbatim,
        });
    }
    Ok((sheet, out))
}

/// Lowercase, keep only ASCII letters/digits ("Ja-Ela" == "Ja Ela").
#[must_use]
pub fn normalize(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Name → p-code index built from `geo.admin_units` plus the reviewed alias
/// list in `registry/crosswalks/ds-name-aliases.csv`.
pub struct NameIndex {
    districts: HashMap<String, String>,
    ds: HashMap<String, HashMap<String, String>>,
    district_alias: HashMap<String, String>,
    ds_alias: HashMap<String, String>,
}

pub enum Match {
    Exact(String),
    Alias(String),
}

impl Match {
    fn pcode(&self) -> &str {
        match self {
            Self::Exact(p) | Self::Alias(p) => p,
        }
    }
    const fn method(&self) -> &'static str {
        match self {
            Self::Exact(_) => "name_exact",
            Self::Alias(_) => "name_alias",
        }
    }
}

impl NameIndex {
    pub fn new(units: &[(String, i16, String, Option<String>)], aliases_csv: &str) -> Result<Self> {
        let mut districts = HashMap::new();
        let mut ds: HashMap<String, HashMap<String, String>> = HashMap::new();
        for (pcode, level, name, parent) in units {
            match level {
                2 => {
                    districts.insert(normalize(name.trim_end_matches(" District")), pcode.clone());
                }
                3 => {
                    if let Some(p) = parent {
                        ds.entry(p.clone())
                            .or_default()
                            .insert(normalize(name), pcode.clone());
                    }
                }
                _ => {}
            }
        }
        let (mut district_alias, mut ds_alias) = (HashMap::new(), HashMap::new());
        for line in aliases_csv.lines().skip(1).filter(|l| !l.trim().is_empty()) {
            let f: Vec<&str> = line.splitn(4, ',').collect();
            ensure!(f.len() >= 3, "bad alias line {line:?}");
            let target = if f[0] == "2" {
                &mut district_alias
            } else {
                &mut ds_alias
            };
            target.insert(f[1].to_owned(), f[2].to_owned());
        }
        Ok(Self {
            districts,
            ds,
            district_alias,
            ds_alias,
        })
    }

    pub fn district(&self, census_name: &str) -> Option<Match> {
        let n = normalize(
            census_name
                .trim_end_matches(" District")
                .trim_end_matches("District"),
        );
        if let Some(p) = self.districts.get(&n) {
            return Some(Match::Exact(p.clone()));
        }
        let a = self.district_alias.get(&n)?;
        self.districts.get(a).map(|p| Match::Alias(p.clone()))
    }

    pub fn ds(&self, district: &str, census_name: &str) -> Option<Match> {
        let n = normalize(census_name);
        let within = self.ds.get(district)?;
        if let Some(p) = within.get(&n) {
            return Some(Match::Exact(p.clone()));
        }
        let a = self.ds_alias.get(&n)?;
        within.get(a).map(|p| Match::Alias(p.clone()))
    }
}

/// A census row with no COD-AB unit: (district p-code, census name, total).
type Unmatched = (String, String, f64);

struct Resolved {
    pcode: String,
    row: SheetRow,
}

/// Port of `resolveRows`: document-order resolution + additivity checks.
fn resolve(
    table: &str,
    rows: Vec<SheetRow>,
    idx: &NameIndex,
    xw: &mut Vec<CrosswalkRow>,
) -> Result<(Vec<Resolved>, Vec<Unmatched>)> {
    let mut resolved = Vec::new();
    let mut unmatched = Vec::new();
    let mut country: Option<Vec<f64>> = None;
    let mut district: Option<(String, String, Vec<f64>)> = None;
    let mut ds_sum: Vec<f64> = Vec::new();
    let mut district_sum: Vec<f64> = Vec::new();

    let check = |d: &Option<(String, String, Vec<f64>)>, sums: &[f64]| -> Result<()> {
        if let Some((_, name, vals)) = d {
            ensure!(
                vals.as_slice() == sums,
                "{table}: DS rows of {name} do not sum to the district row"
            );
        }
        Ok(())
    };

    for row in rows {
        match row.kind {
            Kind::Country => {
                country = Some(row.values.clone());
                district_sum = vec![0.0; row.values.len()];
                resolved.push(Resolved {
                    pcode: "LK".to_owned(),
                    row,
                });
            }
            Kind::District => {
                check(&district, &ds_sum)?;
                let m = idx
                    .district(&row.name_en)
                    .with_context(|| format!("{table}: unknown district {:?}", row.name_en))?;
                xw.push(CrosswalkRow {
                    from_set: NAME_SET.to_owned(),
                    from_code: row.name_en.clone(),
                    to_set: BOUNDARY_SET.to_owned(),
                    to_code: m.pcode().to_owned(),
                    weight: 1.0,
                    method: m.method(),
                });
                ds_sum = vec![0.0; row.values.len()];
                for (s, v) in district_sum.iter_mut().zip(&row.values) {
                    *s += v;
                }
                district = Some((
                    m.pcode().to_owned(),
                    row.name_en.clone(),
                    row.values.clone(),
                ));
                resolved.push(Resolved {
                    pcode: m.pcode().to_owned(),
                    row,
                });
            }
            Kind::Ds => {
                let Some((dp, _, _)) = &district else {
                    bail!("{table}: DS row {:?} before any district", row.name_en)
                };
                for (s, v) in ds_sum.iter_mut().zip(&row.values) {
                    *s += v;
                }
                match idx.ds(dp, &row.name_en) {
                    Some(m) => {
                        xw.push(CrosswalkRow {
                            from_set: NAME_SET.to_owned(),
                            from_code: format!("{dp}/{}", row.name_en),
                            to_set: BOUNDARY_SET.to_owned(),
                            to_code: m.pcode().to_owned(),
                            weight: 1.0,
                            method: m.method(),
                        });
                        resolved.push(Resolved {
                            pcode: m.pcode().to_owned(),
                            row,
                        });
                    }
                    None => unmatched.push((dp.clone(), row.name_en.clone(), row.values[0])),
                }
            }
        }
    }
    check(&district, &ds_sum)?;
    let country = country.with_context(|| format!("{table}: no country row"))?;
    ensure!(
        country == district_sum,
        "{table}: district rows do not sum to the country row"
    );
    Ok((resolved, unmatched))
}

impl Connector for DcsCensus2024 {
    fn source_id(&self) -> &'static str {
        SOURCE
    }

    fn discover<'a>(&'a self, ctx: &'a Ctx<'a>) -> BoxFuture<'a, Result<Vec<Candidate>>> {
        async move {
            Ok(TABLES
                .iter()
                .map(|(t, _, _)| {
                    let local = ctx.opts.from_dir.as_ref().map(|d| {
                        d.join(match *t {
                            "A5" => "a5-sex-age.xlsx",
                            "A6" => "a6-ethnicity.xlsx",
                            _ => "a7-religion.xlsx",
                        })
                    });
                    Candidate {
                        key: (*t).to_owned(),
                        url: format!("{BASE}/{t}"),
                        local_path: local,
                        media_type: Some(
                            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
                                .to_owned(),
                        ),
                        edition: Some("CPH 2024 final report".to_owned()),
                    }
                })
                .collect())
        }
        .boxed()
    }

    fn parse<'a>(
        &'a self,
        ctx: &'a Ctx<'a>,
        fetched: &'a [Fetched],
    ) -> BoxFuture<'a, Result<Vec<Output>>> {
        async move {
            let units: Vec<(String, i16, String, Option<String>)> = sqlx::query_as(
                "SELECT pcode, level, name_en, parent_pcode FROM geo.admin_units WHERE level IN (2, 3)",
            )
            .fetch_all(ctx.pool)
            .await?;
            ensure!(!units.is_empty(), "no ADM2/ADM3 units in geo.admin_units — run `lanka ingest run cod-ab-lka` first");
            let alias_path = ctx.registry.root.join("crosswalks/ds-name-aliases.csv");
            let aliases = std::fs::read_to_string(&alias_path)
                .with_context(|| format!("reading {}", alias_path.display()))?;
            let idx = NameIndex::new(&units, &aliases)?;

            let mut rows: HashMap<&str, Vec<ObsRow>> = HashMap::new();
            let mut xw: Vec<CrosswalkRow> = Vec::new();
            let mut totals: HashMap<&str, HashMap<String, f64>> = HashMap::new();
            for (t, dataset, cols) in TABLES {
                let out = rows.entry(*dataset).or_default();
                let (ai, f) = fetched
                    .iter()
                    .enumerate()
                    .find(|(_, f)| f.key == *t)
                    .with_context(|| format!("table {t} was not fetched"))?;
                let (sheet, sheet_rows) = parse_sheet(&f.bytes, cols.len()).with_context(|| format!("table {t}"))?;
                let (resolved, unmatched) = resolve(t, sheet_rows, &idx, &mut xw)?;
                ensure!(
                    unmatched.len() <= MAX_UNMATCHED_DS,
                    "{t}: {} unmatched DS divisions (max {MAX_UNMATCHED_DS}): {:?}",
                    unmatched.len(),
                    unmatched
                );
                for (d, name, total) in &unmatched {
                    ctx.run
                        .warn(
                            crate::run::Stage::Parse,
                            &format!("{t}: DS row {name:?} in {d} has no COD-AB ADM3 (population {total}) — not keyed"),
                            None,
                        )
                        .await;
                }
                let tot = totals.entry(t).or_default();
                for r in resolved {
                    tot.insert(r.pcode.clone(), r.row.values[0]);
                    for (k, (indicator, dims, label)) in cols.iter().enumerate() {
                        if indicator.is_empty() {
                            continue; // grand total column of A6/A7: checked, not stored twice
                        }
                        let col = 2 + u32::try_from(k)?;
                        out.push(ObsRow {
                            indicator_id: (*indicator).to_owned(),
                            geo_code: r.pcode.clone(),
                            period: PERIOD.to_owned(),
                            dims: dims_key(dims),
                            value: Some(r.row.values[k]),
                            status: "reported",
                            artifact: ai,
                            locator: json!({
                                "table": t, "sheet": sheet,
                                "cell": format!("{}{}", col_letters(col), r.row.row + 1),
                                "row": r.row.name_en, "column": label,
                            }),
                            verbatim: Some(r.row.verbatim[k].clone()),
                            method: "sheet_cell",
                            confidence: Some(1.0),
                        });
                    }
                    if *t == "A5" && r.row.values[2] > 0.0 {
                        // Declared derived indicator: males per 100 females.
                        let (m, f) = (r.row.values[1], r.row.values[2]);
                        out.push(ObsRow {
                            indicator_id: SEX_RATIO.to_owned(),
                            geo_code: r.pcode.clone(),
                            period: PERIOD.to_owned(),
                            dims: String::new(),
                            value: Some((100.0 * m / f * 1e4).round() / 1e4),
                            status: "derived",
                            artifact: ai,
                            locator: json!({
                                "table": t, "sheet": sheet, "formula": "100 * male / female",
                                "inputs": [format!("D{}", r.row.row + 1), format!("E{}", r.row.row + 1)],
                                "row": r.row.name_en,
                            }),
                            verbatim: None,
                            method: "derived",
                            confidence: Some(1.0),
                        });
                    }
                }
            }
            // The three tables publish one enumeration: totals must agree.
            let a5 = totals.get("A5").context("A5 missing")?;
            for t in ["A6", "A7"] {
                for (p, v) in totals.get(t).with_context(|| format!("{t} missing"))? {
                    ensure!(a5.get(p) == Some(v), "{t} total for {p} ({v}) disagrees with A5 ({:?})", a5.get(p));
                }
            }
            xw.sort_by(|a, b| (&a.from_code, &a.to_code).cmp(&(&b.from_code, &b.to_code)));
            xw.dedup_by(|a, b| a.from_code == b.from_code && a.to_code == b.to_code);
            let mut outputs = vec![Output::Crosswalks { source_id: SOURCE.to_owned(), rows: xw }];
            for dataset in [DS_POPULATION, DS_ETH_REL] {
                outputs.push(Output::Observations {
                    dataset_id: dataset.to_owned(),
                    vintage: Some("CPH 2024 final report".to_owned()),
                    rows: rows.remove(dataset).unwrap_or_default(),
                });
            }
            Ok(outputs)
        }
        .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_letters() {
        assert_eq!(col_letters(0), "A");
        assert_eq!(col_letters(2), "C");
        assert_eq!(col_letters(25), "Z");
        assert_eq!(col_letters(26), "AA");
        assert_eq!(col_letters(27), "AB");
    }

    #[test]
    fn names_normalise_and_alias() {
        let units = vec![
            (
                "LK11".to_owned(),
                2,
                "Colombo District".to_owned(),
                Some("LK1".to_owned()),
            ),
            (
                "LK13".to_owned(),
                2,
                "Kalutara District".to_owned(),
                Some("LK1".to_owned()),
            ),
            (
                "LK1103".to_owned(),
                3,
                "Colombo".to_owned(),
                Some("LK11".to_owned()),
            ),
            (
                "LK1336".to_owned(),
                3,
                "Matugama".to_owned(),
                Some("LK13".to_owned()),
            ),
        ];
        let idx = NameIndex::new(
            &units,
            "level,alias_norm,target_norm,note\n3,mathugama,matugama,x\n",
        )
        .expect("index");
        assert!(matches!(idx.district("Colombo District"), Some(Match::Exact(p)) if p == "LK11"));
        assert!(matches!(idx.ds("LK13", "Mathugama"), Some(Match::Alias(p)) if p == "LK1336"));
        assert!(
            idx.ds("LK11", "Mathugama").is_none(),
            "aliases never cross districts"
        );
    }

    /// Parses the real workbook when `LANKA_TEST_CENSUS_DIR` points at the
    /// v1 raw cache (`foundry/data/raw/census-2024`).
    #[test]
    fn parses_real_a5_if_present() {
        let Some(dir) = std::env::var_os("LANKA_TEST_CENSUS_DIR") else {
            return;
        };
        let bytes =
            std::fs::read(std::path::Path::new(&dir).join("a5-sex-age.xlsx")).expect("a5 workbook");
        let (sheet, rows) = parse_sheet(&bytes, A5_COLS.len()).expect("parses");
        assert_eq!(sheet, "A5");
        assert_eq!(rows[0].kind, Kind::Country);
        assert_eq!(rows[0].values[0], 21_781_800.0);
        assert_eq!(rows[0].row + 1, 8, "country row is sheet row 8");
        assert_eq!(rows.iter().filter(|r| r.kind == Kind::District).count(), 25);
    }
}
