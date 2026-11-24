//! `lanka civic aliases suggest` (13 §2.1): candidate aliases for a person to
//! review. Every pair of entity keys found in the instruments is compared with
//! trigram similarity (pg_trgm, on the de-slugged names); pairs that already
//! resolve to one key, or that `aliases.yaml` already joins, are skipped. The
//! candidates are written to `registry/civic/alias-suggestions.yaml`, most
//! similar first. Nothing is ever applied automatically: a reviewer copies the
//! pairs that are clearly one body into `aliases.yaml`.

use anyhow::Result;
use serde::Serialize;
use sqlx::PgPool;

use super::entity::Aliases;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Suggestion {
    pub a: String,
    pub b: String,
    pub similarity: f64,
    /// Gazette numbers (`2412/08`) in which either key is listed, oldest first.
    pub seen_in: Vec<String>,
}

#[derive(sqlx::FromRow)]
struct Pair {
    a: String,
    b: String,
    sim: f32,
}

/// Pairs of entity keys with similarity of at least `min`, most similar first.
pub async fn suggest(pool: &PgPool, aliases: &Aliases, min: f64) -> Result<Vec<Suggestion>> {
    let pairs: Vec<Pair> = sqlx::query_as(
        "WITH k AS (SELECT DISTINCT entity_key FROM civic.items WHERE entity_key IS NOT NULL)
         SELECT a.entity_key AS a, b.entity_key AS b,
                similarity(replace(a.entity_key, '-', ' '), replace(b.entity_key, '-', ' ')) AS sim
         FROM k a JOIN k b ON a.entity_key < b.entity_key
         WHERE similarity(replace(a.entity_key, '-', ' '), replace(b.entity_key, '-', ' ')) >= $1
         ORDER BY sim DESC, a.entity_key, b.entity_key",
    )
    .bind(min as f32)
    .fetch_all(pool)
    .await?;
    let seen: Vec<(String, String)> = sqlx::query_as(
        "SELECT t.entity_key, string_agg(DISTINCT d.number || '/' || d.sub_number, ' ' ORDER BY d.number || '/' || d.sub_number)
         FROM civic.items t JOIN docs.documents d ON d.doc_id = t.doc_id
         WHERE t.entity_key IS NOT NULL GROUP BY t.entity_key",
    )
    .fetch_all(pool)
    .await?;
    let docs_of = |k: &str| -> Vec<String> {
        seen.iter()
            .find(|s| s.0 == k)
            .map(|s| s.1.split(' ').map(str::to_owned).collect())
            .unwrap_or_default()
    };
    Ok(pairs
        .into_iter()
        .filter(|p| !aliases.joins(&p.a, &p.b))
        .map(|p| {
            let mut seen_in = docs_of(&p.a);
            seen_in.extend(docs_of(&p.b));
            seen_in.sort();
            seen_in.dedup();
            Suggestion {
                a: p.a,
                b: p.b,
                similarity: (f64::from(p.sim) * 1000.0).round() / 1000.0,
                seen_in,
            }
        })
        .collect())
}

/// The review file: a header, then one flow-style entry per candidate.
#[must_use]
pub fn render(suggestions: &[Suggestion], min: f64) -> String {
    let mut out = format!(
        "# Alias candidates (lanka civic aliases suggest --min {min}). FOR REVIEW: nothing here is applied.\n\
         # Copy the pairs that are clearly one body (spelling, plural, a missing space, \"Co.\") into\n\
         # aliases.yaml as `old wording: newest wording`. Do not merge different bodies.\n"
    );
    for s in suggestions {
        out.push_str(&format!(
            "- {{ a: {}, b: {}, similarity: {}, seen_in: [{}] }}\n",
            s.a,
            s.b,
            s.similarity,
            s.seen_in
                .iter()
                .map(|d| format!("\"{d}\""))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_review_file_lists_pairs_most_similar_first() {
        let s = vec![Suggestion {
            a: "palmyra-development-board".into(),
            b: "palmyrah-development-board".into(),
            similarity: 0.917,
            seen_in: vec!["2153/12".into(), "2187/27".into()],
        }];
        let r = render(&s, 0.85);
        assert!(r.contains("FOR REVIEW"));
        assert!(r.contains(r#"- { a: palmyra-development-board, b: palmyrah-development-board, similarity: 0.917, seen_in: ["2153/12", "2187/27"] }"#));
        // the file is valid YAML
        let parsed: Vec<serde_json::Value> = serde_yaml_ng::from_str(&r).expect("yaml");
        assert_eq!(parsed[0]["similarity"], 0.917);
    }
}
