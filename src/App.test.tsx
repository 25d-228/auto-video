import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import {
  afterEach,
  beforeEach,
  describe,
  expect,
  it,
  type Mock,
  vi,
} from "vitest";

import App from "./App";

const systemDarkModeQuery = "(prefers-color-scheme: dark)";

type ResizeObserverRecord = {
  callback: ResizeObserverCallback;
  observer: ResizeObserver;
  targets: Set<Element>;
};

let systemPrefersDark = false;
let mediaQueryListeners = new Set<(event: MediaQueryListEvent) => void>();
let invokeMock: Mock<
  (
    command: string,
    parameters?: Record<string, unknown>,
  ) => Promise<unknown>
>;
let scanMoviesMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<string[]>
>;
let queryMoviesStorageMock: Mock<() => Promise<[string, string]>>;
let openMovieMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<void>
>;
let revealMovieMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<void>
>;
let trashMovieMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<void>
>;
let loadMoviesFolderMock: Mock<() => Promise<string | null>>;
let openFolderMock: Mock<() => Promise<string | null>>;
let clearMoviesFolderMock: Mock<() => Promise<void>>;
let loadTvFolderMock: Mock<() => Promise<string[]>>;
let chooseTvFolderMock: Mock<() => Promise<string | null>>;
let clearTvFolderMock: Mock<() => Promise<void>>;
let scanTvLibraryMock: Mock<() => Promise<string[]>>;
let queryTvStorageMock: Mock<() => Promise<[string, string]>>;
let openTvFileMock: Mock<(parameters?: Record<string, unknown>) => Promise<void>>;
let revealTvFileMock: Mock<(parameters?: Record<string, unknown>) => Promise<void>>;
let trashTvFileMock: Mock<(parameters?: Record<string, unknown>) => Promise<void>>;
let loadAdultFolderMock: Mock<() => Promise<string[]>>;
let chooseAdultFolderMock: Mock<() => Promise<string | null>>;
let clearAdultFolderMock: Mock<() => Promise<void>>;
let scanAdultLibraryMock: Mock<() => Promise<string[]>>;
let queryAdultStorageMock: Mock<() => Promise<[string, string]>>;
let openAdultFileMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<void>
>;
let revealAdultFileMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<void>
>;
let loadTmdbTokenMock: Mock<() => Promise<string | null>>;
let saveTmdbTokenMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<void>
>;
let clearTmdbTokenMock: Mock<() => Promise<void>>;
let fetchYtsMovieReleasesMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<string[]>
>;
let fetchApiBayTvReleasesMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<string[]>
>;
let selectApiBayTvReleaseMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<void>
>;
let inspectApiBayTvTorrentMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<string[]>
>;
let invalidateVerifiedTvTorrentMock: Mock<() => Promise<void>>;
let invalidateTvReleaseContextMock: Mock<() => Promise<void>>;
let saveVerifiedTvTorrentMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<boolean>
>;
let inspectYtsMovieTorrentMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<string[]>
>;
let invalidateVerifiedMovieTorrentMock: Mock<() => Promise<void>>;
let invalidateMovieReleaseContextMock: Mock<() => Promise<void>>;
let saveVerifiedMovieTorrentMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<boolean>
>;
let fetchJavdbCatalogMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<string>
>;
let fetchSukebeiAdultReleasesMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<string>
>;
let fetchSukebeiVrReleasesMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<string>
>;
let inspectSukebeiAdultTorrentMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<string[]>
>;
let invalidateVerifiedAdultTorrentMock: Mock<() => Promise<void>>;
let saveVerifiedAdultTorrentMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<boolean>
>;
let inspectSukebeiVrTorrentMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<string[]>
>;
let invalidateVerifiedVrTorrentMock: Mock<() => Promise<void>>;
let saveVerifiedVrTorrentMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<boolean>
>;
let loadVrFolderMock: Mock<() => Promise<string[]>>;
let chooseVrFolderMock: Mock<() => Promise<string | null>>;
let clearVrFolderMock: Mock<() => Promise<void>>;
let scanVrLibraryMock: Mock<() => Promise<string[]>>;
let queryVrStorageMock: Mock<() => Promise<[string, string]>>;
let openVrFileMock: Mock<(parameters?: Record<string, unknown>) => Promise<void>>;
let revealVrFileMock: Mock<(parameters?: Record<string, unknown>) => Promise<void>>;
let loadVrDownloadLimitMock: Mock<() => Promise<string[]>>;
let saveVrDownloadLimitMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<string[]>
>;
let loadVrDownloadsMock: Mock<() => Promise<string[]>>;
let listVrDownloadsMock: Mock<() => Promise<string[]>>;
let startVerifiedVrDownloadMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<string>
>;
let startVerifiedAdultDownloadMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<string>
>;
let startVerifiedMovieDownloadMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<string>
>;
let pauseVrDownloadMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<void>
>;
let resumeVrDownloadMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<void>
>;
let cancelVrDownloadMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<void>
>;
let dismissVrDownloadMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<void>
>;
let previewVrOrganizationMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<string[]>
>;
let applyVrOrganizationMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<void>
>;
let dismissVrOrganizationMock: Mock<() => Promise<void>>;
let fetchMock: Mock<typeof fetch>;
let clipboardWriteMock: Mock<(text: string) => Promise<void>>;
let resizeObserverRecords: ResizeObserverRecord[] = [];
let gallerySizes: Record<
  "discover" | "library",
  { width: number; height: number }
>;
let savedMoviesFolder: string | null;
let savedTvFolder: string | null;
let savedAdultFolder: string | null;
let savedVrFolder: string | null;

function createResizeEntry(
  target: Element,
  width: number,
  height: number,
): ResizeObserverEntry {
  const contentRect = {
    bottom: height,
    height,
    left: 0,
    right: width,
    top: 0,
    width,
    x: 0,
    y: 0,
    toJSON: () => ({}),
  } as DOMRectReadOnly;

  return {
    borderBoxSize: [],
    contentBoxSize: [],
    contentRect,
    devicePixelContentBoxSize: [],
    target,
  };
}

class TestResizeObserver implements ResizeObserver {
  private readonly record: ResizeObserverRecord;

  constructor(callback: ResizeObserverCallback) {
    this.record = {
      callback,
      observer: this,
      targets: new Set(),
    };
    resizeObserverRecords.push(this.record);
  }

  observe(target: Element) {
    this.record.targets.add(target);
    const gallery = target.closest<HTMLElement>("[data-gallery]");
    const variant = gallery?.dataset.gallery;
    const size =
      variant === "discover" || variant === "library"
        ? gallerySizes[variant]
        : { width: 2000, height: 3000 };
    this.record.callback(
      [createResizeEntry(target, size.width, size.height)],
      this,
    );
  }

  unobserve(target: Element) {
    this.record.targets.delete(target);
  }

  disconnect() {
    this.record.targets.clear();
  }
}

function createMediaQueryList(query: string): MediaQueryList {
  return {
    matches: systemPrefersDark,
    media: query,
    onchange: null,
    addListener: () => undefined,
    removeListener: () => undefined,
    addEventListener: (
      _type: string,
      listener: EventListenerOrEventListenerObject,
    ) => {
      if (typeof listener === "function") {
        mediaQueryListeners.add(
          listener as (event: MediaQueryListEvent) => void,
        );
      }
    },
    removeEventListener: (
      _type: string,
      listener: EventListenerOrEventListenerObject,
    ) => {
      if (typeof listener === "function") {
        mediaQueryListeners.delete(
          listener as (event: MediaQueryListEvent) => void,
        );
      }
    },
    dispatchEvent: () => true,
  };
}

function createDeferred<T>() {
  let resolvePromise: (value: T) => void = () => undefined;
  let rejectPromise: (reason?: unknown) => void = () => undefined;
  const promise = new Promise<T>((resolve, reject) => {
    resolvePromise = resolve;
    rejectPromise = reject;
  });
  return { promise, reject: rejectPromise, resolve: resolvePromise };
}

function selectLibrary() {
  fireEvent.click(screen.getByRole("button", { name: "Library" }));
}

function selectDashboard() {
  fireEvent.click(screen.getByRole("button", { name: "Dashboard" }));
}

function selectDiscover() {
  fireEvent.click(screen.getByRole("button", { name: "Discover" }));
}

function selectVrDiscover() {
  fireEvent.click(screen.getByRole("radio", { name: "VR" }));
}

function selectAdultDiscover() {
  fireEvent.click(screen.getByRole("radio", { name: "Adult" }));
}

function selectTvDiscover() {
  fireEvent.click(screen.getByRole("radio", { name: "TV" }));
}

function selectSettings() {
  fireEvent.click(screen.getByRole("button", { name: "Settings" }));
}

async function openVrReleaseComparison(code = "MDVR-419") {
  render(<App />);
  selectDiscover();
  selectVrDiscover();
  fireEvent.change(
    screen.getByRole("textbox", { name: "Search product code" }),
    { target: { value: code } },
  );
  fireEvent.click(screen.getByRole("button", { name: "Search" }));
  const trigger = await screen.findByRole("button", {
    name: `Find releases: ${code}`,
  });
  fireEvent.click(trigger);
  return screen.findByRole("list", {
    name: `Verified releases for ${code}`,
  });
}

async function openAdultReleaseComparison(code = "ADLT-123") {
  render(<App />);
  selectDiscover();
  selectAdultDiscover();
  fireEvent.change(
    screen.getByRole("textbox", { name: "Search product code" }),
    { target: { value: code } },
  );
  fireEvent.click(screen.getByRole("button", { name: "Search" }));
  const trigger = await screen.findByRole("button", {
    name: `Find releases: ${code}`,
  });
  fireEvent.click(trigger);
  return screen.findByRole("list", {
    name: `Verified Adult releases for ${code}`,
  });
}

function jsonResponse(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), {
    headers: { "Content-Type": "application/json" },
    status,
  });
}

function javdbCatalogFixture(
  code: string,
  title = "Provider VR title",
  cover = "https://images.example/vr-cover.jpg",
) {
  return `<!doctype html><html><body><div class="movie-list">
    <div class="item"><a class="box" href="/v/item">
      <img data-src="${cover}">
      <div class="video-title"><strong>${code}</strong> ${title}</div>
    </a></div>
  </div></body></html>`;
}

function sukebeiReleaseFixture(
  releases: Array<{
    infohash?: string;
    itemId?: string;
    name: string;
    seeders?: number;
    size?: string;
    torrentUrl?: string;
  }>,
) {
  return `<rss xmlns:nyaa="https://sukebei.nyaa.si/xmlns/nyaa" version="2.0">
    <channel><title>Sukebei results</title>${releases
      .map(
        ({ infohash, itemId, name, seeders, size, torrentUrl }) => `<item><title>${name}</title>
          ${size === undefined ? "" : `<nyaa:size>${size}</nyaa:size>`}
          ${seeders === undefined ? "" : `<nyaa:seeders>${seeders}</nyaa:seeders>`}
          ${itemId === undefined ? "" : `<guid>https://sukebei.nyaa.si/view/${itemId}</guid>`}
          ${torrentUrl === undefined && itemId === undefined ? "" : `<link>${torrentUrl ?? `https://sukebei.nyaa.si/download/${itemId}.torrent`}</link>`}
          ${infohash === undefined ? "" : `<nyaa:infoHash>${infohash}</nyaa:infoHash>`}
        </item>`,
      )
      .join("")}</channel>
  </rss>`;
}

function ytsMovieReleaseFixture({
  providerMovieId = "700",
  providerTitle = "YTS  Exact — 特別版",
  providerYear = "1999",
  releases,
  tmdbMovieId = "419",
  tmdbTitle = "Exact  Movie — 特別版",
}: {
  providerMovieId?: string;
  providerTitle?: string;
  providerYear?: string;
  releases: Array<{
    expectedInfohash?: string;
    peers?: string;
    quality?: string;
    rowId: string;
    seeds?: string;
    size?: string;
    sizeBytes?: string;
    torrentUrl?: string;
    typeLabel?: string;
    videoCodec?: string;
  }>;
  tmdbMovieId?: string;
  tmdbTitle?: string;
}) {
  return [
    tmdbMovieId,
    tmdbTitle,
    "1999-04-19",
    "tt0123456",
    providerMovieId,
    providerTitle,
    providerYear,
    String(releases.length),
    ...releases.flatMap((release) => [
      release.rowId,
      release.quality ?? "",
      release.typeLabel ?? "",
      release.videoCodec ?? "",
      release.size ?? "",
      release.sizeBytes ?? "",
      release.seeds ?? "",
      release.peers ?? "",
      release.expectedInfohash ?? "",
      release.torrentUrl ?? "",
    ]),
  ];
}

function vrDownloadFixture({
  canOrganize = "false",
  category = "vr",
  code = "MDVR-419",
  downloadedBytes = "5",
  releaseName,
  selectedFileCount = "1",
  speedBytesPerSecond = "1024",
  state,
  totalBytes = "10",
  transferId,
  isCurrentFolder = "true",
  organizationRelativeDirectory = "",
  organizationStatus = "none",
}: {
  canOrganize?: string;
  category?: string;
  code?: string;
  downloadedBytes?: string;
  releaseName: string;
  selectedFileCount?: string;
  speedBytesPerSecond?: string;
  state: string;
  totalBytes?: string;
  transferId: string;
  isCurrentFolder?: string;
  organizationRelativeDirectory?: string;
  organizationStatus?: string;
}) {
  return [
    transferId,
    category,
    code,
    releaseName,
    selectedFileCount,
    totalBytes,
    downloadedBytes,
    speedBytesPerSecond,
    state,
    isCurrentFolder,
    organizationStatus,
    organizationRelativeDirectory,
    canOrganize,
  ];
}

function setSystemPreference(prefersDark: boolean) {
  systemPrefersDark = prefersDark;
  act(() => {
    for (const listener of mediaQueryListeners) {
      listener({
        matches: prefersDark,
        media: systemDarkModeQuery,
      } as MediaQueryListEvent);
    }
  });
}

function resizeGallery(
  variant: "discover" | "library",
  width: number,
  height: number,
) {
  const viewport = document.querySelector(
    `[data-gallery="${variant}"] .media-gallery__viewport`,
  );
  if (viewport === null) {
    throw new Error(`The ${variant} gallery viewport was not rendered.`);
  }

  act(() => {
    gallerySizes[variant] = { width, height };
    for (const record of resizeObserverRecords) {
      if (record.targets.has(viewport)) {
        record.callback(
          [createResizeEntry(viewport, width, height)],
          record.observer,
        );
      }
    }
  });
}

function visibleCardCount(listName: string) {
  return within(screen.getByRole("list", { name: listName })).getAllByRole(
    "article",
  ).length;
}

function visibleMovieTitles() {
  return within(screen.getByRole("list", { name: "Movies" }))
    .getAllByRole("heading", { level: 3 })
    .map((heading) => heading.textContent);
}

function storageValue(label: "Total" | "Used" | "Free") {
  const term = screen.getByText(label, { selector: "dt" });
  return term.parentElement?.querySelector("dd")?.textContent;
}

function searchMovies(query: string) {
  fireEvent.change(screen.getByRole("textbox", { name: "Search titles" }), {
    target: { value: query },
  });
}

function submitDiscoverSearch(query: string) {
  fireEvent.change(screen.getByRole("textbox", { name: "Search Movies" }), {
    target: { value: query },
  });
  fireEvent.click(screen.getByRole("button", { name: "Search" }));
}

function submitTvDiscoverSearch(query: string) {
  fireEvent.change(screen.getByRole("textbox", { name: "Search TV" }), {
    target: { value: query },
  });
  fireEvent.click(screen.getByRole("button", { name: "Search" }));
}

function sortMovies(direction: "ascending" | "descending") {
  fireEvent.change(screen.getByRole("combobox", { name: "Sort titles" }), {
    target: { value: direction },
  });
}

beforeEach(() => {
  systemPrefersDark = false;
  mediaQueryListeners = new Set();
  resizeObserverRecords = [];
  gallerySizes = {
    discover: { width: 2000, height: 3000 },
    library: { width: 2000, height: 3000 },
  };
  savedMoviesFolder = null;
  savedTvFolder = null;
  savedAdultFolder = null;
  savedVrFolder = null;
  scanMoviesMock = vi.fn().mockResolvedValue([]);
  queryMoviesStorageMock = vi
    .fn()
    .mockResolvedValue(["1099511627776", "274877906944"]);
  openMovieMock = vi.fn().mockResolvedValue(undefined);
  revealMovieMock = vi.fn().mockResolvedValue(undefined);
  trashMovieMock = vi.fn().mockResolvedValue(undefined);
  loadMoviesFolderMock = vi
    .fn()
    .mockImplementation(() => Promise.resolve(savedMoviesFolder));
  openFolderMock = vi.fn().mockResolvedValue(null);
  clearMoviesFolderMock = vi.fn().mockResolvedValue(undefined);
  loadTvFolderMock = vi.fn().mockImplementation(() =>
    Promise.resolve(
      savedTvFolder === null ? ["unconfigured"] : ["ready", savedTvFolder],
    ),
  );
  chooseTvFolderMock = vi.fn().mockResolvedValue(null);
  clearTvFolderMock = vi.fn().mockResolvedValue(undefined);
  scanTvLibraryMock = vi.fn().mockResolvedValue([]);
  queryTvStorageMock = vi
    .fn()
    .mockResolvedValue(["3298534883328", "1099511627776"]);
  openTvFileMock = vi.fn().mockResolvedValue(undefined);
  revealTvFileMock = vi.fn().mockResolvedValue(undefined);
  trashTvFileMock = vi.fn().mockResolvedValue(undefined);
  loadAdultFolderMock = vi.fn().mockImplementation(() =>
    Promise.resolve(
      savedAdultFolder === null
        ? ["unconfigured"]
        : ["ready", savedAdultFolder],
    ),
  );
  chooseAdultFolderMock = vi.fn().mockResolvedValue(null);
  clearAdultFolderMock = vi.fn().mockResolvedValue(undefined);
  scanAdultLibraryMock = vi.fn().mockResolvedValue([]);
  queryAdultStorageMock = vi
    .fn()
    .mockResolvedValue(["4398046511104", "1099511627776"]);
  openAdultFileMock = vi.fn().mockResolvedValue(undefined);
  revealAdultFileMock = vi.fn().mockResolvedValue(undefined);
  loadTmdbTokenMock = vi.fn().mockResolvedValue(null);
  saveTmdbTokenMock = vi.fn().mockResolvedValue(undefined);
  clearTmdbTokenMock = vi.fn().mockResolvedValue(undefined);
  fetchJavdbCatalogMock = vi
    .fn()
    .mockResolvedValue('<div class="movie-list"></div>');
  fetchSukebeiAdultReleasesMock = vi
    .fn()
    .mockResolvedValue(sukebeiReleaseFixture([]));
  fetchSukebeiVrReleasesMock = vi
    .fn()
    .mockResolvedValue(sukebeiReleaseFixture([]));
  inspectSukebeiAdultTorrentMock = vi.fn().mockResolvedValue([
    "adult-1-1-321",
    "Verified Adult torrent",
    "0123456789abcdef0123456789abcdef01234567",
    "5",
    "Verified Adult file.mp4",
    "5",
  ]);
  invalidateVerifiedAdultTorrentMock = vi.fn().mockResolvedValue(undefined);
  saveVerifiedAdultTorrentMock = vi.fn().mockResolvedValue(true);
  fetchYtsMovieReleasesMock = vi.fn().mockResolvedValue([
    "419",
    "Exact Movie",
    "1999-04-19",
    "tt0123456",
    "700",
    "Exact YTS Movie",
    "1999",
    "0",
  ]);
  fetchApiBayTvReleasesMock = vi.fn().mockResolvedValue([
    "701",
    "Exact  Show — 特別版",
    "9001",
    "2",
    "9103",
    "3",
    "第三話  —  Exact Episode",
    "tt0123456",
    "0",
  ]);
  selectApiBayTvReleaseMock = vi.fn().mockResolvedValue(undefined);
  inspectApiBayTvTorrentMock = vi.fn().mockResolvedValue([
    "tv-1-1-1001",
    "Exact  Show — 特別版 S02E03",
    "0123456789abcdef0123456789abcdef01234567",
    "5",
    "Exact  Show — 特別版/第三話  —  Exact Episode.mkv",
    "5",
  ]);
  invalidateVerifiedTvTorrentMock = vi.fn().mockResolvedValue(undefined);
  invalidateTvReleaseContextMock = vi.fn().mockResolvedValue(undefined);
  saveVerifiedTvTorrentMock = vi.fn().mockResolvedValue(true);
  inspectYtsMovieTorrentMock = vi.fn().mockResolvedValue([
    "movie-1-1-hash",
    "Verified Movie torrent",
    "0123456789abcdef0123456789abcdef01234567",
    "5",
    "Verified Movie.mp4",
    "5",
  ]);
  invalidateVerifiedMovieTorrentMock = vi.fn().mockResolvedValue(undefined);
  invalidateMovieReleaseContextMock = vi.fn().mockResolvedValue(undefined);
  saveVerifiedMovieTorrentMock = vi.fn().mockResolvedValue(true);
  inspectSukebeiVrTorrentMock = vi.fn().mockResolvedValue([
    "inspection-123",
    "Verified torrent",
    "0123456789abcdef0123456789abcdef01234567",
    "5",
    "Verified file.mp4",
    "5",
  ]);
  invalidateVerifiedVrTorrentMock = vi.fn().mockResolvedValue(undefined);
  saveVerifiedVrTorrentMock = vi.fn().mockResolvedValue(true);
  loadVrFolderMock = vi.fn().mockImplementation(() =>
    Promise.resolve(
      savedVrFolder === null ? ["unconfigured"] : ["ready", savedVrFolder],
    ),
  );
  chooseVrFolderMock = vi.fn().mockResolvedValue(null);
  clearVrFolderMock = vi.fn().mockResolvedValue(undefined);
  scanVrLibraryMock = vi.fn().mockResolvedValue([]);
  queryVrStorageMock = vi
    .fn()
    .mockResolvedValue(["2199023255552", "549755813888"]);
  openVrFileMock = vi.fn().mockResolvedValue(undefined);
  revealVrFileMock = vi.fn().mockResolvedValue(undefined);
  loadVrDownloadLimitMock = vi.fn().mockResolvedValue(["unlimited"]);
  saveVrDownloadLimitMock = vi
    .fn()
    .mockImplementation((parameters) =>
      Promise.resolve(
        parameters?.mibPerSecond === null
          ? ["unlimited"]
          : ["limited", parameters?.mibPerSecond as string],
      ),
    );
  loadVrDownloadsMock = vi.fn().mockResolvedValue([]);
  listVrDownloadsMock = vi.fn().mockResolvedValue([]);
  startVerifiedVrDownloadMock = vi.fn().mockResolvedValue("transfer-123");
  startVerifiedAdultDownloadMock = vi.fn().mockResolvedValue("adult-transfer-123");
  startVerifiedMovieDownloadMock = vi.fn().mockResolvedValue("movie-transfer-123");
  pauseVrDownloadMock = vi.fn().mockResolvedValue(undefined);
  resumeVrDownloadMock = vi.fn().mockResolvedValue(undefined);
  cancelVrDownloadMock = vi.fn().mockResolvedValue(undefined);
  dismissVrDownloadMock = vi.fn().mockResolvedValue(undefined);
  previewVrOrganizationMock = vi.fn().mockImplementation((parameters) =>
    Promise.resolve([
      "plan-123",
      parameters?.transferId as string,
      "MDVR-419",
      "1",
      "1",
      "move",
      "Source/MDVR-419.mp4",
      "MDVR-419/MDVR-419.mp4",
    ]),
  );
  applyVrOrganizationMock = vi.fn().mockResolvedValue(undefined);
  dismissVrOrganizationMock = vi.fn().mockResolvedValue(undefined);
  invokeMock = vi.fn(
    (command: string, parameters?: Record<string, unknown>) => {
      switch (command) {
        case "load_movies_folder":
          return loadMoviesFolderMock();
        case "choose_movies_folder":
          return openFolderMock().then((selectedFolder) => {
            if (selectedFolder !== null) {
              savedMoviesFolder = selectedFolder;
            }
            return selectedFolder;
          });
        case "clear_movies_folder":
          return clearMoviesFolderMock().then(() => {
            savedMoviesFolder = null;
          });
        case "scan_movies":
          return scanMoviesMock(parameters);
        case "query_movies_storage":
          return queryMoviesStorageMock();
        case "open_movie":
          return openMovieMock(parameters);
        case "reveal_movie":
          return revealMovieMock(parameters);
        case "trash_movie":
          return trashMovieMock(parameters);
        case "load_tv_folder":
          return loadTvFolderMock();
        case "choose_tv_folder":
          return chooseTvFolderMock().then((selectedFolder) => {
            if (selectedFolder !== null) {
              savedTvFolder = selectedFolder;
            }
            return selectedFolder;
          });
        case "clear_tv_folder":
          return clearTvFolderMock().then(() => {
            savedTvFolder = null;
          });
        case "scan_tv_library":
          return scanTvLibraryMock().then((rows) => ["1", ...rows]);
        case "query_tv_storage":
          return queryTvStorageMock();
        case "open_tv_file":
          return openTvFileMock(parameters);
        case "reveal_tv_file":
          return revealTvFileMock(parameters);
        case "trash_tv_file":
          return trashTvFileMock(parameters);
        case "load_adult_folder":
          return loadAdultFolderMock();
        case "choose_adult_folder":
          return chooseAdultFolderMock().then((selectedFolder) => {
            if (selectedFolder !== null) {
              savedAdultFolder = selectedFolder;
            }
            return selectedFolder;
          });
        case "clear_adult_folder":
          return clearAdultFolderMock().then(() => {
            savedAdultFolder = null;
          });
        case "scan_adult_library":
          return scanAdultLibraryMock();
        case "query_adult_storage":
          return queryAdultStorageMock();
        case "open_adult_file":
          return openAdultFileMock(parameters);
        case "reveal_adult_file":
          return revealAdultFileMock(parameters);
        case "load_tmdb_token":
          return loadTmdbTokenMock();
        case "save_tmdb_token":
          return saveTmdbTokenMock(parameters);
        case "clear_tmdb_token":
          return clearTmdbTokenMock();
        case "fetch_yts_movie_releases":
          return fetchYtsMovieReleasesMock(parameters);
        case "fetch_apibay_tv_releases":
          return fetchApiBayTvReleasesMock(parameters);
        case "select_apibay_tv_release":
          return selectApiBayTvReleaseMock(parameters);
        case "inspect_apibay_tv_torrent":
          return inspectApiBayTvTorrentMock(parameters);
        case "invalidate_verified_tv_torrent":
          return invalidateVerifiedTvTorrentMock();
        case "invalidate_tv_release_context":
          return invalidateTvReleaseContextMock();
        case "save_verified_tv_torrent":
          return saveVerifiedTvTorrentMock(parameters);
        case "inspect_yts_movie_torrent":
          return inspectYtsMovieTorrentMock(parameters);
        case "invalidate_verified_movie_torrent":
          return invalidateVerifiedMovieTorrentMock();
        case "invalidate_movie_release_context":
          return invalidateMovieReleaseContextMock();
        case "save_verified_movie_torrent":
          return saveVerifiedMovieTorrentMock(parameters);
        case "fetch_javdb_vr_catalog":
        case "fetch_javdb_adult_catalog":
          return fetchJavdbCatalogMock(parameters);
        case "fetch_sukebei_adult_releases":
          return fetchSukebeiAdultReleasesMock(parameters);
        case "fetch_sukebei_vr_releases":
          return fetchSukebeiVrReleasesMock(parameters);
        case "inspect_sukebei_adult_torrent":
          return inspectSukebeiAdultTorrentMock(parameters);
        case "invalidate_verified_adult_torrent":
          return invalidateVerifiedAdultTorrentMock();
        case "save_verified_adult_torrent":
          return saveVerifiedAdultTorrentMock(parameters);
        case "inspect_sukebei_vr_torrent":
          return inspectSukebeiVrTorrentMock(parameters);
        case "invalidate_verified_vr_torrent":
          return invalidateVerifiedVrTorrentMock();
        case "save_verified_vr_torrent":
          return saveVerifiedVrTorrentMock(parameters);
        case "load_vr_folder":
          return loadVrFolderMock();
        case "choose_vr_folder":
          return chooseVrFolderMock().then((selectedFolder) => {
            if (selectedFolder !== null) {
              savedVrFolder = selectedFolder;
            }
            return selectedFolder;
          });
        case "clear_vr_folder":
          return clearVrFolderMock().then(() => {
            savedVrFolder = null;
          });
        case "scan_vr_library":
          return scanVrLibraryMock();
        case "query_vr_storage":
          return queryVrStorageMock();
        case "open_vr_file":
          return openVrFileMock(parameters);
        case "reveal_vr_file":
          return revealVrFileMock(parameters);
        case "load_vr_download_limit":
          return loadVrDownloadLimitMock();
        case "save_vr_download_limit":
          return saveVrDownloadLimitMock(parameters);
        case "load_vr_downloads":
          return loadVrDownloadsMock();
        case "list_vr_downloads":
          return listVrDownloadsMock();
        case "start_verified_vr_download":
          return startVerifiedVrDownloadMock(parameters);
        case "start_verified_adult_download":
          return startVerifiedAdultDownloadMock(parameters);
        case "start_verified_movie_download":
          return startVerifiedMovieDownloadMock(parameters);
        case "pause_vr_download":
          return pauseVrDownloadMock(parameters);
        case "resume_vr_download":
          return resumeVrDownloadMock(parameters);
        case "cancel_vr_download":
          return cancelVrDownloadMock(parameters);
        case "dismiss_vr_download":
          return dismissVrDownloadMock(parameters);
        case "preview_vr_organization":
          return previewVrOrganizationMock(parameters);
        case "apply_vr_organization":
          return applyVrOrganizationMock(parameters);
        case "dismiss_vr_organization":
          return dismissVrOrganizationMock();
        default:
          return Promise.reject(new Error("Unexpected native command."));
      }
    },
  );
  fetchMock = vi.fn();
  clipboardWriteMock = vi.fn().mockResolvedValue(undefined);
  window.localStorage.clear();
  vi.stubGlobal("matchMedia", vi.fn(createMediaQueryList));
  vi.stubGlobal("__TAURI__", { core: { invoke: invokeMock } });
  vi.stubGlobal("fetch", fetchMock);
  vi.stubGlobal("ResizeObserver", TestResizeObserver);
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: clipboardWriteMock },
  });
});

afterEach(() => {
  cleanup();
  window.localStorage.clear();
  delete document.documentElement.dataset.appearance;
  delete document.documentElement.dataset.theme;
  Reflect.deleteProperty(navigator, "clipboard");
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("parsed TV Library and Dashboard", () => {
  it("renders loading, scanning, empty, error, ready, and no-match states deterministically", async () => {
    const folderLoad = createDeferred<string[]>();
    const firstScan = createDeferred<string[]>();
    loadTvFolderMock.mockReturnValue(folderLoad.promise);
    scanTvLibraryMock
      .mockReturnValueOnce(firstScan.promise)
      .mockRejectedValueOnce("tv_library_scan_failed")
      .mockResolvedValueOnce([
        "/TV/Ready Show.S01E02.mp4",
        "Ready Show.S01E02.mp4",
        "1",
      ]);

    render(<App />);
    selectLibrary();
    fireEvent.click(screen.getByRole("radio", { name: "TV" }));
    expect(
      screen.getByRole("heading", { level: 2, name: "Loading TV folder" }),
    ).toBeTruthy();
    await act(async () => {
      folderLoad.resolve(["ready", "/TV"]);
      await folderLoad.promise;
    });
    expect(
      screen.getByRole("heading", { level: 2, name: "Scanning TV folder" }),
    ).toBeTruthy();
    await act(async () => {
      firstScan.resolve([]);
      await firstScan.promise;
    });
    expect(
      await screen.findByRole("heading", {
        level: 2,
        name: "No supported TV videos found",
      }),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(
      await screen.findByRole("heading", {
        level: 2,
        name: "TV folder could not be scanned",
      }),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await screen.findByRole("heading", { level: 3, name: "Ready Show" });
    fireEvent.change(
      screen.getByRole("textbox", { name: "Search titles" }),
      { target: { value: "missing title" } },
    );
    expect(
      screen.getByRole("heading", {
        level: 2,
        name: "No TV items match this search",
      }),
    ).toBeTruthy();
  });

  it("chooses, changes, and clears only the native TV folder configuration", async () => {
    chooseTvFolderMock
      .mockResolvedValueOnce("/TV/First")
      .mockResolvedValueOnce("/TV/Second");

    render(<App />);
    selectSettings();
    fireEvent.click(
      await screen.findByRole("button", { name: "Choose TV folder" }),
    );
    expect(await screen.findByText("/TV/First")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Change TV folder" }));
    expect(await screen.findByText("/TV/Second")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Clear TV folder" }));
    expect(await screen.findByText("No TV folder configured.")).toBeTruthy();
    expect(chooseTvFolderMock).toHaveBeenCalledTimes(2);
    expect(clearTvFolderMock).toHaveBeenCalledTimes(1);
    expect(openFolderMock).not.toHaveBeenCalled();
    expect(chooseVrFolderMock).not.toHaveBeenCalled();
  });

  it("shows independent folder, scan, aggregate, storage, and routing states", async () => {
    savedTvFolder = "/TV/番組  Library";
    scanTvLibraryMock.mockResolvedValue([
      "/TV/番組  Library/星  Show.S01E02.mp4",
      "星  Show.S01E02.mp4",
      "1073741824",
      "/TV/番組  Library/星  Show.S01E03.MKV",
      "星  Show.S01E03.MKV",
      "2147483648",
      "/TV/番組  Library/Unknown release.mp4",
      "Unknown release.mp4",
      "5",
      "/TV/番組  Library/星  Show.S01E02-E03.mp4",
      "星  Show.S01E02-E03.mp4",
      "6",
      "/TV/番組  Library/星  Show.S01E02-03.mkv",
      "星  Show.S01E02-03.mkv",
      "7",
      "/TV/番組  Library/星  Show.1x02-03.mp4",
      "星  Show.1x02-03.mp4",
      "8",
    ]);

    render(<App />);

    expect(
      await screen.findByRole("heading", {
        level: 3,
        name: "6 supported TV files",
      }),
    ).toBeTruthy();
    expect(
      screen.getByText(
        "1 show · 2 associated episodes · 4 files remain unassociated.",
      ),
    ).toBeTruthy();
    expect(
      await screen.findByText("3.0 TiB", { selector: "dd" }),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Open TV Library" }));
    expect(screen.getByRole("radio", { name: "TV" })).toHaveProperty(
      "checked",
      true,
    );
    expect(
      screen.getByRole("list", { name: "TV shows and unassociated files" }),
    ).toBeTruthy();
    const showHeading = await screen.findByRole("heading", {
      level: 3,
      name: "星 Show",
    });
    expect(showHeading.textContent).toBe("星  Show");
    const showCard = showHeading.closest("article");
    if (showCard === null) {
      throw new Error("The grouped TV show card was not rendered.");
    }
    expect(showCard.querySelectorAll("[data-tv-file-path]")).toHaveLength(2);
    expect(screen.getByText("Season 1 · Episode 2 · 1.0 GiB")).toBeTruthy();
    expect(screen.getByText("Unknown release.mp4")).toBeTruthy();
    const visibleFilePaths = Array.from(
      document.querySelectorAll("[data-tv-file-path]"),
      (row) => row.getAttribute("data-tv-file-path"),
    );
    expect(visibleFilePaths).toEqual(
      expect.arrayContaining([
        "/TV/番組  Library/星  Show.S01E02-E03.mp4",
        "/TV/番組  Library/星  Show.S01E02-03.mkv",
        "/TV/番組  Library/星  Show.1x02-03.mp4",
      ]),
    );
  });

  it("searches then sorts then paginates immediately across 25, 7, and 10 item capacities", async () => {
    savedTvFolder = "/TV";
    scanTvLibraryMock.mockResolvedValue([
      ...Array.from({ length: 30 }, (_, index) => {
        const title = `Show ${String(index + 1).padStart(2, "0")}`;
        const filename = `${title}.S01E01.mp4`;
        return [`/TV/${filename}`, filename, "1"];
      }).flat(),
      "/TV/Unassociated special.mp4",
      "Unassociated special.mp4",
      "1",
    ]);
    gallerySizes.library = { width: 1088, height: 728 };

    render(<App />);
    selectLibrary();
    fireEvent.click(screen.getByRole("radio", { name: "TV" }));
    await screen.findByRole("heading", { level: 3, name: "Show 01" });
    const gallery = document.querySelector('[data-gallery="library"]');
    expect(gallery?.getAttribute("data-page-capacity")).toBe("25");
    expect(visibleCardCount("TV shows and unassociated files")).toBe(25);
    fireEvent.click(screen.getByRole("button", { name: /Next TV shows/ }));
    expect(gallery?.getAttribute("data-current-page")).toBe("2");

    resizeGallery("library", 1528, 136);
    expect(gallery?.getAttribute("data-page-capacity")).toBe("7");
    expect(gallery?.getAttribute("data-current-page")).toBe("2");
    resizeGallery("library", 1088, 284);
    expect(gallery?.getAttribute("data-page-capacity")).toBe("10");
    expect(gallery?.getAttribute("data-current-page")).toBe("2");

    fireEvent.change(
      screen.getByRole("textbox", { name: "Search titles" }),
      { target: { value: "Show 2" } },
    );
    expect(gallery?.getAttribute("data-current-page")).toBe("1");
    fireEvent.change(screen.getByRole("combobox", { name: "Sort titles" }), {
      target: { value: "descending" },
    });
    expect(
      within(screen.getByRole("list", { name: "TV shows and unassociated files" }))
        .getAllByRole("heading", { level: 3 })[0].textContent,
    ).toBe("Show 29");

    selectSettings();
    fireEvent.click(screen.getByRole("radio", { name: "Dark" }));
    selectLibrary();
    expect(screen.getByRole("radio", { name: "TV" })).toHaveProperty(
      "checked",
      true,
    );
    expect(screen.getByRole("textbox", { name: "Search titles" })).toHaveProperty(
      "value",
      "Show 2",
    );
    selectDashboard();
    const tvSummary = screen
      .getByRole("heading", { level: 2, name: "TV Library" })
      .closest("section");
    if (tvSummary === null) {
      throw new Error("The TV Dashboard summary was not rendered.");
    }
    expect(
      within(tvSummary).getByRole("heading", {
        level: 3,
        name: "31 supported TV files",
      }),
    ).toBeTruthy();
    expect(
      within(tvSummary).getByText(
        "30 shows · 30 associated episodes · 1 file remains unassociated.",
      ),
    ).toBeTruthy();
    expect(scanTvLibraryMock).toHaveBeenCalledTimes(1);
    expect(queryTvStorageMock).toHaveBeenCalledTimes(1);
  });

  it("rejects stale scan and storage responses after the configured folder changes", async () => {
    savedTvFolder = "/TV/Old";
    const oldScan = createDeferred<string[]>();
    const oldStorage = createDeferred<[string, string]>();
    scanTvLibraryMock
      .mockReturnValueOnce(oldScan.promise)
      .mockResolvedValueOnce([
        "/TV/New/New Show.S01E02.mp4",
        "New Show.S01E02.mp4",
        "2",
      ]);
    queryTvStorageMock
      .mockReturnValueOnce(oldStorage.promise)
      .mockResolvedValueOnce(["200", "50"]);
    chooseTvFolderMock.mockResolvedValue("/TV/New");

    render(<App />);
    selectSettings();
    fireEvent.click(
      await screen.findByRole("button", { name: "Change TV folder" }),
    );
    await screen.findByText("/TV/New");
    selectLibrary();
    fireEvent.click(screen.getByRole("radio", { name: "TV" }));
    expect(
      await screen.findByRole("heading", { level: 3, name: "New Show" }),
    ).toBeTruthy();

    await act(async () => {
      oldScan.resolve([
        "/TV/Old/Old Show.S01E02.mp4",
        "Old Show.S01E02.mp4",
        "1",
      ]);
      oldStorage.resolve(["100", "25"]);
      await Promise.all([oldScan.promise, oldStorage.promise]);
    });
    expect(screen.queryByText("Old Show")).toBeNull();
    selectDashboard();
    const tvSummary = screen
      .getByRole("heading", { level: 2, name: "TV Library" })
      .closest("section");
    if (tvSummary === null) {
      throw new Error("The TV Dashboard summary was not rendered.");
    }
    expect(within(tvSummary).getByText("150 B")).toBeTruthy();
  });

  it("recovers an unavailable folder without accepting a stale revalidation response", async () => {
    loadTvFolderMock.mockResolvedValueOnce(["unavailable", "/TV/Old"]);
    const staleRefresh = createDeferred<string[]>();
    loadTvFolderMock.mockReturnValueOnce(staleRefresh.promise);
    chooseTvFolderMock.mockResolvedValue("/TV/New");

    render(<App />);
    selectLibrary();
    fireEvent.click(screen.getByRole("radio", { name: "TV" }));
    await screen.findByRole("heading", { level: 2, name: "TV folder is unavailable" });
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    selectSettings();
    fireEvent.click(screen.getByRole("button", { name: "Change TV folder" }));
    await screen.findByText("/TV/New");
    await act(async () => {
      staleRefresh.resolve(["ready", "/TV/Old"]);
      await staleRefresh.promise;
    });
    expect(screen.getByText("/TV/New")).toBeTruthy();
    expect(screen.queryByText("/TV/Old")).toBeNull();
  });

  it("keeps copy, Open, Reveal, errors, and unrelated actions isolated per file", async () => {
    savedTvFolder = "/TV";
    const path = "/TV/星  Show.S01E02 — Pilot.mp4";
    scanTvLibraryMock.mockResolvedValue([
      path,
      "星  Show.S01E02 — Pilot.mp4",
      "1024",
    ]);
    openTvFileMock.mockRejectedValue("tv_file_open_stale");

    render(<App />);
    await screen.findByRole("heading", {
      level: 3,
      name: "1 supported TV file",
    });
    fireEvent.click(screen.getByRole("button", { name: "Open TV Library" }));
    expect(screen.getByRole("radio", { name: "TV" })).toHaveProperty(
      "checked",
      true,
    );
    expect(
      screen.getByRole("list", { name: "TV shows and unassociated files" }),
    ).toBeTruthy();
    const showHeading = await screen.findByRole("heading", {
      level: 3,
      name: "星 Show",
    });
    expect(showHeading.textContent).toBe("星  Show");
    fireEvent.click(screen.getByRole("button", { name: "Copy title: 星 Show" }));
    expect(clipboardWriteMock).toHaveBeenCalledWith("星  Show");
    fireEvent.click(
      screen.getByRole("button", {
        name: "Open TV file: 星 Show.S01E02 — Pilot.mp4",
      }),
    );
    expect(
      await screen.findByText("This file is no longer part of the current TV Library."),
    ).toBeTruthy();
    fireEvent.click(
      screen.getByRole("button", {
        name: "Reveal TV file: 星 Show.S01E02 — Pilot.mp4",
      }),
    );
    expect(openTvFileMock).toHaveBeenCalledWith({ path });
    expect(revealTvFileMock).toHaveBeenCalledWith({ path });
    expect(scanMoviesMock).not.toHaveBeenCalled();
    expect(fetchJavdbCatalogMock).not.toHaveBeenCalled();
  });

  it("requires exact member confirmation and keeps every dismissal non-mutating", async () => {
    savedTvFolder = "/TV";
    const path = "/TV/番組/Exact  Show.S02E03 — Finale.MKV";
    scanTvLibraryMock.mockResolvedValue([
      path,
      "番組/Exact  Show.S02E03 — Finale.MKV",
      "10",
    ]);
    const parentActivation = vi.fn();

    render(
      <div onClick={parentActivation} onPointerDown={parentActivation}>
        <App />
      </div>,
    );
    selectLibrary();
    fireEvent.click(screen.getByRole("radio", { name: "TV" }));
    const trashButton = await screen.findByRole("button", {
      name: "Move TV file to Trash or Recycle Bin: Exact Show.S02E03 — Finale.MKV",
    });
    parentActivation.mockClear();
    trashButton.focus();
    fireEvent.click(trashButton);

    let dialog = await screen.findByRole("alertdialog");
    expect(dialog.textContent).toContain(
      "Move “Exact  Show.S02E03 — Finale.MKV” to Trash?",
    );
    expect(dialog.textContent).toContain("exact member of “Exact  Show”");
    expect(dialog.textContent).toContain("Season 2, Episode 3");
    expect(dialog.textContent).not.toContain(path);
    await waitFor(() => {
      expect(document.activeElement).toBe(
        within(dialog).getByRole("button", { name: "Cancel" }),
      );
    });
    expect(trashTvFileMock).not.toHaveBeenCalled();

    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    await waitFor(() => expect(document.activeElement).toBe(trashButton));

    fireEvent.click(trashButton);
    dialog = await screen.findByRole("alertdialog");
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(screen.queryByRole("alertdialog")).toBeNull();
    await waitFor(() => expect(document.activeElement).toBe(trashButton));

    fireEvent.click(trashButton);
    dialog = await screen.findByRole("alertdialog");
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Close confirmation" }),
    );
    expect(screen.queryByRole("alertdialog")).toBeNull();
    await waitFor(() => expect(document.activeElement).toBe(trashButton));

    fireEvent.click(trashButton);
    await screen.findByRole("alertdialog");
    const backdrop = document.querySelector(".trash-dialog__backdrop");
    if (backdrop === null) {
      throw new Error("The TV Trash confirmation backdrop was not rendered.");
    }
    fireEvent.pointerDown(backdrop);
    fireEvent.click(backdrop);
    expect(screen.queryByRole("alertdialog")).toBeNull();
    await waitFor(() => expect(document.activeElement).toBe(trashButton));
    expect(trashTvFileMock).not.toHaveBeenCalled();
    expect(openTvFileMock).not.toHaveBeenCalled();
    expect(revealTvFileMock).not.toHaveBeenCalled();
    expect(clipboardWriteMock).not.toHaveBeenCalled();
    expect(parentActivation).not.toHaveBeenCalled();
    expect(scanTvLibraryMock).toHaveBeenCalledTimes(1);
    expect(queryTvStorageMock).toHaveBeenCalledTimes(1);
  });

  it("moves one grouped member once and reconciles complete TV state only", async () => {
    savedTvFolder = "/TV";
    const removedPath = "/TV/Exact Show.S01E01.mp4";
    const siblingPath = "/TV/Exact Show.S01E02.mkv";
    const unassociatedPath = "/TV/Unicode  特典.mp4";
    const initialRows = [
      removedPath,
      "Exact Show.S01E01.mp4",
      "10",
      siblingPath,
      "Exact Show.S01E02.mkv",
      "20",
      unassociatedPath,
      "Unicode  特典.mp4",
      "30",
    ];
    const remainingRows = [
      siblingPath,
      "Exact Show.S01E02.mkv",
      "20",
      unassociatedPath,
      "Unicode  特典.mp4",
      "30",
    ];
    const pendingTrash = createDeferred<void>();
    scanTvLibraryMock
      .mockResolvedValueOnce(initialRows)
      .mockResolvedValueOnce(remainingRows);
    trashTvFileMock.mockReturnValueOnce(pendingTrash.promise);
    queryTvStorageMock
      .mockResolvedValueOnce(["1000", "400"])
      .mockResolvedValueOnce(["1000", "410"]);

    render(<App />);
    selectLibrary();
    fireEvent.click(screen.getByRole("radio", { name: "TV" }));
    await screen.findByRole("heading", { level: 3, name: "Exact Show" });
    fireEvent.change(screen.getByRole("combobox", { name: "Sort titles" }), {
      target: { value: "descending" },
    });
    fireEvent.change(screen.getByRole("textbox", { name: "Search titles" }), {
      target: { value: "Exact Show" },
    });
    fireEvent.click(
      screen.getByRole("button", {
        name: "Move TV file to Trash or Recycle Bin: Exact Show.S01E01.mp4",
      }),
    );
    const dialog = await screen.findByRole("alertdialog");
    const confirmButton = within(dialog).getByRole("button", {
      name: "Confirm moving TV file to Trash or Recycle Bin: Exact Show.S01E01.mp4",
    });
    fireEvent.click(confirmButton);
    confirmButton.click();

    expect(trashTvFileMock).toHaveBeenCalledTimes(1);
    expect(trashTvFileMock).toHaveBeenCalledWith({
      path: removedPath,
      scanGeneration: "1",
    });
    expect(confirmButton).toHaveProperty("disabled", true);
    expect(dialog.getAttribute("aria-busy")).toBe("true");
    expect(within(dialog).getByRole("button", { name: "Cancel" })).toHaveProperty(
      "disabled",
      true,
    );
    expect(
      within(dialog).getByRole("button", { name: "Close confirmation" }),
    ).toHaveProperty("disabled", true);
    fireEvent.keyDown(dialog, { key: "Escape" });
    const pendingBackdrop = document.querySelector(".trash-dialog__backdrop");
    if (pendingBackdrop === null) {
      throw new Error("The pending TV Trash backdrop was not rendered.");
    }
    fireEvent.click(pendingBackdrop);
    expect(screen.getByRole("alertdialog")).toBe(dialog);
    expect(trashTvFileMock).toHaveBeenCalledTimes(1);

    await act(async () => {
      pendingTrash.resolve(undefined);
      await pendingTrash.promise;
    });
    expect(
      await screen.findByText(
        "Exact Show.S01E01.mp4 was moved to Trash or the Recycle Bin.",
      ),
    ).toBeTruthy();
    expect(screen.queryByText("Exact Show.S01E01.mp4")).toBeNull();
    expect(screen.getByText("Exact Show.S01E02.mkv")).toBeTruthy();
    expect(screen.getByRole("heading", { level: 3, name: "Exact Show" }))
      .toBeTruthy();
    expect(screen.getByRole("textbox", { name: "Search titles" })).toHaveProperty(
      "value",
      "Exact Show",
    );
    expect(screen.getByRole("combobox", { name: "Sort titles" })).toHaveProperty(
      "value",
      "descending",
    );
    fireEvent.click(screen.getByRole("button", { name: "Clear TV search" }));
    await waitFor(() => {
      expect(
        Array.from(document.querySelectorAll("[data-tv-file-path]"), (row) =>
          row.getAttribute("data-tv-file-path"),
        ),
      ).toContain(unassociatedPath);
    });
    expect(scanTvLibraryMock).toHaveBeenCalledTimes(2);
    expect(queryTvStorageMock).toHaveBeenCalledTimes(2);
    expect(scanMoviesMock).not.toHaveBeenCalled();
    expect(scanAdultLibraryMock).not.toHaveBeenCalled();
    expect(scanVrLibraryMock).not.toHaveBeenCalled();
    expect(fetchMock).not.toHaveBeenCalled();

    selectDashboard();
    const summary = screen
      .getByRole("heading", { level: 2, name: "TV Library" })
      .closest("section");
    if (summary === null) {
      throw new Error("The TV Dashboard summary was not rendered.");
    }
    expect(
      within(summary).getByRole("heading", {
        level: 3,
        name: "2 supported TV files",
      }),
    ).toBeTruthy();
    expect(
      within(summary).getByText(
        "1 show · 1 associated episode · 1 file remains unassociated.",
      ),
    ).toBeTruthy();
    expect(within(summary).getByText("590 B")).toBeTruthy();
  });

  it("keeps an accepted unassociated move truthful when reconciliation fails", async () => {
    savedTvFolder = "/TV";
    const removedPath = "/TV/Unassociated  —  remove me.MKV";
    const remainingPath = "/TV/Keep Show.S01E01.mp4";
    const initialRows = [
      removedPath,
      "Unassociated  —  remove me.MKV",
      "10",
      remainingPath,
      "Keep Show.S01E01.mp4",
      "20",
    ];
    const remainingRows = [
      remainingPath,
      "Keep Show.S01E01.mp4",
      "20",
    ];
    scanTvLibraryMock
      .mockResolvedValueOnce(initialRows)
      .mockRejectedValueOnce("tv_library_scan_failed")
      .mockResolvedValueOnce(remainingRows);
    queryTvStorageMock
      .mockResolvedValueOnce(["1000", "400"])
      .mockRejectedValueOnce("tv_storage_failed")
      .mockResolvedValueOnce(["1000", "420"]);

    render(<App />);
    selectLibrary();
    fireEvent.click(screen.getByRole("radio", { name: "TV" }));
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Move TV file to Trash or Recycle Bin: Unassociated — remove me.MKV",
      }),
    );
    const dialog = await screen.findByRole("alertdialog");
    expect(dialog.textContent).toContain(
      "exact unassociated file “Unassociated  —  remove me”",
    );
    fireEvent.click(
      within(dialog).getByRole("button", {
        name: "Confirm moving TV file to Trash or Recycle Bin: Unassociated — remove me.MKV",
      }),
    );

    expect(
      await screen.findByText(
        "Unassociated — remove me.MKV was moved to Trash or the Recycle Bin.",
      ),
    ).toBeTruthy();
    expect(screen.queryByText("Unassociated  —  remove me.MKV")).toBeNull();
    expect(screen.getByText("Keep Show.S01E01.mp4")).toBeTruthy();
    const attention = await screen.findByRole("alert");
    expect(attention.textContent).toContain("file move succeeded");
    expect(attention.textContent).toContain("remains removed");
    expect(trashTvFileMock).toHaveBeenCalledTimes(1);
    expect(scanTvLibraryMock).toHaveBeenCalledTimes(2);
    expect(queryTvStorageMock).toHaveBeenCalledTimes(2);

    fireEvent.click(
      within(attention).getByRole("button", { name: "Retry reconciliation" }),
    );
    await screen.findByRole("heading", { level: 3, name: "Keep Show" });
    await waitFor(() => {
      expect(screen.queryByText(/Library or storage could not be refreshed/))
        .toBeNull();
    });
    expect(screen.queryByText("Unassociated  —  remove me.MKV")).toBeNull();
    expect(scanTvLibraryMock).toHaveBeenCalledTimes(3);
    expect(queryTvStorageMock).toHaveBeenCalledTimes(3);
  });

  it.each([
    ["tv_file_trash_not_found", "This file is no longer available."],
    [
      "tv_file_trash_unavailable",
      "Auto-Video could not access the current TV folder or file.",
    ],
    [
      "tv_file_trash_not_file",
      "This item is not an eligible regular video file.",
    ],
    [
      "tv_file_trash_unsupported",
      "This item is not a supported .mp4 or .mkv file.",
    ],
    [
      "tv_file_trash_outside_folder",
      "This file is outside the configured TV folder.",
    ],
    [
      "tv_file_trash_stale",
      "This file is no longer part of the latest TV Library scan.",
    ],
    [
      "tv_file_trash_failed",
      "The operating system could not move this file to Trash or the Recycle Bin.",
    ],
  ])("reports %s and preserves every current TV result", async (error, message) => {
    savedTvFolder = "/TV";
    const path = "/TV/Failure Show.S01E01.mp4";
    scanTvLibraryMock.mockResolvedValue([
      path,
      "Failure Show.S01E01.mp4",
      "10",
    ]);
    trashTvFileMock.mockRejectedValueOnce(error);

    render(<App />);
    selectLibrary();
    fireEvent.click(screen.getByRole("radio", { name: "TV" }));
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Move TV file to Trash or Recycle Bin: Failure Show.S01E01.mp4",
      }),
    );
    const dialog = await screen.findByRole("alertdialog");
    fireEvent.click(
      within(dialog).getByRole("button", {
        name: "Confirm moving TV file to Trash or Recycle Bin: Failure Show.S01E01.mp4",
      }),
    );

    expect(await within(dialog).findByRole("alert")).toHaveProperty(
      "textContent",
      message,
    );
    expect(screen.getByText("Failure Show.S01E01.mp4")).toBeTruthy();
    expect(scanTvLibraryMock).toHaveBeenCalledTimes(1);
    expect(queryTvStorageMock).toHaveBeenCalledTimes(1);
    expect(trashTvFileMock).toHaveBeenCalledWith({
      path,
      scanGeneration: "1",
    });
  });

  it("removing the final grouped member clamps only an invalid TV page", async () => {
    savedTvFolder = "/TV";
    const rows = Array.from({ length: 29 }, (_, index) => {
      const title = `Show ${String(index + 1).padStart(2, "0")}`;
      return [`/TV/${title}.S01E01.mp4`, `${title}.S01E01.mp4`, "1"];
    }).flat();
    scanTvLibraryMock
      .mockResolvedValueOnce(rows)
      .mockResolvedValueOnce(rows.slice(0, -3));
    gallerySizes.library = { width: 1528, height: 136 };

    render(<App />);
    selectLibrary();
    fireEvent.click(screen.getByRole("radio", { name: "TV" }));
    await screen.findByRole("heading", { level: 3, name: "Show 01" });
    for (let page = 1; page < 5; page += 1) {
      fireEvent.click(
        screen.getByRole("button", { name: "Next TV shows and unassociated files page" }),
      );
    }
    expect(screen.getByText("Page 5 of 5")).toBeTruthy();
    fireEvent.click(
      screen.getByRole("button", {
        name: "Move TV file to Trash or Recycle Bin: Show 29.S01E01.mp4",
      }),
    );
    const dialog = await screen.findByRole("alertdialog");
    fireEvent.click(
      within(dialog).getByRole("button", {
        name: "Confirm moving TV file to Trash or Recycle Bin: Show 29.S01E01.mp4",
      }),
    );

    expect(await screen.findByText("Page 4 of 4")).toBeTruthy();
    expect(screen.queryByRole("heading", { level: 3, name: "Show 29" }))
      .toBeNull();
    expect(screen.getByRole("heading", { level: 3, name: "Show 28" }))
      .toBeTruthy();
    expect(scanTvLibraryMock).toHaveBeenCalledTimes(2);
    expect(queryTvStorageMock).toHaveBeenCalledTimes(2);
  });
});

describe("parsed Adult Library and Dashboard", () => {
  it("distinguishes an unconfigured Adult Library without scanning or querying storage", async () => {
    render(<App />);
    selectLibrary();
    fireEvent.click(screen.getByRole("radio", { name: "Adult" }));

    expect(
      await screen.findByRole("heading", {
        level: 2,
        name: "Choose an Adult folder to begin",
      }),
    ).toBeTruthy();
    expect(scanAdultLibraryMock).not.toHaveBeenCalled();
    expect(queryAdultStorageMock).not.toHaveBeenCalled();
  });

  it("renders loading, scanning, empty, error, ready, and no-match states deterministically", async () => {
    const folderLoad = createDeferred<string[]>();
    const firstScan = createDeferred<string[]>();
    loadAdultFolderMock.mockReturnValue(folderLoad.promise);
    scanAdultLibraryMock
      .mockReturnValueOnce(firstScan.promise)
      .mockRejectedValueOnce("adult_library_scan_failed")
      .mockResolvedValueOnce([
        "/Adult/ADLT-123.mp4",
        "ADLT-123.mp4",
        "1",
      ]);

    render(<App />);
    selectLibrary();
    fireEvent.click(screen.getByRole("radio", { name: "Adult" }));
    expect(
      screen.getByRole("heading", { level: 2, name: "Loading Adult folder" }),
    ).toBeTruthy();
    await act(async () => {
      folderLoad.resolve(["ready", "/Adult"]);
      await folderLoad.promise;
    });
    expect(
      screen.getByRole("heading", { level: 2, name: "Scanning Adult folder" }),
    ).toBeTruthy();
    await act(async () => {
      firstScan.resolve([]);
      await firstScan.promise;
    });
    expect(
      await screen.findByRole("heading", {
        level: 2,
        name: "No supported Adult videos found",
      }),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(
      await screen.findByRole("heading", {
        level: 2,
        name: "Adult folder could not be scanned",
      }),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await screen.findByRole("heading", { level: 3, name: "ADLT-123" });
    fireEvent.change(screen.getByRole("textbox", { name: "Search titles" }), {
      target: { value: "missing title" },
    });
    expect(
      screen.getByRole("heading", {
        level: 2,
        name: "No Adult titles match this search",
      }),
    ).toBeTruthy();
  });

  it("chooses, changes, and clears only the native Adult folder configuration", async () => {
    chooseAdultFolderMock
      .mockResolvedValueOnce("/Adult/First")
      .mockResolvedValueOnce("/Adult/Second");

    render(<App />);
    selectSettings();
    fireEvent.click(
      await screen.findByRole("button", { name: "Choose Adult folder" }),
    );
    expect(await screen.findByText("/Adult/First")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Change Adult folder" }));
    expect(await screen.findByText("/Adult/Second")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Clear Adult folder" }));
    expect(await screen.findByText("No Adult folder configured.")).toBeTruthy();
    expect(chooseAdultFolderMock).toHaveBeenCalledTimes(2);
    expect(clearAdultFolderMock).toHaveBeenCalledTimes(1);
    expect(openFolderMock).not.toHaveBeenCalled();
    expect(chooseTvFolderMock).not.toHaveBeenCalled();
    expect(chooseVrFolderMock).not.toHaveBeenCalled();
  });

  it("shows exact grouped membership, multipart labels, complete totals, storage, and routing", async () => {
    savedAdultFolder = "/Adult/作品  Library";
    scanAdultLibraryMock.mockResolvedValue([
      "/Adult/作品  Library/ADLT-123 Part 01 — 前編.mp4",
      "ADLT-123 Part 01 — 前編.mp4",
      "1073741824",
      "/Adult/作品  Library/adlt_00123_CD2  特別版.MKV",
      "adlt_00123_CD2  特別版.MKV",
      "2147483648",
      "/Adult/作品  Library/ADLT-123 Part 01 Disc 02.mp4",
      "ADLT-123 Part 01 Disc 02.mp4",
      "3221225472",
      "/Adult/作品  Library/ADLT-123 Part 1-2.mp4",
      "ADLT-123 Part 1-2.mp4",
      "6",
      "/Adult/作品  Library/ADLT-123 CD1+2.mkv",
      "ADLT-123 CD1+2.mkv",
      "7",
      "/Adult/作品  Library/ADLT-123 + XYZ-7  pack.mp4",
      "ADLT-123 + XYZ-7  pack.mp4",
      "4",
      "/Adult/作品  Library/ADLT-123 + PT-7.mp4",
      "ADLT-123 + PT-7.mp4",
      "8",
      "/Adult/作品  Library/作品  without code.mkv",
      "作品  without code.mkv",
      "5",
    ]);

    render(<App />);

    expect(
      await screen.findByRole("heading", {
        level: 3,
        name: "1 grouped title · 8 supported files",
      }),
    ).toBeTruthy();
    expect(screen.getByText("3 files remain unassociated.")).toBeTruthy();
    expect(screen.getByText("4.0 TiB", { selector: "dd" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Open Adult Library" }));
    expect(screen.getByRole("radio", { name: "Adult" })).toHaveProperty(
      "checked",
      true,
    );
    const gallery = screen.getByRole("list", {
      name: "Adult titles and unassociated files",
    });
    const groupedHeading = within(gallery).getByRole("heading", {
      level: 3,
      name: "ADLT-123",
    });
    const groupedCard = groupedHeading.closest("article");
    if (groupedCard === null) {
      throw new Error("The grouped Adult card was not rendered.");
    }
    expect(groupedCard.querySelectorAll("[data-adult-file-path]")).toHaveLength(5);
    expect(within(groupedCard).getByText("Part 01 · 1.0 GiB")).toBeTruthy();
    expect(within(groupedCard).getByText("CD2 · 2.0 GiB")).toBeTruthy();
    const ambiguousRow = groupedCard.querySelector(
      '[data-adult-file-path="/Adult/作品  Library/ADLT-123 Part 01 Disc 02.mp4"]',
    );
    expect(ambiguousRow?.textContent).toContain("3.0 GiB");
    expect(ambiguousRow?.textContent).not.toContain("Part 01 ·");
    const compactPartRow = groupedCard.querySelector(
      '[data-adult-file-path="/Adult/作品  Library/ADLT-123 Part 1-2.mp4"]',
    );
    expect(compactPartRow?.textContent).toContain("ADLT-123 Part 1-2.mp4");
    expect(compactPartRow?.textContent).not.toContain("Part 1 ·");
    const compactCdRow = groupedCard.querySelector(
      '[data-adult-file-path="/Adult/作品  Library/ADLT-123 CD1+2.mkv"]',
    );
    expect(compactCdRow?.textContent).toContain("ADLT-123 CD1+2.mkv");
    expect(compactCdRow?.textContent).not.toContain("CD1 ·");
    expect(
      groupedCard.querySelector(
        '[data-adult-file-path="/Adult/作品  Library/ADLT-123 + PT-7.mp4"]',
      ),
    ).toBeNull();
    const vrOnlyPartHeading = within(gallery).getByRole("heading", {
      level: 3,
      name: "ADLT-123 + PT-7",
    });
    expect(vrOnlyPartHeading.textContent).toBe("ADLT-123 + PT-7");
    const unassociatedHeading = within(gallery).getByRole("heading", {
      level: 3,
      name: "作品 without code",
    });
    expect(unassociatedHeading.textContent).toBe("作品  without code");
  });

  it("searches then sorts then paginates across 25, 7, and 10 without changing Dashboard totals", async () => {
    savedAdultFolder = "/Adult";
    scanAdultLibraryMock.mockResolvedValue([
      ...Array.from({ length: 30 }, (_, index) => {
        const code = `ADLT-${String(index + 101)}`;
        const filename = `${code}.mp4`;
        return [`/Adult/${filename}`, filename, "1"];
      }).flat(),
      "/Adult/Unassociated 作品.mp4",
      "Unassociated 作品.mp4",
      "1",
    ]);
    gallerySizes.library = { width: 1088, height: 728 };

    render(<App />);
    selectLibrary();
    fireEvent.click(screen.getByRole("radio", { name: "Adult" }));
    await screen.findByRole("heading", { level: 3, name: "ADLT-101" });
    let gallery = document.querySelector('[data-gallery="library"]');
    expect(gallery?.getAttribute("data-page-capacity")).toBe("25");
    expect(visibleCardCount("Adult titles and unassociated files")).toBe(25);
    fireEvent.click(screen.getByRole("button", { name: /Next Adult titles/ }));
    expect(gallery?.getAttribute("data-current-page")).toBe("2");

    resizeGallery("library", 1528, 136);
    expect(gallery?.getAttribute("data-page-capacity")).toBe("7");
    expect(gallery?.getAttribute("data-current-page")).toBe("2");
    resizeGallery("library", 1088, 284);
    expect(gallery?.getAttribute("data-page-capacity")).toBe("10");
    expect(gallery?.getAttribute("data-current-page")).toBe("2");

    fireEvent.change(screen.getByRole("textbox", { name: "Search titles" }), {
      target: { value: "/Adult/" },
    });
    expect(
      screen.getByRole("heading", {
        level: 2,
        name: "No Adult titles match this search",
      }),
    ).toBeTruthy();
    fireEvent.change(screen.getByRole("textbox", { name: "Search titles" }), {
      target: { value: "ADLT-1" },
    });
    gallery = document.querySelector('[data-gallery="library"]');
    expect(gallery?.getAttribute("data-current-page")).toBe("1");
    fireEvent.change(screen.getByRole("combobox", { name: "Sort titles" }), {
      target: { value: "descending" },
    });
    expect(
      within(
        screen.getByRole("list", {
          name: "Adult titles and unassociated files",
        }),
      ).getAllByRole("heading", { level: 3 })[0].textContent,
    ).toBe("ADLT-130");
    fireEvent.click(screen.getByRole("button", { name: /Next Adult titles/ }));
    expect(gallery?.getAttribute("data-current-page")).toBe("2");

    selectSettings();
    fireEvent.click(screen.getByRole("radio", { name: "Dark" }));
    selectLibrary();
    gallery = document.querySelector('[data-gallery="library"]');
    expect(screen.getByRole("radio", { name: "Adult" })).toHaveProperty(
      "checked",
      true,
    );
    expect(screen.getByRole("textbox", { name: "Search titles" })).toHaveProperty(
      "value",
      "ADLT-1",
    );
    expect(gallery?.getAttribute("data-current-page")).toBe("2");
    selectDashboard();
    const adultSummary = screen
      .getByRole("heading", { level: 2, name: "Adult Library" })
      .closest("section");
    if (adultSummary === null) {
      throw new Error("The Adult Dashboard summary was not rendered.");
    }
    expect(
      within(adultSummary).getByRole("heading", {
        level: 3,
        name: "30 grouped titles · 31 supported files",
      }),
    ).toBeTruthy();
    expect(within(adultSummary).getByText("1 file remains unassociated.")).toBeTruthy();
    expect(scanAdultLibraryMock).toHaveBeenCalledTimes(1);
    expect(queryAdultStorageMock).toHaveBeenCalledTimes(1);
    expect(fetchJavdbCatalogMock).not.toHaveBeenCalled();
    expect(inspectSukebeiVrTorrentMock).not.toHaveBeenCalled();
    expect(startVerifiedVrDownloadMock).not.toHaveBeenCalled();
  });

  it("rejects stale scan and storage responses after the configured folder changes", async () => {
    savedAdultFolder = "/Adult/Old";
    const staleRefreshScan = createDeferred<string[]>();
    const staleRefreshStorage = createDeferred<[string, string]>();
    scanAdultLibraryMock
      .mockResolvedValueOnce([
        "/Adult/Old/ADLT-100.mp4",
        "ADLT-100.mp4",
        "1",
      ])
      .mockReturnValueOnce(staleRefreshScan.promise)
      .mockResolvedValueOnce([
        "/Adult/New/ADLT-200.mp4",
        "ADLT-200.mp4",
        "2",
      ]);
    queryAdultStorageMock
      .mockResolvedValueOnce(["100", "25"])
      .mockReturnValueOnce(staleRefreshStorage.promise)
      .mockResolvedValueOnce(["200", "50"]);
    chooseAdultFolderMock.mockResolvedValue("/Adult/New");

    render(<App />);
    selectLibrary();
    fireEvent.click(screen.getByRole("radio", { name: "Adult" }));
    await screen.findByRole("heading", { level: 3, name: "ADLT-100" });
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(
      screen.getByRole("heading", { level: 2, name: "Scanning Adult folder" }),
    ).toBeTruthy();
    selectSettings();
    fireEvent.click(
      await screen.findByRole("button", { name: "Change Adult folder" }),
    );
    await screen.findByText("/Adult/New");
    selectLibrary();
    fireEvent.click(screen.getByRole("radio", { name: "Adult" }));
    expect(
      await screen.findByRole("heading", { level: 3, name: "ADLT-200" }),
    ).toBeTruthy();

    await act(async () => {
      staleRefreshScan.resolve([
        "/Adult/Old/ADLT-100.mp4",
        "ADLT-100.mp4",
        "1",
      ]);
      staleRefreshStorage.resolve(["1000", "999"]);
      await Promise.all([
        staleRefreshScan.promise,
        staleRefreshStorage.promise,
      ]);
    });
    expect(screen.queryByText("ADLT-100")).toBeNull();
    selectDashboard();
    const adultSummary = screen
      .getByRole("heading", { level: 2, name: "Adult Library" })
      .closest("section");
    if (adultSummary === null) {
      throw new Error("The Adult Dashboard summary was not rendered.");
    }
    expect(within(adultSummary).getByText("150 B")).toBeTruthy();
    expect(scanAdultLibraryMock).toHaveBeenCalledTimes(3);
    expect(queryAdultStorageMock).toHaveBeenCalledTimes(3);
  });

  it("recovers an unavailable folder without accepting a stale revalidation response", async () => {
    loadAdultFolderMock.mockResolvedValueOnce(["unavailable", "/Adult/Old"]);
    const staleRefresh = createDeferred<string[]>();
    loadAdultFolderMock.mockReturnValueOnce(staleRefresh.promise);
    chooseAdultFolderMock.mockResolvedValue("/Adult/New");

    render(<App />);
    selectSettings();
    await screen.findByText(
      "This folder has moved or is unavailable. Restore it, choose another folder, or clear the configuration.",
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Refresh Adult folder" }),
    );
    expect(
      screen.getByRole("button", { name: "Refreshing Adult folder" }),
    ).toHaveProperty("disabled", true);
    fireEvent.click(screen.getByRole("button", { name: "Change Adult folder" }));
    await screen.findByText("/Adult/New");
    await act(async () => {
      staleRefresh.resolve(["ready", "/Adult/Old"]);
      await staleRefresh.promise;
    });
    expect(screen.getByText("/Adult/New")).toBeTruthy();
    expect(screen.queryByText("/Adult/Old")).toBeNull();
  });

  it("keeps exact copy, Open, Reveal, errors, and unrelated actions isolated per file", async () => {
    savedAdultFolder = "/Adult";
    const groupedPath = "/Adult/ADLT-123 Part 01.mp4";
    const unassociatedPath = "/Adult/作品  Special — Edition.mkv";
    scanAdultLibraryMock.mockResolvedValue([
      groupedPath,
      "ADLT-123 Part 01.mp4",
      "1024",
      unassociatedPath,
      "作品  Special — Edition.mkv",
      "2048",
    ]);
    openAdultFileMock.mockRejectedValue("adult_file_open_stale");
    clipboardWriteMock
      .mockResolvedValueOnce(undefined)
      .mockRejectedValueOnce(new Error("denied"));

    render(<App />);
    await screen.findByRole("heading", {
      level: 3,
      name: "1 grouped title · 2 supported files",
    });
    fireEvent.click(screen.getByRole("button", { name: "Open Adult Library" }));
    fireEvent.click(screen.getByRole("button", { name: "Copy title: ADLT-123" }));
    expect(clipboardWriteMock).toHaveBeenCalledWith("ADLT-123");
    const unassociatedHeading = screen.getByRole("heading", {
      level: 3,
      name: "作品 Special — Edition",
    });
    expect(unassociatedHeading.textContent).toBe("作品  Special — Edition");
    fireEvent.click(
      screen.getByRole("button", {
        name: "Copy title: 作品 Special — Edition",
      }),
    );
    expect(clipboardWriteMock).toHaveBeenCalledWith("作品  Special — Edition");
    expect(
      await screen.findByRole("button", {
        name: "Copy failed for title: 作品 Special — Edition",
      }),
    ).toBeTruthy();
    fireEvent.click(
      screen.getByRole("button", {
        name: "Open Adult file: ADLT-123 Part 01.mp4",
      }),
    );
    expect(
      await screen.findByText(
        "This file is no longer part of the current Adult Library.",
      ),
    ).toBeTruthy();
    fireEvent.click(
      screen.getByRole("button", {
        name: "Reveal Adult file: 作品 Special — Edition.mkv",
      }),
    );
    expect(openAdultFileMock).toHaveBeenCalledWith({ path: groupedPath });
    expect(revealAdultFileMock).toHaveBeenCalledWith({ path: unassociatedPath });
    expect(scanAdultLibraryMock).toHaveBeenCalledTimes(1);
    expect(scanMoviesMock).not.toHaveBeenCalled();
    expect(scanTvLibraryMock).not.toHaveBeenCalled();
    expect(scanVrLibraryMock).not.toHaveBeenCalled();
    expect(fetchJavdbCatalogMock).not.toHaveBeenCalled();
  });

  it("keeps a complete Adult scan visible when its storage query fails", async () => {
    savedAdultFolder = "/Adult";
    scanAdultLibraryMock.mockResolvedValue([
      "/Adult/ADLT-123.mp4",
      "ADLT-123.mp4",
      "1",
    ]);
    queryAdultStorageMock.mockRejectedValue("adult_storage_failed");

    render(<App />);
    const adultSummary = await screen.findByRole("region", {
      name: "Adult Library",
    });
    expect(
      await within(adultSummary).findByRole("heading", {
        level: 3,
        name: "1 grouped title · 1 supported file",
      }),
    ).toBeTruthy();
    expect(
      within(adultSummary).getByRole("heading", {
        level: 3,
        name: "Storage could not be loaded",
      }),
    ).toBeTruthy();
  });

  it("keeps Adult storage visible when its scan fails", async () => {
    savedAdultFolder = "/Adult";
    scanAdultLibraryMock.mockRejectedValue("adult_library_scan_failed");

    render(<App />);
    const adultSummary = await screen.findByRole("region", {
      name: "Adult Library",
    });
    expect(
      await within(adultSummary).findByRole("heading", {
        level: 3,
        name: "Adult Library scan failed",
      }),
    ).toBeTruthy();
    expect(
      await within(adultSummary).findByText("4.0 TiB", { selector: "dd" }),
    ).toBeTruthy();
  });
});

describe("parsed VR Library and Dashboard", () => {
  it("distinguishes VR folder loading from an unconfigured Library", async () => {
    const pendingFolder = createDeferred<string[]>();
    loadVrFolderMock.mockReturnValue(pendingFolder.promise);

    render(<App />);
    selectLibrary();
    fireEvent.click(screen.getByRole("radio", { name: "VR" }));
    expect(
      screen.getByRole("heading", { level: 2, name: "Loading VR folder" }),
    ).toBeTruthy();
    await act(async () => {
      pendingFolder.resolve(["unconfigured"]);
      await pendingFolder.promise;
    });
    expect(
      await screen.findByRole("heading", {
        level: 2,
        name: "Choose a VR folder to begin",
      }),
    ).toBeTruthy();
    expect(scanVrLibraryMock).not.toHaveBeenCalled();
    expect(queryVrStorageMock).not.toHaveBeenCalled();
  });

  it.each([
    {
      folderResponse: ["ready", "/VR"],
      heading: "No supported VR videos found",
    },
    {
      folderResponse: ["unavailable", "/missing/VR"],
      heading: "VR folder is unavailable",
    },
  ])("shows the truthful $heading state", async ({ folderResponse, heading }) => {
    loadVrFolderMock.mockResolvedValue(folderResponse);

    render(<App />);
    selectLibrary();
    fireEvent.click(screen.getByRole("radio", { name: "VR" }));
    expect(
      await screen.findByRole("heading", { level: 2, name: heading }),
    ).toBeTruthy();
    expect(scanVrLibraryMock).toHaveBeenCalledTimes(
      folderResponse[0] === "ready" ? 1 : 0,
    );
  });

  it("revalidates a restored persisted VR folder before refreshing its current scan and storage", async () => {
    const folder = "/missing/VR — 作品";
    const restoredFolder = createDeferred<string[]>();
    loadVrFolderMock
      .mockResolvedValueOnce(["unavailable", folder])
      .mockReturnValueOnce(restoredFolder.promise);
    scanVrLibraryMock.mockResolvedValue([`${folder}/MDVR-419.mp4`, "7"]);
    queryVrStorageMock.mockResolvedValue(["4096", "1024"]);

    render(<App />);
    selectLibrary();
    fireEvent.click(screen.getByRole("radio", { name: "VR" }));
    expect(
      await screen.findByRole("heading", {
        level: 2,
        name: "VR folder is unavailable",
      }),
    ).toBeTruthy();
    expect(scanVrLibraryMock).not.toHaveBeenCalled();
    expect(queryVrStorageMock).not.toHaveBeenCalled();

    const refresh = screen.getByRole("button", { name: "Refresh" });
    expect(refresh).toHaveProperty("disabled", false);
    fireEvent.click(refresh);
    expect(loadVrFolderMock).toHaveBeenCalledTimes(2);
    expect(screen.getByRole("button", { name: "Refreshing…" })).toHaveProperty(
      "disabled",
      true,
    );
    expect(scanVrLibraryMock).not.toHaveBeenCalled();
    expect(queryVrStorageMock).not.toHaveBeenCalled();

    await act(async () => {
      restoredFolder.resolve(["ready", folder]);
      await restoredFolder.promise;
    });
    expect(await screen.findByText("MDVR-419")).toBeTruthy();
    await waitFor(() => {
      expect(scanVrLibraryMock).toHaveBeenCalledTimes(1);
      expect(queryVrStorageMock).toHaveBeenCalledTimes(1);
    });

    selectDashboard();
    const summary = screen.getByRole("region", { name: "VR Library" });
    expect(within(summary).getByText(folder)).toBeTruthy();
    expect(within(summary).getByText("4.0 KiB")).toBeTruthy();
    expect(within(summary).getByText("3.0 KiB")).toBeTruthy();
    expect(within(summary).getByText("1.0 KiB")).toBeTruthy();
  });

  it("keeps a persisted VR folder unavailable when Refresh cannot restore it", async () => {
    const folder = "/missing/VR";
    loadVrFolderMock.mockResolvedValue(["unavailable", folder]);

    render(<App />);
    selectLibrary();
    fireEvent.click(screen.getByRole("radio", { name: "VR" }));
    expect(
      await screen.findByRole("heading", {
        level: 2,
        name: "VR folder is unavailable",
      }),
    ).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await waitFor(() => expect(loadVrFolderMock).toHaveBeenCalledTimes(2));
    expect(
      screen.getByRole("heading", {
        level: 2,
        name: "VR folder is unavailable",
      }),
    ).toBeTruthy();
    expect(screen.getByRole("button", { name: "Refresh" })).toHaveProperty(
      "disabled",
      false,
    );
    expect(scanVrLibraryMock).not.toHaveBeenCalled();
    expect(queryVrStorageMock).not.toHaveBeenCalled();
  });

  it("ignores an unavailable-folder revalidation after the folder is replaced", async () => {
    const staleRevalidation = createDeferred<string[]>();
    loadVrFolderMock
      .mockResolvedValueOnce(["unavailable", "/VR/A"])
      .mockReturnValueOnce(staleRevalidation.promise);
    chooseVrFolderMock.mockResolvedValue("/VR/B");
    scanVrLibraryMock.mockResolvedValue(["/VR/B/MDVR-422.mp4", "2"]);
    queryVrStorageMock.mockResolvedValue(["4096", "1024"]);

    render(<App />);
    selectLibrary();
    fireEvent.click(screen.getByRole("radio", { name: "VR" }));
    expect(
      await screen.findByRole("heading", {
        level: 2,
        name: "VR folder is unavailable",
      }),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(loadVrFolderMock).toHaveBeenCalledTimes(2);

    selectSettings();
    fireEvent.click(screen.getByRole("button", { name: "Change VR folder" }));
    expect(await screen.findByText("/VR/B")).toBeTruthy();
    await waitFor(() => {
      expect(scanVrLibraryMock).toHaveBeenCalledTimes(1);
      expect(queryVrStorageMock).toHaveBeenCalledTimes(1);
    });

    await act(async () => {
      staleRevalidation.resolve(["ready", "/VR/A"]);
      await staleRevalidation.promise;
    });
    selectLibrary();
    fireEvent.click(screen.getByRole("radio", { name: "VR" }));
    expect(await screen.findByText("MDVR-422")).toBeTruthy();
    expect(screen.queryByText("/VR/A")).toBeNull();
    expect(scanVrLibraryMock).toHaveBeenCalledTimes(1);
    expect(queryVrStorageMock).toHaveBeenCalledTimes(1);
  });

  it("shows scanning until the complete VR result becomes ready", async () => {
    const pendingScan = createDeferred<string[]>();
    savedVrFolder = "/VR";
    scanVrLibraryMock.mockReturnValue(pendingScan.promise);

    render(<App />);
    selectLibrary();
    fireEvent.click(screen.getByRole("radio", { name: "VR" }));
    expect(
      await screen.findByRole("heading", { level: 2, name: "Scanning VR folder" }),
    ).toBeTruthy();
    await act(async () => {
      pendingScan.resolve(["/VR/MDVR-419.mp4", "1"]);
      await pendingScan.promise;
    });
    expect(await screen.findByText("MDVR-419")).toBeTruthy();
  });

  it("shows grouped counts while preserving exact member copy, open, and reveal actions", async () => {
    const firstPath = "/VR/作品/MDVR-419  Disc 01 — 前編.mp4";
    const secondPath = "/VR/mdvr_00419_CD2  特別版.MKV";
    const unassociatedPath = "/VR/MDVR-419 + ABC-123  pack.mp4";
    savedVrFolder = "/VR";
    scanVrLibraryMock.mockResolvedValue([
      firstPath,
      "1073741824",
      secondPath,
      "2147483648",
      unassociatedPath,
      "3",
    ]);

    render(<App />);

    expect(
      await screen.findByRole("heading", {
        level: 3,
        name: "2 VR titles · 3 files",
      }),
    ).toBeTruthy();
    const vrSummary = screen.getByRole("region", { name: "VR Library" });
    expect(within(vrSummary).getByText("2.0 TiB")).toBeTruthy();
    expect(within(vrSummary).getByText("1.5 TiB")).toBeTruthy();
    expect(within(vrSummary).getByText("512.0 GiB")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Open VR Library" }));
    expect(
      (screen.getByRole("radio", { name: "VR" }) as HTMLInputElement).checked,
    ).toBe(true);
    expect(await screen.findByRole("heading", { level: 3, name: "MDVR-419" })).toBeTruthy();
    const renderedFileRows = Array.from(
      document.querySelectorAll<HTMLElement>("[data-vr-file-path]"),
    );
    expect(renderedFileRows.map((row) => row.dataset.vrFilePath)).toEqual([
      firstPath,
      secondPath,
      unassociatedPath,
    ]);

    fireEvent.click(screen.getByRole("button", { name: "Copy title: MDVR-419" }));
    await waitFor(() => expect(clipboardWriteMock).toHaveBeenCalledWith("MDVR-419"));
    const unassociatedTitle = "MDVR-419 + ABC-123  pack";
    const unassociatedCopy = Array.from(
      document.querySelectorAll<HTMLButtonElement>("button[aria-label]"),
    ).find(
      (button) =>
        button.getAttribute("aria-label") === `Copy title: ${unassociatedTitle}`,
    );
    expect(unassociatedCopy).toBeDefined();
    fireEvent.click(unassociatedCopy as HTMLButtonElement);
    await waitFor(() =>
      expect(clipboardWriteMock).toHaveBeenCalledWith(unassociatedTitle),
    );
    const firstOpen = within(renderedFileRows[0]).getByTitle("Open file");
    const secondReveal = within(renderedFileRows[1]).getByTitle("Reveal file");
    expect(firstOpen.getAttribute("aria-label")).toBe(`Open VR file: ${firstPath}`);
    expect(secondReveal.getAttribute("aria-label")).toBe(
      `Reveal VR file: ${secondPath}`,
    );
    fireEvent.click(firstOpen);
    fireEvent.click(secondReveal);
    await waitFor(() => {
      expect(openVrFileMock).toHaveBeenCalledWith({ path: firstPath });
      expect(revealVrFileMock).toHaveBeenCalledWith({ path: secondPath });
    });
  });

  it("searches before sorting and pagination without rescanning across resize or navigation", async () => {
    savedVrFolder = "/VR";
    scanVrLibraryMock.mockResolvedValue(
      Array.from({ length: 25 }, (_, index) => {
        const code = `MDVR-${String(index + 101)}`;
        return [`/VR/${code}.mp4`, "1"];
      }).flat(),
    );

    render(<App />);
    selectLibrary();
    fireEvent.click(screen.getByRole("radio", { name: "VR" }));
    await screen.findByText("MDVR-101");

    resizeGallery("library", 1088, 728);
    expect(visibleCardCount("VR titles")).toBe(25);
    resizeGallery("library", 1528, 136);
    expect(visibleCardCount("VR titles")).toBe(7);
    expect(screen.getByText("Page 1 of 4")).toBeTruthy();
    fireEvent.click(
      screen.getByRole("button", { name: "Next VR titles page" }),
    );
    expect(screen.getByText("Page 2 of 4")).toBeTruthy();
    resizeGallery("library", 1088, 284);
    expect(visibleCardCount("VR titles")).toBe(10);
    expect(screen.getByText("Page 2 of 3")).toBeTruthy();
    expect(screen.getByText("MDVR-111")).toBeTruthy();

    fireEvent.change(screen.getByRole("textbox", { name: "Search titles" }), {
      target: { value: "MDVR-125" },
    });
    expect(await screen.findByText("MDVR-125")).toBeTruthy();
    expect(visibleCardCount("VR titles")).toBe(1);
    fireEvent.click(screen.getByRole("button", { name: "Clear VR search" }));
    fireEvent.change(screen.getByRole("combobox", { name: "Sort titles" }), {
      target: { value: "descending" },
    });
    const sortedTitles = within(screen.getByRole("list", { name: "VR titles" }))
      .getAllByRole("heading", { level: 3 })
      .map((heading) => heading.textContent);
    expect(sortedTitles.slice(0, 2)).toEqual(["MDVR-125", "MDVR-124"]);
    fireEvent.change(screen.getByRole("textbox", { name: "Search titles" }), {
      target: { value: "MDVR-125" },
    });

    selectDashboard();
    selectLibrary();
    expect(
      (screen.getByRole("radio", { name: "VR" }) as HTMLInputElement).checked,
    ).toBe(true);
    expect(
      (screen.getByRole("textbox", { name: "Search titles" }) as HTMLInputElement)
        .value,
    ).toBe("MDVR-125");
    selectSettings();
    fireEvent.click(screen.getByRole("radio", { name: "Dark" }));
    selectLibrary();
    expect(
      (screen.getByRole("radio", { name: "VR" }) as HTMLInputElement).checked,
    ).toBe(true);
    expect(
      (screen.getByRole("combobox", { name: "Sort titles" }) as HTMLSelectElement)
        .value,
    ).toBe("descending");
    resizeGallery("library", 720, 520);
    expect(screen.getByText("MDVR-125")).toBeTruthy();
    expect(scanVrLibraryMock).toHaveBeenCalledTimes(1);
    expect(queryVrStorageMock).toHaveBeenCalledTimes(1);
    expect(fetchJavdbCatalogMock).not.toHaveBeenCalled();
    expect(fetchSukebeiVrReleasesMock).not.toHaveBeenCalled();
    expect(fetchMock).not.toHaveBeenCalled();
    expect(pauseVrDownloadMock).not.toHaveBeenCalled();
    expect(resumeVrDownloadMock).not.toHaveBeenCalled();
  });

  it("isolates file action errors and keeps storage ready when a refresh scan fails", async () => {
    const firstPath = "/VR/MDVR-419.mp4";
    const secondPath = "/VR/MDVR-422.mkv";
    savedVrFolder = "/VR";
    scanVrLibraryMock.mockResolvedValue([
      firstPath,
      "1",
      secondPath,
      "2",
    ]);
    openVrFileMock.mockRejectedValueOnce("vr_file_open_stale");

    render(<App />);
    selectLibrary();
    fireEvent.click(screen.getByRole("radio", { name: "VR" }));
    await screen.findByText("MDVR-419");
    fireEvent.click(screen.getByRole("button", { name: `Open VR file: ${firstPath}` }));
    expect(
      await screen.findByText("This file is no longer part of the current VR Library."),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: `Reveal VR file: ${secondPath}` }));
    await waitFor(() =>
      expect(revealVrFileMock).toHaveBeenCalledWith({ path: secondPath }),
    );
    expect(screen.getByText("MDVR-422")).toBeTruthy();

    scanVrLibraryMock.mockRejectedValueOnce("vr_library_scan_failed");
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    selectDashboard();
    expect(
      await screen.findByRole("heading", { level: 3, name: "VR Library scan failed" }),
    ).toBeTruthy();
    expect(
      within(screen.getByRole("region", { name: "VR Library" })).getByText(
        "2.0 TiB",
      ),
    ).toBeTruthy();
  });

  it("ignores late scan and storage results from a replaced VR folder", async () => {
    const oldScan = createDeferred<string[]>();
    const oldStorage = createDeferred<[string, string]>();
    savedVrFolder = "/VR/A";
    chooseVrFolderMock.mockResolvedValue("/VR/B");
    scanVrLibraryMock
      .mockReturnValueOnce(oldScan.promise)
      .mockResolvedValueOnce(["/VR/B/MDVR-422.mp4", "2"]);
    queryVrStorageMock
      .mockReturnValueOnce(oldStorage.promise)
      .mockResolvedValueOnce(["4096", "1024"]);

    render(<App />);
    await waitFor(() => {
      expect(scanVrLibraryMock).toHaveBeenCalledTimes(1);
      expect(queryVrStorageMock).toHaveBeenCalledTimes(1);
    });
    selectSettings();
    fireEvent.click(screen.getByRole("button", { name: "Change VR folder" }));
    expect(await screen.findByText("/VR/B")).toBeTruthy();
    await waitFor(() => {
      expect(scanVrLibraryMock).toHaveBeenCalledTimes(2);
      expect(queryVrStorageMock).toHaveBeenCalledTimes(2);
    });

    selectLibrary();
    fireEvent.click(screen.getByRole("radio", { name: "VR" }));
    expect(await screen.findByText("MDVR-422")).toBeTruthy();
    await act(async () => {
      oldScan.resolve(["/VR/A/MDVR-419.mp4", "1"]);
      oldStorage.resolve(["8192", "4096"]);
      await Promise.all([oldScan.promise, oldStorage.promise]);
    });
    expect(screen.queryByText("MDVR-419")).toBeNull();

    selectDashboard();
    const summary = screen.getByRole("region", { name: "VR Library" });
    expect(within(summary).getByText("4.0 KiB")).toBeTruthy();
    expect(within(summary).getByText("3.0 KiB")).toBeTruthy();
    expect(within(summary).getByText("1.0 KiB")).toBeTruthy();
  });

  it("refreshes scan and storage once when a transfer first completes", async () => {
    vi.useFakeTimers();
    savedVrFolder = "/VR";
    loadVrDownloadsMock.mockResolvedValue(
      [
        ...vrDownloadFixture({
          releaseName: "MDVR-419 release",
          state: "downloading",
          transferId: "transfer-419",
        }),
        ...vrDownloadFixture({
          isCurrentFolder: "false",
          releaseName: "MDVR-422 old-folder release",
          state: "downloading",
          transferId: "transfer-422",
        }),
      ],
    );
    listVrDownloadsMock
      .mockResolvedValueOnce([
        ...vrDownloadFixture({
          releaseName: "MDVR-419 release",
          state: "downloading",
          transferId: "transfer-419",
        }),
        ...vrDownloadFixture({
          downloadedBytes: "10",
          isCurrentFolder: "false",
          releaseName: "MDVR-422 old-folder release",
          speedBytesPerSecond: "0",
          state: "completed",
          transferId: "transfer-422",
        }),
      ])
      .mockResolvedValueOnce([
        ...vrDownloadFixture({
          downloadedBytes: "10",
          releaseName: "MDVR-419 release",
          speedBytesPerSecond: "0",
          state: "completed",
          transferId: "transfer-419",
        }),
        ...vrDownloadFixture({
          downloadedBytes: "10",
          isCurrentFolder: "false",
          releaseName: "MDVR-422 old-folder release",
          speedBytesPerSecond: "0",
          state: "completed",
          transferId: "transfer-422",
        }),
      ]);

    render(<App />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(scanVrLibraryMock).toHaveBeenCalledTimes(1);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(scanVrLibraryMock).toHaveBeenCalledTimes(1);
    expect(queryVrStorageMock).toHaveBeenCalledTimes(1);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(scanVrLibraryMock).toHaveBeenCalledTimes(2);
    expect(queryVrStorageMock).toHaveBeenCalledTimes(2);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(scanVrLibraryMock).toHaveBeenCalledTimes(2);
    expect(queryVrStorageMock).toHaveBeenCalledTimes(2);
  });

  it("refreshes only Adult Library and storage for a current-folder Adult completion", async () => {
    vi.useFakeTimers();
    savedAdultFolder = "/Adult";
    savedVrFolder = "/VR";
    const activeRows = [
      ...vrDownloadFixture({
        category: "adult",
        code: "ADLT-123",
        releaseName: "ADLT-123 current",
        state: "downloading",
        transferId: "adult-current",
      }),
      ...vrDownloadFixture({
        category: "adult",
        code: "ADLT-124",
        isCurrentFolder: "false",
        releaseName: "ADLT-124 old folder",
        state: "downloading",
        transferId: "adult-old",
      }),
      ...vrDownloadFixture({
        releaseName: "MDVR-419 remains active",
        state: "downloading",
        transferId: "vr-active",
      }),
    ];
    const oldAdultCompletedRows = [
      ...vrDownloadFixture({
        category: "adult",
        code: "ADLT-123",
        releaseName: "ADLT-123 current",
        state: "downloading",
        transferId: "adult-current",
      }),
      ...vrDownloadFixture({
        category: "adult",
        code: "ADLT-124",
        downloadedBytes: "10",
        isCurrentFolder: "false",
        releaseName: "ADLT-124 old folder",
        speedBytesPerSecond: "0",
        state: "completed",
        transferId: "adult-old",
      }),
      ...vrDownloadFixture({
        releaseName: "MDVR-419 remains active",
        state: "downloading",
        transferId: "vr-active",
      }),
    ];
    const currentAdultCompletedRows = [
      ...vrDownloadFixture({
        category: "adult",
        code: "ADLT-123",
        downloadedBytes: "10",
        releaseName: "ADLT-123 current",
        speedBytesPerSecond: "0",
        state: "completed",
        transferId: "adult-current",
      }),
      ...oldAdultCompletedRows.slice(13),
    ];
    loadVrDownloadsMock.mockResolvedValue(activeRows);
    listVrDownloadsMock
      .mockResolvedValueOnce(oldAdultCompletedRows)
      .mockResolvedValueOnce(currentAdultCompletedRows)
      .mockResolvedValue(currentAdultCompletedRows);

    render(<App />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(scanAdultLibraryMock).toHaveBeenCalledOnce();
    expect(queryAdultStorageMock).toHaveBeenCalledOnce();
    expect(scanVrLibraryMock).toHaveBeenCalledOnce();
    expect(queryVrStorageMock).toHaveBeenCalledOnce();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(scanAdultLibraryMock).toHaveBeenCalledOnce();
    expect(queryAdultStorageMock).toHaveBeenCalledOnce();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(scanAdultLibraryMock).toHaveBeenCalledTimes(2);
    expect(queryAdultStorageMock).toHaveBeenCalledTimes(2);
    expect(scanVrLibraryMock).toHaveBeenCalledOnce();
    expect(queryVrStorageMock).toHaveBeenCalledOnce();
    expect(scanMoviesMock).not.toHaveBeenCalled();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(scanAdultLibraryMock).toHaveBeenCalledTimes(2);
    expect(queryAdultStorageMock).toHaveBeenCalledTimes(2);
  });

  it("refreshes only Movies Library and storage once for a current-folder Movie completion", async () => {
    vi.useFakeTimers();
    savedMoviesFolder = "/Movies";
    savedAdultFolder = "/Adult";
    savedVrFolder = "/VR";
    const activeRows = [
      ...vrDownloadFixture({
        category: "movie",
        code: "tt0123456",
        releaseName: "Current Movie",
        state: "downloading",
        transferId: "movie-current",
      }),
      ...vrDownloadFixture({
        category: "movie",
        code: "tt0765432",
        isCurrentFolder: "false",
        releaseName: "Old-folder Movie",
        state: "downloading",
        transferId: "movie-old",
      }),
    ];
    const oldFolderCompletedRows = [
      ...activeRows.slice(0, 13),
      ...vrDownloadFixture({
        category: "movie",
        code: "tt0765432",
        downloadedBytes: "10",
        isCurrentFolder: "false",
        releaseName: "Old-folder Movie",
        speedBytesPerSecond: "0",
        state: "completed",
        transferId: "movie-old",
      }),
    ];
    const currentFolderCompletedRows = [
      ...vrDownloadFixture({
        category: "movie",
        code: "tt0123456",
        downloadedBytes: "10",
        releaseName: "Current Movie",
        speedBytesPerSecond: "0",
        state: "completed",
        transferId: "movie-current",
      }),
      ...oldFolderCompletedRows.slice(13),
    ];
    loadVrDownloadsMock.mockResolvedValue(activeRows);
    listVrDownloadsMock
      .mockResolvedValueOnce(oldFolderCompletedRows)
      .mockResolvedValueOnce(currentFolderCompletedRows)
      .mockResolvedValue(currentFolderCompletedRows);

    render(<App />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(scanMoviesMock).toHaveBeenCalledOnce();
    expect(queryMoviesStorageMock).toHaveBeenCalledOnce();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(scanMoviesMock).toHaveBeenCalledOnce();
    expect(queryMoviesStorageMock).toHaveBeenCalledOnce();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(scanMoviesMock).toHaveBeenCalledTimes(2);
    expect(queryMoviesStorageMock).toHaveBeenCalledTimes(2);
    expect(scanAdultLibraryMock).toHaveBeenCalledOnce();
    expect(queryAdultStorageMock).toHaveBeenCalledOnce();
    expect(scanVrLibraryMock).toHaveBeenCalledOnce();
    expect(queryVrStorageMock).toHaveBeenCalledOnce();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(scanMoviesMock).toHaveBeenCalledTimes(2);
    expect(queryMoviesStorageMock).toHaveBeenCalledTimes(2);
  });
});

describe("Mira visual preset", () => {
  it("renders existing accessible controls with Base UI and Phosphor icons", () => {
    render(<App />);

    const dashboard = screen.getByRole("button", { name: "Dashboard" });
    expect(dashboard.getAttribute("data-slot")).toBe("button");

    const icons = Array.from(document.querySelectorAll("svg.app-icon"));
    expect(icons.length).toBeGreaterThan(0);
    expect(
      icons.every((icon) => icon.getAttribute("viewBox") === "0 0 256 256"),
    ).toBe(true);
  });
});

describe("Auto-Video application shell", () => {
  it("navigates to every destination and exposes the active page", () => {
    render(<App />);

    for (const destination of [
      "Dashboard",
      "Discover",
      "Library",
      "Downloads",
      "Settings",
    ]) {
      const navigationButton = screen.getByRole("button", {
        name: destination,
      });

      fireEvent.click(navigationButton);

      expect(
        screen.getByRole("heading", { level: 1, name: destination }),
      ).toBeTruthy();
      expect(navigationButton.getAttribute("aria-current")).toBe("page");
      expect(document.querySelectorAll('[aria-current="page"]')).toHaveLength(
        1,
      );
    }
  });

  it("shows truthful unavailable states without fabricated product data", async () => {
    render(<App />);

    expect(
      await screen.findByRole("heading", {
        level: 3,
        name: "Movies Library is not configured",
      }),
    ).toBeTruthy();
    expect(
      screen.getByRole("heading", { level: 3, name: "Storage unavailable" }),
    ).toBeTruthy();

    selectDiscover();
    expect(
      await screen.findByRole("heading", {
        level: 2,
        name: "Configure TMDB to discover movies",
      }),
    ).toBeTruthy();

    selectLibrary();
    expect(
      screen.getByRole("heading", {
        level: 2,
        name: "Choose a Movies folder to begin",
      }),
    ).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Downloads" }));
    expect(
      screen.getByRole("heading", {
        level: 2,
        name: "No downloads",
      }),
    ).toBeTruthy();

    selectSettings();
    expect(
      screen.getByRole("heading", {
        level: 2,
        name: "VR folder",
      }),
    ).toBeTruthy();
  });

  it("moves keyboard focus through the vertical navigation", () => {
    render(<App />);

    const dashboard = screen.getByRole("button", { name: "Dashboard" });
    const discover = screen.getByRole("button", { name: "Discover" });
    const settings = screen.getByRole("button", { name: "Settings" });

    dashboard.focus();
    fireEvent.keyDown(dashboard, { key: "ArrowDown" });
    expect(document.activeElement).toBe(discover);

    fireEvent.keyDown(discover, { key: "End" });
    expect(document.activeElement).toBe(settings);

    fireEvent.keyDown(settings, { key: "ArrowDown" });
    expect(document.activeElement).toBe(dashboard);
  });

  it("returns the workspace to the page header after navigation", () => {
    render(<App />);
    const workspace = document.querySelector<HTMLElement>(".workspace");
    expect(workspace).not.toBeNull();
    (workspace as HTMLElement).scrollTop = 240;

    selectSettings();

    expect(workspace?.scrollTop).toBe(0);
  });

  it("selects light, dark, and system appearance modes", () => {
    render(<App />);
    selectSettings();

    for (const mode of ["Light", "Dark", "System"]) {
      const appearanceControl = screen.getByRole("radio", {
        name: mode,
      }) as HTMLInputElement;

      fireEvent.click(appearanceControl);

      expect(appearanceControl.checked).toBe(true);
      expect(document.documentElement.dataset.appearance).toBe(
        mode.toLowerCase(),
      );
    }
  });

  it("restores the persisted appearance mode after relaunch", () => {
    render(<App />);
    selectSettings();
    fireEvent.click(screen.getByRole("radio", { name: "Dark" }));

    expect(window.localStorage.getItem("auto-video-appearance")).toBe("dark");

    cleanup();
    render(<App />);
    selectSettings();

    expect(
      (screen.getByRole("radio", { name: "Dark" }) as HTMLInputElement)
        .checked,
    ).toBe(true);
    expect(document.documentElement.dataset.theme).toBe("dark");
  });

  it("updates system mode when the operating-system preference changes", () => {
    render(<App />);

    expect(document.documentElement.dataset.appearance).toBe("system");
    expect(document.documentElement.dataset.theme).toBe("light");

    setSystemPreference(true);
    expect(document.documentElement.dataset.theme).toBe("dark");

    setSystemPreference(false);
    expect(document.documentElement.dataset.theme).toBe("light");
  });
});

describe("Movies Library Dashboard", () => {
  it("shows configuration loading before the unconfigured state and opens Settings", async () => {
    const pendingFolder = createDeferred<string | null>();
    loadMoviesFolderMock.mockReturnValue(pendingFolder.promise);

    render(<App />);

    const summary = screen.getByRole("region", { name: "Movies Library" });
    expect(summary.getAttribute("aria-busy")).toBe("true");
    expect(
      screen.getByRole("heading", {
        level: 3,
        name: "Loading Movies Library",
      }),
    ).toBeTruthy();
    expect(screen.queryByRole("button", { name: /^Open / })).toBeNull();

    await act(async () => {
      pendingFolder.resolve(null);
      await pendingFolder.promise;
    });

    expect(
      await screen.findByRole("heading", {
        level: 3,
        name: "Movies Library is not configured",
      }),
    ).toBeTruthy();
    const openSettings = screen.getByRole("button", {
      name: "Open Settings",
    });
    openSettings.focus();
    expect(document.activeElement).toBe(openSettings);
    fireEvent.click(openSettings);
    expect(
      screen.getByRole("heading", { level: 1, name: "Settings" }),
    ).toBeTruthy();
    expect(scanMoviesMock).not.toHaveBeenCalled();
    expect(queryMoviesStorageMock).not.toHaveBeenCalled();
  });

  it("shows the exact configured path and complete scan count without rescanning on navigation or resize", async () => {
    const folder =
      "C:\\映像ライブラリ\\Family — Archive & Restored Editions\\A very long configured Movies folder name";
    const paths = Array.from(
      { length: 25 },
      (_, index) =>
        `${folder}\\Movie ${String(index + 1).padStart(2, "0")}.mkv`,
    );
    savedMoviesFolder = folder;
    scanMoviesMock.mockResolvedValue(paths);

    render(<App />);

    expect(
      await screen.findByRole("heading", {
        level: 3,
        name: "25 supported Movies",
      }),
    ).toBeTruthy();
    expect(
      document.querySelector(".dashboard-library-summary__folder")
        ?.textContent,
    ).toBe(folder);
    expect(scanMoviesMock).toHaveBeenCalledTimes(1);
    expect(queryMoviesStorageMock).toHaveBeenCalledTimes(1);
    expect(fetchMock).not.toHaveBeenCalled();

    fireEvent(window, new Event("resize"));
    const openLibrary = screen.getByRole("button", { name: "Open Library" });
    openLibrary.focus();
    expect(document.activeElement).toBe(openLibrary);
    fireEvent.click(openLibrary);
    await screen.findByText("Movie 01");
    resizeGallery("library", 1528, 136);
    expect(visibleCardCount("Movies")).toBe(7);
    fireEvent.click(
      screen.getByRole("button", { name: "Next Movies page" }),
    );
    expect(screen.getByText("Page 2 of 4")).toBeTruthy();
    expect(screen.getByText("Movie 08")).toBeTruthy();

    selectDashboard();
    expect(
      screen.getByRole("heading", {
        level: 3,
        name: "25 supported Movies",
      }),
    ).toBeTruthy();
    expect(scanMoviesMock).toHaveBeenCalledTimes(1);
    expect(queryMoviesStorageMock).toHaveBeenCalledTimes(1);
    expect(fetchMock).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Open Library" }));
    expect(screen.getByText("Page 2 of 4")).toBeTruthy();
    expect(screen.getByText("Movie 08")).toBeTruthy();
    expect(scanMoviesMock).toHaveBeenCalledTimes(1);
    expect(queryMoviesStorageMock).toHaveBeenCalledTimes(1);

    selectSettings();
    fireEvent.click(screen.getByRole("radio", { name: "Dark" }));
    selectDashboard();
    fireEvent(window, new Event("resize"));
    expect(queryMoviesStorageMock).toHaveBeenCalledTimes(1);
    expect(scanMoviesMock).toHaveBeenCalledTimes(1);
    expect(fetchMock).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Open Library" }));
    expect(screen.getByText("Page 2 of 4")).toBeTruthy();
    expect(screen.getByText("Movie 08")).toBeTruthy();
  });

  it("reports an available empty folder as exactly zero Movies", async () => {
    savedMoviesFolder = "/Movies/Empty";

    render(<App />);

    expect(
      await screen.findByRole("heading", {
        level: 3,
        name: "0 supported Movies",
      }),
    ).toBeTruthy();
    expect(screen.getByRole("button", { name: "Open Library" })).toBeTruthy();
  });

  it("distinguishes unavailable and failed scans and routes each action appropriately", async () => {
    savedMoviesFolder = "/Movies";
    scanMoviesMock.mockRejectedValueOnce("movies_folder_unavailable");

    render(<App />);

    expect(await screen.findByRole("alert")).toHaveProperty(
      "textContent",
      expect.stringContaining("Movies folder is unavailable"),
    );
    fireEvent.click(screen.getByRole("button", { name: "Open Settings" }));
    expect(
      screen.getByRole("heading", { level: 1, name: "Settings" }),
    ).toBeTruthy();

    cleanup();
    scanMoviesMock.mockRejectedValueOnce("movies_scan_failed");
    render(<App />);

    expect(await screen.findByRole("alert")).toHaveProperty(
      "textContent",
      expect.stringContaining("Movies Library scan failed"),
    );
    await waitFor(() => expect(storageValue("Total")).toBe("1.0 TiB"));
    expect(storageValue("Used")).toBe("768.0 GiB");
    fireEvent.click(screen.getByRole("button", { name: "Open Library" }));
    expect(
      screen.getByRole("heading", { level: 1, name: "Library" }),
    ).toBeTruthy();
  });

  it("routes a native folder configuration failure to Settings without claiming the Library is unconfigured", async () => {
    loadMoviesFolderMock.mockRejectedValue(new Error("store unavailable"));

    render(<App />);

    expect(await screen.findByRole("alert")).toHaveProperty(
      "textContent",
      expect.stringContaining("Movies Library needs attention"),
    );
    expect(
      screen.queryByRole("heading", {
        level: 3,
        name: "Movies Library is not configured",
      }),
    ).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Open Settings" }));
    expect(
      screen.getByText("The Movies folder configuration could not be loaded."),
    ).toBeTruthy();
    expect(queryMoviesStorageMock).not.toHaveBeenCalled();
  });

  it("tracks folder choices, refresh results, successful Trash actions, and clearing", async () => {
    const folderA = "/Movies/Family — 家族";
    const folderB = "D:\\Movies & Archive";
    const currentMovie = `${folderB}\\Current.mp4`;
    const newMovie = `${folderB}\\New arrival.mkv`;
    const pendingRefresh = createDeferred<string[]>();
    openFolderMock
      .mockResolvedValueOnce(folderA)
      .mockResolvedValueOnce(folderB);
    scanMoviesMock
      .mockResolvedValueOnce([
        `${folderA}/First.mp4`,
        `${folderA}/Second.mkv`,
      ])
      .mockResolvedValueOnce([currentMovie])
      .mockReturnValueOnce(pendingRefresh.promise);
    queryMoviesStorageMock
      .mockResolvedValueOnce(["4398046511104", "2199023255552"])
      .mockResolvedValueOnce(["6597069766656", "2199023255552"])
      .mockResolvedValueOnce(["6597069766656", "3298534883328"])
      .mockResolvedValueOnce(["6597069766656", "4398046511104"]);

    render(<App />);
    await screen.findByRole("heading", {
      level: 3,
      name: "Movies Library is not configured",
    });

    fireEvent.click(screen.getByRole("button", { name: "Open Settings" }));
    fireEvent.click(screen.getByRole("button", { name: "Choose folder" }));
    await screen.findByText(folderA);
    selectDashboard();
    await screen.findByRole("heading", {
      level: 3,
      name: "2 supported Movies",
    });
    await waitFor(() => expect(storageValue("Total")).toBe("4.0 TiB"));
    expect(storageValue("Used")).toBe("2.0 TiB");

    selectSettings();
    fireEvent.click(screen.getByRole("button", { name: "Change folder" }));
    await screen.findByText(folderB);
    selectDashboard();
    await screen.findByRole("heading", {
      level: 3,
      name: "1 supported Movie",
    });
    await waitFor(() => expect(storageValue("Total")).toBe("6.0 TiB"));
    expect(storageValue("Used")).toBe("4.0 TiB");

    fireEvent.click(screen.getByRole("button", { name: "Open Library" }));
    await screen.findByText("Current");
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    selectDashboard();
    await screen.findByRole("heading", {
      level: 3,
      name: "Scanning Movies Library",
    });
    await act(async () => {
      pendingRefresh.resolve([currentMovie, newMovie]);
      await pendingRefresh.promise;
    });
    await screen.findByRole("heading", {
      level: 3,
      name: "2 supported Movies",
    });
    await waitFor(() => expect(storageValue("Used")).toBe("3.0 TiB"));

    fireEvent.click(screen.getByRole("button", { name: "Open Library" }));
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Move movie to Trash or Recycle Bin: Current",
      }),
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: "Confirm moving movie to Trash or Recycle Bin: Current",
      }),
    );
    await waitFor(() => expect(screen.queryByText("Current")).toBeNull());
    selectDashboard();
    expect(
      screen.getByRole("heading", {
        level: 3,
        name: "1 supported Movie",
      }),
    ).toBeTruthy();
    await waitFor(() => expect(storageValue("Used")).toBe("2.0 TiB"));

    selectSettings();
    fireEvent.click(screen.getByRole("button", { name: "Clear folder" }));
    await screen.findByText("No Movies folder configured.");
    selectDashboard();
    expect(
      screen.getByRole("heading", {
        level: 3,
        name: "Movies Library is not configured",
      }),
    ).toBeTruthy();
    expect(
      screen.getByRole("heading", { level: 3, name: "Storage unavailable" }),
    ).toBeTruthy();
    expect(scanMoviesMock).toHaveBeenCalledTimes(3);
    expect(queryMoviesStorageMock).toHaveBeenCalledTimes(4);
    expect(trashMovieMock).toHaveBeenCalledTimes(1);
  });
});

describe("Movies volume storage Dashboard", () => {
  it("keeps the Library count visible while loading and formats consistent total, used, and free values", async () => {
    const pendingStorage = createDeferred<[string, string]>();
    savedMoviesFolder = "/Movies";
    scanMoviesMock.mockResolvedValue(["/Movies/Current.mp4"]);
    queryMoviesStorageMock.mockReturnValue(pendingStorage.promise);

    render(<App />);

    expect(
      await screen.findByRole("heading", {
        level: 3,
        name: "1 supported Movie",
      }),
    ).toBeTruthy();
    expect(
      screen.getByRole("heading", { level: 3, name: "Loading storage" }),
    ).toBeTruthy();

    await act(async () => {
      pendingStorage.resolve(["2199023255552", "549755813888"]);
      await pendingStorage.promise;
    });

    expect(storageValue("Total")).toBe("2.0 TiB");
    expect(storageValue("Used")).toBe("1.5 TiB");
    expect(storageValue("Free")).toBe("512.0 GiB");
    expect(invokeMock).toHaveBeenCalledWith("query_movies_storage");
  });

  it.each([
    ["zero capacity", ["0", "0"]],
    ["free bytes above total", ["1024", "1025"]],
    ["non-integer bytes", ["1024", "unknown"]],
  ] as const)("rejects %s without hiding a valid Movies count", async (_case, values) => {
    savedMoviesFolder = "/Movies";
    scanMoviesMock.mockResolvedValue(["/Movies/Current.mp4"]);
    queryMoviesStorageMock.mockResolvedValue([...values]);

    render(<App />);

    expect(
      await screen.findByRole("heading", {
        level: 3,
        name: "Storage could not be loaded",
      }),
    ).toBeTruthy();
    expect(
      screen.getByRole("heading", {
        level: 3,
        name: "1 supported Movie",
      }),
    ).toBeTruthy();
    expect(screen.queryByLabelText("Movies volume storage")).toBeNull();
  });

  it("distinguishes an unavailable volume from a failed storage query", async () => {
    savedMoviesFolder = "/Movies";
    queryMoviesStorageMock.mockRejectedValueOnce(
      "movies_storage_unavailable",
    );

    render(<App />);
    expect(
      await screen.findByRole("heading", {
        level: 3,
        name: "Movies volume is unavailable",
      }),
    ).toBeTruthy();

    cleanup();
    queryMoviesStorageMock.mockRejectedValueOnce("movies_storage_failed");
    render(<App />);
    expect(
      await screen.findByRole("heading", {
        level: 3,
        name: "Storage could not be loaded",
      }),
    ).toBeTruthy();
  });

  it("ignores old-folder and superseded-refresh storage responses", async () => {
    const oldFolderStorage = createDeferred<[string, string]>();
    const supersededStorage = createDeferred<[string, string]>();
    const oldFolder = "/Movies/Old";
    const newFolder = "/Movies/New";
    savedMoviesFolder = oldFolder;
    openFolderMock.mockResolvedValue(newFolder);
    scanMoviesMock
      .mockResolvedValueOnce([`${oldFolder}/Old.mp4`])
      .mockResolvedValueOnce([`${newFolder}/Current.mp4`])
      .mockResolvedValueOnce([`${newFolder}/Current.mp4`]);
    queryMoviesStorageMock
      .mockReturnValueOnce(oldFolderStorage.promise)
      .mockReturnValueOnce(supersededStorage.promise)
      .mockResolvedValueOnce(["4398046511104", "1099511627776"]);

    render(<App />);
    await screen.findByRole("heading", {
      level: 3,
      name: "1 supported Movie",
    });

    selectSettings();
    fireEvent.click(screen.getByRole("button", { name: "Change folder" }));
    await screen.findByText(newFolder);
    selectLibrary();
    await screen.findByText("Current");
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    selectDashboard();
    await waitFor(() => expect(storageValue("Total")).toBe("4.0 TiB"));
    expect(storageValue("Used")).toBe("3.0 TiB");

    await act(async () => {
      supersededStorage.resolve(["6597069766656", "1099511627776"]);
      oldFolderStorage.resolve(["8796093022208", "1099511627776"]);
      await Promise.all([
        supersededStorage.promise,
        oldFolderStorage.promise,
      ]);
    });

    expect(storageValue("Total")).toBe("4.0 TiB");
    expect(storageValue("Used")).toBe("3.0 TiB");
    expect(queryMoviesStorageMock).toHaveBeenCalledTimes(3);
  });
});

describe("TMDB Discover", () => {
  it("does not request TMDB without a token and directs the user to Settings", async () => {
    render(<App />);
    selectDiscover();

    expect(
      await screen.findByRole("heading", {
        level: 2,
        name: "Configure TMDB to discover movies",
      }),
    ).toBeTruthy();
    expect(fetchMock).not.toHaveBeenCalled();
    expect(screen.getByLabelText("TMDB credits").textContent).toContain(
      "This product uses the TMDB API but is not endorsed or certified by TMDB.",
    );

    fireEvent.click(screen.getByRole("button", { name: "Open Settings" }));
    expect(
      screen.getByRole("heading", {
        level: 2,
        name: "TMDB API Read Access Token",
      }),
    ).toBeTruthy();
  });

  it("saves, replaces, and clears a masked local token without rendering it", async () => {
    const firstToken = "first-fixture-token";
    const replacementToken = "replacement-fixture-token";

    render(<App />);
    selectSettings();
    expect(await screen.findByText("No TMDB token configured.")).toBeTruthy();

    const tokenInput = screen.getByLabelText("Token") as HTMLInputElement;
    expect(tokenInput.type).toBe("password");
    fireEvent.change(tokenInput, { target: { value: firstToken } });
    fireEvent.click(screen.getByRole("button", { name: "Save token" }));

    expect(await screen.findByText("TMDB token saved.")).toBeTruthy();
    expect(saveTmdbTokenMock).toHaveBeenLastCalledWith({ token: firstToken });
    expect(tokenInput.value).toBe("");
    expect(document.body.textContent).not.toContain(firstToken);

    fireEvent.change(screen.getByLabelText("New token"), {
      target: { value: replacementToken },
    });
    fireEvent.click(screen.getByRole("button", { name: "Replace token" }));

    expect(await screen.findByText("TMDB token replaced.")).toBeTruthy();
    expect(saveTmdbTokenMock).toHaveBeenLastCalledWith({
      token: replacementToken,
    });
    expect(document.body.textContent).not.toContain(replacementToken);

    fireEvent.click(screen.getByRole("button", { name: "Clear token" }));
    expect(await screen.findByText("TMDB token cleared.")).toBeTruthy();
    expect(clearTmdbTokenMock).toHaveBeenCalledTimes(1);
    expect(screen.getByText("No TMDB token configured.")).toBeTruthy();
  });

  it("loads a persisted token without placing its saved value in the form", async () => {
    const savedToken = "persisted-fixture-token";
    loadTmdbTokenMock.mockResolvedValue(savedToken);

    render(<App />);
    selectSettings();

    expect(
      await screen.findByText("TMDB token configured on this device."),
    ).toBeTruthy();
    expect((screen.getByLabelText("New token") as HTMLInputElement).value).toBe(
      "",
    );
    expect(document.body.textContent).not.toContain(savedToken);
  });

  it("renders one accessible card per valid fixture movie with poster fallbacks", async () => {
    loadTmdbTokenMock.mockResolvedValue("fixture-token");
    fetchMock.mockResolvedValue(
      jsonResponse({
        results: [
          {
            id: 81,
            title: "映画  —  Director's “Cut”!",
            poster_path: "/working-poster.jpg",
            release_date: "2026-08-01",
          },
          {
            id: 82,
            title: "Posterless Movie",
            poster_path: null,
            release_date: "",
          },
          { id: "invalid", title: "Malformed movie" },
        ],
      }),
    );

    render(<App />);
    selectDiscover();

    const exactTitle = await screen.findByRole("heading", {
      level: 3,
      name: "映画 — Director's “Cut”!",
    });
    expect(exactTitle.textContent).toBe("映画  —  Director's “Cut”!");
    expect(screen.getAllByRole("article")).toHaveLength(2);
    expect(
      screen.getByRole("article", { name: "Posterless Movie" }),
    ).toBeTruthy();
    expect(screen.getByText("2026-08-01")).toBeTruthy();
    expect(screen.getAllByText("TMDB")).toHaveLength(2);
    expect(screen.getByRole("img", { name: "TMDB" })).toBeTruthy();
    expect(screen.getByText("Poster unavailable")).toBeTruthy();

    const poster = document.querySelector<HTMLImageElement>(
      'img[src="https://image.tmdb.org/t/p/w500/working-poster.jpg"]',
    );
    expect(poster).not.toBeNull();
    fireEvent.error(poster as HTMLImageElement);
    expect(screen.getAllByText("Poster unavailable")).toHaveLength(2);
  });

  it("submits an exact title query explicitly and reuses accessible Discover cards", async () => {
    const token = "search-fixture-token";
    const query = "  映画 — Director's “Cut”! & CAPS  ";
    const exactTitle = "映画  —  Search Director's “Cut”!";
    loadTmdbTokenMock.mockResolvedValue(token);
    fetchMock
      .mockResolvedValueOnce(
        jsonResponse({ results: [{ id: 1, title: "Trending result" }] }),
      )
      .mockResolvedValueOnce(
        jsonResponse({
          results: [
            {
              id: 2,
              title: exactTitle,
              poster_path: null,
              release_date: "2026-08-03",
            },
          ],
        }),
      );

    render(<App />);
    selectDiscover();
    expect(await screen.findByText("Trending result")).toBeTruthy();

    const searchInput = screen.getByRole("textbox", {
      name: "Search Movies",
    });
    const searchButton = screen.getByRole("button", { name: "Search" });
    searchInput.focus();
    expect(document.activeElement).toBe(searchInput);
    fireEvent.change(searchInput, { target: { value: query } });
    expect(fetchMock).toHaveBeenCalledTimes(1);
    searchButton.focus();
    expect(document.activeElement).toBe(searchButton);
    fireEvent.submit(screen.getByRole("search", { name: "Search TMDB Movies" }));

    const resultHeading = await screen.findByRole("heading", {
      level: 3,
      name: "映画 — Search Director's “Cut”!",
    });
    expect(resultHeading.textContent).toBe(exactTitle);
    expect(
      screen.getByRole("list", { name: "TMDB Movies search results" }),
    ).toBeTruthy();
    expect(
      screen.getByRole("heading", {
        level: 2,
        name: "TMDB Movies search results",
      }),
    ).toBeTruthy();
    expect(screen.getByText("2026-08-03")).toBeTruthy();
    expect(screen.getByText("Poster unavailable")).toBeTruthy();

    const [requestUrl, requestOptions] = fetchMock.mock.calls[1];
    const parsedRequestUrl = new URL(String(requestUrl));
    expect(parsedRequestUrl.pathname).toBe("/3/search/movie");
    expect(parsedRequestUrl.searchParams.get("query")).toBe(query);
    expect(String(requestUrl)).not.toContain(token);
    expect(requestOptions?.headers).toEqual({
      Accept: "application/json",
      Authorization: `Bearer ${token}`,
    });
    expect(document.body.textContent).not.toContain(token);

    fireEvent.click(
      screen.getByRole("button", {
        name: /Copy title: 映画.*Search Director's “Cut”!/,
      }),
    );
    expect(clipboardWriteMock).toHaveBeenCalledWith(exactTitle);
    expect(scanMoviesMock).not.toHaveBeenCalled();
    expect(queryMoviesStorageMock).not.toHaveBeenCalled();
    expect(openMovieMock).not.toHaveBeenCalled();
    expect(revealMovieMock).not.toHaveBeenCalled();
    expect(trashMovieMock).not.toHaveBeenCalled();
  });

  it("rejects an empty title query locally without replacing trending results", async () => {
    loadTmdbTokenMock.mockResolvedValue("fixture-token");
    fetchMock.mockResolvedValue(
      jsonResponse({ results: [{ id: 1, title: "Trending result" }] }),
    );

    render(<App />);
    selectDiscover();
    expect(await screen.findByText("Trending result")).toBeTruthy();

    submitDiscoverSearch(" \t ");

    expect(screen.getByRole("alert").textContent).toBe(
      "Enter a movie title to search TMDB.",
    );
    expect(
      screen
        .getByRole("textbox", { name: "Search Movies" })
        .getAttribute("aria-invalid"),
    ).toBe("true");
    expect(screen.getByText("Trending result")).toBeTruthy();
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it.each([
    {
      caseName: "empty results",
      heading: "No TMDB Movies match this search",
      response: jsonResponse({ results: [] }),
    },
    {
      caseName: "unauthorized token",
      heading: "TMDB token was not accepted",
      response: jsonResponse({}, 401),
    },
    {
      caseName: "rate limit",
      heading: "TMDB rate limit reached",
      response: jsonResponse({}, 429),
    },
    {
      caseName: "general provider failure",
      heading: "TMDB could not search Movies",
      response: jsonResponse({}, 500),
    },
    {
      caseName: "malformed provider data",
      heading: "TMDB returned invalid search data",
      response: jsonResponse({ page: 1 }),
    },
  ])(
    "shows the distinct search $caseName state as $heading",
    async ({ heading, response }) => {
      loadTmdbTokenMock.mockResolvedValue("fixture-token");
      fetchMock
        .mockResolvedValueOnce(
          jsonResponse({ results: [{ id: 1, title: "Trending result" }] }),
        )
        .mockResolvedValueOnce(response);

      render(<App />);
      selectDiscover();
      expect(await screen.findByText("Trending result")).toBeTruthy();
      submitDiscoverSearch("Fixture query");

      expect(
        await screen.findByRole("heading", { level: 2, name: heading }),
      ).toBeTruthy();
    },
  );

  it("shows distinct search loading and network states", async () => {
    const pendingSearch = createDeferred<Response>();
    loadTmdbTokenMock.mockResolvedValue("fixture-token");
    fetchMock
      .mockResolvedValueOnce(
        jsonResponse({ results: [{ id: 1, title: "Trending result" }] }),
      )
      .mockReturnValueOnce(pendingSearch.promise);

    render(<App />);
    selectDiscover();
    expect(await screen.findByText("Trending result")).toBeTruthy();
    submitDiscoverSearch("Fixture query");

    expect(
      await screen.findByRole("heading", {
        level: 2,
        name: "Searching TMDB Movies",
      }),
    ).toBeTruthy();

    await act(async () => {
      pendingSearch.reject(new TypeError("offline"));
      await Promise.resolve();
    });
    expect(
      await screen.findByRole("heading", {
        level: 2,
        name: "TMDB search could not be reached",
      }),
    ).toBeTruthy();
  });

  it("copies the exact Discover title with isolated keyboard-accessible feedback", async () => {
    const title = "映画  —  A Very Long Director's “CUT”!";
    loadTmdbTokenMock.mockResolvedValue("fixture-token");
    fetchMock.mockResolvedValue(
      jsonResponse({ results: [{ id: 91, title, poster_path: null }] }),
    );
    const parentActivation = vi.fn();

    render(
      <div onClick={parentActivation} onPointerDown={parentActivation}>
        <App />
      </div>,
    );
    selectDiscover();

    const heading = await screen.findByRole("heading", {
      level: 3,
      name: "映画 — A Very Long Director's “CUT”!",
    });
    const card = heading.closest("article") as HTMLElement;
    const copyButton = within(card).getByRole("button", {
      name: /Copy title:/,
    });
    expect(copyButton.getAttribute("aria-label")).toBe(`Copy title: ${title}`);
    expect(copyButton.getAttribute("data-copy-state")).toBe("idle");
    copyButton.focus();
    expect(document.activeElement).toBe(copyButton);
    parentActivation.mockClear();

    vi.useFakeTimers();
    fireEvent.pointerDown(copyButton);
    await act(async () => {
      fireEvent.click(copyButton);
      await Promise.resolve();
    });

    expect(clipboardWriteMock).toHaveBeenCalledWith(title);
    expect(openMovieMock).not.toHaveBeenCalled();
    expect(revealMovieMock).not.toHaveBeenCalled();
    expect(parentActivation).not.toHaveBeenCalled();
    expect(copyButton.getAttribute("aria-label")).toBe(
      `Copied title: ${title}`,
    );
    expect(trashMovieMock).not.toHaveBeenCalled();
    expect(within(card).getByRole("status").textContent).toBe(
      `Copied title: ${title}`,
    );

    act(() => vi.advanceTimersByTime(2000));
    expect(copyButton.getAttribute("aria-label")).toBe(`Copy title: ${title}`);
  });

  it("reports a rejected Discover clipboard write on its card", async () => {
    clipboardWriteMock.mockRejectedValue(new Error("permission denied"));
    loadTmdbTokenMock.mockResolvedValue("fixture-token");
    fetchMock.mockResolvedValue(
      jsonResponse({ results: [{ id: 92, title: "Rejected title" }] }),
    );

    render(<App />);
    selectDiscover();

    const copyButton = await screen.findByRole("button", {
      name: "Copy title: Rejected title",
    });
    fireEvent.click(copyButton);

    expect(
      await screen.findByRole("button", {
        name: "Copy failed for title: Rejected title",
      }),
    ).toHaveProperty("textContent", "Failed");
    expect(screen.getByRole("alert").textContent).toBe(
      "Copy failed for title: Rejected title",
    );
  });

  it.each([
    {
      caseName: "empty feed",
      heading: "No trending movies returned",
      response: jsonResponse({ results: [] }),
    },
    {
      caseName: "unauthorized token",
      heading: "TMDB token was not accepted",
      response: jsonResponse({}, 401),
    },
    {
      caseName: "rate limit",
      heading: "TMDB rate limit reached",
      response: jsonResponse({}, 429),
    },
    {
      caseName: "provider failure",
      heading: "TMDB could not load trending Movies",
      response: jsonResponse({}, 500),
    },
    {
      caseName: "malformed response",
      heading: "TMDB could not load trending Movies",
      response: jsonResponse({ page: 1 }),
    },
  ])("shows the $caseName state as $heading", async ({ heading, response }) => {
    loadTmdbTokenMock.mockResolvedValue("fixture-token");
    fetchMock.mockResolvedValue(response);

    render(<App />);
    selectDiscover();

    expect(
      await screen.findByRole("heading", { level: 2, name: heading }),
    ).toBeTruthy();
  });

  it("shows a distinct network failure state", async () => {
    loadTmdbTokenMock.mockResolvedValue("fixture-token");
    fetchMock.mockRejectedValue(new TypeError("offline"));

    render(<App />);
    selectDiscover();

    expect(
      await screen.findByRole("heading", {
        level: 2,
        name: "TMDB could not be reached",
      }),
    ).toBeTruthy();
  });

  it("keeps the newest Refresh result when an earlier request finishes late", async () => {
    const earlierRefresh = createDeferred<Response>();
    const latestRefresh = createDeferred<Response>();
    loadTmdbTokenMock.mockResolvedValue("fixture-token");
    fetchMock
      .mockResolvedValueOnce(
        jsonResponse({ results: [{ id: 1, title: "Initial result" }] }),
      )
      .mockReturnValueOnce(earlierRefresh.promise)
      .mockReturnValueOnce(latestRefresh.promise);

    render(<App />);
    selectDiscover();
    expect(await screen.findByText("Initial result")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));

    await act(async () => {
      latestRefresh.resolve(
        jsonResponse({ results: [{ id: 3, title: "Latest result" }] }),
      );
      await latestRefresh.promise;
    });
    expect(await screen.findByText("Latest result")).toBeTruthy();

    await act(async () => {
      earlierRefresh.resolve(
        jsonResponse({ results: [{ id: 2, title: "Stale result" }] }),
      );
      await earlierRefresh.promise;
    });
    expect(screen.queryByText("Stale result")).toBeNull();
    expect(screen.getByText("Latest result")).toBeTruthy();
  });

  it("restores cached trending results when Clear invalidates a pending search", async () => {
    const pendingSearch = createDeferred<Response>();
    loadTmdbTokenMock.mockResolvedValue("fixture-token");
    fetchMock
      .mockResolvedValueOnce(
        jsonResponse({ results: [{ id: 1, title: "Cached trending result" }] }),
      )
      .mockReturnValueOnce(pendingSearch.promise);

    render(<App />);
    selectDiscover();
    expect(await screen.findByText("Cached trending result")).toBeTruthy();
    submitDiscoverSearch("Pending query");
    expect(
      await screen.findByRole("heading", {
        level: 2,
        name: "Searching TMDB Movies",
      }),
    ).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Clear" }));

    expect(await screen.findByText("Cached trending result")).toBeTruthy();
    expect(
      screen.getByRole("heading", {
        level: 2,
        name: "Weekly trending Movies",
      }),
    ).toBeTruthy();
    expect(
      screen.getByRole("textbox", { name: "Search Movies" }),
    ).toHaveProperty("value", "");
    expect(
      screen.getByRole("button", { name: "Discover" }).getAttribute(
        "aria-current",
      ),
    ).toBe("page");
    expect(fetchMock).toHaveBeenCalledTimes(2);

    await act(async () => {
      pendingSearch.resolve(
        jsonResponse({ results: [{ id: 2, title: "Stale search result" }] }),
      );
      await pendingSearch.promise;
    });
    expect(screen.queryByText("Stale search result")).toBeNull();
    expect(screen.getByText("Cached trending result")).toBeTruthy();
  });

  it("refreshes the active search and then the restored trending mode", async () => {
    const earlierSearchRefresh = createDeferred<Response>();
    const latestSearchRefresh = createDeferred<Response>();
    loadTmdbTokenMock.mockResolvedValue("fixture-token");
    fetchMock
      .mockResolvedValueOnce(
        jsonResponse({ results: [{ id: 1, title: "Trending result" }] }),
      )
      .mockResolvedValueOnce(
        jsonResponse({ results: [{ id: 2, title: "Search result" }] }),
      )
      .mockReturnValueOnce(earlierSearchRefresh.promise)
      .mockReturnValueOnce(latestSearchRefresh.promise)
      .mockResolvedValueOnce(
        jsonResponse({ results: [{ id: 5, title: "Refreshed trending" }] }),
      );

    render(<App />);
    selectDiscover();
    expect(await screen.findByText("Trending result")).toBeTruthy();
    submitDiscoverSearch("Active query");
    expect(await screen.findByText("Search result")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await act(async () => {
      latestSearchRefresh.resolve(
        jsonResponse({ results: [{ id: 4, title: "Latest search refresh" }] }),
      );
      await latestSearchRefresh.promise;
    });
    expect(await screen.findByText("Latest search refresh")).toBeTruthy();
    expect(
      new URL(String(fetchMock.mock.calls[3][0])).searchParams.get("query"),
    ).toBe("Active query");

    await act(async () => {
      earlierSearchRefresh.resolve(
        jsonResponse({ results: [{ id: 3, title: "Stale search refresh" }] }),
      );
      await earlierSearchRefresh.promise;
    });
    expect(screen.queryByText("Stale search refresh")).toBeNull();
    expect(screen.getByText("Latest search refresh")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Clear" }));
    expect(await screen.findByText("Trending result")).toBeTruthy();
    expect(fetchMock).toHaveBeenCalledTimes(4);

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(await screen.findByText("Refreshed trending")).toBeTruthy();
    expect(fetchMock.mock.calls[4][0]).toBe(
      "https://api.themoviedb.org/3/trending/movie/week",
    );
  });

  it("keeps the newest title search through token replacement and ignores the older result", async () => {
    const olderSearch = createDeferred<Response>();
    const oldToken = "old-search-token";
    const newToken = "new-search-token";
    loadTmdbTokenMock.mockResolvedValue(oldToken);
    fetchMock
      .mockResolvedValueOnce(
        jsonResponse({ results: [{ id: 1, title: "Trending result" }] }),
      )
      .mockReturnValueOnce(olderSearch.promise)
      .mockResolvedValueOnce(
        jsonResponse({ results: [{ id: 3, title: "Newest search result" }] }),
      )
      .mockResolvedValueOnce(
        jsonResponse({ results: [{ id: 4, title: "New token result" }] }),
      );

    render(<App />);
    selectDiscover();
    expect(await screen.findByText("Trending result")).toBeTruthy();
    submitDiscoverSearch("Older query");
    submitDiscoverSearch("Newest query");
    expect(await screen.findByText("Newest search result")).toBeTruthy();

    selectSettings();
    await screen.findByText("TMDB token configured on this device.");
    fireEvent.change(screen.getByLabelText("New token"), {
      target: { value: newToken },
    });
    fireEvent.click(screen.getByRole("button", { name: "Replace token" }));
    expect(await screen.findByText("TMDB token replaced.")).toBeTruthy();
    selectDiscover();
    expect(await screen.findByText("New token result")).toBeTruthy();

    const replacementRequest = fetchMock.mock.calls[3];
    expect(
      new URL(String(replacementRequest[0])).searchParams.get("query"),
    ).toBe("Newest query");
    expect(replacementRequest[1]?.headers).toEqual({
      Accept: "application/json",
      Authorization: `Bearer ${newToken}`,
    });
    expect(String(replacementRequest[0])).not.toContain(oldToken);
    expect(String(replacementRequest[0])).not.toContain(newToken);
    expect(document.body.textContent).not.toContain(oldToken);
    expect(document.body.textContent).not.toContain(newToken);

    await act(async () => {
      olderSearch.resolve(
        jsonResponse({ results: [{ id: 2, title: "Older search result" }] }),
      );
      await olderSearch.promise;
    });
    expect(screen.queryByText("Older search result")).toBeNull();
    expect(screen.getByText("New token result")).toBeTruthy();
  });

  it("preserves the active query, results, and page through navigation, appearance, and resize", async () => {
    const query = "Persistent query";
    const searchResults = Array.from({ length: 25 }, (_, index) => ({
      id: index + 100,
      title: `Persistent result ${index + 1}`,
    }));
    loadTmdbTokenMock.mockResolvedValue("fixture-token");
    fetchMock
      .mockResolvedValueOnce(
        jsonResponse({ results: [{ id: 1, title: "Trending result" }] }),
      )
      .mockResolvedValueOnce(jsonResponse({ results: searchResults }));

    render(<App />);
    selectDiscover();
    expect(await screen.findByText("Trending result")).toBeTruthy();
    submitDiscoverSearch(query);
    expect(await screen.findByText("Persistent result 1")).toBeTruthy();
    resizeGallery("discover", 1528, 472);
    expect(screen.getByText("Page 1 of 4")).toBeTruthy();
    fireEvent.click(
      screen.getByRole("button", {
        name: "Next TMDB Movies search results page",
      }),
    );
    expect(screen.getByText("Page 2 of 4")).toBeTruthy();

    selectSettings();
    fireEvent.click(screen.getByRole("radio", { name: "Dark" }));
    selectDashboard();
    selectDiscover();

    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(
      screen.getByRole("textbox", { name: "Search Movies" }),
    ).toHaveProperty("value", query);
    expect(screen.getByText("Page 2 of 4")).toBeTruthy();
    expect(document.documentElement.dataset.theme).toBe("dark");

    fireEvent(window, new Event("resize"));
    resizeGallery("discover", 1088, 956);
    expect(screen.getByText("Page 2 of 3")).toBeTruthy();
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it("loads exact ID-verified details from a trending card without prefetching or leaking the token", async () => {
    const token = "details-fixture-token";
    const summaryTitle = "映画  —  Selected Summary";
    const providerTitle = "映画  —  Director's “DETAILS” Cut!";
    const pendingDetails = createDeferred<Response>();
    const parentActivation = vi.fn();
    loadTmdbTokenMock.mockResolvedValue(token);
    fetchMock
      .mockResolvedValueOnce(
        jsonResponse({
          results: [
            { id: 201, title: summaryTitle },
            { id: 202, title: "Other trending Movie" },
          ],
        }),
      )
      .mockReturnValueOnce(pendingDetails.promise);

    render(
      <div onClick={parentActivation} onPointerDown={parentActivation}>
        <App />
      </div>,
    );
    selectDiscover();
    const summaryHeading = await screen.findByRole("heading", {
      level: 3,
      name: "映画 — Selected Summary",
    });
    const card = summaryHeading.closest("article") as HTMLElement;
    const detailsButton = within(card).getByRole("button", {
      name: "View details: 映画 — Selected Summary",
    });
    expect(detailsButton.getAttribute("aria-label")).toBe(
      `View details: ${summaryTitle}`,
    );
    expect(
      screen.getByRole("button", {
        name: "View details: Other trending Movie",
      }),
    ).toBeTruthy();
    expect(fetchMock).toHaveBeenCalledTimes(1);

    detailsButton.focus();
    expect(document.activeElement).toBe(detailsButton);
    parentActivation.mockClear();
    fireEvent.pointerDown(detailsButton);
    fireEvent.click(detailsButton);

    const dialog = await screen.findByRole("dialog");
    const loadingTitle = within(dialog).getByRole("heading", { level: 2 });
    expect(loadingTitle.textContent).toBe(summaryTitle);
    expect(
      within(dialog).getByRole("heading", {
        level: 3,
        name: "Loading Movie details",
      }),
    ).toBeTruthy();
    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(parentActivation).not.toHaveBeenCalled();

    const [detailsUrl, detailsOptions] = fetchMock.mock.calls[1];
    expect(detailsUrl).toBe("https://api.themoviedb.org/3/movie/201");
    expect(String(detailsUrl)).not.toContain(token);
    expect(detailsOptions?.headers).toEqual({
      Accept: "application/json",
      Authorization: `Bearer ${token}`,
    });
    expect(document.body.textContent).not.toContain(token);
    expect(clipboardWriteMock).not.toHaveBeenCalled();

    await act(async () => {
      pendingDetails.resolve(
        jsonResponse({
          id: 201,
          title: providerTitle,
          poster_path: "/verified-details.jpg",
          release_date: "2026-08-03",
          runtime: 143,
          genres: [{ name: "Drama" }, { name: "Science  Fiction" }],
          overview: "Exact  provider overview — punctuation preserved!",
        }),
      );
      await pendingDetails.promise;
    });

    const verifiedTitle = await within(dialog).findByRole("heading", {
      level: 2,
      name: "映画 — Director's “DETAILS” Cut!",
    });
    expect(verifiedTitle.textContent).toBe(providerTitle);
    expect(within(dialog).getByText("2026-08-03")).toBeTruthy();
    expect(within(dialog).getByText("143 minutes")).toBeTruthy();
    expect(within(dialog).getByText("Drama, Science Fiction").textContent).toBe(
      "Drama, Science  Fiction",
    );
    expect(
      within(dialog).getByText(
        "Exact provider overview — punctuation preserved!",
      ).textContent,
    ).toBe("Exact  provider overview — punctuation preserved!");
    const poster = dialog.querySelector(".movie-details__poster img");
    expect(poster).not.toBeNull();
    expect((poster as HTMLImageElement).getAttribute("src")).toBe(
      "https://image.tmdb.org/t/p/w500/verified-details.jpg",
    );
    expect(fetchYtsMovieReleasesMock).not.toHaveBeenCalled();
    fetchYtsMovieReleasesMock.mockResolvedValue(
      ytsMovieReleaseFixture({
        providerMovieId: "0",
        providerTitle: "",
        providerYear: "",
        releases: [],
        tmdbMovieId: "201",
        tmdbTitle: providerTitle,
      }),
    );
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Find releases" }),
    );
    expect(
      await screen.findByRole("heading", {
        name: "No verified YTS releases found",
      }),
    ).toBeTruthy();
    expect(fetchYtsMovieReleasesMock).toHaveBeenCalledWith({
      tmdbMovieId: 201,
    });
    expect(screen.queryByRole("button", { name: "Inspect torrent" })).toBeNull();
    const releaseDialog = screen
      .getByText("Verified YTS release comparison")
      .closest('[role="dialog"]');
    fireEvent.click(
      within(releaseDialog as HTMLElement).getByRole("button", {
        name: "Close",
      }),
    );

    fireEvent.click(within(dialog).getByRole("button", { name: "Close" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    await waitFor(() => expect(document.activeElement).toBe(detailsButton));
    expect(document.body.contains(summaryHeading)).toBe(true);
    expect(summaryHeading.textContent).toBe(summaryTitle);
    parentActivation.mockClear();

    const otherCard = screen
      .getByRole("heading", { level: 3, name: "Other trending Movie" })
      .closest("article") as HTMLElement;
    fireEvent.click(
      within(otherCard).getByRole("button", {
        name: "Copy title: Other trending Movie",
      }),
    );
    expect(clipboardWriteMock).toHaveBeenCalledWith("Other trending Movie");
    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(parentActivation).not.toHaveBeenCalled();
    expect(scanMoviesMock).not.toHaveBeenCalled();
    expect(queryMoviesStorageMock).not.toHaveBeenCalled();
    expect(openMovieMock).not.toHaveBeenCalled();
    expect(revealMovieMock).not.toHaveBeenCalled();
    expect(trashMovieMock).not.toHaveBeenCalled();
  });

  it("requests details by ID from a search card and preserves the submitted search after Close", async () => {
    loadTmdbTokenMock.mockResolvedValue("fixture-token");
    fetchMock
      .mockResolvedValueOnce(
        jsonResponse({ results: [{ id: 1, title: "Trending result" }] }),
      )
      .mockResolvedValueOnce(
        jsonResponse({ results: [{ id: 302, title: "Search card result" }] }),
      )
      .mockResolvedValueOnce(
        jsonResponse({
          id: 302,
          title: "Search card result",
          runtime: 98,
          genres: [{ name: "Comedy" }],
          overview: "Search-backed details.",
        }),
      );

    render(<App />);
    selectDiscover();
    expect(await screen.findByText("Trending result")).toBeTruthy();
    submitDiscoverSearch("Search query");
    expect(await screen.findByText("Search card result")).toBeTruthy();

    fireEvent.click(
      screen.getByRole("button", {
        name: "View details: Search card result",
      }),
    );
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText("Search-backed details.")).toBeTruthy();
    expect(fetchMock.mock.calls[2][0]).toBe(
      "https://api.themoviedb.org/3/movie/302",
    );

    fireEvent.click(within(dialog).getByRole("button", { name: "Close" }));
    expect(
      screen.getByRole("textbox", { name: "Search Movies" }),
    ).toHaveProperty("value", "Search query");
    expect(
      screen.getByRole("list", { name: "TMDB Movies search results" }),
    ).toBeTruthy();
    expect(screen.getByText("Search card result")).toBeTruthy();
    expect(fetchMock).toHaveBeenCalledTimes(3);
  });

  it("shows honest optional-field and poster fallbacks in verified details", async () => {
    loadTmdbTokenMock.mockResolvedValue("fixture-token");
    fetchMock
      .mockResolvedValueOnce(
        jsonResponse({ results: [{ id: 401, title: "Fallback details" }] }),
      )
      .mockResolvedValueOnce(
        jsonResponse({ id: 401, title: "Fallback details" }),
      );

    render(<App />);
    selectDiscover();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "View details: Fallback details",
      }),
    );

    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText("Poster unavailable")).toBeTruthy();
    expect(within(dialog).getAllByText("Unavailable")).toHaveLength(4);
  });

  it.each([
    {
      caseName: "unauthorized token",
      heading: "TMDB token was not accepted",
      outcome: jsonResponse({}, 401),
    },
    {
      caseName: "rate limit",
      heading: "TMDB details rate limit reached",
      outcome: jsonResponse({}, 429),
    },
    {
      caseName: "malformed identity",
      heading: "TMDB returned invalid Movie details",
      outcome: jsonResponse({ id: 999, title: "Wrong Movie" }),
    },
    {
      caseName: "general provider failure",
      heading: "TMDB could not load Movie details",
      outcome: jsonResponse({}, 500),
    },
  ])(
    "keeps the Discover result set behind the local details $caseName state",
    async ({ heading, outcome }) => {
      loadTmdbTokenMock.mockResolvedValue("fixture-token");
      fetchMock
        .mockResolvedValueOnce(
          jsonResponse({ results: [{ id: 501, title: "Stable result" }] }),
        )
        .mockResolvedValueOnce(outcome);

      render(<App />);
      selectDiscover();
      fireEvent.click(
        await screen.findByRole("button", {
          name: "View details: Stable result",
        }),
      );

      const dialog = await screen.findByRole("dialog");
      expect(
        await within(dialog).findByRole("heading", {
          level: 3,
          name: heading,
        }),
      ).toBeTruthy();
      expect(
        document.querySelector('[aria-label="Weekly trending Movies"]'),
      ).not.toBeNull();
      expect(fetchMock).toHaveBeenCalledTimes(2);
    },
  );

  it("shows a details network error without replacing Discover results", async () => {
    loadTmdbTokenMock.mockResolvedValue("fixture-token");
    fetchMock
      .mockResolvedValueOnce(
        jsonResponse({ results: [{ id: 502, title: "Network result" }] }),
      )
      .mockRejectedValueOnce(new TypeError("offline"));

    render(<App />);
    selectDiscover();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "View details: Network result",
      }),
    );

    expect(
      await within(await screen.findByRole("dialog")).findByRole("heading", {
        level: 3,
        name: "TMDB Movie details could not be reached",
      }),
    ).toBeTruthy();
    expect(document.querySelector('[aria-label="Weekly trending Movies"]'))
      .not.toBeNull();
  });

  it("keeps Movie B details when Movie A resolves late", async () => {
    const movieADetails = createDeferred<Response>();
    const movieBDetails = createDeferred<Response>();
    loadTmdbTokenMock.mockResolvedValue("fixture-token");
    fetchMock
      .mockResolvedValueOnce(
        jsonResponse({
          results: [
            { id: 601, title: "Movie A" },
            { id: 602, title: "Movie B" },
          ],
        }),
      )
      .mockReturnValueOnce(movieADetails.promise)
      .mockReturnValueOnce(movieBDetails.promise);

    render(<App />);
    selectDiscover();
    const movieAButton = await screen.findByRole("button", {
      name: "View details: Movie A",
    });
    const movieBButton = screen.getByRole("button", {
      name: "View details: Movie B",
    });

    fireEvent.click(movieAButton);
    expect(
      within(await screen.findByRole("dialog")).getByRole("heading", {
        level: 2,
        name: "Movie A",
      }),
    ).toBeTruthy();
    fireEvent.click(movieBButton);
    const movieBDialog = await screen.findByRole("dialog");
    expect(
      within(movieBDialog).getByRole("heading", {
        level: 2,
        name: "Movie B",
      }),
    ).toBeTruthy();
    expect(fetchMock.mock.calls[1][0]).toBe(
      "https://api.themoviedb.org/3/movie/601",
    );
    expect(fetchMock.mock.calls[2][0]).toBe(
      "https://api.themoviedb.org/3/movie/602",
    );

    await act(async () => {
      movieBDetails.resolve(
        jsonResponse({
          id: 602,
          title: "Movie B verified",
          overview: "Newest selected details.",
        }),
      );
      await movieBDetails.promise;
    });
    expect(await within(movieBDialog).findByText("Newest selected details."))
      .toBeTruthy();

    await act(async () => {
      movieADetails.resolve(
        jsonResponse({
          id: 601,
          title: "Movie A stale",
          overview: "Stale Movie A details.",
        }),
      );
      await movieADetails.promise;
    });
    expect(screen.queryByText("Stale Movie A details.")).toBeNull();
    expect(within(movieBDialog).getByText("Newest selected details."))
      .toBeTruthy();
  });

  it("invalidates pending details on explicit Close and Escape and restores trigger focus", async () => {
    const explicitlyClosedDetails = createDeferred<Response>();
    const escapedDetails = createDeferred<Response>();
    loadTmdbTokenMock.mockResolvedValue("fixture-token");
    fetchMock
      .mockResolvedValueOnce(
        jsonResponse({ results: [{ id: 701, title: "Dismiss details" }] }),
      )
      .mockReturnValueOnce(explicitlyClosedDetails.promise)
      .mockReturnValueOnce(escapedDetails.promise);

    render(<App />);
    selectDiscover();
    const detailsButton = await screen.findByRole("button", {
      name: "View details: Dismiss details",
    });

    detailsButton.focus();
    fireEvent.keyDown(detailsButton, { key: "Enter" });
    fireEvent.click(detailsButton);
    let dialog = await screen.findByRole("dialog");
    await waitFor(() =>
      expect(document.activeElement).toBe(
        within(dialog).getByRole("button", { name: "Close" }),
      ),
    );
    fireEvent.click(within(dialog).getByRole("button", { name: "Close" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    await waitFor(() => expect(document.activeElement).toBe(detailsButton));

    await act(async () => {
      explicitlyClosedDetails.resolve(
        jsonResponse({
          id: 701,
          title: "Late explicit close",
          overview: "Should not reopen.",
        }),
      );
      await explicitlyClosedDetails.promise;
    });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.queryByText("Should not reopen.")).toBeNull();

    fireEvent.click(detailsButton);
    dialog = await screen.findByRole("dialog");
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    await waitFor(() => expect(document.activeElement).toBe(detailsButton));

    await act(async () => {
      escapedDetails.resolve(
        jsonResponse({
          id: 701,
          title: "Late Escape",
          overview: "Should remain closed.",
        }),
      );
      await escapedDetails.promise;
    });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.queryByText("Should remain closed.")).toBeNull();
    expect(fetchMock).toHaveBeenCalledTimes(3);
  });

  it("invalidates pending details when the TMDB token is replaced or cleared", async () => {
    const oldToken = "old-details-token";
    const newToken = "new-details-token";
    const oldTokenDetails = createDeferred<Response>();
    const newTokenDetails = createDeferred<Response>();
    loadTmdbTokenMock.mockResolvedValue(oldToken);
    fetchMock
      .mockResolvedValueOnce(
        jsonResponse({ results: [{ id: 801, title: "Old token result" }] }),
      )
      .mockReturnValueOnce(oldTokenDetails.promise)
      .mockResolvedValueOnce(
        jsonResponse({ results: [{ id: 802, title: "New token result" }] }),
      )
      .mockReturnValueOnce(newTokenDetails.promise);

    render(<App />);
    selectDiscover();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "View details: Old token result",
      }),
    );
    await screen.findByRole("dialog");

    const settingsButton = Array.from(
      document.querySelectorAll<HTMLButtonElement>(".navigation-item"),
    ).find((button) => button.textContent?.trim() === "Settings");
    expect(settingsButton).not.toBeUndefined();
    fireEvent.click(settingsButton as HTMLButtonElement);
    const tokenInput = document.querySelector<HTMLInputElement>("#tmdb-token");
    expect(tokenInput).not.toBeNull();
    fireEvent.change(tokenInput as HTMLInputElement, {
      target: { value: newToken },
    });
    fireEvent.submit((tokenInput as HTMLInputElement).form as HTMLFormElement);

    expect(await screen.findByText("TMDB token replaced.")).toBeTruthy();
    expect(screen.queryByRole("dialog")).toBeNull();
    selectDiscover();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "View details: New token result",
      }),
    );
    await screen.findByRole("dialog");

    fireEvent.click(settingsButton as HTMLButtonElement);
    const clearTokenButton = Array.from(
      document.querySelectorAll<HTMLButtonElement>("button"),
    ).find((button) => button.textContent?.trim() === "Clear token");
    expect(clearTokenButton).not.toBeUndefined();
    fireEvent.click(clearTokenButton as HTMLButtonElement);
    expect(await screen.findByText("TMDB token cleared.")).toBeTruthy();
    expect(screen.queryByRole("dialog")).toBeNull();

    await act(async () => {
      oldTokenDetails.resolve(
        jsonResponse({
          id: 801,
          title: "Old token stale details",
          overview: oldToken,
        }),
      );
      newTokenDetails.resolve(
        jsonResponse({
          id: 802,
          title: "New token stale details",
          overview: newToken,
        }),
      );
      await Promise.all([oldTokenDetails.promise, newTokenDetails.promise]);
    });

    expect(screen.queryByText("Old token stale details")).toBeNull();
    expect(screen.queryByText("New token stale details")).toBeNull();
    expect(document.body.textContent).not.toContain(oldToken);
    expect(document.body.textContent).not.toContain(newToken);
    expect(String(fetchMock.mock.calls[1][0])).not.toContain(oldToken);
    expect(String(fetchMock.mock.calls[3][0])).not.toContain(newToken);
    expect(fetchMock.mock.calls[1][1]?.headers).toEqual({
      Accept: "application/json",
      Authorization: `Bearer ${oldToken}`,
    });
    expect(fetchMock.mock.calls[3][1]?.headers).toEqual({
      Accept: "application/json",
      Authorization: `Bearer ${newToken}`,
    });
  });

  it("preserves search results and responsive page through details, navigation, and appearance changes", async () => {
    const detailsResponse = createDeferred<Response>();
    const searchResults = Array.from({ length: 25 }, (_, index) => ({
      id: index + 901,
      title: `Details preservation ${index + 1}`,
    }));
    loadTmdbTokenMock.mockResolvedValue("fixture-token");
    fetchMock
      .mockResolvedValueOnce(
        jsonResponse({ results: [{ id: 1, title: "Trending result" }] }),
      )
      .mockResolvedValueOnce(jsonResponse({ results: searchResults }))
      .mockReturnValueOnce(detailsResponse.promise);

    render(<App />);
    selectDiscover();
    expect(await screen.findByText("Trending result")).toBeTruthy();
    submitDiscoverSearch("Preserved details query");
    expect(await screen.findByText("Details preservation 1")).toBeTruthy();
    resizeGallery("discover", 1528, 472);
    fireEvent.click(
      screen.getByRole("button", {
        name: "Next TMDB Movies search results page",
      }),
    );
    expect(screen.getByText("Page 2 of 4")).toBeTruthy();

    const searchResultsList = screen.getByRole("list", {
      name: "TMDB Movies search results",
    });
    const detailsButton = within(searchResultsList).getAllByRole("button", {
      name: /View details:/,
    })[0];
    const selectedTitle =
      detailsButton
        .getAttribute("aria-label")
        ?.replace("View details: ", "") ?? "";
    fireEvent.click(detailsButton);
    const dialog = await screen.findByRole("dialog");
    const detailsMovieId = Number(
      String(fetchMock.mock.calls[2][0]).split("/").at(-1),
    );

    const settingsButton = Array.from(
      document.querySelectorAll<HTMLButtonElement>(".navigation-item"),
    ).find((button) => button.textContent?.trim() === "Settings");
    fireEvent.click(settingsButton as HTMLButtonElement);
    const darkAppearance = document.querySelector<HTMLInputElement>(
      'input[name="appearance"][value="dark"]',
    );
    expect(darkAppearance).not.toBeNull();
    fireEvent.click(darkAppearance as HTMLInputElement);

    const discoverButton = Array.from(
      document.querySelectorAll<HTMLButtonElement>(".navigation-item"),
    ).find((button) => button.textContent?.trim() === "Discover");
    fireEvent.click(discoverButton as HTMLButtonElement);
    resizeGallery("discover", 1088, 956);

    await act(async () => {
      detailsResponse.resolve(
        jsonResponse({
          id: detailsMovieId,
          title: selectedTitle,
          overview: "Preserved verified details.",
        }),
      );
      await detailsResponse.promise;
    });
    expect(await within(dialog).findByText("Preserved verified details."))
      .toBeTruthy();
    fireEvent.click(within(dialog).getByRole("button", { name: "Close" }));

    expect(
      screen.getByRole("textbox", { name: "Search Movies" }),
    ).toHaveProperty("value", "Preserved details query");
    expect(screen.getByText("Page 2 of 3")).toBeTruthy();
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(screen.getByLabelText("TMDB credits")).toBeTruthy();
    expect(fetchMock).toHaveBeenCalledTimes(3);
    expect(scanMoviesMock).not.toHaveBeenCalled();
    expect(queryMoviesStorageMock).not.toHaveBeenCalled();
  });

  it("prevents pending results from returning after the token changes or clears", async () => {
    const oldTokenRequest = createDeferred<Response>();
    const pendingClearRequest = createDeferred<Response>();
    loadTmdbTokenMock.mockResolvedValue("old-fixture-token");
    fetchMock
      .mockReturnValueOnce(oldTokenRequest.promise)
      .mockResolvedValueOnce(
        jsonResponse({ results: [{ id: 2, title: "New token result" }] }),
      )
      .mockReturnValueOnce(pendingClearRequest.promise);

    render(<App />);
    selectDiscover();
    expect(
      await screen.findByRole("heading", {
        level: 2,
        name: "Loading weekly trending Movies",
      }),
    ).toBeTruthy();

    selectSettings();
    await screen.findByText("TMDB token configured on this device.");
    fireEvent.change(screen.getByLabelText("New token"), {
      target: { value: "new-fixture-token" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Replace token" }));
    await screen.findByText("TMDB token replaced.");

    selectDiscover();
    expect(await screen.findByText("New token result")).toBeTruthy();
    await act(async () => {
      oldTokenRequest.resolve(
        jsonResponse({ results: [{ id: 1, title: "Old token result" }] }),
      );
      await oldTokenRequest.promise;
    });
    expect(screen.queryByText("Old token result")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    selectSettings();
    fireEvent.click(screen.getByRole("button", { name: "Clear token" }));
    await screen.findByText("TMDB token cleared.");
    await act(async () => {
      pendingClearRequest.resolve(
        jsonResponse({ results: [{ id: 3, title: "Cleared token result" }] }),
      );
      await pendingClearRequest.promise;
    });

    selectDiscover();
    expect(
      await screen.findByRole("heading", {
        level: 2,
        name: "Configure TMDB to discover movies",
      }),
    ).toBeTruthy();
    expect(screen.queryByText("Cleared token result")).toBeNull();
  });
});

describe("YTS Movie release comparison", () => {
  it("requires explicit selection and inspects and saves only the exact verified artifact", async () => {
    const expectedInfohash = "0123456789abcdef0123456789abcdef01234567";
    const exactTmdbTitle = "Exact  Movie — 特別版";
    const exactProviderTitle = "YTS  Exact — 特別版";
    savedMoviesFolder = "/Movies";
    loadTmdbTokenMock.mockResolvedValue("fixture-token");
    fetchMock.mockResolvedValue(
      jsonResponse({
        results: [
          {
            id: 419,
            poster_path: null,
            release_date: "1999-04-19",
            title: exactTmdbTitle,
          },
        ],
      }),
    );
    fetchYtsMovieReleasesMock.mockResolvedValue(
      ytsMovieReleaseFixture({
        providerTitle: exactProviderTitle,
        releases: [
          {
            peers: "",
            quality: "720p",
            rowId: "700:incomplete",
            seeds: "",
            size: "Unavailable artifact",
            typeLabel: "web",
            videoCodec: "x264",
          },
          {
            expectedInfohash,
            peers: "11",
            quality: "1080p",
            rowId: "700:complete",
            seeds: "42",
            size: "1.25 GiB",
            sizeBytes: "1342177280",
            torrentUrl: `https://yts.mx/torrent/download/${expectedInfohash.toUpperCase()}`,
            typeLabel: "bluray",
            videoCodec: "x265",
          },
        ],
      }),
    );
    const inspectionResult = createDeferred<string[]>();
    inspectYtsMovieTorrentMock.mockReturnValue(inspectionResult.promise);

    render(<App />);
    selectDiscover();
    const releasesTrigger = await screen.findByRole("button", {
      name: "Find releases: Exact Movie — 特別版",
    });
    expect(fetchYtsMovieReleasesMock).not.toHaveBeenCalled();
    fireEvent.click(releasesTrigger);

    const releaseList = await screen.findByRole("list", {
      name: "Verified YTS torrents for Exact Movie — 特別版",
    });
    expect(fetchYtsMovieReleasesMock).toHaveBeenCalledWith({
      tmdbMovieId: 419,
    });
    expect(within(releaseList).getAllByRole("button")).toHaveLength(2);
    expect(screen.getByLabelText("Verified Movie release totals").textContent).toBe(
      "2 verified torrentsIMDb tt0123456Retry",
    );
    expect(
      screen.getByText("YTS Movie").parentElement?.querySelector("dd")
        ?.textContent,
    ).toBe(exactProviderTitle);
    expect(
      screen.getByText("Select one verified torrent row to inspect its metadata."),
    ).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Inspect torrent" })).toBeNull();

    const incompleteRelease = within(releaseList).getByRole("button", {
      name: /720p/,
    });
    fireEvent.click(incompleteRelease);
    expect(
      screen.getByText(/no complete safe YTS artifact identity/),
    ).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Inspect torrent" })).toBeNull();

    const completeRelease = within(releaseList).getByRole("button", {
      name: /1080p/,
    });
    fireEvent.click(completeRelease);
    const inspectButton = screen.getByRole("button", {
      name: "Inspect torrent",
    });
    fireEvent.click(inspectButton);
    expect(
      await screen.findByRole("heading", {
        name: "Inspecting verified Movie torrent",
      }),
    ).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Save `.torrent`" })).toBeNull();
    expect(screen.getByLabelText("Selected YTS torrent metadata").textContent).toBe(
      "SourceYTSQuality1080pTypeblurayCodecx265Provider size1.25 GiBSeeds42Peers11",
    );
    expect(inspectYtsMovieTorrentMock).toHaveBeenCalledWith({
      expectedInfohash,
      imdbId: "tt0123456",
      peers: "11",
      providerMovieId: 700,
      providerTitle: exactProviderTitle,
      providerYear: "1999",
      quality: "1080p",
      releaseDate: "1999-04-19",
      rowId: "700:complete",
      seeds: "42",
      size: "1.25 GiB",
      sizeBytes: "1342177280",
      tmdbMovieId: 419,
      tmdbTitle: exactTmdbTitle,
      torrentUrl: `https://yts.mx/torrent/download/${expectedInfohash.toUpperCase()}`,
      typeLabel: "bluray",
      videoCodec: "x265",
    });

    await act(async () => {
      inspectionResult.resolve([
        "movie-419-700-hash",
        "Movie  —  Exact  Torrent",
        expectedInfohash,
        "12",
        "Folder/Part  1 — 映画.mkv",
        "5",
        "Folder/特別版  B.mp4",
        "7",
      ]);
      await inspectionResult.promise;
    });

    expect(
      screen.getByText("Torrent name").parentElement?.querySelector("dd")
        ?.textContent,
    ).toBe("Movie  —  Exact  Torrent");
    expect(screen.getByText(expectedInfohash)).toBeTruthy();
    const fileRows = within(
      screen.getByRole("list", {
        name: "Files in verified Movie torrent for Exact Movie — 特別版",
      }),
    ).getAllByRole("listitem");
    expect(fileRows).toHaveLength(2);
    expect(fileRows[0].textContent).toContain("Folder/Part  1 — 映画.mkv");
    expect(fileRows[1].textContent).toContain("Folder/特別版  B.mp4");
    const fileSelection = screen.getAllByRole("checkbox");
    expect(fileSelection).toHaveLength(2);
    expect(fileSelection.every((checkbox) => !(checkbox as HTMLInputElement).checked)).toBe(
      true,
    );
    const startButton = screen.getByRole("button", { name: "Start download" });
    expect((startButton as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(fileSelection[1]);
    expect((startButton as HTMLButtonElement).disabled).toBe(false);

    const startRequest = createDeferred<string>();
    startVerifiedMovieDownloadMock.mockReturnValue(startRequest.promise);
    fireEvent.click(startButton);
    fireEvent.click(startButton);
    expect(startVerifiedMovieDownloadMock).toHaveBeenCalledOnce();
    expect(startVerifiedMovieDownloadMock).toHaveBeenCalledWith({
      inspectionId: "movie-419-700-hash",
      selectedFileIds: [1],
    });
    expect(startVerifiedAdultDownloadMock).not.toHaveBeenCalled();
    expect(startVerifiedVrDownloadMock).not.toHaveBeenCalled();
    await act(async () => {
      startRequest.resolve("movie-transfer-419");
      await startRequest.promise;
    });
    expect(
      await screen.findByText("Selected Movie files were added to Downloads."),
    ).toBeTruthy();

    const cancelledSave = createDeferred<boolean>();
    saveVerifiedMovieTorrentMock
      .mockReturnValueOnce(cancelledSave.promise)
      .mockResolvedValueOnce(true);
    const saveButton = screen.getByRole("button", { name: "Save `.torrent`" });
    fireEvent.click(saveButton);
    fireEvent.click(saveButton);
    expect(saveVerifiedMovieTorrentMock).toHaveBeenCalledOnce();
    await act(async () => {
      cancelledSave.resolve(false);
      await cancelledSave.promise;
    });
    expect(screen.queryByText("Verified Movie torrent file saved.")).toBeNull();
    fireEvent.click(saveButton);
    expect(
      await screen.findByText("Verified Movie torrent file saved."),
    ).toBeTruthy();
    expect(saveVerifiedMovieTorrentMock).toHaveBeenLastCalledWith({
      inspectionId: "movie-419-700-hash",
    });

    const inspectionDialog = screen
      .getByText("Verified YTS torrent")
      .closest('[role="dialog"]');
    fireEvent.click(
      within(inspectionDialog as HTMLElement).getByRole("button", {
        name: "Close",
      }),
    );
    await waitFor(() => expect(document.activeElement).toBe(inspectButton));
    expect(fetchSukebeiAdultReleasesMock).not.toHaveBeenCalled();
    expect(fetchSukebeiVrReleasesMock).not.toHaveBeenCalled();
    expect(startVerifiedAdultDownloadMock).not.toHaveBeenCalled();
    expect(startVerifiedVrDownloadMock).not.toHaveBeenCalled();
  });

  it("ignores late Movie inspection and Save results after dismissal", async () => {
    const expectedInfohash = "0123456789abcdef0123456789abcdef01234567";
    loadTmdbTokenMock.mockResolvedValue("fixture-token");
    fetchMock.mockResolvedValue(
      jsonResponse({ results: [{ id: 419, title: "Exact Movie" }] }),
    );
    fetchYtsMovieReleasesMock.mockResolvedValue(
      ytsMovieReleaseFixture({
        releases: [
          {
            expectedInfohash,
            quality: "1080p",
            rowId: "700:complete",
            torrentUrl: `https://yts.mx/torrent/download/${expectedInfohash}`,
          },
        ],
        tmdbTitle: "Exact Movie",
      }),
    );
    const lateInspection = createDeferred<string[]>();
    inspectYtsMovieTorrentMock
      .mockReturnValueOnce(lateInspection.promise)
      .mockResolvedValueOnce([
        "movie-current",
        "Current torrent",
        expectedInfohash,
        "5",
        "Current file.mp4",
        "5",
      ]);

    render(<App />);
    selectDiscover();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Find releases: Exact Movie",
      }),
    );
    const releaseList = await screen.findByRole("list", {
      name: "Verified YTS torrents for Exact Movie",
    });
    fireEvent.click(within(releaseList).getByRole("button"));
    const inspectButton = screen.getByRole("button", {
      name: "Inspect torrent",
    });
    fireEvent.click(inspectButton);
    await screen.findByRole("heading", {
      name: "Inspecting verified Movie torrent",
    });
    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => expect(document.activeElement).toBe(inspectButton));

    await act(async () => {
      lateInspection.resolve([
        "movie-late",
        "Late closed torrent",
        expectedInfohash,
        "5",
        "Late closed file.mp4",
        "5",
      ]);
      await lateInspection.promise;
    });
    expect(screen.queryByText("Late closed torrent")).toBeNull();
    expect(screen.queryByText("Late closed file.mp4")).toBeNull();

    fireEvent.click(inspectButton);
    expect(await screen.findByText("Current torrent")).toBeTruthy();
    const lateSave = createDeferred<boolean>();
    saveVerifiedMovieTorrentMock.mockReturnValue(lateSave.promise);
    fireEvent.click(screen.getByRole("button", { name: "Save `.torrent`" }));
    const inspectionDialog = screen
      .getByText("Verified YTS torrent")
      .closest('[role="dialog"]');
    fireEvent.click(
      within(inspectionDialog as HTMLElement).getByRole("button", {
        name: "Close",
      }),
    );
    await act(async () => {
      lateSave.resolve(true);
      await lateSave.promise;
    });
    expect(screen.queryByText("Verified Movie torrent file saved.")).toBeNull();
    expect(invalidateVerifiedMovieTorrentMock).toHaveBeenCalled();
  });

  it("keeps every Movie inspection failure local, distinct, and retryable", async () => {
    const expectedInfohash = "0123456789abcdef0123456789abcdef01234567";
    loadTmdbTokenMock.mockResolvedValue("fixture-token");
    fetchMock.mockResolvedValue(
      jsonResponse({ results: [{ id: 419, title: "Exact Movie" }] }),
    );
    fetchYtsMovieReleasesMock.mockResolvedValue(
      ytsMovieReleaseFixture({
        releases: [
          {
            expectedInfohash,
            quality: "1080p",
            rowId: "700:complete",
            torrentUrl: `https://yts.mx/torrent/download/${expectedInfohash}`,
          },
        ],
        tmdbTitle: "Exact Movie",
      }),
    );
    for (const error of [
      "movie_torrent_source_unavailable",
      "movie_torrent_network_error",
      "movie_torrent_provider_error",
      "movie_torrent_malformed",
      "movie_torrent_unsupported",
      "movie_torrent_infohash_mismatch",
      "movie_torrent_context_invalid",
      "unexpected_error",
    ]) {
      inspectYtsMovieTorrentMock.mockRejectedValueOnce(error);
    }

    render(<App />);
    selectDiscover();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Find releases: Exact Movie",
      }),
    );
    const releaseList = await screen.findByRole("list", {
      name: "Verified YTS torrents for Exact Movie",
    });
    fireEvent.click(within(releaseList).getByRole("button"));
    fireEvent.click(screen.getByRole("button", { name: "Inspect torrent" }));

    for (const heading of [
      "Torrent artifact is unavailable",
      "Torrent artifact could not be reached",
      "Torrent provider rejected the request",
      "Torrent artifact is malformed",
      "Torrent artifact is unsupported",
      "Torrent identity did not match",
      "Torrent inspection is no longer current",
      "Torrent inspection could not be completed",
    ]) {
      expect(await screen.findByRole("heading", { name: heading })).toBeTruthy();
      expect(screen.queryByRole("button", { name: "Save `.torrent`" })).toBeNull();
      if (heading !== "Torrent inspection could not be completed") {
        fireEvent.click(
          screen.getByRole("button", { name: "Retry inspection" }),
        );
      }
    }
    expect(within(releaseList).getByText("1080p")).toBeTruthy();
  });

  it("ignores a dismissed late Movie result and restores the current completed selection", async () => {
    const staleResult = createDeferred<string[]>();
    const currentTitle = "Current  Movie — B";
    loadTmdbTokenMock.mockResolvedValue("fixture-token");
    fetchMock.mockResolvedValue(
      jsonResponse({
        results: [
          { id: 419, title: "Stale Movie A", release_date: "1999-04-19" },
          { id: 420, title: currentTitle, release_date: "1999-04-19" },
        ],
      }),
    );
    fetchYtsMovieReleasesMock.mockImplementation((parameters) =>
      parameters?.tmdbMovieId === 419
        ? staleResult.promise
        : Promise.resolve(
            ytsMovieReleaseFixture({
              providerTitle: "Current YTS Movie B",
              releases: [{ quality: "1080p", rowId: "701:current" }],
              tmdbMovieId: "420",
              tmdbTitle: currentTitle,
            }),
          ),
    );

    render(<App />);
    selectDiscover();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Find releases: Stale Movie A",
      }),
    );
    const loadingDialog = screen
      .getByRole("heading", { name: "Finding verified Movie releases" })
      .closest('[role="dialog"]');
    fireEvent.click(
      within(loadingDialog as HTMLElement).getByRole("button", { name: "Close" }),
    );
    expect(invalidateMovieReleaseContextMock).toHaveBeenCalledOnce();

    fireEvent.click(
      await screen.findByRole("button", {
        name: "Find releases: Current Movie — B",
      }),
    );
    let currentList = await screen.findByRole("list", {
      name: "Verified YTS torrents for Current Movie — B",
    });
    const selectedRow = within(currentList).getByRole("button", {
      name: /1080p/,
    });
    fireEvent.click(selectedRow);
    expect(selectedRow.getAttribute("aria-pressed")).toBe("true");

    await act(async () => {
      staleResult.resolve(
        ytsMovieReleaseFixture({
          providerTitle: "Stale provider response",
          releases: [{ quality: "2160p", rowId: "700:stale" }],
        }),
      );
      await staleResult.promise;
    });
    expect(screen.queryByText("Stale provider response")).toBeNull();
    expect(screen.getAllByText("Current YTS Movie B")).toHaveLength(2);

    const comparisonDialog = screen
      .getByText("Verified YTS release comparison")
      .closest('[role="dialog"]');
    fireEvent.click(
      within(comparisonDialog as HTMLElement).getByRole("button", {
        name: "Close",
      }),
    );
    selectSettings();
    fireEvent.click(screen.getByRole("radio", { name: "Dark" }));
    selectDashboard();
    selectDiscover();
    resizeGallery("discover", 1088, 956);
    fireEvent.click(
      screen.getByRole("button", {
        name: "Find releases: Current Movie — B",
      }),
    );
    currentList = await screen.findByRole("list", {
      name: "Verified YTS torrents for Current Movie — B",
    });
    expect(within(currentList).getByRole("button").getAttribute("aria-pressed")).toBe(
      "true",
    );
    expect(fetchYtsMovieReleasesMock).toHaveBeenCalledTimes(2);
    expect(invalidateMovieReleaseContextMock).toHaveBeenCalledOnce();
    expect(document.documentElement.dataset.theme).toBe("dark");
  });

  it.each([
    ["movie_tmdb_unauthorized", "TMDB token was not accepted"],
    ["movie_tmdb_rate_limited", "TMDB release lookup is rate-limited"],
    ["movie_tmdb_network_error", "TMDB could not be reached"],
    ["movie_tmdb_malformed", "TMDB returned invalid identity data"],
    ["movie_no_imdb_identity", "No IMDb identity is available"],
    ["movie_yts_source_unavailable", "YTS is unavailable"],
    ["movie_yts_network_error", "YTS could not be reached"],
    ["movie_yts_malformed", "YTS returned invalid release data"],
    ["movie_yts_conflicting_provider", "YTS returned conflicting Movie identities"],
    ["movie_yts_provider_error", "YTS could not load Movie releases"],
    ["unexpected_error", "TMDB could not resolve the Movie identity"],
  ])("shows %s as the distinct retryable state %s", async (error, heading) => {
    loadTmdbTokenMock.mockResolvedValue("fixture-token");
    fetchMock.mockResolvedValue(
      jsonResponse({ results: [{ id: 419, title: "Exact Movie" }] }),
    );
    fetchYtsMovieReleasesMock.mockRejectedValue(error);

    render(<App />);
    selectDiscover();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Find releases: Exact Movie",
      }),
    );

    expect(await screen.findByRole("heading", { name: heading })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Retry" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Inspect torrent" })).toBeNull();
  });
});

describe("TMDB TV Discover", () => {
  it("loads TV trends only after activation and renders exact accessible cards without native side effects", async () => {
    const token = "tv-fixture-token";
    const exactName = "番組  —  Director's “Cut”!";
    loadTmdbTokenMock.mockResolvedValue(token);
    fetchMock.mockImplementation((request) => {
      const url = String(request);
      if (url.endsWith("/trending/movie/week")) {
        return Promise.resolve(
          jsonResponse({ results: [{ id: 1, title: "Movie result" }] }),
        );
      }
      return Promise.resolve(
        jsonResponse({
          results: [
            {
              id: 51,
              name: exactName,
              poster_path: "/tv-poster.jpg",
              first_air_date: "2026-08-05",
            },
            { id: 52, name: "Posterless TV" },
            { id: "invalid", name: "Invalid TV" },
          ],
        }),
      );
    });

    render(<App />);
    await waitFor(() => expect(loadTmdbTokenMock).toHaveBeenCalledTimes(1));
    expect(fetchMock).not.toHaveBeenCalled();

    selectDiscover();
    expect(await screen.findByText("Movie result")).toBeTruthy();
    expect(
      fetchMock.mock.calls.some(([request]) =>
        String(request).includes("/trending/tv/week"),
      ),
    ).toBe(false);

    selectTvDiscover();
    const exactHeading = await screen.findByRole("heading", {
      level: 3,
      name: "番組 — Director's “Cut”!",
    });
    expect(exactHeading.textContent).toBe(exactName);
    expect(screen.getAllByRole("article")).toHaveLength(2);
    expect(screen.getByText("2026-08-05")).toBeTruthy();
    expect(screen.getByText("Poster unavailable")).toBeTruthy();
    expect(screen.getByRole("list", { name: "Weekly trending TV" })).toBeTruthy();
    expect(screen.getByLabelText("TMDB credits")).toBeTruthy();

    const exactCard = exactHeading.closest("article") as HTMLElement;
    const copyButton = within(exactCard).getByRole("button", {
      name: /Copy title:/,
    });
    expect(copyButton.getAttribute("aria-label")).toBe(`Copy title: ${exactName}`);
    fireEvent.click(copyButton);
    expect(clipboardWriteMock).toHaveBeenCalledWith(exactName);
    expect(scanMoviesMock).not.toHaveBeenCalled();
    expect(queryMoviesStorageMock).not.toHaveBeenCalled();
    expect(openTvFileMock).not.toHaveBeenCalled();
    expect(revealTvFileMock).not.toHaveBeenCalled();
    expect(startVerifiedVrDownloadMock).not.toHaveBeenCalled();

    const tvRequest = fetchMock.mock.calls.find(([request]) =>
      String(request).includes("/trending/tv/week"),
    );
    expect(tvRequest?.[1]?.headers).toEqual({
      Accept: "application/json",
      Authorization: `Bearer ${token}`,
    });
    expect(String(tvRequest?.[0])).not.toContain(token);
    expect(document.body.textContent).not.toContain(token);
  });

  it("submits exact TV queries explicitly, keeps edits separate, and clears a pending search to cached trends", async () => {
    const query = "  番組 — Director's “Cut”! & CAPS  ";
    const pendingSearch = createDeferred<Response>();
    loadTmdbTokenMock.mockResolvedValue("fixture-token");
    fetchMock
      .mockResolvedValueOnce(
        jsonResponse({ results: [{ id: 1, title: "Movie result" }] }),
      )
      .mockResolvedValueOnce(
        jsonResponse({ results: [{ id: 2, name: "Cached TV trend" }] }),
      )
      .mockReturnValueOnce(pendingSearch.promise);

    render(<App />);
    selectDiscover();
    expect(await screen.findByText("Movie result")).toBeTruthy();
    selectTvDiscover();
    expect(await screen.findByText("Cached TV trend")).toBeTruthy();

    const input = screen.getByRole("textbox", { name: "Search TV" });
    fireEvent.change(input, { target: { value: query } });
    expect(fetchMock).toHaveBeenCalledTimes(2);
    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    expect(
      await screen.findByRole("heading", {
        level: 2,
        name: "Searching TMDB TV",
      }),
    ).toBeTruthy();
    const searchUrl = new URL(String(fetchMock.mock.calls[2][0]));
    expect(searchUrl.pathname).toBe("/3/search/tv");
    expect(searchUrl.searchParams.get("query")).toBe(query);

    fireEvent.change(input, { target: { value: "Unsubmitted edit" } });
    expect(screen.getByText(/Results for/).textContent).toContain(query);
    fireEvent.click(screen.getByRole("button", { name: "Clear" }));
    expect(screen.getByText("Cached TV trend")).toBeTruthy();
    expect(input).toHaveProperty("value", "");

    await act(async () => {
      pendingSearch.resolve(
        jsonResponse({ results: [{ id: 3, name: "Late search result" }] }),
      );
      await pendingSearch.promise;
    });
    expect(screen.queryByText("Late search result")).toBeNull();
    expect(screen.getByText("Cached TV trend")).toBeTruthy();

    submitTvDiscoverSearch(" \t ");
    expect(screen.getByRole("alert").textContent).toBe(
      "Enter a TV title to search TMDB.",
    );
    expect(screen.getByText("Cached TV trend")).toBeTruthy();
    expect(fetchMock).toHaveBeenCalledTimes(3);
  });

  it.each([
    {
      heading: "No trending TV returned",
      response: jsonResponse({ results: [] }),
    },
    {
      heading: "TMDB token was not accepted",
      response: jsonResponse({}, 401),
    },
    {
      heading: "TMDB rate limit reached",
      response: jsonResponse({}, 429),
    },
    {
      heading: "TMDB could not load trending TV",
      response: jsonResponse({}, 500),
    },
    {
      heading: "TMDB returned invalid TV data",
      response: jsonResponse({ page: 1 }),
    },
  ])("shows the local TV provider state $heading", async ({ heading, response }) => {
    loadTmdbTokenMock.mockResolvedValue("fixture-token");
    fetchMock
      .mockResolvedValueOnce(jsonResponse({ results: [] }))
      .mockResolvedValueOnce(response);

    render(<App />);
    selectDiscover();
    await screen.findByRole("heading", { name: "No trending movies returned" });
    selectTvDiscover();

    expect(
      await screen.findByRole("heading", { level: 2, name: heading }),
    ).toBeTruthy();
  });

  it("shows TV network and search provider errors without changing Movies state", async () => {
    loadTmdbTokenMock.mockResolvedValue("fixture-token");
    fetchMock
      .mockResolvedValueOnce(
        jsonResponse({ results: [{ id: 1, title: "Stable Movie" }] }),
      )
      .mockRejectedValueOnce(new TypeError("offline"))
      .mockResolvedValueOnce(
        jsonResponse({ results: [{ id: 2, name: "TV after retry" }] }),
      )
      .mockResolvedValueOnce(jsonResponse({ page: 1 }));

    render(<App />);
    selectDiscover();
    expect(await screen.findByText("Stable Movie")).toBeTruthy();
    selectTvDiscover();
    expect(
      await screen.findByRole("heading", { name: "TMDB could not be reached" }),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(await screen.findByText("TV after retry")).toBeTruthy();
    submitTvDiscoverSearch("Fixture query");
    expect(
      await screen.findByRole("heading", {
        name: "TMDB returned invalid search data",
      }),
    ).toBeTruthy();

    fireEvent.click(screen.getByRole("radio", { name: "Movies" }));
    expect(screen.getByText("Stable Movie")).toBeTruthy();
    expect(fetchMock).toHaveBeenCalledTimes(4);
  });

  it("keeps only the newest TV refresh result", async () => {
    const earlierRefresh = createDeferred<Response>();
    const latestRefresh = createDeferred<Response>();
    loadTmdbTokenMock.mockResolvedValue("fixture-token");
    fetchMock
      .mockResolvedValueOnce(jsonResponse({ results: [] }))
      .mockResolvedValueOnce(
        jsonResponse({ results: [{ id: 1, name: "Initial TV" }] }),
      )
      .mockReturnValueOnce(earlierRefresh.promise)
      .mockReturnValueOnce(latestRefresh.promise);

    render(<App />);
    selectDiscover();
    await screen.findByRole("heading", { name: "No trending movies returned" });
    selectTvDiscover();
    expect(await screen.findByText("Initial TV")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));

    await act(async () => {
      latestRefresh.resolve(
        jsonResponse({ results: [{ id: 3, name: "Latest TV" }] }),
      );
      await latestRefresh.promise;
    });
    expect(await screen.findByText("Latest TV")).toBeTruthy();

    await act(async () => {
      earlierRefresh.resolve(
        jsonResponse({ results: [{ id: 2, name: "Stale TV" }] }),
      );
      await earlierRefresh.promise;
    });
    expect(screen.queryByText("Stale TV")).toBeNull();
    expect(screen.getByText("Latest TV")).toBeTruthy();
  });

  it("invalidates pending TV data when the token is replaced or cleared", async () => {
    const oldToken = "old-tv-token";
    const newToken = "new-tv-token";
    const oldTokenRefresh = createDeferred<Response>();
    let oldTvRequestCount = 0;
    loadTmdbTokenMock.mockResolvedValue(oldToken);
    fetchMock.mockImplementation((request, options) => {
      const url = String(request);
      const authorization =
        options === undefined
          ? undefined
          : (options.headers as Record<string, string>).Authorization;
      if (url.includes("/trending/movie/week")) {
        return Promise.resolve(jsonResponse({ results: [] }));
      }
      if (authorization === `Bearer ${oldToken}`) {
        oldTvRequestCount += 1;
        return oldTvRequestCount === 1
          ? Promise.resolve(
              jsonResponse({ results: [{ id: 1, name: "Old token TV" }] }),
            )
          : oldTokenRefresh.promise;
      }
      return Promise.resolve(
        jsonResponse({ results: [{ id: 2, name: "New token TV" }] }),
      );
    });

    render(<App />);
    selectDiscover();
    await screen.findByRole("heading", { name: "No trending movies returned" });
    selectTvDiscover();
    expect(await screen.findByText("Old token TV")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));

    selectSettings();
    fireEvent.change(screen.getByLabelText("New token"), {
      target: { value: newToken },
    });
    fireEvent.click(screen.getByRole("button", { name: "Replace token" }));
    expect(await screen.findByText("TMDB token replaced.")).toBeTruthy();
    selectDiscover();
    expect(await screen.findByText("New token TV")).toBeTruthy();

    await act(async () => {
      oldTokenRefresh.resolve(
        jsonResponse({ results: [{ id: 3, name: "Stale old-token TV" }] }),
      );
      await oldTokenRefresh.promise;
    });
    expect(screen.queryByText("Stale old-token TV")).toBeNull();

    selectSettings();
    fireEvent.click(screen.getByRole("button", { name: "Clear token" }));
    expect(await screen.findByText("TMDB token cleared.")).toBeTruthy();
    selectDiscover();
    expect(
      await screen.findByRole("heading", {
        name: "Configure TMDB to discover TV",
      }),
    ).toBeTruthy();
    expect(screen.queryByText("New token TV")).toBeNull();
    expect(document.body.textContent).not.toContain(oldToken);
    expect(document.body.textContent).not.toContain(newToken);
    for (const [request] of fetchMock.mock.calls) {
      expect(String(request)).not.toContain(oldToken);
      expect(String(request)).not.toContain(newToken);
    }
  });

  it("loads exact ID-verified TV details with truthful metadata and restores focus on Close", async () => {
    const summaryName = "番組  —  Summary";
    const detailsName = "番組  —  Exact “DETAILS”!";
    const pendingDetails = createDeferred<Response>();
    loadTmdbTokenMock.mockResolvedValue("details-token");
    fetchMock
      .mockResolvedValueOnce(jsonResponse({ results: [] }))
      .mockResolvedValueOnce(
        jsonResponse({
          results: [{ id: 601, name: summaryName, poster_path: null }],
        }),
      )
      .mockReturnValueOnce(pendingDetails.promise);

    render(<App />);
    selectDiscover();
    await screen.findByRole("heading", { name: "No trending movies returned" });
    selectTvDiscover();
    const summaryHeading = await screen.findByRole("heading", {
      level: 3,
      name: "番組 — Summary",
    });
    const detailsButton = within(
      summaryHeading.closest("article") as HTMLElement,
    ).getByRole("button", { name: /View details:/ });
    expect(detailsButton.getAttribute("aria-label")).toBe(
      `View details: ${summaryName}`,
    );
    expect(fetchMock).toHaveBeenCalledTimes(2);
    detailsButton.focus();
    fireEvent.click(detailsButton);
    const dialog = await screen.findByRole("dialog");
    expect(fetchMock.mock.calls[2][0]).toBe(
      "https://api.themoviedb.org/3/tv/601",
    );

    await act(async () => {
      pendingDetails.resolve(
        jsonResponse({
          id: 601,
          name: detailsName,
          poster_path: "/details-tv.jpg",
          first_air_date: "2026-08-05",
          status: "Returning  Series",
          number_of_seasons: 3,
          number_of_episodes: 28,
          genres: [{ name: "ドラマ" }, { name: "Science  Fiction" }],
          overview: "Exact  overview — punctuation preserved!",
        }),
      );
      await pendingDetails.promise;
    });

    expect(
      within(dialog).getByRole("heading", { level: 2 }).textContent,
    ).toBe(detailsName);
    expect(within(dialog).getByText("2026-08-05")).toBeTruthy();
    expect(within(dialog).getByText("Returning Series").textContent).toBe(
      "Returning  Series",
    );
    expect(within(dialog).getByText("3")).toBeTruthy();
    expect(within(dialog).getByText("28")).toBeTruthy();
    expect(within(dialog).getByText("ドラマ, Science Fiction").textContent).toBe(
      "ドラマ, Science  Fiction",
    );
    expect(
      within(dialog).getByText("Exact overview — punctuation preserved!")
        .textContent,
    ).toBe("Exact  overview — punctuation preserved!");

    fireEvent.click(within(dialog).getByRole("button", { name: "Close" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    await waitFor(() => expect(document.activeElement).toBe(detailsButton));
    expect(document.body.contains(summaryHeading)).toBe(true);
    expect(summaryHeading.textContent).toBe(summaryName);
  });

  it("loads only an explicitly selected exact season and preserves the completed guide across UI changes", async () => {
    const showName = "Exact  Show — 特別版";
    const seasonName = "第二期  —  Director's “Cut”!";
    const episodeName = "第一話  —  The  Beginning!";
    loadTmdbTokenMock.mockResolvedValue("season-guide-token");
    fetchMock
      .mockResolvedValueOnce(jsonResponse({ results: [] }))
      .mockResolvedValueOnce(
        jsonResponse({ results: [{ id: 701, name: showName }] }),
      )
      .mockResolvedValueOnce(
        jsonResponse({
          id: 701,
          name: showName,
          seasons: [
            {
              id: 9000,
              season_number: 1,
              name: "Season  One",
              air_date: "2025-01-01",
              poster_path: "/season-one.jpg",
              episode_count: 10,
            },
            {
              id: 9001,
              season_number: 2,
              name: seasonName,
              air_date: "2026-01-02",
              poster_path: "/season-two.jpg",
              episode_count: 2,
            },
            { id: 8999, season_number: 0, name: "Specials" },
            { id: 0, season_number: 3, name: "Invalid identity" },
          ],
        }),
      )
      .mockResolvedValueOnce(
        jsonResponse({
          id: 9001,
          season_number: 2,
          episodes: [
            {
              id: 9101,
              season_number: 2,
              episode_number: 1,
              name: episodeName,
              air_date: "2026-01-02",
              runtime: 47,
              overview: "Exact  overview — punctuation preserved!",
              still_path: "/episode-one.jpg",
            },
            {
              id: 9102,
              season_number: 2,
              episode_number: 2,
              name: "第二話: CAPS & punctuation",
            },
          ],
        }),
      )
      .mockResolvedValueOnce(
        jsonResponse({
          id: 701,
          name: showName,
          seasons: [
            { id: 9003, season_number: 3, name: "Replacement Season" },
          ],
        }),
      );

    render(<App />);
    selectDiscover();
    await screen.findByRole("heading", { name: "No trending movies returned" });
    selectTvDiscover();
    const detailsButton = await screen.findByRole("button", {
      name: /View details: Exact Show — 特別版/,
    });
    expect(detailsButton.getAttribute("aria-label")).toBe(
      `View details: ${showName}`,
    );
    fireEvent.click(detailsButton);
    let dialog = await screen.findByRole("dialog");
    await within(dialog).findByRole("button", { name: "View seasons" });
    expect(fetchMock).toHaveBeenCalledTimes(3);

    fireEvent.click(within(dialog).getByRole("button", { name: "View seasons" }));
    expect(fetchMock).toHaveBeenCalledTimes(3);
    expect(
      within(dialog).getByText("2 verified seasons"),
    ).toBeTruthy();
    expect(within(dialog).queryByText("Specials")).toBeNull();
    expect(
      within(dialog).queryByRole("button", { name: "Select Season 0" }),
    ).toBeNull();
    const exactSeasonHeading = within(dialog).getByRole("heading", {
      level: 4,
      name: "第二期 — Director's “Cut”!",
    });
    expect(exactSeasonHeading.textContent).toBe(seasonName);

    fireEvent.click(
      within(dialog).getByRole("button", { name: "Select Season 2" }),
    );
    expect(fetchMock.mock.calls[3][0]).toBe(
      "https://api.themoviedb.org/3/tv/701/season/2",
    );
    expect(fetchMock.mock.calls[3][1]?.headers).toEqual({
      Accept: "application/json",
      Authorization: "Bearer season-guide-token",
    });
    const exactEpisodeHeading = await within(dialog).findByRole("heading", {
      level: 5,
      name: "第一話 — The Beginning!",
    });
    expect(exactEpisodeHeading.textContent).toBe(episodeName);
    expect(within(dialog).getByText("2 verified episodes")).toBeTruthy();
    expect(
      within(dialog).getByText("Exact overview — punctuation preserved!")
        .textContent,
    ).toBe("Exact  overview — punctuation preserved!");
    expect(within(dialog).getByText("47 minutes")).toBeTruthy();
    expect(within(dialog).getByText("Overview unavailable")).toBeTruthy();
    dialog.scrollTop = 180;
    fireEvent.scroll(dialog);

    fireEvent.click(within(dialog).getByRole("button", { name: "Close" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    selectSettings();
    fireEvent.click(screen.getByRole("radio", { name: "Dark" }));
    fireEvent(window, new Event("resize"));
    selectDiscover();
    fireEvent.click(screen.getByRole("radio", { name: "Movies" }));
    selectTvDiscover();
    const reopenedDetailsButton = await screen.findByRole("button", {
      name: /View details: Exact Show — 特別版/,
    });
    fireEvent.click(reopenedDetailsButton);
    dialog = await screen.findByRole("dialog");
    expect(
      within(dialog).getByRole("heading", {
        level: 5,
        name: "第一話 — The Beginning!",
      }).textContent,
    ).toBe(episodeName);
    expect(fetchMock).toHaveBeenCalledTimes(4);
    expect(dialog.scrollTop).toBe(180);
    expect(document.documentElement.dataset.theme).toBe("dark");

    fireEvent.click(
      within(dialog).getByRole("button", { name: "Retry details" }),
    );
    await within(dialog).findByRole("button", { name: "View seasons" });
    expect(within(dialog).queryByText(episodeName)).toBeNull();
    fireEvent.click(within(dialog).getByRole("button", { name: "View seasons" }));
    expect(within(dialog).getByText("1 verified season")).toBeTruthy();
    expect(
      within(dialog).getByRole("button", { name: "Select Season 3" }),
    ).toBeTruthy();
    expect(
      within(dialog).queryByRole("button", { name: "Select Season 2" }),
    ).toBeNull();
    expect(fetchMock).toHaveBeenCalledTimes(5);
    expect(scanTvLibraryMock).not.toHaveBeenCalled();
    expect(openTvFileMock).not.toHaveBeenCalled();
    expect(startVerifiedVrDownloadMock).not.toHaveBeenCalled();
    expect(startVerifiedAdultDownloadMock).not.toHaveBeenCalled();
    expect(startVerifiedMovieDownloadMock).not.toHaveBeenCalled();
  });

  it("preserves completed API Bay comparison state through ordinary context changes and isolates late inspection and Save results", async () => {
    const showName = "Exact  Show — 特別版";
    const episodeName = "第三話  —  Exact Episode";
    const standardReleaseName =
      "Exact  Show — 特別版.S02E03+720p.第三話  —  1080p";
    const hdReleaseName = "Exact Show - 2x03+10bit - 2160p";
    const compactContinuationNames = [
      "Exact Show S02E03+E04",
      "Exact Show S02E03+04",
      "Exact Show 2x03+04",
      "Exact Show S02E03&E04",
      "Exact Show S02E03,04",
      "Exact Show 2x03/04",
      "Exact Show S02E03:04",
      "Exact Show S02E03 04",
      "Exact Show S02E03-E-04",
      "Exact Show S02E03 / E 04",
      "Exact Show 2x03/x04",
      "Exact Show 2x03 x 04",
    ];
    const standardHash = "0123456789abcdef0123456789abcdef01234567";
    const hdHash = "abcdef0123456789abcdef0123456789abcdef01";
    loadTmdbTokenMock.mockResolvedValue("episode-release-token");
    fetchMock
      .mockResolvedValueOnce(jsonResponse({ results: [] }))
      .mockResolvedValueOnce(
        jsonResponse({ results: [{ id: 701, name: showName }] }),
      )
      .mockResolvedValueOnce(
        jsonResponse({
          id: 701,
          name: showName,
          seasons: [
            { id: 9001, season_number: 2, name: "Season 2" },
          ],
        }),
      )
      .mockResolvedValueOnce(
        jsonResponse({
          id: 9001,
          season_number: 2,
          episodes: [
            {
              id: 9103,
              season_number: 2,
              episode_number: 3,
              name: episodeName,
            },
          ],
        }),
      );
    fetchApiBayTvReleasesMock.mockResolvedValue([
      "701",
      showName,
      "9001",
      "2",
      "9103",
      "3",
      episodeName,
      "tt0123456",
      "2",
      "1001",
      standardReleaseName,
      "205",
      "419000000",
      "12",
      "4",
      "Exact  Uploader",
      "vip",
      "1710000000",
      standardHash,
      "API Bay",
      "1002",
      hdReleaseName,
      "208",
      "",
      "2",
      "0",
      "",
      "",
      "",
      hdHash,
      "API Bay",
    ]);

    render(<App />);
    selectDiscover();
    await screen.findByRole("heading", { name: "No trending movies returned" });
    selectTvDiscover();
    fireEvent.click(
      await screen.findByRole("button", {
        name: /View details: Exact Show — 特別版/,
      }),
    );
    let detailsDialog = await screen.findByRole("dialog");
    fireEvent.click(
      await within(detailsDialog).findByRole("button", {
        name: "View seasons",
      }),
    );
    fireEvent.click(
      within(detailsDialog).getByRole("button", { name: "Select Season 2" }),
    );
    const findReleasesButton = await within(detailsDialog).findByRole(
      "button",
      { name: "Find releases" },
    );
    expect(fetchApiBayTvReleasesMock).not.toHaveBeenCalled();
    findReleasesButton.focus();
    fireEvent.click(findReleasesButton);

    const comparisonDescription = await screen.findByText(
      /Metadata-only comparison for the exact selected episode/,
    );
    let comparisonDialog = comparisonDescription.closest(
      '[role="dialog"]',
    ) as HTMLElement;
    await within(comparisonDialog).findByLabelText("Verified TV release totals");
    expect(fetchApiBayTvReleasesMock).toHaveBeenCalledWith({
      tmdbTvId: 701,
      providerSeasonId: 9001,
      providerEpisodeId: 9103,
    });
    expect(fetchApiBayTvReleasesMock).toHaveBeenCalledTimes(1);
    const totals = within(comparisonDialog).getByLabelText(
      "Verified TV release totals",
    );
    expect(totals.textContent).toContain("2 verified releases");
    expect(totals.textContent).toContain("1 TV Shows");
    expect(totals.textContent).toContain("1 HD TV Shows");
    const releaseList = within(comparisonDialog).getByRole("list", {
      name: /Verified API Bay releases for Exact Show — 特別版 Season 2 Episode 3/,
    });
    expect(within(releaseList).getAllByRole("button")).toHaveLength(2);
    for (const name of compactContinuationNames) {
      expect(within(comparisonDialog).queryByText(name)).toBeNull();
    }
    const standardReleaseLabel = [
      ...comparisonDialog.querySelectorAll(".vr-releases__release-name"),
    ].find((element) => element.textContent === standardReleaseName);
    expect(standardReleaseLabel?.textContent).toBe(standardReleaseName);
    expect(within(comparisonDialog).getByText(hdReleaseName)).toBeTruthy();
    expect(
      within(comparisonDialog).getByText("Select one verified release to compare its metadata."),
    ).toBeTruthy();
    expect(within(comparisonDialog).queryByText("Selected release")).toBeNull();
    expect(within(comparisonDialog).queryByRole("button", { name: "Inspect torrent" })).toBeNull();
    expect(within(comparisonDialog).queryByRole("button", { name: /download/i })).toBeNull();

    fireEvent.click(standardReleaseLabel!.closest("button")!);
    const selectedReleaseHeading = await within(comparisonDialog).findByRole(
      "heading",
      { name: "Selected release" },
    );
    const selection = selectedReleaseHeading.closest("section") as HTMLElement;
    expect(
      selection.querySelector(".vr-releases__release-name")?.textContent,
    ).toBe(standardReleaseName);
    for (const name of compactContinuationNames) {
      expect(within(selection).queryByText(name)).toBeNull();
    }
    expect(within(selection).getByText(standardHash)).toBeTruthy();
    expect(within(selection).getByText(/Season 2, Episode 3/).textContent).toContain(
      episodeName,
    );
    const inspectButton = within(selection).getByRole("button", {
      name: "Inspect torrent",
    });
    const pendingInspection = createDeferred<string[]>();
    inspectApiBayTvTorrentMock.mockReturnValueOnce(pendingInspection.promise);
    inspectButton.focus();
    fireEvent.click(inspectButton);
    const loadingInspectionDialog = (
      await screen.findByText("Retrieving exact-infohash metadata")
    ).closest('[role="dialog"]') as HTMLElement;
    expect(loadingInspectionDialog.getAttribute("aria-busy")).toBe("true");
    expect(inspectApiBayTvTorrentMock).toHaveBeenCalledWith({
      tmdbTvId: 701,
      providerSeasonId: 9001,
      providerEpisodeId: 9103,
      providerItemId: "1001",
    });
    expect(inspectApiBayTvTorrentMock.mock.calls[0][0]).not.toHaveProperty(
      "infohash",
    );
    expect(inspectApiBayTvTorrentMock.mock.calls[0][0]).not.toHaveProperty(
      "trackers",
    );
    await act(async () => {
      pendingInspection.resolve([
        "tv-1-1-1001",
        "Exact  Torrent — 特別版",
        standardHash,
        "12",
        "Exact  Show — 特別版/第三話  —  Exact Episode.mkv",
        "5",
        "Extras/予告  編.mp4",
        "7",
      ]);
      await pendingInspection.promise;
    });
    const inspectionDialog = screen
      .getByText("Generated verified TV metainfo")
      .closest('[role="dialog"]') as HTMLElement;
    expect(
      within(inspectionDialog)
        .getByText("Torrent name")
        .parentElement?.querySelector("dd")?.textContent,
    ).toBe("Exact  Torrent — 特別版");
    const exactFiles = within(inspectionDialog).getByRole("list", {
      name: /Files in generated verified TV metainfo for Exact Show/,
    });
    expect(within(exactFiles).getAllByRole("listitem")).toHaveLength(2);
    expect(exactFiles.textContent).toContain(
      "Exact  Show — 特別版/第三話  —  Exact Episode.mkv",
    );
    expect(within(inspectionDialog).queryByRole("checkbox")).toBeNull();
    expect(
      within(inspectionDialog).queryByRole("button", { name: /start/i }),
    ).toBeNull();
    saveVerifiedTvTorrentMock
      .mockResolvedValueOnce(false)
      .mockResolvedValueOnce(true);
    const saveButton = within(inspectionDialog).getByRole("button", {
      name: "Save generated metainfo",
    });
    fireEvent.click(saveButton);
    expect(
      await within(inspectionDialog).findByText(
        "Destination selection cancelled. No file was written.",
      ),
    ).toBeTruthy();
    fireEvent.click(saveButton);
    expect(
      await within(inspectionDialog).findByText(
        "Generated verified TV metainfo saved.",
      ),
    ).toBeTruthy();
    expect(saveVerifiedTvTorrentMock).toHaveBeenLastCalledWith({
      inspectionId: "tv-1-1-1001",
    });
    const lateSave = createDeferred<boolean>();
    saveVerifiedTvTorrentMock.mockReturnValueOnce(lateSave.promise);
    fireEvent.click(saveButton);
    expect(
      within(inspectionDialog).getByRole("button", { name: "Saving…" }),
    ).toBeTruthy();
    fireEvent.click(
      within(inspectionDialog).getByRole("button", { name: "Close" }),
    );
    await waitFor(() => expect(document.activeElement).toBe(inspectButton));
    await act(async () => {
      lateSave.resolve(true);
      await lateSave.promise;
    });
    expect(
      screen.queryByText("Generated verified TV metainfo saved."),
    ).toBeNull();
    expect(
      screen.queryByText(
        "The destination exists or the generated metainfo could not be written.",
      ),
    ).toBeNull();
    expect(within(comparisonDialog).getByText("Selected release")).toBeTruthy();
    expect(startVerifiedVrDownloadMock).not.toHaveBeenCalled();
    expect(startVerifiedAdultDownloadMock).not.toHaveBeenCalled();
    expect(startVerifiedMovieDownloadMock).not.toHaveBeenCalled();
    const lateInspection = createDeferred<string[]>();
    inspectApiBayTvTorrentMock.mockReturnValueOnce(lateInspection.promise);
    fireEvent.click(inspectButton);
    const dismissedInspectionDialog = (
      await screen.findByText("Retrieving exact-infohash metadata")
    ).closest('[role="dialog"]') as HTMLElement;
    fireEvent.keyDown(dismissedInspectionDialog, { key: "Escape" });
    await waitFor(() =>
      expect(screen.queryByText("Retrieving exact-infohash metadata")).toBeNull(),
    );
    expect(invalidateVerifiedTvTorrentMock).toHaveBeenCalled();
    await act(async () => {
      lateInspection.resolve([
        "tv-stale",
        "Stale TV torrent",
        standardHash,
        "5",
        "Stale.mkv",
        "5",
      ]);
      await lateInspection.promise;
    });
    expect(screen.queryByText("Stale TV torrent")).toBeNull();
    expect(within(comparisonDialog).getByText("Selected release")).toBeTruthy();
    comparisonDialog.scrollTop = 140;
    fireEvent.scroll(comparisonDialog);
    invalidateTvReleaseContextMock.mockClear();

    fireEvent.click(
      within(comparisonDialog).getByRole("button", { name: "Close" }),
    );
    await waitFor(() => expect(document.activeElement).toBe(findReleasesButton));
    fireEvent.click(
      within(detailsDialog).getByRole("button", { name: "Close" }),
    );
    selectDashboard();
    selectLibrary();
    fireEvent.click(screen.getByRole("button", { name: "Downloads" }));
    selectSettings();
    fireEvent.click(screen.getByRole("radio", { name: "Dark" }));
    fireEvent(window, new Event("resize"));
    selectDiscover();
    fireEvent.click(screen.getByRole("radio", { name: "Movies" }));
    selectTvDiscover();
    fireEvent.click(
      await screen.findByRole("button", {
        name: /View details: Exact Show — 特別版/,
      }),
    );
    detailsDialog = await screen.findByRole("dialog");
    fireEvent.click(
      within(detailsDialog).getByRole("button", { name: "Find releases" }),
    );
    comparisonDialog = (
      await screen.findByText(
        /Metadata-only comparison for the exact selected episode/,
      )
    ).closest('[role="dialog"]') as HTMLElement;
    expect(within(comparisonDialog).getByText("Selected release")).toBeTruthy();
    expect(
      [...comparisonDialog.querySelectorAll(".vr-releases__release-name")].some(
        (element) => element.textContent === standardReleaseName,
      ),
    ).toBe(true);
    expect(comparisonDialog.scrollTop).toBe(140);
    expect(fetchApiBayTvReleasesMock).toHaveBeenCalledTimes(1);
    expect(invalidateTvReleaseContextMock).not.toHaveBeenCalled();
    expect(fetchMock).toHaveBeenCalledTimes(4);
    expect(document.documentElement.dataset.theme).toBe("dark");
  });

  it("invalidates a dismissed API Bay request and keeps its late rows out of a newer episode", async () => {
    const lateRelease = createDeferred<string[]>();
    loadTmdbTokenMock.mockResolvedValue("episode-release-token");
    fetchMock
      .mockResolvedValueOnce(jsonResponse({ results: [] }))
      .mockResolvedValueOnce(
        jsonResponse({ results: [{ id: 701, name: "Exact Show" }] }),
      )
      .mockResolvedValueOnce(
        jsonResponse({
          id: 701,
          name: "Exact Show",
          seasons: [{ id: 9001, season_number: 2, name: "Season 2" }],
        }),
      )
      .mockResolvedValueOnce(
        jsonResponse({
          id: 9001,
          season_number: 2,
          episodes: [
            { id: 9103, season_number: 2, episode_number: 3, name: "Episode 3" },
            { id: 9104, season_number: 2, episode_number: 4, name: "Episode 4" },
          ],
        }),
      );
    fetchApiBayTvReleasesMock
      .mockReturnValueOnce(lateRelease.promise)
      .mockResolvedValueOnce([
        "701",
        "Exact Show",
        "9001",
        "2",
        "9104",
        "4",
        "Episode 4",
        "tt0123456",
        "0",
      ]);

    render(<App />);
    selectDiscover();
    await screen.findByRole("heading", { name: "No trending movies returned" });
    selectTvDiscover();
    fireEvent.click(
      await screen.findByRole("button", { name: "View details: Exact Show" }),
    );
    const detailsDialog = await screen.findByRole("dialog");
    fireEvent.click(
      await within(detailsDialog).findByRole("button", { name: "View seasons" }),
    );
    fireEvent.click(
      within(detailsDialog).getByRole("button", { name: "Select Season 2" }),
    );
    const findButtons = await within(detailsDialog).findAllByRole("button", {
      name: "Find releases",
    });
    fireEvent.click(findButtons[0]);
    const loadingDialog = (
      await screen.findByText("Finding verified TV releases")
    ).closest('[role="dialog"]') as HTMLElement;
    fireEvent.keyDown(loadingDialog, { key: "Escape" });
    await waitFor(() =>
      expect(screen.queryByText("Finding verified TV releases")).toBeNull(),
    );

    fireEvent.click(findButtons[1]);
    const currentDialog = (
      await screen.findByText(
        /Metadata-only comparison for the exact selected episode/,
      )
    ).closest('[role="dialog"]') as HTMLElement;
    expect(
      await within(currentDialog).findByText(
        "API Bay returned no exact releases for Season 2, Episode 4.",
      ),
    ).toBeTruthy();
    expect(fetchApiBayTvReleasesMock.mock.calls).toEqual([
      [{ tmdbTvId: 701, providerSeasonId: 9001, providerEpisodeId: 9103 }],
      [{ tmdbTvId: 701, providerSeasonId: 9001, providerEpisodeId: 9104 }],
    ]);

    await act(async () => {
      lateRelease.resolve([
        "701",
        "Exact Show",
        "9001",
        "2",
        "9103",
        "3",
        "Episode 3",
        "tt0123456",
        "1",
        "1003",
        "Late Exact Show S02E03",
        "205",
        "",
        "",
        "",
        "",
        "",
        "",
        "0123456789abcdef0123456789abcdef01234567",
        "API Bay",
      ]);
      await lateRelease.promise;
    });
    expect(within(currentDialog).queryByText("Late Exact Show S02E03")).toBeNull();
    expect(within(currentDialog).getByText(/Episode 4/)).toBeTruthy();
  });

  it("keeps every TV release failure local and retries the exact current episode", async () => {
    loadTmdbTokenMock.mockResolvedValue("episode-release-token");
    fetchMock
      .mockResolvedValueOnce(jsonResponse({ results: [] }))
      .mockResolvedValueOnce(
        jsonResponse({ results: [{ id: 701, name: "Retry Show" }] }),
      )
      .mockResolvedValueOnce(
        jsonResponse({
          id: 701,
          name: "Retry Show",
          seasons: [{ id: 9001, season_number: 2, name: "Season 2" }],
        }),
      )
      .mockResolvedValueOnce(
        jsonResponse({
          id: 9001,
          season_number: 2,
          episodes: [
            {
              id: 9103,
              season_number: 2,
              episode_number: 3,
              name: "Retry Episode",
            },
          ],
        }),
      );
    const outcomes = [
      ["tv_release_tmdb_unauthorized", "TMDB token was not accepted"],
      ["tv_release_tmdb_rate_limited", "TMDB release lookup is rate-limited"],
      ["tv_release_tmdb_network_error", "TMDB could not be reached"],
      ["tv_release_tmdb_malformed", "TMDB returned invalid episode identity data"],
      ["unexpected", "TMDB could not resolve the TV identity"],
      ["tv_release_no_imdb_identity", "No IMDb series identity is available"],
      ["tv_release_apibay_source_unavailable", "API Bay is unavailable"],
      ["tv_release_apibay_network_error", "API Bay could not be reached"],
      ["tv_release_apibay_malformed", "API Bay returned invalid release data"],
      ["tv_release_apibay_conflicting", "API Bay returned conflicting release identities"],
      ["tv_release_apibay_provider_error", "API Bay could not load TV releases"],
    ] as const;
    for (const [error] of outcomes) {
      fetchApiBayTvReleasesMock.mockRejectedValueOnce(error);
    }
    fetchApiBayTvReleasesMock.mockResolvedValueOnce([
      "701",
      "Retry Show",
      "9001",
      "2",
      "9103",
      "3",
      "Retry Episode",
      "tt0123456",
      "0",
    ]);

    render(<App />);
    selectDiscover();
    await screen.findByRole("heading", { name: "No trending movies returned" });
    selectTvDiscover();
    fireEvent.click(
      await screen.findByRole("button", { name: "View details: Retry Show" }),
    );
    const detailsDialog = await screen.findByRole("dialog");
    fireEvent.click(
      await within(detailsDialog).findByRole("button", { name: "View seasons" }),
    );
    fireEvent.click(
      within(detailsDialog).getByRole("button", { name: "Select Season 2" }),
    );
    fireEvent.click(
      await within(detailsDialog).findByRole("button", {
        name: "Find releases",
      }),
    );
    const comparisonDialog = (
      await screen.findByText(
        /Metadata-only comparison for the exact selected episode/,
      )
    ).closest('[role="dialog"]') as HTMLElement;

    for (const [error, heading] of outcomes) {
      expect(
        await within(comparisonDialog).findByRole("heading", { name: heading }),
      ).toBeTruthy();
      if (error === "tv_release_apibay_conflicting") {
        expect(
          within(comparisonDialog).queryByLabelText(
            "Verified TV release totals",
          ),
        ).toBeNull();
        expect(
          within(comparisonDialog).queryByRole("heading", {
            name: "Selected release",
          }),
        ).toBeNull();
        expect(
          within(comparisonDialog).queryByRole("list", {
            name: /Verified API Bay releases/,
          }),
        ).toBeNull();
      }
      fireEvent.click(
        within(comparisonDialog).getByRole("button", { name: "Retry" }),
      );
    }
    expect(
      await within(comparisonDialog).findByText(
        "API Bay returned no exact releases for Season 2, Episode 3.",
      ),
    ).toBeTruthy();
    expect(
      within(comparisonDialog).queryByLabelText("Verified TV release totals"),
    ).toBeNull();
    expect(
      within(comparisonDialog).queryByRole("list", {
        name: /Verified API Bay releases/,
      }),
    ).toBeNull();
    expect(
      within(comparisonDialog).queryByRole("heading", {
        name: "Selected release",
      }),
    ).toBeNull();
    expect(fetchApiBayTvReleasesMock).toHaveBeenCalledTimes(12);
    for (const [parameters] of fetchApiBayTvReleasesMock.mock.calls) {
      expect(parameters).toEqual({
        tmdbTvId: 701,
        providerSeasonId: 9001,
        providerEpisodeId: 9103,
      });
    }
  });

  it("accepts only the newest exact season context and blocks a dismissed pending guide", async () => {
    const seasonOneResponse = createDeferred<Response>();
    const seasonTwoResponse = createDeferred<Response>();
    const dismissedSeasonResponse = createDeferred<Response>();
    loadTmdbTokenMock.mockResolvedValue("fixture-token");
    fetchMock
      .mockResolvedValueOnce(jsonResponse({ results: [] }))
      .mockResolvedValueOnce(
        jsonResponse({ results: [{ id: 701, name: "Exact Show" }] }),
      )
      .mockResolvedValueOnce(
        jsonResponse({
          id: 701,
          name: "Exact Show",
          seasons: [
            { id: 9000, season_number: 1, name: "Season One" },
            { id: 9001, season_number: 2, name: "Season Two" },
          ],
        }),
      )
      .mockReturnValueOnce(seasonOneResponse.promise)
      .mockReturnValueOnce(seasonTwoResponse.promise)
      .mockReturnValueOnce(dismissedSeasonResponse.promise);

    render(<App />);
    selectDiscover();
    await screen.findByRole("heading", { name: "No trending movies returned" });
    selectTvDiscover();
    fireEvent.click(
      await screen.findByRole("button", { name: "View details: Exact Show" }),
    );
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(
      await within(dialog).findByRole("button", { name: "View seasons" }),
    );
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Select Season 1" }),
    );
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Select Season 2" }),
    );

    await act(async () => {
      seasonTwoResponse.resolve(
        jsonResponse({
          id: 9001,
          season_number: 2,
          episodes: [
            {
              id: 9201,
              season_number: 2,
              episode_number: 1,
              name: "Newest season result",
            },
          ],
        }),
      );
      await seasonTwoResponse.promise;
    });
    expect(await within(dialog).findByText("Newest season result")).toBeTruthy();

    await act(async () => {
      seasonOneResponse.resolve(
        jsonResponse({
          id: 9000,
          season_number: 1,
          episodes: [
            {
              id: 9101,
              season_number: 1,
              episode_number: 1,
              name: "Stale season result",
            },
          ],
        }),
      );
      await seasonOneResponse.promise;
    });
    expect(within(dialog).queryByText("Stale season result")).toBeNull();
    expect(within(dialog).getByText("Newest season result")).toBeTruthy();

    fireEvent.click(
      within(dialog).getByRole("button", { name: "Select Season 1" }),
    );
    expect(
      await within(dialog).findByRole("heading", {
        name: "Loading season episodes",
      }),
    ).toBeTruthy();
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();

    await act(async () => {
      dismissedSeasonResponse.resolve(
        jsonResponse({
          id: 9000,
          season_number: 1,
          episodes: [
            {
              id: 9301,
              season_number: 1,
              episode_number: 1,
              name: "Dismissed late result",
            },
          ],
        }),
      );
      await dismissedSeasonResponse.promise;
    });
    expect(screen.queryByText("Dismissed late result")).toBeNull();
    fireEvent.click(
      screen.getByRole("button", { name: "View details: Exact Show" }),
    );
    const reopenedDialog = await screen.findByRole("dialog");
    expect(within(reopenedDialog).queryByText("Dismissed late result")).toBeNull();
    expect(
      within(reopenedDialog).getByText(
        "Select Season 1 again to load its episodes.",
      ),
    ).toBeTruthy();
  });

  it("shows distinct local season outcomes and retries the current exact season", async () => {
    loadTmdbTokenMock.mockResolvedValue("fixture-token");
    fetchMock
      .mockResolvedValueOnce(jsonResponse({ results: [] }))
      .mockResolvedValueOnce(
        jsonResponse({ results: [{ id: 701, name: "Retry Show" }] }),
      )
      .mockResolvedValueOnce(
        jsonResponse({
          id: 701,
          name: "Retry Show",
          seasons: [{ id: 9001, season_number: 2, name: "Retry Season" }],
        }),
      )
      .mockResolvedValueOnce(jsonResponse({}, 401))
      .mockResolvedValueOnce(jsonResponse({}, 429))
      .mockRejectedValueOnce(new TypeError("offline"))
      .mockResolvedValueOnce(
        jsonResponse({ id: 9001, season_number: 3, episodes: [] }),
      )
      .mockResolvedValueOnce(jsonResponse({}, 500))
      .mockResolvedValueOnce(
        jsonResponse({ id: 9001, season_number: 2, episodes: [] }),
      );

    render(<App />);
    selectDiscover();
    await screen.findByRole("heading", { name: "No trending movies returned" });
    selectTvDiscover();
    fireEvent.click(
      await screen.findByRole("button", { name: "View details: Retry Show" }),
    );
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(
      await within(dialog).findByRole("button", { name: "View seasons" }),
    );
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Select Season 2" }),
    );

    for (const heading of [
      "TMDB token was not accepted",
      "TMDB season rate limit reached",
      "TMDB season guide could not be reached",
      "TMDB returned an invalid season guide",
      "TMDB could not load the season guide",
      "No episodes returned",
    ]) {
      expect(
        await within(dialog).findByRole("heading", { name: heading }),
      ).toBeTruthy();
      if (heading !== "No episodes returned") {
        fireEvent.click(
          within(dialog).getByRole("button", { name: "Retry Season 2" }),
        );
      }
    }

    expect(fetchMock).toHaveBeenCalledTimes(9);
    for (const call of fetchMock.mock.calls.slice(3)) {
      expect(call[0]).toBe("https://api.themoviedb.org/3/tv/701/season/2");
    }
  });

  it("keeps details errors local and blocks late details across selection, category, and dismissal", async () => {
    const showADetails = createDeferred<Response>();
    const showBDetails = createDeferred<Response>();
    loadTmdbTokenMock.mockResolvedValue("fixture-token");
    fetchMock
      .mockResolvedValueOnce(jsonResponse({ results: [] }))
      .mockResolvedValueOnce(
        jsonResponse({
          results: [
            { id: 701, name: "TV A" },
            { id: 702, name: "TV B" },
          ],
        }),
      )
      .mockReturnValueOnce(showADetails.promise)
      .mockReturnValueOnce(showBDetails.promise);

    render(<App />);
    selectDiscover();
    await screen.findByRole("heading", { name: "No trending movies returned" });
    selectTvDiscover();
    const showAButton = await screen.findByRole("button", {
      name: "View details: TV A",
    });
    const showBButton = screen.getByRole("button", {
      name: "View details: TV B",
    });
    fireEvent.click(showAButton);
    fireEvent.click(showBButton);
    const dialog = await screen.findByRole("dialog");

    await act(async () => {
      showBDetails.resolve(jsonResponse({ id: 999, name: "Wrong TV" }));
      await showBDetails.promise;
    });
    expect(
      await within(dialog).findByRole("heading", {
        level: 3,
        name: "TMDB returned invalid TV details",
      }),
    ).toBeTruthy();
    expect(document.querySelector('[aria-label="Weekly trending TV"]')).not.toBeNull();

    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    fireEvent.click(screen.getByRole("radio", { name: "Movies" }));
    await act(async () => {
      showADetails.resolve(
        jsonResponse({
          id: 701,
          name: "Late TV A",
          overview: "Must stay absent",
        }),
      );
      await showADetails.promise;
    });
    expect(screen.queryByText("Must stay absent")).toBeNull();
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("preserves independent TV search state and responsive pagination without extra requests", async () => {
    const query = "Persistent TV query";
    const results = Array.from({ length: 25 }, (_, index) => ({
      id: index + 801,
      name: `Persistent TV ${index + 1}`,
    }));
    loadTmdbTokenMock.mockResolvedValue("fixture-token");
    fetchMock
      .mockResolvedValueOnce(
        jsonResponse({ results: [{ id: 1, title: "Movie result" }] }),
      )
      .mockResolvedValueOnce(
        jsonResponse({ results: [{ id: 2, name: "TV trend" }] }),
      )
      .mockResolvedValueOnce(jsonResponse({ results }));
    gallerySizes.discover = { width: 1088, height: 2408 };

    render(<App />);
    selectDiscover();
    expect(await screen.findByText("Movie result")).toBeTruthy();
    selectTvDiscover();
    expect(await screen.findByText("TV trend")).toBeTruthy();
    submitTvDiscoverSearch(query);
    expect(await screen.findByText("Persistent TV 1")).toBeTruthy();

    expect(
      document.querySelector('[data-gallery="discover"]')?.getAttribute(
        "data-page-capacity",
      ),
    ).toBe("25");
    resizeGallery("discover", 1528, 472);
    expect(
      document.querySelector('[data-gallery="discover"]')?.getAttribute(
        "data-page-capacity",
      ),
    ).toBe("7");
    expect(screen.getByText("Page 1 of 4")).toBeTruthy();
    fireEvent.click(
      screen.getByRole("button", {
        name: "Next TMDB TV search results page",
      }),
    );
    expect(screen.getByText("Page 2 of 4")).toBeTruthy();

    selectSettings();
    fireEvent.click(screen.getByRole("radio", { name: "Dark" }));
    selectDashboard();
    selectDiscover();
    selectTvDiscover();
    expect(screen.getByRole("textbox", { name: "Search TV" })).toHaveProperty(
      "value",
      query,
    );
    expect(screen.getByText("Page 2 of 4")).toBeTruthy();
    resizeGallery("discover", 1088, 956);
    expect(
      document.querySelector('[data-gallery="discover"]')?.getAttribute(
        "data-page-capacity",
      ),
    ).toBe("10");
    expect(screen.getByText("Page 2 of 3")).toBeTruthy();
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(fetchMock).toHaveBeenCalledTimes(3);
  });
});

describe("Adult Discover and verified release comparison", () => {
  it("requires an explicit exact-code search and exposes only exact verified releases for selection", async () => {
    const exactTitle = "作品  —  Exact  Title!";
    const exactReleaseName =
      "【作品】 adlt_00123  Director’s Cut\t—\n特別版!?";
    fetchJavdbCatalogMock.mockResolvedValue(
      javdbCatalogFixture("adlt_00123", exactTitle),
    );
    fetchSukebeiAdultReleasesMock.mockResolvedValue(
      sukebeiReleaseFixture([
        { name: "ADLT-123 standard" },
        { name: exactReleaseName, seeders: 4, size: "8.0 GiB" },
        { name: "ADLT-124 neighbor", seeders: 500 },
        { name: "ADLT-1230 extension", seeders: 400 },
        { name: "XADLT-123 embedded", seeders: 300 },
        { name: "ADLT-123 + XYZ-7 pack", seeders: 200 },
        { name: "ADLT-125 same-prefix neighbor", seeders: 100 },
        { name: "Candidate with no product code", seeders: 90 },
      ]),
    );
    render(<App />);
    selectDiscover();
    expect(fetchJavdbCatalogMock).not.toHaveBeenCalled();
    selectAdultDiscover();

    const codeInput = screen.getByRole("textbox", {
      name: "Search product code",
    });
    for (const invalidCode of ["", "ADLT-0", "ADLT-123 extra", "A-1"]) {
      fireEvent.change(codeInput, { target: { value: invalidCode } });
      fireEvent.click(screen.getByRole("button", { name: "Search" }));
      expect(screen.getByRole("alert").textContent).toContain(
        "Enter a valid Adult product code",
      );
    }
    expect(fetchJavdbCatalogMock).not.toHaveBeenCalled();

    fireEvent.change(codeInput, { target: { value: "adlt_00123" } });
    expect(fetchJavdbCatalogMock).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Search" }));

    const codeHeading = await screen.findByRole("heading", {
      level: 3,
      name: "ADLT-123",
    });
    const adultCard = codeHeading.closest("article");
    if (adultCard === null) {
      throw new Error("The exact Adult catalog card was not rendered.");
    }
    expect(
      within(adultCard).getByText("Title").parentElement?.querySelector("dd")
        ?.textContent,
    ).toBe(exactTitle);
    expect(within(adultCard).getByText("JavDB")).toBeTruthy();
    expect(fetchJavdbCatalogMock).toHaveBeenCalledWith({ code: "ADLT-123" });
    expect(fetchSukebeiAdultReleasesMock).not.toHaveBeenCalled();

    fireEvent.change(codeInput, { target: { value: "ADLT-124" } });
    expect(screen.getByRole("heading", { level: 3, name: "ADLT-123" })).toBeTruthy();
    expect(fetchJavdbCatalogMock).toHaveBeenCalledTimes(1);

    fireEvent.click(
      within(adultCard).getByRole("button", {
        name: "Copy title: ADLT-123",
      }),
    );
    await waitFor(() =>
      expect(clipboardWriteMock).toHaveBeenCalledWith("ADLT-123"),
    );
    const cover = adultCard.querySelector("img");
    expect(cover).not.toBeNull();
    fireEvent.error(cover as HTMLImageElement);
    expect(within(adultCard).getByText("Cover unavailable")).toBeTruthy();

    fireEvent.click(
      within(adultCard).getByRole("button", {
        name: "Find releases: ADLT-123",
      }),
    );
    const releaseList = await screen.findByRole("list", {
      name: "Verified Adult releases for ADLT-123",
    });
    expect(fetchSukebeiAdultReleasesMock).toHaveBeenCalledWith({
      code: "ADLT-123",
    });
    expect(within(releaseList).getAllByRole("button")).toHaveLength(2);
    expect(
      within(releaseList).getByRole("button", {
        name: /ADLT-123 standard/,
      }).textContent,
    ).toContain("Size UnavailableSeeders Unavailable");
    expect(screen.getByLabelText("Verified Adult release totals").textContent).toBe(
      "2 verified releases2 from SukebeiRetry",
    );
    for (const rejectedName of [
      "ADLT-124 neighbor",
      "ADLT-1230 extension",
      "XADLT-123 embedded",
      "ADLT-123 + XYZ-7 pack",
      "ADLT-125 same-prefix neighbor",
      "Candidate with no product code",
    ]) {
      expect(screen.queryByText(rejectedName)).toBeNull();
    }
    const exactReleaseRow = Array.from(
      releaseList.querySelectorAll<HTMLElement>(".vr-releases__release-name"),
    ).find((releaseName) => releaseName.textContent === exactReleaseName);
    expect(exactReleaseRow).toBeDefined();
    fireEvent.click(exactReleaseRow?.closest("button") as HTMLButtonElement);
    const selectedSummary = screen
      .getByRole("heading", { name: "Selected release" })
      .closest("section");
    expect(selectedSummary).not.toBeNull();
    const selectedReleaseName = Array.from(
      (selectedSummary as HTMLElement).querySelectorAll("dd"),
    ).find((value) => value.textContent === exactReleaseName);
    expect(selectedReleaseName).toBeDefined();
    expect(screen.queryByRole("button", { name: "Inspect torrent" })).toBeNull();
    expect(fetchSukebeiVrReleasesMock).not.toHaveBeenCalled();
    expect(inspectSukebeiVrTorrentMock).not.toHaveBeenCalled();
    expect(saveVerifiedVrTorrentMock).not.toHaveBeenCalled();
    expect(startVerifiedVrDownloadMock).not.toHaveBeenCalled();
  });

  it("inspects the complete Adult artifact with no initial selection and preserves exact-byte saving", async () => {
    const exactReleaseName = "【Adult】 ADLT-123  Exact\t—\n特別版!?";
    const expectedInfohash = "0123456789abcdef0123456789abcdef01234567";
    fetchJavdbCatalogMock.mockResolvedValue(
      javdbCatalogFixture("ADLT-123", "Inspectable Adult title"),
    );
    fetchSukebeiAdultReleasesMock.mockResolvedValue(
      sukebeiReleaseFixture([
        { name: "ADLT-123 metadata only" },
        {
          infohash: expectedInfohash,
          itemId: "321",
          name: exactReleaseName,
          seeders: 7,
          size: "6.5 GiB",
        },
      ]),
    );
    const inspectionResult = createDeferred<string[]>();
    inspectSukebeiAdultTorrentMock.mockReturnValue(inspectionResult.promise);
    const releaseList = await openAdultReleaseComparison();

    fireEvent.click(
      within(releaseList).getByRole("button", { name: /metadata only/ }),
    );
    expect(
      screen.getByText(/no complete safe provider artifact identity/),
    ).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Inspect torrent" })).toBeNull();

    fireEvent.click(
      within(releaseList).getByRole("button", { name: /Exact/ }),
    );
    const inspectButton = screen.getByRole("button", {
      name: "Inspect torrent",
    });
    fireEvent.click(inspectButton);
    expect(
      await screen.findByRole("heading", { name: "Inspecting verified torrent" }),
    ).toBeTruthy();
    expect(document.querySelector(".vr-torrent__release-name")?.textContent).toBe(
      exactReleaseName,
    );
    expect(screen.queryByRole("button", { name: "Save `.torrent`" })).toBeNull();

    await act(async () => {
      inspectionResult.resolve([
        "adult-1-1-321",
        "作品  —  Exact  Torrent",
        expectedInfohash,
        "12",
        "Folder/Part  1 — 映画.mkv",
        "5",
        "Folder/特別版  B.mp4",
        "7",
      ]);
      await inspectionResult.promise;
    });

    expect(inspectSukebeiAdultTorrentMock).toHaveBeenCalledWith({
      code: "ADLT-123",
      expectedInfohash,
      providerItemId: "321",
      releaseName: exactReleaseName,
      torrentUrl: "https://sukebei.nyaa.si/download/321.torrent",
    });
    const torrentNameTerm = screen.getByText("Torrent name");
    expect(torrentNameTerm.parentElement?.querySelector("dd")?.textContent).toBe(
      "作品  —  Exact  Torrent",
    );
    expect(screen.getByText(expectedInfohash)).toBeTruthy();
    expect(
      screen.getByText("Total size").parentElement?.querySelector("dd")?.textContent,
    ).toBe("12 B (12 bytes)");
    const files = screen.getByRole("list", {
      name: "Files in verified Adult torrent for ADLT-123",
    });
    const fileRows = within(files).getAllByRole("listitem");
    expect(fileRows).toHaveLength(2);
    expect(fileRows[0].querySelector("span")?.textContent).toBe(
      "Folder/Part  1 — 映画.mkv",
    );
    expect(fileRows[1].querySelector("span")?.textContent).toBe(
      "Folder/特別版  B.mp4",
    );
    const checkboxes = screen.getAllByRole("checkbox");
    expect(checkboxes).toHaveLength(2);
    expect(
      checkboxes.every((checkbox) => !(checkbox as HTMLInputElement).checked),
    ).toBe(true);
    expect(
      (screen.getByRole("button", { name: "Start download" }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);

    saveVerifiedAdultTorrentMock
      .mockResolvedValueOnce(false)
      .mockResolvedValueOnce(true)
      .mockRejectedValueOnce("adult_torrent_save_failed");
    const saveButton = screen.getByRole("button", { name: "Save `.torrent`" });
    fireEvent.click(saveButton);
    await waitFor(() =>
      expect(saveVerifiedAdultTorrentMock).toHaveBeenCalledWith({
        inspectionId: "adult-1-1-321",
      }),
    );
    expect(screen.queryByText("Verified Adult torrent file saved.")).toBeNull();
    fireEvent.click(saveButton);
    expect(
      await screen.findByText("Verified Adult torrent file saved."),
    ).toBeTruthy();
    fireEvent.click(saveButton);
    expect(
      await screen.findByText(
        "The verified Adult torrent file could not be saved.",
      ),
    ).toBeTruthy();

    const torrentDialog = screen
      .getByText("Exact selected release")
      .closest('[role="dialog"]');
    fireEvent.click(
      within(torrentDialog as HTMLElement).getByRole("button", { name: "Close" }),
    );
    await waitFor(() => expect(document.activeElement).toBe(inspectButton));
    expect(
      screen.getByRole("heading", { name: "Selected release" }),
    ).toBeTruthy();
    expect(fetchSukebeiAdultReleasesMock).toHaveBeenCalledTimes(1);
    expect(inspectSukebeiVrTorrentMock).not.toHaveBeenCalled();
    expect(saveVerifiedVrTorrentMock).not.toHaveBeenCalled();
    expect(startVerifiedVrDownloadMock).not.toHaveBeenCalled();
  });

  it("starts only explicitly selected Adult files with native-owned destination authority", async () => {
    const exactReleaseName = "【Adult】 ADLT-123  Exact\t—\n特別版!?";
    const expectedInfohash = "0123456789abcdef0123456789abcdef01234567";
    savedAdultFolder = "/Volumes/Adult — 作品";
    fetchJavdbCatalogMock.mockResolvedValue(javdbCatalogFixture("ADLT-123"));
    fetchSukebeiAdultReleasesMock.mockResolvedValue(
      sukebeiReleaseFixture([
        {
          infohash: expectedInfohash,
          itemId: "321",
          name: exactReleaseName,
        },
      ]),
    );
    inspectSukebeiAdultTorrentMock.mockResolvedValue([
      "adult-1-1-321",
      "作品  —  Exact",
      expectedInfohash,
      "12",
      "Folder/Part  1 — 映画.mkv",
      "5",
      "Folder/特別版  B.mp4",
      "7",
    ]);
    const startRequest = createDeferred<string>();
    startVerifiedAdultDownloadMock.mockReturnValue(startRequest.promise);
    listVrDownloadsMock.mockResolvedValue(
      vrDownloadFixture({
        category: "adult",
        code: "ADLT-123",
        downloadedBytes: "7",
        releaseName: exactReleaseName,
        selectedFileCount: "1",
        speedBytesPerSecond: "0",
        state: "paused",
        totalBytes: "7",
        transferId: "adult-transfer-123",
      }),
    );

    const releaseList = await openAdultReleaseComparison();
    fireEvent.click(within(releaseList).getByRole("button"));
    fireEvent.click(screen.getByRole("button", { name: "Inspect torrent" }));
    const checkboxes = await screen.findAllByRole("checkbox");
    expect(checkboxes).toHaveLength(2);
    expect(
      checkboxes.every((checkbox) => !(checkbox as HTMLInputElement).checked),
    ).toBe(true);
    const startButton = screen.getByRole("button", { name: "Start download" });
    expect((startButton as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(checkboxes[1]);
    expect((startButton as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(startButton);
    fireEvent.click(startButton);
    expect(startVerifiedAdultDownloadMock).toHaveBeenCalledOnce();
    expect(startVerifiedAdultDownloadMock).toHaveBeenCalledWith({
      inspectionId: "adult-1-1-321",
      selectedFileIds: [1],
    });
    expect(startVerifiedVrDownloadMock).not.toHaveBeenCalled();

    await act(async () => {
      startRequest.resolve("adult-transfer-123");
      await startRequest.promise;
    });
    expect(
      await screen.findByText("Selected Adult files were added to Downloads."),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "View Downloads" }));
    const heading = await screen.findByText(
      (_text, element) =>
        element?.tagName === "H2" && element.textContent === exactReleaseName,
    );
    const card = heading.closest("article") as HTMLElement;
    expect(within(card).getByText("Adult · ADLT-123")).toBeTruthy();
    expect(within(card).getByRole("button", { name: "Resume" })).toBeTruthy();
    expect(within(card).queryByRole("button", { name: "Organize files" })).toBeNull();
    fireEvent.click(within(card).getByRole("button", { name: "Cancel" }));
    const confirmation = await screen.findByRole("alertdialog");
    expect(
      within(confirmation).getByText(/remain in the accepted Adult folder/),
    ).toBeTruthy();
    fireEvent.click(
      within(confirmation).getByRole("button", { name: "Keep downloading" }),
    );
    expect(document.body.textContent).not.toContain("/Volumes/Adult — 作品");
  });

  it("keeps Adult inspection failures local, distinct, and retryable", async () => {
    const expectedInfohash = "0123456789abcdef0123456789abcdef01234567";
    fetchJavdbCatalogMock.mockResolvedValue(javdbCatalogFixture("ADLT-123"));
    fetchSukebeiAdultReleasesMock.mockResolvedValue(
      sukebeiReleaseFixture([
        {
          infohash: expectedInfohash,
          itemId: "321",
          name: "ADLT-123 exact artifact",
        },
      ]),
    );
    for (const error of [
      "adult_torrent_source_unavailable",
      "adult_torrent_network_error",
      "adult_torrent_provider_error",
      "adult_torrent_malformed",
      "adult_torrent_unsupported",
      "adult_torrent_infohash_mismatch",
      "adult_torrent_context_invalid",
      "unexpected_error",
    ]) {
      inspectSukebeiAdultTorrentMock.mockRejectedValueOnce(error);
    }
    const releaseList = await openAdultReleaseComparison();
    fireEvent.click(within(releaseList).getByRole("button"));
    fireEvent.click(screen.getByRole("button", { name: "Inspect torrent" }));

    for (const heading of [
      "Torrent artifact is unavailable",
      "Torrent artifact could not be reached",
      "Torrent provider rejected the request",
      "Torrent artifact is malformed",
      "Torrent artifact is unsupported",
      "Torrent identity did not match",
      "Torrent inspection is no longer current",
      "Torrent inspection could not be completed",
    ]) {
      expect(await screen.findByRole("heading", { name: heading })).toBeTruthy();
      expect(screen.queryByRole("button", { name: "Save `.torrent`" })).toBeNull();
      if (heading !== "Torrent inspection could not be completed") {
        fireEvent.click(
          screen.getByRole("button", { name: "Retry inspection" }),
        );
      }
    }
    expect(
      within(releaseList).getByText("ADLT-123 exact artifact"),
    ).toBeTruthy();
    expect(startVerifiedVrDownloadMock).not.toHaveBeenCalled();
  });

  it("invalidates late Adult inspection and save work across selection and navigation", async () => {
    const expectedInfohash = "0123456789abcdef0123456789abcdef01234567";
    fetchJavdbCatalogMock.mockResolvedValue(javdbCatalogFixture("ADLT-123"));
    fetchSukebeiAdultReleasesMock.mockResolvedValue(
      sukebeiReleaseFixture([
        {
          infohash: expectedInfohash,
          itemId: "321",
          name: "ADLT-123 release A",
        },
        {
          infohash: expectedInfohash,
          itemId: "322",
          name: "ADLT-123 release B",
        },
      ]),
    );
    const inspectionA = createDeferred<string[]>();
    inspectSukebeiAdultTorrentMock
      .mockReturnValueOnce(inspectionA.promise)
      .mockResolvedValueOnce([
        "adult-1-2-322",
        "Release B torrent",
        expectedInfohash,
        "7",
        "B/Exact file.mp4",
        "7",
      ]);
    const releaseList = await openAdultReleaseComparison();
    const releaseA = within(releaseList).getByRole("button", {
      name: /release A/,
    });
    const releaseB = within(releaseList).getByRole("button", {
      name: /release B/,
    });
    fireEvent.click(releaseA);
    fireEvent.click(screen.getByRole("button", { name: "Inspect torrent" }));
    await screen.findByRole("heading", { name: "Inspecting verified torrent" });

    fireEvent.click(releaseB);
    expect(screen.queryByText("Exact selected release")).toBeNull();
    await act(async () => {
      inspectionA.resolve([
        "adult-1-1-321",
        "Late release A torrent",
        expectedInfohash,
        "5",
        "A/Late file.mp4",
        "5",
      ]);
      await inspectionA.promise;
    });
    expect(screen.queryByText("Late release A torrent")).toBeNull();

    const inspectB = screen.getByRole("button", { name: "Inspect torrent" });
    fireEvent.click(inspectB);
    expect(await screen.findByText("Release B torrent")).toBeTruthy();
    const saveResult = createDeferred<boolean>();
    saveVerifiedAdultTorrentMock.mockReturnValueOnce(saveResult.promise);
    const saveButton = screen.getByRole("button", { name: "Save `.torrent`" });
    fireEvent.click(saveButton);
    fireEvent.click(saveButton);
    expect(saveVerifiedAdultTorrentMock).toHaveBeenCalledTimes(1);
    const torrentDialog = screen
      .getByText("Exact selected release")
      .closest('[role="dialog"]');
    fireEvent.click(
      within(torrentDialog as HTMLElement).getByRole("button", { name: "Close" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    selectSettings();
    await act(async () => {
      saveResult.resolve(true);
      await saveResult.promise;
    });
    expect(screen.queryByText("Verified Adult torrent file saved.")).toBeNull();
    expect(invalidateVerifiedAdultTorrentMock).toHaveBeenCalled();

    selectDiscover();
    selectAdultDiscover();
    const comparisonTrigger = screen.getByRole("button", {
      name: "Find releases: ADLT-123",
    });
    fireEvent.click(comparisonTrigger);
    const restoredReleaseB = screen.getByRole("button", { name: /release B/ });
    expect(restoredReleaseB.getAttribute("aria-pressed")).toBe("true");
    expect(fetchSukebeiAdultReleasesMock).toHaveBeenCalledTimes(1);
    expect(startVerifiedVrDownloadMock).not.toHaveBeenCalled();
  });

  it("dismisses pending Adult inspection by keyboard and backdrop with focus return", async () => {
    const expectedInfohash = "0123456789abcdef0123456789abcdef01234567";
    fetchJavdbCatalogMock.mockResolvedValue(javdbCatalogFixture("ADLT-123"));
    fetchSukebeiAdultReleasesMock.mockResolvedValue(
      sukebeiReleaseFixture([
        {
          infohash: expectedInfohash,
          itemId: "321",
          name: "ADLT-123 pending artifact",
        },
      ]),
    );
    const keyboardInspection = createDeferred<string[]>();
    const backdropInspection = createDeferred<string[]>();
    inspectSukebeiAdultTorrentMock
      .mockReturnValueOnce(keyboardInspection.promise)
      .mockReturnValueOnce(backdropInspection.promise);
    const releaseList = await openAdultReleaseComparison();
    fireEvent.click(within(releaseList).getByRole("button"));
    const inspectButton = screen.getByRole("button", {
      name: "Inspect torrent",
    });

    fireEvent.click(inspectButton);
    await screen.findByRole("heading", { name: "Inspecting verified torrent" });
    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => expect(document.activeElement).toBe(inspectButton));
    await act(async () => {
      keyboardInspection.resolve([
        "adult-1-1-321",
        "Late keyboard torrent",
        expectedInfohash,
        "5",
        "Late keyboard.mp4",
        "5",
      ]);
      await keyboardInspection.promise;
    });
    expect(screen.queryByText("Late keyboard torrent")).toBeNull();

    fireEvent.click(inspectButton);
    await screen.findByRole("heading", { name: "Inspecting verified torrent" });
    fireEvent.click(document.querySelector(".vr-torrent__backdrop") as Element);
    await waitFor(() => expect(document.activeElement).toBe(inspectButton));
    await act(async () => {
      backdropInspection.resolve([
        "adult-1-2-321",
        "Late backdrop torrent",
        expectedInfohash,
        "5",
        "Late backdrop.mp4",
        "5",
      ]);
      await backdropInspection.promise;
    });
    expect(screen.queryByText("Late backdrop torrent")).toBeNull();
    expect(screen.queryByRole("button", { name: "Save `.torrent`" })).toBeNull();
    expect(invalidateVerifiedAdultTorrentMock).toHaveBeenCalled();
  });

  it("restores completed Adult releases and the exact selection without another request", async () => {
    const exactReleaseName = "ADLT-123  Exact\t—\n特別版!?";
    fetchJavdbCatalogMock.mockResolvedValue(
      javdbCatalogFixture("ADLT-123", "Persistent Adult title"),
    );
    fetchSukebeiAdultReleasesMock.mockResolvedValue(
      sukebeiReleaseFixture([
        { name: exactReleaseName, seeders: 7, size: "4.5 GiB" },
        { name: "ADLT-123 alternate", seeders: 2, size: "6 GiB" },
      ]),
    );
    gallerySizes.discover = { width: 1088, height: 2408 };
    render(<App />);
    selectDiscover();
    selectAdultDiscover();
    fireEvent.change(
      screen.getByRole("textbox", { name: "Search product code" }),
      { target: { value: "ADLT-123" } },
    );
    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    let trigger = await screen.findByRole("button", {
      name: "Find releases: ADLT-123",
    });
    fireEvent.click(trigger);
    let releaseList = await screen.findByRole("list", {
      name: "Verified Adult releases for ADLT-123",
    });
    const exactRelease = Array.from(
      releaseList.querySelectorAll<HTMLElement>(".vr-releases__release-name"),
    ).find((releaseName) => releaseName.textContent === exactReleaseName);
    fireEvent.click(exactRelease?.closest("button") as HTMLButtonElement);

    const expectRestoredComparison = async () => {
      releaseList = await screen.findByRole("list", {
        name: "Verified Adult releases for ADLT-123",
      });
      expect(within(releaseList).getAllByRole("button")).toHaveLength(2);
      const selectedRow = Array.from(
        releaseList.querySelectorAll<HTMLElement>(
          ".vr-releases__release-name",
        ),
      )
        .find((releaseName) => releaseName.textContent === exactReleaseName)
        ?.closest("button");
      expect(selectedRow?.getAttribute("aria-pressed")).toBe("true");
      const selectedSummary = screen
        .getByRole("heading", { name: "Selected release" })
        .closest("section");
      expect(
        Array.from(selectedSummary?.querySelectorAll("dd") ?? []).some(
          (value) => value.textContent === exactReleaseName,
        ),
      ).toBe(true);
      expect(fetchSukebeiAdultReleasesMock).toHaveBeenCalledTimes(1);
    };

    await expectRestoredComparison();
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    await waitFor(() => expect(document.activeElement).toBe(trigger));
    fireEvent.click(trigger);
    await expectRestoredComparison();

    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    selectSettings();
    fireEvent.click(screen.getByRole("radio", { name: "Dark" }));
    selectDashboard();
    selectDiscover();
    resizeGallery("discover", 1528, 472);
    fireEvent.click(screen.getByRole("radio", { name: "Movies" }));
    selectAdultDiscover();
    trigger = screen.getByRole("button", {
      name: "Find releases: ADLT-123",
    });
    fireEvent.click(trigger);
    await expectRestoredComparison();
    expect(fetchJavdbCatalogMock).toHaveBeenCalledTimes(1);
  });

  it("clears Adult release selection on retry and restores focus after close, Escape, and backdrop dismissal", async () => {
    fetchJavdbCatalogMock.mockResolvedValue(javdbCatalogFixture("ADLT-123"));
    fetchSukebeiAdultReleasesMock.mockResolvedValue(
      sukebeiReleaseFixture([{ name: "Exact ADLT-123 release" }]),
    );
    render(<App />);
    selectDiscover();
    selectAdultDiscover();
    fireEvent.change(
      screen.getByRole("textbox", { name: "Search product code" }),
      { target: { value: "ADLT-123" } },
    );
    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    const trigger = await screen.findByRole("button", {
      name: "Find releases: ADLT-123",
    });

    fireEvent.click(trigger);
    let releaseList = await screen.findByRole("list", {
      name: "Verified Adult releases for ADLT-123",
    });
    fireEvent.click(within(releaseList).getByRole("button"));
    expect(screen.getByRole("heading", { name: "Selected release" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    releaseList = await screen.findByRole("list", {
      name: "Verified Adult releases for ADLT-123",
    });
    expect(screen.queryByRole("heading", { name: "Selected release" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    await waitFor(() => expect(document.activeElement).toBe(trigger));
    fireEvent.click(trigger);
    await screen.findByRole("list", {
      name: "Verified Adult releases for ADLT-123",
    });
    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => expect(document.activeElement).toBe(trigger));
    fireEvent.click(trigger);
    await screen.findByRole("list", {
      name: "Verified Adult releases for ADLT-123",
    });
    fireEvent.click(document.querySelector(".vr-releases__backdrop") as Element);
    await waitFor(() => expect(document.activeElement).toBe(trigger));
  });

  it("blocks stale Adult catalog and release work across searches, categories, navigation, retries, and dismissal", async () => {
    const firstCatalog = createDeferred<string>();
    const secondCatalog = createDeferred<string>();
    const categoryCatalog = createDeferred<string>();
    const navigationCatalog = createDeferred<string>();
    fetchJavdbCatalogMock
      .mockReturnValueOnce(firstCatalog.promise)
      .mockReturnValueOnce(secondCatalog.promise)
      .mockReturnValueOnce(categoryCatalog.promise)
      .mockReturnValueOnce(navigationCatalog.promise)
      .mockResolvedValueOnce(javdbCatalogFixture("ADLT-130"));
    const dismissedReleases = createDeferred<string>();
    fetchSukebeiAdultReleasesMock
      .mockReturnValueOnce(dismissedReleases.promise)
      .mockResolvedValueOnce("<rss>")
      .mockResolvedValueOnce(
        sukebeiReleaseFixture([{ name: "Current ADLT-130 release" }]),
      );
    render(<App />);
    selectDiscover();
    selectAdultDiscover();
    let codeInput = screen.getByRole("textbox", {
      name: "Search product code",
    });

    fireEvent.change(codeInput, { target: { value: "ADLT-123" } });
    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    fireEvent.change(codeInput, { target: { value: "ADLT-124" } });
    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    await act(async () => {
      firstCatalog.resolve(javdbCatalogFixture("ADLT-123", "Stale search"));
      await firstCatalog.promise;
    });
    expect(screen.queryByText("Stale search")).toBeNull();
    await act(async () => {
      secondCatalog.resolve(javdbCatalogFixture("ADLT-124", "Current search"));
      await secondCatalog.promise;
    });
    expect(await screen.findByText("Current search")).toBeTruthy();

    fireEvent.change(codeInput, { target: { value: "ADLT-125" } });
    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    fireEvent.click(screen.getByRole("radio", { name: "Movies" }));
    await act(async () => {
      categoryCatalog.resolve(javdbCatalogFixture("ADLT-125", "Stale category"));
      await categoryCatalog.promise;
    });
    selectAdultDiscover();
    expect(screen.queryByText("Stale category")).toBeNull();
    codeInput = screen.getByRole("textbox", { name: "Search product code" });

    fireEvent.change(codeInput, { target: { value: "ADLT-126" } });
    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    selectSettings();
    await act(async () => {
      navigationCatalog.resolve(
        javdbCatalogFixture("ADLT-126", "Stale navigation"),
      );
      await navigationCatalog.promise;
    });
    selectDiscover();
    expect(screen.queryByText("Stale navigation")).toBeNull();
    codeInput = screen.getByRole("textbox", { name: "Search product code" });

    fireEvent.change(codeInput, { target: { value: "ADLT-130" } });
    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    const trigger = await screen.findByRole("button", {
      name: "Find releases: ADLT-130",
    });
    fireEvent.click(trigger);
    await screen.findByRole("heading", { name: "Finding verified releases" });
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    await act(async () => {
      dismissedReleases.resolve(
        sukebeiReleaseFixture([{ name: "Dismissed ADLT-130 release" }]),
      );
      await dismissedReleases.promise;
    });
    expect(screen.queryByText("Dismissed ADLT-130 release")).toBeNull();

    fireEvent.click(trigger);
    await screen.findByRole("heading", {
      name: "Sukebei returned invalid release data",
    });
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(
      await screen.findByText("Current ADLT-130 release"),
    ).toBeTruthy();
  });

  it("shows 25 then 7 then 10 Adult items with valid pages and no refetch", async () => {
    const adultCatalogItemsFixture = Array.from({ length: 25 }, (_, index) => ({
      code: "ADLT-123",
      coverUrl: null,
      source: "JavDB" as const,
      title: `Adult fixture ${String(index + 1).padStart(2, "0")}`,
    }));
    fetchJavdbCatalogMock.mockResolvedValue(
      javdbCatalogFixture("ADLT-123", "Preserved Adult title"),
    );
    gallerySizes.discover = { width: 1088, height: 2408 };
    render(<App adultCatalogItemsFixture={adultCatalogItemsFixture} />);
    selectDiscover();
    selectAdultDiscover();
    fireEvent.change(
      screen.getByRole("textbox", { name: "Search product code" }),
      { target: { value: "ADLT-123" } },
    );
    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    expect(await screen.findByText("Adult fixture 01")).toBeTruthy();
    const gallery = document.querySelector('[data-gallery="discover"]');
    expect(gallery?.getAttribute("data-page-capacity")).toBe("25");
    expect(gallery?.getAttribute("data-page-count")).toBe("1");
    expect(gallery?.getAttribute("data-current-page")).toBe("1");
    expect(visibleCardCount("Adult result for ADLT-123")).toBe(25);
    expect(screen.getByText("Page 1 of 1")).toBeTruthy();
    expect(
      screen.getByRole("button", {
        name: "Previous Adult result for ADLT-123 page",
      }),
    ).toHaveProperty("disabled", true);
    expect(
      screen.getByRole("button", {
        name: "Next Adult result for ADLT-123 page",
      }),
    ).toHaveProperty("disabled", true);

    resizeGallery("discover", 1528, 472);
    expect(gallery?.getAttribute("data-page-capacity")).toBe("7");
    expect(gallery?.getAttribute("data-page-count")).toBe("4");
    expect(gallery?.getAttribute("data-current-page")).toBe("1");
    expect(visibleCardCount("Adult result for ADLT-123")).toBe(7);
    expect(screen.getByText("Page 1 of 4")).toBeTruthy();
    expect(
      screen.getByRole("button", {
        name: "Previous Adult result for ADLT-123 page",
      }),
    ).toHaveProperty("disabled", true);
    expect(
      screen.getByRole("button", {
        name: "Next Adult result for ADLT-123 page",
      }),
    ).toHaveProperty("disabled", false);

    resizeGallery("discover", 1088, 956);
    expect(gallery?.getAttribute("data-page-capacity")).toBe("10");
    expect(gallery?.getAttribute("data-page-count")).toBe("3");
    expect(gallery?.getAttribute("data-current-page")).toBe("1");
    expect(visibleCardCount("Adult result for ADLT-123")).toBe(10);
    expect(screen.getByText("Page 1 of 3")).toBeTruthy();
    expect(fetchJavdbCatalogMock).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("radio", { name: "Movies" }));
    selectAdultDiscover();
    selectSettings();
    fireEvent.click(screen.getByRole("radio", { name: "Dark" }));
    selectDashboard();
    selectDiscover();
    expect(
      (screen.getByRole("radio", { name: "Adult" }) as HTMLInputElement).checked,
    ).toBe(true);
    expect(screen.getByRole("textbox", { name: "Search product code" })).toHaveProperty(
      "value",
      "ADLT-123",
    );
    expect(screen.getByText("Adult fixture 01")).toBeTruthy();
    const restoredGallery = document.querySelector('[data-gallery="discover"]');
    expect(restoredGallery?.getAttribute("data-page-capacity")).toBe("10");
    expect(restoredGallery?.getAttribute("data-page-count")).toBe("3");
    expect(restoredGallery?.getAttribute("data-current-page")).toBe("1");
    expect(visibleCardCount("Adult result for ADLT-123")).toBe(10);
    expect(screen.getByText("Page 1 of 3")).toBeTruthy();
    expect(fetchJavdbCatalogMock).toHaveBeenCalledTimes(1);
  });

  it("shows distinct Adult provider failures and a safe no-verified-release state", async () => {
    fetchJavdbCatalogMock
      .mockRejectedValueOnce("adult_network_error")
      .mockResolvedValueOnce("<html>invalid</html>")
      .mockRejectedValueOnce("adult_source_unavailable")
      .mockRejectedValueOnce("adult_provider_error")
      .mockResolvedValueOnce('<div class="movie-list"></div>')
      .mockResolvedValueOnce(javdbCatalogFixture("ADLT-123"));
    fetchSukebeiAdultReleasesMock
      .mockRejectedValueOnce("adult_source_unavailable")
      .mockRejectedValueOnce("adult_network_error")
      .mockResolvedValueOnce("<rss>")
      .mockRejectedValueOnce("adult_provider_error")
      .mockResolvedValueOnce(
        sukebeiReleaseFixture([
          { name: "ADLT-1230 extension" },
          { name: "ADLT-123 + XYZ-7 pack" },
        ]),
      );
    render(<App />);
    selectDiscover();
    selectAdultDiscover();
    fireEvent.change(
      screen.getByRole("textbox", { name: "Search product code" }),
      { target: { value: "ADLT-123" } },
    );
    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    for (const heading of [
      "JavDB could not be reached",
      "JavDB returned invalid catalog data",
      "JavDB is unavailable",
      "JavDB could not complete the search",
    ]) {
      expect(await screen.findByRole("heading", { name: heading })).toBeTruthy();
      fireEvent.click(screen.getByRole("button", { name: "Retry search" }));
    }
    expect(
      await screen.findByRole("heading", { name: "No exact Adult title found" }),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    const trigger = await screen.findByRole("button", {
      name: "Find releases: ADLT-123",
    });
    fireEvent.click(trigger);
    for (const heading of [
      "Sukebei is unavailable",
      "Sukebei could not be reached",
      "Sukebei returned invalid release data",
      "Sukebei could not load releases",
    ]) {
      expect(await screen.findByRole("heading", { name: heading })).toBeTruthy();
      fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    }
    expect(
      await screen.findByRole("heading", { name: "No verified releases found" }),
    ).toBeTruthy();
    expect(screen.queryByText("ADLT-1230 extension")).toBeNull();
    expect(screen.queryByText("ADLT-123 + XYZ-7 pack")).toBeNull();
    expect(screen.queryByRole("heading", { name: "Selected release" })).toBeNull();
  });
});

describe("VR Discover and verified release comparison", () => {
  it("requires an explicit exact-code search and exposes only verified releases for explicit selection", async () => {
    const exactReleaseName =
      "【VR】 MdVr_00419  Director’s Cut\t—\n特別版!?";
    const ambiguousPackName = "MDVR-419 + ABC-123 pack";
    fetchJavdbCatalogMock.mockResolvedValue(
      javdbCatalogFixture("mdvr_00419", "Exact provider title"),
    );
    fetchSukebeiVrReleasesMock.mockResolvedValue(
      sukebeiReleaseFixture([
        { name: "Exact MDVR-419 release", seeders: 10, size: "12.5 GiB" },
        { name: exactReleaseName, seeders: 4, size: "8.0 GiB" },
        { name: "Neighbor MDVR-422 release", seeders: 500, size: "1 GiB" },
        { name: "Neighbor MDVR-430 release", seeders: 400, size: "2 GiB" },
        { name: "Neighbor MDVR-433 release", seeders: 300, size: "3 GiB" },
        { name: "Neighbor MDVR-374 release", seeders: 200, size: "4 GiB" },
        { name: "Extension MDVR-4190 release", seeders: 100, size: "5 GiB" },
        { name: "Embedded XMDVR-419 release", seeders: 90, size: "6 GiB" },
        { name: ambiguousPackName, seeders: 85, size: "6.5 GiB" },
        { name: "Candidate with no established code", seeders: 80, size: "7 GiB" },
      ]),
    );
    render(<App />);
    selectDiscover();
    selectVrDiscover();

    const codeInput = screen.getByRole("textbox", {
      name: "Search product code",
    });
    fireEvent.change(codeInput, { target: { value: "   " } });
    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    expect(
      screen.getByRole("alert", {
        name: "",
      }).textContent,
    ).toContain("Enter a valid VR product code");
    expect(fetchJavdbCatalogMock).not.toHaveBeenCalled();

    fireEvent.change(codeInput, { target: { value: "mdvr_00419" } });
    expect(fetchJavdbCatalogMock).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Search" }));

    const codeHeading = await screen.findByRole("heading", {
      level: 3,
      name: "MDVR-419",
    });
    const vrCard = codeHeading.closest("article");
    expect(vrCard).not.toBeNull();
    expect(within(vrCard as HTMLElement).getByText("Exact provider title")).toBeTruthy();
    expect(within(vrCard as HTMLElement).getByText("JavDB")).toBeTruthy();
    expect(fetchJavdbCatalogMock).toHaveBeenCalledWith({ code: "MDVR-419" });
    expect(fetchSukebeiVrReleasesMock).not.toHaveBeenCalled();
    const cover = vrCard?.querySelector("img");
    expect(cover).not.toBeNull();
    fireEvent.error(cover as HTMLImageElement);
    expect(within(vrCard as HTMLElement).getByText("Cover unavailable")).toBeTruthy();

    fireEvent.click(
      within(vrCard as HTMLElement).getByRole("button", {
        name: "Copy title: MDVR-419",
      }),
    );
    await waitFor(() =>
      expect(clipboardWriteMock).toHaveBeenCalledWith("MDVR-419"),
    );

    fireEvent.click(
      within(vrCard as HTMLElement).getByRole("button", {
        name: "Find releases: MDVR-419",
      }),
    );
    const releaseList = await screen.findByRole("list", {
      name: "Verified releases for MDVR-419",
    });
    expect(fetchSukebeiVrReleasesMock).toHaveBeenCalledWith({
      code: "MDVR-419",
    });
    expect(within(releaseList).getAllByRole("button")).toHaveLength(2);
    expect(screen.getByLabelText("Verified release totals").textContent).toBe(
      "2 verified releases2 from SukebeiRetry",
    );
    expect(screen.queryByText("Neighbor MDVR-422 release")).toBeNull();
    expect(screen.queryByText("Extension MDVR-4190 release")).toBeNull();
    expect(screen.queryByText("Embedded XMDVR-419 release")).toBeNull();
    expect(screen.queryByText(ambiguousPackName)).toBeNull();
    const exactReleaseRow = Array.from(
      releaseList.querySelectorAll<HTMLElement>(
        ".vr-releases__release-name",
      ),
    ).find((releaseName) => releaseName.textContent === exactReleaseName);
    expect(exactReleaseRow).toBeDefined();
    expect(screen.queryByRole("heading", { name: "Selected release" })).toBeNull();
    expect(
      screen.getByText("Select one verified release to compare its metadata."),
    ).toBeTruthy();

    fireEvent.click(
      within(releaseList).getByRole("button", {
        name: /Exact MDVR-419 release/,
      }),
    );
    let selectedSummary = screen
      .getByRole("heading", { name: "Selected release" })
      .closest("section");
    expect(selectedSummary).not.toBeNull();
    expect(within(selectedSummary as HTMLElement).getByText("MDVR-419")).toBeTruthy();
    expect(
      within(selectedSummary as HTMLElement).getByText("Exact MDVR-419 release"),
    ).toBeTruthy();

    fireEvent.click(exactReleaseRow?.closest("button") as HTMLButtonElement);
    selectedSummary = screen
      .getByRole("heading", { name: "Selected release" })
      .closest("section");
    const releaseNameTerm = within(selectedSummary as HTMLElement).getByText(
      "Release name",
    );
    expect(releaseNameTerm.parentElement?.querySelector("dd")?.textContent).toBe(
      exactReleaseName,
    );
    expect(selectedSummary?.textContent).not.toContain(ambiguousPackName);
    expect(screen.queryByRole("button", { name: /torrent|download|save/i })).toBeNull();

    for (const command of [
      "scan_movies",
      "query_movies_storage",
      "open_movie",
      "reveal_movie",
      "trash_movie",
    ]) {
      expect(invokeMock.mock.calls.some(([calledCommand]) => calledCommand === command)).toBe(
        false,
      );
    }
  });

  it("inspects and saves only a complete explicitly selected provider artifact", async () => {
    const exactReleaseName = "【VR】 MDVR-419  Exact — 特別版";
    const expectedInfohash = "0123456789abcdef0123456789abcdef01234567";
    fetchJavdbCatalogMock.mockResolvedValue(
      javdbCatalogFixture("MDVR-419", "Inspectable title"),
    );
    fetchSukebeiVrReleasesMock.mockResolvedValue(
      sukebeiReleaseFixture([
        { name: "MDVR-419 artifact unavailable" },
        {
          infohash: expectedInfohash,
          itemId: "123",
          name: exactReleaseName,
          seeders: 4,
          size: "8.0 GiB",
        },
      ]),
    );
    const inspectionResult = createDeferred<string[]>();
    inspectSukebeiVrTorrentMock.mockReturnValue(inspectionResult.promise);
    const verifiedInspection = [
      "inspection-123",
      "VR  — 作品",
      expectedInfohash,
      "12",
      "Folder/Part  1 — 映画.mkv",
      "5",
      "Folder/特別版  B.mp4",
      "7",
    ];
    const releaseList = await openVrReleaseComparison();

    fireEvent.click(
      within(releaseList).getByRole("button", {
        name: /MDVR-419 artifact unavailable/,
      }),
    );
    expect(
      screen.getByText(/no complete safe provider artifact identity/),
    ).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Inspect torrent" })).toBeNull();

    fireEvent.click(
      within(releaseList).getByRole("button", { name: /Exact — 特別版/ }),
    );
    const inspectButton = screen.getByRole("button", {
      name: "Inspect torrent",
    });
    expect(inspectSukebeiVrTorrentMock).not.toHaveBeenCalled();
    fireEvent.click(inspectButton);

    expect(
      await screen.findByRole("heading", { name: "Inspecting verified torrent" }),
    ).toBeTruthy();
    const loadingReleaseName = document.querySelector(
      ".vr-torrent__release-name",
    );
    expect(loadingReleaseName?.textContent).toBe(exactReleaseName);
    expect(screen.queryByRole("button", { name: "Save `.torrent`" })).toBeNull();

    await act(async () => {
      inspectionResult.resolve(verifiedInspection);
      await inspectionResult.promise;
    });

    expect(
      await screen.findByRole("heading", { name: "Complete file list" }),
    ).toBeTruthy();
    expect(inspectSukebeiVrTorrentMock).toHaveBeenCalledWith({
      code: "MDVR-419",
      expectedInfohash,
      providerItemId: "123",
      releaseName: exactReleaseName,
      torrentUrl: "https://sukebei.nyaa.si/download/123.torrent",
    });
    const torrentNameTerm = screen.getByText("Torrent name");
    expect(torrentNameTerm.parentElement?.querySelector("dd")?.textContent).toBe(
      "VR  — 作品",
    );
    expect(screen.getByText(expectedInfohash)).toBeTruthy();
    const totalSizeTerm = screen.getByText("Total size");
    expect(totalSizeTerm.parentElement?.querySelector("dd")?.textContent).toBe(
      "12 B (12 bytes)",
    );
    const fileList = screen.getByRole("list", {
      name: "Files in verified torrent for MDVR-419",
    });
    const fileRows = within(fileList).getAllByRole("listitem");
    expect(fileRows).toHaveLength(2);
    expect(fileRows[0].querySelector("span")?.textContent).toBe(
      "Folder/Part  1 — 映画.mkv",
    );
    expect(fileRows[0].querySelector("span:last-child")?.textContent).toBe(
      "5 B (5 bytes)",
    );
    expect(fileRows[1].querySelector("span")?.textContent).toBe(
      "Folder/特別版  B.mp4",
    );

    saveVerifiedVrTorrentMock.mockResolvedValueOnce(false).mockResolvedValueOnce(true);
    const saveButton = screen.getByRole("button", { name: "Save `.torrent`" });
    fireEvent.click(saveButton);
    await waitFor(() =>
      expect(saveVerifiedVrTorrentMock).toHaveBeenLastCalledWith({
        inspectionId: "inspection-123",
      }),
    );
    expect(screen.queryByText("Verified torrent file saved.")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();

    fireEvent.click(saveButton);
    expect(await screen.findByText("Verified torrent file saved.")).toBeTruthy();
    saveVerifiedVrTorrentMock.mockRejectedValueOnce("vr_torrent_save_failed");
    fireEvent.click(saveButton);
    expect(
      (
        await screen.findByRole("alert", {
          name: "",
        })
      ).textContent,
    ).toBe("The verified torrent file could not be saved.");
    const torrentDialog = screen
      .getByText("Exact selected release")
      .closest('[role="dialog"]');
    expect(torrentDialog).not.toBeNull();
    fireEvent.click(
      within(torrentDialog as HTMLElement).getByRole("button", { name: "Close" }),
    );
    await waitFor(() => expect(document.activeElement).toBe(inspectButton));

    expect(fetchSukebeiVrReleasesMock).toHaveBeenCalledTimes(1);
    expect(
      invokeMock.mock.calls.some(([command]) =>
        ["scan_movies", "query_movies_storage", "open_movie", "reveal_movie", "trash_movie"].includes(
          command,
        ),
      ),
    ).toBe(false);
  });

  it("keeps every torrent inspection failure local and retryable", async () => {
    const expectedInfohash = "0123456789abcdef0123456789abcdef01234567";
    fetchJavdbCatalogMock.mockResolvedValue(javdbCatalogFixture("MDVR-419"));
    fetchSukebeiVrReleasesMock.mockResolvedValue(
      sukebeiReleaseFixture([
        {
          infohash: expectedInfohash,
          itemId: "123",
          name: "MDVR-419 exact artifact",
        },
      ]),
    );
    for (const error of [
      "vr_torrent_source_unavailable",
      "vr_torrent_network_error",
      "vr_torrent_provider_error",
      "vr_torrent_malformed",
      "vr_torrent_unsupported",
      "vr_torrent_infohash_mismatch",
    ]) {
      inspectSukebeiVrTorrentMock.mockRejectedValueOnce(error);
    }
    const releaseList = await openVrReleaseComparison();
    fireEvent.click(within(releaseList).getByRole("button"));
    fireEvent.click(screen.getByRole("button", { name: "Inspect torrent" }));

    for (const heading of [
      "Torrent artifact is unavailable",
      "Torrent artifact could not be reached",
      "Torrent provider rejected the request",
      "Torrent artifact is malformed",
      "Torrent artifact is unsupported",
      "Torrent identity did not match",
    ]) {
      expect(await screen.findByRole("heading", { name: heading })).toBeTruthy();
      expect(screen.queryByRole("button", { name: "Save `.torrent`" })).toBeNull();
      if (heading !== "Torrent identity did not match") {
        fireEvent.click(
          screen.getByRole("button", { name: "Retry inspection" }),
        );
      }
    }
    expect(document.querySelector(".vr-releases__selection")).not.toBeNull();
  });

  it("invalidates late inspection and save responses across selection and dismissal", async () => {
    const expectedInfohash = "0123456789abcdef0123456789abcdef01234567";
    fetchJavdbCatalogMock.mockResolvedValue(javdbCatalogFixture("MDVR-419"));
    fetchSukebeiVrReleasesMock.mockResolvedValue(
      sukebeiReleaseFixture([
        {
          infohash: expectedInfohash,
          itemId: "123",
          name: "MDVR-419 release A",
        },
        {
          infohash: expectedInfohash,
          itemId: "124",
          name: "MDVR-419 release B",
        },
      ]),
    );
    const inspectionA = createDeferred<string[]>();
    inspectSukebeiVrTorrentMock
      .mockReturnValueOnce(inspectionA.promise)
      .mockResolvedValueOnce([
        "inspection-124",
        "Release B torrent",
        expectedInfohash,
        "7",
        "B/Exact file.mp4",
        "7",
      ]);
    const releaseList = await openVrReleaseComparison();
    const releaseA = within(releaseList).getByRole("button", {
      name: /release A/,
    });
    const releaseB = within(releaseList).getByRole("button", {
      name: /release B/,
    });
    fireEvent.click(releaseA);
    fireEvent.click(screen.getByRole("button", { name: "Inspect torrent" }));
    await screen.findByRole("heading", { name: "Inspecting verified torrent" });

    fireEvent.click(releaseB);
    expect(screen.queryByText("Exact selected release")).toBeNull();
    await act(async () => {
      inspectionA.resolve([
        "inspection-123",
        "Late release A torrent",
        expectedInfohash,
        "5",
        "A/Late file.mp4",
        "5",
      ]);
      await inspectionA.promise;
    });
    expect(screen.queryByText("Late release A torrent")).toBeNull();

    const inspectB = screen.getByRole("button", { name: "Inspect torrent" });
    fireEvent.click(inspectB);
    expect(await screen.findByText("Release B torrent")).toBeTruthy();
    const saveResult = createDeferred<boolean>();
    saveVerifiedVrTorrentMock.mockReturnValueOnce(saveResult.promise);
    const saveButton = screen.getByRole("button", { name: "Save `.torrent`" });
    fireEvent.click(saveButton);
    fireEvent.click(saveButton);
    expect(saveVerifiedVrTorrentMock).toHaveBeenCalledTimes(1);
    const torrentDialog = screen
      .getByText("Exact selected release")
      .closest('[role="dialog"]');
    fireEvent.click(
      within(torrentDialog as HTMLElement).getByRole("button", { name: "Close" }),
    );
    await act(async () => {
      saveResult.resolve(true);
      await saveResult.promise;
    });
    expect(screen.queryByText("Verified torrent file saved.")).toBeNull();
    expect(invalidateVerifiedVrTorrentMock).toHaveBeenCalled();
    expect(document.activeElement).toBe(inspectB);
  });

  it("dismisses pending torrent inspection by keyboard and restores its trigger", async () => {
    const expectedInfohash = "0123456789abcdef0123456789abcdef01234567";
    fetchJavdbCatalogMock.mockResolvedValue(javdbCatalogFixture("MDVR-419"));
    fetchSukebeiVrReleasesMock.mockResolvedValue(
      sukebeiReleaseFixture([
        {
          infohash: expectedInfohash,
          itemId: "123",
          name: "MDVR-419 pending artifact",
        },
      ]),
    );
    const pendingInspection = createDeferred<string[]>();
    inspectSukebeiVrTorrentMock.mockReturnValue(pendingInspection.promise);
    const releaseList = await openVrReleaseComparison();
    fireEvent.click(within(releaseList).getByRole("button"));
    const inspectButton = screen.getByRole("button", {
      name: "Inspect torrent",
    });
    fireEvent.click(inspectButton);
    await screen.findByRole("heading", { name: "Inspecting verified torrent" });

    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => expect(document.activeElement).toBe(inspectButton));
    await act(async () => {
      pendingInspection.resolve([
        "inspection-123",
        "Late closed torrent",
        expectedInfohash,
        "5",
        "Late file.mp4",
        "5",
      ]);
      await pendingInspection.promise;
    });
    expect(screen.queryByText("Late closed torrent")).toBeNull();
    expect(screen.queryByRole("button", { name: "Save `.torrent`" })).toBeNull();
  });

  it("keeps only the newest catalog result and blocks a late result after a category change", async () => {
    const firstCatalog = createDeferred<string>();
    const secondCatalog = createDeferred<string>();
    const closedCatalog = createDeferred<string>();
    fetchJavdbCatalogMock
      .mockReturnValueOnce(firstCatalog.promise)
      .mockReturnValueOnce(secondCatalog.promise)
      .mockReturnValueOnce(closedCatalog.promise);
    render(<App />);
    selectDiscover();
    selectVrDiscover();
    const codeInput = screen.getByRole("textbox", {
      name: "Search product code",
    });

    fireEvent.change(codeInput, { target: { value: "MDVR-419" } });
    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    fireEvent.change(codeInput, { target: { value: "MDVR-422" } });
    fireEvent.click(screen.getByRole("button", { name: "Search" }));

    await act(async () => {
      firstCatalog.resolve(javdbCatalogFixture("MDVR-419", "Stale title"));
      await firstCatalog.promise;
    });
    expect(screen.queryByRole("heading", { name: "MDVR-419" })).toBeNull();
    expect(screen.getByRole("heading", { name: "Searching JavDB" })).toBeTruthy();

    await act(async () => {
      secondCatalog.resolve(javdbCatalogFixture("MDVR-422", "Current title"));
      await secondCatalog.promise;
    });
    expect(
      await screen.findByRole("heading", { level: 3, name: "MDVR-422" }),
    ).toBeTruthy();

    fireEvent.change(codeInput, { target: { value: "MDVR-430" } });
    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    fireEvent.click(screen.getByRole("radio", { name: "Movies" }));
    await act(async () => {
      closedCatalog.resolve(javdbCatalogFixture("MDVR-430", "Closed title"));
      await closedCatalog.promise;
    });
    selectVrDiscover();
    expect(screen.queryByRole("heading", { name: "MDVR-430" })).toBeNull();
    expect(
      screen.getByRole("heading", {
        name: "Search for a VR title by product code",
      }),
    ).toBeTruthy();
  });

  it("dismisses pending comparison safely and restores focus without accepting a late response", async () => {
    fetchJavdbCatalogMock.mockResolvedValue(javdbCatalogFixture("MDVR-419"));
    const firstReleases = createDeferred<string>();
    const secondReleases = createDeferred<string>();
    fetchSukebeiVrReleasesMock
      .mockReturnValueOnce(firstReleases.promise)
      .mockReturnValueOnce(secondReleases.promise);
    render(<App />);
    selectDiscover();
    selectVrDiscover();
    fireEvent.change(
      screen.getByRole("textbox", { name: "Search product code" }),
      { target: { value: "MDVR-419" } },
    );
    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    const trigger = await screen.findByRole("button", {
      name: "Find releases: MDVR-419",
    });

    fireEvent.click(trigger);
    expect(
      await screen.findByRole("heading", { name: "Finding verified releases" }),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    await waitFor(() => expect(document.activeElement).toBe(trigger));
    await act(async () => {
      firstReleases.resolve(
        sukebeiReleaseFixture([{ name: "Late MDVR-419 release" }]),
      );
      await firstReleases.promise;
    });
    expect(screen.queryByText("Late MDVR-419 release")).toBeNull();

    fireEvent.click(trigger);
    await screen.findByRole("heading", { name: "Finding verified releases" });
    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => expect(document.activeElement).toBe(trigger));
    await act(async () => {
      secondReleases.resolve(
        sukebeiReleaseFixture([{ name: "Escaped MDVR-419 release" }]),
      );
      await secondReleases.promise;
    });
    expect(screen.queryByText("Escaped MDVR-419 release")).toBeNull();
  });

  it("shows distinct catalog and release provider failures and a safe accepted-only no-match state", async () => {
    fetchJavdbCatalogMock
      .mockRejectedValueOnce("vr_network_error")
      .mockResolvedValueOnce("<html>invalid</html>")
      .mockRejectedValueOnce("vr_source_unavailable")
      .mockRejectedValueOnce("vr_provider_error")
      .mockResolvedValueOnce('<div class="movie-list"></div>')
      .mockResolvedValueOnce(javdbCatalogFixture("MDVR-419"));
    fetchSukebeiVrReleasesMock
      .mockRejectedValueOnce("vr_source_unavailable")
      .mockRejectedValueOnce("vr_network_error")
      .mockResolvedValueOnce("<rss>")
      .mockRejectedValueOnce("vr_provider_error")
      .mockResolvedValueOnce(
        sukebeiReleaseFixture([
          { name: "Extension MDVR-4190 release", seeders: 999 },
          { name: "Embedded XMDVR-419 release", seeders: 998 },
        ]),
      );
    render(<App />);
    selectDiscover();
    selectVrDiscover();
    fireEvent.change(
      screen.getByRole("textbox", { name: "Search product code" }),
      { target: { value: "MDVR-419" } },
    );
    fireEvent.click(screen.getByRole("button", { name: "Search" }));

    for (const heading of [
      "JavDB could not be reached",
      "JavDB returned invalid catalog data",
      "JavDB is unavailable",
      "JavDB could not complete the search",
    ]) {
      expect(await screen.findByRole("heading", { name: heading })).toBeTruthy();
      fireEvent.click(screen.getByRole("button", { name: "Retry search" }));
    }
    expect(
      await screen.findByRole("heading", { name: "No exact VR title found" }),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    const trigger = await screen.findByRole("button", {
      name: "Find releases: MDVR-419",
    });
    fireEvent.click(trigger);

    for (const heading of [
      "Sukebei is unavailable",
      "Sukebei could not be reached",
      "Sukebei returned invalid release data",
      "Sukebei could not load releases",
    ]) {
      expect(await screen.findByRole("heading", { name: heading })).toBeTruthy();
      fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    }
    expect(
      await screen.findByRole("heading", { name: "No verified releases found" }),
    ).toBeTruthy();
    expect(screen.queryByText("Extension MDVR-4190 release")).toBeNull();
    expect(screen.queryByText("Embedded XMDVR-419 release")).toBeNull();
    expect(screen.queryByRole("heading", { name: "Selected release" })).toBeNull();
  });

  it("preserves independent Movies and VR state through navigation, appearance, and resize without duplicate requests", async () => {
    loadTmdbTokenMock.mockResolvedValue("saved-token");
    fetchMock.mockResolvedValue(
      jsonResponse({
        results: [
          {
            id: 501,
            title: "Preserved Movie",
            poster_path: null,
            release_date: "2026-08-03",
          },
        ],
      }),
    );
    fetchJavdbCatalogMock.mockResolvedValue(
      javdbCatalogFixture("MDVR-419", "Preserved VR title"),
    );
    render(<App />);
    selectDiscover();
    expect(
      await screen.findByRole("heading", { level: 3, name: "Preserved Movie" }),
    ).toBeTruthy();
    selectVrDiscover();
    fireEvent.change(
      screen.getByRole("textbox", { name: "Search product code" }),
      { target: { value: "MDVR-419" } },
    );
    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    expect(
      await screen.findByRole("heading", { level: 3, name: "MDVR-419" }),
    ).toBeTruthy();

    selectSettings();
    fireEvent.click(screen.getByRole("radio", { name: "Dark" }));
    selectDiscover();
    expect(
      (screen.getByRole("radio", { name: "VR" }) as HTMLInputElement).checked,
    ).toBe(true);
    expect(
      (
        screen.getByRole("textbox", {
          name: "Search product code",
        }) as HTMLInputElement
      ).value,
    ).toBe("MDVR-419");
    expect(screen.getByRole("heading", { level: 3, name: "MDVR-419" })).toBeTruthy();
    resizeGallery("discover", 520, 850);
    expect(fetchJavdbCatalogMock).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("radio", { name: "Movies" }));
    expect(
      screen.getByRole("heading", { level: 3, name: "Preserved Movie" }),
    ).toBeTruthy();
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("starts only explicitly selected files without sending destination or torrent identity", async () => {
    const exactReleaseName = "【VR】 MDVR-419  Exact\t—\n特別版";
    const expectedInfohash = "0123456789abcdef0123456789abcdef01234567";
    savedVrFolder = "/Volumes/VR — 作品";
    fetchJavdbCatalogMock.mockResolvedValue(javdbCatalogFixture("MDVR-419"));
    fetchSukebeiVrReleasesMock.mockResolvedValue(
      sukebeiReleaseFixture([
        {
          infohash: expectedInfohash,
          itemId: "123",
          name: exactReleaseName,
        },
      ]),
    );
    inspectSukebeiVrTorrentMock.mockResolvedValue([
      "inspection-123",
      "VR  — 作品",
      expectedInfohash,
      "12",
      "Folder/Part  1 — 映画.mkv",
      "5",
      "Folder/特別版  B.mp4",
      "7",
    ]);
    const startRequest = createDeferred<string>();
    startVerifiedVrDownloadMock.mockReturnValue(startRequest.promise);
    listVrDownloadsMock.mockResolvedValue(
      vrDownloadFixture({
        downloadedBytes: "7",
        releaseName: exactReleaseName,
        selectedFileCount: "1",
        speedBytesPerSecond: "0",
        state: "paused",
        totalBytes: "7",
        transferId: "transfer-123",
      }),
    );

    const releaseList = await openVrReleaseComparison();
    fireEvent.click(within(releaseList).getByRole("button"));
    fireEvent.click(screen.getByRole("button", { name: "Inspect torrent" }));
    const checkboxes = await screen.findAllByRole("checkbox");
    expect(checkboxes).toHaveLength(2);
    expect(checkboxes.every((checkbox) => !(checkbox as HTMLInputElement).checked)).toBe(
      true,
    );
    const startButton = screen.getByRole("button", { name: "Start download" });
    expect((startButton as HTMLButtonElement).disabled).toBe(true);

    expect(checkboxes[1].closest("label")?.textContent).toContain(
      "Folder/特別版  B.mp4",
    );
    fireEvent.click(checkboxes[1]);
    expect((startButton as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(startButton);
    fireEvent.click(startButton);
    expect(startVerifiedVrDownloadMock).toHaveBeenCalledOnce();
    expect(startVerifiedVrDownloadMock).toHaveBeenCalledWith({
      inspectionId: "inspection-123",
      selectedFileIds: [1],
    });
    expect(startVerifiedVrDownloadMock.mock.calls[0]?.[0]).toEqual({
      inspectionId: "inspection-123",
      selectedFileIds: [1],
    });

    await act(async () => {
      startRequest.resolve("transfer-123");
      await startRequest.promise;
    });
    expect(
      await screen.findByText("Selected files were added to Downloads."),
    ).toBeTruthy();
    expect(document.querySelector(".vr-torrent__release-name")?.textContent).toBe(
      exactReleaseName,
    );
    fireEvent.click(screen.getByRole("button", { name: "View Downloads" }));
    const downloadHeading = await screen.findByText(
      (_text, element) =>
        element?.tagName === "H2" && element.textContent === exactReleaseName,
    );
    const card = downloadHeading.closest("article");
    expect(card).not.toBeNull();
    expect(within(card as HTMLElement).getByText("1", { selector: "dd" })).toBeTruthy();
    expect(within(card as HTMLElement).getByText("100%")).toBeTruthy();
  });

  it("persists unavailable, changed, and cleared future-transfer VR folders", async () => {
    loadVrFolderMock.mockResolvedValue(["unavailable", "/missing/VR — 旧"]);
    chooseVrFolderMock.mockResolvedValue("/Volumes/VR — 新");
    render(<App />);
    selectSettings();

    expect(await screen.findByText("/missing/VR — 旧")).toBeTruthy();
    expect(
      screen.getByText(/Existing transfers will not fall back/),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Change VR folder" }));
    expect(await screen.findByText("/Volumes/VR — 新")).toBeTruthy();
    expect(chooseVrFolderMock).toHaveBeenCalledOnce();

    const vrFolderCard = screen
      .getByRole("heading", { name: "VR folder" })
      .closest("section");
    expect(vrFolderCard).not.toBeNull();
    fireEvent.click(
      within(vrFolderCard as HTMLElement).getByRole("button", {
        name: "Clear folder",
      }),
    );
    expect(await within(vrFolderCard as HTMLElement).findByText("No VR folder configured."))
      .toBeTruthy();
    expect(clearVrFolderMock).toHaveBeenCalledOnce();
  });

  it("isolates transfer actions and confirms cancellation while keeping files", async () => {
    const releaseA = "MDVR-419 Active — A";
    const releaseB = "MDVR-420 Paused — B";
    let downloadRows = [
      ...vrDownloadFixture({
        releaseName: releaseA,
        state: "downloading",
        transferId: "transfer-a",
      }),
      ...vrDownloadFixture({
        code: "MDVR-420",
        downloadedBytes: "3",
        releaseName: releaseB,
        speedBytesPerSecond: "0",
        state: "paused",
        transferId: "transfer-b",
      }),
    ];
    loadVrDownloadsMock.mockImplementation(() => Promise.resolve(downloadRows));
    listVrDownloadsMock.mockImplementation(() => Promise.resolve(downloadRows));
    pauseVrDownloadMock.mockImplementation(async () => {
      downloadRows[8] = "paused";
      downloadRows[7] = "0";
    });
    cancelVrDownloadMock.mockImplementation(async () => {
      downloadRows[8] = "cancelled";
      downloadRows[7] = "0";
    });
    dismissVrDownloadMock.mockImplementation(async () => {
      downloadRows = downloadRows.slice(13);
    });
    resumeVrDownloadMock.mockRejectedValueOnce("vr_download_failed");
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Downloads" }));

    const releaseAHeading = await screen.findByRole("heading", {
      level: 2,
      name: releaseA,
    });
    const cardA = releaseAHeading.closest("article") as HTMLElement;
    const cardB = screen
      .getByRole("heading", { level: 2, name: releaseB })
      .closest("article") as HTMLElement;
    fireEvent.click(within(cardA).getByRole("button", { name: "Pause" }));
    expect(await within(cardA).findByRole("button", { name: "Resume" })).toBeTruthy();
    expect(pauseVrDownloadMock).toHaveBeenCalledWith({ transferId: "transfer-a" });
    expect(within(cardB).getByRole("button", { name: "Resume" })).toBeTruthy();

    fireEvent.click(within(cardB).getByRole("button", { name: "Resume" }));
    expect(
      await within(cardB).findByText(/resume action could not be completed/),
    ).toBeTruthy();
    expect(within(cardA).queryByRole("alert")).toBeNull();

    const cancelTrigger = within(cardA).getByRole("button", { name: "Cancel" });
    cancelTrigger.focus();
    fireEvent.click(cancelTrigger);
    let confirmation = await screen.findByRole("alertdialog");
    expect(within(confirmation).getByText(/Downloaded files and partial data will remain/))
      .toBeTruthy();
    fireEvent.click(
      within(confirmation).getByRole("button", { name: "Keep downloading" }),
    );
    await waitFor(() => expect(document.activeElement).toBe(cancelTrigger));
    expect(cancelVrDownloadMock).not.toHaveBeenCalled();

    fireEvent.click(cancelTrigger);
    confirmation = await screen.findByRole("alertdialog");
    fireEvent.click(
      within(confirmation).getByRole("button", { name: "Cancel download" }),
    );
    const dismissButton = await within(cardA).findByRole("button", {
      name: "Dismiss",
    });
    await waitFor(() => expect(document.activeElement).toBe(dismissButton));
    expect(cancelVrDownloadMock).toHaveBeenCalledWith({ transferId: "transfer-a" });
    fireEvent.click(dismissButton);
    await waitFor(() => expect(screen.queryByText(releaseA)).toBeNull());
    expect(document.activeElement).toBe(
      screen.getByRole("button", { name: "Refresh" }),
    );
    expect(screen.getByRole("heading", { name: releaseB })).toBeTruthy();
    expect(dismissVrDownloadMock).toHaveBeenCalledWith({ transferId: "transfer-a" });
  });
});

describe("aggregate Adult and VR download limit and transfer summaries", () => {
  it("loads before transfers and applies finite replacement and Unlimited modes", async () => {
    const pendingLimit = createDeferred<string[]>();
    loadVrDownloadLimitMock.mockReturnValue(pendingLimit.promise);
    render(<App />);
    selectSettings();

    expect(
      screen.getByText("Loading the native-owned aggregate limit…"),
    ).toBeTruthy();
    expect(loadVrDownloadsMock).not.toHaveBeenCalled();

    await act(async () => {
      pendingLimit.resolve(["limited", "8"]);
      await pendingLimit.promise;
    });
    expect(await screen.findByText("Current limit: 8 MiB/s.")).toBeTruthy();
    expect(loadVrDownloadsMock).toHaveBeenCalledOnce();
    expect(
      (screen.getByRole("radio", { name: "Finite" }) as HTMLInputElement)
        .checked,
    ).toBe(true);

    const limitInput = screen.getByRole("spinbutton", {
      name: "Finite limit (MiB/s)",
    });
    fireEvent.change(limitInput, { target: { value: "12" } });
    const applyLimit = screen.getByRole("button", { name: "Apply limit" });
    fireEvent.click(applyLimit);
    fireEvent.click(applyLimit);
    expect(saveVrDownloadLimitMock).toHaveBeenCalledOnce();
    expect(saveVrDownloadLimitMock).toHaveBeenLastCalledWith({
      mibPerSecond: "12",
    });
    expect(
      await screen.findByText("Download limit applied at 12 MiB/s."),
    ).toBeTruthy();
    expect(screen.getByText("Current limit: 12 MiB/s.")).toBeTruthy();

    fireEvent.click(screen.getByRole("radio", { name: "Unlimited" }));
    fireEvent.click(screen.getByRole("button", { name: "Apply limit" }));
    expect(
      await screen.findByText("Downloads are now Unlimited."),
    ).toBeTruthy();
    expect(saveVrDownloadLimitMock).toHaveBeenLastCalledWith({
      mibPerSecond: null,
    });
    expect(screen.getByText("Current limit: Unlimited.")).toBeTruthy();
  });

  it("rejects invalid finite values before native dispatch", async () => {
    loadVrDownloadLimitMock.mockResolvedValue(["limited", "8"]);
    render(<App />);
    selectSettings();
    const input = await screen.findByRole("spinbutton", {
      name: "Finite limit (MiB/s)",
    });

    for (const value of ["", "0", "-1", "1.5", "4096"]) {
      fireEvent.change(input, { target: { value } });
      fireEvent.click(screen.getByRole("button", { name: "Apply limit" }));
      expect(
        screen.getByText("Enter a whole-number limit from 1 to 4095 MiB/s."),
      ).toBeTruthy();
    }
    expect(saveVrDownloadLimitMock).not.toHaveBeenCalled();
    expect(screen.getByText("Current limit: 8 MiB/s.")).toBeTruthy();
  });

  it("keeps the previous limit on save and apply failures and routes attention to Settings", async () => {
    loadVrDownloadLimitMock.mockResolvedValue(["limited", "8"]);
    saveVrDownloadLimitMock
      .mockRejectedValueOnce("vr_download_limit_storage_failed")
      .mockRejectedValueOnce("vr_download_limit_apply_failed");
    render(<App />);
    selectSettings();
    const input = await screen.findByRole("spinbutton", {
      name: "Finite limit (MiB/s)",
    });

    fireEvent.change(input, { target: { value: "12" } });
    fireEvent.click(screen.getByRole("button", { name: "Apply limit" }));
    expect(
      await screen.findByText(
        "The download limit could not be saved. The previous limit remains active.",
      ),
    ).toBeTruthy();
    expect(screen.getByText("Current limit: 8 MiB/s.")).toBeTruthy();

    fireEvent.change(input, { target: { value: "16" } });
    fireEvent.click(screen.getByRole("button", { name: "Apply limit" }));
    expect(
      await screen.findByText(
        "The download limit could not be applied. The previous limit remains active.",
      ),
    ).toBeTruthy();
    expect(screen.getByText("Current limit: 8 MiB/s.")).toBeTruthy();

    selectDashboard();
    const downloads = screen.getByRole("region", { name: "Downloads" });
    fireEvent.click(
      within(downloads).getByRole("button", {
        name: "Open Download Settings",
      }),
    );
    expect(
      screen.getByRole("heading", { level: 1, name: "Settings" }),
    ).toBeTruthy();
  });

  it("blocks transfer restoration after a limit load failure and recovers in order", async () => {
    loadVrDownloadLimitMock
      .mockRejectedValueOnce("vr_download_limit_storage_failed")
      .mockResolvedValueOnce(["limited", "4"]);
    render(<App />);

    const downloads = await screen.findByRole("region", { name: "Downloads" });
    expect(
      within(downloads).getByRole("heading", {
        level: 3,
        name: "Transfers need attention",
      }),
    ).toBeTruthy();
    expect(loadVrDownloadsMock).not.toHaveBeenCalled();
    fireEvent.click(
      within(downloads).getByRole("button", {
        name: "Open Download Settings",
      }),
    );
    expect(
      screen.getByText(
        "The aggregate limit could not be loaded or applied. Eligible saved transfers remain non-running.",
      ),
    ).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Retry limit" }));
    expect(await screen.findByText("Current limit: 4 MiB/s.")).toBeTruthy();
    expect(loadVrDownloadLimitMock).toHaveBeenCalledTimes(2);
    expect(loadVrDownloadsMock).toHaveBeenCalledOnce();
  });

  it("keeps a damaged V2 Adult row category-unknown, inert, and dismissible", async () => {
    loadVrDownloadsMock.mockResolvedValue(
      vrDownloadFixture({
        category: "unknown",
        code: "ADLT-123",
        downloadedBytes: "0",
        isCurrentFolder: "false",
        releaseName: "【Adult】 ADLT-123 damaged V2 record",
        selectedFileCount: "0",
        speedBytesPerSecond: "0",
        state: "offline",
        totalBytes: "0",
        transferId: "corrupt-adult-v2",
      }),
    );
    listVrDownloadsMock.mockResolvedValue([]);
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Downloads" }));

    const heading = await screen.findByRole("heading", {
      name: "【Adult】 ADLT-123 damaged V2 record",
    });
    const card = heading.closest("article") as HTMLElement;
    expect(within(card).getByText("Category unavailable · ADLT-123")).toBeTruthy();
    expect(within(card).getByText("Offline")).toBeTruthy();
    expect(within(card).queryByText("Adult · ADLT-123")).toBeNull();
    expect(within(card).queryByText("VR · ADLT-123")).toBeNull();
    expect(within(card).queryByRole("button", { name: "Pause" })).toBeNull();
    expect(within(card).queryByRole("button", { name: "Resume" })).toBeNull();
    expect(within(card).queryByRole("button", { name: "Cancel" })).toBeNull();
    expect(
      within(card).queryByRole("button", { name: "Organize files" }),
    ).toBeNull();

    fireEvent.click(within(card).getByRole("button", { name: "Dismiss" }));
    await waitFor(() => expect(heading.isConnected).toBe(false));
    expect(dismissVrDownloadMock).toHaveBeenCalledWith({
      transferId: "corrupt-adult-v2",
    });
    expect(pauseVrDownloadMock).not.toHaveBeenCalled();
    expect(resumeVrDownloadMock).not.toHaveBeenCalled();
    expect(cancelVrDownloadMock).not.toHaveBeenCalled();
    expect(previewVrOrganizationMock).not.toHaveBeenCalled();
  });

  it("summarizes only the current native snapshot without extra view work", async () => {
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 720,
    });
    Object.defineProperty(window, "innerHeight", {
      configurable: true,
      value: 520,
    });
    loadVrDownloadLimitMock.mockResolvedValue(["limited", "8"]);
    const initialRows = [
      ...vrDownloadFixture({
        category: "adult",
        code: "ADLT-123",
        releaseName: "Active A",
        speedBytesPerSecond: "1024",
        state: "downloading",
        transferId: "active-a",
      }),
      ...vrDownloadFixture({
        releaseName: "Active B",
        speedBytesPerSecond: "2048",
        state: "downloading",
        transferId: "active-b",
      }),
      ...vrDownloadFixture({
        category: "movie",
        code: "tt0123456",
        releaseName: "Exact  Movie — 特別版",
        speedBytesPerSecond: "1024",
        state: "downloading",
        transferId: "active-movie",
      }),
      ...vrDownloadFixture({
        releaseName: "Queued",
        speedBytesPerSecond: "8192",
        state: "queued",
        transferId: "queued",
      }),
      ...vrDownloadFixture({
        releaseName: "Paused",
        speedBytesPerSecond: "4096",
        state: "paused",
        transferId: "paused",
      }),
      ...vrDownloadFixture({
        releaseName: "Completed",
        speedBytesPerSecond: "4096",
        state: "completed",
        transferId: "completed",
      }),
      ...vrDownloadFixture({
        releaseName: "Offline",
        speedBytesPerSecond: "4096",
        state: "offline",
        transferId: "offline",
      }),
      ...vrDownloadFixture({
        releaseName: "Failed",
        speedBytesPerSecond: "4096",
        state: "failed",
        transferId: "failed",
      }),
      ...vrDownloadFixture({
        releaseName: "Cancelled",
        speedBytesPerSecond: "4096",
        state: "cancelled",
        transferId: "cancelled",
      }),
    ];
    loadVrDownloadsMock.mockResolvedValue(initialRows);
    render(<App />);

    const dashboard = await screen.findByRole("region", { name: "Downloads" });
    const dashboardSummary = within(dashboard).getByLabelText(
      "Transfer summary",
    );
    const expectedDashboardValues = [
      ["Active", "3"],
      ["Paused", "1"],
      ["Completed", "1"],
      ["Needs attention", "2"],
      ["Download speed", "4.0 KiB/s"],
      ["Limit", "8 MiB/s"],
    ];
    for (const [label, value] of expectedDashboardValues) {
      const item = within(dashboardSummary).getByText(label).closest("div");
      expect(item).not.toBeNull();
      expect(within(item as HTMLElement).getByText(value)).toBeTruthy();
    }

    fireEvent.click(
      within(dashboard).getByRole("button", { name: "Open Downloads" }),
    );
    const aggregate = screen.getByLabelText("Downloads aggregate status");
    expect(within(aggregate).getByText("3", { selector: "dd" })).toBeTruthy();
    expect(within(aggregate).getByText("4.0 KiB/s")).toBeTruthy();
    expect(screen.getByText("Adult · ADLT-123")).toBeTruthy();
    const movieHeading = screen.getByRole("heading", {
      name: "Exact Movie — 特別版",
    });
    const movieCard = movieHeading.closest("article") as HTMLElement;
    expect(within(movieCard).getByText("Movie · tt0123456")).toBeTruthy();
    expect(within(movieCard).queryByRole("button", { name: "Organize files" })).toBeNull();
    expect(screen.getAllByText(/VR · MDVR-/).length).toBeGreaterThan(0);

    selectSettings();
    fireEvent.click(screen.getByRole("radio", { name: "Dark" }));
    selectDashboard();
    fireEvent(window, new Event("resize"));
    expect(loadVrDownloadLimitMock).toHaveBeenCalledOnce();
    expect(loadVrDownloadsMock).toHaveBeenCalledOnce();
    expect(listVrDownloadsMock).not.toHaveBeenCalled();
    expect(scanMoviesMock).not.toHaveBeenCalled();
    expect(scanVrLibraryMock).not.toHaveBeenCalled();
    expect(queryMoviesStorageMock).not.toHaveBeenCalled();
    expect(queryVrStorageMock).not.toHaveBeenCalled();
    expect(fetchJavdbCatalogMock).not.toHaveBeenCalled();
    expect(fetchSukebeiVrReleasesMock).not.toHaveBeenCalled();
  });

  it("uses one poll for both summaries and reports zero without downloading transfers", async () => {
    vi.useFakeTimers();
    const activeRows = vrDownloadFixture({
      releaseName: "Active",
      speedBytesPerSecond: "1024",
      state: "downloading",
      transferId: "active",
    });
    const pausedRows = vrDownloadFixture({
      releaseName: "Paused",
      speedBytesPerSecond: "9999",
      state: "paused",
      transferId: "active",
    });
    loadVrDownloadsMock.mockResolvedValue(activeRows);
    listVrDownloadsMock.mockResolvedValue(pausedRows);
    render(<App />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    fireEvent.click(screen.getByRole("button", { name: "Downloads" }));
    let aggregate = screen.getByLabelText("Downloads aggregate status");
    expect(within(aggregate).getByText("1.0 KiB/s")).toBeTruthy();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(listVrDownloadsMock).toHaveBeenCalledOnce();
    aggregate = screen.getByLabelText("Downloads aggregate status");
    expect(within(aggregate).getByText("0 B/s")).toBeTruthy();
    selectDashboard();
    expect(
      within(screen.getByLabelText("Transfer summary")).getByText("0 B/s"),
    ).toBeTruthy();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(listVrDownloadsMock).toHaveBeenCalledOnce();
  });

  it("ignores a late save result after relaunch", async () => {
    const staleSave = createDeferred<string[]>();
    loadVrDownloadLimitMock
      .mockResolvedValueOnce(["unlimited"])
      .mockResolvedValueOnce(["limited", "4"]);
    saveVrDownloadLimitMock.mockReturnValueOnce(staleSave.promise);
    render(<App />);
    selectSettings();
    await screen.findByText("Current limit: Unlimited.");
    fireEvent.click(screen.getByRole("radio", { name: "Finite" }));
    fireEvent.change(
      screen.getByRole("spinbutton", { name: "Finite limit (MiB/s)" }),
      { target: { value: "8" } },
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply limit" }));
    expect(await screen.findByRole("button", { name: "Applying…" })).toBeTruthy();

    cleanup();
    render(<App />);
    selectSettings();
    expect(await screen.findByText("Current limit: 4 MiB/s.")).toBeTruthy();
    await act(async () => {
      staleSave.resolve(["limited", "8"]);
      await staleSave.promise;
    });
    expect(screen.getByText("Current limit: 4 MiB/s.")).toBeTruthy();
    expect(screen.queryByText("Current limit: 8 MiB/s.")).toBeNull();
  });
});

describe("completed download organization", () => {
  it("exposes only native-eligible rows and supports cancel, Escape, close, and focus return", async () => {
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 720,
    });
    Object.defineProperty(window, "innerHeight", {
      configurable: true,
      value: 520,
    });
    loadVrDownloadsMock.mockResolvedValue([
      ...vrDownloadFixture({
        canOrganize: "true",
        downloadedBytes: "10",
        releaseName: "Eligible MDVR-419",
        speedBytesPerSecond: "0",
        state: "completed",
        transferId: "eligible",
      }),
      ...vrDownloadFixture({
        downloadedBytes: "10",
        isCurrentFolder: "false",
        releaseName: "Old-folder completion",
        speedBytesPerSecond: "0",
        state: "completed",
        transferId: "old-folder",
      }),
      ...vrDownloadFixture({
        downloadedBytes: "10",
        organizationRelativeDirectory: "MDVR-419/",
        organizationStatus: "organized",
        releaseName: "Already organized",
        speedBytesPerSecond: "0",
        state: "completed",
        transferId: "organized",
      }),
      ...vrDownloadFixture({
        downloadedBytes: "3",
        releaseName: "Paused transfer",
        speedBytesPerSecond: "0",
        state: "paused",
        transferId: "paused",
      }),
    ]);
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Downloads" }));
    const eligibleCard = (
      await screen.findByRole("heading", { name: "Eligible MDVR-419" })
    ).closest("article") as HTMLElement;
    expect(screen.getAllByRole("button", { name: "Organize files" })).toHaveLength(1);
    const trigger = within(eligibleCard).getByRole("button", {
      name: "Organize files",
    });

    fireEvent.click(trigger);
    let confirmation = await screen.findByRole("alertdialog");
    expect(
      within(confirmation).getByRole("heading", {
        name: "Organize MDVR-419 files?",
      }),
    ).toBeTruthy();
    expect(within(confirmation).getByText("Source/MDVR-419.mp4")).toBeTruthy();
    expect(
      within(confirmation).getByText("Move to: MDVR-419/MDVR-419.mp4"),
    ).toBeTruthy();
    const cancel = within(confirmation).getByRole("button", { name: "Cancel" });
    await waitFor(() => expect(document.activeElement).toBe(cancel));
    fireEvent.click(cancel);
    await waitFor(() => expect(document.activeElement).toBe(trigger));
    expect(screen.queryByRole("alertdialog")).toBeNull();

    fireEvent.click(trigger);
    confirmation = await screen.findByRole("alertdialog");
    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
    await waitFor(() => expect(document.activeElement).toBe(trigger));

    fireEvent.click(trigger);
    confirmation = await screen.findByRole("alertdialog");
    fireEvent.click(
      within(confirmation).getByRole("button", {
        name: "Close organization preview",
      }),
    );
    await waitFor(() => expect(document.activeElement).toBe(trigger));

    fireEvent.click(trigger);
    await screen.findByRole("alertdialog");
    const backdrop = document.querySelector(".trash-dialog__backdrop");
    if (backdrop === null) {
      throw new Error("The organization confirmation backdrop was not rendered.");
    }
    fireEvent.pointerDown(backdrop);
    fireEvent.click(backdrop);
    await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
    await waitFor(() => expect(document.activeElement).toBe(trigger));

    expect(previewVrOrganizationMock).toHaveBeenCalledTimes(4);
    expect(previewVrOrganizationMock).toHaveBeenLastCalledWith({
      transferId: "eligible",
    });
    expect(applyVrOrganizationMock).not.toHaveBeenCalled();
    await waitFor(() => expect(dismissVrOrganizationMock).toHaveBeenCalledTimes(4));
    for (const call of dismissVrOrganizationMock.mock.calls) {
      expect(call).toEqual([]);
    }

    selectSettings();
    fireEvent.click(screen.getByRole("radio", { name: "Dark" }));
    fireEvent(window, new Event("resize"));
    expect(previewVrOrganizationMock).toHaveBeenCalledTimes(4);
    expect(listVrDownloadsMock).not.toHaveBeenCalled();
  });

  it("exposes organization only for the native-eligible completed Movie row", async () => {
    loadVrDownloadsMock.mockResolvedValue([
      ...vrDownloadFixture({
        canOrganize: "true",
        category: "movie",
        code: "tt0123456",
        downloadedBytes: "10",
        releaseName: "Eligible Movie",
        speedBytesPerSecond: "0",
        state: "completed",
        transferId: "eligible-movie",
      }),
      ...vrDownloadFixture({
        category: "movie",
        code: "tt0123457",
        downloadedBytes: "4",
        releaseName: "Queued Movie",
        state: "queued",
        transferId: "queued-movie",
      }),
      ...vrDownloadFixture({
        category: "movie",
        code: "tt0123458",
        downloadedBytes: "10",
        isCurrentFolder: "false",
        releaseName: "Old-folder Movie",
        speedBytesPerSecond: "0",
        state: "completed",
        transferId: "old-movie",
      }),
      ...vrDownloadFixture({
        category: "movie",
        code: "tt0123459",
        downloadedBytes: "10",
        organizationRelativeDirectory: "Organized Movie (1999)/",
        organizationStatus: "organized",
        releaseName: "Organized Movie",
        speedBytesPerSecond: "0",
        state: "completed",
        transferId: "organized-movie",
      }),
      ...vrDownloadFixture({
        category: "movie",
        code: "tt0123460",
        downloadedBytes: "10",
        releaseName: "Failed Movie",
        speedBytesPerSecond: "0",
        state: "failed",
        transferId: "failed-movie",
      }),
    ]);
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Downloads" }));

    const eligibleCard = (
      await screen.findByRole("heading", { name: "Eligible Movie" })
    ).closest("article") as HTMLElement;
    expect(screen.getAllByRole("button", { name: "Organize files" })).toHaveLength(1);
    fireEvent.click(
      within(eligibleCard).getByRole("button", { name: "Organize files" }),
    );
    expect(previewVrOrganizationMock).toHaveBeenCalledWith({
      transferId: "eligible-movie",
    });
  });

  it("applies one exact plan, refreshes Library and storage once, and dismisses without organizing again", async () => {
    savedVrFolder = "/VR";
    const initialRows = vrDownloadFixture({
      canOrganize: "true",
      downloadedBytes: "10",
      releaseName: "Exact completed release",
      speedBytesPerSecond: "0",
      state: "completed",
      transferId: "transfer-419",
    });
    const organizedRows = vrDownloadFixture({
      downloadedBytes: "10",
      organizationRelativeDirectory: "MDVR-419/",
      organizationStatus: "organized",
      releaseName: "Exact completed release",
      speedBytesPerSecond: "0",
      state: "completed",
      transferId: "transfer-419",
    });
    loadVrDownloadsMock.mockResolvedValue(initialRows);
    listVrDownloadsMock.mockResolvedValueOnce(organizedRows);
    previewVrOrganizationMock.mockResolvedValue([
      "plan-419",
      "transfer-419",
      "MDVR-419",
      "1",
      "3",
      "move",
      "Source/MDVR-419  —  映画.MKV",
      "MDVR-419/MDVR-419.MKV",
      "media-unchanged",
      "MDVR-419/MDVR-419 - Part 2.mp4",
      "MDVR-419/MDVR-419 - Part 2.mp4",
      "non-media-unchanged",
      "Source/notes  —  exact.txt",
      "",
    ]);
    const applyRequest = createDeferred<void>();
    applyVrOrganizationMock.mockReturnValue(applyRequest.promise);
    scanVrLibraryMock
      .mockResolvedValueOnce(["/VR/Source/MDVR-419  —  映画.MKV", "10"])
      .mockResolvedValueOnce(["/VR/MDVR-419/MDVR-419.MKV", "10"]);
    render(<App />);
    await waitFor(() => {
      expect(scanVrLibraryMock).toHaveBeenCalledOnce();
      expect(queryVrStorageMock).toHaveBeenCalledOnce();
    });
    fireEvent.click(screen.getByRole("button", { name: "Downloads" }));
    fireEvent.click(await screen.findByRole("button", { name: "Organize files" }));
    const confirmation = await screen.findByRole("alertdialog");
    expect(
      within(confirmation).getByText("Unchanged non-media file"),
    ).toBeTruthy();
    const apply = within(confirmation).getByRole("button", {
      name: "Organize 1 file",
    });
    fireEvent.click(apply);
    fireEvent.click(apply);
    expect(applyVrOrganizationMock).toHaveBeenCalledOnce();
    expect(applyVrOrganizationMock).toHaveBeenCalledWith({ planId: "plan-419" });
    expect(previewVrOrganizationMock).toHaveBeenCalledWith({
      transferId: "transfer-419",
    });
    expect(
      await within(confirmation).findByRole("button", { name: "Organizing…" }),
    ).toBeTruthy();

    await act(async () => {
      applyRequest.resolve();
      await applyRequest.promise;
    });
    const organizedCard = (
      await screen.findByRole("heading", { name: "Exact completed release" })
    ).closest("article") as HTMLElement;
    expect(within(organizedCard).getByText("Organized")).toBeTruthy();
    expect(within(organizedCard).getByText("MDVR-419/")).toBeTruthy();
    expect(within(organizedCard).queryByRole("button", { name: "Organize files" }))
      .toBeNull();
    const dismiss = within(organizedCard).getByRole("button", { name: "Dismiss" });
    await waitFor(() => expect(document.activeElement).toBe(dismiss));
    await waitFor(() => {
      expect(listVrDownloadsMock).toHaveBeenCalledOnce();
      expect(scanVrLibraryMock).toHaveBeenCalledTimes(2);
      expect(queryVrStorageMock).toHaveBeenCalledTimes(2);
    });

    listVrDownloadsMock.mockResolvedValueOnce([]);
    fireEvent.click(dismiss);
    await waitFor(() => expect(screen.queryByText("Exact completed release")).toBeNull());
    expect(dismissVrDownloadMock).toHaveBeenCalledWith({
      transferId: "transfer-419",
    });
    expect(applyVrOrganizationMock).toHaveBeenCalledOnce();
  });

  it("previews the exact Movie name and refreshes only Movies after apply", async () => {
    savedMoviesFolder = "/Movies";
    const releaseName = "映画  —  Exact Edition";
    loadVrDownloadsMock.mockResolvedValue(
      vrDownloadFixture({
        canOrganize: "true",
        category: "movie",
        code: "tt0123456",
        downloadedBytes: "10",
        releaseName,
        speedBytesPerSecond: "0",
        state: "completed",
        transferId: "movie-transfer-123",
      }),
    );
    listVrDownloadsMock.mockResolvedValueOnce(
      vrDownloadFixture({
        category: "movie",
        code: "tt0123456",
        downloadedBytes: "10",
        organizationRelativeDirectory: `${releaseName} (1999)/`,
        organizationStatus: "organized",
        releaseName,
        speedBytesPerSecond: "0",
        state: "completed",
        transferId: "movie-transfer-123",
      }),
    );
    previewVrOrganizationMock.mockResolvedValueOnce([
      "movie-plan-123",
      "movie-transfer-123",
      "tt0123456",
      "1",
      "2",
      "move",
      "Source/Provider  —  映画.MKV",
      `${releaseName} (1999)/${releaseName} (1999).MKV`,
      "non-media-unchanged",
      "Source/notes  —  exact.txt",
      "",
    ]);
    render(<App />);
    await waitFor(() => {
      expect(scanMoviesMock).toHaveBeenCalledOnce();
      expect(queryMoviesStorageMock).toHaveBeenCalledOnce();
    });

    fireEvent.click(screen.getByRole("button", { name: "Downloads" }));
    const releaseHeading = await screen.findByRole("heading", {
      name: "映画 — Exact Edition",
    });
    expect(releaseHeading.textContent).toBe(releaseName);
    const card = releaseHeading.closest("article") as HTMLElement;
    fireEvent.click(within(card).getByRole("button", { name: "Organize files" }));
    const confirmation = await screen.findByRole("alertdialog");
    expect(
      within(confirmation).getByText(
        (_, element) =>
          element?.tagName === "SPAN" &&
          element.textContent ===
            `Move to: ${releaseName} (1999)/${releaseName} (1999).MKV`,
      ),
    ).toBeTruthy();
    expect(within(confirmation).getByText("Unchanged non-media file")).toBeTruthy();
    fireEvent.click(
      within(confirmation).getByRole("button", { name: "Organize 1 file" }),
    );

    const organizedHeading = await screen.findByRole("heading", {
      name: "映画 — Exact Edition",
    });
    expect(organizedHeading.textContent).toBe(releaseName);
    const organizedCard = organizedHeading.closest("article") as HTMLElement;
    expect(within(organizedCard).getByText("Organized")).toBeTruthy();
    expect(
      within(organizedCard).getByText(
        (_, element) =>
          element?.tagName === "DD" &&
          element.textContent === `${releaseName} (1999)/`,
      ),
    ).toBeTruthy();
    await waitFor(() => {
      expect(scanMoviesMock).toHaveBeenCalledTimes(2);
      expect(queryMoviesStorageMock).toHaveBeenCalledTimes(2);
    });
    expect(scanAdultLibraryMock).not.toHaveBeenCalled();
    expect(queryAdultStorageMock).not.toHaveBeenCalled();
    expect(scanVrLibraryMock).not.toHaveBeenCalled();
    expect(queryVrStorageMock).not.toHaveBeenCalled();
    expect(applyVrOrganizationMock).toHaveBeenCalledWith({
      planId: "movie-plan-123",
    });
  });

  it("reloads Movie attention after apply failure without refreshing Movies", async () => {
    savedMoviesFolder = "/Movies";
    const releaseName = "Recoverable Movie";
    loadVrDownloadsMock.mockResolvedValue(
      vrDownloadFixture({
        canOrganize: "true",
        category: "movie",
        code: "tt0123456",
        downloadedBytes: "10",
        releaseName,
        speedBytesPerSecond: "0",
        state: "completed",
        transferId: "movie-recovery-123",
      }),
    );
    previewVrOrganizationMock.mockResolvedValueOnce([
      "movie-recovery-plan",
      "movie-recovery-123",
      "tt0123456",
      "1",
      "1",
      "move",
      "Source/Provider Movie.mp4",
      `${releaseName} (1999)/${releaseName} (1999).mp4`,
    ]);
    applyVrOrganizationMock.mockRejectedValueOnce("vr_organization_failed");
    listVrDownloadsMock.mockResolvedValueOnce(
      vrDownloadFixture({
        canOrganize: "true",
        category: "movie",
        code: "tt0123456",
        downloadedBytes: "10",
        organizationRelativeDirectory: `${releaseName} (1999)/`,
        organizationStatus: "attention",
        releaseName,
        speedBytesPerSecond: "0",
        state: "completed",
        transferId: "movie-recovery-123",
      }),
    );
    render(<App />);
    await waitFor(() => {
      expect(scanMoviesMock).toHaveBeenCalledOnce();
      expect(queryMoviesStorageMock).toHaveBeenCalledOnce();
    });
    fireEvent.click(screen.getByRole("button", { name: "Downloads" }));
    fireEvent.click(await screen.findByRole("button", { name: "Organize files" }));
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Organize 1 file",
      }),
    );

    const card = (
      await screen.findByRole("heading", { name: releaseName })
    ).closest("article") as HTMLElement;
    expect(within(card).getByText("Organization needs attention")).toBeTruthy();
    expect(within(card).getByRole("button", { name: "Organize files" })).toBeTruthy();
    expect(listVrDownloadsMock).toHaveBeenCalledOnce();
    expect(scanMoviesMock).toHaveBeenCalledOnce();
    expect(queryMoviesStorageMock).toHaveBeenCalledOnce();
    expect(scanAdultLibraryMock).not.toHaveBeenCalled();
    expect(scanVrLibraryMock).not.toHaveBeenCalled();
  });

  it("previews Adult paths and refreshes only Adult Library and storage after apply", async () => {
    savedAdultFolder = "/Adult";
    const releaseName = "【Adult】 ADLT-123  Exact — 特別版";
    loadVrDownloadsMock.mockResolvedValue(
      vrDownloadFixture({
        canOrganize: "true",
        category: "adult",
        code: "ADLT-123",
        downloadedBytes: "10",
        releaseName,
        speedBytesPerSecond: "0",
        state: "completed",
        transferId: "adult-transfer-123",
      }),
    );
    listVrDownloadsMock.mockResolvedValueOnce(
      vrDownloadFixture({
        category: "adult",
        code: "ADLT-123",
        downloadedBytes: "10",
        organizationRelativeDirectory: "ADLT-123/",
        organizationStatus: "organized",
        releaseName,
        speedBytesPerSecond: "0",
        state: "completed",
        transferId: "adult-transfer-123",
      }),
    );
    previewVrOrganizationMock.mockResolvedValueOnce([
      "adult-plan-123",
      "adult-transfer-123",
      "ADLT-123",
      "6",
      "7",
      "move",
      "Source/ADLT-123 Part 1-2.mp4",
      "ADLT-123/ADLT-123 Part 1-2.mp4",
      "move",
      "Source/ADLT-123 CD1+2.mkv",
      "ADLT-123/ADLT-123 CD1+2.mkv",
      "move",
      "Source/ADLT-123 Part 01.MP4",
      "ADLT-123/ADLT-123 - Part 01.MP4",
      "move",
      "Source/ADLT-123 CD2.mkv",
      "ADLT-123/ADLT-123 - CD2.mkv",
      "move",
      "Source/ADLT-123 Disc 03.MKV",
      "ADLT-123/ADLT-123 - Disc 03.MKV",
      "move",
      "Source/ADLT-123 Disk-4.mp4",
      "ADLT-123/ADLT-123 - Disk-4.mp4",
      "non-media-unchanged",
      "Source/notes  —  exact.txt",
      "",
    ]);
    render(<App />);
    await waitFor(() => {
      expect(scanAdultLibraryMock).toHaveBeenCalledOnce();
      expect(queryAdultStorageMock).toHaveBeenCalledOnce();
    });

    fireEvent.click(screen.getByRole("button", { name: "Downloads" }));
    const releaseHeading = await screen.findByRole("heading", {
      name: "【Adult】 ADLT-123 Exact — 特別版",
    });
    expect(releaseHeading.textContent).toBe(releaseName);
    const card = releaseHeading.closest("article") as HTMLElement;
    expect(within(card).getByText("Adult · ADLT-123")).toBeTruthy();
    fireEvent.click(within(card).getByRole("button", { name: "Organize files" }));
    const confirmation = await screen.findByRole("alertdialog");
    expect(
      within(confirmation).getByText(
        "Confirm this exact plan. 6 files will move within the current Adult folder.",
      ),
    ).toBeTruthy();
    for (const destination of [
      "Move to: ADLT-123/ADLT-123 Part 1-2.mp4",
      "Move to: ADLT-123/ADLT-123 CD1+2.mkv",
      "Move to: ADLT-123/ADLT-123 - Part 01.MP4",
      "Move to: ADLT-123/ADLT-123 - CD2.mkv",
      "Move to: ADLT-123/ADLT-123 - Disc 03.MKV",
      "Move to: ADLT-123/ADLT-123 - Disk-4.mp4",
    ]) {
      expect(
        within(confirmation).getByText(
          (_, element) =>
            element?.tagName === "SPAN" &&
            element.textContent === destination,
        ),
      ).toBeTruthy();
    }
    for (const truncated of [
      "Move to: ADLT-123/ADLT-123 - Part 1.mp4",
      "Move to: ADLT-123/ADLT-123 - CD1.mkv",
    ]) {
      expect(
        within(confirmation).queryByText(
          (_, element) =>
            element?.tagName === "SPAN" && element.textContent === truncated,
        ),
      ).toBeNull();
    }

    fireEvent.click(
      within(confirmation).getByRole("button", { name: "Organize 6 files" }),
    );
    const organizedHeading = await screen.findByRole("heading", {
      name: "【Adult】 ADLT-123 Exact — 特別版",
    });
    expect(organizedHeading.textContent).toBe(releaseName);
    const organizedCard = organizedHeading.closest("article") as HTMLElement;
    expect(within(organizedCard).getByText("Organized")).toBeTruthy();
    expect(within(organizedCard).getByText("ADLT-123/")).toBeTruthy();
    await waitFor(() => {
      expect(scanAdultLibraryMock).toHaveBeenCalledTimes(2);
      expect(queryAdultStorageMock).toHaveBeenCalledTimes(2);
    });
    expect(scanVrLibraryMock).not.toHaveBeenCalled();
    expect(queryVrStorageMock).not.toHaveBeenCalled();
    expect(applyVrOrganizationMock).toHaveBeenCalledWith({
      planId: "adult-plan-123",
    });
  });

  it("reloads Adult attention after apply failure without refreshing either Library", async () => {
    savedAdultFolder = "/Adult";
    const initialRows = vrDownloadFixture({
      canOrganize: "true",
      category: "adult",
      code: "ADLT-123",
      downloadedBytes: "10",
      releaseName: "Recoverable Adult organization",
      speedBytesPerSecond: "0",
      state: "completed",
      transferId: "adult-recovery-123",
    });
    loadVrDownloadsMock.mockResolvedValue(initialRows);
    previewVrOrganizationMock.mockResolvedValueOnce([
      "adult-recovery-plan",
      "adult-recovery-123",
      "ADLT-123",
      "1",
      "1",
      "move",
      "Source/ADLT-123.mp4",
      "ADLT-123/ADLT-123.mp4",
    ]);
    applyVrOrganizationMock.mockRejectedValueOnce("vr_organization_failed");
    listVrDownloadsMock.mockResolvedValueOnce(
      vrDownloadFixture({
        canOrganize: "true",
        category: "adult",
        code: "ADLT-123",
        downloadedBytes: "10",
        organizationRelativeDirectory: "ADLT-123/",
        organizationStatus: "attention",
        releaseName: "Recoverable Adult organization",
        speedBytesPerSecond: "0",
        state: "completed",
        transferId: "adult-recovery-123",
      }),
    );
    render(<App />);
    await waitFor(() => {
      expect(scanAdultLibraryMock).toHaveBeenCalledOnce();
      expect(queryAdultStorageMock).toHaveBeenCalledOnce();
    });
    fireEvent.click(screen.getByRole("button", { name: "Downloads" }));
    fireEvent.click(await screen.findByRole("button", { name: "Organize files" }));
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Organize 1 file",
      }),
    );

    const card = (
      await screen.findByRole("heading", {
        name: "Recoverable Adult organization",
      })
    ).closest("article") as HTMLElement;
    expect(within(card).getByText("Organization needs attention")).toBeTruthy();
    expect(within(card).getByRole("button", { name: "Organize files" })).toBeTruthy();
    await waitFor(() => expect(document.activeElement).toBe(
      within(card).getByRole("button", { name: "Organize files" }),
    ));
    expect(scanAdultLibraryMock).toHaveBeenCalledOnce();
    expect(queryAdultStorageMock).toHaveBeenCalledOnce();
    expect(scanVrLibraryMock).not.toHaveBeenCalled();
    expect(queryVrStorageMock).not.toHaveBeenCalled();
    expect(listVrDownloadsMock).toHaveBeenCalledOnce();
  });

  it("reports preview and apply failures locally without a Library refresh", async () => {
    savedVrFolder = "/VR";
    loadVrDownloadsMock.mockResolvedValue(
      vrDownloadFixture({
        canOrganize: "true",
        downloadedBytes: "10",
        releaseName: "Failure-isolated release",
        speedBytesPerSecond: "0",
        state: "completed",
        transferId: "failure-row",
      }),
    );
    previewVrOrganizationMock
      .mockRejectedValueOnce("vr_organization_conflict")
      .mockResolvedValueOnce([
        "plan-failure",
        "failure-row",
        "MDVR-419",
        "1",
        "1",
        "move",
        "Source/MDVR-419.mp4",
        "MDVR-419/MDVR-419.mp4",
      ]);
    applyVrOrganizationMock.mockRejectedValueOnce("vr_organization_failed");
    listVrDownloadsMock.mockResolvedValueOnce(
      vrDownloadFixture({
        canOrganize: "true",
        downloadedBytes: "10",
        organizationRelativeDirectory: "MDVR-419/",
        organizationStatus: "attention",
        releaseName: "Failure-isolated release",
        speedBytesPerSecond: "0",
        state: "completed",
        transferId: "failure-row",
      }),
    );
    render(<App />);
    await waitFor(() => {
      expect(scanVrLibraryMock).toHaveBeenCalledOnce();
      expect(queryVrStorageMock).toHaveBeenCalledOnce();
    });
    fireEvent.click(screen.getByRole("button", { name: "Downloads" }));
    const card = (
      await screen.findByRole("heading", { name: "Failure-isolated release" })
    ).closest("article") as HTMLElement;
    const trigger = within(card).getByRole("button", { name: "Organize files" });

    fireEvent.click(trigger);
    expect(
      await within(card).findByText(
        "The complete organization plan conflicts with an existing or duplicate destination.",
      ),
    ).toBeTruthy();
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(applyVrOrganizationMock).not.toHaveBeenCalled();

    fireEvent.click(trigger);
    const confirmation = await screen.findByRole("alertdialog");
    fireEvent.click(
      within(confirmation).getByRole("button", { name: "Organize 1 file" }),
    );
    expect(
      await within(card).findByText(
        "The organization operation could not be completed safely. Review the current Downloads state before retrying.",
      ),
    ).toBeTruthy();
    expect(within(card).getByText("Organization needs attention")).toBeTruthy();
    expect(within(card).getByText("Needs attention")).toBeTruthy();
    expect(within(card).getByRole("button", { name: "Organize files" })).toBeTruthy();
    await waitFor(() => expect(document.activeElement).toBe(trigger));
    expect(listVrDownloadsMock).toHaveBeenCalledOnce();
    expect(scanVrLibraryMock).toHaveBeenCalledOnce();
    expect(queryVrStorageMock).toHaveBeenCalledOnce();
    expect(dismissVrDownloadMock).not.toHaveBeenCalled();
  });

  it("ignores a late preview after navigation without refreshing or applying", async () => {
    const pendingPreview = createDeferred<string[]>();
    loadVrDownloadsMock.mockResolvedValue(
      vrDownloadFixture({
        canOrganize: "true",
        downloadedBytes: "10",
        releaseName: "Late preview release",
        speedBytesPerSecond: "0",
        state: "completed",
        transferId: "late-preview",
      }),
    );
    previewVrOrganizationMock.mockReturnValue(pendingPreview.promise);
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Downloads" }));
    fireEvent.click(await screen.findByRole("button", { name: "Organize files" }));
    selectDashboard();

    await act(async () => {
      pendingPreview.resolve([
        "late-plan",
        "late-preview",
        "MDVR-419",
        "1",
        "1",
        "move",
        "Source/MDVR-419.mp4",
        "MDVR-419/MDVR-419.mp4",
      ]);
      await pendingPreview.promise;
    });
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(applyVrOrganizationMock).not.toHaveBeenCalled();
    expect(listVrDownloadsMock).not.toHaveBeenCalled();
    expect(dismissVrOrganizationMock).toHaveBeenCalledOnce();
  });

  it("ignores a late apply after navigation without refreshing Library or storage", async () => {
    savedVrFolder = "/VR";
    const pendingApply = createDeferred<void>();
    loadVrDownloadsMock.mockResolvedValue(
      vrDownloadFixture({
        canOrganize: "true",
        downloadedBytes: "10",
        releaseName: "Late apply release",
        speedBytesPerSecond: "0",
        state: "completed",
        transferId: "late-apply",
      }),
    );
    applyVrOrganizationMock.mockReturnValue(pendingApply.promise);
    render(<App />);
    await waitFor(() => {
      expect(scanVrLibraryMock).toHaveBeenCalledOnce();
      expect(queryVrStorageMock).toHaveBeenCalledOnce();
    });
    fireEvent.click(screen.getByRole("button", { name: "Downloads" }));
    fireEvent.click(await screen.findByRole("button", { name: "Organize files" }));
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Organize 1 file",
      }),
    );
    const dashboard = document.querySelectorAll<HTMLButtonElement>(
      ".navigation-item",
    )[0];
    fireEvent.click(dashboard);

    await act(async () => {
      pendingApply.resolve();
      await pendingApply.promise;
    });
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(listVrDownloadsMock).not.toHaveBeenCalled();
    expect(scanVrLibraryMock).toHaveBeenCalledOnce();
    expect(queryVrStorageMock).toHaveBeenCalledOnce();
  });

  it("counts recoverable organization attention without losing completed totals", async () => {
    loadVrDownloadsMock.mockResolvedValue(
      vrDownloadFixture({
        canOrganize: "true",
        downloadedBytes: "10",
        organizationRelativeDirectory: "MDVR-419/",
        organizationStatus: "attention",
        releaseName: "Recoverable partial organization",
        speedBytesPerSecond: "0",
        state: "completed",
        transferId: "attention-row",
      }),
    );
    render(<App />);
    const downloads = await screen.findByRole("region", { name: "Downloads" });
    const summary = await within(downloads).findByLabelText("Transfer summary");
    const completed = within(summary).getByText("Completed").closest("div");
    const attention = within(summary).getByText("Needs attention").closest("div");
    expect(within(completed as HTMLElement).getByText("1")).toBeTruthy();
    expect(within(attention as HTMLElement).getByText("1")).toBeTruthy();

    fireEvent.click(
      within(downloads).getByRole("button", { name: "Open Downloads" }),
    );
    const card = (
      await screen.findByRole("heading", {
        name: "Recoverable partial organization",
      })
    ).closest("article") as HTMLElement;
    expect(within(card).getByText("Organization needs attention")).toBeTruthy();
    expect(within(card).getByRole("button", { name: "Organize files" })).toBeTruthy();
  });

  it("retains a recovered row and local error when durable dismissal fails", async () => {
    savedVrFolder = "/VR";
    loadVrDownloadsMock.mockResolvedValue(
      vrDownloadFixture({
        canOrganize: "true",
        downloadedBytes: "10",
        organizationRelativeDirectory: "MDVR-419/",
        organizationStatus: "attention",
        releaseName: "Dismissal recovery retained",
        speedBytesPerSecond: "0",
        state: "completed",
        transferId: "failed-dismiss",
      }),
    );
    dismissVrDownloadMock.mockRejectedValueOnce(
      "vr_download_persistence_failed",
    );
    render(<App />);
    await waitFor(() => {
      expect(scanVrLibraryMock).toHaveBeenCalledOnce();
      expect(queryVrStorageMock).toHaveBeenCalledOnce();
    });
    fireEvent.click(screen.getByRole("button", { name: "Downloads" }));
    const card = (
      await screen.findByRole("heading", {
        name: "Dismissal recovery retained",
      })
    ).closest("article") as HTMLElement;
    const dismiss = within(card).getByRole("button", { name: "Dismiss" });
    fireEvent.click(dismiss);

    expect(
      await within(card).findByText(
        "The dismiss action could not be completed for this transfer.",
      ),
    ).toBeTruthy();
    expect(within(card).getByText("Organization needs attention")).toBeTruthy();
    expect(within(card).getByRole("button", { name: "Organize files" })).toBeTruthy();
    expect(listVrDownloadsMock).not.toHaveBeenCalled();
    expect(scanVrLibraryMock).toHaveBeenCalledOnce();
    expect(queryVrStorageMock).toHaveBeenCalledOnce();
  });
});

describe("local Movies library", () => {
  it("persists the selected folder and clearing it blocks a late scan", async () => {
    const pendingScan = createDeferred<string[]>();
    scanMoviesMock.mockReturnValue(pendingScan.promise);
    openFolderMock.mockResolvedValue("/Local/Movies — 家族");

    render(<App />);
    selectSettings();
    fireEvent.click(screen.getByRole("button", { name: "Choose folder" }));

    expect(await screen.findByText("/Local/Movies — 家族")).toBeTruthy();
    expect(savedMoviesFolder).toBe("/Local/Movies — 家族");
    expect(openFolderMock).toHaveBeenCalledOnce();

    cleanup();
    render(<App />);
    selectSettings();
    expect(await screen.findByText("/Local/Movies — 家族")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Clear folder" }));
    expect(
      await screen.findByText("No Movies folder configured."),
    ).toBeTruthy();
    await waitFor(() => expect(savedMoviesFolder).toBeNull());

    selectLibrary();
    expect(
      screen.getByRole("heading", {
        level: 2,
        name: "Choose a Movies folder to begin",
      }),
    ).toBeTruthy();

    await act(async () => {
      pendingScan.resolve(["/Local/Movies — 家族/Old result.mp4"]);
      await pendingScan.promise;
    });
    expect(screen.queryByText("Old result")).toBeNull();
  });

  it("renders exact Unicode titles and removes only the final extension", async () => {
    savedMoviesFolder = "/Movies";
    scanMoviesMock.mockResolvedValue([
      "/Movies/映画  —  Final.Cut.MKV",
      "C:\\Movies\\CAPS & punctuation!.MP4",
    ]);

    render(<App />);
    selectLibrary();

    const unicodeTitle = await screen.findByRole("heading", {
      level: 3,
      name: "映画 — Final.Cut",
    });
    expect(unicodeTitle.textContent).toBe("映画  —  Final.Cut");
    expect(
      screen.getByRole("heading", {
        level: 3,
        name: "CAPS & punctuation!",
      }),
    ).toBeTruthy();
  });

  it("copies the exact filename-derived Library title without parent activation", async () => {
    const title = "映画  —  Final.CUT & punctuation!";
    savedMoviesFolder = "/Movies";
    scanMoviesMock.mockResolvedValue([`/Movies/${title}.MKV`]);
    const parentActivation = vi.fn();

    render(
      <div onClick={parentActivation} onPointerDown={parentActivation}>
        <App />
      </div>,
    );
    selectLibrary();

    const heading = await screen.findByRole("heading", {
      level: 3,
      name: "映画 — Final.CUT & punctuation!",
    });
    const card = heading.closest("article") as HTMLElement;
    const copyButton = within(card).getByRole("button", {
      name: /Copy title:/,
    });
    parentActivation.mockClear();
    fireEvent.pointerDown(copyButton);
    await act(async () => {
      fireEvent.click(copyButton);
      await Promise.resolve();
    });

    expect(clipboardWriteMock).toHaveBeenCalledWith(title);
    expect(parentActivation).not.toHaveBeenCalled();
    expect(copyButton.getAttribute("aria-label")).toBe(
      `Copied title: ${title}`,
    );
  });

  it("reports an unavailable clipboard on the affected Library card", async () => {
    Reflect.deleteProperty(navigator, "clipboard");
    savedMoviesFolder = "/Movies";
    scanMoviesMock.mockResolvedValue(["/Movies/Unavailable clipboard.mp4"]);

    render(<App />);
    selectLibrary();

    const copyButton = await screen.findByRole("button", {
      name: "Copy title: Unavailable clipboard",
    });
    fireEvent.click(copyButton);

    expect(
      await screen.findByRole("button", {
        name: "Copy failed for title: Unavailable clipboard",
      }),
    ).toHaveProperty("textContent", "Failed");
    expect(screen.getByRole("alert").textContent).toBe(
      "Copy failed for title: Unavailable clipboard",
    );
    expect(clipboardWriteMock).not.toHaveBeenCalled();
  });

  it("requires explicit confirmation and makes every dialog dismissal non-mutating", async () => {
    const path = "/Movies/映画  —  Confirm me.MKV";
    const parentActivation = vi.fn();
    savedMoviesFolder = "/Movies";
    scanMoviesMock.mockResolvedValue([path]);

    render(
      <div onClick={parentActivation} onPointerDown={parentActivation}>
        <App />
      </div>,
    );
    selectLibrary();

    const trashButton = await screen.findByRole("button", {
      name: "Move movie to Trash or Recycle Bin: 映画 — Confirm me",
    });
    parentActivation.mockClear();
    trashButton.focus();
    fireEvent.keyDown(trashButton, { key: "Enter" });
    fireEvent.click(trashButton);

    let dialog = await screen.findByRole("alertdialog");
    expect(dialog.textContent).toContain("Move “映画  —  Confirm me” to Trash?");
    expect(dialog.textContent).toContain(
      "macOS Trash or the Windows Recycle Bin",
    );
    expect(dialog.textContent).not.toContain(path);
    await waitFor(() => {
      expect(document.activeElement).toBe(
        within(dialog).getByRole("button", { name: "Cancel" }),
      );
    });
    expect(trashMovieMock).not.toHaveBeenCalled();

    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    await waitFor(() => expect(document.activeElement).toBe(trashButton));

    fireEvent.click(trashButton);
    dialog = await screen.findByRole("alertdialog");
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(screen.queryByRole("alertdialog")).toBeNull();
    await waitFor(() => expect(document.activeElement).toBe(trashButton));

    fireEvent.click(trashButton);
    dialog = await screen.findByRole("alertdialog");
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Close confirmation" }),
    );
    expect(screen.queryByRole("alertdialog")).toBeNull();
    await waitFor(() => expect(document.activeElement).toBe(trashButton));

    fireEvent.click(trashButton);
    await screen.findByRole("alertdialog");
    const backdrop = document.querySelector(".trash-dialog__backdrop");
    if (backdrop === null) {
      throw new Error("The Trash confirmation backdrop was not rendered.");
    }
    fireEvent.pointerDown(backdrop);
    fireEvent.click(backdrop);
    expect(screen.queryByRole("alertdialog")).toBeNull();
    await waitFor(() => expect(document.activeElement).toBe(trashButton));
    expect(trashMovieMock).not.toHaveBeenCalled();
    expect(openMovieMock).not.toHaveBeenCalled();
    expect(revealMovieMock).not.toHaveBeenCalled();
    expect(clipboardWriteMock).not.toHaveBeenCalled();
    expect(parentActivation).not.toHaveBeenCalled();
  });

  it("trashes the exact confirmed path once and clamps the final page after acceptance", async () => {
    const pendingTrash = createDeferred<void>();
    const folder = "C:\\Movies";
    const exactPath =
      "C:\\Movies\\映画  —  Final.CUT & punctuation! [1080p].MKV";
    const title = "映画  —  Final.CUT & punctuation! [1080p]";
    const paths = Array.from({ length: 15 }, (_, index) =>
      index === 14
        ? exactPath
        : `C:\\Movies\\Library ${String(index + 1).padStart(2, "0")}.mp4`,
    );
    const parentActivation = vi.fn();
    savedMoviesFolder = folder;
    scanMoviesMock.mockResolvedValue(paths);
    trashMovieMock.mockReturnValue(pendingTrash.promise);

    render(
      <div onClick={parentActivation} onPointerDown={parentActivation}>
        <App />
      </div>,
    );
    selectLibrary();
    await screen.findByText("Library 01");
    resizeGallery("library", 1528, 136);
    fireEvent.click(screen.getByRole("button", { name: "Next Movies page" }));
    fireEvent.click(screen.getByRole("button", { name: "Next Movies page" }));
    expect(screen.getByText("Page 3 of 3")).toBeTruthy();

    const card = screen
      .getByRole("heading", {
        level: 3,
        name: "映画 — Final.CUT & punctuation! [1080p]",
      })
      .closest("article") as HTMLElement;
    fireEvent.click(
      within(card).getByRole("button", { name: /Copy title:/ }),
    );
    expect(
      await within(card).findByRole("button", { name: /Copied title:/ }),
    ).toBeTruthy();
    clipboardWriteMock.mockClear();
    parentActivation.mockClear();

    const trashButton = within(card).getByRole("button", {
      name: /Move movie to Trash or Recycle Bin:/,
    });
    fireEvent.pointerDown(trashButton);
    fireEvent.click(trashButton);
    const dialog = await screen.findByRole("alertdialog");
    expect(trashMovieMock).not.toHaveBeenCalled();

    const confirmButton = within(dialog).getByRole("button", {
      name: /Confirm moving movie to Trash or Recycle Bin:/,
    });
    expect(confirmButton.getAttribute("aria-label")).toBe(
      `Confirm moving movie to Trash or Recycle Bin: ${title}`,
    );
    confirmButton.focus();
    fireEvent.keyDown(confirmButton, { key: "Enter" });
    fireEvent.click(confirmButton);
    confirmButton.click();

    expect(trashMovieMock).toHaveBeenCalledTimes(1);
    expect(trashMovieMock).toHaveBeenCalledWith({
      path: exactPath,
    });
    expect(confirmButton).toHaveProperty("disabled", true);
    expect(dialog.getAttribute("aria-busy")).toBe("true");
    expect(dialog.contains(document.activeElement)).toBe(true);
    expect(
      within(dialog).getByRole("button", { name: "Cancel" }),
    ).toHaveProperty("disabled", true);
    expect(
      within(dialog).getByRole("button", { name: "Close confirmation" }),
    ).toHaveProperty("disabled", true);
    fireEvent.keyDown(dialog, { key: "Escape" });
    const pendingBackdrop = document.querySelector(".trash-dialog__backdrop");
    if (pendingBackdrop === null) {
      throw new Error("The pending Trash backdrop was not rendered.");
    }
    fireEvent.click(pendingBackdrop);
    expect(screen.getByRole("alertdialog")).toBe(dialog);
    expect(trashMovieMock).toHaveBeenCalledTimes(1);
    expect(screen.getByText("Page 3 of 3")).toBeTruthy();
    expect(screen.queryByText(`${title} was moved to Trash or the Recycle Bin.`))
      .toBeNull();
    expect(openMovieMock).not.toHaveBeenCalled();
    expect(revealMovieMock).not.toHaveBeenCalled();
    expect(clipboardWriteMock).not.toHaveBeenCalled();
    expect(parentActivation).not.toHaveBeenCalled();
    expect(fetchMock).not.toHaveBeenCalled();
    expect(scanMoviesMock).toHaveBeenCalledTimes(1);

    await act(async () => {
      pendingTrash.resolve(undefined);
      await pendingTrash.promise;
    });

    expect(await screen.findByText("Page 2 of 2")).toBeTruthy();
    expect(
      screen.queryByRole("heading", {
        level: 3,
        name: "映画 — Final.CUT & punctuation! [1080p]",
      }),
    ).toBeNull();
    expect(screen.getByRole("status").textContent).toBe(
      `${title} was moved to Trash or the Recycle Bin.`,
    );
    expect(visibleCardCount("Movies")).toBe(7);
    expect(savedMoviesFolder).toBe(folder);
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe(
      "Library",
    );
    expect(scanMoviesMock).toHaveBeenCalledTimes(1);
  });

  it("removes only the confirmed movie and preserves unrelated card feedback", async () => {
    const firstPath = "/Movies/Remove only me.mp4";
    const secondPath = "/Movies/Keep my feedback.mkv";
    const parentActivation = vi.fn();
    savedMoviesFolder = "/Movies";
    scanMoviesMock.mockResolvedValue([firstPath, secondPath]);
    openMovieMock.mockRejectedValueOnce("movie_open_failed");
    revealMovieMock.mockRejectedValueOnce("movie_reveal_failed");

    render(
      <div onClick={parentActivation} onPointerDown={parentActivation}>
        <App />
      </div>,
    );
    selectLibrary();

    const secondCard = (
      await screen.findByRole("heading", {
        level: 3,
        name: "Keep my feedback",
      })
    ).closest("article") as HTMLElement;
    fireEvent.click(
      within(secondCard).getByRole("button", { name: /Open movie:/ }),
    );
    fireEvent.click(
      within(secondCard).getByRole("button", { name: /Reveal movie:/ }),
    );
    fireEvent.click(
      within(secondCard).getByRole("button", { name: /Copy title:/ }),
    );
    expect(await within(secondCard).findAllByRole("alert")).toHaveLength(2);
    expect(
      await within(secondCard).findByRole("button", { name: /Copied title:/ }),
    ).toBeTruthy();
    expect(trashMovieMock).not.toHaveBeenCalled();

    openMovieMock.mockClear();
    revealMovieMock.mockClear();
    clipboardWriteMock.mockClear();
    parentActivation.mockClear();
    fireEvent.click(
      screen.getByRole("button", {
        name: "Move movie to Trash or Recycle Bin: Remove only me",
      }),
    );
    const dialog = await screen.findByRole("alertdialog");
    fireEvent.click(
      within(dialog).getByRole("button", {
        name: "Confirm moving movie to Trash or Recycle Bin: Remove only me",
      }),
    );

    expect(
      await screen.findByRole("heading", {
        level: 3,
        name: "Keep my feedback",
      }),
    ).toBeTruthy();
    expect(
      screen.queryByRole("heading", { level: 3, name: "Remove only me" }),
    ).toBeNull();
    const fileActionErrors = within(secondCard).getAllByRole("alert");
    expect(fileActionErrors.map((alert) => alert.textContent)).toEqual([
      "The operating system could not open this movie.",
      "The operating system could not reveal this movie.",
    ]);
    expect(
      within(secondCard).getByRole("button", { name: /Copied title:/ }),
    ).toBeTruthy();
    expect(trashMovieMock).toHaveBeenCalledWith({
      path: firstPath,
    });
    expect(openMovieMock).not.toHaveBeenCalled();
    expect(revealMovieMock).not.toHaveBeenCalled();
    expect(clipboardWriteMock).not.toHaveBeenCalled();
    expect(parentActivation).not.toHaveBeenCalled();
    expect(scanMoviesMock).toHaveBeenCalledTimes(1);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it.each([
    ["movie_trash_not_found", "This movie is no longer available."],
    ["movie_trash_unavailable", "Auto-Video could not access this movie."],
    ["movie_trash_not_file", "This item is not an eligible video file."],
    [
      "movie_trash_unsupported",
      "This item is not a supported .mp4 or .mkv file.",
    ],
    [
      "movie_trash_folder_unavailable",
      "The configured Movies folder is no longer available.",
    ],
    [
      "movie_trash_outside_folder",
      "This movie is outside the configured Movies folder.",
    ],
    [
      "movie_trash_stale",
      "This movie is no longer part of the current Library.",
    ],
    [
      "movie_trash_failed",
      "The operating system could not move this movie to Trash or the Recycle Bin.",
    ],
  ])(
    "reports %s in the confirmation and keeps the movie",
    async (errorCode, expectedMessage) => {
      const path = "/Movies/First — exact.mp4";
      savedMoviesFolder = "/Movies";
      scanMoviesMock.mockResolvedValue([
        path,
        "/Movies/Second remains available.mkv",
      ]);
      trashMovieMock.mockRejectedValueOnce(errorCode);

      render(<App />);
      selectLibrary();

      fireEvent.click(
        await screen.findByRole("button", {
          name: "Move movie to Trash or Recycle Bin: First — exact",
        }),
      );
      const dialog = await screen.findByRole("alertdialog");
      fireEvent.click(
        within(dialog).getByRole("button", {
          name: "Confirm moving movie to Trash or Recycle Bin: First — exact",
        }),
      );

      expect(await within(dialog).findByRole("alert")).toHaveProperty(
        "textContent",
        expectedMessage,
      );
      expect(dialog.textContent).not.toContain(path);
      expect(screen.getByText("First — exact", { selector: "h3" })).toBeTruthy();
      expect(
        within(dialog).getByRole("button", {
          name: "Confirm moving movie to Trash or Recycle Bin: First — exact",
        }),
      ).toHaveProperty("disabled", false);
      expect(trashMovieMock).toHaveBeenCalledWith({
        path,
      });
      expect(openMovieMock).not.toHaveBeenCalled();
      expect(revealMovieMock).not.toHaveBeenCalled();
      expect(clipboardWriteMock).not.toHaveBeenCalled();
      expect(fetchMock).not.toHaveBeenCalled();
      expect(scanMoviesMock).toHaveBeenCalledTimes(1);
    },
  );

  it("invalidates a stale refresh result after a pending Trash request succeeds", async () => {
    const pendingTrash = createDeferred<void>();
    const staleRefresh = createDeferred<string[]>();
    const trashedPath = "/Movies/Trash during refresh.mp4";
    const remainingPath = "/Movies/Remaining.mkv";
    savedMoviesFolder = "/Movies";
    scanMoviesMock
      .mockResolvedValueOnce([trashedPath, remainingPath])
      .mockReturnValueOnce(staleRefresh.promise)
      .mockResolvedValueOnce([remainingPath]);
    trashMovieMock.mockReturnValue(pendingTrash.promise);

    render(<App />);
    selectLibrary();

    fireEvent.click(
      await screen.findByRole("button", {
        name: "Move movie to Trash or Recycle Bin: Trash during refresh",
      }),
    );
    const dialog = await screen.findByRole("alertdialog");
    fireEvent.click(
      within(dialog).getByRole("button", {
        name: "Confirm moving movie to Trash or Recycle Bin: Trash during refresh",
      }),
    );
    expect(trashMovieMock).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByText("Refresh", { selector: "button" }));
    expect(await screen.findByText("Scanning Movies folder")).toBeTruthy();
    expect(scanMoviesMock).toHaveBeenCalledTimes(2);

    await act(async () => {
      pendingTrash.resolve(undefined);
      await pendingTrash.promise;
    });

    expect(await screen.findByText("Remaining")).toBeTruthy();
    expect(scanMoviesMock).toHaveBeenCalledTimes(3);
    expect(
      screen.queryByRole("heading", {
        level: 3,
        name: "Trash during refresh",
      }),
    ).toBeNull();

    await act(async () => {
      staleRefresh.resolve([trashedPath, remainingPath]);
      await staleRefresh.promise;
    });
    expect(screen.getByText("Remaining")).toBeTruthy();
    expect(screen.queryByText("Trash during refresh")).toBeNull();
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("does not let a late Trash response alter a replacement folder", async () => {
    const pendingTrash = createDeferred<void>();
    const oldFolder = "/Movies/Old";
    const newFolder = "/Movies/New";
    const oldPath = `${oldFolder}/Old movie.mp4`;
    const newPath = `${newFolder}/New movie.mkv`;
    savedMoviesFolder = oldFolder;
    scanMoviesMock
      .mockResolvedValueOnce([oldPath])
      .mockResolvedValueOnce([newPath]);
    trashMovieMock.mockReturnValue(pendingTrash.promise);
    openFolderMock.mockResolvedValue(newFolder);

    render(<App />);
    selectLibrary();

    fireEvent.click(
      await screen.findByRole("button", {
        name: "Move movie to Trash or Recycle Bin: Old movie",
      }),
    );
    const dialog = await screen.findByRole("alertdialog");
    fireEvent.click(
      within(dialog).getByRole("button", {
        name: "Confirm moving movie to Trash or Recycle Bin: Old movie",
      }),
    );

    fireEvent.click(
      screen.getByRole("button", { hidden: true, name: "Settings" }),
    );
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Change folder" }));
      await Promise.resolve();
    });
    selectLibrary();
    expect(await screen.findByText("New movie")).toBeTruthy();

    await act(async () => {
      pendingTrash.resolve(undefined);
      await pendingTrash.promise;
    });

    expect(screen.getByText("New movie")).toBeTruthy();
    expect(
      screen.queryByText("Old movie was moved to Trash or the Recycle Bin."),
    ).toBeNull();
    expect(savedMoviesFolder).toBe(newFolder);
    expect(scanMoviesMock).toHaveBeenCalledTimes(2);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("opens the exact scanned path once while preserving copy and pagination state", async () => {
    const pendingOpen = createDeferred<void>();
    const exactPath =
      "C:\\Movies\\映画  —  Final.CUT & punctuation! [1080p].MKV";
    const paths = Array.from({ length: 25 }, (_, index) =>
      index === 10
        ? exactPath
        : `/Movies/Library ${String(index + 1).padStart(2, "0")}.mp4`,
    );
    const title = "映画  —  Final.CUT & punctuation! [1080p]";
    const parentActivation = vi.fn();
    savedMoviesFolder = "/Movies";
    scanMoviesMock.mockResolvedValue(paths);
    openMovieMock.mockReturnValue(pendingOpen.promise);

    render(
      <div onClick={parentActivation} onPointerDown={parentActivation}>
        <App />
      </div>,
    );
    selectLibrary();
    await screen.findByText("Library 01");
    resizeGallery("library", 1528, 136);
    for (let page = 1; page < 4; page += 1) {
      fireEvent.click(
        screen.getByRole("button", { name: "Next Movies page" }),
      );
    }

    const heading = screen.getByRole("heading", {
      level: 3,
      name: "映画 — Final.CUT & punctuation! [1080p]",
    });
    const card = heading.closest("article") as HTMLElement;
    const copyButton = within(card).getByRole("button", {
      name: /Copy title:/,
    });
    fireEvent.click(copyButton);
    const copiedButton = await within(card).findByRole("button", {
      name: /Copied title:/,
    });
    expect(copiedButton.getAttribute("aria-label")).toBe(
      `Copied title: ${title}`,
    );

    clipboardWriteMock.mockClear();
    parentActivation.mockClear();
    const openButton = within(card).getByRole("button", {
      name: /Open movie:/,
    });
    openButton.focus();
    expect(document.activeElement).toBe(openButton);
    fireEvent.pointerDown(openButton);
    fireEvent.click(openButton);
    openButton.click();

    expect(openMovieMock).toHaveBeenCalledTimes(1);
    expect(openMovieMock).toHaveBeenCalledWith({ path: exactPath });
    expect(revealMovieMock).not.toHaveBeenCalled();
    expect(trashMovieMock).not.toHaveBeenCalled();
    expect(openButton).toHaveProperty("disabled", true);
    expect(openButton.getAttribute("aria-label")).toBe(
      `Opening movie: ${title}`,
    );
    expect(within(card).queryByText("Opened")).toBeNull();
    expect(
      within(card).getByRole("button", { name: /Copied title:/ }),
    ).toBeTruthy();
    expect(clipboardWriteMock).not.toHaveBeenCalled();
    expect(parentActivation).not.toHaveBeenCalled();
    expect(screen.getByText("Page 4 of 4")).toBeTruthy();
    expect(scanMoviesMock).toHaveBeenCalledTimes(1);
    expect(fetchMock).not.toHaveBeenCalled();

    await act(async () => {
      pendingOpen.resolve(undefined);
      await pendingOpen.promise;
    });
    expect(
      within(card).getByRole("button", { name: /Open movie:/ }),
    ).toHaveProperty("disabled", false);

    resizeGallery("library", 1088, 284);
    expect(screen.getByText("Page 3 of 3")).toBeTruthy();
    expect(
      screen.getByRole("button", { name: /Copied title:/ }),
    ).toBeTruthy();
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe(
      "Library",
    );
    expect(savedMoviesFolder).toBe("/Movies");
    expect(scanMoviesMock).toHaveBeenCalledTimes(1);
  });

  it.each([
    ["movie_open_not_found", "This movie is no longer available."],
    ["movie_open_unavailable", "Auto-Video could not access this movie."],
    ["movie_open_not_file", "This item is not an eligible video file."],
    [
      "movie_open_unsupported",
      "This item is not a supported .mp4 or .mkv file.",
    ],
    [
      "movie_open_failed",
      "The operating system could not open this movie.",
    ],
  ])(
    "reports %s on only the affected card",
    async (errorCode, expectedMessage) => {
      const firstPath = "/Movies/First — exact.mp4";
      savedMoviesFolder = "/Movies";
      scanMoviesMock.mockResolvedValue([
        firstPath,
        "/Movies/Second remains available.mkv",
      ]);
      openMovieMock.mockRejectedValueOnce(errorCode);

      render(<App />);
      selectLibrary();

      const firstOpenButton = await screen.findByRole("button", {
        name: "Open movie: First — exact",
      });
      const firstCard = firstOpenButton.closest("article") as HTMLElement;
      const secondCard = screen
        .getByRole("button", {
          name: "Open movie: Second remains available",
        })
        .closest("article") as HTMLElement;
      firstOpenButton.focus();
      fireEvent.keyDown(firstOpenButton, { key: "Enter" });
      fireEvent.click(firstOpenButton);

      expect(await within(firstCard).findByRole("alert")).toHaveProperty(
        "textContent",
        expectedMessage,
      );
      expect(within(secondCard).queryByRole("alert")).toBeNull();
      expect(firstCard.textContent).not.toContain(firstPath);
      expect(firstOpenButton).toHaveProperty("disabled", false);
      expect(openMovieMock).toHaveBeenCalledWith({ path: firstPath });
      expect(revealMovieMock).not.toHaveBeenCalled();
      expect(trashMovieMock).not.toHaveBeenCalled();
      expect(clipboardWriteMock).not.toHaveBeenCalled();
      expect(scanMoviesMock).toHaveBeenCalledTimes(1);
    },
  );

  it("reveals the exact scanned path once while preserving Open, copy, and pagination state", async () => {
    const pendingReveal = createDeferred<void>();
    const exactPath =
      "C:\\Movies\\映画  —  Final.CUT & punctuation! [1080p].MKV";
    const paths = Array.from({ length: 25 }, (_, index) =>
      index === 10
        ? exactPath
        : `/Movies/Library ${String(index + 1).padStart(2, "0")}.mp4`,
    );
    const title = "映画  —  Final.CUT & punctuation! [1080p]";
    const parentActivation = vi.fn();
    savedMoviesFolder = "/Movies";
    scanMoviesMock.mockResolvedValue(paths);
    openMovieMock.mockRejectedValueOnce("movie_open_failed");
    revealMovieMock
      .mockReturnValueOnce(pendingReveal.promise)
      .mockRejectedValueOnce("movie_reveal_failed");

    render(
      <div onClick={parentActivation} onPointerDown={parentActivation}>
        <App />
      </div>,
    );
    selectLibrary();
    await screen.findByText("Library 01");
    resizeGallery("library", 1528, 136);
    for (let page = 1; page < 4; page += 1) {
      fireEvent.click(
        screen.getByRole("button", { name: "Next Movies page" }),
      );
    }

    const heading = screen.getByRole("heading", {
      level: 3,
      name: "映画 — Final.CUT & punctuation! [1080p]",
    });
    const card = heading.closest("article") as HTMLElement;
    fireEvent.click(
      within(card).getByRole("button", { name: /Open movie:/ }),
    );
    expect(await within(card).findByRole("alert")).toHaveProperty(
      "textContent",
      "The operating system could not open this movie.",
    );
    fireEvent.click(
      within(card).getByRole("button", { name: /Copy title:/ }),
    );
    const copiedButton = await within(card).findByRole("button", {
      name: /Copied title:/,
    });
    expect(copiedButton.getAttribute("aria-label")).toBe(
      `Copied title: ${title}`,
    );

    openMovieMock.mockClear();
    clipboardWriteMock.mockClear();
    parentActivation.mockClear();
    const revealButton = within(card).getByRole("button", {
      name: /Reveal movie:/,
    });
    revealButton.focus();
    expect(document.activeElement).toBe(revealButton);
    fireEvent.pointerDown(revealButton);
    fireEvent.click(revealButton);
    revealButton.click();

    expect(revealMovieMock).toHaveBeenCalledTimes(1);
    expect(revealMovieMock).toHaveBeenCalledWith({ path: exactPath });
    expect(trashMovieMock).not.toHaveBeenCalled();
    expect(revealButton).toHaveProperty("disabled", true);
    expect(revealButton.getAttribute("aria-label")).toBe(
      `Revealing movie: ${title}`,
    );
    expect(within(card).queryByText("Revealed")).toBeNull();
    expect(within(card).getByRole("alert")).toHaveProperty(
      "textContent",
      "The operating system could not open this movie.",
    );
    expect(
      within(card).getByRole("button", { name: /Copied title:/ }),
    ).toBeTruthy();
    expect(
      within(card).getByRole("button", { name: /Open movie:/ }),
    ).toHaveProperty("disabled", false);
    expect(openMovieMock).not.toHaveBeenCalled();
    expect(clipboardWriteMock).not.toHaveBeenCalled();
    expect(parentActivation).not.toHaveBeenCalled();
    expect(screen.getByText("Page 4 of 4")).toBeTruthy();
    expect(scanMoviesMock).toHaveBeenCalledTimes(1);
    expect(fetchMock).not.toHaveBeenCalled();

    await act(async () => {
      pendingReveal.resolve(undefined);
      await pendingReveal.promise;
    });
    expect(
      within(card).getByRole("button", { name: /Reveal movie:/ }),
    ).toHaveProperty("disabled", false);

    fireEvent.click(
      within(card).getByRole("button", { name: /Reveal movie:/ }),
    );
    expect(
      await within(card).findByText(
        "The operating system could not reveal this movie.",
      ),
    ).toBeTruthy();
    const fileActionErrors = within(card).getAllByRole("alert");
    expect(fileActionErrors.map((alert) => alert.textContent)).toEqual([
      "The operating system could not open this movie.",
      "The operating system could not reveal this movie.",
    ]);
    expect(
      within(card).getByRole("button", { name: /Copied title:/ }),
    ).toBeTruthy();
    expect(revealMovieMock).toHaveBeenCalledTimes(2);
    expect(revealMovieMock).toHaveBeenNthCalledWith(2, { path: exactPath });

    resizeGallery("library", 1088, 284);
    expect(screen.getByText("Page 3 of 3")).toBeTruthy();
    expect(screen.getByRole("button", { name: /Copied title:/ })).toBeTruthy();
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe(
      "Library",
    );
    expect(savedMoviesFolder).toBe("/Movies");
    expect(scanMoviesMock).toHaveBeenCalledTimes(1);
  });

  it.each([
    ["movie_reveal_not_found", "This movie is no longer available."],
    ["movie_reveal_unavailable", "Auto-Video could not access this movie."],
    ["movie_reveal_not_file", "This item is not an eligible video file."],
    [
      "movie_reveal_unsupported",
      "This item is not a supported .mp4 or .mkv file.",
    ],
    [
      "movie_reveal_failed",
      "The operating system could not reveal this movie.",
    ],
  ])(
    "reports %s on only the affected card",
    async (errorCode, expectedMessage) => {
      const firstPath = "/Movies/First — exact.mp4";
      savedMoviesFolder = "/Movies";
      scanMoviesMock.mockResolvedValue([
        firstPath,
        "/Movies/Second remains available.mkv",
      ]);
      revealMovieMock.mockRejectedValueOnce(errorCode);

      render(<App />);
      selectLibrary();

      const firstRevealButton = await screen.findByRole("button", {
        name: "Reveal movie: First — exact",
      });
      const firstCard = firstRevealButton.closest("article") as HTMLElement;
      const secondCard = screen
        .getByRole("button", {
          name: "Reveal movie: Second remains available",
        })
        .closest("article") as HTMLElement;
      firstRevealButton.focus();
      fireEvent.keyDown(firstRevealButton, { key: "Enter" });
      fireEvent.click(firstRevealButton);

      expect(await within(firstCard).findByRole("alert")).toHaveProperty(
        "textContent",
        expectedMessage,
      );
      expect(within(secondCard).queryByRole("alert")).toBeNull();
      expect(firstCard.textContent).not.toContain(firstPath);
      expect(firstRevealButton).toHaveProperty("disabled", false);
      expect(revealMovieMock).toHaveBeenCalledWith({ path: firstPath });
      expect(openMovieMock).not.toHaveBeenCalled();
      expect(trashMovieMock).not.toHaveBeenCalled();
      expect(clipboardWriteMock).not.toHaveBeenCalled();
      expect(scanMoviesMock).toHaveBeenCalledTimes(1);
    },
  );

  it("refresh replaces files added or removed since the previous scan", async () => {
    savedMoviesFolder = "/Movies";
    scanMoviesMock
      .mockResolvedValueOnce(["/Movies/First.mp4"])
      .mockResolvedValueOnce(["/Movies/Second.mkv"]);

    render(<App />);
    selectLibrary();
    expect(await screen.findByText("First")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));

    expect(await screen.findByText("Second")).toBeTruthy();
    expect(screen.queryByText("First")).toBeNull();
    expect(scanMoviesMock).toHaveBeenCalledTimes(2);
    expect(scanMoviesMock).toHaveBeenNthCalledWith(2, undefined);
  });

  it("shows distinct scanning and empty-folder states", async () => {
    const pendingScan = createDeferred<string[]>();
    savedMoviesFolder = "/Movies";
    scanMoviesMock.mockReturnValue(pendingScan.promise);

    render(<App />);
    selectLibrary();

    expect(
      (await screen.findByRole("status")).querySelector("h2")?.textContent,
    ).toBe("Scanning Movies folder");

    await act(async () => {
      pendingScan.resolve([]);
      await pendingScan.promise;
    });
    expect(
      await screen.findByRole("heading", {
        level: 2,
        name: "No supported videos found",
      }),
    ).toBeTruthy();
  });

  it("distinguishes an unavailable folder from a recursive scan failure", async () => {
    savedMoviesFolder = "/Movies";
    scanMoviesMock.mockRejectedValueOnce("movies_folder_unavailable");

    render(<App />);
    selectLibrary();
    expect(
      await screen.findByRole("heading", {
        level: 2,
        name: "Movies folder is unavailable",
      }),
    ).toBeTruthy();

    cleanup();
    scanMoviesMock.mockRejectedValueOnce("movies_scan_failed");
    render(<App />);
    selectLibrary();
    expect(
      await screen.findByRole("heading", {
        level: 2,
        name: "Movies folder could not be scanned",
      }),
    ).toBeTruthy();
  });

  it("removes prior results and prevents an earlier scan from replacing a new folder", async () => {
    const earlierScan = createDeferred<string[]>();
    let scanCount = 0;
    savedMoviesFolder = "/Movies/Old";
    scanMoviesMock.mockImplementation(() => {
      scanCount += 1;
      if (scanCount === 1) {
        return Promise.resolve(["/Movies/Old/Old title.mp4"]);
      }
      if (scanCount === 2) {
        return earlierScan.promise;
      }
      return Promise.resolve(["/Movies/New/New title.mkv"]);
    });
    openFolderMock.mockResolvedValue("/Movies/New");

    render(<App />);
    selectLibrary();
    expect(await screen.findByText("Old title")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(screen.queryByText("Old title")).toBeNull();
    expect(screen.getByRole("status")).toBeTruthy();

    selectSettings();
    fireEvent.click(screen.getByRole("button", { name: "Change folder" }));
    expect(await screen.findByText("/Movies/New")).toBeTruthy();

    selectLibrary();
    expect(await screen.findByText("New title")).toBeTruthy();

    await act(async () => {
      earlierScan.resolve(["/Movies/Old/Stale title.mp4"]);
      await earlierScan.promise;
    });
    expect(screen.queryByText("Stale title")).toBeNull();
    expect(screen.getByText("New title")).toBeTruthy();
  });

  it("reports a native folder-picker failure without changing configuration", async () => {
    openFolderMock.mockRejectedValue(new Error("dialog unavailable"));

    render(<App />);
    selectSettings();
    fireEvent.click(screen.getByRole("button", { name: "Choose folder" }));

    expect(await screen.findByRole("alert")).toHaveProperty(
      "textContent",
      "The Movies folder picker could not be opened.",
    );
    expect(savedMoviesFolder).toBeNull();
  });
});

describe("Movies Library title search", () => {
  it("finds a case-insensitive title match outside the visible page without changing the complete Library", async () => {
    const exactTitle = "Needle — TARGET  22";
    const paths = Array.from(
      { length: 25 },
      (_, index) =>
        `/Movies/Title ${String(index + 1).padStart(2, "0")}.mkv`,
    );
    paths[21] = `/Movies/${exactTitle}.MKV`;
    savedMoviesFolder = "/Movies";
    scanMoviesMock.mockResolvedValue(paths);

    render(<App />);
    selectLibrary();
    await screen.findByText("Title 01");
    resizeGallery("library", 1528, 136);
    expect(visibleCardCount("Movies")).toBe(7);
    expect(screen.queryByText(exactTitle)).toBeNull();

    searchMovies("target");

    const matchingHeading = screen.getByRole("heading", {
      level: 3,
      name: "Needle — TARGET 22",
    });
    expect(matchingHeading.textContent).toBe(exactTitle);
    expect(visibleCardCount("Movies")).toBe(1);
    expect(screen.getByText("Page 1 of 1")).toBeTruthy();

    selectDashboard();
    expect(
      screen.getByRole("heading", {
        level: 3,
        name: "25 supported Movies",
      }),
    ).toBeTruthy();

    selectLibrary();
    expect(screen.getByRole("textbox", { name: "Search titles" })).toHaveProperty(
      "value",
      "target",
    );
    expect(screen.getByRole("heading", { level: 3 }).textContent).toBe(
      exactTitle,
    );
    expect(scanMoviesMock).toHaveBeenCalledTimes(1);
    expect(queryMoviesStorageMock).toHaveBeenCalledTimes(1);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("preserves title identity, treats whitespace as no filter, and clears from the keyboard", async () => {
    const exactTitle = "映画  —  Final.CUT!";
    savedMoviesFolder = "/Movies";
    scanMoviesMock.mockResolvedValue([
      `/Movies/${exactTitle}.MKV`,
      "/Movies/Other title.mp4",
    ]);

    render(<App />);
    selectLibrary();
    await screen.findByText("Other title");

    searchMovies("final.cut!");
    const matchingHeading = screen.getByRole("heading", {
      level: 3,
      name: "映画 — Final.CUT!",
    });
    expect(matchingHeading.textContent).toBe(exactTitle);
    expect(visibleCardCount("Movies")).toBe(1);

    searchMovies(" \t ");
    expect(visibleCardCount("Movies")).toBe(2);
    const clearSearch = screen.getByRole("button", {
      name: "Clear Movies search",
    });
    clearSearch.focus();
    fireEvent.keyDown(clearSearch, { key: "Enter" });
    fireEvent.click(clearSearch);

    expect(screen.getByRole("textbox", { name: "Search titles" })).toHaveProperty(
      "value",
      "",
    );
    expect(
      screen.queryByRole("button", { name: "Clear Movies search" }),
    ).toBeNull();
    expect(visibleCardCount("Movies")).toBe(2);
    expect(scanMoviesMock).toHaveBeenCalledTimes(1);
    expect(queryMoviesStorageMock).toHaveBeenCalledTimes(1);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("does not search paths and shows an active-search no-match state", async () => {
    savedMoviesFolder = "/Movies/Searchable Folder";
    scanMoviesMock.mockResolvedValue([
      "/Movies/Searchable Folder/Actual title.mkv",
    ]);

    render(<App />);
    selectLibrary();
    await screen.findByText("Actual title");

    searchMovies("Searchable Folder");

    expect(
      screen.getByRole("heading", {
        level: 2,
        name: "No Movies match this search",
      }),
    ).toBeTruthy();
    expect(screen.queryByRole("list", { name: "Movies" })).toBeNull();
    expect(
      screen.queryByRole("heading", {
        level: 2,
        name: "No supported videos found",
      }),
    ).toBeNull();
    expect(screen.getByText(/0 Movies match the current title search/)).toBeTruthy();

    fireEvent.click(
      screen.getByRole("button", { name: "Clear Movies search" }),
    );
    expect(screen.getByText("Actual title")).toBeTruthy();
    expect(scanMoviesMock).toHaveBeenCalledTimes(1);
    expect(queryMoviesStorageMock).toHaveBeenCalledTimes(1);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("preserves a valid filtered page through navigation, appearance, and resize before resetting on query changes", async () => {
    const paths = [
      ...Array.from(
        { length: 18 },
        (_, index) =>
          `/Movies/Match ${String(index + 1).padStart(2, "0")}.mkv`,
      ),
      ...Array.from(
        { length: 7 },
        (_, index) =>
          `/Movies/Other ${String(index + 1).padStart(2, "0")}.mp4`,
      ),
    ];
    savedMoviesFolder = "/Movies";
    scanMoviesMock.mockResolvedValue(paths);

    render(<App />);
    selectLibrary();
    await screen.findByText("Match 01");
    resizeGallery("library", 1528, 136);
    searchMovies("Match");
    expect(screen.getByText("Page 1 of 3")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Next Movies page" }));
    fireEvent.click(screen.getByRole("button", { name: "Next Movies page" }));
    expect(screen.getByText("Page 3 of 3")).toBeTruthy();

    selectDashboard();
    expect(
      screen.getByRole("heading", {
        level: 3,
        name: "25 supported Movies",
      }),
    ).toBeTruthy();
    selectSettings();
    fireEvent.click(screen.getByRole("radio", { name: "Dark" }));
    selectLibrary();
    expect(screen.getByRole("textbox", { name: "Search titles" })).toHaveProperty(
      "value",
      "Match",
    );
    expect(screen.getByText("Page 3 of 3")).toBeTruthy();

    resizeGallery("library", 1088, 136);
    expect(screen.getByText("Page 3 of 4")).toBeTruthy();
    expect(screen.getByText("Match 11")).toBeTruthy();

    searchMovies("Match 0");
    expect(screen.getByText("Page 1 of 2")).toBeTruthy();
    expect(screen.getByText("Match 01")).toBeTruthy();
    fireEvent.click(
      screen.getByRole("button", { name: "Clear Movies search" }),
    );
    expect(screen.getByText("Page 1 of 5")).toBeTruthy();
    expect(scanMoviesMock).toHaveBeenCalledTimes(1);
    expect(queryMoviesStorageMock).toHaveBeenCalledTimes(1);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("keeps the query current through refresh, folder replacement, and a stale scan", async () => {
    const earlierScan = createDeferred<string[]>();
    savedMoviesFolder = "/Movies/Old";
    scanMoviesMock
      .mockResolvedValueOnce(["/Movies/Old/Old Current.mkv"])
      .mockReturnValueOnce(earlierScan.promise)
      .mockResolvedValueOnce(["/Movies/New/New Current.mp4"])
      .mockResolvedValueOnce(["/Movies/New/Replacement Current.mkv"]);
    openFolderMock.mockResolvedValue("/Movies/New");

    render(<App />);
    selectLibrary();
    await screen.findByText("Old Current");
    searchMovies("Current");

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    selectSettings();
    fireEvent.click(screen.getByRole("button", { name: "Change folder" }));
    expect(await screen.findByText("/Movies/New")).toBeTruthy();

    selectLibrary();
    expect(await screen.findByText("New Current")).toBeTruthy();
    expect(screen.getByRole("textbox", { name: "Search titles" })).toHaveProperty(
      "value",
      "Current",
    );

    await act(async () => {
      earlierScan.resolve(["/Movies/Old/Obsolete Current.mp4"]);
      await earlierScan.promise;
    });
    expect(screen.queryByText("Obsolete Current")).toBeNull();
    expect(screen.getByText("New Current")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(await screen.findByText("Replacement Current")).toBeTruthy();
    expect(screen.queryByText("New Current")).toBeNull();
    expect(screen.getByRole("textbox", { name: "Search titles" })).toHaveProperty(
      "value",
      "Current",
    );
    expect(scanMoviesMock).toHaveBeenCalledTimes(4);
    expect(queryMoviesStorageMock).toHaveBeenCalledTimes(4);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("isolates every card action to the exact filtered movie and updates the filtered result after Trash", async () => {
    const exactTitle = "映画  —  Action.CUT!";
    const exactPath = `/Movies/${exactTitle}.MKV`;
    savedMoviesFolder = "/Movies";
    scanMoviesMock.mockResolvedValue([exactPath, "/Movies/Other movie.mp4"]);

    render(<App />);
    selectLibrary();
    await screen.findByText("Other movie");
    searchMovies("action.cut!");

    const matchingHeading = screen.getByRole("heading", {
      level: 3,
      name: "映画 — Action.CUT!",
    });
    expect(matchingHeading.textContent).toBe(exactTitle);
    const card = matchingHeading.closest("article") as HTMLElement;

    fireEvent.click(within(card).getByRole("button", { name: /Copy title:/ }));
    expect(clipboardWriteMock).toHaveBeenCalledWith(exactTitle);
    fireEvent.click(within(card).getByRole("button", { name: /Open movie:/ }));
    fireEvent.click(within(card).getByRole("button", { name: /Reveal movie:/ }));
    await waitFor(() => {
      expect(openMovieMock).toHaveBeenCalledWith({ path: exactPath });
      expect(revealMovieMock).toHaveBeenCalledWith({ path: exactPath });
    });

    fireEvent.click(
      within(card).getByRole("button", {
        name: /Move movie to Trash or Recycle Bin:/,
      }),
    );
    const dialog = await screen.findByRole("alertdialog");
    fireEvent.click(
      within(dialog).getByRole("button", {
        name: /Confirm moving movie to Trash or Recycle Bin:/,
      }),
    );

    expect(
      await screen.findByRole("heading", {
        level: 2,
        name: "No Movies match this search",
      }),
    ).toBeTruthy();
    expect(trashMovieMock).toHaveBeenCalledTimes(1);
    expect(trashMovieMock).toHaveBeenCalledWith({ path: exactPath });
    expect(screen.getByRole("textbox", { name: "Search titles" })).toHaveProperty(
      "value",
      "action.cut!",
    );
    expect(scanMoviesMock).toHaveBeenCalledTimes(1);
    expect(queryMoviesStorageMock).toHaveBeenCalledTimes(2);
    expect(fetchMock).not.toHaveBeenCalled();
  });
});

describe("Movies Library title sorting", () => {
  it("orders the complete title set case-insensitively in both directions with deterministic ties", async () => {
    const exactUnicodeTitle = "映画  —  Exact!";
    const paths = [
      "/Movies/Zulu.mkv",
      "/Movies/B/same.mkv",
      "/Movies/Beta.mp4",
      `/Movies/${exactUnicodeTitle}.MKV`,
      ...Array.from(
        { length: 17 },
        (_, index) =>
          `/Movies/Middle ${String(index + 1).padStart(2, "0")}.mkv`,
      ),
      "/Movies/alpha.mkv",
      "/Movies/ALPHA.mp4",
      "/Movies/A/same.mkv",
      "/Movies/Punctuation !.mkv",
    ];
    savedMoviesFolder = "/Movies";
    scanMoviesMock.mockResolvedValue(paths);

    render(<App />);
    selectLibrary();
    await screen.findByText("Zulu");
    resizeGallery("library", 1528, 136);

    const sortControl = screen.getByRole("combobox", {
      name: "Sort titles",
    });
    expect(sortControl).toHaveProperty("value", "ascending");
    expect(
      within(sortControl).getAllByRole("option").map((option) => ({
        text: option.textContent,
        value: (option as HTMLOptionElement).value,
      })),
    ).toEqual([
      { text: "Title A–Z", value: "ascending" },
      { text: "Title Z–A", value: "descending" },
    ]);
    expect(visibleMovieTitles()).toEqual([
      "ALPHA",
      "alpha",
      "Beta",
      "Middle 01",
      "Middle 02",
      "Middle 03",
      "Middle 04",
    ]);
    expect(screen.getByText("Page 1 of 4")).toBeTruthy();

    sortControl.focus();
    fireEvent.keyDown(sortControl, { key: "End" });
    sortMovies("descending");

    expect(document.activeElement).toBe(sortControl);
    expect(sortControl).toHaveProperty("value", "descending");
    expect(visibleMovieTitles()).toEqual([
      exactUnicodeTitle,
      "Zulu",
      "same",
      "same",
      "Punctuation !",
      "Middle 17",
      "Middle 16",
    ]);
    const unicodeHeading = screen.getByRole("heading", {
      level: 3,
      name: "映画 — Exact!",
    });
    expect(unicodeHeading.textContent).toBe(exactUnicodeTitle);
    expect(screen.getByText("Page 1 of 4")).toBeTruthy();
    expect(scanMoviesMock).toHaveBeenCalledTimes(1);
    expect(queryMoviesStorageMock).toHaveBeenCalledTimes(1);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("composes with search and preserves direction and a valid page through navigation and resize", async () => {
    const paths = [
      ...Array.from(
        { length: 18 },
        (_, index) =>
          `/Movies/Match ${String(18 - index).padStart(2, "0")}.mkv`,
      ),
      ...Array.from(
        { length: 7 },
        (_, index) =>
          `/Movies/Other ${String(index + 1).padStart(2, "0")}.mp4`,
      ),
    ];
    savedMoviesFolder = "/Movies";
    scanMoviesMock.mockResolvedValue(paths);

    render(<App />);
    selectLibrary();
    await screen.findByText("Match 01");
    resizeGallery("library", 1528, 136);
    searchMovies("match");
    sortMovies("descending");

    expect(visibleMovieTitles()[0]).toBe("Match 18");
    expect(screen.getByText("Page 1 of 3")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Next Movies page" }));
    fireEvent.click(screen.getByRole("button", { name: "Next Movies page" }));
    expect(screen.getByText("Page 3 of 3")).toBeTruthy();
    expect(visibleMovieTitles()).toEqual([
      "Match 04",
      "Match 03",
      "Match 02",
      "Match 01",
    ]);

    selectDashboard();
    expect(
      screen.getByRole("heading", {
        level: 3,
        name: "25 supported Movies",
      }),
    ).toBeTruthy();
    expect(storageValue("Total")).toBe("1.0 TiB");
    expect(storageValue("Used")).toBe("768.0 GiB");
    expect(storageValue("Free")).toBe("256.0 GiB");
    selectSettings();
    fireEvent.click(screen.getByRole("radio", { name: "Dark" }));
    selectLibrary();

    expect(
      screen.getByRole("textbox", { name: "Search titles" }),
    ).toHaveProperty("value", "match");
    expect(
      screen.getByRole("combobox", { name: "Sort titles" }),
    ).toHaveProperty("value", "descending");
    expect(screen.getByText("Page 3 of 3")).toBeTruthy();

    resizeGallery("library", 1088, 136);
    expect(screen.getByText("Page 3 of 4")).toBeTruthy();
    expect(visibleMovieTitles()[0]).toBe("Match 08");

    sortMovies("ascending");
    expect(screen.getByText("Page 1 of 4")).toBeTruthy();
    expect(visibleMovieTitles()[0]).toBe("Match 01");
    expect(
      screen.getByRole("textbox", { name: "Search titles" }),
    ).toHaveProperty("value", "match");
    expect(scanMoviesMock).toHaveBeenCalledTimes(1);
    expect(queryMoviesStorageMock).toHaveBeenCalledTimes(1);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("reorders only current filtered data through refresh, folder replacement, stale scans, and Trash", async () => {
    const earlierScan = createDeferred<string[]>();
    const trashedPath = "/Movies/New/Charlie Current.mkv";
    savedMoviesFolder = "/Movies/Old";
    scanMoviesMock
      .mockResolvedValueOnce([
        "/Movies/Old/Alpha Current.mkv",
        "/Movies/Old/Zulu Current.mp4",
      ])
      .mockReturnValueOnce(earlierScan.promise)
      .mockResolvedValueOnce([
        "/Movies/New/Bravo Current.mkv",
        "/Movies/New/Echo Current.mp4",
        "/Movies/New/Ignore me.mkv",
      ])
      .mockResolvedValueOnce([
        trashedPath,
        "/Movies/New/Beta Current.mp4",
        "/Movies/New/Ignore me.mkv",
      ]);
    openFolderMock.mockResolvedValue("/Movies/New");

    render(<App />);
    selectLibrary();
    await screen.findByText("Alpha Current");
    searchMovies("Current");
    sortMovies("descending");
    expect(visibleMovieTitles()).toEqual(["Zulu Current", "Alpha Current"]);

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    selectSettings();
    fireEvent.click(screen.getByRole("button", { name: "Change folder" }));
    expect(await screen.findByText("/Movies/New")).toBeTruthy();
    selectLibrary();
    expect(await screen.findByText("Echo Current")).toBeTruthy();
    expect(visibleMovieTitles()).toEqual(["Echo Current", "Bravo Current"]);

    await act(async () => {
      earlierScan.resolve(["/Movies/Old/Obsolete Current.mp4"]);
      await earlierScan.promise;
    });
    expect(screen.queryByText("Obsolete Current")).toBeNull();
    expect(visibleMovieTitles()).toEqual(["Echo Current", "Bravo Current"]);

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    const currentHeading = await screen.findByRole("heading", {
      level: 3,
      name: "Charlie Current",
    });
    expect(visibleMovieTitles()).toEqual(["Charlie Current", "Beta Current"]);
    const currentCard = currentHeading.closest("article") as HTMLElement;
    fireEvent.click(
      within(currentCard).getByRole("button", {
        name: /Move movie to Trash or Recycle Bin:/,
      }),
    );
    const dialog = await screen.findByRole("alertdialog");
    fireEvent.click(
      within(dialog).getByRole("button", {
        name: /Confirm moving movie to Trash or Recycle Bin:/,
      }),
    );

    await waitFor(() => expect(visibleMovieTitles()).toEqual(["Beta Current"]));
    expect(trashMovieMock).toHaveBeenCalledWith({ path: trashedPath });
    expect(
      screen.getByRole("textbox", { name: "Search titles" }),
    ).toHaveProperty("value", "Current");
    expect(
      screen.getByRole("combobox", { name: "Sort titles" }),
    ).toHaveProperty("value", "descending");
    expect(scanMoviesMock).toHaveBeenCalledTimes(4);
    await waitFor(() =>
      expect(queryMoviesStorageMock).toHaveBeenCalledTimes(5),
    );
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("keeps equal folded titles and every card action bound to deterministic exact paths", async () => {
    const firstPath = "/Movies/A/Same Title.MKV";
    const secondPath = "/Movies/B/Same Title.MKV";
    const lowercasePath = "/Movies/C/same title.mp4";
    const parentActivation = vi.fn();
    savedMoviesFolder = "/Movies";
    scanMoviesMock.mockResolvedValue([
      lowercasePath,
      secondPath,
      firstPath,
    ]);

    render(
      <div onClick={parentActivation} onPointerDown={parentActivation}>
        <App />
      </div>,
    );
    selectLibrary();
    await screen.findByText("same title");
    searchMovies("same title");
    sortMovies("descending");
    parentActivation.mockClear();

    let cards = within(
      screen.getByRole("list", { name: "Movies" }),
    ).getAllByRole("article");
    expect(
      within(cards[0]).getByRole("heading", { level: 3 }).textContent,
    ).toBe("Same Title");
    fireEvent.click(
      within(cards[0]).getByRole("button", { name: /Copy title:/ }),
    );
    fireEvent.click(
      within(cards[0]).getByRole("button", { name: /Open movie:/ }),
    );
    fireEvent.click(
      within(cards[0]).getByRole("button", { name: /Reveal movie:/ }),
    );
    await waitFor(() => {
      expect(clipboardWriteMock).toHaveBeenCalledWith("Same Title");
      expect(openMovieMock).toHaveBeenCalledWith({ path: firstPath });
      expect(revealMovieMock).toHaveBeenCalledWith({ path: firstPath });
    });

    fireEvent.click(
      within(cards[0]).getByRole("button", {
        name: /Move movie to Trash or Recycle Bin:/,
      }),
    );
    const dialog = await screen.findByRole("alertdialog");
    fireEvent.click(
      within(dialog).getByRole("button", {
        name: /Confirm moving movie to Trash or Recycle Bin:/,
      }),
    );

    await waitFor(() =>
      expect(trashMovieMock).toHaveBeenCalledWith({ path: firstPath }),
    );
    sortMovies("ascending");
    cards = within(
      screen.getByRole("list", { name: "Movies" }),
    ).getAllByRole("article");
    expect(
      within(cards[0]).getByRole("heading", { level: 3 }).textContent,
    ).toBe("Same Title");
    fireEvent.click(
      within(cards[0]).getByRole("button", { name: /Open movie:/ }),
    );
    await waitFor(() => {
      expect(openMovieMock).toHaveBeenNthCalledWith(2, { path: secondPath });
    });

    expect(parentActivation).not.toHaveBeenCalled();
    expect(scanMoviesMock).toHaveBeenCalledTimes(1);
    await waitFor(() =>
      expect(queryMoviesStorageMock).toHaveBeenCalledTimes(2),
    );
    expect(fetchMock).not.toHaveBeenCalled();
  });
});

describe("resize-aware media galleries", () => {
  it("updates Discover through the 25 to 7 to 10 regression without refetching and clamps its page", async () => {
    const results = Array.from({ length: 25 }, (_, index) => ({
      id: index + 1,
      title: `Discover ${String(index + 1).padStart(2, "0")}`,
    }));
    loadTmdbTokenMock.mockResolvedValue("fixture-token");
    fetchMock.mockResolvedValue(jsonResponse({ results }));

    render(<App />);
    selectDiscover();
    await screen.findByText("Discover 01");

    resizeGallery("discover", 1088, 2408);
    expect(visibleCardCount("Weekly trending Movies")).toBe(25);
    expect(screen.getByText("Page 1 of 1")).toBeTruthy();

    const firstCopyButton = screen.getByRole("button", {
      name: "Copy title: Discover 01",
    });
    fireEvent.click(firstCopyButton);
    expect(
      await screen.findByRole("button", {
        name: "Copied title: Discover 01",
      }),
    ).toBeTruthy();

    resizeGallery("discover", 1528, 472);
    expect(visibleCardCount("Weekly trending Movies")).toBe(7);
    expect(screen.getByText("Page 1 of 4")).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "Copied title: Discover 01" }),
    ).toBeTruthy();
    expect(fetchMock).toHaveBeenCalledTimes(1);

    const nextPage = screen.getByRole("button", {
      name: "Next Weekly trending Movies page",
    });
    nextPage.focus();
    expect(document.activeElement).toBe(nextPage);

    resizeGallery("discover", 1088, 956);
    expect(visibleCardCount("Weekly trending Movies")).toBe(10);
    expect(screen.getByText("Page 1 of 3")).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "Copied title: Discover 01" }),
    ).toBeTruthy();

    resizeGallery("discover", 1528, 472);
    for (let page = 1; page < 4; page += 1) {
      fireEvent.click(
        screen.getByRole("button", {
          name: "Next Weekly trending Movies page",
        }),
      );
    }
    expect(screen.getByText("Page 4 of 4")).toBeTruthy();
    expect(visibleCardCount("Weekly trending Movies")).toBe(4);
    expect(screen.getByText("Discover 22")).toBeTruthy();

    resizeGallery("discover", 1088, 956);
    expect(screen.getByText("Page 3 of 3")).toBeTruthy();
    expect(visibleCardCount("Weekly trending Movies")).toBe(5);
    expect(screen.getByText("Discover 21")).toBeTruthy();
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe(
      "Discover",
    );
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("updates Library through the 25 to 7 to 10 regression without rescanning and clamps its page", async () => {
    const paths = Array.from(
      { length: 25 },
      (_, index) =>
        `/Movies/Library ${String(index + 1).padStart(2, "0")}.mp4`,
    );
    savedMoviesFolder = "/Movies";
    scanMoviesMock.mockResolvedValue(paths);

    render(<App />);
    selectLibrary();
    await screen.findByText("Library 01");

    resizeGallery("library", 1088, 728);
    expect(visibleCardCount("Movies")).toBe(25);
    expect(screen.getByText("Page 1 of 1")).toBeTruthy();

    fireEvent.click(
      screen.getByRole("button", { name: "Copy title: Library 01" }),
    );
    expect(
      await screen.findByRole("button", {
        name: "Copied title: Library 01",
      }),
    ).toBeTruthy();

    resizeGallery("library", 1528, 136);
    expect(visibleCardCount("Movies")).toBe(7);
    expect(screen.getByText("Page 1 of 4")).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "Copied title: Library 01" }),
    ).toBeTruthy();
    expect(scanMoviesMock).toHaveBeenCalledTimes(1);

    const nextPage = screen.getByRole("button", {
      name: "Next Movies page",
    });
    nextPage.focus();
    expect(document.activeElement).toBe(nextPage);

    resizeGallery("library", 1088, 284);
    expect(visibleCardCount("Movies")).toBe(10);
    expect(screen.getByText("Page 1 of 3")).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "Copied title: Library 01" }),
    ).toBeTruthy();

    resizeGallery("library", 1528, 136);
    for (let page = 1; page < 4; page += 1) {
      fireEvent.click(
        screen.getByRole("button", { name: "Next Movies page" }),
      );
    }
    expect(screen.getByText("Page 4 of 4")).toBeTruthy();
    expect(visibleCardCount("Movies")).toBe(4);
    expect(screen.getByText("Library 22")).toBeTruthy();

    resizeGallery("library", 1088, 284);
    expect(screen.getByText("Page 3 of 3")).toBeTruthy();
    expect(visibleCardCount("Movies")).toBe(5);
    expect(screen.getByText("Library 21")).toBeTruthy();
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe(
      "Library",
    );
    expect(savedMoviesFolder).toBe("/Movies");
    expect(scanMoviesMock).toHaveBeenCalledTimes(1);
  });
});
