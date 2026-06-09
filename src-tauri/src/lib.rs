use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};
use tokio::sync::Mutex as AsyncMutex;

/// The Python sidecar process (stopped when the app closes).
struct SidecarProc(Mutex<Option<Child>>);
/// The librqbit BitTorrent session (created lazily on first download).
struct DlSession(AsyncMutex<Option<Arc<librqbit::Session>>>);

const SIDECAR_PORT: &str = "8902";

// ------------------------------------------------------------------ sidecar
fn python_candidates() -> &'static [&'static str] {
    if cfg!(windows) { &["python", "py", "python3"] } else { &["python3", "python"] }
}
fn sidecar_script(app: &tauri::App) -> PathBuf {
    if let Ok(res) = app.path().resource_dir() {
        let p = res.join("sidecar").join("av_proxy.py");
        if p.exists() {
            return p;
        }
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("sidecar")
        .join("av_proxy.py")
}
fn spawn_sidecar(script: &Path) -> std::io::Result<Child> {
    let mut last_err: Option<std::io::Error> = None;
    for exe in python_candidates() {
        let mut cmd = Command::new(exe);
        cmd.arg(script).arg(SIDECAR_PORT);
        if let Some(dir) = script.parent() {
            cmd.current_dir(dir);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        match cmd.spawn() {
            Ok(child) => return Ok(child),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "python not found")))
}

// ------------------------------------------------------------------ downloads
#[derive(Clone, serde::Serialize)]
struct DlProgress {
    id: String,
    title: String,
    progress: f64, // 0..1
    speed_mbps: f64,
    state: String, // "downloading" | "done"
}

#[tauri::command]
async fn start_download(
    app: tauri::AppHandle,
    sess: tauri::State<'_, DlSession>,
    magnet: String,
    dest: String,
    id: String,
    title: String,
) -> Result<(), String> {
    // Use the requested folder if it's usable; otherwise fall back to Downloads/auto-video.
    let out = {
        let mut chosen: Option<PathBuf> = None;
        if !dest.trim().is_empty() {
            let p = PathBuf::from(&dest);
            if std::fs::create_dir_all(&p).is_ok() {
                chosen = Some(p);
            }
        }
        match chosen {
            Some(p) => p,
            None => {
                let p = app
                    .path()
                    .download_dir()
                    .map_err(|e| e.to_string())?
                    .join("auto-video");
                std::fs::create_dir_all(&p).map_err(|e| e.to_string())?;
                p
            }
        }
    };

    let session = {
        let mut g = sess.0.lock().await;
        if g.is_none() {
            let s = librqbit::Session::new(out.clone())
                .await
                .map_err(|e| e.to_string())?;
            *g = Some(s);
        }
        g.as_ref().unwrap().clone()
    };

    let opts = librqbit::AddTorrentOptions {
        output_folder: Some(out.to_string_lossy().into_owned()),
        overwrite: true,
        ..Default::default()
    };
    let resp = session
        .add_torrent(librqbit::AddTorrent::from_url(&magnet), Some(opts))
        .await
        .map_err(|e| e.to_string())?;
    let handle = resp.into_handle().ok_or_else(|| "no torrent handle".to_string())?;

    tauri::async_runtime::spawn(async move {
        loop {
            let st = handle.stats();
            let total = st.total_bytes.max(1);
            let prog = (st.progress_bytes as f64 / total as f64).clamp(0.0, 1.0);
            let done = st.finished;
            let speed = st
                .live
                .as_ref()
                .map(|l| l.download_speed.mbps)
                .unwrap_or(0.0);
            let _ = app.emit(
                "download-progress",
                DlProgress {
                    id: id.clone(),
                    title: title.clone(),
                    progress: prog,
                    speed_mbps: speed,
                    state: if done { "done".into() } else { "downloading".into() },
                },
            );
            if done {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
        }
    });
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(SidecarProc(Mutex::new(None)))
        .manage(DlSession(AsyncMutex::new(None)))
        .invoke_handler(tauri::generate_handler![start_download])
        .setup(|app| {
            let script = sidecar_script(app);
            match spawn_sidecar(&script) {
                Ok(child) => {
                    println!("[auto-video] sidecar started: {}", script.display());
                    *app.state::<SidecarProc>().0.lock().unwrap() = Some(child);
                }
                Err(e) => eprintln!(
                    "[auto-video] failed to start sidecar ({}): {}",
                    script.display(),
                    e
                ),
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                if let Some(state) = window.app_handle().try_state::<SidecarProc>() {
                    if let Some(mut child) = state.0.lock().unwrap().take() {
                        let _ = child.kill();
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running auto-video");
}
