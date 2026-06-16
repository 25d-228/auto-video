use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{Emitter, Manager};

mod dl;

/// One managed download: the librqbit handle + numeric torrent id plus the
/// metadata every event carries. Both reach the torrent in the global session.
struct DlEntry {
    handle: dl::Handle,
    torrent_id: usize,
    title: String,
    dest: String,
}
/// our download id -> managed download. Removal from this map is what stops the
/// polling loop (and signals a cancel to a still-running download).
struct DlManager(Mutex<HashMap<String, DlEntry>>);

/// Lock the download-management map. The guard borrows from `app` (not the
/// temporary `State`), so callers can chain map operations directly.
fn dl_map(app: &tauri::AppHandle) -> std::sync::MutexGuard<'_, HashMap<String, DlEntry>> {
    app.state::<DlManager>().inner().0.lock().unwrap()
}

// ------------------------------------------------------------------ downloads
/// The `state` value the frontend treats as "finished"; also the literal the
/// polling loop watches for. Spelled once so it can't drift between producers.
const DL_STATE_DONE: &str = "done";

/// Bytes per MiB. librqbit reports rates/sizes in bytes; the UI shows MB/s.
const BYTES_PER_MIB: f64 = 1_048_576.0;

/// How long the metadata-only shims (list_torrent_files / save_torrent) wait for
/// a bare magnet's metadata to arrive from peers/DHT before giving up.
const METADATA_FETCH_TIMEOUT_MS: u64 = 45_000;

#[derive(Clone, serde::Serialize)]
struct DlProgress {
    id: String,
    title: String,
    progress: f64, // 0..1
    speed_mbps: f64,
    state: String, // "downloading" | "paused" | "done" | "error"
    dest: String,
    /// Error message, present only when state == "error".
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Snapshot a torrent's current librqbit status into the event payload shape.
fn dl_progress(id: &str, handle: &dl::Handle, title: &str, dest: &str) -> DlProgress {
    let st = dl::status(handle);
    // A finished torrent reads as "done" so the polling loop ends.
    let (state, error) = if st.finished {
        (DL_STATE_DONE.to_owned(), None)
    } else if st.state == "error" {
        ("error".to_owned(), (!st.error.is_empty()).then(|| st.error.clone()))
    } else {
        (st.state.clone(), None)
    };
    DlProgress {
        id: id.to_owned(),
        title: title.to_owned(),
        progress: st.progress.clamp(0.0, 1.0),
        // librqbit reports bytes/sec; the UI shows MB/s.
        speed_mbps: st.download_rate as f64 / BYTES_PER_MIB,
        state,
        dest: dest.to_owned(),
        error,
    }
}

/// Emit a one-off "error" progress event for a download that failed before it
/// could be registered (bad dest, session init, or add_torrent). The frontend
/// flips its optimistic entry to the error state.
fn emit_dl_error(app: &tauri::AppHandle, id: &str, title: &str, dest: &str, msg: String) {
    let _ = app.emit(
        "download-progress",
        DlProgress {
            id: id.to_owned(),
            title: title.to_owned(),
            progress: 0.0,
            speed_mbps: 0.0,
            state: "error".to_owned(),
            dest: dest.to_owned(),
            error: Some(msg),
        },
    );
}

/// Clone the managed entry's (handle, title, dest), or explain it isn't managed.
fn dl_entry(app: &tauri::AppHandle, id: &str) -> Result<(dl::Handle, String, String), String> {
    let mgr = app.state::<DlManager>();
    let g = mgr.0.lock().unwrap();
    g.get(id)
        .map(|e| (e.handle.clone(), e.title.clone(), e.dest.clone()))
        .ok_or_else(|| format!("unknown download id: {id}"))
}

#[tauri::command]
async fn pause_download(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let (handle, title, dest) = dl_entry(&app, &id)?;
    dl::pause(&handle).await.map_err(|e| e.to_string())?;
    let _ = app.emit("download-progress", dl_progress(&id, &handle, &title, &dest));
    Ok(())
}

#[tauri::command]
async fn resume_download(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let (handle, title, dest) = dl_entry(&app, &id)?;
    dl::resume(&handle).await.map_err(|e| e.to_string())?;
    let _ = app.emit("download-progress", dl_progress(&id, &handle, &title, &dest));
    Ok(())
}

#[tauri::command]
async fn cancel_download(
    app: tauri::AppHandle,
    id: String,
    delete_files: bool,
) -> Result<(), String> {
    // Remove from the map first: the polling loop checks membership before every
    // emit, so no further events fire for this id once the entry is gone.
    let entry = dl_map(&app)
        .remove(&id)
        .ok_or_else(|| format!("unknown download id: {id}"))?;
    dl::remove(entry.torrent_id, delete_files)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// A post-download rename: source -> target path, both relative to the dest folder.
#[derive(Clone, serde::Deserialize)]
struct RenamePair {
    from: String,
    to: String,
}

/// Resolve the output folder: the requested dir (created if needed), else
/// ~/Downloads/auto-video.
fn resolve_out_dir(app: &tauri::AppHandle, dest: &str) -> Result<PathBuf, String> {
    if !dest.trim().is_empty() {
        let p = PathBuf::from(dest);
        if std::fs::create_dir_all(&p).is_ok() {
            return Ok(p);
        }
    }
    let d = app.path().download_dir().map_err(|e| e.to_string())?;
    let p = d.join("auto-video");
    let _ = std::fs::create_dir_all(&p);
    Ok(p)
}

/// Canonicalize the downloaded file names once a download finishes: remove the
/// torrent from the session (keeping the files) so librqbit closes its
/// handles, rename per the plan, then drop the management entry. A no-op (and
/// leaves the entry) when there are no renames.
async fn apply_renames(app: &tauri::AppHandle, id: &str, dest_out: &str, renames: Option<&Vec<RenamePair>>) {
    let Some(plan) = renames.filter(|p| !p.is_empty()) else { return };
    let torrent_id = dl_map(app).get(id).map(|e| e.torrent_id);
    if let Some(torrent_id) = torrent_id {
        // Remove from the session (keep files) so librqbit releases its handles.
        let _ = dl::remove(torrent_id, false).await;
        // Give it a moment to flush + close the files before moving them.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    for pair in plan {
        let src = Path::new(dest_out).join(&pair.from);
        let dst = Path::new(dest_out).join(&pair.to);
        if !(src.exists() && !dst.exists()) {
            continue;
        }
        if let Some(parent) = dst.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::rename(&src, &dst);
    }
    // Torrent removed + files renamed: drop the management entry.
    dl_map(app).remove(id);
}

#[tauri::command]
async fn start_download(
    app: tauri::AppHandle,
    magnet: String,
    dest: String,
    id: String,
    title: String,
    // Indices of the torrent's files to download (from list_torrent_files);
    // None/empty = all files.
    only_files: Option<Vec<usize>>,
    // Canonical renames to apply once the download finishes (from planRename).
    renames: Option<Vec<RenamePair>>,
) -> Result<(), String> {
    // Return immediately and do the heavy work in the background so the UI never
    // blocks: add_torrent resolves the magnet's metadata (and lazily boots the
    // session — DHT/UPnP — on the first download), which takes seconds and can
    // stall for a low-seeder magnet. The frontend already shows an optimistic
    // entry; any failure comes back as a "download-progress" event (state="error").
    tauri::async_runtime::spawn(async move {
        let out = match resolve_out_dir(&app, &dest) {
            Ok(out) => out,
            Err(e) => {
                emit_dl_error(&app, &id, &title, &dest, e);
                return;
            }
        };
        let dest_out = out.to_string_lossy().into_owned();

        // Only the user-picked file indices (None/empty -> all files). The custom
        // lazy storage never writes (so never creates) the deselected files.
        let only: Vec<usize> = only_files.unwrap_or_default();
        let (torrent_id, handle) = match dl::add(&magnet, &dest_out, &only).await {
            Ok(x) => x,
            Err(e) => {
                emit_dl_error(&app, &id, &title, &dest_out, format!("{e:#}"));
                return;
            }
        };

        // Register for management (pause/resume/cancel) before polling.
        dl_map(&app).insert(
            id.clone(),
            DlEntry {
                handle: handle.clone(),
                torrent_id,
                title: title.clone(),
                dest: dest_out.clone(),
            },
        );

        loop {
            // A missing entry means the download was cancelled: stop quietly.
            if !dl_map(&app).contains_key(&id) {
                break;
            }
            let payload = dl_progress(&id, &handle, &title, &dest_out);
            let done = payload.state == DL_STATE_DONE;
            // Re-check membership right before emitting in case a cancel raced us.
            if !dl_map(&app).contains_key(&id) {
                break;
            }
            if done {
                apply_renames(&app, &id, &dest_out, renames.as_ref()).await;
            }
            let _ = app.emit("download-progress", payload);
            if done {
                // Done: entry stays in the map unless the rename branch above removed it.
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
        }
    });
    Ok(())
}

/// One file inside a torrent, for the download file-picker.
#[derive(Clone, serde::Serialize)]
struct TorrentFile {
    index: usize,
    /// Path within the torrent (slash-joined for multi-file torrents).
    name: String,
    size: u64,
}

impl From<dl::FileEntry> for TorrentFile {
    fn from(f: dl::FileEntry) -> Self {
        TorrentFile { index: f.index, name: f.name, size: f.size }
    }
}

/// Set the session-wide max download/upload speed. Inputs are KiB/s from the UI
/// (0 = unlimited); converted to bytes/sec for librqbit.
#[tauri::command]
async fn set_rate_limits(download_kib: i64, upload_kib: i64) -> Result<(), String> {
    let to_bps = |kib: i64| -> u32 {
        if kib <= 0 {
            0 // unlimited
        } else {
            kib.saturating_mul(1024).min(u32::MAX as i64) as u32
        }
    };
    dl::set_rate_limits(to_bps(download_kib), to_bps(upload_kib))
        .await
        .map_err(|e| e.to_string())
}

/// Resolve a torrent's file list WITHOUT downloading the data (metadata-only),
/// so the UI can let the user choose which files to fetch. For a bare magnet the
/// metadata must be fetched from peers/DHT, so
/// the shim is bounded by a timeout; it BLOCKS, hence spawn_blocking.
#[tauri::command]
async fn list_torrent_files(magnet: String) -> Result<Vec<TorrentFile>, String> {
    let files = dl::list_files(&magnet, METADATA_FETCH_TIMEOUT_MS)
        .await
        .map_err(|e| e.to_string())?;
    if files.is_empty() {
        return Err("timed out reading torrent metadata (no peers?)".to_string());
    }
    Ok(files.into_iter().map(TorrentFile::from).collect())
}

/// Write the magnet's .torrent file to `out_path` (metadata only — no content is
/// downloaded). Awaits the metadata fetch from peers.
#[tauri::command]
async fn save_torrent(magnet: String, out_path: String) -> Result<(), String> {
    dl::save_torrent(&magnet, &out_path, METADATA_FETCH_TIMEOUT_MS)
        .await
        .map_err(|e| e.to_string())
}

// ------------------------------------------------------------------ os integration
/// Move a file or folder to the macOS Trash / Windows Recycle Bin.
#[tauri::command]
async fn trash_delete(path: String) -> Result<(), String> {
    let p = PathBuf::from(&path);
    if !p.is_absolute() {
        return Err(format!("path must be absolute: {path}"));
    }
    if !p.exists() {
        return Err(format!("path does not exist: {path}"));
    }
    tauri::async_runtime::spawn_blocking(move || trash::delete(&p).map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

const VIDEO_EXTS: &[&str] = &[
    "mkv", "mp4", "avi", "wmv", "m4v", "ts", "mov", "flv", "iso", "rmvb", "webm", "mpg", "mpeg",
];
const SCAN_CAP: usize = 5000;

#[derive(Clone, serde::Serialize)]
struct ScanFile {
    name: String,
    path: String,
    size: u64,
}

fn walk_videos(dir: &Path, out: &mut Vec<ScanFile>) {
    if out.len() >= SCAN_CAP {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return, // unreadable dir: skip, keep scanning the rest
    };
    for entry in entries.flatten() {
        if out.len() >= SCAN_CAP {
            return;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue; // skip dotfiles and dot-directories
        }
        let ft = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        let path = entry.path();
        if ft.is_dir() {
            walk_videos(&path, out);
        } else if ft.is_file() {
            let is_video = path
                .extension()
                .map(|e| VIDEO_EXTS.contains(&e.to_string_lossy().to_lowercase().as_str()))
                .unwrap_or(false);
            if is_video {
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                out.push(ScanFile {
                    name,
                    path: path.to_string_lossy().into_owned(),
                    size,
                });
            }
        }
    }
}

/// Recursively list video files under `path` (native replacement for sidecar /scan).
#[tauri::command]
async fn scan_videos(path: String) -> Result<Vec<ScanFile>, String> {
    let root = PathBuf::from(&path);
    if !root.is_absolute() {
        return Err(format!("path must be absolute: {path}"));
    }
    if !root.is_dir() {
        return Err(format!("not a directory: {path}"));
    }
    tauri::async_runtime::spawn_blocking(move || {
        let mut out = Vec::new();
        walk_videos(&root, &mut out);
        out
    })
    .await
    .map_err(|e| e.to_string())
}

#[derive(Clone, serde::Serialize)]
struct DiskStats {
    free: u64,
    total: u64,
}

#[cfg(unix)]
fn volume_stats(path: &Path) -> Result<DiskStats, String> {
    let vfs = rustix::fs::statvfs(path).map_err(|e| e.to_string())?;
    let frsize = if vfs.f_frsize > 0 { vfs.f_frsize } else { vfs.f_bsize };
    Ok(DiskStats {
        free: vfs.f_bavail.saturating_mul(frsize),
        total: vfs.f_blocks.saturating_mul(frsize),
    })
}

#[cfg(windows)]
fn volume_stats(path: &Path) -> Result<DiskStats, String> {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut free: u64 = 0;
    let mut total: u64 = 0;
    let mut total_free: u64 = 0;
    let ok = unsafe {
        windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free,
            &mut total,
            &mut total_free,
        )
    };
    if ok == 0 {
        return Err(format!(
            "GetDiskFreeSpaceExW failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(DiskStats { free, total })
}

/// Free/total bytes of the volume containing `path` (nearest existing ancestor is probed).
#[tauri::command]
async fn disk_stats(path: String) -> Result<DiskStats, String> {
    let p = PathBuf::from(&path);
    if !p.is_absolute() {
        return Err(format!("path must be absolute: {path}"));
    }
    // Walk up to an existing ancestor so a configured-but-not-yet-created folder still works.
    let mut probe = p.as_path();
    while !probe.exists() {
        probe = probe
            .parent()
            .ok_or_else(|| format!("no existing ancestor for: {path}"))?;
    }
    let probe = probe.to_path_buf();
    tauri::async_runtime::spawn_blocking(move || volume_stats(&probe))
        .await
        .map_err(|e| e.to_string())?
}

// ------------------------------------------------------------------ macos titlebar
/// Shift the traffic lights so the close button's left edge sits at x = 18
/// logical px, aligned over the sidebar brand badge (titleBarStyle Overlay
/// leaves them at the ~8px system inset). Keeps the system y and inter-button
/// spacing. macOS resets the buttons on fullscreen/theme/resize, so this is
/// re-applied from window events; it's idempotent and just three frame moves.
#[cfg(target_os = "macos")]
fn align_traffic_lights(window: &tauri::WebviewWindow) {
    // Shift the buttons right (more left margin) and down (more top margin) from
    // macOS's default. The move is RELATIVE to the buttons' own (valid) frame —
    // unlike absolute window-height math, which read the wrong (titlebar-sized)
    // superview height and silently did nothing. An idempotent guard (skip if x
    // is already at our inset) stops the y offset from compounding on re-applies.
    const INSET_X: f64 = 24.0;
    const DOWN_Y: f64 = 14.0;
    let win = window.clone();
    // AppKit is main-thread-only.
    let _ = window.run_on_main_thread(move || unsafe {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        use objc2_foundation::{NSPoint, NSRect};

        let Ok(ns_window) = win.ns_window() else { return };
        let ns_window = ns_window as *mut AnyObject;
        // standardWindowButton: NSWindowButton close = 0, miniaturize = 1, zoom = 2.
        let close: *mut AnyObject = msg_send![ns_window, standardWindowButton: 0usize];
        let mini: *mut AnyObject = msg_send![ns_window, standardWindowButton: 1usize];
        let zoom: *mut AnyObject = msg_send![ns_window, standardWindowButton: 2usize];
        // Buttons can be null before the window is shown — bail silently.
        if close.is_null() || mini.is_null() || zoom.is_null() {
            return;
        }
        let close_f: NSRect = msg_send![close, frame];
        // Idempotency: if x is already at our inset, macOS hasn't reset the
        // buttons since our last apply — skip so the relative y move can't drift.
        if (close_f.origin.x - INSET_X).abs() < 1.0 {
            return;
        }
        let mini_f: NSRect = msg_send![mini, frame];
        let spacing = mini_f.origin.x - close_f.origin.x; // system default (~20)
        let new_y = close_f.origin.y - DOWN_Y; // origin bottom-left: subtract = down
        for (i, btn) in [close, mini, zoom].into_iter().enumerate() {
            let origin = NSPoint::new(INSET_X + spacing * i as f64, new_y);
            let _: () = msg_send![btn, setFrameOrigin: origin];
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_sql::Builder::default().build())
        .manage(DlManager(Mutex::new(HashMap::new())))
        .setup(|app| {
            // Initial traffic-light alignment for the main window (macOS only).
            #[cfg(target_os = "macos")]
            if let Some(win) = app.get_webview_window("main") {
                align_traffic_lights(&win);
            }
            #[cfg(not(target_os = "macos"))]
            let _ = &app;
            Ok(())
        })
        .on_window_event(|window, event| {
            // macOS resets the standard buttons on these state changes; re-apply.
            #[cfg(target_os = "macos")]
            if matches!(
                event,
                tauri::WindowEvent::Resized(_)
                    | tauri::WindowEvent::ThemeChanged(_)
                    | tauri::WindowEvent::Focused(true)
            ) {
                if let Some(win) = window.get_webview_window(window.label()) {
                    align_traffic_lights(&win);
                }
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (window, event);
        })
        .invoke_handler(tauri::generate_handler![
            start_download,
            list_torrent_files,
            save_torrent,
            pause_download,
            resume_download,
            cancel_download,
            set_rate_limits,
            trash_delete,
            scan_videos,
            disk_stats
        ])
        .run(tauri::generate_context!())
        .expect("error while running auto-video");
}
