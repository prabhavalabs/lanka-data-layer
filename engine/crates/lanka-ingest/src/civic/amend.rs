//! The amendment parser (13 §2.2, C2): a pure function from the words of an
//! amending gazette to the changes it makes, in the order it prints them.
//!
//! An amending schedule is prose, clause by clause:
//!
//! ```text
//! (2) With reference to the Heading, No. "02. Minister of Finance …" of the said notification, as follows:-
//!     (e) In Column II thereof, by insertion of the following item immediately after item No. 55;
//!            56. Gambling Regulatory Authority
//!     (f) In Column II thereof, re-numbering of items 56 to 62 respectively, as numbers 57 to 63;
//! (3) By insertion of the following new Heading after Columns I, II and III of the Heading No. "28. …";
//!     <a full three-column table>
//! ```
//!
//! A clause names a heading; its lettered sub-clauses each make one change to
//! one column. The changes read are: omit items (by number, or by the text
//! printed below), insert items after an item (or after an act, in Column
//! III), substitute an item, renumber items, replace a whole column, rename
//! a heading, insert a new heading, replace a heading with a new one, and
//! omit a heading. A change "only in Sinhala (or Tamil) Notification" is read
//! and kept, but does not apply to the English schedule.
//!
//! Tables are read by [`super::schedule::parse`] on the words of the table
//! alone, cut where the next clause starts. A sentence the parser does not
//! understand is kept as [`Op::Unparsed`] with its text: it is never dropped,
//! and it fails the `changes_read` check, so the gazette goes to review.

use std::sync::LazyLock;

use lanka_extractd::{Word, Words};
use regex::Regex;
use serde::{Deserialize, Serialize};

use super::schedule::{self, HEADING, Portfolio, append, clean, is_bullet_token, rows};

/// The column a change applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Column {
    /// Column I: subjects and functions.
    Function,
    /// Column II: departments, statutory institutions and public corporations.
    Institution,
    /// Column III: laws to be implemented.
    Law,
}

impl Column {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Institution => "institution",
            Self::Law => "law",
        }
    }

    fn roman(self) -> &'static str {
        match self {
            Self::Function => "I",
            Self::Institution => "II",
            Self::Law => "III",
        }
    }
}

/// A heading as a clause names it: its number and title, either of which can
/// be missing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeadingRef {
    pub no: Option<i32>,
    pub title: Option<String>,
}

impl std::fmt::Display for HeadingRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.no, &self.title) {
            (Some(n), Some(t)) => write!(f, "{n:02}. {t}"),
            (None, Some(t)) => f.write_str(t),
            (Some(n), None) => write!(f, "heading {n:02}"),
            (None, None) => f.write_str("(no heading)"),
        }
    }
}

/// An item printed in the amendment (a numbered line, or an act after a bullet).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewItem {
    pub item_no: Option<i32>,
    pub text: String,
    pub page: i32,
}

/// Where inserted items go.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "at", rename_all = "snake_case")]
pub enum Anchor {
    /// Immediately after item `no`.
    After { no: i32 },
    /// After the item with this text (Column III names acts, not numbers).
    AfterText { text: String },
    /// Immediately before item `no`.
    Before { no: i32 },
    /// At the end of the column.
    End,
}

/// The item a substitution replaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "by", rename_all = "snake_case")]
pub enum Target {
    No { no: i32 },
    Text { text: String },
}

/// One change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    /// Omit items by number, and/or those whose text is printed (`quoted`).
    /// When both are printed, they must agree.
    Omit {
        col: Column,
        nos: Vec<i32>,
        quoted: Vec<NewItem>,
    },
    Insert {
        col: Column,
        anchor: Anchor,
        items: Vec<NewItem>,
    },
    Substitute {
        col: Column,
        target: Target,
        item: NewItem,
    },
    /// Items `from[k]` become `to[k]`. Without `to` ("re-numbering the items
    /// 21 and 22") they are numbered on from the item before them.
    Renumber {
        col: Column,
        from: Vec<i32>,
        to: Option<Vec<i32>>,
        /// "re-numbering the items from 22": item `from[0]` and every item
        /// after it are numbered on; with `from` empty, the whole column.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        onward: bool,
    },
    /// The whole column is replaced ("in Column I change as follows").
    ReplaceColumn {
        col: Column,
        items: Vec<NewItem>,
    },
    /// All three columns of the heading are replaced; the title stays.
    ReplaceColumns {
        portfolio: Portfolio,
    },
    RenameHeading {
        title: String,
    },
    /// The heading and its three columns are replaced by this one.
    ReplaceHeading {
        portfolio: Portfolio,
    },
    /// A new heading, after the heading the change names (or at the end).
    InsertHeading {
        portfolio: Portfolio,
    },
    OmitHeading,
    /// Headings renumbered. Headings are found by title, so this changes
    /// nothing in the structure; it is kept as a record.
    RenumberHeadings,
    /// A sentence the parser did not understand.
    Unparsed {
        reason: String,
    },
}

impl Op {
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Omit { .. } => "omit",
            Self::Insert { .. } => "insert",
            Self::Substitute { .. } => "substitute",
            Self::Renumber { .. } => "renumber",
            Self::ReplaceColumn { .. } => "replace_column",
            Self::ReplaceColumns { .. } => "replace_columns",
            Self::RenameHeading { .. } => "rename_heading",
            Self::ReplaceHeading { .. } => "replace_heading",
            Self::InsertHeading { .. } => "insert_heading",
            Self::OmitHeading => "omit_heading",
            Self::RenumberHeadings => "renumber_headings",
            Self::Unparsed { .. } => "unparsed",
        }
    }

    #[must_use]
    pub fn column(&self) -> Option<Column> {
        match self {
            Self::Omit { col, .. }
            | Self::Insert { col, .. }
            | Self::Substitute { col, .. }
            | Self::Renumber { col, .. }
            | Self::ReplaceColumn { col, .. } => Some(*col),
            _ => None,
        }
    }
}

/// One change, with where it is printed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    /// `(2)(f)`: the clause and sub-clause labels as printed.
    pub clause: String,
    /// The heading changed; for an inserted heading, the one it goes after.
    pub heading: Option<HeadingRef>,
    /// `si` or `ta` for a change made "only in Sinhala (Tamil) Notification":
    /// read, but not applied to the English schedule.
    pub lang_only: Option<String>,
    pub op: Op,
    pub page: i32,
    /// The sentence as printed.
    pub text: String,
}

/// Everything read from an amending gazette.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Amendment {
    pub changes: Vec<Change>,
    pub warnings: Vec<String>,
}

// ─────────────────────────── patterns ───────────────────────────────────────

fn re(p: &str) -> Regex {
    Regex::new(p).expect("amendment regex")
}

/// `(2) With reference …`, `(3) By insertion …`, `(03) The Heading …`.
static CLAUSE: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^\((\d{1,2})\)\s*(.*)$"));
static CLAUSE_LEAD: LazyLock<Regex> =
    LazyLock::new(|| re(r#"(?i)^(with\s+reference|by\b|the\s+heading|"|accordingly)"#));
/// `(a) In Column II thereof, …` (a stray number may come first: `(a) 6 In Column I`).
static SUB: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^\(([a-z]{1,2})\)\s*(.*)$"));
static SUB_LEAD: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)^(\d{1,3}\s+)?(in\s+column|by\b|the\s+heading|accordingly|re-?\s?numbering)")
});
static RENUMBERING: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\bre\s*-?\s*numbering\b"));
static UNLETTERED: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)^by\s+(?:the\s+)?(omission|omitting|insertion|inserting|substitution|re-?\s?numbering|deleting|deletion|addition)\b",
    )
});
static ITEM_LEAD: LazyLock<Regex> = LazyLock::new(|| re(r"^(\d{1,3})\s*[.)]?\s+(\S.*)$"));
static SCHEDULE_WORD: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^\s*schedule\s*:?-?\s*$"));
static COLUMN_HEADER: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)\bcolumn\s+i\b.*\bcolumn\s+ii\b.*\bcolumn\s+iii\b"));
/// Running heads, page labels and print codes.
static NOISE: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)^(\d{1,3}\s*a\b|part\s+i\s*:\s*sec|i\s+fldgi|.*\bgazette\s+extraordinary\s+of\s+the\s+democratic\b|.*\bfldgi\s*\()",
    )
});
/// Print lines at the foot of a page (anchored: an item can name the
/// Department of Government Printing).
static FOOT: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)^(printed\s+at\b|eog\s+\d)|this\s+gazette\s+extraordinary\s+can\s+be\s+downloaded|printed\s+at\s+the\s+department\s+of\s+government\s+printing",
    )
});
static COLUMN: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\bcolumns?\s+(iii|ii|i|1|2|3)\b"));
static LANG_ONLY: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)only\s+in\s+(sinhala|tamil)"));

/// Phrases matched while reading clauses (compiled once).
static RENUMBER_HEADINGS: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)re-?\s?numbering\s+(?:of\s+)?the\s+headings?"));
static SUBSTITUTE_ALL: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)substitut\w*\s+(?:of\s+)?(?:the\s+)?following\s+items\s+for\s+(?:all\s+)?the\s+items")
});
static FROM_ONWARD: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\bfrom\s+\d+\s*[;:,.]?\s*$"));
static WHOLE_COLUMN: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)(items|institutions|functions)\s+(listed\s+)?in\s+column"));
static AFTER_WORD: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\bafter\b"));
static NEW_HEADINGS: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)\b(insertion|addition)\s+of\s+the\s+following\s+new\s+headings?"));
static OMIT_HEADING: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)\b(removal|omission|omitting|deleting|deletion)\s+of\s+the\s+heading")
});

/// Quotes, dashes and spacing made plain, case kept.
fn plain(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| match c {
            '“' | '”' | '„' | '″' | '‟' => '"',
            '‘' | '’' | '`' | '′' => '\'',
            '–' | '—' => '-',
            '\t' => ' ',
            _ => c,
        })
        .collect();
    // two single quotes are printed as a double quote (`''(8) Minister …''`)
    clean(&t.replace("''", "\""))
}

fn ended(sentence: &str) -> bool {
    let t = sentence.trim_end();
    let l = t.to_lowercase();
    // a quote still open ("after the item "Welfare Benefits Act,") carries on
    if t.matches('"').count() % 2 == 1 {
        return false;
    }
    t.ends_with([';', ':', ',', '-', '·'])
        || l.ends_with("following item")
        || l.ends_with("following items")
        || l.ends_with("as follows")
        || l.ends_with("columns i, ii and iii")
}

// ─────────────────────────── lines and table regions ────────────────────────

struct Line {
    page: u32,
    text: String,
    bullet: bool,
}

enum Block {
    Line(Line),
    Table(Vec<Portfolio>),
}

fn page_i32(p: u32) -> i32 {
    i32::try_from(p).unwrap_or(i32::MAX)
}

/// Rows of the schedule, in reading order, with tables already read.
fn blocks(words: &Words, warnings: &mut Vec<String>) -> Vec<Block> {
    struct R<'a> {
        page: u32,
        text: String,
        words: Vec<&'a Word>,
    }
    let mut all: Vec<R<'_>> = Vec::new();
    for p in &words.pages {
        for r in rows(words.on_page(p.page)) {
            let text = plain(&r.text());
            if text.is_empty() || NOISE.is_match(&text) || FOOT.is_match(&text) {
                continue;
            }
            all.push(R {
                page: p.page,
                text,
                words: r.words,
            });
        }
    }
    // the schedule starts after its title; without one, after the notification
    let start = all
        .iter()
        .position(|r| SCHEDULE_WORD.is_match(&r.text))
        .map_or(0, |i| i + 1);

    let mut out: Vec<Block> = Vec::new();
    let mut table: Option<Vec<Word>> = None;
    let flush =
        |table: &mut Option<Vec<Word>>, out: &mut Vec<Block>, warnings: &mut Vec<String>| {
            if let Some(ws) = table.take() {
                let sched = schedule::parse(&Words {
                    pages: words.pages.clone(),
                    words: ws,
                });
                warnings.extend(sched.warnings.iter().map(|w| format!("table: {w}")));
                out.push(Block::Table(sched.portfolios));
            }
        };
    let mut i = start;
    while i < all.len() {
        let r = &all[i];
        let clause = CLAUSE
            .captures(&r.text)
            .is_some_and(|c| CLAUSE_LEAD.is_match(&c[2]));
        if clause {
            flush(&mut table, &mut out, warnings);
        } else {
            // a heading with a column header just below it opens (or continues) a table
            let opens = HEADING.is_match(&r.text)
                && all[i + 1..]
                    .iter()
                    .take(4)
                    .any(|n| n.page == r.page && COLUMN_HEADER.is_match(&n.text));
            if table.is_none() && opens {
                table = Some(Vec::new());
            } else if table.is_none() && COLUMN_HEADER.is_match(&r.text) {
                // columns printed without a heading ("by omission of the items in
                // Columns I, II and III thereof and insertion of the following
                // items"): the table parser reads them under a stand-in heading
                let (x0, y0) = r.words.iter().fold((f32::MAX, f32::MAX), |(x, y), w| {
                    (x.min(w.bbox[0]), y.min(w.bbox[1]))
                });
                let x0 = x0.min(120.0);
                let word = |text: &str, k: f32| Word {
                    page: r.page,
                    text: text.to_owned(),
                    bbox: [x0 + k * 40.0, y0 - 18.0, x0 + k * 40.0 + 30.0, y0 - 8.0],
                    conf: None,
                };
                table = Some(vec![
                    word("01.", 0.0),
                    word("Minister", 1.0),
                    word("of", 2.0),
                    word(STAND_IN, 3.0),
                ]);
            }
        }
        if let Some(ws) = table.as_mut() {
            ws.extend(r.words.iter().map(|w| (*w).clone()));
        } else {
            let bullet = r.words.first().is_some_and(|w| {
                !w.text.is_empty() && w.text.chars().all(|c| is_bullet_token(&c.to_string()))
            });
            let text = if bullet {
                plain(
                    &r.words[1..]
                        .iter()
                        .map(|w| w.text.as_str())
                        .collect::<Vec<_>>()
                        .join(" "),
                )
            } else {
                r.text.clone()
            };
            out.push(Block::Line(Line {
                page: r.page,
                text,
                bullet,
            }));
        }
        i += 1;
    }
    flush(&mut table, &mut out, warnings);
    out
}

/// The title the table parser is given for columns printed without a heading.
const STAND_IN: &str = "(columns-only)";

// ─────────────────────────── sentences ──────────────────────────────────────

#[derive(Default)]
struct Sentence {
    clause: String,
    sub: Option<String>,
    page: i32,
    text: String,
    payload: Vec<NewItem>,
    tables: Vec<Portfolio>,
}

impl Sentence {
    fn label(&self) -> String {
        match &self.sub {
            Some(s) if !s.is_empty() => format!("({}) ({s})", self.clause),
            _ => format!("({})", self.clause),
        }
    }
}

fn sentences(blocks: Vec<Block>, warnings: &mut Vec<String>) -> (Vec<Sentence>, Vec<Portfolio>) {
    let mut out: Vec<Sentence> = Vec::new();
    // tables printed before any clause (a schedule made only of headings)
    let mut loose: Vec<Portfolio> = Vec::new();
    for b in blocks {
        let l = match b {
            Block::Table(ps) => {
                match out.last_mut() {
                    Some(s) => s.tables.extend(ps),
                    None => loose.extend(ps),
                }
                continue;
            }
            Block::Line(l) => l,
        };
        let page = page_i32(l.page);
        if !l.bullet
            && let Some(c) = CLAUSE.captures(&l.text)
            && CLAUSE_LEAD.is_match(&c[2])
        {
            out.push(Sentence {
                clause: c[1].trim_start_matches('0').to_owned(),
                page,
                text: c[2].to_owned(),
                ..Sentence::default()
            });
            continue;
        }
        let Some(cur) = out.last_mut() else {
            continue;
        };
        if !l.bullet
            && let Some(c) = SUB.captures(&l.text)
            && SUB_LEAD.is_match(&c[2])
        {
            let clause = cur.clause.clone();
            out.push(Sentence {
                clause,
                sub: Some(c[1].to_lowercase()),
                page,
                text: c[2].to_owned(),
                ..Sentence::default()
            });
            continue;
        }
        // an unlettered change under a clause ("By omission of the said Heading …")
        if !l.bullet
            && cur.sub.is_none()
            && ended(&cur.text)
            && cur.payload.is_empty()
            && cur.tables.is_empty()
            && UNLETTERED.is_match(&l.text)
        {
            let clause = cur.clause.clone();
            out.push(Sentence {
                clause,
                sub: Some(String::new()),
                page,
                text: l.text.clone(),
                ..Sentence::default()
            });
            continue;
        }
        if !ended(&cur.text) && cur.payload.is_empty() && cur.tables.is_empty() {
            append(&mut cur.text, &l.text);
            continue;
        }
        // the items printed under the sentence
        let t = l.text.trim_start_matches(['"', '\'', ' ']).to_owned();
        if l.bullet {
            cur.payload.push(NewItem {
                item_no: None,
                text: t,
                page,
            });
        } else if let Some(c) = ITEM_LEAD.captures(&t)
            && let Ok(n) = c[1].parse::<i32>()
            && (1..=400).contains(&n)
            && !cur.payload.last().is_some_and(|p| {
                p.item_no.is_none() && !p.text.is_empty() && !schedule::law_complete(&p.text)
            })
        {
            cur.payload.push(NewItem {
                item_no: Some(n),
                text: c[2].to_owned(),
                page,
            });
        } else if let Some(last) = cur.payload.last_mut() {
            append(&mut last.text, &t);
        } else {
            cur.payload.push(NewItem {
                item_no: None,
                text: t,
                page,
            });
        }
    }
    for s in &mut out {
        for p in &mut s.payload {
            p.text = p
                .text
                .trim_end_matches(['"', '\'', ' ', ';', ','])
                .trim_end_matches(['"', '\''])
                .trim()
                .to_owned();
        }
        s.payload.retain(|p| !p.text.is_empty());
    }
    if out.is_empty() && loose.is_empty() {
        warnings.push("no clause or heading table was found after the schedule title".to_owned());
    }
    (out, loose)
}

// ─────────────────────────── reading a sentence ─────────────────────────────

/// The numbers in `1, 3 and 5 to 8` (ranges expanded).
#[must_use]
pub fn numbers(s: &str) -> Vec<i32> {
    static TOK: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\d+|\bto\b"));
    let mut out: Vec<i32> = Vec::new();
    let mut range = false;
    for m in TOK.find_iter(s) {
        let t = m.as_str();
        if t.eq_ignore_ascii_case("to") {
            range = !out.is_empty();
            continue;
        }
        let Ok(n) = t.parse::<i32>() else { continue };
        if range
            && let Some(&a) = out.last()
            && n > a
            && n - a < 400
        {
            out.extend(a + 1..=n);
        } else {
            out.push(n);
        }
        range = false;
    }
    out
}

/// `re-numbering of item numbers from 19 to 26 respectively, as numbers 18 to
/// 25 and item numbers 41 and 42 respectively, as numbers 26 and 27`.
fn renumbering(l: &str) -> (Vec<i32>, Option<Vec<i32>>) {
    static AS: LazyLock<Regex> =
        LazyLock::new(|| re(r"(?i)^\s*,?\s*as\s+(?:item\s+)?(?:numbers?|nos?\.?)?\s*"));
    static TO_LIST: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(?:\s*(?:\d+|to|and|,)\s*)+"));
    let Some(start) = l.find("number") else {
        return (Vec::new(), None);
    };
    let mut rest = &l[start..];
    let (mut from, mut to) = (Vec::new(), Vec::new());
    let mut any = false;
    while let Some(k) = rest.find("respectively") {
        any = true;
        from.extend(numbers(&rest[..k]));
        let after = &rest[k + "respectively".len()..];
        let after = AS.find(after).map_or(after, |m| &after[m.end()..]);
        let list = TO_LIST.find(after).map_or("", |m| m.as_str());
        // a list that ends on "and" belongs to the next group ("… 25 and item numbers 41 …")
        let list = list
            .trim_end()
            .trim_end_matches("and")
            .trim_end_matches(',');
        to.extend(numbers(list));
        rest = &after[list.len()..];
    }
    if any {
        (from, Some(to))
    } else {
        (numbers(rest), None)
    }
}

/// Every closed `"…"` in a sentence, in order.
fn quotes_in(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(a) = rest.find('"') {
        let after = &rest[a + 1..];
        let Some(b) = after.find('"') else { break };
        let t = after[..b].trim().trim_end_matches([';', ',', '.']).trim();
        if !t.is_empty() {
            out.push(t.to_owned());
        }
        rest = &after[b + 1..];
    }
    out
}

fn quoted(s: &str) -> Option<String> {
    let a = s.find('"')?;
    let rest = &s[a + 1..];
    let b = rest.find('"').unwrap_or(rest.len());
    let t = rest[..b].trim().trim_end_matches(';').trim();
    (!t.is_empty()).then(|| t.to_owned())
}

/// The heading a clause names: `Heading, No. "02. Minister of Finance"`,
/// `Heading (1) Minister of Defence of the said notification`,
/// `Heading : Minister of Finance (No. 16)`.
#[must_use]
pub fn heading_ref(text: &str) -> Option<HeadingRef> {
    static QUOTED: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r#"(?i)heading[^"]{0,25}"\s*\(?\s*(?:no\.?\s*)?\(?(\d{1,3})?\)?\s*[.),]?\s*(minist[^"]+?)\s*""#,
        )
    });
    static COLON_NO: LazyLock<Regex> = LazyLock::new(|| {
        re(r#"(?i)heading\s*[:,]?\s*"?\s*(minist[^"]+?)\s*"?\s*\(\s*no\.?\s*(\d{1,3})\s*\)"#)
    });
    static BARE: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r#"(?i)heading\s*[:,]?\s*(?:no\.?\s*)?\(?(\d{1,3})?\)?\.?\s*"?\s*(minist[^"]+?)"?\s+(?:of\s+the\s+said|in\s+that|thereof)"#,
        )
    });
    let t = plain(text);
    let title = |s: &str| {
        clean(s)
            .trim_end_matches([',', '.', ';', ':', ' '])
            .to_owned()
    };
    // the first heading the text names, whichever form it takes
    let mut found: Vec<(usize, HeadingRef)> = Vec::new();
    if let Some(c) = QUOTED.captures(&t) {
        found.push((
            c.get(0).map_or(0, |m| m.start()),
            HeadingRef {
                no: c.get(1).and_then(|m| m.as_str().parse().ok()),
                title: Some(title(&c[2])),
            },
        ));
    }
    if let Some(c) = COLON_NO.captures(&t) {
        found.push((
            c.get(0).map_or(0, |m| m.start()),
            HeadingRef {
                no: c[2].parse().ok(),
                title: Some(title(&c[1])),
            },
        ));
    }
    if let Some(c) = BARE.captures(&t) {
        found.push((
            c.get(0).map_or(0, |m| m.start()),
            HeadingRef {
                no: c.get(1).and_then(|m| m.as_str().parse().ok()),
                title: Some(title(&c[2])),
            },
        ));
    }
    found.sort_by_key(|f| f.0);
    // a number printed after the title, inside the quotes: "Minister of … (No. 31)"
    static TRAILING_NO: LazyLock<Regex> =
        LazyLock::new(|| re(r"(?i)^(.*?)\s*\(\s*no\.?\s*(\d{1,3})\s*\)$"));
    found.into_iter().next().map(|(_, mut h)| {
        if let Some(t) = h.title.clone()
            && let Some(c) = TRAILING_NO.captures(&t)
        {
            h.no = h.no.or_else(|| c[2].parse().ok());
            h.title = Some(c[1].to_owned());
        }
        h
    })
}

fn column_of(l: &str) -> Option<Column> {
    let c = COLUMN.captures(l)?;
    Some(match &c[1].to_lowercase()[..] {
        "i" | "1" => Column::Function,
        "ii" | "2" => Column::Institution,
        _ => Column::Law,
    })
}

fn unparsed(reason: impl Into<String>) -> Op {
    Op::Unparsed {
        reason: reason.into(),
    }
}

/// "by substitution of following items instead of items No. 06, 10, 11, 12
/// and 13 respectively": one substitution per item, in order.
fn multi_substitute(s: &Sentence) -> Option<Vec<Op>> {
    static ITEMS: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"(?i)substitut\w*\s+(?:of\s+(?:the\s+)?following\s+items\s*,?\s*instead\s+of|for)\s+(?:the\s+)?items\s*(?:no\.?|numbers?|from)?\s*((?:\d+|\s|,|and|to)+)",
        )
    });
    let l = plain(&s.text).to_lowercase();
    let col = column_of(&l)?;
    let nos = numbers(&ITEMS.captures(&l)?[1]);
    if nos.len() < 2 || nos.len() != s.payload.len() {
        return None;
    }
    Some(
        nos.into_iter()
            .zip(s.payload.iter().cloned())
            .map(|(no, item)| Op::Substitute {
                col,
                target: Target::No { no },
                item,
            })
            .collect(),
    )
}

/// Reads a lettered sub-clause (a change to one heading).
fn sub_op(s: &Sentence) -> Op {
    static RENAME: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r#"(?i)the\s+heading\s+(?:thereof\s+)?as\s*"\s*(?:no\.?\s*)?\(?\d{0,3}\)?\s*[.)]?\s*([^"]+?)\s*""#,
        )
    });
    static SUBST_NO: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"(?i)(?:omitting|omission\s+of)\s+(?:the\s+)?item\s*(?:no\.?|number)?\s*(\d+)\s+and\s+(?:the\s+)?insertion",
        )
    });
    static INSTEAD_NO: LazyLock<Regex> = LazyLock::new(|| {
        re(r"(?i)(?:instead\s+of|for)\s+(?:the\s+)?item\s*(?:no\.?|number)?\s*(\d+)")
    });
    static INSTEAD_TEXT: LazyLock<Regex> =
        LazyLock::new(|| re(r#"(?i)(?:instead\s+of|for)\s+(?:the\s+)?item\s*""#));
    static AFTER_TEXT: LazyLock<Regex> = LazyLock::new(|| re(r#"(?i)after\s+(?:the\s+)?item\s*""#));
    static AFTER_NO: LazyLock<Regex> =
        LazyLock::new(|| re(r"(?i)after\s+(?:the\s+)?items?\s*(?:no\.?|numbers?)?\s*(\d+)"));
    static BEFORE_NO: LazyLock<Regex> =
        LazyLock::new(|| re(r"(?i)before\s+(?:the\s+)?items?\s*(?:no\.?|numbers?)?\s*(\d+)"));
    static ITEM_AS: LazyLock<Regex> = LazyLock::new(|| {
        re(r"(?i)(?:\bitem\s+(\d+|i|l)\s+(?:is\s+amended\s+)?as\b|amendment\s+of\s+item\s+(\d+))")
    });
    static OMIT: LazyLock<Regex> =
        LazyLock::new(|| re(r"(?i)\b(omitting|omission|deleting|deletion|removal|removing)\b"));
    static INSERT: LazyLock<Regex> =
        LazyLock::new(|| re(r"(?i)\b(insertion|inserting|addition|adding)\b"));

    let t = plain(&s.text);
    let l = t.to_lowercase();
    if let Some(c) = RENAME.captures(&t) {
        return Op::RenameHeading {
            title: clean(&c[1]),
        };
    }
    if l.contains("omission of the said heading") || l.contains("omitting the said heading") {
        if !l.contains("insertion") && s.tables.is_empty() {
            return Op::OmitHeading;
        }
        return match s.tables.as_slice() {
            [p] => Op::ReplaceHeading {
                portfolio: p.clone(),
            },
            [] => unparsed("replaces the heading, but no heading table was read"),
            _ => unparsed("replaces the heading with more than one table"),
        };
    }
    if RENUMBER_HEADINGS.is_match(&l) {
        return Op::RenumberHeadings;
    }
    if l.contains("columns i, ii and iii")
        && (l.contains("insertion") || l.contains("substitution"))
    {
        return match s.tables.as_slice() {
            [p] => Op::ReplaceColumns {
                portfolio: p.clone(),
            },
            [] => unparsed("replaces the three columns, but no table was read"),
            _ => unparsed("replaces the three columns with more than one table"),
        };
    }
    let Some(col) = column_of(&l) else {
        return unparsed("names no column");
    };
    if l.contains("change as follows") || SUBSTITUTE_ALL.is_match(&l) {
        return if s.payload.is_empty() {
            unparsed("replaces the column, but no items are printed")
        } else {
            Op::ReplaceColumn {
                col,
                items: s.payload.clone(),
            }
        };
    }
    let one = |what: &str| -> Result<NewItem, Op> {
        match s.payload.as_slice() {
            [p] => Ok(p.clone()),
            [] => Err(unparsed(format!("{what}, but no item is printed"))),
            _ => Err(unparsed(format!(
                "{what} with {} items printed",
                s.payload.len()
            ))),
        }
    };
    if let Some(c) = ITEM_AS.captures(&l) {
        let no = match c.get(1).or_else(|| c.get(2)).map_or("", |m| m.as_str()) {
            "i" | "l" => 1,
            n => n.parse().unwrap_or(0),
        };
        return match one("rewords an item") {
            Ok(item) => Op::Substitute {
                col,
                target: Target::No { no },
                item,
            },
            Err(e) => e,
        };
    }
    if let Some(c) = SUBST_NO.captures(&l) {
        let no = c[1].parse().unwrap_or(0);
        return match one("substitutes an item") {
            Ok(item) => Op::Substitute {
                col,
                target: Target::No { no },
                item,
            },
            Err(e) => e,
        };
    }
    if l.contains("substitution") || l.contains("substituting") {
        let target = if let Some(m) = INSTEAD_TEXT.find(&t) {
            quoted(&t[m.end() - 1..]).map(|text| Target::Text { text })
        } else {
            INSTEAD_NO
                .captures(&l)
                .and_then(|c| c[1].parse().ok())
                .map(|no| Target::No { no })
        };
        let Some(target) = target else {
            return unparsed("substitutes an item, but names none");
        };
        return match one("substitutes an item") {
            Ok(item) => Op::Substitute { col, target, item },
            Err(e) => e,
        };
    }
    if RENUMBERING.is_match(&l) {
        let l = RENUMBERING.replace_all(&l, "re-numbering").into_owned();
        let (from, to) = renumbering(&l);
        // "re-numbering the items from 22": that item and the ones after it
        let onward = to.is_none()
            && from.len() == 1
            && FROM_ONWARD.is_match(l.trim_end_matches([';', ' ', '.']));
        if from.is_empty() {
            // "re-numbering Institutions listed in Column II": the whole column
            return if WHOLE_COLUMN.is_match(&l) {
                Op::Renumber {
                    col,
                    from,
                    to: None,
                    onward: true,
                }
            } else {
                unparsed("renumbers, but names no items")
            };
        }
        return Op::Renumber {
            col,
            from,
            to,
            onward,
        };
    }
    if let Some(m) = OMIT.find(&l) {
        // Column III has no numbers: the figures in its sentences are act numbers
        let nos = if col == Column::Law {
            Vec::new()
        } else {
            numbers(&l[m.end()..])
        };
        let mut quoted = s.payload.clone();
        if quoted.is_empty() && col == Column::Law {
            // "by the omission of "Police Ordinance"" / "the item which reads as "…""
            quoted = quotes_in(&t[m.start().min(t.len())..])
                .into_iter()
                .map(|text| NewItem {
                    item_no: None,
                    text,
                    page: s.page,
                })
                .collect();
        }
        let mut nos = nos;
        if nos.is_empty() {
            nos = quoted.iter().filter_map(|q| q.item_no).collect();
        }
        if nos.is_empty() && quoted.is_empty() {
            return unparsed("omits, but names no items");
        }
        return Op::Omit { col, nos, quoted };
    }
    if INSERT.is_match(&l) {
        if s.payload.is_empty() && col == Column::Law {
            // "… immediately after the words : "A Act" "B Act"": after A, insert B
            if let Some(m) = AFTER_WORD.find(&t) {
                let qs = quotes_in(&t[m.end()..]);
                if qs.len() >= 2 {
                    return Op::Insert {
                        col,
                        anchor: Anchor::AfterText {
                            text: qs[0].clone(),
                        },
                        items: qs[1..]
                            .iter()
                            .map(|q| NewItem {
                                item_no: None,
                                text: q.clone(),
                                page: s.page,
                            })
                            .collect(),
                    };
                }
            }
        }
        if s.payload.is_empty() {
            return unparsed("inserts, but no items are printed");
        }
        let anchor = if let Some(m) = AFTER_TEXT.find(&t) {
            match quoted(&t[m.end() - 1..]) {
                Some(text) => Anchor::AfterText { text },
                None => return unparsed("inserts after an item whose text is not read"),
            }
        } else if let Some(c) = AFTER_NO.captures(&l) {
            Anchor::After {
                no: c[1].parse().unwrap_or(0),
            }
        } else if let Some(c) = BEFORE_NO.captures(&l) {
            Anchor::Before {
                no: c[1].parse().unwrap_or(0),
            }
        } else {
            Anchor::End
        };
        return Op::Insert {
            col,
            anchor,
            items: s.payload.clone(),
        };
    }
    unparsed(format!(
        "a change to Column {} that is not read",
        col.roman()
    ))
}

/// Reads the changes of an amending gazette.
#[must_use]
pub fn parse(words: &Words) -> Amendment {
    let mut am = Amendment::default();
    let bl = blocks(words, &mut am.warnings);
    let (ss, loose) = sentences(bl, &mut am.warnings);

    // a schedule made only of headings replaces each of them
    for p in loose {
        am.changes.push(Change {
            clause: format!("(heading {:02})", p.seq),
            heading: Some(HeadingRef {
                no: Some(p.seq),
                title: Some(p.title.clone()),
            }),
            lang_only: None,
            page: p.first_page,
            text: format!("{:02}. {} (printed in full)", p.seq, p.title),
            op: Op::ReplaceHeading { portfolio: p },
        });
    }

    if ss.is_empty() && am.changes.is_empty() {
        am.changes.push(Change {
            clause: "(schedule)".to_owned(),
            heading: None,
            lang_only: None,
            op: unparsed(
                "no numbered clause or heading table was read (a schedule printed in the 2020 policy-statement layout is not read yet)",
            ),
            page: 1,
            text: String::new(),
        });
    }

    let mut heading: Option<HeadingRef> = None;
    let mut clause_no = String::new();
    // tables after a clause whose change is made by its sub-clauses belong to the clause
    for (i, s) in ss.iter().enumerate() {
        if s.clause != clause_no {
            clause_no.clone_from(&s.clause);
            heading = None;
        }
        let t = plain(&s.text);
        let l = t.to_lowercase();
        let lang_only = LANG_ONLY.captures(&l).map(|c| {
            if c[1].starts_with('s') {
                "si".to_owned()
            } else {
                "ta".to_owned()
            }
        });
        let push = |am: &mut Amendment, heading: Option<HeadingRef>, op: Op| {
            am.changes.push(Change {
                clause: s.label(),
                heading,
                lang_only: lang_only.clone(),
                op,
                page: s.page,
                text: t.clone(),
            });
        };
        if s.sub.is_none() {
            // the tables of this clause and its sub-clauses
            let tables: Vec<Portfolio> = ss[i..]
                .iter()
                .take_while(|x| x.clause == s.clause)
                .flat_map(|x| x.tables.iter().cloned())
                .collect();
            let has_subs = ss.get(i + 1).is_some_and(|x| x.clause == s.clause);
            if NEW_HEADINGS.is_match(&l) {
                let after = heading_ref(&t);
                if tables.is_empty() {
                    push(
                        &mut am,
                        after.clone(),
                        unparsed("inserts a heading, but no heading table was read"),
                    );
                }
                for p in tables {
                    push(&mut am, after.clone(), Op::InsertHeading { portfolio: p });
                }
                heading = None;
                continue;
            }
            if l.contains("substitution for the heading") {
                let target = heading_ref(&t);
                match tables.as_slice() {
                    [p] => push(
                        &mut am,
                        target,
                        Op::ReplaceHeading {
                            portfolio: p.clone(),
                        },
                    ),
                    _ => push(
                        &mut am,
                        target,
                        unparsed(format!("replaces a heading with {} tables", tables.len())),
                    ),
                }
                continue;
            }
            if OMIT_HEADING.is_match(&l) && !l.contains("insertion") {
                push(&mut am, heading_ref(&t), Op::OmitHeading);
                continue;
            }
            if RENUMBERING.is_match(&l) && l.contains("heading") && !l.contains("with reference") {
                push(&mut am, None, Op::RenumberHeadings);
                continue;
            }
            if l.contains("with reference to the heading") {
                heading = heading_ref(&t);
                if heading.is_none() {
                    push(&mut am, None, unparsed("names a heading that is not read"));
                } else if !has_subs {
                    push(
                        &mut am,
                        heading.clone(),
                        unparsed("names a heading but makes no change that is read"),
                    );
                }
                continue;
            }
            push(
                &mut am,
                heading_ref(&t),
                unparsed("a clause that is not read"),
            );
            continue;
        }
        for op in multi_substitute(s).unwrap_or_else(|| vec![sub_op(s)]) {
            push(&mut am, heading.clone(), op);
        }
    }
    am
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_lists_and_ranges() {
        assert_eq!(
            numbers("item number 18 and item numbers 27 to 30;"),
            vec![18, 27, 28, 29, 30]
        );
        assert_eq!(numbers("items 7 and 13"), vec![7, 13]);
        assert_eq!(numbers("item 6, 8, 9 and 12"), vec![6, 8, 9, 12]);
        assert_eq!(numbers("from 36 to 38"), vec![36, 37, 38]);
    }

    #[test]
    fn renumbering_pairs_each_group() {
        let (f, t) = renumbering(
            "re-numbering of item numbers from 19 to 21 respectively, as numbers 18 to 20 and item numbers 41 and 42 respectively, as numbers 26 and 27;",
        );
        assert_eq!(f, vec![19, 20, 21, 41, 42]);
        assert_eq!(t, Some(vec![18, 19, 20, 26, 27]));
        let (f, t) = renumbering(
            "re-numbering of item numbers from 3 to 6, numbers 10 to 13 respectively, as numbers 2 to 9;",
        );
        assert_eq!(f, vec![3, 4, 5, 6, 10, 11, 12, 13]);
        assert_eq!(t, Some((2..=9).collect()));
        // no new numbers: numbered on from the item before
        let (f, t) = renumbering("by re-numbering the items 21 and 22");
        assert_eq!((f, t), (vec![21, 22], None));
        let (f, t) =
            renumbering("re-numbering of items 56 to 58 respectively, as numbers 57 to 59;");
        assert_eq!((f, t), (vec![56, 57, 58], Some(vec![57, 58, 59])));
    }

    #[test]
    fn headings_are_read_in_every_form() {
        let h = heading_ref(r#"With reference to the Heading, No. “02. Minister of Finance, Planning and Economic Development” of the said notification"#).expect("ref");
        assert_eq!(h.no, Some(2));
        assert_eq!(
            h.title.as_deref(),
            Some("Minister of Finance, Planning and Economic Development")
        );
        let h = heading_ref("With reference to the Heading (1) Minister of Defence of the said notification, as follows").expect("ref");
        assert_eq!(
            (h.no, h.title.as_deref()),
            (Some(1), Some("Minister of Defence"))
        );
        let h = heading_ref("With reference to the Heading : Minister of National Policies & Economic Affairs (No. 4) of the said Notification").expect("ref");
        assert_eq!(
            (h.no, h.title.as_deref()),
            (
                Some(4),
                Some("Minister of National Policies & Economic Affairs")
            )
        );
        let h = heading_ref(r#"By insertion of the following new Heading after Columns I, II and III of the Heading No. “28. Minister of Women, Child Affairs and Social Empowerment” of the said notification;"#).expect("ref");
        assert_eq!(h.no, Some(28));
        // the older forms: a comma, the number in brackets after the title, two single quotes
        let h = heading_ref("With reference to the heading, Minister of Tourism and Sports (No. 21) of the said Notification").expect("ref");
        assert_eq!(
            (h.no, h.title.as_deref()),
            (Some(21), Some("Minister of Tourism and Sports"))
        );
        let h = heading_ref(r#"With reference to the Heading "(No. 1) Minister of Defence" of the said notification"#).expect("ref");
        assert_eq!(
            (h.no, h.title.as_deref()),
            (Some(1), Some("Minister of Defence"))
        );
        let h = heading_ref("With reference to the Heading, No. (4) Minister of National Policies & Economic Affairs of the said Notification").expect("ref");
        assert_eq!(
            (h.no, h.title.as_deref()),
            (
                Some(4),
                Some("Minister of National Policies & Economic Affairs")
            )
        );
        let h = heading_ref("with reference to the heading, ''Minister of Muslim Religious Affairs and Posts (No. 31)'' of the said Notification").expect("ref");
        assert_eq!(
            (h.no, h.title.as_deref()),
            (
                Some(31),
                Some("Minister of Muslim Religious Affairs and Posts")
            )
        );
        let h = heading_ref(r#"With reference to the heading, “No. 01. Minister of Defence ” of the said notification"#).expect("ref");
        assert_eq!(
            (h.no, h.title.as_deref()),
            (Some(1), Some("Minister of Defence"))
        );
    }

    #[test]
    #[ignore = "development aid: LANKA_CIVIC_PDF=/path/to.pdf cargo test -p lanka-ingest dump_amendment -- --ignored --nocapture"]
    fn dump_amendment() {
        let pdf = std::env::var("LANKA_CIVIC_PDF").expect("LANKA_CIVIC_PDF");
        let out = std::process::Command::new("pdftotext")
            .args(["-bbox-layout", &pdf, "-"])
            .output()
            .expect("pdftotext");
        let words =
            lanka_extractd::words::parse_bbox_layout(&String::from_utf8_lossy(&out.stdout), 1)
                .expect("bbox");
        let am = parse(&words);
        for c in &am.changes {
            let h = c
                .heading
                .as_ref()
                .map_or_else(String::new, ToString::to_string);
            let detail = match &c.op {
                Op::InsertHeading { portfolio: p } | Op::ReplaceHeading { portfolio: p } => {
                    format!(
                        "{:02}. {} fn {} inst {} law {}",
                        p.seq,
                        p.title,
                        p.functions.len(),
                        p.institutions.len(),
                        p.laws.len()
                    )
                }
                op => serde_json::to_string(op).unwrap_or_default(),
            };
            println!(
                "{:<10} p{:<2} {:<16} {}{} | {}",
                c.clause,
                c.page,
                c.op.kind(),
                c.lang_only
                    .as_deref()
                    .map_or(String::new(), |l| format!("[{l} only] ")),
                h,
                detail
            );
        }
        for w in &am.warnings {
            println!("WARN {w}");
        }
    }
}
