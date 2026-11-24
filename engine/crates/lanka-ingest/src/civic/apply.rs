//! Applying an amendment (13 §2.2, C2): the structure in force, plus the
//! changes [`super::amend`] read, gives the structure as amended. Pure.
//!
//! **Headings** are found by title (the portfolio key, then the closest
//! title), and by number only when the clause prints no title: numbers
//! change as headings come and go, titles are what the gazettes repeat.
//!
//! **Items** are found by the numbers they had before the amendment. The
//! changes of one clause all refer to the heading as it stood before that
//! clause, so "insert item 56 after item 55" and then "renumber items 56 to
//! 62 as 57 to 63" moves the old 56, not the new one. When a change prints
//! both an item's number and its text, both must agree. In Column III (acts,
//! not numbered) items are found by their text.
//!
//! Every change gets an [`Outcome`]: whether it was applied, and whether it
//! applied cleanly. A change that could not be applied is skipped, never
//! guessed, and fails the `changes_applied` check. The amended columns must
//! still number upwards (gaps are allowed: an omitted item leaves one).

use std::collections::BTreeSet;

use serde::Serialize;

use super::amend::{Anchor, Change, Column, HeadingRef, NewItem, Op, Target};
use super::entity::portfolio_key;
use super::schedule::{Item, Portfolio};

/// One item of a column in force, with the gazette it is printed in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Line {
    pub item_no: i32,
    pub text: String,
    pub page: Option<i32>,
    pub doc_id: String,
}

/// One minister's heading in force.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Heading {
    /// The number the gazettes give it.
    pub seq: i32,
    pub title: String,
    pub portfolio_key: String,
    /// The gazette (and page) the heading's title is printed in.
    pub doc_id: String,
    pub page: Option<i32>,
    pub functions: Vec<Line>,
    pub institutions: Vec<Line>,
    pub laws: Vec<Line>,
}

impl Heading {
    #[must_use]
    pub fn from_portfolio(p: &Portfolio, doc_id: &str) -> Self {
        let lines = |items: &[Item]| -> Vec<Line> {
            items
                .iter()
                .map(|i| Line {
                    item_no: i.item_no,
                    text: i.text.clone(),
                    page: Some(i.page),
                    doc_id: doc_id.to_owned(),
                })
                .collect()
        };
        Self {
            seq: p.seq,
            title: p.title.clone(),
            portfolio_key: portfolio_key(&p.title),
            doc_id: doc_id.to_owned(),
            page: Some(p.first_page),
            functions: lines(&p.functions),
            institutions: lines(&p.institutions),
            laws: lines(&p.laws),
        }
    }

    fn column(&self, c: Column) -> &Vec<Line> {
        match c {
            Column::Function => &self.functions,
            Column::Institution => &self.institutions,
            Column::Law => &self.laws,
        }
    }

    fn column_mut(&mut self, c: Column) -> &mut Vec<Line> {
        match c {
            Column::Function => &mut self.functions,
            Column::Institution => &mut self.institutions,
            Column::Law => &mut self.laws,
        }
    }
}

/// What happened to one change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Outcome {
    pub applied: bool,
    /// Applied as printed, with nothing to review.
    pub ok: bool,
    pub note: String,
}

impl Outcome {
    fn ok(note: impl Into<String>) -> Self {
        Self {
            applied: true,
            ok: true,
            note: note.into(),
        }
    }
    fn review(note: impl Into<String>) -> Self {
        Self {
            applied: true,
            ok: false,
            note: note.into(),
        }
    }
    fn skipped(note: impl Into<String>) -> Self {
        Self {
            applied: false,
            ok: false,
            note: note.into(),
        }
    }
}

/// The structure as amended.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Applied {
    pub headings: Vec<Heading>,
    /// One per change, in order.
    pub outcomes: Vec<Outcome>,
    /// Columns whose numbers no longer run upwards after the amendment.
    pub numbering: Vec<String>,
}

// ─────────────────────────── matching ───────────────────────────────────────

const STOP: &[&str] = &[
    "the", "of", "and", "no", "in", "to", "for", "a", "an", "its", "with", "on", "by",
];

fn tokens(s: &str) -> BTreeSet<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && !STOP.contains(w))
        .map(str::to_owned)
        .collect()
}

/// Two printings of the same item: most words shared (or one inside the other).
#[must_use]
pub fn same_text(a: &str, b: &str) -> bool {
    let (x, y) = (tokens(a), tokens(b));
    if x.is_empty() || y.is_empty() {
        return false;
    }
    let inter = x.intersection(&y).count();
    let union = x.union(&y).count();
    let small = x.len().min(y.len());
    #[allow(clippy::cast_precision_loss)]
    let jaccard = inter as f64 / union as f64;
    jaccard >= 0.6 || (small >= 3 && inter == small)
}

fn title_score(a: &str, b: &str) -> f64 {
    let strip = |s: &str| {
        let mut t = tokens(s);
        t.remove("minister");
        t.remove("ministry");
        t
    };
    let (x, y) = (strip(a), strip(b));
    if x.is_empty() || y.is_empty() {
        return 0.0;
    }
    #[allow(clippy::cast_precision_loss)]
    let s = x.intersection(&y).count() as f64 / x.union(&y).count() as f64;
    s
}

/// The heading a change names.
fn find_heading(hs: &[Heading], r: &HeadingRef) -> Result<(usize, Option<String>), String> {
    if let Some(title) = &r.title {
        let key = portfolio_key(title);
        let by_key: Vec<usize> = (0..hs.len())
            .filter(|&i| hs[i].portfolio_key == key)
            .collect();
        match by_key.as_slice() {
            [i] => return Ok((*i, None)),
            [] => {}
            many => {
                if let Some(&i) = many.iter().find(|&&i| Some(hs[i].seq) == r.no) {
                    return Ok((i, None));
                }
                return Err(format!("{} headings are titled “{title}”", many.len()));
            }
        }
        let best = (0..hs.len())
            .map(|i| (i, title_score(title, &hs[i].title)))
            .max_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((i, s)) = best {
            let number_agrees = r.no == Some(hs[i].seq);
            if s >= 0.8 || (number_agrees && s >= 0.4) {
                return Ok((
                    i,
                    Some(format!("“{title}” read as the heading “{}”", hs[i].title)),
                ));
            }
        }
        return Err(format!("no heading “{title}” in the structure in force"));
    }
    if let Some(n) = r.no {
        let by_no: Vec<usize> = (0..hs.len()).filter(|&i| hs[i].seq == n).collect();
        return match by_no.as_slice() {
            [i] => Ok((*i, None)),
            [] => Err(format!("no heading numbered {n:02}")),
            _ => Err(format!("more than one heading numbered {n:02}")),
        };
    }
    Err("the change names no heading".to_owned())
}

// ─────────────────────────── one column, one clause ─────────────────────────

#[derive(Debug, Clone)]
struct Entry {
    /// Number before the clause; `None` for an item the clause inserts.
    orig: Option<i32>,
    /// Number after the clause; `None` while it is to be numbered on.
    no: Option<i32>,
    line: Line,
    gone: bool,
}

struct Edit {
    heading: usize,
    col: Column,
    entries: Vec<Entry>,
}

impl Edit {
    fn new(h: &Heading, heading: usize, col: Column) -> Self {
        Self {
            heading,
            col,
            entries: h
                .column(col)
                .iter()
                .map(|l| Entry {
                    orig: Some(l.item_no),
                    no: Some(l.item_no),
                    line: l.clone(),
                    gone: false,
                })
                .collect(),
        }
    }

    fn by_orig(&self, n: i32) -> Option<usize> {
        self.entries.iter().position(|e| e.orig == Some(n))
    }

    fn by_text(&self, text: &str) -> Result<usize, String> {
        let hits: Vec<usize> = (0..self.entries.len())
            .filter(|&i| !self.entries[i].gone && same_text(&self.entries[i].line.text, text))
            .collect();
        match hits.as_slice() {
            [i] => Ok(*i),
            [] => Err(format!("no item reads “{}”", short(text))),
            _ => {
                // an exact wording breaks the tie
                let norm = |s: &str| tokens(s);
                let exact: Vec<usize> = hits
                    .iter()
                    .copied()
                    .filter(|&i| norm(&self.entries[i].line.text) == norm(text))
                    .collect();
                if let [i] = exact.as_slice() {
                    Ok(*i)
                } else {
                    Err(format!("{} items read “{}”", hits.len(), short(text)))
                }
            }
        }
    }

    fn line(item: &NewItem, doc_id: &str) -> Line {
        Line {
            item_no: item.item_no.unwrap_or(0),
            text: item.text.clone(),
            page: Some(item.page),
            doc_id: doc_id.to_owned(),
        }
    }

    fn new_entry(item: &NewItem, doc_id: &str) -> Entry {
        Entry {
            orig: None,
            no: item.item_no,
            line: Self::line(item, doc_id),
            gone: false,
        }
    }

    /// Position after `i` and after any items already inserted right after it.
    fn after(&self, i: usize) -> usize {
        let mut p = i + 1;
        while p < self.entries.len() && self.entries[p].orig.is_none() {
            p += 1;
        }
        p
    }

    fn apply(&mut self, op: &Op, doc_id: &str) -> Outcome {
        let col = self.col;
        match op {
            Op::Omit { nos, quoted, .. } => {
                let mut notes: Vec<String> = Vec::new();
                let mut ok = true;
                let mut done = 0;
                if nos.is_empty() {
                    for q in quoted {
                        match self.by_text(&q.text) {
                            Ok(i) => {
                                self.entries[i].gone = true;
                                done += 1;
                            }
                            Err(e) => {
                                ok = false;
                                notes.push(e);
                            }
                        }
                    }
                } else {
                    for &n in nos {
                        let printed = quoted.iter().find(|q| q.item_no == Some(n));
                        match (self.by_orig(n), printed) {
                            (Some(i), Some(q))
                                if !same_text(&self.entries[i].line.text, &q.text) =>
                            {
                                // the number and the text disagree: the text decides when it is unambiguous
                                ok = false;
                                match self.by_text(&q.text) {
                                    Ok(j) => {
                                        self.entries[j].gone = true;
                                        done += 1;
                                        notes.push(format!(
                                            "item {n} reads “{}”; the printed “{}” is item {}, which is omitted",
                                            short(&self.entries[i].line.text),
                                            short(&q.text),
                                            self.entries[j].orig.map_or_else(|| "?".to_owned(), |o| o.to_string())
                                        ));
                                    }
                                    Err(e) => notes.push(format!(
                                        "item {n} reads “{}”, not “{}” ({e}); not omitted",
                                        short(&self.entries[i].line.text),
                                        short(&q.text)
                                    )),
                                }
                            }
                            (Some(i), _) if self.entries[i].gone => {
                                ok = false;
                                notes.push(format!("item {n} is omitted twice"));
                            }
                            (Some(i), _) => {
                                self.entries[i].gone = true;
                                done += 1;
                            }
                            (None, Some(q)) => {
                                ok = false;
                                match self.by_text(&q.text) {
                                    Ok(j) => {
                                        self.entries[j].gone = true;
                                        done += 1;
                                        notes.push(format!(
                                            "there is no item {n}; “{}” is omitted by its text",
                                            short(&q.text)
                                        ));
                                    }
                                    Err(e) => notes.push(format!("there is no item {n} ({e})")),
                                }
                            }
                            (None, None) => {
                                ok = false;
                                notes.push(format!("there is no item {n}"));
                            }
                        }
                    }
                }
                let note = if notes.is_empty() {
                    format!("{done} item(s) omitted from Column {}", roman(col))
                } else {
                    notes.join("; ")
                };
                Outcome {
                    applied: done > 0,
                    ok,
                    note,
                }
            }
            Op::Insert { anchor, items, .. } => {
                let at = match anchor {
                    Anchor::After { no } => match self.by_orig(*no) {
                        Some(i) => self.after(i),
                        None => {
                            return Outcome::skipped(format!(
                                "there is no item {no} to insert after"
                            ));
                        }
                    },
                    Anchor::Before { no } => match self.by_orig(*no) {
                        Some(i) => i,
                        None => {
                            return Outcome::skipped(format!(
                                "there is no item {no} to insert before"
                            ));
                        }
                    },
                    Anchor::AfterText { text } => match self.by_text(text) {
                        Ok(i) => self.after(i),
                        Err(e) => return Outcome::skipped(format!("{e}, to insert after")),
                    },
                    Anchor::End => self.entries.len(),
                };
                for (k, it) in items.iter().enumerate() {
                    self.entries.insert(at + k, Self::new_entry(it, doc_id));
                }
                Outcome::ok(format!(
                    "{} item(s) inserted in Column {}",
                    items.len(),
                    roman(col)
                ))
            }
            Op::Substitute { target, item, .. } => {
                let i = match target {
                    Target::No { no } => match self.by_orig(*no) {
                        Some(i) => i,
                        None => {
                            return Outcome::skipped(format!(
                                "there is no item {no} to substitute"
                            ));
                        }
                    },
                    Target::Text { text } => match self.by_text(text) {
                        Ok(i) => i,
                        Err(e) => return Outcome::skipped(format!("{e}, to substitute")),
                    },
                };
                let e = &mut self.entries[i];
                let was = e.line.text.clone();
                let no = item.item_no.or(e.no);
                e.line = Self::line(item, doc_id);
                e.no = no;
                Outcome::ok(format!("“{}” is now “{}”", short(&was), short(&item.text)))
            }
            Op::Renumber {
                from, to, onward, ..
            } if *onward => {
                // that item and every one after it (or the whole column) numbered on
                let start = match from.first() {
                    Some(f) => match self.by_orig(*f) {
                        Some(i) => i,
                        None => {
                            return Outcome::skipped(format!(
                                "there is no item {f} to renumber from"
                            ));
                        }
                    },
                    None => 0,
                };
                let _ = to;
                for e in &mut self.entries[start..] {
                    e.no = None;
                }
                // the item before the first renumbered one keeps its number
                Outcome::ok(match from.first() {
                    Some(f) => format!("items from {f} renumbered in order"),
                    None => format!("Column {} renumbered in order", roman(col)),
                })
            }
            Op::Renumber { from, to, .. } => {
                if let Some(to) = to
                    && to.len() != from.len()
                {
                    return Outcome::skipped(format!(
                        "renumbers {} items as {} numbers",
                        from.len(),
                        to.len()
                    ));
                }
                let mut missing = Vec::new();
                for (k, f) in from.iter().enumerate() {
                    match self.by_orig(*f) {
                        Some(i) if !self.entries[i].gone => {
                            self.entries[i].no = to.as_ref().map(|t| t[k]);
                        }
                        _ => missing.push(f.to_string()),
                    }
                }
                if missing.is_empty() {
                    Outcome::ok(format!("{} item(s) renumbered", from.len()))
                } else {
                    Outcome::review(format!("no item {} to renumber", missing.join(", ")))
                }
            }
            Op::ReplaceColumn { items, .. } => {
                for e in &mut self.entries {
                    e.gone = true;
                }
                self.entries
                    .extend(items.iter().map(|it| Self::new_entry(it, doc_id)));
                Outcome::ok(format!(
                    "Column {} replaced with {} item(s)",
                    roman(col),
                    items.len()
                ))
            }
            _ => Outcome::skipped("not a change to a column"),
        }
    }

    /// The column as amended, and a note when its numbers no longer run upwards.
    fn finish(self) -> (usize, Column, Vec<Line>, Option<String>) {
        let mut out: Vec<Line> = Vec::new();
        let mut prev = 0;
        for e in self.entries.into_iter().filter(|e| !e.gone) {
            let mut line = e.line;
            line.item_no = match (self.col, e.no) {
                (Column::Law, _) => i32::try_from(out.len() + 1).unwrap_or(i32::MAX),
                (_, Some(n)) => n,
                (_, None) => prev + 1,
            };
            prev = line.item_no;
            out.push(line);
        }
        let note = if self.col == Column::Law {
            None
        } else {
            out.windows(2)
                .find(|w| w[1].item_no <= w[0].item_no)
                .map(|w| format!("item {} follows item {}", w[1].item_no, w[0].item_no))
        };
        (self.heading, self.col, out, note)
    }
}

fn roman(c: Column) -> &'static str {
    match c {
        Column::Function => "I",
        Column::Institution => "II",
        Column::Law => "III",
    }
}

fn short(s: &str) -> String {
    let t: String = s.chars().take(70).collect();
    if t.len() < s.len() {
        format!("{t}…")
    } else {
        t
    }
}

/// A new heading's own numbering (columns I and II run 1..k).
fn table_problems(p: &Portfolio) -> Vec<String> {
    let mut v = Vec::new();
    if p.functions.is_empty() && p.institutions.is_empty() {
        v.push(format!("the new heading “{}” has no items", p.title));
    }
    for (c, items) in [("I", &p.functions), ("II", &p.institutions)] {
        for (k, it) in items.iter().enumerate() {
            let want = i32::try_from(k + 1).unwrap_or(i32::MAX);
            if it.item_no != want {
                v.push(format!(
                    "the new heading's Column {c} reads item {} where {want} was expected",
                    it.item_no
                ));
                break;
            }
        }
    }
    v
}

// ─────────────────────────── the whole amendment ────────────────────────────

fn target(
    hs: &[Heading],
    ch: &Change,
    pinned: &mut Option<(usize, Option<String>)>,
) -> Result<(usize, Option<String>), String> {
    if let Some(p) = pinned {
        return Ok(p.clone());
    }
    let found = match &ch.heading {
        Some(r) => find_heading(hs, r),
        None => Err("the change names no heading".to_owned()),
    }?;
    *pinned = Some(found.clone());
    Ok(found)
}

fn clause_of(label: &str) -> &str {
    label.split(' ').next().unwrap_or(label)
}

/// Applies the changes of the amendment `doc_id` to `base`.
#[must_use]
pub fn apply(base: &[Heading], changes: &[Change], doc_id: &str) -> Applied {
    let mut hs: Vec<Heading> = base.to_vec();
    let mut outcomes: Vec<Outcome> = Vec::with_capacity(changes.len());
    let mut numbering: Vec<String> = Vec::new();
    let mut edits: Vec<Edit> = Vec::new();
    let mut clause = String::new();
    let mut last_inserted: Option<usize> = None;
    // the heading a clause names, found once: a clause may rename it first
    // ("The Heading as …") and then change its columns
    let mut pinned: Option<(usize, Option<String>)> = None;

    let flush = |hs: &mut Vec<Heading>, edits: &mut Vec<Edit>, numbering: &mut Vec<String>| {
        for e in edits.drain(..) {
            let (h, col, lines, note) = e.finish();
            if let Some(n) = note {
                numbering.push(format!(
                    "{:02}. {}, Column {}: {n}",
                    hs[h].seq,
                    hs[h].title,
                    roman(col)
                ));
            }
            *hs[h].column_mut(col) = lines;
        }
    };

    for ch in changes {
        if clause_of(&ch.clause) != clause {
            flush(&mut hs, &mut edits, &mut numbering);
            clause = clause_of(&ch.clause).to_owned();
            last_inserted = None;
            pinned = None;
        }
        if let Some(lang) = &ch.lang_only {
            let which = if lang == "si" { "Sinhala" } else { "Tamil" };
            outcomes.push(Outcome {
                applied: false,
                ok: true,
                note: format!("made in the {which} text only"),
            });
            continue;
        }
        let outcome = match &ch.op {
            Op::Unparsed { reason } => Outcome::skipped(format!("not read: {reason}")),
            Op::RenumberHeadings => {
                Outcome::ok("headings are found by title, so their numbers change nothing here")
            }
            Op::Omit { .. }
            | Op::Insert { .. }
            | Op::Substitute { .. }
            | Op::Renumber { .. }
            | Op::ReplaceColumn { .. } => {
                match target(&hs, ch, &mut pinned) {
                    Err(e) => Outcome::skipped(e),
                    Ok((h, note)) => {
                        let col = ch.op.column().unwrap_or(Column::Function);
                        let k = match edits.iter().position(|e| e.heading == h && e.col == col) {
                            Some(k) => k,
                            None => {
                                edits.push(Edit::new(&hs[h], h, col));
                                edits.len() - 1
                            }
                        };
                        let mut o = edits[k].apply(&ch.op, doc_id);
                        if let Some(n) = note {
                            // a heading found by a close title, not its own: a person checks it
                            o.note = format!("{n}; {}", o.note);
                            o.ok = false;
                        }
                        o
                    }
                }
            }
            Op::RenameHeading { title } => match target(&hs, ch, &mut pinned) {
                Err(e) => Outcome::skipped(e),
                Ok((h, _)) => {
                    let was = std::mem::replace(&mut hs[h].title, title.clone());
                    hs[h].portfolio_key = portfolio_key(title);
                    Outcome::ok(format!("“{was}” is now “{title}”"))
                }
            },
            Op::ReplaceColumns { portfolio } => {
                flush(&mut hs, &mut edits, &mut numbering);
                match target(&hs, ch, &mut pinned) {
                    Err(e) => Outcome::skipped(e),
                    Ok((h, _)) => {
                        let new = Heading::from_portfolio(portfolio, doc_id);
                        let problems = table_problems(portfolio);
                        hs[h].functions = new.functions;
                        hs[h].institutions = new.institutions;
                        hs[h].laws = new.laws;
                        let note = format!(
                            "the three columns of “{}” replaced ({} institutions)",
                            hs[h].title,
                            hs[h].institutions.len()
                        );
                        if problems.is_empty() {
                            Outcome::ok(note)
                        } else {
                            Outcome::review(format!("{note}; {}", problems.join("; ")))
                        }
                    }
                }
            }
            Op::ReplaceHeading { portfolio } => {
                flush(&mut hs, &mut edits, &mut numbering);
                pinned = None;
                let r = ch.heading.clone().unwrap_or(HeadingRef {
                    no: Some(portfolio.seq),
                    title: Some(portfolio.title.clone()),
                });
                match find_heading(&hs, &r) {
                    Err(e) => Outcome::skipped(e),
                    Ok((h, _)) => {
                        let was = hs[h].title.clone();
                        hs[h] = Heading::from_portfolio(portfolio, doc_id);
                        let problems = table_problems(portfolio);
                        let note = format!("“{was}” replaced by “{}”", portfolio.title);
                        if problems.is_empty() {
                            Outcome::ok(note)
                        } else {
                            Outcome::review(format!("{note}; {}", problems.join("; ")))
                        }
                    }
                }
            }
            Op::InsertHeading { portfolio } => {
                flush(&mut hs, &mut edits, &mut numbering);
                pinned = None;
                let key = portfolio_key(&portfolio.title);
                if hs.iter().any(|h| h.portfolio_key == key) {
                    Outcome::skipped(format!(
                        "a heading “{}” is already in force",
                        portfolio.title
                    ))
                } else {
                    let at = match (last_inserted, &ch.heading) {
                        (Some(i), _) => Ok(i + 1),
                        (None, Some(r)) => find_heading(&hs, r).map(|(i, _)| i + 1),
                        (None, None) => Ok(hs.len()),
                    };
                    match at {
                        Err(e) => Outcome::skipped(format!("{e}, to insert after")),
                        Ok(at) => {
                            hs.insert(at, Heading::from_portfolio(portfolio, doc_id));
                            last_inserted = Some(at);
                            let problems = table_problems(portfolio);
                            let note = format!(
                                "new heading “{}” ({} institutions)",
                                portfolio.title,
                                portfolio.institutions.len()
                            );
                            if problems.is_empty() {
                                Outcome::ok(note)
                            } else {
                                Outcome::review(format!("{note}; {}", problems.join("; ")))
                            }
                        }
                    }
                }
            }
            Op::OmitHeading => {
                flush(&mut hs, &mut edits, &mut numbering);
                match target(&hs, ch, &mut pinned) {
                    Err(e) => Outcome::skipped(e),
                    Ok((h, _)) => {
                        pinned = None;
                        let gone = hs.remove(h);
                        Outcome::ok(format!(
                            "heading “{}” omitted ({} institutions)",
                            gone.title,
                            gone.institutions.len()
                        ))
                    }
                }
            }
        };
        outcomes.push(outcome);
    }
    flush(&mut hs, &mut edits, &mut numbering);
    Applied {
        headings: hs,
        outcomes,
        numbering,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(no: i32, text: &str) -> Line {
        Line {
            item_no: no,
            text: text.to_owned(),
            page: Some(3),
            doc_id: "gze-base".to_owned(),
        }
    }

    fn heading(seq: i32, title: &str, inst: &[&str]) -> Heading {
        Heading {
            seq,
            title: title.to_owned(),
            portfolio_key: portfolio_key(title),
            doc_id: "gze-base".to_owned(),
            page: Some(2),
            functions: vec![
                line(1, "Formulation of policies"),
                line(2, "Supervision of institutions"),
            ],
            institutions: inst
                .iter()
                .enumerate()
                .map(|(k, t)| line(i32::try_from(k + 1).expect("n"), t))
                .collect(),
            laws: vec![
                line(1, "Army Act No. 17 of 1949"),
                line(2, "Navy Act No. 34 of 1950"),
            ],
        }
    }

    fn change(clause: &str, title: &str, op: Op) -> Change {
        Change {
            clause: clause.to_owned(),
            heading: Some(HeadingRef {
                no: None,
                title: Some(title.to_owned()),
            }),
            lang_only: None,
            op,
            page: 1,
            text: String::new(),
        }
    }

    fn new(no: i32, text: &str) -> NewItem {
        NewItem {
            item_no: Some(no),
            text: text.to_owned(),
            page: 1,
        }
    }

    fn nos(h: &Heading) -> Vec<(i32, &str)> {
        h.institutions
            .iter()
            .map(|l| (l.item_no, l.text.as_str()))
            .collect()
    }

    #[test]
    fn insert_then_renumber_refers_to_the_numbers_before_the_clause() {
        let base = vec![heading(
            2,
            "Minister of Finance",
            &["A Board", "B Department", "C Authority", "D Fund"],
        )];
        let ch = vec![
            change(
                "(2) (e)",
                "Minister of Finance",
                Op::Insert {
                    col: Column::Institution,
                    anchor: Anchor::After { no: 2 },
                    items: vec![new(3, "Gambling Regulatory Authority")],
                },
            ),
            change(
                "(2) (f)",
                "Minister of Finance",
                Op::Renumber {
                    col: Column::Institution,
                    from: vec![3, 4],
                    to: Some(vec![4, 5]),
                    onward: false,
                },
            ),
        ];
        let a = apply(&base, &ch, "gze-amend");
        assert!(a.outcomes.iter().all(|o| o.ok), "{:?}", a.outcomes);
        assert!(a.numbering.is_empty(), "{:?}", a.numbering);
        assert_eq!(
            nos(&a.headings[0]),
            vec![
                (1, "A Board"),
                (2, "B Department"),
                (3, "Gambling Regulatory Authority"),
                (4, "C Authority"),
                (5, "D Fund")
            ]
        );
        // the new line carries the amendment as its evidence
        assert_eq!(a.headings[0].institutions[2].doc_id, "gze-amend");
        assert_eq!(a.headings[0].institutions[0].doc_id, "gze-base");
    }

    #[test]
    fn an_omission_without_renumbering_leaves_a_gap() {
        let base = vec![heading(
            1,
            "Minister of Defence",
            &[
                "Sri Lanka Army",
                "Civil Security Department",
                "Sri Lanka Navy",
            ],
        )];
        let ch = vec![change(
            "(1) (c)",
            "Minister of Defence",
            Op::Omit {
                col: Column::Institution,
                nos: vec![2],
                quoted: vec![new(2, "Civil Security Department")],
            },
        )];
        let a = apply(&base, &ch, "gze-amend");
        assert!(a.outcomes[0].ok);
        assert_eq!(
            nos(&a.headings[0]),
            vec![(1, "Sri Lanka Army"), (3, "Sri Lanka Navy")]
        );
        assert!(a.numbering.is_empty());
    }

    #[test]
    fn a_number_that_disagrees_with_its_text_goes_to_review() {
        let base = vec![heading(
            5,
            "Minister of Justice",
            &[
                "Legal Aid Commission",
                "Law Commission",
                "Rehabilitation Bureau",
            ],
        )];
        let ch = vec![change(
            "(5) (a)",
            "Minister of Justice",
            Op::Omit {
                col: Column::Institution,
                nos: vec![2],
                quoted: vec![new(2, "Rehabilitation Bureau")],
            },
        )];
        let a = apply(&base, &ch, "gze-amend");
        assert!(
            a.outcomes[0].applied && !a.outcomes[0].ok,
            "{:?}",
            a.outcomes[0]
        );
        // the text decided: the bureau went, the commission stayed
        assert_eq!(
            nos(&a.headings[0]),
            vec![(1, "Legal Aid Commission"), (2, "Law Commission")]
        );
    }

    #[test]
    fn a_missing_item_is_skipped_never_guessed() {
        let base = vec![heading(1, "Minister of Defence", &["Sri Lanka Army"])];
        let ch = vec![change(
            "(1) (a)",
            "Minister of Defence",
            Op::Substitute {
                col: Column::Institution,
                target: Target::No { no: 9 },
                item: new(9, "Something"),
            },
        )];
        let a = apply(&base, &ch, "gze-amend");
        assert!(!a.outcomes[0].applied && !a.outcomes[0].ok);
        assert_eq!(a.headings, base);
    }

    #[test]
    fn numbering_on_from_the_item_before() {
        // 2096/61: insert 21 after 20, then "re-numbering the items 21 and 22"
        let mut h = heading(1, "Minister of Defence", &["A"]);
        h.functions = (1..=22)
            .map(|n| line(n, &format!("function {n}")))
            .collect();
        let ch = vec![
            change(
                "(1) (a)",
                "Minister of Defence",
                Op::Insert {
                    col: Column::Function,
                    anchor: Anchor::After { no: 20 },
                    items: vec![new(21, "Government printing")],
                },
            ),
            change(
                "(1) (b)",
                "Minister of Defence",
                Op::Renumber {
                    col: Column::Function,
                    from: vec![21, 22],
                    to: None,
                    onward: false,
                },
            ),
        ];
        let a = apply(&[h], &ch, "gze-amend");
        let f: Vec<(i32, &str)> = a.headings[0]
            .functions
            .iter()
            .skip(19)
            .map(|l| (l.item_no, l.text.as_str()))
            .collect();
        assert_eq!(
            f,
            vec![
                (20, "function 20"),
                (21, "Government printing"),
                (22, "function 21"),
                (23, "function 22")
            ]
        );
        assert!(a.numbering.is_empty());
    }

    #[test]
    fn a_forgotten_renumbering_is_reported() {
        let base = vec![heading(1, "Minister of Defence", &["A", "B", "C"])];
        let ch = vec![change(
            "(1) (a)",
            "Minister of Defence",
            Op::Insert {
                col: Column::Institution,
                anchor: Anchor::After { no: 1 },
                items: vec![new(2, "New")],
            },
        )];
        let a = apply(&base, &ch, "gze-amend");
        assert_eq!(a.numbering.len(), 1, "{:?}", a.numbering);
    }

    #[test]
    fn headings_are_found_by_title_and_renamed_inserted_and_omitted() {
        let base = vec![
            heading(1, "Minister of Defence", &["Sri Lanka Army"]),
            heading(
                27,
                "Minister of Technology and Investment Promotion",
                &["Board of Investment"],
            ),
            heading(28, "Minister of Women", &["Women's Bureau"]),
        ];
        let table = Portfolio {
            seq: 29,
            title: "Minister of Investment Promotion".to_owned(),
            first_page: 2,
            functions: vec![Item {
                item_no: 1,
                text: "Investment".to_owned(),
                page: 2,
            }],
            institutions: vec![Item {
                item_no: 1,
                text: "Board of Investment".to_owned(),
                page: 2,
            }],
            laws: vec![],
        };
        let ch = vec![
            // the clause prints a number that no longer matches; the title decides
            Change {
                heading: Some(HeadingRef {
                    no: Some(3),
                    title: Some("Minister of Technology and Investment Promotion".to_owned()),
                }),
                ..change(
                    "(1) (a)",
                    "",
                    Op::RenameHeading {
                        title: "Minister of Technology".to_owned(),
                    },
                )
            },
            change(
                "(2)",
                "Minister of Women",
                Op::InsertHeading { portfolio: table },
            ),
            change("(3)", "Minister of Defence", Op::OmitHeading),
        ];
        let a = apply(&base, &ch, "gze-amend");
        assert!(a.outcomes.iter().all(|o| o.ok), "{:?}", a.outcomes);
        let titles: Vec<&str> = a.headings.iter().map(|h| h.title.as_str()).collect();
        assert_eq!(
            titles,
            vec![
                "Minister of Technology",
                "Minister of Women",
                "Minister of Investment Promotion"
            ]
        );
        assert_eq!(a.headings[0].portfolio_key, "technology");
    }

    #[test]
    fn a_clause_that_renames_its_heading_still_changes_its_columns() {
        let base = vec![heading(
            27,
            "Minister of Technology and Investment Promotion",
            &["Board of Investment", "ICTA"],
        )];
        let ch = vec![
            change(
                "(1) (a)",
                "Minister of Technology and Investment Promotion",
                Op::RenameHeading {
                    title: "Minister of Technology".to_owned(),
                },
            ),
            change(
                "(1) (b)",
                "Minister of Technology and Investment Promotion",
                Op::Omit {
                    col: Column::Institution,
                    nos: vec![1],
                    quoted: vec![],
                },
            ),
        ];
        let a = apply(&base, &ch, "gze-amend");
        assert!(a.outcomes.iter().all(|o| o.ok), "{:?}", a.outcomes);
        assert_eq!(nos(&a.headings[0]), vec![(2, "ICTA")]);
    }

    #[test]
    fn a_close_title_is_not_taken_for_another_ministry() {
        let base = vec![
            heading(14, "Minister of Agriculture", &["A"]),
            heading(18, "Minister of Plantation Industries", &["B"]),
        ];
        let mut c = change(
            "(2) (a)",
            "Minister of Agriculture and Plantation Industries",
            Op::Omit {
                col: Column::Institution,
                nos: vec![1],
                quoted: vec![],
            },
        );
        c.heading = Some(HeadingRef {
            no: Some(14),
            title: Some("Minister of Agriculture and Plantation Industries".to_owned()),
        });
        let a = apply(&base, &[c], "gze-amend");
        // neither title is close enough: skipped for review, never guessed
        assert!(
            !a.outcomes[0].applied && !a.outcomes[0].ok,
            "{:?}",
            a.outcomes[0]
        );
        assert_eq!(a.headings, base);
    }

    #[test]
    fn changes_in_one_language_only_are_not_applied() {
        let base = vec![heading(1, "Minister of Defence", &["A"])];
        let mut c = change(
            "(1) (d)",
            "Minister of Defence",
            Op::Omit {
                col: Column::Law,
                nos: vec![],
                quoted: vec![NewItem {
                    item_no: None,
                    text: "Army Act No. 17 of 1949".to_owned(),
                    page: 1,
                }],
            },
        );
        c.lang_only = Some("ta".to_owned());
        let a = apply(&base, &[c], "gze-amend");
        assert!(!a.outcomes[0].applied && a.outcomes[0].ok);
        assert_eq!(a.headings, base);
    }

    #[test]
    fn acts_are_found_by_their_text() {
        let base = vec![heading(1, "Minister of Defence", &["A"])];
        let ch = vec![
            change(
                "(1) (a)",
                "Minister of Defence",
                Op::Insert {
                    col: Column::Law,
                    anchor: Anchor::AfterText {
                        text: "Army Act, No. 17 of 1949".to_owned(),
                    },
                    items: vec![NewItem {
                        item_no: None,
                        text: "Air Force Act No. 41 of 1949".to_owned(),
                        page: 1,
                    }],
                },
            ),
            change(
                "(1) (b)",
                "Minister of Defence",
                Op::Omit {
                    col: Column::Law,
                    nos: vec![],
                    quoted: vec![NewItem {
                        item_no: None,
                        text: "Navy Act No. 34 of 1950".to_owned(),
                        page: 1,
                    }],
                },
            ),
        ];
        let a = apply(&base, &ch, "gze-amend");
        assert!(a.outcomes.iter().all(|o| o.ok), "{:?}", a.outcomes);
        let laws: Vec<(i32, &str)> = a.headings[0]
            .laws
            .iter()
            .map(|l| (l.item_no, l.text.as_str()))
            .collect();
        assert_eq!(
            laws,
            vec![
                (1, "Army Act No. 17 of 1949"),
                (2, "Air Force Act No. 41 of 1949")
            ]
        );
    }
}
