//! The sandbox's guarantees (ADR 0009, WP-08 acceptance):
//!
//! * `compose_tools_service_is_isolated` — the `tools` service in
//!   `infra/v2/docker-compose.yml` has no network, a read-only root, no
//!   capabilities and no published ports (static; always runs).
//! * `tools_container_has_no_network` — inside the built `tools` image run
//!   with the compose settings, DNS resolution fails. Needs Docker and the
//!   image (`docker build -f infra/v2/tools.Dockerfile -t lanka-tools:dev
//!   engine`, or `LANKA_TOOLS_IMAGE`); **skips with a printed reason**
//!   when either is unavailable.
//! * `malformed_corpus_fails_cleanly` — a corpus of malformed and hostile
//!   PDFs through every operation, locally under tight limits: each call
//!   returns (an error for the broken ones) within the wall-clock limit;
//!   nothing panics or hangs. Skips when poppler is not installed.
//! * `malformed_corpus_fails_cleanly_in_container` — the same corpus inside
//!   the `tools` image with the container limits (skips like the DNS test).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use lanka_extractd::{Limits, Op, Tools, ops};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn image() -> String {
    std::env::var("LANKA_TOOLS_IMAGE").unwrap_or_else(|_| "lanka-tools:dev".to_owned())
}

/// `Some(reason)` when the Docker tests cannot run here.
fn docker_unavailable() -> Option<String> {
    match Command::new("docker")
        .args(["info", "--format", "{{.ServerVersion}}"])
        .output()
    {
        Err(e) => return Some(format!("docker CLI not found ({e})")),
        Ok(o) if !o.status.success() => return Some("docker daemon not reachable".to_owned()),
        Ok(_) => {}
    }
    let img = image();
    match Command::new("docker")
        .args(["image", "inspect", &img])
        .output()
    {
        Ok(o) if o.status.success() => None,
        _ => Some(format!(
            "image {img} not built (docker build -f infra/v2/tools.Dockerfile -t {img} engine)"
        )),
    }
}

/// The isolation flags of the compose `tools` service, as `docker run` flags.
const RUN_FLAGS: &[&str] = &[
    "run",
    "--rm",
    "--network",
    "none",
    "--read-only",
    "--cap-drop",
    "ALL",
    "--security-opt",
    "no-new-privileges:true",
    "--pids-limit",
    "128",
    "--memory",
    "1536m",
    "--tmpfs",
    "/tmp:size=256m",
    "--user",
    "10001:10001",
];

#[test]
fn compose_tools_service_is_isolated() {
    let text = std::fs::read_to_string(repo_root().join("infra/v2/docker-compose.yml"))
        .expect("compose file");
    let doc: serde_yaml_ng::Value = serde_yaml_ng::from_str(&text).expect("yaml");
    let tools = &doc["services"]["tools"];
    assert_eq!(
        tools["network_mode"].as_str(),
        Some("none"),
        "tools must have no network"
    );
    assert_eq!(tools["read_only"].as_bool(), Some(true));
    assert_eq!(tools["cap_drop"][0].as_str(), Some("ALL"));
    assert!(tools["ports"].is_null(), "tools publishes no ports");
    // The process cap sits with the other limits; Compose rejects `pids_limit` beside `deploy` limits.
    let limits = &tools["deploy"]["resources"]["limits"];
    assert!(
        limits["pids"].as_u64().is_some(),
        "tools must cap its processes"
    );
    assert!(
        tools["pids_limit"].is_null(),
        "set the process cap in one place"
    );
    assert!(limits["memory"].as_str().is_some());
    let worker_env = &doc["services"]["worker"]["environment"];
    assert_eq!(worker_env["LANKA_EXTRACTD"].as_str(), Some("require"));
    assert!(worker_env["LANKA_EXTRACTD_SOCKET"].as_str().is_some());
    // The sidecar gets no credentials and no store.
    let env = serde_yaml_ng::to_string(&tools["environment"]).expect("env");
    assert!(!env.contains("DATABASE_URL") && !env.contains("PASSWORD"));
    let vols = serde_yaml_ng::to_string(&tools["volumes"]).expect("vols");
    assert!(
        !vols.contains("/store"),
        "tools must not see the blob store"
    );
}

#[test]
fn tools_container_has_no_network() {
    if let Some(why) = docker_unavailable() {
        eprintln!("SKIPPED tools_container_has_no_network: {why}");
        return;
    }
    let out = Command::new("docker")
        .args(RUN_FLAGS)
        .args([
            "--entrypoint",
            "getent",
            &image(),
            "hosts",
            "deb.debian.org",
        ])
        .output()
        .expect("docker run");
    assert!(
        !out.status.success() && out.stdout.is_empty(),
        "DNS resolved inside the tools container: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    // The daemon binary is there and the tools work without a network.
    let v = Command::new("docker")
        .args(RUN_FLAGS)
        .args([&image(), "versions"])
        .output()
        .expect("docker run");
    let s = String::from_utf8_lossy(&v.stdout);
    assert!(
        v.status.success() && s.contains("pdftotext") && s.contains("tesseract"),
        "{s}"
    );
}

// ───────────────────────────── corpus ──────────────────────────────────────

/// A one-page PDF with correct xref offsets.
fn pdf(media_box: &str, extra_page: &str, content: &str) -> Vec<u8> {
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [{media_box}] /Contents 4 0 R {extra_page} >>"
        ),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offs = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offs.push(out.len());
        out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let x = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offs {
        out.extend(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{x}\n%%EOF\n",
            objs.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// (name, bytes, must every op fail?)
fn corpus() -> Vec<(&'static str, Vec<u8>, bool)> {
    let good = pdf("0 0 612 792", "", "BT /F1 12 Tf 72 712 Td (ok) Tj ET");
    let mut random = Vec::with_capacity(64 * 1024);
    let mut s: u64 = 0x9e37_79b9_7f4a_7c15;
    for _ in 0..64 * 1024 {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        random.push((s >> 24) as u8);
    }
    let mut random_pdf = b"%PDF-1.7\n".to_vec();
    random_pdf.extend(&random);
    let nested = {
        let mut v = b"%PDF-1.4\n1 0 obj\n".to_vec();
        v.extend(std::iter::repeat_n(b'[', 200_000));
        v.extend(std::iter::repeat_n(b']', 200_000));
        v.extend(b"\nendobj\ntrailer\n<< /Root 1 0 R >>\n%%EOF\n");
        v
    };
    let mut lying_length = good.clone();
    let needle = b"/Length ";
    if let Some(i) = lying_length.windows(needle.len()).position(|w| w == needle) {
        lying_length.splice(
            i + needle.len()..i + needle.len() + 2,
            b"999999999".iter().copied(),
        );
    }
    vec![
        ("empty", Vec::new(), true),
        ("random-bytes", random, true),
        (
            "zip-not-pdf",
            b"PK\x03\x04\x14\x00\x00\x00 this is a zip".to_vec(),
            true,
        ),
        ("random-after-header", random_pdf, true),
        ("truncated", good[..good.len() / 2].to_vec(), false),
        ("deep-nesting", nested, true),
        ("lying-stream-length", lying_length, false),
        // 14400 pt square at 150 dpi = 900 Mpx: refused before rendering.
        (
            "huge-mediabox",
            pdf(
                "0 0 14400 14400",
                "/UserUnit 75000",
                "0 0 m 14400 14400 l S",
            ),
            false,
        ),
        (
            "page-tree-loop",
            {
                let mut v = pdf("0 0 612 792", "", "");
                let s = String::from_utf8_lossy(&v)
                    .replace("/Kids [3 0 R] /Count 1", "/Kids [2 0 R] /Count 1");
                v = s.into_bytes();
                v
            },
            false,
        ),
    ]
}

fn ops_for_corpus() -> Vec<Op> {
    vec![
        Op::PdfInfo,
        Op::PdfWords { first: 0, last: 0 },
        Op::PdfRender { page: 1, dpi: 150 },
    ]
}

#[tokio::test]
async fn malformed_corpus_fails_cleanly() {
    let tools = Tools::discover();
    if tools.pdftotext.is_none() || tools.pdfinfo.is_none() || tools.pdftoppm.is_none() {
        eprintln!(
            "SKIPPED malformed_corpus_fails_cleanly: poppler (pdfinfo/pdftotext/pdftoppm) not installed"
        );
        return;
    }
    let lim = Limits {
        wall_secs: 10,
        cpu_secs: 10,
        mem_mb: 512,
        fsize_mb: 64,
        max_input_mb: 8,
        max_output_mb: 16,
        max_render_mpx: 40,
    };
    // The well-formed control works.
    let good = pdf("0 0 612 792", "", "BT /F1 12 Tf 72 712 Td (ok) Tj ET");
    assert!(ops::run(&Op::PdfInfo, &good, &lim, &tools).await.is_ok());
    for (name, bytes, must_fail) in corpus() {
        for op in ops_for_corpus() {
            let t0 = Instant::now();
            let r = ops::run(&op, &bytes, &lim, &tools).await;
            let took = t0.elapsed();
            assert!(
                took < Duration::from_secs(lim.wall_secs * 3 + 5),
                "{name}/{}: {took:?} exceeds the limits",
                op.name()
            );
            if must_fail {
                assert!(r.is_err(), "{name}/{}: expected an error", op.name());
            }
            if name == "huge-mediabox" && matches!(op, Op::PdfRender { .. }) {
                let e = format!("{:#}", r.as_ref().expect_err("render cap"));
                assert!(e.contains("render cap"), "{e}");
            }
            eprintln!(
                "{name:>20} {:>10}: {} in {took:?}",
                op.name(),
                match &r {
                    Ok(o) => format!("ok ({} bytes)", o.len()),
                    Err(e) => format!(
                        "error: {}",
                        format!("{e:#}").chars().take(90).collect::<String>()
                    ),
                }
            );
        }
    }
    // Oversized input is refused before any tool runs.
    let big = vec![b'%'; 9 * 1024 * 1024];
    let e = ops::run(&Op::PdfInfo, &big, &lim, &tools)
        .await
        .expect_err("too big");
    assert!(format!("{e:#}").contains("exceeds"), "{e:#}");
}

#[test]
fn malformed_corpus_fails_cleanly_in_container() {
    if let Some(why) = docker_unavailable() {
        eprintln!("SKIPPED malformed_corpus_fails_cleanly_in_container: {why}");
        return;
    }
    let dir = std::env::temp_dir().join(format!("lanka-extractd-corpus-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    for (name, bytes, _) in corpus() {
        std::fs::write(dir.join(format!("{name}.pdf")), bytes).expect("write");
    }
    let mount = format!("{}:/in:ro", dir.display());
    for (name, _, must_fail) in corpus() {
        for op in ops_for_corpus() {
            let t0 = Instant::now();
            let out = Command::new("docker")
                .args(RUN_FLAGS)
                .args([
                    "-e",
                    "LANKA_EXTRACTD_WALL_SECS=10",
                    "-e",
                    "LANKA_EXTRACTD_MEM_MB=512",
                    "-v",
                    &mount,
                ])
                .arg(image())
                .args([
                    "run",
                    &serde_json::to_string(&op).expect("op"),
                    &format!("/in/{name}.pdf"),
                ])
                .output()
                .expect("docker run");
            let took = t0.elapsed();
            let err = String::from_utf8_lossy(&out.stderr);
            assert!(
                took < Duration::from_secs(60),
                "{name}/{}: {took:?}",
                op.name()
            );
            // A clean failure is an error message and exit status 1, never a
            // crash of the daemon process itself (signal / 101 panic).
            assert!(
                out.status.code().is_some_and(|c| c == 0 || c == 1),
                "{name}/{}: {:?} {err}",
                op.name(),
                out.status
            );
            if must_fail {
                assert_eq!(
                    out.status.code(),
                    Some(1),
                    "{name}/{}: expected an error",
                    op.name()
                );
                assert!(err.contains("error:"), "{err}");
            }
        }
    }
    let _ = std::fs::remove_dir_all(Path::new(&dir));
}
