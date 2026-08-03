#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::process::{Command, Stdio};

use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

const MOVIES_FOLDER_FILE_NAME: &str = ".movies-folder";
const MOVIES_FOLDER_UNAVAILABLE: &str = "movies_folder_unavailable";
const MOVIES_FOLDER_STORAGE_FAILED: &str = "movies_folder_storage_failed";
const MOVIES_STORAGE_FAILED: &str = "movies_storage_failed";
const MOVIES_STORAGE_UNAVAILABLE: &str = "movies_storage_unavailable";
const MOVIES_SCAN_FAILED: &str = "movies_scan_failed";
const MOVIE_OPEN_FAILED: &str = "movie_open_failed";
const MOVIE_OPEN_NOT_FILE: &str = "movie_open_not_file";
const MOVIE_OPEN_NOT_FOUND: &str = "movie_open_not_found";
const MOVIE_OPEN_UNAVAILABLE: &str = "movie_open_unavailable";
const MOVIE_OPEN_UNSUPPORTED: &str = "movie_open_unsupported";
const MOVIE_REVEAL_FAILED: &str = "movie_reveal_failed";
const MOVIE_REVEAL_NOT_FILE: &str = "movie_reveal_not_file";
const MOVIE_REVEAL_NOT_FOUND: &str = "movie_reveal_not_found";
const MOVIE_REVEAL_UNAVAILABLE: &str = "movie_reveal_unavailable";
const MOVIE_REVEAL_UNSUPPORTED: &str = "movie_reveal_unsupported";
const MOVIE_TRASH_FAILED: &str = "movie_trash_failed";
const MOVIE_TRASH_FOLDER_UNAVAILABLE: &str = "movie_trash_folder_unavailable";
const MOVIE_TRASH_NOT_FILE: &str = "movie_trash_not_file";
const MOVIE_TRASH_NOT_FOUND: &str = "movie_trash_not_found";
const MOVIE_TRASH_OUTSIDE_FOLDER: &str = "movie_trash_outside_folder";
const MOVIE_TRASH_STALE: &str = "movie_trash_stale";
const MOVIE_TRASH_UNAVAILABLE: &str = "movie_trash_unavailable";
const MOVIE_TRASH_UNSUPPORTED: &str = "movie_trash_unsupported";
const TMDB_TOKEN_FILE_NAME: &str = ".tmdb-api-read-access-token";
const TMDB_TOKEN_INVALID: &str = "tmdb_token_invalid";
const TMDB_TOKEN_STORAGE_FAILED: &str = "tmdb_token_storage_failed";
// API Read Access Tokens are much shorter; this rejects arbitrary oversized IPC or file input.
const TMDB_TOKEN_MAX_LENGTH: usize = 4096;

#[derive(Default)]
struct MoviesLibraryContext {
    folder: Option<PathBuf>,
    movie_paths: Vec<String>,
}

#[derive(Clone, Default)]
struct MoviesLibraryState(Arc<Mutex<MoviesLibraryContext>>);

struct TrashMovieRequest {
    path: String,
    folder: Option<String>,
    library_paths: Option<Vec<String>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum MoviesVolumeStorageQueryError {
    Unavailable,
    Failed,
}

fn query_movies_volume_storage_with(
    folder: Option<&Path>,
    query: impl FnOnce(&Path) -> Result<[u64; 2], MoviesVolumeStorageQueryError>,
) -> Result<[u64; 2], &'static str> {
    let folder = folder.ok_or(MOVIES_STORAGE_UNAVAILABLE)?;
    let metadata = fs::metadata(folder).map_err(|_| MOVIES_STORAGE_UNAVAILABLE)?;
    if !metadata.is_dir() {
        return Err(MOVIES_STORAGE_UNAVAILABLE);
    }

    let [total_bytes, free_bytes] = query(folder).map_err(|error| match error {
        MoviesVolumeStorageQueryError::Unavailable => MOVIES_STORAGE_UNAVAILABLE,
        MoviesVolumeStorageQueryError::Failed => MOVIES_STORAGE_FAILED,
    })?;
    if total_bytes == 0 || free_bytes > total_bytes {
        return Err(MOVIES_STORAGE_FAILED);
    }

    Ok([total_bytes, free_bytes])
}

#[cfg(target_os = "macos")]
fn parse_macos_volume_storage(output: &[u8]) -> Result<[u64; 2], MoviesVolumeStorageQueryError> {
    let output = std::str::from_utf8(output).map_err(|_| MoviesVolumeStorageQueryError::Failed)?;
    let values = output
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.split_whitespace().collect::<Vec<_>>())
        .filter(|fields| fields.len() >= 4)
        .ok_or(MoviesVolumeStorageQueryError::Failed)?;
    const BYTES_PER_DF_BLOCK: u64 = 1024;
    let total_bytes = values[1]
        .parse::<u64>()
        .ok()
        .and_then(|blocks| blocks.checked_mul(BYTES_PER_DF_BLOCK))
        .ok_or(MoviesVolumeStorageQueryError::Failed)?;
    let free_bytes = values[3]
        .parse::<u64>()
        .ok()
        .and_then(|blocks| blocks.checked_mul(BYTES_PER_DF_BLOCK))
        .ok_or(MoviesVolumeStorageQueryError::Failed)?;

    Ok([total_bytes, free_bytes])
}

#[cfg(target_os = "macos")]
fn query_movies_volume_storage(folder: &Path) -> Result<[u64; 2], MoviesVolumeStorageQueryError> {
    // POSIX output keeps the selected volume on one row even when its mount path contains spaces.
    let output = Command::new("/bin/df")
        .args(["-k", "-P"])
        .arg("--")
        .arg(folder)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| MoviesVolumeStorageQueryError::Failed)?;
    if !output.status.success() {
        return Err(MoviesVolumeStorageQueryError::Unavailable);
    }

    parse_macos_volume_storage(&output.stdout)
}

#[cfg(target_os = "windows")]
fn parse_windows_volume_storage(output: &[u8]) -> Result<[u64; 2], MoviesVolumeStorageQueryError> {
    let output = std::str::from_utf8(output).map_err(|_| MoviesVolumeStorageQueryError::Failed)?;
    let mut values = output.split_whitespace();
    let total_bytes = values
        .next()
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or(MoviesVolumeStorageQueryError::Failed)?;
    let free_bytes = values
        .next()
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or(MoviesVolumeStorageQueryError::Failed)?;
    if values.next().is_some() {
        return Err(MoviesVolumeStorageQueryError::Failed);
    }

    Ok([total_bytes, free_bytes])
}

#[cfg(target_os = "windows")]
fn query_movies_volume_storage(folder: &Path) -> Result<[u64; 2], MoviesVolumeStorageQueryError> {
    const WINDOWS_MOVIES_FOLDER_ENV: &str = "AUTO_VIDEO_MOVIES_FOLDER";
    const WINDOWS_VOLUME_STORAGE_SCRIPT: &str = r#"$ErrorActionPreference = 'Stop'
$root = [System.IO.Path]::GetPathRoot($env:AUTO_VIDEO_MOVIES_FOLDER)
if ([string]::IsNullOrEmpty($root)) { throw 'Movies volume root is unavailable.' }
$volume = [System.IO.DriveInfo]::new($root)
$culture = [Globalization.CultureInfo]::InvariantCulture
[Console]::Out.WriteLine($volume.TotalSize.ToString($culture) + ' ' + $volume.AvailableFreeSpace.ToString($culture))"#;
    // Passing the trusted path through the environment avoids treating path text as script input.
    let output = Command::new("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
        .arg(WINDOWS_VOLUME_STORAGE_SCRIPT)
        .env(WINDOWS_MOVIES_FOLDER_ENV, folder.as_os_str())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| MoviesVolumeStorageQueryError::Failed)?;
    if !output.status.success() {
        return Err(MoviesVolumeStorageQueryError::Unavailable);
    }

    parse_windows_volume_storage(&output.stdout)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn query_movies_volume_storage(_folder: &Path) -> Result<[u64; 2], MoviesVolumeStorageQueryError> {
    Err(MoviesVolumeStorageQueryError::Failed)
}

fn collect_movie_paths(directory: &Path, movie_paths: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();

        if file_type.is_dir() {
            collect_movie_paths(&path, movie_paths)?;
        } else if file_type.is_file() && is_supported_movie(&path) {
            movie_paths.push(path);
        }
    }

    Ok(())
}

fn is_supported_movie(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("mp4") || extension.eq_ignore_ascii_case("mkv")
        })
}

fn scan_movie_paths(folder: &Path) -> Result<Vec<String>, &'static str> {
    let metadata = fs::metadata(folder).map_err(|_| MOVIES_FOLDER_UNAVAILABLE)?;
    if !metadata.is_dir() {
        return Err(MOVIES_FOLDER_UNAVAILABLE);
    }

    let mut movie_paths = Vec::new();
    collect_movie_paths(folder, &mut movie_paths).map_err(|_| MOVIES_SCAN_FAILED)?;
    movie_paths.sort();

    movie_paths
        .into_iter()
        .map(|path| {
            path.into_os_string()
                .into_string()
                .map_err(|_| MOVIES_SCAN_FAILED)
        })
        .collect()
}

fn load_movies_folder_file(path: &Path) -> Result<Option<PathBuf>, &'static str> {
    match fs::read_to_string(path) {
        Ok(folder) if !folder.is_empty() => Ok(Some(PathBuf::from(folder))),
        Ok(_) => Err(MOVIES_FOLDER_STORAGE_FAILED),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(MOVIES_FOLDER_STORAGE_FAILED),
    }
}

fn save_movies_folder_file(path: &Path, folder: &Path) -> Result<(), &'static str> {
    let folder = folder.to_str().ok_or(MOVIES_FOLDER_STORAGE_FAILED)?;
    let parent = path.parent().ok_or(MOVIES_FOLDER_STORAGE_FAILED)?;
    fs::create_dir_all(parent).map_err(|_| MOVIES_FOLDER_STORAGE_FAILED)?;
    fs::write(path, folder).map_err(|_| MOVIES_FOLDER_STORAGE_FAILED)
}

fn clear_movies_folder_file(path: &Path) -> Result<(), &'static str> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(MOVIES_FOLDER_STORAGE_FAILED),
    }
}

fn scan_movies_library(library: &mut MoviesLibraryContext) -> Result<Vec<String>, &'static str> {
    let folder = library.folder.as_deref().ok_or(MOVIES_FOLDER_UNAVAILABLE)?;
    let movie_paths = scan_movie_paths(folder)?;
    library.movie_paths.clone_from(&movie_paths);
    Ok(movie_paths)
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum MoviePathValidationError {
    NotFound,
    Unavailable,
    NotFile,
    Unsupported,
}

impl MoviePathValidationError {
    fn open_error_code(self) -> &'static str {
        match self {
            Self::NotFound => MOVIE_OPEN_NOT_FOUND,
            Self::Unavailable => MOVIE_OPEN_UNAVAILABLE,
            Self::NotFile => MOVIE_OPEN_NOT_FILE,
            Self::Unsupported => MOVIE_OPEN_UNSUPPORTED,
        }
    }

    fn reveal_error_code(self) -> &'static str {
        match self {
            Self::NotFound => MOVIE_REVEAL_NOT_FOUND,
            Self::Unavailable => MOVIE_REVEAL_UNAVAILABLE,
            Self::NotFile => MOVIE_REVEAL_NOT_FILE,
            Self::Unsupported => MOVIE_REVEAL_UNSUPPORTED,
        }
    }

    fn trash_error_code(self) -> &'static str {
        match self {
            Self::NotFound => MOVIE_TRASH_NOT_FOUND,
            Self::Unavailable => MOVIE_TRASH_UNAVAILABLE,
            Self::NotFile => MOVIE_TRASH_NOT_FILE,
            Self::Unsupported => MOVIE_TRASH_UNSUPPORTED,
        }
    }
}

fn movie_metadata_error(error: &io::Error) -> MoviePathValidationError {
    if error.kind() == io::ErrorKind::NotFound {
        MoviePathValidationError::NotFound
    } else {
        MoviePathValidationError::Unavailable
    }
}

fn validate_movie_path(path: &Path) -> Result<(), MoviePathValidationError> {
    let metadata = fs::metadata(path).map_err(|error| movie_metadata_error(&error))?;
    if !metadata.is_file() {
        return Err(MoviePathValidationError::NotFile);
    }
    if !is_supported_movie(path) {
        return Err(MoviePathValidationError::Unsupported);
    }

    Ok(())
}

fn open_movie_path_with(
    path: &Path,
    dispatch: impl FnOnce(&Path) -> Result<(), ()>,
) -> Result<(), &'static str> {
    validate_movie_path(path).map_err(MoviePathValidationError::open_error_code)?;

    dispatch(path).map_err(|_| MOVIE_OPEN_FAILED)
}

fn reveal_movie_path_with(
    path: &Path,
    dispatch: impl FnOnce(&Path) -> Result<(), ()>,
) -> Result<(), &'static str> {
    validate_movie_path(path).map_err(MoviePathValidationError::reveal_error_code)?;

    dispatch(path).map_err(|_| MOVIE_REVEAL_FAILED)
}

fn trash_movie_path_with(
    path: &Path,
    folder: &Path,
    confirmed_movie_paths: &[String],
    current_movie_paths: &[String],
    dispatch: impl FnOnce(&Path) -> Result<(), ()>,
) -> Result<(), &'static str> {
    let folder_metadata = fs::metadata(folder).map_err(|_| MOVIE_TRASH_FOLDER_UNAVAILABLE)?;
    if !folder_metadata.is_dir() {
        return Err(MOVIE_TRASH_FOLDER_UNAVAILABLE);
    }

    let path_metadata = fs::symlink_metadata(path)
        .map_err(|error| movie_metadata_error(&error).trash_error_code())?;
    if !path_metadata.is_file() {
        return Err(MOVIE_TRASH_NOT_FILE);
    }
    if !is_supported_movie(path) {
        return Err(MOVIE_TRASH_UNSUPPORTED);
    }

    let canonical_folder = fs::canonicalize(folder).map_err(|_| MOVIE_TRASH_FOLDER_UNAVAILABLE)?;
    let canonical_path =
        fs::canonicalize(path).map_err(|error| movie_metadata_error(&error).trash_error_code())?;
    if !canonical_path.starts_with(canonical_folder) {
        return Err(MOVIE_TRASH_OUTSIDE_FOLDER);
    }

    let requested_path = path.to_str().ok_or(MOVIE_TRASH_UNAVAILABLE)?;
    if !confirmed_movie_paths
        .iter()
        .any(|confirmed_path| confirmed_path == requested_path)
        || !current_movie_paths
            .iter()
            .any(|current_path| current_path == requested_path)
    {
        return Err(MOVIE_TRASH_STALE);
    }

    dispatch(path).map_err(|_| MOVIE_TRASH_FAILED)
}

fn trash_movie_request_with(
    request: TrashMovieRequest,
    library: &mut MoviesLibraryContext,
    dispatch: impl FnOnce(&Path) -> Result<(), ()>,
) -> Result<(), &'static str> {
    // Caller context is accepted for compatibility but never participates in authorization.
    let _ignored_caller_context = (request.folder, request.library_paths);
    let folder = library
        .folder
        .as_deref()
        .ok_or(MOVIE_TRASH_FOLDER_UNAVAILABLE)?;
    let current_movie_paths = scan_movie_paths(folder).map_err(|error| {
        if error == MOVIES_FOLDER_UNAVAILABLE {
            MOVIE_TRASH_FOLDER_UNAVAILABLE
        } else {
            MOVIE_TRASH_UNAVAILABLE
        }
    })?;

    trash_movie_path_with(
        Path::new(&request.path),
        folder,
        &library.movie_paths,
        &current_movie_paths,
        dispatch,
    )?;
    library
        .movie_paths
        .retain(|movie_path| movie_path != &request.path);
    Ok(())
}

#[cfg(target_os = "macos")]
fn move_to_os_trash(path: &Path) -> Result<(), ()> {
    const MACOS_TRASH_SCRIPT: &str = r#"on run argv
tell application "Finder"
  delete (POSIX file (item 1 of argv))
end tell
end run"#;

    Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(MACOS_TRASH_SCRIPT)
        .arg(path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|_| ())?
        .success()
        .then_some(())
        .ok_or(())
}

#[cfg(target_os = "windows")]
fn move_to_os_trash(path: &Path) -> Result<(), ()> {
    const WINDOWS_TRASH_PATH_ENV: &str = "AUTO_VIDEO_TRASH_PATH";
    // Microsoft.VisualBasic sends the file to the Recycle Bin without a permanent-delete fallback.
    const WINDOWS_RECYCLE_SCRIPT: &str = r#"$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName Microsoft.VisualBasic
[Microsoft.VisualBasic.FileIO.FileSystem]::DeleteFile(
  $env:AUTO_VIDEO_TRASH_PATH,
  [Microsoft.VisualBasic.FileIO.UIOption]::OnlyErrorDialogs,
  [Microsoft.VisualBasic.FileIO.RecycleOption]::SendToRecycleBin,
  [Microsoft.VisualBasic.FileIO.UICancelOption]::ThrowException
)"#;

    Command::new("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
        .arg(WINDOWS_RECYCLE_SCRIPT)
        .env(WINDOWS_TRASH_PATH_ENV, path.as_os_str())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|_| ())?
        .success()
        .then_some(())
        .ok_or(())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn move_to_os_trash(_path: &Path) -> Result<(), ()> {
    Err(())
}

fn is_valid_tmdb_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= TMDB_TOKEN_MAX_LENGTH
        && token.trim() == token
        && !token.chars().any(char::is_control)
}

fn load_tmdb_token_file(path: &Path) -> Result<Option<String>, &'static str> {
    let token = match fs::read_to_string(path) {
        Ok(token) => token,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(TMDB_TOKEN_STORAGE_FAILED),
    };

    if !is_valid_tmdb_token(&token) {
        return Err(TMDB_TOKEN_STORAGE_FAILED);
    }

    Ok(Some(token))
}

fn save_tmdb_token_file(path: &Path, token: &str) -> Result<(), &'static str> {
    if !is_valid_tmdb_token(token) {
        return Err(TMDB_TOKEN_INVALID);
    }

    let parent = path.parent().ok_or(TMDB_TOKEN_STORAGE_FAILED)?;
    fs::create_dir_all(parent).map_err(|_| TMDB_TOKEN_STORAGE_FAILED)?;

    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options.open(path).map_err(|_| TMDB_TOKEN_STORAGE_FAILED)?;
    file.write_all(token.as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|_| TMDB_TOKEN_STORAGE_FAILED)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|_| TMDB_TOKEN_STORAGE_FAILED)?;
    }

    Ok(())
}

fn clear_tmdb_token_file(path: &Path) -> Result<(), &'static str> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(TMDB_TOKEN_STORAGE_FAILED),
    }
}

fn tmdb_token_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|directory| directory.join(TMDB_TOKEN_FILE_NAME))
        .map_err(|_| TMDB_TOKEN_STORAGE_FAILED.to_owned())
}

fn movies_folder_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|directory| directory.join(MOVIES_FOLDER_FILE_NAME))
        .map_err(|_| MOVIES_FOLDER_STORAGE_FAILED.to_owned())
}

#[tauri::command]
fn load_movies_folder(
    app: tauri::AppHandle,
    state: tauri::State<'_, MoviesLibraryState>,
) -> Result<Option<String>, String> {
    let folder = load_movies_folder_file(&movies_folder_path(&app)?)?;
    let response = folder
        .as_ref()
        .map(|folder| {
            folder
                .to_str()
                .map(str::to_owned)
                .ok_or(MOVIES_FOLDER_STORAGE_FAILED)
        })
        .transpose()?;
    let mut library = state
        .0
        .lock()
        .map_err(|_| MOVIES_FOLDER_STORAGE_FAILED.to_owned())?;
    library.folder = folder;
    library.movie_paths.clear();
    Ok(response)
}

#[tauri::command]
async fn choose_movies_folder(
    app: tauri::AppHandle,
    state: tauri::State<'_, MoviesLibraryState>,
) -> Result<Option<String>, String> {
    let dialog_app = app.clone();
    let selected_folder = tauri::async_runtime::spawn_blocking(move || {
        dialog_app
            .dialog()
            .file()
            .set_title("Choose Movies folder")
            .blocking_pick_folder()
    })
    .await
    .map_err(|_| MOVIES_FOLDER_UNAVAILABLE.to_owned())?;
    let Some(selected_folder) = selected_folder else {
        return Ok(None);
    };
    let folder = selected_folder
        .into_path()
        .map_err(|_| MOVIES_FOLDER_UNAVAILABLE.to_owned())?;
    let metadata = fs::metadata(&folder).map_err(|_| MOVIES_FOLDER_UNAVAILABLE.to_owned())?;
    if !metadata.is_dir() {
        return Err(MOVIES_FOLDER_UNAVAILABLE.to_owned());
    }
    let response = folder
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| MOVIES_FOLDER_UNAVAILABLE.to_owned())?;

    save_movies_folder_file(&movies_folder_path(&app)?, &folder)?;
    let mut library = state
        .0
        .lock()
        .map_err(|_| MOVIES_FOLDER_STORAGE_FAILED.to_owned())?;
    library.folder = Some(folder);
    library.movie_paths.clear();
    Ok(Some(response))
}

#[tauri::command]
fn clear_movies_folder(
    app: tauri::AppHandle,
    state: tauri::State<'_, MoviesLibraryState>,
) -> Result<(), String> {
    clear_movies_folder_file(&movies_folder_path(&app)?)?;
    let mut library = state
        .0
        .lock()
        .map_err(|_| MOVIES_FOLDER_STORAGE_FAILED.to_owned())?;
    library.folder = None;
    library.movie_paths.clear();
    Ok(())
}

#[tauri::command]
async fn scan_movies(state: tauri::State<'_, MoviesLibraryState>) -> Result<Vec<String>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut library = state.0.lock().map_err(|_| MOVIES_SCAN_FAILED.to_owned())?;
        scan_movies_library(&mut library).map_err(str::to_owned)
    })
    .await
    .map_err(|_| MOVIES_SCAN_FAILED.to_owned())?
}

#[tauri::command]
async fn query_movies_storage(
    state: tauri::State<'_, MoviesLibraryState>,
) -> Result<[String; 2], String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let folder = state
            .0
            .lock()
            .map_err(|_| MOVIES_STORAGE_FAILED.to_owned())?
            .folder
            .clone();
        let [total_bytes, free_bytes] =
            query_movies_volume_storage_with(folder.as_deref(), query_movies_volume_storage)
                .map_err(str::to_owned)?;
        Ok([total_bytes.to_string(), free_bytes.to_string()])
    })
    .await
    .map_err(|_| MOVIES_STORAGE_FAILED.to_owned())?
}

#[tauri::command]
async fn open_movie(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        open_movie_path_with(Path::new(&path), |movie_path| {
            tauri_plugin_opener::open_path(movie_path, None::<&str>).map_err(|_| ())
        })
    })
    .await
    .map_err(|_| MOVIE_OPEN_FAILED.to_owned())?
    .map_err(str::to_owned)
}

#[tauri::command]
async fn reveal_movie(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        reveal_movie_path_with(Path::new(&path), |movie_path| {
            tauri_plugin_opener::reveal_item_in_dir(movie_path).map_err(|_| ())
        })
    })
    .await
    .map_err(|_| MOVIE_REVEAL_FAILED.to_owned())?
    .map_err(str::to_owned)
}

#[tauri::command]
async fn trash_movie(
    path: String,
    folder: Option<String>,
    library_paths: Option<Vec<String>>,
    state: tauri::State<'_, MoviesLibraryState>,
) -> Result<(), String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut library = state.0.lock().map_err(|_| MOVIE_TRASH_UNAVAILABLE)?;
        trash_movie_request_with(
            TrashMovieRequest {
                path,
                folder,
                library_paths,
            },
            &mut library,
            move_to_os_trash,
        )
    })
    .await
    .map_err(|_| MOVIE_TRASH_FAILED.to_owned())?
    .map_err(str::to_owned)
}

#[tauri::command]
fn load_tmdb_token(app: tauri::AppHandle) -> Result<Option<String>, String> {
    load_tmdb_token_file(&tmdb_token_path(&app)?).map_err(str::to_owned)
}

#[tauri::command]
fn save_tmdb_token(app: tauri::AppHandle, token: String) -> Result<(), String> {
    save_tmdb_token_file(&tmdb_token_path(&app)?, &token).map_err(str::to_owned)
}

#[tauri::command]
fn clear_tmdb_token(app: tauri::AppHandle) -> Result<(), String> {
    clear_tmdb_token_file(&tmdb_token_path(&app)?).map_err(str::to_owned)
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(MoviesLibraryState::default())
        .invoke_handler(tauri::generate_handler![
            load_movies_folder,
            choose_movies_folder,
            clear_movies_folder,
            scan_movies,
            query_movies_storage,
            open_movie,
            reveal_movie,
            trash_movie,
            load_tmdb_token,
            save_tmdb_token,
            clear_tmdb_token
        ])
        .run(tauri::generate_context!())
        .expect("failed to run the Auto-Video desktop application");
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        fs, io,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    #[cfg(target_os = "macos")]
    use super::parse_macos_volume_storage;
    #[cfg(target_os = "windows")]
    use super::parse_windows_volume_storage;
    use super::{
        clear_movies_folder_file, clear_tmdb_token_file, load_movies_folder_file,
        load_tmdb_token_file, movie_metadata_error, open_movie_path_with,
        query_movies_volume_storage_with, reveal_movie_path_with, save_movies_folder_file,
        save_tmdb_token_file, scan_movie_paths, trash_movie_path_with, trash_movie_request_with,
        MoviePathValidationError, MoviesLibraryContext, MoviesVolumeStorageQueryError,
        TrashMovieRequest, MOVIES_FOLDER_UNAVAILABLE, MOVIES_STORAGE_FAILED,
        MOVIES_STORAGE_UNAVAILABLE, MOVIE_OPEN_FAILED, MOVIE_OPEN_NOT_FILE, MOVIE_OPEN_NOT_FOUND,
        MOVIE_OPEN_UNAVAILABLE, MOVIE_OPEN_UNSUPPORTED, MOVIE_REVEAL_FAILED, MOVIE_REVEAL_NOT_FILE,
        MOVIE_REVEAL_NOT_FOUND, MOVIE_REVEAL_UNAVAILABLE, MOVIE_REVEAL_UNSUPPORTED,
        MOVIE_TRASH_FAILED, MOVIE_TRASH_FOLDER_UNAVAILABLE, MOVIE_TRASH_NOT_FILE,
        MOVIE_TRASH_NOT_FOUND, MOVIE_TRASH_OUTSIDE_FOLDER, MOVIE_TRASH_STALE,
        MOVIE_TRASH_UNAVAILABLE, MOVIE_TRASH_UNSUPPORTED, TMDB_TOKEN_INVALID,
    };

    static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

    struct FilesystemFixture {
        path: PathBuf,
    }

    impl FilesystemFixture {
        fn new() -> Self {
            let fixture_id = NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "auto-video-filesystem-fixture-{}-{fixture_id}",
                std::process::id()
            ));
            fs::create_dir(&path).expect("failed to create filesystem fixture");
            Self { path }
        }

        fn create_file(&self, relative_path: impl AsRef<Path>) -> PathBuf {
            let path = self.path.join(relative_path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("failed to create fixture directory");
            }
            fs::write(&path, []).expect("failed to create fixture file");
            path
        }
    }

    impl Drop for FilesystemFixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.path).expect("failed to remove filesystem fixture");
        }
    }

    fn path_string(path: PathBuf) -> String {
        path.into_os_string()
            .into_string()
            .expect("fixture paths must be valid Unicode")
    }

    #[test]
    fn recursively_finds_supported_files_in_deterministic_order() {
        let fixture = FilesystemFixture::new();
        let first_movie = fixture.create_file("Alpha.mp4");
        let second_movie = fixture.create_file(Path::new("nested").join("Beta.MKV"));
        let third_movie =
            fixture.create_file(Path::new("nested").join("deeper").join("映画 — Final.mP4"));
        fixture.create_file(Path::new("nested").join("notes.txt"));
        fixture.create_file("clip.mov");
        fs::create_dir(fixture.path.join("directory.mkv"))
            .expect("failed to create fixture directory");

        let mut expected_paths = vec![
            path_string(first_movie),
            path_string(second_movie),
            path_string(third_movie),
        ];
        expected_paths.sort();

        assert_eq!(scan_movie_paths(&fixture.path), Ok(expected_paths));
    }

    #[test]
    fn reports_an_unavailable_folder_for_missing_or_non_directory_roots() {
        let fixture = FilesystemFixture::new();
        let file_path = fixture.create_file("not-a-folder.mp4");
        let missing_path = fixture.path.join("missing");

        assert_eq!(
            scan_movie_paths(&missing_path),
            Err(MOVIES_FOLDER_UNAVAILABLE)
        );
        assert_eq!(scan_movie_paths(&file_path), Err(MOVIES_FOLDER_UNAVAILABLE));
    }

    #[test]
    fn queries_valid_volume_storage_for_the_trusted_movies_folder() {
        let fixture = FilesystemFixture::new();
        let queried_folder = RefCell::new(None);
        let total_bytes = 4 * 1024 * 1024 * 1024_u64;
        let free_bytes = 1024 * 1024 * 1024_u64;

        let result = query_movies_volume_storage_with(Some(&fixture.path), |folder| {
            queried_folder.replace(Some(folder.to_path_buf()));
            Ok([total_bytes, free_bytes])
        });

        assert_eq!(result, Ok([total_bytes, free_bytes]));
        assert_eq!(queried_folder.into_inner(), Some(fixture.path.clone()));
    }

    #[test]
    fn rejects_missing_and_non_directory_storage_roots_before_querying_the_volume() {
        let fixture = FilesystemFixture::new();
        let file_path = fixture.create_file("not-a-folder.mp4");
        let missing_path = fixture.path.join("missing");

        for folder in [
            None,
            Some(missing_path.as_path()),
            Some(file_path.as_path()),
        ] {
            let queried = RefCell::new(false);
            let result = query_movies_volume_storage_with(folder, |_| {
                queried.replace(true);
                Ok([1024, 512])
            });

            assert_eq!(result, Err(MOVIES_STORAGE_UNAVAILABLE));
            assert!(!queried.into_inner());
        }
    }

    #[test]
    fn rejects_invalid_volume_storage_values() {
        let fixture = FilesystemFixture::new();

        for invalid_values in [[0, 0], [1024, 1025]] {
            assert_eq!(
                query_movies_volume_storage_with(Some(&fixture.path), |_| Ok(invalid_values)),
                Err(MOVIES_STORAGE_FAILED)
            );
        }
    }

    #[test]
    fn distinguishes_an_unavailable_volume_from_a_storage_query_failure() {
        let fixture = FilesystemFixture::new();

        assert_eq!(
            query_movies_volume_storage_with(Some(&fixture.path), |_| {
                Err(MoviesVolumeStorageQueryError::Unavailable)
            }),
            Err(MOVIES_STORAGE_UNAVAILABLE)
        );
        assert_eq!(
            query_movies_volume_storage_with(Some(&fixture.path), |_| {
                Err(MoviesVolumeStorageQueryError::Failed)
            }),
            Err(MOVIES_STORAGE_FAILED)
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn parses_deterministic_macos_volume_storage_output() {
        let output = b"Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk3s5 4294967296 3145728 1073741824 75% /System/Volumes/Data\n";

        assert_eq!(
            parse_macos_volume_storage(output),
            Ok([4_398_046_511_104, 1_099_511_627_776])
        );
        assert_eq!(
            parse_macos_volume_storage(b"invalid output"),
            Err(MoviesVolumeStorageQueryError::Failed)
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn parses_deterministic_windows_volume_storage_output() {
        assert_eq!(
            parse_windows_volume_storage(b"4398046511104 1099511627776\r\n"),
            Ok([4_398_046_511_104, 1_099_511_627_776])
        );
        assert_eq!(
            parse_windows_volume_storage(b"4398046511104 1099511627776 extra"),
            Err(MoviesVolumeStorageQueryError::Failed)
        );
    }

    #[test]
    fn opens_the_exact_supported_movie_path_after_validation() {
        let fixture = FilesystemFixture::new();
        let movie_path = fixture.create_file("映画  —  Final.CUT!.MKV");
        let dispatched_path = RefCell::new(None);

        let result = open_movie_path_with(&movie_path, |path| {
            dispatched_path.replace(Some(path.to_path_buf()));
            Ok(())
        });

        assert_eq!(result, Ok(()));
        assert_eq!(dispatched_path.into_inner(), Some(movie_path));
    }

    #[test]
    fn reveals_the_exact_supported_movie_path_after_validation() {
        let fixture = FilesystemFixture::new();
        let movie_path = fixture.create_file("映画  —  Final.CUT!.MKV");
        let dispatched_path = RefCell::new(None);

        let result = reveal_movie_path_with(&movie_path, |path| {
            dispatched_path.replace(Some(path.to_path_buf()));
            Ok(())
        });

        assert_eq!(result, Ok(()));
        assert_eq!(dispatched_path.into_inner(), Some(movie_path));
    }

    #[test]
    fn trashes_the_exact_current_movie_path_after_root_validation() {
        let fixture = FilesystemFixture::new();
        fixture.create_file("nested/映画  —  Final.CUT!.MKV");
        let movie_paths = scan_movie_paths(&fixture.path).expect("failed to scan fixture");
        let movie_path = PathBuf::from(&movie_paths[0]);
        let mut library = MoviesLibraryContext {
            folder: Some(fixture.path.clone()),
            movie_paths: movie_paths.clone(),
        };
        let dispatched_path = RefCell::new(None);

        let result = trash_movie_request_with(
            TrashMovieRequest {
                path: movie_paths[0].clone(),
                folder: None,
                library_paths: None,
            },
            &mut library,
            |path| {
                dispatched_path.replace(Some(path.to_path_buf()));
                Ok(())
            },
        );

        assert_eq!(result, Ok(()));
        assert_eq!(dispatched_path.into_inner(), Some(movie_path));
        assert!(library.movie_paths.is_empty());
    }

    #[test]
    fn trash_rejects_fabricated_context_for_a_movie_outside_the_trusted_library() {
        let trusted_fixture = FilesystemFixture::new();
        let trusted_movie = trusted_fixture.create_file("Trusted.mp4");
        let unrelated_fixture = FilesystemFixture::new();
        let unrelated_movie = unrelated_fixture.create_file("Unrelated.mkv");
        let unrelated_movie_path = path_string(unrelated_movie.clone());
        let mut trusted_library = MoviesLibraryContext {
            folder: Some(trusted_fixture.path.clone()),
            movie_paths: vec![path_string(trusted_movie)],
        };
        let dispatched = RefCell::new(false);

        let result = trash_movie_request_with(
            TrashMovieRequest {
                path: unrelated_movie_path.clone(),
                folder: Some(path_string(unrelated_fixture.path.clone())),
                library_paths: Some(vec![unrelated_movie_path]),
            },
            &mut trusted_library,
            |_| {
                dispatched.replace(true);
                Ok(())
            },
        );

        assert_eq!(result, Err(MOVIE_TRASH_OUTSIDE_FOLDER));
        assert!(!dispatched.into_inner());
    }

    #[test]
    fn open_rejects_missing_directories_and_unsupported_files_before_dispatch() {
        let fixture = FilesystemFixture::new();
        let missing_path = fixture.path.join("missing.mp4");
        let directory_path = fixture.path.join("directory.mkv");
        let unsupported_path = fixture.create_file("notes.txt");
        fs::create_dir(&directory_path).expect("failed to create fixture directory");

        for (path, expected_error) in [
            (&missing_path, MOVIE_OPEN_NOT_FOUND),
            (&directory_path, MOVIE_OPEN_NOT_FILE),
            (&unsupported_path, MOVIE_OPEN_UNSUPPORTED),
        ] {
            let dispatched = RefCell::new(false);
            let result = open_movie_path_with(path, |_| {
                dispatched.replace(true);
                Ok(())
            });

            assert_eq!(result, Err(expected_error));
            assert!(!dispatched.into_inner());
        }
    }

    #[test]
    fn reveal_rejects_missing_directories_and_unsupported_files_before_dispatch() {
        let fixture = FilesystemFixture::new();
        let missing_path = fixture.path.join("missing.mp4");
        let directory_path = fixture.path.join("directory.mkv");
        let unsupported_path = fixture.create_file("notes.txt");
        fs::create_dir(&directory_path).expect("failed to create fixture directory");

        for (path, expected_error) in [
            (&missing_path, MOVIE_REVEAL_NOT_FOUND),
            (&directory_path, MOVIE_REVEAL_NOT_FILE),
            (&unsupported_path, MOVIE_REVEAL_UNSUPPORTED),
        ] {
            let dispatched = RefCell::new(false);
            let result = reveal_movie_path_with(path, |_| {
                dispatched.replace(true);
                Ok(())
            });

            assert_eq!(result, Err(expected_error));
            assert!(!dispatched.into_inner());
        }
    }

    #[test]
    fn trash_rejects_invalid_or_unassociated_targets_before_dispatch() {
        let fixture = FilesystemFixture::new();
        let outside_fixture = FilesystemFixture::new();
        let missing_path = fixture.path.join("missing.mp4");
        let directory_path = fixture.path.join("directory.mkv");
        let unsupported_path = fixture.create_file("notes.txt");
        let unconfirmed_path = fixture.create_file("unconfirmed.mp4");
        let stale_path = fixture.create_file("stale.mp4");
        let outside_path = outside_fixture.create_file("outside.mkv");
        fs::create_dir(&directory_path).expect("failed to create fixture directory");

        for (path, folder, confirmed_movie_paths, current_movie_paths, expected_error) in [
            (
                &missing_path,
                &fixture.path,
                Vec::new(),
                Vec::new(),
                MOVIE_TRASH_NOT_FOUND,
            ),
            (
                &directory_path,
                &fixture.path,
                Vec::new(),
                Vec::new(),
                MOVIE_TRASH_NOT_FILE,
            ),
            (
                &unsupported_path,
                &fixture.path,
                Vec::new(),
                Vec::new(),
                MOVIE_TRASH_UNSUPPORTED,
            ),
            (
                &outside_path,
                &fixture.path,
                vec![path_string(outside_path.clone())],
                vec![path_string(outside_path.clone())],
                MOVIE_TRASH_OUTSIDE_FOLDER,
            ),
            (
                &unconfirmed_path,
                &fixture.path,
                Vec::new(),
                vec![path_string(unconfirmed_path.clone())],
                MOVIE_TRASH_STALE,
            ),
            (
                &stale_path,
                &fixture.path,
                vec![path_string(stale_path.clone())],
                Vec::new(),
                MOVIE_TRASH_STALE,
            ),
        ] {
            let dispatched = RefCell::new(false);
            let result = trash_movie_path_with(
                path,
                folder,
                &confirmed_movie_paths,
                &current_movie_paths,
                |_| {
                    dispatched.replace(true);
                    Ok(())
                },
            );

            assert_eq!(result, Err(expected_error));
            assert!(!dispatched.into_inner());
        }
    }

    #[test]
    fn trash_rejects_an_unavailable_movies_folder_before_dispatch() {
        let fixture = FilesystemFixture::new();
        let movie_path = fixture.create_file("Movie.mp4");
        let missing_folder = fixture.path.join("missing-folder");
        let current_movie_paths = vec![path_string(movie_path.clone())];
        let dispatched = RefCell::new(false);

        let result = trash_movie_path_with(
            &movie_path,
            &missing_folder,
            &current_movie_paths,
            &current_movie_paths,
            |_| {
                dispatched.replace(true);
                Ok(())
            },
        );

        assert_eq!(result, Err(MOVIE_TRASH_FOLDER_UNAVAILABLE));
        assert!(!dispatched.into_inner());
    }

    #[cfg(unix)]
    #[test]
    fn trash_rejects_a_symlink_instead_of_treating_it_as_a_regular_file() {
        use std::os::unix::fs::symlink;

        let fixture = FilesystemFixture::new();
        let movie_path = fixture.create_file("Movie.mp4");
        let symlink_path = fixture.path.join("Movie link.mp4");
        symlink(movie_path, &symlink_path).expect("failed to create fixture symlink");
        let current_movie_paths = vec![path_string(symlink_path.clone())];
        let dispatched = RefCell::new(false);

        let result = trash_movie_path_with(
            &symlink_path,
            &fixture.path,
            &current_movie_paths,
            &current_movie_paths,
            |_| {
                dispatched.replace(true);
                Ok(())
            },
        );

        assert_eq!(result, Err(MOVIE_TRASH_NOT_FILE));
        assert!(!dispatched.into_inner());
    }

    #[test]
    fn reports_inaccessible_metadata_and_operating_system_action_failures() {
        let inaccessible = io::Error::new(io::ErrorKind::PermissionDenied, "fixture denial");
        assert_eq!(
            movie_metadata_error(&inaccessible),
            MoviePathValidationError::Unavailable
        );
        assert_eq!(
            MoviePathValidationError::Unavailable.open_error_code(),
            MOVIE_OPEN_UNAVAILABLE
        );
        assert_eq!(
            MoviePathValidationError::Unavailable.reveal_error_code(),
            MOVIE_REVEAL_UNAVAILABLE
        );
        assert_eq!(
            MoviePathValidationError::Unavailable.trash_error_code(),
            MOVIE_TRASH_UNAVAILABLE
        );

        let fixture = FilesystemFixture::new();
        let movie_path = fixture.create_file("Valid.mp4");
        assert_eq!(
            open_movie_path_with(&movie_path, |_| Err(())),
            Err(MOVIE_OPEN_FAILED)
        );
        assert_eq!(
            reveal_movie_path_with(&movie_path, |_| Err(())),
            Err(MOVIE_REVEAL_FAILED)
        );
        assert_eq!(
            trash_movie_path_with(
                &movie_path,
                &fixture.path,
                &[path_string(movie_path.clone())],
                &[path_string(movie_path.clone())],
                |_| Err(())
            ),
            Err(MOVIE_TRASH_FAILED)
        );
    }

    #[test]
    fn persists_loads_and_clears_the_configured_movies_folder() {
        let fixture = FilesystemFixture::new();
        let config_path = fixture.path.join("movies-folder");
        let movies_folder = fixture.path.join("Movies — 家族");

        assert_eq!(load_movies_folder_file(&config_path), Ok(None));
        save_movies_folder_file(&config_path, &movies_folder)
            .expect("failed to save Movies folder");
        assert_eq!(
            load_movies_folder_file(&config_path),
            Ok(Some(movies_folder))
        );
        clear_movies_folder_file(&config_path).expect("failed to clear Movies folder");
        assert_eq!(load_movies_folder_file(&config_path), Ok(None));
    }

    #[test]
    fn saves_replaces_loads_and_clears_the_tmdb_token() {
        let fixture = FilesystemFixture::new();
        let token_path = fixture.path.join("token");

        assert_eq!(load_tmdb_token_file(&token_path), Ok(None));

        save_tmdb_token_file(&token_path, "fixture-token-one")
            .expect("failed to save fixture token");
        assert_eq!(
            load_tmdb_token_file(&token_path),
            Ok(Some("fixture-token-one".to_owned()))
        );

        save_tmdb_token_file(&token_path, "fixture-token-two")
            .expect("failed to replace fixture token");
        assert_eq!(
            load_tmdb_token_file(&token_path),
            Ok(Some("fixture-token-two".to_owned()))
        );

        clear_tmdb_token_file(&token_path).expect("failed to clear fixture token");
        assert_eq!(load_tmdb_token_file(&token_path), Ok(None));
        clear_tmdb_token_file(&token_path).expect("clearing a missing token must succeed");
    }

    #[test]
    fn rejects_empty_whitespace_and_control_characters_in_tmdb_tokens() {
        let fixture = FilesystemFixture::new();
        let token_path = fixture.path.join("token");

        for token in ["", " leading", "trailing ", "line\nbreak"] {
            assert_eq!(
                save_tmdb_token_file(&token_path, token),
                Err(TMDB_TOKEN_INVALID)
            );
        }
        assert_eq!(load_tmdb_token_file(&token_path), Ok(None));
    }

    #[cfg(unix)]
    #[test]
    fn restricts_the_tmdb_token_file_to_the_current_user() {
        use std::os::unix::fs::PermissionsExt;

        let fixture = FilesystemFixture::new();
        let token_path = fixture.path.join("token");
        save_tmdb_token_file(&token_path, "fixture-token").expect("failed to save fixture token");

        let mode = fs::metadata(token_path)
            .expect("failed to read fixture token metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
