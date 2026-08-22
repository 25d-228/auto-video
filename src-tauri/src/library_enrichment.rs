use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::process::Command;

use crate::{
    javdb_catalog::fetch_exact_library_metadata_with,
    vr_torrent::{hex_sha1, json_array, json_object, json_string, json_u64, JsonParser, JsonValue},
    ProviderRequestError,
};
use unicode_normalization::UnicodeNormalization;

pub(crate) const LIBRARY_ENRICHMENT_FAILED: &str = "library_enrichment_failed";
pub(crate) const LIBRARY_ENRICHMENT_STALE: &str = "library_enrichment_stale";
pub(crate) const LIBRARY_COVER_STALE: &str = "library_cover_stale";

const CACHE_VERSION: &str = "library-enrichment-cache-v1";
const CACHE_MAX_BYTES: u64 = 4 * 1024 * 1024;
const METADATA_TTL_SECONDS: u64 = 365 * 24 * 60 * 60;
const COVER_TTL_SECONDS: u64 = 24 * 60 * 60;
const DEFAULT_POSTER_ASPECT: f64 = 0.72;
const TMDB_POSTER_ASPECT: f64 = 2.0 / 3.0;
const PROVIDER_TEXT_LIMIT: usize = 4 * 1024 * 1024;
const COVER_BYTE_LIMIT: usize = 16 * 1024 * 1024;
const COVER_MIN_BYTES: usize = 6_000;
const HTTP_STATUS_MARKER: &str = "\nAUTO_VIDEO_HTTP_STATUS:";
#[cfg(target_os = "macos")]
const HTTP_STATUS_WRITE_OUT: &str = "\nAUTO_VIDEO_HTTP_STATUS:%{http_code}";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LibraryCategory {
    Movie,
    Tv,
    Adult,
    Vr,
}

impl LibraryCategory {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "movie" => Some(Self::Movie),
            "tv" => Some(Self::Tv),
            "adult" => Some(Self::Adult),
            "vr" => Some(Self::Vr),
            _ => None,
        }
    }

    fn value(self) -> &'static str {
        match self {
            Self::Movie => "movie",
            Self::Tv => "tv",
            Self::Adult => "adult",
            Self::Vr => "vr",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LibraryItemAuthority {
    pub category: LibraryCategory,
    pub identity: String,
    pub local_title: String,
    pub year: Option<String>,
    pub code: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
struct CoverSource {
    url: String,
    referer: String,
    cookie: String,
    aspect_ratio: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LibraryPresentation {
    source: Option<String>,
    provider_id: Option<String>,
    imdb_id: Option<String>,
    title: Option<String>,
    original_title: Option<String>,
    date: Option<String>,
    runtime: Option<String>,
    genres: Vec<String>,
    cast: Vec<String>,
    overview: Option<String>,
    cover: Option<CoverSource>,
    cover_state: &'static str,
    aspect_ratio: f64,
}

impl LibraryPresentation {
    fn local_only(cover_state: &'static str) -> Self {
        Self {
            source: None,
            provider_id: None,
            imdb_id: None,
            title: None,
            original_title: None,
            date: None,
            runtime: None,
            genres: Vec::new(),
            cast: Vec::new(),
            overview: None,
            cover: None,
            cover_state,
            aspect_ratio: DEFAULT_POSTER_ASPECT,
        }
    }

    fn is_automatic(&self) -> bool {
        self.source.is_some() && self.provider_id.is_some()
    }
}

#[derive(Clone)]
struct CoverAuthority {
    item_identity: String,
    source: CoverSource,
    bytes: Option<Vec<u8>>,
}

#[derive(Default)]
struct LibraryEnrichmentContext {
    covers: HashMap<String, CoverAuthority>,
}

#[derive(Clone, Default)]
pub(crate) struct LibraryEnrichmentState(Arc<Mutex<LibraryEnrichmentContext>>);

#[derive(Clone, Debug)]
struct CacheEntry {
    identity: String,
    metadata_saved_at: u64,
    cover_saved_at: u64,
    presentation: LibraryPresentation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProviderMethod {
    Get,
    Post,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProviderTextRequest {
    pub method: ProviderMethod,
    pub url: String,
    pub accept: &'static str,
    pub referer: Option<&'static str>,
    pub cookie: Option<&'static str>,
    pub body: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProviderImageRequest {
    pub url: String,
    pub referer: String,
    pub cookie: String,
}

fn request_host_and_path(url: &str) -> Option<(&str, &str)> {
    if url
        .bytes()
        .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace() || byte == b'\\')
    {
        return None;
    }
    let remainder = url.strip_prefix("https://")?;
    let (host, path) = remainder.split_once('/')?;
    (!host.contains(['@', ':']) && !path.is_empty()).then_some((host, path))
}

fn valid_text_request(request: &ProviderTextRequest) -> bool {
    let Some((host, path)) = request_host_and_path(&request.url) else {
        return false;
    };
    match (request.method, host) {
        (ProviderMethod::Get, "api.themoviedb.org") => path.starts_with("3/"),
        (ProviderMethod::Get, "api.tvmaze.com") => {
            path.starts_with("lookup/shows?") || path.starts_with("singlesearch/shows?")
        }
        (ProviderMethod::Post, "graphql.anilist.co") => path.is_empty() || path == "/",
        (ProviderMethod::Get, "r18.dev") => path.starts_with("videos/vod/movies/detail/-/"),
        (ProviderMethod::Get, "www.javdatabase.com") => path.starts_with("movies/"),
        (ProviderMethod::Get, "www.mgstage.com") => path.starts_with("product/product_detail/"),
        _ => false,
    }
}

fn valid_image_request(request: &ProviderImageRequest) -> bool {
    let Some((host, path)) = request_host_and_path(&request.url) else {
        return false;
    };
    match host {
        "image.tmdb.org" => path.starts_with("t/p/"),
        "static.tvmaze.com" => true,
        "s1.anilist.co" | "s2.anilist.co" | "s3.anilist.co" | "s4.anilist.co" => true,
        "pics.dmm.co.jp" => {
            request.referer == "https://www.dmm.co.jp/"
                && (path.starts_with("digital/video/")
                    || path.starts_with("digital/amateur/")
                    || path.starts_with("mono/movie/"))
        }
        "image.mgstage.com" => {
            request.referer == "https://www.mgstage.com/" && path.starts_with("images/")
        }
        "www.javdatabase.com" => {
            request.referer == "https://www.javdatabase.com/" && path.starts_with("covers/")
        }
        _ => false,
    }
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
        200..=299 if body.len() <= maximum => Ok(body.to_vec()),
        404 | 410 | 451 => Err(ProviderRequestError::SourceUnavailable),
        0 => Err(ProviderRequestError::Network),
        _ => Err(ProviderRequestError::Provider),
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn fetch_text_request(
    request: &ProviderTextRequest,
    tmdb_token: Option<&str>,
) -> Result<String, ProviderRequestError> {
    if !valid_text_request(request) {
        return Err(ProviderRequestError::Provider);
    }
    let mut command = Command::new("/usr/bin/curl");
    command.args([
        "--silent",
        "--show-error",
        "--connect-timeout",
        "10",
        "--max-time",
        "20",
        "--max-redirs",
        "0",
        "--max-filesize",
        &PROVIDER_TEXT_LIMIT.to_string(),
        "--user-agent",
        "Auto-Video/0.1",
        "--header",
        &format!("Accept: {}", request.accept),
    ]);
    if request.url.starts_with("https://api.themoviedb.org/") {
        let token = tmdb_token.ok_or(ProviderRequestError::Provider)?;
        command
            .arg("--header")
            .arg(format!("Authorization: Bearer {token}"));
    }
    if let Some(referer) = request.referer {
        command.arg("--header").arg(format!("Referer: {referer}"));
    }
    if let Some(cookie) = request.cookie {
        command.arg("--header").arg(format!("Cookie: {cookie}"));
    }
    if request.method == ProviderMethod::Post {
        command.args([
            "--request",
            "POST",
            "--header",
            "Content-Type: application/json",
        ]);
        command
            .arg("--data-binary")
            .arg(request.body.as_deref().unwrap_or(""));
    }
    let output = command
        .arg("--write-out")
        .arg(HTTP_STATUS_WRITE_OUT)
        .arg(&request.url)
        .output()
        .map_err(|_| ProviderRequestError::Network)?;
    if !output.status.success() {
        return Err(ProviderRequestError::Network);
    }
    String::from_utf8(parse_http_response(&output.stdout, PROVIDER_TEXT_LIMIT)?)
        .map_err(|_| ProviderRequestError::Provider)
}

#[cfg(target_os = "macos")]
pub(crate) fn fetch_image_request(
    request: &ProviderImageRequest,
) -> Result<Vec<u8>, ProviderRequestError> {
    if !valid_image_request(request) {
        return Err(ProviderRequestError::Provider);
    }
    let mut command = Command::new("/usr/bin/curl");
    command.args([
        "--silent",
        "--show-error",
        "--connect-timeout",
        "10",
        "--max-time",
        "20",
        "--max-redirs",
        "0",
        "--max-filesize",
        &COVER_BYTE_LIMIT.to_string(),
        "--header",
        "Accept: image/*",
    ]);
    if !request.referer.is_empty() {
        command
            .arg("--header")
            .arg(format!("Referer: {}", request.referer));
    }
    if !request.cookie.is_empty() {
        command
            .arg("--header")
            .arg(format!("Cookie: {}", request.cookie));
    }
    let output = command
        .arg("--write-out")
        .arg(HTTP_STATUS_WRITE_OUT)
        .arg(&request.url)
        .output()
        .map_err(|_| ProviderRequestError::Network)?;
    if !output.status.success() {
        return Err(ProviderRequestError::Network);
    }
    parse_http_response(&output.stdout, COVER_BYTE_LIMIT)
}

#[cfg(any(target_os = "windows", test))]
const WINDOWS_TEXT_SCRIPT: &str = r#"$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
Add-Type -AssemblyName System.Net.Http
$handler = [System.Net.Http.HttpClientHandler]::new()
$handler.AllowAutoRedirect = $false
$client = [System.Net.Http.HttpClient]::new($handler)
$deadline = [System.Threading.CancellationTokenSource]::new()
$deadline.CancelAfter(20000)
try {
  $method = if ($env:AUTO_VIDEO_LIBRARY_METHOD -eq 'POST') { [System.Net.Http.HttpMethod]::Post } else { [System.Net.Http.HttpMethod]::Get }
  $request = [System.Net.Http.HttpRequestMessage]::new($method, $env:AUTO_VIDEO_LIBRARY_URL)
  $request.Headers.Accept.ParseAdd($env:AUTO_VIDEO_LIBRARY_ACCEPT)
  $request.Headers.TryAddWithoutValidation('User-Agent', 'Auto-Video/0.1') | Out-Null
  if (-not [string]::IsNullOrEmpty($env:AUTO_VIDEO_LIBRARY_REFERER)) { $request.Headers.Referrer = [Uri]$env:AUTO_VIDEO_LIBRARY_REFERER }
  if (-not [string]::IsNullOrEmpty($env:AUTO_VIDEO_LIBRARY_COOKIE)) { $request.Headers.TryAddWithoutValidation('Cookie', $env:AUTO_VIDEO_LIBRARY_COOKIE) | Out-Null }
  if (-not [string]::IsNullOrEmpty($env:AUTO_VIDEO_LIBRARY_TOKEN)) { $request.Headers.TryAddWithoutValidation('Authorization', 'Bearer ' + $env:AUTO_VIDEO_LIBRARY_TOKEN) | Out-Null }
  if ($method -eq [System.Net.Http.HttpMethod]::Post) { $request.Content = [System.Net.Http.StringContent]::new($env:AUTO_VIDEO_LIBRARY_BODY, [System.Text.Encoding]::UTF8, 'application/json') }
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
  if (-not $tooLarge) { $bytes = $memory.ToArray(); $output.Write($bytes, 0, $bytes.Length) }
  $status = if ($tooLarge) { 413 } else { [int]$response.StatusCode }
  $marker = [System.Text.Encoding]::UTF8.GetBytes("`nAUTO_VIDEO_HTTP_STATUS:" + $status)
  $output.Write($marker, 0, $marker.Length)
} catch {
  [Environment]::Exit(28)
} finally {
  $deadline.Dispose(); $client.Dispose(); $handler.Dispose()
}"#;

#[cfg(any(target_os = "windows", test))]
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
  $request = [System.Net.Http.HttpRequestMessage]::new([System.Net.Http.HttpMethod]::Get, $env:AUTO_VIDEO_LIBRARY_IMAGE_URL)
  $request.Headers.Accept.ParseAdd('image/*')
  if (-not [string]::IsNullOrEmpty($env:AUTO_VIDEO_LIBRARY_REFERER)) { $request.Headers.Referrer = [Uri]$env:AUTO_VIDEO_LIBRARY_REFERER }
  if (-not [string]::IsNullOrEmpty($env:AUTO_VIDEO_LIBRARY_COOKIE)) { $request.Headers.TryAddWithoutValidation('Cookie', $env:AUTO_VIDEO_LIBRARY_COOKIE) | Out-Null }
  $response = $client.SendAsync($request, [System.Net.Http.HttpCompletionOption]::ResponseHeadersRead, $deadline.Token).GetAwaiter().GetResult()
  $stream = $response.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
  $memory = [System.IO.MemoryStream]::new()
  $buffer = [byte[]]::new(65536)
  $tooLarge = $false
  while (($read = $stream.ReadAsync($buffer, 0, $buffer.Length, $deadline.Token).GetAwaiter().GetResult()) -gt 0) {
    if ($memory.Length + $read -gt 16777216) { $tooLarge = $true; break }
    $memory.Write($buffer, 0, $read)
  }
  if ($tooLarge) { [Console]::Out.Write("`nAUTO_VIDEO_HTTP_STATUS:413") }
  else { [Console]::Out.Write([Convert]::ToBase64String($memory.ToArray())); [Console]::Out.Write("`nAUTO_VIDEO_HTTP_STATUS:" + [int]$response.StatusCode) }
} catch {
  [Environment]::Exit(28)
} finally {
  $deadline.Dispose(); $client.Dispose(); $handler.Dispose()
}"#;

#[cfg(target_os = "windows")]
fn decode_base64(value: &str) -> Option<Vec<u8>> {
    let digit = |byte| match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    if !value.len().is_multiple_of(4) {
        return None;
    }
    let mut output = Vec::with_capacity(value.len() / 4 * 3);
    for chunk in value.as_bytes().chunks_exact(4) {
        let first = digit(chunk[0])?;
        let second = digit(chunk[1])?;
        output.push((first << 2) | (second >> 4));
        if chunk[2] != b'=' {
            let third = digit(chunk[2])?;
            output.push((second << 4) | (third >> 2));
            if chunk[3] != b'=' {
                let fourth = digit(chunk[3])?;
                output.push((third << 6) | fourth);
            }
        } else if chunk[3] != b'=' {
            return None;
        }
    }
    Some(output)
}

#[cfg(target_os = "windows")]
pub(crate) fn fetch_text_request(
    request: &ProviderTextRequest,
    tmdb_token: Option<&str>,
) -> Result<String, ProviderRequestError> {
    if !valid_text_request(request) {
        return Err(ProviderRequestError::Provider);
    }
    let output = Command::new("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
        .arg(WINDOWS_TEXT_SCRIPT)
        .env("AUTO_VIDEO_LIBRARY_URL", &request.url)
        .env(
            "AUTO_VIDEO_LIBRARY_METHOD",
            if request.method == ProviderMethod::Post {
                "POST"
            } else {
                "GET"
            },
        )
        .env("AUTO_VIDEO_LIBRARY_ACCEPT", request.accept)
        .env("AUTO_VIDEO_LIBRARY_REFERER", request.referer.unwrap_or(""))
        .env("AUTO_VIDEO_LIBRARY_COOKIE", request.cookie.unwrap_or(""))
        .env(
            "AUTO_VIDEO_LIBRARY_BODY",
            request.body.as_deref().unwrap_or(""),
        )
        .env(
            "AUTO_VIDEO_LIBRARY_TOKEN",
            if request.url.starts_with("https://api.themoviedb.org/") {
                tmdb_token.ok_or(ProviderRequestError::Provider)?
            } else {
                ""
            },
        )
        .output()
        .map_err(|_| ProviderRequestError::Network)?;
    if !output.status.success() {
        return Err(ProviderRequestError::Network);
    }
    String::from_utf8(parse_http_response(&output.stdout, PROVIDER_TEXT_LIMIT)?)
        .map_err(|_| ProviderRequestError::Provider)
}

#[cfg(target_os = "windows")]
pub(crate) fn fetch_image_request(
    request: &ProviderImageRequest,
) -> Result<Vec<u8>, ProviderRequestError> {
    if !valid_image_request(request) {
        return Err(ProviderRequestError::Provider);
    }
    let output = Command::new("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
        .arg(WINDOWS_IMAGE_SCRIPT)
        .env("AUTO_VIDEO_LIBRARY_IMAGE_URL", &request.url)
        .env("AUTO_VIDEO_LIBRARY_REFERER", &request.referer)
        .env("AUTO_VIDEO_LIBRARY_COOKIE", &request.cookie)
        .output()
        .map_err(|_| ProviderRequestError::Network)?;
    if !output.status.success() {
        return Err(ProviderRequestError::Network);
    }
    let encoded = parse_http_response(&output.stdout, COVER_BYTE_LIMIT.div_ceil(3) * 4)?;
    let encoded = std::str::from_utf8(&encoded).map_err(|_| ProviderRequestError::Provider)?;
    let bytes = decode_base64(encoded).ok_or(ProviderRequestError::Provider)?;
    (bytes.len() <= COVER_BYTE_LIMIT)
        .then_some(bytes)
        .ok_or(ProviderRequestError::Provider)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn fetch_text_request(
    _request: &ProviderTextRequest,
    _tmdb_token: Option<&str>,
) -> Result<String, ProviderRequestError> {
    Err(ProviderRequestError::SourceUnavailable)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn fetch_image_request(
    _request: &ProviderImageRequest,
) -> Result<Vec<u8>, ProviderRequestError> {
    Err(ProviderRequestError::SourceUnavailable)
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn optional_text(object: &BTreeMap<String, JsonValue>, key: &str) -> Option<String> {
    match object.get(key) {
        None | Some(JsonValue::Null) => None,
        Some(JsonValue::String(value)) if !value.trim().is_empty() && value.len() <= 256 * 1024 => {
            Some(value.clone())
        }
        _ => None,
    }
}

fn optional_date(object: &BTreeMap<String, JsonValue>, key: &str) -> Option<String> {
    optional_text(object, key).filter(|value| valid_date(value))
}

fn valid_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
}

fn named_values(object: &BTreeMap<String, JsonValue>, key: &str, limit: usize) -> Vec<String> {
    let Some(values) = object.get(key).and_then(json_array) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for value in values {
        let Some(value) = json_object(value) else {
            continue;
        };
        let Some(name) = json_string(value, "name")
            .filter(|name| !name.trim().is_empty() && name.len() <= 16 * 1024)
        else {
            continue;
        };
        if !names.iter().any(|current| current == name) {
            names.push(name.to_owned());
        }
        if names.len() == limit {
            break;
        }
    }
    names
}

fn r18_actresses(object: &BTreeMap<String, JsonValue>) -> Vec<String> {
    let Some(values) = object.get("actresses").and_then(json_array) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for value in values {
        let Some(value) = json_object(value) else {
            continue;
        };
        let name = ["name_kanji", "name_kana", "name_romaji", "name"]
            .into_iter()
            .find_map(|key| optional_text(value, key));
        if let Some(name) = name.filter(|name| !names.contains(name)) {
            names.push(name);
        }
    }
    names
}

fn normalized_title(value: &str) -> String {
    value
        .nfc()
        .flat_map(char::to_lowercase)
        .filter(|character| character.is_alphanumeric())
        .collect()
}

fn title_matches(local: &str, candidate: &str) -> bool {
    let local = normalized_title(local);
    let candidate = normalized_title(candidate);
    !local.is_empty()
        && !candidate.is_empty()
        && (local == candidate || local.contains(&candidate) || candidate.contains(&local))
}

fn year_matches(requested: Option<&str>, date: Option<&str>) -> bool {
    let Some(requested) = requested else {
        return true;
    };
    let Some(date) = date else {
        return false;
    };
    let Some(candidate) = date.get(..4) else {
        return false;
    };
    requested
        .parse::<i32>()
        .ok()
        .zip(candidate.parse::<i32>().ok())
        .is_some_and(|(requested, candidate)| (requested - candidate).abs() <= 1)
}

#[derive(Clone)]
struct TmdbSearchCandidate {
    id: u64,
    date: Option<String>,
    titles: Vec<String>,
}

enum TmdbSearchOutcome {
    Missing,
    Rejected,
    Accepted {
        provider_id: u64,
        candidates: Vec<TmdbSearchCandidate>,
    },
}

fn tmdb_titles(object: &BTreeMap<String, JsonValue>, keys: &[&str]) -> Vec<String> {
    keys.iter()
        .filter_map(|key| optional_text(object, key))
        .collect()
}

fn tmdb_candidate_matches_details(
    candidate: &TmdbSearchCandidate,
    detail_titles: &[String],
    detail_date: Option<&str>,
    requested_year: Option<&str>,
) -> bool {
    let titles_match = candidate.titles.iter().any(|search_title| {
        let normalized_search_title = normalized_title(search_title);
        detail_titles
            .iter()
            .any(|detail_title| normalized_search_title == normalized_title(detail_title))
    });
    let dates_match = match (candidate.date.as_deref(), detail_date) {
        (Some(search_date), Some(detail_date)) => search_date.get(..4) == detail_date.get(..4),
        (None, None) => true,
        _ => requested_year.is_none(),
    };
    titles_match && dates_match
}

fn has_descriptive_metadata(presentation: &LibraryPresentation) -> bool {
    presentation.title.is_some()
        || presentation.date.is_some()
        || presentation.runtime.is_some()
        || !presentation.genres.is_empty()
        || !presentation.cast.is_empty()
        || presentation.overview.is_some()
}

fn percent_encode(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(value.len());
    for byte in value.as_bytes() {
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

fn validated_document(
    request: &ProviderTextRequest,
    fetch: &mut impl FnMut(&ProviderTextRequest) -> Result<String, ProviderRequestError>,
) -> Result<String, ProviderRequestError> {
    let document = fetch(request)?;
    if document.len() > PROVIDER_TEXT_LIMIT {
        return Err(ProviderRequestError::Provider);
    }
    Ok(document)
}

fn tmdb_search(
    authority: &LibraryItemAuthority,
    kind: &str,
    include_year: bool,
    fetch: &mut impl FnMut(&ProviderTextRequest) -> Result<String, ProviderRequestError>,
) -> Result<TmdbSearchOutcome, ProviderRequestError> {
    let mut search_url = format!(
        "https://api.themoviedb.org/3/search/{kind}?query={}&include_adult=false",
        percent_encode(&authority.local_title)
    );
    if include_year {
        let year_parameter = if kind == "movie" {
            "year"
        } else {
            "first_air_date_year"
        };
        if let Some(year) = authority.year.as_deref() {
            search_url.push('&');
            search_url.push_str(year_parameter);
            search_url.push('=');
            search_url.push_str(year);
        }
    }
    let search = validated_document(
        &ProviderTextRequest {
            method: ProviderMethod::Get,
            url: search_url,
            accept: "application/json",
            referer: None,
            cookie: None,
            body: None,
        },
        fetch,
    )?;
    let root = JsonParser::new(&search)
        .parse()
        .and_then(|value| json_object(&value).cloned())
        .ok_or(ProviderRequestError::Provider)?;
    let results = root
        .get("results")
        .and_then(json_array)
        .ok_or(ProviderRequestError::Provider)?;
    if results.is_empty() {
        return Ok(TmdbSearchOutcome::Missing);
    }
    let title_keys = if kind == "movie" {
        ["title", "original_title"]
    } else {
        ["name", "original_name"]
    };
    let date_key = if kind == "movie" {
        "release_date"
    } else {
        "first_air_date"
    };
    let mut candidates = Vec::new();
    for result in results {
        let Some(result) = json_object(result) else {
            continue;
        };
        let Some(id) = json_u64(result, "id").filter(|id| *id > 0) else {
            continue;
        };
        let date = optional_date(result, date_key);
        let titles = tmdb_titles(result, &title_keys);
        if titles.is_empty() || !year_matches(authority.year.as_deref(), date.as_deref()) {
            continue;
        }
        candidates.push(TmdbSearchCandidate { id, date, titles });
    }
    if candidates.is_empty() {
        return Ok(TmdbSearchOutcome::Rejected);
    }
    let mut matching_ids = candidates
        .iter()
        .filter(|candidate| {
            candidate
                .titles
                .iter()
                .any(|title| title_matches(&authority.local_title, title))
        })
        .map(|candidate| candidate.id)
        .collect::<Vec<_>>();
    matching_ids.sort_unstable();
    matching_ids.dedup();
    let provider_id = if matching_ids.len() == 1 {
        matching_ids[0]
    } else if matching_ids.is_empty()
        && authority.category == LibraryCategory::Movie
        && authority.year.is_some()
    {
        let mut year_matches = candidates
            .iter()
            .map(|candidate| candidate.id)
            .collect::<Vec<_>>();
        year_matches.sort_unstable();
        year_matches.dedup();
        if year_matches.len() != 1 {
            return Ok(TmdbSearchOutcome::Rejected);
        }
        year_matches[0]
    } else {
        return Ok(TmdbSearchOutcome::Rejected);
    };
    Ok(TmdbSearchOutcome::Accepted {
        provider_id,
        candidates,
    })
}

fn tmdb_presentation(
    authority: &LibraryItemAuthority,
    token: Option<&str>,
    fetch: &mut impl FnMut(&ProviderTextRequest) -> Result<String, ProviderRequestError>,
) -> Result<LibraryPresentation, ProviderRequestError> {
    let Some(_token) = token.filter(|token| !token.is_empty()) else {
        return Ok(LibraryPresentation::local_only("unavailable"));
    };
    let kind = if authority.category == LibraryCategory::Movie {
        "movie"
    } else {
        "tv"
    };
    let date_key = if kind == "movie" {
        "release_date"
    } else {
        "first_air_date"
    };
    let search = tmdb_search(authority, kind, authority.year.is_some(), fetch)?;
    let search = if matches!(&search, TmdbSearchOutcome::Missing)
        && authority.category == LibraryCategory::Movie
        && authority.year.is_some()
    {
        tmdb_search(authority, kind, false, fetch)?
    } else {
        search
    };
    let (provider_id, candidates) = match search {
        TmdbSearchOutcome::Accepted {
            provider_id,
            candidates,
        } => (provider_id, candidates),
        TmdbSearchOutcome::Missing | TmdbSearchOutcome::Rejected => {
            return Ok(LibraryPresentation::local_only("missing"));
        }
    };
    let accepted_candidates = candidates
        .iter()
        .filter(|candidate| candidate.id == provider_id)
        .collect::<Vec<_>>();
    let detail_url = format!(
        "https://api.themoviedb.org/3/{kind}/{provider_id}?append_to_response=credits%2Cexternal_ids"
    );
    let details = validated_document(
        &ProviderTextRequest {
            method: ProviderMethod::Get,
            url: detail_url,
            accept: "application/json",
            referer: None,
            cookie: None,
            body: None,
        },
        fetch,
    )?;
    let details = JsonParser::new(&details)
        .parse()
        .and_then(|value| json_object(&value).cloned())
        .ok_or(ProviderRequestError::Provider)?;
    if json_u64(&details, "id") != Some(provider_id) {
        return Ok(LibraryPresentation::local_only("missing"));
    }
    let title_key = if kind == "movie" { "title" } else { "name" };
    let original_title_key = if kind == "movie" {
        "original_title"
    } else {
        "original_name"
    };
    let title = optional_text(&details, title_key).ok_or(ProviderRequestError::Provider)?;
    let detail_date = optional_date(&details, date_key);
    let detail_titles = tmdb_titles(&details, &[title_key, original_title_key]);
    if !year_matches(authority.year.as_deref(), detail_date.as_deref())
        || !accepted_candidates.iter().any(|candidate| {
            tmdb_candidate_matches_details(
                candidate,
                &detail_titles,
                detail_date.as_deref(),
                authority.year.as_deref(),
            )
        })
    {
        return Ok(LibraryPresentation::local_only("missing"));
    }
    let poster = optional_text(&details, "poster_path")
        .filter(|path| path.starts_with('/') && !path.contains(['\\', '?', '#']))
        .map(|path| CoverSource {
            url: format!("https://image.tmdb.org/t/p/w500{path}"),
            referer: String::new(),
            cookie: String::new(),
            aspect_ratio: TMDB_POSTER_ASPECT,
        });
    let runtime_minutes = json_u64(&details, "runtime").or_else(|| {
        details
            .get("episode_run_time")
            .and_then(json_array)
            .and_then(|values| values.first())
            .and_then(|value| match value {
                JsonValue::Number(value) => value.parse().ok(),
                _ => None,
            })
    });
    let cast = details
        .get("credits")
        .and_then(json_object)
        .map(|credits| named_values(credits, "cast", 5))
        .unwrap_or_default();
    let imdb_id = details
        .get("external_ids")
        .and_then(json_object)
        .and_then(|external| optional_text(external, "imdb_id"))
        .filter(|value| {
            let digits = value.strip_prefix("tt").unwrap_or("");
            (7..=10).contains(&digits.len()) && digits.bytes().all(|byte| byte.is_ascii_digit())
        });
    Ok(LibraryPresentation {
        source: Some("TMDB".to_owned()),
        provider_id: Some(provider_id.to_string()),
        imdb_id,
        title: Some(title),
        original_title: optional_text(&details, original_title_key),
        date: detail_date,
        runtime: runtime_minutes
            .filter(|minutes| *minutes > 0)
            .map(|minutes| format!("{minutes} min")),
        genres: named_values(&details, "genres", 8),
        cast,
        overview: optional_text(&details, "overview"),
        cover: poster,
        cover_state: "unavailable",
        aspect_ratio: TMDB_POSTER_ASPECT,
    })
}

fn dmm_cid_variants(code: &str) -> Vec<String> {
    let Some((prefix, number)) = code.split_once('-') else {
        return Vec::new();
    };
    let label = match prefix.to_ascii_lowercase().as_str() {
        "ebon" => "ebod".to_owned(),
        label => label.to_owned(),
    };
    let number = number.trim_start_matches('0');
    let number = if number.is_empty() { "0" } else { number };
    let mut candidates = vec![
        format!("{label}{number:0>5}"),
        format!("{label}{number:0>3}"),
        format!("{label}{number}"),
        format!("1{label}{number:0>5}"),
        format!("1{label}{number:0>3}"),
        format!("13{label}{number:0>5}"),
        format!("13{label}{number:0>3}"),
    ];
    let maker_prefix = match label.as_str() {
        "ccvr" => Some("h_1270"),
        "devr" => Some("h_1711"),
        "clot" => Some("h_237"),
        _ => None,
    };
    if let Some(maker_prefix) = maker_prefix {
        candidates.insert(0, format!("{maker_prefix}{label}{number:0>5}"));
        candidates.insert(1, format!("{maker_prefix}{label}{number:0>3}"));
    }
    let mut unique = HashSet::new();
    candidates.retain(|candidate| unique.insert(candidate.clone()));
    candidates
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
            offset = offset.checked_add(2 + segment)?;
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
        match bytes.get(12..16)? {
            b"VP8X" if bytes.len() >= 30 => {
                let width = 1
                    + u32::from(bytes[24])
                    + (u32::from(bytes[25]) << 8)
                    + (u32::from(bytes[26]) << 16);
                let height = 1
                    + u32::from(bytes[27])
                    + (u32::from(bytes[28]) << 8)
                    + (u32::from(bytes[29]) << 16);
                Some((width, height))
            }
            b"VP8 " if bytes.len() >= 30 => Some((
                u32::from(u16::from_le_bytes([bytes[26], bytes[27]]) & 0x3fff),
                u32::from(u16::from_le_bytes([bytes[28], bytes[29]]) & 0x3fff),
            )),
            b"VP8L" if bytes.len() >= 25 => {
                let bits = u32::from_le_bytes([bytes[21], bytes[22], bytes[23], bytes[24]]);
                Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
            }
            _ => None,
        }
    } else {
        None
    }
}

fn validated_cover(bytes: Vec<u8>) -> Result<(Vec<u8>, f64), ProviderRequestError> {
    if !(COVER_MIN_BYTES..=COVER_BYTE_LIMIT).contains(&bytes.len()) {
        return Err(ProviderRequestError::Provider);
    }
    let (width, height) = image_dimensions(&bytes).ok_or(ProviderRequestError::Provider)?;
    if width == 0 || height == 0 || (width == 590 && height == 800) {
        return Err(ProviderRequestError::Provider);
    }
    Ok((bytes, f64::from(width) / f64::from(height)))
}

fn try_cover(
    request: ProviderImageRequest,
    fetch_image: &mut impl FnMut(&ProviderImageRequest) -> Result<Vec<u8>, ProviderRequestError>,
) -> Result<Option<(CoverSource, Vec<u8>)>, ProviderRequestError> {
    match fetch_image(&request) {
        Ok(bytes) => {
            let (bytes, aspect_ratio) = validated_cover(bytes)?;
            Ok(Some((
                CoverSource {
                    url: request.url,
                    referer: request.referer,
                    cookie: request.cookie,
                    aspect_ratio,
                },
                bytes,
            )))
        }
        Err(ProviderRequestError::SourceUnavailable) => Ok(None),
        Err(error) => Err(error),
    }
}

fn exact_https_image_url(value: &str, allowed_hosts: &[&str]) -> Option<String> {
    if value
        .bytes()
        .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace() || byte == b'\\')
    {
        return None;
    }
    let remainder = value.strip_prefix("https://")?;
    let (authority, path) = remainder.split_once('/')?;
    if authority.contains(['@', ':'])
        || path.is_empty()
        || path.contains(['?', '#'])
        || !allowed_hosts.contains(&authority)
    {
        return None;
    }
    Some(value.to_owned())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CoverDocumentError {
    Malformed,
    Conflicting,
}

fn tvmaze_image(
    document: &str,
    authority: &LibraryItemAuthority,
    required_imdb: Option<&str>,
) -> Result<Option<String>, CoverDocumentError> {
    let show = JsonParser::new(document)
        .parse()
        .and_then(|value| json_object(&value).cloned())
        .ok_or(CoverDocumentError::Malformed)?;
    let name = optional_text(&show, "name").ok_or(CoverDocumentError::Malformed)?;
    if normalized_title(&name) != normalized_title(&authority.local_title) {
        return Err(CoverDocumentError::Conflicting);
    }
    if let Some(required_imdb) = required_imdb {
        let imdb = show
            .get("externals")
            .and_then(json_object)
            .and_then(|externals| optional_text(externals, "imdb"));
        if imdb.as_deref() != Some(required_imdb) {
            return Err(CoverDocumentError::Conflicting);
        }
    }
    let image = show.get("image").and_then(json_object);
    for key in ["medium", "original"] {
        if let Some(url) = image
            .and_then(|image| optional_text(image, key))
            .and_then(|url| exact_https_image_url(&url, &["static.tvmaze.com"]))
        {
            return Ok(Some(url));
        }
    }
    Ok(None)
}

fn anilist_image(
    document: &str,
    authority: &LibraryItemAuthority,
) -> Result<Option<String>, CoverDocumentError> {
    let root = JsonParser::new(document)
        .parse()
        .and_then(|value| json_object(&value).cloned())
        .ok_or(CoverDocumentError::Malformed)?;
    let media = root
        .get("data")
        .and_then(json_object)
        .and_then(|data| data.get("Media"))
        .and_then(json_object)
        .ok_or(CoverDocumentError::Malformed)?;
    let title = media
        .get("title")
        .and_then(json_object)
        .ok_or(CoverDocumentError::Malformed)?;
    if !["userPreferred", "english", "romaji", "native"]
        .into_iter()
        .filter_map(|key| optional_text(title, key))
        .any(|title| normalized_title(&title) == normalized_title(&authority.local_title))
    {
        return Err(CoverDocumentError::Conflicting);
    }
    let image = media.get("coverImage").and_then(json_object);
    for key in ["large", "medium"] {
        let Some(url) = image.and_then(|image| optional_text(image, key)) else {
            continue;
        };
        let host = url
            .strip_prefix("https://")
            .and_then(|remainder| remainder.split_once('/'))
            .map(|(host, _)| host);
        if host.is_some_and(|host| {
            matches!(
                host,
                "s1.anilist.co" | "s2.anilist.co" | "s3.anilist.co" | "s4.anilist.co"
            )
        }) {
            return Ok(Some(url));
        }
    }
    Ok(None)
}

fn resolve_movie_or_tv_presentation(
    authority: &LibraryItemAuthority,
    token: Option<&str>,
    fetch_text: &mut impl FnMut(&ProviderTextRequest) -> Result<String, ProviderRequestError>,
    fetch_image: &mut impl FnMut(&ProviderImageRequest) -> Result<Vec<u8>, ProviderRequestError>,
) -> Result<(LibraryPresentation, Option<Vec<u8>>, bool), ProviderRequestError> {
    let mut presentation = tmdb_presentation(authority, token, fetch_text)?;
    if !presentation.is_automatic() {
        return Ok((presentation, None, true));
    }
    let mut transient_failure = false;
    let mut bytes = None;
    if let Some(source) = presentation.cover.clone() {
        match try_cover(
            ProviderImageRequest {
                url: source.url,
                referer: source.referer,
                cookie: source.cookie,
            },
            fetch_image,
        ) {
            Ok(Some((source, cover_bytes))) => {
                presentation.aspect_ratio = source.aspect_ratio;
                presentation.cover = Some(source);
                presentation.cover_state = "ready";
                bytes = Some(cover_bytes);
            }
            Ok(None) => presentation.cover = None,
            Err(_) => {
                transient_failure = true;
                presentation.cover = None;
            }
        }
    }
    if authority.category == LibraryCategory::Tv && presentation.cover.is_none() {
        let mut fallback_url = None;
        if let Some(imdb_id) = presentation.imdb_id.as_deref() {
            let request = ProviderTextRequest {
                method: ProviderMethod::Get,
                url: format!(
                    "https://api.tvmaze.com/lookup/shows?imdb={}",
                    percent_encode(imdb_id)
                ),
                accept: "application/json",
                referer: None,
                cookie: None,
                body: None,
            };
            match validated_document(&request, fetch_text) {
                Ok(document) => match tvmaze_image(&document, authority, Some(imdb_id)) {
                    Ok(url) => fallback_url = url,
                    Err(CoverDocumentError::Conflicting) => {
                        return Ok((LibraryPresentation::local_only("missing"), None, true));
                    }
                    Err(CoverDocumentError::Malformed) => transient_failure = true,
                },
                Err(ProviderRequestError::SourceUnavailable) => {}
                Err(_) => transient_failure = true,
            }
        }
        if fallback_url.is_none() {
            let request = ProviderTextRequest {
                method: ProviderMethod::Get,
                url: format!(
                    "https://api.tvmaze.com/singlesearch/shows?q={}",
                    percent_encode(&authority.local_title)
                ),
                accept: "application/json",
                referer: None,
                cookie: None,
                body: None,
            };
            match validated_document(&request, fetch_text) {
                Ok(document) => match tvmaze_image(&document, authority, None) {
                    Ok(url) => fallback_url = url,
                    Err(CoverDocumentError::Conflicting) => {
                        return Ok((LibraryPresentation::local_only("missing"), None, true));
                    }
                    Err(CoverDocumentError::Malformed) => transient_failure = true,
                },
                Err(ProviderRequestError::SourceUnavailable) => {}
                Err(_) => transient_failure = true,
            }
        }
        if fallback_url.is_none() {
            let request = ProviderTextRequest {
                method: ProviderMethod::Post,
                url: "https://graphql.anilist.co/".to_owned(),
                accept: "application/json",
                referer: None,
                cookie: None,
                body: Some(format!(
                    "{{\"query\":\"query($s:String){{Media(search:$s,type:ANIME){{title{{userPreferred english romaji native}} coverImage{{large medium}}}}}}\",\"variables\":{{\"s\":\"{}\"}}}}",
                    authority
                        .local_title
                        .replace('\\', "\\\\")
                        .replace('"', "\\\"")
                )),
            };
            match validated_document(&request, fetch_text) {
                Ok(document) => match anilist_image(&document, authority) {
                    Ok(url) => fallback_url = url,
                    Err(CoverDocumentError::Conflicting) => {
                        return Ok((LibraryPresentation::local_only("missing"), None, true));
                    }
                    Err(CoverDocumentError::Malformed) => transient_failure = true,
                },
                Err(ProviderRequestError::SourceUnavailable) => {}
                Err(_) => transient_failure = true,
            }
        }
        if let Some(url) = fallback_url {
            match try_cover(
                ProviderImageRequest {
                    url,
                    referer: String::new(),
                    cookie: String::new(),
                },
                fetch_image,
            ) {
                Ok(Some((source, cover_bytes))) => {
                    presentation.aspect_ratio = source.aspect_ratio;
                    presentation.cover = Some(source);
                    presentation.cover_state = "ready";
                    bytes = Some(cover_bytes);
                }
                Ok(None) => {}
                Err(_) => transient_failure = true,
            }
        }
    }
    if presentation.cover.is_none() {
        presentation.cover_state = if transient_failure {
            "unavailable"
        } else {
            "missing"
        };
    }
    Ok((presentation, bytes, !transient_failure))
}

fn first_https_url(document: &str, prefix: &str, extensions: &[&str]) -> Option<String> {
    let start = document.find(prefix)?;
    let remainder = &document[start..];
    let end = remainder
        .find(|character: char| {
            character.is_ascii_whitespace() || matches!(character, '"' | '\'' | '<' | '>')
        })
        .unwrap_or(remainder.len());
    let url = remainder[..end].replace("&amp;", "&");
    extensions
        .iter()
        .any(|extension| url.to_ascii_lowercase().contains(extension))
        .then_some(url)
}

fn exact_product_code_in(value: &str, code: &str) -> bool {
    let mut codes = crate::vr_torrent::product_code_candidates(value)
        .into_iter()
        .map(|(candidate, _)| candidate)
        .collect::<Vec<_>>();
    codes.sort();
    codes.dedup();
    codes.as_slice() == [code]
}

fn javdatabase_title(document: &str) -> Option<&str> {
    let lower = document.to_ascii_lowercase();
    let start = lower.find("<title>")?;
    let title = &document[start + "<title>".len()..];
    let end = title.to_ascii_lowercase().find("</title>")?;
    Some(title[..end].trim())
}

fn javdatabase_document_matches(document: &str, code: &str) -> bool {
    javdatabase_title(document).is_some_and(|title| exact_product_code_in(title, code))
}

fn javdatabase_romanized_cast(document: &str, code: &str) -> Option<String> {
    let title = javdatabase_title(document)?;
    let suffix = " - jav database";
    let lower = title.to_ascii_lowercase();
    if !lower.ends_with(suffix) {
        return None;
    }
    let identity_and_cast = &title[..title.len() - suffix.len()];
    let (title_code, cast) = identity_and_cast.split_once(" - ")?;
    let cast = cast.trim();
    (title_code.trim().eq_ignore_ascii_case(code)
        && !cast.is_empty()
        && cast.len() <= 16 * 1024
        && !cast.bytes().any(|byte| byte.is_ascii_control())
        && !cast.to_ascii_lowercase().contains("jav"))
    .then(|| cast.to_owned())
}

fn exact_code_presentation(
    authority: &LibraryItemAuthority,
    fetch_text: &mut impl FnMut(&ProviderTextRequest) -> Result<String, ProviderRequestError>,
    fetch_image: &mut impl FnMut(&ProviderImageRequest) -> Result<Vec<u8>, ProviderRequestError>,
    fetch_javdb: &mut impl FnMut(&str) -> Result<String, ProviderRequestError>,
) -> Result<(LibraryPresentation, Option<Vec<u8>>, bool), ProviderRequestError> {
    let code = authority
        .code
        .as_deref()
        .ok_or(ProviderRequestError::Provider)?;
    if code.starts_with("FC2-") {
        return Ok((LibraryPresentation::local_only("missing"), None, true));
    }
    let mut transient_failure = false;
    let mut cover = None;
    let mut cover_bytes = None;
    let mut accepted_cid = None;
    let mut sources = Vec::new();
    for cid in dmm_cid_variants(code) {
        for floor in ["digital/video", "digital/amateur"] {
            for suffix in ["ps", "jp", "pl"] {
                let request = ProviderImageRequest {
                    url: format!("https://pics.dmm.co.jp/{floor}/{cid}/{cid}{suffix}.jpg"),
                    referer: "https://www.dmm.co.jp/".to_owned(),
                    cookie: String::new(),
                };
                match try_cover(request, fetch_image) {
                    Ok(Some((source, bytes))) => {
                        accepted_cid = Some(cid.clone());
                        sources.push("DMM");
                        cover = Some(source);
                        cover_bytes = Some(bytes);
                        break;
                    }
                    Ok(None) => {}
                    Err(_) => transient_failure = true,
                }
            }
            if cover.is_some() {
                break;
            }
        }
        if cover.is_some() {
            break;
        }
    }

    let r18_url = format!(
        "https://r18.dev/videos/vod/movies/detail/-/dvd_id={}/json",
        percent_encode(code)
    );
    let r18 = validated_document(
        &ProviderTextRequest {
            method: ProviderMethod::Get,
            url: r18_url,
            accept: "application/json",
            referer: Some("https://r18.dev/"),
            cookie: None,
            body: None,
        },
        fetch_text,
    );
    let mut metadata = LibraryPresentation::local_only("unavailable");
    let mut accepted_metadata = false;
    let mut romanized_cast = None;
    if let Ok(document) = &r18 {
        if let Some(root) = JsonParser::new(document)
            .parse()
            .and_then(|value| json_object(&value).cloned())
            .filter(|root| {
                optional_text(root, "content_id")
                    .is_some_and(|content_id| exact_product_code_in(&content_id, code))
            })
        {
            sources.push("r18.dev");
            metadata.title =
                optional_text(&root, "title_ja").or_else(|| optional_text(&root, "title"));
            metadata.date = root.get("release_date").and_then(|value| match value {
                JsonValue::String(value) if valid_date(value) => Some(value.clone()),
                _ => None,
            });
            metadata.runtime = root
                .get("runtime_mins")
                .or_else(|| root.get("runtime_minutes"))
                .and_then(|value| match value {
                    JsonValue::String(value) | JsonValue::Number(value)
                        if value.parse::<u64>().is_ok_and(|minutes| minutes > 0) =>
                    {
                        Some(format!("{} min", value.parse::<u64>().ok()?))
                    }
                    _ => None,
                });
            metadata.cast = r18_actresses(&root);
            accepted_metadata = has_descriptive_metadata(&metadata);
            if cover.is_none() {
                let jacket = root
                    .get("images")
                    .and_then(json_object)
                    .and_then(|images| images.get("jacket_image"))
                    .and_then(json_object);
                for key in ["large", "large2"] {
                    let Some(url) = jacket
                        .and_then(|jacket| json_string(jacket, key))
                        .filter(|url| url.starts_with("https://pics.dmm.co.jp/"))
                    else {
                        continue;
                    };
                    match try_cover(
                        ProviderImageRequest {
                            url: url.to_owned(),
                            referer: "https://www.dmm.co.jp/".to_owned(),
                            cookie: String::new(),
                        },
                        fetch_image,
                    ) {
                        Ok(Some((source, bytes))) => {
                            cover = Some(source);
                            cover_bytes = Some(bytes);
                            break;
                        }
                        Ok(None) => {}
                        Err(_) => transient_failure = true,
                    }
                }
            }
        } else {
            transient_failure = true;
        }
    } else if !matches!(&r18, Err(ProviderRequestError::SourceUnavailable)) {
        transient_failure = true;
    }

    if cover.is_none() {
        for product_id in mgstage_ids(code) {
            let path = format!("/product/product_detail/{product_id}/");
            let document = validated_document(
                &ProviderTextRequest {
                    method: ProviderMethod::Get,
                    url: format!("https://www.mgstage.com{path}"),
                    accept: "text/html",
                    referer: Some("https://www.mgstage.com/"),
                    cookie: Some("adc=1"),
                    body: None,
                },
                fetch_text,
            );
            let document = match document {
                Ok(document) => document,
                Err(ProviderRequestError::SourceUnavailable) => continue,
                Err(_) => {
                    transient_failure = true;
                    continue;
                }
            };
            if !document.contains(&path) {
                continue;
            }
            let Some(url) =
                first_https_url(&document, "https://image.mgstage.com/images/", &[".jpg"])
            else {
                continue;
            };
            match try_cover(
                ProviderImageRequest {
                    url,
                    referer: "https://www.mgstage.com/".to_owned(),
                    cookie: "adc=1".to_owned(),
                },
                fetch_image,
            ) {
                Ok(Some((source, bytes))) => {
                    sources.push("MGStage");
                    cover = Some(source);
                    cover_bytes = Some(bytes);
                    break;
                }
                Ok(None) => {}
                Err(_) => transient_failure = true,
            }
        }
    }

    let database_url = format!(
        "https://www.javdatabase.com/movies/{}/",
        code.to_ascii_lowercase()
    );
    let database = validated_document(
        &ProviderTextRequest {
            method: ProviderMethod::Get,
            url: database_url,
            accept: "text/html",
            referer: Some("https://www.javdatabase.com/"),
            cookie: None,
            body: None,
        },
        fetch_text,
    );
    if let Ok(document) = &database {
        if !javdatabase_document_matches(document, code) {
            return Err(ProviderRequestError::Provider);
        }
        sources.push("JavDatabase");
        if metadata.date.is_none() {
            metadata.date = find_date(document);
        }
        if metadata.runtime.is_none() {
            metadata.runtime = find_runtime(document);
        }
        romanized_cast = javdatabase_romanized_cast(document, code);
        accepted_metadata = accepted_metadata
            || metadata.date.is_some()
            || metadata.runtime.is_some()
            || romanized_cast.is_some();
    } else if !matches!(&database, Err(ProviderRequestError::SourceUnavailable)) {
        transient_failure = true;
    }

    if cover.is_none() {
        if let Ok(document) = &database {
            let url =
                first_https_url(document, "https://pics.dmm.co.jp/", &[".jpg"]).or_else(|| {
                    first_https_url(document, "https://www.javdatabase.com/covers/", &[".webp"])
                });
            if let Some(url) = url {
                let referer = if url.starts_with("https://pics.dmm.co.jp/") {
                    "https://www.dmm.co.jp/"
                } else {
                    "https://www.javdatabase.com/"
                };
                match try_cover(
                    ProviderImageRequest {
                        url,
                        referer: referer.to_owned(),
                        cookie: String::new(),
                    },
                    fetch_image,
                ) {
                    Ok(Some((source, bytes))) => {
                        cover = Some(source);
                        cover_bytes = Some(bytes);
                    }
                    Ok(None) => {}
                    Err(_) => transient_failure = true,
                }
            }
        }
    }

    if let Some(cid) = accepted_cid {
        if metadata.cast.is_empty() || metadata.title.is_none() {
            let combined = validated_document(
                &ProviderTextRequest {
                    method: ProviderMethod::Get,
                    url: format!("https://r18.dev/videos/vod/movies/detail/-/combined={cid}/json"),
                    accept: "application/json",
                    referer: Some("https://r18.dev/"),
                    cookie: None,
                    body: None,
                },
                fetch_text,
            );
            if let Ok(combined) = combined {
                if let Some(root) = JsonParser::new(&combined)
                    .parse()
                    .and_then(|value| json_object(&value).cloned())
                    .filter(|root| {
                        optional_text(root, "content_id").is_some_and(|content_id| {
                            content_id == cid && exact_product_code_in(&content_id, code)
                        })
                    })
                {
                    sources.push("r18.dev");
                    metadata.title = metadata.title.or_else(|| optional_text(&root, "title_ja"));
                    if metadata.cast.is_empty() {
                        metadata.cast = r18_actresses(&root);
                    }
                }
            }
        }
    }

    if metadata.title.is_none() || metadata.cast.is_empty() {
        match fetch_exact_library_metadata_with(authority.category.value(), code, fetch_javdb) {
            Ok(javdb) => {
                sources.push("JavDB");
                accepted_metadata = accepted_metadata
                    || javdb.title.is_some()
                    || javdb.release_date.is_some()
                    || javdb.duration.is_some()
                    || !javdb.actors.is_empty();
                metadata.title = metadata.title.or(javdb.title);
                if metadata.cast.is_empty() {
                    metadata.cast = javdb.actors;
                }
                metadata.date = metadata.date.or(javdb.release_date);
                metadata.runtime = metadata.runtime.or(javdb.duration);
                metadata.provider_id = Some(javdb.provider_item_id);
            }
            Err(ProviderRequestError::SourceUnavailable) => {}
            Err(_) => transient_failure = true,
        }
    }
    if metadata.cast.is_empty() {
        if let Some(cast) = romanized_cast {
            metadata.cast.push(cast);
        }
    }
    if cover.is_none() && !accepted_metadata {
        return if transient_failure {
            Err(ProviderRequestError::Network)
        } else {
            Ok((LibraryPresentation::local_only("missing"), None, true))
        };
    }
    let mut unique_sources = HashSet::new();
    sources.retain(|source| unique_sources.insert(*source));
    metadata.source = Some(sources.join(" + "));
    metadata.provider_id = metadata.provider_id.or_else(|| Some(code.to_owned()));
    metadata.cover = cover;
    metadata.aspect_ratio = metadata
        .cover
        .as_ref()
        .map_or(DEFAULT_POSTER_ASPECT, |cover| cover.aspect_ratio);
    metadata.cover_state = if metadata.cover.is_some() {
        "ready"
    } else if transient_failure {
        "unavailable"
    } else {
        "missing"
    };
    Ok((metadata, cover_bytes, !transient_failure))
}

fn find_date(document: &str) -> Option<String> {
    document.as_bytes().windows(10).find_map(|window| {
        let value = std::str::from_utf8(window).ok()?;
        valid_date(value).then(|| value.to_owned())
    })
}

fn find_runtime(document: &str) -> Option<String> {
    let lower = document.to_ascii_lowercase();
    for (index, _) in lower.match_indices(" min") {
        let start = lower[..index]
            .rfind(|character: char| !character.is_ascii_digit())
            .map_or(0, |position| position + 1);
        let value = &lower[start..index];
        if value
            .parse::<u64>()
            .is_ok_and(|minutes| (1..=10_000).contains(&minutes))
        {
            return Some(format!("{value} min"));
        }
    }
    None
}

fn mgstage_ids(code: &str) -> Vec<String> {
    let Some((prefix, number)) = code.split_once('-') else {
        return vec![code.to_owned()];
    };
    let normalized = number.parse::<u64>().ok();
    let mut values = vec![code.to_owned()];
    if let Some(number) = normalized {
        values.push(format!("{prefix}-{number:03}"));
        values.push(format!("{prefix}-{number}"));
    }
    let mut unique = HashSet::new();
    values.retain(|value| unique.insert(value.clone()));
    values
}

fn encode_text(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(value.len() * 2);
    for byte in value.as_bytes() {
        output.push(HEX[usize::from(byte >> 4)] as char);
        output.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    output
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
    let mut entries = Vec::new();
    let mut identities = HashSet::new();
    for line in lines {
        let fields = line.split('\t').collect::<Vec<_>>();
        if fields.len() < 20 {
            return Err(());
        }
        let identity = decode_text(fields[0]).ok_or(())?;
        if !identities.insert(identity.clone()) {
            return Err(());
        }
        let metadata_saved_at = fields[1].parse().map_err(|_| ())?;
        let cover_saved_at = fields[2].parse().map_err(|_| ())?;
        let genre_count: usize = fields[18].parse().map_err(|_| ())?;
        let genre_start = 19;
        let cast_count_index = genre_start + genre_count;
        let cast_count: usize = fields
            .get(cast_count_index)
            .ok_or(())?
            .parse()
            .map_err(|_| ())?;
        let cast_start = cast_count_index + 1;
        if fields.len() != cast_start + cast_count {
            return Err(());
        }
        let cover_url = decode_text(fields[13]).ok_or(())?;
        let cover = if cover_url.is_empty() {
            None
        } else {
            Some(CoverSource {
                url: cover_url,
                referer: decode_text(fields[14]).ok_or(())?,
                cookie: decode_text(fields[15]).ok_or(())?,
                aspect_ratio: fields[17].parse().map_err(|_| ())?,
            })
        };
        let presentation = LibraryPresentation {
            source: nonempty(decode_text(fields[3]).ok_or(())?),
            provider_id: nonempty(decode_text(fields[4]).ok_or(())?),
            imdb_id: nonempty(decode_text(fields[5]).ok_or(())?),
            title: nonempty(decode_text(fields[6]).ok_or(())?),
            original_title: nonempty(decode_text(fields[7]).ok_or(())?),
            date: nonempty(decode_text(fields[8]).ok_or(())?),
            runtime: nonempty(decode_text(fields[9]).ok_or(())?),
            overview: nonempty(decode_text(fields[10]).ok_or(())?),
            cover,
            cover_state: match fields[16] {
                "ready" => "ready",
                "missing" => "missing",
                "unavailable" => "unavailable",
                _ => return Err(()),
            },
            aspect_ratio: fields[17].parse().map_err(|_| ())?,
            genres: fields[genre_start..cast_count_index]
                .iter()
                .map(|value| decode_text(value).ok_or(()))
                .collect::<Result<Vec<_>, _>>()?,
            cast: fields[cast_start..]
                .iter()
                .map(|value| decode_text(value).ok_or(()))
                .collect::<Result<Vec<_>, _>>()?,
        };
        entries.push(CacheEntry {
            identity,
            metadata_saved_at,
            cover_saved_at,
            presentation,
        });
    }
    Ok(entries)
}

fn nonempty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
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
        let presentation = &entry.presentation;
        let cover = presentation.cover.as_ref();
        let mut fields = vec![
            encode_text(&entry.identity),
            entry.metadata_saved_at.to_string(),
            entry.cover_saved_at.to_string(),
            encode_text(presentation.source.as_deref().unwrap_or("")),
            encode_text(presentation.provider_id.as_deref().unwrap_or("")),
            encode_text(presentation.imdb_id.as_deref().unwrap_or("")),
            encode_text(presentation.title.as_deref().unwrap_or("")),
            encode_text(presentation.original_title.as_deref().unwrap_or("")),
            encode_text(presentation.date.as_deref().unwrap_or("")),
            encode_text(presentation.runtime.as_deref().unwrap_or("")),
            encode_text(presentation.overview.as_deref().unwrap_or("")),
            "".to_owned(),
            "".to_owned(),
            encode_text(cover.map_or("", |cover| &cover.url)),
            encode_text(cover.map_or("", |cover| &cover.referer)),
            encode_text(cover.map_or("", |cover| &cover.cookie)),
            presentation.cover_state.to_owned(),
            presentation.aspect_ratio.to_string(),
            presentation.genres.len().to_string(),
        ];
        fields.extend(presentation.genres.iter().map(|value| encode_text(value)));
        fields.push(presentation.cast.len().to_string());
        fields.extend(presentation.cast.iter().map(|value| encode_text(value)));
        writeln!(file, "{}", fields.join("\t")).map_err(|_| ())?;
    }
    file.sync_all().map_err(|_| ())?;
    #[cfg(target_os = "windows")]
    if path.exists() {
        fs::remove_file(path).map_err(|_| ())?;
    }
    fs::rename(replacement, path).map_err(|_| ())
}

fn encode_presentation(
    state: &LibraryEnrichmentState,
    authority: &LibraryItemAuthority,
    presentation: LibraryPresentation,
    bytes: Option<Vec<u8>>,
) -> Result<Vec<String>, &'static str> {
    let cover_authority_id = presentation.cover.as_ref().map(|cover| {
        format!(
            "library-cover-{}",
            hex_sha1(format!("{}\0{}", authority.identity, cover.url).as_bytes())
        )
    });
    if let (Some(id), Some(source)) = (cover_authority_id.as_ref(), presentation.cover.as_ref()) {
        state
            .0
            .lock()
            .map_err(|_| LIBRARY_ENRICHMENT_FAILED)?
            .covers
            .insert(
                id.clone(),
                CoverAuthority {
                    item_identity: authority.identity.clone(),
                    source: source.clone(),
                    bytes,
                },
            );
    }
    let mut response = vec![
        "library-enrichment-v1".to_owned(),
        authority.category.value().to_owned(),
        if presentation.is_automatic() {
            "automatic".to_owned()
        } else {
            "local-only".to_owned()
        },
        presentation.source.clone().unwrap_or_default(),
        presentation.provider_id.clone().unwrap_or_default(),
        presentation.imdb_id.clone().unwrap_or_default(),
        presentation.title.clone().unwrap_or_default(),
        presentation.original_title.clone().unwrap_or_default(),
        presentation.date.clone().unwrap_or_default(),
        presentation.runtime.clone().unwrap_or_default(),
        presentation.overview.clone().unwrap_or_default(),
        cover_authority_id.unwrap_or_default(),
        presentation.cover_state.to_owned(),
        presentation.aspect_ratio.to_string(),
        presentation.genres.len().to_string(),
    ];
    response.extend(presentation.genres);
    response.push(presentation.cast.len().to_string());
    response.extend(presentation.cast);
    Ok(response)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn fetch_presentation_with(
    state: &LibraryEnrichmentState,
    cache_path: &Path,
    authority: &LibraryItemAuthority,
    tmdb_token: Option<&str>,
    mut fetch_text: impl FnMut(&ProviderTextRequest) -> Result<String, ProviderRequestError>,
    mut fetch_image: impl FnMut(&ProviderImageRequest) -> Result<Vec<u8>, ProviderRequestError>,
    mut fetch_javdb: impl FnMut(&str) -> Result<String, ProviderRequestError>,
) -> Result<Vec<String>, &'static str> {
    let now = now_seconds();
    let cache = {
        let _cache_guard = state.0.lock().map_err(|_| LIBRARY_ENRICHMENT_FAILED)?;
        read_cache(cache_path).unwrap_or_default()
    };
    if let Some(entry) = cache.iter().find(|entry| {
        entry.identity == authority.identity
            && now.saturating_sub(entry.metadata_saved_at) <= METADATA_TTL_SECONDS
            && now.saturating_sub(entry.cover_saved_at) <= COVER_TTL_SECONDS
    }) {
        return encode_presentation(state, authority, entry.presentation.clone(), None);
    }
    let cached = cache
        .iter()
        .find(|entry| entry.identity == authority.identity)
        .cloned();
    let cached_metadata = cached
        .as_ref()
        .filter(|entry| now.saturating_sub(entry.metadata_saved_at) <= METADATA_TTL_SECONDS)
        .cloned();
    let resolved = match authority.category {
        LibraryCategory::Movie | LibraryCategory::Tv => resolve_movie_or_tv_presentation(
            authority,
            tmdb_token,
            &mut fetch_text,
            &mut fetch_image,
        ),
        LibraryCategory::Adult | LibraryCategory::Vr => exact_code_presentation(
            authority,
            &mut fetch_text,
            &mut fetch_image,
            &mut fetch_javdb,
        ),
    };
    let (mut presentation, bytes, cache_cover) = match resolved {
        Ok(resolved) => resolved,
        Err(_) if cached_metadata.is_some() => {
            let mut cached = cached_metadata
                .expect("the guarded cache entry must contain fresh metadata")
                .presentation;
            cached.cover = None;
            cached.cover_state = "unavailable";
            return encode_presentation(state, authority, cached, None);
        }
        Err(_) => return Err(LIBRARY_ENRICHMENT_FAILED),
    };
    let metadata_saved_at = if let Some(cached) = cached_metadata {
        if !cache_cover {
            let mut retained = cached.presentation;
            retained.cover = None;
            retained.cover_state = presentation.cover_state;
            return encode_presentation(state, authority, retained, None);
        }
        let mut retained = cached.presentation;
        retained.cover = presentation.cover;
        retained.cover_state = presentation.cover_state;
        retained.aspect_ratio = presentation.aspect_ratio;
        presentation = retained;
        cached.metadata_saved_at
    } else {
        now
    };
    let confirmed_exact_code_miss = matches!(
        authority.category,
        LibraryCategory::Adult | LibraryCategory::Vr
    ) && authority
        .code
        .as_deref()
        .is_some_and(|code| !code.starts_with("FC2-"))
        && presentation.cover_state == "missing"
        && !presentation.is_automatic()
        && cache_cover;
    if presentation.is_automatic() || confirmed_exact_code_miss {
        let _cache_guard = state.0.lock().map_err(|_| LIBRARY_ENRICHMENT_FAILED)?;
        let mut cache = read_cache(cache_path).unwrap_or_default();
        cache.retain(|entry| entry.identity != authority.identity);
        cache.push(CacheEntry {
            identity: authority.identity.clone(),
            metadata_saved_at,
            cover_saved_at: if cache_cover { now } else { 0 },
            presentation: presentation.clone(),
        });
        cache.sort_by(|left, right| left.identity.cmp(&right.identity));
        let _ = write_cache(cache_path, &cache);
    }
    encode_presentation(state, authority, presentation, bytes)
}

pub(crate) fn fetch_cover_with(
    state: &LibraryEnrichmentState,
    authority: &LibraryItemAuthority,
    cover_authority_id: &str,
    mut fetch: impl FnMut(&ProviderImageRequest) -> Result<Vec<u8>, ProviderRequestError>,
) -> Result<Vec<u8>, &'static str> {
    let cover = state
        .0
        .lock()
        .map_err(|_| LIBRARY_COVER_STALE)?
        .covers
        .get(cover_authority_id)
        .filter(|cover| cover.item_identity == authority.identity)
        .cloned()
        .ok_or(LIBRARY_COVER_STALE)?;
    let bytes = if let Some(bytes) = cover.bytes {
        validated_cover(bytes)
    } else {
        validated_cover(
            fetch(&ProviderImageRequest {
                url: cover.source.url,
                referer: cover.source.referer,
                cookie: cover.source.cookie,
            })
            .map_err(|_| LIBRARY_ENRICHMENT_FAILED)?,
        )
    }
    .map(|(bytes, _)| bytes)
    .map_err(|_| LIBRARY_ENRICHMENT_FAILED)?;
    state
        .0
        .lock()
        .map_err(|_| LIBRARY_COVER_STALE)?
        .covers
        .remove(cover_authority_id);
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::sync::atomic::{AtomicU64, Ordering};

    static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct CacheFixture {
        directory: std::path::PathBuf,
        path: std::path::PathBuf,
    }

    impl CacheFixture {
        fn new() -> Self {
            let directory = std::env::temp_dir().join(format!(
                "auto-video-library-enrichment-{}-{}",
                std::process::id(),
                FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&directory).expect("cache fixture directory must exist");
            let path = directory.join("cache");
            Self { directory, path }
        }
    }

    impl Drop for CacheFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }

    fn jpeg(width: u16, height: u16) -> Vec<u8> {
        let mut bytes = vec![0_u8; COVER_MIN_BYTES];
        bytes[..19].copy_from_slice(&[
            0xff,
            0xd8,
            0xff,
            0xc0,
            0x00,
            0x11,
            0x08,
            (height >> 8) as u8,
            height as u8,
            (width >> 8) as u8,
            width as u8,
            0x03,
            0x01,
            0x22,
            0x00,
            0x02,
            0x11,
            0x01,
            0x03,
        ]);
        bytes
    }

    #[test]
    fn rejects_the_known_placeholder_and_non_raster_cover_bytes() {
        assert_eq!(
            validated_cover(jpeg(590, 800)),
            Err(ProviderRequestError::Provider)
        );
        assert_eq!(
            validated_cover(vec![1; COVER_MIN_BYTES]),
            Err(ProviderRequestError::Provider)
        );
        let mut lossy_webp = vec![0_u8; COVER_MIN_BYTES];
        lossy_webp[..16].copy_from_slice(b"RIFF\0\0\0\0WEBPVP8 ");
        lossy_webp[26..30].copy_from_slice(&[0x90, 0x01, 0x58, 0x02]);
        assert_eq!(image_dimensions(&lossy_webp), Some((400, 600)));

        let mut lossless_webp = vec![0_u8; COVER_MIN_BYTES];
        lossless_webp[..16].copy_from_slice(b"RIFF\0\0\0\0WEBPVP8L");
        let bits = (399_u32) | (599_u32 << 14);
        lossless_webp[21..25].copy_from_slice(&bits.to_le_bytes());
        assert_eq!(image_dimensions(&lossless_webp), Some((400, 600)));
    }

    #[test]
    fn transport_boundaries_reject_unapproved_urls_redirect_statuses_and_oversized_bodies() {
        let valid_text = ProviderTextRequest {
            method: ProviderMethod::Get,
            url: "https://api.themoviedb.org/3/search/movie?query=Exact".to_owned(),
            accept: "application/json",
            referer: None,
            cookie: None,
            body: None,
        };
        assert!(valid_text_request(&valid_text));
        for url in [
            "http://api.themoviedb.org/3/search/movie",
            "https://user@api.themoviedb.org/3/search/movie",
            "https://api.themoviedb.org:443/3/search/movie",
            "https://api.themoviedb.org.evil.example/3/search/movie",
            "https://api.themoviedb.org\\evil/3/search/movie",
        ] {
            let mut request = valid_text.clone();
            request.url = url.to_owned();
            assert!(!valid_text_request(&request));
        }
        let valid_image = ProviderImageRequest {
            url: "https://pics.dmm.co.jp/digital/video/exact/exactps.jpg".to_owned(),
            referer: "https://www.dmm.co.jp/".to_owned(),
            cookie: String::new(),
        };
        assert!(valid_image_request(&valid_image));
        let mut wrong_referer = valid_image.clone();
        wrong_referer.referer = "https://evil.example/".to_owned();
        assert!(!valid_image_request(&wrong_referer));

        let unicode = "{\"title\":\"日本語\"}";
        let framed = format!("{unicode}{HTTP_STATUS_MARKER}200");
        assert_eq!(
            parse_http_response(framed.as_bytes(), unicode.len()).unwrap(),
            unicode.as_bytes()
        );
        assert_eq!(
            parse_http_response(b"gone\nAUTO_VIDEO_HTTP_STATUS:302", 100),
            Err(ProviderRequestError::Provider)
        );
        assert_eq!(
            parse_http_response(b"12345\nAUTO_VIDEO_HTTP_STATUS:200", 4),
            Err(ProviderRequestError::Provider)
        );
    }

    #[test]
    fn windows_transports_bound_headers_and_each_body_read() {
        for script in [WINDOWS_TEXT_SCRIPT, WINDOWS_IMAGE_SCRIPT] {
            assert!(script.contains("AllowAutoRedirect = $false"));
            assert!(script.contains("ResponseHeadersRead, $deadline.Token"));
            assert!(script.contains("ReadAsync($buffer, 0, $buffer.Length, $deadline.Token)"));
            assert!(script.contains("CancelAfter(20000)"));
            assert!(script.contains("$ErrorActionPreference = 'Stop'"));
        }
        assert!(WINDOWS_TEXT_SCRIPT.contains("4194304"));
        assert!(WINDOWS_IMAGE_SCRIPT.contains("16777216"));
        assert!(WINDOWS_IMAGE_SCRIPT.contains("ToBase64String($memory.ToArray())"));
    }

    #[test]
    fn movie_title_matching_composes_nfd_without_changing_the_local_identity_or_query() {
        let local_title = "\u{30ab}\u{3099}\u{30f3}\u{30c0}\u{30e0}";
        let authority = LibraryItemAuthority {
            category: LibraryCategory::Movie,
            identity: "nfd-movie".to_owned(),
            local_title: local_title.to_owned(),
            year: Some("1990".to_owned()),
            code: None,
        };
        let mut requests = Vec::new();
        let presentation = tmdb_presentation(&authority, Some("token"), &mut |request| {
            requests.push(request.url.clone());
            if request.url.contains("search/movie") {
                Ok(r#"{"results":[{"id":11,"title":"ガンダム","release_date":"1990-01-01"}]}"#.to_owned())
            } else {
                Ok(r#"{"id":11,"title":"ガンダム","original_title":"ガンダム","release_date":"1990-01-01","genres":[],"credits":{"cast":[]},"external_ids":{},"poster_path":null}"#.to_owned())
            }
        })
        .expect("canonically equivalent movie titles must match");

        assert!(presentation.is_automatic());
        assert_eq!(presentation.title.as_deref(), Some("ガンダム"));
        assert_eq!(authority.local_title, local_title);
        assert!(requests[0].contains("query=%E3%82%AB%E3%82%99"));
    }

    #[test]
    fn grouped_tv_title_matching_composes_nfd_without_changing_the_local_identity_or_query() {
        let local_title = "\u{30cf}\u{309a}\u{30d2}\u{309a}\u{30e8}\u{30f3}";
        let authority = LibraryItemAuthority {
            category: LibraryCategory::Tv,
            identity: "nfd-tv-group".to_owned(),
            local_title: local_title.to_owned(),
            year: None,
            code: None,
        };
        let mut requests = Vec::new();
        let presentation = tmdb_presentation(&authority, Some("token"), &mut |request| {
            requests.push(request.url.clone());
            if request.url.contains("search/tv") {
                Ok(r#"{"results":[{"id":12,"name":"パピヨン","first_air_date":"2003-01-01"}]}"#.to_owned())
            } else {
                Ok(r#"{"id":12,"name":"パピヨン","original_name":"パピヨン","first_air_date":"2003-01-01","genres":[],"credits":{"cast":[]},"external_ids":{},"poster_path":null}"#.to_owned())
            }
        })
        .expect("canonically equivalent grouped TV titles must match");

        assert!(presentation.is_automatic());
        assert_eq!(presentation.title.as_deref(), Some("パピヨン"));
        assert_eq!(authority.local_title, local_title);
        assert!(requests[0].contains("query=%E3%83%8F%E3%82%9A"));
    }

    #[test]
    fn movie_search_retries_once_without_year_after_a_confirmed_empty_result() {
        let authority = LibraryItemAuthority {
            category: LibraryCategory::Movie,
            identity: "bare-title-fallback".to_owned(),
            local_title: "パーフェクトブルー".to_owned(),
            year: Some("1998".to_owned()),
            code: None,
        };
        let mut requests = Vec::new();
        let presentation = tmdb_presentation(&authority, Some("token"), &mut |request| {
            requests.push(request.url.clone());
            match requests.len() {
                1 => Ok(r#"{"results":[]}"#.to_owned()),
                2 => Ok(r#"{"results":[{"id":10494,"title":"Perfect Blue","release_date":"1998-02-28"}]}"#.to_owned()),
                3 => Ok(r#"{"id":10494,"title":"Perfect Blue","original_title":"PERFECT BLUE","release_date":"1998-02-28","genres":[],"credits":{"cast":[]},"external_ids":{"imdb_id":"tt0156887"},"poster_path":null}"#.to_owned()),
                _ => panic!("the bounded fallback must not dispatch again"),
            }
        })
        .expect("the bare-title fallback must resolve one exact result");

        assert!(presentation.is_automatic());
        assert_eq!(presentation.provider_id.as_deref(), Some("10494"));
        assert!(requests[0].contains("&year=1998"));
        assert!(requests[1].contains("search/movie"));
        assert!(!requests[1].contains("&year="));
        assert!(requests[2].contains("/movie/10494?"));
    }

    #[test]
    fn movie_search_does_not_fallback_after_a_first_attempt_acceptance_or_conflict() {
        let authority = LibraryItemAuthority {
            category: LibraryCategory::Movie,
            identity: "bounded-movie-search".to_owned(),
            local_title: "Exact Movie".to_owned(),
            year: Some("1999".to_owned()),
            code: None,
        };
        let mut accepted_requests = Vec::new();
        let accepted = tmdb_presentation(&authority, Some("token"), &mut |request| {
            accepted_requests.push(request.url.clone());
            if request.url.contains("search/movie") {
                Ok(r#"{"results":[{"id":1,"title":"Exact Movie","release_date":"1999-01-01"}]}"#.to_owned())
            } else {
                Ok(r#"{"id":1,"title":"Exact Movie","release_date":"1999-01-01","genres":[],"credits":{"cast":[]},"external_ids":{},"poster_path":null}"#.to_owned())
            }
        })
        .expect("the first accepted result must resolve");
        assert!(accepted.is_automatic());
        assert_eq!(
            accepted_requests
                .iter()
                .filter(|url| url.contains("search/movie"))
                .count(),
            1
        );

        let mut conflict_requests = Vec::new();
        let conflict = tmdb_presentation(&authority, Some("token"), &mut |request| {
            conflict_requests.push(request.url.clone());
            Ok(
                r#"{"results":[{"id":2,"title":"Exact Movie","release_date":"2005-01-01"}]}"#
                    .to_owned(),
            )
        })
        .expect("a conflicting first result must remain local");
        assert!(!conflict.is_automatic());
        assert_eq!(conflict_requests.len(), 1);
    }

    #[test]
    fn movie_search_two_confirmed_misses_are_local_only_and_provider_failures_remain_errors() {
        let authority = LibraryItemAuthority {
            category: LibraryCategory::Movie,
            identity: "missing-movie".to_owned(),
            local_title: "Missing Movie".to_owned(),
            year: Some("1999".to_owned()),
            code: None,
        };
        let mut calls = 0;
        let missing = tmdb_presentation(&authority, Some("token"), &mut |_| {
            calls += 1;
            Ok(r#"{"results":[]}"#.to_owned())
        })
        .expect("two confirmed misses must be an honest local-only result");
        assert_eq!(calls, 2);
        assert!(!missing.is_automatic());

        let mut failure_calls = 0;
        let failure = tmdb_presentation(&authority, Some("token"), &mut |_| {
            failure_calls += 1;
            Err(ProviderRequestError::Network)
        });
        assert_eq!(failure_calls, 1);
        assert_eq!(failure, Err(ProviderRequestError::Network));

        let mut malformed_calls = 0;
        let malformed = tmdb_presentation(&authority, Some("token"), &mut |_| {
            malformed_calls += 1;
            Ok("{}".to_owned())
        });
        assert_eq!(malformed_calls, 1);
        assert_eq!(malformed, Err(ProviderRequestError::Provider));
    }

    #[test]
    fn movie_matching_requires_one_exact_title_and_year_identity() {
        let authority = LibraryItemAuthority {
            category: LibraryCategory::Movie,
            identity: "fixture".to_owned(),
            local_title: "GoodFellas".to_owned(),
            year: Some("1990".to_owned()),
            code: None,
        };
        let mut responses = vec![
            r#"{"results":[{"id":769,"title":"GoodFellas","release_date":"1990-09-12"}]}"#
                .to_owned(),
            r#"{"id":769,"title":"GoodFellas","original_title":"GoodFellas","release_date":"1990-09-12","runtime":145,"genres":[{"name":"Drama"}],"credits":{"cast":[{"name":"Robert De Niro"}]},"external_ids":{"imdb_id":"tt0099685"},"poster_path":"/poster.jpg","overview":"Fixture overview"}"#
                .to_owned(),
        ]
        .into_iter();
        let result = tmdb_presentation(&authority, Some("token"), &mut |_| {
            Ok(responses.next().expect("fixture response"))
        })
        .expect("presentation must parse");
        assert_eq!(result.provider_id.as_deref(), Some("769"));
        assert_eq!(result.imdb_id.as_deref(), Some("tt0099685"));
        assert_eq!(result.runtime.as_deref(), Some("145 min"));
        assert_eq!(result.genres, ["Drama"]);
    }

    #[test]
    fn movie_year_anchor_accepts_one_consistent_romanized_result_and_preserves_its_title() {
        let authority = LibraryItemAuthority {
            category: LibraryCategory::Movie,
            identity: "romanized-movie".to_owned(),
            local_title: "パーフェクトブルー".to_owned(),
            year: Some("1998".to_owned()),
            code: None,
        };
        let mut responses = vec![
            r#"{"results":[{"id":10494,"title":"Perfect Blue","original_title":"PERFECT BLUE","release_date":"1998-02-28"}]}"#.to_owned(),
            r#"{"id":10494,"title":"Perfect Blue","original_title":"PERFECT BLUE","release_date":"1998-02-28","runtime":81,"genres":[{"name":"Animation"}],"credits":{"cast":[]},"external_ids":{"imdb_id":"tt0156887"},"poster_path":null}"#.to_owned(),
        ]
        .into_iter();

        let presentation = tmdb_presentation(&authority, Some("token"), &mut |_| {
            Ok(responses.next().expect("fixture response"))
        })
        .expect("one year-anchored result must resolve");

        assert!(presentation.is_automatic());
        assert_eq!(presentation.provider_id.as_deref(), Some("10494"));
        assert_eq!(presentation.title.as_deref(), Some("Perfect Blue"));
        assert_ne!(
            presentation.title.as_deref(),
            Some(authority.local_title.as_str())
        );
    }

    #[test]
    fn movie_year_anchor_keeps_ambiguous_and_conflicting_results_local_only() {
        let authority = LibraryItemAuthority {
            category: LibraryCategory::Movie,
            identity: "ambiguous-romanized-movie".to_owned(),
            local_title: "日本語題名".to_owned(),
            year: Some("1998".to_owned()),
            code: None,
        };
        let mut calls = 0;
        let ambiguous = tmdb_presentation(&authority, Some("token"), &mut |_| {
            calls += 1;
            Ok(r#"{"results":[{"id":1,"title":"First Romanization","release_date":"1998-01-01"},{"id":2,"title":"Second Romanization","release_date":"1998-02-01"}]}"#.to_owned())
        })
        .expect("ambiguous year matches must remain local");
        assert_eq!(calls, 1);
        assert!(!ambiguous.is_automatic());

        let mut conflicting_year = vec![
            r#"{"results":[{"id":3,"title":"Romanized Title","release_date":"1998-01-01"}]}"#.to_owned(),
            r#"{"id":3,"title":"Romanized Title","release_date":"2001-01-01","genres":[],"credits":{"cast":[]},"external_ids":{},"poster_path":null}"#.to_owned(),
        ]
        .into_iter();
        let year_conflict = tmdb_presentation(&authority, Some("token"), &mut |_| {
            Ok(conflicting_year.next().expect("fixture response"))
        })
        .expect("a conflicting detail year must remain local");
        assert!(!year_conflict.is_automatic());

        let mut conflicting_id = vec![
            r#"{"results":[{"id":4,"title":"Romanized Title","release_date":"1998-01-01"}]}"#.to_owned(),
            r#"{"id":5,"title":"Romanized Title","release_date":"1998-01-01","genres":[],"credits":{"cast":[]},"external_ids":{},"poster_path":null}"#.to_owned(),
        ]
        .into_iter();
        let identity_conflict = tmdb_presentation(&authority, Some("token"), &mut |_| {
            Ok(conflicting_id.next().expect("fixture response"))
        })
        .expect("a conflicting detail identity must remain local");
        assert!(!identity_conflict.is_automatic());
    }

    #[test]
    fn missing_tmdb_token_is_local_only_without_network_dispatch() {
        let authority = LibraryItemAuthority {
            category: LibraryCategory::Tv,
            identity: "fixture".to_owned(),
            local_title: "Fixture Show".to_owned(),
            year: None,
            code: None,
        };
        let presentation = tmdb_presentation(&authority, None, &mut |_| {
            panic!("missing tokens must not dispatch")
        })
        .expect("missing token is an honest local state");
        assert!(!presentation.is_automatic());
    }

    #[test]
    fn ambiguous_movie_results_remain_local_without_detail_dispatch() {
        let authority = LibraryItemAuthority {
            category: LibraryCategory::Movie,
            identity: "fixture".to_owned(),
            local_title: "Exact Movie".to_owned(),
            year: Some("1999".to_owned()),
            code: None,
        };
        let mut calls = 0;
        let presentation = tmdb_presentation(&authority, Some("token"), &mut |_| {
            calls += 1;
            Ok(r#"{"results":[{"id":1,"title":"Exact Movie","release_date":"1999-01-01"},{"id":2,"title":"Exact Movie","release_date":"1999-02-01"}]}"#.to_owned())
        })
        .expect("an ambiguous search is a local-only result");
        assert_eq!(calls, 1);
        assert!(!presentation.is_automatic());
    }

    #[test]
    fn tv_cover_fallback_uses_imdb_then_title_before_anilist() {
        let authority = LibraryItemAuthority {
            category: LibraryCategory::Tv,
            identity: "fixture".to_owned(),
            local_title: "Exact Show".to_owned(),
            year: None,
            code: None,
        };
        let mut requests = Vec::new();
        let (presentation, bytes, cache_cover) = resolve_movie_or_tv_presentation(
            &authority,
            Some("token"),
            &mut |request| {
                requests.push(request.url.clone());
                if request.url.contains("search/tv") {
                    Ok(r#"{"results":[{"id":7,"name":"Exact Show","first_air_date":"2020-01-01"}]}"#.to_owned())
                } else if request.url.contains("/tv/7?") {
                    Ok(r#"{"id":7,"name":"Exact Show","first_air_date":"2020-01-01","genres":[],"credits":{"cast":[]},"external_ids":{"imdb_id":"tt1234567"},"poster_path":null}"#.to_owned())
                } else if request.url.contains("lookup/shows") {
                    Ok(r#"{"name":"Exact Show","externals":{"imdb":"tt1234567"},"image":null}"#.to_owned())
                } else if request.url.contains("singlesearch/shows") {
                    Ok(r#"{"name":"Exact Show","image":{"medium":"https://static.tvmaze.com/uploads/images/medium_portrait/1/2.jpg"}}"#.to_owned())
                } else {
                    panic!("AniList must not run after the title fallback succeeds")
                }
            },
            &mut |_| Ok(jpeg(400, 600)),
        )
        .expect("the exact title fallback must resolve");

        assert!(presentation.is_automatic());
        assert_eq!(presentation.imdb_id.as_deref(), Some("tt1234567"));
        assert_eq!(presentation.cover_state, "ready");
        assert!(bytes.is_some());
        assert!(cache_cover);
        assert!(requests[2].contains("lookup/shows"));
        assert!(requests[3].contains("singlesearch/shows"));
        assert_eq!(requests.len(), 4);
    }

    #[test]
    fn tv_cover_fallback_rejects_conflicting_identity_and_reaches_anilist_only_after_misses() {
        let authority = LibraryItemAuthority {
            category: LibraryCategory::Tv,
            identity: "fixture".to_owned(),
            local_title: "Exact Show".to_owned(),
            year: None,
            code: None,
        };
        let tmdb_search =
            r#"{"results":[{"id":7,"name":"Exact Show","first_air_date":"2020-01-01"}]}"#;
        let tmdb_detail = r#"{"id":7,"name":"Exact Show","first_air_date":"2020-01-01","genres":[],"credits":{"cast":[]},"external_ids":{"imdb_id":"tt1234567"},"poster_path":null}"#;
        let mut conflict_responses = vec![
            tmdb_search.to_owned(),
            tmdb_detail.to_owned(),
            r#"{"name":"Another Show","externals":{"imdb":"tt7654321"},"image":{"medium":"https://static.tvmaze.com/wrong.jpg"}}"#.to_owned(),
        ]
        .into_iter();
        let (conflict, bytes, _) = resolve_movie_or_tv_presentation(
            &authority,
            Some("token"),
            &mut |_| Ok(conflict_responses.next().expect("fixture response")),
            &mut |_| panic!("a conflicting fallback must not dispatch image work"),
        )
        .expect("a conflict becomes a safe local-only state");
        assert!(!conflict.is_automatic());
        assert!(bytes.is_none());

        let mut requests = Vec::new();
        let (anilist, bytes, cache_cover) = resolve_movie_or_tv_presentation(
            &authority,
            Some("token"),
            &mut |request| {
                requests.push(request.url.clone());
                if request.url.contains("search/tv") {
                    Ok(tmdb_search.to_owned())
                } else if request.url.contains("/tv/7?") {
                    Ok(tmdb_detail.to_owned())
                } else if request.url.contains("api.tvmaze.com") {
                    Err(ProviderRequestError::SourceUnavailable)
                } else {
                    Ok(r#"{"data":{"Media":{"title":{"userPreferred":"Exact Show"},"coverImage":{"large":"https://s1.anilist.co/file/anilistcdn/media/anime/cover/large/exact.jpg"}}}}"#.to_owned())
                }
            },
            &mut |_| Ok(jpeg(500, 700)),
        )
        .expect("AniList must resolve after both TVMaze misses");
        assert!(anilist.is_automatic());
        assert_eq!(anilist.cover_state, "ready");
        assert!(bytes.is_some());
        assert!(cache_cover);
        assert!(requests[2].contains("lookup/shows"));
        assert!(requests[3].contains("singlesearch/shows"));
        assert_eq!(requests[4], "https://graphql.anilist.co/");
    }

    #[test]
    fn successful_metadata_and_raw_cover_source_survive_restart_without_blob_values() {
        let fixture = CacheFixture::new();
        let authority = LibraryItemAuthority {
            category: LibraryCategory::Movie,
            identity: "exact-cache-identity".to_owned(),
            local_title: "Exact Movie".to_owned(),
            year: Some("1999".to_owned()),
            code: None,
        };
        let state = LibraryEnrichmentState::default();
        let mut responses = vec![
            r#"{"results":[{"id":419,"title":"Exact Movie","release_date":"1999-04-19"}]}"#.to_owned(),
            r#"{"id":419,"title":"Exact Movie","release_date":"1999-04-19","genres":[],"credits":{"cast":[]},"external_ids":{"imdb_id":"tt0123456"},"poster_path":"/exact.jpg"}"#.to_owned(),
        ]
        .into_iter();
        let first = fetch_presentation_with(
            &state,
            &fixture.path,
            &authority,
            Some("token"),
            |_| Ok(responses.next().expect("fixture response")),
            |_| Ok(jpeg(400, 600)),
            |_| panic!("Movie enrichment must not use JavDB"),
        )
        .expect("the first presentation must resolve");
        assert_eq!(first[2], "automatic");
        assert_eq!(first[12], "ready");
        let cache_text = fs::read_to_string(&fixture.path).expect("cache must persist");
        assert!(!cache_text.contains("blob:"));
        assert!(cache_text.contains(&encode_text("https://image.tmdb.org/t/p/w500/exact.jpg")));

        let restarted = LibraryEnrichmentState::default();
        let cached = fetch_presentation_with(
            &restarted,
            &fixture.path,
            &authority,
            Some("token"),
            |_| panic!("fresh cached metadata must not refetch"),
            |_| panic!("presentation cache lookup must not fetch cover bytes"),
            |_| panic!("Movie enrichment must not use JavDB"),
        )
        .expect("fresh cached presentation must load offline");
        assert_eq!(cached[6], "Exact Movie");
        let cover_authority = cached[11].clone();
        let cover = fetch_cover_with(&restarted, &authority, &cover_authority, |request| {
            assert_eq!(request.url, "https://image.tmdb.org/t/p/w500/exact.jpg");
            Ok(jpeg(400, 600))
        })
        .expect("a restarted cache must re-proxy the retained raw source");
        assert_eq!(image_dimensions(&cover), Some((400, 600)));
    }

    #[test]
    fn concurrent_presentations_merge_each_exact_cache_entry() {
        let fixture = CacheFixture::new();
        let state = LibraryEnrichmentState::default();
        let waiting_state = state.clone();
        let waiting_path = fixture.path.clone();
        let waiting_authority = LibraryItemAuthority {
            category: LibraryCategory::Movie,
            identity: "waiting-cache-identity".to_owned(),
            local_title: "Waiting Movie".to_owned(),
            year: None,
            code: None,
        };
        let (waiting_tx, waiting_rx) = std::sync::mpsc::channel();
        let (continue_tx, continue_rx) = std::sync::mpsc::channel();
        let waiting = std::thread::spawn(move || {
            let mut responses = vec![
                r#"{"results":[{"id":2,"title":"Waiting Movie","release_date":"2000-01-01"}]}"#
                    .to_owned(),
                r#"{"id":2,"title":"Waiting Movie","release_date":"2000-01-01","genres":[],"credits":{"cast":[]},"external_ids":{},"poster_path":null}"#
                    .to_owned(),
            ]
            .into_iter();
            let mut first_request = true;
            fetch_presentation_with(
                &waiting_state,
                &waiting_path,
                &waiting_authority,
                Some("token"),
                |_| {
                    if first_request {
                        first_request = false;
                        waiting_tx.send(()).expect("test must report waiting work");
                        continue_rx.recv().expect("test must release waiting work");
                    }
                    Ok(responses.next().expect("waiting fixture response"))
                },
                |_| panic!("missing fixture poster must not fetch an image"),
                |_| panic!("Movie enrichment must not use JavDB"),
            )
        });
        waiting_rx
            .recv()
            .expect("waiting work must start after its initial cache read");

        let current_authority = LibraryItemAuthority {
            category: LibraryCategory::Movie,
            identity: "current-cache-identity".to_owned(),
            local_title: "Current Movie".to_owned(),
            year: None,
            code: None,
        };
        let mut current_responses = vec![
            r#"{"results":[{"id":1,"title":"Current Movie","release_date":"1999-01-01"}]}"#
                .to_owned(),
            r#"{"id":1,"title":"Current Movie","release_date":"1999-01-01","genres":[],"credits":{"cast":[]},"external_ids":{},"poster_path":null}"#
                .to_owned(),
        ]
        .into_iter();
        fetch_presentation_with(
            &state,
            &fixture.path,
            &current_authority,
            Some("token"),
            |_| Ok(current_responses.next().expect("current fixture response")),
            |_| panic!("missing fixture poster must not fetch an image"),
            |_| panic!("Movie enrichment must not use JavDB"),
        )
        .expect("current presentation must persist");
        continue_tx
            .send(())
            .expect("waiting work must remain available");
        waiting
            .join()
            .expect("waiting thread must finish")
            .expect("waiting presentation must persist");

        let identities = read_cache(&fixture.path)
            .expect("merged cache must parse")
            .into_iter()
            .map(|entry| entry.identity)
            .collect::<Vec<_>>();
        assert_eq!(
            identities,
            vec![
                "current-cache-identity".to_owned(),
                "waiting-cache-identity".to_owned(),
            ]
        );
    }

    #[test]
    fn confirmed_no_cover_is_cached_but_transient_provider_failure_is_not() {
        let no_cover_fixture = CacheFixture::new();
        let authority = LibraryItemAuthority {
            category: LibraryCategory::Movie,
            identity: "confirmed-no-cover".to_owned(),
            local_title: "Exact Movie".to_owned(),
            year: None,
            code: None,
        };
        let mut responses = vec![
            r#"{"results":[{"id":419,"title":"Exact Movie","release_date":"1999-04-19"}]}"#.to_owned(),
            r#"{"id":419,"title":"Exact Movie","release_date":"1999-04-19","genres":[],"credits":{"cast":[]},"external_ids":{},"poster_path":null}"#.to_owned(),
        ]
        .into_iter();
        let result = fetch_presentation_with(
            &LibraryEnrichmentState::default(),
            &no_cover_fixture.path,
            &authority,
            Some("token"),
            |_| Ok(responses.next().expect("fixture response")),
            |_| panic!("a missing poster must not dispatch an image request"),
            |_| panic!("Movie enrichment must not use JavDB"),
        )
        .expect("confirmed no-cover presentation must resolve");
        assert_eq!(result[12], "missing");
        fetch_presentation_with(
            &LibraryEnrichmentState::default(),
            &no_cover_fixture.path,
            &authority,
            Some("token"),
            |_| panic!("confirmed no-cover cache must suppress provider work"),
            |_| panic!("confirmed no-cover cache must suppress image work"),
            |_| panic!("Movie enrichment must not use JavDB"),
        )
        .expect("confirmed no-cover must load from cache");

        let transient_fixture = CacheFixture::new();
        assert_eq!(
            fetch_presentation_with(
                &LibraryEnrichmentState::default(),
                &transient_fixture.path,
                &authority,
                Some("token"),
                |_| Err(ProviderRequestError::Network),
                |_| panic!("failed metadata must not dispatch image work"),
                |_| panic!("Movie enrichment must not use JavDB"),
            ),
            Err(LIBRARY_ENRICHMENT_FAILED)
        );
        assert!(!transient_fixture.path.exists());
    }

    #[test]
    fn confirmed_exact_code_miss_survives_restart_and_refreshes_after_expiry() {
        let fixture = CacheFixture::new();
        let state = LibraryEnrichmentState::default();
        let authority = LibraryItemAuthority {
            category: LibraryCategory::Adult,
            identity: "adult-confirmed-miss".to_owned(),
            local_title: "ADLT-123".to_owned(),
            year: None,
            code: Some("ADLT-123".to_owned()),
        };
        let provider_calls = Cell::new(0);
        let first = fetch_presentation_with(
            &state,
            &fixture.path,
            &authority,
            None,
            |_| {
                provider_calls.set(provider_calls.get() + 1);
                Err(ProviderRequestError::SourceUnavailable)
            },
            |_| {
                provider_calls.set(provider_calls.get() + 1);
                Err(ProviderRequestError::SourceUnavailable)
            },
            |_| {
                provider_calls.set(provider_calls.get() + 1);
                Ok(r#"{"success":1,"data":{"movies":[]}}"#.to_owned())
            },
        )
        .expect("a complete exact-code miss must resolve");
        assert_eq!(first[2], "local-only");
        assert_eq!(first[12], "missing");
        assert!(provider_calls.get() > 0);

        fetch_presentation_with(
            &state,
            &fixture.path,
            &authority,
            None,
            |_| panic!("a repeated view must suppress text provider work"),
            |_| panic!("a repeated view must suppress image provider work"),
            |_| panic!("a repeated view must suppress JavDB work"),
        )
        .expect("a repeated view must use the confirmed miss");
        fetch_presentation_with(
            &LibraryEnrichmentState::default(),
            &fixture.path,
            &authority,
            None,
            |_| panic!("a fresh miss must suppress text provider work after restart"),
            |_| panic!("a fresh miss must suppress image provider work after restart"),
            |_| panic!("a fresh miss must suppress JavDB work after restart"),
        )
        .expect("a fresh confirmed miss must load after restart");

        let replaced_authority = LibraryItemAuthority {
            identity: "adult-replaced-item".to_owned(),
            ..authority.clone()
        };
        let replacement_calls = Cell::new(0);
        fetch_presentation_with(
            &LibraryEnrichmentState::default(),
            &fixture.path,
            &replaced_authority,
            None,
            |_| {
                replacement_calls.set(replacement_calls.get() + 1);
                Err(ProviderRequestError::SourceUnavailable)
            },
            |_| {
                replacement_calls.set(replacement_calls.get() + 1);
                Err(ProviderRequestError::SourceUnavailable)
            },
            |_| {
                replacement_calls.set(replacement_calls.get() + 1);
                Ok(r#"{"success":1,"data":{"movies":[]}}"#.to_owned())
            },
        )
        .expect("a different exact item may establish its own miss");
        assert!(replacement_calls.get() > 0);

        let mut entries = read_cache(&fixture.path).expect("miss cache must parse");
        assert_eq!(entries.len(), 2);
        let original = entries
            .iter_mut()
            .find(|entry| entry.identity == authority.identity)
            .expect("the exact original miss must remain cached");
        original.cover_saved_at = 0;
        write_cache(&fixture.path, &entries).expect("expired miss must persist");

        let refreshed_calls = Cell::new(0);
        fetch_presentation_with(
            &LibraryEnrichmentState::default(),
            &fixture.path,
            &authority,
            None,
            |_| {
                refreshed_calls.set(refreshed_calls.get() + 1);
                Err(ProviderRequestError::SourceUnavailable)
            },
            |_| {
                refreshed_calls.set(refreshed_calls.get() + 1);
                Err(ProviderRequestError::SourceUnavailable)
            },
            |_| {
                refreshed_calls.set(refreshed_calls.get() + 1);
                Ok(r#"{"success":1,"data":{"movies":[]}}"#.to_owned())
            },
        )
        .expect("an expired miss may be confirmed again");
        assert!(refreshed_calls.get() > 0);

        fetch_presentation_with(
            &LibraryEnrichmentState::default(),
            &fixture.path,
            &authority,
            None,
            |_| panic!("the refreshed miss must suppress text provider work"),
            |_| panic!("the refreshed miss must suppress image provider work"),
            |_| panic!("the refreshed miss must suppress JavDB work"),
        )
        .expect("the refreshed miss must remain durable");
    }

    #[test]
    fn transient_exact_code_failure_is_not_persisted_as_a_confirmed_miss() {
        let fixture = CacheFixture::new();
        let authority = LibraryItemAuthority {
            category: LibraryCategory::Vr,
            identity: "vr-transient-miss".to_owned(),
            local_title: "MDVR-419".to_owned(),
            year: None,
            code: Some("MDVR-419".to_owned()),
        };
        let attempts = Cell::new(0);
        for _ in 0..2 {
            let result = fetch_presentation_with(
                &LibraryEnrichmentState::default(),
                &fixture.path,
                &authority,
                None,
                |_| Err(ProviderRequestError::SourceUnavailable),
                |_| {
                    attempts.set(attempts.get() + 1);
                    Err(ProviderRequestError::Network)
                },
                |_| Err(ProviderRequestError::SourceUnavailable),
            );
            assert_eq!(result, Err(LIBRARY_ENRICHMENT_FAILED));
            assert!(!fixture.path.exists());
        }
        assert!(attempts.get() > 1);
    }

    #[test]
    fn expired_cover_refresh_preserves_fresh_metadata_and_expired_metadata_is_replaced() {
        let fixture = CacheFixture::new();
        let authority = LibraryItemAuthority {
            category: LibraryCategory::Movie,
            identity: "expiry-identity".to_owned(),
            local_title: "Exact Movie".to_owned(),
            year: None,
            code: None,
        };
        let state = LibraryEnrichmentState::default();
        let mut initial = vec![
            r#"{"results":[{"id":1,"title":"Exact Movie","release_date":"1999-01-01"}]}"#.to_owned(),
            r#"{"id":1,"title":"Original cached title","original_title":"Exact Movie","release_date":"1999-01-01","genres":[],"credits":{"cast":[]},"external_ids":{},"poster_path":"/old.jpg"}"#.to_owned(),
        ]
        .into_iter();
        fetch_presentation_with(
            &state,
            &fixture.path,
            &authority,
            Some("token"),
            |_| Ok(initial.next().expect("fixture response")),
            |_| Ok(jpeg(400, 600)),
            |_| panic!("Movie enrichment must not use JavDB"),
        )
        .expect("initial presentation must persist");
        let mut entries = read_cache(&fixture.path).expect("cache must parse");
        let original_metadata_saved_at = entries[0].metadata_saved_at;
        entries[0].cover_saved_at = 0;
        write_cache(&fixture.path, &entries).expect("stale cover fixture must persist");

        let offline = fetch_presentation_with(
            &LibraryEnrichmentState::default(),
            &fixture.path,
            &authority,
            Some("token"),
            |_| Err(ProviderRequestError::Network),
            |_| panic!("failed metadata lookup must not dispatch image work"),
            |_| panic!("Movie enrichment must not use JavDB"),
        )
        .expect("fresh metadata may remain visible without an expired cover source");
        assert_eq!(offline[6], "Original cached title");
        assert_eq!(offline[11], "");
        assert_eq!(offline[12], "unavailable");

        let mut cover_refresh = vec![
            r#"{"results":[{"id":2,"title":"Exact Movie","release_date":"2000-01-01"}]}"#.to_owned(),
            r#"{"id":2,"title":"Replacement metadata","original_title":"Exact Movie","release_date":"2000-01-01","genres":[],"credits":{"cast":[]},"external_ids":{},"poster_path":null}"#.to_owned(),
        ]
        .into_iter();
        let refreshed_cover = fetch_presentation_with(
            &LibraryEnrichmentState::default(),
            &fixture.path,
            &authority,
            Some("token"),
            |_| Ok(cover_refresh.next().expect("fixture response")),
            |_| panic!("missing replacement poster must not dispatch image work"),
            |_| panic!("Movie enrichment must not use JavDB"),
        )
        .expect("cover expiry must retain valid metadata");
        assert_eq!(refreshed_cover[6], "Original cached title");
        assert_eq!(refreshed_cover[12], "missing");
        let mut entries = read_cache(&fixture.path).expect("cache must parse");
        assert_eq!(entries[0].metadata_saved_at, original_metadata_saved_at);

        entries[0].metadata_saved_at = 0;
        entries[0].cover_saved_at = 0;
        write_cache(&fixture.path, &entries).expect("expired metadata fixture must persist");
        assert_eq!(
            fetch_presentation_with(
                &LibraryEnrichmentState::default(),
                &fixture.path,
                &authority,
                Some("token"),
                |_| Err(ProviderRequestError::Network),
                |_| panic!("expired metadata must not dispatch image work"),
                |_| panic!("Movie enrichment must not use JavDB"),
            ),
            Err(LIBRARY_ENRICHMENT_FAILED)
        );
        let mut replacement = vec![
            r#"{"results":[{"id":3,"title":"Exact Movie","release_date":"2001-01-01"}]}"#.to_owned(),
            r#"{"id":3,"title":"Current metadata","original_title":"Exact Movie","release_date":"2001-01-01","genres":[],"credits":{"cast":[]},"external_ids":{},"poster_path":null}"#.to_owned(),
        ]
        .into_iter();
        let current = fetch_presentation_with(
            &LibraryEnrichmentState::default(),
            &fixture.path,
            &authority,
            Some("token"),
            |_| Ok(replacement.next().expect("fixture response")),
            |_| panic!("missing replacement poster must not dispatch image work"),
            |_| panic!("Movie enrichment must not use JavDB"),
        )
        .expect("expired metadata must be replaced");
        assert_eq!(current[6], "Current metadata");
        assert!(read_cache(&fixture.path).expect("cache must parse")[0].metadata_saved_at > 0);
    }

    #[test]
    fn fc2_and_conflicting_r18_identity_never_create_exact_code_presentation() {
        let fc2 = LibraryItemAuthority {
            category: LibraryCategory::Adult,
            identity: "fc2".to_owned(),
            local_title: "FC2-1234567".to_owned(),
            year: None,
            code: Some("FC2-1234567".to_owned()),
        };
        let (presentation, bytes, cache_cover) = exact_code_presentation(
            &fc2,
            &mut |_| panic!("FC2 must skip exact-code providers"),
            &mut |_| panic!("FC2 must skip studio cover providers"),
            &mut |_| panic!("FC2 must skip JavDB"),
        )
        .expect("FC2 is an intentional local-only state");
        assert!(!presentation.is_automatic());
        assert!(bytes.is_none());
        assert!(cache_cover);

        let conflicting = LibraryItemAuthority {
            category: LibraryCategory::Vr,
            identity: "vr-conflict".to_owned(),
            local_title: "MDVR-419".to_owned(),
            year: None,
            code: Some("MDVR-419".to_owned()),
        };
        let result = exact_code_presentation(
            &conflicting,
            &mut |request| {
                if request.url.contains("r18.dev") {
                    Ok(r#"{"content_id":"abc00123","title":"Wrong item"}"#.to_owned())
                } else {
                    Err(ProviderRequestError::SourceUnavailable)
                }
            },
            &mut |_| Err(ProviderRequestError::SourceUnavailable),
            &mut |_| Err(ProviderRequestError::SourceUnavailable),
        );
        assert_eq!(result, Err(ProviderRequestError::Network));
    }

    #[test]
    fn exact_code_cover_resolution_preserves_dmm_r18_mgstage_database_order() {
        let authority = LibraryItemAuthority {
            category: LibraryCategory::Adult,
            identity: "adult-order".to_owned(),
            local_title: "ADLT-123".to_owned(),
            year: None,
            code: Some("ADLT-123".to_owned()),
        };
        let events = RefCell::new(Vec::new());
        let (presentation, bytes, cache_cover) = exact_code_presentation(
            &authority,
            &mut |request| {
                events.borrow_mut().push(format!("text:{}", request.url));
                if request.url.contains("r18.dev") {
                    Ok(r#"{"content_id":"adlt00123","title_ja":"日本語題名","release_date":"2020-01-02","runtime_minutes":90,"actresses":[{"name_kanji":"俳優名"}],"images":{"jacket_image":{"large2":"https://pics.dmm.co.jp/digital/video/h_999adlt00123/h_999adlt00123pl.jpg"}}}"#.to_owned())
                } else if request.url.contains("javdatabase.com") {
                    Ok("<title>ADLT-123 - Exact actor - JAV Database</title>".to_owned())
                } else {
                    panic!("MGStage must not run after the r18 cover succeeds")
                }
            },
            &mut |request| {
                events.borrow_mut().push(format!("image:{}", request.url));
                if request.url.contains("h_999adlt00123") {
                    Ok(jpeg(400, 600))
                } else {
                    Err(ProviderRequestError::SourceUnavailable)
                }
            },
            &mut |_| panic!("complete r18 metadata must not use JavDB"),
        )
        .expect("the exact provider chain must resolve");

        assert!(presentation.is_automatic());
        assert_eq!(presentation.title.as_deref(), Some("日本語題名"));
        assert_eq!(presentation.cast, ["俳優名"]);
        assert!(bytes.is_some());
        assert!(cache_cover);
        let events = events.into_inner();
        let r18_position = events
            .iter()
            .position(|event| event.contains("text:https://r18.dev/"))
            .expect("r18 request must run");
        let r18_cover_position = events
            .iter()
            .position(|event| {
                event.contains("image:https://pics.dmm.co.jp") && event.contains("h_999")
            })
            .expect("r18 jacket must run");
        let database_position = events
            .iter()
            .position(|event| event.contains("text:https://www.javdatabase.com/"))
            .expect("descriptive database request must run");
        assert!(events[..r18_position]
            .iter()
            .all(|event| event.starts_with("image:https://pics.dmm.co.jp/")));
        assert!(r18_position < r18_cover_position);
        assert!(r18_cover_position < database_position);
        assert!(!events.iter().any(|event| event.contains("mgstage.com")));
    }

    #[test]
    fn exact_code_uses_validated_javdatabase_cast_only_after_japanese_cast_is_unavailable() {
        let authority = LibraryItemAuthority {
            category: LibraryCategory::Adult,
            identity: "adult-romanized-cast".to_owned(),
            local_title: "ADLT-123".to_owned(),
            year: None,
            code: Some("ADLT-123".to_owned()),
        };
        let (presentation, bytes, cache_cover) = exact_code_presentation(
            &authority,
            &mut |request| {
                if request.url.contains("r18.dev") {
                    Ok(r#"{"content_id":"adlt00123","title_ja":"日本語題名","actresses":[]}"#.to_owned())
                } else if request.url.contains("javdatabase.com") {
                    Ok("<title>ADLT-123 - Romanized Actor - JAV Database</title>".to_owned())
                } else {
                    Err(ProviderRequestError::SourceUnavailable)
                }
            },
            &mut |_| Err(ProviderRequestError::SourceUnavailable),
            &mut |url| {
                if url.contains("/search?") {
                    Ok(r#"{"success":1,"data":{"movies":[{"id":"AdultA","number":"ADLT-123"}]}}"#.to_owned())
                } else {
                    Ok(r#"{"success":1,"data":{"movie":{"id":"AdultA","number":"ADLT-123","title":"日本語題名","actors":[],"tags":[{"id":"1","name":"Drama"}]}}}"#.to_owned())
                }
            },
        )
        .expect("the exact provider chain must resolve");

        assert!(presentation.is_automatic());
        assert_eq!(presentation.title.as_deref(), Some("日本語題名"));
        assert_eq!(presentation.cast, ["Romanized Actor"]);
        assert!(bytes.is_none());
        assert!(cache_cover);
        assert_eq!(
            javdatabase_romanized_cast(
                "<title>OTHER-123 - Wrong Actor - JAV Database</title>",
                "ADLT-123"
            ),
            None
        );
        assert_eq!(
            javdatabase_romanized_cast("<title>ADLT-123 - JAV Database</title>", "ADLT-123"),
            None
        );
        assert_eq!(
            javdatabase_romanized_cast(
                "<title>ADLT-123 - JAV Movie - JAV Database</title>",
                "ADLT-123"
            ),
            None
        );
    }
}
