//! `app.map_snapshots` (migration 0009; 10 §5): anonymous, immutable,
//! content-addressed map documents.

use chrono::{DateTime, NaiveDate, Utc};
use lanka_mapdoc::canon;
use serde_json::Value;
use sqlx::PgPool;

/// A validated document ready to store.
#[derive(Debug, Clone)]
pub struct Prepared {
    /// `ms_` + 16 hex of the content hash.
    pub id: String,
    pub sha256: [u8; 32],
    /// The document with `id` set to the snapshot id.
    pub doc: Value,
    /// Canonical bytes of the content (what the 256 KB limit counts).
    pub bytes: usize,
}

/// Content id, hash and stored form of a validated document.
#[must_use]
pub fn prepare(v: &Value) -> Prepared {
    let sha256 = canon::content_sha256(v);
    let id = format!("ms_{}", &hex::encode(sha256)[..16]);
    let bytes = canon::jcs(&canon::content_value(v)).len();
    let mut doc = v.clone();
    if let Some(o) = doc.as_object_mut() {
        o.insert("id".to_owned(), Value::String(id.clone()));
    }
    Prepared {
        id,
        sha256,
        doc,
        bytes,
    }
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub id: String,
    pub doc: Value,
    pub created_at: DateTime<Utc>,
    pub withdrawn: bool,
}

/// `Some(withdrawn)` when the snapshot exists.
pub async fn state(pool: &PgPool, id: &str) -> sqlx::Result<Option<bool>> {
    sqlx::query_scalar("SELECT withdrawn FROM app.map_snapshots WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// Outcome of [`insert`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stored {
    Created,
    /// The same content was stored before (possibly concurrently).
    Existing,
}

/// Stores `p`; idempotent on content.
pub async fn insert(
    pool: &PgPool,
    p: &Prepared,
    created_by: Option<i64>,
    ip_hash: Option<&[u8; 32]>,
) -> anyhow::Result<Stored> {
    let bytes = i32::try_from(p.bytes)?;
    let created = sqlx::query(
        "INSERT INTO app.map_snapshots (id, doc, doc_sha256, bytes, created_by, created_ip_hash)
         VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT DO NOTHING",
    )
    .bind(&p.id)
    .bind(&p.doc)
    .bind(p.sha256.as_slice())
    .bind(bytes)
    .bind(created_by)
    .bind(ip_hash.map(<[u8; 32]>::as_slice))
    .execute(pool)
    .await?
    .rows_affected()
        == 1;
    if created {
        return Ok(Stored::Created);
    }
    let sha: Option<Vec<u8>> =
        sqlx::query_scalar("SELECT doc_sha256 FROM app.map_snapshots WHERE id = $1")
            .bind(&p.id)
            .fetch_optional(pool)
            .await?;
    match sha {
        Some(s) if s == p.sha256 => Ok(Stored::Existing),
        Some(_) => anyhow::bail!("snapshot id collision on {}", p.id),
        None => anyhow::bail!("snapshot {} conflicted but is not stored", p.id),
    }
}

/// Reads a snapshot and marks it read today (at most one write a day).
pub async fn get(pool: &PgPool, id: &str) -> sqlx::Result<Option<Snapshot>> {
    let row: Option<(Value, DateTime<Utc>, bool, NaiveDate)> = sqlx::query_as(
        "SELECT doc, created_at, withdrawn, last_read_at FROM app.map_snapshots WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    let Some((doc, created_at, withdrawn, last_read)) = row else {
        return Ok(None);
    };
    if !withdrawn && last_read < Utc::now().date_naive() {
        sqlx::query(
            "UPDATE app.map_snapshots SET last_read_at = current_date
             WHERE id = $1 AND last_read_at < current_date",
        )
        .bind(id)
        .execute(pool)
        .await?;
    }
    Ok(Some(Snapshot {
        id: id.to_owned(),
        doc,
        created_at,
        withdrawn,
    }))
}

/// Deletes anonymous snapshots unread for 24 months (10 §5); returns the count.
pub async fn expire(pool: &PgPool) -> sqlx::Result<u64> {
    Ok(sqlx::query(
        "DELETE FROM app.map_snapshots
         WHERE created_by IS NULL AND last_read_at < current_date - interval '24 months'",
    )
    .execute(pool)
    .await?
    .rows_affected())
}

/// `ms_` + 16 lowercase hex.
#[must_use]
pub fn is_snapshot_id(s: &str) -> bool {
    s.strip_prefix("ms_").is_some_and(|h| {
        h.len() == 16
            && h.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn prepared_documents_carry_their_id() {
        let v =
            json!({"spec": "ldl-map/1", "id": "m_old", "title": "t", "meta": {"updated_at": "x"}});
        let p = prepare(&v);
        assert!(is_snapshot_id(&p.id), "{}", p.id);
        assert_eq!(p.doc["id"], p.id.as_str());
        // the stored form hashes to its own id
        assert_eq!(canon::snapshot_id(&p.doc), p.id);
        assert!(!is_snapshot_id("ms_0123"));
        assert!(!is_snapshot_id("ms_0123456789ABCDEF"));
    }
}
