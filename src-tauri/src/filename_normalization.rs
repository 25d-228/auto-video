use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, OpenOptions},
    io::Write,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    time::SystemTime,
};

use unicode_normalization::UnicodeNormalization;

use crate::{
    fanza_catalog, javdb_catalog,
    library_scan::is_supported_library_media,
    vr_download::{
        with_unowned_adult_library_path, with_unowned_vr_library_path, VrDownloadState,
        VrLibraryTrashOwnershipError,
    },
    vr_torrent::{hex_sha1, product_code_forms, JsonParser, JsonValue},
    ProviderRequestError,
};

pub(crate) const NORMALIZATION_FAILED: &str = "filename_normalization_failed";
pub(crate) const NORMALIZATION_STALE: &str = "filename_normalization_stale";
pub(crate) const NORMALIZATION_RECOVERY: &str = "filename_normalization_recovery";

const RESPONSE_VERSION: &str = "filename-normalization-v1";
const RECOVERY_VERSION: &str = "AUTO_VIDEO_FILENAME_NORMALIZATION_V1";
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
}

#[derive(Clone, Default)]
pub(crate) struct FilenameNormalizationState(Arc<Mutex<NormalizationContext>>);

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
            let number = title[number_start..cursor].parse::<u64>().ok()?;
            matches.push((title[index..cursor].to_owned(), number));
        }
    }
    let identities = matches
        .iter()
        .map(|(_, number)| *number)
        .collect::<HashSet<_>>();
    (identities.len() == 1).then(|| matches[0].0.clone())
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
        });
    }
    Ok(members)
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
                    .map(|file| RenameMember {
                        source: file.path.clone(),
                        destination: file.path,
                        source_relative: file.relative_path,
                        destination_relative: String::new(),
                        size: file.size,
                        modified: file.modified,
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
            lines.push(format!(
                "{}\t{}\t{}",
                hex_encode(&member.source_relative),
                hex_encode(&member.destination_relative),
                hex_encode(&staging_relative)
            ));
        }
    }
    lines.join("\n").into_bytes()
}

fn write_recovery(path: &Path, plan: &NormalizationPlan, entries: &[AuditEntry]) -> Result<(), ()> {
    let parent = path.parent().ok_or(())?;
    fs::create_dir_all(parent).map_err(|_| ())?;
    if fs::symlink_metadata(path).is_ok() {
        return Err(());
    }
    let temporary = path.with_extension("next");
    match fs::symlink_metadata(&temporary) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            fs::remove_file(&temporary).map_err(|_| ())?;
        }
        Ok(_) => return Err(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(()),
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|_| ())?;
    file.write_all(&recovery_bytes(plan, entries))
        .map_err(|_| ())?;
    file.sync_all().map_err(|_| ())?;
    fs::rename(&temporary, path).map_err(|_| ())
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
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(()),
    }
}

pub(crate) fn recovery_status(path: &Path) -> Result<Vec<String>, &'static str> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(vec!["none".to_owned()]);
        }
        Err(_) => return Err(NORMALIZATION_RECOVERY),
    };
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
    let mut paths = Vec::new();
    let mut records = 0usize;
    let mut every_source = true;
    let mut every_destination = true;
    for line in lines {
        let mut fields = line.split('\t');
        let source = fields.next().ok_or(NORMALIZATION_RECOVERY)?;
        let destination = fields.next().ok_or(NORMALIZATION_RECOVERY)?;
        let staging = fields.next().ok_or(NORMALIZATION_RECOVERY)?;
        if fields.next().is_some() {
            return Err(NORMALIZATION_RECOVERY);
        }
        let source = hex_decode(source)
            .filter(|value| safe_relative(value))
            .ok_or(NORMALIZATION_RECOVERY)?;
        let destination = hex_decode(destination)
            .filter(|value| safe_relative(value))
            .ok_or(NORMALIZATION_RECOVERY)?;
        let staging = hex_decode(staging)
            .filter(|value| safe_relative(value))
            .ok_or(NORMALIZATION_RECOVERY)?;
        let current = [&source, &staging, &destination]
            .into_iter()
            .filter(|relative| fs::symlink_metadata(folder.join(relative)).is_ok())
            .cloned()
            .collect::<Vec<_>>();
        records += 1;
        every_source &= current.len() == 1 && current[0] == source;
        every_destination &= current.len() == 1 && current[0] == destination;
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
    if every_source || every_destination {
        remove_recovery(path).map_err(|_| NORMALIZATION_RECOVERY)?;
        return Ok(vec!["none".to_owned()]);
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RenameExecution {
    Complete,
    RolledBack,
    RecoveryRequired,
}

fn execute_rename_plan(
    plan: &NormalizationPlan,
    entries: &[AuditEntry],
    mut rename: impl FnMut(&Path, &Path) -> std::io::Result<()>,
) -> RenameExecution {
    let mut staged = Vec::<(PathBuf, RenameMember)>::new();
    let mut completed = Vec::<RenameMember>::new();
    let operation = (|| -> Result<(), ()> {
        for entry in entries {
            for member in &entry.members {
                if !exact_current_member(member, &plan.folder) {
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
        for (temporary, member) in &staged {
            let metadata = fs::symlink_metadata(temporary).map_err(|_| ())?;
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.len() != member.size
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
        if rename(&member.destination, &member.source).is_err() {
            rollback_complete = false;
        }
    }
    for (temporary, member) in staged.iter().rev() {
        if temporary.exists() && rename(temporary, &member.source).is_err() {
            rollback_complete = false;
        }
    }
    if rollback_complete {
        RenameExecution::RolledBack
    } else {
        RenameExecution::RecoveryRequired
    }
}

pub(crate) fn apply(
    state: &FilenameNormalizationState,
    download_state: &VrDownloadState,
    recovery_path: &Path,
    category: NormalizationCategory,
    plan_id: &str,
    selected_ids: &[String],
) -> Result<(NormalizationCategory, u64, Vec<String>), &'static str> {
    if selected_ids.is_empty() || selected_ids.len() > MAX_AUDIT_ITEMS {
        return Err(NORMALIZATION_STALE);
    }
    let plan = {
        let context = state.0.lock().map_err(|_| NORMALIZATION_FAILED)?;
        context.plan.clone().ok_or(NORMALIZATION_STALE)?
    };
    if plan.id != plan_id || plan.category != category {
        return Err(NORMALIZATION_STALE);
    }
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
        }
    }
    write_recovery(recovery_path, &plan, &entries).map_err(|_| NORMALIZATION_RECOVERY)?;
    match execute_rename_plan(&plan, &entries, |source, destination| {
        fs::rename(source, destination)
    }) {
        RenameExecution::Complete => {}
        RenameExecution::RolledBack => {
            let _ = remove_recovery(recovery_path);
            return Err(NORMALIZATION_FAILED);
        }
        RenameExecution::RecoveryRequired => return Err(NORMALIZATION_RECOVERY),
    }
    remove_recovery(recovery_path).map_err(|_| NORMALIZATION_RECOVERY)?;
    let affected_codes = entries
        .iter()
        .filter_map(|entry| entry.local_code.clone())
        .collect::<Vec<_>>();
    dismiss(state)?;
    Ok((category, plan.scan_generation, affected_codes))
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
        assert_eq!(
            fanza_transport_id(NormalizationCategory::Adult, "DSVR-69"),
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
            },
            &fixture.0
        ));
    }
}
