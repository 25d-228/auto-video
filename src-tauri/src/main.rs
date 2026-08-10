#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod adult_library;
mod tv_library;
mod tv_release;
mod vr_download;
mod vr_library;
mod vr_torrent;

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::process::{Command, Stdio};

use adult_library::{
    clear_adult_folder as clear_trusted_adult_folder, configured_adult_folder,
    load_adult_folder_with, open_adult_file_with, reveal_adult_file_with, scan_adult_library_with,
    set_adult_folder, trash_adult_file_with, AdultLibraryState, ADULT_FILE_OPEN_FAILED,
    ADULT_FILE_REVEAL_FAILED, ADULT_FILE_TRASH_FAILED, ADULT_FOLDER_STORAGE_FAILED,
    ADULT_FOLDER_UNAVAILABLE, ADULT_LIBRARY_SCAN_FAILED,
};
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;
use tv_library::{
    clear_tv_folder as clear_trusted_tv_folder, configured_tv_folder, load_tv_folder_with,
    open_tv_file_with, reveal_tv_file_with, scan_tv_library_with, set_tv_folder,
    trash_tv_file_with_download_ownership, TvLibraryState, TV_FILE_OPEN_FAILED,
    TV_FILE_REVEAL_FAILED, TV_FILE_TRASH_FAILED, TV_FOLDER_STORAGE_FAILED, TV_FOLDER_UNAVAILABLE,
    TV_LIBRARY_SCAN_FAILED,
};
use tv_release::{
    fetch_apibay_tv_releases_for_state_with, TvReleaseState, TV_APIBAY_PROVIDER_ERROR,
    TV_TMDB_MALFORMED, TV_TMDB_UNAUTHORIZED,
};
use vr_download::{
    acquire_tv_metainfo, apply_organization, cancel_download,
    clear_vr_folder as clear_trusted_vr_folder, configure_adult_download_folder,
    configure_movie_download_folder, configure_tv_download_folder, configured_vr_folder,
    dismiss_download, dismiss_organization, list_downloads, load_download_limit, load_downloads,
    load_vr_folder_with, pause_download, preview_organization, resume_download,
    save_download_limit, set_vr_folder, start_adult_download, start_download, start_movie_download,
    start_tv_download, TvMetainfoAcquisitionError, VrDownloadState, VR_DOWNLOAD_FAILED,
    VR_DOWNLOAD_LIMIT_STORAGE_FAILED, VR_DOWNLOAD_PERSISTENCE_FAILED, VR_FOLDER_STORAGE_FAILED,
    VR_FOLDER_UNAVAILABLE,
};
use vr_library::{
    invalidate_vr_library, open_vr_file_with, reveal_vr_file_with, scan_vr_library_with,
    trash_vr_file_with, VrLibraryState, VR_FILE_OPEN_FAILED, VR_FILE_REVEAL_FAILED,
    VR_FILE_TRASH_FAILED, VR_LIBRARY_SCAN_FAILED,
};
use vr_torrent::{
    fetch_artifact_response, inspect_sukebei_adult_torrent_with, inspect_sukebei_torrent_with,
    inspect_yts_movie_torrent_with, save_verified_adult_torrent_with,
    save_verified_movie_torrent_with, save_verified_torrent_with, save_verified_tv_torrent_with,
    verified_movie_imdb_id, write_new_torrent_file, AdultTorrentState,
    MovieTorrentInspectionRequest, MovieTorrentState, TorrentInspectionRequest,
    TvTorrentInspectionStart, TvTorrentState, VrTorrentState, ADULT_TORRENT_PROVIDER_ERROR,
    ADULT_TORRENT_SAVE_FAILED, MOVIE_TMDB_MALFORMED, MOVIE_TORRENT_PROVIDER_ERROR,
    MOVIE_TORRENT_SAVE_FAILED, TV_TORRENT_CONTEXT_INVALID, TV_TORRENT_LOCAL_PENDING,
    TV_TORRENT_LOCAL_UNAVAILABLE, TV_TORRENT_NETWORK_ERROR, TV_TORRENT_NO_METADATA_SOURCE,
    TV_TORRENT_SAVE_FAILED, TV_TORRENT_TIMEOUT, VR_TORRENT_PROVIDER_ERROR, VR_TORRENT_SAVE_FAILED,
};

const MOVIES_FOLDER_FILE_NAME: &str = ".movies-folder";
const ADULT_FOLDER_FILE_NAME: &str = ".adult-folder";
const TV_FOLDER_FILE_NAME: &str = ".tv-folder";
const VR_FOLDER_FILE_NAME: &str = ".vr-folder";
const VR_DOWNLOADS_FILE_NAME: &str = ".vr-downloads";
const VR_DOWNLOAD_LIMIT_FILE_NAME: &str = ".vr-download-limit";
const VR_SESSION_FOLDER_NAME: &str = "vr-session";
const MOVIES_FOLDER_UNAVAILABLE: &str = "movies_folder_unavailable";
const MOVIES_FOLDER_STORAGE_FAILED: &str = "movies_folder_storage_failed";
const MOVIES_STORAGE_FAILED: &str = "movies_storage_failed";
const MOVIES_STORAGE_UNAVAILABLE: &str = "movies_storage_unavailable";
const VR_STORAGE_FAILED: &str = "vr_storage_failed";
const VR_STORAGE_UNAVAILABLE: &str = "vr_storage_unavailable";
const TV_STORAGE_FAILED: &str = "tv_storage_failed";
const TV_STORAGE_UNAVAILABLE: &str = "tv_storage_unavailable";
const ADULT_STORAGE_FAILED: &str = "adult_storage_failed";
const ADULT_STORAGE_UNAVAILABLE: &str = "adult_storage_unavailable";
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
const MOVIE_TMDB_NETWORK_ERROR: &str = "movie_tmdb_network_error";
const MOVIE_TMDB_PROVIDER_ERROR: &str = "movie_tmdb_provider_error";
const MOVIE_TMDB_RATE_LIMITED: &str = "movie_tmdb_rate_limited";
const MOVIE_TMDB_UNAUTHORIZED: &str = "movie_tmdb_unauthorized";
const MOVIE_YTS_NETWORK_ERROR: &str = "movie_yts_network_error";
const MOVIE_YTS_PROVIDER_ERROR: &str = "movie_yts_provider_error";
const MOVIE_YTS_SOURCE_UNAVAILABLE: &str = "movie_yts_source_unavailable";
const ADULT_NETWORK_ERROR: &str = "adult_network_error";
const ADULT_PROVIDER_ERROR: &str = "adult_provider_error";
const ADULT_SOURCE_UNAVAILABLE: &str = "adult_source_unavailable";
const VR_NETWORK_ERROR: &str = "vr_network_error";
const VR_PROVIDER_ERROR: &str = "vr_provider_error";
const VR_SOURCE_UNAVAILABLE: &str = "vr_source_unavailable";
// API Read Access Tokens are much shorter; this rejects arbitrary oversized IPC or file input.
const TMDB_TOKEN_MAX_LENGTH: usize = 4096;
// Provider documents are small result pages; this limits data returned across the native boundary.
const PROVIDER_RESPONSE_MAX_BYTES: usize = 4 * 1024 * 1024;
const PROVIDER_HTTP_STATUS_MARKER: &str = "\nAUTO_VIDEO_HTTP_STATUS:";
#[cfg(target_os = "macos")]
const PROVIDER_HTTP_STATUS_WRITE_OUT: &str = "\nAUTO_VIDEO_HTTP_STATUS:%{http_code}";
const JAVDB_CATALOG_URL: &str = "https://javdb.com/search?q=";
const SUKEBEI_RELEASES_URL: &str = "https://sukebei.nyaa.si/?page=rss&q=%22";
const TMDB_MOVIE_URL: &str = "https://api.themoviedb.org/3/movie/";
const YTS_MOVIES_URL: &str = "https://yts.mx/api/v2/list_movies.json?limit=50&query_term=";

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
enum ProviderRequestError {
    SourceUnavailable,
    Network,
    Provider,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum MovieProviderRequestError {
    Unauthorized,
    RateLimited,
    SourceUnavailable,
    Network,
    Provider,
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
    query_volume_storage_with(
        folder,
        MOVIES_STORAGE_UNAVAILABLE,
        MOVIES_STORAGE_FAILED,
        query,
    )
}

fn query_volume_storage_with(
    folder: Option<&Path>,
    unavailable_error: &'static str,
    failed_error: &'static str,
    query: impl FnOnce(&Path) -> Result<[u64; 2], MoviesVolumeStorageQueryError>,
) -> Result<[u64; 2], &'static str> {
    let folder = folder.ok_or(unavailable_error)?;
    let metadata = fs::metadata(folder).map_err(|_| unavailable_error)?;
    if !metadata.is_dir() {
        return Err(unavailable_error);
    }

    let [total_bytes, free_bytes] = query(folder).map_err(|error| match error {
        MoviesVolumeStorageQueryError::Unavailable => unavailable_error,
        MoviesVolumeStorageQueryError::Failed => failed_error,
    })?;
    if total_bytes == 0 || free_bytes > total_bytes {
        return Err(failed_error);
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

fn is_canonical_product_code(code: &str) -> bool {
    let Some((prefix, number)) = code.split_once('-') else {
        return false;
    };

    (2..=16).contains(&prefix.len())
        && prefix
            .bytes()
            .all(|character| character.is_ascii_uppercase())
        && (1..=10).contains(&number.len())
        && !number.starts_with('0')
        && number.bytes().all(|character| character.is_ascii_digit())
}

fn provider_error_code(error: ProviderRequestError) -> &'static str {
    match error {
        ProviderRequestError::SourceUnavailable => VR_SOURCE_UNAVAILABLE,
        ProviderRequestError::Network => VR_NETWORK_ERROR,
        ProviderRequestError::Provider => VR_PROVIDER_ERROR,
    }
}

fn adult_provider_error_code(error: ProviderRequestError) -> &'static str {
    match error {
        ProviderRequestError::SourceUnavailable => ADULT_SOURCE_UNAVAILABLE,
        ProviderRequestError::Network => ADULT_NETWORK_ERROR,
        ProviderRequestError::Provider => ADULT_PROVIDER_ERROR,
    }
}

fn parse_provider_response(output: &[u8]) -> Result<String, ProviderRequestError> {
    let output = std::str::from_utf8(output).map_err(|_| ProviderRequestError::Provider)?;
    let (document, status) = output
        .rsplit_once(PROVIDER_HTTP_STATUS_MARKER)
        .ok_or(ProviderRequestError::Provider)?;
    let status = status
        .trim()
        .parse::<u16>()
        .map_err(|_| ProviderRequestError::Provider)?;

    match status {
        200..=299 if document.len() <= PROVIDER_RESPONSE_MAX_BYTES => Ok(document.to_owned()),
        404 | 410 | 451 => Err(ProviderRequestError::SourceUnavailable),
        _ => Err(ProviderRequestError::Provider),
    }
}

#[cfg(target_os = "macos")]
fn fetch_provider_document(url: &str) -> Result<String, ProviderRequestError> {
    let output = Command::new("/usr/bin/curl")
        .args([
            "--silent",
            "--show-error",
            "--location",
            "--connect-timeout",
            "10",
            "--max-time",
            "20",
            "--user-agent",
            "Auto-Video/0.1",
            "--header",
            "Accept: text/html, application/xml;q=0.9",
            "--write-out",
            PROVIDER_HTTP_STATUS_WRITE_OUT,
            url,
        ])
        .output()
        .map_err(|_| ProviderRequestError::Network)?;
    if !output.status.success() {
        return Err(ProviderRequestError::Network);
    }

    parse_provider_response(&output.stdout)
}

#[cfg(target_os = "windows")]
fn fetch_provider_document(url: &str) -> Result<String, ProviderRequestError> {
    const PROVIDER_URL_ENV: &str = "AUTO_VIDEO_PROVIDER_URL";
    const WINDOWS_PROVIDER_SCRIPT: &str = r#"$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
try {
  $response = Invoke-WebRequest -UseBasicParsing -Uri $env:AUTO_VIDEO_PROVIDER_URL -Headers @{ Accept = 'text/html, application/xml;q=0.9'; 'User-Agent' = 'Auto-Video/0.1' } -TimeoutSec 20
  [Console]::Out.Write($response.Content)
  [Console]::Out.Write("`nAUTO_VIDEO_HTTP_STATUS:" + [int]$response.StatusCode)
} catch {
  $status = if ($null -eq $_.Exception.Response) { 0 } else { [int]$_.Exception.Response.StatusCode }
  [Console]::Out.Write("`nAUTO_VIDEO_HTTP_STATUS:" + $status)
}"#;
    let output = Command::new("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
        .arg(WINDOWS_PROVIDER_SCRIPT)
        // The validated native-built URL stays out of PowerShell source and cannot become script input.
        .env(PROVIDER_URL_ENV, url)
        .output()
        .map_err(|_| ProviderRequestError::Network)?;
    if !output.status.success() {
        return Err(ProviderRequestError::Network);
    }
    if output.stdout.ends_with(b"AUTO_VIDEO_HTTP_STATUS:0") {
        return Err(ProviderRequestError::Network);
    }

    parse_provider_response(&output.stdout)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn fetch_provider_document(_url: &str) -> Result<String, ProviderRequestError> {
    Err(ProviderRequestError::Network)
}

fn parse_movie_provider_response(output: &[u8]) -> Result<String, MovieProviderRequestError> {
    let output = std::str::from_utf8(output).map_err(|_| MovieProviderRequestError::Provider)?;
    let (document, status) = output
        .rsplit_once(PROVIDER_HTTP_STATUS_MARKER)
        .ok_or(MovieProviderRequestError::Provider)?;
    let status = status
        .trim()
        .parse::<u16>()
        .map_err(|_| MovieProviderRequestError::Provider)?;
    match status {
        200..=299 if document.len() <= PROVIDER_RESPONSE_MAX_BYTES => Ok(document.to_owned()),
        401 | 403 => Err(MovieProviderRequestError::Unauthorized),
        404 | 410 | 451 => Err(MovieProviderRequestError::SourceUnavailable),
        429 => Err(MovieProviderRequestError::RateLimited),
        0 => Err(MovieProviderRequestError::Network),
        _ => Err(MovieProviderRequestError::Provider),
    }
}

#[cfg(target_os = "macos")]
fn fetch_movie_provider_document(
    url: &str,
    tmdb_token: Option<&str>,
) -> Result<String, MovieProviderRequestError> {
    let mut command = Command::new("/usr/bin/curl");
    command.args([
        "--silent",
        "--show-error",
        "--connect-timeout",
        "10",
        "--max-time",
        "20",
        "--user-agent",
        "Auto-Video/0.1",
        "--header",
        "Accept: application/json",
        "--write-out",
        PROVIDER_HTTP_STATUS_WRITE_OUT,
    ]);
    if let Some(token) = tmdb_token {
        command
            .arg("--header")
            .arg(format!("Authorization: Bearer {token}"));
    } else {
        command.args(["--location", "--max-redirs", "3"]);
    }
    let output = command
        .arg(url)
        .output()
        .map_err(|_| MovieProviderRequestError::Network)?;
    if !output.status.success() {
        return Err(MovieProviderRequestError::Network);
    }
    parse_movie_provider_response(&output.stdout)
}

#[cfg(target_os = "windows")]
fn fetch_movie_provider_document(
    url: &str,
    tmdb_token: Option<&str>,
) -> Result<String, MovieProviderRequestError> {
    const MOVIE_PROVIDER_URL_ENV: &str = "AUTO_VIDEO_MOVIE_PROVIDER_URL";
    const TMDB_TOKEN_ENV: &str = "AUTO_VIDEO_TMDB_TOKEN";
    const WINDOWS_MOVIE_PROVIDER_SCRIPT: &str = r#"$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
try {
  $headers = @{ Accept = 'application/json'; 'User-Agent' = 'Auto-Video/0.1' }
  if (-not [string]::IsNullOrEmpty($env:AUTO_VIDEO_TMDB_TOKEN)) {
    $headers.Authorization = 'Bearer ' + $env:AUTO_VIDEO_TMDB_TOKEN
  }
  $response = Invoke-WebRequest -UseBasicParsing -Uri $env:AUTO_VIDEO_MOVIE_PROVIDER_URL -Headers $headers -MaximumRedirection 0 -TimeoutSec 20
  [Console]::Out.Write($response.Content)
  [Console]::Out.Write("`nAUTO_VIDEO_HTTP_STATUS:" + [int]$response.StatusCode)
} catch {
  $status = if ($null -eq $_.Exception.Response) { 0 } else { [int]$_.Exception.Response.StatusCode }
  [Console]::Out.Write("`nAUTO_VIDEO_HTTP_STATUS:" + $status)
}"#;
    let mut command = Command::new("powershell.exe");
    command
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
        .arg(WINDOWS_MOVIE_PROVIDER_SCRIPT)
        .env(MOVIE_PROVIDER_URL_ENV, url);
    if let Some(token) = tmdb_token {
        command.env(TMDB_TOKEN_ENV, token);
    } else {
        command.env_remove(TMDB_TOKEN_ENV);
    }
    let output = command
        .output()
        .map_err(|_| MovieProviderRequestError::Network)?;
    if !output.status.success() {
        return Err(MovieProviderRequestError::Network);
    }
    parse_movie_provider_response(&output.stdout)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn fetch_movie_provider_document(
    _url: &str,
    _tmdb_token: Option<&str>,
) -> Result<String, MovieProviderRequestError> {
    Err(MovieProviderRequestError::Network)
}

fn tmdb_movie_provider_error_code(error: MovieProviderRequestError) -> &'static str {
    match error {
        MovieProviderRequestError::Unauthorized => MOVIE_TMDB_UNAUTHORIZED,
        MovieProviderRequestError::RateLimited => MOVIE_TMDB_RATE_LIMITED,
        MovieProviderRequestError::Network => MOVIE_TMDB_NETWORK_ERROR,
        MovieProviderRequestError::SourceUnavailable | MovieProviderRequestError::Provider => {
            MOVIE_TMDB_PROVIDER_ERROR
        }
    }
}

fn yts_movie_provider_error_code(error: MovieProviderRequestError) -> &'static str {
    match error {
        MovieProviderRequestError::SourceUnavailable => MOVIE_YTS_SOURCE_UNAVAILABLE,
        MovieProviderRequestError::Network => MOVIE_YTS_NETWORK_ERROR,
        MovieProviderRequestError::Unauthorized
        | MovieProviderRequestError::RateLimited
        | MovieProviderRequestError::Provider => MOVIE_YTS_PROVIDER_ERROR,
    }
}

fn fetch_yts_movie_releases_with(
    state: &MovieTorrentState,
    generation: u64,
    tmdb_movie_id: u64,
    tmdb_token: &str,
    mut request: impl FnMut(&str, Option<&str>) -> Result<String, MovieProviderRequestError>,
) -> Result<Vec<String>, &'static str> {
    if tmdb_movie_id == 0 || !is_valid_tmdb_token(tmdb_token) {
        return Err(MOVIE_TMDB_MALFORMED);
    }
    let details_url = format!("{TMDB_MOVIE_URL}{tmdb_movie_id}");
    let details =
        request(&details_url, Some(tmdb_token)).map_err(tmdb_movie_provider_error_code)?;
    let external_ids_url = format!("{details_url}/external_ids");
    let external_ids =
        request(&external_ids_url, Some(tmdb_token)).map_err(tmdb_movie_provider_error_code)?;
    let imdb_id = verified_movie_imdb_id(tmdb_movie_id, &details, &external_ids)?;
    let yts = request(&format!("{YTS_MOVIES_URL}{imdb_id}"), None)
        .map_err(yts_movie_provider_error_code)?;
    state.finish_release_lookup(generation, tmdb_movie_id, &details, &external_ids, &yts)
}

fn fetch_javdb_vr_catalog_with(
    code: &str,
    request: impl FnOnce(&str) -> Result<String, ProviderRequestError>,
) -> Result<String, &'static str> {
    if !is_canonical_product_code(code) {
        return Err(VR_PROVIDER_ERROR);
    }

    request(&format!("{JAVDB_CATALOG_URL}{code}&f=all")).map_err(provider_error_code)
}

fn fetch_sukebei_vr_releases_with(
    code: &str,
    request: impl FnOnce(&str) -> Result<String, ProviderRequestError>,
) -> Result<String, &'static str> {
    if !is_canonical_product_code(code) {
        return Err(VR_PROVIDER_ERROR);
    }

    request(&format!("{SUKEBEI_RELEASES_URL}{code}%22&c=0_0&f=0")).map_err(provider_error_code)
}

fn fetch_javdb_adult_catalog_with(
    code: &str,
    request: impl FnOnce(&str) -> Result<String, ProviderRequestError>,
) -> Result<String, &'static str> {
    if !is_canonical_product_code(code) {
        return Err(ADULT_PROVIDER_ERROR);
    }

    request(&format!("{JAVDB_CATALOG_URL}{code}&f=all")).map_err(adult_provider_error_code)
}

fn fetch_sukebei_adult_releases_with(
    code: &str,
    request: impl FnOnce(&str) -> Result<String, ProviderRequestError>,
) -> Result<String, &'static str> {
    if !is_canonical_product_code(code) {
        return Err(ADULT_PROVIDER_ERROR);
    }

    request(&format!("{SUKEBEI_RELEASES_URL}{code}%22&c=0_0&f=0"))
        .map_err(adult_provider_error_code)
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

fn adult_folder_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|directory| directory.join(ADULT_FOLDER_FILE_NAME))
        .map_err(|_| ADULT_FOLDER_STORAGE_FAILED.to_owned())
}

fn tv_folder_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|directory| directory.join(TV_FOLDER_FILE_NAME))
        .map_err(|_| TV_FOLDER_STORAGE_FAILED.to_owned())
}

fn vr_folder_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|directory| directory.join(VR_FOLDER_FILE_NAME))
        .map_err(|_| VR_FOLDER_STORAGE_FAILED.to_owned())
}

fn vr_downloads_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|directory| directory.join(VR_DOWNLOADS_FILE_NAME))
        .map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED.to_owned())
}

fn vr_download_limit_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|directory| directory.join(VR_DOWNLOAD_LIMIT_FILE_NAME))
        .map_err(|_| VR_DOWNLOAD_LIMIT_STORAGE_FAILED.to_owned())
}

fn vr_session_folder(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|directory| directory.join(VR_SESSION_FOLDER_NAME))
        .map_err(|_| VR_DOWNLOAD_FAILED.to_owned())
}

#[tauri::command]
fn load_movies_folder(
    app: tauri::AppHandle,
    state: tauri::State<'_, MoviesLibraryState>,
    download_state: tauri::State<'_, VrDownloadState>,
) -> Result<Option<String>, String> {
    let folder = load_movies_folder_file(&movies_folder_path(&app)?)?;
    configure_movie_download_folder(download_state.inner(), folder.clone())
        .map_err(str::to_owned)?;
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
    download_state: tauri::State<'_, VrDownloadState>,
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
    let selected_folder = selected_folder
        .into_path()
        .map_err(|_| MOVIES_FOLDER_UNAVAILABLE.to_owned())?;
    let folder =
        fs::canonicalize(selected_folder).map_err(|_| MOVIES_FOLDER_UNAVAILABLE.to_owned())?;
    let metadata = fs::metadata(&folder).map_err(|_| MOVIES_FOLDER_UNAVAILABLE.to_owned())?;
    if !metadata.is_dir() {
        return Err(MOVIES_FOLDER_UNAVAILABLE.to_owned());
    }
    let response = folder
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| MOVIES_FOLDER_UNAVAILABLE.to_owned())?;

    save_movies_folder_file(&movies_folder_path(&app)?, &folder)?;
    configure_movie_download_folder(download_state.inner(), Some(folder.clone()))
        .map_err(str::to_owned)?;
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
    download_state: tauri::State<'_, VrDownloadState>,
) -> Result<(), String> {
    clear_movies_folder_file(&movies_folder_path(&app)?)?;
    configure_movie_download_folder(download_state.inner(), None).map_err(str::to_owned)?;
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
fn load_tv_folder(
    app: tauri::AppHandle,
    state: tauri::State<'_, TvLibraryState>,
    download_state: tauri::State<'_, VrDownloadState>,
) -> Result<Vec<String>, String> {
    let response = match load_tv_folder_with(state.inner(), &tv_folder_path(&app)?) {
        Ok(response) => response,
        Err(error) => {
            let _ = configure_tv_download_folder(download_state.inner(), None);
            return Err(error.to_owned());
        }
    };
    configure_tv_download_folder(
        download_state.inner(),
        configured_tv_folder(state.inner()).map_err(str::to_owned)?,
    )
    .map_err(str::to_owned)?;
    Ok(response)
}

#[tauri::command]
async fn choose_tv_folder(
    app: tauri::AppHandle,
    state: tauri::State<'_, TvLibraryState>,
    download_state: tauri::State<'_, VrDownloadState>,
) -> Result<Option<String>, String> {
    let dialog_app = app.clone();
    let selected_folder = tauri::async_runtime::spawn_blocking(move || {
        dialog_app
            .dialog()
            .file()
            .set_title("Choose TV folder")
            .blocking_pick_folder()
    })
    .await
    .map_err(|_| TV_FOLDER_UNAVAILABLE.to_owned())?;
    let Some(selected_folder) = selected_folder else {
        return Ok(None);
    };
    let folder = selected_folder
        .into_path()
        .map_err(|_| TV_FOLDER_UNAVAILABLE.to_owned())?;
    let folder =
        set_tv_folder(state.inner(), &tv_folder_path(&app)?, folder).map_err(str::to_owned)?;
    configure_tv_download_folder(
        download_state.inner(),
        configured_tv_folder(state.inner()).map_err(str::to_owned)?,
    )
    .map_err(str::to_owned)?;
    Ok(Some(folder))
}

#[tauri::command]
fn clear_tv_folder(
    app: tauri::AppHandle,
    state: tauri::State<'_, TvLibraryState>,
    download_state: tauri::State<'_, VrDownloadState>,
) -> Result<(), String> {
    clear_trusted_tv_folder(state.inner(), &tv_folder_path(&app)?).map_err(str::to_owned)?;
    configure_tv_download_folder(download_state.inner(), None).map_err(str::to_owned)
}

#[tauri::command]
async fn scan_tv_library(state: tauri::State<'_, TvLibraryState>) -> Result<Vec<String>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        scan_tv_library_with(&state).map_err(str::to_owned)
    })
    .await
    .map_err(|_| TV_LIBRARY_SCAN_FAILED.to_owned())?
}

#[tauri::command]
async fn query_tv_storage(state: tauri::State<'_, TvLibraryState>) -> Result<[String; 2], String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let folder = configured_tv_folder(&state).map_err(str::to_owned)?;
        let folder = folder
            .as_deref()
            .ok_or_else(|| TV_STORAGE_UNAVAILABLE.to_owned())?;
        if fs::canonicalize(folder)
            .ok()
            .as_deref()
            .is_none_or(|canonical_folder| canonical_folder != folder)
        {
            return Err(TV_STORAGE_UNAVAILABLE.to_owned());
        }
        let [total_bytes, free_bytes] = query_volume_storage_with(
            Some(folder),
            TV_STORAGE_UNAVAILABLE,
            TV_STORAGE_FAILED,
            query_movies_volume_storage,
        )
        .map_err(str::to_owned)?;
        Ok([total_bytes.to_string(), free_bytes.to_string()])
    })
    .await
    .map_err(|_| TV_STORAGE_FAILED.to_owned())?
}

#[tauri::command]
async fn open_tv_file(path: String, state: tauri::State<'_, TvLibraryState>) -> Result<(), String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        open_tv_file_with(Path::new(&path), &state, |file_path| {
            tauri_plugin_opener::open_path(file_path, None::<&str>).map_err(|_| ())
        })
    })
    .await
    .map_err(|_| TV_FILE_OPEN_FAILED.to_owned())?
    .map_err(str::to_owned)
}

#[tauri::command]
async fn reveal_tv_file(
    path: String,
    state: tauri::State<'_, TvLibraryState>,
) -> Result<(), String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        reveal_tv_file_with(Path::new(&path), &state, |file_path| {
            tauri_plugin_opener::reveal_item_in_dir(file_path).map_err(|_| ())
        })
    })
    .await
    .map_err(|_| TV_FILE_REVEAL_FAILED.to_owned())?
    .map_err(str::to_owned)
}

#[tauri::command]
async fn trash_tv_file(
    path: String,
    scan_generation: String,
    download_state: tauri::State<'_, VrDownloadState>,
    library_state: tauri::State<'_, TvLibraryState>,
) -> Result<(), String> {
    let scan_generation = scan_generation
        .parse::<u64>()
        .map_err(|_| tv_library::TV_FILE_TRASH_STALE.to_owned())?;
    let download_state = download_state.inner().clone();
    let library_state = library_state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        trash_tv_file_with_download_ownership(
            Path::new(&path),
            scan_generation,
            &download_state,
            &library_state,
            move_to_os_trash,
        )
    })
    .await
    .map_err(|_| TV_FILE_TRASH_FAILED.to_owned())?
    .map_err(str::to_owned)
}

#[tauri::command]
fn load_adult_folder(
    app: tauri::AppHandle,
    state: tauri::State<'_, AdultLibraryState>,
    download_state: tauri::State<'_, VrDownloadState>,
) -> Result<Vec<String>, String> {
    let response = match load_adult_folder_with(state.inner(), &adult_folder_path(&app)?) {
        Ok(response) => response,
        Err(error) => {
            let _ = configure_adult_download_folder(download_state.inner(), None);
            return Err(error.to_owned());
        }
    };
    configure_adult_download_folder(
        download_state.inner(),
        configured_adult_folder(state.inner()).map_err(str::to_owned)?,
    )
    .map_err(str::to_owned)?;
    Ok(response)
}

#[tauri::command]
async fn choose_adult_folder(
    app: tauri::AppHandle,
    state: tauri::State<'_, AdultLibraryState>,
    download_state: tauri::State<'_, VrDownloadState>,
) -> Result<Option<String>, String> {
    let dialog_app = app.clone();
    let selected_folder = tauri::async_runtime::spawn_blocking(move || {
        dialog_app
            .dialog()
            .file()
            .set_title("Choose Adult folder")
            .blocking_pick_folder()
    })
    .await
    .map_err(|_| ADULT_FOLDER_UNAVAILABLE.to_owned())?;
    let Some(selected_folder) = selected_folder else {
        return Ok(None);
    };
    let folder = selected_folder
        .into_path()
        .map_err(|_| ADULT_FOLDER_UNAVAILABLE.to_owned())?;
    let response = set_adult_folder(state.inner(), &adult_folder_path(&app)?, folder)
        .map_err(str::to_owned)?;
    configure_adult_download_folder(
        download_state.inner(),
        configured_adult_folder(state.inner()).map_err(str::to_owned)?,
    )
    .map_err(str::to_owned)?;
    Ok(Some(response))
}

#[tauri::command]
fn clear_adult_folder(
    app: tauri::AppHandle,
    state: tauri::State<'_, AdultLibraryState>,
    download_state: tauri::State<'_, VrDownloadState>,
) -> Result<(), String> {
    clear_trusted_adult_folder(state.inner(), &adult_folder_path(&app)?).map_err(str::to_owned)?;
    configure_adult_download_folder(download_state.inner(), None).map_err(str::to_owned)
}

#[tauri::command]
async fn scan_adult_library(
    state: tauri::State<'_, AdultLibraryState>,
) -> Result<Vec<String>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        scan_adult_library_with(&state).map_err(str::to_owned)
    })
    .await
    .map_err(|_| ADULT_LIBRARY_SCAN_FAILED.to_owned())?
}

#[tauri::command]
async fn query_adult_storage(
    state: tauri::State<'_, AdultLibraryState>,
) -> Result<[String; 2], String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let folder = configured_adult_folder(&state).map_err(str::to_owned)?;
        let folder = folder
            .as_deref()
            .ok_or_else(|| ADULT_STORAGE_UNAVAILABLE.to_owned())?;
        if fs::canonicalize(folder)
            .ok()
            .as_deref()
            .is_none_or(|canonical_folder| canonical_folder != folder)
        {
            return Err(ADULT_STORAGE_UNAVAILABLE.to_owned());
        }
        let [total_bytes, free_bytes] = query_volume_storage_with(
            Some(folder),
            ADULT_STORAGE_UNAVAILABLE,
            ADULT_STORAGE_FAILED,
            query_movies_volume_storage,
        )
        .map_err(str::to_owned)?;
        Ok([total_bytes.to_string(), free_bytes.to_string()])
    })
    .await
    .map_err(|_| ADULT_STORAGE_FAILED.to_owned())?
}

#[tauri::command]
async fn open_adult_file(
    path: String,
    state: tauri::State<'_, AdultLibraryState>,
) -> Result<(), String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        open_adult_file_with(Path::new(&path), &state, |file_path| {
            tauri_plugin_opener::open_path(file_path, None::<&str>).map_err(|_| ())
        })
    })
    .await
    .map_err(|_| ADULT_FILE_OPEN_FAILED.to_owned())?
    .map_err(str::to_owned)
}

#[tauri::command]
async fn reveal_adult_file(
    path: String,
    state: tauri::State<'_, AdultLibraryState>,
) -> Result<(), String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        reveal_adult_file_with(Path::new(&path), &state, |file_path| {
            tauri_plugin_opener::reveal_item_in_dir(file_path).map_err(|_| ())
        })
    })
    .await
    .map_err(|_| ADULT_FILE_REVEAL_FAILED.to_owned())?
    .map_err(str::to_owned)
}

#[tauri::command]
async fn trash_adult_file(
    path: String,
    scan_generation: String,
    state: tauri::State<'_, AdultLibraryState>,
) -> Result<(), String> {
    let scan_generation = scan_generation
        .parse::<u64>()
        .map_err(|_| adult_library::ADULT_FILE_TRASH_STALE.to_owned())?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        trash_adult_file_with(Path::new(&path), scan_generation, &state, move_to_os_trash)
    })
    .await
    .map_err(|_| ADULT_FILE_TRASH_FAILED.to_owned())?
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
fn save_tmdb_token(
    app: tauri::AppHandle,
    token: String,
    tv_release_state: tauri::State<'_, TvReleaseState>,
    tv_torrent_state: tauri::State<'_, TvTorrentState>,
) -> Result<(), String> {
    save_tmdb_token_file(&tmdb_token_path(&app)?, &token).map_err(str::to_owned)?;
    tv_release_state.invalidate().map_err(str::to_owned)?;
    tv_torrent_state
        .invalidate_inspection()
        .map_err(str::to_owned)
}

#[tauri::command]
fn clear_tmdb_token(
    app: tauri::AppHandle,
    tv_release_state: tauri::State<'_, TvReleaseState>,
    tv_torrent_state: tauri::State<'_, TvTorrentState>,
) -> Result<(), String> {
    clear_tmdb_token_file(&tmdb_token_path(&app)?).map_err(str::to_owned)?;
    tv_release_state.invalidate().map_err(str::to_owned)?;
    tv_torrent_state
        .invalidate_inspection()
        .map_err(str::to_owned)
}

#[tauri::command]
fn load_vr_folder(
    app: tauri::AppHandle,
    state: tauri::State<'_, VrDownloadState>,
    library_state: tauri::State<'_, VrLibraryState>,
) -> Result<Vec<String>, String> {
    let response =
        load_vr_folder_with(state.inner(), &vr_folder_path(&app)?).map_err(str::to_owned)?;
    invalidate_vr_library(library_state.inner()).map_err(str::to_owned)?;
    Ok(response)
}

#[tauri::command]
async fn choose_vr_folder(
    app: tauri::AppHandle,
    state: tauri::State<'_, VrDownloadState>,
    library_state: tauri::State<'_, VrLibraryState>,
) -> Result<Option<String>, String> {
    let dialog_app = app.clone();
    let selected_folder = tauri::async_runtime::spawn_blocking(move || {
        dialog_app
            .dialog()
            .file()
            .set_title("Choose VR folder")
            .blocking_pick_folder()
    })
    .await
    .map_err(|_| VR_FOLDER_UNAVAILABLE.to_owned())?;
    let Some(selected_folder) = selected_folder else {
        return Ok(None);
    };
    let folder = selected_folder
        .into_path()
        .map_err(|_| VR_FOLDER_UNAVAILABLE.to_owned())?;
    let folder =
        set_vr_folder(state.inner(), &vr_folder_path(&app)?, folder).map_err(str::to_owned)?;
    invalidate_vr_library(library_state.inner()).map_err(str::to_owned)?;
    Ok(Some(folder))
}

#[tauri::command]
fn clear_vr_folder(
    app: tauri::AppHandle,
    state: tauri::State<'_, VrDownloadState>,
    library_state: tauri::State<'_, VrLibraryState>,
) -> Result<(), String> {
    clear_trusted_vr_folder(state.inner(), &vr_folder_path(&app)?).map_err(str::to_owned)?;
    invalidate_vr_library(library_state.inner()).map_err(str::to_owned)
}

#[tauri::command]
async fn scan_vr_library(
    download_state: tauri::State<'_, VrDownloadState>,
    library_state: tauri::State<'_, VrLibraryState>,
) -> Result<Vec<String>, String> {
    let download_state = download_state.inner().clone();
    let library_state = library_state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        scan_vr_library_with(&download_state, &library_state).map_err(str::to_owned)
    })
    .await
    .map_err(|_| VR_LIBRARY_SCAN_FAILED.to_owned())?
}

#[tauri::command]
async fn query_vr_storage(state: tauri::State<'_, VrDownloadState>) -> Result<[String; 2], String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let folder = configured_vr_folder(&state).map_err(str::to_owned)?;
        let folder = folder
            .as_deref()
            .ok_or_else(|| VR_STORAGE_UNAVAILABLE.to_owned())?;
        if fs::canonicalize(folder)
            .ok()
            .as_deref()
            .is_none_or(|canonical_folder| canonical_folder != folder)
        {
            return Err(VR_STORAGE_UNAVAILABLE.to_owned());
        }
        let [total_bytes, free_bytes] = query_volume_storage_with(
            Some(folder),
            VR_STORAGE_UNAVAILABLE,
            VR_STORAGE_FAILED,
            query_movies_volume_storage,
        )
        .map_err(str::to_owned)?;
        Ok([total_bytes.to_string(), free_bytes.to_string()])
    })
    .await
    .map_err(|_| VR_STORAGE_FAILED.to_owned())?
}

#[tauri::command]
async fn open_vr_file(
    path: String,
    download_state: tauri::State<'_, VrDownloadState>,
    library_state: tauri::State<'_, VrLibraryState>,
) -> Result<(), String> {
    let download_state = download_state.inner().clone();
    let library_state = library_state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        open_vr_file_with(
            Path::new(&path),
            &download_state,
            &library_state,
            |file_path| tauri_plugin_opener::open_path(file_path, None::<&str>).map_err(|_| ()),
        )
    })
    .await
    .map_err(|_| VR_FILE_OPEN_FAILED.to_owned())?
    .map_err(str::to_owned)
}

#[tauri::command]
async fn reveal_vr_file(
    path: String,
    download_state: tauri::State<'_, VrDownloadState>,
    library_state: tauri::State<'_, VrLibraryState>,
) -> Result<(), String> {
    let download_state = download_state.inner().clone();
    let library_state = library_state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        reveal_vr_file_with(
            Path::new(&path),
            &download_state,
            &library_state,
            |file_path| tauri_plugin_opener::reveal_item_in_dir(file_path).map_err(|_| ()),
        )
    })
    .await
    .map_err(|_| VR_FILE_REVEAL_FAILED.to_owned())?
    .map_err(str::to_owned)
}

#[tauri::command]
async fn trash_vr_file(
    path: String,
    scan_generation: String,
    download_state: tauri::State<'_, VrDownloadState>,
    library_state: tauri::State<'_, VrLibraryState>,
) -> Result<(), String> {
    let scan_generation = scan_generation
        .parse::<u64>()
        .map_err(|_| vr_library::VR_FILE_TRASH_STALE.to_owned())?;
    let download_state = download_state.inner().clone();
    let library_state = library_state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        trash_vr_file_with(
            Path::new(&path),
            scan_generation,
            &download_state,
            &library_state,
            move_to_os_trash,
        )
    })
    .await
    .map_err(|_| VR_FILE_TRASH_FAILED.to_owned())?
    .map_err(str::to_owned)
}

#[tauri::command]
fn load_vr_download_limit(
    app: tauri::AppHandle,
    state: tauri::State<'_, VrDownloadState>,
) -> Result<Vec<String>, String> {
    load_download_limit(state.inner(), &vr_download_limit_path(&app)?).map_err(str::to_owned)
}

#[tauri::command]
fn save_vr_download_limit(
    app: tauri::AppHandle,
    mib_per_second: Option<String>,
    state: tauri::State<'_, VrDownloadState>,
) -> Result<Vec<String>, String> {
    save_download_limit(
        state.inner(),
        &vr_download_limit_path(&app)?,
        mib_per_second.as_deref(),
    )
    .map_err(str::to_owned)
}

#[tauri::command]
async fn load_vr_downloads(
    app: tauri::AppHandle,
    state: tauri::State<'_, VrDownloadState>,
) -> Result<Vec<String>, String> {
    load_downloads(
        state.inner(),
        &vr_downloads_path(&app)?,
        &vr_session_folder(&app)?,
        &vr_download_limit_path(&app)?,
    )
    .await
    .map_err(str::to_owned)
}

#[tauri::command]
fn list_vr_downloads(
    app: tauri::AppHandle,
    state: tauri::State<'_, VrDownloadState>,
) -> Result<Vec<String>, String> {
    list_downloads(state.inner(), &vr_downloads_path(&app)?).map_err(str::to_owned)
}

#[tauri::command]
async fn start_verified_vr_download(
    app: tauri::AppHandle,
    inspection_id: String,
    selected_file_ids: Vec<usize>,
    download_state: tauri::State<'_, VrDownloadState>,
    torrent_state: tauri::State<'_, VrTorrentState>,
) -> Result<String, String> {
    start_download(
        download_state.inner(),
        torrent_state.inner(),
        &vr_downloads_path(&app)?,
        &vr_session_folder(&app)?,
        &inspection_id,
        &selected_file_ids,
    )
    .await
    .map_err(str::to_owned)
}

#[tauri::command]
async fn start_verified_adult_download(
    app: tauri::AppHandle,
    inspection_id: String,
    selected_file_ids: Vec<usize>,
    download_state: tauri::State<'_, VrDownloadState>,
    torrent_state: tauri::State<'_, AdultTorrentState>,
) -> Result<String, String> {
    start_adult_download(
        download_state.inner(),
        torrent_state.inner(),
        &vr_downloads_path(&app)?,
        &vr_session_folder(&app)?,
        &inspection_id,
        &selected_file_ids,
    )
    .await
    .map_err(str::to_owned)
}

#[tauri::command]
async fn start_verified_movie_download(
    app: tauri::AppHandle,
    inspection_id: String,
    selected_file_ids: Vec<usize>,
    download_state: tauri::State<'_, VrDownloadState>,
    torrent_state: tauri::State<'_, MovieTorrentState>,
) -> Result<String, String> {
    start_movie_download(
        download_state.inner(),
        torrent_state.inner(),
        &vr_downloads_path(&app)?,
        &vr_session_folder(&app)?,
        &inspection_id,
        &selected_file_ids,
    )
    .await
    .map_err(str::to_owned)
}

#[tauri::command]
async fn start_verified_tv_download(
    app: tauri::AppHandle,
    inspection_id: String,
    selected_file_ids: Vec<usize>,
    download_state: tauri::State<'_, VrDownloadState>,
    torrent_state: tauri::State<'_, TvTorrentState>,
    release_state: tauri::State<'_, TvReleaseState>,
) -> Result<String, String> {
    start_tv_download(
        download_state.inner(),
        torrent_state.inner(),
        release_state.inner(),
        &vr_downloads_path(&app)?,
        &vr_session_folder(&app)?,
        &inspection_id,
        &selected_file_ids,
    )
    .await
    .map_err(str::to_owned)
}

#[tauri::command]
async fn pause_vr_download(
    app: tauri::AppHandle,
    transfer_id: String,
    state: tauri::State<'_, VrDownloadState>,
) -> Result<(), String> {
    pause_download(state.inner(), &vr_downloads_path(&app)?, &transfer_id)
        .await
        .map_err(str::to_owned)
}

#[tauri::command]
async fn resume_vr_download(
    app: tauri::AppHandle,
    transfer_id: String,
    state: tauri::State<'_, VrDownloadState>,
) -> Result<(), String> {
    resume_download(state.inner(), &vr_downloads_path(&app)?, &transfer_id)
        .await
        .map_err(str::to_owned)
}

#[tauri::command]
async fn cancel_vr_download(
    app: tauri::AppHandle,
    transfer_id: String,
    state: tauri::State<'_, VrDownloadState>,
) -> Result<(), String> {
    cancel_download(state.inner(), &vr_downloads_path(&app)?, &transfer_id)
        .await
        .map_err(str::to_owned)
}

#[tauri::command]
fn dismiss_vr_download(
    app: tauri::AppHandle,
    transfer_id: String,
    state: tauri::State<'_, VrDownloadState>,
) -> Result<(), String> {
    dismiss_download(state.inner(), &vr_downloads_path(&app)?, &transfer_id).map_err(str::to_owned)
}

#[tauri::command]
async fn preview_vr_organization(
    transfer_id: String,
    state: tauri::State<'_, VrDownloadState>,
) -> Result<Vec<String>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        preview_organization(&state, &transfer_id).map_err(str::to_owned)
    })
    .await
    .map_err(|_| vr_download::VR_ORGANIZATION_FAILED.to_owned())?
}

#[tauri::command]
async fn apply_vr_organization(
    app: tauri::AppHandle,
    plan_id: String,
    state: tauri::State<'_, VrDownloadState>,
) -> Result<(), String> {
    let state = state.inner().clone();
    let persistence_path = vr_downloads_path(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        apply_organization(&state, &persistence_path, &plan_id).map_err(str::to_owned)
    })
    .await
    .map_err(|_| vr_download::VR_ORGANIZATION_FAILED.to_owned())?
}

#[tauri::command]
fn dismiss_vr_organization(state: tauri::State<'_, VrDownloadState>) -> Result<(), String> {
    dismiss_organization(state.inner()).map_err(str::to_owned)
}

#[tauri::command]
async fn fetch_javdb_vr_catalog(code: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        fetch_javdb_vr_catalog_with(&code, fetch_provider_document).map_err(str::to_owned)
    })
    .await
    .map_err(|_| VR_PROVIDER_ERROR.to_owned())?
}

#[tauri::command]
async fn fetch_javdb_adult_catalog(code: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        fetch_javdb_adult_catalog_with(&code, fetch_provider_document).map_err(str::to_owned)
    })
    .await
    .map_err(|_| ADULT_PROVIDER_ERROR.to_owned())?
}

#[tauri::command]
async fn fetch_sukebei_adult_releases(
    code: String,
    state: tauri::State<'_, AdultTorrentState>,
) -> Result<String, String> {
    let state = state.inner().clone();
    let generation = state.begin_release_lookup().map_err(str::to_owned)?;
    tauri::async_runtime::spawn_blocking(move || {
        let document = fetch_sukebei_adult_releases_with(&code, fetch_provider_document)
            .map_err(str::to_owned)?;
        state
            .finish_release_lookup(generation, &code, &document)
            .map_err(str::to_owned)?;
        Ok(document)
    })
    .await
    .map_err(|_| ADULT_PROVIDER_ERROR.to_owned())?
}

#[tauri::command]
async fn fetch_sukebei_vr_releases(
    code: String,
    state: tauri::State<'_, VrTorrentState>,
) -> Result<String, String> {
    let state = state.inner().clone();
    let generation = state.begin_release_lookup().map_err(str::to_owned)?;
    tauri::async_runtime::spawn_blocking(move || {
        let document = fetch_sukebei_vr_releases_with(&code, fetch_provider_document)
            .map_err(str::to_owned)?;
        state
            .finish_release_lookup(generation, &code, &document)
            .map_err(str::to_owned)?;
        Ok(document)
    })
    .await
    .map_err(|_| VR_PROVIDER_ERROR.to_owned())?
}

#[tauri::command]
async fn fetch_yts_movie_releases(
    app: tauri::AppHandle,
    tmdb_movie_id: u64,
    state: tauri::State<'_, MovieTorrentState>,
) -> Result<Vec<String>, String> {
    let state = state.inner().clone();
    let generation = state.begin_release_lookup().map_err(str::to_owned)?;
    let token = load_tmdb_token_file(&tmdb_token_path(&app)?)
        .map_err(str::to_owned)?
        .ok_or_else(|| MOVIE_TMDB_UNAUTHORIZED.to_owned())?;
    tauri::async_runtime::spawn_blocking(move || {
        fetch_yts_movie_releases_with(
            &state,
            generation,
            tmdb_movie_id,
            &token,
            fetch_movie_provider_document,
        )
        .map_err(str::to_owned)
    })
    .await
    .map_err(|_| MOVIE_YTS_PROVIDER_ERROR.to_owned())?
}

#[tauri::command]
async fn fetch_apibay_tv_releases(
    app: tauri::AppHandle,
    tmdb_tv_id: u64,
    provider_season_id: u64,
    provider_episode_id: u64,
    release_state: tauri::State<'_, TvReleaseState>,
    torrent_state: tauri::State<'_, TvTorrentState>,
) -> Result<Vec<String>, String> {
    let release_state = release_state.inner().clone();
    let generation = release_state
        .begin_release_lookup()
        .map_err(str::to_owned)?;
    torrent_state
        .invalidate_inspection()
        .map_err(str::to_owned)?;
    if tmdb_tv_id == 0 || provider_season_id == 0 || provider_episode_id == 0 {
        return Err(TV_TMDB_MALFORMED.to_owned());
    }
    let token = load_tmdb_token_file(&tmdb_token_path(&app)?)
        .map_err(str::to_owned)?
        .ok_or_else(|| TV_TMDB_UNAUTHORIZED.to_owned())?;
    if !is_valid_tmdb_token(&token) {
        return Err(TV_TMDB_MALFORMED.to_owned());
    }
    tauri::async_runtime::spawn_blocking(move || {
        fetch_apibay_tv_releases_for_state_with(
            &release_state,
            generation,
            tmdb_tv_id,
            provider_season_id,
            provider_episode_id,
            &token,
            fetch_movie_provider_document,
        )
        .map_err(str::to_owned)
    })
    .await
    .map_err(|_| TV_APIBAY_PROVIDER_ERROR.to_owned())?
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn inspect_apibay_tv_torrent(
    app: tauri::AppHandle,
    tmdb_tv_id: u64,
    provider_season_id: u64,
    provider_episode_id: u64,
    provider_item_id: String,
    release_state: tauri::State<'_, TvReleaseState>,
    torrent_state: tauri::State<'_, TvTorrentState>,
    download_state: tauri::State<'_, VrDownloadState>,
) -> Result<Vec<String>, String> {
    let release_state = release_state.inner().clone();
    let torrent_state = torrent_state.inner().clone();
    let plan = match torrent_state
        .begin_inspection(
            &release_state,
            tmdb_tv_id,
            provider_season_id,
            provider_episode_id,
            &provider_item_id,
        )
        .map_err(str::to_owned)?
    {
        TvTorrentInspectionStart::Cached(response) => return Ok(response),
        TvTorrentInspectionStart::Acquire(plan) => plan,
    };
    let bytes = acquire_tv_metainfo(
        download_state.inner(),
        &vr_session_folder(&app)?,
        plan.expected_infohash(),
    )
    .await
    .map_err(|error| {
        match error {
            TvMetainfoAcquisitionError::LocalPending => TV_TORRENT_LOCAL_PENDING,
            TvMetainfoAcquisitionError::LocalUnavailable => TV_TORRENT_LOCAL_UNAVAILABLE,
            TvMetainfoAcquisitionError::Network => TV_TORRENT_NETWORK_ERROR,
            TvMetainfoAcquisitionError::NoMetadataSource => TV_TORRENT_NO_METADATA_SOURCE,
            TvMetainfoAcquisitionError::Timeout => TV_TORRENT_TIMEOUT,
        }
        .to_owned()
    })?;
    torrent_state
        .finish_inspection(&release_state, plan, bytes)
        .map_err(str::to_owned)
}

#[tauri::command]
fn select_apibay_tv_release(
    tmdb_tv_id: u64,
    provider_season_id: u64,
    provider_episode_id: u64,
    provider_item_id: String,
    release_state: tauri::State<'_, TvReleaseState>,
    torrent_state: tauri::State<'_, TvTorrentState>,
) -> Result<(), String> {
    let changed = release_state
        .select_release(
            tmdb_tv_id,
            provider_season_id,
            provider_episode_id,
            &provider_item_id,
        )
        .map_err(|_| TV_TORRENT_CONTEXT_INVALID.to_owned())?;
    if changed {
        torrent_state
            .invalidate_inspection()
            .map_err(str::to_owned)?;
    }
    Ok(())
}

#[tauri::command]
fn invalidate_verified_tv_torrent(state: tauri::State<'_, TvTorrentState>) -> Result<(), String> {
    state.invalidate_inspection().map_err(str::to_owned)
}

#[tauri::command]
fn invalidate_tv_release_context(
    release_state: tauri::State<'_, TvReleaseState>,
    torrent_state: tauri::State<'_, TvTorrentState>,
) -> Result<(), String> {
    release_state.invalidate().map_err(str::to_owned)?;
    torrent_state.invalidate_inspection().map_err(str::to_owned)
}

#[tauri::command]
async fn inspect_sukebei_vr_torrent(
    code: String,
    release_name: String,
    provider_item_id: String,
    torrent_url: String,
    expected_infohash: String,
    state: tauri::State<'_, VrTorrentState>,
) -> Result<Vec<String>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        inspect_sukebei_torrent_with(
            &state,
            TorrentInspectionRequest {
                code,
                release_name,
                provider_item_id,
                torrent_url,
                expected_infohash,
            },
            fetch_artifact_response,
        )
        .map_err(str::to_owned)
    })
    .await
    .map_err(|_| VR_TORRENT_PROVIDER_ERROR.to_owned())?
}

#[tauri::command]
async fn inspect_sukebei_adult_torrent(
    code: String,
    release_name: String,
    provider_item_id: String,
    torrent_url: String,
    expected_infohash: String,
    state: tauri::State<'_, AdultTorrentState>,
) -> Result<Vec<String>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        inspect_sukebei_adult_torrent_with(
            &state,
            TorrentInspectionRequest {
                code,
                release_name,
                provider_item_id,
                torrent_url,
                expected_infohash,
            },
            fetch_artifact_response,
        )
        .map_err(str::to_owned)
    })
    .await
    .map_err(|_| ADULT_TORRENT_PROVIDER_ERROR.to_owned())?
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn inspect_yts_movie_torrent(
    tmdb_movie_id: u64,
    tmdb_title: String,
    release_date: Option<String>,
    imdb_id: String,
    provider_movie_id: u64,
    provider_title: Option<String>,
    provider_year: Option<String>,
    row_id: String,
    quality: Option<String>,
    type_label: Option<String>,
    video_codec: Option<String>,
    size: Option<String>,
    size_bytes: Option<String>,
    seeds: Option<String>,
    peers: Option<String>,
    expected_infohash: String,
    torrent_url: String,
    state: tauri::State<'_, MovieTorrentState>,
) -> Result<Vec<String>, String> {
    let state = state.inner().clone();
    let request = MovieTorrentInspectionRequest {
        tmdb_movie_id,
        tmdb_title,
        release_date,
        imdb_id,
        provider_movie_id,
        provider_title,
        provider_year,
        row_id,
        quality,
        type_label,
        video_codec,
        size,
        size_bytes,
        seeds,
        peers,
        expected_infohash,
        torrent_url,
    };
    let (release_generation, inspection_generation) =
        state.begin_inspection(&request).map_err(str::to_owned)?;
    tauri::async_runtime::spawn_blocking(move || {
        inspect_yts_movie_torrent_with(
            &state,
            release_generation,
            inspection_generation,
            request,
            fetch_artifact_response,
        )
        .map_err(str::to_owned)
    })
    .await
    .map_err(|_| MOVIE_TORRENT_PROVIDER_ERROR.to_owned())?
}

#[tauri::command]
fn invalidate_verified_vr_torrent(state: tauri::State<'_, VrTorrentState>) -> Result<(), String> {
    state.invalidate_inspection().map_err(str::to_owned)
}

#[tauri::command]
fn invalidate_verified_adult_torrent(
    state: tauri::State<'_, AdultTorrentState>,
) -> Result<(), String> {
    state.invalidate_inspection().map_err(str::to_owned)
}

#[tauri::command]
fn invalidate_verified_movie_torrent(
    state: tauri::State<'_, MovieTorrentState>,
) -> Result<(), String> {
    state.invalidate_inspection().map_err(str::to_owned)
}

#[tauri::command]
fn invalidate_movie_release_context(
    state: tauri::State<'_, MovieTorrentState>,
) -> Result<(), String> {
    state.invalidate_release_context().map_err(str::to_owned)
}

#[tauri::command]
async fn save_verified_vr_torrent(
    app: tauri::AppHandle,
    inspection_id: String,
    state: tauri::State<'_, VrTorrentState>,
) -> Result<bool, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        save_verified_torrent_with(
            &state,
            &inspection_id,
            |default_file_name| {
                app.dialog()
                    .file()
                    .set_title("Save verified torrent")
                    .add_filter("Torrent", &["torrent"])
                    .set_file_name(default_file_name)
                    .blocking_save_file()
                    .and_then(|path| path.into_path().ok())
            },
            |path, bytes| fs::write(path, bytes),
        )
        .map_err(str::to_owned)
    })
    .await
    .map_err(|_| VR_TORRENT_SAVE_FAILED.to_owned())?
}

#[tauri::command]
async fn save_verified_adult_torrent(
    app: tauri::AppHandle,
    inspection_id: String,
    state: tauri::State<'_, AdultTorrentState>,
) -> Result<bool, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        save_verified_adult_torrent_with(
            &state,
            &inspection_id,
            |default_file_name| {
                app.dialog()
                    .file()
                    .set_title("Save verified Adult torrent")
                    .add_filter("Torrent", &["torrent"])
                    .set_file_name(default_file_name)
                    .blocking_save_file()
                    .and_then(|path| path.into_path().ok())
            },
            write_new_torrent_file,
        )
        .map_err(str::to_owned)
    })
    .await
    .map_err(|_| ADULT_TORRENT_SAVE_FAILED.to_owned())?
}

#[tauri::command]
async fn save_verified_movie_torrent(
    app: tauri::AppHandle,
    inspection_id: String,
    state: tauri::State<'_, MovieTorrentState>,
) -> Result<bool, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        save_verified_movie_torrent_with(
            &state,
            &inspection_id,
            |default_file_name| {
                app.dialog()
                    .file()
                    .set_title("Save verified Movie torrent")
                    .add_filter("Torrent", &["torrent"])
                    .set_file_name(default_file_name)
                    .blocking_save_file()
                    .and_then(|path| path.into_path().ok())
            },
            write_new_torrent_file,
        )
        .map_err(str::to_owned)
    })
    .await
    .map_err(|_| MOVIE_TORRENT_SAVE_FAILED.to_owned())?
}

#[tauri::command]
async fn save_verified_tv_torrent(
    app: tauri::AppHandle,
    inspection_id: String,
    state: tauri::State<'_, TvTorrentState>,
) -> Result<bool, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        save_verified_tv_torrent_with(
            &state,
            &inspection_id,
            |default_file_name| {
                app.dialog()
                    .file()
                    .set_title("Save generated verified TV metainfo")
                    .add_filter("Torrent", &["torrent"])
                    .set_file_name(default_file_name)
                    .blocking_save_file()
                    .and_then(|path| path.into_path().ok())
            },
            write_new_torrent_file,
        )
        .map_err(str::to_owned)
    })
    .await
    .map_err(|_| TV_TORRENT_SAVE_FAILED.to_owned())?
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(MoviesLibraryState::default())
        .manage(MovieTorrentState::default())
        .manage(AdultLibraryState::default())
        .manage(AdultTorrentState::default())
        .manage(TvLibraryState::default())
        .manage(TvReleaseState::default())
        .manage(TvTorrentState::default())
        .manage(VrTorrentState::default())
        .manage(VrDownloadState::default())
        .manage(VrLibraryState::default())
        .invoke_handler(tauri::generate_handler![
            load_movies_folder,
            choose_movies_folder,
            clear_movies_folder,
            scan_movies,
            query_movies_storage,
            open_movie,
            reveal_movie,
            trash_movie,
            load_tv_folder,
            choose_tv_folder,
            clear_tv_folder,
            scan_tv_library,
            query_tv_storage,
            open_tv_file,
            reveal_tv_file,
            trash_tv_file,
            load_adult_folder,
            choose_adult_folder,
            clear_adult_folder,
            scan_adult_library,
            query_adult_storage,
            open_adult_file,
            reveal_adult_file,
            trash_adult_file,
            load_tmdb_token,
            save_tmdb_token,
            clear_tmdb_token,
            load_vr_folder,
            choose_vr_folder,
            clear_vr_folder,
            scan_vr_library,
            query_vr_storage,
            open_vr_file,
            reveal_vr_file,
            trash_vr_file,
            load_vr_download_limit,
            save_vr_download_limit,
            load_vr_downloads,
            list_vr_downloads,
            start_verified_vr_download,
            start_verified_adult_download,
            start_verified_movie_download,
            start_verified_tv_download,
            pause_vr_download,
            resume_vr_download,
            cancel_vr_download,
            dismiss_vr_download,
            preview_vr_organization,
            apply_vr_organization,
            dismiss_vr_organization,
            fetch_javdb_vr_catalog,
            fetch_javdb_adult_catalog,
            fetch_sukebei_adult_releases,
            fetch_sukebei_vr_releases,
            fetch_yts_movie_releases,
            fetch_apibay_tv_releases,
            select_apibay_tv_release,
            inspect_apibay_tv_torrent,
            inspect_sukebei_adult_torrent,
            inspect_sukebei_vr_torrent,
            inspect_yts_movie_torrent,
            invalidate_verified_adult_torrent,
            invalidate_verified_vr_torrent,
            invalidate_verified_movie_torrent,
            invalidate_verified_tv_torrent,
            invalidate_tv_release_context,
            invalidate_movie_release_context,
            save_verified_adult_torrent,
            save_verified_vr_torrent,
            save_verified_movie_torrent,
            save_verified_tv_torrent
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
        clear_movies_folder_file, clear_tmdb_token_file, fetch_javdb_adult_catalog_with,
        fetch_javdb_vr_catalog_with, fetch_sukebei_adult_releases_with,
        fetch_sukebei_vr_releases_with, fetch_yts_movie_releases_with, load_movies_folder_file,
        load_tmdb_token_file, movie_metadata_error, open_movie_path_with,
        parse_movie_provider_response, parse_provider_response, query_movies_volume_storage_with,
        reveal_movie_path_with, save_movies_folder_file, save_tmdb_token_file, scan_movie_paths,
        trash_movie_path_with, trash_movie_request_with, MoviePathValidationError,
        MovieProviderRequestError, MovieTorrentState, MoviesLibraryContext,
        MoviesVolumeStorageQueryError, ProviderRequestError, TrashMovieRequest,
        ADULT_PROVIDER_ERROR, MOVIES_FOLDER_UNAVAILABLE, MOVIES_STORAGE_FAILED,
        MOVIES_STORAGE_UNAVAILABLE, MOVIE_OPEN_FAILED, MOVIE_OPEN_NOT_FILE, MOVIE_OPEN_NOT_FOUND,
        MOVIE_OPEN_UNAVAILABLE, MOVIE_OPEN_UNSUPPORTED, MOVIE_REVEAL_FAILED, MOVIE_REVEAL_NOT_FILE,
        MOVIE_REVEAL_NOT_FOUND, MOVIE_REVEAL_UNAVAILABLE, MOVIE_REVEAL_UNSUPPORTED,
        MOVIE_TRASH_FAILED, MOVIE_TRASH_FOLDER_UNAVAILABLE, MOVIE_TRASH_NOT_FILE,
        MOVIE_TRASH_NOT_FOUND, MOVIE_TRASH_OUTSIDE_FOLDER, MOVIE_TRASH_STALE,
        MOVIE_TRASH_UNAVAILABLE, MOVIE_TRASH_UNSUPPORTED, TMDB_TOKEN_INVALID, VR_PROVIDER_ERROR,
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
    fn constructs_literal_exact_code_provider_requests() {
        let javdb_url = RefCell::new(None);
        let sukebei_url = RefCell::new(None);

        assert_eq!(
            fetch_javdb_vr_catalog_with("MDVR-419", |url| {
                javdb_url.replace(Some(url.to_owned()));
                Ok("catalog".to_owned())
            }),
            Ok("catalog".to_owned())
        );
        assert_eq!(
            fetch_sukebei_vr_releases_with("MDVR-419", |url| {
                sukebei_url.replace(Some(url.to_owned()));
                Ok("releases".to_owned())
            }),
            Ok("releases".to_owned())
        );
        assert_eq!(
            javdb_url.into_inner().as_deref(),
            Some("https://javdb.com/search?q=MDVR-419&f=all")
        );
        assert_eq!(
            sukebei_url.into_inner().as_deref(),
            Some("https://sukebei.nyaa.si/?page=rss&q=%22MDVR-419%22&c=0_0&f=0")
        );
    }

    #[test]
    fn constructs_literal_adult_exact_code_provider_requests() {
        let javdb_url = RefCell::new(None);
        let sukebei_url = RefCell::new(None);

        assert_eq!(
            fetch_javdb_adult_catalog_with("ADLT-123", |url| {
                javdb_url.replace(Some(url.to_owned()));
                Ok("catalog".to_owned())
            }),
            Ok("catalog".to_owned())
        );
        assert_eq!(
            fetch_sukebei_adult_releases_with("ADLT-123", |url| {
                sukebei_url.replace(Some(url.to_owned()));
                Ok("releases".to_owned())
            }),
            Ok("releases".to_owned())
        );
        assert_eq!(
            javdb_url.into_inner().as_deref(),
            Some("https://javdb.com/search?q=ADLT-123&f=all")
        );
        assert_eq!(
            sukebei_url.into_inner().as_deref(),
            Some("https://sukebei.nyaa.si/?page=rss&q=%22ADLT-123%22&c=0_0&f=0")
        );
    }

    #[test]
    fn resolves_tmdb_identity_before_the_literal_imdb_yts_request() {
        let state = MovieTorrentState::default();
        let generation = state
            .begin_release_lookup()
            .expect("Movie release lookup must begin");
        let requests = RefCell::new(Vec::new());
        let response = fetch_yts_movie_releases_with(
            &state,
            generation,
            419,
            "fixture-token",
            |url, token| {
                requests
                    .borrow_mut()
                    .push((url.to_owned(), token.map(str::to_owned)));
                match url {
                    "https://api.themoviedb.org/3/movie/419" => Ok(
                        r#"{"id":419,"title":"Exact Movie","release_date":"1999-04-19"}"#
                            .to_owned(),
                    ),
                    "https://api.themoviedb.org/3/movie/419/external_ids" => {
                        Ok(r#"{"id":419,"imdb_id":"tt0123456"}"#.to_owned())
                    }
                    "https://yts.mx/api/v2/list_movies.json?limit=50&query_term=tt0123456" => {
                        Ok(r#"{"status":"ok","data":{"movies":null}}"#.to_owned())
                    }
                    _ => unreachable!("only exact native-built provider URLs are allowed"),
                }
            },
        )
        .expect("exact Movie identity lookup must succeed");

        assert_eq!(response[0], "419");
        assert_eq!(response[1], "Exact Movie");
        assert_eq!(response[3], "tt0123456");
        assert_eq!(response[7], "0");
        assert_eq!(
            requests.into_inner(),
            vec![
                (
                    "https://api.themoviedb.org/3/movie/419".to_owned(),
                    Some("fixture-token".to_owned()),
                ),
                (
                    "https://api.themoviedb.org/3/movie/419/external_ids".to_owned(),
                    Some("fixture-token".to_owned()),
                ),
                (
                    "https://yts.mx/api/v2/list_movies.json?limit=50&query_term=tt0123456"
                        .to_owned(),
                    None,
                ),
            ]
        );
    }

    #[test]
    fn maps_movie_provider_http_failures_without_exposing_response_text() {
        for (status, error) in [
            (401, MovieProviderRequestError::Unauthorized),
            (429, MovieProviderRequestError::RateLimited),
            (404, MovieProviderRequestError::SourceUnavailable),
            (500, MovieProviderRequestError::Provider),
            (0, MovieProviderRequestError::Network),
        ] {
            assert_eq!(
                parse_movie_provider_response(
                    format!("secret response\nAUTO_VIDEO_HTTP_STATUS:{status}").as_bytes(),
                ),
                Err(error)
            );
        }
    }

    #[test]
    fn rejects_noncanonical_provider_codes_before_dispatch() {
        for code in ["", "mdvr-419", "MDVR_419", "MDVR-0419", "MDVR-4190 extra"] {
            let dispatched = RefCell::new(false);
            let result = fetch_javdb_vr_catalog_with(code, |_| {
                dispatched.replace(true);
                Err(ProviderRequestError::Network)
            });

            assert_eq!(result, Err(VR_PROVIDER_ERROR));
            assert!(!dispatched.into_inner());
        }
    }

    #[test]
    fn rejects_noncanonical_adult_provider_codes_before_either_dispatch() {
        for code in [
            "",
            "adlt-123",
            "ADLT_123",
            "ADLT-0123",
            "ADLT-0",
            "XADLT-123 extra",
        ] {
            let javdb_dispatched = RefCell::new(false);
            let sukebei_dispatched = RefCell::new(false);

            assert_eq!(
                fetch_javdb_adult_catalog_with(code, |_| {
                    javdb_dispatched.replace(true);
                    Err(ProviderRequestError::Network)
                }),
                Err(ADULT_PROVIDER_ERROR)
            );
            assert_eq!(
                fetch_sukebei_adult_releases_with(code, |_| {
                    sukebei_dispatched.replace(true);
                    Err(ProviderRequestError::Network)
                }),
                Err(ADULT_PROVIDER_ERROR)
            );
            assert!(!javdb_dispatched.into_inner());
            assert!(!sukebei_dispatched.into_inner());
        }
    }

    #[test]
    fn distinguishes_provider_response_statuses_at_the_native_boundary() {
        assert_eq!(
            parse_provider_response(b"document\nAUTO_VIDEO_HTTP_STATUS:200"),
            Ok("document".to_owned())
        );
        assert_eq!(
            parse_provider_response(b"missing\nAUTO_VIDEO_HTTP_STATUS:404"),
            Err(ProviderRequestError::SourceUnavailable)
        );
        assert_eq!(
            parse_provider_response(b"failure\nAUTO_VIDEO_HTTP_STATUS:500"),
            Err(ProviderRequestError::Provider)
        );
        assert_eq!(
            parse_provider_response(b"invalid"),
            Err(ProviderRequestError::Provider)
        );
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
