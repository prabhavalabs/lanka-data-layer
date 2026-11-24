//! Track C1 (13 §2.1): ministries and the institutions in their charge, over
//! time, parsed from the President's Article 44 gazettes.
//!
//! | module | what |
//! |---|---|
//! | [`schedule`] | pure parser: words of a gazette to ministers, columns and items |
//! | [`validate`] | the checks stored with an instrument (`checks`) |
//! | [`entity`] | entity keys, kind guess, the aliases file |
//! | [`instrument`] | baseline vs amendment, effective date, amended gazette, head of state (pure) |
//! | [`discover`] | candidate instruments from document titles, confirmed from the text |
//! | [`extract`] | the pipeline: archive blobs, parse, validate, store |
//! | [`build`] | assignments with continuous intervals; the diff of two dates |

pub mod build;
pub mod discover;
pub mod entity;
pub mod exceptions;
pub mod extract;
pub mod instrument;
pub mod schedule;
pub mod schedule_b;
pub mod suggest;
pub mod validate;

/// Parser identifier stored in `civic.instruments.parser`.
pub const PARSER: &str = "schedule/1";
