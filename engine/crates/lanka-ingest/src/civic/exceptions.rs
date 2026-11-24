//! `registry/civic/exceptions.yaml`: reviewed facts about single gazettes that
//! the checks must know. Today one kind: `expected_gaps`, numbers the gazette
//! itself skips (printed `4.` then `6.`), so the sequence check passes with a
//! note instead of failing on something no parser can fix.
//!
//! `expected_citations: [{heading, item_no, note}]` likewise lets an institution
//! whose real name cites an act pass `institution_text_clean`.
//!
//! ```yaml
//! gze-1933-13:
//!   expected_gaps:
//!     - { heading: 38, column: I, missing: [5], note: "the gazette prints 4 then 6" }
//! ```

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ExpectedGap {
    /// The minister's heading number.
    pub heading: i32,
    /// `I` (functions) or `II` (institutions).
    pub column: String,
    pub missing: Vec<i32>,
    pub note: String,
}

/// An institution whose real name cites an act (`… under the Act, No. 43 of 2011`):
/// the `institution_text_clean` check lets it pass.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ExpectedCitation {
    pub heading: i32,
    pub item_no: i32,
    pub note: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct DocEntry {
    #[serde(default)]
    expected_gaps: Vec<ExpectedGap>,
    #[serde(default)]
    expected_citations: Vec<ExpectedCitation>,
}

#[derive(Debug, Clone, Default)]
pub struct Exceptions {
    docs: HashMap<String, DocEntry>,
}

impl Exceptions {
    pub fn parse(yaml: &str) -> Result<Self> {
        let docs: Option<HashMap<String, DocEntry>> =
            serde_yaml_ng::from_str(yaml).context("exceptions.yaml is not a map of documents")?;
        let docs = docs.unwrap_or_default();
        for (id, e) in &docs {
            for g in &e.expected_gaps {
                if !matches!(g.column.as_str(), "I" | "II") {
                    bail!(
                        "exceptions.yaml: {id}: column must be I or II, not {:?}",
                        g.column
                    );
                }
                if g.missing.is_empty() || g.note.trim().is_empty() {
                    bail!("exceptions.yaml: {id}: a gap needs `missing` numbers and a `note`");
                }
            }
        }
        Ok(Self { docs })
    }

    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(s) => Self::parse(&s).with_context(|| path.display().to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    /// The institutions of one gazette whose names legitimately cite an act.
    #[must_use]
    pub fn citations(&self, doc_id: &str) -> &[ExpectedCitation] {
        self.docs
            .get(doc_id)
            .map_or(&[], |e| e.expected_citations.as_slice())
    }

    /// The expected gaps of one gazette.
    #[must_use]
    pub fn gaps(&self, doc_id: &str) -> &[ExpectedGap] {
        self.docs
            .get(doc_id)
            .map_or(&[], |e| e.expected_gaps.as_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaps_are_read_per_document() {
        let e = Exceptions::parse(
            "gze-1933-13:\n  expected_gaps:\n    - { heading: 38, column: I, missing: [5], note: \"prints 4 then 6\" }\n",
        )
        .expect("parse");
        assert_eq!(e.gaps("gze-1933-13").len(), 1);
        assert_eq!(e.gaps("gze-1933-13")[0].missing, vec![5]);
        assert!(e.gaps("gze-2412-08").is_empty());
        assert!(Exceptions::parse("").expect("empty").gaps("x").is_empty());
        assert!(
            Exceptions::parse(
                "a:\n  expected_gaps:\n    - { heading: 1, column: III, missing: [1], note: x }\n"
            )
            .is_err()
        );
        assert!(
            Exceptions::parse(
                "a:\n  expected_gaps:\n    - { heading: 1, column: I, missing: [1], note: \"\" }\n"
            )
            .is_err()
        );
    }
}
