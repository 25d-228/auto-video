use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::process::Command;
#[cfg(target_os = "macos")]
use std::process::Stdio;

use crate::{
    vr_torrent::{product_code_display_form, product_code_forms, JsonParser, JsonValue},
    ProviderRequestError, ADULT_NETWORK_ERROR, ADULT_PROVIDER_ERROR, ADULT_SOURCE_UNAVAILABLE,
    VR_NETWORK_ERROR, VR_PROVIDER_ERROR, VR_SOURCE_UNAVAILABLE,
};

const JAVDB_API_URL: &str = "https://apidd.spthgb.com";
const JAVDB_API_USER_AGENT: &str = "Dart/3.5 (dart:io)";
const JAVDB_SIGNATURE_MIDDLE: &str = "lpw6vgqzsp";
const JAVDB_SIGNATURE_SECRET: &str = "71cf27bb3c0bcdf207b64abecddc970098c7421ee7203b9cdae54478478a199e7d5a6e1a57691123c1a931c057842fb73ba3b3c83bcd69c17ccf174081e3d8aa";
const JAVDB_RESPONSE_MAX_BYTES: usize = 4 * 1024 * 1024;
const JAVDB_COVER_MAX_BYTES: usize = 16 * 1024 * 1024;
const JAVDB_PREVIEW_LIMIT: usize = 24;
// Four global worker slots reduce serial latency while bounding provider processes and native threads.
const ADULT_CATEGORY_CHECK_CONCURRENCY: usize = 4;
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
    pub context_generation: String,
    pub mode: String,
    pub period: String,
    pub year: Option<String>,
    pub month: Option<u8>,
    pub sort: String,
    pub count: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JavdbDetailRequest {
    pub category: String,
    pub context_generation: String,
    pub request_generation: String,
    pub provider_item_id: String,
    pub code: String,
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
    context_generation: u64,
    items: Vec<AuthorizedCatalogItem>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DetailImageAuthority {
    authority_id: String,
    url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DetailAuthority {
    generation: u64,
    catalog_context_generation: u64,
    catalog_request_generation: u64,
    provider_item_id: String,
    code: String,
    title: Option<String>,
    original_title: Option<String>,
    release_date: Option<String>,
    duration: Option<String>,
    summary: Option<String>,
    actors: Vec<String>,
    tags: Vec<String>,
    cover: Option<DetailImageAuthority>,
    previews: Vec<DetailImageAuthority>,
}

#[derive(Default)]
struct CatalogContext {
    generation: u64,
    detail_generation: u64,
    adult_context_generation: u64,
    vr_context_generation: u64,
    adult_checks_in_progress: usize,
    #[cfg(test)]
    adult_check_waiters: Vec<u64>,
    adult: Option<CatalogAuthority>,
    vr: Option<CatalogAuthority>,
    adult_detail: Option<DetailAuthority>,
    vr_detail: Option<DetailAuthority>,
}

struct AdultCategoryCheckPermit<'a> {
    state: &'a JavdbCatalogState,
}

impl Drop for AdultCategoryCheckPermit<'_> {
    fn drop(&mut self) {
        if let Ok(mut context) = self.state.0.lock() {
            context.adult_checks_in_progress = context.adult_checks_in_progress.saturating_sub(1);
            drop(context);
            self.state.1.notify_all();
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct JavdbCatalogState(Arc<Mutex<CatalogContext>>, Arc<Condvar>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CatalogDocumentError {
    Malformed,
    Provider,
    Conflicting,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AdultCategoryCheck {
    Adult,
    NotAdult,
    Inconclusive,
    Conflicting,
}

#[derive(Debug, PartialEq, Eq)]
struct ParsedListing {
    items: Vec<CatalogItem>,
    provider_empty: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct ParsedDetail {
    title: Option<String>,
    original_title: Option<String>,
    release_date: Option<String>,
    duration: Option<String>,
    summary: Option<String>,
    actors: Vec<String>,
    tags: Vec<String>,
    cover_url: Option<String>,
    preview_urls: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExactLibraryItem {
    pub provider_item_id: String,
    pub display_code: String,
    pub title: Option<String>,
    pub release_date: Option<String>,
    pub duration: Option<String>,
    pub actors: Vec<String>,
    pub cover_url: Option<String>,
}

fn valid_provider_item_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|character| character.is_ascii_alphanumeric())
}

fn canonical_product_code(value: &str) -> Option<String> {
    crate::vr_torrent::canonical_product_code(value)
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

fn optional_number_text(object: &BTreeMap<String, JsonValue>, key: &str) -> Option<String> {
    match object.get(key) {
        Some(JsonValue::Number(value)) => Some(value.clone()),
        Some(JsonValue::String(value)) => {
            let value = value.trim();
            (!value.is_empty()).then(|| value.to_owned())
        }
        _ => None,
    }
}

fn optional_names(object: &BTreeMap<String, JsonValue>, key: &str) -> Vec<String> {
    let Some(JsonValue::Array(values)) = object.get(key) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for value in values {
        let JsonValue::Object(value) = value else {
            continue;
        };
        let Some(name) = optional_text(value, "name") else {
            continue;
        };
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
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

pub(crate) fn valid_library_cover_url(value: &str) -> bool {
    valid_cover_url(value)
}

fn cover_url(movie: &BTreeMap<String, JsonValue>) -> Option<String> {
    ["cover_url", "thumb_url"].into_iter().find_map(|key| {
        let JsonValue::String(value) = movie.get(key)? else {
            return None;
        };
        valid_cover_url(value).then(|| value.clone())
    })
}

fn exact_cover_url(
    movie: &BTreeMap<String, JsonValue>,
) -> Result<Option<String>, CatalogDocumentError> {
    let mut accepted = None;
    for key in ["cover_url", "thumb_url"] {
        let url = match movie.get(key) {
            None | Some(JsonValue::Null) => continue,
            Some(JsonValue::String(url)) if valid_cover_url(url) => url,
            Some(_) => return Err(CatalogDocumentError::Malformed),
        };
        if accepted.as_ref().is_some_and(|current| current != url) {
            return Err(CatalogDocumentError::Conflicting);
        }
        accepted = Some(url.clone());
    }
    Ok(accepted)
}

fn exact_listing_items(
    document: &str,
    requested_identity: &str,
) -> Result<Vec<CatalogItem>, CatalogDocumentError> {
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

    let relevant_ids = movies
        .iter()
        .filter_map(|value| match value {
            JsonValue::Object(movie) => Some(movie),
            _ => None,
        })
        .filter_map(|movie| {
            let JsonValue::String(provider_item_id) = movie.get("id")? else {
                return None;
            };
            let JsonValue::String(number) = movie.get("number")? else {
                return None;
            };
            (valid_provider_item_id(provider_item_id)
                && canonical_product_code(number).as_deref() == Some(requested_identity))
            .then(|| provider_item_id.clone())
        })
        .collect::<HashSet<_>>();
    let mut exact_items = HashMap::<String, CatalogItem>::new();
    for movie in movies.iter().filter_map(|value| match value {
        JsonValue::Object(movie) => Some(movie),
        _ => None,
    }) {
        let Some(JsonValue::String(provider_item_id)) = movie.get("id") else {
            continue;
        };
        if !relevant_ids.contains(provider_item_id) {
            continue;
        }
        let Some(JsonValue::String(number)) = movie.get("number") else {
            return Err(CatalogDocumentError::Malformed);
        };
        let forms = product_code_forms(number).ok_or(CatalogDocumentError::Malformed)?;
        let item = CatalogItem {
            provider_item_id: provider_item_id.clone(),
            code: forms.display,
            title: optional_text(movie, "title").or_else(|| optional_text(movie, "origin_title")),
            release_date: optional_text(movie, "release_date"),
            cover_url: exact_cover_url(movie)?,
        };
        if canonical_product_code(&item.code).as_deref() != Some(requested_identity) {
            return Err(CatalogDocumentError::Conflicting);
        }
        if exact_items
            .insert(provider_item_id.clone(), item.clone())
            .is_some_and(|previous| previous != item)
        {
            return Err(CatalogDocumentError::Conflicting);
        }
    }
    let mut exact_items = exact_items.into_values().collect::<Vec<_>>();
    exact_items.sort_by(|left, right| left.provider_item_id.cmp(&right.provider_item_id));
    Ok(exact_items)
}

fn exact_detail_cover_url(document: &str) -> Result<Option<String>, CatalogDocumentError> {
    let Some(JsonValue::Object(envelope)) = JsonParser::new(document).parse() else {
        return Err(CatalogDocumentError::Malformed);
    };
    let Some(JsonValue::Object(data)) = envelope.get("data") else {
        return Err(CatalogDocumentError::Malformed);
    };
    let Some(JsonValue::Object(movie)) = data.get("movie") else {
        return Err(CatalogDocumentError::Malformed);
    };
    exact_cover_url(movie)
}

fn preview_urls(movie: &BTreeMap<String, JsonValue>) -> Vec<String> {
    let Some(JsonValue::Array(previews)) = movie.get("preview_images") else {
        return Vec::new();
    };
    let mut urls = Vec::new();
    for preview in previews {
        let JsonValue::Object(preview) = preview else {
            continue;
        };
        let url = ["large_url", "thumb_url"].into_iter().find_map(|key| {
            let JsonValue::String(value) = preview.get(key)? else {
                return None;
            };
            valid_cover_url(value).then(|| value.clone())
        });
        if let Some(url) = url {
            if !urls.contains(&url) {
                urls.push(url);
            }
            if urls.len() == JAVDB_PREVIEW_LIMIT {
                break;
            }
        }
    }
    urls
}

fn valid_detail_tag_id(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|character| character.is_ascii_digit())
}

fn parse_detail(
    document: &str,
    category: CatalogCategory,
    provider_item_id: &str,
    code: &str,
) -> Result<ParsedDetail, CatalogDocumentError> {
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
    let Some(JsonValue::Object(movie)) = data.get("movie") else {
        return Err(CatalogDocumentError::Malformed);
    };
    let Some(JsonValue::String(response_item_id)) = movie.get("id") else {
        return Err(CatalogDocumentError::Malformed);
    };
    if !valid_provider_item_id(response_item_id) || response_item_id != provider_item_id {
        return Err(CatalogDocumentError::Conflicting);
    }
    let Some(JsonValue::String(number)) = movie.get("number") else {
        return Err(CatalogDocumentError::Malformed);
    };
    if canonical_product_code(number) != canonical_product_code(code) {
        return Err(CatalogDocumentError::Conflicting);
    }

    let Some(JsonValue::Array(tags)) = movie.get("tags") else {
        return Err(CatalogDocumentError::Malformed);
    };
    let mut valid_tag_ids = Vec::new();
    let mut valid_tag_names = Vec::new();
    for tag in tags {
        let JsonValue::Object(tag) = tag else {
            continue;
        };
        let Some(JsonValue::String(tag_id)) = tag.get("id") else {
            continue;
        };
        if !valid_detail_tag_id(tag_id) {
            continue;
        }
        if !valid_tag_ids.contains(tag_id) {
            valid_tag_ids.push(tag_id.clone());
        }
        if let Some(name) = optional_text(tag, "name") {
            if !valid_tag_names.contains(&name) {
                valid_tag_names.push(name);
            }
        }
    }
    if !tags.is_empty() && valid_tag_ids.is_empty() {
        return Err(CatalogDocumentError::Malformed);
    }
    let has_vr_tag = valid_tag_ids.iter().any(|tag_id| tag_id == "212");
    if category == CatalogCategory::Adult && has_vr_tag {
        return Err(CatalogDocumentError::Conflicting);
    }
    if category == CatalogCategory::Vr && !has_vr_tag {
        return Err(if valid_tag_ids.is_empty() {
            CatalogDocumentError::Malformed
        } else {
            CatalogDocumentError::Conflicting
        });
    }

    Ok(ParsedDetail {
        title: optional_text(movie, "title"),
        original_title: optional_text(movie, "origin_title"),
        release_date: optional_text(movie, "release_date"),
        duration: optional_number_text(movie, "duration"),
        summary: optional_text(movie, "summary"),
        actors: optional_names(movie, "actors"),
        tags: valid_tag_names,
        cover_url: cover_url(movie),
        preview_urls: preview_urls(movie),
    })
}

pub(crate) fn fetch_exact_library_item_with(
    category: &str,
    code: &str,
    fetch: &mut impl FnMut(&str) -> Result<String, ProviderRequestError>,
) -> Result<Option<ExactLibraryItem>, ProviderRequestError> {
    let category = CatalogCategory::parse(category).ok_or(ProviderRequestError::Provider)?;
    let requested_identity = canonical_product_code(code).ok_or(ProviderRequestError::Provider)?;
    if product_code_display_form(code).as_deref() != Some(code) {
        return Err(ProviderRequestError::Provider);
    }

    let listing = fetch(&format!(
        "{JAVDB_API_URL}/api/v2/search?q={code}&type=movie"
    ))?;
    let mut exact_items = exact_listing_items(&listing, &requested_identity)
        .map_err(|_| ProviderRequestError::Provider)?;
    if exact_items.is_empty() {
        return Ok(None);
    }
    if exact_items.len() != 1 {
        return Err(ProviderRequestError::Provider);
    }
    let item = exact_items
        .pop()
        .expect("one exact JavDB Library item was established");
    let detail_document = fetch(&format!(
        "{JAVDB_API_URL}/api/v4/movies/{}?from_rankings=false",
        item.provider_item_id
    ))?;
    let mut detail = parse_detail(
        &detail_document,
        category,
        &item.provider_item_id,
        &item.code,
    )
    .map_err(|_| ProviderRequestError::Provider)?;
    detail.cover_url =
        exact_detail_cover_url(&detail_document).map_err(|_| ProviderRequestError::Provider)?;

    Ok(Some(ExactLibraryItem {
        provider_item_id: item.provider_item_id,
        display_code: item.code,
        title: detail.title.or(item.title),
        release_date: detail.release_date.or(item.release_date),
        duration: detail.duration.map(|duration| {
            if duration.ends_with(" min") {
                duration
            } else {
                format!("{duration} min")
            }
        }),
        actors: detail.actors,
        cover_url: detail.cover_url.or(item.cover_url),
    }))
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
        let Some(forms) = product_code_forms(number) else {
            continue;
        };
        if let Some(previous_code) = accepted_codes.get(provider_item_id) {
            if previous_code != &forms.display {
                return Err(CatalogDocumentError::Conflicting);
            }
            continue;
        }
        accepted_codes.insert(provider_item_id.clone(), forms.display.clone());
        items.push(CatalogItem {
            provider_item_id: provider_item_id.clone(),
            code: forms.display,
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

fn adult_item_category(
    document: &str,
    provider_item_id: &str,
    retained_code: &str,
) -> AdultCategoryCheck {
    let Some(JsonValue::Object(envelope)) = JsonParser::new(document).parse() else {
        return AdultCategoryCheck::Inconclusive;
    };
    if envelope.get("success") != Some(&JsonValue::Number("1".to_owned())) {
        return AdultCategoryCheck::Inconclusive;
    }
    let Some(JsonValue::Object(data)) = envelope.get("data") else {
        return AdultCategoryCheck::Inconclusive;
    };
    let Some(JsonValue::Object(movie)) = data.get("movie") else {
        return AdultCategoryCheck::Inconclusive;
    };
    if movie.get("id") != Some(&JsonValue::String(provider_item_id.to_owned())) {
        return AdultCategoryCheck::Inconclusive;
    }
    match movie.get("number") {
        None | Some(JsonValue::Null) => {}
        Some(JsonValue::String(number)) => match canonical_product_code(number) {
            Some(code) if Some(&code) == canonical_product_code(retained_code).as_ref() => {}
            Some(_) => return AdultCategoryCheck::Conflicting,
            None => return AdultCategoryCheck::Inconclusive,
        },
        Some(_) => return AdultCategoryCheck::Inconclusive,
    }
    let Some(JsonValue::Array(tags)) = movie.get("tags") else {
        return AdultCategoryCheck::Inconclusive;
    };
    let mut is_vr = false;
    for tag in tags {
        let JsonValue::Object(tag) = tag else {
            return AdultCategoryCheck::Inconclusive;
        };
        let Some(JsonValue::String(tag_id)) = tag.get("id") else {
            return AdultCategoryCheck::Inconclusive;
        };
        is_vr |= tag_id == "212";
    }
    if is_vr {
        AdultCategoryCheck::NotAdult
    } else {
        AdultCategoryCheck::Adult
    }
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
    if request
        .context_generation
        .parse::<u64>()
        .ok()
        .is_none_or(|generation| generation == 0)
        || !matches!(request.count, 10 | 25 | 50 | 100)
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
    context_generation: u64,
) -> Result<u64, &'static str> {
    let mut context = state
        .0
        .lock()
        .map_err(|_| category.provider_error(ProviderRequestError::Provider))?;
    let current_context_generation = match category {
        CatalogCategory::Adult => &mut context.adult_context_generation,
        CatalogCategory::Vr => &mut context.vr_context_generation,
    };
    if context_generation <= *current_context_generation {
        return Err(category.stale_error());
    }
    *current_context_generation = context_generation;
    context.generation = context
        .generation
        .checked_add(1)
        .ok_or_else(|| category.provider_error(ProviderRequestError::Provider))?;
    let authority = CatalogAuthority {
        generation: context.generation,
        context_generation,
        items: Vec::new(),
    };
    match category {
        CatalogCategory::Adult => {
            context.adult = Some(authority);
            context.adult_detail = None;
        }
        CatalogCategory::Vr => {
            context.vr = Some(authority);
            context.vr_detail = None;
        }
    }
    let generation = context.generation;
    drop(context);
    if category == CatalogCategory::Adult {
        state.1.notify_all();
    }
    Ok(generation)
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

fn detail_authority(
    context: &CatalogContext,
    category: CatalogCategory,
) -> Option<&DetailAuthority> {
    match category {
        CatalogCategory::Adult => context.adult_detail.as_ref(),
        CatalogCategory::Vr => context.vr_detail.as_ref(),
    }
}

fn set_detail_authority(
    context: &mut CatalogContext,
    category: CatalogCategory,
    detail: Option<DetailAuthority>,
) {
    match category {
        CatalogCategory::Adult => context.adult_detail = detail,
        CatalogCategory::Vr => context.vr_detail = detail,
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

fn request_is_current(
    state: &JavdbCatalogState,
    category: CatalogCategory,
    generation: u64,
) -> Result<bool, &'static str> {
    let context = state
        .0
        .lock()
        .map_err(|_| category.provider_error(ProviderRequestError::Provider))?;
    Ok(authority(&context, category).is_some_and(|authority| authority.generation == generation))
}

fn require_current_request(
    state: &JavdbCatalogState,
    category: CatalogCategory,
    generation: u64,
) -> Result<(), &'static str> {
    if request_is_current(state, category, generation)? {
        Ok(())
    } else {
        Err(category.stale_error())
    }
}

fn reserve_adult_category_check(
    state: &JavdbCatalogState,
    generation: u64,
) -> Result<AdultCategoryCheckPermit<'_>, &'static str> {
    let mut context = state
        .0
        .lock()
        .map_err(|_| CatalogCategory::Adult.provider_error(ProviderRequestError::Provider))?;
    loop {
        if authority(&context, CatalogCategory::Adult)
            .is_none_or(|authority| authority.generation != generation)
        {
            return Err(CatalogCategory::Adult.stale_error());
        }
        if context.adult_checks_in_progress < ADULT_CATEGORY_CHECK_CONCURRENCY {
            context.adult_checks_in_progress += 1;
            return Ok(AdultCategoryCheckPermit { state });
        }
        #[cfg(test)]
        {
            context.adult_check_waiters.push(generation);
            state.1.notify_all();
        }
        context = state
            .1
            .wait(context)
            .map_err(|_| CatalogCategory::Adult.provider_error(ProviderRequestError::Provider))?;
        #[cfg(test)]
        {
            let waiter = context
                .adult_check_waiters
                .iter()
                .position(|waiter| *waiter == generation)
                .expect("the waiting Adult request must remain registered");
            context.adult_check_waiters.remove(waiter);
            state.1.notify_all();
        }
    }
}

fn verify_adult_items<F>(
    state: &JavdbCatalogState,
    generation: u64,
    items: Vec<CatalogItem>,
    fetch: &F,
) -> Result<Vec<CatalogItem>, &'static str>
where
    F: Fn(&str) -> Result<String, ProviderRequestError> + Sync,
{
    if items.is_empty() {
        return Ok(items);
    }

    let results = Mutex::new(vec![None; items.len()]);
    let stale = AtomicBool::new(false);
    let failed = AtomicBool::new(false);
    let conflicting = AtomicBool::new(false);

    thread::scope(|scope| {
        let mut workers = Vec::new();
        for (index, item) in items.iter().enumerate() {
            if stale.load(Ordering::Acquire)
                || failed.load(Ordering::Acquire)
                || conflicting.load(Ordering::Acquire)
            {
                break;
            }
            let permit = match reserve_adult_category_check(state, generation) {
                Ok(permit) => permit,
                Err(error) if error == CatalogCategory::Adult.stale_error() => {
                    stale.store(true, Ordering::Release);
                    break;
                }
                Err(_) => {
                    failed.store(true, Ordering::Release);
                    break;
                }
            };
            if failed.load(Ordering::Acquire) || conflicting.load(Ordering::Acquire) {
                break;
            }
            let results = &results;
            let failed = &failed;
            let conflicting = &conflicting;
            workers.push(scope.spawn(move || {
                let url = format!(
                    "{JAVDB_API_URL}/api/v4/movies/{}?from_rankings=false",
                    item.provider_item_id
                );
                let category_check = fetch(&url)
                    .ok()
                    .map(|document| {
                        adult_item_category(&document, &item.provider_item_id, &item.code)
                    })
                    .unwrap_or(AdultCategoryCheck::Inconclusive);
                if let Ok(mut results) = results.lock() {
                    results[index] = Some(category_check);
                } else {
                    failed.store(true, Ordering::Release);
                    return;
                }
                if category_check == AdultCategoryCheck::Conflicting {
                    conflicting.store(true, Ordering::Release);
                }
                drop(permit);
            }));
        }
        for worker in workers {
            if worker.join().is_err() {
                failed.store(true, Ordering::Release);
            }
        }
    });

    require_current_request(state, CatalogCategory::Adult, generation)?;
    if failed.load(Ordering::Acquire) {
        return Err(CatalogCategory::Adult.provider_error(ProviderRequestError::Provider));
    }
    let results = results
        .into_inner()
        .map_err(|_| CatalogCategory::Adult.provider_error(ProviderRequestError::Provider))?;
    if conflicting.load(Ordering::Acquire)
        || results.contains(&Some(AdultCategoryCheck::Conflicting))
    {
        return Err(CatalogCategory::Adult.conflicting_error());
    }
    if stale.load(Ordering::Acquire) || results.iter().any(Option::is_none) {
        return Err(CatalogCategory::Adult.stale_error());
    }

    Ok(items
        .into_iter()
        .zip(results)
        .filter_map(|(item, category)| {
            (category == Some(AdultCategoryCheck::Adult)).then_some(item)
        })
        .collect())
}

pub(crate) fn fetch_catalog_with<F>(
    state: &JavdbCatalogState,
    request: &JavdbCatalogRequest,
    fetch: F,
) -> Result<Vec<String>, &'static str>
where
    F: Fn(&str) -> Result<String, ProviderRequestError> + Sync,
{
    let category = CatalogCategory::parse(&request.category).ok_or(VR_PROVIDER_ERROR)?;
    if validated_request(request) != Some(category) {
        return Err(category.provider_error(ProviderRequestError::Provider));
    }
    let context_generation = request
        .context_generation
        .parse::<u64>()
        .map_err(|_| category.provider_error(ProviderRequestError::Provider))?;
    let generation = begin_request(state, category, context_generation)?;
    let mut items = Vec::new();
    let mut accepted_codes = HashMap::<String, String>::new();
    for url in listing_urls(request, category) {
        require_current_request(state, category, generation)?;
        let document = fetch(&url);
        require_current_request(state, category, generation)?;
        let document = document.map_err(|error| category.provider_error(error))?;
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
        items = verify_adult_items(state, generation, items, &fetch)?;
    }
    items.truncate(request.count as usize);
    finish_request(state, category, generation, items)
}

fn parsed_detail_request(
    request: &JavdbDetailRequest,
) -> Result<(CatalogCategory, u64, u64), &'static str> {
    let category = CatalogCategory::parse(&request.category).ok_or(VR_PROVIDER_ERROR)?;
    let context_generation = request
        .context_generation
        .parse::<u64>()
        .ok()
        .filter(|generation| *generation > 0)
        .ok_or_else(|| category.stale_error())?;
    let request_generation = request
        .request_generation
        .parse::<u64>()
        .ok()
        .filter(|generation| *generation > 0)
        .ok_or_else(|| category.stale_error())?;
    if !valid_provider_item_id(&request.provider_item_id)
        || product_code_display_form(&request.code).as_deref() != Some(&request.code)
    {
        return Err(category.stale_error());
    }
    Ok((category, context_generation, request_generation))
}

fn catalog_contains_detail_request(
    context: &CatalogContext,
    category: CatalogCategory,
    context_generation: u64,
    request_generation: u64,
    provider_item_id: &str,
    code: &str,
) -> bool {
    authority(context, category)
        .filter(|authority| {
            authority.context_generation == context_generation
                && authority.generation == request_generation
        })
        .is_some_and(|authority| {
            authority
                .items
                .iter()
                .any(|item| item.provider_item_id == provider_item_id && item.code == code)
        })
}

fn begin_detail(
    state: &JavdbCatalogState,
    request: &JavdbDetailRequest,
) -> Result<(CatalogCategory, u64), &'static str> {
    let (category, context_generation, request_generation) = parsed_detail_request(request)?;
    let mut context = state.0.lock().map_err(|_| category.stale_error())?;
    if !catalog_contains_detail_request(
        &context,
        category,
        context_generation,
        request_generation,
        &request.provider_item_id,
        &request.code,
    ) {
        return Err(category.stale_error());
    }
    context.detail_generation = context
        .detail_generation
        .checked_add(1)
        .ok_or_else(|| category.provider_error(ProviderRequestError::Provider))?;
    let generation = context.detail_generation;
    set_detail_authority(
        &mut context,
        category,
        Some(DetailAuthority {
            generation,
            catalog_context_generation: context_generation,
            catalog_request_generation: request_generation,
            provider_item_id: request.provider_item_id.clone(),
            code: request.code.clone(),
            title: None,
            original_title: None,
            release_date: None,
            duration: None,
            summary: None,
            actors: Vec::new(),
            tags: Vec::new(),
            cover: None,
            previews: Vec::new(),
        }),
    );
    Ok((category, generation))
}

fn detail_matches_request(
    detail: &DetailAuthority,
    request: &JavdbDetailRequest,
    context_generation: u64,
    request_generation: u64,
    detail_generation: u64,
) -> bool {
    detail.generation == detail_generation
        && detail.catalog_context_generation == context_generation
        && detail.catalog_request_generation == request_generation
        && detail.provider_item_id == request.provider_item_id
        && detail.code == request.code
}

fn detail_image_authority(
    generation: u64,
    position: usize,
    kind: &str,
    url: String,
) -> DetailImageAuthority {
    DetailImageAuthority {
        authority_id: format!(
            "javdb-{kind}-{generation}-{position}-{}",
            &md5_hex(url.as_bytes())[..8]
        ),
        url,
    }
}

fn encode_detail(category: CatalogCategory, detail: &DetailAuthority) -> Vec<String> {
    let mut response = vec![
        detail.generation.to_string(),
        category.value().to_owned(),
        detail.catalog_context_generation.to_string(),
        detail.catalog_request_generation.to_string(),
        detail.provider_item_id.clone(),
        detail.code.clone(),
        detail.title.clone().unwrap_or_default(),
        detail.original_title.clone().unwrap_or_default(),
        detail.release_date.clone().unwrap_or_default(),
        detail.duration.clone().unwrap_or_default(),
        detail.summary.clone().unwrap_or_default(),
        detail
            .cover
            .as_ref()
            .map(|cover| cover.authority_id.clone())
            .unwrap_or_default(),
        detail.actors.len().to_string(),
    ];
    response.extend(detail.actors.iter().cloned());
    response.push(detail.tags.len().to_string());
    response.extend(detail.tags.iter().cloned());
    response.push(detail.previews.len().to_string());
    response.extend(
        detail
            .previews
            .iter()
            .map(|preview| preview.authority_id.clone()),
    );
    response
}

fn finish_detail(
    state: &JavdbCatalogState,
    request: &JavdbDetailRequest,
    category: CatalogCategory,
    generation: u64,
    detail: ParsedDetail,
) -> Result<Vec<String>, &'static str> {
    let (_, context_generation, request_generation) = parsed_detail_request(request)?;
    let mut context = state.0.lock().map_err(|_| category.stale_error())?;
    if !catalog_contains_detail_request(
        &context,
        category,
        context_generation,
        request_generation,
        &request.provider_item_id,
        &request.code,
    ) || !detail_authority(&context, category).is_some_and(|detail| {
        detail_matches_request(
            detail,
            request,
            context_generation,
            request_generation,
            generation,
        )
    }) {
        return Err(category.stale_error());
    }
    let authority = DetailAuthority {
        generation,
        catalog_context_generation: context_generation,
        catalog_request_generation: request_generation,
        provider_item_id: request.provider_item_id.clone(),
        code: request.code.clone(),
        title: detail.title,
        original_title: detail.original_title,
        release_date: detail.release_date,
        duration: detail.duration,
        summary: detail.summary,
        actors: detail.actors,
        tags: detail.tags,
        cover: detail
            .cover_url
            .map(|url| detail_image_authority(generation, 1, "detail-cover", url)),
        previews: detail
            .preview_urls
            .into_iter()
            .enumerate()
            .map(|(index, url)| detail_image_authority(generation, index + 1, "preview", url))
            .collect(),
    };
    let response = encode_detail(category, &authority);
    set_detail_authority(&mut context, category, Some(authority));
    Ok(response)
}

fn clear_detail_generation(
    state: &JavdbCatalogState,
    category: CatalogCategory,
    generation: u64,
) -> Result<(), &'static str> {
    let mut context = state.0.lock().map_err(|_| category.stale_error())?;
    if detail_authority(&context, category).is_some_and(|detail| detail.generation == generation) {
        set_detail_authority(&mut context, category, None);
    }
    Ok(())
}

pub(crate) fn fetch_detail_with(
    state: &JavdbCatalogState,
    request: &JavdbDetailRequest,
    fetch: impl FnOnce(&str) -> Result<String, ProviderRequestError>,
) -> Result<Vec<String>, &'static str> {
    let (category, generation) = begin_detail(state, request)?;
    let result = (|| {
        let url = format!(
            "{JAVDB_API_URL}/api/v4/movies/{}?from_rankings=false",
            request.provider_item_id
        );
        let document = fetch(&url).map_err(|error| category.provider_error(error))?;
        let detail = parse_detail(
            &document,
            category,
            &request.provider_item_id,
            &request.code,
        )
        .map_err(|error| parse_error(category, error))?;
        finish_detail(state, request, category, generation, detail)
    })();
    if result.is_err() {
        clear_detail_generation(state, category, generation)?;
    }
    result
}

fn authorized_detail_image_url(
    state: &JavdbCatalogState,
    request: &JavdbDetailRequest,
    detail_generation: u64,
    image_authority_id: &str,
) -> Result<String, &'static str> {
    let (category, context_generation, request_generation) = parsed_detail_request(request)?;
    let context = state.0.lock().map_err(|_| category.stale_error())?;
    if !catalog_contains_detail_request(
        &context,
        category,
        context_generation,
        request_generation,
        &request.provider_item_id,
        &request.code,
    ) {
        return Err(category.stale_error());
    }
    let detail = detail_authority(&context, category)
        .filter(|detail| {
            detail_matches_request(
                detail,
                request,
                context_generation,
                request_generation,
                detail_generation,
            )
        })
        .ok_or_else(|| category.stale_error())?;
    detail
        .cover
        .iter()
        .chain(detail.previews.iter())
        .find(|image| image.authority_id == image_authority_id)
        .map(|image| image.url.clone())
        .ok_or_else(|| category.stale_error())
}

pub(crate) fn fetch_detail_image_with(
    state: &JavdbCatalogState,
    request: &JavdbDetailRequest,
    detail_generation: &str,
    image_authority_id: &str,
    fetch: impl FnOnce(&str) -> Result<Vec<u8>, ProviderRequestError>,
) -> Result<Vec<u8>, &'static str> {
    let (category, _, _) = parsed_detail_request(request)?;
    let detail_generation = detail_generation
        .parse::<u64>()
        .ok()
        .filter(|generation| *generation > 0)
        .ok_or_else(|| category.stale_error())?;
    if image_authority_id.is_empty() || image_authority_id.contains("://") {
        return Err(category.stale_error());
    }
    let url = authorized_detail_image_url(state, request, detail_generation, image_authority_id)?;
    let bytes = fetch(&url).map_err(|error| category.provider_error(error))?;
    if !accepted_raster(&bytes) {
        return Err(category.provider_error(ProviderRequestError::Provider));
    }
    authorized_detail_image_url(state, request, detail_generation, image_authority_id)?;
    Ok(bytes)
}

pub(crate) fn open_detail_source_with(
    state: &JavdbCatalogState,
    request: &JavdbDetailRequest,
    detail_generation: &str,
    open: impl FnOnce(&str) -> Result<(), ()>,
) -> Result<(), &'static str> {
    let (category, context_generation, request_generation) = parsed_detail_request(request)?;
    let detail_generation = detail_generation
        .parse::<u64>()
        .ok()
        .filter(|generation| *generation > 0)
        .ok_or_else(|| category.stale_error())?;
    let context = state.0.lock().map_err(|_| category.stale_error())?;
    if !catalog_contains_detail_request(
        &context,
        category,
        context_generation,
        request_generation,
        &request.provider_item_id,
        &request.code,
    ) || !detail_authority(&context, category).is_some_and(|detail| {
        detail_matches_request(
            detail,
            request,
            context_generation,
            request_generation,
            detail_generation,
        )
    }) {
        return Err(category.stale_error());
    }
    let url = format!("https://javdb.com/v/{}", request.provider_item_id);
    let result = open(&url).map_err(|()| category.provider_error(ProviderRequestError::Provider));
    drop(context);
    result
}

pub(crate) fn invalidate_detail(
    state: &JavdbCatalogState,
    category: &str,
    detail_generation: &str,
) -> Result<(), &'static str> {
    let category = CatalogCategory::parse(category).ok_or(VR_PROVIDER_ERROR)?;
    let detail_generation = detail_generation
        .parse::<u64>()
        .ok()
        .filter(|generation| *generation > 0)
        .ok_or_else(|| category.stale_error())?;
    clear_detail_generation(state, category, detail_generation)
}

pub(crate) fn invalidate_catalog(
    state: &JavdbCatalogState,
    category: &str,
    context_generation: &str,
) -> Result<(), &'static str> {
    let category = CatalogCategory::parse(category).ok_or(VR_PROVIDER_ERROR)?;
    let context_generation = context_generation
        .parse::<u64>()
        .ok()
        .filter(|generation| *generation > 0)
        .ok_or_else(|| category.provider_error(ProviderRequestError::Provider))?;
    let mut context = state
        .0
        .lock()
        .map_err(|_| category.provider_error(ProviderRequestError::Provider))?;
    let current_context_generation = match category {
        CatalogCategory::Adult => &mut context.adult_context_generation,
        CatalogCategory::Vr => &mut context.vr_context_generation,
    };
    if context_generation < *current_context_generation {
        return Ok(());
    }
    *current_context_generation = context_generation;
    match category {
        CatalogCategory::Adult => {
            if context
                .adult
                .as_ref()
                .is_some_and(|authority| authority.context_generation <= context_generation)
            {
                context.adult = None;
                context.adult_detail = None;
            }
        }
        CatalogCategory::Vr => {
            if context
                .vr
                .as_ref()
                .is_some_and(|authority| authority.context_generation <= context_generation)
            {
                context.vr = None;
                context.vr_detail = None;
            }
        }
    }
    drop(context);
    if category == CatalogCategory::Adult {
        state.1.notify_all();
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
    use std::sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        mpsc,
    };

    use super::*;

    static NEXT_CONTEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

    fn request(category: &str) -> JavdbCatalogRequest {
        JavdbCatalogRequest {
            category: category.to_owned(),
            context_generation: NEXT_CONTEXT_GENERATION
                .fetch_add(1, Ordering::Relaxed)
                .to_string(),
            mode: "category".to_owned(),
            period: "daily".to_owned(),
            year: None,
            month: None,
            sort: "newest".to_owned(),
            count: 25,
        }
    }

    fn wait_for_only_current_adult_waiter(state: &JavdbCatalogState) {
        let mut context = state.0.lock().expect("catalog state must remain available");
        loop {
            let current_generation = context
                .adult
                .as_ref()
                .expect("a current Adult request must exist")
                .generation;
            if context.adult_check_waiters.as_slice() == [current_generation] {
                return;
            }
            context = state
                .1
                .wait(context)
                .expect("catalog state must remain available");
        }
    }

    fn detail(item_id: &str, tags: &str) -> String {
        format!(r#"{{"success":1,"data":{{"movie":{{"id":"{item_id}","tags":{tags}}}}}}}"#)
    }

    fn detail_with_number(item_id: &str, number: &str, tags: &str) -> String {
        format!(
            r#"{{"success":1,"data":{{"movie":{{"id":"{item_id}","number":"{number}","tags":{tags}}}}}}}"#
        )
    }

    fn jpeg() -> Vec<u8> {
        vec![0xff, 0xd8, 0xff, 0xe0, 0, 16, 0, 0, 0, 0, 0, 0]
    }

    fn detail_request(
        category: &str,
        catalog: &[String],
        provider_item_id: &str,
        code: &str,
    ) -> JavdbDetailRequest {
        JavdbDetailRequest {
            category: category.to_owned(),
            context_generation: String::new(),
            request_generation: catalog[0].clone(),
            provider_item_id: provider_item_id.to_owned(),
            code: code.to_owned(),
        }
    }

    fn established_detail_request(
        state: &JavdbCatalogState,
        category: &str,
        provider_item_id: &str,
        code: &str,
    ) -> JavdbDetailRequest {
        let request = request(category);
        let context_generation = request.context_generation.clone();
        let catalog = fetch_catalog_with(state, &request, |url| {
            Ok(if category == "adult" && url.contains("/api/v4/") {
                detail_with_number(provider_item_id, code, r#"[{"id":"28"}]"#)
            } else {
                format!(
                    r#"{{"success":1,"data":{{"movies":[{{"id":"{provider_item_id}","number":"{code}"}}]}}}}"#
                )
            })
        })
        .expect("catalog authority must be established");
        let mut detail = detail_request(category, &catalog, provider_item_id, code);
        detail.context_generation = context_generation;
        detail
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
        let urls = Mutex::new(Vec::new());
        for period in ["daily", "weekly", "monthly"] {
            let ranking = JavdbCatalogRequest {
                category: "adult".to_owned(),
                context_generation: NEXT_CONTEXT_GENERATION
                    .fetch_add(1, Ordering::Relaxed)
                    .to_string(),
                mode: "ranking".to_owned(),
                period: period.to_owned(),
                year: None,
                month: None,
                sort: "newest".to_owned(),
                count: 25,
            };
            fetch_catalog_with(&state, &ranking, |url| {
                urls.lock()
                    .expect("request list must remain available")
                    .push(url.to_owned());
                Ok(if url.contains("rankings") {
                    r#"{"success":1,"data":{"movies":[]}}"#.to_owned()
                } else {
                    unreachable!()
                })
            })
            .expect("Adult ranking period must be accepted");
        }
        assert_eq!(
            urls.lock()
                .expect("request list must remain available")
                .as_slice(),
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
            let url = Mutex::new(String::new());
            fetch_catalog_with(&state, &adult, |value| {
                *url.lock().expect("request URL must remain available") = value.to_owned();
                Ok(r#"{"success":1,"data":{"movies":[]}}"#.to_owned())
            })
            .expect("Adult category request must be accepted");
            assert_eq!(
                url.into_inner().expect("request URL must remain available"),
                format!("https://apidd.spthgb.com/api/v1/movies/tags?filter_by=0%3At%3Am%3A%3A{current_year}%3A%3A6&filter_by_tags=&sort_by={provider_sort}&order_by={order}&page=1&limit=10")
            );
        }

        for invalid_year in [
            "2000".to_owned(),
            (current_calendar_year().expect("current year must be available") + 1).to_string(),
        ] {
            let mut invalid = request("adult");
            invalid.year = Some(invalid_year);
            let dispatched = AtomicBool::new(false);
            assert!(fetch_catalog_with(&state, &invalid, |_| {
                dispatched.store(true, Ordering::Relaxed);
                unreachable!()
            })
            .is_err());
            assert!(!dispatched.load(Ordering::Relaxed));
        }

        let mut vr = request("vr");
        vr.count = 100;
        let urls = Mutex::new(Vec::new());
        fetch_catalog_with(&state, &vr, |url| {
            urls.lock()
                .expect("request list must remain available")
                .push(url.to_owned());
            Ok(r#"{"success":1,"data":{"movies":[]}}"#.to_owned())
        })
        .expect("VR request must be accepted");
        let urls = urls.lock().expect("request list must remain available");
        assert_eq!(urls.len(), 1);
        assert!(urls[0].contains("filter_by=0%3At%3Am%3A212%3A%3A%3A"));
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
        assert_eq!(response[4], "MDVR-00419");
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
    fn exact_library_rejects_conflicting_rows_for_one_provider_item() {
        for (first_fields, second_fields) in [
            (r#""title":"First title""#, r#""title":"Another title""#),
            (
                r#""release_date":"2024-01-01""#,
                r#""release_date":"2024-02-01""#,
            ),
            (
                r#""cover_url":"https://tp.cmastd.com/first.jpg""#,
                r#""cover_url":"https://tp.cmastd.com/second.jpg""#,
            ),
        ] {
            let rows = format!(
                r#"{{"id":"Same","number":"MDVR-419",{first_fields}}},{{"id":"Same","number":"MDVR-419",{second_fields}}}"#
            );
            let calls = Cell::new(0);
            assert_eq!(
                fetch_exact_library_item_with("vr", "MDVR-419", &mut |_| {
                    calls.set(calls.get() + 1);
                    Ok(format!(r#"{{"success":1,"data":{{"movies":[{rows}]}}}}"#))
                }),
                Err(ProviderRequestError::Provider)
            );
            assert_eq!(calls.get(), 1);
        }

        assert_eq!(
            fetch_exact_library_item_with("vr", "MDVR-419", &mut |_| {
                Ok(r#"{"success":1,"data":{"movies":[{"id":"Same","number":"MDVR-419","title":"Exact"},{"id":"Same","number":"MDVR-420","title":"Exact"}]}}"#.to_owned())
            }),
            Err(ProviderRequestError::Provider)
        );
    }

    #[test]
    fn exact_library_distinguishes_absent_cover_from_malformed_cover_data() {
        for malformed_cover in [
            r#""cover_url":42"#,
            r#""cover_url":"https://tp.cmastd.com.evil.example/cover.jpg""#,
            r#""cover_url":" https://tp.cmastd.com/cover.jpg ""#,
        ] {
            let calls = Cell::new(0);
            assert_eq!(
                fetch_exact_library_item_with("adult", "CAWB-1", &mut |url| {
                    calls.set(calls.get() + 1);
                    assert!(url.contains("search"));
                    Ok(format!(
                        r#"{{"success":1,"data":{{"movies":[{{"id":"Exact","number":"CAWB-1",{malformed_cover}}}]}}}}"#
                    ))
                }),
                Err(ProviderRequestError::Provider)
            );
            assert_eq!(calls.get(), 1);
        }

        let calls = Cell::new(0);
        let accepted = fetch_exact_library_item_with("adult", "CAWB-1", &mut |url| {
            calls.set(calls.get() + 1);
            Ok(if url.contains("search") {
                r#"{"success":1,"data":{"movies":[{"id":"Exact","number":"CAWB-1","cover_url":null}]}}"#.to_owned()
            } else {
                r#"{"success":1,"data":{"movie":{"id":"Exact","number":"CAWB-1","tags":[],"cover_url":null}}}"#.to_owned()
            })
        })
        .expect("an exact null cover must remain a valid provider response")
        .expect("the exact item must remain accepted");
        assert!(accepted.cover_url.is_none());
        assert_eq!(calls.get(), 2);

        assert_eq!(
            fetch_exact_library_item_with("adult", "CAWB-1", &mut |url| {
                Ok(if url.contains("search") {
                    r#"{"success":1,"data":{"movies":[{"id":"Exact","number":"CAWB-1","cover_url":null}]}}"#.to_owned()
                } else {
                    r#"{"success":1,"data":{"movie":{"id":"Exact","number":"CAWB-1","tags":[],"cover_url":[]}}}"#.to_owned()
                })
            }),
            Err(ProviderRequestError::Provider)
        );
    }

    #[test]
    fn adult_category_checks_overlap_without_exceeding_the_fixed_bound() {
        let state = JavdbCatalogState::default();
        let request = request("adult");
        let (started_sender, started_receiver) = mpsc::channel();
        let release = (Mutex::new(false), Condvar::new());
        let active = AtomicUsize::new(0);
        let maximum_active = AtomicUsize::new(0);
        let listing = (1..=8)
            .map(|index| format!(r#"{{"id":"Adult{index}","number":"ADLT-{index}"}}"#))
            .collect::<Vec<_>>()
            .join(",");

        thread::scope(|scope| {
            let request = &request;
            let result = scope.spawn(|| {
                fetch_catalog_with(&state, request, |url| {
                    if url.contains("movies/tags") {
                        return Ok(format!(
                            r#"{{"success":1,"data":{{"movies":[{listing}]}}}}"#
                        ));
                    }
                    let item_id = url
                        .split("/api/v4/movies/")
                        .nth(1)
                        .and_then(|value| value.split('?').next())
                        .expect("detail URL must contain the provider item");
                    let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                    maximum_active.fetch_max(current, Ordering::SeqCst);
                    started_sender
                        .send(item_id.to_owned())
                        .expect("the test must observe each started check");
                    let (released, available) = &release;
                    let mut released = released.lock().expect("release gate must remain available");
                    while !*released {
                        released = available
                            .wait(released)
                            .expect("release gate must remain available");
                    }
                    active.fetch_sub(1, Ordering::SeqCst);
                    Ok(detail_with_number(
                        item_id,
                        &format!("ADLT-{}", &item_id[5..]),
                        r#"[{"id":"28"}]"#,
                    ))
                })
            });

            let started = (0..ADULT_CATEGORY_CHECK_CONCURRENCY)
                .map(|_| {
                    started_receiver
                        .recv()
                        .expect("the configured checks must start")
                })
                .collect::<Vec<_>>();
            assert_eq!(started.len(), ADULT_CATEGORY_CHECK_CONCURRENCY);
            assert_eq!(
                active.load(Ordering::SeqCst),
                ADULT_CATEGORY_CHECK_CONCURRENCY
            );
            assert!(started_receiver.try_recv().is_err());
            {
                let (released, available) = &release;
                *released.lock().expect("release gate must remain available") = true;
                available.notify_all();
            }
            let response = result
                .join()
                .expect("catalog worker must not panic")
                .expect("every exact Adult row must be accepted");
            assert_eq!(response[1], "8");
        });

        assert_eq!(active.load(Ordering::SeqCst), 0);
        assert_eq!(
            maximum_active.load(Ordering::SeqCst),
            ADULT_CATEGORY_CHECK_CONCURRENCY
        );
    }

    #[test]
    fn overlapping_adult_requests_cancel_stale_waiters_before_latest_work_starts() {
        let state = JavdbCatalogState::default();
        let first_request = request("adult");
        let second_request = request("adult");
        let latest_request = request("adult");
        let (started_sender, started_receiver) = mpsc::channel::<String>();
        let (listed_sender, listed_receiver) = mpsc::channel::<&'static str>();
        let release = (Mutex::new((false, Vec::<String>::new())), Condvar::new());
        let active = AtomicUsize::new(0);
        let maximum_active = AtomicUsize::new(0);
        let first_listing = (1..=8)
            .map(|index| format!(r#"{{"id":"First{index}","number":"ADLT-{index}"}}"#))
            .collect::<Vec<_>>()
            .join(",");
        let second_listing = (1..=8)
            .map(|index| format!(r#"{{"id":"Second{index}","number":"ADLT-1{index}"}}"#))
            .collect::<Vec<_>>()
            .join(",");
        let latest_listing =
            r#"{"id":"Latest1","number":"ADLT-21"},{"id":"Latest2","number":"ADLT-22"}"#;
        let fetch_detail = |url: &str| {
            let item_id = url
                .split("/api/v4/movies/")
                .nth(1)
                .and_then(|value| value.split('?').next())
                .expect("detail URL must contain the provider item");
            let current = active.fetch_add(1, Ordering::SeqCst) + 1;
            maximum_active.fetch_max(current, Ordering::SeqCst);
            started_sender
                .send(item_id.to_owned())
                .expect("the test must observe every started check");
            let (release_state, available) = &release;
            let mut release_state = release_state
                .lock()
                .expect("release state must remain available");
            while !release_state.0 && !release_state.1.iter().any(|id| id == item_id) {
                release_state = available
                    .wait(release_state)
                    .expect("release state must remain available");
            }
            active.fetch_sub(1, Ordering::SeqCst);
            let number = if let Some(index) = item_id.strip_prefix("First") {
                format!("ADLT-{index}")
            } else if let Some(index) = item_id.strip_prefix("Second") {
                format!("ADLT-1{index}")
            } else if let Some(index) = item_id.strip_prefix("Latest") {
                format!("ADLT-2{index}")
            } else {
                unreachable!()
            };
            Ok(detail_with_number(item_id, &number, r#"[{"id":"28"}]"#))
        };

        thread::scope(|scope| {
            let first_result = scope.spawn(|| {
                fetch_catalog_with(&state, &first_request, |url| {
                    if url.contains("movies/tags") {
                        Ok(format!(
                            r#"{{"success":1,"data":{{"movies":[{first_listing}]}}}}"#
                        ))
                    } else {
                        fetch_detail(url)
                    }
                })
            });

            let first_started = (0..ADULT_CATEGORY_CHECK_CONCURRENCY)
                .map(|_| {
                    started_receiver
                        .recv()
                        .expect("the first request must fill every global worker slot")
                })
                .collect::<Vec<_>>();
            assert!(first_started.iter().all(|item| item.starts_with("First")));
            assert_eq!(
                state
                    .0
                    .lock()
                    .expect("catalog state must remain available")
                    .adult_checks_in_progress,
                ADULT_CATEGORY_CHECK_CONCURRENCY
            );

            let second_result = scope.spawn(|| {
                fetch_catalog_with(&state, &second_request, |url| {
                    if url.contains("movies/tags") {
                        listed_sender
                            .send("second")
                            .expect("the second listing must be observed");
                        Ok(format!(
                            r#"{{"success":1,"data":{{"movies":[{second_listing}]}}}}"#
                        ))
                    } else {
                        fetch_detail(url)
                    }
                })
            });
            assert_eq!(
                listed_receiver
                    .recv()
                    .expect("the second request must reach verification"),
                "second"
            );
            wait_for_only_current_adult_waiter(&state);

            let latest_result = scope.spawn(|| {
                fetch_catalog_with(&state, &latest_request, |url| {
                    if url.contains("movies/tags") {
                        listed_sender
                            .send("latest")
                            .expect("the latest listing must be observed");
                        Ok(format!(
                            r#"{{"success":1,"data":{{"movies":[{latest_listing}]}}}}"#
                        ))
                    } else {
                        fetch_detail(url)
                    }
                })
            });
            assert_eq!(
                listed_receiver
                    .recv()
                    .expect("the latest request must reach verification"),
                "latest"
            );
            wait_for_only_current_adult_waiter(&state);

            {
                let (release_state, available) = &release;
                release_state
                    .lock()
                    .expect("release state must remain available")
                    .1
                    .push(first_started[0].clone());
                available.notify_all();
            }
            let first_replacement = started_receiver
                .recv()
                .expect("the latest request must receive the released worker slot");
            assert!(first_replacement.starts_with("Latest"));
            assert_eq!(
                state
                    .0
                    .lock()
                    .expect("catalog state must remain available")
                    .adult_checks_in_progress,
                ADULT_CATEGORY_CHECK_CONCURRENCY
            );
            wait_for_only_current_adult_waiter(&state);

            {
                let (release_state, available) = &release;
                release_state
                    .lock()
                    .expect("release state must remain available")
                    .0 = true;
                available.notify_all();
            }

            let latest = latest_result
                .join()
                .expect("latest catalog worker must not panic")
                .expect("the latest request must complete");
            assert_eq!(latest[1], "2");
            assert_eq!(latest[4], "ADLT-21");
            assert_eq!(latest[11], "ADLT-22");
            assert_eq!(
                second_result
                    .join()
                    .expect("second catalog worker must not panic"),
                Err(ADULT_JAVDB_STALE)
            );
            assert_eq!(
                first_result
                    .join()
                    .expect("first catalog worker must not panic"),
                Err(ADULT_JAVDB_STALE)
            );

            let mut all_started = first_started;
            all_started.push(first_replacement);
            all_started.extend(started_receiver.try_iter());
            assert_eq!(
                all_started
                    .iter()
                    .filter(|item| item.starts_with("First"))
                    .count(),
                ADULT_CATEGORY_CHECK_CONCURRENCY
            );
            assert!(!all_started.iter().any(|item| item.starts_with("Second")));
            assert_eq!(
                all_started
                    .iter()
                    .filter(|item| item.starts_with("Latest"))
                    .count(),
                2
            );
        });

        assert_eq!(maximum_active.load(Ordering::SeqCst), 4);
        assert_eq!(active.load(Ordering::SeqCst), 0);
        assert_eq!(
            state
                .0
                .lock()
                .expect("catalog state must remain available")
                .adult_checks_in_progress,
            0
        );
        assert!(state
            .0
            .lock()
            .expect("catalog state must remain available")
            .adult_check_waiters
            .is_empty());
    }

    #[test]
    fn adult_category_completion_order_cannot_change_provider_source_order() {
        let state = JavdbCatalogState::default();
        let request = request("adult");
        let (started_sender, started_receiver) = mpsc::channel();
        let (completed_sender, completed_receiver) = mpsc::channel();
        let released = (Mutex::new(Vec::<String>::new()), Condvar::new());

        thread::scope(|scope| {
            let result = scope.spawn(|| {
                fetch_catalog_with(&state, &request, |url| {
                    if url.contains("movies/tags") {
                        return Ok(r#"{"success":1,"data":{"movies":[{"id":"First","number":"ADLT-1"},{"id":"Second","number":"ADLT-2"},{"id":"Third","number":"ADLT-3"},{"id":"Fourth","number":"ADLT-4"}]}}"#.to_owned());
                    }
                    let item_id = url
                        .split("/api/v4/movies/")
                        .nth(1)
                        .and_then(|value| value.split('?').next())
                        .expect("detail URL must contain the provider item");
                    started_sender
                        .send(item_id.to_owned())
                        .expect("the test must observe each started check");
                    let (released_items, available) = &released;
                    let mut released_items = released_items
                        .lock()
                        .expect("completion gate must remain available");
                    while !released_items.iter().any(|released| released == item_id) {
                        released_items = available
                            .wait(released_items)
                            .expect("completion gate must remain available");
                    }
                    completed_sender
                        .send(item_id.to_owned())
                        .expect("the test must observe completion order");
                    let number = match item_id {
                        "First" => "ADLT-1",
                        "Second" => "ADLT-2",
                        "Third" => "ADLT-3",
                        "Fourth" => "ADLT-4",
                        _ => unreachable!(),
                    };
                    Ok(detail_with_number(item_id, number, r#"[{"id":"28"}]"#))
                })
            });

            let mut started = (0..4)
                .map(|_| started_receiver.recv().expect("all four checks must start"))
                .collect::<Vec<_>>();
            started.sort();
            assert_eq!(started, ["First", "Fourth", "Second", "Third"]);
            for item_id in ["Fourth", "Third", "Second", "First"] {
                let (released_items, available) = &released;
                released_items
                    .lock()
                    .expect("completion gate must remain available")
                    .push(item_id.to_owned());
                available.notify_all();
                assert_eq!(
                    completed_receiver
                        .recv()
                        .expect("the released check must finish"),
                    item_id
                );
            }

            let response = result
                .join()
                .expect("catalog worker must not panic")
                .expect("every exact Adult row must be accepted");
            assert_eq!(response[1], "4");
            assert_eq!(response[4], "ADLT-1");
            assert_eq!(response[11], "ADLT-2");
            assert_eq!(response[18], "ADLT-3");
            assert_eq!(response[25], "ADLT-4");
        });
    }

    #[test]
    fn newer_adult_request_stops_obsolete_detail_scheduling_and_rejects_late_results() {
        let state = JavdbCatalogState::default();
        let old_request = request("adult");
        let (started_sender, started_receiver) = mpsc::channel();
        let release = (Mutex::new(false), Condvar::new());
        let detail_starts = AtomicUsize::new(0);
        let listing = (1..=8)
            .map(|index| format!(r#"{{"id":"Old{index}","number":"ADLT-{index}"}}"#))
            .collect::<Vec<_>>()
            .join(",");

        thread::scope(|scope| {
            let old_result = scope.spawn(|| {
                fetch_catalog_with(&state, &old_request, |url| {
                    if url.contains("movies/tags") {
                        return Ok(format!(
                            r#"{{"success":1,"data":{{"movies":[{listing}]}}}}"#
                        ));
                    }
                    detail_starts.fetch_add(1, Ordering::SeqCst);
                    started_sender
                        .send(())
                        .expect("the test must observe each started check");
                    let (released, available) = &release;
                    let mut released = released.lock().expect("release gate must remain available");
                    while !*released {
                        released = available
                            .wait(released)
                            .expect("release gate must remain available");
                    }
                    let item_id = url
                        .split("/api/v4/movies/")
                        .nth(1)
                        .and_then(|value| value.split('?').next())
                        .expect("detail URL must contain the provider item");
                    Ok(detail(item_id, r#"[{"id":"28"}]"#))
                })
            });

            for _ in 0..ADULT_CATEGORY_CHECK_CONCURRENCY {
                started_receiver
                    .recv()
                    .expect("the initial bounded checks must start");
            }
            let current = fetch_catalog_with(&state, &request("adult"), |_| {
                Ok(r#"{"success":1,"data":{"movies":[]}}"#.to_owned())
            })
            .expect("the newer request must become current");
            assert_eq!(current[1], "0");
            {
                let (released, available) = &release;
                *released.lock().expect("release gate must remain available") = true;
                available.notify_all();
            }
            assert_eq!(
                old_result.join().expect("catalog worker must not panic"),
                Err(ADULT_JAVDB_STALE)
            );
            assert_eq!(
                detail_starts.load(Ordering::SeqCst),
                ADULT_CATEGORY_CHECK_CONCURRENCY
            );
            let context = state.0.lock().expect("catalog state must remain available");
            assert!(context.adult.as_ref().is_some_and(|authority| {
                authority.generation.to_string() == current[0] && authority.items.is_empty()
            }));
        });
    }

    #[test]
    fn obsolete_two_page_request_stops_before_the_next_listing_dispatch() {
        let state = JavdbCatalogState::default();
        let mut obsolete = request("vr");
        obsolete.count = 100;
        let newer_context = NEXT_CONTEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
        let calls = AtomicUsize::new(0);

        let result = fetch_catalog_with(&state, &obsolete, |_| {
            calls.fetch_add(1, Ordering::SeqCst);
            invalidate_catalog(&state, "vr", &newer_context.to_string())
                .expect("newer context must invalidate the request");
            Ok(r#"{"success":1,"data":{"movies":[{"id":"Late","number":"MDVR-419"}]}}"#.to_owned())
        });

        assert_eq!(result, Err(VR_JAVDB_STALE));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn vr_browse_never_dispatches_adult_per_item_verification() {
        let state = JavdbCatalogState::default();
        let calls = AtomicUsize::new(0);
        let response = fetch_catalog_with(&state, &request("vr"), |url| {
            calls.fetch_add(1, Ordering::SeqCst);
            assert!(!url.contains("/api/v4/"));
            Ok(r#"{"success":1,"data":{"movies":[{"id":"VrA","number":"MDVR-419"}]}}"#.to_owned())
        })
        .expect("VR category listing must not need Adult verification");

        assert_eq!(response[1], "1");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn drops_only_adult_rows_with_vr_failed_or_inconclusive_category_checks() {
        let state = JavdbCatalogState::default();
        let calls = Mutex::new(Vec::new());
        let response = fetch_catalog_with(&state, &request("adult"), |url| {
            calls.lock().expect("request list must remain available").push(url.to_owned());
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
        assert_eq!(
            calls
                .lock()
                .expect("request list must remain available")
                .len(),
            6
        );
    }

    #[test]
    fn rejects_the_complete_adult_request_when_detail_reuses_an_item_for_another_code() {
        let state = JavdbCatalogState::default();
        let dispatched_urls = Mutex::new(Vec::new());
        let result = fetch_catalog_with(&state, &request("adult"), |url| {
            dispatched_urls
                .lock()
                .expect("request list must remain available")
                .push(url.to_owned());
            if url.contains("movies/tags") {
                Ok(r#"{"success":1,"data":{"movies":[{"id":"AdultA","number":"ADLT-123"},{"id":"AdultB","number":"ADLT-124"}]}}"#.to_owned())
            } else if url.contains("AdultA") {
                Ok(detail_with_number("AdultA", "ADLT-999", r#"[{"id":"28"}]"#))
            } else {
                Ok(detail_with_number("AdultB", "ADLT-124", r#"[{"id":"28"}]"#))
            }
        });

        assert_eq!(result, Err(ADULT_JAVDB_CONFLICTING));
        let dispatched_urls = dispatched_urls
            .lock()
            .expect("request list must remain available");
        assert!(dispatched_urls
            .iter()
            .any(|url| url.contains("movies/tags")));
        assert!(dispatched_urls.iter().any(|url| url.contains("AdultA")));
        let context = state.0.lock().expect("catalog state must remain available");
        assert!(context
            .adult
            .as_ref()
            .is_some_and(|authority| authority.items.is_empty()));
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
        invalidate_catalog(
            &state,
            "vr",
            &NEXT_CONTEXT_GENERATION
                .fetch_add(1, Ordering::Relaxed)
                .to_string(),
        )
        .expect("invalidation must succeed");
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
        let first_context_generation = NEXT_CONTEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
        let first_generation = begin_request(&state, CatalogCategory::Vr, first_context_generation)
            .expect("first request must begin");
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
                invalidate_catalog(
                    &state,
                    "vr",
                    &NEXT_CONTEXT_GENERATION
                        .fetch_add(1, Ordering::Relaxed)
                        .to_string(),
                )
                .expect("cover must invalidate");
                Ok(jpeg())
            });
        assert!(cover_started.get());
        assert_eq!(cover_result, Err(VR_JAVDB_STALE));
    }

    #[test]
    fn an_older_invalidation_cannot_clear_a_newer_catalog_or_cover_authority() {
        let state = JavdbCatalogState::default();
        let older_context = NEXT_CONTEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
        let current_request = request("vr");
        let current_context = current_request
            .context_generation
            .parse::<u64>()
            .expect("test context must be valid");
        assert!(current_context > older_context);
        let current = fetch_catalog_with(&state, &current_request, |_| {
            Ok(r#"{"success":1,"data":{"movies":[{"id":"Current","number":"MDVR-419","cover_url":"https://tp.cmastd.com/current.jpg"}]}}"#.to_owned())
        })
        .expect("newer catalog must finish");

        invalidate_catalog(&state, "vr", &older_context.to_string())
            .expect("older invalidation must complete as a no-op");
        let dispatched = Cell::new(false);
        assert_eq!(
            fetch_cover_with(&state, "vr", &current[0], "Current", &current[7], |_| {
                dispatched.set(true);
                Ok(jpeg())
            }),
            Ok(jpeg())
        );
        assert!(dispatched.get());
    }

    #[test]
    fn exact_adult_and_vr_details_preserve_optional_fields_and_bound_preview_authorities() {
        for (category, item_id, code, category_tag) in [
            ("adult", "AdultA", "ADLT-123", "28"),
            ("vr", "VrA", "MDVR-419", "212"),
        ] {
            let state = JavdbCatalogState::default();
            let request = established_detail_request(&state, category, item_id, code);
            let previews = (0..30)
                .map(|index| {
                    if index == 1 {
                        r#"{"large_url":"https://tp.cmastd.com/0.jpg"}"#.to_owned()
                    } else if index == 2 {
                        r#"{"large_url":"https://tp.evil.example/forged.jpg"}"#.to_owned()
                    } else {
                        format!(r#"{{"large_url":"https://tp.rotating-{index}.com/{index}.jpg"}}"#)
                    }
                })
                .collect::<Vec<_>>()
                .join(",");
            let document = format!(
                r#"{{"success":1,"data":{{"movie":{{"id":"{item_id}","number":"{code}","title":" Provider title ","origin_title":"Original","release_date":"2026-08-12","duration":123,"summary":"Summary","cover_url":"https://tp.spfcas.com/cover.jpg","actors":[{{"name":"Actor A"}},null,{{"name":" "}},{{"name":"Actor B"}}],"tags":[{{"id":"{category_tag}","name":"Category"}},{{"name":"Tag"}},false],"preview_images":[{previews}]}}}}}}"#
            );
            let dispatched_url = RefCell::new(String::new());
            let response = fetch_detail_with(&state, &request, |url| {
                dispatched_url.replace(url.to_owned());
                Ok(document)
            })
            .expect("exact detail must be accepted");
            assert_eq!(
                dispatched_url.into_inner(),
                format!("{JAVDB_API_URL}/api/v4/movies/{item_id}?from_rankings=false")
            );
            assert_eq!(
                &response[1..12],
                [
                    category,
                    &request.context_generation,
                    &request.request_generation,
                    item_id,
                    code,
                    "Provider title",
                    "Original",
                    "2026-08-12",
                    "123",
                    "Summary",
                    response[11].as_str()
                ]
            );
            assert!(response[11].starts_with("javdb-detail-cover-"));
            assert_eq!(response[12], "2");
            assert_eq!(&response[13..15], ["Actor A", "Actor B"]);
            assert_eq!(response[15], "1");
            assert_eq!(response[16], "Category");
            assert_eq!(response[17], "24");
            assert_eq!(response.len(), 42);
            assert!(response[18].starts_with("javdb-preview-"));

            let image_dispatched = Cell::new(false);
            assert_eq!(
                fetch_detail_image_with(&state, &request, &response[0], &response[18], |url| {
                    image_dispatched.set(true);
                    assert_eq!(url, "https://tp.rotating-0.com/0.jpg");
                    Ok(jpeg())
                },),
                Ok(jpeg())
            );
            assert!(image_dispatched.get());

            let opened = RefCell::new(String::new());
            open_detail_source_with(&state, &request, &response[0], |url| {
                opened.replace(url.to_owned());
                Ok(())
            })
            .expect("the exact provider source must open");
            assert_eq!(
                opened.into_inner(),
                format!("https://javdb.com/v/{item_id}")
            );
        }
    }

    #[test]
    fn exact_details_require_present_category_tags_and_keep_tag_names_optional() {
        for (category, item_id, code, malformed_error) in [
            ("adult", "AdultA", "ADLT-123", ADULT_JAVDB_MALFORMED),
            ("vr", "VrA", "MDVR-419", VR_JAVDB_MALFORMED),
        ] {
            for tags in [
                None,
                Some("null"),
                Some(r#"[null,{"name":"Presentation only"},{"id":null},{"id":" "}]"#),
            ] {
                let state = JavdbCatalogState::default();
                let request = established_detail_request(&state, category, item_id, code);
                let tags = tags
                    .map(|tags| format!(r#","tags":{tags}"#))
                    .unwrap_or_default();
                let document = format!(
                    r#"{{"success":1,"data":{{"movie":{{"id":"{item_id}","number":"{code}"{tags}}}}}}}"#
                );
                assert_eq!(
                    fetch_detail_with(&state, &request, |_| Ok(document)),
                    Err(malformed_error)
                );
            }
        }

        let vr_state = JavdbCatalogState::default();
        let vr_request = established_detail_request(&vr_state, "vr", "VrA", "MDVR-419");
        let empty_presentation = fetch_detail_with(&vr_state, &vr_request, |_| {
            Ok(r#"{"success":1,"data":{"movie":{"id":"VrA","number":"MDVR-419","title":null,"origin_title":null,"release_date":null,"duration":null,"summary":null,"actors":null,"tags":[{"id":"212"}],"cover_url":null,"preview_images":null}}}"#.to_owned())
        })
        .expect("optional presentation fields may remain absent or null");
        assert_eq!(&empty_presentation[6..13], ["", "", "", "", "", "", "0"]);
        assert_eq!(&empty_presentation[13..], ["0", "0"]);

        let adult_state = JavdbCatalogState::default();
        let adult_request = established_detail_request(&adult_state, "adult", "AdultA", "ADLT-123");
        let empty_adult = fetch_detail_with(&adult_state, &adult_request, |_| {
            Ok(
                r#"{"success":1,"data":{"movie":{"id":"AdultA","number":"ADLT-123","tags":[]}}}"#
                    .to_owned(),
            )
        })
        .expect("an exact empty Adult tag collection proves that tag 212 is absent");
        assert_eq!(&empty_adult[12..], ["0", "0", "0"]);

        for (category, item_id, code, tags, expected_names) in [
            (
                "adult",
                "AdultA",
                "ADLT-123",
                r#"[null,{"id":"28","name":"Adult"},{"name":"Optional"}]"#,
                vec!["Adult"],
            ),
            (
                "vr",
                "VrA",
                "MDVR-419",
                r#"[false,{"id":"212"},{"name":"Optional"}]"#,
                vec![],
            ),
        ] {
            let state = JavdbCatalogState::default();
            let request = established_detail_request(&state, category, item_id, code);
            let document = format!(
                r#"{{"success":1,"data":{{"movie":{{"id":"{item_id}","number":"{code}","tags":{tags}}}}}}}"#
            );
            let response = fetch_detail_with(&state, &request, |_| Ok(document))
                .expect("malformed presentation tags must not hide valid category proof");
            let tag_count = response[13].parse::<usize>().expect("tag count must parse");
            assert_eq!(&response[14..14 + tag_count], expected_names);
        }
    }

    #[test]
    fn empty_and_opposite_tag_sets_cannot_establish_the_requested_category() {
        for (category, item_id, code, tags, expected_error) in [
            ("vr", "VrA", "MDVR-419", "[]", VR_JAVDB_MALFORMED),
            (
                "vr",
                "VrA",
                "MDVR-419",
                r#"[{"id":"28"}]"#,
                VR_JAVDB_CONFLICTING,
            ),
            (
                "adult",
                "AdultA",
                "ADLT-123",
                r#"[{"id":"212"}]"#,
                ADULT_JAVDB_CONFLICTING,
            ),
        ] {
            let state = JavdbCatalogState::default();
            let request = established_detail_request(&state, category, item_id, code);
            let document = format!(
                r#"{{"success":1,"data":{{"movie":{{"id":"{item_id}","number":"{code}","tags":{tags}}}}}}}"#
            );
            assert_eq!(
                fetch_detail_with(&state, &request, |_| Ok(document)),
                Err(expected_error)
            );
        }

        let state = JavdbCatalogState::default();
        let request = established_detail_request(&state, "vr", "VrA", "MDVR-419");
        for document in [
            r#"{"success":1,"data":{"movie":{"id":"VrB","number":"MDVR-419","tags":[{"id":"212"}]}}}"#,
            r#"{"success":1,"data":{"movie":{"id":"VrA","number":"ADLT-123","tags":[{"id":"212"}]}}}"#,
        ] {
            assert_eq!(
                fetch_detail_with(&state, &request, |_| Ok(document.to_owned())),
                Err(VR_JAVDB_CONFLICTING)
            );
        }
    }

    #[test]
    fn failed_detail_attempts_clear_only_their_exact_provisional_authority() {
        #[derive(Clone, Copy)]
        enum Failure {
            Network,
            Provider,
            Malformed,
            Conflicting,
        }

        for (failure, expected_error) in [
            (Failure::Network, VR_NETWORK_ERROR),
            (Failure::Provider, VR_PROVIDER_ERROR),
            (Failure::Malformed, VR_JAVDB_MALFORMED),
            (Failure::Conflicting, VR_JAVDB_CONFLICTING),
        ] {
            let state = JavdbCatalogState::default();
            let request = established_detail_request(&state, "vr", "VrA", "MDVR-419");
            let provisional_generation = Cell::new(0);
            let result = fetch_detail_with(&state, &request, |_| {
                let generation = state
                    .0
                    .lock()
                    .expect("detail state must lock")
                    .vr_detail
                    .as_ref()
                    .expect("the provisional authority must exist")
                    .generation;
                provisional_generation.set(generation);
                match failure {
                    Failure::Network => Err(ProviderRequestError::Network),
                    Failure::Provider => Ok(r#"{"success":0,"data":{}}"#.to_owned()),
                    Failure::Malformed => Ok("{".to_owned()),
                    Failure::Conflicting => Ok(r#"{"success":1,"data":{"movie":{"id":"VrB","number":"MDVR-419","tags":[{"id":"212"}]}}}"#.to_owned()),
                }
            });
            assert_eq!(result, Err(expected_error));
            assert!(state
                .0
                .lock()
                .expect("detail state must lock")
                .vr_detail
                .is_none());

            let source_dispatched = Cell::new(false);
            assert!(open_detail_source_with(
                &state,
                &request,
                &provisional_generation.get().to_string(),
                |_| {
                    source_dispatched.set(true);
                    Ok(())
                },
            )
            .is_err());
            assert!(!source_dispatched.get());
            let image_dispatched = Cell::new(false);
            assert!(fetch_detail_image_with(
                &state,
                &request,
                &provisional_generation.get().to_string(),
                "javdb-preview-stale",
                |_| {
                    image_dispatched.set(true);
                    Ok(jpeg())
                },
            )
            .is_err());
            assert!(!image_dispatched.get());

            fetch_detail_with(&state, &request, |_| {
                Ok(r#"{"success":1,"data":{"movie":{"id":"VrA","number":"MDVR-419","tags":[{"id":"212"}]}}}"#.to_owned())
            })
            .expect("the same exact current item must remain retryable");
        }
    }

    #[test]
    fn closing_during_failed_detail_attempts_keeps_stale_authority_revoked() {
        for (document, expected_error) in [
            (None, VR_NETWORK_ERROR),
            (Some("{"), VR_JAVDB_MALFORMED),
            (
                Some(
                    r#"{"success":1,"data":{"movie":{"id":"VrB","number":"MDVR-419","tags":[{"id":"212"}]}}}"#,
                ),
                VR_JAVDB_CONFLICTING,
            ),
        ] {
            let state = JavdbCatalogState::default();
            let request = established_detail_request(&state, "vr", "VrA", "MDVR-419");
            let result = fetch_detail_with(&state, &request, |_| {
                let generation = state
                    .0
                    .lock()
                    .expect("detail state must lock")
                    .vr_detail
                    .as_ref()
                    .expect("the provisional authority must exist")
                    .generation;
                invalidate_detail(&state, "vr", &generation.to_string())
                    .expect("closing must invalidate the pending generation");
                document
                    .map(str::to_owned)
                    .ok_or(ProviderRequestError::Network)
            });
            assert_eq!(result, Err(expected_error));
            assert!(state
                .0
                .lock()
                .expect("detail state must lock")
                .vr_detail
                .is_none());

            fetch_detail_with(&state, &request, |_| {
                Ok(r#"{"success":1,"data":{"movie":{"id":"VrA","number":"MDVR-419","tags":[{"id":"212"}]}}}"#.to_owned())
            })
            .expect("closing a failed attempt must not block an exact retry");
        }
    }

    #[test]
    fn stale_forged_cross_item_and_cross_category_details_cause_no_dispatch() {
        let state = JavdbCatalogState::default();
        let request = established_detail_request(&state, "vr", "VrA", "MDVR-419");
        for invalid in [
            JavdbDetailRequest {
                category: "adult".to_owned(),
                ..request.clone()
            },
            JavdbDetailRequest {
                provider_item_id: "VrB".to_owned(),
                ..request.clone()
            },
            JavdbDetailRequest {
                code: "MDVR-422".to_owned(),
                ..request.clone()
            },
            JavdbDetailRequest {
                request_generation: "999".to_owned(),
                ..request.clone()
            },
            JavdbDetailRequest {
                context_generation: "999".to_owned(),
                ..request.clone()
            },
        ] {
            let dispatched = Cell::new(false);
            assert!(fetch_detail_with(&state, &invalid, |_| {
                dispatched.set(true);
                unreachable!()
            })
            .is_err());
            assert!(!dispatched.get());
        }

        invalidate_catalog(
            &state,
            "vr",
            &NEXT_CONTEXT_GENERATION
                .fetch_add(1, Ordering::Relaxed)
                .to_string(),
        )
        .expect("catalog invalidation must succeed");
        let dispatched = Cell::new(false);
        assert!(fetch_detail_with(&state, &request, |_| {
            dispatched.set(true);
            unreachable!()
        })
        .is_err());
        assert!(!dispatched.get());
    }

    #[test]
    fn detail_images_reject_unretained_stale_cross_context_and_invalid_payloads() {
        let state = JavdbCatalogState::default();
        let request = established_detail_request(&state, "vr", "VrA", "MDVR-419");
        let response = fetch_detail_with(&state, &request, |_| {
            Ok(r#"{"success":1,"data":{"movie":{"id":"VrA","number":"MDVR-419","tags":[{"id":"212"}],"preview_images":[{"large_url":"https://tp.cmastd.com/a.jpg"},{"large_url":"https://tp.spfcas.com/b.jpg"}]}}}"#.to_owned())
        })
        .expect("detail authority must be established");
        for (authority, detail_request) in [
            ("javdb-preview-forged".to_owned(), request.clone()),
            (
                response[16].clone(),
                JavdbDetailRequest {
                    category: "adult".to_owned(),
                    ..request.clone()
                },
            ),
            (
                response[16].clone(),
                JavdbDetailRequest {
                    provider_item_id: "VrB".to_owned(),
                    ..request.clone()
                },
            ),
        ] {
            let dispatched = Cell::new(false);
            assert!(fetch_detail_image_with(
                &state,
                &detail_request,
                &response[0],
                &authority,
                |_| {
                    dispatched.set(true);
                    Ok(jpeg())
                },
            )
            .is_err());
            assert!(!dispatched.get());
        }

        let dispatched = Cell::new(false);
        assert_eq!(
            fetch_detail_image_with(&state, &request, &response[0], &response[16], |_| {
                dispatched.set(true);
                Ok(vec![0; 12])
            },),
            Err(VR_PROVIDER_ERROR)
        );
        assert!(dispatched.get());

        invalidate_detail(&state, "vr", &response[0]).expect("detail invalidation must succeed");
        let dispatched = Cell::new(false);
        assert!(
            fetch_detail_image_with(&state, &request, &response[0], &response[16], |_| {
                dispatched.set(true);
                Ok(jpeg())
            },)
            .is_err()
        );
        assert!(!dispatched.get());
    }

    #[test]
    fn late_detail_and_image_results_cannot_replace_newer_authority() {
        let state = JavdbCatalogState::default();
        let first = established_detail_request(&state, "vr", "VrA", "MDVR-419");
        let late_generation = Cell::new(0);
        let current_response = RefCell::new(None);
        let late_result = fetch_detail_with(&state, &first, |_| {
            late_generation.set(
                state
                    .0
                    .lock()
                    .expect("detail state must lock")
                    .vr_detail
                    .as_ref()
                    .expect("the provisional authority must exist")
                    .generation,
            );
            let current = fetch_detail_with(&state, &first, |_| {
                Ok(r#"{"success":1,"data":{"movie":{"id":"VrA","number":"MDVR-419","tags":[{"id":"212"}],"preview_images":[{"large_url":"https://tp.cmastd.com/current.jpg"}]}}}"#.to_owned())
            })
            .expect("newer detail must finish");
            assert_eq!(current[6], "");
            current_response.replace(Some(current));
            Ok(r#"{"success":1,"data":{"movie":{"id":"VrA","number":"MDVR-419","tags":[{"id":"212"}],"title":"Late"}}}"#.to_owned())
        });
        assert_eq!(late_result, Err(VR_JAVDB_STALE));

        let current = current_response
            .into_inner()
            .expect("the newer detail must remain current");
        let image = current
            .last()
            .expect("preview authority must be present")
            .clone();
        let current_dispatched = Cell::new(false);
        assert_eq!(
            fetch_detail_image_with(&state, &first, &current[0], &image, |_| {
                current_dispatched.set(true);
                Ok(jpeg())
            }),
            Ok(jpeg())
        );
        assert!(current_dispatched.get());

        let stale_source_dispatched = Cell::new(false);
        assert!(open_detail_source_with(
            &state,
            &first,
            &late_generation.get().to_string(),
            |_| {
                stale_source_dispatched.set(true);
                Ok(())
            },
        )
        .is_err());
        assert!(!stale_source_dispatched.get());

        let result = fetch_detail_image_with(&state, &first, &current[0], &image, |_| {
            invalidate_detail(&state, "vr", &current[0]).expect("detail must invalidate");
            Ok(jpeg())
        });
        assert_eq!(result, Err(VR_JAVDB_STALE));
    }

    #[test]
    fn context_change_during_detail_fetch_leaves_no_stale_authority() {
        let state = JavdbCatalogState::default();
        let request = established_detail_request(&state, "vr", "VrA", "MDVR-419");
        let provisional_generation = Cell::new(0);
        let result = fetch_detail_with(&state, &request, |_| {
            provisional_generation.set(
                state
                    .0
                    .lock()
                    .expect("detail state must lock")
                    .vr_detail
                    .as_ref()
                    .expect("the provisional authority must exist")
                    .generation,
            );
            invalidate_catalog(&state, "vr", &request.context_generation)
                .expect("the context change must invalidate its detail authority");
            Ok(r#"{"success":1,"data":{"movie":{"id":"VrA","number":"MDVR-419","tags":[{"id":"212"}]}}}"#.to_owned())
        });
        assert_eq!(result, Err(VR_JAVDB_STALE));

        let source_dispatched = Cell::new(false);
        assert!(open_detail_source_with(
            &state,
            &request,
            &provisional_generation.get().to_string(),
            |_| {
                source_dispatched.set(true);
                Ok(())
            },
        )
        .is_err());
        assert!(!source_dispatched.get());
        let image_dispatched = Cell::new(false);
        assert!(fetch_detail_image_with(
            &state,
            &request,
            &provisional_generation.get().to_string(),
            "javdb-preview-stale",
            |_| {
                image_dispatched.set(true);
                Ok(jpeg())
            },
        )
        .is_err());
        assert!(!image_dispatched.get());
    }
}
