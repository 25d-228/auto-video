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
const FANZA_HTTP_STATUS_MARKER: &str = "\nAUTO_VIDEO_HTTP_STATUS:";
#[cfg(target_os = "macos")]
const FANZA_HTTP_STATUS_WRITE_OUT: &str = "\nAUTO_VIDEO_HTTP_STATUS:%{http_code}";

#[cfg(any(test, target_os = "windows"))]
const WINDOWS_FANZA_GRAPHQL_SCRIPT: &str = r#"$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
Add-Type -AssemblyName System.Net.Http
$handler = [System.Net.Http.HttpClientHandler]::new()
$handler.AllowAutoRedirect = $false
$client = [System.Net.Http.HttpClient]::new($handler)
$deadline = [System.Threading.CancellationTokenSource]::new()
$deadline.CancelAfter(20000)
try {
  $request = [System.Net.Http.HttpRequestMessage]::new([System.Net.Http.HttpMethod]::Post, $env:FANZA_URL)
  $request.Content = [System.Net.Http.StringContent]::new($env:FANZA_BODY, [System.Text.Encoding]::UTF8, 'application/json')
  $request.Headers.Accept.ParseAdd('application/json')
  $request.Headers.Referrer = [Uri]$env:FANZA_REFERER
  $request.Headers.TryAddWithoutValidation('Origin', $env:FANZA_ORIGIN) | Out-Null
  $response = $client.SendAsync($request, [System.Net.Http.HttpCompletionOption]::ResponseHeadersRead, $deadline.Token).GetAwaiter().GetResult()
  $stream = $response.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
  $memory = [System.IO.MemoryStream]::new()
  $buffer = [byte[]]::new(65536)
  $tooLarge = $false
  while (($read = $stream.ReadAsync($buffer, 0, $buffer.Length, $deadline.Token).GetAwaiter().GetResult()) -gt 0) {
    if ($memory.Length + $read -gt 4194304) { $tooLarge = $true; break }
    $memory.Write($buffer, 0, $read)
  }
  $output = [Console]::OpenStandardOutput()
  if (-not $tooLarge) {
    $responseBytes = $memory.ToArray()
    $output.Write($responseBytes, 0, $responseBytes.Length)
  }
  $status = if ($tooLarge) { 413 } else { [int]$response.StatusCode }
  $marker = [System.Text.Encoding]::UTF8.GetBytes("`nAUTO_VIDEO_HTTP_STATUS:" + $status)
  $output.Write($marker, 0, $marker.Length)
} catch {
  [Environment]::Exit(28)
} finally {
  $deadline.Dispose()
  $client.Dispose()
  $handler.Dispose()
}"#;

#[cfg(any(test, target_os = "windows"))]
const WINDOWS_FANZA_IMAGE_SCRIPT: &str = r#"$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
Add-Type -AssemblyName System.Net.Http
$handler = [System.Net.Http.HttpClientHandler]::new()
$handler.AllowAutoRedirect = $false
$client = [System.Net.Http.HttpClient]::new($handler)
$deadline = [System.Threading.CancellationTokenSource]::new()
$deadline.CancelAfter(20000)
try {
  $request = [System.Net.Http.HttpRequestMessage]::new([System.Net.Http.HttpMethod]::Get, $env:FANZA_IMAGE)
  $request.Headers.Referrer = [Uri]$env:FANZA_REFERER
  $response = $client.SendAsync($request, [System.Net.Http.HttpCompletionOption]::ResponseHeadersRead, $deadline.Token).GetAwaiter().GetResult()
  $stream = $response.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
  $memory = [System.IO.MemoryStream]::new()
  $buffer = [byte[]]::new(65536)
  $tooLarge = $false
  while (($read = $stream.ReadAsync($buffer, 0, $buffer.Length, $deadline.Token).GetAwaiter().GetResult()) -gt 0) {
    if ($memory.Length + $read -gt 16777216) { $tooLarge = $true; break }
    $memory.Write($buffer, 0, $read)
  }
  if ($tooLarge) {
    [Console]::Out.Write("`nAUTO_VIDEO_HTTP_STATUS:413")
  } else {
    [Console]::Out.Write([Convert]::ToBase64String($memory.ToArray()))
    [Console]::Out.Write("`nAUTO_VIDEO_HTTP_STATUS:" + [int]$response.StatusCode)
  }
} catch {
  [Environment]::Exit(28)
} finally {
  $deadline.Dispose()
  $client.Dispose()
  $handler.Dispose()
}"#;

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

    fn request_error(self, error: ProviderRequestError) -> &'static str {
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
struct ParsedItem {
    provider_item_id: String,
    code: String,
    title: Option<String>,
    cover_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CoverAuthority {
    id: String,
    url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AuthorizedItem {
    provider_item_id: String,
    code: String,
    title: Option<String>,
    cover: Option<CoverAuthority>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CatalogAuthority {
    context_generation: u64,
    request_generation: u64,
    items: Vec<AuthorizedItem>,
}

#[derive(Default)]
struct CatalogContext {
    request_generation: u64,
    adult_context_generation: u64,
    vr_context_generation: u64,
    adult: Option<CatalogAuthority>,
    vr: Option<CatalogAuthority>,
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

fn code_from_content_id(value: &str) -> Option<String> {
    if !valid_item_id(value) {
        return None;
    }
    let mut content_id = value;
    if let Some(rest) = content_id.strip_prefix("n_") {
        let distributor_digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if distributor_digits == 0 {
            return None;
        }
        content_id = &rest[distributor_digits..];
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
    for distributor in ["k9", "c9", "tk", "tn"] {
        if let Some(rest) = prefix.strip_prefix(distributor) {
            if (2..=16).contains(&rest.len()) {
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
        || !prefix
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphabetic)
    {
        return None;
    }
    let number = content_id[number_start..].parse::<u64>().ok()?;
    let code = (number > 0).then(|| format!("{}-{number}", prefix.to_ascii_uppercase()))?;
    crate::is_canonical_product_code(&code).then_some(code)
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

fn parse_content(value: &JsonValue) -> Option<ParsedItem> {
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
    Some(ParsedItem {
        provider_item_id,
        code,
        title: optional_text(object, "title"),
        cover_url,
    })
}

fn parse_root(
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

fn parse_catalog_document(
    document: &str,
    feed: &str,
    maximum: usize,
) -> Result<Vec<ParsedItem>, DocumentError> {
    let root = parse_root(document)?;
    let Some(JsonValue::Object(data)) = root.get("data") else {
        return Err(DocumentError::Malformed);
    };
    let values = if matches!(feed, "popular" | "newest" | "top-rated") {
        let Some(JsonValue::Object(search)) = data.get("legacySearchPPV") else {
            return Err(DocumentError::Malformed);
        };
        let result = match search.get("result") {
            Some(JsonValue::Object(result)) => result,
            Some(JsonValue::Null) | None => return Ok(Vec::new()),
            _ => return Err(DocumentError::Malformed),
        };
        match result.get("contents") {
            Some(JsonValue::Array(contents)) => contents,
            Some(JsonValue::Null) | None => return Ok(Vec::new()),
            _ => return Err(DocumentError::Malformed),
        }
    } else {
        let Some(JsonValue::Object(ranking)) = data.get("ppvContentRanking") else {
            return Err(DocumentError::Malformed);
        };
        match ranking.get("items") {
            Some(JsonValue::Array(items)) => items,
            Some(JsonValue::Null) | None => return Ok(Vec::new()),
            _ => return Err(DocumentError::Malformed),
        }
    };

    let mut items = Vec::new();
    let mut identities = HashMap::<String, ParsedItem>::new();
    for value in values {
        let content = if matches!(feed, "popular" | "newest" | "top-rated") {
            value
        } else {
            let JsonValue::Object(row) = value else {
                continue;
            };
            let Some(content) = row.get("content") else {
                continue;
            };
            content
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
        if items.len() < maximum {
            items.push(item);
        }
    }
    Ok(items)
}

fn valid_request(request: &FanzaCatalogRequest) -> Option<Category> {
    let category = Category::parse(&request.category)?;
    request
        .context_generation
        .parse::<u64>()
        .ok()
        .filter(|generation| *generation > 0)?;
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

fn set_authority(
    context: &mut CatalogContext,
    category: Category,
    value: Option<CatalogAuthority>,
) {
    match category {
        Category::Adult => context.adult = value,
        Category::Vr => context.vr = value,
    }
}

fn context_generation(context: &CatalogContext, category: Category) -> u64 {
    match category {
        Category::Adult => context.adult_context_generation,
        Category::Vr => context.vr_context_generation,
    }
}

fn set_context_generation(context: &mut CatalogContext, category: Category, generation: u64) {
    match category {
        Category::Adult => context.adult_context_generation = generation,
        Category::Vr => context.vr_context_generation = generation,
    }
}

pub(crate) fn fetch_catalog_with(
    state: &FanzaCatalogState,
    request: &FanzaCatalogRequest,
    fetch: impl FnOnce(&str) -> Result<String, ProviderRequestError>,
) -> Result<Vec<String>, &'static str> {
    let category = Category::parse(&request.category).ok_or(VR_PROVIDER_ERROR)?;
    if valid_request(request).is_none() {
        return Err(category.request_error(ProviderRequestError::Provider));
    }
    let requested_context_generation = request
        .context_generation
        .parse::<u64>()
        .map_err(|_| category.stale())?;
    let request_generation = {
        let mut context = state
            .0
            .lock()
            .map_err(|_| category.request_error(ProviderRequestError::Provider))?;
        if requested_context_generation <= context_generation(&context, category) {
            return Err(category.stale());
        }
        set_context_generation(&mut context, category, requested_context_generation);
        context.request_generation = context
            .request_generation
            .checked_add(1)
            .ok_or_else(|| category.request_error(ProviderRequestError::Provider))?;
        let request_generation = context.request_generation;
        set_authority(
            &mut context,
            category,
            Some(CatalogAuthority {
                context_generation: requested_context_generation,
                request_generation,
                items: Vec::new(),
            }),
        );
        request_generation
    };

    let document = fetch(&graphql_body(category, &request.feed, request.count))
        .map_err(|error| category.request_error(error))?;
    let items = parse_catalog_document(&document, &request.feed, usize::from(request.count))
        .map_err(|error| match error {
            DocumentError::Malformed => category.malformed(),
            DocumentError::Provider => category.request_error(ProviderRequestError::Provider),
            DocumentError::Conflicting => category.conflicting(),
        })?;

    let mut context = state.0.lock().map_err(|_| category.stale())?;
    let current = match category {
        Category::Adult => context.adult.as_mut(),
        Category::Vr => context.vr.as_mut(),
    }
    .filter(|authority| {
        authority.context_generation == requested_context_generation
            && authority.request_generation == request_generation
    })
    .ok_or_else(|| category.stale())?;
    current.items = items
        .into_iter()
        .enumerate()
        .map(|(index, item)| AuthorizedItem {
            provider_item_id: item.provider_item_id,
            code: item.code,
            title: item.title,
            cover: item.cover_url.map(|url| CoverAuthority {
                id: format!("fanza-cover-{request_generation}-{}", index + 1),
                url,
            }),
        })
        .collect();

    let mut response = vec![
        request_generation.to_string(),
        current.items.len().to_string(),
    ];
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
        .filter(|generation| *generation > 0)
        .ok_or_else(|| category.stale())?;
    let mut context = state.0.lock().map_err(|_| category.stale())?;
    if generation <= self::context_generation(&context, category) {
        return Err(category.stale());
    }
    set_context_generation(&mut context, category, generation);
    set_authority(&mut context, category, None);
    Ok(())
}

fn cover_url(
    state: &FanzaCatalogState,
    category: &str,
    context_generation: &str,
    request_generation: &str,
    provider_item_id: &str,
    code: &str,
    cover_authority_id: &str,
) -> Result<(Category, String), &'static str> {
    let category = Category::parse(category).ok_or(VR_PROVIDER_ERROR)?;
    let context_generation = context_generation
        .parse::<u64>()
        .ok()
        .filter(|generation| *generation > 0)
        .ok_or_else(|| category.stale())?;
    let request_generation = request_generation
        .parse::<u64>()
        .ok()
        .filter(|generation| *generation > 0)
        .ok_or_else(|| category.stale())?;
    if !valid_item_id(provider_item_id) || !crate::is_canonical_product_code(code) {
        return Err(category.stale());
    }
    let context = state.0.lock().map_err(|_| category.stale())?;
    let authority = authority(&context, category)
        .filter(|authority| {
            authority.context_generation == context_generation
                && authority.request_generation == request_generation
        })
        .ok_or_else(|| category.stale())?;
    authority
        .items
        .iter()
        .find(|item| item.provider_item_id == provider_item_id && item.code == code)
        .and_then(|item| item.cover.as_ref())
        .filter(|cover| cover.id == cover_authority_id)
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn fetch_cover_with(
    state: &FanzaCatalogState,
    category: &str,
    context_generation: &str,
    request_generation: &str,
    provider_item_id: &str,
    code: &str,
    cover_authority_id: &str,
    fetch: impl FnOnce(&str) -> Result<Vec<u8>, ProviderRequestError>,
) -> Result<Vec<u8>, &'static str> {
    let (category, url) = cover_url(
        state,
        category,
        context_generation,
        request_generation,
        provider_item_id,
        code,
        cover_authority_id,
    )?;
    let bytes = fetch(&url).map_err(|error| category.request_error(error))?;
    if !accepted_raster(&bytes) {
        return Err(category.request_error(ProviderRequestError::Provider));
    }
    if cover_url(
        state,
        category.value(),
        context_generation,
        request_generation,
        provider_item_id,
        code,
        cover_authority_id,
    )?
    .1 != url
    {
        return Err(category.stale());
    }
    Ok(bytes)
}

fn parse_framed_response(output: &[u8], maximum: usize) -> Result<Vec<u8>, ProviderRequestError> {
    let marker = FANZA_HTTP_STATUS_MARKER.as_bytes();
    let marker_position = output
        .windows(marker.len())
        .rposition(|window| window == marker)
        .ok_or(ProviderRequestError::Provider)?;
    let status = std::str::from_utf8(&output[marker_position + marker.len()..])
        .map_err(|_| ProviderRequestError::Provider)?
        .trim()
        .parse::<u16>()
        .map_err(|_| ProviderRequestError::Provider)?;
    let body = &output[..marker_position];
    match status {
        200..=299 if !body.is_empty() && body.len() <= maximum => Ok(body.to_vec()),
        404 | 410 | 451 => Err(ProviderRequestError::SourceUnavailable),
        0 => Err(ProviderRequestError::Network),
        _ => Err(ProviderRequestError::Provider),
    }
}

#[cfg(any(test, target_os = "windows"))]
fn parse_process_response(
    output: &[u8],
    maximum: usize,
    process_succeeded: bool,
) -> Result<Vec<u8>, ProviderRequestError> {
    if !process_succeeded {
        return Err(ProviderRequestError::Network);
    }
    parse_framed_response(output, maximum)
}

#[cfg(any(test, target_os = "macos"))]
fn macos_process_error(exit_code: Option<i32>) -> ProviderRequestError {
    if exit_code == Some(63) {
        ProviderRequestError::Provider
    } else {
        ProviderRequestError::Network
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
            "--max-filesize",
            &FANZA_RESPONSE_MAX_BYTES.to_string(),
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
        return Err(macos_process_error(output.status.code()));
    }
    String::from_utf8(parse_framed_response(
        &output.stdout,
        FANZA_RESPONSE_MAX_BYTES,
    )?)
    .map_err(|_| ProviderRequestError::Provider)
}

#[cfg(target_os = "windows")]
pub(crate) fn fetch_graphql_document(body: &str) -> Result<String, ProviderRequestError> {
    let output = Command::new("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
        .arg(WINDOWS_FANZA_GRAPHQL_SCRIPT)
        .env("FANZA_URL", FANZA_GRAPHQL_URL)
        .env("FANZA_ORIGIN", FANZA_ORIGIN)
        .env("FANZA_REFERER", FANZA_REFERER)
        .env("FANZA_BODY", body)
        .output()
        .map_err(|_| ProviderRequestError::Network)?;
    String::from_utf8(parse_process_response(
        &output.stdout,
        FANZA_RESPONSE_MAX_BYTES,
        output.status.success(),
    )?)
    .map_err(|_| ProviderRequestError::Provider)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn fetch_graphql_document(_body: &str) -> Result<String, ProviderRequestError> {
    Err(ProviderRequestError::SourceUnavailable)
}

#[cfg(target_os = "macos")]
pub(crate) fn fetch_cover_bytes(url: &str) -> Result<Vec<u8>, ProviderRequestError> {
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
            "--max-filesize",
            &FANZA_IMAGE_MAX_BYTES.to_string(),
            "--header",
            &format!("Referer: {FANZA_REFERER}"),
            "--write-out",
            FANZA_HTTP_STATUS_WRITE_OUT,
            url,
        ])
        .output()
        .map_err(|_| ProviderRequestError::Network)?;
    if !output.status.success() {
        return Err(macos_process_error(output.status.code()));
    }
    parse_framed_response(&output.stdout, FANZA_IMAGE_MAX_BYTES)
}

#[cfg(target_os = "windows")]
pub(crate) fn fetch_cover_bytes(url: &str) -> Result<Vec<u8>, ProviderRequestError> {
    if !valid_https_image_url(url) {
        return Err(ProviderRequestError::Provider);
    }
    let output = Command::new("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
        .arg(WINDOWS_FANZA_IMAGE_SCRIPT)
        .env("FANZA_IMAGE", url)
        .env("FANZA_REFERER", FANZA_REFERER)
        .output()
        .map_err(|_| ProviderRequestError::Network)?;
    parse_windows_image_response(&output.stdout, output.status.success())
}

#[cfg(any(test, target_os = "windows"))]
fn parse_windows_image_response(
    output: &[u8],
    process_succeeded: bool,
) -> Result<Vec<u8>, ProviderRequestError> {
    let encoded_maximum = FANZA_IMAGE_MAX_BYTES.div_ceil(3) * 4;
    let encoded = parse_process_response(output, encoded_maximum, process_succeeded)?;
    let bytes = crate::javdb_catalog::decode_base64(
        std::str::from_utf8(&encoded).map_err(|_| ProviderRequestError::Provider)?,
    )
    .ok_or(ProviderRequestError::Provider)?;
    if bytes.len() > FANZA_IMAGE_MAX_BYTES {
        return Err(ProviderRequestError::Provider);
    }
    Ok(bytes)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn fetch_cover_bytes(_url: &str) -> Result<Vec<u8>, ProviderRequestError> {
    Err(ProviderRequestError::SourceUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog_document(feed: &str, contents: &str) -> String {
        if matches!(feed, "popular" | "newest" | "top-rated") {
            format!(r#"{{"data":{{"legacySearchPPV":{{"result":{{"contents":[{contents}]}}}}}}}}"#)
        } else {
            format!(r#"{{"data":{{"ppvContentRanking":{{"items":[{contents}]}}}}}}"#)
        }
    }

    fn request(category: &str, context_generation: u64) -> FanzaCatalogRequest {
        FanzaCatalogRequest {
            category: category.to_owned(),
            context_generation: context_generation.to_string(),
            feed: "popular".to_owned(),
            count: 10,
        }
    }

    #[test]
    fn maps_exact_content_ids_and_rejects_digit_ending_prefixes() {
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
        assert_eq!(code_from_content_id("ab1_2"), None);
    }

    #[test]
    fn maps_every_feed_to_the_exact_operation_and_category() {
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
            }
        }
    }

    #[test]
    fn isolates_bad_rows_preserves_order_and_rejects_conflicting_duplicates() {
        let valid = r#"{"id":"vrkm01577","title":"First","packageImage":{"largeUrl":"https://awsimgsrc.dmm.co.jp/path/pl.jpg"}}"#;
        let second = r#"{"id":"ovvr616","title":"Second"}"#;
        let document = catalog_document(
            "popular",
            &format!("{valid},{valid},{{\"id\":\"BAD-ID\"}},{second}"),
        );
        let parsed = parse_catalog_document(&document, "popular", 10).unwrap();
        assert_eq!(
            parsed
                .iter()
                .map(|item| item.code.as_str())
                .collect::<Vec<_>>(),
            ["VRKM-1577", "OVVR-616"]
        );
        assert_eq!(
            parsed[0].cover_url.as_deref(),
            Some("https://awsimgsrc.dmm.co.jp/path/ps.jpg")
        );
        let conflict = catalog_document(
            "popular",
            &format!("{valid},{{\"id\":\"vrkm01577\",\"title\":\"Different\"}}"),
        );
        assert_eq!(
            parse_catalog_document(&conflict, "popular", 10),
            Err(DocumentError::Conflicting)
        );
    }

    #[test]
    fn requires_ranking_content_rows_and_treats_documented_null_collections_as_empty() {
        let ranking = catalog_document(
            "trending",
            r#"{"content":{"id":"vrkm01577"}},{"id":"wrapper"},{"content":{"id":"ovvr616"}}"#,
        );
        let parsed = parse_catalog_document(&ranking, "trending", 10).unwrap();
        assert_eq!(
            parsed
                .iter()
                .map(|item| item.code.as_str())
                .collect::<Vec<_>>(),
            ["VRKM-1577", "OVVR-616"]
        );
        for (document, feed) in [
            (r#"{"data":{"legacySearchPPV":{"result":null}}}"#, "popular"),
            (
                r#"{"data":{"legacySearchPPV":{"result":{"contents":null}}}}"#,
                "popular",
            ),
            (r#"{"data":{"legacySearchPPV":{"result":{}}}}"#, "popular"),
            (
                r#"{"data":{"ppvContentRanking":{"items":null}}}"#,
                "trending",
            ),
            (r#"{"data":{"ppvContentRanking":{}}}"#, "monthly"),
        ] {
            assert!(parse_catalog_document(document, feed, 10)
                .unwrap()
                .is_empty());
        }
    }

    #[test]
    fn retains_the_exact_requested_count_and_binds_cover_authority() {
        let contents = (1..=11)
            .map(|number| {
                format!(
                    r#"{{"id":"ovvr{number}","packageImage":{{"largeUrl":"https://awsimgsrc.dmm.co.jp/{number}/pl.jpg"}}}}"#
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let state = FanzaCatalogState::default();
        let response = fetch_catalog_with(&state, &request("vr", 1), |_| {
            Ok(catalog_document("popular", &contents))
        })
        .unwrap();
        assert_eq!(response[1], "10");
        assert!(!response.iter().any(|field| field == "OVVR-11"));

        let bytes = vec![0xff, 0xd8, 0xff, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let dispatched = std::cell::Cell::new(false);
        assert_eq!(
            fetch_cover_with(
                &state,
                "vr",
                "1",
                &response[0],
                "ovvr1",
                "OVVR-1",
                &response[6],
                |url| {
                    dispatched.set(true);
                    assert_eq!(url, "https://awsimgsrc.dmm.co.jp/1/ps.jpg");
                    Ok(bytes.clone())
                },
            ),
            Ok(bytes)
        );
        assert!(dispatched.get());
        assert!(fetch_cover_with(
            &state,
            "vr",
            "1",
            &response[0],
            "ovvr2",
            "OVVR-2",
            &response[6],
            |_| panic!("cross-item cover dispatched"),
        )
        .is_err());
        assert!(fetch_cover_with(
            &state,
            "adult",
            "1",
            &response[0],
            "ovvr1",
            "OVVR-1",
            &response[6],
            |_| panic!("cross-category cover dispatched"),
        )
        .is_err());
    }

    #[test]
    fn stale_catalog_completion_and_invalidated_cover_do_not_replace_current_authority() {
        let state = FanzaCatalogState::default();
        let current = fetch_catalog_with(&state, &request("vr", 2), |_| {
            Ok(catalog_document(
                "popular",
                r#"{"id":"ovvr616","packageImage":{"largeUrl":"https://awsimgsrc.dmm.co.jp/current/pl.jpg"}}"#,
            ))
        })
        .unwrap();
        assert_eq!(
            fetch_catalog_with(&state, &request("vr", 1), |_| {
                panic!("stale request dispatched")
            }),
            Err(VR_FANZA_STALE)
        );
        invalidate_catalog(&state, "vr", "3").unwrap();
        assert!(fetch_cover_with(
            &state,
            "vr",
            "2",
            &current[0],
            "ovvr616",
            "OVVR-616",
            &current[6],
            |_| panic!("invalidated cover dispatched"),
        )
        .is_err());
    }

    #[test]
    fn late_catalog_completion_cannot_replace_a_newer_catalog_or_cover_authority() {
        let state = FanzaCatalogState::default();
        let started = Arc::new(std::sync::Barrier::new(2));
        let release = Arc::new(std::sync::Barrier::new(2));
        let late_state = state.clone();
        let late_started = started.clone();
        let late_release = release.clone();
        let late = std::thread::spawn(move || {
            fetch_catalog_with(&late_state, &request("vr", 1), |_| {
                late_started.wait();
                late_release.wait();
                Ok(catalog_document(
                    "popular",
                    r#"{"id":"vrkm01577","packageImage":{"largeUrl":"https://awsimgsrc.dmm.co.jp/old/pl.jpg"}}"#,
                ))
            })
        });
        started.wait();
        let current = fetch_catalog_with(&state, &request("vr", 2), |_| {
            Ok(catalog_document(
                "popular",
                r#"{"id":"ovvr616","packageImage":{"largeUrl":"https://awsimgsrc.dmm.co.jp/current/pl.jpg"}}"#,
            ))
        })
        .unwrap();
        release.wait();
        assert_eq!(late.join().unwrap(), Err(VR_FANZA_STALE));

        assert!(fetch_cover_with(
            &state,
            "vr",
            "1",
            "1",
            "vrkm01577",
            "VRKM-1577",
            "fanza-cover-1-1",
            |_| panic!("late cover dispatched"),
        )
        .is_err());
        assert!(fetch_cover_with(
            &state,
            "vr",
            "2",
            &current[0],
            "ovvr616",
            "OVVR-616",
            &current[6],
            |url| {
                assert_eq!(url, "https://awsimgsrc.dmm.co.jp/current/ps.jpg");
                Ok(vec![0xff, 0xd8, 0xff, 0, 0, 0, 0, 0, 0, 0, 0, 0])
            },
        )
        .is_ok());
    }

    #[test]
    fn failed_catalog_request_revokes_previous_cover_and_allows_exact_retry() {
        let state = FanzaCatalogState::default();
        let first = fetch_catalog_with(&state, &request("vr", 1), |_| {
            Ok(catalog_document(
                "popular",
                r#"{"id":"vrkm01577","packageImage":{"largeUrl":"https://awsimgsrc.dmm.co.jp/old/pl.jpg"}}"#,
            ))
        })
        .unwrap();
        assert_eq!(
            fetch_catalog_with(&state, &request("vr", 2), |_| {
                Err(ProviderRequestError::Network)
            }),
            Err(VR_NETWORK_ERROR)
        );
        assert!(fetch_cover_with(
            &state,
            "vr",
            "1",
            &first[0],
            "vrkm01577",
            "VRKM-1577",
            &first[6],
            |_| panic!("failed request left old cover authority"),
        )
        .is_err());
        assert!(fetch_catalog_with(&state, &request("vr", 3), |_| {
            Ok(catalog_document("popular", r#"{"id":"ovvr616"}"#))
        })
        .is_ok());
    }

    #[test]
    fn windows_transport_uses_one_deadline_for_headers_and_stalled_body_reads() {
        for script in [WINDOWS_FANZA_GRAPHQL_SCRIPT, WINDOWS_FANZA_IMAGE_SCRIPT] {
            assert_eq!(script.matches("CancellationTokenSource]::new()").count(), 1);
            assert_eq!(script.matches("$deadline.CancelAfter(20000)").count(), 1);
            assert_eq!(script.matches("$deadline.Token").count(), 2);
            assert!(script.contains("ResponseHeadersRead, $deadline.Token"));
            assert!(
                script.contains("$stream.ReadAsync($buffer, 0, $buffer.Length, $deadline.Token)")
            );
            assert!(!script.contains("$stream.Read($buffer"));
        }
    }

    #[test]
    fn validates_transport_framing_unicode_statuses_bounds_and_exact_image_host() {
        let unicode = "{\"data\":{\"title\":\"日本語の作品\"}}";
        let framed = format!("{unicode}\nAUTO_VIDEO_HTTP_STATUS:200");
        assert_eq!(
            String::from_utf8(parse_framed_response(framed.as_bytes(), 1024).unwrap()).unwrap(),
            unicode
        );
        assert_eq!(
            parse_process_response(b"body\nAUTO_VIDEO_HTTP_STATUS:200", 1024, false),
            Err(ProviderRequestError::Network)
        );
        assert_eq!(
            parse_windows_image_response(b"AP8QgA==\nAUTO_VIDEO_HTTP_STATUS:200", true),
            Ok(vec![0x00, 0xff, 0x10, 0x80])
        );
        assert_eq!(
            parse_framed_response(b"four\nAUTO_VIDEO_HTTP_STATUS:200", 3),
            Err(ProviderRequestError::Provider)
        );
        assert_eq!(
            parse_framed_response(b"four\nAUTO_VIDEO_HTTP_STATUS:200", 4),
            Ok(b"four".to_vec())
        );
        for (status, expected) in [
            ("404", ProviderRequestError::SourceUnavailable),
            ("410", ProviderRequestError::SourceUnavailable),
            ("451", ProviderRequestError::SourceUnavailable),
            ("0", ProviderRequestError::Network),
            ("500", ProviderRequestError::Provider),
        ] {
            assert_eq!(
                parse_framed_response(
                    format!("body\nAUTO_VIDEO_HTTP_STATUS:{status}").as_bytes(),
                    1024,
                ),
                Err(expected)
            );
        }
        for url in [
            "http://awsimgsrc.dmm.co.jp/image.jpg",
            "https://user@awsimgsrc.dmm.co.jp/image.jpg",
            "https://awsimgsrc.dmm.co.jp:443/image.jpg",
            "https://awsimgsrc.dmm.co.jp.evil.example/image.jpg",
            "https://awsimgsrc.dmm.co.jp\\image.jpg",
        ] {
            assert!(!valid_https_image_url(url));
        }
        assert!(valid_https_image_url(
            "https://awsimgsrc.dmm.co.jp/dig/digital/video/vrkm01577/ps.jpg"
        ));
        for script in [WINDOWS_FANZA_GRAPHQL_SCRIPT, WINDOWS_FANZA_IMAGE_SCRIPT] {
            assert!(script.contains("$ErrorActionPreference = 'Stop'"));
            assert!(script.contains("$ProgressPreference = 'SilentlyContinue'"));
            assert!(script.contains("AllowAutoRedirect = $false"));
            assert!(script.contains("ResponseHeadersRead"));
            assert!(script.contains("[Console]::OutputEncoding = [System.Text.Encoding]::UTF8"));
            assert!(!script.contains("Cookie"));
            assert!(!script.contains("Authorization"));
        }
        assert!(WINDOWS_FANZA_GRAPHQL_SCRIPT.contains("'Origin', $env:FANZA_ORIGIN"));
        assert!(WINDOWS_FANZA_GRAPHQL_SCRIPT.contains("Referrer = [Uri]$env:FANZA_REFERER"));
        assert!(WINDOWS_FANZA_GRAPHQL_SCRIPT.contains("-gt 4194304"));
        assert!(WINDOWS_FANZA_IMAGE_SCRIPT.contains("ToBase64String($memory.ToArray())"));
        assert!(WINDOWS_FANZA_IMAGE_SCRIPT.contains("-gt 16777216"));
        assert_eq!(
            macos_process_error(Some(63)),
            ProviderRequestError::Provider
        );
        assert_eq!(macos_process_error(Some(28)), ProviderRequestError::Network);
    }
}
