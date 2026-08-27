use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    time::SystemTime,
};

#[cfg(unix)]
use std::fs::File;

use crate::library_scan::{is_supported_library_media, scan_library_files};
use crate::vr_download::{
    with_unowned_tv_library_path, VrDownloadState, VrLibraryTrashOwnershipError,
};
use crate::vr_torrent::{
    canonical_imdb_id, hex_sha1, json_array, json_object, json_string, json_u64, JsonParser,
    JsonValue,
};
use crate::{movie_file_fingerprint, movie_path_identity, replace_movie_metadata_file};

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
pub const TV_METADATA_CONTEXT_INVALID: &str = "tv_metadata_context_invalid";
pub const TV_METADATA_MALFORMED: &str = "tv_metadata_malformed_provider";
pub const TV_METADATA_PERSISTENCE_FAILED: &str = "tv_metadata_persistence_failed";
pub const TV_METADATA_STALE: &str = "tv_metadata_stale";
pub const TV_METADATA_UNAVAILABLE: &str = "tv_metadata_unavailable";

const MAXIMUM_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const TV_METADATA_HEADER: &[u8] = b"AUTO_VIDEO_TV_SHOW_METADATA_V1\n";
const TV_METADATA_MAX_BYTES: u64 = 4 * 1024 * 1024;
const TV_METADATA_MAX_RECORDS: usize = 10_000;
const TV_METADATA_MAX_ANCHORS: usize = 10_000;
const TV_METADATA_MAX_QUERY_BYTES: usize = 512;
const TV_METADATA_MAX_CANDIDATES: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TvLibraryIdentity {
    pub(crate) show_title: String,
    pub(crate) season: u64,
    pub(crate) episode: u64,
}

#[derive(Clone)]
struct TrustedTvFile {
    path: PathBuf,
    relative_path: String,
    file_identity: String,
    fingerprint: String,
    size: u64,
    modified: SystemTime,
    identity: Option<TvLibraryIdentity>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TvMetadataAnchor {
    relative_path: String,
    season: u64,
    episode: u64,
    file_identity: String,
    fingerprint: String,
    size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TvMetadataAssociation {
    folder: PathBuf,
    folder_identity: String,
    local_show_title: String,
    anchors: Vec<TvMetadataAnchor>,
    tmdb_tv_id: u64,
    imdb_id: String,
    name: String,
    original_name: Option<String>,
    first_air_date: Option<String>,
    poster_path: Option<String>,
    overview: Option<String>,
    generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TvTmdbCoverAuthority {
    pub(crate) scan_generation: u64,
    pub(crate) group_id: String,
    pub(crate) tmdb_id: u64,
    pub(crate) poster_path: Option<String>,
    pub(crate) association_generation: u64,
}

#[derive(Clone)]
struct TrustedTvShowGroup {
    group_id: String,
    show_title: String,
    member_paths: Vec<PathBuf>,
    association: Option<TvMetadataAssociation>,
    metadata_attention: bool,
}

struct CompletedTvScan {
    folder: PathBuf,
    folder_identity: String,
    generation: u64,
    files: Vec<TrustedTvFile>,
    groups: Vec<TrustedTvShowGroup>,
    metadata_status: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TvMetadataCandidate {
    tmdb_tv_id: u64,
    name: String,
    original_name: Option<String>,
    first_air_date: Option<String>,
    poster_path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TvMetadataAuthority {
    scan_generation: u64,
    folder: PathBuf,
    folder_identity: String,
    group_id: String,
    local_show_title: String,
    anchors: Vec<TvMetadataAnchor>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TvMetadataSearch {
    request_id: String,
    operation_generation: u64,
    authority: TvMetadataAuthority,
    query: String,
    token_identity: String,
    candidates: Vec<TvMetadataCandidate>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TvMetadataVerification {
    verification_id: String,
    operation_generation: u64,
    matching_request_id: String,
    authority: TvMetadataAuthority,
    association: TvMetadataAssociation,
    token_identity: String,
}

#[derive(Default)]
struct TvLibraryContext {
    folder: Option<PathBuf>,
    generation: u64,
    completed_scan: Option<CompletedTvScan>,
    metadata_client_generation: u64,
    metadata_operation_generation: u64,
    metadata_search: Option<TvMetadataSearch>,
    metadata_verification: Option<TvMetadataVerification>,
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

fn invalidate_tv_metadata_context(context: &mut TvLibraryContext) {
    context.metadata_operation_generation = context.metadata_operation_generation.wrapping_add(1);
    context.metadata_search = None;
    context.metadata_verification = None;
}

fn begin_tv_metadata_client_operation(
    context: &mut TvLibraryContext,
    client_generation: u64,
) -> Result<(), &'static str> {
    if client_generation == 0 || client_generation <= context.metadata_client_generation {
        return Err(TV_METADATA_CONTEXT_INVALID);
    }
    context.metadata_client_generation = client_generation;
    Ok(())
}

pub fn invalidate_tv_metadata_client_context(
    state: &TvLibraryState,
    client_generation: u64,
) -> Result<(), &'static str> {
    let mut context = state.0.lock().map_err(|_| TV_METADATA_UNAVAILABLE)?;
    if client_generation < context.metadata_client_generation {
        return Ok(());
    }
    context.metadata_client_generation = client_generation;
    invalidate_tv_metadata_context(&mut context);
    Ok(())
}

pub fn invalidate_tv_metadata_context_for_state(
    state: &TvLibraryState,
) -> Result<(), &'static str> {
    let mut context = state.0.lock().map_err(|_| TV_METADATA_UNAVAILABLE)?;
    invalidate_tv_metadata_context(&mut context);
    Ok(())
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
            invalidate_tv_metadata_context(&mut context);
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
    invalidate_tv_metadata_context(&mut context);
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
    invalidate_tv_metadata_context(&mut context);
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
    invalidate_tv_metadata_context(&mut context);
    Ok(())
}

pub fn configured_tv_folder(state: &TvLibraryState) -> Result<Option<PathBuf>, &'static str> {
    state
        .0
        .lock()
        .map(|context| context.folder.clone())
        .map_err(|_| TV_FOLDER_STORAGE_FAILED)
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

fn encode_tv_metadata_text(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(value.len() * 2);
    for byte in value.as_bytes() {
        encoded.push(HEX[usize::from(byte >> 4)] as char);
        encoded.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    encoded
}

fn decode_tv_metadata_text(value: &str) -> Option<String> {
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

fn valid_tv_metadata_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
}

fn valid_tv_metadata_relative_path(value: &str) -> bool {
    let path = Path::new(value);
    !path.is_absolute()
        && path.components().next().is_some()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn validate_tv_metadata_association(
    association: &TvMetadataAssociation,
) -> Result<(), &'static str> {
    if !association.folder.is_absolute()
        || association.folder.to_str().is_none()
        || association.folder_identity.is_empty()
        || association.folder_identity.len() > 256
        || association.local_show_title.trim().is_empty()
        || association.local_show_title.len() > 16 * 1024
        || association.anchors.is_empty()
        || association.anchors.len() > TV_METADATA_MAX_ANCHORS
        || association.tmdb_tv_id == 0
        || canonical_imdb_id(&association.imdb_id).as_deref() != Some(association.imdb_id.as_str())
        || association.name.trim().is_empty()
        || association.name.len() > 16 * 1024
        || association
            .original_name
            .as_ref()
            .is_some_and(|value| value.trim().is_empty() || value.len() > 16 * 1024)
        || association
            .first_air_date
            .as_deref()
            .is_some_and(|value| !valid_tv_metadata_date(value))
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
        return Err(TV_METADATA_PERSISTENCE_FAILED);
    }
    let mut anchor_paths = HashSet::new();
    for anchor in &association.anchors {
        if !valid_tv_metadata_relative_path(&anchor.relative_path)
            || anchor.relative_path.len() > 32 * 1024
            || anchor.season == 0
            || anchor.season > MAXIMUM_SAFE_INTEGER
            || anchor.episode == 0
            || anchor.episode > MAXIMUM_SAFE_INTEGER
            || anchor.file_identity.is_empty()
            || anchor.file_identity.len() > 256
            || anchor.fingerprint.is_empty()
            || anchor.fingerprint.len() > 512
            || !anchor_paths.insert(anchor.relative_path.as_str())
        {
            return Err(TV_METADATA_PERSISTENCE_FAILED);
        }
    }
    Ok(())
}

fn encoded_tv_metadata_associations(
    associations: &[TvMetadataAssociation],
) -> Result<Vec<u8>, &'static str> {
    if associations.len() > TV_METADATA_MAX_RECORDS {
        return Err(TV_METADATA_PERSISTENCE_FAILED);
    }
    let mut payload = format!("{}\n", associations.len());
    for association in associations {
        validate_tv_metadata_association(association)?;
        let mut fields = vec![
            encode_tv_metadata_text(
                association
                    .folder
                    .to_str()
                    .ok_or(TV_METADATA_PERSISTENCE_FAILED)?,
            ),
            encode_tv_metadata_text(&association.folder_identity),
            encode_tv_metadata_text(&association.local_show_title),
            association.anchors.len().to_string(),
            association.tmdb_tv_id.to_string(),
            encode_tv_metadata_text(&association.imdb_id),
            encode_tv_metadata_text(&association.name),
            encode_tv_metadata_text(association.original_name.as_deref().unwrap_or("")),
            encode_tv_metadata_text(association.first_air_date.as_deref().unwrap_or("")),
            encode_tv_metadata_text(association.poster_path.as_deref().unwrap_or("")),
            encode_tv_metadata_text(association.overview.as_deref().unwrap_or("")),
            association.generation.to_string(),
        ];
        for anchor in &association.anchors {
            fields.extend([
                encode_tv_metadata_text(&anchor.relative_path),
                anchor.season.to_string(),
                anchor.episode.to_string(),
                encode_tv_metadata_text(&anchor.file_identity),
                encode_tv_metadata_text(&anchor.fingerprint),
                anchor.size.to_string(),
            ]);
        }
        payload.push_str(&fields.join("\t"));
        payload.push('\n');
    }
    let mut bytes = TV_METADATA_HEADER.to_vec();
    bytes.extend_from_slice(hex_sha1(payload.as_bytes()).as_bytes());
    bytes.push(b'\n');
    bytes.extend_from_slice(payload.as_bytes());
    if bytes.len() as u64 > TV_METADATA_MAX_BYTES {
        return Err(TV_METADATA_PERSISTENCE_FAILED);
    }
    Ok(bytes)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TvMetadataReadError {
    Invalid,
    Unavailable,
}

fn parse_tv_metadata_associations(
    bytes: &[u8],
) -> Result<Vec<TvMetadataAssociation>, TvMetadataReadError> {
    if bytes.len() as u64 > TV_METADATA_MAX_BYTES {
        return Err(TvMetadataReadError::Invalid);
    }
    let content = bytes
        .strip_prefix(TV_METADATA_HEADER)
        .ok_or(TvMetadataReadError::Invalid)?;
    let checksum_end = content
        .iter()
        .position(|byte| *byte == b'\n')
        .ok_or(TvMetadataReadError::Invalid)?;
    let checksum =
        std::str::from_utf8(&content[..checksum_end]).map_err(|_| TvMetadataReadError::Invalid)?;
    let payload = &content[checksum_end + 1..];
    if checksum.len() != 40 || checksum != hex_sha1(payload).as_str() {
        return Err(TvMetadataReadError::Invalid);
    }
    let payload = std::str::from_utf8(payload).map_err(|_| TvMetadataReadError::Invalid)?;
    let payload = payload
        .strip_suffix('\n')
        .ok_or(TvMetadataReadError::Invalid)?;
    let mut lines = payload.split('\n');
    let count = lines
        .next()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|count| *count <= TV_METADATA_MAX_RECORDS)
        .ok_or(TvMetadataReadError::Invalid)?;
    let mut associations = Vec::with_capacity(count);
    let mut group_keys = HashSet::new();
    let mut anchor_keys = HashSet::new();
    let mut generations = HashSet::new();
    for _ in 0..count {
        let fields = lines
            .next()
            .map(|line| line.split('\t').collect::<Vec<_>>())
            .ok_or(TvMetadataReadError::Invalid)?;
        if fields.len() < 12 {
            return Err(TvMetadataReadError::Invalid);
        }
        let anchor_count = fields[3]
            .parse::<usize>()
            .ok()
            .filter(|count| *count > 0 && *count <= TV_METADATA_MAX_ANCHORS)
            .ok_or(TvMetadataReadError::Invalid)?;
        if fields.len() != 12 + anchor_count * 6 {
            return Err(TvMetadataReadError::Invalid);
        }
        let optional_text = |value: &str| {
            decode_tv_metadata_text(value)
                .map(|value| (!value.is_empty()).then_some(value))
                .ok_or(TvMetadataReadError::Invalid)
        };
        let folder =
            PathBuf::from(decode_tv_metadata_text(fields[0]).ok_or(TvMetadataReadError::Invalid)?);
        let folder_identity =
            decode_tv_metadata_text(fields[1]).ok_or(TvMetadataReadError::Invalid)?;
        let local_show_title =
            decode_tv_metadata_text(fields[2]).ok_or(TvMetadataReadError::Invalid)?;
        let mut anchors = Vec::with_capacity(anchor_count);
        for anchor_fields in fields[12..].chunks_exact(6) {
            let anchor = TvMetadataAnchor {
                relative_path: decode_tv_metadata_text(anchor_fields[0])
                    .ok_or(TvMetadataReadError::Invalid)?,
                season: anchor_fields[1]
                    .parse()
                    .map_err(|_| TvMetadataReadError::Invalid)?,
                episode: anchor_fields[2]
                    .parse()
                    .map_err(|_| TvMetadataReadError::Invalid)?,
                file_identity: decode_tv_metadata_text(anchor_fields[3])
                    .ok_or(TvMetadataReadError::Invalid)?,
                fingerprint: decode_tv_metadata_text(anchor_fields[4])
                    .ok_or(TvMetadataReadError::Invalid)?,
                size: anchor_fields[5]
                    .parse()
                    .map_err(|_| TvMetadataReadError::Invalid)?,
            };
            if !anchor_keys.insert((
                folder.clone(),
                folder_identity.clone(),
                anchor.relative_path.clone(),
            )) {
                return Err(TvMetadataReadError::Invalid);
            }
            anchors.push(anchor);
        }
        let association = TvMetadataAssociation {
            folder: folder.clone(),
            folder_identity: folder_identity.clone(),
            local_show_title: local_show_title.clone(),
            anchors,
            tmdb_tv_id: fields[4]
                .parse()
                .map_err(|_| TvMetadataReadError::Invalid)?,
            imdb_id: decode_tv_metadata_text(fields[5]).ok_or(TvMetadataReadError::Invalid)?,
            name: decode_tv_metadata_text(fields[6]).ok_or(TvMetadataReadError::Invalid)?,
            original_name: optional_text(fields[7])?,
            first_air_date: optional_text(fields[8])?,
            poster_path: optional_text(fields[9])?,
            overview: optional_text(fields[10])?,
            generation: fields[11]
                .parse()
                .map_err(|_| TvMetadataReadError::Invalid)?,
        };
        validate_tv_metadata_association(&association).map_err(|_| TvMetadataReadError::Invalid)?;
        if !group_keys.insert((folder, folder_identity, local_show_title))
            || !generations.insert(association.generation)
        {
            return Err(TvMetadataReadError::Invalid);
        }
        associations.push(association);
    }
    if lines.next().is_some() {
        return Err(TvMetadataReadError::Invalid);
    }
    Ok(associations)
}

fn read_tv_metadata_associations(
    path: &Path,
) -> Result<Vec<TvMetadataAssociation>, TvMetadataReadError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(TvMetadataReadError::Unavailable),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(TvMetadataReadError::Invalid);
    }
    if metadata.len() > TV_METADATA_MAX_BYTES {
        return Err(TvMetadataReadError::Invalid);
    }
    let bytes = fs::read(path).map_err(|_| TvMetadataReadError::Unavailable)?;
    parse_tv_metadata_associations(&bytes)
}

fn write_tv_metadata_associations(
    path: &Path,
    associations: &[TvMetadataAssociation],
) -> Result<(), &'static str> {
    let bytes = encoded_tv_metadata_associations(associations)?;
    let parent = path.parent().ok_or(TV_METADATA_PERSISTENCE_FAILED)?;
    fs::create_dir_all(parent).map_err(|_| TV_METADATA_PERSISTENCE_FAILED)?;
    let file_name = path.file_name().ok_or(TV_METADATA_PERSISTENCE_FAILED)?;
    let mut replacement_name = file_name.to_os_string();
    replacement_name.push(".next");
    let replacement = path.with_file_name(replacement_name);
    match fs::symlink_metadata(&replacement) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(TV_METADATA_PERSISTENCE_FAILED);
        }
        Ok(_) => fs::remove_file(&replacement).map_err(|_| TV_METADATA_PERSISTENCE_FAILED)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => return Err(TV_METADATA_PERSISTENCE_FAILED),
    }
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&replacement)
            .map_err(|_| TV_METADATA_PERSISTENCE_FAILED)?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| TV_METADATA_PERSISTENCE_FAILED)?;
        replace_movie_metadata_file(&replacement, path)
            .map_err(|_| TV_METADATA_PERSISTENCE_FAILED)?;
        #[cfg(unix)]
        let _ = File::open(parent).and_then(|directory| directory.sync_all());
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&replacement);
    }
    result
}

fn scan_media_files(folder: &Path) -> Result<(String, Vec<TrustedTvFile>), &'static str> {
    let canonical_folder = fs::canonicalize(folder).map_err(|_| TV_FOLDER_UNAVAILABLE)?;
    if canonical_folder != folder
        || !fs::metadata(&canonical_folder)
            .map_err(|_| TV_FOLDER_UNAVAILABLE)?
            .is_dir()
    {
        return Err(TV_FOLDER_UNAVAILABLE);
    }
    let folder_identity = movie_path_identity(folder, false).map_err(|_| TV_LIBRARY_SCAN_FAILED)?;
    let files = scan_library_files(folder, |path, metadata| {
        let canonical_path = fs::canonicalize(&path).ok()?;
        if canonical_path != path || !canonical_path.starts_with(folder) {
            return None;
        }
        let relative_path = path
            .strip_prefix(folder)
            .ok()
            .and_then(Path::to_str)
            .filter(|relative| !relative.is_empty())?
            .to_owned();
        let file_identity = movie_path_identity(&path, true).ok()?;
        Some(TrustedTvFile {
            path,
            identity: parse_tv_relative_identity(&relative_path),
            relative_path,
            file_identity,
            fingerprint: movie_file_fingerprint(&metadata),
            size: metadata.len(),
            modified: metadata.modified().ok()?,
        })
    })
    .map_err(|_| TV_LIBRARY_SCAN_FAILED)?;
    Ok((folder_identity, files))
}

fn tv_metadata_anchor(file: &TrustedTvFile) -> Option<TvMetadataAnchor> {
    let identity = file.identity.as_ref()?;
    Some(TvMetadataAnchor {
        relative_path: file.relative_path.clone(),
        season: identity.season,
        episode: identity.episode,
        file_identity: file.file_identity.clone(),
        fingerprint: file.fingerprint.clone(),
        size: file.size,
    })
}

fn anchor_matches_file(
    anchor: &TvMetadataAnchor,
    local_show_title: &str,
    file: &TrustedTvFile,
) -> bool {
    file.identity.as_ref().is_some_and(|identity| {
        identity.show_title == local_show_title
            && identity.season == anchor.season
            && identity.episode == anchor.episode
    }) && file.relative_path == anchor.relative_path
        && file.file_identity == anchor.file_identity
        && file.fingerprint == anchor.fingerprint
        && file.size == anchor.size
}

fn build_tv_show_groups(
    folder_identity: &str,
    _generation: u64,
    files: &[TrustedTvFile],
) -> Vec<TrustedTvShowGroup> {
    let mut grouped = BTreeMap::<String, Vec<&TrustedTvFile>>::new();
    for file in files {
        if let Some(identity) = &file.identity {
            grouped
                .entry(identity.show_title.clone())
                .or_default()
                .push(file);
        }
    }
    grouped
        .into_iter()
        .map(|(show_title, members)| {
            let mut authority = format!("{folder_identity}\0{show_title}");
            let member_paths = members
                .into_iter()
                .map(|file| {
                    authority.push('\0');
                    authority.push_str(&file.relative_path);
                    authority.push('\0');
                    authority.push_str(&file.file_identity);
                    authority.push('\0');
                    authority.push_str(&file.fingerprint);
                    authority.push('\0');
                    authority.push_str(&file.size.to_string());
                    file.path.clone()
                })
                .collect();
            TrustedTvShowGroup {
                group_id: hex_sha1(authority.as_bytes()),
                show_title,
                member_paths,
                association: None,
                metadata_attention: false,
            }
        })
        .collect()
}

fn reconcile_tv_metadata_associations(
    folder: &Path,
    folder_identity: &str,
    files: &[TrustedTvFile],
    groups: &mut [TrustedTvShowGroup],
    associations: &mut Vec<TvMetadataAssociation>,
) -> bool {
    let mut changed = false;
    let mut index = 0;
    while index < associations.len() {
        if associations[index].folder != folder
            || associations[index].folder_identity != folder_identity
        {
            index += 1;
            continue;
        }
        let association = associations[index].clone();
        let group_index = groups
            .iter()
            .position(|group| group.show_title == association.local_show_title);
        let mut retained = Vec::new();
        let mut invalidated = false;
        for anchor in &association.anchors {
            match files
                .iter()
                .find(|file| file.relative_path == anchor.relative_path)
            {
                Some(file) if anchor_matches_file(anchor, &association.local_show_title, file) => {
                    retained.push(anchor.clone());
                }
                Some(_) => invalidated = true,
                None => match fs::symlink_metadata(folder.join(&anchor.relative_path)) {
                    Err(error) if error.kind() == io::ErrorKind::NotFound => changed = true,
                    _ => invalidated = true,
                },
            }
        }
        if invalidated || retained.is_empty() || group_index.is_none() {
            associations.remove(index);
            if let Some(group_index) = group_index {
                groups[group_index].metadata_attention = true;
            }
            changed = true;
            continue;
        }
        if retained.len() != association.anchors.len() {
            associations[index].anchors = retained.clone();
        }
        let mut current = association;
        current.anchors = retained;
        groups[group_index.expect("current anchor must retain its parsed group")].association =
            Some(current);
        index += 1;
    }
    changed
}

fn encode_tv_metadata_association(association: &TvMetadataAssociation) -> Vec<String> {
    vec![
        association.tmdb_tv_id.to_string(),
        association.imdb_id.clone(),
        association.name.clone(),
        association.original_name.clone().unwrap_or_default(),
        association.first_air_date.clone().unwrap_or_default(),
        association.poster_path.clone().unwrap_or_default(),
        association.overview.clone().unwrap_or_default(),
        association.generation.to_string(),
    ]
}

fn encode_tv_scan(scan: &CompletedTvScan) -> Result<Vec<String>, &'static str> {
    let mut response = Vec::with_capacity(4 + scan.files.len() * 16);
    response.extend([
        "tv-library-metadata-v1".to_owned(),
        scan.metadata_status.to_owned(),
        scan.generation.to_string(),
        scan.files.len().to_string(),
    ]);
    for file in &scan.files {
        response.push(
            file.path
                .to_str()
                .map(str::to_owned)
                .ok_or(TV_LIBRARY_SCAN_FAILED)?,
        );
        response.push(file.relative_path.clone());
        response.push(file.size.to_string());
        let group = file.identity.as_ref().and_then(|identity| {
            scan.groups
                .iter()
                .find(|group| group.show_title == identity.show_title)
        });
        if let (Some(identity), Some(group)) = (&file.identity, group) {
            response.extend([
                identity.show_title.clone(),
                identity.season.to_string(),
                identity.episode.to_string(),
                group.group_id.clone(),
            ]);
            if let Some(association) = &group.association {
                response.push("ready".to_owned());
                response.extend(encode_tv_metadata_association(association));
            } else {
                response.push(if group.metadata_attention {
                    "attention".to_owned()
                } else {
                    String::new()
                });
                response.extend((0..8).map(|_| String::new()));
            }
        } else {
            response.extend((0..13).map(|_| String::new()));
        }
    }
    Ok(response)
}

fn scan_tv_library_with_path_before_persistence(
    state: &TvLibraryState,
    association_path: Option<&Path>,
    before_persistence: impl FnOnce(),
) -> Result<Vec<String>, &'static str> {
    let (folder, generation) = {
        let mut context = state.0.lock().map_err(|_| TV_LIBRARY_SCAN_FAILED)?;
        let folder = context.folder.clone().ok_or(TV_FOLDER_UNAVAILABLE)?;
        context.generation = context.generation.wrapping_add(1);
        context.completed_scan = None;
        invalidate_tv_metadata_context(&mut context);
        (folder, context.generation)
    };
    let (folder_identity, files) = scan_media_files(&folder)?;
    let mut groups = build_tv_show_groups(&folder_identity, generation, &files);
    let (mut associations, mut metadata_status) = match association_path {
        Some(path) => match read_tv_metadata_associations(path) {
            Ok(associations) => (associations, "ready"),
            Err(TvMetadataReadError::Invalid) => (Vec::new(), "attention"),
            Err(TvMetadataReadError::Unavailable) => (Vec::new(), "unavailable"),
        },
        None => (Vec::new(), "ready"),
    };
    let associations_changed = metadata_status == "ready"
        && reconcile_tv_metadata_associations(
            &folder,
            &folder_identity,
            &files,
            &mut groups,
            &mut associations,
        );
    before_persistence();

    let mut context = state.0.lock().map_err(|_| TV_LIBRARY_SCAN_FAILED)?;
    if context.generation != generation || context.folder.as_ref() != Some(&folder) {
        return Err(TV_LIBRARY_STALE);
    }
    if associations_changed
        && association_path
            .is_some_and(|path| write_tv_metadata_associations(path, &associations).is_err())
    {
        metadata_status = "unavailable";
    }
    let scan = CompletedTvScan {
        folder,
        folder_identity,
        generation,
        files,
        groups,
        metadata_status,
    };
    let response = encode_tv_scan(&scan)?;
    context.completed_scan = Some(scan);
    Ok(response)
}

fn scan_tv_library_with_path(
    state: &TvLibraryState,
    association_path: Option<&Path>,
) -> Result<Vec<String>, &'static str> {
    scan_tv_library_with_path_before_persistence(state, association_path, || {})
}

#[cfg(test)]
fn scan_tv_library_with_metadata_before_persistence(
    state: &TvLibraryState,
    association_path: &Path,
    before_persistence: impl FnOnce(),
) -> Result<Vec<String>, &'static str> {
    scan_tv_library_with_path_before_persistence(state, Some(association_path), before_persistence)
}

pub fn scan_tv_library_with_metadata(
    state: &TvLibraryState,
    association_path: &Path,
) -> Result<Vec<String>, &'static str> {
    scan_tv_library_with_path(state, Some(association_path))
}

#[cfg(test)]
pub(crate) fn scan_tv_library_with(state: &TvLibraryState) -> Result<Vec<String>, &'static str> {
    let response = scan_tv_library_with_path(state, None)?;
    let mut legacy = vec![response[2].clone()];
    for row in response[4..].chunks_exact(16) {
        legacy.extend(row[..6].iter().cloned());
    }
    Ok(legacy)
}

fn next_tv_metadata_operation(context: &mut TvLibraryContext) -> u64 {
    context.metadata_operation_generation = context.metadata_operation_generation.wrapping_add(1);
    if context.metadata_operation_generation == 0 {
        context.metadata_operation_generation = 1;
    }
    context.metadata_operation_generation
}

fn tv_metadata_authority(
    context: &TvLibraryContext,
    group_id: &str,
) -> Result<TvMetadataAuthority, &'static str> {
    let scan = context.completed_scan.as_ref().ok_or(TV_METADATA_STALE)?;
    if scan.metadata_status != "ready" {
        return Err(TV_METADATA_UNAVAILABLE);
    }
    let group = scan
        .groups
        .iter()
        .find(|group| group.group_id == group_id)
        .filter(|group| !group.metadata_attention)
        .ok_or(TV_METADATA_STALE)?;
    let mut anchors = Vec::with_capacity(group.member_paths.len());
    for path in &group.member_paths {
        validate_tv_file(path, &scan.folder, scan, Some(scan.generation))
            .map_err(|_| TV_METADATA_STALE)?;
        let file = scan
            .files
            .iter()
            .find(|file| &file.path == path)
            .ok_or(TV_METADATA_STALE)?;
        let anchor = tv_metadata_anchor(file).ok_or(TV_METADATA_STALE)?;
        if file
            .identity
            .as_ref()
            .is_none_or(|identity| identity.show_title != group.show_title)
        {
            return Err(TV_METADATA_STALE);
        }
        anchors.push(anchor);
    }
    if anchors.is_empty() {
        return Err(TV_METADATA_STALE);
    }
    Ok(TvMetadataAuthority {
        scan_generation: scan.generation,
        folder: scan.folder.clone(),
        folder_identity: scan.folder_identity.clone(),
        group_id: group.group_id.clone(),
        local_show_title: group.show_title.clone(),
        anchors,
    })
}

pub(crate) fn tv_tmdb_cover_authority(
    state: &TvLibraryState,
    group_id: &str,
) -> Result<TvTmdbCoverAuthority, &'static str> {
    let context = state.0.lock().map_err(|_| TV_METADATA_UNAVAILABLE)?;
    let authority = tv_metadata_authority(&context, group_id)?;
    let scan = context.completed_scan.as_ref().ok_or(TV_METADATA_STALE)?;
    let association = scan
        .groups
        .iter()
        .find(|group| group.group_id == group_id)
        .and_then(|group| group.association.as_ref())
        .ok_or(TV_METADATA_STALE)?;
    Ok(TvTmdbCoverAuthority {
        scan_generation: authority.scan_generation,
        group_id: group_id.to_owned(),
        tmdb_id: association.tmdb_tv_id,
        poster_path: association.poster_path.clone(),
        association_generation: association.generation,
    })
}

fn validate_tv_metadata_authority(
    context: &TvLibraryContext,
    authority: &TvMetadataAuthority,
) -> Result<(), &'static str> {
    let current = tv_metadata_authority(context, &authority.group_id)?;
    if &current != authority {
        return Err(TV_METADATA_STALE);
    }
    Ok(())
}

fn tv_metadata_optional_text(
    object: &BTreeMap<String, JsonValue>,
    key: &str,
    max_bytes: usize,
) -> Result<Option<String>, &'static str> {
    match object.get(key) {
        None | Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(value)) if value.trim().is_empty() => Ok(None),
        Some(JsonValue::String(value)) if value.len() <= max_bytes => Ok(Some(value.clone())),
        _ => Err(TV_METADATA_MALFORMED),
    }
}

fn tv_metadata_optional_date(
    object: &BTreeMap<String, JsonValue>,
    key: &str,
) -> Result<Option<String>, &'static str> {
    match object.get(key) {
        None | Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(value)) if value.is_empty() => Ok(None),
        Some(JsonValue::String(value)) if valid_tv_metadata_date(value) => Ok(Some(value.clone())),
        _ => Err(TV_METADATA_MALFORMED),
    }
}

fn tv_metadata_optional_poster(
    object: &BTreeMap<String, JsonValue>,
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
        _ => Err(TV_METADATA_MALFORMED),
    }
}

pub(crate) fn parse_tv_metadata_candidates(
    document: &str,
) -> Result<Vec<TvMetadataCandidate>, &'static str> {
    let document = JsonParser::new(document)
        .parse()
        .and_then(|value| json_object(&value).cloned())
        .ok_or(TV_METADATA_MALFORMED)?;
    let results = document
        .get("results")
        .and_then(json_array)
        .ok_or(TV_METADATA_MALFORMED)?;
    if results.len() > TV_METADATA_MAX_CANDIDATES {
        return Err(TV_METADATA_MALFORMED);
    }
    let mut candidates = Vec::with_capacity(results.len());
    let mut ids = HashSet::new();
    for value in results {
        let object = json_object(value).ok_or(TV_METADATA_MALFORMED)?;
        let tmdb_tv_id = json_u64(object, "id")
            .filter(|id| *id > 0)
            .ok_or(TV_METADATA_MALFORMED)?;
        let name = json_string(object, "name")
            .filter(|name| !name.trim().is_empty() && name.len() <= 16 * 1024)
            .map(str::to_owned)
            .ok_or(TV_METADATA_MALFORMED)?;
        if !ids.insert(tmdb_tv_id) {
            return Err(TV_METADATA_MALFORMED);
        }
        candidates.push(TvMetadataCandidate {
            tmdb_tv_id,
            name,
            original_name: tv_metadata_optional_text(object, "original_name", 16 * 1024)?,
            first_air_date: tv_metadata_optional_date(object, "first_air_date")?,
            poster_path: tv_metadata_optional_poster(object)?,
        });
    }
    Ok(candidates)
}

pub(crate) fn percent_encode_tv_metadata_query(query: &str) -> String {
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

pub(crate) fn begin_tv_metadata_search(
    state: &TvLibraryState,
    group_id: &str,
    query: &str,
    token: &str,
    client_generation: u64,
) -> Result<(u64, String), &'static str> {
    if query.trim().is_empty()
        || query.len() > TV_METADATA_MAX_QUERY_BYTES
        || query.chars().any(char::is_control)
        || token.trim().is_empty()
    {
        return Err(TV_METADATA_CONTEXT_INVALID);
    }
    let mut context = state.0.lock().map_err(|_| TV_METADATA_UNAVAILABLE)?;
    begin_tv_metadata_client_operation(&mut context, client_generation)?;
    let authority = tv_metadata_authority(&context, group_id)?;
    let operation_generation = next_tv_metadata_operation(&mut context);
    let token_identity = hex_sha1(token.as_bytes());
    let request_id = hex_sha1(
        format!(
            "tv-search\0{operation_generation}\0{}\0{}\0{query}\0{token_identity}",
            authority.group_id, authority.local_show_title
        )
        .as_bytes(),
    );
    context.metadata_verification = None;
    context.metadata_search = Some(TvMetadataSearch {
        request_id: request_id.clone(),
        operation_generation,
        authority,
        query: query.to_owned(),
        token_identity,
        candidates: Vec::new(),
    });
    Ok((operation_generation, request_id))
}

fn encode_tv_metadata_search(search: &TvMetadataSearch) -> Vec<String> {
    let mut response = Vec::with_capacity(2 + search.candidates.len() * 5);
    response.extend([
        search.request_id.clone(),
        search.candidates.len().to_string(),
    ]);
    for candidate in &search.candidates {
        response.extend([
            candidate.tmdb_tv_id.to_string(),
            candidate.name.clone(),
            candidate.original_name.clone().unwrap_or_default(),
            candidate.first_air_date.clone().unwrap_or_default(),
            candidate.poster_path.clone().unwrap_or_default(),
        ]);
    }
    response
}

pub(crate) fn finish_tv_metadata_search(
    state: &TvLibraryState,
    operation_generation: u64,
    request_id: &str,
    token: &str,
    candidates: Vec<TvMetadataCandidate>,
) -> Result<Vec<String>, &'static str> {
    let mut context = state.0.lock().map_err(|_| TV_METADATA_UNAVAILABLE)?;
    let search = context
        .metadata_search
        .as_ref()
        .filter(|search| {
            search.operation_generation == operation_generation
                && search.request_id == request_id
                && search.token_identity == hex_sha1(token.as_bytes())
        })
        .cloned()
        .ok_or(TV_METADATA_CONTEXT_INVALID)?;
    validate_tv_metadata_authority(&context, &search.authority)?;
    let current = context
        .metadata_search
        .as_mut()
        .ok_or(TV_METADATA_CONTEXT_INVALID)?;
    current.candidates = candidates;
    Ok(encode_tv_metadata_search(current))
}

pub(crate) fn begin_tv_metadata_verification(
    state: &TvLibraryState,
    matching_request_id: &str,
    tmdb_tv_id: u64,
    token: &str,
    client_generation: u64,
) -> Result<(u64, TvMetadataSearch), &'static str> {
    if tmdb_tv_id == 0 || token.trim().is_empty() {
        return Err(TV_METADATA_CONTEXT_INVALID);
    }
    let mut context = state.0.lock().map_err(|_| TV_METADATA_UNAVAILABLE)?;
    begin_tv_metadata_client_operation(&mut context, client_generation)?;
    let search = context
        .metadata_search
        .as_ref()
        .filter(|search| {
            search.request_id == matching_request_id
                && search.token_identity == hex_sha1(token.as_bytes())
                && search
                    .candidates
                    .iter()
                    .any(|candidate| candidate.tmdb_tv_id == tmdb_tv_id)
        })
        .cloned()
        .ok_or(TV_METADATA_CONTEXT_INVALID)?;
    validate_tv_metadata_authority(&context, &search.authority)?;
    let operation_generation = next_tv_metadata_operation(&mut context);
    context.metadata_verification = None;
    Ok((operation_generation, search))
}

pub(crate) fn parse_verified_tv_metadata(
    search: &TvMetadataSearch,
    tmdb_tv_id: u64,
    details_document: &str,
    external_ids_document: &str,
) -> Result<TvMetadataAssociation, &'static str> {
    let details = JsonParser::new(details_document)
        .parse()
        .and_then(|value| json_object(&value).cloned())
        .ok_or(TV_METADATA_MALFORMED)?;
    let external_ids = JsonParser::new(external_ids_document)
        .parse()
        .and_then(|value| json_object(&value).cloned())
        .ok_or(TV_METADATA_MALFORMED)?;
    if json_u64(&details, "id") != Some(tmdb_tv_id)
        || json_u64(&external_ids, "id") != Some(tmdb_tv_id)
    {
        return Err(TV_METADATA_MALFORMED);
    }
    let imdb_id = json_string(&external_ids, "imdb_id")
        .and_then(canonical_imdb_id)
        .ok_or(TV_METADATA_MALFORMED)?;
    let name = json_string(&details, "name")
        .filter(|name| !name.trim().is_empty() && name.len() <= 16 * 1024)
        .map(str::to_owned)
        .ok_or(TV_METADATA_MALFORMED)?;
    let association = TvMetadataAssociation {
        folder: search.authority.folder.clone(),
        folder_identity: search.authority.folder_identity.clone(),
        local_show_title: search.authority.local_show_title.clone(),
        anchors: search.authority.anchors.clone(),
        tmdb_tv_id,
        imdb_id,
        name,
        original_name: tv_metadata_optional_text(&details, "original_name", 16 * 1024)?,
        first_air_date: tv_metadata_optional_date(&details, "first_air_date")?,
        poster_path: tv_metadata_optional_poster(&details)?,
        overview: tv_metadata_optional_text(&details, "overview", 256 * 1024)?,
        generation: 1,
    };
    validate_tv_metadata_association(&association).map_err(|_| TV_METADATA_MALFORMED)?;
    Ok(association)
}

pub(crate) fn finish_tv_metadata_verification(
    state: &TvLibraryState,
    operation_generation: u64,
    search: &TvMetadataSearch,
    tmdb_tv_id: u64,
    token: &str,
    association: TvMetadataAssociation,
) -> Result<Vec<String>, &'static str> {
    let mut context = state.0.lock().map_err(|_| TV_METADATA_UNAVAILABLE)?;
    if context.metadata_operation_generation != operation_generation
        || context
            .metadata_search
            .as_ref()
            .is_none_or(|current| current.request_id != search.request_id)
        || search.token_identity != hex_sha1(token.as_bytes())
        || association.tmdb_tv_id != tmdb_tv_id
    {
        return Err(TV_METADATA_CONTEXT_INVALID);
    }
    validate_tv_metadata_authority(&context, &search.authority)?;
    let verification_id = hex_sha1(
        format!(
            "tv-verify\0{operation_generation}\0{}\0{tmdb_tv_id}\0{}",
            search.request_id, association.imdb_id
        )
        .as_bytes(),
    );
    context.metadata_verification = Some(TvMetadataVerification {
        verification_id: verification_id.clone(),
        operation_generation,
        matching_request_id: search.request_id.clone(),
        authority: search.authority.clone(),
        association: association.clone(),
        token_identity: search.token_identity.clone(),
    });
    let mut response = vec![verification_id];
    response.extend(encode_tv_metadata_association(&association));
    Ok(response)
}

pub fn save_tv_metadata_match_with(
    state: &TvLibraryState,
    persistence_path: &Path,
    verification_id: &str,
) -> Result<Vec<String>, &'static str> {
    let mut context = state.0.lock().map_err(|_| TV_METADATA_UNAVAILABLE)?;
    let verification = context
        .metadata_verification
        .as_ref()
        .filter(|verification| verification.verification_id == verification_id)
        .cloned()
        .ok_or(TV_METADATA_CONTEXT_INVALID)?;
    validate_tv_metadata_authority(&context, &verification.authority)?;
    let mut associations = read_tv_metadata_associations(persistence_path).map_err(|error| {
        if error == TvMetadataReadError::Unavailable {
            TV_METADATA_UNAVAILABLE
        } else {
            TV_METADATA_PERSISTENCE_FAILED
        }
    })?;
    associations.retain(|association| {
        association.folder != verification.authority.folder
            || association.folder_identity != verification.authority.folder_identity
            || association.local_show_title != verification.authority.local_show_title
    });
    let mut association = verification.association;
    association.generation = associations
        .iter()
        .map(|association| association.generation)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or(TV_METADATA_PERSISTENCE_FAILED)?;
    validate_tv_metadata_association(&association)?;
    associations.push(association.clone());
    write_tv_metadata_associations(persistence_path, &associations)?;
    let scan = context.completed_scan.as_mut().ok_or(TV_METADATA_STALE)?;
    let group = scan
        .groups
        .iter_mut()
        .find(|group| group.group_id == verification.authority.group_id)
        .ok_or(TV_METADATA_STALE)?;
    group.association = Some(association.clone());
    group.metadata_attention = false;
    scan.metadata_status = "ready";
    invalidate_tv_metadata_context(&mut context);
    Ok(encode_tv_metadata_association(&association))
}

pub fn clear_tv_metadata_match_with(
    state: &TvLibraryState,
    persistence_path: &Path,
    group_id: &str,
) -> Result<(), &'static str> {
    let mut context = state.0.lock().map_err(|_| TV_METADATA_UNAVAILABLE)?;
    let authority = tv_metadata_authority(&context, group_id)?;
    let mut associations = read_tv_metadata_associations(persistence_path).map_err(|error| {
        if error == TvMetadataReadError::Unavailable {
            TV_METADATA_UNAVAILABLE
        } else {
            TV_METADATA_PERSISTENCE_FAILED
        }
    })?;
    let original_count = associations.len();
    associations.retain(|association| {
        association.folder != authority.folder
            || association.folder_identity != authority.folder_identity
            || association.local_show_title != authority.local_show_title
    });
    if associations.len() == original_count {
        return Err(TV_METADATA_STALE);
    }
    write_tv_metadata_associations(persistence_path, &associations)?;
    let scan = context.completed_scan.as_mut().ok_or(TV_METADATA_STALE)?;
    let group = scan
        .groups
        .iter_mut()
        .find(|group| group.group_id == group_id)
        .ok_or(TV_METADATA_STALE)?;
    group.association = None;
    group.metadata_attention = false;
    scan.metadata_status = "ready";
    invalidate_tv_metadata_context(&mut context);
    Ok(())
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
        || movie_path_identity(configured_folder, false)
            .ok()
            .as_deref()
            != Some(scan.folder_identity.as_str())
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
    if !is_supported_library_media(requested_path) {
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
        || movie_path_identity(requested_path, true).ok().as_deref()
            != Some(trusted_file.file_identity.as_str())
        || movie_file_fingerprint(&metadata) != trusted_file.fingerprint
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
    association_path: Option<&Path>,
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
    let scan = context.completed_scan.as_mut().ok_or(TV_FILE_TRASH_STALE)?;
    scan.files.retain(|file| file.path != path);
    scan.groups = build_tv_show_groups(&scan.folder_identity, scan.generation, &scan.files);
    let (mut associations, mut metadata_status) = match association_path {
        Some(path) => match read_tv_metadata_associations(path) {
            Ok(associations) => (associations, "ready"),
            Err(TvMetadataReadError::Invalid) => (Vec::new(), "attention"),
            Err(TvMetadataReadError::Unavailable) => (Vec::new(), "unavailable"),
        },
        None => (Vec::new(), "ready"),
    };
    if metadata_status == "ready"
        && reconcile_tv_metadata_associations(
            &scan.folder,
            &scan.folder_identity,
            &scan.files,
            &mut scan.groups,
            &mut associations,
        )
        && association_path
            .is_some_and(|path| write_tv_metadata_associations(path, &associations).is_err())
    {
        metadata_status = "unavailable";
    }
    scan.metadata_status = metadata_status;
    invalidate_tv_metadata_context(&mut context);
    Ok(())
}

fn trash_tv_file_with_download_ownership_path(
    path: &Path,
    scan_generation: u64,
    download_state: &VrDownloadState,
    library_state: &TvLibraryState,
    association_path: Option<&Path>,
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
        trash_trusted_tv_file_with(
            path,
            scan_generation,
            library_state,
            association_path,
            dispatch,
        )
    })
    .map_err(|error| match error {
        VrLibraryTrashOwnershipError::Owned => TV_FILE_TRASH_OWNED,
        VrLibraryTrashOwnershipError::Unavailable => TV_FILE_TRASH_OWNERSHIP_UNAVAILABLE,
    })?
}

#[cfg(test)]
pub(crate) fn trash_tv_file_with_download_ownership(
    path: &Path,
    scan_generation: u64,
    download_state: &VrDownloadState,
    library_state: &TvLibraryState,
    dispatch: impl FnOnce(&Path) -> Result<(), ()>,
) -> Result<(), &'static str> {
    trash_tv_file_with_download_ownership_path(
        path,
        scan_generation,
        download_state,
        library_state,
        None,
        dispatch,
    )
}

pub fn trash_tv_file_with_download_ownership_and_metadata(
    path: &Path,
    scan_generation: u64,
    download_state: &VrDownloadState,
    library_state: &TvLibraryState,
    association_path: &Path,
    dispatch: impl FnOnce(&Path) -> Result<(), ()>,
) -> Result<(), &'static str> {
    trash_tv_file_with_download_ownership_path(
        path,
        scan_generation,
        download_state,
        library_state,
        Some(association_path),
        dispatch,
    )
}

#[cfg(test)]
fn trash_tv_file_with(
    path: &Path,
    scan_generation: u64,
    state: &TvLibraryState,
    dispatch: impl FnOnce(&Path) -> Result<(), ()>,
) -> Result<(), &'static str> {
    trash_trusted_tv_file_with(path, scan_generation, state, None, dispatch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::Cell,
        sync::{
            atomic::{AtomicU64, Ordering},
            Arc, Barrier,
        },
        thread,
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

    fn verified_tv_association(
        state: &TvLibraryState,
        association_path: &Path,
        group_id: &str,
        client_generation: u64,
    ) -> TvMetadataAssociation {
        let token = "fixture-token";
        let (search_generation, request_id) = begin_tv_metadata_search(
            state,
            group_id,
            "Exact Local Show",
            token,
            client_generation,
        )
        .expect("metadata search must begin");
        let candidates = parse_tv_metadata_candidates(
            r#"{"results":[{"id":701,"name":"Canonical Show","original_name":"Original Show","first_air_date":"2020-04-03","poster_path":"/poster.jpg"}]}"#,
        )
        .expect("candidate fixture must be valid");
        finish_tv_metadata_search(state, search_generation, &request_id, token, candidates)
            .expect("metadata search must finish");
        let (verification_generation, search) =
            begin_tv_metadata_verification(state, &request_id, 701, token, client_generation + 1)
                .expect("metadata verification must begin");
        let association = parse_verified_tv_metadata(
            &search,
            701,
            r#"{"id":701,"name":"Canonical Show","original_name":"Original Show","first_air_date":"2020-04-03","poster_path":"/poster.jpg","overview":"Exact overview."}"#,
            r#"{"id":701,"imdb_id":"tt1234567"}"#,
        )
        .expect("provider identity must verify");
        let verification = finish_tv_metadata_verification(
            state,
            verification_generation,
            &search,
            701,
            token,
            association,
        )
        .expect("metadata verification must finish");
        save_tv_metadata_match_with(state, association_path, &verification[0])
            .expect("association must persist");
        read_tv_metadata_associations(association_path)
            .expect("association store must load")
            .into_iter()
            .next()
            .expect("association must exist")
    }

    fn tmdb_test_jpeg() -> Vec<u8> {
        let mut bytes = vec![0xff, 0xd8, 0xff, 0xc0, 0, 17, 8];
        bytes.extend(750_u16.to_be_bytes());
        bytes.extend(500_u16.to_be_bytes());
        bytes.resize(6_000, 0);
        bytes
    }

    #[test]
    fn tv_cover_command_rejects_replaced_scans_and_reuses_stable_cache_after_rescan() {
        use crate::tmdb_cover::{
            confirm_cover, fetch_cover, TmdbCoverCategory, TmdbCoverRequest, TmdbCoverState,
            TmdbCoverSurface,
        };

        let fixture = Fixture::new("tmdb-cover-scan");
        let folder_path = fixture.path.join("folder-store");
        let association_path = fixture.path.join("association-store");
        fs::write(fixture.path.join("Exact Local Show.S01E01.mkv"), b"episode").unwrap();
        let state = TvLibraryState::default();
        set_tv_folder(&state, &folder_path, fixture.path.clone()).unwrap();
        let initial = scan_tv_library_with_metadata(&state, &association_path).unwrap();
        verified_tv_association(&state, &association_path, &initial[10], 1);
        let first = tv_tmdb_cover_authority(&state, &initial[10]).unwrap();
        let rescanned = scan_tv_library_with_metadata(&state, &association_path).unwrap();
        let current = tv_tmdb_cover_authority(&state, &rescanned[10]).unwrap();
        assert_eq!(current.group_id, first.group_id);
        assert_ne!(current.scan_generation, first.scan_generation);

        let cache = fixture.path.join("cover-cache");
        let movie_state = crate::MoviesLibraryState::default();
        let cover_state = TmdbCoverState::default();
        let request = TmdbCoverRequest {
            category: TmdbCoverCategory::Tv,
            surface: TmdbCoverSurface::Library,
            tmdb_id: current.tmdb_id,
            poster_path: current.poster_path.clone(),
            context_generation: current.association_generation,
            request_generation: 1,
            library_item_id: Some(current.group_id.clone()),
            association_generation: Some(current.association_generation),
            scan_generation: Some(current.scan_generation),
        };
        let stale = TmdbCoverRequest {
            scan_generation: Some(first.scan_generation),
            ..request.clone()
        };
        assert_eq!(
            crate::resolve_tmdb_library_cover_command_with(
                &stale,
                &cache,
                &movie_state,
                &state,
                &cover_state,
                |_| Ok(tmdb_test_jpeg()),
            ),
            Err(crate::tmdb_cover::TMDB_COVER_STALE.to_owned())
        );

        let remote = Cell::new(0);
        let response = crate::resolve_tmdb_library_cover_command_with(
            &request,
            &cache,
            &movie_state,
            &state,
            &cover_state,
            |_| {
                remote.set(remote.get() + 1);
                Ok(tmdb_test_jpeg())
            },
        )
        .unwrap();
        assert_eq!(
            fetch_cover(&cover_state, &request, &response[11]).unwrap(),
            tmdb_test_jpeg()
        );
        confirm_cover(&cover_state, &cache, &request, &response[11]).unwrap();

        let later = scan_tv_library_with_metadata(&state, &association_path).unwrap();
        let later_authority = tv_tmdb_cover_authority(&state, &later[10]).unwrap();
        let revisit = TmdbCoverRequest {
            scan_generation: Some(later_authority.scan_generation),
            request_generation: 2,
            ..request.clone()
        };
        crate::resolve_tmdb_library_cover_command_with(
            &revisit,
            &cache,
            &movie_state,
            &state,
            &cover_state,
            |_| {
                remote.set(remote.get() + 1);
                Err(crate::ProviderRequestError::Network)
            },
        )
        .unwrap();

        let restarted = TvLibraryState::default();
        load_tv_folder_with(&restarted, &folder_path).unwrap();
        let restart_scan = scan_tv_library_with_metadata(&restarted, &association_path).unwrap();
        let restart_authority = tv_tmdb_cover_authority(&restarted, &restart_scan[10]).unwrap();
        let restart_request = TmdbCoverRequest {
            scan_generation: Some(restart_authority.scan_generation),
            request_generation: 3,
            ..request
        };
        crate::resolve_tmdb_library_cover_command_with(
            &restart_request,
            &cache,
            &movie_state,
            &restarted,
            &TmdbCoverState::default(),
            |_| {
                remote.set(remote.get() + 1);
                Err(crate::ProviderRequestError::Network)
            },
        )
        .unwrap();
        assert_eq!(remote.get(), 1);
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
        let unsupported = fixture.path.join("unsupported.txt");
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
        let movie = fixture.path.join("Show.S01E02.MOV");
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
        let first = fixture.path.join("Show.S01E01.AVI");
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
        let unsupported = trusted.path.join("unsupported.txt");
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

    #[test]
    fn exact_show_metadata_association_survives_restart_and_reconciles_members_conservatively() {
        let fixture = Fixture::new("metadata-restart");
        let configuration = Fixture::new("metadata-restart-config");
        let folder_path = configuration.path.join("tv-folder");
        let association_path = configuration.path.join("tv-metadata");
        let first = fixture.path.join("Exact Local Show.S01E01.mp4");
        let second = fixture.path.join("Exact Local Show.S01E02.mkv");
        fs::write(&first, b"first exact episode").expect("first member must be written");
        fs::write(&second, b"second exact episode").expect("second member must be written");
        let state = TvLibraryState::default();
        set_tv_folder(&state, &folder_path, fixture.path.clone())
            .expect("TV folder must be configured");
        let initial = scan_tv_library_with_metadata(&state, &association_path)
            .expect("trusted metadata scan must complete");
        let group_id = initial[10].clone();
        let persisted = verified_tv_association(&state, &association_path, &group_id, 1);
        assert_eq!(persisted.anchors.len(), 2);

        let restarted = TvLibraryState::default();
        load_tv_folder_with(&restarted, &folder_path).expect("folder must reload");
        let reloaded = scan_tv_library_with_metadata(&restarted, &association_path)
            .expect("restart scan must complete");
        assert_eq!(&reloaded[1], "ready");
        assert_eq!(&reloaded[11], "ready");
        assert_eq!(&reloaded[14], "Canonical Show");

        let new_member = fixture.path.join("Exact Local Show.S01E03.mp4");
        fs::write(&new_member, b"new unverified local episode")
            .expect("new local member must be written");
        let with_new_member = scan_tv_library_with_metadata(&restarted, &association_path)
            .expect("new-member scan must complete");
        assert_eq!(with_new_member[3], "3");
        assert!(with_new_member[4..]
            .chunks_exact(16)
            .all(|row| row[7] == "ready" && row[10] == "Canonical Show"));
        assert_eq!(
            read_tv_metadata_associations(&association_path).expect("store must remain readable")
                [0]
            .anchors
            .len(),
            2,
            "a new local member must not become provider-verified episode data"
        );

        fs::remove_file(&first).expect("one persisted anchor must be removed");
        let one_anchor = scan_tv_library_with_metadata(&restarted, &association_path)
            .expect("missing-anchor scan must complete");
        assert!(one_anchor[4..]
            .chunks_exact(16)
            .all(|row| row[7] == "ready"));
        assert_eq!(
            read_tv_metadata_associations(&association_path).expect("reconciled store must load")
                [0]
            .anchors
            .len(),
            1
        );

        fs::remove_file(&second).expect("final persisted anchor must be removed");
        let without_anchor = scan_tv_library_with_metadata(&restarted, &association_path)
            .expect("final-anchor scan must complete");
        assert_eq!(without_anchor[3], "1");
        assert_eq!(&without_anchor[11], "attention");
        assert!(without_anchor[12..20].iter().all(String::is_empty));
        assert!(read_tv_metadata_associations(&association_path)
            .expect("empty reconciled store must load")
            .is_empty());
        assert!(new_member.is_file());
    }

    #[test]
    fn replaced_anchor_invalidates_association_without_hiding_the_local_group() {
        let fixture = Fixture::new("metadata-replacement");
        let association_path = fixture.path.join("association-store");
        let file = fixture.path.join("Exact Local Show.S01E01.mkv");
        fs::write(&file, b"original exact member").expect("member must be written");
        let state = TvLibraryState::default();
        set_tv_folder(
            &state,
            &fixture.path.join("folder-store"),
            fixture.path.clone(),
        )
        .expect("TV folder must be configured");
        let scan = scan_tv_library_with_metadata(&state, &association_path)
            .expect("trusted scan must complete");
        verified_tv_association(&state, &association_path, &scan[10], 1);

        fs::remove_file(&file).expect("original anchor must be removed");
        fs::write(&file, b"replacement file at the same path")
            .expect("replacement object must be created at the exact path");
        let replaced = scan_tv_library_with_metadata(&state, &association_path)
            .expect("replacement scan must keep base Library usable");
        assert_eq!(&replaced[1], "ready");
        assert_eq!(&replaced[3], "1");
        assert_eq!(&replaced[11], "attention");
        assert!(replaced[12..20].iter().all(String::is_empty));
        assert!(read_tv_metadata_associations(&association_path)
            .expect("reconciled store must load")
            .is_empty());
    }

    #[test]
    fn provider_identity_requires_unique_candidates_matching_ids_and_canonical_imdb_series() {
        assert_eq!(
            parse_tv_metadata_candidates(
                r#"{"results":[{"id":7,"name":"First"},{"id":7,"name":"Conflict"}]}"#,
            ),
            Err(TV_METADATA_MALFORMED)
        );
        let fixture = Fixture::new("metadata-provider");
        let media = fixture.path.join("Exact Local Show.S01E01.mp4");
        fs::write(&media, b"episode").expect("member must be written");
        let state = TvLibraryState::default();
        set_tv_folder(
            &state,
            &fixture.path.join("folder-store"),
            fixture.path.clone(),
        )
        .expect("TV folder must be configured");
        let scan = scan_tv_library_with_metadata(&state, &fixture.path.join("metadata-store"))
            .expect("scan must complete");
        let token = "fixture-token";
        let (operation, request_id) =
            begin_tv_metadata_search(&state, &scan[10], "Exact Local Show", token, 1)
                .expect("search must begin");
        let candidates = parse_tv_metadata_candidates(
            r#"{"results":[{"id":7,"name":"Same Name","first_air_date":"2001-01-01"},{"id":8,"name":"Same Name","first_air_date":"2021-01-01"}]}"#,
        )
        .expect("same-name distinct candidates must remain distinct");
        finish_tv_metadata_search(&state, operation, &request_id, token, candidates)
            .expect("search must finish");
        let (_, search) = begin_tv_metadata_verification(&state, &request_id, 8, token, 2)
            .expect("manual second-candidate selection must begin verification");
        for (details, external_ids) in [
            (
                r#"{"id":7,"name":"Wrong details"}"#,
                r#"{"id":8,"imdb_id":"tt1234567"}"#,
            ),
            (
                r#"{"id":8,"name":"Exact details"}"#,
                r#"{"id":7,"imdb_id":"tt1234567"}"#,
            ),
            (
                r#"{"id":8,"name":"Exact details"}"#,
                r#"{"id":8,"imdb_id":"nm1234567"}"#,
            ),
        ] {
            assert_eq!(
                parse_verified_tv_metadata(&search, 8, details, external_ids),
                Err(TV_METADATA_MALFORMED)
            );
        }
        let exact = parse_verified_tv_metadata(
            &search,
            8,
            r#"{"id":8,"name":"Exact details"}"#,
            r#"{"id":8,"imdb_id":"tt1234567"}"#,
        )
        .expect("matching provider identities must verify");
        assert_eq!(exact.tmdb_tv_id, 8);
        assert_eq!(exact.imdb_id, "tt1234567");
    }

    #[test]
    fn stale_member_and_context_invalidation_cannot_finish_or_save_metadata() {
        let fixture = Fixture::new("metadata-stale");
        let association_path = fixture.path.join("metadata-store");
        let media = fixture.path.join("Exact Local Show.S01E01.mp4");
        fs::write(&media, b"trusted episode").expect("member must be written");
        let state = TvLibraryState::default();
        set_tv_folder(
            &state,
            &fixture.path.join("folder-store"),
            fixture.path.clone(),
        )
        .expect("TV folder must be configured");
        let scan =
            scan_tv_library_with_metadata(&state, &association_path).expect("scan must complete");
        let (operation, request_id) =
            begin_tv_metadata_search(&state, &scan[10], "Exact Local Show", "fixture-token", 2)
                .expect("search must begin");
        fs::write(&media, b"replaced episode bytes").expect("member must change");
        assert_eq!(
            finish_tv_metadata_search(&state, operation, &request_id, "fixture-token", Vec::new(),),
            Err(TV_METADATA_STALE)
        );
        assert!(read_tv_metadata_associations(&association_path)
            .expect("unused store must remain empty")
            .is_empty());
        invalidate_tv_metadata_client_context(&state, 5).expect("context must invalidate");
        assert_eq!(
            begin_tv_metadata_search(&state, &scan[10], "Exact Local Show", "fixture-token", 4,),
            Err(TV_METADATA_CONTEXT_INVALID)
        );
    }

    #[test]
    fn corrupt_and_oversized_metadata_stores_fail_closed_while_tv_groups_remain_visible() {
        let fixture = Fixture::new("metadata-corruption");
        let media = fixture.path.join("Exact Local Show.S01E01.mp4");
        let association_path = fixture.path.join("metadata-store");
        fs::write(&media, b"episode").expect("member must be written");
        let state = TvLibraryState::default();
        set_tv_folder(
            &state,
            &fixture.path.join("folder-store"),
            fixture.path.clone(),
        )
        .expect("TV folder must be configured");

        fs::write(&association_path, b"corrupt association data")
            .expect("corrupt store must be written");
        let corrupt = scan_tv_library_with_metadata(&state, &association_path)
            .expect("corrupt metadata must not hide the Library");
        assert_eq!(&corrupt[1], "attention");
        assert_eq!(&corrupt[3], "1");
        assert_eq!(&corrupt[7], "Exact Local Show");
        assert_eq!(
            begin_tv_metadata_search(&state, &corrupt[10], "Exact Local Show", "fixture-token", 1,),
            Err(TV_METADATA_UNAVAILABLE)
        );

        let oversized = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&association_path)
            .expect("oversized store must open");
        oversized
            .set_len(TV_METADATA_MAX_BYTES + 1)
            .expect("oversized store must be allocated");
        let oversized_scan = scan_tv_library_with_metadata(&state, &association_path)
            .expect("oversized metadata must not hide the Library");
        assert_eq!(&oversized_scan[1], "attention");
        assert_eq!(&oversized_scan[3], "1");
    }

    #[test]
    fn clear_and_member_trash_reconcile_only_metadata_for_exact_remaining_anchors() {
        let fixture = Fixture::new("metadata-clear-trash");
        let holding = Fixture::new("metadata-clear-trash-holding");
        let folder_path = fixture.path.join("folder-store");
        let association_path = fixture.path.join("association-store");
        let first = fixture.path.join("Exact Local Show.S01E01.mp4");
        let second = fixture.path.join("Exact Local Show.S01E02.mkv");
        fs::write(&first, b"first exact bytes").expect("first member must be written");
        fs::write(&second, b"second exact bytes").expect("second member must be written");
        let state = TvLibraryState::default();
        set_tv_folder(&state, &folder_path, fixture.path.clone())
            .expect("TV folder must be configured");
        let scan =
            scan_tv_library_with_metadata(&state, &association_path).expect("scan must complete");
        verified_tv_association(&state, &association_path, &scan[10], 1);

        clear_tv_metadata_match_with(&state, &association_path, &scan[10])
            .expect("metadata-only clear must persist");
        assert_eq!(
            fs::read(&first).expect("first member must remain"),
            b"first exact bytes"
        );
        assert_eq!(
            fs::read(&second).expect("second member must remain"),
            b"second exact bytes"
        );
        assert!(read_tv_metadata_associations(&association_path)
            .expect("cleared store must load")
            .is_empty());

        let rescanned = scan_tv_library_with_metadata(&state, &association_path)
            .expect("scan after clear must complete");
        let generation = rescanned[2].parse().expect("new generation must be valid");
        verified_tv_association(&state, &association_path, &rescanned[10], 4);
        let first_destination = holding.path.join("first.mp4");
        trash_trusted_tv_file_with(
            &first,
            generation,
            &state,
            Some(&association_path),
            |path| fs::rename(path, &first_destination).map_err(|_| ()),
        )
        .expect("first exact member Trash dispatch must succeed");
        let remaining = read_tv_metadata_associations(&association_path)
            .expect("remaining-anchor store must load");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].anchors.len(), 1);
        assert_eq!(
            remaining[0].anchors[0].relative_path,
            "Exact Local Show.S01E02.mkv"
        );
        assert!(second.is_file());

        let second_destination = holding.path.join("second.mkv");
        trash_trusted_tv_file_with(
            &second,
            generation,
            &state,
            Some(&association_path),
            |path| fs::rename(path, &second_destination).map_err(|_| ()),
        )
        .expect("final exact member Trash dispatch must succeed");
        assert!(read_tv_metadata_associations(&association_path)
            .expect("final-anchor reconciliation store must load")
            .is_empty());
        assert!(first_destination.is_file());
        assert!(second_destination.is_file());

        let restarted = TvLibraryState::default();
        load_tv_folder_with(&restarted, &folder_path).expect("folder must reload");
        let restarted_scan = scan_tv_library_with_metadata(&restarted, &association_path)
            .expect("restart scan must complete");
        assert_eq!(&restarted_scan[3], "0");
    }

    #[test]
    fn folder_isolation_hides_then_restores_only_the_exact_anchor_bound_association() {
        let configuration = Fixture::new("metadata-folder-isolation-config");
        let first_folder = Fixture::new("metadata-folder-isolation-first");
        let second_folder = Fixture::new("metadata-folder-isolation-second");
        let folder_path = configuration.path.join("folder-store");
        let association_path = configuration.path.join("association-store");
        let first_member = first_folder.path.join("Exact Local Show.S01E01.mp4");
        let second_member = second_folder.path.join("Exact Local Show.S01E01.mp4");
        fs::write(&first_member, b"first-folder member")
            .expect("first-folder member must be written");
        fs::write(&second_member, b"similar second-folder member")
            .expect("second-folder member must be written");
        let state = TvLibraryState::default();
        set_tv_folder(&state, &folder_path, first_folder.path.clone())
            .expect("first TV folder must be configured");
        let first_scan = scan_tv_library_with_metadata(&state, &association_path)
            .expect("first-folder scan must complete");
        verified_tv_association(&state, &association_path, &first_scan[10], 1);

        set_tv_folder(&state, &folder_path, second_folder.path.clone())
            .expect("second TV folder must be configured");
        let second_scan = scan_tv_library_with_metadata(&state, &association_path)
            .expect("second-folder scan must complete");
        assert_eq!(&second_scan[7], "Exact Local Show");
        assert!(second_scan[11..20].iter().all(String::is_empty));
        assert_eq!(
            read_tv_metadata_associations(&association_path)
                .expect("isolated association store must load")
                .len(),
            1
        );

        set_tv_folder(&state, &folder_path, first_folder.path.clone())
            .expect("first TV folder must be restored");
        let restored = scan_tv_library_with_metadata(&state, &association_path)
            .expect("restored first-folder scan must complete");
        assert_eq!(&restored[11], "ready");
        assert_eq!(&restored[14], "Canonical Show");
        assert!(first_member.is_file());
        assert!(second_member.is_file());
    }

    #[test]
    fn late_scan_cannot_resurrect_association_cleared_by_newer_scan() {
        let fixture = Fixture::new("metadata-late-scan-clear");
        let configuration = Fixture::new("metadata-late-scan-clear-config");
        let folder_path = configuration.path.join("folder-store");
        let association_path = configuration.path.join("association-store");
        let first = fixture.path.join("Exact Local Show.S01E01.mp4");
        let second = fixture.path.join("Exact Local Show.S01E02.mkv");
        fs::write(&first, b"first exact episode").expect("first member must be written");
        fs::write(&second, b"second exact episode").expect("second member must be written");
        let state = TvLibraryState::default();
        set_tv_folder(&state, &folder_path, fixture.path.clone())
            .expect("TV folder must be configured");
        let initial = scan_tv_library_with_metadata(&state, &association_path)
            .expect("initial scan must complete");
        verified_tv_association(&state, &association_path, &initial[10], 1);
        fs::remove_file(&first).expect("one persisted anchor must be removed");

        let reconciliation_ready = Arc::new(Barrier::new(2));
        let resume_late_scan = Arc::new(Barrier::new(2));
        let late_state = state.clone();
        let late_association_path = association_path.clone();
        let late_reconciliation_ready = Arc::clone(&reconciliation_ready);
        let late_resume = Arc::clone(&resume_late_scan);
        let late_scan = thread::spawn(move || {
            scan_tv_library_with_metadata_before_persistence(
                &late_state,
                &late_association_path,
                || {
                    late_reconciliation_ready.wait();
                    late_resume.wait();
                },
            )
        });
        reconciliation_ready.wait();

        let current = scan_tv_library_with_metadata(&state, &association_path)
            .expect("newer scan must become current");
        clear_tv_metadata_match_with(&state, &association_path, &current[10])
            .expect("newer scan must clear the exact association");
        assert!(read_tv_metadata_associations(&association_path)
            .expect("cleared store must load")
            .is_empty());

        resume_late_scan.wait();
        assert_eq!(
            late_scan.join().expect("late scan thread must finish"),
            Err(TV_LIBRARY_STALE)
        );
        assert!(read_tv_metadata_associations(&association_path)
            .expect("late scan must leave the cleared store readable")
            .is_empty());
        assert!(state
            .0
            .lock()
            .expect("TV state must remain available")
            .completed_scan
            .as_ref()
            .and_then(|scan| scan.groups.first())
            .is_some_and(|group| group.association.is_none()));

        let restarted = TvLibraryState::default();
        load_tv_folder_with(&restarted, &folder_path).expect("folder must reload");
        let restarted_scan = scan_tv_library_with_metadata(&restarted, &association_path)
            .expect("restart scan must complete");
        assert_eq!(&restarted_scan[11], "");
        assert!(restarted_scan[12..20].iter().all(String::is_empty));
        assert!(second.is_file());
    }

    #[test]
    fn late_old_folder_scan_cannot_overwrite_association_saved_by_current_folder() {
        let configuration = Fixture::new("metadata-late-old-folder-config");
        let old_folder = Fixture::new("metadata-late-old-folder-a");
        let current_folder = Fixture::new("metadata-late-old-folder-b");
        let folder_path = configuration.path.join("folder-store");
        let association_path = configuration.path.join("association-store");
        let old_first = old_folder.path.join("Exact Local Show.S01E01.mp4");
        let old_second = old_folder.path.join("Exact Local Show.S01E02.mkv");
        let current_member = current_folder.path.join("Exact Local Show.S01E01.mp4");
        fs::write(&old_first, b"old first episode").expect("old first member must be written");
        fs::write(&old_second, b"old second episode").expect("old second member must be written");
        fs::write(&current_member, b"current exact episode")
            .expect("current member must be written");
        let state = TvLibraryState::default();
        set_tv_folder(&state, &folder_path, old_folder.path.clone())
            .expect("old TV folder must be configured");
        let initial = scan_tv_library_with_metadata(&state, &association_path)
            .expect("old-folder scan must complete");
        verified_tv_association(&state, &association_path, &initial[10], 1);
        fs::remove_file(&old_first).expect("old association must require reconciliation");

        let reconciliation_ready = Arc::new(Barrier::new(2));
        let resume_late_scan = Arc::new(Barrier::new(2));
        let late_state = state.clone();
        let late_association_path = association_path.clone();
        let late_reconciliation_ready = Arc::clone(&reconciliation_ready);
        let late_resume = Arc::clone(&resume_late_scan);
        let late_scan = thread::spawn(move || {
            scan_tv_library_with_metadata_before_persistence(
                &late_state,
                &late_association_path,
                || {
                    late_reconciliation_ready.wait();
                    late_resume.wait();
                },
            )
        });
        reconciliation_ready.wait();

        set_tv_folder(&state, &folder_path, current_folder.path.clone())
            .expect("new TV folder must become current");
        let current = scan_tv_library_with_metadata(&state, &association_path)
            .expect("current-folder scan must complete");
        verified_tv_association(&state, &association_path, &current[10], 10);
        let current_store = read_tv_metadata_associations(&association_path)
            .expect("newer association store must load");
        assert_eq!(current_store.len(), 2);
        let saved = current_store
            .iter()
            .find(|association| association.folder == current_folder.path)
            .cloned()
            .expect("current-folder association must persist");
        assert_eq!(saved.folder, current_folder.path);
        assert_eq!(saved.generation, 2);

        resume_late_scan.wait();
        assert_eq!(
            late_scan.join().expect("late old-folder scan must finish"),
            Err(TV_LIBRARY_STALE)
        );
        assert_eq!(
            read_tv_metadata_associations(&association_path)
                .expect("late scan must not replace the newer store"),
            current_store
        );
        let current_group_association = state
            .0
            .lock()
            .expect("TV state must remain available")
            .completed_scan
            .as_ref()
            .and_then(|scan| scan.groups.first())
            .and_then(|group| group.association.as_ref())
            .cloned();
        assert_eq!(current_group_association, Some(saved.clone()));

        let restarted = TvLibraryState::default();
        load_tv_folder_with(&restarted, &folder_path).expect("current folder must reload");
        let restarted_scan = scan_tv_library_with_metadata(&restarted, &association_path)
            .expect("restart scan must retain the current association");
        assert_eq!(&restarted_scan[11], "ready");
        assert_eq!(&restarted_scan[14], "Canonical Show");
        assert_eq!(&restarted_scan[19], &saved.generation.to_string());
        assert!(old_second.is_file());
        assert!(current_member.is_file());
    }
}
