//! `lanka civic build` (13 §2.1): assignments with continuous intervals.
//!
//! The validated or approved baselines and supplements form steps (a baseline,
//! or a run of supplements accumulated; see `lanka_core::civic_timeline`), in
//! order of `effective_from`. Step *i* holds from its own date until step
//! *i+1*'s date, or until now for the last one. Its institutions become `civic.assignments` with that
//! interval; an institution that stays under the same `portfolio_key` in
//! consecutive baselines keeps one continuous row. The result is replaced in
//! one transaction, so a rebuild is idempotent.
//!
//! [`plan`] is the pure part (testable without a database); [`build`] reads
//! the instruments, applies the aliases and writes the plan.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result};
use chrono::NaiveDate;
use serde::Serialize;
use sqlx::PgPool;

use lanka_core::civic_timeline::{Heading, Instrument, compose};

use super::entity::{Aliases, guess_kind, has_soft_hyphen};

/// One institution line of a baseline, its key already after aliases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occ {
    /// The gazette the line is printed in.
    pub doc_id: String,
    pub entity_key: String,
    /// The wording of the gazette.
    pub name: String,
    pub seq: i32,
    pub portfolio_title: String,
    pub portfolio_key: String,
    pub page: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct BaselineInput {
    pub doc_id: String,
    pub effective_from: NaiveDate,
    /// In reading order (heading, then item number).
    pub occs: Vec<Occ>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssignmentRow {
    pub entity_key: String,
    pub doc_id: String,
    pub seq: i32,
    pub portfolio_title: String,
    pub portfolio_key: String,
    pub valid_from: NaiveDate,
    pub valid_to: Option<NaiveDate>,
    pub page: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityOut {
    pub entity_key: String,
    pub name: String,
    pub kind: &'static str,
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub assignments: Vec<AssignmentRow>,
    pub entities: Vec<EntityOut>,
    /// `(doc_id, entity_key)`: listed under more than one minister in one
    /// baseline; only the first listing is kept (the primary key allows one
    /// assignment per institution and start date).
    pub shared: Vec<(String, String)>,
}

/// Turns baselines into assignments and entities.
#[must_use]
pub fn plan(inputs: &[BaselineInput]) -> Plan {
    let mut bs: Vec<&BaselineInput> = inputs.iter().collect();
    bs.sort_by(|a, b| (a.effective_from, &a.doc_id).cmp(&(b.effective_from, &b.doc_id)));
    // two baselines of one day: the later one replaces the earlier
    let mut kept: Vec<&BaselineInput> = Vec::new();
    for b in bs {
        if kept
            .last()
            .is_some_and(|l| l.effective_from == b.effective_from)
        {
            kept.pop();
        }
        kept.push(b);
    }

    let mut plan = Plan::default();
    // key → [(baseline index, occurrence)]
    let mut by_key: BTreeMap<String, Vec<(usize, &Occ)>> = BTreeMap::new();
    for (i, b) in kept.iter().enumerate() {
        let mut seen = BTreeSet::new();
        for o in &b.occs {
            if seen.insert(o.entity_key.as_str()) {
                by_key.entry(o.entity_key.clone()).or_default().push((i, o));
            } else {
                plan.shared.push((b.doc_id.clone(), o.entity_key.clone()));
            }
        }
    }
    let end_of = |last: usize| kept.get(last + 1).map(|b| b.effective_from);

    for (key, occs) in &by_key {
        // runs of consecutive baselines under one portfolio
        let mut run: Option<(usize, usize, &Occ)> = None; // (first idx, last idx, first occ)
        let close = |plan: &mut Plan, r: (usize, usize, &Occ)| {
            let (first, last, o) = r;
            plan.assignments.push(AssignmentRow {
                entity_key: key.clone(),
                doc_id: o.doc_id.clone(),
                seq: o.seq,
                portfolio_title: o.portfolio_title.clone(),
                portfolio_key: o.portfolio_key.clone(),
                valid_from: kept[first].effective_from,
                valid_to: end_of(last),
                page: o.page,
            });
        };
        for &(i, o) in occs {
            run = match run {
                Some((first, last, fo)) if last + 1 == i && fo.portfolio_key == o.portfolio_key => {
                    Some((first, i, fo))
                }
                Some(r) => {
                    close(&mut plan, r);
                    Some((i, i, o))
                }
                None => Some((i, i, o)),
            };
        }
        if let Some(r) = run {
            close(&mut plan, r);
        }

        // entity: the latest wording without a line-wrap hyphen in it (else the
        // latest); the other wordings are aliases
        let name = occs
            .iter()
            .rev()
            .find(|(_, o)| !has_soft_hyphen(&o.name))
            .or_else(|| occs.last())
            .map(|(_, o)| o.name.clone())
            .unwrap_or_default();
        let mut aliases: Vec<String> = occs
            .iter()
            .map(|(_, o)| o.name.clone())
            .filter(|n| *n != name)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        aliases.sort();
        plan.entities.push(EntityOut {
            entity_key: key.clone(),
            kind: guess_kind(&name).as_str(),
            name,
            aliases,
        });
    }
    plan.assignments
        .sort_by(|a, b| (&a.entity_key, a.valid_from).cmp(&(&b.entity_key, b.valid_from)));
    plan
}

/// `(doc_id, seq, item_no, text, portfolio title, portfolio key, page)`
type InstitutionRow = (String, i32, i32, String, String, String, Option<i32>);

#[derive(Debug, Clone, Serialize)]
pub struct BuildReport {
    /// Served baselines and supplements.
    pub instruments: usize,
    /// Structures over time they make up (a baseline, or a run of supplements).
    pub steps: usize,
    pub entities: usize,
    pub assignments: usize,
    /// Assignments still in force (no end date).
    pub open: usize,
    /// Institutions listed under two ministers in one structure.
    pub shared_listings: usize,
    pub aliases_loaded: usize,
}

/// Rebuilds `civic.assignments` (and `civic.entities`) from the served baselines and supplements.
pub async fn build(pool: &PgPool, aliases: &Aliases) -> Result<BuildReport> {
    let insts: Vec<(String, String, NaiveDate)> = sqlx::query_as(
        "SELECT doc_id, kind, effective_from FROM civic.instruments
         WHERE kind IN ('baseline', 'supplement') AND status IN ('validated', 'approved')
         ORDER BY effective_from, doc_id",
    )
    .fetch_all(pool)
    .await?;
    let docs: Vec<String> = insts.iter().map(|i| i.0.clone()).collect();
    let heads: Vec<(String, i32, String)> = sqlx::query_as(
        "SELECT doc_id, seq, portfolio_key FROM civic.portfolios WHERE doc_id = ANY($1) ORDER BY doc_id, seq",
    )
    .bind(&docs)
    .fetch_all(pool)
    .await?;
    let rows: Vec<InstitutionRow> = sqlx::query_as(
        "SELECT t.doc_id, t.seq, t.item_no, t.text, p.title, p.portfolio_key, t.page
         FROM civic.items t JOIN civic.portfolios p ON p.doc_id = t.doc_id AND p.seq = t.seq
         WHERE t.col = 'institution' AND t.doc_id = ANY($1)
         ORDER BY t.doc_id, t.seq, t.item_no",
    )
    .bind(&docs)
    .fetch_all(pool)
    .await?;

    // keys after aliases, and the items to correct
    let mut by_heading: BTreeMap<(String, i32), Vec<Occ>> = BTreeMap::new();
    let (mut u_doc, mut u_seq, mut u_no, mut u_key) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (doc_id, seq, item_no, text, title, pkey, page) in rows {
        let key = aliases.key_of(&text);
        u_doc.push(doc_id.clone());
        u_seq.push(seq);
        u_no.push(item_no);
        u_key.push(key.clone());
        by_heading
            .entry((doc_id.clone(), seq))
            .or_default()
            .push(Occ {
                doc_id,
                entity_key: key,
                name: text,
                seq,
                portfolio_title: title,
                portfolio_key: pkey,
                page,
            });
    }
    let instruments: Vec<Instrument> = insts
        .iter()
        .map(|(doc_id, kind, effective_from)| Instrument {
            doc_id: doc_id.clone(),
            kind: kind.clone(),
            effective_from: *effective_from,
            headings: heads
                .iter()
                .filter(|h| &h.0 == doc_id)
                .map(|h| Heading {
                    doc_id: doc_id.clone(),
                    seq: h.1,
                    portfolio_key: h.2.clone(),
                })
                .collect(),
        })
        .collect();
    let steps = compose(&instruments);
    let inputs: Vec<BaselineInput> = steps
        .iter()
        .map(|s| BaselineInput {
            doc_id: s.latest.clone(),
            effective_from: s.effective_from,
            occs: s
                .headings
                .iter()
                .flat_map(|h| {
                    by_heading
                        .get(&(h.doc_id.clone(), h.seq))
                        .cloned()
                        .unwrap_or_default()
                })
                .collect(),
        })
        .collect();
    let plan = plan(&inputs);

    let mut tx = pool.begin().await?;
    sqlx::query(
        "UPDATE civic.items t SET entity_key = u.k
         FROM unnest($1::text[], $2::int[], $3::int[], $4::text[]) AS u(d, s, n, k)
         WHERE t.doc_id = u.d AND t.seq = u.s AND t.col = 'institution' AND t.item_no = u.n
           AND t.entity_key IS DISTINCT FROM u.k",
    )
    .bind(&u_doc)
    .bind(&u_seq)
    .bind(&u_no)
    .bind(&u_key)
    .execute(&mut *tx)
    .await
    .context("updating item entity keys")?;
    sqlx::query("DELETE FROM civic.assignments")
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM civic.entities")
        .execute(&mut *tx)
        .await?;

    let keys: Vec<String> = plan.entities.iter().map(|e| e.entity_key.clone()).collect();
    let names: Vec<String> = plan.entities.iter().map(|e| e.name.clone()).collect();
    let kinds: Vec<String> = plan.entities.iter().map(|e| e.kind.to_owned()).collect();
    for chunk in (0..keys.len()).collect::<Vec<_>>().chunks(1000) {
        let (k, n, kd): (Vec<_>, Vec<_>, Vec<_>) = chunk
            .iter()
            .map(|&i| (keys[i].clone(), names[i].clone(), kinds[i].clone()))
            .collect();
        sqlx::query(
            "INSERT INTO civic.entities (entity_key, name, kind)
             SELECT * FROM unnest($1::text[], $2::text[], $3::text[])",
        )
        .bind(&k)
        .bind(&n)
        .bind(&kd)
        .execute(&mut *tx)
        .await
        .context("inserting entities")?;
    }
    for e in plan.entities.iter().filter(|e| !e.aliases.is_empty()) {
        sqlx::query("UPDATE civic.entities SET aliases = $2 WHERE entity_key = $1")
            .bind(&e.entity_key)
            .bind(&e.aliases)
            .execute(&mut *tx)
            .await?;
    }
    for chunk in plan.assignments.chunks(1000) {
        sqlx::query(
            "INSERT INTO civic.assignments
               (entity_key, doc_id, seq, portfolio_title, portfolio_key, valid_from, valid_to, page)
             SELECT * FROM unnest($1::text[], $2::text[], $3::int[], $4::text[], $5::text[],
                                  $6::date[], $7::date[], $8::int[])",
        )
        .bind(
            chunk
                .iter()
                .map(|a| a.entity_key.clone())
                .collect::<Vec<_>>(),
        )
        .bind(chunk.iter().map(|a| a.doc_id.clone()).collect::<Vec<_>>())
        .bind(chunk.iter().map(|a| a.seq).collect::<Vec<_>>())
        .bind(
            chunk
                .iter()
                .map(|a| a.portfolio_title.clone())
                .collect::<Vec<_>>(),
        )
        .bind(
            chunk
                .iter()
                .map(|a| a.portfolio_key.clone())
                .collect::<Vec<_>>(),
        )
        .bind(chunk.iter().map(|a| a.valid_from).collect::<Vec<_>>())
        .bind(chunk.iter().map(|a| a.valid_to).collect::<Vec<_>>())
        .bind(chunk.iter().map(|a| a.page).collect::<Vec<_>>())
        .execute(&mut *tx)
        .await
        .context("inserting assignments")?;
    }
    tx.commit().await?;

    Ok(BuildReport {
        instruments: insts.len(),
        steps: steps.len(),
        entities: plan.entities.len(),
        assignments: plan.assignments.len(),
        open: plan
            .assignments
            .iter()
            .filter(|a| a.valid_to.is_none())
            .count(),
        shared_listings: plan.shared.len(),
        aliases_loaded: aliases.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).expect("date")
    }

    fn occ(key: &str, name: &str, seq: i32, title: &str, pkey: &str) -> Occ {
        Occ {
            doc_id: "gze-test".to_owned(),
            entity_key: key.to_owned(),
            name: name.to_owned(),
            seq,
            portfolio_title: title.to_owned(),
            portfolio_key: pkey.to_owned(),
            page: Some(3),
        }
    }

    fn baseline(id: &str, eff: NaiveDate, mut occs: Vec<Occ>) -> BaselineInput {
        for o in &mut occs {
            o.doc_id = id.to_owned();
        }
        BaselineInput {
            doc_id: id.to_owned(),
            effective_from: eff,
            occs,
        }
    }

    #[test]
    fn continuity_moves_removals_and_additions() {
        let b1 = baseline(
            "gze-1",
            d(2022, 7, 22),
            vec![
                occ(
                    "army",
                    "Sri Lanka Army",
                    1,
                    "Minister of Defence",
                    "defence",
                ),
                occ(
                    "customs",
                    "Sri Lanka Customs",
                    2,
                    "Minister of Finance",
                    "finance",
                ),
                occ(
                    "met",
                    "Department of Meteorology",
                    1,
                    "Minister of Defence",
                    "defence",
                ),
                occ("old", "Old Board", 1, "Minister of Defence", "defence"),
            ],
        );
        let b2 = baseline(
            "gze-2",
            d(2024, 9, 27),
            vec![
                occ(
                    "army",
                    "Sri Lanka Army",
                    1,
                    "Minister of Defence",
                    "defence",
                ),
                occ(
                    "customs",
                    "Sri Lanka Customs",
                    1,
                    "Minister of Finance, Planning",
                    "finance-planning",
                ),
                occ(
                    "met",
                    "Department of Meteorology",
                    3,
                    "Minister of Environment",
                    "environment",
                ),
                occ(
                    "new",
                    "New Authority",
                    3,
                    "Minister of Environment",
                    "environment",
                ),
            ],
        );
        let b3 = baseline(
            "gze-3",
            d(2024, 11, 25),
            vec![
                occ(
                    "army",
                    "Sri Lanka Army",
                    1,
                    "Minister of Defence",
                    "defence",
                ),
                occ(
                    "met",
                    "Department of Meteorology",
                    1,
                    "Minister of Defence",
                    "defence",
                ),
                occ(
                    "new",
                    "New Authority",
                    3,
                    "Minister of Environment",
                    "environment",
                ),
            ],
        );
        // given out of order: the plan sorts by date
        let p = plan(&[b3, b1, b2]);
        let rows = |k: &str| -> Vec<(NaiveDate, Option<NaiveDate>, String, String)> {
            p.assignments
                .iter()
                .filter(|a| a.entity_key == k)
                .map(|a| {
                    (
                        a.valid_from,
                        a.valid_to,
                        a.portfolio_key.clone(),
                        a.doc_id.clone(),
                    )
                })
                .collect()
        };
        // army: one continuous row across all three baselines, open at the end
        assert_eq!(
            rows("army"),
            vec![(d(2022, 7, 22), None, "defence".into(), "gze-1".into())]
        );
        // met: defence, then environment, then defence again; no row spans a move
        assert_eq!(
            rows("met"),
            vec![
                (
                    d(2022, 7, 22),
                    Some(d(2024, 9, 27)),
                    "defence".into(),
                    "gze-1".into()
                ),
                (
                    d(2024, 9, 27),
                    Some(d(2024, 11, 25)),
                    "environment".into(),
                    "gze-2".into()
                ),
                (d(2024, 11, 25), None, "defence".into(), "gze-3".into()),
            ]
        );
        // customs: moved to a differently keyed ministry, then not listed any more
        assert_eq!(
            rows("customs"),
            vec![
                (
                    d(2022, 7, 22),
                    Some(d(2024, 9, 27)),
                    "finance".into(),
                    "gze-1".into()
                ),
                (
                    d(2024, 9, 27),
                    Some(d(2024, 11, 25)),
                    "finance-planning".into(),
                    "gze-2".into()
                ),
            ]
        );
        // old: gone after the first baseline; new: added in the second, continuous after
        assert_eq!(
            rows("old"),
            vec![(
                d(2022, 7, 22),
                Some(d(2024, 9, 27)),
                "defence".into(),
                "gze-1".into()
            )]
        );
        assert_eq!(
            rows("new"),
            vec![(d(2024, 9, 27), None, "environment".into(), "gze-2".into())]
        );
        assert_eq!(p.entities.len(), 5);
        assert!(p.shared.is_empty());
    }

    #[test]
    fn wordings_become_aliases_and_the_latest_is_the_name() {
        let b1 = baseline(
            "a",
            d(2022, 1, 1),
            vec![occ(
                "x",
                "Sri Lanka Telecom Regulatory Commission",
                1,
                "M",
                "m",
            )],
        );
        let b2 = baseline(
            "b",
            d(2024, 1, 1),
            vec![occ(
                "x",
                "Telecommunications Regulatory Commission of Sri Lanka",
                1,
                "M",
                "m",
            )],
        );
        let p = plan(&[b1, b2]);
        assert_eq!(p.assignments.len(), 1);
        assert_eq!(
            p.entities[0].name,
            "Telecommunications Regulatory Commission of Sri Lanka"
        );
        assert_eq!(
            p.entities[0].aliases,
            vec!["Sri Lanka Telecom Regulatory Commission".to_owned()]
        );
        assert_eq!(p.entities[0].kind, "statutory_body");
    }

    #[test]
    fn a_wording_broken_by_a_line_wrap_is_not_the_name() {
        let b1 = baseline(
            "a",
            d(2022, 1, 1),
            vec![occ(
                "x",
                "State Development and Construction Corporation",
                1,
                "M",
                "m",
            )],
        );
        let b2 = baseline(
            "b",
            d(2024, 1, 1),
            vec![occ(
                "x",
                "State Development and Construc-tion Corporation",
                1,
                "M",
                "m",
            )],
        );
        let p = plan(&[b1, b2]);
        assert_eq!(
            p.entities[0].name,
            "State Development and Construction Corporation"
        );
        assert_eq!(
            p.entities[0].aliases,
            vec!["State Development and Construc-tion Corporation".to_owned()]
        );
    }

    #[test]
    fn an_institution_under_two_ministers_keeps_its_first_listing() {
        let b = baseline(
            "a",
            d(2022, 1, 1),
            vec![
                occ("x", "Shared Board", 1, "Minister of A", "a"),
                occ("x", "Shared Board", 4, "Minister of D", "d"),
            ],
        );
        let p = plan(&[b]);
        assert_eq!(p.assignments.len(), 1);
        assert_eq!(p.assignments[0].portfolio_key, "a");
        assert_eq!(p.shared, vec![("a".to_owned(), "x".to_owned())]);
    }

    #[test]
    fn two_baselines_on_one_day_keep_the_later() {
        let b1 = baseline("a", d(2022, 1, 1), vec![occ("x", "X", 1, "M", "m")]);
        let b2 = baseline("b", d(2022, 1, 1), vec![occ("y", "Y", 1, "M", "m")]);
        let p = plan(&[b1, b2]);
        assert_eq!(p.assignments.len(), 1);
        assert_eq!(p.assignments[0].entity_key, "y");
    }
}
