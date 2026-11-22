//! Code connectors (03 §2.3): irregular sources whose parsing needs logic the
//! declarative drivers do not have. Each implements
//! [`crate::connector::Connector`] (discover + extract + standardise).
//! World Bank WDI moved to `registry/connectors/worldbank-wdi.yaml` on the
//! `http_json` driver.

pub mod cod_ab;
pub mod dcs_census;
