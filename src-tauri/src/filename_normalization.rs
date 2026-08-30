use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs::{self, OpenOptions},
    io::Write,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use unicode_normalization::UnicodeNormalization;

use crate::{
    fanza_catalog, javdb_catalog,
    library_scan::is_supported_library_media,
    vr_download::{
        file_fingerprint, rename_without_overwrite, validate_portable_organization_component,
        with_unowned_adult_library_path, with_unowned_adult_library_paths,
        with_unowned_vr_library_path, with_unowned_vr_library_paths, VrDownloadState,
        VrLibraryTrashOwnershipError,
    },
    vr_torrent::{hex_sha1, product_code_forms, JsonParser, JsonValue},
    ProviderRequestError,
};

pub(crate) const NORMALIZATION_FAILED: &str = "filename_normalization_failed";
pub(crate) const NORMALIZATION_STALE: &str = "filename_normalization_stale";
pub(crate) const NORMALIZATION_RECOVERY: &str = "filename_normalization_recovery";
pub(crate) const NORMALIZATION_COMMITTED: &str = "filename_normalization_committed";

const RESPONSE_VERSION: &str = "filename-normalization-v1";
const RECOVERY_VERSION: &str = "AUTO_VIDEO_FILENAME_NORMALIZATION_V2";
const RECOVERY_MAX_BYTES: u64 = 8 * 1024 * 1024;
const MAX_AUDIT_ITEMS: usize = 100_000;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(crate) enum NormalizationCategory {
    Adult,
    Vr,
}

impl NormalizationCategory {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "adult" => Some(Self::Adult),
            "vr" => Some(Self::Vr),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Adult => "adult",
            Self::Vr => "vr",
        }
    }

    fn fanza_content_type(self) -> &'static str {
        match self {
            Self::Adult => "TWO_DIMENSION",
            Self::Vr => "VR",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct NormalizationFile {
    pub path: PathBuf,
    pub relative_path: String,
    pub size: u64,
    pub modified: SystemTime,
    pub local_identity: Option<String>,
    pub local_display: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct NormalizationSnapshot {
    pub category: NormalizationCategory,
    pub folder: PathBuf,
    pub generation: u64,
    pub files: Vec<NormalizationFile>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ProviderProof {
    provider: &'static str,
    provider_id: String,
    display_code: String,
    reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RenameMember {
    source: PathBuf,
    destination: PathBuf,
    source_relative: String,
    destination_relative: String,
    size: u64,
    modified: SystemTime,
    fingerprint: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AuditEntry {
    id: String,
    status: &'static str,
    local_code: Option<String>,
    proof: Option<ProviderProof>,
    reason: String,
    members: Vec<RenameMember>,
}

#[derive(Clone, Debug)]
struct NormalizationPlan {
    id: String,
    category: NormalizationCategory,
    folder: PathBuf,
    scan_generation: u64,
    entries: Vec<AuditEntry>,
}

#[derive(Default)]
struct NormalizationContext {
    generation: u64,
    plan: Option<NormalizationPlan>,
    active_operation: Option<u64>,
}

#[derive(Clone, Default)]
pub(crate) struct FilenameNormalizationState(Arc<Mutex<NormalizationContext>>);

struct OperationGuard {
    state: FilenameNormalizationState,
    generation: u64,
}

impl Drop for OperationGuard {
    fn drop(&mut self) {
        if let Ok(mut context) = self.state.0.lock() {
            if context.active_operation == Some(self.generation) {
                context.active_operation = None;
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FanzaExactResult {
    NoMatch,
    MissingMaker,
}

fn json_string(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len() + 2);
    encoded.push('"');
    for character in value.chars() {
        match character {
            '"' => encoded.push_str("\\\""),
            '\\' => encoded.push_str("\\\\"),
            '\n' => encoded.push_str("\\n"),
            '\r' => encoded.push_str("\\r"),
            '\t' => encoded.push_str("\\t"),
            value if value.is_control() => {
                encoded.push_str(&format!("\\u{:04x}", u32::from(value)));
            }
            value => encoded.push(value),
        }
    }
    encoded.push('"');
    encoded
}

fn fanza_transport_id(category: NormalizationCategory, code: &str) -> Option<String> {
    let forms = product_code_forms(code)?;
    if forms.display != code {
        return None;
    }
    let (_, number) = forms.identity.split_once('-')?;
    let prefix = match (category, forms.prefix.as_str()) {
        (NormalizationCategory::Adult, "CAWB") | (NormalizationCategory::Vr, "DSVR") => {
            forms.prefix.to_ascii_lowercase()
        }
        (NormalizationCategory::Vr, "3DSVR") => {
            format!("1{}", forms.prefix.to_ascii_lowercase())
        }
        _ => return None,
    };
    Some(format!("{prefix}{number:0>5}"))
}

fn fanza_exact_body(content_id: &str) -> String {
    format!(
        "{{\"query\":\"query{{ppvContent(id:{}){{id contentType makerContentId}}}}\",\"variables\":{{}}}}",
        json_string(content_id)
    )
}

fn exact_string<'a>(
    object: &'a BTreeMap<String, JsonValue>,
    key: &str,
) -> Result<&'a str, ProviderRequestError> {
    match object.get(key) {
        Some(JsonValue::String(value))
            if !value.is_empty()
                && value.len() <= 128
                && value.trim() == value
                && !value.bytes().any(|byte| byte.is_ascii_control()) =>
        {
            Ok(value)
        }
        _ => Err(ProviderRequestError::Provider),
    }
}

fn parse_fanza_exact(
    document: &str,
    category: NormalizationCategory,
    local_code: &str,
    requested_content_id: &str,
) -> Result<Result<ProviderProof, FanzaExactResult>, ProviderRequestError> {
    let JsonValue::Object(root) = JsonParser::new(document)
        .parse()
        .ok_or(ProviderRequestError::Provider)?
    else {
        return Err(ProviderRequestError::Provider);
    };
    match root.get("errors") {
        None | Some(JsonValue::Null) => {}
        Some(JsonValue::Array(errors)) if errors.is_empty() => {}
        Some(JsonValue::Array(_)) => return Err(ProviderRequestError::Provider),
        Some(_) => return Err(ProviderRequestError::Provider),
    }
    let Some(JsonValue::Object(data)) = root.get("data") else {
        return Err(ProviderRequestError::Provider);
    };
    let Some(value) = data.get("ppvContent") else {
        return Err(ProviderRequestError::Provider);
    };
    let JsonValue::Object(content) = value else {
        return if matches!(value, JsonValue::Null) {
            Ok(Err(FanzaExactResult::NoMatch))
        } else {
            Err(ProviderRequestError::Provider)
        };
    };
    let returned_content_id = exact_string(content, "id")?;
    let content_type = exact_string(content, "contentType")?;
    if returned_content_id != requested_content_id || content_type != category.fanza_content_type()
    {
        return Err(ProviderRequestError::Provider);
    }
    let maker_code = match content.get("makerContentId") {
        None | Some(JsonValue::Null) => return Ok(Err(FanzaExactResult::MissingMaker)),
        Some(JsonValue::String(value)) if value.is_empty() => {
            return Ok(Err(FanzaExactResult::MissingMaker));
        }
        Some(JsonValue::String(value))
            if value.trim() == value
                && !value.bytes().any(|byte| byte.is_ascii_control())
                && product_code_forms(value).is_some_and(|forms| forms.display == *value) =>
        {
            value
        }
        _ => return Err(ProviderRequestError::Provider),
    };
    let local = product_code_forms(local_code).ok_or(ProviderRequestError::Provider)?;
    let maker = product_code_forms(maker_code).ok_or(ProviderRequestError::Provider)?;
    if local.identity != maker.identity {
        return Err(ProviderRequestError::Provider);
    }
    Ok(Ok(ProviderProof {
        provider: "FANZA",
        provider_id: returned_content_id.to_owned(),
        display_code: maker_code.to_owned(),
        reason: "Exact FANZA item, category, transport identity, and maker item number agree."
            .to_owned(),
    }))
}

fn resolve_provider_proof(
    category: NormalizationCategory,
    local_code: &str,
    fanza_fetch: &mut impl FnMut(&str) -> Result<String, ProviderRequestError>,
    javdb_fetch: &mut impl FnMut(&str) -> Result<String, ProviderRequestError>,
) -> Result<Option<ProviderProof>, (&'static str, String)> {
    let Some(content_id) = fanza_transport_id(category, local_code) else {
        return Ok(None);
    };
    let fanza_document = fanza_fetch(&fanza_exact_body(&content_id)).map_err(|error| {
        (
            "unresolved",
            match error {
                ProviderRequestError::Network => {
                    "FANZA could not be reached; provider precedence remains unresolved."
                }
                ProviderRequestError::SourceUnavailable => {
                    "FANZA is unavailable; provider precedence remains unresolved."
                }
                ProviderRequestError::Provider => "FANZA returned an unusable provider response.",
            }
            .to_owned(),
        )
    })?;
    match parse_fanza_exact(&fanza_document, category, local_code, &content_id) {
        Ok(Ok(proof)) => return Ok(Some(proof)),
        Ok(Err(FanzaExactResult::NoMatch | FanzaExactResult::MissingMaker)) => {}
        Err(_) => {
            return Err((
                "conflicting",
                "FANZA did not prove one exact current item and maker code.".to_owned(),
            ));
        }
    }

    let mut fetch = |url: &str| javdb_fetch(url);
    match javdb_catalog::fetch_exact_library_item_with(category.as_str(), local_code, &mut fetch) {
        Ok(Some(item)) => {
            let local = product_code_forms(local_code).ok_or((
                "unresolved",
                "The local product code is not canonical.".to_owned(),
            ))?;
            let verified = product_code_forms(&item.display_code).ok_or((
                "conflicting",
                "JavDB returned an invalid display code.".to_owned(),
            ))?;
            if local.identity != verified.identity {
                return Err((
                    "conflicting",
                    "JavDB returned a different product identity.".to_owned(),
                ));
            }
            Ok(Some(ProviderProof {
                provider: "JavDB",
                provider_id: item.provider_item_id,
                display_code: item.display_code,
                reason: "FANZA proved no exact maker code; JavDB search and detail agree."
                    .to_owned(),
            }))
        }
        Ok(None) => Ok(None),
        Err(_) => Err((
            "unresolved",
            "JavDB fallback did not establish one exact current identity.".to_owned(),
        )),
    }
}

fn folded_path(value: &str) -> String {
    value.nfc().flat_map(char::to_lowercase).collect()
}

fn multipart_label(title: &str, allow_pt: bool) -> Option<String> {
    let bytes = title.as_bytes();
    let prefixes = if allow_pt {
        &["PART", "PT", "CD", "DISC", "DISK"][..]
    } else {
        &["PART", "CD", "DISC", "DISK"][..]
    };
    let mut matches = Vec::<(String, u64)>::new();
    for index in 0..bytes.len() {
        if index > 0 && bytes[index - 1].is_ascii_alphanumeric() {
            continue;
        }
        for prefix in prefixes {
            let end = index + prefix.len();
            if end > bytes.len() || !bytes[index..end].eq_ignore_ascii_case(prefix.as_bytes()) {
                continue;
            }
            let mut cursor = end;
            while cursor < bytes.len() && matches!(bytes[cursor], b' ' | b'_' | b'-') {
                cursor += 1;
            }
            let number_start = cursor;
            while cursor < bytes.len()
                && bytes[cursor].is_ascii_digit()
                && cursor - number_start < 4
            {
                cursor += 1;
            }
            if cursor == number_start
                || bytes.get(cursor).is_some_and(u8::is_ascii_alphanumeric)
                || bytes[number_start..cursor].iter().all(|byte| *byte == b'0')
            {
                continue;
            }
            let continuation = title[cursor..]
                .chars()
                .find(|character| character.is_alphanumeric());
            if continuation.is_some_and(|character| character.is_ascii_digit()) {
                return None;
            }
            let number = title[number_start..cursor].parse::<u64>().ok()?;
            matches.push((title[index..cursor].to_owned(), number));
        }
    }
    (matches.len() == 1).then(|| matches[0].0.clone())
}

fn proposed_members(
    snapshot: &NormalizationSnapshot,
    files: &[NormalizationFile],
    verified_code: &str,
) -> Result<Vec<RenameMember>, String> {
    let multiple = files.len() > 1;
    let mut destinations = HashSet::new();
    let mut part_identities = HashSet::new();
    let mut members = Vec::with_capacity(files.len());
    for file in files {
        let source = &file.path;
        let filename = source
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| "The current filename is not valid Unicode.".to_owned())?;
        let extension = source
            .extension()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "The current filename has no supported extension.".to_owned())?;
        let stem = source
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| "The current filename is not valid Unicode.".to_owned())?;
        let label = if multiple {
            let label = multipart_label(stem, snapshot.category == NormalizationCategory::Vr)
                .ok_or_else(|| {
                    "Multipart members do not retain one exact part identity.".to_owned()
                })?;
            let part_identity = label.bytes().filter(u8::is_ascii_digit).collect::<Vec<_>>();
            let part_identity = std::str::from_utf8(&part_identity)
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .filter(|value| *value > 0)
                .ok_or_else(|| {
                    "Multipart members do not retain one exact part identity.".to_owned()
                })?;
            if !part_identities.insert(part_identity) {
                return Err("Multipart members repeat one part identity.".to_owned());
            }
            Some(label)
        } else {
            None
        };
        let destination_name = match label {
            Some(label) => format!("{verified_code} - {label}.{extension}"),
            None => format!("{verified_code}.{extension}"),
        };
        validate_portable_organization_component(&destination_name)
            .map_err(|_| "The proposed filename is not one portable path component.".to_owned())?;
        let parent = source
            .parent()
            .ok_or_else(|| "The current file has no safe parent directory.".to_owned())?;
        let destination = parent.join(destination_name);
        let destination_relative = destination
            .strip_prefix(&snapshot.folder)
            .ok()
            .and_then(Path::to_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "The proposed filename leaves the configured folder.".to_owned())?
            .to_owned();
        if !destinations.insert(folded_path(&destination_relative)) {
            return Err(
                "The proposed filenames collide after Unicode and case folding.".to_owned(),
            );
        }
        let selected_sources = files
            .iter()
            .map(|selected| selected.path.clone())
            .collect::<HashSet<_>>();
        if unrelated_sibling_collision(&destination, &selected_sources) {
            return Err(
                "The proposed filename collides with an unrelated existing sibling.".to_owned(),
            );
        }
        let source_relative = file.relative_path.clone();
        if filename.is_empty() || source_relative.is_empty() {
            return Err("The current relative filename is unavailable.".to_owned());
        }
        members.push(RenameMember {
            source: source.clone(),
            destination,
            source_relative,
            destination_relative,
            size: file.size,
            modified: file.modified,
            fingerprint: file_fingerprint(&file.path)
                .map_err(|_| "The current file identity cannot be retained safely.".to_owned())?,
        });
    }
    Ok(members)
}

fn unrelated_sibling_collision(destination: &Path, allowed: &HashSet<PathBuf>) -> bool {
    let Some(parent) = destination.parent() else {
        return true;
    };
    let Some(destination_name) = destination.file_name().and_then(|value| value.to_str()) else {
        return true;
    };
    let folded_destination = folded_path(destination_name);
    let Ok(siblings) = fs::read_dir(parent) else {
        return true;
    };
    for sibling in siblings {
        let Ok(sibling) = sibling else {
            return true;
        };
        if allowed.contains(&sibling.path()) {
            continue;
        }
        let sibling_name = sibling.file_name();
        let Some(sibling_name) = sibling_name.to_str() else {
            return true;
        };
        if folded_path(sibling_name) == folded_destination {
            return true;
        }
    }
    false
}

fn unchanged_members(files: &[NormalizationFile]) -> Vec<RenameMember> {
    files
        .iter()
        .map(|file| RenameMember {
            source: file.path.clone(),
            destination: file.path.clone(),
            source_relative: file.relative_path.clone(),
            destination_relative: String::new(),
            size: file.size,
            modified: file.modified,
            fingerprint: file_fingerprint(&file.path).unwrap_or_default(),
        })
        .collect()
}

fn ownership_is_clear(
    download_state: &VrDownloadState,
    category: NormalizationCategory,
    file: &NormalizationFile,
    folder: &Path,
) -> Result<bool, VrLibraryTrashOwnershipError> {
    let check = |configured: Option<&Path>| configured == Some(folder);
    match category {
        NormalizationCategory::Adult => {
            with_unowned_adult_library_path(download_state, &file.path, check)
        }
        NormalizationCategory::Vr => {
            with_unowned_vr_library_path(download_state, &file.path, check)
        }
    }
}

fn plan_response(plan: &NormalizationPlan) -> Vec<String> {
    let mut response = vec![
        RESPONSE_VERSION.to_owned(),
        plan.id.clone(),
        plan.category.as_str().to_owned(),
        plan.scan_generation.to_string(),
        plan.entries.len().to_string(),
    ];
    for entry in &plan.entries {
        response.extend([
            entry.id.clone(),
            entry.status.to_owned(),
            entry.local_code.clone().unwrap_or_default(),
            entry
                .proof
                .as_ref()
                .map_or_else(String::new, |proof| proof.provider.to_owned()),
            entry
                .proof
                .as_ref()
                .map_or_else(String::new, |proof| proof.provider_id.clone()),
            entry
                .proof
                .as_ref()
                .map_or_else(String::new, |proof| proof.display_code.clone()),
            entry.reason.clone(),
            entry.members.len().to_string(),
        ]);
        for member in &entry.members {
            response.push(member.source_relative.clone());
            response.push(member.destination_relative.clone());
        }
    }
    response
}

fn proof_matches_local(
    category: NormalizationCategory,
    proof: &ProviderProof,
    local_code: &str,
) -> bool {
    let Some(local) = product_code_forms(local_code) else {
        return false;
    };
    let Some(verified) = product_code_forms(&proof.display_code) else {
        return false;
    };
    if local.identity != verified.identity || proof.provider_id.is_empty() {
        return false;
    }
    match proof.provider {
        "FANZA" => fanza_transport_id(category, local_code).as_deref() == Some(&proof.provider_id),
        "JavDB" => true,
        _ => false,
    }
}

pub(crate) fn audit_with(
    state: &FilenameNormalizationState,
    download_state: &VrDownloadState,
    snapshot: NormalizationSnapshot,
    mut fanza_fetch: impl FnMut(&str) -> Result<String, ProviderRequestError>,
    mut javdb_fetch: impl FnMut(&str) -> Result<String, ProviderRequestError>,
) -> Result<Vec<String>, &'static str> {
    if snapshot.files.len() > MAX_AUDIT_ITEMS {
        return Err(NORMALIZATION_FAILED);
    }
    let generation = {
        let mut context = state.0.lock().map_err(|_| NORMALIZATION_FAILED)?;
        context.generation = context.generation.wrapping_add(1);
        context.plan = None;
        context.active_operation = None;
        context.generation
    };
    let mut grouped = BTreeMap::<String, Vec<NormalizationFile>>::new();
    let mut entries = Vec::new();
    for file in snapshot.files.iter().cloned() {
        if let Some(identity) = &file.local_identity {
            grouped.entry(identity.clone()).or_default().push(file);
        } else {
            entries.push(AuditEntry {
                id: hex_sha1(file.relative_path.as_bytes()),
                status: "unresolved",
                local_code: None,
                proof: None,
                reason: "No single supported local product-code identity was parsed.".to_owned(),
                members: vec![RenameMember {
                    source: file.path.clone(),
                    destination: file.path.clone(),
                    source_relative: file.relative_path.clone(),
                    destination_relative: String::new(),
                    size: file.size,
                    modified: file.modified,
                    fingerprint: file_fingerprint(&file.path).unwrap_or_default(),
                }],
            });
        }
    }
    for (identity, mut files) in grouped {
        files.sort_by(|left, right| left.path.cmp(&right.path));
        let displays = files
            .iter()
            .filter_map(|file| file.local_display.clone())
            .collect::<HashSet<_>>();
        let local_code = displays.iter().min().cloned();
        let entry_id = hex_sha1(
            format!(
                "{}\0{}\0{}",
                snapshot.category.as_str(),
                snapshot.generation,
                files
                    .iter()
                    .map(|file| file.relative_path.as_str())
                    .collect::<Vec<_>>()
                    .join("\0")
            )
            .as_bytes(),
        );
        let unsafe_member = files.iter().any(|file| {
            !ownership_is_clear(download_state, snapshot.category, file, &snapshot.folder)
                .unwrap_or(false)
        });
        if unsafe_member {
            entries.push(AuditEntry {
                id: entry_id,
                status: "unsafe",
                local_code,
                proof: None,
                reason:
                    "A current native owner or unavailable ownership boundary forbids mutation."
                        .to_owned(),
                members: files
                    .into_iter()
                    .map(|file| {
                        let fingerprint = file_fingerprint(&file.path).unwrap_or_default();
                        RenameMember {
                            source: file.path.clone(),
                            destination: file.path,
                            source_relative: file.relative_path,
                            destination_relative: String::new(),
                            size: file.size,
                            modified: file.modified,
                            fingerprint,
                        }
                    })
                    .collect(),
            });
            continue;
        }
        let Some(local_code) = local_code else {
            entries.push(AuditEntry {
                id: entry_id,
                status: "conflicting",
                local_code: None,
                proof: None,
                reason: "The grouped files do not retain one local display code.".to_owned(),
                members: unchanged_members(&files),
            });
            continue;
        };
        let request_is_current = || {
            state
                .0
                .lock()
                .is_ok_and(|context| context.generation == generation)
        };
        if !request_is_current() {
            return Err(NORMALIZATION_STALE);
        }
        let mut guarded_fanza = |body: &str| {
            if !request_is_current() {
                return Err(ProviderRequestError::SourceUnavailable);
            }
            let result = fanza_fetch(body);
            if request_is_current() {
                result
            } else {
                Err(ProviderRequestError::SourceUnavailable)
            }
        };
        let mut guarded_javdb = |url: &str| {
            if !request_is_current() {
                return Err(ProviderRequestError::SourceUnavailable);
            }
            let result = javdb_fetch(url);
            if request_is_current() {
                result
            } else {
                Err(ProviderRequestError::SourceUnavailable)
            }
        };
        let proof = match resolve_provider_proof(
            snapshot.category,
            &local_code,
            &mut guarded_fanza,
            &mut guarded_javdb,
        ) {
            Ok(Some(proof)) => proof,
            Ok(None) => {
                entries.push(AuditEntry {
                    id: entry_id,
                    status: "unresolved",
                    local_code: Some(local_code),
                    proof: None,
                    reason: "Neither FANZA nor the allowed JavDB fallback proved an exact item."
                        .to_owned(),
                    members: unchanged_members(&files),
                });
                continue;
            }
            Err((status, reason)) => {
                entries.push(AuditEntry {
                    id: entry_id,
                    status,
                    local_code: Some(local_code),
                    proof: None,
                    reason,
                    members: unchanged_members(&files),
                });
                continue;
            }
        };
        let members = match proposed_members(&snapshot, &files, &proof.display_code) {
            Ok(members) => members,
            Err(reason) => {
                entries.push(AuditEntry {
                    id: entry_id,
                    status: "unresolved",
                    local_code: Some(local_code),
                    proof: Some(proof),
                    reason,
                    members: unchanged_members(&files),
                });
                continue;
            }
        };
        let already_canonical = members
            .iter()
            .all(|member| member.source == member.destination);
        entries.push(AuditEntry {
            id: entry_id,
            status: if already_canonical {
                "already-canonical"
            } else {
                "ready"
            },
            local_code: Some(local_code),
            reason: proof.reason.clone(),
            proof: Some(proof),
            members,
        });
        let _ = identity;
    }
    entries.sort_by(|left, right| left.id.cmp(&right.id));
    let plan = NormalizationPlan {
        id: hex_sha1(
            format!(
                "{}\0{}\0{}\0{}",
                snapshot.category.as_str(),
                snapshot.folder.display(),
                snapshot.generation,
                generation
            )
            .as_bytes(),
        ),
        category: snapshot.category,
        folder: snapshot.folder,
        scan_generation: snapshot.generation,
        entries,
    };
    let response = plan_response(&plan);
    let mut context = state.0.lock().map_err(|_| NORMALIZATION_FAILED)?;
    if context.generation != generation {
        return Err(NORMALIZATION_STALE);
    }
    context.plan = Some(plan);
    Ok(response)
}

pub(crate) fn dismiss(state: &FilenameNormalizationState) -> Result<(), &'static str> {
    let mut context = state.0.lock().map_err(|_| NORMALIZATION_FAILED)?;
    context.generation = context.generation.wrapping_add(1);
    context.plan = None;
    context.active_operation = None;
    Ok(())
}

pub(crate) fn plan_scan_generation(
    state: &FilenameNormalizationState,
    category: NormalizationCategory,
    plan_id: &str,
) -> Result<u64, &'static str> {
    let context = state.0.lock().map_err(|_| NORMALIZATION_FAILED)?;
    let plan = context.plan.as_ref().ok_or(NORMALIZATION_STALE)?;
    if plan.category != category || plan.id != plan_id {
        return Err(NORMALIZATION_STALE);
    }
    Ok(plan.scan_generation)
}

fn exact_current_member(member: &RenameMember, folder: &Path) -> bool {
    let Ok(relative) = member.source.strip_prefix(folder) else {
        return false;
    };
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return false;
    }
    let mut checked = folder.to_path_buf();
    for component in relative.components() {
        checked.push(component);
        let Ok(metadata) = fs::symlink_metadata(&checked) else {
            return false;
        };
        if metadata.file_type().is_symlink() {
            return false;
        }
    }
    let Ok(metadata) = fs::metadata(&member.source) else {
        return false;
    };
    metadata.is_file()
        && is_supported_library_media(&member.source)
        && is_supported_library_media(&member.destination)
        && metadata.len() == member.size
        && metadata.modified().ok() == Some(member.modified)
        && file_fingerprint(&member.source).ok().as_deref() == Some(&member.fingerprint)
        && fs::canonicalize(&member.source).ok().as_deref() == Some(member.source.as_path())
}

fn hex_encode(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn recovery_bytes(plan: &NormalizationPlan, entries: &[AuditEntry]) -> Vec<u8> {
    let mut lines = vec![
        RECOVERY_VERSION.to_owned(),
        hex_encode(plan.category.as_str()),
        hex_encode(&plan.id),
        hex_encode(&plan.folder.to_string_lossy()),
    ];
    for entry in entries {
        for member in &entry.members {
            let token = hex_sha1(format!("{}\0{}", plan.id, member.source_relative).as_bytes());
            let staging_relative = Path::new(&member.source_relative)
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .join(format!(".auto-video-normalize-{token}.pending"))
                .to_string_lossy()
                .into_owned();
            let modified = member
                .modified
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default();
            lines.push(format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                hex_encode(&member.source_relative),
                hex_encode(&member.destination_relative),
                hex_encode(&staging_relative),
                member.size,
                modified.as_secs(),
                modified.subsec_nanos(),
                hex_encode(&member.fingerprint),
                hex_encode(entry.local_code.as_deref().unwrap_or_default()),
            ));
        }
    }
    lines.join("\n").into_bytes()
}

fn write_recovery(path: &Path, plan: &NormalizationPlan, entries: &[AuditEntry]) -> Result<(), ()> {
    let bytes = recovery_bytes(plan, entries);
    if u64::try_from(bytes.len()).map_err(|_| ())? > RECOVERY_MAX_BYTES {
        return Err(());
    }
    let parent = path.parent().ok_or(())?;
    fs::create_dir_all(parent).map_err(|_| ())?;
    if fs::symlink_metadata(path).is_ok() {
        return Err(());
    }
    let temporary = path.with_extension(format!("next-{}", plan.id));
    match fs::symlink_metadata(&temporary) {
        Ok(_) => return Err(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(()),
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|_| ())?;
    file.write_all(&bytes).map_err(|_| ())?;
    file.sync_all().map_err(|_| ())?;
    rename_without_overwrite(&temporary, path).map_err(|_| ())?;
    sync_directory(parent)
}

fn sync_directory(path: &Path) -> Result<(), ()> {
    #[cfg(unix)]
    {
        fs::File::open(path)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| ())
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

fn hex_decode(value: &str) -> Option<String> {
    if !value.len().is_multiple_of(2) || value.len() > 8_192 {
        return None;
    }
    let bytes = value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).ok()?;
            u8::from_str_radix(text, 16).ok()
        })
        .collect::<Option<Vec<_>>>()?;
    String::from_utf8(bytes).ok()
}

fn safe_relative(value: &str) -> bool {
    !value.is_empty()
        && Path::new(value)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn remove_recovery(path: &Path) -> Result<(), ()> {
    let parent = path.parent().ok_or(())?;
    match fs::remove_file(path) {
        Ok(()) => sync_directory(parent),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(()),
    }
}

fn recovery_file_matches(
    folder: &Path,
    relative: &str,
    size: u64,
    modified: SystemTime,
    fingerprint: &str,
) -> bool {
    let path = folder.join(relative);
    let Ok(metadata) = fs::symlink_metadata(&path) else {
        return false;
    };
    metadata.is_file()
        && !metadata.file_type().is_symlink()
        && metadata.len() == size
        && metadata.modified().ok() == Some(modified)
        && file_fingerprint(&path).ok().as_deref() == Some(fingerprint)
        && fs::canonicalize(&path)
            .ok()
            .is_some_and(|canonical| canonical.starts_with(folder) && canonical == path)
}

fn recovery_status_with_source_retirement(
    path: &Path,
    expected_source: Option<(NormalizationCategory, &str)>,
    retire_source: impl FnOnce(&Path) -> Result<(), ()>,
) -> Result<Vec<String>, &'static str> {
    match fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.len() <= RECOVERY_MAX_BYTES => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(vec!["none".to_owned()]);
        }
        _ => return Err(NORMALIZATION_RECOVERY),
    }
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(vec!["none".to_owned()]);
        }
        Err(_) => return Err(NORMALIZATION_RECOVERY),
    };
    if u64::try_from(bytes.len()).map_err(|_| NORMALIZATION_RECOVERY)? > RECOVERY_MAX_BYTES {
        return Err(NORMALIZATION_RECOVERY);
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| NORMALIZATION_RECOVERY)?;
    let mut lines = text.lines();
    if lines.next() != Some(RECOVERY_VERSION) {
        return Err(NORMALIZATION_RECOVERY);
    }
    let category = hex_decode(lines.next().ok_or(NORMALIZATION_RECOVERY)?)
        .filter(|value| NormalizationCategory::parse(value).is_some())
        .ok_or(NORMALIZATION_RECOVERY)?;
    let plan = hex_decode(lines.next().ok_or(NORMALIZATION_RECOVERY)?)
        .filter(|value| value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or(NORMALIZATION_RECOVERY)?;
    let folder = hex_decode(lines.next().ok_or(NORMALIZATION_RECOVERY)?)
        .map(PathBuf::from)
        .filter(|value| value.is_absolute())
        .ok_or(NORMALIZATION_RECOVERY)?;
    let folder_available = fs::canonicalize(&folder).ok().as_deref() == Some(folder.as_path())
        && fs::symlink_metadata(&folder)
            .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink());
    let mut paths = Vec::new();
    let mut affected_codes = BTreeSet::new();
    let mut records = 0usize;
    let mut every_source = true;
    let mut every_destination = true;
    for line in lines {
        let mut fields = line.split('\t');
        let source = fields.next().ok_or(NORMALIZATION_RECOVERY)?;
        let destination = fields.next().ok_or(NORMALIZATION_RECOVERY)?;
        let staging = fields.next().ok_or(NORMALIZATION_RECOVERY)?;
        let size = fields
            .next()
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or(NORMALIZATION_RECOVERY)?;
        let modified_secs = fields
            .next()
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or(NORMALIZATION_RECOVERY)?;
        let modified_nanos = fields
            .next()
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|value| *value < 1_000_000_000)
            .ok_or(NORMALIZATION_RECOVERY)?;
        let fingerprint = hex_decode(fields.next().ok_or(NORMALIZATION_RECOVERY)?)
            .filter(|value| !value.is_empty() && value.len() <= 256)
            .ok_or(NORMALIZATION_RECOVERY)?;
        let local_code = hex_decode(fields.next().ok_or(NORMALIZATION_RECOVERY)?)
            .filter(|value| value.len() <= 128 && product_code_forms(value).is_some())
            .ok_or(NORMALIZATION_RECOVERY)?;
        if fields.next().is_some() {
            return Err(NORMALIZATION_RECOVERY);
        }
        affected_codes.insert(local_code);
        let source = hex_decode(source)
            .filter(|value| safe_relative(value))
            .ok_or(NORMALIZATION_RECOVERY)?;
        let destination = hex_decode(destination)
            .filter(|value| safe_relative(value))
            .ok_or(NORMALIZATION_RECOVERY)?;
        let staging = hex_decode(staging)
            .filter(|value| safe_relative(value))
            .ok_or(NORMALIZATION_RECOVERY)?;
        if !is_supported_library_media(&folder.join(&source))
            || !is_supported_library_media(&folder.join(&destination))
        {
            return Err(NORMALIZATION_RECOVERY);
        }
        let modified = UNIX_EPOCH
            .checked_add(std::time::Duration::new(modified_secs, modified_nanos))
            .ok_or(NORMALIZATION_RECOVERY)?;
        let current = if folder_available {
            [&source, &staging, &destination]
                .into_iter()
                .filter(|relative| {
                    recovery_file_matches(&folder, relative, size, modified, &fingerprint)
                })
                .cloned()
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let foreign = folder_available
            && [&source, &staging, &destination]
                .into_iter()
                .any(|relative| {
                    fs::symlink_metadata(folder.join(relative)).is_ok()
                        && !recovery_file_matches(&folder, relative, size, modified, &fingerprint)
                });
        records += 1;
        every_source &= folder_available && !foreign && current.len() == 1 && current[0] == source;
        every_destination &=
            folder_available && !foreign && current.len() == 1 && current[0] == destination;
        if current.is_empty() {
            paths.push(source);
            paths.push(destination);
        } else {
            for current in current {
                paths.push(current);
                paths.push(destination.clone());
            }
        }
    }
    if records == 0 {
        return Err(NORMALIZATION_RECOVERY);
    }
    if every_source {
        if expected_source.is_some_and(|(expected_category, expected_plan)| {
            category != expected_category.as_str() || plan != expected_plan
        }) {
            return Err(NORMALIZATION_RECOVERY);
        }
        if retire_source(path).is_ok() {
            return Ok(vec!["none".to_owned()]);
        }
        let mut response = vec![
            "cleanup-pending".to_owned(),
            category,
            plan,
            (paths.len() / 2).to_string(),
        ];
        response.extend(paths);
        return Ok(response);
    }
    if every_destination {
        let mut response = vec![
            "committed".to_owned(),
            category,
            plan,
            (paths.len() / 2).to_string(),
        ];
        response.extend(paths);
        response.push(affected_codes.len().to_string());
        response.extend(affected_codes);
        return Ok(response);
    }
    let mut response = vec![
        "attention".to_owned(),
        category,
        plan,
        (paths.len() / 2).to_string(),
    ];
    response.extend(paths);
    Ok(response)
}

pub(crate) fn recovery_status(path: &Path) -> Result<Vec<String>, &'static str> {
    recovery_status_with_source_retirement(path, None, remove_recovery)
}

pub(crate) fn retire_rolled_back_recovery(
    path: &Path,
    expected_category: NormalizationCategory,
    expected_plan_id: &str,
) -> Result<(), &'static str> {
    let response = recovery_status_with_source_retirement(
        path,
        Some((expected_category, expected_plan_id)),
        remove_recovery,
    )?;
    if response == ["none"] {
        Ok(())
    } else {
        Err(NORMALIZATION_RECOVERY)
    }
}

pub(crate) fn committed_recovery(
    path: &Path,
    expected_category: NormalizationCategory,
    expected_plan_id: &str,
) -> Result<Vec<String>, &'static str> {
    let response = recovery_status(path)?;
    if response.first().map(String::as_str) != Some("committed")
        || response.get(1).map(String::as_str) != Some(expected_category.as_str())
        || response.get(2).map(String::as_str) != Some(expected_plan_id)
    {
        return Err(NORMALIZATION_RECOVERY);
    }
    let path_count = response
        .get(3)
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or(NORMALIZATION_RECOVERY)?;
    let code_count_index = 4usize
        .checked_add(path_count.checked_mul(2).ok_or(NORMALIZATION_RECOVERY)?)
        .ok_or(NORMALIZATION_RECOVERY)?;
    let code_count = response
        .get(code_count_index)
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or(NORMALIZATION_RECOVERY)?;
    if response.len() != code_count_index + 1 + code_count || code_count == 0 {
        return Err(NORMALIZATION_RECOVERY);
    }
    Ok(response[code_count_index + 1..].to_vec())
}

fn finalize_committed_recovery(
    path: &Path,
    category: NormalizationCategory,
    plan_id: &str,
) -> Result<(), ()> {
    committed_recovery(path, category, plan_id).map_err(|_| ())?;
    remove_recovery(path)
}

pub(crate) fn reconcile_committed_with<T, E>(
    recovery_path: &Path,
    category: NormalizationCategory,
    plan_id: &str,
    prepare_cache: impl FnOnce() -> Result<(), E>,
    scan: impl FnOnce() -> Result<T, E>,
) -> Result<T, &'static str> {
    prepare_cache().map_err(|_| NORMALIZATION_COMMITTED)?;
    let response = scan().map_err(|_| NORMALIZATION_COMMITTED)?;
    finalize_committed_recovery(recovery_path, category, plan_id)
        .map_err(|_| NORMALIZATION_COMMITTED)?;
    Ok(response)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RenameExecution {
    Complete,
    RolledBack,
    RecoveryRequired,
}

fn rolled_back_error(
    recovery_path: &Path,
    remove: impl FnOnce(&Path) -> Result<(), ()>,
) -> &'static str {
    if remove(recovery_path).is_ok() {
        NORMALIZATION_FAILED
    } else {
        NORMALIZATION_RECOVERY
    }
}

#[cfg(test)]
fn execute_rename_plan(
    plan: &NormalizationPlan,
    entries: &[AuditEntry],
    mut rename: impl FnMut(&Path, &Path) -> std::io::Result<()>,
) -> RenameExecution {
    execute_rename_plan_while_current(plan, entries, &mut rename, || true, || true)
}

fn execute_rename_plan_while_current(
    plan: &NormalizationPlan,
    entries: &[AuditEntry],
    mut rename: impl FnMut(&Path, &Path) -> std::io::Result<()>,
    is_current: impl Fn() -> bool,
    ownership_is_current: impl Fn() -> bool,
) -> RenameExecution {
    let mut staged = Vec::<(PathBuf, RenameMember)>::new();
    let mut completed = Vec::<RenameMember>::new();
    let operation = (|| -> Result<(), ()> {
        for entry in entries {
            for member in &entry.members {
                if !is_current()
                    || !ownership_is_current()
                    || !exact_current_member(member, &plan.folder)
                {
                    return Err(());
                }
                let parent = member.source.parent().ok_or(())?;
                let token = hex_sha1(format!("{}\0{}", plan.id, member.source_relative).as_bytes());
                let temporary = parent.join(format!(".auto-video-normalize-{token}.pending"));
                if temporary.exists() {
                    return Err(());
                }
                rename(&member.source, &temporary).map_err(|_| ())?;
                staged.push((temporary, member.clone()));
            }
        }
        let allowed = staged
            .iter()
            .map(|(temporary, _)| temporary.clone())
            .collect::<HashSet<_>>();
        for (temporary, member) in &staged {
            if !is_current() || !ownership_is_current() {
                return Err(());
            }
            let metadata = fs::symlink_metadata(temporary).map_err(|_| ())?;
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.len() != member.size
                || unrelated_sibling_collision(&member.destination, &allowed)
                || (member.destination.exists()
                    && !staged.iter().any(|(_, staged_member)| {
                        folded_path(&staged_member.source_relative)
                            == folded_path(&member.destination_relative)
                    }))
            {
                return Err(());
            }
            rename(temporary, &member.destination).map_err(|_| ())?;
            completed.push(member.clone());
        }
        Ok(())
    })();
    if operation.is_ok() {
        return RenameExecution::Complete;
    }
    let mut rollback_complete = true;
    for member in completed.iter().rev() {
        if !ownership_is_current() || rename(&member.destination, &member.source).is_err() {
            rollback_complete = false;
        }
    }
    for (temporary, member) in staged.iter().rev() {
        if temporary.exists()
            && (!ownership_is_current() || rename(temporary, &member.source).is_err())
        {
            rollback_complete = false;
        }
    }
    if rollback_complete {
        RenameExecution::RolledBack
    } else {
        RenameExecution::RecoveryRequired
    }
}

pub(crate) fn apply_with_current(
    state: &FilenameNormalizationState,
    download_state: &VrDownloadState,
    recovery_path: &Path,
    category: NormalizationCategory,
    plan_id: &str,
    selected_ids: &[String],
    is_scan_current: impl Fn() -> bool,
) -> Result<(NormalizationCategory, u64, Vec<String>), &'static str> {
    if selected_ids.is_empty() || selected_ids.len() > MAX_AUDIT_ITEMS {
        return Err(NORMALIZATION_STALE);
    }
    let (plan, operation_generation) = {
        let mut context = state.0.lock().map_err(|_| NORMALIZATION_FAILED)?;
        let current = context.plan.as_ref().ok_or(NORMALIZATION_STALE)?;
        if current.id != plan_id || current.category != category || !is_scan_current() {
            return Err(NORMALIZATION_STALE);
        }
        let plan = context.plan.take().ok_or(NORMALIZATION_STALE)?;
        let generation = context.generation;
        context.active_operation = Some(generation);
        (plan, generation)
    };
    let operation_is_current = || {
        is_scan_current()
            && state.0.lock().is_ok_and(|context| {
                context.generation == operation_generation
                    && context.active_operation == Some(operation_generation)
            })
    };
    let _operation_guard = OperationGuard {
        state: state.clone(),
        generation: operation_generation,
    };
    let selected = selected_ids.iter().cloned().collect::<HashSet<_>>();
    if selected.len() != selected_ids.len() {
        return Err(NORMALIZATION_STALE);
    }
    let entries = plan
        .entries
        .iter()
        .filter(|entry| selected.contains(&entry.id))
        .cloned()
        .collect::<Vec<_>>();
    if entries.len() != selected.len() || entries.iter().any(|entry| entry.status != "ready") {
        return Err(NORMALIZATION_STALE);
    }
    let mut source_paths = HashSet::new();
    let mut destination_paths = HashSet::new();
    for entry in &entries {
        let proof = entry.proof.as_ref().ok_or(NORMALIZATION_STALE)?;
        if entry
            .local_code
            .as_deref()
            .is_none_or(|local_code| !proof_matches_local(category, proof, local_code))
        {
            return Err(NORMALIZATION_STALE);
        }
        for member in &entry.members {
            if !exact_current_member(member, &plan.folder)
                || !ownership_is_clear(
                    download_state,
                    category,
                    &NormalizationFile {
                        path: member.source.clone(),
                        relative_path: member.source_relative.clone(),
                        size: member.size,
                        modified: member.modified,
                        local_identity: None,
                        local_display: None,
                    },
                    &plan.folder,
                )
                .unwrap_or(false)
            {
                return Err(NORMALIZATION_STALE);
            }
            source_paths.insert(folded_path(&member.source_relative));
            if !destination_paths.insert(folded_path(&member.destination_relative)) {
                return Err(NORMALIZATION_FAILED);
            }
        }
    }
    for entry in &entries {
        for member in &entry.members {
            if member.destination.exists()
                && !source_paths.contains(&folded_path(&member.destination_relative))
            {
                return Err(NORMALIZATION_FAILED);
            }
            let allowed = entries
                .iter()
                .flat_map(|entry| entry.members.iter().map(|member| member.source.clone()))
                .collect::<HashSet<_>>();
            if unrelated_sibling_collision(&member.destination, &allowed) {
                return Err(NORMALIZATION_FAILED);
            }
        }
    }
    let selected_paths = entries
        .iter()
        .flat_map(|entry| {
            entry
                .members
                .iter()
                .flat_map(|member| [member.source.clone(), member.destination.clone()])
        })
        .collect::<Vec<_>>();
    let execute = |configured_folder: Option<&Path>| {
        if configured_folder != Some(plan.folder.as_path()) || !operation_is_current() {
            return Err(NORMALIZATION_STALE);
        }
        write_recovery(recovery_path, &plan, &entries).map_err(|_| NORMALIZATION_RECOVERY)?;
        Ok(execute_rename_plan_while_current(
            &plan,
            &entries,
            rename_without_overwrite,
            operation_is_current,
            || true,
        ))
    };
    let outcome = match category {
        NormalizationCategory::Adult => {
            with_unowned_adult_library_paths(download_state, &selected_paths, execute)
        }
        NormalizationCategory::Vr => {
            with_unowned_vr_library_paths(download_state, &selected_paths, execute)
        }
    }
    .map_err(|_| NORMALIZATION_STALE)??;
    match outcome {
        RenameExecution::Complete => {}
        RenameExecution::RolledBack => {
            return Err(rolled_back_error(recovery_path, remove_recovery));
        }
        RenameExecution::RecoveryRequired => return Err(NORMALIZATION_RECOVERY),
    }
    let affected_codes = entries
        .iter()
        .filter_map(|entry| entry.local_code.clone())
        .collect::<Vec<_>>();
    Ok((category, plan.scan_generation, affected_codes))
}

#[cfg(test)]
fn apply(
    state: &FilenameNormalizationState,
    download_state: &VrDownloadState,
    recovery_path: &Path,
    category: NormalizationCategory,
    plan_id: &str,
    selected_ids: &[String],
) -> Result<(NormalizationCategory, u64, Vec<String>), &'static str> {
    apply_with_current(
        state,
        download_state,
        recovery_path,
        category,
        plan_id,
        selected_ids,
        || true,
    )
}

pub(crate) fn production_audit(
    state: &FilenameNormalizationState,
    download_state: &VrDownloadState,
    snapshot: NormalizationSnapshot,
) -> Result<Vec<String>, &'static str> {
    audit_with(
        state,
        download_state,
        snapshot,
        fanza_catalog::fetch_graphql_document,
        javdb_catalog::fetch_api_document,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vr_download::{
        configure_adult_download_folder, prepare_unowned_library_paths_for_test,
    };
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "auto-video-filename-normalization-{}-{}",
                std::process::id(),
                NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).expect("fixture directory must be created");
            Self(fs::canonicalize(path).expect("fixture path must canonicalize"))
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn normalization_file(folder: &Path, name: &str, identity: Option<&str>) -> NormalizationFile {
        let path = folder.join(name);
        fs::write(&path, b"media").expect("fixture file must be written");
        let metadata = fs::metadata(&path).expect("fixture metadata must exist");
        NormalizationFile {
            path,
            relative_path: name.to_owned(),
            size: metadata.len(),
            modified: metadata.modified().expect("modified time must exist"),
            local_identity: identity.map(str::to_owned),
            local_display: identity.map(str::to_owned),
        }
    }

    fn ready_download_state(folder: &Path) -> VrDownloadState {
        let state = VrDownloadState::default();
        configure_adult_download_folder(&state, Some(folder.to_path_buf()))
            .expect("Adult folder must configure");
        prepare_unowned_library_paths_for_test(&state, folder.join("transfers.json"));
        state
    }

    #[test]
    fn fanza_maker_code_is_separate_from_transport_identity() {
        let document = r#"{"data":{"ppvContent":{"id":"dsvr00069","contentType":"VR","makerContentId":"DSVR-069"}}}"#;
        let proof = parse_fanza_exact(document, NormalizationCategory::Vr, "DSVR-69", "dsvr00069")
            .expect("the provider document must parse")
            .expect("the exact maker code must exist");
        assert_eq!(proof.provider_id, "dsvr00069");
        assert_eq!(proof.display_code, "DSVR-069");
        assert_eq!(
            fanza_transport_id(NormalizationCategory::Vr, "DSVR-69").as_deref(),
            Some("dsvr00069")
        );
        assert_eq!(
            fanza_transport_id(NormalizationCategory::Vr, "3DSVR-1871").as_deref(),
            Some("13dsvr01871")
        );
        assert_ne!(
            fanza_transport_id(NormalizationCategory::Vr, "3DSVR-1871").as_deref(),
            Some("3dsvr01871")
        );
        assert_eq!(
            fanza_transport_id(NormalizationCategory::Adult, "DSVR-69").as_deref(),
            None
        );
    }

    #[test]
    fn fanza_identity_and_category_are_exact_and_malformed_data_never_falls_through() {
        for document in [
            r#"{"data":{"ppvContent":{"id":"dsvr00070","contentType":"VR","makerContentId":"DSVR-069"}}}"#,
            r#"{"data":{"ppvContent":{"id":"dsvr00069","contentType":"TWO_DIMENSION","makerContentId":"DSVR-069"}}}"#,
            r#"{"data":{"ppvContent":{"id":"dsvr00069","contentType":"VR","makerContentId":"DSVR-070"}}}"#,
            r#"{"data":{"ppvContent":{"id":" dsvr00069","contentType":"VR","makerContentId":"DSVR-069"}}}"#,
        ] {
            assert_eq!(
                parse_fanza_exact(document, NormalizationCategory::Vr, "DSVR-69", "dsvr00069"),
                Err(ProviderRequestError::Provider)
            );
        }
    }

    #[test]
    fn javdb_runs_only_after_exact_no_match_or_missing_maker() {
        for fanza in [
            r#"{"data":{"ppvContent":null}}"#,
            r#"{"data":{"ppvContent":{"id":"dsvr00069","contentType":"VR","makerContentId":null}}}"#,
        ] {
            let mut javdb_calls = 0;
            let result = resolve_provider_proof(
                NormalizationCategory::Vr,
                "DSVR-69",
                &mut |_| Ok(fanza.to_owned()),
                &mut |url| {
                    javdb_calls += 1;
                    if url.contains("/movies/") {
                        Ok(r#"{"success":1,"data":{"movie":{"id":"provider69","number":"DSVR-069","tags":[{"id":"212"}],"cover_url":null,"thumb_url":null}}}"#.to_owned())
                    } else {
                        Ok(
                            r#"{"success":1,"data":{"movies":[{"id":"provider69","number":"DSVR-069"}]}}"#
                                .to_owned(),
                        )
                    }
                },
            );
            assert!(result.is_ok(), "{result:?}");
            assert!(javdb_calls > 0);
        }
        let mut javdb_calls = 0;
        let result = resolve_provider_proof(
            NormalizationCategory::Vr,
            "DSVR-69",
            &mut |_| Err(ProviderRequestError::Network),
            &mut |_| {
                javdb_calls += 1;
                panic!("JavDB must not run after a FANZA failure")
            },
        );
        assert!(result.is_err());
        assert_eq!(javdb_calls, 0);
    }

    #[test]
    fn exact_multipart_names_preserve_labels_and_reject_ambiguity() {
        assert_eq!(
            multipart_label("CODE - Part 001", false).as_deref(),
            Some("Part 001")
        );
        assert_eq!(
            multipart_label("CODE - PT_02", true).as_deref(),
            Some("PT_02")
        );
        assert_eq!(multipart_label("CODE Part 1 Disc 2", true), None);
        assert_eq!(multipart_label("CODE Part 0", true), None);
        assert_eq!(
            folded_path("Cafe\u{301}/FILE.MP4"),
            folded_path("Café/file.mp4")
        );
    }

    #[test]
    fn one_and_multipart_proposals_preserve_extensions_and_exact_labels() {
        let fixture = Fixture::new();
        let one = normalization_file(&fixture.0, "DSVR-69.MKV", Some("DSVR-69"));
        let snapshot = NormalizationSnapshot {
            category: NormalizationCategory::Vr,
            folder: fixture.0.clone(),
            generation: 1,
            files: vec![one.clone()],
        };
        assert_eq!(
            proposed_members(&snapshot, &[one], "DSVR-069").expect("one member must resolve")[0]
                .destination_relative,
            "DSVR-069.MKV"
        );
        let first = normalization_file(&fixture.0, "DSVR-69 - Part 001.mp4", Some("DSVR-69"));
        let second = normalization_file(&fixture.0, "DSVR-69 - PT_02.Mp4", Some("DSVR-69"));
        let proposals = proposed_members(&snapshot, &[first, second], "DSVR-069")
            .expect("distinct exact labels must resolve");
        assert_eq!(proposals[0].destination_relative, "DSVR-069 - Part 001.mp4");
        assert_eq!(proposals[1].destination_relative, "DSVR-069 - PT_02.Mp4");
    }

    #[test]
    fn malformed_fanza_never_dispatches_the_javdb_fallback() {
        for document in [
            r#"{}"#,
            r#"{"data":{"ppvContent":[]}}"#,
            r#"{"data":{"ppvContent":{"id":" dsvr00069","contentType":"VR","makerContentId":"DSVR-069"}}}"#,
            r#"{"data":{"ppvContent":{"id":"dsvr00069","contentType":"VR","makerContentId":"DSVR-070"}}}"#,
        ] {
            let mut javdb_calls = 0;
            let result = resolve_provider_proof(
                NormalizationCategory::Vr,
                "DSVR-69",
                &mut |_| Ok(document.to_owned()),
                &mut |_| {
                    javdb_calls += 1;
                    Ok(String::new())
                },
            );
            assert!(result.is_err());
            assert_eq!(javdb_calls, 0);
        }
    }

    #[test]
    fn changed_missing_outside_and_unsupported_members_are_not_current() {
        let fixture = Fixture::new();
        let file = normalization_file(&fixture.0, "CAWB-1.mp4", Some("CAWB-1"));
        let snapshot = NormalizationSnapshot {
            category: NormalizationCategory::Adult,
            folder: fixture.0.clone(),
            generation: 1,
            files: vec![file.clone()],
        };
        let member = proposed_members(&snapshot, std::slice::from_ref(&file), "CAWB-001")
            .expect("proposal must resolve")
            .remove(0);
        assert!(exact_current_member(&member, &fixture.0));
        fs::write(&file.path, b"changed media").expect("fixture must change");
        assert!(!exact_current_member(&member, &fixture.0));
        fs::remove_file(&file.path).expect("fixture must be removed");
        assert!(!exact_current_member(&member, &fixture.0));

        let outside = Fixture::new();
        let outside_file = normalization_file(&outside.0, "CAWB-1.mp4", Some("CAWB-1"));
        let metadata = fs::metadata(&outside_file.path).expect("outside metadata must exist");
        let outside_member = RenameMember {
            source: outside_file.path,
            destination: outside.0.join("CAWB-001.mp4"),
            source_relative: "CAWB-1.mp4".to_owned(),
            destination_relative: "CAWB-001.mp4".to_owned(),
            size: metadata.len(),
            modified: metadata
                .modified()
                .expect("outside modified time must exist"),
            fingerprint: file_fingerprint(&outside.0.join("CAWB-1.mp4"))
                .expect("outside fingerprint must exist"),
        };
        assert!(!exact_current_member(&outside_member, &fixture.0));
        let unsupported = normalization_file(&fixture.0, "CAWB-1.txt", Some("CAWB-1"));
        let unsupported_metadata = fs::metadata(&unsupported.path).expect("metadata must exist");
        assert!(!exact_current_member(
            &RenameMember {
                source: unsupported.path,
                destination: fixture.0.join("CAWB-001.txt"),
                source_relative: "CAWB-1.txt".to_owned(),
                destination_relative: "CAWB-001.txt".to_owned(),
                size: unsupported_metadata.len(),
                modified: unsupported_metadata
                    .modified()
                    .expect("modified time must exist"),
                fingerprint: file_fingerprint(&fixture.0.join("CAWB-1.txt"))
                    .expect("unsupported fingerprint must exist"),
            },
            &fixture.0
        ));
    }

    #[test]
    fn audit_accounts_for_ready_and_unassociated_files_without_mutation() {
        let fixture = Fixture::new();
        let first = normalization_file(&fixture.0, "CAWB-1.MP4", Some("CAWB-1"));
        let second = normalization_file(&fixture.0, "notes-video.mp4", None);
        let state = FilenameNormalizationState::default();
        let download_state = ready_download_state(&fixture.0);
        let response = audit_with(
            &state,
            &download_state,
            NormalizationSnapshot {
                category: NormalizationCategory::Adult,
                folder: fixture.0.clone(),
                generation: 7,
                files: vec![first.clone(), second],
            },
            |_| Ok(r#"{"data":{"ppvContent":{"id":"cawb00001","contentType":"TWO_DIMENSION","makerContentId":"CAWB-001"}}}"#.to_owned()),
            |_| panic!("JavDB must not run after exact FANZA proof"),
        )
        .expect("audit must succeed");
        assert_eq!(response[4], "2");
        assert!(response.iter().any(|field| field == "ready"));
        assert!(response.iter().any(|field| field == "unresolved"));
        assert!(response.iter().any(|field| field == "CAWB-1.MP4"));
        assert!(first.path.exists(), "audit must not rename a file");
    }

    #[test]
    fn unavailable_native_ownership_marks_the_item_unsafe_without_provider_work() {
        let fixture = Fixture::new();
        let file = normalization_file(&fixture.0, "CAWB-1.mp4", Some("CAWB-1"));
        let response = audit_with(
            &FilenameNormalizationState::default(),
            &VrDownloadState::default(),
            NormalizationSnapshot {
                category: NormalizationCategory::Adult,
                folder: fixture.0.clone(),
                generation: 1,
                files: vec![file],
            },
            |_| panic!("an unsafe item must not dispatch FANZA"),
            |_| panic!("an unsafe item must not dispatch JavDB"),
        )
        .expect("unsafe audit must still account for the item");
        assert!(response.iter().any(|field| field == "unsafe"));
    }

    #[test]
    fn stale_fanza_completion_dispatches_no_javdb_fallback() {
        let fixture = Fixture::new();
        let file = normalization_file(&fixture.0, "CAWB-1.mp4", Some("CAWB-1"));
        let state = FilenameNormalizationState::default();
        let stale_state = state.clone();
        let result = audit_with(
            &state,
            &ready_download_state(&fixture.0),
            NormalizationSnapshot {
                category: NormalizationCategory::Adult,
                folder: fixture.0.clone(),
                generation: 1,
                files: vec![file],
            },
            move |_| {
                dismiss(&stale_state).expect("newer context must invalidate the audit");
                Ok(r#"{"data":{"ppvContent":null}}"#.to_owned())
            },
            |_| panic!("a stale FANZA completion must not dispatch JavDB"),
        );
        assert_eq!(result, Err(NORMALIZATION_STALE));
    }

    #[test]
    fn confirmed_plan_renames_once_and_rejects_stale_replay_or_overwrite() {
        let fixture = Fixture::new();
        let file = normalization_file(&fixture.0, "CAWB-1.MP4", Some("CAWB-1"));
        let member = proposed_members(
            &NormalizationSnapshot {
                category: NormalizationCategory::Adult,
                folder: fixture.0.clone(),
                generation: 9,
                files: vec![file.clone()],
            },
            std::slice::from_ref(&file),
            "CAWB-001",
        )
        .expect("proposal must be safe")
        .remove(0);
        let entry = AuditEntry {
            id: hex_sha1(b"entry"),
            status: "ready",
            local_code: Some("CAWB-1".to_owned()),
            proof: Some(ProviderProof {
                provider: "FANZA",
                provider_id: "cawb00001".to_owned(),
                display_code: "CAWB-001".to_owned(),
                reason: "exact".to_owned(),
            }),
            reason: "exact".to_owned(),
            members: vec![member],
        };
        let plan = NormalizationPlan {
            id: hex_sha1(b"plan"),
            category: NormalizationCategory::Adult,
            folder: fixture.0.clone(),
            scan_generation: 9,
            entries: vec![entry.clone()],
        };
        let state = FilenameNormalizationState::default();
        state.0.lock().expect("state must lock").plan = Some(plan.clone());
        let download_state = ready_download_state(&fixture.0);
        let recovery = fixture.0.join("recovery");
        let destination = fixture.0.join("CAWB-001.MP4");
        fs::write(&destination, b"unrelated").expect("collision fixture must be written");
        assert_eq!(
            apply(
                &state,
                &download_state,
                &recovery,
                NormalizationCategory::Adult,
                &plan.id,
                std::slice::from_ref(&entry.id),
            ),
            Err(NORMALIZATION_FAILED)
        );
        assert!(file.path.exists(), "a collision must not mutate the source");
        assert_eq!(
            fs::read(&destination).expect("collision must remain"),
            b"unrelated"
        );
        fs::remove_file(&destination).expect("collision fixture must be removed");
        state.0.lock().expect("state must lock").plan = Some(plan.clone());
        apply(
            &state,
            &download_state,
            &recovery,
            NormalizationCategory::Adult,
            &plan.id,
            std::slice::from_ref(&entry.id),
        )
        .expect("confirmed rename must succeed");
        assert!(!file.path.exists());
        assert!(destination.exists());
        assert!(
            recovery.exists(),
            "committed renames retain reconciliation evidence"
        );
        finalize_committed_recovery(&recovery, plan.category, &plan.id)
            .expect("successful reconciliation must retire exact evidence");
        assert!(!recovery.exists());
        assert_eq!(
            apply(
                &state,
                &download_state,
                &recovery,
                NormalizationCategory::Adult,
                &plan.id,
                &[entry.id],
            ),
            Err(NORMALIZATION_STALE)
        );
    }

    #[test]
    fn durable_recovery_reports_every_current_path_and_replaces_interrupted_temporary_write() {
        let fixture = Fixture::new();
        let recovery = fixture.0.join("recovery");
        fs::write(recovery.with_extension("next"), b"interrupted")
            .expect("interrupted temporary must be written");
        let file = normalization_file(&fixture.0, "DSVR-69.mp4", Some("DSVR-69"));
        let plan = NormalizationPlan {
            id: hex_sha1(b"recovery-plan"),
            category: NormalizationCategory::Adult,
            folder: fixture.0.clone(),
            scan_generation: 1,
            entries: Vec::new(),
        };
        let entry = AuditEntry {
            id: hex_sha1(b"recovery-entry"),
            status: "ready",
            local_code: Some("DSVR-69".to_owned()),
            proof: None,
            reason: "fixture".to_owned(),
            members: vec![RenameMember {
                source: file.path,
                destination: fixture.0.join("DSVR-069.mp4"),
                source_relative: "DSVR-69.mp4".to_owned(),
                destination_relative: "DSVR-069.mp4".to_owned(),
                size: file.size,
                modified: file.modified,
                fingerprint: file_fingerprint(&fixture.0.join("DSVR-69.mp4"))
                    .expect("fingerprint must exist"),
            }],
        };
        write_recovery(&recovery, &plan, &[entry]).expect("recovery must be durable");
        let staging = format!(
            ".auto-video-normalize-{}.pending",
            hex_sha1(format!("{}\0{}", plan.id, "DSVR-69.mp4").as_bytes())
        );
        fs::rename(fixture.0.join("DSVR-69.mp4"), fixture.0.join(&staging))
            .expect("fixture must simulate an interrupted staged rename");
        assert_eq!(
            recovery_status(&recovery).expect("recovery status must parse"),
            vec![
                "attention",
                "adult",
                &plan.id,
                "1",
                &staging,
                "DSVR-069.mp4",
            ]
        );
        fs::rename(fixture.0.join(&staging), fixture.0.join("DSVR-69.mp4"))
            .expect("fixture rollback must complete");
        assert_eq!(
            recovery_status(&recovery).expect("restart must reconcile rollback"),
            vec!["none"]
        );
        assert!(!recovery.exists());
    }

    #[test]
    fn partial_failure_rolls_back_or_reports_durable_recovery_need() {
        let fixture = Fixture::new();
        let first = normalization_file(&fixture.0, "DSVR-69 Part 1.mp4", Some("DSVR-69"));
        let second = normalization_file(&fixture.0, "DSVR-69 Part 2.mp4", Some("DSVR-69"));
        let snapshot = NormalizationSnapshot {
            category: NormalizationCategory::Vr,
            folder: fixture.0.clone(),
            generation: 1,
            files: vec![first.clone(), second.clone()],
        };
        let entry = AuditEntry {
            id: hex_sha1(b"rollback-entry"),
            status: "ready",
            local_code: Some("DSVR-69".to_owned()),
            proof: None,
            reason: "fixture".to_owned(),
            members: proposed_members(&snapshot, &[first.clone(), second.clone()], "DSVR-069")
                .expect("multipart proposal must resolve"),
        };
        let plan = NormalizationPlan {
            id: hex_sha1(b"rollback-plan"),
            category: NormalizationCategory::Vr,
            folder: fixture.0.clone(),
            scan_generation: 1,
            entries: vec![entry.clone()],
        };
        let mut calls = 0;
        let outcome = execute_rename_plan(&plan, &[entry], |source, destination| {
            calls += 1;
            if calls == 4 {
                Err(std::io::Error::other("injected destination failure"))
            } else {
                fs::rename(source, destination)
            }
        });
        assert_eq!(outcome, RenameExecution::RolledBack);
        assert!(first.path.exists());
        assert!(second.path.exists());

        let entry = AuditEntry {
            id: hex_sha1(b"recovery-required-entry"),
            status: "ready",
            local_code: Some("DSVR-69".to_owned()),
            proof: None,
            reason: "fixture".to_owned(),
            members: proposed_members(&snapshot, &[first, second], "DSVR-069")
                .expect("multipart proposal must resolve again"),
        };
        let mut calls = 0;
        assert_eq!(
            execute_rename_plan(&plan, &[entry], |source, destination| {
                calls += 1;
                if calls >= 4 {
                    Err(std::io::Error::other("injected rollback failure"))
                } else {
                    fs::rename(source, destination)
                }
            }),
            RenameExecution::RecoveryRequired
        );
    }

    #[test]
    fn staged_execution_handles_a_case_only_filename_change() {
        let fixture = Fixture::new();
        let file = normalization_file(&fixture.0, "dsvr-069.mp4", Some("DSVR-069"));
        let metadata = fs::metadata(&file.path).expect("metadata must exist");
        let member = RenameMember {
            source: file.path.clone(),
            destination: fixture.0.join("DSVR-069.mp4"),
            source_relative: "dsvr-069.mp4".to_owned(),
            destination_relative: "DSVR-069.mp4".to_owned(),
            size: metadata.len(),
            modified: metadata.modified().expect("modified time must exist"),
            fingerprint: file_fingerprint(&file.path).expect("fingerprint must exist"),
        };
        let plan = NormalizationPlan {
            id: hex_sha1(b"case-only-plan"),
            category: NormalizationCategory::Vr,
            folder: fixture.0.clone(),
            scan_generation: 1,
            entries: Vec::new(),
        };
        let entry = AuditEntry {
            id: hex_sha1(b"case-only-entry"),
            status: "ready",
            local_code: Some("DSVR-069".to_owned()),
            proof: None,
            reason: "fixture".to_owned(),
            members: vec![member],
        };
        assert_eq!(
            execute_rename_plan(&plan, &[entry], |source, destination| {
                fs::rename(source, destination)
            }),
            RenameExecution::Complete
        );
        assert!(fixture.0.join("DSVR-069.mp4").exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_member_is_never_current() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new();
        let target = normalization_file(&fixture.0, "target.mp4", None);
        let link = fixture.0.join("CAWB-1.mp4");
        symlink(&target.path, &link).expect("fixture symlink must be created");
        let metadata = fs::metadata(&link).expect("symlink target metadata must exist");
        assert!(!exact_current_member(
            &RenameMember {
                source: link,
                destination: fixture.0.join("CAWB-001.mp4"),
                source_relative: "CAWB-1.mp4".to_owned(),
                destination_relative: "CAWB-001.mp4".to_owned(),
                size: metadata.len(),
                modified: metadata.modified().expect("modified time must exist"),
                fingerprint: file_fingerprint(&target.path).expect("fingerprint must exist"),
            },
            &fixture.0
        ));
    }

    #[test]
    fn unsupported_fanza_transport_families_remain_unresolved_without_provider_dispatch() {
        for (category, code) in [
            (NormalizationCategory::Adult, "ADLT-123"),
            (NormalizationCategory::Vr, "MDVR-419"),
        ] {
            let mut fanza_calls = 0;
            let mut javdb_calls = 0;
            let result = resolve_provider_proof(
                category,
                code,
                &mut |_| {
                    fanza_calls += 1;
                    panic!("an unsupported transport family must not query FANZA")
                },
                &mut |_| {
                    javdb_calls += 1;
                    panic!("a guessed FANZA no-match must not authorize JavDB")
                },
            )
            .expect("unsupported evidence must remain a truthful unresolved result");
            assert!(result.is_none());
            assert_eq!(fanza_calls, 0);
            assert_eq!(javdb_calls, 0);
        }
    }

    #[test]
    fn evidence_backed_fanza_no_match_and_missing_maker_allow_javdb_fallback() {
        for (category, code, content_id, content_type, display, tag) in [
            (
                NormalizationCategory::Adult,
                "CAWB-1",
                "cawb00001",
                "TWO_DIMENSION",
                "CAWB-001",
                None,
            ),
            (
                NormalizationCategory::Vr,
                "3DSVR-1871",
                "13dsvr01871",
                "VR",
                "3DSVR-01871",
                Some("212"),
            ),
        ] {
            for fanza in [
                r#"{"data":{"ppvContent":null}}"#.to_owned(),
                format!(
                    r#"{{"data":{{"ppvContent":{{"id":"{content_id}","contentType":"{content_type}","makerContentId":null}}}}}}"#
                ),
            ] {
                let mut fanza_calls = 0;
                let mut javdb_calls = 0;
                let proof = resolve_provider_proof(
                    category,
                    code,
                    &mut |body| {
                        fanza_calls += 1;
                        assert!(body.contains(content_id));
                        Ok(fanza.clone())
                    },
                    &mut |url| {
                        javdb_calls += 1;
                        if url.contains("/movies/") {
                            let tags = tag.map_or_else(
                                || "[]".to_owned(),
                                |id| format!(r#"[{{"id":"{id}"}}]"#),
                            );
                            Ok(format!(r#"{{"success":1,"data":{{"movie":{{"id":"item","number":"{display}","tags":{tags},"cover_url":null,"thumb_url":null}}}}}}"#))
                        } else {
                            Ok(format!(r#"{{"success":1,"data":{{"movies":[{{"id":"item","number":"{display}"}}]}}}}"#))
                        }
                    },
                )
                .expect("the allowed fallback must complete")
                .expect("JavDB must prove the exact item");
                assert_eq!(proof.provider, "JavDB");
                assert_eq!(fanza_calls, 1);
                assert!(javdb_calls >= 2);
            }
        }
    }

    #[test]
    fn evidence_backed_fanza_failure_or_conflict_never_falls_through() {
        let documents = [
            Err(ProviderRequestError::Network),
            Ok(r#"{"data":{"ppvContent":{"id":"13dsvr01871","contentType":"VR","makerContentId":"3DSVR-01872"}}}"#.to_owned()),
        ];
        for document in documents {
            let mut javdb_calls = 0;
            let result = resolve_provider_proof(
                NormalizationCategory::Vr,
                "3DSVR-1871",
                &mut |_| document.clone(),
                &mut |_| {
                    javdb_calls += 1;
                    Err(ProviderRequestError::Provider)
                },
            );
            assert!(result.is_err());
            assert_eq!(javdb_calls, 0);
        }
    }

    #[test]
    fn compact_and_repeated_multipart_labels_are_rejected_for_both_categories() {
        for (category, allow_pt) in [
            (NormalizationCategory::Adult, false),
            (NormalizationCategory::Vr, true),
        ] {
            assert_eq!(multipart_label("Feature Part 1-2", allow_pt), None);
            assert_eq!(multipart_label("Feature CD1+2", allow_pt), None);
            assert_eq!(multipart_label("Feature Part 1&2", allow_pt), None);
            assert_eq!(multipart_label("Feature CD1,2", allow_pt), None);
            assert_eq!(multipart_label("Feature Disc 3:4", allow_pt), None);
            assert_eq!(multipart_label("Feature Part 1・2", allow_pt), None);
            assert_eq!(multipart_label("Feature CD1，2", allow_pt), None);
            assert_eq!(multipart_label("Feature Part 1—2", allow_pt), None);
            assert_eq!(multipart_label("Feature Part 01 CD 01", allow_pt), None);

            for ambiguous in [
                "Part 1&2",
                "CD1,2",
                "Disc 3:4",
                "Part 1・2",
                "CD1，2",
                "Part 1—2",
            ] {
                let fixture = Fixture::new();
                let first = normalization_file(
                    &fixture.0,
                    &format!("DSVR-69 {ambiguous}.mp4"),
                    Some("DSVR-69"),
                );
                let second = normalization_file(&fixture.0, "DSVR-69 Part 04.mp4", Some("DSVR-69"));
                assert!(proposed_members(
                    &NormalizationSnapshot {
                        category,
                        folder: fixture.0.clone(),
                        generation: 1,
                        files: vec![first.clone(), second.clone()],
                    },
                    &[first, second],
                    "DSVR-069",
                )
                .is_err());
            }
        }
    }

    #[test]
    fn portable_component_limits_and_unicode_sibling_collisions_are_preflighted() {
        assert!(validate_portable_organization_component(&"x".repeat(255)).is_ok());
        assert!(validate_portable_organization_component(&"x".repeat(256)).is_err());

        let fixture = Fixture::new();
        let file = normalization_file(&fixture.0, "CAWB-1.mp4", Some("CAWB-1"));
        fs::write(fixture.0.join("CAWB-001.MP4"), b"case collision")
            .expect("case collision fixture must exist");
        let snapshot = NormalizationSnapshot {
            category: NormalizationCategory::Adult,
            folder: fixture.0.clone(),
            generation: 1,
            files: vec![file.clone()],
        };
        assert!(proposed_members(&snapshot, std::slice::from_ref(&file), "CAWB-001").is_err());
        fs::remove_file(fixture.0.join("CAWB-001.MP4")).expect("case fixture must remove");
        fs::write(
            fixture.0.join("CAWB-001 - Cafe\u{301}.mp4"),
            b"unicode collision",
        )
        .expect("Unicode collision fixture must exist");
        let part = normalization_file(&fixture.0, "CAWB-1 - Caf\u{e9}.mp4", Some("CAWB-1"));
        let second = normalization_file(&fixture.0, "CAWB-1 - Part 02.mp4", Some("CAWB-1"));
        let multipart_snapshot = NormalizationSnapshot {
            files: vec![part.clone(), second.clone()],
            ..snapshot
        };
        assert!(proposed_members(&multipart_snapshot, &[part, second], "CAWB-001").is_err());
    }

    #[test]
    fn no_replace_race_rolls_back_without_touching_the_raced_destination() {
        let fixture = Fixture::new();
        let file = normalization_file(&fixture.0, "CAWB-1.mp4", Some("CAWB-1"));
        let snapshot = NormalizationSnapshot {
            category: NormalizationCategory::Adult,
            folder: fixture.0.clone(),
            generation: 1,
            files: vec![file.clone()],
        };
        let member = proposed_members(&snapshot, std::slice::from_ref(&file), "CAWB-001")
            .expect("proposal must resolve")
            .remove(0);
        let plan = NormalizationPlan {
            id: hex_sha1(b"race-plan"),
            category: snapshot.category,
            folder: snapshot.folder,
            scan_generation: 1,
            entries: Vec::new(),
        };
        let entry = AuditEntry {
            id: hex_sha1(b"race-entry"),
            status: "ready",
            local_code: Some("CAWB-1".to_owned()),
            proof: None,
            reason: "fixture".to_owned(),
            members: vec![member.clone()],
        };
        let mut calls = 0;
        assert_eq!(
            execute_rename_plan(&plan, &[entry], |source, destination| {
                calls += 1;
                if calls == 2 {
                    fs::write(destination, b"raced unrelated file")?;
                }
                rename_without_overwrite(source, destination)
            }),
            RenameExecution::RolledBack
        );
        assert!(member.source.exists());
        assert_eq!(
            fs::read(member.destination).expect("race must remain"),
            b"raced unrelated file"
        );
    }

    #[test]
    fn final_placement_rechecks_casefolded_and_nfc_sibling_races() {
        for (destination_name, raced_name) in [
            ("CAWB-001.mp4", "CAWB-001.MP4"),
            ("Caf\u{e9}.mp4", "Cafe\u{301}.mp4"),
        ] {
            let fixture = Fixture::new();
            let file = normalization_file(&fixture.0, "CAWB-1.mp4", Some("CAWB-1"));
            let metadata = fs::metadata(&file.path).expect("metadata must exist");
            let member = RenameMember {
                source: file.path.clone(),
                destination: fixture.0.join(destination_name),
                source_relative: "CAWB-1.mp4".to_owned(),
                destination_relative: destination_name.to_owned(),
                size: metadata.len(),
                modified: metadata.modified().expect("modified time must exist"),
                fingerprint: file_fingerprint(&file.path).expect("fingerprint must exist"),
            };
            let plan = NormalizationPlan {
                id: hex_sha1(destination_name.as_bytes()),
                category: NormalizationCategory::Adult,
                folder: fixture.0.clone(),
                scan_generation: 1,
                entries: Vec::new(),
            };
            let entry = AuditEntry {
                id: hex_sha1(raced_name.as_bytes()),
                status: "ready",
                local_code: Some("CAWB-1".to_owned()),
                proof: None,
                reason: "fixture".to_owned(),
                members: vec![member.clone()],
            };
            let raced = fixture.0.join(raced_name);
            let mut calls = 0;
            let outcome = execute_rename_plan(&plan, &[entry], |source, destination| {
                calls += 1;
                rename_without_overwrite(source, destination)?;
                if calls == 1 {
                    fs::write(&raced, b"unrelated race")?;
                }
                Ok(())
            });
            assert_eq!(outcome, RenameExecution::RolledBack);
            assert!(member.source.exists());
            assert_eq!(
                fs::read(&raced).expect("race must remain"),
                b"unrelated race"
            );
        }
    }

    #[test]
    fn stale_apply_does_not_consume_the_newer_current_plan() {
        let fixture = Fixture::new();
        let newer = NormalizationPlan {
            id: hex_sha1(b"newer-plan"),
            category: NormalizationCategory::Adult,
            folder: fixture.0.clone(),
            scan_generation: 2,
            entries: Vec::new(),
        };
        let state = FilenameNormalizationState::default();
        state.0.lock().expect("state must lock").plan = Some(newer.clone());
        assert_eq!(
            apply_with_current(
                &state,
                &VrDownloadState::default(),
                &fixture.0.join("recovery"),
                NormalizationCategory::Adult,
                &hex_sha1(b"stale-plan"),
                &[hex_sha1(b"stale-entry")],
                || true,
            ),
            Err(NORMALIZATION_STALE)
        );
        assert_eq!(
            state
                .0
                .lock()
                .expect("state must lock")
                .plan
                .as_ref()
                .map(|plan| plan.id.as_str()),
            Some(newer.id.as_str())
        );
    }

    #[test]
    fn committed_renames_remain_recoverable_until_cache_and_scan_reconcile() {
        let fixture = Fixture::new();
        let recovery = fixture.0.join("recovery");
        let file = normalization_file(&fixture.0, "DSVR-69.mp4", Some("DSVR-69"));
        let snapshot = NormalizationSnapshot {
            category: NormalizationCategory::Vr,
            folder: fixture.0.clone(),
            generation: 1,
            files: vec![file.clone()],
        };
        let member = proposed_members(&snapshot, &[file], "DSVR-069")
            .expect("proposal must resolve")
            .remove(0);
        let plan = NormalizationPlan {
            id: hex_sha1(b"committed-plan"),
            category: NormalizationCategory::Vr,
            folder: fixture.0.clone(),
            scan_generation: 1,
            entries: Vec::new(),
        };
        let entry = AuditEntry {
            id: hex_sha1(b"committed-entry"),
            status: "ready",
            local_code: Some("DSVR-69".to_owned()),
            proof: None,
            reason: "fixture".to_owned(),
            members: vec![member.clone()],
        };
        write_recovery(&recovery, &plan, &[entry]).expect("record must persist");
        rename_without_overwrite(&member.source, &member.destination)
            .expect("fixture rename must commit");

        assert_eq!(
            reconcile_committed_with(
                &recovery,
                plan.category,
                &plan.id,
                || Err::<(), ()>(()),
                || Ok::<_, ()>(vec!["scan"]),
            ),
            Err(NORMALIZATION_COMMITTED)
        );
        assert!(member.destination.exists());
        assert_eq!(
            recovery_status(&recovery)
                .expect("committed record must remain")
                .first()
                .map(String::as_str),
            Some("committed")
        );
        assert_eq!(
            committed_recovery(&recovery, plan.category, &plan.id),
            Ok(vec!["DSVR-69".to_owned()])
        );
        assert_eq!(
            reconcile_committed_with(
                &recovery,
                plan.category,
                &plan.id,
                || Ok::<_, ()>(()),
                || Err::<Vec<String>, ()>(()),
            ),
            Err(NORMALIZATION_COMMITTED)
        );
        assert!(recovery.exists());
        assert_eq!(
            reconcile_committed_with(
                &recovery,
                plan.category,
                &plan.id,
                || Ok::<_, ()>(()),
                || Ok::<_, ()>(vec!["scan"]),
            ),
            Ok(vec!["scan"])
        );
        assert!(!recovery.exists());
    }

    #[test]
    fn unavailable_adult_and_vr_volumes_preserve_verified_recovery_evidence() {
        for category in [NormalizationCategory::Adult, NormalizationCategory::Vr] {
            let fixture = Fixture::new();
            let library = fixture.0.join(category.as_str());
            fs::create_dir(&library).expect("library must exist");
            let recovery = fixture.0.join(format!("{}-recovery", category.as_str()));
            let file = normalization_file(&library, "DSVR-69.mp4", Some("DSVR-69"));
            let snapshot = NormalizationSnapshot {
                category,
                folder: fs::canonicalize(&library).expect("library must canonicalize"),
                generation: 1,
                files: vec![file.clone()],
            };
            let member = proposed_members(&snapshot, &[file], "DSVR-069")
                .expect("proposal must resolve")
                .remove(0);
            let plan = NormalizationPlan {
                id: hex_sha1(category.as_str().as_bytes()),
                category,
                folder: snapshot.folder,
                scan_generation: 1,
                entries: Vec::new(),
            };
            let entry = AuditEntry {
                id: hex_sha1(format!("{}-entry", category.as_str()).as_bytes()),
                status: "ready",
                local_code: Some("DSVR-69".to_owned()),
                proof: None,
                reason: "fixture".to_owned(),
                members: vec![member],
            };
            write_recovery(&recovery, &plan, &[entry]).expect("record must persist");
            let unavailable = fixture.0.join(format!("{}-offline", category.as_str()));
            fs::rename(&library, &unavailable).expect("volume must become unavailable");
            let status = recovery_status(&recovery).expect("safe evidence must remain readable");
            assert_eq!(status[0], "attention");
            assert_eq!(status[1], category.as_str());
            assert_eq!(status[4], "DSVR-69.mp4");
            assert_eq!(status[5], "DSVR-069.mp4");
            assert!(recovery.exists());
            fs::rename(&unavailable, &library).expect("volume must be restored");
            assert_eq!(recovery_status(&recovery), Ok(vec!["none".to_owned()]));
        }
    }

    #[test]
    fn committed_recovery_survives_restart_with_an_unavailable_then_restored_volume() {
        for category in [NormalizationCategory::Adult, NormalizationCategory::Vr] {
            let fixture = Fixture::new();
            let library = fixture.0.join(category.as_str());
            fs::create_dir(&library).expect("library must exist");
            let library = fs::canonicalize(library).expect("library must canonicalize");
            let recovery = fixture.0.join(format!("{}-committed", category.as_str()));
            let file = normalization_file(&library, "DSVR-69.mp4", Some("DSVR-69"));
            let snapshot = NormalizationSnapshot {
                category,
                folder: library.clone(),
                generation: 1,
                files: vec![file.clone()],
            };
            let member = proposed_members(&snapshot, &[file], "DSVR-069")
                .expect("proposal must resolve")
                .remove(0);
            let plan = NormalizationPlan {
                id: hex_sha1(format!("{}-committed-plan", category.as_str()).as_bytes()),
                category,
                folder: library.clone(),
                scan_generation: 1,
                entries: Vec::new(),
            };
            let entry = AuditEntry {
                id: hex_sha1(format!("{}-committed-entry", category.as_str()).as_bytes()),
                status: "ready",
                local_code: Some("DSVR-69".to_owned()),
                proof: None,
                reason: "fixture".to_owned(),
                members: vec![member.clone()],
            };
            write_recovery(&recovery, &plan, &[entry]).expect("record must persist");
            rename_without_overwrite(&member.source, &member.destination)
                .expect("rename must commit");
            let unavailable = fixture
                .0
                .join(format!("{}-committed-offline", category.as_str()));
            fs::rename(&library, &unavailable).expect("volume must become unavailable");
            assert_eq!(
                recovery_status(&recovery)
                    .expect("unavailable evidence must parse")
                    .first()
                    .map(String::as_str),
                Some("attention")
            );
            fs::rename(&unavailable, &library).expect("volume must be restored");
            assert_eq!(
                committed_recovery(&recovery, category, &plan.id),
                Ok(vec!["DSVR-69".to_owned()])
            );
            assert_eq!(
                reconcile_committed_with(
                    &recovery,
                    category,
                    &plan.id,
                    || Ok::<_, ()>(()),
                    || Ok::<_, ()>(vec!["scan"]),
                ),
                Ok(vec!["scan"])
            );
            assert!(!recovery.exists());
            assert!(member.destination.exists());
        }
    }

    #[test]
    fn oversized_recovery_is_rejected_before_creating_persistent_state() {
        let fixture = Fixture::new();
        let recovery = fixture.0.join("recovery");
        let member = RenameMember {
            source: fixture.0.join("source.mp4"),
            destination: fixture.0.join("destination.mp4"),
            source_relative: format!("{}.mp4", "s".repeat(4_000)),
            destination_relative: format!("{}.mp4", "d".repeat(4_000)),
            size: 1,
            modified: UNIX_EPOCH,
            fingerprint: "f".repeat(256),
        };
        let entry = AuditEntry {
            id: hex_sha1(b"oversized-entry"),
            status: "ready",
            local_code: None,
            proof: None,
            reason: "fixture".to_owned(),
            members: vec![member; 1_100],
        };
        let plan = NormalizationPlan {
            id: hex_sha1(b"oversized-plan"),
            category: NormalizationCategory::Adult,
            folder: fixture.0.clone(),
            scan_generation: 1,
            entries: Vec::new(),
        };
        assert_eq!(write_recovery(&recovery, &plan, &[entry]), Err(()));
        assert!(!recovery.exists());
    }

    #[test]
    fn stale_operation_rolls_back_before_the_next_mutation() {
        use std::cell::Cell;

        let fixture = Fixture::new();
        let file = normalization_file(&fixture.0, "DSVR-69.mp4", Some("DSVR-69"));
        let snapshot = NormalizationSnapshot {
            category: NormalizationCategory::Vr,
            folder: fixture.0.clone(),
            generation: 1,
            files: vec![file.clone()],
        };
        let member = proposed_members(&snapshot, std::slice::from_ref(&file), "DSVR-069")
            .expect("proposal must resolve")
            .remove(0);
        let plan = NormalizationPlan {
            id: hex_sha1(b"stale-plan"),
            category: snapshot.category,
            folder: snapshot.folder,
            scan_generation: 1,
            entries: Vec::new(),
        };
        let entry = AuditEntry {
            id: hex_sha1(b"stale-entry"),
            status: "ready",
            local_code: Some("DSVR-69".to_owned()),
            proof: None,
            reason: "fixture".to_owned(),
            members: vec![member.clone()],
        };
        let checks = Cell::new(0);
        assert_eq!(
            execute_rename_plan_while_current(
                &plan,
                &[entry],
                rename_without_overwrite,
                || {
                    let current = checks.get() == 0;
                    checks.set(checks.get() + 1);
                    current
                },
                || true
            ),
            RenameExecution::RolledBack
        );
        assert!(member.source.exists());
        assert!(!member.destination.exists());
    }

    #[test]
    fn ownership_claim_after_the_first_move_stops_later_mutation_with_exact_recovery() {
        use std::cell::Cell;

        let fixture = Fixture::new();
        let recovery = fixture.0.join("recovery");
        let first = normalization_file(&fixture.0, "DSVR-69 Part 1.mp4", Some("DSVR-69"));
        let second = normalization_file(&fixture.0, "DSVR-69 Part 2.mp4", Some("DSVR-69"));
        let snapshot = NormalizationSnapshot {
            category: NormalizationCategory::Vr,
            folder: fixture.0.clone(),
            generation: 1,
            files: vec![first.clone(), second.clone()],
        };
        let entry = AuditEntry {
            id: hex_sha1(b"ownership-race-entry"),
            status: "ready",
            local_code: Some("DSVR-69".to_owned()),
            proof: None,
            reason: "fixture".to_owned(),
            members: proposed_members(&snapshot, &[first.clone(), second.clone()], "DSVR-069")
                .expect("multipart proposal must resolve"),
        };
        let plan = NormalizationPlan {
            id: hex_sha1(b"ownership-race-plan"),
            category: snapshot.category,
            folder: snapshot.folder,
            scan_generation: 1,
            entries: Vec::new(),
        };
        write_recovery(&recovery, &plan, std::slice::from_ref(&entry))
            .expect("exact recovery must precede mutation");
        let ownership_checks = Cell::new(0);
        let outcome = execute_rename_plan_while_current(
            &plan,
            std::slice::from_ref(&entry),
            rename_without_overwrite,
            || true,
            || {
                let clear = ownership_checks.get() == 0;
                ownership_checks.set(ownership_checks.get() + 1);
                clear
            },
        );
        assert_eq!(outcome, RenameExecution::RecoveryRequired);
        assert!(
            !first.path.exists(),
            "the first move remains exactly staged"
        );
        assert!(
            second.path.exists(),
            "the newly owned later path is not mutated"
        );
        let status = recovery_status(&recovery).expect("exact recovery must remain readable");
        assert_eq!(status.first().map(String::as_str), Some("attention"));
        assert!(status.iter().any(|path| path.ends_with(".pending")));
    }

    #[test]
    fn failed_rollback_record_removal_requires_recovery_attention() {
        let fixture = Fixture::new();
        let recovery = fixture.0.join("recovery");
        fs::write(&recovery, b"durable evidence").expect("fixture record must exist");
        assert_eq!(
            rolled_back_error(&recovery, |_| Err(())),
            NORMALIZATION_RECOVERY
        );
        assert!(recovery.exists());
        assert_eq!(
            rolled_back_error(&recovery, remove_recovery),
            NORMALIZATION_FAILED
        );
        assert!(!recovery.exists());
    }

    #[test]
    fn exact_rolled_back_recovery_survives_restart_and_retries_cleanup_for_both_categories() {
        for (category, source_name, destination_name, local_code) in [
            (
                NormalizationCategory::Adult,
                "CAWB-1.mp4",
                "CAWB-001.mp4",
                "CAWB-1",
            ),
            (
                NormalizationCategory::Vr,
                "DSVR-69.mp4",
                "DSVR-069.mp4",
                "DSVR-69",
            ),
        ] {
            let fixture = Fixture::new();
            let recovery = fixture.0.join("recovery");
            let file = normalization_file(&fixture.0, source_name, Some(local_code));
            let plan = NormalizationPlan {
                id: hex_sha1(format!("cleanup-{local_code}").as_bytes()),
                category,
                folder: fixture.0.clone(),
                scan_generation: 1,
                entries: Vec::new(),
            };
            let entry = AuditEntry {
                id: hex_sha1(format!("cleanup-entry-{local_code}").as_bytes()),
                status: "ready",
                local_code: Some(local_code.to_owned()),
                proof: None,
                reason: "fixture".to_owned(),
                members: vec![RenameMember {
                    source: file.path.clone(),
                    destination: fixture.0.join(destination_name),
                    source_relative: source_name.to_owned(),
                    destination_relative: destination_name.to_owned(),
                    size: file.size,
                    modified: file.modified,
                    fingerprint: file_fingerprint(&file.path)
                        .expect("source fingerprint must exist"),
                }],
            };
            write_recovery(&recovery, &plan, &[entry]).expect("recovery must persist");

            for _ in 0..2 {
                assert_eq!(
                    recovery_status_with_source_retirement(&recovery, None, |_| Err(()))
                        .expect("exact rollback must remain readable"),
                    vec![
                        "cleanup-pending",
                        category.as_str(),
                        &plan.id,
                        "1",
                        source_name,
                        destination_name,
                    ]
                );
                assert!(recovery.exists(), "persistent failure keeps exact evidence");
                assert!(file.path.exists(), "cleanup retry never renames the source");
                assert!(!fixture.0.join(destination_name).exists());
            }

            assert_eq!(
                retire_rolled_back_recovery(&recovery, category, &hex_sha1(b"wrong-plan")),
                Err(NORMALIZATION_RECOVERY)
            );
            assert!(
                recovery.exists(),
                "wrong authority cannot retire the record"
            );
            retire_rolled_back_recovery(&recovery, category, &plan.id)
                .expect("a later exact cleanup retry must succeed");
            assert!(!recovery.exists());
            assert!(file.path.exists());
            assert!(!fixture.0.join(destination_name).exists());
        }
    }

    #[test]
    fn recovery_never_accepts_or_discards_an_unrelated_replacement() {
        let fixture = Fixture::new();
        let recovery = fixture.0.join("recovery");
        let file = normalization_file(&fixture.0, "DSVR-69.mp4", Some("DSVR-69"));
        let snapshot = NormalizationSnapshot {
            category: NormalizationCategory::Vr,
            folder: fixture.0.clone(),
            generation: 1,
            files: vec![file.clone()],
        };
        let member = proposed_members(&snapshot, std::slice::from_ref(&file), "DSVR-069")
            .expect("proposal must resolve")
            .remove(0);
        let plan = NormalizationPlan {
            id: hex_sha1(b"replacement-plan"),
            category: snapshot.category,
            folder: snapshot.folder,
            scan_generation: 1,
            entries: Vec::new(),
        };
        let entry = AuditEntry {
            id: hex_sha1(b"replacement-entry"),
            status: "ready",
            local_code: Some("DSVR-69".to_owned()),
            proof: None,
            reason: "fixture".to_owned(),
            members: vec![member.clone()],
        };
        write_recovery(&recovery, &plan, &[entry]).expect("record must persist");
        rename_without_overwrite(&member.source, &member.destination)
            .expect("exact member must move");
        fs::write(&member.source, b"unrelated replacement").expect("replacement must be created");
        let status = recovery_status(&recovery).expect("record must remain readable");
        assert_eq!(status.first().map(String::as_str), Some("attention"));
        assert!(recovery.exists());
        assert_eq!(
            fs::read(&member.source).expect("replacement must remain"),
            b"unrelated replacement"
        );
    }
}
