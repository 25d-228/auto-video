use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    time::SystemTime,
};

use crate::vr_download::{
    with_unowned_tv_library_path, VrDownloadState, VrLibraryTrashOwnershipError,
};

pub const TV_FOLDER_STORAGE_FAILED: &str = "tv_folder_storage_failed";
pub const TV_FOLDER_UNAVAILABLE: &str = "tv_folder_unavailable";
pub const TV_LIBRARY_SCAN_FAILED: &str = "tv_library_scan_failed";
pub const TV_LIBRARY_STALE: &str = "tv_library_stale";
pub const TV_FILE_OPEN_FAILED: &str = "tv_file_open_failed";
pub const TV_FILE_OPEN_NOT_FILE: &str = "tv_file_open_not_file";
pub const TV_FILE_OPEN_NOT_FOUND: &str = "tv_file_open_not_found";
pub const TV_FILE_OPEN_OUTSIDE_FOLDER: &str = "tv_file_open_outside_folder";
pub const TV_FILE_OPEN_STALE: &str = "tv_file_open_stale";
pub const TV_FILE_OPEN_UNAVAILABLE: &str = "tv_file_open_unavailable";
pub const TV_FILE_OPEN_UNSUPPORTED: &str = "tv_file_open_unsupported";
pub const TV_FILE_REVEAL_FAILED: &str = "tv_file_reveal_failed";
pub const TV_FILE_REVEAL_NOT_FILE: &str = "tv_file_reveal_not_file";
pub const TV_FILE_REVEAL_NOT_FOUND: &str = "tv_file_reveal_not_found";
pub const TV_FILE_REVEAL_OUTSIDE_FOLDER: &str = "tv_file_reveal_outside_folder";
pub const TV_FILE_REVEAL_STALE: &str = "tv_file_reveal_stale";
pub const TV_FILE_REVEAL_UNAVAILABLE: &str = "tv_file_reveal_unavailable";
pub const TV_FILE_REVEAL_UNSUPPORTED: &str = "tv_file_reveal_unsupported";
pub const TV_FILE_TRASH_FAILED: &str = "tv_file_trash_failed";
pub const TV_FILE_TRASH_NOT_FILE: &str = "tv_file_trash_not_file";
pub const TV_FILE_TRASH_NOT_FOUND: &str = "tv_file_trash_not_found";
pub const TV_FILE_TRASH_OWNED: &str = "tv_file_trash_owned";
pub const TV_FILE_TRASH_OWNERSHIP_UNAVAILABLE: &str = "tv_file_trash_ownership_unavailable";
pub const TV_FILE_TRASH_OUTSIDE_FOLDER: &str = "tv_file_trash_outside_folder";
pub const TV_FILE_TRASH_STALE: &str = "tv_file_trash_stale";
pub const TV_FILE_TRASH_UNAVAILABLE: &str = "tv_file_trash_unavailable";
pub const TV_FILE_TRASH_UNSUPPORTED: &str = "tv_file_trash_unsupported";

const MAXIMUM_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TvLibraryIdentity {
    pub(crate) show_title: String,
    pub(crate) season: u64,
    pub(crate) episode: u64,
}

#[derive(Clone)]
struct TrustedTvFile {
    path: PathBuf,
    size: u64,
    modified: SystemTime,
}

struct CompletedTvScan {
    folder: PathBuf,
    generation: u64,
    files: Vec<TrustedTvFile>,
}

#[derive(Default)]
struct TvLibraryContext {
    folder: Option<PathBuf>,
    generation: u64,
    completed_scan: Option<CompletedTvScan>,
}

#[derive(Clone, Default)]
pub struct TvLibraryState(Arc<Mutex<TvLibraryContext>>);

#[derive(Clone, Copy)]
enum TvFileValidationError {
    NotFound,
    Unavailable,
    NotFile,
    Unsupported,
    OutsideFolder,
    Stale,
    Dispatch,
}

fn save_tv_folder(path: &Path, folder: &Path) -> Result<(), &'static str> {
    let parent = path.parent().ok_or(TV_FOLDER_STORAGE_FAILED)?;
    fs::create_dir_all(parent).map_err(|_| TV_FOLDER_STORAGE_FAILED)?;
    let folder = folder.to_str().ok_or(TV_FOLDER_STORAGE_FAILED)?;

    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|_| TV_FOLDER_STORAGE_FAILED)?;
    file.write_all(folder.as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|_| TV_FOLDER_STORAGE_FAILED)
}

pub fn load_tv_folder_with(
    state: &TvLibraryState,
    persistence_path: &Path,
) -> Result<Vec<String>, &'static str> {
    let stored_folder = match fs::read_to_string(persistence_path) {
        Ok(value) if !value.is_empty() => PathBuf::from(value),
        Ok(_) => return Err(TV_FOLDER_STORAGE_FAILED),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let mut context = state.0.lock().map_err(|_| TV_FOLDER_STORAGE_FAILED)?;
            context.folder = None;
            context.generation = context.generation.wrapping_add(1);
            context.completed_scan = None;
            return Ok(vec!["unconfigured".to_owned()]);
        }
        Err(_) => return Err(TV_FOLDER_STORAGE_FAILED),
    };
    let status = fs::canonicalize(&stored_folder)
        .ok()
        .filter(|canonical| canonical == &stored_folder)
        .and_then(|canonical| fs::metadata(canonical).ok())
        .filter(|metadata| metadata.is_dir())
        .map_or("unavailable", |_| "ready");
    let response_path = stored_folder
        .to_str()
        .map(str::to_owned)
        .ok_or(TV_FOLDER_STORAGE_FAILED)?;

    let mut context = state.0.lock().map_err(|_| TV_FOLDER_STORAGE_FAILED)?;
    context.folder = Some(stored_folder);
    context.generation = context.generation.wrapping_add(1);
    context.completed_scan = None;
    Ok(vec![status.to_owned(), response_path])
}

pub fn set_tv_folder(
    state: &TvLibraryState,
    persistence_path: &Path,
    selected_folder: PathBuf,
) -> Result<String, &'static str> {
    let folder = fs::canonicalize(selected_folder).map_err(|_| TV_FOLDER_UNAVAILABLE)?;
    if !fs::metadata(&folder)
        .map_err(|_| TV_FOLDER_UNAVAILABLE)?
        .is_dir()
    {
        return Err(TV_FOLDER_UNAVAILABLE);
    }
    save_tv_folder(persistence_path, &folder)?;
    let response = folder
        .to_str()
        .map(str::to_owned)
        .ok_or(TV_FOLDER_UNAVAILABLE)?;

    let mut context = state.0.lock().map_err(|_| TV_FOLDER_STORAGE_FAILED)?;
    context.folder = Some(folder);
    context.generation = context.generation.wrapping_add(1);
    context.completed_scan = None;
    Ok(response)
}

pub fn clear_tv_folder(
    state: &TvLibraryState,
    persistence_path: &Path,
) -> Result<(), &'static str> {
    match fs::remove_file(persistence_path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => return Err(TV_FOLDER_STORAGE_FAILED),
    }
    let mut context = state.0.lock().map_err(|_| TV_FOLDER_STORAGE_FAILED)?;
    context.folder = None;
    context.generation = context.generation.wrapping_add(1);
    context.completed_scan = None;
    Ok(())
}

pub fn configured_tv_folder(state: &TvLibraryState) -> Result<Option<PathBuf>, &'static str> {
    state
        .0
        .lock()
        .map(|context| context.folder.clone())
        .map_err(|_| TV_FOLDER_STORAGE_FAILED)
}

fn is_supported_media(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("mp4") || extension.eq_ignore_ascii_case("mkv")
        })
}

fn episode_component_end(bytes: &[u8], start: usize) -> Option<usize> {
    match bytes.get(start).copied()? {
        b'0' => {
            let end = start.checked_add(2)?;
            matches!(bytes.get(start + 1), Some(b'1'..=b'9'))
                .then_some(end)
                .filter(|end| !bytes.get(*end).is_some_and(u8::is_ascii_digit))
        }
        b'1'..=b'9' => {
            let mut end = start + 1;
            while bytes.get(end).is_some_and(u8::is_ascii_digit) {
                end += 1;
            }
            Some(end)
        }
        _ => None,
    }
}

#[derive(Clone, Copy)]
struct EpisodeMarker {
    start: usize,
    end: usize,
    season: u64,
    episode: u64,
}

fn episode_component(value: &str, start: usize, end: usize) -> Option<u64> {
    value
        .get(start..end)?
        .parse::<u64>()
        .ok()
        .filter(|number| *number <= MAXIMUM_SAFE_INTEGER)
}

fn episode_markers(value: &str) -> Vec<EpisodeMarker> {
    let bytes = value.as_bytes();
    let mut markers = Vec::new();
    for start in 0..bytes.len() {
        if start > 0 && bytes[start - 1].is_ascii_alphanumeric() {
            continue;
        }
        let parsed = if matches!(bytes.get(start), Some(b'S' | b's')) {
            let season_start = start + 1;
            episode_component_end(bytes, season_start).and_then(|season_end| {
                if !matches!(bytes.get(season_end), Some(b'E' | b'e')) {
                    return None;
                }
                let episode_start = season_end + 1;
                episode_component_end(bytes, episode_start)
                    .map(|end| (season_start, season_end, episode_start, end))
            })
        } else {
            episode_component_end(bytes, start).and_then(|season_end| {
                if !matches!(bytes.get(season_end), Some(b'X' | b'x')) {
                    return None;
                }
                let episode_start = season_end + 1;
                episode_component_end(bytes, episode_start)
                    .map(|end| (start, season_end, episode_start, end))
            })
        };
        let Some((season_start, season_end, episode_start, end)) = parsed else {
            continue;
        };
        if end < bytes.len() && bytes[end].is_ascii_alphanumeric() {
            continue;
        }
        let Some(season) = episode_component(value, season_start, season_end) else {
            continue;
        };
        let Some(episode) = episode_component(value, episode_start, end) else {
            continue;
        };
        markers.push(EpisodeMarker {
            start,
            end,
            season,
            episode,
        });
    }
    markers
}

fn continuation_separator(character: char) -> bool {
    character.is_whitespace() || !character.is_alphanumeric()
}

fn compact_episode_continuation(value: &str) -> bool {
    let mut characters = value.char_indices().peekable();
    if !characters
        .peek()
        .is_some_and(|(_, character)| continuation_separator(*character))
    {
        return false;
    }
    while characters
        .peek()
        .is_some_and(|(_, character)| continuation_separator(*character))
    {
        characters.next();
    }
    let mut number_start = characters
        .peek()
        .map(|(index, _)| *index)
        .unwrap_or(value.len());
    if characters
        .peek()
        .is_some_and(|(_, character)| matches!(character, 'E' | 'e' | 'X' | 'x'))
    {
        let (_, marker) = characters.next().expect("peeked marker must exist");
        number_start = characters
            .peek()
            .map(|(index, _)| *index)
            .unwrap_or(value.len());
        if matches!(marker, 'X' | 'x') {
            let suffix = &value[number_start..];
            if ["264", "265", "266"].iter().any(|codec| {
                suffix.strip_prefix(codec).is_some_and(|remaining| {
                    remaining.is_empty()
                        || remaining.chars().next().is_some_and(continuation_separator)
                })
            }) {
                return false;
            }
        }
        while characters
            .peek()
            .is_some_and(|(_, character)| continuation_separator(*character))
        {
            characters.next();
        }
        number_start = characters
            .peek()
            .map(|(index, _)| *index)
            .unwrap_or(value.len());
    }
    let mut number_end = number_start;
    while let Some((index, character)) = characters.peek().copied() {
        if !character.is_ascii_digit() {
            break;
        }
        number_end = index + character.len_utf8();
        characters.next();
    }
    number_end > number_start
        && characters
            .peek()
            .is_none_or(|(_, character)| !character.is_ascii_alphanumeric())
}

fn usable_show_title(value: &str) -> bool {
    let season_only = value.get(..6).is_some_and(|prefix| {
        prefix.eq_ignore_ascii_case("season") && {
            let suffix = value[6..].trim_start_matches(|character: char| {
                character.is_whitespace() || matches!(character, '.' | '_' | '-')
            });
            !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
        }
    });
    let numbered_season = value.get(..1).is_some_and(|prefix| {
        prefix.eq_ignore_ascii_case("s")
            && !value[1..].is_empty()
            && value[1..].bytes().all(|byte| byte.is_ascii_digit())
    });
    value.chars().any(char::is_alphanumeric)
        && !season_only
        && !numbered_season
        && episode_markers(value).is_empty()
}

pub(crate) fn parse_tv_relative_identity(relative_path: &str) -> Option<TvLibraryIdentity> {
    let components = relative_path.split(['/', '\\']).collect::<Vec<_>>();
    if relative_path.is_empty()
        || relative_path.starts_with(['/', '\\'])
        || components
            .iter()
            .any(|component| component.is_empty() || matches!(*component, "." | ".."))
    {
        return None;
    }
    let filename = *components.last()?;
    let stem = filename
        .rfind('.')
        .filter(|index| *index > 0)
        .map_or(filename, |index| &filename[..index]);
    let markers = episode_markers(stem);
    let [marker] = markers.as_slice() else {
        return None;
    };
    if compact_episode_continuation(&stem[marker.end..]) {
        return None;
    }

    let filename_title = stem[..marker.start].trim_matches(|character: char| {
        character.is_whitespace() || matches!(character, '.' | '_' | '-')
    });
    let show_title = if !filename_title.is_empty() && usable_show_title(filename_title) {
        filename_title
    } else {
        let parent = components.get(components.len().checked_sub(2)?)?;
        if usable_show_title(parent) {
            parent
        } else {
            let grandparent = components.get(components.len().checked_sub(3)?)?;
            let season_directory = format!("Season {:02}", marker.season);
            if *parent != season_directory || !usable_show_title(grandparent) {
                return None;
            }
            grandparent
        }
    };
    Some(TvLibraryIdentity {
        show_title: show_title.to_owned(),
        season: marker.season,
        episode: marker.episode,
    })
}

fn collect_media_files(directory: &Path, files: &mut Vec<TrustedTvFile>) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();

        if file_type.is_dir() {
            collect_media_files(&path, files)?;
        } else if file_type.is_file() && is_supported_media(&path) {
            let metadata = entry.metadata()?;
            files.push(TrustedTvFile {
                path,
                size: metadata.len(),
                modified: metadata.modified()?,
            });
        }
    }
    Ok(())
}

fn scan_media_files(folder: &Path) -> Result<Vec<TrustedTvFile>, &'static str> {
    let canonical_folder = fs::canonicalize(folder).map_err(|_| TV_FOLDER_UNAVAILABLE)?;
    if canonical_folder != folder
        || !fs::metadata(&canonical_folder)
            .map_err(|_| TV_FOLDER_UNAVAILABLE)?
            .is_dir()
    {
        return Err(TV_FOLDER_UNAVAILABLE);
    }
    let mut files = Vec::new();
    collect_media_files(folder, &mut files).map_err(|_| TV_LIBRARY_SCAN_FAILED)?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

pub fn scan_tv_library_with(state: &TvLibraryState) -> Result<Vec<String>, &'static str> {
    let (folder, generation) = {
        let mut context = state.0.lock().map_err(|_| TV_LIBRARY_SCAN_FAILED)?;
        let folder = context.folder.clone().ok_or(TV_FOLDER_UNAVAILABLE)?;
        context.generation = context.generation.wrapping_add(1);
        context.completed_scan = None;
        (folder, context.generation)
    };
    let files = scan_media_files(&folder)?;

    let mut response = Vec::with_capacity(1 + files.len() * 6);
    response.push(generation.to_string());
    for file in &files {
        response.push(
            file.path
                .to_str()
                .map(str::to_owned)
                .ok_or(TV_LIBRARY_SCAN_FAILED)?,
        );
        let relative_path = file
            .path
            .strip_prefix(&folder)
            .ok()
            .and_then(Path::to_str)
            .map(str::to_owned)
            .filter(|path| !path.is_empty())
            .ok_or(TV_LIBRARY_SCAN_FAILED)?;
        let identity = parse_tv_relative_identity(&relative_path);
        response.push(relative_path);
        response.push(file.size.to_string());
        if let Some(identity) = identity {
            response.extend([
                identity.show_title,
                identity.season.to_string(),
                identity.episode.to_string(),
            ]);
        } else {
            response.extend([String::new(), String::new(), String::new()]);
        }
    }

    let mut context = state.0.lock().map_err(|_| TV_LIBRARY_SCAN_FAILED)?;
    if context.generation != generation || context.folder.as_ref() != Some(&folder) {
        return Err(TV_LIBRARY_STALE);
    }
    context.completed_scan = Some(CompletedTvScan {
        folder,
        generation,
        files,
    });
    Ok(response)
}

fn metadata_error(error: &io::Error) -> TvFileValidationError {
    if error.kind() == io::ErrorKind::NotFound {
        TvFileValidationError::NotFound
    } else {
        TvFileValidationError::Unavailable
    }
}

fn validate_tv_file(
    requested_path: &Path,
    configured_folder: &Path,
    scan: &CompletedTvScan,
    requested_generation: Option<u64>,
) -> Result<(), TvFileValidationError> {
    if scan.folder != configured_folder
        || requested_generation.is_some_and(|generation| generation != scan.generation)
    {
        return Err(TvFileValidationError::Stale);
    }
    let relative_path = requested_path
        .strip_prefix(configured_folder)
        .map_err(|_| TvFileValidationError::OutsideFolder)?;
    let mut checked_path = configured_folder.to_path_buf();
    for component in relative_path.components() {
        let Component::Normal(component) = component else {
            return Err(TvFileValidationError::OutsideFolder);
        };
        checked_path.push(component);
        let metadata =
            fs::symlink_metadata(&checked_path).map_err(|error| metadata_error(&error))?;
        if metadata.file_type().is_symlink() {
            return Err(TvFileValidationError::NotFile);
        }
    }
    let metadata = fs::metadata(requested_path).map_err(|error| metadata_error(&error))?;
    if !metadata.is_file() {
        return Err(TvFileValidationError::NotFile);
    }
    if !is_supported_media(requested_path) {
        return Err(TvFileValidationError::Unsupported);
    }
    let canonical_path =
        fs::canonicalize(requested_path).map_err(|error| metadata_error(&error))?;
    if canonical_path != requested_path || !canonical_path.starts_with(configured_folder) {
        return Err(TvFileValidationError::OutsideFolder);
    }
    let trusted_file = scan
        .files
        .iter()
        .find(|file| file.path == requested_path)
        .ok_or(TvFileValidationError::Stale)?;
    if trusted_file.size != metadata.len()
        || metadata
            .modified()
            .map_err(|error| metadata_error(&error))?
            != trusted_file.modified
    {
        return Err(TvFileValidationError::Stale);
    }
    Ok(())
}

fn run_tv_file_action(
    path: &Path,
    state: &TvLibraryState,
    dispatch: impl FnOnce(&Path) -> Result<(), ()>,
) -> Result<(), TvFileValidationError> {
    let context = state
        .0
        .lock()
        .map_err(|_| TvFileValidationError::Unavailable)?;
    let configured_folder = context
        .folder
        .as_deref()
        .ok_or(TvFileValidationError::Unavailable)?;
    let canonical_folder =
        fs::canonicalize(configured_folder).map_err(|_| TvFileValidationError::Unavailable)?;
    if canonical_folder != configured_folder
        || !fs::metadata(&canonical_folder)
            .map_err(|_| TvFileValidationError::Unavailable)?
            .is_dir()
    {
        return Err(TvFileValidationError::Unavailable);
    }
    let scan = context
        .completed_scan
        .as_ref()
        .ok_or(TvFileValidationError::Stale)?;
    validate_tv_file(path, &canonical_folder, scan, None)?;
    dispatch(path).map_err(|_| TvFileValidationError::Dispatch)
}

pub fn open_tv_file_with(
    path: &Path,
    state: &TvLibraryState,
    dispatch: impl FnOnce(&Path) -> Result<(), ()>,
) -> Result<(), &'static str> {
    run_tv_file_action(path, state, dispatch).map_err(|error| match error {
        TvFileValidationError::NotFound => TV_FILE_OPEN_NOT_FOUND,
        TvFileValidationError::Unavailable => TV_FILE_OPEN_UNAVAILABLE,
        TvFileValidationError::NotFile => TV_FILE_OPEN_NOT_FILE,
        TvFileValidationError::Unsupported => TV_FILE_OPEN_UNSUPPORTED,
        TvFileValidationError::OutsideFolder => TV_FILE_OPEN_OUTSIDE_FOLDER,
        TvFileValidationError::Stale => TV_FILE_OPEN_STALE,
        TvFileValidationError::Dispatch => TV_FILE_OPEN_FAILED,
    })
}

pub fn reveal_tv_file_with(
    path: &Path,
    state: &TvLibraryState,
    dispatch: impl FnOnce(&Path) -> Result<(), ()>,
) -> Result<(), &'static str> {
    run_tv_file_action(path, state, dispatch).map_err(|error| match error {
        TvFileValidationError::NotFound => TV_FILE_REVEAL_NOT_FOUND,
        TvFileValidationError::Unavailable => TV_FILE_REVEAL_UNAVAILABLE,
        TvFileValidationError::NotFile => TV_FILE_REVEAL_NOT_FILE,
        TvFileValidationError::Unsupported => TV_FILE_REVEAL_UNSUPPORTED,
        TvFileValidationError::OutsideFolder => TV_FILE_REVEAL_OUTSIDE_FOLDER,
        TvFileValidationError::Stale => TV_FILE_REVEAL_STALE,
        TvFileValidationError::Dispatch => TV_FILE_REVEAL_FAILED,
    })
}

fn trash_trusted_tv_file_with(
    path: &Path,
    scan_generation: u64,
    state: &TvLibraryState,
    dispatch: impl FnOnce(&Path) -> Result<(), ()>,
) -> Result<(), &'static str> {
    let mut context = state.0.lock().map_err(|_| TV_FILE_TRASH_UNAVAILABLE)?;
    let configured_folder = context.folder.clone().ok_or(TV_FILE_TRASH_UNAVAILABLE)?;
    let canonical_folder =
        fs::canonicalize(&configured_folder).map_err(|_| TV_FILE_TRASH_UNAVAILABLE)?;
    if canonical_folder != configured_folder
        || !fs::metadata(&canonical_folder)
            .map_err(|_| TV_FILE_TRASH_UNAVAILABLE)?
            .is_dir()
    {
        return Err(TV_FILE_TRASH_UNAVAILABLE);
    }
    let scan = context.completed_scan.as_ref().ok_or(TV_FILE_TRASH_STALE)?;
    validate_tv_file(path, &canonical_folder, scan, Some(scan_generation)).map_err(|error| {
        match error {
            TvFileValidationError::NotFound => TV_FILE_TRASH_NOT_FOUND,
            TvFileValidationError::Unavailable => TV_FILE_TRASH_UNAVAILABLE,
            TvFileValidationError::NotFile => TV_FILE_TRASH_NOT_FILE,
            TvFileValidationError::Unsupported => TV_FILE_TRASH_UNSUPPORTED,
            TvFileValidationError::OutsideFolder => TV_FILE_TRASH_OUTSIDE_FOLDER,
            TvFileValidationError::Stale => TV_FILE_TRASH_STALE,
            TvFileValidationError::Dispatch => TV_FILE_TRASH_FAILED,
        }
    })?;

    dispatch(path).map_err(|_| TV_FILE_TRASH_FAILED)?;
    context
        .completed_scan
        .as_mut()
        .ok_or(TV_FILE_TRASH_STALE)?
        .files
        .retain(|file| file.path != path);
    Ok(())
}

pub fn trash_tv_file_with_download_ownership(
    path: &Path,
    scan_generation: u64,
    download_state: &VrDownloadState,
    library_state: &TvLibraryState,
    dispatch: impl FnOnce(&Path) -> Result<(), ()>,
) -> Result<(), &'static str> {
    with_unowned_tv_library_path(download_state, path, |configured_download_folder| {
        let configured_download_folder =
            configured_download_folder.ok_or(TV_FILE_TRASH_UNAVAILABLE)?;
        let configured_library_folder = configured_tv_folder(library_state)
            .map_err(|_| TV_FILE_TRASH_UNAVAILABLE)?
            .ok_or(TV_FILE_TRASH_UNAVAILABLE)?;
        if configured_download_folder != configured_library_folder {
            return Err(TV_FILE_TRASH_UNAVAILABLE);
        }
        trash_trusted_tv_file_with(path, scan_generation, library_state, dispatch)
    })
    .map_err(|error| match error {
        VrLibraryTrashOwnershipError::Owned => TV_FILE_TRASH_OWNED,
        VrLibraryTrashOwnershipError::Unavailable => TV_FILE_TRASH_OWNERSHIP_UNAVAILABLE,
    })?
}

#[cfg(test)]
fn trash_tv_file_with(
    path: &Path,
    scan_generation: u64,
    state: &TvLibraryState,
    dispatch: impl FnOnce(&Path) -> Result<(), ()>,
) -> Result<(), &'static str> {
    trash_trusted_tv_file_with(path, scan_generation, state, dispatch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::Cell,
        sync::atomic::{AtomicU64, Ordering},
    };

    static FIXTURE_ID: AtomicU64 = AtomicU64::new(1);

    struct Fixture {
        path: PathBuf,
    }

    impl Fixture {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "auto-video-tv-library-{label}-{}-{}",
                std::process::id(),
                FIXTURE_ID.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).expect("fixture folder must be created");
            Self {
                path: fs::canonicalize(path).expect("fixture folder must be canonical"),
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn folder_configuration_persists_unavailable_state_recovers_and_clears() {
        let fixture = Fixture::new("folder");
        let folder = fixture.path.join("TV shows");
        let persistence_path = fixture.path.join("config");
        fs::create_dir(&folder).expect("TV folder must be created");
        let state = TvLibraryState::default();

        assert_eq!(
            set_tv_folder(&state, &persistence_path, folder.clone()),
            Ok(folder.to_string_lossy().into_owned())
        );
        assert_eq!(
            load_tv_folder_with(&TvLibraryState::default(), &persistence_path),
            Ok(vec![
                "ready".to_owned(),
                folder.to_string_lossy().into_owned()
            ])
        );
        fs::remove_dir(&folder).expect("TV folder must be removed");
        assert_eq!(
            load_tv_folder_with(&state, &persistence_path),
            Ok(vec![
                "unavailable".to_owned(),
                folder.to_string_lossy().into_owned()
            ])
        );
        fs::create_dir(&folder).expect("TV folder must be restored");
        assert_eq!(
            load_tv_folder_with(&state, &persistence_path),
            Ok(vec![
                "ready".to_owned(),
                folder.to_string_lossy().into_owned()
            ])
        );
        clear_tv_folder(&state, &persistence_path).expect("configuration must clear");
        assert_eq!(
            load_tv_folder_with(&state, &persistence_path),
            Ok(vec!["unconfigured".to_owned()])
        );
    }

    #[test]
    fn scan_preserves_exact_paths_relative_paths_sizes_and_order() {
        let fixture = Fixture::new("scan");
        let nested = fixture.path.join("番組  Name");
        fs::create_dir(&nested).expect("nested folder must be created");
        let first = fixture.path.join("A  Show.S01E02.mp4");
        let second = nested.join("S1E3.MKV");
        fs::write(&second, b"second").expect("second file must be written");
        fs::write(&first, b"one").expect("first file must be written");
        fs::write(fixture.path.join("ignored.txt"), b"ignored")
            .expect("ignored file must be written");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&first, fixture.path.join("ignored-link.mp4"))
            .expect("fixture symlink must be created");
        let state = TvLibraryState::default();
        set_tv_folder(&state, &fixture.path.join("config"), fixture.path.clone())
            .expect("TV folder must be configured");

        assert_eq!(
            scan_tv_library_with(&state),
            Ok(vec![
                "2".to_owned(),
                first.to_string_lossy().into_owned(),
                "A  Show.S01E02.mp4".to_owned(),
                "3".to_owned(),
                "A  Show".to_owned(),
                "1".to_owned(),
                "2".to_owned(),
                second.to_string_lossy().into_owned(),
                Path::new("番組  Name")
                    .join("S1E3.MKV")
                    .to_string_lossy()
                    .into_owned(),
                "6".to_owned(),
                "番組  Name".to_owned(),
                "1".to_owned(),
                "3".to_owned(),
            ])
        );
    }

    #[test]
    fn conservative_identity_parser_accepts_exact_single_episode_paths() {
        for (path, show_title, season, episode) in [
            (
                "Exact  Show — 特別版/Season 02/Exact  Show — 特別版 - S02E03 - Episode 4.MP4",
                "Exact  Show — 特別版",
                2,
                3,
            ),
            (
                "Exact Show/Season 02/Exact Show - S02E03 - Part 2.mkv",
                "Exact Show",
                2,
                3,
            ),
            (
                "Exact Big Show/Season 123/Exact Big Show - S123E456 - Exact Episode.MKV",
                "Exact Big Show",
                123,
                456,
            ),
            ("Show.S123E456+720p.mp4", "Show", 123, 456),
            ("Show.123x456.x265.mkv", "Show", 123, 456),
        ] {
            assert_eq!(
                parse_tv_relative_identity(path),
                Some(TvLibraryIdentity {
                    show_title: show_title.to_owned(),
                    season,
                    episode,
                }),
                "single episode path {path:?} did not retain its exact identity"
            );
        }
    }

    #[test]
    fn conservative_identity_parser_rejects_non_round_trippable_exact_names_and_markers() {
        for path in [
            "Exact Show/Season 02/Exact Show - S02E03 - 2.mp4",
            "Exact Show/Season 02/Exact Show - S02E03 - 04.mp4",
            "Exact Show/Season 02/Exact Show - S02E03 - E04.mp4",
            "Exact Show/Season 02/Exact Show - S02E03 - x04.mp4",
            "Exact Show/Season 02/Exact Show - S02E03 - #4.mp4",
            "Exact S01E02 Show/Season 02/Exact S01E02 Show - S02E03 - Episode.mp4",
            "Exact Show/Season 02/Exact Show - S02E03 - Flashback 1x02.mp4",
            "S123/S123E456.mkv",
            "Wrong Parent/Season 124/S123E456.mp4",
            "Show.S01E02.S01E03.mp4",
            "Show.S123E456-x457.mp4",
            "Show.123x456/x457.mp4",
        ] {
            assert_eq!(
                parse_tv_relative_identity(path),
                None,
                "ambiguous TV path {path:?} was associated"
            );
        }
    }

    #[test]
    fn file_action_rejects_unrelated_changed_and_unscanned_files_without_dispatch() {
        let trusted = Fixture::new("trusted");
        let unrelated = Fixture::new("unrelated");
        let trusted_file = trusted.path.join("Show.S01E02.mp4");
        let unrelated_file = unrelated.path.join("Show.S01E02.mp4");
        let unscanned_file = trusted.path.join("Show.S01E03.mkv");
        fs::write(&trusted_file, b"trusted").expect("trusted file must be written");
        fs::write(&unrelated_file, b"unrelated").expect("unrelated file must be written");
        let state = TvLibraryState::default();
        set_tv_folder(&state, &trusted.path.join("config"), trusted.path.clone())
            .expect("TV folder must be configured");
        scan_tv_library_with(&state).expect("scan must complete");
        let dispatched = Cell::new(false);

        assert_eq!(
            open_tv_file_with(&unrelated_file, &state, |_| {
                dispatched.set(true);
                Ok(())
            }),
            Err(TV_FILE_OPEN_OUTSIDE_FOLDER)
        );
        fs::write(&unscanned_file, b"new").expect("unscanned file must be written");
        assert_eq!(
            open_tv_file_with(&unscanned_file, &state, |_| {
                dispatched.set(true);
                Ok(())
            }),
            Err(TV_FILE_OPEN_STALE)
        );
        fs::write(&trusted_file, b"changed content").expect("trusted file must change");
        assert_eq!(
            reveal_tv_file_with(&trusted_file, &state, |_| {
                dispatched.set(true);
                Ok(())
            }),
            Err(TV_FILE_REVEAL_STALE)
        );
        assert!(!dispatched.get());
    }

    #[test]
    fn file_action_rejects_missing_directory_unsupported_and_symlink_paths() {
        let fixture = Fixture::new("invalid");
        let movie = fixture.path.join("Show.S01E02.mp4");
        fs::write(&movie, b"movie").expect("movie must be written");
        let state = TvLibraryState::default();
        set_tv_folder(&state, &fixture.path.join("config"), fixture.path.clone())
            .expect("TV folder must be configured");
        scan_tv_library_with(&state).expect("scan must complete");
        let directory = fixture.path.join("directory.mkv");
        let unsupported = fixture.path.join("unsupported.avi");
        fs::create_dir(&directory).expect("directory must be created");
        fs::write(&unsupported, b"unsupported").expect("unsupported file must be written");
        let dispatched = Cell::new(false);

        for (path, error) in [
            (fixture.path.join("missing.mp4"), TV_FILE_OPEN_NOT_FOUND),
            (directory, TV_FILE_OPEN_NOT_FILE),
            (unsupported, TV_FILE_OPEN_UNSUPPORTED),
        ] {
            assert_eq!(
                open_tv_file_with(&path, &state, |_| {
                    dispatched.set(true);
                    Ok(())
                }),
                Err(error)
            );
        }
        #[cfg(unix)]
        {
            let link = fixture.path.join("link.mp4");
            std::os::unix::fs::symlink(&movie, &link).expect("symlink must be created");
            assert_eq!(
                open_tv_file_with(&link, &state, |_| {
                    dispatched.set(true);
                    Ok(())
                }),
                Err(TV_FILE_OPEN_NOT_FILE)
            );
        }
        assert!(!dispatched.get());
    }

    #[test]
    fn file_actions_dispatch_only_the_exact_trusted_file_and_report_failures() {
        let fixture = Fixture::new("dispatch");
        let movie = fixture.path.join("Show.S01E02.mp4");
        fs::write(&movie, b"movie").expect("movie must be written");
        let state = TvLibraryState::default();
        set_tv_folder(&state, &fixture.path.join("config"), fixture.path.clone())
            .expect("TV folder must be configured");
        scan_tv_library_with(&state).expect("scan must complete");
        let opened = Cell::new(false);

        assert_eq!(
            open_tv_file_with(&movie, &state, |path| {
                assert_eq!(path, movie);
                opened.set(true);
                Ok(())
            }),
            Ok(())
        );
        assert!(opened.get());
        let revealed = Cell::new(false);
        assert_eq!(
            reveal_tv_file_with(&movie, &state, |path| {
                assert_eq!(path, movie);
                revealed.set(true);
                Ok(())
            }),
            Ok(())
        );
        assert!(revealed.get());
        assert_eq!(
            open_tv_file_with(&movie, &state, |_| Err(())),
            Err(TV_FILE_OPEN_FAILED)
        );
        assert_eq!(
            reveal_tv_file_with(&movie, &state, |_| Err(())),
            Err(TV_FILE_REVEAL_FAILED)
        );
    }

    #[test]
    fn trash_dispatches_one_exact_scanned_file_and_updates_state_only_after_success() {
        let fixture = Fixture::new("trash-exact");
        let first = fixture.path.join("Show.S01E01.mp4");
        let sibling = fixture.path.join("Show.S01E02.mkv");
        let unassociated = fixture.path.join("Special feature.mp4");
        fs::write(&first, b"first").expect("first episode must be written");
        fs::write(&sibling, b"sibling").expect("sibling episode must be written");
        fs::write(&unassociated, b"special").expect("special must be written");
        let state = TvLibraryState::default();
        set_tv_folder(&state, &fixture.path.join("config"), fixture.path.clone())
            .expect("TV folder must be configured");
        let scan = scan_tv_library_with(&state).expect("scan must complete");
        let generation = scan[0].parse().expect("generation must be valid");
        let dispatch_count = Cell::new(0);

        assert_eq!(
            trash_tv_file_with(&first, generation, &state, |_| {
                dispatch_count.set(dispatch_count.get() + 1);
                Err(())
            }),
            Err(TV_FILE_TRASH_FAILED)
        );
        assert_eq!(
            trash_tv_file_with(&first, generation, &state, |path| {
                assert_eq!(path, first);
                dispatch_count.set(dispatch_count.get() + 1);
                Ok(())
            }),
            Ok(())
        );
        assert_eq!(
            trash_tv_file_with(&first, generation, &state, |_| {
                dispatch_count.set(dispatch_count.get() + 1);
                Ok(())
            }),
            Err(TV_FILE_TRASH_STALE)
        );
        assert_eq!(
            trash_tv_file_with(&sibling, generation, &state, |_| Ok(())),
            Ok(())
        );
        assert_eq!(
            trash_tv_file_with(&unassociated, generation, &state, |_| Ok(())),
            Ok(())
        );
        assert_eq!(dispatch_count.get(), 2);
    }

    #[test]
    fn trash_rejects_untrusted_changed_and_unsafe_paths_without_dispatch() {
        let trusted = Fixture::new("trash-trusted");
        let unrelated = Fixture::new("trash-unrelated");
        let current = trusted.path.join("Show.S01E01.mp4");
        let changed = trusted.path.join("Show.S01E02.mkv");
        let missing = trusted.path.join("Show.S01E03.mp4");
        fs::write(&current, b"current").expect("current episode must be written");
        fs::write(&changed, b"changed").expect("changed episode must be written");
        fs::write(&missing, b"missing").expect("missing episode must be written");
        let state = TvLibraryState::default();
        set_tv_folder(&state, &trusted.path.join("config"), trusted.path.clone())
            .expect("TV folder must be configured");
        let scan = scan_tv_library_with(&state).expect("scan must complete");
        let generation = scan[0].parse().expect("generation must be valid");
        let dispatched = Cell::new(false);
        let same_name_elsewhere = unrelated.path.join("Show.S01E01.mp4");
        fs::write(&same_name_elsewhere, b"current").expect("unrelated episode must be written");
        let neighbor = trusted.path.join("Show.S01E04.mp4");
        fs::write(&neighbor, b"neighbor").expect("neighbor must be written");
        let directory = trusted.path.join("directory.mkv");
        fs::create_dir(&directory).expect("directory must be created");
        let unsupported = trusted.path.join("unsupported.avi");
        fs::write(&unsupported, b"unsupported").expect("unsupported file must be written");
        fs::write(&changed, b"different content").expect("episode must change");
        fs::remove_file(&missing).expect("episode must be removed");

        for (path, expected) in [
            (same_name_elsewhere, TV_FILE_TRASH_OUTSIDE_FOLDER),
            (neighbor, TV_FILE_TRASH_STALE),
            (directory, TV_FILE_TRASH_NOT_FILE),
            (unsupported, TV_FILE_TRASH_UNSUPPORTED),
            (changed, TV_FILE_TRASH_STALE),
            (missing, TV_FILE_TRASH_NOT_FOUND),
        ] {
            assert_eq!(
                trash_tv_file_with(&path, generation, &state, |_| {
                    dispatched.set(true);
                    Ok(())
                }),
                Err(expected)
            );
        }

        #[cfg(unix)]
        {
            let link = trusted.path.join("linked.mp4");
            std::os::unix::fs::symlink(&current, &link).expect("file symlink must be created");
            assert_eq!(
                trash_tv_file_with(&link, generation, &state, |_| {
                    dispatched.set(true);
                    Ok(())
                }),
                Err(TV_FILE_TRASH_NOT_FILE)
            );
            let linked_parent = trusted.path.join("linked-parent");
            std::os::unix::fs::symlink(&unrelated.path, &linked_parent)
                .expect("parent symlink must be created");
            let linked_child = linked_parent.join("Show.S01E01.mp4");
            assert_eq!(
                trash_tv_file_with(&linked_child, generation, &state, |_| {
                    dispatched.set(true);
                    Ok(())
                }),
                Err(TV_FILE_TRASH_NOT_FILE)
            );
        }
        assert!(!dispatched.get());
    }

    #[test]
    fn trash_rejects_stale_generations_and_restart_scan_reflects_an_accepted_move() {
        let fixture = Fixture::new("trash-generation");
        let holding = Fixture::new("trash-holding");
        let persistence_path = fixture.path.join("config");
        let movie = fixture.path.join("Show.S01E01.mp4");
        fs::write(&movie, b"episode").expect("episode must be written");
        let state = TvLibraryState::default();
        set_tv_folder(&state, &persistence_path, fixture.path.clone())
            .expect("TV folder must be configured");
        let first_scan = scan_tv_library_with(&state).expect("scan must complete");
        let first_generation = first_scan[0].parse().expect("generation must be valid");
        let second_scan = scan_tv_library_with(&state).expect("scan must complete");
        let current_generation = second_scan[0].parse().expect("generation must be valid");
        let dispatched = Cell::new(false);

        assert_eq!(
            trash_tv_file_with(&movie, first_generation, &state, |_| {
                dispatched.set(true);
                Ok(())
            }),
            Err(TV_FILE_TRASH_STALE)
        );
        assert!(!dispatched.get());

        let moved_path = holding.path.join("Show.S01E01.mp4");
        trash_tv_file_with(&movie, current_generation, &state, |path| {
            fs::rename(path, &moved_path).map_err(|_| ())
        })
        .expect("accepted dispatch must succeed");
        assert!(moved_path.is_file());
        assert!(!movie.exists());

        let restarted = TvLibraryState::default();
        assert_eq!(
            load_tv_folder_with(&restarted, &persistence_path),
            Ok(vec![
                "ready".to_owned(),
                fixture.path.to_string_lossy().into_owned(),
            ])
        );
        let restarted_scan = scan_tv_library_with(&restarted).expect("restart scan must complete");
        assert_eq!(restarted_scan.len(), 1);
    }

    #[test]
    fn folder_replacement_clear_and_failed_refresh_invalidate_trash_requests() {
        let configuration = Fixture::new("trash-configuration");
        let first = Fixture::new("trash-first-folder");
        let replacement = Fixture::new("trash-replacement-folder");
        let persistence_path = configuration.path.join("config");
        let movie = first.path.join("Show.S01E01.mp4");
        fs::write(&movie, b"episode").expect("episode must be written");
        let state = TvLibraryState::default();
        set_tv_folder(&state, &persistence_path, first.path.clone())
            .expect("first TV folder must be configured");
        let scan = scan_tv_library_with(&state).expect("scan must complete");
        let generation = scan[0].parse().expect("generation must be valid");
        let dispatched = Cell::new(false);

        set_tv_folder(&state, &persistence_path, replacement.path.clone())
            .expect("replacement TV folder must be configured");
        assert_eq!(
            trash_tv_file_with(&movie, generation, &state, |_| {
                dispatched.set(true);
                Ok(())
            }),
            Err(TV_FILE_TRASH_STALE)
        );
        clear_tv_folder(&state, &persistence_path).expect("TV folder must clear");
        assert_eq!(
            trash_tv_file_with(&movie, generation, &state, |_| {
                dispatched.set(true);
                Ok(())
            }),
            Err(TV_FILE_TRASH_UNAVAILABLE)
        );

        set_tv_folder(&state, &persistence_path, first.path.clone())
            .expect("first TV folder must be restored");
        let refreshed_scan = scan_tv_library_with(&state).expect("scan must complete");
        let refreshed_generation = refreshed_scan[0].parse().expect("generation must be valid");
        let unavailable_folder = configuration.path.join("unavailable-TV-folder");
        fs::rename(&first.path, &unavailable_folder).expect("TV folder must become unavailable");
        assert_eq!(scan_tv_library_with(&state), Err(TV_FOLDER_UNAVAILABLE));
        assert_eq!(
            trash_tv_file_with(&movie, refreshed_generation, &state, |_| {
                dispatched.set(true);
                Ok(())
            }),
            Err(TV_FILE_TRASH_UNAVAILABLE)
        );
        assert!(!dispatched.get());
    }
}
