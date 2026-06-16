//! Pure-Rust torrent backend (librqbit) — replaces the libtorrent C++ shim.
//!
//! librqbit was originally dropped because its default filesystem storage opens
//! and creates EVERY file in the torrent up front (even deselected ones) as
//! 0-byte stubs. This module plugs a custom [`LazyStorage`] into librqbit that:
//!   - creates a file on disk only on the FIRST write to it, and
//!   - SKIPS writes to deselected files entirely — including the boundary bytes
//!     of a piece shared with a selected file.
//! So deselected files are never created (no stubs) — the same clean on-disk
//! result as libtorrent's sparse + priority-0, but in pure Rust (no C++, no
//! Homebrew dylib). Torrent resume is still managed by the app, but DHT keeps
//! its own lightweight routing-table cache so magnet starts are not always cold.

use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use librqbit::storage::{BoxStorageFactory, StorageFactory, StorageFactoryExt, TorrentStorage};
use librqbit::{
    dht::PersistentDhtConfig, AddTorrent, AddTorrentOptions, AddTorrentResponse, ManagedTorrent,
    ManagedTorrentShared, Session, SessionOptions, TorrentMetadata, TorrentStatsState,
};
use tokio::sync::OnceCell;

/// A managed-torrent handle. librqbit's `ManagedTorrentHandle` alias isn't
/// re-exported at the crate root, but it is exactly `Arc<ManagedTorrent>`.
pub type Handle = Arc<ManagedTorrent>;

/// Bytes per MiB. librqbit reports live speed in MiB/s; the UI wants bytes/sec.
const BYTES_PER_MIB: f64 = 1_048_576.0;

// ----------------------------------------------------------------- session

static SESSION: OnceCell<Arc<Session>> = OnceCell::const_new();

const PUBLIC_TRACKERS: &[&str] = &[
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://open.demonii.com:1337/announce",
    "udp://tracker.openbittorrent.com:6969/announce",
    "udp://exodus.desync.com:6969/announce",
];

fn public_trackers() -> HashSet<url::Url> {
    PUBLIC_TRACKERS
        .iter()
        .filter_map(|tracker| tracker.parse().ok())
        .collect()
}

fn dht_cache_file() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join("Library")
                .join("Caches")
                .join("auto-video")
                .join("librqbit-dht.json");
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(local_app_data)
                .join("auto-video")
                .join("librqbit-dht.json");
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        if let Some(cache_home) = std::env::var_os("XDG_CACHE_HOME") {
            return PathBuf::from(cache_home)
                .join("auto-video")
                .join("librqbit-dht.json");
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join(".cache")
                .join("auto-video")
                .join("librqbit-dht.json");
        }
    }

    std::env::temp_dir().join("auto-video-librqbit-dht.json")
}

/// Lazily boot the single global librqbit session.
pub async fn session() -> anyhow::Result<Arc<Session>> {
    SESSION
        .get_or_try_init(|| async {
            Session::new_with_opts(
                // Never used: every add sets its own output_folder + storage.
                std::env::temp_dir(),
                SessionOptions {
                    persistence: None,
                    disable_dht_persistence: false,
                    dht_config: Some(PersistentDhtConfig {
                        config_filename: Some(dht_cache_file()),
                        ..Default::default()
                    }),
                    listen_port_range: Some(6881..6999),
                    enable_upnp_port_forwarding: true,
                    trackers: public_trackers(),
                    ..Default::default()
                },
            )
            .await
        })
        .await
        .map(Arc::clone)
}

// ----------------------------------------------------------------- lazy storage

#[cfg(unix)]
fn pwrite(f: &File, offset: u64, buf: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::FileExt;
    f.write_all_at(buf, offset)
}
#[cfg(unix)]
fn pread(f: &File, offset: u64, buf: &mut [u8]) -> std::io::Result<()> {
    use std::os::unix::fs::FileExt;
    f.read_exact_at(buf, offset)
}
#[cfg(windows)]
fn pwrite(f: &File, mut offset: u64, mut buf: &[u8]) -> std::io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        let n = f.seek_write(buf, offset)?;
        buf = &buf[n..];
        offset += n as u64;
    }
    Ok(())
}
#[cfg(windows)]
fn pread(f: &File, mut offset: u64, mut buf: &mut [u8]) -> std::io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        let n = f.seek_read(buf, offset)?;
        if n == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "short read"));
        }
        buf = &mut buf[n..];
        offset += n as u64;
    }
    Ok(())
}

struct LazyFile {
    path: PathBuf,
    padding: bool,
    handle: Mutex<Option<File>>,
}

/// Storage that materializes a file only on first write and never writes to
/// deselected files. See the module docs.
struct LazyStorage {
    base: PathBuf,
    files: Vec<LazyFile>,
    selected: Option<HashSet<usize>>, // None = all files selected
}

impl LazyStorage {
    /// A file we should actually write to disk (non-padding and selected).
    fn wanted(&self, id: usize) -> bool {
        self.files.get(id).map_or(false, |f| !f.padding)
            && self.selected.as_ref().map_or(true, |s| s.contains(&id))
    }
}

impl TorrentStorage for LazyStorage {
    fn pread_exact(&self, file_id: usize, offset: u64, buf: &mut [u8]) -> anyhow::Result<()> {
        let lf = self.files.get(file_id).context("no such file")?;
        let mut g = lf.handle.lock().unwrap();
        if g.is_none() {
            // Open an EXISTING file only — a missing file surfaces as Err, which
            // the integrity check treats as "piece not present" (no creation).
            *g = Some(
                OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&lf.path)
                    .with_context(|| format!("open(read) {:?}", lf.path))?,
            );
        }
        pread(g.as_ref().unwrap(), offset, buf)?;
        Ok(())
    }

    fn pwrite_all(&self, file_id: usize, offset: u64, buf: &[u8]) -> anyhow::Result<()> {
        // Skip deselected / padding files entirely so they are never created —
        // including the boundary bytes of a piece shared with a selected file.
        if !self.wanted(file_id) {
            return Ok(());
        }
        let lf = &self.files[file_id];
        let mut g = lf.handle.lock().unwrap();
        if g.is_none() {
            if let Some(parent) = lf.path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            *g = Some(
                OpenOptions::new()
                    .create(true)
                    .read(true)
                    .write(true)
                    .open(&lf.path)
                    .with_context(|| format!("create {:?}", lf.path))?,
            );
        }
        pwrite(g.as_ref().unwrap(), offset, buf)?;
        Ok(())
    }

    fn remove_file(&self, file_id: usize, _filename: &Path) -> anyhow::Result<()> {
        if let Some(lf) = self.files.get(file_id) {
            *lf.handle.lock().unwrap() = None;
            match std::fs::remove_file(&lf.path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }

    fn remove_directory_if_empty(&self, path: &Path) -> anyhow::Result<()> {
        let full = self.base.join(path);
        if full.is_dir() && std::fs::read_dir(&full)?.next().is_none() {
            let _ = std::fs::remove_dir(&full);
        }
        Ok(())
    }

    fn ensure_file_length(&self, _file_id: usize, _length: u64) -> anyhow::Result<()> {
        // No-op: stay lazy/sparse. Selected files are created on first write and
        // grow to their true length as pieces land.
        Ok(())
    }

    fn take(&self) -> anyhow::Result<Box<dyn TorrentStorage>> {
        // Move the open handles into a fresh storage, neutering this one (called
        // on pause/delete), mirroring FilesystemStorage::take.
        let files = self
            .files
            .iter()
            .map(|lf| LazyFile {
                path: lf.path.clone(),
                padding: lf.padding,
                handle: Mutex::new(lf.handle.lock().unwrap().take()),
            })
            .collect();
        Ok(Box::new(LazyStorage {
            base: self.base.clone(),
            files,
            selected: self.selected.clone(),
        }))
    }

    fn init(
        &mut self,
        _shared: &ManagedTorrentShared,
        _meta: &TorrentMetadata,
    ) -> anyhow::Result<()> {
        // Crucially a NO-OP: create nothing here. (The stock storage opens every
        // file here — that is what produced the 0-byte stubs.)
        Ok(())
    }
}

#[derive(Clone)]
struct LazyStorageFactory {
    base: PathBuf,
    selected: Option<Arc<HashSet<usize>>>,
}

impl StorageFactory for LazyStorageFactory {
    type Storage = LazyStorage;

    fn create(
        &self,
        _shared: &ManagedTorrentShared,
        metadata: &TorrentMetadata,
    ) -> anyhow::Result<LazyStorage> {
        let files = metadata
            .file_infos
            .iter()
            .map(|fi| LazyFile {
                path: self.base.join(&fi.relative_filename),
                padding: fi.attrs.padding,
                handle: Mutex::new(None),
            })
            .collect();
        Ok(LazyStorage {
            base: self.base.clone(),
            files,
            selected: self.selected.as_ref().map(|s| (**s).clone()),
        })
    }

    fn clone_box(&self) -> BoxStorageFactory {
        self.clone().boxed()
    }
}

// ----------------------------------------------------------------- ops

fn selected_set(only_files: &[usize]) -> Option<Arc<HashSet<usize>>> {
    if only_files.is_empty() {
        None
    } else {
        Some(Arc::new(only_files.iter().copied().collect()))
    }
}

/// Add a magnet for download into `dest`, fetching ONLY `only_files` (empty =
/// all). Returns (torrent_id, handle). Bounded by a metadata-fetch timeout (a
/// bare magnet otherwise awaits peers forever).
pub async fn add(magnet: &str, dest: &str, only_files: &[usize]) -> anyhow::Result<(usize, Handle)> {
    let s = session().await?;
    let factory = LazyStorageFactory {
        base: PathBuf::from(dest),
        selected: selected_set(only_files),
    };
    let only = (!only_files.is_empty()).then(|| only_files.to_vec());
    let resp = tokio::time::timeout(
        Duration::from_secs(60),
        s.add_torrent(
            AddTorrent::from_url(magnet),
            Some(AddTorrentOptions {
                only_files: only,
                output_folder: Some(dest.to_string()),
                overwrite: true,
                paused: false,
                storage_factory: Some(factory.boxed()),
                ..Default::default()
            }),
        ),
    )
    .await
    .context("timed out fetching torrent metadata (no peers?)")??;
    match resp {
        AddTorrentResponse::Added(id, h) | AddTorrentResponse::AlreadyManaged(id, h) => Ok((id, h)),
        AddTorrentResponse::ListOnly(_) => anyhow::bail!("unexpected list-only response"),
    }
}

/// A polled status snapshot (shape kept close to the old shim's LtStatus).
pub struct Status {
    pub progress: f64,      // 0..1 over the SELECTED files
    pub state: String,      // "downloading" | "paused" | "done" | "error"
    pub download_rate: u64, // bytes/sec
    pub finished: bool,
    pub error: String,
}

pub fn status(handle: &Handle) -> Status {
    let s = handle.stats();
    // total_bytes is whole-torrent (not selected-only) during Initializing, so
    // don't compute a percentage until past it.
    let progress = match s.state {
        TorrentStatsState::Initializing => 0.0,
        _ if s.total_bytes > 0 => (s.progress_bytes as f64 / s.total_bytes as f64).clamp(0.0, 1.0),
        _ => 0.0,
    };
    let download_rate = s
        .live
        .as_ref()
        .map(|l| (l.download_speed.mbps * BYTES_PER_MIB) as u64)
        .unwrap_or(0);
    let (state, error) = if matches!(s.state, TorrentStatsState::Error) {
        ("error".to_string(), s.error.clone().unwrap_or_default())
    } else if s.finished {
        ("done".to_string(), String::new())
    } else if matches!(s.state, TorrentStatsState::Paused) {
        ("paused".to_string(), String::new())
    } else {
        ("downloading".to_string(), String::new())
    };
    Status {
        progress,
        state,
        download_rate,
        finished: s.finished,
        error,
    }
}

pub async fn pause(handle: &Handle) -> anyhow::Result<()> {
    let s = session().await?;
    s.pause(handle).await
}

pub async fn resume(handle: &Handle) -> anyhow::Result<()> {
    let s = session().await?;
    s.unpause(handle).await
}

pub async fn remove(torrent_id: usize, delete_files: bool) -> anyhow::Result<()> {
    let s = session().await?;
    s.delete(librqbit::api::TorrentIdOrHash::from(torrent_id), delete_files)
        .await
}

/// Set the session-wide max rates in bytes/sec (0 = unlimited).
pub async fn set_rate_limits(download_bps: u32, upload_bps: u32) -> anyhow::Result<()> {
    let s = session().await?;
    s.ratelimits.set_download_bps(NonZeroU32::new(download_bps));
    s.ratelimits.set_upload_bps(NonZeroU32::new(upload_bps));
    Ok(())
}

/// One file inside a torrent (index/name/size) for the picker.
pub struct FileEntry {
    pub index: usize,
    pub name: String,
    pub size: u64,
}

/// Resolve a magnet's file list WITHOUT downloading (list-only), bounded by a
/// timeout. The `index` matches the `only_files` / storage file id.
pub async fn list_files(magnet: &str, timeout_ms: u64) -> anyhow::Result<Vec<FileEntry>> {
    let lo = list_only(magnet, timeout_ms).await?;
    let mut out = Vec::new();
    for (index, fd) in lo.info.iter_file_details()?.enumerate() {
        out.push(FileEntry {
            index,
            name: fd
                .filename
                .to_string()
                .unwrap_or_else(|_| format!("file {index}")),
            size: fd.len,
        });
    }
    Ok(out)
}

/// Fetch a magnet's metadata (list-only) and write its .torrent bytes to disk.
pub async fn save_torrent(magnet: &str, out_path: &str, timeout_ms: u64) -> anyhow::Result<()> {
    let lo = list_only(magnet, timeout_ms).await?;
    std::fs::write(out_path, &lo.torrent_bytes).with_context(|| format!("write {out_path}"))?;
    Ok(())
}

/// Shared list-only add (metadata only, never starts a download).
async fn list_only(magnet: &str, timeout_ms: u64) -> anyhow::Result<librqbit::ListOnlyResponse> {
    let s = session().await?;
    let resp = tokio::time::timeout(
        Duration::from_millis(timeout_ms),
        s.add_torrent(
            AddTorrent::from_url(magnet),
            Some(AddTorrentOptions {
                list_only: true,
                ..Default::default()
            }),
        ),
    )
    .await
    .context("timed out fetching torrent metadata (no peers?)")??;
    match resp {
        AddTorrentResponse::ListOnly(lo) => Ok(lo),
        _ => anyhow::bail!("expected a list-only response"),
    }
}
