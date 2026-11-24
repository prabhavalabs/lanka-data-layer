//! The first step of `lanka civic build` (13 §2.2, C2): apply every
//! amendment, in date order, to the structure in force when it takes effect.
//!
//! The timeline is replayed: a baseline starts a structure, a run of
//! supplements accumulates one (as in `lanka_core::civic_timeline`), and each
//! amendment is applied by [`super::apply`] to the structure in force just
//! before it. An amendment is only applied when the gazette it names is part
//! of that structure. Its outcome becomes three checks:
//!
//! - `amended_structure`: the gazette it names is in force when it takes effect;
//! - `changes_applied`: every change applied as printed;
//! - `numbering_consistent`: the columns it changed still number upwards.
//!
//! The amendment's status follows from all its checks unless a reviewer has
//! approved or rejected it. A served amendment (validated or approved)
//! becomes the structure later amendments apply to; one that is not served
//! is skipped, and the structure before it carries on. The structure as
//! amended is written under the amendment's doc_id, each line with the
//! gazette it is printed in (`source_doc`), so `/v2/civic/structure` and the
//! assignments read it like any other structure.
//!
//! [`plan`] is the pure part; [`run`] reads and writes the database.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result};
use chrono::NaiveDate;
use serde::Serialize;
use sqlx::PgPool;

use super::amend::Change;
use super::apply::{Applied, Heading, Line, apply};
use super::validate::{Check, all_ok};

/// A served baseline or supplement with its headings.
#[derive(Debug, Clone)]
pub struct BaseInput {
    pub doc_id: String,
    /// `baseline` | `supplement`
    pub kind: String,
    pub effective_from: NaiveDate,
    pub headings: Vec<Heading>,
}

/// An amendment with the changes extract read.
#[derive(Debug, Clone)]
pub struct AmendmentInput {
    pub doc_id: String,
    pub effective_from: NaiveDate,
    pub amends: Option<String>,
    pub changes: Vec<Change>,
    /// Checks stored by extract (and reviews).
    pub checks: Vec<Check>,
}

/// What the build decided for one amendment.
#[derive(Debug, Clone)]
pub struct AmendmentResult {
    pub doc_id: String,
    pub applied: Option<Applied>,
    pub checks: Vec<Check>,
    pub status: &'static str,
    pub served: bool,
}

/// The checks this step owns (replaced on every build).
const OWN: &[&str] = &[
    "amended_structure",
    "changes_applied",
    "numbering_consistent",
    "earlier_amendments_served",
];

fn review_decision(checks: &[Check]) -> Option<bool> {
    checks
        .iter()
        .rev()
        .find(|c| c.check == "review")
        .map(|c| c.ok)
}

fn changes_check(a: &Applied) -> Check {
    let total = a.outcomes.len();
    let other_text = a.outcomes.iter().filter(|o| !o.applied && o.ok).count();
    let problems: Vec<String> = a
        .outcomes
        .iter()
        .filter(|o| !o.ok)
        .map(|o| o.note.clone())
        .collect();
    let applied = a.outcomes.iter().filter(|o| o.applied).count();
    let mut detail = format!("{applied} of {total} change(s) applied");
    if other_text > 0 {
        detail.push_str(&format!(
            "; {other_text} made in the Sinhala or Tamil text only"
        ));
    }
    if !problems.is_empty() {
        detail.push_str(&format!(
            "; {} to review: {}",
            problems.len(),
            problems
                .iter()
                .take(6)
                .cloned()
                .collect::<Vec<_>>()
                .join(" | ")
        ));
    }
    if total == 0 {
        return Check {
            check: "changes_applied".to_owned(),
            ok: false,
            detail: "no change was read from this amendment, so nothing is applied".to_owned(),
        };
    }
    Check {
        check: "changes_applied".to_owned(),
        ok: problems.is_empty(),
        detail,
    }
}

enum Event<'a> {
    Base(&'a BaseInput),
    Amend(&'a AmendmentInput),
}

impl Event<'_> {
    fn key(&self) -> (NaiveDate, &str) {
        match self {
            Event::Base(b) => (b.effective_from, b.doc_id.as_str()),
            Event::Amend(a) => (a.effective_from, a.doc_id.as_str()),
        }
    }
}

/// Replays the timeline and applies each amendment.
#[must_use]
pub fn plan(bases: &[BaseInput], amendments: &[AmendmentInput]) -> Vec<AmendmentResult> {
    let mut events: Vec<Event<'_>> = bases
        .iter()
        .map(Event::Base)
        .chain(amendments.iter().map(Event::Amend))
        .collect();
    events.sort_by(|a, b| a.key().cmp(&b.key()));

    let mut state: Option<(Vec<Heading>, Vec<String>)> = None;
    let mut in_run = false;
    // amendments applied to the structure in force but not served (waiting
    // for review): later amendments apply on top of them, and wait too
    let mut pending: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for e in events {
        match e {
            Event::Base(b) => {
                if b.kind == "supplement"
                    && in_run
                    && let Some((hs, composed)) = state.as_mut()
                {
                    let keys: BTreeSet<&str> = b
                        .headings
                        .iter()
                        .map(|h| h.portfolio_key.as_str())
                        .collect();
                    hs.retain(|h| !keys.contains(h.portfolio_key.as_str()));
                    hs.extend(b.headings.iter().cloned());
                    composed.push(b.doc_id.clone());
                } else {
                    state = Some((b.headings.clone(), vec![b.doc_id.clone()]));
                    pending.clear();
                }
                in_run = b.kind == "supplement";
            }
            Event::Amend(a) => {
                let mut checks: Vec<Check> = a
                    .checks
                    .iter()
                    .filter(|c| !OWN.contains(&c.check.as_str()))
                    .cloned()
                    .collect();
                let structure = match (&state, &a.amends) {
                    (None, _) => Err(format!("no structure is in force on {}", a.effective_from)),
                    (Some(_), None) => Err("the notification names no gazette".to_owned()),
                    (Some((_, composed)), Some(named)) if !composed.contains(named) => {
                        Err(format!(
                            "it amends {named}, but the structure in force on {} is made of {}",
                            a.effective_from,
                            composed.join(", ")
                        ))
                    }
                    (Some((hs, composed)), Some(_)) => Ok((hs, composed)),
                };
                let applied = match &structure {
                    Ok((hs, composed)) => {
                        checks.push(Check {
                            check: "amended_structure".to_owned(),
                            ok: true,
                            detail: format!(
                                "applied to the structure made of {}",
                                composed.join(", ")
                            ),
                        });
                        let ap = apply(hs, &a.changes, &a.doc_id);
                        checks.push(changes_check(&ap));
                        checks.push(Check {
                            check: "earlier_amendments_served".to_owned(),
                            ok: pending.is_empty(),
                            detail: if pending.is_empty() {
                                "every earlier amendment of this structure is served".to_owned()
                            } else {
                                format!(
                                    "applied on top of {}, which wait(s) for review; served once they are",
                                    pending.join(", ")
                                )
                            },
                        });
                        checks.push(Check {
                            check: "numbering_consistent".to_owned(),
                            ok: ap.numbering.is_empty(),
                            detail: if ap.numbering.is_empty() {
                                "every column it changes still numbers upwards".to_owned()
                            } else {
                                ap.numbering.join(" | ")
                            },
                        });
                        Some(ap)
                    }
                    Err(e) => {
                        checks.push(Check {
                            check: "amended_structure".to_owned(),
                            ok: false,
                            detail: e.clone(),
                        });
                        None
                    }
                };
                let status = match review_decision(&checks) {
                    Some(true) => "approved",
                    Some(false) => "rejected",
                    None if all_ok(&checks) => "validated",
                    None => "needs_review",
                };
                let served = applied.is_some()
                    && matches!(status, "validated" | "approved")
                    && pending.is_empty();
                // applied and not rejected: the structure later amendments apply to
                if status != "rejected"
                    && let (Some(ap), Some((hs, composed))) = (&applied, state.as_mut())
                {
                    hs.clone_from(&ap.headings);
                    composed.push(a.doc_id.clone());
                    if !served {
                        pending.push(a.doc_id.clone());
                    }
                }
                out.push(AmendmentResult {
                    doc_id: a.doc_id.clone(),
                    applied,
                    checks,
                    status,
                    served,
                });
            }
        }
    }
    out
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ConsolidateReport {
    pub amendments: usize,
    /// Applied and served (validated or approved).
    pub served: usize,
    pub needs_review: Vec<String>,
    pub changes_applied: usize,
    pub changes_total: usize,
}

type PortfolioRow = (String, i32, String, Option<i32>);
/// `(doc_id, effective_from, amends, checks)`
type AmendmentRow = (
    String,
    NaiveDate,
    Option<String>,
    sqlx::types::Json<Vec<Check>>,
);
/// `(doc_id, clause, heading, lang_only, spec, page, text)`
type ChangeRow = (
    String,
    String,
    Option<sqlx::types::Json<super::amend::HeadingRef>>,
    Option<String>,
    sqlx::types::Json<super::amend::Op>,
    Option<i32>,
    String,
);
type ItemRow = (String, i32, String, i32, String, Option<i32>);

async fn bases(pool: &PgPool) -> Result<Vec<BaseInput>> {
    let insts: Vec<(String, String, NaiveDate)> = sqlx::query_as(
        "SELECT doc_id, kind, effective_from FROM civic.instruments
         WHERE kind IN ('baseline', 'supplement') AND status IN ('validated', 'approved')",
    )
    .fetch_all(pool)
    .await?;
    let ids: Vec<String> = insts.iter().map(|i| i.0.clone()).collect();
    let ports: Vec<PortfolioRow> = sqlx::query_as(
        "SELECT doc_id, seq, title, first_page FROM civic.portfolios WHERE doc_id = ANY($1) ORDER BY doc_id, seq",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await?;
    let items: Vec<ItemRow> = sqlx::query_as(
        "SELECT doc_id, seq, col, item_no, text, page FROM civic.items WHERE doc_id = ANY($1)
         ORDER BY doc_id, seq, col, item_no",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await?;
    let mut by: BTreeMap<(String, i32), Heading> = BTreeMap::new();
    for (doc_id, seq, title, page) in ports {
        by.insert(
            (doc_id.clone(), seq),
            Heading {
                seq,
                portfolio_key: super::entity::portfolio_key(&title),
                title,
                doc_id,
                page,
                functions: Vec::new(),
                institutions: Vec::new(),
                laws: Vec::new(),
            },
        );
    }
    for (doc_id, seq, col, item_no, text, page) in items {
        if let Some(h) = by.get_mut(&(doc_id.clone(), seq)) {
            let line = Line {
                item_no,
                text,
                page,
                doc_id,
            };
            match col.as_str() {
                "function" => h.functions.push(line),
                "institution" => h.institutions.push(line),
                _ => h.laws.push(line),
            }
        }
    }
    Ok(insts
        .into_iter()
        .map(|(doc_id, kind, effective_from)| BaseInput {
            headings: by
                .iter()
                .filter(|((d, _), _)| *d == doc_id)
                .map(|(_, h)| h.clone())
                .collect(),
            doc_id,
            kind,
            effective_from,
        })
        .collect())
}

async fn amendments(pool: &PgPool) -> Result<Vec<AmendmentInput>> {
    let rows: Vec<AmendmentRow> = sqlx::query_as(
        "SELECT doc_id, effective_from, amends, checks FROM civic.instruments WHERE kind = 'amendment'",
    )
    .fetch_all(pool)
    .await?;
    let changes: Vec<ChangeRow> = sqlx::query_as(
        "SELECT doc_id, clause, heading, lang_only, spec, page, text FROM civic.amendment_changes
             ORDER BY doc_id, change_no",
    )
    .fetch_all(pool)
    .await?;
    let mut by: BTreeMap<String, Vec<Change>> = BTreeMap::new();
    for (doc_id, clause, heading, lang_only, spec, page, text) in changes {
        by.entry(doc_id).or_default().push(Change {
            clause,
            heading: heading.map(|h| h.0),
            lang_only,
            op: spec.0,
            page: page.unwrap_or(0),
            text,
        });
    }
    Ok(rows
        .into_iter()
        .map(|(doc_id, effective_from, amends, checks)| AmendmentInput {
            changes: by.remove(&doc_id).unwrap_or_default(),
            doc_id,
            effective_from,
            amends,
            checks: checks.0,
        })
        .collect())
}

/// Item numbers to store: as amended, or 1..n when they repeat (the
/// `numbering_consistent` check has already failed then).
fn storable(lines: &[Line]) -> Vec<i32> {
    let nos: Vec<i32> = lines.iter().map(|l| l.item_no).collect();
    let distinct: BTreeSet<i32> = nos.iter().copied().collect();
    if distinct.len() == nos.len() {
        nos
    } else {
        (1..=i32::try_from(lines.len()).unwrap_or(i32::MAX)).collect()
    }
}

/// Applies every amendment and stores the structures as amended.
pub async fn run(pool: &PgPool) -> Result<ConsolidateReport> {
    let bs = bases(pool).await?;
    let am = amendments(pool).await?;
    let results = plan(&bs, &am);

    let mut report = ConsolidateReport {
        amendments: results.len(),
        ..ConsolidateReport::default()
    };
    let mut tx = pool.begin().await?;
    for r in &results {
        sqlx::query("DELETE FROM civic.portfolios WHERE doc_id = $1")
            .bind(&r.doc_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "UPDATE civic.amendment_changes SET applied = NULL, ok = NULL, note = NULL WHERE doc_id = $1",
        )
        .bind(&r.doc_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE civic.instruments SET status = $2, checks = $3 WHERE doc_id = $1")
            .bind(&r.doc_id)
            .bind(r.status)
            .bind(sqlx::types::Json(&r.checks))
            .execute(&mut *tx)
            .await?;
        if r.served {
            report.served += 1;
        } else if r.status == "needs_review" {
            report.needs_review.push(r.doc_id.clone());
        }
        let Some(ap) = &r.applied else { continue };
        report.changes_total += ap.outcomes.len();
        report.changes_applied += ap.outcomes.iter().filter(|o| o.applied).count();
        let (mut nos, mut applied, mut ok, mut notes) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for (k, o) in ap.outcomes.iter().enumerate() {
            nos.push(i32::try_from(k + 1).unwrap_or(i32::MAX));
            applied.push(o.applied);
            ok.push(o.ok);
            notes.push(o.note.clone());
        }
        sqlx::query(
            "UPDATE civic.amendment_changes c SET applied = u.a, ok = u.o, note = u.n
             FROM unnest($2::int[], $3::bool[], $4::bool[], $5::text[]) AS u(k, a, o, n)
             WHERE c.doc_id = $1 AND c.change_no = u.k",
        )
        .bind(&r.doc_id)
        .bind(&nos)
        .bind(&applied)
        .bind(&ok)
        .bind(&notes)
        .execute(&mut *tx)
        .await?;

        // the structure as amended, one heading per position
        for (pos, h) in ap.headings.iter().enumerate() {
            let seq = i32::try_from(pos + 1).unwrap_or(i32::MAX);
            sqlx::query(
                "INSERT INTO civic.portfolios (doc_id, seq, title, portfolio_key, first_page, source_doc)
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(&r.doc_id)
            .bind(seq)
            .bind(&h.title)
            .bind(&h.portfolio_key)
            .bind(h.page)
            .bind(&h.doc_id)
            .execute(&mut *tx)
            .await
            .with_context(|| format!("{}: heading {seq}", r.doc_id))?;
            let (mut cols, mut ns, mut texts, mut pages, mut srcs) =
                (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
            for (col, lines) in [
                ("function", &h.functions),
                ("institution", &h.institutions),
                ("law", &h.laws),
            ] {
                for (l, n) in lines.iter().zip(storable(lines)) {
                    cols.push(col.to_owned());
                    ns.push(n);
                    texts.push(l.text.clone());
                    pages.push(l.page);
                    srcs.push(l.doc_id.clone());
                }
            }
            sqlx::query(
                "INSERT INTO civic.items (doc_id, seq, col, item_no, text, page, source_doc)
                 SELECT $1, $2, c, n, t, g, s
                 FROM unnest($3::text[], $4::int[], $5::text[], $6::int[], $7::text[]) AS u(c, n, t, g, s)",
            )
            .bind(&r.doc_id)
            .bind(seq)
            .bind(&cols)
            .bind(&ns)
            .bind(&texts)
            .bind(&pages)
            .bind(&srcs)
            .execute(&mut *tx)
            .await
            .with_context(|| format!("{}: items of heading {seq}", r.doc_id))?;
        }
    }
    tx.commit().await?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::civic::amend::{Anchor, Column, HeadingRef, NewItem, Op};

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).expect("date")
    }

    fn heading(doc: &str, seq: i32, title: &str, inst: &[&str]) -> Heading {
        Heading {
            seq,
            title: title.to_owned(),
            portfolio_key: super::super::entity::portfolio_key(title),
            doc_id: doc.to_owned(),
            page: Some(2),
            functions: vec![],
            institutions: inst
                .iter()
                .enumerate()
                .map(|(k, t)| Line {
                    item_no: i32::try_from(k + 1).expect("n"),
                    text: (*t).to_owned(),
                    page: Some(3),
                    doc_id: doc.to_owned(),
                })
                .collect(),
            laws: vec![],
        }
    }

    fn insert(doc: &str, on: NaiveDate, amends: &str, title: &str, text: &str) -> AmendmentInput {
        AmendmentInput {
            doc_id: doc.to_owned(),
            effective_from: on,
            amends: Some(amends.to_owned()),
            changes: vec![Change {
                clause: "(1) (a)".to_owned(),
                heading: Some(HeadingRef {
                    no: None,
                    title: Some(title.to_owned()),
                }),
                lang_only: None,
                op: Op::Insert {
                    col: Column::Institution,
                    anchor: Anchor::End,
                    items: vec![NewItem {
                        // numbered on from the last item
                        item_no: None,
                        text: text.to_owned(),
                        page: 1,
                    }],
                },
                page: 1,
                text: String::new(),
            }],
            checks: vec![],
        }
    }

    #[test]
    fn amendments_build_on_each_other_and_a_new_baseline_ends_them() {
        let bases = vec![
            BaseInput {
                doc_id: "b1".to_owned(),
                kind: "baseline".to_owned(),
                effective_from: d(2022, 5, 27),
                headings: vec![heading("b1", 1, "Minister of Defence", &["Sri Lanka Army"])],
            },
            BaseInput {
                doc_id: "b2".to_owned(),
                kind: "baseline".to_owned(),
                effective_from: d(2022, 9, 1),
                headings: vec![heading("b2", 1, "Minister of Defence", &["Sri Lanka Army"])],
            },
        ];
        let ams = vec![
            insert(
                "a1",
                d(2022, 6, 10),
                "b1",
                "Minister of Defence",
                "Sri Lanka Navy",
            ),
            insert(
                "a2",
                d(2022, 7, 1),
                "b1",
                "Minister of Defence",
                "Sri Lanka Air Force",
            ),
            // names b1, but b2 is in force by then
            insert(
                "a3",
                d(2022, 10, 1),
                "b1",
                "Minister of Defence",
                "Coast Guard",
            ),
        ];
        let r = plan(&bases, &ams);
        assert_eq!(
            r.iter().map(|x| x.status).collect::<Vec<_>>(),
            vec!["validated", "validated", "needs_review"]
        );
        let names = |x: &AmendmentResult| -> Vec<String> {
            x.applied.as_ref().expect("applied").headings[0]
                .institutions
                .iter()
                .map(|l| l.text.clone())
                .collect()
        };
        assert_eq!(
            names(&r[1]),
            vec!["Sri Lanka Army", "Sri Lanka Navy", "Sri Lanka Air Force"]
        );
        assert!(r[2].applied.is_none() && !r[2].served);
        // evidence survives: the army line is still the baseline's, the navy line a1's
        let h = &r[1].applied.as_ref().expect("applied").headings[0];
        assert_eq!(
            (
                h.institutions[0].doc_id.as_str(),
                h.institutions[1].doc_id.as_str()
            ),
            ("b1", "a1")
        );
    }

    #[test]
    fn a_rejected_amendment_is_skipped_and_the_next_applies_to_the_structure_before_it() {
        let bases = vec![BaseInput {
            doc_id: "b1".to_owned(),
            kind: "baseline".to_owned(),
            effective_from: d(2022, 5, 27),
            headings: vec![heading("b1", 1, "Minister of Defence", &["Sri Lanka Army"])],
        }];
        let mut a1 = insert(
            "a1",
            d(2022, 6, 10),
            "b1",
            "Minister of Defence",
            "Sri Lanka Navy",
        );
        a1.checks.push(Check {
            check: "review".to_owned(),
            ok: false,
            detail: "rejected".to_owned(),
        });
        let a2 = insert(
            "a2",
            d(2022, 7, 1),
            "b1",
            "Minister of Defence",
            "Sri Lanka Air Force",
        );
        let r = plan(&bases, &[a1, a2]);
        assert_eq!((r[0].status, r[0].served), ("rejected", false));
        let h = &r[1].applied.as_ref().expect("applied").headings[0];
        assert_eq!(h.institutions.len(), 2, "{:?}", h.institutions);
    }

    #[test]
    fn an_amendment_waiting_for_review_holds_back_the_ones_after_it() {
        let bases = vec![BaseInput {
            doc_id: "b1".to_owned(),
            kind: "baseline".to_owned(),
            effective_from: d(2022, 5, 27),
            headings: vec![heading("b1", 1, "Minister of Defence", &["Sri Lanka Army"])],
        }];
        // a1 names a heading that does not exist: applied in part, to review
        let mut a1 = insert(
            "a1",
            d(2022, 6, 10),
            "b1",
            "Minister of Defence",
            "Sri Lanka Navy",
        );
        a1.changes.push(Change {
            clause: "(2) (a)".to_owned(),
            heading: Some(HeadingRef {
                no: None,
                title: Some("Minister of Nothing".to_owned()),
            }),
            ..a1.changes[0].clone()
        });
        let a2 = insert(
            "a2",
            d(2022, 7, 1),
            "b1",
            "Minister of Defence",
            "Sri Lanka Air Force",
        );
        let r = plan(&bases, &[a1.clone(), a2.clone()]);
        assert_eq!((r[0].status, r[0].served), ("needs_review", false));
        // a2 applies on top of a1 (so its own changes resolve) but is not served
        assert_eq!((r[1].status, r[1].served), ("needs_review", false));
        let h = &r[1].applied.as_ref().expect("applied").headings[0];
        assert_eq!(h.institutions.len(), 3);
        // once a1 is approved, both are served
        a1.checks.push(Check {
            check: "review".to_owned(),
            ok: true,
            detail: "approved".to_owned(),
        });
        let r = plan(&bases, &[a1, a2]);
        assert!(r[0].served && r[1].served, "{:?}", r[1].checks);
    }

    #[test]
    fn an_amendment_to_a_supplement_applies_to_the_run() {
        let bases = vec![
            BaseInput {
                doc_id: "s1".to_owned(),
                kind: "supplement".to_owned(),
                effective_from: d(2018, 11, 5),
                headings: vec![heading("s1", 1, "Minister of Defence", &["Sri Lanka Army"])],
            },
            BaseInput {
                doc_id: "s2".to_owned(),
                kind: "supplement".to_owned(),
                effective_from: d(2018, 11, 6),
                headings: vec![heading("s2", 1, "Minister of Health", &["Hospital Board"])],
            },
            BaseInput {
                doc_id: "s3".to_owned(),
                kind: "supplement".to_owned(),
                effective_from: d(2018, 11, 12),
                headings: vec![heading("s3", 1, "Minister of Trade", &["Trade Board"])],
            },
        ];
        let a = insert(
            "a1",
            d(2018, 11, 9),
            "s1",
            "Minister of Defence",
            "Department of Government Printing",
        );
        let r = plan(&bases, &[a]);
        assert!(r[0].served, "{:?}", r[0].checks);
        assert_eq!(r[0].applied.as_ref().expect("applied").headings.len(), 2);
    }
}
