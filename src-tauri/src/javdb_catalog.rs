use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::process::Command;
#[cfg(target_os = "macos")]
use std::process::Stdio;

use crate::{
    vr_torrent::{JsonParser, JsonValue},
    ProviderRequestError, ADULT_NETWORK_ERROR, ADULT_PROVIDER_ERROR, ADULT_SOURCE_UNAVAILABLE,
    VR_NETWORK_ERROR, VR_PROVIDER_ERROR, VR_SOURCE_UNAVAILABLE,
};

const JAVDB_API_URL: &str = "https://apidd.spthgb.com";
const JAVDB_API_USER_AGENT: &str = "Dart/3.5 (dart:io)";
const JAVDB_SIGNATURE_MIDDLE: &str = "lpw6vgqzsp";
const JAVDB_SIGNATURE_SECRET: &str = "71cf27bb3c0bcdf207b64abecddc970098c7421ee7203b9cdae54478478a199e7d5a6e1a57691123c1a931c057842fb73ba3b3c83bcd69c17ccf174081e3d8aa";
const JAVDB_RESPONSE_MAX_BYTES: usize = 4 * 1024 * 1024;
const JAVDB_COVER_MAX_BYTES: usize = 16 * 1024 * 1024;
const JAVDB_HTTP_STATUS_MARKER: &str = "\nAUTO_VIDEO_HTTP_STATUS:";
#[cfg(target_os = "macos")]
const JAVDB_HTTP_STATUS_WRITE_OUT: &str = "\nAUTO_VIDEO_HTTP_STATUS:%{http_code}";

pub(crate) const ADULT_JAVDB_MALFORMED: &str = "adult_javdb_malformed_provider";
pub(crate) const ADULT_JAVDB_CONFLICTING: &str = "adult_javdb_conflicting_provider";
pub(crate) const ADULT_JAVDB_STALE: &str = "adult_javdb_stale";
pub(crate) const VR_JAVDB_MALFORMED: &str = "vr_javdb_malformed_provider";
pub(crate) const VR_JAVDB_CONFLICTING: &str = "vr_javdb_conflicting_provider";
pub(crate) const VR_JAVDB_STALE: &str = "vr_javdb_stale";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CatalogCategory {
    Adult,
    Vr,
}

impl CatalogCategory {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "adult" => Some(Self::Adult),
            "vr" => Some(Self::Vr),
            _ => None,
        }
    }

    fn value(self) -> &'static str {
        match self {
            Self::Adult => "adult",
            Self::Vr => "vr",
        }
    }

    fn provider_error(self, error: ProviderRequestError) -> &'static str {
        match (self, error) {
            (Self::Adult, ProviderRequestError::SourceUnavailable) => ADULT_SOURCE_UNAVAILABLE,
            (Self::Adult, ProviderRequestError::Network) => ADULT_NETWORK_ERROR,
            (Self::Adult, ProviderRequestError::Provider) => ADULT_PROVIDER_ERROR,
            (Self::Vr, ProviderRequestError::SourceUnavailable) => VR_SOURCE_UNAVAILABLE,
            (Self::Vr, ProviderRequestError::Network) => VR_NETWORK_ERROR,
            (Self::Vr, ProviderRequestError::Provider) => VR_PROVIDER_ERROR,
        }
    }

    fn malformed_error(self) -> &'static str {
        match self {
            Self::Adult => ADULT_JAVDB_MALFORMED,
            Self::Vr => VR_JAVDB_MALFORMED,
        }
    }

    fn conflicting_error(self) -> &'static str {
        match self {
            Self::Adult => ADULT_JAVDB_CONFLICTING,
            Self::Vr => VR_JAVDB_CONFLICTING,
        }
    }

    fn stale_error(self) -> &'static str {
        match self {
            Self::Adult => ADULT_JAVDB_STALE,
            Self::Vr => VR_JAVDB_STALE,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JavdbCatalogRequest {
    pub category: String,
    pub mode: String,
    pub period: String,
    pub year: Option<String>,
    pub month: Option<u8>,
    pub sort: String,
    pub count: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CatalogItem {
    provider_item_id: String,
    code: String,
    title: Option<String>,
    release_date: Option<String>,
    cover_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AuthorizedCatalogItem {
    provider_item_id: String,
    code: String,
    title: Option<String>,
    release_date: Option<String>,
    cover_authority_id: Option<String>,
    cover_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CatalogAuthority {
    generation: u64,
    items: Vec<AuthorizedCatalogItem>,
}

#[derive(Default)]
struct CatalogContext {
    generation: u64,
    adult: Option<CatalogAuthority>,
    vr: Option<CatalogAuthority>,
}

#[derive(Clone, Default)]
pub(crate) struct JavdbCatalogState(Arc<Mutex<CatalogContext>>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CatalogDocumentError {
    Malformed,
    Provider,
    Conflicting,
}

#[derive(Debug, PartialEq, Eq)]
struct ParsedListing {
    items: Vec<CatalogItem>,
    provider_empty: bool,
}

fn valid_provider_item_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|character| character.is_ascii_alphanumeric())
}

fn canonical_product_code(value: &str) -> Option<String> {
    let value = value.trim();
    let prefix_end = value
        .bytes()
        .position(|character| !character.is_ascii_alphabetic())?;
    let prefix = &value[..prefix_end];
    if !(2..=16).contains(&prefix.len()) {
        return None;
    }
    let number = value[prefix_end..].trim_start_matches([' ', '_', '-']);
    if number.is_empty()
        || number.len() > 10
        || !number.bytes().all(|character| character.is_ascii_digit())
    {
        return None;
    }
    let number = number.parse::<u64>().ok()?;
    (number > 0).then(|| format!("{}-{number}", prefix.to_ascii_uppercase()))
}

fn optional_text(object: &BTreeMap<String, JsonValue>, key: &str) -> Option<String> {
    match object.get(key) {
        Some(JsonValue::String(value)) => {
            let value = value.trim();
            (!value.is_empty()).then(|| value.to_owned())
        }
        _ => None,
    }
}

fn valid_cover_url(value: &str) -> bool {
    if value.bytes().any(|character| {
        character.is_ascii_control() || character.is_ascii_whitespace() || character == b'\\'
    }) {
        return false;
    }
    let Some(remainder) = value.strip_prefix("https://") else {
        return false;
    };
    let Some((authority, path)) = remainder.split_once('/') else {
        return false;
    };
    if authority.contains(['@', ':']) || path.is_empty() {
        return false;
    }
    let labels: Vec<&str> = authority.split('.').collect();
    if labels.len() != 3 || labels[0] != "tp" || labels[2] != "com" {
        return false;
    }
    let label = labels[1];
    !label.is_empty()
        && label.len() <= 63
        && label
            .bytes()
            .all(|character| character.is_ascii_alphanumeric() || character == b'-')
        && label
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && label
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
}

fn cover_url(movie: &BTreeMap<String, JsonValue>) -> Option<String> {
    ["cover_url", "thumb_url"].into_iter().find_map(|key| {
        let JsonValue::String(value) = movie.get(key)? else {
            return None;
        };
        valid_cover_url(value).then(|| value.clone())
    })
}

fn parse_listing(document: &str) -> Result<ParsedListing, CatalogDocumentError> {
    let JsonValue::Object(envelope) = JsonParser::new(document)
        .parse()
        .ok_or(CatalogDocumentError::Malformed)?
    else {
        return Err(CatalogDocumentError::Malformed);
    };
    let Some(JsonValue::Number(success)) = envelope.get("success") else {
        return Err(CatalogDocumentError::Malformed);
    };
    if success != "1" {
        return Err(CatalogDocumentError::Provider);
    }
    let Some(JsonValue::Object(data)) = envelope.get("data") else {
        return Err(CatalogDocumentError::Malformed);
    };
    let Some(JsonValue::Array(movies)) = data.get("movies") else {
        return Err(CatalogDocumentError::Malformed);
    };

    let mut items = Vec::new();
    let mut accepted_codes = HashMap::<String, String>::new();
    for movie in movies {
        let JsonValue::Object(movie) = movie else {
            continue;
        };
        let Some(JsonValue::String(provider_item_id)) = movie.get("id") else {
            continue;
        };
        let Some(JsonValue::String(number)) = movie.get("number") else {
            continue;
        };
        if !valid_provider_item_id(provider_item_id) {
            continue;
        }
        let Some(code) = canonical_product_code(number) else {
            continue;
        };
        if let Some(previous_code) = accepted_codes.get(provider_item_id) {
            if previous_code != &code {
                return Err(CatalogDocumentError::Conflicting);
            }
            continue;
        }
        accepted_codes.insert(provider_item_id.clone(), code.clone());
        items.push(CatalogItem {
            provider_item_id: provider_item_id.clone(),
            code,
            title: optional_text(movie, "title").or_else(|| optional_text(movie, "origin_title")),
            release_date: optional_text(movie, "release_date"),
            cover_url: cover_url(movie),
        });
    }
    Ok(ParsedListing {
        items,
        provider_empty: movies.is_empty(),
    })
}

fn adult_item_category(document: &str, provider_item_id: &str) -> Option<CatalogCategory> {
    let JsonValue::Object(envelope) = JsonParser::new(document).parse()? else {
        return None;
    };
    if envelope.get("success") != Some(&JsonValue::Number("1".to_owned())) {
        return None;
    }
    let JsonValue::Object(data) = envelope.get("data")? else {
        return None;
    };
    let JsonValue::Object(movie) = data.get("movie")? else {
        return None;
    };
    if movie.get("id") != Some(&JsonValue::String(provider_item_id.to_owned())) {
        return None;
    }
    let JsonValue::Array(tags) = movie.get("tags")? else {
        return None;
    };
    let mut is_vr = false;
    for tag in tags {
        let JsonValue::Object(tag) = tag else {
            return None;
        };
        let JsonValue::String(tag_id) = tag.get("id")? else {
            return None;
        };
        is_vr |= tag_id == "212";
    }
    Some(if is_vr {
        CatalogCategory::Vr
    } else {
        CatalogCategory::Adult
    })
}

fn sort_parameters(value: &str) -> Option<(&'static str, &'static str)> {
    match value {
        "newest" => Some(("release", "desc")),
        "oldest" => Some(("release", "asc")),
        "recently-updated" => Some(("update", "desc")),
        "top-rated" => Some(("score", "desc")),
        "most-viewed" => Some(("hit", "desc")),
        "most-wanted" => Some(("want_watch_count", "desc")),
        "most-watched" => Some(("watched_count", "desc")),
        _ => None,
    }
}

fn current_calendar_year() -> Option<u16> {
    let unix_days = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs() / 86_400;
    let days = i64::try_from(unix_days).ok()?.checked_add(719_468)?;
    let era = days / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    u16::try_from(year).ok()
}

fn validated_request(request: &JavdbCatalogRequest) -> Option<CatalogCategory> {
    let category = CatalogCategory::parse(&request.category)?;
    if !matches!(request.count, 10 | 25 | 50 | 100)
        || !matches!(request.period.as_str(), "daily" | "weekly" | "monthly")
        || request.year.as_deref().is_some_and(|year| {
            year.len() != 4
                || !year.bytes().all(|byte| byte.is_ascii_digit())
                || year
                    .parse::<u16>()
                    .ok()
                    .zip(current_calendar_year())
                    .is_none_or(|(year, current)| !(2001..=current).contains(&year))
        })
        || request
            .month
            .is_some_and(|month| !(1..=12).contains(&month))
        || sort_parameters(&request.sort).is_none()
    {
        return None;
    }
    match (category, request.mode.as_str()) {
        (CatalogCategory::Adult, "ranking")
            if request.year.is_none() && request.month.is_none() && request.sort == "newest" => {}
        (CatalogCategory::Adult | CatalogCategory::Vr, "category") => {}
        _ => return None,
    }
    Some(category)
}

fn listing_urls(request: &JavdbCatalogRequest, category: CatalogCategory) -> Vec<String> {
    if request.mode == "ranking" {
        return vec![format!(
            "{JAVDB_API_URL}/api/v1/rankings?type=0&period={}",
            request.period
        )];
    }
    let (sort_by, order_by) = sort_parameters(&request.sort).expect("validated sort");
    let genre = if category == CatalogCategory::Vr {
        "212"
    } else {
        ""
    };
    let year = request.year.as_deref().unwrap_or("");
    let month = request
        .month
        .map(|value| value.to_string())
        .unwrap_or_default();
    let limit = request.count.min(50);
    let pages = if request.count == 100 { 2 } else { 1 };
    (1..=pages)
        .map(|page| {
            format!(
                "{JAVDB_API_URL}/api/v1/movies/tags?filter_by=0%3At%3Am%3A{genre}%3A{year}%3A%3A{month}&filter_by_tags=&sort_by={sort_by}&order_by={order_by}&page={page}&limit={limit}"
            )
        })
        .collect()
}

fn begin_request(
    state: &JavdbCatalogState,
    category: CatalogCategory,
) -> Result<u64, &'static str> {
    let mut context = state
        .0
        .lock()
        .map_err(|_| category.provider_error(ProviderRequestError::Provider))?;
    context.generation = context
        .generation
        .checked_add(1)
        .ok_or_else(|| category.provider_error(ProviderRequestError::Provider))?;
    let authority = CatalogAuthority {
        generation: context.generation,
        items: Vec::new(),
    };
    match category {
        CatalogCategory::Adult => context.adult = Some(authority),
        CatalogCategory::Vr => context.vr = Some(authority),
    }
    Ok(context.generation)
}

fn authority(context: &CatalogContext, category: CatalogCategory) -> Option<&CatalogAuthority> {
    match category {
        CatalogCategory::Adult => context.adult.as_ref(),
        CatalogCategory::Vr => context.vr.as_ref(),
    }
}

fn authority_mut(
    context: &mut CatalogContext,
    category: CatalogCategory,
) -> Option<&mut CatalogAuthority> {
    match category {
        CatalogCategory::Adult => context.adult.as_mut(),
        CatalogCategory::Vr => context.vr.as_mut(),
    }
}

fn encode_catalog(
    category: CatalogCategory,
    generation: u64,
    items: &[AuthorizedCatalogItem],
) -> Vec<String> {
    let mut response = Vec::with_capacity(2 + items.len() * 7);
    response.push(generation.to_string());
    response.push(items.len().to_string());
    for item in items {
        response.extend([
            category.value().to_owned(),
            item.provider_item_id.clone(),
            item.code.clone(),
            item.title.clone().unwrap_or_default(),
            item.release_date.clone().unwrap_or_default(),
            item.cover_authority_id.clone().unwrap_or_default(),
            "1.48".to_owned(),
        ]);
    }
    response
}

fn finish_request(
    state: &JavdbCatalogState,
    category: CatalogCategory,
    generation: u64,
    items: Vec<CatalogItem>,
) -> Result<Vec<String>, &'static str> {
    let mut context = state.0.lock().map_err(|_| category.stale_error())?;
    let current = authority_mut(&mut context, category)
        .filter(|authority| authority.generation == generation)
        .ok_or_else(|| category.stale_error())?;
    current.items = items
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            let cover_authority_id = item.cover_url.as_ref().map(|url| {
                format!(
                    "javdb-cover-{generation}-{}-{}",
                    index + 1,
                    &md5_hex(url.as_bytes())[..8]
                )
            });
            AuthorizedCatalogItem {
                provider_item_id: item.provider_item_id,
                code: item.code,
                title: item.title,
                release_date: item.release_date,
                cover_authority_id,
                cover_url: item.cover_url,
            }
        })
        .collect();
    Ok(encode_catalog(category, generation, &current.items))
}

fn parse_error(category: CatalogCategory, error: CatalogDocumentError) -> &'static str {
    match error {
        CatalogDocumentError::Malformed => category.malformed_error(),
        CatalogDocumentError::Provider => category.provider_error(ProviderRequestError::Provider),
        CatalogDocumentError::Conflicting => category.conflicting_error(),
    }
}

pub(crate) fn fetch_catalog_with(
    state: &JavdbCatalogState,
    request: &JavdbCatalogRequest,
    mut fetch: impl FnMut(&str) -> Result<String, ProviderRequestError>,
) -> Result<Vec<String>, &'static str> {
    let category = CatalogCategory::parse(&request.category).ok_or(VR_PROVIDER_ERROR)?;
    if validated_request(request) != Some(category) {
        return Err(category.provider_error(ProviderRequestError::Provider));
    }
    let generation = begin_request(state, category)?;
    let mut items = Vec::new();
    let mut accepted_codes = HashMap::<String, String>::new();
    for url in listing_urls(request, category) {
        let document = fetch(&url).map_err(|error| category.provider_error(error))?;
        let page = parse_listing(&document).map_err(|error| parse_error(category, error))?;
        for item in page.items {
            if let Some(previous_code) = accepted_codes.get(&item.provider_item_id) {
                if previous_code != &item.code {
                    return Err(category.conflicting_error());
                }
                continue;
            }
            accepted_codes.insert(item.provider_item_id.clone(), item.code.clone());
            items.push(item);
        }
        if page.provider_empty {
            break;
        }
    }

    if category == CatalogCategory::Adult {
        items.retain(|item| {
            let url = format!(
                "{JAVDB_API_URL}/api/v4/movies/{}?from_rankings=false",
                item.provider_item_id
            );
            fetch(&url)
                .ok()
                .and_then(|document| adult_item_category(&document, &item.provider_item_id))
                == Some(CatalogCategory::Adult)
        });
    }
    items.truncate(request.count as usize);
    finish_request(state, category, generation, items)
}

pub(crate) fn invalidate_catalog(
    state: &JavdbCatalogState,
    category: &str,
) -> Result<(), &'static str> {
    let category = CatalogCategory::parse(category).ok_or(VR_PROVIDER_ERROR)?;
    let mut context = state
        .0
        .lock()
        .map_err(|_| category.provider_error(ProviderRequestError::Provider))?;
    context.generation = context
        .generation
        .checked_add(1)
        .ok_or_else(|| category.provider_error(ProviderRequestError::Provider))?;
    match category {
        CatalogCategory::Adult => context.adult = None,
        CatalogCategory::Vr => context.vr = None,
    }
    Ok(())
}

fn authorized_cover_url(
    state: &JavdbCatalogState,
    category: CatalogCategory,
    generation: u64,
    provider_item_id: &str,
    cover_authority_id: &str,
) -> Result<String, &'static str> {
    let context = state.0.lock().map_err(|_| category.stale_error())?;
    authority(&context, category)
        .filter(|authority| authority.generation == generation)
        .and_then(|authority| {
            authority
                .items
                .iter()
                .find(|item| item.provider_item_id == provider_item_id)
        })
        .filter(|item| item.cover_authority_id.as_deref() == Some(cover_authority_id))
        .and_then(|item| item.cover_url.clone())
        .ok_or_else(|| category.stale_error())
}

fn accepted_raster(bytes: &[u8]) -> bool {
    bytes.len() >= 12
        && bytes.len() <= JAVDB_COVER_MAX_BYTES
        && (bytes.starts_with(&[0xff, 0xd8, 0xff])
            || bytes.starts_with(&[0x89, b'P', b'N', b'G'])
            || bytes.starts_with(b"GIF8")
            || (bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP")))
}

pub(crate) fn fetch_cover_with(
    state: &JavdbCatalogState,
    category: &str,
    generation: &str,
    provider_item_id: &str,
    cover_authority_id: &str,
    fetch: impl FnOnce(&str) -> Result<Vec<u8>, ProviderRequestError>,
) -> Result<Vec<u8>, &'static str> {
    let category = CatalogCategory::parse(category).ok_or(VR_PROVIDER_ERROR)?;
    let generation = generation
        .parse::<u64>()
        .ok()
        .filter(|generation| *generation > 0)
        .ok_or_else(|| category.stale_error())?;
    if !valid_provider_item_id(provider_item_id)
        || cover_authority_id.is_empty()
        || cover_authority_id.contains("://")
    {
        return Err(category.stale_error());
    }
    let url = authorized_cover_url(
        state,
        category,
        generation,
        provider_item_id,
        cover_authority_id,
    )?;
    let bytes = fetch(&url).map_err(|error| category.provider_error(error))?;
    if !accepted_raster(&bytes) {
        return Err(category.provider_error(ProviderRequestError::Provider));
    }
    authorized_cover_url(
        state,
        category,
        generation,
        provider_item_id,
        cover_authority_id,
    )?;
    Ok(bytes)
}

fn md5_hex(input: &[u8]) -> String {
    const SHIFTS: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    const CONSTANTS: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];

    let mut padded = input.to_vec();
    let bit_length = (padded.len() as u64).wrapping_mul(8);
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_length.to_le_bytes());
    let mut state = [0x67452301_u32, 0xefcdab89, 0x98badcfe, 0x10325476];
    for block in padded.chunks_exact(64) {
        let mut words = [0_u32; 16];
        for (word, bytes) in words.iter_mut().zip(block.chunks_exact(4)) {
            *word = u32::from_le_bytes(bytes.try_into().expect("four-byte MD5 word"));
        }
        let [mut a, mut b, mut c, mut d] = state;
        for index in 0..64 {
            let (value, word_index) = if index < 16 {
                ((b & c) | (!b & d), index)
            } else if index < 32 {
                ((d & b) | (!d & c), (5 * index + 1) % 16)
            } else if index < 48 {
                (b ^ c ^ d, (3 * index + 5) % 16)
            } else {
                (c ^ (b | !d), (7 * index) % 16)
            };
            let next = b.wrapping_add(
                a.wrapping_add(value)
                    .wrapping_add(CONSTANTS[index])
                    .wrapping_add(words[word_index])
                    .rotate_left(SHIFTS[index]),
            );
            a = d;
            d = c;
            c = b;
            b = next;
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
    }
    state
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn javdb_signature(timestamp: u64) -> String {
    format!(
        "{timestamp}.{JAVDB_SIGNATURE_MIDDLE}.{}",
        md5_hex(format!("{timestamp}{JAVDB_SIGNATURE_SECRET}").as_bytes())
    )
}

fn parse_text_response(output: &[u8]) -> Result<String, ProviderRequestError> {
    let output = std::str::from_utf8(output).map_err(|_| ProviderRequestError::Provider)?;
    let (document, status) = output
        .rsplit_once(JAVDB_HTTP_STATUS_MARKER)
        .ok_or(ProviderRequestError::Provider)?;
    let status = status
        .trim()
        .parse::<u16>()
        .map_err(|_| ProviderRequestError::Provider)?;
    match status {
        200..=299 if document.len() <= JAVDB_RESPONSE_MAX_BYTES => Ok(document.to_owned()),
        404 | 410 | 451 => Err(ProviderRequestError::SourceUnavailable),
        0 => Err(ProviderRequestError::Network),
        _ => Err(ProviderRequestError::Provider),
    }
}

fn current_timestamp() -> Result<u64, ProviderRequestError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| ProviderRequestError::Provider)
}

#[cfg(target_os = "macos")]
pub(crate) fn fetch_api_document(url: &str) -> Result<String, ProviderRequestError> {
    let signature = javdb_signature(current_timestamp()?);
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
            "--user-agent",
            JAVDB_API_USER_AGENT,
            "--header",
            "Accept: application/json",
            "--header",
            "accept-language: en",
            "--header",
            &format!("jdsignature: {signature}"),
            "--write-out",
            JAVDB_HTTP_STATUS_WRITE_OUT,
            url,
        ])
        .output()
        .map_err(|_| ProviderRequestError::Network)?;
    if !output.status.success() {
        return Err(ProviderRequestError::Network);
    }
    parse_text_response(&output.stdout)
}

#[cfg(target_os = "windows")]
pub(crate) fn fetch_api_document(url: &str) -> Result<String, ProviderRequestError> {
    const URL_ENV: &str = "AUTO_VIDEO_JAVDB_URL";
    const SIGNATURE_ENV: &str = "AUTO_VIDEO_JAVDB_SIGNATURE";
    const USER_AGENT_ENV: &str = "AUTO_VIDEO_JAVDB_USER_AGENT";
    const SCRIPT: &str = r#"$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
try {
  $headers = @{ Accept = 'application/json'; 'accept-language' = 'en'; jdsignature = $env:AUTO_VIDEO_JAVDB_SIGNATURE; 'User-Agent' = $env:AUTO_VIDEO_JAVDB_USER_AGENT }
  $response = Invoke-WebRequest -UseBasicParsing -Uri $env:AUTO_VIDEO_JAVDB_URL -Headers $headers -MaximumRedirection 0 -TimeoutSec 20
  [Console]::Out.Write($response.Content)
  [Console]::Out.Write("`nAUTO_VIDEO_HTTP_STATUS:" + [int]$response.StatusCode)
} catch {
  $status = if ($null -eq $_.Exception.Response) { 0 } else { [int]$_.Exception.Response.StatusCode }
  [Console]::Out.Write("`nAUTO_VIDEO_HTTP_STATUS:" + $status)
}"#;
    let output = Command::new("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
        .arg(SCRIPT)
        .env(URL_ENV, url)
        .env(SIGNATURE_ENV, javdb_signature(current_timestamp()?))
        .env(USER_AGENT_ENV, JAVDB_API_USER_AGENT)
        .output()
        .map_err(|_| ProviderRequestError::Network)?;
    if !output.status.success() {
        return Err(ProviderRequestError::Network);
    }
    parse_text_response(&output.stdout)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn fetch_api_document(_url: &str) -> Result<String, ProviderRequestError> {
    Err(ProviderRequestError::Network)
}

fn binary_marker(output: &[u8]) -> Option<usize> {
    output
        .windows(JAVDB_HTTP_STATUS_MARKER.len())
        .rposition(|window| window == JAVDB_HTTP_STATUS_MARKER.as_bytes())
}

#[cfg(target_os = "macos")]
fn parse_binary_response(output: &[u8]) -> Result<Vec<u8>, ProviderRequestError> {
    let marker = binary_marker(output).ok_or(ProviderRequestError::Provider)?;
    let status = std::str::from_utf8(&output[marker + JAVDB_HTTP_STATUS_MARKER.len()..])
        .map_err(|_| ProviderRequestError::Provider)?
        .trim()
        .parse::<u16>()
        .map_err(|_| ProviderRequestError::Provider)?;
    match status {
        200..=299 if marker <= JAVDB_COVER_MAX_BYTES + 1 => Ok(output[..marker].to_vec()),
        404 | 410 | 451 => Err(ProviderRequestError::SourceUnavailable),
        0 => Err(ProviderRequestError::Network),
        _ => Err(ProviderRequestError::Provider),
    }
}

#[cfg(target_os = "windows")]
fn decode_base64(value: &str) -> Option<Vec<u8>> {
    fn digit(character: u8) -> Option<u8> {
        match character {
            b'A'..=b'Z' => Some(character - b'A'),
            b'a'..=b'z' => Some(character - b'a' + 26),
            b'0'..=b'9' => Some(character - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    if !value.len().is_multiple_of(4) {
        return None;
    }
    let mut output = Vec::with_capacity(value.len() / 4 * 3);
    for chunk in value.as_bytes().chunks_exact(4) {
        let first = digit(chunk[0])?;
        let second = digit(chunk[1])?;
        let third = (chunk[2] != b'=').then(|| digit(chunk[2])).flatten();
        let fourth = (chunk[3] != b'=').then(|| digit(chunk[3])).flatten();
        if chunk[2] == b'=' && chunk[3] != b'=' {
            return None;
        }
        output.push((first << 2) | (second >> 4));
        if let Some(third) = third {
            output.push((second << 4) | (third >> 2));
            if let Some(fourth) = fourth {
                output.push((third << 6) | fourth);
            }
        }
    }
    Some(output)
}

fn decode_cover_payload(payload: &[u8]) -> Result<Vec<u8>, ProviderRequestError> {
    if payload.len() < 2 || payload.len() > JAVDB_COVER_MAX_BYTES + 1 {
        return Err(ProviderRequestError::Provider);
    }
    let key = payload[0];
    let decoded: Vec<u8> = payload[1..].iter().map(|byte| byte ^ key).collect();
    accepted_raster(&decoded)
        .then_some(decoded)
        .ok_or(ProviderRequestError::Provider)
}

#[cfg(target_os = "macos")]
pub(crate) fn fetch_cover_bytes(url: &str) -> Result<Vec<u8>, ProviderRequestError> {
    let output = Command::new("/usr/bin/curl")
        .args([
            "--silent",
            "--show-error",
            "--connect-timeout",
            "5",
            "--max-time",
            "20",
            "--max-redirs",
            "0",
            "--max-filesize",
            "16777217",
            "--user-agent",
            JAVDB_API_USER_AGENT,
            "--header",
            "Accept: image/*",
            "--header",
            "Referer: https://javdb.com/",
            "--write-out",
            JAVDB_HTTP_STATUS_WRITE_OUT,
            url,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| ProviderRequestError::Network)?;
    if !output.status.success() {
        return Err(ProviderRequestError::Network);
    }
    decode_cover_payload(&parse_binary_response(&output.stdout)?)
}

#[cfg(target_os = "windows")]
pub(crate) fn fetch_cover_bytes(url: &str) -> Result<Vec<u8>, ProviderRequestError> {
    const URL_ENV: &str = "AUTO_VIDEO_JAVDB_COVER_URL";
    const USER_AGENT_ENV: &str = "AUTO_VIDEO_JAVDB_USER_AGENT";
    const SCRIPT: &str = r#"$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
try {
  $headers = @{ Accept = 'image/*'; Referer = 'https://javdb.com/'; 'User-Agent' = $env:AUTO_VIDEO_JAVDB_USER_AGENT }
  $response = Invoke-WebRequest -UseBasicParsing -Uri $env:AUTO_VIDEO_JAVDB_COVER_URL -Headers $headers -MaximumRedirection 0 -TimeoutSec 20
  $memory = New-Object System.IO.MemoryStream
  $response.RawContentStream.CopyTo($memory)
  if ($memory.Length -gt 16777217) { throw 'cover too large' }
  [Console]::Out.Write([Convert]::ToBase64String($memory.ToArray()))
  [Console]::Out.Write("`nAUTO_VIDEO_HTTP_STATUS:" + [int]$response.StatusCode)
} catch {
  $status = if ($null -eq $_.Exception.Response) { 0 } else { [int]$_.Exception.Response.StatusCode }
  [Console]::Out.Write("`nAUTO_VIDEO_HTTP_STATUS:" + $status)
}"#;
    let output = Command::new("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
        .arg(SCRIPT)
        .env(URL_ENV, url)
        .env(USER_AGENT_ENV, JAVDB_API_USER_AGENT)
        .output()
        .map_err(|_| ProviderRequestError::Network)?;
    if !output.status.success() {
        return Err(ProviderRequestError::Network);
    }
    let marker = binary_marker(&output.stdout).ok_or(ProviderRequestError::Provider)?;
    let status = std::str::from_utf8(&output.stdout[marker + JAVDB_HTTP_STATUS_MARKER.len()..])
        .map_err(|_| ProviderRequestError::Provider)?
        .trim()
        .parse::<u16>()
        .map_err(|_| ProviderRequestError::Provider)?;
    if status == 0 {
        return Err(ProviderRequestError::Network);
    }
    if !(200..=299).contains(&status) {
        return Err(ProviderRequestError::Provider);
    }
    let encoded = std::str::from_utf8(&output.stdout[..marker])
        .map_err(|_| ProviderRequestError::Provider)?
        .trim();
    decode_cover_payload(&decode_base64(encoded).ok_or(ProviderRequestError::Provider)?)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn fetch_cover_bytes(_url: &str) -> Result<Vec<u8>, ProviderRequestError> {
    Err(ProviderRequestError::Network)
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;

    fn request(category: &str) -> JavdbCatalogRequest {
        JavdbCatalogRequest {
            category: category.to_owned(),
            mode: "category".to_owned(),
            period: "daily".to_owned(),
            year: None,
            month: None,
            sort: "newest".to_owned(),
            count: 25,
        }
    }

    fn detail(item_id: &str, tags: &str) -> String {
        format!(r#"{{"success":1,"data":{{"movie":{{"id":"{item_id}","tags":{tags}}}}}}}"#)
    }

    fn jpeg() -> Vec<u8> {
        vec![0xff, 0xd8, 0xff, 0xe0, 0, 16, 0, 0, 0, 0, 0, 0]
    }

    #[test]
    fn signs_requests_with_the_captured_mobile_identity() {
        assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(
            javdb_signature(1_712_345_678),
            "1712345678.lpw6vgqzsp.091fb26889e107ccb852b17532498a5b"
        );
    }

    #[test]
    fn constructs_adult_period_category_sort_count_and_exact_vr_tag_requests() {
        let state = JavdbCatalogState::default();
        let urls = RefCell::new(Vec::new());
        for period in ["daily", "weekly", "monthly"] {
            let ranking = JavdbCatalogRequest {
                category: "adult".to_owned(),
                mode: "ranking".to_owned(),
                period: period.to_owned(),
                year: None,
                month: None,
                sort: "newest".to_owned(),
                count: 25,
            };
            fetch_catalog_with(&state, &ranking, |url| {
                urls.borrow_mut().push(url.to_owned());
                Ok(if url.contains("rankings") {
                    r#"{"success":1,"data":{"movies":[]}}"#.to_owned()
                } else {
                    unreachable!()
                })
            })
            .expect("Adult ranking period must be accepted");
        }
        assert_eq!(
            urls.borrow().as_slice(),
            [
                "https://apidd.spthgb.com/api/v1/rankings?type=0&period=daily",
                "https://apidd.spthgb.com/api/v1/rankings?type=0&period=weekly",
                "https://apidd.spthgb.com/api/v1/rankings?type=0&period=monthly",
            ]
        );

        for (sort, provider_sort, order) in [
            ("newest", "release", "desc"),
            ("oldest", "release", "asc"),
            ("recently-updated", "update", "desc"),
            ("top-rated", "score", "desc"),
            ("most-viewed", "hit", "desc"),
            ("most-wanted", "want_watch_count", "desc"),
            ("most-watched", "watched_count", "desc"),
        ] {
            let mut adult = request("adult");
            let current_year = current_calendar_year().expect("current year must be available");
            adult.year = Some(current_year.to_string());
            adult.month = Some(6);
            adult.sort = sort.to_owned();
            adult.count = 10;
            let url = RefCell::new(String::new());
            fetch_catalog_with(&state, &adult, |value| {
                url.replace(value.to_owned());
                Ok(r#"{"success":1,"data":{"movies":[]}}"#.to_owned())
            })
            .expect("Adult category request must be accepted");
            assert_eq!(
                url.into_inner(),
                format!("https://apidd.spthgb.com/api/v1/movies/tags?filter_by=0%3At%3Am%3A%3A{current_year}%3A%3A6&filter_by_tags=&sort_by={provider_sort}&order_by={order}&page=1&limit=10")
            );
        }

        for invalid_year in [
            "2000".to_owned(),
            (current_calendar_year().expect("current year must be available") + 1).to_string(),
        ] {
            let mut invalid = request("adult");
            invalid.year = Some(invalid_year);
            let dispatched = Cell::new(false);
            assert!(fetch_catalog_with(&state, &invalid, |_| {
                dispatched.set(true);
                unreachable!()
            })
            .is_err());
            assert!(!dispatched.get());
        }

        let mut vr = request("vr");
        vr.count = 100;
        let urls = RefCell::new(Vec::new());
        fetch_catalog_with(&state, &vr, |url| {
            urls.borrow_mut().push(url.to_owned());
            Ok(r#"{"success":1,"data":{"movies":[]}}"#.to_owned())
        })
        .expect("VR request must be accepted");
        assert_eq!(urls.borrow().len(), 1);
        assert!(urls.borrow()[0].contains("filter_by=0%3At%3Am%3A212%3A%3A%3A"));
    }

    #[test]
    fn preserves_source_order_while_skipping_only_unusable_rows() {
        let state = JavdbCatalogState::default();
        let response = fetch_catalog_with(&state, &request("vr"), |_| {
            Ok(r#"{"success":1,"data":{"movies":[null,{"id":"VrA","number":"MDVR-419","title":null,"release_date":null,"cover_url":null},{"id":"MissingCode","cover_url":"https://tp.cmastd.com/a.jpg"},{"id":"Bad Id!","number":"MDVR-420"},{"id":"VrB","number":"MDVR-422","title":"Second","cover_url":"https://tp.spfcas.com/b.jpg"},{"id":"VrC","number":"MDVR-430","cover_url":"https://tp.new-cdn.com/c.jpg"}]}}"#.to_owned())
        })
        .expect("mixed rows must preserve valid results");
        assert_eq!(response[1], "3");
        assert_eq!(
            &response[2..9],
            ["vr", "VrA", "MDVR-419", "", "", "", "1.48"]
        );
        assert_eq!(&response[9..12], ["vr", "VrB", "MDVR-422"]);
        assert_eq!(&response[16..19], ["vr", "VrC", "MDVR-430"]);
        assert!(!response[14].is_empty());
        assert!(!response[21].is_empty());

        let mut two_pages = request("vr");
        two_pages.count = 100;
        let response = fetch_catalog_with(&state, &two_pages, |url| {
            Ok(if url.contains("page=1") {
                r#"{"success":1,"data":{"movies":[{"id":"MissingCode"}]}}"#.to_owned()
            } else {
                r#"{"success":1,"data":{"movies":[{"id":"Later","number":"mdvr_00419"}]}}"#
                    .to_owned()
            })
        })
        .expect("unusable rows must not hide later provider pages");
        assert_eq!(response[1], "1");
        assert_eq!(response[4], "MDVR-419");
    }

    #[test]
    fn distinguishes_malformed_empty_identical_and_conflicting_provider_results() {
        let state = JavdbCatalogState::default();
        assert_eq!(
            fetch_catalog_with(&state, &request("vr"), |_| Ok("{}".to_owned())),
            Err(VR_JAVDB_MALFORMED)
        );
        assert_eq!(
            fetch_catalog_with(&state, &request("vr"), |_| {
                Ok(r#"{"success":0,"data":{}}"#.to_owned())
            }),
            Err(VR_PROVIDER_ERROR)
        );
        let empty = fetch_catalog_with(&state, &request("vr"), |_| {
            Ok(r#"{"success":1,"data":{"movies":[]}}"#.to_owned())
        })
        .expect("valid empty result must succeed");
        assert_eq!(empty[1], "0");
        let identical = fetch_catalog_with(&state, &request("vr"), |_| {
            Ok(r#"{"success":1,"data":{"movies":[{"id":"Same","number":"MDVR-419"},{"id":"Same","number":"mdvr 419"}]}}"#.to_owned())
        })
        .expect("identical duplicates must deduplicate");
        assert_eq!(identical[1], "1");
        assert_eq!(
            fetch_catalog_with(&state, &request("vr"), |_| {
                Ok(r#"{"success":1,"data":{"movies":[{"id":"Same","number":"MDVR-419"},{"id":"Same","number":"MDVR-422"}]}}"#.to_owned())
            }),
            Err(VR_JAVDB_CONFLICTING)
        );
    }

    #[test]
    fn drops_only_adult_rows_with_vr_failed_or_inconclusive_category_checks() {
        let state = JavdbCatalogState::default();
        let calls = RefCell::new(Vec::new());
        let response = fetch_catalog_with(&state, &request("adult"), |url| {
            calls.borrow_mut().push(url.to_owned());
            if url.contains("movies/tags") {
                return Ok(r#"{"success":1,"data":{"movies":[{"id":"AdultA","number":"ADLT-123"},{"id":"VrA","number":"MDVR-419"},{"id":"Failed","number":"ADLT-124"},{"id":"Unknown","number":"ADLT-125"},{"id":"AdultB","number":"ADLT-126"}]}}"#.to_owned());
            }
            if url.contains("AdultA") {
                Ok(detail("AdultA", r#"[{"id":"28"}]"#))
            } else if url.contains("VrA") {
                Ok(detail("VrA", r#"[{"id":"212"}]"#))
            } else if url.contains("Failed") {
                Err(ProviderRequestError::Network)
            } else if url.contains("Unknown") {
                Ok(detail("Unknown", "null"))
            } else {
                Ok(detail("AdultB", "[]"))
            }
        })
        .expect("one Adult category check failure must stay local");
        assert_eq!(response[1], "2");
        assert_eq!(response[4], "ADLT-123");
        assert_eq!(response[11], "ADLT-126");
        assert_eq!(calls.borrow().len(), 6);
    }

    #[test]
    fn accepts_only_exact_retained_rotating_cover_authority() {
        let state = JavdbCatalogState::default();
        let response = fetch_catalog_with(&state, &request("vr"), |_| {
            Ok(r#"{"success":1,"data":{"movies":[{"id":"A","number":"MDVR-419","cover_url":"https://tp.cmastd.com/a.jpg"},{"id":"B","number":"MDVR-422","cover_url":"https://tp.spfcas.com/b.jpg"},{"id":"C","number":"MDVR-430","cover_url":"https://tp.rotating-7.com/c.jpg"}]}}"#.to_owned())
        })
        .expect("valid rotating covers must be retained");
        let generation = &response[0];
        let dispatched = Cell::new(false);
        assert_eq!(
            fetch_cover_with(&state, "vr", generation, "A", &response[7], |_| {
                dispatched.set(true);
                Ok(vec![0, 1, 2, 3])
            }),
            Err(VR_PROVIDER_ERROR)
        );
        assert!(dispatched.get());
        for (item_index, item_id) in [(0, "A"), (1, "B"), (2, "C")] {
            let authority_id = &response[2 + item_index * 7 + 5];
            let dispatched = Cell::new(false);
            assert_eq!(
                fetch_cover_with(&state, "vr", generation, item_id, authority_id, |_| {
                    dispatched.set(true);
                    Ok(jpeg())
                }),
                Ok(jpeg())
            );
            assert!(dispatched.get());
        }
    }

    #[test]
    fn rejects_forged_stale_cross_item_cross_category_and_raw_url_without_dispatch() {
        let state = JavdbCatalogState::default();
        let response = fetch_catalog_with(&state, &request("vr"), |_| {
            Ok(r#"{"success":1,"data":{"movies":[{"id":"A","number":"MDVR-419","cover_url":"https://tp.cmastd.com/a.jpg"},{"id":"B","number":"MDVR-422","cover_url":"https://tp.spfcas.com/b.jpg"}]}}"#.to_owned())
        })
        .expect("cover authority must be established");
        let generation = response[0].clone();
        let authority = response[7].clone();
        for (category, generation, item, token) in [
            ("adult", generation.as_str(), "A", authority.as_str()),
            ("vr", generation.as_str(), "B", authority.as_str()),
            ("vr", "999", "A", authority.as_str()),
            ("vr", generation.as_str(), "A", "unknown"),
            (
                "vr",
                generation.as_str(),
                "A",
                "https://tp.evil.com/forged.jpg",
            ),
        ] {
            let dispatched = Cell::new(false);
            assert!(
                fetch_cover_with(&state, category, generation, item, token, |_| {
                    dispatched.set(true);
                    Ok(jpeg())
                })
                .is_err()
            );
            assert!(!dispatched.get());
        }
        invalidate_catalog(&state, "vr").expect("invalidation must succeed");
        let dispatched = Cell::new(false);
        assert!(
            fetch_cover_with(&state, "vr", &generation, "A", &authority, |_| {
                dispatched.set(true);
                Ok(jpeg())
            })
            .is_err()
        );
        assert!(!dispatched.get());
    }

    #[test]
    fn rejects_unsafe_cover_urls_and_invalid_or_oversized_rasters() {
        for value in [
            "http://tp.cmastd.com/a.jpg",
            "https://tp.cmastd.com.evil.example/a.jpg",
            "https://user:secret@tp.cmastd.com/a.jpg",
            "https://tp.cmastd.com:443/a.jpg",
            "https://tp.cmastd.com\\@evil.example/a.jpg",
            "https://tp.extra.cmastd.com/a.jpg",
            "https://tp.-bad.com/a.jpg",
            "https://tp.bad-.com/a.jpg",
            "https://tp.c mastd.com/a.jpg",
        ] {
            assert!(!valid_cover_url(value), "{value} must be rejected");
        }
        assert!(valid_cover_url("https://tp.another-cdn7.com/a.jpg"));
        assert_eq!(
            decode_cover_payload(&[1, b'n' ^ 1, b'o' ^ 1]),
            Err(ProviderRequestError::Provider)
        );
        assert!(!accepted_raster(&vec![0; JAVDB_COVER_MAX_BYTES + 1]));
    }

    #[test]
    fn a_late_catalog_or_cover_cannot_replace_a_newer_generation() {
        let state = JavdbCatalogState::default();
        let first_generation =
            begin_request(&state, CatalogCategory::Vr).expect("first request must begin");
        let second = fetch_catalog_with(&state, &request("vr"), |_| {
            Ok(r#"{"success":1,"data":{"movies":[{"id":"Current","number":"MDVR-422","cover_url":"https://tp.cmastd.com/current.jpg"}]}}"#.to_owned())
        })
        .expect("new request must finish");
        assert_eq!(
            finish_request(
                &state,
                CatalogCategory::Vr,
                first_generation,
                vec![CatalogItem {
                    provider_item_id: "Late".to_owned(),
                    code: "MDVR-419".to_owned(),
                    title: None,
                    release_date: None,
                    cover_url: None,
                }]
            ),
            Err(VR_JAVDB_STALE)
        );
        let cover_started = Cell::new(false);
        let cover_result =
            fetch_cover_with(&state, "vr", &second[0], "Current", &second[7], |_| {
                cover_started.set(true);
                invalidate_catalog(&state, "vr").expect("cover must invalidate");
                Ok(jpeg())
            });
        assert!(cover_started.get());
        assert_eq!(cover_result, Err(VR_JAVDB_STALE));
    }
}
