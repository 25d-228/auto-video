use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::process::Command;

use crate::{
    vr_torrent::{
        canonical_product_code, product_code_display_form, product_code_forms, JsonParser,
        JsonValue,
    },
    ProviderRequestError, ADULT_NETWORK_ERROR, ADULT_PROVIDER_ERROR, ADULT_SOURCE_UNAVAILABLE,
    VR_NETWORK_ERROR, VR_PROVIDER_ERROR, VR_SOURCE_UNAVAILABLE,
};

const GRAPHQL_URL: &str = "https://api.video.dmm.co.jp/graphql";
const ORIGIN: &str = "https://video.dmm.co.jp";
const REFERER: &str = "https://video.dmm.co.jp/";
const IMAGE_HOST: &str = "awsimgsrc.dmm.co.jp";
const RESPONSE_MAX_BYTES: usize = 4 * 1024 * 1024;
const IMAGE_MAX_BYTES: usize = 16 * 1024 * 1024;
const STATUS_MARKER: &str = "\nAUTO_VIDEO_HTTP_STATUS:";
#[cfg(target_os = "macos")]
const STATUS_WRITE_OUT: &str = "\nAUTO_VIDEO_HTTP_STATUS:%{http_code}";

#[cfg(any(test, target_os = "windows"))]
const WINDOWS_GRAPHQL_SCRIPT: &str = r#"$ErrorActionPreference = 'Stop'
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
    $bytes = $memory.ToArray()
    $output.Write($bytes, 0, $bytes.Length)
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
const WINDOWS_IMAGE_SCRIPT: &str = r#"$ErrorActionPreference = 'Stop'
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

pub(crate) const ADULT_MALFORMED: &str = "adult_fanza_malformed_provider";
pub(crate) const ADULT_CONFLICTING: &str = "adult_fanza_conflicting_provider";
pub(crate) const ADULT_STALE: &str = "adult_fanza_stale";
pub(crate) const VR_MALFORMED: &str = "vr_fanza_malformed_provider";
pub(crate) const VR_CONFLICTING: &str = "vr_fanza_conflicting_provider";
pub(crate) const VR_STALE: &str = "vr_fanza_stale";

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
            Self::Adult => ADULT_MALFORMED,
            Self::Vr => VR_MALFORMED,
        }
    }

    fn conflicting(self) -> &'static str {
        match self {
            Self::Adult => ADULT_CONFLICTING,
            Self::Vr => VR_CONFLICTING,
        }
    }

    fn stale(self) -> &'static str {
        match self {
            Self::Adult => ADULT_STALE,
            Self::Vr => VR_STALE,
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
    content_id: String,
    display_code: String,
    title: Option<String>,
    cover_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExactLibraryItem {
    pub content_id: String,
    pub display_code: String,
    pub title: Option<String>,
    pub cover_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AuthorizedItem {
    content_id: String,
    display_code: String,
    title: Option<String>,
    cover: Option<(String, String)>,
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

fn valid_content_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn valid_display_code(value: &str) -> bool {
    product_code_display_form(value).as_deref() == Some(value)
}

fn display_code_from_content_id(value: &str) -> Option<String> {
    if !valid_content_id(value) {
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
    let number = content_id[number_start..].parse::<u64>().ok()?;
    if number == 0 {
        return None;
    }
    let prefix = prefix.to_ascii_uppercase();
    let code = match prefix.as_str() {
        "CAWB" => format!("CAWB-{number:03}"),
        "3DSVR" => format!("3DSVR-{number:05}"),
        _ => format!("{prefix}-{number}"),
    };
    valid_display_code(&code).then_some(code)
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

fn valid_image_url(value: &str) -> bool {
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
    authority == IMAGE_HOST
        && !authority.contains('@')
        && !authority.contains(':')
        && rest.len() > authority.len() + 1
}

fn package_cover_url(value: &str) -> Option<String> {
    if !valid_image_url(value) {
        return None;
    }
    Some(if let Some(prefix) = value.strip_suffix("pl.jpg") {
        format!("{prefix}ps.jpg")
    } else {
        value.to_owned()
    })
}

fn package_image_matches_content_id(value: &str, content_id: &str) -> bool {
    let Some(path) = value.strip_prefix("https://awsimgsrc.dmm.co.jp/") else {
        return false;
    };
    if path.contains(['?', '#']) {
        return false;
    }
    let mut components = path.rsplit('/');
    let filename = components.next().unwrap_or_default();
    let directory = components.next().unwrap_or_default();
    directory == content_id
        && (filename == format!("{content_id}pl.jpg") || filename == format!("{content_id}ps.jpg"))
}

pub(crate) fn valid_exact_library_cover_url(value: &str, content_id: &str) -> bool {
    valid_content_id(content_id)
        && valid_image_url(value)
        && package_image_matches_content_id(value, content_id)
}

fn parse_exact_cover_url(
    content: &std::collections::BTreeMap<String, JsonValue>,
    content_id: &str,
) -> Result<Option<String>, DocumentError> {
    let image = match content.get("packageImage") {
        None | Some(JsonValue::Null) => return Ok(None),
        Some(JsonValue::Object(image)) => image,
        Some(_) => return Err(DocumentError::Malformed),
    };
    let url = match image.get("largeUrl") {
        None | Some(JsonValue::Null) => return Ok(None),
        Some(JsonValue::String(url)) => url,
        Some(_) => return Err(DocumentError::Malformed),
    };
    if !valid_exact_library_cover_url(url, content_id) {
        return Err(DocumentError::Conflicting);
    }
    package_cover_url(url)
        .map(Some)
        .ok_or(DocumentError::Malformed)
}

fn parse_content(value: &JsonValue) -> Option<ParsedItem> {
    let JsonValue::Object(object) = value else {
        return None;
    };
    let content_id = optional_text(object, "id")?;
    let display_code = display_code_from_content_id(&content_id)?;
    let cover_url = match object.get("packageImage") {
        Some(JsonValue::Object(image)) => {
            optional_text(image, "largeUrl").and_then(|url| package_cover_url(&url))
        }
        _ => None,
    };
    Some(ParsedItem {
        content_id,
        display_code,
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

fn parse_catalog(
    document: &str,
    feed: &str,
    maximum: usize,
) -> Result<Vec<ParsedItem>, DocumentError> {
    let root = parse_root(document)?;
    let Some(JsonValue::Object(data)) = root.get("data") else {
        return Err(DocumentError::Malformed);
    };
    let search_feed = matches!(feed, "popular" | "newest" | "top-rated");
    let values = if search_feed {
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
        let content = if search_feed {
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
        if let Some(previous) = identities.get(&item.content_id) {
            if previous != &item {
                return Err(DocumentError::Conflicting);
            }
            continue;
        }
        identities.insert(item.content_id.clone(), item.clone());
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct ExactContentRequest {
    alias: String,
    content_id: String,
}

fn exact_content_id_candidates(code: &str) -> Vec<String> {
    let Some(forms) = product_code_forms(code) else {
        return Vec::new();
    };
    if forms.display != code {
        return Vec::new();
    }
    let number = forms
        .identity
        .split_once('-')
        .map(|(_, number)| number)
        .unwrap_or_default();
    if forms.prefix == "CAWB" {
        return vec![format!("cawb{number:0>5}")];
    }
    if forms.prefix == "3DSVR" {
        return vec![format!("13dsvr{number:0>5}")];
    }
    Vec::new()
}

fn exact_content_requests(code: &str) -> Vec<ExactContentRequest> {
    exact_content_id_candidates(code)
        .into_iter()
        .enumerate()
        .map(|(index, content_id)| ExactContentRequest {
            alias: format!("c{index}"),
            content_id,
        })
        .collect()
}

fn exact_content_body(requests: &[ExactContentRequest]) -> String {
    let fields = requests
        .iter()
        .map(|request| {
            format!(
                "{}:ppvContent(id:\"{}\"){{id contentType title packageImage{{largeUrl}}}}",
                request.alias, request.content_id
            )
        })
        .collect::<String>();
    format!(r#"{{"query":"query{{{fields}}}","variables":{{}}}}"#)
}

fn parse_exact_content(
    document: &str,
    category: Category,
    code: &str,
    requests: &[ExactContentRequest],
) -> Result<Option<ExactLibraryItem>, DocumentError> {
    let root = parse_root(document)?;
    let Some(JsonValue::Object(data)) = root.get("data") else {
        return Err(DocumentError::Malformed);
    };
    if data
        .keys()
        .any(|alias| !requests.iter().any(|request| request.alias == *alias))
    {
        return Err(DocumentError::Malformed);
    }
    let mut accepted = Vec::new();
    for request in requests {
        let Some(value) = data.get(&request.alias) else {
            continue;
        };
        let JsonValue::Object(content) = value else {
            if matches!(value, JsonValue::Null) {
                continue;
            }
            return Err(DocumentError::Malformed);
        };
        let content_id = match content.get("id") {
            Some(JsonValue::String(content_id)) if valid_content_id(content_id) => content_id,
            _ => return Err(DocumentError::Malformed),
        };
        let content_type = match content.get("contentType") {
            Some(JsonValue::String(content_type)) => content_type,
            _ => return Err(DocumentError::Malformed),
        };
        if content_id != &request.content_id || content_type != category.content_type() {
            return Err(DocumentError::Conflicting);
        }
        let display_code =
            display_code_from_content_id(content_id).ok_or(DocumentError::Malformed)?;
        if canonical_product_code(&display_code) != canonical_product_code(code) {
            return Err(DocumentError::Conflicting);
        }
        let cover_url = parse_exact_cover_url(content, content_id)?;
        accepted.push(ExactLibraryItem {
            content_id: content_id.clone(),
            display_code,
            title: optional_text(content, "title"),
            cover_url,
        });
    }
    accepted.sort_by(|left, right| left.content_id.cmp(&right.content_id));
    accepted.dedup();
    match accepted.len() {
        0 => Ok(None),
        1 => Ok(accepted.pop()),
        _ => Err(DocumentError::Conflicting),
    }
}

pub(crate) fn fetch_exact_library_item_with(
    category: &str,
    code: &str,
    fetch: impl FnOnce(&str) -> Result<String, ProviderRequestError>,
) -> Result<Option<ExactLibraryItem>, ProviderRequestError> {
    let category = Category::parse(category).ok_or(ProviderRequestError::Provider)?;
    product_code_forms(code).ok_or(ProviderRequestError::Provider)?;
    let requests = exact_content_requests(code);
    if requests.is_empty() {
        return Ok(None);
    }
    let body = exact_content_body(&requests);
    let document = fetch(&body)?;
    parse_exact_content(&document, category, code, &requests)
        .map_err(|_| ProviderRequestError::Provider)
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

fn current_context_generation(context: &CatalogContext, category: Category) -> u64 {
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
    valid_request(request).ok_or_else(|| category.request_error(ProviderRequestError::Provider))?;
    let requested_context_generation = request
        .context_generation
        .parse::<u64>()
        .map_err(|_| category.stale())?;
    let request_generation = {
        let mut context = state
            .0
            .lock()
            .map_err(|_| category.request_error(ProviderRequestError::Provider))?;
        if requested_context_generation <= current_context_generation(&context, category) {
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
    let items =
        parse_catalog(&document, &request.feed, usize::from(request.count)).map_err(|error| {
            match error {
                DocumentError::Malformed => category.malformed(),
                DocumentError::Provider => category.request_error(ProviderRequestError::Provider),
                DocumentError::Conflicting => category.conflicting(),
            }
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
            content_id: item.content_id,
            display_code: item.display_code,
            title: item.title,
            cover: item.cover_url.map(|url| {
                (
                    format!("fanza-cover-{request_generation}-{}", index + 1),
                    url,
                )
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
            item.content_id.clone(),
            item.display_code.clone(),
            item.title.clone().unwrap_or_default(),
            item.cover
                .as_ref()
                .map(|(id, _)| id.clone())
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
    if generation <= current_context_generation(&context, category) {
        return Err(category.stale());
    }
    set_context_generation(&mut context, category, generation);
    set_authority(&mut context, category, None);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cover_url(
    state: &FanzaCatalogState,
    category: &str,
    context_generation: &str,
    request_generation: &str,
    content_id: &str,
    display_code: &str,
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
    if !valid_content_id(content_id) || !valid_display_code(display_code) {
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
        .find(|item| item.content_id == content_id && item.display_code == display_code)
        .and_then(|item| item.cover.as_ref())
        .filter(|(id, _)| id == cover_authority_id)
        .map(|(_, url)| (category, url.clone()))
        .ok_or_else(|| category.stale())
}

fn accepted_raster(bytes: &[u8]) -> bool {
    bytes.len() >= 12
        && bytes.len() <= IMAGE_MAX_BYTES
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
    content_id: &str,
    display_code: &str,
    cover_authority_id: &str,
    fetch: impl FnOnce(&str) -> Result<Vec<u8>, ProviderRequestError>,
) -> Result<Vec<u8>, &'static str> {
    let (category, url) = cover_url(
        state,
        category,
        context_generation,
        request_generation,
        content_id,
        display_code,
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
        content_id,
        display_code,
        cover_authority_id,
    )?
    .1 != url
    {
        return Err(category.stale());
    }
    Ok(bytes)
}

fn parse_framed_response(output: &[u8], maximum: usize) -> Result<Vec<u8>, ProviderRequestError> {
    let marker = STATUS_MARKER.as_bytes();
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

#[cfg(target_os = "macos")]
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
            &RESPONSE_MAX_BYTES.to_string(),
            "--request",
            "POST",
            "--header",
            "Accept: application/json",
            "--header",
            "Content-Type: application/json",
            "--header",
            &format!("Origin: {ORIGIN}"),
            "--header",
            &format!("Referer: {REFERER}"),
            "--data-binary",
            body,
            "--write-out",
            STATUS_WRITE_OUT,
            GRAPHQL_URL,
        ])
        .output()
        .map_err(|_| ProviderRequestError::Network)?;
    if !output.status.success() {
        return Err(macos_process_error(output.status.code()));
    }
    String::from_utf8(parse_framed_response(&output.stdout, RESPONSE_MAX_BYTES)?)
        .map_err(|_| ProviderRequestError::Provider)
}

#[cfg(target_os = "windows")]
pub(crate) fn fetch_graphql_document(body: &str) -> Result<String, ProviderRequestError> {
    let output = Command::new("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
        .arg(WINDOWS_GRAPHQL_SCRIPT)
        .env("FANZA_URL", GRAPHQL_URL)
        .env("FANZA_ORIGIN", ORIGIN)
        .env("FANZA_REFERER", REFERER)
        .env("FANZA_BODY", body)
        .output()
        .map_err(|_| ProviderRequestError::Network)?;
    String::from_utf8(parse_process_response(
        &output.stdout,
        RESPONSE_MAX_BYTES,
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
    if !valid_image_url(url) {
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
            &IMAGE_MAX_BYTES.to_string(),
            "--header",
            &format!("Referer: {REFERER}"),
            "--write-out",
            STATUS_WRITE_OUT,
            url,
        ])
        .output()
        .map_err(|_| ProviderRequestError::Network)?;
    if !output.status.success() {
        return Err(macos_process_error(output.status.code()));
    }
    parse_framed_response(&output.stdout, IMAGE_MAX_BYTES)
}

#[cfg(any(test, target_os = "windows"))]
fn decode_base64(value: &str) -> Option<Vec<u8>> {
    if !value.len().is_multiple_of(4) {
        return None;
    }
    let mut output = Vec::with_capacity(value.len() / 4 * 3);
    for chunk in value.as_bytes().chunks_exact(4) {
        let decode = |byte| match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        };
        let a = decode(chunk[0])?;
        let b = decode(chunk[1])?;
        output.push((a << 2) | (b >> 4));
        if chunk[2] != b'=' {
            let c = decode(chunk[2])?;
            output.push((b << 4) | (c >> 2));
            if chunk[3] != b'=' {
                let d = decode(chunk[3])?;
                output.push((c << 6) | d);
            }
        }
    }
    Some(output)
}

#[cfg(target_os = "windows")]
pub(crate) fn fetch_cover_bytes(url: &str) -> Result<Vec<u8>, ProviderRequestError> {
    if !valid_image_url(url) {
        return Err(ProviderRequestError::Provider);
    }
    let output = Command::new("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
        .arg(WINDOWS_IMAGE_SCRIPT)
        .env("FANZA_IMAGE", url)
        .env("FANZA_REFERER", REFERER)
        .output()
        .map_err(|_| ProviderRequestError::Network)?;
    parse_windows_image_response(&output.stdout, output.status.success())
}

#[cfg(any(test, target_os = "windows"))]
fn parse_windows_image_response(
    output: &[u8],
    process_succeeded: bool,
) -> Result<Vec<u8>, ProviderRequestError> {
    let encoded_maximum = IMAGE_MAX_BYTES.div_ceil(3) * 4;
    let encoded = parse_process_response(output, encoded_maximum, process_succeeded)?;
    let bytes =
        decode_base64(std::str::from_utf8(&encoded).map_err(|_| ProviderRequestError::Provider)?)
            .ok_or(ProviderRequestError::Provider)?;
    (bytes.len() <= IMAGE_MAX_BYTES)
        .then_some(bytes)
        .ok_or(ProviderRequestError::Provider)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn fetch_cover_bytes(_url: &str) -> Result<Vec<u8>, ProviderRequestError> {
    Err(ProviderRequestError::SourceUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(category: &str, generation: u64) -> FanzaCatalogRequest {
        FanzaCatalogRequest {
            category: category.to_owned(),
            context_generation: generation.to_string(),
            feed: "popular".to_owned(),
            count: 10,
        }
    }

    fn search_document(contents: &str) -> String {
        format!(r#"{{"data":{{"legacySearchPPV":{{"result":{{"contents":[{contents}]}}}}}}}}"#)
    }

    #[test]
    fn exact_library_content_requires_the_requested_transport_code_and_category() {
        let accepted = fetch_exact_library_item_with("vr", "3DSVR-01871", |body| {
            assert!(body.contains("ppvContent"));
            assert!(body.contains("c0:ppvContent(id:\"13dsvr01871\")"));
            Ok(r#"{"data":{"c0":{"id":"13dsvr01871","contentType":"VR","title":"Exact VR","packageImage":{"largeUrl":"https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/13dsvr01871/13dsvr01871pl.jpg"}}}}"#.to_owned())
        })
        .expect("the exact FANZA response must parse")
        .expect("the exact item must be retained");
        assert_eq!(accepted.content_id, "13dsvr01871");
        assert_eq!(accepted.title.as_deref(), Some("Exact VR"));
        assert!(accepted
            .cover_url
            .as_deref()
            .is_some_and(|url| url.ends_with("ps.jpg")));

        let cawb = fetch_exact_library_item_with("adult", "CAWB-1", |body| {
            assert!(body.contains("c0:ppvContent(id:\"cawb00001\")"));
            assert!(!body.contains("cawb001"));
            Ok(r#"{"data":{"c0":{"id":"cawb00001","contentType":"TWO_DIMENSION","title":"Exact Adult","packageImage":{"largeUrl":"https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/cawb00001/cawb00001pl.jpg"}}}}"#.to_owned())
        })
        .expect("the exact FANZA response must parse")
        .expect("the exact CAWB item must be retained");
        assert_eq!(cawb.content_id, "cawb00001");
        assert_eq!(cawb.display_code, "CAWB-001");

        for document in [
            r#"{"data":{"c0":{"id":"cawb001","contentType":"TWO_DIMENSION","title":"Wrong transport","packageImage":null}}}"#,
            r#"{"data":{"c0":{"id":"cawb00002","contentType":"TWO_DIMENSION","title":"Wrong code","packageImage":null}}}"#,
            r#"{"data":{"c0":{"id":"cawb00001","contentType":"VR","title":"Wrong category","packageImage":null}}}"#,
            r#"{"data":{"c0":{"id":" cawb00001 ","contentType":"TWO_DIMENSION","title":"Padded transport","packageImage":null}}}"#,
            r#"{"data":{"c0":{"id":"cawb00001\u0000","contentType":"TWO_DIMENSION","title":"Controlled transport","packageImage":null}}}"#,
            r#"{"data":{"c0":{"id":"cawb00001","contentType":" TWO_DIMENSION ","title":"Padded category","packageImage":null}}}"#,
            r#"{"data":{"c0":{"id":"cawb00001","contentType":"TWO_DIMENSION\u0000","title":"Controlled category","packageImage":null}}}"#,
            r#"{"data":{"c0":{"id":"unsafe-id","contentType":"TWO_DIMENSION","title":"Malformed identity","packageImage":null}}}"#,
            r#"{"data":{"c0":{"contentType":"TWO_DIMENSION","title":"Missing identity","packageImage":null}}}"#,
            r#"{"data":{"c1":{"id":"cawb00001","contentType":"TWO_DIMENSION","title":"Unrequested alias","packageImage":null}}}"#,
            r#"{"data":{"c0":{"id":"cawb00001","contentType":"TWO_DIMENSION","title":"Malformed image","packageImage":[]}}}"#,
            r#"{"data":{"c0":{"id":"cawb00001","contentType":"TWO_DIMENSION","title":"Malformed image","packageImage":"missing"}}}"#,
            r#"{"data":{"c0":{"id":"cawb00001","contentType":"TWO_DIMENSION","title":"Malformed URL","packageImage":{"largeUrl":42}}}}"#,
            r#"{"data":{"c0":{"id":"cawb00001","contentType":"TWO_DIMENSION","title":"Padded URL","packageImage":{"largeUrl":" https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/cawb00001/cawb00001pl.jpg "}}}}"#,
            r#"{"data":{"c0":{"id":"cawb00001","contentType":"TWO_DIMENSION","title":"Controlled URL","packageImage":{"largeUrl":"https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/cawb00001/cawb00001pl.jpg\u0000"}}}}"#,
            r#"{"data":{"c0":{"id":"cawb00001","contentType":"TWO_DIMENSION","title":"Neighbor image","packageImage":{"largeUrl":"https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/cawb00002/cawb00002pl.jpg"}}}}"#,
        ] {
            assert_eq!(
                fetch_exact_library_item_with("adult", "CAWB-1", |_| Ok(document.to_owned())),
                Err(ProviderRequestError::Provider)
            );
        }

        assert_eq!(
            fetch_exact_library_item_with("vr", "3DSVR-01871", |_| {
                Ok(r#"{"data":{"c0":{"id":"13dsvr01871","contentType":"VR","title":"Neighbor image","packageImage":{"largeUrl":"https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/13dsvr01872/13dsvr01872pl.jpg"}}}}"#.to_owned())
            }),
            Err(ProviderRequestError::Provider)
        );

        assert_eq!(
            fetch_exact_library_item_with("adult", "CAWB-1", |_| {
                Ok(r#"{"data":{"c0":null}}"#.to_owned())
            }),
            Ok(None)
        );
        assert_eq!(
            fetch_exact_library_item_with("adult", "CAWB-1", |_| {
                Ok(r#"{"data":{}}"#.to_owned())
            }),
            Ok(None)
        );

        for cover in [
            "",
            r#","packageImage":null"#,
            r#","packageImage":{}"#,
            r#","packageImage":{"largeUrl":null}"#,
        ] {
            let document = format!(
                r#"{{"data":{{"c0":{{"id":"cawb00001","contentType":"TWO_DIMENSION","title":"Exact without cover"{cover}}}}}}}"#
            );
            let item = fetch_exact_library_item_with("adult", "CAWB-1", |_| Ok(document))
                .expect("missing or null cover data must remain valid")
                .expect("the exact item must remain accepted");
            assert!(item.cover_url.is_none());
        }
    }

    #[test]
    fn exact_library_content_rejects_conflicting_alias_responses() {
        let requests = vec![
            ExactContentRequest {
                alias: "c0".to_owned(),
                content_id: "cawb00001".to_owned(),
            },
            ExactContentRequest {
                alias: "c1".to_owned(),
                content_id: "cawb001".to_owned(),
            },
        ];
        assert_eq!(
            parse_exact_content(
                r#"{"data":{"c0":{"id":"cawb00001","contentType":"TWO_DIMENSION","title":"First","packageImage":null},"c1":{"id":"cawb001","contentType":"TWO_DIMENSION","title":"Second","packageImage":null}}}"#,
                Category::Adult,
                "CAWB-1",
                &requests,
            ),
            Err(DocumentError::Conflicting)
        );
    }

    #[test]
    fn exact_content_candidates_are_limited_to_evidence_backed_prefixes() {
        assert_eq!(exact_content_id_candidates("CAWB-001"), ["cawb00001"]);
        assert_eq!(exact_content_id_candidates("3DSVR-01871"), ["13dsvr01871"]);
        for unsupported in ["ADLT-123", "MDVR-419", "EBON-123", "VRKM-1577"] {
            assert!(exact_content_id_candidates(unsupported).is_empty());
            assert_eq!(
                fetch_exact_library_item_with("vr", unsupported, |_| {
                    panic!("unsupported prefixes must not dispatch speculative aliases")
                }),
                Ok(None)
            );
        }
    }

    #[test]
    fn maps_required_content_ids_without_shared_media_identity() {
        for (content_id, display_code) in [
            ("cawb00001", "CAWB-001"),
            ("vrkm01577", "VRKM-1577"),
            ("13dsvr01947", "3DSVR-01947"),
            ("n_709maraa244tk", "MARAA-244"),
            ("k9snos258", "SNOS-258"),
            ("tkipzz855", "IPZZ-855"),
            ("ovvr616", "OVVR-616"),
        ] {
            assert_eq!(
                display_code_from_content_id(content_id).as_deref(),
                Some(display_code)
            );
        }
        assert_eq!(
            display_code_from_content_id("ab12").as_deref(),
            Some("AB-12")
        );
        assert_eq!(display_code_from_content_id("unsafe-id"), None);
        assert!(valid_exact_library_cover_url(
            "https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/13dsvr01871/13dsvr01871ps.jpg",
            "13dsvr01871"
        ));
        assert!(!valid_exact_library_cover_url(
            "https://awsimgsrc.dmm.co.jp/pics_dig/digital/video/13dsvr01872/13dsvr01872ps.jpg",
            "13dsvr01871"
        ));
    }

    #[test]
    fn builds_all_exact_feed_and_category_requests() {
        for (feed, marker) in [
            ("popular", "RECOMMENDED"),
            ("newest", "RELEASE_DATE"),
            ("top-rated", "REVIEW_RANK_SCORE"),
            ("trending", "SALES_BEST_SELLERS"),
            ("monthly", "SALES_MONTHLY"),
        ] {
            let body = graphql_body(Category::Vr, feed, 100);
            assert!(body.contains(marker));
            assert!(body.contains("VR"));
            assert!(body.contains("100"));
        }
        assert!(graphql_body(Category::Adult, "popular", 10).contains("TWO_DIMENSION"));
    }

    #[test]
    fn isolates_bad_rows_preserves_order_and_rejects_conflicting_duplicates() {
        let document = search_document(
            r#"{"id":"vrkm01577","title":"First","packageImage":{"largeUrl":"https://awsimgsrc.dmm.co.jp/p/vrkm01577pl.jpg"}},
               {"id":"bad-id","title":"Bad"},
               {"id":"ovvr616","title":"Third","packageImage":null}"#,
        );
        let items = parse_catalog(&document, "popular", 10).unwrap();
        assert_eq!(
            items
                .iter()
                .map(|item| item.display_code.as_str())
                .collect::<Vec<_>>(),
            vec!["VRKM-1577", "OVVR-616"]
        );
        assert_eq!(
            items[0].cover_url.as_deref(),
            Some("https://awsimgsrc.dmm.co.jp/p/vrkm01577ps.jpg")
        );
        assert_eq!(items[1].cover_url, None);

        let conflict = search_document(
            r#"{"id":"vrkm01577","title":"First"},{"id":"vrkm01577","title":"Changed"}"#,
        );
        assert_eq!(
            parse_catalog(&conflict, "popular", 10),
            Err(DocumentError::Conflicting)
        );
    }

    #[test]
    fn treats_documented_null_collections_as_empty_and_malformed_shapes_as_errors() {
        assert_eq!(
            parse_catalog(
                r#"{"data":{"legacySearchPPV":{"result":null}}}"#,
                "popular",
                10
            ),
            Ok(Vec::new())
        );
        assert_eq!(
            parse_catalog(r#"{"data":{"legacySearchPPV":{}}}"#, "popular", 10),
            Ok(Vec::new())
        );
        assert_eq!(
            parse_catalog(
                r#"{"data":{"ppvContentRanking":{"items":null}}}"#,
                "trending",
                10
            ),
            Ok(Vec::new())
        );
        assert_eq!(
            parse_catalog(r#"{"data":{"ppvContentRanking":{}}}"#, "monthly", 10),
            Ok(Vec::new())
        );
        assert_eq!(
            parse_catalog(
                r#"{"data":{"legacySearchPPV":{"result":{"contents":{}}}}}"#,
                "popular",
                10
            ),
            Err(DocumentError::Malformed)
        );
    }

    #[test]
    fn ranking_accepts_only_required_content_wrappers_and_caps_accepted_rows() {
        let document = r#"{"data":{"ppvContentRanking":{"items":[
          {"id":"vrkm01577"},
          {"content":{"id":"vrkm01577","title":"First"}},
          {"content":{"id":"ovvr616","title":"Second"}}
        ]}}}"#;
        let items = parse_catalog(document, "trending", 1).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].display_code, "VRKM-1577");
    }

    #[test]
    fn exact_authority_blocks_stale_cross_item_and_unretained_cover_requests() {
        let state = FanzaCatalogState::default();
        let response = fetch_catalog_with(&state, &request("vr", 1), |_| {
            Ok(search_document(r#"{"id":"vrkm01577","packageImage":{"largeUrl":"https://awsimgsrc.dmm.co.jp/p/vrkm01577pl.jpg"}}"#))
        }).unwrap();
        assert_eq!(
            response[2..8],
            [
                "vr",
                "vrkm01577",
                "VRKM-1577",
                "",
                "fanza-cover-1-1",
                "0.72"
            ]
        );
        let fetches = std::cell::Cell::new(0);
        let bytes = fetch_cover_with(
            &state,
            "vr",
            "1",
            "1",
            "vrkm01577",
            "VRKM-1577",
            "fanza-cover-1-1",
            |url| {
                fetches.set(fetches.get() + 1);
                assert!(url.ends_with("ps.jpg"));
                Ok(vec![0xff, 0xd8, 0xff, 0, 0, 0, 0, 0, 0, 0, 0, 0])
            },
        )
        .unwrap();
        assert_eq!(bytes.len(), 12);
        assert_eq!(fetches.get(), 1);
        for (content_id, generation) in [("ovvr616", "1"), ("vrkm01577", "2")] {
            assert_eq!(
                fetch_cover_with(
                    &state,
                    "vr",
                    generation,
                    "1",
                    content_id,
                    "VRKM-1577",
                    "fanza-cover-1-1",
                    |_| {
                        fetches.set(fetches.get() + 1);
                        Ok(vec![])
                    }
                ),
                Err(VR_STALE)
            );
        }
        assert_eq!(fetches.get(), 1);
    }

    #[test]
    fn newer_invalidation_rejects_late_results_and_preserves_newer_authority() {
        let state = FanzaCatalogState::default();
        fetch_catalog_with(&state, &request("vr", 1), |_| {
            Ok(search_document(r#"{"id":"vrkm01577"}"#))
        })
        .unwrap();
        invalidate_catalog(&state, "vr", "2").unwrap();
        assert_eq!(
            fetch_catalog_with(&state, &request("vr", 2), |_| Ok(search_document(
                r#"{"id":"ovvr616"}"#
            ))),
            Err(VR_STALE)
        );
        let newer = fetch_catalog_with(&state, &request("vr", 3), |_| {
            Ok(search_document(r#"{"id":"ovvr616"}"#))
        })
        .unwrap();
        assert_eq!(newer[4], "OVVR-616");
        assert_eq!(invalidate_catalog(&state, "vr", "2"), Err(VR_STALE));
    }

    #[test]
    fn cover_host_and_raster_boundaries_fail_before_dispatch() {
        for url in [
            "http://awsimgsrc.dmm.co.jp/p/xps.jpg",
            "https://user@awsimgsrc.dmm.co.jp/p/xps.jpg",
            "https://awsimgsrc.dmm.co.jp:443/p/xps.jpg",
            "https://awsimgsrc.dmm.co.jp.evil.example/p/xps.jpg",
            "https://awsimgsrc.dmm.co.jp\\evil/p/xps.jpg",
        ] {
            assert!(!valid_image_url(url));
        }
        assert!(valid_image_url("https://awsimgsrc.dmm.co.jp/p/xps.jpg"));
        assert!(accepted_raster(&[
            0xff, 0xd8, 0xff, 0, 0, 0, 0, 0, 0, 0, 0, 0
        ]));
        assert!(!accepted_raster(b"not an image"));
    }

    #[test]
    fn transport_framing_preserves_unicode_statuses_and_exact_bounds() {
        let unicode = "{\"title\":\"日本語\"}";
        let framed = format!("{unicode}{STATUS_MARKER}200");
        assert_eq!(
            parse_framed_response(framed.as_bytes(), unicode.len()).unwrap(),
            unicode.as_bytes()
        );
        assert_eq!(
            parse_framed_response(b"gone\nAUTO_VIDEO_HTTP_STATUS:404", 10),
            Err(ProviderRequestError::SourceUnavailable)
        );
        assert_eq!(
            parse_process_response(framed.as_bytes(), 100, false),
            Err(ProviderRequestError::Network)
        );
        assert_eq!(
            parse_framed_response(b"12345\nAUTO_VIDEO_HTTP_STATUS:200", 4),
            Err(ProviderRequestError::Provider)
        );
    }

    #[test]
    fn windows_scripts_bound_headers_and_every_body_read_without_credentials() {
        for script in [WINDOWS_GRAPHQL_SCRIPT, WINDOWS_IMAGE_SCRIPT] {
            assert!(script.contains("AllowAutoRedirect = $false"));
            assert!(script.contains("ResponseHeadersRead, $deadline.Token"));
            assert!(script.contains("ReadAsync($buffer, 0, $buffer.Length, $deadline.Token)"));
            assert!(script.contains("CancelAfter(20000)"));
            assert!(!script.contains("Cookie"));
            assert!(!script.contains("Authorization"));
        }
        assert!(WINDOWS_GRAPHQL_SCRIPT.contains("Origin"));
        assert!(WINDOWS_GRAPHQL_SCRIPT.contains("Referrer"));
        assert!(WINDOWS_IMAGE_SCRIPT.contains("ToBase64String($memory.ToArray())"));
    }

    #[test]
    fn windows_image_parser_uses_raw_bounded_bytes() {
        let framed = b"/9j/AAAAAAAAAAAA\nAUTO_VIDEO_HTTP_STATUS:200";
        assert_eq!(
            parse_windows_image_response(framed, true).unwrap(),
            vec![0xff, 0xd8, 0xff, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            parse_windows_image_response(framed, false),
            Err(ProviderRequestError::Network)
        );
    }
}
