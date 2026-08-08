use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs::OpenOptions,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
};

const SUKEBEI_DOWNLOAD_PREFIX: &str = "https://sukebei.nyaa.si/download/";
const SUKEBEI_VIEW_PREFIX: &str = "https://sukebei.nyaa.si/view/";
const YTS_DOWNLOAD_PREFIX: &str = "https://yts.mx/torrent/download/";
const PROVIDER_ITEM_ID_MAX_DIGITS: usize = 20;
const SHA1_DIGEST_BYTES: usize = 20;
const TORRENT_MAX_BYTES: usize = 2 * 1024 * 1024;
const TORRENT_MAX_REDIRECTS: usize = 3;
const TORRENT_STATUS_MARKER: &[u8] = b"\nAUTO_VIDEO_TORRENT_STATUS:";
const TORRENT_REDIRECT_MARKER: &[u8] = b"\nAUTO_VIDEO_TORRENT_REDIRECT:";

pub const VR_TORRENT_CONTEXT_INVALID: &str = "vr_torrent_context_invalid";
pub const VR_TORRENT_INFOHASH_MISMATCH: &str = "vr_torrent_infohash_mismatch";
pub const VR_TORRENT_MALFORMED: &str = "vr_torrent_malformed";
pub const VR_TORRENT_NETWORK_ERROR: &str = "vr_torrent_network_error";
pub const VR_TORRENT_PROVIDER_ERROR: &str = "vr_torrent_provider_error";
pub const VR_TORRENT_SAVE_FAILED: &str = "vr_torrent_save_failed";
pub const VR_TORRENT_SOURCE_UNAVAILABLE: &str = "vr_torrent_source_unavailable";
pub const VR_TORRENT_STALE: &str = "vr_torrent_stale";
pub const VR_TORRENT_UNSUPPORTED: &str = "vr_torrent_unsupported";
pub const ADULT_TORRENT_CONTEXT_INVALID: &str = "adult_torrent_context_invalid";
pub const ADULT_TORRENT_INFOHASH_MISMATCH: &str = "adult_torrent_infohash_mismatch";
pub const ADULT_TORRENT_MALFORMED: &str = "adult_torrent_malformed";
pub const ADULT_TORRENT_NETWORK_ERROR: &str = "adult_torrent_network_error";
pub const ADULT_TORRENT_PROVIDER_ERROR: &str = "adult_torrent_provider_error";
pub const ADULT_TORRENT_SAVE_FAILED: &str = "adult_torrent_save_failed";
pub const ADULT_TORRENT_SOURCE_UNAVAILABLE: &str = "adult_torrent_source_unavailable";
pub const ADULT_TORRENT_STALE: &str = "adult_torrent_stale";
pub const ADULT_TORRENT_UNSUPPORTED: &str = "adult_torrent_unsupported";
pub const MOVIE_NO_IMDB_IDENTITY: &str = "movie_no_imdb_identity";
pub const MOVIE_TMDB_MALFORMED: &str = "movie_tmdb_malformed";
pub const MOVIE_TORRENT_CONTEXT_INVALID: &str = "movie_torrent_context_invalid";
pub const MOVIE_TORRENT_INFOHASH_MISMATCH: &str = "movie_torrent_infohash_mismatch";
pub const MOVIE_TORRENT_MALFORMED: &str = "movie_torrent_malformed";
pub const MOVIE_TORRENT_NETWORK_ERROR: &str = "movie_torrent_network_error";
pub const MOVIE_TORRENT_PROVIDER_ERROR: &str = "movie_torrent_provider_error";
pub const MOVIE_TORRENT_SAVE_FAILED: &str = "movie_torrent_save_failed";
pub const MOVIE_TORRENT_SOURCE_UNAVAILABLE: &str = "movie_torrent_source_unavailable";
pub const MOVIE_TORRENT_STALE: &str = "movie_torrent_stale";
pub const MOVIE_TORRENT_UNSUPPORTED: &str = "movie_torrent_unsupported";
pub const MOVIE_YTS_CONFLICTING_PROVIDER: &str = "movie_yts_conflicting_provider";
pub const MOVIE_YTS_MALFORMED: &str = "movie_yts_malformed";

#[derive(Clone, Debug, PartialEq, Eq)]
struct TrustedArtifact {
    code: String,
    release_name: String,
    provider_item_id: String,
    torrent_url: String,
    expected_infohash: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TorrentInspectionRequest {
    pub code: String,
    pub release_name: String,
    pub provider_item_id: String,
    pub torrent_url: String,
    pub expected_infohash: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TrustedMovieContext {
    tmdb_movie_id: u64,
    tmdb_title: String,
    release_date: Option<String>,
    imdb_id: String,
    provider_movie_id: u64,
    provider_title: Option<String>,
    provider_year: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TrustedMovieTorrent {
    row_id: String,
    quality: Option<String>,
    type_label: Option<String>,
    video_codec: Option<String>,
    size: Option<String>,
    size_bytes: Option<String>,
    seeds: Option<String>,
    peers: Option<String>,
    expected_infohash: Option<String>,
    torrent_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TrustedMovieReleaseSet {
    generation: u64,
    context: TrustedMovieContext,
    torrents: Vec<TrustedMovieTorrent>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MovieTorrentInspectionRequest {
    pub tmdb_movie_id: u64,
    pub tmdb_title: String,
    pub release_date: Option<String>,
    pub imdb_id: String,
    pub provider_movie_id: u64,
    pub provider_title: Option<String>,
    pub provider_year: Option<String>,
    pub row_id: String,
    pub quality: Option<String>,
    pub type_label: Option<String>,
    pub video_codec: Option<String>,
    pub size: Option<String>,
    pub size_bytes: Option<String>,
    pub seeds: Option<String>,
    pub peers: Option<String>,
    pub expected_infohash: String,
    pub torrent_url: String,
}

impl MovieTorrentInspectionRequest {
    fn context(&self) -> TrustedMovieContext {
        TrustedMovieContext {
            tmdb_movie_id: self.tmdb_movie_id,
            tmdb_title: self.tmdb_title.clone(),
            release_date: self.release_date.clone(),
            imdb_id: self.imdb_id.clone(),
            provider_movie_id: self.provider_movie_id,
            provider_title: self.provider_title.clone(),
            provider_year: self.provider_year.clone(),
        }
    }

    fn torrent(&self) -> TrustedMovieTorrent {
        TrustedMovieTorrent {
            row_id: self.row_id.clone(),
            quality: self.quality.clone(),
            type_label: self.type_label.clone(),
            video_codec: self.video_codec.clone(),
            size: self.size.clone(),
            size_bytes: self.size_bytes.clone(),
            seeds: self.seeds.clone(),
            peers: self.peers.clone(),
            expected_infohash: Some(self.expected_infohash.clone()),
            torrent_url: Some(self.torrent_url.clone()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MovieDownloadIdentity {
    pub tmdb_movie_id: u64,
    pub tmdb_title: String,
    pub release_date: Option<String>,
    pub imdb_id: String,
    pub provider_movie_id: u64,
    pub provider_title: Option<String>,
    pub provider_year: Option<String>,
    pub row_id: String,
    pub quality: Option<String>,
    pub type_label: Option<String>,
    pub video_codec: Option<String>,
    pub size: Option<String>,
    pub size_bytes: Option<String>,
    pub seeds: Option<String>,
    pub peers: Option<String>,
    pub expected_infohash: String,
    pub torrent_url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TvDownloadIdentity {
    pub tmdb_tv_id: u64,
    pub show_name: String,
    pub provider_season_id: u64,
    pub season_number: u64,
    pub provider_episode_id: u64,
    pub episode_number: u64,
    pub episode_name: String,
    pub imdb_id: String,
    pub provider_item_id: String,
    pub provider_category: String,
    pub release_name: String,
    pub expected_infohash: String,
}

impl TvDownloadIdentity {
    pub(crate) fn is_valid(&self) -> bool {
        self.tmdb_tv_id > 0
            && !self.show_name.trim().is_empty()
            && self.provider_season_id > 0
            && self.season_number > 0
            && self.provider_episode_id > 0
            && self.episode_number > 0
            && !self.episode_name.trim().is_empty()
            && canonical_imdb_id(&self.imdb_id).as_deref() == Some(self.imdb_id.as_str())
            && provider_item_id(&self.provider_item_id).as_deref()
                == Some(self.provider_item_id.as_str())
            && matches!(self.provider_category.as_str(), "205" | "208")
            && !self.release_name.trim().is_empty()
            && crate::tv_release::has_exact_episode_identity(
                &self.release_name,
                self.season_number,
                self.episode_number,
            )
            && canonical_infohash(&self.expected_infohash).as_deref()
                == Some(self.expected_infohash.as_str())
    }
}

impl From<&MovieTorrentInspectionRequest> for MovieDownloadIdentity {
    fn from(request: &MovieTorrentInspectionRequest) -> Self {
        Self {
            tmdb_movie_id: request.tmdb_movie_id,
            tmdb_title: request.tmdb_title.clone(),
            release_date: request.release_date.clone(),
            imdb_id: request.imdb_id.clone(),
            provider_movie_id: request.provider_movie_id,
            provider_title: request.provider_title.clone(),
            provider_year: request.provider_year.clone(),
            row_id: request.row_id.clone(),
            quality: request.quality.clone(),
            type_label: request.type_label.clone(),
            video_codec: request.video_codec.clone(),
            size: request.size.clone(),
            size_bytes: request.size_bytes.clone(),
            seeds: request.seeds.clone(),
            peers: request.peers.clone(),
            expected_infohash: request.expected_infohash.clone(),
            torrent_url: request.torrent_url.clone(),
        }
    }
}

impl MovieDownloadIdentity {
    fn is_valid(&self) -> bool {
        fn optional_text_is_valid(value: &Option<String>) -> bool {
            value.as_ref().is_none_or(|value| !value.trim().is_empty())
        }

        fn optional_number_is_valid(value: &Option<String>, require_positive: bool) -> bool {
            value.as_ref().is_none_or(|value| {
                value
                    .parse::<u64>()
                    .ok()
                    .filter(|number| !require_positive || *number > 0)
                    .is_some_and(|number| number.to_string() == *value)
            })
        }

        let row_prefix = format!("{}:", self.provider_movie_id);
        self.tmdb_movie_id > 0
            && !self.tmdb_title.trim().is_empty()
            && self.release_date.as_deref().is_none_or(valid_release_date)
            && canonical_imdb_id(&self.imdb_id).as_deref() == Some(self.imdb_id.as_str())
            && self.provider_movie_id > 0
            && optional_text_is_valid(&self.provider_title)
            && optional_number_is_valid(&self.provider_year, true)
            && self.row_id.strip_prefix(&row_prefix).is_some_and(|index| {
                index
                    .parse::<usize>()
                    .ok()
                    .is_some_and(|number| number.to_string() == index)
            })
            && optional_text_is_valid(&self.quality)
            && optional_text_is_valid(&self.type_label)
            && optional_text_is_valid(&self.video_codec)
            && optional_text_is_valid(&self.size)
            && optional_number_is_valid(&self.size_bytes, false)
            && optional_number_is_valid(&self.seeds, false)
            && optional_number_is_valid(&self.peers, false)
            && canonical_infohash(&self.expected_infohash).as_deref()
                == Some(self.expected_infohash.as_str())
    }
}

impl From<TorrentInspectionRequest> for TrustedArtifact {
    fn from(request: TorrentInspectionRequest) -> Self {
        Self {
            code: request.code,
            release_name: request.release_name,
            provider_item_id: request.provider_item_id,
            torrent_url: request.torrent_url,
            expected_infohash: request.expected_infohash,
        }
    }
}

#[derive(Default)]
struct TrustedReleaseFeed {
    generation: u64,
    code: String,
    artifacts: Vec<TrustedArtifact>,
}

struct CachedTorrent {
    artifact: TrustedArtifact,
    generation: u64,
    inspection_id: String,
    default_file_name: String,
    bytes: Vec<u8>,
    metadata: TorrentMetadata,
}

struct TorrentContext {
    inspection_id_prefix: &'static str,
    release_generation: u64,
    inspection_generation: u64,
    release_feed: Option<TrustedReleaseFeed>,
    cached_torrent: Option<CachedTorrent>,
}

impl Default for TorrentContext {
    fn default() -> Self {
        Self {
            inspection_id_prefix: "vr",
            release_generation: 0,
            inspection_generation: 0,
            release_feed: None,
            cached_torrent: None,
        }
    }
}

#[derive(Clone)]
struct TorrentState(Arc<Mutex<TorrentContext>>);

impl TorrentState {
    fn new(inspection_id_prefix: &'static str) -> Self {
        Self(Arc::new(Mutex::new(TorrentContext {
            inspection_id_prefix,
            ..TorrentContext::default()
        })))
    }
}

#[derive(Clone)]
pub struct VrTorrentState(TorrentState);

impl Default for VrTorrentState {
    fn default() -> Self {
        Self(TorrentState::new("vr"))
    }
}

#[derive(Clone)]
pub struct AdultTorrentState(TorrentState);

impl Default for AdultTorrentState {
    fn default() -> Self {
        Self(TorrentState::new("adult"))
    }
}

struct CachedMovieTorrent {
    generation: u64,
    inspection_id: String,
    default_file_name: String,
    identity: MovieDownloadIdentity,
    bytes: Vec<u8>,
    metadata: TorrentMetadata,
}

#[derive(Default)]
struct MovieTorrentContext {
    release_generation: u64,
    inspection_generation: u64,
    release_set: Option<TrustedMovieReleaseSet>,
    cached_torrent: Option<CachedMovieTorrent>,
}

#[derive(Clone, Default)]
pub struct MovieTorrentState(Arc<Mutex<MovieTorrentContext>>);

impl MovieTorrentState {
    pub fn begin_release_lookup(&self) -> Result<u64, &'static str> {
        let mut context = self.0.lock().map_err(|_| MOVIE_TORRENT_PROVIDER_ERROR)?;
        context.release_generation = context.release_generation.wrapping_add(1);
        context.inspection_generation = context.inspection_generation.wrapping_add(1);
        context.release_set = None;
        context.cached_torrent = None;
        Ok(context.release_generation)
    }

    pub fn finish_release_lookup(
        &self,
        generation: u64,
        requested_tmdb_movie_id: u64,
        tmdb_details_document: &str,
        tmdb_external_ids_document: &str,
        yts_document: &str,
    ) -> Result<Vec<String>, &'static str> {
        let trusted_movie = trusted_movie_release_set(
            generation,
            requested_tmdb_movie_id,
            tmdb_details_document,
            tmdb_external_ids_document,
            yts_document,
        )?;
        let response = encode_movie_release_set(&trusted_movie);
        let mut context = self.0.lock().map_err(|_| MOVIE_TORRENT_PROVIDER_ERROR)?;
        if context.release_generation != generation {
            return Err(MOVIE_TORRENT_STALE);
        }
        context.release_set = Some(trusted_movie);
        Ok(response)
    }

    pub fn invalidate_inspection(&self) -> Result<(), &'static str> {
        let mut context = self.0.lock().map_err(|_| MOVIE_TORRENT_PROVIDER_ERROR)?;
        context.inspection_generation = context.inspection_generation.wrapping_add(1);
        context.cached_torrent = None;
        Ok(())
    }

    pub fn invalidate_release_context(&self) -> Result<(), &'static str> {
        self.begin_release_lookup().map(|_| ())
    }

    pub fn begin_inspection(
        &self,
        request: &MovieTorrentInspectionRequest,
    ) -> Result<(u64, u64), &'static str> {
        let mut context = self.0.lock().map_err(|_| MOVIE_TORRENT_PROVIDER_ERROR)?;
        context.inspection_generation = context.inspection_generation.wrapping_add(1);
        context.cached_torrent = None;
        let release_set = context
            .release_set
            .as_ref()
            .filter(|release_set| {
                release_set.generation == context.release_generation
                    && release_set.context == request.context()
            })
            .ok_or(MOVIE_TORRENT_CONTEXT_INVALID)?;
        if !release_set.torrents.contains(&request.torrent()) {
            return Err(MOVIE_TORRENT_CONTEXT_INVALID);
        }
        Ok((context.release_generation, context.inspection_generation))
    }

    fn validate_inspection(
        &self,
        release_generation: u64,
        inspection_generation: u64,
        request: &MovieTorrentInspectionRequest,
    ) -> Result<(), &'static str> {
        let context = self.0.lock().map_err(|_| MOVIE_TORRENT_PROVIDER_ERROR)?;
        let request_is_current = context.release_set.as_ref().is_some_and(|release_set| {
            release_set.generation == release_generation
                && release_set.context == request.context()
                && release_set.torrents.contains(&request.torrent())
        });
        if context.release_generation != release_generation
            || context.inspection_generation != inspection_generation
            || !request_is_current
        {
            return Err(MOVIE_TORRENT_CONTEXT_INVALID);
        }
        Ok(())
    }

    fn finish_inspection(
        &self,
        release_generation: u64,
        inspection_generation: u64,
        request: &MovieTorrentInspectionRequest,
        bytes: Vec<u8>,
        metadata: TorrentMetadata,
    ) -> Result<String, &'static str> {
        let mut context = self.0.lock().map_err(|_| MOVIE_TORRENT_PROVIDER_ERROR)?;
        let request_is_current = context.release_set.as_ref().is_some_and(|release_set| {
            release_set.generation == release_generation
                && release_set.context == request.context()
                && release_set.torrents.contains(&request.torrent())
        });
        if context.release_generation != release_generation
            || context.inspection_generation != inspection_generation
            || !request_is_current
        {
            return Err(MOVIE_TORRENT_STALE);
        }

        let inspection_id = format!(
            "movie-{release_generation}-{inspection_generation}-{}",
            request.expected_infohash
        );
        context.cached_torrent = Some(CachedMovieTorrent {
            generation: inspection_generation,
            inspection_id: inspection_id.clone(),
            default_file_name: format!(
                "movie-{}-{}.torrent",
                request.tmdb_movie_id, request.expected_infohash
            ),
            identity: MovieDownloadIdentity::from(request),
            bytes,
            metadata,
        });
        Ok(inspection_id)
    }

    pub fn verified_download_source(
        &self,
        inspection_id: &str,
        selected_file_ids: &[usize],
    ) -> Result<VerifiedDownloadSource, VerifiedDownloadSourceError> {
        let cached = {
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

        let (identity, bytes, cached_metadata) = cached;
        revalidate_persisted_movie_download_source(
            &bytes,
            &identity,
            &cached_metadata.infohash,
            selected_file_ids,
        )
    }
}

impl TorrentState {
    fn begin_release_lookup(&self) -> Result<u64, &'static str> {
        let mut context = self.0.lock().map_err(|_| VR_TORRENT_PROVIDER_ERROR)?;
        context.release_generation = context.release_generation.wrapping_add(1);
        context.inspection_generation = context.inspection_generation.wrapping_add(1);
        context.release_feed = None;
        context.cached_torrent = None;
        Ok(context.release_generation)
    }

    fn finish_release_lookup(
        &self,
        generation: u64,
        code: &str,
        document: &str,
    ) -> Result<(), &'static str> {
        let artifacts = trusted_artifacts_from_feed(document, code);
        let mut context = self.0.lock().map_err(|_| VR_TORRENT_PROVIDER_ERROR)?;
        if context.release_generation != generation {
            return Err(VR_TORRENT_STALE);
        }
        context.release_feed = Some(TrustedReleaseFeed {
            generation,
            code: code.to_owned(),
            artifacts,
        });
        Ok(())
    }

    fn invalidate_inspection(&self) -> Result<(), &'static str> {
        let mut context = self.0.lock().map_err(|_| VR_TORRENT_PROVIDER_ERROR)?;
        context.inspection_generation = context.inspection_generation.wrapping_add(1);
        context.cached_torrent = None;
        Ok(())
    }

    fn begin_inspection(
        &self,
        requested_artifact: &TrustedArtifact,
    ) -> Result<(u64, u64), &'static str> {
        let mut context = self.0.lock().map_err(|_| VR_TORRENT_PROVIDER_ERROR)?;
        context.inspection_generation = context.inspection_generation.wrapping_add(1);
        context.cached_torrent = None;

        let release_feed = context
            .release_feed
            .as_ref()
            .filter(|feed| {
                feed.generation == context.release_generation
                    && feed.code == requested_artifact.code
            })
            .ok_or(VR_TORRENT_CONTEXT_INVALID)?;
        if !release_feed.artifacts.contains(requested_artifact) {
            return Err(VR_TORRENT_CONTEXT_INVALID);
        }

        Ok((context.release_generation, context.inspection_generation))
    }

    fn finish_inspection(
        &self,
        release_generation: u64,
        inspection_generation: u64,
        artifact: &TrustedArtifact,
        bytes: Vec<u8>,
        metadata: TorrentMetadata,
    ) -> Result<String, &'static str> {
        let mut context = self.0.lock().map_err(|_| VR_TORRENT_PROVIDER_ERROR)?;
        let feed_is_current = context.release_feed.as_ref().is_some_and(|feed| {
            feed.generation == release_generation && feed.artifacts.contains(artifact)
        });
        if context.release_generation != release_generation
            || context.inspection_generation != inspection_generation
            || !feed_is_current
        {
            return Err(VR_TORRENT_STALE);
        }

        let inspection_id = format!(
            "{}-{release_generation}-{inspection_generation}-{}",
            context.inspection_id_prefix, artifact.provider_item_id
        );
        context.cached_torrent = Some(CachedTorrent {
            artifact: artifact.clone(),
            generation: inspection_generation,
            inspection_id: inspection_id.clone(),
            default_file_name: format!("{}-{}.torrent", artifact.code, artifact.provider_item_id),
            bytes,
            metadata,
        });
        Ok(inspection_id)
    }

    fn verified_download_source(
        &self,
        inspection_id: &str,
        selected_file_ids: &[usize],
    ) -> Result<VerifiedDownloadSource, VerifiedDownloadSourceError> {
        let cached = {
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
                        torrent.artifact.clone(),
                        torrent.bytes.clone(),
                        torrent.metadata.clone(),
                    )
                })
                .ok_or(VerifiedDownloadSourceError::Context)?
        };

        let (artifact, bytes, cached_metadata) = cached;
        let metadata =
            parse_torrent_metadata(&bytes).map_err(|_| VerifiedDownloadSourceError::Metainfo)?;
        if metadata != cached_metadata || metadata.infohash != artifact.expected_infohash {
            return Err(VerifiedDownloadSourceError::Metainfo);
        }
        let selected_files = verified_selected_files(&metadata, selected_file_ids)?;

        Ok(VerifiedDownloadSource {
            bytes,
            code: artifact.code,
            infohash: metadata.infohash,
            release_name: artifact.release_name,
            movie_identity: None,
            tv_identity: None,
            selected_files,
        })
    }
}

impl VrTorrentState {
    pub fn begin_release_lookup(&self) -> Result<u64, &'static str> {
        self.0.begin_release_lookup()
    }

    pub fn finish_release_lookup(
        &self,
        generation: u64,
        code: &str,
        document: &str,
    ) -> Result<(), &'static str> {
        self.0.finish_release_lookup(generation, code, document)
    }

    pub fn invalidate_inspection(&self) -> Result<(), &'static str> {
        self.0.invalidate_inspection()
    }

    pub fn verified_download_source(
        &self,
        inspection_id: &str,
        selected_file_ids: &[usize],
    ) -> Result<VerifiedDownloadSource, VerifiedDownloadSourceError> {
        self.0
            .verified_download_source(inspection_id, selected_file_ids)
    }
}

impl AdultTorrentState {
    pub fn begin_release_lookup(&self) -> Result<u64, &'static str> {
        self.0
            .begin_release_lookup()
            .map_err(adult_torrent_error_code)
    }

    pub fn finish_release_lookup(
        &self,
        generation: u64,
        code: &str,
        document: &str,
    ) -> Result<(), &'static str> {
        self.0
            .finish_release_lookup(generation, code, document)
            .map_err(adult_torrent_error_code)
    }

    pub fn invalidate_inspection(&self) -> Result<(), &'static str> {
        self.0
            .invalidate_inspection()
            .map_err(adult_torrent_error_code)
    }

    pub fn verified_download_source(
        &self,
        inspection_id: &str,
        selected_file_ids: &[usize],
    ) -> Result<VerifiedDownloadSource, VerifiedDownloadSourceError> {
        self.0
            .verified_download_source(inspection_id, selected_file_ids)
    }
}

fn adult_torrent_error_code(error: &'static str) -> &'static str {
    match error {
        VR_TORRENT_CONTEXT_INVALID => ADULT_TORRENT_CONTEXT_INVALID,
        VR_TORRENT_INFOHASH_MISMATCH => ADULT_TORRENT_INFOHASH_MISMATCH,
        VR_TORRENT_MALFORMED => ADULT_TORRENT_MALFORMED,
        VR_TORRENT_NETWORK_ERROR => ADULT_TORRENT_NETWORK_ERROR,
        VR_TORRENT_PROVIDER_ERROR => ADULT_TORRENT_PROVIDER_ERROR,
        VR_TORRENT_SAVE_FAILED => ADULT_TORRENT_SAVE_FAILED,
        VR_TORRENT_SOURCE_UNAVAILABLE => ADULT_TORRENT_SOURCE_UNAVAILABLE,
        VR_TORRENT_STALE => ADULT_TORRENT_STALE,
        VR_TORRENT_UNSUPPORTED => ADULT_TORRENT_UNSUPPORTED,
        _ => ADULT_TORRENT_PROVIDER_ERROR,
    }
}

pub fn revalidate_persisted_download_source(
    bytes: &[u8],
    code: &str,
    release_name: &str,
    expected_infohash: &str,
    selected_file_ids: &[usize],
) -> Result<VerifiedDownloadSource, VerifiedDownloadSourceError> {
    if !crate::is_canonical_product_code(code)
        || release_name.trim().is_empty()
        || !release_matches_product_code(release_name, code)
        || expected_infohash.len() != SHA1_DIGEST_BYTES * 2
        || !expected_infohash
            .bytes()
            .all(|character| character.is_ascii_digit() || (b'a'..=b'f').contains(&character))
    {
        return Err(VerifiedDownloadSourceError::Context);
    }

    let metadata =
        parse_torrent_metadata(bytes).map_err(|_| VerifiedDownloadSourceError::Metainfo)?;
    if metadata.infohash != expected_infohash {
        return Err(VerifiedDownloadSourceError::Metainfo);
    }
    let selected_files = verified_selected_files(&metadata, selected_file_ids)?;

    Ok(VerifiedDownloadSource {
        bytes: bytes.to_vec(),
        code: code.to_owned(),
        infohash: metadata.infohash,
        release_name: release_name.to_owned(),
        movie_identity: None,
        tv_identity: None,
        selected_files,
    })
}

pub fn revalidate_persisted_movie_download_source(
    bytes: &[u8],
    identity: &MovieDownloadIdentity,
    expected_infohash: &str,
    selected_file_ids: &[usize],
) -> Result<VerifiedDownloadSource, VerifiedDownloadSourceError> {
    if !identity.is_valid()
        || expected_infohash != identity.expected_infohash
        || yts_artifact_infohash(&identity.torrent_url).as_deref()
            != Some(identity.expected_infohash.as_str())
    {
        return Err(VerifiedDownloadSourceError::Context);
    }

    let metadata =
        parse_torrent_metadata(bytes).map_err(|_| VerifiedDownloadSourceError::Metainfo)?;
    if metadata.infohash != expected_infohash {
        return Err(VerifiedDownloadSourceError::Metainfo);
    }
    let selected_files = verified_selected_files(&metadata, selected_file_ids)?;

    Ok(VerifiedDownloadSource {
        bytes: bytes.to_vec(),
        code: String::new(),
        infohash: metadata.infohash,
        release_name: identity.tmdb_title.clone(),
        movie_identity: Some(identity.clone()),
        tv_identity: None,
        selected_files,
    })
}

pub fn revalidate_persisted_tv_download_source(
    bytes: &[u8],
    identity: &TvDownloadIdentity,
    expected_infohash: &str,
    selected_file_ids: &[usize],
) -> Result<VerifiedDownloadSource, VerifiedDownloadSourceError> {
    if !identity.is_valid() || expected_infohash != identity.expected_infohash {
        return Err(VerifiedDownloadSourceError::Context);
    }
    let metadata =
        parse_torrent_metadata(bytes).map_err(|_| VerifiedDownloadSourceError::Metainfo)?;
    if metadata.infohash != expected_infohash {
        return Err(VerifiedDownloadSourceError::Metainfo);
    }
    let selected_files = verified_selected_files(&metadata, selected_file_ids)?;
    Ok(VerifiedDownloadSource {
        bytes: bytes.to_vec(),
        code: String::new(),
        infohash: metadata.infohash,
        release_name: identity.release_name.clone(),
        movie_identity: None,
        tv_identity: Some(identity.clone()),
        selected_files,
    })
}

fn verified_selected_files(
    metadata: &TorrentMetadata,
    selected_file_ids: &[usize],
) -> Result<Vec<VerifiedDownloadFile>, VerifiedDownloadSourceError> {
    let selected = selected_file_ids.iter().copied().collect::<BTreeSet<_>>();
    if selected.is_empty()
        || selected.len() != selected_file_ids.len()
        || selected
            .iter()
            .any(|file_id| *file_id >= metadata.files.len())
    {
        return Err(VerifiedDownloadSourceError::Selection);
    }

    Ok(metadata
        .files
        .iter()
        .enumerate()
        .filter(|(file_id, _)| selected.contains(file_id))
        .map(|(file_id, file)| VerifiedDownloadFile {
            file_id,
            path: file.path.clone(),
            size: file.size,
        })
        .collect())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerifiedDownloadSourceError {
    Context,
    Metainfo,
    Selection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedDownloadFile {
    pub file_id: usize,
    pub path: String,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedDownloadSource {
    pub bytes: Vec<u8>,
    pub code: String,
    pub infohash: String,
    pub release_name: String,
    pub movie_identity: Option<MovieDownloadIdentity>,
    pub tv_identity: Option<TvDownloadIdentity>,
    pub selected_files: Vec<VerifiedDownloadFile>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtifactRequestError {
    Network,
    TooLarge,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactResponse {
    pub status: u16,
    pub redirect_url: Option<String>,
    pub body: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TorrentInspectionError {
    SourceUnavailable,
    Network,
    Provider,
    Malformed,
    Unsupported,
}

impl TorrentInspectionError {
    fn code(self) -> &'static str {
        match self {
            Self::SourceUnavailable => VR_TORRENT_SOURCE_UNAVAILABLE,
            Self::Network => VR_TORRENT_NETWORK_ERROR,
            Self::Provider => VR_TORRENT_PROVIDER_ERROR,
            Self::Malformed => VR_TORRENT_MALFORMED,
            Self::Unsupported => VR_TORRENT_UNSUPPORTED,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TorrentFile {
    pub(crate) path: String,
    pub(crate) size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TorrentMetadata {
    pub(crate) display_name: String,
    pub(crate) infohash: String,
    pub(crate) total_size: u64,
    pub(crate) files: Vec<TorrentFile>,
}

pub fn inspect_sukebei_torrent_with(
    state: &VrTorrentState,
    request: TorrentInspectionRequest,
    fetch: impl FnMut(&str) -> Result<ArtifactResponse, ArtifactRequestError>,
) -> Result<Vec<String>, &'static str> {
    inspect_sukebei_torrent_state_with(&state.0, request, fetch)
}

fn inspect_sukebei_torrent_state_with(
    state: &TorrentState,
    request: TorrentInspectionRequest,
    fetch: impl FnMut(&str) -> Result<ArtifactResponse, ArtifactRequestError>,
) -> Result<Vec<String>, &'static str> {
    let artifact = TrustedArtifact::from(request);
    let (release_generation, inspection_generation) = state.begin_inspection(&artifact)?;
    let bytes = fetch_torrent_artifact(&artifact, fetch).map_err(TorrentInspectionError::code)?;
    let metadata = parse_torrent_metadata(&bytes).map_err(TorrentInspectionError::code)?;
    if metadata.infohash != artifact.expected_infohash {
        return Err(VR_TORRENT_INFOHASH_MISMATCH);
    }

    let inspection_id = state.finish_inspection(
        release_generation,
        inspection_generation,
        &artifact,
        bytes,
        metadata.clone(),
    )?;
    Ok(encode_torrent_inspection(inspection_id, metadata))
}

pub fn inspect_sukebei_adult_torrent_with(
    state: &AdultTorrentState,
    request: TorrentInspectionRequest,
    fetch: impl FnMut(&str) -> Result<ArtifactResponse, ArtifactRequestError>,
) -> Result<Vec<String>, &'static str> {
    inspect_sukebei_torrent_state_with(&state.0, request, fetch).map_err(adult_torrent_error_code)
}

pub fn inspect_yts_movie_torrent_with(
    state: &MovieTorrentState,
    release_generation: u64,
    inspection_generation: u64,
    request: MovieTorrentInspectionRequest,
    fetch: impl FnMut(&str) -> Result<ArtifactResponse, ArtifactRequestError>,
) -> Result<Vec<String>, &'static str> {
    state.validate_inspection(release_generation, inspection_generation, &request)?;
    let bytes = fetch_yts_torrent_artifact(&request, fetch).map_err(movie_torrent_error_code)?;
    let metadata = parse_torrent_metadata(&bytes).map_err(movie_torrent_error_code)?;
    if metadata.infohash != request.expected_infohash {
        return Err(MOVIE_TORRENT_INFOHASH_MISMATCH);
    }
    let inspection_id = state.finish_inspection(
        release_generation,
        inspection_generation,
        &request,
        bytes,
        metadata.clone(),
    )?;
    Ok(encode_torrent_inspection(inspection_id, metadata))
}

pub fn save_verified_torrent_with(
    state: &VrTorrentState,
    inspection_id: &str,
    choose_destination: impl FnOnce(&str) -> Option<PathBuf>,
    write: impl FnOnce(&Path, &[u8]) -> io::Result<()>,
) -> Result<bool, &'static str> {
    save_verified_torrent_state_with(&state.0, inspection_id, choose_destination, write)
}

fn save_verified_torrent_state_with(
    state: &TorrentState,
    inspection_id: &str,
    choose_destination: impl FnOnce(&str) -> Option<PathBuf>,
    write: impl FnOnce(&Path, &[u8]) -> io::Result<()>,
) -> Result<bool, &'static str> {
    let default_file_name = {
        let context = state.0.lock().map_err(|_| VR_TORRENT_SAVE_FAILED)?;
        context
            .cached_torrent
            .as_ref()
            .filter(|torrent| {
                torrent.inspection_id == inspection_id
                    && torrent.generation == context.inspection_generation
            })
            .map(|torrent| torrent.default_file_name.clone())
            .ok_or(VR_TORRENT_STALE)?
    };
    let Some(destination) = choose_destination(&default_file_name) else {
        return Ok(false);
    };

    let context = state.0.lock().map_err(|_| VR_TORRENT_SAVE_FAILED)?;
    let torrent = context
        .cached_torrent
        .as_ref()
        .filter(|torrent| {
            torrent.inspection_id == inspection_id
                && torrent.generation == context.inspection_generation
        })
        .ok_or(VR_TORRENT_STALE)?;
    write(&destination, &torrent.bytes).map_err(|_| VR_TORRENT_SAVE_FAILED)?;
    Ok(true)
}

pub fn save_verified_adult_torrent_with(
    state: &AdultTorrentState,
    inspection_id: &str,
    choose_destination: impl FnOnce(&str) -> Option<PathBuf>,
    write: impl FnOnce(&Path, &[u8]) -> io::Result<()>,
) -> Result<bool, &'static str> {
    save_verified_torrent_state_with(&state.0, inspection_id, choose_destination, write)
        .map_err(adult_torrent_error_code)
}

pub fn save_verified_movie_torrent_with(
    state: &MovieTorrentState,
    inspection_id: &str,
    choose_destination: impl FnOnce(&str) -> Option<PathBuf>,
    write: impl FnOnce(&Path, &[u8]) -> io::Result<()>,
) -> Result<bool, &'static str> {
    let default_file_name = {
        let context = state.0.lock().map_err(|_| MOVIE_TORRENT_SAVE_FAILED)?;
        context
            .cached_torrent
            .as_ref()
            .filter(|torrent| {
                torrent.inspection_id == inspection_id
                    && torrent.generation == context.inspection_generation
            })
            .map(|torrent| torrent.default_file_name.clone())
            .ok_or(MOVIE_TORRENT_STALE)?
    };
    let Some(destination) = choose_destination(&default_file_name) else {
        return Ok(false);
    };

    let context = state.0.lock().map_err(|_| MOVIE_TORRENT_SAVE_FAILED)?;
    let torrent = context
        .cached_torrent
        .as_ref()
        .filter(|torrent| {
            torrent.inspection_id == inspection_id
                && torrent.generation == context.inspection_generation
        })
        .ok_or(MOVIE_TORRENT_STALE)?;
    let reparsed = parse_torrent_metadata(&torrent.bytes).map_err(movie_torrent_error_code)?;
    if reparsed != torrent.metadata {
        return Err(MOVIE_TORRENT_MALFORMED);
    }
    write(&destination, &torrent.bytes).map_err(|_| MOVIE_TORRENT_SAVE_FAILED)?;
    Ok(true)
}

pub fn write_new_torrent_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)
}

fn fetch_torrent_artifact(
    artifact: &TrustedArtifact,
    mut fetch: impl FnMut(&str) -> Result<ArtifactResponse, ArtifactRequestError>,
) -> Result<Vec<u8>, TorrentInspectionError> {
    if artifact_item_id(&artifact.torrent_url).as_deref()
        != Some(artifact.provider_item_id.as_str())
    {
        return Err(TorrentInspectionError::Provider);
    }

    let mut current_url = artifact.torrent_url.clone();
    for redirect_count in 0..=TORRENT_MAX_REDIRECTS {
        let response = fetch(&current_url).map_err(|error| match error {
            ArtifactRequestError::Network => TorrentInspectionError::Network,
            ArtifactRequestError::TooLarge => TorrentInspectionError::Malformed,
        })?;
        if response.body.len() > TORRENT_MAX_BYTES {
            return Err(TorrentInspectionError::Malformed);
        }

        match response.status {
            200..=299 => {
                if response.body.is_empty() {
                    return Err(TorrentInspectionError::Malformed);
                }
                return Ok(response.body);
            }
            301 | 302 | 303 | 307 | 308 if redirect_count < TORRENT_MAX_REDIRECTS => {
                let redirect_url = response
                    .redirect_url
                    .filter(|url| {
                        artifact_item_id(url).as_deref() == Some(artifact.provider_item_id.as_str())
                    })
                    .ok_or(TorrentInspectionError::Provider)?;
                current_url = redirect_url;
            }
            404 | 410 | 451 => return Err(TorrentInspectionError::SourceUnavailable),
            _ => return Err(TorrentInspectionError::Provider),
        }
    }

    Err(TorrentInspectionError::Provider)
}

fn fetch_yts_torrent_artifact(
    request: &MovieTorrentInspectionRequest,
    mut fetch: impl FnMut(&str) -> Result<ArtifactResponse, ArtifactRequestError>,
) -> Result<Vec<u8>, TorrentInspectionError> {
    if yts_artifact_infohash(&request.torrent_url).as_deref()
        != Some(request.expected_infohash.as_str())
    {
        return Err(TorrentInspectionError::Provider);
    }

    let mut current_url = request.torrent_url.clone();
    for redirect_count in 0..=TORRENT_MAX_REDIRECTS {
        let response = fetch(&current_url).map_err(|error| match error {
            ArtifactRequestError::Network => TorrentInspectionError::Network,
            ArtifactRequestError::TooLarge => TorrentInspectionError::Malformed,
        })?;
        if response.body.len() > TORRENT_MAX_BYTES {
            return Err(TorrentInspectionError::Malformed);
        }

        match response.status {
            200..=299 => {
                if response.body.is_empty() {
                    return Err(TorrentInspectionError::Malformed);
                }
                return Ok(response.body);
            }
            301 | 302 | 303 | 307 | 308 if redirect_count < TORRENT_MAX_REDIRECTS => {
                let redirect_url = response
                    .redirect_url
                    .filter(|url| {
                        yts_artifact_infohash(url).as_deref()
                            == Some(request.expected_infohash.as_str())
                    })
                    .ok_or(TorrentInspectionError::Provider)?;
                current_url = redirect_url;
            }
            404 | 410 | 451 => return Err(TorrentInspectionError::SourceUnavailable),
            _ => return Err(TorrentInspectionError::Provider),
        }
    }

    Err(TorrentInspectionError::Provider)
}

pub(crate) fn encode_torrent_inspection(
    inspection_id: String,
    metadata: TorrentMetadata,
) -> Vec<String> {
    let mut response = vec![
        inspection_id,
        metadata.display_name,
        metadata.infohash,
        metadata.total_size.to_string(),
    ];
    for file in metadata.files {
        response.push(file.path);
        response.push(file.size.to_string());
    }
    response
}

fn movie_torrent_error_code(error: TorrentInspectionError) -> &'static str {
    match error {
        TorrentInspectionError::SourceUnavailable => MOVIE_TORRENT_SOURCE_UNAVAILABLE,
        TorrentInspectionError::Network => MOVIE_TORRENT_NETWORK_ERROR,
        TorrentInspectionError::Provider => MOVIE_TORRENT_PROVIDER_ERROR,
        TorrentInspectionError::Malformed => MOVIE_TORRENT_MALFORMED,
        TorrentInspectionError::Unsupported => MOVIE_TORRENT_UNSUPPORTED,
    }
}

pub fn fetch_artifact_response(url: &str) -> Result<ArtifactResponse, ArtifactRequestError> {
    fetch_artifact_response_platform(url)
}

#[cfg(target_os = "macos")]
fn fetch_artifact_response_platform(url: &str) -> Result<ArtifactResponse, ArtifactRequestError> {
    let output = Command::new("/usr/bin/curl")
        .args([
            "--silent",
            "--show-error",
            "--connect-timeout",
            "10",
            "--max-time",
            "20",
            "--max-filesize",
            &TORRENT_MAX_BYTES.to_string(),
            "--user-agent",
            "Auto-Video/0.1",
            "--header",
            "Accept: application/x-bittorrent",
            "--write-out",
            "\nAUTO_VIDEO_TORRENT_STATUS:%{http_code}\nAUTO_VIDEO_TORRENT_REDIRECT:%{redirect_url}",
            url,
        ])
        .output()
        .map_err(|_| ArtifactRequestError::Network)?;
    if !output.status.success() {
        return match output.status.code() {
            Some(63) => Err(ArtifactRequestError::TooLarge),
            _ => Err(ArtifactRequestError::Network),
        };
    }

    parse_artifact_command_output(&output.stdout)
}

#[cfg(target_os = "windows")]
fn fetch_artifact_response_platform(url: &str) -> Result<ArtifactResponse, ArtifactRequestError> {
    const ARTIFACT_URL_ENV: &str = "AUTO_VIDEO_TORRENT_URL";
    const WINDOWS_ARTIFACT_SCRIPT: &str = r#"$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
Add-Type -AssemblyName System.Net.Http
$handler = [System.Net.Http.HttpClientHandler]::new()
$handler.AllowAutoRedirect = $false
$client = [System.Net.Http.HttpClient]::new($handler)
$client.Timeout = [TimeSpan]::FromSeconds(20)
$client.DefaultRequestHeaders.UserAgent.ParseAdd('Auto-Video/0.1')
$client.DefaultRequestHeaders.Accept.ParseAdd('application/x-bittorrent')
try {
  $response = $client.GetAsync($env:AUTO_VIDEO_TORRENT_URL, [System.Net.Http.HttpCompletionOption]::ResponseHeadersRead).GetAwaiter().GetResult()
  $stream = $response.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
  $memory = [System.IO.MemoryStream]::new()
  $buffer = [byte[]]::new(65536)
  while (($read = $stream.Read($buffer, 0, $buffer.Length)) -gt 0) {
    if ($memory.Length + $read -gt 2097152) { [Environment]::Exit(63) }
    $memory.Write($buffer, 0, $read)
  }
  $redirect = ''
  if ($null -ne $response.Headers.Location) {
    $redirect = [Uri]::new([Uri]$env:AUTO_VIDEO_TORRENT_URL, $response.Headers.Location).AbsoluteUri
  }
  $output = [Console]::OpenStandardOutput()
  $body = $memory.ToArray()
  $output.Write($body, 0, $body.Length)
  $marker = [Text.Encoding]::UTF8.GetBytes("`nAUTO_VIDEO_TORRENT_STATUS:" + [int]$response.StatusCode + "`nAUTO_VIDEO_TORRENT_REDIRECT:" + $redirect)
  $output.Write($marker, 0, $marker.Length)
} catch {
  [Environment]::Exit(28)
} finally {
  $client.Dispose()
  $handler.Dispose()
}"#;
    let output = Command::new("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
        .arg(WINDOWS_ARTIFACT_SCRIPT)
        // The URL is native-validated and stays out of PowerShell source.
        .env(ARTIFACT_URL_ENV, url)
        .output()
        .map_err(|_| ArtifactRequestError::Network)?;
    if !output.status.success() {
        return match output.status.code() {
            Some(63) => Err(ArtifactRequestError::TooLarge),
            _ => Err(ArtifactRequestError::Network),
        };
    }

    parse_artifact_command_output(&output.stdout)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn fetch_artifact_response_platform(_url: &str) -> Result<ArtifactResponse, ArtifactRequestError> {
    Err(ArtifactRequestError::Network)
}

fn parse_artifact_command_output(output: &[u8]) -> Result<ArtifactResponse, ArtifactRequestError> {
    let status_marker = output
        .windows(TORRENT_STATUS_MARKER.len())
        .rposition(|window| window == TORRENT_STATUS_MARKER)
        .ok_or(ArtifactRequestError::Network)?;
    let redirect_marker = output[status_marker + TORRENT_STATUS_MARKER.len()..]
        .windows(TORRENT_REDIRECT_MARKER.len())
        .position(|window| window == TORRENT_REDIRECT_MARKER)
        .map(|position| position + status_marker + TORRENT_STATUS_MARKER.len())
        .ok_or(ArtifactRequestError::Network)?;
    let status =
        std::str::from_utf8(&output[status_marker + TORRENT_STATUS_MARKER.len()..redirect_marker])
            .ok()
            .and_then(|value| value.parse::<u16>().ok())
            .ok_or(ArtifactRequestError::Network)?;
    let redirect = std::str::from_utf8(&output[redirect_marker + TORRENT_REDIRECT_MARKER.len()..])
        .map_err(|_| ArtifactRequestError::Network)?;
    if status_marker > TORRENT_MAX_BYTES {
        return Err(ArtifactRequestError::TooLarge);
    }

    Ok(ArtifactResponse {
        status,
        redirect_url: (!redirect.is_empty()).then(|| redirect.to_owned()),
        body: output[..status_marker].to_vec(),
    })
}

fn trusted_artifacts_from_feed(document: &str, requested_code: &str) -> Vec<TrustedArtifact> {
    rss_items(document)
        .filter_map(|item| {
            let release_name = xml_element_text(item, "title")?;
            if release_name.trim().is_empty()
                || !release_matches_product_code(&release_name, requested_code)
            {
                return None;
            }

            let provider_item_id = view_item_id(xml_element_text(item, "guid")?.trim())?;
            let torrent_url = xml_element_text(item, "link")?.trim().to_owned();
            if artifact_item_id(&torrent_url).as_deref() != Some(provider_item_id.as_str()) {
                return None;
            }
            let expected_infohash = canonical_infohash(xml_element_text(item, "infoHash")?.trim())?;

            Some(TrustedArtifact {
                code: requested_code.to_owned(),
                release_name,
                provider_item_id,
                torrent_url,
                expected_infohash,
            })
        })
        .collect()
}

fn rss_items(document: &str) -> impl Iterator<Item = &str> {
    let mut remaining = document;
    std::iter::from_fn(move || loop {
        let start = remaining.find("<item")?;
        let after_name = remaining.as_bytes().get(start + 5).copied()?;
        if !after_name.is_ascii_whitespace() && after_name != b'>' {
            remaining = &remaining[start + 5..];
            continue;
        }
        let opening_end = remaining[start..].find('>')? + start + 1;
        let end = remaining[opening_end..].find("</item>")? + opening_end;
        let item = &remaining[opening_end..end];
        remaining = &remaining[end + "</item>".len()..];
        return Some(item);
    })
}

fn xml_element_text(item: &str, local_name: &str) -> Option<String> {
    let mut remaining = item;
    while let Some(start) = remaining.find('<') {
        remaining = &remaining[start + 1..];
        if remaining.starts_with(['/', '!', '?']) {
            continue;
        }
        let opening_end = remaining.find('>')?;
        let opening = &remaining[..opening_end];
        let tag_name = opening
            .split(|character: char| character.is_ascii_whitespace() || character == '/')
            .next()?;
        if tag_name.rsplit(':').next()? != local_name {
            remaining = &remaining[opening_end + 1..];
            continue;
        }
        if opening.trim_end().ends_with('/') {
            return None;
        }

        let content = &remaining[opening_end + 1..];
        let closing = format!("</{tag_name}>");
        let closing_start = content.find(&closing)?;
        let raw_text = &content[..closing_start];
        if raw_text.contains('<') {
            return None;
        }
        return decode_xml_text(raw_text);
    }
    None
}

fn decode_xml_text(value: &str) -> Option<String> {
    let mut decoded = String::with_capacity(value.len());
    let mut remaining = value;
    while let Some(entity_start) = remaining.find('&') {
        decoded.push_str(&remaining[..entity_start]);
        let entity = &remaining[entity_start + 1..];
        let entity_end = entity.find(';')?;
        let replacement = match &entity[..entity_end] {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            numeric if numeric.starts_with("#x") => {
                char::from_u32(u32::from_str_radix(&numeric[2..], 16).ok()?)?
            }
            numeric if numeric.starts_with('#') => {
                char::from_u32(numeric[1..].parse::<u32>().ok()?)?
            }
            _ => return None,
        };
        decoded.push(replacement);
        remaining = &entity[entity_end + 1..];
    }
    decoded.push_str(remaining);
    Some(decoded.replace("\r\n", "\n").replace('\r', "\n"))
}

fn product_code_candidates(name: &str) -> Vec<(String, String)> {
    let bytes = name.as_bytes();
    let mut candidates = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if index > 0 && bytes[index - 1].is_ascii_alphanumeric() {
            index += 1;
            continue;
        }

        let prefix_start = index;
        while index < bytes.len() && bytes[index].is_ascii_alphabetic() {
            index += 1;
        }
        let prefix_length = index - prefix_start;
        if !(2..=16).contains(&prefix_length) {
            index = prefix_start + 1;
            continue;
        }
        while index < bytes.len() && matches!(bytes[index], b' ' | b'_' | b'-') {
            index += 1;
        }
        let number_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        let number_length = index - number_start;
        if !(1..=10).contains(&number_length)
            || (index < bytes.len() && bytes[index].is_ascii_alphanumeric())
        {
            index = prefix_start + 1;
            continue;
        }

        let number = std::str::from_utf8(&bytes[number_start..index])
            .ok()
            .and_then(|value| value.parse::<u64>().ok());
        if let Some(number) = number.filter(|number| *number > 0) {
            let prefix = std::str::from_utf8(&bytes[prefix_start..prefix_start + prefix_length])
                .expect("ASCII product-code prefixes are valid UTF-8");
            let prefix = prefix.to_ascii_uppercase();
            candidates.push((format!("{prefix}-{number}"), prefix));
        }
    }

    candidates
}

fn release_matches_product_code(name: &str, requested_code: &str) -> bool {
    let identities = product_code_candidates(name)
        .into_iter()
        .map(|(code, _)| code)
        .collect::<BTreeSet<_>>();

    identities.len() == 1 && identities.contains(requested_code)
}

fn media_name_matches_product_code_with_labels(
    name: &str,
    requested_code: &str,
    ignored_labels: &[&str],
) -> bool {
    let identities = product_code_candidates(name)
        .into_iter()
        .filter(|(_, prefix)| !ignored_labels.contains(&prefix.as_str()))
        .map(|(code, _)| code)
        .collect::<BTreeSet<_>>();
    if !identities.is_empty() {
        return identities.len() == 1 && identities.contains(requested_code);
    }

    let Some((requested_prefix, requested_number)) = requested_code.split_once('-') else {
        return false;
    };
    let bytes = name.as_bytes();
    let prefix = requested_prefix.as_bytes();
    let number = requested_number.as_bytes();
    !(0..bytes.len()).any(|start| {
        let prefix_end = start + prefix.len();
        if prefix_end > bytes.len() || !bytes[start..prefix_end].eq_ignore_ascii_case(prefix) {
            return false;
        }
        let mut number_start = prefix_end;
        while number_start < bytes.len() && matches!(bytes[number_start], b' ' | b'_' | b'-') {
            number_start += 1;
        }
        let number_end = number_start + number.len();
        number_end <= bytes.len() && bytes[number_start..number_end].eq(number)
    })
}

pub(crate) fn media_name_matches_product_code(name: &str, requested_code: &str) -> bool {
    media_name_matches_product_code_with_labels(
        name,
        requested_code,
        &["PART", "PT", "CD", "DISC", "DISK"],
    )
}

pub(crate) fn adult_media_name_matches_product_code(name: &str, requested_code: &str) -> bool {
    media_name_matches_product_code_with_labels(
        name,
        requested_code,
        &["PART", "CD", "DISC", "DISK"],
    )
}

fn view_item_id(url: &str) -> Option<String> {
    provider_item_id(url.strip_prefix(SUKEBEI_VIEW_PREFIX)?)
}

fn artifact_item_id(url: &str) -> Option<String> {
    let item_id = url
        .strip_prefix(SUKEBEI_DOWNLOAD_PREFIX)?
        .strip_suffix(".torrent")?;
    provider_item_id(item_id)
}

fn provider_item_id(value: &str) -> Option<String> {
    if value.is_empty()
        || value.starts_with('0')
        || !value.bytes().all(|character| character.is_ascii_digit())
        || value.len() > PROVIDER_ITEM_ID_MAX_DIGITS
    {
        return None;
    }
    Some(value.to_owned())
}

pub(crate) fn canonical_infohash(value: &str) -> Option<String> {
    (value.len() == 40 && value.bytes().all(|character| character.is_ascii_hexdigit()))
        .then(|| value.to_ascii_lowercase())
}

fn yts_artifact_infohash(url: &str) -> Option<String> {
    canonical_infohash(url.strip_prefix(YTS_DOWNLOAD_PREFIX)?)
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum JsonValue {
    Null,
    Boolean,
    Number(String),
    String(String),
    Array(Vec<JsonValue>),
    Object(BTreeMap<String, JsonValue>),
}

pub(crate) struct JsonParser<'a> {
    input: &'a [u8],
    position: usize,
    value_count: usize,
}

impl<'a> JsonParser<'a> {
    pub(crate) fn new(input: &'a str) -> Self {
        Self {
            input: input.as_bytes(),
            position: 0,
            value_count: 0,
        }
    }

    pub(crate) fn parse(mut self) -> Option<JsonValue> {
        let value = self.parse_value(0)?;
        self.skip_whitespace();
        (self.position == self.input.len()).then_some(value)
    }

    fn parse_value(&mut self, depth: usize) -> Option<JsonValue> {
        if depth > 64 || self.value_count >= 100_000 {
            return None;
        }
        self.value_count += 1;
        self.skip_whitespace();
        match self.input.get(self.position).copied()? {
            b'n' => self.parse_literal(b"null", JsonValue::Null),
            b't' => self.parse_literal(b"true", JsonValue::Boolean),
            b'f' => self.parse_literal(b"false", JsonValue::Boolean),
            b'"' => self.parse_string().map(JsonValue::String),
            b'[' => self.parse_array(depth + 1),
            b'{' => self.parse_object(depth + 1),
            b'-' | b'0'..=b'9' => self.parse_number().map(JsonValue::Number),
            _ => None,
        }
    }

    fn parse_literal(&mut self, literal: &[u8], value: JsonValue) -> Option<JsonValue> {
        if !self.input[self.position..].starts_with(literal) {
            return None;
        }
        self.position += literal.len();
        Some(value)
    }

    fn parse_string(&mut self) -> Option<String> {
        self.position += 1;
        let mut decoded = String::new();
        let mut plain_start = self.position;
        loop {
            let character = *self.input.get(self.position)?;
            match character {
                b'"' => {
                    decoded.push_str(
                        std::str::from_utf8(&self.input[plain_start..self.position]).ok()?,
                    );
                    self.position += 1;
                    return Some(decoded);
                }
                b'\\' => {
                    decoded.push_str(
                        std::str::from_utf8(&self.input[plain_start..self.position]).ok()?,
                    );
                    self.position += 1;
                    let escaped = *self.input.get(self.position)?;
                    self.position += 1;
                    match escaped {
                        b'"' => decoded.push('"'),
                        b'\\' => decoded.push('\\'),
                        b'/' => decoded.push('/'),
                        b'b' => decoded.push('\u{0008}'),
                        b'f' => decoded.push('\u{000c}'),
                        b'n' => decoded.push('\n'),
                        b'r' => decoded.push('\r'),
                        b't' => decoded.push('\t'),
                        b'u' => decoded.push(self.parse_unicode_escape()?),
                        _ => return None,
                    }
                    plain_start = self.position;
                }
                0x00..=0x1f => return None,
                _ => self.position += 1,
            }
        }
    }

    fn parse_unicode_escape(&mut self) -> Option<char> {
        let first = self.parse_hex_quad()?;
        let scalar = if (0xd800..=0xdbff).contains(&first) {
            if self.input.get(self.position..self.position + 2)? != b"\\u" {
                return None;
            }
            self.position += 2;
            let second = self.parse_hex_quad()?;
            if !(0xdc00..=0xdfff).contains(&second) {
                return None;
            }
            0x10000 + ((u32::from(first) - 0xd800) << 10) + (u32::from(second) - 0xdc00)
        } else if (0xdc00..=0xdfff).contains(&first) {
            return None;
        } else {
            u32::from(first)
        };
        char::from_u32(scalar)
    }

    fn parse_hex_quad(&mut self) -> Option<u16> {
        let encoded = self.input.get(self.position..self.position + 4)?;
        if !encoded
            .iter()
            .all(|character| character.is_ascii_hexdigit())
        {
            return None;
        }
        self.position += 4;
        u16::from_str_radix(std::str::from_utf8(encoded).ok()?, 16).ok()
    }

    fn parse_number(&mut self) -> Option<String> {
        let start = self.position;
        if self.input.get(self.position) == Some(&b'-') {
            self.position += 1;
        }
        match self.input.get(self.position).copied()? {
            b'0' => self.position += 1,
            b'1'..=b'9' => {
                self.position += 1;
                while self
                    .input
                    .get(self.position)
                    .is_some_and(u8::is_ascii_digit)
                {
                    self.position += 1;
                }
            }
            _ => return None,
        }
        if self.input.get(self.position) == Some(&b'.') {
            self.position += 1;
            let fraction_start = self.position;
            while self
                .input
                .get(self.position)
                .is_some_and(u8::is_ascii_digit)
            {
                self.position += 1;
            }
            if self.position == fraction_start {
                return None;
            }
        }
        if self
            .input
            .get(self.position)
            .is_some_and(|character| matches!(character, b'e' | b'E'))
        {
            self.position += 1;
            if self
                .input
                .get(self.position)
                .is_some_and(|character| matches!(character, b'+' | b'-'))
            {
                self.position += 1;
            }
            let exponent_start = self.position;
            while self
                .input
                .get(self.position)
                .is_some_and(u8::is_ascii_digit)
            {
                self.position += 1;
            }
            if self.position == exponent_start {
                return None;
            }
        }
        Some(
            std::str::from_utf8(&self.input[start..self.position])
                .ok()?
                .to_owned(),
        )
    }

    fn parse_array(&mut self, depth: usize) -> Option<JsonValue> {
        self.position += 1;
        self.skip_whitespace();
        let mut values = Vec::new();
        if self.input.get(self.position) == Some(&b']') {
            self.position += 1;
            return Some(JsonValue::Array(values));
        }
        loop {
            values.push(self.parse_value(depth)?);
            self.skip_whitespace();
            match self.input.get(self.position).copied()? {
                b',' => self.position += 1,
                b']' => {
                    self.position += 1;
                    return Some(JsonValue::Array(values));
                }
                _ => return None,
            }
        }
    }

    fn parse_object(&mut self, depth: usize) -> Option<JsonValue> {
        self.position += 1;
        self.skip_whitespace();
        let mut entries = BTreeMap::new();
        if self.input.get(self.position) == Some(&b'}') {
            self.position += 1;
            return Some(JsonValue::Object(entries));
        }
        loop {
            self.skip_whitespace();
            let key = self.parse_string()?;
            self.skip_whitespace();
            if self.input.get(self.position) != Some(&b':') {
                return None;
            }
            self.position += 1;
            let value = self.parse_value(depth)?;
            if entries.insert(key, value).is_some() {
                return None;
            }
            self.skip_whitespace();
            match self.input.get(self.position).copied()? {
                b',' => self.position += 1,
                b'}' => {
                    self.position += 1;
                    return Some(JsonValue::Object(entries));
                }
                _ => return None,
            }
        }
    }

    fn skip_whitespace(&mut self) {
        while self
            .input
            .get(self.position)
            .is_some_and(|character| matches!(character, b' ' | b'\n' | b'\r' | b'\t'))
        {
            self.position += 1;
        }
    }
}

pub(crate) fn json_object(value: &JsonValue) -> Option<&BTreeMap<String, JsonValue>> {
    match value {
        JsonValue::Object(entries) => Some(entries),
        _ => None,
    }
}

pub(crate) fn json_array(value: &JsonValue) -> Option<&[JsonValue]> {
    match value {
        JsonValue::Array(values) => Some(values),
        _ => None,
    }
}

pub(crate) fn json_string<'a>(
    object: &'a BTreeMap<String, JsonValue>,
    key: &str,
) -> Option<&'a str> {
    match object.get(key) {
        Some(JsonValue::String(value)) => Some(value),
        _ => None,
    }
}

fn json_optional_text(object: &BTreeMap<String, JsonValue>, key: &str) -> Option<String> {
    json_string(object, key)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
}

pub(crate) fn json_u64(object: &BTreeMap<String, JsonValue>, key: &str) -> Option<u64> {
    match object.get(key) {
        Some(JsonValue::Number(value)) => value.parse().ok(),
        _ => None,
    }
}

fn valid_release_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, character)| matches!(index, 4 | 7) || character.is_ascii_digit())
}

pub(crate) fn canonical_imdb_id(value: &str) -> Option<String> {
    let canonical = value.to_ascii_lowercase();
    let digits = canonical.strip_prefix("tt")?;
    ((7..=10).contains(&digits.len()) && digits.bytes().all(|character| character.is_ascii_digit()))
        .then_some(canonical)
}

pub fn verified_movie_imdb_id(
    requested_tmdb_movie_id: u64,
    tmdb_details_document: &str,
    tmdb_external_ids_document: &str,
) -> Result<String, &'static str> {
    trusted_tmdb_movie_context(
        requested_tmdb_movie_id,
        tmdb_details_document,
        tmdb_external_ids_document,
    )
    .map(|context| context.imdb_id)
}

fn trusted_tmdb_movie_context(
    requested_tmdb_movie_id: u64,
    tmdb_details_document: &str,
    tmdb_external_ids_document: &str,
) -> Result<TrustedMovieContext, &'static str> {
    let details = JsonParser::new(tmdb_details_document)
        .parse()
        .and_then(|value| json_object(&value).cloned())
        .ok_or(MOVIE_TMDB_MALFORMED)?;
    if json_u64(&details, "id") != Some(requested_tmdb_movie_id) {
        return Err(MOVIE_TMDB_MALFORMED);
    }
    let tmdb_title = json_string(&details, "title")
        .filter(|title| !title.trim().is_empty())
        .map(str::to_owned)
        .ok_or(MOVIE_TMDB_MALFORMED)?;
    let release_date = match details.get("release_date") {
        None | Some(JsonValue::Null) => None,
        Some(JsonValue::String(value)) if value.is_empty() => None,
        Some(JsonValue::String(value)) if valid_release_date(value) => Some(value.clone()),
        _ => return Err(MOVIE_TMDB_MALFORMED),
    };

    let external_ids = JsonParser::new(tmdb_external_ids_document)
        .parse()
        .and_then(|value| json_object(&value).cloned())
        .ok_or(MOVIE_TMDB_MALFORMED)?;
    if json_u64(&external_ids, "id") != Some(requested_tmdb_movie_id) {
        return Err(MOVIE_TMDB_MALFORMED);
    }
    let imdb_id = match external_ids.get("imdb_id") {
        None | Some(JsonValue::Null) => return Err(MOVIE_NO_IMDB_IDENTITY),
        Some(JsonValue::String(value)) if value.is_empty() => {
            return Err(MOVIE_NO_IMDB_IDENTITY);
        }
        Some(JsonValue::String(value)) => canonical_imdb_id(value).ok_or(MOVIE_TMDB_MALFORMED)?,
        _ => return Err(MOVIE_TMDB_MALFORMED),
    };

    Ok(TrustedMovieContext {
        tmdb_movie_id: requested_tmdb_movie_id,
        tmdb_title,
        release_date,
        imdb_id,
        provider_movie_id: 0,
        provider_title: None,
        provider_year: None,
    })
}

fn trusted_movie_release_set(
    generation: u64,
    requested_tmdb_movie_id: u64,
    tmdb_details_document: &str,
    tmdb_external_ids_document: &str,
    yts_document: &str,
) -> Result<TrustedMovieReleaseSet, &'static str> {
    let mut context = trusted_tmdb_movie_context(
        requested_tmdb_movie_id,
        tmdb_details_document,
        tmdb_external_ids_document,
    )?;
    let imdb_id = context.imdb_id.clone();

    let yts = JsonParser::new(yts_document)
        .parse()
        .and_then(|value| json_object(&value).cloned())
        .ok_or(MOVIE_YTS_MALFORMED)?;
    if json_string(&yts, "status") != Some("ok") {
        return Err(MOVIE_YTS_MALFORMED);
    }
    let data = yts
        .get("data")
        .and_then(json_object)
        .ok_or(MOVIE_YTS_MALFORMED)?;
    let movies = match data.get("movies") {
        None | Some(JsonValue::Null) => &[][..],
        Some(value) => json_array(value).ok_or(MOVIE_YTS_MALFORMED)?,
    };
    let mut accepted_movies = Vec::new();
    for movie in movies {
        let Some(movie) = json_object(movie) else {
            continue;
        };
        let Some(candidate_imdb_id) = json_string(movie, "imdb_code").and_then(canonical_imdb_id)
        else {
            continue;
        };
        if candidate_imdb_id != imdb_id {
            continue;
        }
        let provider_movie_id = json_u64(movie, "id")
            .filter(|id| *id > 0)
            .ok_or(MOVIE_YTS_MALFORMED)?;
        let provider_title = json_optional_text(movie, "title");
        let provider_year = json_u64(movie, "year")
            .filter(|year| *year > 0)
            .map(|year| year.to_string());
        let torrents = match movie.get("torrents") {
            None | Some(JsonValue::Null) => Vec::new(),
            Some(value) => json_array(value)
                .ok_or(MOVIE_YTS_MALFORMED)?
                .iter()
                .enumerate()
                .filter_map(|(index, torrent)| parse_yts_torrent(torrent, provider_movie_id, index))
                .collect(),
        };
        accepted_movies.push((provider_movie_id, provider_title, provider_year, torrents));
    }

    let Some((provider_movie_id, provider_title, provider_year, torrents)) =
        accepted_movies.first().cloned()
    else {
        return Ok(TrustedMovieReleaseSet {
            generation,
            context,
            torrents: Vec::new(),
        });
    };
    if accepted_movies
        .iter()
        .skip(1)
        .any(|candidate| candidate != accepted_movies.first().expect("accepted movie exists"))
    {
        return Err(MOVIE_YTS_CONFLICTING_PROVIDER);
    }

    context.provider_movie_id = provider_movie_id;
    context.provider_title = provider_title;
    context.provider_year = provider_year;
    Ok(TrustedMovieReleaseSet {
        generation,
        context,
        torrents,
    })
}

fn parse_yts_torrent(
    value: &JsonValue,
    provider_movie_id: u64,
    index: usize,
) -> Option<TrustedMovieTorrent> {
    let torrent = json_object(value)?;
    let expected_infohash = json_string(torrent, "hash").and_then(canonical_infohash);
    let torrent_url = json_string(torrent, "url")
        .filter(|url| {
            expected_infohash
                .as_deref()
                .is_some_and(|expected| yts_artifact_infohash(url).as_deref() == Some(expected))
        })
        .map(str::to_owned);
    let trusted_infohash = torrent_url.as_ref().and(expected_infohash);
    let parsed = TrustedMovieTorrent {
        row_id: format!("{provider_movie_id}:{index}"),
        quality: json_optional_text(torrent, "quality"),
        type_label: json_optional_text(torrent, "type"),
        video_codec: json_optional_text(torrent, "video_codec"),
        size: json_optional_text(torrent, "size"),
        size_bytes: json_u64(torrent, "size_bytes").map(|value| value.to_string()),
        seeds: json_u64(torrent, "seeds").map(|value| value.to_string()),
        peers: json_u64(torrent, "peers").map(|value| value.to_string()),
        expected_infohash: trusted_infohash,
        torrent_url,
    };
    (parsed.quality.is_some()
        || parsed.type_label.is_some()
        || parsed.video_codec.is_some()
        || parsed.size.is_some()
        || parsed.size_bytes.is_some()
        || parsed.seeds.is_some()
        || parsed.peers.is_some()
        || parsed.expected_infohash.is_some())
    .then_some(parsed)
}

fn encode_movie_release_set(release_set: &TrustedMovieReleaseSet) -> Vec<String> {
    let context = &release_set.context;
    let mut response = vec![
        context.tmdb_movie_id.to_string(),
        context.tmdb_title.clone(),
        context.release_date.clone().unwrap_or_default(),
        context.imdb_id.clone(),
        context.provider_movie_id.to_string(),
        context.provider_title.clone().unwrap_or_default(),
        context.provider_year.clone().unwrap_or_default(),
        release_set.torrents.len().to_string(),
    ];
    for torrent in &release_set.torrents {
        response.extend([
            torrent.row_id.clone(),
            torrent.quality.clone().unwrap_or_default(),
            torrent.type_label.clone().unwrap_or_default(),
            torrent.video_codec.clone().unwrap_or_default(),
            torrent.size.clone().unwrap_or_default(),
            torrent.size_bytes.clone().unwrap_or_default(),
            torrent.seeds.clone().unwrap_or_default(),
            torrent.peers.clone().unwrap_or_default(),
            torrent.expected_infohash.clone().unwrap_or_default(),
            torrent.torrent_url.clone().unwrap_or_default(),
        ]);
    }
    response
}

#[derive(Debug)]
struct BencodedNode<'a> {
    start: usize,
    end: usize,
    value: BencodedValue<'a>,
}

#[derive(Debug)]
enum BencodedValue<'a> {
    Bytes(&'a [u8]),
    Integer(i64),
    List(Vec<BencodedNode<'a>>),
    Dictionary(Vec<(&'a [u8], BencodedNode<'a>)>),
}

struct BencodeParser<'a> {
    input: &'a [u8],
    position: usize,
    value_count: usize,
}

impl<'a> BencodeParser<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self {
            input,
            position: 0,
            value_count: 0,
        }
    }

    fn parse(mut self) -> Result<BencodedNode<'a>, TorrentInspectionError> {
        let value = self.parse_value(0)?;
        if self.position != self.input.len() {
            return Err(TorrentInspectionError::Malformed);
        }
        Ok(value)
    }

    fn parse_value(&mut self, depth: usize) -> Result<BencodedNode<'a>, TorrentInspectionError> {
        if depth > 64 || self.value_count >= 100_000 {
            return Err(TorrentInspectionError::Malformed);
        }
        self.value_count += 1;
        let start = self.position;
        let value = match self.input.get(self.position).copied() {
            Some(b'i') => self.parse_integer()?,
            Some(b'l') => self.parse_list(depth + 1)?,
            Some(b'd') => self.parse_dictionary(depth + 1)?,
            Some(character) if character.is_ascii_digit() => {
                BencodedValue::Bytes(self.parse_bytes()?)
            }
            _ => return Err(TorrentInspectionError::Malformed),
        };
        Ok(BencodedNode {
            start,
            end: self.position,
            value,
        })
    }

    fn parse_integer(&mut self) -> Result<BencodedValue<'a>, TorrentInspectionError> {
        self.position += 1;
        let end = self.input[self.position..]
            .iter()
            .position(|character| *character == b'e')
            .map(|offset| offset + self.position)
            .ok_or(TorrentInspectionError::Malformed)?;
        let encoded = &self.input[self.position..end];
        let digits = encoded.strip_prefix(b"-").unwrap_or(encoded);
        if digits.is_empty()
            || !digits.iter().all(|character| character.is_ascii_digit())
            || (digits.starts_with(b"0") && (digits.len() > 1 || encoded.starts_with(b"-")))
        {
            return Err(TorrentInspectionError::Malformed);
        }
        let value = std::str::from_utf8(encoded)
            .ok()
            .and_then(|value| value.parse::<i64>().ok())
            .ok_or(TorrentInspectionError::Malformed)?;
        self.position = end + 1;
        Ok(BencodedValue::Integer(value))
    }

    fn parse_bytes(&mut self) -> Result<&'a [u8], TorrentInspectionError> {
        let separator = self.input[self.position..]
            .iter()
            .position(|character| *character == b':')
            .map(|offset| offset + self.position)
            .ok_or(TorrentInspectionError::Malformed)?;
        let encoded_length = &self.input[self.position..separator];
        if encoded_length.is_empty()
            || (encoded_length.starts_with(b"0") && encoded_length.len() > 1)
            || !encoded_length
                .iter()
                .all(|character| character.is_ascii_digit())
        {
            return Err(TorrentInspectionError::Malformed);
        }
        let length = std::str::from_utf8(encoded_length)
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or(TorrentInspectionError::Malformed)?;
        let start = separator + 1;
        let end = start
            .checked_add(length)
            .filter(|end| *end <= self.input.len())
            .ok_or(TorrentInspectionError::Malformed)?;
        self.position = end;
        Ok(&self.input[start..end])
    }

    fn parse_list(&mut self, depth: usize) -> Result<BencodedValue<'a>, TorrentInspectionError> {
        self.position += 1;
        let mut values = Vec::new();
        while self.input.get(self.position) != Some(&b'e') {
            values.push(self.parse_value(depth)?);
        }
        self.position += 1;
        Ok(BencodedValue::List(values))
    }

    fn parse_dictionary(
        &mut self,
        depth: usize,
    ) -> Result<BencodedValue<'a>, TorrentInspectionError> {
        self.position += 1;
        let mut entries = Vec::new();
        let mut previous_key: Option<&[u8]> = None;
        while self.input.get(self.position) != Some(&b'e') {
            let key = self.parse_bytes()?;
            if previous_key.is_some_and(|previous| previous >= key) {
                return Err(TorrentInspectionError::Malformed);
            }
            previous_key = Some(key);
            entries.push((key, self.parse_value(depth)?));
        }
        self.position += 1;
        Ok(BencodedValue::Dictionary(entries))
    }
}

pub(crate) fn parse_torrent_metadata(
    bytes: &[u8],
) -> Result<TorrentMetadata, TorrentInspectionError> {
    if bytes.is_empty() || bytes.len() > TORRENT_MAX_BYTES {
        return Err(TorrentInspectionError::Malformed);
    }
    let root = BencodeParser::new(bytes).parse()?;
    let root_dictionary = dictionary(&root)?;
    let info =
        dictionary_value(root_dictionary, b"info").ok_or(TorrentInspectionError::Unsupported)?;
    let info_dictionary = dictionary(info)?;
    if dictionary_value(info_dictionary, b"meta version").is_some() {
        return Err(TorrentInspectionError::Unsupported);
    }
    let piece_length = integer_value(info_dictionary, b"piece length")?;
    let pieces = bytes_value(info_dictionary, b"pieces")?;
    if piece_length <= 0 || pieces.is_empty() || pieces.len() % SHA1_DIGEST_BYTES != 0 {
        return Err(TorrentInspectionError::Unsupported);
    }
    let piece_length =
        u64::try_from(piece_length).map_err(|_| TorrentInspectionError::Unsupported)?;

    let display_name = match dictionary_value(info_dictionary, b"name.utf-8") {
        Some(name) => node_text(name)?,
        None => text_value(info_dictionary, b"name")?,
    };
    validate_display_text(&display_name)?;

    let single_length = dictionary_value(info_dictionary, b"length");
    let multi_files = dictionary_value(info_dictionary, b"files");
    let files = match (single_length, multi_files) {
        (Some(length), None) => vec![TorrentFile {
            path: display_name.clone(),
            size: nonnegative_integer(length)?,
        }],
        (None, Some(files)) => parse_multi_file_list(files)?,
        _ => return Err(TorrentInspectionError::Unsupported),
    };
    let total_size = files.iter().try_fold(0_u64, |total, file| {
        total
            .checked_add(file.size)
            .ok_or(TorrentInspectionError::Unsupported)
    })?;
    let piece_hash_count = u64::try_from(pieces.len() / SHA1_DIGEST_BYTES)
        .map_err(|_| TorrentInspectionError::Unsupported)?;
    if piece_hash_count != total_size.div_ceil(piece_length) {
        return Err(TorrentInspectionError::Unsupported);
    }
    let infohash = hex_sha1(&bytes[info.start..info.end]);

    Ok(TorrentMetadata {
        display_name,
        infohash,
        total_size,
        files,
    })
}

fn parse_multi_file_list(
    files: &BencodedNode<'_>,
) -> Result<Vec<TorrentFile>, TorrentInspectionError> {
    let BencodedValue::List(files) = &files.value else {
        return Err(TorrentInspectionError::Unsupported);
    };
    if files.is_empty() {
        return Err(TorrentInspectionError::Unsupported);
    }

    let mut parsed_files = Vec::with_capacity(files.len());
    let mut paths = HashSet::new();
    for file in files {
        let file = dictionary(file)?;
        let size = nonnegative_integer(
            dictionary_value(file, b"length").ok_or(TorrentInspectionError::Unsupported)?,
        )?;
        let path = dictionary_value(file, b"path.utf-8")
            .or_else(|| dictionary_value(file, b"path"))
            .ok_or(TorrentInspectionError::Unsupported)?;
        let BencodedValue::List(components) = &path.value else {
            return Err(TorrentInspectionError::Unsupported);
        };
        if components.is_empty() {
            return Err(TorrentInspectionError::Unsupported);
        }
        let path = components
            .iter()
            .map(|component| match &component.value {
                BencodedValue::Bytes(value) => std::str::from_utf8(value)
                    .map(str::to_owned)
                    .map_err(|_| TorrentInspectionError::Unsupported),
                _ => Err(TorrentInspectionError::Unsupported),
            })
            .collect::<Result<Vec<_>, _>>()?;
        if path.iter().any(|component| {
            validate_display_text(component).is_err()
                || matches!(component.as_str(), "." | "..")
                || component.contains(['/', '\\'])
        }) {
            return Err(TorrentInspectionError::Unsupported);
        }
        let path = path.join("/");
        if !paths.insert(path.clone()) {
            return Err(TorrentInspectionError::Unsupported);
        }
        parsed_files.push(TorrentFile { path, size });
    }
    Ok(parsed_files)
}

fn dictionary<'a>(
    node: &'a BencodedNode<'a>,
) -> Result<&'a [(&'a [u8], BencodedNode<'a>)], TorrentInspectionError> {
    match &node.value {
        BencodedValue::Dictionary(entries) => Ok(entries),
        _ => Err(TorrentInspectionError::Unsupported),
    }
}

fn dictionary_value<'a>(
    dictionary: &'a [(&'a [u8], BencodedNode<'a>)],
    key: &[u8],
) -> Option<&'a BencodedNode<'a>> {
    dictionary
        .iter()
        .find_map(|(candidate, value)| (*candidate == key).then_some(value))
}

fn bytes_value<'a>(
    dictionary: &'a [(&'a [u8], BencodedNode<'a>)],
    key: &[u8],
) -> Result<&'a [u8], TorrentInspectionError> {
    match &dictionary_value(dictionary, key)
        .ok_or(TorrentInspectionError::Unsupported)?
        .value
    {
        BencodedValue::Bytes(value) => Ok(value),
        _ => Err(TorrentInspectionError::Unsupported),
    }
}

fn integer_value(
    dictionary: &[(&[u8], BencodedNode<'_>)],
    key: &[u8],
) -> Result<i64, TorrentInspectionError> {
    match dictionary_value(dictionary, key)
        .ok_or(TorrentInspectionError::Unsupported)?
        .value
    {
        BencodedValue::Integer(value) => Ok(value),
        _ => Err(TorrentInspectionError::Unsupported),
    }
}

fn nonnegative_integer(node: &BencodedNode<'_>) -> Result<u64, TorrentInspectionError> {
    match node.value {
        BencodedValue::Integer(value) if value >= 0 => Ok(value as u64),
        _ => Err(TorrentInspectionError::Unsupported),
    }
}

fn text_value(
    dictionary: &[(&[u8], BencodedNode<'_>)],
    key: &[u8],
) -> Result<String, TorrentInspectionError> {
    std::str::from_utf8(bytes_value(dictionary, key)?)
        .map(str::to_owned)
        .map_err(|_| TorrentInspectionError::Unsupported)
}

fn node_text(node: &BencodedNode<'_>) -> Result<String, TorrentInspectionError> {
    match &node.value {
        BencodedValue::Bytes(value) => std::str::from_utf8(value)
            .map(str::to_owned)
            .map_err(|_| TorrentInspectionError::Unsupported),
        _ => Err(TorrentInspectionError::Unsupported),
    }
}

fn validate_display_text(value: &str) -> Result<(), TorrentInspectionError> {
    if value.trim().is_empty() || value.chars().any(char::is_control) {
        return Err(TorrentInspectionError::Unsupported);
    }
    Ok(())
}

pub(crate) fn hex_sha1(input: &[u8]) -> String {
    sha1(input)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn sha1(input: &[u8]) -> [u8; 20] {
    let mut message = input.to_vec();
    let bit_length = (input.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_length.to_be_bytes());

    let mut hash = [
        0x6745_2301_u32,
        0xefcd_ab89,
        0x98ba_dcfe,
        0x1032_5476,
        0xc3d2_e1f0,
    ];
    for chunk in message.chunks_exact(64) {
        let mut words = [0_u32; 80];
        for (index, word) in words[..16].iter_mut().enumerate() {
            *word = u32::from_be_bytes(
                chunk[index * 4..index * 4 + 4]
                    .try_into()
                    .expect("SHA-1 chunks have complete words"),
            );
        }
        for index in 16..80 {
            words[index] =
                (words[index - 3] ^ words[index - 8] ^ words[index - 14] ^ words[index - 16])
                    .rotate_left(1);
        }

        let [mut a, mut b, mut c, mut d, mut e] = hash;
        for (index, word) in words.iter().enumerate() {
            let (function, constant) = match index {
                0..=19 => ((b & c) | ((!b) & d), 0x5a82_7999),
                20..=39 => (b ^ c ^ d, 0x6ed9_eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1b_bcdc),
                _ => (b ^ c ^ d, 0xca62_c1d6),
            };
            let next = a
                .rotate_left(5)
                .wrapping_add(function)
                .wrapping_add(e)
                .wrapping_add(constant)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = next;
        }
        hash[0] = hash[0].wrapping_add(a);
        hash[1] = hash[1].wrapping_add(b);
        hash[2] = hash[2].wrapping_add(c);
        hash[3] = hash[3].wrapping_add(d);
        hash[4] = hash[4].wrapping_add(e);
    }

    let mut result = [0_u8; 20];
    for (index, word) in hash.iter().enumerate() {
        result[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    result
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, fs};

    use super::*;

    fn single_file_torrent_with_fields(
        length: &str,
        piece_length: &str,
        piece_hash_count: usize,
    ) -> Vec<u8> {
        let piece_bytes = piece_hash_count * SHA1_DIGEST_BYTES;
        let mut encoded = b"d4:infod6:lengthi".to_vec();
        encoded.extend_from_slice(length.as_bytes());
        encoded.extend_from_slice(b"e4:name12:Movie  A.mp412:piece lengthi");
        encoded.extend_from_slice(piece_length.as_bytes());
        encoded.extend_from_slice(b"e6:pieces");
        encoded.extend_from_slice(piece_bytes.to_string().as_bytes());
        encoded.push(b':');
        encoded.extend(std::iter::repeat_n(b'a', piece_bytes));
        encoded.extend_from_slice(b"ee");
        encoded
    }

    fn single_file_torrent() -> Vec<u8> {
        single_file_torrent_with_fields("5", "16384", 1)
    }

    fn push_bencoded_bytes(encoded: &mut Vec<u8>, value: &str) {
        encoded.extend_from_slice(value.len().to_string().as_bytes());
        encoded.push(b':');
        encoded.extend_from_slice(value.as_bytes());
    }

    fn multi_file_torrent_with_piece_fields(
        piece_length: &str,
        piece_hash_count: usize,
    ) -> Vec<u8> {
        let mut encoded = b"d4:infod5:filesl".to_vec();
        encoded.extend_from_slice(b"d6:lengthi3e4:pathl");
        push_bencoded_bytes(&mut encoded, "Folder");
        push_bencoded_bytes(&mut encoded, "Part  1 — 映画.mkv");
        encoded.extend_from_slice(b"eed6:lengthi7e10:path.utf-8l");
        push_bencoded_bytes(&mut encoded, "Folder");
        push_bencoded_bytes(&mut encoded, "特別版  B.mp4");
        encoded.extend_from_slice(b"eee4:name");
        push_bencoded_bytes(&mut encoded, "VR  — 作品");
        encoded.extend_from_slice(b"12:piece lengthi");
        encoded.extend_from_slice(piece_length.as_bytes());
        encoded.extend_from_slice(b"e6:pieces");
        let piece_bytes = piece_hash_count * SHA1_DIGEST_BYTES;
        encoded.extend_from_slice(piece_bytes.to_string().as_bytes());
        encoded.push(b':');
        encoded.extend(std::iter::repeat_n(b'b', piece_bytes));
        encoded.extend_from_slice(b"ee");
        encoded
    }

    fn multi_file_torrent() -> Vec<u8> {
        multi_file_torrent_with_piece_fields("16384", 1)
    }

    fn duplicate_path_torrent() -> Vec<u8> {
        let mut encoded = b"d4:infod5:filesl".to_vec();
        for _ in 0..2 {
            encoded.extend_from_slice(b"d6:lengthi1e4:pathl");
            push_bencoded_bytes(&mut encoded, "same.mp4");
            encoded.extend_from_slice(b"ee");
        }
        encoded.extend_from_slice(
            b"e4:name4:Root12:piece lengthi1e6:pieces40:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaee",
        );
        encoded
    }

    fn fixture_infohash(bytes: &[u8]) -> String {
        let info_start = b"d4:info".len();
        assert!(bytes.starts_with(b"d4:info") && bytes.ends_with(b"e"));
        hex_sha1(&bytes[info_start..bytes.len() - 1])
    }

    fn release_feed(name: &str, item_id: &str, torrent_url: &str, infohash: &str) -> String {
        format!(
            "<rss><channel><item><title>{name}</title><guid>https://sukebei.nyaa.si/view/{item_id}</guid><link>{torrent_url}</link><nyaa:infoHash>{infohash}</nyaa:infoHash></item></channel></rss>"
        )
    }

    fn trusted_request(name: &str, infohash: &str) -> TorrentInspectionRequest {
        TorrentInspectionRequest {
            code: "MDVR-419".to_owned(),
            release_name: name.to_owned(),
            provider_item_id: "123".to_owned(),
            torrent_url: "https://sukebei.nyaa.si/download/123.torrent".to_owned(),
            expected_infohash: infohash.to_owned(),
        }
    }

    fn state_with_release(name: &str, infohash: &str) -> VrTorrentState {
        let state = VrTorrentState::default();
        let generation = state.begin_release_lookup().expect("lookup must start");
        state
            .finish_release_lookup(
                generation,
                "MDVR-419",
                &release_feed(
                    name,
                    "123",
                    "https://sukebei.nyaa.si/download/123.torrent",
                    infohash,
                ),
            )
            .expect("feed must be current");
        state
    }

    fn adult_trusted_request(name: &str, infohash: &str) -> TorrentInspectionRequest {
        TorrentInspectionRequest {
            code: "ADLT-123".to_owned(),
            release_name: name.to_owned(),
            provider_item_id: "321".to_owned(),
            torrent_url: "https://sukebei.nyaa.si/download/321.torrent".to_owned(),
            expected_infohash: infohash.to_owned(),
        }
    }

    fn adult_state_with_release(name: &str, infohash: &str) -> AdultTorrentState {
        let state = AdultTorrentState::default();
        let generation = state.begin_release_lookup().expect("lookup must start");
        state
            .finish_release_lookup(
                generation,
                "ADLT-123",
                &release_feed(
                    name,
                    "321",
                    "https://sukebei.nyaa.si/download/321.torrent",
                    infohash,
                ),
            )
            .expect("feed must be current");
        state
    }

    fn assert_inspection_rejected_without_cached_save(
        bytes: Vec<u8>,
        expected_error: &'static str,
    ) {
        let infohash = fixture_infohash(&bytes);
        let state = state_with_release("MDVR-419 exact", &infohash);
        assert_eq!(
            inspect_sukebei_torrent_with(
                &state,
                trusted_request("MDVR-419 exact", &infohash),
                |_| Ok(ArtifactResponse {
                    status: 200,
                    redirect_url: None,
                    body: bytes.clone(),
                }),
            ),
            Err(expected_error)
        );
        assert!(state
            .0
             .0
            .lock()
            .expect("torrent state must remain readable")
            .cached_torrent
            .is_none());

        let chose_destination = RefCell::new(false);
        let wrote = RefCell::new(false);
        assert_eq!(
            save_verified_torrent_with(
                &state,
                "rejected-inspection",
                |_| {
                    chose_destination.replace(true);
                    Some(PathBuf::from("unused.torrent"))
                },
                |_, _| {
                    wrote.replace(true);
                    Ok(())
                },
            ),
            Err(VR_TORRENT_STALE)
        );
        assert!(!chose_destination.into_inner());
        assert!(!wrote.into_inner());
    }

    #[test]
    fn computes_the_standard_sha1_vector() {
        assert_eq!(hex_sha1(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
    }

    #[test]
    fn retains_only_complete_same_item_artifacts_for_the_exact_identity() {
        let valid_hash = "0123456789abcdef0123456789abcdef01234567";
        let document = format!(
            "<rss><channel>{}{}{}{}{}{}{}{} </channel></rss>",
            release_feed(
                "MDVR-419 exact",
                "123",
                "https://sukebei.nyaa.si/download/123.torrent",
                valid_hash,
            ),
            release_feed(
                "MDVR-419 + ABC-123 pack",
                "124",
                "https://sukebei.nyaa.si/download/124.torrent",
                valid_hash,
            ),
            release_feed(
                "MDVR-419 wrong host",
                "125",
                "https://example.com/download/125.torrent",
                valid_hash,
            ),
            release_feed(
                "MDVR-419 credentials",
                "126",
                "https://user@sukebei.nyaa.si/download/126.torrent",
                valid_hash,
            ),
            release_feed(
                "MDVR-419 mismatched item",
                "127",
                "https://sukebei.nyaa.si/download/128.torrent",
                valid_hash,
            ),
            release_feed(
                "MDVR-419 invalid hash",
                "129",
                "https://sukebei.nyaa.si/download/129.torrent",
                "not-a-hash",
            ),
            release_feed(
                "MDVR-419 unsupported scheme",
                "130",
                "http://sukebei.nyaa.si/download/130.torrent",
                valid_hash,
            ),
            release_feed(
                "MDVR-419 malformed location",
                "131",
                "https://sukebei.nyaa.si/download/131.torrent?alternate=1",
                valid_hash,
            ),
        );

        assert_eq!(
            trusted_artifacts_from_feed(&document, "MDVR-419"),
            vec![TrustedArtifact {
                code: "MDVR-419".to_owned(),
                release_name: "MDVR-419 exact".to_owned(),
                provider_item_id: "123".to_owned(),
                torrent_url: "https://sukebei.nyaa.si/download/123.torrent".to_owned(),
                expected_infohash: valid_hash.to_owned(),
            }]
        );

        let adult_document = format!(
            "<rss><channel>{}{}<item><title>ADLT-123 metadata only</title></item></channel></rss>",
            release_feed(
                "ADLT-123 exact",
                "321",
                "https://sukebei.nyaa.si/download/321.torrent",
                valid_hash,
            ),
            release_feed(
                "ADLT-123 mismatched item",
                "322",
                "https://sukebei.nyaa.si/download/323.torrent",
                valid_hash,
            ),
        );
        assert_eq!(
            trusted_artifacts_from_feed(&adult_document, "ADLT-123"),
            vec![TrustedArtifact {
                code: "ADLT-123".to_owned(),
                release_name: "ADLT-123 exact".to_owned(),
                provider_item_id: "321".to_owned(),
                torrent_url: "https://sukebei.nyaa.si/download/321.torrent".to_owned(),
                expected_infohash: valid_hash.to_owned(),
            }]
        );
    }

    #[test]
    fn rejects_fabricated_adult_context_before_artifact_dispatch() {
        let bytes = single_file_torrent();
        let infohash = fixture_infohash(&bytes);
        let exact_release_name = "【Adult】 ADLT-123  Exact\t—\n特別版";
        let state = adult_state_with_release(exact_release_name, &infohash);
        let mut fabricated_requests = Vec::new();
        let mut wrong_code = adult_trusted_request(exact_release_name, &infohash);
        wrong_code.code = "ADLT-124".to_owned();
        fabricated_requests.push(wrong_code);
        let mut wrong_name = adult_trusted_request(exact_release_name, &infohash);
        wrong_name.release_name = "ADLT-123 fabricated".to_owned();
        fabricated_requests.push(wrong_name);
        let mut wrong_item = adult_trusted_request(exact_release_name, &infohash);
        wrong_item.provider_item_id = "999".to_owned();
        fabricated_requests.push(wrong_item);
        let mut wrong_url = adult_trusted_request(exact_release_name, &infohash);
        wrong_url.torrent_url = "https://sukebei.nyaa.si/download/999.torrent".to_owned();
        fabricated_requests.push(wrong_url);
        let mut wrong_hash = adult_trusted_request(exact_release_name, &infohash);
        wrong_hash.expected_infohash = "0000000000000000000000000000000000000001".to_owned();
        fabricated_requests.push(wrong_hash);

        for request in fabricated_requests {
            let dispatched = RefCell::new(false);
            assert_eq!(
                inspect_sukebei_adult_torrent_with(&state, request, |_| {
                    dispatched.replace(true);
                    unreachable!()
                }),
                Err(ADULT_TORRENT_CONTEXT_INVALID)
            );
            assert!(!dispatched.into_inner());
        }
    }

    #[test]
    fn adult_inspection_rejects_parser_and_artifact_failures_without_savable_bytes() {
        let valid_hash = "0123456789abcdef0123456789abcdef01234567";
        let cases = [
            (b"not-bencode".to_vec(), ADULT_TORRENT_MALFORMED),
            (
                b"d4:infod12:meta versioni2eee".to_vec(),
                ADULT_TORRENT_UNSUPPORTED,
            ),
            (
                single_file_torrent_with_fields("+1", "16384", 1),
                ADULT_TORRENT_MALFORMED,
            ),
            (
                single_file_torrent_with_fields("5", "16384", 2),
                ADULT_TORRENT_UNSUPPORTED,
            ),
            (
                b"d4:infod5:filesld6:lengthi1e4:pathl2:..eee4:name4:Root12:piece lengthi1e6:pieces20:aaaaaaaaaaaaaaaaaaaaee".to_vec(),
                ADULT_TORRENT_UNSUPPORTED,
            ),
        ];

        for (bytes, expected_error) in cases {
            let state = adult_state_with_release("ADLT-123 exact", valid_hash);
            assert_eq!(
                inspect_sukebei_adult_torrent_with(
                    &state,
                    adult_trusted_request("ADLT-123 exact", valid_hash),
                    |_| {
                        Ok(ArtifactResponse {
                            status: 200,
                            redirect_url: None,
                            body: bytes.clone(),
                        })
                    },
                ),
                Err(expected_error)
            );
            let chose_destination = RefCell::new(false);
            assert_eq!(
                save_verified_adult_torrent_with(
                    &state,
                    "adult-rejected",
                    |_| {
                        chose_destination.replace(true);
                        None
                    },
                    |_, _| unreachable!(),
                ),
                Err(ADULT_TORRENT_STALE)
            );
            assert!(!chose_destination.into_inner());
        }

        for (response, expected_error) in [
            (
                Ok(ArtifactResponse {
                    status: 404,
                    redirect_url: None,
                    body: Vec::new(),
                }),
                ADULT_TORRENT_SOURCE_UNAVAILABLE,
            ),
            (
                Ok(ArtifactResponse {
                    status: 302,
                    redirect_url: Some("https://example.com/321.torrent".to_owned()),
                    body: Vec::new(),
                }),
                ADULT_TORRENT_PROVIDER_ERROR,
            ),
            (
                Err(ArtifactRequestError::Network),
                ADULT_TORRENT_NETWORK_ERROR,
            ),
            (Err(ArtifactRequestError::TooLarge), ADULT_TORRENT_MALFORMED),
        ] {
            let state = adult_state_with_release("ADLT-123 exact", valid_hash);
            assert_eq!(
                inspect_sukebei_adult_torrent_with(
                    &state,
                    adult_trusted_request("ADLT-123 exact", valid_hash),
                    |_| response.clone(),
                ),
                Err(expected_error)
            );
        }
    }

    #[test]
    fn adult_redirects_remain_same_item_and_bounded() {
        let bytes = single_file_torrent();
        let infohash = fixture_infohash(&bytes);
        let state = adult_state_with_release("ADLT-123 exact", &infohash);
        let requests = RefCell::new(Vec::new());
        let response = inspect_sukebei_adult_torrent_with(
            &state,
            adult_trusted_request("ADLT-123 exact", &infohash),
            |url| {
                let request_number = requests.borrow().len();
                requests.borrow_mut().push(url.to_owned());
                if request_number == 0 {
                    Ok(ArtifactResponse {
                        status: 302,
                        redirect_url: Some(
                            "https://sukebei.nyaa.si/download/321.torrent".to_owned(),
                        ),
                        body: Vec::new(),
                    })
                } else {
                    Ok(ArtifactResponse {
                        status: 200,
                        redirect_url: None,
                        body: bytes.clone(),
                    })
                }
            },
        )
        .expect("the same Adult provider item redirect must remain inspectable");
        assert_eq!(response[2], infohash);
        assert_eq!(requests.into_inner().len(), 2);

        let state = adult_state_with_release("ADLT-123 exact", &infohash);
        let request_count = RefCell::new(0);
        assert_eq!(
            inspect_sukebei_adult_torrent_with(
                &state,
                adult_trusted_request("ADLT-123 exact", &infohash),
                |_| {
                    request_count.replace_with(|count| *count + 1);
                    Ok(ArtifactResponse {
                        status: 302,
                        redirect_url: Some(
                            "https://sukebei.nyaa.si/download/321.torrent".to_owned(),
                        ),
                        body: Vec::new(),
                    })
                },
            ),
            Err(ADULT_TORRENT_PROVIDER_ERROR)
        );
        assert_eq!(request_count.into_inner(), TORRENT_MAX_REDIRECTS + 1);
    }

    #[test]
    fn adult_inspection_saves_exact_bytes_and_rejects_cross_category_identities() {
        let bytes = multi_file_torrent();
        let infohash = fixture_infohash(&bytes);
        let adult_release_name = "【Adult】 ADLT-123  Exact\t—\n特別版";
        let adult_state = adult_state_with_release(adult_release_name, &infohash);
        let adult_response = inspect_sukebei_adult_torrent_with(
            &adult_state,
            adult_trusted_request(adult_release_name, &infohash),
            |_| {
                Ok(ArtifactResponse {
                    status: 200,
                    redirect_url: None,
                    body: bytes.clone(),
                })
            },
        )
        .expect("trusted Adult inspection must succeed");
        let adult_inspection_id = &adult_response[0];
        assert!(adult_inspection_id.starts_with("adult-"));
        assert_eq!(adult_response[1], "VR  — 作品");
        assert_eq!(adult_response[2], infohash);
        assert_eq!(adult_response[4], "Folder/Part  1 — 映画.mkv");
        assert_eq!(adult_response[6], "Folder/特別版  B.mp4");
        let adult_download_source = adult_state
            .verified_download_source(adult_inspection_id, &[1])
            .expect("current Adult inspection must authorize exact selected files");
        assert_eq!(adult_download_source.code, "ADLT-123");
        assert_eq!(adult_download_source.release_name, adult_release_name);
        assert_eq!(adult_download_source.infohash, infohash);
        assert_eq!(adult_download_source.bytes, bytes);
        assert_eq!(
            adult_download_source.selected_files,
            vec![VerifiedDownloadFile {
                file_id: 1,
                path: "Folder/特別版  B.mp4".to_owned(),
                size: 7,
            }]
        );
        assert_eq!(
            adult_state.verified_download_source(adult_inspection_id, &[]),
            Err(VerifiedDownloadSourceError::Selection)
        );

        let wrote_cancelled = RefCell::new(false);
        assert_eq!(
            save_verified_adult_torrent_with(
                &adult_state,
                adult_inspection_id,
                |_| None,
                |_, _| {
                    wrote_cancelled.replace(true);
                    Ok(())
                },
            ),
            Ok(false)
        );
        assert!(!wrote_cancelled.into_inner());

        let destination = std::env::temp_dir().join(format!(
            "auto-video-adult-torrent-save-{}-{adult_inspection_id}.torrent",
            std::process::id()
        ));
        assert_eq!(
            save_verified_adult_torrent_with(
                &adult_state,
                adult_inspection_id,
                |default_name| {
                    assert_eq!(default_name, "ADLT-123-321.torrent");
                    Some(destination.clone())
                },
                write_new_torrent_file,
            ),
            Ok(true)
        );
        assert_eq!(fs::read(&destination).expect("saved bytes"), bytes);
        assert_eq!(
            save_verified_adult_torrent_with(
                &adult_state,
                adult_inspection_id,
                |_| Some(destination.clone()),
                write_new_torrent_file,
            ),
            Err(ADULT_TORRENT_SAVE_FAILED)
        );
        assert_eq!(fs::read(&destination).expect("existing bytes"), bytes);

        let vr_state = state_with_release("MDVR-419 exact", &infohash);
        let vr_response = inspect_sukebei_torrent_with(
            &vr_state,
            trusted_request("MDVR-419 exact", &infohash),
            |_| {
                Ok(ArtifactResponse {
                    status: 200,
                    redirect_url: None,
                    body: bytes.clone(),
                })
            },
        )
        .expect("trusted VR inspection must succeed");
        assert!(vr_response[0].starts_with("vr-"));

        let adult_dispatched_from_vr = RefCell::new(false);
        assert_eq!(
            inspect_sukebei_torrent_with(
                &vr_state,
                adult_trusted_request(adult_release_name, &infohash),
                |_| {
                    adult_dispatched_from_vr.replace(true);
                    unreachable!()
                },
            ),
            Err(VR_TORRENT_CONTEXT_INVALID)
        );
        assert!(!adult_dispatched_from_vr.into_inner());

        let vr_dispatched_from_adult = RefCell::new(false);
        assert_eq!(
            inspect_sukebei_adult_torrent_with(
                &adult_state,
                trusted_request("MDVR-419 exact", &infohash),
                |_| {
                    vr_dispatched_from_adult.replace(true);
                    unreachable!()
                },
            ),
            Err(ADULT_TORRENT_CONTEXT_INVALID)
        );
        assert!(!vr_dispatched_from_adult.into_inner());

        let chose_adult_destination = RefCell::new(false);
        assert_eq!(
            save_verified_adult_torrent_with(
                &adult_state,
                &vr_response[0],
                |_| {
                    chose_adult_destination.replace(true);
                    None
                },
                |_, _| unreachable!(),
            ),
            Err(ADULT_TORRENT_STALE)
        );
        assert!(!chose_adult_destination.into_inner());

        let chose_vr_destination = RefCell::new(false);
        assert_eq!(
            save_verified_torrent_with(
                &vr_state,
                adult_inspection_id,
                |_| {
                    chose_vr_destination.replace(true);
                    None
                },
                |_, _| unreachable!(),
            ),
            Err(VR_TORRENT_STALE)
        );
        assert_eq!(
            vr_state.verified_download_source(adult_inspection_id, &[0]),
            Err(VerifiedDownloadSourceError::Context)
        );
        assert_eq!(
            adult_state.verified_download_source(&vr_response[0], &[0]),
            Err(VerifiedDownloadSourceError::Context)
        );
        assert!(!chose_vr_destination.into_inner());
        fs::remove_file(destination).expect("fixture must be removable");
    }

    #[test]
    fn verifies_single_and_multi_file_metainfo_with_exact_paths() {
        let single = parse_torrent_metadata(&single_file_torrent()).expect("valid single file");
        assert_eq!(single.display_name, "Movie  A.mp4");
        assert_eq!(single.infohash, "8b16011989123e1d68a8aaf18f5a599e6a4a0bc7");
        assert_eq!(single.total_size, 5);
        assert_eq!(
            single.files,
            vec![TorrentFile {
                path: "Movie  A.mp4".to_owned(),
                size: 5
            }]
        );

        let multi = parse_torrent_metadata(&multi_file_torrent()).expect("valid multi file");
        assert_eq!(multi.display_name, "VR  — 作品");
        assert_eq!(multi.total_size, 10);
        assert_eq!(multi.files[0].path, "Folder/Part  1 — 映画.mkv");
        assert_eq!(multi.files[1].path, "Folder/特別版  B.mp4");
    }

    #[test]
    fn rejects_malformed_unsupported_and_unsafe_metainfo() {
        assert_eq!(
            parse_torrent_metadata(b"not-bencode"),
            Err(TorrentInspectionError::Malformed)
        );
        assert_eq!(
            parse_torrent_metadata(b"d4:infod12:meta versioni2eee"),
            Err(TorrentInspectionError::Unsupported)
        );
        assert_eq!(
            parse_torrent_metadata(b"d4:infod5:filesld6:lengthi1e4:pathl2:..eee4:name4:Root12:piece lengthi1e6:pieces20:aaaaaaaaaaaaaaaaaaaaee"),
            Err(TorrentInspectionError::Unsupported)
        );
        assert_eq!(
            parse_torrent_metadata(&duplicate_path_torrent()),
            Err(TorrentInspectionError::Unsupported)
        );
    }

    #[test]
    fn rejects_plus_prefixed_required_integers_without_caching_for_save() {
        for bytes in [
            single_file_torrent_with_fields("+1", "16384", 1),
            single_file_torrent_with_fields("5", "+03", 2),
            single_file_torrent_with_fields("05", "16384", 1),
            single_file_torrent_with_fields("5", "016384", 1),
        ] {
            assert_inspection_rejected_without_cached_save(bytes, VR_TORRENT_MALFORMED);
        }
    }

    #[test]
    fn rejects_too_many_and_too_few_piece_hashes_without_caching_for_save() {
        let too_many_single_file_hashes = single_file_torrent_with_fields("5", "16384", 2);
        assert_inspection_rejected_without_cached_save(
            too_many_single_file_hashes,
            VR_TORRENT_UNSUPPORTED,
        );

        let too_few_multi_file_hashes = multi_file_torrent_with_piece_fields("4", 2);
        assert_inspection_rejected_without_cached_save(
            too_few_multi_file_hashes,
            VR_TORRENT_UNSUPPORTED,
        );
    }

    #[test]
    fn rejects_fabricated_context_before_artifact_dispatch() {
        let bytes = single_file_torrent();
        let infohash = parse_torrent_metadata(&bytes)
            .expect("valid torrent")
            .infohash;
        let state = state_with_release("MDVR-419 exact", &infohash);
        let mut fabricated_requests = Vec::new();
        let mut wrong_code = trusted_request("MDVR-419 exact", &infohash);
        wrong_code.code = "MDVR-422".to_owned();
        fabricated_requests.push(wrong_code);
        let mut wrong_name = trusted_request("MDVR-419 exact", &infohash);
        wrong_name.release_name = "MDVR-419 fabricated".to_owned();
        fabricated_requests.push(wrong_name);
        let mut wrong_item = trusted_request("MDVR-419 exact", &infohash);
        wrong_item.provider_item_id = "999".to_owned();
        fabricated_requests.push(wrong_item);
        let mut wrong_url = trusted_request("MDVR-419 exact", &infohash);
        wrong_url.torrent_url = "https://sukebei.nyaa.si/download/999.torrent".to_owned();
        fabricated_requests.push(wrong_url);
        let mut wrong_hash = trusted_request("MDVR-419 exact", &infohash);
        wrong_hash.expected_infohash = "0000000000000000000000000000000000000001".to_owned();
        fabricated_requests.push(wrong_hash);

        for request in fabricated_requests {
            let dispatched = RefCell::new(false);
            assert_eq!(
                inspect_sukebei_torrent_with(&state, request, |_| {
                    dispatched.replace(true);
                    unreachable!()
                }),
                Err(VR_TORRENT_CONTEXT_INVALID)
            );
            assert!(!dispatched.into_inner());
        }
    }

    #[test]
    fn rejects_boundary_escaping_redirects_and_infohash_mismatches() {
        let bytes = single_file_torrent();
        let infohash = parse_torrent_metadata(&bytes)
            .expect("valid torrent")
            .infohash;
        let state = state_with_release("MDVR-419 exact", &infohash);
        assert_eq!(
            inspect_sukebei_torrent_with(
                &state,
                trusted_request("MDVR-419 exact", &infohash),
                |_| Ok(ArtifactResponse {
                    status: 302,
                    redirect_url: Some("https://example.com/123.torrent".to_owned()),
                    body: Vec::new(),
                })
            ),
            Err(VR_TORRENT_PROVIDER_ERROR)
        );

        let wrong_hash = "0000000000000000000000000000000000000001";
        let state = state_with_release("MDVR-419 exact", wrong_hash);
        assert_eq!(
            inspect_sukebei_torrent_with(
                &state,
                trusted_request("MDVR-419 exact", wrong_hash),
                |_| Ok(ArtifactResponse {
                    status: 200,
                    redirect_url: None,
                    body: bytes.clone(),
                })
            ),
            Err(VR_TORRENT_INFOHASH_MISMATCH)
        );
    }

    #[test]
    fn distinguishes_artifact_fetch_failures_and_accepts_only_bounded_bytes() {
        let bytes = single_file_torrent();
        let infohash = parse_torrent_metadata(&bytes)
            .expect("valid torrent")
            .infohash;
        for (response, expected_error) in [
            (
                Ok(ArtifactResponse {
                    status: 404,
                    redirect_url: None,
                    body: Vec::new(),
                }),
                VR_TORRENT_SOURCE_UNAVAILABLE,
            ),
            (
                Ok(ArtifactResponse {
                    status: 500,
                    redirect_url: None,
                    body: Vec::new(),
                }),
                VR_TORRENT_PROVIDER_ERROR,
            ),
            (Err(ArtifactRequestError::Network), VR_TORRENT_NETWORK_ERROR),
            (Err(ArtifactRequestError::TooLarge), VR_TORRENT_MALFORMED),
        ] {
            let state = state_with_release("MDVR-419 exact", &infohash);
            assert_eq!(
                inspect_sukebei_torrent_with(
                    &state,
                    trusted_request("MDVR-419 exact", &infohash),
                    |_| response.clone(),
                ),
                Err(expected_error)
            );
        }

        let state = state_with_release("MDVR-419 exact", &infohash);
        let requests = RefCell::new(Vec::new());
        let response = inspect_sukebei_torrent_with(
            &state,
            trusted_request("MDVR-419 exact", &infohash),
            |url| {
                let request_number = requests.borrow().len();
                requests.borrow_mut().push(url.to_owned());
                if request_number == 0 {
                    Ok(ArtifactResponse {
                        status: 302,
                        redirect_url: Some(
                            "https://sukebei.nyaa.si/download/123.torrent".to_owned(),
                        ),
                        body: Vec::new(),
                    })
                } else {
                    Ok(ArtifactResponse {
                        status: 200,
                        redirect_url: None,
                        body: bytes.clone(),
                    })
                }
            },
        )
        .expect("same-artifact redirects must remain inspectable");
        assert_eq!(response[2], infohash);
        assert_eq!(requests.into_inner().len(), 2);
    }

    #[test]
    fn parses_binary_command_output_without_treating_markers_as_torrent_bytes() {
        let mut output = single_file_torrent();
        output.extend_from_slice(b"\nAUTO_VIDEO_TORRENT_STATUS:200\nAUTO_VIDEO_TORRENT_REDIRECT:");
        assert_eq!(
            parse_artifact_command_output(&output),
            Ok(ArtifactResponse {
                status: 200,
                redirect_url: None,
                body: single_file_torrent(),
            })
        );
    }

    #[test]
    fn saves_only_the_exact_cached_bytes_and_treats_cancellation_as_no_write() {
        let bytes = single_file_torrent();
        let infohash = parse_torrent_metadata(&bytes)
            .expect("valid torrent")
            .infohash;
        let state = state_with_release("MDVR-419 exact", &infohash);
        let response = inspect_sukebei_torrent_with(
            &state,
            trusted_request("MDVR-419 exact", &infohash),
            |_| {
                Ok(ArtifactResponse {
                    status: 200,
                    redirect_url: None,
                    body: bytes.clone(),
                })
            },
        )
        .expect("inspection must succeed");
        let inspection_id = &response[0];

        let wrote_cancelled = RefCell::new(false);
        assert_eq!(
            save_verified_torrent_with(
                &state,
                inspection_id,
                |_| None,
                |_, _| {
                    wrote_cancelled.replace(true);
                    Ok(())
                },
            ),
            Ok(false)
        );
        assert!(!wrote_cancelled.into_inner());

        let destination = std::env::temp_dir().join(format!(
            "auto-video-torrent-save-{}-{}.torrent",
            std::process::id(),
            inspection_id
        ));
        assert_eq!(
            save_verified_torrent_with(
                &state,
                inspection_id,
                |default_name| {
                    assert_eq!(default_name, "MDVR-419-123.torrent");
                    Some(destination.clone())
                },
                |path, bytes| fs::write(path, bytes),
            ),
            Ok(true)
        );
        assert_eq!(fs::read(&destination).expect("saved bytes"), bytes);
        fs::remove_file(destination).expect("fixture must be removable");
    }

    #[test]
    fn invalidation_blocks_a_late_save_before_writing() {
        let bytes = single_file_torrent();
        let infohash = parse_torrent_metadata(&bytes)
            .expect("valid torrent")
            .infohash;
        let state = state_with_release("MDVR-419 exact", &infohash);
        let response = inspect_sukebei_torrent_with(
            &state,
            trusted_request("MDVR-419 exact", &infohash),
            |_| {
                Ok(ArtifactResponse {
                    status: 200,
                    redirect_url: None,
                    body: bytes.clone(),
                })
            },
        )
        .expect("inspection must succeed");
        let wrote = RefCell::new(false);

        assert_eq!(
            save_verified_torrent_with(
                &state,
                &response[0],
                |_| {
                    state
                        .invalidate_inspection()
                        .expect("invalidation must work");
                    Some(PathBuf::from("unused.torrent"))
                },
                |_, _| {
                    wrote.replace(true);
                    Ok(())
                },
            ),
            Err(VR_TORRENT_STALE)
        );
        assert!(!wrote.into_inner());
    }

    #[test]
    fn download_source_uses_only_the_current_native_inspection_and_selected_ids() {
        let bytes = multi_file_torrent();
        let infohash = fixture_infohash(&bytes);
        let exact_release_name = "【VR】 MDVR-419  Exact\t—\n特別版";
        let state = state_with_release(exact_release_name, &infohash);
        let response = inspect_sukebei_torrent_with(
            &state,
            trusted_request(exact_release_name, &infohash),
            |_| {
                Ok(ArtifactResponse {
                    status: 200,
                    redirect_url: None,
                    body: bytes.clone(),
                })
            },
        )
        .expect("trusted inspection must succeed");

        let source = state
            .verified_download_source(&response[0], &[1])
            .expect("current selected file must resolve from native state");
        assert_eq!(source.bytes, bytes);
        assert_eq!(source.code, "MDVR-419");
        assert_eq!(source.release_name, exact_release_name);
        assert_eq!(source.infohash, infohash);
        assert_eq!(
            source.selected_files,
            vec![VerifiedDownloadFile {
                file_id: 1,
                path: "Folder/特別版  B.mp4".to_owned(),
                size: 7,
            }]
        );
        assert_eq!(
            state.verified_download_source(&response[0], &[1, 1]),
            Err(VerifiedDownloadSourceError::Selection)
        );

        state
            .invalidate_inspection()
            .expect("invalidation must succeed");
        assert_eq!(
            state.verified_download_source(&response[0], &[1]),
            Err(VerifiedDownloadSourceError::Context)
        );
    }

    #[test]
    fn persisted_download_revalidation_rejects_changed_or_ambiguous_identity() {
        let bytes = single_file_torrent();
        let infohash = fixture_infohash(&bytes);
        let exact_release_name = "【VR】 MDVR-419  Exact\t—\n特別版";
        let source = revalidate_persisted_download_source(
            &bytes,
            "MDVR-419",
            exact_release_name,
            &infohash,
            &[0],
        )
        .expect("exact persisted identity must revalidate");
        assert_eq!(source.release_name, exact_release_name);

        assert_eq!(
            revalidate_persisted_download_source(
                &bytes,
                "MDVR-419",
                "MDVR-419 + ABC-123 pack",
                &infohash,
                &[0],
            ),
            Err(VerifiedDownloadSourceError::Context)
        );
        assert_eq!(
            revalidate_persisted_download_source(
                &bytes,
                "MDVR-419",
                exact_release_name,
                "0000000000000000000000000000000000000000",
                &[0],
            ),
            Err(VerifiedDownloadSourceError::Metainfo)
        );
        assert_eq!(
            revalidate_persisted_download_source(
                &bytes,
                "MDVR-419",
                exact_release_name,
                &infohash,
                &[1],
            ),
            Err(VerifiedDownloadSourceError::Selection)
        );
    }

    fn tmdb_movie_documents() -> (&'static str, &'static str) {
        (
            r#"{"id":419,"title":"Exact  Movie — 特別版","release_date":"1999-04-19"}"#,
            r#"{"id":419,"imdb_id":"tt0123456"}"#,
        )
    }

    fn exact_yts_document(infohash: &str) -> String {
        format!(
            r#"{{"status":"ok","data":{{"movies":[
              {{"id":700,"imdb_code":"tt0123456","title":"Exact  Movie — 特別版","year":1999,"torrents":[
                {{"quality":"1080p","type":"bluray","video_codec":"x264","size":"1.5 GB","size_bytes":1500000000,"seeds":42,"peers":7,"hash":"{upper_hash}","url":"https://yts.mx/torrent/download/{upper_hash}"}},
                {{"quality":"2160p","type":"web","video_codec":"x265","size":"Unavailable","seeds":2}}
              ]}},
              {{"id":701,"imdb_code":"tt7654321","title":"Exact  Movie — 特別版","year":1999,"torrents":[{{"quality":"wrong identity"}}]}},
              {{"id":702,"imdb_code":"tt7654322","title":"Exact  Movie — 特別版","year":2000,"torrents":[{{"quality":"neighboring year"}}]}},
              {{"id":703,"title":"Exact  Movie — 特別版","year":1999,"torrents":[{{"quality":"missing identity"}}]}},
              {{"id":704,"imdb_code":"not-an-imdb-id","title":"Exact  Movie — 特別版","year":1999,"torrents":[{{"quality":"malformed identity"}}]}},
              {{"id":705,"imdb_code":"tt9999999","title":"Unrelated result","year":1980,"torrents":[{{"quality":"unrelated"}}]}}
            ]}}}}"#,
            upper_hash = infohash.to_ascii_uppercase(),
        )
    }

    fn trusted_movie_request(infohash: &str) -> MovieTorrentInspectionRequest {
        MovieTorrentInspectionRequest {
            tmdb_movie_id: 419,
            tmdb_title: "Exact  Movie — 特別版".to_owned(),
            release_date: Some("1999-04-19".to_owned()),
            imdb_id: "tt0123456".to_owned(),
            provider_movie_id: 700,
            provider_title: Some("Exact  Movie — 特別版".to_owned()),
            provider_year: Some("1999".to_owned()),
            row_id: "700:0".to_owned(),
            quality: Some("1080p".to_owned()),
            type_label: Some("bluray".to_owned()),
            video_codec: Some("x264".to_owned()),
            size: Some("1.5 GB".to_owned()),
            size_bytes: Some("1500000000".to_owned()),
            seeds: Some("42".to_owned()),
            peers: Some("7".to_owned()),
            expected_infohash: infohash.to_owned(),
            torrent_url: format!(
                "https://yts.mx/torrent/download/{}",
                infohash.to_ascii_uppercase()
            ),
        }
    }

    fn movie_state_with_release(infohash: &str) -> MovieTorrentState {
        let state = MovieTorrentState::default();
        let generation = state
            .begin_release_lookup()
            .expect("Movie release lookup must begin");
        let (details, external_ids) = tmdb_movie_documents();
        state
            .finish_release_lookup(
                generation,
                419,
                details,
                external_ids,
                &exact_yts_document(infohash),
            )
            .expect("exact Movie release set must be accepted");
        state
    }

    #[test]
    fn movie_identity_accepts_only_the_exact_imdb_movie_and_preserves_metadata() {
        let infohash = "0123456789abcdef0123456789abcdef01234567";
        let (details, external_ids) = tmdb_movie_documents();
        assert_eq!(
            verified_movie_imdb_id(419, details, external_ids),
            Ok("tt0123456".to_owned())
        );
        let release_set =
            trusted_movie_release_set(3, 419, details, external_ids, &exact_yts_document(infohash))
                .expect("the exact IMDb candidate must be accepted");

        assert_eq!(release_set.context.tmdb_title, "Exact  Movie — 特別版");
        assert_eq!(release_set.context.imdb_id, "tt0123456");
        assert_eq!(release_set.context.provider_movie_id, 700);
        assert_eq!(
            release_set.context.provider_title.as_deref(),
            Some("Exact  Movie — 特別版")
        );
        assert_eq!(release_set.torrents.len(), 2);
        assert_eq!(release_set.torrents[0].quality.as_deref(), Some("1080p"));
        assert_eq!(
            release_set.torrents[0].expected_infohash.as_deref(),
            Some(infohash)
        );
        assert_eq!(release_set.torrents[1].quality.as_deref(), Some("2160p"));
        assert_eq!(release_set.torrents[1].expected_infohash, None);
        assert_eq!(encode_movie_release_set(&release_set)[7], "2");
        for rejected_quality in [
            "wrong identity",
            "neighboring year",
            "missing identity",
            "malformed identity",
            "unrelated",
        ] {
            assert!(release_set
                .torrents
                .iter()
                .all(|torrent| torrent.quality.as_deref() != Some(rejected_quality)));
        }
    }

    #[test]
    fn movie_identity_rejects_wrong_tmdb_context_and_missing_or_malformed_imdb_ids() {
        let (details, external_ids) = tmdb_movie_documents();
        for (details_document, external_ids_document, expected) in [
            (
                r#"{"id":420,"title":"Exact Movie","release_date":"1999-04-19"}"#,
                external_ids,
                MOVIE_TMDB_MALFORMED,
            ),
            (
                r#"{"id":419,"title":" ","release_date":"1999-04-19"}"#,
                external_ids,
                MOVIE_TMDB_MALFORMED,
            ),
            (
                r#"{"id":419,"title":"Exact Movie","release_date":"1999"}"#,
                external_ids,
                MOVIE_TMDB_MALFORMED,
            ),
            (
                details,
                r#"{"id":420,"imdb_id":"tt0123456"}"#,
                MOVIE_TMDB_MALFORMED,
            ),
            (
                details,
                r#"{"id":419,"imdb_id":null}"#,
                MOVIE_NO_IMDB_IDENTITY,
            ),
            (
                details,
                r#"{"id":419,"imdb_id":""}"#,
                MOVIE_NO_IMDB_IDENTITY,
            ),
            (
                details,
                r#"{"id":419,"imdb_id":"tt123"}"#,
                MOVIE_TMDB_MALFORMED,
            ),
        ] {
            assert_eq!(
                verified_movie_imdb_id(419, details_document, external_ids_document),
                Err(expected)
            );
        }
    }

    #[test]
    fn movie_identity_rejects_conflicting_exact_imdb_provider_objects() {
        let (details, external_ids) = tmdb_movie_documents();
        let conflicting = r#"{"status":"ok","data":{"movies":[
          {"id":700,"imdb_code":"tt0123456","title":"Exact Movie","year":1999,"torrents":[]},
          {"id":701,"imdb_code":"tt0123456","title":"Conflicting Movie","year":1999,"torrents":[]}
        ]}}"#;
        assert_eq!(
            trusted_movie_release_set(1, 419, details, external_ids, conflicting),
            Err(MOVIE_YTS_CONFLICTING_PROVIDER)
        );
    }

    #[test]
    fn newer_movie_release_generation_rejects_an_older_provider_result() {
        let infohash = "0123456789abcdef0123456789abcdef01234567";
        let state = MovieTorrentState::default();
        let older_generation = state
            .begin_release_lookup()
            .expect("older Movie lookup must begin");
        let current_generation = state
            .begin_release_lookup()
            .expect("current Movie lookup must begin");
        let (details, external_ids) = tmdb_movie_documents();
        assert!(state
            .finish_release_lookup(
                current_generation,
                419,
                details,
                external_ids,
                &exact_yts_document(infohash),
            )
            .is_ok());
        assert_eq!(
            state.finish_release_lookup(
                older_generation,
                419,
                details,
                external_ids,
                &exact_yts_document(infohash),
            ),
            Err(MOVIE_TORRENT_STALE)
        );
        assert!(state
            .begin_inspection(&trusted_movie_request(infohash))
            .is_ok());
    }

    #[test]
    fn movie_inspection_rejects_fabricated_context_before_artifact_dispatch() {
        let infohash = "0123456789abcdef0123456789abcdef01234567";
        let state = movie_state_with_release(infohash);
        let exact = trusted_movie_request(infohash);
        let mut fabricated = Vec::new();
        let mut request = exact.clone();
        request.tmdb_movie_id = 420;
        fabricated.push(request);
        let mut request = exact.clone();
        request.imdb_id = "tt7654321".to_owned();
        fabricated.push(request);
        let mut request = exact.clone();
        request.provider_movie_id = 701;
        fabricated.push(request);
        let mut request = exact.clone();
        request.quality = Some("2160p".to_owned());
        fabricated.push(request);
        let mut request = exact.clone();
        request.torrent_url = format!("https://example.com/torrent/{infohash}");
        fabricated.push(request);
        let mut request = exact.clone();
        request.row_id = "700:fabricated".to_owned();
        fabricated.push(request);
        let mut request = exact.clone();
        request.provider_title = Some("Fabricated provider title".to_owned());
        fabricated.push(request);
        let mut request = exact.clone();
        request.expected_infohash = "abcdef0123456789abcdef0123456789abcdef01".to_owned();
        request.torrent_url = format!(
            "https://yts.mx/torrent/download/{}",
            request.expected_infohash
        );
        fabricated.push(request);
        let dispatch_count = RefCell::new(0_u32);
        let (release_generation, inspection_generation) = state
            .begin_inspection(&exact)
            .expect("exact Movie inspection must begin");

        for request in fabricated {
            assert_eq!(
                inspect_yts_movie_torrent_with(
                    &state,
                    release_generation,
                    inspection_generation,
                    request,
                    |_| {
                        dispatch_count.replace_with(|count| *count + 1);
                        unreachable!("fabricated Movie context must fail before dispatch")
                    }
                ),
                Err(MOVIE_TORRENT_CONTEXT_INVALID)
            );
        }
        assert_eq!(dispatch_count.into_inner(), 0);
    }

    #[test]
    fn movie_artifact_redirects_remain_on_the_exact_hash_and_are_bounded() {
        let infohash = "0123456789abcdef0123456789abcdef01234567";
        let request = trusted_movie_request(infohash);
        assert_eq!(
            fetch_yts_torrent_artifact(&request, |_| Ok(ArtifactResponse {
                status: 302,
                redirect_url: Some(format!("https://example.com/{infohash}")),
                body: Vec::new(),
            })),
            Err(TorrentInspectionError::Provider)
        );

        let bytes = multi_file_torrent();
        let dispatch_count = RefCell::new(0_u32);
        assert_eq!(
            fetch_yts_torrent_artifact(&request, |_| {
                let current_count = dispatch_count.replace_with(|count| *count + 1);
                Ok(if current_count == 0 {
                    ArtifactResponse {
                        status: 302,
                        redirect_url: Some(request.torrent_url.clone()),
                        body: Vec::new(),
                    }
                } else {
                    ArtifactResponse {
                        status: 200,
                        redirect_url: None,
                        body: bytes.clone(),
                    }
                })
            }),
            Ok(bytes)
        );
        assert_eq!(dispatch_count.into_inner(), 2);

        let dispatch_count = RefCell::new(0_u32);
        assert_eq!(
            fetch_yts_torrent_artifact(&request, |_| {
                dispatch_count.replace_with(|count| *count + 1);
                Ok(ArtifactResponse {
                    status: 302,
                    redirect_url: Some(request.torrent_url.clone()),
                    body: Vec::new(),
                })
            }),
            Err(TorrentInspectionError::Provider)
        );
        assert_eq!(dispatch_count.into_inner(), 4);
    }

    #[test]
    fn newer_movie_inspection_generation_rejects_late_artifact_bytes() {
        let bytes = multi_file_torrent();
        let infohash = fixture_infohash(&bytes);
        let state = movie_state_with_release(&infohash);
        let request = trusted_movie_request(&infohash);
        let (older_release_generation, older_inspection_generation) = state
            .begin_inspection(&request)
            .expect("older Movie inspection must begin");
        let current_generation = RefCell::new(None);
        assert_eq!(
            inspect_yts_movie_torrent_with(
                &state,
                older_release_generation,
                older_inspection_generation,
                request.clone(),
                |_| {
                    current_generation.replace(Some(
                        state
                            .begin_inspection(&request)
                            .expect("current Movie inspection must begin"),
                    ));
                    Ok(ArtifactResponse {
                        status: 200,
                        redirect_url: None,
                        body: bytes.clone(),
                    })
                },
            ),
            Err(MOVIE_TORRENT_STALE)
        );

        let (current_release_generation, current_inspection_generation) = current_generation
            .into_inner()
            .expect("current Movie inspection generation must be recorded");
        assert!(inspect_yts_movie_torrent_with(
            &state,
            current_release_generation,
            current_inspection_generation,
            request,
            |_| Ok(ArtifactResponse {
                status: 200,
                redirect_url: None,
                body: bytes.clone(),
            }),
        )
        .is_ok());
    }

    #[test]
    fn movie_inspection_verifies_and_saves_only_the_exact_cached_bytes() {
        let bytes = multi_file_torrent();
        let infohash = fixture_infohash(&bytes);
        let state = movie_state_with_release(&infohash);
        let request = trusted_movie_request(&infohash);
        let (release_generation, inspection_generation) = state
            .begin_inspection(&request)
            .expect("exact Movie inspection must begin");
        let response = inspect_yts_movie_torrent_with(
            &state,
            release_generation,
            inspection_generation,
            request.clone(),
            |url| {
                assert_eq!(
                    url,
                    format!(
                        "https://yts.mx/torrent/download/{}",
                        infohash.to_ascii_uppercase()
                    )
                );
                Ok(ArtifactResponse {
                    status: 200,
                    redirect_url: None,
                    body: bytes.clone(),
                })
            },
        )
        .expect("exact YTS artifact must inspect");
        assert_eq!(response[2], infohash);
        assert_eq!(response[4], "Folder/Part  1 — 映画.mkv");
        let source = state
            .verified_download_source(&response[0], &[1])
            .expect("current Movie selection must resolve only from native state");
        assert!(source.code.is_empty());
        assert_eq!(source.release_name, request.tmdb_title);
        assert_eq!(source.infohash, infohash);
        assert_eq!(
            source.movie_identity,
            Some(MovieDownloadIdentity::from(&request))
        );
        assert_eq!(
            source.selected_files,
            vec![VerifiedDownloadFile {
                file_id: 1,
                path: "Folder/特別版  B.mp4".to_owned(),
                size: 7,
            }]
        );
        assert_eq!(
            state.verified_download_source("vr-1-1-123", &[1]),
            Err(VerifiedDownloadSourceError::Context)
        );
        assert_eq!(
            save_verified_movie_torrent_with(
                &state,
                &response[0],
                |_| None,
                |_, _| { unreachable!("cancelled Save must not write") }
            ),
            Ok(false)
        );

        let destination = std::env::temp_dir().join(format!(
            "auto-video-movie-torrent-{}-{}.torrent",
            std::process::id(),
            infohash
        ));
        let _ = fs::remove_file(&destination);
        assert_eq!(
            save_verified_movie_torrent_with(
                &state,
                &response[0],
                |_| Some(destination.clone()),
                write_new_torrent_file,
            ),
            Ok(true)
        );
        assert_eq!(
            fs::read(&destination).expect("saved torrent must exist"),
            bytes
        );
        assert_eq!(
            save_verified_movie_torrent_with(
                &state,
                &response[0],
                |_| Some(destination.clone()),
                write_new_torrent_file,
            ),
            Err(MOVIE_TORRENT_SAVE_FAILED)
        );
        assert_eq!(
            fs::read(&destination).expect("existing torrent must remain"),
            bytes
        );
        assert_eq!(
            save_verified_movie_torrent_with(
                &state,
                "vr-1-1-123",
                |_| Some(destination.clone()),
                write_new_torrent_file,
            ),
            Err(MOVIE_TORRENT_STALE)
        );
        fs::remove_file(destination).expect("fixture torrent must be removable");
    }
}
