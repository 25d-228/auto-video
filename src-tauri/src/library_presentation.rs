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
    vr_torrent::{hex_sha1, json_array, json_object, JsonParser, JsonValue},
    ProviderRequestError,
};

pub(crate) const LIBRARY_PRESENTATION_FAILED: &str = "library_presentation_failed";
pub(crate) const LIBRARY_PRESENTATION_STALE: &str = "library_presentation_stale";

const CACHE_VERSION: &str = "library-presentation-v3";
const CACHE_MAX_BYTES: u64 = 4 * 1024 * 1024;
const COVER_TTL_SECONDS: u64 = 24 * 60 * 60;
const METADATA_TTL_SECONDS: u64 = 365 * 24 * 60 * 60;
const COVER_MAX_BYTES: usize = 16 * 1024 * 1024;
const COVER_MIN_BYTES: usize = 6_000;
const RESPONSE_MAX_BYTES: usize = 4 * 1024 * 1024;
const DEFAULT_COVER_RATIO: f64 = 0.72;
const MAX_COVER_RATIO: f64 = 4.0;
const MAX_RETAINED_COVER_AUTHORITIES: usize = 128;
const MAX_RETAINED_COVER_BYTES: usize = 4;
const MAX_RETAINED_COVER_REQUESTS: usize = 128;
const MAX_RETAINED_FAILED_COVER_ITEMS: usize = 128;
const MAX_RETAINED_METADATA_SEEDS: usize = 128;
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
    pub product_identity: String,
}

#[derive(Clone, Debug, PartialEq)]
struct CoverSource {
    provider: &'static str,
    provider_id: String,
    display_code: String,
    url: String,
    aspect_ratio: f64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct VerifiedDisplayIdentity {
    provider: &'static str,
    provider_id: String,
    display_code: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct PresentationMetadata {
    verified_identity: Option<VerifiedDisplayIdentity>,
    identity_conflict: bool,
    source: Option<String>,
    provider_id: Option<String>,
    display_code: Option<String>,
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
    fn has_provider_fields(&self) -> bool {
        self.source.is_some() && self.provider_id.is_some() && self.display_code.is_some()
    }

    fn has_descriptive_fields(&self) -> bool {
        self.title.is_some()
            || self.date.is_some()
            || self.runtime.is_some()
            || !self.cast.is_empty()
    }

    fn is_ready(&self) -> bool {
        self.has_provider_fields()
            && (self.has_descriptive_fields() || self.verified_identity.is_some())
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

struct FailedCoverSources {
    category: LibraryPresentationCategory,
    code: String,
    urls: HashSet<String>,
    sequence: u64,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct MetadataSeedKey {
    category: LibraryPresentationCategory,
    code: String,
    item_identity: String,
    request_generation: u64,
}

struct MetadataSeed {
    metadata: PresentationMetadata,
    sequence: u64,
}

struct VerifiedIdentitySeed {
    identity: VerifiedDisplayIdentity,
    sequence: u64,
}

#[derive(Clone, Debug)]
struct CacheEntry {
    identity: String,
    category: LibraryPresentationCategory,
    code: String,
    identity_saved_at: u64,
    verified_identity: Option<VerifiedDisplayIdentity>,
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
    failed_cover_sources: HashMap<String, FailedCoverSources>,
    metadata_seeds: HashMap<MetadataSeedKey, MetadataSeed>,
    verified_identity_seeds: HashMap<MetadataSeedKey, VerifiedIdentitySeed>,
    next_cover_sequence: u64,
}

fn metadata_seed_key(authority: &LibraryItemAuthority, request_generation: u64) -> MetadataSeedKey {
    MetadataSeedKey {
        category: authority.category,
        code: authority.code.clone(),
        item_identity: authority.identity.clone(),
        request_generation,
    }
}

fn retire_metadata_seed(
    context: &mut LibraryPresentationContext,
    category: LibraryPresentationCategory,
    code: &str,
    request_generation: u64,
) {
    context.metadata_seeds.retain(|key, _| {
        key.category != category || key.code != code || key.request_generation != request_generation
    });
    context.verified_identity_seeds.retain(|key, _| {
        key.category != category || key.code != code || key.request_generation != request_generation
    });
}

fn bound_metadata_seeds(context: &mut LibraryPresentationContext) {
    while context.metadata_seeds.len() > MAX_RETAINED_METADATA_SEEDS {
        let oldest_obsolete = context
            .metadata_seeds
            .iter()
            .filter(|(key, _)| {
                !cover_request_matches(context, key.category, &key.code, key.request_generation)
            })
            .min_by_key(|(_, seed)| seed.sequence)
            .map(|(key, _)| key.clone());
        let Some(oldest_obsolete) = oldest_obsolete else {
            break;
        };
        context.metadata_seeds.remove(&oldest_obsolete);
    }
    while context.verified_identity_seeds.len() > MAX_RETAINED_METADATA_SEEDS {
        let oldest_obsolete = context
            .verified_identity_seeds
            .iter()
            .filter(|(key, _)| {
                !cover_request_matches(context, key.category, &key.code, key.request_generation)
            })
            .min_by_key(|(_, seed)| seed.sequence)
            .map(|(key, _)| key.clone());
        let Some(oldest_obsolete) = oldest_obsolete else {
            break;
        };
        context.verified_identity_seeds.remove(&oldest_obsolete);
    }
}

fn retain_metadata_seed(
    context: &mut LibraryPresentationContext,
    authority: &LibraryItemAuthority,
    request_generation: u64,
    metadata: PresentationMetadata,
) {
    context.next_cover_sequence = context.next_cover_sequence.wrapping_add(1);
    let sequence = context.next_cover_sequence;
    context.metadata_seeds.insert(
        metadata_seed_key(authority, request_generation),
        MetadataSeed { metadata, sequence },
    );
    bound_metadata_seeds(context);
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
        verified_identity: Some(javdb_verified_identity(item)),
        identity_conflict: false,
        source: Some("JavDB".to_owned()),
        provider_id: Some(item.provider_item_id.clone()),
        display_code: Some(item.display_code.clone()),
        title: item.title.clone(),
        date: item.release_date.clone().filter(|date| valid_date(date)),
        runtime: item.duration.clone(),
        cast: item.actors.clone(),
    }
}

fn exact_legacy_cover_url(
    document: &str,
    identity: &str,
) -> Result<Option<String>, ProviderRequestError> {
    let Some(JsonValue::Object(root)) = JsonParser::new(document).parse() else {
        return Err(ProviderRequestError::Provider);
    };
    let Some(JsonValue::String(content_id)) = root.get("content_id") else {
        return Err(ProviderRequestError::Provider);
    };
    let canonical = crate::vr_torrent::product_code_candidates(content_id)
        .into_iter()
        .map(|(candidate, _)| candidate)
        .collect::<HashSet<_>>();
    if canonical.len() != 1 || !canonical.contains(identity) {
        return Err(ProviderRequestError::Provider);
    }
    let images = match root.get("images") {
        None | Some(JsonValue::Null) => return Ok(None),
        Some(JsonValue::Object(images)) => images,
        Some(_) => return Err(ProviderRequestError::Provider),
    };
    let jacket = match images.get("jacket_image") {
        None | Some(JsonValue::Null) => return Ok(None),
        Some(JsonValue::Object(jacket)) => jacket,
        Some(_) => return Err(ProviderRequestError::Provider),
    };
    let mut accepted = None;
    for key in ["large2", "large"] {
        let url = match jacket.get(key) {
            None | Some(JsonValue::Null) => continue,
            Some(JsonValue::String(url)) if valid_legacy_cover_url(url) => url,
            Some(_) => return Err(ProviderRequestError::Provider),
        };
        if accepted.is_none() {
            accepted = Some(url.clone());
        }
    }
    Ok(accepted)
}

fn metadata_from_legacy(
    document: &str,
    identity: &str,
    display_code: &str,
) -> Option<PresentationMetadata> {
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
    if codes.len() != 1 || !codes.contains(identity) {
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
        verified_identity: None,
        identity_conflict: false,
        source: Some("r18.dev".to_owned()),
        provider_id: Some(content_id.clone()),
        display_code: Some(display_code.to_owned()),
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

fn cover_source_valid(
    source: &CoverSource,
    category: LibraryPresentationCategory,
    code: &str,
) -> bool {
    !source.provider_id.is_empty()
        && crate::vr_torrent::product_code_display_form(&source.display_code).as_deref()
            == Some(&source.display_code)
        && source.aspect_ratio.is_finite()
        && source.aspect_ratio > 0.0
        && source.aspect_ratio <= MAX_COVER_RATIO
        && match source.provider {
            "JavDB" => javdb_catalog::valid_library_cover_url(&source.url),
            "FANZA" => fanza_catalog::valid_cached_exact_library_cover(
                category.value(),
                code,
                &source.provider_id,
                &source.display_code,
                &source.url,
            ),
            "r18.dev" => valid_legacy_cover_url(&source.url),
            _ => false,
        }
}

fn verified_display_identity_valid(
    identity: &VerifiedDisplayIdentity,
    category: LibraryPresentationCategory,
    code: &str,
) -> bool {
    crate::vr_torrent::product_code_display_form(&identity.display_code).as_deref()
        == Some(&identity.display_code)
        && crate::vr_torrent::canonical_product_code(&identity.display_code)
            == crate::vr_torrent::canonical_product_code(code)
        && match identity.provider {
            "JavDB" => {
                !identity.provider_id.is_empty()
                    && identity.provider_id.len() <= 64
                    && identity
                        .provider_id
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric())
            }
            "FANZA" => fanza_catalog::valid_cached_exact_library_identity(
                category.value(),
                code,
                &identity.provider_id,
                &identity.display_code,
            ),
            _ => false,
        }
}

fn javdb_verified_identity(item: &javdb_catalog::ExactLibraryItem) -> VerifiedDisplayIdentity {
    VerifiedDisplayIdentity {
        provider: "JavDB",
        provider_id: item.provider_item_id.clone(),
        display_code: item.display_code.clone(),
    }
}

fn fanza_verified_identity(item: &fanza_catalog::ExactLibraryItem) -> VerifiedDisplayIdentity {
    VerifiedDisplayIdentity {
        provider: "FANZA",
        provider_id: item.content_id.clone(),
        display_code: item.display_code.clone(),
    }
}

fn verified_identity_from_cover(source: &CoverSource) -> Option<VerifiedDisplayIdentity> {
    matches!(source.provider, "JavDB" | "FANZA").then(|| VerifiedDisplayIdentity {
        provider: source.provider,
        provider_id: source.provider_id.clone(),
        display_code: source.display_code.clone(),
    })
}

fn reconcile_verified_identities(
    current: Option<VerifiedDisplayIdentity>,
    candidate: Option<VerifiedDisplayIdentity>,
) -> Result<Option<VerifiedDisplayIdentity>, ()> {
    let (current, candidate) = match (current, candidate) {
        (Some(current), Some(candidate)) => (current, candidate),
        (current, candidate) => return Ok(current.or(candidate)),
    };
    if current.provider == candidate.provider {
        return (current.provider_id == candidate.provider_id
            && current.display_code == candidate.display_code)
            .then_some(Some(current))
            .ok_or(());
    }
    if current.display_code != candidate.display_code {
        return Err(());
    }
    if current.provider == "JavDB" {
        Ok(Some(current))
    } else if candidate.provider == "JavDB" {
        Ok(Some(candidate))
    } else {
        Err(())
    }
}

fn cover_identity_consistent(
    source: &CoverSource,
    identity: Option<&VerifiedDisplayIdentity>,
    category: LibraryPresentationCategory,
    code: &str,
) -> bool {
    if !cover_source_valid(source, category, code) {
        return false;
    }
    match source.provider {
        "JavDB" => identity.is_some_and(|identity| {
            identity.provider == "JavDB"
                && identity.provider_id == source.provider_id
                && identity.display_code == source.display_code
        }),
        "FANZA" => identity.is_some_and(|identity| {
            identity.display_code == source.display_code
                && (identity.provider == "JavDB"
                    || (identity.provider == "FANZA" && identity.provider_id == source.provider_id))
        }),
        "r18.dev" => identity.map_or_else(
            || source.display_code == code,
            |identity| identity.display_code == source.display_code,
        ),
        _ => false,
    }
}

fn metadata_identity_consistent(
    metadata: &PresentationMetadata,
    identity: Option<&VerifiedDisplayIdentity>,
    code: &str,
) -> bool {
    if metadata.identity_conflict {
        return false;
    }
    if metadata
        .verified_identity
        .as_ref()
        .is_some_and(|metadata_identity| Some(metadata_identity) != identity)
    {
        return false;
    }
    let expected_display = identity.map_or(code, |identity| identity.display_code.as_str());
    if metadata.display_code.as_deref() != Some(expected_display) {
        return false;
    }
    if metadata
        .source
        .as_deref()
        .is_some_and(|source| source.starts_with("JavDB"))
    {
        return identity.is_some_and(|identity| {
            identity.provider == "JavDB"
                && metadata.provider_id.as_deref() == Some(identity.provider_id.as_str())
        });
    }
    true
}

fn attach_verified_identity(
    metadata: &mut PresentationMetadata,
    identity: Option<VerifiedDisplayIdentity>,
) {
    if let Some(identity) = identity {
        metadata.display_code = Some(identity.display_code.clone());
        metadata.verified_identity = Some(identity);
    } else {
        metadata.verified_identity = None;
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

fn javdb_query_forms(code: &str) -> Vec<String> {
    let Some(forms) = crate::vr_torrent::product_code_forms(code) else {
        return Vec::new();
    };
    let number = forms
        .identity
        .split_once('-')
        .map(|(_, number)| number)
        .unwrap_or_default();
    let mut queries = vec![forms.display];
    if let Some(query) = crate::vr_torrent::javdb_product_code_query_form(code) {
        queries.push(query);
    }
    queries.push(format!("{}-{number}", forms.prefix));
    let mut retained = HashSet::new();
    queries.retain(|query| retained.insert(query.clone()));
    queries
}

fn fetch_exact_javdb_item(
    authority: &LibraryItemAuthority,
    fetch: &mut impl FnMut(&str) -> Result<String, ProviderRequestError>,
) -> Result<Option<javdb_catalog::ExactLibraryItem>, ProviderRequestError> {
    for query in javdb_query_forms(&authority.code) {
        match javdb_catalog::fetch_exact_library_item_with(
            authority.category.value(),
            &query,
            fetch,
        ) {
            Ok(Some(item)) => return Ok(Some(item)),
            Ok(None) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}

fn fetch_current_cover_source(
    url: &str,
    failed_urls: &HashSet<String>,
    fetch: impl FnOnce(&str) -> Result<Vec<u8>, ProviderRequestError>,
) -> Result<Vec<u8>, ProviderRequestError> {
    if failed_urls.contains(url) {
        return Err(ProviderRequestError::Provider);
    }
    fetch(url)
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
    let mut verified_identity = None;
    match fetch_exact_javdb_item(authority, &mut javdb_document) {
        Ok(Some(item)) => {
            verified_identity = Some(javdb_verified_identity(&item));
            metadata = Some(metadata_from_javdb(&item));
            if let Some(url) = item.cover_url {
                match javdb_image(&url).and_then(validate_cover) {
                    Ok((bytes, ratio)) => {
                        return (
                            Some((
                                CoverSource {
                                    provider: "JavDB",
                                    provider_id: item.provider_item_id,
                                    display_code: item.display_code,
                                    url,
                                    aspect_ratio: ratio,
                                },
                                bytes,
                            )),
                            metadata,
                            transient,
                        );
                    }
                    Err(_) => transient = true,
                }
            }
        }
        Ok(None) => {}
        Err(_) => transient = true,
    }

    match fanza_catalog::fetch_exact_library_item_with(
        authority.category.value(),
        &authority.code,
        fanza_document,
    ) {
        Ok(Some(item)) => {
            let fanza_identity = fanza_verified_identity(&item);
            verified_identity =
                match reconcile_verified_identities(verified_identity, Some(fanza_identity)) {
                    Ok(identity) => identity,
                    Err(()) => {
                        return (
                            None,
                            Some(PresentationMetadata {
                                identity_conflict: true,
                                ..PresentationMetadata::default()
                            }),
                            true,
                        )
                    }
                };
            if let Some(identity) = verified_identity.clone() {
                attach_verified_identity(
                    metadata.get_or_insert_with(PresentationMetadata::default),
                    Some(identity),
                );
            }
            if let Some(url) = item.cover_url {
                match fanza_image(&url).and_then(validate_cover) {
                    Ok((bytes, ratio)) => {
                        return (
                            Some((
                                CoverSource {
                                    provider: "FANZA",
                                    provider_id: item.content_id,
                                    display_code: item.display_code,
                                    url,
                                    aspect_ratio: ratio,
                                },
                                bytes,
                            )),
                            metadata,
                            transient,
                        );
                    }
                    Err(_) => transient = true,
                }
            }
        }
        Ok(None) => {}
        Err(_) => transient = true,
    }

    match legacy_document(&legacy_url(&authority.product_identity)) {
        Ok(document) => {
            if metadata.is_none() {
                metadata =
                    metadata_from_legacy(&document, &authority.product_identity, &authority.code);
            }
            match exact_legacy_cover_url(&document, &authority.product_identity) {
                Ok(Some(url)) => match legacy_image(&url).and_then(validate_cover) {
                    Ok((bytes, ratio)) => {
                        return (
                            Some((
                                CoverSource {
                                    provider: "r18.dev",
                                    provider_id: authority.product_identity.clone(),
                                    display_code: verified_identity.as_ref().map_or_else(
                                        || authority.code.clone(),
                                        |identity| identity.display_code.clone(),
                                    ),
                                    url,
                                    aspect_ratio: ratio,
                                },
                                bytes,
                            )),
                            metadata,
                            transient,
                        );
                    }
                    Err(_) => transient = true,
                },
                Ok(None) => {}
                Err(_) => transient = true,
            }
        }
        Err(_) => transient = true,
    }
    if let Some(identity) = verified_identity {
        attach_verified_identity(
            metadata.get_or_insert_with(PresentationMetadata::default),
            Some(identity),
        );
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
    match fetch_exact_javdb_item(authority, &mut javdb_document) {
        Ok(Some(item)) => {
            let metadata = metadata_from_javdb(&item);
            accepted = metadata.is_ready().then_some(metadata);
        }
        Ok(None) | Err(ProviderRequestError::SourceUnavailable) => {}
        Err(_) => transient = true,
    }
    match legacy_document(&legacy_url(&authority.product_identity)) {
        Ok(document) => {
            if let Some(metadata) =
                metadata_from_legacy(&document, &authority.product_identity, &authority.code)
            {
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
                if let Some(cast) =
                    javdatabase_romanized_cast(&document, &authority.product_identity)
                {
                    if let Some(metadata) = &mut accepted {
                        metadata.cast.push(cast);
                        metadata.source = Some(match metadata.source.as_deref() {
                            Some(source) => format!("{source} + JavDatabase"),
                            None => "JavDatabase".to_owned(),
                        });
                    } else {
                        accepted = Some(PresentationMetadata {
                            source: Some("JavDatabase".to_owned()),
                            provider_id: Some(authority.product_identity.clone()),
                            display_code: Some(authority.code.clone()),
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
        && crate::vr_torrent::product_code_display_form(&entry.code).as_deref() == Some(&entry.code)
        && match (&entry.verified_identity, entry.identity_saved_at) {
            (Some(_), saved_at) => saved_at > 0,
            (None, 0) => true,
            _ => false,
        }
        && entry.verified_identity.as_ref().is_none_or(|identity| {
            verified_display_identity_valid(identity, entry.category, &entry.code)
        })
        && match (entry.cover_state, entry.cover.as_ref()) {
            ("ready", Some(source)) => cover_identity_consistent(
                source,
                entry.verified_identity.as_ref(),
                entry.category,
                &entry.code,
            ),
            ("missing", None) => true,
            _ => false,
        }
        && match entry.metadata_state {
            "ready" => {
                entry.metadata.has_provider_fields()
                    && (entry.metadata.has_descriptive_fields()
                        || entry.verified_identity.is_some())
                    && metadata_identity_consistent(
                        &entry.metadata,
                        entry.verified_identity.as_ref(),
                        &entry.code,
                    )
            }
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
        if fields.len() != 23 {
            return Err(());
        }
        let category = LibraryPresentationCategory::parse(fields[1]).ok_or(())?;
        let verified_identity = if fields[4].is_empty() {
            if !fields[5].is_empty() || !fields[6].is_empty() {
                return Err(());
            }
            None
        } else {
            Some(VerifiedDisplayIdentity {
                provider: match fields[4] {
                    "JavDB" => "JavDB",
                    "FANZA" => "FANZA",
                    _ => return Err(()),
                },
                provider_id: decode_text(fields[5]).ok_or(())?,
                display_code: decode_text(fields[6]).ok_or(())?,
            })
        };
        let cover = if fields[8] == "ready" {
            Some(CoverSource {
                provider: match fields[9] {
                    "JavDB" => "JavDB",
                    "FANZA" => "FANZA",
                    "r18.dev" => "r18.dev",
                    _ => return Err(()),
                },
                provider_id: decode_text(fields[10]).ok_or(())?,
                display_code: decode_text(fields[11]).ok_or(())?,
                url: decode_text(fields[12]).ok_or(())?,
                aspect_ratio: fields[13].parse().map_err(|_| ())?,
            })
        } else {
            None
        };
        let metadata = PresentationMetadata {
            verified_identity: None,
            identity_conflict: false,
            source: optional_text(fields[16]),
            provider_id: optional_text(fields[17]),
            display_code: optional_text(fields[18]),
            title: optional_text(fields[19]),
            date: optional_text(fields[20]),
            runtime: optional_text(fields[21]),
            cast: fields[22]
                .split(',')
                .filter(|value| !value.is_empty())
                .map(|value| decode_text(value).ok_or(()))
                .collect::<Result<Vec<_>, _>>()?,
        };
        let entry = CacheEntry {
            identity: fields[0].to_owned(),
            category,
            code: decode_text(fields[2]).ok_or(())?,
            identity_saved_at: fields[3].parse().map_err(|_| ())?,
            verified_identity,
            cover_saved_at: fields[7].parse().map_err(|_| ())?,
            cover_state: match fields[8] {
                "ready" => "ready",
                "missing" => "missing",
                _ => return Err(()),
            },
            cover,
            metadata_saved_at: fields[14].parse().map_err(|_| ())?,
            metadata_state: match fields[15] {
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
        let verified_identity = entry.verified_identity.as_ref();
        let fields = [
            entry.identity.clone(),
            entry.category.value().to_owned(),
            encode_text(&entry.code),
            entry.identity_saved_at.to_string(),
            verified_identity
                .map_or("", |identity| identity.provider)
                .to_owned(),
            encode_text(verified_identity.map_or("", |identity| identity.provider_id.as_str())),
            encode_text(verified_identity.map_or("", |identity| identity.display_code.as_str())),
            entry.cover_saved_at.to_string(),
            entry.cover_state.to_owned(),
            cover.map_or("", |cover| cover.provider).to_owned(),
            encode_text(cover.map_or("", |cover| &cover.provider_id)),
            encode_text(cover.map_or("", |cover| &cover.display_code)),
            encode_text(cover.map_or("", |cover| &cover.url)),
            cover
                .map_or(DEFAULT_COVER_RATIO, |cover| cover.aspect_ratio)
                .to_string(),
            entry.metadata_saved_at.to_string(),
            entry.metadata_state.to_owned(),
            encode_text(entry.metadata.source.as_deref().unwrap_or("")),
            encode_text(entry.metadata.provider_id.as_deref().unwrap_or("")),
            encode_text(entry.metadata.display_code.as_deref().unwrap_or("")),
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

fn remove_cache_entry(path: &Path, identity: &str) -> Result<(), ()> {
    let mut entries = load_cache(path);
    let original_len = entries.len();
    entries.retain(|entry| entry.identity != identity);
    if entries.len() == original_len && !path.exists() {
        return Ok(());
    }
    write_cache(path, &entries)
}

fn cover_response(
    state: &LibraryPresentationState,
    authority: &LibraryItemAuthority,
    request_generation: u64,
    cover_state: &'static str,
    cover: Option<CoverSource>,
    bytes: Option<Vec<u8>>,
    verified_identity: Option<VerifiedDisplayIdentity>,
) -> Result<Vec<String>, &'static str> {
    if verified_identity.as_ref().is_some_and(|identity| {
        !verified_display_identity_valid(identity, authority.category, &authority.code)
    }) {
        return Err(LIBRARY_PRESENTATION_FAILED);
    }
    let cover_contract_valid = match cover_state {
        "ready" => cover.as_ref().is_some_and(|source| {
            cover_identity_consistent(
                source,
                verified_identity.as_ref(),
                authority.category,
                &authority.code,
            )
        }),
        "missing" | "unavailable" => cover.is_none(),
        _ => false,
    };
    if !cover_contract_valid {
        return Err(LIBRARY_PRESENTATION_FAILED);
    }
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
        while context.covers.len() >= MAX_RETAINED_COVER_AUTHORITIES {
            let oldest = context
                .covers
                .iter()
                .filter(|(_, retained)| {
                    !cover_request_matches(
                        &context,
                        retained.category,
                        &retained.code,
                        retained.request_generation,
                    )
                })
                .min_by_key(|(_, retained)| retained.sequence)
                .map(|(id, _)| id.clone());
            let Some(oldest) = oldest else {
                return Err(LIBRARY_PRESENTATION_FAILED);
            };
            context.covers.remove(&oldest);
        }
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
        context.failed_cover_sources.remove(&authority.identity);
        cover_authority_id = Some(id);
        while context
            .covers
            .values()
            .filter(|cover| cover.bytes.is_some())
            .count()
            > MAX_RETAINED_COVER_BYTES
        {
            let oldest_bytes = context
                .covers
                .iter()
                .filter(|(_, retained)| retained.bytes.is_some())
                .min_by_key(|(_, retained)| retained.sequence)
                .map(|(id, _)| id.clone());
            let Some(oldest_bytes) = oldest_bytes else {
                break;
            };
            if let Some(retained) = context.covers.get_mut(&oldest_bytes) {
                retained.bytes = None;
            }
        }
    }
    if let Some(identity) = &verified_identity {
        let mut context = state.0.lock().map_err(|_| LIBRARY_PRESENTATION_FAILED)?;
        if !cover_request_matches(
            &context,
            authority.category,
            &authority.code,
            request_generation,
        ) {
            return Err(LIBRARY_PRESENTATION_STALE);
        }
        context.next_cover_sequence = context.next_cover_sequence.wrapping_add(1);
        let sequence = context.next_cover_sequence;
        context.verified_identity_seeds.insert(
            metadata_seed_key(authority, request_generation),
            VerifiedIdentitySeed {
                identity: identity.clone(),
                sequence,
            },
        );
        bound_metadata_seeds(&mut context);
    }
    Ok(vec![
        "library-cover-v3".to_owned(),
        authority.category.value().to_owned(),
        cover_state.to_owned(),
        cover
            .as_ref()
            .map_or_else(String::new, |cover| cover.provider.to_owned()),
        cover
            .as_ref()
            .map_or_else(String::new, |cover| cover.provider_id.clone()),
        cover
            .as_ref()
            .map_or_else(String::new, |cover| cover.display_code.clone()),
        cover_authority_id.unwrap_or_default(),
        cover
            .as_ref()
            .map_or(DEFAULT_COVER_RATIO, |cover| cover.aspect_ratio)
            .to_string(),
        verified_identity
            .as_ref()
            .map_or_else(String::new, |identity| identity.provider.to_owned()),
        verified_identity
            .as_ref()
            .map_or_else(String::new, |identity| identity.provider_id.clone()),
        verified_identity
            .as_ref()
            .map_or_else(String::new, |identity| identity.display_code.clone()),
    ])
}

pub(crate) fn resolve_cover(
    state: &LibraryPresentationState,
    cache_path: &Path,
    authority: &LibraryItemAuthority,
    request_generation: u64,
    is_current: impl Fn() -> bool,
) -> Result<Vec<String>, &'static str> {
    let failed_urls = state
        .0
        .lock()
        .map_err(|_| LIBRARY_PRESENTATION_FAILED)?
        .failed_cover_sources
        .get(&authority.identity)
        .filter(|failed| failed.category == authority.category && failed.code == authority.code)
        .map_or_else(HashSet::new, |failed| failed.urls.clone());
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
                |url| {
                    fetch_current_cover_source(url, &failed_urls, javdb_catalog::fetch_cover_bytes)
                },
                fanza_catalog::fetch_graphql_document,
                |url| {
                    fetch_current_cover_source(url, &failed_urls, fanza_catalog::fetch_cover_bytes)
                },
                fetch_legacy_document,
                |url| fetch_current_cover_source(url, &failed_urls, fetch_legacy_image),
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
    let cached_identity = cache
        .iter()
        .find(|entry| {
            entry.identity == authority.identity
                && entry.category == authority.category
                && entry.code == authority.code
                && entry.identity_saved_at > 0
                && entry.identity_saved_at <= now
                && now.saturating_sub(entry.identity_saved_at) <= METADATA_TTL_SECONDS
        })
        .and_then(|entry| {
            entry
                .verified_identity
                .clone()
                .map(|identity| (entry.identity_saved_at, identity))
        });
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
            entry.verified_identity.clone(),
        );
    }

    let (mut cover, mut metadata_seed, transient) = resolve();
    let identity_conflict = metadata_seed
        .as_ref()
        .is_some_and(|metadata| metadata.identity_conflict);
    if identity_conflict {
        if !is_current() {
            return Err(LIBRARY_PRESENTATION_STALE);
        }
        {
            let mut context = state.0.lock().map_err(|_| LIBRARY_PRESENTATION_FAILED)?;
            remove_cache_entry(cache_path, &authority.identity)
                .map_err(|_| LIBRARY_PRESENTATION_FAILED)?;
            retain_metadata_seed(
                &mut context,
                authority,
                request_generation,
                PresentationMetadata {
                    identity_conflict: true,
                    ..PresentationMetadata::default()
                },
            );
        }
        return cover_response(
            state,
            authority,
            request_generation,
            "unavailable",
            None,
            None,
            None,
        );
    }
    let resolved_identity = match reconcile_verified_identities(
        metadata_seed
            .as_ref()
            .and_then(|metadata| metadata.verified_identity.clone()),
        cover
            .as_ref()
            .and_then(|(source, _)| verified_identity_from_cover(source)),
    ) {
        Ok(identity) => identity,
        Err(()) => {
            if !is_current() {
                return Err(LIBRARY_PRESENTATION_STALE);
            }
            {
                let mut context = state.0.lock().map_err(|_| LIBRARY_PRESENTATION_FAILED)?;
                remove_cache_entry(cache_path, &authority.identity)
                    .map_err(|_| LIBRARY_PRESENTATION_FAILED)?;
                retain_metadata_seed(
                    &mut context,
                    authority,
                    request_generation,
                    PresentationMetadata {
                        identity_conflict: true,
                        ..PresentationMetadata::default()
                    },
                );
            }
            return cover_response(
                state,
                authority,
                request_generation,
                "unavailable",
                None,
                None,
                None,
            );
        }
    };
    let (identity_saved_at, verified_identity) = match resolved_identity {
        Some(identity) => (now, Some(identity)),
        None if transient => cached_identity
            .map(|(saved_at, identity)| (saved_at, Some(identity)))
            .unwrap_or((0, None)),
        None => (0, None),
    };
    if let (Some((source, _)), Some(identity)) = (&mut cover, &verified_identity) {
        if source.provider == "r18.dev" {
            source.display_code = identity.display_code.clone();
        }
    }
    if let Some(metadata) = &mut metadata_seed {
        attach_verified_identity(metadata, verified_identity.clone());
    }
    if cover.as_ref().is_some_and(|(source, _)| {
        !cover_identity_consistent(
            source,
            verified_identity.as_ref(),
            authority.category,
            &authority.code,
        )
    }) {
        return Err(LIBRARY_PRESENTATION_FAILED);
    }
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
            retain_metadata_seed(&mut context, authority, request_generation, metadata);
        }
    }
    let (cover_state, source, bytes) = match cover {
        Some((source, bytes)) => ("ready", Some(source), Some(bytes)),
        None if transient => ("unavailable", None, None),
        None => ("missing", None, None),
    };
    if cover_state != "unavailable" || verified_identity.is_some() {
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
                identity_saved_at: 0,
                verified_identity: None,
                cover_saved_at: 0,
                cover_state: "missing",
                cover: None,
                metadata_saved_at: 0,
                metadata_state: "missing",
                metadata: PresentationMetadata::default(),
            });
        entry.category = authority.category;
        entry.code = authority.code.clone();
        entry.identity_saved_at = identity_saved_at;
        entry.verified_identity = verified_identity.clone();
        if cover_state != "unavailable" {
            entry.cover_saved_at = now;
            entry.cover_state = cover_state;
            entry.cover = source.clone();
        }
        merge_cache_entry(cache_path, entry).map_err(|_| LIBRARY_PRESENTATION_FAILED)?;
    }
    cover_response(
        state,
        authority,
        request_generation,
        cover_state,
        source,
        bytes,
        verified_identity,
    )
}

pub(crate) fn resolve_metadata(
    state: &LibraryPresentationState,
    cache_path: &Path,
    authority: &LibraryItemAuthority,
    request_generation: u64,
    is_current: impl Fn() -> bool,
) -> Result<Vec<String>, &'static str> {
    if request_generation == 0
        || !state.0.lock().is_ok_and(|context| {
            cover_request_matches(
                &context,
                authority.category,
                &authority.code,
                request_generation,
            )
        })
    {
        return Err(LIBRARY_PRESENTATION_STALE);
    }
    resolve_metadata_at_with(
        state,
        cache_path,
        authority,
        now_seconds(),
        is_current,
        || {
            let key = metadata_seed_key(authority, request_generation);
            let (seed, verified_identity) = {
                let mut context = state.0.lock().map_err(|_| ProviderRequestError::Provider)?;
                (
                    context.metadata_seeds.remove(&key),
                    context
                        .verified_identity_seeds
                        .remove(&key)
                        .map(|seed| seed.identity),
                )
            };
            let seeded_metadata = seed.map(|seed| {
                let mut metadata = seed.metadata;
                if metadata.verified_identity.is_none() {
                    metadata.verified_identity = verified_identity;
                }
                metadata
            });
            match seeded_metadata {
                Some(metadata) if metadata.identity_conflict || metadata.is_ready() => {
                    Ok(Some(metadata))
                }
                _ => resolve_metadata_with(
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
    let current_entry = cache.iter().find(|entry| {
        entry.identity == authority.identity
            && entry.category == authority.category
            && entry.code == authority.code
    });
    let current_identity = current_entry
        .filter(|entry| {
            entry.identity_saved_at > 0
                && entry.identity_saved_at <= now
                && now.saturating_sub(entry.identity_saved_at) <= METADATA_TTL_SECONDS
        })
        .and_then(|entry| entry.verified_identity.clone());
    if let Some(entry) = current_entry.filter(|entry| {
        entry.identity == authority.identity
            && entry.category == authority.category
            && entry.code == authority.code
            && entry.metadata_saved_at > 0
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
            entry.verified_identity.as_ref(),
            false,
        ));
    }
    let resolved = resolve();
    if !is_current() {
        return Err(LIBRARY_PRESENTATION_STALE);
    }
    if resolved.as_ref().is_ok_and(|metadata| {
        metadata
            .as_ref()
            .is_some_and(|metadata| metadata.identity_conflict)
    }) {
        {
            let _guard = state.0.lock().map_err(|_| LIBRARY_PRESENTATION_FAILED)?;
            remove_cache_entry(cache_path, &authority.identity)
                .map_err(|_| LIBRARY_PRESENTATION_FAILED)?;
        }
        return Ok(metadata_response(
            authority,
            "unavailable",
            &PresentationMetadata::default(),
            None,
            true,
        ));
    }
    let (metadata_state, mut metadata, resolved_identity) = match resolved {
        Ok(Some(metadata)) => {
            let identity = metadata.verified_identity.clone();
            ("automatic", metadata, identity)
        }
        Ok(None) => ("local-only", PresentationMetadata::default(), None),
        Err(_) => {
            return Ok(metadata_response(
                authority,
                "unavailable",
                &PresentationMetadata::default(),
                current_identity.as_ref(),
                false,
            ))
        }
    };
    let verified_identity =
        match reconcile_verified_identities(current_identity.clone(), resolved_identity) {
            Ok(identity) => identity,
            Err(()) => {
                {
                    let _guard = state.0.lock().map_err(|_| LIBRARY_PRESENTATION_FAILED)?;
                    remove_cache_entry(cache_path, &authority.identity)
                        .map_err(|_| LIBRARY_PRESENTATION_FAILED)?;
                }
                return Ok(metadata_response(
                    authority,
                    "unavailable",
                    &PresentationMetadata::default(),
                    None,
                    true,
                ));
            }
        };
    attach_verified_identity(&mut metadata, verified_identity.clone());
    if metadata_state == "automatic" {
        let _guard = state.0.lock().map_err(|_| LIBRARY_PRESENTATION_FAILED)?;
        let mut entry = cache
            .into_iter()
            .find(|entry| entry.identity == authority.identity)
            .unwrap_or(CacheEntry {
                identity: authority.identity.clone(),
                category: authority.category,
                code: authority.code.clone(),
                identity_saved_at: 0,
                verified_identity: None,
                cover_saved_at: 0,
                cover_state: "missing",
                cover: None,
                metadata_saved_at: 0,
                metadata_state: "missing",
                metadata: PresentationMetadata::default(),
            });
        entry.category = authority.category;
        entry.code = authority.code.clone();
        if verified_identity != current_identity {
            entry.identity_saved_at = now;
        }
        entry.verified_identity = verified_identity.clone();
        if let (Some(source), Some(identity)) = (&mut entry.cover, &verified_identity) {
            if source.provider == "r18.dev" {
                source.display_code = identity.display_code.clone();
            }
        }
        if entry.cover_state == "ready"
            && !entry.cover.as_ref().is_some_and(|source| {
                cover_identity_consistent(
                    source,
                    verified_identity.as_ref(),
                    authority.category,
                    &authority.code,
                )
            })
        {
            remove_cache_entry(cache_path, &authority.identity)
                .map_err(|_| LIBRARY_PRESENTATION_FAILED)?;
            return Ok(metadata_response(
                authority,
                "unavailable",
                &PresentationMetadata::default(),
                None,
                true,
            ));
        }
        entry.metadata_saved_at = now;
        entry.metadata_state = "ready";
        entry.metadata = metadata.clone();
        merge_cache_entry(cache_path, entry).map_err(|_| LIBRARY_PRESENTATION_FAILED)?;
    }
    Ok(metadata_response(
        authority,
        metadata_state,
        &metadata,
        verified_identity.as_ref(),
        false,
    ))
}

fn metadata_response(
    authority: &LibraryItemAuthority,
    state: &str,
    metadata: &PresentationMetadata,
    verified_identity: Option<&VerifiedDisplayIdentity>,
    identity_conflict: bool,
) -> Vec<String> {
    let mut response = vec![
        "library-metadata-v3".to_owned(),
        authority.category.value().to_owned(),
        state.to_owned(),
        if identity_conflict {
            "conflict"
        } else {
            "current"
        }
        .to_owned(),
        verified_identity.map_or_else(String::new, |identity| identity.provider.to_owned()),
        verified_identity.map_or_else(String::new, |identity| identity.provider_id.clone()),
        verified_identity.map_or_else(String::new, |identity| identity.display_code.clone()),
        metadata.source.clone().unwrap_or_default(),
        metadata.provider_id.clone().unwrap_or_default(),
        metadata.display_code.clone().unwrap_or_default(),
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
    if !context.cover_requests.contains_key(&key)
        && context.cover_requests.len() >= MAX_RETAINED_COVER_REQUESTS
        && context
            .cover_requests
            .values()
            .all(|request| request.active)
    {
        return Err(LIBRARY_PRESENTATION_FAILED);
    }
    context
        .covers
        .retain(|_, cover| cover.category != category || cover.code != code);
    context
        .metadata_seeds
        .retain(|seed, _| seed.category != category || seed.code != code);
    context
        .verified_identity_seeds
        .retain(|seed, _| seed.category != category || seed.code != code);
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
        let Some(oldest) = oldest_inactive else {
            break;
        };
        let removed_request = context.cover_requests.get(&oldest).copied();
        context
            .covers
            .retain(|_, cover| cover.category != oldest.0 || cover.code != oldest.1);
        context.cover_requests.remove(&oldest);
        if let Some(removed_request) = removed_request {
            retire_metadata_seed(
                &mut context,
                oldest.0,
                &oldest.1,
                removed_request.generation,
            );
        }
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
        }
    }
    context.covers.retain(|_, cover| {
        cover.category != category
            || cover.code != code
            || cover.request_generation != request_generation
    });
    retire_metadata_seed(&mut context, category, code, request_generation);
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
    context
        .metadata_seeds
        .remove(&metadata_seed_key(authority, request_generation));
    context
        .verified_identity_seeds
        .remove(&metadata_seed_key(authority, request_generation));
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
    let failed_url = source.url.clone();
    entry.cover_saved_at = 0;
    entry.cover_state = "missing";
    entry.cover = None;
    write_cache(cache_path, &entries).map_err(|_| LIBRARY_PRESENTATION_FAILED)?;

    context.next_cover_sequence = context.next_cover_sequence.wrapping_add(1);
    let sequence = context.next_cover_sequence;
    let failed = context
        .failed_cover_sources
        .entry(authority.identity.clone())
        .or_insert_with(|| FailedCoverSources {
            category: authority.category,
            code: authority.code.clone(),
            urls: HashSet::new(),
            sequence,
        });
    failed.urls.insert(failed_url);
    failed.sequence = sequence;
    while context.failed_cover_sources.len() > MAX_RETAINED_FAILED_COVER_ITEMS {
        let oldest_inactive = context
            .failed_cover_sources
            .iter()
            .filter(|(_, failed)| {
                !context
                    .cover_requests
                    .get(&(failed.category, failed.code.clone()))
                    .is_some_and(|request| request.active)
            })
            .min_by_key(|(_, failed)| failed.sequence)
            .map(|(identity, _)| identity.clone());
        let Some(oldest) = oldest_inactive else { break };
        context.failed_cover_sources.remove(&oldest);
    }
    Ok(())
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
        let code = if category == LibraryPresentationCategory::Vr {
            "MDVR-419"
        } else {
            "ADLT-123"
        };
        LibraryItemAuthority {
            category,
            identity: "a".repeat(40),
            code: code.to_owned(),
            product_identity: code.to_owned(),
        }
    }

    fn cover(provider_id: &str, ratio: f64) -> CoverSource {
        CoverSource {
            provider: "JavDB",
            provider_id: provider_id.to_owned(),
            display_code: "MDVR-419".to_owned(),
            url: format!("https://tp.cmastd.com/{provider_id}.jpg"),
            aspect_ratio: ratio,
        }
    }

    fn verified_javdb(provider_id: &str, display_code: &str) -> VerifiedDisplayIdentity {
        VerifiedDisplayIdentity {
            provider: "JavDB",
            provider_id: provider_id.to_owned(),
            display_code: display_code.to_owned(),
        }
    }

    fn verified_fanza(provider_id: &str, display_code: &str) -> VerifiedDisplayIdentity {
        VerifiedDisplayIdentity {
            provider: "FANZA",
            provider_id: provider_id.to_owned(),
            display_code: display_code.to_owned(),
        }
    }

    fn identity_evidence(identity: VerifiedDisplayIdentity) -> PresentationMetadata {
        PresentationMetadata {
            verified_identity: Some(identity),
            ..PresentationMetadata::default()
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
        let authority = LibraryItemAuthority {
            category: LibraryPresentationCategory::Adult,
            identity: "c".repeat(40),
            code: "CAWB-1".to_owned(),
            product_identity: "CAWB-1".to_owned(),
        };
        let events = std::cell::RefCell::new(Vec::new());
        let (cover, metadata, transient) = resolve_cover_with(
            &authority,
            |url| {
                events.borrow_mut().push(format!("javdb:{url}"));
                if url.contains("search") {
                    Ok(javdb_listing("CAWB-001"))
                } else {
                    Ok(javdb_detail("CAWB-001", authority.category))
                }
            },
            |url| {
                events.borrow_mut().push(format!("javdb-image:{url}"));
                Err(ProviderRequestError::Network)
            },
            |body| {
                events.borrow_mut().push(format!("fanza:{body}"));
                Ok(r#"{"data":{"c0":{"id":"cawb00001","contentType":"TWO_DIMENSION","title":"Exact","packageImage":{"largeUrl":"https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/cawb00001/cawb00001pl.jpg"}}}}"#.to_owned())
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
    fn failed_javdb_source_is_skipped_when_retry_advances_to_fanza() {
        let authority = LibraryItemAuthority {
            category: LibraryPresentationCategory::Adult,
            identity: "d".repeat(40),
            code: "CAWB-1".to_owned(),
            product_identity: "CAWB-1".to_owned(),
        };
        let failed_url = "https://tp.cmastd.com/exact.jpg".to_owned();
        let failed_urls = HashSet::from([failed_url]);
        let javdb_image_calls = Cell::new(0);
        let (cover, metadata, transient) = resolve_cover_with(
            &authority,
            |url| {
                if url.contains("search") {
                    Ok(javdb_listing("CAWB-001"))
                } else {
                    Ok(javdb_detail("CAWB-001", authority.category))
                }
            },
            |url| {
                fetch_current_cover_source(url, &failed_urls, |_| {
                    javdb_image_calls.set(javdb_image_calls.get() + 1);
                    Ok(jpeg(600, 800))
                })
            },
            |_| {
                Ok(r#"{"data":{"c0":{"id":"cawb00001","contentType":"TWO_DIMENSION","title":"Exact","packageImage":{"largeUrl":"https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/cawb00001/cawb00001pl.jpg"}}}}"#.to_owned())
            },
            |_| Ok(jpeg(600, 800)),
            |_| panic!("legacy must not run after exact FANZA success"),
            |_| panic!("legacy image must not run"),
        );
        assert_eq!(javdb_image_calls.get(), 0);
        assert_eq!(
            cover.as_ref().map(|(source, _)| source.provider),
            Some("FANZA")
        );
        assert!(metadata.is_some());
        assert!(transient);
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
    fn distinct_javdb_cover_alternatives_dispatch_only_the_preferred_cover() {
        let authority = authority(LibraryPresentationCategory::Vr);
        let image_calls = Cell::new(0);
        let (cover, metadata, transient) = resolve_cover_with(
            &authority,
            |url| {
                Ok(if url.contains("search") {
                    r#"{"success":1,"data":{"movies":[{"id":"item","number":"MDVR-419","title":"Exact listing","cover_url":"https://tp.cmastd.com/listing-cover.jpg","thumb_url":"https://tp.cmastd.com/listing-thumb.jpg"}]}}"#.to_owned()
                } else {
                    r#"{"success":1,"data":{"movie":{"id":"item","number":"MDVR-419","title":"Exact detail","tags":[{"id":"212","name":"VR"}],"cover_url":"https://tp.cmastd.com/detail-cover.jpg","thumb_url":"https://tp.cmastd.com/detail-thumb.jpg"}}}"#.to_owned()
                })
            },
            |url| {
                image_calls.set(image_calls.get() + 1);
                assert_eq!(url, "https://tp.cmastd.com/detail-cover.jpg");
                Ok(jpeg(600, 800))
            },
            |_| panic!("FANZA must not run after the preferred JavDB cover succeeds"),
            |_| panic!("FANZA image must not run"),
            |_| panic!("legacy must not run after the preferred JavDB cover succeeds"),
            |_| panic!("legacy image must not run"),
        );
        let source = cover
            .expect("the preferred exact JavDB cover must resolve")
            .0;
        assert_eq!(source.provider, "JavDB");
        assert_eq!(source.url, "https://tp.cmastd.com/detail-cover.jpg");
        assert_eq!(image_calls.get(), 1);
        assert_eq!(
            metadata.and_then(|value| value.title),
            Some("Exact detail".to_owned())
        );
        assert!(!transient);
    }

    #[test]
    fn exact_3dsvr_identity_reaches_javdb_and_then_exact_category_fanza_fallback() {
        let authority = LibraryItemAuthority {
            category: LibraryPresentationCategory::Vr,
            identity: "3".repeat(40),
            code: "3DSVR-01871".to_owned(),
            product_identity: "3DSVR-1871".to_owned(),
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
        assert_eq!(fanza.0.display_code, "3DSVR-01871");
    }

    #[test]
    fn fanza_only_3dsvr_cover_preserves_the_evidence_backed_display_width() {
        let authority = LibraryItemAuthority {
            category: LibraryPresentationCategory::Vr,
            identity: "3".repeat(40),
            code: "3DSVR-01871".to_owned(),
            product_identity: "3DSVR-1871".to_owned(),
        };
        let (cover, metadata, transient) = resolve_cover_with(
            &authority,
            |_| Ok(r#"{"success":1,"data":{"movies":[]}}"#.to_owned()),
            |_| panic!("a missing JavDB item has no image"),
            |body| {
                assert!(body.contains("c0:ppvContent(id:\"13dsvr01871\")"));
                Ok(r#"{"data":{"c0":{"id":"13dsvr01871","contentType":"VR","title":"Exact VR","packageImage":{"largeUrl":"https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/13dsvr01871/13dsvr01871pl.jpg"}}}}"#.to_owned())
            },
            |_| Ok(jpeg(600, 800)),
            |_| panic!("legacy must not run after exact FANZA success"),
            |_| panic!("legacy image must not run"),
        );
        let source = cover.expect("the FANZA-only 3DSVR cover must resolve").0;
        assert_eq!(source.provider, "FANZA");
        assert_eq!(source.provider_id, "13dsvr01871");
        assert_eq!(source.display_code, "3DSVR-01871");
        assert!(metadata.is_some_and(|metadata| !metadata.is_ready()));
        assert!(!transient);
    }

    #[test]
    fn fanza_exact_identity_survives_a_genuine_missing_package_image() {
        for (category, code, identity, content_id, content_type, display_code) in [
            (
                LibraryPresentationCategory::Adult,
                "CAWB-1",
                "CAWB-1",
                "cawb00001",
                "TWO_DIMENSION",
                "CAWB-001",
            ),
            (
                LibraryPresentationCategory::Vr,
                "3DSVR-01871",
                "3DSVR-1871",
                "13dsvr01871",
                "VR",
                "3DSVR-01871",
            ),
        ] {
            let authority = LibraryItemAuthority {
                category,
                identity: hex_sha1(code.as_bytes()),
                code: code.to_owned(),
                product_identity: identity.to_owned(),
            };
            let (cover, evidence, transient) = resolve_cover_with(
                &authority,
                |_| Ok(r#"{"success":1,"data":{"movies":[]}}"#.to_owned()),
                |_| panic!("a missing JavDB item has no image"),
                |_| {
                    Ok(format!(
                        r#"{{"data":{{"c0":{{"id":"{content_id}","contentType":"{content_type}","title":"Exact","packageImage":null}}}}}}"#
                    ))
                },
                |_| panic!("a null FANZA package image has no image request"),
                |_| Ok(format!(r#"{{"content_id":"{identity}","images":null}}"#)),
                |_| panic!("a null legacy image has no image request"),
            );
            assert!(cover.is_none());
            assert!(!transient);
            assert_eq!(
                evidence.and_then(|evidence| evidence.verified_identity),
                Some(verified_fanza(content_id, display_code))
            );
        }
    }

    #[test]
    fn cawb_identity_keeps_verified_javdb_display_across_fanza_transport_fallback() {
        let authority = LibraryItemAuthority {
            category: LibraryPresentationCategory::Adult,
            identity: "4".repeat(40),
            code: "CAWB-1".to_owned(),
            product_identity: "CAWB-1".to_owned(),
        };
        let javdb_requests = std::cell::RefCell::new(Vec::new());
        let (resolved, metadata, transient) = resolve_cover_with(
            &authority,
            |url| {
                javdb_requests.borrow_mut().push(url.to_owned());
                if url.contains("search?q=CAWB-1&") {
                    return Ok(r#"{"success":1,"data":{"movies":[]}}"#.to_owned());
                }
                if url.contains("search?q=CAWB-001&") {
                    return Ok(r#"{"success":1,"data":{"movies":[{"id":"item","number":"CAWB-001","title":"Provider title","tags":[],"cover_url":null}]}}"#.to_owned());
                }
                Ok(r#"{"success":1,"data":{"movie":{"id":"item","number":"CAWB-001","title":"Provider title","release_date":"2024-01-02","duration":90,"actors":[{"name":"Actor"}],"tags":[],"cover_url":null}}}"#.to_owned())
            },
            |_| panic!("JavDB without a cover must not dispatch an image"),
            |body| {
                assert!(body.contains("c0:ppvContent(id:\"cawb00001\")"));
                Ok(r#"{"data":{"c0":{"id":"cawb00001","contentType":"TWO_DIMENSION","title":"Exact Adult","packageImage":{"largeUrl":"https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/cawb00001/cawb00001pl.jpg"}}}}"#.to_owned())
            },
            |_| Ok(jpeg(600, 800)),
            |_| panic!("legacy must not run after exact FANZA success"),
            |_| panic!("legacy image must not run"),
        );
        let source = resolved
            .expect("the exact FANZA package cover must resolve")
            .0;
        assert_eq!(source.provider, "FANZA");
        assert_eq!(source.provider_id, "cawb00001");
        assert_eq!(source.display_code, "CAWB-001");
        assert_eq!(
            metadata.and_then(|metadata| metadata.display_code),
            Some("CAWB-001".to_owned())
        );
        let javdb_requests = javdb_requests.into_inner();
        assert_eq!(javdb_requests.len(), 3);
        assert!(javdb_requests[0].contains("search?q=CAWB-1&"));
        assert!(javdb_requests[1].contains("search?q=CAWB-001&"));
        assert!(javdb_requests[2].contains("/api/v4/movies/item?"));
        assert!(!transient);
    }

    #[test]
    fn exact_cross_provider_display_agreement_keeps_javdb_identity_for_cawb_and_3dsvr() {
        for (authority, display_code, content_id, content_type) in [
            (
                LibraryItemAuthority {
                    category: LibraryPresentationCategory::Adult,
                    identity: "1".repeat(40),
                    code: "CAWB-1".to_owned(),
                    product_identity: "CAWB-1".to_owned(),
                },
                "CAWB-001",
                "cawb00001",
                "TWO_DIMENSION",
            ),
            (
                LibraryItemAuthority {
                    category: LibraryPresentationCategory::Vr,
                    identity: "2".repeat(40),
                    code: "3DSVR-01871".to_owned(),
                    product_identity: "3DSVR-1871".to_owned(),
                },
                "3DSVR-01871",
                "13dsvr01871",
                "VR",
            ),
        ] {
            let (cover, metadata, transient) = resolve_cover_with(
                &authority,
                |url| {
                    Ok(if url.contains("search") {
                        javdb_listing(display_code).replace(
                            r#""cover_url":"https://tp.cmastd.com/exact.jpg""#,
                            r#""cover_url":null"#,
                        )
                    } else {
                        javdb_detail(display_code, authority.category).replace(
                            r#""cover_url":"https://tp.cmastd.com/exact.jpg""#,
                            r#""cover_url":null"#,
                        )
                    })
                },
                |_| panic!("a null JavDB cover must not dispatch an image"),
                |_| {
                    Ok(format!(
                        r#"{{"data":{{"c0":{{"id":"{content_id}","contentType":"{content_type}","title":"Exact","packageImage":{{"largeUrl":"https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/{content_id}/{content_id}pl.jpg"}}}}}}}}"#
                    ))
                },
                |_| Ok(jpeg(600, 800)),
                |_| panic!("legacy must not run after exact FANZA cover success"),
                |_| panic!("legacy image must not run"),
            );
            let source = cover.expect("the agreed FANZA cover must resolve").0;
            assert_eq!(source.provider, "FANZA");
            assert_eq!(source.display_code, display_code);
            assert_eq!(
                metadata.and_then(|metadata| metadata.verified_identity),
                Some(verified_javdb("item", display_code))
            );
            assert!(!transient);
        }
    }

    #[test]
    fn canonical_only_cross_provider_agreement_is_a_durable_conflict() {
        for (index, authority, javdb_display, content_id, content_type, fanza_display) in [
            (
                0,
                LibraryItemAuthority {
                    category: LibraryPresentationCategory::Adult,
                    identity: "3".repeat(40),
                    code: "CAWB-1".to_owned(),
                    product_identity: "CAWB-1".to_owned(),
                },
                "CAWB-1",
                "cawb00001",
                "TWO_DIMENSION",
                "CAWB-001",
            ),
            (
                1,
                LibraryItemAuthority {
                    category: LibraryPresentationCategory::Vr,
                    identity: "4".repeat(40),
                    code: "3DSVR-01871".to_owned(),
                    product_identity: "3DSVR-1871".to_owned(),
                },
                "3DSVR-1871",
                "13dsvr01871",
                "VR",
                "3DSVR-01871",
            ),
        ] {
            let fixture = CacheFixture::new(&format!("identity-conflict-{index}"));
            write_cache(
                &fixture.path,
                &[CacheEntry {
                    identity: authority.identity.clone(),
                    category: authority.category,
                    code: authority.code.clone(),
                    identity_saved_at: 90,
                    verified_identity: Some(verified_fanza(content_id, fanza_display)),
                    cover_saved_at: 0,
                    cover_state: "missing",
                    cover: None,
                    metadata_saved_at: 0,
                    metadata_state: "missing",
                    metadata: PresentationMetadata::default(),
                }],
            )
            .expect("the previous exact identity must be durable");
            let legacy_calls = Cell::new(0);
            let state = LibraryPresentationState::default();
            let response = resolve_cover_at_with(
                &state,
                &fixture.path,
                &authority,
                100,
                || true,
                || {
                    resolve_cover_with(
                        &authority,
                        |url| {
                            Ok(if url.contains("search") {
                                javdb_listing(javdb_display).replace(
                                    r#""cover_url":"https://tp.cmastd.com/exact.jpg""#,
                                    r#""cover_url":null"#,
                                )
                            } else {
                                javdb_detail(javdb_display, authority.category).replace(
                                    r#""cover_url":"https://tp.cmastd.com/exact.jpg""#,
                                    r#""cover_url":null"#,
                                )
                            })
                        },
                        |_| panic!("a null JavDB cover must not dispatch an image"),
                        |_| {
                            Ok(format!(
                                r#"{{"data":{{"c0":{{"id":"{content_id}","contentType":"{content_type}","title":"Exact","packageImage":null}}}}}}"#
                            ))
                        },
                        |_| panic!("a conflicting FANZA row must not dispatch an image"),
                        |_| {
                            legacy_calls.set(legacy_calls.get() + 1);
                            Ok(r#"{"content_id":"ignored","images":null}"#.to_owned())
                        },
                        |_| panic!("legacy image must not run after an identity conflict"),
                    )
                },
            )
            .expect("an identity conflict must remain a current unavailable state");
            assert_eq!(response[2], "unavailable");
            assert_eq!(&response[8..], ["", "", ""]);
            assert_eq!(legacy_calls.get(), 0);
            assert!(read_cache(&fixture.path)
                .expect("the conflict cleanup must leave a valid cache")
                .is_empty());
            let conflict_seed = state
                .0
                .lock()
                .expect("the conflict seed must remain readable")
                .metadata_seeds
                .remove(&metadata_seed_key(&authority, 0))
                .expect("the metadata phase must retain the cover conflict")
                .metadata;
            let metadata_response = resolve_metadata_at_with(
                &state,
                &fixture.path,
                &authority,
                100,
                || true,
                || Ok(Some(conflict_seed)),
            )
            .expect("metadata must expose the retained conflict without selecting one side");
            assert_eq!(metadata_response[2], "unavailable");
            assert_eq!(metadata_response[3], "conflict");
            assert_eq!(&metadata_response[4..7], ["", "", ""]);
            assert!(read_cache(&fixture.path)
                .expect("metadata conflict cleanup must leave a valid cache")
                .is_empty());

            let restarted_resolution = Cell::new(false);
            let restarted = resolve_cover_at_with(
                &LibraryPresentationState::default(),
                &fixture.path,
                &authority,
                101,
                || true,
                || {
                    restarted_resolution.set(true);
                    (None, None, false)
                },
            )
            .expect("restart must not restore the conflicted identity");
            assert!(restarted_resolution.get());
            assert_eq!(restarted[2], "missing");
            assert_eq!(&restarted[8..], ["", "", ""]);
        }
    }

    #[test]
    fn failed_first_javdb_form_advances_directly_to_fanza() {
        let authority = LibraryItemAuthority {
            category: LibraryPresentationCategory::Adult,
            identity: "7".repeat(40),
            code: "CAWB-1".to_owned(),
            product_identity: "CAWB-1".to_owned(),
        };
        let javdb_requests = Cell::new(0);
        let (cover, metadata, transient) = resolve_cover_with(
            &authority,
            |_| {
                javdb_requests.set(javdb_requests.get() + 1);
                Err(ProviderRequestError::Network)
            },
            |_| panic!("a failed JavDB document must not dispatch an image"),
            |_| {
                Ok(r#"{"data":{"c0":{"id":"cawb00001","contentType":"TWO_DIMENSION","title":"Exact FANZA","packageImage":{"largeUrl":"https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/cawb00001/cawb00001pl.jpg"}}}}"#.to_owned())
            },
            |_| Ok(jpeg(600, 800)),
            |_| panic!("legacy must not run after exact FANZA success"),
            |_| panic!("legacy image must not run"),
        );
        let source = cover
            .expect("FANZA must remain eligible after a JavDB failure")
            .0;
        assert_eq!(source.provider, "FANZA");
        assert_eq!(javdb_requests.get(), 1);
        assert!(metadata.is_some_and(|metadata| !metadata.is_ready()));
        assert!(transient);
    }

    #[test]
    fn failed_javdb_cover_data_dispatches_no_image_and_later_fanza_can_succeed() {
        let authority = LibraryItemAuthority {
            category: LibraryPresentationCategory::Adult,
            identity: "6".repeat(40),
            code: "CAWB-1".to_owned(),
            product_identity: "CAWB-1".to_owned(),
        };
        for document in [
            r#"{"success":1,"data":{"movies":[{"id":"Same","number":"CAWB-1","title":"First","cover_url":"https://tp.cmastd.com/first.jpg"},{"id":"Same","number":"CAWB-001","title":"Second","cover_url":"https://tp.cmastd.com/second.jpg"}]}}"#,
            r#"{"success":1,"data":{"movies":[{"id":"Same","number":"CAWB-1","cover_url":42}]}}"#,
            r#"{"success":1,"data":{"movies":[{"id":"Same","number":"CAWB-1","cover_url":"https://tp.cmastd.com.evil.example/cover.jpg"}]}}"#,
        ] {
            let javdb_requests = Cell::new(0);
            let javdb_images = Cell::new(0);
            let (cover, metadata, transient) = resolve_cover_with(
                &authority,
                |_| {
                    javdb_requests.set(javdb_requests.get() + 1);
                    Ok(document.to_owned())
                },
                |_| {
                    javdb_images.set(javdb_images.get() + 1);
                    Ok(jpeg(600, 800))
                },
                |_| {
                    Ok(r#"{"data":{"c0":{"id":"cawb00001","contentType":"TWO_DIMENSION","title":"Exact FANZA","packageImage":{"largeUrl":"https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/cawb00001/cawb00001pl.jpg"}}}}"#.to_owned())
                },
                |_| Ok(jpeg(600, 800)),
                |_| panic!("legacy must not run after exact FANZA success"),
                |_| panic!("legacy image must not run"),
            );
            let source = cover.expect("the later exact FANZA cover must resolve").0;
            assert_eq!(source.provider, "FANZA");
            assert_eq!(javdb_requests.get(), 1);
            assert_eq!(javdb_images.get(), 0);
            assert!(metadata.is_some_and(|metadata| !metadata.is_ready()));
            assert!(transient);
        }
    }

    #[test]
    fn mismatched_fanza_category_and_code_cannot_supply_a_cover() {
        for document in [
            r#"{"data":{"c0":{"id":"cawb00002","contentType":"TWO_DIMENSION","title":"Wrong","packageImage":{"largeUrl":"https://awsimgsrc.dmm.co.jp/wrong.jpg"}}}}"#,
            r#"{"data":{"c0":{"id":"cawb00001","contentType":"VR","title":"Wrong","packageImage":{"largeUrl":"https://awsimgsrc.dmm.co.jp/wrong.jpg"}}}}"#,
        ] {
            let authority = LibraryItemAuthority {
                category: LibraryPresentationCategory::Adult,
                identity: "e".repeat(40),
                code: "CAWB-1".to_owned(),
                product_identity: "CAWB-1".to_owned(),
            };
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
    fn failed_fanza_row_contributes_nothing_and_later_exact_source_can_succeed() {
        let authority = LibraryItemAuthority {
            category: LibraryPresentationCategory::Adult,
            identity: "a".repeat(40),
            code: "CAWB-1".to_owned(),
            product_identity: "CAWB-1".to_owned(),
        };
        for document in [
            r#"{"data":{"c0":{"id":"cawb001","contentType":"TWO_DIMENSION","title":"Conflicting FANZA title","packageImage":{"largeUrl":"https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/cawb001/cawb001pl.jpg"}}}}"#,
            r#"{"data":{"c0":{"id":"cawb00001","contentType":"TWO_DIMENSION","title":"Malformed FANZA cover","packageImage":{"largeUrl":" https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/cawb00001/cawb00001pl.jpg "}}}}"#,
            r#"{"data":{"c0":{"id":"cawb00001","contentType":"TWO_DIMENSION","title":"Neighbor FANZA cover","packageImage":{"largeUrl":"https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/cawb00002/cawb00002pl.jpg"}}}}"#,
        ] {
            let fanza_image_calls = Cell::new(0);
            let (cover, metadata, transient) = resolve_cover_with(
                &authority,
                |_| Err(ProviderRequestError::SourceUnavailable),
                |_| panic!("missing JavDB item has no image"),
                |_| Ok(document.to_owned()),
                |_| {
                    fanza_image_calls.set(fanza_image_calls.get() + 1);
                    Ok(jpeg(600, 800))
                },
                |_| {
                    Ok(r#"{"content_id":"CAWB-1","title_ja":"Legacy title","images":{"jacket_image":{"large":"https://pics.dmm.co.jp/digital/video/cawb00001/cawb00001pl.jpg"}}}"#.to_owned())
                },
                |_| Ok(jpeg(600, 800)),
            );
            let source = cover
                .expect("the later exact legacy source must remain eligible")
                .0;
            assert_eq!(source.provider, "r18.dev");
            assert_eq!(source.display_code, "CAWB-1");
            assert_eq!(fanza_image_calls.get(), 0);
            assert_eq!(
                metadata.and_then(|value| value.title),
                Some("Legacy title".to_owned())
            );
            assert!(transient);
        }
    }

    #[test]
    fn failed_fanza_row_without_later_success_is_unavailable_and_not_cached() {
        let authority = LibraryItemAuthority {
            category: LibraryPresentationCategory::Adult,
            identity: "b".repeat(40),
            code: "CAWB-1".to_owned(),
            product_identity: "CAWB-1".to_owned(),
        };
        for (index, document) in [
            r#"{"data":{"c0":{"id":"cawb00002","contentType":"TWO_DIMENSION","title":"Wrong","packageImage":{"largeUrl":"https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/cawb00002/cawb00002pl.jpg"}}}}"#,
            r#"{"data":{"c0":{"id":"cawb00001","contentType":"TWO_DIMENSION","title":"Malformed FANZA cover","packageImage":{"largeUrl":42}}}}"#,
            r#"{"data":{"c0":{"id":"cawb00001","contentType":"TWO_DIMENSION","title":"Neighbor FANZA cover","packageImage":{"largeUrl":"https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/cawb00002/cawb00002pl.jpg"}}}}"#,
        ]
        .into_iter()
        .enumerate()
        {
            let fixture = CacheFixture::new(&format!("fanza-failure-{index}"));
            let fanza_image_calls = Cell::new(0);
            let response = resolve_cover_at_with(
                &LibraryPresentationState::default(),
                &fixture.path,
                &authority,
                100,
                || true,
                || {
                    resolve_cover_with(
                        &authority,
                        |_| Err(ProviderRequestError::SourceUnavailable),
                        |_| panic!("missing JavDB item has no image"),
                        |_| Ok(document.to_owned()),
                        |_| {
                            fanza_image_calls.set(fanza_image_calls.get() + 1);
                            Ok(jpeg(600, 800))
                        },
                        |_| Err(ProviderRequestError::SourceUnavailable),
                        |_| panic!("missing legacy item has no image"),
                    )
                },
            )
            .expect("a current provider failure must remain a local state");
            assert_eq!(response[2], "unavailable");
            assert_eq!(response[3], "");
            assert_eq!(response[4], "");
            assert_eq!(response[5], "");
            assert_eq!(fanza_image_calls.get(), 0);
            assert!(!fixture.path.exists());
        }
    }

    #[test]
    fn failed_javdb_or_legacy_cover_data_is_unavailable_and_never_cached_as_a_miss() {
        let authority = LibraryItemAuthority {
            category: LibraryPresentationCategory::Adult,
            identity: "5".repeat(40),
            code: "CAWB-1".to_owned(),
            product_identity: "CAWB-1".to_owned(),
        };
        let javdb_fixture = CacheFixture::new("failed-javdb-no-miss");
        let javdb_images = Cell::new(0);
        let response = resolve_cover_at_with(
            &LibraryPresentationState::default(),
            &javdb_fixture.path,
            &authority,
            100,
            || true,
            || {
                resolve_cover_with(
                    &authority,
                    |_| {
                        Ok(r#"{"success":1,"data":{"movies":[{"id":"Same","number":"CAWB-1","cover_url":42}]}}"#.to_owned())
                    },
                    |_| {
                        javdb_images.set(javdb_images.get() + 1);
                        Ok(jpeg(600, 800))
                    },
                    |_| Ok(r#"{"data":{"c0":null}}"#.to_owned()),
                    |_| panic!("an absent FANZA item has no image"),
                    |_| Ok(r#"{"content_id":"CAWB-1","images":null}"#.to_owned()),
                    |_| panic!("an absent legacy cover has no image"),
                )
            },
        )
        .expect("a failed exact source must remain a current local state");
        assert_eq!(response[2], "unavailable");
        assert_eq!(javdb_images.get(), 0);
        assert!(!javdb_fixture.path.exists());

        for (index, legacy_cover) in [
            r#""images":[]"#,
            r#""images":{"jacket_image":{"large":42}}"#,
            r#""images":{"jacket_image":{"large":"https://pics.dmm.co.jp/ unsafe.jpg"}}"#,
            r#""images":{"jacket_image":{"large2":"https://pics.dmm.co.jp/first.jpg","large":"https://pics.dmm.co.jp/ unsafe.jpg"}}"#,
        ]
        .into_iter()
        .enumerate()
        {
            let fixture = CacheFixture::new(&format!("failed-legacy-no-miss-{index}"));
            let legacy_images = Cell::new(0);
            let response = resolve_cover_at_with(
                &LibraryPresentationState::default(),
                &fixture.path,
                &authority,
                100,
                || true,
                || {
                    resolve_cover_with(
                        &authority,
                        |_| Ok(r#"{"success":1,"data":{"movies":[]}}"#.to_owned()),
                        |_| panic!("an empty JavDB result has no image"),
                        |_| Ok(r#"{"data":{"c0":null}}"#.to_owned()),
                        |_| panic!("an absent FANZA item has no image"),
                        |_| {
                            Ok(format!(
                                r#"{{"content_id":"CAWB-1",{legacy_cover}}}"#
                            ))
                        },
                        |_| {
                            legacy_images.set(legacy_images.get() + 1);
                            Ok(jpeg(600, 800))
                        },
                    )
                },
            )
            .expect("malformed legacy cover data must remain a current local state");
            assert_eq!(response[2], "unavailable");
            assert_eq!(legacy_images.get(), 0);
            assert!(!fixture.path.exists());
        }
    }

    #[test]
    fn exact_absent_and_null_cover_results_create_a_reusable_confirmed_miss() {
        let authority = LibraryItemAuthority {
            category: LibraryPresentationCategory::Adult,
            identity: "4".repeat(40),
            code: "CAWB-1".to_owned(),
            product_identity: "CAWB-1".to_owned(),
        };
        for (index, javdb_cover, fanza_cover, legacy_cover) in [
            (0, "", "", ""),
            (
                1,
                r#","cover_url":null"#,
                r#","packageImage":null"#,
                r#","images":null"#,
            ),
        ] {
            let fixture = CacheFixture::new(&format!("confirmed-provider-miss-{index}"));
            let response = resolve_cover_at_with(
                &LibraryPresentationState::default(),
                &fixture.path,
                &authority,
                100,
                || true,
                || {
                    resolve_cover_with(
                        &authority,
                        |url| {
                            Ok(if url.contains("search") {
                                r#"{"success":1,"data":{"movies":[{"id":"Exact","number":"CAWB-001"$COVER}]}}"#
                                    .replace("$COVER", javdb_cover)
                            } else {
                                r#"{"success":1,"data":{"movie":{"id":"Exact","number":"CAWB-001","tags":[]$COVER}}}"#
                                    .replace("$COVER", javdb_cover)
                            })
                        },
                        |_| panic!("an absent JavDB cover has no image"),
                        |_| {
                            Ok(r#"{"data":{"c0":{"id":"cawb00001","contentType":"TWO_DIMENSION"$COVER}}}"#
                                .replace("$COVER", fanza_cover))
                        },
                        |_| panic!("an absent FANZA cover has no image"),
                        |_| {
                            Ok(r#"{"content_id":"CAWB-1"$COVER}"#
                                .replace("$COVER", legacy_cover))
                        },
                        |_| panic!("an absent legacy cover has no image"),
                    )
                },
            )
            .expect("genuine absent cover data must establish a confirmed miss");
            assert_eq!(response[2], "missing");
            assert_eq!(&response[8..], ["JavDB", "Exact", "CAWB-001"]);
            assert!(fixture.path.exists());

            let cached = resolve_cover_at_with(
                &LibraryPresentationState::default(),
                &fixture.path,
                &authority,
                101,
                || true,
                || panic!("a fresh confirmed miss must skip provider rediscovery"),
            )
            .expect("the confirmed miss must remain reusable");
            assert_eq!(cached[2], "missing");
            assert_eq!(&cached[8..], ["JavDB", "Exact", "CAWB-001"]);
        }
    }

    #[test]
    fn verified_display_identity_survives_missing_and_unavailable_cover_outcomes() {
        let fixture = CacheFixture::new("verified-display-without-cover");
        let authority = LibraryItemAuthority {
            category: LibraryPresentationCategory::Adult,
            identity: "9".repeat(40),
            code: "CAWB-1".to_owned(),
            product_identity: "CAWB-1".to_owned(),
        };
        let identity = verified_fanza("cawb00001", "CAWB-001");
        let response = resolve_cover_at_with(
            &LibraryPresentationState::default(),
            &fixture.path,
            &authority,
            100,
            || true,
            || (None, Some(identity_evidence(identity.clone())), false),
        )
        .expect("a verified identity with no cover must remain a confirmed result");
        assert_eq!(response[2], "missing");
        assert_eq!(&response[8..], ["FANZA", "cawb00001", "CAWB-001"]);

        let restarted = resolve_cover_at_with(
            &LibraryPresentationState::default(),
            &fixture.path,
            &authority,
            101,
            || true,
            || panic!("a fresh verified missing-cover cache must survive restart"),
        )
        .expect("the verified display identity must load after restart");
        assert_eq!(&restarted[8..], ["FANZA", "cawb00001", "CAWB-001"]);

        let failed_refresh = resolve_cover_at_with(
            &LibraryPresentationState::default(),
            &fixture.path,
            &authority,
            101 + COVER_TTL_SECONDS,
            || true,
            || (None, None, true),
        )
        .expect("an expired cover refresh must retain durable verified identity");
        assert_eq!(failed_refresh[2], "unavailable");
        assert_eq!(&failed_refresh[8..], ["FANZA", "cawb00001", "CAWB-001"]);
        let failed_restart = resolve_cover_at_with(
            &LibraryPresentationState::default(),
            &fixture.path,
            &authority,
            102 + COVER_TTL_SECONDS,
            || true,
            || (None, None, true),
        )
        .expect("a restarted transient failure must keep the durable identity proof");
        assert_eq!(&failed_restart[8..], ["FANZA", "cawb00001", "CAWB-001"]);

        let unavailable = resolve_cover_at_with(
            &LibraryPresentationState::default(),
            &CacheFixture::new("verified-display-cover-failure").path,
            &authority,
            100,
            || true,
            || (None, Some(identity_evidence(identity)), true),
        )
        .expect("cover failure must not discard accepted display identity");
        assert_eq!(unavailable[2], "unavailable");
        assert_eq!(&unavailable[8..], ["FANZA", "cawb00001", "CAWB-001"]);
    }

    #[test]
    fn later_legacy_cover_does_not_replace_verified_provider_display_identity() {
        let fixture = CacheFixture::new("legacy-cover-verified-display");
        let authority = LibraryItemAuthority {
            category: LibraryPresentationCategory::Adult,
            identity: "7".repeat(40),
            code: "CAWB-1".to_owned(),
            product_identity: "CAWB-1".to_owned(),
        };
        let legacy = CoverSource {
            provider: "r18.dev",
            provider_id: "CAWB-1".to_owned(),
            display_code: "CAWB-001".to_owned(),
            url: "https://pics.dmm.co.jp/digital/video/cawb00001/cawb00001pl.jpg".to_owned(),
            aspect_ratio: 0.72,
        };
        let response = resolve_cover_at_with(
            &LibraryPresentationState::default(),
            &fixture.path,
            &authority,
            100,
            || true,
            || {
                (
                    Some((legacy, jpeg(600, 800))),
                    Some(identity_evidence(verified_fanza("cawb00001", "CAWB-001"))),
                    false,
                )
            },
        )
        .expect("legacy image may accompany retained exact display identity");
        assert_eq!(response[3], "r18.dev");
        assert_eq!(&response[8..], ["FANZA", "cawb00001", "CAWB-001"]);
    }

    #[test]
    fn invalid_cached_display_proof_is_a_miss_and_current_resolution_replaces_it() {
        let fixture = CacheFixture::new("invalid-display-proof");
        let authority = LibraryItemAuthority {
            category: LibraryPresentationCategory::Adult,
            identity: "6".repeat(40),
            code: "CAWB-1".to_owned(),
            product_identity: "CAWB-1".to_owned(),
        };
        write_cache(
            &fixture.path,
            &[CacheEntry {
                identity: authority.identity.clone(),
                category: authority.category,
                code: authority.code.clone(),
                identity_saved_at: 100,
                verified_identity: Some(verified_fanza("cawb00001", "CAWB-1")),
                cover_saved_at: 100,
                cover_state: "missing",
                cover: None,
                metadata_saved_at: 0,
                metadata_state: "missing",
                metadata: PresentationMetadata::default(),
            }],
        )
        .expect("invalid proof fixture must be writable");
        let refreshed = Cell::new(false);
        let response = resolve_cover_at_with(
            &LibraryPresentationState::default(),
            &fixture.path,
            &authority,
            101,
            || true,
            || {
                refreshed.set(true);
                (
                    None,
                    Some(identity_evidence(verified_fanza("cawb00001", "CAWB-001"))),
                    false,
                )
            },
        )
        .expect("fresh exact evidence must replace invalid persisted identity");
        assert!(refreshed.get());
        assert_eq!(&response[8..], ["FANZA", "cawb00001", "CAWB-001"]);
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
        assert_eq!(cached[7], (16.0 / 9.0).to_string());
        let fetched = fetch_cover_with(&restarted, &authority, &cached[6], |source| {
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
            product_identity: "MDVR-420".to_owned(),
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
        let authority_id = &resolved[6];
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
            fetch_cover_with(&restarted, &authority, &cached[6], |_| {
                Err(ProviderRequestError::Network)
            }),
            Err(LIBRARY_PRESENTATION_FAILED)
        );
        assert!(restarted.0.lock().unwrap().covers.is_empty());
        assert_eq!(
            fetch_cover_with(&restarted, &authority, &cached[6], |_| {
                panic!("a failed source must be single use")
            }),
            Err(LIBRARY_PRESENTATION_STALE)
        );
    }

    #[test]
    fn metadata_seed_is_retired_when_the_cover_request_unmounts_before_metadata() {
        let fixture = CacheFixture::new("metadata-seed-unmount");
        let authority = authority(LibraryPresentationCategory::Adult);
        let state = LibraryPresentationState::default();
        begin_cover_request(&state, authority.category, &authority.code, 1)
            .expect("cover request must begin");
        resolve_cover_at_with_request(
            &state,
            &fixture.path,
            &authority,
            1,
            100,
            || true,
            || {
                (
                    None,
                    Some(PresentationMetadata {
                        verified_identity: Some(verified_javdb("olditem", &authority.code)),
                        source: Some("JavDB".to_owned()),
                        provider_id: Some("olditem".to_owned()),
                        display_code: Some(authority.code.clone()),
                        title: Some("Old metadata".to_owned()),
                        ..PresentationMetadata::default()
                    }),
                    true,
                )
            },
        )
        .expect("cover-first metadata may be retained for the current request");
        assert_eq!(state.0.lock().unwrap().metadata_seeds.len(), 1);
        assert_eq!(state.0.lock().unwrap().verified_identity_seeds.len(), 1);

        cancel_cover_request(&state, authority.category, &authority.code, 1)
            .expect("unmount cancellation must retire the matching seed");
        assert!(state.0.lock().unwrap().metadata_seeds.is_empty());
        assert!(state.0.lock().unwrap().verified_identity_seeds.is_empty());
    }

    #[test]
    fn metadata_seed_cannot_cross_replacement_or_obsolete_cancellation() {
        let fixture = CacheFixture::new("metadata-seed-replacement");
        let authority = authority(LibraryPresentationCategory::Adult);
        let state = LibraryPresentationState::default();
        begin_cover_request(&state, authority.category, &authority.code, 1)
            .expect("old cover request must begin");
        resolve_cover_at_with_request(
            &state,
            &fixture.path,
            &authority,
            1,
            100,
            || true,
            || {
                (
                    None,
                    Some(PresentationMetadata {
                        verified_identity: Some(verified_javdb("olditem", &authority.code)),
                        source: Some("JavDB".to_owned()),
                        provider_id: Some("olditem".to_owned()),
                        display_code: Some(authority.code.clone()),
                        title: Some("Old metadata".to_owned()),
                        ..PresentationMetadata::default()
                    }),
                    true,
                )
            },
        )
        .expect("old cover request may retain one seed");

        begin_cover_request(&state, authority.category, &authority.code, 2)
            .expect("replacement cover request must begin");
        assert!(state.0.lock().unwrap().metadata_seeds.is_empty());
        assert!(state.0.lock().unwrap().verified_identity_seeds.is_empty());
        resolve_cover_at_with_request(
            &state,
            &fixture.path,
            &authority,
            2,
            101,
            || true,
            || {
                (
                    None,
                    Some(PresentationMetadata {
                        verified_identity: Some(verified_javdb("newitem", &authority.code)),
                        source: Some("JavDB".to_owned()),
                        provider_id: Some("newitem".to_owned()),
                        display_code: Some(authority.code.clone()),
                        title: Some("New metadata".to_owned()),
                        ..PresentationMetadata::default()
                    }),
                    true,
                )
            },
        )
        .expect("replacement request must retain its own seed");

        cancel_cover_request(&state, authority.category, &authority.code, 1)
            .expect("obsolete cancellation must be harmless");
        assert_eq!(state.0.lock().unwrap().metadata_seeds.len(), 1);
        assert_eq!(state.0.lock().unwrap().verified_identity_seeds.len(), 1);
        let metadata = resolve_metadata(&state, &fixture.path, &authority, 2, || true)
            .expect("the current request must consume only its own seed");
        assert_eq!(metadata[5], "newitem");
        assert_eq!(metadata[10], "New metadata");
        assert!(state.0.lock().unwrap().metadata_seeds.is_empty());
        assert!(state.0.lock().unwrap().verified_identity_seeds.is_empty());
    }

    #[test]
    fn repeated_scan_replacements_do_not_accumulate_metadata_seeds() {
        let fixture = CacheFixture::new("metadata-seed-scans");
        let state = LibraryPresentationState::default();
        for generation in 1..=32 {
            let authority = LibraryItemAuthority {
                category: LibraryPresentationCategory::Vr,
                identity: format!("{generation:040x}"),
                code: "3DSVR-01871".to_owned(),
                product_identity: "3DSVR-1871".to_owned(),
            };
            begin_cover_request(&state, authority.category, &authority.code, generation)
                .expect("replacement scan request must begin");
            resolve_cover_at_with_request(
                &state,
                &fixture.path,
                &authority,
                generation,
                100 + generation,
                || true,
                || {
                    (
                        None,
                        Some(PresentationMetadata {
                            verified_identity: Some(verified_javdb(
                                &format!("item{generation}"),
                                &authority.code,
                            )),
                            source: Some("JavDB".to_owned()),
                            provider_id: Some(format!("item{generation}")),
                            display_code: Some(authority.code.clone()),
                            title: Some(format!("Metadata {generation}")),
                            ..PresentationMetadata::default()
                        }),
                        true,
                    )
                },
            )
            .expect("current scan may retain one seed");
            let context = state.0.lock().unwrap();
            assert_eq!(context.metadata_seeds.len(), 1);
            assert_eq!(context.verified_identity_seeds.len(), 1);
            assert!(context.metadata_seeds.keys().all(|seed| {
                seed.item_identity == authority.identity && seed.request_generation == generation
            }));
            assert!(context.verified_identity_seeds.keys().all(|seed| {
                seed.item_identity == authority.identity && seed.request_generation == generation
            }));
        }
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
                product_identity: "3DSVR-1871".to_owned(),
            };
            let response = cover_response(
                &state,
                &authority,
                0,
                "ready",
                Some(CoverSource {
                    provider: "JavDB",
                    provider_id: format!("item{index}"),
                    display_code: authority.code.clone(),
                    url: format!("https://tp.cmastd.com/item-{index}.jpg"),
                    aspect_ratio: 0.72,
                }),
                Some(jpeg(600, 800)),
                Some(verified_javdb(&format!("item{index}"), &authority.code)),
            )
            .expect("replacement authority must be retained");
            if !previous_id.is_empty() {
                assert!(!state.0.lock().unwrap().covers.contains_key(&previous_id));
            }
            previous_id = response[6].clone();
            assert_eq!(state.0.lock().unwrap().covers.len(), 1);
        }

        for index in 0..MAX_RETAINED_COVER_AUTHORITIES + 3 {
            let authority = LibraryItemAuthority {
                category: LibraryPresentationCategory::Adult,
                identity: format!("f{index:039x}"),
                code: format!("ADLT-{}", index + 1),
                product_identity: format!("ADLT-{}", index + 1),
            };
            let response = cover_response(
                &state,
                &authority,
                0,
                "ready",
                Some(CoverSource {
                    provider: "JavDB",
                    provider_id: format!("adult{index}"),
                    display_code: authority.code.clone(),
                    url: format!("https://tp.cmastd.com/adult-{index}.jpg"),
                    aspect_ratio: 0.72,
                }),
                Some(jpeg(600, 800)),
                Some(verified_javdb(&format!("adult{index}"), &authority.code)),
            );
            if index + 1 < MAX_RETAINED_COVER_AUTHORITIES {
                response.expect("capacity must retain current authorities");
            } else {
                assert_eq!(response, Err(LIBRARY_PRESENTATION_FAILED));
            }
        }
        let context = state.0.lock().unwrap();
        assert_eq!(context.covers.len(), MAX_RETAINED_COVER_AUTHORITIES);
        assert_eq!(
            context
                .covers
                .values()
                .filter(|retained| retained.bytes.is_some())
                .count(),
            MAX_RETAINED_COVER_BYTES
        );
    }

    #[test]
    fn fourteen_current_cover_authorities_remain_fetchable_until_consumed() {
        let state = LibraryPresentationState::default();
        for first in (1..=14).step_by(MAX_RETAINED_COVER_BYTES) {
            let mut current = Vec::new();
            for index in first..=(first + MAX_RETAINED_COVER_BYTES - 1).min(14) {
                let code = format!("ADLT-{index}");
                let authority = LibraryItemAuthority {
                    category: LibraryPresentationCategory::Adult,
                    identity: format!("{index:040x}"),
                    code: code.clone(),
                    product_identity: code.clone(),
                };
                let source = CoverSource {
                    provider: "JavDB",
                    provider_id: format!("item{index}"),
                    display_code: code,
                    url: format!("https://tp.cmastd.com/item-{index}.jpg"),
                    aspect_ratio: 0.72,
                };
                let response = cover_response(
                    &state,
                    &authority,
                    0,
                    "ready",
                    Some(source),
                    Some(jpeg(600, 800)),
                    Some(verified_javdb(&format!("item{index}"), &authority.code)),
                )
                .expect("each current card must retain its cover authority");
                current.push((authority, response[6].clone()));
            }
            assert!(state.0.lock().unwrap().covers.len() <= MAX_RETAINED_COVER_BYTES);
            for (authority, cover_authority_id) in current {
                let fetched = fetch_cover_with(&state, &authority, &cover_authority_id, |_| {
                    panic!("current cover bytes must be consumed before another batch")
                })
                .expect("a current cover authority must remain fetchable until consumed");
                assert!(!fetched.is_empty());
            }
        }
        assert!(state.0.lock().unwrap().covers.is_empty());
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
                Some(verified_javdb("obsolete", &authority.code)),
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
            Some(verified_javdb("current", &authority.code)),
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
        assert!(state.0.lock().unwrap().covers.contains_key(&current[6]));
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
    fn current_cover_requests_remain_bounded_without_evicting_live_authority() {
        let state = LibraryPresentationState::default();
        for generation in 1..=MAX_RETAINED_COVER_REQUESTS as u64 + 3 {
            let code = format!("ADLT-{generation}");
            let result = begin_cover_request(
                &state,
                LibraryPresentationCategory::Adult,
                &code,
                generation,
            );
            if generation <= MAX_RETAINED_COVER_REQUESTS as u64 {
                result.expect("capacity must retain current requests");
            } else {
                assert_eq!(result, Err(LIBRARY_PRESENTATION_FAILED));
            }
        }
        assert_eq!(
            state.0.lock().unwrap().cover_requests.len(),
            MAX_RETAINED_COVER_REQUESTS
        );
        assert!(cover_request_is_current(
            &state,
            LibraryPresentationCategory::Adult,
            "ADLT-1",
            1
        ));
        assert!(!cover_request_is_current(
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
        fetch_cover_with(&state, &authority, &first[6], |_| {
            panic!("fresh bytes must be retained")
        })
        .expect("browser decode begins after exact bytes are returned");
        invalidate_cover(&state, &fixture.path, &authority, 0, &first[6])
            .expect("browser decode failure must invalidate the exact cached source");
        assert!(state
            .0
            .lock()
            .unwrap()
            .failed_cover_sources
            .get(&authority.identity)
            .is_some_and(|failed| failed.urls.contains("https://tp.cmastd.com/unusable.jpg")));

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
                            provider: "JavDB",
                            provider_id: "replacement".to_owned(),
                            display_code: "MDVR-419".to_owned(),
                            url: "https://tp.cmastd.com/replacement.jpg".to_owned(),
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
        assert_eq!(replacement[3], "JavDB");
        assert_ne!(replacement[6], first[6]);
        assert_eq!(
            fetch_cover_with(&state, &authority, &first[6], |_| {
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
            verified_identity: Some(verified_javdb("item", "ADLT-123")),
            identity_conflict: false,
            source: Some("JavDB".to_owned()),
            provider_id: Some("item".to_owned()),
            display_code: Some("ADLT-123".to_owned()),
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
            product_identity: "ADLT-124".to_owned(),
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
    fn metadata_without_a_cover_establishes_exact_identity_and_survives_restart() {
        for (index, authority, display_code) in [
            (
                0,
                LibraryItemAuthority {
                    category: LibraryPresentationCategory::Adult,
                    identity: "5".repeat(40),
                    code: "CAWB-1".to_owned(),
                    product_identity: "CAWB-1".to_owned(),
                },
                "CAWB-001",
            ),
            (
                1,
                LibraryItemAuthority {
                    category: LibraryPresentationCategory::Vr,
                    identity: "6".repeat(40),
                    code: "3DSVR-01871".to_owned(),
                    product_identity: "3DSVR-1871".to_owned(),
                },
                "3DSVR-01871",
            ),
        ] {
            let fixture = CacheFixture::new(&format!("metadata-identity-{index}"));
            let metadata = PresentationMetadata {
                verified_identity: Some(verified_javdb("metadataitem", display_code)),
                source: Some("JavDB".to_owned()),
                provider_id: Some("metadataitem".to_owned()),
                display_code: Some(display_code.to_owned()),
                ..PresentationMetadata::default()
            };
            let response = resolve_metadata_at_with(
                &LibraryPresentationState::default(),
                &fixture.path,
                &authority,
                100,
                || true,
                || Ok(Some(metadata)),
            )
            .expect("metadata identity must resolve without a cover");
            assert_eq!(response[2], "automatic");
            assert_eq!(
                &response[3..7],
                ["current", "JavDB", "metadataitem", display_code]
            );
            assert_eq!(response[10], "");

            let restarted = resolve_metadata_at_with(
                &LibraryPresentationState::default(),
                &fixture.path,
                &authority,
                101,
                || true,
                || panic!("restart must use the durable metadata identity"),
            )
            .expect("metadata identity must survive restart");
            assert_eq!(restarted, response);
            assert_eq!(
                read_cache(&fixture.path).expect("metadata cache must remain valid")[0]
                    .verified_identity,
                Some(verified_javdb("metadataitem", display_code))
            );
        }
    }

    #[test]
    fn cover_and_metadata_identity_conflicts_clear_durable_presentation() {
        for (index, current_identity, metadata_identity) in [
            (
                0,
                verified_javdb("coveritem", "CAWB-001"),
                verified_javdb("metadataitem", "CAWB-001"),
            ),
            (
                1,
                verified_fanza("cawb00001", "CAWB-001"),
                verified_javdb("metadataitem", "CAWB-1"),
            ),
        ] {
            let fixture = CacheFixture::new(&format!("metadata-conflict-{index}"));
            let authority = LibraryItemAuthority {
                category: LibraryPresentationCategory::Adult,
                identity: format!("7{index:039x}"),
                code: "CAWB-1".to_owned(),
                product_identity: "CAWB-1".to_owned(),
            };
            write_cache(
                &fixture.path,
                &[CacheEntry {
                    identity: authority.identity.clone(),
                    category: authority.category,
                    code: authority.code.clone(),
                    identity_saved_at: 90,
                    verified_identity: Some(current_identity),
                    cover_saved_at: 90,
                    cover_state: "missing",
                    cover: None,
                    metadata_saved_at: 0,
                    metadata_state: "missing",
                    metadata: PresentationMetadata::default(),
                }],
            )
            .expect("the cover-phase identity must be durable");
            let metadata = PresentationMetadata {
                verified_identity: Some(metadata_identity.clone()),
                source: Some("JavDB".to_owned()),
                provider_id: Some(metadata_identity.provider_id),
                display_code: Some(metadata_identity.display_code),
                title: Some("Conflicting metadata".to_owned()),
                ..PresentationMetadata::default()
            };
            let response = resolve_metadata_at_with(
                &LibraryPresentationState::default(),
                &fixture.path,
                &authority,
                100,
                || true,
                || Ok(Some(metadata)),
            )
            .expect("the conflict must be returned as local unavailable state");
            assert_eq!(&response[2..7], ["unavailable", "conflict", "", "", ""]);
            assert!(read_cache(&fixture.path)
                .expect("conflicting presentation must not survive restart")
                .is_empty());
        }
    }

    #[test]
    fn exact_javdb_metadata_can_replace_agreeing_fanza_identity_without_losing_cover() {
        let fixture = CacheFixture::new("metadata-exact-agreement");
        let authority = LibraryItemAuthority {
            category: LibraryPresentationCategory::Adult,
            identity: "9".repeat(40),
            code: "CAWB-1".to_owned(),
            product_identity: "CAWB-1".to_owned(),
        };
        write_cache(
            &fixture.path,
            &[CacheEntry {
                identity: authority.identity.clone(),
                category: authority.category,
                code: authority.code.clone(),
                identity_saved_at: 90,
                verified_identity: Some(verified_fanza("cawb00001", "CAWB-001")),
                cover_saved_at: 90,
                cover_state: "ready",
                cover: Some(CoverSource {
                    provider: "FANZA",
                    provider_id: "cawb00001".to_owned(),
                    display_code: "CAWB-001".to_owned(),
                    url: "https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/cawb00001/cawb00001ps.jpg".to_owned(),
                    aspect_ratio: 0.75,
                }),
                metadata_saved_at: 0,
                metadata_state: "missing",
                metadata: PresentationMetadata::default(),
            }],
        )
        .expect("the exact FANZA cover identity must be durable");
        let response = resolve_metadata_at_with(
            &LibraryPresentationState::default(),
            &fixture.path,
            &authority,
            100,
            || true,
            || {
                Ok(Some(PresentationMetadata {
                    verified_identity: Some(verified_javdb("javdbitem", "CAWB-001")),
                    source: Some("JavDB".to_owned()),
                    provider_id: Some("javdbitem".to_owned()),
                    display_code: Some("CAWB-001".to_owned()),
                    title: Some("Exact metadata".to_owned()),
                    ..PresentationMetadata::default()
                }))
            },
        )
        .expect("exact cross-provider agreement must remain usable");
        assert_eq!(
            &response[3..7],
            ["current", "JavDB", "javdbitem", "CAWB-001"]
        );
        let entry = &read_cache(&fixture.path).expect("the reconciled cache must remain valid")[0];
        assert_eq!(
            entry.verified_identity,
            Some(verified_javdb("javdbitem", "CAWB-001"))
        );
        assert_eq!(
            entry.cover.as_ref().map(|source| source.provider),
            Some("FANZA")
        );
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
            identity_saved_at: 0,
            verified_identity: None,
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
    fn cached_fanza_cover_requires_the_exact_transport_display_and_image_association() {
        for (index, authority, persisted_display, persisted_url, expected_display) in [
            (
                0,
                LibraryItemAuthority {
                    category: LibraryPresentationCategory::Adult,
                    identity: "d".repeat(40),
                    code: "CAWB-1".to_owned(),
                    product_identity: "CAWB-1".to_owned(),
                },
                "CAWB-1",
                "https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/cawb00001/cawb00001ps.jpg",
                "CAWB-001",
            ),
            (
                1,
                LibraryItemAuthority {
                    category: LibraryPresentationCategory::Adult,
                    identity: "e".repeat(40),
                    code: "CAWB-1".to_owned(),
                    product_identity: "CAWB-1".to_owned(),
                },
                "CAWB-00001",
                "https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/cawb00001/cawb00001ps.jpg",
                "CAWB-001",
            ),
            (
                2,
                LibraryItemAuthority {
                    category: LibraryPresentationCategory::Vr,
                    identity: "f".repeat(40),
                    code: "3DSVR-01871".to_owned(),
                    product_identity: "3DSVR-1871".to_owned(),
                },
                "3DSVR-1871",
                "https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/13dsvr01871/13dsvr01871ps.jpg",
                "3DSVR-01871",
            ),
            (
                3,
                LibraryItemAuthority {
                    category: LibraryPresentationCategory::Adult,
                    identity: "9".repeat(40),
                    code: "CAWB-1".to_owned(),
                    product_identity: "CAWB-1".to_owned(),
                },
                "CAWB-001",
                "https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/cawb00002/cawb00002ps.jpg",
                "CAWB-001",
            ),
        ] {
            let fixture = CacheFixture::new(&format!("fanza-image-identity-{index}"));
            let provider_id = if authority.category == LibraryPresentationCategory::Adult {
                "cawb00001"
            } else {
                "13dsvr01871"
            };
            let exact_url = if authority.category == LibraryPresentationCategory::Adult {
                "https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/cawb00001/cawb00001ps.jpg"
            } else {
                "https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/13dsvr01871/13dsvr01871ps.jpg"
            };
            write_cache(
                &fixture.path,
                &[CacheEntry {
                    identity: authority.identity.clone(),
                    category: authority.category,
                    code: authority.code.clone(),
                    identity_saved_at: 0,
                    verified_identity: None,
                    cover_saved_at: 100,
                    cover_state: "ready",
                    cover: Some(CoverSource {
                        provider: "FANZA",
                        provider_id: provider_id.to_owned(),
                        display_code: persisted_display.to_owned(),
                        url: persisted_url.to_owned(),
                        aspect_ratio: 0.72,
                    }),
                    metadata_saved_at: 0,
                    metadata_state: "missing",
                    metadata: PresentationMetadata::default(),
                }],
            )
            .expect("the mismatched FANZA cache fixture must be written");

            let provider_calls = Cell::new(0);
            let response = resolve_cover_at_with(
                &LibraryPresentationState::default(),
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
                                provider_id: provider_id.to_owned(),
                                display_code: expected_display.to_owned(),
                                url: exact_url.to_owned(),
                                aspect_ratio: 0.75,
                            },
                            jpeg(600, 800),
                        )),
                        None,
                        false,
                    )
                },
            )
            .expect("a mismatched FANZA cache must permit fresh provider resolution");
            assert_eq!(provider_calls.get(), 1);
            assert_eq!(
                &response[2..6],
                ["ready", "FANZA", provider_id, expected_display]
            );
            let cache = read_cache(&fixture.path).expect("the replacement cache must be valid");
            let source = cache[0]
                .cover
                .as_ref()
                .expect("the fresh exact FANZA source must be persisted");
            assert_eq!(source.display_code, expected_display);
            assert_eq!(source.url, exact_url);
        }
    }

    #[test]
    fn exact_fanza_cache_is_reused_without_provider_rediscovery() {
        let fixture = CacheFixture::new("valid-fanza-cache");
        let authority = LibraryItemAuthority {
            category: LibraryPresentationCategory::Adult,
            identity: "8".repeat(40),
            code: "CAWB-1".to_owned(),
            product_identity: "CAWB-1".to_owned(),
        };
        write_cache(
            &fixture.path,
            &[CacheEntry {
                identity: authority.identity.clone(),
                category: authority.category,
                code: authority.code.clone(),
                identity_saved_at: 100,
                verified_identity: Some(VerifiedDisplayIdentity {
                    provider: "FANZA",
                    provider_id: "cawb00001".to_owned(),
                    display_code: "CAWB-001".to_owned(),
                }),
                cover_saved_at: 100,
                cover_state: "ready",
                cover: Some(CoverSource {
                    provider: "FANZA",
                    provider_id: "cawb00001".to_owned(),
                    display_code: "CAWB-001".to_owned(),
                    url: "https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/cawb00001/cawb00001ps.jpg".to_owned(),
                    aspect_ratio: 0.75,
                }),
                metadata_saved_at: 0,
                metadata_state: "missing",
                metadata: PresentationMetadata::default(),
            }],
        )
        .expect("the exact FANZA cache fixture must be written");
        let response = resolve_cover_at_with(
            &LibraryPresentationState::default(),
            &fixture.path,
            &authority,
            101,
            || true,
            || panic!("a valid exact FANZA cache must skip provider rediscovery"),
        )
        .expect("the exact FANZA cache must remain reusable");
        assert_eq!(&response[2..6], ["ready", "FANZA", "cawb00001", "CAWB-001"]);
    }

    #[test]
    fn ready_cover_cache_requires_a_complete_consistent_verified_identity() {
        let authority = LibraryItemAuthority {
            category: LibraryPresentationCategory::Adult,
            identity: "8".repeat(40),
            code: "CAWB-1".to_owned(),
            product_identity: "CAWB-1".to_owned(),
        };
        let javdb_cover = CoverSource {
            provider: "JavDB",
            provider_id: "coveritem".to_owned(),
            display_code: "CAWB-001".to_owned(),
            url: "https://tp.cmastd.com/cover.jpg".to_owned(),
            aspect_ratio: 0.75,
        };
        let fanza_cover = CoverSource {
            provider: "FANZA",
            provider_id: "cawb00001".to_owned(),
            display_code: "CAWB-001".to_owned(),
            url: "https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/cawb00001/cawb00001ps.jpg"
                .to_owned(),
            aspect_ratio: 0.75,
        };
        for (index, source, identity) in [
            (0, javdb_cover.clone(), None),
            (1, fanza_cover.clone(), None),
            (
                2,
                javdb_cover.clone(),
                Some(verified_javdb("anotheritem", "CAWB-001")),
            ),
            (
                3,
                javdb_cover.clone(),
                Some(verified_javdb("coveritem", "CAWB-1")),
            ),
            (
                4,
                fanza_cover.clone(),
                Some(verified_fanza("cawb00001", "CAWB-1")),
            ),
        ] {
            let fixture = CacheFixture::new(&format!("invalid-cover-identity-{index}"));
            write_cache(
                &fixture.path,
                &[CacheEntry {
                    identity: authority.identity.clone(),
                    category: authority.category,
                    code: authority.code.clone(),
                    identity_saved_at: identity.as_ref().map_or(0, |_| 100),
                    verified_identity: identity,
                    cover_saved_at: 100,
                    cover_state: "ready",
                    cover: Some(source),
                    metadata_saved_at: 0,
                    metadata_state: "missing",
                    metadata: PresentationMetadata::default(),
                }],
            )
            .expect("the invalid combination fixture must be written");
            assert_invalid_cache_refreshes(&fixture.path, &authority);
        }

        for (index, source, identity) in [
            (0, javdb_cover, verified_javdb("coveritem", "CAWB-001")),
            (
                1,
                fanza_cover.clone(),
                verified_fanza("cawb00001", "CAWB-001"),
            ),
            (2, fanza_cover, verified_javdb("metadataitem", "CAWB-001")),
            (
                3,
                CoverSource {
                    provider: "r18.dev",
                    provider_id: "CAWB-1".to_owned(),
                    display_code: "CAWB-001".to_owned(),
                    url: "https://pics.dmm.co.jp/digital/video/runtime/runtimepl.jpg".to_owned(),
                    aspect_ratio: 0.75,
                },
                verified_javdb("metadataitem", "CAWB-001"),
            ),
        ] {
            let fixture = CacheFixture::new(&format!("valid-cover-identity-{index}"));
            write_cache(
                &fixture.path,
                &[CacheEntry {
                    identity: authority.identity.clone(),
                    category: authority.category,
                    code: authority.code.clone(),
                    identity_saved_at: 100,
                    verified_identity: Some(identity),
                    cover_saved_at: 100,
                    cover_state: "ready",
                    cover: Some(source),
                    metadata_saved_at: 0,
                    metadata_state: "missing",
                    metadata: PresentationMetadata::default(),
                }],
            )
            .expect("the valid combination fixture must be written");
            let response = resolve_cover_at_with(
                &LibraryPresentationState::default(),
                &fixture.path,
                &authority,
                101,
                || true,
                || panic!("a valid cover and identity cache must be reused"),
            )
            .expect("the valid cover and identity cache must resolve");
            assert_eq!(response[2], "ready");
        }

        let fixture = CacheFixture::new("valid-local-legacy-cover");
        write_cache(
            &fixture.path,
            &[CacheEntry {
                identity: authority.identity.clone(),
                category: authority.category,
                code: authority.code.clone(),
                identity_saved_at: 0,
                verified_identity: None,
                cover_saved_at: 100,
                cover_state: "ready",
                cover: Some(CoverSource {
                    provider: "r18.dev",
                    provider_id: "CAWB-1".to_owned(),
                    display_code: "CAWB-1".to_owned(),
                    url: "https://pics.dmm.co.jp/digital/video/runtime/runtimepl.jpg".to_owned(),
                    aspect_ratio: 0.75,
                }),
                metadata_saved_at: 0,
                metadata_state: "missing",
                metadata: PresentationMetadata::default(),
            }],
        )
        .expect("the local-only legacy fixture must be written");
        resolve_cover_at_with(
            &LibraryPresentationState::default(),
            &fixture.path,
            &authority,
            101,
            || true,
            || panic!("a valid local-only legacy cover must be reused"),
        )
        .expect("the local-only legacy cover must resolve");
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
                identity_saved_at: 0,
                verified_identity: None,
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
            product_identity: "ADLT-124".to_owned(),
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
