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
let loadTmdbTokenMock: Mock<() => Promise<string | null>>;
let saveTmdbTokenMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<void>
>;
let clearTmdbTokenMock: Mock<() => Promise<void>>;
let fetchJavdbVrCatalogMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<string>
>;
let fetchSukebeiVrReleasesMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<string>
>;
let inspectSukebeiVrTorrentMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<string[]>
>;
let invalidateVerifiedVrTorrentMock: Mock<() => Promise<void>>;
let saveVerifiedVrTorrentMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<boolean>
>;
let fetchMock: Mock<typeof fetch>;
let clipboardWriteMock: Mock<(text: string) => Promise<void>>;
let resizeObserverRecords: ResizeObserverRecord[] = [];
let gallerySizes: Record<
  "discover" | "library",
  { width: number; height: number }
>;
let savedMoviesFolder: string | null;

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
  loadTmdbTokenMock = vi.fn().mockResolvedValue(null);
  saveTmdbTokenMock = vi.fn().mockResolvedValue(undefined);
  clearTmdbTokenMock = vi.fn().mockResolvedValue(undefined);
  fetchJavdbVrCatalogMock = vi
    .fn()
    .mockResolvedValue('<div class="movie-list"></div>');
  fetchSukebeiVrReleasesMock = vi
    .fn()
    .mockResolvedValue(sukebeiReleaseFixture([]));
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
        case "load_tmdb_token":
          return loadTmdbTokenMock();
        case "save_tmdb_token":
          return saveTmdbTokenMock(parameters);
        case "clear_tmdb_token":
          return clearTmdbTokenMock();
        case "fetch_javdb_vr_catalog":
          return fetchJavdbVrCatalogMock(parameters);
        case "fetch_sukebei_vr_releases":
          return fetchSukebeiVrReleasesMock(parameters);
        case "inspect_sukebei_vr_torrent":
          return inspectSukebeiVrTorrentMock(parameters);
        case "invalidate_verified_vr_torrent":
          return invalidateVerifiedVrTorrentMock();
        case "save_verified_vr_torrent":
          return saveVerifiedVrTorrentMock(parameters);
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
        name: "Downloads are not available yet",
      }),
    ).toBeTruthy();

    selectSettings();
    expect(
      screen.getByRole("heading", {
        level: 2,
        name: "Other settings are not configured",
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

describe("VR Discover and verified release comparison", () => {
  it("requires an explicit exact-code search and exposes only verified releases for explicit selection", async () => {
    const exactReleaseName =
      "【VR】 MdVr_00419  Director’s Cut\t—\n特別版!?";
    const ambiguousPackName = "MDVR-419 + ABC-123 pack";
    fetchJavdbVrCatalogMock.mockResolvedValue(
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
    expect(fetchJavdbVrCatalogMock).not.toHaveBeenCalled();

    fireEvent.change(codeInput, { target: { value: "mdvr_00419" } });
    expect(fetchJavdbVrCatalogMock).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Search" }));

    const codeHeading = await screen.findByRole("heading", {
      level: 3,
      name: "MDVR-419",
    });
    const vrCard = codeHeading.closest("article");
    expect(vrCard).not.toBeNull();
    expect(within(vrCard as HTMLElement).getByText("Exact provider title")).toBeTruthy();
    expect(within(vrCard as HTMLElement).getByText("JavDB")).toBeTruthy();
    expect(fetchJavdbVrCatalogMock).toHaveBeenCalledWith({ code: "MDVR-419" });
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
    fetchJavdbVrCatalogMock.mockResolvedValue(
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
    fetchJavdbVrCatalogMock.mockResolvedValue(javdbCatalogFixture("MDVR-419"));
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
    fetchJavdbVrCatalogMock.mockResolvedValue(javdbCatalogFixture("MDVR-419"));
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
    fetchJavdbVrCatalogMock.mockResolvedValue(javdbCatalogFixture("MDVR-419"));
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
    fetchJavdbVrCatalogMock
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
    fetchJavdbVrCatalogMock.mockResolvedValue(javdbCatalogFixture("MDVR-419"));
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
    fetchJavdbVrCatalogMock
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
    fetchJavdbVrCatalogMock.mockResolvedValue(
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
    expect(fetchJavdbVrCatalogMock).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("radio", { name: "Movies" }));
    expect(
      screen.getByRole("heading", { level: 3, name: "Preserved Movie" }),
    ).toBeTruthy();
    expect(fetchMock).toHaveBeenCalledTimes(1);
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
