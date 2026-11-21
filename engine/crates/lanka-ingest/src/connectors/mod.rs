//! Source connectors. One module per upstream source; each implements
//! [`crate::connector::Connector`] (discover + parse only).

pub mod cod_ab;
pub mod dcs_census;
pub mod world_bank;
