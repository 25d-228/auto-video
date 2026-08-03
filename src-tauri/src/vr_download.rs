use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
};

use anyhow::{anyhow, Context};
use librqbit::{
    storage::{BoxStorageFactory, StorageFactory, StorageFactoryExt, TorrentStorage},
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ManagedTorrent, Session, SessionOptions,
    TorrentStatsState,
};

use crate::vr_torrent::{
    hex_sha1, revalidate_persisted_download_source, VerifiedDownloadFile, VerifiedDownloadSource,
    VerifiedDownloadSourceError, VrTorrentState,
};

pub const VR_DOWNLOAD_ACTION_INVALID: &str = "vr_download_action_invalid";
pub const VR_DOWNLOAD_CONTEXT_INVALID: &str = "vr_download_context_invalid";
pub const VR_DOWNLOAD_DESTINATION_CONFLICT: &str = "vr_download_destination_conflict";
pub const VR_DOWNLOAD_DUPLICATE: &str = "vr_download_duplicate";
pub const VR_DOWNLOAD_FAILED: &str = "vr_download_failed";
pub const VR_DOWNLOAD_PERSISTENCE_FAILED: &str = "vr_download_persistence_failed";
pub const VR_DOWNLOAD_STALE: &str = "vr_download_stale";
pub const VR_FOLDER_STORAGE_FAILED: &str = "vr_folder_storage_failed";
pub const VR_FOLDER_UNAVAILABLE: &str = "vr_folder_unavailable";

const PERSISTENCE_HEADER: &[u8] = b"AUTO_VIDEO_VR_DOWNLOADS_V1\n";
const MAX_PERSISTENCE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PERSISTED_TRANSFERS: usize = 100;
const MAX_SELECTED_FILES: usize = 100_000;

type ManagedTorrentHandle = Arc<ManagedTorrent>;

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

struct TransferRecord {
    transfer_id: String,
    code: String,
    release_name: String,
    infohash: String,
    metainfo: Vec<u8>,
    selected_files: Vec<VerifiedDownloadFile>,
    destination: PathBuf,
    fingerprints: Vec<String>,
    state: TransferState,
    downloaded_bytes: u64,
    boundary_segments: Arc<Mutex<BTreeMap<usize, Vec<SparseSegment>>>>,
    handle: Option<ManagedTorrentHandle>,
    handle_generation: u64,
    pending_action: Option<TransferAction>,
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
    code: String,
    release_name: String,
    raw_line: Vec<u8>,
}

enum StoredTransfer {
    Valid(TransferRecord),
    Corrupt(CorruptTransferRecord),
}

#[derive(Default)]
struct VrDownloadContext {
    future_folder: Option<PathBuf>,
    session: Option<Arc<Session>>,
    session_starting: bool,
    transfers_loaded: bool,
    transfers_loading: bool,
    transfers: Vec<StoredTransfer>,
}

#[derive(Clone, Default)]
pub struct VrDownloadState(Arc<Mutex<VrDownloadContext>>);

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
    state
        .0
        .lock()
        .map_err(|_| VR_FOLDER_STORAGE_FAILED)?
        .future_folder = Some(canonical);
    Ok(response)
}

pub fn clear_vr_folder(state: &VrDownloadState, path: &Path) -> Result<(), &'static str> {
    clear_vr_folder_file(path)?;
    state
        .0
        .lock()
        .map_err(|_| VR_FOLDER_STORAGE_FAILED)?
        .future_folder = None;
    Ok(())
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

fn transfer_identity(source: &VerifiedDownloadSource, destination: &Path) -> String {
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
    source: VerifiedDownloadSource,
    destination: PathBuf,
    state: TransferState,
) -> TransferRecord {
    TransferRecord {
        transfer_id: transfer_identity(&source, &destination),
        code: source.code,
        release_name: source.release_name,
        infohash: source.infohash,
        metainfo: source.bytes,
        selected_files: source.selected_files,
        destination,
        fingerprints: Vec::new(),
        state,
        downloaded_bytes: 0,
        boundary_segments: Arc::new(Mutex::new(BTreeMap::new())),
        handle: None,
        handle_generation: 0,
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

fn encode_transfer(record: &TransferRecord) -> Result<Vec<u8>, &'static str> {
    Ok([
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
    ]
    .join("\t")
    .into_bytes())
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

fn parse_transfer_line(line: &[u8]) -> Option<TransferRecord> {
    let fields = line.split(|byte| *byte == b'\t').collect::<Vec<_>>();
    let (fields, boundary_segments) = match fields.as_slice() {
        [transfer_id, code, release_name, infohash, destination, state, metainfo, selected_ids, fingerprints, downloaded_bytes] => {
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
            )
        }
        [transfer_id, code, release_name, infohash, destination, state, metainfo, selected_ids, fingerprints, downloaded_bytes, boundary_segments] => {
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

    let source = revalidate_persisted_download_source(
        &metainfo,
        &code,
        &release_name,
        &infohash,
        &selected_ids,
    )
    .ok()?;
    let selected_total = checked_selected_total(&source.selected_files).ok()?;
    if transfer_identity(&source, &destination) != transfer_id || downloaded_bytes > selected_total
    {
        return None;
    }

    Some(TransferRecord {
        transfer_id,
        code,
        release_name,
        infohash,
        metainfo,
        selected_files: source.selected_files,
        destination,
        fingerprints,
        state,
        downloaded_bytes,
        boundary_segments: Arc::new(Mutex::new(boundary_segments)),
        handle: None,
        handle_generation: 0,
        pending_action: None,
    })
}

fn corrupt_transfer(line: &[u8], line_number: usize) -> CorruptTransferRecord {
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
        ))]);
    }
    let bytes = fs::read(path).map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
    let Some(body) = bytes.strip_prefix(PERSISTENCE_HEADER) else {
        return Ok(vec![StoredTransfer::Corrupt(corrupt_transfer(&bytes, 0))]);
    };
    let mut transfers = Vec::new();
    let mut transfer_ids = BTreeSet::new();
    for (line_number, line) in body
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .take(MAX_PERSISTED_TRANSFERS)
        .enumerate()
    {
        transfers.push(match parse_transfer_line(line) {
            Some(record) if transfer_ids.insert(record.transfer_id.clone()) => {
                StoredTransfer::Valid(record)
            }
            None => StoredTransfer::Corrupt(corrupt_transfer(line, line_number)),
            Some(_) => StoredTransfer::Corrupt(corrupt_transfer(line, line_number)),
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
    fs::write(path, bytes).map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)
}

async fn session_for(
    state: &VrDownloadState,
    session_folder: &Path,
) -> Result<Arc<Session>, &'static str> {
    {
        let mut context = state.0.lock().map_err(|_| VR_DOWNLOAD_FAILED)?;
        if let Some(session) = &context.session {
            return Ok(session.clone());
        }
        if context.session_starting {
            return Err(VR_DOWNLOAD_ACTION_INVALID);
        }
        context.session_starting = true;
    }

    if fs::create_dir_all(session_folder).is_err() {
        if let Ok(mut context) = state.0.lock() {
            context.session_starting = false;
        }
        return Err(VR_DOWNLOAD_FAILED);
    }

    let result = Session::new_with_opts(
        session_folder.to_owned(),
        SessionOptions {
            disable_upload: true,
            disable_dht: true,
            disable_dht_persistence: true,
            fastresume: false,
            enable_upnp_port_forwarding: false,
            ..Default::default()
        },
    )
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
        .map(|file| file_fingerprint(&selected_target(&record.destination, file)?))
        .collect()
}

fn validate_resume_context(record: &TransferRecord) -> Result<(), &'static str> {
    if canonical_destination(&record.destination)? != record.destination
        || record.fingerprints.len() != record.selected_files.len()
    {
        return Err(VR_DOWNLOAD_STALE);
    }
    for (file, expected_fingerprint) in record.selected_files.iter().zip(record.fingerprints.iter())
    {
        if file_fingerprint(&selected_target(&record.destination, file)?)? != *expected_fingerprint
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
        {
            let mut context = match state.0.lock() {
                Ok(context) => context,
                Err(_) => return,
            };
            let Some(record) = find_valid_record_mut(&mut context.transfers, &transfer_id) else {
                return;
            };
            if record.handle_generation != handle_generation || !record.state.is_active() {
                return;
            }
            record.handle = None;
            record.pending_action = None;
            if result.is_ok() {
                record.state = TransferState::Completed;
                record.downloaded_bytes = record.selected_total();
            } else {
                record.state = TransferState::Failed;
            }
            let _ = write_persisted_transfers(&persistence_path, &context.transfers);
        }
        let _ = session.delete(handle.id().into(), false).await;
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
            code: record.code.clone(),
            release_name: record.release_name.clone(),
            infohash: record.infohash.clone(),
            metainfo: record.metainfo.clone(),
            selected_files: record.selected_files.clone(),
            destination: record.destination.clone(),
            fingerprints: record.fingerprints.clone(),
            state: record.state,
            downloaded_bytes: record.downloaded_bytes,
            boundary_segments: record.boundary_segments.clone(),
            handle: None,
            handle_generation: record.handle_generation,
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
        let _ = session.delete(handle.id().into(), false).await;
        if let Ok(mut context) = state.0.lock() {
            if let Some(record) = find_valid_record_mut(&mut context.transfers, transfer_id) {
                if record.handle_generation == handle_generation {
                    record.state = TransferState::Failed;
                    record.handle = None;
                }
            }
            let _ = write_persisted_transfers(persistence_path, &context.transfers);
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

pub async fn load_downloads(
    state: &VrDownloadState,
    persistence_path: &Path,
    session_folder: &Path,
) -> Result<Vec<String>, &'static str> {
    {
        let mut context = state.0.lock().map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
        if context.transfers_loaded {
            return Ok(download_rows(&mut context));
        }
        if context.transfers_loading {
            return Err(VR_DOWNLOAD_ACTION_INVALID);
        }
        context.transfers_loading = true;
    }
    let transfers = read_persisted_transfers(persistence_path);
    let active_ids = {
        let mut context = state.0.lock().map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
        context.transfers_loading = false;
        context.transfers = transfers?;
        context.transfers_loaded = true;
        for transfer in &mut context.transfers {
            if let StoredTransfer::Valid(record) = transfer {
                if validate_resume_context(record).is_err() {
                    record.state = TransferState::Offline;
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
    list_downloads(state, persistence_path)
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

fn download_rows(context: &mut VrDownloadContext) -> Vec<String> {
    let mut rows = Vec::new();
    for transfer in &mut context.transfers {
        match transfer {
            StoredTransfer::Valid(record) => {
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
                        TorrentStatsState::Error => TransferState::Failed,
                    };
                }
                if record.state == TransferState::Completed {
                    record.downloaded_bytes = record.selected_total();
                }
                rows.extend([
                    record.transfer_id.clone(),
                    record.code.clone(),
                    record.release_name.clone(),
                    record.selected_files.len().to_string(),
                    record.selected_total().to_string(),
                    record.downloaded_bytes.to_string(),
                    speed.to_string(),
                    record.state.as_str().to_owned(),
                ]);
            }
            StoredTransfer::Corrupt(record) => rows.extend([
                record.transfer_id.clone(),
                record.code.clone(),
                record.release_name.clone(),
                "0".to_owned(),
                "0".to_owned(),
                "0".to_owned(),
                "0".to_owned(),
                TransferState::Offline.as_str().to_owned(),
            ]),
        }
    }
    rows
}

pub fn list_downloads(
    state: &VrDownloadState,
    persistence_path: &Path,
) -> Result<Vec<String>, &'static str> {
    let mut context = state.0.lock().map_err(|_| VR_DOWNLOAD_PERSISTENCE_FAILED)?;
    if !context.transfers_loaded {
        return Err(VR_DOWNLOAD_ACTION_INVALID);
    }
    let rows = download_rows(&mut context);
    write_persisted_transfers(persistence_path, &context.transfers)?;
    Ok(rows)
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
    checked_selected_total(&source.selected_files)?;
    let destination = {
        let context = state.0.lock().map_err(|_| VR_DOWNLOAD_FAILED)?;
        if !context.transfers_loaded {
            return Err(VR_DOWNLOAD_ACTION_INVALID);
        }
        let destination = canonical_destination(
            context
                .future_folder
                .as_deref()
                .ok_or(VR_FOLDER_UNAVAILABLE)?,
        )?;
        if has_active_duplicate(&context.transfers, &source.infohash, &destination) {
            return Err(VR_DOWNLOAD_DUPLICATE);
        }
        destination
    };
    validate_new_targets(&destination, &source.selected_files)?;
    let mut record = transfer_from_source(source, destination, TransferState::Queued);
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
                        code: record.code.clone(),
                        release_name: record.release_name.clone(),
                        infohash: record.infohash.clone(),
                        metainfo: record.metainfo.clone(),
                        selected_files: record.selected_files.clone(),
                        destination: record.destination.clone(),
                        fingerprints: Vec::new(),
                        state: record.state,
                        downloaded_bytes: 0,
                        boundary_segments: record.boundary_segments.clone(),
                        handle: None,
                        handle_generation: 0,
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
            let _ = session.delete(handle.id().into(), false).await;
            mark_transfer_failed(state, persistence_path, &transfer_id);
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
            let _ = session.delete(handle.id().into(), false).await;
            mark_transfer_failed(state, persistence_path, &transfer_id);
            return Err(error);
        }
    };
    if session.unpause(&handle).await.is_err() {
        let _ = session.delete(handle.id().into(), false).await;
        mark_transfer_failed(state, persistence_path, &transfer_id);
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

fn mark_transfer_failed(state: &VrDownloadState, persistence_path: &Path, transfer_id: &str) {
    if let Ok(mut context) = state.0.lock() {
        if let Some(record) = find_valid_record_mut(&mut context.transfers, transfer_id) {
            record.state = TransferState::Failed;
            record.handle = None;
            record.pending_action = None;
        }
        let _ = write_persisted_transfers(persistence_path, &context.transfers);
    }
}

async fn controlled_transfer(
    state: &VrDownloadState,
    persistence_path: &Path,
    transfer_id: &str,
    action: TransferAction,
) -> Result<(), &'static str> {
    let (session, handle, handle_generation) = {
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
        if action == TransferAction::Cancel {
            record.handle_generation = record.handle_generation.wrapping_add(1);
        }
        let handle_generation = record.handle_generation;
        let handle = record.handle.clone();
        match action {
            TransferAction::Cancel if handle.is_none() => (session, None, handle_generation),
            _ => (
                Some(session.ok_or(VR_DOWNLOAD_STALE)?),
                Some(handle.ok_or(VR_DOWNLOAD_STALE)?),
                handle_generation,
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
        if let (Some(session), Some(handle)) = (session.as_ref(), handle.as_ref()) {
            let _ = session.delete(handle.id().into(), false).await;
        }
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
    if result.is_err() {
        record.state = TransferState::Failed;
        record.handle = None;
        write_persisted_transfers(persistence_path, &context.transfers)?;
        return Err(VR_DOWNLOAD_FAILED);
    }
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
    context.transfers.remove(position);
    write_persisted_transfers(persistence_path, &context.transfers)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;

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
            selected_files: vec![VerifiedDownloadFile {
                file_id: 0,
                path: "Folder/Part  1 — 映画.mkv".to_owned(),
                size: 5,
            }],
        }
    }

    fn persistable_fixture_source() -> VerifiedDownloadSource {
        VerifiedDownloadSource {
            bytes: b"d4:infod6:lengthi5e4:name12:Movie  A.mp412:piece lengthi16384e6:pieces20:aaaaaaaaaaaaaaaaaaaaee".to_vec(),
            code: "MDVR-419".to_owned(),
            infohash: "8b16011989123e1d68a8aaf18f5a599e6a4a0bc7".to_owned(),
            release_name: "【VR】 MDVR-419  Exact — 特別版".to_owned(),
            selected_files: vec![VerifiedDownloadFile {
                file_id: 0,
                path: "Movie  A.mp4".to_owned(),
                size: 5,
            }],
        }
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
        let mut record = transfer_from_source(source, destination, TransferState::Downloading);
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
    fn reports_a_persisted_future_folder_as_unavailable_without_replacing_it() {
        let fixture = FilesystemFixture::new();
        let config_path = fixture.path.join("vr-folder");
        let missing = fixture.path.join("missing");
        save_vr_folder_file(&config_path, &missing).expect("folder must persist");
        let state = VrDownloadState::default();
        assert_eq!(
            load_vr_folder_with(&state, &config_path),
            Ok(vec![
                "unavailable".to_owned(),
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
        let identity = transfer_identity(&source, &destination);
        assert_eq!(identity.len(), 40);

        let mut changed_release = source.clone();
        changed_release.release_name.push(' ');
        assert_ne!(identity, transfer_identity(&changed_release, &destination));
        let mut changed_selection = source.clone();
        changed_selection.selected_files[0].file_id = 1;
        assert_ne!(
            identity,
            transfer_identity(&changed_selection, &destination)
        );
        let mut changed_metainfo = source.clone();
        changed_metainfo.bytes.push(b'!');
        assert_ne!(identity, transfer_identity(&changed_metainfo, &destination));
        let other_destination = destination.join("other");
        fs::create_dir(&other_destination).expect("other destination must exist");
        assert_ne!(identity, transfer_identity(&source, &other_destination));
    }

    #[test]
    fn corrupt_persistence_isolated_from_a_valid_record() {
        let fixture = FilesystemFixture::new();
        let path = fixture.path.join("downloads");
        let source = persistable_fixture_source();
        let destination = fs::canonicalize(&fixture.path).expect("destination must canonicalize");
        let record = transfer_from_source(source, destination, TransferState::Cancelled);
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
        ))
        .expect("valid terminal row must load");
        assert_eq!(available_rows[7], "cancelled");
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
        ))
        .expect("missing terminal row must remain visible");
        assert_eq!(missing_rows[7], "offline");
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
        let mut record =
            transfer_from_source(source, destination.clone(), TransferState::Downloading);
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
            )
            .await
            .expect("verified complete content must restore");
            let mut completed_rows = None;
            let mut last_rows = Vec::new();
            // One second bounds deterministic local completion without relying on a network peer.
            for _ in 0..COMPLETION_ATTEMPTS {
                let rows = list_downloads(&state, &persistence_path)
                    .expect("completion progress must remain readable");
                if rows[7] == "completed" {
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
            assert_eq!(rows[4], contents.len().to_string());
            assert_eq!(rows[5], contents.len().to_string());
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
            )
            .await
            .expect("retained boundary state must restore without a peer");
            let mut completed_rows = None;
            let mut last_rows = Vec::new();
            // One second bounds deterministic local completion without relying on a network peer.
            for _ in 0..COMPLETION_ATTEMPTS {
                let rows = list_downloads(&state, &persistence_path)
                    .expect("restored boundary progress must remain readable");
                if rows[7] == "completed" {
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
            assert_eq!(rows[4], "7");
            assert_eq!(rows[5], "7");
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
            ))
            .expect("invalid retained state must remain dismissable");
            assert_eq!(rows[5], "0", "{case} boundary state reported progress");
            assert_eq!(rows[7], "offline", "{case} boundary state resumed");
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
    fn native_engine_resumes_only_selected_files_and_cancel_keeps_partial_data() {
        let fixture = FilesystemFixture::new();
        let destination = fixture.path.join("VR — 作品");
        fs::create_dir_all(&destination).expect("destination must exist");
        let destination = fs::canonicalize(destination).expect("destination must canonicalize");
        let persistence_path = fixture.path.join("downloads");
        let session_folder = fixture.path.join("session");
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
            load_downloads(&state, &persistence_path, &session_folder)
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
            )
            .await
            .expect("valid paused transfer must restore");
            assert_eq!(resumed_rows[7], "paused");
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
