import { open } from "@tauri-apps/plugin-dialog";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
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

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const systemDarkModeQuery = "(prefers-color-scheme: dark)";
const openFolderMock = vi.mocked(open);

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
let openMovieMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<void>
>;
let loadTmdbTokenMock: Mock<() => Promise<string | null>>;
let saveTmdbTokenMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<void>
>;
let clearTmdbTokenMock: Mock<() => Promise<void>>;
let fetchMock: Mock<typeof fetch>;
let clipboardWriteMock: Mock<(text: string) => Promise<void>>;
let resizeObserverRecords: ResizeObserverRecord[] = [];

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
    this.record.callback(
      [createResizeEntry(target, 2000, 3000)],
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
  const promise = new Promise<T>((resolve) => {
    resolvePromise = resolve;
  });
  return { promise, resolve: resolvePromise };
}

function selectLibrary() {
  fireEvent.click(screen.getByRole("button", { name: "Library" }));
}

function selectDiscover() {
  fireEvent.click(screen.getByRole("button", { name: "Discover" }));
}

function selectSettings() {
  fireEvent.click(screen.getByRole("button", { name: "Settings" }));
}

function jsonResponse(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), {
    headers: { "Content-Type": "application/json" },
    status,
  });
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

beforeEach(() => {
  systemPrefersDark = false;
  mediaQueryListeners = new Set();
  resizeObserverRecords = [];
  scanMoviesMock = vi.fn().mockResolvedValue([]);
  openMovieMock = vi.fn().mockResolvedValue(undefined);
  loadTmdbTokenMock = vi.fn().mockResolvedValue(null);
  saveTmdbTokenMock = vi.fn().mockResolvedValue(undefined);
  clearTmdbTokenMock = vi.fn().mockResolvedValue(undefined);
  invokeMock = vi.fn(
    (command: string, parameters?: Record<string, unknown>) => {
      switch (command) {
        case "scan_movies":
          return scanMoviesMock(parameters);
        case "open_movie":
          return openMovieMock(parameters);
        case "load_tmdb_token":
          return loadTmdbTokenMock();
        case "save_tmdb_token":
          return saveTmdbTokenMock(parameters);
        case "clear_tmdb_token":
          return clearTmdbTokenMock();
        default:
          return Promise.reject(new Error("Unexpected native command."));
      }
    },
  );
  fetchMock = vi.fn();
  clipboardWriteMock = vi.fn().mockResolvedValue(undefined);
  openFolderMock.mockReset();
  openFolderMock.mockResolvedValue(null);
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
      screen.getByRole("heading", {
        level: 2,
        name: "Dashboard data is not available yet",
      }),
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
    expect(parentActivation).not.toHaveBeenCalled();
    expect(copyButton.getAttribute("aria-label")).toBe(
      `Copied title: ${title}`,
    );
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

describe("local Movies library", () => {
  it("persists the selected folder and clearing it blocks a late scan", async () => {
    const pendingScan = createDeferred<string[]>();
    scanMoviesMock.mockReturnValue(pendingScan.promise);
    openFolderMock.mockResolvedValue("/Local/Movies — 家族");

    render(<App />);
    selectSettings();
    fireEvent.click(screen.getByRole("button", { name: "Choose folder" }));

    expect(await screen.findByText("/Local/Movies — 家族")).toBeTruthy();
    expect(window.localStorage.getItem("auto-video-movies-folder")).toBe(
      "/Local/Movies — 家族",
    );
    expect(openFolderMock).toHaveBeenCalledWith({
      directory: true,
      multiple: false,
      title: "Choose Movies folder",
    });

    cleanup();
    render(<App />);
    selectSettings();
    expect(screen.getByText("/Local/Movies — 家族")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Clear folder" }));
    expect(screen.getByText("No Movies folder configured.")).toBeTruthy();
    expect(window.localStorage.getItem("auto-video-movies-folder")).toBeNull();

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
    window.localStorage.setItem("auto-video-movies-folder", "/Movies");
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
    window.localStorage.setItem("auto-video-movies-folder", "/Movies");
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
    window.localStorage.setItem("auto-video-movies-folder", "/Movies");
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
    window.localStorage.setItem("auto-video-movies-folder", "/Movies");
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
    fireEvent.click(screen.getByRole("button", { name: "Next Movies page" }));

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
    expect(screen.getByText("Page 2 of 4")).toBeTruthy();
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
    expect(screen.getByText("Page 2 of 3")).toBeTruthy();
    expect(
      screen.getByRole("button", { name: /Copied title:/ }),
    ).toBeTruthy();
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe(
      "Library",
    );
    expect(window.localStorage.getItem("auto-video-movies-folder")).toBe(
      "/Movies",
    );
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
      window.localStorage.setItem("auto-video-movies-folder", "/Movies");
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
      expect(clipboardWriteMock).not.toHaveBeenCalled();
      expect(scanMoviesMock).toHaveBeenCalledTimes(1);
    },
  );

  it("refresh replaces files added or removed since the previous scan", async () => {
    window.localStorage.setItem("auto-video-movies-folder", "/Movies");
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
    expect(scanMoviesMock).toHaveBeenNthCalledWith(2, {
      folder: "/Movies",
    });
  });

  it("shows distinct scanning and empty-folder states", async () => {
    const pendingScan = createDeferred<string[]>();
    window.localStorage.setItem("auto-video-movies-folder", "/Movies");
    scanMoviesMock.mockReturnValue(pendingScan.promise);

    render(<App />);
    selectLibrary();

    expect(
      screen.getByRole("status").querySelector("h2")?.textContent,
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
    window.localStorage.setItem("auto-video-movies-folder", "/Movies");
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
    let oldFolderScans = 0;
    window.localStorage.setItem("auto-video-movies-folder", "/Movies/Old");
    scanMoviesMock.mockImplementation(
      (argumentsValue?: Record<string, unknown>) => {
        if (argumentsValue?.folder === "/Movies/Old") {
          oldFolderScans += 1;
          return oldFolderScans === 1
            ? Promise.resolve(["/Movies/Old/Old title.mp4"])
            : earlierScan.promise;
        }
        return Promise.resolve(["/Movies/New/New title.mkv"]);
      },
    );
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
    expect(window.localStorage.getItem("auto-video-movies-folder")).toBeNull();
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
    window.localStorage.setItem("auto-video-movies-folder", "/Movies");
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
    expect(window.localStorage.getItem("auto-video-movies-folder")).toBe(
      "/Movies",
    );
    expect(scanMoviesMock).toHaveBeenCalledTimes(1);
  });
});
