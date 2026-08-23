use std::{
    fs, io,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    time::SystemTime,
};

use crate::vr_download::{
    configured_vr_folder, with_configured_vr_folder, with_unowned_vr_library_path, VrDownloadState,
    VrLibraryTrashOwnershipError,
};
use crate::{
    library_presentation::{LibraryItemAuthority, LibraryPresentationCategory},
    library_scan::{is_supported_library_media, scan_library_files},
    vr_torrent::{hex_sha1, product_code_candidates, vr_library_product_code_prefix_is_supported},
};

pub const VR_LIBRARY_FOLDER_UNAVAILABLE: &str = "vr_library_folder_unavailable";
pub const VR_LIBRARY_SCAN_FAILED: &str = "vr_library_scan_failed";
pub const VR_LIBRARY_STALE: &str = "vr_library_stale";
pub const VR_FILE_OPEN_FAILED: &str = "vr_file_open_failed";
pub const VR_FILE_OPEN_NOT_FILE: &str = "vr_file_open_not_file";
pub const VR_FILE_OPEN_NOT_FOUND: &str = "vr_file_open_not_found";
pub const VR_FILE_OPEN_OUTSIDE_FOLDER: &str = "vr_file_open_outside_folder";
pub const VR_FILE_OPEN_STALE: &str = "vr_file_open_stale";
pub const VR_FILE_OPEN_UNAVAILABLE: &str = "vr_file_open_unavailable";
pub const VR_FILE_OPEN_UNSUPPORTED: &str = "vr_file_open_unsupported";
pub const VR_FILE_REVEAL_FAILED: &str = "vr_file_reveal_failed";
pub const VR_FILE_REVEAL_NOT_FILE: &str = "vr_file_reveal_not_file";
pub const VR_FILE_REVEAL_NOT_FOUND: &str = "vr_file_reveal_not_found";
pub const VR_FILE_REVEAL_OUTSIDE_FOLDER: &str = "vr_file_reveal_outside_folder";
pub const VR_FILE_REVEAL_STALE: &str = "vr_file_reveal_stale";
pub const VR_FILE_REVEAL_UNAVAILABLE: &str = "vr_file_reveal_unavailable";
pub const VR_FILE_REVEAL_UNSUPPORTED: &str = "vr_file_reveal_unsupported";
pub const VR_FILE_TRASH_FAILED: &str = "vr_file_trash_failed";
pub const VR_FILE_TRASH_NOT_FILE: &str = "vr_file_trash_not_file";
pub const VR_FILE_TRASH_NOT_FOUND: &str = "vr_file_trash_not_found";
pub const VR_FILE_TRASH_OWNED: &str = "vr_file_trash_owned";
pub const VR_FILE_TRASH_OWNERSHIP_UNAVAILABLE: &str = "vr_file_trash_ownership_unavailable";
pub const VR_FILE_TRASH_OUTSIDE_FOLDER: &str = "vr_file_trash_outside_folder";
pub const VR_FILE_TRASH_STALE: &str = "vr_file_trash_stale";
pub const VR_FILE_TRASH_UNAVAILABLE: &str = "vr_file_trash_unavailable";
pub const VR_FILE_TRASH_UNSUPPORTED: &str = "vr_file_trash_unsupported";

#[derive(Clone)]
struct TrustedVrFile {
    path: PathBuf,
    size: u64,
    modified: SystemTime,
}

struct CompletedVrScan {
    folder: PathBuf,
    generation: u64,
    files: Vec<TrustedVrFile>,
}

#[derive(Default)]
struct VrLibraryContext {
    generation: u64,
    completed_scan: Option<CompletedVrScan>,
}

#[derive(Clone, Default)]
pub struct VrLibraryState(Arc<Mutex<VrLibraryContext>>);

const MULTIPART_IDENTITY_PREFIXES: &[&str] = &["PART", "PT", "CD", "DISC", "DISK"];

fn exact_file_product_code(path: &Path) -> Option<String> {
    let title = path.file_stem()?.to_str()?;
    let mut codes = product_code_candidates(title)
        .into_iter()
        .filter(|(_, prefix)| {
            vr_library_product_code_prefix_is_supported(prefix)
                && !MULTIPART_IDENTITY_PREFIXES.contains(&prefix.as_str())
        })
        .map(|(code, _)| code)
        .collect::<Vec<_>>();
    codes.sort();
    codes.dedup();
    (codes.len() == 1).then(|| codes.remove(0))
}

pub(crate) fn vr_library_presentation_authority(
    state: &VrLibraryState,
    scan_generation: u64,
    configured_folder: &Path,
    code: &str,
) -> Result<LibraryItemAuthority, &'static str> {
    if exact_file_product_code(Path::new(code)).as_deref() != Some(code) {
        return Err(VR_LIBRARY_STALE);
    }
    let context = state.0.lock().map_err(|_| VR_LIBRARY_SCAN_FAILED)?;
    let scan = context.completed_scan.as_ref().ok_or(VR_LIBRARY_STALE)?;
    if scan.generation != scan_generation || scan.folder != configured_folder {
        return Err(VR_LIBRARY_STALE);
    }
    let mut members = scan
        .files
        .iter()
        .filter(|file| exact_file_product_code(&file.path).as_deref() == Some(code))
        .collect::<Vec<_>>();
    if members.is_empty() {
        return Err(VR_LIBRARY_STALE);
    }
    members.sort_by(|left, right| left.path.cmp(&right.path));
    let mut identity = format!("vr\0{}\0{code}", scan.folder.display());
    for member in members {
        let modified = member
            .modified
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(|_| VR_LIBRARY_STALE)?;
        identity.push_str(&format!(
            "\0{}\0{}\0{}\0{}",
            member.path.display(),
            member.size,
            modified.as_secs(),
            modified.subsec_nanos()
        ));
    }
    Ok(LibraryItemAuthority {
        category: LibraryPresentationCategory::Vr,
        identity: hex_sha1(identity.as_bytes()),
        code: code.to_owned(),
    })
}

#[derive(Clone, Copy)]
enum VrFileValidationError {
    NotFound,
    Unavailable,
    NotFile,
    Unsupported,
    OutsideFolder,
    Stale,
    Dispatch,
}

pub fn invalidate_vr_library(state: &VrLibraryState) -> Result<(), &'static str> {
    let mut context = state.0.lock().map_err(|_| VR_LIBRARY_SCAN_FAILED)?;
    context.generation = context.generation.wrapping_add(1);
    context.completed_scan = None;
    Ok(())
}

fn scan_media_files(folder: &Path) -> Result<Vec<TrustedVrFile>, &'static str> {
    let canonical_folder = fs::canonicalize(folder).map_err(|_| VR_LIBRARY_FOLDER_UNAVAILABLE)?;
    if canonical_folder != folder
        || !fs::metadata(&canonical_folder)
            .map_err(|_| VR_LIBRARY_FOLDER_UNAVAILABLE)?
            .is_dir()
    {
        return Err(VR_LIBRARY_FOLDER_UNAVAILABLE);
    }

    scan_library_files(folder, |path, metadata| {
        Some(TrustedVrFile {
            path,
            size: metadata.len(),
            modified: metadata.modified().ok()?,
        })
    })
    .map_err(|_| VR_LIBRARY_SCAN_FAILED)
}

pub fn scan_vr_library_with(
    download_state: &VrDownloadState,
    library_state: &VrLibraryState,
) -> Result<Vec<String>, &'static str> {
    let folder = configured_vr_folder(download_state)?.ok_or(VR_LIBRARY_FOLDER_UNAVAILABLE)?;
    let generation = {
        let mut context = library_state.0.lock().map_err(|_| VR_LIBRARY_SCAN_FAILED)?;
        context.generation = context.generation.wrapping_add(1);
        context.completed_scan = None;
        context.generation
    };
    let files = scan_media_files(&folder)?;

    if configured_vr_folder(download_state)?.as_ref() != Some(&folder) {
        return Err(VR_LIBRARY_STALE);
    }

    let mut response = Vec::with_capacity(1 + files.len() * 2);
    response.push(generation.to_string());
    for file in &files {
        response.push(
            file.path
                .to_str()
                .map(str::to_owned)
                .ok_or(VR_LIBRARY_SCAN_FAILED)?,
        );
        response.push(file.size.to_string());
    }
    let mut context = library_state.0.lock().map_err(|_| VR_LIBRARY_SCAN_FAILED)?;
    if context.generation != generation {
        return Err(VR_LIBRARY_STALE);
    }
    context.completed_scan = Some(CompletedVrScan {
        folder,
        generation,
        files,
    });
    Ok(response)
}

fn metadata_error(error: &io::Error) -> VrFileValidationError {
    if error.kind() == io::ErrorKind::NotFound {
        VrFileValidationError::NotFound
    } else {
        VrFileValidationError::Unavailable
    }
}

fn validate_vr_file(
    requested_path: &Path,
    configured_folder: &Path,
    scan: &CompletedVrScan,
    requested_generation: Option<u64>,
) -> Result<(), VrFileValidationError> {
    if scan.folder != configured_folder
        || requested_generation.is_some_and(|generation| generation != scan.generation)
    {
        return Err(VrFileValidationError::Stale);
    }

    let relative_path = requested_path
        .strip_prefix(configured_folder)
        .map_err(|_| VrFileValidationError::OutsideFolder)?;
    let mut checked_path = configured_folder.to_path_buf();
    for component in relative_path.components() {
        let Component::Normal(component) = component else {
            return Err(VrFileValidationError::OutsideFolder);
        };
        checked_path.push(component);
        let metadata =
            fs::symlink_metadata(&checked_path).map_err(|error| metadata_error(&error))?;
        if metadata.file_type().is_symlink() {
            return Err(VrFileValidationError::NotFile);
        }
    }

    let metadata = fs::metadata(requested_path).map_err(|error| metadata_error(&error))?;
    if !metadata.is_file() {
        return Err(VrFileValidationError::NotFile);
    }
    if !is_supported_library_media(requested_path) {
        return Err(VrFileValidationError::Unsupported);
    }
    let canonical_path =
        fs::canonicalize(requested_path).map_err(|error| metadata_error(&error))?;
    if canonical_path != requested_path || !canonical_path.starts_with(configured_folder) {
        return Err(VrFileValidationError::OutsideFolder);
    }

    let trusted_file = scan
        .files
        .iter()
        .find(|file| file.path == requested_path)
        .ok_or(VrFileValidationError::Stale)?;
    if trusted_file.size != metadata.len()
        || metadata
            .modified()
            .map_err(|error| metadata_error(&error))?
            != trusted_file.modified
    {
        return Err(VrFileValidationError::Stale);
    }
    Ok(())
}

fn run_vr_file_action(
    path: &Path,
    download_state: &VrDownloadState,
    library_state: &VrLibraryState,
    dispatch: impl FnOnce(&Path) -> Result<(), ()>,
) -> Result<(), VrFileValidationError> {
    with_configured_vr_folder(download_state, |configured_folder| {
        let configured_folder = configured_folder.ok_or(VrFileValidationError::Unavailable)?;
        let canonical_folder =
            fs::canonicalize(configured_folder).map_err(|_| VrFileValidationError::Unavailable)?;
        if canonical_folder != configured_folder
            || !fs::metadata(&canonical_folder)
                .map_err(|_| VrFileValidationError::Unavailable)?
                .is_dir()
        {
            return Err(VrFileValidationError::Unavailable);
        }

        let context = library_state
            .0
            .lock()
            .map_err(|_| VrFileValidationError::Unavailable)?;
        let scan = context
            .completed_scan
            .as_ref()
            .ok_or(VrFileValidationError::Stale)?;
        validate_vr_file(path, &canonical_folder, scan, None)?;
        dispatch(path).map_err(|_| VrFileValidationError::Dispatch)
    })
    .map_err(|_| VrFileValidationError::Unavailable)?
}

pub fn open_vr_file_with(
    path: &Path,
    download_state: &VrDownloadState,
    library_state: &VrLibraryState,
    dispatch: impl FnOnce(&Path) -> Result<(), ()>,
) -> Result<(), &'static str> {
    run_vr_file_action(path, download_state, library_state, dispatch).map_err(|error| match error {
        VrFileValidationError::NotFound => VR_FILE_OPEN_NOT_FOUND,
        VrFileValidationError::Unavailable => VR_FILE_OPEN_UNAVAILABLE,
        VrFileValidationError::NotFile => VR_FILE_OPEN_NOT_FILE,
        VrFileValidationError::Unsupported => VR_FILE_OPEN_UNSUPPORTED,
        VrFileValidationError::OutsideFolder => VR_FILE_OPEN_OUTSIDE_FOLDER,
        VrFileValidationError::Stale => VR_FILE_OPEN_STALE,
        VrFileValidationError::Dispatch => VR_FILE_OPEN_FAILED,
    })
}

pub fn reveal_vr_file_with(
    path: &Path,
    download_state: &VrDownloadState,
    library_state: &VrLibraryState,
    dispatch: impl FnOnce(&Path) -> Result<(), ()>,
) -> Result<(), &'static str> {
    run_vr_file_action(path, download_state, library_state, dispatch).map_err(|error| match error {
        VrFileValidationError::NotFound => VR_FILE_REVEAL_NOT_FOUND,
        VrFileValidationError::Unavailable => VR_FILE_REVEAL_UNAVAILABLE,
        VrFileValidationError::NotFile => VR_FILE_REVEAL_NOT_FILE,
        VrFileValidationError::Unsupported => VR_FILE_REVEAL_UNSUPPORTED,
        VrFileValidationError::OutsideFolder => VR_FILE_REVEAL_OUTSIDE_FOLDER,
        VrFileValidationError::Stale => VR_FILE_REVEAL_STALE,
        VrFileValidationError::Dispatch => VR_FILE_REVEAL_FAILED,
    })
}

pub fn trash_vr_file_with(
    path: &Path,
    scan_generation: u64,
    download_state: &VrDownloadState,
    library_state: &VrLibraryState,
    dispatch: impl FnOnce(&Path) -> Result<(), ()>,
) -> Result<(), &'static str> {
    with_unowned_vr_library_path(download_state, path, |configured_folder| {
        let configured_folder = configured_folder.ok_or(VR_FILE_TRASH_UNAVAILABLE)?;
        let canonical_folder =
            fs::canonicalize(configured_folder).map_err(|_| VR_FILE_TRASH_UNAVAILABLE)?;
        if canonical_folder != configured_folder
            || !fs::metadata(&canonical_folder)
                .map_err(|_| VR_FILE_TRASH_UNAVAILABLE)?
                .is_dir()
        {
            return Err(VR_FILE_TRASH_UNAVAILABLE);
        }

        let mut context = library_state
            .0
            .lock()
            .map_err(|_| VR_FILE_TRASH_UNAVAILABLE)?;
        let scan = context.completed_scan.as_ref().ok_or(VR_FILE_TRASH_STALE)?;
        validate_vr_file(path, &canonical_folder, scan, Some(scan_generation)).map_err(
            |error| match error {
                VrFileValidationError::NotFound => VR_FILE_TRASH_NOT_FOUND,
                VrFileValidationError::Unavailable => VR_FILE_TRASH_UNAVAILABLE,
                VrFileValidationError::NotFile => VR_FILE_TRASH_NOT_FILE,
                VrFileValidationError::Unsupported => VR_FILE_TRASH_UNSUPPORTED,
                VrFileValidationError::OutsideFolder => VR_FILE_TRASH_OUTSIDE_FOLDER,
                VrFileValidationError::Stale => VR_FILE_TRASH_STALE,
                VrFileValidationError::Dispatch => VR_FILE_TRASH_FAILED,
            },
        )?;

        dispatch(path).map_err(|_| VR_FILE_TRASH_FAILED)?;
        context
            .completed_scan
            .as_mut()
            .ok_or(VR_FILE_TRASH_STALE)?
            .files
            .retain(|file| file.path != path);
        Ok(())
    })
    .map_err(|error| match error {
        VrLibraryTrashOwnershipError::Owned => VR_FILE_TRASH_OWNED,
        VrLibraryTrashOwnershipError::Unavailable => VR_FILE_TRASH_OWNERSHIP_UNAVAILABLE,
    })?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vr_download::{clear_vr_folder, load_downloads, load_vr_folder_with, set_vr_folder};
    use std::{
        cell::Cell,
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    static FIXTURE_ID: AtomicU64 = AtomicU64::new(1);

    struct Fixture {
        path: PathBuf,
    }

    impl Fixture {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "auto-video-vr-library-{label}-{}-{}",
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

    fn configured_state(folder: &Path, config_path: &Path) -> VrDownloadState {
        let state = VrDownloadState::default();
        set_vr_folder(&state, config_path, folder.to_path_buf())
            .expect("VR folder must be configured");
        tauri::async_runtime::block_on(load_downloads(
            &state,
            &config_path.with_extension("downloads"),
            &config_path.with_extension("session"),
            &config_path.with_extension("limit"),
        ))
        .expect("empty transfer state must load");
        state
    }

    #[test]
    fn presentation_authority_requires_the_exact_current_mdvr_group() {
        let fixture = Fixture::new("presentation-authority");
        for name in [
            "MDVR-419 Part 01.mp4",
            "mdvr_00419 Disc 02.MKV",
            "MDVR-420.mp4",
            "MDVR-419 + ABC-123 pack.mp4",
            "unassociated.mp4",
        ] {
            fs::write(fixture.path.join(name), name.as_bytes()).expect("VR member must be written");
        }
        let config = fixture.path.join("config");
        let download_state = configured_state(&fixture.path, &config);
        let state = VrLibraryState::default();
        let scan = scan_vr_library_with(&download_state, &state).expect("scan must complete");
        let generation = scan[0].parse().expect("generation must be valid");

        let authority =
            vr_library_presentation_authority(&state, generation, &fixture.path, "MDVR-419")
                .expect("one exact MDVR group must authorize presentation");
        assert_eq!(authority.category, LibraryPresentationCategory::Vr);
        assert_eq!(authority.code, "MDVR-419");
        assert_eq!(authority.identity.len(), 40);
        for code in ["MDVR-421", "MDVR-419 + ABC-123"] {
            assert_eq!(
                vr_library_presentation_authority(&state, generation, &fixture.path, code,),
                Err(VR_LIBRARY_STALE)
            );
        }
        assert_eq!(
            vr_library_presentation_authority(
                &state,
                generation,
                &fixture.path.join("other"),
                "MDVR-419",
            ),
            Err(VR_LIBRARY_STALE)
        );
    }

    #[test]
    fn presentation_authority_accepts_one_exact_current_3dsvr_group() {
        let fixture = Fixture::new("presentation-authority-3dsvr");
        for name in [
            "3DSVR-01871-A.mp4",
            "3dsvr_001871-B.MKV",
            "3DSVR-01872-A.mp4",
            "9DSVR-01871-A.mp4",
            "X3DSVR-01871-A.mp4",
            "3DSVR-01871B.mp4",
            "459TEN-00048.mp4",
            "3DSVR-01871-A + MDVR-419.mp4",
        ] {
            fs::write(fixture.path.join(name), name.as_bytes()).expect("VR member must be written");
        }
        let config = fixture.path.join("config");
        let download_state = configured_state(&fixture.path, &config);
        let state = VrLibraryState::default();
        let scan = scan_vr_library_with(&download_state, &state).expect("scan must complete");
        let generation = scan[0].parse().expect("generation must be valid");

        let authority =
            vr_library_presentation_authority(&state, generation, &fixture.path, "3DSVR-1871")
                .expect("one exact 3DSVR group must authorize presentation");
        assert_eq!(authority.category, LibraryPresentationCategory::Vr);
        assert_eq!(authority.code, "3DSVR-1871");
        assert_eq!(authority.identity.len(), 40);
        let neighbor =
            vr_library_presentation_authority(&state, generation, &fixture.path, "3DSVR-1872")
                .expect("a neighboring exact code must remain a separate group");
        assert_ne!(neighbor.identity, authority.identity);
        for code in ["9DSVR-1871", "459TEN-48", "3DSVR-1871 + MDVR-419"] {
            assert_eq!(
                vr_library_presentation_authority(&state, generation, &fixture.path, code),
                Err(VR_LIBRARY_STALE)
            );
        }
    }

    #[test]
    fn scan_preserves_exact_paths_sizes_and_deterministic_order() {
        let fixture = Fixture::new("scan");
        let nested = fixture.path.join("深い folder");
        fs::create_dir(&nested).expect("nested folder must be created");
        let second = nested.join("Ｂ  Name.MKV");
        let first = fixture.path.join("A  Name.mp4");
        fs::write(&second, b"second").expect("second file must be written");
        fs::write(&first, b"one").expect("first file must be written");
        fs::write(fixture.path.join("ignored.txt"), b"ignored")
            .expect("ignored file must be written");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&first, fixture.path.join("ignored-link.mp4"))
            .expect("fixture symlink must be created");
        let config_path = fixture.path.join("config");
        let download_state = configured_state(&fixture.path, &config_path);
        let library_state = VrLibraryState::default();

        assert_eq!(
            scan_vr_library_with(&download_state, &library_state),
            Ok(vec![
                "1".to_owned(),
                first.to_string_lossy().into_owned(),
                "3".to_owned(),
                second.to_string_lossy().into_owned(),
                "6".to_owned(),
            ])
        );
    }

    #[test]
    fn file_action_rejects_fabricated_unrelated_folder_context_without_dispatch() {
        let trusted = Fixture::new("trusted-a");
        let unrelated = Fixture::new("unrelated-b");
        let trusted_file = trusted.path.join("MDVR-419.mp4");
        let unrelated_file = unrelated.path.join("MDVR-419.mp4");
        fs::write(&trusted_file, b"trusted").expect("trusted file must be written");
        fs::write(&unrelated_file, b"unrelated").expect("unrelated file must be written");
        let download_state = configured_state(&trusted.path, &trusted.path.join("config"));
        let library_state = VrLibraryState::default();
        scan_vr_library_with(&download_state, &library_state).expect("scan must complete");
        let dispatched = Cell::new(false);

        let result = open_vr_file_with(&unrelated_file, &download_state, &library_state, |_| {
            dispatched.set(true);
            Ok(())
        });

        assert_eq!(result, Err(VR_FILE_OPEN_OUTSIDE_FOLDER));
        assert!(!dispatched.get());
    }

    #[test]
    fn file_action_rejects_changed_file_from_latest_scan_without_dispatch() {
        let fixture = Fixture::new("stale");
        let movie = fixture.path.join("MDVR-419.mp4");
        fs::write(&movie, b"before").expect("movie must be written");
        let download_state = configured_state(&fixture.path, &fixture.path.join("config"));
        let library_state = VrLibraryState::default();
        scan_vr_library_with(&download_state, &library_state).expect("scan must complete");
        fs::write(&movie, b"after change").expect("movie must be replaced");
        let dispatched = Cell::new(false);

        let result = reveal_vr_file_with(&movie, &download_state, &library_state, |_| {
            dispatched.set(true);
            Ok(())
        });

        assert_eq!(result, Err(VR_FILE_REVEAL_STALE));
        assert!(!dispatched.get());
    }

    #[test]
    fn file_action_rejects_missing_directory_and_unsupported_paths_without_dispatch() {
        let fixture = Fixture::new("invalid-paths");
        let movie = fixture.path.join("MDVR-419.mp4");
        fs::write(&movie, b"movie").expect("movie must be written");
        let download_state = configured_state(&fixture.path, &fixture.path.join("config"));
        let library_state = VrLibraryState::default();
        scan_vr_library_with(&download_state, &library_state).expect("scan must complete");
        let dispatched = Cell::new(false);

        for (path, expected_error) in [
            (fixture.path.join("missing.mp4"), VR_FILE_OPEN_NOT_FOUND),
            (fixture.path.join("folder"), VR_FILE_OPEN_NOT_FILE),
            (
                fixture.path.join("unsupported.txt"),
                VR_FILE_OPEN_UNSUPPORTED,
            ),
        ] {
            if path.file_name().is_some_and(|name| name == "folder") {
                fs::create_dir(&path).expect("directory must be created");
            } else if path.extension().is_some_and(|extension| extension == "txt") {
                fs::write(&path, b"unsupported").expect("unsupported file must be written");
            }
            assert_eq!(
                open_vr_file_with(&path, &download_state, &library_state, |_| {
                    dispatched.set(true);
                    Ok(())
                }),
                Err(expected_error)
            );
        }
        assert!(!dispatched.get());
    }

    #[test]
    fn file_action_reports_dispatch_failure_for_an_exact_trusted_file() {
        let fixture = Fixture::new("dispatch-failure");
        let movie = fixture.path.join("MDVR-419.AVI");
        fs::write(&movie, b"movie").expect("movie must be written");
        let download_state = configured_state(&fixture.path, &fixture.path.join("config"));
        let library_state = VrLibraryState::default();
        scan_vr_library_with(&download_state, &library_state).expect("scan must complete");

        assert_eq!(
            open_vr_file_with(&movie, &download_state, &library_state, |_| Err(())),
            Err(VR_FILE_OPEN_FAILED)
        );
        assert_eq!(
            reveal_vr_file_with(&movie, &download_state, &library_state, |_| Err(())),
            Err(VR_FILE_REVEAL_FAILED)
        );
    }

    #[cfg(unix)]
    #[test]
    fn file_action_rejects_a_symlink_created_after_the_scan_without_dispatch() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new("symlink");
        let movie = fixture.path.join("MDVR-419.mp4");
        let link = fixture.path.join("MDVR-422.mp4");
        fs::write(&movie, b"movie").expect("movie must be written");
        let download_state = configured_state(&fixture.path, &fixture.path.join("config"));
        let library_state = VrLibraryState::default();
        scan_vr_library_with(&download_state, &library_state).expect("scan must complete");
        symlink(&movie, &link).expect("symlink must be created");
        let dispatched = Cell::new(false);

        assert_eq!(
            reveal_vr_file_with(&link, &download_state, &library_state, |_| {
                dispatched.set(true);
                Ok(())
            }),
            Err(VR_FILE_REVEAL_NOT_FILE)
        );
        assert!(!dispatched.get());
    }

    #[test]
    fn trash_dispatches_one_exact_mdvr_419_member_without_mutating_siblings_or_neighbors() {
        let fixture = Fixture::new("trash-exact");
        let configuration = Fixture::new("trash-config");
        let removed = fixture.path.join("MDVR-419 Part 01.WMV");
        let sibling = fixture.path.join("MDVR-419 PT 02.mkv");
        let ambiguous = fixture.path.join("MDVR-419 Part 01 Disc 02.mp4");
        let unassociated = fixture.path.join("MDVR-419 + ABC-123 pack.mkv");
        let neighbors = [
            fixture.path.join("MDVR-422.mp4"),
            fixture.path.join("MDVR-430.mp4"),
            fixture.path.join("MDVR-433.mp4"),
            fixture.path.join("MDVR-374.mp4"),
        ];
        for path in [
            &removed,
            &sibling,
            &ambiguous,
            &unassociated,
            &neighbors[0],
            &neighbors[1],
            &neighbors[2],
            &neighbors[3],
        ] {
            fs::write(path, b"file").expect("VR fixture file must be written");
        }
        let download_state = configured_state(&fixture.path, &configuration.path.join("config"));
        let library_state = VrLibraryState::default();
        let scan =
            scan_vr_library_with(&download_state, &library_state).expect("scan must complete");
        let generation = scan[0].parse().expect("generation must be valid");
        let dispatch_count = Cell::new(0);

        assert_eq!(
            trash_vr_file_with(
                &removed,
                generation,
                &download_state,
                &library_state,
                |_| {
                    dispatch_count.set(dispatch_count.get() + 1);
                    Err(())
                },
            ),
            Err(VR_FILE_TRASH_FAILED)
        );
        assert_eq!(
            trash_vr_file_with(
                &removed,
                generation,
                &download_state,
                &library_state,
                |path| {
                    assert_eq!(path, removed);
                    dispatch_count.set(dispatch_count.get() + 1);
                    Ok(())
                },
            ),
            Ok(())
        );
        assert_eq!(
            trash_vr_file_with(
                &removed,
                generation,
                &download_state,
                &library_state,
                |_| {
                    dispatch_count.set(dispatch_count.get() + 1);
                    Ok(())
                },
            ),
            Err(VR_FILE_TRASH_STALE)
        );
        assert_eq!(
            trash_vr_file_with(
                &sibling,
                generation,
                &download_state,
                &library_state,
                |_| Ok(()),
            ),
            Ok(())
        );
        assert_eq!(
            trash_vr_file_with(
                &unassociated,
                generation,
                &download_state,
                &library_state,
                |_| Ok(()),
            ),
            Ok(())
        );
        for path in [
            &ambiguous,
            &neighbors[0],
            &neighbors[1],
            &neighbors[2],
            &neighbors[3],
        ] {
            assert!(path.is_file());
        }
        assert_eq!(dispatch_count.get(), 2);
    }

    #[test]
    fn trash_rejects_untrusted_changed_and_unsafe_vr_paths_without_dispatch() {
        let trusted = Fixture::new("trash-trusted");
        let unrelated = Fixture::new("trash-unrelated");
        let configuration = Fixture::new("trash-invalid-config");
        let current = trusted.path.join("MDVR-419 Part 01.mp4");
        let changed = trusted.path.join("MDVR-419 CD2.mkv");
        let missing = trusted.path.join("MDVR-422.mp4");
        fs::write(&current, b"current").expect("current member must be written");
        fs::write(&changed, b"changed").expect("changed member must be written");
        fs::write(&missing, b"missing").expect("missing member must be written");
        let download_state = configured_state(&trusted.path, &configuration.path.join("config"));
        let library_state = VrLibraryState::default();
        let scan =
            scan_vr_library_with(&download_state, &library_state).expect("scan must complete");
        let generation = scan[0].parse().expect("generation must be valid");
        let dispatched = Cell::new(false);
        let same_name_elsewhere = unrelated.path.join("MDVR-419 Part 01.mp4");
        fs::write(&same_name_elsewhere, b"current").expect("unrelated file must be written");
        let neighboring_code = trusted.path.join("MDVR-430.mp4");
        fs::write(&neighboring_code, b"neighbor").expect("neighbor must be written");
        let mixed_code = trusted.path.join("MDVR-419 + ABC-123.mp4");
        fs::write(&mixed_code, b"mixed").expect("mixed-code file must be written");
        let directory = trusted.path.join("directory.mkv");
        fs::create_dir(&directory).expect("directory must be created");
        let unsupported = trusted.path.join("unsupported.txt");
        fs::write(&unsupported, b"unsupported").expect("unsupported file must be written");
        fs::write(&changed, b"different content").expect("member must change");
        fs::remove_file(&missing).expect("member must be removed");

        for (path, expected) in [
            (same_name_elsewhere, VR_FILE_TRASH_OUTSIDE_FOLDER),
            (neighboring_code, VR_FILE_TRASH_STALE),
            (mixed_code, VR_FILE_TRASH_STALE),
            (directory, VR_FILE_TRASH_NOT_FILE),
            (unsupported, VR_FILE_TRASH_UNSUPPORTED),
            (changed, VR_FILE_TRASH_STALE),
            (missing, VR_FILE_TRASH_NOT_FOUND),
        ] {
            assert_eq!(
                trash_vr_file_with(&path, generation, &download_state, &library_state, |_| {
                    dispatched.set(true);
                    Ok(())
                },),
                Err(expected)
            );
        }

        #[cfg(unix)]
        {
            let link = trusted.path.join("linked.mp4");
            std::os::unix::fs::symlink(&current, &link).expect("file symlink must be created");
            assert_eq!(
                trash_vr_file_with(&link, generation, &download_state, &library_state, |_| {
                    dispatched.set(true);
                    Ok(())
                },),
                Err(VR_FILE_TRASH_NOT_FILE)
            );
            let linked_parent = trusted.path.join("linked-parent");
            std::os::unix::fs::symlink(&unrelated.path, &linked_parent)
                .expect("parent symlink must be created");
            let linked_child = linked_parent.join("MDVR-419 Part 01.mp4");
            assert_eq!(
                trash_vr_file_with(
                    &linked_child,
                    generation,
                    &download_state,
                    &library_state,
                    |_| {
                        dispatched.set(true);
                        Ok(())
                    },
                ),
                Err(VR_FILE_TRASH_NOT_FILE)
            );
        }
        assert!(!dispatched.get());
    }

    #[test]
    fn trash_rejects_stale_generations_and_restart_scan_keeps_an_accepted_member_absent() {
        let fixture = Fixture::new("trash-generation");
        let configuration = Fixture::new("trash-generation-config");
        let holding = Fixture::new("trash-holding");
        let persistence_path = configuration.path.join("config");
        let member = fixture.path.join("MDVR-419 Disk-4.mp4");
        fs::write(&member, b"member").expect("VR member must be written");
        let download_state = configured_state(&fixture.path, &persistence_path);
        let library_state = VrLibraryState::default();
        let first_scan =
            scan_vr_library_with(&download_state, &library_state).expect("scan must complete");
        let first_generation = first_scan[0].parse().expect("generation must be valid");
        let second_scan =
            scan_vr_library_with(&download_state, &library_state).expect("scan must complete");
        let current_generation = second_scan[0].parse().expect("generation must be valid");
        let dispatched = Cell::new(false);

        assert_eq!(
            trash_vr_file_with(
                &member,
                first_generation,
                &download_state,
                &library_state,
                |_| {
                    dispatched.set(true);
                    Ok(())
                },
            ),
            Err(VR_FILE_TRASH_STALE)
        );
        assert!(!dispatched.get());

        let moved_path = holding.path.join("MDVR-419 Disk-4.mp4");
        trash_vr_file_with(
            &member,
            current_generation,
            &download_state,
            &library_state,
            |path| fs::rename(path, &moved_path).map_err(|_| ()),
        )
        .expect("accepted dispatch must succeed");
        assert!(moved_path.is_file());
        assert!(!member.exists());

        let restarted_download_state = VrDownloadState::default();
        assert_eq!(
            load_vr_folder_with(&restarted_download_state, &persistence_path),
            Ok(vec![
                "ready".to_owned(),
                fixture.path.to_string_lossy().into_owned(),
            ])
        );
        let restarted_library_state = VrLibraryState::default();
        let restarted_scan =
            scan_vr_library_with(&restarted_download_state, &restarted_library_state)
                .expect("restart scan must complete");
        assert_eq!(restarted_scan.len(), 1);
    }

    #[test]
    fn folder_replacement_clear_and_failed_refresh_invalidate_vr_trash_requests() {
        let configuration = Fixture::new("trash-configuration");
        let first = Fixture::new("trash-first-folder");
        let replacement = Fixture::new("trash-replacement-folder");
        let persistence_path = configuration.path.join("config");
        let member = first.path.join("MDVR-419 Disc 03.mp4");
        fs::write(&member, b"member").expect("VR member must be written");
        let download_state = configured_state(&first.path, &persistence_path);
        let library_state = VrLibraryState::default();
        let scan =
            scan_vr_library_with(&download_state, &library_state).expect("scan must complete");
        let generation = scan[0].parse().expect("generation must be valid");
        let dispatched = Cell::new(false);

        set_vr_folder(&download_state, &persistence_path, replacement.path.clone())
            .expect("replacement VR folder must be configured");
        assert_eq!(
            trash_vr_file_with(&member, generation, &download_state, &library_state, |_| {
                dispatched.set(true);
                Ok(())
            },),
            Err(VR_FILE_TRASH_STALE)
        );
        clear_vr_folder(&download_state, &persistence_path).expect("VR folder must clear");
        assert_eq!(
            trash_vr_file_with(&member, generation, &download_state, &library_state, |_| {
                dispatched.set(true);
                Ok(())
            },),
            Err(VR_FILE_TRASH_UNAVAILABLE)
        );

        set_vr_folder(&download_state, &persistence_path, first.path.clone())
            .expect("first VR folder must be restored");
        let refreshed_scan =
            scan_vr_library_with(&download_state, &library_state).expect("scan must complete");
        let refreshed_generation = refreshed_scan[0].parse().expect("generation must be valid");
        let unavailable_folder = configuration.path.join("unavailable-VR-folder");
        fs::rename(&first.path, &unavailable_folder).expect("VR folder must be unavailable");
        assert_eq!(
            scan_vr_library_with(&download_state, &library_state),
            Err(VR_LIBRARY_FOLDER_UNAVAILABLE)
        );
        assert_eq!(
            trash_vr_file_with(
                &member,
                refreshed_generation,
                &download_state,
                &library_state,
                |_| {
                    dispatched.set(true);
                    Ok(())
                },
            ),
            Err(VR_FILE_TRASH_UNAVAILABLE)
        );
        assert!(!dispatched.get());
    }
}
