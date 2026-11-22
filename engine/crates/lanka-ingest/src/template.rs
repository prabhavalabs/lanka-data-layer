//! Table templates (03 §3.2): where a table is in a PDF and how to read it,
//! as data in `registry/templates/<source>/<table>.yaml`.
//!
//! ```yaml
//! id: dcs-statistical-abstract/table-2.1a-midyear-population-district
//! editions:
//!   - match: { edition: "2024..2026" }
//!     locate: { page: { find_text: "Table 2.1a", then_text: "mid year population" }, span_pages: 1 }
//!     label: { key: district, header: "District" }
//!     columns:                                     # x-ranges come from header words, never pixels
//!       - { key: total_2020, header: "Total", under: "2020" }
//!       - { key: male_2020,  header: "Male",  under: "2020" }
//!     rows: { stop_at: ["(1)", "Note"] }
//!     number: { thousands: ",", missing: ["..", "-"] }
//!     checks:
//!       - { sum: [male_2020, female_2020], equals: total_2020, tol: 1 }
//!       - { sum_rows: all_but_first, equals_row: "Sri Lanka", tol: 13 }
//! ```
//!
//! Algorithm (pure, over sandbox [`Words`]): drop superscripts (words much
//! shorter than the page's median height — footnote markers); find the first
//! page that has `find_text` (and `then_text`) **and** on which every column
//! header resolves; cluster words into lines by y; resolve each column's
//! header phrase below the anchor (a repeated header is disambiguated by the
//! nearest `under` group label above it, then `nth`); column x-ranges are the
//! midpoints between header centres; the row label is the rightmost word
//! cluster left of the first column (and right of the label header); every
//! line between the header and `stop_at` with a label and at least one
//! number is a row. [`Table::check`] runs the template checks on the parsed
//! values; a failing check fails the extract.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail, ensure};
use lanka_extractd::{Word, Words};
use serde::{Deserialize, Serialize};

/// Error prefix of a template whose anchors or headers are not found (the
/// pipeline opens a `template_miss` incident on it).
pub const TEMPLATE_MISS: &str = "template_miss";

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Template {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    pub editions: Vec<Edition>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EditionMatch {
    /// `"2025"` or a range of leading years `"2024..2026"`; absent = any.
    #[serde(default)]
    pub edition: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Edition {
    #[serde(rename = "match", default)]
    pub when: EditionMatch,
    pub locate: Locate,
    #[serde(default)]
    pub label: LabelSpec,
    pub columns: Vec<ColumnSpec>,
    #[serde(default)]
    pub rows: RowsSpec,
    #[serde(default)]
    pub number: NumberSpec,
    /// Drop words shorter than this share of the page's median word height
    /// (footnote superscripts); 0 disables.
    #[serde(default = "default_superscript")]
    pub superscript_ratio: f32,
    #[serde(default)]
    pub checks: Vec<Check>,
}

const fn default_superscript() -> f32 {
    0.75
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Locate {
    pub page: PageFind,
    #[serde(default = "one")]
    pub span_pages: u32,
}

const fn one() -> u32 {
    1
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PageFind {
    pub find_text: String,
    #[serde(default)]
    pub then_text: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LabelSpec {
    #[serde(default = "label_key")]
    pub key: String,
    /// Header of the label column: label words start at its left edge.
    #[serde(default)]
    pub header: Option<String>,
    /// Tolerance left of the label header, points.
    #[serde(default = "label_slack")]
    pub slack: f32,
    /// Keep every word cluster between the label header and the first
    /// column ("2022    January"), not only the rightmost one.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub all_clusters: bool,
}

fn label_key() -> String {
    "label".to_owned()
}

const fn label_slack() -> f32 {
    4.0
}

impl Default for LabelSpec {
    fn default() -> Self {
        Self {
            key: label_key(),
            header: None,
            slack: label_slack(),
            all_clusters: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ColumnSpec {
    pub key: String,
    pub header: String,
    /// Group label printed above (spanning) the header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub under: Option<String>,
    /// 1-based occurrence among the qualifying headers, left to right.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nth: Option<usize>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RowsSpec {
    /// Rows start below the line containing this text (default: below the
    /// column headers).
    #[serde(default)]
    pub start_after: Option<String>,
    /// The first line (below the headers) containing any of these ends the table.
    #[serde(default)]
    pub stop_at: Vec<String>,
    /// Minimum number cells for a line to be a row (default 1).
    #[serde(default)]
    pub min_cells: Option<usize>,
    /// A line of numbers without a label takes the label printed on the
    /// line(s) above it that had no numbers (labels wrapped over two lines,
    /// numbers aligned with the last or a middle line).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub label_above: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NumberSpec {
    #[serde(default = "comma")]
    pub thousands: String,
    #[serde(default = "dot")]
    pub decimal: String,
    /// Tokens meaning "no value" (`..`, `-`, `n.a.`).
    #[serde(default = "missing_tokens")]
    pub missing: Vec<String>,
    /// Trailing markers stripped before parsing (`*`, `(p)`).
    #[serde(default)]
    pub strip: Vec<String>,
    /// How numbers sit under their headers (`right`: by right edge).
    #[serde(default)]
    pub align: Align,
    /// Remove spaces inside a cell before parsing ("7 . 0" → 7.0): for text
    /// layers that split a number into several words. Opt-in, since it would
    /// also join two numbers that fell into one cell.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub squeeze_spaces: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    #[default]
    Right,
    Center,
}

fn comma() -> String {
    ",".to_owned()
}
fn dot() -> String {
    ".".to_owned()
}
fn missing_tokens() -> Vec<String> {
    ["..", "...", "-", "–", "—", "n.a.", "na", "N/A"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect()
}

impl Default for NumberSpec {
    fn default() -> Self {
        Self {
            thousands: comma(),
            decimal: dot(),
            missing: missing_tokens(),
            strip: Vec::new(),
            align: Align::Right,
            squeeze_spaces: false,
        }
    }
}

/// Rows a check sums.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum RowSet {
    /// `all`, `all_but_first`, `all_but_last`.
    Named(String),
    /// Row labels (`"LKR Denominated#2"` = second row with that label).
    Labels(Vec<String>),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MinMax {
    #[serde(default)]
    pub min: Option<usize>,
    #[serde(default)]
    pub max: Option<usize>,
}

/// A template check, run on the extract before any mapping (03 §3.2).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Check {
    /// Per row: the listed columns sum to `equals`.
    Sum {
        sum: Vec<String>,
        equals: String,
        #[serde(default)]
        tol: f64,
    },
    /// Per column: the listed rows sum to `equals_row`.
    SumRows {
        sum_rows: RowSet,
        equals_row: String,
        #[serde(default)]
        columns: Option<Vec<String>>,
        #[serde(default)]
        tol: f64,
    },
    RowCount {
        row_count: MinMax,
    },
    RowsPresent {
        rows_present: Vec<String>,
    },
}

/// Result of one check.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckResult {
    pub check: String,
    pub ok: bool,
    /// Instances evaluated.
    pub n: usize,
    /// Failing instances, described.
    pub failures: Vec<String>,
    /// (row label, column key) of the cells a failure involves.
    #[serde(default)]
    pub cells: Vec<(String, String)>,
}

// ───────────────────────────── text matching ───────────────────────────────

/// Lowercase ASCII letters and digits only ("Total;" == "total").
#[must_use]
pub fn norm(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Does a printed word match one normalised pattern token? Equal, or the
/// pattern is a prefix followed by a non-letter ("2020" ~ "2020(1)", but
/// "No" !~ "Note").
fn word_matches(word: &str, tok: &str) -> bool {
    if !tok.bytes().any(|b| b.is_ascii_alphanumeric()) {
        // A symbol header ("%"): compare as printed.
        return word.trim() == tok;
    }
    let n = norm(word);
    if n == tok {
        return true;
    }
    if tok.is_empty() || !n.starts_with(tok) {
        return false;
    }
    // Find the original char right after the first `tok.len()` alphanumerics.
    let mut seen = 0;
    let mut chars = word.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_alphanumeric() {
            seen += 1;
            if seen == tok.len() {
                return chars.peek().is_none_or(|n| !n.is_ascii_alphabetic());
            }
        }
    }
    false
}

fn tokens(phrase: &str) -> Vec<String> {
    phrase
        .split_whitespace()
        .map(|t| {
            let n = norm(t);
            if n.is_empty() { t.to_owned() } else { n }
        })
        .collect()
}

/// A line of words (sorted left to right) on one page.
#[derive(Debug, Clone)]
pub struct Line<'w> {
    pub page: u32,
    pub words: Vec<&'w Word>,
    pub y0: f32,
    pub y1: f32,
}

impl Line<'_> {
    fn cy(&self) -> f32 {
        (self.y0 + self.y1) / 2.0
    }

    /// Start indexes of every occurrence of the phrase.
    fn find(&self, toks: &[String]) -> Vec<usize> {
        if toks.is_empty() || toks.len() > self.words.len() {
            return Vec::new();
        }
        (0..=self.words.len() - toks.len())
            .filter(|&i| {
                toks.iter()
                    .enumerate()
                    .all(|(k, t)| word_matches(&self.words[i + k].text, t))
            })
            .collect()
    }
}

fn median(mut v: Vec<f32>) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f32::total_cmp);
    v[v.len() / 2]
}

/// Groups a page's words into lines by vertical centre.
#[must_use]
pub fn lines<'w>(words: &[&'w Word]) -> Vec<Line<'w>> {
    let h = median(words.iter().map(|w| w.height()).collect()).max(1.0);
    let mut ws: Vec<&Word> = words.to_vec();
    ws.sort_by(|a, b| {
        a.cy()
            .total_cmp(&b.cy())
            .then(a.bbox[0].total_cmp(&b.bbox[0]))
    });
    let mut out: Vec<Line> = Vec::new();
    for w in ws {
        match out.last_mut() {
            Some(l) if (w.cy() - l.cy()).abs() <= 0.45 * h => {
                l.words.push(w);
                l.y0 = l.y0.min(w.bbox[1]);
                l.y1 = l.y1.max(w.bbox[3]);
            }
            _ => out.push(Line {
                page: w.page,
                words: vec![w],
                y0: w.bbox[1],
                y1: w.bbox[3],
            }),
        }
    }
    for l in &mut out {
        l.words.sort_by(|a, b| a.bbox[0].total_cmp(&b.bbox[0]));
    }
    out
}

/// Words of a line split where the horizontal gap exceeds `gap`.
fn clusters<'a, 'w>(words: &'a [&'w Word], gap: f32) -> Vec<&'a [&'w Word]> {
    let mut out = Vec::new();
    let mut start = 0;
    for i in 1..words.len() {
        if words[i].bbox[0] - words[i - 1].bbox[2] > gap {
            out.push(&words[start..i]);
            start = i;
        }
    }
    if !words.is_empty() {
        out.push(&words[start..]);
    }
    out
}

fn span(ws: &[&Word]) -> [f32; 4] {
    let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for w in ws {
        b[0] = b[0].min(w.bbox[0]);
        b[1] = b[1].min(w.bbox[1]);
        b[2] = b[2].max(w.bbox[2]);
        b[3] = b[3].max(w.bbox[3]);
    }
    b
}

fn join(ws: &[&Word]) -> String {
    ws.iter()
        .map(|w| w.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Text of `ws` with any superscript marker printed against it (`2020` +
/// `(1)` → `2020(1)`): raised markers that start right at a word's end.
fn with_marks(ws: &[&Word], sup: &[&Word]) -> String {
    let mut out = String::new();
    for (i, w) in ws.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(&w.text);
        let mut marks: Vec<&&Word> = sup
            .iter()
            .filter(|s| {
                s.bbox[0] >= w.bbox[2] - 2.0
                    && s.bbox[0] <= w.bbox[2] + 3.0
                    && s.cy() >= w.bbox[1] - 1.0
                    && s.cy() <= w.cy()
            })
            .collect();
        marks.sort_by(|a, b| a.bbox[0].total_cmp(&b.bbox[0]));
        for m in marks {
            // "2020" + "(1)" → "2020(1)"; "2026" + "1" → "2026 1" (keeps the year readable).
            if m.text.starts_with(|c: char| c.is_ascii_alphanumeric()) {
                out.push(' ');
            }
            out.push_str(&m.text);
        }
    }
    out
}

// ───────────────────────────── values ──────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(f64),
    /// A printed "no value" token (`..`).
    Missing,
    /// Not a number.
    Text,
}

impl NumberSpec {
    /// Parses a printed token.
    #[must_use]
    pub fn parse(&self, text: &str) -> Value {
        let squeezed: String;
        let mut t = text.trim();
        if self.squeeze_spaces {
            squeezed = t.split_whitespace().collect();
            t = &squeezed;
        }
        for s in &self.strip {
            t = t.trim_end_matches(s.as_str()).trim();
        }
        if self.missing.iter().any(|m| m == t) {
            return Value::Missing;
        }
        let (neg, body) = match t.strip_prefix('-').or_else(|| t.strip_prefix('−')) {
            Some(b) => (true, b.trim()),
            None => (false, t),
        };
        let (int, frac) = match body.split_once(self.decimal.as_str()) {
            Some((i, f)) => (i, Some(f)),
            None => (body, None),
        };
        let groups: Vec<&str> = if self.thousands.is_empty() {
            vec![int]
        } else {
            int.split(self.thousands.as_str()).collect()
        };
        let ok_int = !groups.is_empty()
            && groups
                .iter()
                .all(|g| !g.is_empty() && g.bytes().all(|b| b.is_ascii_digit()))
            && (groups.len() == 1
                || (groups[0].len() <= 3 && groups[1..].iter().all(|g| g.len() == 3)));
        let ok_frac = frac.is_none_or(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()));
        if !ok_int || !ok_frac {
            return Value::Text;
        }
        let s = format!(
            "{}{}",
            groups.concat(),
            frac.map(|f| format!(".{f}")).unwrap_or_default()
        );
        match s.parse::<f64>() {
            Ok(v) => Value::Number(if neg { -v } else { v }),
            Err(_) => Value::Text,
        }
    }
}

// ───────────────────────────── the table ───────────────────────────────────

/// One located cell: printed text and where it was printed.
#[derive(Debug, Clone, PartialEq)]
pub struct TCell {
    pub text: String,
    pub page: u32,
    pub bbox: [f32; 4],
    pub value: Value,
}

#[derive(Debug, Clone)]
pub struct TRow {
    /// Row label as printed.
    pub label: String,
    pub label_page: u32,
    pub label_bbox: [f32; 4],
    /// By column key.
    pub cells: BTreeMap<String, TCell>,
}

#[derive(Debug, Clone)]
pub struct THeader {
    pub key: String,
    /// Group label (if any) + header, as printed.
    pub text: String,
    pub page: u32,
    pub bbox: [f32; 4],
}

/// A table read through a template.
#[derive(Debug, Clone)]
pub struct Table {
    pub template_id: String,
    pub edition_idx: usize,
    pub first_page: u32,
    /// Label column first, then the template's columns in order.
    pub headers: Vec<THeader>,
    pub rows: Vec<TRow>,
}

impl Template {
    /// Parses and sanity-checks a template file.
    pub fn from_yaml(text: &str) -> Result<Self> {
        let t: Self = serde_yaml_ng::from_str(text)?;
        ensure!(!t.editions.is_empty(), "{}: no editions", t.id);
        for e in &t.editions {
            ensure!(!e.columns.is_empty(), "{}: an edition has no columns", t.id);
            let mut keys: Vec<&str> = e.columns.iter().map(|c| c.key.as_str()).collect();
            keys.push(&e.label.key);
            let n = keys.len();
            keys.sort_unstable();
            keys.dedup();
            ensure!(keys.len() == n, "{}: duplicate column keys", t.id);
            for c in &e.checks {
                if let Check::Sum { sum, equals, .. } = c {
                    for k in sum.iter().chain([equals]) {
                        ensure!(
                            e.columns.iter().any(|c| &c.key == k),
                            "{}: check refers to unknown column {k:?}",
                            t.id
                        );
                    }
                }
            }
        }
        Ok(t)
    }

    /// The edition whose `match` accepts `edition` (first wins).
    pub fn edition(&self, edition: Option<&str>) -> Result<(usize, &Edition)> {
        self.editions
            .iter()
            .enumerate()
            .find(|(_, e)| edition_matches(e.when.edition.as_deref(), edition))
            .with_context(|| {
                format!(
                    "{TEMPLATE_MISS}: {} has no edition matching {:?}",
                    self.id,
                    edition.unwrap_or("(none)")
                )
            })
    }

    /// Locates and reads the table. Errors starting with [`TEMPLATE_MISS`]
    /// mean the layout was not recognised.
    pub fn read(&self, words: &Words, edition: Option<&str>) -> Result<Table> {
        let (idx, e) = self.edition(edition)?;
        let pages = candidate_pages(words, e);
        ensure!(
            !pages.is_empty(),
            "{TEMPLATE_MISS}: {}: no page contains {:?}{}",
            self.id,
            e.locate.page.find_text,
            e.locate
                .page
                .then_text
                .as_deref()
                .map(|t| format!(" and {t:?}"))
                .unwrap_or_default()
        );
        let mut last_err = None;
        for (page, anchor_y) in pages {
            match read_at(self, idx, e, words, page, anchor_y) {
                Ok(t) => return Ok(t),
                Err(err) => last_err = Some(err),
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("{TEMPLATE_MISS}: {}", self.id)))
    }
}

/// `"2025"` matches `"2025"`, `"2024..2026"` matches leading years 2024–2026,
/// and `"2025-Q1..2027-Q1"` matches leading years 2025–2027 whose text after
/// the year is `-Q1` (both bounds must carry the same suffix): one edition
/// entry per recurring layout, such as a quarter of a quarterly bulletin.
fn edition_matches(pattern: Option<&str>, edition: Option<&str>) -> bool {
    let Some(p) = pattern else { return true };
    let Some(e) = edition else { return false };
    let year = |s: &str| s.trim().get(..4).and_then(|y| y.parse::<i32>().ok());
    let suffix = |s: &str| s.trim().get(4..).unwrap_or("").to_owned();
    match p.split_once("..") {
        Some((a, b)) => match (year(a), year(b), year(e)) {
            (Some(ya), Some(yb), Some(y)) => {
                let (sa, sb) = (suffix(a), suffix(b));
                (ya..=yb).contains(&y) && (sa.is_empty() || (sa == sb && sa == suffix(e)))
            }
            _ => false,
        },
        None => p.trim() == e.trim(),
    }
}

/// Words of a page without superscripts.
fn page_words(words: &Words, page: u32, ratio: f32) -> Vec<&Word> {
    let ws: Vec<&Word> = words.on_page(page).collect();
    if ratio <= 0.0 {
        return ws;
    }
    let h = median(ws.iter().map(|w| w.height()).collect());
    ws.into_iter().filter(|w| w.height() >= ratio * h).collect()
}

/// Pages with the anchor text (and `then_text`), with the bottom of the
/// anchor line: headers and rows are searched strictly below it.
fn candidate_pages(words: &Words, e: &Edition) -> Vec<(u32, f32)> {
    let find = tokens(&e.locate.page.find_text);
    let then = e.locate.page.then_text.as_deref().map(tokens);
    let mut out = Vec::new();
    for p in &words.pages {
        let ls = lines(&page_words(words, p.page, e.superscript_ratio));
        let anchor = ls
            .iter()
            .find_map(|l| (!l.find(&find).is_empty()).then_some(l.y1));
        let Some(y) = anchor else { continue };
        if let Some(t) = &then
            && !ls.iter().any(|l| l.y1 >= y - 0.5 && !l.find(t).is_empty())
        {
            continue;
        }
        out.push((p.page, y));
    }
    out
}

/// A phrase occurrence: the words it spans.
struct Hit<'w> {
    line: usize,
    words: Vec<&'w Word>,
}

impl Hit<'_> {
    fn bbox(&self) -> [f32; 4] {
        span(&self.words)
    }
    fn cx(&self) -> f32 {
        let b = self.bbox();
        (b[0] + b[2]) / 2.0
    }
}

fn hits<'w>(ls: &[Line<'w>], phrase: &str, below: f32) -> Vec<Hit<'w>> {
    let toks = tokens(phrase);
    let mut out = Vec::new();
    for (li, l) in ls.iter().enumerate().filter(|(_, l)| l.y0 >= below - 0.5) {
        for i in l.find(&toks) {
            out.push(Hit {
                line: li,
                words: l.words[i..i + toks.len()].to_vec(),
            });
        }
    }
    out
}

fn h_dist(x: f32, b: [f32; 4]) -> f32 {
    if x < b[0] {
        b[0] - x
    } else if x > b[2] {
        x - b[2]
    } else {
        0.0
    }
}

#[allow(clippy::too_many_lines)]
fn read_at(
    t: &Template,
    idx: usize,
    e: &Edition,
    words: &Words,
    page: u32,
    anchor_y: f32,
) -> Result<Table> {
    let ws = page_words(words, page, e.superscript_ratio);
    // The dropped superscripts, kept to print header text as printed ("2020(1)").
    let sup: Vec<&Word> = words
        .on_page(page)
        .filter(|w| !ws.iter().any(|k| std::ptr::eq(*k, *w)))
        .collect();
    let ls = lines(&ws);
    let line_h = median(ws.iter().map(|w| w.height()).collect()).max(1.0);
    let miss = |what: String| anyhow::anyhow!("{TEMPLATE_MISS}: {} (page {page}): {what}", t.id);

    // Group labels: every `under` phrase of the template, widened to the
    // word cluster it sits in on its line.
    let mut groups: Vec<(String, [f32; 4], usize)> = Vec::new();
    let unders: Vec<&str> = {
        let mut u: Vec<&str> = e
            .columns
            .iter()
            .filter_map(|c| c.under.as_deref())
            .collect();
        u.sort_unstable();
        u.dedup();
        u
    };
    for u in &unders {
        for h in hits(&ls, u, anchor_y) {
            let l = &ls[h.line];
            let first = h.words[0];
            let cl = clusters(&l.words, 1.5 * line_h)
                .into_iter()
                .find(|c| c.iter().any(|w| std::ptr::eq(*w, first)))
                .map_or_else(|| h.bbox(), span);
            groups.push(((*u).to_owned(), cl, h.line));
        }
    }

    // Column headers.
    let mut headers = Vec::with_capacity(e.columns.len());
    for c in &e.columns {
        let mut cands: Vec<Hit> = hits(&ls, &c.header, anchor_y);
        if let Some(u) = &c.under {
            cands.retain(|h| {
                let cy = (h.bbox()[1] + h.bbox()[3]) / 2.0;
                let near = groups
                    .iter()
                    .filter(|(_, b, gl)| *gl != h.line && b[3] <= cy && cy - b[3] < 8.0 * line_h)
                    .min_by(|a, b| {
                        h_dist(h.cx(), a.1)
                            .total_cmp(&h_dist(h.cx(), b.1))
                            .then((cy - a.1[3]).total_cmp(&(cy - b.1[3])))
                    });
                near.is_some_and(|(g, _, _)| g == u)
            });
        }
        // Topmost qualifying line, left to right.
        let Some(top) = cands.iter().map(|h| h.line).min() else {
            return Err(miss(format!(
                "header {:?}{} not found",
                c.header,
                c.under
                    .as_deref()
                    .map(|u| format!(" under {u:?}"))
                    .unwrap_or_default()
            )));
        };
        cands.retain(|h| h.line == top);
        cands.sort_by(|a, b| a.cx().total_cmp(&b.cx()));
        let pick = c.nth.unwrap_or(1);
        ensure!(pick >= 1, "{}: nth is 1-based", t.id);
        let h = cands
            .get(pick - 1)
            .ok_or_else(|| miss(format!("header {:?}: no occurrence #{pick}", c.header)))?;
        let group_text = c.under.as_ref().and_then(|u| {
            let cy = (h.bbox()[1] + h.bbox()[3]) / 2.0;
            groups
                .iter()
                .filter(|(g, b, gl)| {
                    g == u && *gl != h.line && b[3] <= cy && cy - b[3] < 8.0 * line_h
                })
                .min_by(|a, b| {
                    h_dist(h.cx(), a.1)
                        .total_cmp(&h_dist(h.cx(), b.1))
                        .then((cy - a.1[3]).total_cmp(&(cy - b.1[3])))
                })
                .map(|(_, b, gl)| {
                    let l = &ls[*gl];
                    let inside: Vec<&Word> = l
                        .words
                        .iter()
                        .copied()
                        .filter(|w| w.cx() >= b[0] && w.cx() <= b[2])
                        .collect();
                    with_marks(&inside, &sup)
                })
        });
        headers.push(THeader {
            key: c.key.clone(),
            text: match group_text {
                Some(g) => format!("{g} {}", with_marks(&h.words, &sup)),
                None => with_marks(&h.words, &sup),
            },
            page,
            bbox: h.bbox(),
        });
    }
    // Two columns resolved to the same header occurrence = ambiguous template.
    for i in 0..headers.len() {
        for j in i + 1..headers.len() {
            ensure!(
                headers[i].bbox != headers[j].bbox,
                "{TEMPLATE_MISS}: {} (page {page}): columns {} and {} resolved to the same header",
                t.id,
                headers[i].key,
                headers[j].key
            );
        }
    }

    // Column x-ranges, pass 1: midpoints of the gaps between header boxes
    // (the leftmost column starts half a gap left of its header).
    let mut order: Vec<usize> = (0..headers.len()).collect();
    order.sort_by(|&a, &b| headers[a].bbox[0].total_cmp(&headers[b].bbox[0]));
    let mut cuts: Vec<f32> = order
        .windows(2)
        .map(|p| (headers[p[0]].bbox[2] + headers[p[1]].bbox[0]) / 2.0)
        .collect();
    let first_header_left = headers[order[0]].bbox[0];
    let mut first_left = {
        let h0 = headers[order[0]].bbox;
        let gap = cuts.first().map_or(20.0, |c| (c - h0[2]).max(4.0));
        h0[0] - gap
    };

    // Label column. A label header closer to the first column than half the
    // gap to the second (a narrow "Year | Total | 1 | 2 …" layout) moves the
    // label/column boundary to its right edge.
    let header_bottom = headers.iter().map(|h| h.bbox[3]).fold(anchor_y, f32::max);
    let (label_min, label_header) = match &e.label.header {
        Some(lh) => {
            let hs = hits(&ls, lh, anchor_y);
            let h = hs
                .iter()
                .filter(|h| h.bbox()[2] < first_header_left)
                .min_by(|a, b| {
                    (a.bbox()[1] - header_bottom)
                        .abs()
                        .total_cmp(&(b.bbox()[1] - header_bottom).abs())
                })
                .ok_or_else(|| miss(format!("label header {lh:?} not found")))?;
            if h.bbox()[2] > first_left + 1.0 {
                first_left = h.bbox()[2] + 1.0;
            }
            (
                h.bbox()[0] - e.label.slack,
                Some((join(&h.words), h.bbox())),
            )
        }
        None => (f32::MIN, None),
    };

    // Candidate lines: label words (the rightmost cluster left of the
    // columns, which drops group labels printed further left) and the
    // words in the column area.
    let mut start_y = header_bottom;
    if let Some(sa) = &e.rows.start_after {
        let h = hits(&ls, sa, anchor_y)
            .into_iter()
            .next()
            .ok_or_else(|| miss(format!("start_after {sa:?} not found")))?;
        start_y = start_y.max(h.bbox()[3]);
    }
    let stops: Vec<Vec<String>> = e.rows.stop_at.iter().map(|s| tokens(s)).collect();
    let min_cells = e.rows.min_cells.unwrap_or(1);
    let span_pages = e.locate.span_pages.max(1);
    let mut cand: Vec<(u32, Vec<&Word>, Vec<&Word>)> = Vec::new();
    let mut pending: Vec<&Word> = Vec::new();
    let mut other_pages: Vec<Vec<Line>> = Vec::new();
    for p in page + 1..page + span_pages {
        other_pages.push(lines(&page_words(words, p, e.superscript_ratio)));
    }
    'pages: for (pi, pls) in std::iter::once(&ls).chain(other_pages.iter()).enumerate() {
        let p = page + u32::try_from(pi)?;
        for l in pls {
            if p == page && l.y0 < start_y - 0.5 {
                continue;
            }
            if stops.iter().any(|s| !l.find(s).is_empty()) {
                break 'pages;
            }
            let label_ws: Vec<&Word> = l
                .words
                .iter()
                .copied()
                .filter(|w| w.bbox[0] >= label_min && w.cx() < first_left)
                .collect();
            let label_ws: Vec<&Word> = if e.label.all_clusters {
                label_ws
            } else {
                clusters(&label_ws, 0.8 * line_h)
                    .last()
                    .map(|c| c.to_vec())
                    .unwrap_or_default()
            };
            let col_ws: Vec<&Word> = l
                .words
                .iter()
                .copied()
                .filter(|w| w.cx() >= first_left)
                .collect();
            if !label_ws.is_empty() && !col_ws.is_empty() {
                cand.push((p, label_ws, col_ws));
                pending.clear();
            } else if e.rows.label_above {
                if col_ws.is_empty() {
                    // A label line without numbers: the start of a wrapped label.
                    pending.extend(label_ws);
                } else if !pending.is_empty() {
                    // Numbers printed below their (wrapped) label.
                    cand.push((p, std::mem::take(&mut pending), col_ws));
                }
            }
        }
    }

    // Pass 2 (numbers are right-aligned): each column's anchor is the
    // median right edge of the tokens pass 1 gave it; tokens are then
    // assigned by their right edge to the nearest anchor.
    let column_of = |x: f32, cuts: &[f32]| cuts.iter().filter(|c| x >= **c).count();
    if e.number.align == Align::Right {
        let mut edges: Vec<Vec<f32>> = vec![Vec::new(); order.len()];
        for (_, _, ws) in &cand {
            for w in ws {
                edges[column_of(w.cx(), &cuts)].push(w.bbox[2]);
            }
        }
        let anchors: Vec<f32> = edges
            .into_iter()
            .enumerate()
            .map(|(k, v)| {
                if v.is_empty() {
                    headers[order[k]].bbox[2]
                } else {
                    median(v)
                }
            })
            .collect();
        if anchors.windows(2).all(|a| a[0] < a[1]) {
            cuts = anchors.windows(2).map(|a| (a[0] + a[1]) / 2.0).collect();
        }
    }
    let mut rows = Vec::new();
    for (p, label_ws, col_ws) in cand {
        let mut cells: BTreeMap<String, Vec<&Word>> = BTreeMap::new();
        for w in col_ws {
            let x = if e.number.align == Align::Right {
                w.bbox[2]
            } else {
                w.cx()
            };
            let k = column_of(x, &cuts);
            cells
                .entry(headers[order[k]].key.clone())
                .or_default()
                .push(w);
        }
        if cells.len() < min_cells {
            continue;
        }
        rows.push(TRow {
            label: join(&label_ws),
            label_page: p,
            label_bbox: span(&label_ws),
            cells: cells
                .into_iter()
                .map(|(k, ws)| {
                    let text = join(&ws);
                    let value = e.number.parse(&text);
                    (
                        k,
                        TCell {
                            text,
                            page: p,
                            bbox: span(&ws),
                            value,
                        },
                    )
                })
                .collect(),
        });
    }
    if rows.is_empty() {
        return Err(miss("no data rows below the headers".to_owned()));
    }

    let mut all_headers = Vec::with_capacity(headers.len() + 1);
    all_headers.push(THeader {
        key: e.label.key.clone(),
        text: label_header
            .as_ref()
            .map_or_else(String::new, |h| h.0.clone()),
        page,
        bbox: label_header.map_or([0.0; 4], |h| h.1),
    });
    all_headers.extend(headers);
    Ok(Table {
        template_id: t.id.clone(),
        edition_idx: idx,
        first_page: page,
        headers: all_headers,
        rows,
    })
}

// ───────────────────────────── checks ──────────────────────────────────────

impl Table {
    /// Index of the row a reference names (`"Label"` or `"Label#n"`).
    #[must_use]
    pub fn row(&self, reference: &str) -> Option<usize> {
        let (label, nth) = match reference.rsplit_once('#') {
            Some((l, n)) if n.parse::<usize>().is_ok() => (l, n.parse::<usize>().unwrap_or(1)),
            _ => (reference, 1),
        };
        let want = norm(label);
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, r)| norm(&r.label) == want)
            .nth(nth.saturating_sub(1))
            .map(|(i, _)| i)
    }

    fn num(&self, row: usize, col: &str) -> Result<f64, String> {
        match self.rows[row].cells.get(col).map(|c| (&c.value, &c.text)) {
            Some((Value::Number(v), _)) => Ok(*v),
            Some((Value::Missing, _)) | None => Ok(0.0),
            Some((Value::Text, t)) => Err(format!(
                "{:?}/{col}: {t:?} is not a number",
                self.rows[row].label
            )),
        }
    }

    fn number_columns(&self) -> Vec<String> {
        self.headers.iter().skip(1).map(|h| h.key.clone()).collect()
    }

    /// Runs the edition's checks.
    #[must_use]
    pub fn check(&self, e: &Edition) -> Vec<CheckResult> {
        e.checks.iter().map(|c| self.check_one(c)).collect()
    }

    #[allow(clippy::too_many_lines)]
    fn check_one(&self, c: &Check) -> CheckResult {
        let mut failures = Vec::new();
        let mut cells = Vec::new();
        let mut n = 0;
        let name;
        match c {
            Check::Sum { sum, equals, tol } => {
                name = format!("sum({}) = {equals} ±{tol}", sum.join("+"));
                for (ri, r) in self.rows.iter().enumerate() {
                    if !sum.iter().chain([equals]).any(|k| r.cells.contains_key(k)) {
                        continue;
                    }
                    n += 1;
                    let got: Result<f64, String> = sum.iter().map(|k| self.num(ri, k)).sum();
                    match (got, self.num(ri, equals)) {
                        (Ok(s), Ok(t)) if (s - t).abs() <= tol + 1e-9 => {}
                        (Ok(s), Ok(t)) => {
                            failures.push(format!("{:?}: {s} vs {t} (diff {})", r.label, s - t));
                            cells.extend(
                                sum.iter()
                                    .chain([equals])
                                    .map(|k| (r.label.clone(), k.clone())),
                            );
                        }
                        (Err(m), _) | (_, Err(m)) => failures.push(m),
                    }
                }
            }
            Check::SumRows {
                sum_rows,
                equals_row,
                columns,
                tol,
            } => {
                name = format!("sum_rows → {equals_row:?} ±{tol}");
                let Some(target) = self.row(equals_row) else {
                    return CheckResult {
                        check: name,
                        ok: false,
                        n: 0,
                        failures: vec![format!("row {equals_row:?} not found")],
                        cells,
                    };
                };
                let members: Vec<usize> = match sum_rows {
                    RowSet::Named(s) => {
                        let all: Vec<usize> =
                            (0..self.rows.len()).filter(|i| *i != target).collect();
                        match s.as_str() {
                            "all" => all,
                            "all_but_first" => {
                                (1..self.rows.len()).filter(|i| *i != target).collect()
                            }
                            "all_but_last" => (0..self.rows.len().saturating_sub(1))
                                .filter(|i| *i != target)
                                .collect(),
                            other => {
                                return CheckResult {
                                    check: name,
                                    ok: false,
                                    n: 0,
                                    failures: vec![format!("unknown row set {other:?}")],
                                    cells,
                                };
                            }
                        }
                    }
                    RowSet::Labels(ls) => {
                        let mut v = Vec::new();
                        for l in ls {
                            match self.row(l) {
                                Some(i) => v.push(i),
                                None => failures.push(format!("row {l:?} not found")),
                            }
                        }
                        v
                    }
                };
                let cols = columns.clone().unwrap_or_else(|| self.number_columns());
                for col in &cols {
                    n += 1;
                    let got: Result<f64, String> = members.iter().map(|&i| self.num(i, col)).sum();
                    match (got, self.num(target, col)) {
                        (Ok(s), Ok(t)) if (s - t).abs() <= tol + 1e-9 => {}
                        (Ok(s), Ok(t)) => {
                            failures.push(format!(
                                "{col}: rows sum to {s}, {equals_row:?} is {t} (diff {})",
                                s - t
                            ));
                            cells.push((self.rows[target].label.clone(), col.clone()));
                        }
                        (Err(m), _) | (_, Err(m)) => failures.push(m),
                    }
                }
            }
            Check::RowCount { row_count } => {
                name = format!("row_count in {:?}..{:?}", row_count.min, row_count.max);
                n = 1;
                let k = self.rows.len();
                if row_count.min.is_some_and(|m| k < m) || row_count.max.is_some_and(|m| k > m) {
                    failures.push(format!("{k} rows"));
                }
            }
            Check::RowsPresent { rows_present } => {
                name = "rows_present".to_owned();
                for r in rows_present {
                    n += 1;
                    if self.row(r).is_none() {
                        failures.push(format!("row {r:?} missing"));
                    }
                }
            }
        }
        CheckResult {
            check: name,
            ok: failures.is_empty(),
            n,
            failures,
            cells,
        }
    }
}

/// Formats failed checks for an error message.
#[must_use]
pub fn failed_summary(results: &[CheckResult]) -> Option<String> {
    let bad: Vec<String> = results
        .iter()
        .filter(|r| !r.ok)
        .map(|r| {
            format!(
                "{}: {}",
                r.check,
                r.failures
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("; ")
            )
        })
        .collect();
    (!bad.is_empty()).then(|| bad.join(" | "))
}

/// Loads `registry/templates/<id>.yaml`.
pub fn load(root: &std::path::Path, id: &str) -> Result<(Template, [u8; 32])> {
    use sha2::{Digest, Sha256};
    ensure!(
        !id.contains("..")
            && !id.starts_with('/')
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_./".contains(c)),
        "bad template id {id:?}"
    );
    let path = root.join("templates").join(format!("{id}.yaml"));
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let t = Template::from_yaml(&text).with_context(|| format!("parsing {}", path.display()))?;
    if t.id != id {
        bail!("{}: id {:?} does not match its path", path.display(), t.id);
    }
    Ok((t, Sha256::digest(text.as_bytes()).into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(page: u32, text: &str, x: f32, y: f32) -> Word {
        #[allow(clippy::cast_precision_loss)]
        let width = 5.0 * text.chars().count() as f32;
        Word {
            page,
            text: text.into(),
            bbox: [x, y, x + width, y + 10.0],
            conf: None,
        }
    }

    /// A small district table with a two-level header and a superscript.
    fn sample() -> Words {
        let mut v = vec![
            w(1, "Table", 10.0, 10.0),
            w(1, "9.9", 40.0, 10.0),
            w(1, "Population", 60.0, 10.0),
            w(1, "2023", 150.0, 30.0),
            w(1, "2024(1)", 250.0, 30.0),
            w(1, "District", 20.0, 50.0),
            w(1, "Total", 120.0, 50.0),
            w(1, "Male", 160.0, 50.0),
            w(1, "Total", 230.0, 50.0),
            w(1, "Male", 270.0, 50.0),
        ];
        let rows = [
            ("Sri", "Lanka", ["30", "14", "33", "16"]),
            ("Nuwara", "- Eliya", ["10", "5", "11", "6"]),
            ("Colombo", "", ["20", "9", "22", "10"]),
        ];
        let mut y = 70.0;
        for (a, b, vals) in rows {
            v.push(w(1, a, 20.0, y));
            if !b.is_empty() {
                #[allow(clippy::cast_precision_loss)]
                let x = 20.0 + 5.0 * a.chars().count() as f32 + 3.0;
                v.push(w(1, b, x, y));
            }
            for (i, x) in [125.0, 160.0, 235.0, 270.0].into_iter().enumerate() {
                v.push(w(1, vals[i], x, y));
            }
            y += 15.0;
        }
        // A superscript next to the last value (half height): dropped.
        v.push(Word {
            page: 1,
            text: "7".into(),
            bbox: [285.0, 100.0, 288.0, 105.0],
            conf: None,
        });
        v.push(w(1, "Source:", 20.0, y + 10.0));
        v.push(w(1, "99", 125.0, y + 10.0));
        Words {
            pages: vec![lanka_extractd::PageBox {
                page: 1,
                width: 400.0,
                height: 400.0,
            }],
            words: v,
        }
    }

    const TPL: &str = r#"
id: test/pop
editions:
  - match: { edition: "2020..2025" }
    locate: { page: { find_text: "Table 9.9", then_text: "District" } }
    label: { key: district, header: "District" }
    columns:
      - { key: total_2023, header: Total, under: "2023" }
      - { key: male_2023, header: Male, under: "2023" }
      - { key: total_2024, header: Total, under: "2024" }
      - { key: male_2024, header: Male, under: "2024" }
    rows: { stop_at: ["Source:"] }
    checks:
      - { sum_rows: all_but_first, equals_row: "Sri Lanka", tol: 0 }
      - { row_count: { min: 3, max: 3 } }
      - { rows_present: ["Nuwara Eliya"] }
"#;

    #[test]
    fn edition_patterns() {
        assert!(edition_matches(None, Some("2026-Q2")));
        assert!(edition_matches(Some("2026-Q2"), Some("2026-Q2")));
        assert!(edition_matches(Some("2025..2027"), Some("2026-Q2")));
        assert!(edition_matches(Some("2025-Q2..2027-Q2"), Some("2026-Q2")));
        assert!(!edition_matches(Some("2025-Q2..2027-Q2"), Some("2026-Q1")));
        assert!(!edition_matches(Some("2025-Q2..2027-Q2"), Some("2028-Q2")));
        assert!(!edition_matches(Some("2025-Q2..2027-Q3"), Some("2026-Q2")));
        assert!(!edition_matches(Some("2025"), None));
    }

    #[test]
    fn reads_a_two_level_header_table() {
        let t = Template::from_yaml(TPL).expect("template");
        let tab = t.read(&sample(), Some("2024")).expect("read");
        assert_eq!(tab.rows.len(), 3);
        assert_eq!(tab.rows[1].label, "Nuwara - Eliya");
        assert_eq!(tab.headers[3].text, "2024(1) Total");
        assert_eq!(tab.rows[2].cells["male_2024"].text, "10");
        assert_eq!(tab.rows[0].cells["total_2023"].value, Value::Number(30.0));
        let res = tab.check(&t.editions[0]);
        assert!(res.iter().all(|r| r.ok), "{res:?}");
    }

    #[test]
    fn misses_are_template_misses_and_checks_fail_loudly() {
        let t = Template::from_yaml(TPL).expect("template");
        assert!(
            t.read(&sample(), Some("2019"))
                .expect_err("edition")
                .to_string()
                .starts_with(TEMPLATE_MISS)
        );
        let bad = TPL.replace("Table 9.9", "Table 1.1");
        let t2 = Template::from_yaml(&bad).expect("template");
        let e = t2.read(&sample(), Some("2024")).expect_err("anchor");
        assert!(format!("{e:#}").starts_with(TEMPLATE_MISS), "{e:#}");
        let mut words = sample();
        // Corrupt Colombo's 2023 total: the column sum no longer matches.
        let i = words.words.iter().position(|w| w.text == "20").expect("20");
        words.words[i].text = "21".into();
        let tab = t.read(&words, Some("2024")).expect("read");
        let res = tab.check(&t.editions[0]);
        assert!(!res[0].ok);
        assert!(
            failed_summary(&res)
                .expect("summary")
                .contains("total_2023")
        );
        assert_eq!(
            res[0].cells,
            vec![("Sri Lanka".to_owned(), "total_2023".to_owned())]
        );
    }

    /// "Year | Total | 1 | 2" with the label header close to the first
    /// column, a label wrapped above its numbers, and a month table whose
    /// year is a separate cluster.
    #[test]
    fn narrow_label_wrapped_labels_and_all_clusters() {
        let mut v = vec![
            w(1, "Table", 10.0, 10.0),
            w(1, "4.2", 40.0, 10.0),
            w(1, "Year", 20.0, 30.0),
            w(1, "Total", 45.0, 30.0),
            w(1, "1", 110.0, 30.0),
            w(1, "2", 140.0, 30.0),
        ];
        for (i, y) in ["2023", "2024"].iter().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let yy = 45.0 + 15.0 * i as f32;
            v.extend([
                w(1, y, 20.0, yy),
                w(1, "30", 65.0, yy),
                w(1, "10", 105.0, yy),
                w(1, "20", 135.0, yy),
            ]);
        }
        let words = Words {
            pages: vec![lanka_extractd::PageBox {
                page: 1,
                width: 400.0,
                height: 400.0,
            }],
            words: v,
        };
        let tpl = r#"
id: test/narrow
editions:
  - locate: { page: { find_text: "Table 4.2" } }
    label: { key: year, header: "Year" }
    columns:
      - { key: total, header: Total }
      - { key: g1, header: "1" }
      - { key: g2, header: "2" }
    checks:
      - { sum: [g1, g2], equals: total }
"#;
        let t = Template::from_yaml(tpl).expect("template");
        let tab = t.read(&words, None).expect("read");
        assert_eq!(tab.rows.len(), 2);
        assert_eq!(tab.rows[1].label, "2024");
        assert_eq!(tab.rows[0].cells["total"].text, "30");
        assert!(tab.check(&t.editions[0]).iter().all(|r| r.ok));

        // A label wrapped over two lines, numbers on a third line without one.
        let words = Words {
            pages: vec![lanka_extractd::PageBox {
                page: 1,
                width: 400.0,
                height: 400.0,
            }],
            words: vec![
                w(1, "Table", 10.0, 10.0),
                w(1, "9", 40.0, 10.0),
                w(1, "Item", 20.0, 30.0),
                w(1, "2024", 150.0, 30.0),
                w(1, "Cars", 20.0, 45.0),
                w(1, "5", 155.0, 45.0),
                w(1, "Goods", 20.0, 60.0),
                w(1, "vehicles", 20.0, 72.0),
                w(1, "7", 155.0, 84.0),
                w(1, "January", 60.0, 100.0),
                w(1, "2025", 20.0, 100.0),
                w(1, "9", 155.0, 100.0),
            ],
        };
        let wrapped = r#"
id: test/wrapped
editions:
  - locate: { page: { find_text: "Table 9" } }
    label: { key: item, header: "Item" }
    columns: [{ key: y, header: "2024" }]
    rows: { label_above: true }
"#;
        let t = Template::from_yaml(wrapped).expect("template");
        let tab = t.read(&words, None).expect("read");
        let labels: Vec<&str> = tab.rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, ["Cars", "Goods vehicles", "January"]);
        let joined = wrapped.replace(
            "header: \"Item\" }",
            "header: \"Item\", all_clusters: true }",
        );
        let t = Template::from_yaml(&joined).expect("template");
        let tab = t.read(&words, None).expect("read");
        assert_eq!(tab.rows[2].label, "2025 January");
    }

    #[test]
    fn numbers_parse_strictly() {
        let n = NumberSpec::default();
        assert_eq!(n.parse("21,919"), Value::Number(21919.0));
        assert_eq!(n.parse("-4.5"), Value::Number(-4.5));
        assert_eq!(n.parse(".."), Value::Missing);
        assert_eq!(n.parse("2,19,19"), Value::Text);
        assert_eq!(n.parse("12a"), Value::Text);
        assert_eq!(n.parse("7 . 0"), Value::Text);
        let sq = NumberSpec {
            squeeze_spaces: true,
            ..NumberSpec::default()
        };
        assert_eq!(sq.parse("7 . 0"), Value::Number(7.0));
        assert_eq!(sq.parse("35.0 50.7"), Value::Text);
        assert!(word_matches("2020(1)", "2020"));
        assert!(word_matches("Total;", "total"));
        assert!(!word_matches("Note", "no"));
    }
}
