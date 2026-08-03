import { AlertDialog } from "@base-ui/react/alert-dialog";
import { Dialog } from "@base-ui/react/dialog";
import {
  ArrowClockwiseIcon,
  CheckIcon,
  CompassIcon,
  CopySimpleIcon,
  DownloadSimpleIcon,
  FilmSlateIcon,
  FilmStripIcon,
  FolderOpenIcon,
  FolderSimpleIcon,
  GearSixIcon,
  ImageSquareIcon,
  InfoIcon,
  type Icon,
  KeyIcon,
  MagnifyingGlassIcon,
  MonitorIcon,
  MoonIcon,
  PlayIcon,
  SquaresFourIcon,
  SunIcon,
  TrashIcon,
  WarningCircleIcon,
  XIcon,
} from "@phosphor-icons/react";
import {
  type CSSProperties,
  type FormEvent,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  type ReactNode,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
} from "react";

import tmdbLogo from "@/assets/tmdb-logo.svg";
import { Button } from "@/components/ui/button";
import {
  fetchTmdbMovieDetails,
  fetchTmdbMoviesByTitle,
  fetchWeeklyTrendingMovies,
  type TmdbMovie,
  type TmdbMovieDetailsResult,
  type TmdbMoviesResult,
  tmdbPosterUrl,
} from "@/tmdb";

import "./index.css";

const destinations = [
  {
    id: "dashboard",
    label: "Dashboard",
    description: "Current status for your local Movies Library.",
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
const libraryDestination = destinations[2];
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
  open: PlayIcon,
  reveal: FolderOpenIcon,
  trash: TrashIcon,
  close: XIcon,
  poster: ImageSquareIcon,
  copy: CopySimpleIcon,
  copied: CheckIcon,
  "copy-error": WarningCircleIcon,
  search: MagnifyingGlassIcon,
  details: InfoIcon,
} satisfies Record<string, Icon>;

type AppearanceMode = (typeof appearanceModes)[number]["id"];
type IconName = keyof typeof appIcons;
type ResolvedTheme = Exclude<AppearanceMode, "system">;
type Movie = { path: string; title: string };
type LibraryTitleSortDirection = "ascending" | "descending";
type MovieScanState =
  | { status: "unconfigured" }
  | { status: "scanning" }
  | { status: "empty" }
  | { status: "unavailable" }
  | { status: "error" }
  | { status: "ready"; movies: Movie[] };
type MoviesStorageState =
  | { status: "unconfigured" }
  | { status: "loading" }
  | { status: "unavailable" }
  | { status: "error" }
  | { status: "ready"; totalBytes: bigint; freeBytes: bigint };
type DiscoverState =
  | { status: "loading-credential" }
  | { status: "credential-error" }
  | { status: "unconfigured" }
  | { status: "loading" }
  | TmdbMoviesResult;
type MovieDetailsState =
  | { status: "loading" }
  | TmdbMovieDetailsResult;
type CredentialMessage = {
  role: "alert" | "status";
  text: string;
};
type CopyTitleState = "idle" | "success" | "error";
type GalleryVariant = "discover" | "library";
type GalleryLayout = {
  capacity: number;
  columns: number;
  rowHeight: number;
};

const appearanceStorageKey = "auto-video-appearance";
const moviesFolderUnavailable = "movies_folder_unavailable";
const moviesStorageUnavailable = "movies_storage_unavailable";
const systemDarkModeQuery = "(prefers-color-scheme: dark)";
// Two seconds confirms a successful copy without leaving stale feedback on the card.
const copySuccessDuration = 2000;
// These pixel values mirror the current 0.75rem gap and 13rem minimum card width.
const galleryGap = 12;
const minimumGalleryCardWidth = 208;
// Fixed title limits keep every calculated row within its observed viewport.
const discoverCardBodyHeight = 160;
const libraryCardHeight = 136;

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

const movieOpenErrorMessages: Record<string, string> = {
  movie_open_not_found: "This movie is no longer available.",
  movie_open_unavailable: "Auto-Video could not access this movie.",
  movie_open_not_file: "This item is not an eligible video file.",
  movie_open_unsupported: "This item is not a supported .mp4 or .mkv file.",
  movie_open_failed: "The operating system could not open this movie.",
};
const movieOpenFallbackMessage = "Auto-Video could not open this movie.";

const movieRevealErrorMessages: Record<string, string> = {
  movie_reveal_not_found: "This movie is no longer available.",
  movie_reveal_unavailable: "Auto-Video could not access this movie.",
  movie_reveal_not_file: "This item is not an eligible video file.",
  movie_reveal_unsupported: "This item is not a supported .mp4 or .mkv file.",
  movie_reveal_failed: "The operating system could not reveal this movie.",
};
const movieRevealFallbackMessage = "Auto-Video could not reveal this movie.";

const movieTrashErrorMessages: Record<string, string> = {
  movie_trash_not_found: "This movie is no longer available.",
  movie_trash_unavailable: "Auto-Video could not access this movie.",
  movie_trash_not_file: "This item is not an eligible video file.",
  movie_trash_unsupported: "This item is not a supported .mp4 or .mkv file.",
  movie_trash_folder_unavailable:
    "The configured Movies folder is no longer available.",
  movie_trash_outside_folder:
    "This movie is outside the configured Movies folder.",
  movie_trash_stale: "This movie is no longer part of the current Library.",
  movie_trash_failed:
    "The operating system could not move this movie to Trash or the Recycle Bin.",
};
const movieTrashFallbackMessage =
  "Auto-Video could not move this movie to Trash or the Recycle Bin.";

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
  "malformed-provider": {
    heading: "TMDB returned invalid Movies data",
    message: "TMDB returned an unexpected response. Try Refresh later.",
    role: "alert",
  },
  "provider-error": {
    heading: "TMDB could not load trending Movies",
    message: "TMDB returned an unexpected response. Try Refresh later.",
    role: "alert",
  },
} as const;

const discoverSearchMessages = {
  ...discoverMessages,
  loading: {
    heading: "Searching TMDB Movies",
    message: "Requesting Movies title matches from TMDB.",
    role: "status",
  },
  empty: {
    heading: "No TMDB Movies match this search",
    message: "TMDB returned no Movies for the submitted title search.",
    role: undefined,
  },
  "network-error": {
    heading: "TMDB search could not be reached",
    message: "Check the network connection and try Refresh.",
    role: "alert",
  },
  "malformed-provider": {
    heading: "TMDB returned invalid search data",
    message: "TMDB returned a malformed Movies search response.",
    role: "alert",
  },
  "provider-error": {
    heading: "TMDB could not search Movies",
    message: "TMDB returned an unexpected error. Try Refresh later.",
    role: "alert",
  },
} as const;

const movieDetailsMessages = {
  loading: {
    heading: "Loading Movie details",
    message: "Requesting the selected Movie details from TMDB.",
    role: "status",
  },
  unauthorized: {
    heading: "TMDB token was not accepted",
    message: "Replace the API Read Access Token in Settings and try again.",
    role: "alert",
  },
  "rate-limited": {
    heading: "TMDB details rate limit reached",
    message: "TMDB is temporarily limiting requests. Wait before trying again.",
    role: "alert",
  },
  "network-error": {
    heading: "TMDB Movie details could not be reached",
    message: "Check the network connection and try View details again.",
    role: "alert",
  },
  "malformed-provider": {
    heading: "TMDB returned invalid Movie details",
    message: "The response did not verify the selected TMDB Movie identity.",
    role: "alert",
  },
  "provider-error": {
    heading: "TMDB could not load Movie details",
    message: "TMDB returned an unexpected error. Try again later.",
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

function calculateGalleryLayout(
  variant: GalleryVariant,
  width: number,
  height: number,
): GalleryLayout {
  const columns = Math.max(
    1,
    Math.floor(
      (width + galleryGap) / (minimumGalleryCardWidth + galleryGap),
    ),
  );
  const cardWidth = Math.max(
    0,
    (width - galleryGap * (columns - 1)) / columns,
  );
  const rowHeight =
    variant === "discover"
      ? cardWidth * 1.5 + discoverCardBodyHeight
      : libraryCardHeight;
  const rows = Math.max(
    1,
    Math.floor((height + galleryGap) / (rowHeight + galleryGap)),
  );

  return { capacity: columns * rows, columns, rowHeight };
}

function ResizeAwareGallery<Item>({
  ariaLabel,
  getItemKey,
  items,
  onSelectedPageChange,
  renderItem,
  selectedPage,
  variant,
}: {
  ariaLabel: string;
  getItemKey: (item: Item, index: number) => string;
  items: Item[];
  onSelectedPageChange: (page: number) => void;
  renderItem: (item: Item, index: number) => ReactNode;
  selectedPage: number;
  variant: GalleryVariant;
}) {
  const viewport = useRef<HTMLDivElement | null>(null);
  const [layout, setLayout] = useState<GalleryLayout>(() =>
    calculateGalleryLayout(variant, minimumGalleryCardWidth, 1),
  );

  useLayoutEffect(() => {
    const galleryViewport = viewport.current;
    if (galleryViewport === null) {
      return;
    }

    const updateLayout = (width: number, height: number) => {
      if (width <= 0 || height <= 0) {
        return;
      }

      const nextLayout = calculateGalleryLayout(variant, width, height);
      setLayout((currentLayout) =>
        currentLayout.capacity === nextLayout.capacity &&
        currentLayout.columns === nextLayout.columns &&
        currentLayout.rowHeight === nextLayout.rowHeight
          ? currentLayout
          : nextLayout,
      );
    };
    const resizeObserver = new ResizeObserver((entries) => {
      const entry = entries.find(
        ({ target }) => target === galleryViewport,
      );
      if (entry !== undefined) {
        updateLayout(entry.contentRect.width, entry.contentRect.height);
      }
    });
    const initialBounds = galleryViewport.getBoundingClientRect();

    updateLayout(initialBounds.width, initialBounds.height);
    resizeObserver.observe(galleryViewport);
    return () => resizeObserver.disconnect();
  }, [variant]);

  const pageCount = Math.max(1, Math.ceil(items.length / layout.capacity));
  const currentPage = Math.min(selectedPage, pageCount);
  const firstVisibleIndex = (currentPage - 1) * layout.capacity;
  const visibleItems = items.slice(
    firstVisibleIndex,
    firstVisibleIndex + layout.capacity,
  );
  const gridStyle = {
    "--gallery-columns": layout.columns,
    "--gallery-row-height": `${layout.rowHeight}px`,
  } as CSSProperties;

  useLayoutEffect(() => {
    if (selectedPage !== currentPage) {
      onSelectedPageChange(currentPage);
    }
  }, [currentPage, onSelectedPageChange, selectedPage]);

  return (
    <div
      className={`media-gallery media-gallery--${variant}`}
      data-current-page={currentPage}
      data-gallery={variant}
      data-page-capacity={layout.capacity}
      data-page-count={pageCount}
    >
      <div className="media-gallery__viewport" ref={viewport}>
        <ul
          aria-label={ariaLabel}
          className={`media-grid ${variant === "discover" ? "discover-grid" : "movie-grid"}`}
          style={gridStyle}
        >
          {visibleItems.map((item, visibleIndex) => {
            const itemIndex = firstVisibleIndex + visibleIndex;
            return (
              <li key={getItemKey(item, itemIndex)}>
                {renderItem(item, itemIndex)}
              </li>
            );
          })}
        </ul>
      </div>
      <nav
        aria-label={`${ariaLabel} pagination`}
        className="media-pagination"
      >
        <Button
          aria-label={`Previous ${ariaLabel} page`}
          disabled={currentPage === 1}
          onClick={() => onSelectedPageChange(currentPage - 1)}
          size="sm"
          type="button"
          variant="outline"
        >
          Previous
        </Button>
        <p aria-atomic="true" aria-live="polite">
          Page {currentPage} of {pageCount}
        </p>
        <Button
          aria-label={`Next ${ariaLabel} page`}
          disabled={currentPage === pageCount}
          onClick={() => onSelectedPageChange(currentPage + 1)}
          size="sm"
          type="button"
          variant="outline"
        >
          Next
        </Button>
      </nav>
    </div>
  );
}

function DiscoverMovieCard({
  movie,
  onViewDetails,
  resultIndex,
}: {
  movie: TmdbMovie;
  onViewDetails: (movie: TmdbMovie, triggerId: string) => void;
  resultIndex: number;
}) {
  const [posterFailed, setPosterFailed] = useState(false);
  const detailsTriggerId = useId();
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
          <div className="discover-card__title-actions">
            <CopyTitleAction title={movie.title} />
            <Button
              aria-label={`View details: ${movie.title}`}
              className="discover-card__details-action"
              id={detailsTriggerId}
              onClick={(event) => {
                event.stopPropagation();
                onViewDetails(movie, detailsTriggerId);
              }}
              onKeyDown={(event) => event.stopPropagation()}
              onPointerDown={(event) => event.stopPropagation()}
              size="xs"
              title="View details"
              type="button"
              variant="outline"
            >
              <AppIcon name="details" />
              Details
            </Button>
          </div>
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

function DiscoverMovieDetails({
  movie,
  state,
  triggerId,
}: {
  movie: TmdbMovie;
  state: MovieDetailsState;
  triggerId: string;
}) {
  const [failedPosterPath, setFailedPosterPath] = useState<string | null>(null);
  const details = state.status === "ready" ? state.details : null;
  const displayedTitle = details?.title ?? movie.title;
  const currentMessage =
    state.status === "ready" ? null : movieDetailsMessages[state.status];

  return (
    <Dialog.Portal>
      <Dialog.Backdrop className="movie-details__backdrop" />
      <Dialog.Viewport className="movie-details__viewport">
        <Dialog.Popup
          aria-busy={state.status === "loading"}
          className="movie-details__popup"
          finalFocus={() => document.getElementById(triggerId)}
          onClick={(event) => event.stopPropagation()}
          onPointerDown={(event) => event.stopPropagation()}
        >
          <div className="movie-details__heading">
            <div>
              <p className="card-eyebrow">TMDB Movie details</p>
              <Dialog.Title>{displayedTitle}</Dialog.Title>
            </div>
            <Dialog.Close
              render={
                <Button type="button" variant="ghost">
                  <AppIcon name="close" />
                  Close
                </Button>
              }
            />
          </div>
          <Dialog.Description className="movie-details__description">
            Provider details for the selected TMDB Movie.
          </Dialog.Description>

          {details === null ? (
            <div
              className="movie-details__state"
              role={currentMessage?.role}
            >
              <span className="empty-state__icon">
                <AppIcon name="details" />
              </span>
              <div>
                <h3>{currentMessage?.heading}</h3>
                <p>{currentMessage?.message}</p>
              </div>
            </div>
          ) : (
            <div className="movie-details__content">
              <div className="movie-details__poster">
                {details.posterPath !== null &&
                failedPosterPath !== details.posterPath ? (
                  <img
                    alt=""
                    onError={() => setFailedPosterPath(details.posterPath)}
                    src={tmdbPosterUrl(details.posterPath)}
                  />
                ) : (
                  <div className="discover-card__poster-fallback">
                    <AppIcon name="poster" />
                    <span>Poster unavailable</span>
                  </div>
                )}
              </div>
              <div className="movie-details__information">
                <dl>
                  <div>
                    <dt>Release date</dt>
                    <dd>{details.releaseDate ?? "Unavailable"}</dd>
                  </div>
                  <div>
                    <dt>Runtime</dt>
                    <dd>
                      {details.runtimeMinutes === null
                        ? "Unavailable"
                        : `${details.runtimeMinutes} minutes`}
                    </dd>
                  </div>
                  <div>
                    <dt>Genres</dt>
                    <dd>
                      {details.genres.length === 0
                        ? "Unavailable"
                        : details.genres.join(", ")}
                    </dd>
                  </div>
                </dl>
                <div className="movie-details__overview">
                  <h3>Overview</h3>
                  <p>{details.overview ?? "Unavailable"}</p>
                </div>
              </div>
            </div>
          )}
        </Dialog.Popup>
      </Dialog.Viewport>
    </Dialog.Portal>
  );
}

function LibraryMovieCard({
  folder,
  movie,
  onMovieTrashed,
}: {
  folder: string;
  movie: Movie;
  onMovieTrashed: (movie: Movie, folder: string) => void;
}) {
  const [isOpening, setIsOpening] = useState(false);
  const [isRevealing, setIsRevealing] = useState(false);
  const [isTrashing, setIsTrashing] = useState(false);
  const [openError, setOpenError] = useState<string | null>(null);
  const [revealError, setRevealError] = useState<string | null>(null);
  const [trashError, setTrashError] = useState<string | null>(null);
  const [trashDialogOpen, setTrashDialogOpen] = useState(false);
  const openRequestPending = useRef(false);
  const revealRequestPending = useRef(false);
  const trashRequestPending = useRef(false);
  const trashCancelButton = useRef<HTMLButtonElement | null>(null);
  const trashDialogPopup = useRef<HTMLDivElement | null>(null);
  const trashTriggerId = useId();

  const openMovie = async (event: ReactMouseEvent<HTMLButtonElement>) => {
    event.stopPropagation();
    if (openRequestPending.current) {
      return;
    }

    openRequestPending.current = true;
    setIsOpening(true);
    setOpenError(null);

    try {
      await window.__TAURI__.core.invoke("open_movie", { path: movie.path });
    } catch (error: unknown) {
      const errorCode =
        typeof error === "string"
          ? error
          : error instanceof Error
            ? error.message
            : "";
      setOpenError(
        movieOpenErrorMessages[errorCode] ?? movieOpenFallbackMessage,
      );
    } finally {
      openRequestPending.current = false;
      setIsOpening(false);
    }
  };

  const revealMovie = async (event: ReactMouseEvent<HTMLButtonElement>) => {
    event.stopPropagation();
    if (revealRequestPending.current) {
      return;
    }

    revealRequestPending.current = true;
    setIsRevealing(true);
    setRevealError(null);

    try {
      await window.__TAURI__.core.invoke("reveal_movie", { path: movie.path });
    } catch (error: unknown) {
      const errorCode =
        typeof error === "string"
          ? error
          : error instanceof Error
            ? error.message
            : "";
      setRevealError(
        movieRevealErrorMessages[errorCode] ?? movieRevealFallbackMessage,
      );
    } finally {
      revealRequestPending.current = false;
      setIsRevealing(false);
    }
  };

  const updateTrashDialog = (open: boolean) => {
    if (!open && trashRequestPending.current) {
      return;
    }

    setTrashDialogOpen(open);
    if (open) {
      setTrashError(null);
    }
  };

  const trashMovie = async (event: ReactMouseEvent<HTMLButtonElement>) => {
    event.stopPropagation();
    if (trashRequestPending.current) {
      return;
    }

    trashRequestPending.current = true;
    setIsTrashing(true);
    setTrashError(null);
    trashDialogPopup.current?.focus();

    try {
      await window.__TAURI__.core.invoke("trash_movie", {
        path: movie.path,
      });
      onMovieTrashed(movie, folder);
    } catch (error: unknown) {
      const errorCode =
        typeof error === "string"
          ? error
          : error instanceof Error
            ? error.message
            : "";
      setTrashError(
        movieTrashErrorMessages[errorCode] ?? movieTrashFallbackMessage,
      );
    } finally {
      trashRequestPending.current = false;
      setIsTrashing(false);
    }
  };

  const fileActionErrorCount =
    Number(openError !== null) + Number(revealError !== null);

  return (
    <article
      className="movie-card"
      data-file-action-errors={fileActionErrorCount}
      data-open-state={
        openError === null ? (isOpening ? "pending" : "idle") : "error"
      }
      data-reveal-state={
        revealError === null ? (isRevealing ? "pending" : "idle") : "error"
      }
    >
      <div className="movie-card__header">
        <span className="movie-card__icon">
          <AppIcon name="movie" />
        </span>
        <div className="movie-card__actions">
          <Button
            aria-label={`${isOpening ? "Opening" : "Open"} movie: ${movie.title}`}
            disabled={isOpening}
            onClick={openMovie}
            onKeyDown={(event) => event.stopPropagation()}
            onPointerDown={(event) => event.stopPropagation()}
            size="xs"
            type="button"
            variant="outline"
          >
            <AppIcon name="open" />
            {isOpening ? "Opening" : "Open"}
          </Button>
          <Button
            aria-label={`${isRevealing ? "Revealing" : "Reveal"} movie: ${movie.title}`}
            disabled={isRevealing}
            onClick={revealMovie}
            onKeyDown={(event) => event.stopPropagation()}
            onPointerDown={(event) => event.stopPropagation()}
            size="xs"
            type="button"
            variant="outline"
          >
            <AppIcon name="reveal" />
            {isRevealing ? "Revealing" : "Reveal"}
          </Button>
        </div>
      </div>
      <div className="media-title-row">
        <h3>{movie.title}</h3>
        <div className="movie-card__title-actions">
          <AlertDialog.Root
            onOpenChange={updateTrashDialog}
            open={trashDialogOpen}
            triggerId={trashDialogOpen ? trashTriggerId : null}
          >
            <AlertDialog.Trigger
              id={trashTriggerId}
              render={
                <Button
                  aria-label={`Move movie to Trash or Recycle Bin: ${movie.title}`}
                  disabled={isTrashing}
                  onClick={(event) => event.stopPropagation()}
                  onKeyDown={(event) => event.stopPropagation()}
                  onPointerDown={(event) => event.stopPropagation()}
                  size="icon-xs"
                  title="Move to Trash or Recycle Bin"
                  type="button"
                  variant="destructive"
                >
                  <AppIcon name="trash" />
                </Button>
              }
            />
            <AlertDialog.Portal>
              <AlertDialog.Backdrop
                className="trash-dialog__backdrop"
                onClick={(event) => {
                  event.stopPropagation();
                  updateTrashDialog(false);
                }}
                onPointerDown={(event) => event.stopPropagation()}
              />
              <AlertDialog.Viewport className="trash-dialog__viewport">
                <AlertDialog.Popup
                  aria-busy={isTrashing}
                  className="trash-dialog__popup"
                  initialFocus={() => trashCancelButton.current}
                  onClick={(event) => event.stopPropagation()}
                  onPointerDown={(event) => event.stopPropagation()}
                  ref={trashDialogPopup}
                >
                  <div className="trash-dialog__heading">
                    <AlertDialog.Title>
                      Move “{movie.title}” to Trash?
                    </AlertDialog.Title>
                    <AlertDialog.Close
                      render={
                        <Button
                          aria-label="Close confirmation"
                          disabled={isTrashing}
                          size="icon-xs"
                          type="button"
                          variant="ghost"
                        >
                          <AppIcon name="close" />
                        </Button>
                      }
                    />
                  </div>
                  <AlertDialog.Description>
                    This moves the selected video to macOS Trash or the Windows
                    Recycle Bin. It may be recoverable there.
                  </AlertDialog.Description>
                  {trashError === null ? null : (
                    <p
                      aria-atomic="true"
                      className="trash-dialog__error"
                      role="alert"
                    >
                      {trashError}
                    </p>
                  )}
                  <div className="trash-dialog__actions">
                    <AlertDialog.Close
                      render={
                        <Button
                          disabled={isTrashing}
                          ref={trashCancelButton}
                          type="button"
                          variant="outline"
                        >
                          Cancel
                        </Button>
                      }
                    />
                    <Button
                      aria-label={`${isTrashing ? "Moving" : "Confirm moving"} movie to Trash or Recycle Bin: ${movie.title}`}
                      disabled={isTrashing}
                      onClick={trashMovie}
                      onKeyDown={(event) => event.stopPropagation()}
                      type="button"
                      variant="destructive"
                    >
                      <AppIcon name="trash" />
                      {isTrashing ? "Moving…" : "Move file"}
                    </Button>
                  </div>
                </AlertDialog.Popup>
              </AlertDialog.Viewport>
            </AlertDialog.Portal>
          </AlertDialog.Root>
          <CopyTitleAction title={movie.title} />
        </div>
      </div>
      <div className="movie-card__file-action-errors">
        {openError === null ? null : (
          <p aria-atomic="true" role="alert">
            {openError}
          </p>
        )}
        {revealError === null ? null : (
          <p aria-atomic="true" role="alert">
            {revealError}
          </p>
        )}
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

function compareLibraryMoviesByTitle(
  leftMovie: Movie,
  rightMovie: Movie,
  direction: LibraryTitleSortDirection,
) {
  const leftTitle = leftMovie.title.toLowerCase();
  const rightTitle = rightMovie.title.toLowerCase();
  const titleOrder =
    leftTitle < rightTitle ? -1 : leftTitle > rightTitle ? 1 : 0;
  if (titleOrder !== 0) {
    return direction === "ascending" ? titleOrder : -titleOrder;
  }

  if (leftMovie.title !== rightMovie.title) {
    return leftMovie.title < rightMovie.title ? -1 : 1;
  }
  if (leftMovie.path === rightMovie.path) {
    return 0;
  }
  return leftMovie.path < rightMovie.path ? -1 : 1;
}

function formatStorageBytes(bytes: bigint) {
  const units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
  const bytesPerUnit = 1024n;
  const tenthsPerUnit = 10n;
  let unitIndex = 0;
  let unitSize = 1n;

  while (
    unitIndex < units.length - 1 &&
    bytes >= unitSize * bytesPerUnit
  ) {
    unitIndex += 1;
    unitSize *= bytesPerUnit;
  }
  if (unitIndex === 0) {
    return `${bytes} ${units[unitIndex]}`;
  }

  const roundedTenths =
    (bytes * tenthsPerUnit + unitSize / 2n) / unitSize;
  return `${roundedTenths / tenthsPerUnit}.${roundedTenths % tenthsPerUnit} ${units[unitIndex]}`;
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
  const [moviesFolder, setMoviesFolder] = useState<string | null>(null);
  const [isMoviesFolderLoaded, setIsMoviesFolderLoaded] = useState(false);
  const [movieScanState, setMovieScanState] = useState<MovieScanState>({
    status: "unconfigured",
  });
  const [movieRefreshVersion, setMovieRefreshVersion] = useState(0);
  const [movieStorageRefreshVersion, setMovieStorageRefreshVersion] =
    useState(0);
  const [moviesStorageState, setMoviesStorageState] =
    useState<MoviesStorageState>({ status: "unconfigured" });
  const [librarySelectedPage, setLibrarySelectedPage] = useState(1);
  const [librarySearchQuery, setLibrarySearchQuery] = useState("");
  const [libraryTitleSortDirection, setLibraryTitleSortDirection] =
    useState<LibraryTitleSortDirection>("ascending");
  const [movieTrashAnnouncement, setMovieTrashAnnouncement] = useState<
    string | null
  >(null);
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
  const [isDiscoverActivated, setIsDiscoverActivated] = useState(false);
  const [discoverSearchInput, setDiscoverSearchInput] = useState("");
  const [submittedDiscoverSearchQuery, setSubmittedDiscoverSearchQuery] =
    useState<string | null>(null);
  const [discoverSearchInputError, setDiscoverSearchInputError] = useState<
    string | null
  >(null);
  const [trendingDiscoverRefreshVersion, setTrendingDiscoverRefreshVersion] =
    useState(0);
  const [searchDiscoverRefreshVersion, setSearchDiscoverRefreshVersion] =
    useState(0);
  const [discoverSelectedPage, setDiscoverSelectedPage] = useState(1);
  const [selectedDiscoverMovie, setSelectedDiscoverMovie] =
    useState<TmdbMovie | null>(null);
  const [movieDetailsState, setMovieDetailsState] =
    useState<MovieDetailsState | null>(null);
  const [movieDetailsTriggerId, setMovieDetailsTriggerId] = useState<
    string | null
  >(null);
  const [movieDetailsRequestVersion, setMovieDetailsRequestVersion] =
    useState(0);
  const navigationItems = useRef<Array<HTMLButtonElement | null>>([]);
  const workspace = useRef<HTMLElement | null>(null);
  const scanRequestId = useRef(0);
  const storageRequestId = useRef(0);
  const discoverRequestId = useRef(0);
  const movieDetailsRequestId = useRef(0);
  const trendingDiscoverResult = useRef<{
    refreshVersion: number;
    result: TmdbMoviesResult;
  } | null>(null);
  const currentMoviesFolder = useRef(moviesFolder);
  const currentMovieScanState = useRef(movieScanState);
  // Late Trash responses read current state so an old card cannot modify replacement results.
  currentMoviesFolder.current = moviesFolder;
  currentMovieScanState.current = movieScanState;

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
    let isCurrent = true;

    void window.__TAURI__.core
      .invoke<string | null>("load_movies_folder")
      .then((savedFolder) => {
        if (!isCurrent) {
          return;
        }
        if (
          savedFolder !== null &&
          (typeof savedFolder !== "string" || savedFolder === "")
        ) {
          throw new Error("The native Movies folder store returned invalid data.");
        }

        setMoviesFolder(savedFolder);
      })
      .catch(() => {
        if (isCurrent) {
          setMoviesFolder(null);
          setFolderSelectionError(
            "The Movies folder configuration could not be loaded.",
          );
        }
      })
      .finally(() => {
        if (isCurrent) {
          setIsMoviesFolderLoaded(true);
        }
      });

    return () => {
      isCurrent = false;
    };
  }, []);

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

    if (!isMoviesFolderLoaded) {
      return;
    }
    if (moviesFolder === null) {
      setMovieScanState({ status: "unconfigured" });
      return;
    }

    setMovieScanState({ status: "scanning" });
    void window.__TAURI__.core
      .invoke<string[]>("scan_movies")
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
  }, [isMoviesFolderLoaded, moviesFolder, movieRefreshVersion]);

  useEffect(() => {
    const requestId = ++storageRequestId.current;

    if (!isMoviesFolderLoaded || moviesFolder === null) {
      setMoviesStorageState({ status: "unconfigured" });
      return;
    }

    setMoviesStorageState({ status: "loading" });
    void window.__TAURI__.core
      .invoke<unknown>("query_movies_storage")
      .then((values) => {
        if (requestId !== storageRequestId.current) {
          return;
        }
        if (
          !Array.isArray(values) ||
          values.length !== 2 ||
          values.some(
            (value) =>
              typeof value !== "string" || !/^\d+$/.test(value),
          )
        ) {
          throw new Error("The native storage query returned invalid data.");
        }

        const totalBytes = BigInt(values[0]);
        const freeBytes = BigInt(values[1]);
        if (totalBytes === 0n || freeBytes > totalBytes) {
          throw new Error("The native storage values were inconsistent.");
        }

        setMoviesStorageState({ status: "ready", totalBytes, freeBytes });
      })
      .catch((error: unknown) => {
        if (requestId !== storageRequestId.current) {
          return;
        }

        const errorCode =
          typeof error === "string"
            ? error
            : error instanceof Error
              ? error.message
              : "";
        setMoviesStorageState({
          status:
            errorCode === moviesStorageUnavailable
              ? "unavailable"
              : "error",
        });
      });

    return () => {
      storageRequestId.current += 1;
    };
  }, [isMoviesFolderLoaded, moviesFolder, movieStorageRefreshVersion]);

  useEffect(() => {
    if (activeDestination.id === "discover") {
      setIsDiscoverActivated(true);
    }
  }, [activeDestination.id]);

  useEffect(() => {
    const requestId = ++discoverRequestId.current;

    if (!isDiscoverActivated) {
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

    if (submittedDiscoverSearchQuery === null) {
      const cachedTrendingResult = trendingDiscoverResult.current;
      if (
        cachedTrendingResult?.refreshVersion ===
        trendingDiscoverRefreshVersion
      ) {
        setDiscoverState(cachedTrendingResult.result);
        return;
      }
    }

    const abortController = new AbortController();
    setDiscoverState({ status: "loading" });
    const request =
      submittedDiscoverSearchQuery === null
        ? fetchWeeklyTrendingMovies(tmdbToken, abortController.signal)
        : fetchTmdbMoviesByTitle(
            tmdbToken,
            submittedDiscoverSearchQuery,
            abortController.signal,
          );
    void request.then((result) => {
      if (requestId !== discoverRequestId.current) {
        return;
      }
      if (submittedDiscoverSearchQuery === null) {
        trendingDiscoverResult.current = {
          refreshVersion: trendingDiscoverRefreshVersion,
          result,
        };
      }
      setDiscoverState(result);
    });

    return () => {
      discoverRequestId.current += 1;
      abortController.abort();
    };
  }, [
    isDiscoverActivated,
    isTmdbTokenLoaded,
    searchDiscoverRefreshVersion,
    submittedDiscoverSearchQuery,
    tmdbCredentialLoadFailed,
    tmdbToken,
    trendingDiscoverRefreshVersion,
  ]);

  useEffect(() => {
    const requestId = ++movieDetailsRequestId.current;

    if (selectedDiscoverMovie === null) {
      return;
    }
    if (tmdbToken === null) {
      setMovieDetailsState({ status: "unauthorized" });
      return;
    }

    const abortController = new AbortController();
    setMovieDetailsState({ status: "loading" });
    void fetchTmdbMovieDetails(
      tmdbToken,
      selectedDiscoverMovie.id,
      abortController.signal,
    ).then((result) => {
      if (requestId === movieDetailsRequestId.current) {
        setMovieDetailsState(result);
      }
    });

    return () => {
      movieDetailsRequestId.current += 1;
      abortController.abort();
    };
  }, [
    movieDetailsRequestVersion,
    selectedDiscoverMovie,
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
      const selectedFolder = await window.__TAURI__.core.invoke<string | null>(
        "choose_movies_folder",
      );

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
        setMovieStorageRefreshVersion((version) => version + 1);
      } else {
        setMoviesFolder(selectedFolder);
      }
    } catch {
      setFolderSelectionError("The Movies folder picker could not be opened.");
    } finally {
      setIsChoosingFolder(false);
    }
  };

  const clearMoviesFolder = async () => {
    scanRequestId.current += 1;
    setFolderSelectionError(null);
    setMovieScanState({ status: "unconfigured" });
    try {
      await window.__TAURI__.core.invoke("clear_movies_folder");
      setMoviesFolder(null);
    } catch {
      setFolderSelectionError(
        "The Movies folder configuration could not be cleared.",
      );
      setMovieRefreshVersion((version) => version + 1);
    }
  };

  const refreshMovies = () => {
    if (moviesFolder === null) {
      return;
    }

    scanRequestId.current += 1;
    setMovieScanState({ status: "scanning" });
    setMovieRefreshVersion((version) => version + 1);
    setMovieStorageRefreshVersion((version) => version + 1);
  };

  const updateLibrarySearchQuery = (query: string) => {
    setLibrarySearchQuery(query);
    setLibrarySelectedPage(1);
  };

  const updateLibraryTitleSortDirection = (
    direction: LibraryTitleSortDirection,
  ) => {
    setLibraryTitleSortDirection(direction);
    setLibrarySelectedPage(1);
  };

  const recordTrashedMovie = (movie: Movie, confirmedFolder: string) => {
    if (currentMoviesFolder.current !== confirmedFolder) {
      return;
    }

    setMovieTrashAnnouncement(
      `${movie.title} was moved to Trash or the Recycle Bin.`,
    );
    setMovieStorageRefreshVersion((version) => version + 1);

    if (currentMovieScanState.current.status === "scanning") {
      scanRequestId.current += 1;
      setMovieRefreshVersion((version) => version + 1);
      return;
    }

    setMovieScanState((currentState) => {
      if (currentState.status !== "ready") {
        return currentState;
      }

      const remainingMovies = currentState.movies.filter(
        (currentMovie) => currentMovie.path !== movie.path,
      );
      if (remainingMovies.length === currentState.movies.length) {
        return currentState;
      }

      return remainingMovies.length === 0
        ? { status: "empty" }
        : { status: "ready", movies: remainingMovies };
    });
  };

  const openDiscoverMovieDetails = (
    movie: TmdbMovie,
    triggerId: string,
  ) => {
    movieDetailsRequestId.current += 1;
    setSelectedDiscoverMovie(movie);
    setMovieDetailsState({ status: "loading" });
    setMovieDetailsTriggerId(triggerId);
    setMovieDetailsRequestVersion((version) => version + 1);
  };

  const closeDiscoverMovieDetails = () => {
    movieDetailsRequestId.current += 1;
    setSelectedDiscoverMovie(null);
    setMovieDetailsState(null);
  };

  const reloadDiscoverMode = () => {
    discoverRequestId.current += 1;
    setDiscoverState({ status: "loading" });
    if (submittedDiscoverSearchQuery === null) {
      setTrendingDiscoverRefreshVersion((version) => version + 1);
    } else {
      setSearchDiscoverRefreshVersion((version) => version + 1);
    }
  };

  const saveTmdbToken = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const token = tmdbTokenInput.trim();
    if (token === "") {
      return;
    }

    const previousToken = tmdbToken;
    closeDiscoverMovieDetails();
    discoverRequestId.current += 1;
    trendingDiscoverResult.current = null;
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
      reloadDiscoverMode();
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
    closeDiscoverMovieDetails();
    discoverRequestId.current += 1;
    trendingDiscoverResult.current = null;
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
      reloadDiscoverMode();
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

    reloadDiscoverMode();
  };

  const searchDiscoverMovies = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (tmdbToken === null) {
      return;
    }
    if (discoverSearchInput.trim() === "") {
      setDiscoverSearchInputError("Enter a movie title to search TMDB.");
      return;
    }

    discoverRequestId.current += 1;
    setDiscoverState({ status: "loading" });
    setDiscoverSearchInputError(null);
    setSubmittedDiscoverSearchQuery(discoverSearchInput);
    setDiscoverSelectedPage(1);
    setSearchDiscoverRefreshVersion((version) => version + 1);
  };

  const clearDiscoverSearch = () => {
    discoverRequestId.current += 1;
    setDiscoverSearchInput("");
    setDiscoverSearchInputError(null);
    setSubmittedDiscoverSearchQuery(null);
    setDiscoverSelectedPage(1);

    const cachedTrendingResult = trendingDiscoverResult.current;
    setDiscoverState(
      cachedTrendingResult?.refreshVersion ===
        trendingDiscoverRefreshVersion
        ? cachedTrendingResult.result
        : { status: "loading" },
    );
  };

  const currentMovieScanMessage =
    movieScanState.status === "ready"
      ? null
      : movieScanMessages[movieScanState.status];
  const isDiscoverSearchActive = submittedDiscoverSearchQuery !== null;
  const currentDiscoverMessage =
    discoverState.status === "ready"
      ? null
      : (isDiscoverSearchActive
          ? discoverSearchMessages
          : discoverMessages)[discoverState.status];
  const discoverGalleryLabel = isDiscoverSearchActive
    ? "TMDB Movies search results"
    : "Weekly trending Movies";
  const isLibrarySearchActive = librarySearchQuery.trim() !== "";
  const completeLibraryMovies =
    movieScanState.status === "ready" ? movieScanState.movies : [];
  const matchingLibraryMovies = isLibrarySearchActive
    ? completeLibraryMovies.filter((movie) =>
        movie.title.toLowerCase().includes(librarySearchQuery.toLowerCase()),
      )
    : completeLibraryMovies;
  const orderedLibraryMovies = [...matchingLibraryMovies].sort(
    (leftMovie, rightMovie) =>
      compareLibraryMoviesByTitle(
        leftMovie,
        rightMovie,
        libraryTitleSortDirection,
      ),
  );
  let dashboardMoviesHeading = "Loading Movies Library";
  let dashboardMoviesMessage = "Loading the configured Movies folder.";
  let dashboardMoviesRole: "alert" | "status" | undefined = "status";
  let dashboardMoviesDestination: (typeof destinations)[number] | null = null;

  if (isMoviesFolderLoaded) {
    if (moviesFolder === null && folderSelectionError !== null) {
      dashboardMoviesHeading = "Movies Library needs attention";
      dashboardMoviesMessage = folderSelectionError;
      dashboardMoviesRole = "alert";
      dashboardMoviesDestination = settingsDestination;
    } else if (moviesFolder === null) {
      dashboardMoviesHeading = "Movies Library is not configured";
      dashboardMoviesMessage = "Choose one local Movies folder in Settings.";
      dashboardMoviesRole = undefined;
      dashboardMoviesDestination = settingsDestination;
    } else if (
      movieScanState.status === "scanning" ||
      movieScanState.status === "unconfigured"
    ) {
      dashboardMoviesHeading = "Scanning Movies Library";
      dashboardMoviesMessage =
        "Looking recursively for supported .mp4 and .mkv files.";
      dashboardMoviesDestination = libraryDestination;
    } else if (movieScanState.status === "empty") {
      dashboardMoviesHeading = "0 supported Movies";
      dashboardMoviesMessage =
        "The configured folder is available but contains no supported .mp4 or .mkv files.";
      dashboardMoviesRole = undefined;
      dashboardMoviesDestination = libraryDestination;
    } else if (movieScanState.status === "ready") {
      const movieCount = movieScanState.movies.length;
      dashboardMoviesHeading = `${movieCount} supported ${movieCount === 1 ? "Movie" : "Movies"}`;
      dashboardMoviesMessage =
        "This total comes from the complete current folder scan.";
      dashboardMoviesRole = undefined;
      dashboardMoviesDestination = libraryDestination;
    } else if (movieScanState.status === "unavailable") {
      dashboardMoviesHeading = "Movies folder is unavailable";
      dashboardMoviesMessage =
        "The configured folder may have moved or become inaccessible.";
      dashboardMoviesRole = "alert";
      dashboardMoviesDestination = settingsDestination;
    } else {
      dashboardMoviesHeading = "Movies Library scan failed";
      dashboardMoviesMessage =
        "Auto-Video could not read every item in the configured folder.";
      dashboardMoviesRole = "alert";
      dashboardMoviesDestination = libraryDestination;
    }
  }
  let dashboardStorageHeading = "Waiting for Movies folder configuration";
  let dashboardStorageMessage =
    "Storage will load after the configured Movies folder is known.";
  let dashboardStorageRole: "alert" | "status" | undefined;

  if (isMoviesFolderLoaded) {
    if (moviesFolder === null) {
      dashboardStorageHeading = "Storage unavailable";
      dashboardStorageMessage =
        folderSelectionError ??
        "Configure a Movies folder before loading volume storage.";
    } else if (
      moviesStorageState.status === "loading" ||
      moviesStorageState.status === "unconfigured"
    ) {
      dashboardStorageHeading = "Loading storage";
      dashboardStorageMessage =
        "Reading the volume capacity for the configured Movies folder.";
      dashboardStorageRole = "status";
    } else if (moviesStorageState.status === "unavailable") {
      dashboardStorageHeading = "Movies volume is unavailable";
      dashboardStorageMessage =
        "The configured folder or its containing volume is not accessible.";
      dashboardStorageRole = "alert";
    } else if (moviesStorageState.status === "error") {
      dashboardStorageHeading = "Storage could not be loaded";
      dashboardStorageMessage =
        "Auto-Video could not read the containing volume capacity.";
      dashboardStorageRole = "alert";
    }
  }

  return (
    <>
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

          {movieTrashAnnouncement === null ? null : (
            <p aria-atomic="true" className="sr-only" role="status">
              {movieTrashAnnouncement}
            </p>
          )}

          {activeDestination.id === "dashboard" ? (
            <section
              aria-busy={
                !isMoviesFolderLoaded ||
                movieScanState.status === "scanning" ||
                (moviesFolder !== null &&
                  movieScanState.status === "unconfigured")
              }
              aria-labelledby="dashboard-movies-heading"
              className="dashboard-library-summary"
            >
              <div className="dashboard-library-summary__heading">
                <span className="empty-state__icon">
                  <AppIcon name="library" />
                </span>
                <div>
                  <p className="card-eyebrow">Local library</p>
                  <h2 id="dashboard-movies-heading">Movies Library</h2>
                  <p className="dashboard-library-summary__folder">
                    {!isMoviesFolderLoaded
                      ? "Loading configured Movies folder…"
                      : moviesFolder ?? "No Movies folder configured"}
                  </p>
                </div>
              </div>

              <div
                className="dashboard-library-summary__status"
                role={dashboardMoviesRole}
              >
                <p className="card-eyebrow">Current status</p>
                <h3>{dashboardMoviesHeading}</h3>
                <p>{dashboardMoviesMessage}</p>
              </div>

              <div
                aria-busy={moviesStorageState.status === "loading"}
                className="dashboard-library-summary__storage"
              >
                <p className="card-eyebrow">Storage</p>
                {moviesStorageState.status === "ready" &&
                moviesFolder !== null ? (
                  <dl aria-label="Movies volume storage">
                    <div>
                      <dt>Total</dt>
                      <dd>
                        {formatStorageBytes(moviesStorageState.totalBytes)}
                      </dd>
                    </div>
                    <div>
                      <dt>Used</dt>
                      <dd>
                        {formatStorageBytes(
                          moviesStorageState.totalBytes -
                            moviesStorageState.freeBytes,
                        )}
                      </dd>
                    </div>
                    <div>
                      <dt>Free</dt>
                      <dd>
                        {formatStorageBytes(moviesStorageState.freeBytes)}
                      </dd>
                    </div>
                  </dl>
                ) : (
                  <div role={dashboardStorageRole}>
                    <h3>{dashboardStorageHeading}</h3>
                    <p>{dashboardStorageMessage}</p>
                  </div>
                )}
              </div>

              {dashboardMoviesDestination === null ? null : (
                <Button
                  className="dashboard-library-summary__action"
                  onClick={() => navigateTo(dashboardMoviesDestination)}
                  type="button"
                >
                  <AppIcon name={dashboardMoviesDestination.id} />
                  Open {dashboardMoviesDestination.label}
                </Button>
              )}
            </section>
          ) : activeDestination.id === "discover" ? (
            <section
              aria-busy={discoverState.status === "loading"}
              aria-labelledby="discover-movies-heading"
              className="discover-content"
            >
              <div className="library-toolbar library-toolbar--discover">
                <div className="library-toolbar__heading">
                  <span className="empty-state__icon">
                    <AppIcon name="discover" />
                  </span>
                  <div>
                    <p className="card-eyebrow">TMDB Discover</p>
                    <h2 id="discover-movies-heading">
                      {discoverGalleryLabel}
                    </h2>
                    <p className="library-folder">
                      {isDiscoverSearchActive ? (
                        <>
                          Results for “
                          <span className="discover-search-query">
                            {submittedDiscoverSearchQuery}
                          </span>
                          ”
                        </>
                      ) : (
                        "Weekly Movies feed"
                      )}
                    </p>
                  </div>
                </div>
                {isTmdbTokenLoaded &&
                !tmdbCredentialLoadFailed &&
                tmdbToken !== null ? (
                  <div className="discover-toolbar__controls">
                    <form
                      aria-label="Search TMDB Movies"
                      className="discover-search"
                      onSubmit={searchDiscoverMovies}
                      role="search"
                    >
                      <label htmlFor="discover-movies-search">
                        Search Movies
                      </label>
                      <div className="discover-search__field">
                        <input
                          aria-describedby={
                            discoverSearchInputError === null
                              ? undefined
                              : "discover-search-error"
                          }
                          aria-invalid={
                            discoverSearchInputError === null
                              ? undefined
                              : true
                          }
                          className="discover-search__input"
                          id="discover-movies-search"
                          onChange={(event) => {
                            setDiscoverSearchInput(event.target.value);
                            if (discoverSearchInputError !== null) {
                              setDiscoverSearchInputError(null);
                            }
                          }}
                          placeholder="Find a Movie"
                          type="text"
                          value={discoverSearchInput}
                        />
                        <Button type="submit">
                          <AppIcon name="search" />
                          Search
                        </Button>
                      </div>
                      {discoverSearchInputError === null ? null : (
                        <p
                          className="discover-search__error"
                          id="discover-search-error"
                          role="alert"
                        >
                          {discoverSearchInputError}
                        </p>
                      )}
                    </form>
                    {isDiscoverSearchActive ? (
                      <Button
                        onClick={clearDiscoverSearch}
                        type="button"
                        variant="outline"
                      >
                        <AppIcon name="close" />
                        Clear
                      </Button>
                    ) : null}
                    <Button
                      onClick={refreshDiscover}
                      type="button"
                      variant="outline"
                    >
                      <AppIcon name="refresh" />
                      Refresh
                    </Button>
                  </div>
                ) : null}
              </div>

              {discoverState.status === "ready" ? (
                <ResizeAwareGallery
                  ariaLabel={discoverGalleryLabel}
                  getItemKey={(movie, resultIndex) =>
                    `${movie.id}-${resultIndex}-${movie.posterPath ?? "posterless"}`
                  }
                  items={discoverState.movies}
                  key={
                    isDiscoverSearchActive
                      ? "discover-search-gallery"
                      : "discover-trending-gallery"
                  }
                  onSelectedPageChange={setDiscoverSelectedPage}
                  renderItem={(movie, resultIndex) => (
                    <DiscoverMovieCard
                      movie={movie}
                      onViewDetails={openDiscoverMovieDetails}
                      resultIndex={resultIndex}
                    />
                  )}
                  selectedPage={discoverSelectedPage}
                  variant="discover"
                />
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
              <div className="library-toolbar library-toolbar--movies">
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
                {moviesFolder === null ? null : (
                  <div className="library-toolbar__controls">
                    <div
                      aria-label="Movies title search"
                      className="movie-search"
                      role="search"
                    >
                      <label htmlFor="movies-title-search">Search titles</label>
                      <div className="movie-search__field">
                        <span className="movie-search__icon">
                          <AppIcon name="search" />
                        </span>
                        <input
                          aria-describedby={
                            movieScanState.status === "ready"
                              ? "movies-search-results"
                              : undefined
                          }
                          className="movie-search__input"
                          id="movies-title-search"
                          onChange={(event) =>
                            updateLibrarySearchQuery(event.target.value)
                          }
                          placeholder="Find a Movie"
                          type="text"
                          value={librarySearchQuery}
                        />
                        {librarySearchQuery === "" ? null : (
                          <Button
                            aria-label="Clear Movies search"
                            className="movie-search__clear"
                            onClick={() => updateLibrarySearchQuery("")}
                            size="icon-sm"
                            type="button"
                            variant="ghost"
                          >
                            <AppIcon name="close" />
                          </Button>
                        )}
                      </div>
                    </div>
                    <div className="movie-sort">
                      <label htmlFor="movies-title-sort">Sort titles</label>
                      <select
                        className="movie-sort__select"
                        id="movies-title-sort"
                        onChange={(event) => {
                          const direction = event.target.value;
                          if (
                            direction !== "ascending" &&
                            direction !== "descending"
                          ) {
                            throw new Error(
                              "The Movies title sort returned an invalid direction.",
                            );
                          }
                          updateLibraryTitleSortDirection(direction);
                        }}
                        value={libraryTitleSortDirection}
                      >
                        <option value="ascending">Title A–Z</option>
                        <option value="descending">Title Z–A</option>
                      </select>
                    </div>
                    <Button
                      disabled={movieScanState.status === "scanning"}
                      onClick={refreshMovies}
                      type="button"
                      variant="outline"
                    >
                      <AppIcon name="refresh" />
                      Refresh
                    </Button>
                  </div>
                )}
              </div>

              {movieScanState.status === "ready" && moviesFolder !== null ? (
                <>
                  <p
                    aria-atomic="true"
                    aria-live="polite"
                    className="sr-only"
                    id="movies-search-results"
                  >
                    {isLibrarySearchActive
                      ? `${matchingLibraryMovies.length} Movies match the current title search.`
                      : `${movieScanState.movies.length} Movies in the complete current result.`}
                  </p>
                  {matchingLibraryMovies.length === 0 &&
                  isLibrarySearchActive ? (
                    <div className="empty-state library-state library-search-empty">
                      <span className="empty-state__icon">
                        <AppIcon name="search" />
                      </span>
                      <h2>No Movies match this search</h2>
                      <p>
                        No titles match “
                        <span className="library-search-empty__query">
                          {librarySearchQuery}
                        </span>
                        ”. Clear the search to restore the complete Library.
                      </p>
                    </div>
                  ) : (
                    <ResizeAwareGallery
                      ariaLabel="Movies"
                      getItemKey={(movie) => movie.path}
                      items={orderedLibraryMovies}
                      key="library-gallery"
                      onSelectedPageChange={setLibrarySelectedPage}
                      renderItem={(movie) => (
                        <LibraryMovieCard
                          folder={moviesFolder}
                          movie={movie}
                          onMovieTrashed={recordTrashedMovie}
                        />
                      )}
                      selectedPage={librarySelectedPage}
                      variant="library"
                    />
                  )}
                </>
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
                      Save one token locally for Discover Movies. The saved
                      value is never shown.
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
      <Dialog.Root
        onOpenChange={(open) => {
          if (!open) {
            closeDiscoverMovieDetails();
          }
        }}
        open={selectedDiscoverMovie !== null}
      >
        {selectedDiscoverMovie === null ||
        movieDetailsState === null ||
        movieDetailsTriggerId === null ? null : (
          <DiscoverMovieDetails
            key={selectedDiscoverMovie.id}
            movie={selectedDiscoverMovie}
            state={movieDetailsState}
            triggerId={movieDetailsTriggerId}
          />
        )}
      </Dialog.Root>
    </>
  );
}
