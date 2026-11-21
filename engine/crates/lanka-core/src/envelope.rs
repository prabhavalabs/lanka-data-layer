//! API envelope `{ success, message, payload, meta }` — same shape as /v1
//! (docs/architecture.md §4), with the v2 meta additions: dataset versions,
//! licences and ready-to-send `next` links.

use serde::Serialize;
use utoipa::ToSchema;

/// Schema version of the v2 data contract (docs/data-platform-v2.md).
pub const CONTRACT_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Envelope<T> {
    pub success: bool,
    pub message: String,
    pub payload: Option<T>,
    pub meta: Meta,
}

impl<T> Envelope<T> {
    pub fn ok(payload: T, meta: Meta) -> Self {
        Self {
            success: true,
            message: "ok".to_owned(),
            payload: Some(payload),
            meta,
        }
    }
}

impl Envelope<()> {
    pub fn error(message: impl Into<String>, meta: Meta) -> Self {
        Self {
            success: false,
            message: message.into(),
            payload: None,
            meta,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, ToSchema)]
pub struct Meta {
    /// Release id (`YYYYMMDD.N`); null before the first release.
    pub data_version: Option<String>,
    pub contract_version: u32,
    /// Exact dataset versions behind this response.
    pub dataset_versions: Vec<DatasetVersionRef>,
    /// Attribution for every source touched.
    pub source: Vec<SourceRef>,
    pub licences: Vec<LicenceRef>,
    /// Ready-to-send follow-up calls.
    pub next: Vec<Link>,
}

impl Meta {
    #[must_use]
    pub fn bare(data_version: Option<String>) -> Self {
        Self {
            data_version,
            contract_version: CONTRACT_VERSION,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct DatasetVersionRef {
    pub dataset: String,
    pub dsv: String,
    pub vintage: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct SourceRef {
    pub id: String,
    pub publisher: String,
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct LicenceRef {
    pub id: String,
    /// open | attribution | permission_unclear | restricted
    pub status: String,
    pub url: Option<String>,
    pub attribution: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct Link {
    pub rel: String,
    pub href: String,
}

impl Link {
    pub fn new(rel: impl Into<String>, href: impl Into<String>) -> Self {
        Self {
            rel: rel.into(),
            href: href.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ok_envelope_shape() {
        let mut meta = Meta::bare(Some("20261121.1".into()));
        meta.next
            .push(Link::new("series", "/v2/indicators/x/series?geo=LK"));
        let v = serde_json::to_value(Envelope::ok(serde_json::json!({"a": 1}), meta))
            .expect("serializes");
        assert_eq!(v["success"], true);
        assert_eq!(v["message"], "ok");
        assert_eq!(v["payload"]["a"], 1);
        assert_eq!(v["meta"]["data_version"], "20261121.1");
        assert_eq!(v["meta"]["contract_version"], CONTRACT_VERSION);
        assert_eq!(v["meta"]["next"][0]["rel"], "series");
        for key in ["dataset_versions", "source", "licences"] {
            assert!(v["meta"][key].is_array(), "{key} must always be present");
        }
    }

    #[test]
    fn error_envelope_has_null_payload() {
        let v = serde_json::to_value(Envelope::error("not_found", Meta::bare(None)))
            .expect("serializes");
        assert_eq!(v["success"], false);
        assert_eq!(v["message"], "not_found");
        assert!(v["payload"].is_null());
        assert!(v["meta"]["data_version"].is_null());
    }
}
