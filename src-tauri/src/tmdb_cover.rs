use std::{
    collections::{HashMap, HashSet},
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Condvar, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{vr_torrent::hex_sha1, ProviderRequestError};

pub(crate) const TMDB_COVER_FAILED: &str = "tmdb_cover_failed";
pub(crate) const TMDB_COVER_STALE: &str = "tmdb_cover_stale";

const CACHE_VERSION: &str = "AUTO_VIDEO_TMDB_CARD_COVER_V1";
const CACHE_TTL_SECONDS: u64 = 24 * 60 * 60;
const CACHE_MAX_FILES: usize = 256;
const CACHE_MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
const COVER_MAX_BYTES: usize = 16 * 1024 * 1024;
const COVER_MIN_BYTES: usize = 64;
const MAX_DIMENSION: u32 = 16_384;
const MAX_CURRENT_REQUESTS: usize = 256;
const MAX_CURRENT_AUTHORITIES: usize = 128;
const MAX_CONCURRENT_OPERATIONS: usize = 4;
const DEFAULT_RATIO: f64 = 2.0 / 3.0;
const MAX_RATIO: f64 = 4.0;
const TMDB_IMAGE_BASE_URL: &str = "https://image.tmdb.org/t/p/w500";
const STATUS_MARKER: &str = "\nAUTO_VIDEO_HTTP_STATUS:";
#[cfg(target_os = "macos")]
const STATUS_WRITE_OUT: &str = "\nAUTO_VIDEO_HTTP_STATUS:%{http_code}";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum TmdbCoverCategory {
    Movie,
    Tv,
}

impl TmdbCoverCategory {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "movie" => Some(Self::Movie),
            "tv" => Some(Self::Tv),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Movie => "movie",
            Self::Tv => "tv",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum TmdbCoverSurface {
    Discover,
    Library,
}

impl TmdbCoverSurface {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "discover" => Some(Self::Discover),
            "library" => Some(Self::Library),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Discover => "discover",
            Self::Library => "library",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TmdbCoverRequest {
    pub(crate) category: TmdbCoverCategory,
    pub(crate) surface: TmdbCoverSurface,
    pub(crate) tmdb_id: u64,
    pub(crate) poster_path: Option<String>,
    pub(crate) context_generation: u64,
    pub(crate) request_generation: u64,
    pub(crate) library_item_id: Option<String>,
    pub(crate) association_generation: Option<u64>,
    pub(crate) scan_generation: Option<u64>,
}

impl TmdbCoverRequest {
    pub(crate) fn validate(&self) -> bool {
        if self.tmdb_id == 0
            || self.tmdb_id > 9_007_199_254_740_991
            || self.context_generation == 0
            || self.request_generation == 0
            || self
                .poster_path
                .as_deref()
                .is_some_and(|path| tmdb_poster_url(path).is_none())
        {
            return false;
        }
        match self.surface {
            TmdbCoverSurface::Discover => {
                self.library_item_id.is_none()
                    && self.association_generation.is_none()
                    && self.scan_generation.is_none()
            }
            TmdbCoverSurface::Library => {
                self.library_item_id
                    .as_deref()
                    .is_some_and(valid_library_item_id)
                    && self
                        .association_generation
                        .is_some_and(|generation| generation > 0)
                    && self
                        .scan_generation
                        .is_some_and(|generation| generation > 0)
            }
        }
    }

    fn slot(&self) -> RequestSlot {
        RequestSlot {
            category: self.category,
            surface: self.surface,
            stable_id: self
                .library_item_id
                .clone()
                .unwrap_or_else(|| self.tmdb_id.to_string()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct RequestSlot {
    category: TmdbCoverCategory,
    surface: TmdbCoverSurface,
    stable_id: String,
}

#[derive(Clone)]
struct CoverAuthority {
    request: TmdbCoverRequest,
    authority_id: String,
    source: String,
    bytes: Vec<u8>,
    ratio: f64,
    persisted: bool,
}

#[derive(Default)]
struct TmdbCoverContext {
    requests: HashMap<RequestSlot, TmdbCoverRequest>,
    authorities: HashMap<String, CoverAuthority>,
    active_operations: usize,
}

#[derive(Clone, Default)]
pub(crate) struct TmdbCoverState {
    context: Arc<(Mutex<TmdbCoverContext>, Condvar)>,
    cache_admission: Arc<Mutex<()>>,
}

struct OperationPermit {
    state: TmdbCoverState,
}

impl Drop for OperationPermit {
    fn drop(&mut self) {
        let (lock, available) = &*self.state.context;
        if let Ok(mut context) = lock.lock() {
            context.active_operations = context.active_operations.saturating_sub(1);
            available.notify_all();
        }
    }
}

fn valid_library_item_id(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn tmdb_poster_url(path: &str) -> Option<String> {
    if path.len() < 2
        || path.len() > 1024
        || !path.starts_with('/')
        || path.starts_with("//")
        || path.contains("..")
        || path.contains('\\')
        || path
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
        || !path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.'))
        || ![".jpg", ".jpeg", ".png", ".webp"]
            .iter()
            .any(|extension| path.to_ascii_lowercase().ends_with(extension))
    {
        return None;
    }
    Some(format!("{TMDB_IMAGE_BASE_URL}{path}"))
}

fn image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        let mut offset = 2;
        while offset + 9 < bytes.len() {
            if bytes[offset] != 0xff {
                offset += 1;
                continue;
            }
            let marker = bytes[offset + 1];
            offset += 2;
            if marker == 0xd8 || marker == 0xd9 || marker == 0x01 {
                continue;
            }
            if offset + 2 > bytes.len() {
                return None;
            }
            let length = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]) as usize;
            if length < 2 || offset + length > bytes.len() {
                return None;
            }
            if matches!(
                marker,
                0xc0 | 0xc1
                    | 0xc2
                    | 0xc3
                    | 0xc5
                    | 0xc6
                    | 0xc7
                    | 0xc9
                    | 0xca
                    | 0xcb
                    | 0xcd
                    | 0xce
                    | 0xcf
            ) && length >= 7
            {
                return Some((
                    u16::from_be_bytes([bytes[offset + 5], bytes[offset + 6]]) as u32,
                    u16::from_be_bytes([bytes[offset + 3], bytes[offset + 4]]) as u32,
                ));
            }
            offset += length;
        }
        return None;
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.len() >= 24 {
        return Some((
            u32::from_be_bytes(bytes[16..20].try_into().ok()?),
            u32::from_be_bytes(bytes[20..24].try_into().ok()?),
        ));
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        if bytes.get(12..16) == Some(b"VP8X") && bytes.len() >= 30 {
            let width = 1 + u32::from_le_bytes([bytes[24], bytes[25], bytes[26], 0]);
            let height = 1 + u32::from_le_bytes([bytes[27], bytes[28], bytes[29], 0]);
            return Some((width, height));
        }
        if bytes.get(12..16) == Some(b"VP8L") && bytes.len() >= 25 {
            let bits = u32::from_le_bytes([bytes[21], bytes[22], bytes[23], bytes[24]]);
            return Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1));
        }
    }
    None
}

fn validate_cover(bytes: Vec<u8>) -> Result<(Vec<u8>, f64), ProviderRequestError> {
    if bytes.len() < COVER_MIN_BYTES || bytes.len() > COVER_MAX_BYTES {
        return Err(ProviderRequestError::Provider);
    }
    let (width, height) = image_dimensions(&bytes).ok_or(ProviderRequestError::Provider)?;
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(ProviderRequestError::Provider);
    }
    let ratio = f64::from(width) / f64::from(height);
    if !ratio.is_finite() || ratio <= 0.0 || ratio > MAX_RATIO {
        return Err(ProviderRequestError::Provider);
    }
    Ok((bytes, ratio))
}

fn request_is_current(context: &TmdbCoverContext, request: &TmdbCoverRequest) -> bool {
    context.requests.get(&request.slot()) == Some(request)
}

fn begin_request(state: &TmdbCoverState, request: &TmdbCoverRequest) -> Result<(), &'static str> {
    if !request.validate() {
        return Err(TMDB_COVER_STALE);
    }
    let (lock, available) = &*state.context;
    let mut context = lock.lock().map_err(|_| TMDB_COVER_FAILED)?;
    let slot = request.slot();
    context.requests.insert(slot.clone(), request.clone());
    context
        .authorities
        .retain(|_, authority| authority.request.slot() != slot);
    while context.requests.len() > MAX_CURRENT_REQUESTS {
        let obsolete = context.requests.keys().find(|key| **key != slot).cloned();
        let Some(obsolete) = obsolete else { break };
        context.requests.remove(&obsolete);
        context
            .authorities
            .retain(|_, authority| authority.request.slot() != obsolete);
    }
    available.notify_all();
    Ok(())
}

fn acquire_operation(
    state: &TmdbCoverState,
    request: &TmdbCoverRequest,
) -> Result<OperationPermit, &'static str> {
    let (lock, available) = &*state.context;
    let mut context = lock.lock().map_err(|_| TMDB_COVER_FAILED)?;
    loop {
        if !request_is_current(&context, request) {
            return Err(TMDB_COVER_STALE);
        }
        if context.active_operations < MAX_CONCURRENT_OPERATIONS {
            context.active_operations += 1;
            return Ok(OperationPermit {
                state: state.clone(),
            });
        }
        context = available.wait(context).map_err(|_| TMDB_COVER_FAILED)?;
    }
}

fn encode_text(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn decode_text(value: &str) -> Option<String> {
    if !value.len().is_multiple_of(2) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let bytes = value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            std::str::from_utf8(pair)
                .ok()
                .and_then(|digits| u8::from_str_radix(digits, 16).ok())
        })
        .collect::<Option<Vec<_>>>()?;
    String::from_utf8(bytes).ok()
}

fn cache_identity(request: &TmdbCoverRequest) -> String {
    let stable_item = request.library_item_id.as_deref().unwrap_or("");
    let association = request.association_generation.unwrap_or(0);
    hex_sha1(
        format!(
            "tmdb-cover\0{}\0{}\0{}\0{}\0{}\0{}",
            request.category.as_str(),
            request.surface.as_str(),
            stable_item,
            association,
            request.tmdb_id,
            request.poster_path.as_deref().unwrap_or("")
        )
        .as_bytes(),
    )
}

fn cache_path(directory: &Path, request: &TmdbCoverRequest) -> PathBuf {
    directory.join(format!("tmdb-cover-{}.bin", cache_identity(request)))
}

fn cache_header(
    request: &TmdbCoverRequest,
    source: &str,
    ratio: f64,
    timestamp: u64,
    length: usize,
) -> String {
    [
        CACHE_VERSION.to_owned(),
        request.category.as_str().to_owned(),
        request.surface.as_str().to_owned(),
        encode_text(request.library_item_id.as_deref().unwrap_or("")),
        request.association_generation.unwrap_or(0).to_string(),
        request.tmdb_id.to_string(),
        encode_text(request.poster_path.as_deref().unwrap_or("")),
        encode_text(source),
        timestamp.to_string(),
        ratio.to_string(),
        length.to_string(),
        "DATA".to_owned(),
    ]
    .join("\n")
        + "\n"
}

fn remove_cache_file_checked(path: &Path) -> Result<(), ()> {
    match fs::symlink_metadata(path) {
        Ok(_) => fs::remove_file(path).map_err(|_| ()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(()),
    }
}

fn remove_cache_file(path: &Path) {
    let _ = remove_cache_file_checked(path);
}

fn read_cache(directory: &Path, request: &TmdbCoverRequest, now: u64) -> Option<(Vec<u8>, f64)> {
    let path = cache_path(directory, request);
    let result = read_cache_file(&path, request, now);
    if result.is_none() && fs::symlink_metadata(&path).is_ok() {
        remove_cache_file(&path);
    }
    result
}

fn read_cache_file(path: &Path, request: &TmdbCoverRequest, now: u64) -> Option<(Vec<u8>, f64)> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > COVER_MAX_BYTES as u64 + 4096
    {
        return None;
    }
    let file = fs::read(path).ok()?;
    let marker = b"\nDATA\n";
    let marker_index = file
        .windows(marker.len())
        .position(|value| value == marker)?;
    let header = std::str::from_utf8(&file[..marker_index]).ok()?;
    let fields = header.lines().collect::<Vec<_>>();
    if fields.len() != 11
        || fields[0] != CACHE_VERSION
        || fields[1] != request.category.as_str()
        || fields[2] != request.surface.as_str()
        || decode_text(fields[3]).as_deref() != request.library_item_id.as_deref().or(Some(""))
        || fields[4].parse::<u64>().ok() != Some(request.association_generation.unwrap_or(0))
        || fields[5].parse::<u64>().ok() != Some(request.tmdb_id)
        || decode_text(fields[6]).as_deref() != request.poster_path.as_deref().or(Some(""))
    {
        return None;
    }
    let source = decode_text(fields[7])?;
    if request
        .poster_path
        .as_deref()
        .and_then(tmdb_poster_url)
        .as_deref()
        != Some(&source)
    {
        return None;
    }
    let timestamp = fields[8].parse::<u64>().ok()?;
    let ratio = fields[9].parse::<f64>().ok()?;
    let length = fields[10].parse::<usize>().ok()?;
    let bytes = file[marker_index + marker.len()..].to_vec();
    if now < timestamp
        || now - timestamp >= CACHE_TTL_SECONDS
        || length != bytes.len()
        || !ratio.is_finite()
        || ratio <= 0.0
        || ratio > MAX_RATIO
    {
        return None;
    }
    let Ok((bytes, validated_ratio)) = validate_cover(bytes) else {
        return None;
    };
    if (validated_ratio - ratio).abs() > 0.000_001 {
        return None;
    }
    Some((bytes, ratio))
}

#[derive(Clone)]
struct CacheEntry {
    path: PathBuf,
    length: u64,
    modified: SystemTime,
    temporary: bool,
}

fn cache_entries(directory: &Path) -> Vec<CacheEntry> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_str()?;
            let temporary = name.starts_with(".tmdb-cover-") && name.ends_with(".tmp");
            if !temporary && (!name.starts_with("tmdb-cover-") || !name.ends_with(".bin")) {
                return None;
            }
            let metadata = fs::symlink_metadata(&path).ok()?;
            Some(CacheEntry {
                path,
                length: metadata.len(),
                modified: metadata.modified().unwrap_or(UNIX_EPOCH),
                temporary,
            })
        })
        .collect()
}

#[cfg(test)]
fn cache_files(directory: &Path) -> Vec<(PathBuf, u64, SystemTime)> {
    cache_entries(directory)
        .into_iter()
        .filter(|entry| !entry.temporary)
        .map(|entry| (entry.path, entry.length, entry.modified))
        .collect()
}

fn bound_cache(
    directory: &Path,
    target: &Path,
    required_bytes: u64,
    protected: &HashSet<PathBuf>,
) -> Result<(), ()> {
    remove_cache_file_checked(target)?;
    let mut files = cache_entries(directory);
    files.sort_by(|left, right| {
        right
            .temporary
            .cmp(&left.temporary)
            .then_with(|| left.modified.cmp(&right.modified))
            .then_with(|| left.path.cmp(&right.path))
    });
    let mut total = files.iter().map(|entry| entry.length).sum::<u64>();
    while files.len() + 1 > CACHE_MAX_FILES
        || total.saturating_add(required_bytes) > CACHE_MAX_TOTAL_BYTES
    {
        let Some(index) = files
            .iter()
            .position(|entry| !protected.contains(&entry.path))
        else {
            return Err(());
        };
        let entry = &files[index];
        remove_cache_file_checked(&entry.path)?;
        total = total.saturating_sub(entry.length);
        files.remove(index);
    }
    Ok(())
}

fn write_cache(
    directory: &Path,
    request: &TmdbCoverRequest,
    source: &str,
    ratio: f64,
    bytes: &[u8],
    protected: &HashSet<PathBuf>,
) -> Result<(), ()> {
    fs::create_dir_all(directory).map_err(|_| ())?;
    let directory_metadata = fs::symlink_metadata(directory).map_err(|_| ())?;
    if directory_metadata.file_type().is_symlink() || !directory_metadata.is_dir() {
        return Err(());
    }
    let path = cache_path(directory, request);
    let header = cache_header(request, source, ratio, now_seconds(), bytes.len());
    let total_length = header.len() as u64 + bytes.len() as u64;
    bound_cache(directory, &path, total_length, protected)?;
    let temporary = directory.join(format!(
        ".tmdb-cover-{}-{}.tmp",
        cache_identity(request),
        request.request_generation
    ));
    remove_cache_file_checked(&temporary)?;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|_| ())?;
    if file.write_all(header.as_bytes()).is_err()
        || file.write_all(bytes).is_err()
        || file.sync_all().is_err()
    {
        let _ = remove_cache_file_checked(&temporary);
        return Err(());
    }
    if fs::rename(&temporary, &path).is_err() {
        let _ = remove_cache_file_checked(&temporary);
        return Err(());
    }
    Ok(())
}

fn active_cache_paths(context: &TmdbCoverContext, directory: &Path) -> HashSet<PathBuf> {
    context
        .requests
        .values()
        .map(|request| cache_path(directory, request))
        .collect()
}

fn authority_id(request: &TmdbCoverRequest, source: &str) -> String {
    format!(
        "tmdb-cover-{}",
        hex_sha1(
            format!(
                "{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}",
                request.category.as_str(),
                request.surface.as_str(),
                request.tmdb_id,
                request.poster_path.as_deref().unwrap_or(""),
                request.context_generation,
                request.request_generation,
                request.library_item_id.as_deref().unwrap_or(""),
                request.scan_generation.unwrap_or(0),
                source
            )
            .as_bytes()
        )
    )
}

fn pending_response(request: &TmdbCoverRequest, authority_id: &str, ratio: f64) -> Vec<String> {
    vec![
        "tmdb-card-cover-v1".to_owned(),
        "pending".to_owned(),
        request.category.as_str().to_owned(),
        request.surface.as_str().to_owned(),
        request.tmdb_id.to_string(),
        request.poster_path.clone().unwrap_or_default(),
        request.context_generation.to_string(),
        request.request_generation.to_string(),
        request.library_item_id.clone().unwrap_or_default(),
        request.association_generation.unwrap_or(0).to_string(),
        request.scan_generation.unwrap_or(0).to_string(),
        authority_id.to_owned(),
        ratio.to_string(),
        "TMDB".to_owned(),
    ]
}

fn missing_response(request: &TmdbCoverRequest) -> Vec<String> {
    vec![
        "tmdb-card-cover-v1".to_owned(),
        "missing".to_owned(),
        request.category.as_str().to_owned(),
        request.surface.as_str().to_owned(),
        request.tmdb_id.to_string(),
        String::new(),
        request.context_generation.to_string(),
        request.request_generation.to_string(),
        request.library_item_id.clone().unwrap_or_default(),
        request.association_generation.unwrap_or(0).to_string(),
        request.scan_generation.unwrap_or(0).to_string(),
        String::new(),
        DEFAULT_RATIO.to_string(),
        String::new(),
    ]
}

pub(crate) fn resolve_cover_with(
    state: &TmdbCoverState,
    cache_directory: &Path,
    request: &TmdbCoverRequest,
    is_current: impl Fn() -> bool,
    fetch: impl FnOnce(&str) -> Result<Vec<u8>, ProviderRequestError>,
) -> Result<Vec<String>, &'static str> {
    begin_request(state, request)?;
    if request.poster_path.is_none() {
        return Ok(missing_response(request));
    }
    let _permit = acquire_operation(state, request)?;
    if !is_current() {
        cancel_request(state, request);
        return Err(TMDB_COVER_STALE);
    }
    let source = tmdb_poster_url(request.poster_path.as_deref().unwrap_or_default())
        .ok_or(TMDB_COVER_FAILED)?;
    let cached = {
        let _cache = state
            .cache_admission
            .lock()
            .map_err(|_| TMDB_COVER_FAILED)?;
        read_cache(cache_directory, request, now_seconds())
    };
    let (bytes, ratio, persisted) = match cached {
        Some((bytes, ratio)) => (bytes, ratio, true),
        None => {
            let bytes = fetch(&source).map_err(|_| TMDB_COVER_FAILED)?;
            let (bytes, ratio) = validate_cover(bytes).map_err(|_| TMDB_COVER_FAILED)?;
            if !is_current_request(state, request) || !is_current() {
                return Err(TMDB_COVER_STALE);
            }
            (bytes, ratio, false)
        }
    };
    if !is_current_request(state, request) || !is_current() {
        return Err(TMDB_COVER_STALE);
    }
    let authority_id = authority_id(request, &source);
    let authority = CoverAuthority {
        request: request.clone(),
        authority_id: authority_id.clone(),
        source,
        bytes,
        ratio,
        persisted,
    };
    let (lock, _) = &*state.context;
    let mut context = lock.lock().map_err(|_| TMDB_COVER_FAILED)?;
    if !request_is_current(&context, request) {
        return Err(TMDB_COVER_STALE);
    }
    context.authorities.insert(authority_id.clone(), authority);
    while context.authorities.len() > MAX_CURRENT_AUTHORITIES {
        let obsolete = context
            .authorities
            .keys()
            .find(|key| **key != authority_id)
            .cloned();
        let Some(obsolete) = obsolete else { break };
        context.authorities.remove(&obsolete);
    }
    Ok(pending_response(request, &authority_id, ratio))
}

pub(crate) fn fetch_cover(
    state: &TmdbCoverState,
    request: &TmdbCoverRequest,
    requested_authority_id: &str,
) -> Result<Vec<u8>, &'static str> {
    let (lock, _) = &*state.context;
    let context = lock.lock().map_err(|_| TMDB_COVER_FAILED)?;
    if !request_is_current(&context, request) {
        return Err(TMDB_COVER_STALE);
    }
    let authority = context
        .authorities
        .get(requested_authority_id)
        .filter(|authority| {
            authority.authority_id == requested_authority_id
                && authority.request == *request
                && authority.ratio.is_finite()
        })
        .ok_or(TMDB_COVER_STALE)?;
    Ok(authority.bytes.clone())
}

pub(crate) fn confirm_cover(
    state: &TmdbCoverState,
    cache_directory: &Path,
    request: &TmdbCoverRequest,
    requested_authority_id: &str,
) -> Result<(), &'static str> {
    let _admission = state
        .cache_admission
        .lock()
        .map_err(|_| TMDB_COVER_FAILED)?;
    let (lock, _) = &*state.context;
    let mut context = lock.lock().map_err(|_| TMDB_COVER_FAILED)?;
    if !request_is_current(&context, request) {
        return Err(TMDB_COVER_STALE);
    }
    let authority = context
        .authorities
        .get(requested_authority_id)
        .filter(|authority| {
            authority.authority_id == requested_authority_id
                && authority.request == *request
                && authority.ratio.is_finite()
        })
        .cloned()
        .ok_or(TMDB_COVER_STALE)?;
    if !authority.persisted {
        let protected = active_cache_paths(&context, cache_directory);
        write_cache(
            cache_directory,
            request,
            &authority.source,
            authority.ratio,
            &authority.bytes,
            &protected,
        )
        .map_err(|_| TMDB_COVER_FAILED)?;
        context
            .authorities
            .get_mut(requested_authority_id)
            .ok_or(TMDB_COVER_STALE)?
            .persisted = true;
    }
    Ok(())
}

pub(crate) fn is_current_request(state: &TmdbCoverState, request: &TmdbCoverRequest) -> bool {
    let (lock, _) = &*state.context;
    lock.lock()
        .is_ok_and(|context| request_is_current(&context, request))
}

pub(crate) fn cancel_request(state: &TmdbCoverState, request: &TmdbCoverRequest) {
    let (lock, available) = &*state.context;
    if let Ok(mut context) = lock.lock() {
        if request_is_current(&context, request) {
            context.requests.remove(&request.slot());
            context
                .authorities
                .retain(|_, authority| authority.request != *request);
        }
        available.notify_all();
    }
}

pub(crate) fn invalidate_cover(
    state: &TmdbCoverState,
    cache_directory: &Path,
    request: &TmdbCoverRequest,
) -> Result<(), &'static str> {
    if !request.validate() {
        return Err(TMDB_COVER_STALE);
    }
    let _admission = state
        .cache_admission
        .lock()
        .map_err(|_| TMDB_COVER_FAILED)?;
    let (lock, available) = &*state.context;
    let mut context = lock.lock().map_err(|_| TMDB_COVER_FAILED)?;
    if !request_is_current(&context, request) {
        return Err(TMDB_COVER_STALE);
    }
    context
        .authorities
        .retain(|_, authority| authority.request != *request);
    drop(context);
    remove_cache_file_checked(&cache_path(cache_directory, request))
        .map_err(|_| TMDB_COVER_FAILED)?;
    available.notify_all();
    Ok(())
}

#[cfg(target_os = "macos")]
pub(crate) fn fetch_tmdb_image(url: &str) -> Result<Vec<u8>, ProviderRequestError> {
    if !url.starts_with(TMDB_IMAGE_BASE_URL) {
        return Err(ProviderRequestError::Provider);
    }
    let output = Command::new("/usr/bin/curl")
        .args([
            "--max-time",
            "20",
            "--max-filesize",
            "16777216",
            "--max-redirs",
            "0",
            "--proto",
            "=https",
            "--silent",
            "--show-error",
            "--write-out",
            STATUS_WRITE_OUT,
            url,
        ])
        .output()
        .map_err(|_| ProviderRequestError::Network)?;
    parse_transport_response(&output.stdout, output.status.success())
}

#[cfg(target_os = "windows")]
pub(crate) fn fetch_tmdb_image(url: &str) -> Result<Vec<u8>, ProviderRequestError> {
    if !url.starts_with(TMDB_IMAGE_BASE_URL) {
        return Err(ProviderRequestError::Provider);
    }
    let script = r#"$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);Add-Type -AssemblyName System.Net.Http;$h=[Net.Http.HttpClientHandler]::new();$h.AllowAutoRedirect=$false;$c=[Net.Http.HttpClient]::new($h);$d=[Threading.CancellationTokenSource]::new();$d.CancelAfter(20000);try{$r=$c.GetAsync($env:AUTO_VIDEO_TMDB_COVER_URL,[Net.Http.HttpCompletionOption]::ResponseHeadersRead,$d.Token).GetAwaiter().GetResult();$s=$r.Content.ReadAsStreamAsync().GetAwaiter().GetResult();$m=[IO.MemoryStream]::new();$b=[byte[]]::new(65536);while(($n=$s.ReadAsync($b,0,$b.Length,$d.Token).GetAwaiter().GetResult()) -gt 0){if($m.Length+$n -gt 16777216){[Environment]::Exit(63)};$m.Write($b,0,$n)};$o=[Console]::OpenStandardOutput();$v=$m.ToArray();$o.Write($v,0,$v.Length);$x=[Text.Encoding]::UTF8.GetBytes("`nAUTO_VIDEO_HTTP_STATUS:"+[int]$r.StatusCode);$o.Write($x,0,$x.Length)}catch{[Environment]::Exit(28)}finally{$d.Dispose();$c.Dispose();$h.Dispose()}"#;
    let output = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ])
        .env("AUTO_VIDEO_TMDB_COVER_URL", url)
        .output()
        .map_err(|_| ProviderRequestError::Network)?;
    parse_transport_response(&output.stdout, output.status.success())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn fetch_tmdb_image(_url: &str) -> Result<Vec<u8>, ProviderRequestError> {
    Err(ProviderRequestError::SourceUnavailable)
}

fn parse_transport_response(
    output: &[u8],
    process_succeeded: bool,
) -> Result<Vec<u8>, ProviderRequestError> {
    if !process_succeeded {
        return Err(ProviderRequestError::Network);
    }
    let marker = output
        .windows(STATUS_MARKER.len())
        .rposition(|value| value == STATUS_MARKER.as_bytes())
        .ok_or(ProviderRequestError::Provider)?;
    let status = std::str::from_utf8(&output[marker + STATUS_MARKER.len()..])
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or(ProviderRequestError::Provider)?;
    let body = &output[..marker];
    if body.len() > COVER_MAX_BYTES {
        return Err(ProviderRequestError::Provider);
    }
    match status {
        200 => Ok(body.to_vec()),
        301 | 302 | 303 | 307 | 308 => Err(ProviderRequestError::Provider),
        404 => Err(ProviderRequestError::SourceUnavailable),
        500..=599 => Err(ProviderRequestError::Provider),
        _ => Err(ProviderRequestError::Provider),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::Cell,
        sync::{
            atomic::{AtomicU64, AtomicUsize, Ordering},
            mpsc, Condvar, Mutex,
        },
    };

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("auto-video-tmdb-cover-{}-{id}", std::process::id()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn request(category: TmdbCoverCategory, surface: TmdbCoverSurface) -> TmdbCoverRequest {
        TmdbCoverRequest {
            category,
            surface,
            tmdb_id: 701,
            poster_path: Some("/exact-poster.jpg".to_owned()),
            context_generation: 4,
            request_generation: 9,
            library_item_id: (surface == TmdbCoverSurface::Library).then(|| "a".repeat(40)),
            association_generation: (surface == TmdbCoverSurface::Library).then_some(3),
            scan_generation: (surface == TmdbCoverSurface::Library).then_some(4),
        }
    }

    fn jpeg() -> Vec<u8> {
        let mut bytes = vec![0xff, 0xd8, 0xff, 0xc0, 0, 17, 8];
        bytes.extend(750_u16.to_be_bytes());
        bytes.extend(500_u16.to_be_bytes());
        bytes.resize(6_000, 0);
        bytes
    }

    #[test]
    fn exact_cover_is_a_zero_remote_hot_hit_on_revisit_and_restart() {
        for (category, surface) in [
            (TmdbCoverCategory::Movie, TmdbCoverSurface::Discover),
            (TmdbCoverCategory::Tv, TmdbCoverSurface::Discover),
            (TmdbCoverCategory::Movie, TmdbCoverSurface::Library),
            (TmdbCoverCategory::Tv, TmdbCoverSurface::Library),
        ] {
            let fixture = Fixture::new();
            let state = TmdbCoverState::default();
            let request = request(category, surface);
            let dispatches = Cell::new(0);
            let response = resolve_cover_with(
                &state,
                &fixture.0,
                &request,
                || true,
                |url| {
                    dispatches.set(dispatches.get() + 1);
                    assert_eq!(url, "https://image.tmdb.org/t/p/w500/exact-poster.jpg");
                    Ok(jpeg())
                },
            )
            .unwrap();
            assert_eq!(response[1], "pending");
            assert_eq!(response[2], category.as_str());
            assert_eq!(response[3], surface.as_str());
            assert_eq!(response[13], "TMDB");
            assert_eq!(
                fetch_cover(&state, &request, &response[11]).unwrap(),
                jpeg()
            );
            assert!(!cache_path(&fixture.0, &request).exists());
            confirm_cover(&state, &fixture.0, &request, &response[11]).unwrap();
            assert!(cache_path(&fixture.0, &request).exists());
            assert_eq!(dispatches.get(), 1);

            let mut revisit = request.clone();
            revisit.request_generation += 1;
            if revisit.surface == TmdbCoverSurface::Library {
                let original_cache_path = cache_path(&fixture.0, &revisit);
                revisit.scan_generation = revisit.scan_generation.map(|generation| generation + 1);
                assert_eq!(cache_path(&fixture.0, &revisit), original_cache_path);
            }
            let response = resolve_cover_with(
                &state,
                &fixture.0,
                &revisit,
                || true,
                |_| {
                    dispatches.set(dispatches.get() + 1);
                    Err(ProviderRequestError::Network)
                },
            )
            .unwrap();
            assert_eq!(
                fetch_cover(&state, &revisit, &response[11]).unwrap(),
                jpeg()
            );

            let restarted = TmdbCoverState::default();
            let mut restart_request = revisit.clone();
            restart_request.context_generation += 1;
            restart_request.request_generation += 1;
            let response = resolve_cover_with(
                &restarted,
                &fixture.0,
                &restart_request,
                || true,
                |_| {
                    dispatches.set(dispatches.get() + 1);
                    Err(ProviderRequestError::Network)
                },
            )
            .unwrap();
            assert_eq!(
                fetch_cover(&restarted, &restart_request, &response[11]).unwrap(),
                jpeg()
            );
            assert_eq!(dispatches.get(), 1);
        }
    }

    #[test]
    fn missing_crossed_stale_corrupt_expired_and_failed_persistence_fail_closed() {
        let fixture = Fixture::new();
        let state = TmdbCoverState::default();
        let mut missing = request(TmdbCoverCategory::Movie, TmdbCoverSurface::Discover);
        missing.poster_path = None;
        let dispatches = Cell::new(0);
        let response = resolve_cover_with(
            &state,
            &fixture.0,
            &missing,
            || true,
            |_| {
                dispatches.set(dispatches.get() + 1);
                Ok(jpeg())
            },
        )
        .unwrap();
        assert_eq!(response[1], "missing");
        assert_eq!(dispatches.get(), 0);

        let request = request(TmdbCoverCategory::Movie, TmdbCoverSurface::Library);
        let response =
            resolve_cover_with(&state, &fixture.0, &request, || true, |_| Ok(jpeg())).unwrap();
        for crossed in [
            TmdbCoverRequest {
                category: TmdbCoverCategory::Tv,
                ..request.clone()
            },
            TmdbCoverRequest {
                tmdb_id: request.tmdb_id + 1,
                ..request.clone()
            },
            TmdbCoverRequest {
                poster_path: Some("/crossed.jpg".to_owned()),
                ..request.clone()
            },
            TmdbCoverRequest {
                library_item_id: Some("b".repeat(40)),
                ..request.clone()
            },
            TmdbCoverRequest {
                association_generation: Some(4),
                ..request.clone()
            },
            TmdbCoverRequest {
                scan_generation: Some(5),
                ..request.clone()
            },
        ] {
            assert_eq!(
                fetch_cover(&state, &crossed, &response[11]),
                Err(TMDB_COVER_STALE)
            );
        }

        let path = cache_path(&fixture.0, &request);
        fs::write(&path, b"corrupt").unwrap();
        let mut retry = request.clone();
        retry.request_generation += 1;
        let calls = Cell::new(0);
        let response = resolve_cover_with(
            &state,
            &fixture.0,
            &retry,
            || true,
            |_| {
                calls.set(calls.get() + 1);
                Ok(jpeg())
            },
        )
        .unwrap();
        assert_eq!(calls.get(), 1);
        assert_eq!(fetch_cover(&state, &retry, &response[11]).unwrap(), jpeg());

        let invalid_directory = fixture.0.join("not-a-directory");
        fs::write(&invalid_directory, b"file").unwrap();
        let mut failed_write = request.clone();
        failed_write.request_generation += 2;
        let response = resolve_cover_with(
            &state,
            &invalid_directory,
            &failed_write,
            || true,
            |_| Ok(jpeg()),
        )
        .unwrap();
        assert_eq!(
            confirm_cover(&state, &invalid_directory, &failed_write, &response[11]),
            Err(TMDB_COVER_FAILED)
        );
    }

    #[test]
    fn raster_and_transport_validation_rejects_unsafe_or_unbounded_results() {
        assert!(tmdb_poster_url("/poster.webp").is_some());
        for path in [
            "poster.jpg",
            "//evil.example/a.jpg",
            "/../a.jpg",
            "/a.jpg?token=secret",
            "/a\\b.jpg",
            "/a.jpg ",
            "/a.svg",
        ] {
            assert!(tmdb_poster_url(path).is_none(), "{path}");
        }
        assert_eq!(
            validate_cover(b"<html>no</html>".to_vec()),
            Err(ProviderRequestError::Provider)
        );
        let mut invalid_dimensions = jpeg();
        invalid_dimensions[5] = 0;
        invalid_dimensions[6] = 0;
        assert_eq!(
            validate_cover(invalid_dimensions),
            Err(ProviderRequestError::Provider)
        );
        assert_eq!(
            validate_cover(vec![0; COVER_MAX_BYTES + 1]),
            Err(ProviderRequestError::Provider)
        );
        assert_eq!(
            parse_transport_response(b"redirect\nAUTO_VIDEO_HTTP_STATUS:302", true),
            Err(ProviderRequestError::Provider)
        );
        assert_eq!(
            parse_transport_response(b"body\nAUTO_VIDEO_HTTP_STATUS:200", false),
            Err(ProviderRequestError::Network)
        );
    }

    #[test]
    fn current_cover_operations_share_one_global_four_operation_bound() {
        let fixture = Fixture::new();
        let state = TmdbCoverState::default();
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let released = Arc::new((Mutex::new(HashSet::<u64>::new()), Condvar::new()));
        let (started_sender, started_receiver) = mpsc::channel();
        let mut workers = Vec::new();

        for item in 1..=5_u64 {
            let directory = fixture.0.clone();
            let state = state.clone();
            let active = active.clone();
            let maximum = maximum.clone();
            let released = released.clone();
            let started_sender = started_sender.clone();
            workers.push(std::thread::spawn(move || {
                let mut request = request(TmdbCoverCategory::Movie, TmdbCoverSurface::Discover);
                request.tmdb_id = item;
                request.poster_path = Some(format!("/poster-{item}.jpg"));
                resolve_cover_with(
                    &state,
                    &directory,
                    &request,
                    || true,
                    |_| {
                        let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                        maximum.fetch_max(current, Ordering::SeqCst);
                        started_sender.send(item).unwrap();
                        let (lock, ready) = &*released;
                        let mut values = lock.lock().unwrap();
                        while !values.contains(&item) {
                            values = ready.wait(values).unwrap();
                        }
                        active.fetch_sub(1, Ordering::SeqCst);
                        Ok(jpeg())
                    },
                )
                .unwrap();
            }));
        }
        drop(started_sender);
        let first = (0..4)
            .map(|_| started_receiver.recv().unwrap())
            .collect::<Vec<_>>();
        assert!(started_receiver.try_recv().is_err());
        {
            let (lock, ready) = &*released;
            lock.lock().unwrap().insert(first[0]);
            ready.notify_all();
        }
        let fifth = started_receiver.recv().unwrap();
        assert!(!first.contains(&fifth));
        {
            let (lock, ready) = &*released;
            let mut values = lock.lock().unwrap();
            values.extend(1..=5);
            ready.notify_all();
        }
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(maximum.load(Ordering::SeqCst), MAX_CONCURRENT_OPERATIONS);
    }

    #[test]
    fn expired_cache_and_supported_rasters_are_validated_before_reuse() {
        let fixture = Fixture::new();
        let state = TmdbCoverState::default();
        let request = request(TmdbCoverCategory::Tv, TmdbCoverSurface::Library);
        let source = tmdb_poster_url(request.poster_path.as_deref().unwrap()).unwrap();
        let bytes = jpeg();
        let header = cache_header(
            &request,
            &source,
            2.0 / 3.0,
            now_seconds() - CACHE_TTL_SECONDS,
            bytes.len(),
        );
        let mut expired = header.into_bytes();
        expired.extend(bytes);
        fs::write(cache_path(&fixture.0, &request), expired).unwrap();
        let dispatches = Cell::new(0);
        let response = resolve_cover_with(
            &state,
            &fixture.0,
            &request,
            || true,
            |_| {
                dispatches.set(dispatches.get() + 1);
                Ok(jpeg())
            },
        )
        .unwrap();
        confirm_cover(&state, &fixture.0, &request, &response[11]).unwrap();
        assert_eq!(dispatches.get(), 1);

        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.resize(16, 0);
        png.extend(500_u32.to_be_bytes());
        png.extend(750_u32.to_be_bytes());
        png.resize(64, 0);
        assert_eq!(validate_cover(png).unwrap().1, 2.0 / 3.0);

        let mut webp = b"RIFF0000WEBPVP8X00000000".to_vec();
        webp.extend([0xf3, 0x01, 0x00, 0xed, 0x02, 0x00]);
        webp.resize(64, 0);
        assert_eq!(validate_cover(webp).unwrap().1, 2.0 / 3.0);
    }

    #[test]
    fn cache_count_and_total_bytes_evict_obsolete_files_deterministically() {
        let fixture = Fixture::new();
        for index in 0..CACHE_MAX_FILES {
            fs::write(fixture.0.join(format!("tmdb-cover-{index:040x}.bin")), [0]).unwrap();
        }
        bound_cache(
            &fixture.0,
            &fixture
                .0
                .join(format!("tmdb-cover-{:040x}.bin", CACHE_MAX_FILES)),
            1,
            &HashSet::new(),
        )
        .unwrap();
        assert_eq!(cache_files(&fixture.0).len(), CACHE_MAX_FILES - 1);

        for (path, _, _) in cache_files(&fixture.0) {
            remove_cache_file(&path);
        }
        for index in 0..13 {
            let file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(fixture.0.join(format!("tmdb-cover-{index:040x}.bin")))
                .unwrap();
            file.set_len(20 * 1024 * 1024).unwrap();
        }
        bound_cache(
            &fixture.0,
            &fixture
                .0
                .join("tmdb-cover-ffffffffffffffffffffffffffffffffffffffff.bin"),
            16 * 1024 * 1024,
            &HashSet::new(),
        )
        .unwrap();
        let total = cache_files(&fixture.0)
            .iter()
            .map(|(_, length, _)| *length)
            .sum::<u64>();
        assert!(total + 16 * 1024 * 1024 <= CACHE_MAX_TOTAL_BYTES);
    }

    #[test]
    fn cache_admission_fails_when_deterministic_obsolete_eviction_cannot_complete() {
        let fixture = Fixture::new();
        let state = TmdbCoverState::default();
        let request = request(TmdbCoverCategory::Movie, TmdbCoverSurface::Discover);
        let response =
            resolve_cover_with(&state, &fixture.0, &request, || true, |_| Ok(jpeg())).unwrap();
        let blocked = fixture
            .0
            .join("tmdb-cover-0000000000000000000000000000000000000000.bin");
        fs::create_dir(&blocked).unwrap();
        for index in 1..CACHE_MAX_FILES {
            fs::write(fixture.0.join(format!("tmdb-cover-{index:040x}.bin")), [0]).unwrap();
        }
        assert_eq!(
            confirm_cover(&state, &fixture.0, &request, &response[11]),
            Err(TMDB_COVER_FAILED)
        );
        assert!(blocked.is_dir());
        assert!(!cache_path(&fixture.0, &request).exists());
        assert_eq!(cache_entries(&fixture.0).len(), CACHE_MAX_FILES);
    }

    #[test]
    fn interrupted_temporary_files_are_counted_and_removed_before_admission() {
        let fixture = Fixture::new();
        let state = TmdbCoverState::default();
        let mut retained_request = request(TmdbCoverCategory::Movie, TmdbCoverSurface::Discover);
        retained_request.tmdb_id = 702;
        retained_request.poster_path = Some("/retained.jpg".to_owned());
        let retained_response = resolve_cover_with(
            &state,
            &fixture.0,
            &retained_request,
            || true,
            |_| Ok(jpeg()),
        )
        .unwrap();
        confirm_cover(
            &state,
            &fixture.0,
            &retained_request,
            &retained_response[11],
        )
        .unwrap();
        let request = request(TmdbCoverCategory::Tv, TmdbCoverSurface::Discover);
        let response =
            resolve_cover_with(&state, &fixture.0, &request, || true, |_| Ok(jpeg())).unwrap();
        let stale = fixture
            .0
            .join(".tmdb-cover-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-7.tmp");
        let stale_file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&stale)
            .unwrap();
        stale_file.set_len(CACHE_MAX_TOTAL_BYTES).unwrap();

        confirm_cover(&state, &fixture.0, &request, &response[11]).unwrap();

        assert!(!stale.exists());
        assert!(cache_path(&fixture.0, &retained_request).exists());
        assert!(cache_path(&fixture.0, &request).exists());
        let retained = cache_entries(&fixture.0);
        assert_eq!(retained.len(), 2);
        assert!(retained.iter().map(|entry| entry.length).sum::<u64>() <= CACHE_MAX_TOTAL_BYTES);
    }

    fn stage_current_covers(
        state: &TmdbCoverState,
        directory: &Path,
        bytes: &[u8],
    ) -> Vec<(TmdbCoverRequest, String)> {
        (1..=MAX_CONCURRENT_OPERATIONS as u64)
            .map(|item| {
                let mut request = request(TmdbCoverCategory::Movie, TmdbCoverSurface::Discover);
                request.tmdb_id = 10_000 + item;
                request.poster_path = Some(format!("/current-{item}.jpg"));
                request.request_generation = 100 + item;
                let response =
                    resolve_cover_with(state, directory, &request, || true, |_| Ok(bytes.to_vec()))
                        .unwrap();
                (request, response[11].clone())
            })
            .collect()
    }

    fn confirm_simultaneously(
        state: &TmdbCoverState,
        directory: &Path,
        current: &[(TmdbCoverRequest, String)],
    ) {
        let barrier = Arc::new(std::sync::Barrier::new(current.len()));
        let workers = current
            .iter()
            .cloned()
            .map(|(request, authority_id)| {
                let state = state.clone();
                let directory = directory.to_owned();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    confirm_cover(&state, &directory, &request, &authority_id)
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap().unwrap();
        }
    }

    #[test]
    fn simultaneous_count_limit_admission_retains_every_current_visible_cover() {
        let fixture = Fixture::new();
        let state = TmdbCoverState::default();
        let current = stage_current_covers(&state, &fixture.0, &jpeg());
        for index in 0..CACHE_MAX_FILES {
            fs::write(fixture.0.join(format!("tmdb-cover-{index:040x}.bin")), [0]).unwrap();
        }

        confirm_simultaneously(&state, &fixture.0, &current);

        let files = cache_files(&fixture.0);
        assert_eq!(files.len(), CACHE_MAX_FILES);
        for (request, _) in &current {
            assert!(cache_path(&fixture.0, request).is_file());
        }
    }

    #[test]
    fn simultaneous_byte_limit_admission_retains_every_current_visible_cover() {
        let fixture = Fixture::new();
        let state = TmdbCoverState::default();
        let mut large_jpeg = jpeg();
        large_jpeg.resize(1024 * 1024, 0);
        let current = stage_current_covers(&state, &fixture.0, &large_jpeg);
        let current_bytes = current
            .iter()
            .map(|(request, _)| {
                let source = tmdb_poster_url(request.poster_path.as_deref().unwrap()).unwrap();
                cache_header(request, &source, 2.0 / 3.0, now_seconds(), large_jpeg.len()).len()
                    as u64
                    + large_jpeg.len() as u64
            })
            .sum::<u64>();
        let obsolete = fixture
            .0
            .join("tmdb-cover-ffffffffffffffffffffffffffffffffffffffff.bin");
        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&obsolete)
            .unwrap();
        file.set_len(CACHE_MAX_TOTAL_BYTES - current_bytes + 1)
            .unwrap();

        confirm_simultaneously(&state, &fixture.0, &current);

        let files = cache_files(&fixture.0);
        assert!(files.iter().map(|(_, length, _)| *length).sum::<u64>() <= CACHE_MAX_TOTAL_BYTES);
        assert!(!obsolete.exists());
        for (request, _) in &current {
            assert!(cache_path(&fixture.0, request).is_file());
        }
    }

    #[test]
    fn failed_decode_cleanup_is_reported_and_keeps_the_authority_unusable() {
        let fixture = Fixture::new();
        let state = TmdbCoverState::default();
        let request = request(TmdbCoverCategory::Movie, TmdbCoverSurface::Discover);
        let response =
            resolve_cover_with(&state, &fixture.0, &request, || true, |_| Ok(jpeg())).unwrap();
        fs::create_dir(cache_path(&fixture.0, &request)).unwrap();

        assert_eq!(
            invalidate_cover(&state, &fixture.0, &request),
            Err(TMDB_COVER_FAILED)
        );
        assert_eq!(
            fetch_cover(&state, &request, &response[11]),
            Err(TMDB_COVER_STALE)
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_cache_entry_is_removed_before_one_fresh_request() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new();
        let state = TmdbCoverState::default();
        let request = request(TmdbCoverCategory::Movie, TmdbCoverSurface::Discover);
        let target = fixture.0.join("outside.bin");
        fs::write(&target, jpeg()).unwrap();
        let path = cache_path(&fixture.0, &request);
        symlink(&target, &path).unwrap();
        let dispatches = Cell::new(0);
        let response = resolve_cover_with(
            &state,
            &fixture.0,
            &request,
            || true,
            |_| {
                dispatches.set(dispatches.get() + 1);
                Ok(jpeg())
            },
        )
        .unwrap();
        confirm_cover(&state, &fixture.0, &request, &response[11]).unwrap();
        assert_eq!(dispatches.get(), 1);
        assert!(!fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read(target).unwrap(), jpeg());
    }
}
