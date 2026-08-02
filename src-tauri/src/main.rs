#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    fs, io,
    path::{Path, PathBuf},
};

const MOVIES_FOLDER_UNAVAILABLE: &str = "movies_folder_unavailable";
const MOVIES_SCAN_FAILED: &str = "movies_scan_failed";

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

#[tauri::command]
async fn scan_movies(folder: String) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || scan_movie_paths(Path::new(&folder)))
        .await
        .map_err(|_| MOVIES_SCAN_FAILED.to_owned())?
        .map_err(str::to_owned)
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![scan_movies])
        .run(tauri::generate_context!())
        .expect("failed to run the Auto-Video desktop application");
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::{scan_movie_paths, MOVIES_FOLDER_UNAVAILABLE};

    static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

    struct MoviesFixture {
        path: PathBuf,
    }

    impl MoviesFixture {
        fn new() -> Self {
            let fixture_id = NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "auto-video-movies-fixture-{}-{fixture_id}",
                std::process::id()
            ));
            fs::create_dir(&path).expect("failed to create Movies fixture");
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

    impl Drop for MoviesFixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.path).expect("failed to remove Movies fixture");
        }
    }

    fn path_string(path: PathBuf) -> String {
        path.into_os_string()
            .into_string()
            .expect("fixture paths must be valid Unicode")
    }

    #[test]
    fn recursively_finds_supported_files_in_deterministic_order() {
        let fixture = MoviesFixture::new();
        let first_movie = fixture.create_file("Alpha.mp4");
        let second_movie = fixture.create_file("nested/Beta.MKV");
        let third_movie = fixture.create_file("nested/deeper/映画 — Final.mP4");
        fixture.create_file("nested/notes.txt");
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
        let fixture = MoviesFixture::new();
        let file_path = fixture.create_file("not-a-folder.mp4");
        let missing_path = fixture.path.join("missing");

        assert_eq!(
            scan_movie_paths(&missing_path),
            Err(MOVIES_FOLDER_UNAVAILABLE)
        );
        assert_eq!(scan_movie_paths(&file_path), Err(MOVIES_FOLDER_UNAVAILABLE));
    }
}
