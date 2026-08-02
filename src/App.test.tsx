import { open } from "@tauri-apps/plugin-dialog";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
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
let loadTmdbTokenMock: Mock<() => Promise<string | null>>;
let saveTmdbTokenMock: Mock<
  (parameters?: Record<string, unknown>) => Promise<void>
>;
let clearTmdbTokenMock: Mock<() => Promise<void>>;
let fetchMock: Mock<typeof fetch>;

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

beforeEach(() => {
  systemPrefersDark = false;
  mediaQueryListeners = new Set();
  scanMoviesMock = vi.fn().mockResolvedValue([]);
  loadTmdbTokenMock = vi.fn().mockResolvedValue(null);
  saveTmdbTokenMock = vi.fn().mockResolvedValue(undefined);
  clearTmdbTokenMock = vi.fn().mockResolvedValue(undefined);
  invokeMock = vi.fn(
    (command: string, parameters?: Record<string, unknown>) => {
      switch (command) {
        case "scan_movies":
          return scanMoviesMock(parameters);
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
  openFolderMock.mockReset();
  openFolderMock.mockResolvedValue(null);
  window.localStorage.clear();
  vi.stubGlobal("matchMedia", vi.fn(createMediaQueryList));
  vi.stubGlobal("__TAURI__", { core: { invoke: invokeMock } });
  vi.stubGlobal("fetch", fetchMock);
});

afterEach(() => {
  cleanup();
  window.localStorage.clear();
  delete document.documentElement.dataset.appearance;
  delete document.documentElement.dataset.theme;
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
