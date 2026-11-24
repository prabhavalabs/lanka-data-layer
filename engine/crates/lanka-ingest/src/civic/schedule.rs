//! The schedule parser (13 §2.1): a pure function from the words of a gazette
//! (pages plus word boxes, as `pdf_words` returns them) to the ministers'
//! headings and their three columns.
//!
//! The layout is read from what the page itself says, never from fixed
//! coordinates, so 2010 and 2024 gazettes go through the same code:
//!
//! 1. **Header rows.** A row of the page holding `Column I`, `Column II` and
//!    `Column III` marks a table. Its x centres give the three bands; the
//!    exact cut between two bands is the widest empty vertical strip of the
//!    page body between their header centres (words of one column never cross
//!    it), falling back to the midpoint.
//! 2. **Headings.** Just above a header row, a row matching
//!    `NN. Minister of …` (`Ministry of …` in 2010) (wrapped onto further rows when long) is the
//!    minister's heading. `(Contd.)` is dropped; a heading with the number of
//!    the minister in progress continues that minister across pages. A header
//!    row without a heading above it also continues the minister in progress.
//! 3. **Columns I and II** are numbered lists. A line starts an item when its
//!    first word is `N.` (or `N)`) close to the left edge of its band; other
//!    lines wrap onto the item before them (`hydro-` + `spatial` joins
//!    without a space). A wrapped line after a blank line is an unnumbered
//!    note (for example "Institution being reviewed"), kept out of the item
//!    and reported as a warning.
//! 4. **Column III** has no numbers. Items start at a bullet when the
//!    document uses bullets; otherwise at the left edge when the page indents
//!    wrapped lines; otherwise after a line that completes an act name
//!    (`… No. 12 of 1999`).

use std::sync::LazyLock;

use lanka_extractd::{Word, Words};
use regex::Regex;
use serde::{Deserialize, Serialize};

/// One numbered line of a column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub item_no: i32,
    pub text: String,
    pub page: i32,
}

/// One minister's heading with its three columns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Portfolio {
    /// The heading number (`01.` is 1).
    pub seq: i32,
    /// `Minister of Defence`.
    pub title: String,
    pub first_page: i32,
    pub functions: Vec<Item>,
    pub institutions: Vec<Item>,
    /// Numbered 1..n in reading order (the column itself has no numbers).
    pub laws: Vec<Item>,
}

/// What the parser read.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Schedule {
    pub portfolios: Vec<Portfolio>,
    /// Pages that carried a `Column I / II / III` header row.
    pub table_pages: Vec<i32>,
    /// Things the parser skipped or could not place (never silent).
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Col {
    Function,
    Institution,
    Law,
}

// ─────────────────────────── text helpers ───────────────────────────────────

pub(super) static HEADING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\s*\(?(\d{1,3})(?:\s*[.)]\s*|\s+)(Minist(?:er|ry)\b.*)$")
        .expect("heading regex")
});
pub(super) static FOOTER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(printed\s+at\b|eog\s+\d|this\s+gazette\s+extraordinary\s+can\s+be\s+downloaded)|department\s+of\s+government\s+printing").expect("footer regex")
});
static SUBLIST_LEAD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:\(?(?:[ivxlcdm]{1,6}|[a-z])\)|(?:[ivxlcdm]{1,6}|[a-z])\.)$")
        .expect("sublist regex")
});
static CONTD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\(\s*contd\.?\s*\)").expect("contd regex"));
pub(super) static NUM_SPLIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d{1,3})[,;:]?[.)]($|[^\d].*)$").expect("number regex"));
static LAW_END: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(\bof\s+\d{4}|\(Chapter\s*\d+\)|\bamend(ed|ments?)\.?\)?)\s*[.;,)]?\s*$")
        .expect("law end regex")
});
static LAW_CONT_LEAD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(No\.?(\s|\d|$)|Nos\.?(\s|\d|$)|of\s|\(|and\s|as\s|\d)").expect("lead regex")
});

/// Words that appear in a column header (so a row made only of them is part
/// of the header, not of the body).
const HEADER_VOCAB: &[&str] = &[
    "column",
    "powers",
    "i",
    "ii",
    "iii",
    "subjects",
    "subject",
    "and",
    "functions",
    "function",
    "duties",
    "departments",
    "department",
    "statutory",
    "institutions",
    "institution",
    "intitutions",
    "public",
    "corporations",
    "corporation",
    "state",
    "laws",
    "law",
    "acts",
    "act",
    "ordinances",
    "ordinance",
    "enactments",
    "legislation",
    "to",
    "be",
    "implemented",
    "administered",
    "or",
    "of",
    "the",
    "",
];

const BULLETS: &[char] = &['•', '·', '●', '▪', '■', '◦', '○', '‣', '∙', '*'];

pub(super) fn is_bullet_token(t: &str) -> bool {
    let mut it = t.chars();
    match (it.next(), it.next()) {
        (Some(c), None) => {
            BULLETS.contains(&c) || ('\u{e000}'..='\u{f8ff}').contains(&c) || c == '\u{f0b7}'
        }
        _ => false,
    }
}

fn starts_lower(s: &str) -> bool {
    s.chars().next().is_some_and(char::is_lowercase)
}

fn ends_alpha(s: &str) -> bool {
    s.chars().next_back().is_some_and(char::is_alphabetic)
}

/// Words of one row to text. Fragments the PDF printed with no space at all
/// ("effi" + "cient", the `fi` ligature) are glued back together.
pub(super) fn join_words(ws: &[&Word]) -> String {
    let mut s = String::new();
    let mut prev: Option<&Word> = None;
    for w in ws {
        if let Some(p) = prev {
            let gap = w.bbox[0] - p.bbox[2];
            let glue = gap < 0.5 && starts_lower(&w.text) && ends_alpha(&p.text);
            if !glue {
                s.push(' ');
            }
        }
        s.push_str(&w.text);
        prev = Some(w);
    }
    s
}

/// Appends a wrapped line. A line ending in `-` before a lower-case letter
/// is a word split by the line break: `Authori-` + `ty` is `Authority`. Before
/// a capital the hyphen is a real one (`Kotelawala-` + `Defence`).
pub(super) fn append(text: &mut String, next: &str) {
    if next.is_empty() {
        return;
    }
    if text.is_empty() {
        text.push_str(next);
        return;
    }
    let after_letter =
        text.ends_with('-') && text.chars().rev().nth(1).is_some_and(char::is_alphabetic);
    if after_letter && starts_lower(next) {
        // a word split by the line break (`Authori-` `ty`): the hyphen was the break's
        text.pop();
    } else if !(after_letter && next.chars().next().is_some_and(char::is_uppercase)) {
        // (a hyphen before a capital is a real one: `Kotelawala-` `Defence`, no space)
        text.push(' ');
    }
    text.push_str(next);
}

pub(super) fn clean(s: &str) -> String {
    let visible: String = s
        .chars()
        .filter(|c| !matches!(c, '\u{200b}'..='\u{200d}' | '\u{2060}' | '\u{feff}'))
        .collect();
    visible.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Does `text` read as a finished act name (so the next line starts a new one)?
#[must_use]
pub fn law_complete(text: &str) -> bool {
    LAW_END.is_match(text.trim_end())
}

// ─────────────────────────── geometry ───────────────────────────────────────

pub(super) struct Row<'a> {
    pub(super) cy: f32,
    pub(super) y0: f32,
    pub(super) y1: f32,
    pub(super) words: Vec<&'a Word>,
}

impl Row<'_> {
    pub(super) fn text(&self) -> String {
        join_words(&self.words)
    }
}

pub(super) fn rows<'a>(ws: impl IntoIterator<Item = &'a Word>) -> Vec<Row<'a>> {
    let mut v: Vec<&Word> = ws.into_iter().collect();
    v.sort_by(|a, b| {
        a.cy()
            .total_cmp(&b.cy())
            .then(a.bbox[0].total_cmp(&b.bbox[0]))
    });
    let mut out: Vec<Row<'a>> = Vec::new();
    for w in v {
        let tol = 0.45 * w.height().max(6.0);
        match out.last_mut() {
            Some(r) if (w.cy() - r.cy).abs() <= tol => {
                r.y0 = r.y0.min(w.bbox[1]);
                r.y1 = r.y1.max(w.bbox[3]);
                r.words.push(w);
            }
            _ => out.push(Row {
                cy: w.cy(),
                y0: w.bbox[1],
                y1: w.bbox[3],
                words: vec![w],
            }),
        }
    }
    for r in &mut out {
        r.words.sort_by(|a, b| a.bbox[0].total_cmp(&b.bbox[0]));
    }
    out
}

/// Header centres that are not roughly evenly spaced (a header printed out of
/// place on one page) cannot be trusted.
fn centres_plausible(c: [f32; 3]) -> bool {
    let (a, b) = (c[1] - c[0], c[2] - c[1]);
    a > 20.0 && b > 20.0 && (0.5..=2.0).contains(&(a / b))
}

/// `(centre x of "Column I", II, III)` when the row is a column header row.
fn header_centres(r: &Row<'_>) -> Option<[f32; 3]> {
    let mut c = [None::<f32>; 3];
    for pair in r.words.windows(2) {
        if pair[0].text.eq_ignore_ascii_case("column") {
            let idx = match pair[1].text.as_str() {
                "I" => 0,
                "II" => 1,
                "III" => 2,
                _ => continue,
            };
            c[idx] = Some((pair[0].bbox[0] + pair[1].bbox[2]) / 2.0);
        }
    }
    match c {
        [Some(a), Some(b), Some(d)] if a < b && b < d => Some([a, b, d]),
        _ => None,
    }
}

pub(super) fn is_header_vocab(r: &Row<'_>) -> bool {
    r.words.iter().all(|w| {
        let t: String = w
            .text
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
            .to_lowercase();
        HEADER_VOCAB.contains(&t.as_str())
    })
}

/// Cuts between the bands. The header is centred over its column, so the
/// midpoint of two header centres lies near the cell boundary; the cut is the
/// middle of the empty vertical strip (no body word covers it) nearest to that
/// midpoint, or the midpoint itself when no strip is close.
fn boundaries(centres: [f32; 3], body: &[&Word]) -> [f32; 2] {
    let mut iv: Vec<(f32, f32)> = body.iter().map(|w| (w.bbox[0], w.bbox[2])).collect();
    iv.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut gaps: Vec<(f32, f32)> = Vec::new(); // (start, end)
    if let (Some(first), Some(last)) = (iv.first(), iv.iter().map(|v| v.1).max_by(f32::total_cmp)) {
        // the margins count as gaps: a page with one column filled still cuts after it
        gaps.push((first.0 - 30.0, first.0));
        gaps.push((last, last + 30.0));
    }
    let mut reach = f32::NEG_INFINITY;
    for (x0, x1) in iv {
        if reach.is_finite() && x0 > reach {
            gaps.push((reach, x0));
        }
        reach = reach.max(x1);
    }
    let cut = |a: f32, b: f32| -> f32 {
        let mid = (a + b) / 2.0;
        let dist = |g: &(f32, f32)| {
            if mid < g.0 {
                g.0 - mid
            } else if mid > g.1 {
                mid - g.1
            } else {
                0.0
            }
        };
        gaps.iter()
            .filter(|g| (g.1 - g.0) >= 2.0 && dist(g) <= 0.2 * (b - a))
            .min_by(|x, y| {
                dist(x)
                    .total_cmp(&dist(y))
                    .then((y.1 - y.0).total_cmp(&(x.1 - x.0)))
            })
            .map_or(mid, |(s, e)| (s + e) / 2.0)
    };
    [cut(centres[0], centres[1]), cut(centres[1], centres[2])]
}

// ─────────────────────────── pass 1: segments ───────────────────────────────

#[derive(Debug, Clone)]
struct Line {
    page: u32,
    x0: f32,
    /// Text of the first word (`12.`, `•`).
    lead: String,
    text: String,
}

struct Seg {
    page: u32,
    heading: Option<(i32, String)>,
    bands: [Vec<Line>; 3],
}

/// The lines of one band. In the numbered columns a bullet in the margin is
/// not part of the line (`keep_bullets` is for column III).
fn band_lines(page: u32, ws: &[&Word], keep_bullets: bool) -> Vec<Line> {
    rows(ws.iter().copied())
        .into_iter()
        .filter_map(|mut r| {
            if !keep_bullets {
                while r.words.len() > 1 && is_bullet_token(&r.words[0].text) {
                    r.words.remove(0);
                }
            }
            if FOOTER.is_match(&r.text()) {
                return None;
            }
            Some(Line {
                page,
                x0: r.words[0].bbox[0],
                lead: r.words[0].text.clone(),
                text: r.text(),
            })
        })
        .collect()
}

/// A row that opens a minister's heading: `NN. Minister …` (or `(N) Minister …`)
/// at the left, numbered as the minister in progress (a `(Contd.)` heading) or
/// the next one. A new number must be followed by the new list starting at 1:
/// that keeps an item such as `2. Minister of X to be consulted` out.
fn heading_match(rs: &[Row<'_>], i: usize, cur: Option<i32>) -> Option<i32> {
    let r = &rs[i];
    if r.words[0].bbox[0] >= 200.0 {
        return None;
    }
    let text = r.text();
    let n: i32 = HEADING.captures(&text)?[1].parse().ok()?;
    match cur {
        Some(c) if n == c => Some(n),
        Some(c) if n != c + 1 => None,
        _ => {
            // the first numbered row after the title (and header) must be item 1
            let first = rs.iter().skip(i + 1).take(14).find_map(|r| {
                let w = r.words.first()?;
                // `1.`, `1.Formulation`, or a bare `1` (a full stop some gazettes leave out)
                if let Some(c) = NUM_SPLIT.captures(&w.text) {
                    return c[1].parse::<i32>().ok();
                }
                (w.text.len() <= 3 && w.text.chars().all(|c| c.is_ascii_digit()))
                    .then(|| w.text.parse::<i32>().ok())
                    .flatten()
            });
            first.is_none_or(|f| f == 1).then_some(n)
        }
    }
}

fn clean_title(raw: &str) -> String {
    let t = clean(&CONTD.replace_all(raw, ""));
    t.trim_end_matches(['.', ',', ':', ';']).trim().to_owned()
}

/// Index of the first row after `from` that is not part of the header block
/// (rows made only of header vocabulary), and the block's bottom edge.
fn skip_header(rs: &[Row<'_>], hidx: usize, limit: usize) -> (usize, f32) {
    let mut bottom = rs[hidx].y1;
    let mut i = hidx + 1;
    while i < limit && i <= hidx + 6 && is_header_vocab(&rs[i]) {
        bottom = bottom.max(rs[i].y1);
        i += 1;
    }
    (i, bottom)
}

fn segments(words: &Words, out: &mut Schedule) -> Vec<Seg> {
    struct Start {
        row: usize,
        heading: Option<(i32, String)>,
    }
    let mut segs = Vec::new();
    let mut cur_seq: Option<i32> = None;
    let mut carried: Option<[f32; 3]> = None;
    for pb in &words.pages {
        let page = pb.page;
        let pw: Vec<&Word> = words.on_page(page).collect();
        let rs = rows(pw.iter().copied());
        let hdr: Vec<Option<[f32; 3]>> = rs.iter().map(header_centres).collect();
        // heading rows, in reading order, numbered as the running count says
        let mut starts: Vec<Start> = Vec::new();
        let mut seq = cur_seq;
        for i in 0..rs.len() {
            if let Some(n) = heading_match(&rs, i, seq) {
                seq = Some(n);
                starts.push(Start {
                    row: i,
                    heading: Some((n, String::new())),
                });
            }
        }
        let has_header = hdr.iter().any(Option::is_some);
        if starts.is_empty() && !has_header {
            continue;
        }
        // a table that opens the page before any heading continues the minister in progress
        let first_header = hdr.iter().position(Option::is_some);
        if first_header.is_some_and(|h| starts.first().is_none_or(|s| h < s.row)) {
            starts.insert(
                0,
                Start {
                    row: first_header.unwrap_or(0),
                    heading: None,
                },
            );
        }
        if has_header {
            out.table_pages.push(page_i32(page));
        }
        let page_centres = hdr.iter().flatten().next().copied().or(carried);
        let limit = pb.height * 0.97;

        struct Tab {
            heading: Option<(i32, String)>,
            centres: Option<[f32; 3]>,
            top: f32,
            end: f32,
        }
        let mut tabs: Vec<Tab> = Vec::new();
        for (k, st) in starts.iter().enumerate() {
            let next_row = starts.get(k + 1).map_or(rs.len(), |n| n.row);
            let mut i = st.row;
            let mut heading = None;
            let mut top = rs[st.row].y0;
            if let Some((n, _)) = &st.heading {
                // the heading's own row, then wrapped title rows kept in column I
                let mut title = rs[i].text();
                let mut y1 = rs[i].y1;
                i += 1;
                let mut wrapped = 0;
                while i < next_row
                    && wrapped < 4
                    && hdr[i].is_none()
                    && !is_header_vocab(&rs[i])
                    && NUM_SPLIT.captures(&rs[i].words[0].text).is_none()
                    && rs[i].y0 - y1 <= 1.4 * (rs[i].y1 - rs[i].y0).max(6.0)
                {
                    title.push(' ');
                    title.push_str(&rs[i].text());
                    y1 = rs[i].y1;
                    i += 1;
                    wrapped += 1;
                }
                let caps = HEADING.captures(&title);
                heading = caps.map(|c| (*n, clean_title(&c[2])));
                top = y1;
            }
            let mut centres = page_centres;
            // the table's own header rows, if any, before the body
            if let Some(h) = (i..next_row).find(|&j| hdr[j].is_some()) {
                // only rows between the title and the header may be skipped
                if (i..h).all(|j| is_header_vocab(&rs[j]) || rs[j].words.len() <= 3) {
                    centres = hdr[h];
                    let (_, bottom) = skip_header(&rs, h, next_row);
                    top = bottom;
                }
            }
            // a header printed out of place: keep the bands of the page before
            if let (Some(c), Some(prev)) = (centres, carried)
                && !centres_plausible(c)
                && centres_plausible(prev)
            {
                out.warnings.push(format!(
                    "page {page}: column headers out of place; the bands of the page before are used"
                ));
                centres = Some(prev);
            }
            let end = starts.get(k + 1).map_or(f32::INFINITY, |n| rs[n.row].y0);
            tabs.push(Tab {
                heading,
                centres,
                top,
                end,
            });
        }
        let body_of = |t: &Tab| -> Vec<&Word> {
            pw.iter()
                .copied()
                .filter(|w| w.cy() > t.top - 0.5 && w.cy() < t.end - 0.5 && w.bbox[1] < limit)
                .collect()
        };
        let all_body: Vec<&Word> = tabs.iter().flat_map(&body_of).collect();
        for t in &tabs {
            if let Some((n, _)) = &t.heading {
                cur_seq = Some(*n);
            }
            let Some(centres) = t.centres else {
                if body_of(t).is_empty() {
                    continue;
                }
                out.warnings.push(format!(
                    "page {page}: text under a heading with no column header to read the bands from"
                ));
                continue;
            };
            carried = Some(centres);
            let cut = boundaries(centres, &all_body);
            let body = body_of(t);
            let band_of = |x: f32| usize::from(x >= cut[0]) + usize::from(x >= cut[1]);
            let mut by: [Vec<&Word>; 3] = [Vec::new(), Vec::new(), Vec::new()];
            for w in body.iter().filter(|w| !is_bullet_token(&w.text)) {
                by[band_of(w.bbox[0])].push(w);
            }
            // a bullet belongs to the band of the word it introduces
            for w in body.iter().filter(|w| is_bullet_token(&w.text)) {
                let next = body
                    .iter()
                    .filter(|n| {
                        !is_bullet_token(&n.text)
                            && n.bbox[0] >= w.bbox[2]
                            && n.bbox[0] - w.bbox[2] < 30.0
                            && (n.cy() - w.cy()).abs() < 8.0
                    })
                    .min_by(|x, y| x.bbox[0].total_cmp(&y.bbox[0]));
                by[next.map_or_else(|| band_of(w.bbox[0]), |n| band_of(n.bbox[0]))].push(w);
            }
            segs.push(Seg {
                page,
                heading: t.heading.clone(),
                bands: [
                    band_lines(page, &by[0], false),
                    band_lines(page, &by[1], false),
                    band_lines(page, &by[2], true),
                ],
            });
        }
    }
    segs
}

// ─────────────────────────── pass 2: items ──────────────────────────────────

struct Open {
    no: i32,
    text: String,
    page: u32,
    /// After an unnumbered note: further unnumbered lines are part of the note.
    note: bool,
}

struct Builder {
    cur: Option<Portfolio>,
    open: [Option<Open>; 3],
    done: Vec<Portfolio>,
    warnings: Vec<String>,
}

fn idx(c: Col) -> usize {
    match c {
        Col::Function => 0,
        Col::Institution => 1,
        Col::Law => 2,
    }
}

pub(super) fn page_i32(p: u32) -> i32 {
    i32::try_from(p).unwrap_or(i32::MAX)
}

impl Builder {
    fn flush(&mut self, c: Col) {
        let Some(o) = self.open[idx(c)].take() else {
            return;
        };
        let Some(p) = self.cur.as_mut() else { return };
        let it = Item {
            item_no: o.no,
            text: clean(&o.text),
            page: page_i32(o.page),
        };
        match c {
            Col::Function => p.functions.push(it),
            Col::Institution => p.institutions.push(it),
            Col::Law => p.laws.push(it),
        }
    }

    fn flush_all(&mut self) {
        for c in [Col::Function, Col::Institution, Col::Law] {
            self.flush(c);
        }
    }

    fn finish_portfolio(&mut self) {
        self.flush_all();
        if let Some(mut p) = self.cur.take() {
            for (i, l) in p.laws.iter_mut().enumerate() {
                l.item_no = i32::try_from(i + 1).unwrap_or(i32::MAX);
            }
            self.done.push(p);
        }
    }

    fn start(&mut self, c: Col, no: i32, text: &str, line: &Line) {
        self.flush(c);
        self.open[idx(c)] = Some(Open {
            no,
            text: text.to_owned(),
            page: line.page,
            note: false,
        });
    }

    fn cont(&mut self, c: Col, text: &str) {
        if let Some(o) = self.open[idx(c)].as_mut() {
            append(&mut o.text, text);
        }
    }

    fn warn(&mut self, msg: String) {
        if self.warnings.len() < 200 {
            self.warnings.push(msg);
        }
    }

    fn numbered(&mut self, c: Col, lines: &[Line]) {
        // the numbers anchor the left edge (a stray bullet in the margin must not)
        let numbered_min = lines
            .iter()
            .filter(|l| NUM_SPLIT.is_match(&l.lead))
            .map(|l| l.x0)
            .min_by(f32::total_cmp);
        let Some(left) = numbered_min.or_else(|| lines.iter().map(|l| l.x0).min_by(f32::total_cmp))
        else {
            return;
        };
        // wrapped lines hang under the text, not under the number: a line
        // back at the number's edge is a note, not a wrapped line
        let hanging = lines
            .iter()
            .any(|l| !NUM_SPLIT.is_match(&l.lead) && l.x0 > left + 8.0);
        for (n, line) in lines.iter().enumerate() {
            let expected = Some(self.open[idx(c)].as_ref().map_or(1, |o| o.no + 1));
            let split = number_start(line, left, expected);
            if let Some((no, rest)) = split {
                self.start(c, no, &rest, line);
                continue;
            }
            let Some((o_no, o_note)) = self.open[idx(c)].as_ref().map(|o| (o.no, o.note)) else {
                self.warn(format!(
                    "page {}: unnumbered text before the first item: {}",
                    line.page,
                    snippet(&line.text)
                ));
                continue;
            };
            // `i.`, `ii.`, `(a)`: a list inside an item belongs to it
            let sublist = SUBLIST_LEAD.is_match(&line.lead);
            // the first line of a band continues the item of the page before
            let at_edge = n > 0 && hanging && line.x0 <= left + 3.0 && !sublist;
            if o_note || at_edge {
                if !o_note {
                    if let Some(o) = self.open[idx(c)].as_mut() {
                        o.note = true;
                    }
                    self.warn(format!(
                        "page {}: unnumbered note after item {o_no}: {}",
                        line.page,
                        snippet(&line.text)
                    ));
                }
                continue;
            }
            let text = strip_bullet(line).1;
            self.cont(c, &text);
        }
    }

    fn laws(&mut self, lines: &[Line]) {
        let Some(left) = lines.iter().map(|l| l.x0).min_by(f32::total_cmp) else {
            return;
        };
        let indented = lines.iter().any(|l| l.x0 > left + 4.0);
        // a page uses bullets or it does not (gazettes mix both)
        let bulleted = lines.iter().filter(|l| is_bullet_token(&l.lead)).count();
        let bullets = bulleted > 0 && bulleted * 10 >= lines.len() * 3;
        for line in lines {
            let (bullet, text) = strip_bullet(line);
            let open_text = self.open[idx(Col::Law)].as_ref().map(|o| o.text.clone());
            let new = if bullets {
                bullet || open_text.is_none()
            } else if indented {
                line.x0 <= left + 2.0 || open_text.is_none()
            } else {
                match &open_text {
                    None => true,
                    Some(t) => law_complete(t) && !LAW_CONT_LEAD.is_match(&text),
                }
            };
            if new {
                self.start(Col::Law, 0, &text, line);
            } else {
                self.cont(Col::Law, &text);
            }
        }
    }

    fn feed(&mut self, seg: &Seg) {
        if seg.heading.is_none() && seg.bands.iter().all(Vec::is_empty) {
            return;
        }
        match &seg.heading {
            Some((seq, title)) => {
                if self.cur.as_ref().map(|p| p.seq) != Some(*seq) {
                    self.finish_portfolio();
                    if self.done.iter().any(|p| p.seq == *seq) {
                        self.warn(format!(
                            "page {}: heading number {seq} appears again",
                            seg.page
                        ));
                    }
                    self.cur = Some(Portfolio {
                        seq: *seq,
                        title: title.clone(),
                        first_page: page_i32(seg.page),
                        functions: Vec::new(),
                        institutions: Vec::new(),
                        laws: Vec::new(),
                    });
                }
            }
            None => {
                // the cover page repeats the column header before any minister
                if self.cur.is_none() {
                    return;
                }
            }
        }
        self.numbered(Col::Function, &seg.bands[0]);
        self.numbered(Col::Institution, &seg.bands[1]);
        self.laws(&seg.bands[2]);
    }
}

/// The number a lead word reads as, with the slips the gazettes print:
/// `` `7. `` (a stray mark in front), `3l.` for `31.` (a letter for a digit; read
/// only when it is the number expected next), `23,.`, and a number without
/// its full stop (also only when expected).
fn lead_number(lead: &str, expected: Option<i32>) -> Option<(i32, String)> {
    let stripped = lead.trim_start_matches(['`', '\'', '‘', '’', '"', '*', '·', '•']);
    if let Some(c) = NUM_SPLIT.captures(stripped) {
        let no: i32 = c[1].parse().ok()?;
        // a leading mark is forgiven only for the number expected
        if stripped.len() != lead.len() && Some(no) != expected {
            return None;
        }
        return Some((no, c.get(2).map_or("", |m| m.as_str()).to_owned()));
    }
    // a bare number, or a comma for the full stop (`15, Water management`)
    if let Ok(no) = stripped.trim_end_matches([',', ';', ':']).parse::<i32>() {
        return (Some(no) == expected).then(|| (no, String::new()));
    }
    // letters read as digits: `3l.`, `1O.`
    let body = stripped.strip_suffix(['.', ')'])?;
    if body.len() > 3 || !body.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    let mapped: String = body
        .chars()
        .map(|c| match c {
            'l' | 'I' => '1',
            'O' | 'o' => '0',
            c => c,
        })
        .collect();
    let no: i32 = mapped.parse().ok()?;
    (Some(no) == expected && mapped != body).then(|| (no, String::new()))
}

/// `(number, rest of the line)` when `line` starts a numbered item.
fn number_start(line: &Line, left: f32, expected: Option<i32>) -> Option<(i32, String)> {
    if line.x0 > left + 12.0 {
        return None;
    }
    let (no, glued) = lead_number(&line.lead, expected)?;
    // the lead word's own text after the number, then the rest of the line
    let after_lead = line
        .text
        .strip_prefix(line.lead.as_str())
        .unwrap_or("")
        .trim_start();
    let mut rest = glued;
    append(&mut rest, after_lead);
    // a bare number needs something after it
    (!rest.is_empty()).then_some((no, rest))
}

fn strip_bullet(line: &Line) -> (bool, String) {
    if is_bullet_token(&line.lead) {
        let rest = line
            .text
            .strip_prefix(line.lead.as_str())
            .unwrap_or("")
            .trim_start()
            .to_owned();
        (true, rest)
    } else {
        (false, line.text.clone())
    }
}

pub(super) fn snippet(s: &str) -> String {
    s.chars().take(60).collect()
}

/// Parses every table of the document (the three-column layout; see
/// [`super::schedule_b`] for the 2020 policy-statement layout).
#[must_use]
pub fn parse(words: &Words) -> Schedule {
    let mut out = Schedule::default();
    let segs = segments(words, &mut out);
    let mut b = Builder {
        cur: None,
        open: [None, None, None],
        done: Vec::new(),
        warnings: Vec::new(),
    };
    for s in &segs {
        b.feed(s);
    }
    b.finish_portfolio();
    out.portfolios = b.done;
    out.warnings.extend(b.warnings);
    out
}

#[cfg(test)]
#[path = "schedule_tests.rs"]
mod tests;
