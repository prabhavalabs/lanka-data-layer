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
}

pub type CoreResult<T> = Result<T, CoreError>;
