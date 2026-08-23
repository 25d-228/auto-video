use std::{
    collections::{HashMap, HashSet},
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::process::Command;

use crate::{
    fanza_catalog, javdb_catalog,
    vr_torrent::{hex_sha1, json_array, json_object, json_string, JsonParser, JsonValue},
    ProviderRequestError,
};

pub(crate) const LIBRARY_PRESENTATION_FAILED: &str = "library_presentation_failed";
pub(crate) const LIBRARY_PRESENTATION_STALE: &str = "library_presentation_stale";

const CACHE_VERSION: &str = "library-presentation-v1";
const CACHE_MAX_BYTES: u64 = 4 * 1024 * 1024;
const COVER_TTL_SECONDS: u64 = 24 * 60 * 60;
const METADATA_TTL_SECONDS: u64 = 365 * 24 * 60 * 60;
const COVER_MAX_BYTES: usize = 16 * 1024 * 1024;
const COVER_MIN_BYTES: usize = 6_000;
const RESPONSE_MAX_BYTES: usize = 4 * 1024 * 1024;
const DEFAULT_COVER_RATIO: f64 = 0.72;
const MAX_COVER_RATIO: f64 = 4.0;
const MAX_RETAINED_COVER_AUTHORITIES: usize = 8;
const MAX_RETAINED_COVER_REQUESTS: usize = 128;
const HTTP_STATUS_MARKER: &str = "\nAUTO_VIDEO_HTTP_STATUS:";
#[cfg(target_os = "macos")]
const HTTP_STATUS_WRITE_OUT: &str = "\nAUTO_VIDEO_HTTP_STATUS:%{http_code}";

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(crate) enum LibraryPresentationCategory {
    Adult,
    Vr,
}

impl LibraryPresentationCategory {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "adult" => Some(Self::Adult),
            "vr" => Some(Self::Vr),
            _ => None,
        }
    }

    pub(crate) fn value(self) -> &'static str {
        match self {
            Self::Adult => "adult",
            Self::Vr => "vr",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LibraryItemAuthority {
    pub category: LibraryPresentationCategory,
    pub identity: String,
    pub code: String,
}

#[derive(Clone, Debug, PartialEq)]
struct CoverSource {
    provider: &'static str,
    provider_id: String,
    url: String,
    aspect_ratio: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct PresentationMetadata {
    source: Option<String>,
    provider_id: Option<String>,
    title: Option<String>,
    date: Option<String>,
    runtime: Option<String>,
    cast: Vec<String>,
}

type CoverResolution = (
    Option<(CoverSource, Vec<u8>)>,
    Option<PresentationMetadata>,
    bool,
);

impl PresentationMetadata {
    fn is_ready(&self) -> bool {
        self.source.is_some()
            && self.provider_id.is_some()
            && (self.title.is_some()
                || self.date.is_some()
                || self.runtime.is_some()
                || !self.cast.is_empty())
    }
}

#[derive(Clone)]
struct CoverAuthority {
    category: LibraryPresentationCategory,
    code: String,
    item_identity: String,
    source: CoverSource,
    bytes: Option<Vec<u8>>,
    request_generation: u64,
    sequence: u64,
}

#[derive(Clone, Copy)]
struct CoverRequestAuthority {
    active: bool,
    generation: u64,
}

#[derive(Clone, Debug)]
struct CacheEntry {
    identity: String,
    category: LibraryPresentationCategory,
    code: String,
    cover_saved_at: u64,
    cover_state: &'static str,
    cover: Option<CoverSource>,
    metadata_saved_at: u64,
    metadata_state: &'static str,
    metadata: PresentationMetadata,
}

#[derive(Default)]
struct LibraryPresentationContext {
    covers: HashMap<String, CoverAuthority>,
    cover_requests: HashMap<(LibraryPresentationCategory, String), CoverRequestAuthority>,
    metadata_seeds: HashMap<String, PresentationMetadata>,
    next_cover_sequence: u64,
}

fn cover_request_matches(
    context: &LibraryPresentationContext,
    category: LibraryPresentationCategory,
    code: &str,
    request_generation: u64,
) -> bool {
    request_generation == 0
        || context
            .cover_requests
            .get(&(category, code.to_owned()))
            .is_some_and(|request| request.active && request.generation == request_generation)
}

#[derive(Clone, Default)]
pub(crate) struct LibraryPresentationState(Arc<Mutex<LibraryPresentationContext>>);

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn valid_date(value: &str) -> bool {
    value.len() == 10
        && value.as_bytes()[4] == b'-'
        && value.as_bytes()[7] == b'-'
        && value
            .bytes()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
}

fn image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 {
        return None;
    }
    if bytes.starts_with(&[0xff, 0xd8]) {
        let mut offset = 2;
        while offset + 9 < bytes.len() {
            if bytes[offset] != 0xff {
                offset += 1;
                continue;
            }
            let marker = bytes[offset + 1];
            if (0xc0..=0xcf).contains(&marker) && !matches!(marker, 0xc4 | 0xc8 | 0xcc) {
                let height = u16::from_be_bytes([bytes[offset + 5], bytes[offset + 6]]);
                let width = u16::from_be_bytes([bytes[offset + 7], bytes[offset + 8]]);
                return Some((u32::from(width), u32::from(height)));
            }
            let segment = usize::from(u16::from_be_bytes([
                *bytes.get(offset + 2)?,
                *bytes.get(offset + 3)?,
            ]));
            if segment < 2 {
                return None;
            }
            offset = offset.checked_add(segment + 2)?;
        }
        return None;
    }
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        return Some((
            u32::from_be_bytes(bytes[16..20].try_into().ok()?),
            u32::from_be_bytes(bytes[20..24].try_into().ok()?),
        ));
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        return match bytes.get(12..16)? {
            b"VP8X" if bytes.len() >= 30 => Some((
                1 + u32::from(bytes[24])
                    + (u32::from(bytes[25]) << 8)
                    + (u32::from(bytes[26]) << 16),
                1 + u32::from(bytes[27])
                    + (u32::from(bytes[28]) << 8)
                    + (u32::from(bytes[29]) << 16),
            )),
            b"VP8 " if bytes.len() >= 30 => Some((
                u32::from(u16::from_le_bytes([bytes[26], bytes[27]]) & 0x3fff),
                u32::from(u16::from_le_bytes([bytes[28], bytes[29]]) & 0x3fff),
            )),
            b"VP8L" if bytes.len() >= 25 => {
                let bits = u32::from_le_bytes([bytes[21], bytes[22], bytes[23], bytes[24]]);
                Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
            }
            _ => None,
        };
    }
    None
}

fn validate_cover(bytes: Vec<u8>) -> Result<(Vec<u8>, f64), ProviderRequestError> {
    if !(COVER_MIN_BYTES..=COVER_MAX_BYTES).contains(&bytes.len()) {
        return Err(ProviderRequestError::Provider);
    }
    let (width, height) = image_dimensions(&bytes).ok_or(ProviderRequestError::Provider)?;
    if width == 0 || height == 0 || (width == 590 && height == 800) {
        return Err(ProviderRequestError::Provider);
    }
    let ratio = f64::from(width) / f64::from(height);
    if !ratio.is_finite() || ratio <= 0.0 || ratio > MAX_COVER_RATIO {
        return Err(ProviderRequestError::Provider);
    }
    Ok((bytes, ratio))
}

fn metadata_from_javdb(item: &javdb_catalog::ExactLibraryItem) -> PresentationMetadata {
    PresentationMetadata {
        source: Some("JavDB".to_owned()),
        provider_id: Some(item.provider_item_id.clone()),
        title: item.title.clone(),
        date: item.release_date.clone().filter(|date| valid_date(date)),
        runtime: item.duration.clone(),
        cast: item.actors.clone(),
    }
}

fn exact_legacy_cover_url(document: &str, code: &str) -> Option<String> {
    let JsonValue::Object(root) = JsonParser::new(document).parse()? else {
        return None;
    };
    let content_id = root.get("content_id").and_then(|value| match value {
        JsonValue::String(value) => Some(value),
        _ => None,
    })?;
    let canonical = crate::vr_torrent::product_code_candidates(content_id)
        .into_iter()
        .map(|(candidate, _)| candidate)
        .collect::<HashSet<_>>();
    if canonical.len() != 1 || !canonical.contains(code) {
        return None;
    }
    let images = root.get("images").and_then(json_object)?;
    let jacket = images.get("jacket_image").and_then(json_object)?;
    ["large2", "large"]
        .into_iter()
        .find_map(|key| json_string(jacket, key))
        .filter(|url| valid_legacy_cover_url(url))
        .map(str::to_owned)
}

fn metadata_from_legacy(document: &str, code: &str) -> Option<PresentationMetadata> {
    let JsonValue::Object(root) = JsonParser::new(document).parse()? else {
        return None;
    };
    let content_id = root.get("content_id").and_then(|value| match value {
        JsonValue::String(value) => Some(value),
        _ => None,
    })?;
    let codes = crate::vr_torrent::product_code_candidates(content_id)
        .into_iter()
        .map(|(candidate, _)| candidate)
        .collect::<HashSet<_>>();
    if codes.len() != 1 || !codes.contains(code) {
        return None;
    }
    let title = ["title_ja", "title"]
        .into_iter()
        .find_map(|key| root.get(key))
        .and_then(|value| match value {
            JsonValue::String(value) if !value.trim().is_empty() => Some(value.clone()),
            _ => None,
        });
    let date = root.get("release_date").and_then(|value| match value {
        JsonValue::String(value) if valid_date(value) => Some(value.clone()),
        _ => None,
    });
    let runtime = root
        .get("runtime_mins")
        .or_else(|| root.get("runtime_minutes"))
        .and_then(|value| match value {
            JsonValue::String(value) | JsonValue::Number(value) => value.parse::<u64>().ok(),
            _ => None,
        })
        .filter(|minutes| *minutes > 0)
        .map(|minutes| format!("{minutes} min"));
    let cast = root
        .get("actresses")
        .and_then(json_array)
        .map(|values| {
            values
                .iter()
                .filter_map(json_object)
                .filter_map(|actor| {
                    ["name_kanji", "name"]
                        .into_iter()
                        .find_map(|key| actor.get(key))
                })
                .filter_map(|value| match value {
                    JsonValue::String(value) if !value.trim().is_empty() => Some(value.clone()),
                    _ => None,
                })
                .take(8)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let metadata = PresentationMetadata {
        source: Some("r18.dev".to_owned()),
        provider_id: Some(content_id.clone()),
        title,
        date,
        runtime,
        cast,
    };
    metadata.is_ready().then_some(metadata)
}

fn html_title(document: &str) -> Option<&str> {
    let lower = document.to_ascii_lowercase();
    let start = lower.find("<title>")? + "<title>".len();
    let end = lower[start..].find("</title>")? + start;
    Some(document[start..end].trim())
}

fn javdatabase_romanized_cast(document: &str, code: &str) -> Option<String> {
    let title = html_title(document)?;
    let suffix = " - JAV Database";
    let identity_and_cast = title.strip_suffix(suffix)?;
    let (title_and_code, cast) = identity_and_cast.rsplit_once(" - ")?;
    let codes = crate::vr_torrent::product_code_candidates(title_and_code)
        .into_iter()
        .map(|(candidate, _)| candidate)
        .collect::<HashSet<_>>();
    let cast = cast.trim();
    let folded_cast = cast.to_ascii_lowercase();
    (codes == HashSet::from([code.to_owned()])
        && !cast.is_empty()
        && cast.len() <= 16 * 1024
        && !cast.bytes().any(|byte| byte.is_ascii_control())
        && !matches!(folded_cast.as_str(), "jav" | "javdatabase")
        && !folded_cast.contains("jav database"))
    .then(|| cast.to_owned())
}

fn javdatabase_url(code: &str) -> String {
    format!(
        "https://www.javdatabase.com/movies/{}/",
        code.to_ascii_lowercase()
    )
}

fn valid_legacy_cover_url(url: &str) -> bool {
    if url
        .bytes()
        .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace() || byte == b'\\')
    {
        return false;
    }
    let Some(path) = url.strip_prefix("https://pics.dmm.co.jp/") else {
        return false;
    };
    !path.is_empty() && !path.contains(['?', '#', '@', ':'])
}

fn legacy_url(code: &str) -> String {
    format!("https://r18.dev/videos/vod/movies/detail/-/dvd_id={code}/json")
}

fn cover_source_valid(source: &CoverSource) -> bool {
    !source.provider_id.is_empty()
        && source.aspect_ratio.is_finite()
        && source.aspect_ratio > 0.0
        && source.aspect_ratio <= MAX_COVER_RATIO
        && match source.provider {
            "JavDB" => javdb_catalog::valid_library_cover_url(&source.url),
            "FANZA" => fanza_catalog::valid_library_cover_url(&source.url),
            "r18.dev" => valid_legacy_cover_url(&source.url),
            _ => false,
        }
}

fn fetch_source_bytes(source: &CoverSource) -> Result<Vec<u8>, ProviderRequestError> {
    match source.provider {
        "JavDB" => javdb_catalog::fetch_cover_bytes(&source.url),
        "FANZA" => fanza_catalog::fetch_cover_bytes(&source.url),
        "r18.dev" => fetch_legacy_image(&source.url),
        _ => Err(ProviderRequestError::Provider),
    }
}

fn resolve_cover_with(
    authority: &LibraryItemAuthority,
    mut javdb_document: impl FnMut(&str) -> Result<String, ProviderRequestError>,
    mut javdb_image: impl FnMut(&str) -> Result<Vec<u8>, ProviderRequestError>,
    fanza_document: impl FnOnce(&str) -> Result<String, ProviderRequestError>,
    fanza_image: impl FnOnce(&str) -> Result<Vec<u8>, ProviderRequestError>,
    mut legacy_document: impl FnMut(&str) -> Result<String, ProviderRequestError>,
    legacy_image: impl FnOnce(&str) -> Result<Vec<u8>, ProviderRequestError>,
) -> CoverResolution {
    let mut transient = false;
    let mut metadata = None;
    match javdb_catalog::fetch_exact_library_item_with(
        authority.category.value(),
        &authority.code,
        &mut javdb_document,
    ) {
        Ok(Some(item)) => {
            metadata = Some(metadata_from_javdb(&item));
            if let Some(url) = item.cover_url {
                match javdb_image(&url).and_then(validate_cover) {
                    Ok((bytes, ratio)) => {
                        return (
                            Some((
                                CoverSource {
                                    provider: "JavDB",
                                    provider_id: item.provider_item_id,
                                    url,
                                    aspect_ratio: ratio,
                                },
                                bytes,
                            )),
                            metadata,
                            transient,
                        );
                    }
                    Err(ProviderRequestError::SourceUnavailable) => {}
                    Err(_) => transient = true,
                }
            }
        }
        Ok(None) | Err(ProviderRequestError::SourceUnavailable) => {}
        Err(_) => transient = true,
    }

    match fanza_catalog::fetch_exact_library_item_with(
        authority.category.value(),
        &authority.code,
        fanza_document,
    ) {
        Ok(Some(item)) => {
            if let Some(url) = item.cover_url {
                match fanza_image(&url).and_then(validate_cover) {
                    Ok((bytes, ratio)) => {
                        return (
                            Some((
                                CoverSource {
                                    provider: "FANZA",
                                    provider_id: item.content_id,
                                    url,
                                    aspect_ratio: ratio,
                                },
                                bytes,
                            )),
                            metadata,
                            transient,
                        );
                    }
                    Err(ProviderRequestError::SourceUnavailable) => {}
                    Err(_) => transient = true,
                }
            }
        }
        Ok(None) | Err(ProviderRequestError::SourceUnavailable) => {}
        Err(_) => transient = true,
    }

    match legacy_document(&legacy_url(&authority.code)) {
        Ok(document) => {
            if metadata.is_none() {
                metadata = metadata_from_legacy(&document, &authority.code);
            }
            if let Some(url) = exact_legacy_cover_url(&document, &authority.code) {
                match legacy_image(&url).and_then(validate_cover) {
                    Ok((bytes, ratio)) => {
                        return (
                            Some((
                                CoverSource {
                                    provider: "r18.dev",
                                    provider_id: authority.code.clone(),
                                    url,
                                    aspect_ratio: ratio,
                                },
                                bytes,
                            )),
                            metadata,
                            transient,
                        );
                    }
                    Err(ProviderRequestError::SourceUnavailable) => {}
                    Err(_) => transient = true,
                }
            }
        }
        Err(ProviderRequestError::SourceUnavailable) => {}
        Err(_) => transient = true,
    }
    (None, metadata, transient)
}

fn resolve_metadata_with(
    authority: &LibraryItemAuthority,
    mut javdb_document: impl FnMut(&str) -> Result<String, ProviderRequestError>,
    mut legacy_document: impl FnMut(&str) -> Result<String, ProviderRequestError>,
) -> Result<Option<PresentationMetadata>, ProviderRequestError> {
    let mut transient = false;
    let mut accepted = None;
    match javdb_catalog::fetch_exact_library_item_with(
        authority.category.value(),
        &authority.code,
        &mut javdb_document,
    ) {
        Ok(Some(item)) => {
            let metadata = metadata_from_javdb(&item);
            accepted = metadata.is_ready().then_some(metadata);
        }
        Ok(None) | Err(ProviderRequestError::SourceUnavailable) => {}
        Err(_) => transient = true,
    }
    match legacy_document(&legacy_url(&authority.code)) {
        Ok(document) => {
            if let Some(metadata) = metadata_from_legacy(&document, &authority.code) {
                if let Some(current) = &mut accepted {
                    current.title = current.title.take().or(metadata.title);
                    current.date = current.date.take().or(metadata.date);
                    current.runtime = current.runtime.take().or(metadata.runtime);
                    if current.cast.is_empty() {
                        current.cast = metadata.cast;
                    }
                    current.source = Some("JavDB + r18.dev".to_owned());
                } else {
                    accepted = Some(metadata);
                }
            }
        }
        Err(ProviderRequestError::SourceUnavailable) => {}
        Err(_) => transient = true,
    }
    if accepted
        .as_ref()
        .is_none_or(|metadata| metadata.cast.is_empty())
    {
        match legacy_document(&javdatabase_url(&authority.code)) {
            Ok(document) => {
                if let Some(cast) = javdatabase_romanized_cast(&document, &authority.code) {
                    if let Some(metadata) = &mut accepted {
                        metadata.cast.push(cast);
                        metadata.source = Some(match metadata.source.as_deref() {
                            Some(source) => format!("{source} + JavDatabase"),
                            None => "JavDatabase".to_owned(),
                        });
                    } else {
                        accepted = Some(PresentationMetadata {
                            source: Some("JavDatabase".to_owned()),
                            provider_id: Some(authority.code.clone()),
                            cast: vec![cast],
                            ..PresentationMetadata::default()
                        });
                    }
                }
            }
            Err(ProviderRequestError::SourceUnavailable) => {}
            Err(_) => transient = true,
        }
    }
    if accepted.is_some() {
        return Ok(accepted);
    }
    if transient {
        Err(ProviderRequestError::Network)
    } else {
        Ok(None)
    }
}

fn encode_text(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(value.len() * 2);
    for byte in value.bytes() {
        encoded.push(HEX[usize::from(byte >> 4)] as char);
        encoded.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    encoded
}

fn decode_text(value: &str) -> Option<String> {
    if !value.len().is_multiple_of(2) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let bytes = value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok())
        .collect::<Option<Vec<_>>>()?;
    String::from_utf8(bytes).ok()
}

fn optional_text(value: &str) -> Option<String> {
    let value = decode_text(value)?;
    (!value.is_empty()).then_some(value)
}

fn cache_entry_valid(entry: &CacheEntry) -> bool {
    entry.identity.len() == 40
        && entry.identity.bytes().all(|byte| byte.is_ascii_hexdigit())
        && crate::vr_torrent::product_code_candidates(&entry.code)
            .into_iter()
            .map(|(code, _)| code)
            .collect::<HashSet<_>>()
            == HashSet::from([entry.code.clone()])
        && match (entry.cover_state, entry.cover.as_ref()) {
            ("ready", Some(source)) => cover_source_valid(source),
            ("missing", None) => true,
            _ => false,
        }
        && match entry.metadata_state {
            "ready" => entry.metadata.is_ready(),
            "missing" => entry.metadata == PresentationMetadata::default(),
            _ => false,
        }
}

fn read_cache(path: &Path) -> Result<Vec<CacheEntry>, ()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > CACHE_MAX_BYTES
    {
        return Err(());
    }
    let text = fs::read_to_string(path).map_err(|_| ())?;
    let mut lines = text.lines();
    if lines.next() != Some(CACHE_VERSION) {
        return Err(());
    }
    let mut identities = HashSet::new();
    let mut entries = Vec::new();
    for line in lines {
        let fields = line.split('\t').collect::<Vec<_>>();
        if fields.len() != 17 {
            return Err(());
        }
        let category = LibraryPresentationCategory::parse(fields[1]).ok_or(())?;
        let cover = if fields[4] == "ready" {
            Some(CoverSource {
                provider: match fields[5] {
                    "JavDB" => "JavDB",
                    "FANZA" => "FANZA",
                    "r18.dev" => "r18.dev",
                    _ => return Err(()),
                },
                provider_id: decode_text(fields[6]).ok_or(())?,
                url: decode_text(fields[7]).ok_or(())?,
                aspect_ratio: fields[8].parse().map_err(|_| ())?,
            })
        } else {
            None
        };
        let metadata = PresentationMetadata {
            source: optional_text(fields[11]),
            provider_id: optional_text(fields[12]),
            title: optional_text(fields[13]),
            date: optional_text(fields[14]),
            runtime: optional_text(fields[15]),
            cast: fields[16]
                .split(',')
                .filter(|value| !value.is_empty())
                .map(|value| decode_text(value).ok_or(()))
                .collect::<Result<Vec<_>, _>>()?,
        };
        let entry = CacheEntry {
            identity: fields[0].to_owned(),
            category,
            code: decode_text(fields[2]).ok_or(())?,
            cover_saved_at: fields[3].parse().map_err(|_| ())?,
            cover_state: match fields[4] {
                "ready" => "ready",
                "missing" => "missing",
                _ => return Err(()),
            },
            cover,
            metadata_saved_at: fields[9].parse().map_err(|_| ())?,
            metadata_state: match fields[10] {
                "ready" => "ready",
                "missing" => "missing",
                _ => return Err(()),
            },
            metadata,
        };
        if !identities.insert(entry.identity.clone()) || !cache_entry_valid(&entry) {
            return Err(());
        }
        entries.push(entry);
    }
    Ok(entries)
}

fn write_cache(path: &Path, entries: &[CacheEntry]) -> Result<(), ()> {
    let parent = path.parent().ok_or(())?;
    fs::create_dir_all(parent).map_err(|_| ())?;
    let replacement = path.with_extension("replacement");
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&replacement).map_err(|_| ())?;
    writeln!(file, "{CACHE_VERSION}").map_err(|_| ())?;
    for entry in entries {
        let cover = entry.cover.as_ref();
        let fields = [
            entry.identity.clone(),
            entry.category.value().to_owned(),
            encode_text(&entry.code),
            entry.cover_saved_at.to_string(),
            entry.cover_state.to_owned(),
            cover.map_or("", |cover| cover.provider).to_owned(),
            encode_text(cover.map_or("", |cover| &cover.provider_id)),
            encode_text(cover.map_or("", |cover| &cover.url)),
            cover
                .map_or(DEFAULT_COVER_RATIO, |cover| cover.aspect_ratio)
                .to_string(),
            entry.metadata_saved_at.to_string(),
            entry.metadata_state.to_owned(),
            encode_text(entry.metadata.source.as_deref().unwrap_or("")),
            encode_text(entry.metadata.provider_id.as_deref().unwrap_or("")),
            encode_text(entry.metadata.title.as_deref().unwrap_or("")),
            encode_text(entry.metadata.date.as_deref().unwrap_or("")),
            encode_text(entry.metadata.runtime.as_deref().unwrap_or("")),
            entry
                .metadata
                .cast
                .iter()
                .map(|value| encode_text(value))
                .collect::<Vec<_>>()
                .join(","),
        ];
        writeln!(file, "{}", fields.join("\t")).map_err(|_| ())?;
    }
    file.sync_all().map_err(|_| ())?;
    #[cfg(target_os = "windows")]
    if path.exists() {
        fs::remove_file(path).map_err(|_| ())?;
    }
    fs::rename(replacement, path).map_err(|_| ())
}

fn load_cache(path: &Path) -> Vec<CacheEntry> {
    read_cache(path).unwrap_or_default()
}

fn merge_cache_entry(path: &Path, entry: CacheEntry) -> Result<(), ()> {
    let mut entries = load_cache(path);
    entries.retain(|current| current.identity != entry.identity);
    entries.push(entry);
    entries.sort_by(|left, right| left.identity.cmp(&right.identity));
    write_cache(path, &entries)
}

fn cover_response(
    state: &LibraryPresentationState,
    authority: &LibraryItemAuthority,
    request_generation: u64,
    cover_state: &'static str,
    cover: Option<CoverSource>,
    bytes: Option<Vec<u8>>,
) -> Result<Vec<String>, &'static str> {
    let mut cover_authority_id = None;
    if let Some(source) = &cover {
        let mut context = state.0.lock().map_err(|_| LIBRARY_PRESENTATION_FAILED)?;
        if !cover_request_matches(
            &context,
            authority.category,
            &authority.code,
            request_generation,
        ) {
            return Err(LIBRARY_PRESENTATION_STALE);
        }
        context.covers.retain(|_, retained| {
            retained.category != authority.category || retained.code != authority.code
        });
        context.next_cover_sequence = context.next_cover_sequence.wrapping_add(1);
        let sequence = context.next_cover_sequence;
        let id = format!(
            "library-cover-{}",
            hex_sha1(
                format!(
                    "{}\0{}\0{request_generation}",
                    authority.identity, source.url
                )
                .as_bytes(),
            )
        );
        context.covers.insert(
            id.clone(),
            CoverAuthority {
                category: authority.category,
                code: authority.code.clone(),
                item_identity: authority.identity.clone(),
                source: source.clone(),
                bytes,
                request_generation,
                sequence,
            },
        );
        cover_authority_id = Some(id);
        while context.covers.len() > MAX_RETAINED_COVER_AUTHORITIES {
            let Some(oldest) = context
                .covers
                .iter()
                .min_by_key(|(_, retained)| retained.sequence)
                .map(|(id, _)| id.clone())
            else {
                break;
            };
            context.covers.remove(&oldest);
        }
    }
    Ok(vec![
        "library-cover-v1".to_owned(),
        authority.category.value().to_owned(),
        cover_state.to_owned(),
        cover
            .as_ref()
            .map_or_else(String::new, |cover| cover.provider.to_owned()),
        cover
            .as_ref()
            .map_or_else(String::new, |cover| cover.provider_id.clone()),
        cover_authority_id.unwrap_or_default(),
        cover
            .as_ref()
            .map_or(DEFAULT_COVER_RATIO, |cover| cover.aspect_ratio)
            .to_string(),
    ])
}

pub(crate) fn resolve_cover(
    state: &LibraryPresentationState,
    cache_path: &Path,
    authority: &LibraryItemAuthority,
    request_generation: u64,
    is_current: impl Fn() -> bool,
) -> Result<Vec<String>, &'static str> {
    resolve_cover_at_with_request(
        state,
        cache_path,
        authority,
        request_generation,
        now_seconds(),
        is_current,
        || {
            resolve_cover_with(
                authority,
                javdb_catalog::fetch_api_document,
                javdb_catalog::fetch_cover_bytes,
                fanza_catalog::fetch_graphql_document,
                fanza_catalog::fetch_cover_bytes,
                fetch_legacy_document,
                fetch_legacy_image,
            )
        },
    )
}

#[cfg(test)]
fn resolve_cover_at_with(
    state: &LibraryPresentationState,
    cache_path: &Path,
    authority: &LibraryItemAuthority,
    now: u64,
    is_current: impl Fn() -> bool,
    resolve: impl FnOnce() -> CoverResolution,
) -> Result<Vec<String>, &'static str> {
    resolve_cover_at_with_request(state, cache_path, authority, 0, now, is_current, resolve)
}

fn resolve_cover_at_with_request(
    state: &LibraryPresentationState,
    cache_path: &Path,
    authority: &LibraryItemAuthority,
    request_generation: u64,
    now: u64,
    is_current: impl Fn() -> bool,
    resolve: impl FnOnce() -> CoverResolution,
) -> Result<Vec<String>, &'static str> {
    let cache = {
        let _guard = state.0.lock().map_err(|_| LIBRARY_PRESENTATION_FAILED)?;
        load_cache(cache_path)
    };
    if let Some(entry) = cache.iter().find(|entry| {
        entry.identity == authority.identity
            && entry.category == authority.category
            && entry.code == authority.code
            && entry.cover_saved_at > 0
            && entry.cover_saved_at <= now
            && now.saturating_sub(entry.cover_saved_at) <= COVER_TTL_SECONDS
    }) {
        if !is_current() {
            return Err(LIBRARY_PRESENTATION_STALE);
        }
        return cover_response(
            state,
            authority,
            request_generation,
            entry.cover_state,
            entry.cover.clone(),
            None,
        );
    }

    let (cover, metadata_seed, transient) = resolve();
    if !is_current() {
        return Err(LIBRARY_PRESENTATION_STALE);
    }
    if let Some(metadata) = metadata_seed {
        if let Ok(mut context) = state.0.lock() {
            if !cover_request_matches(
                &context,
                authority.category,
                &authority.code,
                request_generation,
            ) {
                return Err(LIBRARY_PRESENTATION_STALE);
            }
            context
                .metadata_seeds
                .insert(authority.identity.clone(), metadata);
        }
    }
    let (cover_state, source, bytes) = match cover {
        Some((source, bytes)) => ("ready", Some(source), Some(bytes)),
        None if transient => ("unavailable", None, None),
        None => ("missing", None, None),
    };
    if cover_state != "unavailable" {
        let _guard = state.0.lock().map_err(|_| LIBRARY_PRESENTATION_FAILED)?;
        if !cover_request_matches(
            &_guard,
            authority.category,
            &authority.code,
            request_generation,
        ) {
            return Err(LIBRARY_PRESENTATION_STALE);
        }
        let mut entry = cache
            .into_iter()
            .find(|entry| entry.identity == authority.identity)
            .unwrap_or(CacheEntry {
                identity: authority.identity.clone(),
                category: authority.category,
                code: authority.code.clone(),
                cover_saved_at: 0,
                cover_state: "missing",
                cover: None,
                metadata_saved_at: 0,
                metadata_state: "missing",
                metadata: PresentationMetadata::default(),
            });
        entry.category = authority.category;
        entry.code = authority.code.clone();
        entry.cover_saved_at = now;
        entry.cover_state = cover_state;
        entry.cover = source.clone();
        merge_cache_entry(cache_path, entry).map_err(|_| LIBRARY_PRESENTATION_FAILED)?;
    }
    cover_response(
        state,
        authority,
        request_generation,
        cover_state,
        source,
        bytes,
    )
}

pub(crate) fn resolve_metadata(
    state: &LibraryPresentationState,
    cache_path: &Path,
    authority: &LibraryItemAuthority,
    is_current: impl Fn() -> bool,
) -> Result<Vec<String>, &'static str> {
    resolve_metadata_at_with(
        state,
        cache_path,
        authority,
        now_seconds(),
        is_current,
        || {
            let seed = state
                .0
                .lock()
                .map_err(|_| ProviderRequestError::Provider)?
                .metadata_seeds
                .remove(&authority.identity);
            match seed.filter(PresentationMetadata::is_ready) {
                Some(seed) => Ok(Some(seed)),
                None => resolve_metadata_with(
                    authority,
                    javdb_catalog::fetch_api_document,
                    fetch_legacy_document,
                ),
            }
        },
    )
}

fn resolve_metadata_at_with(
    state: &LibraryPresentationState,
    cache_path: &Path,
    authority: &LibraryItemAuthority,
    now: u64,
    is_current: impl Fn() -> bool,
    resolve: impl FnOnce() -> Result<Option<PresentationMetadata>, ProviderRequestError>,
) -> Result<Vec<String>, &'static str> {
    let cache = {
        let _guard = state.0.lock().map_err(|_| LIBRARY_PRESENTATION_FAILED)?;
        load_cache(cache_path)
    };
    if let Some(entry) = cache.iter().find(|entry| {
        entry.identity == authority.identity
            && entry.category == authority.category
            && entry.code == authority.code
            && entry.metadata_saved_at <= now
            && now.saturating_sub(entry.metadata_saved_at) <= METADATA_TTL_SECONDS
    }) {
        if !is_current() {
            return Err(LIBRARY_PRESENTATION_STALE);
        }
        return Ok(metadata_response(
            authority,
            if entry.metadata_state == "ready" {
                "automatic"
            } else {
                "local-only"
            },
            &entry.metadata,
        ));
    }
    let resolved = resolve();
    if !is_current() {
        return Err(LIBRARY_PRESENTATION_STALE);
    }
    let (metadata_state, metadata) = match resolved {
        Ok(Some(metadata)) => ("automatic", metadata),
        Ok(None) => ("local-only", PresentationMetadata::default()),
        Err(_) => {
            return Ok(metadata_response(
                authority,
                "unavailable",
                &PresentationMetadata::default(),
            ))
        }
    };
    if metadata_state == "automatic" {
        let _guard = state.0.lock().map_err(|_| LIBRARY_PRESENTATION_FAILED)?;
        let mut entry = cache
            .into_iter()
            .find(|entry| entry.identity == authority.identity)
            .unwrap_or(CacheEntry {
                identity: authority.identity.clone(),
                category: authority.category,
                code: authority.code.clone(),
                cover_saved_at: 0,
                cover_state: "missing",
                cover: None,
                metadata_saved_at: 0,
                metadata_state: "missing",
                metadata: PresentationMetadata::default(),
            });
        entry.category = authority.category;
        entry.code = authority.code.clone();
        entry.metadata_saved_at = now;
        entry.metadata_state = "ready";
        entry.metadata = metadata.clone();
        let _ = merge_cache_entry(cache_path, entry);
    }
    Ok(metadata_response(authority, metadata_state, &metadata))
}

fn metadata_response(
    authority: &LibraryItemAuthority,
    state: &str,
    metadata: &PresentationMetadata,
) -> Vec<String> {
    let mut response = vec![
        "library-metadata-v1".to_owned(),
        authority.category.value().to_owned(),
        state.to_owned(),
        metadata.source.clone().unwrap_or_default(),
        metadata.provider_id.clone().unwrap_or_default(),
        metadata.title.clone().unwrap_or_default(),
        metadata.date.clone().unwrap_or_default(),
        metadata.runtime.clone().unwrap_or_default(),
        metadata.cast.len().to_string(),
    ];
    response.extend(metadata.cast.clone());
    response
}

pub(crate) fn fetch_cover(
    state: &LibraryPresentationState,
    authority: &LibraryItemAuthority,
    cover_authority_id: &str,
) -> Result<Vec<u8>, &'static str> {
    fetch_cover_with(state, authority, cover_authority_id, fetch_source_bytes)
}

fn fetch_cover_with(
    state: &LibraryPresentationState,
    authority: &LibraryItemAuthority,
    cover_authority_id: &str,
    fetch: impl FnOnce(&CoverSource) -> Result<Vec<u8>, ProviderRequestError>,
) -> Result<Vec<u8>, &'static str> {
    let cover = {
        let mut context = state.0.lock().map_err(|_| LIBRARY_PRESENTATION_STALE)?;
        let matches = context.covers.get(cover_authority_id).is_some_and(|cover| {
            cover.category == authority.category
                && cover.code == authority.code
                && cover.item_identity == authority.identity
        });
        matches
            .then(|| context.covers.remove(cover_authority_id))
            .flatten()
            .ok_or(LIBRARY_PRESENTATION_STALE)?
    };
    let bytes = match cover.bytes {
        Some(bytes) => validate_cover(bytes),
        None => fetch(&cover.source).and_then(validate_cover),
    }
    .map(|(bytes, _)| bytes)
    .map_err(|_| LIBRARY_PRESENTATION_FAILED)?;
    Ok(bytes)
}

pub(crate) fn begin_cover_request(
    state: &LibraryPresentationState,
    category: LibraryPresentationCategory,
    code: &str,
    request_generation: u64,
) -> Result<(), &'static str> {
    if request_generation == 0 {
        return Err(LIBRARY_PRESENTATION_STALE);
    }
    let key = (category, code.to_owned());
    let mut context = state.0.lock().map_err(|_| LIBRARY_PRESENTATION_STALE)?;
    if context
        .cover_requests
        .get(&key)
        .is_some_and(|request| request_generation <= request.generation)
    {
        return Err(LIBRARY_PRESENTATION_STALE);
    }
    context
        .covers
        .retain(|_, cover| cover.category != category || cover.code != code);
    context.cover_requests.insert(
        key,
        CoverRequestAuthority {
            active: true,
            generation: request_generation,
        },
    );
    while context.cover_requests.len() > MAX_RETAINED_COVER_REQUESTS {
        let oldest_inactive = context
            .cover_requests
            .iter()
            .filter(|(_, request)| !request.active)
            .min_by_key(|(_, request)| request.generation)
            .map(|(key, _)| key.clone());
        let Some(oldest) = oldest_inactive.or_else(|| {
            context
                .cover_requests
                .iter()
                .min_by_key(|(_, request)| request.generation)
                .map(|(key, _)| key.clone())
        }) else {
            break;
        };
        context
            .covers
            .retain(|_, cover| cover.category != oldest.0 || cover.code != oldest.1);
        context.cover_requests.remove(&oldest);
    }
    Ok(())
}

pub(crate) fn cover_request_is_current(
    state: &LibraryPresentationState,
    category: LibraryPresentationCategory,
    code: &str,
    request_generation: u64,
) -> bool {
    state
        .0
        .lock()
        .is_ok_and(|context| cover_request_matches(&context, category, code, request_generation))
}

pub(crate) fn cancel_cover_request(
    state: &LibraryPresentationState,
    category: LibraryPresentationCategory,
    code: &str,
    request_generation: u64,
) -> Result<(), &'static str> {
    let mut context = state.0.lock().map_err(|_| LIBRARY_PRESENTATION_STALE)?;
    let key = (category, code.to_owned());
    if let Some(request) = context.cover_requests.get_mut(&key) {
        if request.generation == request_generation {
            request.active = false;
            context.covers.retain(|_, cover| {
                cover.category != category
                    || cover.code != code
                    || cover.request_generation != request_generation
            });
        }
    }
    Ok(())
}

pub(crate) fn invalidate_cover(
    state: &LibraryPresentationState,
    cache_path: &Path,
    authority: &LibraryItemAuthority,
    request_generation: u64,
    cover_authority_id: &str,
) -> Result<(), &'static str> {
    let mut context = state.0.lock().map_err(|_| LIBRARY_PRESENTATION_STALE)?;
    if !cover_request_matches(
        &context,
        authority.category,
        &authority.code,
        request_generation,
    ) {
        return Err(LIBRARY_PRESENTATION_STALE);
    }
    if context.covers.get(cover_authority_id).is_some_and(|cover| {
        cover.category == authority.category
            && cover.code == authority.code
            && cover.item_identity == authority.identity
    }) {
        context.covers.remove(cover_authority_id);
    }
    let mut entries = load_cache(cache_path);
    let entry = entries
        .iter_mut()
        .find(|entry| {
            entry.identity == authority.identity
                && entry.category == authority.category
                && entry.code == authority.code
        })
        .ok_or(LIBRARY_PRESENTATION_STALE)?;
    let source = entry
        .cover
        .as_ref()
        .filter(|_| entry.cover_state == "ready")
        .ok_or(LIBRARY_PRESENTATION_STALE)?;
    let expected_id = format!(
        "library-cover-{}",
        hex_sha1(
            format!(
                "{}\0{}\0{request_generation}",
                authority.identity, source.url
            )
            .as_bytes()
        )
    );
    if expected_id != cover_authority_id {
        return Err(LIBRARY_PRESENTATION_STALE);
    }
    entry.cover_saved_at = 0;
    entry.cover_state = "missing";
    entry.cover = None;
    write_cache(cache_path, &entries).map_err(|_| LIBRARY_PRESENTATION_FAILED)
}

#[cfg(target_os = "macos")]
fn fetch_legacy_document(url: &str) -> Result<String, ProviderRequestError> {
    if !url.starts_with("https://r18.dev/videos/vod/movies/detail/-/")
        && !url.starts_with("https://www.javdatabase.com/movies/")
    {
        return Err(ProviderRequestError::Provider);
    }
    let output = Command::new("/usr/bin/curl")
        .args([
            "--silent",
            "--show-error",
            "--connect-timeout",
            "10",
            "--max-time",
            "20",
            "--max-redirs",
            "0",
            "--max-filesize",
            &RESPONSE_MAX_BYTES.to_string(),
            "--header",
            "Accept: application/json",
            "--write-out",
            HTTP_STATUS_WRITE_OUT,
            url,
        ])
        .output()
        .map_err(|_| ProviderRequestError::Network)?;
    if !output.status.success() {
        return Err(ProviderRequestError::Network);
    }
    String::from_utf8(parse_http_response(&output.stdout, RESPONSE_MAX_BYTES)?)
        .map_err(|_| ProviderRequestError::Provider)
}

#[cfg(target_os = "macos")]
fn fetch_legacy_image(url: &str) -> Result<Vec<u8>, ProviderRequestError> {
    if !valid_legacy_cover_url(url) {
        return Err(ProviderRequestError::Provider);
    }
    let output = Command::new("/usr/bin/curl")
        .args([
            "--silent",
            "--show-error",
            "--connect-timeout",
            "10",
            "--max-time",
            "20",
            "--max-redirs",
            "0",
            "--max-filesize",
            &COVER_MAX_BYTES.to_string(),
            "--header",
            "Referer: https://www.dmm.co.jp/",
            "--write-out",
            HTTP_STATUS_WRITE_OUT,
            url,
        ])
        .output()
        .map_err(|_| ProviderRequestError::Network)?;
    if !output.status.success() {
        return Err(ProviderRequestError::Network);
    }
    parse_http_response(&output.stdout, COVER_MAX_BYTES)
}

#[cfg(target_os = "windows")]
fn fetch_legacy_document(url: &str) -> Result<String, ProviderRequestError> {
    let bytes = fetch_legacy_with_powershell(url, false)?;
    String::from_utf8(bytes).map_err(|_| ProviderRequestError::Provider)
}

#[cfg(target_os = "windows")]
fn fetch_legacy_image(url: &str) -> Result<Vec<u8>, ProviderRequestError> {
    if !valid_legacy_cover_url(url) {
        return Err(ProviderRequestError::Provider);
    }
    fetch_legacy_with_powershell(url, true)
}

#[cfg(target_os = "windows")]
fn fetch_legacy_with_powershell(url: &str, image: bool) -> Result<Vec<u8>, ProviderRequestError> {
    if (!image
        && !url.starts_with("https://r18.dev/videos/vod/movies/detail/-/")
        && !url.starts_with("https://www.javdatabase.com/movies/"))
        || url.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(ProviderRequestError::Provider);
    }
    let maximum = if image {
        COVER_MAX_BYTES
    } else {
        RESPONSE_MAX_BYTES
    };
    let script = r#"$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';Add-Type -AssemblyName System.Net.Http;$h=[System.Net.Http.HttpClientHandler]::new();$h.AllowAutoRedirect=$false;$c=[System.Net.Http.HttpClient]::new($h);$d=[System.Threading.CancellationTokenSource]::new();$d.CancelAfter(20000);try{$r=[System.Net.Http.HttpRequestMessage]::new([System.Net.Http.HttpMethod]::Get,$env:AUTO_VIDEO_LIBRARY_URL);if($env:AUTO_VIDEO_LIBRARY_IMAGE -eq '1'){$r.Headers.Referrer=[Uri]'https://www.dmm.co.jp/'}else{$r.Headers.Accept.ParseAdd('application/json')};$s=$c.SendAsync($r,[System.Net.Http.HttpCompletionOption]::ResponseHeadersRead,$d.Token).GetAwaiter().GetResult();$i=$s.Content.ReadAsStreamAsync().GetAwaiter().GetResult();$m=[System.IO.MemoryStream]::new();$b=[byte[]]::new(65536);while(($n=$i.ReadAsync($b,0,$b.Length,$d.Token).GetAwaiter().GetResult()) -gt 0){if($m.Length+$n -gt [int]$env:AUTO_VIDEO_LIBRARY_MAX){[Environment]::Exit(63)};$m.Write($b,0,$n)};$o=[Console]::OpenStandardOutput();$v=$m.ToArray();$o.Write($v,0,$v.Length);$x=[Text.Encoding]::UTF8.GetBytes("`nAUTO_VIDEO_HTTP_STATUS:"+[int]$s.StatusCode);$o.Write($x,0,$x.Length)}catch{[Environment]::Exit(28)}finally{$d.Dispose();$c.Dispose();$h.Dispose()}"#;
    let output = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ])
        .env("AUTO_VIDEO_LIBRARY_URL", url)
        .env("AUTO_VIDEO_LIBRARY_IMAGE", if image { "1" } else { "0" })
        .env("AUTO_VIDEO_LIBRARY_MAX", maximum.to_string())
        .output()
        .map_err(|_| ProviderRequestError::Network)?;
    if !output.status.success() {
        return Err(ProviderRequestError::Network);
    }
    parse_http_response(&output.stdout, maximum)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn fetch_legacy_document(_url: &str) -> Result<String, ProviderRequestError> {
    Err(ProviderRequestError::SourceUnavailable)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn fetch_legacy_image(_url: &str) -> Result<Vec<u8>, ProviderRequestError> {
    Err(ProviderRequestError::SourceUnavailable)
}

fn parse_http_response(output: &[u8], maximum: usize) -> Result<Vec<u8>, ProviderRequestError> {
    let marker = output
        .windows(HTTP_STATUS_MARKER.len())
        .rposition(|window| window == HTTP_STATUS_MARKER.as_bytes())
        .ok_or(ProviderRequestError::Provider)?;
    let status = std::str::from_utf8(&output[marker + HTTP_STATUS_MARKER.len()..])
        .map_err(|_| ProviderRequestError::Provider)?
        .trim()
        .parse::<u16>()
        .map_err(|_| ProviderRequestError::Provider)?;
    let body = &output[..marker];
    match status {
        200..=299 if !body.is_empty() && body.len() <= maximum => Ok(body.to_vec()),
        404 | 410 | 451 => Err(ProviderRequestError::SourceUnavailable),
        0 => Err(ProviderRequestError::Network),
        _ => Err(ProviderRequestError::Provider),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::Cell,
        sync::{Arc, Barrier},
        thread,
    };

    struct CacheFixture {
        directory: std::path::PathBuf,
        path: std::path::PathBuf,
    }

    impl CacheFixture {
        fn new(label: &str) -> Self {
            let directory = std::env::temp_dir().join(format!(
                "auto-video-library-presentation-{label}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("test time must be available")
                    .as_nanos()
            ));
            fs::create_dir_all(&directory).expect("cache fixture must be created");
            let path = directory.join("presentation-cache");
            Self { directory, path }
        }
    }

    impl Drop for CacheFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }

    fn jpeg(width: u16, height: u16) -> Vec<u8> {
        let mut bytes = vec![0xff, 0xd8, 0xff, 0xc0, 0, 17, 8];
        bytes.extend(height.to_be_bytes());
        bytes.extend(width.to_be_bytes());
        bytes.resize(COVER_MIN_BYTES, 0);
        bytes
    }

    fn authority(category: LibraryPresentationCategory) -> LibraryItemAuthority {
        LibraryItemAuthority {
            category,
            identity: "a".repeat(40),
            code: if category == LibraryPresentationCategory::Vr {
                "MDVR-419".to_owned()
            } else {
                "ADLT-123".to_owned()
            },
        }
    }

    fn cover(provider_id: &str, ratio: f64) -> CoverSource {
        CoverSource {
            provider: "JavDB",
            provider_id: provider_id.to_owned(),
            url: format!("https://tp.cmastd.com/{provider_id}.jpg"),
            aspect_ratio: ratio,
        }
    }

    fn assert_invalid_cache_refreshes(path: &Path, authority: &LibraryItemAuthority) {
        let refreshed = Cell::new(false);
        resolve_cover_at_with(
            &LibraryPresentationState::default(),
            path,
            authority,
            10,
            || true,
            || {
                refreshed.set(true);
                (None, None, false)
            },
        )
        .expect("an invalid cache must permit provider resolution");
        assert!(refreshed.get());
    }

    fn javdb_listing(code: &str) -> String {
        format!(
            r#"{{"success":1,"data":{{"movies":[{{"id":"item","number":"{code}","title":"Provider title","tags":[],"cover_url":"https://tp.cmastd.com/exact.jpg"}}]}}}}"#
        )
    }

    fn javdb_detail(code: &str, category: LibraryPresentationCategory) -> String {
        let tags = if category == LibraryPresentationCategory::Vr {
            r#"[{"id":"212","name":"VR"}]"#
        } else {
            "[]"
        };
        format!(
            r#"{{"success":1,"data":{{"movie":{{"id":"item","number":"{code}","title":"Provider title","release_date":"2024-01-02","duration":90,"actors":[{{"name":"Actor"}}],"tags":{tags},"cover_url":"https://tp.cmastd.com/exact.jpg"}}}}}}"#
        )
    }

    #[test]
    fn exact_cover_order_is_javdb_then_fanza_then_bounded_legacy() {
        let authority = authority(LibraryPresentationCategory::Adult);
        let events = std::cell::RefCell::new(Vec::new());
        let (cover, metadata, transient) = resolve_cover_with(
            &authority,
            |url| {
                events.borrow_mut().push(format!("javdb:{url}"));
                if url.contains("search") {
                    Ok(javdb_listing(&authority.code))
                } else {
                    Ok(javdb_detail(&authority.code, authority.category))
                }
            },
            |url| {
                events.borrow_mut().push(format!("javdb-image:{url}"));
                Err(ProviderRequestError::Network)
            },
            |body| {
                events.borrow_mut().push(format!("fanza:{body}"));
                Ok(r#"{"data":{"c0":{"id":"adlt00123","contentType":"TWO_DIMENSION","title":"Exact","packageImage":{"largeUrl":"https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/adlt00123/adlt00123pl.jpg"}}}}"#.to_owned())
            },
            |url| {
                events.borrow_mut().push(format!("fanza-image:{url}"));
                Ok(jpeg(600, 800))
            },
            |url| {
                events.borrow_mut().push(format!("legacy:{url}"));
                panic!("legacy must not run after FANZA succeeds")
            },
            |_| panic!("legacy image must not run"),
        );
        assert_eq!(
            cover.as_ref().map(|(source, _)| source.provider),
            Some("FANZA")
        );
        assert!(metadata.is_some());
        assert!(transient);
        let events = events.into_inner();
        assert!(events[0].starts_with("javdb:"));
        assert!(
            events
                .iter()
                .position(|event| event.starts_with("fanza:"))
                .unwrap()
                > events
                    .iter()
                    .position(|event| event.starts_with("javdb-image:"))
                    .unwrap()
        );
    }

    #[test]
    fn exact_javdb_vr_cover_preserves_wide_ratio_and_never_dispatches_fanza() {
        let authority = authority(LibraryPresentationCategory::Vr);
        let (cover, _, transient) = resolve_cover_with(
            &authority,
            |url| {
                if url.contains("search") {
                    Ok(javdb_listing(&authority.code))
                } else {
                    Ok(javdb_detail(&authority.code, authority.category))
                }
            },
            |_| Ok(jpeg(1600, 900)),
            |_| panic!("FANZA must not run after JavDB cover success"),
            |_| panic!("FANZA image must not run"),
            |_| panic!("legacy must not run"),
            |_| panic!("legacy image must not run"),
        );
        let source = cover.expect("VR cover must resolve").0;
        assert_eq!(source.provider, "JavDB");
        assert!((source.aspect_ratio - 16.0 / 9.0).abs() < f64::EPSILON);
        assert!(!transient);
    }

    #[test]
    fn exact_3dsvr_identity_reaches_javdb_and_then_exact_category_fanza_fallback() {
        let authority = LibraryItemAuthority {
            category: LibraryPresentationCategory::Vr,
            identity: "3".repeat(40),
            code: "3DSVR-1871".to_owned(),
        };
        let javdb = resolve_cover_with(
            &authority,
            |url| {
                if url.contains("search") {
                    Ok(javdb_listing("3DSVR-01871"))
                } else {
                    Ok(javdb_detail("3DSVR-01871", authority.category))
                }
            },
            |_| Ok(jpeg(600, 800)),
            |_| panic!("FANZA must not run after exact JavDB success"),
            |_| panic!("FANZA image must not run"),
            |_| panic!("legacy must not run"),
            |_| panic!("legacy image must not run"),
        )
        .0
        .expect("JavDB must accept canonical 3DSVR padding");
        assert_eq!(javdb.0.provider, "JavDB");

        let fanza = resolve_cover_with(
            &authority,
            |url| {
                if url.contains("search") {
                    Ok(r#"{"success":1,"data":{"movies":[{"id":"item","number":"3DSVR-01871","title":"Provider title","tags":[],"cover_url":null}]}}"#.to_owned())
                } else {
                    Ok(r#"{"success":1,"data":{"movie":{"id":"item","number":"3DSVR-01871","title":"Provider title","tags":[{"id":"212","name":"VR"}],"cover_url":null}}}"#.to_owned())
                }
            },
            |_| panic!("JavDB without a cover must not dispatch an image"),
            |body| {
                assert!(body.contains("13dsvr01871"));
                Ok(r#"{"data":{"c0":{"id":"13dsvr01871","contentType":"VR","title":"Exact VR","packageImage":{"largeUrl":"https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/13dsvr01871/13dsvr01871pl.jpg"}}}}"#.to_owned())
            },
            |_| Ok(jpeg(600, 800)),
            |_| panic!("legacy must not run after exact FANZA success"),
            |_| panic!("legacy image must not run"),
        )
        .0
        .expect("FANZA must accept the exact 3DSVR category and content identity");
        assert_eq!(fanza.0.provider, "FANZA");
        assert_eq!(fanza.0.provider_id, "13dsvr01871");
    }

    #[test]
    fn mismatched_fanza_category_and_code_cannot_supply_a_cover() {
        for document in [
            r#"{"data":{"c0":{"id":"abc00123","contentType":"TWO_DIMENSION","title":"Wrong","packageImage":{"largeUrl":"https://awsimgsrc.dmm.co.jp/wrong.jpg"}}}}"#,
            r#"{"data":{"c0":{"id":"adlt00123","contentType":"VR","title":"Wrong","packageImage":{"largeUrl":"https://awsimgsrc.dmm.co.jp/wrong.jpg"}}}}"#,
        ] {
            let authority = authority(LibraryPresentationCategory::Adult);
            let image_calls = std::cell::Cell::new(0);
            let (cover, _, _) = resolve_cover_with(
                &authority,
                |_| Err(ProviderRequestError::SourceUnavailable),
                |_| panic!("missing JavDB item has no image"),
                |_| Ok(document.to_owned()),
                |_| {
                    image_calls.set(image_calls.get() + 1);
                    Ok(jpeg(600, 800))
                },
                |_| Err(ProviderRequestError::SourceUnavailable),
                |_| panic!("missing legacy item has no image"),
            );
            assert!(cover.is_none());
            assert_eq!(image_calls.get(), 0);
        }
    }

    #[test]
    fn cover_validation_rejects_placeholder_undersized_and_non_raster_bytes() {
        assert!(validate_cover(jpeg(590, 800)).is_err());
        assert!(validate_cover(vec![0xff, 0xd8, 0xff]).is_err());
        assert!(validate_cover(vec![1; COVER_MIN_BYTES]).is_err());
    }

    #[test]
    fn exact_javdatabase_cast_accepts_canonical_padding_after_identity_validation() {
        let accepted =
            r#"<html><title>作品 ADLT-00123 - Alice Example - JAV Database</title></html>"#;
        assert_eq!(
            javdatabase_romanized_cast(accepted, "ADLT-123").as_deref(),
            Some("Alice Example")
        );
        for rejected in [
            r#"<title>作品 ADLT-124 - Alice Example - JAV Database</title>"#,
            r#"<title>作品 ADLT-123 ABC-7 - Alice Example - JAV Database</title>"#,
            r#"<title>作品 ADLT-123 - JAV Database - JAV Database</title>"#,
            r#"<title>作品 ADLT-123 - JAV - JAV Database</title>"#,
            "<title>作品 ADLT-123 - \u{7} - JAV Database</title>",
        ] {
            assert_eq!(javdatabase_romanized_cast(rejected, "ADLT-123"), None);
        }
    }

    #[test]
    fn fresh_cover_and_confirmed_miss_cache_survive_restart_until_exact_expiry() {
        let fixture = CacheFixture::new("restart-expiry");
        let authority = authority(LibraryPresentationCategory::Vr);
        let state = LibraryPresentationState::default();
        let calls = Cell::new(0);
        let ready = resolve_cover_at_with(
            &state,
            &fixture.path,
            &authority,
            100,
            || true,
            || {
                calls.set(calls.get() + 1);
                (
                    Some((cover("item", 16.0 / 9.0), jpeg(1600, 900))),
                    None,
                    false,
                )
            },
        )
        .expect("cover must resolve");
        assert_eq!(ready[2], "ready");
        assert_eq!(calls.get(), 1);

        let restarted = LibraryPresentationState::default();
        let cached = resolve_cover_at_with(
            &restarted,
            &fixture.path,
            &authority,
            100 + COVER_TTL_SECONDS,
            || true,
            || panic!("a fresh restart cache must skip provider discovery"),
        )
        .expect("cached cover must resolve");
        assert_eq!(cached[2], "ready");
        assert_eq!(cached[6], (16.0 / 9.0).to_string());
        let fetched = fetch_cover_with(&restarted, &authority, &cached[5], |source| {
            assert_eq!(source.provider_id, "item");
            Ok(jpeg(1600, 900))
        })
        .expect("restart must re-proxy the retained raw source");
        assert_eq!(image_dimensions(&fetched), Some((1600, 900)));

        let expired_calls = Cell::new(0);
        resolve_cover_at_with(
            &LibraryPresentationState::default(),
            &fixture.path,
            &authority,
            101 + COVER_TTL_SECONDS,
            || true,
            || {
                expired_calls.set(expired_calls.get() + 1);
                (None, None, false)
            },
        )
        .expect("expired cover must refresh");
        assert_eq!(expired_calls.get(), 1);

        let miss_authority = LibraryItemAuthority {
            identity: "b".repeat(40),
            code: "MDVR-420".to_owned(),
            ..authority
        };
        resolve_cover_at_with(
            &state,
            &fixture.path,
            &miss_authority,
            200,
            || true,
            || (None, None, false),
        )
        .expect("confirmed miss must resolve");
        resolve_cover_at_with(
            &LibraryPresentationState::default(),
            &fixture.path,
            &miss_authority,
            200 + COVER_TTL_SECONDS,
            || true,
            || panic!("a fresh confirmed miss must skip providers after restart"),
        )
        .expect("cached miss must resolve");
    }

    #[test]
    fn cover_authority_is_released_on_cancellation_and_consumed_on_fetch_failure() {
        let fixture = CacheFixture::new("cover-authority-lifecycle");
        let authority = authority(LibraryPresentationCategory::Vr);
        let state = LibraryPresentationState::default();
        begin_cover_request(&state, authority.category, &authority.code, 1)
            .expect("cover request must begin");
        let resolved = resolve_cover_at_with_request(
            &state,
            &fixture.path,
            &authority,
            1,
            100,
            || true,
            || {
                (
                    Some((cover("item", 16.0 / 9.0), jpeg(1600, 900))),
                    None,
                    false,
                )
            },
        )
        .expect("cover must resolve");
        let authority_id = &resolved[5];
        assert_eq!(state.0.lock().unwrap().covers.len(), 1);
        cancel_cover_request(&state, authority.category, &authority.code, 1)
            .expect("cancellation must release the exact authority");
        assert!(state.0.lock().unwrap().covers.is_empty());
        assert_eq!(
            fetch_cover_with(&state, &authority, authority_id, |_| {
                panic!("a released source must not be fetched")
            }),
            Err(LIBRARY_PRESENTATION_STALE)
        );

        let restarted = LibraryPresentationState::default();
        begin_cover_request(&restarted, authority.category, &authority.code, 2)
            .expect("restarted request must begin");
        let cached = resolve_cover_at_with_request(
            &restarted,
            &fixture.path,
            &authority,
            2,
            101,
            || true,
            || panic!("fresh cached source must skip provider discovery"),
        )
        .expect("cached source must create one current authority");
        assert_eq!(
            fetch_cover_with(&restarted, &authority, &cached[5], |_| {
                Err(ProviderRequestError::Network)
            }),
            Err(LIBRARY_PRESENTATION_FAILED)
        );
        assert!(restarted.0.lock().unwrap().covers.is_empty());
        assert_eq!(
            fetch_cover_with(&restarted, &authority, &cached[5], |_| {
                panic!("a failed source must be single use")
            }),
            Err(LIBRARY_PRESENTATION_STALE)
        );
    }

    #[test]
    fn repeated_scan_identity_replacement_and_global_bound_retire_obsolete_bytes() {
        let state = LibraryPresentationState::default();
        let mut previous_id = String::new();
        for index in 0..16 {
            let authority = LibraryItemAuthority {
                category: LibraryPresentationCategory::Vr,
                identity: format!("{index:040x}"),
                code: "3DSVR-1871".to_owned(),
            };
            let response = cover_response(
                &state,
                &authority,
                0,
                "ready",
                Some(cover(&format!("item-{index}"), 0.72)),
                Some(jpeg(600, 800)),
            )
            .expect("replacement authority must be retained");
            if !previous_id.is_empty() {
                assert!(!state.0.lock().unwrap().covers.contains_key(&previous_id));
            }
            previous_id = response[5].clone();
            assert_eq!(state.0.lock().unwrap().covers.len(), 1);
        }

        for index in 0..MAX_RETAINED_COVER_AUTHORITIES + 3 {
            let authority = LibraryItemAuthority {
                category: LibraryPresentationCategory::Adult,
                identity: format!("f{index:039x}"),
                code: format!("ADLT-{}", index + 1),
            };
            cover_response(
                &state,
                &authority,
                0,
                "ready",
                Some(cover(&format!("adult-{index}"), 0.72)),
                Some(jpeg(600, 800)),
            )
            .expect("bounded authority must be retained");
        }
        let context = state.0.lock().unwrap();
        assert_eq!(context.covers.len(), MAX_RETAINED_COVER_AUTHORITIES);
        assert!(context
            .covers
            .values()
            .all(|retained| retained.bytes.is_some()));
    }

    #[test]
    fn obsolete_cover_request_cannot_replace_or_cancel_the_latest_authority() {
        let fixture = CacheFixture::new("obsolete-cover-request");
        let state = LibraryPresentationState::default();
        let authority = authority(LibraryPresentationCategory::Vr);
        begin_cover_request(&state, authority.category, &authority.code, 1)
            .expect("first request must begin");
        begin_cover_request(&state, authority.category, &authority.code, 2)
            .expect("replacement request must begin");
        assert_eq!(
            resolve_cover_at_with_request(
                &state,
                &fixture.path,
                &authority,
                1,
                10,
                || true,
                || (None, None, false),
            ),
            Err(LIBRARY_PRESENTATION_STALE)
        );
        assert!(!fixture.path.exists());
        assert_eq!(
            cover_response(
                &state,
                &authority,
                1,
                "ready",
                Some(cover("obsolete", 0.72)),
                Some(jpeg(600, 800)),
            ),
            Err(LIBRARY_PRESENTATION_STALE)
        );
        let current = cover_response(
            &state,
            &authority,
            2,
            "ready",
            Some(cover("current", 0.72)),
            Some(jpeg(600, 800)),
        )
        .expect("latest request must retain its authority");
        cancel_cover_request(&state, authority.category, &authority.code, 1)
            .expect("obsolete cancellation must be harmless");
        assert!(cover_request_is_current(
            &state,
            authority.category,
            &authority.code,
            2
        ));
        assert!(state.0.lock().unwrap().covers.contains_key(&current[5]));
        cancel_cover_request(&state, authority.category, &authority.code, 2)
            .expect("current cancellation must retire the authority");
        assert!(!cover_request_is_current(
            &state,
            authority.category,
            &authority.code,
            2
        ));
        assert!(state.0.lock().unwrap().covers.is_empty());
    }

    #[test]
    fn retained_cover_request_tombstones_remain_bounded() {
        let state = LibraryPresentationState::default();
        for generation in 1..=MAX_RETAINED_COVER_REQUESTS as u64 + 3 {
            let code = format!("ADLT-{generation}");
            begin_cover_request(
                &state,
                LibraryPresentationCategory::Adult,
                &code,
                generation,
            )
            .expect("bounded request must begin");
        }
        assert_eq!(
            state.0.lock().unwrap().cover_requests.len(),
            MAX_RETAINED_COVER_REQUESTS
        );
        assert!(!cover_request_is_current(
            &state,
            LibraryPresentationCategory::Adult,
            "ADLT-1",
            1
        ));
        assert!(cover_request_is_current(
            &state,
            LibraryPresentationCategory::Adult,
            "ADLT-131",
            131
        ));
    }

    #[test]
    fn invalidated_cached_source_is_not_retried_and_provider_resolution_can_replace_it() {
        let fixture = CacheFixture::new("cover-invalidation");
        let authority = authority(LibraryPresentationCategory::Vr);
        let state = LibraryPresentationState::default();
        let first = resolve_cover_at_with(
            &state,
            &fixture.path,
            &authority,
            100,
            || true,
            || (Some((cover("unusable", 0.5), jpeg(400, 800))), None, false),
        )
        .expect("first source must resolve");
        fetch_cover_with(&state, &authority, &first[5], |_| {
            panic!("fresh bytes must be retained")
        })
        .expect("browser decode begins after exact bytes are returned");
        invalidate_cover(&state, &fixture.path, &authority, 0, &first[5])
            .expect("browser decode failure must invalidate the exact cached source");

        let provider_calls = Cell::new(0);
        let replacement = resolve_cover_at_with(
            &state,
            &fixture.path,
            &authority,
            101,
            || true,
            || {
                provider_calls.set(provider_calls.get() + 1);
                (
                    Some((
                        CoverSource {
                            provider: "FANZA",
                            provider_id: "13dsvr01871".to_owned(),
                            url: "https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/13dsvr01871/13dsvr01871ps.jpg".to_owned(),
                            aspect_ratio: 0.72,
                        },
                        jpeg(576, 800),
                    )),
                    None,
                    false,
                )
            },
        )
        .expect("retry must run provider resolution again");
        assert_eq!(provider_calls.get(), 1);
        assert_eq!(replacement[2], "ready");
        assert_eq!(replacement[3], "FANZA");
        assert_ne!(replacement[5], first[5]);
        assert_eq!(
            fetch_cover_with(&state, &authority, &first[5], |_| {
                panic!("invalidated authority must not be fetchable")
            }),
            Err(LIBRARY_PRESENTATION_STALE)
        );
    }

    #[test]
    fn successful_metadata_cache_survives_restart_and_stale_metadata_is_not_cached() {
        let fixture = CacheFixture::new("metadata");
        let authority = authority(LibraryPresentationCategory::Adult);
        let calls = Cell::new(0);
        let metadata = PresentationMetadata {
            source: Some("JavDB".to_owned()),
            provider_id: Some("item".to_owned()),
            title: Some("Exact title".to_owned()),
            date: Some("2024-01-02".to_owned()),
            runtime: Some("90 min".to_owned()),
            cast: vec!["Actor".to_owned()],
        };
        let ready = resolve_metadata_at_with(
            &LibraryPresentationState::default(),
            &fixture.path,
            &authority,
            100,
            || true,
            || {
                calls.set(calls.get() + 1);
                Ok(Some(metadata.clone()))
            },
        )
        .expect("metadata must resolve");
        assert_eq!(ready[2], "automatic");

        let restarted = resolve_metadata_at_with(
            &LibraryPresentationState::default(),
            &fixture.path,
            &authority,
            100 + METADATA_TTL_SECONDS,
            || true,
            || panic!("fresh metadata must not repeat provider work after restart"),
        )
        .expect("cached metadata must resolve");
        assert_eq!(restarted, ready);
        assert_eq!(calls.get(), 1);

        let stale_authority = LibraryItemAuthority {
            identity: "d".repeat(40),
            code: "ADLT-124".to_owned(),
            ..authority
        };
        let current = Cell::new(true);
        let stale = resolve_metadata_at_with(
            &LibraryPresentationState::default(),
            &fixture.path,
            &stale_authority,
            200,
            || current.get(),
            || {
                current.set(false);
                Ok(Some(metadata))
            },
        );
        assert_eq!(stale, Err(LIBRARY_PRESENTATION_STALE));
        assert!(read_cache(&fixture.path)
            .expect("cache must remain valid")
            .iter()
            .all(|entry| entry.identity != stale_authority.identity));
    }

    #[test]
    fn transient_cover_failure_is_never_cached_as_a_confirmed_miss() {
        let fixture = CacheFixture::new("transient");
        let authority = authority(LibraryPresentationCategory::Adult);
        let first = resolve_cover_at_with(
            &LibraryPresentationState::default(),
            &fixture.path,
            &authority,
            100,
            || true,
            || (None, None, true),
        )
        .expect("transient state must remain local");
        assert_eq!(first[2], "unavailable");
        let retried = Cell::new(false);
        let second = resolve_cover_at_with(
            &LibraryPresentationState::default(),
            &fixture.path,
            &authority,
            101,
            || true,
            || {
                retried.set(true);
                (None, None, false)
            },
        )
        .expect("retry must dispatch provider discovery");
        assert!(retried.get());
        assert_eq!(second[2], "missing");
    }

    #[test]
    fn stale_provider_result_never_creates_cover_or_cache_authority() {
        let fixture = CacheFixture::new("stale");
        let authority = authority(LibraryPresentationCategory::Vr);
        let current = Cell::new(true);
        let state = LibraryPresentationState::default();
        let result = resolve_cover_at_with(
            &state,
            &fixture.path,
            &authority,
            10,
            || current.get(),
            || {
                current.set(false);
                (
                    Some((cover("stale-item", 16.0 / 9.0), jpeg(1600, 900))),
                    Some(PresentationMetadata {
                        source: Some("JavDB".to_owned()),
                        provider_id: Some("stale-item".to_owned()),
                        title: Some("Stale".to_owned()),
                        ..PresentationMetadata::default()
                    }),
                    false,
                )
            },
        );
        assert_eq!(result, Err(LIBRARY_PRESENTATION_STALE));
        assert!(!fixture.path.exists());
        let context = state
            .0
            .lock()
            .expect("presentation state must remain readable");
        assert!(context.covers.is_empty());
        assert!(context.metadata_seeds.is_empty());
    }

    #[test]
    fn version_oversize_duplicate_and_symlink_caches_are_all_misses() {
        let fixture = CacheFixture::new("invalid-forms");
        let authority = authority(LibraryPresentationCategory::Vr);
        fs::write(&fixture.path, "another-version\n").expect("version fixture must be written");
        assert_invalid_cache_refreshes(&fixture.path, &authority);

        fs::write(&fixture.path, vec![b'x'; CACHE_MAX_BYTES as usize + 1])
            .expect("oversize fixture must be written");
        assert_invalid_cache_refreshes(&fixture.path, &authority);

        let entry = CacheEntry {
            identity: authority.identity.clone(),
            category: authority.category,
            code: authority.code.clone(),
            cover_saved_at: 1,
            cover_state: "missing",
            cover: None,
            metadata_saved_at: 0,
            metadata_state: "missing",
            metadata: PresentationMetadata::default(),
        };
        write_cache(&fixture.path, std::slice::from_ref(&entry))
            .expect("valid fixture must be written");
        let original = fs::read_to_string(&fixture.path).expect("fixture must be readable");
        let duplicate = original
            .lines()
            .nth(1)
            .expect("fixture must contain one entry");
        fs::write(&fixture.path, format!("{original}{duplicate}\n"))
            .expect("duplicate fixture must be written");
        assert_invalid_cache_refreshes(&fixture.path, &authority);

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            let target = fixture.directory.join("cache-target");
            write_cache(&target, &[entry]).expect("symlink target must be written");
            fs::remove_file(&fixture.path).expect("cache fixture must be removed");
            symlink(&target, &fixture.path).expect("cache symlink must be created");
            assert_invalid_cache_refreshes(&fixture.path, &authority);
            assert_eq!(
                read_cache(&target)
                    .expect("symlink target must remain valid")
                    .len(),
                1
            );
        }
    }

    #[test]
    fn malformed_cache_is_a_miss_and_concurrent_item_updates_are_merged() {
        let fixture = CacheFixture::new("malformed-merge");
        fs::write(
            &fixture.path,
            format!(
                "{CACHE_VERSION}\n{}\tvr\t{}\t1\tready\tJavDB\t{}\t{}\tNaN\t0\tmissing\t\t\t\t\t\t\n",
                "a".repeat(40),
                encode_text("MDVR-419"),
                encode_text("item"),
                encode_text("https://tp.cmastd.com/item.jpg")
            ),
        )
        .expect("malformed cache fixture must be written");
        let refreshed = Cell::new(false);
        resolve_cover_at_with(
            &LibraryPresentationState::default(),
            &fixture.path,
            &authority(LibraryPresentationCategory::Vr),
            10,
            || true,
            || {
                refreshed.set(true);
                (None, None, false)
            },
        )
        .expect("invalid cache must permit provider resolution");
        assert!(refreshed.get());

        write_cache(
            &fixture.path,
            &[CacheEntry {
                identity: authority(LibraryPresentationCategory::Vr).identity,
                category: LibraryPresentationCategory::Vr,
                code: "MDVR-419".to_owned(),
                cover_saved_at: 1,
                cover_state: "ready",
                cover: Some(cover("", 16.0 / 9.0)),
                metadata_saved_at: 0,
                metadata_state: "missing",
                metadata: PresentationMetadata::default(),
            }],
        )
        .expect("empty provider identity fixture must be written");
        assert_invalid_cache_refreshes(&fixture.path, &authority(LibraryPresentationCategory::Vr));

        let state = LibraryPresentationState::default();
        let barrier = Arc::new(Barrier::new(2));
        let first_authority = authority(LibraryPresentationCategory::Adult);
        let second_authority = LibraryItemAuthority {
            identity: "c".repeat(40),
            code: "ADLT-124".to_owned(),
            ..first_authority.clone()
        };
        let first = {
            let state = state.clone();
            let path = fixture.path.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                resolve_cover_at_with(
                    &state,
                    &path,
                    &first_authority,
                    20,
                    || true,
                    || {
                        barrier.wait();
                        (None, None, false)
                    },
                )
            })
        };
        let second = {
            let state = state.clone();
            let path = fixture.path.clone();
            thread::spawn(move || {
                resolve_cover_at_with(
                    &state,
                    &path,
                    &second_authority,
                    20,
                    || true,
                    || {
                        barrier.wait();
                        (None, None, false)
                    },
                )
            })
        };
        first
            .join()
            .expect("first cache writer must finish")
            .unwrap();
        second
            .join()
            .expect("second cache writer must finish")
            .unwrap();
        let entries = read_cache(&fixture.path).expect("merged cache must remain valid");
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.code.as_str())
                .collect::<HashSet<_>>(),
            HashSet::from(["ADLT-123", "ADLT-124"])
        );
    }
}
