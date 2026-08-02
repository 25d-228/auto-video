import {
  ArrowClockwiseIcon,
  CheckIcon,
  CompassIcon,
  CopySimpleIcon,
  DownloadSimpleIcon,
  FilmSlateIcon,
  FilmStripIcon,
  FolderSimpleIcon,
  GearSixIcon,
  ImageSquareIcon,
  type Icon,
  KeyIcon,
  MonitorIcon,
  MoonIcon,
  PlayIcon,
  SquaresFourIcon,
  SunIcon,
  WarningCircleIcon,
} from "@phosphor-icons/react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  type FormEvent,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  useEffect,
  useRef,
  useState,
} from "react";

import tmdbLogo from "@/assets/tmdb-logo.svg";
import { Button } from "@/components/ui/button";
import {
  fetchWeeklyTrendingMovies,
  type TmdbMovie,
  type TmdbTrendingResult,
  tmdbPosterUrl,
} from "@/tmdb";

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
    description: "Browse TMDB's weekly trending Movies feed.",
    emptyHeading: "Discovery is not configured",
    emptyMessage:
      "Add a TMDB API Read Access Token in Settings to load weekly trending Movies.",
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
    description: "Configure TMDB, your local Movies folder, and appearance.",
    emptyHeading: "Other settings are not configured",
    emptyMessage:
      "Provider credentials and additional preferences will appear only with the features they control.",
  },
] as const;
const settingsDestination = destinations[4];

const appearanceModes = [
  { id: "light", label: "Light" },
  { id: "dark", label: "Dark" },
  { id: "system", label: "System" },
] as const;

const appIcons = {
  brand: PlayIcon,
  dashboard: SquaresFourIcon,
  discover: CompassIcon,
  library: FilmSlateIcon,
  downloads: DownloadSimpleIcon,
  settings: GearSixIcon,
  light: SunIcon,
  dark: MoonIcon,
  system: MonitorIcon,
  folder: FolderSimpleIcon,
  credential: KeyIcon,
  refresh: ArrowClockwiseIcon,
  movie: FilmStripIcon,
  poster: ImageSquareIcon,
  copy: CopySimpleIcon,
  copied: CheckIcon,
  "copy-error": WarningCircleIcon,
} satisfies Record<string, Icon>;

type AppearanceMode = (typeof appearanceModes)[number]["id"];
type IconName = keyof typeof appIcons;
type ResolvedTheme = Exclude<AppearanceMode, "system">;
type Movie = { path: string; title: string };
type MovieScanState =
  | { status: "unconfigured" }
  | { status: "scanning" }
  | { status: "empty" }
  | { status: "unavailable" }
  | { status: "error" }
  | { status: "ready"; movies: Movie[] };
type DiscoverState =
  | { status: "loading-credential" }
  | { status: "credential-error" }
  | { status: "unconfigured" }
  | { status: "loading" }
  | TmdbTrendingResult;
type CredentialMessage = {
  role: "alert" | "status";
  text: string;
};
type CopyTitleState = "idle" | "success" | "error";

const appearanceStorageKey = "auto-video-appearance";
const moviesFolderStorageKey = "auto-video-movies-folder";
const moviesFolderUnavailable = "movies_folder_unavailable";
const systemDarkModeQuery = "(prefers-color-scheme: dark)";
// Two seconds confirms a successful copy without leaving stale feedback on the card.
const copySuccessDuration = 2000;

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

const discoverMessages = {
  "loading-credential": {
    heading: "Loading TMDB configuration",
    message: "Checking for a locally saved TMDB token.",
    role: "status",
  },
  "credential-error": {
    heading: "TMDB configuration could not be loaded",
    message: "Open Settings to save the TMDB token again.",
    role: "alert",
  },
  unconfigured: {
    heading: "Configure TMDB to discover movies",
    message: "Add a TMDB API Read Access Token in Settings before loading the feed.",
    role: undefined,
  },
  loading: {
    heading: "Loading weekly trending Movies",
    message: "Requesting this week's Movies feed from TMDB.",
    role: "status",
  },
  empty: {
    heading: "No trending movies returned",
    message: "TMDB returned an empty weekly Movies feed. Try Refresh later.",
    role: undefined,
  },
  unauthorized: {
    heading: "TMDB token was not accepted",
    message: "Replace the API Read Access Token in Settings and try again.",
    role: "alert",
  },
  "rate-limited": {
    heading: "TMDB rate limit reached",
    message: "TMDB is temporarily limiting requests. Wait before trying Refresh.",
    role: "alert",
  },
  "network-error": {
    heading: "TMDB could not be reached",
    message: "Check the network connection and try Refresh.",
    role: "alert",
  },
  "provider-error": {
    heading: "TMDB could not load trending Movies",
    message: "TMDB returned an unexpected response. Try Refresh later.",
    role: "alert",
  },
} as const;

function AppIcon({ name }: { name: IconName }) {
  const IconComponent = appIcons[name];

  return (
    <IconComponent
      aria-hidden="true"
      className="app-icon"
      focusable="false"
      weight="regular"
    />
  );
}

function CopyTitleAction({ title }: { title: string }) {
  const [copyState, setCopyState] = useState<CopyTitleState>("idle");
  const resetFeedbackTimeout = useRef<number | null>(null);

  useEffect(
    () => () => {
      if (resetFeedbackTimeout.current !== null) {
        window.clearTimeout(resetFeedbackTimeout.current);
      }
    },
    [],
  );

  const copyTitle = async (event: ReactMouseEvent<HTMLButtonElement>) => {
    event.stopPropagation();

    if (resetFeedbackTimeout.current !== null) {
      window.clearTimeout(resetFeedbackTimeout.current);
      resetFeedbackTimeout.current = null;
    }
    setCopyState("idle");

    try {
      const clipboard = navigator.clipboard;
      if (
        clipboard === undefined ||
        typeof clipboard.writeText !== "function"
      ) {
        setCopyState("error");
        return;
      }

      await clipboard.writeText(title);
      setCopyState("success");
      resetFeedbackTimeout.current = window.setTimeout(() => {
        setCopyState("idle");
        resetFeedbackTimeout.current = null;
      }, copySuccessDuration);
    } catch {
      setCopyState("error");
    }
  };

  const accessibleLabel =
    copyState === "success"
      ? `Copied title: ${title}`
      : copyState === "error"
        ? `Copy failed for title: ${title}`
        : `Copy title: ${title}`;
  const visibleLabel =
    copyState === "success"
      ? "Copied"
      : copyState === "error"
        ? "Failed"
        : "Copy";
  const iconName =
    copyState === "success"
      ? "copied"
      : copyState === "error"
        ? "copy-error"
        : "copy";

  return (
    <>
      <Button
        aria-label={accessibleLabel}
        className="title-copy-button"
        data-copy-state={copyState}
        onClick={copyTitle}
        onKeyDown={(event) => event.stopPropagation()}
        onPointerDown={(event) => event.stopPropagation()}
        size="xs"
        type="button"
        variant="ghost"
      >
        <AppIcon name={iconName} />
        {visibleLabel}
      </Button>
      {copyState !== "idle" ? (
        <span
          aria-atomic="true"
          className="sr-only"
          role={copyState === "error" ? "alert" : "status"}
        >
          {accessibleLabel}
        </span>
      ) : null}
    </>
  );
}

function DiscoverMovieCard({
  movie,
  resultIndex,
}: {
  movie: TmdbMovie;
  resultIndex: number;
}) {
  const [posterFailed, setPosterFailed] = useState(false);
  const titleId = `tmdb-movie-${movie.id}-${resultIndex}`;

  return (
    <article aria-labelledby={titleId} className="discover-card">
      <div className="discover-card__poster">
        {movie.posterPath !== null && !posterFailed ? (
          <img
            alt=""
            onError={() => setPosterFailed(true)}
            src={tmdbPosterUrl(movie.posterPath)}
          />
        ) : (
          <div className="discover-card__poster-fallback">
            <AppIcon name="poster" />
            <span>Poster unavailable</span>
          </div>
        )}
      </div>
      <div className="discover-card__body">
        <div className="media-title-row">
          <h3 id={titleId}>{movie.title}</h3>
          <CopyTitleAction title={movie.title} />
        </div>
        <dl>
          <div>
            <dt>Release</dt>
            <dd>{movie.releaseDate ?? "Unavailable"}</dd>
          </div>
          <div>
            <dt>Source</dt>
            <dd>TMDB</dd>
          </div>
        </dl>
      </div>
    </article>
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
  const [movieRefreshVersion, setMovieRefreshVersion] = useState(0);
  const [isChoosingFolder, setIsChoosingFolder] = useState(false);
  const [folderSelectionError, setFolderSelectionError] = useState<
    string | null
  >(null);
  const [tmdbToken, setTmdbToken] = useState<string | null>(null);
  const [isTmdbTokenLoaded, setIsTmdbTokenLoaded] = useState(false);
  const [tmdbCredentialLoadFailed, setTmdbCredentialLoadFailed] =
    useState(false);
  const [tmdbTokenInput, setTmdbTokenInput] = useState("");
  const [isSavingTmdbToken, setIsSavingTmdbToken] = useState(false);
  const [tmdbCredentialMessage, setTmdbCredentialMessage] =
    useState<CredentialMessage | null>(null);
  const [discoverState, setDiscoverState] = useState<DiscoverState>({
    status: "loading-credential",
  });
  const [discoverRefreshVersion, setDiscoverRefreshVersion] = useState(0);
  const navigationItems = useRef<Array<HTMLButtonElement | null>>([]);
  const workspace = useRef<HTMLElement | null>(null);
  const scanRequestId = useRef(0);
  const discoverRequestId = useRef(0);

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
    let isCurrent = true;

    void window.__TAURI__.core
      .invoke<string | null>("load_tmdb_token")
      .then((savedToken) => {
        if (!isCurrent) {
          return;
        }
        if (
          savedToken !== null &&
          (typeof savedToken !== "string" || savedToken === "")
        ) {
          throw new Error("The native credential store returned invalid data.");
        }

        setTmdbToken(savedToken);
        setTmdbCredentialLoadFailed(false);
      })
      .catch(() => {
        if (isCurrent) {
          setTmdbToken(null);
          setTmdbCredentialLoadFailed(true);
        }
      })
      .finally(() => {
        if (isCurrent) {
          setIsTmdbTokenLoaded(true);
        }
      });

    return () => {
      isCurrent = false;
    };
  }, []);

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
  }, [moviesFolder, movieRefreshVersion]);

  useEffect(() => {
    const requestId = ++discoverRequestId.current;

    if (activeDestination.id !== "discover") {
      return;
    }
    if (!isTmdbTokenLoaded) {
      setDiscoverState({ status: "loading-credential" });
      return;
    }
    if (tmdbCredentialLoadFailed) {
      setDiscoverState({ status: "credential-error" });
      return;
    }
    if (tmdbToken === null) {
      setDiscoverState({ status: "unconfigured" });
      return;
    }

    const abortController = new AbortController();
    setDiscoverState({ status: "loading" });
    void fetchWeeklyTrendingMovies(tmdbToken, abortController.signal).then(
      (result) => {
        if (requestId === discoverRequestId.current) {
          setDiscoverState(result);
        }
      },
    );

    return () => {
      discoverRequestId.current += 1;
      abortController.abort();
    };
  }, [
    activeDestination.id,
    discoverRefreshVersion,
    isTmdbTokenLoaded,
    tmdbCredentialLoadFailed,
    tmdbToken,
  ]);

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

  const navigateTo = (destination: (typeof destinations)[number]) => {
    setActiveDestination(destination);
    if (workspace.current !== null) {
      workspace.current.scrollTop = 0;
    }
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
        setMovieRefreshVersion((version) => version + 1);
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
    setMovieRefreshVersion((version) => version + 1);
  };

  const saveTmdbToken = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const token = tmdbTokenInput.trim();
    if (token === "") {
      return;
    }

    const previousToken = tmdbToken;
    discoverRequestId.current += 1;
    setDiscoverState({ status: "loading" });
    setTmdbToken(null);
    setIsSavingTmdbToken(true);
    setTmdbCredentialMessage(null);

    try {
      await window.__TAURI__.core.invoke("save_tmdb_token", { token });
      setTmdbToken(token);
      setIsTmdbTokenLoaded(true);
      setTmdbCredentialLoadFailed(false);
      setTmdbTokenInput("");
      setTmdbCredentialMessage({
        role: "status",
        text:
          previousToken === null ? "TMDB token saved." : "TMDB token replaced.",
      });
    } catch {
      setTmdbToken(previousToken);
      setDiscoverRefreshVersion((version) => version + 1);
      setTmdbCredentialMessage({
        role: "alert",
        text: "The TMDB token could not be saved on this device.",
      });
    } finally {
      setIsSavingTmdbToken(false);
    }
  };

  const clearTmdbToken = async () => {
    const tokenToRestore = tmdbToken;
    discoverRequestId.current += 1;
    setDiscoverState({ status: "unconfigured" });
    setTmdbToken(null);
    setIsSavingTmdbToken(true);
    setTmdbCredentialMessage(null);

    try {
      await window.__TAURI__.core.invoke("clear_tmdb_token");
      setTmdbToken(null);
      setIsTmdbTokenLoaded(true);
      setTmdbCredentialLoadFailed(false);
      setTmdbTokenInput("");
      setTmdbCredentialMessage({
        role: "status",
        text: "TMDB token cleared.",
      });
    } catch {
      setTmdbToken(tokenToRestore);
      setDiscoverRefreshVersion((version) => version + 1);
      setTmdbCredentialMessage({
        role: "alert",
        text: "The TMDB token could not be cleared from this device.",
      });
    } finally {
      setIsSavingTmdbToken(false);
    }
  };

  const refreshDiscover = () => {
    if (tmdbToken === null) {
      return;
    }

    discoverRequestId.current += 1;
    setDiscoverState({ status: "loading" });
    setDiscoverRefreshVersion((version) => version + 1);
  };

  const currentMovieScanMessage =
    movieScanState.status === "ready"
      ? null
      : movieScanMessages[movieScanState.status];
  const currentDiscoverMessage =
    discoverState.status === "ready"
      ? null
      : discoverMessages[discoverState.status];

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
                  <Button
                    aria-current={isActive ? "page" : undefined}
                    className="navigation-item"
                    onClick={() => navigateTo(destination)}
                    onKeyDown={(event) => moveNavigationFocus(event, index)}
                    ref={(element) => {
                      navigationItems.current[index] = element;
                    }}
                    type="button"
                    variant="ghost"
                  >
                    <AppIcon name={destination.id} />
                    <span>{destination.label}</span>
                  </Button>
                </li>
              );
            })}
          </ul>
        </nav>

        <p className="sidebar__status">Local desktop library</p>
      </aside>

      <main className="workspace" ref={workspace}>
        <div className="workspace__content">
          <header className="page-header">
            <p className="page-eyebrow">Auto-Video workspace</p>
            <h1>{activeDestination.label}</h1>
            <p>{activeDestination.description}</p>
          </header>

          {activeDestination.id === "discover" ? (
            <section
              aria-busy={discoverState.status === "loading"}
              aria-labelledby="discover-movies-heading"
              className="discover-content"
            >
              <div className="library-toolbar">
                <div className="library-toolbar__heading">
                  <span className="empty-state__icon">
                    <AppIcon name="discover" />
                  </span>
                  <div>
                    <p className="card-eyebrow">TMDB Discover</p>
                    <h2 id="discover-movies-heading">Weekly trending Movies</h2>
                    <p className="library-folder">Weekly Movies feed</p>
                  </div>
                </div>
                {isTmdbTokenLoaded &&
                !tmdbCredentialLoadFailed &&
                tmdbToken !== null ? (
                  <Button
                    onClick={refreshDiscover}
                    type="button"
                    variant="outline"
                  >
                    <AppIcon name="refresh" />
                    Refresh
                  </Button>
                ) : null}
              </div>

              {discoverState.status === "ready" ? (
                <ul aria-label="Weekly trending Movies" className="discover-grid">
                  {discoverState.movies.map((movie, resultIndex) => (
                    <li
                      key={`${movie.id}-${resultIndex}-${movie.posterPath ?? "posterless"}`}
                    >
                      <DiscoverMovieCard
                        movie={movie}
                        resultIndex={resultIndex}
                      />
                    </li>
                  ))}
                </ul>
              ) : (
                <div
                  className="empty-state discover-state"
                  role={currentDiscoverMessage?.role}
                >
                  <span className="empty-state__icon">
                    <AppIcon name="discover" />
                  </span>
                  <h2>{currentDiscoverMessage?.heading}</h2>
                  <p>{currentDiscoverMessage?.message}</p>
                  {discoverState.status === "unconfigured" ||
                  discoverState.status === "credential-error" ||
                  discoverState.status === "unauthorized" ? (
                    <Button
                      className="empty-state__action"
                      onClick={() => navigateTo(settingsDestination)}
                      type="button"
                    >
                      Open Settings
                    </Button>
                  ) : null}
                </div>
              )}

              <footer aria-label="TMDB credits" className="tmdb-attribution">
                <img alt="TMDB" src={tmdbLogo} />
                <p>
                  This product uses the TMDB API but is not endorsed or certified
                  by TMDB.
                </p>
              </footer>
            </section>
          ) : activeDestination.id === "library" ? (
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
                  <Button
                    disabled={movieScanState.status === "scanning"}
                    onClick={refreshMovies}
                    type="button"
                    variant="outline"
                  >
                    <AppIcon name="refresh" />
                    Refresh
                  </Button>
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
                        <div className="media-title-row">
                          <h3>{movie.title}</h3>
                          <CopyTitleAction title={movie.title} />
                        </div>
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
                aria-labelledby="tmdb-token-heading"
                className="settings-card"
              >
                <div className="settings-card__heading">
                  <span className="empty-state__icon">
                    <AppIcon name="credential" />
                  </span>
                  <div>
                    <h2 id="tmdb-token-heading">TMDB API Read Access Token</h2>
                    <p>
                      Save one token locally for the weekly Movies feed. The
                      saved value is never shown.
                    </p>
                  </div>
                </div>

                <form className="credential-setting" onSubmit={saveTmdbToken}>
                  <p
                    className={
                      tmdbCredentialLoadFailed
                        ? "field-error credential-setting__status"
                        : "credential-setting__status"
                    }
                    id="tmdb-token-status"
                    role={tmdbCredentialLoadFailed ? "alert" : undefined}
                  >
                    {!isTmdbTokenLoaded
                      ? "Loading saved token…"
                      : tmdbCredentialLoadFailed
                        ? "The saved TMDB token could not be read. Save it again."
                        : tmdbToken === null
                          ? "No TMDB token configured."
                          : "TMDB token configured on this device."}
                  </p>
                  <label className="field-label" htmlFor="tmdb-token">
                    {tmdbToken === null ? "Token" : "New token"}
                  </label>
                  <input
                    aria-describedby="tmdb-token-help tmdb-token-status"
                    autoComplete="off"
                    className="credential-input"
                    id="tmdb-token"
                    onChange={(event) => setTmdbTokenInput(event.target.value)}
                    spellCheck={false}
                    type="password"
                    value={tmdbTokenInput}
                  />
                  <p className="field-help" id="tmdb-token-help">
                    Use the API Read Access Token from TMDB account settings.
                  </p>
                  <div className="folder-setting__actions">
                    <Button
                      disabled={
                        isSavingTmdbToken || tmdbTokenInput.trim() === ""
                      }
                      type="submit"
                    >
                      <AppIcon name="credential" />
                      {isSavingTmdbToken
                        ? "Saving…"
                        : tmdbToken === null
                          ? "Save token"
                          : "Replace token"}
                    </Button>
                    {tmdbToken !== null ? (
                      <Button
                        disabled={isSavingTmdbToken}
                        onClick={() => void clearTmdbToken()}
                        type="button"
                        variant="outline"
                      >
                        Clear token
                      </Button>
                    ) : null}
                  </div>
                  {tmdbCredentialMessage === null ? null : (
                    <p
                      className={
                        tmdbCredentialMessage.role === "alert"
                          ? "field-error"
                          : "field-success"
                      }
                      role={tmdbCredentialMessage.role}
                    >
                      {tmdbCredentialMessage.text}
                    </p>
                  )}
                </form>
              </section>

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
                    <Button
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
                    </Button>
                    {moviesFolder !== null ? (
                      <Button
                        onClick={clearMoviesFolder}
                        type="button"
                        variant="outline"
                      >
                        Clear folder
                      </Button>
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
