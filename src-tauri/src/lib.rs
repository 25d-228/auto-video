use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};
use tokio::sync::Mutex as AsyncMutex;

/// The librqbit BitTorrent session (created lazily on first download).
struct DlSession(AsyncMutex<Option<Arc<librqbit::Session>>>);

/// One managed download: the librqbit handle plus the metadata every event carries.
struct DlEntry {
    handle: Arc<librqbit::ManagedTorrent>,
    title: String,
    dest: String,
}
/// id -> managed download. Keeps torrent handles reachable so they can be
/// paused/resumed/cancelled; removal from this map is what stops the polling loop.
struct DlManager(Mutex<HashMap<String, DlEntry>>);

// ------------------------------------------------------------------ downloads
/// The `state` value the frontend treats as "finished"; also the literal the
/// polling loop watches for. Spelled once so it can't drift between producers.
const DL_STATE_DONE: &str = "done";

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

/// Snapshot the torrent's current stats into a contract-shaped event payload.
fn dl_progress(id: &str, title: &str, dest: &str, handle: &Arc<librqbit::ManagedTorrent>) -> DlProgress {
    use librqbit::TorrentStatsState as S;
    let st = handle.stats();
    let total = st.total_bytes.max(1);
    let progress = (st.progress_bytes as f64 / total as f64).clamp(0.0, 1.0);
    // st.live is None while paused/initializing/errored, so speed naturally reads 0 then.
    let speed_mbps = st
        .live
        .as_ref()
        .map(|l| l.download_speed.mbps)
        .unwrap_or(0.0);
    let state = match st.state {
        S::Error => "error",
        S::Paused => "paused",
        _ if st.finished => DL_STATE_DONE,
        _ => "downloading", // Initializing | Live
    };
    DlProgress {
        id: id.to_owned(),
        title: title.to_owned(),
        progress,
        speed_mbps,
        state: state.to_owned(),
        dest: dest.to_owned(),
        error: st.error,
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

/// Clone the managed entry for `id`, or explain that it isn't managed.
fn dl_entry(app: &tauri::AppHandle, id: &str) -> Result<(Arc<librqbit::ManagedTorrent>, String, String), String> {
    let mgr = app.state::<DlManager>();
    let g = mgr.0.lock().unwrap();
    g.get(id)
        .map(|e| (e.handle.clone(), e.title.clone(), e.dest.clone()))
        .ok_or_else(|| format!("unknown download id: {id}"))
}

/// The lazily-created session, or an error if no download ever started.
async fn current_session(sess: &DlSession) -> Result<Arc<librqbit::Session>, String> {
    sess.0
        .lock()
        .await
        .clone()
        .ok_or_else(|| "no active download session".to_string())
}

#[tauri::command]
async fn pause_download(
    app: tauri::AppHandle,
    sess: tauri::State<'_, DlSession>,
    id: String,
) -> Result<(), String> {
    let (handle, title, dest) = dl_entry(&app, &id)?;
    let session = current_session(&sess).await?;
    session.pause(&handle).await.map_err(|e| e.to_string())?;
    // Immediate event on state change (stats now report Paused).
    let _ = app.emit("download-progress", dl_progress(&id, &title, &dest, &handle));
    Ok(())
}

#[tauri::command]
async fn resume_download(
    app: tauri::AppHandle,
    sess: tauri::State<'_, DlSession>,
    id: String,
) -> Result<(), String> {
    let (handle, title, dest) = dl_entry(&app, &id)?;
    let session = current_session(&sess).await?;
    session.unpause(&handle).await.map_err(|e| e.to_string())?;
    // Immediate event on state change (stats now report Initializing/Live).
    let _ = app.emit("download-progress", dl_progress(&id, &title, &dest, &handle));
    Ok(())
}

#[tauri::command]
async fn cancel_download(
    app: tauri::AppHandle,
    sess: tauri::State<'_, DlSession>,
    id: String,
    delete_files: bool,
) -> Result<(), String> {
    let session = current_session(&sess).await?;
    // Remove from the map first: the polling loop checks membership before every
    // emit, so no further events fire for this id once the entry is gone.
    let entry = app
        .state::<DlManager>()
        .0
        .lock()
        .unwrap()
        .remove(&id)
        .ok_or_else(|| format!("unknown download id: {id}"))?;
    let torrent_id = librqbit::api::TorrentIdOrHash::Id(entry.handle.id());
    if let Err(e) = session.delete(torrent_id, delete_files).await {
        // Deletion failed: restore the entry so the download stays manageable.
        app.state::<DlManager>().0.lock().unwrap().insert(id, entry);
        return Err(e.to_string());
    }
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

/// The shared librqbit session, creating it on the first download of this run.
async fn get_or_init_session(
    app: &tauri::AppHandle,
    out: &Path,
) -> Result<Arc<librqbit::Session>, String> {
    let sess = app.state::<DlSession>();
    let mut g = sess.0.lock().await;
    if g.is_none() {
        let s = librqbit::Session::new(out.to_path_buf())
            .await
            .map_err(|e| e.to_string())?;
        *g = Some(s);
    }
    Ok(g.as_ref().unwrap().clone())
}

/// Canonicalize the downloaded file names once a download finishes: stop the
/// torrent so librqbit releases the files, rename per the plan, then drop the
/// management entry. A no-op (and leaves the entry) when there are no renames.
async fn apply_renames(app: &tauri::AppHandle, id: &str, dest_out: &str, renames: Option<&Vec<RenamePair>>) {
    let Some(plan) = renames.filter(|p| !p.is_empty()) else { return };
    let session = app.state::<DlSession>().0.lock().await.clone();
    let torrent_id = app
        .state::<DlManager>()
        .0
        .lock()
        .unwrap()
        .get(id)
        .map(|e| e.handle.id());
    if let (Some(session), Some(torrent_id)) = (session, torrent_id) {
        let _ = session
            .delete(librqbit::api::TorrentIdOrHash::Id(torrent_id), false)
            .await;
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
    app.state::<DlManager>().0.lock().unwrap().remove(id);
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

        let session = match get_or_init_session(&app, &out).await {
            Ok(s) => s,
            Err(e) => {
                emit_dl_error(&app, &id, &title, &dest_out, e);
                return;
            }
        };

        let opts = librqbit::AddTorrentOptions {
            output_folder: Some(dest_out.clone()),
            overwrite: true,
            // Only the user-picked files (None/empty -> everything).
            only_files: only_files.filter(|v| !v.is_empty()),
            ..Default::default()
        };
        let handle = match session
            .add_torrent(librqbit::AddTorrent::from_url(&magnet), Some(opts))
            .await
        {
            Ok(resp) => match resp.into_handle() {
                Some(h) => h,
                None => {
                    emit_dl_error(&app, &id, &title, &dest_out, "no torrent handle".into());
                    return;
                }
            },
            Err(e) => {
                emit_dl_error(&app, &id, &title, &dest_out, e.to_string());
                return;
            }
        };

        // Register the handle for management (pause/resume/cancel) before polling.
        app.state::<DlManager>().0.lock().unwrap().insert(
            id.clone(),
            DlEntry {
                handle,
                title: title.clone(),
                dest: dest_out.clone(),
            },
        );

        loop {
            // Read the handle through the map each tick; a missing entry means the
            // download was cancelled, so exit without emitting anything more.
            let handle = {
                let mgr = app.state::<DlManager>();
                let g = mgr.0.lock().unwrap();
                match g.get(&id) {
                    Some(e) => e.handle.clone(),
                    None => break,
                }
            };
            let payload = dl_progress(&id, &title, &dest_out, &handle);
            let done = payload.state == DL_STATE_DONE;
            // Re-check membership right before emitting in case a cancel raced us.
            if !app.state::<DlManager>().0.lock().unwrap().contains_key(&id) {
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

/// Resolve a torrent's file list WITHOUT downloading (librqbit list_only), so
/// the UI can let the user choose which files to fetch. For a .torrent the
/// metadata is embedded (instant); for a bare magnet it must be fetched from
/// peers/DHT, so this is bounded by a timeout.
#[tauri::command]
async fn list_torrent_files(
    app: tauri::AppHandle,
    magnet: String,
) -> Result<Vec<TorrentFile>, String> {
    // Reuse (or lazily create) the session; the output folder is irrelevant here.
    let session = {
        let sess = app.state::<DlSession>();
        let mut g = sess.0.lock().await;
        if g.is_none() {
            let base = app
                .path()
                .download_dir()
                .map_err(|e| e.to_string())?
                .join("auto-video");
            let _ = std::fs::create_dir_all(&base);
            let s = librqbit::Session::new(base).await.map_err(|e| e.to_string())?;
            *g = Some(s);
        }
        g.as_ref().unwrap().clone()
    };
    let opts = librqbit::AddTorrentOptions {
        list_only: true,
        ..Default::default()
    };
    let fut = session.add_torrent(librqbit::AddTorrent::from_url(&magnet), Some(opts));
    let resp = tokio::time::timeout(std::time::Duration::from_secs(45), fut)
        .await
        .map_err(|_| "timed out reading torrent metadata (no peers?)".to_string())?
        .map_err(|e| e.to_string())?;
    match resp {
        librqbit::AddTorrentResponse::ListOnly(r) => {
            let details = r.info.iter_file_details().map_err(|e| e.to_string())?;
            Ok(details
                .enumerate()
                .map(|(index, d)| TorrentFile {
                    index,
                    name: d.filename.to_string().unwrap_or_else(|_| format!("file {index}")),
                    size: d.len,
                })
                .collect())
        }
        _ => Err("unexpected response while listing files".to_string()),
    }
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
        .manage(DlSession(AsyncMutex::new(None)))
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
            pause_download,
            resume_download,
            cancel_download,
            trash_delete,
            scan_videos,
            disk_stats
        ])
        .run(tauri::generate_context!())
        .expect("error while running auto-video");
}

// ------------------------------------------------------------------ download tests
// Offline, deterministic checks of the librqbit behaviour the download commands
// rely on: (1) re-adding a torrent over existing on-disk files RESUMES from them
// (the core of "continue after restart"); (2) cancel-with-delete removes files
// (the core of "abort"). No network: DHT + trackers are disabled and the file is
// already complete on disk, so the initial hash-check finishes immediately.
#[cfg(test)]
mod download_tests {
    use super::*;
    use std::io::Write;

    fn fresh_dir(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("av-dltest-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn write_file(path: &Path, byte: u8, len: usize) {
        let mut f = std::fs::File::create(path).unwrap();
        let chunk = vec![byte; 64 * 1024];
        let mut left = len;
        while left > 0 {
            let n = left.min(chunk.len());
            f.write_all(&chunk[..n]).unwrap();
            left -= n;
        }
        f.flush().unwrap();
    }

    async fn offline_session(dir: &Path) -> Arc<librqbit::Session> {
        librqbit::Session::new_with_opts(
            dir.to_path_buf(),
            librqbit::SessionOptions {
                disable_dht: true,
                persistence: None,
                ..Default::default()
            },
        )
        .await
        .unwrap()
    }

    async fn add_existing(
        session: &Arc<librqbit::Session>,
        dir: &Path,
    ) -> Arc<librqbit::ManagedTorrent> {
        let file = dir.join("data.bin");
        let res = librqbit::create_torrent(
            &file,
            librqbit::CreateTorrentOptions { name: None, piece_length: Some(65536) },
        )
        .await
        .unwrap();
        let bytes = res.as_bytes().unwrap();
        let opts = librqbit::AddTorrentOptions {
            output_folder: Some(dir.to_string_lossy().into_owned()),
            overwrite: true,
            disable_trackers: true,
            ..Default::default()
        };
        session
            .add_torrent(librqbit::AddTorrent::TorrentFileBytes(bytes), Some(opts))
            .await
            .unwrap()
            .into_handle()
            .unwrap()
    }

    /// Re-adding a torrent whose output folder already holds the data resumes to
    /// 100% from disk (no peers) — the mechanism that lets a download continue.
    #[tokio::test]
    async fn readd_over_existing_files_resumes_to_complete() {
        let dir = fresh_dir("resume");
        write_file(&dir.join("data.bin"), 0xAB, 2 * 1024 * 1024);
        let session = offline_session(&dir).await;
        let handle = add_existing(&session, &dir).await;

        let mut finished = false;
        for _ in 0..100 {
            if handle.stats().finished {
                finished = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let st = handle.stats();
        assert!(finished, "did not resume from disk: {}/{}", st.progress_bytes, st.total_bytes);
        assert_eq!(st.progress_bytes, st.total_bytes);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// list_only resolves a multi-file torrent's file list without downloading —
    /// the data behind the download file-picker.
    #[tokio::test]
    async fn list_only_returns_the_file_list() {
        let dir = fresh_dir("list");
        write_file(&dir.join("data1.bin"), 0x11, 128 * 1024);
        write_file(&dir.join("data2.bin"), 0x22, 256 * 1024);
        let res = librqbit::create_torrent(
            &dir, // a directory -> multi-file torrent
            librqbit::CreateTorrentOptions { name: None, piece_length: Some(65536) },
        )
        .await
        .unwrap();
        let bytes = res.as_bytes().unwrap();
        let session = offline_session(&dir).await;
        let opts = librqbit::AddTorrentOptions {
            list_only: true,
            disable_trackers: true,
            ..Default::default()
        };
        let resp = session
            .add_torrent(librqbit::AddTorrent::TorrentFileBytes(bytes), Some(opts))
            .await
            .unwrap();
        match resp {
            librqbit::AddTorrentResponse::ListOnly(r) => {
                let names: Vec<String> = r
                    .info
                    .iter_file_details()
                    .unwrap()
                    .map(|d| d.filename.to_string().unwrap())
                    .collect();
                assert_eq!(names.len(), 2, "expected 2 files, got {names:?}");
                assert!(names.iter().any(|n| n.contains("data1.bin")));
                assert!(names.iter().any(|n| n.contains("data2.bin")));
            }
            _ => panic!("expected ListOnly response"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// cancel_download(delete_files = true) removes the torrent's files from disk.
    #[tokio::test]
    async fn cancel_with_delete_removes_files() {
        let dir = fresh_dir("delete");
        let file = dir.join("data.bin");
        write_file(&file, 0xCD, 256 * 1024);
        let session = offline_session(&dir).await;
        let handle = add_existing(&session, &dir).await;
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        let id = librqbit::api::TorrentIdOrHash::Id(handle.id());
        session.delete(id, true).await.unwrap();
        assert!(!file.exists(), "file should be gone after cancel(delete=true)");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
