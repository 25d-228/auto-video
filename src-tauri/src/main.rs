#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod adult_library;
mod fanza_catalog;
mod javdb_catalog;
mod library_presentation;
mod library_scan;
mod tv_library;
mod tv_release;
mod vr_download;
mod vr_library;
mod vr_torrent;

use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
};

#[cfg(unix)]
use std::fs::File;

#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::process::{Command, Stdio};

use adult_library::{
    adult_library_presentation_authority, clear_adult_folder as clear_trusted_adult_folder,
    configured_adult_folder, load_adult_folder_with, open_adult_file_with, reveal_adult_file_with,
    scan_adult_library_with, set_adult_folder, trash_adult_file_with, AdultLibraryState,
    ADULT_FILE_OPEN_FAILED, ADULT_FILE_REVEAL_FAILED, ADULT_FILE_TRASH_FAILED,
    ADULT_FOLDER_STORAGE_FAILED, ADULT_FOLDER_UNAVAILABLE, ADULT_LIBRARY_SCAN_FAILED,
};
use fanza_catalog::{
    fetch_catalog_with as fetch_fanza_catalog_with, fetch_cover_bytes as fetch_fanza_cover_bytes,
    fetch_cover_with as fetch_fanza_cover_with,
    fetch_graphql_document as fetch_fanza_graphql_document,
    invalidate_catalog as invalidate_fanza_catalog_with, FanzaCatalogRequest, FanzaCatalogState,
};
use javdb_catalog::{
    fetch_api_document as fetch_javdb_api_document, fetch_catalog_with as fetch_javdb_catalog_with,
    fetch_cover_bytes as fetch_javdb_cover_bytes, fetch_cover_with as fetch_javdb_cover_with,
    fetch_detail_image_with as fetch_javdb_detail_image_with,
    fetch_detail_with as fetch_javdb_detail_with,
    invalidate_catalog as invalidate_javdb_catalog_with,
    invalidate_detail as invalidate_javdb_detail_with,
    open_detail_source_with as open_javdb_detail_source_with, JavdbCatalogRequest,
    JavdbCatalogState, JavdbDetailRequest,
};
use library_presentation::{
    begin_cover_request as begin_library_cover_request,
    cancel_cover_request as cancel_library_cover_request_with,
    cover_request_is_current as library_cover_request_is_current,
    fetch_cover as fetch_library_presentation_cover_with,
    invalidate_cover as invalidate_library_presentation_cover_with,
    resolve_cover as resolve_library_cover_with, resolve_metadata as resolve_library_metadata_with,
    LibraryItemAuthority, LibraryPresentationCategory, LibraryPresentationState,
    LIBRARY_PRESENTATION_FAILED, LIBRARY_PRESENTATION_STALE,
};
use library_scan::{is_supported_library_media, scan_library_files};
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;
use tv_library::{
    begin_tv_metadata_search, begin_tv_metadata_verification,
    clear_tv_folder as clear_trusted_tv_folder, clear_tv_metadata_match_with, configured_tv_folder,
    finish_tv_metadata_search, finish_tv_metadata_verification,
    invalidate_tv_metadata_client_context, invalidate_tv_metadata_context_for_state,
    load_tv_folder_with, open_tv_file_with, parse_tv_metadata_candidates,
    parse_verified_tv_metadata, percent_encode_tv_metadata_query, reveal_tv_file_with,
    save_tv_metadata_match_with, scan_tv_library_with_metadata, set_tv_folder,
    trash_tv_file_with_download_ownership_and_metadata, TvLibraryState, TV_FILE_OPEN_FAILED,
    TV_FILE_REVEAL_FAILED, TV_FILE_TRASH_FAILED, TV_FOLDER_STORAGE_FAILED, TV_FOLDER_UNAVAILABLE,
    TV_LIBRARY_SCAN_FAILED, TV_METADATA_CONTEXT_INVALID, TV_METADATA_PERSISTENCE_FAILED,
};
use tv_release::{
    fetch_apibay_tv_releases_for_state_with, TvReleaseState, TV_APIBAY_PROVIDER_ERROR,
    TV_TMDB_MALFORMED, TV_TMDB_UNAUTHORIZED,
};
use vr_download::{
    acquire_tv_metainfo, apply_organization, cancel_download, cleanup_cancelled_download,
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
    trash_vr_file_with, vr_library_presentation_authority, VrLibraryState, VR_FILE_OPEN_FAILED,
    VR_FILE_REVEAL_FAILED, VR_FILE_TRASH_FAILED, VR_LIBRARY_SCAN_FAILED,
};
use vr_torrent::{
    canonical_imdb_id, fetch_artifact_response, hex_sha1, inspect_sukebei_adult_torrent_with,
    inspect_sukebei_torrent_with, inspect_yts_movie_torrent_with, json_array, json_object,
    json_string, json_u64, save_verified_adult_torrent_with, save_verified_movie_torrent_with,
    save_verified_torrent_with, save_verified_tv_torrent_with, verified_movie_imdb_id,
    write_new_torrent_file, AdultTorrentState, JsonParser, JsonValue,
    MovieTorrentInspectionRequest, MovieTorrentState, TorrentInspectionRequest,
    TvTorrentInspectionStart, TvTorrentState, VrTorrentState, ADULT_TORRENT_PROVIDER_ERROR,
    ADULT_TORRENT_SAVE_FAILED, MOVIE_TMDB_MALFORMED, MOVIE_TORRENT_PROVIDER_ERROR,
    MOVIE_TORRENT_SAVE_FAILED, TV_TORRENT_CONTEXT_INVALID, TV_TORRENT_LOCAL_PENDING,
    TV_TORRENT_LOCAL_UNAVAILABLE, TV_TORRENT_NETWORK_ERROR, TV_TORRENT_NO_METADATA_SOURCE,
    TV_TORRENT_SAVE_FAILED, TV_TORRENT_TIMEOUT, VR_TORRENT_PROVIDER_ERROR, VR_TORRENT_SAVE_FAILED,
};

const MOVIES_FOLDER_FILE_NAME: &str = ".movies-folder";
const MOVIE_METADATA_FILE_NAME: &str = ".movie-library-metadata";
const ADULT_FOLDER_FILE_NAME: &str = ".adult-folder";
const TV_FOLDER_FILE_NAME: &str = ".tv-folder";
const TV_METADATA_FILE_NAME: &str = ".tv-library-show-metadata";
const VR_FOLDER_FILE_NAME: &str = ".vr-folder";
const VR_DOWNLOADS_FILE_NAME: &str = ".vr-downloads";
const VR_DOWNLOAD_LIMIT_FILE_NAME: &str = ".vr-download-limit";
const VR_SESSION_FOLDER_NAME: &str = "vr-session";
const LIBRARY_PRESENTATION_CACHE_FILE_NAME: &str = ".library-presentation-cache";
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
const MOVIE_METADATA_CONTEXT_INVALID: &str = "movie_metadata_context_invalid";
const MOVIE_METADATA_MALFORMED: &str = "movie_metadata_malformed_provider";
const MOVIE_METADATA_PERSISTENCE_FAILED: &str = "movie_metadata_persistence_failed";
const MOVIE_METADATA_STALE: &str = "movie_metadata_stale";
const MOVIE_METADATA_UNAVAILABLE: &str = "movie_metadata_unavailable";
const TMDB_TOKEN_FILE_NAME: &str = ".tmdb-api-read-access-token";
const TMDB_TOKEN_INVALID: &str = "tmdb_token_invalid";
const TMDB_TOKEN_STORAGE_FAILED: &str = "tmdb_token_storage_failed";
const MOVIE_TMDB_NETWORK_ERROR: &str = "movie_tmdb_network_error";
const MOVIE_TMDB_PROVIDER_ERROR: &str = "movie_tmdb_provider_error";
const MOVIE_TMDB_RATE_LIMITED: &str = "movie_tmdb_rate_limited";
const MOVIE_TMDB_UNAUTHORIZED: &str = "movie_tmdb_unauthorized";
const TV_METADATA_TMDB_NETWORK_ERROR: &str = "tv_metadata_tmdb_network_error";
const TV_METADATA_TMDB_PROVIDER_ERROR: &str = "tv_metadata_tmdb_provider_error";
const TV_METADATA_TMDB_RATE_LIMITED: &str = "tv_metadata_tmdb_rate_limited";
const TV_METADATA_TMDB_UNAUTHORIZED: &str = "tv_metadata_tmdb_unauthorized";
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
const TMDB_MOVIE_SEARCH_URL: &str = "https://api.themoviedb.org/3/search/movie?query=";
const TMDB_TV_URL: &str = "https://api.themoviedb.org/3/tv/";
const TMDB_TV_SEARCH_URL: &str = "https://api.themoviedb.org/3/search/tv?query=";
const YTS_MOVIES_URL: &str = "https://yts.mx/api/v2/list_movies.json?limit=50&query_term=";
const MOVIE_METADATA_HEADER: &[u8] = b"AUTO_VIDEO_MOVIE_METADATA_V1\n";
const MOVIE_METADATA_MAX_BYTES: u64 = 4 * 1024 * 1024;
const MOVIE_METADATA_MAX_RECORDS: usize = 10_000;
const MOVIE_METADATA_MAX_QUERY_BYTES: usize = 512;
const MOVIE_METADATA_MAX_CANDIDATES: usize = 100;

#[derive(Clone, Debug, PartialEq)]
struct MovieMetadataAssociation {
    folder: PathBuf,
    folder_identity: String,
    relative_path: String,
    file_identity: String,
    fingerprint: String,
    size: u64,
    tmdb_movie_id: u64,
    imdb_id: String,
    title: String,
    original_title: Option<String>,
    release_date: Option<String>,
    poster_path: Option<String>,
    overview: Option<String>,
    generation: u64,
}

#[derive(Clone, Debug, PartialEq)]
struct TrustedMovieFile {
    file_id: String,
    path: PathBuf,
    relative_path: String,
    file_identity: String,
    fingerprint: String,
    size: u64,
    association: Option<MovieMetadataAssociation>,
}

#[derive(Clone, Debug, PartialEq)]
struct CompletedMovieScan {
    folder: PathBuf,
    folder_identity: String,
    generation: u64,
    files: Vec<TrustedMovieFile>,
    association_status: &'static str,
}

#[derive(Clone, Debug, PartialEq)]
struct MovieMetadataCandidate {
    tmdb_movie_id: u64,
    title: String,
    original_title: Option<String>,
    release_date: Option<String>,
    poster_path: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
struct MovieMetadataAuthority {
    scan_generation: u64,
    folder: PathBuf,
    folder_identity: String,
    file_id: String,
    path: PathBuf,
    relative_path: String,
    file_identity: String,
    fingerprint: String,
    size: u64,
}

#[derive(Clone, Debug, PartialEq)]
struct MovieMetadataSearch {
    request_id: String,
    operation_generation: u64,
    authority: MovieMetadataAuthority,
    query: String,
    token_identity: String,
    candidates: Vec<MovieMetadataCandidate>,
}

#[derive(Clone, Debug, PartialEq)]
struct MovieMetadataVerification {
    verification_id: String,
    operation_generation: u64,
    matching_request_id: String,
    authority: MovieMetadataAuthority,
    association: MovieMetadataAssociation,
    token_identity: String,
}

#[derive(Default)]
struct MoviesLibraryContext {
    folder: Option<PathBuf>,
    movie_paths: Vec<String>,
    generation: u64,
    completed_scan: Option<CompletedMovieScan>,
    metadata_client_generation: u64,
    metadata_operation_generation: u64,
    metadata_search: Option<MovieMetadataSearch>,
    metadata_verification: Option<MovieMetadataVerification>,
}

#[derive(Clone, Default)]
struct MoviesLibraryState(Arc<Mutex<MoviesLibraryContext>>);

struct TrashMovieRequest {
    path: String,
    folder: Option<String>,
    library_paths: Option<Vec<String>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ProviderRequestError {
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

fn scan_movie_paths(folder: &Path) -> Result<Vec<String>, &'static str> {
    let metadata = fs::metadata(folder).map_err(|_| MOVIES_FOLDER_UNAVAILABLE)?;
    if !metadata.is_dir() {
        return Err(MOVIES_FOLDER_UNAVAILABLE);
    }

    scan_library_files(folder, |path, _| path.into_os_string().into_string().ok())
        .map_err(|_| MOVIES_SCAN_FAILED)
}

#[cfg(unix)]
pub(crate) fn movie_path_identity(path: &Path, regular_file: bool) -> Result<String, &'static str> {
    use std::os::unix::fs::MetadataExt;

    let metadata = fs::symlink_metadata(path).map_err(|_| MOVIE_METADATA_UNAVAILABLE)?;
    if metadata.file_type().is_symlink()
        || (regular_file && !metadata.is_file())
        || (!regular_file && !metadata.is_dir())
    {
        return Err(MOVIE_METADATA_STALE);
    }
    Ok(format!("{}:{}", metadata.dev(), metadata.ino()))
}

#[cfg(target_os = "windows")]
pub(crate) fn movie_path_identity(path: &Path, regular_file: bool) -> Result<String, &'static str> {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_READ_ATTRIBUTES: u32 = 0x0000_0080;
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const FILE_SHARE_DELETE: u32 = 0x0000_0004;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    let metadata = fs::symlink_metadata(path).map_err(|_| MOVIE_METADATA_UNAVAILABLE)?;
    if metadata.file_type().is_symlink()
        || (regular_file && !metadata.is_file())
        || (!regular_file && !metadata.is_dir())
    {
        return Err(MOVIE_METADATA_STALE);
    }
    let file = OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| MOVIE_METADATA_UNAVAILABLE)?;
    vr_download::open_file_fingerprint(&file).map_err(|_| MOVIE_METADATA_UNAVAILABLE)
}

#[cfg(not(any(unix, target_os = "windows")))]
pub(crate) fn movie_path_identity(path: &Path, regular_file: bool) -> Result<String, &'static str> {
    let metadata = fs::symlink_metadata(path).map_err(|_| MOVIE_METADATA_UNAVAILABLE)?;
    if metadata.file_type().is_symlink()
        || (regular_file && !metadata.is_file())
        || (!regular_file && !metadata.is_dir())
    {
        return Err(MOVIE_METADATA_STALE);
    }
    let modified = metadata
        .modified()
        .map_err(|_| MOVIE_METADATA_UNAVAILABLE)?
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| MOVIE_METADATA_UNAVAILABLE)?;
    Ok(format!("{}:{}", modified.as_nanos(), metadata.len()))
}

fn validate_movie_components(folder: &Path, path: &Path) -> Result<(), &'static str> {
    let relative = path
        .strip_prefix(folder)
        .map_err(|_| MOVIE_METADATA_STALE)?;
    if relative.components().next().is_none() {
        return Err(MOVIE_METADATA_STALE);
    }
    let mut current = folder.to_path_buf();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err(MOVIE_METADATA_STALE);
        };
        current.push(component);
        let metadata = fs::symlink_metadata(&current).map_err(|_| MOVIE_METADATA_STALE)?;
        if metadata.file_type().is_symlink() {
            return Err(MOVIE_METADATA_STALE);
        }
    }
    Ok(())
}

#[cfg(unix)]
pub(crate) fn movie_file_fingerprint(metadata: &fs::Metadata) -> String {
    use std::os::unix::fs::MetadataExt;

    format!(
        "{}:{}:{}:{}:{}",
        metadata.len(),
        metadata.mtime(),
        metadata.mtime_nsec(),
        metadata.ctime(),
        metadata.ctime_nsec()
    )
}

#[cfg(target_os = "windows")]
pub(crate) fn movie_file_fingerprint(metadata: &fs::Metadata) -> String {
    use std::os::windows::fs::MetadataExt;

    format!(
        "{}:{}:{}",
        metadata.file_size(),
        metadata.last_write_time(),
        metadata.creation_time()
    )
}

#[cfg(not(any(unix, target_os = "windows")))]
pub(crate) fn movie_file_fingerprint(metadata: &fs::Metadata) -> String {
    let modified = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    format!("{}:{modified}", metadata.len())
}

fn capture_trusted_movie_file(
    folder: &Path,
    folder_identity: &str,
    generation: u64,
    path: PathBuf,
) -> Result<TrustedMovieFile, &'static str> {
    validate_movie_components(folder, &path)?;
    let metadata = fs::metadata(&path).map_err(|_| MOVIES_SCAN_FAILED)?;
    if !metadata.is_file() || !is_supported_library_media(&path) {
        return Err(MOVIES_SCAN_FAILED);
    }
    let canonical_path = fs::canonicalize(&path).map_err(|_| MOVIES_SCAN_FAILED)?;
    if canonical_path != path || !canonical_path.starts_with(folder) {
        return Err(MOVIES_SCAN_FAILED);
    }
    let relative_path = path
        .strip_prefix(folder)
        .ok()
        .and_then(Path::to_str)
        .filter(|relative| !relative.is_empty())
        .map(str::to_owned)
        .ok_or(MOVIES_SCAN_FAILED)?;
    let file_identity = movie_path_identity(&path, true).map_err(|_| MOVIES_SCAN_FAILED)?;
    let fingerprint = movie_file_fingerprint(&metadata);
    let size = metadata.len();
    let file_id = hex_sha1(
        format!(
            "{generation}\0{folder_identity}\0{relative_path}\0{file_identity}\0{fingerprint}\0{size}"
        )
        .as_bytes(),
    );
    Ok(TrustedMovieFile {
        file_id,
        path,
        relative_path,
        file_identity,
        fingerprint,
        size,
        association: None,
    })
}

fn scan_trusted_movie_files(
    folder: &Path,
    generation: u64,
) -> Result<(String, Vec<TrustedMovieFile>), &'static str> {
    let canonical_folder = fs::canonicalize(folder).map_err(|_| MOVIES_FOLDER_UNAVAILABLE)?;
    if canonical_folder != folder
        || !fs::metadata(folder)
            .map_err(|_| MOVIES_FOLDER_UNAVAILABLE)?
            .is_dir()
    {
        return Err(MOVIES_FOLDER_UNAVAILABLE);
    }
    let folder_identity = movie_path_identity(folder, false).map_err(|_| MOVIES_SCAN_FAILED)?;
    let files = scan_library_files(folder, |path, _| {
        capture_trusted_movie_file(folder, &folder_identity, generation, path).ok()
    })
    .map_err(|_| MOVIES_SCAN_FAILED)?;
    Ok((folder_identity, files))
}

fn encode_movie_metadata_text(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(value.len() * 2);
    for byte in value.as_bytes() {
        encoded.push(HEX[usize::from(byte >> 4)] as char);
        encoded.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    encoded
}

fn decode_movie_metadata_text(value: &str) -> Option<String> {
    if !value.len().is_multiple_of(2) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut decoded = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks_exact(2) {
        let pair = std::str::from_utf8(pair).ok()?;
        decoded.push(u8::from_str_radix(pair, 16).ok()?);
    }
    String::from_utf8(decoded).ok()
}

fn valid_movie_metadata_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
}

fn valid_movie_metadata_relative_path(value: &str) -> bool {
    let path = Path::new(value);
    !path.is_absolute()
        && path.components().next().is_some()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn validate_movie_metadata_association(
    association: &MovieMetadataAssociation,
) -> Result<(), &'static str> {
    if !association.folder.is_absolute()
        || association.folder.to_str().is_none()
        || association.folder_identity.is_empty()
        || association.folder_identity.len() > 256
        || !valid_movie_metadata_relative_path(&association.relative_path)
        || association.relative_path.len() > 32 * 1024
        || association.file_identity.is_empty()
        || association.file_identity.len() > 256
        || association.fingerprint.is_empty()
        || association.fingerprint.len() > 512
        || association.tmdb_movie_id == 0
        || canonical_imdb_id(&association.imdb_id).as_deref() != Some(association.imdb_id.as_str())
        || association.title.trim().is_empty()
        || association.title.len() > 16 * 1024
        || association
            .original_title
            .as_ref()
            .is_some_and(|value| value.trim().is_empty() || value.len() > 16 * 1024)
        || association
            .release_date
            .as_deref()
            .is_some_and(|value| !valid_movie_metadata_date(value))
        || association.poster_path.as_ref().is_some_and(|value| {
            !value.starts_with('/')
                || value.len() > 16 * 1024
                || value.chars().any(char::is_control)
        })
        || association
            .overview
            .as_ref()
            .is_some_and(|value| value.trim().is_empty() || value.len() > 256 * 1024)
        || association.generation == 0
    {
        return Err(MOVIE_METADATA_PERSISTENCE_FAILED);
    }
    Ok(())
}

fn encoded_movie_metadata_associations(
    associations: &[MovieMetadataAssociation],
) -> Result<Vec<u8>, &'static str> {
    if associations.len() > MOVIE_METADATA_MAX_RECORDS {
        return Err(MOVIE_METADATA_PERSISTENCE_FAILED);
    }
    let mut payload = format!("{}\n", associations.len());
    for association in associations {
        validate_movie_metadata_association(association)?;
        let fields = [
            encode_movie_metadata_text(
                association
                    .folder
                    .to_str()
                    .ok_or(MOVIE_METADATA_PERSISTENCE_FAILED)?,
            ),
            encode_movie_metadata_text(&association.folder_identity),
            encode_movie_metadata_text(&association.relative_path),
            encode_movie_metadata_text(&association.file_identity),
            encode_movie_metadata_text(&association.fingerprint),
            association.size.to_string(),
            association.tmdb_movie_id.to_string(),
            encode_movie_metadata_text(&association.imdb_id),
            encode_movie_metadata_text(&association.title),
            encode_movie_metadata_text(association.original_title.as_deref().unwrap_or("")),
            encode_movie_metadata_text(association.release_date.as_deref().unwrap_or("")),
            encode_movie_metadata_text(association.poster_path.as_deref().unwrap_or("")),
            encode_movie_metadata_text(association.overview.as_deref().unwrap_or("")),
            association.generation.to_string(),
        ];
        payload.push_str(&fields.join("\t"));
        payload.push('\n');
    }
    let mut bytes = MOVIE_METADATA_HEADER.to_vec();
    bytes.extend_from_slice(hex_sha1(payload.as_bytes()).as_bytes());
    bytes.push(b'\n');
    bytes.extend_from_slice(payload.as_bytes());
    if bytes.len() as u64 > MOVIE_METADATA_MAX_BYTES {
        return Err(MOVIE_METADATA_PERSISTENCE_FAILED);
    }
    Ok(bytes)
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum MovieMetadataReadError {
    Invalid,
    Unavailable,
}

fn parse_movie_metadata_associations(
    bytes: &[u8],
) -> Result<Vec<MovieMetadataAssociation>, MovieMetadataReadError> {
    if bytes.len() as u64 > MOVIE_METADATA_MAX_BYTES {
        return Err(MovieMetadataReadError::Invalid);
    }
    let content = bytes
        .strip_prefix(MOVIE_METADATA_HEADER)
        .ok_or(MovieMetadataReadError::Invalid)?;
    let checksum_end = content
        .iter()
        .position(|byte| *byte == b'\n')
        .ok_or(MovieMetadataReadError::Invalid)?;
    let checksum = std::str::from_utf8(&content[..checksum_end])
        .map_err(|_| MovieMetadataReadError::Invalid)?;
    let payload = &content[checksum_end + 1..];
    if checksum.len() != 40 || checksum != hex_sha1(payload).as_str() {
        return Err(MovieMetadataReadError::Invalid);
    }
    let payload = std::str::from_utf8(payload).map_err(|_| MovieMetadataReadError::Invalid)?;
    let payload = payload
        .strip_suffix('\n')
        .ok_or(MovieMetadataReadError::Invalid)?;
    let mut lines = payload.split('\n');
    let count = lines
        .next()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|count| *count <= MOVIE_METADATA_MAX_RECORDS)
        .ok_or(MovieMetadataReadError::Invalid)?;
    let mut associations = Vec::with_capacity(count);
    let mut path_keys = HashSet::new();
    let mut generations = HashSet::new();
    for _ in 0..count {
        let fields = lines
            .next()
            .map(|line| line.split('\t').collect::<Vec<_>>())
            .ok_or(MovieMetadataReadError::Invalid)?;
        let [folder, folder_identity, relative_path, file_identity, fingerprint, size, tmdb_movie_id, imdb_id, title, original_title, release_date, poster_path, overview, generation] =
            fields.as_slice()
        else {
            return Err(MovieMetadataReadError::Invalid);
        };
        let optional_text = |value: &str| {
            decode_movie_metadata_text(value)
                .map(|value| (!value.is_empty()).then_some(value))
                .ok_or(MovieMetadataReadError::Invalid)
        };
        let association = MovieMetadataAssociation {
            folder: PathBuf::from(
                decode_movie_metadata_text(folder).ok_or(MovieMetadataReadError::Invalid)?,
            ),
            folder_identity: decode_movie_metadata_text(folder_identity)
                .ok_or(MovieMetadataReadError::Invalid)?,
            relative_path: decode_movie_metadata_text(relative_path)
                .ok_or(MovieMetadataReadError::Invalid)?,
            file_identity: decode_movie_metadata_text(file_identity)
                .ok_or(MovieMetadataReadError::Invalid)?,
            fingerprint: decode_movie_metadata_text(fingerprint)
                .ok_or(MovieMetadataReadError::Invalid)?,
            size: size.parse().map_err(|_| MovieMetadataReadError::Invalid)?,
            tmdb_movie_id: tmdb_movie_id
                .parse()
                .map_err(|_| MovieMetadataReadError::Invalid)?,
            imdb_id: decode_movie_metadata_text(imdb_id).ok_or(MovieMetadataReadError::Invalid)?,
            title: decode_movie_metadata_text(title).ok_or(MovieMetadataReadError::Invalid)?,
            original_title: optional_text(original_title)?,
            release_date: optional_text(release_date)?,
            poster_path: optional_text(poster_path)?,
            overview: optional_text(overview)?,
            generation: generation
                .parse()
                .map_err(|_| MovieMetadataReadError::Invalid)?,
        };
        validate_movie_metadata_association(&association)
            .map_err(|_| MovieMetadataReadError::Invalid)?;
        let path_key = (
            association.folder.clone(),
            association.folder_identity.clone(),
            association.relative_path.clone(),
        );
        if !path_keys.insert(path_key) || !generations.insert(association.generation) {
            return Err(MovieMetadataReadError::Invalid);
        }
        associations.push(association);
    }
    if lines.next().is_some() {
        return Err(MovieMetadataReadError::Invalid);
    }
    Ok(associations)
}

fn read_movie_metadata_associations(
    path: &Path,
) -> Result<Vec<MovieMetadataAssociation>, MovieMetadataReadError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(MovieMetadataReadError::Unavailable),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(MovieMetadataReadError::Invalid);
    }
    if metadata.len() > MOVIE_METADATA_MAX_BYTES {
        return Err(MovieMetadataReadError::Invalid);
    }
    let bytes = fs::read(path).map_err(|_| MovieMetadataReadError::Unavailable)?;
    parse_movie_metadata_associations(&bytes)
}

#[cfg(target_os = "windows")]
#[link(name = "Kernel32")]
extern "system" {
    #[link_name = "MoveFileExW"]
    fn replace_movie_metadata_file_windows(
        existing_file_name: *const u16,
        new_file_name: *const u16,
        flags: u32,
    ) -> i32;
}

#[cfg(target_os = "windows")]
pub(crate) fn replace_movie_metadata_file(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
    let source = source
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let succeeded = unsafe {
        replace_movie_metadata_file_windows(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    (succeeded != 0)
        .then_some(())
        .ok_or_else(io::Error::last_os_error)
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn replace_movie_metadata_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

fn write_movie_metadata_associations(
    path: &Path,
    associations: &[MovieMetadataAssociation],
) -> Result<(), &'static str> {
    let bytes = encoded_movie_metadata_associations(associations)?;
    let parent = path.parent().ok_or(MOVIE_METADATA_PERSISTENCE_FAILED)?;
    fs::create_dir_all(parent).map_err(|_| MOVIE_METADATA_PERSISTENCE_FAILED)?;
    let file_name = path.file_name().ok_or(MOVIE_METADATA_PERSISTENCE_FAILED)?;
    let mut replacement_name = file_name.to_os_string();
    replacement_name.push(".next");
    let replacement = path.with_file_name(replacement_name);
    match fs::symlink_metadata(&replacement) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(MOVIE_METADATA_PERSISTENCE_FAILED);
        }
        Ok(_) => fs::remove_file(&replacement).map_err(|_| MOVIE_METADATA_PERSISTENCE_FAILED)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => return Err(MOVIE_METADATA_PERSISTENCE_FAILED),
    }
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&replacement)
            .map_err(|_| MOVIE_METADATA_PERSISTENCE_FAILED)?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| MOVIE_METADATA_PERSISTENCE_FAILED)?;
        replace_movie_metadata_file(&replacement, path)
            .map_err(|_| MOVIE_METADATA_PERSISTENCE_FAILED)?;
        #[cfg(unix)]
        let _ = File::open(parent).and_then(|directory| directory.sync_all());
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&replacement);
    }
    result
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

fn invalidate_movie_metadata_context(library: &mut MoviesLibraryContext) {
    library.metadata_operation_generation = library.metadata_operation_generation.wrapping_add(1);
    library.metadata_search = None;
    library.metadata_verification = None;
}

fn begin_movie_metadata_client_operation(
    library: &mut MoviesLibraryContext,
    client_generation: u64,
) -> Result<(), &'static str> {
    if client_generation == 0 || client_generation <= library.metadata_client_generation {
        return Err(MOVIE_METADATA_CONTEXT_INVALID);
    }
    library.metadata_client_generation = client_generation;
    Ok(())
}

fn invalidate_movie_metadata_client_context(
    library: &mut MoviesLibraryContext,
    client_generation: u64,
) {
    if client_generation < library.metadata_client_generation {
        return;
    }
    library.metadata_client_generation = client_generation;
    invalidate_movie_metadata_context(library);
}

fn association_matches_file(
    association: &MovieMetadataAssociation,
    scan: &CompletedMovieScan,
    file: &TrustedMovieFile,
) -> bool {
    association.folder == scan.folder
        && association.folder_identity == scan.folder_identity
        && association.relative_path == file.relative_path
        && association.file_identity == file.file_identity
        && association.fingerprint == file.fingerprint
        && association.size == file.size
}

fn encode_movie_scan(scan: &CompletedMovieScan) -> Result<Vec<String>, &'static str> {
    let mut response = Vec::with_capacity(3 + scan.files.len() * 13);
    response.push("movie-library-v1".to_owned());
    response.push(scan.association_status.to_owned());
    response.push(scan.files.len().to_string());
    for file in &scan.files {
        response.push(file.file_id.clone());
        response.push(
            file.path
                .to_str()
                .map(str::to_owned)
                .ok_or(MOVIES_SCAN_FAILED)?,
        );
        response.push(file.relative_path.clone());
        response.push(file.size.to_string());
        response.push(if file.association.is_some() {
            "1".to_owned()
        } else {
            "0".to_owned()
        });
        if let Some(association) = &file.association {
            response.push(association.tmdb_movie_id.to_string());
            response.push(association.imdb_id.clone());
            response.push(association.title.clone());
            response.push(association.original_title.clone().unwrap_or_default());
            response.push(association.release_date.clone().unwrap_or_default());
            response.push(association.poster_path.clone().unwrap_or_default());
            response.push(association.overview.clone().unwrap_or_default());
            response.push(association.generation.to_string());
        } else {
            response.extend((0..8).map(|_| String::new()));
        }
    }
    Ok(response)
}

fn scan_movies_library(
    library: &mut MoviesLibraryContext,
    association_path: &Path,
) -> Result<Vec<String>, &'static str> {
    let folder = library.folder.clone().ok_or(MOVIES_FOLDER_UNAVAILABLE)?;
    library.generation = library.generation.wrapping_add(1);
    let generation = library.generation;
    library.completed_scan = None;
    invalidate_movie_metadata_context(library);
    let (folder_identity, mut files) = scan_trusted_movie_files(&folder, generation)?;
    if library.folder.as_ref() != Some(&folder) || library.generation != generation {
        return Err(MOVIE_METADATA_STALE);
    }
    let (associations, association_status) =
        match read_movie_metadata_associations(association_path) {
            Ok(associations) => (associations, "ready"),
            Err(MovieMetadataReadError::Invalid) => (Vec::new(), "attention"),
            Err(MovieMetadataReadError::Unavailable) => (Vec::new(), "unavailable"),
        };
    let scan_identity = CompletedMovieScan {
        folder: folder.clone(),
        folder_identity,
        generation,
        files: Vec::new(),
        association_status,
    };
    for file in &mut files {
        file.association = associations
            .iter()
            .find(|association| association_matches_file(association, &scan_identity, file))
            .cloned();
    }
    library.movie_paths = files
        .iter()
        .map(|file| {
            file.path
                .to_str()
                .map(str::to_owned)
                .ok_or(MOVIES_SCAN_FAILED)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let scan = CompletedMovieScan {
        files,
        ..scan_identity
    };
    let response = encode_movie_scan(&scan)?;
    library.completed_scan = Some(scan);
    Ok(response)
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum MoviePathValidationError {
    NotFound,
    Unavailable,
    NotFile,
    Unsupported,
    OutsideFolder,
    Stale,
}

impl MoviePathValidationError {
    fn open_error_code(self) -> &'static str {
        match self {
            Self::NotFound => MOVIE_OPEN_NOT_FOUND,
            Self::Unavailable => MOVIE_OPEN_UNAVAILABLE,
            Self::NotFile => MOVIE_OPEN_NOT_FILE,
            Self::Unsupported => MOVIE_OPEN_UNSUPPORTED,
            Self::OutsideFolder | Self::Stale => MOVIE_OPEN_UNAVAILABLE,
        }
    }

    fn reveal_error_code(self) -> &'static str {
        match self {
            Self::NotFound => MOVIE_REVEAL_NOT_FOUND,
            Self::Unavailable => MOVIE_REVEAL_UNAVAILABLE,
            Self::NotFile => MOVIE_REVEAL_NOT_FILE,
            Self::Unsupported => MOVIE_REVEAL_UNSUPPORTED,
            Self::OutsideFolder | Self::Stale => MOVIE_REVEAL_UNAVAILABLE,
        }
    }

    fn trash_error_code(self) -> &'static str {
        match self {
            Self::NotFound => MOVIE_TRASH_NOT_FOUND,
            Self::Unavailable => MOVIE_TRASH_UNAVAILABLE,
            Self::NotFile => MOVIE_TRASH_NOT_FILE,
            Self::Unsupported => MOVIE_TRASH_UNSUPPORTED,
            Self::OutsideFolder => MOVIE_TRASH_OUTSIDE_FOLDER,
            Self::Stale => MOVIE_TRASH_STALE,
        }
    }
}

fn validate_current_movie_file(
    library: &MoviesLibraryContext,
    requested_path: &Path,
) -> Result<TrustedMovieFile, MoviePathValidationError> {
    let folder = library
        .folder
        .as_deref()
        .ok_or(MoviePathValidationError::Unavailable)?;
    let scan = library
        .completed_scan
        .as_ref()
        .ok_or(MoviePathValidationError::Stale)?;
    if scan.folder != folder
        || fs::canonicalize(folder).ok().as_deref() != Some(folder)
        || movie_path_identity(folder, false).ok().as_deref() != Some(scan.folder_identity.as_str())
    {
        return Err(MoviePathValidationError::Stale);
    }
    let relative = requested_path
        .strip_prefix(folder)
        .map_err(|_| MoviePathValidationError::OutsideFolder)?;
    let mut current = folder.to_path_buf();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err(MoviePathValidationError::OutsideFolder);
        };
        current.push(component);
        let metadata =
            fs::symlink_metadata(&current).map_err(|error| movie_metadata_error(&error))?;
        if metadata.file_type().is_symlink() {
            return Err(MoviePathValidationError::NotFile);
        }
    }
    let metadata = fs::metadata(requested_path).map_err(|error| movie_metadata_error(&error))?;
    if !metadata.is_file() {
        return Err(MoviePathValidationError::NotFile);
    }
    if !is_supported_library_media(requested_path) {
        return Err(MoviePathValidationError::Unsupported);
    }
    let canonical =
        fs::canonicalize(requested_path).map_err(|error| movie_metadata_error(&error))?;
    if canonical != requested_path || !canonical.starts_with(folder) {
        return Err(MoviePathValidationError::OutsideFolder);
    }
    let trusted = scan
        .files
        .iter()
        .find(|file| file.path == requested_path)
        .cloned()
        .ok_or(MoviePathValidationError::Stale)?;
    if trusted.size != metadata.len()
        || movie_path_identity(requested_path, true).ok().as_deref()
            != Some(trusted.file_identity.as_str())
        || movie_file_fingerprint(&metadata) != trusted.fingerprint
    {
        return Err(MoviePathValidationError::Stale);
    }
    Ok(trusted)
}

fn movie_metadata_authority(
    library: &MoviesLibraryContext,
    file_id: &str,
) -> Result<MovieMetadataAuthority, &'static str> {
    let scan = library
        .completed_scan
        .as_ref()
        .ok_or(MOVIE_METADATA_STALE)?;
    let file = scan
        .files
        .iter()
        .find(|file| file.file_id == file_id)
        .ok_or(MOVIE_METADATA_STALE)?;
    let file =
        validate_current_movie_file(library, &file.path).map_err(|_| MOVIE_METADATA_STALE)?;
    Ok(MovieMetadataAuthority {
        scan_generation: scan.generation,
        folder: scan.folder.clone(),
        folder_identity: scan.folder_identity.clone(),
        file_id: file.file_id,
        path: file.path,
        relative_path: file.relative_path,
        file_identity: file.file_identity,
        fingerprint: file.fingerprint,
        size: file.size,
    })
}

fn validate_movie_metadata_authority(
    library: &MoviesLibraryContext,
    authority: &MovieMetadataAuthority,
) -> Result<TrustedMovieFile, &'static str> {
    let scan = library
        .completed_scan
        .as_ref()
        .ok_or(MOVIE_METADATA_STALE)?;
    if scan.generation != authority.scan_generation
        || scan.folder != authority.folder
        || scan.folder_identity != authority.folder_identity
    {
        return Err(MOVIE_METADATA_STALE);
    }
    let file =
        validate_current_movie_file(library, &authority.path).map_err(|_| MOVIE_METADATA_STALE)?;
    if file.file_id != authority.file_id
        || file.relative_path != authority.relative_path
        || file.file_identity != authority.file_identity
        || file.fingerprint != authority.fingerprint
        || file.size != authority.size
    {
        return Err(MOVIE_METADATA_STALE);
    }
    Ok(file)
}

fn movie_metadata_error(error: &io::Error) -> MoviePathValidationError {
    if error.kind() == io::ErrorKind::NotFound {
        MoviePathValidationError::NotFound
    } else {
        MoviePathValidationError::Unavailable
    }
}

#[cfg(test)]
fn validate_movie_path(path: &Path) -> Result<(), MoviePathValidationError> {
    let metadata = fs::metadata(path).map_err(|error| movie_metadata_error(&error))?;
    if !metadata.is_file() {
        return Err(MoviePathValidationError::NotFile);
    }
    if !is_supported_library_media(path) {
        return Err(MoviePathValidationError::Unsupported);
    }

    Ok(())
}

#[cfg(test)]
fn open_movie_path_with(
    path: &Path,
    dispatch: impl FnOnce(&Path) -> Result<(), ()>,
) -> Result<(), &'static str> {
    validate_movie_path(path).map_err(MoviePathValidationError::open_error_code)?;

    dispatch(path).map_err(|_| MOVIE_OPEN_FAILED)
}

#[cfg(test)]
fn reveal_movie_path_with(
    path: &Path,
    dispatch: impl FnOnce(&Path) -> Result<(), ()>,
) -> Result<(), &'static str> {
    validate_movie_path(path).map_err(MoviePathValidationError::reveal_error_code)?;

    dispatch(path).map_err(|_| MOVIE_REVEAL_FAILED)
}

fn open_movie_request_with(
    path: &Path,
    library: &MoviesLibraryContext,
    dispatch: impl FnOnce(&Path) -> Result<(), ()>,
) -> Result<(), &'static str> {
    validate_current_movie_file(library, path)
        .map_err(MoviePathValidationError::open_error_code)?;
    dispatch(path).map_err(|_| MOVIE_OPEN_FAILED)
}

fn reveal_movie_request_with(
    path: &Path,
    library: &MoviesLibraryContext,
    dispatch: impl FnOnce(&Path) -> Result<(), ()>,
) -> Result<(), &'static str> {
    validate_current_movie_file(library, path)
        .map_err(MoviePathValidationError::reveal_error_code)?;
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
    if !is_supported_library_media(path) {
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
    validate_current_movie_file(library, Path::new(&request.path))
        .map_err(MoviePathValidationError::trash_error_code)?;
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
    if let Some(scan) = &mut library.completed_scan {
        scan.files
            .retain(|file| file.path != Path::new(&request.path));
    }
    invalidate_movie_metadata_context(library);
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
    crate::vr_torrent::canonical_product_code(code).as_deref() == Some(code)
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

fn tmdb_tv_metadata_provider_error_code(error: MovieProviderRequestError) -> &'static str {
    match error {
        MovieProviderRequestError::Unauthorized => TV_METADATA_TMDB_UNAUTHORIZED,
        MovieProviderRequestError::RateLimited => TV_METADATA_TMDB_RATE_LIMITED,
        MovieProviderRequestError::Network => TV_METADATA_TMDB_NETWORK_ERROR,
        MovieProviderRequestError::SourceUnavailable | MovieProviderRequestError::Provider => {
            TV_METADATA_TMDB_PROVIDER_ERROR
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

fn movie_metadata_optional_text(
    object: &std::collections::BTreeMap<String, JsonValue>,
    key: &str,
    max_bytes: usize,
) -> Result<Option<String>, &'static str> {
    match object.get(key) {
        None | Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(value)) if value.trim().is_empty() => Ok(None),
        Some(JsonValue::String(value)) if value.len() <= max_bytes => Ok(Some(value.clone())),
        _ => Err(MOVIE_METADATA_MALFORMED),
    }
}

fn movie_metadata_optional_date(
    object: &std::collections::BTreeMap<String, JsonValue>,
    key: &str,
) -> Result<Option<String>, &'static str> {
    match object.get(key) {
        None | Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(value)) if value.is_empty() => Ok(None),
        Some(JsonValue::String(value)) if valid_movie_metadata_date(value) => {
            Ok(Some(value.clone()))
        }
        _ => Err(MOVIE_METADATA_MALFORMED),
    }
}

fn movie_metadata_optional_poster(
    object: &std::collections::BTreeMap<String, JsonValue>,
) -> Result<Option<String>, &'static str> {
    match object.get("poster_path") {
        None | Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(value))
            if value.starts_with('/')
                && value.len() <= 16 * 1024
                && !value.chars().any(char::is_control) =>
        {
            Ok(Some(value.clone()))
        }
        _ => Err(MOVIE_METADATA_MALFORMED),
    }
}

fn parse_movie_metadata_candidates(
    document: &str,
) -> Result<Vec<MovieMetadataCandidate>, &'static str> {
    let document = JsonParser::new(document)
        .parse()
        .and_then(|value| json_object(&value).cloned())
        .ok_or(MOVIE_METADATA_MALFORMED)?;
    let results = document
        .get("results")
        .and_then(json_array)
        .ok_or(MOVIE_METADATA_MALFORMED)?;
    if results.len() > MOVIE_METADATA_MAX_CANDIDATES {
        return Err(MOVIE_METADATA_MALFORMED);
    }
    let mut candidates = Vec::with_capacity(results.len());
    let mut ids = HashSet::new();
    for value in results {
        let object = json_object(value).ok_or(MOVIE_METADATA_MALFORMED)?;
        let tmdb_movie_id = json_u64(object, "id")
            .filter(|id| *id > 0)
            .ok_or(MOVIE_METADATA_MALFORMED)?;
        let title = json_string(object, "title")
            .filter(|title| !title.trim().is_empty() && title.len() <= 16 * 1024)
            .map(str::to_owned)
            .ok_or(MOVIE_METADATA_MALFORMED)?;
        if !ids.insert(tmdb_movie_id) {
            return Err(MOVIE_METADATA_MALFORMED);
        }
        candidates.push(MovieMetadataCandidate {
            tmdb_movie_id,
            title,
            original_title: movie_metadata_optional_text(object, "original_title", 16 * 1024)?,
            release_date: movie_metadata_optional_date(object, "release_date")?,
            poster_path: movie_metadata_optional_poster(object)?,
        });
    }
    Ok(candidates)
}

fn percent_encode_movie_metadata_query(query: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(query.len());
    for byte in query.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(*byte));
        } else {
            encoded.push('%');
            encoded.push(HEX[usize::from(byte >> 4)] as char);
            encoded.push(HEX[usize::from(byte & 0x0f)] as char);
        }
    }
    encoded
}

fn next_movie_metadata_operation(library: &mut MoviesLibraryContext) -> u64 {
    library.metadata_operation_generation = library.metadata_operation_generation.wrapping_add(1);
    if library.metadata_operation_generation == 0 {
        library.metadata_operation_generation = 1;
    }
    library.metadata_operation_generation
}

fn begin_movie_metadata_search(
    library: &mut MoviesLibraryContext,
    file_id: &str,
    query: &str,
    token: &str,
) -> Result<(u64, String), &'static str> {
    if query.trim().is_empty()
        || query.len() > MOVIE_METADATA_MAX_QUERY_BYTES
        || query.chars().any(char::is_control)
        || !is_valid_tmdb_token(token)
    {
        return Err(MOVIE_METADATA_CONTEXT_INVALID);
    }
    let authority = movie_metadata_authority(library, file_id)?;
    let operation_generation = next_movie_metadata_operation(library);
    let token_identity = hex_sha1(token.as_bytes());
    let request_id = hex_sha1(
        format!(
            "search\0{operation_generation}\0{}\0{}\0{query}\0{token_identity}",
            authority.file_id, authority.fingerprint
        )
        .as_bytes(),
    );
    library.metadata_verification = None;
    library.metadata_search = Some(MovieMetadataSearch {
        request_id: request_id.clone(),
        operation_generation,
        authority,
        query: query.to_owned(),
        token_identity,
        candidates: Vec::new(),
    });
    Ok((operation_generation, request_id))
}

fn encode_movie_metadata_search(search: &MovieMetadataSearch) -> Vec<String> {
    let mut response = Vec::with_capacity(2 + search.candidates.len() * 5);
    response.push(search.request_id.clone());
    response.push(search.candidates.len().to_string());
    for candidate in &search.candidates {
        response.push(candidate.tmdb_movie_id.to_string());
        response.push(candidate.title.clone());
        response.push(candidate.original_title.clone().unwrap_or_default());
        response.push(candidate.release_date.clone().unwrap_or_default());
        response.push(candidate.poster_path.clone().unwrap_or_default());
    }
    response
}

fn finish_movie_metadata_search(
    library: &mut MoviesLibraryContext,
    operation_generation: u64,
    request_id: &str,
    token: &str,
    candidates: Vec<MovieMetadataCandidate>,
) -> Result<Vec<String>, &'static str> {
    let search = library
        .metadata_search
        .as_ref()
        .filter(|search| {
            search.operation_generation == operation_generation
                && search.request_id == request_id
                && search.token_identity == hex_sha1(token.as_bytes())
        })
        .cloned()
        .ok_or(MOVIE_METADATA_CONTEXT_INVALID)?;
    validate_movie_metadata_authority(library, &search.authority)?;
    let current = library
        .metadata_search
        .as_mut()
        .ok_or(MOVIE_METADATA_CONTEXT_INVALID)?;
    current.candidates = candidates;
    Ok(encode_movie_metadata_search(current))
}

fn parse_verified_movie_metadata(
    authority: &MovieMetadataAuthority,
    tmdb_movie_id: u64,
    details_document: &str,
    external_ids_document: &str,
) -> Result<MovieMetadataAssociation, &'static str> {
    let imdb_id = verified_movie_imdb_id(tmdb_movie_id, details_document, external_ids_document)
        .map_err(|_| MOVIE_METADATA_MALFORMED)?;
    let details = JsonParser::new(details_document)
        .parse()
        .and_then(|value| json_object(&value).cloned())
        .ok_or(MOVIE_METADATA_MALFORMED)?;
    if json_u64(&details, "id") != Some(tmdb_movie_id) {
        return Err(MOVIE_METADATA_MALFORMED);
    }
    let title = json_string(&details, "title")
        .filter(|title| !title.trim().is_empty() && title.len() <= 16 * 1024)
        .map(str::to_owned)
        .ok_or(MOVIE_METADATA_MALFORMED)?;
    let association = MovieMetadataAssociation {
        folder: authority.folder.clone(),
        folder_identity: authority.folder_identity.clone(),
        relative_path: authority.relative_path.clone(),
        file_identity: authority.file_identity.clone(),
        fingerprint: authority.fingerprint.clone(),
        size: authority.size,
        tmdb_movie_id,
        imdb_id,
        title,
        original_title: movie_metadata_optional_text(&details, "original_title", 16 * 1024)?,
        release_date: movie_metadata_optional_date(&details, "release_date")?,
        poster_path: movie_metadata_optional_poster(&details)?,
        overview: movie_metadata_optional_text(&details, "overview", 256 * 1024)?,
        generation: 1,
    };
    validate_movie_metadata_association(&association).map_err(|_| MOVIE_METADATA_MALFORMED)?;
    Ok(association)
}

fn begin_movie_metadata_verification(
    library: &mut MoviesLibraryContext,
    matching_request_id: &str,
    tmdb_movie_id: u64,
    token: &str,
) -> Result<(u64, MovieMetadataSearch), &'static str> {
    if tmdb_movie_id == 0 || !is_valid_tmdb_token(token) {
        return Err(MOVIE_METADATA_CONTEXT_INVALID);
    }
    let search = library
        .metadata_search
        .as_ref()
        .filter(|search| {
            search.request_id == matching_request_id
                && search.token_identity == hex_sha1(token.as_bytes())
                && search
                    .candidates
                    .iter()
                    .any(|candidate| candidate.tmdb_movie_id == tmdb_movie_id)
        })
        .cloned()
        .ok_or(MOVIE_METADATA_CONTEXT_INVALID)?;
    validate_movie_metadata_authority(library, &search.authority)?;
    let operation_generation = next_movie_metadata_operation(library);
    library.metadata_verification = None;
    Ok((operation_generation, search))
}

fn encode_movie_metadata_association(association: &MovieMetadataAssociation) -> Vec<String> {
    vec![
        association.tmdb_movie_id.to_string(),
        association.imdb_id.clone(),
        association.title.clone(),
        association.original_title.clone().unwrap_or_default(),
        association.release_date.clone().unwrap_or_default(),
        association.poster_path.clone().unwrap_or_default(),
        association.overview.clone().unwrap_or_default(),
        association.generation.to_string(),
    ]
}

fn finish_movie_metadata_verification(
    library: &mut MoviesLibraryContext,
    operation_generation: u64,
    search: &MovieMetadataSearch,
    tmdb_movie_id: u64,
    token: &str,
    association: MovieMetadataAssociation,
) -> Result<Vec<String>, &'static str> {
    if library.metadata_operation_generation != operation_generation
        || library
            .metadata_search
            .as_ref()
            .is_none_or(|current| current.request_id != search.request_id)
        || search.token_identity != hex_sha1(token.as_bytes())
        || association.tmdb_movie_id != tmdb_movie_id
    {
        return Err(MOVIE_METADATA_CONTEXT_INVALID);
    }
    validate_movie_metadata_authority(library, &search.authority)?;
    let verification_id = hex_sha1(
        format!(
            "verify\0{operation_generation}\0{}\0{tmdb_movie_id}\0{}",
            search.request_id, association.imdb_id
        )
        .as_bytes(),
    );
    library.metadata_verification = Some(MovieMetadataVerification {
        verification_id: verification_id.clone(),
        operation_generation,
        matching_request_id: search.request_id.clone(),
        authority: search.authority.clone(),
        association: association.clone(),
        token_identity: search.token_identity.clone(),
    });
    let mut response = vec![verification_id];
    response.extend(encode_movie_metadata_association(&association));
    Ok(response)
}

fn save_movie_metadata_match_with(
    library: &mut MoviesLibraryContext,
    persistence_path: &Path,
    verification_id: &str,
) -> Result<Vec<String>, &'static str> {
    let verification = library
        .metadata_verification
        .as_ref()
        .filter(|verification| {
            verification.verification_id == verification_id
                && verification.operation_generation == library.metadata_operation_generation
        })
        .cloned()
        .ok_or(MOVIE_METADATA_CONTEXT_INVALID)?;
    validate_movie_metadata_authority(library, &verification.authority)?;
    let mut associations = read_movie_metadata_associations(persistence_path).map_err(|error| {
        if error == MovieMetadataReadError::Unavailable {
            MOVIE_METADATA_UNAVAILABLE
        } else {
            MOVIE_METADATA_PERSISTENCE_FAILED
        }
    })?;
    associations.retain(|association| {
        association.folder != verification.authority.folder
            || association.folder_identity != verification.authority.folder_identity
            || association.relative_path != verification.authority.relative_path
    });
    let generation = associations
        .iter()
        .map(|association| association.generation)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or(MOVIE_METADATA_PERSISTENCE_FAILED)?;
    let mut association = verification.association;
    association.generation = generation;
    associations.push(association.clone());
    associations.sort_by(|left, right| {
        left.folder
            .cmp(&right.folder)
            .then_with(|| left.folder_identity.cmp(&right.folder_identity))
            .then_with(|| left.relative_path.cmp(&right.relative_path))
    });
    write_movie_metadata_associations(persistence_path, &associations)?;
    let scan = library
        .completed_scan
        .as_mut()
        .ok_or(MOVIE_METADATA_STALE)?;
    let file = scan
        .files
        .iter_mut()
        .find(|file| file.file_id == verification.authority.file_id)
        .ok_or(MOVIE_METADATA_STALE)?;
    file.association = Some(association.clone());
    invalidate_movie_metadata_context(library);
    Ok(encode_movie_metadata_association(&association))
}

fn clear_movie_metadata_match_with(
    library: &mut MoviesLibraryContext,
    persistence_path: &Path,
    file_id: &str,
) -> Result<(), &'static str> {
    let authority = movie_metadata_authority(library, file_id)?;
    let mut associations = read_movie_metadata_associations(persistence_path).map_err(|error| {
        if error == MovieMetadataReadError::Unavailable {
            MOVIE_METADATA_UNAVAILABLE
        } else {
            MOVIE_METADATA_PERSISTENCE_FAILED
        }
    })?;
    let original_count = associations.len();
    associations.retain(|association| {
        association.folder != authority.folder
            || association.folder_identity != authority.folder_identity
            || association.relative_path != authority.relative_path
            || association.file_identity != authority.file_identity
            || association.fingerprint != authority.fingerprint
            || association.size != authority.size
    });
    if associations.len() == original_count {
        return Err(MOVIE_METADATA_STALE);
    }
    write_movie_metadata_associations(persistence_path, &associations)?;
    let scan = library
        .completed_scan
        .as_mut()
        .ok_or(MOVIE_METADATA_STALE)?;
    let file = scan
        .files
        .iter_mut()
        .find(|file| file.file_id == file_id)
        .ok_or(MOVIE_METADATA_STALE)?;
    file.association = None;
    invalidate_movie_metadata_context(library);
    Ok(())
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

fn movie_metadata_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|directory| directory.join(MOVIE_METADATA_FILE_NAME))
        .map_err(|_| MOVIE_METADATA_PERSISTENCE_FAILED.to_owned())
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

fn tv_metadata_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|directory| directory.join(TV_METADATA_FILE_NAME))
        .map_err(|_| TV_METADATA_PERSISTENCE_FAILED.to_owned())
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

fn library_presentation_cache_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|directory| directory.join(LIBRARY_PRESENTATION_CACHE_FILE_NAME))
        .map_err(|_| LIBRARY_PRESENTATION_FAILED.to_owned())
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
    library.completed_scan = None;
    invalidate_movie_metadata_context(&mut library);
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
    library.completed_scan = None;
    invalidate_movie_metadata_context(&mut library);
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
    library.completed_scan = None;
    invalidate_movie_metadata_context(&mut library);
    Ok(())
}

#[tauri::command]
async fn scan_movies(
    app: tauri::AppHandle,
    state: tauri::State<'_, MoviesLibraryState>,
) -> Result<Vec<String>, String> {
    let state = state.inner().clone();
    let association_path = movie_metadata_path(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut library = state.0.lock().map_err(|_| MOVIES_SCAN_FAILED.to_owned())?;
        scan_movies_library(&mut library, &association_path).map_err(str::to_owned)
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
async fn open_movie(
    path: String,
    state: tauri::State<'_, MoviesLibraryState>,
) -> Result<(), String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let library = state.0.lock().map_err(|_| MOVIE_OPEN_UNAVAILABLE)?;
        open_movie_request_with(Path::new(&path), &library, |movie_path| {
            tauri_plugin_opener::open_path(movie_path, None::<&str>).map_err(|_| ())
        })
    })
    .await
    .map_err(|_| MOVIE_OPEN_FAILED.to_owned())?
    .map_err(str::to_owned)
}

#[tauri::command]
async fn reveal_movie(
    path: String,
    state: tauri::State<'_, MoviesLibraryState>,
) -> Result<(), String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let library = state.0.lock().map_err(|_| MOVIE_REVEAL_UNAVAILABLE)?;
        reveal_movie_request_with(Path::new(&path), &library, |movie_path| {
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
async fn scan_tv_library(
    app: tauri::AppHandle,
    state: tauri::State<'_, TvLibraryState>,
) -> Result<Vec<String>, String> {
    let state = state.inner().clone();
    let association_path = tv_metadata_path(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        scan_tv_library_with_metadata(&state, &association_path).map_err(str::to_owned)
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
    app: tauri::AppHandle,
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
    let association_path = tv_metadata_path(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        trash_tv_file_with_download_ownership_and_metadata(
            Path::new(&path),
            scan_generation,
            &download_state,
            &library_state,
            &association_path,
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

fn current_library_presentation_authority(
    category: LibraryPresentationCategory,
    item_id: &str,
    scan_generation: u64,
    adult_state: &AdultLibraryState,
    download_state: &VrDownloadState,
    vr_state: &VrLibraryState,
) -> Result<LibraryItemAuthority, String> {
    match category {
        LibraryPresentationCategory::Adult => {
            adult_library_presentation_authority(adult_state, scan_generation, item_id)
                .map_err(str::to_owned)
        }
        LibraryPresentationCategory::Vr => {
            let folder = configured_vr_folder(download_state)
                .map_err(str::to_owned)?
                .ok_or_else(|| LIBRARY_PRESENTATION_STALE.to_owned())?;
            vr_library_presentation_authority(vr_state, scan_generation, &folder, item_id)
                .map_err(str::to_owned)
        }
    }
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn resolve_library_cover(
    app: tauri::AppHandle,
    category: String,
    item_id: String,
    scan_generation: String,
    cover_request_generation: String,
    adult_state: tauri::State<'_, AdultLibraryState>,
    download_state: tauri::State<'_, VrDownloadState>,
    vr_state: tauri::State<'_, VrLibraryState>,
    presentation_state: tauri::State<'_, LibraryPresentationState>,
) -> Result<Vec<String>, String> {
    let category = LibraryPresentationCategory::parse(&category)
        .ok_or_else(|| LIBRARY_PRESENTATION_STALE.to_owned())?;
    let scan_generation = scan_generation
        .parse::<u64>()
        .map_err(|_| LIBRARY_PRESENTATION_STALE.to_owned())?;
    let cover_request_generation = cover_request_generation
        .parse::<u64>()
        .ok()
        .filter(|generation| *generation > 0)
        .ok_or_else(|| LIBRARY_PRESENTATION_STALE.to_owned())?;
    let cache_path = library_presentation_cache_path(&app)?;
    let adult_state = adult_state.inner().clone();
    let download_state = download_state.inner().clone();
    let vr_state = vr_state.inner().clone();
    let presentation_state = presentation_state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let authority = current_library_presentation_authority(
            category,
            &item_id,
            scan_generation,
            &adult_state,
            &download_state,
            &vr_state,
        )?;
        begin_library_cover_request(
            &presentation_state,
            category,
            &item_id,
            cover_request_generation,
        )
        .map_err(str::to_owned)?;
        let response = resolve_library_cover_with(
            &presentation_state,
            &cache_path,
            &authority,
            cover_request_generation,
            || {
                library_cover_request_is_current(
                    &presentation_state,
                    category,
                    &item_id,
                    cover_request_generation,
                ) && current_library_presentation_authority(
                    category,
                    &item_id,
                    scan_generation,
                    &adult_state,
                    &download_state,
                    &vr_state,
                )
                .is_ok_and(|current| current == authority)
            },
        )
        .map_err(str::to_owned)?;
        let current = current_library_presentation_authority(
            category,
            &item_id,
            scan_generation,
            &adult_state,
            &download_state,
            &vr_state,
        );
        if current.as_ref().is_ok_and(|current| current == &authority)
            && library_cover_request_is_current(
                &presentation_state,
                category,
                &item_id,
                cover_request_generation,
            )
        {
            return Ok(response);
        }
        let _ = cancel_library_cover_request_with(
            &presentation_state,
            category,
            &item_id,
            cover_request_generation,
        );
        Err(LIBRARY_PRESENTATION_STALE.to_owned())
    })
    .await
    .map_err(|_| LIBRARY_PRESENTATION_FAILED.to_owned())?
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn resolve_library_metadata(
    app: tauri::AppHandle,
    category: String,
    item_id: String,
    scan_generation: String,
    adult_state: tauri::State<'_, AdultLibraryState>,
    download_state: tauri::State<'_, VrDownloadState>,
    vr_state: tauri::State<'_, VrLibraryState>,
    presentation_state: tauri::State<'_, LibraryPresentationState>,
) -> Result<Vec<String>, String> {
    let category = LibraryPresentationCategory::parse(&category)
        .ok_or_else(|| LIBRARY_PRESENTATION_STALE.to_owned())?;
    let scan_generation = scan_generation
        .parse::<u64>()
        .map_err(|_| LIBRARY_PRESENTATION_STALE.to_owned())?;
    let cache_path = library_presentation_cache_path(&app)?;
    let adult_state = adult_state.inner().clone();
    let download_state = download_state.inner().clone();
    let vr_state = vr_state.inner().clone();
    let presentation_state = presentation_state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let authority = current_library_presentation_authority(
            category,
            &item_id,
            scan_generation,
            &adult_state,
            &download_state,
            &vr_state,
        )?;
        let response =
            resolve_library_metadata_with(&presentation_state, &cache_path, &authority, || {
                current_library_presentation_authority(
                    category,
                    &item_id,
                    scan_generation,
                    &adult_state,
                    &download_state,
                    &vr_state,
                )
                .is_ok_and(|current| current == authority)
            })
            .map_err(str::to_owned)?;
        let current = current_library_presentation_authority(
            category,
            &item_id,
            scan_generation,
            &adult_state,
            &download_state,
            &vr_state,
        )?;
        (current == authority)
            .then_some(response)
            .ok_or_else(|| LIBRARY_PRESENTATION_STALE.to_owned())
    })
    .await
    .map_err(|_| LIBRARY_PRESENTATION_FAILED.to_owned())?
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn fetch_library_cover(
    category: String,
    item_id: String,
    scan_generation: String,
    cover_authority_id: String,
    adult_state: tauri::State<'_, AdultLibraryState>,
    download_state: tauri::State<'_, VrDownloadState>,
    vr_state: tauri::State<'_, VrLibraryState>,
    presentation_state: tauri::State<'_, LibraryPresentationState>,
) -> Result<Vec<u8>, String> {
    let category = LibraryPresentationCategory::parse(&category)
        .ok_or_else(|| LIBRARY_PRESENTATION_STALE.to_owned())?;
    let scan_generation = scan_generation
        .parse::<u64>()
        .map_err(|_| LIBRARY_PRESENTATION_STALE.to_owned())?;
    let adult_state = adult_state.inner().clone();
    let download_state = download_state.inner().clone();
    let vr_state = vr_state.inner().clone();
    let presentation_state = presentation_state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let authority = current_library_presentation_authority(
            category,
            &item_id,
            scan_generation,
            &adult_state,
            &download_state,
            &vr_state,
        );
        let authority = authority?;
        let bytes = fetch_library_presentation_cover_with(
            &presentation_state,
            &authority,
            &cover_authority_id,
        )
        .map_err(str::to_owned)?;
        let current = current_library_presentation_authority(
            category,
            &item_id,
            scan_generation,
            &adult_state,
            &download_state,
            &vr_state,
        )?;
        (current == authority)
            .then_some(bytes)
            .ok_or_else(|| LIBRARY_PRESENTATION_STALE.to_owned())
    })
    .await
    .map_err(|_| LIBRARY_PRESENTATION_FAILED.to_owned())?
}

#[tauri::command]
fn cancel_library_cover_request(
    category: String,
    item_id: String,
    cover_request_generation: String,
    presentation_state: tauri::State<'_, LibraryPresentationState>,
) -> Result<(), String> {
    let category = LibraryPresentationCategory::parse(&category)
        .ok_or_else(|| LIBRARY_PRESENTATION_STALE.to_owned())?;
    if !is_canonical_product_code(&item_id) {
        return Err(LIBRARY_PRESENTATION_STALE.to_owned());
    }
    let cover_request_generation = cover_request_generation
        .parse::<u64>()
        .ok()
        .filter(|generation| *generation > 0)
        .ok_or_else(|| LIBRARY_PRESENTATION_STALE.to_owned())?;
    cancel_library_cover_request_with(
        presentation_state.inner(),
        category,
        &item_id,
        cover_request_generation,
    )
    .map_err(str::to_owned)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn invalidate_library_cover(
    app: tauri::AppHandle,
    category: String,
    item_id: String,
    scan_generation: String,
    cover_request_generation: String,
    cover_authority_id: String,
    adult_state: tauri::State<'_, AdultLibraryState>,
    download_state: tauri::State<'_, VrDownloadState>,
    vr_state: tauri::State<'_, VrLibraryState>,
    presentation_state: tauri::State<'_, LibraryPresentationState>,
) -> Result<(), String> {
    let category = LibraryPresentationCategory::parse(&category)
        .ok_or_else(|| LIBRARY_PRESENTATION_STALE.to_owned())?;
    let scan_generation = scan_generation
        .parse::<u64>()
        .map_err(|_| LIBRARY_PRESENTATION_STALE.to_owned())?;
    let cover_request_generation = cover_request_generation
        .parse::<u64>()
        .ok()
        .filter(|generation| *generation > 0)
        .ok_or_else(|| LIBRARY_PRESENTATION_STALE.to_owned())?;
    let cache_path = library_presentation_cache_path(&app)?;
    let adult_state = adult_state.inner().clone();
    let download_state = download_state.inner().clone();
    let vr_state = vr_state.inner().clone();
    let presentation_state = presentation_state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if !library_cover_request_is_current(
            &presentation_state,
            category,
            &item_id,
            cover_request_generation,
        ) {
            return Err(LIBRARY_PRESENTATION_STALE.to_owned());
        }
        let authority = current_library_presentation_authority(
            category,
            &item_id,
            scan_generation,
            &adult_state,
            &download_state,
            &vr_state,
        )?;
        invalidate_library_presentation_cover_with(
            &presentation_state,
            &cache_path,
            &authority,
            cover_request_generation,
            &cover_authority_id,
        )
        .map_err(str::to_owned)
    })
    .await
    .map_err(|_| LIBRARY_PRESENTATION_FAILED.to_owned())?
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
    movie_library_state: tauri::State<'_, MoviesLibraryState>,
    tv_library_state: tauri::State<'_, TvLibraryState>,
    tv_release_state: tauri::State<'_, TvReleaseState>,
    tv_torrent_state: tauri::State<'_, TvTorrentState>,
) -> Result<(), String> {
    save_tmdb_token_file(&tmdb_token_path(&app)?, &token).map_err(str::to_owned)?;
    let mut movie_library = movie_library_state
        .0
        .lock()
        .map_err(|_| MOVIE_METADATA_UNAVAILABLE.to_owned())?;
    invalidate_movie_metadata_context(&mut movie_library);
    drop(movie_library);
    invalidate_tv_metadata_context_for_state(tv_library_state.inner()).map_err(str::to_owned)?;
    tv_release_state.invalidate().map_err(str::to_owned)?;
    tv_torrent_state
        .invalidate_inspection()
        .map_err(str::to_owned)
}

#[tauri::command]
fn clear_tmdb_token(
    app: tauri::AppHandle,
    movie_library_state: tauri::State<'_, MoviesLibraryState>,
    tv_library_state: tauri::State<'_, TvLibraryState>,
    tv_release_state: tauri::State<'_, TvReleaseState>,
    tv_torrent_state: tauri::State<'_, TvTorrentState>,
) -> Result<(), String> {
    clear_tmdb_token_file(&tmdb_token_path(&app)?).map_err(str::to_owned)?;
    let mut movie_library = movie_library_state
        .0
        .lock()
        .map_err(|_| MOVIE_METADATA_UNAVAILABLE.to_owned())?;
    invalidate_movie_metadata_context(&mut movie_library);
    drop(movie_library);
    invalidate_tv_metadata_context_for_state(tv_library_state.inner()).map_err(str::to_owned)?;
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
async fn cleanup_cancelled_vr_download(
    app: tauri::AppHandle,
    transfer_id: String,
    state: tauri::State<'_, VrDownloadState>,
) -> Result<Vec<String>, String> {
    let state = state.inner().clone();
    let persistence_path = vr_downloads_path(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        cleanup_cancelled_download(&state, &persistence_path, &transfer_id).map_err(str::to_owned)
    })
    .await
    .map_err(|_| vr_download::VR_DOWNLOAD_CLEANUP_FAILED.to_owned())?
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
#[allow(clippy::too_many_arguments)]
async fn fetch_javdb_catalog(
    category: String,
    context_generation: String,
    mode: String,
    period: String,
    year: Option<String>,
    month: Option<u8>,
    sort: String,
    count: u16,
    state: tauri::State<'_, JavdbCatalogState>,
) -> Result<Vec<String>, String> {
    let state = state.inner().clone();
    let join_error = if category == "adult" {
        ADULT_PROVIDER_ERROR
    } else {
        VR_PROVIDER_ERROR
    };
    let request = JavdbCatalogRequest {
        category,
        context_generation,
        mode,
        period,
        year,
        month,
        sort,
        count,
    };
    tauri::async_runtime::spawn_blocking(move || {
        fetch_javdb_catalog_with(&state, &request, fetch_javdb_api_document).map_err(str::to_owned)
    })
    .await
    .map_err(|_| join_error.to_owned())?
}

#[tauri::command]
async fn fetch_fanza_catalog(
    category: String,
    context_generation: String,
    feed: String,
    count: u16,
    state: tauri::State<'_, FanzaCatalogState>,
) -> Result<Vec<String>, String> {
    let state = state.inner().clone();
    let join_error = if category == "adult" {
        ADULT_PROVIDER_ERROR
    } else {
        VR_PROVIDER_ERROR
    };
    let request = FanzaCatalogRequest {
        category,
        context_generation,
        feed,
        count,
    };
    tauri::async_runtime::spawn_blocking(move || {
        fetch_fanza_catalog_with(&state, &request, fetch_fanza_graphql_document)
            .map_err(str::to_owned)
    })
    .await
    .map_err(|_| join_error.to_owned())?
}

#[tauri::command]
fn invalidate_fanza_catalog(
    category: String,
    context_generation: String,
    state: tauri::State<'_, FanzaCatalogState>,
) -> Result<(), String> {
    invalidate_fanza_catalog_with(state.inner(), &category, &context_generation)
        .map_err(str::to_owned)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn fetch_fanza_cover(
    category: String,
    context_generation: String,
    request_generation: String,
    content_id: String,
    display_code: String,
    cover_authority_id: String,
    state: tauri::State<'_, FanzaCatalogState>,
) -> Result<Vec<u8>, String> {
    let state = state.inner().clone();
    let join_error = if category == "adult" {
        ADULT_PROVIDER_ERROR
    } else {
        VR_PROVIDER_ERROR
    };
    tauri::async_runtime::spawn_blocking(move || {
        fetch_fanza_cover_with(
            &state,
            &category,
            &context_generation,
            &request_generation,
            &content_id,
            &display_code,
            &cover_authority_id,
            fetch_fanza_cover_bytes,
        )
        .map_err(str::to_owned)
    })
    .await
    .map_err(|_| join_error.to_owned())?
}

#[tauri::command]
fn invalidate_javdb_catalog(
    category: String,
    context_generation: String,
    state: tauri::State<'_, JavdbCatalogState>,
) -> Result<(), String> {
    invalidate_javdb_catalog_with(state.inner(), &category, &context_generation)
        .map_err(str::to_owned)
}

#[tauri::command]
async fn fetch_javdb_cover(
    category: String,
    request_generation: String,
    provider_item_id: String,
    cover_authority_id: String,
    state: tauri::State<'_, JavdbCatalogState>,
) -> Result<Vec<u8>, String> {
    let state = state.inner().clone();
    let join_error = if category == "adult" {
        ADULT_PROVIDER_ERROR
    } else {
        VR_PROVIDER_ERROR
    };
    tauri::async_runtime::spawn_blocking(move || {
        fetch_javdb_cover_with(
            &state,
            &category,
            &request_generation,
            &provider_item_id,
            &cover_authority_id,
            fetch_javdb_cover_bytes,
        )
        .map_err(str::to_owned)
    })
    .await
    .map_err(|_| join_error.to_owned())?
}

#[tauri::command]
async fn fetch_javdb_detail(
    category: String,
    context_generation: String,
    request_generation: String,
    provider_item_id: String,
    code: String,
    state: tauri::State<'_, JavdbCatalogState>,
) -> Result<Vec<String>, String> {
    let state = state.inner().clone();
    let join_error = if category == "adult" {
        ADULT_PROVIDER_ERROR
    } else {
        VR_PROVIDER_ERROR
    };
    let request = JavdbDetailRequest {
        category,
        context_generation,
        request_generation,
        provider_item_id,
        code,
    };
    tauri::async_runtime::spawn_blocking(move || {
        fetch_javdb_detail_with(&state, &request, fetch_javdb_api_document).map_err(str::to_owned)
    })
    .await
    .map_err(|_| join_error.to_owned())?
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn fetch_javdb_detail_image(
    category: String,
    context_generation: String,
    request_generation: String,
    provider_item_id: String,
    code: String,
    detail_generation: String,
    image_authority_id: String,
    state: tauri::State<'_, JavdbCatalogState>,
) -> Result<Vec<u8>, String> {
    let state = state.inner().clone();
    let join_error = if category == "adult" {
        ADULT_PROVIDER_ERROR
    } else {
        VR_PROVIDER_ERROR
    };
    let request = JavdbDetailRequest {
        category,
        context_generation,
        request_generation,
        provider_item_id,
        code,
    };
    tauri::async_runtime::spawn_blocking(move || {
        fetch_javdb_detail_image_with(
            &state,
            &request,
            &detail_generation,
            &image_authority_id,
            fetch_javdb_cover_bytes,
        )
        .map_err(str::to_owned)
    })
    .await
    .map_err(|_| join_error.to_owned())?
}

#[tauri::command]
fn invalidate_javdb_detail(
    category: String,
    detail_generation: String,
    state: tauri::State<'_, JavdbCatalogState>,
) -> Result<(), String> {
    invalidate_javdb_detail_with(state.inner(), &category, &detail_generation)
        .map_err(str::to_owned)
}

#[tauri::command]
async fn open_javdb_detail_source(
    category: String,
    context_generation: String,
    request_generation: String,
    provider_item_id: String,
    code: String,
    detail_generation: String,
    state: tauri::State<'_, JavdbCatalogState>,
) -> Result<(), String> {
    let state = state.inner().clone();
    let join_error = if category == "adult" {
        ADULT_PROVIDER_ERROR
    } else {
        VR_PROVIDER_ERROR
    };
    let request = JavdbDetailRequest {
        category,
        context_generation,
        request_generation,
        provider_item_id,
        code,
    };
    tauri::async_runtime::spawn_blocking(move || {
        open_javdb_detail_source_with(&state, &request, &detail_generation, |url| {
            tauri_plugin_opener::open_url(url, None::<&str>).map_err(|_| ())
        })
        .map_err(str::to_owned)
    })
    .await
    .map_err(|_| join_error.to_owned())?
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
async fn search_movie_metadata(
    app: tauri::AppHandle,
    file_id: String,
    query: String,
    context_generation: u64,
    state: tauri::State<'_, MoviesLibraryState>,
) -> Result<Vec<String>, String> {
    let token_path = tmdb_token_path(&app)?;
    let token = load_tmdb_token_file(&token_path)
        .map_err(str::to_owned)?
        .ok_or_else(|| MOVIE_TMDB_UNAUTHORIZED.to_owned())?;
    let state = state.inner().clone();
    let (operation_generation, request_id) = {
        let mut library = state
            .0
            .lock()
            .map_err(|_| MOVIE_METADATA_UNAVAILABLE.to_owned())?;
        begin_movie_metadata_client_operation(&mut library, context_generation)
            .map_err(str::to_owned)?;
        begin_movie_metadata_search(&mut library, &file_id, &query, &token)
            .map_err(str::to_owned)?
    };
    tauri::async_runtime::spawn_blocking(move || {
        let url = format!(
            "{TMDB_MOVIE_SEARCH_URL}{}",
            percent_encode_movie_metadata_query(&query)
        );
        let document = fetch_movie_provider_document(&url, Some(&token))
            .map_err(tmdb_movie_provider_error_code)?;
        let candidates = parse_movie_metadata_candidates(&document)?;
        if load_tmdb_token_file(&token_path).ok().flatten().as_deref() != Some(token.as_str()) {
            return Err(MOVIE_METADATA_CONTEXT_INVALID);
        }
        let mut library = state.0.lock().map_err(|_| MOVIE_METADATA_UNAVAILABLE)?;
        finish_movie_metadata_search(
            &mut library,
            operation_generation,
            &request_id,
            &token,
            candidates,
        )
    })
    .await
    .map_err(|_| MOVIE_TMDB_PROVIDER_ERROR.to_owned())?
    .map_err(str::to_owned)
}

#[tauri::command]
async fn verify_movie_metadata_candidate(
    app: tauri::AppHandle,
    matching_request_id: String,
    tmdb_movie_id: u64,
    context_generation: u64,
    state: tauri::State<'_, MoviesLibraryState>,
) -> Result<Vec<String>, String> {
    let token_path = tmdb_token_path(&app)?;
    let token = load_tmdb_token_file(&token_path)
        .map_err(str::to_owned)?
        .ok_or_else(|| MOVIE_TMDB_UNAUTHORIZED.to_owned())?;
    let state = state.inner().clone();
    let (operation_generation, search) = {
        let mut library = state
            .0
            .lock()
            .map_err(|_| MOVIE_METADATA_UNAVAILABLE.to_owned())?;
        begin_movie_metadata_client_operation(&mut library, context_generation)
            .map_err(str::to_owned)?;
        begin_movie_metadata_verification(&mut library, &matching_request_id, tmdb_movie_id, &token)
            .map_err(str::to_owned)?
    };
    tauri::async_runtime::spawn_blocking(move || {
        let details_url = format!("{TMDB_MOVIE_URL}{tmdb_movie_id}");
        let details = fetch_movie_provider_document(&details_url, Some(&token))
            .map_err(tmdb_movie_provider_error_code)?;
        let external_ids =
            fetch_movie_provider_document(&format!("{details_url}/external_ids"), Some(&token))
                .map_err(tmdb_movie_provider_error_code)?;
        let association = parse_verified_movie_metadata(
            &search.authority,
            tmdb_movie_id,
            &details,
            &external_ids,
        )?;
        if load_tmdb_token_file(&token_path).ok().flatten().as_deref() != Some(token.as_str()) {
            return Err(MOVIE_METADATA_CONTEXT_INVALID);
        }
        let mut library = state.0.lock().map_err(|_| MOVIE_METADATA_UNAVAILABLE)?;
        finish_movie_metadata_verification(
            &mut library,
            operation_generation,
            &search,
            tmdb_movie_id,
            &token,
            association,
        )
    })
    .await
    .map_err(|_| MOVIE_TMDB_PROVIDER_ERROR.to_owned())?
    .map_err(str::to_owned)
}

#[tauri::command]
async fn save_movie_metadata_match(
    app: tauri::AppHandle,
    verification_id: String,
    state: tauri::State<'_, MoviesLibraryState>,
) -> Result<Vec<String>, String> {
    let persistence_path = movie_metadata_path(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut library = state.0.lock().map_err(|_| MOVIE_METADATA_UNAVAILABLE)?;
        save_movie_metadata_match_with(&mut library, &persistence_path, &verification_id)
    })
    .await
    .map_err(|_| MOVIE_METADATA_PERSISTENCE_FAILED.to_owned())?
    .map_err(str::to_owned)
}

#[tauri::command]
async fn clear_movie_metadata_match(
    app: tauri::AppHandle,
    file_id: String,
    state: tauri::State<'_, MoviesLibraryState>,
) -> Result<(), String> {
    let persistence_path = movie_metadata_path(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut library = state.0.lock().map_err(|_| MOVIE_METADATA_UNAVAILABLE)?;
        clear_movie_metadata_match_with(&mut library, &persistence_path, &file_id)
    })
    .await
    .map_err(|_| MOVIE_METADATA_PERSISTENCE_FAILED.to_owned())?
    .map_err(str::to_owned)
}

#[tauri::command]
fn invalidate_movie_metadata_match_context(
    context_generation: u64,
    state: tauri::State<'_, MoviesLibraryState>,
) -> Result<(), String> {
    let mut library = state
        .0
        .lock()
        .map_err(|_| MOVIE_METADATA_UNAVAILABLE.to_owned())?;
    invalidate_movie_metadata_client_context(&mut library, context_generation);
    Ok(())
}

#[tauri::command]
async fn search_tv_show_metadata(
    app: tauri::AppHandle,
    group_id: String,
    query: String,
    context_generation: u64,
    state: tauri::State<'_, TvLibraryState>,
) -> Result<Vec<String>, String> {
    let token_path = tmdb_token_path(&app)?;
    let token = load_tmdb_token_file(&token_path)
        .map_err(str::to_owned)?
        .ok_or_else(|| TV_METADATA_TMDB_UNAUTHORIZED.to_owned())?;
    let state = state.inner().clone();
    let (operation_generation, request_id) =
        begin_tv_metadata_search(&state, &group_id, &query, &token, context_generation)
            .map_err(str::to_owned)?;
    tauri::async_runtime::spawn_blocking(move || {
        let url = format!(
            "{TMDB_TV_SEARCH_URL}{}",
            percent_encode_tv_metadata_query(&query)
        );
        let document = fetch_movie_provider_document(&url, Some(&token))
            .map_err(tmdb_tv_metadata_provider_error_code)?;
        let candidates = parse_tv_metadata_candidates(&document)?;
        if load_tmdb_token_file(&token_path).ok().flatten().as_deref() != Some(token.as_str()) {
            return Err(TV_METADATA_CONTEXT_INVALID);
        }
        finish_tv_metadata_search(
            &state,
            operation_generation,
            &request_id,
            &token,
            candidates,
        )
    })
    .await
    .map_err(|_| TV_METADATA_TMDB_PROVIDER_ERROR.to_owned())?
    .map_err(str::to_owned)
}

#[tauri::command]
async fn verify_tv_show_metadata_candidate(
    app: tauri::AppHandle,
    matching_request_id: String,
    tmdb_tv_id: u64,
    context_generation: u64,
    state: tauri::State<'_, TvLibraryState>,
) -> Result<Vec<String>, String> {
    let token_path = tmdb_token_path(&app)?;
    let token = load_tmdb_token_file(&token_path)
        .map_err(str::to_owned)?
        .ok_or_else(|| TV_METADATA_TMDB_UNAUTHORIZED.to_owned())?;
    let state = state.inner().clone();
    let (operation_generation, search) = begin_tv_metadata_verification(
        &state,
        &matching_request_id,
        tmdb_tv_id,
        &token,
        context_generation,
    )
    .map_err(str::to_owned)?;
    tauri::async_runtime::spawn_blocking(move || {
        let details_url = format!("{TMDB_TV_URL}{tmdb_tv_id}");
        let details = fetch_movie_provider_document(&details_url, Some(&token))
            .map_err(tmdb_tv_metadata_provider_error_code)?;
        let external_ids =
            fetch_movie_provider_document(&format!("{details_url}/external_ids"), Some(&token))
                .map_err(tmdb_tv_metadata_provider_error_code)?;
        let association = parse_verified_tv_metadata(&search, tmdb_tv_id, &details, &external_ids)?;
        if load_tmdb_token_file(&token_path).ok().flatten().as_deref() != Some(token.as_str()) {
            return Err(TV_METADATA_CONTEXT_INVALID);
        }
        finish_tv_metadata_verification(
            &state,
            operation_generation,
            &search,
            tmdb_tv_id,
            &token,
            association,
        )
    })
    .await
    .map_err(|_| TV_METADATA_TMDB_PROVIDER_ERROR.to_owned())?
    .map_err(str::to_owned)
}

#[tauri::command]
async fn save_tv_show_metadata_match(
    app: tauri::AppHandle,
    verification_id: String,
    state: tauri::State<'_, TvLibraryState>,
) -> Result<Vec<String>, String> {
    let persistence_path = tv_metadata_path(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        save_tv_metadata_match_with(&state, &persistence_path, &verification_id)
    })
    .await
    .map_err(|_| TV_METADATA_PERSISTENCE_FAILED.to_owned())?
    .map_err(str::to_owned)
}

#[tauri::command]
async fn clear_tv_show_metadata_match(
    app: tauri::AppHandle,
    group_id: String,
    state: tauri::State<'_, TvLibraryState>,
) -> Result<(), String> {
    let persistence_path = tv_metadata_path(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        clear_tv_metadata_match_with(&state, &persistence_path, &group_id)
    })
    .await
    .map_err(|_| TV_METADATA_PERSISTENCE_FAILED.to_owned())?
    .map_err(str::to_owned)
}

#[tauri::command]
fn invalidate_tv_show_metadata_context(
    context_generation: u64,
    state: tauri::State<'_, TvLibraryState>,
) -> Result<(), String> {
    invalidate_tv_metadata_client_context(state.inner(), context_generation).map_err(str::to_owned)
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
        .manage(FanzaCatalogState::default())
        .manage(JavdbCatalogState::default())
        .manage(LibraryPresentationState::default())
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
            search_movie_metadata,
            verify_movie_metadata_candidate,
            save_movie_metadata_match,
            clear_movie_metadata_match,
            invalidate_movie_metadata_match_context,
            load_tv_folder,
            choose_tv_folder,
            clear_tv_folder,
            scan_tv_library,
            query_tv_storage,
            open_tv_file,
            reveal_tv_file,
            trash_tv_file,
            search_tv_show_metadata,
            verify_tv_show_metadata_candidate,
            save_tv_show_metadata_match,
            clear_tv_show_metadata_match,
            invalidate_tv_show_metadata_context,
            load_adult_folder,
            choose_adult_folder,
            clear_adult_folder,
            scan_adult_library,
            query_adult_storage,
            resolve_library_cover,
            resolve_library_metadata,
            fetch_library_cover,
            cancel_library_cover_request,
            invalidate_library_cover,
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
            cleanup_cancelled_vr_download,
            dismiss_vr_download,
            preview_vr_organization,
            apply_vr_organization,
            dismiss_vr_organization,
            fetch_javdb_vr_catalog,
            fetch_javdb_adult_catalog,
            fetch_fanza_catalog,
            invalidate_fanza_catalog,
            fetch_fanza_cover,
            fetch_javdb_catalog,
            invalidate_javdb_catalog,
            fetch_javdb_cover,
            fetch_javdb_detail,
            fetch_javdb_detail_image,
            invalidate_javdb_detail,
            open_javdb_detail_source,
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
        begin_movie_metadata_client_operation, begin_movie_metadata_search,
        begin_movie_metadata_verification, clear_movie_metadata_match_with,
        clear_movies_folder_file, clear_tmdb_token_file, fetch_javdb_adult_catalog_with,
        fetch_javdb_vr_catalog_with, fetch_sukebei_adult_releases_with,
        fetch_sukebei_vr_releases_with, fetch_yts_movie_releases_with,
        finish_movie_metadata_search, finish_movie_metadata_verification,
        invalidate_movie_metadata_client_context, load_movies_folder_file, load_tmdb_token_file,
        movie_metadata_error, open_movie_path_with, open_movie_request_with,
        parse_movie_metadata_candidates, parse_movie_provider_response, parse_provider_response,
        parse_verified_movie_metadata, query_movies_volume_storage_with, reveal_movie_path_with,
        reveal_movie_request_with, save_movie_metadata_match_with, save_movies_folder_file,
        save_tmdb_token_file, scan_movie_paths, scan_movies_library, trash_movie_path_with,
        trash_movie_request_with, MoviePathValidationError, MovieProviderRequestError,
        MovieTorrentState, MoviesLibraryContext, MoviesVolumeStorageQueryError,
        ProviderRequestError, TrashMovieRequest, ADULT_PROVIDER_ERROR, MOVIES_FOLDER_UNAVAILABLE,
        MOVIES_STORAGE_FAILED, MOVIES_STORAGE_UNAVAILABLE, MOVIE_METADATA_CONTEXT_INVALID,
        MOVIE_METADATA_MALFORMED, MOVIE_METADATA_PERSISTENCE_FAILED, MOVIE_METADATA_STALE,
        MOVIE_OPEN_FAILED, MOVIE_OPEN_NOT_FILE, MOVIE_OPEN_NOT_FOUND, MOVIE_OPEN_UNAVAILABLE,
        MOVIE_OPEN_UNSUPPORTED, MOVIE_REVEAL_FAILED, MOVIE_REVEAL_NOT_FILE, MOVIE_REVEAL_NOT_FOUND,
        MOVIE_REVEAL_UNAVAILABLE, MOVIE_REVEAL_UNSUPPORTED, MOVIE_TRASH_FAILED,
        MOVIE_TRASH_FOLDER_UNAVAILABLE, MOVIE_TRASH_NOT_FILE, MOVIE_TRASH_NOT_FOUND,
        MOVIE_TRASH_OUTSIDE_FOLDER, MOVIE_TRASH_STALE, MOVIE_TRASH_UNAVAILABLE,
        MOVIE_TRASH_UNSUPPORTED, TMDB_TOKEN_INVALID, VR_PROVIDER_ERROR,
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

    fn scan_trusted_movie_fixture(
        fixture: &FilesystemFixture,
        association_path: &Path,
    ) -> MoviesLibraryContext {
        let folder = fs::canonicalize(&fixture.path).expect("fixture folder must canonicalize");
        let mut library = MoviesLibraryContext {
            folder: Some(folder),
            ..MoviesLibraryContext::default()
        };
        scan_movies_library(&mut library, association_path)
            .expect("trusted Movie scan must succeed");
        library
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
        let fourth_movie = fixture.create_file("clip.mov");
        fs::create_dir(fixture.path.join("directory.mkv"))
            .expect("failed to create fixture directory");

        let mut expected_paths = vec![
            path_string(first_movie),
            path_string(second_movie),
            path_string(third_movie),
            path_string(fourth_movie),
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
    fn persists_reloads_and_clears_only_an_explicit_verified_movie_match() {
        let fixture = FilesystemFixture::new();
        let created_path = fixture.create_file("Nested/映画  —  Exact.Movie.MKV");
        fs::write(&created_path, b"exact local movie bytes")
            .expect("failed to write fixture Movie bytes");
        let association_path = fixture.path.join("movie-metadata");
        let mut library = scan_trusted_movie_fixture(&fixture, &association_path);
        let scanned_file = library
            .completed_scan
            .as_ref()
            .and_then(|scan| scan.files.first())
            .cloned()
            .expect("trusted scan must contain the Movie");

        let candidates = parse_movie_metadata_candidates(
            r#"{"results":[{"id":101,"title":"同じ題名","original_title":"Original One","release_date":"2001-01-01","poster_path":"/one.jpg"},{"id":202,"title":"同じ題名","original_title":"Original Two","release_date":"2002-02-02","poster_path":"/two.jpg"}]}"#,
        )
        .expect("valid same-title candidates must parse");
        let (search_generation, matching_request_id) = begin_movie_metadata_search(
            &mut library,
            &scanned_file.file_id,
            "同じ  題名",
            "fixture-token",
        )
        .expect("explicit matching search must begin");
        let search_response = finish_movie_metadata_search(
            &mut library,
            search_generation,
            &matching_request_id,
            "fixture-token",
            candidates,
        )
        .expect("matching search must finish");
        assert_eq!(search_response[1], "2");
        assert!(library.metadata_verification.is_none());

        let (verification_generation, search) = begin_movie_metadata_verification(
            &mut library,
            &matching_request_id,
            202,
            "fixture-token",
        )
        .expect("the manually chosen candidate must begin verification");
        let association = parse_verified_movie_metadata(
            &search.authority,
            202,
            r#"{"id":202,"title":"Accepted  Title — 特別版","original_title":"Original Two","release_date":"2002-02-02","poster_path":"/two.jpg","overview":"Exact verified overview."}"#,
            r#"{"id":202,"imdb_id":"tt7654321"}"#,
        )
        .expect("exact TMDB and canonical IMDb details must verify");
        let verification_response = finish_movie_metadata_verification(
            &mut library,
            verification_generation,
            &search,
            202,
            "fixture-token",
            association,
        )
        .expect("manual verification must finish");
        assert_eq!(verification_response[3], "Accepted  Title — 特別版");
        assert_eq!(verification_response[2], "tt7654321");

        let saved = save_movie_metadata_match_with(
            &mut library,
            &association_path,
            &verification_response[0],
        )
        .expect("verified association must persist");
        assert_eq!(saved[0], "202");
        assert_eq!(saved[1], "tt7654321");
        assert_eq!(saved[2], "Accepted  Title — 特別版");
        assert_eq!(
            fs::read(&scanned_file.path).unwrap(),
            b"exact local movie bytes"
        );

        let mut restarted = scan_trusted_movie_fixture(&fixture, &association_path);
        let restarted_file = restarted
            .completed_scan
            .as_ref()
            .and_then(|scan| scan.files.first())
            .cloned()
            .expect("restarted scan must contain the exact Movie");
        let restarted_association = restarted_file
            .association
            .as_ref()
            .expect("exact persisted metadata must load without a provider request");
        assert_eq!(restarted_association.title, "Accepted  Title — 特別版");
        assert_eq!(restarted_association.imdb_id, "tt7654321");

        clear_movie_metadata_match_with(&mut restarted, &association_path, &restarted_file.file_id)
            .expect("metadata-only clearing must persist");
        assert_eq!(
            fs::read(&scanned_file.path).unwrap(),
            b"exact local movie bytes"
        );
        let cleared = scan_trusted_movie_fixture(&fixture, &association_path);
        assert!(cleared.completed_scan.as_ref().unwrap().files[0]
            .association
            .is_none());
    }

    #[test]
    fn delayed_client_invalidation_cannot_erase_a_newer_search_or_verification() {
        let fixture = FilesystemFixture::new();
        fixture.create_file("Exact.mp4");
        let association_path = fixture.path.join("movie-metadata");
        let mut library = scan_trusted_movie_fixture(&fixture, &association_path);
        let file_id = library.completed_scan.as_ref().unwrap().files[0]
            .file_id
            .clone();
        let candidates = parse_movie_metadata_candidates(
            r#"{"results":[{"id":419,"title":"Exact Movie","release_date":"1999-04-19"}]}"#,
        )
        .unwrap();

        begin_movie_metadata_client_operation(&mut library, 2).unwrap();
        let (search_generation, request_id) =
            begin_movie_metadata_search(&mut library, &file_id, "Exact Movie", "fixture-token")
                .unwrap();
        finish_movie_metadata_search(
            &mut library,
            search_generation,
            &request_id,
            "fixture-token",
            candidates,
        )
        .unwrap();
        invalidate_movie_metadata_client_context(&mut library, 1);
        assert_eq!(
            library.metadata_search.as_ref().unwrap().request_id,
            request_id
        );

        begin_movie_metadata_client_operation(&mut library, 3).unwrap();
        let (verification_generation, search) =
            begin_movie_metadata_verification(&mut library, &request_id, 419, "fixture-token")
                .unwrap();
        let association = parse_verified_movie_metadata(
            &search.authority,
            419,
            r#"{"id":419,"title":"Exact Movie","release_date":"1999-04-19"}"#,
            r#"{"id":419,"imdb_id":"tt0123456"}"#,
        )
        .unwrap();
        invalidate_movie_metadata_client_context(&mut library, 2);
        let verified = finish_movie_metadata_verification(
            &mut library,
            verification_generation,
            &search,
            419,
            "fixture-token",
            association,
        )
        .unwrap();
        assert_eq!(verified[2], "tt0123456");
        assert!(library.metadata_verification.is_some());
        assert_eq!(
            begin_movie_metadata_client_operation(&mut library, 3),
            Err(MOVIE_METADATA_CONTEXT_INVALID)
        );

        invalidate_movie_metadata_client_context(&mut library, 4);
        assert!(library.metadata_search.is_none());
        assert!(library.metadata_verification.is_none());
    }

    #[test]
    fn rejects_stale_searches_and_prevents_moved_or_replaced_files_from_inheriting_metadata() {
        let fixture = FilesystemFixture::new();
        let created_path = fixture.create_file("Exact.mp4");
        fs::write(&created_path, b"original movie").expect("failed to write original Movie bytes");
        let association_path = fixture.path.join("movie-metadata");
        let mut library = scan_trusted_movie_fixture(&fixture, &association_path);
        let original = library.completed_scan.as_ref().unwrap().files[0].clone();
        let candidates = parse_movie_metadata_candidates(
            r#"{"results":[{"id":419,"title":"Exact Movie","release_date":"1999-04-19","poster_path":null}]}"#,
        )
        .unwrap();
        let (stale_generation, stale_request_id) = begin_movie_metadata_search(
            &mut library,
            &original.file_id,
            "Exact Movie",
            "fixture-token",
        )
        .unwrap();
        fs::write(&original.path, b"changed movie bytes")
            .expect("failed to change the fixture Movie");
        assert_eq!(
            finish_movie_metadata_search(
                &mut library,
                stale_generation,
                &stale_request_id,
                "fixture-token",
                candidates.clone(),
            ),
            Err(MOVIE_METADATA_STALE)
        );
        assert!(!association_path.exists());

        let mut current = scan_trusted_movie_fixture(&fixture, &association_path);
        let current_file = current.completed_scan.as_ref().unwrap().files[0].clone();
        let (token_generation, token_request_id) = begin_movie_metadata_search(
            &mut current,
            &current_file.file_id,
            "Exact Movie",
            "different-token",
        )
        .unwrap();
        assert_eq!(
            finish_movie_metadata_search(
                &mut current,
                token_generation,
                &token_request_id,
                "fixture-token",
                candidates.clone(),
            ),
            Err(MOVIE_METADATA_CONTEXT_INVALID)
        );

        let (search_generation, request_id) = begin_movie_metadata_search(
            &mut current,
            &current_file.file_id,
            "Exact Movie",
            "fixture-token",
        )
        .unwrap();
        finish_movie_metadata_search(
            &mut current,
            search_generation,
            &request_id,
            "fixture-token",
            candidates,
        )
        .unwrap();
        let (verification_generation, search) =
            begin_movie_metadata_verification(&mut current, &request_id, 419, "fixture-token")
                .unwrap();
        let association = parse_verified_movie_metadata(
            &search.authority,
            419,
            r#"{"id":419,"title":"Exact Movie","release_date":"1999-04-19"}"#,
            r#"{"id":419,"imdb_id":"tt0123456"}"#,
        )
        .unwrap();
        let verified = finish_movie_metadata_verification(
            &mut current,
            verification_generation,
            &search,
            419,
            "fixture-token",
            association,
        )
        .unwrap();
        save_movie_metadata_match_with(&mut current, &association_path, &verified[0]).unwrap();

        let other_fixture = FilesystemFixture::new();
        let other_folder = fs::canonicalize(&other_fixture.path).unwrap();
        let moved_path = other_folder.join("Moved.mp4");
        fs::rename(&current_file.path, &moved_path).expect("failed to move associated Movie");
        fs::write(&current_file.path, b"changed movie bytes")
            .expect("failed to create replacement Movie");

        let replacement_scan = scan_trusted_movie_fixture(&fixture, &association_path);
        assert!(replacement_scan.completed_scan.as_ref().unwrap().files[0]
            .association
            .is_none());
        let moved_scan = scan_trusted_movie_fixture(&other_fixture, &association_path);
        assert!(moved_scan.completed_scan.as_ref().unwrap().files[0]
            .association
            .is_none());
        assert_eq!(fs::read(moved_path).unwrap(), b"changed movie bytes");
    }

    #[test]
    fn rejects_malformed_or_conflicting_tmdb_movie_identity_documents() {
        assert_eq!(
            parse_movie_metadata_candidates(
                r#"{"results":[{"id":7,"title":"Duplicate"},{"id":7,"title":"Conflict"}]}"#,
            ),
            Err(MOVIE_METADATA_MALFORMED)
        );
        for document in [
            r#"{"results":[{"id":0,"title":"Invalid"}]}"#,
            r#"{"results":[{"id":1,"title":"   "}]}"#,
            r#"{"results":[{"id":1,"title":"Invalid","release_date":"2024"}]}"#,
            r#"{"results":[{"id":1,"title":"Invalid","poster_path":"https://example.invalid/poster.jpg"}]}"#,
            r#"{"results":null}"#,
        ] {
            assert_eq!(
                parse_movie_metadata_candidates(document),
                Err(MOVIE_METADATA_MALFORMED)
            );
        }

        let fixture = FilesystemFixture::new();
        fixture.create_file("Exact.mp4");
        let association_path = fixture.path.join("movie-metadata");
        let library = scan_trusted_movie_fixture(&fixture, &association_path);
        let file_id = &library.completed_scan.as_ref().unwrap().files[0].file_id;
        let authority = super::movie_metadata_authority(&library, file_id).unwrap();
        for (details, external_ids) in [
            (
                r#"{"id":420,"title":"Wrong Movie"}"#,
                r#"{"id":419,"imdb_id":"tt0123456"}"#,
            ),
            (
                r#"{"id":419,"title":"Exact Movie"}"#,
                r#"{"id":420,"imdb_id":"tt0123456"}"#,
            ),
            (
                r#"{"id":419,"title":"Exact Movie"}"#,
                r#"{"id":419,"imdb_id":null}"#,
            ),
        ] {
            assert_eq!(
                parse_verified_movie_metadata(&authority, 419, details, external_ids),
                Err(MOVIE_METADATA_MALFORMED)
            );
        }
        assert_eq!(
            parse_verified_movie_metadata(
                &authority,
                419,
                r#"{"id":419,"title":"Exact Movie"}"#,
                r#"{"id":419,"imdb_id":"TT0123456"}"#,
            )
            .unwrap()
            .imdb_id,
            "tt0123456"
        );
    }

    #[test]
    fn keeps_local_movies_available_when_metadata_storage_is_corrupt_or_oversized() {
        let fixture = FilesystemFixture::new();
        let movie_path = fixture.create_file("Available.mp4");
        fs::write(&movie_path, b"available local bytes").unwrap();
        let association_path = fixture.path.join("movie-metadata");
        fs::write(&association_path, b"corrupt association data").unwrap();

        let mut library = scan_trusted_movie_fixture(&fixture, &association_path);
        let scan = library.completed_scan.as_ref().unwrap();
        assert_eq!(scan.association_status, "attention");
        assert_eq!(scan.files.len(), 1);
        assert!(scan.files[0].association.is_none());
        let file_id = scan.files[0].file_id.clone();
        let candidates =
            parse_movie_metadata_candidates(r#"{"results":[{"id":419,"title":"Exact Movie"}]}"#)
                .unwrap();
        let (search_generation, request_id) =
            begin_movie_metadata_search(&mut library, &file_id, "Exact Movie", "fixture-token")
                .unwrap();
        finish_movie_metadata_search(
            &mut library,
            search_generation,
            &request_id,
            "fixture-token",
            candidates,
        )
        .unwrap();
        let (verification_generation, search) =
            begin_movie_metadata_verification(&mut library, &request_id, 419, "fixture-token")
                .unwrap();
        let association = parse_verified_movie_metadata(
            &search.authority,
            419,
            r#"{"id":419,"title":"Exact Movie"}"#,
            r#"{"id":419,"imdb_id":"tt0123456"}"#,
        )
        .unwrap();
        let verified = finish_movie_metadata_verification(
            &mut library,
            verification_generation,
            &search,
            419,
            "fixture-token",
            association,
        )
        .unwrap();
        assert_eq!(
            save_movie_metadata_match_with(&mut library, &association_path, &verified[0]),
            Err(MOVIE_METADATA_PERSISTENCE_FAILED)
        );
        assert_eq!(fs::read(&movie_path).unwrap(), b"available local bytes");

        fs::write(
            &association_path,
            vec![0; super::MOVIE_METADATA_MAX_BYTES as usize + 1],
        )
        .unwrap();
        let oversized = scan_trusted_movie_fixture(&fixture, &association_path);
        assert_eq!(
            oversized
                .completed_scan
                .as_ref()
                .unwrap()
                .association_status,
            "attention"
        );
        assert_eq!(oversized.completed_scan.as_ref().unwrap().files.len(), 1);

        let oversized_file = &oversized.completed_scan.as_ref().unwrap().files[0];
        let authority = super::movie_metadata_authority(&oversized, &oversized_file.file_id)
            .expect("the local Movie must remain trusted");
        let association = super::MovieMetadataAssociation {
            folder: authority.folder,
            folder_identity: authority.folder_identity,
            relative_path: authority.relative_path,
            file_identity: authority.file_identity,
            fingerprint: authority.fingerprint,
            size: authority.size,
            tmdb_movie_id: 419,
            imdb_id: "tt0123456".to_owned(),
            title: "Exact Movie".to_owned(),
            original_title: None,
            release_date: None,
            poster_path: None,
            overview: None,
            generation: 1,
        };
        let conflicting = super::MovieMetadataAssociation {
            tmdb_movie_id: 420,
            imdb_id: "tt7654321".to_owned(),
            title: "Conflicting Movie".to_owned(),
            generation: 2,
            ..association.clone()
        };
        fs::write(
            &association_path,
            super::encoded_movie_metadata_associations(&[association, conflicting]).unwrap(),
        )
        .unwrap();
        let conflicting = scan_trusted_movie_fixture(&fixture, &association_path);
        assert_eq!(
            conflicting
                .completed_scan
                .as_ref()
                .unwrap()
                .association_status,
            "attention"
        );
        assert!(conflicting.completed_scan.as_ref().unwrap().files[0]
            .association
            .is_none());
    }

    #[test]
    fn opens_the_exact_supported_movie_path_after_validation() {
        let fixture = FilesystemFixture::new();
        fixture.create_file("映画  —  Final.CUT!.AVI");
        let association_path = fixture.path.join("metadata-associations");
        let library = scan_trusted_movie_fixture(&fixture, &association_path);
        let movie_path = PathBuf::from(&library.movie_paths[0]);
        let dispatched_path = RefCell::new(None);

        let result = open_movie_request_with(&movie_path, &library, |path| {
            dispatched_path.replace(Some(path.to_path_buf()));
            Ok(())
        });

        assert_eq!(result, Ok(()));
        assert_eq!(dispatched_path.into_inner(), Some(movie_path));
    }

    #[test]
    fn reveals_the_exact_supported_movie_path_after_validation() {
        let fixture = FilesystemFixture::new();
        fixture.create_file("映画  —  Final.CUT!.MOV");
        let association_path = fixture.path.join("metadata-associations");
        let library = scan_trusted_movie_fixture(&fixture, &association_path);
        let movie_path = PathBuf::from(&library.movie_paths[0]);
        let dispatched_path = RefCell::new(None);

        let result = reveal_movie_request_with(&movie_path, &library, |path| {
            dispatched_path.replace(Some(path.to_path_buf()));
            Ok(())
        });

        assert_eq!(result, Ok(()));
        assert_eq!(dispatched_path.into_inner(), Some(movie_path));
    }

    #[test]
    fn trashes_the_exact_current_movie_path_after_root_validation() {
        let fixture = FilesystemFixture::new();
        fixture.create_file("nested/映画  —  Final.CUT!.WMV");
        let association_path = fixture.path.join("metadata-associations");
        let mut library = scan_trusted_movie_fixture(&fixture, &association_path);
        let movie_paths = library.movie_paths.clone();
        let movie_path = PathBuf::from(&movie_paths[0]);
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
        trusted_fixture.create_file("Trusted.mp4");
        let unrelated_fixture = FilesystemFixture::new();
        let unrelated_movie = unrelated_fixture.create_file("Unrelated.mkv");
        let unrelated_movie_path = path_string(unrelated_movie.clone());
        let association_path = trusted_fixture.path.join("metadata-associations");
        let mut trusted_library = scan_trusted_movie_fixture(&trusted_fixture, &association_path);
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
