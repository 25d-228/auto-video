use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    time::SystemTime,
};

use crate::{
    library_presentation::{LibraryItemAuthority, LibraryPresentationCategory},
    library_scan::{is_supported_library_media, scan_library_files},
    vr_torrent::{hex_sha1, product_code_candidates},
};

pub const ADULT_FOLDER_STORAGE_FAILED: &str = "adult_folder_storage_failed";
pub const ADULT_FOLDER_UNAVAILABLE: &str = "adult_folder_unavailable";
pub const ADULT_LIBRARY_SCAN_FAILED: &str = "adult_library_scan_failed";
pub const ADULT_LIBRARY_STALE: &str = "adult_library_stale";
pub const ADULT_FILE_OPEN_FAILED: &str = "adult_file_open_failed";
pub const ADULT_FILE_OPEN_NOT_FILE: &str = "adult_file_open_not_file";
pub const ADULT_FILE_OPEN_NOT_FOUND: &str = "adult_file_open_not_found";
pub const ADULT_FILE_OPEN_OUTSIDE_FOLDER: &str = "adult_file_open_outside_folder";
pub const ADULT_FILE_OPEN_STALE: &str = "adult_file_open_stale";
pub const ADULT_FILE_OPEN_UNAVAILABLE: &str = "adult_file_open_unavailable";
pub const ADULT_FILE_OPEN_UNSUPPORTED: &str = "adult_file_open_unsupported";
pub const ADULT_FILE_REVEAL_FAILED: &str = "adult_file_reveal_failed";
pub const ADULT_FILE_REVEAL_NOT_FILE: &str = "adult_file_reveal_not_file";
pub const ADULT_FILE_REVEAL_NOT_FOUND: &str = "adult_file_reveal_not_found";
pub const ADULT_FILE_REVEAL_OUTSIDE_FOLDER: &str = "adult_file_reveal_outside_folder";
pub const ADULT_FILE_REVEAL_STALE: &str = "adult_file_reveal_stale";
pub const ADULT_FILE_REVEAL_UNAVAILABLE: &str = "adult_file_reveal_unavailable";
pub const ADULT_FILE_REVEAL_UNSUPPORTED: &str = "adult_file_reveal_unsupported";
pub const ADULT_FILE_TRASH_FAILED: &str = "adult_file_trash_failed";
pub const ADULT_FILE_TRASH_NOT_FILE: &str = "adult_file_trash_not_file";
pub const ADULT_FILE_TRASH_NOT_FOUND: &str = "adult_file_trash_not_found";
pub const ADULT_FILE_TRASH_OUTSIDE_FOLDER: &str = "adult_file_trash_outside_folder";
pub const ADULT_FILE_TRASH_STALE: &str = "adult_file_trash_stale";
pub const ADULT_FILE_TRASH_UNAVAILABLE: &str = "adult_file_trash_unavailable";
pub const ADULT_FILE_TRASH_UNSUPPORTED: &str = "adult_file_trash_unsupported";

#[derive(Clone)]
struct TrustedAdultFile {
    path: PathBuf,
    size: u64,
    modified: SystemTime,
}

struct CompletedAdultScan {
    folder: PathBuf,
    generation: u64,
    files: Vec<TrustedAdultFile>,
}

#[derive(Default)]
struct AdultLibraryContext {
    folder: Option<PathBuf>,
    generation: u64,
    completed_scan: Option<CompletedAdultScan>,
}

#[derive(Clone, Default)]
pub struct AdultLibraryState(Arc<Mutex<AdultLibraryContext>>);

const MULTIPART_IDENTITY_PREFIXES: &[&str] = &["PART", "CD", "DISC", "DISK"];

fn exact_file_product_code(path: &Path) -> Option<String> {
    let title = path.file_stem()?.to_str()?;
    let mut codes = product_code_candidates(title)
        .into_iter()
        .filter(|(_, prefix)| !MULTIPART_IDENTITY_PREFIXES.contains(&prefix.as_str()))
        .map(|(code, _)| code)
        .collect::<Vec<_>>();
    codes.sort();
    codes.dedup();
    (codes.len() == 1).then(|| codes.remove(0))
}

pub(crate) fn adult_library_presentation_authority(
    state: &AdultLibraryState,
    scan_generation: u64,
    code: &str,
) -> Result<LibraryItemAuthority, &'static str> {
    if exact_file_product_code(Path::new(code)).as_deref() != Some(code) {
        return Err(ADULT_LIBRARY_STALE);
    }
    let context = state.0.lock().map_err(|_| ADULT_LIBRARY_SCAN_FAILED)?;
    let scan = context.completed_scan.as_ref().ok_or(ADULT_LIBRARY_STALE)?;
    if scan.generation != scan_generation || context.folder.as_ref() != Some(&scan.folder) {
        return Err(ADULT_LIBRARY_STALE);
    }
    let mut members = scan
        .files
        .iter()
        .filter(|file| exact_file_product_code(&file.path).as_deref() == Some(code))
        .collect::<Vec<_>>();
    if members.is_empty() {
        return Err(ADULT_LIBRARY_STALE);
    }
    members.sort_by(|left, right| left.path.cmp(&right.path));
    let mut identity = format!("adult\0{}\0{code}", scan.folder.display());
    for member in members {
        let modified = member
            .modified
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(|_| ADULT_LIBRARY_STALE)?;
        identity.push_str(&format!(
            "\0{}\0{}\0{}\0{}",
            member.path.display(),
            member.size,
            modified.as_secs(),
            modified.subsec_nanos()
        ));
    }
    Ok(LibraryItemAuthority {
        category: LibraryPresentationCategory::Adult,
        identity: hex_sha1(identity.as_bytes()),
        code: code.to_owned(),
    })
}

#[derive(Clone, Copy)]
enum AdultFileValidationError {
    NotFound,
    Unavailable,
    NotFile,
    Unsupported,
    OutsideFolder,
    Stale,
    Dispatch,
}

fn save_adult_folder(path: &Path, folder: &Path) -> Result<(), &'static str> {
    let parent = path.parent().ok_or(ADULT_FOLDER_STORAGE_FAILED)?;
    fs::create_dir_all(parent).map_err(|_| ADULT_FOLDER_STORAGE_FAILED)?;
    let folder = folder.to_str().ok_or(ADULT_FOLDER_STORAGE_FAILED)?;

    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|_| ADULT_FOLDER_STORAGE_FAILED)?;
    file.write_all(folder.as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|_| ADULT_FOLDER_STORAGE_FAILED)
}

pub fn load_adult_folder_with(
    state: &AdultLibraryState,
    persistence_path: &Path,
) -> Result<Vec<String>, &'static str> {
    let stored_folder = match fs::read_to_string(persistence_path) {
        Ok(value) if !value.is_empty() => PathBuf::from(value),
        Ok(_) => return Err(ADULT_FOLDER_STORAGE_FAILED),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let mut context = state.0.lock().map_err(|_| ADULT_FOLDER_STORAGE_FAILED)?;
            context.folder = None;
            context.generation = context.generation.wrapping_add(1);
            context.completed_scan = None;
            return Ok(vec!["unconfigured".to_owned()]);
        }
        Err(_) => return Err(ADULT_FOLDER_STORAGE_FAILED),
    };
    let status = fs::canonicalize(&stored_folder)
        .ok()
        .filter(|canonical| canonical == &stored_folder)
        .and_then(|canonical| fs::metadata(canonical).ok())
        .filter(|metadata| metadata.is_dir())
        .map_or("unavailable", |_| "ready");
    let response_path = stored_folder
        .to_str()
        .map(str::to_owned)
        .ok_or(ADULT_FOLDER_STORAGE_FAILED)?;

    let mut context = state.0.lock().map_err(|_| ADULT_FOLDER_STORAGE_FAILED)?;
    context.folder = Some(stored_folder);
    context.generation = context.generation.wrapping_add(1);
    context.completed_scan = None;
    Ok(vec![status.to_owned(), response_path])
}

pub fn set_adult_folder(
    state: &AdultLibraryState,
    persistence_path: &Path,
    selected_folder: PathBuf,
) -> Result<String, &'static str> {
    let folder = fs::canonicalize(selected_folder).map_err(|_| ADULT_FOLDER_UNAVAILABLE)?;
    if !fs::metadata(&folder)
        .map_err(|_| ADULT_FOLDER_UNAVAILABLE)?
        .is_dir()
    {
        return Err(ADULT_FOLDER_UNAVAILABLE);
    }
    save_adult_folder(persistence_path, &folder)?;
    let response = folder
        .to_str()
        .map(str::to_owned)
        .ok_or(ADULT_FOLDER_UNAVAILABLE)?;

    let mut context = state.0.lock().map_err(|_| ADULT_FOLDER_STORAGE_FAILED)?;
    context.folder = Some(folder);
    context.generation = context.generation.wrapping_add(1);
    context.completed_scan = None;
    Ok(response)
}

pub fn clear_adult_folder(
    state: &AdultLibraryState,
    persistence_path: &Path,
) -> Result<(), &'static str> {
    match fs::remove_file(persistence_path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => return Err(ADULT_FOLDER_STORAGE_FAILED),
    }
    let mut context = state.0.lock().map_err(|_| ADULT_FOLDER_STORAGE_FAILED)?;
    context.folder = None;
    context.generation = context.generation.wrapping_add(1);
    context.completed_scan = None;
    Ok(())
}

pub fn configured_adult_folder(state: &AdultLibraryState) -> Result<Option<PathBuf>, &'static str> {
    state
        .0
        .lock()
        .map(|context| context.folder.clone())
        .map_err(|_| ADULT_FOLDER_STORAGE_FAILED)
}

fn scan_media_files(folder: &Path) -> Result<Vec<TrustedAdultFile>, &'static str> {
    let canonical_folder = fs::canonicalize(folder).map_err(|_| ADULT_FOLDER_UNAVAILABLE)?;
    if canonical_folder != folder
        || !fs::metadata(&canonical_folder)
            .map_err(|_| ADULT_FOLDER_UNAVAILABLE)?
            .is_dir()
    {
        return Err(ADULT_FOLDER_UNAVAILABLE);
    }
    scan_library_files(folder, |path, metadata| {
        Some(TrustedAdultFile {
            path,
            size: metadata.len(),
            modified: metadata.modified().ok()?,
        })
    })
    .map_err(|_| ADULT_LIBRARY_SCAN_FAILED)
}

pub fn scan_adult_library_with(state: &AdultLibraryState) -> Result<Vec<String>, &'static str> {
    let (folder, generation) = {
        let mut context = state.0.lock().map_err(|_| ADULT_LIBRARY_SCAN_FAILED)?;
        let folder = context.folder.clone().ok_or(ADULT_FOLDER_UNAVAILABLE)?;
        context.generation = context.generation.wrapping_add(1);
        context.completed_scan = None;
        (folder, context.generation)
    };
    let files = scan_media_files(&folder)?;

    let mut response = Vec::with_capacity(1 + files.len() * 3);
    response.push(generation.to_string());
    for file in &files {
        response.push(
            file.path
                .to_str()
                .map(str::to_owned)
                .ok_or(ADULT_LIBRARY_SCAN_FAILED)?,
        );
        response.push(
            file.path
                .strip_prefix(&folder)
                .ok()
                .and_then(Path::to_str)
                .map(str::to_owned)
                .filter(|path| !path.is_empty())
                .ok_or(ADULT_LIBRARY_SCAN_FAILED)?,
        );
        response.push(file.size.to_string());
    }

    let mut context = state.0.lock().map_err(|_| ADULT_LIBRARY_SCAN_FAILED)?;
    if context.generation != generation || context.folder.as_ref() != Some(&folder) {
        return Err(ADULT_LIBRARY_STALE);
    }
    context.completed_scan = Some(CompletedAdultScan {
        folder,
        generation,
        files,
    });
    Ok(response)
}

fn metadata_error(error: &io::Error) -> AdultFileValidationError {
    if error.kind() == io::ErrorKind::NotFound {
        AdultFileValidationError::NotFound
    } else {
        AdultFileValidationError::Unavailable
    }
}

fn validate_adult_file(
    requested_path: &Path,
    configured_folder: &Path,
    scan: &CompletedAdultScan,
    requested_generation: Option<u64>,
) -> Result<(), AdultFileValidationError> {
    if scan.folder != configured_folder
        || requested_generation.is_some_and(|generation| generation != scan.generation)
    {
        return Err(AdultFileValidationError::Stale);
    }
    let relative_path = requested_path
        .strip_prefix(configured_folder)
        .map_err(|_| AdultFileValidationError::OutsideFolder)?;
    let mut checked_path = configured_folder.to_path_buf();
    for component in relative_path.components() {
        let Component::Normal(component) = component else {
            return Err(AdultFileValidationError::OutsideFolder);
        };
        checked_path.push(component);
        let metadata =
            fs::symlink_metadata(&checked_path).map_err(|error| metadata_error(&error))?;
        if metadata.file_type().is_symlink() {
            return Err(AdultFileValidationError::NotFile);
        }
    }
    let metadata = fs::metadata(requested_path).map_err(|error| metadata_error(&error))?;
    if !metadata.is_file() {
        return Err(AdultFileValidationError::NotFile);
    }
    if !is_supported_library_media(requested_path) {
        return Err(AdultFileValidationError::Unsupported);
    }
    let canonical_path =
        fs::canonicalize(requested_path).map_err(|error| metadata_error(&error))?;
    if canonical_path != requested_path || !canonical_path.starts_with(configured_folder) {
        return Err(AdultFileValidationError::OutsideFolder);
    }
    let trusted_file = scan
        .files
        .iter()
        .find(|file| file.path == requested_path)
        .ok_or(AdultFileValidationError::Stale)?;
    if trusted_file.size != metadata.len()
        || metadata
            .modified()
            .map_err(|error| metadata_error(&error))?
            != trusted_file.modified
    {
        return Err(AdultFileValidationError::Stale);
    }
    Ok(())
}

fn run_adult_file_action(
    path: &Path,
    state: &AdultLibraryState,
    dispatch: impl FnOnce(&Path) -> Result<(), ()>,
) -> Result<(), AdultFileValidationError> {
    let context = state
        .0
        .lock()
        .map_err(|_| AdultFileValidationError::Unavailable)?;
    let configured_folder = context
        .folder
        .as_deref()
        .ok_or(AdultFileValidationError::Unavailable)?;
    let canonical_folder =
        fs::canonicalize(configured_folder).map_err(|_| AdultFileValidationError::Unavailable)?;
    if canonical_folder != configured_folder
        || !fs::metadata(&canonical_folder)
            .map_err(|_| AdultFileValidationError::Unavailable)?
            .is_dir()
    {
        return Err(AdultFileValidationError::Unavailable);
    }
    let scan = context
        .completed_scan
        .as_ref()
        .ok_or(AdultFileValidationError::Stale)?;
    validate_adult_file(path, &canonical_folder, scan, None)?;
    dispatch(path).map_err(|_| AdultFileValidationError::Dispatch)
}

pub fn open_adult_file_with(
    path: &Path,
    state: &AdultLibraryState,
    dispatch: impl FnOnce(&Path) -> Result<(), ()>,
) -> Result<(), &'static str> {
    run_adult_file_action(path, state, dispatch).map_err(|error| match error {
        AdultFileValidationError::NotFound => ADULT_FILE_OPEN_NOT_FOUND,
        AdultFileValidationError::Unavailable => ADULT_FILE_OPEN_UNAVAILABLE,
        AdultFileValidationError::NotFile => ADULT_FILE_OPEN_NOT_FILE,
        AdultFileValidationError::Unsupported => ADULT_FILE_OPEN_UNSUPPORTED,
        AdultFileValidationError::OutsideFolder => ADULT_FILE_OPEN_OUTSIDE_FOLDER,
        AdultFileValidationError::Stale => ADULT_FILE_OPEN_STALE,
        AdultFileValidationError::Dispatch => ADULT_FILE_OPEN_FAILED,
    })
}

pub fn reveal_adult_file_with(
    path: &Path,
    state: &AdultLibraryState,
    dispatch: impl FnOnce(&Path) -> Result<(), ()>,
) -> Result<(), &'static str> {
    run_adult_file_action(path, state, dispatch).map_err(|error| match error {
        AdultFileValidationError::NotFound => ADULT_FILE_REVEAL_NOT_FOUND,
        AdultFileValidationError::Unavailable => ADULT_FILE_REVEAL_UNAVAILABLE,
        AdultFileValidationError::NotFile => ADULT_FILE_REVEAL_NOT_FILE,
        AdultFileValidationError::Unsupported => ADULT_FILE_REVEAL_UNSUPPORTED,
        AdultFileValidationError::OutsideFolder => ADULT_FILE_REVEAL_OUTSIDE_FOLDER,
        AdultFileValidationError::Stale => ADULT_FILE_REVEAL_STALE,
        AdultFileValidationError::Dispatch => ADULT_FILE_REVEAL_FAILED,
    })
}

pub fn trash_adult_file_with(
    path: &Path,
    scan_generation: u64,
    state: &AdultLibraryState,
    dispatch: impl FnOnce(&Path) -> Result<(), ()>,
) -> Result<(), &'static str> {
    let mut context = state.0.lock().map_err(|_| ADULT_FILE_TRASH_UNAVAILABLE)?;
    let configured_folder = context.folder.clone().ok_or(ADULT_FILE_TRASH_UNAVAILABLE)?;
    let canonical_folder =
        fs::canonicalize(&configured_folder).map_err(|_| ADULT_FILE_TRASH_UNAVAILABLE)?;
    if canonical_folder != configured_folder
        || !fs::metadata(&canonical_folder)
            .map_err(|_| ADULT_FILE_TRASH_UNAVAILABLE)?
            .is_dir()
    {
        return Err(ADULT_FILE_TRASH_UNAVAILABLE);
    }
    let scan = context
        .completed_scan
        .as_ref()
        .ok_or(ADULT_FILE_TRASH_STALE)?;
    validate_adult_file(path, &canonical_folder, scan, Some(scan_generation)).map_err(|error| {
        match error {
            AdultFileValidationError::NotFound => ADULT_FILE_TRASH_NOT_FOUND,
            AdultFileValidationError::Unavailable => ADULT_FILE_TRASH_UNAVAILABLE,
            AdultFileValidationError::NotFile => ADULT_FILE_TRASH_NOT_FILE,
            AdultFileValidationError::Unsupported => ADULT_FILE_TRASH_UNSUPPORTED,
            AdultFileValidationError::OutsideFolder => ADULT_FILE_TRASH_OUTSIDE_FOLDER,
            AdultFileValidationError::Stale => ADULT_FILE_TRASH_STALE,
            AdultFileValidationError::Dispatch => ADULT_FILE_TRASH_FAILED,
        }
    })?;

    dispatch(path).map_err(|_| ADULT_FILE_TRASH_FAILED)?;
    context
        .completed_scan
        .as_mut()
        .ok_or(ADULT_FILE_TRASH_STALE)?
        .files
        .retain(|file| file.path != path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::Cell,
        sync::atomic::{AtomicU64, Ordering},
    };

    static FIXTURE_ID: AtomicU64 = AtomicU64::new(1);

    struct Fixture {
        path: PathBuf,
    }

    impl Fixture {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "auto-video-adult-library-{label}-{}-{}",
                std::process::id(),
                FIXTURE_ID.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).expect("fixture folder must be created");
            Self {
                path: fs::canonicalize(path).expect("fixture folder must be canonical"),
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn presentation_authority_requires_one_exact_current_group_and_member_identity() {
        let fixture = Fixture::new("presentation-authority");
        for name in [
            "ADLT-123 Part 01.mp4",
            "adlt_00123 CD2.MKV",
            "ADLT-124.mp4",
            "ADLT-123 + XYZ-7.mp4",
            "unassociated.mp4",
        ] {
            fs::write(fixture.path.join(name), name.as_bytes())
                .expect("Adult member must be written");
        }
        let state = AdultLibraryState::default();
        set_adult_folder(&state, &fixture.path.join("config"), fixture.path.clone())
            .expect("Adult folder must be configured");
        let scan = scan_adult_library_with(&state).expect("scan must complete");
        let generation = scan[0].parse().expect("generation must be valid");

        let authority = adult_library_presentation_authority(&state, generation, "ADLT-123")
            .expect("one exact grouped code must authorize presentation");
        assert_eq!(authority.category, LibraryPresentationCategory::Adult);
        assert_eq!(authority.code, "ADLT-123");
        assert_eq!(authority.identity.len(), 40);
        assert_eq!(
            adult_library_presentation_authority(&state, generation, "ADLT-125"),
            Err(ADULT_LIBRARY_STALE)
        );
        assert_eq!(
            adult_library_presentation_authority(&state, generation, "ADLT-123 + XYZ-7"),
            Err(ADULT_LIBRARY_STALE)
        );

        fs::write(fixture.path.join("ADLT-123 Part 01.mp4"), b"changed")
            .expect("member must change");
        let refreshed = scan_adult_library_with(&state).expect("refresh must complete");
        let refreshed_generation = refreshed[0].parse().expect("generation must be valid");
        assert_eq!(
            adult_library_presentation_authority(&state, generation, "ADLT-123"),
            Err(ADULT_LIBRARY_STALE)
        );
        assert_ne!(
            adult_library_presentation_authority(&state, refreshed_generation, "ADLT-123")
                .expect("refreshed authority must resolve")
                .identity,
            authority.identity
        );
    }

    #[test]
    fn folder_configuration_persists_unavailable_state_recovers_and_clears() {
        let fixture = Fixture::new("folder");
        let folder = fixture.path.join("Adult titles");
        let persistence_path = fixture.path.join("config");
        fs::create_dir(&folder).expect("Adult folder must be created");
        let state = AdultLibraryState::default();

        assert_eq!(
            set_adult_folder(&state, &persistence_path, folder.clone()),
            Ok(folder.to_string_lossy().into_owned())
        );
        assert_eq!(
            load_adult_folder_with(&AdultLibraryState::default(), &persistence_path),
            Ok(vec![
                "ready".to_owned(),
                folder.to_string_lossy().into_owned()
            ])
        );
        fs::remove_dir(&folder).expect("Adult folder must be removed");
        assert_eq!(
            load_adult_folder_with(&state, &persistence_path),
            Ok(vec![
                "unavailable".to_owned(),
                folder.to_string_lossy().into_owned()
            ])
        );
        fs::create_dir(&folder).expect("Adult folder must be restored");
        assert_eq!(
            load_adult_folder_with(&state, &persistence_path),
            Ok(vec![
                "ready".to_owned(),
                folder.to_string_lossy().into_owned()
            ])
        );
        clear_adult_folder(&state, &persistence_path).expect("configuration must clear");
        assert_eq!(
            load_adult_folder_with(&state, &persistence_path),
            Ok(vec!["unconfigured".to_owned()])
        );
    }

    #[test]
    fn scan_preserves_exact_paths_relative_paths_sizes_and_order() {
        let fixture = Fixture::new("scan");
        let nested = fixture.path.join("作品  Name");
        fs::create_dir(&nested).expect("nested folder must be created");
        let first = fixture.path.join("ADLT-123  Part 01.mp4");
        let second = nested.join("adlt_00123_CD2.MKV");
        fs::write(&second, b"second").expect("second file must be written");
        fs::write(&first, b"one").expect("first file must be written");
        fs::write(fixture.path.join("ignored.txt"), b"ignored")
            .expect("ignored file must be written");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&first, fixture.path.join("ignored-link.mp4"))
            .expect("fixture symlink must be created");
        let state = AdultLibraryState::default();
        set_adult_folder(&state, &fixture.path.join("config"), fixture.path.clone())
            .expect("Adult folder must be configured");

        assert_eq!(
            scan_adult_library_with(&state),
            Ok(vec![
                "2".to_owned(),
                first.to_string_lossy().into_owned(),
                "ADLT-123  Part 01.mp4".to_owned(),
                "3".to_owned(),
                second.to_string_lossy().into_owned(),
                Path::new("作品  Name")
                    .join("adlt_00123_CD2.MKV")
                    .to_string_lossy()
                    .into_owned(),
                "6".to_owned(),
            ])
        );
    }

    #[test]
    fn file_action_rejects_unrelated_changed_and_unscanned_files_without_dispatch() {
        let trusted = Fixture::new("trusted");
        let unrelated = Fixture::new("unrelated");
        let trusted_file = trusted.path.join("ADLT-123.mp4");
        let unrelated_file = unrelated.path.join("ADLT-123.mp4");
        let unscanned_file = trusted.path.join("ADLT-124.mkv");
        fs::write(&trusted_file, b"trusted").expect("trusted file must be written");
        fs::write(&unrelated_file, b"unrelated").expect("unrelated file must be written");
        let state = AdultLibraryState::default();
        set_adult_folder(&state, &trusted.path.join("config"), trusted.path.clone())
            .expect("Adult folder must be configured");
        scan_adult_library_with(&state).expect("scan must complete");
        let dispatched = Cell::new(false);

        assert_eq!(
            open_adult_file_with(&unrelated_file, &state, |_| {
                dispatched.set(true);
                Ok(())
            }),
            Err(ADULT_FILE_OPEN_OUTSIDE_FOLDER)
        );
        fs::write(&unscanned_file, b"new").expect("unscanned file must be written");
        assert_eq!(
            open_adult_file_with(&unscanned_file, &state, |_| {
                dispatched.set(true);
                Ok(())
            }),
            Err(ADULT_FILE_OPEN_STALE)
        );
        fs::write(&trusted_file, b"changed content").expect("trusted file must change");
        assert_eq!(
            reveal_adult_file_with(&trusted_file, &state, |_| {
                dispatched.set(true);
                Ok(())
            }),
            Err(ADULT_FILE_REVEAL_STALE)
        );
        assert!(!dispatched.get());
    }

    #[test]
    fn file_action_rejects_missing_directory_unsupported_and_symlink_paths() {
        let fixture = Fixture::new("invalid");
        let movie = fixture.path.join("ADLT-123.mp4");
        fs::write(&movie, b"movie").expect("movie must be written");
        let state = AdultLibraryState::default();
        set_adult_folder(&state, &fixture.path.join("config"), fixture.path.clone())
            .expect("Adult folder must be configured");
        scan_adult_library_with(&state).expect("scan must complete");
        let directory = fixture.path.join("directory.mkv");
        let unsupported = fixture.path.join("unsupported.txt");
        fs::create_dir(&directory).expect("directory must be created");
        fs::write(&unsupported, b"unsupported").expect("unsupported file must be written");
        let dispatched = Cell::new(false);

        for (path, error) in [
            (fixture.path.join("missing.mp4"), ADULT_FILE_OPEN_NOT_FOUND),
            (directory, ADULT_FILE_OPEN_NOT_FILE),
            (unsupported, ADULT_FILE_OPEN_UNSUPPORTED),
        ] {
            assert_eq!(
                open_adult_file_with(&path, &state, |_| {
                    dispatched.set(true);
                    Ok(())
                }),
                Err(error)
            );
        }
        #[cfg(unix)]
        {
            let link = fixture.path.join("link.mp4");
            std::os::unix::fs::symlink(&movie, &link).expect("symlink must be created");
            assert_eq!(
                open_adult_file_with(&link, &state, |_| {
                    dispatched.set(true);
                    Ok(())
                }),
                Err(ADULT_FILE_OPEN_NOT_FILE)
            );
        }
        assert!(!dispatched.get());
    }

    #[test]
    fn file_actions_dispatch_only_the_exact_trusted_file_and_report_failures() {
        let fixture = Fixture::new("dispatch");
        let movie = fixture.path.join("ADLT-123.AVI");
        fs::write(&movie, b"movie").expect("movie must be written");
        let state = AdultLibraryState::default();
        set_adult_folder(&state, &fixture.path.join("config"), fixture.path.clone())
            .expect("Adult folder must be configured");
        scan_adult_library_with(&state).expect("scan must complete");
        let opened = Cell::new(false);

        assert_eq!(
            open_adult_file_with(&movie, &state, |path| {
                assert_eq!(path, movie);
                opened.set(true);
                Ok(())
            }),
            Ok(())
        );
        assert!(opened.get());
        let revealed = Cell::new(false);
        assert_eq!(
            reveal_adult_file_with(&movie, &state, |path| {
                assert_eq!(path, movie);
                revealed.set(true);
                Ok(())
            }),
            Ok(())
        );
        assert!(revealed.get());
        assert_eq!(
            open_adult_file_with(&movie, &state, |_| Err(())),
            Err(ADULT_FILE_OPEN_FAILED)
        );
        assert_eq!(
            reveal_adult_file_with(&movie, &state, |_| Err(())),
            Err(ADULT_FILE_REVEAL_FAILED)
        );
    }

    #[test]
    fn trash_dispatches_one_exact_scanned_adult_member_and_updates_state_only_after_success() {
        let fixture = Fixture::new("trash-exact");
        let first = fixture.path.join("ADLT-123 Part 01.WMV");
        let sibling = fixture.path.join("ADLT-123 CD2.mkv");
        let ambiguous = fixture.path.join("ADLT-123 Part 1-2.mp4");
        let unassociated = fixture.path.join("作品 without code.mp4");
        for (path, content) in [
            (&first, b"first".as_slice()),
            (&sibling, b"sibling".as_slice()),
            (&ambiguous, b"ambiguous".as_slice()),
            (&unassociated, b"unassociated".as_slice()),
        ] {
            fs::write(path, content).expect("Adult file must be written");
        }
        let state = AdultLibraryState::default();
        set_adult_folder(&state, &fixture.path.join("config"), fixture.path.clone())
            .expect("Adult folder must be configured");
        let scan = scan_adult_library_with(&state).expect("scan must complete");
        let generation = scan[0].parse().expect("generation must be valid");
        let dispatch_count = Cell::new(0);

        assert_eq!(
            trash_adult_file_with(&first, generation, &state, |_| {
                dispatch_count.set(dispatch_count.get() + 1);
                Err(())
            }),
            Err(ADULT_FILE_TRASH_FAILED)
        );
        assert_eq!(
            trash_adult_file_with(&first, generation, &state, |path| {
                assert_eq!(path, first);
                dispatch_count.set(dispatch_count.get() + 1);
                Ok(())
            }),
            Ok(())
        );
        assert_eq!(
            trash_adult_file_with(&first, generation, &state, |_| {
                dispatch_count.set(dispatch_count.get() + 1);
                Ok(())
            }),
            Err(ADULT_FILE_TRASH_STALE)
        );
        for path in [&sibling, &ambiguous, &unassociated] {
            assert_eq!(
                trash_adult_file_with(path, generation, &state, |_| Ok(())),
                Ok(())
            );
        }
        assert_eq!(dispatch_count.get(), 2);
    }

    #[test]
    fn trash_rejects_untrusted_changed_and_unsafe_adult_paths_without_dispatch() {
        let trusted = Fixture::new("trash-trusted");
        let unrelated = Fixture::new("trash-unrelated");
        let current = trusted.path.join("ADLT-123 Part 01.mp4");
        let changed = trusted.path.join("ADLT-123 CD2.mkv");
        let missing = trusted.path.join("ADLT-124.mp4");
        fs::write(&current, b"current").expect("current member must be written");
        fs::write(&changed, b"changed").expect("changed member must be written");
        fs::write(&missing, b"missing").expect("missing member must be written");
        let state = AdultLibraryState::default();
        set_adult_folder(&state, &trusted.path.join("config"), trusted.path.clone())
            .expect("Adult folder must be configured");
        let scan = scan_adult_library_with(&state).expect("scan must complete");
        let generation = scan[0].parse().expect("generation must be valid");
        let dispatched = Cell::new(false);
        let same_name_elsewhere = unrelated.path.join("ADLT-123 Part 01.mp4");
        fs::write(&same_name_elsewhere, b"current").expect("unrelated file must be written");
        let neighboring_code = trusted.path.join("ADLT-125.mp4");
        fs::write(&neighboring_code, b"neighbor").expect("neighbor must be written");
        let mixed_code = trusted.path.join("ADLT-123 + XYZ-7.mp4");
        fs::write(&mixed_code, b"mixed").expect("mixed-code file must be written");
        let directory = trusted.path.join("directory.mkv");
        fs::create_dir(&directory).expect("directory must be created");
        let unsupported = trusted.path.join("unsupported.txt");
        fs::write(&unsupported, b"unsupported").expect("unsupported file must be written");
        fs::write(&changed, b"different content").expect("member must change");
        fs::remove_file(&missing).expect("member must be removed");

        for (path, expected) in [
            (same_name_elsewhere, ADULT_FILE_TRASH_OUTSIDE_FOLDER),
            (neighboring_code, ADULT_FILE_TRASH_STALE),
            (mixed_code, ADULT_FILE_TRASH_STALE),
            (directory, ADULT_FILE_TRASH_NOT_FILE),
            (unsupported, ADULT_FILE_TRASH_UNSUPPORTED),
            (changed, ADULT_FILE_TRASH_STALE),
            (missing, ADULT_FILE_TRASH_NOT_FOUND),
        ] {
            assert_eq!(
                trash_adult_file_with(&path, generation, &state, |_| {
                    dispatched.set(true);
                    Ok(())
                }),
                Err(expected)
            );
        }

        #[cfg(unix)]
        {
            let link = trusted.path.join("linked.mp4");
            std::os::unix::fs::symlink(&current, &link).expect("file symlink must be created");
            assert_eq!(
                trash_adult_file_with(&link, generation, &state, |_| {
                    dispatched.set(true);
                    Ok(())
                }),
                Err(ADULT_FILE_TRASH_NOT_FILE)
            );
            let linked_parent = trusted.path.join("linked-parent");
            std::os::unix::fs::symlink(&unrelated.path, &linked_parent)
                .expect("parent symlink must be created");
            let linked_child = linked_parent.join("ADLT-123 Part 01.mp4");
            assert_eq!(
                trash_adult_file_with(&linked_child, generation, &state, |_| {
                    dispatched.set(true);
                    Ok(())
                }),
                Err(ADULT_FILE_TRASH_NOT_FILE)
            );
        }
        assert!(!dispatched.get());
    }

    #[test]
    fn trash_rejects_stale_generations_and_restart_scan_keeps_an_accepted_member_absent() {
        let fixture = Fixture::new("trash-generation");
        let holding = Fixture::new("trash-holding");
        let persistence_path = fixture.path.join("config");
        let member = fixture.path.join("ADLT-123 Disk-4.mp4");
        fs::write(&member, b"member").expect("Adult member must be written");
        let state = AdultLibraryState::default();
        set_adult_folder(&state, &persistence_path, fixture.path.clone())
            .expect("Adult folder must be configured");
        let first_scan = scan_adult_library_with(&state).expect("scan must complete");
        let first_generation = first_scan[0].parse().expect("generation must be valid");
        let second_scan = scan_adult_library_with(&state).expect("scan must complete");
        let current_generation = second_scan[0].parse().expect("generation must be valid");
        let dispatched = Cell::new(false);

        assert_eq!(
            trash_adult_file_with(&member, first_generation, &state, |_| {
                dispatched.set(true);
                Ok(())
            }),
            Err(ADULT_FILE_TRASH_STALE)
        );
        assert!(!dispatched.get());

        let moved_path = holding.path.join("ADLT-123 Disk-4.mp4");
        trash_adult_file_with(&member, current_generation, &state, |path| {
            fs::rename(path, &moved_path).map_err(|_| ())
        })
        .expect("accepted dispatch must succeed");
        assert!(moved_path.is_file());
        assert!(!member.exists());

        let restarted = AdultLibraryState::default();
        assert_eq!(
            load_adult_folder_with(&restarted, &persistence_path),
            Ok(vec![
                "ready".to_owned(),
                fixture.path.to_string_lossy().into_owned(),
            ])
        );
        let restarted_scan =
            scan_adult_library_with(&restarted).expect("restart scan must complete");
        assert_eq!(restarted_scan.len(), 1);
    }

    #[test]
    fn folder_replacement_clear_and_failed_refresh_invalidate_adult_trash_requests() {
        let configuration = Fixture::new("trash-configuration");
        let first = Fixture::new("trash-first-folder");
        let replacement = Fixture::new("trash-replacement-folder");
        let persistence_path = configuration.path.join("config");
        let member = first.path.join("ADLT-123 Disc 03.mp4");
        fs::write(&member, b"member").expect("Adult member must be written");
        let state = AdultLibraryState::default();
        set_adult_folder(&state, &persistence_path, first.path.clone())
            .expect("first Adult folder must be configured");
        let scan = scan_adult_library_with(&state).expect("scan must complete");
        let generation = scan[0].parse().expect("generation must be valid");
        let dispatched = Cell::new(false);

        set_adult_folder(&state, &persistence_path, replacement.path.clone())
            .expect("replacement Adult folder must be configured");
        assert_eq!(
            trash_adult_file_with(&member, generation, &state, |_| {
                dispatched.set(true);
                Ok(())
            }),
            Err(ADULT_FILE_TRASH_STALE)
        );
        clear_adult_folder(&state, &persistence_path).expect("Adult folder must clear");
        assert_eq!(
            trash_adult_file_with(&member, generation, &state, |_| {
                dispatched.set(true);
                Ok(())
            }),
            Err(ADULT_FILE_TRASH_UNAVAILABLE)
        );

        set_adult_folder(&state, &persistence_path, first.path.clone())
            .expect("first Adult folder must be restored");
        let refreshed_scan = scan_adult_library_with(&state).expect("scan must complete");
        let refreshed_generation = refreshed_scan[0].parse().expect("generation must be valid");
        let unavailable_folder = configuration.path.join("unavailable-Adult-folder");
        fs::rename(&first.path, &unavailable_folder).expect("Adult folder must be unavailable");
        assert_eq!(
            scan_adult_library_with(&state),
            Err(ADULT_FOLDER_UNAVAILABLE)
        );
        assert_eq!(
            trash_adult_file_with(&member, refreshed_generation, &state, |_| {
                dispatched.set(true);
                Ok(())
            }),
            Err(ADULT_FILE_TRASH_UNAVAILABLE)
        );
        assert!(!dispatched.get());
    }
}
