//! The engine side: [`Sandbox`] sends an [`Op`] to the daemon over its Unix
//! socket, or — in development, when no socket is configured — runs the same
//! allow-listed operation as a local subprocess under the same limits.
//!
//! | `LANKA_EXTRACTD_SOCKET` | `LANKA_EXTRACTD` | backend |
//! |---|---|---|
//! | set | any | socket (the `tools` container) |
//! | unset | unset / `local` | local subprocess (dev; logs a warning once) |
//! | unset | `require` | error: production refuses to run tools in-process |

use std::path::{Path, PathBuf};
use std::sync::Once;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use tokio::net::UnixStream;

use crate::ops::{self, Limits, Op, Tools};
use crate::proto::{Reply, read_frame, write_frame};
use crate::words::{Words, parse_bbox_layout, parse_hocr};

#[derive(Debug, Clone)]
pub enum Sandbox {
    /// The daemon in the `tools` container.
    Socket { path: PathBuf, timeout: Duration },
    /// Local subprocesses (development and tests).
    Local { tools: Tools, limits: Limits },
}

static WARN_LOCAL: Once = Once::new();

impl Sandbox {
    /// The backend configured by the environment (see the module table).
    pub fn from_env() -> Result<Self> {
        if let Some(p) = std::env::var_os("LANKA_EXTRACTD_SOCKET").filter(|v| !v.is_empty()) {
            return Ok(Self::socket(PathBuf::from(p)));
        }
        match std::env::var("LANKA_EXTRACTD").ok().as_deref() {
            Some("require") => bail!(
                "LANKA_EXTRACTD=require but LANKA_EXTRACTD_SOCKET is not set: start the tools container"
            ),
            _ => {
                WARN_LOCAL.call_once(|| {
                    tracing::warn!(
                        "document tools run as local subprocesses (no LANKA_EXTRACTD_SOCKET): development only"
                    );
                });
                Ok(Self::local())
            }
        }
    }

    #[must_use]
    pub fn local() -> Self {
        Self::Local {
            tools: Tools::discover(),
            limits: Limits::from_env(),
        }
    }

    #[must_use]
    pub fn socket(path: PathBuf) -> Self {
        Self::Socket {
            path,
            timeout: Duration::from_secs(600),
        }
    }

    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Socket { path, .. } => format!("lanka-extractd at {}", path.display()),
            Self::Local { .. } => "local subprocess (unsandboxed)".to_owned(),
        }
    }

    /// Runs one operation; the output bytes of the tool.
    pub async fn run(&self, op: &Op, input: &[u8]) -> Result<Vec<u8>> {
        op.validate()?;
        match self {
            Self::Local { tools, limits } => ops::run(op, input, limits, tools)
                .await
                .with_context(|| format!("sandbox op {}", op.name())),
            Self::Socket { path, timeout } => tokio::time::timeout(*timeout, call(path, op, input))
                .await
                .map_err(|_| anyhow!("lanka-extractd did not answer within {timeout:?}"))?
                .with_context(|| format!("sandbox op {} via {}", op.name(), path.display())),
        }
    }

    /// Page count.
    pub async fn page_count(&self, pdf: &[u8]) -> Result<u32> {
        let out = self.run(&Op::PdfInfo, pdf).await?;
        Ok(ops::parse_pdfinfo(&String::from_utf8_lossy(&out), None)?.0)
    }

    /// Text-layer words of pages `first..=last` (`0, 0` = all pages).
    pub async fn pdf_words(&self, pdf: &[u8], first: u32, last: u32) -> Result<Words> {
        let out = self.run(&Op::PdfWords { first, last }, pdf).await?;
        let text = String::from_utf8(out).context("pdftotext output is not UTF-8")?;
        parse_bbox_layout(&text, first.max(1))
    }

    /// PNG of one page.
    pub async fn render(&self, pdf: &[u8], page: u32, dpi: u32) -> Result<Vec<u8>> {
        self.run(&Op::PdfRender { page, dpi }, pdf).await
    }

    /// OCR words of one page (scanned documents).
    pub async fn ocr(&self, pdf: &[u8], page: u32, dpi: u32, langs: &[&str]) -> Result<Words> {
        let op = Op::Ocr {
            page,
            dpi,
            langs: langs.iter().map(|s| (*s).to_owned()).collect(),
        };
        let out = self.run(&op, pdf).await?;
        let text = String::from_utf8(out).context("hOCR is not UTF-8")?;
        parse_hocr(&text, page, dpi)
    }
}

async fn call(path: &Path, op: &Op, input: &[u8]) -> Result<Vec<u8>> {
    let mut s = UnixStream::connect(path)
        .await
        .with_context(|| format!("connecting to {}", path.display()))?;
    write_frame(&mut s, &serde_json::to_vec(op)?, input).await?;
    let (header, payload) = read_frame(&mut s, 1024 * 1024 * 1024).await?;
    let reply: Reply = serde_json::from_slice(&header).context("bad reply header")?;
    if reply.ok {
        Ok(payload)
    } else {
        bail!(
            "{}",
            reply.error.unwrap_or_else(|| "unknown error".to_owned())
        )
    }
}
