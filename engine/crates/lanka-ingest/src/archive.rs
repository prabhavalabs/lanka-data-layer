//! Content-addressed raw archive on local disk.
//!
//! Layout: `<root>/raw/<h0h1>/<h2h3>/<sha256-hex>`. Writes are atomic
//! (temp file + fsync + rename), so a crash never leaves a truncated object
//! under a valid name, and identical bytes are stored once.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
pub struct Archive {
    root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct Stored {
    pub sha256: [u8; 32],
    pub path: PathBuf,
    pub bytes: u64,
    /// False when the object was already archived.
    pub created: bool,
}

impl Stored {
    #[must_use]
    pub fn hex(&self) -> String {
        hex::encode(self.sha256)
    }
}

impl Archive {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn path_for(&self, sha256: &[u8; 32]) -> PathBuf {
        let h = hex::encode(sha256);
        self.root.join("raw").join(&h[0..2]).join(&h[2..4]).join(h)
    }

    /// Stores `bytes`, returning their digest and location. Blocking I/O —
    /// call through `spawn_blocking` from async code.
    pub fn put(&self, bytes: &[u8]) -> Result<Stored> {
        let sha256: [u8; 32] = Sha256::digest(bytes).into();
        let path = self.path_for(&sha256);
        let len = bytes.len() as u64;
        if path.is_file() {
            return Ok(Stored {
                sha256,
                path,
                bytes: len,
                created: false,
            });
        }
        let dir = path.parent().context("archive path has no parent")?;
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let tmp = dir.join(format!(
            ".tmp-{}-{}",
            std::process::id(),
            hex::encode(&sha256[..8])
        ));
        {
            let mut f = std::fs::File::create(&tmp)
                .with_context(|| format!("creating {}", tmp.display()))?;
            f.write_all(bytes)?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, &path)
            .with_context(|| format!("renaming into {}", path.display()))?;
        Ok(Stored {
            sha256,
            path,
            bytes: len,
            created: true,
        })
    }

    /// Reads an object back and re-verifies its digest.
    pub fn get(&self, sha256: &[u8; 32]) -> Result<Vec<u8>> {
        let path = self.path_for(sha256);
        let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        let actual: [u8; 32] = Sha256::digest(&bytes).into();
        anyhow::ensure!(
            &actual == sha256,
            "archive object {} is corrupt",
            path.display()
        );
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_is_idempotent_and_verified() {
        let dir = std::env::temp_dir().join(format!("lanka-archive-{}", std::process::id()));
        let a = Archive::new(&dir);
        let s1 = a.put(b"hello").expect("put");
        assert!(s1.created);
        assert_eq!(
            s1.hex(),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        let s2 = a.put(b"hello").expect("put again");
        assert!(!s2.created);
        assert_eq!(a.get(&s1.sha256).expect("get"), b"hello");
        std::fs::write(&s1.path, b"tampered").expect("tamper");
        assert!(a.get(&s1.sha256).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
