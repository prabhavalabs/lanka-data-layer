//! The second layout (Gazette Extraordinary 2187/27, 2020-08-09): a policy
//! document, not three columns. Each Cabinet minister has a section
//! `N.0 Minister of …` with numbered parts (`Purview`, `Subjects and
//! Functions`, `Special Priorities`, `Related Institutional and Legal
//! Framework`), followed by the State Ministers under him (`N.k State Minister
//! of …`) with the same parts.
//!
//! Read into the same model as the table layout:
//! - **functions**: the numbered `Subjects and Functions` of the Cabinet
//!   minister (a State Minister's are his own and are left out);
//! - **institutions**: the lists of the `Related Institutional and Legal
//!   Framework` table of the Cabinet minister and, appended, of his State
//!   Ministers (their institutions are in the ministry's charge). The left
//!   band holds either numbered institutions (`1. Sri Lanka Police`) or
//!   numbered groups (`1. Treasury Affairs`) of roman-numbered institutions
//!   (`i. General Treasury`); groups are labels, not institutions. They are
//!   numbered 1..n in reading order;
//! - **laws**: the bullets of the right band.

use lanka_extractd::{Word, Words};
use regex::Regex;
use std::sync::LazyLock;

use super::schedule::{
    Item, NUM_SPLIT, Portfolio, Schedule, append, clean, is_bullet_token, is_header_vocab,
    join_words, law_complete, page_i32, rows, snippet,
};

static CABINET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\s*(\d{1,2})\.0\s+((?:Foreign\s+)?Min(?:i)?st(?:er|ry)\b.*)$")
        .expect("cabinet regex")
});
static STATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^\s*\d{1,2}\.[1-9]\s+State\s+Minist").expect("state regex"));
static SECTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\s*\d\.\s*(Purview|Subjects\s+and\s+Functions|Special\s+Priorities|Rel\w+\s+Institution)")
        .expect("section regex")
});
static CONTENTS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s\d{1,3}\s*$").expect("contents regex"));
static ROMAN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[ivxlc]{1,6}\.$").expect("roman regex"));
static RUNNING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)GAZETTE\s+EXTRAORDINARY|fldgi|^\s*\d+A\s*$|^PART\s+I\b")
        .expect("running regex")
});

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Before the first heading, or in a part that is not read.
    Skip,
    Functions,
    Table,
}

struct Open {
    no: i32,
    text: String,
    page: u32,
}

struct P {
    out: Portfolio,
    fun: Option<Open>,
    inst: Option<Open>,
    law: Option<Open>,
}

impl P {
    fn flush_all(&mut self) {
        if let Some(o) = self.fun.take() {
            self.out.functions.push(item(o));
        }
        if let Some(o) = self.inst.take() {
            self.out.institutions.push(item(o));
        }
        if let Some(o) = self.law.take() {
            self.out.laws.push(item(o));
        }
    }

    fn start_inst(&mut self, text: &str, page: u32) {
        if let Some(o) = self.inst.take() {
            self.out.institutions.push(item(o));
        }
        let no = i32::try_from(self.out.institutions.len() + 1).unwrap_or(i32::MAX);
        self.inst = Some(Open {
            no,
            text: text.to_owned(),
            page,
        });
    }

    fn start_law(&mut self, text: &str, page: u32) {
        if let Some(o) = self.law.take() {
            self.out.laws.push(item(o));
        }
        let no = i32::try_from(self.out.laws.len() + 1).unwrap_or(i32::MAX);
        self.law = Some(Open {
            no,
            text: text.to_owned(),
            page,
        });
    }
}

fn item(o: Open) -> Item {
    Item {
        item_no: o.no,
        text: clean(&o.text),
        page: page_i32(o.page),
    }
}

/// The left-band words and the right-band words of a row, cut at `cut`.
fn split<'a>(words: &[&'a Word], cut: f32) -> (Vec<&'a Word>, Vec<&'a Word>) {
    let (mut l, mut r) = (Vec::new(), Vec::new());
    for w in words {
        if w.bbox[0] < cut {
            l.push(*w)
        } else {
            r.push(*w)
        }
    }
    (l, r)
}

/// A row that ends a table: a heading or the start of another part.
fn is_boundary(text: &str) -> bool {
    (CABINET.is_match(text) || STATE.is_match(text)) && !CONTENTS.is_match(text)
        || SECTION.is_match(text)
}

/// The x of `Laws` when the row is a table's header (`Departments, Statutory
/// Institutions … Laws and Ordinance …`, or only the laws' half of it).
fn table_header_laws_x(r: &super::schedule::Row<'_>, text: &str) -> Option<f32> {
    let x = r
        .words
        .iter()
        .find(|w| w.text.eq_ignore_ascii_case("Laws"))
        .map(|w| w.bbox[0])?;
    (text.contains("Departments") || text.contains("Ordinance")).then_some(x)
}

/// The cut between the institutions and the laws of the table whose header is
/// row `h`, and whether its laws carry bullets. The laws' bullets mark the
/// band; without them, the widest empty strip between the bands just left of
/// the (centred) `Laws` header. The rows read are those down to the table's end.
fn region_cut(rs: &[super::schedule::Row<'_>], h: usize, laws_x: f32) -> (f32, bool) {
    let mut region: Vec<&Word> = Vec::new();
    for r in rs.iter().skip(h + 1) {
        if is_boundary(&r.text()) {
            break;
        }
        region.extend(r.words.iter().copied());
    }
    let bullet = region
        .iter()
        .filter(|w| is_bullet_token(&w.text) && w.bbox[0] > 150.0)
        .map(|w| w.bbox[0])
        .min_by(f32::total_cmp);
    if let Some(x) = bullet {
        return (x - 3.0, true);
    }
    let mut iv: Vec<(f32, f32)> = region
        .iter()
        .filter(|w| w.bbox[0] > 150.0)
        .map(|w| (w.bbox[0], w.bbox[2]))
        .collect();
    iv.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut best: Option<(f32, f32)> = None; // (width, mid)
    let mut reach = f32::NEG_INFINITY;
    for (x0, x1) in iv {
        if reach.is_finite() && x0 - reach >= 2.0 {
            let mid = (reach + x0) / 2.0;
            if mid >= laws_x - 80.0
                && mid <= laws_x + 5.0
                && best.is_none_or(|(w, _)| x0 - reach > w)
            {
                best = Some((x0 - reach, mid));
            }
        }
        reach = reach.max(x1);
    }
    (best.map_or(laws_x - 4.0, |(_, m)| m), false)
}

/// Parses the policy-statement layout.
#[must_use]
pub fn parse(words: &Words) -> Schedule {
    let mut out = Schedule::default();
    let mut ps: Vec<P> = Vec::new();
    let mut mode = Mode::Skip;
    let mut in_state = false;
    let mut carry: Option<(f32, bool)> = None;
    let mut fun_left: Option<f32> = None;
    let mut group_wrap = false;
    let mut warnings: Vec<String> = Vec::new();

    for pb in &words.pages {
        let page = pb.page;
        let rs = rows(words.on_page(page).filter(|w| w.bbox[1] < pb.height * 0.97));
        let mut table_page = false;
        // the cut between a table's bands, from its own rows (see `region_cut`)
        let mut cuts: Vec<Option<(f32, bool)>> = vec![None; rs.len()];
        let mut region = carry;
        for (i, r) in rs.iter().enumerate() {
            let t = r.text();
            if is_boundary(&t) {
                region = None;
            } else if let Some(x) = table_header_laws_x(r, &t) {
                region = Some(region_cut(&rs, i, x));
            }
            cuts[i] = region;
        }
        carry = region;
        // the rows of this page's tables, for the group look-ahead
        let left_leads: Vec<Option<String>> = rs
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let (c, _) = cuts[i]?;
                let (l, _) = split(&r.words, c);
                l.first().map(|w| w.text.clone())
            })
            .collect();
        for (i, r) in rs.iter().enumerate() {
            let text = r.text();
            if RUNNING.is_match(&text) || super::schedule::FOOTER.is_match(&text) {
                continue;
            }
            // the contents pages list every heading with its page number
            if (CABINET.is_match(&text) || STATE.is_match(&text)) && CONTENTS.is_match(&text) {
                continue;
            }
            if let Some(c) = CABINET.captures(&text) {
                if let Some(p) = ps.last_mut() {
                    p.flush_all();
                }
                let seq: i32 = c[1].parse().unwrap_or(0);
                let mut title = clean(&c[2]);
                if title.to_lowercase().starts_with("minster") {
                    title = format!("Minister{}", &title["Minster".len()..]);
                }
                ps.push(P {
                    out: Portfolio {
                        seq,
                        title,
                        first_page: page_i32(page),
                        functions: Vec::new(),
                        institutions: Vec::new(),
                        laws: Vec::new(),
                    },
                    fun: None,
                    inst: None,
                    law: None,
                });
                mode = Mode::Skip;
                in_state = false;
                fun_left = None;
                group_wrap = false;
                continue;
            }
            if STATE.is_match(&text) {
                if let Some(p) = ps.last_mut() {
                    p.flush_all();
                }
                mode = Mode::Skip;
                in_state = true;
                group_wrap = false;
                continue;
            }
            if let Some(c) = SECTION.captures(&text) {
                if let Some(p) = ps.last_mut() {
                    p.flush_all();
                }
                let name = c[1].to_lowercase();
                mode = if name.starts_with("subjects") && !in_state {
                    fun_left = None;
                    Mode::Functions
                } else if name.starts_with("rel") {
                    group_wrap = false;
                    Mode::Table
                } else {
                    Mode::Skip
                };
                continue;
            }
            let Some(p) = ps.last_mut() else { continue };
            match mode {
                Mode::Skip => {}
                Mode::Functions => {
                    let lead = r.words[0].text.as_str();
                    let left = *fun_left.get_or_insert(r.words[0].bbox[0]);
                    let numbered = NUM_SPLIT
                        .captures(lead)
                        .filter(|_| r.words[0].bbox[0] <= left + 12.0);
                    if let Some(c) = numbered {
                        if let Some(o) = p.fun.take() {
                            p.out.functions.push(item(o));
                        }
                        let no: i32 = c[1].parse().unwrap_or(0);
                        let mut rest = c.get(2).map_or("", |m| m.as_str()).to_owned();
                        let after = text.strip_prefix(lead).unwrap_or("").trim_start();
                        append(&mut rest, after);
                        p.fun = Some(Open {
                            no,
                            text: rest,
                            page,
                        });
                    } else if let Some(o) = p.fun.as_mut() {
                        append(&mut o.text, &text);
                    }
                }
                Mode::Table => {
                    // the table's header row; its cut was fixed from the table's rows above
                    if table_header_laws_x(r, &text).is_some() {
                        table_page = true;
                        continue;
                    }
                    if is_header_vocab(r) {
                        continue;
                    }
                    let Some((c, has_bullets)) = cuts[i] else {
                        continue;
                    };
                    let (l, rt) = split(&r.words, c);
                    // laws: bullets
                    if let Some(first) = rt.first() {
                        let t = join_words(&rt);
                        if is_bullet_token(&first.text) {
                            let rest = t
                                .strip_prefix(first.text.as_str())
                                .unwrap_or("")
                                .trim_start();
                            p.start_law(rest, page);
                        } else if !has_bullets
                            && p.law.as_ref().is_none_or(|o| law_complete(&o.text))
                            && !t.to_lowercase().starts_with("no.")
                        {
                            // no bullets on this page: an act name ends where it says it does
                            p.start_law(&t, page);
                        } else if let Some(o) = p.law.as_mut() {
                            append(&mut o.text, &t);
                        }
                    }
                    // institutions
                    let Some(first) = l.first() else { continue };
                    let lead = first.text.as_str();
                    let t = join_words(&l);
                    let rest = t.strip_prefix(lead).unwrap_or("").trim_start().to_owned();
                    if ROMAN.is_match(lead) {
                        group_wrap = false;
                        p.start_inst(&rest, page);
                    } else if NUM_SPLIT.is_match(lead) {
                        // a group label when a roman list follows, else an institution
                        let next = (i + 1..rs.len()).find_map(|j| left_leads[j].clone());
                        if next.as_deref().is_some_and(|n| ROMAN.is_match(n)) {
                            if let Some(o) = p.inst.take() {
                                p.out.institutions.push(item(o));
                            }
                            group_wrap = true;
                        } else {
                            group_wrap = false;
                            let lead_rest = NUM_SPLIT
                                .captures(lead)
                                .and_then(|c| c.get(2))
                                .map_or("", |m| m.as_str())
                                .to_owned();
                            let mut body = lead_rest;
                            append(&mut body, &rest);
                            p.start_inst(&body, page);
                        }
                    } else if group_wrap {
                        // the group label wraps onto this line
                    } else if let Some(o) = p.inst.as_mut() {
                        append(&mut o.text, &t);
                    } else if !t.trim().is_empty() {
                        warnings.push(format!(
                            "page {page}: unplaced text in an institutions list: {}",
                            snippet(&t)
                        ));
                    }
                }
            }
        }
        if table_page {
            out.table_pages.push(page_i32(page));
        }
    }
    for p in &mut ps {
        p.flush_all();
    }
    out.portfolios = ps.into_iter().map(|p| p.out).collect();
    out.warnings = warnings;
    out
}
