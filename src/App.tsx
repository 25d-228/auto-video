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
  GogglesIcon,
  ListMagnifyingGlassIcon,
  MonitorIcon,
  MoonIcon,
  PauseIcon,
  PlayIcon,
  SquaresFourIcon,
  SunIcon,
  TelevisionSimpleIcon,
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
import {
  chooseTvFolder,
  clearTvFolder,
  loadTvFolder,
  openTvFile,
  queryTvStorage,
  revealTvFile,
  scanTvLibrary,
  type TvFolderState,
  type TvLibraryFile,
  type TvLibraryItem,
} from "@/tv";
import {
  applyVrOrganization,
  canonicalizeProductCode,
  cancelVrDownload,
  chooseVrFolder,
  clearVrFolder,
  dismissVrDownload,
  dismissVrOrganization,
  fetchExactJavdbVrItem,
  fetchVerifiedSukebeiReleases,
  inspectVerifiedSukebeiTorrent,
  invalidateVerifiedVrTorrent,
  listVrDownloads,
  loadVrDownloadLimit,
  loadVrDownloads,
  loadVrFolder,
  openVrFile,
  pauseVrDownload,
  previewVrOrganization,
  revealVrFile,
  resumeVrDownload,
  saveVrDownloadLimit,
  saveVerifiedVrTorrent,
  scanVrLibrary,
  startVerifiedVrDownload,
  type VrDownload,
  type VrDownloadLimit,
  type VrFolderState,
  type VrLibraryFile,
  type VrLibraryItem,
  type VrOrganizationPreview,
  type VrCatalogItem,
  type VrCatalogResult,
  type VrRelease,
  type VrReleasesResult,
  type VrTorrentInspectionResult,
} from "@/vr";

import "./index.css";

const destinations = [
  {
    id: "dashboard",
    label: "Dashboard",
    description: "Current status for local libraries and VR transfers.",
    emptyHeading: "Dashboard data is not available yet",
    emptyMessage:
      "Metrics and storage details will appear here only after their data sources are implemented.",
  },
  {
    id: "discover",
    label: "Discover",
    description: "Browse TMDB Movies or find VR titles by exact product code.",
    emptyHeading: "Discovery is not configured",
    emptyMessage:
      "Add a TMDB API Read Access Token in Settings to load weekly trending Movies.",
  },
  {
    id: "library",
    label: "Library",
    description: "Browse supported video files from your local Movies, TV, and VR folders.",
    emptyHeading: "Choose a Movies folder to begin",
    emptyMessage:
      "Configure one local Movies folder in Settings before scanning your library.",
  },
  {
    id: "downloads",
    label: "Downloads",
    description: "Review and manage selected-file VR transfers.",
    emptyHeading: "No VR downloads",
    emptyMessage: "Start a selected-file transfer from a verified torrent inspection.",
  },
  {
    id: "settings",
    label: "Settings",
    description: "Configure TMDB, local media folders, VR transfers, and appearance.",
    emptyHeading: "Other settings are not configured",
    emptyMessage:
      "Provider credentials and additional preferences will appear only with the features they control.",
  },
] as const;
const libraryDestination = destinations[2];
const downloadsDestination = destinations[3];
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
  vr: GogglesIcon,
  tv: TelevisionSimpleIcon,
  releases: ListMagnifyingGlassIcon,
  pause: PauseIcon,
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
type TvLibraryScanState =
  | { status: "loading" }
  | { status: "unconfigured" }
  | { status: "scanning" }
  | { status: "empty" }
  | { status: "unavailable" }
  | { status: "error" }
  | { status: "ready"; items: TvLibraryItem[] };
type VolumeStorageState =
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
type DiscoverCategory = "movies" | "vr";
type LibraryCategory = "movies" | "tv" | "vr";
type VrLibraryScanState =
  | { status: "loading" }
  | { status: "unconfigured" }
  | { status: "scanning" }
  | { status: "empty" }
  | { status: "unavailable" }
  | { status: "error" }
  | { status: "ready"; items: VrLibraryItem[] };
type VrCatalogState =
  | { status: "idle" }
  | { status: "loading" }
  | VrCatalogResult;
type VrReleaseComparisonState = { status: "loading" } | VrReleasesResult;
type VrTorrentInspectionState =
  | { status: "loading" }
  | VrTorrentInspectionResult;
type VrTorrentSaveState = "idle" | "saving" | "success" | "error";
type VrTorrentStartState =
  | { status: "idle" }
  | { status: "starting" }
  | { status: "success" }
  | { status: "error"; message: string };
type VrFolderUiState =
  | { status: "loading" }
  | VrFolderState
  | { status: "error" };
type TvFolderUiState =
  | { status: "loading" }
  | TvFolderState
  | { status: "error" };
type VrDownloadsUiState =
  | { status: "loading" }
  | { status: "error" }
  | { status: "ready"; downloads: VrDownload[] };
type VrDownloadLimitUiState =
  | { status: "loading" }
  | { status: "error" }
  | { status: "ready"; limit: VrDownloadLimit };
type VrDownloadLimitMode = "unlimited" | "limited";
type VrDownloadSummary = {
  activeCount: number;
  pausedCount: number;
  completedCount: number;
  attentionCount: number;
  aggregateSpeedBytesPerSecond: bigint;
};
type VrTorrentInspectionContext = {
  item: VrCatalogItem;
  release: VrRelease;
  triggerId: string;
};
type GalleryVariant = "discover" | "library";
type GalleryLayout = {
  capacity: number;
  columns: number;
  rowHeight: number;
};

const appearanceStorageKey = "auto-video-appearance";
const moviesFolderUnavailable = "movies_folder_unavailable";
const moviesStorageUnavailable = "movies_storage_unavailable";
const vrStorageUnavailable = "vr_storage_unavailable";
const tvStorageUnavailable = "tv_storage_unavailable";
const activeVrDownloadStates = new Set(["queued", "downloading", "paused"]);
const systemDarkModeQuery = "(prefers-color-scheme: dark)";
// Two seconds confirms a successful copy without leaving stale feedback on the card.
const copySuccessDuration = 2000;
// Active rows refresh once per second so progress stays useful without overlapping native polls.
const vrDownloadRefreshInterval = 1000;
const maximumVrDownloadLimitMibPerSecond = 4095n;
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

const vrLibraryScanMessages = {
  loading: {
    heading: "Loading VR folder",
    message: "Checking the configured VR folder.",
    role: "status",
  },
  unconfigured: {
    heading: "Choose a VR folder to begin",
    message: "Configure one local VR folder in Settings before scanning your library.",
    role: undefined,
  },
  scanning: {
    heading: "Scanning VR folder",
    message: "Looking recursively for .mp4 and .mkv files.",
    role: "status",
  },
  empty: {
    heading: "No supported VR videos found",
    message: "This folder does not contain any .mp4 or .mkv files.",
    role: undefined,
  },
  unavailable: {
    heading: "VR folder is unavailable",
    message: "The configured folder may have moved or become inaccessible. Check it in Settings or try Refresh.",
    role: "alert",
  },
  error: {
    heading: "VR folder could not be scanned",
    message: "Auto-Video could not read every item in this folder. Check its access and try Refresh.",
    role: "alert",
  },
} as const;

const tvLibraryScanMessages = {
  loading: {
    heading: "Loading TV folder",
    message: "Checking the configured TV folder.",
    role: "status",
  },
  unconfigured: {
    heading: "Choose a TV folder to begin",
    message: "Configure one local TV folder in Settings before scanning your library.",
    role: undefined,
  },
  scanning: {
    heading: "Scanning TV folder",
    message: "Looking recursively for .mp4 and .mkv files.",
    role: "status",
  },
  empty: {
    heading: "No supported TV videos found",
    message: "This folder does not contain any .mp4 or .mkv files.",
    role: undefined,
  },
  unavailable: {
    heading: "TV folder is unavailable",
    message: "The configured folder may have moved or become inaccessible. Check it in Settings or try Refresh.",
    role: "alert",
  },
  error: {
    heading: "TV folder could not be scanned",
    message: "Auto-Video could not read every item in this folder. Check its access and try Refresh.",
    role: "alert",
  },
} as const;

const tvFileOpenErrorMessages: Record<string, string> = {
  tv_file_open_not_found: "This file is no longer available.",
  tv_file_open_unavailable: "Auto-Video could not access this file.",
  tv_file_open_not_file: "This item is not an eligible video file.",
  tv_file_open_unsupported: "This item is not a supported .mp4 or .mkv file.",
  tv_file_open_outside_folder: "This file is outside the configured TV folder.",
  tv_file_open_stale: "This file is no longer part of the current TV Library.",
  tv_file_open_failed: "The operating system could not open this file.",
};

const tvFileRevealErrorMessages: Record<string, string> = {
  tv_file_reveal_not_found: "This file is no longer available.",
  tv_file_reveal_unavailable: "Auto-Video could not access this file.",
  tv_file_reveal_not_file: "This item is not an eligible video file.",
  tv_file_reveal_unsupported: "This item is not a supported .mp4 or .mkv file.",
  tv_file_reveal_outside_folder: "This file is outside the configured TV folder.",
  tv_file_reveal_stale: "This file is no longer part of the current TV Library.",
  tv_file_reveal_failed: "The operating system could not reveal this file.",
};

const vrFileOpenErrorMessages: Record<string, string> = {
  vr_file_open_not_found: "This file is no longer available.",
  vr_file_open_unavailable: "Auto-Video could not access this file.",
  vr_file_open_not_file: "This item is not an eligible video file.",
  vr_file_open_unsupported: "This item is not a supported .mp4 or .mkv file.",
  vr_file_open_outside_folder: "This file is outside the configured VR folder.",
  vr_file_open_stale: "This file is no longer part of the current VR Library.",
  vr_file_open_failed: "The operating system could not open this file.",
};

const vrFileRevealErrorMessages: Record<string, string> = {
  vr_file_reveal_not_found: "This file is no longer available.",
  vr_file_reveal_unavailable: "Auto-Video could not access this file.",
  vr_file_reveal_not_file: "This item is not an eligible video file.",
  vr_file_reveal_unsupported: "This item is not a supported .mp4 or .mkv file.",
  vr_file_reveal_outside_folder: "This file is outside the configured VR folder.",
  vr_file_reveal_stale: "This file is no longer part of the current VR Library.",
  vr_file_reveal_failed: "The operating system could not reveal this file.",
};

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

const vrCatalogMessages = {
  idle: {
    heading: "Search for a VR title by product code",
    message: "Submit one exact product code to search JavDB.",
    role: undefined,
  },
  loading: {
    heading: "Searching JavDB",
    message: "Verifying the requested VR product-code identity.",
    role: "status",
  },
  "no-exact-match": {
    heading: "No exact VR title found",
    message: "JavDB returned no media item with the requested product code.",
    role: undefined,
  },
  "source-unavailable": {
    heading: "JavDB is unavailable",
    message: "The catalog source is not available. Try the search again later.",
    role: "alert",
  },
  "network-error": {
    heading: "JavDB could not be reached",
    message: "Check the network connection and try the search again.",
    role: "alert",
  },
  "malformed-provider": {
    heading: "JavDB returned invalid catalog data",
    message: "The response could not establish the requested product identity.",
    role: "alert",
  },
  "provider-error": {
    heading: "JavDB could not complete the search",
    message: "The catalog provider returned an unexpected error. Try again later.",
    role: "alert",
  },
} as const;

const vrReleaseMessages = {
  loading: {
    heading: "Finding verified releases",
    message: "Requesting candidates from Sukebei and verifying each identity.",
    role: "status",
  },
  "source-unavailable": {
    heading: "Sukebei is unavailable",
    message: "The release source is not available. Try again later.",
    role: "alert",
  },
  "network-error": {
    heading: "Sukebei could not be reached",
    message: "Check the network connection and try again.",
    role: "alert",
  },
  "malformed-provider": {
    heading: "Sukebei returned invalid release data",
    message: "The response could not be verified safely.",
    role: "alert",
  },
  "provider-error": {
    heading: "Sukebei could not load releases",
    message: "The release provider returned an unexpected error. Try again later.",
    role: "alert",
  },
} as const;

const vrTorrentMessages = {
  loading: {
    heading: "Inspecting verified torrent",
    message: "Fetching and verifying the exact selected Sukebei artifact.",
    role: "status",
  },
  "source-unavailable": {
    heading: "Torrent artifact is unavailable",
    message: "The selected provider artifact is no longer available.",
    role: "alert",
  },
  "network-error": {
    heading: "Torrent artifact could not be reached",
    message: "Check the network connection and try inspection again.",
    role: "alert",
  },
  "provider-error": {
    heading: "Torrent provider rejected the request",
    message: "The exact provider artifact could not be fetched safely.",
    role: "alert",
  },
  "malformed-torrent": {
    heading: "Torrent artifact is malformed",
    message: "The selected artifact did not contain valid torrent metainfo.",
    role: "alert",
  },
  "unsupported-torrent": {
    heading: "Torrent artifact is unsupported",
    message: "The selected artifact is not supported v1 torrent metainfo.",
    role: "alert",
  },
  "infohash-mismatch": {
    heading: "Torrent identity did not match",
    message: "The fetched artifact did not match the selected provider item.",
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

function DiscoverVrCard({
  item,
  onFindReleases,
}: {
  item: VrCatalogItem;
  onFindReleases: (item: VrCatalogItem, triggerId: string) => void;
}) {
  const [coverFailed, setCoverFailed] = useState(false);
  const releasesTriggerId = useId();
  const titleId = useId();

  return (
    <article
      aria-labelledby={titleId}
      className="discover-card discover-card--vr"
    >
      <div className="discover-card__poster">
        {item.coverUrl !== null && !coverFailed ? (
          <img
            alt=""
            onError={() => setCoverFailed(true)}
            src={item.coverUrl}
          />
        ) : (
          <div className="discover-card__poster-fallback">
            <AppIcon name="poster" />
            <span>Cover unavailable</span>
          </div>
        )}
      </div>
      <div className="discover-card__body">
        <div className="media-title-row">
          <h3 id={titleId}>{item.code}</h3>
          <div className="discover-card__title-actions">
            <CopyTitleAction title={item.code} />
            <Button
              aria-label={`Find releases: ${item.code}`}
              className="discover-card__releases-action"
              id={releasesTriggerId}
              onClick={(event) => {
                event.stopPropagation();
                onFindReleases(item, releasesTriggerId);
              }}
              onKeyDown={(event) => event.stopPropagation()}
              onPointerDown={(event) => event.stopPropagation()}
              size="xs"
              type="button"
              variant="outline"
            >
              <AppIcon name="releases" />
              Find releases
            </Button>
          </div>
        </div>
        <dl>
          <div>
            <dt>Title</dt>
            <dd>{item.title ?? "Unavailable"}</dd>
          </div>
          <div>
            <dt>Source</dt>
            <dd>{item.source}</dd>
          </div>
        </dl>
      </div>
    </article>
  );
}

function VrReleaseComparison({
  item,
  onInspectRelease,
  onRetry,
  onSelectRelease,
  selectedRelease,
  state,
  triggerId,
}: {
  item: VrCatalogItem;
  onInspectRelease: (release: VrRelease, triggerId: string) => void;
  onRetry: () => void;
  onSelectRelease: (release: VrRelease) => void;
  selectedRelease: VrRelease | null;
  state: VrReleaseComparisonState;
  triggerId: string;
}) {
  const releases = state.status === "ready" ? state.releases : null;
  const noVerifiedReleases = releases !== null && releases.length === 0;
  const currentMessage =
    state.status === "ready" ? null : vrReleaseMessages[state.status];

  return (
    <Dialog.Portal>
      <Dialog.Backdrop className="vr-releases__backdrop" />
      <Dialog.Viewport className="vr-releases__viewport">
        <Dialog.Popup
          aria-busy={state.status === "loading"}
          className="vr-releases__popup"
          finalFocus={() => document.getElementById(triggerId)}
          onClick={(event) => event.stopPropagation()}
          onPointerDown={(event) => event.stopPropagation()}
        >
          <div className="vr-releases__heading">
            <div>
              <p className="card-eyebrow">Sukebei release comparison</p>
              <Dialog.Title>{item.code}</Dialog.Title>
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
          <Dialog.Description className="vr-releases__description">
            Metadata-only comparison of releases verified for this product code.
          </Dialog.Description>

          {releases === null || noVerifiedReleases ? (
            <div
              className="vr-releases__state"
              role={noVerifiedReleases ? undefined : currentMessage?.role}
            >
              <span className="empty-state__icon">
                <AppIcon name="releases" />
              </span>
              <div>
                <h3>
                  {noVerifiedReleases
                    ? "No verified releases found"
                    : currentMessage?.heading}
                </h3>
                <p>
                  {noVerifiedReleases
                    ? `Sukebei returned no releases verified as ${item.code}.`
                    : currentMessage?.message}
                </p>
                {state.status !== "loading" && !noVerifiedReleases ? (
                  <Button onClick={onRetry} type="button" variant="outline">
                    <AppIcon name="refresh" />
                    Retry
                  </Button>
                ) : null}
              </div>
            </div>
          ) : (
            <div className="vr-releases__content">
              <div
                aria-label="Verified release totals"
                className="vr-releases__totals"
              >
                <p>
                  <strong>{releases.length}</strong> verified releases
                </p>
                <p>
                  <strong>{releases.length}</strong> from Sukebei
                </p>
                <Button onClick={onRetry} size="sm" type="button" variant="outline">
                  <AppIcon name="refresh" />
                  Retry
                </Button>
              </div>
              <ul aria-label={`Verified releases for ${item.code}`}>
                {releases.map((release, releaseIndex) => (
                  <li key={`${release.name}-${releaseIndex}`}>
                    <button
                      aria-pressed={selectedRelease === release}
                      onClick={() => onSelectRelease(release)}
                      type="button"
                    >
                      <span className="vr-releases__release-name">
                        {release.name}
                      </span>
                      <span className="vr-releases__release-metadata">
                        <span>Source {release.source}</span>
                        <span>Size {release.size ?? "Unavailable"}</span>
                        <span>
                          Seeders {release.seeders ?? "Unavailable"}
                        </span>
                      </span>
                    </button>
                  </li>
                ))}
              </ul>
              {selectedRelease === null ? (
                <p className="vr-releases__selection-prompt">
                  Select one verified release to compare its metadata.
                </p>
              ) : (
                <section
                  aria-labelledby="selected-vr-release-heading"
                  className="vr-releases__selection"
                >
                  <h3 id="selected-vr-release-heading">Selected release</h3>
                  <dl>
                    <div>
                      <dt>Product code</dt>
                      <dd>{item.code}</dd>
                    </div>
                    <div>
                      <dt>Release name</dt>
                      <dd>{selectedRelease.name}</dd>
                    </div>
                    <div>
                      <dt>Source</dt>
                      <dd>{selectedRelease.source}</dd>
                    </div>
                    <div>
                      <dt>Size</dt>
                      <dd>{selectedRelease.size ?? "Unavailable"}</dd>
                    </div>
                    <div>
                      <dt>Seeders</dt>
                      <dd>{selectedRelease.seeders ?? "Unavailable"}</dd>
                    </div>
                  </dl>
                  {selectedRelease.artifact === undefined ? (
                    <p className="vr-releases__artifact-unavailable">
                      Torrent inspection is unavailable because this release
                      has no complete safe provider artifact identity.
                    </p>
                  ) : (
                    <Button
                      id={`inspect-vr-torrent-${selectedRelease.artifact.providerItemId}`}
                      onClick={(event) =>
                        onInspectRelease(selectedRelease, event.currentTarget.id)
                      }
                      type="button"
                    >
                      <AppIcon name="details" />
                      Inspect torrent
                    </Button>
                  )}
                </section>
              )}
            </div>
          )}
        </Dialog.Popup>
      </Dialog.Viewport>
    </Dialog.Portal>
  );
}

function VrTorrentInspectionDialog({
  context,
  downloadsReady,
  folderState,
  onOpenDownloads,
  onOpenSettings,
  onRetry,
  onSave,
  onStart,
  onToggleFile,
  saveState,
  selectedFileIds,
  startState,
  state,
}: {
  context: VrTorrentInspectionContext;
  downloadsReady: boolean;
  folderState: VrFolderUiState;
  onOpenDownloads: () => void;
  onOpenSettings: () => void;
  onRetry: () => void;
  onSave: () => void;
  onStart: () => void;
  onToggleFile: (fileId: number) => void;
  saveState: VrTorrentSaveState;
  selectedFileIds: Set<number>;
  startState: VrTorrentStartState;
  state: VrTorrentInspectionState;
}) {
  const currentMessage =
    vrTorrentMessages[state.status === "ready" ? "loading" : state.status];
  const isFolderReady = folderState.status === "ready";
  const folderMessage =
    folderState.status === "loading"
      ? "Loading the configured VR folder…"
      : folderState.status === "ready"
        ? `Selected files will download to ${folderState.path}.`
        : folderState.status === "unavailable"
          ? "The configured VR folder is unavailable. Change or clear it in Settings."
          : folderState.status === "unconfigured"
            ? "Choose a VR folder in Settings before starting a download."
            : "The VR folder configuration could not be loaded.";

  return (
    <Dialog.Portal>
      <Dialog.Backdrop className="vr-torrent__backdrop" />
      <Dialog.Viewport className="vr-torrent__viewport">
        <Dialog.Popup
          aria-busy={state.status === "loading" || saveState === "saving"}
          className="vr-torrent__popup"
          finalFocus={() => document.getElementById(context.triggerId)}
        >
          <div className="vr-torrent__heading">
            <div>
              <p className="card-eyebrow">Verified Sukebei torrent</p>
              <Dialog.Title>{context.item.code}</Dialog.Title>
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
          <Dialog.Description className="vr-torrent__description">
            <span>Exact selected release</span>
            <span className="vr-torrent__release-name">
              {context.release.name}
            </span>
          </Dialog.Description>

          {state.status === "ready" ? (
            <div className="vr-torrent__content">
              <dl className="vr-torrent__metadata">
                <div>
                  <dt>Torrent name</dt>
                  <dd>{state.inspection.displayName}</dd>
                </div>
                <div>
                  <dt>Verified infohash</dt>
                  <dd>{state.inspection.infohash}</dd>
                </div>
                <div>
                  <dt>Total size</dt>
                  <dd>
                    {formatStorageBytes(BigInt(state.inspection.totalBytes))} (
                    {state.inspection.totalBytes} bytes)
                  </dd>
                </div>
                <div>
                  <dt>Files</dt>
                  <dd>{state.inspection.files.length}</dd>
                </div>
              </dl>
              <h3>Complete file list</h3>
              <fieldset className="vr-torrent__file-selection">
                <legend className="sr-only">Files to download</legend>
                <p>Select the files to download. No files are selected initially.</p>
                <ul aria-label={`Files in verified torrent for ${context.item.code}`}>
                  {state.inspection.files.map((file, fileId) => (
                    <li key={file.path}>
                      <label>
                        <input
                          checked={selectedFileIds.has(fileId)}
                          disabled={
                            startState.status === "starting" ||
                            startState.status === "success"
                          }
                          onChange={() => onToggleFile(fileId)}
                          type="checkbox"
                        />
                        <span>{file.path}</span>
                        <span>
                          {formatStorageBytes(BigInt(file.sizeBytes))} (
                          {file.sizeBytes} bytes)
                        </span>
                      </label>
                    </li>
                  ))}
                </ul>
              </fieldset>
              <div className="vr-torrent__destination">
                <p>{folderMessage}</p>
                {!isFolderReady ? (
                  <Button onClick={onOpenSettings} type="button" variant="outline">
                    <AppIcon name="settings" />
                    Open Settings
                  </Button>
                ) : null}
              </div>
              <div className="vr-torrent__actions">
                <Button
                  disabled={
                    selectedFileIds.size === 0 ||
                    startState.status === "starting" ||
                    startState.status === "success" ||
                    !isFolderReady ||
                    !downloadsReady
                  }
                  onClick={onStart}
                  type="button"
                >
                  <AppIcon name="downloads" />
                  {startState.status === "starting" ? "Starting…" : "Start download"}
                </Button>
                <Button
                  disabled={
                    saveState === "saving" || startState.status === "starting"
                  }
                  onClick={onSave}
                  type="button"
                  variant="outline"
                >
                  <AppIcon name="downloads" />
                  {saveState === "saving" ? "Saving…" : "Save `.torrent`"}
                </Button>
                {saveState === "success" ? (
                  <p role="status">Verified torrent file saved.</p>
                ) : saveState === "error" ? (
                  <p role="alert">The verified torrent file could not be saved.</p>
                ) : null}
                {startState.status === "success" ? (
                  <div className="vr-torrent__start-result" role="status">
                    <p>Selected files were added to Downloads.</p>
                    <Button onClick={onOpenDownloads} type="button" variant="outline">
                      View Downloads
                    </Button>
                  </div>
                ) : startState.status === "error" ? (
                  <p role="alert">{startState.message}</p>
                ) : null}
              </div>
            </div>
          ) : (
            <div className="vr-torrent__state" role={currentMessage.role}>
              <span className="empty-state__icon">
                <AppIcon name="releases" />
              </span>
              <div>
                <h3>{currentMessage.heading}</h3>
                <p>{currentMessage.message}</p>
                {state.status === "loading" ? null : (
                  <Button onClick={onRetry} type="button" variant="outline">
                    <AppIcon name="refresh" />
                    Retry inspection
                  </Button>
                )}
              </div>
            </div>
          )}
        </Dialog.Popup>
      </Dialog.Viewport>
    </Dialog.Portal>
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

function VrLibraryFileRow({ file }: { file: VrLibraryFile }) {
  const [pendingAction, setPendingAction] = useState<"open" | "reveal" | null>(
    null,
  );
  const [actionError, setActionError] = useState<string | null>(null);
  const actionPending = useRef(false);

  const runAction = async (action: "open" | "reveal") => {
    if (actionPending.current) {
      return;
    }
    actionPending.current = true;
    setPendingAction(action);
    setActionError(null);
    try {
      if (action === "open") {
        await openVrFile(file.path);
      } else {
        await revealVrFile(file.path);
      }
    } catch (error: unknown) {
      const errorCode = nativeErrorCode(error);
      setActionError(
        action === "open"
          ? (vrFileOpenErrorMessages[errorCode] ??
              "Auto-Video could not open this VR file.")
          : (vrFileRevealErrorMessages[errorCode] ??
              "Auto-Video could not reveal this VR file."),
      );
    } finally {
      actionPending.current = false;
      setPendingAction(null);
    }
  };

  return (
    <li className="vr-library-file" data-vr-file-path={file.path}>
      <div className="vr-library-file__identity">
        <span title={file.path}>{file.path}</span>
        <small>
          {file.partLabel === null ? null : `${file.partLabel} · `}
          {formatStorageBytes(BigInt(file.sizeBytes))}
        </small>
      </div>
      <div className="vr-library-file__actions">
        <Button
          aria-label={`${pendingAction === "open" ? "Opening" : "Open"} VR file: ${file.path}`}
          disabled={pendingAction !== null}
          onClick={() => void runAction("open")}
          size="icon-xs"
          title="Open file"
          type="button"
          variant="outline"
        >
          <AppIcon name="open" />
        </Button>
        <Button
          aria-label={`${pendingAction === "reveal" ? "Revealing" : "Reveal"} VR file: ${file.path}`}
          disabled={pendingAction !== null}
          onClick={() => void runAction("reveal")}
          size="icon-xs"
          title="Reveal file"
          type="button"
          variant="outline"
        >
          <AppIcon name="reveal" />
        </Button>
      </div>
      {actionError === null ? null : (
        <p aria-atomic="true" role="alert">
          {actionError}
        </p>
      )}
    </li>
  );
}

function VrLibraryCard({ item }: { item: VrLibraryItem }) {
  return (
    <article className="movie-card vr-library-card">
      <div className="media-title-row">
        <div>
          <p className="card-eyebrow">
            {item.code === null
              ? "Unassociated file"
              : `${item.files.length} ${item.files.length === 1 ? "file" : "files"}`}
          </p>
          <h3>{item.title}</h3>
        </div>
        <CopyTitleAction title={item.title} />
      </div>
      <ul aria-label={`Files for ${item.title}`} className="vr-library-card__files">
        {item.files.map((file) => (
          <VrLibraryFileRow file={file} key={file.path} />
        ))}
      </ul>
    </article>
  );
}

function TvLibraryFileRow({ file }: { file: TvLibraryFile }) {
  const [pendingAction, setPendingAction] = useState<"open" | "reveal" | null>(
    null,
  );
  const [actionError, setActionError] = useState<string | null>(null);
  const actionPending = useRef(false);

  const runAction = async (action: "open" | "reveal") => {
    if (actionPending.current) {
      return;
    }
    actionPending.current = true;
    setPendingAction(action);
    setActionError(null);
    try {
      if (action === "open") {
        await openTvFile(file.path);
      } else {
        await revealTvFile(file.path);
      }
    } catch (error: unknown) {
      const errorCode = nativeErrorCode(error);
      setActionError(
        action === "open"
          ? (tvFileOpenErrorMessages[errorCode] ??
              "Auto-Video could not open this TV file.")
          : (tvFileRevealErrorMessages[errorCode] ??
              "Auto-Video could not reveal this TV file."),
      );
    } finally {
      actionPending.current = false;
      setPendingAction(null);
    }
  };

  return (
    <li className="vr-library-file" data-tv-file-path={file.path}>
      <div className="vr-library-file__identity">
        <span title={file.path}>{file.filename}</span>
        <small>
          {file.season === null || file.episode === null
            ? "Unassociated"
            : `Season ${file.season} · Episode ${file.episode}`}
          {` · ${formatStorageBytes(BigInt(file.sizeBytes))}`}
        </small>
      </div>
      <div className="vr-library-file__actions">
        <Button
          aria-label={`${pendingAction === "open" ? "Opening" : "Open"} TV file: ${file.filename}`}
          disabled={pendingAction !== null}
          onClick={() => void runAction("open")}
          size="icon-xs"
          title="Open file"
          type="button"
          variant="outline"
        >
          <AppIcon name="open" />
        </Button>
        <Button
          aria-label={`${pendingAction === "reveal" ? "Revealing" : "Reveal"} TV file: ${file.filename}`}
          disabled={pendingAction !== null}
          onClick={() => void runAction("reveal")}
          size="icon-xs"
          title="Reveal file"
          type="button"
          variant="outline"
        >
          <AppIcon name="reveal" />
        </Button>
      </div>
      {actionError === null ? null : (
        <p aria-atomic="true" role="alert">
          {actionError}
        </p>
      )}
    </li>
  );
}

function TvLibraryCard({ item }: { item: TvLibraryItem }) {
  return (
    <article className="movie-card vr-library-card tv-library-card">
      <div className="media-title-row">
        <div>
          <p className="card-eyebrow">
            {item.showTitle === null
              ? "Unassociated file"
              : `${item.files.length} ${item.files.length === 1 ? "episode" : "episodes"}`}
          </p>
          <h3>{item.title}</h3>
        </div>
        <CopyTitleAction title={item.title} />
      </div>
      <ul
        aria-label={
          item.showTitle === null
            ? `File details for ${item.title}`
            : `Episodes for ${item.title}`
        }
        className="vr-library-card__files"
      >
        {item.files.map((file) => (
          <TvLibraryFileRow file={file} key={file.path} />
        ))}
      </ul>
    </article>
  );
}

function VrDownloadCard({
  download,
  error,
  isPending,
  organizationPreview,
  onApplyOrganization,
  onCancel,
  onCloseOrganization,
  onDismiss,
  onPause,
  onPreviewOrganization,
  onResume,
}: {
  download: VrDownload;
  error: string | null;
  isPending: boolean;
  organizationPreview: VrOrganizationPreview | null;
  onApplyOrganization: () => void;
  onCancel: () => void;
  onCloseOrganization: () => void;
  onDismiss: () => void;
  onPause: () => void;
  onPreviewOrganization: () => void;
  onResume: () => void;
}) {
  const organizationCancelButton = useRef<HTMLButtonElement | null>(null);
  const totalBytes = BigInt(download.totalBytes);
  const downloadedBytes = BigInt(download.downloadedBytes);
  const percent =
    totalBytes === 0n ? 0 : Number((downloadedBytes * 100n) / totalBytes);
  const stateLabel =
    download.organizationStatus === "organized"
      ? "Organized"
      : download.organizationStatus === "attention"
        ? "Organization needs attention"
        : download.state.charAt(0).toUpperCase() + download.state.slice(1);
  const stateClass =
    download.organizationStatus === "attention"
      ? "attention"
      : download.organizationStatus === "organized"
        ? "organized"
        : download.state;
  const isTerminal = !activeVrDownloadStates.has(download.state);

  return (
    <article
      aria-labelledby={`vr-download-${download.transferId}`}
      className="vr-download-card"
    >
      <div className="vr-download-card__heading">
        <div>
          <p className="card-eyebrow">{download.code}</p>
          <h2 id={`vr-download-${download.transferId}`}>
            {download.releaseName}
          </h2>
        </div>
        <span
          className={`vr-download-card__state is-${stateClass}`}
          role={download.organizationStatus === "none" ? undefined : "status"}
        >
          {stateLabel}
        </span>
      </div>
      <div className="vr-download-card__progress">
        <progress
          aria-label={`${download.code} selected-file download progress`}
          max={100}
          value={percent}
        />
        <div>
          <span>{percent}%</span>
          <span>
            {formatStorageBytes(downloadedBytes)} of {formatStorageBytes(totalBytes)}
          </span>
        </div>
      </div>
      <dl className="vr-download-card__metadata">
        <div>
          <dt>Selected files</dt>
          <dd>{download.selectedFileCount}</dd>
        </div>
        <div>
          <dt>Speed</dt>
          <dd>
            {formatStorageBytes(BigInt(download.speedBytesPerSecond))}/s
          </dd>
        </div>
        {download.organizationStatus === "none" ? null : (
          <div>
            <dt>
              {download.organizationStatus === "organized"
                ? "Organized location"
                : "Organization"}
            </dt>
            <dd>
              {download.organizationStatus === "organized"
                ? download.organizationRelativeDirectory
                : "Needs attention"}
            </dd>
          </div>
        )}
      </dl>
      <div className="vr-download-card__actions">
        {download.state === "downloading" ? (
          <Button
            disabled={isPending}
            id={`vr-download-pause-${download.transferId}`}
            onClick={onPause}
            type="button"
            variant="outline"
          >
            <AppIcon name="pause" />
            {isPending ? "Pausing…" : "Pause"}
          </Button>
        ) : download.state === "paused" ? (
          <Button
            disabled={isPending}
            id={`vr-download-resume-${download.transferId}`}
            onClick={onResume}
            type="button"
            variant="outline"
          >
            <AppIcon name="brand" />
            {isPending ? "Resuming…" : "Resume"}
          </Button>
        ) : null}
        {download.canOrganize ? (
          <AlertDialog.Root
            onOpenChange={(open) => {
              if (open && organizationPreview === null) {
                onPreviewOrganization();
              } else if (!open && !isPending) {
                onCloseOrganization();
              }
            }}
            open={organizationPreview !== null}
            triggerId={
              organizationPreview === null
                ? null
                : `vr-download-organize-${download.transferId}`
            }
          >
            <AlertDialog.Trigger
              id={`vr-download-organize-${download.transferId}`}
              render={
                <Button disabled={isPending} type="button" variant="outline">
                  <AppIcon name="folder" />
                  {isPending && organizationPreview === null
                    ? "Preparing…"
                    : "Organize files"}
                </Button>
              }
            />
            {organizationPreview === null ? null : (
              <AlertDialog.Portal>
                <AlertDialog.Backdrop
                  className="trash-dialog__backdrop"
                  onClick={(event) => {
                    event.stopPropagation();
                    onCloseOrganization();
                  }}
                  onPointerDown={(event) => event.stopPropagation()}
                />
                <AlertDialog.Viewport className="trash-dialog__viewport">
                  <AlertDialog.Popup
                    aria-busy={isPending}
                    className="trash-dialog__popup vr-organization-dialog"
                    initialFocus={() => organizationCancelButton.current}
                  >
                    <div className="trash-dialog__heading">
                      <AlertDialog.Title>
                        Organize {organizationPreview.code} files?
                      </AlertDialog.Title>
                      <AlertDialog.Close
                        render={
                          <Button
                            aria-label="Close organization preview"
                            disabled={isPending}
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
                      Confirm this exact plan. {organizationPreview.moveCount}{" "}
                      {organizationPreview.moveCount === 1 ? "file" : "files"}
                      {" "}will move within the current VR folder.
                    </AlertDialog.Description>
                    <ul
                      aria-label={`Organization plan for ${organizationPreview.code}`}
                      className="vr-organization-dialog__files"
                    >
                      {organizationPreview.entries.map((entry) => (
                        <li key={entry.sourceRelativePath}>
                          <span>{entry.sourceRelativePath}</span>
                          <span>
                            {entry.kind === "non-media-unchanged"
                              ? "Unchanged non-media file"
                              : entry.kind === "media-unchanged"
                                ? `Already canonical: ${entry.destinationRelativePath}`
                                : `Move to: ${entry.destinationRelativePath}`}
                          </span>
                        </li>
                      ))}
                    </ul>
                    <div className="trash-dialog__actions">
                      <AlertDialog.Close
                        render={
                          <Button
                            disabled={isPending}
                            ref={organizationCancelButton}
                            type="button"
                            variant="outline"
                          >
                            Cancel
                          </Button>
                        }
                      />
                      <Button
                        disabled={isPending}
                        onClick={onApplyOrganization}
                        type="button"
                      >
                        <AppIcon name="folder" />
                        {isPending
                          ? "Organizing…"
                          : `Organize ${organizationPreview.moveCount} ${
                              organizationPreview.moveCount === 1
                                ? "file"
                                : "files"
                            }`}
                      </Button>
                    </div>
                  </AlertDialog.Popup>
                </AlertDialog.Viewport>
              </AlertDialog.Portal>
            )}
          </AlertDialog.Root>
        ) : null}
        {!isTerminal ? (
          <AlertDialog.Root>
            <AlertDialog.Trigger
              render={
                <Button
                  disabled={isPending}
                  id={`vr-download-cancel-${download.transferId}`}
                  type="button"
                  variant="outline"
                >
                  <AppIcon name="close" />
                  Cancel
                </Button>
              }
            />
            <AlertDialog.Portal>
              <AlertDialog.Backdrop className="trash-dialog__backdrop" />
              <AlertDialog.Viewport className="trash-dialog__viewport">
                <AlertDialog.Popup className="trash-dialog__popup">
                  <AlertDialog.Title>Cancel this download?</AlertDialog.Title>
                  <AlertDialog.Description>
                    The transfer will stop. Downloaded files and partial data
                    will remain in the VR folder.
                  </AlertDialog.Description>
                  <div className="trash-dialog__actions">
                    <AlertDialog.Close
                      render={
                        <Button type="button" variant="outline">
                          Keep downloading
                        </Button>
                      }
                    />
                    <AlertDialog.Close
                      render={
                        <Button
                          disabled={isPending}
                          onClick={onCancel}
                          type="button"
                          variant="destructive"
                        >
                          Cancel download
                        </Button>
                      }
                    />
                  </div>
                </AlertDialog.Popup>
              </AlertDialog.Viewport>
            </AlertDialog.Portal>
          </AlertDialog.Root>
        ) : (
          <Button
            disabled={isPending}
            id={`vr-download-dismiss-${download.transferId}`}
            onClick={onDismiss}
            type="button"
            variant="outline"
          >
            <AppIcon name="close" />
            {isPending ? "Dismissing…" : "Dismiss"}
          </Button>
        )}
      </div>
      {error === null ? null : (
        <p className="field-error" role="alert">
          {error}
        </p>
      )}
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

function compareVrLibraryItemsByTitle(
  leftItem: VrLibraryItem,
  rightItem: VrLibraryItem,
  direction: LibraryTitleSortDirection,
) {
  const leftTitle = leftItem.title.toLowerCase();
  const rightTitle = rightItem.title.toLowerCase();
  const titleOrder = leftTitle < rightTitle ? -1 : leftTitle > rightTitle ? 1 : 0;
  if (titleOrder !== 0) {
    return direction === "ascending" ? titleOrder : -titleOrder;
  }
  if (leftItem.title !== rightItem.title) {
    return leftItem.title < rightItem.title ? -1 : 1;
  }
  return leftItem.id < rightItem.id ? -1 : leftItem.id > rightItem.id ? 1 : 0;
}

function compareTvLibraryItemsByTitle(
  leftItem: TvLibraryItem,
  rightItem: TvLibraryItem,
  direction: LibraryTitleSortDirection,
) {
  const leftTitle = leftItem.title.toLowerCase();
  const rightTitle = rightItem.title.toLowerCase();
  const titleOrder = leftTitle < rightTitle ? -1 : leftTitle > rightTitle ? 1 : 0;
  if (titleOrder !== 0) {
    return direction === "ascending" ? titleOrder : -titleOrder;
  }
  if (leftItem.title !== rightItem.title) {
    return leftItem.title < rightItem.title ? -1 : 1;
  }
  return leftItem.id < rightItem.id ? -1 : leftItem.id > rightItem.id ? 1 : 0;
}

function summarizeVrDownloads(downloads: VrDownload[]): VrDownloadSummary {
  let activeCount = 0;
  let pausedCount = 0;
  let completedCount = 0;
  let attentionCount = 0;
  let aggregateSpeedBytesPerSecond = 0n;
  for (const download of downloads) {
    if (download.state === "downloading") {
      activeCount += 1;
      aggregateSpeedBytesPerSecond += BigInt(download.speedBytesPerSecond);
    } else if (download.state === "paused") {
      pausedCount += 1;
    } else if (download.state === "completed") {
      completedCount += 1;
    }
    if (
      download.state === "offline" ||
      download.state === "failed" ||
      download.organizationStatus === "attention"
    ) {
      attentionCount += 1;
    }
  }
  return {
    activeCount,
    pausedCount,
    completedCount,
    attentionCount,
    aggregateSpeedBytesPerSecond,
  };
}

function formatVrDownloadLimit(limit: VrDownloadLimit) {
  return limit.mibPerSecond === null
    ? "Unlimited"
    : `${limit.mibPerSecond} MiB/s`;
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

function nativeErrorCode(error: unknown) {
  return typeof error === "string"
    ? error
    : error instanceof Error
      ? error.message
      : "";
}

function vrDownloadStartError(error: unknown) {
  switch (nativeErrorCode(error)) {
    case "vr_download_destination_conflict":
      return "A selected file already exists in the VR folder. Nothing was overwritten.";
    case "vr_download_duplicate":
      return "This torrent is already active in the configured VR folder.";
    case "vr_folder_unavailable":
      return "The configured VR folder is unavailable. Check it in Settings.";
    case "vr_download_stale":
    case "vr_download_context_invalid":
      return "This inspection or file selection is no longer current. Inspect the release again.";
    case "vr_download_persistence_failed":
      return "The transfer could not be saved locally, so it was not started.";
    default:
      return "The selected-file download could not be started.";
  }
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
    useState<VolumeStorageState>({ status: "unconfigured" });
  const [librarySelectedPage, setLibrarySelectedPage] = useState(1);
  const [librarySearchQuery, setLibrarySearchQuery] = useState("");
  const [libraryTitleSortDirection, setLibraryTitleSortDirection] =
    useState<LibraryTitleSortDirection>("ascending");
  const [libraryCategory, setLibraryCategory] =
    useState<LibraryCategory>("movies");
  const [tvFolderState, setTvFolderState] = useState<TvFolderUiState>({
    status: "loading",
  });
  const [tvLibraryScanState, setTvLibraryScanState] =
    useState<TvLibraryScanState>({ status: "loading" });
  const [tvLibraryRefreshVersion, setTvLibraryRefreshVersion] = useState(0);
  const [tvStorageRefreshVersion, setTvStorageRefreshVersion] = useState(0);
  const [tvStorageState, setTvStorageState] = useState<VolumeStorageState>({
    status: "unconfigured",
  });
  const [tvLibrarySelectedPage, setTvLibrarySelectedPage] = useState(1);
  const [tvLibrarySearchQuery, setTvLibrarySearchQuery] = useState("");
  const [tvLibraryTitleSortDirection, setTvLibraryTitleSortDirection] =
    useState<LibraryTitleSortDirection>("ascending");
  const [isChoosingTvFolder, setIsChoosingTvFolder] = useState(false);
  const [isRevalidatingTvFolder, setIsRevalidatingTvFolder] = useState(false);
  const [tvFolderActionError, setTvFolderActionError] = useState<string | null>(
    null,
  );
  const [vrLibraryScanState, setVrLibraryScanState] =
    useState<VrLibraryScanState>({ status: "loading" });
  const [vrLibraryRefreshVersion, setVrLibraryRefreshVersion] = useState(0);
  const [vrStorageRefreshVersion, setVrStorageRefreshVersion] = useState(0);
  const [vrStorageState, setVrStorageState] = useState<VolumeStorageState>({
    status: "unconfigured",
  });
  const [vrLibrarySelectedPage, setVrLibrarySelectedPage] = useState(1);
  const [vrLibrarySearchQuery, setVrLibrarySearchQuery] = useState("");
  const [vrLibraryTitleSortDirection, setVrLibraryTitleSortDirection] =
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
  const [discoverCategory, setDiscoverCategory] =
    useState<DiscoverCategory>("movies");
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
  const [vrSearchInput, setVrSearchInput] = useState("");
  const [vrSearchInputError, setVrSearchInputError] = useState<string | null>(
    null,
  );
  const [submittedVrCode, setSubmittedVrCode] = useState<string | null>(null);
  const [vrCatalogState, setVrCatalogState] = useState<VrCatalogState>({
    status: "idle",
  });
  const [vrCatalogRequestVersion, setVrCatalogRequestVersion] = useState(0);
  const [vrSelectedPage, setVrSelectedPage] = useState(1);
  const [releaseComparisonItem, setReleaseComparisonItem] =
    useState<VrCatalogItem | null>(null);
  const [releaseComparisonState, setReleaseComparisonState] =
    useState<VrReleaseComparisonState | null>(null);
  const [releaseComparisonTriggerId, setReleaseComparisonTriggerId] =
    useState<string | null>(null);
  const [selectedVrRelease, setSelectedVrRelease] =
    useState<VrRelease | null>(null);
  const [releaseRequestVersion, setReleaseRequestVersion] = useState(0);
  const [torrentInspectionContext, setTorrentInspectionContext] =
    useState<VrTorrentInspectionContext | null>(null);
  const [torrentInspectionState, setTorrentInspectionState] =
    useState<VrTorrentInspectionState | null>(null);
  const [torrentInspectionRequestVersion, setTorrentInspectionRequestVersion] =
    useState(0);
  const [torrentSaveState, setTorrentSaveState] =
    useState<VrTorrentSaveState>("idle");
  const [torrentStartState, setTorrentStartState] =
    useState<VrTorrentStartState>({ status: "idle" });
  const [selectedTorrentFileIds, setSelectedTorrentFileIds] = useState<
    Set<number>
  >(new Set());
  const [vrFolderState, setVrFolderState] = useState<VrFolderUiState>({
    status: "loading",
  });
  const [isChoosingVrFolder, setIsChoosingVrFolder] = useState(false);
  const [isRevalidatingVrFolder, setIsRevalidatingVrFolder] = useState(false);
  const [vrFolderActionError, setVrFolderActionError] = useState<string | null>(
    null,
  );
  const [vrDownloadsState, setVrDownloadsState] =
    useState<VrDownloadsUiState>({ status: "loading" });
  const [vrDownloadLimitState, setVrDownloadLimitState] =
    useState<VrDownloadLimitUiState>({ status: "loading" });
  const [vrDownloadLimitMode, setVrDownloadLimitMode] =
    useState<VrDownloadLimitMode>("unlimited");
  const [vrDownloadLimitInput, setVrDownloadLimitInput] = useState("");
  const [isSavingVrDownloadLimit, setIsSavingVrDownloadLimit] = useState(false);
  const [vrDownloadLimitMessage, setVrDownloadLimitMessage] =
    useState<CredentialMessage | null>(null);
  const [pendingVrDownloadIds, setPendingVrDownloadIds] = useState<Set<string>>(
    new Set(),
  );
  const [vrDownloadErrors, setVrDownloadErrors] = useState<
    Record<string, string>
  >({});
  const [vrOrganizationPreview, setVrOrganizationPreview] =
    useState<VrOrganizationPreview | null>(null);
  const [vrDownloadFocusTarget, setVrDownloadFocusTarget] = useState<
    string | null
  >(null);
  const navigationItems = useRef<Array<HTMLButtonElement | null>>([]);
  const workspace = useRef<HTMLElement | null>(null);
  const scanRequestId = useRef(0);
  const storageRequestId = useRef(0);
  const discoverRequestId = useRef(0);
  const movieDetailsRequestId = useRef(0);
  const vrCatalogRequestId = useRef(0);
  const releaseRequestId = useRef(0);
  const torrentInspectionRequestId = useRef(0);
  const torrentSaveRequestId = useRef(0);
  const torrentStartRequestId = useRef(0);
  const vrDownloadsRequestId = useRef(0);
  const vrDownloadLimitRequestId = useRef(0);
  const vrOrganizationRequestId = useRef(0);
  const vrOrganizationPreviewPending = useRef(false);
  const vrFolderRequestId = useRef(0);
  const vrLibraryScanRequestId = useRef(0);
  const vrStorageRequestId = useRef(0);
  const tvFolderRequestId = useRef(0);
  const tvLibraryScanRequestId = useRef(0);
  const tvStorageRequestId = useRef(0);
  const torrentSavePending = useRef(false);
  const torrentStartPending = useRef(false);
  const vrDownloadsRefreshPending = useRef(false);
  const vrDownloadLimitSavePending = useRef(false);
  const vrDownloadActionsPending = useRef(new Set<string>());
  const trendingDiscoverResult = useRef<{
    refreshVersion: number;
    result: TmdbMoviesResult;
  } | null>(null);
  const currentMoviesFolder = useRef(moviesFolder);
  const currentMovieScanState = useRef(movieScanState);
  const currentVrDownloadsState = useRef(vrDownloadsState);
  const previousVrDownloadStates = useRef<Map<string, VrDownload["state"]>>(
    new Map(),
  );
  const hasObservedVrDownloads = useRef(false);
  const pendingCompletedVrRefresh = useRef(false);
  // Late Trash responses read current state so an old card cannot modify replacement results.
  currentMoviesFolder.current = moviesFolder;
  currentMovieScanState.current = movieScanState;
  currentVrDownloadsState.current = vrDownloadsState;

  useLayoutEffect(() => {
    if (vrDownloadFocusTarget === null) {
      return;
    }
    const activeElement = document.activeElement;
    const actionCanRestoreFocus =
      activeElement === null ||
      activeElement === document.body ||
      !activeElement.isConnected;
    const terminalActionRequiresFocus =
      vrDownloadFocusTarget === "vr-downloads-refresh" ||
      vrDownloadFocusTarget.startsWith("vr-download-dismiss-");
    if (actionCanRestoreFocus || terminalActionRequiresFocus) {
      document.getElementById(vrDownloadFocusTarget)?.focus();
    }
    setVrDownloadFocusTarget(null);
  }, [vrDownloadFocusTarget, vrDownloadsState]);

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
    const requestId = ++tvFolderRequestId.current;
    void loadTvFolder()
      .then((folderState) => {
        if (requestId === tvFolderRequestId.current) {
          setTvFolderState(folderState);
        }
      })
      .catch(() => {
        if (requestId === tvFolderRequestId.current) {
          setTvFolderState({ status: "error" });
        }
      });
    return () => {
      tvFolderRequestId.current += 1;
    };
  }, []);

  useEffect(() => {
    const requestId = ++vrFolderRequestId.current;
    void loadVrFolder()
      .then((folderState) => {
        if (requestId === vrFolderRequestId.current) {
          setVrFolderState(folderState);
        }
      })
      .catch(() => {
        if (requestId === vrFolderRequestId.current) {
          setVrFolderState({ status: "error" });
        }
      });
    return () => {
      vrFolderRequestId.current += 1;
    };
  }, []);

  useEffect(() => {
    const limitRequestId = ++vrDownloadLimitRequestId.current;
    const downloadsRequestId = ++vrDownloadsRequestId.current;
    void (async () => {
      let limit: VrDownloadLimit;
      try {
        limit = await loadVrDownloadLimit();
      } catch {
        if (
          limitRequestId === vrDownloadLimitRequestId.current &&
          downloadsRequestId === vrDownloadsRequestId.current
        ) {
          setVrDownloadLimitState({ status: "error" });
          setVrDownloadsState({ status: "error" });
        }
        return;
      }
      if (
        limitRequestId !== vrDownloadLimitRequestId.current ||
        downloadsRequestId !== vrDownloadsRequestId.current
      ) {
        return;
      }
      setVrDownloadLimitState({ status: "ready", limit });
      setVrDownloadLimitMode(
        limit.mibPerSecond === null ? "unlimited" : "limited",
      );
      setVrDownloadLimitInput(limit.mibPerSecond ?? "");
      try {
        const downloads = await loadVrDownloads();
        if (downloadsRequestId === vrDownloadsRequestId.current) {
          setVrDownloadsState({ status: "ready", downloads });
        }
      } catch {
        if (downloadsRequestId === vrDownloadsRequestId.current) {
          setVrDownloadsState({ status: "error" });
        }
      }
    })();
    return () => {
      vrDownloadLimitRequestId.current += 1;
      vrDownloadsRequestId.current += 1;
    };
  }, []);

  useEffect(() => {
    if (
      vrDownloadsState.status !== "ready" ||
      !vrDownloadsState.downloads.some(
        (download) =>
          download.state === "queued" || download.state === "downloading",
      )
    ) {
      return;
    }

    const interval = window.setInterval(() => {
      if (vrDownloadsRefreshPending.current) {
        return;
      }
      vrDownloadsRefreshPending.current = true;
      const requestId = ++vrDownloadsRequestId.current;
      void listVrDownloads()
        .then((downloads) => {
          if (requestId === vrDownloadsRequestId.current) {
            setVrDownloadsState({ status: "ready", downloads });
          }
        })
        .catch(() => {
          if (requestId === vrDownloadsRequestId.current) {
            setVrDownloadsState({ status: "error" });
          }
        })
        .finally(() => {
          vrDownloadsRefreshPending.current = false;
        });
    }, vrDownloadRefreshInterval);
    return () => window.clearInterval(interval);
  }, [vrDownloadsState]);

  useEffect(() => {
    if (vrDownloadsState.status !== "ready") {
      return;
    }
    const nextStates = new Map(
      vrDownloadsState.downloads.map((download) => [
        download.transferId,
        download.state,
      ]),
    );
    const completedTransferAppeared =
      hasObservedVrDownloads.current &&
      vrDownloadsState.downloads.some(
        (download) =>
          download.state === "completed" &&
          download.isCurrentFolder &&
          previousVrDownloadStates.current.get(download.transferId) !==
            "completed",
      );
    previousVrDownloadStates.current = nextStates;
    hasObservedVrDownloads.current = true;
    if (completedTransferAppeared) {
      pendingCompletedVrRefresh.current = true;
    }
    if (
      pendingCompletedVrRefresh.current &&
      vrFolderState.status === "ready"
    ) {
      pendingCompletedVrRefresh.current = false;
      setVrLibraryRefreshVersion((version) => version + 1);
      setVrStorageRefreshVersion((version) => version + 1);
    }
  }, [vrDownloadsState, vrFolderState.status]);

  useEffect(() => {
    const requestId = ++vrLibraryScanRequestId.current;
    if (vrFolderState.status === "loading") {
      setVrLibraryScanState({ status: "loading" });
      return;
    }
    if (vrFolderState.status === "unconfigured") {
      setVrLibraryScanState({ status: "unconfigured" });
      return;
    }
    if (vrFolderState.status === "unavailable") {
      setVrLibraryScanState({ status: "unavailable" });
      return;
    }
    if (vrFolderState.status === "error") {
      setVrLibraryScanState({ status: "error" });
      return;
    }

    setVrLibraryScanState({ status: "scanning" });
    void scanVrLibrary()
      .then((items) => {
        if (requestId !== vrLibraryScanRequestId.current) {
          return;
        }
        setVrLibraryScanState(
          items.length === 0 ? { status: "empty" } : { status: "ready", items },
        );
      })
      .catch((error: unknown) => {
        if (requestId !== vrLibraryScanRequestId.current) {
          return;
        }
        setVrLibraryScanState({
          status:
            nativeErrorCode(error) === "vr_library_folder_unavailable"
              ? "unavailable"
              : "error",
        });
      });

    return () => {
      vrLibraryScanRequestId.current += 1;
    };
  }, [vrFolderState, vrLibraryRefreshVersion]);

  useEffect(() => {
    const requestId = ++vrStorageRequestId.current;
    if (vrFolderState.status === "loading") {
      setVrStorageState({ status: "loading" });
      return;
    }
    if (vrFolderState.status === "unconfigured") {
      setVrStorageState({ status: "unconfigured" });
      return;
    }
    if (vrFolderState.status === "unavailable") {
      setVrStorageState({ status: "unavailable" });
      return;
    }
    if (vrFolderState.status === "error") {
      setVrStorageState({ status: "error" });
      return;
    }

    setVrStorageState({ status: "loading" });
    void window.__TAURI__.core
      .invoke<unknown>("query_vr_storage")
      .then((values) => {
        if (requestId !== vrStorageRequestId.current) {
          return;
        }
        if (
          !Array.isArray(values) ||
          values.length !== 2 ||
          values.some((value) => typeof value !== "string" || !/^\d+$/.test(value))
        ) {
          throw new Error("The native VR storage query returned invalid data.");
        }
        const totalBytes = BigInt(values[0]);
        const freeBytes = BigInt(values[1]);
        if (totalBytes === 0n || freeBytes > totalBytes) {
          throw new Error("The native VR storage values were inconsistent.");
        }
        setVrStorageState({ status: "ready", totalBytes, freeBytes });
      })
      .catch((error: unknown) => {
        if (requestId === vrStorageRequestId.current) {
          setVrStorageState({
            status:
              nativeErrorCode(error) === vrStorageUnavailable
                ? "unavailable"
                : "error",
          });
        }
      });

    return () => {
      vrStorageRequestId.current += 1;
    };
  }, [vrFolderState, vrStorageRefreshVersion]);

  useEffect(() => {
    const requestId = ++tvLibraryScanRequestId.current;
    if (tvFolderState.status === "loading") {
      setTvLibraryScanState({ status: "loading" });
      return;
    }
    if (tvFolderState.status === "unconfigured") {
      setTvLibraryScanState({ status: "unconfigured" });
      return;
    }
    if (tvFolderState.status === "unavailable") {
      setTvLibraryScanState({ status: "unavailable" });
      return;
    }
    if (tvFolderState.status === "error") {
      setTvLibraryScanState({ status: "error" });
      return;
    }

    setTvLibraryScanState({ status: "scanning" });
    void scanTvLibrary()
      .then((items) => {
        if (requestId === tvLibraryScanRequestId.current) {
          setTvLibraryScanState(
            items.length === 0
              ? { status: "empty" }
              : { status: "ready", items },
          );
        }
      })
      .catch((error: unknown) => {
        if (requestId === tvLibraryScanRequestId.current) {
          setTvLibraryScanState({
            status:
              nativeErrorCode(error) === "tv_folder_unavailable"
                ? "unavailable"
                : "error",
          });
        }
      });
    return () => {
      tvLibraryScanRequestId.current += 1;
    };
  }, [tvFolderState, tvLibraryRefreshVersion]);

  useEffect(() => {
    const requestId = ++tvStorageRequestId.current;
    if (tvFolderState.status === "loading") {
      setTvStorageState({ status: "loading" });
      return;
    }
    if (tvFolderState.status === "unconfigured") {
      setTvStorageState({ status: "unconfigured" });
      return;
    }
    if (tvFolderState.status === "unavailable") {
      setTvStorageState({ status: "unavailable" });
      return;
    }
    if (tvFolderState.status === "error") {
      setTvStorageState({ status: "error" });
      return;
    }

    setTvStorageState({ status: "loading" });
    void queryTvStorage()
      .then(({ totalBytes, freeBytes }) => {
        if (requestId === tvStorageRequestId.current) {
          setTvStorageState({ status: "ready", totalBytes, freeBytes });
        }
      })
      .catch((error: unknown) => {
        if (requestId === tvStorageRequestId.current) {
          setTvStorageState({
            status:
              nativeErrorCode(error) === tvStorageUnavailable
                ? "unavailable"
                : "error",
          });
        }
      });
    return () => {
      tvStorageRequestId.current += 1;
    };
  }, [tvFolderState, tvStorageRefreshVersion]);

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
    if (activeDestination.id !== "downloads") {
      vrOrganizationRequestId.current += 1;
      if (vrOrganizationPreview !== null) {
        void dismissVrOrganization();
      }
      setVrOrganizationPreview(null);
    }
  }, [activeDestination.id, vrOrganizationPreview]);

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

  useEffect(() => {
    const requestId = ++vrCatalogRequestId.current;
    if (submittedVrCode === null) {
      return;
    }

    setVrCatalogState({ status: "loading" });
    void fetchExactJavdbVrItem(submittedVrCode).then((result) => {
      if (requestId === vrCatalogRequestId.current) {
        setVrCatalogState(result);
      }
    });

    return () => {
      vrCatalogRequestId.current += 1;
    };
  }, [submittedVrCode, vrCatalogRequestVersion]);

  useEffect(() => {
    const requestId = ++releaseRequestId.current;
    if (releaseComparisonItem === null) {
      return;
    }

    setReleaseComparisonState({ status: "loading" });
    void fetchVerifiedSukebeiReleases(releaseComparisonItem.code).then(
      (result) => {
        if (requestId === releaseRequestId.current) {
          setReleaseComparisonState(result);
        }
      },
    );

    return () => {
      releaseRequestId.current += 1;
    };
  }, [releaseComparisonItem, releaseRequestVersion]);

  useEffect(() => {
    const requestId = ++torrentInspectionRequestId.current;
    if (torrentInspectionContext === null) {
      return;
    }

    setTorrentInspectionState({ status: "loading" });
    setTorrentSaveState("idle");
    void inspectVerifiedSukebeiTorrent(
      torrentInspectionContext.item.code,
      torrentInspectionContext.release,
    ).then((result) => {
      if (requestId === torrentInspectionRequestId.current) {
        setTorrentInspectionState(result);
      }
    });

    return () => {
      torrentInspectionRequestId.current += 1;
      void invalidateVerifiedVrTorrent().catch(() => undefined);
    };
  }, [torrentInspectionContext, torrentInspectionRequestVersion]);

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

  const chooseConfiguredTvFolder = async () => {
    if (isChoosingTvFolder) {
      return;
    }
    const requestId = ++tvFolderRequestId.current;
    setIsRevalidatingTvFolder(false);
    setTvFolderActionError(null);
    setIsChoosingTvFolder(true);
    try {
      const selectedFolder = await chooseTvFolder();
      if (requestId === tvFolderRequestId.current && selectedFolder !== null) {
        tvLibraryScanRequestId.current += 1;
        tvStorageRequestId.current += 1;
        setTvLibraryScanState({ status: "scanning" });
        setTvStorageState({ status: "loading" });
        setTvFolderState({ status: "ready", path: selectedFolder });
      }
    } catch {
      if (requestId === tvFolderRequestId.current) {
        setTvFolderActionError("The TV folder picker could not be opened.");
      }
    } finally {
      setIsChoosingTvFolder(false);
    }
  };

  const clearConfiguredTvFolder = async () => {
    const requestId = ++tvFolderRequestId.current;
    setIsRevalidatingTvFolder(false);
    tvLibraryScanRequestId.current += 1;
    tvStorageRequestId.current += 1;
    setTvFolderActionError(null);
    try {
      await clearTvFolder();
      if (requestId === tvFolderRequestId.current) {
        setTvFolderState({ status: "unconfigured" });
      }
    } catch {
      if (requestId === tvFolderRequestId.current) {
        setTvFolderActionError(
          "The TV folder configuration could not be cleared.",
        );
      }
    }
  };

  const refreshTvLibrary = () => {
    if (tvFolderState.status === "unavailable") {
      if (isRevalidatingTvFolder) {
        return;
      }
      const requestId = ++tvFolderRequestId.current;
      tvLibraryScanRequestId.current += 1;
      tvStorageRequestId.current += 1;
      setIsRevalidatingTvFolder(true);
      void loadTvFolder()
        .then((folderState) => {
          if (requestId === tvFolderRequestId.current) {
            setTvFolderState(folderState);
          }
        })
        .catch(() => {
          if (requestId === tvFolderRequestId.current) {
            setTvFolderState({ status: "error" });
          }
        })
        .finally(() => {
          if (requestId === tvFolderRequestId.current) {
            setIsRevalidatingTvFolder(false);
          }
        });
      return;
    }
    if (tvFolderState.status !== "ready") {
      return;
    }
    tvLibraryScanRequestId.current += 1;
    tvStorageRequestId.current += 1;
    setTvLibraryScanState({ status: "scanning" });
    setTvStorageState({ status: "loading" });
    setTvLibraryRefreshVersion((version) => version + 1);
    setTvStorageRefreshVersion((version) => version + 1);
  };

  const updateTvLibrarySearchQuery = (query: string) => {
    setTvLibrarySearchQuery(query);
    setTvLibrarySelectedPage(1);
  };

  const updateTvLibraryTitleSortDirection = (
    direction: LibraryTitleSortDirection,
  ) => {
    setTvLibraryTitleSortDirection(direction);
    setTvLibrarySelectedPage(1);
  };

  const chooseConfiguredVrFolder = async () => {
    if (isChoosingVrFolder) {
      return;
    }
    const requestId = ++vrFolderRequestId.current;
    setIsRevalidatingVrFolder(false);
    setVrFolderActionError(null);
    setIsChoosingVrFolder(true);
    try {
      const selectedFolder = await chooseVrFolder();
      if (
        requestId === vrFolderRequestId.current &&
        selectedFolder !== null
      ) {
        vrOrganizationRequestId.current += 1;
        setVrOrganizationPreview(null);
        vrLibraryScanRequestId.current += 1;
        vrStorageRequestId.current += 1;
        setVrLibraryScanState({ status: "scanning" });
        setVrStorageState({ status: "loading" });
        setVrFolderState({ status: "ready", path: selectedFolder });
      }
    } catch {
      if (requestId === vrFolderRequestId.current) {
        setVrFolderActionError("The VR folder picker could not be opened.");
      }
    } finally {
      setIsChoosingVrFolder(false);
    }
  };

  const clearConfiguredVrFolder = async () => {
    const requestId = ++vrFolderRequestId.current;
    vrOrganizationRequestId.current += 1;
    setVrOrganizationPreview(null);
    setIsRevalidatingVrFolder(false);
    vrLibraryScanRequestId.current += 1;
    vrStorageRequestId.current += 1;
    setVrFolderActionError(null);
    try {
      await clearVrFolder();
      if (requestId === vrFolderRequestId.current) {
        setVrFolderState({ status: "unconfigured" });
      }
    } catch {
      if (requestId === vrFolderRequestId.current) {
        setVrFolderActionError(
          "The VR folder configuration could not be cleared.",
        );
      }
    }
  };

  const refreshVrLibrary = () => {
    if (vrFolderState.status === "unavailable") {
      if (isRevalidatingVrFolder) {
        return;
      }
      const requestId = ++vrFolderRequestId.current;
      vrLibraryScanRequestId.current += 1;
      vrStorageRequestId.current += 1;
      setIsRevalidatingVrFolder(true);
      void loadVrFolder()
        .then((folderState) => {
          if (requestId === vrFolderRequestId.current) {
            setVrFolderState(folderState);
          }
        })
        .catch(() => {
          if (requestId === vrFolderRequestId.current) {
            setVrFolderState({ status: "error" });
          }
        })
        .finally(() => {
          if (requestId === vrFolderRequestId.current) {
            setIsRevalidatingVrFolder(false);
          }
        });
      return;
    }
    if (vrFolderState.status !== "ready") {
      return;
    }
    vrLibraryScanRequestId.current += 1;
    vrStorageRequestId.current += 1;
    setVrLibraryScanState({ status: "scanning" });
    setVrStorageState({ status: "loading" });
    setVrLibraryRefreshVersion((version) => version + 1);
    setVrStorageRefreshVersion((version) => version + 1);
  };

  const updateVrLibrarySearchQuery = (query: string) => {
    setVrLibrarySearchQuery(query);
    setVrLibrarySelectedPage(1);
  };

  const updateVrLibraryTitleSortDirection = (
    direction: LibraryTitleSortDirection,
  ) => {
    setVrLibraryTitleSortDirection(direction);
    setVrLibrarySelectedPage(1);
  };

  const refreshVrDownloads = async () => {
    const requestId = ++vrDownloadsRequestId.current;
    try {
      const downloads = await listVrDownloads();
      if (requestId === vrDownloadsRequestId.current) {
        setVrDownloadsState({ status: "ready", downloads });
      }
    } catch {
      if (requestId === vrDownloadsRequestId.current) {
        setVrDownloadsState({ status: "error" });
      }
    }
  };

  const retryVrDownloads = async () => {
    const limitRequestId =
      vrDownloadLimitState.status === "error"
        ? ++vrDownloadLimitRequestId.current
        : vrDownloadLimitRequestId.current;
    const downloadsRequestId = ++vrDownloadsRequestId.current;
    setVrDownloadsState({ status: "loading" });
    if (vrDownloadLimitState.status === "error") {
      setVrDownloadLimitState({ status: "loading" });
      setVrDownloadLimitMessage(null);
      try {
        const limit = await loadVrDownloadLimit();
        if (limitRequestId !== vrDownloadLimitRequestId.current) {
          return;
        }
        setVrDownloadLimitState({ status: "ready", limit });
        setVrDownloadLimitMode(
          limit.mibPerSecond === null ? "unlimited" : "limited",
        );
        setVrDownloadLimitInput(limit.mibPerSecond ?? "");
      } catch {
        if (limitRequestId === vrDownloadLimitRequestId.current) {
          setVrDownloadLimitState({ status: "error" });
          setVrDownloadsState({ status: "error" });
        }
        return;
      }
    }
    try {
      const downloads = await loadVrDownloads();
      if (downloadsRequestId === vrDownloadsRequestId.current) {
        setVrDownloadsState({ status: "ready", downloads });
      }
    } catch {
      if (downloadsRequestId === vrDownloadsRequestId.current) {
        setVrDownloadsState({ status: "error" });
      }
    }
  };

  const saveConfiguredVrDownloadLimit = async (event: FormEvent) => {
    event.preventDefault();
    if (
      vrDownloadLimitSavePending.current ||
      vrDownloadLimitState.status !== "ready"
    ) {
      return;
    }
    const mibPerSecond = vrDownloadLimitInput.trim();
    if (
      vrDownloadLimitMode === "limited" &&
      (!/^[1-9]\d*$/.test(mibPerSecond) ||
        BigInt(mibPerSecond) > maximumVrDownloadLimitMibPerSecond)
    ) {
      setVrDownloadLimitMessage({
        role: "alert",
        text: "Enter a whole-number limit from 1 to 4095 MiB/s.",
      });
      return;
    }

    const requestId = ++vrDownloadLimitRequestId.current;
    vrDownloadLimitSavePending.current = true;
    setIsSavingVrDownloadLimit(true);
    setVrDownloadLimitMessage(null);
    try {
      const limit = await saveVrDownloadLimit(
        vrDownloadLimitMode === "limited" ? mibPerSecond : null,
      );
      if (requestId !== vrDownloadLimitRequestId.current) {
        return;
      }
      setVrDownloadLimitState({ status: "ready", limit });
      setVrDownloadLimitMode(
        limit.mibPerSecond === null ? "unlimited" : "limited",
      );
      setVrDownloadLimitInput(limit.mibPerSecond ?? "");
      setVrDownloadLimitMessage({
        role: "status",
        text:
          limit.mibPerSecond === null
            ? "VR downloads are now Unlimited."
            : `VR download limit applied at ${limit.mibPerSecond} MiB/s.`,
      });
    } catch (error: unknown) {
      if (requestId !== vrDownloadLimitRequestId.current) {
        return;
      }
      const message = (() => {
        switch (nativeErrorCode(error)) {
          case "vr_download_limit_invalid":
            return "Enter a whole-number limit from 1 to 4095 MiB/s.";
          case "vr_download_limit_storage_failed":
            return "The VR download limit could not be saved. The previous limit remains active.";
          case "vr_download_limit_apply_failed":
            return "The VR download limit could not be applied. The previous limit remains active.";
          default:
            return "The VR download limit is unavailable. Reload it before saving.";
        }
      })();
      setVrDownloadLimitMessage({ role: "alert", text: message });
    } finally {
      if (requestId === vrDownloadLimitRequestId.current) {
        vrDownloadLimitSavePending.current = false;
        setIsSavingVrDownloadLimit(false);
      }
    }
  };

  const organizationErrorMessage = (error: unknown) => {
    switch (nativeErrorCode(error)) {
      case "vr_organization_conflict":
        return "The complete organization plan conflicts with an existing or duplicate destination.";
      case "vr_organization_ineligible":
        return "This transfer is no longer eligible for organization in the current VR folder.";
      case "vr_organization_stale":
        return "The organization plan is stale because its transfer, folder, or files changed.";
      default:
        return "The organization operation could not be completed safely. Review the current Downloads state before retrying.";
    }
  };

  const previewDownloadOrganization = async (download: VrDownload) => {
    if (
      vrOrganizationPreviewPending.current ||
      vrOrganizationPreview !== null ||
      vrDownloadActionsPending.current.has(download.transferId)
    ) {
      return;
    }
    const currentState = currentVrDownloadsState.current;
    const currentDownload =
      currentState.status === "ready"
        ? currentState.downloads.find(
            (candidate) => candidate.transferId === download.transferId,
          )
        : undefined;
    if (currentDownload?.canOrganize !== true) {
      return;
    }

    const requestId = ++vrOrganizationRequestId.current;
    vrOrganizationPreviewPending.current = true;
    vrDownloadActionsPending.current.add(download.transferId);
    setPendingVrDownloadIds(new Set(vrDownloadActionsPending.current));
    setVrDownloadErrors((errors) => {
      const nextErrors = { ...errors };
      delete nextErrors[download.transferId];
      return nextErrors;
    });
    try {
      const preview = await previewVrOrganization(download.transferId);
      if (requestId !== vrOrganizationRequestId.current) {
        await dismissVrOrganization();
        return;
      }
      const latestState = currentVrDownloadsState.current;
      const latestDownload =
        latestState.status === "ready"
          ? latestState.downloads.find(
              (candidate) => candidate.transferId === download.transferId,
            )
          : undefined;
      if (
        latestDownload?.canOrganize !== true ||
        preview.transferId !== download.transferId ||
        preview.code !== download.code
      ) {
        await dismissVrOrganization();
        throw new Error("vr_organization_stale");
      }
      setVrOrganizationPreview(preview);
    } catch (error: unknown) {
      if (requestId === vrOrganizationRequestId.current) {
        setVrDownloadErrors((errors) => ({
          ...errors,
          [download.transferId]: organizationErrorMessage(error),
        }));
      }
    } finally {
      vrOrganizationPreviewPending.current = false;
      vrDownloadActionsPending.current.delete(download.transferId);
      setPendingVrDownloadIds(new Set(vrDownloadActionsPending.current));
    }
  };

  const closeDownloadOrganization = () => {
    if (
      vrOrganizationPreview !== null &&
      vrDownloadActionsPending.current.has(vrOrganizationPreview.transferId)
    ) {
      return;
    }
    vrOrganizationRequestId.current += 1;
    void dismissVrOrganization();
    setVrOrganizationPreview(null);
  };

  const applyDownloadOrganization = async () => {
    const preview = vrOrganizationPreview;
    if (
      preview === null ||
      vrDownloadActionsPending.current.has(preview.transferId)
    ) {
      return;
    }
    const currentState = currentVrDownloadsState.current;
    const currentDownload =
      currentState.status === "ready"
        ? currentState.downloads.find(
            (candidate) => candidate.transferId === preview.transferId,
          )
        : undefined;
    if (currentDownload?.canOrganize !== true) {
      return;
    }

    const requestId = ++vrOrganizationRequestId.current;
    vrDownloadActionsPending.current.add(preview.transferId);
    setPendingVrDownloadIds(new Set(vrDownloadActionsPending.current));
    setVrDownloadErrors((errors) => {
      const nextErrors = { ...errors };
      delete nextErrors[preview.transferId];
      return nextErrors;
    });
    try {
      await applyVrOrganization(preview.planId);
      if (requestId !== vrOrganizationRequestId.current) {
        return;
      }
      setVrOrganizationPreview(null);
      await refreshVrDownloads();
      refreshVrLibrary();
      setVrDownloadFocusTarget(`vr-download-dismiss-${preview.transferId}`);
    } catch (error: unknown) {
      if (requestId === vrOrganizationRequestId.current) {
        setVrOrganizationPreview(null);
        await refreshVrDownloads();
        setVrDownloadErrors((errors) => ({
          ...errors,
          [preview.transferId]: organizationErrorMessage(error),
        }));
        setVrDownloadFocusTarget(
          `vr-download-organize-${preview.transferId}`,
        );
      }
    } finally {
      vrDownloadActionsPending.current.delete(preview.transferId);
      setPendingVrDownloadIds(new Set(vrDownloadActionsPending.current));
    }
  };

  const runVrDownloadAction = async (
    download: VrDownload,
    action: "pause" | "resume" | "cancel" | "dismiss",
  ) => {
    if (vrDownloadActionsPending.current.has(download.transferId)) {
      return;
    }
    const currentState = currentVrDownloadsState.current;
    const currentDownload =
      currentState.status === "ready"
        ? currentState.downloads.find(
            (candidate) => candidate.transferId === download.transferId,
          )
        : undefined;
    const actionIsCurrent =
      currentDownload !== undefined &&
      ((action === "pause" && currentDownload.state === "downloading") ||
        (action === "resume" && currentDownload.state === "paused") ||
        (action === "cancel" &&
          (activeVrDownloadStates.has(currentDownload.state) ||
            currentDownload.state === "offline")) ||
        (action === "dismiss" &&
          !activeVrDownloadStates.has(currentDownload.state)));
    if (!actionIsCurrent) {
      return;
    }

    vrDownloadActionsPending.current.add(download.transferId);
    setPendingVrDownloadIds(
      new Set(vrDownloadActionsPending.current),
    );
    setVrDownloadErrors((errors) => {
      const nextErrors = { ...errors };
      delete nextErrors[download.transferId];
      return nextErrors;
    });
    try {
      if (action === "pause") {
        await pauseVrDownload(download.transferId);
      } else if (action === "resume") {
        await resumeVrDownload(download.transferId);
      } else if (action === "cancel") {
        await cancelVrDownload(download.transferId);
      } else {
        await dismissVrDownload(download.transferId);
      }
      await refreshVrDownloads();
      const focusTarget = {
        cancel: `vr-download-dismiss-${download.transferId}`,
        dismiss: "vr-downloads-refresh",
        pause: `vr-download-resume-${download.transferId}`,
        resume: `vr-download-pause-${download.transferId}`,
      }[action];
      setVrDownloadFocusTarget(focusTarget);
    } catch {
      setVrDownloadErrors((errors) => ({
        ...errors,
        [download.transferId]: `The ${action} action could not be completed for this transfer.`,
      }));
    } finally {
      vrDownloadActionsPending.current.delete(download.transferId);
      setPendingVrDownloadIds(
        new Set(vrDownloadActionsPending.current),
      );
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

  const closeVrTorrentInspection = () => {
    torrentInspectionRequestId.current += 1;
    torrentSaveRequestId.current += 1;
    torrentStartRequestId.current += 1;
    setTorrentInspectionContext(null);
    setTorrentInspectionState(null);
    setTorrentSaveState("idle");
    setTorrentStartState({ status: "idle" });
    setSelectedTorrentFileIds(new Set());
    void invalidateVerifiedVrTorrent().catch(() => undefined);
  };

  const closeVrReleaseComparison = () => {
    closeVrTorrentInspection();
    releaseRequestId.current += 1;
    setReleaseComparisonItem(null);
    setReleaseComparisonState(null);
    setSelectedVrRelease(null);
  };

  const changeDiscoverCategory = (category: DiscoverCategory) => {
    if (category === discoverCategory) {
      return;
    }

    closeDiscoverMovieDetails();
    closeVrReleaseComparison();
    if (discoverCategory === "vr") {
      vrCatalogRequestId.current += 1;
      setVrCatalogState((currentState) =>
        currentState.status === "loading"
          ? { status: "idle" }
          : currentState,
      );
    }
    setDiscoverCategory(category);
  };

  const searchVrCatalog = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const canonicalCode = canonicalizeProductCode(vrSearchInput);
    if (canonicalCode === null) {
      setVrSearchInputError("Enter a valid VR product code, such as MDVR-419.");
      return;
    }

    closeVrReleaseComparison();
    vrCatalogRequestId.current += 1;
    setVrSearchInput(canonicalCode);
    setVrSearchInputError(null);
    setSubmittedVrCode(canonicalCode);
    setVrCatalogState({ status: "loading" });
    setVrSelectedPage(1);
    setVrCatalogRequestVersion((version) => version + 1);
  };

  const retryVrCatalog = () => {
    if (submittedVrCode === null) {
      return;
    }

    closeVrReleaseComparison();
    vrCatalogRequestId.current += 1;
    setVrCatalogState({ status: "loading" });
    setVrCatalogRequestVersion((version) => version + 1);
  };

  const openVrReleaseComparison = (
    item: VrCatalogItem,
    triggerId: string,
  ) => {
    releaseRequestId.current += 1;
    setReleaseComparisonItem(item);
    setReleaseComparisonState({ status: "loading" });
    setReleaseComparisonTriggerId(triggerId);
    setSelectedVrRelease(null);
    setReleaseRequestVersion((version) => version + 1);
  };

  const retryVrReleaseComparison = () => {
    if (releaseComparisonItem === null) {
      return;
    }

    closeVrTorrentInspection();
    releaseRequestId.current += 1;
    setReleaseComparisonState({ status: "loading" });
    setSelectedVrRelease(null);
    setReleaseRequestVersion((version) => version + 1);
  };

  const selectVrRelease = (release: VrRelease) => {
    if (
      releaseComparisonState?.status === "ready" &&
      releaseComparisonState.releases.includes(release)
    ) {
      if (selectedVrRelease !== release) {
        closeVrTorrentInspection();
      }
      setSelectedVrRelease(release);
    }
  };

  const openVrTorrentInspection = (
    release: VrRelease,
    triggerId: string,
  ) => {
    if (
      release.artifact === undefined ||
      releaseComparisonItem === null ||
      selectedVrRelease !== release
    ) {
      return;
    }

    torrentInspectionRequestId.current += 1;
    torrentSaveRequestId.current += 1;
    torrentStartRequestId.current += 1;
    setTorrentInspectionContext({
      item: releaseComparisonItem,
      release,
      triggerId,
    });
    setTorrentInspectionState({ status: "loading" });
    setTorrentSaveState("idle");
    setTorrentStartState({ status: "idle" });
    setSelectedTorrentFileIds(new Set());
    setTorrentInspectionRequestVersion((version) => version + 1);
  };

  const retryVrTorrentInspection = () => {
    if (torrentInspectionContext === null) {
      return;
    }
    torrentInspectionRequestId.current += 1;
    torrentSaveRequestId.current += 1;
    torrentStartRequestId.current += 1;
    setTorrentInspectionState({ status: "loading" });
    setTorrentSaveState("idle");
    setTorrentStartState({ status: "idle" });
    setSelectedTorrentFileIds(new Set());
    setTorrentInspectionRequestVersion((version) => version + 1);
  };

  const saveVrTorrent = async () => {
    if (
      torrentSavePending.current ||
      torrentInspectionState?.status !== "ready"
    ) {
      return;
    }

    torrentSavePending.current = true;
    const requestId = ++torrentSaveRequestId.current;
    setTorrentSaveState("saving");
    try {
      const saved = await saveVerifiedVrTorrent(
        torrentInspectionState.inspection.inspectionId,
      );
      if (requestId === torrentSaveRequestId.current && saved) {
        setTorrentSaveState("success");
      } else if (requestId === torrentSaveRequestId.current) {
        setTorrentSaveState("idle");
      }
    } catch {
      if (requestId === torrentSaveRequestId.current) {
        setTorrentSaveState("error");
      }
    } finally {
      torrentSavePending.current = false;
    }
  };

  const toggleTorrentFile = (fileId: number) => {
    if (
      torrentStartState.status === "starting" ||
      torrentStartState.status === "success" ||
      torrentInspectionState?.status !== "ready" ||
      fileId < 0 ||
      fileId >= torrentInspectionState.inspection.files.length
    ) {
      return;
    }
    setTorrentStartState({ status: "idle" });
    setSelectedTorrentFileIds((selectedFileIds) => {
      const nextSelection = new Set(selectedFileIds);
      if (nextSelection.has(fileId)) {
        nextSelection.delete(fileId);
      } else {
        nextSelection.add(fileId);
      }
      return nextSelection;
    });
  };

  const startVrDownload = async () => {
    if (
      torrentStartPending.current ||
      torrentInspectionState?.status !== "ready" ||
      selectedTorrentFileIds.size === 0 ||
      vrFolderState.status !== "ready" ||
      vrDownloadsState.status !== "ready"
    ) {
      return;
    }
    torrentStartPending.current = true;
    const requestId = ++torrentStartRequestId.current;
    const selectedFileIds = [...selectedTorrentFileIds].sort(
      (left, right) => left - right,
    );
    setTorrentStartState({ status: "starting" });
    try {
      await startVerifiedVrDownload(
        torrentInspectionState.inspection.inspectionId,
        selectedFileIds,
      );
      await refreshVrDownloads();
      if (requestId === torrentStartRequestId.current) {
        setTorrentStartState({ status: "success" });
      }
    } catch (error: unknown) {
      if (requestId === torrentStartRequestId.current) {
        setTorrentStartState({
          status: "error",
          message: vrDownloadStartError(error),
        });
      }
    } finally {
      torrentStartPending.current = false;
    }
  };

  const openVrDestinationFromInspection = (
    destination: typeof settingsDestination | typeof downloadsDestination,
  ) => {
    closeVrReleaseComparison();
    navigateTo(destination);
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
  const currentVrCatalogMessage =
    vrCatalogState.status === "ready"
      ? null
      : vrCatalogMessages[vrCatalogState.status];
  const vrGalleryLabel =
    submittedVrCode === null
      ? "VR product-code search"
      : `VR result for ${submittedVrCode}`;
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
  const currentVrLibraryScanMessage =
    vrLibraryScanState.status === "ready"
      ? null
      : vrLibraryScanMessages[vrLibraryScanState.status];
  const completeVrLibraryItems =
    vrLibraryScanState.status === "ready" ? vrLibraryScanState.items : [];
  const vrLibrarySearch = vrLibrarySearchQuery.toLowerCase();
  const isVrLibrarySearchActive = vrLibrarySearchQuery.trim() !== "";
  const matchingVrLibraryItems = isVrLibrarySearchActive
    ? completeVrLibraryItems.filter(
        (item) =>
          item.title.toLowerCase().includes(vrLibrarySearch) ||
          item.code?.toLowerCase().includes(vrLibrarySearch),
      )
    : completeVrLibraryItems;
  const orderedVrLibraryItems = [...matchingVrLibraryItems].sort(
    (leftItem, rightItem) =>
      compareVrLibraryItemsByTitle(
        leftItem,
        rightItem,
        vrLibraryTitleSortDirection,
      ),
  );
  const completeVrLibraryFileCount = completeVrLibraryItems.reduce(
    (count, item) => count + item.files.length,
    0,
  );
  const currentTvLibraryScanMessage =
    tvLibraryScanState.status === "ready"
      ? null
      : tvLibraryScanMessages[tvLibraryScanState.status];
  const completeTvLibraryItems =
    tvLibraryScanState.status === "ready" ? tvLibraryScanState.items : [];
  const tvLibrarySearch = tvLibrarySearchQuery.toLowerCase();
  const isTvLibrarySearchActive = tvLibrarySearchQuery.trim() !== "";
  const matchingTvLibraryItems = isTvLibrarySearchActive
    ? completeTvLibraryItems.filter(
        (item) =>
          item.title.toLowerCase().includes(tvLibrarySearch) ||
          item.files.some((file) =>
            file.filename.toLowerCase().includes(tvLibrarySearch),
          ),
      )
    : completeTvLibraryItems;
  const orderedTvLibraryItems = [...matchingTvLibraryItems].sort(
    (leftItem, rightItem) =>
      compareTvLibraryItemsByTitle(
        leftItem,
        rightItem,
        tvLibraryTitleSortDirection,
      ),
  );
  const completeTvLibraryEpisodeCount = completeTvLibraryItems.reduce(
    (count, item) =>
      count +
      item.files.filter(
        (file) => file.season !== null && file.episode !== null,
      ).length,
    0,
  );
  const completeTvLibraryShowCount = completeTvLibraryItems.filter(
    (item) => item.showTitle !== null,
  ).length;
  const currentVrDownloads =
    vrDownloadsState.status === "ready" ? vrDownloadsState.downloads : [];
  const vrDownloadSummary = summarizeVrDownloads(currentVrDownloads);
  const vrDownloadLimitRequiresAttention =
    vrDownloadLimitState.status === "error" ||
    vrDownloadLimitMessage?.role === "alert";
  const currentVrDownloadLimit =
    vrDownloadLimitState.status === "ready"
      ? formatVrDownloadLimit(vrDownloadLimitState.limit)
      : vrDownloadLimitState.status === "loading"
        ? "Loading…"
        : "Needs attention";
  const dashboardDownloadsDestination = vrDownloadLimitRequiresAttention
    ? settingsDestination
    : currentVrDownloads.length > 0 || vrDownloadsState.status === "error"
      ? downloadsDestination
      : null;
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
  let dashboardTvHeading = "Loading TV Library";
  let dashboardTvMessage = "Loading the configured TV folder.";
  let dashboardTvRole: "alert" | "status" | undefined = "status";
  let dashboardTvDestination: (typeof destinations)[number] = libraryDestination;

  if (tvFolderState.status === "unconfigured") {
    dashboardTvHeading = "TV Library is not configured";
    dashboardTvMessage = "Choose one local TV folder in Settings.";
    dashboardTvRole = undefined;
    dashboardTvDestination = settingsDestination;
  } else if (tvFolderState.status === "unavailable") {
    dashboardTvHeading = "TV folder is unavailable";
    dashboardTvMessage = "The configured folder may have moved or become inaccessible.";
    dashboardTvRole = "alert";
    dashboardTvDestination = settingsDestination;
  } else if (tvFolderState.status === "error") {
    dashboardTvHeading = "TV Library needs attention";
    dashboardTvMessage = "The TV folder configuration could not be loaded.";
    dashboardTvRole = "alert";
    dashboardTvDestination = settingsDestination;
  } else if (tvLibraryScanState.status === "scanning") {
    dashboardTvHeading = "Scanning TV Library";
    dashboardTvMessage = "Looking recursively for supported .mp4 and .mkv files.";
  } else if (tvLibraryScanState.status === "empty") {
    dashboardTvHeading = "0 shows · 0 episodes";
    dashboardTvMessage = "The configured folder contains no supported video files.";
    dashboardTvRole = undefined;
  } else if (tvLibraryScanState.status === "ready") {
    dashboardTvHeading = `${completeTvLibraryShowCount} ${completeTvLibraryShowCount === 1 ? "show" : "shows"} · ${completeTvLibraryEpisodeCount} ${completeTvLibraryEpisodeCount === 1 ? "episode" : "episodes"}`;
    const unassociatedCount = completeTvLibraryItems.filter(
      (item) => item.showTitle === null,
    ).length;
    dashboardTvMessage =
      unassociatedCount === 0
        ? "These totals come from the latest complete TV folder scan."
        : `${unassociatedCount} ${unassociatedCount === 1 ? "file remains" : "files remain"} unassociated.`;
    dashboardTvRole = undefined;
  } else if (tvLibraryScanState.status === "unavailable") {
    dashboardTvHeading = "TV folder is unavailable";
    dashboardTvMessage = "The configured folder may have moved or become inaccessible.";
    dashboardTvRole = "alert";
    dashboardTvDestination = settingsDestination;
  } else if (tvLibraryScanState.status === "error") {
    dashboardTvHeading = "TV Library scan failed";
    dashboardTvMessage = "Auto-Video could not read every item in the configured folder.";
    dashboardTvRole = "alert";
  }

  let dashboardTvStorageHeading = "Waiting for TV folder configuration";
  let dashboardTvStorageMessage =
    "Storage will load after the configured TV folder is known.";
  let dashboardTvStorageRole: "alert" | "status" | undefined;
  if (tvStorageState.status === "loading") {
    dashboardTvStorageHeading = "Loading storage";
    dashboardTvStorageMessage = "Reading the volume capacity for the configured TV folder.";
    dashboardTvStorageRole = "status";
  } else if (tvStorageState.status === "unavailable") {
    dashboardTvStorageHeading = "TV volume is unavailable";
    dashboardTvStorageMessage = "The configured folder or its containing volume is not accessible.";
    dashboardTvStorageRole = "alert";
  } else if (tvStorageState.status === "error") {
    dashboardTvStorageHeading = "Storage could not be loaded";
    dashboardTvStorageMessage = "Auto-Video could not read the containing volume capacity.";
    dashboardTvStorageRole = "alert";
  }
  let dashboardVrHeading = "Loading VR Library";
  let dashboardVrMessage = "Loading the configured VR folder.";
  let dashboardVrRole: "alert" | "status" | undefined = "status";
  let dashboardVrDestination: (typeof destinations)[number] = libraryDestination;

  if (vrFolderState.status === "unconfigured") {
    dashboardVrHeading = "VR Library is not configured";
    dashboardVrMessage = "Choose one local VR folder in Settings.";
    dashboardVrRole = undefined;
    dashboardVrDestination = settingsDestination;
  } else if (vrFolderState.status === "unavailable") {
    dashboardVrHeading = "VR folder is unavailable";
    dashboardVrMessage = "The configured folder may have moved or become inaccessible.";
    dashboardVrRole = "alert";
    dashboardVrDestination = settingsDestination;
  } else if (vrFolderState.status === "error") {
    dashboardVrHeading = "VR Library needs attention";
    dashboardVrMessage = "The VR folder configuration could not be loaded.";
    dashboardVrRole = "alert";
    dashboardVrDestination = settingsDestination;
  } else if (vrLibraryScanState.status === "scanning") {
    dashboardVrHeading = "Scanning VR Library";
    dashboardVrMessage = "Looking recursively for supported .mp4 and .mkv files.";
  } else if (vrLibraryScanState.status === "empty") {
    dashboardVrHeading = "0 VR titles · 0 files";
    dashboardVrMessage = "The configured folder contains no supported video files.";
    dashboardVrRole = undefined;
  } else if (vrLibraryScanState.status === "ready") {
    const titleCount = vrLibraryScanState.items.length;
    dashboardVrHeading = `${titleCount} VR ${titleCount === 1 ? "title" : "titles"} · ${completeVrLibraryFileCount} ${completeVrLibraryFileCount === 1 ? "file" : "files"}`;
    dashboardVrMessage = "These totals come from the latest complete VR folder scan.";
    dashboardVrRole = undefined;
  } else if (vrLibraryScanState.status === "unavailable") {
    dashboardVrHeading = "VR folder is unavailable";
    dashboardVrMessage = "The configured folder may have moved or become inaccessible.";
    dashboardVrRole = "alert";
    dashboardVrDestination = settingsDestination;
  } else if (vrLibraryScanState.status === "error") {
    dashboardVrHeading = "VR Library scan failed";
    dashboardVrMessage = "Auto-Video could not read every item in the configured folder.";
    dashboardVrRole = "alert";
  }

  let dashboardVrStorageHeading = "Waiting for VR folder configuration";
  let dashboardVrStorageMessage =
    "Storage will load after the configured VR folder is known.";
  let dashboardVrStorageRole: "alert" | "status" | undefined;
  if (vrStorageState.status === "loading") {
    dashboardVrStorageHeading = "Loading storage";
    dashboardVrStorageMessage = "Reading the volume capacity for the configured VR folder.";
    dashboardVrStorageRole = "status";
  } else if (vrStorageState.status === "unavailable") {
    dashboardVrStorageHeading = "VR volume is unavailable";
    dashboardVrStorageMessage = "The configured folder or its containing volume is not accessible.";
    dashboardVrStorageRole = "alert";
  } else if (vrStorageState.status === "error") {
    dashboardVrStorageHeading = "Storage could not be loaded";
    dashboardVrStorageMessage = "Auto-Video could not read the containing volume capacity.";
    dashboardVrStorageRole = "alert";
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
            <>
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
            <section
              aria-busy={
                tvFolderState.status === "loading" ||
                tvLibraryScanState.status === "scanning"
              }
              aria-labelledby="dashboard-tv-heading"
              className="dashboard-library-summary"
            >
              <div className="dashboard-library-summary__heading">
                <span className="empty-state__icon">
                  <AppIcon name="tv" />
                </span>
                <div>
                  <p className="card-eyebrow">Local library</p>
                  <h2 id="dashboard-tv-heading">TV Library</h2>
                  <p className="dashboard-library-summary__folder">
                    {tvFolderState.status === "loading"
                      ? "Loading configured TV folder…"
                      : tvFolderState.status === "ready" ||
                          tvFolderState.status === "unavailable"
                        ? tvFolderState.path
                        : "No TV folder configured"}
                  </p>
                </div>
              </div>

              <div
                className="dashboard-library-summary__status"
                role={dashboardTvRole}
              >
                <p className="card-eyebrow">Current status</p>
                <h3>{dashboardTvHeading}</h3>
                <p>{dashboardTvMessage}</p>
              </div>

              <div
                aria-busy={tvStorageState.status === "loading"}
                className="dashboard-library-summary__storage"
              >
                <p className="card-eyebrow">Storage</p>
                {tvStorageState.status === "ready" ? (
                  <dl aria-label="TV volume storage">
                    <div>
                      <dt>Total</dt>
                      <dd>{formatStorageBytes(tvStorageState.totalBytes)}</dd>
                    </div>
                    <div>
                      <dt>Used</dt>
                      <dd>
                        {formatStorageBytes(
                          tvStorageState.totalBytes - tvStorageState.freeBytes,
                        )}
                      </dd>
                    </div>
                    <div>
                      <dt>Free</dt>
                      <dd>{formatStorageBytes(tvStorageState.freeBytes)}</dd>
                    </div>
                  </dl>
                ) : (
                  <div role={dashboardTvStorageRole}>
                    <h3>{dashboardTvStorageHeading}</h3>
                    <p>{dashboardTvStorageMessage}</p>
                  </div>
                )}
              </div>

              {tvFolderState.status === "loading" ? null : (
                <Button
                  className="dashboard-library-summary__action"
                  onClick={() => {
                    if (dashboardTvDestination.id === "library") {
                      setLibraryCategory("tv");
                    }
                    navigateTo(dashboardTvDestination);
                  }}
                  type="button"
                >
                  <AppIcon name={dashboardTvDestination.id} />
                  {dashboardTvDestination.id === "library"
                    ? "Open TV Library"
                    : "Open TV Settings"}
                </Button>
              )}
            </section>
            <section
              aria-busy={
                vrFolderState.status === "loading" ||
                vrLibraryScanState.status === "scanning"
              }
              aria-labelledby="dashboard-vr-heading"
              className="dashboard-library-summary"
            >
              <div className="dashboard-library-summary__heading">
                <span className="empty-state__icon">
                  <AppIcon name="vr" />
                </span>
                <div>
                  <p className="card-eyebrow">Local library</p>
                  <h2 id="dashboard-vr-heading">VR Library</h2>
                  <p className="dashboard-library-summary__folder">
                    {vrFolderState.status === "loading"
                      ? "Loading configured VR folder…"
                      : vrFolderState.status === "ready" ||
                          vrFolderState.status === "unavailable"
                        ? vrFolderState.path
                        : "No VR folder configured"}
                  </p>
                </div>
              </div>

              <div
                className="dashboard-library-summary__status"
                role={dashboardVrRole}
              >
                <p className="card-eyebrow">Current status</p>
                <h3>{dashboardVrHeading}</h3>
                <p>{dashboardVrMessage}</p>
              </div>

              <div
                aria-busy={vrStorageState.status === "loading"}
                className="dashboard-library-summary__storage"
              >
                <p className="card-eyebrow">Storage</p>
                {vrStorageState.status === "ready" ? (
                  <dl aria-label="VR volume storage">
                    <div>
                      <dt>Total</dt>
                      <dd>{formatStorageBytes(vrStorageState.totalBytes)}</dd>
                    </div>
                    <div>
                      <dt>Used</dt>
                      <dd>
                        {formatStorageBytes(
                          vrStorageState.totalBytes - vrStorageState.freeBytes,
                        )}
                      </dd>
                    </div>
                    <div>
                      <dt>Free</dt>
                      <dd>{formatStorageBytes(vrStorageState.freeBytes)}</dd>
                    </div>
                  </dl>
                ) : (
                  <div role={dashboardVrStorageRole}>
                    <h3>{dashboardVrStorageHeading}</h3>
                    <p>{dashboardVrStorageMessage}</p>
                  </div>
                )}
              </div>

              {vrFolderState.status === "loading" ? null : (
                <Button
                  className="dashboard-library-summary__action"
                  onClick={() => {
                    if (dashboardVrDestination.id === "library") {
                      setLibraryCategory("vr");
                    }
                    navigateTo(dashboardVrDestination);
                  }}
                  type="button"
                >
                  <AppIcon name={dashboardVrDestination.id} />
                  {dashboardVrDestination.id === "library"
                    ? "Open VR Library"
                    : "Open VR Settings"}
                </Button>
              )}
            </section>
            <section
              aria-busy={
                vrDownloadsState.status === "loading" ||
                vrDownloadLimitState.status === "loading"
              }
              aria-labelledby="dashboard-downloads-heading"
              className="dashboard-library-summary"
            >
              <div className="dashboard-library-summary__heading">
                <span className="empty-state__icon">
                  <AppIcon name="downloads" />
                </span>
                <div>
                  <p className="card-eyebrow">Current transfers</p>
                  <h2 id="dashboard-downloads-heading">Downloads</h2>
                  <p className="dashboard-library-summary__folder">
                    Aggregate VR transfer activity
                  </p>
                </div>
              </div>

              {vrDownloadsState.status === "ready" ? (
                <div className="dashboard-library-summary__storage">
                  <p className="card-eyebrow">Current status</p>
                  <dl aria-label="VR transfer summary">
                    <div>
                      <dt>Active</dt>
                      <dd>{vrDownloadSummary.activeCount}</dd>
                    </div>
                    <div>
                      <dt>Paused</dt>
                      <dd>{vrDownloadSummary.pausedCount}</dd>
                    </div>
                    <div>
                      <dt>Completed</dt>
                      <dd>{vrDownloadSummary.completedCount}</dd>
                    </div>
                    <div>
                      <dt>Needs attention</dt>
                      <dd>{vrDownloadSummary.attentionCount}</dd>
                    </div>
                    <div>
                      <dt>Download speed</dt>
                      <dd>
                        {formatStorageBytes(
                          vrDownloadSummary.aggregateSpeedBytesPerSecond,
                        )}
                        /s
                      </dd>
                    </div>
                    <div>
                      <dt>Limit</dt>
                      <dd>{currentVrDownloadLimit}</dd>
                    </div>
                  </dl>
                </div>
              ) : (
                <div
                  className="dashboard-library-summary__status"
                  role={vrDownloadsState.status === "error" ? "alert" : "status"}
                >
                  <p className="card-eyebrow">Current status</p>
                  <h3>
                    {vrDownloadsState.status === "loading"
                      ? "Loading VR transfers"
                      : "VR transfers need attention"}
                  </h3>
                  <p>
                    {vrDownloadsState.status === "loading"
                      ? "Loading the current native transfer snapshot and aggregate limit."
                      : vrDownloadLimitRequiresAttention
                        ? "The aggregate limit could not be loaded or applied safely."
                        : "The current transfer snapshot could not be loaded."}
                  </p>
                </div>
              )}

              {dashboardDownloadsDestination === null ? null : (
                <Button
                  className="dashboard-library-summary__action"
                  onClick={() => navigateTo(dashboardDownloadsDestination)}
                  type="button"
                >
                  <AppIcon name={dashboardDownloadsDestination.id} />
                  {dashboardDownloadsDestination.id === "settings"
                    ? "Open Download Settings"
                    : "Open Downloads"}
                </Button>
              )}
            </section>
            </>
          ) : activeDestination.id === "discover" ? (
            <section
              aria-busy={
                discoverCategory === "movies"
                  ? discoverState.status === "loading"
                  : vrCatalogState.status === "loading"
              }
              aria-labelledby="discover-movies-heading"
              className="discover-content"
            >
              <div className="library-toolbar library-toolbar--discover">
                <div className="library-toolbar__heading">
                  <span className="empty-state__icon">
                    <AppIcon
                      name={discoverCategory === "movies" ? "discover" : "vr"}
                    />
                  </span>
                  <div>
                    <p className="card-eyebrow">
                      {discoverCategory === "movies"
                        ? "TMDB Discover"
                        : "JavDB VR Discover"}
                    </p>
                    <h2 id="discover-movies-heading">
                      {discoverCategory === "movies"
                        ? discoverGalleryLabel
                        : vrGalleryLabel}
                    </h2>
                    <p className="library-folder">
                      {discoverCategory === "vr" ? (
                        submittedVrCode === null ? (
                          "Exact product-code lookup"
                        ) : (
                          <>Requested code {submittedVrCode}</>
                        )
                      ) : isDiscoverSearchActive ? (
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
                <div className="discover-toolbar__controls">
                  <fieldset className="discover-category">
                    <legend>Discover category</legend>
                    <div>
                      {([
                        ["movies", "Movies"],
                        ["vr", "VR"],
                      ] as const).map(([category, label]) => (
                        <label key={category}>
                          <input
                            checked={discoverCategory === category}
                            name="discover-category"
                            onChange={() => changeDiscoverCategory(category)}
                            type="radio"
                            value={category}
                          />
                          <span>{label}</span>
                        </label>
                      ))}
                    </div>
                  </fieldset>
                  {discoverCategory === "vr" ? (
                    <form
                      aria-label="Search JavDB VR titles"
                      className="discover-search"
                      onSubmit={searchVrCatalog}
                      role="search"
                    >
                      <label htmlFor="discover-vr-search">
                        Search product code
                      </label>
                      <div className="discover-search__field">
                        <input
                          aria-describedby={
                            vrSearchInputError === null
                              ? undefined
                              : "discover-vr-search-error"
                          }
                          aria-invalid={
                            vrSearchInputError === null
                              ? undefined
                              : true
                          }
                          className="discover-search__input"
                          id="discover-vr-search"
                          onChange={(event) => {
                            setVrSearchInput(event.target.value);
                            if (vrSearchInputError !== null) {
                              setVrSearchInputError(null);
                            }
                          }}
                          placeholder="MDVR-419"
                          type="text"
                          value={vrSearchInput}
                        />
                        <Button type="submit">
                          <AppIcon name="search" />
                          Search
                        </Button>
                      </div>
                      {vrSearchInputError === null ? null : (
                        <p
                          className="discover-search__error"
                          id="discover-vr-search-error"
                          role="alert"
                        >
                          {vrSearchInputError}
                        </p>
                      )}
                    </form>
                  ) : isTmdbTokenLoaded &&
                    !tmdbCredentialLoadFailed &&
                    tmdbToken !== null ? (
                    <>
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
                    </>
                  ) : null}
                </div>
              </div>

              {discoverCategory === "vr" ? (
                vrCatalogState.status === "ready" ? (
                  <ResizeAwareGallery
                    ariaLabel={vrGalleryLabel}
                    getItemKey={(item) => item.code}
                    items={[vrCatalogState.item]}
                    key={`vr-gallery-${vrCatalogState.item.code}`}
                    onSelectedPageChange={setVrSelectedPage}
                    renderItem={(item) => (
                      <DiscoverVrCard
                        item={item}
                        onFindReleases={openVrReleaseComparison}
                      />
                    )}
                    selectedPage={vrSelectedPage}
                    variant="discover"
                  />
                ) : (
                  <div
                    className="empty-state discover-state"
                    role={currentVrCatalogMessage?.role}
                  >
                    <span className="empty-state__icon">
                      <AppIcon name="vr" />
                    </span>
                    <h2>{currentVrCatalogMessage?.heading}</h2>
                    <p>{currentVrCatalogMessage?.message}</p>
                    {vrCatalogState.status === "source-unavailable" ||
                    vrCatalogState.status === "network-error" ||
                    vrCatalogState.status === "malformed-provider" ||
                    vrCatalogState.status === "provider-error" ? (
                      <Button
                        className="empty-state__action"
                        onClick={retryVrCatalog}
                        type="button"
                        variant="outline"
                      >
                        <AppIcon name="refresh" />
                        Retry search
                      </Button>
                    ) : null}
                  </div>
                )
              ) : discoverState.status === "ready" ? (
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

              {discoverCategory === "movies" ? (
                <footer aria-label="TMDB credits" className="tmdb-attribution">
                  <img alt="TMDB" src={tmdbLogo} />
                  <p>
                    This product uses the TMDB API but is not endorsed or certified
                    by TMDB.
                  </p>
                </footer>
              ) : null}
            </section>
          ) : activeDestination.id === "library" ? (
            <section
              aria-busy={
                libraryCategory === "movies"
                  ? movieScanState.status === "scanning"
                  : libraryCategory === "tv"
                    ? tvLibraryScanState.status === "scanning"
                    : vrLibraryScanState.status === "scanning"
              }
              aria-labelledby="library-heading"
              className="library-content"
            >
              <div className="library-toolbar library-toolbar--movies">
                <div className="library-toolbar__heading">
                  <span className="empty-state__icon">
                    <AppIcon
                      name={
                        libraryCategory === "movies"
                          ? "library"
                          : libraryCategory
                      }
                    />
                  </span>
                  <div>
                    <p className="card-eyebrow">Local library</p>
                    <h2 id="library-heading">
                      {libraryCategory === "movies"
                        ? "Movies"
                        : libraryCategory === "tv"
                          ? "TV"
                          : "VR"}
                    </h2>
                    <p className="library-folder">
                      {libraryCategory === "movies"
                        ? (moviesFolder ?? "No Movies folder configured")
                        : libraryCategory === "tv"
                          ? tvFolderState.status === "ready" ||
                            tvFolderState.status === "unavailable"
                            ? tvFolderState.path
                            : tvFolderState.status === "loading"
                              ? "Loading configured TV folder…"
                              : "No TV folder configured"
                          : vrFolderState.status === "ready" ||
                            vrFolderState.status === "unavailable"
                          ? vrFolderState.path
                          : vrFolderState.status === "loading"
                            ? "Loading configured VR folder…"
                            : "No VR folder configured"}
                    </p>
                  </div>
                </div>
                <div className="library-toolbar__controls">
                  <fieldset className="discover-category">
                    <legend>Library category</legend>
                    <div>
                      {([
                        ["movies", "Movies"],
                        ["tv", "TV"],
                        ["vr", "VR"],
                      ] as const).map(([category, label]) => (
                        <label key={category}>
                          <input
                            checked={libraryCategory === category}
                            name="library-category"
                            onChange={() => setLibraryCategory(category)}
                            type="radio"
                            value={category}
                          />
                          <span>{label}</span>
                        </label>
                      ))}
                    </div>
                  </fieldset>
                {libraryCategory === "movies" && moviesFolder !== null ? (
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
                ) : libraryCategory === "tv" &&
                  (tvFolderState.status === "ready" ||
                    tvFolderState.status === "unavailable") ? (
                  <div className="library-toolbar__controls">
                    <div
                      aria-label="TV title search"
                      className="movie-search"
                      role="search"
                    >
                      <label htmlFor="tv-library-title-search">Search titles</label>
                      <div className="movie-search__field">
                        <span className="movie-search__icon">
                          <AppIcon name="search" />
                        </span>
                        <input
                          aria-describedby={
                            tvLibraryScanState.status === "ready"
                              ? "tv-library-search-results"
                              : undefined
                          }
                          className="movie-search__input"
                          id="tv-library-title-search"
                          onChange={(event) =>
                            updateTvLibrarySearchQuery(event.target.value)
                          }
                          placeholder="Find a TV show or file"
                          type="text"
                          value={tvLibrarySearchQuery}
                        />
                        {tvLibrarySearchQuery === "" ? null : (
                          <Button
                            aria-label="Clear TV search"
                            className="movie-search__clear"
                            onClick={() => updateTvLibrarySearchQuery("")}
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
                      <label htmlFor="tv-library-title-sort">Sort titles</label>
                      <select
                        className="movie-sort__select"
                        id="tv-library-title-sort"
                        onChange={(event) => {
                          const direction = event.target.value;
                          if (direction !== "ascending" && direction !== "descending") {
                            throw new Error(
                              "The TV title sort returned an invalid direction.",
                            );
                          }
                          updateTvLibraryTitleSortDirection(direction);
                        }}
                        value={tvLibraryTitleSortDirection}
                      >
                        <option value="ascending">Title A–Z</option>
                        <option value="descending">Title Z–A</option>
                      </select>
                    </div>
                    <Button
                      disabled={
                        tvLibraryScanState.status === "scanning" ||
                        isRevalidatingTvFolder
                      }
                      onClick={refreshTvLibrary}
                      type="button"
                      variant="outline"
                    >
                      <AppIcon name="refresh" />
                      {isRevalidatingTvFolder ? "Refreshing…" : "Refresh"}
                    </Button>
                  </div>
                ) : libraryCategory === "vr" &&
                  (vrFolderState.status === "ready" ||
                    vrFolderState.status === "unavailable") ? (
                  <div className="library-toolbar__controls">
                    <div
                      aria-label="VR title search"
                      className="movie-search"
                      role="search"
                    >
                      <label htmlFor="vr-library-title-search">Search titles</label>
                      <div className="movie-search__field">
                        <span className="movie-search__icon">
                          <AppIcon name="search" />
                        </span>
                        <input
                          aria-describedby={
                            vrLibraryScanState.status === "ready"
                              ? "vr-library-search-results"
                              : undefined
                          }
                          className="movie-search__input"
                          id="vr-library-title-search"
                          onChange={(event) =>
                            updateVrLibrarySearchQuery(event.target.value)
                          }
                          placeholder="Find a VR title or code"
                          type="text"
                          value={vrLibrarySearchQuery}
                        />
                        {vrLibrarySearchQuery === "" ? null : (
                          <Button
                            aria-label="Clear VR search"
                            className="movie-search__clear"
                            onClick={() => updateVrLibrarySearchQuery("")}
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
                      <label htmlFor="vr-library-title-sort">Sort titles</label>
                      <select
                        className="movie-sort__select"
                        id="vr-library-title-sort"
                        onChange={(event) => {
                          const direction = event.target.value;
                          if (direction !== "ascending" && direction !== "descending") {
                            throw new Error(
                              "The VR title sort returned an invalid direction.",
                            );
                          }
                          updateVrLibraryTitleSortDirection(direction);
                        }}
                        value={vrLibraryTitleSortDirection}
                      >
                        <option value="ascending">Title A–Z</option>
                        <option value="descending">Title Z–A</option>
                      </select>
                    </div>
                    <Button
                      disabled={
                        vrLibraryScanState.status === "scanning" ||
                        isRevalidatingVrFolder
                      }
                      onClick={refreshVrLibrary}
                      type="button"
                      variant="outline"
                    >
                      <AppIcon name="refresh" />
                      {isRevalidatingVrFolder ? "Refreshing…" : "Refresh"}
                    </Button>
                  </div>
                ) : null}
                </div>
              </div>

              {libraryCategory === "movies" ? (
                movieScanState.status === "ready" && moviesFolder !== null ? (
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
                )
              ) : libraryCategory === "tv" ? (
                tvLibraryScanState.status === "ready" ? (
                  <>
                    <p
                      aria-atomic="true"
                      aria-live="polite"
                      className="sr-only"
                      id="tv-library-search-results"
                    >
                      {isTvLibrarySearchActive
                        ? `${matchingTvLibraryItems.length} TV items match the current search.`
                        : `${completeTvLibraryShowCount} shows, ${completeTvLibraryEpisodeCount} episodes, and ${completeTvLibraryItems.length - completeTvLibraryShowCount} unassociated files in the complete current result.`}
                    </p>
                    {matchingTvLibraryItems.length === 0 &&
                    isTvLibrarySearchActive ? (
                      <div className="empty-state library-state library-search-empty">
                        <span className="empty-state__icon">
                          <AppIcon name="search" />
                        </span>
                        <h2>No TV items match this search</h2>
                        <p>
                          No show titles or filenames match “
                          <span className="library-search-empty__query">
                            {tvLibrarySearchQuery}
                          </span>
                          ”. Clear the search to restore the complete Library.
                        </p>
                      </div>
                    ) : (
                      <ResizeAwareGallery
                        ariaLabel="TV shows and unassociated files"
                        getItemKey={(item) => item.id}
                        items={orderedTvLibraryItems}
                        key="tv-library-gallery"
                        onSelectedPageChange={setTvLibrarySelectedPage}
                        renderItem={(item) => <TvLibraryCard item={item} />}
                        selectedPage={tvLibrarySelectedPage}
                        variant="library"
                      />
                    )}
                  </>
                ) : (
                  <div
                    className="empty-state library-state"
                    role={currentTvLibraryScanMessage?.role}
                  >
                    <span className="empty-state__icon">
                      <AppIcon name="tv" />
                    </span>
                    <h2>{currentTvLibraryScanMessage?.heading}</h2>
                    <p>{currentTvLibraryScanMessage?.message}</p>
                  </div>
                )
              ) : vrLibraryScanState.status === "ready" ? (
                <>
                  <p
                    aria-atomic="true"
                    aria-live="polite"
                    className="sr-only"
                    id="vr-library-search-results"
                  >
                    {isVrLibrarySearchActive
                      ? `${matchingVrLibraryItems.length} VR titles match the current search.`
                      : `${completeVrLibraryItems.length} VR titles and ${completeVrLibraryFileCount} files in the complete current result.`}
                  </p>
                  {matchingVrLibraryItems.length === 0 &&
                  isVrLibrarySearchActive ? (
                    <div className="empty-state library-state library-search-empty">
                      <span className="empty-state__icon">
                        <AppIcon name="search" />
                      </span>
                      <h2>No VR titles match this search</h2>
                      <p>
                        No titles or codes match “
                        <span className="library-search-empty__query">
                          {vrLibrarySearchQuery}
                        </span>
                        ”. Clear the search to restore the complete Library.
                      </p>
                    </div>
                  ) : (
                    <ResizeAwareGallery
                      ariaLabel="VR titles"
                      getItemKey={(item) => item.id}
                      items={orderedVrLibraryItems}
                      key="vr-library-gallery"
                      onSelectedPageChange={setVrLibrarySelectedPage}
                      renderItem={(item) => <VrLibraryCard item={item} />}
                      selectedPage={vrLibrarySelectedPage}
                      variant="library"
                    />
                  )}
                </>
              ) : (
                <div
                  className="empty-state library-state"
                  role={currentVrLibraryScanMessage?.role}
                >
                  <span className="empty-state__icon">
                    <AppIcon name="vr" />
                  </span>
                  <h2>{currentVrLibraryScanMessage?.heading}</h2>
                  <p>{currentVrLibraryScanMessage?.message}</p>
                </div>
              )}
            </section>
          ) : activeDestination.id === "downloads" ? (
            <section aria-labelledby="vr-downloads-heading" className="vr-downloads">
              <div className="library-toolbar">
                <div>
                  <p className="card-eyebrow">Selected-file transfers</p>
                  <h2 id="vr-downloads-heading">VR downloads</h2>
                  <p>
                    Each row is managed independently. Cancelling keeps all
                    downloaded files and partial data.
                  </p>
                </div>
                <Button
                  disabled={vrDownloadsState.status === "loading"}
                  id="vr-downloads-refresh"
                  onClick={() => void retryVrDownloads()}
                  type="button"
                  variant="outline"
                >
                  <AppIcon name="refresh" />
                  Refresh
                </Button>
              </div>
              {vrDownloadsState.status === "ready" ? (
                <div
                  aria-atomic="true"
                  aria-live="polite"
                  className="vr-downloads__summary"
                  role="status"
                >
                  <p className="card-eyebrow">Aggregate activity</p>
                  <dl aria-label="VR downloads aggregate status">
                    <div>
                      <dt>Network-active transfers</dt>
                      <dd>{vrDownloadSummary.activeCount}</dd>
                    </div>
                    <div>
                      <dt>Current download speed</dt>
                      <dd>
                        {formatStorageBytes(
                          vrDownloadSummary.aggregateSpeedBytesPerSecond,
                        )}
                        /s
                      </dd>
                    </div>
                  </dl>
                </div>
              ) : null}
              {vrDownloadsState.status === "ready" &&
              vrDownloadsState.downloads.length > 0 ? (
                <div className="vr-downloads__list">
                  {vrDownloadsState.downloads.map((download) => (
                    <VrDownloadCard
                      download={download}
                      error={vrDownloadErrors[download.transferId] ?? null}
                      isPending={pendingVrDownloadIds.has(download.transferId)}
                      key={download.transferId}
                      onApplyOrganization={() =>
                        void applyDownloadOrganization()
                      }
                      onCancel={() =>
                        void runVrDownloadAction(download, "cancel")
                      }
                      onCloseOrganization={closeDownloadOrganization}
                      onDismiss={() =>
                        void runVrDownloadAction(download, "dismiss")
                      }
                      onPause={() =>
                        void runVrDownloadAction(download, "pause")
                      }
                      onPreviewOrganization={() =>
                        void previewDownloadOrganization(download)
                      }
                      onResume={() =>
                        void runVrDownloadAction(download, "resume")
                      }
                      organizationPreview={
                        vrOrganizationPreview?.transferId === download.transferId
                          ? vrOrganizationPreview
                          : null
                      }
                    />
                  ))}
                </div>
              ) : (
                <div
                  className="empty-state"
                  role={vrDownloadsState.status === "error" ? "alert" : "status"}
                >
                  <span className="empty-state__icon">
                    <AppIcon name="downloads" />
                  </span>
                  <h2>
                    {vrDownloadsState.status === "loading"
                      ? "Loading VR downloads"
                      : vrDownloadsState.status === "error"
                        ? "VR downloads could not be loaded"
                        : activeDestination.emptyHeading}
                  </h2>
                  <p>
                    {vrDownloadsState.status === "loading"
                      ? "Validating saved transfers and their selected files."
                      : vrDownloadsState.status === "error"
                        ? "Retry to validate the local transfer state again."
                        : activeDestination.emptyMessage}
                  </p>
                </div>
              )}
            </section>
          ) : (
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
                aria-labelledby="tv-folder-heading"
                className="settings-card"
              >
                <div className="settings-card__heading">
                  <span className="empty-state__icon">
                    <AppIcon name="tv" />
                  </span>
                  <div>
                    <h2 id="tv-folder-heading">TV folder</h2>
                    <p>
                      Choose one local folder to scan recursively for supported
                      TV episode files. Auto-Video never renames or moves them.
                    </p>
                  </div>
                </div>

                <div className="folder-setting">
                  {tvFolderState.status === "ready" ||
                  tvFolderState.status === "unavailable" ? (
                    <div>
                      <p className="field-label">Configured folder</p>
                      <p className="folder-path">{tvFolderState.path}</p>
                      {tvFolderState.status === "unavailable" ? (
                        <p className="field-error" role="alert">
                          This folder has moved or is unavailable. Restore it,
                          choose another folder, or clear the configuration.
                        </p>
                      ) : null}
                    </div>
                  ) : (
                    <p
                      className={
                        tvFolderState.status === "error"
                          ? "field-error folder-setting__empty"
                          : "folder-setting__empty"
                      }
                      role={tvFolderState.status === "error" ? "alert" : undefined}
                    >
                      {tvFolderState.status === "loading"
                        ? "Loading TV folder configuration…"
                        : tvFolderState.status === "error"
                          ? "The TV folder configuration could not be loaded."
                          : "No TV folder configured."}
                    </p>
                  )}
                  <div className="folder-setting__actions">
                    <Button
                      disabled={
                        isChoosingTvFolder || tvFolderState.status === "loading"
                      }
                      onClick={() => void chooseConfiguredTvFolder()}
                      type="button"
                    >
                      <AppIcon name="folder" />
                      {isChoosingTvFolder
                        ? "Choosing…"
                        : tvFolderState.status === "ready" ||
                            tvFolderState.status === "unavailable"
                          ? "Change TV folder"
                          : "Choose TV folder"}
                    </Button>
                    {tvFolderState.status === "ready" ||
                    tvFolderState.status === "unavailable" ? (
                      <Button
                        aria-label="Clear TV folder"
                        disabled={isChoosingTvFolder}
                        onClick={() => void clearConfiguredTvFolder()}
                        type="button"
                        variant="outline"
                      >
                        Clear folder
                      </Button>
                    ) : null}
                  </div>
                  {tvFolderActionError === null ? null : (
                    <p className="field-error" role="alert">
                      {tvFolderActionError}
                    </p>
                  )}
                </div>
              </section>

              <section
                aria-labelledby="vr-folder-heading"
                className="settings-card"
              >
                <div className="settings-card__heading">
                  <span className="empty-state__icon">
                    <AppIcon name="vr" />
                  </span>
                  <div>
                    <h2 id="vr-folder-heading">VR folder</h2>
                    <p>
                      Selected files from future VR downloads are saved here.
                      Changing this folder does not move or redirect existing
                      transfers.
                    </p>
                  </div>
                </div>

                <div className="folder-setting">
                  {vrFolderState.status === "ready" ||
                  vrFolderState.status === "unavailable" ? (
                    <div>
                      <p className="field-label">Configured folder</p>
                      <p className="folder-path">{vrFolderState.path}</p>
                      {vrFolderState.status === "unavailable" ? (
                        <p className="field-error" role="alert">
                          This folder has moved or is unavailable. Existing
                          transfers will not fall back to another folder.
                        </p>
                      ) : null}
                    </div>
                  ) : (
                    <p
                      className={
                        vrFolderState.status === "error"
                          ? "field-error folder-setting__empty"
                          : "folder-setting__empty"
                      }
                      role={vrFolderState.status === "error" ? "alert" : undefined}
                    >
                      {vrFolderState.status === "loading"
                        ? "Loading VR folder configuration…"
                        : vrFolderState.status === "error"
                          ? "The VR folder configuration could not be loaded."
                          : "No VR folder configured."}
                    </p>
                  )}
                  <div className="folder-setting__actions">
                    <Button
                      disabled={
                        isChoosingVrFolder || vrFolderState.status === "loading"
                      }
                      onClick={() => void chooseConfiguredVrFolder()}
                      type="button"
                    >
                      <AppIcon name="folder" />
                      {isChoosingVrFolder
                        ? "Choosing…"
                        : vrFolderState.status === "ready" ||
                            vrFolderState.status === "unavailable"
                          ? "Change VR folder"
                          : "Choose VR folder"}
                    </Button>
                    {vrFolderState.status === "ready" ||
                    vrFolderState.status === "unavailable" ? (
                      <Button
                        disabled={isChoosingVrFolder}
                        onClick={() => void clearConfiguredVrFolder()}
                        type="button"
                        variant="outline"
                      >
                        Clear folder
                      </Button>
                    ) : null}
                  </div>
                  {vrFolderActionError === null ? null : (
                    <p className="field-error" role="alert">
                      {vrFolderActionError}
                    </p>
                  )}
                </div>
              </section>

              <section
                aria-labelledby="vr-download-limit-heading"
                className="settings-card"
              >
                <div className="settings-card__heading">
                  <span className="empty-state__icon">
                    <AppIcon name="downloads" />
                  </span>
                  <div>
                    <h2 id="vr-download-limit-heading">VR download limit</h2>
                    <p>
                      Set one aggregate limit for all current and future VR
                      downloads. Limits use whole MiB per second.
                    </p>
                  </div>
                </div>

                <form
                  className="credential-setting download-limit-setting"
                  noValidate
                  onSubmit={saveConfiguredVrDownloadLimit}
                >
                  <p
                    className={
                      vrDownloadLimitState.status === "error"
                        ? "field-error credential-setting__status"
                        : "credential-setting__status"
                    }
                    id="vr-download-limit-status"
                    role={
                      vrDownloadLimitState.status === "error"
                        ? "alert"
                        : undefined
                    }
                  >
                    {vrDownloadLimitState.status === "loading"
                      ? "Loading the native-owned aggregate limit…"
                      : vrDownloadLimitState.status === "error"
                        ? "The aggregate limit could not be loaded or applied. Eligible saved transfers remain non-running."
                        : `Current limit: ${formatVrDownloadLimit(vrDownloadLimitState.limit)}.`}
                  </p>
                  {vrDownloadLimitState.status === "ready" ? (
                    <>
                      <fieldset className="appearance-options download-limit-options">
                        <legend>Limit mode</legend>
                        <div>
                          {([
                            ["unlimited", "Unlimited"],
                            ["limited", "Finite"],
                          ] as const).map(([mode, label]) => (
                            <label className="appearance-option" key={mode}>
                              <input
                                checked={vrDownloadLimitMode === mode}
                                disabled={isSavingVrDownloadLimit}
                                name="vr-download-limit-mode"
                                onChange={() => {
                                  setVrDownloadLimitMode(mode);
                                  setVrDownloadLimitMessage(null);
                                }}
                                type="radio"
                                value={mode}
                              />
                              <span className="appearance-option__content">
                                {label}
                              </span>
                            </label>
                          ))}
                        </div>
                      </fieldset>
                      <label className="field-label" htmlFor="vr-download-limit-value">
                        Finite limit (MiB/s)
                      </label>
                      <input
                        aria-describedby="vr-download-limit-help"
                        className="credential-input"
                        disabled={
                          isSavingVrDownloadLimit ||
                          vrDownloadLimitMode === "unlimited"
                        }
                        id="vr-download-limit-value"
                        inputMode="numeric"
                        max="4095"
                        min="1"
                        onChange={(event) => {
                          setVrDownloadLimitInput(event.target.value);
                          setVrDownloadLimitMessage(null);
                        }}
                        step="1"
                        type="number"
                        value={vrDownloadLimitInput}
                      />
                      <p className="field-help" id="vr-download-limit-help">
                        Enter a whole number from 1 to 4095. Unlimited removes
                        the aggregate download cap.
                      </p>
                      <div className="folder-setting__actions">
                        <Button disabled={isSavingVrDownloadLimit} type="submit">
                          {isSavingVrDownloadLimit ? "Applying…" : "Apply limit"}
                        </Button>
                      </div>
                    </>
                  ) : vrDownloadLimitState.status === "error" ? (
                    <div className="folder-setting__actions">
                      <Button
                        onClick={() => void retryVrDownloads()}
                        type="button"
                        variant="outline"
                      >
                        <AppIcon name="refresh" />
                        Retry limit
                      </Button>
                    </div>
                  ) : null}
                  {vrDownloadLimitMessage === null ? null : (
                    <p
                      className={
                        vrDownloadLimitMessage.role === "alert"
                          ? "field-error"
                          : "field-success"
                      }
                      role={vrDownloadLimitMessage.role}
                    >
                      {vrDownloadLimitMessage.text}
                    </p>
                  )}
                </form>
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
            </div>
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
      <Dialog.Root
        onOpenChange={(open) => {
          if (!open && torrentInspectionContext === null) {
            closeVrReleaseComparison();
          }
        }}
        open={releaseComparisonItem !== null}
      >
        {releaseComparisonItem === null ||
        releaseComparisonState === null ||
        releaseComparisonTriggerId === null ? null : (
          <VrReleaseComparison
            item={releaseComparisonItem}
            onInspectRelease={openVrTorrentInspection}
            onRetry={retryVrReleaseComparison}
            onSelectRelease={selectVrRelease}
            selectedRelease={selectedVrRelease}
            state={releaseComparisonState}
            triggerId={releaseComparisonTriggerId}
          />
        )}
      </Dialog.Root>
      <Dialog.Root
        onOpenChange={(open) => {
          if (!open) {
            closeVrTorrentInspection();
          }
        }}
        open={torrentInspectionContext !== null}
      >
        {torrentInspectionContext === null ||
        torrentInspectionState === null ? null : (
          <VrTorrentInspectionDialog
            context={torrentInspectionContext}
            downloadsReady={vrDownloadsState.status === "ready"}
            folderState={vrFolderState}
            onOpenDownloads={() =>
              openVrDestinationFromInspection(downloadsDestination)
            }
            onOpenSettings={() =>
              openVrDestinationFromInspection(settingsDestination)
            }
            onRetry={retryVrTorrentInspection}
            onSave={() => void saveVrTorrent()}
            onStart={() => void startVrDownload()}
            onToggleFile={toggleTorrentFile}
            saveState={torrentSaveState}
            selectedFileIds={selectedTorrentFileIds}
            startState={torrentStartState}
            state={torrentInspectionState}
          />
        )}
      </Dialog.Root>
    </>
  );
}
