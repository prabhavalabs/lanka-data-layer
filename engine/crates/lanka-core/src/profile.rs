//! The country profile (`country/<id>/profile.yaml`): everything
//! country-specific the engine needs — grid parameters, admin code rules,
//! reference boundaries, languages, calendars, other geography schemes.
//!
//! The engine contains no country literal. A profile is loaded from disk by
//! ingestion (and synced to `catalog.profile`), and read back from the
//! database by servers, so `lanka serve` needs no filesystem access to it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::geo_code::AdminCodes;
use crate::grid::Grid;
use crate::period::Calendars;

#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("reading {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("parsing {path}: {msg}")]
    Parse { path: PathBuf, msg: String },
    #[error("invalid profile: {0}")]
    Invalid(String),
}

/// Language-tagged label; `en` is mandatory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Label {
    pub en: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub si: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ta: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Iso3166 {
    pub alpha2: String,
    pub alpha3: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Site {
    pub base_url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GridSpec {
    pub lat_max: f64,
    pub lon_min: f64,
    pub step_deg: f64,
    pub rows: u32,
    pub cols: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminLevel {
    pub level: u8,
    /// Digits appended to the root code at this level (cumulative length).
    pub digits: u8,
    pub name: Label,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminSpec {
    pub scheme: String,
    pub pcode_pattern: String,
    pub root_code: String,
    pub levels: Vec<AdminLevel>,
    pub reference_boundary_set: String,
    pub boundary_dataset: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonthStart {
    pub start_month: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeasonSpec {
    pub id: String,
    /// `MM-DD` within the named year.
    pub start: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarSpec {
    pub fiscal: MonthStart,
    pub agri: MonthStart,
    #[serde(default)]
    pub seasons: Vec<SeasonSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemeSpec {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nests_in: Option<String>,
}

/// `country/<id>/profile.yaml`, deserialised.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileSpec {
    pub id: String,
    pub name: Label,
    pub iso3166: Iso3166,
    pub site: Site,
    pub languages: Vec<String>,
    pub timezone: String,
    pub currency: String,
    pub grid: GridSpec,
    pub admin: AdminSpec,
    pub metric_crs: String,
    pub calendars: CalendarSpec,
    #[serde(default)]
    pub schemes: Vec<SchemeSpec>,
    #[serde(default)]
    pub suppression: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub fetch: FetchSpec,
}

/// How connectors reach the country's publishers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FetchSpec {
    /// Hosts (exact names) fetched with the platform TLS stack instead of
    /// rustls: servers offering only TLS 1.2 CBC suites.
    #[serde(default)]
    pub legacy_tls_hosts: Vec<String>,
}

/// A validated profile plus the derived runtime helpers.
#[derive(Debug, Clone)]
pub struct Profile {
    pub spec: ProfileSpec,
    /// The profile as JSON (stored verbatim in `catalog.profile.spec`).
    pub json: serde_json::Value,
    /// sha256 of the source file (recorded with releases).
    pub sha256: [u8; 32],
    pub admin: AdminCodes,
    pub grid: Grid,
    pub calendars: Calendars,
}

impl Profile {
    /// Loads and validates a profile file.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ProfileError> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|source| ProfileError::Io {
            path: path.to_owned(),
            source,
        })?;
        let json: serde_json::Value =
            serde_yaml_ng::from_slice(&bytes).map_err(|e| ProfileError::Parse {
                path: path.to_owned(),
                msg: e.to_string(),
            })?;
        let sha256: [u8; 32] = Sha256::digest(&bytes).into();
        Self::from_json(json, sha256)
    }

    /// Builds a profile from its JSON form (as stored in `catalog.profile`).
    pub fn from_json(json: serde_json::Value, sha256: [u8; 32]) -> Result<Self, ProfileError> {
        let spec: ProfileSpec =
            serde_json::from_value(json.clone()).map_err(|e| ProfileError::Parse {
                path: PathBuf::from("<profile>"),
                msg: e.to_string(),
            })?;
        let admin = AdminCodes::from_spec(&spec.admin)?;
        let grid = Grid::from_spec(&spec.grid)?;
        let calendars = Calendars::from_spec(&spec.calendars)?;
        if spec.languages.first().map(String::as_str) != Some("en") {
            return Err(ProfileError::Invalid(
                "languages must start with \"en\" (the mandatory fallback)".into(),
            ));
        }
        Ok(Self {
            spec,
            json,
            sha256,
            admin,
            grid,
            calendars,
        })
    }

    /// Finds the profile next to a registry: `LANKA_COUNTRY_PROFILE` if set,
    /// else the only `country/*/profile.yaml` beside `registry_dir`.
    pub fn discover(registry_dir: &Path) -> Result<PathBuf, ProfileError> {
        if let Some(p) = std::env::var_os("LANKA_COUNTRY_PROFILE") {
            return Ok(PathBuf::from(p));
        }
        let country = registry_dir
            .parent()
            .unwrap_or(Path::new("."))
            .join("country");
        let mut found: Vec<PathBuf> = std::fs::read_dir(&country)
            .map_err(|source| ProfileError::Io {
                path: country.clone(),
                source,
            })?
            .filter_map(|e| e.ok().map(|e| e.path().join("profile.yaml")))
            .filter(|p| p.is_file())
            .collect();
        found.sort();
        match found.len() {
            1 => Ok(found.remove(0)),
            0 => Err(ProfileError::Invalid(format!(
                "no country/*/profile.yaml under {}",
                country.display()
            ))),
            _ => Err(ProfileError::Invalid(format!(
                "several country profiles under {}; set LANKA_COUNTRY_PROFILE",
                country.display()
            ))),
        }
    }

    /// The scheme id of admin units (bare p-codes).
    #[must_use]
    pub fn admin_scheme(&self) -> &str {
        &self.spec.admin.scheme
    }

    /// Dataset that loads the reference admin boundaries.
    #[must_use]
    pub fn boundary_dataset(&self) -> &str {
        &self.spec.admin.boundary_dataset
    }

    #[must_use]
    pub fn reference_boundary_set(&self) -> &str {
        &self.spec.admin.reference_boundary_set
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn repo_profile() -> Profile {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let path = Profile::discover(&root.join("registry")).expect("one profile");
        Profile::load(path).expect("repository profile must load")
    }

    #[test]
    fn repository_profile_loads() {
        let p = repo_profile();
        assert_eq!(p.spec.languages[0], "en");
        assert!(p.admin.parse(&p.spec.admin.root_code).is_ok());
        assert_eq!(
            p.grid,
            Grid::CANONICAL,
            "profile grid must match the v1 contract"
        );
    }
}
