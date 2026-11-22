//! `lanka-extractd`: the document sandbox (ADR 0009, 07 §5).
//!
//! poppler and tesseract are C/C++ with a history of memory-safety bugs, and
//! the PDFs they read come from ~170 upstream sites. They never run in the
//! process that holds database credentials: the daemon ([`server`]) runs in
//! the `tools` container — no network, read-only root, all capabilities
//! dropped, memory and pid limits — and executes a fixed set of operations
//! ([`ops::Op`]) with fixed argv, per-call `ulimit`s and a wall-clock
//! timeout. The engine talks to it through [`Sandbox`] over a Unix socket and
//! parses the tool output in pure Rust ([`words`]).
//!
//! A malicious document can at worst crash or time out one tool invocation
//! in a container with nothing to steal and nowhere to send it.

pub mod client;
pub mod ops;
pub mod proto;
pub mod server;
pub mod words;

pub use client::Sandbox;
pub use ops::{Limits, Op, Tools};
pub use words::{PageBox, Word, Words};
