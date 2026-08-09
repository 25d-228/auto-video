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

use crate::vr_library::is_supported_media;
use crate::vr_torrent::{
    adult_media_name_matches_product_code, hex_sha1, media_name_matches_product_code,
    revalidate_persisted_download_source, revalidate_persisted_movie_download_source,
    AdultTorrentState, MovieDownloadIdentity, MovieTorrentState, VerifiedDownloadFile,
    VerifiedDownloadSource, VerifiedDownloadSourceError, VrTorrentState,
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
const ORGANIZATION_RECOVERY_PREFIX: &str = ".auto-video-organization-";
const ORGANIZATION_RECOVERY_SUFFIX: &str = ".recovery";
const ORGANIZATION_RECOVERY_SUCCESSOR_SUFFIX: &str = ".recovery.next";
const TERMINAL_RECOVERY_PREFIX: &str = ".auto-video-transfer-terminal-";
const TERMINAL_RECOVERY_SUFFIX: &str = ".recovery";
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

type ManagedTorrentHandle = Arc<ManagedTorrent>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransferCategory {
    Adult,
    Movie,
    Vr,
}

impl TransferCategory {
    fn as_str(self) -> &'static str {
        match self {
            Self::Adult => "adult",
            Self::Movie => "movie",
            Self::Vr => "vr",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "adult" => Some(Self::Adult),
            "movie" => Some(Self::Movie),
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

struct TransferRecord {
    transfer_id: String,
    category: TransferCategory,
    code: String,
    release_name: String,
    movie_identity: Option<Box<MovieDownloadIdentity>>,
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
    session: Option<Arc<Session>>,
    session_starting: bool,
    download_limit: DownloadLimitState,
    transfers_loaded: bool,
    transfers_loading: bool,
    transfers: Vec<StoredTransfer>,
    organization_generation: u64,
    organization_plan: Option<OrganizationPlan>,
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

pub(crate) fn with_unowned_vr_library_path<T>(
    state: &VrDownloadState,
    requested_path: &Path,
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
    if let Some(folder) = context.future_folder.as_deref() {
        let folder_is_available = fs::canonicalize(folder).ok().as_deref() == Some(folder)
            && fs::metadata(folder).is_ok_and(|metadata| metadata.is_dir());
        if folder_is_available && durable_recovery_owns_path(folder, requested_path)? {
            return Err(VrLibraryTrashOwnershipError::Owned);
        }
    }
    Ok(operation(context.future_folder.as_deref()))
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
    let (fields, boundary_segments, organization_state, current_paths, category, movie_identity) =
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
                    TransferCategory::Vr,
                    None,
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
                    TransferCategory::Vr,
                    None,
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
                    TransferCategory::Vr,
                    None,
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
                    TransferCategory::from_str(std::str::from_utf8(category).ok()?)?,
                    None,
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
                    TransferCategory::Movie,
                    Some(Box::new(decode_movie_identity(&decode_hex(
                        movie_identity,
                    )?)?)),
                )
            }
            _ => return None,
        };
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
            if !code.is_empty() || release_name != identity.tmdb_title {
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
        TransferCategory::Adult | TransferCategory::Vr => {
            if movie_identity.is_some() {
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
    if record.category == TransferCategory::Movie
        && record.organization_state != OrganizationState::None
    {
        let eligible_media = record
            .selected_files
            .iter()
            .filter(|file| is_supported_media(Path::new(&file.path)))
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

fn write_persisted_transfers(
    path: &Path,
    transfers: &[StoredTransfer],
) -> Result<(), &'static str> {
    let parent = path.parent().ok_or(VR_DOWNLOAD_PERSISTENCE_FAILED)?;
    fs::create_dir_all(parent).map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
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
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
        .map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)
}

fn terminal_recovery_path(record: &TransferRecord) -> PathBuf {
    record.destination.join(format!(
        "{TERMINAL_RECOVERY_PREFIX}{}{TERMINAL_RECOVERY_SUFFIX}",
        record.transfer_id
    ))
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

fn write_terminal_recovery(record: &TransferRecord, generation: u64) -> Result<(), &'static str> {
    let path = terminal_recovery_path(record);
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
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
            file.write_all(&bytes)
                .and_then(|()| file.sync_all())
                .map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)
        }
        Err(_) => Err(VR_DOWNLOAD_PERSISTENCE_FAILED),
    }
}

fn parse_terminal_recovery(path: &Path) -> Option<TransferRecord> {
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
        || terminal_recovery_path(&record) != path
        || validate_resume_context(&record).is_err()
    {
        return None;
    }
    record.terminal_recovery_generation = Some(generation);
    Some(record)
}

fn read_terminal_recoveries(destination: &Path) -> Vec<TransferRecord> {
    let Ok(entries) = fs::read_dir(destination) else {
        return Vec::new();
    };
    let mut paths = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            let transfer_id = name
                .strip_prefix(TERMINAL_RECOVERY_PREFIX)?
                .strip_suffix(TERMINAL_RECOVERY_SUFFIX)?;
            (transfer_id.len() == 40 && transfer_id.bytes().all(|byte| byte.is_ascii_hexdigit()))
                .then_some(entry.path())
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths.truncate(MAX_PERSISTED_TRANSFERS);
    paths
        .into_iter()
        .filter_map(|path| parse_terminal_recovery(&path))
        .collect()
}

fn same_transfer_authority(left: &TransferRecord, right: &TransferRecord) -> bool {
    left.transfer_id == right.transfer_id
        && left.category == right.category
        && left.code == right.code
        && left.release_name == right.release_name
        && left.movie_identity == right.movie_identity
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

fn remove_terminal_recovery(record: &TransferRecord) -> Result<(), &'static str> {
    let path = terminal_recovery_path(record);
    let recovered = match fs::symlink_metadata(&path) {
        Ok(_) => parse_terminal_recovery(&path).ok_or(VR_DOWNLOAD_PERSISTENCE_FAILED)?,
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
    fs::remove_file(path).map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)
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
        .filter(|file| is_supported_media(Path::new(&file.path)))
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
        || plan.plan_id != organization_plan_id(plan.generation, record, &plan.entries)
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

fn durable_recovery_owns_path(
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
        let organization_recovery = name.starts_with(ORGANIZATION_RECOVERY_PREFIX)
            && (name.ends_with(ORGANIZATION_RECOVERY_SUFFIX)
                || name.ends_with(ORGANIZATION_RECOVERY_SUCCESSOR_SUFFIX));
        let terminal_recovery =
            name.starts_with(TERMINAL_RECOVERY_PREFIX) && name.ends_with(TERMINAL_RECOVERY_SUFFIX);
        if !organization_recovery && !terminal_recovery {
            continue;
        }
        recovery_count += 1;
        if recovery_count > MAX_PERSISTED_TRANSFERS * 3 {
            return Err(VrLibraryTrashOwnershipError::Unavailable);
        }
        let record = if terminal_recovery {
            parse_terminal_recovery(&entry.path())
        } else {
            parse_organization_recovery_file(&entry.path())
        }
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
fn file_fingerprint(path: &Path) -> Result<String, &'static str> {
    use std::os::unix::fs::MetadataExt;

    let metadata = fs::symlink_metadata(path).map_err(|_| VR_FOLDER_UNAVAILABLE)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(VR_DOWNLOAD_DESTINATION_CONFLICT);
    }
    Ok(format!("{}:{}", metadata.dev(), metadata.ino()))
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
}

#[cfg(target_os = "windows")]
fn file_fingerprint(path: &Path) -> Result<String, &'static str> {
    use std::{mem::MaybeUninit, os::windows::io::AsRawHandle};

    let metadata = fs::symlink_metadata(path).map_err(|_| VR_FOLDER_UNAVAILABLE)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(VR_DOWNLOAD_DESTINATION_CONFLICT);
    }
    let file = File::open(path).map_err(|_| VR_FOLDER_UNAVAILABLE)?;
    let mut information = MaybeUninit::<WindowsFileInformation>::uninit();
    // Rust's equivalent metadata methods are unstable, so use the stable Windows handle API.
    let succeeded = unsafe {
        get_file_information_by_handle(file.as_raw_handle().cast(), information.as_mut_ptr())
    };
    if succeeded == 0 {
        return Err(VR_FOLDER_UNAVAILABLE);
    }
    // The Windows API initialized the complete structure after reporting success.
    let information = unsafe { information.assume_init() };
    let file_index =
        (u64::from(information.file_index_high) << 32) | u64::from(information.file_index_low);
    Ok(format!("{}:{file_index}", information.volume_serial_number))
}

#[cfg(not(any(unix, target_os = "windows")))]
fn file_fingerprint(path: &Path) -> Result<String, &'static str> {
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

fn finalize_monitored_transfer_with(
    context: &mut VrDownloadContext,
    transfer_id: &str,
    handle_generation: u64,
    completed: bool,
    persistence_path: &Path,
    mut persist: impl FnMut(&Path, &[StoredTransfer]) -> Result<(), &'static str>,
    mut persist_recovery: impl FnMut(&TransferRecord, u64) -> Result<(), &'static str>,
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

    let recovery_saved = find_valid_record_mut(&mut context.transfers, transfer_id)
        .is_some_and(|record| persist_recovery(record, handle_generation).is_ok());
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
    let primary_terminal_saved = persist(persistence_path, &context.transfers).is_ok();
    if primary_terminal_saved {
        let recovery_removed = find_valid_record_mut(&mut context.transfers, transfer_id)
            .is_some_and(|record| remove_terminal_recovery(record).is_ok());
        if recovery_removed || !completed {
            let record = find_valid_record_mut(&mut context.transfers, transfer_id)
                .expect("the validated transfer must remain present");
            record.handle = None;
            if recovery_removed {
                record.terminal_recovery_generation = None;
            }
            return true;
        }
    }

    {
        let record = find_valid_record_mut(&mut context.transfers, transfer_id)
            .expect("the validated transfer must remain present");
        record.state = TransferState::Failed;
    }
    if persist(persistence_path, &context.transfers).is_ok() {
        let recovery_removed = find_valid_record_mut(&mut context.transfers, transfer_id)
            .is_some_and(|record| remove_terminal_recovery(record).is_ok());
        if recovery_removed {
            find_valid_record_mut(&mut context.transfers, transfer_id)
                .expect("the validated transfer must remain present")
                .terminal_recovery_generation = None;
        }
    }
    find_valid_record_mut(&mut context.transfers, transfer_id)
        .expect("the validated transfer must remain present")
        .handle = None;
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
    let mut terminal_destinations = recovery_destinations
        .iter()
        .filter_map(|(category, destination)| {
            destination
                .as_ref()
                .map(|destination| (*category, destination.clone()))
        })
        .collect::<Vec<_>>();
    if let Ok(transfers) = &persisted_transfers {
        terminal_destinations.extend(transfers.iter().filter_map(|transfer| match transfer {
            StoredTransfer::Valid(record) => Some((record.category, record.destination.clone())),
            StoredTransfer::Corrupt(_) => None,
        }));
    }
    terminal_destinations.sort_by(|left, right| {
        left.0
            .as_str()
            .cmp(right.0.as_str())
            .then_with(|| left.1.cmp(&right.1))
    });
    terminal_destinations.dedup();
    let mut terminal_recovery_ids = BTreeSet::new();
    let terminal_recoveries = terminal_destinations
        .into_iter()
        .flat_map(|(category, destination)| {
            read_terminal_recoveries(&destination)
                .into_iter()
                .filter(move |record| {
                    record.category == category && record.destination == destination
                })
        })
        .filter(|record| terminal_recovery_ids.insert(record.transfer_id.clone()))
        .collect::<Vec<_>>();
    let has_durable_recovery = !recoveries.is_empty() || !terminal_recoveries.is_empty();
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
    let active_ids = {
        let mut context = state.0.lock().map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
        context.transfers_loading = false;
        context.transfers = transfers;
        context.transfers_loaded = true;
        for transfer in &mut context.transfers {
            if let StoredTransfer::Valid(record) = transfer {
                if validate_resume_context(record).is_err() {
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

fn validate_movie_organization_component_length(value: &str) -> Result<(), &'static str> {
    if value.len() > 255 || value.encode_utf16().count() > 255 {
        Err(VR_ORGANIZATION_INELIGIBLE)
    } else {
        Ok(())
    }
}

fn portable_movie_organization_directory(
    identity: &MovieDownloadIdentity,
) -> Result<String, &'static str> {
    let title = identity.tmdb_title.as_str();
    let reserved_base = title.split('.').next().unwrap_or(title);
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
    if title.is_empty()
        || matches!(title, "." | "..")
        || title.ends_with(' ')
        || title.ends_with('.')
        || is_reserved
        || title
            .chars()
            .any(|character| character.is_control() || r#"<>:"/\|?*"#.contains(character))
    {
        return Err(VR_ORGANIZATION_INELIGIBLE);
    }
    let year = identity
        .release_date
        .as_deref()
        .and_then(movie_release_year)
        .ok_or(VR_ORGANIZATION_INELIGIBLE)?;
    let directory = format!("{title} ({year})");
    validate_movie_organization_component_length(&directory)?;
    relative_file_path(&directory).map_err(|_| VR_ORGANIZATION_INELIGIBLE)?;
    Ok(directory)
}

fn organization_identity(record: &TransferRecord) -> Result<String, &'static str> {
    match record.category {
        TransferCategory::Adult | TransferCategory::Vr => Ok(record.code.clone()),
        TransferCategory::Movie => record
            .movie_identity
            .as_ref()
            .map(|identity| identity.imdb_id.clone())
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
    }
}

fn validate_organization_directory(
    destination: &Path,
    directory_name: &str,
) -> Result<Option<PathBuf>, &'static str> {
    for entry in fs::read_dir(destination).map_err(|_| VR_ORGANIZATION_INELIGIBLE)? {
        let entry = entry.map_err(|_| VR_ORGANIZATION_INELIGIBLE)?;
        let name = entry
            .file_name()
            .to_str()
            .ok_or(VR_ORGANIZATION_CONFLICT)?
            .to_owned();
        if name.to_lowercase() == directory_name.to_lowercase() && name != directory_name {
            return Err(VR_ORGANIZATION_CONFLICT);
        }
    }
    let directory = destination.join(directory_name);
    match fs::symlink_metadata(&directory) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(VR_ORGANIZATION_CONFLICT)
        }
        Ok(_) => {
            let canonical = fs::canonicalize(&directory).map_err(|_| VR_ORGANIZATION_CONFLICT)?;
            if canonical != directory || !canonical.starts_with(destination) {
                return Err(VR_ORGANIZATION_CONFLICT);
            }
            Ok(Some(directory))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(VR_ORGANIZATION_CONFLICT),
    }
}

fn destination_has_case_collision(directory: &Path, file_name: &str) -> Result<bool, &'static str> {
    let expected = file_name.to_lowercase();
    for entry in fs::read_dir(directory).map_err(|_| VR_ORGANIZATION_CONFLICT)? {
        let entry = entry.map_err(|_| VR_ORGANIZATION_CONFLICT)?;
        let existing = entry
            .file_name()
            .to_str()
            .ok_or(VR_ORGANIZATION_CONFLICT)?
            .to_lowercase();
        if existing == expected {
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
    if !is_supported_media(Path::new(original_relative)) {
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
            validate_movie_organization_component_length(&destination_name)?;
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
    };
    let destination_relative = format!("{directory_name}/{destination_name}");
    relative_file_path(&destination_relative).map_err(|_| VR_ORGANIZATION_CONFLICT)?;
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
    if record.category == TransferCategory::Movie {
        validate_movie_organization_identity(record)?;
    }

    let eligible_media = record
        .selected_files
        .iter()
        .filter(|file| is_supported_media(Path::new(&file.path)))
        .count();
    if eligible_media == 0 {
        return Err(VR_ORGANIZATION_INELIGIBLE);
    }
    let directory_name = organization_directory_name(record)?;
    let existing_directory = validate_organization_directory(&record.destination, &directory_name)?;
    let current_paths = record
        .current_paths
        .iter()
        .map(|path| path.to_lowercase())
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
        let destination_key = destination_relative.to_lowercase();
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
) -> String {
    let mut identity = generation.to_be_bytes().to_vec();
    identity_field(&mut identity, record.transfer_id.as_bytes());
    identity_field(&mut identity, record.category.as_str().as_bytes());
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
    format!("{generation}-{}", hex_sha1(&identity))
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
        plan_id: organization_plan_id(generation, record, &entries),
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
fn rename_without_overwrite(source: &Path, destination: &Path) -> io::Result<()> {
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
fn rename_without_overwrite(source: &Path, destination: &Path) -> io::Result<()> {
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
fn rename_without_overwrite(source: &Path, destination: &Path) -> io::Result<()> {
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
    if plan.category != record.category
        || plan.identity != organization_identity(record)?
        || plan.entries != entries
    {
        return Err(VR_ORGANIZATION_STALE);
    }
    let previous_state = record.organization_state;
    let original_paths = record.current_paths.clone();
    let destination_root = record.destination.clone();
    let directory_name = organization_directory_name(record)?;
    let organization_directory = destination_root.join(&directory_name);
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

    let mut created_directory = false;
    if !move_entries.is_empty()
        && validate_organization_directory(&destination_root, &directory_name)?.is_none()
    {
        match fs::create_dir(&organization_directory) {
            Ok(()) => created_directory = true,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                validate_organization_directory(&destination_root, &directory_name)?;
            }
            Err(_) => return Err(VR_ORGANIZATION_FAILED),
        }
    }

    let recovery_result = match &context.transfers[record_index] {
        StoredTransfer::Valid(record) => write_organization_recovery(record, &original_paths, None),
        StoredTransfer::Corrupt(_) => Err(VR_ORGANIZATION_STALE),
    };
    if let Err(error) = recovery_result {
        if created_directory {
            let _ = fs::remove_dir(&organization_directory);
        }
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
            if created_directory {
                let _ = fs::remove_dir(&organization_directory);
            }
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
        if created_directory {
            let _ = fs::remove_dir(&organization_directory);
        }
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
    for transfer in &mut context.transfers {
        match transfer {
            StoredTransfer::Valid(record) => {
                let current_folder = match record.category {
                    TransferCategory::Adult => &current_adult_folder,
                    TransferCategory::Movie => &current_movie_folder,
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
    let terminal_recoveries = recovery_destinations
        .into_iter()
        .flat_map(|destination| read_terminal_recoveries(&destination))
        .collect::<Vec<_>>();
    let has_durable_recovery = context.transfers.iter().any(|transfer| {
        matches!(transfer, StoredTransfer::Valid(record) if organization_recovery_transfer_ids.contains(&record.transfer_id)
            || terminal_recoveries.iter().any(|recovery| same_terminal_authority(record, recovery)))
    });
    match persist_transfers(persistence_path, &context.transfers) {
        Ok(()) => {
            for transfer in &mut context.transfers {
                if let StoredTransfer::Valid(record) = transfer {
                    clear_organization_recovery(record);
                    if remove_terminal_recovery(record).is_ok() {
                        record.terminal_recovery_generation = None;
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
                && source.movie_identity.as_ref().is_some_and(|identity| {
                    source.release_name == identity.tmdb_title
                        && source.infohash == identity.expected_infohash
                })
        }
        TransferCategory::Adult | TransferCategory::Vr => source.movie_identity.is_none(),
    };
    if !source_matches_category {
        return Err(VR_DOWNLOAD_CONTEXT_INVALID);
    }
    checked_selected_total(&source.selected_files)?;
    let destination = {
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
            TransferCategory::Vr => context.future_folder.as_deref(),
        };
        let destination = canonical_destination(future_folder.ok_or(VR_FOLDER_UNAVAILABLE)?)?;
        if has_active_duplicate(&context.transfers, &source.infohash, &destination) {
            return Err(VR_DOWNLOAD_DUPLICATE);
        }
        destination
    };
    validate_new_targets(&destination, &source.selected_files)?;
    let mut record = transfer_from_source(category, source, destination, TransferState::Queued);
    let transfer_id = record.transfer_id.clone();
    {
        let mut context = state.0.lock().map_err(|_| VR_DOWNLOAD_FAILED)?;
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
        if let Err(error) =
            remove_terminal_recovery(record).and_then(|()| remove_organization_recovery(record))
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
    use crate::vr_library::{
        scan_vr_library_with, trash_vr_file_with, VrLibraryState, VR_FILE_TRASH_OWNED,
        VR_FILE_TRASH_OWNERSHIP_UNAVAILABLE,
    };

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

    fn organization_state(record: TransferRecord) -> (VrDownloadState, String) {
        let transfer_id = record.transfer_id.clone();
        let destination = record.destination.clone();
        let category = record.category;
        let state = VrDownloadState::default();
        {
            let mut context = state.0.lock().expect("state must lock");
            match category {
                TransferCategory::Adult => context.adult_future_folder = Some(destination),
                TransferCategory::Movie => context.movie_future_folder = Some(destination),
                TransferCategory::Vr => context.future_folder = Some(destination),
            }
            context.transfers_loaded = true;
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
            TransferCategory::Vr => context.future_folder = Some(destination.to_owned()),
        }
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
    fn movie_adult_and_vr_completion_is_exposed_only_after_exact_terminal_authority_is_durable() {
        for (category, label) in [
            (TransferCategory::Movie, "Movies — terminal"),
            (TransferCategory::Adult, "Adult — terminal"),
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
                    assert!(terminal_recovery_path(match &transfers[0] {
                        StoredTransfer::Valid(record) => record,
                        StoredTransfer::Corrupt(_) => panic!("terminal authority must be valid"),
                    })
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
            assert!(!destination
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
        let recovery_path = terminal_recovery_path(match &context.transfers[0] {
            StoredTransfer::Valid(record) => record,
            StoredTransfer::Corrupt(_) => panic!("recovered terminal authority must be valid"),
        });
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
    fn exact_recovery_does_not_downgrade_an_already_durable_completion() {
        let fixture = FilesystemFixture::new();
        let mut record = terminal_record_for_category(
            &fixture,
            TransferCategory::Movie,
            "Movies — committed completion",
        );
        record.state = TransferState::Failed;
        record.downloaded_bytes = record.selected_total();
        let destination = record.destination.clone();
        let persistence_path = fixture.path.join("downloads");
        let recovery_path = terminal_recovery_path(&record);
        write_terminal_recovery(&record, 3).expect("pre-commit recovery must persist");
        record.state = TransferState::Completed;
        write_persisted_transfers(&persistence_path, &[StoredTransfer::Valid(record)])
            .expect("completed primary authority must persist");

        let restarted = VrDownloadState::default();
        configure_category_folder(&restarted, TransferCategory::Movie, &destination);
        let rows = tauri::async_runtime::block_on(load_downloads_with_persistence(
            &restarted,
            &persistence_path,
            &fixture.path.join("completed-primary-session"),
            &fixture.path.join("download-limit"),
            |_, _| Err(VR_DOWNLOAD_PERSISTENCE_FAILED),
        ))
        .expect("durable completion must remain visible during cleanup failure");
        assert_eq!(rows[1], "movie");
        assert_eq!(rows[8], "completed");
        assert_eq!(rows[13], "false");
        assert!(recovery_path.is_file());
        assert!(restarted
            .0
            .lock()
            .expect("state must lock")
            .session
            .is_none());
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
            |_, _| Err(VR_DOWNLOAD_PERSISTENCE_FAILED),
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
                    |_, transfers| {
                        persistence_attempts += 1;
                        assert!(matches!(
                            &transfers[0],
                            StoredTransfer::Valid(record)
                                if record.state == TransferState::Failed
                                    && record.downloaded_bytes == 7
                                    && record.handle.as_ref().is_some_and(|current| Arc::ptr_eq(current, &handle))
                        ));
                        Err(VR_DOWNLOAD_PERSISTENCE_FAILED)
                    },
                    |record, generation| {
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
        write_terminal_recovery(record, 7).expect("exact terminal recovery must persist");
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
        let malformed_path = terminal_recovery_path(&malformed_record);
        fs::write(
            &malformed_path,
            b"AUTO_VIDEO_TRANSFER_TERMINAL_V1\ninvalid\n",
        )
        .expect("malformed recovery fixture must write");
        assert!(parse_terminal_recovery(&malformed_path).is_none());
        let malformed_state = VrDownloadState::default();
        configure_category_folder(
            &malformed_state,
            TransferCategory::Movie,
            &malformed_destination,
        );
        let rows = tauri::async_runtime::block_on(load_downloads(
            &malformed_state,
            &malformed_fixture.path.join("downloads"),
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
        let exact_path = terminal_recovery_path(&record);
        let exact_bytes = encoded_terminal_recovery(&record, 11)
            .expect("exact terminal recovery fixture must encode");

        let stale_path = record.destination.join(format!(
            "{TERMINAL_RECOVERY_PREFIX}0000000000000000000000000000000000000000{TERMINAL_RECOVERY_SUFFIX}"
        ));
        fs::write(&stale_path, &exact_bytes).expect("stale recovery fixture must write");
        assert!(parse_terminal_recovery(&stale_path).is_none());

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
        assert!(parse_terminal_recovery(&wrong_destination_path).is_none());

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
                parse_terminal_recovery(&exact_path).is_none(),
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
        assert!(parse_terminal_recovery(&exact_path).is_none());

        fs::write(
            &exact_path,
            encoded_terminal_recovery(&record, 11)
                .expect("exact recovery fixture must encode again"),
        )
        .expect("exact recovery must replace corrupt fixture");
        record.terminal_recovery_generation = Some(12);
        assert_eq!(
            remove_terminal_recovery(&record),
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
        let recovery_path = terminal_recovery_path(&record);
        write_persisted_transfers(&persistence_path, &[StoredTransfer::Valid(record)])
            .expect("failed primary authority must persist");
        let persisted = read_persisted_transfers(&persistence_path)
            .expect("failed primary authority must reload");
        let StoredTransfer::Valid(recovery) = &persisted[0] else {
            panic!("failed primary authority must remain valid");
        };
        write_terminal_recovery(recovery, 9).expect("terminal recovery must persist");
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
            write_terminal_recovery(record, 9).expect("terminal recovery must persist again");
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
            |_, _| panic!("late monitor wrote recovery state"),
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
        let recovery_path = terminal_recovery_path(&record);
        write_terminal_recovery(&record, 4).expect("terminal recovery must persist");
        let recovery_bytes = fs::read(&recovery_path).expect("recovery must remain readable");
        let state = VrDownloadState::default();
        configure_category_folder(&state, TransferCategory::Vr, &destination);
        state.0.lock().expect("state must lock").transfers_loaded = true;
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
        let vr_recovery = terminal_recovery_path(&vr_record);
        let adult_recovery = terminal_recovery_path(&adult_record);
        write_terminal_recovery(&vr_record, 1).expect("VR recovery must persist");
        write_terminal_recovery(&adult_record, 2).expect("Adult recovery must persist");
        let adult_recovery_bytes =
            fs::read(&adult_recovery).expect("Adult recovery must remain readable");
        let persistence_path = fixture.path.join("downloads");
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
        {
            let mut context = state.0.lock().expect("state must lock");
            context.future_folder = Some(shared_destination.clone());
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
        let persistence_path = fixture
            .path
            .join(format!("{}-downloads", category.as_str()));
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
        {
            let mut context = recovery_state.0.lock().expect("recovery state must lock");
            context.future_folder = Some(recovery_destination.clone());
            match category {
                TransferCategory::Adult => {
                    context.adult_future_folder = Some(recovery_destination.clone())
                }
                TransferCategory::Movie => {
                    context.movie_future_folder = Some(recovery_destination.clone())
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
        let dismissal_persistence = fixture
            .path
            .join(format!("{}-recovery-downloads", category.as_str()));
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
        let vr_destination = fixture.path.join("VR");
        fs::create_dir(&adult_destination).expect("Adult destination must exist");
        fs::create_dir(&movie_destination).expect("Movies destination must exist");
        fs::create_dir(&vr_destination).expect("VR destination must exist");
        let adult_destination =
            fs::canonicalize(adult_destination).expect("Adult destination must canonicalize");
        let movie_destination =
            fs::canonicalize(movie_destination).expect("Movies destination must canonicalize");
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
        write_organization_recovery(&adult_record, &adult_record.current_paths, None)
            .expect("Adult recovery must persist");
        write_organization_recovery(&movie_record, &movie_record.current_paths, None)
            .expect("Movie recovery must persist");
        write_organization_recovery(&vr_record, &vr_record.current_paths, None)
            .expect("VR recovery must persist");
        let persistence_path = fixture.path.join("downloads");

        let swapped = VrDownloadState::default();
        {
            let mut context = swapped.0.lock().expect("state must lock");
            context.adult_future_folder = Some(vr_destination.clone());
            context.movie_future_folder = Some(adult_destination.clone());
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
            context.future_folder = Some(vr_destination);
        }
        let rows = tauri::async_runtime::block_on(load_downloads(
            &current,
            &persistence_path,
            &fixture.path.join("current-session"),
            &fixture.path.join("limit"),
        ))
        .expect("category-matched recoveries must load");
        assert_eq!(rows.len(), 42);
        assert_eq!(rows[1], "vr");
        assert_eq!(rows[15], "adult");
        assert_eq!(rows[29], "movie");
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

    fn completed_selected_boundary_record(
        fixture: &FilesystemFixture,
        boundary_bytes: Option<&[u8]>,
    ) -> TransferRecord {
        let destination = fixture.path.join("VR — retained boundary");
        fs::create_dir_all(destination.join("Folder")).expect("selected file parent must exist");
        let destination = fs::canonicalize(destination).expect("destination must canonicalize");
        let metainfo = selected_file_torrent();
        let infohash = hex_sha1(&metainfo[b"d4:info".len()..metainfo.len() - 1]);
        let source = revalidate_persisted_download_source(
            &metainfo,
            "MDVR-419",
            "【VR】 MDVR-419  Exact — 特別版",
            &infohash,
            &[1],
        )
        .expect("selected boundary fixture must revalidate");
        let mut record = transfer_from_source(
            TransferCategory::Vr,
            source,
            destination,
            TransferState::Downloading,
        );
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
    fn relaunch_preserves_a_completed_selected_boundary_piece_without_a_deselected_file() {
        const COMPLETION_ATTEMPTS: usize = 100;
        const COMPLETION_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(10);

        let fixture = FilesystemFixture::new();
        let record = completed_selected_boundary_record(&fixture, Some(b"abc"));
        let transfer_id = record.transfer_id.clone();
        let destination = record.destination.clone();
        let persistence_path = fixture.path.join("downloads");
        write_persisted_transfers(&persistence_path, &[StoredTransfer::Valid(record)])
            .expect("completed boundary fixture must persist");
        let state = VrDownloadState::default();

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
                panic!("retained boundary piece must complete locally: {last_rows:?}")
            });
            assert_eq!(rows[0], transfer_id);
            assert_eq!(rows[5], "7");
            assert_eq!(rows[6], "7");
            let context = state.0.lock().expect("state must lock");
            let StoredTransfer::Valid(record) = &context.transfers[0] else {
                panic!("completed boundary transfer must remain valid");
            };
            assert!(record.handle.is_none());
        });
        assert!(!destination.join("Folder/Part  1 — 映画.mkv").exists());
        assert_eq!(
            fs::read(destination.join("Folder/特別版  B.mp4")).expect("selected file must remain"),
            b"1234567"
        );
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
        {
            let mut context = state.0.lock().expect("state must lock");
            context.future_folder = Some(destination.clone());
            context.transfers_loaded = true;
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
        {
            let mut context = recovery_state.0.lock().expect("recovery state must lock");
            context.future_folder = Some(recovery_destination.clone());
            context.transfers_loaded = true;
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
        let recovery_persistence = fixture.path.join("recovery-downloads");
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

        state.0.lock().expect("state must lock").transfers_loaded = true;
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
