use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use crate::{
    is_valid_tmdb_token,
    vr_torrent::{
        canonical_imdb_id, canonical_infohash, encode_torrent_inspection, json_array, json_object,
        json_string, json_u64, parse_torrent_metadata, revalidate_persisted_tv_download_source,
        JsonParser, JsonValue, TorrentInspectionError, TorrentMetadata, TvDownloadIdentity,
        VerifiedDownloadSource, VerifiedDownloadSourceError,
    },
    MovieProviderRequestError,
};

pub(crate) const TV_TMDB_UNAUTHORIZED: &str = "tv_release_tmdb_unauthorized";
pub(crate) const TV_TMDB_RATE_LIMITED: &str = "tv_release_tmdb_rate_limited";
pub(crate) const TV_TMDB_NETWORK_ERROR: &str = "tv_release_tmdb_network_error";
pub(crate) const TV_TMDB_MALFORMED: &str = "tv_release_tmdb_malformed";
pub(crate) const TV_TMDB_PROVIDER_ERROR: &str = "tv_release_tmdb_provider_error";
pub(crate) const TV_NO_IMDB_IDENTITY: &str = "tv_release_no_imdb_identity";
pub(crate) const TV_APIBAY_SOURCE_UNAVAILABLE: &str = "tv_release_apibay_source_unavailable";
pub(crate) const TV_APIBAY_NETWORK_ERROR: &str = "tv_release_apibay_network_error";
pub(crate) const TV_APIBAY_MALFORMED: &str = "tv_release_apibay_malformed";
pub(crate) const TV_APIBAY_CONFLICTING: &str = "tv_release_apibay_conflicting";
pub(crate) const TV_APIBAY_PROVIDER_ERROR: &str = "tv_release_apibay_provider_error";
pub(crate) const TV_TORRENT_CONTEXT_INVALID: &str = "tv_torrent_context_invalid";
pub(crate) const TV_TORRENT_INFOHASH_MISMATCH: &str = "tv_torrent_infohash_mismatch";
pub(crate) const TV_TORRENT_MALFORMED: &str = "tv_torrent_malformed";
pub(crate) const TV_TORRENT_NETWORK_ERROR: &str = "tv_torrent_network_error";
pub(crate) const TV_TORRENT_NO_PEERS: &str = "tv_torrent_no_peers";
pub(crate) const TV_TORRENT_SAVE_FAILED: &str = "tv_torrent_save_failed";
pub(crate) const TV_TORRENT_SOURCE_UNAVAILABLE: &str = "tv_torrent_source_unavailable";
pub(crate) const TV_TORRENT_STALE: &str = "tv_torrent_stale";
pub(crate) const TV_TORRENT_TIMEOUT: &str = "tv_torrent_timeout";
pub(crate) const TV_TORRENT_UNSUPPORTED: &str = "tv_torrent_unsupported";

const TMDB_TV_URL: &str = "https://api.themoviedb.org/3/tv/";
const API_BAY_QUERY_URL: &str = "https://apibay.org/q.php?q=";
const API_BAY_NO_RESULTS_NAME: &str = "No results returned";
const API_BAY_NO_RESULTS_INFOHASH: &str = "0000000000000000000000000000000000000000";
const TV_CATEGORIES: [u64; 2] = [205, 208];
const MAX_PROVIDER_ROWS: usize = 500;

#[derive(Clone, Debug, PartialEq, Eq)]
struct TrustedEpisodeContext {
    tmdb_tv_id: u64,
    show_name: String,
    provider_season_id: u64,
    season_number: u64,
    provider_episode_id: u64,
    episode_number: u64,
    episode_name: String,
    imdb_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ApiBayRelease {
    provider_item_id: String,
    name: String,
    category: String,
    size_bytes: String,
    seeders: String,
    leechers: String,
    uploader: String,
    status: String,
    added: String,
    infohash: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TrustedTvReleaseSet {
    generation: u64,
    context: TrustedEpisodeContext,
    releases: Vec<ApiBayRelease>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TvTorrentInspectionRequest {
    pub(crate) tmdb_tv_id: u64,
    pub(crate) show_name: String,
    pub(crate) provider_season_id: u64,
    pub(crate) season_number: u64,
    pub(crate) provider_episode_id: u64,
    pub(crate) episode_number: u64,
    pub(crate) episode_name: String,
    pub(crate) imdb_id: String,
    pub(crate) provider_item_id: String,
    pub(crate) provider_category: String,
    pub(crate) release_name: String,
    pub(crate) expected_infohash: String,
}

impl TvTorrentInspectionRequest {
    fn context(&self) -> TrustedEpisodeContext {
        TrustedEpisodeContext {
            tmdb_tv_id: self.tmdb_tv_id,
            show_name: self.show_name.clone(),
            provider_season_id: self.provider_season_id,
            season_number: self.season_number,
            provider_episode_id: self.provider_episode_id,
            episode_number: self.episode_number,
            episode_name: self.episode_name.clone(),
            imdb_id: self.imdb_id.clone(),
        }
    }

    fn identity(&self) -> TvDownloadIdentity {
        TvDownloadIdentity {
            tmdb_tv_id: self.tmdb_tv_id,
            show_name: self.show_name.clone(),
            provider_season_id: self.provider_season_id,
            season_number: self.season_number,
            provider_episode_id: self.provider_episode_id,
            episode_number: self.episode_number,
            episode_name: self.episode_name.clone(),
            imdb_id: self.imdb_id.clone(),
            provider_item_id: self.provider_item_id.clone(),
            provider_category: self.provider_category.clone(),
            release_name: self.release_name.clone(),
            expected_infohash: self.expected_infohash.clone(),
        }
    }
}

struct CachedTvTorrent {
    generation: u64,
    inspection_id: String,
    default_file_name: String,
    identity: TvDownloadIdentity,
    bytes: Vec<u8>,
    metadata: TorrentMetadata,
}

#[derive(Default)]
struct TvReleaseContext {
    release_generation: u64,
    inspection_generation: u64,
    release_set: Option<TrustedTvReleaseSet>,
    cached_torrent: Option<CachedTvTorrent>,
}

#[derive(Clone, Default)]
pub(crate) struct TvReleaseState(Arc<Mutex<TvReleaseContext>>);

#[derive(Clone)]
pub(crate) struct TvInspectionTicket {
    release_generation: u64,
    inspection_generation: u64,
    request: TvTorrentInspectionRequest,
}

impl TvInspectionTicket {
    pub(crate) fn magnet_url(&self) -> String {
        let display_name = percent_encode(&self.request.release_name);
        format!(
            "magnet:?xt=urn:btih:{}&dn={display_name}",
            self.request.expected_infohash
        )
    }
}

impl TvReleaseState {
    pub(crate) fn begin_release_lookup(&self) -> Result<u64, &'static str> {
        let mut context = self.0.lock().map_err(|_| TV_APIBAY_PROVIDER_ERROR)?;
        context.release_generation = context.release_generation.wrapping_add(1);
        context.inspection_generation = context.inspection_generation.wrapping_add(1);
        context.release_set = None;
        context.cached_torrent = None;
        Ok(context.release_generation)
    }

    fn finish_release_lookup(
        &self,
        release_set: TrustedTvReleaseSet,
    ) -> Result<Vec<String>, &'static str> {
        let response = flatten_releases(&release_set.context, &release_set.releases);
        let mut context = self.0.lock().map_err(|_| TV_APIBAY_PROVIDER_ERROR)?;
        if context.release_generation != release_set.generation {
            return Err(TV_TORRENT_STALE);
        }
        context.release_set = Some(release_set);
        Ok(response)
    }

    pub(crate) fn begin_inspection(
        &self,
        request: TvTorrentInspectionRequest,
    ) -> Result<TvInspectionTicket, &'static str> {
        let mut context = self.0.lock().map_err(|_| TV_APIBAY_PROVIDER_ERROR)?;
        context.inspection_generation = context.inspection_generation.wrapping_add(1);
        context.cached_torrent = None;
        let release_is_current = context.release_set.as_ref().is_some_and(|release_set| {
            release_set.generation == context.release_generation
                && release_set.context == request.context()
                && release_set.releases.iter().any(|release| {
                    release.provider_item_id == request.provider_item_id
                        && release.name == request.release_name
                        && release.category == request.provider_category
                        && release.infohash == request.expected_infohash
                })
        });
        if !release_is_current {
            return Err(TV_TORRENT_CONTEXT_INVALID);
        }
        Ok(TvInspectionTicket {
            release_generation: context.release_generation,
            inspection_generation: context.inspection_generation,
            request,
        })
    }

    pub(crate) fn finish_inspection(
        &self,
        ticket: TvInspectionTicket,
        bytes: Vec<u8>,
    ) -> Result<Vec<String>, &'static str> {
        let metadata = parse_torrent_metadata(&bytes).map_err(tv_torrent_parse_error)?;
        if metadata.infohash != ticket.request.expected_infohash {
            return Err(TV_TORRENT_INFOHASH_MISMATCH);
        }
        let mut context = self.0.lock().map_err(|_| TV_APIBAY_PROVIDER_ERROR)?;
        let is_current = context.release_set.as_ref().is_some_and(|release_set| {
            release_set.generation == ticket.release_generation
                && release_set.context == ticket.request.context()
                && release_set.releases.iter().any(|release| {
                    release.provider_item_id == ticket.request.provider_item_id
                        && release.name == ticket.request.release_name
                        && release.category == ticket.request.provider_category
                        && release.infohash == ticket.request.expected_infohash
                })
        });
        if context.release_generation != ticket.release_generation
            || context.inspection_generation != ticket.inspection_generation
            || !is_current
        {
            return Err(TV_TORRENT_STALE);
        }
        let inspection_id = format!(
            "tv-{}-{}-{}",
            ticket.release_generation,
            ticket.inspection_generation,
            ticket.request.provider_item_id
        );
        context.cached_torrent = Some(CachedTvTorrent {
            generation: ticket.inspection_generation,
            inspection_id: inspection_id.clone(),
            default_file_name: format!(
                "tv-{}-{}.torrent",
                ticket.request.provider_item_id, ticket.request.expected_infohash
            ),
            identity: ticket.request.identity(),
            bytes,
            metadata: metadata.clone(),
        });
        Ok(encode_torrent_inspection(inspection_id, metadata))
    }

    pub(crate) fn invalidate_inspection(&self) -> Result<(), &'static str> {
        let mut context = self.0.lock().map_err(|_| TV_APIBAY_PROVIDER_ERROR)?;
        context.inspection_generation = context.inspection_generation.wrapping_add(1);
        context.cached_torrent = None;
        Ok(())
    }

    pub(crate) fn invalidate_release_context(&self) -> Result<(), &'static str> {
        self.begin_release_lookup().map(|_| ())
    }

    pub(crate) fn verified_download_source(
        &self,
        inspection_id: &str,
        selected_file_ids: &[usize],
    ) -> Result<VerifiedDownloadSource, VerifiedDownloadSourceError> {
        let (identity, bytes, expected_metadata) = {
            let context = self
                .0
                .lock()
                .map_err(|_| VerifiedDownloadSourceError::Context)?;
            context
                .cached_torrent
                .as_ref()
                .filter(|torrent| {
                    torrent.inspection_id == inspection_id
                        && torrent.generation == context.inspection_generation
                })
                .map(|torrent| {
                    (
                        torrent.identity.clone(),
                        torrent.bytes.clone(),
                        torrent.metadata.clone(),
                    )
                })
                .ok_or(VerifiedDownloadSourceError::Context)?
        };
        let source = revalidate_persisted_tv_download_source(
            &bytes,
            &identity,
            &identity.expected_infohash,
            selected_file_ids,
        )?;
        let reparsed =
            parse_torrent_metadata(&bytes).map_err(|_| VerifiedDownloadSourceError::Metainfo)?;
        if reparsed != expected_metadata {
            return Err(VerifiedDownloadSourceError::Metainfo);
        }
        Ok(source)
    }

    pub(crate) fn save_verified_torrent_with(
        &self,
        inspection_id: &str,
        choose_destination: impl FnOnce(&str) -> Option<PathBuf>,
        write: impl FnOnce(&Path, &[u8]) -> io::Result<()>,
    ) -> Result<bool, &'static str> {
        let default_file_name = {
            let context = self.0.lock().map_err(|_| TV_TORRENT_SAVE_FAILED)?;
            context
                .cached_torrent
                .as_ref()
                .filter(|torrent| {
                    torrent.inspection_id == inspection_id
                        && torrent.generation == context.inspection_generation
                })
                .map(|torrent| torrent.default_file_name.clone())
                .ok_or(TV_TORRENT_STALE)?
        };
        let Some(destination) = choose_destination(&default_file_name) else {
            return Ok(false);
        };
        let context = self.0.lock().map_err(|_| TV_TORRENT_SAVE_FAILED)?;
        let torrent = context
            .cached_torrent
            .as_ref()
            .filter(|torrent| {
                torrent.inspection_id == inspection_id
                    && torrent.generation == context.inspection_generation
            })
            .ok_or(TV_TORRENT_STALE)?;
        let reparsed = parse_torrent_metadata(&torrent.bytes).map_err(tv_torrent_parse_error)?;
        if reparsed != torrent.metadata {
            return Err(TV_TORRENT_MALFORMED);
        }
        write(&destination, &torrent.bytes).map_err(|_| TV_TORRENT_SAVE_FAILED)?;
        Ok(true)
    }
}

fn tv_torrent_parse_error(error: TorrentInspectionError) -> &'static str {
    match error {
        TorrentInspectionError::Malformed => TV_TORRENT_MALFORMED,
        TorrentInspectionError::Unsupported => TV_TORRENT_UNSUPPORTED,
        _ => TV_TORRENT_MALFORMED,
    }
}

fn tmdb_error_code(error: MovieProviderRequestError) -> &'static str {
    match error {
        MovieProviderRequestError::Unauthorized => TV_TMDB_UNAUTHORIZED,
        MovieProviderRequestError::RateLimited => TV_TMDB_RATE_LIMITED,
        MovieProviderRequestError::Network => TV_TMDB_NETWORK_ERROR,
        MovieProviderRequestError::SourceUnavailable | MovieProviderRequestError::Provider => {
            TV_TMDB_PROVIDER_ERROR
        }
    }
}

fn apibay_error_code(error: MovieProviderRequestError) -> &'static str {
    match error {
        MovieProviderRequestError::SourceUnavailable => TV_APIBAY_SOURCE_UNAVAILABLE,
        MovieProviderRequestError::Network => TV_APIBAY_NETWORK_ERROR,
        MovieProviderRequestError::Unauthorized
        | MovieProviderRequestError::RateLimited
        | MovieProviderRequestError::Provider => TV_APIBAY_PROVIDER_ERROR,
    }
}

#[cfg(test)]
pub(crate) fn fetch_apibay_tv_releases_with(
    tmdb_tv_id: u64,
    requested_provider_season_id: u64,
    requested_provider_episode_id: u64,
    tmdb_token: &str,
    request: impl FnMut(&str, Option<&str>) -> Result<String, MovieProviderRequestError>,
) -> Result<Vec<String>, &'static str> {
    let release_set = trusted_apibay_tv_releases_with(
        0,
        tmdb_tv_id,
        requested_provider_season_id,
        requested_provider_episode_id,
        tmdb_token,
        request,
    )?;
    Ok(flatten_releases(
        &release_set.context,
        &release_set.releases,
    ))
}

pub(crate) fn fetch_apibay_tv_releases_for_state_with(
    state: &TvReleaseState,
    generation: u64,
    tmdb_tv_id: u64,
    requested_provider_season_id: u64,
    requested_provider_episode_id: u64,
    tmdb_token: &str,
    request: impl FnMut(&str, Option<&str>) -> Result<String, MovieProviderRequestError>,
) -> Result<Vec<String>, &'static str> {
    let release_set = trusted_apibay_tv_releases_with(
        generation,
        tmdb_tv_id,
        requested_provider_season_id,
        requested_provider_episode_id,
        tmdb_token,
        request,
    )?;
    state.finish_release_lookup(release_set)
}

fn trusted_apibay_tv_releases_with(
    generation: u64,
    tmdb_tv_id: u64,
    requested_provider_season_id: u64,
    requested_provider_episode_id: u64,
    tmdb_token: &str,
    mut request: impl FnMut(&str, Option<&str>) -> Result<String, MovieProviderRequestError>,
) -> Result<TrustedTvReleaseSet, &'static str> {
    if tmdb_tv_id == 0
        || requested_provider_season_id == 0
        || requested_provider_episode_id == 0
        || !is_valid_tmdb_token(tmdb_token)
    {
        return Err(TV_TMDB_MALFORMED);
    }

    let details_url = format!("{TMDB_TV_URL}{tmdb_tv_id}");
    let details = request(&details_url, Some(tmdb_token)).map_err(tmdb_error_code)?;
    let (show_name, season_number) =
        trusted_show_season(tmdb_tv_id, requested_provider_season_id, &details)?;

    let season_url = format!("{details_url}/season/{season_number}");
    let season = request(&season_url, Some(tmdb_token)).map_err(tmdb_error_code)?;
    let (episode_number, episode_name) = trusted_episode(
        requested_provider_season_id,
        season_number,
        requested_provider_episode_id,
        &season,
    )?;

    let external_ids = request(&format!("{details_url}/external_ids"), Some(tmdb_token))
        .map_err(tmdb_error_code)?;
    let imdb_id = trusted_imdb_id(tmdb_tv_id, &external_ids)?;
    let context = TrustedEpisodeContext {
        tmdb_tv_id,
        show_name,
        provider_season_id: requested_provider_season_id,
        season_number,
        provider_episode_id: requested_provider_episode_id,
        episode_number,
        episode_name,
        imdb_id,
    };

    let query = percent_encode(&format!(
        "{} S{:02}E{:02}",
        context.show_name, context.season_number, context.episode_number
    ));
    let mut releases = Vec::new();
    let mut item_ids = BTreeSet::new();
    let mut infohashes = BTreeSet::new();
    for category in TV_CATEGORIES {
        let document = request(&format!("{API_BAY_QUERY_URL}{query}&cat={category}"), None)
            .map_err(apibay_error_code)?;
        releases.extend(verified_apibay_releases(
            &document,
            category,
            &context,
            &mut item_ids,
            &mut infohashes,
        )?);
    }

    Ok(TrustedTvReleaseSet {
        generation,
        context,
        releases,
    })
}

fn trusted_show_season(
    tmdb_tv_id: u64,
    requested_provider_season_id: u64,
    document: &str,
) -> Result<(String, u64), &'static str> {
    let value = JsonParser::new(document).parse().ok_or(TV_TMDB_MALFORMED)?;
    let object = json_object(&value).ok_or(TV_TMDB_MALFORMED)?;
    if json_u64(object, "id") != Some(tmdb_tv_id) {
        return Err(TV_TMDB_MALFORMED);
    }
    let show_name = json_string(object, "name").ok_or(TV_TMDB_MALFORMED)?;
    if show_name.trim().is_empty() {
        return Err(TV_TMDB_MALFORMED);
    }
    let seasons =
        json_array(object.get("seasons").ok_or(TV_TMDB_MALFORMED)?).ok_or(TV_TMDB_MALFORMED)?;
    let mut season_ids = BTreeSet::new();
    let mut season_numbers = BTreeSet::new();
    let mut selected_season_number = None;
    for season in seasons {
        let season = json_object(season).ok_or(TV_TMDB_MALFORMED)?;
        let provider_season_id = json_u64(season, "id").ok_or(TV_TMDB_MALFORMED)?;
        let season_number = json_u64(season, "season_number").ok_or(TV_TMDB_MALFORMED)?;
        if provider_season_id == 0 {
            return Err(TV_TMDB_MALFORMED);
        }
        if season_number == 0 {
            continue;
        }
        if !season_ids.insert(provider_season_id) || !season_numbers.insert(season_number) {
            return Err(TV_TMDB_MALFORMED);
        }
        if provider_season_id == requested_provider_season_id {
            selected_season_number = Some(season_number);
        }
    }
    Ok((
        show_name.to_owned(),
        selected_season_number.ok_or(TV_TMDB_MALFORMED)?,
    ))
}

fn trusted_episode(
    provider_season_id: u64,
    season_number: u64,
    requested_provider_episode_id: u64,
    document: &str,
) -> Result<(u64, String), &'static str> {
    let value = JsonParser::new(document).parse().ok_or(TV_TMDB_MALFORMED)?;
    let object = json_object(&value).ok_or(TV_TMDB_MALFORMED)?;
    if json_u64(object, "id") != Some(provider_season_id)
        || json_u64(object, "season_number") != Some(season_number)
    {
        return Err(TV_TMDB_MALFORMED);
    }
    let episodes =
        json_array(object.get("episodes").ok_or(TV_TMDB_MALFORMED)?).ok_or(TV_TMDB_MALFORMED)?;
    let mut episode_ids = BTreeSet::new();
    let mut episode_numbers = BTreeSet::new();
    let mut selected_episode = None;
    for episode in episodes {
        let episode = json_object(episode).ok_or(TV_TMDB_MALFORMED)?;
        let provider_episode_id = json_u64(episode, "id").ok_or(TV_TMDB_MALFORMED)?;
        let episode_number = json_u64(episode, "episode_number").ok_or(TV_TMDB_MALFORMED)?;
        let episode_season_number = json_u64(episode, "season_number").ok_or(TV_TMDB_MALFORMED)?;
        let episode_name = json_string(episode, "name").ok_or(TV_TMDB_MALFORMED)?;
        if provider_episode_id == 0
            || episode_number == 0
            || episode_season_number != season_number
            || episode_name.trim().is_empty()
            || !episode_ids.insert(provider_episode_id)
            || !episode_numbers.insert(episode_number)
        {
            return Err(TV_TMDB_MALFORMED);
        }
        if provider_episode_id == requested_provider_episode_id {
            selected_episode = Some((episode_number, episode_name.to_owned()));
        }
    }
    selected_episode.ok_or(TV_TMDB_MALFORMED)
}

fn trusted_imdb_id(tmdb_tv_id: u64, document: &str) -> Result<String, &'static str> {
    let value = JsonParser::new(document).parse().ok_or(TV_TMDB_MALFORMED)?;
    let object = json_object(&value).ok_or(TV_TMDB_MALFORMED)?;
    if json_u64(object, "id") != Some(tmdb_tv_id) {
        return Err(TV_TMDB_MALFORMED);
    }
    match object.get("imdb_id") {
        None | Some(JsonValue::Null) => Err(TV_NO_IMDB_IDENTITY),
        Some(JsonValue::String(value)) if value.trim().is_empty() => Err(TV_NO_IMDB_IDENTITY),
        Some(JsonValue::String(value)) => canonical_imdb_id(value)
            .filter(|canonical| canonical == value)
            .ok_or(TV_TMDB_MALFORMED),
        _ => Err(TV_TMDB_MALFORMED),
    }
}

fn verified_apibay_releases(
    document: &str,
    requested_category: u64,
    context: &TrustedEpisodeContext,
    item_ids: &mut BTreeSet<String>,
    infohashes: &mut BTreeSet<String>,
) -> Result<Vec<ApiBayRelease>, &'static str> {
    let value = JsonParser::new(document)
        .parse()
        .ok_or(TV_APIBAY_MALFORMED)?;
    let rows = json_array(&value).ok_or(TV_APIBAY_MALFORMED)?;
    if rows.len() > MAX_PROVIDER_ROWS {
        return Err(TV_APIBAY_MALFORMED);
    }
    let mut releases = Vec::new();
    for row in rows {
        if let Some(object) = json_object(row) {
            if json_string(object, "id") == Some("0")
                && json_string(object, "name") == Some(API_BAY_NO_RESULTS_NAME)
                && json_string(object, "info_hash") == Some(API_BAY_NO_RESULTS_INFOHASH)
            {
                continue;
            }
            if let Some(item_id) = canonical_positive_string(object, "id") {
                if !item_ids.insert(item_id) {
                    return Err(TV_APIBAY_CONFLICTING);
                }
            }
            if let Some(infohash) = json_string(object, "info_hash").and_then(canonical_infohash) {
                if !infohashes.insert(infohash) {
                    return Err(TV_APIBAY_CONFLICTING);
                }
            }
        }
        if let Some(release) = verified_apibay_release(row, requested_category, context) {
            releases.push(release);
        }
    }
    Ok(releases)
}

fn verified_apibay_release(
    value: &JsonValue,
    requested_category: u64,
    context: &TrustedEpisodeContext,
) -> Option<ApiBayRelease> {
    let object = json_object(value)?;
    let provider_item_id = canonical_positive_string(object, "id")?;
    let name = json_string(object, "name")?;
    let category = canonical_positive_string(object, "category")?;
    let imdb_id = json_string(object, "imdb")?;
    let infohash = canonical_infohash(json_string(object, "info_hash")?)?;
    if name.trim().is_empty()
        || category.parse::<u64>().ok() != Some(requested_category)
        || !TV_CATEGORIES.contains(&requested_category)
        || canonical_imdb_id(imdb_id).as_deref() != Some(imdb_id)
        || imdb_id != context.imdb_id
        || !has_exact_episode_identity(name, context.season_number, context.episode_number)
    {
        return None;
    }
    Some(ApiBayRelease {
        provider_item_id,
        name: name.to_owned(),
        category,
        size_bytes: optional_unsigned_string(object, "size"),
        seeders: optional_unsigned_string(object, "seeders"),
        leechers: optional_unsigned_string(object, "leechers"),
        uploader: optional_text(object, "username"),
        status: optional_text(object, "status"),
        added: optional_unsigned_string(object, "added"),
        infohash,
    })
}

fn canonical_positive_string(object: &BTreeMap<String, JsonValue>, key: &str) -> Option<String> {
    let value = json_string(object, key)?;
    if value.is_empty()
        || value.starts_with('0')
        || value.len() > 20
        || !value.bytes().all(|character| character.is_ascii_digit())
        || value.parse::<u64>().ok()? == 0
    {
        return None;
    }
    Some(value.to_owned())
}

fn optional_unsigned_string(object: &BTreeMap<String, JsonValue>, key: &str) -> String {
    json_string(object, key)
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 20
                && value.bytes().all(|character| character.is_ascii_digit())
                && value.parse::<u64>().is_ok()
        })
        .unwrap_or("")
        .to_owned()
}

fn optional_text(object: &BTreeMap<String, JsonValue>, key: &str) -> String {
    json_string(object, key)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("")
        .to_owned()
}

fn flatten_releases(context: &TrustedEpisodeContext, releases: &[ApiBayRelease]) -> Vec<String> {
    let mut values = vec![
        context.tmdb_tv_id.to_string(),
        context.show_name.clone(),
        context.provider_season_id.to_string(),
        context.season_number.to_string(),
        context.provider_episode_id.to_string(),
        context.episode_number.to_string(),
        context.episode_name.clone(),
        context.imdb_id.clone(),
        releases.len().to_string(),
    ];
    for release in releases {
        values.extend([
            release.provider_item_id.clone(),
            release.name.clone(),
            release.category.clone(),
            release.size_bytes.clone(),
            release.seeders.clone(),
            release.leechers.clone(),
            release.uploader.clone(),
            release.status.clone(),
            release.added.clone(),
            release.infohash.clone(),
            "API Bay".to_owned(),
        ]);
    }
    values
}

fn percent_encode(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    encoded
}

pub(crate) fn has_exact_episode_identity(
    name: &str,
    season_number: u64,
    episode_number: u64,
) -> bool {
    let bytes = name.as_bytes();
    let mut identities = Vec::new();
    for start in 0..bytes.len() {
        if start > 0 && bytes[start - 1].is_ascii_alphanumeric() {
            continue;
        }
        if let Some(identity) = parse_sxe_identity(bytes, start) {
            identities.push(identity);
        }
        if let Some(identity) = parse_x_identity(bytes, start) {
            identities.push(identity);
        }
    }
    identities.len() == 1
        && identities[0].0 == season_number
        && identities[0].1 == episode_number
        && !has_episode_continuation(bytes, identities[0].2)
}

fn parse_sxe_identity(bytes: &[u8], start: usize) -> Option<(u64, u64, usize)> {
    if !bytes.get(start)?.eq_ignore_ascii_case(&b's') {
        return None;
    }
    let (season, position) = parse_component(bytes, skip_separators(bytes, start + 1))?;
    let position = skip_separators(bytes, position);
    if !bytes.get(position)?.eq_ignore_ascii_case(&b'e') {
        return None;
    }
    let (episode, end) = parse_component(bytes, skip_separators(bytes, position + 1))?;
    is_identity_end(bytes, end).then_some((season, episode, end))
}

fn parse_x_identity(bytes: &[u8], start: usize) -> Option<(u64, u64, usize)> {
    let (season, position) = parse_component(bytes, start)?;
    let position = skip_separators(bytes, position);
    if !bytes.get(position)?.eq_ignore_ascii_case(&b'x') {
        return None;
    }
    let (episode, end) = parse_component(bytes, skip_separators(bytes, position + 1))?;
    is_identity_end(bytes, end).then_some((season, episode, end))
}

fn parse_component(bytes: &[u8], start: usize) -> Option<(u64, usize)> {
    let mut end = start;
    while end < bytes.len() && bytes[end].is_ascii_digit() && end - start < 3 {
        end += 1;
    }
    if end == start || bytes.get(end).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let value = std::str::from_utf8(&bytes[start..end]).ok()?.parse().ok()?;
    (value > 0).then_some((value, end))
}

fn skip_separators(bytes: &[u8], mut position: usize) -> usize {
    while bytes.get(position).is_some_and(|character| {
        character.is_ascii_whitespace() || matches!(character, b'.' | b'_' | b'-')
    }) {
        position += 1;
    }
    position
}

fn is_identity_end(bytes: &[u8], end: usize) -> bool {
    bytes
        .get(end)
        .is_none_or(|character| !character.is_ascii_alphanumeric())
}

fn has_episode_continuation(bytes: &[u8], end: usize) -> bool {
    let position = skip_continuation_separators(bytes, end);
    let position = if bytes.get(position).is_some_and(|character| {
        character.eq_ignore_ascii_case(&b'e') || character.eq_ignore_ascii_case(&b'x')
    }) {
        skip_continuation_separators(bytes, position + 1)
    } else {
        position
    };
    parse_component(bytes, position).is_some_and(|(_, end)| is_identity_end(bytes, end))
}

fn skip_continuation_separators(bytes: &[u8], mut position: usize) -> usize {
    while bytes.get(position).is_some_and(|character| {
        character.is_ascii_whitespace() || character.is_ascii_punctuation()
    }) {
        position += 1;
    }
    position
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const DETAILS: &str = r#"{"id":701,"name":"Exact  Show — 特別版","seasons":[{"id":9000,"season_number":0},{"id":9001,"season_number":2}]}"#;
    const SEASON: &str = r#"{"id":9001,"season_number":2,"episodes":[{"id":9103,"season_number":2,"episode_number":3,"name":"第三話  —  Exact Episode"}]}"#;
    const EXTERNAL_IDS: &str = r#"{"id":701,"imdb_id":"tt0123456"}"#;
    const NO_RESULTS: &str = r#"[{"id":"0","name":"No results returned","info_hash":"0000000000000000000000000000000000000000","leechers":"0","seeders":"0","num_files":"0","size":"0","username":"","added":"0","status":"member","category":"0","imdb":""}]"#;

    fn release(id: &str, name: &str, category: &str, imdb: &str, info_hash: &str) -> String {
        format!(
            r#"{{"id":"{id}","name":"{name}","info_hash":"{info_hash}","leechers":"4","seeders":"12","size":"419000000","username":"Exact Uploader","added":"1710000000","status":"vip","category":"{category}","imdb":"{imdb}"}}"#
        )
    }

    #[test]
    fn accepts_one_boundary_aware_episode_marker_and_rejects_ambiguous_variants() {
        for name in [
            "Show.S02E03.1080p",
            "Show s-2.e-003 720p",
            "Show 2x03 WEB",
            "Show 002 x 003",
            "Show S02E03+720p",
            "Show 2x03+10bit",
        ] {
            assert!(has_exact_episode_identity(name, 2, 3), "{name}");
        }
        for name in [
            "Show S02E04",
            "Show S03E03",
            "Show S02E030",
            "Show XS02E03",
            "Show Season 2 Pack",
            "Show S02E03E04",
            "Show S02E03-E04",
            "Show S02E03-04",
            "Show S02E03+E04",
            "Show S02E03+04",
            "Show S02E03&E04",
            "Show S02E03,04",
            "Show S02E03:04",
            "Show S02E03 04",
            "Show S02E03-E-04",
            "Show S02E03 / e 04",
            "Show 2x03x04",
            "Show 2x03-04",
            "Show 2x03+04",
            "Show 2x03/04",
            "Show 2x03/X04",
            "Show 2x03 x 04",
            "Show S02E03 2x03",
            "Show S02E03 S03E03",
        ] {
            assert!(!has_exact_episode_identity(name, 2, 3), "{name}");
        }
    }

    #[test]
    fn resolves_exact_episode_identity_and_only_returns_verified_tv_rows() {
        let hash_a = "A".repeat(40);
        let hash_b = "b".repeat(40);
        let rejected = [
            ("3", "Exact Show S02E04", "205", "tt0123456"),
            ("4", "Exact Show S03E03", "205", "tt0123456"),
            ("5", "Exact Show S02E030", "205", "tt0123456"),
            ("6", "Exact Show XS02E03", "205", "tt0123456"),
            ("7", "Exact Show Season 2 Pack", "205", "tt0123456"),
            ("8", "Exact Show S02E03E04", "205", "tt0123456"),
            ("9", "Exact Show S02E03-E04", "205", "tt0123456"),
            ("10", "Exact Show S02E03-04", "205", "tt0123456"),
            ("11", "Exact Show 2x03x04", "205", "tt0123456"),
            ("12", "Exact Show S02E03 S03E03", "205", "tt0123456"),
            ("13", "Exact Show S02E03", "208", "tt0123456"),
            ("14", "Exact Show S02E03", "205", "tt9999999"),
            ("15", "Exact Show S02E03+E04", "205", "tt0123456"),
            ("16", "Exact Show S02E03+04", "205", "tt0123456"),
            ("17", "Exact Show 2x03+04", "205", "tt0123456"),
            ("18", "Exact Show S02E03&E04", "205", "tt0123456"),
            ("19", "Exact Show S02E03,04", "205", "tt0123456"),
            ("20", "Exact Show 2x03/04", "205", "tt0123456"),
            ("21", "Exact Show S02E03:04", "205", "tt0123456"),
            ("22", "Exact Show S02E03 04", "205", "tt0123456"),
            ("23", "Exact Show S02E03", "201", "tt0123456"),
            ("24", "Exact Show S02E03", "205", ""),
            ("29", "Exact Show S02E03-E-04", "205", "tt0123456"),
            ("30", "Exact Show S02E03 / E 04", "205", "tt0123456"),
            ("31", "Exact Show 2x03/x04", "205", "tt0123456"),
            ("32", "Exact Show 2x03 x 04", "205", "tt0123456"),
        ];
        let mut standard_rows = vec![release(
            "1",
            "Exact  Show — 特別版.S02E03+720p.第三話",
            "205",
            "tt0123456",
            &hash_a,
        )];
        standard_rows.extend(rejected.map(|(id, name, category, imdb)| {
            release(id, name, category, imdb, &format!("{id:0>40}"))
        }));
        standard_rows.extend([
            format!(
                r#"{{"id":"25","name":7,"info_hash":"{:0>40}","category":"205","imdb":"tt0123456"}}"#,
                "25"
            ),
            r#"{"id":"26","name":"Exact Show S02E03","category":"205","imdb":"tt0123456"}"#.to_owned(),
            format!(
                r#"{{"name":"Exact Show S02E03","info_hash":"{:0>40}","category":"205","imdb":"tt0123456"}}"#,
                "27"
            ),
            format!(
                r#"{{"id":"28","name":"Exact Show S02E03","info_hash":"{:0>40}","category":"205"}}"#,
                "28"
            ),
        ]);
        let standard = format!("[{}]", standard_rows.join(","));
        let hd = format!(
            "[{}]",
            release(
                "2",
                "Exact Show - 02x003+10bit - 2160p",
                "208",
                "tt0123456",
                &hash_b
            )
        );
        let requests = RefCell::new(Vec::new());

        let values =
            fetch_apibay_tv_releases_with(701, 9001, 9103, "fixture-token", |url, token| {
                requests
                    .borrow_mut()
                    .push((url.to_owned(), token.map(str::to_owned)));
                match url {
                    "https://api.themoviedb.org/3/tv/701" => Ok(DETAILS.to_owned()),
                    "https://api.themoviedb.org/3/tv/701/season/2" => Ok(SEASON.to_owned()),
                    "https://api.themoviedb.org/3/tv/701/external_ids" => {
                        Ok(EXTERNAL_IDS.to_owned())
                    }
                    url if url.ends_with("&cat=205") => Ok(standard.clone()),
                    url if url.ends_with("&cat=208") => Ok(hd.clone()),
                    _ => unreachable!("only native-built provider URLs are allowed"),
                }
            })
            .expect("the exact TV episode lookup must succeed");

        assert_eq!(
            values[0..9],
            [
                "701",
                "Exact  Show — 特別版",
                "9001",
                "2",
                "9103",
                "3",
                "第三話  —  Exact Episode",
                "tt0123456",
                "2"
            ]
        );
        assert_eq!(values[9], "1");
        assert_eq!(values[10], "Exact  Show — 特別版.S02E03+720p.第三話");
        assert_eq!(values[18], hash_a.to_ascii_lowercase());
        assert_eq!(values[19], "API Bay");
        assert_eq!(values[20], "2");
        assert_eq!(values[21], "Exact Show - 02x003+10bit - 2160p");
        assert_eq!(values[29], hash_b);
        assert_eq!(values[30], "API Bay");
        for compact_continuation in [
            "Exact Show S02E03+E04",
            "Exact Show S02E03+04",
            "Exact Show 2x03+04",
            "Exact Show S02E03&E04",
            "Exact Show S02E03,04",
            "Exact Show 2x03/04",
            "Exact Show S02E03:04",
            "Exact Show S02E03 04",
            "Exact Show S02E03-E-04",
            "Exact Show S02E03 / E 04",
            "Exact Show 2x03/x04",
            "Exact Show 2x03 x 04",
        ] {
            assert!(!values.iter().any(|value| value == compact_continuation));
        }
        let requests = requests.into_inner();
        assert_eq!(requests.len(), 5);
        assert_eq!(requests[0].1.as_deref(), Some("fixture-token"));
        assert_eq!(requests[1].1.as_deref(), Some("fixture-token"));
        assert_eq!(requests[2].1.as_deref(), Some("fixture-token"));
        assert_eq!(requests[3].1, None);
        assert_eq!(requests[4].1, None);
        assert_eq!(requests[3].0, "https://apibay.org/q.php?q=Exact%20%20Show%20%E2%80%94%20%E7%89%B9%E5%88%A5%E7%89%88%20S02E03&cat=205");
        assert_eq!(requests[4].0, "https://apibay.org/q.php?q=Exact%20%20Show%20%E2%80%94%20%E7%89%B9%E5%88%A5%E7%89%88%20S02E03&cat=208");
    }

    #[test]
    fn treats_both_api_bay_no_result_sentinels_as_an_empty_lookup() {
        let values = fetch_apibay_tv_releases_with(701, 9001, 9103, "token", |url, _| {
            if url.ends_with("/tv/701") {
                Ok(DETAILS.to_owned())
            } else if url.ends_with("/season/2") {
                Ok(SEASON.to_owned())
            } else if url.ends_with("/external_ids") {
                Ok(EXTERNAL_IDS.to_owned())
            } else {
                Ok(NO_RESULTS.to_owned())
            }
        })
        .expect("two ordinary no-result responses must remain a no-match");

        assert_eq!(values[8], "0");
        assert_eq!(values.len(), 9);
    }

    #[test]
    fn separator_hidden_episode_continuations_produce_no_verified_rows() {
        let standard = format!(
            "[{},{}]",
            release(
                "1",
                "Exact Show S02E03-E-04",
                "205",
                "tt0123456",
                &"1".repeat(40)
            ),
            release(
                "2",
                "Exact Show S02E03 / E 04",
                "205",
                "tt0123456",
                &"2".repeat(40)
            )
        );
        let hd = format!(
            "[{},{}]",
            release(
                "3",
                "Exact Show 2x03/x04",
                "208",
                "tt0123456",
                &"3".repeat(40)
            ),
            release(
                "4",
                "Exact Show 2x03 x 04",
                "208",
                "tt0123456",
                &"4".repeat(40)
            )
        );
        let values = fetch_apibay_tv_releases_with(701, 9001, 9103, "token", |url, _| {
            if url.ends_with("/tv/701") {
                Ok(DETAILS.to_owned())
            } else if url.ends_with("/season/2") {
                Ok(SEASON.to_owned())
            } else if url.ends_with("/external_ids") {
                Ok(EXTERNAL_IDS.to_owned())
            } else if url.ends_with("cat=205") {
                Ok(standard.clone())
            } else {
                Ok(hd.clone())
            }
        })
        .expect("ambiguous continuations must be excluded without a fallback");

        assert_eq!(values[8], "0");
        assert_eq!(values.len(), 9);
    }

    #[test]
    fn rejects_duplicate_provider_claims_before_release_identity_filtering() {
        let shared_hash = "a".repeat(40);
        let other_hash = "b".repeat(40);
        let exact = release("1", "Show S02E03", "205", "tt0123456", &shared_hash);
        let mut conflicting_documents = vec![
            format!(
                "[{},{}]",
                exact,
                release("1", "Show 2x03", "205", "tt0123456", &other_hash)
            ),
            format!(
                "[{},{}]",
                exact,
                release("2", "Show 2x03", "205", "tt0123456", &shared_hash)
            ),
        ];

        for (name, category, imdb) in [
            ("Show S02E04", "205", "tt0123456"),
            ("Show S02E03", "208", "tt0123456"),
            ("Show S02E03", "205", "tt9999999"),
        ] {
            for (item_id, infohash) in [("1", &other_hash), ("2", &shared_hash)] {
                conflicting_documents.push(format!(
                    "[{},{}]",
                    exact,
                    release(item_id, name, category, imdb, infohash)
                ));
            }
        }

        for (item_id, infohash) in [("1", &other_hash), ("2", &shared_hash)] {
            let malformed = format!(
                r#"{{"id":"{item_id}","name":7,"info_hash":"{infohash}","category":"205","imdb":"tt0123456"}}"#
            );
            conflicting_documents.push(format!("[{exact},{malformed}]"));
        }
        let missing_item_id = format!(
            r#"{{"name":"Show S02E04","info_hash":"{shared_hash}","category":"205","imdb":"tt0123456"}}"#
        );
        conflicting_documents.push(format!("[{exact},{missing_item_id}]"));
        let malformed_item_id = format!(
            r#"{{"id":"invalid","name":"Show S02E04","info_hash":"{shared_hash}","category":"205","imdb":"tt0123456"}}"#
        );
        conflicting_documents.push(format!("[{exact},{malformed_item_id}]"));

        for standard in conflicting_documents {
            let result = fetch_apibay_tv_releases_with(701, 9001, 9103, "token", |url, _| {
                if url.ends_with("/tv/701") {
                    Ok(DETAILS.to_owned())
                } else if url.ends_with("/season/2") {
                    Ok(SEASON.to_owned())
                } else if url.ends_with("/external_ids") {
                    Ok(EXTERNAL_IDS.to_owned())
                } else if url.ends_with("cat=205") {
                    Ok(standard.clone())
                } else {
                    Ok("[]".to_owned())
                }
            });
            assert_eq!(result, Err(TV_APIBAY_CONFLICTING));
        }
    }

    #[test]
    fn rejects_provider_claim_conflicts_across_approved_category_responses() {
        let shared_hash = "a".repeat(40);
        let other_hash = "b".repeat(40);
        let standard = format!(
            "[{}]",
            release("1", "Exact Show S02E03", "205", "tt0123456", &shared_hash)
        );

        for hd in [
            release("1", "Exact Show 2x03", "208", "tt0123456", &other_hash),
            release("2", "Exact Show 2x03", "208", "tt0123456", &shared_hash),
        ] {
            let hd = format!("[{hd}]");
            let result = fetch_apibay_tv_releases_with(701, 9001, 9103, "token", |url, _| {
                if url.ends_with("/tv/701") {
                    Ok(DETAILS.to_owned())
                } else if url.ends_with("/season/2") {
                    Ok(SEASON.to_owned())
                } else if url.ends_with("/external_ids") {
                    Ok(EXTERNAL_IDS.to_owned())
                } else if url.ends_with("cat=205") {
                    Ok(standard.clone())
                } else {
                    Ok(hd.clone())
                }
            });
            assert_eq!(result, Err(TV_APIBAY_CONFLICTING));
        }
    }

    #[test]
    fn rejects_invalid_context_before_an_api_bay_request() {
        for (document, error) in [
            (
                r#"{"id":702,"name":"Other","seasons":[]}"#,
                TV_TMDB_MALFORMED,
            ),
            (
                r#"{"id":701,"name":"Show","seasons":[]}"#,
                TV_TMDB_MALFORMED,
            ),
        ] {
            let dispatches = RefCell::new(0);
            let result = fetch_apibay_tv_releases_with(701, 9001, 9103, "token", |url, _| {
                *dispatches.borrow_mut() += 1;
                if url.ends_with("/tv/701") {
                    Ok(document.to_owned())
                } else {
                    unreachable!("invalid details must stop dispatch")
                }
            });
            assert_eq!(result, Err(error));
            assert_eq!(dispatches.into_inner(), 1);
        }

        let result = fetch_apibay_tv_releases_with(701, 9001, 9103, "token", |url, _| {
            if url.ends_with("/tv/701") {
                Ok(DETAILS.to_owned())
            } else if url.ends_with("/season/2") {
                Ok(SEASON.to_owned())
            } else if url.ends_with("/external_ids") {
                Ok(r#"{"id":701,"imdb_id":null}"#.to_owned())
            } else {
                unreachable!("missing IMDb must stop before API Bay")
            }
        });
        assert_eq!(result, Err(TV_NO_IMDB_IDENTITY));
    }

    #[test]
    fn rejects_changed_or_duplicate_native_context_before_api_bay() {
        for details in [
            r#"{"id":701,"name":"Exact Show","seasons":[{"id":9001,"season_number":2},{"id":9001,"season_number":3}]}"#,
            r#"{"id":701,"name":"Exact Show","seasons":[{"id":9001,"season_number":2},{"id":9002,"season_number":2}]}"#,
        ] {
            let api_bay_requests = RefCell::new(0);
            let result = fetch_apibay_tv_releases_with(701, 9001, 9103, "token", |url, _| {
                if url.ends_with("/tv/701") {
                    Ok(details.to_owned())
                } else {
                    *api_bay_requests.borrow_mut() += 1;
                    Ok("[]".to_owned())
                }
            });
            assert_eq!(result, Err(TV_TMDB_MALFORMED));
            assert_eq!(api_bay_requests.into_inner(), 0);
        }

        for season in [
            r#"{"id":9002,"season_number":2,"episodes":[]}"#,
            r#"{"id":9001,"season_number":3,"episodes":[]}"#,
            r#"{"id":9001,"season_number":2,"episodes":[{"id":9104,"season_number":2,"episode_number":4,"name":"Other"}]}"#,
            r#"{"id":9001,"season_number":2,"episodes":[{"id":9103,"season_number":2,"episode_number":3,"name":"Selected"},{"id":9103,"season_number":2,"episode_number":4,"name":"Duplicate ID"}]}"#,
            r#"{"id":9001,"season_number":2,"episodes":[{"id":9103,"season_number":2,"episode_number":3,"name":"Selected"},{"id":9104,"season_number":2,"episode_number":3,"name":"Duplicate number"}]}"#,
            r#"{"id":9001,"season_number":2,"episodes":[{"id":9103,"season_number":3,"episode_number":3,"name":"Wrong season"}]}"#,
        ] {
            let api_bay_requests = RefCell::new(0);
            let result = fetch_apibay_tv_releases_with(701, 9001, 9103, "token", |url, _| {
                if url.ends_with("/tv/701") {
                    Ok(DETAILS.to_owned())
                } else if url.ends_with("/season/2") {
                    Ok(season.to_owned())
                } else {
                    *api_bay_requests.borrow_mut() += 1;
                    Ok("[]".to_owned())
                }
            });
            assert_eq!(result, Err(TV_TMDB_MALFORMED));
            assert_eq!(api_bay_requests.into_inner(), 0);
        }

        for external_ids in [
            r#"{"id":702,"imdb_id":"tt0123456"}"#,
            r#"{"id":701,"imdb_id":"TT0123456"}"#,
        ] {
            let api_bay_requests = RefCell::new(0);
            let result = fetch_apibay_tv_releases_with(701, 9001, 9103, "token", |url, _| {
                if url.ends_with("/tv/701") {
                    Ok(DETAILS.to_owned())
                } else if url.ends_with("/season/2") {
                    Ok(SEASON.to_owned())
                } else if url.ends_with("/external_ids") {
                    Ok(external_ids.to_owned())
                } else {
                    *api_bay_requests.borrow_mut() += 1;
                    Ok("[]".to_owned())
                }
            });
            assert_eq!(result, Err(TV_TMDB_MALFORMED));
            assert_eq!(api_bay_requests.into_inner(), 0);
        }
    }

    #[test]
    fn maps_tmdb_and_api_bay_failures_to_distinct_local_errors() {
        for (error, expected) in [
            (
                MovieProviderRequestError::Unauthorized,
                TV_TMDB_UNAUTHORIZED,
            ),
            (MovieProviderRequestError::RateLimited, TV_TMDB_RATE_LIMITED),
            (MovieProviderRequestError::Network, TV_TMDB_NETWORK_ERROR),
            (MovieProviderRequestError::Provider, TV_TMDB_PROVIDER_ERROR),
        ] {
            assert_eq!(
                fetch_apibay_tv_releases_with(701, 9001, 9103, "token", |_, _| Err(error)),
                Err(expected)
            );
        }

        for (error, expected) in [
            (
                MovieProviderRequestError::SourceUnavailable,
                TV_APIBAY_SOURCE_UNAVAILABLE,
            ),
            (MovieProviderRequestError::Network, TV_APIBAY_NETWORK_ERROR),
            (
                MovieProviderRequestError::Provider,
                TV_APIBAY_PROVIDER_ERROR,
            ),
        ] {
            let result = fetch_apibay_tv_releases_with(701, 9001, 9103, "token", |url, _| {
                if url.ends_with("/tv/701") {
                    Ok(DETAILS.to_owned())
                } else if url.ends_with("/season/2") {
                    Ok(SEASON.to_owned())
                } else if url.ends_with("/external_ids") {
                    Ok(EXTERNAL_IDS.to_owned())
                } else {
                    Err(error)
                }
            });
            assert_eq!(result, Err(expected));
        }
    }

    fn push_bencoded_bytes(target: &mut Vec<u8>, value: &[u8]) {
        target.extend_from_slice(value.len().to_string().as_bytes());
        target.push(b':');
        target.extend_from_slice(value);
    }

    fn deterministic_tv_metainfo() -> Vec<u8> {
        let mut info = b"d5:filesl".to_vec();
        for (size, components) in [
            (
                5_u64,
                vec![
                    "Exact  Show — 特別版".as_bytes(),
                    "第三話  —  Exact Episode.mkv".as_bytes(),
                ],
            ),
            (
                4_u64,
                vec!["Exact  Show — 特別版".as_bytes(), "notes.txt".as_bytes()],
            ),
        ] {
            info.extend_from_slice(b"d6:lengthi");
            info.extend_from_slice(size.to_string().as_bytes());
            info.extend_from_slice(b"e4:pathl");
            for component in components {
                push_bencoded_bytes(&mut info, component);
            }
            info.extend_from_slice(b"ee");
        }
        info.extend_from_slice(b"e4:name");
        push_bencoded_bytes(&mut info, "Exact  Show — 特別版 S02E03".as_bytes());
        info.extend_from_slice(b"12:piece lengthi8e6:pieces40:");
        info.extend_from_slice(b"aaaaaaaaaaaaaaaaaaaabbbbbbbbbbbbbbbbbbbb");
        info.push(b'e');
        let mut metainfo = b"d4:info".to_vec();
        metainfo.extend_from_slice(&info);
        metainfo.push(b'e');
        metainfo
    }

    fn state_with_exact_tv_release(
        metainfo: &[u8],
    ) -> (TvReleaseState, TvTorrentInspectionRequest) {
        let infohash = parse_torrent_metadata(metainfo)
            .expect("fixture metainfo must parse")
            .infohash;
        let state = TvReleaseState::default();
        let generation = state
            .begin_release_lookup()
            .expect("release lookup must begin");
        let row = release(
            "1001",
            "Exact  Show — 特別版.S02E03+720p.第三話",
            "205",
            "tt0123456",
            &infohash,
        );
        fetch_apibay_tv_releases_for_state_with(
            &state,
            generation,
            701,
            9001,
            9103,
            "fixture-token",
            |url, _| {
                if url.ends_with("/tv/701") {
                    Ok(DETAILS.to_owned())
                } else if url.ends_with("/season/2") {
                    Ok(SEASON.to_owned())
                } else if url.ends_with("/external_ids") {
                    Ok(EXTERNAL_IDS.to_owned())
                } else if url.ends_with("cat=205") {
                    Ok(format!("[{row}]"))
                } else {
                    Ok(NO_RESULTS.to_owned())
                }
            },
        )
        .expect("exact release context must finish");
        (
            state,
            TvTorrentInspectionRequest {
                tmdb_tv_id: 701,
                show_name: "Exact  Show — 特別版".to_owned(),
                provider_season_id: 9001,
                season_number: 2,
                provider_episode_id: 9103,
                episode_number: 3,
                episode_name: "第三話  —  Exact Episode".to_owned(),
                imdb_id: "tt0123456".to_owned(),
                provider_item_id: "1001".to_owned(),
                provider_category: "205".to_owned(),
                release_name: "Exact  Show — 特別版.S02E03+720p.第三話".to_owned(),
                expected_infohash: infohash,
            },
        )
    }

    #[test]
    fn caches_exact_tv_metainfo_and_revalidates_only_explicit_selected_files() {
        let metainfo = deterministic_tv_metainfo();
        let (state, request) = state_with_exact_tv_release(&metainfo);
        let ticket = state
            .begin_inspection(request.clone())
            .expect("trusted TV inspection must begin");
        assert!(ticket.magnet_url().starts_with(&format!(
            "magnet:?xt=urn:btih:{}&dn=",
            request.expected_infohash
        )));
        let response = state
            .finish_inspection(ticket, metainfo.clone())
            .expect("exact TV metainfo must verify");
        assert_eq!(response[1], "Exact  Show — 特別版 S02E03");
        assert_eq!(response[2], request.expected_infohash);
        assert_eq!(response[3], "9");
        assert_eq!(response.len(), 8);

        let source = state
            .verified_download_source(&response[0], &[0])
            .expect("one selected episode file must revalidate");
        assert_eq!(source.selected_files.len(), 1);
        assert_eq!(source.selected_files[0].file_id, 0);
        assert_eq!(source.bytes, metainfo);
        assert_eq!(source.tv_identity.as_ref(), Some(&request.identity()));
        assert!(source.movie_identity.is_none());
    }

    #[test]
    fn rejects_fabricated_or_stale_tv_context_and_saves_only_cached_bytes() {
        let metainfo = deterministic_tv_metainfo();
        let (state, request) = state_with_exact_tv_release(&metainfo);
        let mut fabricated = request.clone();
        fabricated.provider_category = "208".to_owned();
        assert_eq!(
            state.begin_inspection(fabricated).err(),
            Some(TV_TORRENT_CONTEXT_INVALID)
        );

        let mismatch_ticket = state
            .begin_inspection(request.clone())
            .expect("trusted inspection must begin before hash verification");
        let mut mismatched_metainfo = metainfo.clone();
        let piece_byte = mismatched_metainfo
            .iter()
            .rposition(|byte| *byte == b'b')
            .expect("fixture piece bytes must exist");
        mismatched_metainfo[piece_byte] = b'c';
        assert_eq!(
            state.finish_inspection(mismatch_ticket, mismatched_metainfo),
            Err(TV_TORRENT_INFOHASH_MISMATCH)
        );

        let ticket = state
            .begin_inspection(request.clone())
            .expect("current inspection must begin");
        let response = state
            .finish_inspection(ticket, metainfo.clone())
            .expect("metainfo must cache");
        let mut writes = 0;
        assert_eq!(
            state.save_verified_torrent_with(
                &response[0],
                |_| None,
                |_, _| {
                    writes += 1;
                    Ok(())
                }
            ),
            Ok(false)
        );
        assert_eq!(writes, 0);
        assert_eq!(
            state.save_verified_torrent_with(
                &response[0],
                |_| Some(PathBuf::from("fixture.torrent")),
                |path, bytes| {
                    assert_eq!(path, Path::new("fixture.torrent"));
                    assert_eq!(bytes, metainfo);
                    writes += 1;
                    Ok(())
                }
            ),
            Ok(true)
        );
        assert_eq!(writes, 1);

        let stale_ticket = state
            .begin_inspection(request)
            .expect("second inspection must begin");
        state
            .begin_release_lookup()
            .expect("new release lookup must invalidate inspection");
        assert_eq!(
            state.finish_inspection(stale_ticket, metainfo),
            Err(TV_TORRENT_STALE)
        );
    }
}
