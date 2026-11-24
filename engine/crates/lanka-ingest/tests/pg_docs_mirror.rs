//! `lanka docs mirror` against a real Postgres, a temporary bare git
//! repository and a stub `gh` (a shell script that records its calls and keeps
//! the "release assets" in a directory).
//!
//! Gated like `pg_docs.rs`: runs only when `LANKA_TEST_DATABASE_URL` points at
//! a server where the user may CREATE DATABASE.
//!
//! Covered: only complete months are selected; a corrupt blob stops the month
//! with nothing uploaded or committed; a good run creates the release,
//! uploads, verifies and commits `months/`, `metadata/` and the README
//! statistics; a second run does nothing.

use std::path::{Path, PathBuf};
use std::process::Command;

use bytes::Bytes;
use lanka_core::db::{self, PoolRole};
use lanka_ingest::docs::mirror::{self, MirrorOptions};
use lanka_store::{BlobStore, FsStore, Ns};
use sha2::{Digest, Sha256};
use sqlx::{Connection, Executor, PgConnection, PgPool};

const COLLECTION: &str = "lk-gazette-extraordinary";

const STUB: &str = r#"#!/bin/bash
S="__STATE__"
echo "$@" >> "$S/calls.log"
case "$1" in
  api)
    tag="${2##*/}"
    if [ ! -d "$S/rel/$tag" ]; then echo "gh: Not Found (HTTP 404)" >&2; exit 1; fi
    printf '{"assets":['
    first=1
    for f in "$S/rel/$tag"/*; do
      [ -f "$f" ] || continue
      n=$(basename "$f"); sz=$(wc -c < "$f" | tr -d ' ')
      [ $first = 1 ] || printf ','
      first=0
      printf '{"name":"%s","size":%s}' "$n" "$sz"
    done
    printf ']}\n'
    ;;
  release)
    case "$2" in
      create) mkdir -p "$S/rel/$3" ;;
      edit) ;;
      upload)
        tag="$3"; shift 3
        for a in "$@"; do [ -f "$a" ] && cp "$a" "$S/rel/$tag/"; done
        exit 0
        ;;
      download)
        tag="$3"; shift 3
        while [ $# -gt 0 ]; do
          if [ "$1" = "-p" ]; then name="$2"; shift; fi
          shift
        done
        cat "$S/rel/$tag/$name"
        ;;
    esac
    ;;
esac
exit 0
"#;

async fn with_database<F, Fut>(tag: &str, f: F)
where
    F: FnOnce(String) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let Ok(admin_url) = std::env::var("LANKA_TEST_DATABASE_URL") else {
        eprintln!("skipping: LANKA_TEST_DATABASE_URL not set");
        return;
    };
    let db = format!("lanka_it_{tag}_{}", std::process::id());
    let mut admin = PgConnection::connect(&admin_url)
        .await
        .expect("connect admin");
    let drop = format!("DROP DATABASE IF EXISTS {db} WITH (FORCE)");
    admin
        .execute(sqlx::raw_sql(sqlx::AssertSqlSafe(drop.clone())))
        .await
        .expect("drop");
    admin
        .execute(sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "CREATE DATABASE {db}"
        ))))
        .await
        .expect("create");
    let base = admin_url
        .rsplit_once('/')
        .map_or(admin_url.as_str(), |(b, _)| b);
    let result = tokio::spawn(f(format!("{base}/{db}"))).await;
    admin
        .execute(sqlx::raw_sql(sqlx::AssertSqlSafe(drop)))
        .await
        .expect("drop test database");
    if let Err(e) = result {
        std::panic::resume_unwind(e.into_panic());
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git");
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

async fn seed_doc(pool: &PgPool, doc_id: &str, number: &str, issued: &str) {
    sqlx::query(
        "INSERT INTO docs.documents (doc_id, collection, number, sub_number, issued_on, title,
                                    upstream_id, meta_hash)
         VALUES ($1, $2, $3, '01', $4::date, '{\"en\": \"A title\"}', $5, 'h')",
    )
    .bind(doc_id)
    .bind(COLLECTION)
    .bind(number)
    .bind(issued)
    .bind(format!("up-{doc_id}"))
    .execute(pool)
    .await
    .expect("document");
    sqlx::query(
        "INSERT INTO docs.document_revisions (doc_id, seen_at, meta_hash, record)
         VALUES ($1, now(), 'h', '{\"title\": \"A title\"}')",
    )
    .bind(doc_id)
    .execute(pool)
    .await
    .expect("revision");
}

async fn seed_file(pool: &PgPool, doc_id: &str, lang: &str, status: &str) {
    sqlx::query(
        "INSERT INTO docs.files (doc_id, lang, upstream_path, status) VALUES ($1, $2, $3, $4)",
    )
    .bind(doc_id)
    .bind(lang)
    .bind(format!("extra-gazette-content/{doc_id} ({lang}).pdf"))
    .bind(status)
    .execute(pool)
    .await
    .expect("file");
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

#[tokio::test(flavor = "multi_thread")]
async fn mirror_publishes_verifies_commits_and_is_idempotent() {
    with_database("mirror", run).await;
}

#[allow(clippy::too_many_lines)]
async fn run(url: String) {
    let pool = db::connect(&url, PoolRole::Ingest).await.expect("pool");
    db::migrate(&pool).await.expect("migrate");

    let root: PathBuf =
        std::env::temp_dir().join(format!("lanka-mirror-it-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let state = root.join("gh-state");
    std::fs::create_dir_all(state.join("rel")).expect("state dir");
    let store = FsStore::new(root.join("store"));

    // ── a month of three documents: two with PDFs and text, one with a missing file;
    //    and a later month that still has a pending file ──
    let pdf_a = Bytes::from_static(b"%PDF-1.4 first gazette, english");
    let pdf_b = Bytes::from_static(b"%PDF-1.4 first gazette, sinhala");
    let pdf_c = Bytes::from_static(b"%PDF-1.4 second gazette, english");
    let text_a = Bytes::from_static("page one\u{c}page two".as_bytes());
    seed_doc(&pool, "gze-2000-01", "2000", "2026-09-03").await;
    seed_doc(&pool, "gze-2001-01", "2001", "2026-09-20").await;
    seed_doc(&pool, "gze-2002-01", "2002", "2026-10-02").await;
    // a month whose documents have no file rows is not published
    seed_doc(&pool, "gze-1999-01", "1999", "2026-08-05").await;
    let mut blobs = Vec::new();
    for (doc, lang, data, text) in [
        ("gze-2000-01", "en", &pdf_a, Some(&text_a)),
        ("gze-2000-01", "si", &pdf_b, None),
        ("gze-2001-01", "en", &pdf_c, None),
    ] {
        seed_file(&pool, doc, lang, "pending").await;
        let (b, _) = store.put_cas(Ns::Raw, data.clone()).await.expect("raw");
        let (tsha, tstatus) = match text {
            Some(t) => (
                Some(
                    store
                        .put_cas(Ns::Text, t.clone())
                        .await
                        .expect("text")
                        .0
                        .hex(),
                ),
                "text",
            ),
            None => (None, "none"),
        };
        sqlx::query(
            "UPDATE docs.files SET status = 'ok', sha256 = $3, bytes = $4, pages = 2,
                    text_status = $5, text_sha256 = $6 WHERE doc_id = $1 AND lang = $2",
        )
        .bind(doc)
        .bind(lang)
        .bind(b.hex())
        .bind(i64::try_from(data.len()).expect("len"))
        .bind(tstatus)
        .bind(tsha)
        .execute(&pool)
        .await
        .expect("mark ok");
        blobs.push((doc, lang, b));
    }
    seed_file(&pool, "gze-2001-01", "ta", "missing").await;
    seed_file(&pool, "gze-2002-01", "en", "pending").await;

    // ── the repository: a bare remote and a working clone with the README markers ──
    let bare = root.join("remote.git");
    let work = root.join("archive");
    std::fs::create_dir_all(&work).expect("work dir");
    let o = Command::new("git")
        .args(["init", "--bare", "-b", "main"])
        .arg(&bare)
        .output()
        .expect("git init --bare");
    assert!(o.status.success());
    git(&work, &["init", "-b", "main"]);
    git(&work, &["config", "user.email", "archive@example.org"]);
    git(&work, &["config", "user.name", "Archive Test"]);
    git(&work, &["config", "commit.gpgsign", "false"]);
    git(
        &work,
        &["remote", "add", "origin", bare.to_str().expect("path")],
    );
    std::fs::write(
        work.join("README.md"),
        "# archive\n\nprose\n\n<!-- stats:start -->\n*Not yet mirrored.*\n<!-- stats:end -->\n",
    )
    .expect("readme");
    git(&work, &["add", "README.md"]);
    git(&work, &["commit", "-m", "docs: readme"]);
    git(&work, &["push", "-u", "origin", "main"]);

    let gh = root.join("gh");
    std::fs::write(
        &gh,
        STUB.replace("__STATE__", state.to_str().expect("path")),
    )
    .expect("stub");
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    let mut opts = MirrorOptions::new(&work);
    opts.gh_bin = gh.to_str().expect("path").to_owned();
    opts.work_dir = Some(root.join("work"));
    std::fs::create_dir_all(root.join("work")).expect("work");
    let calls = || std::fs::read_to_string(state.join("calls.log")).unwrap_or_default();
    let remote_commits = || {
        git(&bare, &["rev-list", "--count", "main"])
            .trim()
            .to_owned()
    };

    // ── an explicit month that is not complete is refused ──
    let mut o2 = opts.clone();
    o2.months = vec!["2026-10".into()];
    let err = mirror::run(&pool, &store, COLLECTION, &o2)
        .await
        .expect_err("incomplete month");
    assert!(format!("{err:#}").contains("not complete"), "{err:#}");
    // without --month it is skipped and reported
    // ── a corrupt blob stops the month; nothing is uploaded or committed ──
    let corrupt = store.path(Ns::Raw, &blobs[0].2.key).expect("blob path");
    let original = std::fs::read(&corrupt).expect("blob");
    std::fs::remove_file(&corrupt).expect("rm");
    std::fs::write(&corrupt, b"damaged").expect("write");
    let err = mirror::run(&pool, &store, COLLECTION, &opts)
        .await
        .expect_err("corrupt blob");
    let msg = format!("{err:#}");
    assert!(msg.contains("gze-2000-01"), "{msg}");
    assert!(msg.contains("nothing was published"), "{msg}");
    assert!(!calls().contains("upload"), "{}", calls());
    assert_eq!(remote_commits(), "1");
    std::fs::remove_file(&corrupt).expect("rm");
    std::fs::write(&corrupt, &original).expect("restore");

    // ── dry run: packs are built, nothing is uploaded or committed ──
    let mut dry = opts.clone();
    dry.dry_run = true;
    let rep = mirror::run(&pool, &store, COLLECTION, &dry)
        .await
        .expect("dry run");
    assert_eq!(rep.selected, ["2026-09"]);
    assert_eq!(rep.incomplete, ["2026-10"]);
    assert_eq!(rep.empty, ["2026-08"]);
    assert!(!rep.committed && !rep.months[0].published);
    let names: Vec<_> = rep.months[0]
        .assets
        .iter()
        .map(|a| a.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "gazettes-2026-09.tar",
            "text-2026-09.tar.gz",
            "manifest-2026-09.jsonl"
        ]
    );
    let dir = rep.work_dir.clone().expect("kept for a dry run");
    assert!(dir.join("2026-09/gazettes-2026-09.tar").exists());
    assert!(!calls().contains("release"), "{}", calls());
    assert_eq!(remote_commits(), "1");
    let _ = std::fs::remove_dir_all(&dir);

    // ── the real run ──
    let rep = mirror::run(&pool, &store, COLLECTION, &opts)
        .await
        .expect("mirror");
    assert!(rep.committed, "{rep:?}");
    assert_eq!(rep.selected, ["2026-09"]);
    let log = calls();
    assert!(log.contains("release create 2026-09"), "{log}");
    assert!(log.contains("--clobber"), "{log}");
    assert!(log.contains("release download 2026-09"), "{log}");
    assert_eq!(remote_commits(), "2");
    assert_eq!(
        git(&bare, &["log", "-1", "--format=%B", "main"]).trim(),
        "data: mirror 2026-09"
    );
    let files = git(&bare, &["ls-tree", "-r", "--name-only", "main"]);
    for f in [
        "months/2026-09.json",
        "metadata/documents/2026.jsonl",
        "metadata/revisions/2026.jsonl",
        "README.md",
    ] {
        assert!(files.lines().any(|l| l == f), "{f} not in {files}");
    }
    // the release holds the three assets, and they extract to the archived bytes
    let rel = state.join("rel/2026-09");
    let tar_bytes = std::fs::read(rel.join("gazettes-2026-09.tar")).expect("tar");
    let mut ar = tar::Archive::new(tar_bytes.as_slice());
    let mut entries = Vec::new();
    for e in ar.entries().expect("entries") {
        let mut e = e.expect("entry");
        let name = e.path().expect("path").to_string_lossy().into_owned();
        let mut data = Vec::new();
        std::io::Read::read_to_end(&mut e, &mut data).expect("read");
        entries.push((name, hex::encode(Sha256::digest(&data))));
    }
    assert_eq!(
        entries.iter().map(|e| e.0.as_str()).collect::<Vec<_>>(),
        [
            "gze-2000-01_en.pdf",
            "gze-2000-01_si.pdf",
            "gze-2001-01_en.pdf"
        ]
    );
    // metadata: sorted by doc_id, compact, with the files and their source URLs
    let docs = read(&work.join("metadata/documents/2026.jsonl"));
    let ids: Vec<String> = docs
        .lines()
        .map(|l| {
            serde_json::from_str::<serde_json::Value>(l).expect("json")["doc_id"]
                .as_str()
                .expect("id")
                .to_owned()
        })
        .collect();
    assert_eq!(
        ids,
        ["gze-1999-01", "gze-2000-01", "gze-2001-01", "gze-2002-01"]
    );
    let first: serde_json::Value =
        serde_json::from_str(docs.lines().nth(1).expect("line")).expect("json");
    let en = first["files"]
        .as_array()
        .expect("files")
        .iter()
        .find(|f| f["lang"] == "en")
        .expect("en");
    assert_eq!(en["sha256"], entries[0].1);
    assert!(
        en["source_url"]
            .as_str()
            .expect("url")
            .ends_with("file=extra-gazette-content%2Fgze-2000-01%20%28en%29.pdf")
    );
    assert_eq!(
        read(&work.join("metadata/revisions/2026.jsonl"))
            .lines()
            .count(),
        4
    );
    // the month file
    let month: serde_json::Value =
        serde_json::from_str(&read(&work.join("months/2026-09.json"))).expect("month json");
    assert_eq!(month["complete"], true);
    assert_eq!(month["documents"], 2);
    assert_eq!(month["files"]["ok"], 3);
    assert_eq!(month["files"]["missing"], 1);
    assert_eq!(month["assets"].as_array().expect("assets").len(), 3);
    let manifest = std::fs::read(rel.join("manifest-2026-09.jsonl")).expect("manifest");
    assert_eq!(
        month["manifest_sha256"],
        hex::encode(Sha256::digest(&manifest))
    );
    // the README keeps its prose and has the statistics
    let readme = read(&work.join("README.md"));
    assert!(readme.starts_with("# archive\n\nprose\n\n<!-- stats:start -->\n"));
    assert!(
        readme.contains("| 2026 | 4 | 3 | 1 | 0 | 1 | 1 |"),
        "{readme}"
    );
    assert!(!readme.contains("Not yet mirrored"));
    // no attribution in the commit
    let body = git(&bare, &["log", "-1", "--format=%B%an%ae", "main"]);
    assert!(!body.to_lowercase().contains("co-authored"), "{body}");

    // ── a second run has nothing to do ──
    let calls_before = calls().lines().count();
    let rep = mirror::run(&pool, &store, COLLECTION, &opts)
        .await
        .expect("second run");
    assert!(rep.selected.is_empty() && !rep.committed, "{rep:?}");
    assert_eq!(rep.unchanged, ["2026-09"]);
    assert!(rep.message.contains("nothing to mirror"), "{}", rep.message);
    assert_eq!(calls().lines().count(), calls_before);
    assert_eq!(remote_commits(), "2");

    // ── finishing the other month makes it selectable; the first stays untouched ──
    let (b, _) = store
        .put_cas(Ns::Raw, Bytes::from_static(b"%PDF-1.4 third"))
        .await
        .expect("raw");
    sqlx::query(
        "UPDATE docs.files SET status = 'ok', sha256 = $1, bytes = 14, pages = 1, text_status = 'none'
          WHERE doc_id = 'gze-2002-01'",
    )
    .bind(b.hex())
    .execute(&pool)
    .await
    .expect("complete month");
    let rep = mirror::run(&pool, &store, COLLECTION, &opts)
        .await
        .expect("third run");
    // 2026-09's manifest is unchanged: it does not mention the other month
    assert_eq!(rep.selected, ["2026-10"]);
    assert_eq!(rep.unchanged, ["2026-09"]);
    assert_eq!(remote_commits(), "3");
    assert_eq!(
        git(&bare, &["log", "-1", "--format=%s", "main"]).trim(),
        "data: mirror 2026-10"
    );
    let readme = read(&work.join("README.md"));
    assert!(
        readme.contains("| 2026 | 4 | 4 | 1 | 0 | 0 | 2 |"),
        "{readme}"
    );

    let _ = std::fs::remove_dir_all(&root);
}
