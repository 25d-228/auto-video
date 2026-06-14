//! libtorrent-rasterbar backend (replaces librqbit).
//!
//! A thin `cxx` bridge over a C++ shim (`lt_shim.cpp`) that owns a single global
//! libtorrent session. The key win over librqbit: deselected files are given
//! `file_priority = 0` (dont_download), so libtorrent (sparse storage) NEVER
//! creates them on disk — no 0-byte stub files.
//!
//! The model is poll-based to match the download command's existing loop:
//! `add` returns an info-hash id, then `status(id)` is polled each tick. File
//! selection + the post-metadata resume are applied lazily inside `status` the
//! first time metadata is available (so nothing is written before priorities are
//! set, and `add` never blocks waiting for metadata).

#[cxx::bridge]
mod ffi {
    /// One file inside a torrent (for the download file-picker).
    struct LtFile {
        index: u32,
        /// Path within the torrent (slash-joined for multi-file torrents).
        path: String,
        size: u64,
    }

    /// A torrent's polled status.
    struct LtStatus {
        /// false when the id is unknown to the session.
        valid: bool,
        has_metadata: bool,
        /// 0..1 over the WANTED (selected) bytes.
        progress: f64,
        /// "downloading" | "paused" | "done" | "error".
        state: String,
        /// Download payload rate in bytes/sec.
        download_rate: u64,
        /// All wanted pieces present.
        finished: bool,
        /// Non-empty only when state == "error".
        error: String,
    }

    unsafe extern "C++" {
        include!("lt_shim.h");

        /// Boot the global libtorrent session once. Idempotent; false on failure.
        fn lt_init() -> bool;

        /// Add a magnet for download into `save_path`, downloading ONLY the file
        /// indices in `only_files` (empty = all). Returns the info-hash hex id,
        /// or "" on error. The torrent starts paused; selection + resume are
        /// applied by the first `lt_status` once metadata is known, so deselected
        /// files (priority 0) are never created.
        fn lt_add(magnet: &str, save_path: &str, only_files: &[u32]) -> String;

        /// Resolve a magnet's file list without downloading data (metadata only),
        /// bounded by `timeout_ms`. Empty vec on timeout/error. BLOCKING — call
        /// from a blocking task.
        fn lt_list_files(magnet: &str, timeout_ms: u32) -> Vec<LtFile>;

        /// Current status of a torrent by info-hash id.
        fn lt_status(id: &str) -> LtStatus;

        /// User pause / resume (distinct from the internal add-paused state).
        fn lt_pause(id: &str);
        fn lt_resume(id: &str);

        /// Remove the torrent from the session, optionally deleting its files.
        fn lt_remove(id: &str, delete_files: bool);
    }
}

pub use ffi::{LtFile, LtStatus};

/// Boot the session (idempotent).
pub fn init() -> bool {
    ffi::lt_init()
}

/// Add a magnet; returns the info-hash id ("" on failure).
pub fn add(magnet: &str, save_path: &str, only_files: &[u32]) -> String {
    ffi::lt_add(magnet, save_path, only_files)
}

/// Blocking metadata-only file listing.
pub fn list_files(magnet: &str, timeout_ms: u32) -> Vec<LtFile> {
    ffi::lt_list_files(magnet, timeout_ms)
}

pub fn status(id: &str) -> LtStatus {
    ffi::lt_status(id)
}

pub fn pause(id: &str) {
    ffi::lt_pause(id)
}

pub fn resume(id: &str) {
    ffi::lt_resume(id)
}

pub fn remove(id: &str, delete_files: bool) {
    ffi::lt_remove(id, delete_files)
}
