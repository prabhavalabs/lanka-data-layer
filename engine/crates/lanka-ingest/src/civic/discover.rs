//! `lanka civic discover` (13 §2.1): candidate instruments from document
//! titles, confirmed from the English text where the file has arrived.
//!
//! The titles are the publisher's ("Presidential Secretariat - Duties and
//! Functions of the Ministers."). A candidate matches a phrase for the
//! assignment of subjects and functions and none of the exclusions (state and
//! deputy ministers, provincial councils and governors, non-cabinet ministers,
//! appointments of secretaries). Confirmation reads the notification
//! paragraph: it must cite Article 44 (or 43) of the Constitution.

use std::sync::LazyLock;

use anyhow::Result;
use chrono::NaiveDate;
use lanka_store::{BlobRef, BlobStore, Ns};
use regex::Regex;
use serde::Serialize;
use sqlx::PgPool;

use super::instrument::{self, Info};

static INCLUDE: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"subjects?\s*(?:&|and)\s*functions",
        r"(?:duties|functions)\s*(?:&|and)\s*(?:duties|functions)",
        r"assignment\s+of\s+subjects",
        r"cabinet\s+of\s+ministers\s+and\s+the\s+ministries",
    ]
    .iter()
    .map(|p| Regex::new(&format!("(?i){p}")).expect("include regex"))
    .collect()
});

static EXCLUDE: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"state\s+minist",
        r"deputy",
        r"provinc",
        r"governor",
        r"non\s*-?\s*cabinet",
        r"appoint|resign",
        r"secretar(?:y|ies)\s+(?:to|of|for)\b",
        r"customs|import\s+duties|anti.?dumping|countervailing|revenue\s+protection",
    ]
    .iter()
    .map(|p| Regex::new(&format!("(?i){p}")).expect("exclude regex"))
    .collect()
});

/// Does an (English) title look like an assignment of subjects and functions
/// to Cabinet ministers?
#[must_use]
pub fn is_candidate_title(title: &str) -> bool {
    INCLUDE.iter().any(|r| r.is_match(title)) && !EXCLUDE.iter().any(|r| r.is_match(title))
}

/// What the confirmation read from the text.
#[derive(Debug, Clone, Serialize)]
pub struct Confirmation {
    pub kind: Option<&'static str>,
    pub effective_from: NaiveDate,
    pub amends: Option<String>,
    pub head_of_state: Option<String>,
    pub articles: Vec<u32>,
    /// Cites Article 44 or 43.
    pub confirmed: bool,
}

impl Confirmation {
    fn of(i: &Info) -> Self {
        Self {
            kind: i.kind.map(instrument::Kind::as_str),
            effective_from: i.effective_from,
            amends: i.amends.clone(),
            head_of_state: i.head_of_state.clone(),
            articles: i.articles.clone(),
            confirmed: i.is_cabinet_notification(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    pub doc_id: String,
    pub number: Option<String>,
    pub issued_on: NaiveDate,
    pub title: String,
    /// Status of the English file; `None` = the document has none.
    pub en_status: Option<String>,
    /// Status in `civic.instruments`, when already extracted.
    pub extracted: Option<String>,
    /// From the text; `None` while the English file is not archived with text.
    pub confirmation: Option<Confirmation>,
}

/// A text blob of the store by its sha256 (hex).
pub async fn read_text(store: &dyn BlobStore, sha_hex: &str) -> Result<String> {
    let raw = hex::decode(sha_hex)?;
    let sha256: [u8; 32] = raw
        .try_into()
        .map_err(|_| anyhow::anyhow!("text sha256 is not 32 bytes"))?;
    let r = BlobRef {
        ns: Ns::Text,
        key: Ns::Text.cas_key(&sha256),
        sha256,
        bytes: 0,
    };
    let b = store.get(&r).await?;
    Ok(String::from_utf8_lossy(&b).into_owned())
}

/// How many pages from the start of a gazette the notification may span (a
/// cover and contents page can come first, as in 2187/27).
const LEAD_PAGES: usize = 6;

/// The first pages of a text blob (pages are separated by `\f`), joined: the
/// notification and its signature are somewhere in them.
#[must_use]
pub fn lead_pages(text: &str) -> String {
    text.split('\u{c}')
        .take(LEAD_PAGES)
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(sqlx::FromRow)]
struct Row {
    doc_id: String,
    number: Option<String>,
    issued_on: NaiveDate,
    title: Option<String>,
    en_status: Option<String>,
    text_sha256: Option<String>,
    extracted: Option<String>,
}

/// Candidates from every document title, oldest first.
pub async fn discover(pool: &PgPool, store: &dyn BlobStore) -> Result<Vec<Candidate>> {
    let rows = sqlx::query_as::<_, Row>(
        "SELECT d.doc_id,
                CASE WHEN d.number IS NULL THEN NULL ELSE d.number || COALESCE('/' || d.sub_number, '') END AS number,
                d.issued_on, d.title ->> 'en' AS title,
                f.status AS en_status, f.text_sha256, i.status AS extracted
         FROM docs.documents d
         LEFT JOIN docs.files f ON f.doc_id = d.doc_id AND f.lang = 'en'
         LEFT JOIN civic.instruments i ON i.doc_id = d.doc_id
         WHERE d.withdrawn_at IS NULL
           AND d.title ->> 'en' ~* '(subjects|duties|functions|assignment|ministries)'
         ORDER BY d.issued_on, d.doc_id",
    )
    .fetch_all(pool)
    .await?;
    let mut out = Vec::new();
    for r in rows {
        let title = r.title.clone().unwrap_or_default();
        if !is_candidate_title(&title) {
            continue;
        }
        let confirmation = match (r.en_status.as_deref(), r.text_sha256.as_deref()) {
            (Some("ok"), Some(sha)) => match read_text(store, sha).await {
                Ok(t) => Some(Confirmation::of(&instrument::analyse(
                    &lead_pages(&t),
                    r.issued_on,
                ))),
                Err(e) => {
                    tracing::warn!(doc_id = %r.doc_id, error = %e, "candidate text not readable");
                    None
                }
            },
            _ => None,
        };
        out.push(Candidate {
            doc_id: r.doc_id,
            number: r.number,
            issued_on: r.issued_on,
            title,
            en_status: r.en_status,
            extracted: r.extracted,
            confirmation,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_of_the_instruments_match() {
        for t in [
            "Presidential Secretariat - Duties and Functions of the Ministers.",
            "Presidential secretariat - Subjects and Functions.",
            "Presidential Secretariat - Cabinet of Ministers and the Ministries and the assignment of Subjects and Functions and Departments, Public Corporations and Statutary Institutions",
            "Presidential Secretariat - The Constitution of the Democratic Socialist Republic of Sri Lanka. - Subjects and Functions.",
            "Presidential Secretariat - Duties & Functions of the Ministers",
            "Presidential Secretariat - Amended Functions and Duties Extra Gazette No. 2289/43 of 22 July, 2022",
            "Presidential Secretariat - The Assignment of Subjects and Functions & Departments, State Corporations and Statutory Institutions to the Ministers",
        ] {
            assert!(is_candidate_title(t), "{t}");
        }
    }

    #[test]
    fn state_deputy_provincial_and_appointments_are_out() {
        for t in [
            "Presidential Secretariat - Duties & Functions assigned to the State Minister of Defence w.e.f. 10.02.2015",
            "Presidential Secretariat - Notify the Functions & Duties for the State Ministries",
            "Presidential Secretariat - Duties and Functions of the non - Cabinet Ministers",
            "Presidential Secretariat - Amendment to the Subjects & Functions - Delegation of Powers and Duties to Hon. N. Kothalawala Deputy Minister of Highways",
            "Secretariat of the Governor - Eastern Province - Amended that the Subject of the Ministries and Cluster of Eastern Provincial Council with Effect from 01st of June 2024",
            "Presidential Secretariat - Appointed Ranil Wickremesinghe Prime Minister with effect from 13.07.2022 to exercise, perform and discharge the powers, duties and functions of the office",
            "Presidential Secretariat - Appointments of Secretaries to Ministries",
            "Department of Commerce - The Anti - Dumping and Countervailing Duties Act, No 2 of 2018 - Regulations",
            "Presidential Secretariat - Resigned the Governors on their Duties with Effect from 25.09.2024",
        ] {
            assert!(!is_candidate_title(t), "{t}");
        }
    }
}
