//! The blob store (01-layers.md, ADR 0002): every file the platform keeps —
//! raw artifacts (L0), extracts (L1), grids, tiles, distributions, release
//! files, evidence sheets — goes through [`BlobStore`]. The only backend
//! today is [`FsStore`] (a directory on local NVMe); an S3/Garage backend
//! implements the same trait when a switch trigger fires (WP-23).
//!
//! Layout under the root (kept from the foundation's raw archive):
//!
//! ```text
//! raw/ab/cd/<sha256>                 L0, content-addressed
//! extract/ab/cd/<sha256>.parquet     L1, content-addressed
//! grids/… tiles/… datasets/… releases/… evidence/…   named, immutable
//! tmp/                               same filesystem → rename is atomic
//! ```
//!
//! Writes are temp file + fsync + rename, so a crash never leaves a
//! truncated object under a valid name. Content-addressed writes are
//! idempotent; named writes refuse to overwrite different content.

use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, bail, ensure};
use async_trait::async_trait;
use bytes::Bytes;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

/// Namespaces (top-level directories) of the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Ns {
    Raw,
    Extract,
    Grid,
    Tile,
    Dataset,
    Release,
    Evidence,
    /// Analysis results and outputs, `analysis/<an_id>/…` (WP-29; served
    /// through the application, never under `/dl`).
    Analysis,
    Tmp,
}

impl Ns {
    #[must_use]
    pub const fn dir(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Extract => "extract",
            Self::Grid => "grids",
            Self::Tile => "tiles",
            Self::Dataset => "datasets",
            Self::Release => "releases",
            Self::Evidence => "evidence",
            Self::Analysis => "analysis",
            Self::Tmp => "tmp",
        }
    }

    /// Namespaces whose keys are derived from the content hash.
    #[must_use]
    pub const fn is_cas(self) -> bool {
        matches!(self, Self::Raw | Self::Extract)
    }

    const fn cas_suffix(self) -> &'static str {
        match self {
            Self::Extract => ".parquet",
            _ => "",
        }
    }

    /// Key of a content-addressed object: `ab/cd/<sha256>[.parquet]`.
    #[must_use]
    pub fn cas_key(self, sha256: &[u8; 32]) -> String {
        let h = hex::encode(sha256);
        format!("{}/{}/{h}{}", &h[0..2], &h[2..4], self.cas_suffix())
    }
}

/// A stored object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobRef {
    pub ns: Ns,
    /// Key relative to the namespace directory.
    pub key: String,
    pub sha256: [u8; 32],
    pub bytes: u64,
}

impl BlobRef {
    #[must_use]
    pub fn hex(&self) -> String {
        hex::encode(self.sha256)
    }

    /// Path relative to the store root (`raw/ab/cd/<sha>`), as recorded in
    /// `ops.artifacts.archive_path`.
    #[must_use]
    pub fn rel_path(&self) -> String {
        format!("{}/{}", self.ns.dir(), self.key)
    }
}

pub type BoxAsyncRead = Pin<Box<dyn AsyncRead + Send + Unpin>>;

// async_trait marks the boxed futures #[must_use]; clippy then flags that.
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait BlobStore: Send + Sync {
    /// Content-addressed write (Raw, Extract). Idempotent; `created = false`
    /// when the object was already present.
    async fn put_cas(&self, ns: Ns, bytes: Bytes) -> Result<(BlobRef, bool)>;
    /// Content-addressed write of a local file (streamed: large files are
    /// never held in memory). The source file is left in place.
    async fn put_cas_file(&self, ns: Ns, src: &Path) -> Result<(BlobRef, bool)>;
    /// Content-addressed write of a scratch file from [`BlobStore::tmp_file`]
    /// whose sha256 and length the caller computed while writing it (a
    /// streamed download). The file is moved, not copied.
    async fn adopt_tmp(
        &self,
        ns: Ns,
        tmp: &Path,
        sha256: [u8; 32],
        bytes: u64,
    ) -> Result<(BlobRef, bool)>;
    /// Named immutable write (Dataset, Release, Grid, Tile). Fails if the key
    /// exists with other content.
    async fn put_named(&self, ns: Ns, key: &str, src: &Path) -> Result<BlobRef>;
    /// Reads a content-addressed object back and re-verifies its sha256.
    async fn get(&self, r: &BlobRef) -> Result<Bytes>;
    async fn reader(&self, ns: Ns, key: &str) -> Result<BoxAsyncRead>;
    async fn exists(&self, ns: Ns, key: &str) -> Result<bool>;
    /// Retention and takedown only.
    async fn delete(&self, ns: Ns, key: &str) -> Result<()>;
    /// A real path when the backend is local (grids need it for mmap).
    fn local_path(&self, ns: Ns, key: &str) -> Option<PathBuf>;
    /// A fresh scratch file path on the store's filesystem (for streamed
    /// downloads that are then moved in with [`BlobStore::put_cas_file`]).
    fn tmp_file(&self, hint: &str) -> Result<PathBuf>;
}

/// The filesystem backend.
#[derive(Debug, Clone)]
pub struct FsStore {
    root: PathBuf,
}

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

const CHUNK: usize = 1 << 20;

impl FsStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Absolute path of `ns/key`. Keys are validated: no `..`, no absolute
    /// paths, no empty segments.
    pub fn path(&self, ns: Ns, key: &str) -> Result<PathBuf> {
        ensure!(
            !key.is_empty()
                && !key.starts_with('/')
                && key
                    .split('/')
                    .all(|s| !s.is_empty() && s != "." && s != ".."),
            "invalid store key {key:?}"
        );
        Ok(self.root.join(ns.dir()).join(key))
    }

    /// Path of a content-addressed object.
    #[must_use]
    pub fn cas_path(&self, ns: Ns, sha256: &[u8; 32]) -> PathBuf {
        self.root.join(ns.dir()).join(ns.cas_key(sha256))
    }

    fn tmp_path(&self, hint: &str) -> Result<PathBuf> {
        let dir = self.root.join(Ns::Tmp.dir());
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let safe: String = hint
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .take(40)
            .collect();
        Ok(dir.join(format!(
            ".{}-{}-{safe}",
            std::process::id(),
            TMP_SEQ.fetch_add(1, Ordering::Relaxed)
        )))
    }

    /// Moves a fully written, fsynced temp file into `dest` (atomic).
    async fn commit(tmp: &Path, dest: &Path) -> Result<()> {
        let dir = dest.parent().context("store path has no parent")?;
        tokio::fs::create_dir_all(dir)
            .await
            .with_context(|| format!("creating {}", dir.display()))?;
        tokio::fs::rename(tmp, dest)
            .await
            .with_context(|| format!("renaming into {}", dest.display()))
    }

    /// Copies `src` into a temp file while hashing it; returns (tmp, sha, len).
    async fn copy_hashing(&self, src: &Path) -> Result<(PathBuf, [u8; 32], u64)> {
        let tmp = self.tmp_path("copy")?;
        let mut input = tokio::fs::File::open(src)
            .await
            .with_context(|| format!("opening {}", src.display()))?;
        let mut out = tokio::fs::File::create(&tmp)
            .await
            .with_context(|| format!("creating {}", tmp.display()))?;
        let mut h = Sha256::new();
        let mut buf = vec![0u8; CHUNK];
        let mut len = 0u64;
        loop {
            let n = input.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
            out.write_all(&buf[..n]).await?;
            len += n as u64;
        }
        out.sync_all().await?;
        Ok((tmp, h.finalize().into(), len))
    }

    /// sha256 and length of a file, streamed.
    pub async fn hash_file(path: &Path) -> Result<([u8; 32], u64)> {
        let mut f = tokio::fs::File::open(path)
            .await
            .with_context(|| format!("opening {}", path.display()))?;
        let mut h = Sha256::new();
        let mut buf = vec![0u8; CHUNK];
        let mut len = 0u64;
        loop {
            let n = f.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
            len += n as u64;
        }
        Ok((h.finalize().into(), len))
    }
}

#[async_trait]
impl BlobStore for FsStore {
    async fn put_cas(&self, ns: Ns, bytes: Bytes) -> Result<(BlobRef, bool)> {
        ensure!(ns.is_cas(), "{ns:?} is not content-addressed");
        let sha256: [u8; 32] = Sha256::digest(&bytes).into();
        let r = BlobRef {
            ns,
            key: ns.cas_key(&sha256),
            sha256,
            bytes: bytes.len() as u64,
        };
        let dest = self.cas_path(ns, &sha256);
        if tokio::fs::try_exists(&dest).await? {
            return Ok((r, false));
        }
        let tmp = self.tmp_path(&r.hex()[..16])?;
        {
            let mut f = tokio::fs::File::create(&tmp)
                .await
                .with_context(|| format!("creating {}", tmp.display()))?;
            f.write_all(&bytes).await?;
            f.sync_all().await?;
        }
        Self::commit(&tmp, &dest).await?;
        Ok((r, true))
    }

    async fn put_cas_file(&self, ns: Ns, src: &Path) -> Result<(BlobRef, bool)> {
        ensure!(ns.is_cas(), "{ns:?} is not content-addressed");
        let (sha256, len) = Self::hash_file(src).await?;
        let r = BlobRef {
            ns,
            key: ns.cas_key(&sha256),
            sha256,
            bytes: len,
        };
        let dest = self.cas_path(ns, &sha256);
        if tokio::fs::try_exists(&dest).await? {
            return Ok((r, false));
        }
        let (tmp, again, _) = self.copy_hashing(src).await?;
        if again != sha256 {
            let _ = tokio::fs::remove_file(&tmp).await;
            bail!("{} changed while being stored", src.display());
        }
        Self::commit(&tmp, &dest).await?;
        Ok((r, true))
    }

    async fn adopt_tmp(
        &self,
        ns: Ns,
        tmp: &Path,
        sha256: [u8; 32],
        bytes: u64,
    ) -> Result<(BlobRef, bool)> {
        ensure!(ns.is_cas(), "{ns:?} is not content-addressed");
        ensure!(
            tmp.starts_with(self.root.join(Ns::Tmp.dir())),
            "{} is not a scratch file of this store",
            tmp.display()
        );
        let r = BlobRef {
            ns,
            key: ns.cas_key(&sha256),
            sha256,
            bytes,
        };
        let dest = self.cas_path(ns, &sha256);
        if tokio::fs::try_exists(&dest).await? {
            let _ = tokio::fs::remove_file(tmp).await;
            return Ok((r, false));
        }
        tokio::fs::File::open(tmp).await?.sync_all().await?;
        Self::commit(tmp, &dest).await?;
        Ok((r, true))
    }

    async fn put_named(&self, ns: Ns, key: &str, src: &Path) -> Result<BlobRef> {
        ensure!(!ns.is_cas(), "{ns:?} is content-addressed; use put_cas");
        let dest = self.path(ns, key)?;
        let (tmp, sha256, len) = self.copy_hashing(src).await?;
        if tokio::fs::try_exists(&dest).await? {
            let _ = tokio::fs::remove_file(&tmp).await;
            let (existing, _) = Self::hash_file(&dest).await?;
            ensure!(
                existing == sha256,
                "{}/{key} exists with different content (named objects are immutable)",
                ns.dir()
            );
        } else {
            Self::commit(&tmp, &dest).await?;
        }
        Ok(BlobRef {
            ns,
            key: key.to_owned(),
            sha256,
            bytes: len,
        })
    }

    async fn get(&self, r: &BlobRef) -> Result<Bytes> {
        let path = self.path(r.ns, &r.key)?;
        let bytes = tokio::fs::read(&path)
            .await
            .with_context(|| format!("reading {}", path.display()))?;
        let actual: [u8; 32] = Sha256::digest(&bytes).into();
        ensure!(
            actual == r.sha256,
            "store object {} is corrupt (sha256 mismatch)",
            path.display()
        );
        Ok(Bytes::from(bytes))
    }

    async fn reader(&self, ns: Ns, key: &str) -> Result<BoxAsyncRead> {
        let path = self.path(ns, key)?;
        let f = tokio::fs::File::open(&path)
            .await
            .with_context(|| format!("opening {}", path.display()))?;
        Ok(Box::pin(f))
    }

    async fn exists(&self, ns: Ns, key: &str) -> Result<bool> {
        Ok(tokio::fs::try_exists(self.path(ns, key)?).await?)
    }

    async fn delete(&self, ns: Ns, key: &str) -> Result<()> {
        let path = self.path(ns, key)?;
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e).with_context(|| format!("deleting {}", path.display())),
        }
    }

    fn local_path(&self, ns: Ns, key: &str) -> Option<PathBuf> {
        self.path(ns, key).ok()
    }

    fn tmp_file(&self, hint: &str) -> Result<PathBuf> {
        self.tmp_path(hint)
    }
}

/// The store root from the environment: `LANKA_STORE_DIR`, else the
/// deprecated `LANKA_DATA_DIR` (accepted for one release), else `default`.
#[must_use]
pub fn root_from_env(default: PathBuf) -> PathBuf {
    std::env::var_os("LANKA_STORE_DIR")
        .or_else(|| std::env::var_os("LANKA_DATA_DIR"))
        .filter(|v| !v.is_empty())
        .map_or(default, PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_root(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lanka-store-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[tokio::test]
    async fn cas_is_idempotent_and_verified() {
        let root = tmp_root("cas");
        let s = FsStore::new(&root);
        let (r1, created) = s
            .put_cas(Ns::Raw, Bytes::from_static(b"hello"))
            .await
            .expect("put");
        assert!(created);
        assert_eq!(
            r1.hex(),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert_eq!(r1.rel_path(), format!("raw/2c/f2/{}", r1.hex()));
        let (r2, created) = s
            .put_cas(Ns::Raw, Bytes::from_static(b"hello"))
            .await
            .expect("put again");
        assert!(!created);
        assert_eq!(r1, r2);
        assert_eq!(&s.get(&r1).await.expect("get")[..], b"hello");
        let path = s.local_path(Ns::Raw, &r1.key).expect("local");
        std::fs::write(&path, b"tampered").expect("tamper");
        assert!(s.get(&r1).await.is_err(), "corruption is detected");
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn files_and_named_objects() {
        let root = tmp_root("named");
        let s = FsStore::new(&root);
        let src = root.join("src.bin");
        std::fs::create_dir_all(&root).expect("mkdir");
        std::fs::write(&src, b"payload").expect("write");
        let (r, created) = s.put_cas_file(Ns::Extract, &src).await.expect("file");
        assert!(created && r.key.ends_with(".parquet"));
        assert!(s.exists(Ns::Extract, &r.key).await.expect("exists"));
        let n = s
            .put_named(Ns::Dataset, "ds/dsv_1/data.csv", &src)
            .await
            .expect("named");
        assert_eq!(n.sha256, r.sha256);
        s.put_named(Ns::Dataset, "ds/dsv_1/data.csv", &src)
            .await
            .expect("same content is fine");
        std::fs::write(&src, b"other").expect("write");
        assert!(
            s.put_named(Ns::Dataset, "ds/dsv_1/data.csv", &src)
                .await
                .is_err(),
            "named objects are immutable"
        );
        assert!(s.path(Ns::Dataset, "../escape").is_err());
        let mut rd = s
            .reader(Ns::Dataset, "ds/dsv_1/data.csv")
            .await
            .expect("rd");
        let mut out = Vec::new();
        rd.read_to_end(&mut out).await.expect("read");
        assert_eq!(out, b"payload");
        s.delete(Ns::Dataset, "ds/dsv_1/data.csv")
            .await
            .expect("delete");
        assert!(!s.exists(Ns::Dataset, "ds/dsv_1/data.csv").await.expect("x"));
        let _ = std::fs::remove_dir_all(root);
    }
}
