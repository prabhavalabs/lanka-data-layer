//! The operations the sandbox performs, each a fixed argv (ADR 0009).
//!
//! Nothing a caller sends reaches a command line except integers that are
//! range-checked here and language codes from a three-entry allow-list.
//! The input is always written to `input.pdf` in a fresh work directory and
//! outputs are read back from fixed file names in it, so a document can
//! neither name files nor inject flags.
//!
//! Every tool runs under [`Limits`]: wall-clock timeout (the process is
//! killed), CPU seconds, address space, largest file written (`ulimit`),
//! plus input/output size caps and a pixel cap checked *before* rendering.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail, ensure};
use serde::{Deserialize, Serialize};

/// Languages tesseract may be asked for (`eng+sin+tam`, 03 §3.1).
pub const OCR_LANGS: &[&str] = &["eng", "sin", "tam"];

/// One sandboxed operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    /// `pdfinfo` text.
    PdfInfo,
    /// `pdftotext -bbox-layout` XHTML for pages `first..=last` (1-based;
    /// `0, 0` = every page).
    PdfWords { first: u32, last: u32 },
    /// `pdftoppm -png` of one page.
    PdfRender { page: u32, dpi: u32 },
    /// `tesseract … hocr` of one page rendered at `dpi`.
    Ocr {
        page: u32,
        dpi: u32,
        langs: Vec<String>,
    },
    /// Tool versions (health check; takes no input).
    Versions,
}

impl Op {
    /// Rejects anything outside the allowed ranges.
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::PdfInfo | Self::Versions => Ok(()),
            Self::PdfWords { first, last } => {
                ensure!(
                    (*first == 0 && *last == 0) || (*first >= 1 && last >= first),
                    "bad page range {first}..{last}"
                );
                ensure!(*last <= 100_000, "page {last} out of range");
                Ok(())
            }
            Self::PdfRender { page, dpi } => {
                ensure!((1..=100_000).contains(page), "page {page} out of range");
                ensure!((36..=300).contains(dpi), "dpi {dpi} out of range 36..300");
                Ok(())
            }
            Self::Ocr { page, dpi, langs } => {
                ensure!((1..=100_000).contains(page), "page {page} out of range");
                ensure!((72..=400).contains(dpi), "dpi {dpi} out of range 72..400");
                ensure!(!langs.is_empty() && langs.len() <= 3, "1–3 OCR languages");
                for l in langs {
                    ensure!(
                        OCR_LANGS.contains(&l.as_str()),
                        "OCR language {l:?} not allowed"
                    );
                }
                Ok(())
            }
        }
    }

    #[must_use]
    pub const fn needs_input(&self) -> bool {
        !matches!(self, Self::Versions)
    }

    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::PdfInfo => "pdf_info",
            Self::PdfWords { .. } => "pdf_words",
            Self::PdfRender { .. } => "pdf_render",
            Self::Ocr { .. } => "ocr",
            Self::Versions => "versions",
        }
    }
}

/// Resource limits for one operation. The server's limits are fixed by its
/// own configuration; a client cannot raise them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    /// Wall clock per tool invocation; the process is killed after it.
    pub wall_secs: u64,
    /// `ulimit -t`.
    pub cpu_secs: u64,
    /// `ulimit -v` (address space). Not enforced by macOS kernels; the
    /// container's memory limit backs it up on Linux.
    pub mem_mb: u64,
    /// `ulimit -f`: largest file a tool may write.
    pub fsize_mb: u64,
    /// Largest accepted input document.
    pub max_input_mb: u64,
    /// Largest output returned to the caller.
    pub max_output_mb: u64,
    /// Largest page render, in megapixels (checked from `pdfinfo` first).
    pub max_render_mpx: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            wall_secs: 60,
            cpu_secs: 60,
            mem_mb: 1024,
            fsize_mb: 256,
            max_input_mb: 200,
            max_output_mb: 64,
            max_render_mpx: 40,
        }
    }
}

impl Limits {
    /// Defaults overridden by `LANKA_EXTRACTD_{WALL_SECS,CPU_SECS,MEM_MB,…}`.
    #[must_use]
    pub fn from_env() -> Self {
        let mut l = Self::default();
        let get = |k: &str, d: u64| {
            std::env::var(format!("LANKA_EXTRACTD_{k}"))
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|v| *v > 0)
                .unwrap_or(d)
        };
        l.wall_secs = get("WALL_SECS", l.wall_secs);
        l.cpu_secs = get("CPU_SECS", l.cpu_secs);
        l.mem_mb = get("MEM_MB", l.mem_mb);
        l.fsize_mb = get("FSIZE_MB", l.fsize_mb);
        l.max_input_mb = get("MAX_INPUT_MB", l.max_input_mb);
        l.max_output_mb = get("MAX_OUTPUT_MB", l.max_output_mb);
        l.max_render_mpx = get("MAX_RENDER_MPX", l.max_render_mpx);
        l
    }
}

/// Absolute paths of the tools (the child environment is cleared, so
/// nothing is looked up on a caller-controlled `PATH`).
#[derive(Debug, Clone)]
pub struct Tools {
    pub pdfinfo: Option<PathBuf>,
    pub pdftotext: Option<PathBuf>,
    pub pdftoppm: Option<PathBuf>,
    pub tesseract: Option<PathBuf>,
    /// `TESSDATA_PREFIX` passed through to tesseract, if set.
    pub tessdata: Option<PathBuf>,
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

impl Tools {
    /// Finds the tools on `PATH` (or `LANKA_POPPLER_BIN` / `LANKA_TESSERACT`).
    #[must_use]
    pub fn discover() -> Self {
        let poppler = std::env::var_os("LANKA_POPPLER_BIN").map(PathBuf::from);
        let find = |n: &str| match &poppler {
            Some(d) => Some(d.join(n)).filter(|p| p.is_file()),
            None => which(n),
        };
        Self {
            pdfinfo: find("pdfinfo"),
            pdftotext: find("pdftotext"),
            pdftoppm: find("pdftoppm"),
            tesseract: std::env::var_os("LANKA_TESSERACT")
                .map(PathBuf::from)
                .filter(|p| p.is_file())
                .or_else(|| which("tesseract")),
            tessdata: std::env::var_os("TESSDATA_PREFIX").map(PathBuf::from),
        }
    }

    fn get<'a>(p: &'a Option<PathBuf>, name: &str) -> Result<&'a Path> {
        p.as_deref()
            .with_context(|| format!("{name} is not installed in the sandbox"))
    }
}

static SEQ: AtomicU64 = AtomicU64::new(0);

/// A fresh private work directory, removed on drop.
struct WorkDir(PathBuf);

impl WorkDir {
    fn new() -> Result<Self> {
        let base =
            std::env::var_os("LANKA_EXTRACTD_WORK").map_or_else(std::env::temp_dir, PathBuf::from);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let p = base.join(format!(
            "lanka-extractd-{}-{}-{nanos}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).with_context(|| format!("creating {}", p.display()))?;
        Ok(Self(p))
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// What one tool invocation produced.
struct ToolOut {
    stdout: Vec<u8>,
}

/// Runs `bin args…` in `work` under the limits: environment cleared,
/// stdin closed, `ulimit -t/-f/-v` applied by a fixed `/bin/sh` prologue,
/// killed at the wall-clock limit.
async fn run_tool(
    bin: &Path,
    args: &[String],
    work: &Path,
    lim: &Limits,
    tools: &Tools,
) -> Result<ToolOut> {
    let name = bin
        .file_name()
        .map_or_else(|| "tool".to_owned(), |n| n.to_string_lossy().into_owned());
    // POSIX sh: -f counts 512-byte blocks, -v KiB. `ulimit -v` is refused on
    // macOS; the container memory limit is the backstop there.
    let prologue = "ulimit -t \"$1\" && ulimit -f \"$2\" || exit 125; ulimit -v \"$3\" 2>/dev/null; shift 3; exec \"$@\"";
    let mut cmd = tokio::process::Command::new("/bin/sh");
    cmd.arg("-c")
        .arg(prologue)
        .arg("lanka-tool")
        .arg(lim.cpu_secs.to_string())
        .arg((lim.fsize_mb * 2048).to_string())
        .arg((lim.mem_mb * 1024).to_string())
        .arg(bin)
        .args(args)
        .current_dir(work)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", work)
        .env("OMP_THREAD_LIMIT", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(t) = &tools.tessdata {
        cmd.env("TESSDATA_PREFIX", t);
    }
    let child = cmd.spawn().with_context(|| format!("starting {name}"))?;
    let out =
        match tokio::time::timeout(Duration::from_secs(lim.wall_secs), child.wait_with_output())
            .await
        {
            Ok(r) => r.with_context(|| format!("waiting for {name}"))?,
            Err(_) => bail!("{name} timed out after {}s (killed)", lim.wall_secs),
        };
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let tail: String = stderr
            .lines()
            .filter(|l| !l.contains("Syntax Warning"))
            .take(6)
            .collect::<Vec<_>>()
            .join(" | ");
        let why = match out.status.code() {
            Some(125) => "could not apply resource limits".to_owned(),
            Some(c) => format!("exit status {c}"),
            None => {
                #[cfg(unix)]
                {
                    use std::os::unix::process::ExitStatusExt;
                    format!(
                        "killed by signal {} (crash or resource limit)",
                        out.status.signal().unwrap_or(0)
                    )
                }
                #[cfg(not(unix))]
                {
                    "killed".to_owned()
                }
            }
        };
        bail!(
            "{name} failed: {why}: {}",
            tail.chars().take(400).collect::<String>()
        );
    }
    Ok(ToolOut { stdout: out.stdout })
}

fn read_capped(p: &Path, lim: &Limits) -> Result<Vec<u8>> {
    let meta = std::fs::metadata(p).with_context(|| format!("tool wrote no {}", p.display()))?;
    ensure!(
        meta.len() <= lim.max_output_mb * 1024 * 1024,
        "output {} bytes exceeds {} MB",
        meta.len(),
        lim.max_output_mb
    );
    Ok(std::fs::read(p)?)
}

/// `%PDF-` within the first KiB (what poppler itself requires).
fn looks_like_pdf(b: &[u8]) -> bool {
    b.len() >= 8 && b[..b.len().min(1024)].windows(5).any(|w| w == b"%PDF-")
}

/// Page count and the size (points) of `page` from `pdfinfo` output.
pub fn parse_pdfinfo(text: &str, page: Option<u32>) -> Result<(u32, Option<(f64, f64)>)> {
    let mut pages = None;
    let mut size = None;
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("Pages:") {
            pages = v.trim().parse::<u32>().ok();
        }
        if let Some(p) = page {
            // "Page    3 size: 595.5 x 842.25 pts (A4)" (with -f/-l) or "Page size: …"
            let l = line.trim_start();
            let tagged = l.starts_with(&format!("Page {p:>4} size:"))
                || l.starts_with(&format!("Page {p} size:"));
            if (tagged || l.starts_with("Page size:"))
                && let Some(rest) = l.split("size:").nth(1)
            {
                let mut it = rest.split_whitespace();
                let w = it.next().and_then(|t| t.parse::<f64>().ok());
                let _x = it.next();
                let h = it.next().and_then(|t| t.parse::<f64>().ok());
                if let (Some(w), Some(h)) = (w, h)
                    && (tagged || size.is_none())
                {
                    size = Some((w, h));
                }
            }
        }
    }
    let pages = pages.context("pdfinfo reported no page count (not a readable PDF)")?;
    Ok((pages, size))
}

/// Runs one operation locally under the limits. Shared by the daemon and
/// by the in-process fallback used in development.
pub async fn run(op: &Op, input: &[u8], lim: &Limits, tools: &Tools) -> Result<Vec<u8>> {
    op.validate()?;
    if let Op::Versions = op {
        return versions(tools).await;
    }
    ensure!(!input.is_empty(), "empty input");
    ensure!(
        input.len() as u64 <= lim.max_input_mb * 1024 * 1024,
        "input {} bytes exceeds {} MB",
        input.len(),
        lim.max_input_mb
    );
    ensure!(
        looks_like_pdf(input),
        "input is not a PDF (no %PDF- header)"
    );
    let work = WorkDir::new()?;
    let pdf = work.0.join("input.pdf");
    std::fs::write(&pdf, input)?;
    let a = |s: &str| s.to_owned();
    match op {
        Op::PdfInfo => {
            let out = run_tool(
                Tools::get(&tools.pdfinfo, "pdfinfo")?,
                &[a("input.pdf")],
                &work.0,
                lim,
                tools,
            )
            .await?;
            parse_pdfinfo(&String::from_utf8_lossy(&out.stdout), None)?;
            Ok(out.stdout)
        }
        Op::PdfWords { first, last } => {
            let mut args = vec![a("-bbox-layout"), a("-q")];
            if *first > 0 {
                args.extend([a("-f"), first.to_string(), a("-l"), last.to_string()]);
            }
            args.extend([a("input.pdf"), a("words.html")]);
            run_tool(
                Tools::get(&tools.pdftotext, "pdftotext")?,
                &args,
                &work.0,
                lim,
                tools,
            )
            .await?;
            read_capped(&work.0.join("words.html"), lim)
        }
        Op::PdfRender { page, dpi } => {
            render(&work.0, *page, *dpi, lim, tools).await?;
            read_capped(&work.0.join("page.png"), lim)
        }
        Op::Ocr { page, dpi, langs } => {
            render(&work.0, *page, *dpi, lim, tools).await?;
            let args = vec![
                a("page.png"),
                a("ocr"),
                a("-l"),
                langs.join("+"),
                a("--dpi"),
                dpi.to_string(),
                a("hocr"),
            ];
            run_tool(
                Tools::get(&tools.tesseract, "tesseract")?,
                &args,
                &work.0,
                lim,
                tools,
            )
            .await?;
            read_capped(&work.0.join("ocr.hocr"), lim)
        }
        Op::Versions => Err(anyhow!("unreachable")),
    }
}

/// Checks the page exists and its render fits the pixel cap, then renders.
async fn render(work: &Path, page: u32, dpi: u32, lim: &Limits, tools: &Tools) -> Result<()> {
    let p = page.to_string();
    let info = run_tool(
        Tools::get(&tools.pdfinfo, "pdfinfo")?,
        &[
            "-f".into(),
            p.clone(),
            "-l".into(),
            p.clone(),
            "input.pdf".into(),
        ],
        work,
        lim,
        tools,
    )
    .await?;
    let (pages, size) = parse_pdfinfo(&String::from_utf8_lossy(&info.stdout), Some(page))?;
    ensure!(page <= pages, "page {page} > page count {pages}");
    let (w, h) = size.context("pdfinfo reported no page size")?;
    let px = (w / 72.0 * f64::from(dpi)) * (h / 72.0 * f64::from(dpi));
    #[allow(clippy::cast_precision_loss)]
    let cap = (lim.max_render_mpx * 1_000_000) as f64;
    ensure!(
        w > 0.0 && h > 0.0 && px <= cap,
        "page {page} is {w:.0}x{h:.0} pt: {:.0} Mpx at {dpi} dpi exceeds the {} Mpx render cap",
        px / 1e6,
        lim.max_render_mpx
    );
    run_tool(
        Tools::get(&tools.pdftoppm, "pdftoppm")?,
        &[
            "-r".into(),
            dpi.to_string(),
            "-f".into(),
            p.clone(),
            "-l".into(),
            p,
            "-png".into(),
            "-singlefile".into(),
            "input.pdf".into(),
            "page".into(),
        ],
        work,
        lim,
        tools,
    )
    .await?;
    Ok(())
}

async fn versions(tools: &Tools) -> Result<Vec<u8>> {
    let mut lines = Vec::new();
    let lim = Limits {
        wall_secs: 10,
        ..Limits::default()
    };
    let work = WorkDir::new()?;
    for (name, bin, flag) in [
        ("pdftotext", &tools.pdftotext, "-v"),
        ("pdftoppm", &tools.pdftoppm, "-v"),
        ("tesseract", &tools.tesseract, "--version"),
    ] {
        let v = match bin {
            None => "missing".to_owned(),
            Some(b) => {
                // poppler prints its version on stderr with exit 0/99; ask via sh.
                let out = tokio::process::Command::new(b)
                    .arg(flag)
                    .env_clear()
                    .current_dir(&work.0)
                    .stdin(Stdio::null())
                    .output();
                match tokio::time::timeout(Duration::from_secs(lim.wall_secs), out).await {
                    Ok(Ok(o)) => {
                        let s = if o.stdout.is_empty() {
                            o.stderr
                        } else {
                            o.stdout
                        };
                        String::from_utf8_lossy(&s)
                            .lines()
                            .next()
                            .unwrap_or("")
                            .trim()
                            .to_owned()
                    }
                    _ => "unavailable".to_owned(),
                }
            }
        };
        lines.push(format!("{name}: {v}"));
    }
    Ok(lines.join("\n").into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ops_are_range_checked() {
        assert!(Op::PdfRender { page: 0, dpi: 150 }.validate().is_err());
        assert!(Op::PdfRender { page: 1, dpi: 2000 }.validate().is_err());
        assert!(Op::PdfWords { first: 3, last: 2 }.validate().is_err());
        assert!(Op::PdfWords { first: 0, last: 0 }.validate().is_ok());
        let bad = Op::Ocr {
            page: 1,
            dpi: 300,
            langs: vec!["eng; rm -rf /".into()],
        };
        assert!(bad.validate().is_err());
        let json = serde_json::to_string(&Op::PdfWords { first: 1, last: 2 }).expect("json");
        assert_eq!(json, r#"{"op":"pdf_words","first":1,"last":2}"#);
    }

    #[test]
    fn pdfinfo_page_sizes() {
        let t = "Producer: x\nPages:           34\nPage    4 size: 595.5 x 842.25 pts (A4)\n";
        assert_eq!(
            parse_pdfinfo(t, Some(4)).expect("parse"),
            (34, Some((595.5, 842.25)))
        );
        let t = "Pages: 1\nPage size:       841.89 x 595.276 pts (A4)\n";
        assert_eq!(
            parse_pdfinfo(t, Some(1)).expect("parse").1,
            Some((841.89, 595.276))
        );
        assert!(parse_pdfinfo("nothing", None).is_err());
    }

    #[test]
    fn non_pdf_input_is_refused_before_any_tool_runs() {
        assert!(!looks_like_pdf(b"PK\x03\x04 zip"));
        assert!(looks_like_pdf(b"\n\n%PDF-1.7\n"));
    }
}
