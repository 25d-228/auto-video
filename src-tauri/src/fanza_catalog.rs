use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::process::Command;

use crate::{
    vr_torrent::{JsonParser, JsonValue},
    ProviderRequestError, ADULT_NETWORK_ERROR, ADULT_PROVIDER_ERROR, ADULT_SOURCE_UNAVAILABLE,
    VR_NETWORK_ERROR, VR_PROVIDER_ERROR, VR_SOURCE_UNAVAILABLE,
};

const FANZA_GRAPHQL_URL: &str = "https://api.video.dmm.co.jp/graphql";
const FANZA_ORIGIN: &str = "https://video.dmm.co.jp";
const FANZA_REFERER: &str = "https://video.dmm.co.jp/";
const FANZA_IMAGE_HOST: &str = "awsimgsrc.dmm.co.jp";
const FANZA_RESPONSE_MAX_BYTES: usize = 4 * 1024 * 1024;
const FANZA_IMAGE_MAX_BYTES: usize = 16 * 1024 * 1024;
const FANZA_PREVIEW_LIMIT: usize = 24;
const FANZA_HTTP_STATUS_MARKER: &str = "\nAUTO_VIDEO_HTTP_STATUS:";
#[cfg(target_os = "macos")]
const FANZA_HTTP_STATUS_WRITE_OUT: &str = "\nAUTO_VIDEO_HTTP_STATUS:%{http_code}";

pub(crate) const ADULT_FANZA_MALFORMED: &str = "adult_fanza_malformed_provider";
pub(crate) const ADULT_FANZA_CONFLICTING: &str = "adult_fanza_conflicting_provider";
pub(crate) const ADULT_FANZA_STALE: &str = "adult_fanza_stale";
pub(crate) const VR_FANZA_MALFORMED: &str = "vr_fanza_malformed_provider";
pub(crate) const VR_FANZA_CONFLICTING: &str = "vr_fanza_conflicting_provider";
pub(crate) const VR_FANZA_STALE: &str = "vr_fanza_stale";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Category {
    Adult,
    Vr,
}

impl Category {
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

    fn content_type(self) -> &'static str {
        match self {
            Self::Adult => "TWO_DIMENSION",
            Self::Vr => "VR",
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

    fn malformed(self) -> &'static str {
        match self {
            Self::Adult => ADULT_FANZA_MALFORMED,
            Self::Vr => VR_FANZA_MALFORMED,
        }
    }

    fn conflicting(self) -> &'static str {
        match self {
            Self::Adult => ADULT_FANZA_CONFLICTING,
            Self::Vr => VR_FANZA_CONFLICTING,
        }
    }

    fn stale(self) -> &'static str {
        match self {
            Self::Adult => ADULT_FANZA_STALE,
            Self::Vr => VR_FANZA_STALE,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FanzaCatalogRequest {
    pub category: String,
    pub context_generation: String,
    pub feed: String,
    pub count: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FanzaItemRequest {
    pub category: String,
    pub context_generation: String,
    pub request_generation: String,
    pub provider_item_id: String,
    pub code: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FanzaImageRequest {
    pub item: FanzaItemRequest,
    pub preview_generation: Option<String>,
    pub image_authority_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CatalogItem {
    provider_item_id: String,
    code: String,
    title: Option<String>,
    cover_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ImageAuthority {
    id: String,
    url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AuthorizedItem {
    provider_item_id: String,
    code: String,
    title: Option<String>,
    cover: Option<ImageAuthority>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CatalogAuthority {
    context_generation: u64,
    request_generation: u64,
    items: Vec<AuthorizedItem>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PreviewAuthority {
    generation: u64,
    catalog_context_generation: u64,
    catalog_request_generation: u64,
    provider_item_id: String,
    code: String,
    images: Vec<ImageAuthority>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DetailAuthority {
    generation: u64,
    catalog_context_generation: u64,
    catalog_request_generation: u64,
    provider_item_id: String,
    code: String,
}

#[derive(Default)]
struct CatalogContext {
    generation: u64,
    preview_generation: u64,
    detail_generation: u64,
    adult_context_generation: u64,
    vr_context_generation: u64,
    adult: Option<CatalogAuthority>,
    vr: Option<CatalogAuthority>,
    adult_preview: Option<PreviewAuthority>,
    vr_preview: Option<PreviewAuthority>,
    adult_detail: Option<DetailAuthority>,
    vr_detail: Option<DetailAuthority>,
}

#[derive(Clone, Default)]
pub(crate) struct FanzaCatalogState(Arc<Mutex<CatalogContext>>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DocumentError {
    Malformed,
    Provider,
    Conflicting,
}

fn valid_item_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn canonical_code(value: &str) -> Option<String> {
    let (prefix, number) = value.split_once('-')?;
    if !(2..=16).contains(&prefix.len())
        || !prefix
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        || prefix.bytes().all(|byte| byte.is_ascii_digit())
        || number.is_empty()
        || number.len() > 10
        || number.starts_with('0')
        || !number.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    number.parse::<u64>().ok().filter(|number| *number > 0)?;
    Some(value.to_owned())
}

fn code_from_content_id(value: &str) -> Option<String> {
    if !valid_item_id(value) {
        return None;
    }
    let mut content_id = value;
    if let Some(rest) = content_id.strip_prefix("n_") {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        content_id = &rest[digits..];
    }
    content_id = content_id
        .strip_suffix("btk")
        .or_else(|| content_id.strip_suffix("tk"))
        .unwrap_or(content_id);
    let number_start = content_id
        .bytes()
        .rposition(|byte| !byte.is_ascii_digit())?
        .checked_add(1)?;
    if number_start == content_id.len() {
        return None;
    }
    let mut prefix = &content_id[..number_start];
    for marker in ["k9", "c9", "tk", "tn"] {
        if let Some(rest) = prefix.strip_prefix(marker) {
            if (3..=16).contains(&rest.len()) {
                prefix = rest;
                break;
            }
        }
    }
    if prefix.starts_with('1') && prefix.as_bytes().get(1).is_some_and(u8::is_ascii_digit) {
        prefix = &prefix[1..];
    }
    if !(2..=16).contains(&prefix.len())
        || !prefix.bytes().all(|byte| byte.is_ascii_alphanumeric())
        || prefix.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let number = content_id[number_start..].parse::<u64>().ok()?;
    (number > 0).then(|| format!("{}-{number}", prefix.to_ascii_uppercase()))
}

fn optional_text(
    object: &std::collections::BTreeMap<String, JsonValue>,
    key: &str,
) -> Option<String> {
    match object.get(key) {
        Some(JsonValue::String(value)) => {
            let value = value.trim();
            (!value.is_empty()).then(|| value.to_owned())
        }
        _ => None,
    }
}

fn valid_https_image_url(value: &str) -> bool {
    if value.trim() != value
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b'\\')
    {
        return false;
    }
    let Some(rest) = value.strip_prefix("https://") else {
        return false;
    };
    let authority = rest.split('/').next().unwrap_or_default();
    authority == FANZA_IMAGE_HOST
        && !authority.contains('@')
        && !authority.contains(':')
        && rest.len() > authority.len() + 1
}

fn normalized_cover_url(value: &str) -> Option<String> {
    if !valid_https_image_url(value) {
        return None;
    }
    Some(if let Some(prefix) = value.strip_suffix("pl.jpg") {
        format!("{prefix}ps.jpg")
    } else {
        value.to_owned()
    })
}

fn parse_content(value: &JsonValue) -> Option<CatalogItem> {
    let JsonValue::Object(object) = value else {
        return None;
    };
    let provider_item_id = optional_text(object, "id")?;
    let code = code_from_content_id(&provider_item_id)?;
    let cover_url = match object.get("packageImage") {
        Some(JsonValue::Object(image)) => {
            optional_text(image, "largeUrl").and_then(|url| normalized_cover_url(&url))
        }
        _ => None,
    };
    Some(CatalogItem {
        provider_item_id,
        code,
        title: optional_text(object, "title"),
        cover_url,
    })
}

fn parsed_root(
    document: &str,
) -> Result<std::collections::BTreeMap<String, JsonValue>, DocumentError> {
    let JsonValue::Object(root) = JsonParser::new(document)
        .parse()
        .ok_or(DocumentError::Malformed)?
    else {
        return Err(DocumentError::Malformed);
    };
    match root.get("errors") {
        None | Some(JsonValue::Null) => {}
        Some(JsonValue::Array(errors)) if errors.is_empty() => {}
        Some(JsonValue::Array(_)) => return Err(DocumentError::Provider),
        Some(_) => return Err(DocumentError::Malformed),
    }
    Ok(root)
}

fn parse_catalog_document(document: &str, feed: &str) -> Result<Vec<CatalogItem>, DocumentError> {
    let root = parsed_root(document)?;
    let Some(JsonValue::Object(data)) = root.get("data") else {
        return Err(DocumentError::Malformed);
    };
    let values = if matches!(feed, "popular" | "newest" | "top-rated") {
        let Some(JsonValue::Object(search)) = data.get("legacySearchPPV") else {
            return Err(DocumentError::Malformed);
        };
        let Some(JsonValue::Object(result)) = search.get("result") else {
            return Err(DocumentError::Malformed);
        };
        let Some(JsonValue::Array(contents)) = result.get("contents") else {
            return Err(DocumentError::Malformed);
        };
        contents
    } else {
        let Some(JsonValue::Object(ranking)) = data.get("ppvContentRanking") else {
            return Err(DocumentError::Malformed);
        };
        let Some(JsonValue::Array(items)) = ranking.get("items") else {
            return Err(DocumentError::Malformed);
        };
        items
    };
    let mut items = Vec::new();
    let mut identities = HashMap::<String, CatalogItem>::new();
    for value in values {
        let content = if matches!(feed, "popular" | "newest" | "top-rated") {
            value
        } else {
            match value {
                JsonValue::Object(item) => item.get("content").unwrap_or(value),
                _ => value,
            }
        };
        let Some(item) = parse_content(content) else {
            continue;
        };
        if let Some(previous) = identities.get(&item.provider_item_id) {
            if previous != &item {
                return Err(DocumentError::Conflicting);
            }
            continue;
        }
        identities.insert(item.provider_item_id.clone(), item.clone());
        items.push(item);
    }
    Ok(items)
}

fn valid_request(request: &FanzaCatalogRequest) -> Option<Category> {
    let category = Category::parse(&request.category)?;
    request
        .context_generation
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)?;
    matches!(
        request.feed.as_str(),
        "popular" | "newest" | "top-rated" | "trending" | "monthly"
    )
    .then_some(())?;
    matches!(request.count, 10 | 25 | 50 | 100).then_some(category)
}

fn graphql_body(category: Category, feed: &str, count: u16) -> String {
    if matches!(feed, "popular" | "newest" | "top-rated") {
        let sort = match feed {
            "popular" => "RECOMMENDED",
            "newest" => "RELEASE_DATE",
            _ => "REVIEW_RANK_SCORE",
        };
        format!(
            r#"{{"query":"query S($limit:Int!,$floor:PPVFloor,$sort:ContentSearchPPVSort!,$filter:ContentSearchPPVFilterInput){{legacySearchPPV(limit:$limit,floor:$floor,sort:$sort,filter:$filter,includeExplicit:true){{result{{contents{{id title packageImage{{largeUrl}}}}}}}}}}","variables":{{"limit":{count},"floor":"AV","sort":"{sort}","filter":{{"contentType":"{}"}}}}}}"#,
            category.content_type()
        )
    } else {
        let ranking = if feed == "trending" {
            "SALES_BEST_SELLERS"
        } else {
            "SALES_MONTHLY"
        };
        format!(
            r#"{{"query":"{{ppvContentRanking(floor:AV,type:{ranking},limit:{count},contentType:{}){{items{{content{{id title packageImage{{largeUrl}}}}}}}}}}","variables":{{}}}}"#,
            category.content_type()
        )
    }
}

fn authority(context: &CatalogContext, category: Category) -> Option<&CatalogAuthority> {
    match category {
        Category::Adult => context.adult.as_ref(),
        Category::Vr => context.vr.as_ref(),
    }
}

fn preview(context: &CatalogContext, category: Category) -> Option<&PreviewAuthority> {
    match category {
        Category::Adult => context.adult_preview.as_ref(),
        Category::Vr => context.vr_preview.as_ref(),
    }
}

fn set_preview(context: &mut CatalogContext, category: Category, value: Option<PreviewAuthority>) {
    match category {
        Category::Adult => context.adult_preview = value,
        Category::Vr => context.vr_preview = value,
    }
}

fn detail_authority(context: &CatalogContext, category: Category) -> Option<&DetailAuthority> {
    match category {
        Category::Adult => context.adult_detail.as_ref(),
        Category::Vr => context.vr_detail.as_ref(),
    }
}

fn set_detail(context: &mut CatalogContext, category: Category, value: Option<DetailAuthority>) {
    match category {
        Category::Adult => context.adult_detail = value,
        Category::Vr => context.vr_detail = value,
    }
}

fn parsed_item_request(request: &FanzaItemRequest) -> Result<(Category, u64, u64), &'static str> {
    let category = Category::parse(&request.category).ok_or(VR_PROVIDER_ERROR)?;
    let context_generation = request
        .context_generation
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| category.stale())?;
    let request_generation = request
        .request_generation
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| category.stale())?;
    if !valid_item_id(&request.provider_item_id)
        || canonical_code(&request.code).as_deref() != Some(&request.code)
    {
        return Err(category.stale());
    }
    Ok((category, context_generation, request_generation))
}

fn find_item<'a>(
    context: &'a CatalogContext,
    category: Category,
    request: &FanzaItemRequest,
) -> Option<&'a AuthorizedItem> {
    let (_, context_generation, request_generation) = parsed_item_request(request).ok()?;
    authority(context, category)
        .filter(|value| {
            value.context_generation == context_generation
                && value.request_generation == request_generation
        })?
        .items
        .iter()
        .find(|item| item.provider_item_id == request.provider_item_id && item.code == request.code)
}

pub(crate) fn fetch_catalog_with(
    state: &FanzaCatalogState,
    request: &FanzaCatalogRequest,
    fetch: impl FnOnce(&str) -> Result<String, ProviderRequestError>,
) -> Result<Vec<String>, &'static str> {
    let category = valid_request(request).ok_or(VR_PROVIDER_ERROR)?;
    let context_generation = request
        .context_generation
        .parse::<u64>()
        .map_err(|_| category.stale())?;
    let generation = {
        let mut context = state
            .0
            .lock()
            .map_err(|_| category.provider_error(ProviderRequestError::Provider))?;
        let current = match category {
            Category::Adult => &mut context.adult_context_generation,
            Category::Vr => &mut context.vr_context_generation,
        };
        if context_generation <= *current {
            return Err(category.stale());
        }
        *current = context_generation;
        context.generation = context
            .generation
            .checked_add(1)
            .ok_or_else(|| category.provider_error(ProviderRequestError::Provider))?;
        let generation = context.generation;
        let authority = CatalogAuthority {
            context_generation,
            request_generation: generation,
            items: Vec::new(),
        };
        match category {
            Category::Adult => {
                context.adult = Some(authority);
                context.adult_preview = None;
                context.adult_detail = None;
            }
            Category::Vr => {
                context.vr = Some(authority);
                context.vr_preview = None;
                context.vr_detail = None;
            }
        }
        generation
    };
    let document = fetch(&graphql_body(category, &request.feed, request.count))
        .map_err(|error| category.provider_error(error))?;
    let items = parse_catalog_document(&document, &request.feed).map_err(|error| match error {
        DocumentError::Malformed => category.malformed(),
        DocumentError::Provider => category.provider_error(ProviderRequestError::Provider),
        DocumentError::Conflicting => category.conflicting(),
    })?;
    let mut context = state.0.lock().map_err(|_| category.stale())?;
    let current = match category {
        Category::Adult => context.adult.as_mut(),
        Category::Vr => context.vr.as_mut(),
    }
    .filter(|value| value.request_generation == generation)
    .ok_or_else(|| category.stale())?;
    current.items = items
        .into_iter()
        .enumerate()
        .map(|(index, item)| AuthorizedItem {
            provider_item_id: item.provider_item_id,
            code: item.code,
            title: item.title,
            cover: item.cover_url.map(|url| ImageAuthority {
                id: format!("fanza-cover-{generation}-{}", index + 1),
                url,
            }),
        })
        .collect();
    let mut response = vec![generation.to_string(), current.items.len().to_string()];
    for item in &current.items {
        response.extend([
            category.value().to_owned(),
            item.provider_item_id.clone(),
            item.code.clone(),
            item.title.clone().unwrap_or_default(),
            item.cover
                .as_ref()
                .map(|cover| cover.id.clone())
                .unwrap_or_default(),
            "0.72".to_owned(),
        ]);
    }
    Ok(response)
}

pub(crate) fn invalidate_catalog(
    state: &FanzaCatalogState,
    category: &str,
    context_generation: &str,
) -> Result<(), &'static str> {
    let category = Category::parse(category).ok_or(VR_PROVIDER_ERROR)?;
    let generation = context_generation
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| category.stale())?;
    let mut context = state.0.lock().map_err(|_| category.stale())?;
    let current = match category {
        Category::Adult => &mut context.adult_context_generation,
        Category::Vr => &mut context.vr_context_generation,
    };
    if generation <= *current {
        return Err(category.stale());
    }
    *current = generation;
    match category {
        Category::Adult => {
            context.adult = None;
            context.adult_preview = None;
            context.adult_detail = None;
        }
        Category::Vr => {
            context.vr = None;
            context.vr_preview = None;
            context.vr_detail = None;
        }
    }
    Ok(())
}

pub(crate) fn detail(
    state: &FanzaCatalogState,
    request: &FanzaItemRequest,
) -> Result<Vec<String>, &'static str> {
    let (category, catalog_context_generation, catalog_request_generation) =
        parsed_item_request(request)?;
    let mut context = state.0.lock().map_err(|_| category.stale())?;
    let item = find_item(&context, category, request)
        .ok_or_else(|| category.stale())?
        .clone();
    context.detail_generation = context
        .detail_generation
        .checked_add(1)
        .ok_or_else(|| category.provider_error(ProviderRequestError::Provider))?;
    let generation = context.detail_generation;
    set_detail(
        &mut context,
        category,
        Some(DetailAuthority {
            generation,
            catalog_context_generation,
            catalog_request_generation,
            provider_item_id: item.provider_item_id.clone(),
            code: item.code.clone(),
        }),
    );
    Ok(vec![
        generation.to_string(),
        category.value().to_owned(),
        request.context_generation.clone(),
        request.request_generation.clone(),
        item.provider_item_id.clone(),
        item.code.clone(),
        item.title.clone().unwrap_or_default(),
        item.cover
            .as_ref()
            .map(|cover| cover.id.clone())
            .unwrap_or_default(),
    ])
}

pub(crate) fn invalidate_detail(
    state: &FanzaCatalogState,
    category: &str,
    detail_generation: &str,
) -> Result<(), &'static str> {
    let category = Category::parse(category).ok_or(VR_PROVIDER_ERROR)?;
    let generation = detail_generation
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| category.stale())?;
    let mut context = state.0.lock().map_err(|_| category.stale())?;
    if detail_authority(&context, category).is_some_and(|detail| detail.generation == generation) {
        set_detail(&mut context, category, None);
    }
    Ok(())
}

fn preview_body(provider_item_id: &str) -> String {
    format!(
        r#"{{"query":"{{ppvContent(id:\"{provider_item_id}\"){{id sampleImages{{largeImageUrl}}}}}}","variables":{{}}}}"#
    )
}

fn parse_preview_document(
    document: &str,
    provider_item_id: &str,
) -> Result<Vec<String>, DocumentError> {
    let root = parsed_root(document)?;
    let Some(JsonValue::Object(data)) = root.get("data") else {
        return Err(DocumentError::Malformed);
    };
    let Some(JsonValue::Object(content)) = data.get("ppvContent") else {
        return Err(DocumentError::Malformed);
    };
    if optional_text(content, "id").as_deref() != Some(provider_item_id) {
        return Err(DocumentError::Conflicting);
    }
    let images = match content.get("sampleImages") {
        Some(JsonValue::Array(images)) => images,
        Some(JsonValue::Null) | None => return Ok(Vec::new()),
        _ => return Err(DocumentError::Malformed),
    };
    let mut urls = Vec::new();
    for image in images {
        let Some(url) = (match image {
            JsonValue::Object(object) => optional_text(object, "largeImageUrl"),
            _ => None,
        }) else {
            continue;
        };
        if valid_https_image_url(&url) && !urls.contains(&url) {
            urls.push(url);
            if urls.len() == FANZA_PREVIEW_LIMIT {
                break;
            }
        }
    }
    Ok(urls)
}

pub(crate) fn fetch_preview_with(
    state: &FanzaCatalogState,
    request: &FanzaItemRequest,
    fetch: impl FnOnce(&str) -> Result<String, ProviderRequestError>,
) -> Result<Vec<String>, &'static str> {
    let (category, context_generation, request_generation) = parsed_item_request(request)?;
    let generation = {
        let mut context = state.0.lock().map_err(|_| category.stale())?;
        if find_item(&context, category, request).is_none() {
            return Err(category.stale());
        }
        context.preview_generation = context
            .preview_generation
            .checked_add(1)
            .ok_or_else(|| category.provider_error(ProviderRequestError::Provider))?;
        let generation = context.preview_generation;
        set_preview(
            &mut context,
            category,
            Some(PreviewAuthority {
                generation,
                catalog_context_generation: context_generation,
                catalog_request_generation: request_generation,
                provider_item_id: request.provider_item_id.clone(),
                code: request.code.clone(),
                images: Vec::new(),
            }),
        );
        generation
    };
    let document = fetch(&preview_body(&request.provider_item_id))
        .map_err(|error| category.provider_error(error))?;
    let urls =
        parse_preview_document(&document, &request.provider_item_id).map_err(
            |error| match error {
                DocumentError::Malformed => category.malformed(),
                DocumentError::Provider => category.provider_error(ProviderRequestError::Provider),
                DocumentError::Conflicting => category.conflicting(),
            },
        )?;
    let mut context = state.0.lock().map_err(|_| category.stale())?;
    if find_item(&context, category, request).is_none() {
        return Err(category.stale());
    }
    let current = match category {
        Category::Adult => context.adult_preview.as_mut(),
        Category::Vr => context.vr_preview.as_mut(),
    }
    .filter(|value| value.generation == generation)
    .ok_or_else(|| category.stale())?;
    current.images = urls
        .into_iter()
        .enumerate()
        .map(|(index, url)| ImageAuthority {
            id: format!("fanza-preview-{generation}-{}", index + 1),
            url,
        })
        .collect();
    let mut response = vec![generation.to_string(), current.images.len().to_string()];
    response.extend(current.images.iter().map(|image| image.id.clone()));
    Ok(response)
}

pub(crate) fn invalidate_preview(
    state: &FanzaCatalogState,
    category: &str,
    preview_generation: &str,
) -> Result<(), &'static str> {
    let category = Category::parse(category).ok_or(VR_PROVIDER_ERROR)?;
    let generation = preview_generation
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| category.stale())?;
    let mut context = state.0.lock().map_err(|_| category.stale())?;
    if preview(&context, category).is_some_and(|value| value.generation == generation) {
        set_preview(&mut context, category, None);
    }
    Ok(())
}

fn image_url(
    state: &FanzaCatalogState,
    request: &FanzaImageRequest,
) -> Result<(Category, String), &'static str> {
    let (category, _, _) = parsed_item_request(&request.item)?;
    let context = state.0.lock().map_err(|_| category.stale())?;
    if let Some(generation) = &request.preview_generation {
        let generation = generation
            .parse::<u64>()
            .ok()
            .filter(|value| *value > 0)
            .ok_or_else(|| category.stale())?;
        let authority = preview(&context, category)
            .filter(|value| {
                value.generation == generation
                    && value.catalog_context_generation.to_string()
                        == request.item.context_generation
                    && value.catalog_request_generation.to_string()
                        == request.item.request_generation
                    && value.provider_item_id == request.item.provider_item_id
                    && value.code == request.item.code
            })
            .ok_or_else(|| category.stale())?;
        return authority
            .images
            .iter()
            .find(|image| image.id == request.image_authority_id)
            .map(|image| (category, image.url.clone()))
            .ok_or_else(|| category.stale());
    }
    let item = find_item(&context, category, &request.item).ok_or_else(|| category.stale())?;
    item.cover
        .as_ref()
        .filter(|cover| cover.id == request.image_authority_id)
        .map(|cover| (category, cover.url.clone()))
        .ok_or_else(|| category.stale())
}

fn accepted_raster(bytes: &[u8]) -> bool {
    bytes.len() >= 12
        && bytes.len() <= FANZA_IMAGE_MAX_BYTES
        && (bytes.starts_with(&[0xff, 0xd8, 0xff])
            || bytes.starts_with(&[0x89, b'P', b'N', b'G'])
            || bytes.starts_with(b"GIF8")
            || (bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP")))
}

pub(crate) fn fetch_image_with(
    state: &FanzaCatalogState,
    request: &FanzaImageRequest,
    fetch: impl FnOnce(&str) -> Result<Vec<u8>, ProviderRequestError>,
) -> Result<Vec<u8>, &'static str> {
    let (category, url) = image_url(state, request)?;
    let bytes = fetch(&url).map_err(|error| category.provider_error(error))?;
    if !accepted_raster(&bytes) {
        return Err(category.provider_error(ProviderRequestError::Provider));
    }
    if image_url(state, request)?.1 != url {
        return Err(category.stale());
    }
    Ok(bytes)
}

pub(crate) fn open_source_with(
    state: &FanzaCatalogState,
    request: &FanzaItemRequest,
    detail_generation: &str,
    open: impl FnOnce(&str) -> Result<(), ()>,
) -> Result<(), &'static str> {
    let (category, catalog_context_generation, catalog_request_generation) =
        parsed_item_request(request)?;
    let detail_generation = detail_generation
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| category.stale())?;
    let context = state.0.lock().map_err(|_| category.stale())?;
    find_item(&context, category, request).ok_or_else(|| category.stale())?;
    if !detail_authority(&context, category).is_some_and(|detail| {
        detail.generation == detail_generation
            && detail.catalog_context_generation == catalog_context_generation
            && detail.catalog_request_generation == catalog_request_generation
            && detail.provider_item_id == request.provider_item_id
            && detail.code == request.code
    }) {
        return Err(category.stale());
    }
    open(&format!(
        "https://video.dmm.co.jp/av/content/?id={}",
        request.provider_item_id
    ))
    .map_err(|_| category.provider_error(ProviderRequestError::Provider))
}

fn parse_text_response(output: &[u8], maximum: usize) -> Result<Vec<u8>, ProviderRequestError> {
    let marker = FANZA_HTTP_STATUS_MARKER.as_bytes();
    let position = output
        .windows(marker.len())
        .rposition(|window| window == marker)
        .ok_or(ProviderRequestError::Provider)?;
    let status = std::str::from_utf8(&output[position + marker.len()..])
        .map_err(|_| ProviderRequestError::Provider)?
        .trim()
        .parse::<u16>()
        .map_err(|_| ProviderRequestError::Provider)?;
    let body = &output[..position];
    match status {
        200..=299 if !body.is_empty() && body.len() <= maximum => Ok(body.to_vec()),
        404 | 410 | 451 => Err(ProviderRequestError::SourceUnavailable),
        0 => Err(ProviderRequestError::Network),
        _ => Err(ProviderRequestError::Provider),
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn fetch_graphql_document(body: &str) -> Result<String, ProviderRequestError> {
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
            "--request",
            "POST",
            "--header",
            "Accept: application/json",
            "--header",
            "Content-Type: application/json",
            "--header",
            &format!("Origin: {FANZA_ORIGIN}"),
            "--header",
            &format!("Referer: {FANZA_REFERER}"),
            "--data-binary",
            body,
            "--write-out",
            FANZA_HTTP_STATUS_WRITE_OUT,
            FANZA_GRAPHQL_URL,
        ])
        .output()
        .map_err(|_| ProviderRequestError::Network)?;
    if !output.status.success() {
        return Err(ProviderRequestError::Network);
    }
    String::from_utf8(parse_text_response(
        &output.stdout,
        FANZA_RESPONSE_MAX_BYTES,
    )?)
    .map_err(|_| ProviderRequestError::Provider)
}

#[cfg(target_os = "windows")]
pub(crate) fn fetch_graphql_document(body: &str) -> Result<String, ProviderRequestError> {
    let script = "$ProgressPreference='SilentlyContinue';try{$r=Invoke-WebRequest -UseBasicParsing -MaximumRedirection 0 -TimeoutSec 20 -Method Post -Uri $env:FANZA_URL -ContentType 'application/json' -Headers @{Accept='application/json';Origin=$env:FANZA_ORIGIN;Referer=$env:FANZA_REFERER} -Body $env:FANZA_BODY;[Console]::Out.Write($r.Content);[Console]::Out.Write('`nAUTO_VIDEO_HTTP_STATUS:'+[int]$r.StatusCode)}catch{if($_.Exception.Response){[Console]::Out.Write('`nAUTO_VIDEO_HTTP_STATUS:'+[int]$_.Exception.Response.StatusCode.value__)}else{[Console]::Out.Write('`nAUTO_VIDEO_HTTP_STATUS:0')}}";
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("FANZA_URL", FANZA_GRAPHQL_URL)
        .env("FANZA_ORIGIN", FANZA_ORIGIN)
        .env("FANZA_REFERER", FANZA_REFERER)
        .env("FANZA_BODY", body)
        .output()
        .map_err(|_| ProviderRequestError::Network)?;
    String::from_utf8(parse_text_response(
        &output.stdout,
        FANZA_RESPONSE_MAX_BYTES,
    )?)
    .map_err(|_| ProviderRequestError::Provider)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn fetch_graphql_document(_body: &str) -> Result<String, ProviderRequestError> {
    Err(ProviderRequestError::SourceUnavailable)
}

#[cfg(target_os = "macos")]
pub(crate) fn fetch_image_bytes(url: &str) -> Result<Vec<u8>, ProviderRequestError> {
    if !valid_https_image_url(url) {
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
            "--header",
            &format!("Referer: {FANZA_REFERER}"),
            "--write-out",
            FANZA_HTTP_STATUS_WRITE_OUT,
            url,
        ])
        .output()
        .map_err(|_| ProviderRequestError::Network)?;
    if !output.status.success() {
        return Err(ProviderRequestError::Network);
    }
    parse_text_response(&output.stdout, FANZA_IMAGE_MAX_BYTES)
}

#[cfg(target_os = "windows")]
pub(crate) fn fetch_image_bytes(url: &str) -> Result<Vec<u8>, ProviderRequestError> {
    if !valid_https_image_url(url) {
        return Err(ProviderRequestError::Provider);
    }
    let script = "$ProgressPreference='SilentlyContinue';try{$r=Invoke-WebRequest -UseBasicParsing -MaximumRedirection 0 -TimeoutSec 20 -Uri $env:FANZA_IMAGE -Headers @{Referer=$env:FANZA_REFERER};[Console]::Out.Write([Convert]::ToBase64String($r.Content));[Console]::Out.Write('`nAUTO_VIDEO_HTTP_STATUS:'+[int]$r.StatusCode)}catch{if($_.Exception.Response){[Console]::Out.Write('`nAUTO_VIDEO_HTTP_STATUS:'+[int]$_.Exception.Response.StatusCode.value__)}else{[Console]::Out.Write('`nAUTO_VIDEO_HTTP_STATUS:0')}}";
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("FANZA_IMAGE", url)
        .env("FANZA_REFERER", FANZA_REFERER)
        .output()
        .map_err(|_| ProviderRequestError::Network)?;
    let encoded = parse_text_response(&output.stdout, FANZA_IMAGE_MAX_BYTES * 2)?;
    crate::decode_base64(std::str::from_utf8(&encoded).map_err(|_| ProviderRequestError::Provider)?)
        .ok_or(ProviderRequestError::Provider)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn fetch_image_bytes(_url: &str) -> Result<Vec<u8>, ProviderRequestError> {
    Err(ProviderRequestError::SourceUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raster_bytes() -> Vec<u8> {
        vec![0xff, 0xd8, 0xff, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    }

    fn catalog_document(feed: &str, contents: &str) -> String {
        if matches!(feed, "popular" | "newest" | "top-rated") {
            format!(r#"{{"data":{{"legacySearchPPV":{{"result":{{"contents":[{contents}]}}}}}}}}"#)
        } else {
            format!(r#"{{"data":{{"ppvContentRanking":{{"items":[{contents}]}}}}}}"#)
        }
    }

    #[test]
    fn maps_representative_content_ids_to_exact_codes() {
        for (content_id, code) in [
            ("vrkm01577", "VRKM-1577"),
            ("13dsvr01947", "3DSVR-1947"),
            ("n_709maraa244tk", "MARAA-244"),
            ("k9snos258", "SNOS-258"),
            ("tkipzz855", "IPZZ-855"),
            ("ovvr616", "OVVR-616"),
        ] {
            assert_eq!(code_from_content_id(content_id).as_deref(), Some(code));
        }
    }

    #[test]
    fn maps_every_feed_to_the_exact_operation_and_content_type() {
        for category in [Category::Adult, Category::Vr] {
            for (feed, marker) in [
                ("popular", "RECOMMENDED"),
                ("newest", "RELEASE_DATE"),
                ("top-rated", "REVIEW_RANK_SCORE"),
                ("trending", "SALES_BEST_SELLERS"),
                ("monthly", "SALES_MONTHLY"),
            ] {
                let body = graphql_body(category, feed, 25);
                assert!(body.contains(marker));
                assert!(body.contains(category.content_type()));
                assert!(body.contains("\"limit\":25") || body.contains("limit:25"));
                if matches!(feed, "popular" | "newest" | "top-rated") {
                    assert!(body.contains("includeExplicit:true"));
                }
            }
        }
    }

    #[test]
    fn accepts_only_the_exact_https_fanza_image_origin() {
        assert!(valid_https_image_url(
            "https://awsimgsrc.dmm.co.jp/dig/digital/video/vrkm01577/ps.jpg"
        ));
        for url in [
            "http://awsimgsrc.dmm.co.jp/image.jpg",
            "https://user@awsimgsrc.dmm.co.jp/image.jpg",
            "https://awsimgsrc.dmm.co.jp:443/image.jpg",
            "https://awsimgsrc.dmm.co.jp.evil.example/image.jpg",
            "https://evil.example/awsimgsrc.dmm.co.jp/image.jpg",
            "https://awsimgsrc.dmm.co.jp\\image.jpg",
            " https://awsimgsrc.dmm.co.jp/image.jpg",
            "https://awsimgsrc.dmm.co.jp/image.jpg\n",
        ] {
            assert!(!valid_https_image_url(url), "accepted {url}");
        }
    }

    #[test]
    fn isolates_bad_rows_and_rejects_conflicting_duplicates() {
        let valid = r#"{"id":"vrkm01577","title":"First","packageImage":{"largeUrl":"https://awsimgsrc.dmm.co.jp/path/pl.jpg"}}"#;
        let invalid = r#"{"id":"BAD-ID","title":"Bad"}"#;
        let document = catalog_document("popular", &format!("{valid},{invalid}"));
        let parsed = parse_catalog_document(&document, "popular").unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].code, "VRKM-1577");
        assert_eq!(
            parsed[0].cover_url.as_deref(),
            Some("https://awsimgsrc.dmm.co.jp/path/ps.jpg")
        );
        let conflict = catalog_document(
            "popular",
            &format!("{valid},{{\"id\":\"vrkm01577\",\"title\":\"Different\"}}"),
        );
        assert_eq!(
            parse_catalog_document(&conflict, "popular"),
            Err(DocumentError::Conflicting)
        );
    }

    #[test]
    fn cover_and_preview_authority_rejects_forged_stale_and_cross_item_requests() {
        let state = FanzaCatalogState::default();
        let request = FanzaCatalogRequest {
            category: "vr".into(),
            context_generation: "1".into(),
            feed: "popular".into(),
            count: 10,
        };
        let response = fetch_catalog_with(&state, &request, |_| Ok(catalog_document("popular", r#"{"id":"vrkm01577","title":"First","packageImage":{"largeUrl":"https://awsimgsrc.dmm.co.jp/path/pl.jpg"}},{"id":"ovvr616","title":"Second"}"#))).unwrap();
        let item = FanzaItemRequest {
            category: "vr".into(),
            context_generation: "1".into(),
            request_generation: response[0].clone(),
            provider_item_id: "vrkm01577".into(),
            code: "VRKM-1577".into(),
        };
        let cover = FanzaImageRequest {
            item: item.clone(),
            preview_generation: None,
            image_authority_id: response[6].clone(),
        };
        let dispatched = std::cell::Cell::new(false);
        assert_eq!(
            fetch_image_with(&state, &cover, |url| {
                dispatched.set(true);
                assert_eq!(url, "https://awsimgsrc.dmm.co.jp/path/ps.jpg");
                Ok(raster_bytes())
            }),
            Ok(raster_bytes())
        );
        assert!(dispatched.get());
        let forged = FanzaImageRequest {
            image_authority_id: "fanza-cover-1-2".into(),
            ..cover.clone()
        };
        let dispatched = std::cell::Cell::new(false);
        assert_eq!(
            fetch_image_with(&state, &forged, |_| {
                dispatched.set(true);
                Ok(vec![])
            }),
            Err(VR_FANZA_STALE)
        );
        assert!(!dispatched.get());
        let preview = fetch_preview_with(&state, &item, |_| Ok(r#"{"data":{"ppvContent":{"id":"vrkm01577","sampleImages":[{"largeImageUrl":"https://awsimgsrc.dmm.co.jp/preview/1.jpg"},{"largeImageUrl":"https://evil.example/2.jpg"}]}}}"#.into())).unwrap();
        assert_eq!(preview.len(), 3);
        let preview_request = FanzaImageRequest {
            item,
            preview_generation: Some(preview[0].clone()),
            image_authority_id: preview[2].clone(),
        };
        assert_eq!(
            fetch_image_with(&state, &preview_request, |url| {
                assert_eq!(url, "https://awsimgsrc.dmm.co.jp/preview/1.jpg");
                Ok(raster_bytes())
            }),
            Ok(raster_bytes())
        );
    }

    #[test]
    fn rejects_invalid_rasters_and_authority_invalidated_during_fetch() {
        let state = FanzaCatalogState::default();
        let response = fetch_catalog_with(
            &state,
            &FanzaCatalogRequest {
                category: "vr".into(),
                context_generation: "1".into(),
                feed: "popular".into(),
                count: 10,
            },
            |_| {
                Ok(catalog_document(
                    "popular",
                    r#"{"id":"vrkm01577","packageImage":{"largeUrl":"https://awsimgsrc.dmm.co.jp/path/pl.jpg"}}"#,
                ))
            },
        )
        .unwrap();
        let request = FanzaImageRequest {
            item: FanzaItemRequest {
                category: "vr".into(),
                context_generation: "1".into(),
                request_generation: response[0].clone(),
                provider_item_id: "vrkm01577".into(),
                code: "VRKM-1577".into(),
            },
            preview_generation: None,
            image_authority_id: response[6].clone(),
        };
        assert_eq!(
            fetch_image_with(&state, &request, |_| Ok(vec![0; 12])),
            Err(VR_PROVIDER_ERROR)
        );
        assert_eq!(
            fetch_image_with(&state, &request, |_| {
                invalidate_catalog(&state, "vr", "2").unwrap();
                Ok(raster_bytes())
            }),
            Err(VR_FANZA_STALE)
        );
    }

    #[test]
    fn invalidation_blocks_late_results_and_source_authority_is_exact() {
        let state = FanzaCatalogState::default();
        let request = FanzaCatalogRequest {
            category: "adult".into(),
            context_generation: "1".into(),
            feed: "newest".into(),
            count: 10,
        };
        let response = fetch_catalog_with(&state, &request, |_| {
            Ok(catalog_document(
                "newest",
                r#"{"id":"k9snos258","title":"Title"}"#,
            ))
        })
        .unwrap();
        let item = FanzaItemRequest {
            category: "adult".into(),
            context_generation: "1".into(),
            request_generation: response[0].clone(),
            provider_item_id: "k9snos258".into(),
            code: "SNOS-258".into(),
        };
        let detail = detail(&state, &item).unwrap();
        let opened = std::cell::RefCell::new(String::new());
        open_source_with(&state, &item, &detail[0], |url| {
            opened.replace(url.into());
            Ok(())
        })
        .unwrap();
        assert_eq!(
            &*opened.borrow(),
            "https://video.dmm.co.jp/av/content/?id=k9snos258"
        );
        invalidate_catalog(&state, "adult", "2").unwrap();
        assert_eq!(
            open_source_with(&state, &item, &detail[0], |_| Ok(())),
            Err(ADULT_FANZA_STALE)
        );
    }

    #[test]
    fn invalidated_detail_cannot_dispatch_a_source_action() {
        let state = FanzaCatalogState::default();
        let request = FanzaCatalogRequest {
            category: "adult".into(),
            context_generation: "1".into(),
            feed: "popular".into(),
            count: 10,
        };
        let response = fetch_catalog_with(&state, &request, |_| {
            Ok(catalog_document(
                "popular",
                r#"{"id":"k9snos258","title":"Title"}"#,
            ))
        })
        .unwrap();
        let item = FanzaItemRequest {
            category: "adult".into(),
            context_generation: "1".into(),
            request_generation: response[0].clone(),
            provider_item_id: "k9snos258".into(),
            code: "SNOS-258".into(),
        };
        let detail = detail(&state, &item).unwrap();
        invalidate_detail(&state, "adult", &detail[0]).unwrap();
        let dispatched = std::cell::Cell::new(false);
        assert_eq!(
            open_source_with(&state, &item, &detail[0], |_| {
                dispatched.set(true);
                Ok(())
            }),
            Err(ADULT_FANZA_STALE)
        );
        assert!(!dispatched.get());
    }
}
