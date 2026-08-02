#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use tauri::Manager;

const MOVIES_FOLDER_UNAVAILABLE: &str = "movies_folder_unavailable";
const MOVIES_SCAN_FAILED: &str = "movies_scan_failed";
const MOVIE_OPEN_FAILED: &str = "movie_open_failed";
const MOVIE_OPEN_NOT_FILE: &str = "movie_open_not_file";
const MOVIE_OPEN_NOT_FOUND: &str = "movie_open_not_found";
const MOVIE_OPEN_UNAVAILABLE: &str = "movie_open_unavailable";
const MOVIE_OPEN_UNSUPPORTED: &str = "movie_open_unsupported";
const TMDB_TOKEN_FILE_NAME: &str = ".tmdb-api-read-access-token";
const TMDB_TOKEN_INVALID: &str = "tmdb_token_invalid";
const TMDB_TOKEN_STORAGE_FAILED: &str = "tmdb_token_storage_failed";
// API Read Access Tokens are much shorter; this rejects arbitrary oversized IPC or file input.
const TMDB_TOKEN_MAX_LENGTH: usize = 4096;

fn collect_movie_paths(directory: &Path, movie_paths: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();

        if file_type.is_dir() {
            collect_movie_paths(&path, movie_paths)?;
        } else if file_type.is_file() && is_supported_movie(&path) {
            movie_paths.push(path);
        }
    }

    Ok(())
}

fn is_supported_movie(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("mp4") || extension.eq_ignore_ascii_case("mkv")
        })
}

fn scan_movie_paths(folder: &Path) -> Result<Vec<String>, &'static str> {
    let metadata = fs::metadata(folder).map_err(|_| MOVIES_FOLDER_UNAVAILABLE)?;
    if !metadata.is_dir() {
        return Err(MOVIES_FOLDER_UNAVAILABLE);
    }

    let mut movie_paths = Vec::new();
    collect_movie_paths(folder, &mut movie_paths).map_err(|_| MOVIES_SCAN_FAILED)?;
    movie_paths.sort();

    movie_paths
        .into_iter()
        .map(|path| {
            path.into_os_string()
                .into_string()
                .map_err(|_| MOVIES_SCAN_FAILED)
        })
        .collect()
}

fn movie_metadata_error(error: &io::Error) -> &'static str {
    if error.kind() == io::ErrorKind::NotFound {
        MOVIE_OPEN_NOT_FOUND
    } else {
        MOVIE_OPEN_UNAVAILABLE
    }
}

fn open_movie_path_with(
    path: &Path,
    dispatch: impl FnOnce(&Path) -> Result<(), ()>,
) -> Result<(), &'static str> {
    let metadata = fs::metadata(path).map_err(|error| movie_metadata_error(&error))?;
    if !metadata.is_file() {
        return Err(MOVIE_OPEN_NOT_FILE);
    }
    if !is_supported_movie(path) {
        return Err(MOVIE_OPEN_UNSUPPORTED);
    }

    dispatch(path).map_err(|_| MOVIE_OPEN_FAILED)
}

fn is_valid_tmdb_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= TMDB_TOKEN_MAX_LENGTH
        && token.trim() == token
        && !token.chars().any(char::is_control)
}

fn load_tmdb_token_file(path: &Path) -> Result<Option<String>, &'static str> {
    let token = match fs::read_to_string(path) {
        Ok(token) => token,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(TMDB_TOKEN_STORAGE_FAILED),
    };

    if !is_valid_tmdb_token(&token) {
        return Err(TMDB_TOKEN_STORAGE_FAILED);
    }

    Ok(Some(token))
}

fn save_tmdb_token_file(path: &Path, token: &str) -> Result<(), &'static str> {
    if !is_valid_tmdb_token(token) {
        return Err(TMDB_TOKEN_INVALID);
    }

    let parent = path.parent().ok_or(TMDB_TOKEN_STORAGE_FAILED)?;
    fs::create_dir_all(parent).map_err(|_| TMDB_TOKEN_STORAGE_FAILED)?;

    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options.open(path).map_err(|_| TMDB_TOKEN_STORAGE_FAILED)?;
    file.write_all(token.as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|_| TMDB_TOKEN_STORAGE_FAILED)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|_| TMDB_TOKEN_STORAGE_FAILED)?;
    }

    Ok(())
}

fn clear_tmdb_token_file(path: &Path) -> Result<(), &'static str> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(TMDB_TOKEN_STORAGE_FAILED),
    }
}

fn tmdb_token_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|directory| directory.join(TMDB_TOKEN_FILE_NAME))
        .map_err(|_| TMDB_TOKEN_STORAGE_FAILED.to_owned())
}

#[tauri::command]
async fn scan_movies(folder: String) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || scan_movie_paths(Path::new(&folder)))
        .await
        .map_err(|_| MOVIES_SCAN_FAILED.to_owned())?
        .map_err(str::to_owned)
}

#[tauri::command]
async fn open_movie(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        open_movie_path_with(Path::new(&path), |movie_path| {
            tauri_plugin_opener::open_path(movie_path, None::<&str>).map_err(|_| ())
        })
    })
    .await
    .map_err(|_| MOVIE_OPEN_FAILED.to_owned())?
    .map_err(str::to_owned)
}

#[tauri::command]
fn load_tmdb_token(app: tauri::AppHandle) -> Result<Option<String>, String> {
    load_tmdb_token_file(&tmdb_token_path(&app)?).map_err(str::to_owned)
}

#[tauri::command]
fn save_tmdb_token(app: tauri::AppHandle, token: String) -> Result<(), String> {
    save_tmdb_token_file(&tmdb_token_path(&app)?, &token).map_err(str::to_owned)
}

#[tauri::command]
fn clear_tmdb_token(app: tauri::AppHandle) -> Result<(), String> {
    clear_tmdb_token_file(&tmdb_token_path(&app)?).map_err(str::to_owned)
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            scan_movies,
            open_movie,
            load_tmdb_token,
            save_tmdb_token,
            clear_tmdb_token
        ])
        .run(tauri::generate_context!())
        .expect("failed to run the Auto-Video desktop application");
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        fs, io,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::{
        clear_tmdb_token_file, load_tmdb_token_file, movie_metadata_error, open_movie_path_with,
        save_tmdb_token_file, scan_movie_paths, MOVIES_FOLDER_UNAVAILABLE, MOVIE_OPEN_FAILED,
        MOVIE_OPEN_NOT_FILE, MOVIE_OPEN_NOT_FOUND, MOVIE_OPEN_UNAVAILABLE, MOVIE_OPEN_UNSUPPORTED,
        TMDB_TOKEN_INVALID,
    };

    static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

    struct FilesystemFixture {
        path: PathBuf,
    }

    impl FilesystemFixture {
        fn new() -> Self {
            let fixture_id = NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "auto-video-filesystem-fixture-{}-{fixture_id}",
                std::process::id()
            ));
            fs::create_dir(&path).expect("failed to create filesystem fixture");
            Self { path }
        }

        fn create_file(&self, relative_path: impl AsRef<Path>) -> PathBuf {
            let path = self.path.join(relative_path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("failed to create fixture directory");
            }
            fs::write(&path, []).expect("failed to create fixture file");
            path
        }
    }

    impl Drop for FilesystemFixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.path).expect("failed to remove filesystem fixture");
        }
    }

    fn path_string(path: PathBuf) -> String {
        path.into_os_string()
            .into_string()
            .expect("fixture paths must be valid Unicode")
    }

    #[test]
    fn recursively_finds_supported_files_in_deterministic_order() {
        let fixture = FilesystemFixture::new();
        let first_movie = fixture.create_file("Alpha.mp4");
        let second_movie = fixture.create_file(Path::new("nested").join("Beta.MKV"));
        let third_movie =
            fixture.create_file(Path::new("nested").join("deeper").join("映画 — Final.mP4"));
        fixture.create_file(Path::new("nested").join("notes.txt"));
        fixture.create_file("clip.mov");
        fs::create_dir(fixture.path.join("directory.mkv"))
            .expect("failed to create fixture directory");

        let mut expected_paths = vec![
            path_string(first_movie),
            path_string(second_movie),
            path_string(third_movie),
        ];
        expected_paths.sort();

        assert_eq!(scan_movie_paths(&fixture.path), Ok(expected_paths));
    }

    #[test]
    fn reports_an_unavailable_folder_for_missing_or_non_directory_roots() {
        let fixture = FilesystemFixture::new();
        let file_path = fixture.create_file("not-a-folder.mp4");
        let missing_path = fixture.path.join("missing");

        assert_eq!(
            scan_movie_paths(&missing_path),
            Err(MOVIES_FOLDER_UNAVAILABLE)
        );
        assert_eq!(scan_movie_paths(&file_path), Err(MOVIES_FOLDER_UNAVAILABLE));
    }

    #[test]
    fn opens_the_exact_supported_movie_path_after_validation() {
        let fixture = FilesystemFixture::new();
        let movie_path = fixture.create_file("映画  —  Final.CUT!.MKV");
        let dispatched_path = RefCell::new(None);

        let result = open_movie_path_with(&movie_path, |path| {
            dispatched_path.replace(Some(path.to_path_buf()));
            Ok(())
        });

        assert_eq!(result, Ok(()));
        assert_eq!(dispatched_path.into_inner(), Some(movie_path));
    }

    #[test]
    fn rejects_missing_directories_and_unsupported_files_before_dispatch() {
        let fixture = FilesystemFixture::new();
        let missing_path = fixture.path.join("missing.mp4");
        let directory_path = fixture.path.join("directory.mkv");
        let unsupported_path = fixture.create_file("notes.txt");
        fs::create_dir(&directory_path).expect("failed to create fixture directory");

        for (path, expected_error) in [
            (&missing_path, MOVIE_OPEN_NOT_FOUND),
            (&directory_path, MOVIE_OPEN_NOT_FILE),
            (&unsupported_path, MOVIE_OPEN_UNSUPPORTED),
        ] {
            let dispatched = RefCell::new(false);
            let result = open_movie_path_with(path, |_| {
                dispatched.replace(true);
                Ok(())
            });

            assert_eq!(result, Err(expected_error));
            assert!(!dispatched.into_inner());
        }
    }

    #[test]
    fn reports_inaccessible_metadata_and_operating_system_failures() {
        let inaccessible = io::Error::new(io::ErrorKind::PermissionDenied, "fixture denial");
        assert_eq!(movie_metadata_error(&inaccessible), MOVIE_OPEN_UNAVAILABLE);

        let fixture = FilesystemFixture::new();
        let movie_path = fixture.create_file("Valid.mp4");
        assert_eq!(
            open_movie_path_with(&movie_path, |_| Err(())),
            Err(MOVIE_OPEN_FAILED)
        );
    }

    #[test]
    fn saves_replaces_loads_and_clears_the_tmdb_token() {
        let fixture = FilesystemFixture::new();
        let token_path = fixture.path.join("token");

        assert_eq!(load_tmdb_token_file(&token_path), Ok(None));

        save_tmdb_token_file(&token_path, "fixture-token-one")
            .expect("failed to save fixture token");
        assert_eq!(
            load_tmdb_token_file(&token_path),
            Ok(Some("fixture-token-one".to_owned()))
        );

        save_tmdb_token_file(&token_path, "fixture-token-two")
            .expect("failed to replace fixture token");
        assert_eq!(
            load_tmdb_token_file(&token_path),
            Ok(Some("fixture-token-two".to_owned()))
        );

        clear_tmdb_token_file(&token_path).expect("failed to clear fixture token");
        assert_eq!(load_tmdb_token_file(&token_path), Ok(None));
        clear_tmdb_token_file(&token_path).expect("clearing a missing token must succeed");
    }

    #[test]
    fn rejects_empty_whitespace_and_control_characters_in_tmdb_tokens() {
        let fixture = FilesystemFixture::new();
        let token_path = fixture.path.join("token");

        for token in ["", " leading", "trailing ", "line\nbreak"] {
            assert_eq!(
                save_tmdb_token_file(&token_path, token),
                Err(TMDB_TOKEN_INVALID)
            );
        }
        assert_eq!(load_tmdb_token_file(&token_path), Ok(None));
    }

    #[cfg(unix)]
    #[test]
    fn restricts_the_tmdb_token_file_to_the_current_user() {
        use std::os::unix::fs::PermissionsExt;

        let fixture = FilesystemFixture::new();
        let token_path = fixture.path.join("token");
        save_tmdb_token_file(&token_path, "fixture-token").expect("failed to save fixture token");

        let mode = fs::metadata(token_path)
            .expect("failed to read fixture token metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
