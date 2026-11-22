use std::borrow::Cow;

/// Errors surfaced by the query core. Transports map them to status codes:
/// `NotFound`/`NotInCoverage` → 404, `BadRequest` → 400, `Db` → 500 (masked).
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("not_found: {0}")]
    NotFound(Cow<'static, str>),
    #[error("not_in_coverage")]
    NotInCoverage,
    #[error("bad_request: {0}")]
    BadRequest(Cow<'static, str>),
    /// A 400 with a machine-readable problem (`{code, …, valid}`) that the
    /// transports put in the error envelope's `payload.problem`.
    #[error("bad_request: {message}")]
    Invalid {
        message: String,
        problem: serde_json::Value,
    },
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

impl CoreError {
    pub fn not_found(msg: impl Into<Cow<'static, str>>) -> Self {
        Self::NotFound(msg.into())
    }
    pub fn bad_request(msg: impl Into<Cow<'static, str>>) -> Self {
        Self::BadRequest(msg.into())
    }
    pub fn invalid(message: impl Into<String>, problem: serde_json::Value) -> Self {
        Self::Invalid {
            message: message.into(),
            problem,
        }
    }
}

pub type CoreResult<T> = Result<T, CoreError>;
