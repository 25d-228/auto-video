use std::{
    fs::{self, Metadata},
    io,
    path::{Path, PathBuf},
};

const MAX_LIBRARY_FILES: usize = 5_000;

const SUPPORTED_LIBRARY_EXTENSIONS: [&str; 13] = [
    "mkv", "mp4", "avi", "wmv", "m4v", "ts", "mov", "flv", "iso", "rmvb", "webm", "mpg", "mpeg",
];

pub(crate) fn is_supported_library_media(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            SUPPORTED_LIBRARY_EXTENSIONS
                .iter()
                .any(|supported| extension.eq_ignore_ascii_case(supported))
        })
}

pub(crate) fn scan_library_files<T>(
    root: &Path,
    mut capture: impl FnMut(PathBuf, Metadata) -> Option<T>,
) -> io::Result<Vec<T>> {
    let mut files = Vec::new();
    collect_library_files(root, true, &mut files, &mut capture)?;
    Ok(files)
}

fn collect_library_files<T>(
    directory: &Path,
    is_root: bool,
    files: &mut Vec<T>,
    capture: &mut impl FnMut(PathBuf, Metadata) -> Option<T>,
) -> io::Result<()> {
    if files.len() >= MAX_LIBRARY_FILES {
        return Ok(());
    }
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if is_root => return Err(error),
        Err(_) => return Ok(()),
    };
    let mut entries = entries.filter_map(Result::ok).collect::<Vec<_>>();
    entries.sort_by_key(fs::DirEntry::file_name);

    for entry in entries {
        if files.len() >= MAX_LIBRARY_FILES {
            break;
        }
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        if file_name.starts_with('.') {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if file_type.is_dir() {
            collect_library_files(&path, false, files, capture)?;
        } else if file_type.is_file() && is_supported_library_media(&path) {
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if let Some(file) = capture(path, metadata) {
                files.push(file);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::File,
        sync::atomic::{AtomicU64, Ordering},
    };

    static FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        path: PathBuf,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "auto-video-library-scan-{name}-{}-{}",
                std::process::id(),
                FIXTURE_ID.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).expect("fixture directory must be created");
            Self { path }
        }

        fn create_file(&self, relative_path: impl AsRef<Path>) -> PathBuf {
            let path = self.path.join(relative_path);
            fs::create_dir_all(path.parent().expect("fixture file must have a parent"))
                .expect("fixture parent must be created");
            File::create(&path).expect("fixture file must be created");
            path
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn relative_paths(root: &Path) -> Vec<PathBuf> {
        scan_library_files(root, |path, _| {
            path.strip_prefix(root).ok().map(Path::to_path_buf)
        })
        .expect("Library scan must complete")
    }

    #[test]
    fn accepts_every_library_extension_and_excludes_hidden_unsupported_and_symlink_entries() {
        let fixture = Fixture::new("formats");
        let root = fixture.path.join(".configured-root");
        fs::create_dir(&root).expect("dot-prefixed configured root must be created");
        let extensions = [
            "MKV", "Mp4", "AVI", "WmV", "M4V", "Ts", "MOV", "FlV", "ISO", "RmVb", "WebM", "MpG",
            "MPEG",
        ];
        let expected = extensions
            .iter()
            .enumerate()
            .map(|(index, extension)| {
                let relative =
                    PathBuf::from("visible").join(format!("{index:02}-media.{extension}"));
                let path = root.join(&relative);
                fs::create_dir_all(path.parent().expect("media file must have a parent"))
                    .expect("visible directory must be created");
                File::create(path).expect("media file must be created");
                relative
            })
            .collect::<Vec<_>>();
        File::create(root.join(".preview.mp4")).expect("hidden file must be created");
        fs::create_dir(root.join(".cache")).expect("hidden directory must be created");
        File::create(root.join(".cache/hidden.avi")).expect("hidden media must be created");
        fs::create_dir_all(root.join("nested/.metadata/deeper"))
            .expect("nested hidden directory must be created");
        File::create(root.join("nested/.metadata/deeper/hidden.mkv"))
            .expect("nested hidden media must be created");
        File::create(root.join("visible/notes.txt")).expect("unsupported file must be created");
        fs::create_dir(root.join("visible/directory.mp4"))
            .expect("extension-shaped directory must be created");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(
                root.join(&expected[0]),
                root.join("visible/linked-file.mkv"),
            )
            .expect("file symlink must be created");
            std::os::unix::fs::symlink(root.join("visible"), root.join("linked-directory"))
                .expect("directory symlink must be created");
        }

        assert_eq!(relative_paths(&root), expected);
    }

    #[cfg(unix)]
    #[test]
    fn skips_an_unreadable_nested_directory_and_keeps_readable_siblings() {
        use std::os::unix::fs::PermissionsExt;

        let fixture = Fixture::new("unreadable");
        let first = fixture.create_file("a-readable/first.mp4");
        let unreadable = fixture.path.join("b-unreadable");
        fs::create_dir(&unreadable).expect("unreadable directory must be created");
        File::create(unreadable.join("hidden-by-permissions.mkv"))
            .expect("unreadable child must be created");
        let second = fixture.create_file("c-readable/second.avi");
        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000))
            .expect("unreadable permissions must be applied");

        let result = relative_paths(&fixture.path);

        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o700))
            .expect("fixture permissions must be restored");
        assert_eq!(
            result,
            vec![
                first.strip_prefix(&fixture.path).unwrap().to_path_buf(),
                second.strip_prefix(&fixture.path).unwrap().to_path_buf(),
            ]
        );
    }

    #[test]
    fn returns_the_same_first_five_thousand_relative_paths_in_order() {
        let fixture = Fixture::new("limit");
        for index in 0..=MAX_LIBRARY_FILES {
            fixture.create_file(format!("media-{index:05}.mkv"));
        }
        fixture.create_file(".hidden.mp4");
        fixture.create_file("unsupported.txt");

        let first = relative_paths(&fixture.path);
        let second = relative_paths(&fixture.path);

        assert_eq!(first, second);
        assert_eq!(first.len(), MAX_LIBRARY_FILES);
        assert_eq!(first.first(), Some(&PathBuf::from("media-00000.mkv")));
        assert_eq!(first.last(), Some(&PathBuf::from("media-04999.mkv")));
    }
}
