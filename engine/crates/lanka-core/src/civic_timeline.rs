//! The pure part of the civic reads (13 §2.1): which gazettes make up the
//! structure in force on a date, short titles, and the comparison of two
//! structures with renamed ministries recognised.
//!
//! **Composition.** The structure in force is the latest *baseline*. A run of
//! *supplements* after a baseline (or after another supplement) accumulates
//! into a new structure that replaces the previous one from the first
//! supplement's date; each later supplement in the run adds or replaces
//! headings by `portfolio_key`, from its own date. A later baseline starts
//! afresh.

use std::collections::{BTreeMap, BTreeSet};

use chrono::NaiveDate;
use serde::Serialize;
use utoipa::ToSchema;

/// One minister heading of one gazette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    pub doc_id: String,
    pub seq: i32,
    pub portfolio_key: String,
}

/// A served baseline or supplement.
#[derive(Debug, Clone)]
pub struct Instrument {
    pub doc_id: String,
    /// `baseline` | `supplement`
    pub kind: String,
    pub effective_from: NaiveDate,
    pub headings: Vec<Heading>,
}

/// The structure in force from `effective_from` until the next step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub effective_from: NaiveDate,
    /// The latest gazette of the step.
    pub latest: String,
    /// Every gazette the structure is composed of, oldest first.
    pub composed: Vec<(String, NaiveDate)>,
    pub headings: Vec<Heading>,
}

/// Steps in order of date. Two steps on one day: the later gazette wins.
#[must_use]
pub fn compose(instruments: &[Instrument]) -> Vec<Step> {
    let mut sorted: Vec<&Instrument> = instruments.iter().collect();
    sorted.sort_by(|a, b| (a.effective_from, &a.doc_id).cmp(&(b.effective_from, &b.doc_id)));
    let mut steps: Vec<Step> = Vec::new();
    let mut prev_supplement = false;
    for i in sorted {
        let step = if i.kind == "supplement"
            && prev_supplement
            && let Some(last) = steps.last()
        {
            let keys: BTreeSet<&str> = i
                .headings
                .iter()
                .map(|h| h.portfolio_key.as_str())
                .collect();
            let mut headings: Vec<Heading> = last
                .headings
                .iter()
                .filter(|h| !keys.contains(h.portfolio_key.as_str()))
                .cloned()
                .collect();
            headings.extend(i.headings.iter().cloned());
            let mut composed = last.composed.clone();
            composed.push((i.doc_id.clone(), i.effective_from));
            Step {
                effective_from: i.effective_from,
                latest: i.doc_id.clone(),
                composed,
                headings,
            }
        } else {
            Step {
                effective_from: i.effective_from,
                latest: i.doc_id.clone(),
                composed: vec![(i.doc_id.clone(), i.effective_from)],
                headings: i.headings.clone(),
            }
        };
        prev_supplement = i.kind == "supplement";
        if steps
            .last()
            .is_some_and(|l| l.effective_from == step.effective_from)
        {
            steps.pop();
        }
        steps.push(step);
    }
    steps
}

/// The step in force on `at`.
#[must_use]
pub fn step_at(steps: &[Step], at: NaiveDate) -> Option<&Step> {
    steps.iter().rev().find(|s| s.effective_from <= at)
}

/// `Minister of Defence` → `Defence` (also `Minister for …`, `Ministry of …`).
#[must_use]
pub fn short_title(title: &str) -> String {
    let t = title.trim();
    let lower = t.to_lowercase();
    for p in [
        "minister of the ",
        "minister of ",
        "minister for the ",
        "minister for ",
        "ministry of the ",
        "ministry of ",
    ] {
        if lower.starts_with(p) && t.is_char_boundary(p.len()) && lower.len() == t.len() {
            let rest = t[p.len()..].trim();
            let mut cs = rest.chars();
            return cs.next().map_or_else(String::new, |f| {
                f.to_uppercase().collect::<String>() + cs.as_str()
            });
        }
    }
    t.to_owned()
}

// ───────────────────────────── comparing two structures ─────────────────────

/// A ministry of one structure with its institutions, for the comparison.
#[derive(Debug, Clone)]
pub struct Ministry {
    pub key: String,
    pub title: String,
}

/// An institution of one structure, under one ministry (its first listing).
#[derive(Debug, Clone)]
pub struct Listing {
    pub entity_key: String,
    pub name: String,
    pub kind: String,
    pub page: Option<i32>,
    pub ministry_key: String,
}

/// One structure for the comparison.
#[derive(Debug, Clone, Default)]
pub struct Side {
    pub ministries: Vec<Ministry>,
    pub listings: Vec<Listing>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PortfolioSide {
    pub portfolio_key: String,
    /// As printed: `Minister of Defence`.
    pub title: String,
    /// Without `Minister of`: `Defence`.
    pub short_title: String,
    /// The institution's page in that gazette.
    pub page: Option<i32>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Moved {
    pub entity_key: String,
    pub name: String,
    pub kind: String,
    pub from: PortfolioSide,
    pub to: PortfolioSide,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Added {
    pub entity_key: String,
    pub name: String,
    pub kind: String,
    pub to: PortfolioSide,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Removed {
    pub entity_key: String,
    pub name: String,
    pub kind: String,
    pub from: PortfolioSide,
}

/// A ministry that is the same one in both structures (renamed or not).
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct Kept {
    pub from_title: String,
    pub to_title: String,
    pub from_key: String,
    pub to_key: String,
    /// Jaccard overlap of their institutions, 0..1.
    pub overlap: f64,
    /// Share of the old ministry's institutions the new one holds, 0..1.
    pub share: f64,
    /// `same` (same key), `renamed` (a new name, Jaccard >= 0.5), or
    /// `successor` (it took most of the old ministry's institutions but is
    /// not the same ministry: a split or a merge).
    pub relation: String,
    /// `relation == "renamed"`
    pub renamed: bool,
}

#[derive(Debug, Clone, Default, Serialize, ToSchema)]
pub struct PortfolioDiff {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub kept: Vec<Kept>,
}

#[derive(Debug, Clone, Default, Serialize, ToSchema)]
pub struct DiffCounts {
    pub moved: usize,
    pub added: usize,
    pub removed: usize,
    pub portfolios_added: usize,
    pub portfolios_removed: usize,
    pub portfolios_renamed: usize,
}

#[derive(Debug, Clone, Default)]
pub struct Changes {
    pub moved: Vec<Moved>,
    pub added: Vec<Added>,
    pub removed: Vec<Removed>,
    pub portfolios: PortfolioDiff,
    pub counts: DiffCounts,
}

/// A ministry whose institutions are at least this similar (Jaccard) is the
/// same ministry under a new name.
const MIN_JACCARD: f64 = 0.5;
/// … or the new ministry keeps at least this share of the old one's
/// institutions: its successor in a split or merge (not counted as renamed).
const MIN_KEPT_SHARE: f64 = 0.6;

fn side(m: &Ministry, page: Option<i32>) -> PortfolioSide {
    PortfolioSide {
        portfolio_key: m.key.clone(),
        title: m.title.clone(),
        short_title: short_title(&m.title),
        page,
    }
}

/// Moved, added and removed institutions between two structures, and the
/// ministries kept (same key, or recognised by overlap), added and removed.
/// An institution only counts as moved when its old and new ministries are
/// not a kept pair.
#[must_use]
pub fn compare(from: &Side, to: &Side) -> Changes {
    let sets = |s: &Side| -> BTreeMap<String, BTreeSet<String>> {
        let mut m: BTreeMap<String, BTreeSet<String>> = s
            .ministries
            .iter()
            .map(|x| (x.key.clone(), BTreeSet::new()))
            .collect();
        for l in &s.listings {
            m.entry(l.ministry_key.clone())
                .or_default()
                .insert(l.entity_key.clone());
        }
        m
    };
    let (fs, ts) = (sets(from), sets(to));
    let mut kept: Vec<Kept> = Vec::new();
    for f in &from.ministries {
        for t in &to.ministries {
            let (a, b) = (&fs[&f.key], &ts[&t.key]);
            let inter = a.intersection(b).count() as f64;
            let union = a.union(b).count() as f64;
            let jaccard = if union > 0.0 { inter / union } else { 0.0 };
            let share = if a.is_empty() {
                0.0
            } else {
                inter / a.len() as f64
            };
            if f.key == t.key || jaccard >= MIN_JACCARD || share >= MIN_KEPT_SHARE {
                let relation = if f.key == t.key {
                    "same"
                } else if jaccard >= MIN_JACCARD {
                    "renamed"
                } else {
                    "successor"
                };
                kept.push(Kept {
                    from_title: f.title.clone(),
                    to_title: t.title.clone(),
                    from_key: f.key.clone(),
                    to_key: t.key.clone(),
                    overlap: (jaccard * 1000.0).round() / 1000.0,
                    share: (share * 1000.0).round() / 1000.0,
                    relation: relation.to_owned(),
                    renamed: relation == "renamed",
                });
            }
        }
    }
    let is_kept = |fk: &str, tk: &str| kept.iter().any(|k| k.from_key == fk && k.to_key == tk);
    let title_of = |s: &Side, key: &str| -> Ministry {
        s.ministries
            .iter()
            .find(|m| m.key == key)
            .cloned()
            .unwrap_or(Ministry {
                key: key.to_owned(),
                title: key.to_owned(),
            })
    };
    let (fl, tl): (BTreeMap<&str, &Listing>, BTreeMap<&str, &Listing>) = (
        from.listings
            .iter()
            .map(|l| (l.entity_key.as_str(), l))
            .collect(),
        to.listings
            .iter()
            .map(|l| (l.entity_key.as_str(), l))
            .collect(),
    );
    let mut c = Changes::default();
    for (k, old) in &fl {
        let from_side = side(&title_of(from, &old.ministry_key), old.page);
        match tl.get(k) {
            Some(new) if !is_kept(&old.ministry_key, &new.ministry_key) => c.moved.push(Moved {
                entity_key: (*k).to_owned(),
                name: new.name.clone(),
                kind: new.kind.clone(),
                from: from_side,
                to: side(&title_of(to, &new.ministry_key), new.page),
            }),
            Some(_) => {}
            None => c.removed.push(Removed {
                entity_key: (*k).to_owned(),
                name: old.name.clone(),
                kind: old.kind.clone(),
                from: from_side,
            }),
        }
    }
    for (k, new) in &tl {
        if !fl.contains_key(k) {
            c.added.push(Added {
                entity_key: (*k).to_owned(),
                name: new.name.clone(),
                kind: new.kind.clone(),
                to: side(&title_of(to, &new.ministry_key), new.page),
            });
        }
    }
    c.moved
        .sort_by(|a, b| (&a.from.title, &a.entity_key).cmp(&(&b.from.title, &b.entity_key)));
    c.added
        .sort_by(|a, b| (&a.to.title, &a.entity_key).cmp(&(&b.to.title, &b.entity_key)));
    c.removed
        .sort_by(|a, b| (&a.from.title, &a.entity_key).cmp(&(&b.from.title, &b.entity_key)));
    for f in &from.ministries {
        if !kept.iter().any(|k| k.from_key == f.key) {
            c.portfolios.removed.push(f.title.clone());
        }
    }
    for t in &to.ministries {
        if !kept.iter().any(|k| k.to_key == t.key) {
            c.portfolios.added.push(t.title.clone());
        }
    }
    c.portfolios.kept = kept;
    c.counts = DiffCounts {
        moved: c.moved.len(),
        added: c.added.len(),
        removed: c.removed.len(),
        portfolios_added: c.portfolios.added.len(),
        portfolios_removed: c.portfolios.removed.len(),
        portfolios_renamed: c.portfolios.kept.iter().filter(|k| k.renamed).count(),
    };
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).expect("date")
    }

    fn inst(id: &str, kind: &str, eff: NaiveDate, keys: &[&str]) -> Instrument {
        Instrument {
            doc_id: id.into(),
            kind: kind.into(),
            effective_from: eff,
            headings: keys
                .iter()
                .enumerate()
                .map(|(i, k)| Heading {
                    doc_id: id.into(),
                    seq: i32::try_from(i + 1).expect("seq"),
                    portfolio_key: (*k).into(),
                })
                .collect(),
        }
    }

    #[test]
    fn a_run_of_supplements_accumulates_and_a_baseline_resets() {
        let steps = compose(&[
            inst("base", "baseline", d(2015, 1, 18), &["a", "b", "c", "d"]),
            inst("s1", "supplement", d(2018, 11, 5), &["a", "b", "c"]),
            inst("s2", "supplement", d(2018, 11, 7), &["d"]),
            inst("s3", "supplement", d(2018, 11, 9), &["b", "e"]),
            inst("base2", "baseline", d(2018, 12, 24), &["a", "z"]),
        ]);
        assert_eq!(steps.len(), 5);
        // the first supplement replaces the baseline's structure outright
        fn keys(s: &Step) -> Vec<&str> {
            s.headings
                .iter()
                .map(|h| h.portfolio_key.as_str())
                .collect()
        }
        assert_eq!(keys(&steps[1]), ["a", "b", "c"]);
        assert_eq!(steps[1].composed.len(), 1);
        // later supplements add; a heading with a known key is replaced by the new one
        assert_eq!(keys(&steps[2]), ["a", "b", "c", "d"]);
        assert_eq!(keys(&steps[3]), ["a", "c", "d", "b", "e"]);
        assert_eq!(steps[3].headings[3].doc_id, "s3");
        assert_eq!(steps[3].latest, "s3");
        assert_eq!(
            steps[3]
                .composed
                .iter()
                .map(|c| c.0.as_str())
                .collect::<Vec<_>>(),
            ["s1", "s2", "s3"]
        );
        // a baseline starts afresh
        assert_eq!(keys(&steps[4]), ["a", "z"]);
        assert_eq!(steps[4].composed.len(), 1);
        // step lookup by date
        assert!(step_at(&steps, d(2015, 1, 1)).is_none());
        assert_eq!(step_at(&steps, d(2018, 11, 8)).expect("step").latest, "s2");
        assert_eq!(
            step_at(&steps, d(2030, 1, 1)).expect("step").latest,
            "base2"
        );
    }

    #[test]
    fn a_supplement_after_a_baseline_starts_a_new_run() {
        let steps = compose(&[
            inst("base", "baseline", d(2020, 1, 1), &["a", "b"]),
            inst("s1", "supplement", d(2020, 2, 1), &["c"]),
        ]);
        assert_eq!(
            steps[1].headings.len(),
            1,
            "replaces, not extends, the baseline"
        );
    }

    #[test]
    fn short_titles_drop_the_minister_prefix() {
        assert_eq!(short_title("Minister of Defence"), "Defence");
        assert_eq!(short_title("Minister for the Environment"), "Environment");
        assert_eq!(
            short_title("Ministry of Finance & Planning"),
            "Finance & Planning"
        );
        assert_eq!(short_title("Prime Minister"), "Prime Minister");
    }

    fn side_of(ministries: &[(&str, &str)], listings: &[(&str, &str)]) -> Side {
        Side {
            ministries: ministries
                .iter()
                .map(|(k, t)| Ministry {
                    key: (*k).into(),
                    title: (*t).into(),
                })
                .collect(),
            listings: listings
                .iter()
                .map(|(e, m)| Listing {
                    entity_key: (*e).into(),
                    name: (*e).into(),
                    kind: "other".into(),
                    page: Some(7),
                    ministry_key: (*m).into(),
                })
                .collect(),
        }
    }

    #[test]
    fn a_renamed_ministry_keeps_its_institutions_and_real_moves_still_show() {
        let from = side_of(
            &[
                ("health", "Minister of Health"),
                ("media", "Minister of Mass Media"),
                ("def", "Minister of Defence"),
            ],
            &[
                ("h1", "health"),
                ("h2", "health"),
                ("h3", "health"),
                ("m1", "media"),
                ("d1", "def"),
                ("d2", "def"),
                ("gone", "def"),
            ],
        );
        let to = side_of(
            &[
                ("health-and-mass-media", "Minister of Health and Mass Media"),
                ("def", "Minister of Defence"),
                ("new", "Minister of New Things"),
            ],
            &[
                ("h1", "health-and-mass-media"),
                ("h2", "health-and-mass-media"),
                ("h3", "health-and-mass-media"),
                ("m1", "health-and-mass-media"),
                ("d1", "def"),
                ("d2", "new"),
                ("fresh", "new"),
            ],
        );
        let c = compare(&from, &to);
        // health and media both fold into the new ministry: kept pairs, nothing moved
        let pairs: Vec<(&str, &str, bool)> = c
            .portfolios
            .kept
            .iter()
            .map(|k| (k.from_key.as_str(), k.to_key.as_str(), k.renamed))
            .collect();
        // health is renamed (Jaccard 3/4); media merged into it is its successor (1/4, but all of media)
        assert!(pairs.contains(&("health", "health-and-mass-media", true)));
        assert!(pairs.contains(&("media", "health-and-mass-media", false)));
        assert!(pairs.contains(&("def", "def", false)));
        let rel = |f: &str| {
            c.portfolios
                .kept
                .iter()
                .find(|k| k.from_key == f)
                .map(|k| k.relation.as_str())
        };
        assert_eq!(
            (rel("health"), rel("media"), rel("def")),
            (Some("renamed"), Some("successor"), Some("same"))
        );
        assert_eq!(c.counts.portfolios_renamed, 1);
        // d2 left defence for a ministry that is not its pair: a real move, with pages
        assert_eq!(c.moved.len(), 1);
        assert_eq!(c.moved[0].entity_key, "d2");
        assert_eq!(
            (
                c.moved[0].from.short_title.as_str(),
                c.moved[0].to.short_title.as_str()
            ),
            ("Defence", "New Things")
        );
        assert_eq!(
            (c.moved[0].from.page, c.moved[0].to.page),
            (Some(7), Some(7))
        );
        assert_eq!(
            c.removed
                .iter()
                .map(|r| r.entity_key.as_str())
                .collect::<Vec<_>>(),
            ["gone"]
        );
        assert_eq!(
            c.added
                .iter()
                .map(|r| r.entity_key.as_str())
                .collect::<Vec<_>>(),
            ["fresh"]
        );
        assert_eq!(
            c.portfolios.added,
            vec!["Minister of New Things".to_owned()]
        );
        assert!(c.portfolios.removed.is_empty());
        assert_eq!(
            (c.counts.moved, c.counts.added, c.counts.removed),
            (1, 1, 1)
        );
    }

    #[test]
    fn unrelated_ministries_are_not_paired() {
        let from = side_of(
            &[("a", "Minister of A")],
            &[("x1", "a"), ("x2", "a"), ("x3", "a")],
        );
        let to = side_of(
            &[("b", "Minister of B")],
            &[("x1", "b"), ("y1", "b"), ("y2", "b"), ("y3", "b")],
        );
        let c = compare(&from, &to);
        assert!(
            c.portfolios.kept.is_empty(),
            "1 of 3 kept is below 60%, jaccard 1/6"
        );
        assert_eq!(c.moved.len(), 1);
        assert_eq!(
            (c.counts.portfolios_added, c.counts.portfolios_removed),
            (1, 1)
        );
    }
}
