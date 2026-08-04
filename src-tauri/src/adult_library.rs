use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    time::SystemTime,
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

#[derive(Clone)]
struct TrustedAdultFile {
    path: PathBuf,
    size: u64,
    modified: SystemTime,
}

struct CompletedAdultScan {
    folder: PathBuf,
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

fn is_supported_media(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("mp4") || extension.eq_ignore_ascii_case("mkv")
        })
}

fn collect_media_files(directory: &Path, files: &mut Vec<TrustedAdultFile>) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();

        if file_type.is_dir() {
            collect_media_files(&path, files)?;
        } else if file_type.is_file() && is_supported_media(&path) {
            let metadata = entry.metadata()?;
            files.push(TrustedAdultFile {
                path,
                size: metadata.len(),
                modified: metadata.modified()?,
            });
        }
    }
    Ok(())
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
    let mut files = Vec::new();
    collect_media_files(folder, &mut files).map_err(|_| ADULT_LIBRARY_SCAN_FAILED)?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
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

    let mut response = Vec::with_capacity(files.len() * 3);
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
    context.completed_scan = Some(CompletedAdultScan { folder, files });
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
) -> Result<(), AdultFileValidationError> {
    if scan.folder != configured_folder {
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
    if !is_supported_media(requested_path) {
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
    validate_adult_file(path, &canonical_folder, scan)?;
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
        let unsupported = fixture.path.join("unsupported.avi");
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
        let movie = fixture.path.join("ADLT-123.mp4");
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
}
