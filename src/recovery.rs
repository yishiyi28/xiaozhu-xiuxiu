//! Crash recovery snapshots.
//!
//! Snapshots live outside the user's document tree so an interrupted save can
//! never damage the project itself. Each snapshot is a regular `.xuan` archive
//! plus a `meta.json` commit marker; a snapshot is only listed after both files
//! are durable, which makes torn snapshots harmless.
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{document::Document, io};

pub const SNAPSHOT_FILE: &str = "document.xuan";
pub const META_FILE: &str = "meta.json";
/// Snapshots older than two weeks are removed during a startup scan.
const MAX_AGE_SECONDS: u64 = 14 * 24 * 60 * 60;
/// Upper bound on retained snapshots, newest first.
const MAX_SNAPSHOTS: usize = 32;

/// Metadata committed alongside every recovery snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meta {
    pub id: String,
    pub title: String,
    pub source: Option<PathBuf>,
    pub created_unix: u64,
    pub app_version: String,
}

/// A complete, loadable snapshot found on disk.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub dir: PathBuf,
    pub meta: Meta,
}

impl Candidate {
    pub fn snapshot_path(&self) -> PathBuf {
        self.dir.join(SNAPSHOT_FILE)
    }
}

/// Recovery storage rooted at a directory. The user store lives under the
/// platform data directory; tests construct one with [`Store::at`].
#[derive(Clone, Debug)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn user() -> Result<Self> {
        if let Some(path) = std::env::var_os("XUAN_RECOVERY_DIR").filter(|p| !p.is_empty()) {
            return Ok(Self::at(PathBuf::from(path)));
        }
        let base = if cfg!(windows) {
            std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
        } else {
            std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .or_else(|| {
                    std::env::var_os("HOME")
                        .map(PathBuf::from)
                        .filter(|p| p.is_absolute())
                        .map(|home| home.join(".local/share"))
                })
        };
        let base = base.context("Cannot locate the user data directory")?;
        Ok(Self::at(base.join("xuan").join("recovery")))
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Atomically write a snapshot for `document`. The project archive is
    /// replaced first and `meta.json` last, so `meta.json` always describes a
    /// complete archive even after a crash mid-write.
    pub fn snapshot(
        &self,
        document: &Document,
        title: &str,
        source: Option<&Path>,
        created_unix: u64,
    ) -> Result<PathBuf> {
        let dir = self.root.join(document.id.to_string());
        fs::create_dir_all(&dir)
            .with_context(|| format!("Cannot create recovery directory {}", dir.display()))?;
        io::save(document, &dir.join(SNAPSHOT_FILE))?;
        let meta = Meta {
            id: document.id.to_string(),
            title: title.to_owned(),
            source: source.map(Path::to_path_buf),
            created_unix,
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
        };
        write_atomic(&dir.join(META_FILE), &serde_json::to_vec_pretty(&meta)?)?;
        Ok(dir)
    }

    /// List loadable snapshots, newest first, pruning stale or surplus ones.
    /// Malformed entries are ignored rather than reported: a crash can leave
    /// partial directories and those must not block startup.
    pub fn snapshots(&self) -> Result<Vec<Candidate>> {
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(error).with_context(|| format!("Cannot read {}", self.root.display()));
            }
        };
        let mut candidates = Vec::new();
        for entry in entries {
            let Ok(entry) = entry else { continue };
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let dir = entry.path();
            let Ok(text) = fs::read_to_string(dir.join(META_FILE)) else {
                continue;
            };
            let Ok(meta) = serde_json::from_str::<Meta>(&text) else {
                continue;
            };
            if meta.id != entry.file_name().to_string_lossy() || Uuid::parse_str(&meta.id).is_err()
            {
                continue;
            }
            if !dir.join(SNAPSHOT_FILE).is_file() {
                continue;
            }
            candidates.push(Candidate { dir, meta });
        }
        candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.meta.created_unix));
        self.prune(&mut candidates);
        Ok(candidates)
    }

    pub fn load(&self, candidate: &Candidate) -> Result<Document> {
        io::load(&candidate.snapshot_path())
    }

    pub fn discard(&self, id: Uuid) -> Result<()> {
        let dir = self.root.join(id.to_string());
        match fs::remove_dir_all(&dir) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).with_context(|| format!("Cannot remove {}", dir.display())),
        }
    }

    pub fn discard_candidate(&self, candidate: &Candidate) -> Result<()> {
        match fs::remove_dir_all(&candidate.dir) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => {
                Err(error).with_context(|| format!("Cannot remove {}", candidate.dir.display()))
            }
        }
    }

    fn prune(&self, candidates: &mut Vec<Candidate>) {
        let cutoff = unix_now().saturating_sub(MAX_AGE_SECONDS);
        let mut kept = Vec::new();
        for (index, candidate) in std::mem::take(candidates).into_iter().enumerate() {
            if index < MAX_SNAPSHOTS && candidate.meta.created_unix >= cutoff {
                kept.push(candidate);
            } else {
                let _ = fs::remove_dir_all(&candidate.dir);
            }
        }
        *candidates = kept;
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("Recovery path has no parent")?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|error| error.error)?;
    Ok(())
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// Compact age label for the recovery dialog.
pub fn age_label(created_unix: u64, now_unix: u64) -> String {
    let seconds = now_unix.saturating_sub(created_unix);
    match seconds {
        0..=59 => format!("{seconds}s ago"),
        60..=3599 => format!("{} min ago", seconds / 60),
        3600..=86_399 => format!("{} h ago", seconds / 3_600),
        _ => format!("{} d ago", seconds / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use std::sync::Arc;

    fn doc(color: [u8; 4]) -> Document {
        let mut document = Document::new(3, 2).unwrap();
        document.layers[0].pixels = Some(Arc::new(RgbaImage::from_pixel(3, 2, Rgba(color)).into()));
        document
    }

    #[test]
    fn snapshot_round_trips_and_can_be_discarded() {
        let temporary = tempfile::tempdir().unwrap();
        let store = Store::at(temporary.path().join("recovery"));
        let document = doc([10, 20, 30, 255]);
        let source = temporary.path().join("photo.xuan");
        store
            .snapshot(&document, "Photo", Some(&source), unix_now())
            .unwrap();

        let found = store.snapshots().unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].meta.title, "Photo");
        assert_eq!(found[0].meta.source.as_deref(), Some(source.as_path()));
        assert_eq!(found[0].meta.id, document.id.to_string());

        let loaded = store.load(&found[0]).unwrap();
        assert_eq!((loaded.width, loaded.height), (3, 2));
        assert_eq!(
            loaded.layers[0].pixels.as_ref().unwrap().get_pixel(0, 0).0,
            [10, 20, 30, 255]
        );

        store.discard_candidate(&found[0]).unwrap();
        assert!(store.snapshots().unwrap().is_empty());
    }

    #[test]
    fn newer_snapshot_replaces_the_same_document() {
        let temporary = tempfile::tempdir().unwrap();
        let store = Store::at(temporary.path());
        let document = doc([1, 2, 3, 255]);
        store
            .snapshot(&document, "Draft", None, unix_now())
            .unwrap();
        let mut edited = doc([9, 8, 7, 255]);
        edited.id = document.id;
        store.snapshot(&edited, "Draft", None, unix_now()).unwrap();

        let found = store.snapshots().unwrap();
        assert_eq!(found.len(), 1);
        let loaded = store.load(&found[0]).unwrap();
        assert_eq!(
            loaded.layers[0].pixels.as_ref().unwrap().get_pixel(0, 0).0,
            [9, 8, 7, 255]
        );
    }

    #[test]
    fn stale_snapshots_are_pruned_but_fresh_ones_survive() {
        let temporary = tempfile::tempdir().unwrap();
        let store = Store::at(temporary.path());
        let fresh = doc([4, 5, 6, 255]);
        let stale = doc([7, 8, 9, 255]);
        store.snapshot(&fresh, "Fresh", None, unix_now()).unwrap();
        let stale_dir = store
            .snapshot(&stale, "Stale", None, unix_now() - MAX_AGE_SECONDS - 1)
            .unwrap();

        let found = store.snapshots().unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].meta.title, "Fresh");
        assert!(!stale_dir.exists());
    }

    #[test]
    fn malformed_entries_do_not_block_recovery() {
        let temporary = tempfile::tempdir().unwrap();
        let store = Store::at(temporary.path());
        let broken = temporary.path().join(Uuid::new_v4().to_string());
        fs::create_dir_all(&broken).unwrap();
        fs::write(broken.join(META_FILE), "{not json").unwrap();
        fs::write(broken.join(SNAPSHOT_FILE), b"not a project").unwrap();
        let unrelated = temporary.path().join("notes");
        fs::create_dir_all(&unrelated).unwrap();

        let good = doc([1, 1, 1, 255]);
        store.snapshot(&good, "Good", None, unix_now()).unwrap();
        let found = store.snapshots().unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].meta.title, "Good");
    }
}
