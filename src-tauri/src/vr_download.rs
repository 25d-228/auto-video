use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    num::NonZeroU32,
    path::{Component, Path, PathBuf},
    sync::{mpsc, Arc, Mutex},
    time::Duration,
};

use anyhow::{anyhow, Context};
use librqbit::{
    limits::LimitsConfig,
    storage::{BoxStorageFactory, StorageFactory, StorageFactoryExt, TorrentStorage},
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ManagedTorrent, PeerConnectionOptions,
    Session, SessionOptions, TorrentStatsState,
};
use unicode_normalization::UnicodeNormalization;

use crate::tv_library::parse_tv_relative_identity;
use crate::tv_release::{TvDownloadIdentity, TvReleaseState};
use crate::vr_torrent::{
    adult_media_name_matches_product_code, hex_sha1, media_name_matches_product_code,
    revalidate_persisted_download_source, revalidate_persisted_movie_download_source,
    revalidate_persisted_tv_download_source, AdultTorrentState, MovieDownloadIdentity,
    MovieTorrentState, TvTorrentState, VerifiedDownloadFile, VerifiedDownloadSource,
    VerifiedDownloadSourceError, VrTorrentState,
};

pub const VR_DOWNLOAD_ACTION_INVALID: &str = "vr_download_action_invalid";
pub const VR_DOWNLOAD_CONTEXT_INVALID: &str = "vr_download_context_invalid";
pub const VR_DOWNLOAD_DESTINATION_CONFLICT: &str = "vr_download_destination_conflict";
pub const VR_DOWNLOAD_DUPLICATE: &str = "vr_download_duplicate";
pub const VR_DOWNLOAD_FAILED: &str = "vr_download_failed";
pub const VR_DOWNLOAD_LIMIT_APPLY_FAILED: &str = "vr_download_limit_apply_failed";
pub const VR_DOWNLOAD_LIMIT_INVALID: &str = "vr_download_limit_invalid";
pub const VR_DOWNLOAD_LIMIT_STORAGE_FAILED: &str = "vr_download_limit_storage_failed";
pub const VR_DOWNLOAD_LIMIT_UNAVAILABLE: &str = "vr_download_limit_unavailable";
pub const VR_DOWNLOAD_PERSISTENCE_FAILED: &str = "vr_download_persistence_failed";
pub const VR_DOWNLOAD_STALE: &str = "vr_download_stale";
pub const VR_DOWNLOAD_CLEANUP_FAILED: &str = "vr_download_cleanup_failed";
pub const VR_FOLDER_STORAGE_FAILED: &str = "vr_folder_storage_failed";
pub const VR_FOLDER_UNAVAILABLE: &str = "vr_folder_unavailable";
pub const VR_ORGANIZATION_CONFLICT: &str = "vr_organization_conflict";
pub const VR_ORGANIZATION_FAILED: &str = "vr_organization_failed";
pub const VR_ORGANIZATION_INELIGIBLE: &str = "vr_organization_ineligible";
pub const VR_ORGANIZATION_STALE: &str = "vr_organization_stale";

const PERSISTENCE_HEADER: &[u8] = b"AUTO_VIDEO_DOWNLOADS_V2\n";
const LEGACY_PERSISTENCE_HEADER: &[u8] = b"AUTO_VIDEO_VR_DOWNLOADS_V1\n";
const ORGANIZATION_RECOVERY_HEADER: &[u8] = b"AUTO_VIDEO_ORGANIZATION_V2\n";
const LEGACY_ORGANIZATION_RECOVERY_HEADER: &[u8] = b"AUTO_VIDEO_VR_ORGANIZATION_V1\n";
const TERMINAL_RECOVERY_HEADER: &[u8] = b"AUTO_VIDEO_TRANSFER_TERMINAL_V1\n";
const CLEANUP_RECOVERY_HEADER: &[u8] = b"AUTO_VIDEO_WINDOWS_TRANSFER_CLEANUP_V1\n";
#[cfg(target_os = "macos")]
const MACOS_CLEANUP_MUTATION_HEADER: &[u8] = b"AUTO_VIDEO_MACOS_TRANSFER_CLEANUP_MUTATION_V1\n";
const ORGANIZATION_RECOVERY_PREFIX: &str = ".auto-video-organization-";
const ORGANIZATION_RECOVERY_SUFFIX: &str = ".recovery";
const ORGANIZATION_RECOVERY_SUCCESSOR_SUFFIX: &str = ".recovery.next";
const TERMINAL_RECOVERY_PREFIX: &str = ".auto-video-transfer-terminal-";
const TERMINAL_RECOVERY_SUFFIX: &str = ".recovery";
const CLEANUP_RECOVERY_PREFIX: &str = ".auto-video-windows-transfer-cleanup-";
#[cfg(target_os = "macos")]
const MACOS_CLEANUP_MUTATION_PREFIX: &str = ".auto-video-macos-transfer-cleanup-mutation-";
const PERSISTENCE_REPLACEMENT_SUFFIX: &str = ".next";
const TERMINAL_RECOVERY_DIRECTORY: &str = ".auto-video-transfer-terminal-recovery";
const CLEANUP_RECOVERY_DIRECTORY: &str = ".auto-video-windows-transfer-cleanup-recovery";
const MAX_PERSISTENCE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PERSISTED_TRANSFERS: usize = 100;
const MAX_SELECTED_FILES: usize = 100_000;
const BYTES_PER_MIB: u32 = 1024 * 1024;
const MAX_DOWNLOAD_LIMIT_MIB_PER_SECOND: u32 = u32::MAX / BYTES_PER_MIB;
const DOWNLOAD_LIMIT_UNLIMITED: &str = "unlimited\n";
const TV_METADATA_TRACKERS: [&str; 3] = [
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://open.stealth.si:80/announce",
    "https://tracker.tamersunion.org:443/announce",
];
// A bounded request prevents an unavailable metadata swarm from holding the inspection open.
const TV_METADATA_TIMEOUT: Duration = Duration::from_secs(45);

fn is_supported_transfer_media(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("mp4") || extension.eq_ignore_ascii_case("mkv")
        })
}

type ManagedTorrentHandle = Arc<ManagedTorrent>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransferCategory {
    Adult,
    Movie,
    Tv,
    Vr,
}

impl TransferCategory {
    fn as_str(self) -> &'static str {
        match self {
            Self::Adult => "adult",
            Self::Movie => "movie",
            Self::Tv => "tv",
            Self::Vr => "vr",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "adult" => Some(Self::Adult),
            "movie" => Some(Self::Movie),
            "tv" => Some(Self::Tv),
            "vr" => Some(Self::Vr),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransferState {
    Queued,
    Downloading,
    Paused,
    Completed,
    Cancelled,
    Offline,
    Failed,
    Cleanup,
}

impl TransferState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Downloading => "downloading",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Offline => "offline",
            Self::Failed => "failed",
            Self::Cleanup => "cleanup",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "queued" => Some(Self::Queued),
            "downloading" => Some(Self::Downloading),
            "paused" => Some(Self::Paused),
            "completed" => Some(Self::Completed),
            "cancelled" => Some(Self::Cancelled),
            "offline" => Some(Self::Offline),
            "failed" => Some(Self::Failed),
            "cleanup" => Some(Self::Cleanup),
            _ => None,
        }
    }

    fn is_active(self) -> bool {
        matches!(self, Self::Queued | Self::Downloading | Self::Paused)
    }

    fn can_dismiss(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Cancelled | Self::Offline | Self::Failed
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransferAction {
    Pause,
    Resume,
    Cancel,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum OrganizationState {
    #[default]
    None,
    Organized,
    Attention,
}

impl OrganizationState {
    fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Organized => "organized",
            Self::Attention => "attention",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Self::None),
            "organized" => Some(Self::Organized),
            "attention" => Some(Self::Attention),
            _ => None,
        }
    }
}

#[derive(Clone)]
struct TransferRecord {
    transfer_id: String,
    category: TransferCategory,
    code: String,
    release_name: String,
    movie_identity: Option<Box<MovieDownloadIdentity>>,
    tv_identity: Option<Box<TvDownloadIdentity>>,
    infohash: String,
    metainfo: Vec<u8>,
    selected_files: Vec<VerifiedDownloadFile>,
    destination: PathBuf,
    fingerprints: Vec<String>,
    current_paths: Vec<String>,
    organization_state: OrganizationState,
    state: TransferState,
    downloaded_bytes: u64,
    boundary_segments: Arc<Mutex<BTreeMap<usize, Vec<SparseSegment>>>>,
    handle: Option<ManagedTorrentHandle>,
    handle_generation: u64,
    terminal_recovery_generation: Option<u64>,
    pending_action: Option<TransferAction>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CleanupFileState {
    Present,
    Deleted,
    AbsentBeforeCleanup,
}

impl CleanupFileState {
    #[cfg(any(target_os = "macos", target_os = "windows", test))]
    fn as_str(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Deleted => "deleted",
            Self::AbsentBeforeCleanup => "absent",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "present" => Some(Self::Present),
            "deleted" => Some(Self::Deleted),
            "absent" => Some(Self::AbsentBeforeCleanup),
            _ => None,
        }
    }
}

#[derive(Clone)]
struct CleanupRecovery {
    record: TransferRecord,
    files: Vec<CleanupFileState>,
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CleanupDeletionOutcome {
    TargetAbsent,
    #[cfg(target_os = "macos")]
    ReplacementPreserved,
}

#[cfg(test)]
impl From<()> for CleanupDeletionOutcome {
    fn from((): ()) -> Self {
        Self::TargetAbsent
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
enum CleanupReconciliation {
    Continue,
    #[cfg(target_os = "macos")]
    DeletionCompleted,
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
impl CleanupReconciliation {
    fn deletion_completed(self) -> bool {
        #[cfg(target_os = "macos")]
        {
            matches!(self, Self::DeletionCompleted)
        }
        #[cfg(not(target_os = "macos"))]
        {
            match self {
                Self::Continue => false,
            }
        }
    }
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MacosCleanupMutationPhase {
    StagingCreationPrepared,
    StagingCreated,
    ExchangePrepared,
    Exchanged,
    ExactDeletionPrepared,
    ExactDeleted,
    StagingCleanupPrepared,
    RollbackExchangePrepared,
    RolledBack,
    RollbackStagingCleanupPrepared,
}

#[cfg(target_os = "macos")]
impl MacosCleanupMutationPhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::StagingCreationPrepared => "staging-creation-prepared",
            Self::StagingCreated => "staging-created",
            Self::ExchangePrepared => "exchange-prepared",
            Self::Exchanged => "exchanged",
            Self::ExactDeletionPrepared => "exact-deletion-prepared",
            Self::ExactDeleted => "exact-deleted",
            Self::StagingCleanupPrepared => "staging-cleanup-prepared",
            Self::RollbackExchangePrepared => "rollback-exchange-prepared",
            Self::RolledBack => "rolled-back",
            Self::RollbackStagingCleanupPrepared => "rollback-staging-cleanup-prepared",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "staging-creation-prepared" => Some(Self::StagingCreationPrepared),
            "staging-created" => Some(Self::StagingCreated),
            "exchange-prepared" => Some(Self::ExchangePrepared),
            "exchanged" => Some(Self::Exchanged),
            "exact-deletion-prepared" => Some(Self::ExactDeletionPrepared),
            "exact-deleted" => Some(Self::ExactDeleted),
            "staging-cleanup-prepared" => Some(Self::StagingCleanupPrepared),
            "rollback-exchange-prepared" => Some(Self::RollbackExchangePrepared),
            "rolled-back" => Some(Self::RolledBack),
            "rollback-staging-cleanup-prepared" => Some(Self::RollbackStagingCleanupPrepared),
            _ => None,
        }
    }
}

#[cfg(target_os = "macos")]
#[derive(Clone)]
struct MacosCleanupMutation {
    record: TransferRecord,
    selected_index: usize,
    target_path: PathBuf,
    staging_path: PathBuf,
    expected_fingerprint: String,
    staging_token: String,
    phase: MacosCleanupMutationPhase,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum OrganizationEntryKind {
    Move,
    MediaUnchanged,
    NonMediaUnchanged,
}

impl OrganizationEntryKind {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Move => "move",
            Self::MediaUnchanged => "media-unchanged",
            Self::NonMediaUnchanged => "non-media-unchanged",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OrganizationEntry {
    selected_index: usize,
    kind: OrganizationEntryKind,
    source_relative: String,
    destination_relative: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OrganizationPlan {
    plan_id: String,
    generation: u64,
    transfer_id: String,
    category: TransferCategory,
    identity: String,
    entries: Vec<OrganizationEntry>,
}

impl TransferRecord {
    fn selected_total(&self) -> u64 {
        self.selected_files.iter().map(|file| file.size).sum()
    }

    fn selected_file_ids(&self) -> Vec<usize> {
        self.selected_files
            .iter()
            .map(|file| file.file_id)
            .collect()
    }
}

struct CorruptTransferRecord {
    transfer_id: String,
    category: Option<TransferCategory>,
    code: String,
    release_name: String,
    raw_line: Vec<u8>,
}

enum StoredTransfer {
    Valid(TransferRecord),
    Corrupt(CorruptTransferRecord),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum DownloadLimitState {
    #[default]
    Unloaded,
    Loaded(Option<NonZeroU32>),
}

#[derive(Default)]
struct VrDownloadContext {
    future_folder: Option<PathBuf>,
    adult_future_folder: Option<PathBuf>,
    movie_future_folder: Option<PathBuf>,
    tv_future_folder: Option<PathBuf>,
    session: Option<Arc<Session>>,
    session_starting: bool,
    download_limit: DownloadLimitState,
    transfers_loaded: bool,
    transfers_loading: bool,
    transfers: Vec<StoredTransfer>,
    persistence_path: Option<PathBuf>,
    organization_generation: u64,
    organization_plan: Option<OrganizationPlan>,
    #[cfg(any(target_os = "macos", target_os = "windows", test))]
    cleanup_transfer_id: Option<String>,
}

#[derive(Clone, Default)]
pub struct VrDownloadState(Arc<Mutex<VrDownloadContext>>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VrLibraryTrashOwnershipError {
    Owned,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TvMetainfoAcquisitionError {
    LocalPending,
    LocalUnavailable,
    Network,
    NoMetadataSource,
    Timeout,
}

fn invalidate_organization_plan(context: &mut VrDownloadContext) {
    context.organization_generation = context.organization_generation.wrapping_add(1);
    context.organization_plan = None;
}

fn configured_folder(context: &VrDownloadContext, category: TransferCategory) -> Option<&Path> {
    match category {
        TransferCategory::Adult => context.adult_future_folder.as_deref(),
        TransferCategory::Movie => context.movie_future_folder.as_deref(),
        TransferCategory::Tv => context.tv_future_folder.as_deref(),
        TransferCategory::Vr => context.future_folder.as_deref(),
    }
}

pub(crate) fn configured_vr_folder(
    state: &VrDownloadState,
) -> Result<Option<PathBuf>, &'static str> {
    state
        .0
        .lock()
        .map_err(|_| VR_FOLDER_STORAGE_FAILED)
        .map(|context| context.future_folder.clone())
}

pub(crate) fn with_configured_vr_folder<T>(
    state: &VrDownloadState,
    operation: impl FnOnce(Option<&Path>) -> T,
) -> Result<T, &'static str> {
    let context = state.0.lock().map_err(|_| VR_FOLDER_STORAGE_FAILED)?;
    Ok(operation(context.future_folder.as_deref()))
}

fn with_unowned_library_path<T>(
    state: &VrDownloadState,
    requested_path: &Path,
    category: TransferCategory,
    operation: impl FnOnce(Option<&Path>) -> T,
) -> Result<T, VrLibraryTrashOwnershipError> {
    let context = state
        .0
        .lock()
        .map_err(|_| VrLibraryTrashOwnershipError::Unavailable)?;
    if !context.transfers_loaded || context.transfers_loading {
        return Err(VrLibraryTrashOwnershipError::Unavailable);
    }
    for transfer in &context.transfers {
        match transfer {
            StoredTransfer::Valid(record) => {
                if transfer_record_owns_path(record, requested_path)? {
                    return Err(VrLibraryTrashOwnershipError::Owned);
                }
            }
            StoredTransfer::Corrupt(_) => {
                return Err(VrLibraryTrashOwnershipError::Unavailable);
            }
        }
    }
    if organization_plan_owns_path(&context, requested_path)? {
        return Err(VrLibraryTrashOwnershipError::Owned);
    }
    let persistence_path = context
        .persistence_path
        .as_deref()
        .ok_or(VrLibraryTrashOwnershipError::Unavailable)?;
    if durable_terminal_recovery_owns_path(persistence_path, requested_path)? {
        return Err(VrLibraryTrashOwnershipError::Owned);
    }
    if durable_cleanup_recovery_owns_path(persistence_path, requested_path)? {
        return Err(VrLibraryTrashOwnershipError::Owned);
    }
    let folder = configured_folder(&context, category);
    if let Some(folder) = folder {
        let folder_is_available = fs::canonicalize(folder).ok().as_deref() == Some(folder)
            && fs::metadata(folder).is_ok_and(|metadata| metadata.is_dir());
        if folder_is_available && durable_organization_recovery_owns_path(folder, requested_path)? {
            return Err(VrLibraryTrashOwnershipError::Owned);
        }
    }
    Ok(operation(folder))
}

pub(crate) fn with_unowned_vr_library_path<T>(
    state: &VrDownloadState,
    requested_path: &Path,
    operation: impl FnOnce(Option<&Path>) -> T,
) -> Result<T, VrLibraryTrashOwnershipError> {
    with_unowned_library_path(state, requested_path, TransferCategory::Vr, operation)
}

pub(crate) fn with_unowned_adult_library_path<T>(
    state: &VrDownloadState,
    requested_path: &Path,
    operation: impl FnOnce(Option<&Path>) -> T,
) -> Result<T, VrLibraryTrashOwnershipError> {
    with_unowned_library_path(state, requested_path, TransferCategory::Adult, operation)
}

#[cfg(test)]
pub(crate) fn prepare_unowned_library_paths_for_test(
    state: &VrDownloadState,
    persistence_path: PathBuf,
) {
    let mut context = state.0.lock().expect("download test state must lock");
    context.transfers_loaded = true;
    context.persistence_path = Some(persistence_path);
}

pub(crate) fn with_unowned_tv_library_path<T>(
    state: &VrDownloadState,
    requested_path: &Path,
    operation: impl FnOnce(Option<&Path>) -> T,
) -> Result<T, VrLibraryTrashOwnershipError> {
    with_unowned_library_path(state, requested_path, TransferCategory::Tv, operation)
}

pub fn configure_adult_download_folder(
    state: &VrDownloadState,
    folder: Option<PathBuf>,
) -> Result<(), &'static str> {
    let mut context = state.0.lock().map_err(|_| VR_FOLDER_STORAGE_FAILED)?;
    invalidate_organization_plan(&mut context);
    context.adult_future_folder = folder;
    Ok(())
}

pub fn configure_movie_download_folder(
    state: &VrDownloadState,
    folder: Option<PathBuf>,
) -> Result<(), &'static str> {
    let mut context = state.0.lock().map_err(|_| VR_FOLDER_STORAGE_FAILED)?;
    invalidate_organization_plan(&mut context);
    context.movie_future_folder = folder;
    Ok(())
}

pub fn configure_tv_download_folder(
    state: &VrDownloadState,
    folder: Option<PathBuf>,
) -> Result<(), &'static str> {
    let mut context = state.0.lock().map_err(|_| VR_FOLDER_STORAGE_FAILED)?;
    invalidate_organization_plan(&mut context);
    context.tv_future_folder = folder;
    Ok(())
}

pub fn load_vr_folder_file(path: &Path) -> Result<Option<PathBuf>, &'static str> {
    match fs::read_to_string(path) {
        Ok(folder) if !folder.is_empty() => Ok(Some(PathBuf::from(folder))),
        Ok(_) => Err(VR_FOLDER_STORAGE_FAILED),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(VR_FOLDER_STORAGE_FAILED),
    }
}

pub fn save_vr_folder_file(path: &Path, folder: &Path) -> Result<(), &'static str> {
    let folder = folder.to_str().ok_or(VR_FOLDER_STORAGE_FAILED)?;
    let parent = path.parent().ok_or(VR_FOLDER_STORAGE_FAILED)?;
    fs::create_dir_all(parent).map_err(|_| VR_FOLDER_STORAGE_FAILED)?;
    fs::write(path, folder).map_err(|_| VR_FOLDER_STORAGE_FAILED)
}

pub fn clear_vr_folder_file(path: &Path) -> Result<(), &'static str> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(VR_FOLDER_STORAGE_FAILED),
    }
}

pub fn load_vr_folder_with(
    state: &VrDownloadState,
    path: &Path,
) -> Result<Vec<String>, &'static str> {
    let configured = load_vr_folder_file(path)?;
    let mut context = state.0.lock().map_err(|_| VR_FOLDER_STORAGE_FAILED)?;
    invalidate_organization_plan(&mut context);
    context.future_folder.clone_from(&configured);
    let Some(folder) = configured else {
        return Ok(vec!["unconfigured".to_owned()]);
    };
    let folder_text = folder
        .to_str()
        .map(str::to_owned)
        .ok_or(VR_FOLDER_STORAGE_FAILED)?;
    let available = fs::canonicalize(&folder)
        .ok()
        .filter(|canonical| canonical == &folder)
        .and_then(|canonical| fs::metadata(canonical).ok())
        .is_some_and(|metadata| metadata.is_dir());
    Ok(vec![
        if available { "ready" } else { "unavailable" }.to_owned(),
        folder_text,
    ])
}

pub fn set_vr_folder(
    state: &VrDownloadState,
    path: &Path,
    selected_folder: PathBuf,
) -> Result<String, &'static str> {
    let canonical = fs::canonicalize(selected_folder).map_err(|_| VR_FOLDER_UNAVAILABLE)?;
    if !fs::metadata(&canonical)
        .map_err(|_| VR_FOLDER_UNAVAILABLE)?
        .is_dir()
    {
        return Err(VR_FOLDER_UNAVAILABLE);
    }
    let response = canonical
        .to_str()
        .map(str::to_owned)
        .ok_or(VR_FOLDER_UNAVAILABLE)?;
    save_vr_folder_file(path, &canonical)?;
    let mut context = state.0.lock().map_err(|_| VR_FOLDER_STORAGE_FAILED)?;
    invalidate_organization_plan(&mut context);
    context.future_folder = Some(canonical);
    Ok(response)
}

pub fn clear_vr_folder(state: &VrDownloadState, path: &Path) -> Result<(), &'static str> {
    clear_vr_folder_file(path)?;
    let mut context = state.0.lock().map_err(|_| VR_FOLDER_STORAGE_FAILED)?;
    invalidate_organization_plan(&mut context);
    context.future_folder = None;
    Ok(())
}

fn parse_download_limit(mib_per_second: Option<&str>) -> Result<Option<NonZeroU32>, &'static str> {
    let Some(value) = mib_per_second else {
        return Ok(None);
    };
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(VR_DOWNLOAD_LIMIT_INVALID);
    }
    let value = value
        .parse::<u32>()
        .ok()
        .filter(|value| *value <= MAX_DOWNLOAD_LIMIT_MIB_PER_SECOND)
        .and_then(NonZeroU32::new)
        .ok_or(VR_DOWNLOAD_LIMIT_INVALID)?;
    Ok(Some(value))
}

fn read_download_limit(path: &Path) -> Result<Option<NonZeroU32>, &'static str> {
    match fs::read_to_string(path) {
        Ok(value) if value == DOWNLOAD_LIMIT_UNLIMITED => Ok(None),
        Ok(value) => parse_download_limit(Some(
            value.strip_suffix('\n').ok_or(VR_DOWNLOAD_LIMIT_INVALID)?,
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(VR_DOWNLOAD_LIMIT_STORAGE_FAILED),
    }
}

fn write_download_limit(
    path: &Path,
    mib_per_second: Option<NonZeroU32>,
) -> Result<(), &'static str> {
    let parent = path.parent().ok_or(VR_DOWNLOAD_LIMIT_STORAGE_FAILED)?;
    fs::create_dir_all(parent).map_err(|_| VR_DOWNLOAD_LIMIT_STORAGE_FAILED)?;
    let value = mib_per_second
        .map(|value| format!("{}\n", value.get()))
        .unwrap_or_else(|| DOWNLOAD_LIMIT_UNLIMITED.to_owned());
    fs::write(path, value).map_err(|_| VR_DOWNLOAD_LIMIT_STORAGE_FAILED)
}

fn download_limit_response(mib_per_second: Option<NonZeroU32>) -> Vec<String> {
    match mib_per_second {
        Some(value) => vec!["limited".to_owned(), value.get().to_string()],
        None => vec!["unlimited".to_owned()],
    }
}

fn download_limit_bytes_per_second(mib_per_second: Option<NonZeroU32>) -> Option<NonZeroU32> {
    mib_per_second.and_then(|value| NonZeroU32::new(value.get() * BYTES_PER_MIB))
}

fn apply_download_limit(
    session: Option<&Arc<Session>>,
    bytes_per_second: Option<NonZeroU32>,
) -> Result<(), ()> {
    if let Some(session) = session {
        session.ratelimits.set_download_bps(bytes_per_second);
    }
    Ok(())
}

fn load_download_limit_with(
    state: &VrDownloadState,
    path: &Path,
    apply: impl FnOnce(Option<&Arc<Session>>, Option<NonZeroU32>) -> Result<(), ()>,
) -> Result<Vec<String>, &'static str> {
    let mut context = state
        .0
        .lock()
        .map_err(|_| VR_DOWNLOAD_LIMIT_STORAGE_FAILED)?;
    if let DownloadLimitState::Loaded(mib_per_second) = context.download_limit {
        return Ok(download_limit_response(mib_per_second));
    }
    if context.session_starting {
        return Err(VR_DOWNLOAD_LIMIT_APPLY_FAILED);
    }
    let mib_per_second = read_download_limit(path)?;
    apply(
        context.session.as_ref(),
        download_limit_bytes_per_second(mib_per_second),
    )
    .map_err(|_| VR_DOWNLOAD_LIMIT_APPLY_FAILED)?;
    context.download_limit = DownloadLimitState::Loaded(mib_per_second);
    Ok(download_limit_response(mib_per_second))
}

pub fn load_download_limit(
    state: &VrDownloadState,
    path: &Path,
) -> Result<Vec<String>, &'static str> {
    load_download_limit_with(state, path, apply_download_limit)
}

fn save_download_limit_with(
    state: &VrDownloadState,
    path: &Path,
    mib_per_second: Option<&str>,
    mut apply: impl FnMut(Option<&Arc<Session>>, Option<NonZeroU32>) -> Result<(), ()>,
) -> Result<Vec<String>, &'static str> {
    let mib_per_second = parse_download_limit(mib_per_second)?;
    let mut context = state
        .0
        .lock()
        .map_err(|_| VR_DOWNLOAD_LIMIT_STORAGE_FAILED)?;
    let DownloadLimitState::Loaded(previous_limit) = context.download_limit else {
        return Err(VR_DOWNLOAD_LIMIT_UNAVAILABLE);
    };
    if context.session_starting {
        return Err(VR_DOWNLOAD_LIMIT_APPLY_FAILED);
    }
    apply(
        context.session.as_ref(),
        download_limit_bytes_per_second(mib_per_second),
    )
    .map_err(|_| VR_DOWNLOAD_LIMIT_APPLY_FAILED)?;
    if let Err(error) = write_download_limit(path, mib_per_second) {
        apply(
            context.session.as_ref(),
            download_limit_bytes_per_second(previous_limit),
        )
        .map_err(|_| VR_DOWNLOAD_LIMIT_APPLY_FAILED)?;
        return Err(error);
    }
    context.download_limit = DownloadLimitState::Loaded(mib_per_second);
    Ok(download_limit_response(mib_per_second))
}

pub fn save_download_limit(
    state: &VrDownloadState,
    path: &Path,
    mib_per_second: Option<&str>,
) -> Result<Vec<String>, &'static str> {
    save_download_limit_with(state, path, mib_per_second, apply_download_limit)
}

fn map_source_error(error: VerifiedDownloadSourceError) -> &'static str {
    match error {
        VerifiedDownloadSourceError::Selection => VR_DOWNLOAD_CONTEXT_INVALID,
        VerifiedDownloadSourceError::Context | VerifiedDownloadSourceError::Metainfo => {
            VR_DOWNLOAD_STALE
        }
    }
}

fn canonical_destination(folder: &Path) -> Result<PathBuf, &'static str> {
    let canonical = fs::canonicalize(folder).map_err(|_| VR_FOLDER_UNAVAILABLE)?;
    if canonical != folder
        || !fs::metadata(&canonical)
            .map_err(|_| VR_FOLDER_UNAVAILABLE)?
            .is_dir()
    {
        return Err(VR_FOLDER_UNAVAILABLE);
    }
    if canonical.to_str().is_some_and(|value| !value.is_empty()) {
        Ok(canonical)
    } else {
        Err(VR_FOLDER_UNAVAILABLE)
    }
}

fn relative_file_path(path: &str) -> Result<PathBuf, &'static str> {
    let relative = PathBuf::from(path);
    if relative.is_absolute()
        || relative.components().next().is_none()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(VR_DOWNLOAD_CONTEXT_INVALID);
    }
    Ok(relative)
}

fn validate_existing_parents(destination: &Path, relative: &Path) -> Result<(), &'static str> {
    let mut current = destination.to_owned();
    for component in relative.parent().into_iter().flat_map(Path::components) {
        let Component::Normal(component) = component else {
            return Err(VR_DOWNLOAD_CONTEXT_INVALID);
        };
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(VR_DOWNLOAD_DESTINATION_CONFLICT);
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(_) => return Err(VR_FOLDER_UNAVAILABLE),
        }
    }
    Ok(())
}

fn selected_target(
    destination: &Path,
    file: &VerifiedDownloadFile,
) -> Result<PathBuf, &'static str> {
    let relative = relative_file_path(&file.path)?;
    validate_existing_parents(destination, &relative)?;
    let target = destination.join(relative);
    if !target.starts_with(destination) {
        return Err(VR_DOWNLOAD_CONTEXT_INVALID);
    }
    Ok(target)
}

fn current_target(record: &TransferRecord, selected_index: usize) -> Result<PathBuf, &'static str> {
    let relative = record
        .current_paths
        .get(selected_index)
        .ok_or(VR_DOWNLOAD_STALE)
        .and_then(|path| relative_file_path(path))?;
    validate_existing_parents(&record.destination, &relative)?;
    let target = record.destination.join(relative);
    if !target.starts_with(&record.destination) {
        return Err(VR_DOWNLOAD_CONTEXT_INVALID);
    }
    Ok(target)
}

fn validate_new_targets(
    destination: &Path,
    selected_files: &[VerifiedDownloadFile],
) -> Result<(), &'static str> {
    for file in selected_files {
        let target = selected_target(destination, file)?;
        match fs::symlink_metadata(target) {
            Ok(_) => return Err(VR_DOWNLOAD_DESTINATION_CONFLICT),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(VR_FOLDER_UNAVAILABLE),
        }
    }
    Ok(())
}

fn identity_field(identity: &mut Vec<u8>, value: &[u8]) {
    identity.extend_from_slice(&(value.len() as u64).to_be_bytes());
    identity.extend_from_slice(value);
}

fn transfer_identity(
    category: TransferCategory,
    source: &VerifiedDownloadSource,
    destination: &Path,
) -> String {
    let mut identity = Vec::new();
    identity_field(&mut identity, category.as_str().as_bytes());
    identity_field(&mut identity, source.code.as_bytes());
    identity_field(&mut identity, source.release_name.as_bytes());
    identity_field(&mut identity, source.infohash.as_bytes());
    identity_field(&mut identity, &source.bytes);
    if let Some(movie_identity) = &source.movie_identity {
        identity_field(&mut identity, &encode_movie_identity(movie_identity));
    }
    if let Some(tv_identity) = &source.tv_identity {
        identity_field(&mut identity, &encode_tv_identity(tv_identity));
    }
    identity_field(&mut identity, destination.to_string_lossy().as_bytes());
    for file in &source.selected_files {
        identity.extend_from_slice(&(file.file_id as u64).to_be_bytes());
        identity_field(&mut identity, file.path.as_bytes());
        identity.extend_from_slice(&file.size.to_be_bytes());
    }
    hex_sha1(&identity)
}

fn legacy_vr_transfer_identity(source: &VerifiedDownloadSource, destination: &Path) -> String {
    let mut identity = Vec::new();
    identity_field(&mut identity, source.code.as_bytes());
    identity_field(&mut identity, source.release_name.as_bytes());
    identity_field(&mut identity, source.infohash.as_bytes());
    identity_field(&mut identity, &source.bytes);
    identity_field(&mut identity, destination.to_string_lossy().as_bytes());
    for file in &source.selected_files {
        identity.extend_from_slice(&(file.file_id as u64).to_be_bytes());
        identity_field(&mut identity, file.path.as_bytes());
        identity.extend_from_slice(&file.size.to_be_bytes());
    }
    hex_sha1(&identity)
}

fn transfer_from_source(
    category: TransferCategory,
    source: VerifiedDownloadSource,
    destination: PathBuf,
    state: TransferState,
) -> TransferRecord {
    let current_paths = source
        .selected_files
        .iter()
        .map(|file| file.path.clone())
        .collect();
    TransferRecord {
        transfer_id: transfer_identity(category, &source, &destination),
        category,
        code: source.code,
        release_name: source.release_name,
        movie_identity: source.movie_identity.map(Box::new),
        tv_identity: source.tv_identity.map(Box::new),
        infohash: source.infohash,
        metainfo: source.bytes,
        selected_files: source.selected_files,
        destination,
        fingerprints: Vec::new(),
        current_paths,
        organization_state: OrganizationState::None,
        state,
        downloaded_bytes: 0,
        boundary_segments: Arc::new(Mutex::new(BTreeMap::new())),
        handle: None,
        handle_generation: 0,
        terminal_recovery_generation: None,
        pending_action: None,
    }
}

fn checked_selected_total(files: &[VerifiedDownloadFile]) -> Result<u64, &'static str> {
    files.iter().try_fold(0_u64, |total, file| {
        total
            .checked_add(file.size)
            .ok_or(VR_DOWNLOAD_CONTEXT_INVALID)
    })
}

fn encode_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(DIGITS[(byte >> 4) as usize] as char);
        encoded.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn decode_hex(value: &[u8]) -> Option<Vec<u8>> {
    fn digit(value: u8) -> Option<u8> {
        match value {
            b'0'..=b'9' => Some(value - b'0'),
            b'a'..=b'f' => Some(value - b'a' + 10),
            _ => None,
        }
    }

    if !value.len().is_multiple_of(2) {
        return None;
    }
    value
        .chunks_exact(2)
        .map(|pair| Some((digit(pair[0])? << 4) | digit(pair[1])?))
        .collect()
}

fn decode_text(value: &[u8]) -> Option<String> {
    String::from_utf8(decode_hex(value)?).ok()
}

fn encode_movie_identity(identity: &MovieDownloadIdentity) -> Vec<u8> {
    let mut encoded = Vec::new();
    for value in [
        identity.tmdb_movie_id.to_string(),
        identity.tmdb_title.clone(),
        identity.release_date.clone().unwrap_or_default(),
        identity.imdb_id.clone(),
        identity.provider_movie_id.to_string(),
        identity.provider_title.clone().unwrap_or_default(),
        identity.provider_year.clone().unwrap_or_default(),
        identity.row_id.clone(),
        identity.quality.clone().unwrap_or_default(),
        identity.type_label.clone().unwrap_or_default(),
        identity.video_codec.clone().unwrap_or_default(),
        identity.size.clone().unwrap_or_default(),
        identity.size_bytes.clone().unwrap_or_default(),
        identity.seeds.clone().unwrap_or_default(),
        identity.peers.clone().unwrap_or_default(),
        identity.expected_infohash.clone(),
        identity.torrent_url.clone(),
    ] {
        identity_field(&mut encoded, value.as_bytes());
    }
    encoded
}

fn decode_movie_identity(value: &[u8]) -> Option<MovieDownloadIdentity> {
    fn next_field<'a>(value: &'a [u8], position: &mut usize) -> Option<&'a [u8]> {
        let length_end = position.checked_add(8)?;
        let length = u64::from_be_bytes(value.get(*position..length_end)?.try_into().ok()?);
        let length = usize::try_from(length).ok()?;
        let field_end = length_end.checked_add(length)?;
        let field = value.get(length_end..field_end)?;
        *position = field_end;
        Some(field)
    }

    fn next_text(value: &[u8], position: &mut usize) -> Option<String> {
        String::from_utf8(next_field(value, position)?.to_vec()).ok()
    }

    fn optional(value: String) -> Option<String> {
        (!value.is_empty()).then_some(value)
    }

    let mut position = 0;
    let identity = MovieDownloadIdentity {
        tmdb_movie_id: next_text(value, &mut position)?.parse().ok()?,
        tmdb_title: next_text(value, &mut position)?,
        release_date: optional(next_text(value, &mut position)?),
        imdb_id: next_text(value, &mut position)?,
        provider_movie_id: next_text(value, &mut position)?.parse().ok()?,
        provider_title: optional(next_text(value, &mut position)?),
        provider_year: optional(next_text(value, &mut position)?),
        row_id: next_text(value, &mut position)?,
        quality: optional(next_text(value, &mut position)?),
        type_label: optional(next_text(value, &mut position)?),
        video_codec: optional(next_text(value, &mut position)?),
        size: optional(next_text(value, &mut position)?),
        size_bytes: optional(next_text(value, &mut position)?),
        seeds: optional(next_text(value, &mut position)?),
        peers: optional(next_text(value, &mut position)?),
        expected_infohash: next_text(value, &mut position)?,
        torrent_url: next_text(value, &mut position)?,
    };
    (position == value.len()).then_some(identity)
}

fn encode_tv_identity(identity: &TvDownloadIdentity) -> Vec<u8> {
    let mut encoded = Vec::new();
    for value in [
        identity.tmdb_tv_id.to_string(),
        identity.show_name.clone(),
        identity.provider_season_id.to_string(),
        identity.season_number.to_string(),
        identity.provider_episode_id.to_string(),
        identity.episode_number.to_string(),
        identity.episode_name.clone(),
        identity.imdb_id.clone(),
        identity.provider_item_id.clone(),
        identity.category.clone(),
        identity.release_name.clone(),
        identity.infohash.clone(),
    ] {
        identity_field(&mut encoded, value.as_bytes());
    }
    encoded
}

fn decode_tv_identity(value: &[u8]) -> Option<TvDownloadIdentity> {
    fn next_field<'a>(value: &'a [u8], position: &mut usize) -> Option<&'a [u8]> {
        let length_end = position.checked_add(8)?;
        let length = u64::from_be_bytes(value.get(*position..length_end)?.try_into().ok()?);
        let length = usize::try_from(length).ok()?;
        let field_end = length_end.checked_add(length)?;
        let field = value.get(length_end..field_end)?;
        *position = field_end;
        Some(field)
    }

    fn next_text(value: &[u8], position: &mut usize) -> Option<String> {
        String::from_utf8(next_field(value, position)?.to_vec()).ok()
    }

    let mut position = 0;
    let identity = TvDownloadIdentity {
        tmdb_tv_id: next_text(value, &mut position)?.parse().ok()?,
        show_name: next_text(value, &mut position)?,
        provider_season_id: next_text(value, &mut position)?.parse().ok()?,
        season_number: next_text(value, &mut position)?.parse().ok()?,
        provider_episode_id: next_text(value, &mut position)?.parse().ok()?,
        episode_number: next_text(value, &mut position)?.parse().ok()?,
        episode_name: next_text(value, &mut position)?,
        imdb_id: next_text(value, &mut position)?,
        provider_item_id: next_text(value, &mut position)?,
        category: next_text(value, &mut position)?,
        release_name: next_text(value, &mut position)?,
        infohash: next_text(value, &mut position)?,
    };
    (position == value.len()).then_some(identity)
}

fn encoded_selected_ids(record: &TransferRecord) -> String {
    record
        .selected_files
        .iter()
        .map(|file| file.file_id.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn encoded_fingerprints(record: &TransferRecord) -> String {
    record
        .fingerprints
        .iter()
        .map(|fingerprint| encode_hex(fingerprint.as_bytes()))
        .collect::<Vec<_>>()
        .join(",")
}

fn encoded_paths(paths: &[String]) -> String {
    paths
        .iter()
        .map(|path| encode_hex(path.as_bytes()))
        .collect::<Vec<_>>()
        .join(",")
}

fn encoded_boundary_segments(record: &TransferRecord) -> Result<String, &'static str> {
    let segments = record
        .boundary_segments
        .lock()
        .map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
    Ok(segments
        .iter()
        .flat_map(|(file_id, segments)| {
            segments.iter().map(move |segment| {
                format!(
                    "{file_id}:{}:{}",
                    segment.offset,
                    encode_hex(&segment.bytes)
                )
            })
        })
        .collect::<Vec<_>>()
        .join(";"))
}

fn encode_transfer_state(
    record: &TransferRecord,
    organization_state: OrganizationState,
    current_paths: &[String],
) -> Result<Vec<u8>, &'static str> {
    let mut fields = vec![
        encode_hex(record.transfer_id.as_bytes()),
        encode_hex(record.code.as_bytes()),
        encode_hex(record.release_name.as_bytes()),
        encode_hex(record.infohash.as_bytes()),
        encode_hex(record.destination.to_string_lossy().as_bytes()),
        record.state.as_str().to_owned(),
        encode_hex(&record.metainfo),
        encoded_selected_ids(record),
        encoded_fingerprints(record),
        record.downloaded_bytes.to_string(),
        encoded_boundary_segments(record)?,
        organization_state.as_str().to_owned(),
        encoded_paths(current_paths),
        record.category.as_str().to_owned(),
    ];
    if let Some(identity) = &record.movie_identity {
        fields.push(encode_hex(&encode_movie_identity(identity)));
    } else if let Some(identity) = &record.tv_identity {
        fields.push(encode_hex(&encode_tv_identity(identity)));
    }
    Ok(fields.join("\t").into_bytes())
}

fn encode_transfer(record: &TransferRecord) -> Result<Vec<u8>, &'static str> {
    encode_transfer_state(record, record.organization_state, &record.current_paths)
}

fn parse_selected_ids(value: &[u8]) -> Option<Vec<usize>> {
    let value = std::str::from_utf8(value).ok()?;
    if value.is_empty() {
        return None;
    }
    let ids = value
        .split(',')
        .map(|id| id.parse::<usize>().ok())
        .collect::<Option<Vec<_>>>()?;
    let unique = ids.iter().copied().collect::<BTreeSet<_>>();
    (ids.len() <= MAX_SELECTED_FILES && unique.len() == ids.len()).then_some(ids)
}

fn parse_fingerprints(value: &[u8]) -> Option<Vec<String>> {
    if value.is_empty() {
        return Some(Vec::new());
    }
    value.split(|byte| *byte == b',').map(decode_text).collect()
}

fn parse_current_paths(value: &[u8]) -> Option<Vec<String>> {
    if value.is_empty() {
        return None;
    }
    value.split(|byte| *byte == b',').map(decode_text).collect()
}

fn parse_boundary_segments(value: &[u8]) -> Option<BTreeMap<usize, Vec<SparseSegment>>> {
    let value = std::str::from_utf8(value).ok()?;
    let mut retained = BTreeMap::<usize, Vec<SparseSegment>>::new();
    if value.is_empty() {
        return Some(retained);
    }
    for encoded in value.split(';') {
        let mut fields = encoded.split(':');
        let file_id = fields.next()?.parse::<usize>().ok()?;
        let offset = fields.next()?.parse::<u64>().ok()?;
        let bytes = decode_hex(fields.next()?.as_bytes())?;
        if fields.next().is_some() || bytes.is_empty() {
            return None;
        }
        offset.checked_add(bytes.len() as u64)?;
        let segments = retained.entry(file_id).or_default();
        if let Some(previous) = segments.last() {
            let previous_end = previous.offset.checked_add(previous.bytes.len() as u64)?;
            if previous_end >= offset {
                return None;
            }
        }
        segments.push(SparseSegment { offset, bytes });
    }
    (retained.len() <= MAX_SELECTED_FILES).then_some(retained)
}

fn parse_transfer_line(line: &[u8], allow_legacy_vr: bool) -> Option<TransferRecord> {
    let fields = line.split(|byte| *byte == b'\t').collect::<Vec<_>>();
    let (fields, boundary_segments, organization_state, current_paths, category_identity) =
        match fields.as_slice() {
            [transfer_id, code, release_name, infohash, destination, state, metainfo, selected_ids, fingerprints, downloaded_bytes]
                if allow_legacy_vr =>
            {
                (
                    [
                        *transfer_id,
                        *code,
                        *release_name,
                        *infohash,
                        *destination,
                        *state,
                        *metainfo,
                        *selected_ids,
                        *fingerprints,
                        *downloaded_bytes,
                    ],
                    BTreeMap::new(),
                    OrganizationState::None,
                    None,
                    (TransferCategory::Vr, None, None),
                )
            }
            [transfer_id, code, release_name, infohash, destination, state, metainfo, selected_ids, fingerprints, downloaded_bytes, boundary_segments]
                if allow_legacy_vr =>
            {
                (
                    [
                        *transfer_id,
                        *code,
                        *release_name,
                        *infohash,
                        *destination,
                        *state,
                        *metainfo,
                        *selected_ids,
                        *fingerprints,
                        *downloaded_bytes,
                    ],
                    parse_boundary_segments(boundary_segments)?,
                    OrganizationState::None,
                    None,
                    (TransferCategory::Vr, None, None),
                )
            }
            [transfer_id, code, release_name, infohash, destination, state, metainfo, selected_ids, fingerprints, downloaded_bytes, boundary_segments, organization_state, current_paths]
                if allow_legacy_vr =>
            {
                (
                    [
                        *transfer_id,
                        *code,
                        *release_name,
                        *infohash,
                        *destination,
                        *state,
                        *metainfo,
                        *selected_ids,
                        *fingerprints,
                        *downloaded_bytes,
                    ],
                    parse_boundary_segments(boundary_segments)?,
                    OrganizationState::from_str(std::str::from_utf8(organization_state).ok()?)?,
                    Some(parse_current_paths(current_paths)?),
                    (TransferCategory::Vr, None, None),
                )
            }
            [transfer_id, code, release_name, infohash, destination, state, metainfo, selected_ids, fingerprints, downloaded_bytes, boundary_segments, organization_state, current_paths, category] => {
                (
                    [
                        *transfer_id,
                        *code,
                        *release_name,
                        *infohash,
                        *destination,
                        *state,
                        *metainfo,
                        *selected_ids,
                        *fingerprints,
                        *downloaded_bytes,
                    ],
                    parse_boundary_segments(boundary_segments)?,
                    OrganizationState::from_str(std::str::from_utf8(organization_state).ok()?)?,
                    Some(parse_current_paths(current_paths)?),
                    (
                        TransferCategory::from_str(std::str::from_utf8(category).ok()?)?,
                        None,
                        None,
                    ),
                )
            }
            [transfer_id, code, release_name, infohash, destination, state, metainfo, selected_ids, fingerprints, downloaded_bytes, boundary_segments, organization_state, current_paths, category, movie_identity]
                if std::str::from_utf8(category).ok()? == TransferCategory::Movie.as_str() =>
            {
                (
                    [
                        *transfer_id,
                        *code,
                        *release_name,
                        *infohash,
                        *destination,
                        *state,
                        *metainfo,
                        *selected_ids,
                        *fingerprints,
                        *downloaded_bytes,
                    ],
                    parse_boundary_segments(boundary_segments)?,
                    OrganizationState::from_str(std::str::from_utf8(organization_state).ok()?)?,
                    Some(parse_current_paths(current_paths)?),
                    (
                        TransferCategory::Movie,
                        Some(Box::new(decode_movie_identity(&decode_hex(
                            movie_identity,
                        )?)?)),
                        None,
                    ),
                )
            }
            [transfer_id, code, release_name, infohash, destination, state, metainfo, selected_ids, fingerprints, downloaded_bytes, boundary_segments, organization_state, current_paths, category, tv_identity]
                if std::str::from_utf8(category).ok()? == TransferCategory::Tv.as_str() =>
            {
                (
                    [
                        *transfer_id,
                        *code,
                        *release_name,
                        *infohash,
                        *destination,
                        *state,
                        *metainfo,
                        *selected_ids,
                        *fingerprints,
                        *downloaded_bytes,
                    ],
                    parse_boundary_segments(boundary_segments)?,
                    OrganizationState::from_str(std::str::from_utf8(organization_state).ok()?)?,
                    Some(parse_current_paths(current_paths)?),
                    (
                        TransferCategory::Tv,
                        None,
                        Some(Box::new(decode_tv_identity(&decode_hex(tv_identity)?)?)),
                    ),
                )
            }
            _ => return None,
        };
    let (category, movie_identity, tv_identity) = category_identity;
    let [transfer_id, code, release_name, infohash, destination, state, metainfo, selected_ids, fingerprints, downloaded_bytes] =
        fields;
    let transfer_id = decode_text(transfer_id)?;
    let code = decode_text(code)?;
    let release_name = decode_text(release_name)?;
    let infohash = decode_text(infohash)?;
    let destination = PathBuf::from(decode_text(destination)?);
    let state = TransferState::from_str(std::str::from_utf8(state).ok()?)?;
    let metainfo = decode_hex(metainfo)?;
    let selected_ids = parse_selected_ids(selected_ids)?;
    let fingerprints = parse_fingerprints(fingerprints)?;
    let downloaded_bytes = std::str::from_utf8(downloaded_bytes)
        .ok()?
        .parse::<u64>()
        .ok()?;
    if transfer_id.is_empty()
        || release_name.trim().is_empty()
        || destination.to_str().is_none()
        || metainfo.len() > 2 * 1024 * 1024
        || (!fingerprints.is_empty() && fingerprints.len() != selected_ids.len())
    {
        return None;
    }

    let source = match category {
        TransferCategory::Movie => {
            let identity = movie_identity.as_ref()?;
            if tv_identity.is_some() || !code.is_empty() || release_name != identity.tmdb_title {
                return None;
            }
            revalidate_persisted_movie_download_source(
                &metainfo,
                identity,
                &infohash,
                &selected_ids,
            )
            .ok()?
        }
        TransferCategory::Tv => {
            let identity = tv_identity.as_ref()?;
            if movie_identity.is_some()
                || !code.is_empty()
                || release_name != identity.release_name
                || infohash != identity.infohash
            {
                return None;
            }
            revalidate_persisted_tv_download_source(&metainfo, identity, &infohash, &selected_ids)
                .ok()?
        }
        TransferCategory::Adult | TransferCategory::Vr => {
            if movie_identity.is_some() || tv_identity.is_some() {
                return None;
            }
            revalidate_persisted_download_source(
                &metainfo,
                &code,
                &release_name,
                &infohash,
                &selected_ids,
            )
            .ok()?
        }
    };
    let current_paths = current_paths.unwrap_or_else(|| {
        source
            .selected_files
            .iter()
            .map(|file| file.path.clone())
            .collect()
    });
    let selected_total = checked_selected_total(&source.selected_files).ok()?;
    let category_identity = transfer_identity(category, &source, &destination);
    let identity_is_valid = category_identity == transfer_id
        || (category == TransferCategory::Vr
            && legacy_vr_transfer_identity(&source, &destination) == transfer_id);
    if !identity_is_valid || downloaded_bytes > selected_total {
        return None;
    }
    if current_paths.len() != source.selected_files.len()
        || current_paths
            .iter()
            .any(|path| relative_file_path(path).is_err())
        || current_paths.iter().collect::<BTreeSet<_>>().len() != current_paths.len()
        || (organization_state == OrganizationState::None
            && current_paths
                .iter()
                .zip(source.selected_files.iter())
                .any(|(current, selected)| current != &selected.path))
        || (organization_state != OrganizationState::None
            && (state != TransferState::Completed
                || downloaded_bytes != selected_total
                || fingerprints.len() != source.selected_files.len()))
    {
        return None;
    }

    let record = TransferRecord {
        transfer_id,
        category,
        code,
        release_name,
        movie_identity,
        tv_identity,
        infohash,
        metainfo,
        selected_files: source.selected_files,
        destination,
        fingerprints,
        current_paths,
        organization_state,
        state,
        downloaded_bytes,
        boundary_segments: Arc::new(Mutex::new(boundary_segments)),
        handle: None,
        handle_generation: 0,
        terminal_recovery_generation: None,
        pending_action: None,
    };
    if matches!(
        record.category,
        TransferCategory::Movie | TransferCategory::Tv
    ) && record.organization_state != OrganizationState::None
    {
        let eligible_media = record
            .selected_files
            .iter()
            .filter(|file| is_supported_transfer_media(Path::new(&file.path)))
            .count();
        if eligible_media == 0 {
            return None;
        }
        for (selected_index, current_path) in record.current_paths.iter().enumerate() {
            let original_path = &record.selected_files[selected_index].path;
            let expected_path =
                organization_destination_relative(&record, selected_index, eligible_media).ok()?;
            let valid = match (record.organization_state, expected_path) {
                (_, None) => current_path == original_path,
                (OrganizationState::Organized, Some(expected)) => current_path == &expected,
                (OrganizationState::Attention, Some(expected)) => {
                    current_path == original_path || current_path == &expected
                }
                (OrganizationState::None, _) => unreachable!(),
            };
            if !valid {
                return None;
            }
        }
    }
    Some(record)
}

fn corrupt_transfer(
    line: &[u8],
    line_number: usize,
    category: Option<TransferCategory>,
) -> CorruptTransferRecord {
    let decoded_line = line
        .strip_prefix(b"!")
        .and_then(decode_hex)
        .unwrap_or_else(|| line.to_vec());
    let fields = decoded_line
        .split(|byte| *byte == b'\t')
        .collect::<Vec<_>>();
    let transfer_id = format!("corrupt-{line_number}-{}", &hex_sha1(&decoded_line)[..12]);
    let code = fields
        .get(1)
        .and_then(|value| decode_text(value))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Unavailable".to_owned());
    let release_name = fields
        .get(2)
        .and_then(|value| decode_text(value))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "Persisted transfer data is corrupt.".to_owned());
    CorruptTransferRecord {
        transfer_id,
        category,
        code,
        release_name,
        raw_line: decoded_line,
    }
}

fn read_persisted_transfers(path: &Path) -> Result<Vec<StoredTransfer>, &'static str> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(VR_DOWNLOAD_PERSISTENCE_FAILED),
    };
    if metadata.len() > MAX_PERSISTENCE_BYTES {
        return Ok(vec![StoredTransfer::Corrupt(corrupt_transfer(
            b"oversized persistence",
            0,
            None,
        ))]);
    }
    let bytes = fs::read(path).map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
    let (body, allow_legacy_vr) = if let Some(body) = bytes.strip_prefix(PERSISTENCE_HEADER) {
        (body, false)
    } else if let Some(body) = bytes.strip_prefix(LEGACY_PERSISTENCE_HEADER) {
        (body, true)
    } else {
        return Ok(vec![StoredTransfer::Corrupt(corrupt_transfer(
            &bytes, 0, None,
        ))]);
    };
    let corrupt_category = allow_legacy_vr.then_some(TransferCategory::Vr);
    let mut transfers = Vec::new();
    let mut transfer_ids = BTreeSet::new();
    for (line_number, line) in body
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .take(MAX_PERSISTED_TRANSFERS)
        .enumerate()
    {
        transfers.push(match parse_transfer_line(line, allow_legacy_vr) {
            Some(record) if allow_legacy_vr && record.category != TransferCategory::Vr => {
                StoredTransfer::Corrupt(corrupt_transfer(line, line_number, corrupt_category))
            }
            Some(record) if transfer_ids.insert(record.transfer_id.clone()) => {
                StoredTransfer::Valid(record)
            }
            None => StoredTransfer::Corrupt(corrupt_transfer(line, line_number, corrupt_category)),
            Some(_) => {
                StoredTransfer::Corrupt(corrupt_transfer(line, line_number, corrupt_category))
            }
        });
    }
    Ok(transfers)
}

fn encoded_persisted_transfers(transfers: &[StoredTransfer]) -> Result<Vec<u8>, &'static str> {
    let mut bytes = PERSISTENCE_HEADER.to_vec();
    for transfer in transfers.iter().take(MAX_PERSISTED_TRANSFERS) {
        match transfer {
            StoredTransfer::Valid(record) => bytes.extend_from_slice(&encode_transfer(record)?),
            StoredTransfer::Corrupt(record) => {
                bytes.push(b'!');
                bytes.extend_from_slice(encode_hex(&record.raw_line).as_bytes());
            }
        }
        bytes.push(b'\n');
    }
    if bytes.len() as u64 > MAX_PERSISTENCE_BYTES {
        return Err(VR_DOWNLOAD_PERSISTENCE_FAILED);
    }
    Ok(bytes)
}

fn path_with_suffix(path: &Path, suffix: &str) -> Result<PathBuf, &'static str> {
    let name = path.file_name().ok_or(VR_DOWNLOAD_PERSISTENCE_FAILED)?;
    let mut replacement = name.to_os_string();
    replacement.push(suffix);
    Ok(path.with_file_name(replacement))
}

fn persistence_replacement_path(path: &Path) -> Result<PathBuf, &'static str> {
    path_with_suffix(path, PERSISTENCE_REPLACEMENT_SUFFIX)
}

fn clear_stale_persistence_replacement(path: &Path) -> Result<(), &'static str> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(VR_DOWNLOAD_PERSISTENCE_FAILED)
        }
        Ok(_) => fs::remove_file(path).map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(VR_DOWNLOAD_PERSISTENCE_FAILED),
    }
}

fn write_replacement_file(path: &Path, bytes: &[u8]) -> Result<(), &'static str> {
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
            .map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)
    })();
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result
}

#[cfg(target_os = "windows")]
#[link(name = "Kernel32")]
extern "system" {
    #[link_name = "MoveFileExW"]
    fn move_file_ex_w(existing_file_name: *const u16, new_file_name: *const u16, flags: u32)
        -> i32;
}

#[cfg(target_os = "windows")]
fn replace_persistence_file(source: &Path, destination: &Path) -> Result<(), &'static str> {
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
    let source = source
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let succeeded = unsafe {
        move_file_ex_w(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if succeeded == 0 {
        Err(VR_DOWNLOAD_PERSISTENCE_FAILED)
    } else {
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
fn replace_persistence_file(source: &Path, destination: &Path) -> Result<(), &'static str> {
    fs::rename(source, destination).map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) {
    if let Some(parent) = path.parent() {
        let _ = File::open(parent).and_then(|directory| directory.sync_all());
    }
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) {}

fn write_persisted_transfers_with(
    path: &Path,
    transfers: &[StoredTransfer],
    mut write_replacement: impl FnMut(&Path, &[u8]) -> Result<(), &'static str>,
    mut replace: impl FnMut(&Path, &Path) -> Result<(), &'static str>,
) -> Result<(), &'static str> {
    let parent = path.parent().ok_or(VR_DOWNLOAD_PERSISTENCE_FAILED)?;
    fs::create_dir_all(parent).map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
    let bytes = encoded_persisted_transfers(transfers)?;
    let replacement = persistence_replacement_path(path)?;
    clear_stale_persistence_replacement(&replacement)?;
    if let Err(error) = write_replacement(&replacement, &bytes) {
        let _ = fs::remove_file(&replacement);
        return Err(error);
    }
    if let Err(error) = replace(&replacement, path) {
        let _ = fs::remove_file(&replacement);
        return Err(error);
    }
    // The replacement file is synced before the atomic rename. Directory syncing is best-effort;
    // every reported error occurs before the previous primary authority is replaced.
    sync_parent_directory(path);
    Ok(())
}

fn write_persisted_transfers(
    path: &Path,
    transfers: &[StoredTransfer],
) -> Result<(), &'static str> {
    write_persisted_transfers_with(
        path,
        transfers,
        write_replacement_file,
        replace_persistence_file,
    )
}

fn terminal_recovery_directory(persistence_path: &Path) -> Result<PathBuf, &'static str> {
    let parent = persistence_path
        .parent()
        .ok_or(VR_DOWNLOAD_PERSISTENCE_FAILED)?;
    Ok(parent.join(TERMINAL_RECOVERY_DIRECTORY))
}

fn terminal_recovery_path(
    persistence_path: &Path,
    record: &TransferRecord,
) -> Result<PathBuf, &'static str> {
    Ok(terminal_recovery_directory(persistence_path)?.join(format!(
        "{TERMINAL_RECOVERY_PREFIX}{}{TERMINAL_RECOVERY_SUFFIX}",
        record.transfer_id
    )))
}

fn encoded_terminal_recovery(
    record: &TransferRecord,
    generation: u64,
) -> Result<Vec<u8>, &'static str> {
    if record.state != TransferState::Failed
        || record.organization_state != OrganizationState::None
        || validate_resume_context(record).is_err()
    {
        return Err(VR_DOWNLOAD_PERSISTENCE_FAILED);
    }
    let encoded_record = encode_transfer(record)?;
    let mut checksum_input = generation.to_be_bytes().to_vec();
    checksum_input.extend_from_slice(&encoded_record);
    let mut bytes = TERMINAL_RECOVERY_HEADER.to_vec();
    bytes.extend_from_slice(generation.to_string().as_bytes());
    bytes.push(b'\n');
    bytes.extend_from_slice(hex_sha1(&checksum_input).as_bytes());
    bytes.push(b'\n');
    bytes.extend_from_slice(&encoded_record);
    bytes.push(b'\n');
    if bytes.len() as u64 > MAX_PERSISTENCE_BYTES {
        return Err(VR_DOWNLOAD_PERSISTENCE_FAILED);
    }
    Ok(bytes)
}

fn write_terminal_recovery(
    persistence_path: &Path,
    record: &TransferRecord,
    generation: u64,
) -> Result<(), &'static str> {
    let directory = terminal_recovery_directory(persistence_path)?;
    fs::create_dir_all(&directory).map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
    let directory_metadata =
        fs::symlink_metadata(&directory).map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
    if directory_metadata.file_type().is_symlink() || !directory_metadata.is_dir() {
        return Err(VR_DOWNLOAD_PERSISTENCE_FAILED);
    }
    let path = terminal_recovery_path(persistence_path, record)?;
    let bytes = encoded_terminal_recovery(record, generation)?;
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink()
                || !metadata.is_file()
                || fs::canonicalize(&path).ok().as_deref() != Some(path.as_path())
                || fs::read(&path).ok().as_deref() != Some(bytes.as_slice())
            {
                return Err(VR_DOWNLOAD_PERSISTENCE_FAILED);
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let result = (|| {
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
                file.write_all(&bytes)
                    .and_then(|()| file.sync_all())
                    .map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)
            })();
            if result.is_err() {
                let _ = fs::remove_file(&path);
            } else {
                sync_parent_directory(&path);
            }
            result
        }
        Err(_) => Err(VR_DOWNLOAD_PERSISTENCE_FAILED),
    }
}

fn parse_terminal_recovery(persistence_path: &Path, path: &Path) -> Option<TransferRecord> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_PERSISTENCE_BYTES
    {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    let fields = bytes
        .strip_prefix(TERMINAL_RECOVERY_HEADER)?
        .split(|byte| *byte == b'\n')
        .collect::<Vec<_>>();
    let [generation, checksum, encoded_record, trailing] = fields.as_slice() else {
        return None;
    };
    if !trailing.is_empty() || checksum.len() != 40 {
        return None;
    }
    let generation = std::str::from_utf8(generation).ok()?.parse::<u64>().ok()?;
    let mut checksum_input = generation.to_be_bytes().to_vec();
    checksum_input.extend_from_slice(encoded_record);
    if checksum != &hex_sha1(&checksum_input).as_bytes() {
        return None;
    }
    let mut record = parse_transfer_line(encoded_record, false)?;
    if record.state != TransferState::Failed
        || record.organization_state != OrganizationState::None
        || terminal_recovery_path(persistence_path, &record)
            .ok()?
            .as_path()
            != path
        || validate_resume_context(&record).is_err()
    {
        return None;
    }
    record.terminal_recovery_generation = Some(generation);
    Some(record)
}

fn terminal_recovery_paths(persistence_path: &Path) -> Result<Vec<PathBuf>, &'static str> {
    let directory = terminal_recovery_directory(persistence_path)?;
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(VR_DOWNLOAD_PERSISTENCE_FAILED),
    };
    let metadata = fs::symlink_metadata(&directory).map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(VR_DOWNLOAD_PERSISTENCE_FAILED);
    }
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(transfer_id) = name
            .strip_prefix(TERMINAL_RECOVERY_PREFIX)
            .and_then(|name| name.strip_suffix(TERMINAL_RECOVERY_SUFFIX))
        else {
            continue;
        };
        if transfer_id.len() != 40 || !transfer_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            continue;
        }
        if paths.len() >= MAX_PERSISTED_TRANSFERS {
            return Err(VR_DOWNLOAD_PERSISTENCE_FAILED);
        }
        paths.push(entry.path());
    }
    paths.sort();
    Ok(paths)
}

fn read_terminal_recoveries(persistence_path: &Path) -> Result<Vec<TransferRecord>, &'static str> {
    Ok(terminal_recovery_paths(persistence_path)?
        .into_iter()
        .filter_map(|path| parse_terminal_recovery(persistence_path, &path))
        .collect())
}

fn same_transfer_authority(left: &TransferRecord, right: &TransferRecord) -> bool {
    left.transfer_id == right.transfer_id
        && left.category == right.category
        && left.code == right.code
        && left.release_name == right.release_name
        && left.movie_identity == right.movie_identity
        && left.tv_identity == right.tv_identity
        && left.infohash == right.infohash
        && left.metainfo == right.metainfo
        && left.selected_files == right.selected_files
        && left.destination == right.destination
        && left.fingerprints == right.fingerprints
        && left.current_paths == right.current_paths
        && left.organization_state == right.organization_state
        && encoded_boundary_segments(left).ok() == encoded_boundary_segments(right).ok()
}

fn same_terminal_authority(left: &TransferRecord, right: &TransferRecord) -> bool {
    same_transfer_authority(left, right) && left.downloaded_bytes == right.downloaded_bytes
}

fn remove_terminal_recovery(
    persistence_path: &Path,
    record: &TransferRecord,
) -> Result<(), &'static str> {
    let path = terminal_recovery_path(persistence_path, record)?;
    let recovered = match fs::symlink_metadata(&path) {
        Ok(_) => parse_terminal_recovery(persistence_path, &path)
            .ok_or(VR_DOWNLOAD_PERSISTENCE_FAILED)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(VR_DOWNLOAD_PERSISTENCE_FAILED),
    };
    if !same_terminal_authority(record, &recovered)
        || record
            .terminal_recovery_generation
            .is_some_and(|generation| recovered.terminal_recovery_generation != Some(generation))
    {
        return Err(VR_DOWNLOAD_PERSISTENCE_FAILED);
    }
    fs::remove_file(&path).map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
    sync_parent_directory(&path);
    if let Ok(directory) = terminal_recovery_directory(persistence_path) {
        let _ = fs::remove_dir(directory);
    }
    Ok(())
}

fn cleanup_recovery_directory(persistence_path: &Path) -> Result<PathBuf, &'static str> {
    Ok(persistence_path
        .parent()
        .ok_or(VR_DOWNLOAD_PERSISTENCE_FAILED)?
        .join(CLEANUP_RECOVERY_DIRECTORY))
}

fn cleanup_recovery_path(
    persistence_path: &Path,
    record: &TransferRecord,
) -> Result<PathBuf, &'static str> {
    Ok(cleanup_recovery_directory(persistence_path)?.join(format!(
        "{CLEANUP_RECOVERY_PREFIX}{}{TERMINAL_RECOVERY_SUFFIX}",
        record.transfer_id
    )))
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn encoded_cleanup_recovery(recovery: &CleanupRecovery) -> Result<Vec<u8>, &'static str> {
    if recovery.record.state != TransferState::Cleanup
        || recovery.record.organization_state != OrganizationState::None
        || recovery.record.fingerprints.len() != recovery.record.selected_files.len()
        || recovery.files.len() != recovery.record.selected_files.len()
    {
        return Err(VR_DOWNLOAD_PERSISTENCE_FAILED);
    }
    let states = recovery
        .files
        .iter()
        .map(|state| state.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let encoded_record = encode_transfer(&recovery.record)?;
    let mut checksum_input = states.as_bytes().to_vec();
    checksum_input.push(b'\n');
    checksum_input.extend_from_slice(&encoded_record);
    let mut bytes = CLEANUP_RECOVERY_HEADER.to_vec();
    bytes.extend_from_slice(hex_sha1(&checksum_input).as_bytes());
    bytes.push(b'\n');
    bytes.extend_from_slice(&checksum_input);
    bytes.push(b'\n');
    if bytes.len() as u64 > MAX_PERSISTENCE_BYTES {
        return Err(VR_DOWNLOAD_PERSISTENCE_FAILED);
    }
    Ok(bytes)
}

fn parse_cleanup_recovery(persistence_path: &Path, path: &Path) -> Option<CleanupRecovery> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_PERSISTENCE_BYTES
    {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    let fields = bytes
        .strip_prefix(CLEANUP_RECOVERY_HEADER)?
        .split(|byte| *byte == b'\n')
        .collect::<Vec<_>>();
    let [checksum, states, encoded_record, trailing] = fields.as_slice() else {
        return None;
    };
    if !trailing.is_empty() || checksum.len() != 40 {
        return None;
    }
    let mut checksum_input = states.to_vec();
    checksum_input.push(b'\n');
    checksum_input.extend_from_slice(encoded_record);
    if checksum != &hex_sha1(&checksum_input).as_bytes() {
        return None;
    }
    let files = std::str::from_utf8(states)
        .ok()?
        .split(',')
        .map(CleanupFileState::from_str)
        .collect::<Option<Vec<_>>>()?;
    let record = parse_transfer_line(encoded_record, false)?;
    if record.state != TransferState::Cleanup
        || record.organization_state != OrganizationState::None
        || record.handle.is_some()
        || record.pending_action.is_some()
        || files.len() != record.selected_files.len()
        || cleanup_recovery_path(persistence_path, &record)
            .ok()?
            .as_path()
            != path
    {
        return None;
    }
    Some(CleanupRecovery { record, files })
}

fn cleanup_recovery_paths(persistence_path: &Path) -> Result<Vec<PathBuf>, &'static str> {
    let directory = cleanup_recovery_directory(persistence_path)?;
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(VR_DOWNLOAD_PERSISTENCE_FAILED),
    };
    let metadata = fs::symlink_metadata(&directory).map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(VR_DOWNLOAD_PERSISTENCE_FAILED);
    }
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
        let name = entry.file_name();
        let Some(transfer_id) = name
            .to_str()
            .and_then(|name| name.strip_prefix(CLEANUP_RECOVERY_PREFIX))
            .and_then(|name| name.strip_suffix(TERMINAL_RECOVERY_SUFFIX))
        else {
            continue;
        };
        if transfer_id.len() != 40 || !transfer_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            continue;
        }
        if paths.len() >= MAX_PERSISTED_TRANSFERS {
            return Err(VR_DOWNLOAD_PERSISTENCE_FAILED);
        }
        paths.push(entry.path());
    }
    paths.sort();
    Ok(paths)
}

fn read_cleanup_recoveries(persistence_path: &Path) -> Result<Vec<CleanupRecovery>, &'static str> {
    cleanup_recovery_paths(persistence_path)?
        .into_iter()
        .map(|path| {
            parse_cleanup_recovery(persistence_path, &path).ok_or(VR_DOWNLOAD_PERSISTENCE_FAILED)
        })
        .collect()
}

fn same_cleanup_record_identity(left: &TransferRecord, right: &TransferRecord) -> bool {
    left.transfer_id == right.transfer_id
        && left.category == right.category
        && left.code == right.code
        && left.release_name == right.release_name
        && left.movie_identity == right.movie_identity
        && left.tv_identity == right.tv_identity
        && left.infohash == right.infohash
        && left.metainfo == right.metainfo
        && left.selected_files == right.selected_files
        && left.destination == right.destination
        && left.fingerprints == right.fingerprints
        && left.current_paths == right.current_paths
        && left.organization_state == right.organization_state
        && left.downloaded_bytes == right.downloaded_bytes
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn write_cleanup_recovery(
    persistence_path: &Path,
    recovery: &CleanupRecovery,
) -> Result<(), &'static str> {
    let directory = cleanup_recovery_directory(persistence_path)?;
    fs::create_dir_all(&directory).map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
    let metadata = fs::symlink_metadata(&directory).map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(VR_DOWNLOAD_PERSISTENCE_FAILED);
    }
    let path = cleanup_recovery_path(persistence_path, &recovery.record)?;
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            let existing = parse_cleanup_recovery(persistence_path, &path)
                .ok_or(VR_DOWNLOAD_PERSISTENCE_FAILED)?;
            let file_states_advance =
                existing
                    .files
                    .iter()
                    .zip(&recovery.files)
                    .all(|(before, after)| {
                        before == after
                            || (*before == CleanupFileState::Present
                                && *after == CleanupFileState::Deleted)
                    });
            let existing_boundary = encoded_boundary_segments(&existing.record)?;
            let next_boundary = encoded_boundary_segments(&recovery.record)?;
            let boundary_is_valid = existing_boundary == next_boundary
                || (recovery
                    .files
                    .iter()
                    .all(|state| *state != CleanupFileState::Present)
                    && next_boundary.is_empty());
            if metadata.file_type().is_symlink()
                || !metadata.is_file()
                || !same_cleanup_record_identity(&existing.record, &recovery.record)
                || !file_states_advance
                || !boundary_is_valid
            {
                return Err(VR_DOWNLOAD_PERSISTENCE_FAILED);
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => return Err(VR_DOWNLOAD_PERSISTENCE_FAILED),
    }
    let bytes = encoded_cleanup_recovery(recovery)?;
    let replacement = path_with_suffix(&path, PERSISTENCE_REPLACEMENT_SUFFIX)?;
    clear_stale_persistence_replacement(&replacement)?;
    write_replacement_file(&replacement, &bytes)?;
    if let Err(error) = replace_persistence_file(&replacement, &path) {
        let _ = fs::remove_file(replacement);
        return Err(error);
    }
    sync_parent_directory(&path);
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn remove_cleanup_recovery(
    persistence_path: &Path,
    expected: &CleanupRecovery,
) -> Result<(), &'static str> {
    let path = cleanup_recovery_path(persistence_path, &expected.record)?;
    let current =
        parse_cleanup_recovery(persistence_path, &path).ok_or(VR_DOWNLOAD_PERSISTENCE_FAILED)?;
    if !same_transfer_authority(&current.record, &expected.record)
        || current.record.state != expected.record.state
        || current.record.downloaded_bytes != expected.record.downloaded_bytes
        || current.files != expected.files
    {
        return Err(VR_DOWNLOAD_PERSISTENCE_FAILED);
    }
    fs::remove_file(&path).map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
    sync_parent_directory(&path);
    if let Ok(directory) = cleanup_recovery_directory(persistence_path) {
        let _ = fs::remove_dir(directory);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn macos_cleanup_mutation_path(
    persistence_path: &Path,
    transfer_id: &str,
) -> Result<PathBuf, &'static str> {
    if transfer_id.len() != 40 || !transfer_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(VR_DOWNLOAD_PERSISTENCE_FAILED);
    }
    Ok(cleanup_recovery_directory(persistence_path)?.join(format!(
        "{MACOS_CLEANUP_MUTATION_PREFIX}{transfer_id}{TERMINAL_RECOVERY_SUFFIX}"
    )))
}

#[cfg(target_os = "macos")]
fn macos_cleanup_staging_path(
    record: &TransferRecord,
    selected_index: usize,
    expected_fingerprint: &str,
) -> Result<PathBuf, &'static str> {
    let target = current_target(record, selected_index)?;
    let mut identity = record.transfer_id.as_bytes().to_vec();
    identity.extend_from_slice(&(selected_index as u64).to_be_bytes());
    identity.extend_from_slice(target.to_string_lossy().as_bytes());
    identity.extend_from_slice(expected_fingerprint.as_bytes());
    let suffix = hex_sha1(&identity);
    Ok(target
        .parent()
        .ok_or(VR_DOWNLOAD_PERSISTENCE_FAILED)?
        .join(format!(
            ".auto-video-macos-cleanup-{}-{selected_index}-{}.delete",
            record.transfer_id,
            &suffix[..12]
        )))
}

#[cfg(target_os = "macos")]
fn macos_cleanup_staging_token(
    record: &TransferRecord,
    selected_index: usize,
    staging_path: &Path,
    expected_fingerprint: &str,
) -> String {
    let mut identity = record.transfer_id.as_bytes().to_vec();
    identity.extend_from_slice(&(selected_index as u64).to_be_bytes());
    identity.extend_from_slice(staging_path.to_string_lossy().as_bytes());
    identity.extend_from_slice(expected_fingerprint.as_bytes());
    format!("auto-video-macos-cleanup-{}", hex_sha1(&identity))
}

#[cfg(target_os = "macos")]
fn same_macos_cleanup_mutation_identity(
    left: &MacosCleanupMutation,
    right: &MacosCleanupMutation,
) -> bool {
    same_terminal_authority(&left.record, &right.record)
        && left.selected_index == right.selected_index
        && left.target_path == right.target_path
        && left.staging_path == right.staging_path
        && left.expected_fingerprint == right.expected_fingerprint
        && left.staging_token == right.staging_token
}

#[cfg(target_os = "macos")]
fn valid_macos_cleanup_phase_transition(
    current: MacosCleanupMutationPhase,
    next: MacosCleanupMutationPhase,
) -> bool {
    use MacosCleanupMutationPhase as Phase;

    current == next
        || matches!(
            (current, next),
            (Phase::StagingCreationPrepared, Phase::StagingCreated)
                | (Phase::StagingCreated, Phase::ExchangePrepared)
                | (Phase::ExchangePrepared, Phase::Exchanged)
                | (Phase::Exchanged, Phase::ExactDeletionPrepared)
                | (Phase::ExactDeletionPrepared, Phase::ExactDeleted)
                | (Phase::ExactDeleted, Phase::StagingCleanupPrepared)
                | (Phase::ExchangePrepared, Phase::RollbackExchangePrepared)
                | (Phase::RollbackExchangePrepared, Phase::RolledBack)
                | (
                    Phase::ExchangePrepared,
                    Phase::RollbackStagingCleanupPrepared
                )
                | (Phase::Exchanged, Phase::RollbackStagingCleanupPrepared)
                | (
                    Phase::ExactDeletionPrepared,
                    Phase::RollbackStagingCleanupPrepared
                )
                | (Phase::RolledBack, Phase::RollbackStagingCleanupPrepared)
        )
}

#[cfg(target_os = "macos")]
fn validate_macos_cleanup_mutation(
    persistence_path: &Path,
    mutation: &MacosCleanupMutation,
) -> Result<(), &'static str> {
    let record = &mutation.record;
    if record.state != TransferState::Cleanup
        || record.organization_state != OrganizationState::None
        || record.handle.is_some()
        || record.pending_action.is_some()
        || record.fingerprints.len() != record.selected_files.len()
        || record.current_paths.len() != record.selected_files.len()
        || mutation.selected_index >= record.selected_files.len()
        || current_target(record, mutation.selected_index)? != mutation.target_path
        || record.fingerprints[mutation.selected_index] != mutation.expected_fingerprint
        || macos_cleanup_staging_path(
            record,
            mutation.selected_index,
            &mutation.expected_fingerprint,
        )? != mutation.staging_path
        || macos_cleanup_staging_token(
            record,
            mutation.selected_index,
            &mutation.staging_path,
            &mutation.expected_fingerprint,
        ) != mutation.staging_token
        || macos_cleanup_mutation_path(persistence_path, &record.transfer_id).is_err()
    {
        return Err(VR_DOWNLOAD_PERSISTENCE_FAILED);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn encoded_macos_cleanup_mutation(
    persistence_path: &Path,
    mutation: &MacosCleanupMutation,
) -> Result<Vec<u8>, &'static str> {
    validate_macos_cleanup_mutation(persistence_path, mutation)?;
    let fields = [
        mutation.phase.as_str().to_owned(),
        mutation.selected_index.to_string(),
        encode_hex(mutation.target_path.to_string_lossy().as_bytes()),
        encode_hex(mutation.staging_path.to_string_lossy().as_bytes()),
        encode_hex(mutation.expected_fingerprint.as_bytes()),
        encode_hex(mutation.staging_token.as_bytes()),
        encode_hex(&encode_transfer(&mutation.record)?),
    ];
    let payload = fields.join("\n");
    let mut bytes = MACOS_CLEANUP_MUTATION_HEADER.to_vec();
    bytes.extend_from_slice(hex_sha1(payload.as_bytes()).as_bytes());
    bytes.push(b'\n');
    bytes.extend_from_slice(payload.as_bytes());
    bytes.push(b'\n');
    if bytes.len() as u64 > MAX_PERSISTENCE_BYTES {
        return Err(VR_DOWNLOAD_PERSISTENCE_FAILED);
    }
    Ok(bytes)
}

#[cfg(target_os = "macos")]
fn parse_macos_cleanup_mutation(
    persistence_path: &Path,
    path: &Path,
) -> Option<MacosCleanupMutation> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_PERSISTENCE_BYTES
    {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    let fields = bytes
        .strip_prefix(MACOS_CLEANUP_MUTATION_HEADER)?
        .split(|byte| *byte == b'\n')
        .collect::<Vec<_>>();
    let [checksum, phase, selected_index, target, staging, fingerprint, token, record, trailing] =
        fields.as_slice()
    else {
        return None;
    };
    if !trailing.is_empty() || checksum.len() != 40 {
        return None;
    }
    let payload = [
        *phase,
        *selected_index,
        *target,
        *staging,
        *fingerprint,
        *token,
        *record,
    ]
    .join(&b'\n');
    if checksum != &hex_sha1(&payload).as_bytes() {
        return None;
    }
    let mutation = MacosCleanupMutation {
        record: parse_transfer_line(&decode_hex(record)?, false)?,
        selected_index: std::str::from_utf8(selected_index).ok()?.parse().ok()?,
        target_path: PathBuf::from(decode_text(target)?),
        staging_path: PathBuf::from(decode_text(staging)?),
        expected_fingerprint: decode_text(fingerprint)?,
        staging_token: decode_text(token)?,
        phase: MacosCleanupMutationPhase::from_str(std::str::from_utf8(phase).ok()?)?,
    };
    if macos_cleanup_mutation_path(persistence_path, &mutation.record.transfer_id)
        .ok()?
        .as_path()
        != path
        || validate_macos_cleanup_mutation(persistence_path, &mutation).is_err()
    {
        return None;
    }
    Some(mutation)
}

#[cfg(target_os = "macos")]
fn read_macos_cleanup_mutations(
    persistence_path: &Path,
) -> Result<Vec<MacosCleanupMutation>, &'static str> {
    use std::os::unix::ffi::OsStrExt;

    let directory = cleanup_recovery_directory(persistence_path)?;
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(VR_DOWNLOAD_PERSISTENCE_FAILED),
    };
    let metadata = fs::symlink_metadata(&directory).map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(VR_DOWNLOAD_PERSISTENCE_FAILED);
    }
    let mut mutations = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
        let name = entry.file_name();
        if !name
            .as_bytes()
            .starts_with(MACOS_CLEANUP_MUTATION_PREFIX.as_bytes())
        {
            continue;
        }
        let name = name.to_str().ok_or(VR_DOWNLOAD_PERSISTENCE_FAILED)?;
        let transfer_id = name
            .strip_prefix(MACOS_CLEANUP_MUTATION_PREFIX)
            .and_then(|name| name.strip_suffix(TERMINAL_RECOVERY_SUFFIX))
            .ok_or(VR_DOWNLOAD_PERSISTENCE_FAILED)?;
        if transfer_id.len() != 40
            || !transfer_id.bytes().all(|byte| byte.is_ascii_hexdigit())
            || mutations.len() >= MAX_PERSISTED_TRANSFERS
        {
            return Err(VR_DOWNLOAD_PERSISTENCE_FAILED);
        }
        mutations.push(
            parse_macos_cleanup_mutation(persistence_path, &entry.path())
                .ok_or(VR_DOWNLOAD_PERSISTENCE_FAILED)?,
        );
    }
    mutations.sort_by(|left, right| left.record.transfer_id.cmp(&right.record.transfer_id));
    Ok(mutations)
}

#[cfg(target_os = "macos")]
fn write_macos_cleanup_mutation(
    persistence_path: &Path,
    mutation: &MacosCleanupMutation,
) -> Result<(), &'static str> {
    let directory = cleanup_recovery_directory(persistence_path)?;
    fs::create_dir_all(&directory).map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
    let metadata = fs::symlink_metadata(&directory).map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(VR_DOWNLOAD_PERSISTENCE_FAILED);
    }
    let path = macos_cleanup_mutation_path(persistence_path, &mutation.record.transfer_id)?;
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            let current = parse_macos_cleanup_mutation(persistence_path, &path)
                .ok_or(VR_DOWNLOAD_PERSISTENCE_FAILED)?;
            if metadata.file_type().is_symlink()
                || !metadata.is_file()
                || !same_macos_cleanup_mutation_identity(&current, mutation)
                || !valid_macos_cleanup_phase_transition(current.phase, mutation.phase)
            {
                return Err(VR_DOWNLOAD_PERSISTENCE_FAILED);
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if mutation.phase != MacosCleanupMutationPhase::StagingCreationPrepared {
                return Err(VR_DOWNLOAD_PERSISTENCE_FAILED);
            }
        }
        Err(_) => return Err(VR_DOWNLOAD_PERSISTENCE_FAILED),
    }
    let bytes = encoded_macos_cleanup_mutation(persistence_path, mutation)?;
    let replacement = path_with_suffix(&path, PERSISTENCE_REPLACEMENT_SUFFIX)?;
    clear_stale_persistence_replacement(&replacement)?;
    write_replacement_file(&replacement, &bytes)?;
    if let Err(error) = replace_persistence_file(&replacement, &path) {
        let _ = fs::remove_file(replacement);
        return Err(error);
    }
    sync_parent_directory(&path);
    Ok(())
}

#[cfg(target_os = "macos")]
fn remove_macos_cleanup_mutation(
    persistence_path: &Path,
    expected: &MacosCleanupMutation,
) -> Result<(), &'static str> {
    let path = macos_cleanup_mutation_path(persistence_path, &expected.record.transfer_id)?;
    let current = parse_macos_cleanup_mutation(persistence_path, &path)
        .ok_or(VR_DOWNLOAD_PERSISTENCE_FAILED)?;
    if !same_macos_cleanup_mutation_identity(&current, expected) || current.phase != expected.phase
    {
        return Err(VR_DOWNLOAD_PERSISTENCE_FAILED);
    }
    fs::remove_file(&path).map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
    sync_parent_directory(&path);
    Ok(())
}

fn organization_recovery_path(record: &TransferRecord) -> PathBuf {
    record.destination.join(format!(
        "{ORGANIZATION_RECOVERY_PREFIX}{}{ORGANIZATION_RECOVERY_SUFFIX}",
        record.transfer_id
    ))
}

fn organization_recovery_successor_path(record: &TransferRecord) -> PathBuf {
    record.destination.join(format!(
        "{ORGANIZATION_RECOVERY_PREFIX}{}{ORGANIZATION_RECOVERY_SUCCESSOR_SUFFIX}",
        record.transfer_id
    ))
}

fn encoded_organization_recovery(
    record: &TransferRecord,
    current_paths: &[String],
) -> Result<Vec<u8>, &'static str> {
    let mut bytes = ORGANIZATION_RECOVERY_HEADER.to_vec();
    bytes.extend_from_slice(&encode_transfer_state(
        record,
        OrganizationState::Attention,
        current_paths,
    )?);
    bytes.push(b'\n');
    if bytes.len() as u64 > MAX_PERSISTENCE_BYTES {
        return Err(VR_DOWNLOAD_PERSISTENCE_FAILED);
    }
    Ok(bytes)
}

fn write_organization_recovery(
    record: &TransferRecord,
    current_paths: &[String],
    preserved_paths: Option<&[String]>,
) -> Result<(), &'static str> {
    let bytes = encoded_organization_recovery(record, current_paths)?;
    let path = if let Some(preserved_paths) = preserved_paths {
        let preserved_path = organization_recovery_path(record);
        let metadata =
            fs::symlink_metadata(&preserved_path).map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || fs::canonicalize(&preserved_path).ok().as_deref() != Some(preserved_path.as_path())
            || fs::read(&preserved_path).ok().as_deref()
                != Some(encoded_organization_recovery(record, preserved_paths)?.as_slice())
        {
            return Err(VR_DOWNLOAD_PERSISTENCE_FAILED);
        }
        organization_recovery_successor_path(record)
    } else {
        organization_recovery_path(record)
    };
    let mut options = OpenOptions::new();
    options.write(true);
    if preserved_paths.is_some() {
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink()
                    || !metadata.is_file()
                    || fs::canonicalize(&path).ok().as_deref() != Some(path.as_path())
                {
                    return Err(VR_DOWNLOAD_PERSISTENCE_FAILED);
                }
                options.truncate(true);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                options.create_new(true);
            }
            Err(_) => return Err(VR_DOWNLOAD_PERSISTENCE_FAILED),
        }
    } else {
        options.create_new(true);
    }
    let mut file = options
        .open(path)
        .map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)
}

fn clear_organization_recovery(record: &TransferRecord) {
    let _ = remove_organization_recovery(record);
}

fn remove_organization_recovery(record: &TransferRecord) -> Result<(), &'static str> {
    let mut failed = false;
    for path in [
        organization_recovery_path(record),
        organization_recovery_successor_path(record),
    ] {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => failed = true,
        }
    }
    if failed {
        Err(VR_DOWNLOAD_PERSISTENCE_FAILED)
    } else {
        Ok(())
    }
}

fn recovery_file_matches(
    record: &TransferRecord,
    selected_index: usize,
    relative_path: &str,
) -> bool {
    let Ok(relative_path) = relative_file_path(relative_path) else {
        return false;
    };
    let path = record.destination.join(relative_path);
    let Ok(metadata) = fs::symlink_metadata(&path) else {
        return false;
    };
    !metadata.file_type().is_symlink()
        && metadata.is_file()
        && metadata.len() == record.selected_files[selected_index].size
        && fs::canonicalize(&path).ok().as_deref() == Some(path.as_path())
        && path.starts_with(&record.destination)
        && file_fingerprint(&path).ok().as_ref() == record.fingerprints.get(selected_index)
}

fn reconcile_interrupted_organization(mut record: TransferRecord) -> Option<TransferRecord> {
    if validate_resume_context(&record).is_ok() {
        return Some(record);
    }
    let eligible_media = record
        .selected_files
        .iter()
        .filter(|file| is_supported_transfer_media(Path::new(&file.path)))
        .count();
    if eligible_media == 0 {
        return None;
    }
    let mut current_paths = Vec::with_capacity(record.current_paths.len());
    for selected_index in 0..record.current_paths.len() {
        let source_relative = &record.current_paths[selected_index];
        let destination_relative =
            organization_destination_relative(&record, selected_index, eligible_media).ok()?;
        let Some(destination_relative) = destination_relative else {
            if !recovery_file_matches(&record, selected_index, source_relative) {
                return None;
            }
            current_paths.push(source_relative.clone());
            continue;
        };
        if destination_relative == *source_relative {
            if !recovery_file_matches(&record, selected_index, source_relative) {
                return None;
            }
            current_paths.push(source_relative.clone());
            continue;
        }
        let source_matches = recovery_file_matches(&record, selected_index, source_relative);
        let destination_matches =
            recovery_file_matches(&record, selected_index, &destination_relative);
        current_paths.push(match (source_matches, destination_matches) {
            (true, false) => source_relative.clone(),
            (false, true) => destination_relative,
            _ => return None,
        });
    }
    record.current_paths = current_paths;
    record.organization_state = OrganizationState::Attention;
    validate_resume_context(&record).ok()?;
    Some(record)
}

fn parse_organization_recovery_file(path: &Path) -> Option<TransferRecord> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_PERSISTENCE_BYTES
    {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    let (line, legacy_vr) = if let Some(line) = bytes.strip_prefix(ORGANIZATION_RECOVERY_HEADER) {
        (line, false)
    } else {
        (
            bytes.strip_prefix(LEGACY_ORGANIZATION_RECOVERY_HEADER)?,
            true,
        )
    };
    let record = parse_transfer_line(line.strip_suffix(b"\n")?, legacy_vr)?;
    if record.organization_state != OrganizationState::Attention
        || record.state != TransferState::Completed
        || (legacy_vr && record.category != TransferCategory::Vr)
        || (organization_recovery_path(&record) != path
            && organization_recovery_successor_path(&record) != path)
    {
        return None;
    }
    Some(record)
}

fn recorded_path_matches(
    destination: &Path,
    relative_path: &str,
    requested_path: &Path,
) -> Result<bool, VrLibraryTrashOwnershipError> {
    if !destination.is_absolute()
        || destination.components().any(|component| {
            !matches!(
                component,
                Component::Prefix(_) | Component::RootDir | Component::Normal(_)
            )
        })
    {
        return Err(VrLibraryTrashOwnershipError::Unavailable);
    }
    let relative_path =
        relative_file_path(relative_path).map_err(|_| VrLibraryTrashOwnershipError::Unavailable)?;
    let target = destination.join(relative_path);
    if !target.starts_with(destination) {
        return Err(VrLibraryTrashOwnershipError::Unavailable);
    }
    if target == requested_path {
        return Ok(true);
    }
    match fs::canonicalize(target) {
        Ok(canonical_target) => Ok(canonical_target == requested_path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(VrLibraryTrashOwnershipError::Unavailable),
    }
}

fn transfer_record_owns_path(
    record: &TransferRecord,
    requested_path: &Path,
) -> Result<bool, VrLibraryTrashOwnershipError> {
    if record.current_paths.len() != record.selected_files.len()
        || record
            .selected_files
            .iter()
            .map(|file| &file.path)
            .collect::<BTreeSet<_>>()
            .len()
            != record.selected_files.len()
        || record.current_paths.iter().collect::<BTreeSet<_>>().len() != record.current_paths.len()
        || (record.organization_state == OrganizationState::None
            && record
                .current_paths
                .iter()
                .zip(&record.selected_files)
                .any(|(current, selected)| current != &selected.path))
        || (record.organization_state != OrganizationState::None
            && (record.state != TransferState::Completed
                || record.fingerprints.len() != record.selected_files.len()))
    {
        return Err(VrLibraryTrashOwnershipError::Unavailable);
    }
    for selected_file in &record.selected_files {
        if recorded_path_matches(&record.destination, &selected_file.path, requested_path)? {
            return Ok(true);
        }
    }
    for current_path in &record.current_paths {
        if recorded_path_matches(&record.destination, current_path, requested_path)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn organization_plan_owns_path(
    context: &VrDownloadContext,
    requested_path: &Path,
) -> Result<bool, VrLibraryTrashOwnershipError> {
    let Some(plan) = context.organization_plan.as_ref() else {
        return Ok(false);
    };
    let record = context
        .transfers
        .iter()
        .find_map(|transfer| match transfer {
            StoredTransfer::Valid(record) if record.transfer_id == plan.transfer_id => Some(record),
            StoredTransfer::Valid(_) | StoredTransfer::Corrupt(_) => None,
        })
        .filter(|record| record.category == plan.category)
        .ok_or(VrLibraryTrashOwnershipError::Unavailable)?;
    if plan.generation != context.organization_generation
        || plan.entries.len() != record.selected_files.len()
        || plan.identity
            != organization_identity(record)
                .map_err(|_| VrLibraryTrashOwnershipError::Unavailable)?
        || plan.plan_id
            != organization_plan_id(plan.generation, record, &plan.entries)
                .map_err(|_| VrLibraryTrashOwnershipError::Unavailable)?
    {
        return Err(VrLibraryTrashOwnershipError::Unavailable);
    }
    let mut selected_indices = BTreeSet::new();
    for entry in &plan.entries {
        let current_path = record
            .current_paths
            .get(entry.selected_index)
            .ok_or(VrLibraryTrashOwnershipError::Unavailable)?;
        let destination_is_valid = match entry.kind {
            OrganizationEntryKind::Move | OrganizationEntryKind::MediaUnchanged => {
                entry.destination_relative.is_some()
            }
            OrganizationEntryKind::NonMediaUnchanged => entry.destination_relative.is_none(),
        };
        if !selected_indices.insert(entry.selected_index)
            || &entry.source_relative != current_path
            || !destination_is_valid
        {
            return Err(VrLibraryTrashOwnershipError::Unavailable);
        }
        if recorded_path_matches(&record.destination, &entry.source_relative, requested_path)? {
            return Ok(true);
        }
        if let Some(destination_relative) = &entry.destination_relative {
            if recorded_path_matches(&record.destination, destination_relative, requested_path)? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn durable_terminal_recovery_owns_path(
    persistence_path: &Path,
    requested_path: &Path,
) -> Result<bool, VrLibraryTrashOwnershipError> {
    let paths = terminal_recovery_paths(persistence_path)
        .map_err(|_| VrLibraryTrashOwnershipError::Unavailable)?;
    for path in paths {
        let record = parse_terminal_recovery(persistence_path, &path)
            .ok_or(VrLibraryTrashOwnershipError::Unavailable)?;
        if transfer_record_owns_path(&record, requested_path)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn durable_cleanup_recovery_owns_path(
    persistence_path: &Path,
    requested_path: &Path,
) -> Result<bool, VrLibraryTrashOwnershipError> {
    for path in cleanup_recovery_paths(persistence_path)
        .map_err(|_| VrLibraryTrashOwnershipError::Unavailable)?
    {
        let recovery = parse_cleanup_recovery(persistence_path, &path)
            .ok_or(VrLibraryTrashOwnershipError::Unavailable)?;
        if transfer_record_owns_path(&recovery.record, requested_path)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn durable_organization_recovery_owns_path(
    destination: &Path,
    requested_path: &Path,
) -> Result<bool, VrLibraryTrashOwnershipError> {
    let entries =
        fs::read_dir(destination).map_err(|_| VrLibraryTrashOwnershipError::Unavailable)?;
    let mut recovery_count = 0;
    for entry in entries {
        let entry = entry.map_err(|_| VrLibraryTrashOwnershipError::Unavailable)?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !name.starts_with(ORGANIZATION_RECOVERY_PREFIX)
            || (!name.ends_with(ORGANIZATION_RECOVERY_SUFFIX)
                && !name.ends_with(ORGANIZATION_RECOVERY_SUCCESSOR_SUFFIX))
        {
            continue;
        }
        recovery_count += 1;
        if recovery_count > MAX_PERSISTED_TRANSFERS * 2 {
            return Err(VrLibraryTrashOwnershipError::Unavailable);
        }
        let record = parse_organization_recovery_file(&entry.path())
            .ok_or(VrLibraryTrashOwnershipError::Unavailable)?;
        if transfer_record_owns_path(&record, requested_path)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn read_organization_recovery_file(path: &Path) -> Option<TransferRecord> {
    reconcile_interrupted_organization(parse_organization_recovery_file(path)?)
}

fn read_organization_recoveries(destination: &Path) -> Vec<TransferRecord> {
    let Ok(entries) = fs::read_dir(destination) else {
        return Vec::new();
    };
    let mut paths = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            let name = name.strip_prefix(ORGANIZATION_RECOVERY_PREFIX)?;
            let transfer_id = name
                .strip_suffix(ORGANIZATION_RECOVERY_SUCCESSOR_SUFFIX)
                .or_else(|| name.strip_suffix(ORGANIZATION_RECOVERY_SUFFIX))?;
            (transfer_id.len() == 40 && transfer_id.bytes().all(|byte| byte.is_ascii_hexdigit()))
                .then_some(entry.path())
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths.truncate(MAX_PERSISTED_TRANSFERS * 2);
    let mut transfer_ids = BTreeSet::new();
    paths
        .into_iter()
        .filter_map(|path| read_organization_recovery_file(&path))
        .filter(|record| {
            record.destination == destination && transfer_ids.insert(record.transfer_id.clone())
        })
        .take(MAX_PERSISTED_TRANSFERS)
        .collect()
}

async fn session_for(
    state: &VrDownloadState,
    session_folder: &Path,
) -> Result<Arc<Session>, &'static str> {
    let download_limit = {
        let mut context = state.0.lock().map_err(|_| VR_DOWNLOAD_FAILED)?;
        if let Some(session) = &context.session {
            return Ok(session.clone());
        }
        if context.session_starting {
            return Err(VR_DOWNLOAD_ACTION_INVALID);
        }
        let DownloadLimitState::Loaded(download_limit) = context.download_limit else {
            return Err(VR_DOWNLOAD_LIMIT_UNAVAILABLE);
        };
        context.session_starting = true;
        download_limit
    };

    if fs::create_dir_all(session_folder).is_err() {
        if let Ok(mut context) = state.0.lock() {
            context.session_starting = false;
        }
        return Err(VR_DOWNLOAD_FAILED);
    }

    let result = Session::new_with_opts(session_folder.to_owned(), session_options(download_limit))
        .await
        .map_err(|_| VR_DOWNLOAD_FAILED);

    let mut context = state.0.lock().map_err(|_| VR_DOWNLOAD_FAILED)?;
    context.session_starting = false;
    match result {
        Ok(session) => {
            context.session = Some(session.clone());
            Ok(session)
        }
        Err(error) => Err(error),
    }
}

pub(crate) async fn acquire_tv_metainfo(
    state: &VrDownloadState,
    session_folder: &Path,
    infohash: &str,
) -> Result<Vec<u8>, TvMetainfoAcquisitionError> {
    let existing_session = {
        let context = state
            .0
            .lock()
            .map_err(|_| TvMetainfoAcquisitionError::LocalUnavailable)?;
        if let Some(session) = &context.session {
            Some(session.clone())
        } else if context.session_starting {
            return Err(TvMetainfoAcquisitionError::LocalPending);
        } else if context.download_limit == DownloadLimitState::Unloaded {
            return Err(TvMetainfoAcquisitionError::LocalUnavailable);
        } else {
            None
        }
    };
    let session = match existing_session {
        Some(session) => session,
        None => session_for(state, session_folder)
            .await
            .map_err(|error| match error {
                VR_DOWNLOAD_ACTION_INVALID => TvMetainfoAcquisitionError::LocalPending,
                _ => TvMetainfoAcquisitionError::LocalUnavailable,
            })?,
    };
    let magnet = format!("magnet:?xt=urn:btih:{infohash}");
    let (sender, receiver) = mpsc::sync_channel(1);
    let acquisition = tauri::async_runtime::spawn(async move {
        let response = session
            .add_torrent(
                AddTorrent::from_url(magnet),
                Some(AddTorrentOptions {
                    list_only: true,
                    peer_opts: Some(PeerConnectionOptions {
                        connect_timeout: Some(Duration::from_secs(10)),
                        read_write_timeout: Some(Duration::from_secs(20)),
                        keep_alive_interval: Some(Duration::from_secs(10)),
                    }),
                    trackers: Some(
                        TV_METADATA_TRACKERS
                            .iter()
                            .map(|tracker| (*tracker).to_owned())
                            .collect(),
                    ),
                    ..Default::default()
                }),
            )
            .await;
        let _ = sender.send(response);
    });
    let response = match tauri::async_runtime::spawn_blocking(move || {
        receiver.recv_timeout(TV_METADATA_TIMEOUT)
    })
    .await
    .map_err(|_| TvMetainfoAcquisitionError::LocalUnavailable)?
    {
        Ok(response) => {
            let _ = acquisition.await;
            response
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            acquisition.abort();
            return Err(TvMetainfoAcquisitionError::Timeout);
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            return Err(TvMetainfoAcquisitionError::Network);
        }
    }
    .map_err(|error| {
        let message = error.to_string().to_ascii_lowercase();
        if message.contains("timeout") {
            TvMetainfoAcquisitionError::Timeout
        } else if message.contains("input address stream exhausted")
            || message.contains("no known way to resolve peers")
            || message.contains("metadata") && message.contains("peer")
        {
            TvMetainfoAcquisitionError::NoMetadataSource
        } else {
            TvMetainfoAcquisitionError::Network
        }
    })?;
    match response {
        AddTorrentResponse::ListOnly(metadata) => Ok(metadata.torrent_bytes.to_vec()),
        AddTorrentResponse::AlreadyManaged(_, _) | AddTorrentResponse::Added(_, _) => {
            Err(TvMetainfoAcquisitionError::LocalUnavailable)
        }
    }
}

fn session_options(download_limit: Option<NonZeroU32>) -> SessionOptions {
    SessionOptions {
        disable_upload: true,
        disable_dht: true,
        disable_dht_persistence: true,
        fastresume: false,
        enable_upnp_port_forwarding: false,
        ratelimits: LimitsConfig {
            download_bps: download_limit_bytes_per_second(download_limit),
            upload_bps: None,
        },
        ..Default::default()
    }
}

#[derive(Clone)]
struct SelectedFileStorageFactory {
    destination: PathBuf,
    selected_files: Arc<BTreeMap<usize, VerifiedDownloadFile>>,
    boundary_segments: Arc<Mutex<BTreeMap<usize, Vec<SparseSegment>>>>,
    resume: bool,
}

impl SelectedFileStorageFactory {
    fn new(
        destination: PathBuf,
        selected_files: &[VerifiedDownloadFile],
        boundary_segments: Arc<Mutex<BTreeMap<usize, Vec<SparseSegment>>>>,
        resume: bool,
    ) -> Self {
        Self {
            destination,
            selected_files: Arc::new(
                selected_files
                    .iter()
                    .cloned()
                    .map(|file| (file.file_id, file))
                    .collect(),
            ),
            boundary_segments,
            resume,
        }
    }
}

impl StorageFactory for SelectedFileStorageFactory {
    type Storage = SelectedFileStorage;

    fn create(
        &self,
        _shared: &librqbit::ManagedTorrentShared,
        _metadata: &librqbit::TorrentMetadata,
    ) -> anyhow::Result<Self::Storage> {
        Ok(SelectedFileStorage {
            destination: self.destination.clone(),
            selected_files: self.selected_files.clone(),
            boundary_segments: self.boundary_segments.clone(),
            resume: self.resume,
            slots: Vec::new(),
        })
    }

    fn clone_box(&self) -> BoxStorageFactory {
        self.clone().boxed()
    }
}

#[derive(Clone)]
struct SparseSegment {
    offset: u64,
    bytes: Vec<u8>,
}

struct SelectedStorageSlot {
    file: Mutex<Option<File>>,
}

impl SelectedStorageSlot {
    fn new(file: Option<File>) -> Self {
        Self {
            file: Mutex::new(file),
        }
    }

    fn take(&self) -> anyhow::Result<Self> {
        let file = self
            .file
            .lock()
            .map_err(|_| anyhow!("selected file lock failed"))?
            .take();
        Ok(Self::new(file))
    }
}

struct SelectedFileStorage {
    destination: PathBuf,
    selected_files: Arc<BTreeMap<usize, VerifiedDownloadFile>>,
    boundary_segments: Arc<Mutex<BTreeMap<usize, Vec<SparseSegment>>>>,
    resume: bool,
    slots: Vec<SelectedStorageSlot>,
}

fn write_sparse_segment(
    segments: &mut Vec<SparseSegment>,
    offset: u64,
    bytes: &[u8],
) -> anyhow::Result<()> {
    if bytes.is_empty() {
        return Ok(());
    }
    let mut merged_start = offset;
    let mut merged_end = offset
        .checked_add(bytes.len() as u64)
        .context("boundary write overflow")?;
    let first = segments
        .iter()
        .position(|segment| {
            segment
                .offset
                .checked_add(segment.bytes.len() as u64)
                .is_none_or(|end| end >= merged_start)
        })
        .unwrap_or(segments.len());
    let mut last = first;
    while let Some(segment) = segments.get(last) {
        if segment.offset > merged_end {
            break;
        }
        merged_start = merged_start.min(segment.offset);
        merged_end = merged_end.max(
            segment
                .offset
                .checked_add(segment.bytes.len() as u64)
                .context("boundary segment overflow")?,
        );
        last += 1;
    }

    let merged_length =
        usize::try_from(merged_end - merged_start).context("boundary segment is too large")?;
    let mut merged_bytes = vec![0; merged_length];
    for segment in &segments[first..last] {
        let start = usize::try_from(segment.offset - merged_start)?;
        merged_bytes[start..start + segment.bytes.len()].copy_from_slice(&segment.bytes);
    }
    let write_start = usize::try_from(offset - merged_start)?;
    merged_bytes[write_start..write_start + bytes.len()].copy_from_slice(bytes);
    segments.splice(
        first..last,
        [SparseSegment {
            offset: merged_start,
            bytes: merged_bytes,
        }],
    );
    Ok(())
}

fn librqbit_relative_path(path: &Path) -> anyhow::Result<String> {
    path.components()
        .map(|component| match component {
            Component::Normal(value) => value
                .to_str()
                .map(str::to_owned)
                .context("torrent path is not valid UTF-8"),
            _ => Err(anyhow!("torrent path is unsafe")),
        })
        .collect::<anyhow::Result<Vec<_>>>()
        .map(|components| components.join("/"))
}

fn prepare_selected_file(
    destination: &Path,
    file: &VerifiedDownloadFile,
    resume: bool,
) -> anyhow::Result<File> {
    let relative = relative_file_path(&file.path).map_err(|error| anyhow!(error))?;
    validate_existing_parents(destination, &relative).map_err(|error| anyhow!(error))?;
    let target = destination.join(&relative);
    let parent = target.parent().context("selected file has no parent")?;
    fs::create_dir_all(parent).context("failed to create selected file parent")?;
    let canonical_parent =
        fs::canonicalize(parent).context("selected file parent is unavailable")?;
    if !canonical_parent.starts_with(destination) {
        return Err(anyhow!("selected file parent escaped destination"));
    }

    let mut options = OpenOptions::new();
    options.read(true).write(true);
    if resume {
        let metadata = fs::symlink_metadata(&target).context("trusted partial file is missing")?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(anyhow!("trusted partial file changed type"));
        }
    } else {
        options.create_new(true);
    }
    options.open(target).context("failed to open selected file")
}

impl TorrentStorage for SelectedFileStorage {
    fn init(
        &mut self,
        _shared: &librqbit::ManagedTorrentShared,
        metadata: &librqbit::TorrentMetadata,
    ) -> anyhow::Result<()> {
        {
            let boundary_segments = self
                .boundary_segments
                .lock()
                .map_err(|_| anyhow!("boundary-byte lock failed"))?;
            for (file_id, segments) in boundary_segments.iter() {
                let file_info = metadata
                    .file_infos
                    .get(*file_id)
                    .context("retained boundary file is outside parsed torrent")?;
                if self.selected_files.contains_key(file_id) {
                    return Err(anyhow!("retained boundary file is selected"));
                }
                for segment in segments {
                    let end = segment
                        .offset
                        .checked_add(segment.bytes.len() as u64)
                        .context("retained boundary segment overflow")?;
                    if end > file_info.len {
                        return Err(anyhow!("retained boundary segment exceeds its file"));
                    }
                }
            }
        }
        let mut slots = Vec::with_capacity(metadata.file_infos.len());
        for (file_id, file_info) in metadata.file_infos.iter().enumerate() {
            let selected = self.selected_files.get(&file_id);
            if let Some(expected) = selected {
                let parsed_path = librqbit_relative_path(&file_info.relative_filename)?;
                if parsed_path != expected.path || file_info.len != expected.size {
                    return Err(anyhow!("selected file identity changed"));
                }
            }
            let file = match selected {
                Some(expected) if !file_info.attrs.padding => Some(prepare_selected_file(
                    &self.destination,
                    expected,
                    self.resume,
                )?),
                _ => None,
            };
            slots.push(SelectedStorageSlot::new(file));
        }
        if self
            .selected_files
            .keys()
            .any(|file_id| *file_id >= slots.len())
        {
            return Err(anyhow!("selected file is outside parsed torrent"));
        }
        self.slots = slots;
        Ok(())
    }

    fn pread_exact(&self, file_id: usize, offset: u64, buffer: &mut [u8]) -> anyhow::Result<()> {
        let slot = self.slots.get(file_id).context("no such torrent file")?;
        if let Some(file) = slot
            .file
            .lock()
            .map_err(|_| anyhow!("selected file lock failed"))?
            .as_mut()
        {
            file.seek(SeekFrom::Start(offset))?;
            file.read_exact(buffer)?;
            return Ok(());
        }

        buffer.fill(0);
        let read_end = offset
            .checked_add(buffer.len() as u64)
            .context("boundary read overflow")?;
        let boundary_segments = self
            .boundary_segments
            .lock()
            .map_err(|_| anyhow!("boundary-byte lock failed"))?;
        if let Some(segments) = boundary_segments.get(&file_id) {
            for segment in segments {
                let segment_end = segment
                    .offset
                    .checked_add(segment.bytes.len() as u64)
                    .context("boundary segment overflow")?;
                let overlap_start = offset.max(segment.offset);
                let overlap_end = read_end.min(segment_end);
                if overlap_start < overlap_end {
                    let source_start = (overlap_start - segment.offset) as usize;
                    let destination_start = (overlap_start - offset) as usize;
                    let length = (overlap_end - overlap_start) as usize;
                    buffer[destination_start..destination_start + length]
                        .copy_from_slice(&segment.bytes[source_start..source_start + length]);
                }
            }
        }
        Ok(())
    }

    fn pwrite_all(&self, file_id: usize, offset: u64, buffer: &[u8]) -> anyhow::Result<()> {
        let slot = self.slots.get(file_id).context("no such torrent file")?;
        if let Some(file) = slot
            .file
            .lock()
            .map_err(|_| anyhow!("selected file lock failed"))?
            .as_mut()
        {
            file.seek(SeekFrom::Start(offset))?;
            file.write_all(buffer)?;
            return Ok(());
        }
        let mut boundary_segments = self
            .boundary_segments
            .lock()
            .map_err(|_| anyhow!("boundary-byte lock failed"))?;
        write_sparse_segment(
            boundary_segments.entry(file_id).or_default(),
            offset,
            buffer,
        )
    }

    fn remove_file(&self, file_id: usize, _filename: &Path) -> anyhow::Result<()> {
        let Some(file) = self.selected_files.get(&file_id) else {
            return Ok(());
        };
        let target = self
            .destination
            .join(relative_file_path(&file.path).map_err(|e| anyhow!(e))?);
        match fs::remove_file(target) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn remove_directory_if_empty(&self, path: &Path) -> anyhow::Result<()> {
        let relative = librqbit_relative_path(path)?;
        let directory = self
            .destination
            .join(relative_file_path(&relative).map_err(|error| anyhow!(error))?);
        if directory.is_dir() && fs::read_dir(&directory)?.next().is_none() {
            fs::remove_dir(directory)?;
        }
        Ok(())
    }

    fn ensure_file_length(&self, file_id: usize, length: u64) -> anyhow::Result<()> {
        let slot = self.slots.get(file_id).context("no such torrent file")?;
        if let Some(file) = slot
            .file
            .lock()
            .map_err(|_| anyhow!("selected file lock failed"))?
            .as_ref()
        {
            file.set_len(length)?;
        }
        Ok(())
    }

    fn take(&self) -> anyhow::Result<Box<dyn TorrentStorage>> {
        Ok(Box::new(Self {
            destination: self.destination.clone(),
            selected_files: self.selected_files.clone(),
            boundary_segments: self.boundary_segments.clone(),
            resume: self.resume,
            slots: self
                .slots
                .iter()
                .map(SelectedStorageSlot::take)
                .collect::<anyhow::Result<Vec<_>>>()?,
        }))
    }
}

#[cfg(unix)]
pub(crate) fn file_fingerprint(path: &Path) -> Result<String, &'static str> {
    use std::os::unix::fs::MetadataExt;

    let metadata = fs::symlink_metadata(path).map_err(|_| VR_FOLDER_UNAVAILABLE)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(VR_DOWNLOAD_DESTINATION_CONFLICT);
    }
    Ok(format!("{}:{}", metadata.dev(), metadata.ino()))
}

#[cfg(target_os = "macos")]
#[link(name = "System")]
extern "C" {
    fn renameatx_np(
        from_fd: i32,
        from: *const std::ffi::c_char,
        to_fd: i32,
        to: *const std::ffi::c_char,
        flags: u32,
    ) -> i32;
}

#[cfg(target_os = "macos")]
fn exchange_macos_cleanup_paths(left: &Path, right: &Path) -> io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};

    const AT_FDCWD: i32 = -2;
    const RENAME_SWAP: u32 = 0x0000_0002;
    let left = CString::new(left.as_os_str().as_bytes())
        .map_err(|_| io::Error::other("cleanup path contains a null byte"))?;
    let right = CString::new(right.as_os_str().as_bytes())
        .map_err(|_| io::Error::other("cleanup path contains a null byte"))?;
    let result = unsafe {
        renameatx_np(
            AT_FDCWD,
            left.as_ptr(),
            AT_FDCWD,
            right.as_ptr(),
            RENAME_SWAP,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MacosCleanupPathState {
    Absent,
    ExactFile,
    StagingToken,
    Other,
}

#[cfg(target_os = "macos")]
fn macos_cleanup_path_state(
    path: &Path,
    expected_fingerprint: &str,
    staging_token: &str,
) -> Result<MacosCleanupPathState, &'static str> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(MacosCleanupPathState::Absent)
        }
        Err(_) => return Err(VR_DOWNLOAD_STALE),
    };
    if metadata.file_type().is_symlink() {
        return Ok(
            if fs::read_link(path).ok().as_deref() == Some(Path::new(staging_token)) {
                MacosCleanupPathState::StagingToken
            } else {
                MacosCleanupPathState::Other
            },
        );
    }
    if metadata.is_file() && file_fingerprint(path)? == expected_fingerprint {
        Ok(MacosCleanupPathState::ExactFile)
    } else {
        Ok(MacosCleanupPathState::Other)
    }
}

#[cfg(target_os = "macos")]
fn macos_cleanup_metadata_matches(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    left.dev() == right.dev()
        && left.ino() == right.ino()
        && left.file_type().is_symlink() == right.file_type().is_symlink()
        && left.is_file() == right.is_file()
}

#[cfg(target_os = "macos")]
fn open_exact_macos_cleanup_file(
    path: &Path,
    expected_fingerprint: &str,
) -> Result<File, &'static str> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    const O_EVTONLY: i32 = 0x0000_8000;
    const O_NOFOLLOW: i32 = 0x0000_0100;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(O_EVTONLY | O_NOFOLLOW)
        .open(path)
        .map_err(|_| VR_DOWNLOAD_STALE)?;
    let pinned = file.metadata().map_err(|_| VR_DOWNLOAD_STALE)?;
    let current = fs::symlink_metadata(path).map_err(|_| VR_DOWNLOAD_STALE)?;
    if !pinned.is_file()
        || pinned.file_type().is_symlink()
        || pinned.nlink() != 1
        || !macos_cleanup_metadata_matches(&pinned, &current)
        || format!("{}:{}", pinned.dev(), pinned.ino()) != expected_fingerprint
    {
        return Err(VR_DOWNLOAD_STALE);
    }
    Ok(file)
}

#[cfg(target_os = "macos")]
fn open_exact_macos_cleanup_staging_token(
    path: &Path,
    staging_token: &str,
) -> Result<Option<File>, &'static str> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    const O_EVTONLY: i32 = 0x0000_8000;
    const O_SYMLINK: i32 = 0x0020_0000;
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(O_EVTONLY | O_SYMLINK)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(VR_DOWNLOAD_STALE),
    };
    let pinned = file.metadata().map_err(|_| VR_DOWNLOAD_STALE)?;
    let before = fs::symlink_metadata(path).map_err(|_| VR_DOWNLOAD_STALE)?;
    let token = fs::read_link(path).map_err(|_| VR_DOWNLOAD_STALE)?;
    let after = fs::symlink_metadata(path).map_err(|_| VR_DOWNLOAD_STALE)?;
    if !pinned.file_type().is_symlink()
        || pinned.nlink() != 1
        || token != Path::new(staging_token)
        || !macos_cleanup_metadata_matches(&pinned, &before)
        || !macos_cleanup_metadata_matches(&pinned, &after)
    {
        return Err(VR_DOWNLOAD_STALE);
    }
    Ok(Some(file))
}

#[cfg(target_os = "macos")]
fn remove_pinned_macos_cleanup_object(
    file: &File,
    path: &Path,
) -> Result<CleanupDeletionOutcome, &'static str> {
    use std::os::unix::fs::MetadataExt;

    let pinned = file.metadata().map_err(|_| VR_DOWNLOAD_STALE)?;
    if pinned.nlink() != 1 {
        return Err(VR_DOWNLOAD_STALE);
    }
    // The open vnode prevents file-ID reuse while /.vol avoids unlinking a raced pathname.
    let volume_path = PathBuf::from("/.vol")
        .join(pinned.dev().to_string())
        .join(pinned.ino().to_string());
    let volume_metadata = fs::symlink_metadata(&volume_path).map_err(|_| VR_DOWNLOAD_STALE)?;
    if !macos_cleanup_metadata_matches(&pinned, &volume_metadata) {
        return Err(VR_DOWNLOAD_STALE);
    }
    fs::remove_file(&volume_path).map_err(|_| VR_DOWNLOAD_CLEANUP_FAILED)?;
    if file.metadata().map_err(|_| VR_DOWNLOAD_STALE)?.nlink() != 0 {
        return Err(VR_DOWNLOAD_STALE);
    }
    sync_parent_directory(&volume_path);
    sync_parent_directory(path);
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Ok(CleanupDeletionOutcome::TargetAbsent)
        }
        Ok(_) => Ok(CleanupDeletionOutcome::ReplacementPreserved),
        Err(_) => Err(VR_DOWNLOAD_STALE),
    }
}

#[cfg(target_os = "macos")]
fn remove_macos_cleanup_staging_token(
    path: &Path,
    staging_token: &str,
    before_removal: &mut impl FnMut(&Path) -> Result<(), &'static str>,
) -> Result<CleanupDeletionOutcome, &'static str> {
    let Some(file) = open_exact_macos_cleanup_staging_token(path, staging_token)? else {
        return Ok(CleanupDeletionOutcome::TargetAbsent);
    };
    before_removal(path)?;
    remove_pinned_macos_cleanup_object(&file, path)
}

#[cfg(target_os = "macos")]
fn remove_exact_macos_cleanup_file(
    path: &Path,
    expected_fingerprint: &str,
    before_removal: &mut impl FnMut(&Path) -> Result<(), &'static str>,
) -> Result<Option<CleanupDeletionOutcome>, &'static str> {
    let file = open_exact_macos_cleanup_file(path, expected_fingerprint)?;
    before_removal(path)?;
    let current = fs::symlink_metadata(path).map_err(|_| VR_DOWNLOAD_STALE)?;
    let pinned = file.metadata().map_err(|_| VR_DOWNLOAD_STALE)?;
    if !macos_cleanup_metadata_matches(&pinned, &current) {
        return Ok(None);
    }
    remove_pinned_macos_cleanup_object(&file, path).map(Some)
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MacosCleanupMutationBoundary {
    StagingCreated,
    Exchanged,
    ExactDeleted,
    RolledBack,
    StagingRemoved,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MacosCleanupPreparationBoundary {
    Exchange,
    ExactDeletion,
    StagingTokenRemoval,
}

#[cfg(target_os = "macos")]
fn advance_macos_cleanup_mutation(
    persistence_path: &Path,
    mutation: &mut MacosCleanupMutation,
    phase: MacosCleanupMutationPhase,
    persist: &mut impl FnMut(&Path, &MacosCleanupMutation) -> Result<(), &'static str>,
) -> Result<(), &'static str> {
    let mut next = mutation.clone();
    next.phase = phase;
    persist(persistence_path, &next)?;
    *mutation = next;
    Ok(())
}

#[cfg(target_os = "macos")]
fn macos_cleanup_deletion_outcome(
    mutation: &MacosCleanupMutation,
) -> Result<CleanupDeletionOutcome, &'static str> {
    match macos_cleanup_path_state(
        &mutation.target_path,
        &mutation.expected_fingerprint,
        &mutation.staging_token,
    )? {
        MacosCleanupPathState::Absent => Ok(CleanupDeletionOutcome::TargetAbsent),
        MacosCleanupPathState::Other => Ok(CleanupDeletionOutcome::ReplacementPreserved),
        MacosCleanupPathState::ExactFile | MacosCleanupPathState::StagingToken => {
            Err(VR_DOWNLOAD_STALE)
        }
    }
}

#[cfg(target_os = "macos")]
fn persist_macos_cleanup_file_deleted(
    persistence_path: &Path,
    mutation: &MacosCleanupMutation,
    persist: &mut impl FnMut(&Path, &CleanupRecovery) -> Result<(), &'static str>,
) -> Result<(), &'static str> {
    let recovery_path = cleanup_recovery_path(persistence_path, &mutation.record)?;
    let mut recovery = parse_cleanup_recovery(persistence_path, &recovery_path)
        .ok_or(VR_DOWNLOAD_PERSISTENCE_FAILED)?;
    if !same_terminal_authority(&recovery.record, &mutation.record)
        || recovery.files.get(mutation.selected_index).is_none()
        || !matches!(
            recovery.files[mutation.selected_index],
            CleanupFileState::Present | CleanupFileState::Deleted
        )
    {
        return Err(VR_DOWNLOAD_PERSISTENCE_FAILED);
    }
    recovery.files[mutation.selected_index] = CleanupFileState::Deleted;
    persist(persistence_path, &recovery)
}

#[cfg(target_os = "macos")]
fn run_macos_cleanup_mutation_with(
    persistence_path: &Path,
    mutation: &mut MacosCleanupMutation,
    mut persist_mutation: impl FnMut(&Path, &MacosCleanupMutation) -> Result<(), &'static str>,
    mut persist_cleanup: impl FnMut(&Path, &CleanupRecovery) -> Result<(), &'static str>,
    mut remove_mutation: impl FnMut(&Path, &MacosCleanupMutation) -> Result<(), &'static str>,
    mut before_mutation: impl FnMut(MacosCleanupPreparationBoundary, &Path) -> Result<(), &'static str>,
    mut after_mutation: impl FnMut(MacosCleanupMutationBoundary) -> Result<(), &'static str>,
) -> Result<CleanupDeletionOutcome, &'static str> {
    use std::os::unix::fs::symlink;
    use MacosCleanupMutationPhase as Phase;
    use MacosCleanupPathState as PathState;

    for _ in 0..20 {
        let target_state = macos_cleanup_path_state(
            &mutation.target_path,
            &mutation.expected_fingerprint,
            &mutation.staging_token,
        )?;
        let staging_state = macos_cleanup_path_state(
            &mutation.staging_path,
            &mutation.expected_fingerprint,
            &mutation.staging_token,
        )?;
        match mutation.phase {
            Phase::StagingCreationPrepared => match (target_state, staging_state) {
                (PathState::ExactFile, PathState::Absent) => {
                    symlink(&mutation.staging_token, &mutation.staging_path)
                        .map_err(|_| VR_DOWNLOAD_CLEANUP_FAILED)?;
                    sync_parent_directory(&mutation.staging_path);
                    after_mutation(MacosCleanupMutationBoundary::StagingCreated)?;
                    advance_macos_cleanup_mutation(
                        persistence_path,
                        mutation,
                        Phase::StagingCreated,
                        &mut persist_mutation,
                    )?;
                }
                (PathState::ExactFile, PathState::StagingToken) => {
                    advance_macos_cleanup_mutation(
                        persistence_path,
                        mutation,
                        Phase::StagingCreated,
                        &mut persist_mutation,
                    )?;
                }
                _ => return Err(VR_DOWNLOAD_STALE),
            },
            Phase::StagingCreated => {
                if (target_state, staging_state) != (PathState::ExactFile, PathState::StagingToken)
                {
                    return Err(VR_DOWNLOAD_STALE);
                }
                advance_macos_cleanup_mutation(
                    persistence_path,
                    mutation,
                    Phase::ExchangePrepared,
                    &mut persist_mutation,
                )?;
            }
            Phase::ExchangePrepared => match (target_state, staging_state) {
                (PathState::ExactFile, PathState::StagingToken) => {
                    before_mutation(
                        MacosCleanupPreparationBoundary::Exchange,
                        &mutation.target_path,
                    )?;
                    exchange_macos_cleanup_paths(&mutation.target_path, &mutation.staging_path)
                        .map_err(|_| VR_DOWNLOAD_CLEANUP_FAILED)?;
                    sync_parent_directory(&mutation.target_path);
                    after_mutation(MacosCleanupMutationBoundary::Exchanged)?;
                    let target_state = macos_cleanup_path_state(
                        &mutation.target_path,
                        &mutation.expected_fingerprint,
                        &mutation.staging_token,
                    )?;
                    let staging_state = macos_cleanup_path_state(
                        &mutation.staging_path,
                        &mutation.expected_fingerprint,
                        &mutation.staging_token,
                    )?;
                    match (target_state, staging_state) {
                        (PathState::StagingToken, PathState::ExactFile)
                        | (PathState::Other | PathState::Absent, PathState::ExactFile) => {
                            advance_macos_cleanup_mutation(
                                persistence_path,
                                mutation,
                                Phase::Exchanged,
                                &mut persist_mutation,
                            )?;
                        }
                        (PathState::StagingToken, PathState::Other) => {
                            advance_macos_cleanup_mutation(
                                persistence_path,
                                mutation,
                                Phase::RollbackExchangePrepared,
                                &mut persist_mutation,
                            )?;
                        }
                        (PathState::Other | PathState::Absent, PathState::StagingToken) => {
                            advance_macos_cleanup_mutation(
                                persistence_path,
                                mutation,
                                Phase::RollbackStagingCleanupPrepared,
                                &mut persist_mutation,
                            )?;
                        }
                        _ => return Err(VR_DOWNLOAD_STALE),
                    }
                }
                (PathState::StagingToken, PathState::ExactFile)
                | (PathState::Other | PathState::Absent, PathState::ExactFile) => {
                    advance_macos_cleanup_mutation(
                        persistence_path,
                        mutation,
                        Phase::Exchanged,
                        &mut persist_mutation,
                    )?;
                    continue;
                }
                (PathState::StagingToken, PathState::Other) => {
                    advance_macos_cleanup_mutation(
                        persistence_path,
                        mutation,
                        Phase::RollbackExchangePrepared,
                        &mut persist_mutation,
                    )?;
                    continue;
                }
                (PathState::Other | PathState::Absent, PathState::StagingToken) => {
                    advance_macos_cleanup_mutation(
                        persistence_path,
                        mutation,
                        Phase::RollbackStagingCleanupPrepared,
                        &mut persist_mutation,
                    )?;
                    continue;
                }
                _ => return Err(VR_DOWNLOAD_STALE),
            },
            Phase::RollbackExchangePrepared => match (target_state, staging_state) {
                (PathState::StagingToken, PathState::Other) => {
                    exchange_macos_cleanup_paths(&mutation.target_path, &mutation.staging_path)
                        .map_err(|_| VR_DOWNLOAD_CLEANUP_FAILED)?;
                    sync_parent_directory(&mutation.target_path);
                    after_mutation(MacosCleanupMutationBoundary::RolledBack)?;
                    advance_macos_cleanup_mutation(
                        persistence_path,
                        mutation,
                        Phase::RolledBack,
                        &mut persist_mutation,
                    )?;
                }
                (PathState::Other, PathState::StagingToken) => {
                    advance_macos_cleanup_mutation(
                        persistence_path,
                        mutation,
                        Phase::RolledBack,
                        &mut persist_mutation,
                    )?;
                }
                _ => return Err(VR_DOWNLOAD_STALE),
            },
            Phase::RolledBack => {
                advance_macos_cleanup_mutation(
                    persistence_path,
                    mutation,
                    Phase::RollbackStagingCleanupPrepared,
                    &mut persist_mutation,
                )?;
            }
            Phase::RollbackStagingCleanupPrepared => {
                if target_state == PathState::StagingToken {
                    remove_macos_cleanup_staging_token(
                        &mutation.target_path,
                        &mutation.staging_token,
                        &mut |path| {
                            before_mutation(
                                MacosCleanupPreparationBoundary::StagingTokenRemoval,
                                path,
                            )
                        },
                    )?;
                    after_mutation(MacosCleanupMutationBoundary::StagingRemoved)?;
                }
                if staging_state == PathState::StagingToken {
                    remove_macos_cleanup_staging_token(
                        &mutation.staging_path,
                        &mutation.staging_token,
                        &mut |path| {
                            before_mutation(
                                MacosCleanupPreparationBoundary::StagingTokenRemoval,
                                path,
                            )
                        },
                    )?;
                    after_mutation(MacosCleanupMutationBoundary::StagingRemoved)?;
                }
                remove_mutation(persistence_path, mutation)?;
                return Err(VR_DOWNLOAD_STALE);
            }
            Phase::Exchanged => match staging_state {
                PathState::ExactFile => advance_macos_cleanup_mutation(
                    persistence_path,
                    mutation,
                    Phase::ExactDeletionPrepared,
                    &mut persist_mutation,
                )?,
                PathState::Absent | PathState::Other | PathState::StagingToken => {
                    advance_macos_cleanup_mutation(
                        persistence_path,
                        mutation,
                        Phase::RollbackStagingCleanupPrepared,
                        &mut persist_mutation,
                    )?;
                }
            },
            Phase::ExactDeletionPrepared => match staging_state {
                PathState::ExactFile => {
                    let Some(outcome) = remove_exact_macos_cleanup_file(
                        &mutation.staging_path,
                        &mutation.expected_fingerprint,
                        &mut |path| {
                            before_mutation(MacosCleanupPreparationBoundary::ExactDeletion, path)
                        },
                    )?
                    else {
                        return Err(VR_DOWNLOAD_STALE);
                    };
                    after_mutation(MacosCleanupMutationBoundary::ExactDeleted)?;
                    advance_macos_cleanup_mutation(
                        persistence_path,
                        mutation,
                        Phase::ExactDeleted,
                        &mut persist_mutation,
                    )?;
                    if outcome == CleanupDeletionOutcome::ReplacementPreserved {
                        return Err(VR_DOWNLOAD_STALE);
                    }
                }
                PathState::Absent => advance_macos_cleanup_mutation(
                    persistence_path,
                    mutation,
                    Phase::ExactDeleted,
                    &mut persist_mutation,
                )?,
                PathState::Other | PathState::StagingToken => advance_macos_cleanup_mutation(
                    persistence_path,
                    mutation,
                    Phase::RollbackStagingCleanupPrepared,
                    &mut persist_mutation,
                )?,
            },
            Phase::ExactDeleted => advance_macos_cleanup_mutation(
                persistence_path,
                mutation,
                Phase::StagingCleanupPrepared,
                &mut persist_mutation,
            )?,
            Phase::StagingCleanupPrepared => {
                if staging_state == PathState::ExactFile {
                    return Err(VR_DOWNLOAD_STALE);
                }
                let mut replacement_detected = false;
                if target_state == PathState::StagingToken {
                    replacement_detected |= remove_macos_cleanup_staging_token(
                        &mutation.target_path,
                        &mutation.staging_token,
                        &mut |path| {
                            before_mutation(
                                MacosCleanupPreparationBoundary::StagingTokenRemoval,
                                path,
                            )
                        },
                    )? == CleanupDeletionOutcome::ReplacementPreserved;
                    after_mutation(MacosCleanupMutationBoundary::StagingRemoved)?;
                }
                if staging_state == PathState::StagingToken {
                    replacement_detected |= remove_macos_cleanup_staging_token(
                        &mutation.staging_path,
                        &mutation.staging_token,
                        &mut |path| {
                            before_mutation(
                                MacosCleanupPreparationBoundary::StagingTokenRemoval,
                                path,
                            )
                        },
                    )? == CleanupDeletionOutcome::ReplacementPreserved;
                    after_mutation(MacosCleanupMutationBoundary::StagingRemoved)?;
                }
                if replacement_detected {
                    return Err(VR_DOWNLOAD_STALE);
                }
                persist_macos_cleanup_file_deleted(
                    persistence_path,
                    mutation,
                    &mut persist_cleanup,
                )?;
                let outcome = macos_cleanup_deletion_outcome(mutation)?;
                remove_mutation(persistence_path, mutation)?;
                return Ok(outcome);
            }
        }
    }
    Err(VR_DOWNLOAD_CLEANUP_FAILED)
}

#[cfg(target_os = "windows")]
#[repr(C)]
struct WindowsFileTime {
    _low: u32,
    _high: u32,
}

#[cfg(target_os = "windows")]
#[repr(C)]
struct WindowsFileInformation {
    _file_attributes: u32,
    _creation_time: WindowsFileTime,
    _last_access_time: WindowsFileTime,
    _last_write_time: WindowsFileTime,
    volume_serial_number: u32,
    _file_size_high: u32,
    _file_size_low: u32,
    _number_of_links: u32,
    file_index_high: u32,
    file_index_low: u32,
}

#[cfg(target_os = "windows")]
#[link(name = "Kernel32")]
extern "system" {
    #[link_name = "GetFileInformationByHandle"]
    fn get_file_information_by_handle(
        file: *mut std::ffi::c_void,
        information: *mut WindowsFileInformation,
    ) -> i32;

    #[link_name = "SetFileInformationByHandle"]
    fn set_file_information_by_handle(
        file: *mut std::ffi::c_void,
        information_class: i32,
        information: *const std::ffi::c_void,
        information_size: u32,
    ) -> i32;
}

#[cfg(target_os = "windows")]
pub(crate) fn open_file_fingerprint(file: &File) -> io::Result<String> {
    use std::{mem::MaybeUninit, os::windows::io::AsRawHandle};

    let mut information = MaybeUninit::<WindowsFileInformation>::uninit();
    // Rust's equivalent metadata methods are unstable, so use the stable Windows handle API.
    let succeeded = unsafe {
        get_file_information_by_handle(file.as_raw_handle().cast(), information.as_mut_ptr())
    };
    if succeeded == 0 {
        return Err(io::Error::last_os_error());
    }
    // The Windows API initialized the complete structure after reporting success.
    let information = unsafe { information.assume_init() };
    let file_index =
        (u64::from(information.file_index_high) << 32) | u64::from(information.file_index_low);
    Ok(format!("{}:{file_index}", information.volume_serial_number))
}

#[cfg(target_os = "windows")]
pub(crate) fn file_fingerprint(path: &Path) -> Result<String, &'static str> {
    let metadata = fs::symlink_metadata(path).map_err(|_| VR_FOLDER_UNAVAILABLE)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(VR_DOWNLOAD_DESTINATION_CONFLICT);
    }
    let file = File::open(path).map_err(|_| VR_FOLDER_UNAVAILABLE)?;
    open_file_fingerprint(&file).map_err(|_| VR_FOLDER_UNAVAILABLE)
}

#[cfg(target_os = "windows")]
#[repr(C)]
struct WindowsFileDispositionInformation {
    delete_file: i32,
}

#[cfg(target_os = "windows")]
fn delete_exact_windows_cleanup_file_with(
    target: &Path,
    expected_fingerprint: &str,
    before_disposition: impl FnOnce() -> io::Result<()>,
) -> Result<(), &'static str> {
    use std::{
        mem::size_of,
        os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    };

    const DELETE: u32 = 0x0001_0000;
    const FILE_READ_ATTRIBUTES: u32 = 0x0000_0080;
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const FILE_SHARE_DELETE: u32 = 0x0000_0004;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FILE_DISPOSITION_INFO_CLASS: i32 = 4;

    let metadata = fs::symlink_metadata(target).map_err(|_| VR_DOWNLOAD_STALE)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(VR_DOWNLOAD_STALE);
    }
    let file = OpenOptions::new()
        .access_mode(DELETE | FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(target)
        .map_err(|_| VR_DOWNLOAD_CLEANUP_FAILED)?;
    if open_file_fingerprint(&file).map_err(|_| VR_DOWNLOAD_STALE)? != expected_fingerprint {
        return Err(VR_DOWNLOAD_STALE);
    }
    before_disposition().map_err(|_| VR_DOWNLOAD_CLEANUP_FAILED)?;
    let disposition = WindowsFileDispositionInformation { delete_file: 1 };
    let succeeded = unsafe {
        set_file_information_by_handle(
            file.as_raw_handle().cast(),
            FILE_DISPOSITION_INFO_CLASS,
            (&disposition as *const WindowsFileDispositionInformation).cast(),
            size_of::<WindowsFileDispositionInformation>() as u32,
        )
    };
    if succeeded == 0 {
        return Err(VR_DOWNLOAD_CLEANUP_FAILED);
    }
    drop(file);
    match fs::symlink_metadata(target) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        _ => Err(VR_DOWNLOAD_STALE),
    }
}

#[cfg(target_os = "windows")]
fn delete_exact_windows_cleanup_file(
    target: &Path,
    expected_fingerprint: &str,
) -> Result<CleanupDeletionOutcome, &'static str> {
    delete_exact_windows_cleanup_file_with(target, expected_fingerprint, || Ok(()))?;
    Ok(CleanupDeletionOutcome::TargetAbsent)
}

#[cfg(not(any(unix, target_os = "windows")))]
pub(crate) fn file_fingerprint(path: &Path) -> Result<String, &'static str> {
    let metadata = fs::symlink_metadata(path).map_err(|_| VR_FOLDER_UNAVAILABLE)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(VR_DOWNLOAD_DESTINATION_CONFLICT);
    }
    Ok(format!("{}", metadata.len()))
}

fn capture_fingerprints(record: &TransferRecord) -> Result<Vec<String>, &'static str> {
    record
        .selected_files
        .iter()
        .enumerate()
        .map(|(index, _)| file_fingerprint(&current_target(record, index)?))
        .collect()
}

fn validate_resume_context(record: &TransferRecord) -> Result<(), &'static str> {
    if canonical_destination(&record.destination)? != record.destination
        || record.fingerprints.len() != record.selected_files.len()
    {
        return Err(VR_DOWNLOAD_STALE);
    }
    for (index, expected_fingerprint) in record.fingerprints.iter().enumerate() {
        let target = current_target(record, index)?;
        let metadata = fs::metadata(&target).map_err(|_| VR_FOLDER_UNAVAILABLE)?;
        if metadata.len() != record.selected_files[index].size
            || file_fingerprint(&target)? != *expected_fingerprint
        {
            return Err(VR_DOWNLOAD_STALE);
        }
    }
    Ok(())
}

async fn add_record_to_session(
    session: &Arc<Session>,
    record: &TransferRecord,
    resume: bool,
) -> Result<ManagedTorrentHandle, &'static str> {
    let selected_file_ids = record.selected_file_ids();
    let storage = SelectedFileStorageFactory::new(
        record.destination.clone(),
        &record.selected_files,
        record.boundary_segments.clone(),
        resume,
    );
    let response = session
        .add_torrent(
            AddTorrent::from_bytes(record.metainfo.clone()),
            Some(AddTorrentOptions {
                paused: true,
                only_files: Some(selected_file_ids),
                overwrite: resume,
                output_folder: Some(record.destination.to_string_lossy().into_owned()),
                storage_factory: Some(storage.boxed()),
                ..Default::default()
            }),
        )
        .await
        .map_err(|_| VR_DOWNLOAD_FAILED)?;
    let handle = match response {
        AddTorrentResponse::Added(_, handle) => handle,
        AddTorrentResponse::AlreadyManaged(_, _) | AddTorrentResponse::ListOnly(_) => {
            return Err(VR_DOWNLOAD_DUPLICATE);
        }
    };
    handle
        .wait_until_initialized()
        .await
        .map_err(|_| VR_DOWNLOAD_FAILED)?;
    Ok(handle)
}

fn find_valid_record_mut<'a>(
    transfers: &'a mut [StoredTransfer],
    transfer_id: &str,
) -> Option<&'a mut TransferRecord> {
    transfers.iter_mut().find_map(|transfer| match transfer {
        StoredTransfer::Valid(record) if record.transfer_id == transfer_id => Some(record),
        _ => None,
    })
}

fn has_active_duplicate(transfers: &[StoredTransfer], infohash: &str, destination: &Path) -> bool {
    transfers.iter().any(|transfer| match transfer {
        StoredTransfer::Valid(record) => {
            record.state.is_active()
                && record.infohash == infohash
                && record.destination == destination
        }
        StoredTransfer::Corrupt(_) => false,
    })
}

fn corrupt_transfer_may_hold_cleanup_authority(record: &CorruptTransferRecord) -> bool {
    let state = record
        .raw_line
        .split(|byte| *byte == b'\t')
        .nth(5)
        .and_then(|state| std::str::from_utf8(state).ok())
        .and_then(TransferState::from_str);
    state.is_none_or(|state| state == TransferState::Cleanup)
}

fn cleanup_record_start_conflict(
    cleanup: &TransferRecord,
    proposed: &TransferRecord,
    proposed_targets: &[PathBuf],
) -> Result<Option<&'static str>, &'static str> {
    if cleanup.transfer_id == proposed.transfer_id {
        return Ok(Some(VR_DOWNLOAD_DUPLICATE));
    }
    for target in proposed_targets {
        if transfer_record_owns_path(cleanup, target).map_err(|_| VR_DOWNLOAD_STALE)? {
            return Ok(Some(VR_DOWNLOAD_DESTINATION_CONFLICT));
        }
    }
    Ok(None)
}

fn validate_cleanup_start_reservations(
    context: &VrDownloadContext,
    persistence_path: &Path,
    proposed: &TransferRecord,
) -> Result<(), &'static str> {
    let recoveries = read_cleanup_recoveries(persistence_path).map_err(|_| VR_DOWNLOAD_STALE)?;
    #[cfg(target_os = "macos")]
    let macos_mutations =
        read_macos_cleanup_mutations(persistence_path).map_err(|_| VR_DOWNLOAD_STALE)?;
    let proposed_targets = proposed
        .selected_files
        .iter()
        .map(|file| selected_target(&proposed.destination, file))
        .collect::<Result<Vec<_>, _>>()?;

    for transfer in &context.transfers {
        match transfer {
            StoredTransfer::Valid(cleanup) if cleanup.state == TransferState::Cleanup => {
                if !recoveries.iter().any(|recovery| {
                    recovery.record.state == TransferState::Cleanup
                        && same_terminal_authority(cleanup, &recovery.record)
                }) {
                    return Err(VR_DOWNLOAD_STALE);
                }
                if let Some(error) =
                    cleanup_record_start_conflict(cleanup, proposed, &proposed_targets)?
                {
                    return Err(error);
                }
            }
            StoredTransfer::Corrupt(record)
                if corrupt_transfer_may_hold_cleanup_authority(record) =>
            {
                return Err(VR_DOWNLOAD_STALE);
            }
            StoredTransfer::Valid(_) | StoredTransfer::Corrupt(_) => {}
        }
    }
    for recovery in recoveries {
        if let Some(error) =
            cleanup_record_start_conflict(&recovery.record, proposed, &proposed_targets)?
        {
            return Err(error);
        }
    }
    #[cfg(target_os = "macos")]
    for mutation in macos_mutations {
        if mutation.record.transfer_id == proposed.transfer_id {
            return Err(VR_DOWNLOAD_DUPLICATE);
        }
        if proposed_targets
            .iter()
            .any(|target| target == &mutation.target_path || target == &mutation.staging_path)
        {
            return Err(VR_DOWNLOAD_DESTINATION_CONFLICT);
        }
    }
    Ok(())
}

fn finalize_monitored_transfer_with(
    context: &mut VrDownloadContext,
    transfer_id: &str,
    handle_generation: u64,
    completed: bool,
    persistence_path: &Path,
    mut persist: impl FnMut(&Path, &[StoredTransfer]) -> Result<(), &'static str>,
    mut persist_recovery: impl FnMut(&Path, &TransferRecord, u64) -> Result<(), &'static str>,
) -> bool {
    let (active_state, active_downloaded_bytes, active_handle, active_pending_action) = {
        let Some(record) = find_valid_record_mut(&mut context.transfers, transfer_id) else {
            return false;
        };
        if record.handle_generation != handle_generation || !record.state.is_active() {
            return false;
        }
        let active = (
            record.state,
            record.downloaded_bytes,
            record.handle.clone(),
            record.pending_action,
        );
        record.state = TransferState::Failed;
        record.pending_action = None;
        if completed {
            record.downloaded_bytes = record.selected_total();
        }
        active
    };

    let recovery_saved =
        find_valid_record_mut(&mut context.transfers, transfer_id).is_some_and(|record| {
            persist_recovery(persistence_path, record, handle_generation).is_ok()
        });
    if !recovery_saved {
        if persist(persistence_path, &context.transfers).is_ok() {
            let record = find_valid_record_mut(&mut context.transfers, transfer_id)
                .expect("the validated transfer must remain present");
            record.handle = None;
            record.terminal_recovery_generation = None;
            return true;
        }
        let record = find_valid_record_mut(&mut context.transfers, transfer_id)
            .expect("the validated transfer must remain present");
        record.state = active_state;
        record.downloaded_bytes = active_downloaded_bytes;
        record.handle = active_handle;
        record.pending_action = active_pending_action;
        return false;
    }

    {
        let record = find_valid_record_mut(&mut context.transfers, transfer_id)
            .expect("the validated transfer must remain present");
        record.terminal_recovery_generation = Some(handle_generation);
        if completed {
            record.state = TransferState::Completed;
        }
    }

    // Once the desired terminal state is durable, it remains authoritative even when cleanup of
    // the now-obsolete recovery record fails. Dismiss or a later successful list will retry cleanup.
    if persist(persistence_path, &context.transfers).is_ok() {
        let record = find_valid_record_mut(&mut context.transfers, transfer_id)
            .expect("the validated transfer must remain present");
        let _ = remove_terminal_recovery(persistence_path, record);
        record.handle = None;
        record.terminal_recovery_generation = None;
        return true;
    }

    if completed {
        find_valid_record_mut(&mut context.transfers, transfer_id)
            .expect("the validated transfer must remain present")
            .state = TransferState::Failed;
        if persist(persistence_path, &context.transfers).is_ok() {
            let record = find_valid_record_mut(&mut context.transfers, transfer_id)
                .expect("the validated transfer must remain present");
            let _ = remove_terminal_recovery(persistence_path, record);
            record.handle = None;
            record.terminal_recovery_generation = None;
            return true;
        }
    }

    // The exact failed recovery is durable even though the primary file is not. It is therefore
    // safe to stop the native handle and expose a non-running recovery-attention row.
    let record = find_valid_record_mut(&mut context.transfers, transfer_id)
        .expect("the validated transfer must remain present");
    record.state = TransferState::Failed;
    record.handle = None;
    true
}

fn spawn_completion_monitor(
    state: VrDownloadState,
    session: Arc<Session>,
    handle: ManagedTorrentHandle,
    transfer_id: String,
    handle_generation: u64,
    persistence_path: PathBuf,
) {
    tauri::async_runtime::spawn(async move {
        let result = handle.wait_until_completed().await;
        let should_detach = {
            let mut context = match state.0.lock() {
                Ok(context) => context,
                Err(_) => return,
            };
            finalize_monitored_transfer_with(
                &mut context,
                &transfer_id,
                handle_generation,
                result.is_ok(),
                &persistence_path,
                write_persisted_transfers,
                write_terminal_recovery,
            )
        };
        if should_detach {
            let _ = session.delete(handle.id().into(), false).await;
        }
    });
}

async fn restore_record(
    state: &VrDownloadState,
    session_folder: &Path,
    persistence_path: &Path,
    transfer_id: &str,
) {
    let session = match session_for(state, session_folder).await {
        Ok(session) => session,
        Err(_) => {
            if let Ok(mut context) = state.0.lock() {
                if let Some(record) = find_valid_record_mut(&mut context.transfers, transfer_id) {
                    record.state = TransferState::Offline;
                }
                let _ = write_persisted_transfers(persistence_path, &context.transfers);
            }
            return;
        }
    };
    let (record_snapshot, should_resume) = {
        let mut context = match state.0.lock() {
            Ok(context) => context,
            Err(_) => return,
        };
        let Some(record) = find_valid_record_mut(&mut context.transfers, transfer_id) else {
            return;
        };
        if validate_resume_context(record).is_err() {
            record.state = TransferState::Offline;
            let _ = write_persisted_transfers(persistence_path, &context.transfers);
            return;
        }
        let should_resume = record.state != TransferState::Paused;
        let snapshot = TransferRecord {
            transfer_id: record.transfer_id.clone(),
            category: record.category,
            code: record.code.clone(),
            release_name: record.release_name.clone(),
            movie_identity: record.movie_identity.clone(),
            tv_identity: record.tv_identity.clone(),
            infohash: record.infohash.clone(),
            metainfo: record.metainfo.clone(),
            selected_files: record.selected_files.clone(),
            destination: record.destination.clone(),
            fingerprints: record.fingerprints.clone(),
            current_paths: record.current_paths.clone(),
            organization_state: record.organization_state,
            state: record.state,
            downloaded_bytes: record.downloaded_bytes,
            boundary_segments: record.boundary_segments.clone(),
            handle: None,
            handle_generation: record.handle_generation,
            terminal_recovery_generation: record.terminal_recovery_generation,
            pending_action: None,
        };
        (snapshot, should_resume)
    };

    let handle = match add_record_to_session(&session, &record_snapshot, true).await {
        Ok(handle) => handle,
        Err(_) => {
            if let Ok(mut context) = state.0.lock() {
                if let Some(record) = find_valid_record_mut(&mut context.transfers, transfer_id) {
                    record.state = TransferState::Offline;
                    record.downloaded_bytes = 0;
                }
                let _ = write_persisted_transfers(persistence_path, &context.transfers);
            }
            return;
        }
    };
    let restored_downloaded_bytes = verified_selected_bytes(&record_snapshot, &handle)
        .unwrap_or_default()
        .min(record_snapshot.selected_total());
    if restored_downloaded_bytes < record_snapshot.downloaded_bytes {
        let _ = session.delete(handle.id().into(), false).await;
        if let Ok(mut context) = state.0.lock() {
            if let Some(record) = find_valid_record_mut(&mut context.transfers, transfer_id) {
                record.state = TransferState::Offline;
                record.downloaded_bytes = restored_downloaded_bytes;
                record.handle = None;
                record.pending_action = None;
            }
            let _ = write_persisted_transfers(persistence_path, &context.transfers);
        }
        return;
    }

    let handle_generation = {
        let mut context = match state.0.lock() {
            Ok(context) => context,
            Err(_) => return,
        };
        let Some(record) = find_valid_record_mut(&mut context.transfers, transfer_id) else {
            return;
        };
        record.downloaded_bytes = restored_downloaded_bytes;
        record.handle_generation = record.handle_generation.wrapping_add(1);
        record.handle = Some(handle.clone());
        record.state = if should_resume {
            TransferState::Downloading
        } else {
            TransferState::Paused
        };
        let generation = record.handle_generation;
        let _ = write_persisted_transfers(persistence_path, &context.transfers);
        generation
    };
    if should_resume && session.unpause(&handle).await.is_err() {
        let terminal_saved = state.0.lock().is_ok_and(|mut context| {
            finalize_monitored_transfer_with(
                &mut context,
                transfer_id,
                handle_generation,
                false,
                persistence_path,
                write_persisted_transfers,
                write_terminal_recovery,
            )
        });
        if terminal_saved {
            let _ = session.delete(handle.id().into(), false).await;
        }
        return;
    }
    spawn_completion_monitor(
        state.clone(),
        session,
        handle,
        transfer_id.to_owned(),
        handle_generation,
        persistence_path.to_owned(),
    );
}

async fn load_downloads_with_persistence(
    state: &VrDownloadState,
    persistence_path: &Path,
    session_folder: &Path,
    download_limit_path: &Path,
    persist_transfers: fn(&Path, &[StoredTransfer]) -> Result<(), &'static str>,
) -> Result<Vec<String>, &'static str> {
    load_download_limit(state, download_limit_path)?;
    let recovery_destinations = {
        let mut context = state.0.lock().map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
        if context
            .persistence_path
            .as_deref()
            .is_some_and(|current| current != persistence_path)
        {
            return Err(VR_DOWNLOAD_ACTION_INVALID);
        }
        context.persistence_path = Some(persistence_path.to_owned());
        if context.transfers_loaded {
            return Ok(download_rows(&mut context));
        }
        if context.transfers_loading {
            return Err(VR_DOWNLOAD_ACTION_INVALID);
        }
        context.transfers_loading = true;
        [
            (TransferCategory::Vr, context.future_folder.clone()),
            (TransferCategory::Adult, context.adult_future_folder.clone()),
            (TransferCategory::Movie, context.movie_future_folder.clone()),
            (TransferCategory::Tv, context.tv_future_folder.clone()),
        ]
    };
    let persisted_transfers = read_persisted_transfers(persistence_path);
    let mut recovered_transfer_ids = BTreeSet::new();
    let recoveries = recovery_destinations
        .iter()
        .cloned()
        .filter_map(|(category, destination)| {
            destination.map(|destination| (category, destination))
        })
        .flat_map(|(category, destination)| {
            read_organization_recoveries(&destination)
                .into_iter()
                .filter(move |record| record.category == category)
        })
        .filter(|record| recovered_transfer_ids.insert(record.transfer_id.clone()))
        .collect::<Vec<_>>();
    let terminal_recoveries = match read_terminal_recoveries(persistence_path) {
        Ok(recoveries) => recoveries,
        Err(error) => {
            state
                .0
                .lock()
                .map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?
                .transfers_loading = false;
            return Err(error);
        }
    };
    let cleanup_recoveries = match read_cleanup_recoveries(persistence_path) {
        Ok(recoveries) => recoveries,
        Err(error) => {
            state
                .0
                .lock()
                .map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?
                .transfers_loading = false;
            return Err(error);
        }
    };
    let has_durable_recovery =
        !recoveries.is_empty() || !terminal_recoveries.is_empty() || !cleanup_recoveries.is_empty();
    let mut transfers = match persisted_transfers {
        Ok(transfers) => transfers,
        Err(_) if has_durable_recovery => Vec::new(),
        Err(error) => {
            state
                .0
                .lock()
                .map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?
                .transfers_loading = false;
            return Err(error);
        }
    };
    for recovered in recoveries {
        let existing = transfers.iter().position(|transfer| {
            matches!(transfer, StoredTransfer::Valid(record) if record.transfer_id == recovered.transfer_id)
        });
        match existing {
            Some(index) => {
                let StoredTransfer::Valid(record) = &transfers[index] else {
                    continue;
                };
                if validate_resume_context(record).is_err() {
                    transfers[index] = StoredTransfer::Valid(recovered);
                }
            }
            None if transfers.len() < MAX_PERSISTED_TRANSFERS => {
                transfers.push(StoredTransfer::Valid(recovered));
            }
            None => {}
        }
    }
    for recovered in terminal_recoveries {
        let existing = transfers.iter().position(|transfer| {
            matches!(transfer, StoredTransfer::Valid(record) if record.transfer_id == recovered.transfer_id)
        });
        match existing {
            Some(index) => {
                let StoredTransfer::Valid(record) = &transfers[index] else {
                    continue;
                };
                if record.state.is_active() && same_transfer_authority(record, &recovered) {
                    transfers[index] = StoredTransfer::Valid(recovered);
                }
            }
            None if transfers.len() < MAX_PERSISTED_TRANSFERS => {
                transfers.push(StoredTransfer::Valid(recovered));
            }
            None => {}
        }
    }
    for recovery in cleanup_recoveries {
        if recovery.files.len() != recovery.record.selected_files.len() {
            return Err(VR_DOWNLOAD_PERSISTENCE_FAILED);
        }
        let existing = transfers.iter().position(|transfer| {
            matches!(transfer, StoredTransfer::Valid(record) if record.transfer_id == recovery.record.transfer_id)
        });
        match existing {
            Some(index) => {
                let StoredTransfer::Valid(record) = &transfers[index] else {
                    continue;
                };
                if !same_cleanup_record_identity(record, &recovery.record) {
                    state
                        .0
                        .lock()
                        .map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?
                        .transfers_loading = false;
                    return Err(VR_DOWNLOAD_PERSISTENCE_FAILED);
                }
                transfers[index] = StoredTransfer::Valid(recovery.record);
            }
            None if transfers.len() < MAX_PERSISTED_TRANSFERS => {
                transfers.push(StoredTransfer::Valid(recovery.record));
            }
            None => {}
        }
    }
    let active_ids = {
        let mut context = state.0.lock().map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
        context.transfers_loading = false;
        context.transfers = transfers;
        context.transfers_loaded = true;
        for transfer in &mut context.transfers {
            if let StoredTransfer::Valid(record) = transfer {
                if record.state != TransferState::Cleanup
                    && validate_resume_context(record).is_err()
                {
                    if record.organization_state == OrganizationState::None {
                        record.state = TransferState::Offline;
                    } else {
                        record.organization_state = OrganizationState::Attention;
                    }
                    record.handle = None;
                    record.pending_action = None;
                }
            }
        }
        context
            .transfers
            .iter()
            .filter_map(|transfer| match transfer {
                StoredTransfer::Valid(record) if record.state.is_active() => {
                    Some(record.transfer_id.clone())
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    for transfer_id in active_ids {
        restore_record(state, session_folder, persistence_path, &transfer_id).await;
    }
    list_downloads_with_persistence(state, persistence_path, persist_transfers)
}

pub async fn load_downloads(
    state: &VrDownloadState,
    persistence_path: &Path,
    session_folder: &Path,
    download_limit_path: &Path,
) -> Result<Vec<String>, &'static str> {
    load_downloads_with_persistence(
        state,
        persistence_path,
        session_folder,
        download_limit_path,
        write_persisted_transfers,
    )
    .await
}

fn verified_selected_bytes(record: &TransferRecord, handle: &ManagedTorrentHandle) -> Option<u64> {
    let stats = handle.stats();
    (!stats.file_progress.is_empty()).then(|| {
        record
            .selected_files
            .iter()
            .map(|file| {
                stats
                    .file_progress
                    .get(file.file_id)
                    .copied()
                    .unwrap_or_default()
                    .min(file.size)
            })
            .sum()
    })
}

fn selected_progress(record: &TransferRecord, handle: &ManagedTorrentHandle) -> (u64, u64) {
    let stats = handle.stats();
    let downloaded = verified_selected_bytes(record, handle).unwrap_or(record.downloaded_bytes);
    let speed = stats
        .live
        .map(|live| (live.download_speed.mbps.max(0.0) * 1024.0 * 1024.0) as u64)
        .unwrap_or_default();
    (downloaded.min(record.selected_total()), speed)
}

fn exact_part_label(file_name: &str, category: TransferCategory) -> Option<String> {
    let title = Path::new(file_name).file_stem()?.to_str()?;
    let bytes = title.as_bytes();
    let mut matches = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if index > 0 && bytes[index - 1].is_ascii_alphanumeric() {
            index += 1;
            continue;
        }
        let label_start = index;
        while index < bytes.len() && bytes[index].is_ascii_alphabetic() {
            index += 1;
        }
        let prefix = std::str::from_utf8(&bytes[label_start..index])
            .ok()?
            .to_ascii_uppercase();
        if !(matches!(prefix.as_str(), "PART" | "CD" | "DISC" | "DISK")
            || category == TransferCategory::Vr && prefix == "PT")
        {
            index = label_start + 1;
            continue;
        }
        while index < bytes.len() && matches!(bytes[index], b' ' | b'_' | b'-') {
            index += 1;
        }
        let number_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        let number = std::str::from_utf8(&bytes[number_start..index]).ok()?;
        let significant_number = number.trim_start_matches('0');
        if significant_number.is_empty()
            || significant_number.len() > 4
            || (index < bytes.len() && bytes[index].is_ascii_alphanumeric())
        {
            index = label_start + 1;
            continue;
        }
        if category == TransferCategory::Adult {
            let mut continuation_index = index;
            while continuation_index < bytes.len()
                && bytes[continuation_index].is_ascii()
                && !bytes[continuation_index].is_ascii_alphanumeric()
            {
                continuation_index += 1;
            }
            if continuation_index > index
                && continuation_index < bytes.len()
                && bytes[continuation_index].is_ascii_digit()
            {
                return None;
            }
        }
        matches.push((
            significant_number.to_owned(),
            title[label_start..index].to_owned(),
        ));
    }
    let numbers = matches
        .iter()
        .map(|(number, _)| number.as_str())
        .collect::<BTreeSet<_>>();
    (numbers.len() == 1).then(|| matches[0].1.clone())
}

fn validate_current_organization_file(
    record: &TransferRecord,
    selected_index: usize,
) -> Result<PathBuf, &'static str> {
    let path = current_target(record, selected_index).map_err(|_| VR_ORGANIZATION_STALE)?;
    let metadata = fs::symlink_metadata(&path).map_err(|_| VR_ORGANIZATION_STALE)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(VR_ORGANIZATION_STALE);
    }
    if metadata.len() != record.selected_files[selected_index].size
        || file_fingerprint(&path).map_err(|_| VR_ORGANIZATION_STALE)?
            != record.fingerprints[selected_index]
    {
        return Err(VR_ORGANIZATION_STALE);
    }
    let canonical = fs::canonicalize(&path).map_err(|_| VR_ORGANIZATION_STALE)?;
    if canonical != path || !canonical.starts_with(&record.destination) {
        return Err(VR_ORGANIZATION_STALE);
    }
    Ok(path)
}

fn validate_movie_organization_identity(record: &TransferRecord) -> Result<(), &'static str> {
    let identity = record
        .movie_identity
        .as_deref()
        .ok_or(VR_ORGANIZATION_INELIGIBLE)?;
    let source = revalidate_persisted_movie_download_source(
        &record.metainfo,
        identity,
        &record.infohash,
        &record.selected_file_ids(),
    )
    .map_err(|_| VR_ORGANIZATION_INELIGIBLE)?;
    if source.selected_files != record.selected_files
        || source.release_name != record.release_name
        || source.movie_identity.as_ref() != Some(identity)
        || transfer_identity(record.category, &source, &record.destination) != record.transfer_id
    {
        return Err(VR_ORGANIZATION_INELIGIBLE);
    }
    Ok(())
}

fn validate_tv_organization_identity(record: &TransferRecord) -> Result<(), &'static str> {
    let identity = record
        .tv_identity
        .as_deref()
        .ok_or(VR_ORGANIZATION_INELIGIBLE)?;
    let source = revalidate_persisted_tv_download_source(
        &record.metainfo,
        identity,
        &record.infohash,
        &record.selected_file_ids(),
    )
    .map_err(|_| VR_ORGANIZATION_INELIGIBLE)?;
    if source.selected_files != record.selected_files
        || source.release_name != record.release_name
        || source.tv_identity.as_ref() != Some(identity)
        || transfer_identity(record.category, &source, &record.destination) != record.transfer_id
    {
        return Err(VR_ORGANIZATION_INELIGIBLE);
    }
    Ok(())
}

fn movie_release_year(release_date: &str) -> Option<&str> {
    let bytes = release_date.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| !matches!(index, 4 | 7) && !byte.is_ascii_digit())
    {
        return None;
    }
    let year = release_date[..4].parse::<u16>().ok()?;
    let month = release_date[5..7].parse::<u8>().ok()?;
    let day = release_date[8..].parse::<u8>().ok()?;
    let leap_year =
        year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap_year => 29,
        2 => 28,
        _ => return None,
    };
    (year > 0 && day > 0 && day <= days).then_some(&release_date[..4])
}

fn validate_organization_component_length(value: &str) -> Result<(), &'static str> {
    if value.len() > 255 || value.encode_utf16().count() > 255 {
        Err(VR_ORGANIZATION_INELIGIBLE)
    } else {
        Ok(())
    }
}

pub(crate) fn validate_portable_organization_component(value: &str) -> Result<(), &'static str> {
    let reserved_base = value.split('.').next().unwrap_or(value);
    let reserved_name = reserved_base.to_ascii_uppercase();
    let is_reserved = matches!(
        reserved_name.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "CLOCK$"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "COM¹"
            | "COM²"
            | "COM³"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
            | "LPT¹"
            | "LPT²"
            | "LPT³"
    );
    if value.is_empty()
        || matches!(value, "." | "..")
        || value.ends_with(' ')
        || value.ends_with('.')
        || is_reserved
        || value
            .chars()
            .any(|character| character.is_control() || r#"<>:"/\|?*"#.contains(character))
    {
        return Err(VR_ORGANIZATION_INELIGIBLE);
    }
    validate_organization_component_length(value)
}

fn portable_movie_organization_directory(
    identity: &MovieDownloadIdentity,
) -> Result<String, &'static str> {
    let title = identity.tmdb_title.as_str();
    validate_portable_organization_component(title)?;
    let year = identity
        .release_date
        .as_deref()
        .and_then(movie_release_year)
        .ok_or(VR_ORGANIZATION_INELIGIBLE)?;
    let directory = format!("{title} ({year})");
    validate_portable_organization_component(&directory)?;
    relative_file_path(&directory).map_err(|_| VR_ORGANIZATION_INELIGIBLE)?;
    Ok(directory)
}

fn tv_organization_directory(
    identity: &TvDownloadIdentity,
    requires_portable_episode_name: bool,
) -> Result<String, &'static str> {
    validate_portable_organization_component(&identity.show_name)?;
    if requires_portable_episode_name {
        validate_portable_organization_component(&identity.episode_name)?;
    }
    let season_directory = format!("Season {:02}", identity.season_number);
    validate_portable_organization_component(&season_directory)?;
    Ok(format!("{}/{season_directory}", identity.show_name))
}

fn organization_identity(record: &TransferRecord) -> Result<String, &'static str> {
    match record.category {
        TransferCategory::Adult | TransferCategory::Vr => Ok(record.code.clone()),
        TransferCategory::Movie => record
            .movie_identity
            .as_ref()
            .map(|identity| identity.imdb_id.clone())
            .ok_or(VR_ORGANIZATION_INELIGIBLE),
        TransferCategory::Tv => record
            .tv_identity
            .as_ref()
            .map(|identity| {
                format!(
                    "{} · S{:02}E{:02}",
                    identity.imdb_id, identity.season_number, identity.episode_number
                )
            })
            .ok_or(VR_ORGANIZATION_INELIGIBLE),
    }
}

fn organization_directory_name(record: &TransferRecord) -> Result<String, &'static str> {
    match record.category {
        TransferCategory::Adult | TransferCategory::Vr => Ok(record.code.clone()),
        TransferCategory::Movie => portable_movie_organization_directory(
            record
                .movie_identity
                .as_deref()
                .ok_or(VR_ORGANIZATION_INELIGIBLE)?,
        ),
        TransferCategory::Tv => {
            let media_count = record
                .selected_files
                .iter()
                .filter(|file| is_supported_transfer_media(Path::new(&file.path)))
                .count();
            tv_organization_directory(
                record
                    .tv_identity
                    .as_deref()
                    .ok_or(VR_ORGANIZATION_INELIGIBLE)?,
                media_count == 1,
            )
        }
    }
}

fn organization_collision_key(value: &str) -> String {
    value.nfd().flat_map(char::to_lowercase).nfd().collect()
}

fn validate_organization_directory(
    destination: &Path,
    directory_name: &str,
) -> Result<Option<PathBuf>, &'static str> {
    let relative = relative_file_path(directory_name).map_err(|_| VR_ORGANIZATION_INELIGIBLE)?;
    let mut parent = destination.to_owned();
    for component in relative.components() {
        let Component::Normal(expected) = component else {
            return Err(VR_ORGANIZATION_INELIGIBLE);
        };
        let expected = expected.to_str().ok_or(VR_ORGANIZATION_INELIGIBLE)?;
        let expected_key = organization_collision_key(expected);
        for entry in fs::read_dir(&parent).map_err(|_| VR_ORGANIZATION_INELIGIBLE)? {
            let entry = entry.map_err(|_| VR_ORGANIZATION_INELIGIBLE)?;
            let name = entry
                .file_name()
                .to_str()
                .ok_or(VR_ORGANIZATION_CONFLICT)?
                .to_owned();
            if organization_collision_key(&name) == expected_key && name != expected {
                return Err(VR_ORGANIZATION_CONFLICT);
            }
        }
        let directory = parent.join(expected);
        match fs::symlink_metadata(&directory) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(VR_ORGANIZATION_CONFLICT);
            }
            Ok(_) => {
                let canonical =
                    fs::canonicalize(&directory).map_err(|_| VR_ORGANIZATION_CONFLICT)?;
                if canonical != directory || !canonical.starts_with(destination) {
                    return Err(VR_ORGANIZATION_CONFLICT);
                }
                parent = directory;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(VR_ORGANIZATION_CONFLICT),
        }
    }
    Ok(Some(parent))
}

fn destination_has_case_collision(directory: &Path, file_name: &str) -> Result<bool, &'static str> {
    let expected = organization_collision_key(file_name);
    for entry in fs::read_dir(directory).map_err(|_| VR_ORGANIZATION_CONFLICT)? {
        let entry = entry.map_err(|_| VR_ORGANIZATION_CONFLICT)?;
        let existing = entry
            .file_name()
            .to_str()
            .ok_or(VR_ORGANIZATION_CONFLICT)?
            .to_owned();
        if organization_collision_key(&existing) == expected {
            return Ok(true);
        }
    }
    Ok(false)
}

fn organization_destination_relative(
    record: &TransferRecord,
    selected_index: usize,
    eligible_media: usize,
) -> Result<Option<String>, &'static str> {
    let original_relative = record
        .selected_files
        .get(selected_index)
        .map(|file| file.path.as_str())
        .ok_or(VR_ORGANIZATION_STALE)?;
    if !is_supported_transfer_media(Path::new(original_relative)) {
        return Ok(None);
    }
    let source_name = Path::new(original_relative)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(VR_ORGANIZATION_STALE)?;
    let source_title = Path::new(source_name)
        .file_stem()
        .and_then(|title| title.to_str())
        .ok_or(VR_ORGANIZATION_STALE)?;
    let extension = Path::new(source_name)
        .extension()
        .and_then(|extension| extension.to_str())
        .ok_or(VR_ORGANIZATION_STALE)?;
    let directory_name = organization_directory_name(record)?;
    let destination_name = match record.category {
        TransferCategory::Movie if eligible_media == 1 => {
            let destination_name = format!("{directory_name}.{extension}");
            validate_organization_component_length(&destination_name)?;
            destination_name
        }
        TransferCategory::Movie => source_name.to_owned(),
        TransferCategory::Adult | TransferCategory::Vr => {
            let identity_matches = match record.category {
                TransferCategory::Adult => {
                    adult_media_name_matches_product_code(source_title, &record.code)
                }
                TransferCategory::Vr => media_name_matches_product_code(source_title, &record.code),
                TransferCategory::Movie => unreachable!(),
                TransferCategory::Tv => unreachable!(),
            };
            if !identity_matches {
                return Err(VR_ORGANIZATION_INELIGIBLE);
            }
            if eligible_media == 1 {
                format!("{}.{}", record.code, extension)
            } else if let Some(part_label) = exact_part_label(source_name, record.category) {
                format!("{} - {}.{}", record.code, part_label, extension)
            } else {
                source_name.to_owned()
            }
        }
        TransferCategory::Tv if eligible_media == 1 => {
            let identity = record
                .tv_identity
                .as_deref()
                .ok_or(VR_ORGANIZATION_INELIGIBLE)?;
            let destination_name = format!(
                "{} - S{:02}E{:02} - {}.{extension}",
                identity.show_name,
                identity.season_number,
                identity.episode_number,
                identity.episode_name
            );
            validate_portable_organization_component(&destination_name)?;
            destination_name
        }
        TransferCategory::Tv => {
            validate_portable_organization_component(source_name)?;
            source_name.to_owned()
        }
    };
    let destination_relative = format!("{directory_name}/{destination_name}");
    relative_file_path(&destination_relative).map_err(|_| VR_ORGANIZATION_CONFLICT)?;
    if record.category == TransferCategory::Tv {
        let identity = record
            .tv_identity
            .as_deref()
            .ok_or(VR_ORGANIZATION_INELIGIBLE)?;
        let parsed =
            parse_tv_relative_identity(&destination_relative).ok_or(VR_ORGANIZATION_INELIGIBLE)?;
        if parsed.show_title != identity.show_name
            || parsed.season != identity.season_number
            || parsed.episode != identity.episode_number
        {
            return Err(VR_ORGANIZATION_INELIGIBLE);
        }
    }
    Ok(Some(destination_relative))
}

fn organization_entries(
    record: &TransferRecord,
    current_folder: Option<&Path>,
) -> Result<Vec<OrganizationEntry>, &'static str> {
    if record.state != TransferState::Completed
        || record.handle.is_some()
        || record.pending_action.is_some()
        || record.organization_state == OrganizationState::Organized
        || current_folder != Some(record.destination.as_path())
        || canonical_destination(&record.destination).ok().as_deref()
            != Some(record.destination.as_path())
        || record.current_paths.len() != record.selected_files.len()
        || record.fingerprints.len() != record.selected_files.len()
    {
        return Err(VR_ORGANIZATION_INELIGIBLE);
    }
    match record.category {
        TransferCategory::Movie => validate_movie_organization_identity(record)?,
        TransferCategory::Tv => validate_tv_organization_identity(record)?,
        TransferCategory::Adult | TransferCategory::Vr => {}
    }

    let eligible_media = record
        .selected_files
        .iter()
        .filter(|file| is_supported_transfer_media(Path::new(&file.path)))
        .count();
    if eligible_media == 0 {
        return Err(VR_ORGANIZATION_INELIGIBLE);
    }
    let directory_name = organization_directory_name(record)?;
    let existing_directory = validate_organization_directory(&record.destination, &directory_name)?;
    let current_paths = record
        .current_paths
        .iter()
        .map(|path| organization_collision_key(path))
        .collect::<BTreeSet<_>>();
    let mut proposed_paths = BTreeSet::new();
    let mut entries = Vec::with_capacity(record.selected_files.len());

    for (selected_index, source_relative) in record.current_paths.iter().enumerate() {
        validate_current_organization_file(record, selected_index)?;
        let Some(destination_relative) =
            organization_destination_relative(record, selected_index, eligible_media)?
        else {
            entries.push(OrganizationEntry {
                selected_index,
                kind: OrganizationEntryKind::NonMediaUnchanged,
                source_relative: source_relative.clone(),
                destination_relative: None,
            });
            continue;
        };
        let destination_name = Path::new(&destination_relative)
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(VR_ORGANIZATION_CONFLICT)?;
        let destination_key = organization_collision_key(&destination_relative);
        if !proposed_paths.insert(destination_key.clone()) {
            return Err(VR_ORGANIZATION_CONFLICT);
        }
        let same_path = source_relative == &destination_relative;
        if !same_path && current_paths.contains(&destination_key) {
            return Err(VR_ORGANIZATION_CONFLICT);
        }
        if !same_path
            && existing_directory.as_ref().is_some_and(|directory| {
                destination_has_case_collision(directory, destination_name).unwrap_or(true)
            })
        {
            return Err(VR_ORGANIZATION_CONFLICT);
        }
        entries.push(OrganizationEntry {
            selected_index,
            kind: if same_path {
                OrganizationEntryKind::MediaUnchanged
            } else {
                OrganizationEntryKind::Move
            },
            source_relative: source_relative.clone(),
            destination_relative: Some(destination_relative),
        });
    }
    Ok(entries)
}

fn organization_plan_id(
    generation: u64,
    record: &TransferRecord,
    entries: &[OrganizationEntry],
) -> Result<String, &'static str> {
    let mut identity = generation.to_be_bytes().to_vec();
    identity_field(&mut identity, record.transfer_id.as_bytes());
    identity_field(&mut identity, record.category.as_str().as_bytes());
    identity_field(&mut identity, encoded_boundary_segments(record)?.as_bytes());
    for entry in entries {
        identity.extend_from_slice(&(entry.selected_index as u64).to_be_bytes());
        identity_field(&mut identity, entry.kind.as_str().as_bytes());
        identity_field(&mut identity, entry.source_relative.as_bytes());
        identity_field(
            &mut identity,
            entry
                .destination_relative
                .as_deref()
                .unwrap_or("")
                .as_bytes(),
        );
        identity_field(
            &mut identity,
            record.fingerprints[entry.selected_index].as_bytes(),
        );
    }
    Ok(format!("{generation}-{}", hex_sha1(&identity)))
}

fn organization_plan_response(plan: &OrganizationPlan) -> Vec<String> {
    let move_count = plan
        .entries
        .iter()
        .filter(|entry| entry.kind == OrganizationEntryKind::Move)
        .count();
    let mut response = vec![
        plan.plan_id.clone(),
        plan.transfer_id.clone(),
        plan.identity.clone(),
        move_count.to_string(),
        plan.entries.len().to_string(),
    ];
    for entry in &plan.entries {
        response.extend([
            entry.kind.as_str().to_owned(),
            entry.source_relative.clone(),
            entry.destination_relative.clone().unwrap_or_default(),
        ]);
    }
    response
}

pub fn preview_organization(
    state: &VrDownloadState,
    transfer_id: &str,
) -> Result<Vec<String>, &'static str> {
    let mut context = state.0.lock().map_err(|_| VR_ORGANIZATION_FAILED)?;
    invalidate_organization_plan(&mut context);
    let generation = context.organization_generation;
    let record = context
        .transfers
        .iter()
        .find_map(|transfer| match transfer {
            StoredTransfer::Valid(record) if record.transfer_id == transfer_id => Some(record),
            _ => None,
        })
        .ok_or(VR_ORGANIZATION_STALE)?;
    let entries = organization_entries(record, configured_folder(&context, record.category))?;
    let plan = OrganizationPlan {
        plan_id: organization_plan_id(generation, record, &entries)
            .map_err(|_| VR_ORGANIZATION_FAILED)?,
        generation,
        transfer_id: record.transfer_id.clone(),
        category: record.category,
        identity: organization_identity(record)?,
        entries,
    };
    let response = organization_plan_response(&plan);
    context.organization_plan = Some(plan);
    Ok(response)
}

pub fn dismiss_organization(state: &VrDownloadState) -> Result<(), &'static str> {
    let mut context = state.0.lock().map_err(|_| VR_ORGANIZATION_FAILED)?;
    invalidate_organization_plan(&mut context);
    Ok(())
}

#[cfg(target_os = "macos")]
pub(crate) fn rename_without_overwrite(source: &Path, destination: &Path) -> io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};

    unsafe extern "C" {
        fn renamex_np(
            from: *const std::ffi::c_char,
            to: *const std::ffi::c_char,
            flags: u32,
        ) -> std::ffi::c_int;
    }

    let source = CString::new(source.as_os_str().as_bytes())?;
    let destination = CString::new(destination.as_os_str().as_bytes())?;
    // RENAME_EXCL makes the final mutation fail instead of replacing a raced destination.
    if unsafe { renamex_np(source.as_ptr(), destination.as_ptr(), 0x0000_0004) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn rename_without_overwrite(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn MoveFileW(existing: *const u16, new: *const u16) -> i32;
    }

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    if unsafe { MoveFileW(source.as_ptr(), destination.as_ptr()) } != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn rename_without_overwrite(source: &Path, destination: &Path) -> io::Result<()> {
    match fs::symlink_metadata(destination) {
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "target exists",
            ))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    fs::rename(source, destination)
}

fn rollback_organization_moves(
    moved: &[(usize, PathBuf, PathBuf)],
    original_paths: &[String],
    current_paths: &mut [String],
    move_file: &mut impl FnMut(&Path, &Path) -> io::Result<()>,
) -> bool {
    let mut restored = true;
    for (selected_index, source, destination) in moved.iter().rev() {
        if move_file(destination, source).is_ok() {
            current_paths[*selected_index] = original_paths[*selected_index].clone();
        } else {
            restored = false;
        }
    }
    restored
}

fn organization_directory_paths(
    destination: &Path,
    directory_name: &str,
) -> Result<Vec<PathBuf>, &'static str> {
    let relative = relative_file_path(directory_name).map_err(|_| VR_ORGANIZATION_INELIGIBLE)?;
    let mut current = destination.to_owned();
    let mut paths = Vec::new();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err(VR_ORGANIZATION_INELIGIBLE);
        };
        current.push(component);
        paths.push(current.clone());
    }
    Ok(paths)
}

fn remove_created_organization_directories(paths: &[PathBuf]) {
    for path in paths.iter().rev() {
        let _ = fs::remove_dir(path);
    }
}

fn apply_organization_with_persistence(
    state: &VrDownloadState,
    persistence_path: &Path,
    plan_id: &str,
    mut move_file: impl FnMut(&Path, &Path) -> io::Result<()>,
    mut persist_transfers: impl FnMut(&Path, &[StoredTransfer]) -> Result<(), &'static str>,
) -> Result<(), &'static str> {
    let mut context = state.0.lock().map_err(|_| VR_ORGANIZATION_FAILED)?;
    let plan = context
        .organization_plan
        .take()
        .filter(|plan| plan.plan_id == plan_id)
        .ok_or(VR_ORGANIZATION_STALE)?;
    if plan.generation != context.organization_generation {
        invalidate_organization_plan(&mut context);
        return Err(VR_ORGANIZATION_STALE);
    }
    invalidate_organization_plan(&mut context);
    let current_folder = match plan.category {
        TransferCategory::Adult => context.adult_future_folder.clone(),
        TransferCategory::Movie => context.movie_future_folder.clone(),
        TransferCategory::Tv => context.tv_future_folder.clone(),
        TransferCategory::Vr => context.future_folder.clone(),
    };
    let record_index = context
        .transfers
        .iter()
        .position(|transfer| {
            matches!(transfer, StoredTransfer::Valid(record) if record.transfer_id == plan.transfer_id)
        })
        .ok_or(VR_ORGANIZATION_STALE)?;
    let record = match &context.transfers[record_index] {
        StoredTransfer::Valid(record) => record,
        StoredTransfer::Corrupt(_) => return Err(VR_ORGANIZATION_STALE),
    };
    let entries = organization_entries(record, current_folder.as_deref())?;
    let current_plan_id = organization_plan_id(plan.generation, record, &entries)
        .map_err(|_| VR_ORGANIZATION_FAILED)?;
    if plan.category != record.category
        || plan.identity != organization_identity(record)?
        || plan.entries != entries
        || plan.plan_id != current_plan_id
    {
        return Err(VR_ORGANIZATION_STALE);
    }
    let previous_state = record.organization_state;
    let original_paths = record.current_paths.clone();
    let destination_root = record.destination.clone();
    let directory_name = organization_directory_name(record)?;
    let organization_directories =
        organization_directory_paths(&destination_root, &directory_name)?;
    let move_entries = entries
        .iter()
        .filter(|entry| entry.kind == OrganizationEntryKind::Move)
        .map(|entry| {
            let destination_relative = entry
                .destination_relative
                .as_deref()
                .ok_or(VR_ORGANIZATION_STALE)?;
            Ok((
                entry.selected_index,
                destination_root.join(relative_file_path(&entry.source_relative)?),
                destination_root.join(relative_file_path(destination_relative)?),
                destination_relative.to_owned(),
            ))
        })
        .collect::<Result<Vec<_>, &'static str>>()?;

    let mut created_directories = Vec::new();
    if !move_entries.is_empty() {
        validate_organization_directory(&destination_root, &directory_name)?;
        for directory in &organization_directories {
            match fs::symlink_metadata(directory) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                    remove_created_organization_directories(&created_directories);
                    return Err(VR_ORGANIZATION_CONFLICT);
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    match fs::create_dir(directory) {
                        Ok(()) => created_directories.push(directory.clone()),
                        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                        Err(_) => {
                            remove_created_organization_directories(&created_directories);
                            return Err(VR_ORGANIZATION_FAILED);
                        }
                    }
                }
                Err(_) => {
                    remove_created_organization_directories(&created_directories);
                    return Err(VR_ORGANIZATION_CONFLICT);
                }
            }
            if fs::canonicalize(directory).ok().as_deref() != Some(directory.as_path())
                || !directory.starts_with(&destination_root)
            {
                remove_created_organization_directories(&created_directories);
                return Err(VR_ORGANIZATION_CONFLICT);
            }
        }
    }

    let recovery_result = match &context.transfers[record_index] {
        StoredTransfer::Valid(record) => write_organization_recovery(record, &original_paths, None),
        StoredTransfer::Corrupt(_) => Err(VR_ORGANIZATION_STALE),
    };
    if let Err(error) = recovery_result {
        remove_created_organization_directories(&created_directories);
        return Err(error);
    }

    let mut current_paths = original_paths.clone();
    let mut moved = Vec::new();
    for (selected_index, source, destination, destination_relative) in &move_entries {
        if move_file(source, destination).is_err() {
            let restored = rollback_organization_moves(
                &moved,
                &original_paths,
                &mut current_paths,
                &mut move_file,
            );
            remove_created_organization_directories(&created_directories);
            let recovery_saved = {
                let StoredTransfer::Valid(record) = &mut context.transfers[record_index] else {
                    return Err(VR_ORGANIZATION_FAILED);
                };
                record.current_paths.clone_from(&current_paths);
                record.organization_state = if restored {
                    previous_state
                } else {
                    OrganizationState::Attention
                };
                write_organization_recovery(record, &current_paths, Some(&original_paths)).is_ok()
            };
            if persist_transfers(persistence_path, &context.transfers).is_ok() {
                let StoredTransfer::Valid(record) = &context.transfers[record_index] else {
                    return Err(VR_ORGANIZATION_FAILED);
                };
                clear_organization_recovery(record);
            } else if !recovery_saved {
                return Err(VR_DOWNLOAD_PERSISTENCE_FAILED);
            }
            return Err(VR_ORGANIZATION_FAILED);
        }
        current_paths[*selected_index] = destination_relative.clone();
        moved.push((*selected_index, source.clone(), destination.clone()));
    }

    {
        let StoredTransfer::Valid(record) = &mut context.transfers[record_index] else {
            return Err(VR_ORGANIZATION_FAILED);
        };
        record.current_paths.clone_from(&current_paths);
        record.organization_state = OrganizationState::Organized;
    }
    let recovery_result = match &context.transfers[record_index] {
        StoredTransfer::Valid(record) => {
            write_organization_recovery(record, &current_paths, Some(&original_paths))
        }
        StoredTransfer::Corrupt(_) => Err(VR_ORGANIZATION_STALE),
    };
    let persistence_result =
        recovery_result.and_then(|()| persist_transfers(persistence_path, &context.transfers));
    if let Err(error) = persistence_result {
        let restored = rollback_organization_moves(
            &moved,
            &original_paths,
            &mut current_paths,
            &mut move_file,
        );
        remove_created_organization_directories(&created_directories);
        let recovery_saved = {
            let StoredTransfer::Valid(record) = &mut context.transfers[record_index] else {
                return Err(VR_ORGANIZATION_FAILED);
            };
            record.current_paths = if restored {
                original_paths.clone()
            } else {
                current_paths
            };
            record.organization_state = if restored {
                previous_state
            } else {
                OrganizationState::Attention
            };
            write_organization_recovery(record, &record.current_paths, Some(&original_paths))
                .is_ok()
        };
        if persist_transfers(persistence_path, &context.transfers).is_ok() {
            let StoredTransfer::Valid(record) = &context.transfers[record_index] else {
                return Err(VR_ORGANIZATION_FAILED);
            };
            clear_organization_recovery(record);
        } else if !recovery_saved {
            return Err(VR_DOWNLOAD_PERSISTENCE_FAILED);
        }
        return Err(error);
    }
    let StoredTransfer::Valid(record) = &context.transfers[record_index] else {
        return Err(VR_ORGANIZATION_FAILED);
    };
    clear_organization_recovery(record);
    Ok(())
}

fn apply_organization_with(
    state: &VrDownloadState,
    persistence_path: &Path,
    plan_id: &str,
    move_file: impl FnMut(&Path, &Path) -> io::Result<()>,
) -> Result<(), &'static str> {
    apply_organization_with_persistence(
        state,
        persistence_path,
        plan_id,
        move_file,
        write_persisted_transfers,
    )
}

pub fn apply_organization(
    state: &VrDownloadState,
    persistence_path: &Path,
    plan_id: &str,
) -> Result<(), &'static str> {
    apply_organization_with(state, persistence_path, plan_id, rename_without_overwrite)
}

fn download_rows(context: &mut VrDownloadContext) -> Vec<String> {
    let mut rows = Vec::new();
    let current_vr_folder = context.future_folder.clone();
    let current_adult_folder = context.adult_future_folder.clone();
    let current_movie_folder = context.movie_future_folder.clone();
    let current_tv_folder = context.tv_future_folder.clone();
    for transfer in &mut context.transfers {
        match transfer {
            StoredTransfer::Valid(record) => {
                let current_folder = match record.category {
                    TransferCategory::Adult => &current_adult_folder,
                    TransferCategory::Movie => &current_movie_folder,
                    TransferCategory::Tv => &current_tv_folder,
                    TransferCategory::Vr => &current_vr_folder,
                };
                let mut speed = 0;
                if let Some(handle) = &record.handle {
                    let (downloaded, current_speed) = selected_progress(record, handle);
                    record.downloaded_bytes = downloaded;
                    speed = current_speed;
                    let stats = handle.stats();
                    record.state = match stats.state {
                        TorrentStatsState::Initializing => TransferState::Queued,
                        TorrentStatsState::Live => TransferState::Downloading,
                        TorrentStatsState::Paused => TransferState::Paused,
                        TorrentStatsState::Error => record.state,
                    };
                }
                if record.state == TransferState::Completed {
                    record.downloaded_bytes = record.selected_total();
                }
                let organization_relative_directory =
                    if record.organization_state == OrganizationState::None {
                        String::new()
                    } else {
                        organization_directory_name(record)
                            .map(|directory| format!("{directory}/"))
                            .unwrap_or_default()
                    };
                rows.extend([
                    record.transfer_id.clone(),
                    record.category.as_str().to_owned(),
                    record
                        .movie_identity
                        .as_ref()
                        .map(|identity| identity.imdb_id.clone())
                        .or_else(|| {
                            record.tv_identity.as_ref().map(|identity| {
                                format!(
                                    "{} · S{:02}E{:02}",
                                    identity.imdb_id,
                                    identity.season_number,
                                    identity.episode_number
                                )
                            })
                        })
                        .unwrap_or_else(|| record.code.clone()),
                    record.release_name.clone(),
                    record.selected_files.len().to_string(),
                    record.selected_total().to_string(),
                    record.downloaded_bytes.to_string(),
                    speed.to_string(),
                    record.state.as_str().to_owned(),
                    (current_folder.as_ref() == Some(&record.destination)).to_string(),
                    record.organization_state.as_str().to_owned(),
                    organization_relative_directory,
                    organization_entries(record, current_folder.as_deref())
                        .is_ok()
                        .to_string(),
                    record.terminal_recovery_generation.is_some().to_string(),
                    record
                        .selected_files
                        .iter()
                        .map(|file| encode_hex(file.path.as_bytes()))
                        .collect::<Vec<_>>()
                        .join(","),
                    (cfg!(any(target_os = "macos", target_os = "windows"))
                        && (record.state == TransferState::Cleanup
                            || (record.state == TransferState::Cancelled
                                && record.handle.is_none()
                                && record.pending_action.is_none()
                                && record.terminal_recovery_generation.is_none()
                                && record.organization_state == OrganizationState::None)))
                        .to_string(),
                ]);
            }
            StoredTransfer::Corrupt(record) => rows.extend([
                record.transfer_id.clone(),
                record
                    .category
                    .map(TransferCategory::as_str)
                    .unwrap_or("unknown")
                    .to_owned(),
                record.code.clone(),
                record.release_name.clone(),
                "0".to_owned(),
                "0".to_owned(),
                "0".to_owned(),
                "0".to_owned(),
                TransferState::Offline.as_str().to_owned(),
                "false".to_owned(),
                OrganizationState::None.as_str().to_owned(),
                String::new(),
                "false".to_owned(),
                "false".to_owned(),
                String::new(),
                "false".to_owned(),
            ]),
        }
    }
    rows
}

fn list_downloads_with_persistence(
    state: &VrDownloadState,
    persistence_path: &Path,
    persist_transfers: fn(&Path, &[StoredTransfer]) -> Result<(), &'static str>,
) -> Result<Vec<String>, &'static str> {
    let mut context = state.0.lock().map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
    if !context.transfers_loaded {
        return Err(VR_DOWNLOAD_ACTION_INVALID);
    }
    if context
        .persistence_path
        .as_deref()
        .is_some_and(|current| current != persistence_path)
    {
        return Err(VR_DOWNLOAD_ACTION_INVALID);
    }
    context.persistence_path = Some(persistence_path.to_owned());
    let rows = download_rows(&mut context);
    let recovery_destinations = context
        .transfers
        .iter()
        .filter_map(|transfer| match transfer {
            StoredTransfer::Valid(record) => Some(record.destination.clone()),
            StoredTransfer::Corrupt(_) => None,
        })
        .collect::<BTreeSet<_>>();
    let organization_recovery_transfer_ids = recovery_destinations
        .iter()
        .flat_map(|destination| read_organization_recoveries(destination))
        .map(|record| record.transfer_id)
        .collect::<BTreeSet<_>>();
    let terminal_recoveries = read_terminal_recoveries(persistence_path)?;
    let cleanup_recoveries = read_cleanup_recoveries(persistence_path)?;
    let has_durable_recovery = context.transfers.iter().any(|transfer| {
        matches!(transfer, StoredTransfer::Valid(record) if organization_recovery_transfer_ids.contains(&record.transfer_id)
            || terminal_recoveries.iter().any(|recovery| same_terminal_authority(record, recovery))
            || cleanup_recoveries.iter().any(|recovery| same_cleanup_record_identity(record, &recovery.record)))
    });
    match persist_transfers(persistence_path, &context.transfers) {
        Ok(()) => {
            for transfer in &mut context.transfers {
                if let StoredTransfer::Valid(record) = transfer {
                    if record.state != TransferState::Cleanup {
                        clear_organization_recovery(record);
                        if remove_terminal_recovery(persistence_path, record).is_ok() {
                            record.terminal_recovery_generation = None;
                        }
                    }
                }
            }
            Ok(download_rows(&mut context))
        }
        Err(_) if has_durable_recovery => Ok(rows),
        Err(error) => Err(error),
    }
}

pub fn list_downloads(
    state: &VrDownloadState,
    persistence_path: &Path,
) -> Result<Vec<String>, &'static str> {
    list_downloads_with_persistence(state, persistence_path, write_persisted_transfers)
}

async fn start_download_source(
    state: &VrDownloadState,
    persistence_path: &Path,
    session_folder: &Path,
    category: TransferCategory,
    source: VerifiedDownloadSource,
) -> Result<String, &'static str> {
    let source_matches_category = match category {
        TransferCategory::Movie => {
            source.code.is_empty()
                && source.tv_identity.is_none()
                && source.movie_identity.as_ref().is_some_and(|identity| {
                    source.release_name == identity.tmdb_title
                        && source.infohash == identity.expected_infohash
                })
        }
        TransferCategory::Tv => {
            source.code.is_empty()
                && source.movie_identity.is_none()
                && source.tv_identity.as_ref().is_some_and(|identity| {
                    source.release_name == identity.release_name
                        && source.infohash == identity.infohash
                })
        }
        TransferCategory::Adult | TransferCategory::Vr => {
            source.movie_identity.is_none() && source.tv_identity.is_none()
        }
    };
    if !source_matches_category {
        return Err(VR_DOWNLOAD_CONTEXT_INVALID);
    }
    checked_selected_total(&source.selected_files)?;
    let mut record = {
        let context = state.0.lock().map_err(|_| VR_DOWNLOAD_FAILED)?;
        if !context.transfers_loaded {
            return Err(VR_DOWNLOAD_ACTION_INVALID);
        }
        if matches!(context.download_limit, DownloadLimitState::Unloaded) {
            return Err(VR_DOWNLOAD_LIMIT_UNAVAILABLE);
        }
        let future_folder = match category {
            TransferCategory::Adult => context.adult_future_folder.as_deref(),
            TransferCategory::Movie => context.movie_future_folder.as_deref(),
            TransferCategory::Tv => context.tv_future_folder.as_deref(),
            TransferCategory::Vr => context.future_folder.as_deref(),
        };
        let destination = canonical_destination(future_folder.ok_or(VR_FOLDER_UNAVAILABLE)?)?;
        if has_active_duplicate(&context.transfers, &source.infohash, &destination) {
            return Err(VR_DOWNLOAD_DUPLICATE);
        }
        let record = transfer_from_source(category, source, destination, TransferState::Queued);
        validate_cleanup_start_reservations(&context, persistence_path, &record)?;
        record
    };
    validate_new_targets(&record.destination, &record.selected_files)?;
    let transfer_id = record.transfer_id.clone();
    {
        let mut context = state.0.lock().map_err(|_| VR_DOWNLOAD_FAILED)?;
        validate_cleanup_start_reservations(&context, persistence_path, &record)?;
        if has_active_duplicate(&context.transfers, &record.infohash, &record.destination) {
            return Err(VR_DOWNLOAD_DUPLICATE);
        }
        if context.transfers.len() >= MAX_PERSISTED_TRANSFERS {
            return Err(VR_DOWNLOAD_FAILED);
        }
        context.transfers.push(StoredTransfer::Valid(record));
        if let Err(error) = write_persisted_transfers(persistence_path, &context.transfers) {
            context.transfers.pop();
            return Err(error);
        }
    }

    let session = match session_for(state, session_folder).await {
        Ok(session) => session,
        Err(error) => {
            mark_transfer_failed(state, persistence_path, &transfer_id);
            return Err(error);
        }
    };
    let record_snapshot = {
        let context = state.0.lock().map_err(|_| VR_DOWNLOAD_FAILED)?;
        context
            .transfers
            .iter()
            .find_map(|transfer| match transfer {
                StoredTransfer::Valid(record) if record.transfer_id == transfer_id => {
                    Some(TransferRecord {
                        transfer_id: record.transfer_id.clone(),
                        category: record.category,
                        code: record.code.clone(),
                        release_name: record.release_name.clone(),
                        movie_identity: record.movie_identity.clone(),
                        tv_identity: record.tv_identity.clone(),
                        infohash: record.infohash.clone(),
                        metainfo: record.metainfo.clone(),
                        selected_files: record.selected_files.clone(),
                        destination: record.destination.clone(),
                        fingerprints: Vec::new(),
                        current_paths: record.current_paths.clone(),
                        organization_state: record.organization_state,
                        state: record.state,
                        downloaded_bytes: 0,
                        boundary_segments: record.boundary_segments.clone(),
                        handle: None,
                        handle_generation: 0,
                        terminal_recovery_generation: None,
                        pending_action: None,
                    })
                }
                _ => None,
            })
            .ok_or(VR_DOWNLOAD_STALE)?
    };
    let handle = match add_record_to_session(&session, &record_snapshot, false).await {
        Ok(handle) => handle,
        Err(error) => {
            mark_transfer_failed(state, persistence_path, &transfer_id);
            return Err(error);
        }
    };
    record = record_snapshot;
    record.fingerprints = match capture_fingerprints(&record) {
        Ok(fingerprints) => fingerprints,
        Err(error) => {
            if let Ok(mut context) = state.0.lock() {
                if let Some(current) = find_valid_record_mut(&mut context.transfers, &transfer_id) {
                    current.handle_generation = current.handle_generation.wrapping_add(1);
                    current.handle = Some(handle.clone());
                }
            }
            if mark_transfer_failed(state, persistence_path, &transfer_id) {
                let _ = session.delete(handle.id().into(), false).await;
            }
            return Err(error);
        }
    };

    let saved_handle_generation = {
        let mut context = state.0.lock().map_err(|_| VR_DOWNLOAD_FAILED)?;
        let current =
            find_valid_record_mut(&mut context.transfers, &transfer_id).ok_or(VR_DOWNLOAD_STALE)?;
        current.fingerprints = record.fingerprints;
        current.handle_generation = current.handle_generation.wrapping_add(1);
        current.handle = Some(handle.clone());
        current.state = TransferState::Downloading;
        let generation = current.handle_generation;
        write_persisted_transfers(persistence_path, &context.transfers).map(|()| generation)
    };
    let handle_generation = match saved_handle_generation {
        Ok(generation) => generation,
        Err(error) => {
            if mark_transfer_failed(state, persistence_path, &transfer_id) {
                let _ = session.delete(handle.id().into(), false).await;
            }
            return Err(error);
        }
    };
    if session.unpause(&handle).await.is_err() {
        if mark_transfer_failed(state, persistence_path, &transfer_id) {
            let _ = session.delete(handle.id().into(), false).await;
        }
        return Err(VR_DOWNLOAD_FAILED);
    }
    spawn_completion_monitor(
        state.clone(),
        session,
        handle,
        transfer_id.clone(),
        handle_generation,
        persistence_path.to_owned(),
    );
    Ok(transfer_id)
}

pub async fn start_download(
    state: &VrDownloadState,
    torrent_state: &VrTorrentState,
    persistence_path: &Path,
    session_folder: &Path,
    inspection_id: &str,
    selected_file_ids: &[usize],
) -> Result<String, &'static str> {
    let source = torrent_state
        .verified_download_source(inspection_id, selected_file_ids)
        .map_err(map_source_error)?;
    start_download_source(
        state,
        persistence_path,
        session_folder,
        TransferCategory::Vr,
        source,
    )
    .await
}

pub async fn start_adult_download(
    state: &VrDownloadState,
    torrent_state: &AdultTorrentState,
    persistence_path: &Path,
    session_folder: &Path,
    inspection_id: &str,
    selected_file_ids: &[usize],
) -> Result<String, &'static str> {
    let source = torrent_state
        .verified_download_source(inspection_id, selected_file_ids)
        .map_err(map_source_error)?;
    start_download_source(
        state,
        persistence_path,
        session_folder,
        TransferCategory::Adult,
        source,
    )
    .await
}

pub async fn start_movie_download(
    state: &VrDownloadState,
    torrent_state: &MovieTorrentState,
    persistence_path: &Path,
    session_folder: &Path,
    inspection_id: &str,
    selected_file_ids: &[usize],
) -> Result<String, &'static str> {
    let source = torrent_state
        .verified_download_source(inspection_id, selected_file_ids)
        .map_err(map_source_error)?;
    start_download_source(
        state,
        persistence_path,
        session_folder,
        TransferCategory::Movie,
        source,
    )
    .await
}

pub(crate) async fn start_tv_download(
    state: &VrDownloadState,
    torrent_state: &TvTorrentState,
    release_state: &TvReleaseState,
    persistence_path: &Path,
    session_folder: &Path,
    inspection_id: &str,
    selected_file_ids: &[usize],
) -> Result<String, &'static str> {
    let source = torrent_state
        .verified_download_source(release_state, inspection_id, selected_file_ids)
        .map_err(map_source_error)?;
    start_download_source(
        state,
        persistence_path,
        session_folder,
        TransferCategory::Tv,
        source,
    )
    .await
}

fn mark_transfer_failed(
    state: &VrDownloadState,
    persistence_path: &Path,
    transfer_id: &str,
) -> bool {
    let Ok(mut context) = state.0.lock() else {
        return false;
    };
    let Some(handle_generation) = context
        .transfers
        .iter()
        .find_map(|transfer| match transfer {
            StoredTransfer::Valid(record) if record.transfer_id == transfer_id => {
                Some(record.handle_generation)
            }
            StoredTransfer::Valid(_) | StoredTransfer::Corrupt(_) => None,
        })
    else {
        return false;
    };
    finalize_monitored_transfer_with(
        &mut context,
        transfer_id,
        handle_generation,
        false,
        persistence_path,
        write_persisted_transfers,
        write_terminal_recovery,
    )
}

async fn controlled_transfer(
    state: &VrDownloadState,
    persistence_path: &Path,
    transfer_id: &str,
    action: TransferAction,
) -> Result<(), &'static str> {
    let (session, handle, handle_generation, previous_handle_generation) = {
        let mut context = state.0.lock().map_err(|_| VR_DOWNLOAD_FAILED)?;
        let session = context.session.clone();
        let record =
            find_valid_record_mut(&mut context.transfers, transfer_id).ok_or(VR_DOWNLOAD_STALE)?;
        if record.pending_action.is_some() {
            return Err(VR_DOWNLOAD_ACTION_INVALID);
        }
        let valid = match action {
            TransferAction::Pause => record.state == TransferState::Downloading,
            TransferAction::Resume => record.state == TransferState::Paused,
            TransferAction::Cancel => {
                record.state.is_active() || record.state == TransferState::Offline
            }
        };
        if !valid {
            return Err(VR_DOWNLOAD_ACTION_INVALID);
        }
        if let Some(handle) = record.handle.as_ref() {
            if let Some(downloaded_bytes) = verified_selected_bytes(record, handle) {
                record.downloaded_bytes = downloaded_bytes.min(record.selected_total());
            }
        }
        record.pending_action = Some(action);
        let previous_handle_generation = record.handle_generation;
        if action == TransferAction::Cancel {
            record.handle_generation = record.handle_generation.wrapping_add(1);
        }
        let handle_generation = record.handle_generation;
        let handle = record.handle.clone();
        match action {
            TransferAction::Cancel if handle.is_none() => {
                (session, None, handle_generation, previous_handle_generation)
            }
            _ => (
                Some(session.ok_or(VR_DOWNLOAD_STALE)?),
                Some(handle.ok_or(VR_DOWNLOAD_STALE)?),
                handle_generation,
                previous_handle_generation,
            ),
        }
    };

    let result = match (action, session.as_ref(), handle.as_ref()) {
        (TransferAction::Pause, Some(session), Some(handle)) => session.pause(handle).await,
        (TransferAction::Resume, Some(session), Some(handle)) => session.unpause(handle).await,
        (TransferAction::Cancel, Some(session), Some(handle)) => {
            session.delete(handle.id().into(), false).await
        }
        (TransferAction::Cancel, _, None) => Ok(()),
        _ => Err(anyhow!("transfer action lost its native handle")),
    };
    if result.is_err() {
        let terminal_saved = {
            let mut context = state.0.lock().map_err(|_| VR_DOWNLOAD_FAILED)?;
            {
                let record = find_valid_record_mut(&mut context.transfers, transfer_id)
                    .ok_or(VR_DOWNLOAD_STALE)?;
                if record.pending_action != Some(action)
                    || record.handle_generation != handle_generation
                {
                    return Err(VR_DOWNLOAD_STALE);
                }
                if let Some(handle) = handle.as_ref() {
                    if let Some(downloaded_bytes) = verified_selected_bytes(record, handle) {
                        record.downloaded_bytes = downloaded_bytes.min(record.selected_total());
                    }
                }
                record.pending_action = None;
            }
            let terminal_generation = if action == TransferAction::Cancel {
                find_valid_record_mut(&mut context.transfers, transfer_id)
                    .expect("the validated transfer must remain present")
                    .handle_generation = previous_handle_generation;
                previous_handle_generation
            } else {
                handle_generation
            };
            finalize_monitored_transfer_with(
                &mut context,
                transfer_id,
                terminal_generation,
                false,
                persistence_path,
                write_persisted_transfers,
                write_terminal_recovery,
            )
        };
        if terminal_saved {
            if let (Some(session), Some(handle)) = (session.as_ref(), handle.as_ref()) {
                let _ = session.delete(handle.id().into(), false).await;
            }
        }
        return Err(VR_DOWNLOAD_FAILED);
    }
    let mut context = state.0.lock().map_err(|_| VR_DOWNLOAD_FAILED)?;
    let record =
        find_valid_record_mut(&mut context.transfers, transfer_id).ok_or(VR_DOWNLOAD_STALE)?;
    if record.pending_action != Some(action) || record.handle_generation != handle_generation {
        return Err(VR_DOWNLOAD_STALE);
    }
    if let Some(handle) = handle.as_ref() {
        if let Some(downloaded_bytes) = verified_selected_bytes(record, handle) {
            record.downloaded_bytes = downloaded_bytes.min(record.selected_total());
        }
    }
    record.pending_action = None;
    record.state = match action {
        TransferAction::Pause => TransferState::Paused,
        TransferAction::Resume => TransferState::Downloading,
        TransferAction::Cancel => TransferState::Cancelled,
    };
    if action == TransferAction::Cancel {
        record.handle = None;
    }
    write_persisted_transfers(persistence_path, &context.transfers)
}

pub async fn pause_download(
    state: &VrDownloadState,
    persistence_path: &Path,
    transfer_id: &str,
) -> Result<(), &'static str> {
    controlled_transfer(state, persistence_path, transfer_id, TransferAction::Pause).await
}

pub async fn resume_download(
    state: &VrDownloadState,
    persistence_path: &Path,
    transfer_id: &str,
) -> Result<(), &'static str> {
    controlled_transfer(state, persistence_path, transfer_id, TransferAction::Resume).await
}

pub async fn cancel_download(
    state: &VrDownloadState,
    persistence_path: &Path,
    transfer_id: &str,
) -> Result<(), &'static str> {
    controlled_transfer(state, persistence_path, transfer_id, TransferAction::Cancel).await
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn cleanup_target_state(
    record: &TransferRecord,
    selected_index: usize,
) -> Result<CleanupFileState, &'static str> {
    if record.organization_state != OrganizationState::None
        || record.current_paths.get(selected_index)
            != record
                .selected_files
                .get(selected_index)
                .map(|file| &file.path)
        || canonical_destination(&record.destination)? != record.destination
        || record.fingerprints.len() != record.selected_files.len()
    {
        return Err(VR_DOWNLOAD_STALE);
    }
    let target = current_target(record, selected_index)?;
    match fs::symlink_metadata(&target) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink()
                || !metadata.is_file()
                || metadata.len() > record.selected_files[selected_index].size
                || fs::canonicalize(&target).ok().as_deref() != Some(target.as_path())
                || file_fingerprint(&target)? != record.fingerprints[selected_index]
            {
                return Err(VR_DOWNLOAD_STALE);
            }
            Ok(CleanupFileState::Present)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Ok(CleanupFileState::AbsentBeforeCleanup)
        }
        Err(_) => Err(VR_DOWNLOAD_STALE),
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn validate_cleanup_boundary_data(record: &TransferRecord) -> Result<(), &'static str> {
    let selected_file_ids = record
        .selected_file_ids()
        .into_iter()
        .collect::<BTreeSet<_>>();
    let boundary_segments = record
        .boundary_segments
        .lock()
        .map_err(|_| VR_DOWNLOAD_STALE)?;
    if boundary_segments.iter().any(|(file_id, segments)| {
        selected_file_ids.contains(file_id)
            || segments.is_empty()
            || segments.iter().any(|segment| segment.bytes.is_empty())
    }) {
        return Err(VR_DOWNLOAD_STALE);
    }
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn cleanup_target_is_owned_elsewhere(
    context: &VrDownloadContext,
    persistence_path: &Path,
    transfer_id: &str,
    target: &Path,
) -> Result<bool, &'static str> {
    for transfer in &context.transfers {
        match transfer {
            StoredTransfer::Valid(record) if record.transfer_id != transfer_id => {
                if transfer_record_owns_path(record, target).map_err(|_| VR_DOWNLOAD_STALE)? {
                    return Ok(true);
                }
            }
            StoredTransfer::Valid(_) => {}
            StoredTransfer::Corrupt(_) => return Err(VR_DOWNLOAD_STALE),
        }
    }
    if organization_plan_owns_path(context, target).map_err(|_| VR_DOWNLOAD_STALE)? {
        return Ok(true);
    }
    for path in terminal_recovery_paths(persistence_path)? {
        let record = parse_terminal_recovery(persistence_path, &path).ok_or(VR_DOWNLOAD_STALE)?;
        if transfer_record_owns_path(&record, target).map_err(|_| VR_DOWNLOAD_STALE)? {
            return Ok(true);
        }
    }
    for path in cleanup_recovery_paths(persistence_path)? {
        let recovery = parse_cleanup_recovery(persistence_path, &path).ok_or(VR_DOWNLOAD_STALE)?;
        if recovery.record.transfer_id != transfer_id
            && transfer_record_owns_path(&recovery.record, target).map_err(|_| VR_DOWNLOAD_STALE)?
        {
            return Ok(true);
        }
    }
    let mut destinations = [
        context.future_folder.clone(),
        context.adult_future_folder.clone(),
        context.movie_future_folder.clone(),
        context.tv_future_folder.clone(),
    ]
    .into_iter()
    .flatten()
    .collect::<BTreeSet<_>>();
    if let Some(record) = context
        .transfers
        .iter()
        .find_map(|transfer| match transfer {
            StoredTransfer::Valid(record) if record.transfer_id == transfer_id => Some(record),
            StoredTransfer::Valid(_) | StoredTransfer::Corrupt(_) => None,
        })
    {
        destinations.insert(record.destination.clone());
    }
    for destination in destinations {
        if fs::metadata(&destination).is_ok_and(|metadata| metadata.is_dir())
            && durable_organization_recovery_owns_path(&destination, target)
                .map_err(|_| VR_DOWNLOAD_STALE)?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn persisted_cancelled_record_is_exact(
    persistence_path: &Path,
    expected: &TransferRecord,
) -> Result<bool, &'static str> {
    let persisted = read_persisted_transfers(persistence_path)?;
    let mut matching = persisted.iter().filter_map(|transfer| match transfer {
        StoredTransfer::Valid(record) if record.transfer_id == expected.transfer_id => Some(record),
        StoredTransfer::Valid(_) | StoredTransfer::Corrupt(_) => None,
    });
    let Some(record) = matching.next() else {
        return Ok(false);
    };
    Ok(matching.next().is_none()
        && record.state == TransferState::Cancelled
        && record.pending_action.is_none()
        && record.handle.is_none()
        && same_terminal_authority(record, expected))
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn cleanup_cancelled_download_with_reconciliation<Deletion>(
    state: &VrDownloadState,
    persistence_path: &Path,
    transfer_id: &str,
    mut reconcile_file: impl FnMut(
        CleanupFileState,
        &Path,
        &str,
    ) -> Result<CleanupReconciliation, &'static str>,
    mut delete_file: impl FnMut(&Path, &str) -> Result<Deletion, &'static str>,
    mut persistence: (
        impl FnMut(&Path, &CleanupRecovery) -> Result<(), &'static str>,
        impl FnMut(&Path, &[StoredTransfer]) -> Result<(), &'static str>,
        impl FnMut(&Path, &CleanupRecovery) -> Result<(), &'static str>,
    ),
) -> Result<Vec<String>, &'static str>
where
    Deletion: Into<CleanupDeletionOutcome>,
{
    let mut context = state.0.lock().map_err(|_| VR_DOWNLOAD_CLEANUP_FAILED)?;
    if context.cleanup_transfer_id.is_some() {
        return Err(VR_DOWNLOAD_ACTION_INVALID);
    }
    context.cleanup_transfer_id = Some(transfer_id.to_owned());
    let result = (|| {
        if !context.transfers_loaded
            || context.transfers_loading
            || context.persistence_path.as_deref() != Some(persistence_path)
        {
            return Err(VR_DOWNLOAD_ACTION_INVALID);
        }
        let position = context
            .transfers
            .iter()
            .position(|transfer| {
                matches!(transfer, StoredTransfer::Valid(record) if record.transfer_id == transfer_id)
            })
            .ok_or(VR_DOWNLOAD_STALE)?;
        let StoredTransfer::Valid(current) = &context.transfers[position] else {
            return Err(VR_DOWNLOAD_STALE);
        };

        let mut recovery = if current.state == TransferState::Cleanup {
            let path = cleanup_recovery_path(persistence_path, current)?;
            let recovery = parse_cleanup_recovery(persistence_path, &path)
                .ok_or(VR_DOWNLOAD_PERSISTENCE_FAILED)?;
            if !same_terminal_authority(current, &recovery.record) {
                return Err(VR_DOWNLOAD_STALE);
            }
            recovery
        } else {
            if current.state != TransferState::Cancelled
                || current.handle.is_some()
                || current.pending_action.is_some()
                || current.terminal_recovery_generation.is_some()
                || current.organization_state != OrganizationState::None
                || !persisted_cancelled_record_is_exact(persistence_path, current)?
            {
                return Err(VR_DOWNLOAD_ACTION_INVALID);
            }
            let previous = current.clone();
            validate_cleanup_boundary_data(&previous)?;
            let files = (0..previous.selected_files.len())
                .map(|selected_index| {
                    let target = current_target(&previous, selected_index)?;
                    let file = cleanup_target_state(&previous, selected_index)?;
                    if cleanup_target_is_owned_elsewhere(
                        &context,
                        persistence_path,
                        transfer_id,
                        &target,
                    )? {
                        return Err(VR_DOWNLOAD_STALE);
                    }
                    Ok(file)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let mut cleanup_record = previous.clone();
            cleanup_record.state = TransferState::Cleanup;
            let recovery = CleanupRecovery {
                record: cleanup_record.clone(),
                files,
            };
            (persistence.0)(persistence_path, &recovery)?;
            context.transfers[position] = StoredTransfer::Valid(cleanup_record);
            if let Err(error) = (persistence.1)(persistence_path, &context.transfers) {
                if (persistence.2)(persistence_path, &recovery).is_ok() {
                    context.transfers[position] = StoredTransfer::Valid(previous);
                }
                return Err(error);
            }
            recovery
        };

        validate_cleanup_boundary_data(&recovery.record)?;
        for selected_index in 0..recovery.files.len() {
            let target = current_target(&recovery.record, selected_index)?;
            let expected_fingerprint = recovery.record.fingerprints[selected_index].clone();
            if cleanup_target_is_owned_elsewhere(&context, persistence_path, transfer_id, &target)?
            {
                return Err(VR_DOWNLOAD_STALE);
            }
            let reconciliation = reconcile_file(
                recovery.files[selected_index],
                &target,
                &expected_fingerprint,
            )?;
            match recovery.files[selected_index] {
                CleanupFileState::Deleted | CleanupFileState::AbsentBeforeCleanup => {
                    if reconciliation.deletion_completed() {
                        continue;
                    }
                    match fs::symlink_metadata(&target) {
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                        _ => return Err(VR_DOWNLOAD_STALE),
                    }
                }
                CleanupFileState::Present if reconciliation.deletion_completed() => {
                    recovery.files[selected_index] = CleanupFileState::Deleted;
                    (persistence.0)(persistence_path, &recovery)?;
                }
                CleanupFileState::Present => match fs::symlink_metadata(&target) {
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        recovery.files[selected_index] = CleanupFileState::Deleted;
                        (persistence.0)(persistence_path, &recovery)?;
                    }
                    Ok(_) => {
                        if cleanup_target_state(&recovery.record, selected_index)?
                            != CleanupFileState::Present
                        {
                            return Err(VR_DOWNLOAD_STALE);
                        }
                        let deletion = delete_file(&target, &expected_fingerprint)?.into();
                        match (deletion, fs::symlink_metadata(&target)) {
                            (CleanupDeletionOutcome::TargetAbsent, Err(error))
                                if error.kind() == io::ErrorKind::NotFound => {}
                            #[cfg(target_os = "macos")]
                            (CleanupDeletionOutcome::ReplacementPreserved, Ok(metadata))
                                if !metadata.file_type().is_symlink() && metadata.is_file() => {}
                            _ => return Err(VR_DOWNLOAD_STALE),
                        }
                        sync_parent_directory(&target);
                        recovery.files[selected_index] = CleanupFileState::Deleted;
                        (persistence.0)(persistence_path, &recovery)?;
                    }
                    Err(_) => return Err(VR_DOWNLOAD_STALE),
                },
            }
        }

        recovery.record.boundary_segments = Arc::new(Mutex::new(BTreeMap::new()));
        (persistence.0)(persistence_path, &recovery)?;
        let category = recovery.record.category;
        let current_folder =
            configured_folder(&context, category) == Some(&recovery.record.destination);
        let StoredTransfer::Valid(current) = &mut context.transfers[position] else {
            return Err(VR_DOWNLOAD_STALE);
        };
        current.boundary_segments = recovery.record.boundary_segments.clone();
        invalidate_organization_plan(&mut context);
        let removed = context.transfers.remove(position);
        if let Err(error) = (persistence.1)(persistence_path, &context.transfers) {
            context.transfers.insert(position, removed);
            return Err(error);
        }
        if let Err(error) = (persistence.2)(persistence_path, &recovery) {
            context.transfers.insert(position, removed);
            return Err(error);
        }
        Ok(vec![
            category.as_str().to_owned(),
            current_folder.to_string(),
        ])
    })();
    context.cleanup_transfer_id = None;
    result
}

#[cfg(any(target_os = "windows", test))]
fn cleanup_cancelled_download_with<Deletion>(
    state: &VrDownloadState,
    persistence_path: &Path,
    transfer_id: &str,
    delete_file: impl FnMut(&Path, &str) -> Result<Deletion, &'static str>,
    persist_recovery: impl FnMut(&Path, &CleanupRecovery) -> Result<(), &'static str>,
    persist_primary: impl FnMut(&Path, &[StoredTransfer]) -> Result<(), &'static str>,
    remove_recovery: impl FnMut(&Path, &CleanupRecovery) -> Result<(), &'static str>,
) -> Result<Vec<String>, &'static str>
where
    Deletion: Into<CleanupDeletionOutcome>,
{
    cleanup_cancelled_download_with_reconciliation(
        state,
        persistence_path,
        transfer_id,
        |_, _, _| Ok(CleanupReconciliation::Continue),
        delete_file,
        (persist_recovery, persist_primary, remove_recovery),
    )
}

#[cfg(target_os = "windows")]
pub fn cleanup_cancelled_download(
    state: &VrDownloadState,
    persistence_path: &Path,
    transfer_id: &str,
) -> Result<Vec<String>, &'static str> {
    cleanup_cancelled_download_with(
        state,
        persistence_path,
        transfer_id,
        delete_exact_windows_cleanup_file,
        write_cleanup_recovery,
        write_persisted_transfers,
        remove_cleanup_recovery,
    )
}

#[cfg(target_os = "macos")]
fn new_macos_cleanup_mutation(
    persistence_path: &Path,
    transfer_id: &str,
    target: &Path,
    expected_fingerprint: &str,
) -> Result<MacosCleanupMutation, &'static str> {
    let cleanup_path = cleanup_recovery_directory(persistence_path)?.join(format!(
        "{CLEANUP_RECOVERY_PREFIX}{transfer_id}{TERMINAL_RECOVERY_SUFFIX}"
    ));
    let recovery = parse_cleanup_recovery(persistence_path, &cleanup_path)
        .ok_or(VR_DOWNLOAD_PERSISTENCE_FAILED)?;
    let mut matching_indices = recovery.record.fingerprints.iter().enumerate().filter_map(
        |(selected_index, fingerprint)| {
            (fingerprint == expected_fingerprint
                && current_target(&recovery.record, selected_index)
                    .ok()
                    .as_deref()
                    == Some(target))
            .then_some(selected_index)
        },
    );
    let selected_index = matching_indices.next().ok_or(VR_DOWNLOAD_STALE)?;
    if matching_indices.next().is_some()
        || recovery.files[selected_index] != CleanupFileState::Present
    {
        return Err(VR_DOWNLOAD_STALE);
    }
    let staging_path =
        macos_cleanup_staging_path(&recovery.record, selected_index, expected_fingerprint)?;
    let staging_token = macos_cleanup_staging_token(
        &recovery.record,
        selected_index,
        &staging_path,
        expected_fingerprint,
    );
    Ok(MacosCleanupMutation {
        record: recovery.record,
        selected_index,
        target_path: target.to_owned(),
        staging_path,
        expected_fingerprint: expected_fingerprint.to_owned(),
        staging_token,
        phase: MacosCleanupMutationPhase::StagingCreationPrepared,
    })
}

#[cfg(target_os = "macos")]
fn run_macos_cleanup_mutation(
    persistence_path: &Path,
    mutation: &mut MacosCleanupMutation,
) -> Result<CleanupDeletionOutcome, &'static str> {
    run_macos_cleanup_mutation_with(
        persistence_path,
        mutation,
        write_macos_cleanup_mutation,
        write_cleanup_recovery,
        remove_macos_cleanup_mutation,
        |_, _| Ok(()),
        |_| Ok(()),
    )
}

#[cfg(target_os = "macos")]
fn delete_exact_macos_cancelled_cleanup_file(
    persistence_path: &Path,
    transfer_id: &str,
    target: &Path,
    expected_fingerprint: &str,
) -> Result<CleanupDeletionOutcome, &'static str> {
    let mutation_path = macos_cleanup_mutation_path(persistence_path, transfer_id)?;
    if fs::symlink_metadata(&mutation_path).is_ok() {
        return Err(VR_DOWNLOAD_PERSISTENCE_FAILED);
    }
    let mut mutation =
        new_macos_cleanup_mutation(persistence_path, transfer_id, target, expected_fingerprint)?;
    write_macos_cleanup_mutation(persistence_path, &mutation)?;
    run_macos_cleanup_mutation(persistence_path, &mut mutation)
}

#[cfg(target_os = "macos")]
fn reconcile_macos_cancelled_cleanup_file(
    persistence_path: &Path,
    transfer_id: &str,
    state: CleanupFileState,
    target: &Path,
    expected_fingerprint: &str,
) -> Result<CleanupReconciliation, &'static str> {
    let mutation_path = macos_cleanup_mutation_path(persistence_path, transfer_id)?;
    match fs::symlink_metadata(&mutation_path) {
        Ok(_) => {
            let mut mutation = parse_macos_cleanup_mutation(persistence_path, &mutation_path)
                .ok_or(VR_DOWNLOAD_PERSISTENCE_FAILED)?;
            if mutation.target_path != target
                || mutation.expected_fingerprint != expected_fingerprint
            {
                return Ok(CleanupReconciliation::Continue);
            }
            run_macos_cleanup_mutation(persistence_path, &mut mutation)?;
            Ok(CleanupReconciliation::DeletionCompleted)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if state != CleanupFileState::Deleted {
                return Ok(CleanupReconciliation::Continue);
            }
            match fs::symlink_metadata(target) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    CleanupDeletionOutcome::TargetAbsent
                }
                Ok(metadata)
                    if metadata.is_file()
                        && !metadata.file_type().is_symlink()
                        && file_fingerprint(target)? == expected_fingerprint =>
                {
                    return Err(VR_DOWNLOAD_STALE)
                }
                Ok(_) => CleanupDeletionOutcome::ReplacementPreserved,
                Err(_) => return Err(VR_DOWNLOAD_STALE),
            };
            Ok(CleanupReconciliation::DeletionCompleted)
        }
        Err(_) => Err(VR_DOWNLOAD_PERSISTENCE_FAILED),
    }
}

#[cfg(target_os = "macos")]
pub fn cleanup_cancelled_download(
    state: &VrDownloadState,
    persistence_path: &Path,
    transfer_id: &str,
) -> Result<Vec<String>, &'static str> {
    cleanup_cancelled_download_with_reconciliation(
        state,
        persistence_path,
        transfer_id,
        |file_state, target, fingerprint| {
            reconcile_macos_cancelled_cleanup_file(
                persistence_path,
                transfer_id,
                file_state,
                target,
                fingerprint,
            )
        },
        |target, fingerprint| {
            delete_exact_macos_cancelled_cleanup_file(
                persistence_path,
                transfer_id,
                target,
                fingerprint,
            )
        },
        (
            write_cleanup_recovery,
            write_persisted_transfers,
            remove_cleanup_recovery,
        ),
    )
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn cleanup_cancelled_download(
    _state: &VrDownloadState,
    _persistence_path: &Path,
    _transfer_id: &str,
) -> Result<Vec<String>, &'static str> {
    Err(VR_DOWNLOAD_ACTION_INVALID)
}

pub fn dismiss_download(
    state: &VrDownloadState,
    persistence_path: &Path,
    transfer_id: &str,
) -> Result<(), &'static str> {
    let mut context = state.0.lock().map_err(|_| VR_DOWNLOAD_FAILED)?;
    let position = context
        .transfers
        .iter()
        .position(|transfer| match transfer {
            StoredTransfer::Valid(record) => {
                record.transfer_id == transfer_id
                    && record.state.can_dismiss()
                    && record.pending_action.is_none()
            }
            StoredTransfer::Corrupt(record) => record.transfer_id == transfer_id,
        })
        .ok_or(VR_DOWNLOAD_ACTION_INVALID)?;
    invalidate_organization_plan(&mut context);
    let dismissed = context.transfers.remove(position);
    if let Err(error) = write_persisted_transfers(persistence_path, &context.transfers) {
        context.transfers.insert(position, dismissed);
        return Err(error);
    }
    if let StoredTransfer::Valid(record) = &dismissed {
        if let Err(error) = remove_terminal_recovery(persistence_path, record)
            .and_then(|()| remove_organization_recovery(record))
        {
            context.transfers.insert(position, dismissed);
            let _ = write_persisted_transfers(persistence_path, &context.transfers);
            return Err(error);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        fs,
        sync::{
            atomic::{AtomicU64, Ordering},
            TryLockError,
        },
    };

    use super::*;
    use crate::tv_library::{
        scan_tv_library_with, set_tv_folder, trash_tv_file_with_download_ownership, TvLibraryState,
        TV_FILE_TRASH_OWNED,
    };
    use crate::tv_release::fetch_apibay_tv_releases_for_state_with;
    use crate::vr_library::{
        scan_vr_library_with, trash_vr_file_with, VrLibraryState, VR_FILE_TRASH_OWNED,
        VR_FILE_TRASH_OWNERSHIP_UNAVAILABLE,
    };
    use crate::vr_torrent::TvTorrentInspectionStart;

    struct FilesystemFixture {
        path: PathBuf,
    }

    impl FilesystemFixture {
        fn new() -> Self {
            static NEXT_ID: AtomicU64 = AtomicU64::new(1);
            let path = std::env::temp_dir().join(format!(
                "auto-video-vr-download-test-{}-{}",
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).expect("fixture directory must be created");
            Self { path }
        }
    }

    impl Drop for FilesystemFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn fixture_source() -> VerifiedDownloadSource {
        VerifiedDownloadSource {
            bytes: b"fixture torrent bytes".to_vec(),
            code: "MDVR-419".to_owned(),
            infohash: "0123456789abcdef0123456789abcdef01234567".to_owned(),
            release_name: "【VR】 MDVR-419  Exact — 特別版".to_owned(),
            movie_identity: None,
            tv_identity: None,
            selected_files: vec![VerifiedDownloadFile {
                file_id: 0,
                path: "Folder/Part  1 — 映画.mkv".to_owned(),
                size: 5,
            }],
        }
    }

    #[test]
    fn tv_metainfo_reports_shared_session_readiness_without_network_activity() {
        let fixture = FilesystemFixture::new();
        let state = VrDownloadState::default();
        assert_eq!(
            tauri::async_runtime::block_on(acquire_tv_metainfo(
                &state,
                &fixture.path,
                "0123456789abcdef0123456789abcdef01234567",
            )),
            Err(TvMetainfoAcquisitionError::LocalUnavailable)
        );
        assert!(fs::read_dir(&fixture.path)
            .expect("fixture directory must remain readable")
            .next()
            .is_none());

        state
            .0
            .lock()
            .expect("state must remain available")
            .session_starting = true;
        assert_eq!(
            tauri::async_runtime::block_on(acquire_tv_metainfo(
                &state,
                &fixture.path,
                "0123456789abcdef0123456789abcdef01234567",
            )),
            Err(TvMetainfoAcquisitionError::LocalPending)
        );
        assert!(state
            .0
            .lock()
            .expect("state must remain available")
            .session
            .is_none());
    }

    fn persistable_fixture_source() -> VerifiedDownloadSource {
        VerifiedDownloadSource {
            bytes: b"d4:infod6:lengthi5e4:name12:Movie  A.mp412:piece lengthi16384e6:pieces20:aaaaaaaaaaaaaaaaaaaaee".to_vec(),
            code: "MDVR-419".to_owned(),
            infohash: "8b16011989123e1d68a8aaf18f5a599e6a4a0bc7".to_owned(),
            release_name: "【VR】 MDVR-419  Exact — 特別版".to_owned(),
            movie_identity: None,
            tv_identity: None,
            selected_files: vec![VerifiedDownloadFile {
                file_id: 0,
                path: "Movie  A.mp4".to_owned(),
                size: 5,
            }],
        }
    }

    fn persistable_adult_fixture_source() -> VerifiedDownloadSource {
        VerifiedDownloadSource {
            bytes: b"d4:infod6:lengthi5e4:name12:Movie  A.mp412:piece lengthi16384e6:pieces20:aaaaaaaaaaaaaaaaaaaaee".to_vec(),
            code: "ADLT-123".to_owned(),
            infohash: "8b16011989123e1d68a8aaf18f5a599e6a4a0bc7".to_owned(),
            release_name: "【Adult】 ADLT-123  Exact — 特別版".to_owned(),
            movie_identity: None,
            tv_identity: None,
            selected_files: vec![VerifiedDownloadFile {
                file_id: 0,
                path: "Movie  A.mp4".to_owned(),
                size: 5,
            }],
        }
    }

    fn organization_source(files: Vec<(&str, u64)>) -> VerifiedDownloadSource {
        VerifiedDownloadSource {
            bytes: b"organization fixture torrent".to_vec(),
            code: "MDVR-419".to_owned(),
            infohash: "0123456789abcdef0123456789abcdef01234567".to_owned(),
            release_name: "【VR】 MDVR-419  Exact — 特別版".to_owned(),
            movie_identity: None,
            tv_identity: None,
            selected_files: files
                .into_iter()
                .enumerate()
                .map(|(file_id, (path, size))| VerifiedDownloadFile {
                    file_id,
                    path: path.to_owned(),
                    size,
                })
                .collect(),
        }
    }

    fn adult_organization_source(files: Vec<(&str, u64)>) -> VerifiedDownloadSource {
        VerifiedDownloadSource {
            bytes: b"Adult organization fixture torrent".to_vec(),
            code: "ADLT-123".to_owned(),
            infohash: "89abcdef0123456789abcdef0123456789abcdef".to_owned(),
            release_name: "【Adult】 ADLT-123  Exact — 特別版".to_owned(),
            movie_identity: None,
            tv_identity: None,
            selected_files: files
                .into_iter()
                .enumerate()
                .map(|(file_id, (path, size))| VerifiedDownloadFile {
                    file_id,
                    path: path.to_owned(),
                    size,
                })
                .collect(),
        }
    }

    fn movie_organization_metainfo(files: &[(&str, u64)]) -> Vec<u8> {
        let mut info = b"d5:filesl".to_vec();
        for (path, size) in files {
            info.extend_from_slice(format!("d6:lengthi{size}e4:pathl").as_bytes());
            for component in path.split('/') {
                info.extend_from_slice(format!("{}:{component}", component.len()).as_bytes());
            }
            info.extend_from_slice(b"ee");
        }
        info.extend_from_slice(
            b"e4:name12:Movie Bundle12:piece lengthi16384e6:pieces20:aaaaaaaaaaaaaaaaaaaae",
        );
        let mut metainfo = b"d4:info".to_vec();
        metainfo.extend_from_slice(&info);
        metainfo.push(b'e');
        metainfo
    }

    fn movie_organization_source(
        title: &str,
        release_date: Option<&str>,
        provider_title: &str,
        files: &[(&str, u64)],
        selected_file_ids: &[usize],
    ) -> VerifiedDownloadSource {
        let metainfo = movie_organization_metainfo(files);
        let infohash = hex_sha1(&metainfo[b"d4:info".len()..metainfo.len() - 1]);
        let selected_size = selected_file_ids
            .iter()
            .map(|file_id| files[*file_id].1)
            .sum::<u64>();
        let identity = MovieDownloadIdentity {
            tmdb_movie_id: 419,
            tmdb_title: title.to_owned(),
            release_date: release_date.map(str::to_owned),
            imdb_id: "tt0123456".to_owned(),
            provider_movie_id: 700,
            provider_title: Some(provider_title.to_owned()),
            provider_year: Some("1999".to_owned()),
            row_id: "700:0".to_owned(),
            quality: Some("1080p".to_owned()),
            type_label: Some("bluray".to_owned()),
            video_codec: Some("x264".to_owned()),
            size: Some(format!("{selected_size} B")),
            size_bytes: Some(selected_size.to_string()),
            seeds: Some("0".to_owned()),
            peers: Some("0".to_owned()),
            expected_infohash: infohash.clone(),
            torrent_url: format!(
                "https://yts.mx/torrent/download/{}",
                infohash.to_ascii_uppercase()
            ),
        };
        revalidate_persisted_movie_download_source(
            &metainfo,
            &identity,
            &infohash,
            selected_file_ids,
        )
        .expect("Movie organization source must revalidate")
    }

    fn tv_download_source(
        files: &[(&str, u64)],
        selected_file_ids: &[usize],
    ) -> VerifiedDownloadSource {
        let metainfo = movie_organization_metainfo(files);
        let infohash = hex_sha1(&metainfo[b"d4:info".len()..metainfo.len() - 1]);
        let identity = TvDownloadIdentity {
            tmdb_tv_id: 701,
            show_name: "Exact  Show — 特別版".to_owned(),
            provider_season_id: 9001,
            season_number: 2,
            provider_episode_id: 9103,
            episode_number: 3,
            episode_name: "第三話  —  Exact Episode".to_owned(),
            imdb_id: "tt0123456".to_owned(),
            provider_item_id: "1001".to_owned(),
            category: "205".to_owned(),
            release_name: "Exact  Show — 特別版.S02E03+720p.第三話".to_owned(),
            infohash: infohash.clone(),
        };
        revalidate_persisted_tv_download_source(&metainfo, &identity, &infohash, selected_file_ids)
            .expect("TV download source must revalidate")
    }

    fn inspected_tv_torrent(metainfo: Vec<u8>) -> (TvReleaseState, TvTorrentState, String) {
        let infohash = hex_sha1(&metainfo[b"d4:info".len()..metainfo.len() - 1]);
        let release_state = TvReleaseState::default();
        let generation = release_state
            .begin_release_lookup()
            .expect("TV release lookup must begin");
        let standard = format!(
            r#"[{{"id":"1001","name":"Exact  Show — 特別版.S02E03+720p.第三話","info_hash":"{infohash}","leechers":"4","seeders":"12","size":"12","username":"Exact Uploader","added":"1710000000","status":"vip","category":"205","imdb":"tt0123456"}}]"#
        );
        fetch_apibay_tv_releases_for_state_with(
            &release_state,
            generation,
            701,
            9001,
            9103,
            "fixture-token",
            |url, _| {
                if url.ends_with("/tv/701") {
                    Ok(r#"{"id":701,"name":"Exact  Show — 特別版","seasons":[{"id":9001,"season_number":2}]}"#.to_owned())
                } else if url.ends_with("/season/2") {
                    Ok(r#"{"id":9001,"season_number":2,"episodes":[{"id":9103,"season_number":2,"episode_number":3,"name":"第三話  —  Exact Episode"}]}"#.to_owned())
                } else if url.ends_with("/external_ids") {
                    Ok(r#"{"id":701,"imdb_id":"tt0123456"}"#.to_owned())
                } else if url.ends_with("cat=205") {
                    Ok(standard.clone())
                } else {
                    Ok("[]".to_owned())
                }
            },
        )
        .expect("exact TV release must be trusted");
        release_state
            .select_release(701, 9001, 9103, "1001")
            .expect("exact TV release must be selected");
        let torrent_state = TvTorrentState::default();
        let plan = match torrent_state
            .begin_inspection(&release_state, 701, 9001, 9103, "1001")
            .expect("exact TV inspection must begin")
        {
            TvTorrentInspectionStart::Acquire(plan) => plan,
            TvTorrentInspectionStart::Cached(_) => unreachable!(),
        };
        let response = torrent_state
            .finish_inspection(&release_state, plan, metainfo)
            .expect("exact TV metainfo must inspect");
        (release_state, torrent_state, response[0].clone())
    }

    fn completed_organization_record_for_category(
        category: TransferCategory,
        destination: &Path,
        source: VerifiedDownloadSource,
    ) -> TransferRecord {
        let mut record = transfer_from_source(
            category,
            source,
            destination.to_owned(),
            TransferState::Completed,
        );
        for (index, file) in record.selected_files.iter().enumerate() {
            let target = selected_target(destination, file).expect("selected target must resolve");
            fs::create_dir_all(target.parent().expect("selected target must have a parent"))
                .expect("selected parent must exist");
            fs::write(&target, vec![b'a' + index as u8; file.size as usize])
                .expect("selected contents must exist");
        }
        record.downloaded_bytes = record.selected_total();
        record.fingerprints = capture_fingerprints(&record).expect("fingerprints must resolve");
        record
    }

    fn completed_organization_record(
        destination: &Path,
        source: VerifiedDownloadSource,
    ) -> TransferRecord {
        completed_organization_record_for_category(TransferCategory::Vr, destination, source)
    }

    fn completed_adult_organization_record(
        destination: &Path,
        source: VerifiedDownloadSource,
    ) -> TransferRecord {
        completed_organization_record_for_category(TransferCategory::Adult, destination, source)
    }

    fn completed_movie_organization_record(
        destination: &Path,
        source: VerifiedDownloadSource,
    ) -> TransferRecord {
        completed_organization_record_for_category(TransferCategory::Movie, destination, source)
    }

    fn completed_tv_organization_record(
        destination: &Path,
        source: VerifiedDownloadSource,
    ) -> TransferRecord {
        completed_organization_record_for_category(TransferCategory::Tv, destination, source)
    }

    fn organization_state(record: TransferRecord) -> (VrDownloadState, String) {
        let transfer_id = record.transfer_id.clone();
        let destination = record.destination.clone();
        let persistence_path = destination.join(".test-downloads");
        let category = record.category;
        let state = VrDownloadState::default();
        {
            let mut context = state.0.lock().expect("state must lock");
            match category {
                TransferCategory::Adult => context.adult_future_folder = Some(destination),
                TransferCategory::Movie => context.movie_future_folder = Some(destination),
                TransferCategory::Tv => context.tv_future_folder = Some(destination),
                TransferCategory::Vr => context.future_folder = Some(destination),
            }
            context.transfers_loaded = true;
            context.persistence_path = Some(persistence_path);
            context.transfers.push(StoredTransfer::Valid(record));
        }
        (state, transfer_id)
    }

    fn transfer_snapshots(state: &VrDownloadState) -> Vec<Vec<u8>> {
        state
            .0
            .lock()
            .expect("state must lock")
            .transfers
            .iter()
            .map(|transfer| match transfer {
                StoredTransfer::Valid(record) => {
                    encode_transfer(record).expect("transfer snapshot must encode")
                }
                StoredTransfer::Corrupt(record) => record.raw_line.clone(),
            })
            .collect()
    }

    fn transfer_rows(state: &VrDownloadState) -> Vec<String> {
        download_rows(&mut state.0.lock().expect("state must lock"))
    }

    fn cancelled_cleanup_state(
        record: TransferRecord,
        persistence_path: &Path,
    ) -> (VrDownloadState, String) {
        let transfer_id = record.transfer_id.clone();
        write_persisted_transfers(persistence_path, &[StoredTransfer::Valid(record.clone())])
            .expect("cancelled cleanup authority must persist");
        let state = VrDownloadState::default();
        {
            let mut context = state.0.lock().expect("state must lock");
            configure_category_context(&mut context, record.category, record.destination.clone());
            context.transfers_loaded = true;
            context.persistence_path = Some(persistence_path.to_owned());
            context.transfers.push(StoredTransfer::Valid(record));
        }
        (state, transfer_id)
    }

    #[cfg(target_os = "macos")]
    fn prepared_macos_cleanup_mutation(
        fixture: &FilesystemFixture,
        label: &str,
    ) -> (PathBuf, TransferRecord, MacosCleanupMutation, PathBuf) {
        let mut record = cancelled_record_for_category(fixture, TransferCategory::Vr, label);
        record.state = TransferState::Cleanup;
        let target = current_target(&record, 0).expect("selected target must resolve");
        let recovery = CleanupRecovery {
            record: record.clone(),
            files: vec![CleanupFileState::Present],
        };
        let persistence_path = fixture.path.join("downloads");
        write_persisted_transfers(&persistence_path, &[StoredTransfer::Valid(record.clone())])
            .expect("cleanup row must persist");
        write_cleanup_recovery(&persistence_path, &recovery)
            .expect("cleanup recovery must persist");
        let staging_path = macos_cleanup_staging_path(&record, 0, &record.fingerprints[0])
            .expect("staging path must resolve");
        let mutation = MacosCleanupMutation {
            staging_token: macos_cleanup_staging_token(
                &record,
                0,
                &staging_path,
                &record.fingerprints[0],
            ),
            record: record.clone(),
            selected_index: 0,
            target_path: target.clone(),
            staging_path,
            expected_fingerprint: record.fingerprints[0].clone(),
            phase: MacosCleanupMutationPhase::StagingCreationPrepared,
        };
        (persistence_path, record, mutation, target)
    }

    #[cfg(target_os = "macos")]
    fn restarted_macos_cleanup_state(
        fixture: &FilesystemFixture,
        persistence_path: &Path,
        record: &TransferRecord,
    ) -> VrDownloadState {
        let state = VrDownloadState::default();
        {
            let mut context = state.0.lock().expect("state must lock");
            configure_category_context(&mut context, record.category, record.destination.clone());
        }
        let rows = tauri::async_runtime::block_on(load_downloads(
            &state,
            persistence_path,
            &fixture.path.join("restart-session"),
            &fixture.path.join("restart-limit"),
        ))
        .expect("cleanup recovery must reload");
        assert_eq!(rows[8], "cleanup");
        assert!(state.0.lock().expect("state must lock").session.is_none());
        state
    }

    fn configure_category_context(
        context: &mut VrDownloadContext,
        category: TransferCategory,
        destination: PathBuf,
    ) {
        match category {
            TransferCategory::Adult => context.adult_future_folder = Some(destination),
            TransferCategory::Movie => context.movie_future_folder = Some(destination),
            TransferCategory::Tv => context.tv_future_folder = Some(destination),
            TransferCategory::Vr => context.future_folder = Some(destination),
        }
    }

    fn cancelled_record_for_category(
        fixture: &FilesystemFixture,
        category: TransferCategory,
        label: &str,
    ) -> TransferRecord {
        let mut record = terminal_record_for_category(fixture, category, label);
        record.state = TransferState::Cancelled;
        record.handle = None;
        record.pending_action = None;
        record
    }

    fn terminal_record_for_category(
        fixture: &FilesystemFixture,
        category: TransferCategory,
        label: &str,
    ) -> TransferRecord {
        let destination = fixture.path.join(label);
        fs::create_dir_all(&destination).expect("terminal destination must exist");
        let destination =
            fs::canonicalize(destination).expect("terminal destination must canonicalize");
        let source = match category {
            TransferCategory::Adult => persistable_adult_fixture_source(),
            TransferCategory::Movie => movie_organization_source(
                "Exact Movie",
                Some("1999-04-19"),
                "Exact Provider Movie",
                &[("Provider/Feature.mp4", 5)],
                &[0],
            ),
            TransferCategory::Tv => tv_download_source(&[("Show.S02E03.mkv", 5)], &[0]),
            TransferCategory::Vr => persistable_fixture_source(),
        };
        let mut record =
            transfer_from_source(category, source, destination, TransferState::Downloading);
        for file in &record.selected_files {
            let target = selected_target(&record.destination, file)
                .expect("terminal selected path must resolve");
            fs::create_dir_all(
                target
                    .parent()
                    .expect("terminal selected path must have a parent"),
            )
            .expect("terminal selected parent must exist");
            fs::write(target, vec![b'p'; file.size as usize])
                .expect("terminal selected media must exist");
        }
        record.fingerprints = capture_fingerprints(&record).expect("fingerprints must resolve");
        record.downloaded_bytes = 2;
        record
    }

    fn configure_category_folder(
        state: &VrDownloadState,
        category: TransferCategory,
        destination: &Path,
    ) {
        let mut context = state.0.lock().expect("state must lock");
        match category {
            TransferCategory::Adult => context.adult_future_folder = Some(destination.to_owned()),
            TransferCategory::Movie => context.movie_future_folder = Some(destination.to_owned()),
            TransferCategory::Tv => context.tv_future_folder = Some(destination.to_owned()),
            TransferCategory::Vr => context.future_folder = Some(destination.to_owned()),
        }
    }

    fn test_terminal_recovery_path(persistence_path: &Path, record: &TransferRecord) -> PathBuf {
        terminal_recovery_path(persistence_path, record)
            .expect("terminal recovery path must resolve")
    }

    fn unchecked_terminal_recovery(record: &TransferRecord, generation: u64) -> Vec<u8> {
        let encoded_record = encode_transfer(record).expect("recovery fixture must encode");
        let mut checksum_input = generation.to_be_bytes().to_vec();
        checksum_input.extend_from_slice(&encoded_record);
        let mut bytes = TERMINAL_RECOVERY_HEADER.to_vec();
        bytes.extend_from_slice(generation.to_string().as_bytes());
        bytes.push(b'\n');
        bytes.extend_from_slice(hex_sha1(&checksum_input).as_bytes());
        bytes.push(b'\n');
        bytes.extend_from_slice(&encoded_record);
        bytes.extend_from_slice(b"\n");
        bytes
    }

    #[test]
    fn every_transfer_category_exposes_completion_only_after_exact_terminal_authority_is_durable() {
        for (category, label) in [
            (TransferCategory::Movie, "Movies — terminal"),
            (TransferCategory::Adult, "Adult — terminal"),
            (TransferCategory::Tv, "TV — terminal"),
            (TransferCategory::Vr, "VR — terminal"),
        ] {
            let fixture = FilesystemFixture::new();
            let record = terminal_record_for_category(&fixture, category, label);
            let transfer_id = record.transfer_id.clone();
            let destination = record.destination.clone();
            let selected_path = current_target(&record, 0).expect("selected path must resolve");
            let persistence_path = fixture.path.join("downloads");
            write_persisted_transfers(&persistence_path, &[StoredTransfer::Valid(record)])
                .expect("active authority must persist");
            let mut context = VrDownloadContext {
                transfers_loaded: true,
                transfers: read_persisted_transfers(&persistence_path)
                    .expect("active authority must reload"),
                ..VrDownloadContext::default()
            };
            let mut persistence_attempts = 0;

            assert!(finalize_monitored_transfer_with(
                &mut context,
                &transfer_id,
                0,
                true,
                &persistence_path,
                |path, transfers| {
                    persistence_attempts += 1;
                    assert!(test_terminal_recovery_path(
                        path,
                        match &transfers[0] {
                            StoredTransfer::Valid(record) => record,
                            StoredTransfer::Corrupt(_) => {
                                panic!("terminal authority must be valid")
                            }
                        },
                    )
                    .is_file());
                    assert!(matches!(
                        &transfers[0],
                        StoredTransfer::Valid(record)
                            if record.state == TransferState::Completed
                                && record.downloaded_bytes == record.selected_total()
                    ));
                    write_persisted_transfers(path, transfers)
                },
                write_terminal_recovery,
            ));
            assert_eq!(persistence_attempts, 1);
            assert!(matches!(
                context.transfers.as_slice(),
                [StoredTransfer::Valid(record)]
                    if record.state == TransferState::Completed
                        && record.handle.is_none()
                        && record.terminal_recovery_generation.is_none()
            ));
            assert!(!terminal_recovery_directory(&persistence_path)
                .expect("terminal recovery directory must resolve")
                .join(format!(
                    "{TERMINAL_RECOVERY_PREFIX}{transfer_id}{TERMINAL_RECOVERY_SUFFIX}"
                ))
                .exists());

            let restarted = VrDownloadState::default();
            configure_category_folder(&restarted, category, &destination);
            let rows = tauri::async_runtime::block_on(load_downloads(
                &restarted,
                &persistence_path,
                &fixture.path.join("terminal-session"),
                &fixture.path.join("download-limit"),
            ))
            .expect("durable completion must reload");
            assert_eq!(rows[0], transfer_id);
            assert_eq!(rows[1], category.as_str());
            assert_eq!(rows[8], "completed");
            assert_eq!(rows[13], "false");
            assert!(restarted
                .0
                .lock()
                .expect("state must lock")
                .session
                .is_none());
            assert_eq!(
                fs::read(selected_path).expect("terminal media must remain readable"),
                vec![b'p'; 5]
            );
        }
    }

    #[test]
    fn exact_terminal_recovery_remains_visible_while_primary_persistence_keeps_failing() {
        let fixture = FilesystemFixture::new();
        let record = completed_selected_boundary_record(&fixture, Some(b"abc"));
        let transfer_id = record.transfer_id.clone();
        let destination = record.destination.clone();
        let persistence_path = fixture.path.join("downloads");
        write_persisted_transfers(&persistence_path, &[StoredTransfer::Valid(record)])
            .expect("old active authority must persist");
        let mut context = VrDownloadContext {
            transfers_loaded: true,
            transfers: read_persisted_transfers(&persistence_path)
                .expect("old active authority must reload"),
            ..VrDownloadContext::default()
        };
        let mut persistence_attempts = 0;

        assert!(finalize_monitored_transfer_with(
            &mut context,
            &transfer_id,
            0,
            true,
            &persistence_path,
            |_, transfers| {
                persistence_attempts += 1;
                let expected = if persistence_attempts == 1 {
                    TransferState::Completed
                } else {
                    TransferState::Failed
                };
                assert!(matches!(
                    &transfers[0],
                    StoredTransfer::Valid(record) if record.state == expected
                ));
                Err(VR_DOWNLOAD_PERSISTENCE_FAILED)
            },
            write_terminal_recovery,
        ));
        assert_eq!(persistence_attempts, 2);
        let recovery_path = test_terminal_recovery_path(
            &persistence_path,
            match &context.transfers[0] {
                StoredTransfer::Valid(record) => record,
                StoredTransfer::Corrupt(_) => {
                    panic!("recovered terminal authority must be valid")
                }
            },
        );
        assert!(recovery_path.is_file());
        assert_eq!(download_rows(&mut context)[8], "failed");
        assert_eq!(download_rows(&mut context)[13], "true");

        let restarted = VrDownloadState::default();
        configure_category_folder(&restarted, TransferCategory::Vr, &destination);
        let rows = tauri::async_runtime::block_on(load_downloads_with_persistence(
            &restarted,
            &persistence_path,
            &fixture.path.join("failed-terminal-session"),
            &fixture.path.join("download-limit"),
            |_, _| Err(VR_DOWNLOAD_PERSISTENCE_FAILED),
        ))
        .expect("terminal recovery must survive a failed primary rewrite during load");
        assert_eq!(rows[0], transfer_id);
        assert_eq!(rows[1], "vr");
        assert_eq!(rows[6], "7");
        assert_eq!(rows[8], "failed");
        assert_eq!(rows[10], "none");
        assert_eq!(rows[12], "false");
        assert_eq!(rows[13], "true");
        assert_eq!(
            list_downloads_with_persistence(&restarted, &persistence_path, |_, _| {
                Err(VR_DOWNLOAD_PERSISTENCE_FAILED)
            })
            .expect("terminal recovery must survive a failed primary rewrite during list"),
            rows
        );
        assert!(recovery_path.is_file());
        assert!(matches!(
            restarted
                .0
                .lock()
                .expect("state must lock")
                .transfers
                .as_slice(),
            [StoredTransfer::Valid(record)] if record.handle.is_none()
        ));
        assert_eq!(
            preview_organization(&restarted, &transfer_id),
            Err(VR_ORGANIZATION_INELIGIBLE)
        );
        let context = restarted.0.lock().expect("state must lock");
        let StoredTransfer::Valid(record) = &context.transfers[0] else {
            panic!("recovered terminal authority must remain valid");
        };
        assert_eq!(
            record
                .boundary_segments
                .lock()
                .expect("boundary state must lock")[&0][0]
                .bytes,
            b"abc"
        );
        assert_eq!(
            fs::read(destination.join("Folder/特別版  B.mp4"))
                .expect("selected media must remain readable"),
            b"1234567"
        );
        assert!(!destination.join("Folder/Part  1 — 映画.mkv").exists());
    }

    #[test]
    fn completed_primary_remains_authoritative_when_recovery_cleanup_fails() {
        let fixture = FilesystemFixture::new();
        let record = terminal_record_for_category(
            &fixture,
            TransferCategory::Movie,
            "Movies — committed completion",
        );
        let transfer_id = record.transfer_id.clone();
        let destination = record.destination.clone();
        let media_path = current_target(&record, 0).expect("selected path must resolve");
        let media_bytes = fs::read(&media_path).expect("selected media must remain readable");
        let persistence_path = fixture.path.join("downloads");
        write_persisted_transfers(&persistence_path, &[StoredTransfer::Valid(record)])
            .expect("active primary authority must persist");
        let mut context = VrDownloadContext {
            movie_future_folder: Some(destination.clone()),
            transfers_loaded: true,
            transfers: read_persisted_transfers(&persistence_path)
                .expect("active primary authority must reload"),
            ..VrDownloadContext::default()
        };
        let recovery_path = test_terminal_recovery_path(
            &persistence_path,
            match &context.transfers[0] {
                StoredTransfer::Valid(record) => record,
                StoredTransfer::Corrupt(_) => panic!("active authority must remain valid"),
            },
        );
        let mut persistence_attempts = 0;

        assert!(finalize_monitored_transfer_with(
            &mut context,
            &transfer_id,
            0,
            true,
            &persistence_path,
            |path, transfers| {
                persistence_attempts += 1;
                assert_eq!(
                    persistence_attempts, 1,
                    "completed authority must not be downgraded"
                );
                assert!(matches!(
                    &transfers[0],
                    StoredTransfer::Valid(record)
                        if record.state == TransferState::Completed
                            && record.downloaded_bytes == record.selected_total()
                ));
                write_persisted_transfers(path, transfers)?;
                fs::remove_file(&recovery_path).map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
                fs::create_dir(&recovery_path).map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
                Ok(())
            },
            write_terminal_recovery,
        ));
        assert_eq!(persistence_attempts, 1);
        assert!(matches!(
            context.transfers.as_slice(),
            [StoredTransfer::Valid(record)]
                if record.state == TransferState::Completed
                    && record.handle.is_none()
                    && record.terminal_recovery_generation.is_none()
        ));
        let current_rows = download_rows(&mut context);
        assert!(matches!(
            &read_persisted_transfers(&persistence_path)
                .expect("completed primary authority must remain readable")[0],
            StoredTransfer::Valid(record) if record.state == TransferState::Completed
        ));

        let restarted = VrDownloadState::default();
        configure_category_folder(&restarted, TransferCategory::Movie, &destination);
        let rows = tauri::async_runtime::block_on(load_downloads(
            &restarted,
            &persistence_path,
            &fixture.path.join("completed-primary-session"),
            &fixture.path.join("download-limit"),
        ))
        .expect("durable completion must remain visible during cleanup failure");
        assert_eq!(rows, current_rows);
        assert_eq!(rows[0], transfer_id);
        assert_eq!(rows[1], "movie");
        assert_eq!(rows[8], "completed");
        assert_eq!(rows[13], "false");
        assert!(restarted
            .0
            .lock()
            .expect("state must lock")
            .session
            .is_none());
        assert_eq!(
            fs::read(media_path).expect("completed media must remain readable"),
            media_bytes
        );
        fs::remove_dir(&recovery_path).expect("cleanup fixture must be removable");
    }

    #[test]
    fn old_destination_terminal_recovery_survives_unreadable_primary_for_all_categories() {
        for (category, label) in [
            (TransferCategory::Movie, "Movies — old terminal destination"),
            (TransferCategory::Adult, "Adult — old terminal destination"),
            (TransferCategory::Tv, "TV — old terminal destination"),
            (TransferCategory::Vr, "VR — old terminal destination"),
        ] {
            let fixture = FilesystemFixture::new();
            let record = terminal_record_for_category(&fixture, category, label);
            let transfer_id = record.transfer_id.clone();
            let old_destination = record.destination.clone();
            let media_path = current_target(&record, 0).expect("selected path must resolve");
            let media_bytes = fs::read(&media_path).expect("selected media must be readable");
            let persistence_path = fixture.path.join("downloads");
            write_persisted_transfers(&persistence_path, &[StoredTransfer::Valid(record)])
                .expect("old active primary authority must persist");
            let mut context = VrDownloadContext {
                transfers_loaded: true,
                transfers: read_persisted_transfers(&persistence_path)
                    .expect("old active primary authority must reload"),
                ..VrDownloadContext::default()
            };
            let mut persistence_attempts = 0;
            assert!(finalize_monitored_transfer_with(
                &mut context,
                &transfer_id,
                0,
                true,
                &persistence_path,
                |_, _| {
                    persistence_attempts += 1;
                    Err(VR_DOWNLOAD_PERSISTENCE_FAILED)
                },
                write_terminal_recovery,
            ));
            assert_eq!(persistence_attempts, 2);
            let recovery_path = test_terminal_recovery_path(
                &persistence_path,
                match &context.transfers[0] {
                    StoredTransfer::Valid(record) => record,
                    StoredTransfer::Corrupt(_) => panic!("terminal recovery must remain valid"),
                },
            );
            assert!(recovery_path.is_file());

            let replacement_destination = fixture
                .path
                .join(format!("{} — current replacement", category.as_str()));
            fs::create_dir(&replacement_destination).expect("replacement destination must exist");
            let replacement_destination = fs::canonicalize(replacement_destination)
                .expect("replacement destination must canonicalize");
            fs::remove_file(&persistence_path).expect("primary path must be replaceable");
            fs::create_dir(&persistence_path).expect("primary persistence must remain unreadable");

            let restarted = VrDownloadState::default();
            configure_category_folder(&restarted, category, &replacement_destination);
            let rows = tauri::async_runtime::block_on(load_downloads(
                &restarted,
                &persistence_path,
                &fixture
                    .path
                    .join(format!("{}-old-destination-session", category.as_str())),
                &fixture.path.join("download-limit"),
            ))
            .expect("old-destination terminal recovery must remain discoverable");
            assert_eq!(rows[0], transfer_id);
            assert_eq!(rows[1], category.as_str());
            assert_eq!(rows[8], "failed");
            assert_eq!(rows[9], "false");
            assert_eq!(rows[10], "none");
            assert_eq!(rows[12], "false");
            assert_eq!(rows[13], "true");
            assert!(restarted
                .0
                .lock()
                .expect("state must lock")
                .session
                .is_none());
            assert_eq!(
                fs::read(&media_path).expect("old-destination media must remain readable"),
                media_bytes
            );
            assert_ne!(old_destination, replacement_destination);
        }
    }

    #[test]
    fn unavailable_terminal_recovery_persists_failed_without_exposing_completed() {
        let fixture = FilesystemFixture::new();
        let record = terminal_record_for_category(
            &fixture,
            TransferCategory::Adult,
            "Adult — recovery unavailable",
        );
        let transfer_id = record.transfer_id.clone();
        let destination = record.destination.clone();
        let persistence_path = fixture.path.join("downloads");
        write_persisted_transfers(&persistence_path, &[StoredTransfer::Valid(record)])
            .expect("active authority must persist");
        let mut context = VrDownloadContext {
            transfers_loaded: true,
            transfers: read_persisted_transfers(&persistence_path)
                .expect("active authority must reload"),
            ..VrDownloadContext::default()
        };
        let mut persistence_attempts = 0;
        assert!(finalize_monitored_transfer_with(
            &mut context,
            &transfer_id,
            0,
            true,
            &persistence_path,
            |path, transfers| {
                persistence_attempts += 1;
                assert!(matches!(
                    &transfers[0],
                    StoredTransfer::Valid(record) if record.state == TransferState::Failed
                ));
                write_persisted_transfers(path, transfers)
            },
            |_, _, _| Err(VR_DOWNLOAD_PERSISTENCE_FAILED),
        ));
        assert_eq!(persistence_attempts, 1);
        assert!(matches!(
            &read_persisted_transfers(&persistence_path)
                .expect("failed primary authority must reload")[0],
            StoredTransfer::Valid(record) if record.state == TransferState::Failed
        ));

        let restarted = VrDownloadState::default();
        configure_category_folder(&restarted, TransferCategory::Adult, &destination);
        let rows = tauri::async_runtime::block_on(load_downloads(
            &restarted,
            &persistence_path,
            &fixture.path.join("failed-primary-session"),
            &fixture.path.join("download-limit"),
        ))
        .expect("durable failed primary authority must reload");
        assert_eq!(rows[1], "adult");
        assert_eq!(rows[8], "failed");
        assert_eq!(rows[13], "false");
        assert!(restarted
            .0
            .lock()
            .expect("state must lock")
            .session
            .is_none());
    }

    #[test]
    fn terminal_double_persistence_failure_keeps_active_handle_and_restart_authority() {
        let fixture = FilesystemFixture::new();
        let mut record = completed_selected_boundary_record(&fixture, Some(b"abc"));
        record.state = TransferState::Paused;
        fs::write(record.destination.join("Folder/特別版  B.mp4"), b"partial")
            .expect("partial selected media must remain writable");
        record.downloaded_bytes = 0;
        let transfer_id = record.transfer_id.clone();
        let destination = record.destination.clone();
        let persistence_path = fixture.path.join("downloads");
        write_persisted_transfers(&persistence_path, &[StoredTransfer::Valid(record)])
            .expect("active authority must persist");
        let active_primary_bytes =
            fs::read(&persistence_path).expect("active authority bytes must remain readable");

        tauri::async_runtime::block_on(async {
            let mut stored =
                read_persisted_transfers(&persistence_path).expect("active authority must reload");
            let StoredTransfer::Valid(record) = stored.pop().expect("active record must exist")
            else {
                panic!("active authority must remain valid");
            };
            let session_folder = fixture.path.join("active-session");
            fs::create_dir(&session_folder).expect("active session folder must exist");
            let session = Session::new_with_opts(session_folder, session_options(None))
                .await
                .expect("active session must start");
            let handle = add_record_to_session(&session, &record, true)
                .await
                .expect("active handle must attach");
            let state = VrDownloadState::default();
            configure_category_folder(&state, TransferCategory::Vr, &destination);
            {
                let mut context = state.0.lock().expect("state must lock");
                context.download_limit = DownloadLimitState::Loaded(None);
                context.transfers_loaded = true;
                context.session = Some(session.clone());
                let mut record = record;
                record.handle = Some(handle.clone());
                context.transfers.push(StoredTransfer::Valid(record));

                let mut persistence_attempts = 0;
                assert!(!finalize_monitored_transfer_with(
                    &mut context,
                    &transfer_id,
                    0,
                    true,
                    &persistence_path,
                    |path, transfers| {
                        persistence_attempts += 1;
                        assert!(matches!(
                            &transfers[0],
                            StoredTransfer::Valid(record)
                                if record.state == TransferState::Failed
                                    && record.downloaded_bytes == 7
                                    && record.handle.as_ref().is_some_and(|current| Arc::ptr_eq(current, &handle))
                        ));
                        write_persisted_transfers_with(
                            path,
                            transfers,
                            |replacement, bytes| {
                                let mut file = OpenOptions::new()
                                    .create_new(true)
                                    .write(true)
                                    .open(replacement)
                                    .map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
                                let partial_length = (bytes.len() / 2).max(1);
                                file.write_all(&bytes[..partial_length])
                                    .and_then(|()| file.sync_all())
                                    .map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
                                Err(VR_DOWNLOAD_PERSISTENCE_FAILED)
                            },
                            |_, _| panic!("partial replacement reached atomic rename"),
                        )
                    },
                    |path, record, generation| {
                        assert_eq!(path, persistence_path);
                        assert_eq!(record.state, TransferState::Failed);
                        assert_eq!(generation, 0);
                        Err(VR_DOWNLOAD_PERSISTENCE_FAILED)
                    },
                ));
                assert_eq!(persistence_attempts, 1);
                let StoredTransfer::Valid(record) = &context.transfers[0] else {
                    panic!("active authority must remain valid");
                };
                assert_eq!(record.state, TransferState::Paused);
                assert_eq!(record.downloaded_bytes, 0);
                assert!(record
                    .handle
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, &handle)));
                assert_eq!(
                    record
                        .boundary_segments
                        .lock()
                        .expect("boundary state must lock")[&0][0]
                        .bytes,
                    b"abc"
                );
            }

            assert_eq!(
                fs::read(&persistence_path).expect("older active bytes must remain readable"),
                active_primary_bytes
            );
            assert!(!persistence_replacement_path(&persistence_path)
                .expect("replacement path must resolve")
                .exists());
            let persisted = read_persisted_transfers(&persistence_path)
                .expect("older active authority must remain readable");
            assert!(matches!(
                &persisted[0],
                StoredTransfer::Valid(record)
                    if record.state == TransferState::Paused && record.downloaded_bytes == 0
            ));
            assert_eq!(
                fs::read(destination.join("Folder/特別版  B.mp4"))
                    .expect("partial media must remain readable"),
                b"partial"
            );
            {
                let mut context = state.0.lock().expect("state must lock");
                let record = find_valid_record_mut(&mut context.transfers, &transfer_id)
                    .expect("active authority must remain attached");
                assert!(record.handle.take().is_some());
                context.session = None;
            }
            session
                .delete(handle.id().into(), false)
                .await
                .expect("old process handle must detach without deleting media");

            let restarted = VrDownloadState::default();
            configure_category_folder(&restarted, TransferCategory::Vr, &destination);
            let rows = load_downloads(
                &restarted,
                &persistence_path,
                &fixture.path.join("restarted-session"),
                &fixture.path.join("download-limit"),
            )
            .await
            .expect("older active authority must restore after relaunch");
            assert_eq!(rows[0], transfer_id);
            assert_eq!(rows[6], "0");
            assert_eq!(rows[8], "paused");
            assert_eq!(rows[13], "false");
            let (restarted_session, restarted_handle) = {
                let mut context = restarted.0.lock().expect("state must lock");
                let StoredTransfer::Valid(record) = &mut context.transfers[0] else {
                    panic!("restarted active authority must remain valid");
                };
                assert_eq!(
                    record
                        .boundary_segments
                        .lock()
                        .expect("boundary state must lock")[&0][0]
                        .bytes,
                    b"abc"
                );
                let handle = record.handle.take().expect("restarted handle must attach");
                let session = context
                    .session
                    .take()
                    .expect("restarted session must exist");
                (session, handle)
            };
            restarted_session
                .delete(restarted_handle.id().into(), false)
                .await
                .expect("restarted handle must detach without deleting media");
        });
        assert_eq!(
            fs::read(destination.join("Folder/特別版  B.mp4"))
                .expect("relaunch must retain partial media"),
            b"partial"
        );
        assert!(!destination.join("Folder/Part  1 — 映画.mkv").exists());
    }

    #[test]
    fn conflicting_or_malformed_terminal_recovery_cannot_authorize_a_terminal_row() {
        let fixture = FilesystemFixture::new();
        let record = completed_selected_boundary_record(&fixture, Some(b"abc"));
        let destination = record.destination.clone();
        let persistence_path = fixture.path.join("downloads");
        write_persisted_transfers(&persistence_path, &[StoredTransfer::Valid(record)])
            .expect("active authority must persist");
        let mut transfers =
            read_persisted_transfers(&persistence_path).expect("active authority must reload");
        let StoredTransfer::Valid(record) = &mut transfers[0] else {
            panic!("active authority must remain valid");
        };
        record.state = TransferState::Failed;
        record.downloaded_bytes = record.selected_total();
        write_terminal_recovery(&persistence_path, record, 7)
            .expect("exact terminal recovery must persist");
        record.state = TransferState::Downloading;
        record.downloaded_bytes = 1;
        record
            .boundary_segments
            .lock()
            .expect("boundary state must lock")
            .get_mut(&0)
            .expect("boundary file must remain present")[0]
            .bytes = b"abd".to_vec();
        write_persisted_transfers(&persistence_path, &transfers)
            .expect("conflicting active authority must persist");

        let restarted = VrDownloadState::default();
        configure_category_folder(&restarted, TransferCategory::Vr, &destination);
        let rows = tauri::async_runtime::block_on(load_downloads(
            &restarted,
            &persistence_path,
            &fixture.path.join("conflicting-session"),
            &fixture.path.join("download-limit"),
        ))
        .expect("conflicting recovery must fail closed locally");
        assert_eq!(rows[8], "offline");
        assert_eq!(rows[13], "false");
        assert!(matches!(
            restarted
                .0
                .lock()
                .expect("state must lock")
                .transfers
                .as_slice(),
            [StoredTransfer::Valid(record)] if record.handle.is_none()
        ));

        let malformed_fixture = FilesystemFixture::new();
        let mut malformed_record = terminal_record_for_category(
            &malformed_fixture,
            TransferCategory::Movie,
            "Movies — malformed terminal",
        );
        malformed_record.state = TransferState::Failed;
        let malformed_destination = malformed_record.destination.clone();
        let malformed_persistence_path = malformed_fixture.path.join("downloads");
        let malformed_path =
            test_terminal_recovery_path(&malformed_persistence_path, &malformed_record);
        fs::create_dir_all(
            malformed_path
                .parent()
                .expect("malformed recovery must have a parent"),
        )
        .expect("malformed recovery parent must exist");
        fs::write(
            &malformed_path,
            b"AUTO_VIDEO_TRANSFER_TERMINAL_V1\ninvalid\n",
        )
        .expect("malformed recovery fixture must write");
        assert!(parse_terminal_recovery(&malformed_persistence_path, &malformed_path).is_none());
        let malformed_state = VrDownloadState::default();
        configure_category_folder(
            &malformed_state,
            TransferCategory::Movie,
            &malformed_destination,
        );
        let rows = tauri::async_runtime::block_on(load_downloads(
            &malformed_state,
            &malformed_persistence_path,
            &malformed_fixture.path.join("malformed-session"),
            &malformed_fixture.path.join("download-limit"),
        ))
        .expect("malformed recovery must not create a global load error");
        assert!(rows.is_empty());
        assert!(malformed_state
            .0
            .lock()
            .expect("state must lock")
            .session
            .is_none());
    }

    #[test]
    fn stale_cross_category_destination_infohash_and_file_recoveries_fail_closed() {
        let fixture = FilesystemFixture::new();
        let mut record = completed_selected_boundary_record(&fixture, Some(b"abc"));
        record.state = TransferState::Failed;
        record.downloaded_bytes = record.selected_total();
        let persistence_path = fixture.path.join("downloads");
        let exact_path = test_terminal_recovery_path(&persistence_path, &record);
        fs::create_dir_all(
            exact_path
                .parent()
                .expect("exact recovery must have a parent"),
        )
        .expect("exact recovery parent must exist");
        let exact_bytes = encoded_terminal_recovery(&record, 11)
            .expect("exact terminal recovery fixture must encode");

        let stale_path = terminal_recovery_directory(&persistence_path)
            .expect("terminal recovery directory must resolve")
            .join(format!(
                "{TERMINAL_RECOVERY_PREFIX}0000000000000000000000000000000000000000{TERMINAL_RECOVERY_SUFFIX}"
            ));
        fs::write(&stale_path, &exact_bytes).expect("stale recovery fixture must write");
        assert!(parse_terminal_recovery(&persistence_path, &stale_path).is_none());
        fs::remove_file(&stale_path).expect("stale fixture must be removable");

        let wrong_destination = fixture.path.join("VR — wrong destination");
        fs::create_dir(&wrong_destination).expect("wrong destination must exist");
        let wrong_destination =
            fs::canonicalize(wrong_destination).expect("wrong destination must canonicalize");
        let wrong_destination_path = wrong_destination.join(
            exact_path
                .file_name()
                .expect("recovery path must have a filename"),
        );
        fs::write(&wrong_destination_path, &exact_bytes)
            .expect("wrong-destination recovery fixture must write");
        assert!(parse_terminal_recovery(&persistence_path, &wrong_destination_path).is_none());

        for (case, mutate) in [
            (
                "category",
                (|record: &mut TransferRecord| record.category = TransferCategory::Adult)
                    as fn(&mut TransferRecord),
            ),
            ("infohash", |record: &mut TransferRecord| {
                record.infohash = "ffffffffffffffffffffffffffffffffffffffff".to_owned()
            }),
            ("selected-file", |record: &mut TransferRecord| {
                record.selected_files[0].file_id = 0
            }),
        ] {
            let encoded = encode_transfer(&record).expect("exact recovery record must encode");
            let mut mismatched =
                parse_transfer_line(&encoded, false).expect("exact recovery record must parse");
            mutate(&mut mismatched);
            fs::write(&exact_path, unchecked_terminal_recovery(&mismatched, 11))
                .expect("mismatched recovery fixture must write");
            assert!(
                parse_terminal_recovery(&persistence_path, &exact_path).is_none(),
                "{case} recovery was accepted"
            );
        }

        let mut corrupt_checksum = exact_bytes;
        let checksum_start = TERMINAL_RECOVERY_HEADER.len() + "11\n".len();
        corrupt_checksum[checksum_start] = if corrupt_checksum[checksum_start] == b'a' {
            b'b'
        } else {
            b'a'
        };
        fs::write(&exact_path, corrupt_checksum).expect("corrupt recovery fixture must write");
        assert!(parse_terminal_recovery(&persistence_path, &exact_path).is_none());

        fs::write(
            &exact_path,
            encoded_terminal_recovery(&record, 11)
                .expect("exact recovery fixture must encode again"),
        )
        .expect("exact recovery must replace corrupt fixture");
        record.terminal_recovery_generation = Some(12);
        assert_eq!(
            remove_terminal_recovery(&persistence_path, &record),
            Err(VR_DOWNLOAD_PERSISTENCE_FAILED)
        );
        assert!(exact_path.is_file());
    }

    #[test]
    fn failed_terminal_dismiss_is_retryable_durable_and_preserves_media() {
        let fixture = FilesystemFixture::new();
        let mut record = completed_selected_boundary_record(&fixture, Some(b"abc"));
        record.state = TransferState::Failed;
        record.downloaded_bytes = record.selected_total();
        record.terminal_recovery_generation = Some(9);
        let transfer_id = record.transfer_id.clone();
        let destination = record.destination.clone();
        let persistence_path = fixture.path.join("downloads");
        let recovery_path = test_terminal_recovery_path(&persistence_path, &record);
        write_persisted_transfers(&persistence_path, &[StoredTransfer::Valid(record)])
            .expect("failed primary authority must persist");
        let persisted = read_persisted_transfers(&persistence_path)
            .expect("failed primary authority must reload");
        let StoredTransfer::Valid(recovery) = &persisted[0] else {
            panic!("failed primary authority must remain valid");
        };
        write_terminal_recovery(&persistence_path, recovery, 9)
            .expect("terminal recovery must persist");
        let recovery_bytes = fs::read(&recovery_path).expect("recovery must remain readable");

        let state = VrDownloadState::default();
        configure_category_folder(&state, TransferCategory::Vr, &destination);
        {
            let mut context = state.0.lock().expect("state must lock");
            context.transfers_loaded = true;
            context.transfers = persisted;
        }
        fs::remove_file(&persistence_path).expect("primary path must be replaceable");
        fs::create_dir(&persistence_path)
            .expect("primary persistence failure must be deterministic");
        assert_eq!(
            dismiss_download(&state, &persistence_path, &transfer_id),
            Err(VR_DOWNLOAD_PERSISTENCE_FAILED)
        );
        assert_eq!(
            fs::read(&recovery_path).expect("failed dismiss must retain recovery"),
            recovery_bytes
        );
        assert_eq!(transfer_rows(&state)[8], "failed");

        let persistence_failed_restart = VrDownloadState::default();
        configure_category_folder(
            &persistence_failed_restart,
            TransferCategory::Vr,
            &destination,
        );
        let rows = tauri::async_runtime::block_on(load_downloads(
            &persistence_failed_restart,
            &persistence_path,
            &fixture.path.join("dismiss-persistence-session"),
            &fixture.path.join("download-limit"),
        ))
        .expect("recovery must remain visible while primary persistence fails");
        assert_eq!(rows[0], transfer_id);
        assert_eq!(rows[8], "failed");
        assert_eq!(rows[13], "true");

        fs::remove_dir(&persistence_path).expect("primary path must become available");
        {
            let context = persistence_failed_restart
                .0
                .lock()
                .expect("state must lock");
            write_persisted_transfers(&persistence_path, &context.transfers)
                .expect("recovered primary authority must persist");
        }
        fs::remove_file(&recovery_path).expect("recovery path must be replaceable");
        fs::create_dir(&recovery_path).expect("recovery cleanup failure must be deterministic");
        assert_eq!(
            dismiss_download(&persistence_failed_restart, &persistence_path, &transfer_id,),
            Err(VR_DOWNLOAD_PERSISTENCE_FAILED)
        );
        assert!(matches!(
            &read_persisted_transfers(&persistence_path)
                .expect("failed cleanup must restore the primary row")[0],
            StoredTransfer::Valid(record) if record.transfer_id == transfer_id
        ));

        fs::remove_dir(&recovery_path).expect("recovery path must become available");
        {
            let context = persistence_failed_restart
                .0
                .lock()
                .expect("state must lock");
            let StoredTransfer::Valid(record) = &context.transfers[0] else {
                panic!("failed terminal row must remain valid");
            };
            write_terminal_recovery(&persistence_path, record, 9)
                .expect("terminal recovery must persist again");
        }
        dismiss_download(&persistence_failed_restart, &persistence_path, &transfer_id)
            .expect("terminal row must dismiss after persistence recovers");
        assert!(!recovery_path.exists());
        assert_eq!(
            fs::read(destination.join("Folder/特別版  B.mp4"))
                .expect("dismiss must preserve selected media"),
            b"1234567"
        );
        assert!(!destination.join("Folder/Part  1 — 映画.mkv").exists());
        let mut context = persistence_failed_restart
            .0
            .lock()
            .expect("state must lock");
        assert!(!finalize_monitored_transfer_with(
            &mut context,
            &transfer_id,
            9,
            true,
            &persistence_path,
            |_, _| panic!("late monitor wrote primary state"),
            |_, _, _| panic!("late monitor wrote recovery state"),
        ));
    }

    #[test]
    fn durable_terminal_recovery_keeps_its_media_owned_for_vr_trash() {
        let fixture = FilesystemFixture::new();
        let mut record = terminal_record_for_category(
            &fixture,
            TransferCategory::Vr,
            "VR — terminal recovery ownership",
        );
        record.state = TransferState::Failed;
        let destination = record.destination.clone();
        let media_path = current_target(&record, 0).expect("owned media path must resolve");
        let persistence_path = fixture.path.join("downloads");
        let recovery_path = test_terminal_recovery_path(&persistence_path, &record);
        write_terminal_recovery(&persistence_path, &record, 4)
            .expect("terminal recovery must persist");
        let recovery_bytes = fs::read(&recovery_path).expect("recovery must remain readable");
        let state = VrDownloadState::default();
        configure_category_folder(&state, TransferCategory::Vr, &destination);
        {
            let mut context = state.0.lock().expect("state must lock");
            context.transfers_loaded = true;
            context.persistence_path = Some(persistence_path.clone());
        }
        let library_state = VrLibraryState::default();
        let rows = scan_vr_library_with(&state, &library_state).expect("VR scan must succeed");
        let generation = rows[0].parse().expect("scan generation must be valid");
        let dispatch_count = Cell::new(0);

        assert_eq!(
            trash_vr_file_with(&media_path, generation, &state, &library_state, |_| {
                dispatch_count.set(dispatch_count.get() + 1);
                Ok(())
            },),
            Err(VR_FILE_TRASH_OWNED)
        );
        assert_eq!(dispatch_count.get(), 0);
        assert_eq!(
            fs::read(&media_path).expect("owned media must remain readable"),
            vec![b'p'; 5]
        );
        assert_eq!(
            fs::read(recovery_path).expect("recovery metadata must remain readable"),
            recovery_bytes
        );
        assert!(state
            .0
            .lock()
            .expect("state must lock")
            .transfers
            .is_empty());
    }

    #[test]
    fn terminal_dismiss_removes_only_the_exact_category_record_and_recovery() {
        let fixture = FilesystemFixture::new();
        let mut vr_record = terminal_record_for_category(
            &fixture,
            TransferCategory::Vr,
            "Shared terminal dismissal — VR",
        );
        let mut adult_record = terminal_record_for_category(
            &fixture,
            TransferCategory::Adult,
            "Shared terminal dismissal — Adult",
        );
        vr_record.state = TransferState::Failed;
        adult_record.state = TransferState::Failed;
        let vr_id = vr_record.transfer_id.clone();
        let adult_id = adult_record.transfer_id.clone();
        let vr_media = current_target(&vr_record, 0).expect("VR media path must resolve");
        let adult_media = current_target(&adult_record, 0).expect("Adult media path must resolve");
        let persistence_path = fixture.path.join("downloads");
        let vr_recovery = test_terminal_recovery_path(&persistence_path, &vr_record);
        let adult_recovery = test_terminal_recovery_path(&persistence_path, &adult_record);
        write_terminal_recovery(&persistence_path, &vr_record, 1)
            .expect("VR recovery must persist");
        write_terminal_recovery(&persistence_path, &adult_record, 2)
            .expect("Adult recovery must persist");
        let adult_recovery_bytes =
            fs::read(&adult_recovery).expect("Adult recovery must remain readable");
        write_persisted_transfers(
            &persistence_path,
            &[
                StoredTransfer::Valid(vr_record),
                StoredTransfer::Valid(adult_record),
            ],
        )
        .expect("terminal category records must persist");
        let state = VrDownloadState::default();
        {
            let mut context = state.0.lock().expect("state must lock");
            context.transfers_loaded = true;
            context.transfers = read_persisted_transfers(&persistence_path)
                .expect("terminal category records must reload");
        }

        dismiss_download(&state, &persistence_path, &vr_id)
            .expect("exact VR terminal record must dismiss");
        assert!(!vr_recovery.exists());
        assert_eq!(
            fs::read(&adult_recovery).expect("Adult recovery must remain readable"),
            adult_recovery_bytes
        );
        assert!(matches!(
            state.0.lock().expect("state must lock").transfers.as_slice(),
            [StoredTransfer::Valid(record)] if record.transfer_id == adult_id
                && record.category == TransferCategory::Adult
        ));
        assert_eq!(
            fs::read(vr_media).expect("VR media must remain readable"),
            vec![b'p'; 5]
        );
        assert_eq!(
            fs::read(adult_media).expect("Adult media must remain readable"),
            vec![b'p'; 5]
        );
    }

    fn shared_category_source(category: TransferCategory) -> VerifiedDownloadSource {
        match category {
            TransferCategory::Adult => persistable_adult_fixture_source(),
            TransferCategory::Movie => movie_organization_source(
                "Exact Movie",
                Some("1999-04-19"),
                "Exact Provider Movie",
                &[("Provider/Feature.mp4", 5)],
                &[0],
            ),
            TransferCategory::Tv => tv_download_source(&[("Show.S02E03.mkv", 5)], &[0]),
            TransferCategory::Vr => panic!("shared-category coverage requires a non-VR record"),
        }
    }

    fn assert_shared_category_vr_trash_ownership(category: TransferCategory) {
        let fixture = FilesystemFixture::new();
        let shared_destination = fixture
            .path
            .join(format!("VR + {} — transfer", category.as_str()));
        fs::create_dir_all(&shared_destination).expect("shared destination must exist");
        let shared_destination =
            fs::canonicalize(shared_destination).expect("shared destination must canonicalize");
        let record = completed_organization_record_for_category(
            category,
            &shared_destination,
            shared_category_source(category),
        );
        let selected_path = shared_destination.join(&record.current_paths[0]);
        let selected_bytes = fs::read(&selected_path).expect("selected media must be readable");
        let (state, transfer_id) = organization_state(record);
        let persistence_path = fixture
            .path
            .join(format!("{}-downloads", category.as_str()));
        {
            let mut context = state.0.lock().expect("state must lock");
            context.future_folder = Some(shared_destination.clone());
            context.persistence_path = Some(persistence_path.clone());
            assert_eq!(
                configured_folder(&context, category),
                Some(shared_destination.as_path())
            );
        }
        let library_state = VrLibraryState::default();
        let scan = scan_vr_library_with(&state, &library_state)
            .expect("shared transfer scan must succeed");
        let generation = scan[0]
            .parse()
            .expect("shared transfer generation must be valid");
        let dispatch_count = Cell::new(0);
        let transfer_snapshot = transfer_snapshots(&state);
        let transfer_row_snapshot = transfer_rows(&state);
        assert_eq!(
            trash_vr_file_with(&selected_path, generation, &state, &library_state, |_| {
                dispatch_count.set(dispatch_count.get() + 1);
                Ok(())
            },),
            Err(VR_FILE_TRASH_OWNED)
        );
        assert_eq!(
            fs::read(&selected_path).expect("selected media must remain readable"),
            selected_bytes
        );
        assert_eq!(transfer_snapshots(&state), transfer_snapshot);
        assert_eq!(transfer_rows(&state), transfer_row_snapshot);
        assert!(state
            .0
            .lock()
            .expect("state must lock")
            .organization_plan
            .is_none());

        let preview =
            preview_organization(&state, &transfer_id).expect("organization must preview");
        let planned_path = shared_destination.join(&preview[7]);
        fs::create_dir_all(
            planned_path
                .parent()
                .expect("planned path must have a parent"),
        )
        .expect("planned parent must exist");
        fs::write(&planned_path, b"reserved").expect("planned target fixture must exist");
        let plan_scan =
            scan_vr_library_with(&state, &library_state).expect("planned path scan must succeed");
        let plan_generation = plan_scan[0]
            .parse()
            .expect("planned path generation must be valid");
        let plan_snapshot = {
            let context = state.0.lock().expect("state must lock");
            organization_plan_response(
                context
                    .organization_plan
                    .as_ref()
                    .expect("organization plan must remain current"),
            )
        };
        let planned_transfer_snapshot = transfer_snapshots(&state);
        let planned_row_snapshot = transfer_rows(&state);
        assert_eq!(
            trash_vr_file_with(
                &planned_path,
                plan_generation,
                &state,
                &library_state,
                |_| {
                    dispatch_count.set(dispatch_count.get() + 1);
                    Ok(())
                },
            ),
            Err(VR_FILE_TRASH_OWNED)
        );
        assert_eq!(
            fs::read(&planned_path).expect("planned media must remain readable"),
            b"reserved"
        );
        assert_eq!(transfer_snapshots(&state), planned_transfer_snapshot);
        assert_eq!(transfer_rows(&state), planned_row_snapshot);
        assert_eq!(
            organization_plan_response(
                state
                    .0
                    .lock()
                    .expect("state must lock")
                    .organization_plan
                    .as_ref()
                    .expect("organization plan must remain current"),
            ),
            plan_snapshot
        );

        fs::remove_file(&planned_path).expect("planned fixture must be removed before Apply");
        apply_organization(&state, &persistence_path, &preview[0])
            .expect("organization must apply");
        let organized_snapshot = transfer_snapshots(&state);
        let organized_row_snapshot = transfer_rows(&state);
        let organized_scan =
            scan_vr_library_with(&state, &library_state).expect("organized path scan must succeed");
        let organized_generation = organized_scan[0]
            .parse()
            .expect("organized path generation must be valid");
        assert_eq!(
            trash_vr_file_with(
                &planned_path,
                organized_generation,
                &state,
                &library_state,
                |_| {
                    dispatch_count.set(dispatch_count.get() + 1);
                    Ok(())
                },
            ),
            Err(VR_FILE_TRASH_OWNED)
        );
        assert_eq!(
            fs::read(&planned_path).expect("organized media must remain readable"),
            selected_bytes
        );
        assert_eq!(transfer_snapshots(&state), organized_snapshot);
        assert_eq!(transfer_rows(&state), organized_row_snapshot);

        let recovery_destination = fixture
            .path
            .join(format!("VR + {} — recovery", category.as_str()));
        let holding = fixture
            .path
            .join(format!("{} recovery Trash fixture", category.as_str()));
        fs::create_dir_all(&recovery_destination).expect("recovery destination must exist");
        fs::create_dir_all(&holding).expect("recovery Trash fixture must exist");
        let recovery_destination =
            fs::canonicalize(recovery_destination).expect("recovery destination must canonicalize");
        let mut recovery_record = completed_organization_record_for_category(
            category,
            &recovery_destination,
            shared_category_source(category),
        );
        recovery_record.organization_state = OrganizationState::Attention;
        let recovery_transfer_id = recovery_record.transfer_id.clone();
        let recovered_media = recovery_destination.join(&recovery_record.current_paths[0]);
        let recovered_bytes = fs::read(&recovered_media).expect("recovered media must be readable");
        let recovery_path = organization_recovery_path(&recovery_record);
        write_organization_recovery(&recovery_record, &recovery_record.current_paths, None)
            .expect("shared-category recovery must persist");
        let recovery_bytes =
            fs::read(&recovery_path).expect("shared-category recovery must be readable");
        let recovery_state = VrDownloadState::default();
        let dismissal_persistence = fixture
            .path
            .join(format!("{}-recovery-downloads", category.as_str()));
        {
            let mut context = recovery_state.0.lock().expect("recovery state must lock");
            context.persistence_path = Some(dismissal_persistence.clone());
            context.future_folder = Some(recovery_destination.clone());
            match category {
                TransferCategory::Adult => {
                    context.adult_future_folder = Some(recovery_destination.clone())
                }
                TransferCategory::Movie => {
                    context.movie_future_folder = Some(recovery_destination.clone())
                }
                TransferCategory::Tv => {
                    context.tv_future_folder = Some(recovery_destination.clone())
                }
                TransferCategory::Vr => unreachable!(),
            }
            context.transfers_loaded = true;
        }
        let recovery_library_state = VrLibraryState::default();
        let recovery_scan = scan_vr_library_with(&recovery_state, &recovery_library_state)
            .expect("shared-category recovery scan must succeed");
        let recovery_generation = recovery_scan[0]
            .parse()
            .expect("shared-category recovery generation must be valid");
        assert_eq!(
            trash_vr_file_with(
                &recovered_media,
                recovery_generation,
                &recovery_state,
                &recovery_library_state,
                |_| {
                    dispatch_count.set(dispatch_count.get() + 1);
                    Ok(())
                },
            ),
            Err(VR_FILE_TRASH_OWNED)
        );
        assert_eq!(
            fs::read(&recovered_media).expect("recovery-owned media must remain readable"),
            recovered_bytes
        );
        assert_eq!(
            fs::read(&recovery_path).expect("recovery metadata must remain readable"),
            recovery_bytes
        );
        {
            let context = recovery_state.0.lock().expect("recovery state must lock");
            assert!(context.transfers.is_empty());
            assert!(context.organization_plan.is_none());
        }
        assert_eq!(dispatch_count.get(), 0);

        recovery_state
            .0
            .lock()
            .expect("recovery state must lock")
            .transfers
            .push(StoredTransfer::Valid(recovery_record));
        dismiss_download(
            &recovery_state,
            &dismissal_persistence,
            &recovery_transfer_id,
        )
        .expect("shared-category recovery row must dismiss durably");
        assert!(read_persisted_transfers(&dismissal_persistence)
            .expect("dismissed persistence must remain valid")
            .is_empty());
        assert!(!recovery_path.exists());
        assert_eq!(
            fs::read(&recovered_media).expect("dismissal must retain recovery-owned media"),
            recovered_bytes
        );

        let fresh_scan = scan_vr_library_with(&recovery_state, &recovery_library_state)
            .expect("fresh post-dismissal scan must succeed");
        let fresh_generation = fresh_scan[0]
            .parse()
            .expect("fresh post-dismissal generation must be valid");
        let moved_media = holding.join(
            recovered_media
                .file_name()
                .expect("recovered media must have a basename"),
        );
        trash_vr_file_with(
            &recovered_media,
            fresh_generation,
            &recovery_state,
            &recovery_library_state,
            |path| {
                assert!(matches!(
                    recovery_state.0.try_lock(),
                    Err(TryLockError::WouldBlock)
                ));
                fs::rename(path, &moved_media).map_err(|_| ())
            },
        )
        .expect("fresh scan must authorize shared-category post-dismissal Trash");
        assert_eq!(
            fs::read(moved_media).expect("moved media must remain readable"),
            recovered_bytes
        );
    }

    #[test]
    fn download_rows_mark_only_the_current_configured_destination() {
        let fixture = FilesystemFixture::new();
        let destination = fixture.path.join("current");
        let record = transfer_from_source(
            TransferCategory::Vr,
            fixture_source(),
            destination.clone(),
            TransferState::Cancelled,
        );
        let mut context = VrDownloadContext {
            future_folder: Some(destination),
            transfers: vec![StoredTransfer::Valid(record)],
            ..VrDownloadContext::default()
        };

        assert_eq!(download_rows(&mut context)[9], "true");
        context.future_folder = Some(fixture.path.join("replacement"));
        assert_eq!(download_rows(&mut context)[9], "false");
    }

    #[test]
    fn previews_applies_persists_and_dismisses_single_file_organization() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let record = completed_organization_record(&destination, persistable_fixture_source());
        let expected_identity = (
            record.transfer_id.clone(),
            record.code.clone(),
            record.release_name.clone(),
            record.infohash.clone(),
            record.metainfo.clone(),
            record.selected_file_ids(),
            record.destination.clone(),
        );
        let (state, transfer_id) = organization_state(record);
        let persistence_path = fixture.path.join("downloads");

        let preview = preview_organization(&state, &transfer_id).expect("preview must succeed");
        assert_eq!(&preview[1..5], &[&transfer_id, "MDVR-419", "1", "1"]);
        assert_eq!(
            &preview[5..],
            &["move", "Movie  A.mp4", "MDVR-419/MDVR-419.mp4"]
        );
        apply_organization(&state, &persistence_path, &preview[0])
            .expect("organization must succeed");
        assert!(!destination.join("Movie  A.mp4").exists());
        let organized_file = destination.join("MDVR-419/MDVR-419.mp4");
        assert_eq!(
            fs::read(&organized_file).expect("organized file must remain readable"),
            vec![b'a'; 5]
        );

        let context = state.0.lock().expect("state must lock");
        let StoredTransfer::Valid(record) = &context.transfers[0] else {
            panic!("transfer must remain valid");
        };
        assert_eq!(
            (
                record.transfer_id.clone(),
                record.code.clone(),
                record.release_name.clone(),
                record.infohash.clone(),
                record.metainfo.clone(),
                record.selected_file_ids(),
                record.destination.clone(),
            ),
            expected_identity
        );
        assert_eq!(record.state, TransferState::Completed);
        assert_eq!(record.organization_state, OrganizationState::Organized);
        assert_eq!(record.current_paths, ["MDVR-419/MDVR-419.mp4"]);
        assert!(record.handle.is_none());
        drop(context);

        let restarted = VrDownloadState::default();
        restarted.0.lock().expect("state must lock").future_folder = Some(destination.clone());
        let rows = tauri::async_runtime::block_on(load_downloads(
            &restarted,
            &persistence_path,
            &fixture.path.join("session"),
            &fixture.path.join("limit"),
        ))
        .expect("organized transfer must reload");
        assert_eq!(
            &rows[8..13],
            &["completed", "true", "organized", "MDVR-419/", "false"]
        );
        assert!(
            restarted
                .0
                .lock()
                .expect("state must lock")
                .session
                .is_none(),
            "organized completion restarted a native session"
        );
        dismiss_download(&restarted, &persistence_path, &transfer_id)
            .expect("organized row must dismiss");
        for restart_index in 0..2 {
            let dismissed_restart = VrDownloadState::default();
            dismissed_restart
                .0
                .lock()
                .expect("state must lock")
                .future_folder = Some(destination.clone());
            let rows = tauri::async_runtime::block_on(load_downloads(
                &dismissed_restart,
                &persistence_path,
                &fixture
                    .path
                    .join(format!("dismissed-organized-session-{restart_index}")),
                &fixture.path.join("limit"),
            ))
            .expect("dismissed organized transfer must remain absent");
            assert!(rows.is_empty(), "dismissed organized row was recreated");
            assert_eq!(
                fs::read(&organized_file).expect("organized media must remain at its exact path"),
                vec![b'a'; 5]
            );
        }
    }

    #[test]
    fn adult_preview_applies_and_reloads_the_exact_canonical_single_file() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let record =
            completed_adult_organization_record(&destination, persistable_adult_fixture_source());
        let release_name = record.release_name.clone();
        let (state, transfer_id) = organization_state(record);
        let persistence_path = fixture.path.join("downloads");

        let preview = preview_organization(&state, &transfer_id).expect("preview must succeed");
        assert_eq!(&preview[1..5], &[&transfer_id, "ADLT-123", "1", "1"]);
        assert_eq!(
            &preview[5..],
            &["move", "Movie  A.mp4", "ADLT-123/ADLT-123.mp4"]
        );
        apply_organization(&state, &persistence_path, &preview[0])
            .expect("Adult organization must succeed");
        let organized_file = destination.join("ADLT-123/ADLT-123.mp4");
        assert_eq!(
            fs::read(&organized_file).expect("organized file must remain readable"),
            vec![b'a'; 5]
        );

        let restarted = VrDownloadState::default();
        configure_adult_download_folder(&restarted, Some(destination.clone()))
            .expect("Adult folder must restore");
        let rows = tauri::async_runtime::block_on(load_downloads(
            &restarted,
            &persistence_path,
            &fixture.path.join("adult-session"),
            &fixture.path.join("limit"),
        ))
        .expect("organized Adult transfer must reload");
        assert_eq!(rows[1], "adult");
        assert_eq!(rows[3], release_name);
        assert_eq!(
            &rows[8..13],
            &["completed", "true", "organized", "ADLT-123/", "false"]
        );
        assert!(
            restarted
                .0
                .lock()
                .expect("state must lock")
                .session
                .is_none(),
            "organized Adult completion restarted a native session"
        );
    }

    #[test]
    fn movie_preview_applies_reloads_and_dismisses_the_exact_tmdb_single_file() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let source = movie_organization_source(
            "Exact  Movie — 特別版",
            Some("1999-04-19"),
            "Different YTS Provider Title",
            &[("Provider/Unrelated  Name.MP4", 5)],
            &[0],
        );
        let record = completed_movie_organization_record(&destination, source);
        let expected_identity = record.movie_identity.clone();
        let (state, transfer_id) = organization_state(record);
        let persistence_path = fixture.path.join("downloads");

        let preview = preview_organization(&state, &transfer_id).expect("preview must succeed");
        assert_eq!(&preview[1..5], &[&transfer_id, "tt0123456", "1", "1"]);
        assert_eq!(
            &preview[5..],
            &[
                "move",
                "Provider/Unrelated  Name.MP4",
                "Exact  Movie — 特別版 (1999)/Exact  Movie — 特別版 (1999).MP4",
            ]
        );
        apply_organization(&state, &persistence_path, &preview[0])
            .expect("Movie organization must succeed");
        let organized_file =
            destination.join("Exact  Movie — 特別版 (1999)/Exact  Movie — 特別版 (1999).MP4");
        assert_eq!(
            fs::read(&organized_file).expect("organized Movie must remain readable"),
            vec![b'a'; 5]
        );

        let restarted = VrDownloadState::default();
        configure_movie_download_folder(&restarted, Some(destination.clone()))
            .expect("Movies folder must restore");
        let rows = tauri::async_runtime::block_on(load_downloads(
            &restarted,
            &persistence_path,
            &fixture.path.join("movie-organized-session"),
            &fixture.path.join("limit"),
        ))
        .expect("organized Movie transfer must reload");
        assert_eq!(rows[1], "movie");
        assert_eq!(rows[2], "tt0123456");
        assert_eq!(rows[3], "Exact  Movie — 特別版");
        assert_eq!(
            &rows[8..13],
            &[
                "completed",
                "true",
                "organized",
                "Exact  Movie — 特別版 (1999)/",
                "false",
            ]
        );
        let context = restarted.0.lock().expect("state must lock");
        let StoredTransfer::Valid(record) = &context.transfers[0] else {
            panic!("organized Movie transfer must remain valid");
        };
        assert_eq!(record.movie_identity, expected_identity);
        assert!(
            context.session.is_none(),
            "organized Movie restarted a session"
        );
        drop(context);

        dismiss_download(&restarted, &persistence_path, &transfer_id)
            .expect("organized Movie row must dismiss");
        for restart_index in 0..2 {
            let dismissed = VrDownloadState::default();
            configure_movie_download_folder(&dismissed, Some(destination.clone()))
                .expect("Movies folder must restore after dismissal");
            let rows = tauri::async_runtime::block_on(load_downloads(
                &dismissed,
                &persistence_path,
                &fixture
                    .path
                    .join(format!("movie-dismissed-session-{restart_index}")),
                &fixture.path.join("limit"),
            ))
            .expect("dismissed Movie transfer must remain absent");
            assert!(rows.is_empty());
            assert_eq!(
                fs::read(&organized_file).expect("dismissal must retain organized Movie media"),
                vec![b'a'; 5]
            );
        }
    }

    #[test]
    fn movie_multi_file_organization_preserves_exact_basenames_and_non_media_paths() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let source = movie_organization_source(
            "Exact  Movie — 特別版",
            Some("1999-04-19"),
            "YTS title must not name files",
            &[
                ("Provider/Feature  Cut.mp4", 3),
                ("Provider/Second — 特別.MKV", 4),
                ("Provider/notes  exact.txt", 5),
            ],
            &[0, 1, 2],
        );
        let record = completed_movie_organization_record(&destination, source);
        let (state, transfer_id) = organization_state(record);

        let preview = preview_organization(&state, &transfer_id).expect("preview must succeed");
        assert_eq!(&preview[3..5], &["2", "3"]);
        assert_eq!(
            &preview[5..],
            &[
                "move",
                "Provider/Feature  Cut.mp4",
                "Exact  Movie — 特別版 (1999)/Feature  Cut.mp4",
                "move",
                "Provider/Second — 特別.MKV",
                "Exact  Movie — 特別版 (1999)/Second — 特別.MKV",
                "non-media-unchanged",
                "Provider/notes  exact.txt",
                "",
            ]
        );
        apply_organization(&state, &fixture.path.join("downloads"), &preview[0])
            .expect("Movie multi-file organization must succeed");
        for path in [
            "Exact  Movie — 特別版 (1999)/Feature  Cut.mp4",
            "Exact  Movie — 特別版 (1999)/Second — 特別.MKV",
            "Provider/notes  exact.txt",
        ] {
            assert!(destination.join(path).is_file(), "missing {path:?}");
        }
        assert!(!destination
            .join("Exact  Movie — 特別版 (1999)/Exact  Movie — 特別版 (1999).mp4")
            .exists());
    }

    #[test]
    fn tv_preview_applies_reloads_and_dismisses_the_exact_episode_single_file() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let source = tv_download_source(
            &[
                ("Provider/Unrelated  Name.MP4", 5),
                ("Provider/notes  exact.txt", 4),
            ],
            &[0, 1],
        );
        let record = completed_tv_organization_record(&destination, source);
        let expected_identity = record.tv_identity.clone();
        let expected_boundary = encoded_boundary_segments(&record).expect("boundary must encode");
        let (state, transfer_id) = organization_state(record);
        let persistence_path = fixture.path.join("downloads");

        let preview = preview_organization(&state, &transfer_id).expect("preview must succeed");
        assert_eq!(
            &preview[1..5],
            &[&transfer_id, "tt0123456 · S02E03", "1", "2"]
        );
        assert_eq!(
            &preview[5..],
            &[
                "move",
                "Provider/Unrelated  Name.MP4",
                "Exact  Show — 特別版/Season 02/Exact  Show — 特別版 - S02E03 - 第三話  —  Exact Episode.MP4",
                "non-media-unchanged",
                "Provider/notes  exact.txt",
                "",
            ]
        );
        assert!(destination.join("Provider/Unrelated  Name.MP4").is_file());
        assert_eq!(
            fs::read(destination.join("Provider/notes  exact.txt"))
                .expect("selected non-media file must exist"),
            vec![b'b'; 4]
        );
        assert!(!organization_recovery_path(
            match &state.0.lock().expect("state must lock").transfers[0] {
                StoredTransfer::Valid(record) => record,
                StoredTransfer::Corrupt(_) => panic!("TV transfer must remain valid"),
            }
        )
        .exists());

        apply_organization(&state, &persistence_path, &preview[0])
            .expect("TV organization must succeed");
        let organized_file = destination.join(
            "Exact  Show — 特別版/Season 02/Exact  Show — 特別版 - S02E03 - 第三話  —  Exact Episode.MP4",
        );
        assert_eq!(
            fs::read(&organized_file).expect("organized TV media must remain readable"),
            vec![b'a'; 5]
        );
        assert_eq!(
            fs::read(destination.join("Provider/notes  exact.txt"))
                .expect("organization must leave selected non-media at its exact path"),
            vec![b'b'; 4]
        );

        let restarted = VrDownloadState::default();
        configure_tv_download_folder(&restarted, Some(destination.clone()))
            .expect("TV folder must restore");
        let rows = tauri::async_runtime::block_on(load_downloads(
            &restarted,
            &persistence_path,
            &fixture.path.join("tv-organized-session"),
            &fixture.path.join("limit"),
        ))
        .expect("organized TV transfer must reload");
        assert_eq!(rows[1], "tv");
        assert_eq!(rows[2], "tt0123456 · S02E03");
        assert_eq!(
            &rows[8..13],
            &[
                "completed",
                "true",
                "organized",
                "Exact  Show — 特別版/Season 02/",
                "false",
            ]
        );
        let context = restarted.0.lock().expect("state must lock");
        let StoredTransfer::Valid(record) = &context.transfers[0] else {
            panic!("organized TV transfer must remain valid");
        };
        assert_eq!(record.tv_identity, expected_identity);
        assert_eq!(
            encoded_boundary_segments(record).expect("restarted boundary must encode"),
            expected_boundary
        );
        assert!(
            context.session.is_none(),
            "organized TV restarted a session"
        );
        drop(context);

        dismiss_download(&restarted, &persistence_path, &transfer_id)
            .expect("organized TV row must dismiss");
        let dismissed = VrDownloadState::default();
        configure_tv_download_folder(&dismissed, Some(destination))
            .expect("TV folder must restore after dismissal");
        let rows = tauri::async_runtime::block_on(load_downloads(
            &dismissed,
            &persistence_path,
            &fixture.path.join("tv-dismissed-session"),
            &fixture.path.join("limit"),
        ))
        .expect("dismissed TV transfer must remain absent");
        assert!(rows.is_empty());
        assert_eq!(
            fs::read(organized_file).expect("dismissal must retain organized TV media"),
            vec![b'a'; 5]
        );
        assert_eq!(
            fs::read(fixture.path.join("Provider/notes  exact.txt"),)
                .expect("dismissal must retain selected non-media"),
            vec![b'b'; 4]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn tv_organization_discards_a_nonportable_source_basename_and_regroups_the_exact_episode() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let source = tv_download_source(
            &[("Provider/CON.MP4", 5), ("Provider/notes exact.txt", 4)],
            &[0, 1],
        );
        let record = completed_tv_organization_record(&destination, source);
        let (state, transfer_id) = organization_state(record);
        let persistence_path = fixture.path.join("downloads");
        let target_relative = "Exact  Show — 特別版/Season 02/Exact  Show — 特別版 - S02E03 - 第三話  —  Exact Episode.MP4";

        let preview = preview_organization(&state, &transfer_id)
            .expect("discarded source basename must not prevent an exact TV plan");
        assert_eq!(
            &preview[5..],
            &[
                "move",
                "Provider/CON.MP4",
                target_relative,
                "non-media-unchanged",
                "Provider/notes exact.txt",
                "",
            ]
        );

        apply_organization(&state, &persistence_path, &preview[0])
            .expect("exact TV plan must apply");
        let organized_file = destination.join(target_relative);
        assert_eq!(
            fs::read(&organized_file).expect("organized TV media must remain readable"),
            vec![b'a'; 5]
        );
        assert!(!destination.join("Provider/CON.MP4").exists());
        assert_eq!(
            fs::read(destination.join("Provider/notes exact.txt"))
                .expect("selected non-media must remain at its exact path"),
            vec![b'b'; 4]
        );

        let library_state = TvLibraryState::default();
        set_tv_folder(&library_state, &fixture.path.join("tv-folder"), destination)
            .expect("TV Library folder must configure");
        let scan = scan_tv_library_with(&library_state).expect("TV Library scan must succeed");
        assert_eq!(scan.len(), 7);
        assert_eq!(Path::new(&scan[1]), organized_file);
        assert_eq!(
            &scan[2..],
            &[target_relative, "5", "Exact  Show — 特別版", "2", "3",]
        );
    }

    #[test]
    fn tv_multi_media_preserves_exact_basenames_and_regroups_every_episode_file() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let source = tv_download_source(
            &[
                ("Provider/Exact  Show — 特別版.S02E03.Cut  A — 特別.MP4", 3),
                ("Provider/S02E03 — Cut  B.MkV", 4),
                ("Exact  Show — 特別版/Season 02/S02E03 — Existing.mkv", 5),
                ("Provider/notes  exact.txt", 6),
            ],
            &[0, 1, 2, 3],
        );
        let record = completed_tv_organization_record(&destination, source);
        let expected_boundary = encoded_boundary_segments(&record).expect("boundary must encode");
        let (state, transfer_id) = organization_state(record);
        let persistence_path = fixture.path.join("downloads");
        assert_eq!(transfer_rows(&state)[12], "true");

        let preview = preview_organization(&state, &transfer_id)
            .expect("every exact multi-media member must preview");
        assert_eq!(
            &preview[1..5],
            &[&transfer_id, "tt0123456 · S02E03", "2", "4"]
        );
        assert_eq!(
            &preview[5..],
            &[
                "move",
                "Provider/Exact  Show — 特別版.S02E03.Cut  A — 特別.MP4",
                "Exact  Show — 特別版/Season 02/Exact  Show — 特別版.S02E03.Cut  A — 特別.MP4",
                "move",
                "Provider/S02E03 — Cut  B.MkV",
                "Exact  Show — 特別版/Season 02/S02E03 — Cut  B.MkV",
                "media-unchanged",
                "Exact  Show — 特別版/Season 02/S02E03 — Existing.mkv",
                "Exact  Show — 特別版/Season 02/S02E03 — Existing.mkv",
                "non-media-unchanged",
                "Provider/notes  exact.txt",
                "",
            ]
        );

        apply_organization(&state, &persistence_path, &preview[0])
            .expect("complete multi-media TV plan must apply");
        for (path, bytes) in [
            (
                "Exact  Show — 特別版/Season 02/Exact  Show — 特別版.S02E03.Cut  A — 特別.MP4",
                vec![b'a'; 3],
            ),
            (
                "Exact  Show — 特別版/Season 02/S02E03 — Cut  B.MkV",
                vec![b'b'; 4],
            ),
            (
                "Exact  Show — 特別版/Season 02/S02E03 — Existing.mkv",
                vec![b'c'; 5],
            ),
            ("Provider/notes  exact.txt", vec![b'd'; 6]),
        ] {
            assert_eq!(
                fs::read(destination.join(path)).expect("organized member must remain exact"),
                bytes
            );
        }
        assert!(!destination
            .join("Provider/Exact  Show — 特別版.S02E03.Cut  A — 特別.MP4")
            .exists());
        assert!(!destination.join("Provider/S02E03 — Cut  B.MkV").exists());

        let library_state = TvLibraryState::default();
        set_tv_folder(
            &library_state,
            &fixture.path.join("tv-multi-folder"),
            destination.clone(),
        )
        .expect("TV Library folder must configure");
        let scan = scan_tv_library_with(&library_state).expect("TV Library scan must succeed");
        assert_eq!(scan.len(), 19);
        for (relative_path, size) in [
            (
                "Exact  Show — 特別版/Season 02/Exact  Show — 特別版.S02E03.Cut  A — 特別.MP4",
                "3",
            ),
            ("Exact  Show — 特別版/Season 02/S02E03 — Cut  B.MkV", "4"),
            ("Exact  Show — 特別版/Season 02/S02E03 — Existing.mkv", "5"),
        ] {
            let expected_relative_path = relative_path.split('/').collect::<PathBuf>();
            let fields = scan[1..]
                .chunks_exact(6)
                .find(|fields| Path::new(&fields[1]) == expected_relative_path)
                .expect("organized TV member must be scanned");
            assert_eq!(fields[2], size);
            assert_eq!(fields[3], "Exact  Show — 特別版");
            assert_eq!(fields[4], "2");
            assert_eq!(fields[5], "3");
        }
        let context = state.0.lock().expect("state must lock");
        let StoredTransfer::Valid(record) = &context.transfers[0] else {
            panic!("organized TV transfer must remain valid");
        };
        assert_eq!(record.organization_state, OrganizationState::Organized);
        assert_eq!(
            encoded_boundary_segments(record).expect("organized boundary must encode"),
            expected_boundary
        );
    }

    #[test]
    fn tv_multi_media_rejects_the_complete_plan_when_any_member_cannot_round_trip() {
        for invalid_path in [
            "Provider/No episode.MKV",
            "Provider/Other Show.S02E03.MKV",
            "Provider/S03E03.MKV",
            "Provider/S02E04.MKV",
            "Provider/S02E03.S02E03.MKV",
            "Provider/S02E03-x04.MKV",
            "Provider/Exact ShowS02E03.MKV",
            "Provider/S02E003.MKV",
        ] {
            let fixture = FilesystemFixture::new();
            let destination =
                fs::canonicalize(&fixture.path).expect("destination must canonicalize");
            let source = tv_download_source(
                &[
                    ("Provider/S02E03 — Valid Cut.MP4", 3),
                    (invalid_path, 4),
                    ("Provider/notes exact.txt", 5),
                ],
                &[0, 1, 2],
            );
            let record = completed_tv_organization_record(&destination, source);
            let recovery_path = organization_recovery_path(&record);
            let recovery_successor_path = organization_recovery_successor_path(&record);
            let (state, transfer_id) = organization_state(record);
            let before = transfer_snapshots(&state);
            let persistence_path = fixture.path.join("downloads");
            assert_eq!(
                transfer_rows(&state)[12],
                "false",
                "invalid member {invalid_path:?} remained organizable"
            );

            assert_eq!(
                preview_organization(&state, &transfer_id),
                Err(VR_ORGANIZATION_INELIGIBLE),
                "invalid member {invalid_path:?} exposed a partial plan"
            );
            assert!(
                state
                    .0
                    .lock()
                    .expect("state must lock")
                    .organization_plan
                    .is_none(),
                "invalid member {invalid_path:?} retained a native plan"
            );
            let mut move_calls = 0;
            assert_eq!(
                apply_organization_with(
                    &state,
                    &persistence_path,
                    "fabricated-plan",
                    |source, destination| {
                        move_calls += 1;
                        fs::rename(source, destination)
                    },
                ),
                Err(VR_ORGANIZATION_STALE)
            );
            assert_eq!(move_calls, 0);
            for (path, bytes) in [
                ("Provider/S02E03 — Valid Cut.MP4", vec![b'a'; 3]),
                (invalid_path, vec![b'b'; 4]),
                ("Provider/notes exact.txt", vec![b'c'; 5]),
            ] {
                assert_eq!(
                    fs::read(destination.join(path))
                        .expect("rejected plan must retain every selected file"),
                    bytes
                );
            }
            assert!(!destination.join("Exact  Show — 特別版").exists());
            assert_eq!(transfer_snapshots(&state), before);
            assert!(!persistence_path.exists());
            assert!(!recovery_path.exists());
            assert!(!recovery_successor_path.exists());
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn tv_multi_media_rejects_a_nonportable_retained_source_basename() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let source = tv_download_source(
            &[
                ("Provider/S02E03 — Valid Cut.MP4", 3),
                ("Provider/S02E03?.MKV", 4),
            ],
            &[0, 1],
        );
        let record = completed_tv_organization_record(&destination, source);
        let (state, transfer_id) = organization_state(record);

        assert_eq!(
            preview_organization(&state, &transfer_id),
            Err(VR_ORGANIZATION_INELIGIBLE)
        );
        assert_eq!(
            fs::read(destination.join("Provider/S02E03?.MKV"))
                .expect("unsafe retained basename must remain at its source"),
            vec![b'b'; 4]
        );
        assert!(!destination.join("Exact  Show — 特別版").exists());
    }

    #[test]
    fn tv_multi_media_rejects_unsafe_generated_components_without_using_the_episode_name() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let mut source = tv_download_source(
            &[
                ("Provider/S02E03 — Cut A.MP4", 3),
                ("Provider/S02E03 — Cut B.MKV", 4),
            ],
            &[0, 1],
        );
        source
            .tv_identity
            .as_mut()
            .expect("TV identity must exist")
            .episode_name = "Episode?".to_owned();
        let record = completed_tv_organization_record(&destination, source);
        let (state, transfer_id) = organization_state(record);

        preview_organization(&state, &transfer_id)
            .expect("an unused episode name must not alter retained-basename eligibility");

        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let mut source = tv_download_source(
            &[
                ("Provider/S02E03 — Cut A.MP4", 3),
                ("Provider/S02E03 — Cut B.MKV", 4),
            ],
            &[0, 1],
        );
        source
            .tv_identity
            .as_mut()
            .expect("TV identity must exist")
            .show_name = "Unsafe? Show".to_owned();
        let record = completed_tv_organization_record(&destination, source);
        let (state, transfer_id) = organization_state(record);

        assert_eq!(
            preview_organization(&state, &transfer_id),
            Err(VR_ORGANIZATION_INELIGIBLE)
        );
        assert!(!destination.join("Unsafe? Show").exists());
    }

    #[test]
    fn tv_multi_media_rejects_duplicate_normalized_and_existing_retained_targets() {
        for (first, second) in [
            ("One/S02E03 — Same.MP4", "Two/S02E03 — Same.MP4"),
            ("One/S02E03 — Case.MP4", "Two/s02e03 — case.mp4"),
            ("One/S02E03 — Cut Ḋ.MP4", "Two/S02E03 — Cut D\u{307}.MP4"),
        ] {
            let fixture = FilesystemFixture::new();
            let destination =
                fs::canonicalize(&fixture.path).expect("destination must canonicalize");
            let source = tv_download_source(&[(first, 3), (second, 4)], &[0, 1]);
            let record = completed_tv_organization_record(&destination, source);
            let (state, transfer_id) = organization_state(record);

            assert_eq!(
                preview_organization(&state, &transfer_id),
                Err(VR_ORGANIZATION_CONFLICT),
                "conflicting retained basenames {first:?} / {second:?} were eligible"
            );
            assert_eq!(
                fs::read(destination.join(first)).expect("first source must remain"),
                vec![b'a'; 3]
            );
            assert_eq!(
                fs::read(destination.join(second)).expect("second source must remain"),
                vec![b'b'; 4]
            );
            assert!(!destination.join("Exact  Show — 特別版").exists());
        }

        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let source = tv_download_source(
            &[
                ("Provider/S02E03 — Existing.MP4", 3),
                ("Provider/S02E03 — Other.MKV", 4),
            ],
            &[0, 1],
        );
        let record = completed_tv_organization_record(&destination, source);
        let season_directory = destination.join("Exact  Show — 特別版/Season 02");
        fs::create_dir_all(&season_directory).expect("canonical TV directories must exist");
        fs::write(season_directory.join("s02e03 — existing.mp4"), b"unrelated")
            .expect("existing case-fold target must exist");
        let (state, transfer_id) = organization_state(record);

        assert_eq!(
            preview_organization(&state, &transfer_id),
            Err(VR_ORGANIZATION_CONFLICT)
        );
        assert_eq!(
            fs::read(destination.join("Provider/S02E03 — Existing.MP4"))
                .expect("existing target conflict must retain source"),
            vec![b'a'; 3]
        );

        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let source = tv_download_source(
            &[
                ("Provider/S02E03 — First.MP4", 3),
                ("Provider/S02E03 — Second.MKV", 4),
            ],
            &[0, 1],
        );
        let record = completed_tv_organization_record(&destination, source);
        let (state, transfer_id) = organization_state(record);
        let preview = preview_organization(&state, &transfer_id).expect("preview must succeed");
        let season_directory = destination.join("Exact  Show — 特別版/Season 02");
        fs::create_dir_all(&season_directory).expect("late target parent must exist");
        let late_target = season_directory.join("S02E03 — Second.MKV");
        fs::write(&late_target, b"unrelated").expect("late target must exist");
        let mut move_calls = 0;

        assert_eq!(
            apply_organization_with(
                &state,
                &fixture.path.join("downloads"),
                &preview[0],
                |source, destination| {
                    move_calls += 1;
                    fs::rename(source, destination)
                },
            ),
            Err(VR_ORGANIZATION_CONFLICT)
        );
        assert_eq!(move_calls, 0);
        assert!(destination.join("Provider/S02E03 — First.MP4").is_file());
        assert!(destination.join("Provider/S02E03 — Second.MKV").is_file());
        assert_eq!(
            fs::read(late_target).expect("late target must not be overwritten"),
            b"unrelated"
        );
    }

    #[test]
    fn tv_complete_filename_limit_accepts_the_boundary_and_rejects_the_next_unit() {
        for (episode_length, expected) in [(238, Ok(())), (239, Err(VR_ORGANIZATION_INELIGIBLE))] {
            let fixture = FilesystemFixture::new();
            let destination =
                fs::canonicalize(&fixture.path).expect("destination must canonicalize");
            let mut source = tv_download_source(&[("Provider/Episode.MP4", 3)], &[0]);
            let identity = source.tv_identity.as_mut().expect("TV identity must exist");
            identity.show_name = "A".to_owned();
            identity.episode_name = "B".repeat(episode_length);
            let record = completed_tv_organization_record(&destination, source);
            let (state, transfer_id) = organization_state(record);
            let result = preview_organization(&state, &transfer_id);
            match expected {
                Ok(()) => {
                    let preview = result.expect("255-unit TV filename must preview");
                    let filename = preview[7]
                        .rsplit_once('/')
                        .expect("TV target must have a parent")
                        .1;
                    assert_eq!(filename.len(), 255);
                    assert_eq!(filename.encode_utf16().count(), 255);
                }
                Err(error) => assert_eq!(result, Err(error)),
            }
            assert_eq!(
                fs::read(destination.join("Provider/Episode.MP4"))
                    .expect("rejected preview must retain source"),
                vec![b'a'; 3]
            );
        }
    }

    #[test]
    fn tv_preview_rejects_unsafe_names_and_changed_exact_identity_before_mutation() {
        for (show_name, episode_name) in [
            ("Unsafe/Show", "Exact Episode"),
            ("Exact Show", "Unsafe:Episode"),
            ("CON", "Exact Episode"),
            ("Exact Show", "NUL"),
            ("Trailing ", "Exact Episode"),
            ("Exact Show", "Trailing."),
            ("Exact\u{7}Show", "Exact Episode"),
        ] {
            let fixture = FilesystemFixture::new();
            let destination =
                fs::canonicalize(&fixture.path).expect("destination must canonicalize");
            let mut source = tv_download_source(&[("Provider/Episode.mp4", 3)], &[0]);
            let identity = source.tv_identity.as_mut().expect("TV identity must exist");
            identity.show_name = show_name.to_owned();
            identity.episode_name = episode_name.to_owned();
            let record = completed_tv_organization_record(&destination, source);
            let (state, transfer_id) = organization_state(record);
            assert_eq!(
                preview_organization(&state, &transfer_id),
                Err(VR_ORGANIZATION_INELIGIBLE),
                "unsafe TV identity {show_name:?} / {episode_name:?} was eligible"
            );
            assert_eq!(
                fs::read(destination.join("Provider/Episode.mp4"))
                    .expect("unsafe preview must retain source"),
                vec![b'a'; 3]
            );
        }

        for alteration in 0..12 {
            let fixture = FilesystemFixture::new();
            let destination =
                fs::canonicalize(&fixture.path).expect("destination must canonicalize");
            let source = tv_download_source(&[("Provider/Episode.mp4", 3)], &[0]);
            let mut record = completed_tv_organization_record(&destination, source);
            let identity = record.tv_identity.as_mut().expect("TV identity must exist");
            match alteration {
                0 => identity.tmdb_tv_id += 1,
                1 => identity.show_name.push_str(" changed"),
                2 => identity.provider_season_id += 1,
                3 => identity.season_number += 1,
                4 => identity.provider_episode_id += 1,
                5 => identity.episode_number += 1,
                6 => identity.episode_name.push_str(" changed"),
                7 => identity.imdb_id = "tt7654321".to_owned(),
                8 => identity.provider_item_id = "1002".to_owned(),
                9 => identity.category = "208".to_owned(),
                10 => identity.release_name.push_str(" changed"),
                11 => identity.infohash = "0123456789abcdef0123456789abcdef01234567".to_owned(),
                _ => unreachable!(),
            }
            let source_path = destination.join("Provider/Episode.mp4");
            let (state, transfer_id) = organization_state(record);
            assert_eq!(
                preview_organization(&state, &transfer_id),
                Err(VR_ORGANIZATION_INELIGIBLE),
                "changed TV identity field {alteration} was eligible"
            );
            assert_eq!(
                fs::read(source_path).expect("changed identity must retain source"),
                vec![b'a'; 3]
            );
        }
    }

    #[test]
    fn tv_preview_rejects_exact_names_that_cannot_regroup_without_mutation() {
        for (show_name, episode_name) in [
            ("S123", "Exact Episode"),
            ("Season 123", "Exact Episode"),
            ("Exact S123E456 Show", "Exact Episode"),
            ("Exact Show", "Flashback 123x456"),
            ("Exact Show", "2"),
            ("Exact Show", "04"),
            ("Exact Show", "E04"),
            ("Exact Show", "x04"),
            ("Exact Show", "#4"),
        ] {
            let fixture = FilesystemFixture::new();
            let destination =
                fs::canonicalize(&fixture.path).expect("destination must canonicalize");
            let mut source = tv_download_source(&[("Provider/Episode.mp4", 3)], &[0]);
            let identity = source.tv_identity.as_mut().expect("TV identity must exist");
            identity.show_name = show_name.to_owned();
            identity.episode_name = episode_name.to_owned();
            let record = completed_tv_organization_record(&destination, source);
            let recovery_path = organization_recovery_path(&record);
            let recovery_successor_path = organization_recovery_successor_path(&record);
            let source_path = destination.join("Provider/Episode.mp4");
            let persistence_path = fixture.path.join("downloads");
            let (state, transfer_id) = organization_state(record);
            let before = transfer_snapshots(&state);

            assert_eq!(
                preview_organization(&state, &transfer_id),
                Err(VR_ORGANIZATION_INELIGIBLE),
                "unregroupable TV identity {show_name:?} / {episode_name:?} was eligible"
            );
            let mut move_calls = 0;
            assert_eq!(
                apply_organization_with(
                    &state,
                    &persistence_path,
                    "fabricated-plan",
                    |source, destination| {
                        move_calls += 1;
                        fs::rename(source, destination)
                    },
                ),
                Err(VR_ORGANIZATION_STALE)
            );

            assert_eq!(move_calls, 0);
            assert_eq!(
                fs::read(source_path).expect("rejected TV identity must retain source bytes"),
                vec![b'a'; 3]
            );
            assert!(!destination.join(show_name).exists());
            assert_eq!(transfer_snapshots(&state), before);
            assert!(!persistence_path.exists());
            assert!(!recovery_path.exists());
            assert!(!recovery_successor_path.exists());
        }
    }

    #[test]
    fn tv_preview_accepts_round_trippable_ordinary_episode_numbers() {
        for episode_name in ["Episode 4", "Part 2"] {
            let fixture = FilesystemFixture::new();
            let destination =
                fs::canonicalize(&fixture.path).expect("destination must canonicalize");
            let mut source = tv_download_source(&[("Provider/Episode.MKV", 3)], &[0]);
            let identity = source.tv_identity.as_mut().expect("TV identity must exist");
            identity.episode_name = episode_name.to_owned();
            let record = completed_tv_organization_record(&destination, source);
            let (state, transfer_id) = organization_state(record);

            let preview = preview_organization(&state, &transfer_id)
                .expect("ordinary episode number must round-trip");

            assert_eq!(
                preview[7],
                format!(
                    "Exact  Show — 特別版/Season 02/Exact  Show — 特別版 - S02E03 - {episode_name}.MKV"
                )
            );
            assert_eq!(
                fs::read(destination.join("Provider/Episode.MKV"))
                    .expect("preview must not mutate media"),
                vec![b'a'; 3]
            );
        }
    }

    #[test]
    fn tv_large_single_episode_preview_uses_a_regroupable_canonical_path() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let mut source = tv_download_source(&[("Provider/Episode.MKV", 3)], &[0]);
        let identity = source.tv_identity.as_mut().expect("TV identity must exist");
        identity.show_name = "Exact Big Show".to_owned();
        identity.season_number = 123;
        identity.episode_number = 456;
        identity.episode_name = "Exact Episode".to_owned();
        identity.release_name = "Exact Big Show.S123E456+720p".to_owned();
        source.release_name = identity.release_name.clone();
        let record = completed_tv_organization_record(&destination, source);
        let (state, transfer_id) = organization_state(record);

        let preview = preview_organization(&state, &transfer_id)
            .expect("large exact TV episode must preview");

        assert_eq!(preview[2], "tt0123456 · S123E456");
        assert_eq!(
            preview[7],
            "Exact Big Show/Season 123/Exact Big Show - S123E456 - Exact Episode.MKV"
        );
    }

    #[test]
    fn tv_large_multi_media_preview_preserves_each_exact_episode_identity() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let mut source = tv_download_source(
            &[
                ("Provider/S123E456 — Cut A.MP4", 3),
                ("Provider/123x456 — Cut B.MkV", 4),
            ],
            &[0, 1],
        );
        let identity = source.tv_identity.as_mut().expect("TV identity must exist");
        identity.show_name = "Exact Big Show".to_owned();
        identity.season_number = 123;
        identity.episode_number = 456;
        identity.episode_name = "Exact Episode".to_owned();
        identity.release_name = "Exact Big Show.S123E456+720p".to_owned();
        source.release_name = identity.release_name.clone();
        let record = completed_tv_organization_record(&destination, source);
        let (state, transfer_id) = organization_state(record);

        let preview = preview_organization(&state, &transfer_id)
            .expect("large exact multi-media episode must preview");

        assert_eq!(preview[2], "tt0123456 · S123E456");
        assert_eq!(
            &preview[5..],
            &[
                "move",
                "Provider/S123E456 — Cut A.MP4",
                "Exact Big Show/Season 123/S123E456 — Cut A.MP4",
                "move",
                "Provider/123x456 — Cut B.MkV",
                "Exact Big Show/Season 123/123x456 — Cut B.MkV",
            ]
        );
    }

    #[test]
    fn tv_multi_media_retained_filename_accepts_the_portable_component_boundary() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let retained_name = format!("S02E03 {}.MP4", "A".repeat(244));
        assert_eq!(retained_name.len(), 255);
        assert_eq!(retained_name.encode_utf16().count(), 255);
        let retained_path = format!("Provider/{retained_name}");
        let source = tv_download_source(
            &[(&retained_path, 3), ("Provider/S02E03 — Other.MKV", 4)],
            &[0, 1],
        );
        let record = completed_tv_organization_record(&destination, source);
        let (state, transfer_id) = organization_state(record);

        let preview = preview_organization(&state, &transfer_id)
            .expect("largest portable retained TV filename must preview");

        assert_eq!(
            preview[7],
            format!("Exact  Show — 特別版/Season 02/{retained_name}")
        );
    }

    #[test]
    fn tv_apply_rejects_changed_retained_boundary_authority_before_move_dispatch() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let record = completed_tv_organization_record(
            &destination,
            tv_download_source(
                &[("Provider/Episode.mp4", 3), ("Provider/unselected.bin", 5)],
                &[0],
            ),
        );
        let (state, transfer_id) = organization_state(record);
        let preview = preview_organization(&state, &transfer_id).expect("preview must succeed");
        {
            let context = state.0.lock().expect("state must lock");
            let StoredTransfer::Valid(record) = &context.transfers[0] else {
                panic!("TV transfer must remain valid");
            };
            record
                .boundary_segments
                .lock()
                .expect("boundary state must lock")
                .insert(
                    1,
                    vec![SparseSegment {
                        offset: 0,
                        bytes: vec![b'x'],
                    }],
                );
        }

        let mut move_calls = 0;
        assert_eq!(
            apply_organization_with(
                &state,
                &fixture.path.join("downloads"),
                &preview[0],
                |source, destination| {
                    move_calls += 1;
                    fs::rename(source, destination)
                },
            ),
            Err(VR_ORGANIZATION_STALE)
        );
        assert_eq!(move_calls, 0);
        assert_eq!(
            fs::read(destination.join("Provider/Episode.mp4"))
                .expect("stale Apply must retain TV media"),
            vec![b'a'; 3]
        );
    }

    #[test]
    fn tv_multi_media_apply_rejects_changed_file_or_boundary_before_any_move() {
        for alteration in 0..2 {
            let fixture = FilesystemFixture::new();
            let destination =
                fs::canonicalize(&fixture.path).expect("destination must canonicalize");
            let record = completed_tv_organization_record(
                &destination,
                tv_download_source(
                    &[
                        ("Provider/S02E03 — Cut A.MP4", 3),
                        ("Provider/S02E03 — Cut B.MKV", 4),
                        ("Provider/unselected.bin", 5),
                    ],
                    &[0, 1],
                ),
            );
            let (state, transfer_id) = organization_state(record);
            let preview = preview_organization(&state, &transfer_id).expect("preview must succeed");
            if alteration == 0 {
                let mut context = state.0.lock().expect("state must lock");
                let StoredTransfer::Valid(record) = &mut context.transfers[0] else {
                    panic!("TV transfer must remain valid");
                };
                record.fingerprints[1].push('0');
            } else {
                let context = state.0.lock().expect("state must lock");
                let StoredTransfer::Valid(record) = &context.transfers[0] else {
                    panic!("TV transfer must remain valid");
                };
                record
                    .boundary_segments
                    .lock()
                    .expect("boundary state must lock")
                    .insert(
                        1,
                        vec![SparseSegment {
                            offset: 0,
                            bytes: vec![b'x'],
                        }],
                    );
            }
            let mut move_calls = 0;

            let result = apply_organization_with(
                &state,
                &fixture.path.join("downloads"),
                &preview[0],
                |source, destination| {
                    move_calls += 1;
                    fs::rename(source, destination)
                },
            );
            assert_eq!(
                result,
                Err(VR_ORGANIZATION_STALE),
                "alteration {alteration} reached move dispatch"
            );
            assert_eq!(move_calls, 0);
            assert!(destination.join("Provider/S02E03 — Cut A.MP4").is_file());
            assert!(destination.join("Provider/S02E03 — Cut B.MKV").is_file());
            assert!(!destination.join("Exact  Show — 特別版").exists());
        }
    }

    #[test]
    fn tv_preview_rejects_case_and_unicode_normalization_collisions() {
        for existing_show in ["exact  show — 特別版", "Exact  Show — 特別版"] {
            let fixture = FilesystemFixture::new();
            let destination =
                fs::canonicalize(&fixture.path).expect("destination must canonicalize");
            let record = completed_tv_organization_record(
                &destination,
                tv_download_source(&[("Provider/Episode.mp4", 3)], &[0]),
            );
            fs::create_dir(destination.join(existing_show)).expect("show fixture must exist");
            if existing_show == "Exact  Show — 特別版" {
                fs::create_dir(destination.join(existing_show).join("season 02"))
                    .expect("season fixture must exist");
            }
            let (state, transfer_id) = organization_state(record);
            assert_eq!(
                preview_organization(&state, &transfer_id),
                Err(VR_ORGANIZATION_CONFLICT)
            );
        }

        for (show_name, existing_show) in [
            ("Café", "Cafe\u{301}"),
            ("Ḋ Show", "D\u{307} Show"),
            ("A\u{323}\u{307} Show", "A\u{307}\u{323} Show"),
        ] {
            let fixture = FilesystemFixture::new();
            let destination =
                fs::canonicalize(&fixture.path).expect("destination must canonicalize");
            let mut source = tv_download_source(&[("Provider/Episode.mp4", 3)], &[0]);
            source
                .tv_identity
                .as_mut()
                .expect("TV identity must exist")
                .show_name = show_name.to_owned();
            let record = completed_tv_organization_record(&destination, source);
            fs::create_dir(destination.join(existing_show))
                .expect("normalization-colliding show must exist");
            let source_path = destination.join("Provider/Episode.mp4");
            let (state, transfer_id) = organization_state(record);
            assert_eq!(
                preview_organization(&state, &transfer_id),
                Err(VR_ORGANIZATION_CONFLICT),
                "canonically equivalent show {show_name:?} / {existing_show:?} was eligible"
            );
            assert_eq!(
                fs::read(source_path).expect("colliding show must retain source media"),
                vec![b'a'; 3]
            );
        }

        for (episode_name, existing_name) in [
            ("Cut Ḋ", "Exact  Show — 特別版 - S02E03 - Cut D\u{307}.mp4"),
            (
                "Cut A\u{323}\u{307}",
                "Exact  Show — 特別版 - S02E03 - Cut A\u{307}\u{323}.mp4",
            ),
        ] {
            let fixture = FilesystemFixture::new();
            let destination =
                fs::canonicalize(&fixture.path).expect("destination must canonicalize");
            let mut source = tv_download_source(&[("Provider/Episode.mp4", 3)], &[0]);
            source
                .tv_identity
                .as_mut()
                .expect("TV identity must exist")
                .episode_name = episode_name.to_owned();
            let record = completed_tv_organization_record(&destination, source);
            let season_directory = destination.join("Exact  Show — 特別版/Season 02");
            fs::create_dir_all(&season_directory).expect("canonical TV directories must exist");
            fs::write(season_directory.join(existing_name), vec![b'x'; 3])
                .expect("normalization-colliding target must exist");
            let source_path = destination.join("Provider/Episode.mp4");
            let (state, transfer_id) = organization_state(record);
            assert_eq!(
                preview_organization(&state, &transfer_id),
                Err(VR_ORGANIZATION_CONFLICT),
                "canonically equivalent episode filename {episode_name:?} / {existing_name:?} was eligible"
            );
            assert_eq!(
                fs::read(source_path).expect("colliding target must retain source media"),
                vec![b'a'; 3]
            );
        }
    }

    #[test]
    fn tv_apply_rejects_late_canonical_collisions_before_creation_or_move_dispatch() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let mut source = tv_download_source(&[("Provider/Episode.mp4", 3)], &[0]);
        source
            .tv_identity
            .as_mut()
            .expect("TV identity must exist")
            .show_name = "Ḋ Show".to_owned();
        let record = completed_tv_organization_record(&destination, source);
        let (state, transfer_id) = organization_state(record);
        let preview = preview_organization(&state, &transfer_id).expect("preview must succeed");
        let existing_show = destination.join("D\u{307} Show");
        fs::create_dir(&existing_show).expect("late normalization-colliding show must exist");

        let mut move_calls = 0;
        assert_eq!(
            apply_organization_with(
                &state,
                &fixture.path.join("downloads"),
                &preview[0],
                |source, destination| {
                    move_calls += 1;
                    fs::rename(source, destination)
                },
            ),
            Err(VR_ORGANIZATION_CONFLICT)
        );
        assert_eq!(move_calls, 0);
        assert!(!existing_show.join("Season 02").exists());
        assert_eq!(
            fs::read(destination.join("Provider/Episode.mp4"))
                .expect("late directory collision must retain source"),
            vec![b'a'; 3]
        );

        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let mut source = tv_download_source(&[("Provider/Episode.mp4", 3)], &[0]);
        source
            .tv_identity
            .as_mut()
            .expect("TV identity must exist")
            .episode_name = "Cut A\u{323}\u{307}".to_owned();
        let record = completed_tv_organization_record(&destination, source);
        let (state, transfer_id) = organization_state(record);
        let preview = preview_organization(&state, &transfer_id).expect("preview must succeed");
        let season_directory = destination.join("Exact  Show — 特別版/Season 02");
        fs::create_dir_all(&season_directory).expect("canonical TV directories must exist");
        fs::write(
            season_directory.join("Exact  Show — 特別版 - S02E03 - Cut A\u{307}\u{323}.mp4"),
            vec![b'x'; 3],
        )
        .expect("late normalization-colliding target must exist");

        let mut move_calls = 0;
        assert_eq!(
            apply_organization_with(
                &state,
                &fixture.path.join("downloads"),
                &preview[0],
                |source, destination| {
                    move_calls += 1;
                    fs::rename(source, destination)
                },
            ),
            Err(VR_ORGANIZATION_CONFLICT)
        );
        assert_eq!(move_calls, 0);
        assert_eq!(
            fs::read(destination.join("Provider/Episode.mp4"))
                .expect("late collision must retain source"),
            vec![b'a'; 3]
        );
    }

    #[test]
    fn tv_preview_rejects_noncompleted_recovered_old_folder_and_organized_rows() {
        for transfer_state in [
            TransferState::Queued,
            TransferState::Downloading,
            TransferState::Paused,
            TransferState::Cancelled,
            TransferState::Offline,
            TransferState::Failed,
        ] {
            let fixture = FilesystemFixture::new();
            let destination =
                fs::canonicalize(&fixture.path).expect("destination must canonicalize");
            let mut record = completed_tv_organization_record(
                &destination,
                tv_download_source(&[("Provider/Episode.mp4", 3)], &[0]),
            );
            record.state = transfer_state;
            let (state, transfer_id) = organization_state(record);
            assert_eq!(
                preview_organization(&state, &transfer_id),
                Err(VR_ORGANIZATION_INELIGIBLE),
                "noncompleted TV state {transfer_state:?} was eligible"
            );
        }

        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let mut recovered = completed_tv_organization_record(
            &destination,
            tv_download_source(&[("Provider/Episode.mp4", 3)], &[0]),
        );
        recovered.state = TransferState::Failed;
        recovered.terminal_recovery_generation = Some(7);
        let (state, transfer_id) = organization_state(recovered);
        assert_eq!(
            preview_organization(&state, &transfer_id),
            Err(VR_ORGANIZATION_INELIGIBLE)
        );

        let fixture = FilesystemFixture::new();
        let current = fixture.path.join("current");
        let old = fixture.path.join("old");
        fs::create_dir(&current).expect("current TV folder must exist");
        fs::create_dir(&old).expect("old TV folder must exist");
        let current = fs::canonicalize(current).expect("current TV folder must canonicalize");
        let old = fs::canonicalize(old).expect("old TV folder must canonicalize");
        let old_record = completed_tv_organization_record(
            &old,
            tv_download_source(&[("Provider/Episode.mp4", 3)], &[0]),
        );
        let (state, transfer_id) = organization_state(old_record);
        configure_tv_download_folder(&state, Some(current)).expect("TV folder must change");
        assert_eq!(
            preview_organization(&state, &transfer_id),
            Err(VR_ORGANIZATION_INELIGIBLE)
        );

        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let mut organized = completed_tv_organization_record(
            &destination,
            tv_download_source(&[("Provider/Episode.mp4", 3)], &[0]),
        );
        organized.organization_state = OrganizationState::Organized;
        let (state, transfer_id) = organization_state(organized);
        assert_eq!(
            preview_organization(&state, &transfer_id),
            Err(VR_ORGANIZATION_INELIGIBLE)
        );
    }

    #[test]
    fn tv_multi_media_move_failure_rolls_every_member_back_without_a_partial_result() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let source = tv_download_source(
            &[
                ("Provider/S02E03 — Cut A.MP4", 3),
                ("Provider/S02E03 — Cut B.MKV", 4),
                ("Provider/notes exact.txt", 5),
            ],
            &[0, 1, 2],
        );
        let record = completed_tv_organization_record(&destination, source);
        let recovery_path = organization_recovery_path(&record);
        let (state, transfer_id) = organization_state(record);
        let persistence_path = fixture.path.join("downloads");
        let preview = preview_organization(&state, &transfer_id).expect("preview must succeed");
        let mut move_calls = 0;

        assert_eq!(
            apply_organization_with(
                &state,
                &persistence_path,
                &preview[0],
                |source, destination| {
                    move_calls += 1;
                    if move_calls == 2 {
                        Err(io::Error::other("injected second TV move failure"))
                    } else {
                        fs::rename(source, destination)
                    }
                },
            ),
            Err(VR_ORGANIZATION_FAILED)
        );
        assert_eq!(move_calls, 3);
        for (path, bytes) in [
            ("Provider/S02E03 — Cut A.MP4", vec![b'a'; 3]),
            ("Provider/S02E03 — Cut B.MKV", vec![b'b'; 4]),
            ("Provider/notes exact.txt", vec![b'c'; 5]),
        ] {
            assert_eq!(
                fs::read(destination.join(path)).expect("rollback must restore every source"),
                bytes
            );
        }
        assert!(!destination.join("Exact  Show — 特別版").exists());
        assert!(!recovery_path.exists());
        let context = state.0.lock().expect("state must lock");
        let StoredTransfer::Valid(record) = &context.transfers[0] else {
            panic!("rolled-back TV transfer must remain valid");
        };
        assert_eq!(record.organization_state, OrganizationState::None);
        assert_eq!(
            record.current_paths,
            [
                "Provider/S02E03 — Cut A.MP4",
                "Provider/S02E03 — Cut B.MKV",
                "Provider/notes exact.txt",
            ]
        );
    }

    #[test]
    fn tv_multi_media_rollback_failure_recovers_exact_paths_and_dismisses_without_moving_files() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let source = tv_download_source(
            &[
                ("Provider/S02E03 — Cut A.MP4", 3),
                ("Provider/S02E03 — Cut B.MKV", 4),
                ("Provider/notes exact.txt", 5),
            ],
            &[0, 1, 2],
        );
        let record = completed_tv_organization_record(&destination, source);
        let expected_boundary = encoded_boundary_segments(&record).expect("boundary must encode");
        let (state, transfer_id) = organization_state(record);
        let persistence_path = fixture.path.join("downloads");
        {
            let context = state.0.lock().expect("state must lock");
            write_persisted_transfers(&persistence_path, &context.transfers)
                .expect("original multi-media TV paths must persist");
        }
        let preview = preview_organization(&state, &transfer_id).expect("preview must succeed");
        let mut move_calls = 0;

        assert_eq!(
            apply_organization_with_persistence(
                &state,
                &persistence_path,
                &preview[0],
                |source, destination| {
                    move_calls += 1;
                    if move_calls == 3 {
                        Err(io::Error::other("injected multi-media rollback failure"))
                    } else {
                        fs::rename(source, destination)
                    }
                },
                |_, _| Err(VR_DOWNLOAD_PERSISTENCE_FAILED),
            ),
            Err(VR_DOWNLOAD_PERSISTENCE_FAILED)
        );
        assert_eq!(move_calls, 4);
        let first_source = destination.join("Provider/S02E03 — Cut A.MP4");
        let second_destination =
            destination.join("Exact  Show — 特別版/Season 02/S02E03 — Cut B.MKV");
        let unchanged = destination.join("Provider/notes exact.txt");
        assert_eq!(
            fs::read(&first_source).expect("first media source must be restored"),
            vec![b'a'; 3]
        );
        assert_eq!(
            fs::read(&second_destination).expect("failed rollback path must remain exact"),
            vec![b'b'; 4]
        );
        assert_eq!(
            fs::read(&unchanged).expect("selected non-media must remain exact"),
            vec![b'c'; 5]
        );

        let restarted = VrDownloadState::default();
        configure_tv_download_folder(&restarted, Some(destination.clone()))
            .expect("TV folder must restore");
        let rows = tauri::async_runtime::block_on(load_downloads(
            &restarted,
            &persistence_path,
            &fixture.path.join("tv-multi-recovery-session"),
            &fixture.path.join("limit"),
        ))
        .expect("multi-media TV attention recovery must load");
        assert_eq!(
            &rows[8..13],
            &[
                "completed",
                "true",
                "attention",
                "Exact  Show — 特別版/Season 02/",
                "true",
            ]
        );
        let context = restarted.0.lock().expect("state must lock");
        let StoredTransfer::Valid(record) = &context.transfers[0] else {
            panic!("recovered multi-media TV transfer must remain valid");
        };
        assert_eq!(
            record.current_paths,
            [
                "Provider/S02E03 — Cut A.MP4",
                "Exact  Show — 特別版/Season 02/S02E03 — Cut B.MKV",
                "Provider/notes exact.txt",
            ]
        );
        assert_eq!(
            encoded_boundary_segments(record).expect("recovery boundary must encode"),
            expected_boundary
        );
        assert!(context.session.is_none(), "TV recovery started a session");
        drop(context);

        dismiss_download(&restarted, &persistence_path, &transfer_id)
            .expect("multi-media TV attention row must dismiss");
        let dismissed = VrDownloadState::default();
        configure_tv_download_folder(&dismissed, Some(destination))
            .expect("TV folder must restore after dismissal");
        let rows = tauri::async_runtime::block_on(load_downloads(
            &dismissed,
            &persistence_path,
            &fixture.path.join("tv-multi-recovery-dismissed"),
            &fixture.path.join("limit"),
        ))
        .expect("dismissed multi-media TV recovery must remain absent");
        assert!(rows.is_empty());
        assert_eq!(
            fs::read(first_source).expect("dismiss must retain restored media"),
            vec![b'a'; 3]
        );
        assert_eq!(
            fs::read(second_destination).expect("dismiss must retain moved media"),
            vec![b'b'; 4]
        );
        assert_eq!(
            fs::read(unchanged).expect("dismiss must retain selected non-media"),
            vec![b'c'; 5]
        );
    }

    #[test]
    fn tv_persistence_and_rollback_failure_recovers_exact_paths_and_dismisses_durably() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let source = tv_download_source(
            &[
                ("Provider/Episode  A.S02E03.mp4", 3),
                ("Provider/notes  exact.txt", 4),
            ],
            &[0, 1],
        );
        let record = completed_tv_organization_record(&destination, source);
        let expected_identity = record.tv_identity.clone();
        let expected_boundary = encoded_boundary_segments(&record).expect("boundary must encode");
        let (state, transfer_id) = organization_state(record);
        let persistence_path = fixture.path.join("downloads");
        {
            let context = state.0.lock().expect("state must lock");
            write_persisted_transfers(&persistence_path, &context.transfers)
                .expect("original TV paths must persist");
        }
        let preview = preview_organization(&state, &transfer_id).expect("preview must succeed");
        let mut move_calls = 0;
        assert_eq!(
            apply_organization_with_persistence(
                &state,
                &persistence_path,
                &preview[0],
                |source, destination| {
                    move_calls += 1;
                    if move_calls == 2 {
                        Err(io::Error::other("injected TV rollback failure"))
                    } else {
                        fs::rename(source, destination)
                    }
                },
                |_, _| Err(VR_DOWNLOAD_PERSISTENCE_FAILED),
            ),
            Err(VR_DOWNLOAD_PERSISTENCE_FAILED)
        );
        assert_eq!(move_calls, 2);
        let moved_path = destination.join(
            "Exact  Show — 特別版/Season 02/Exact  Show — 特別版 - S02E03 - 第三話  —  Exact Episode.mp4",
        );
        let unchanged_path = destination.join("Provider/notes  exact.txt");
        assert_eq!(
            fs::read(&moved_path).expect("moved TV media must remain"),
            vec![b'a'; 3]
        );
        assert_eq!(
            fs::read(&unchanged_path).expect("selected non-media file must remain unchanged"),
            vec![b'b'; 4]
        );

        let restarted = VrDownloadState::default();
        configure_tv_download_folder(&restarted, Some(destination.clone()))
            .expect("TV folder must restore");
        let rows = tauri::async_runtime::block_on(load_downloads(
            &restarted,
            &persistence_path,
            &fixture.path.join("tv-recovery-session"),
            &fixture.path.join("limit"),
        ))
        .expect("TV attention recovery must load");
        assert_eq!(rows[1], "tv");
        assert_eq!(
            &rows[8..13],
            &[
                "completed",
                "true",
                "attention",
                "Exact  Show — 特別版/Season 02/",
                "true",
            ]
        );
        let context = restarted.0.lock().expect("state must lock");
        let StoredTransfer::Valid(record) = &context.transfers[0] else {
            panic!("recovered TV transfer must remain valid");
        };
        assert_eq!(record.tv_identity, expected_identity);
        assert_eq!(
            record.current_paths,
            [
                "Exact  Show — 特別版/Season 02/Exact  Show — 特別版 - S02E03 - 第三話  —  Exact Episode.mp4",
                "Provider/notes  exact.txt",
            ]
        );
        assert_eq!(
            encoded_boundary_segments(record).expect("recovery boundary must encode"),
            expected_boundary
        );
        assert!(context.session.is_none(), "TV recovery started a session");
        drop(context);

        dismiss_download(&restarted, &persistence_path, &transfer_id)
            .expect("TV attention row must dismiss");
        let dismissed = VrDownloadState::default();
        configure_tv_download_folder(&dismissed, Some(destination))
            .expect("TV folder must restore after dismissal");
        let rows = tauri::async_runtime::block_on(load_downloads(
            &dismissed,
            &persistence_path,
            &fixture.path.join("tv-recovery-dismissed"),
            &fixture.path.join("limit"),
        ))
        .expect("dismissed TV recovery must remain absent");
        assert!(rows.is_empty());
        assert_eq!(
            fs::read(moved_path).expect("dismissed moved TV media must remain"),
            vec![b'a'; 3]
        );
        assert_eq!(
            fs::read(unchanged_path).expect("dismissed selected non-media must remain"),
            vec![b'b'; 4]
        );
    }

    #[test]
    fn movie_single_file_component_limit_preserves_the_boundary_and_rejects_the_next_title() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let title = "A".repeat(244);
        let directory_name = format!("{title} (1999)");
        let file_name = format!("{directory_name}.mp4");
        assert_eq!(file_name.len(), 255);
        assert_eq!(file_name.encode_utf16().count(), 255);
        let source = movie_organization_source(
            &title,
            Some("1999-04-19"),
            "Provider title",
            &[("Provider/Feature.mp4", 3)],
            &[0],
        );
        let record = completed_movie_organization_record(&destination, source);
        let recovery_path = organization_recovery_path(&record);
        let (state, transfer_id) = organization_state(record);

        let preview = preview_organization(&state, &transfer_id)
            .expect("largest portable Movie filename must preview");
        assert_eq!(preview[7], format!("{directory_name}/{file_name}"));
        assert!(!destination.join(&directory_name).exists());
        assert!(!recovery_path.exists());
        assert_eq!(
            fs::read(destination.join("Provider/Feature.mp4"))
                .expect("preview must retain the exact source"),
            vec![b'a'; 3]
        );

        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let title = "A".repeat(245);
        let directory_name = format!("{title} (1999)");
        let file_name = format!("{directory_name}.mp4");
        assert_eq!(directory_name.len(), 252);
        assert_eq!(directory_name.encode_utf16().count(), 252);
        assert_eq!(file_name.len(), 256);
        assert_eq!(file_name.encode_utf16().count(), 256);
        let source = movie_organization_source(
            &title,
            Some("1999-04-19"),
            "Provider title",
            &[("Provider/Feature.mp4", 3)],
            &[0],
        );
        let record = completed_movie_organization_record(&destination, source);
        let recovery_path = organization_recovery_path(&record);
        let (state, transfer_id) = organization_state(record);

        assert_eq!(
            preview_organization(&state, &transfer_id),
            Err(VR_ORGANIZATION_INELIGIBLE)
        );
        assert!(!destination.join(directory_name).exists());
        assert!(!recovery_path.exists());
        assert_eq!(
            fs::read(destination.join("Provider/Feature.mp4"))
                .expect("rejected preview must retain the exact source"),
            vec![b'a'; 3]
        );
    }

    #[test]
    fn movie_organization_rejects_unsafe_title_or_year_and_altered_identity_without_mutation() {
        for (title, release_date) in [
            ("Unsafe/Title", Some("1999-04-19")),
            ("Unsafe:Title", Some("1999-04-19")),
            (".", Some("1999-04-19")),
            ("..", Some("1999-04-19")),
            ("CON", Some("1999-04-19")),
            ("COM¹", Some("1999-04-19")),
            ("Trailing.", Some("1999-04-19")),
            ("Trailing ", Some("1999-04-19")),
            ("Exact Movie", None),
            ("Exact Movie", Some("1999-02-30")),
        ] {
            let fixture = FilesystemFixture::new();
            let destination =
                fs::canonicalize(&fixture.path).expect("destination must canonicalize");
            let source = movie_organization_source(
                title,
                release_date,
                "Provider title",
                &[("Provider/Feature.mp4", 3)],
                &[0],
            );
            let record = completed_movie_organization_record(&destination, source);
            let source_path = destination.join("Provider/Feature.mp4");
            let (state, transfer_id) = organization_state(record);
            assert_eq!(
                preview_organization(&state, &transfer_id),
                Err(VR_ORGANIZATION_INELIGIBLE),
                "unsafe identity {title:?} {release_date:?} was eligible"
            );
            assert_eq!(
                fs::read(source_path).expect("source must remain"),
                vec![b'a'; 3]
            );
        }

        for alteration in 0..6 {
            let fixture = FilesystemFixture::new();
            let destination =
                fs::canonicalize(&fixture.path).expect("destination must canonicalize");
            let source = movie_organization_source(
                "Exact  Movie — 特別版",
                Some("1999-04-19"),
                "YTS Substitute Title",
                &[("Provider/Feature.mp4", 3)],
                &[0],
            );
            let mut record = completed_movie_organization_record(&destination, source);
            let identity = record
                .movie_identity
                .as_mut()
                .expect("Movie identity must exist");
            match alteration {
                0 => identity.tmdb_movie_id = 420,
                1 => identity.imdb_id = "tt7654321".to_owned(),
                2 => identity.provider_movie_id = 701,
                3 => identity.row_id = "700:1".to_owned(),
                4 => identity.release_date = Some("1999".to_owned()),
                5 => record.release_name = "YTS Substitute Title".to_owned(),
                _ => unreachable!(),
            }
            let source_path = destination.join("Provider/Feature.mp4");
            let (state, transfer_id) = organization_state(record);
            assert_eq!(
                preview_organization(&state, &transfer_id),
                Err(VR_ORGANIZATION_INELIGIBLE),
                "altered Movie identity {alteration} was eligible"
            );
            assert_eq!(
                fs::read(source_path).expect("source must remain"),
                vec![b'a'; 3]
            );
        }
    }

    #[test]
    fn movie_preview_rejects_duplicate_and_case_colliding_canonical_targets() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let source = movie_organization_source(
            "Exact Movie",
            Some("1999-04-19"),
            "Provider title",
            &[("A/Feature.mp4", 3), ("B/feature.MP4", 4)],
            &[0, 1],
        );
        let record = completed_movie_organization_record(&destination, source);
        let (state, transfer_id) = organization_state(record);
        assert_eq!(
            preview_organization(&state, &transfer_id),
            Err(VR_ORGANIZATION_CONFLICT)
        );

        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let source = movie_organization_source(
            "Exact Movie",
            Some("1999-04-19"),
            "Provider title",
            &[("Provider/Feature.mp4", 3)],
            &[0],
        );
        let record = completed_movie_organization_record(&destination, source);
        fs::create_dir(destination.join("exact movie (1999)"))
            .expect("case-colliding directory must exist");
        let (state, transfer_id) = organization_state(record);
        assert_eq!(
            preview_organization(&state, &transfer_id),
            Err(VR_ORGANIZATION_CONFLICT)
        );
    }

    #[test]
    fn movie_folder_change_invalidates_the_plan_before_move_dispatch() {
        let fixture = FilesystemFixture::new();
        let destination = fixture.path.join("current");
        let replacement = fixture.path.join("replacement");
        fs::create_dir(&destination).expect("current Movies folder must exist");
        fs::create_dir(&replacement).expect("replacement Movies folder must exist");
        let destination =
            fs::canonicalize(destination).expect("current Movies folder must canonicalize");
        let replacement =
            fs::canonicalize(replacement).expect("replacement Movies folder must canonicalize");
        let source = movie_organization_source(
            "Exact Movie",
            Some("1999-04-19"),
            "Provider title",
            &[("Provider/Feature.mp4", 3)],
            &[0],
        );
        let record = completed_movie_organization_record(&destination, source);
        let (state, transfer_id) = organization_state(record);
        let preview = preview_organization(&state, &transfer_id).expect("preview must succeed");
        configure_movie_download_folder(&state, Some(replacement))
            .expect("Movies folder must change");

        assert_eq!(
            apply_organization_with(
                &state,
                &fixture.path.join("downloads"),
                &preview[0],
                |_, _| panic!("folder-stale Movie plan dispatched"),
            ),
            Err(VR_ORGANIZATION_STALE)
        );
    }

    #[test]
    fn movie_noncanonical_persisted_organization_is_inert_and_keeps_media() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let source = movie_organization_source(
            "Exact Movie",
            Some("1999-04-19"),
            "Provider title",
            &[("Provider/Feature.mp4", 3)],
            &[0],
        );
        let mut record = completed_movie_organization_record(&destination, source);
        record.organization_state = OrganizationState::Attention;
        record.current_paths[0] = "Fabricated/Feature.mp4".to_owned();
        let mut persisted = PERSISTENCE_HEADER.to_vec();
        persisted.extend_from_slice(&encode_transfer(&record).expect("Movie transfer must encode"));
        persisted.push(b'\n');
        let persistence_path = fixture.path.join("downloads");
        fs::write(&persistence_path, persisted).expect("fabricated Movie state must persist");

        let state = VrDownloadState::default();
        configure_movie_download_folder(&state, Some(destination.clone()))
            .expect("Movies folder must configure");
        let rows = tauri::async_runtime::block_on(load_downloads(
            &state,
            &persistence_path,
            &fixture.path.join("corrupt-movie-session"),
            &fixture.path.join("limit"),
        ))
        .expect("fabricated Movie state must load inertly");
        assert_eq!(rows[1], "unknown");
        assert_eq!(&rows[8..13], &["offline", "false", "none", "", "false"]);
        assert_eq!(
            preview_organization(&state, &rows[0]),
            Err(VR_ORGANIZATION_STALE)
        );
        assert!(state.0.lock().expect("state must lock").session.is_none());
        assert_eq!(
            fs::read(destination.join("Provider/Feature.mp4"))
                .expect("original Movie media must remain"),
            vec![b'a'; 3]
        );
    }

    #[test]
    fn preserves_exact_multipart_labels_ambiguous_basenames_and_non_media_files() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let source = organization_source(vec![
            ("Source/MDVR-419 Part  000001 — 映画.MKV", 3),
            ("Source/MDVR-419 Disc-2 — 特別.mp4", 4),
            ("Source/MDVR-419 Part 3 Disc 4 — ambiguous.mkv", 5),
            ("Source/MDVR-419  feature  —  Final.mp4", 6),
            ("Source/notes  —  exact.txt", 7),
        ]);
        let record = completed_organization_record(&destination, source);
        let (state, transfer_id) = organization_state(record);

        let preview = preview_organization(&state, &transfer_id).expect("preview must succeed");
        assert_eq!(&preview[3..5], &["4", "5"]);
        assert_eq!(
            &preview[5..],
            &[
                "move",
                "Source/MDVR-419 Part  000001 — 映画.MKV",
                "MDVR-419/MDVR-419 - Part  000001.MKV",
                "move",
                "Source/MDVR-419 Disc-2 — 特別.mp4",
                "MDVR-419/MDVR-419 - Disc-2.mp4",
                "move",
                "Source/MDVR-419 Part 3 Disc 4 — ambiguous.mkv",
                "MDVR-419/MDVR-419 Part 3 Disc 4 — ambiguous.mkv",
                "move",
                "Source/MDVR-419  feature  —  Final.mp4",
                "MDVR-419/MDVR-419  feature  —  Final.mp4",
                "non-media-unchanged",
                "Source/notes  —  exact.txt",
                "",
            ]
        );
        apply_organization(&state, &fixture.path.join("downloads"), &preview[0])
            .expect("multipart organization must succeed");
        for path in [
            "MDVR-419/MDVR-419 - Part  000001.MKV",
            "MDVR-419/MDVR-419 - Disc-2.mp4",
            "MDVR-419/MDVR-419 Part 3 Disc 4 — ambiguous.mkv",
            "MDVR-419/MDVR-419  feature  —  Final.mp4",
            "Source/notes  —  exact.txt",
        ] {
            assert!(destination.join(path).exists(), "missing {path:?}");
        }
    }

    #[test]
    fn adult_part_label_parser_rejects_compact_continuations_without_changing_vr_results() {
        for ambiguous in [
            "ADLT-123 Part 1-2.mp4",
            "ADLT-123 CD1+2.mkv",
            "ADLT-123 Disc 03_04.MKV",
            "ADLT-123 Disk-4 5.mp4",
        ] {
            assert_eq!(
                exact_part_label(ambiguous, TransferCategory::Adult),
                None,
                "{ambiguous:?} was treated as one Adult label",
            );
        }
        for (file_name, label) in [
            ("ADLT-123 Part 01.mp4", "Part 01"),
            ("ADLT-123 CD2.mkv", "CD2"),
            ("ADLT-123 Disc 03.MKV", "Disc 03"),
            ("ADLT-123 Disk-4.mp4", "Disk-4"),
        ] {
            assert_eq!(
                exact_part_label(file_name, TransferCategory::Adult).as_deref(),
                Some(label),
            );
        }
        assert_eq!(
            exact_part_label("MDVR-419 Part 1-2.mp4", TransferCategory::Vr).as_deref(),
            Some("Part 1")
        );
        assert_eq!(
            exact_part_label("MDVR-419 CD1+2.mkv", TransferCategory::Vr).as_deref(),
            Some("CD1")
        );
    }

    #[test]
    fn adult_preview_and_apply_preserve_compact_basenames_and_valid_exact_labels() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let source = adult_organization_source(vec![
            ("Source/ADLT-123 Part 1-2.mp4", 3),
            ("Source/ADLT-123 CD1+2.mkv", 4),
            ("Source/ADLT-123 Part 01.MP4", 5),
            ("Source/ADLT-123 CD2.mkv", 6),
            ("Source/ADLT-123 Disc 03.MKV", 7),
            ("Source/ADLT-123 Disk-4.mp4", 8),
            ("Source/ADLT-123 Part 3 Disk 4 — ambiguous.mkv", 9),
            ("Source/notes  —  exact.txt", 10),
        ]);
        let record = completed_adult_organization_record(&destination, source);
        let (state, transfer_id) = organization_state(record);

        let preview = preview_organization(&state, &transfer_id).expect("preview must succeed");
        assert_eq!(&preview[3..5], &["7", "8"]);
        assert_eq!(
            &preview[5..],
            &[
                "move",
                "Source/ADLT-123 Part 1-2.mp4",
                "ADLT-123/ADLT-123 Part 1-2.mp4",
                "move",
                "Source/ADLT-123 CD1+2.mkv",
                "ADLT-123/ADLT-123 CD1+2.mkv",
                "move",
                "Source/ADLT-123 Part 01.MP4",
                "ADLT-123/ADLT-123 - Part 01.MP4",
                "move",
                "Source/ADLT-123 CD2.mkv",
                "ADLT-123/ADLT-123 - CD2.mkv",
                "move",
                "Source/ADLT-123 Disc 03.MKV",
                "ADLT-123/ADLT-123 - Disc 03.MKV",
                "move",
                "Source/ADLT-123 Disk-4.mp4",
                "ADLT-123/ADLT-123 - Disk-4.mp4",
                "move",
                "Source/ADLT-123 Part 3 Disk 4 — ambiguous.mkv",
                "ADLT-123/ADLT-123 Part 3 Disk 4 — ambiguous.mkv",
                "non-media-unchanged",
                "Source/notes  —  exact.txt",
                "",
            ]
        );
        apply_organization(&state, &fixture.path.join("downloads"), &preview[0])
            .expect("Adult multipart organization must succeed");
        for path in [
            "ADLT-123/ADLT-123 Part 1-2.mp4",
            "ADLT-123/ADLT-123 CD1+2.mkv",
            "ADLT-123/ADLT-123 - Part 01.MP4",
            "ADLT-123/ADLT-123 - CD2.mkv",
            "ADLT-123/ADLT-123 - Disc 03.MKV",
            "ADLT-123/ADLT-123 - Disk-4.mp4",
            "ADLT-123/ADLT-123 Part 3 Disk 4 — ambiguous.mkv",
            "Source/notes  —  exact.txt",
        ] {
            assert!(destination.join(path).exists(), "missing {path:?}");
        }
        for truncated in [
            "ADLT-123/ADLT-123 - Part 1.mp4",
            "ADLT-123/ADLT-123 - CD1.mkv",
        ] {
            assert!(
                !destination.join(truncated).exists(),
                "invented truncated destination {truncated:?}",
            );
        }
    }

    #[test]
    fn adult_preview_rejects_every_ambiguous_candidate_identity() {
        for file_name in [
            "ADLT-124 wrong item.mp4",
            "ADLT-1230 neighboring item.mp4",
            "XADLT-123 embedded item.mp4",
            "ADLT-123 + XYZ-7 pack.mp4",
            "ADLT-123 + PT-7 pack.mp4",
        ] {
            let fixture = FilesystemFixture::new();
            let destination =
                fs::canonicalize(&fixture.path).expect("destination must canonicalize");
            let record = completed_adult_organization_record(
                &destination,
                adult_organization_source(vec![(file_name, 3)]),
            );
            let (state, transfer_id) = organization_state(record);

            assert_eq!(
                preview_organization(&state, &transfer_id),
                Err(VR_ORGANIZATION_INELIGIBLE),
                "{file_name:?} was assigned to ADLT-123",
            );
        }
    }

    #[test]
    fn adult_folder_change_invalidates_the_plan_before_move_dispatch() {
        let fixture = FilesystemFixture::new();
        let destination = fixture.path.join("current");
        let replacement = fixture.path.join("replacement");
        fs::create_dir(&destination).expect("current Adult folder must exist");
        fs::create_dir(&replacement).expect("replacement Adult folder must exist");
        let destination =
            fs::canonicalize(destination).expect("current Adult folder must canonicalize");
        let replacement =
            fs::canonicalize(replacement).expect("replacement Adult folder must canonicalize");
        let record = completed_adult_organization_record(
            &destination,
            adult_organization_source(vec![("Source/ADLT-123.mp4", 3)]),
        );
        let (state, transfer_id) = organization_state(record);
        let preview = preview_organization(&state, &transfer_id).expect("preview must succeed");
        configure_adult_download_folder(&state, Some(replacement))
            .expect("Adult folder must change");
        let mut dispatched = false;

        assert_eq!(
            apply_organization_with(
                &state,
                &fixture.path.join("downloads"),
                &preview[0],
                |_, _| {
                    dispatched = true;
                    Ok(())
                },
            ),
            Err(VR_ORGANIZATION_STALE)
        );
        assert!(!dispatched, "folder-stale Adult plan reached mutation");
    }

    #[test]
    fn rejects_ambiguous_media_identity_and_complete_destination_collisions() {
        for file_name in [
            "MDVR-4190 neighboring.mp4",
            "XMDVR-419 embedded.mp4",
            "MDVR-419B attached.mp4",
            "MDVR-419 + ABC-123 mixed.mp4",
        ] {
            let fixture = FilesystemFixture::new();
            let destination =
                fs::canonicalize(&fixture.path).expect("destination must canonicalize");
            let record = completed_organization_record(
                &destination,
                organization_source(vec![(file_name, 3)]),
            );
            let (state, transfer_id) = organization_state(record);
            assert_eq!(
                preview_organization(&state, &transfer_id),
                Err(VR_ORGANIZATION_INELIGIBLE),
                "{file_name:?} was assigned to MDVR-419",
            );
        }

        for files in [
            vec![("A/MDVR-419 Part 1.mp4", 3), ("B/MDVR-419 part 1.mp4", 4)],
            vec![("A/MDVR-419 feature.mp4", 3), ("B/MDVR-419 feature.mp4", 4)],
        ] {
            let fixture = FilesystemFixture::new();
            let destination =
                fs::canonicalize(&fixture.path).expect("destination must canonicalize");
            let record = completed_organization_record(&destination, organization_source(files));
            let (state, transfer_id) = organization_state(record);
            assert_eq!(
                preview_organization(&state, &transfer_id),
                Err(VR_ORGANIZATION_CONFLICT)
            );
        }

        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let record = completed_organization_record(
            &destination,
            organization_source(vec![("Source/MDVR-419.mp4", 3)]),
        );
        fs::create_dir(destination.join("MDVR-419")).expect("canonical directory must exist");
        fs::write(destination.join("MDVR-419/mdvr-419.MP4"), b"unrelated")
            .expect("case-colliding target must exist");
        let (state, transfer_id) = organization_state(record);
        assert_eq!(
            preview_organization(&state, &transfer_id),
            Err(VR_ORGANIZATION_CONFLICT)
        );

        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let record = completed_organization_record(
            &destination,
            organization_source(vec![("Source/MDVR-419.mp4", 3)]),
        );
        fs::create_dir(destination.join("mdvr-419")).expect("case-colliding directory must exist");
        let (state, transfer_id) = organization_state(record);
        assert_eq!(
            preview_organization(&state, &transfer_id),
            Err(VR_ORGANIZATION_CONFLICT)
        );
    }

    #[test]
    fn stale_plans_and_changed_files_never_reach_the_move_dispatcher() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let record = completed_organization_record(
            &destination,
            organization_source(vec![("Source/MDVR-419.mp4", 3)]),
        );
        let (state, transfer_id) = organization_state(record);
        let first = preview_organization(&state, &transfer_id).expect("first preview must succeed");
        preview_organization(&state, &transfer_id).expect("second preview must succeed");
        let mut dispatched = false;
        assert_eq!(
            apply_organization_with(
                &state,
                &fixture.path.join("downloads"),
                &first[0],
                |_, _| {
                    dispatched = true;
                    Ok(())
                },
            ),
            Err(VR_ORGANIZATION_STALE)
        );
        assert!(!dispatched, "obsolete preview reached mutation");

        let current =
            preview_organization(&state, &transfer_id).expect("current preview must succeed");
        let source_path = destination.join("Source/MDVR-419.mp4");
        fs::remove_file(&source_path).expect("old source must be removed");
        fs::write(&source_path, b"new").expect("replacement source must exist");
        assert_eq!(
            apply_organization_with(
                &state,
                &fixture.path.join("downloads"),
                &current[0],
                |_, _| {
                    dispatched = true;
                    Ok(())
                },
            ),
            Err(VR_ORGANIZATION_STALE)
        );
        assert!(!dispatched, "changed source reached mutation");
    }

    #[test]
    fn organization_plans_do_not_survive_application_state_restart() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let record = completed_organization_record(
            &destination,
            organization_source(vec![("Source/MDVR-419.mp4", 3)]),
        );
        let (state, transfer_id) = organization_state(record);
        let preview = preview_organization(&state, &transfer_id).expect("preview must succeed");
        let restarted = VrDownloadState::default();
        let mut dispatched = false;
        assert_eq!(
            apply_organization_with(
                &restarted,
                &fixture.path.join("downloads"),
                &preview[0],
                |_, _| {
                    dispatched = true;
                    Ok(())
                },
            ),
            Err(VR_ORGANIZATION_STALE)
        );
        assert!(!dispatched, "restarted application state dispatched a plan");
    }

    #[test]
    fn folder_change_and_dismissals_invalidate_native_owned_plans() {
        let fixture = FilesystemFixture::new();
        let destination = fixture.path.join("current");
        let replacement = fixture.path.join("replacement");
        fs::create_dir(&destination).expect("current folder must exist");
        fs::create_dir(&replacement).expect("replacement folder must exist");
        let destination = fs::canonicalize(destination).expect("current folder must canonicalize");
        let replacement = fs::canonicalize(replacement).expect("replacement must canonicalize");
        let record = completed_organization_record(
            &destination,
            organization_source(vec![("Source/MDVR-419.mp4", 3)]),
        );
        let (state, transfer_id) = organization_state(record);
        let preview = preview_organization(&state, &transfer_id).expect("preview must succeed");
        set_vr_folder(&state, &fixture.path.join("folder"), replacement)
            .expect("folder must change");
        assert_eq!(
            apply_organization_with(
                &state,
                &fixture.path.join("downloads"),
                &preview[0],
                |_, _| panic!("folder-stale plan dispatched"),
            ),
            Err(VR_ORGANIZATION_STALE)
        );

        set_vr_folder(&state, &fixture.path.join("folder"), destination)
            .expect("folder must restore");
        let preview = preview_organization(&state, &transfer_id).expect("preview must succeed");
        dismiss_organization(&state).expect("preview must dismiss");
        assert_eq!(
            apply_organization_with(
                &state,
                &fixture.path.join("downloads"),
                &preview[0],
                |_, _| panic!("dismissed preview dispatched"),
            ),
            Err(VR_ORGANIZATION_STALE)
        );

        let preview = preview_organization(&state, &transfer_id).expect("preview must succeed");
        dismiss_download(&state, &fixture.path.join("downloads"), &transfer_id)
            .expect("completed transfer must dismiss");
        assert_eq!(
            apply_organization_with(
                &state,
                &fixture.path.join("downloads"),
                &preview[0],
                |_, _| panic!("dismissed plan dispatched"),
            ),
            Err(VR_ORGANIZATION_STALE)
        );
    }

    #[test]
    fn rejects_noncompleted_old_folder_traversal_and_symlink_contexts() {
        let fixture = FilesystemFixture::new();
        let destination = fixture.path.join("current");
        let old_destination = fixture.path.join("old");
        fs::create_dir(&destination).expect("current folder must exist");
        fs::create_dir(&old_destination).expect("old folder must exist");
        let destination = fs::canonicalize(destination).expect("current folder must canonicalize");
        let old_destination =
            fs::canonicalize(old_destination).expect("old folder must canonicalize");

        let mut paused = completed_organization_record(
            &destination,
            organization_source(vec![("Source/MDVR-419.mp4", 3)]),
        );
        paused.state = TransferState::Paused;
        let (paused_state, paused_id) = organization_state(paused);
        assert_eq!(
            preview_organization(&paused_state, &paused_id),
            Err(VR_ORGANIZATION_INELIGIBLE)
        );

        let old = completed_organization_record(
            &old_destination,
            organization_source(vec![("Source/MDVR-419.mp4", 3)]),
        );
        let (old_state, old_id) = organization_state(old);
        old_state.0.lock().expect("state must lock").future_folder = Some(destination);
        assert_eq!(
            preview_organization(&old_state, &old_id),
            Err(VR_ORGANIZATION_INELIGIBLE)
        );

        let mut traversal = completed_organization_record(
            &old_destination,
            organization_source(vec![("Source/MDVR-419.mp4", 3)]),
        );
        traversal.current_paths[0] = "../outside.mp4".to_owned();
        let (traversal_state, traversal_id) = organization_state(traversal);
        assert_eq!(
            preview_organization(&traversal_state, &traversal_id),
            Err(VR_ORGANIZATION_STALE)
        );

        #[cfg(unix)]
        {
            let symlink_fixture = FilesystemFixture::new();
            let symlink_destination = symlink_fixture.path.join("current");
            let outside = symlink_fixture.path.join("outside");
            fs::create_dir(&symlink_destination).expect("current folder must exist");
            fs::create_dir(&outside).expect("outside folder must exist");
            let symlink_destination =
                fs::canonicalize(symlink_destination).expect("current folder must canonicalize");
            let record = completed_organization_record(
                &symlink_destination,
                organization_source(vec![("Source/MDVR-419.mp4", 3)]),
            );
            fs::remove_dir_all(symlink_destination.join("Source"))
                .expect("source parent must be replaced");
            std::os::unix::fs::symlink(&outside, symlink_destination.join("Source"))
                .expect("source symlink must exist");
            let (state, transfer_id) = organization_state(record);
            assert_eq!(
                preview_organization(&state, &transfer_id),
                Err(VR_ORGANIZATION_STALE)
            );

            let destination_symlink_fixture = FilesystemFixture::new();
            let destination = destination_symlink_fixture.path.join("current");
            let outside = destination_symlink_fixture.path.join("outside");
            fs::create_dir(&destination).expect("current folder must exist");
            fs::create_dir(&outside).expect("outside folder must exist");
            let destination =
                fs::canonicalize(destination).expect("current folder must canonicalize");
            let record = completed_organization_record(
                &destination,
                organization_source(vec![("Source/MDVR-419.mp4", 3)]),
            );
            std::os::unix::fs::symlink(&outside, destination.join("MDVR-419"))
                .expect("destination symlink must exist");
            let (state, transfer_id) = organization_state(record);
            assert_eq!(
                preview_organization(&state, &transfer_id),
                Err(VR_ORGANIZATION_CONFLICT)
            );
        }
    }

    #[test]
    fn injected_mid_operation_failure_restores_every_source_and_consumes_the_plan() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let record = completed_organization_record(
            &destination,
            organization_source(vec![
                ("Source/MDVR-419 Part 1.mp4", 3),
                ("Source/MDVR-419 Part 2.mkv", 4),
            ]),
        );
        let (state, transfer_id) = organization_state(record);
        let preview = preview_organization(&state, &transfer_id).expect("preview must succeed");
        let mut calls = 0;
        assert_eq!(
            apply_organization_with(
                &state,
                &fixture.path.join("downloads"),
                &preview[0],
                |source, target| {
                    calls += 1;
                    if calls == 2 {
                        Err(io::Error::other("injected second move failure"))
                    } else {
                        fs::rename(source, target)
                    }
                },
            ),
            Err(VR_ORGANIZATION_FAILED)
        );
        assert_eq!(calls, 3, "the first move was not rolled back exactly once");
        assert!(destination.join("Source/MDVR-419 Part 1.mp4").exists());
        assert!(destination.join("Source/MDVR-419 Part 2.mkv").exists());
        assert!(!destination.join("MDVR-419/MDVR-419 - Part 1.mp4").exists());
        let context = state.0.lock().expect("state must lock");
        let StoredTransfer::Valid(record) = &context.transfers[0] else {
            panic!("transfer must remain valid");
        };
        assert_eq!(record.organization_state, OrganizationState::None);
        assert_eq!(
            record.current_paths,
            ["Source/MDVR-419 Part 1.mp4", "Source/MDVR-419 Part 2.mkv"]
        );
        drop(context);
        assert_eq!(
            apply_organization_with(
                &state,
                &fixture.path.join("downloads"),
                &preview[0],
                |_, _| panic!("consumed plan dispatched twice"),
            ),
            Err(VR_ORGANIZATION_STALE)
        );
    }

    #[test]
    fn rollback_failure_persists_exact_moved_and_unmoved_paths_for_safe_relaunch() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let metainfo = selected_file_torrent();
        let infohash = hex_sha1(&metainfo[b"d4:info".len()..metainfo.len() - 1]);
        let source = revalidate_persisted_download_source(
            &metainfo,
            "MDVR-419",
            "【VR】 MDVR-419  Exact — 特別版",
            &infohash,
            &[0, 1],
        )
        .expect("multipart source must revalidate");
        let record = completed_organization_record(&destination, source);
        let (state, transfer_id) = organization_state(record);
        let persistence_path = fixture.path.join("downloads");
        let preview = preview_organization(&state, &transfer_id).expect("preview must succeed");
        let mut calls = 0;
        assert_eq!(
            apply_organization_with(&state, &persistence_path, &preview[0], |source, target| {
                calls += 1;
                if calls == 1 {
                    fs::rename(source, target)
                } else {
                    Err(io::Error::other("injected move and rollback failure"))
                }
            },),
            Err(VR_ORGANIZATION_FAILED)
        );
        assert_eq!(calls, 3);
        let context = state.0.lock().expect("state must lock");
        let StoredTransfer::Valid(record) = &context.transfers[0] else {
            panic!("transfer must remain valid");
        };
        assert_eq!(record.organization_state, OrganizationState::Attention);
        assert_eq!(
            record.current_paths,
            ["MDVR-419/MDVR-419 - Part  1.mkv", "Folder/特別版  B.mp4"]
        );
        drop(context);

        let restarted = VrDownloadState::default();
        restarted.0.lock().expect("state must lock").future_folder = Some(destination);
        let rows = tauri::async_runtime::block_on(load_downloads(
            &restarted,
            &persistence_path,
            &fixture.path.join("session"),
            &fixture.path.join("limit"),
        ))
        .expect("attention state must reload");
        assert_eq!(
            &rows[8..13],
            &["completed", "true", "attention", "MDVR-419/", "true"]
        );
        assert!(
            restarted
                .0
                .lock()
                .expect("state must lock")
                .session
                .is_none(),
            "attention completion restarted a native session"
        );
    }

    #[test]
    fn persistence_and_rollback_failures_recover_exact_paths_after_restart() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let metainfo = selected_file_torrent();
        let infohash = hex_sha1(&metainfo[b"d4:info".len()..metainfo.len() - 1]);
        let source = revalidate_persisted_download_source(
            &metainfo,
            "MDVR-419",
            "【VR】 MDVR-419  Exact — 特別版",
            &infohash,
            &[0, 1],
        )
        .expect("multipart source must revalidate");
        let record = completed_organization_record(&destination, source);
        let (state, transfer_id) = organization_state(record);
        let persistence_path = fixture.path.join("downloads");
        {
            let context = state.0.lock().expect("state must lock");
            write_persisted_transfers(&persistence_path, &context.transfers)
                .expect("original paths must persist before organization");
        }
        let original_persistence = fs::read(&persistence_path).expect("persistence must exist");
        let preview = preview_organization(&state, &transfer_id).expect("preview must succeed");
        let mut move_calls = 0;
        let mut persistence_calls = 0;

        assert_eq!(
            apply_organization_with_persistence(
                &state,
                &persistence_path,
                &preview[0],
                |source, target| {
                    move_calls += 1;
                    if move_calls == 4 {
                        Err(io::Error::other("injected rollback failure"))
                    } else {
                        fs::rename(source, target)
                    }
                },
                |_, _| {
                    persistence_calls += 1;
                    Err(VR_DOWNLOAD_PERSISTENCE_FAILED)
                },
            ),
            Err(VR_DOWNLOAD_PERSISTENCE_FAILED)
        );
        assert_eq!(move_calls, 4);
        assert_eq!(persistence_calls, 2);
        assert_eq!(
            fs::read(&persistence_path).expect("old persistence must remain readable"),
            original_persistence
        );
        assert!(destination.join("MDVR-419/MDVR-419 - Part  1.mkv").exists());
        assert!(destination.join("Folder/特別版  B.mp4").exists());
        let recovery_path = {
            let context = state.0.lock().expect("state must lock");
            let StoredTransfer::Valid(record) = &context.transfers[0] else {
                panic!("transfer must remain valid");
            };
            organization_recovery_path(record)
        };
        assert!(recovery_path.exists());
        fs::remove_file(&persistence_path).expect("old persistence must be removed");
        fs::create_dir(&persistence_path).expect("persistence must remain unavailable");

        let restarted = VrDownloadState::default();
        restarted.0.lock().expect("state must lock").future_folder = Some(destination.clone());
        let rows = tauri::async_runtime::block_on(load_downloads(
            &restarted,
            &persistence_path,
            &fixture.path.join("session"),
            &fixture.path.join("limit"),
        ))
        .expect("durable recovery must load");
        assert_eq!(
            &rows[8..13],
            &["completed", "true", "attention", "MDVR-419/", "true"]
        );
        let context = restarted.0.lock().expect("state must lock");
        let StoredTransfer::Valid(record) = &context.transfers[0] else {
            panic!("recovered transfer must remain valid");
        };
        assert_eq!(
            record.current_paths,
            ["MDVR-419/MDVR-419 - Part  1.mkv", "Folder/特別版  B.mp4"]
        );
        assert_eq!(record.organization_state, OrganizationState::Attention);
        assert!(organization_recovery_path(record).exists());
        drop(context);

        fs::remove_dir(&persistence_path).expect("persistence must become available");
        let second_restart = VrDownloadState::default();
        second_restart
            .0
            .lock()
            .expect("state must lock")
            .future_folder = Some(destination.clone());
        let rows = tauri::async_runtime::block_on(load_downloads(
            &second_restart,
            &persistence_path,
            &fixture.path.join("second-session"),
            &fixture.path.join("limit"),
        ))
        .expect("reconciled attention state must remain durable");
        assert_eq!(
            &rows[8..13],
            &["completed", "true", "attention", "MDVR-419/", "true"]
        );
        assert!(!recovery_path.exists());

        let third_restart = VrDownloadState::default();
        third_restart
            .0
            .lock()
            .expect("state must lock")
            .future_folder = Some(destination);
        let rows = tauri::async_runtime::block_on(load_downloads(
            &third_restart,
            &persistence_path,
            &fixture.path.join("third-session"),
            &fixture.path.join("limit"),
        ))
        .expect("persisted attention state must survive another restart");
        assert_eq!(
            &rows[8..13],
            &["completed", "true", "attention", "MDVR-419/", "true"]
        );
    }

    #[test]
    fn interruption_after_one_move_recovers_exact_moved_and_unmoved_paths() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let metainfo = selected_file_torrent();
        let infohash = hex_sha1(&metainfo[b"d4:info".len()..metainfo.len() - 1]);
        let source = revalidate_persisted_download_source(
            &metainfo,
            "MDVR-419",
            "【VR】 MDVR-419  Exact — 特別版",
            &infohash,
            &[0, 1],
        )
        .expect("multipart source must revalidate");
        let record = completed_organization_record(&destination, source);
        let media_contents = record
            .current_paths
            .iter()
            .map(|relative_path| {
                fs::read(destination.join(relative_path)).expect("media must remain readable")
            })
            .collect::<Vec<_>>();
        let (state, transfer_id) = organization_state(record);
        let preview = preview_organization(&state, &transfer_id).expect("preview must succeed");
        assert_eq!(preview[3], "2");
        let persistence_path = fixture.path.join("downloads");
        fs::create_dir(destination.join("MDVR-419")).expect("organization directory must exist");
        let recovery_path = {
            let context = state.0.lock().expect("state must lock");
            write_persisted_transfers(&persistence_path, &context.transfers)
                .expect("original transfer must persist");
            let StoredTransfer::Valid(record) = &context.transfers[0] else {
                panic!("transfer must remain valid");
            };
            write_organization_recovery(record, &record.current_paths, None)
                .expect("pre-mutation recovery must persist");
            organization_recovery_path(record)
        };
        let original_persistence =
            fs::read(&persistence_path).expect("main persistence must remain readable");
        let original_recovery =
            fs::read(&recovery_path).expect("recovery state must remain readable");

        let moved_path = destination.join("MDVR-419/MDVR-419 - Part  1.mkv");
        let unmoved_path = destination.join("Folder/特別版  B.mp4");
        fs::rename(destination.join("Folder/Part  1 — 映画.mkv"), &moved_path)
            .expect("first move must complete before the simulated interruption");
        assert_eq!(
            fs::read(&persistence_path).expect("main store must remain unchanged"),
            original_persistence
        );
        assert_eq!(
            fs::read(&recovery_path).expect("recovery store must remain unchanged"),
            original_recovery
        );

        let restarted = VrDownloadState::default();
        restarted.0.lock().expect("state must lock").future_folder = Some(destination);
        let rows = tauri::async_runtime::block_on(load_downloads(
            &restarted,
            &persistence_path,
            &fixture.path.join("interrupted-session"),
            &fixture.path.join("limit"),
        ))
        .expect("interrupted paths must recover on restart");
        assert_eq!(
            &rows[8..13],
            &["completed", "true", "attention", "MDVR-419/", "true"]
        );
        let context = restarted.0.lock().expect("state must lock");
        let StoredTransfer::Valid(record) = &context.transfers[0] else {
            panic!("recovered transfer must remain valid");
        };
        assert_eq!(
            record.current_paths,
            ["MDVR-419/MDVR-419 - Part  1.mkv", "Folder/特別版  B.mp4"]
        );
        assert!(
            context.session.is_none(),
            "recovery started a transfer session"
        );
        drop(context);
        assert_eq!(
            fs::read(moved_path).expect("moved media must remain at its exact path"),
            media_contents[0]
        );
        assert_eq!(
            fs::read(unmoved_path).expect("unmoved media must remain at its exact path"),
            media_contents[1]
        );
    }

    #[test]
    fn partial_recovery_successor_preserves_exact_paths_after_restart() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let metainfo = selected_file_torrent();
        let infohash = hex_sha1(&metainfo[b"d4:info".len()..metainfo.len() - 1]);
        let source = revalidate_persisted_download_source(
            &metainfo,
            "MDVR-419",
            "【VR】 MDVR-419  Exact — 特別版",
            &infohash,
            &[0, 1],
        )
        .expect("multipart source must revalidate");
        let record = completed_organization_record(&destination, source);
        let media_contents = record
            .current_paths
            .iter()
            .map(|relative_path| {
                fs::read(destination.join(relative_path)).expect("media must remain readable")
            })
            .collect::<Vec<_>>();
        let (state, transfer_id) = organization_state(record);
        let preview = preview_organization(&state, &transfer_id).expect("preview must succeed");
        assert_eq!(preview[3], "2");
        let persistence_path = fixture.path.join("downloads");
        fs::create_dir(destination.join("MDVR-419")).expect("organization directory must exist");
        let (recovery_path, successor_path) = {
            let context = state.0.lock().expect("state must lock");
            write_persisted_transfers(&persistence_path, &context.transfers)
                .expect("original transfer must persist");
            let StoredTransfer::Valid(record) = &context.transfers[0] else {
                panic!("transfer must remain valid");
            };
            write_organization_recovery(record, &record.current_paths, None)
                .expect("pre-mutation recovery must persist");
            (
                organization_recovery_path(record),
                organization_recovery_successor_path(record),
            )
        };
        let original_persistence =
            fs::read(&persistence_path).expect("main persistence must remain readable");
        let original_recovery =
            fs::read(&recovery_path).expect("recovery state must remain readable");

        let moved_path = destination.join("MDVR-419/MDVR-419 - Part  1.mkv");
        let unmoved_path = destination.join("Folder/特別版  B.mp4");
        fs::rename(destination.join("Folder/Part  1 — 映画.mkv"), &moved_path)
            .expect("first move must complete before the simulated interruption");
        let current_paths = vec![
            "MDVR-419/MDVR-419 - Part  1.mkv".to_owned(),
            "Folder/特別版  B.mp4".to_owned(),
        ];
        {
            let context = state.0.lock().expect("state must lock");
            let StoredTransfer::Valid(record) = &context.transfers[0] else {
                panic!("transfer must remain valid");
            };
            write_organization_recovery(record, &current_paths, Some(&record.current_paths))
                .expect("successor recovery must persist separately");
        }
        assert_eq!(
            fs::read(&recovery_path).expect("original recovery must remain readable"),
            original_recovery
        );
        let mut interrupted_successor = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&successor_path)
            .expect("successor must open for interrupted replacement");
        interrupted_successor
            .write_all(b"AUTO_VIDEO_VR_ORGANIZATION")
            .and_then(|()| interrupted_successor.sync_all())
            .expect("partial successor state must be deterministic");
        drop(interrupted_successor);
        assert_eq!(
            fs::read(&persistence_path).expect("main store must remain unchanged"),
            original_persistence
        );
        assert_eq!(
            fs::read(&recovery_path).expect("complete recovery must remain unchanged"),
            original_recovery
        );

        let restarted = VrDownloadState::default();
        restarted.0.lock().expect("state must lock").future_folder = Some(destination);
        let rows = tauri::async_runtime::block_on(load_downloads(
            &restarted,
            &persistence_path,
            &fixture.path.join("partial-successor-session"),
            &fixture.path.join("limit"),
        ))
        .expect("complete recovery must survive a partial successor");
        assert_eq!(
            &rows[8..13],
            &["completed", "true", "attention", "MDVR-419/", "true"]
        );
        let context = restarted.0.lock().expect("state must lock");
        let StoredTransfer::Valid(record) = &context.transfers[0] else {
            panic!("recovered transfer must remain valid");
        };
        assert_eq!(record.current_paths, current_paths);
        assert!(
            context.session.is_none(),
            "recovery started a transfer session"
        );
        drop(context);
        assert!(!recovery_path.exists());
        assert!(!successor_path.exists());
        assert_eq!(
            fs::read(moved_path).expect("moved media must remain at its exact path"),
            media_contents[0]
        );
        assert_eq!(
            fs::read(unmoved_path).expect("unmoved media must remain at its exact path"),
            media_contents[1]
        );
    }

    #[test]
    fn dismissing_recovered_terminal_transfer_survives_restart_without_moving_media() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let metainfo = selected_file_torrent();
        let infohash = hex_sha1(&metainfo[b"d4:info".len()..metainfo.len() - 1]);
        let source = revalidate_persisted_download_source(
            &metainfo,
            "MDVR-419",
            "【VR】 MDVR-419  Exact — 特別版",
            &infohash,
            &[0, 1],
        )
        .expect("multipart source must revalidate");
        let mut record = completed_organization_record(&destination, source);
        let original_first_path = destination.join(&record.current_paths[0]);
        let moved_first_path = destination.join("MDVR-419/MDVR-419 - Part  1.mkv");
        fs::create_dir(destination.join("MDVR-419")).expect("organization directory must exist");
        fs::rename(&original_first_path, &moved_first_path)
            .expect("first media file must be partially organized");
        record.current_paths[0] = "MDVR-419/MDVR-419 - Part  1.mkv".to_owned();
        record.organization_state = OrganizationState::Attention;
        let media = record
            .current_paths
            .iter()
            .map(|relative_path| {
                let path = destination.join(relative_path);
                let contents = fs::read(&path).expect("media must remain readable");
                (path, contents)
            })
            .collect::<Vec<_>>();
        let recovery_path = organization_recovery_path(&record);
        let successor_path = organization_recovery_successor_path(&record);
        let (state, transfer_id) = organization_state(record);
        let persistence_path = fixture.path.join("downloads");
        {
            let context = state.0.lock().expect("state must lock");
            write_persisted_transfers(&persistence_path, &context.transfers)
                .expect("attention transfer must persist");
            let StoredTransfer::Valid(record) = &context.transfers[0] else {
                panic!("transfer must remain valid");
            };
            write_organization_recovery(record, &record.current_paths, None)
                .expect("durable recovery must be created");
            write_organization_recovery(record, &record.current_paths, Some(&record.current_paths))
                .expect("durable successor must be created");
        }
        assert!(recovery_path.exists());
        assert!(successor_path.exists());

        dismiss_download(&state, &persistence_path, &transfer_id)
            .expect("recovered terminal row must dismiss");
        assert!(!recovery_path.exists());
        assert!(!successor_path.exists());
        for (path, contents) in &media {
            assert_eq!(
                fs::read(path).expect("dismissed media must remain at its exact path"),
                *contents
            );
        }

        for restart_index in 0..2 {
            let restarted = VrDownloadState::default();
            restarted.0.lock().expect("state must lock").future_folder = Some(destination.clone());
            let rows = tauri::async_runtime::block_on(load_downloads(
                &restarted,
                &persistence_path,
                &fixture
                    .path
                    .join(format!("dismissed-recovery-session-{restart_index}")),
                &fixture.path.join("limit"),
            ))
            .expect("downloads must reload after dismissal");
            assert!(rows.is_empty(), "dismissed recovery recreated its row");
            for (path, contents) in &media {
                assert_eq!(
                    fs::read(path).expect("reloaded media must remain at its exact path"),
                    *contents
                );
            }
        }
    }

    #[test]
    fn recovery_cleanup_failure_rejects_dismiss_and_retains_row_and_media_after_restart() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let mut record = completed_organization_record(&destination, persistable_fixture_source());
        record.organization_state = OrganizationState::Attention;
        let media_path = destination.join(&record.current_paths[0]);
        let media_contents = fs::read(&media_path).expect("media must remain readable");
        let recovery_path = organization_recovery_path(&record);
        let (state, transfer_id) = organization_state(record);
        let persistence_path = fixture.path.join("downloads");
        {
            let context = state.0.lock().expect("state must lock");
            write_persisted_transfers(&persistence_path, &context.transfers)
                .expect("attention transfer must persist");
            let StoredTransfer::Valid(record) = &context.transfers[0] else {
                panic!("transfer must remain valid");
            };
            write_organization_recovery(record, &record.current_paths, None)
                .expect("durable recovery must be created");
        }
        fs::remove_file(&recovery_path).expect("recovery file must be replaceable");
        fs::create_dir(&recovery_path).expect("cleanup failure must be deterministic");

        assert_eq!(
            dismiss_download(&state, &persistence_path, &transfer_id),
            Err(VR_DOWNLOAD_PERSISTENCE_FAILED)
        );
        {
            let context = state.0.lock().expect("state must lock");
            assert!(matches!(
                context.transfers.as_slice(),
                [StoredTransfer::Valid(record)] if record.transfer_id == transfer_id
            ));
        }
        assert_eq!(
            fs::read(&media_path).expect("failed dismissal must leave media at its exact path"),
            media_contents
        );

        let restarted = VrDownloadState::default();
        restarted.0.lock().expect("state must lock").future_folder = Some(destination);
        let rows = tauri::async_runtime::block_on(load_downloads(
            &restarted,
            &persistence_path,
            &fixture.path.join("failed-dismiss-session"),
            &fixture.path.join("limit"),
        ))
        .expect("retained row must reload after failed dismissal");
        assert_eq!(
            &rows[8..13],
            &["completed", "true", "attention", "MDVR-419/", "true"]
        );
        assert_eq!(
            fs::read(media_path).expect("reloaded media must remain at its exact path"),
            media_contents
        );
    }

    #[test]
    fn movie_persistence_and_rollback_failure_recovers_exact_paths_and_dismisses_durably() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let source = movie_organization_source(
            "Exact  Movie — 特別版",
            Some("1999-04-19"),
            "Different YTS title",
            &[
                ("Provider/Feature  A.mp4", 3),
                ("Provider/Feature  B.MKV", 4),
            ],
            &[0, 1],
        );
        let record = completed_movie_organization_record(&destination, source);
        let (state, transfer_id) = organization_state(record);
        let persistence_path = fixture.path.join("downloads");
        {
            let context = state.0.lock().expect("state must lock");
            write_persisted_transfers(&persistence_path, &context.transfers)
                .expect("original Movie paths must persist");
        }
        let preview = preview_organization(&state, &transfer_id).expect("preview must succeed");
        let mut move_calls = 0;
        assert_eq!(
            apply_organization_with_persistence(
                &state,
                &persistence_path,
                &preview[0],
                |source, destination| {
                    move_calls += 1;
                    if move_calls == 4 {
                        Err(io::Error::other("injected Movie rollback failure"))
                    } else {
                        fs::rename(source, destination)
                    }
                },
                |_, _| Err(VR_DOWNLOAD_PERSISTENCE_FAILED),
            ),
            Err(VR_DOWNLOAD_PERSISTENCE_FAILED)
        );
        assert_eq!(move_calls, 4);
        let moved_path = destination.join("Exact  Movie — 特別版 (1999)/Feature  A.mp4");
        let unmoved_path = destination.join("Provider/Feature  B.MKV");
        assert_eq!(
            fs::read(&moved_path).expect("moved Movie must remain"),
            vec![b'a'; 3]
        );
        assert_eq!(
            fs::read(&unmoved_path).expect("unmoved Movie must remain"),
            vec![b'b'; 4]
        );

        let restarted = VrDownloadState::default();
        configure_movie_download_folder(&restarted, Some(destination.clone()))
            .expect("Movies folder must restore");
        let rows = tauri::async_runtime::block_on(load_downloads(
            &restarted,
            &persistence_path,
            &fixture.path.join("movie-recovery-session"),
            &fixture.path.join("limit"),
        ))
        .expect("Movie attention recovery must load");
        assert_eq!(rows[1], "movie");
        assert_eq!(
            &rows[8..13],
            &[
                "completed",
                "true",
                "attention",
                "Exact  Movie — 特別版 (1999)/",
                "true",
            ]
        );
        let context = restarted.0.lock().expect("state must lock");
        let StoredTransfer::Valid(record) = &context.transfers[0] else {
            panic!("recovered Movie transfer must remain valid");
        };
        assert_eq!(
            record.current_paths,
            [
                "Exact  Movie — 特別版 (1999)/Feature  A.mp4",
                "Provider/Feature  B.MKV",
            ]
        );
        assert!(
            context.session.is_none(),
            "Movie recovery started a session"
        );
        drop(context);

        dismiss_download(&restarted, &persistence_path, &transfer_id)
            .expect("Movie attention row must dismiss");
        for restart_index in 0..2 {
            let dismissed = VrDownloadState::default();
            configure_movie_download_folder(&dismissed, Some(destination.clone()))
                .expect("Movies folder must restore after dismissal");
            let rows = tauri::async_runtime::block_on(load_downloads(
                &dismissed,
                &persistence_path,
                &fixture
                    .path
                    .join(format!("movie-recovery-dismissed-{restart_index}")),
                &fixture.path.join("limit"),
            ))
            .expect("dismissed Movie recovery must remain absent");
            assert!(rows.is_empty());
            assert_eq!(
                fs::read(&moved_path).expect("moved Movie must remain"),
                vec![b'a'; 3]
            );
            assert_eq!(
                fs::read(&unmoved_path).expect("unmoved Movie must remain"),
                vec![b'b'; 4]
            );
        }
    }

    #[test]
    fn interrupted_movie_move_reconstructs_exact_paths_from_the_durable_record() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let source = movie_organization_source(
            "Exact  Movie — 特別版",
            Some("1999-04-19"),
            "Different YTS title",
            &[
                ("Provider/Feature  A.mp4", 3),
                ("Provider/Feature  B.MKV", 4),
            ],
            &[0, 1],
        );
        let record = completed_movie_organization_record(&destination, source);
        let (state, transfer_id) = organization_state(record);
        let preview = preview_organization(&state, &transfer_id).expect("preview must succeed");
        assert_eq!(preview[3], "2");
        let persistence_path = fixture.path.join("downloads");
        fs::create_dir(destination.join("Exact  Movie — 特別版 (1999)"))
            .expect("canonical Movie directory must exist");
        let recovery_path = {
            let context = state.0.lock().expect("state must lock");
            write_persisted_transfers(&persistence_path, &context.transfers)
                .expect("original Movie transfer must persist");
            let StoredTransfer::Valid(record) = &context.transfers[0] else {
                panic!("Movie transfer must remain valid");
            };
            write_organization_recovery(record, &record.current_paths, None)
                .expect("pre-mutation Movie recovery must persist");
            organization_recovery_path(record)
        };
        let moved_path = destination.join("Exact  Movie — 特別版 (1999)/Feature  A.mp4");
        let unmoved_path = destination.join("Provider/Feature  B.MKV");
        fs::rename(destination.join("Provider/Feature  A.mp4"), &moved_path)
            .expect("first Movie move must complete before interruption");

        let restarted = VrDownloadState::default();
        configure_movie_download_folder(&restarted, Some(destination))
            .expect("Movies folder must restore");
        let rows = tauri::async_runtime::block_on(load_downloads(
            &restarted,
            &persistence_path,
            &fixture.path.join("movie-interrupted-session"),
            &fixture.path.join("limit"),
        ))
        .expect("interrupted Movie paths must recover");
        assert_eq!(rows[10], "attention");
        assert_eq!(&rows[11..13], &["Exact  Movie — 特別版 (1999)/", "true"]);
        assert!(!recovery_path.exists());
        assert_eq!(
            fs::read(moved_path).expect("moved Movie must remain"),
            vec![b'a'; 3]
        );
        assert_eq!(
            fs::read(unmoved_path).expect("unmoved Movie must remain"),
            vec![b'b'; 4]
        );
    }

    #[test]
    fn movie_recovery_cleanup_failure_rejects_dismiss_and_keeps_media() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let source = movie_organization_source(
            "Exact  Movie — 特別版",
            Some("1999-04-19"),
            "Different YTS title",
            &[("Provider/Feature.mp4", 3)],
            &[0],
        );
        let mut record = completed_movie_organization_record(&destination, source);
        record.organization_state = OrganizationState::Attention;
        let media_path = destination.join("Provider/Feature.mp4");
        let recovery_path = organization_recovery_path(&record);
        let (state, transfer_id) = organization_state(record);
        let persistence_path = fixture.path.join("downloads");
        {
            let context = state.0.lock().expect("state must lock");
            write_persisted_transfers(&persistence_path, &context.transfers)
                .expect("attention Movie transfer must persist");
            let StoredTransfer::Valid(record) = &context.transfers[0] else {
                panic!("Movie transfer must remain valid");
            };
            write_organization_recovery(record, &record.current_paths, None)
                .expect("Movie recovery must persist");
        }
        fs::remove_file(&recovery_path).expect("recovery file must be replaceable");
        fs::create_dir(&recovery_path).expect("cleanup failure must be deterministic");

        assert_eq!(
            dismiss_download(&state, &persistence_path, &transfer_id),
            Err(VR_DOWNLOAD_PERSISTENCE_FAILED)
        );
        assert_eq!(
            fs::read(media_path).expect("failed dismiss must retain media"),
            vec![b'a'; 3]
        );
        assert!(matches!(
            state.0.lock().expect("state must lock").transfers.as_slice(),
            [StoredTransfer::Valid(record)] if record.transfer_id == transfer_id
        ));
    }

    #[test]
    fn adult_persistence_and_rollback_failure_recovers_and_dismisses_without_moving_media_again() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let record =
            completed_adult_organization_record(&destination, persistable_adult_fixture_source());
        let (state, transfer_id) = organization_state(record);
        let persistence_path = fixture.path.join("downloads");
        {
            let context = state.0.lock().expect("state must lock");
            write_persisted_transfers(&persistence_path, &context.transfers)
                .expect("original Adult paths must persist");
        }
        let preview = preview_organization(&state, &transfer_id).expect("preview must succeed");
        let mut move_calls = 0;
        assert_eq!(
            apply_organization_with_persistence(
                &state,
                &persistence_path,
                &preview[0],
                |source, destination| {
                    move_calls += 1;
                    if move_calls == 1 {
                        fs::rename(source, destination)
                    } else {
                        Err(io::Error::other("injected Adult rollback failure"))
                    }
                },
                |_, _| Err(VR_DOWNLOAD_PERSISTENCE_FAILED),
            ),
            Err(VR_DOWNLOAD_PERSISTENCE_FAILED)
        );
        assert_eq!(move_calls, 2);
        let organized_file = destination.join("ADLT-123/ADLT-123.mp4");
        assert_eq!(
            fs::read(&organized_file).expect("moved Adult media must remain readable"),
            vec![b'a'; 5]
        );

        let restarted = VrDownloadState::default();
        configure_adult_download_folder(&restarted, Some(destination.clone()))
            .expect("Adult folder must restore");
        let rows = tauri::async_runtime::block_on(load_downloads(
            &restarted,
            &persistence_path,
            &fixture.path.join("adult-recovery-session"),
            &fixture.path.join("limit"),
        ))
        .expect("Adult attention recovery must load");
        assert_eq!(rows[1], "adult");
        assert_eq!(
            &rows[8..13],
            &["completed", "true", "attention", "ADLT-123/", "true"]
        );

        dismiss_download(&restarted, &persistence_path, &transfer_id)
            .expect("Adult recovery must dismiss");
        let dismissed_restart = VrDownloadState::default();
        configure_adult_download_folder(&dismissed_restart, Some(destination))
            .expect("Adult folder must restore after dismissal");
        let rows = tauri::async_runtime::block_on(load_downloads(
            &dismissed_restart,
            &persistence_path,
            &fixture.path.join("adult-dismissed-session"),
            &fixture.path.join("limit"),
        ))
        .expect("dismissed Adult recovery must remain absent");
        assert!(
            rows.is_empty(),
            "dismissed Adult recovery recreated its row"
        );
        assert_eq!(
            fs::read(organized_file).expect("dismissed Adult media must remain in place"),
            vec![b'a'; 5]
        );
    }

    #[test]
    fn organization_recovery_loading_is_category_isolated() {
        let fixture = FilesystemFixture::new();
        let adult_destination = fixture.path.join("Adult");
        let movie_destination = fixture.path.join("Movies");
        let tv_destination = fixture.path.join("TV");
        let vr_destination = fixture.path.join("VR");
        fs::create_dir(&adult_destination).expect("Adult destination must exist");
        fs::create_dir(&movie_destination).expect("Movies destination must exist");
        fs::create_dir(&tv_destination).expect("TV destination must exist");
        fs::create_dir(&vr_destination).expect("VR destination must exist");
        let adult_destination =
            fs::canonicalize(adult_destination).expect("Adult destination must canonicalize");
        let movie_destination =
            fs::canonicalize(movie_destination).expect("Movies destination must canonicalize");
        let tv_destination =
            fs::canonicalize(tv_destination).expect("TV destination must canonicalize");
        let vr_destination =
            fs::canonicalize(vr_destination).expect("VR destination must canonicalize");
        let adult_record = completed_adult_organization_record(
            &adult_destination,
            persistable_adult_fixture_source(),
        );
        let vr_record =
            completed_organization_record(&vr_destination, persistable_fixture_source());
        let movie_record = completed_movie_organization_record(
            &movie_destination,
            movie_organization_source(
                "Exact Movie",
                Some("1999-04-19"),
                "Provider title",
                &[("Provider/Feature.mp4", 3)],
                &[0],
            ),
        );
        let tv_record = completed_tv_organization_record(
            &tv_destination,
            tv_download_source(&[("Provider/Episode.mp4", 3)], &[0]),
        );
        write_organization_recovery(&adult_record, &adult_record.current_paths, None)
            .expect("Adult recovery must persist");
        write_organization_recovery(&movie_record, &movie_record.current_paths, None)
            .expect("Movie recovery must persist");
        write_organization_recovery(&tv_record, &tv_record.current_paths, None)
            .expect("TV recovery must persist");
        write_organization_recovery(&vr_record, &vr_record.current_paths, None)
            .expect("VR recovery must persist");
        let persistence_path = fixture.path.join("downloads");

        let swapped = VrDownloadState::default();
        {
            let mut context = swapped.0.lock().expect("state must lock");
            context.adult_future_folder = Some(tv_destination.clone());
            context.movie_future_folder = Some(adult_destination.clone());
            context.tv_future_folder = Some(vr_destination.clone());
            context.future_folder = Some(movie_destination.clone());
        }
        let rows = tauri::async_runtime::block_on(load_downloads(
            &swapped,
            &persistence_path,
            &fixture.path.join("swapped-session"),
            &fixture.path.join("limit"),
        ))
        .expect("swapped recovery folders must load safely");
        assert!(rows.is_empty(), "cross-category recovery was loaded");

        let current = VrDownloadState::default();
        {
            let mut context = current.0.lock().expect("state must lock");
            context.adult_future_folder = Some(adult_destination);
            context.movie_future_folder = Some(movie_destination);
            context.tv_future_folder = Some(tv_destination);
            context.future_folder = Some(vr_destination);
        }
        let rows = tauri::async_runtime::block_on(load_downloads(
            &current,
            &persistence_path,
            &fixture.path.join("current-session"),
            &fixture.path.join("limit"),
        ))
        .expect("category-matched recoveries must load");
        assert_eq!(rows.len(), 64);
        assert_eq!(rows[1], "vr");
        assert_eq!(rows[17], "adult");
        assert_eq!(rows[33], "movie");
        assert_eq!(rows[49], "tv");
    }

    #[test]
    fn legacy_recovery_header_remains_vr_only() {
        let fixture = FilesystemFixture::new();
        let vr_destination = fixture.path.join("VR");
        let adult_destination = fixture.path.join("Adult");
        fs::create_dir(&vr_destination).expect("VR destination must exist");
        fs::create_dir(&adult_destination).expect("Adult destination must exist");
        let vr_destination =
            fs::canonicalize(vr_destination).expect("VR destination must canonicalize");
        let adult_destination =
            fs::canonicalize(adult_destination).expect("Adult destination must canonicalize");
        let vr_record =
            completed_organization_record(&vr_destination, persistable_fixture_source());
        let adult_record = completed_adult_organization_record(
            &adult_destination,
            persistable_adult_fixture_source(),
        );

        for (record, expected_category) in [
            (&vr_record, Some(TransferCategory::Vr)),
            (&adult_record, None),
        ] {
            let current = encoded_organization_recovery(record, &record.current_paths)
                .expect("current recovery must encode");
            let body = current
                .strip_prefix(ORGANIZATION_RECOVERY_HEADER)
                .expect("current header must exist");
            let mut legacy = LEGACY_ORGANIZATION_RECOVERY_HEADER.to_vec();
            legacy.extend_from_slice(body);
            let path = organization_recovery_path(record);
            fs::write(&path, legacy).expect("legacy recovery must persist");

            assert_eq!(
                read_organization_recovery_file(&path).map(|record| record.category),
                expected_category
            );
        }
    }

    #[test]
    fn persists_replaces_and_clears_the_aggregate_download_limit() {
        let fixture = FilesystemFixture::new();
        let limit_path = fixture.path.join("download-limit");
        let state = VrDownloadState::default();
        let mut applied = Vec::new();

        assert_eq!(
            load_download_limit_with(&state, &limit_path, |_, bytes_per_second| {
                applied.push(bytes_per_second.map(NonZeroU32::get));
                Ok(())
            }),
            Ok(vec!["unlimited".to_owned()])
        );
        assert_eq!(
            save_download_limit_with(&state, &limit_path, Some("8"), |_, bytes_per_second| {
                applied.push(bytes_per_second.map(NonZeroU32::get));
                Ok(())
            },),
            Ok(vec!["limited".to_owned(), "8".to_owned()])
        );
        assert_eq!(
            fs::read_to_string(&limit_path).expect("finite limit must persist"),
            "8\n"
        );
        let restarted = VrDownloadState::default();
        assert_eq!(
            load_download_limit(&restarted, &limit_path),
            Ok(vec!["limited".to_owned(), "8".to_owned()])
        );
        assert_eq!(
            save_download_limit_with(&state, &limit_path, None, |_, bytes_per_second| {
                applied.push(bytes_per_second.map(NonZeroU32::get));
                Ok(())
            }),
            Ok(vec!["unlimited".to_owned()])
        );
        assert_eq!(applied, vec![None, Some(8 * BYTES_PER_MIB), None]);
        assert_eq!(
            fs::read_to_string(&limit_path).expect("unlimited state must persist"),
            DOWNLOAD_LIMIT_UNLIMITED
        );
        assert_eq!(
            load_download_limit(&VrDownloadState::default(), &limit_path),
            Ok(vec!["unlimited".to_owned()])
        );
    }

    #[test]
    fn rejects_invalid_download_limits_before_engine_application() {
        for invalid in ["", "0", "01", "-1", "+1", "1.5", "4096", "4294967296"] {
            let fixture = FilesystemFixture::new();
            let limit_path = fixture.path.join("download-limit");
            let state = VrDownloadState::default();
            load_download_limit(&state, &limit_path).expect("unlimited state must load");
            let mut applied = false;

            assert_eq!(
                save_download_limit_with(&state, &limit_path, Some(invalid), |_, _| {
                    applied = true;
                    Ok(())
                },),
                Err(VR_DOWNLOAD_LIMIT_INVALID),
                "{invalid:?} was accepted",
            );
            assert!(!applied, "{invalid:?} reached the engine");
            assert!(!limit_path.exists(), "{invalid:?} reached persistence");
        }
    }

    #[test]
    fn live_limit_replacement_preserves_transfer_identity_and_lifecycle() {
        let fixture = FilesystemFixture::new();
        let limit_path = fixture.path.join("download-limit");
        let state = VrDownloadState::default();
        load_download_limit(&state, &limit_path).expect("unlimited state must load");
        tauri::async_runtime::block_on(session_for(&state, &fixture.path.join("session")))
            .expect("local session must start");
        let destination = fixture.path.join("destination");
        let record = transfer_from_source(
            TransferCategory::Vr,
            fixture_source(),
            destination.clone(),
            TransferState::Paused,
        );
        let expected_identity = (
            record.transfer_id.clone(),
            record.infohash.clone(),
            record.selected_file_ids(),
            record.destination.clone(),
            record.downloaded_bytes,
            record.state,
        );
        state
            .0
            .lock()
            .expect("state must lock")
            .transfers
            .push(StoredTransfer::Valid(record));
        let mut applied = None;

        assert_eq!(
            save_download_limit_with(
                &state,
                &limit_path,
                Some("4"),
                |session, bytes_per_second| {
                    assert!(session.is_some());
                    applied = bytes_per_second.map(NonZeroU32::get);
                    Ok(())
                },
            ),
            Ok(vec!["limited".to_owned(), "4".to_owned()])
        );
        assert_eq!(applied, Some(4 * BYTES_PER_MIB));
        let context = state.0.lock().expect("state must lock");
        let StoredTransfer::Valid(record) = &context.transfers[0] else {
            panic!("transfer must remain valid");
        };
        assert_eq!(
            (
                record.transfer_id.clone(),
                record.infohash.clone(),
                record.selected_file_ids(),
                record.destination.clone(),
                record.downloaded_bytes,
                record.state,
            ),
            expected_identity
        );
    }

    #[test]
    fn limit_failures_preserve_the_previous_setting_and_block_unsafe_restart() {
        let fixture = FilesystemFixture::new();
        let limit_path = fixture.path.join("download-limit");
        let state = VrDownloadState::default();
        load_download_limit(&state, &limit_path).expect("unlimited state must load");
        assert_eq!(
            save_download_limit_with(&state, &limit_path, Some("2"), |_, _| Err(())),
            Err(VR_DOWNLOAD_LIMIT_APPLY_FAILED)
        );
        assert!(!limit_path.exists());
        assert_eq!(
            state.0.lock().expect("state must lock").download_limit,
            DownloadLimitState::Loaded(None)
        );

        let blocking_file = fixture.path.join("blocking-file");
        fs::write(&blocking_file, b"not a directory").expect("blocking file must exist");
        let mut applied = Vec::new();
        assert_eq!(
            save_download_limit_with(
                &state,
                &blocking_file.join("download-limit"),
                Some("2"),
                |_, bytes_per_second| {
                    applied.push(bytes_per_second.map(NonZeroU32::get));
                    Ok(())
                },
            ),
            Err(VR_DOWNLOAD_LIMIT_STORAGE_FAILED)
        );
        assert_eq!(applied, vec![Some(2 * BYTES_PER_MIB), None]);
        assert_eq!(
            state.0.lock().expect("state must lock").download_limit,
            DownloadLimitState::Loaded(None)
        );

        fs::write(&limit_path, b"2\n").expect("finite restart limit must persist");
        let apply_failure_state = VrDownloadState::default();
        assert_eq!(
            load_download_limit_with(&apply_failure_state, &limit_path, |_, _| Err(())),
            Err(VR_DOWNLOAD_LIMIT_APPLY_FAILED)
        );
        let context = apply_failure_state.0.lock().expect("state must lock");
        assert_eq!(context.download_limit, DownloadLimitState::Unloaded);
        assert!(context.session.is_none());
        drop(context);

        fs::write(&limit_path, b"1.5\n").expect("invalid limit must persist for the fixture");
        let restart_state = VrDownloadState::default();
        assert_eq!(
            tauri::async_runtime::block_on(load_downloads(
                &restart_state,
                &fixture.path.join("downloads"),
                &fixture.path.join("restart-session"),
                &limit_path,
            )),
            Err(VR_DOWNLOAD_LIMIT_INVALID)
        );
        let context = restart_state.0.lock().expect("state must lock");
        assert_eq!(context.download_limit, DownloadLimitState::Unloaded);
        assert!(!context.transfers_loaded);
        assert!(context.session.is_none());
    }

    #[test]
    fn finite_limit_is_present_in_session_options_before_session_creation() {
        let limit = NonZeroU32::new(3).expect("finite limit must be nonzero");
        let options = session_options(Some(limit));
        assert_eq!(
            options.ratelimits.download_bps.map(NonZeroU32::get),
            Some(3 * BYTES_PER_MIB)
        );
        assert_eq!(options.ratelimits.upload_bps, None);
    }

    fn push_bencoded_text(encoded: &mut Vec<u8>, value: &str) {
        encoded.extend_from_slice(value.len().to_string().as_bytes());
        encoded.push(b':');
        encoded.extend_from_slice(value.as_bytes());
    }

    fn selected_file_torrent() -> Vec<u8> {
        let mut encoded = b"d4:infod5:filesl".to_vec();
        encoded.extend_from_slice(b"d6:lengthi3e4:pathl");
        push_bencoded_text(&mut encoded, "Folder");
        push_bencoded_text(&mut encoded, "Part  1 — 映画.mkv");
        encoded.extend_from_slice(b"eed6:lengthi7e4:pathl");
        push_bencoded_text(&mut encoded, "Folder");
        push_bencoded_text(&mut encoded, "特別版  B.mp4");
        encoded.extend_from_slice(b"e10:path.utf-8l");
        push_bencoded_text(&mut encoded, "Folder");
        push_bencoded_text(&mut encoded, "特別版  B.mp4");
        encoded.extend_from_slice(b"eee4:name");
        push_bencoded_text(&mut encoded, "VR  — 作品");
        encoded.extend_from_slice(b"12:piece lengthi16384e6:pieces20:");
        encoded.extend_from_slice(
            &decode_hex(hex_sha1(b"abc1234567").as_bytes()).expect("piece hash must decode"),
        );
        encoded.extend_from_slice(b"ee");
        encoded
    }

    fn completed_file_torrent(contents: &[u8]) -> Vec<u8> {
        let mut encoded = b"d4:infod6:lengthi".to_vec();
        encoded.extend_from_slice(contents.len().to_string().as_bytes());
        encoded.extend_from_slice(b"e4:name9:Movie.mp412:piece lengthi16384e6:pieces20:");
        encoded.extend_from_slice(
            &decode_hex(hex_sha1(contents).as_bytes()).expect("piece hash must decode"),
        );
        encoded.extend_from_slice(b"ee");
        encoded
    }

    fn completed_selected_boundary_record_for_category(
        fixture: &FilesystemFixture,
        category: TransferCategory,
        boundary_bytes: Option<&[u8]>,
    ) -> TransferRecord {
        let destination = fixture
            .path
            .join(format!("{} — retained boundary", category.as_str()));
        fs::create_dir_all(destination.join("Folder")).expect("selected file parent must exist");
        let destination = fs::canonicalize(destination).expect("destination must canonicalize");
        let metainfo = selected_file_torrent();
        let infohash = hex_sha1(&metainfo[b"d4:info".len()..metainfo.len() - 1]);
        let source = match category {
            TransferCategory::Tv => {
                let identity = TvDownloadIdentity {
                    tmdb_tv_id: 701,
                    show_name: "Exact  Show — 特別版".to_owned(),
                    provider_season_id: 9001,
                    season_number: 2,
                    provider_episode_id: 9103,
                    episode_number: 3,
                    episode_name: "第三話  —  Exact Episode".to_owned(),
                    imdb_id: "tt0123456".to_owned(),
                    provider_item_id: "1001".to_owned(),
                    category: "205".to_owned(),
                    release_name: "Exact  Show — 特別版.S02E03+720p.第三話".to_owned(),
                    infohash: infohash.clone(),
                };
                revalidate_persisted_tv_download_source(&metainfo, &identity, &infohash, &[1])
                    .expect("TV selected boundary fixture must revalidate")
            }
            TransferCategory::Vr => revalidate_persisted_download_source(
                &metainfo,
                "MDVR-419",
                "【VR】 MDVR-419  Exact — 特別版",
                &infohash,
                &[1],
            )
            .expect("VR selected boundary fixture must revalidate"),
            TransferCategory::Adult | TransferCategory::Movie => {
                panic!("retained boundary fixture supports VR and TV")
            }
        };
        let mut record =
            transfer_from_source(category, source, destination, TransferState::Downloading);
        fs::write(record.destination.join("Folder/特別版  B.mp4"), b"1234567")
            .expect("completed selected file must exist");
        record.fingerprints = capture_fingerprints(&record).expect("fingerprint must resolve");
        record.downloaded_bytes = 7;
        if let Some(boundary_bytes) = boundary_bytes {
            let storage = SelectedFileStorage {
                destination: record.destination.clone(),
                selected_files: Arc::new(BTreeMap::new()),
                boundary_segments: record.boundary_segments.clone(),
                resume: true,
                slots: vec![SelectedStorageSlot::new(None)],
            };
            storage
                .pwrite_all(0, 0, boundary_bytes)
                .expect("boundary bytes must be retained");
        }
        record
    }

    fn completed_selected_boundary_record(
        fixture: &FilesystemFixture,
        boundary_bytes: Option<&[u8]>,
    ) -> TransferRecord {
        completed_selected_boundary_record_for_category(
            fixture,
            TransferCategory::Vr,
            boundary_bytes,
        )
    }

    #[test]
    fn persists_loads_changes_and_clears_the_future_vr_folder() {
        let fixture = FilesystemFixture::new();
        let config_path = fixture.path.join("vr-folder");
        let first = fixture.path.join("VR — 一");
        let second = fixture.path.join("VR — 二");
        fs::create_dir_all(&first).expect("first folder must exist");
        fs::create_dir_all(&second).expect("second folder must exist");
        let state = VrDownloadState::default();

        assert_eq!(
            load_vr_folder_with(&state, &config_path),
            Ok(vec!["unconfigured".to_owned()])
        );
        let first = fs::canonicalize(first).expect("first folder must canonicalize");
        assert_eq!(
            set_vr_folder(&state, &config_path, first.clone()),
            Ok(first.to_string_lossy().into_owned())
        );
        assert_eq!(
            load_vr_folder_with(&state, &config_path),
            Ok(vec![
                "ready".to_owned(),
                first.to_string_lossy().into_owned()
            ])
        );
        state
            .0
            .lock()
            .expect("state must lock")
            .transfers
            .push(StoredTransfer::Valid(transfer_from_source(
                TransferCategory::Vr,
                fixture_source(),
                first.clone(),
                TransferState::Cancelled,
            )));
        let second = fs::canonicalize(second).expect("second folder must canonicalize");
        set_vr_folder(&state, &config_path, second.clone()).expect("folder must change");
        assert_eq!(load_vr_folder_file(&config_path), Ok(Some(second)));
        let context = state.0.lock().expect("state must lock");
        let StoredTransfer::Valid(existing) = &context.transfers[0] else {
            panic!("existing transfer must remain valid");
        };
        assert_eq!(existing.destination, first);
        drop(context);
        clear_vr_folder(&state, &config_path).expect("folder must clear");
        assert_eq!(load_vr_folder_file(&config_path), Ok(None));
    }

    #[test]
    fn revalidates_the_same_persisted_future_folder_after_it_is_restored() {
        let fixture = FilesystemFixture::new();
        let config_path = fixture.path.join("vr-folder");
        let missing = fs::canonicalize(&fixture.path)
            .expect("fixture must canonicalize")
            .join("missing");
        save_vr_folder_file(&config_path, &missing).expect("folder must persist");
        let state = VrDownloadState::default();
        assert_eq!(
            load_vr_folder_with(&state, &config_path),
            Ok(vec![
                "unavailable".to_owned(),
                missing.to_string_lossy().into_owned()
            ])
        );
        fs::create_dir(&missing).expect("persisted folder must be restored");
        assert_eq!(
            load_vr_folder_with(&state, &config_path),
            Ok(vec![
                "ready".to_owned(),
                missing.to_string_lossy().into_owned()
            ])
        );
    }

    #[test]
    fn rejects_existing_selected_targets_and_symlinked_parents() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let source = fixture_source();
        let target = destination.join("Folder/Part  1 — 映画.mkv");
        fs::create_dir_all(target.parent().expect("target parent")).expect("parent must exist");
        fs::write(&target, b"unrelated").expect("conflicting file must exist");
        assert_eq!(
            validate_new_targets(&destination, &source.selected_files),
            Err(VR_DOWNLOAD_DESTINATION_CONFLICT)
        );

        #[cfg(unix)]
        {
            fs::remove_dir_all(destination.join("Folder")).expect("folder must clear");
            std::os::unix::fs::symlink(fixture.path.join("outside"), destination.join("Folder"))
                .expect("symlink must be created");
            assert_eq!(
                validate_new_targets(&destination, &source.selected_files),
                Err(VR_DOWNLOAD_DESTINATION_CONFLICT)
            );
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_fingerprint_tracks_file_identity_while_content_changes() {
        let fixture = FilesystemFixture::new();
        let first = fixture.path.join("first.partial");
        let second = fixture.path.join("second.partial");
        fs::write(&first, b"initial").expect("first partial file must exist");
        fs::write(&second, b"initial").expect("second partial file must exist");
        let first_identity = file_fingerprint(&first).expect("first identity must resolve");

        assert_ne!(
            first_identity,
            file_fingerprint(&second).expect("second identity must resolve")
        );
        fs::write(&first, b"downloaded content")
            .expect("the same partial file must remain writable");
        assert_eq!(
            first_identity,
            file_fingerprint(&first).expect("updated identity must resolve")
        );
    }

    #[test]
    fn transfer_identity_binds_metainfo_release_selection_and_destination() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let source = fixture_source();
        let identity = transfer_identity(TransferCategory::Vr, &source, &destination);
        assert_eq!(identity.len(), 40);

        let mut changed_release = source.clone();
        changed_release.release_name.push(' ');
        assert_ne!(
            identity,
            transfer_identity(TransferCategory::Vr, &changed_release, &destination)
        );
        let mut changed_selection = source.clone();
        changed_selection.selected_files[0].file_id = 1;
        assert_ne!(
            identity,
            transfer_identity(TransferCategory::Vr, &changed_selection, &destination)
        );
        let mut changed_metainfo = source.clone();
        changed_metainfo.bytes.push(b'!');
        assert_ne!(
            identity,
            transfer_identity(TransferCategory::Vr, &changed_metainfo, &destination)
        );
        let other_destination = destination.join("other");
        fs::create_dir(&other_destination).expect("other destination must exist");
        assert_ne!(
            identity,
            transfer_identity(TransferCategory::Vr, &source, &other_destination)
        );
        assert_ne!(
            identity,
            transfer_identity(TransferCategory::Adult, &source, &destination)
        );
    }

    #[test]
    fn tv_transfer_persistence_binds_and_restores_the_complete_episode_identity_chain() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let source =
            tv_download_source(&[("Show.S02E03.mkv", 5), ("Extras/予告  編.mp4", 7)], &[1]);
        let identity = transfer_identity(TransferCategory::Tv, &source, &destination);
        let mut record = transfer_from_source(
            TransferCategory::Tv,
            source.clone(),
            destination.clone(),
            TransferState::Cancelled,
        );
        fs::create_dir_all(destination.join("Extras")).expect("selected parent must exist");
        fs::write(destination.join("Extras/予告  編.mp4"), b"1234567")
            .expect("selected TV media must exist");
        record.fingerprints = capture_fingerprints(&record).expect("fingerprint must resolve");
        let encoded = encode_transfer(&record).expect("TV transfer must encode");
        let parsed = parse_transfer_line(&encoded, false).expect("exact TV transfer must parse");
        let restored = parsed
            .tv_identity
            .as_deref()
            .expect("TV identity must remain durable");
        assert_eq!(restored.tmdb_tv_id, 701);
        assert_eq!(restored.show_name, "Exact  Show — 特別版");
        assert_eq!(restored.provider_season_id, 9001);
        assert_eq!(restored.season_number, 2);
        assert_eq!(restored.provider_episode_id, 9103);
        assert_eq!(restored.episode_number, 3);
        assert_eq!(restored.episode_name, "第三話  —  Exact Episode");
        assert_eq!(restored.imdb_id, "tt0123456");
        assert_eq!(restored.provider_item_id, "1001");
        assert_eq!(restored.category, "205");
        assert_eq!(restored.release_name, source.release_name);
        assert_eq!(restored.infohash, source.infohash);
        assert_eq!(parsed.selected_files, source.selected_files);
        assert_eq!(parsed.destination, destination);
        assert_eq!(parsed.transfer_id, identity);

        let mut changed_source = source.clone();
        changed_source
            .tv_identity
            .as_mut()
            .expect("TV identity must exist")
            .provider_episode_id += 1;
        assert_ne!(
            identity,
            transfer_identity(TransferCategory::Tv, &changed_source, &destination)
        );
        let mut fabricated_record = record;
        fabricated_record
            .tv_identity
            .as_mut()
            .expect("TV identity must exist")
            .provider_item_id = "1002".to_owned();
        assert!(parse_transfer_line(
            &encode_transfer(&fabricated_record).expect("fabricated row must encode"),
            false,
        )
        .is_none());
    }

    #[test]
    fn migrates_legacy_vr_records_without_reclassification_or_identity_reset() {
        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let source = persistable_fixture_source();
        let mut record = transfer_from_source(
            TransferCategory::Vr,
            source.clone(),
            destination.clone(),
            TransferState::Cancelled,
        );
        record.transfer_id = legacy_vr_transfer_identity(&source, &destination);
        fs::write(destination.join("Movie  A.mp4"), b"abcde")
            .expect("legacy selected file must exist");
        record.fingerprints = capture_fingerprints(&record).expect("fingerprint must resolve");
        let legacy_transfer_id = record.transfer_id.clone();
        let mut legacy_line = encode_transfer(&record).expect("legacy record must encode");
        let category_separator = legacy_line
            .iter()
            .rposition(|byte| *byte == b'\t')
            .expect("encoded category separator must exist");
        legacy_line.truncate(category_separator);
        let persistence_path = fixture.path.join("downloads");
        let mut bytes = LEGACY_PERSISTENCE_HEADER.to_vec();
        bytes.extend_from_slice(&legacy_line);
        bytes.push(b'\n');
        fs::write(&persistence_path, bytes).expect("legacy record must persist");

        let state = VrDownloadState::default();
        state.0.lock().expect("state must lock").future_folder = Some(destination);
        let rows = tauri::async_runtime::block_on(load_downloads(
            &state,
            &persistence_path,
            &fixture.path.join("session"),
            &fixture.path.join("limit"),
        ))
        .expect("legacy VR record must migrate");
        assert_eq!(rows[0], legacy_transfer_id);
        assert_eq!(rows[1], "vr");
        assert_eq!(rows[8], "cancelled");
        let migrated = fs::read(&persistence_path).expect("migrated record must persist");
        assert!(migrated.starts_with(PERSISTENCE_HEADER));
        assert!(migrated.ends_with(b"\tvr\n"));
        let reloaded = read_persisted_transfers(&persistence_path)
            .expect("migrated VR record must remain readable");
        let StoredTransfer::Valid(reloaded_record) = &reloaded[0] else {
            panic!("migrated VR record must remain valid");
        };
        assert_eq!(reloaded_record.transfer_id, legacy_transfer_id);
        assert_eq!(reloaded_record.category, TransferCategory::Vr);
    }

    #[test]
    fn damaged_v2_adult_categories_stay_unknown_and_dismiss_keeps_media() {
        for category_case in ["missing", "invalid", "mismatched"] {
            let fixture = FilesystemFixture::new();
            let destination =
                fs::canonicalize(&fixture.path).expect("destination must canonicalize");
            let record = transfer_from_source(
                TransferCategory::Adult,
                persistable_adult_fixture_source(),
                destination.clone(),
                TransferState::Cancelled,
            );
            let media_path = destination.join("Movie  A.mp4");
            fs::write(&media_path, b"media").expect("existing Adult media must be created");
            let mut encoded = encode_transfer(&record).expect("Adult record must encode");
            assert!(encoded.ends_with(b"\tadult"));
            let category_start = encoded.len() - "adult".len();
            match category_case {
                "missing" => encoded.truncate(category_start - 1),
                "invalid" => {
                    encoded.truncate(category_start);
                    encoded.extend_from_slice(b"invalid");
                }
                "mismatched" => {
                    encoded.truncate(category_start);
                    encoded.extend_from_slice(b"vr");
                }
                _ => unreachable!(),
            }
            assert!(parse_transfer_line(&encoded, false).is_none());

            let persistence_path = fixture.path.join("downloads");
            let mut persisted = PERSISTENCE_HEADER.to_vec();
            persisted.extend_from_slice(&encoded);
            persisted.push(b'\n');
            fs::write(&persistence_path, persisted).expect("damaged V2 row must persist");
            let state = VrDownloadState::default();
            let rows = tauri::async_runtime::block_on(load_downloads(
                &state,
                &persistence_path,
                &fixture.path.join("session"),
                &fixture.path.join("limit"),
            ))
            .expect("damaged V2 row must load as a terminal corrupt row");
            assert_eq!(rows[1], "unknown", "{category_case} category fell back");
            assert_eq!(rows[2], "ADLT-123");
            assert_eq!(rows[8], "offline");
            assert_eq!(rows[9], "false");
            assert_eq!(rows[10], "none");
            assert_eq!(rows[12], "false");
            assert_eq!(
                preview_organization(&state, &rows[0]),
                Err(VR_ORGANIZATION_STALE)
            );
            assert!(state.0.lock().expect("state must lock").session.is_none());

            dismiss_download(&state, &persistence_path, &rows[0])
                .expect("corrupt terminal row must dismiss");
            assert_eq!(
                fs::read(&media_path).expect("dismissed media must remain at its exact path"),
                b"media"
            );
            let restarted = VrDownloadState::default();
            assert!(tauri::async_runtime::block_on(load_downloads(
                &restarted,
                &persistence_path,
                &fixture.path.join("restart-session"),
                &fixture.path.join("limit"),
            ))
            .expect("dismissed row state must reload")
            .is_empty());
            assert_eq!(
                fs::read(&media_path).expect("relaunch must not alter dismissed media"),
                b"media"
            );
        }
    }

    #[test]
    fn corrupt_persistence_isolated_from_a_valid_record() {
        let fixture = FilesystemFixture::new();
        let path = fixture.path.join("downloads");
        let source = persistable_fixture_source();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let record = transfer_from_source(
            TransferCategory::Vr,
            source,
            destination,
            TransferState::Cancelled,
        );
        let mut bytes = PERSISTENCE_HEADER.to_vec();
        bytes.extend_from_slice(&encode_transfer(&record).expect("valid transfer must encode"));
        bytes.extend_from_slice(b"\nnot-a-record\n");
        fs::write(&path, bytes).expect("persistence fixture must write");

        let records = read_persisted_transfers(&path).expect("records must load independently");
        assert_eq!(records.len(), 2);
        assert!(matches!(records[0], StoredTransfer::Valid(_)));
        assert!(matches!(records[1], StoredTransfer::Corrupt(_)));
    }

    #[test]
    fn sparse_boundary_bytes_never_create_a_deselected_file() {
        let fixture = FilesystemFixture::new();
        let destination = fixture.path.join("unused-destination");
        let slot = SelectedStorageSlot::new(None);
        let storage = SelectedFileStorage {
            destination: destination.clone(),
            selected_files: Arc::new(BTreeMap::new()),
            boundary_segments: Arc::new(Mutex::new(BTreeMap::new())),
            resume: false,
            slots: vec![slot],
        };
        storage
            .pwrite_all(0, 3, b"boundary")
            .expect("boundary bytes must remain in memory");
        let mut bytes = [0_u8; 12];
        storage
            .pread_exact(0, 0, &mut bytes)
            .expect("boundary bytes must be readable");
        assert_eq!(&bytes[..3], &[0, 0, 0]);
        assert_eq!(&bytes[3..11], b"boundary");
        assert!(!destination.exists());
    }

    #[test]
    fn terminal_launch_validation_never_starts_network_and_marks_missing_files_offline() {
        let fixture = FilesystemFixture::new();
        let persistence_path = fixture.path.join("downloads");
        let session_folder = fixture.path.join("session");
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let mut record = transfer_from_source(
            TransferCategory::Vr,
            persistable_fixture_source(),
            destination.clone(),
            TransferState::Cancelled,
        );
        fs::write(destination.join("Movie  A.mp4"), b"abcde").expect("selected fixture must exist");
        record.fingerprints = capture_fingerprints(&record).expect("fingerprint must resolve");
        write_persisted_transfers(&persistence_path, &[StoredTransfer::Valid(record)])
            .expect("terminal fixture must persist");

        let available_state = VrDownloadState::default();
        let available_rows = tauri::async_runtime::block_on(load_downloads(
            &available_state,
            &persistence_path,
            &session_folder,
            &fixture.path.join("download-limit"),
        ))
        .expect("valid terminal row must load");
        assert_eq!(available_rows[8], "cancelled");
        assert!(available_state
            .0
            .lock()
            .expect("state must lock")
            .session
            .is_none());

        fs::remove_file(destination.join("Movie  A.mp4"))
            .expect("selected fixture must be removable");
        let missing_state = VrDownloadState::default();
        let missing_rows = tauri::async_runtime::block_on(load_downloads(
            &missing_state,
            &persistence_path,
            &session_folder,
            &fixture.path.join("download-limit"),
        ))
        .expect("missing terminal row must remain visible");
        assert_eq!(missing_rows[8], "offline");
        assert!(missing_state
            .0
            .lock()
            .expect("state must lock")
            .session
            .is_none());
    }

    #[test]
    fn relaunch_marks_verified_selected_content_complete_and_stops_its_handle() {
        const COMPLETION_ATTEMPTS: usize = 100;
        const COMPLETION_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(10);

        let fixture = FilesystemFixture::new();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let contents = b"ready";
        let metainfo = completed_file_torrent(contents);
        let infohash = hex_sha1(&metainfo[b"d4:info".len()..metainfo.len() - 1]);
        let source = revalidate_persisted_download_source(
            &metainfo,
            "MDVR-419",
            "MDVR-419 exact local completion",
            &infohash,
            &[0],
        )
        .expect("completion fixture must revalidate");
        let mut record = transfer_from_source(
            TransferCategory::Vr,
            source,
            destination.clone(),
            TransferState::Downloading,
        );
        fs::write(destination.join("Movie.mp4"), contents)
            .expect("completed selected file must exist");
        record.fingerprints = capture_fingerprints(&record).expect("fingerprint must resolve");
        let transfer_id = record.transfer_id.clone();
        let persistence_path = fixture.path.join("downloads");
        write_persisted_transfers(&persistence_path, &[StoredTransfer::Valid(record)])
            .expect("active fixture must persist");
        let state = VrDownloadState::default();

        tauri::async_runtime::block_on(async {
            load_downloads(
                &state,
                &persistence_path,
                &fixture.path.join("completion-session"),
                &fixture.path.join("download-limit"),
            )
            .await
            .expect("verified complete content must restore");
            let mut completed_rows = None;
            let mut last_rows = Vec::new();
            // One second bounds deterministic local completion without relying on a network peer.
            for _ in 0..COMPLETION_ATTEMPTS {
                let rows = list_downloads(&state, &persistence_path)
                    .expect("completion progress must remain readable");
                if rows[8] == "completed" {
                    completed_rows = Some(rows);
                    break;
                }
                last_rows = rows;
                std::thread::sleep(COMPLETION_POLL_INTERVAL);
            }
            let rows = completed_rows.unwrap_or_else(|| {
                panic!("verified selected content must complete locally: {last_rows:?}")
            });
            assert_eq!(rows[0], transfer_id);
            assert_eq!(rows[5], contents.len().to_string());
            assert_eq!(rows[6], contents.len().to_string());
            let context = state.0.lock().expect("state must lock");
            let StoredTransfer::Valid(record) = &context.transfers[0] else {
                panic!("completed transfer must remain valid");
            };
            assert!(record.handle.is_none());
        });
        assert_eq!(
            fs::read(destination.join("Movie.mp4")).expect("completed file must remain"),
            contents
        );
    }

    #[test]
    fn relaunch_preserves_vr_and_tv_selected_boundary_pieces_without_deselected_files() {
        const COMPLETION_ATTEMPTS: usize = 100;
        const COMPLETION_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(10);

        for category in [TransferCategory::Vr, TransferCategory::Tv] {
            let fixture = FilesystemFixture::new();
            let record =
                completed_selected_boundary_record_for_category(&fixture, category, Some(b"abc"));
            let transfer_id = record.transfer_id.clone();
            let destination = record.destination.clone();
            let persistence_path = fixture.path.join("downloads");
            write_persisted_transfers(&persistence_path, &[StoredTransfer::Valid(record)])
                .expect("completed boundary fixture must persist");
            let state = VrDownloadState::default();
            configure_category_folder(&state, category, &destination);

            tauri::async_runtime::block_on(async {
                load_downloads(
                    &state,
                    &persistence_path,
                    &fixture.path.join("boundary-session"),
                    &fixture.path.join("download-limit"),
                )
                .await
                .expect("retained boundary state must restore without a peer");
                let mut completed_rows = None;
                let mut last_rows = Vec::new();
                // One second bounds deterministic local completion without relying on a network peer.
                for _ in 0..COMPLETION_ATTEMPTS {
                    let rows = list_downloads(&state, &persistence_path)
                        .expect("restored boundary progress must remain readable");
                    if rows[8] == "completed" {
                        completed_rows = Some(rows);
                        break;
                    }
                    last_rows = rows;
                    std::thread::sleep(COMPLETION_POLL_INTERVAL);
                }
                let rows = completed_rows.unwrap_or_else(|| {
                    panic!(
                        "retained {category:?} boundary piece must complete locally: {last_rows:?}"
                    )
                });
                assert_eq!(rows[0], transfer_id);
                assert_eq!(rows[1], category.as_str());
                assert_eq!(rows[5], "7");
                assert_eq!(rows[6], "7");
                assert_eq!(rows[9], "true");
                let context = state.0.lock().expect("state must lock");
                let StoredTransfer::Valid(record) = &context.transfers[0] else {
                    panic!("completed boundary transfer must remain valid");
                };
                assert!(record.handle.is_none());
            });
            assert!(!destination.join("Folder/Part  1 — 映画.mkv").exists());
            assert_eq!(
                fs::read(destination.join("Folder/特別版  B.mp4"))
                    .expect("selected file must remain"),
                b"1234567"
            );
        }
    }

    #[test]
    fn missing_or_corrupt_retained_boundary_state_restores_offline_without_false_progress() {
        for (case, boundary_bytes) in [("missing", None), ("corrupt", Some(&b"abd"[..]))] {
            let fixture = FilesystemFixture::new();
            let record = completed_selected_boundary_record(&fixture, boundary_bytes);
            let destination = record.destination.clone();
            let persistence_path = fixture.path.join("downloads");
            write_persisted_transfers(&persistence_path, &[StoredTransfer::Valid(record)])
                .expect("invalid boundary fixture must persist");
            let state = VrDownloadState::default();

            let rows = tauri::async_runtime::block_on(load_downloads(
                &state,
                &persistence_path,
                &fixture.path.join(format!("{case}-boundary-session")),
                &fixture.path.join("download-limit"),
            ))
            .expect("invalid retained state must remain dismissable");
            assert_eq!(rows[6], "0", "{case} boundary state reported progress");
            assert_eq!(rows[8], "offline", "{case} boundary state resumed");
            let context = state.0.lock().expect("state must lock");
            let StoredTransfer::Valid(record) = &context.transfers[0] else {
                panic!("{case} boundary transfer must remain valid");
            };
            assert!(
                record.handle.is_none(),
                "{case} boundary handle remained active"
            );
            assert!(!destination.join("Folder/Part  1 — 映画.mkv").exists());
        }
    }

    #[test]
    fn movie_transfer_uses_only_native_identity_folder_and_selected_files_across_restart() {
        let fixture = FilesystemFixture::new();
        let movie_destination = fixture.path.join("Movies — 現在");
        let replacement_destination = fixture.path.join("Movies — replacement");
        fs::create_dir_all(&movie_destination).expect("Movies destination must exist");
        fs::create_dir_all(&replacement_destination)
            .expect("replacement Movies destination must exist");
        let movie_destination =
            fs::canonicalize(movie_destination).expect("Movies destination must canonicalize");
        let replacement_destination = fs::canonicalize(replacement_destination)
            .expect("replacement Movies destination must canonicalize");
        let bytes = selected_file_torrent();
        let infohash = hex_sha1(&bytes[b"d4:info".len()..bytes.len() - 1]);
        let torrent_url = format!(
            "https://yts.mx/torrent/download/{}",
            infohash.to_ascii_uppercase()
        );
        let torrent_state = MovieTorrentState::default();
        let release_generation = torrent_state
            .begin_release_lookup()
            .expect("Movie release lookup must begin");
        torrent_state
            .finish_release_lookup(
                release_generation,
                419,
                r#"{"id":419,"title":"Exact  Movie — 特別版","release_date":"1999-04-19"}"#,
                r#"{"id":419,"imdb_id":"tt0123456"}"#,
                &format!(
                    r#"{{"status":"ok","data":{{"movies":[{{"id":700,"imdb_code":"tt0123456","title":"Exact  YTS — 特別版","year":1999,"torrents":[{{"quality":"1080p","type":"bluray","video_codec":"x264","size":"12 B","size_bytes":12,"seeds":0,"peers":0,"hash":"{infohash}","url":"{torrent_url}"}}]}}]}}}}"#
                ),
            )
            .expect("exact Movie release identity must be stored");
        let request = crate::vr_torrent::MovieTorrentInspectionRequest {
            tmdb_movie_id: 419,
            tmdb_title: "Exact  Movie — 特別版".to_owned(),
            release_date: Some("1999-04-19".to_owned()),
            imdb_id: "tt0123456".to_owned(),
            provider_movie_id: 700,
            provider_title: Some("Exact  YTS — 特別版".to_owned()),
            provider_year: Some("1999".to_owned()),
            row_id: "700:0".to_owned(),
            quality: Some("1080p".to_owned()),
            type_label: Some("bluray".to_owned()),
            video_codec: Some("x264".to_owned()),
            size: Some("12 B".to_owned()),
            size_bytes: Some("12".to_owned()),
            seeds: Some("0".to_owned()),
            peers: Some("0".to_owned()),
            expected_infohash: infohash,
            torrent_url,
        };
        let (release_generation, inspection_generation) = torrent_state
            .begin_inspection(&request)
            .expect("exact Movie inspection must begin");
        let inspection = crate::vr_torrent::inspect_yts_movie_torrent_with(
            &torrent_state,
            release_generation,
            inspection_generation,
            request,
            |_| {
                Ok(crate::vr_torrent::ArtifactResponse {
                    status: 200,
                    redirect_url: None,
                    body: bytes.clone(),
                })
            },
        )
        .expect("exact Movie torrent must inspect");
        let persistence_path = fixture.path.join("downloads");
        let download_limit_path = fixture.path.join("limit");
        let state = VrDownloadState::default();

        tauri::async_runtime::block_on(async {
            load_downloads(
                &state,
                &persistence_path,
                &fixture.path.join("session"),
                &download_limit_path,
            )
            .await
            .expect("empty transfer state must load");
            configure_movie_download_folder(&state, Some(movie_destination.clone()))
                .expect("trusted Movies folder must configure");
            assert_eq!(
                start_movie_download(
                    &state,
                    &torrent_state,
                    &persistence_path,
                    &fixture.path.join("session"),
                    &inspection[0],
                    &[],
                )
                .await,
                Err(VR_DOWNLOAD_CONTEXT_INVALID)
            );
            let transfer_id = start_movie_download(
                &state,
                &torrent_state,
                &persistence_path,
                &fixture.path.join("session"),
                &inspection[0],
                &[1],
            )
            .await
            .expect("Movie transfer must start from the trusted native inspection");
            assert!(!movie_destination.join("Folder/Part  1 — 映画.mkv").exists());
            assert!(movie_destination.join("Folder/特別版  B.mp4").is_file());
            assert_eq!(
                start_movie_download(
                    &state,
                    &torrent_state,
                    &persistence_path,
                    &fixture.path.join("session"),
                    &inspection[0],
                    &[1],
                )
                .await,
                Err(VR_DOWNLOAD_DUPLICATE)
            );
            assert_eq!(
                start_download(
                    &state,
                    &VrTorrentState::default(),
                    &persistence_path,
                    &fixture.path.join("session"),
                    &inspection[0],
                    &[1],
                )
                .await,
                Err(VR_DOWNLOAD_STALE)
            );
            let rows = list_downloads(&state, &persistence_path)
                .expect("Movie transfer row must remain readable");
            assert_eq!(rows[0], transfer_id);
            assert_eq!(rows[1], "movie");
            assert_eq!(rows[2], "tt0123456");
            assert_eq!(rows[3], "Exact  Movie — 特別版");
            assert_eq!(rows[4], "1");
            assert_eq!(rows[9], "true");
            assert_eq!(rows[10], "none");
            assert_eq!(rows[12], "false");
            assert_eq!(
                preview_organization(&state, &transfer_id),
                Err(VR_ORGANIZATION_INELIGIBLE)
            );

            configure_movie_download_folder(&state, Some(replacement_destination))
                .expect("future Movies folder must change");
            assert_eq!(
                list_downloads(&state, &persistence_path)
                    .expect("old-folder Movie row must remain readable")[9],
                "false"
            );
            pause_download(&state, &persistence_path, &transfer_id)
                .await
                .expect("Movie transfer must pause");
            let (old_session, old_handle) = {
                let mut context = state.0.lock().expect("state must lock");
                let session = context.session.clone().expect("session must exist");
                let record = find_valid_record_mut(&mut context.transfers, &transfer_id)
                    .expect("paused Movie transfer must exist");
                record.handle_generation = record.handle_generation.wrapping_add(1);
                let handle = record.handle.take().expect("paused handle must exist");
                write_persisted_transfers(&persistence_path, &context.transfers)
                    .expect("paused Movie transfer must persist");
                (session, handle)
            };
            old_session
                .delete(old_handle.id().into(), false)
                .await
                .expect("fixture session must detach without deleting Movie media");
            assert!(movie_destination.join("Folder/特別版  B.mp4").is_file());

            let resumed = VrDownloadState::default();
            configure_movie_download_folder(&resumed, Some(movie_destination.clone()))
                .expect("current Movies folder must restore");
            let rows = load_downloads(
                &resumed,
                &persistence_path,
                &fixture.path.join("restart-session"),
                &download_limit_path,
            )
            .await
            .expect("paused Movie transfer must reload");
            assert_eq!(rows[0], transfer_id);
            assert_eq!(rows[1], "movie");
            assert_eq!(rows[8], "paused");
            assert_eq!(rows[9], "true");
            {
                let context = resumed.0.lock().expect("state must lock");
                let StoredTransfer::Valid(record) = &context.transfers[0] else {
                    panic!("persisted Movie transfer must remain valid");
                };
                let identity = record
                    .movie_identity
                    .as_ref()
                    .expect("Movie identity must persist");
                assert_eq!(identity.tmdb_movie_id, 419);
                assert_eq!(identity.tmdb_title, "Exact  Movie — 特別版");
                assert_eq!(identity.release_date.as_deref(), Some("1999-04-19"));
                assert_eq!(identity.imdb_id, "tt0123456");
                assert_eq!(identity.provider_movie_id, 700);
                assert_eq!(
                    identity.provider_title.as_deref(),
                    Some("Exact  YTS — 特別版")
                );
                assert_eq!(identity.row_id, "700:0");
                assert_eq!(identity.quality.as_deref(), Some("1080p"));
                assert_eq!(identity.expected_infohash, record.infohash);
            }
            resume_download(&resumed, &persistence_path, &transfer_id)
                .await
                .expect("restored Movie transfer must resume");
            cancel_download(&resumed, &persistence_path, &transfer_id)
                .await
                .expect("resumed Movie transfer must cancel without deleting media");
            assert!(movie_destination.join("Folder/特別版  B.mp4").is_file());

            let restarted = VrDownloadState::default();
            configure_movie_download_folder(&restarted, Some(movie_destination.clone()))
                .expect("current Movies folder must restore after cancellation");
            let rows = load_downloads(
                &restarted,
                &persistence_path,
                &fixture.path.join("cancelled-restart-session"),
                &download_limit_path,
            )
            .await
            .expect("cancelled Movie transfer must reload");
            assert_eq!(rows[0], transfer_id);
            assert_eq!(rows[1], "movie");
            assert_eq!(rows[8], "cancelled");
            assert_eq!(rows[9], "true");
            assert!(restarted
                .0
                .lock()
                .expect("state must lock")
                .session
                .is_none());
            dismiss_download(&restarted, &persistence_path, &transfer_id)
                .expect("terminal Movie row must dismiss");
            assert!(movie_destination.join("Folder/特別版  B.mp4").is_file());
        });
    }

    #[test]
    fn adult_transfer_uses_native_folder_selected_only_writes_and_category_isolated_restart() {
        let fixture = FilesystemFixture::new();
        let adult_destination = fixture.path.join("Adult — 現在");
        let unrelated_destination = fixture.path.join("Adult — unrelated");
        fs::create_dir_all(&adult_destination).expect("Adult destination must exist");
        fs::create_dir_all(&unrelated_destination).expect("unrelated destination must exist");
        let adult_destination =
            fs::canonicalize(adult_destination).expect("Adult destination must canonicalize");
        let unrelated_destination = fs::canonicalize(unrelated_destination)
            .expect("unrelated destination must canonicalize");
        let unrelated_file = unrelated_destination.join("Folder/特別版  B.mp4");
        fs::create_dir_all(
            unrelated_file
                .parent()
                .expect("unrelated parent must exist"),
        )
        .expect("unrelated parent must be created");
        fs::write(&unrelated_file, b"unrelated").expect("unrelated media must exist");

        let metainfo = selected_file_torrent();
        let infohash = hex_sha1(&metainfo[b"d4:info".len()..metainfo.len() - 1]);
        let source = revalidate_persisted_download_source(
            &metainfo,
            "ADLT-123",
            "【Adult】 ADLT-123  Exact — 特別版",
            &infohash,
            &[1],
        )
        .expect("Adult selected source must revalidate");
        let persistence_path = fixture.path.join("downloads");
        let session_folder = fixture.path.join("session");
        let download_limit_path = fixture.path.join("limit");
        let state = VrDownloadState::default();
        {
            let mut context = state.0.lock().expect("state must lock");
            context.adult_future_folder = Some(adult_destination.clone());
            context.download_limit = DownloadLimitState::Loaded(None);
            context.transfers_loaded = true;
        }

        tauri::async_runtime::block_on(async {
            let transfer_id = start_download_source(
                &state,
                &persistence_path,
                &session_folder,
                TransferCategory::Adult,
                source.clone(),
            )
            .await
            .expect("Adult transfer must start from native state");
            assert_eq!(
                start_download_source(
                    &state,
                    &persistence_path,
                    &session_folder,
                    TransferCategory::Adult,
                    source,
                )
                .await,
                Err(VR_DOWNLOAD_DUPLICATE)
            );
            assert!(!adult_destination.join("Folder/Part  1 — 映画.mkv").exists());
            assert!(adult_destination.join("Folder/特別版  B.mp4").is_file());
            assert_eq!(
                fs::read(&unrelated_file).expect("unrelated media must remain readable"),
                b"unrelated"
            );

            configure_adult_download_folder(&state, Some(unrelated_destination.clone()))
                .expect("future Adult folder must change");
            let rows =
                list_downloads(&state, &persistence_path).expect("Adult row must remain readable");
            assert_eq!(rows[0], transfer_id);
            assert_eq!(rows[1], "adult");
            assert_eq!(rows[2], "ADLT-123");
            assert_eq!(rows[9], "false");
            assert_eq!(rows[10], "none");
            assert_eq!(rows[12], "false");
            assert_eq!(
                preview_organization(&state, &transfer_id),
                Err(VR_ORGANIZATION_INELIGIBLE)
            );
            cancel_download(&state, &persistence_path, &transfer_id)
                .await
                .expect("Adult transfer must cancel without deleting media");
            assert!(adult_destination.join("Folder/特別版  B.mp4").is_file());

            let restarted = VrDownloadState::default();
            configure_adult_download_folder(&restarted, Some(adult_destination.clone()))
                .expect("current Adult folder must restore");
            let rows = load_downloads(
                &restarted,
                &persistence_path,
                &fixture.path.join("restart-session"),
                &download_limit_path,
            )
            .await
            .expect("cancelled Adult transfer must reload");
            assert_eq!(rows[0], transfer_id);
            assert_eq!(rows[1], "adult");
            assert_eq!(rows[8], "cancelled");
            assert_eq!(rows[9], "true");
            assert!(restarted
                .0
                .lock()
                .expect("state must lock")
                .session
                .is_none());
            dismiss_download(&restarted, &persistence_path, &transfer_id)
                .expect("terminal Adult row must dismiss");
            assert!(adult_destination.join("Folder/特別版  B.mp4").is_file());
        });
    }

    #[test]
    fn tv_start_uses_only_current_inspection_selected_files_and_native_folder() {
        let fixture = FilesystemFixture::new();
        let destination = fixture.path.join("TV — 現在");
        fs::create_dir_all(&destination).expect("TV destination must exist");
        let destination = fs::canonicalize(destination).expect("TV destination must canonicalize");
        let metainfo = selected_file_torrent();
        let (release_state, torrent_state, inspection_id) = inspected_tv_torrent(metainfo);
        let persistence_path = fixture.path.join("downloads");
        let session_folder = fixture.path.join("session");
        let state = VrDownloadState::default();

        tauri::async_runtime::block_on(async {
            load_downloads(
                &state,
                &persistence_path,
                &session_folder,
                &fixture.path.join("limit"),
            )
            .await
            .expect("empty shared transfer state must load");
            configure_tv_download_folder(&state, Some(destination.clone()))
                .expect("native TV folder must configure");
            assert_eq!(
                start_tv_download(
                    &state,
                    &torrent_state,
                    &release_state,
                    &persistence_path,
                    &session_folder,
                    &inspection_id,
                    &[],
                )
                .await,
                Err(VR_DOWNLOAD_CONTEXT_INVALID)
            );
            let transfer_id = start_tv_download(
                &state,
                &torrent_state,
                &release_state,
                &persistence_path,
                &session_folder,
                &inspection_id,
                &[1],
            )
            .await
            .expect("current TV inspection and explicit file must start");
            assert!(!destination.join("Folder/Part  1 — 映画.mkv").exists());
            assert!(destination.join("Folder/特別版  B.mp4").is_file());
            let rows = list_downloads(&state, &persistence_path)
                .expect("started TV row must remain readable");
            assert_eq!(rows[0], transfer_id);
            assert_eq!(rows[1], "tv");
            assert_eq!(rows[2], "tt0123456 · S02E03");
            assert_eq!(rows[3], "Exact  Show — 特別版.S02E03+720p.第三話");
            assert_eq!(rows[4], "1");
            assert_eq!(rows[9], "true");
            assert_eq!(rows[10], "none");
            assert_eq!(rows[12], "false");
            assert_eq!(
                preview_organization(&state, &transfer_id),
                Err(VR_ORGANIZATION_INELIGIBLE)
            );
            assert_eq!(
                start_tv_download(
                    &state,
                    &torrent_state,
                    &release_state,
                    &persistence_path,
                    &session_folder,
                    &inspection_id,
                    &[1],
                )
                .await,
                Err(VR_DOWNLOAD_DUPLICATE)
            );
            torrent_state
                .invalidate_inspection()
                .expect("TV inspection must invalidate");
            assert_eq!(
                start_tv_download(
                    &state,
                    &torrent_state,
                    &release_state,
                    &persistence_path,
                    &session_folder,
                    &inspection_id,
                    &[0],
                )
                .await,
                Err(VR_DOWNLOAD_STALE)
            );
            cancel_download(&state, &persistence_path, &transfer_id)
                .await
                .expect("TV cancel must retain selected media");
            assert!(destination.join("Folder/特別版  B.mp4").is_file());

            let restarted = VrDownloadState::default();
            configure_tv_download_folder(&restarted, Some(destination.clone()))
                .expect("TV folder must restore");
            let rows = load_downloads(
                &restarted,
                &persistence_path,
                &fixture.path.join("restart-session"),
                &fixture.path.join("limit"),
            )
            .await
            .expect("cancelled TV transfer must reload");
            assert_eq!(rows[0], transfer_id);
            assert_eq!(rows[1], "tv");
            assert_eq!(rows[8], "cancelled");
            assert_eq!(rows[9], "true");
            assert!(restarted
                .0
                .lock()
                .expect("state must lock")
                .session
                .is_none());
            dismiss_download(&restarted, &persistence_path, &transfer_id)
                .expect("terminal TV row must dismiss durably");
            assert!(destination.join("Folder/特別版  B.mp4").is_file());
        });
    }

    #[test]
    fn tv_explicit_start_survives_pause_restart_resume_cancel_restart_and_durable_dismiss() {
        let fixture = FilesystemFixture::new();
        let destination = fixture.path.join("TV — lifecycle");
        fs::create_dir_all(&destination).expect("TV destination must exist");
        let destination = fs::canonicalize(destination).expect("TV destination must canonicalize");
        let (release_state, torrent_state, inspection_id) =
            inspected_tv_torrent(selected_file_torrent());
        let persistence_path = fixture.path.join("downloads");
        let download_limit_path = fixture.path.join("limit");
        let state = VrDownloadState::default();
        let exact_authority = |record: &TransferRecord| {
            (
                record.transfer_id.clone(),
                record.category,
                record.code.clone(),
                record.release_name.clone(),
                record.tv_identity.clone(),
                record.infohash.clone(),
                record.metainfo.clone(),
                record.selected_files.clone(),
                record.destination.clone(),
                record.fingerprints.clone(),
                record.current_paths.clone(),
                record.organization_state,
            )
        };

        tauri::async_runtime::block_on(async {
            load_downloads(
                &state,
                &persistence_path,
                &fixture.path.join("session"),
                &download_limit_path,
            )
            .await
            .expect("empty shared transfer state must load");
            save_download_limit(&state, &download_limit_path, Some("2"))
                .expect("finite aggregate limit must persist");
            configure_tv_download_folder(&state, Some(destination.clone()))
                .expect("native TV folder must configure");
            let transfer_id = start_tv_download(
                &state,
                &torrent_state,
                &release_state,
                &persistence_path,
                &fixture.path.join("session"),
                &inspection_id,
                &[1],
            )
            .await
            .expect("explicit selected TV file must start");
            let selected_path = destination.join("Folder/特別版  B.mp4");
            let deselected_path = destination.join("Folder/Part  1 — 映画.mkv");
            assert!(selected_path.is_file());
            assert!(!deselected_path.exists());

            pause_download(&state, &persistence_path, &transfer_id)
                .await
                .expect("TV transfer must pause and persist");
            let expected_authority = {
                let mut context = state.0.lock().expect("download state must lock");
                let record = find_valid_record_mut(&mut context.transfers, &transfer_id)
                    .expect("started TV transfer must remain valid");
                assert_eq!(record.state, TransferState::Paused);
                let boundary_storage = SelectedFileStorage {
                    destination: record.destination.clone(),
                    selected_files: Arc::new(
                        record
                            .selected_files
                            .iter()
                            .cloned()
                            .map(|file| (file.file_id, file))
                            .collect(),
                    ),
                    boundary_segments: record.boundary_segments.clone(),
                    resume: true,
                    slots: vec![SelectedStorageSlot::new(None)],
                };
                boundary_storage
                    .pwrite_all(0, 0, b"ab")
                    .expect("boundary-piece progress must be retained");
                let authority = exact_authority(record);
                write_persisted_transfers(&persistence_path, &context.transfers)
                    .expect("paused TV boundary progress must persist");
                authority
            };
            let selected_media = fs::read(&selected_path).expect("selected TV media must exist");
            assert!(!deselected_path.exists());

            let restarted = VrDownloadState::default();
            configure_tv_download_folder(&restarted, Some(destination.clone()))
                .expect("TV folder must restore");
            let rows = load_downloads(
                &restarted,
                &persistence_path,
                &fixture.path.join("restart-session"),
                &download_limit_path,
            )
            .await
            .expect("paused TV transfer must reload");
            assert_eq!(rows[0], transfer_id);
            assert_eq!(rows[1], "tv");
            assert_eq!(rows[2], "tt0123456 · S02E03");
            assert_eq!(rows[3], "Exact  Show — 特別版.S02E03+720p.第三話");
            assert_eq!(rows[8], "paused");
            assert_eq!(rows[9], "true");
            {
                let mut context = restarted.0.lock().expect("restarted state must lock");
                assert_eq!(
                    context.download_limit,
                    DownloadLimitState::Loaded(NonZeroU32::new(2))
                );
                let record = find_valid_record_mut(&mut context.transfers, &transfer_id)
                    .expect("restarted TV transfer must remain exact");
                assert_eq!(exact_authority(record), expected_authority);
                let boundary = record
                    .boundary_segments
                    .lock()
                    .expect("boundary state must lock");
                assert_eq!(boundary[&0][0].offset, 0);
                assert_eq!(boundary[&0][0].bytes, b"ab");
                assert!(record.handle.is_some());
            }
            assert_eq!(
                fs::read(&selected_path).expect("selected media must survive restart"),
                selected_media
            );
            assert!(!deselected_path.exists());

            resume_download(&restarted, &persistence_path, &transfer_id)
                .await
                .expect("paused TV transfer must resume explicitly");
            {
                let mut context = restarted.0.lock().expect("resumed state must lock");
                let record = find_valid_record_mut(&mut context.transfers, &transfer_id)
                    .expect("resumed TV transfer must remain valid");
                assert_eq!(record.state, TransferState::Downloading);
                assert_eq!(exact_authority(record), expected_authority);
                assert_eq!(
                    record
                        .boundary_segments
                        .lock()
                        .expect("boundary state must lock")[&0][0]
                        .bytes,
                    b"ab"
                );
            }
            assert_eq!(
                fs::read(&selected_path).expect("resume must retain selected media"),
                selected_media
            );
            assert!(!deselected_path.exists());

            cancel_download(&restarted, &persistence_path, &transfer_id)
                .await
                .expect("TV cancel must retain all partial data");
            assert_eq!(
                fs::read(&selected_path).expect("cancel must retain selected media"),
                selected_media
            );
            assert!(!deselected_path.exists());

            let cancelled_restart = VrDownloadState::default();
            configure_tv_download_folder(&cancelled_restart, Some(destination.clone()))
                .expect("TV folder must remain current");
            let rows = load_downloads(
                &cancelled_restart,
                &persistence_path,
                &fixture.path.join("cancelled-restart-session"),
                &download_limit_path,
            )
            .await
            .expect("cancelled TV transfer must reload");
            assert_eq!(rows[0], transfer_id);
            assert_eq!(rows[1], "tv");
            assert_eq!(rows[8], "cancelled");
            assert_eq!(rows[9], "true");
            {
                let mut context = cancelled_restart
                    .0
                    .lock()
                    .expect("cancelled restart state must lock");
                let record = find_valid_record_mut(&mut context.transfers, &transfer_id)
                    .expect("cancelled TV transfer must remain exact");
                assert_eq!(exact_authority(record), expected_authority);
                assert_eq!(
                    record
                        .boundary_segments
                        .lock()
                        .expect("boundary state must lock")[&0][0]
                        .bytes,
                    b"ab"
                );
                assert!(record.handle.is_none());
            }
            dismiss_download(&cancelled_restart, &persistence_path, &transfer_id)
                .expect("terminal TV row must dismiss durably");
            assert_eq!(
                fs::read(&selected_path).expect("dismiss must retain selected media"),
                selected_media
            );
            assert!(!deselected_path.exists());

            let dismissed_restart = VrDownloadState::default();
            configure_tv_download_folder(&dismissed_restart, Some(destination.clone()))
                .expect("TV folder must remain configured");
            assert!(load_downloads(
                &dismissed_restart,
                &persistence_path,
                &fixture.path.join("dismissed-restart-session"),
                &download_limit_path,
            )
            .await
            .expect("dismissed TV state must remain readable")
            .is_empty());
            assert_eq!(
                fs::read(&selected_path).expect("durable dismiss must retain media"),
                selected_media
            );
            assert!(!deselected_path.exists());
        });
    }

    #[test]
    fn tv_library_trash_rejects_a_selected_transfer_file_without_mutating_transfer_state() {
        let fixture = FilesystemFixture::new();
        let destination = fixture.path.join("TV — transfer-owned");
        let holding = fixture.path.join("Trash fixture");
        fs::create_dir_all(&destination).expect("TV destination must exist");
        fs::create_dir_all(&holding).expect("Trash fixture must exist");
        let destination = fs::canonicalize(destination).expect("TV destination must canonicalize");
        let selected = destination.join("Show.S02E03.mkv");
        let unrelated = destination.join("Unrelated.S01E01.mp4");
        fs::write(&selected, b"selected").expect("selected TV media must exist");
        fs::write(&unrelated, b"unrelated").expect("unrelated TV media must exist");

        let mut record = transfer_from_source(
            TransferCategory::Tv,
            tv_download_source(&[("Show.S02E03.mkv", 8)], &[0]),
            destination.clone(),
            TransferState::Downloading,
        );
        record.downloaded_bytes = 4;
        record
            .boundary_segments
            .lock()
            .expect("boundary state must lock")
            .insert(
                0,
                vec![SparseSegment {
                    offset: 2,
                    bytes: b"boundary".to_vec(),
                }],
            );
        let download_state = VrDownloadState::default();
        {
            let mut context = download_state.0.lock().expect("download state must lock");
            context.tv_future_folder = Some(destination.clone());
            context.transfers_loaded = true;
            context.persistence_path = Some(fixture.path.join("downloads"));
            context.transfers.push(StoredTransfer::Valid(record));
        }
        let library_state = TvLibraryState::default();
        set_tv_folder(
            &library_state,
            &fixture.path.join("tv-folder"),
            destination.clone(),
        )
        .expect("TV Library folder must configure");
        let scan = scan_tv_library_with(&library_state).expect("TV Library scan must succeed");
        let generation = scan[0].parse().expect("scan generation must be valid");
        let before_transfers = transfer_snapshots(&download_state);
        let before_rows = transfer_rows(&download_state);
        let dispatched = Cell::new(false);

        assert_eq!(
            trash_tv_file_with_download_ownership(
                &selected,
                generation,
                &download_state,
                &library_state,
                |_| {
                    dispatched.set(true);
                    Ok(())
                },
            ),
            Err(TV_FILE_TRASH_OWNED)
        );
        assert!(!dispatched.get());
        assert_eq!(
            fs::read(&selected).expect("selected TV media must remain readable"),
            b"selected"
        );
        assert_eq!(transfer_snapshots(&download_state), before_transfers);
        assert_eq!(transfer_rows(&download_state), before_rows);

        let moved = holding.join("Unrelated.S01E01.mp4");
        trash_tv_file_with_download_ownership(
            &unrelated,
            generation,
            &download_state,
            &library_state,
            |path| {
                assert!(matches!(
                    download_state.0.try_lock(),
                    Err(TryLockError::WouldBlock)
                ));
                fs::rename(path, &moved).map_err(|_| ())
            },
        )
        .expect("unrelated TV media must remain removable");
        assert_eq!(
            fs::read(moved).expect("moved unrelated TV media must remain readable"),
            b"unrelated"
        );
        assert_eq!(transfer_snapshots(&download_state), before_transfers);
        assert_eq!(transfer_rows(&download_state), before_rows);
    }

    #[test]
    fn vr_library_trash_rejects_active_paused_cancelled_and_completed_selected_files() {
        let fixture = FilesystemFixture::new();
        let destination = fixture.path.join("VR — transfer-owned");
        let holding = fixture.path.join("Trash fixture");
        fs::create_dir_all(&destination).expect("VR destination must exist");
        fs::create_dir_all(&holding).expect("Trash fixture must exist");
        let destination = fs::canonicalize(destination).expect("VR destination must canonicalize");
        let protected_files = [
            (
                "MDVR-419 Part 01.mp4",
                TransferState::Downloading,
                b"active".as_slice(),
                b"active-boundary".as_slice(),
            ),
            (
                "MDVR-419 PT 02.mkv",
                TransferState::Paused,
                b"paused".as_slice(),
                b"paused-boundary".as_slice(),
            ),
            (
                "MDVR-419 CD3.mp4",
                TransferState::Cancelled,
                b"cancelled".as_slice(),
                b"cancelled-boundary".as_slice(),
            ),
            (
                "MDVR-419 Disc 04.mkv",
                TransferState::Completed,
                b"completed".as_slice(),
                b"".as_slice(),
            ),
        ];
        let mut records = Vec::new();
        for (relative_path, transfer_state, media_bytes, boundary_bytes) in protected_files {
            fs::write(destination.join(relative_path), media_bytes)
                .expect("protected transfer media must exist");
            let mut record = transfer_from_source(
                TransferCategory::Vr,
                organization_source(vec![(relative_path, media_bytes.len() as u64)]),
                destination.clone(),
                transfer_state,
            );
            record.downloaded_bytes = if transfer_state == TransferState::Completed {
                media_bytes.len() as u64
            } else {
                1
            };
            if !boundary_bytes.is_empty() {
                record
                    .boundary_segments
                    .lock()
                    .expect("boundary state must lock")
                    .insert(
                        0,
                        vec![SparseSegment {
                            offset: 1,
                            bytes: boundary_bytes.to_vec(),
                        }],
                    );
            }
            records.push(StoredTransfer::Valid(record));
        }
        let unrelated = destination.join("MDVR-430 unrelated.mp4");
        fs::write(&unrelated, b"unrelated").expect("unrelated VR media must exist");
        let state = VrDownloadState::default();
        let ownership_persistence = fixture.path.join("ownership-downloads");
        {
            let mut context = state.0.lock().expect("state must lock");
            context.future_folder = Some(destination.clone());
            context.transfers_loaded = true;
            context.persistence_path = Some(ownership_persistence);
            context.transfers = records;
        }
        let library_state = VrLibraryState::default();
        let scan = scan_vr_library_with(&state, &library_state).expect("VR scan must succeed");
        let generation = scan[0].parse().expect("scan generation must be valid");
        let before_snapshots = transfer_snapshots(&state);
        let before_rows = transfer_rows(&state);
        let dispatch_count = Cell::new(0);

        for (relative_path, _, media_bytes, _) in protected_files {
            let path = destination.join(relative_path);
            assert_eq!(
                trash_vr_file_with(&path, generation, &state, &library_state, |_| {
                    dispatch_count.set(dispatch_count.get() + 1);
                    Ok(())
                }),
                Err(VR_FILE_TRASH_OWNED),
                "{relative_path:?} was not protected",
            );
            assert_eq!(
                fs::read(path).expect("protected transfer media must remain readable"),
                media_bytes,
            );
        }
        assert_eq!(dispatch_count.get(), 0);
        assert_eq!(transfer_snapshots(&state), before_snapshots);
        assert_eq!(transfer_rows(&state), before_rows);

        let moved_unrelated = holding.join("MDVR-430 unrelated.mp4");
        trash_vr_file_with(&unrelated, generation, &state, &library_state, |path| {
            assert!(matches!(state.0.try_lock(), Err(TryLockError::WouldBlock)));
            fs::rename(path, &moved_unrelated).map_err(|_| ())
        })
        .expect("an unrelated scanned VR file must remain removable");
        assert_eq!(
            fs::read(moved_unrelated).expect("moved unrelated media must remain readable"),
            b"unrelated"
        );
        assert_eq!(transfer_snapshots(&state), before_snapshots);
        assert_eq!(transfer_rows(&state), before_rows);
    }

    #[test]
    fn vr_library_trash_rejects_planned_organized_and_recovery_owned_paths() {
        let fixture = FilesystemFixture::new();

        let plan_destination = fixture.path.join("VR — planned");
        fs::create_dir_all(&plan_destination).expect("plan destination must exist");
        let plan_destination =
            fs::canonicalize(plan_destination).expect("plan destination must canonicalize");
        let plan_record = completed_organization_record(
            &plan_destination,
            organization_source(vec![("Source/MDVR-419 Part 01.mp4", 3)]),
        );
        let (plan_state, plan_transfer_id) = organization_state(plan_record);
        let preview =
            preview_organization(&plan_state, &plan_transfer_id).expect("plan must preview");
        let planned_path = plan_destination.join(&preview[7]);
        fs::create_dir_all(
            planned_path
                .parent()
                .expect("planned path must have a parent"),
        )
        .expect("planned parent must exist");
        fs::write(&planned_path, b"reserved").expect("planned target fixture must exist");
        let plan_library_state = VrLibraryState::default();
        let plan_scan = scan_vr_library_with(&plan_state, &plan_library_state)
            .expect("planned-path scan must succeed");
        let plan_generation = plan_scan[0]
            .parse()
            .expect("planned-path generation must be valid");
        let plan_snapshot = {
            let context = plan_state.0.lock().expect("plan state must lock");
            organization_plan_response(
                context
                    .organization_plan
                    .as_ref()
                    .expect("plan must remain current"),
            )
        };
        let plan_transfer_snapshot = transfer_snapshots(&plan_state);
        assert_eq!(
            trash_vr_file_with(
                &planned_path,
                plan_generation,
                &plan_state,
                &plan_library_state,
                |_| panic!("planned path reached Trash dispatch"),
            ),
            Err(VR_FILE_TRASH_OWNED)
        );
        assert_eq!(
            fs::read(&planned_path).expect("planned media must remain readable"),
            b"reserved"
        );
        assert_eq!(transfer_snapshots(&plan_state), plan_transfer_snapshot);
        assert_eq!(
            organization_plan_response(
                plan_state
                    .0
                    .lock()
                    .expect("plan state must lock")
                    .organization_plan
                    .as_ref()
                    .expect("plan must remain current"),
            ),
            plan_snapshot,
        );

        let organized_destination = fixture.path.join("VR — organized");
        fs::create_dir_all(&organized_destination).expect("organized destination must exist");
        let organized_destination = fs::canonicalize(organized_destination)
            .expect("organized destination must canonicalize");
        let organized_record =
            completed_organization_record(&organized_destination, persistable_fixture_source());
        let (organized_state, organized_transfer_id) = organization_state(organized_record);
        let organized_persistence = fixture.path.join("organized-downloads");
        let organized_preview = preview_organization(&organized_state, &organized_transfer_id)
            .expect("organization must preview");
        apply_organization(
            &organized_state,
            &organized_persistence,
            &organized_preview[0],
        )
        .expect("organization must apply");
        let organized_path = organized_destination.join("MDVR-419/MDVR-419.mp4");
        let organized_library_state = VrLibraryState::default();
        let organized_scan = scan_vr_library_with(&organized_state, &organized_library_state)
            .expect("organized-path scan must succeed");
        let organized_generation = organized_scan[0]
            .parse()
            .expect("organized-path generation must be valid");
        let organized_snapshot = transfer_snapshots(&organized_state);
        let organized_rows = transfer_rows(&organized_state);
        assert_eq!(
            trash_vr_file_with(
                &organized_path,
                organized_generation,
                &organized_state,
                &organized_library_state,
                |_| panic!("organized path reached Trash dispatch"),
            ),
            Err(VR_FILE_TRASH_OWNED)
        );
        assert_eq!(
            fs::read(&organized_path).expect("organized media must remain readable"),
            vec![b'a'; 5]
        );
        assert_eq!(transfer_snapshots(&organized_state), organized_snapshot);
        assert_eq!(transfer_rows(&organized_state), organized_rows);

        let recovery_destination = fixture.path.join("VR — recovery");
        let recovered_holding = fixture.path.join("Recovered Trash fixture");
        fs::create_dir_all(&recovery_destination).expect("recovery destination must exist");
        fs::create_dir_all(&recovered_holding).expect("recovery Trash fixture must exist");
        let recovery_destination =
            fs::canonicalize(recovery_destination).expect("recovery destination must canonicalize");
        let mut recovery_record =
            completed_organization_record(&recovery_destination, persistable_fixture_source());
        recovery_record.organization_state = OrganizationState::Attention;
        let recovery_transfer_id = recovery_record.transfer_id.clone();
        let recovery_path = organization_recovery_path(&recovery_record);
        write_organization_recovery(&recovery_record, &recovery_record.current_paths, None)
            .expect("durable recovery must persist");
        let recovery_bytes = fs::read(&recovery_path).expect("recovery metadata must be readable");
        let recovered_media = recovery_destination.join("Movie  A.mp4");
        let recovery_state = VrDownloadState::default();
        let recovery_persistence = fixture.path.join("recovery-downloads");
        {
            let mut context = recovery_state.0.lock().expect("recovery state must lock");
            context.future_folder = Some(recovery_destination.clone());
            context.transfers_loaded = true;
            context.persistence_path = Some(recovery_persistence.clone());
        }
        let recovery_library_state = VrLibraryState::default();
        let recovery_scan = scan_vr_library_with(&recovery_state, &recovery_library_state)
            .expect("recovery-path scan must succeed");
        let recovery_generation = recovery_scan[0]
            .parse()
            .expect("recovery-path generation must be valid");
        assert_eq!(
            trash_vr_file_with(
                &recovered_media,
                recovery_generation,
                &recovery_state,
                &recovery_library_state,
                |_| panic!("durable recovery path reached Trash dispatch"),
            ),
            Err(VR_FILE_TRASH_OWNED)
        );
        assert_eq!(
            fs::read(&recovered_media).expect("recovery-owned media must remain readable"),
            vec![b'a'; 5]
        );
        assert_eq!(
            fs::read(&recovery_path).expect("recovery metadata must remain readable"),
            recovery_bytes
        );
        assert!(recovery_state
            .0
            .lock()
            .expect("recovery state must lock")
            .transfers
            .is_empty());

        recovery_state
            .0
            .lock()
            .expect("recovery state must lock")
            .transfers
            .push(StoredTransfer::Valid(recovery_record));
        let attention_snapshot = transfer_snapshots(&recovery_state);
        let attention_rows = transfer_rows(&recovery_state);
        assert_eq!(
            trash_vr_file_with(
                &recovered_media,
                recovery_generation,
                &recovery_state,
                &recovery_library_state,
                |_| panic!("attention recovery path reached Trash dispatch"),
            ),
            Err(VR_FILE_TRASH_OWNED)
        );
        assert_eq!(transfer_snapshots(&recovery_state), attention_snapshot);
        assert_eq!(transfer_rows(&recovery_state), attention_rows);
        assert_eq!(
            fs::read(&recovery_path).expect("attention recovery metadata must remain readable"),
            recovery_bytes
        );
        dismiss_download(
            &recovery_state,
            &recovery_persistence,
            &recovery_transfer_id,
        )
        .expect("terminal recovery row must dismiss durably");
        assert!(read_persisted_transfers(&recovery_persistence)
            .expect("dismissed persistence must remain valid")
            .is_empty());
        assert!(!recovery_path.exists());
        assert_eq!(
            fs::read(&recovered_media).expect("dismissal must retain recovery-owned media"),
            vec![b'a'; 5]
        );

        let fresh_scan = scan_vr_library_with(&recovery_state, &recovery_library_state)
            .expect("fresh post-dismissal scan must succeed");
        let fresh_generation = fresh_scan[0]
            .parse()
            .expect("fresh post-dismissal generation must be valid");
        let moved_recovered_media = recovered_holding.join("Movie  A.mp4");
        trash_vr_file_with(
            &recovered_media,
            fresh_generation,
            &recovery_state,
            &recovery_library_state,
            |path| {
                assert!(matches!(
                    recovery_state.0.try_lock(),
                    Err(TryLockError::WouldBlock)
                ));
                fs::rename(path, &moved_recovered_media).map_err(|_| ())
            },
        )
        .expect("fresh scan must authorize post-dismissal Trash");
        assert_eq!(
            fs::read(moved_recovered_media).expect("post-dismissal move must retain media bytes"),
            vec![b'a'; 5]
        );
    }

    #[test]
    fn vr_library_trash_rejects_shared_adult_transfer_organization_and_recovery_paths() {
        assert_shared_category_vr_trash_ownership(TransferCategory::Adult);
    }

    #[test]
    fn vr_library_trash_rejects_shared_movie_transfer_organization_and_recovery_paths() {
        assert_shared_category_vr_trash_ownership(TransferCategory::Movie);
    }

    #[test]
    fn vr_library_trash_rejects_shared_tv_transfer_organization_and_recovery_paths() {
        assert_shared_category_vr_trash_ownership(TransferCategory::Tv);
    }

    #[test]
    fn vr_library_trash_fails_closed_for_unavailable_transfer_or_recovery_ownership() {
        let fixture = FilesystemFixture::new();
        let destination = fixture.path.join("VR — unavailable ownership");
        fs::create_dir_all(&destination).expect("VR destination must exist");
        let destination = fs::canonicalize(destination).expect("VR destination must canonicalize");
        let media = destination.join("MDVR-419 Disk-4.mp4");
        fs::write(&media, b"media").expect("VR media must exist");
        let state = VrDownloadState::default();
        state.0.lock().expect("state must lock").future_folder = Some(destination.clone());
        let library_state = VrLibraryState::default();
        let first_scan =
            scan_vr_library_with(&state, &library_state).expect("initial scan must succeed");
        let first_generation = first_scan[0]
            .parse()
            .expect("initial scan generation must be valid");
        let dispatched = Cell::new(false);

        assert_eq!(
            trash_vr_file_with(&media, first_generation, &state, &library_state, |_| {
                dispatched.set(true);
                Ok(())
            }),
            Err(VR_FILE_TRASH_OWNERSHIP_UNAVAILABLE)
        );
        assert_eq!(
            fs::read(&media).expect("media must remain after unavailable ownership"),
            b"media"
        );

        {
            let mut context = state.0.lock().expect("state must lock");
            context.transfers_loaded = true;
            context.persistence_path = Some(fixture.path.join("ownership-downloads"));
        }
        let corrupt_recovery = destination.join(format!(
            "{ORGANIZATION_RECOVERY_PREFIX}{}{ORGANIZATION_RECOVERY_SUFFIX}",
            "0".repeat(40)
        ));
        fs::write(&corrupt_recovery, b"corrupt recovery")
            .expect("corrupt recovery fixture must exist");
        let second_scan =
            scan_vr_library_with(&state, &library_state).expect("second scan must succeed");
        let second_generation = second_scan[0]
            .parse()
            .expect("second scan generation must be valid");
        assert_eq!(
            trash_vr_file_with(&media, second_generation, &state, &library_state, |_| {
                dispatched.set(true);
                Ok(())
            }),
            Err(VR_FILE_TRASH_OWNERSHIP_UNAVAILABLE)
        );
        assert!(!dispatched.get());
        assert_eq!(
            fs::read(&media).expect("media must remain after corrupt recovery"),
            b"media"
        );
        assert_eq!(
            fs::read(&corrupt_recovery).expect("corrupt recovery must remain unchanged"),
            b"corrupt recovery"
        );
        assert!(state
            .0
            .lock()
            .expect("state must lock")
            .transfers
            .is_empty());

        fs::remove_file(&corrupt_recovery).expect("corrupt recovery fixture must be removed");
        for category in [TransferCategory::Adult, TransferCategory::Movie] {
            let corrupt = corrupt_transfer(
                format!("corrupt {} transfer", category.as_str()).as_bytes(),
                0,
                Some(category),
            );
            {
                let mut context = state.0.lock().expect("state must lock");
                context.transfers = vec![StoredTransfer::Corrupt(corrupt)];
            }
            let corrupt_snapshot = transfer_snapshots(&state);
            let corrupt_rows = transfer_rows(&state);
            let corrupt_scan = scan_vr_library_with(&state, &library_state)
                .expect("corrupt shared-category scan must succeed");
            let corrupt_generation = corrupt_scan[0]
                .parse()
                .expect("corrupt shared-category generation must be valid");
            assert_eq!(
                trash_vr_file_with(&media, corrupt_generation, &state, &library_state, |_| {
                    dispatched.set(true);
                    Ok(())
                },),
                Err(VR_FILE_TRASH_OWNERSHIP_UNAVAILABLE)
            );
            assert_eq!(
                fs::read(&media).expect("media must remain after corrupt transfer ownership"),
                b"media"
            );
            assert_eq!(transfer_snapshots(&state), corrupt_snapshot);
            assert_eq!(transfer_rows(&state), corrupt_rows);
        }
        assert!(!dispatched.get());
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    #[test]
    fn permanent_cleanup_is_unavailable_on_unsupported_platforms() {
        let fixture = FilesystemFixture::new();
        let record =
            cancelled_record_for_category(&fixture, TransferCategory::Vr, "non-windows-cleanup");
        let target = current_target(&record, 0).expect("selected target must resolve");
        let persistence_path = fixture.path.join("non-windows.downloads");
        let (state, transfer_id) = cancelled_cleanup_state(record, &persistence_path);
        let before = transfer_snapshots(&state);

        assert_eq!(
            cleanup_cancelled_download(&state, &persistence_path, &transfer_id),
            Err(VR_DOWNLOAD_ACTION_INVALID)
        );
        assert_eq!(transfer_snapshots(&state), before);
        assert_eq!(
            fs::read(target).expect("selected bytes must remain"),
            vec![b'p'; 5]
        );
        assert!(cleanup_recovery_paths(&persistence_path)
            .expect("cleanup recovery directory must be readable")
            .is_empty());
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn permanent_cleanup_capability_requires_an_exact_cancelled_or_cleanup_row() {
        let fixture = FilesystemFixture::new();
        let mut record =
            cancelled_record_for_category(&fixture, TransferCategory::Vr, "capability");
        let state = VrDownloadState::default();
        {
            let mut context = state.0.lock().expect("state must lock");
            context
                .transfers
                .push(StoredTransfer::Valid(record.clone()));
        }
        assert_eq!(transfer_rows(&state)[15], "true");

        for mutation in [
            "terminal-recovery",
            "organized",
            "organization-attention",
            "completed",
        ] {
            record.state = TransferState::Cancelled;
            record.terminal_recovery_generation = None;
            record.organization_state = OrganizationState::None;
            match mutation {
                "terminal-recovery" => record.terminal_recovery_generation = Some(1),
                "organized" => record.organization_state = OrganizationState::Organized,
                "organization-attention" => {
                    record.organization_state = OrganizationState::Attention
                }
                "completed" => record.state = TransferState::Completed,
                _ => unreachable!(),
            }
            state.0.lock().expect("state must lock").transfers =
                vec![StoredTransfer::Valid(record.clone())];
            assert_eq!(transfer_rows(&state)[15], "false");
        }

        record.state = TransferState::Cleanup;
        record.terminal_recovery_generation = None;
        record.organization_state = OrganizationState::None;
        state.0.lock().expect("state must lock").transfers = vec![StoredTransfer::Valid(record)];
        assert_eq!(transfer_rows(&state)[15], "true");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_cleanup_deletes_exact_cancelled_members_for_every_category() {
        for category in [
            TransferCategory::Movie,
            TransferCategory::Tv,
            TransferCategory::Adult,
            TransferCategory::Vr,
        ] {
            let fixture = FilesystemFixture::new();
            let record = cancelled_record_for_category(
                &fixture,
                category,
                &format!("macos-{}-cleanup", category.as_str()),
            );
            let destination = record.destination.clone();
            let target = current_target(&record, 0).expect("selected target must resolve");
            let unrelated = destination.join("unrelated.bin");
            fs::write(&unrelated, b"unrelated").expect("unrelated fixture must exist");
            let persistence_path = fixture.path.join("downloads");
            let (state, transfer_id) = cancelled_cleanup_state(record, &persistence_path);

            assert_eq!(
                cleanup_cancelled_download(&state, &persistence_path, &transfer_id)
                    .expect("macOS cleanup must finish"),
                vec![category.as_str(), "true"]
            );
            assert!(!target.exists());
            assert_eq!(
                fs::read(&unrelated).expect("unrelated bytes must remain"),
                b"unrelated"
            );
            assert!(destination.is_dir());
            assert!(cleanup_recovery_directory(&persistence_path)
                .is_ok_and(|directory| !directory.exists()));
            assert!(read_persisted_transfers(&persistence_path)
                .expect("empty primary must remain valid")
                .is_empty());
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_cleanup_reconciles_every_interrupted_mutation_after_restart() {
        for boundary in [
            None,
            Some(MacosCleanupMutationBoundary::StagingCreated),
            Some(MacosCleanupMutationBoundary::Exchanged),
            Some(MacosCleanupMutationBoundary::ExactDeleted),
            Some(MacosCleanupMutationBoundary::StagingRemoved),
        ] {
            let fixture = FilesystemFixture::new();
            let (persistence_path, record, mut mutation, target) =
                prepared_macos_cleanup_mutation(&fixture, "interrupted-mutation");
            let staging_path = mutation.staging_path.clone();
            write_macos_cleanup_mutation(&persistence_path, &mutation)
                .expect("prepared mutation authority must persist");
            if let Some(boundary) = boundary {
                assert_eq!(
                    run_macos_cleanup_mutation_with(
                        &persistence_path,
                        &mut mutation,
                        write_macos_cleanup_mutation,
                        write_cleanup_recovery,
                        remove_macos_cleanup_mutation,
                        |_, _| Ok(()),
                        |observed| {
                            if observed == boundary {
                                Err(VR_DOWNLOAD_CLEANUP_FAILED)
                            } else {
                                Ok(())
                            }
                        },
                    ),
                    Err(VR_DOWNLOAD_CLEANUP_FAILED)
                );
            }

            let restarted = restarted_macos_cleanup_state(&fixture, &persistence_path, &record);
            cleanup_cancelled_download(&restarted, &persistence_path, &record.transfer_id)
                .expect("interrupted mutation must reconcile");
            assert!(!target.exists());
            assert!(!staging_path.exists());
            assert!(transfer_rows(&restarted).is_empty());
            assert!(
                !macos_cleanup_mutation_path(&persistence_path, &record.transfer_id)
                    .expect("mutation path must resolve")
                    .exists()
            );
            assert!(cleanup_recovery_paths(&persistence_path)
                .expect("cleanup recovery directory must remain readable")
                .is_empty());
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_cleanup_reconciles_every_phase_persistence_failure_after_restart() {
        for failed_phase in [
            MacosCleanupMutationPhase::StagingCreated,
            MacosCleanupMutationPhase::ExchangePrepared,
            MacosCleanupMutationPhase::Exchanged,
            MacosCleanupMutationPhase::ExactDeletionPrepared,
            MacosCleanupMutationPhase::ExactDeleted,
            MacosCleanupMutationPhase::StagingCleanupPrepared,
        ] {
            let fixture = FilesystemFixture::new();
            let (persistence_path, record, mut mutation, target) =
                prepared_macos_cleanup_mutation(&fixture, failed_phase.as_str());
            let staging_path = mutation.staging_path.clone();
            write_macos_cleanup_mutation(&persistence_path, &mutation)
                .expect("prepared mutation authority must persist");
            assert_eq!(
                run_macos_cleanup_mutation_with(
                    &persistence_path,
                    &mut mutation,
                    |path, next| {
                        if next.phase == failed_phase {
                            Err(VR_DOWNLOAD_PERSISTENCE_FAILED)
                        } else {
                            write_macos_cleanup_mutation(path, next)
                        }
                    },
                    write_cleanup_recovery,
                    remove_macos_cleanup_mutation,
                    |_, _| Ok(()),
                    |_| Ok(()),
                ),
                Err(VR_DOWNLOAD_PERSISTENCE_FAILED)
            );

            let restarted = restarted_macos_cleanup_state(&fixture, &persistence_path, &record);
            cleanup_cancelled_download(&restarted, &persistence_path, &record.transfer_id)
                .expect("phase persistence failure must reconcile");
            assert!(!target.exists());
            assert!(!staging_path.exists());
            assert!(transfer_rows(&restarted).is_empty());
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_cleanup_keeps_attention_when_final_cleanup_persistence_fails() {
        for fail_recovery_removal in [false, true] {
            let fixture = FilesystemFixture::new();
            let (persistence_path, record, mut mutation, target) =
                prepared_macos_cleanup_mutation(&fixture, "final-persistence");
            let staging_path = mutation.staging_path.clone();
            write_macos_cleanup_mutation(&persistence_path, &mutation)
                .expect("prepared mutation authority must persist");
            let result = run_macos_cleanup_mutation_with(
                &persistence_path,
                &mut mutation,
                write_macos_cleanup_mutation,
                |path, recovery| {
                    if fail_recovery_removal {
                        write_cleanup_recovery(path, recovery)
                    } else {
                        Err(VR_DOWNLOAD_PERSISTENCE_FAILED)
                    }
                },
                |path, expected| {
                    if fail_recovery_removal {
                        Err(VR_DOWNLOAD_PERSISTENCE_FAILED)
                    } else {
                        remove_macos_cleanup_mutation(path, expected)
                    }
                },
                |_, _| Ok(()),
                |_| Ok(()),
            );
            assert_eq!(result, Err(VR_DOWNLOAD_PERSISTENCE_FAILED));
            assert!(!target.exists());
            assert!(!staging_path.exists());
            assert!(
                macos_cleanup_mutation_path(&persistence_path, &record.transfer_id)
                    .expect("mutation path must resolve")
                    .is_file()
            );

            let restarted = restarted_macos_cleanup_state(&fixture, &persistence_path, &record);
            cleanup_cancelled_download(&restarted, &persistence_path, &record.transfer_id)
                .expect("final persistence failure must remain retryable");
            assert!(transfer_rows(&restarted).is_empty());
            assert!(!staging_path.exists());
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_final_deletion_race_preserves_replacement_without_cleanup_success() {
        let fixture = FilesystemFixture::new();
        let (persistence_path, record, mut mutation, target) =
            prepared_macos_cleanup_mutation(&fixture, "final-deletion-race");
        let staging_path = mutation.staging_path.clone();
        let displaced = record.destination.join("displaced-selected.mp4");
        let unrelated = record.destination.join("unrelated.bin");
        fs::write(&unrelated, b"unrelated").expect("unrelated fixture must exist");
        write_macos_cleanup_mutation(&persistence_path, &mutation)
            .expect("prepared mutation authority must persist");

        assert_eq!(
            run_macos_cleanup_mutation_with(
                &persistence_path,
                &mut mutation,
                write_macos_cleanup_mutation,
                write_cleanup_recovery,
                remove_macos_cleanup_mutation,
                |boundary, path| {
                    if boundary == MacosCleanupPreparationBoundary::ExactDeletion {
                        fs::rename(path, &displaced).map_err(|_| VR_DOWNLOAD_CLEANUP_FAILED)?;
                        fs::write(path, b"replacement").map_err(|_| VR_DOWNLOAD_CLEANUP_FAILED)?;
                    }
                    Ok(())
                },
                |_| Ok(()),
            ),
            Err(VR_DOWNLOAD_STALE)
        );
        assert_eq!(
            fs::read(&staging_path).expect("replacement must survive"),
            b"replacement"
        );
        assert_eq!(
            file_fingerprint(&displaced).expect("selected object must remain identifiable"),
            record.fingerprints[0]
        );
        assert!(target.is_symlink());
        assert_eq!(
            read_cleanup_recoveries(&persistence_path).expect("cleanup recovery must remain")[0]
                .files,
            vec![CleanupFileState::Present]
        );
        assert!(
            macos_cleanup_mutation_path(&persistence_path, &record.transfer_id)
                .expect("mutation path must resolve")
                .is_file()
        );
        let restarted = restarted_macos_cleanup_state(&fixture, &persistence_path, &record);
        assert_eq!(transfer_rows(&restarted)[8], "cleanup");
        assert_eq!(
            fs::read(&staging_path).expect("replacement must survive restart"),
            b"replacement"
        );
        assert_eq!(
            file_fingerprint(&displaced).expect("selected object must survive restart"),
            record.fingerprints[0]
        );
        assert_eq!(
            fs::read(&unrelated).expect("unrelated bytes must survive restart"),
            b"unrelated"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_final_staging_token_race_preserves_replacement_and_restarts_safely() {
        let fixture = FilesystemFixture::new();
        let (persistence_path, record, mut mutation, target) =
            prepared_macos_cleanup_mutation(&fixture, "staging-token-race");
        let staging_path = mutation.staging_path.clone();
        let displaced_token = record.destination.join("displaced-staging-token");
        let unrelated = record.destination.join("unrelated.bin");
        let raced = Cell::new(false);
        fs::write(&unrelated, b"unrelated").expect("unrelated fixture must exist");
        write_macos_cleanup_mutation(&persistence_path, &mutation)
            .expect("prepared mutation authority must persist");

        assert_eq!(
            run_macos_cleanup_mutation_with(
                &persistence_path,
                &mut mutation,
                write_macos_cleanup_mutation,
                write_cleanup_recovery,
                remove_macos_cleanup_mutation,
                |boundary, path| {
                    if boundary == MacosCleanupPreparationBoundary::StagingTokenRemoval
                        && !raced.get()
                    {
                        fs::rename(path, &displaced_token)
                            .map_err(|_| VR_DOWNLOAD_CLEANUP_FAILED)?;
                        fs::write(path, b"replacement").map_err(|_| VR_DOWNLOAD_CLEANUP_FAILED)?;
                        raced.set(true);
                    }
                    Ok(())
                },
                |_| Ok(()),
            ),
            Err(VR_DOWNLOAD_STALE)
        );
        assert!(raced.get());
        assert_eq!(
            fs::read(&target).expect("staging-path replacement must survive"),
            b"replacement"
        );
        assert!(!staging_path.exists());
        assert!(!displaced_token.exists());
        assert_eq!(
            fs::read(&unrelated).expect("unrelated bytes must survive"),
            b"unrelated"
        );
        assert_eq!(
            read_persisted_transfers(&persistence_path)
                .expect("cleanup row must remain durable")
                .len(),
            1
        );
        assert_eq!(
            read_cleanup_recoveries(&persistence_path).expect("cleanup recovery must remain")[0]
                .files,
            vec![CleanupFileState::Present]
        );
        assert_eq!(
            read_macos_cleanup_mutations(&persistence_path)
                .expect("mutation authority must remain")[0]
                .phase,
            MacosCleanupMutationPhase::StagingCleanupPrepared
        );

        let restarted = restarted_macos_cleanup_state(&fixture, &persistence_path, &record);
        assert_eq!(transfer_rows(&restarted)[8], "cleanup");
        cleanup_cancelled_download(&restarted, &persistence_path, &record.transfer_id)
            .expect("explicit retry must finish exact durable cleanup");
        assert_eq!(
            fs::read(&target).expect("replacement must survive retry"),
            b"replacement"
        );
        assert_eq!(
            fs::read(&unrelated).expect("unrelated bytes must survive retry"),
            b"unrelated"
        );
        assert!(transfer_rows(&restarted).is_empty());
        assert!(read_macos_cleanup_mutations(&persistence_path)
            .expect("mutation directory must remain readable")
            .is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn durable_macos_mutations_reserve_every_category_start_until_cleanup_finishes() {
        for after_exact_deletion in [false, true] {
            let fixture = FilesystemFixture::new();
            let label = if after_exact_deletion {
                "exact-deleted-start-reservation"
            } else {
                "prepared-start-reservation"
            };
            let (persistence_path, record, mut mutation, target) =
                prepared_macos_cleanup_mutation(&fixture, label);
            let destination = record.destination.clone();
            let staging_path = mutation.staging_path.clone();
            let staging_relative = staging_path
                .strip_prefix(&destination)
                .expect("staging path must remain inside the shared folder")
                .to_str()
                .expect("fixture staging path must be UTF-8")
                .to_owned();
            write_macos_cleanup_mutation(&persistence_path, &mutation)
                .expect("prepared mutation authority must persist");
            if after_exact_deletion {
                assert_eq!(
                    run_macos_cleanup_mutation_with(
                        &persistence_path,
                        &mut mutation,
                        |path, next| {
                            if next.phase == MacosCleanupMutationPhase::StagingCleanupPrepared {
                                Err(VR_DOWNLOAD_PERSISTENCE_FAILED)
                            } else {
                                write_macos_cleanup_mutation(path, next)
                            }
                        },
                        write_cleanup_recovery,
                        remove_macos_cleanup_mutation,
                        |_, _| Ok(()),
                        |_| Ok(()),
                    ),
                    Err(VR_DOWNLOAD_PERSISTENCE_FAILED)
                );
                assert_eq!(
                    read_macos_cleanup_mutations(&persistence_path)
                        .expect("exact-deleted mutation must remain durable")[0]
                        .phase,
                    MacosCleanupMutationPhase::ExactDeleted
                );
            }
            assert!(!staging_path.exists());

            let state = restarted_macos_cleanup_state(&fixture, &persistence_path, &record);
            {
                let mut context = state.0.lock().expect("state must lock");
                context.future_folder = Some(destination.clone());
                context.adult_future_folder = Some(destination.clone());
                context.movie_future_folder = Some(destination.clone());
                context.tv_future_folder = Some(destination.clone());
            }
            let session_folder = fixture.path.join("blocked-session");
            let product_source = |code: &str, release_name: &str| {
                let metainfo = movie_organization_metainfo(&[(&staging_relative, 5)]);
                let infohash = hex_sha1(&metainfo[b"d4:info".len()..metainfo.len() - 1]);
                revalidate_persisted_download_source(&metainfo, code, release_name, &infohash, &[0])
                    .expect("cross-category source must revalidate")
            };
            let adult_source =
                product_source("ADLT-124", "【Adult】 ADLT-124  Exact — Start reservation");
            let proposals = [
                (
                    TransferCategory::Vr,
                    persistable_fixture_source(),
                    VR_DOWNLOAD_DUPLICATE,
                ),
                (
                    TransferCategory::Vr,
                    product_source("MDVR-420", "【VR】 MDVR-420  Exact — Start reservation"),
                    VR_DOWNLOAD_DESTINATION_CONFLICT,
                ),
                (
                    TransferCategory::Adult,
                    adult_source.clone(),
                    VR_DOWNLOAD_DESTINATION_CONFLICT,
                ),
                (
                    TransferCategory::Movie,
                    movie_organization_source(
                        "Exact Movie",
                        Some("1999-04-19"),
                        "Exact Provider Movie",
                        &[(&staging_relative, 5)],
                        &[0],
                    ),
                    VR_DOWNLOAD_DESTINATION_CONFLICT,
                ),
                (
                    TransferCategory::Tv,
                    tv_download_source(&[(&staging_relative, 5)], &[0]),
                    VR_DOWNLOAD_DESTINATION_CONFLICT,
                ),
            ];
            let primary_before =
                fs::read(&persistence_path).expect("primary authority must remain readable");
            let recovery_path = cleanup_recovery_path(&persistence_path, &record)
                .expect("cleanup recovery path must resolve");
            let recovery_before =
                fs::read(&recovery_path).expect("cleanup recovery must remain readable");
            let mutation_path = macos_cleanup_mutation_path(&persistence_path, &record.transfer_id)
                .expect("mutation path must resolve");
            let mutation_before =
                fs::read(&mutation_path).expect("mutation authority must remain readable");
            let transfers_before = transfer_snapshots(&state);

            tauri::async_runtime::block_on(async {
                for (category, source, expected_error) in proposals {
                    assert_eq!(
                        start_download_source(
                            &state,
                            &persistence_path,
                            &session_folder,
                            category,
                            source,
                        )
                        .await,
                        Err(expected_error),
                        "{} Start ignored durable macOS mutation authority",
                        category.as_str()
                    );
                    assert!(!staging_path.exists());
                    assert_eq!(transfer_snapshots(&state), transfers_before);
                    assert_eq!(
                        fs::read(&persistence_path)
                            .expect("blocked Start must not rewrite primary authority"),
                        primary_before
                    );
                    assert_eq!(
                        fs::read(&recovery_path)
                            .expect("blocked Start must not rewrite cleanup recovery"),
                        recovery_before
                    );
                    assert_eq!(
                        fs::read(&mutation_path)
                            .expect("blocked Start must not rewrite mutation authority"),
                        mutation_before
                    );
                    let context = state.0.lock().expect("state must lock");
                    assert!(context.session.is_none());
                    assert!(!context.session_starting);
                    assert_eq!(context.transfers.len(), 1);
                    assert!(context.transfers.iter().all(|transfer| {
                        matches!(transfer, StoredTransfer::Valid(record) if record.handle.is_none())
                    }));
                    drop(context);
                    assert!(!session_folder.exists());
                }

                cleanup_cancelled_download(&state, &persistence_path, &record.transfer_id)
                    .expect("cleanup retry must remove every durable authority");
                assert!(!target.exists());
                assert!(!staging_path.exists());
                assert!(transfer_rows(&state).is_empty());
                assert!(read_cleanup_recoveries(&persistence_path)
                    .expect("cleanup recovery directory must remain readable")
                    .is_empty());
                assert!(read_macos_cleanup_mutations(&persistence_path)
                    .expect("mutation directory must remain readable")
                    .is_empty());

                let started_transfer_id = start_download_source(
                    &state,
                    &persistence_path,
                    &session_folder,
                    TransferCategory::Adult,
                    adult_source,
                )
                .await
                .expect("Start must become available after durable authority removal");
                assert!(staging_path.is_file());
                assert!(state.0.lock().expect("state must lock").session.is_some());
                cancel_download(&state, &persistence_path, &started_transfer_id)
                    .await
                    .expect("started fixture must cancel without deleting files");
                dismiss_download(&state, &persistence_path, &started_transfer_id)
                    .expect("cancelled fixture metadata must dismiss");
                assert!(staging_path.is_file());
            });
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn corrupt_or_unavailable_macos_mutation_authority_blocks_start_without_side_effects() {
        for unavailable in [false, true] {
            let fixture = FilesystemFixture::new();
            let destination = fixture.path.join("destination");
            fs::create_dir(&destination).expect("destination must exist");
            let destination = fs::canonicalize(destination).expect("destination must canonicalize");
            let persistence_path = fixture.path.join("downloads");
            let session_folder = fixture.path.join("session");
            let mutation_path = macos_cleanup_mutation_path(&persistence_path, &"0".repeat(40))
                .expect("mutation path must resolve");
            fs::create_dir(mutation_path.parent().expect("mutation must have a parent"))
                .expect("mutation directory must exist");
            if unavailable {
                std::os::unix::fs::symlink("missing-mutation-authority", &mutation_path)
                    .expect("unavailable mutation fixture must exist");
            } else {
                fs::write(&mutation_path, b"corrupt macOS cleanup mutation")
                    .expect("corrupt mutation fixture must exist");
            }
            let state = VrDownloadState::default();
            {
                let mut context = state.0.lock().expect("state must lock");
                context.future_folder = Some(destination.clone());
                context.download_limit = DownloadLimitState::Loaded(None);
                context.transfers_loaded = true;
                context.persistence_path = Some(persistence_path.clone());
            }

            assert_eq!(
                tauri::async_runtime::block_on(start_download_source(
                    &state,
                    &persistence_path,
                    &session_folder,
                    TransferCategory::Vr,
                    persistable_fixture_source(),
                )),
                Err(VR_DOWNLOAD_STALE)
            );
            assert!(transfer_rows(&state).is_empty());
            let context = state.0.lock().expect("state must lock");
            assert!(context.session.is_none());
            assert!(!context.session_starting);
            drop(context);
            assert!(!session_folder.exists());
            assert!(!destination.join("Movie  A.mp4").exists());
            assert!(!persistence_path.exists());
            assert!(fs::symlink_metadata(&mutation_path).is_ok());
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_selected_path_replacement_after_exchange_survives_restart_completion() {
        let fixture = FilesystemFixture::new();
        let (persistence_path, record, mut mutation, target) =
            prepared_macos_cleanup_mutation(&fixture, "selected-path-replacement");
        let staging_path = mutation.staging_path.clone();
        let replaced = Cell::new(false);
        write_macos_cleanup_mutation(&persistence_path, &mutation)
            .expect("prepared mutation authority must persist");

        run_macos_cleanup_mutation_with(
            &persistence_path,
            &mut mutation,
            write_macos_cleanup_mutation,
            write_cleanup_recovery,
            remove_macos_cleanup_mutation,
            |_, _| Ok(()),
            |boundary| {
                if boundary == MacosCleanupMutationBoundary::Exchanged && !replaced.get() {
                    fs::remove_file(&target).map_err(|_| VR_DOWNLOAD_CLEANUP_FAILED)?;
                    fs::write(&target, b"replacement").map_err(|_| VR_DOWNLOAD_CLEANUP_FAILED)?;
                    replaced.set(true);
                }
                Ok(())
            },
        )
        .expect("exact staged object must be deleted");
        assert_eq!(
            fs::read(&target).expect("selected-path replacement must survive"),
            b"replacement"
        );
        assert!(!staging_path.exists());

        let restarted = restarted_macos_cleanup_state(&fixture, &persistence_path, &record);
        cleanup_cancelled_download(&restarted, &persistence_path, &record.transfer_id)
            .expect("durably deleted selected object must finalize without touching replacement");
        assert_eq!(
            fs::read(&target).expect("replacement must remain after row removal"),
            b"replacement"
        );
        assert!(transfer_rows(&restarted).is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_exchange_race_rolls_back_without_leaving_an_app_staging_object() {
        for failed_phase in [
            None,
            Some(MacosCleanupMutationPhase::RollbackExchangePrepared),
            Some(MacosCleanupMutationPhase::RolledBack),
            Some(MacosCleanupMutationPhase::RollbackStagingCleanupPrepared),
        ] {
            let fixture = FilesystemFixture::new();
            let (persistence_path, record, mut mutation, target) =
                prepared_macos_cleanup_mutation(&fixture, "exchange-race");
            let staging_path = mutation.staging_path.clone();
            let displaced = record.destination.join("displaced-selected.mp4");
            let raced = Cell::new(false);
            write_macos_cleanup_mutation(&persistence_path, &mutation)
                .expect("prepared mutation authority must persist");

            let result = run_macos_cleanup_mutation_with(
                &persistence_path,
                &mut mutation,
                |path, next| {
                    if failed_phase == Some(next.phase) {
                        Err(VR_DOWNLOAD_PERSISTENCE_FAILED)
                    } else {
                        write_macos_cleanup_mutation(path, next)
                    }
                },
                write_cleanup_recovery,
                remove_macos_cleanup_mutation,
                |boundary, path| {
                    if boundary == MacosCleanupPreparationBoundary::Exchange && !raced.get() {
                        fs::rename(path, &displaced).map_err(|_| VR_DOWNLOAD_CLEANUP_FAILED)?;
                        fs::write(path, b"replacement").map_err(|_| VR_DOWNLOAD_CLEANUP_FAILED)?;
                        raced.set(true);
                    }
                    Ok(())
                },
                |_| Ok(()),
            );
            if failed_phase.is_some() {
                assert_eq!(result, Err(VR_DOWNLOAD_PERSISTENCE_FAILED));
                let mutation_path =
                    macos_cleanup_mutation_path(&persistence_path, &record.transfer_id)
                        .expect("mutation path must resolve");
                let mut recovered = parse_macos_cleanup_mutation(&persistence_path, &mutation_path)
                    .expect("rollback mutation must remain durable");
                assert_eq!(
                    run_macos_cleanup_mutation(&persistence_path, &mut recovered),
                    Err(VR_DOWNLOAD_STALE)
                );
            } else {
                assert_eq!(result, Err(VR_DOWNLOAD_STALE));
            }

            assert_eq!(
                fs::read(&target).expect("selected-path replacement must survive"),
                b"replacement"
            );
            assert_eq!(
                file_fingerprint(&displaced).expect("original selected object must survive"),
                record.fingerprints[0]
            );
            assert!(!staging_path.exists());
            assert!(
                !macos_cleanup_mutation_path(&persistence_path, &record.transfer_id)
                    .expect("mutation path must resolve")
                    .exists()
            );
            assert_eq!(
                read_cleanup_recoveries(&persistence_path).expect("cleanup recovery must remain")
                    [0]
                .files,
                vec![CleanupFileState::Present]
            );
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_final_handle_deletion_preserves_a_path_replacement_and_cleanup_row() {
        let fixture = FilesystemFixture::new();
        let record =
            cancelled_record_for_category(&fixture, TransferCategory::Vr, "windows-handle-race");
        let target = current_target(&record, 0).expect("selected target must resolve");
        let displaced = record.destination.join("displaced-original.mp4");
        let persistence_path = fixture.path.join("downloads");
        let (state, transfer_id) = cancelled_cleanup_state(record, &persistence_path);

        assert_eq!(
            cleanup_cancelled_download_with(
                &state,
                &persistence_path,
                &transfer_id,
                |path, fingerprint| {
                    delete_exact_windows_cleanup_file_with(path, fingerprint, || {
                        fs::rename(path, &displaced)?;
                        fs::write(path, b"replacement")
                    })
                },
                write_cleanup_recovery,
                write_persisted_transfers,
                remove_cleanup_recovery,
            ),
            Err(VR_DOWNLOAD_STALE)
        );
        assert_eq!(
            fs::read(&target).expect("replacement must survive"),
            b"replacement"
        );
        assert!(!displaced.exists());
        assert_eq!(transfer_rows(&state)[8], "cleanup");
        assert_eq!(
            read_cleanup_recoveries(&persistence_path).expect("cleanup recovery must remain")[0]
                .files,
            vec![CleanupFileState::Present]
        );
    }

    #[test]
    fn cleanup_accepts_only_exact_durably_cancelled_or_recovery_rows() {
        for lifecycle in [
            TransferState::Queued,
            TransferState::Downloading,
            TransferState::Paused,
            TransferState::Completed,
            TransferState::Offline,
            TransferState::Failed,
        ] {
            let fixture = FilesystemFixture::new();
            let mut record =
                cancelled_record_for_category(&fixture, TransferCategory::Vr, lifecycle.as_str());
            record.state = lifecycle;
            let target = current_target(&record, 0).expect("selected target must resolve");
            let persistence_path = fixture.path.join("downloads");
            let (state, transfer_id) = cancelled_cleanup_state(record, &persistence_path);
            let before = transfer_snapshots(&state);
            let dispatches = Cell::new(0);

            assert_eq!(
                cleanup_cancelled_download_with(
                    &state,
                    &persistence_path,
                    &transfer_id,
                    |_, _| {
                        dispatches.set(dispatches.get() + 1);
                        Ok(())
                    },
                    write_cleanup_recovery,
                    write_persisted_transfers,
                    remove_cleanup_recovery,
                ),
                Err(VR_DOWNLOAD_ACTION_INVALID)
            );
            assert_eq!(dispatches.get(), 0);
            assert_eq!(transfer_snapshots(&state), before);
            assert!(target.is_file());
            assert!(cleanup_recovery_paths(&persistence_path)
                .expect("cleanup recovery directory must be readable")
                .is_empty());
        }
    }

    #[test]
    fn cancelled_movie_tv_adult_and_vr_cleanup_deletes_only_selected_files() {
        for category in [
            TransferCategory::Movie,
            TransferCategory::Tv,
            TransferCategory::Adult,
            TransferCategory::Vr,
        ] {
            let fixture = FilesystemFixture::new();
            let record = cancelled_record_for_category(
                &fixture,
                category,
                &format!("{}-cleanup", category.as_str()),
            );
            let destination = record.destination.clone();
            let target = current_target(&record, 0).expect("selected target must resolve");
            let unrelated = destination.join("unrelated.bin");
            fs::write(&unrelated, b"unrelated").expect("unrelated fixture must exist");
            let persistence_path = fixture.path.join("downloads");
            let (state, transfer_id) = cancelled_cleanup_state(record, &persistence_path);

            assert_eq!(
                cleanup_cancelled_download_with(
                    &state,
                    &persistence_path,
                    &transfer_id,
                    |path, _| fs::remove_file(path).map_err(|_| VR_DOWNLOAD_CLEANUP_FAILED),
                    write_cleanup_recovery,
                    write_persisted_transfers,
                    remove_cleanup_recovery,
                )
                .expect("cancelled cleanup must finish"),
                vec![category.as_str(), "true"]
            );
            assert!(!target.exists());
            assert_eq!(
                fs::read(&unrelated).expect("unrelated bytes must remain"),
                b"unrelated"
            );
            assert!(destination.is_dir());
            assert!(read_persisted_transfers(&persistence_path)
                .expect("empty primary must remain valid")
                .is_empty());
        }

        let fixture = FilesystemFixture::new();
        let destination = fixture.path.join("selected-members");
        fs::create_dir(&destination).expect("destination must exist");
        let destination = fs::canonicalize(destination).expect("destination must canonicalize");
        let source = movie_organization_source(
            "Exact Movie",
            Some("1999-04-19"),
            "Exact Movie",
            &[
                ("Provider/media.mkv", 5),
                ("Provider/notes.txt", 3),
                ("Provider/deselected.bin", 9),
            ],
            &[0, 1],
        );
        let mut record = completed_organization_record_for_category(
            TransferCategory::Movie,
            &destination,
            source,
        );
        record.state = TransferState::Cancelled;
        record.downloaded_bytes = 6;
        record
            .boundary_segments
            .lock()
            .expect("boundary data must lock")
            .insert(
                2,
                vec![SparseSegment {
                    offset: 1,
                    bytes: vec![7, 8],
                }],
            );
        let media = current_target(&record, 0).expect("media target must resolve");
        let non_media = current_target(&record, 1).expect("non-media target must resolve");
        let deselected = destination.join("Provider/deselected.bin");
        fs::write(&deselected, b"deselected").expect("deselected bytes must exist");
        let persistence_path = fixture.path.join("members.downloads");
        let (state, transfer_id) = cancelled_cleanup_state(record, &persistence_path);

        cleanup_cancelled_download_with(
            &state,
            &persistence_path,
            &transfer_id,
            |path, _| fs::remove_file(path).map_err(|_| VR_DOWNLOAD_CLEANUP_FAILED),
            write_cleanup_recovery,
            write_persisted_transfers,
            remove_cleanup_recovery,
        )
        .expect("selected media and non-media cleanup must finish");
        assert!(!media.exists());
        assert!(!non_media.exists());
        assert_eq!(
            fs::read(deselected).expect("deselected bytes must remain"),
            b"deselected"
        );
    }

    #[test]
    fn cleanup_authority_failures_do_not_dispatch_or_change_cancelled_state() {
        for fail_primary in [false, true] {
            let fixture = FilesystemFixture::new();
            let record = cancelled_record_for_category(
                &fixture,
                TransferCategory::Vr,
                if fail_primary {
                    "primary-failure"
                } else {
                    "recovery-failure"
                },
            );
            let target = current_target(&record, 0).expect("selected target must resolve");
            let persistence_path = fixture.path.join("downloads");
            let (state, transfer_id) = cancelled_cleanup_state(record, &persistence_path);
            let before = transfer_snapshots(&state);
            let dispatches = Cell::new(0);
            let result = if fail_primary {
                cleanup_cancelled_download_with(
                    &state,
                    &persistence_path,
                    &transfer_id,
                    |_, _| {
                        dispatches.set(dispatches.get() + 1);
                        Ok(())
                    },
                    write_cleanup_recovery,
                    |_, _| Err(VR_DOWNLOAD_PERSISTENCE_FAILED),
                    remove_cleanup_recovery,
                )
            } else {
                cleanup_cancelled_download_with(
                    &state,
                    &persistence_path,
                    &transfer_id,
                    |_, _| {
                        dispatches.set(dispatches.get() + 1);
                        Ok(())
                    },
                    |_, _| Err(VR_DOWNLOAD_PERSISTENCE_FAILED),
                    write_persisted_transfers,
                    remove_cleanup_recovery,
                )
            };
            assert_eq!(result, Err(VR_DOWNLOAD_PERSISTENCE_FAILED));
            assert_eq!(dispatches.get(), 0);
            assert_eq!(transfer_snapshots(&state), before);
            assert!(target.is_file());
            assert!(cleanup_recovery_paths(&persistence_path)
                .expect("cleanup recovery directory must be readable")
                .is_empty());
        }
    }

    #[test]
    fn retained_cleanup_recovery_overrides_cancelled_primary_after_restart() {
        let fixture = FilesystemFixture::new();
        let record =
            cancelled_record_for_category(&fixture, TransferCategory::Vr, "retained-authority");
        let target = current_target(&record, 0).expect("selected target must resolve");
        let destination = record.destination.clone();
        let persistence_path = fixture.path.join("downloads");
        let (state, transfer_id) = cancelled_cleanup_state(record, &persistence_path);
        let dispatches = Cell::new(0);

        assert_eq!(
            cleanup_cancelled_download_with(
                &state,
                &persistence_path,
                &transfer_id,
                |_, _| {
                    dispatches.set(dispatches.get() + 1);
                    Ok(())
                },
                write_cleanup_recovery,
                |_, _| Err(VR_DOWNLOAD_PERSISTENCE_FAILED),
                |_, _| Err(VR_DOWNLOAD_PERSISTENCE_FAILED),
            ),
            Err(VR_DOWNLOAD_PERSISTENCE_FAILED)
        );
        assert_eq!(dispatches.get(), 0);
        assert!(target.is_file());
        assert_eq!(transfer_rows(&state)[8], "cleanup");
        assert!(matches!(
            &read_persisted_transfers(&persistence_path)
                .expect("cancelled primary must remain")[0],
            StoredTransfer::Valid(record) if record.state == TransferState::Cancelled
        ));

        let restarted = VrDownloadState::default();
        configure_category_folder(&restarted, TransferCategory::Vr, &destination);
        let rows = tauri::async_runtime::block_on(load_downloads(
            &restarted,
            &persistence_path,
            &fixture.path.join("session"),
            &fixture.path.join("limit"),
        ))
        .expect("retained cleanup recovery must reload");
        assert_eq!(rows[8], "cleanup");
        cleanup_cancelled_download_with(
            &restarted,
            &persistence_path,
            &transfer_id,
            |path, _| fs::remove_file(path).map_err(|_| VR_DOWNLOAD_CLEANUP_FAILED),
            write_cleanup_recovery,
            write_persisted_transfers,
            remove_cleanup_recovery,
        )
        .expect("retained cleanup recovery must remain retryable");
        assert!(!target.exists());
    }

    #[test]
    fn absent_and_durable_recovery_owned_paths_fail_closed_without_dispatch() {
        let fixture = FilesystemFixture::new();
        let record =
            cancelled_record_for_category(&fixture, TransferCategory::Vr, "absent-replacement");
        let target = current_target(&record, 0).expect("selected target must resolve");
        fs::remove_file(&target).expect("selected target must start absent");
        let persistence_path = fixture.path.join("absent.downloads");
        let (state, transfer_id) = cancelled_cleanup_state(record, &persistence_path);
        let recovery_writes = Cell::new(0);
        assert_eq!(
            cleanup_cancelled_download_with(
                &state,
                &persistence_path,
                &transfer_id,
                |_, _| -> Result<(), &'static str> {
                    panic!("an absent-before-cleanup file must not dispatch")
                },
                |path, recovery| {
                    recovery_writes.set(recovery_writes.get() + 1);
                    if recovery_writes.get() == 2 {
                        Err(VR_DOWNLOAD_PERSISTENCE_FAILED)
                    } else {
                        write_cleanup_recovery(path, recovery)
                    }
                },
                write_persisted_transfers,
                remove_cleanup_recovery,
            ),
            Err(VR_DOWNLOAD_PERSISTENCE_FAILED)
        );
        fs::write(&target, b"replacement").expect("absent-path replacement must exist");
        let dispatches = Cell::new(0);
        assert_eq!(
            cleanup_cancelled_download_with(
                &state,
                &persistence_path,
                &transfer_id,
                |_, _| {
                    dispatches.set(dispatches.get() + 1);
                    Ok(())
                },
                write_cleanup_recovery,
                write_persisted_transfers,
                remove_cleanup_recovery,
            ),
            Err(VR_DOWNLOAD_STALE)
        );
        assert_eq!(dispatches.get(), 0);
        assert_eq!(
            fs::read(&target).expect("replacement must survive"),
            b"replacement"
        );

        let fixture = FilesystemFixture::new();
        let destination = fixture.path.join("recovery-owned");
        fs::create_dir(&destination).expect("destination must exist");
        let destination = fs::canonicalize(destination).expect("destination must canonicalize");
        let mut cleanup = completed_organization_record_for_category(
            TransferCategory::Vr,
            &destination,
            persistable_fixture_source(),
        );
        cleanup.state = TransferState::Cancelled;
        let owner = completed_organization_record_for_category(
            TransferCategory::Adult,
            &destination,
            persistable_adult_fixture_source(),
        );
        write_organization_recovery(&owner, &owner.current_paths, None)
            .expect("organization recovery must persist");
        let target = current_target(&cleanup, 0).expect("shared target must resolve");
        let persistence_path = fixture.path.join("owned.downloads");
        let (state, transfer_id) = cancelled_cleanup_state(cleanup, &persistence_path);
        assert_eq!(
            cleanup_cancelled_download_with(
                &state,
                &persistence_path,
                &transfer_id,
                |_, _| {
                    dispatches.set(dispatches.get() + 1);
                    Ok(())
                },
                write_cleanup_recovery,
                write_persisted_transfers,
                remove_cleanup_recovery,
            ),
            Err(VR_DOWNLOAD_STALE)
        );
        assert_eq!(dispatches.get(), 0);
        assert!(target.is_file());
        assert!(organization_recovery_path(&owner).is_file());
    }

    #[test]
    fn interrupted_cleanup_restarts_with_exact_remaining_files_and_boundary_data() {
        let fixture = FilesystemFixture::new();
        let destination = fixture.path.join("restart-cleanup");
        fs::create_dir(&destination).expect("destination must exist");
        let destination = fs::canonicalize(destination).expect("destination must canonicalize");
        let source = movie_organization_source(
            "Exact Movie",
            Some("1999-04-19"),
            "Exact Movie",
            &[
                ("Provider/media.mkv", 5),
                ("Provider/notes.txt", 3),
                ("Provider/deselected.bin", 9),
            ],
            &[0, 1],
        );
        let mut record = completed_organization_record_for_category(
            TransferCategory::Movie,
            &destination,
            source,
        );
        record.state = TransferState::Cancelled;
        record
            .boundary_segments
            .lock()
            .expect("boundary data must lock")
            .insert(
                2,
                vec![SparseSegment {
                    offset: 3,
                    bytes: vec![4, 5, 6],
                }],
            );
        let deleted = current_target(&record, 0).expect("first target must resolve");
        let remaining = current_target(&record, 1).expect("second target must resolve");
        let deselected = destination.join("Provider/deselected.bin");
        fs::write(&deselected, b"deselected").expect("deselected bytes must exist");
        let persistence_path = fixture.path.join("downloads");
        let (state, transfer_id) = cancelled_cleanup_state(record, &persistence_path);
        let dispatches = Cell::new(0);

        assert_eq!(
            cleanup_cancelled_download_with(
                &state,
                &persistence_path,
                &transfer_id,
                |path, _| {
                    dispatches.set(dispatches.get() + 1);
                    if dispatches.get() == 2 {
                        return Err(VR_DOWNLOAD_CLEANUP_FAILED);
                    }
                    fs::remove_file(path).map_err(|_| VR_DOWNLOAD_CLEANUP_FAILED)
                },
                write_cleanup_recovery,
                write_persisted_transfers,
                remove_cleanup_recovery,
            ),
            Err(VR_DOWNLOAD_CLEANUP_FAILED)
        );
        assert!(!deleted.exists());
        assert!(remaining.is_file());
        let durable = read_cleanup_recoveries(&persistence_path)
            .expect("partial cleanup recovery must remain");
        assert_eq!(
            durable[0].files,
            vec![CleanupFileState::Deleted, CleanupFileState::Present]
        );
        assert_eq!(
            encoded_boundary_segments(&durable[0].record).expect("boundary data must encode"),
            "2:3:040506"
        );

        fs::write(&deleted, b"replacement").expect("replacement must exist");
        let restarted = VrDownloadState::default();
        configure_movie_download_folder(&restarted, Some(destination.clone()))
            .expect("Movies folder must configure");
        let rows = tauri::async_runtime::block_on(load_downloads_with_persistence(
            &restarted,
            &persistence_path,
            &fixture.path.join("session"),
            &fixture.path.join("limit"),
            |_, _| Err(VR_DOWNLOAD_PERSISTENCE_FAILED),
        ))
        .expect("cleanup recovery must remain visible during primary failure");
        assert_eq!(rows[8], "cleanup");
        let retry_dispatches = Cell::new(0);
        assert_eq!(
            cleanup_cancelled_download_with(
                &restarted,
                &persistence_path,
                &transfer_id,
                |_, _| {
                    retry_dispatches.set(retry_dispatches.get() + 1);
                    Ok(())
                },
                write_cleanup_recovery,
                write_persisted_transfers,
                remove_cleanup_recovery,
            ),
            Err(VR_DOWNLOAD_STALE)
        );
        assert_eq!(retry_dispatches.get(), 0);
        assert_eq!(
            fs::read(&deleted).expect("replacement must survive"),
            b"replacement"
        );
        assert!(remaining.is_file());
        fs::remove_file(&deleted).expect("replacement fixture must be removed");

        cleanup_cancelled_download_with(
            &restarted,
            &persistence_path,
            &transfer_id,
            |path, _| fs::remove_file(path).map_err(|_| VR_DOWNLOAD_CLEANUP_FAILED),
            write_cleanup_recovery,
            write_persisted_transfers,
            remove_cleanup_recovery,
        )
        .expect("exact remaining cleanup must finish");
        assert!(!remaining.exists());
        assert_eq!(
            fs::read(deselected).expect("deselected bytes must remain"),
            b"deselected"
        );
        assert!(read_persisted_transfers(&persistence_path)
            .expect("empty primary must remain valid")
            .is_empty());
    }

    #[test]
    fn cleanup_rejects_replacements_and_cross_owned_paths_without_dispatch() {
        let fixture = FilesystemFixture::new();
        let record =
            cancelled_record_for_category(&fixture, TransferCategory::Vr, "replacement-cleanup");
        let target = current_target(&record, 0).expect("selected target must resolve");
        let persistence_path = fixture.path.join("replacement.downloads");
        let (state, transfer_id) = cancelled_cleanup_state(record, &persistence_path);
        fs::remove_file(&target).expect("original must be removed");
        fs::write(&target, b"replacement").expect("replacement must exist");
        let dispatches = Cell::new(0);
        assert_eq!(
            cleanup_cancelled_download_with(
                &state,
                &persistence_path,
                &transfer_id,
                |_, _| {
                    dispatches.set(dispatches.get() + 1);
                    Ok(())
                },
                write_cleanup_recovery,
                write_persisted_transfers,
                remove_cleanup_recovery,
            ),
            Err(VR_DOWNLOAD_STALE)
        );
        assert_eq!(dispatches.get(), 0);
        assert_eq!(
            fs::read(&target).expect("replacement must survive"),
            b"replacement"
        );

        let fixture = FilesystemFixture::new();
        let destination = fixture.path.join("shared");
        fs::create_dir(&destination).expect("shared destination must exist");
        let destination = fs::canonicalize(destination).expect("destination must canonicalize");
        let mut cleanup = completed_organization_record_for_category(
            TransferCategory::Vr,
            &destination,
            persistable_fixture_source(),
        );
        cleanup.state = TransferState::Cancelled;
        let mut owner = completed_organization_record_for_category(
            TransferCategory::Adult,
            &destination,
            persistable_adult_fixture_source(),
        );
        owner.state = TransferState::Cancelled;
        let target = current_target(&cleanup, 0).expect("shared target must resolve");
        let transfer_id = cleanup.transfer_id.clone();
        let persistence_path = fixture.path.join("shared.downloads");
        write_persisted_transfers(
            &persistence_path,
            &[
                StoredTransfer::Valid(cleanup.clone()),
                StoredTransfer::Valid(owner.clone()),
            ],
        )
        .expect("shared ownership must persist");
        let state = VrDownloadState::default();
        {
            let mut context = state.0.lock().expect("state must lock");
            context.future_folder = Some(destination.clone());
            context.adult_future_folder = Some(destination);
            context.transfers_loaded = true;
            context.persistence_path = Some(persistence_path.clone());
            context
                .transfers
                .extend([StoredTransfer::Valid(cleanup), StoredTransfer::Valid(owner)]);
        }
        let before = transfer_snapshots(&state);
        assert_eq!(
            cleanup_cancelled_download_with(
                &state,
                &persistence_path,
                &transfer_id,
                |_, _| {
                    dispatches.set(dispatches.get() + 1);
                    Ok(())
                },
                write_cleanup_recovery,
                write_persisted_transfers,
                remove_cleanup_recovery,
            ),
            Err(VR_DOWNLOAD_STALE)
        );
        assert_eq!(dispatches.get(), 0);
        assert_eq!(transfer_snapshots(&state), before);
        assert_eq!(
            fs::read(target).expect("shared bytes must remain"),
            vec![b'a'; 5]
        );
    }

    #[test]
    fn cleanup_replacement_race_and_global_reservation_preserve_all_rows() {
        let fixture = FilesystemFixture::new();
        let record = cancelled_record_for_category(&fixture, TransferCategory::Vr, "deletion-race");
        let target = current_target(&record, 0).expect("selected target must resolve");
        let persistence_path = fixture.path.join("race.downloads");
        let (state, transfer_id) = cancelled_cleanup_state(record, &persistence_path);
        {
            state.0.lock().expect("state must lock").cleanup_transfer_id =
                Some("another-transfer".to_owned());
        }
        let before = transfer_snapshots(&state);
        let dispatches = Cell::new(0);
        assert_eq!(
            cleanup_cancelled_download_with(
                &state,
                &persistence_path,
                &transfer_id,
                |_, _| {
                    dispatches.set(dispatches.get() + 1);
                    Ok(())
                },
                write_cleanup_recovery,
                write_persisted_transfers,
                remove_cleanup_recovery,
            ),
            Err(VR_DOWNLOAD_ACTION_INVALID)
        );
        assert_eq!(dispatches.get(), 0);
        assert_eq!(transfer_snapshots(&state), before);
        state.0.lock().expect("state must lock").cleanup_transfer_id = None;

        assert_eq!(
            cleanup_cancelled_download_with(
                &state,
                &persistence_path,
                &transfer_id,
                |path, _| {
                    fs::remove_file(path).map_err(|_| VR_DOWNLOAD_CLEANUP_FAILED)?;
                    fs::write(path, b"replacement").map_err(|_| VR_DOWNLOAD_CLEANUP_FAILED)
                },
                write_cleanup_recovery,
                write_persisted_transfers,
                remove_cleanup_recovery,
            ),
            Err(VR_DOWNLOAD_STALE)
        );
        assert_eq!(
            fs::read(&target).expect("replacement must survive"),
            b"replacement"
        );
        assert_eq!(transfer_rows(&state)[8], "cleanup");
        assert_eq!(
            read_cleanup_recoveries(&persistence_path).expect("cleanup recovery must remain")[0]
                .files,
            vec![CleanupFileState::Present]
        );
    }

    #[test]
    fn final_primary_and_recovery_removal_failures_restart_as_retryable_cleanup() {
        for fail_recovery_removal in [false, true] {
            let fixture = FilesystemFixture::new();
            let record = cancelled_record_for_category(
                &fixture,
                TransferCategory::Vr,
                if fail_recovery_removal {
                    "recovery-removal-failure"
                } else {
                    "final-primary-failure"
                },
            );
            let target = current_target(&record, 0).expect("selected target must resolve");
            let persistence_path = fixture.path.join("downloads");
            let (state, transfer_id) = cancelled_cleanup_state(record, &persistence_path);
            let primary_writes = Cell::new(0);
            let result = cleanup_cancelled_download_with(
                &state,
                &persistence_path,
                &transfer_id,
                |path, _| fs::remove_file(path).map_err(|_| VR_DOWNLOAD_CLEANUP_FAILED),
                write_cleanup_recovery,
                |path, transfers| {
                    primary_writes.set(primary_writes.get() + 1);
                    if !fail_recovery_removal && primary_writes.get() == 2 {
                        Err(VR_DOWNLOAD_PERSISTENCE_FAILED)
                    } else {
                        write_persisted_transfers(path, transfers)
                    }
                },
                |path, recovery| {
                    if fail_recovery_removal {
                        Err(VR_DOWNLOAD_PERSISTENCE_FAILED)
                    } else {
                        remove_cleanup_recovery(path, recovery)
                    }
                },
            );
            assert_eq!(result, Err(VR_DOWNLOAD_PERSISTENCE_FAILED));
            assert!(!target.exists());
            assert_eq!(transfer_rows(&state)[8], "cleanup");

            let restarted = VrDownloadState::default();
            let destination = state
                .0
                .lock()
                .expect("state must lock")
                .transfers
                .iter()
                .find_map(|transfer| match transfer {
                    StoredTransfer::Valid(record) => Some(record.destination.clone()),
                    StoredTransfer::Corrupt(_) => None,
                })
                .expect("cleanup row must remain");
            configure_category_folder(&restarted, TransferCategory::Vr, &destination);
            let rows = tauri::async_runtime::block_on(load_downloads(
                &restarted,
                &persistence_path,
                &fixture.path.join("session"),
                &fixture.path.join("limit"),
            ))
            .expect("durable cleanup must reload");
            assert_eq!(rows[8], "cleanup");
            let retry_dispatches = Cell::new(0);
            cleanup_cancelled_download_with(
                &restarted,
                &persistence_path,
                &transfer_id,
                |_, _| {
                    retry_dispatches.set(retry_dispatches.get() + 1);
                    Ok(())
                },
                write_cleanup_recovery,
                write_persisted_transfers,
                remove_cleanup_recovery,
            )
            .expect("restart retry must remove durable row");
            assert_eq!(retry_dispatches.get(), 0);
            assert!(read_persisted_transfers(&persistence_path)
                .expect("empty primary must remain valid")
                .is_empty());
            assert!(cleanup_recovery_paths(&persistence_path)
                .expect("cleanup recovery directory must be readable")
                .is_empty());
        }
    }

    #[test]
    fn cleanup_authority_reserves_every_category_start_until_durable_removal() {
        let fixture = FilesystemFixture::new();
        let destination = fixture.path.join("shared-cleanup-start");
        fs::create_dir(&destination).expect("shared destination must exist");
        let destination = fs::canonicalize(destination).expect("destination must canonicalize");
        let vr_source = persistable_fixture_source();
        let mut cleanup = completed_organization_record_for_category(
            TransferCategory::Vr,
            &destination,
            vr_source.clone(),
        );
        cleanup.state = TransferState::Cancelled;
        let target = current_target(&cleanup, 0).expect("selected target must resolve");
        let persistence_path = fixture.path.join("downloads");
        let session_folder = fixture.path.join("session");
        let (state, transfer_id) = cancelled_cleanup_state(cleanup, &persistence_path);
        {
            let mut context = state.0.lock().expect("state must lock");
            context.future_folder = Some(destination.clone());
            context.adult_future_folder = Some(destination.clone());
            context.movie_future_folder = Some(destination.clone());
            context.tv_future_folder = Some(destination.clone());
            context.download_limit = DownloadLimitState::Loaded(None);
        }

        assert_eq!(
            cleanup_cancelled_download_with(
                &state,
                &persistence_path,
                &transfer_id,
                |path, _| fs::remove_file(path).map_err(|_| VR_DOWNLOAD_CLEANUP_FAILED),
                write_cleanup_recovery,
                write_persisted_transfers,
                |_, _| Err(VR_DOWNLOAD_PERSISTENCE_FAILED),
            ),
            Err(VR_DOWNLOAD_PERSISTENCE_FAILED)
        );
        assert!(!target.exists());
        assert_eq!(transfer_rows(&state)[8], "cleanup");
        let recovery_path = cleanup_recovery_paths(&persistence_path)
            .expect("cleanup recovery paths must remain readable")
            .into_iter()
            .next()
            .expect("cleanup recovery must remain durable");
        let primary_before =
            fs::read(&persistence_path).expect("primary state must remain readable");
        let recovery_before =
            fs::read(&recovery_path).expect("recovery state must remain readable");
        let transfers_before = transfer_snapshots(&state);

        let proposals = [
            (
                TransferCategory::Vr,
                vr_source.clone(),
                VR_DOWNLOAD_DUPLICATE,
            ),
            (
                TransferCategory::Adult,
                persistable_adult_fixture_source(),
                VR_DOWNLOAD_DESTINATION_CONFLICT,
            ),
            (
                TransferCategory::Movie,
                movie_organization_source(
                    "Exact Movie",
                    Some("1999-04-19"),
                    "Exact Provider Movie",
                    &[("Movie  A.mp4", 5)],
                    &[0],
                ),
                VR_DOWNLOAD_DESTINATION_CONFLICT,
            ),
            (
                TransferCategory::Tv,
                tv_download_source(&[("Movie  A.mp4", 5)], &[0]),
                VR_DOWNLOAD_DESTINATION_CONFLICT,
            ),
        ];
        tauri::async_runtime::block_on(async {
            for (category, source, expected_error) in proposals {
                assert_eq!(
                    start_download_source(
                        &state,
                        &persistence_path,
                        &session_folder,
                        category,
                        source,
                    )
                    .await,
                    Err(expected_error),
                    "{} Start ignored cleanup authority",
                    category.as_str()
                );
                assert!(!target.exists());
                assert_eq!(transfer_snapshots(&state), transfers_before);
                assert_eq!(
                    fs::read(&persistence_path).expect("blocked Start must not rewrite primary"),
                    primary_before
                );
                assert_eq!(
                    fs::read(&recovery_path).expect("blocked Start must not rewrite recovery"),
                    recovery_before
                );
                let context = state.0.lock().expect("state must lock");
                assert_eq!(context.transfers.len(), 1);
                assert!(context.session.is_none());
                assert!(!context.session_starting);
                assert!(context.transfers.iter().all(|transfer| {
                    matches!(transfer, StoredTransfer::Valid(record) if record.handle.is_none())
                }));
                drop(context);
                assert!(!session_folder.exists());
            }

            cleanup_cancelled_download_with(
                &state,
                &persistence_path,
                &transfer_id,
                |_, _| -> Result<(), &'static str> {
                    panic!("durably deleted files must not be dispatched again")
                },
                write_cleanup_recovery,
                write_persisted_transfers,
                remove_cleanup_recovery,
            )
            .expect("cleanup retry must durably remove its exact authority");
            assert!(transfer_rows(&state).is_empty());
            assert!(read_persisted_transfers(&persistence_path)
                .expect("empty primary must remain readable")
                .is_empty());
            assert!(cleanup_recovery_paths(&persistence_path)
                .expect("cleanup recovery directory must remain readable")
                .is_empty());

            let started_transfer_id = start_download_source(
                &state,
                &persistence_path,
                &session_folder,
                TransferCategory::Vr,
                vr_source,
            )
            .await
            .expect("Start must become available after durable cleanup removal");
            assert_eq!(started_transfer_id, transfer_id);
            assert!(target.is_file());
            assert_eq!(transfer_rows(&state).len(), 16);
            assert!(state.0.lock().expect("state must lock").session.is_some());
            cancel_download(&state, &persistence_path, &started_transfer_id)
                .await
                .expect("replacement transfer must cancel without deleting files");
        });
    }

    #[test]
    fn corrupt_or_unavailable_cleanup_ownership_blocks_start_without_side_effects() {
        for unavailable_directory in [false, true] {
            let fixture = FilesystemFixture::new();
            let destination = fixture.path.join("destination");
            fs::create_dir(&destination).expect("destination must exist");
            let destination = fs::canonicalize(destination).expect("destination must canonicalize");
            let persistence_path = fixture.path.join("downloads");
            let session_folder = fixture.path.join("session");
            let recovery_directory =
                cleanup_recovery_directory(&persistence_path).expect("recovery path must resolve");
            if unavailable_directory {
                fs::write(&recovery_directory, b"not a directory")
                    .expect("unavailable recovery directory fixture must exist");
            } else {
                fs::create_dir(&recovery_directory).expect("cleanup recovery directory must exist");
                fs::write(
                    recovery_directory.join(format!(
                        "{CLEANUP_RECOVERY_PREFIX}{}{TERMINAL_RECOVERY_SUFFIX}",
                        "0".repeat(40)
                    )),
                    b"corrupt cleanup recovery",
                )
                .expect("corrupt cleanup recovery must exist");
            }
            let state = VrDownloadState::default();
            {
                let mut context = state.0.lock().expect("state must lock");
                context.future_folder = Some(destination.clone());
                context.download_limit = DownloadLimitState::Loaded(None);
                context.transfers_loaded = true;
                context.persistence_path = Some(persistence_path.clone());
            }

            assert_eq!(
                tauri::async_runtime::block_on(start_download_source(
                    &state,
                    &persistence_path,
                    &session_folder,
                    TransferCategory::Vr,
                    persistable_fixture_source(),
                )),
                Err(VR_DOWNLOAD_STALE)
            );
            assert!(transfer_rows(&state).is_empty());
            assert!(state.0.lock().expect("state must lock").session.is_none());
            assert!(!session_folder.exists());
            assert!(!destination.join("Movie  A.mp4").exists());
            assert!(!persistence_path.exists());
        }
    }

    #[test]
    fn native_engine_resumes_only_selected_files_and_cancel_keeps_partial_data() {
        let fixture = FilesystemFixture::new();
        let destination = fixture.path.join("VR — 作品");
        fs::create_dir_all(&destination).expect("destination must exist");
        let destination = fs::canonicalize(destination).expect("destination must canonicalize");
        let persistence_path = fixture.path.join("downloads");
        let session_folder = fixture.path.join("session");
        let download_limit_path = fixture.path.join("download-limit");
        let folder_path = fixture.path.join("vr-folder");
        let bytes = selected_file_torrent();
        let infohash = hex_sha1(&bytes[b"d4:info".len()..bytes.len() - 1]);
        let exact_release_name = "【VR】 MDVR-419  Exact — 特別版";
        let torrent_state = VrTorrentState::default();
        let release_generation = torrent_state
            .begin_release_lookup()
            .expect("release lookup must start");
        torrent_state
            .finish_release_lookup(
                release_generation,
                "MDVR-419",
                &format!(
                    "<rss><channel><item><title>{exact_release_name}</title><guid>https://sukebei.nyaa.si/view/123</guid><link>https://sukebei.nyaa.si/download/123.torrent</link><nyaa:infoHash>{infohash}</nyaa:infoHash></item></channel></rss>"
                ),
            )
            .expect("trusted feed must be stored");
        let inspection = crate::vr_torrent::inspect_sukebei_torrent_with(
            &torrent_state,
            crate::vr_torrent::TorrentInspectionRequest {
                code: "MDVR-419".to_owned(),
                release_name: exact_release_name.to_owned(),
                provider_item_id: "123".to_owned(),
                torrent_url: "https://sukebei.nyaa.si/download/123.torrent".to_owned(),
                expected_infohash: infohash,
            },
            |_| {
                Ok(crate::vr_torrent::ArtifactResponse {
                    status: 200,
                    redirect_url: None,
                    body: bytes.clone(),
                })
            },
        )
        .expect("trusted torrent must inspect");
        let state = VrDownloadState::default();

        tauri::async_runtime::block_on(async {
            load_downloads(
                &state,
                &persistence_path,
                &session_folder,
                &download_limit_path,
            )
            .await
            .expect("empty transfer state must load");
            set_vr_folder(&state, &folder_path, destination.clone())
                .expect("trusted destination must configure");
            let transfer_id = start_download(
                &state,
                &torrent_state,
                &persistence_path,
                &session_folder,
                &inspection[0],
                &[1],
            )
            .await
            .expect("selected transfer must start from local fixtures");

            assert!(!destination.join("Folder/Part  1 — 映画.mkv").exists());
            assert!(destination.join("Folder/特別版  B.mp4").is_file());
            assert_eq!(
                start_download(
                    &state,
                    &torrent_state,
                    &persistence_path,
                    &session_folder,
                    &inspection[0],
                    &[1],
                )
                .await,
                Err(VR_DOWNLOAD_DUPLICATE)
            );
            pause_download(&state, &persistence_path, &transfer_id)
                .await
                .expect("active local transfer must pause");
            let (old_session, old_handle) = {
                let mut context = state.0.lock().expect("state must lock");
                let session = context.session.clone().expect("session must exist");
                let record = find_valid_record_mut(&mut context.transfers, &transfer_id)
                    .expect("paused transfer must exist");
                record.handle_generation = record.handle_generation.wrapping_add(1);
                let handle = record.handle.take().expect("paused handle must exist");
                write_persisted_transfers(&persistence_path, &context.transfers)
                    .expect("paused transfer must persist");
                (session, handle)
            };
            old_session
                .delete(old_handle.id().into(), false)
                .await
                .expect("fixture session must detach without deleting files");

            let resumed_state = VrDownloadState::default();
            let resumed_rows = load_downloads(
                &resumed_state,
                &persistence_path,
                &fixture.path.join("resumed-session"),
                &download_limit_path,
            )
            .await
            .expect("valid paused transfer must restore");
            assert_eq!(resumed_rows[8], "paused");
            assert!(!destination.join("Folder/Part  1 — 映画.mkv").exists());
            resume_download(&resumed_state, &persistence_path, &transfer_id)
                .await
                .expect("restored partial transfer must resume");
            cancel_download(&resumed_state, &persistence_path, &transfer_id)
                .await
                .expect("resumed local transfer must cancel");
            assert!(destination.join("Folder/特別版  B.mp4").is_file());
            dismiss_download(&resumed_state, &persistence_path, &transfer_id)
                .expect("cancelled local transfer must dismiss");
            assert!(destination.join("Folder/特別版  B.mp4").is_file());
        });
    }
}
