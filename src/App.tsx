import { open } from "@tauri-apps/plugin-dialog";
import {
  type KeyboardEvent as ReactKeyboardEvent,
  useEffect,
  useRef,
  useState,
} from "react";

import "./index.css";

const destinations = [
  {
    id: "dashboard",
    label: "Dashboard",
    description: "A home for verified activity and storage information.",
    emptyHeading: "Dashboard data is not available yet",
    emptyMessage:
      "Metrics and storage details will appear here only after their data sources are implemented.",
  },
  {
    id: "discover",
    label: "Discover",
    description: "Find media after discovery providers are introduced.",
    emptyHeading: "Discovery is not configured",
    emptyMessage:
      "Provider feeds, search, and media results will be added in a later product slice.",
  },
  {
    id: "library",
    label: "Library",
    description: "Browse supported video files from your local Movies folder.",
    emptyHeading: "Choose a Movies folder to begin",
    emptyMessage:
      "Configure one local Movies folder in Settings before scanning your library.",
  },
  {
    id: "downloads",
    label: "Downloads",
    description: "Review transfers after download support is implemented.",
    emptyHeading: "Downloads are not available yet",
    emptyMessage:
      "Queue behavior, transfer controls, and torrent handling will be introduced separately.",
  },
  {
    id: "settings",
    label: "Settings",
    description: "Configure your local Movies folder and application appearance.",
    emptyHeading: "Other settings are not configured",
    emptyMessage:
      "Provider credentials and additional preferences will appear only with the features they control.",
  },
] as const;

const appearanceModes = [
  { id: "light", label: "Light" },
  { id: "dark", label: "Dark" },
  { id: "system", label: "System" },
] as const;

const iconPaths = {
  brand: ["m8 5 11 7-11 7V5Z"],
  dashboard: [
    "M3 3h7v7H3V3Z",
    "M14 3h7v7h-7V3Z",
    "M3 14h7v7H3v-7Z",
    "M14 14h7v7h-7v-7Z",
  ],
  discover: [
    "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18Z",
    "m15.5 8.5-2 5-5 2 2-5 5-2Z",
  ],
  library: [
    "M4 19.5V5a2 2 0 0 1 2-2h11v16H6a2 2 0 0 0-2 2Z",
    "M8 7h5",
    "M8 11h5",
  ],
  downloads: ["M12 3v12", "m7 10 5 5 5-5", "M5 21h14"],
  settings: [
    "M12 15.5a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7Z",
    "M19.4 15a1.7 1.7 0 0 0 .34 1.88l.06.06-2.12 2.12-.06-.06a1.7 1.7 0 0 0-1.88-.34 1.7 1.7 0 0 0-1 1.56V20h-3v-.08a1.7 1.7 0 0 0-1-1.56 1.7 1.7 0 0 0-1.88.34l-.06.06-2.12-2.12.06-.06A1.7 1.7 0 0 0 6.08 15 1.7 1.7 0 0 0 4.52 14H4v-3h.52a1.7 1.7 0 0 0 1.56-1 1.7 1.7 0 0 0-.34-1.88l-.06-.06 2.12-2.12.06.06a1.7 1.7 0 0 0 1.88.34 1.7 1.7 0 0 0 1-1.56V4h3v.78a1.7 1.7 0 0 0 1 1.56 1.7 1.7 0 0 0 1.88-.34l.06-.06 2.12 2.12-.06.06A1.7 1.7 0 0 0 19.4 10 1.7 1.7 0 0 0 20.96 11H21v3h-.04a1.7 1.7 0 0 0-1.56 1Z",
  ],
  light: [
    "M12 2v2",
    "M12 20v2",
    "m4.93-14.93 1.41-1.41",
    "m5.66 18.34 1.41-1.41",
    "M20 12h2",
    "M2 12h2",
    "m16.93 4.93 1.41 1.41",
    "m5.66 5.66 1.41 1.41",
    "M12 17a5 5 0 1 0 0-10 5 5 0 0 0 0 10Z",
  ],
  dark: ["M21 12.8A9 9 0 1 1 11.2 3 7 7 0 0 0 21 12.8Z"],
  system: ["M4 4h16v12H4V4Z", "M8 20h8", "M12 16v4"],
  folder: ["M3 6h6l2 2h10v11H3V6Z"],
  refresh: ["M20 6v5h-5", "M4 18v-5h5", "M18.5 9A7 7 0 0 0 6 6.5L4 11", "M5.5 15A7 7 0 0 0 18 17.5l2-4.5"],
  movie: ["M5 3h14v18H5V3Z", "m10 12-5 3V9l5 3Z"],
} as const;

type AppearanceMode = (typeof appearanceModes)[number]["id"];
type IconName = keyof typeof iconPaths;
type ResolvedTheme = Exclude<AppearanceMode, "system">;
type Movie = { path: string; title: string };
type MovieScanState =
  | { status: "unconfigured" }
  | { status: "scanning" }
  | { status: "empty" }
  | { status: "unavailable" }
  | { status: "error" }
  | { status: "ready"; movies: Movie[] };

const appearanceStorageKey = "auto-video-appearance";
const moviesFolderStorageKey = "auto-video-movies-folder";
const moviesFolderUnavailable = "movies_folder_unavailable";
const systemDarkModeQuery = "(prefers-color-scheme: dark)";

const movieScanMessages = {
  unconfigured: {
    heading: "Choose a Movies folder to begin",
    message:
      "Configure one local Movies folder in Settings before scanning your library.",
    role: undefined,
  },
  scanning: {
    heading: "Scanning Movies folder",
    message: "Looking recursively for .mp4 and .mkv files.",
    role: "status",
  },
  empty: {
    heading: "No supported videos found",
    message: "This folder does not contain any .mp4 or .mkv files.",
    role: undefined,
  },
  unavailable: {
    heading: "Movies folder is unavailable",
    message:
      "The configured folder may have moved or become inaccessible. Check it in Settings or try Refresh.",
    role: "alert",
  },
  error: {
    heading: "Movies folder could not be scanned",
    message:
      "Auto-Video could not read every item in this folder. Check its access and try Refresh.",
    role: "alert",
  },
} as const;

function AppIcon({ name }: { name: IconName }) {
  return (
    <svg
      aria-hidden="true"
      className="app-icon"
      fill="none"
      focusable="false"
      viewBox="0 0 24 24"
    >
      {iconPaths[name].map((path) => (
        <path d={path} key={path} />
      ))}
    </svg>
  );
}

function isAppearanceMode(value: string | null): value is AppearanceMode {
  return appearanceModes.some((mode) => mode.id === value);
}

function movieTitleFromPath(path: string) {
  const filename = path.split(/[/\\]/).at(-1) ?? path;
  const extensionStart = filename.lastIndexOf(".");
  return extensionStart > 0 ? filename.slice(0, extensionStart) : filename;
}

export default function App() {
  const [activeDestination, setActiveDestination] = useState<
    (typeof destinations)[number]
  >(destinations[0]);
  const [appearance, setAppearance] = useState<AppearanceMode>(() => {
    const storedAppearance = window.localStorage.getItem(appearanceStorageKey);
    return isAppearanceMode(storedAppearance) ? storedAppearance : "system";
  });
  const [systemTheme, setSystemTheme] = useState<ResolvedTheme>(() =>
    window.matchMedia(systemDarkModeQuery).matches ? "dark" : "light",
  );
  const [moviesFolder, setMoviesFolder] = useState<string | null>(() => {
    const storedFolder = window.localStorage.getItem(moviesFolderStorageKey);
    return storedFolder === "" ? null : storedFolder;
  });
  const [movieScanState, setMovieScanState] = useState<MovieScanState>(
    moviesFolder === null ? { status: "unconfigured" } : { status: "scanning" },
  );
  const [refreshVersion, setRefreshVersion] = useState(0);
  const [isChoosingFolder, setIsChoosingFolder] = useState(false);
  const [folderSelectionError, setFolderSelectionError] = useState<
    string | null
  >(null);
  const navigationItems = useRef<Array<HTMLButtonElement | null>>([]);
  const scanRequestId = useRef(0);

  useEffect(() => {
    const systemPreference = window.matchMedia(systemDarkModeQuery);
    const updateSystemTheme = (event: MediaQueryListEvent) => {
      setSystemTheme(event.matches ? "dark" : "light");
    };

    setSystemTheme(systemPreference.matches ? "dark" : "light");
    systemPreference.addEventListener("change", updateSystemTheme);
    return () => {
      systemPreference.removeEventListener("change", updateSystemTheme);
    };
  }, []);

  const resolvedTheme = appearance === "system" ? systemTheme : appearance;

  useEffect(() => {
    document.documentElement.dataset.appearance = appearance;
    document.documentElement.dataset.theme = resolvedTheme;
    window.localStorage.setItem(appearanceStorageKey, appearance);
  }, [appearance, resolvedTheme]);

  useEffect(() => {
    if (moviesFolder === null) {
      window.localStorage.removeItem(moviesFolderStorageKey);
    } else {
      window.localStorage.setItem(moviesFolderStorageKey, moviesFolder);
    }
  }, [moviesFolder]);

  useEffect(() => {
    const requestId = ++scanRequestId.current;

    if (moviesFolder === null) {
      setMovieScanState({ status: "unconfigured" });
      return;
    }

    setMovieScanState({ status: "scanning" });
    void window.__TAURI__.core
      .invoke<string[]>("scan_movies", { folder: moviesFolder })
      .then((paths) => {
        if (requestId !== scanRequestId.current) {
          return;
        }
        if (!Array.isArray(paths) || paths.some((path) => typeof path !== "string")) {
          throw new Error("The native scanner returned invalid movie paths.");
        }

        const movies = paths.map((path) => ({
          path,
          title: movieTitleFromPath(path),
        }));
        setMovieScanState(
          movies.length === 0
            ? { status: "empty" }
            : { status: "ready", movies },
        );
      })
      .catch((error: unknown) => {
        if (requestId !== scanRequestId.current) {
          return;
        }

        const errorCode =
          typeof error === "string"
            ? error
            : error instanceof Error
              ? error.message
              : "";
        setMovieScanState({
          status: errorCode === moviesFolderUnavailable ? "unavailable" : "error",
        });
      });

    return () => {
      scanRequestId.current += 1;
    };
  }, [moviesFolder, refreshVersion]);

  const moveNavigationFocus = (
    event: ReactKeyboardEvent<HTMLButtonElement>,
    currentIndex: number,
  ) => {
    let nextIndex: number;

    switch (event.key) {
      case "ArrowDown":
        nextIndex = (currentIndex + 1) % destinations.length;
        break;
      case "ArrowUp":
        nextIndex =
          (currentIndex - 1 + destinations.length) % destinations.length;
        break;
      case "Home":
        nextIndex = 0;
        break;
      case "End":
        nextIndex = destinations.length - 1;
        break;
      default:
        return;
    }

    event.preventDefault();
    navigationItems.current[nextIndex]?.focus();
  };

  const chooseMoviesFolder = async () => {
    setFolderSelectionError(null);
    setIsChoosingFolder(true);

    try {
      const selectedFolder = await open({
        directory: true,
        multiple: false,
        title: "Choose Movies folder",
      });

      if (selectedFolder === null) {
        return;
      }
      if (typeof selectedFolder !== "string") {
        throw new Error("The native folder picker returned an invalid path.");
      }

      scanRequestId.current += 1;
      setMovieScanState({ status: "scanning" });
      if (selectedFolder === moviesFolder) {
        setRefreshVersion((version) => version + 1);
      } else {
        setMoviesFolder(selectedFolder);
      }
    } catch {
      setFolderSelectionError("The Movies folder picker could not be opened.");
    } finally {
      setIsChoosingFolder(false);
    }
  };

  const clearMoviesFolder = () => {
    scanRequestId.current += 1;
    setFolderSelectionError(null);
    setMovieScanState({ status: "unconfigured" });
    setMoviesFolder(null);
  };

  const refreshMovies = () => {
    if (moviesFolder === null) {
      return;
    }

    scanRequestId.current += 1;
    setMovieScanState({ status: "scanning" });
    setRefreshVersion((version) => version + 1);
  };

  const currentMovieScanMessage =
    movieScanState.status === "ready"
      ? null
      : movieScanMessages[movieScanState.status];

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <span className="brand__mark">
            <AppIcon name="brand" />
          </span>
          <span>
            <strong>Auto-Video</strong>
            <small>Desktop workspace</small>
          </span>
        </div>

        <nav aria-label="Primary navigation" className="primary-navigation">
          <p className="navigation-label">Workspace</p>
          <ul>
            {destinations.map((destination, index) => {
              const isActive = destination.id === activeDestination.id;

              return (
                <li key={destination.id}>
                  <button
                    aria-current={isActive ? "page" : undefined}
                    className="navigation-item"
                    onClick={() => setActiveDestination(destination)}
                    onKeyDown={(event) => moveNavigationFocus(event, index)}
                    ref={(element) => {
                      navigationItems.current[index] = element;
                    }}
                    type="button"
                  >
                    <AppIcon name={destination.id} />
                    <span>{destination.label}</span>
                  </button>
                </li>
              );
            })}
          </ul>
        </nav>

        <p className="sidebar__status">Local desktop library</p>
      </aside>

      <main className="workspace">
        <div className="workspace__content">
          <header className="page-header">
            <p className="page-eyebrow">Auto-Video workspace</p>
            <h1>{activeDestination.label}</h1>
            <p>{activeDestination.description}</p>
          </header>

          {activeDestination.id === "library" ? (
            <section
              aria-busy={movieScanState.status === "scanning"}
              aria-labelledby="movies-heading"
              className="library-content"
            >
              <div className="library-toolbar">
                <div className="library-toolbar__heading">
                  <span className="empty-state__icon">
                    <AppIcon name="library" />
                  </span>
                  <div>
                    <p className="card-eyebrow">Local library</p>
                    <h2 id="movies-heading">Movies</h2>
                    <p className="library-folder">
                      {moviesFolder ?? "No Movies folder configured"}
                    </p>
                  </div>
                </div>
                {moviesFolder !== null ? (
                  <button
                    className="button button--secondary"
                    disabled={movieScanState.status === "scanning"}
                    onClick={refreshMovies}
                    type="button"
                  >
                    <AppIcon name="refresh" />
                    Refresh
                  </button>
                ) : null}
              </div>

              {movieScanState.status === "ready" ? (
                <ul aria-label="Movies" className="movie-grid">
                  {movieScanState.movies.map((movie) => (
                    <li key={movie.path}>
                      <article className="movie-card">
                        <span className="movie-card__icon">
                          <AppIcon name="movie" />
                        </span>
                        <h3>{movie.title}</h3>
                      </article>
                    </li>
                  ))}
                </ul>
              ) : (
                <div
                  className="empty-state library-state"
                  role={currentMovieScanMessage?.role}
                >
                  <span className="empty-state__icon">
                    <AppIcon name="library" />
                  </span>
                  <h2>{currentMovieScanMessage?.heading}</h2>
                  <p>{currentMovieScanMessage?.message}</p>
                </div>
              )}
            </section>
          ) : activeDestination.id === "settings" ? (
            <div className="settings-content">
              <section
                aria-labelledby="movies-folder-heading"
                className="settings-card"
              >
                <div className="settings-card__heading">
                  <span className="empty-state__icon">
                    <AppIcon name="folder" />
                  </span>
                  <div>
                    <h2 id="movies-folder-heading">Movies folder</h2>
                    <p>
                      Choose one local folder. Its path stays on this device and
                      is used only to scan for supported videos.
                    </p>
                  </div>
                </div>

                <div className="folder-setting">
                  {moviesFolder === null ? (
                    <p className="folder-setting__empty">
                      No Movies folder configured.
                    </p>
                  ) : (
                    <div>
                      <p className="field-label">Configured folder</p>
                      <p className="folder-path">{moviesFolder}</p>
                    </div>
                  )}
                  <div className="folder-setting__actions">
                    <button
                      className="button button--primary"
                      disabled={isChoosingFolder}
                      onClick={() => void chooseMoviesFolder()}
                      type="button"
                    >
                      <AppIcon name="folder" />
                      {isChoosingFolder
                        ? "Choosing…"
                        : moviesFolder === null
                          ? "Choose folder"
                          : "Change folder"}
                    </button>
                    {moviesFolder !== null ? (
                      <button
                        className="button button--secondary"
                        onClick={clearMoviesFolder}
                        type="button"
                      >
                        Clear folder
                      </button>
                    ) : null}
                  </div>
                  {folderSelectionError === null ? null : (
                    <p className="field-error" role="alert">
                      {folderSelectionError}
                    </p>
                  )}
                </div>
              </section>

              <section
                aria-labelledby="appearance-heading"
                className="settings-card"
              >
                <div className="settings-card__heading">
                  <span className="empty-state__icon">
                    <AppIcon name="settings" />
                  </span>
                  <div>
                    <h2 id="appearance-heading">Appearance</h2>
                    <p>
                      Choose a palette for this application. System follows the
                      operating-system preference.
                    </p>
                  </div>
                </div>

                <fieldset className="appearance-options">
                  <legend>Appearance mode</legend>
                  <div>
                    {appearanceModes.map((mode) => (
                      <label className="appearance-option" key={mode.id}>
                        <input
                          checked={appearance === mode.id}
                          name="appearance"
                          onChange={() => setAppearance(mode.id)}
                          type="radio"
                          value={mode.id}
                        />
                        <span className="appearance-option__content">
                          <AppIcon name={mode.id} />
                          <span>{mode.label}</span>
                        </span>
                      </label>
                    ))}
                  </div>
                </fieldset>
              </section>

              <section
                aria-labelledby="settings-empty-heading"
                className="empty-state empty-state--compact"
              >
                <h2 id="settings-empty-heading">
                  {activeDestination.emptyHeading}
                </h2>
                <p>{activeDestination.emptyMessage}</p>
              </section>
            </div>
          ) : (
            <section
              aria-labelledby={`${activeDestination.id}-empty-heading`}
              className="empty-state"
            >
              <span className="empty-state__icon">
                <AppIcon name={activeDestination.id} />
              </span>
              <h2 id={`${activeDestination.id}-empty-heading`}>
                {activeDestination.emptyHeading}
              </h2>
              <p>{activeDestination.emptyMessage}</p>
            </section>
          )}
        </div>
      </main>
    </div>
  );
}
