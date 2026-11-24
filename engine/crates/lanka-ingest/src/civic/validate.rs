//! The validation checks of a parsed baseline (13 §2.1), stored in
//! `civic.instruments.checks` and shown to reviewers. All passing means the
//! instrument is `validated`; otherwise it waits for review.
//!
//! - `schedule_found`: at least one `Column I / II / III` table with a
//!   minister's heading was read (a gazette in another layout reads as none).
//! - `headings_sequential`: the heading numbers run 1..n with no gap.
//! - `items_sequential`: inside each heading, Column I and Column II numbers
//!   run 1..k with no gap.
//! - `institution_text_clean`: no institution's text holds an act citation
//!   (`No. 12`, `of 1979`, `Act,`/`Ordinance` ending or followed by `No`) or
//!   ends by repeating a word that is already in the name: both mean words
//!   of a neighbouring column were glued on.
//! - `headings_have_items`: every heading has at least one item.
//! - `heading_count`: the number of headings parsed equals the number of
//!   distinct `NN. Minister` headings found in the plain text.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use super::exceptions::{ExpectedCitation, ExpectedGap};
use super::schedule::{Item, Schedule};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    pub check: String,
    pub ok: bool,
    pub detail: String,
}

impl Check {
    fn new(check: &str, ok: bool, detail: impl Into<String>) -> Self {
        Self {
            check: check.to_owned(),
            ok,
            detail: detail.into(),
        }
    }
}

/// What a law column's text looks like: `No. 37`, `of 1991`, `… Act,`, `… Ordinance`.
static CITATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bNos?\.?\s*\d|\bof\s+(?:18|19|20)\d\d\b|\b(?:Act|Ordinance)\s*(?:,|$|No\b)|\bNo\.?\s*$|\s\d{1,4}$")
        .expect("citation regex")
});

/// Does the text of an institution show words of a neighbouring column?
#[must_use]
pub fn bleeds(text: &str) -> bool {
    if CITATION.is_match(text) {
        return true;
    }
    let words: Vec<String> = text
        .split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect();
    // a name that lists two things may repeat a word (`Secretariats and … Secretariats`)
    let lower = text.to_lowercase();
    if lower.contains(" and ")
        || lower.contains('&')
        || lower.contains(',')
        || lower.contains(" or ")
        || lower.contains('(')
    {
        return false;
    }
    const STOP: &[&str] = &["of", "and", "the", "for", "in", "on", "to", "its", "a"];
    match words.split_last() {
        Some((last, rest)) if rest.len() >= 3 => {
            last.chars().count() >= 3
                && !STOP.contains(&last.as_str())
                && rest.iter().any(|w| w == last)
        }
        _ => false,
    }
}

static TEXT_HEADING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?mi)^[ \t\f]*\(?(\d{1,3})(?:\.0[ \t]+(?:Foreign[ \t]+)?|(?:[ \t]*[.)][ \t]*|[ \t]+))Min(?:i)?st(?:er|ry)\b").expect("heading regex")
});

/// A line that is only a heading number: `(1)`, `01.`
static LONE_NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*\(?(\d{1,3})\)?\.?\s*$").expect("lone number regex"));
static MINISTRY_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^\s*Minist(?:er|ry)\s+of\b").expect("ministry line regex"));

/// The distinct heading numbers `NN. Minister …` found in the plain text of a
/// document (pages separated by `\f`). The text is in reading order of the
/// PDF's flow, which can put a heading's number and its words on separate
/// lines; a line holding only a number counts when `Minister of …` follows
/// within six lines.
#[must_use]
pub fn headings_in_text(text: &str) -> BTreeSet<i32> {
    let mut out: BTreeSet<i32> = TEXT_HEADING
        .captures_iter(text)
        .filter_map(|c| c[1].parse().ok())
        .collect();
    // a heading number is not far above the highest one found in one piece
    let ceiling = out.iter().next_back().copied().unwrap_or(0) + 1;
    let lines: Vec<&str> = text.lines().collect();
    for (i, l) in lines.iter().enumerate() {
        if let Some(n) = LONE_NUMBER
            .captures(l)
            .and_then(|c| c[1].parse::<i32>().ok())
            && n <= ceiling
            && lines
                .iter()
                .skip(i + 1)
                .take(6)
                .any(|n| MINISTRY_LINE.is_match(n))
        {
            out.insert(n);
        }
    }
    out
}

/// The first place where `nos` is not 1, 2, 3, … once the numbers in
/// `allowed` (printed gaps) are skipped (`None` when it is).
fn first_break(nos: impl IntoIterator<Item = i32>, allowed: &[i32]) -> Option<String> {
    let mut want = 1;
    for n in nos {
        while allowed.contains(&want) && n != want {
            want += 1;
        }
        if n != want {
            return Some(format!("expected {want}, found {n}"));
        }
        want += 1;
    }
    None
}

fn nos(items: &[Item]) -> impl Iterator<Item = i32> + '_ {
    items.iter().map(|i| i.item_no)
}

/// Runs the checks (and the parser's own warnings, as `parser_warnings`
/// information that never fails an instrument by itself).
#[must_use]
pub fn validate(
    sched: &Schedule,
    text: &str,
    gaps: &[ExpectedGap],
    citations: &[ExpectedCitation],
) -> Vec<Check> {
    let mut out = Vec::new();

    out.push(if sched.portfolios.is_empty() || sched.table_pages.is_empty() {
        Check::new(
            "schedule_found",
            false,
            "no Column I / II / III table with a minister's heading was found: this gazette uses another layout",
        )
    } else {
        Check::new(
            "schedule_found",
            true,
            format!("{} table pages", sched.table_pages.len()),
        )
    });

    // headings 1..n
    let h = first_break(sched.portfolios.iter().map(|p| p.seq), &[]);
    out.push(match &h {
        None => Check::new(
            "headings_sequential",
            true,
            format!(
                "{} headings, 1..{}",
                sched.portfolios.len(),
                sched.portfolios.len()
            ),
        ),
        Some(m) => Check::new("headings_sequential", false, m.clone()),
    });

    // columns I and II 1..k inside each heading (printed gaps from the exceptions file allowed)
    let allowed = |seq: i32, col: &str| -> Vec<i32> {
        gaps.iter()
            .filter(|g| g.heading == seq && g.column == col)
            .flat_map(|g| g.missing.iter().copied())
            .collect()
    };
    let mut bad = Vec::new();
    for p in &sched.portfolios {
        if let Some(m) = first_break(nos(&p.functions), &allowed(p.seq, "I")) {
            bad.push(format!("{:02} column I: {m}", p.seq));
        }
        if let Some(m) = first_break(nos(&p.institutions), &allowed(p.seq, "II")) {
            bad.push(format!("{:02} column II: {m}", p.seq));
        }
    }
    let notes: Vec<String> = gaps
        .iter()
        .map(|g| {
            format!(
                "{:02} column {} skips {:?} in the gazette ({})",
                g.heading, g.column, g.missing, g.note
            )
        })
        .collect();
    out.push(if bad.is_empty() {
        let mut d = "columns I and II run 1..k in every heading".to_owned();
        if !notes.is_empty() {
            d.push_str("; accepted by exceptions.yaml: ");
            d.push_str(&notes.join("; "));
        }
        Check::new("items_sequential", true, d)
    } else {
        Check::new("items_sequential", false, bad.join("; "))
    });

    // no words of a neighbouring column in the institutions
    let dirty: Vec<String> = sched
        .portfolios
        .iter()
        .flat_map(|p| {
            p.institutions
                .iter()
                .filter(|i| {
                    bleeds(&i.text)
                        && !citations
                            .iter()
                            .any(|c| c.heading == p.seq && c.item_no == i.item_no)
                })
                .map(move |i| {
                    format!(
                        "{:02}/{}: {}",
                        p.seq,
                        i.item_no,
                        i.text.chars().take(60).collect::<String>()
                    )
                })
        })
        .collect();
    out.push(if dirty.is_empty() {
        Check::new(
            "institution_text_clean",
            true,
            "no institution carries words of another column",
        )
    } else {
        let mut d = format!(
            "{} institution(s): {}",
            dirty.len(),
            dirty
                .iter()
                .take(5)
                .cloned()
                .collect::<Vec<_>>()
                .join(" | ")
        );
        if dirty.len() > 5 {
            d.push_str(" | …");
        }
        Check::new("institution_text_clean", false, d)
    });

    // every heading has an item
    let empty: Vec<String> = sched
        .portfolios
        .iter()
        .filter(|p| p.functions.is_empty() && p.institutions.is_empty() && p.laws.is_empty())
        .map(|p| format!("{:02}", p.seq))
        .collect();
    out.push(if empty.is_empty() {
        Check::new("headings_have_items", true, "every heading has items")
    } else {
        Check::new(
            "headings_have_items",
            false,
            format!("no items under {}", empty.join(", ")),
        )
    });

    // headings parsed = headings in the plain text
    let found = headings_in_text(text);
    let parsed: BTreeSet<i32> = sched.portfolios.iter().map(|p| p.seq).collect();
    out.push(
        if found.len() == sched.portfolios.len() && found == parsed {
            Check::new(
                "heading_count",
                true,
                format!(
                    "{} headings in the text, {} parsed",
                    found.len(),
                    sched.portfolios.len()
                ),
            )
        } else {
            let missing: Vec<String> = found
                .difference(&parsed)
                .map(|n| format!("{n:02}"))
                .collect();
            let extra: Vec<String> = parsed
                .difference(&found)
                .map(|n| format!("{n:02}"))
                .collect();
            Check::new(
                "heading_count",
                false,
                format!(
                    "{} headings in the text, {} parsed; in the text only: [{}]; parsed only: [{}]",
                    found.len(),
                    sched.portfolios.len(),
                    missing.join(", "),
                    extra.join(", ")
                ),
            )
        },
    );

    if !sched.warnings.is_empty() {
        let mut d = sched
            .warnings
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join(" | ");
        if sched.warnings.len() > 5 {
            d.push_str(&format!(" | (+{} more)", sched.warnings.len() - 5));
        }
        // informational: recorded, never a reason to hold an instrument back
        out.push(Check::new("parser_warnings", true, d));
    }
    out
}

/// All four contract checks passed.
#[must_use]
pub fn all_ok(checks: &[Check]) -> bool {
    checks.iter().all(|c| c.ok)
}

#[cfg(test)]
mod tests {
    use super::super::schedule::Portfolio;
    use super::*;

    fn item(n: i32) -> Item {
        Item {
            item_no: n,
            text: format!(
                "Inst {}",
                ["A", "B", "C", "D"][usize::try_from(n).unwrap_or(0) % 4]
            ),
            page: 2,
        }
    }

    fn portfolio(seq: i32, f: &[i32], i: &[i32]) -> Portfolio {
        Portfolio {
            seq,
            title: format!("Minister of Thing {seq}"),
            first_page: 2,
            functions: f.iter().map(|n| item(*n)).collect(),
            institutions: i.iter().map(|n| item(*n)).collect(),
            laws: vec![],
        }
    }

    fn text(n: i32) -> String {
        (1..=n)
            .map(|i| format!("{i:02}. Minister of Thing {i}\nColumn I"))
            .collect::<Vec<_>>()
            .join("\n\u{c}")
    }

    #[test]
    fn clean_schedule_passes_every_check() {
        let s = Schedule {
            portfolios: vec![portfolio(1, &[1, 2], &[1]), portfolio(2, &[1], &[1, 2, 3])],
            table_pages: vec![2],
            ..Schedule::default()
        };
        let c = validate(&s, &text(2), &[], &[]);
        assert_eq!(c.len(), 6);
        assert!(all_ok(&c), "{c:?}");
    }

    #[test]
    fn a_gap_in_numbering_fails() {
        let s = Schedule {
            portfolios: vec![portfolio(1, &[1, 2, 4], &[1, 2])],
            table_pages: vec![2],
            ..Schedule::default()
        };
        let c = validate(&s, &text(1), &[], &[]);
        assert!(!all_ok(&c));
        let items = c
            .iter()
            .find(|c| c.check == "items_sequential")
            .expect("check");
        assert!(!items.ok);
        assert!(
            items.detail.contains("01 column I: expected 3, found 4"),
            "{}",
            items.detail
        );
    }

    #[test]
    fn a_missing_heading_fails_both_sequence_and_count() {
        let s = Schedule {
            portfolios: vec![portfolio(1, &[1], &[1]), portfolio(3, &[1], &[1])],
            table_pages: vec![2],
            ..Schedule::default()
        };
        let c = validate(&s, &text(3), &[], &[]);
        let get = |n: &str| c.iter().find(|c| c.check == n).expect("check").clone();
        assert!(get("schedule_found").ok);
        assert!(!get("headings_sequential").ok);
        assert!(!get("heading_count").ok);
        assert!(
            get("heading_count")
                .detail
                .contains("in the text only: [02]")
        );
        assert!(get("items_sequential").ok);
    }

    #[test]
    fn a_printed_gap_listed_in_the_exceptions_passes_with_a_note() {
        let s = Schedule {
            portfolios: vec![portfolio(1, &[1, 2, 4], &[1, 2])],
            table_pages: vec![2],
            ..Schedule::default()
        };
        let gap = ExpectedGap {
            heading: 1,
            column: "I".into(),
            missing: vec![3],
            note: "prints 2 then 4".into(),
        };
        let c = validate(&s, &text(1), std::slice::from_ref(&gap), &[]);
        let seq = c
            .iter()
            .find(|c| c.check == "items_sequential")
            .expect("check");
        assert!(
            seq.ok && seq.detail.contains("prints 2 then 4"),
            "{}",
            seq.detail
        );
        // another gap in the same column is still a failure
        let s2 = Schedule {
            portfolios: vec![portfolio(1, &[1, 2, 4, 6], &[1])],
            table_pages: vec![2],
            ..Schedule::default()
        };
        assert!(
            !validate(&s2, &text(1), &[gap], &[])
                .iter()
                .find(|c| c.check == "items_sequential")
                .expect("check")
                .ok
        );
    }

    #[test]
    fn column_bleed_is_told_from_real_names() {
        for bad in [
            "National Transport Commission National",
            "Sri Lanka Press Council Sri",
            "Department of Sri Lanka Railways Railways",
            "Lanka General Trading Company Ltd. 1949 of 1949",
            "Department of Textile Industries No.",
            "Department of Co-operative Development Registrar Co-operative 12",
            "National Sports Council 17 of 1999",
            "Sri Lanka Export Development Board Sri Lanka 1979 of 1979",
            "Vidatha Centres National Crafts Act, No. 4 of",
            "Police Ordinance No. 16 of 1865",
            "Some Board Act, No. 3",
        ] {
            assert!(bleeds(bad), "{bad}");
        }
        for good in [
            "Coast Guard Department of Sri Lanka",
            "Sri Lanka Army",
            "All District Secretariats and Divisional Secretariats",
            "National Council for Elders and National Secretariat for Elders",
            "Department of Registration of Persons",
            "Strike, Riot, Civil Commotion and Terrorism Fund",
            "Institutions coming under the Revival (Removal) of Underperforming Enterprises or Underutilized Assets Act vested to the Secretary to the Treasury",
            "Telecommunications Regulatory Commission of Sri Lanka and Allied Institutions",
            "National Institute of Post-Harvest Management",
            "Central Bank of Sri Lanka",
            "Hotel Developers (Lanka) PLC (PQ 143)",
        ] {
            assert!(!bleeds(good), "{good}");
        }
    }

    #[test]
    fn a_gazette_in_another_layout_has_no_schedule() {
        let c = validate(&Schedule::default(), "1.0 Minister of Defence", &[], &[]);
        assert!(!all_ok(&c));
        assert!(
            !c.iter()
                .find(|c| c.check == "schedule_found")
                .expect("check")
                .ok
        );
    }

    #[test]
    fn an_empty_heading_fails() {
        let s = Schedule {
            portfolios: vec![portfolio(1, &[], &[])],
            table_pages: vec![2],
            ..Schedule::default()
        };
        let c = validate(&s, &text(1), &[], &[]);
        assert!(
            !c.iter()
                .find(|c| c.check == "headings_have_items")
                .expect("check")
                .ok
        );
    }

    #[test]
    fn a_number_split_from_its_title_by_the_text_flow_still_counts() {
        let t = "Powers, Duties & Functions\n(1)\nLaws to be Implemented\nMinistry of Defence\n1. Formulation\n(2) Ministry of Finance &\nPlanning\n3\nMinistry of Health";
        let h = headings_in_text(t);
        assert_eq!(h.into_iter().collect::<Vec<_>>(), vec![1, 2, 3]);
    }

    #[test]
    fn headings_in_text_are_distinct_numbers() {
        let t = "SCHEDULE\n01. Minister of Defence\n...\n\u{c}01.Minister of Defence (Contd.)\n 2. Minister of Finance\n10. Provision of services";
        let h = headings_in_text(t);
        assert_eq!(h.into_iter().collect::<Vec<_>>(), vec![1, 2]);
    }
}
