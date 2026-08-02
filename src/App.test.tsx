import { open } from "@tauri-apps/plugin-dialog";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import App from "./App";

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const systemDarkModeQuery = "(prefers-color-scheme: dark)";
const openFolderMock = vi.mocked(open);

let systemPrefersDark = false;
let mediaQueryListeners = new Set<(event: MediaQueryListEvent) => void>();
let invokeMock: ReturnType<typeof vi.fn>;

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

function selectSettings() {
  fireEvent.click(screen.getByRole("button", { name: "Settings" }));
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
  invokeMock = vi.fn().mockResolvedValue([]);
  openFolderMock.mockReset();
  openFolderMock.mockResolvedValue(null);
  window.localStorage.clear();
  vi.stubGlobal("matchMedia", vi.fn(createMediaQueryList));
  vi.stubGlobal("__TAURI__", { core: { invoke: invokeMock } });
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

  it("shows truthful unavailable states without fabricated product data", () => {
    render(<App />);

    expect(
      screen.getByRole("heading", {
        level: 2,
        name: "Dashboard data is not available yet",
      }),
    ).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Discover" }));
    expect(
      screen.getByRole("heading", {
        level: 2,
        name: "Discovery is not configured",
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

describe("local Movies library", () => {
  it("persists the selected folder and clearing it blocks a late scan", async () => {
    const pendingScan = createDeferred<string[]>();
    invokeMock.mockReturnValue(pendingScan.promise);
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
    invokeMock.mockResolvedValue([
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
    invokeMock
      .mockResolvedValueOnce(["/Movies/First.mp4"])
      .mockResolvedValueOnce(["/Movies/Second.mkv"]);

    render(<App />);
    selectLibrary();
    expect(await screen.findByText("First")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));

    expect(await screen.findByText("Second")).toBeTruthy();
    expect(screen.queryByText("First")).toBeNull();
    expect(invokeMock).toHaveBeenCalledTimes(2);
    expect(invokeMock).toHaveBeenNthCalledWith(2, "scan_movies", {
      folder: "/Movies",
    });
  });

  it("shows distinct scanning and empty-folder states", async () => {
    const pendingScan = createDeferred<string[]>();
    window.localStorage.setItem("auto-video-movies-folder", "/Movies");
    invokeMock.mockReturnValue(pendingScan.promise);

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
    invokeMock.mockRejectedValueOnce("movies_folder_unavailable");

    render(<App />);
    selectLibrary();
    expect(
      await screen.findByRole("heading", {
        level: 2,
        name: "Movies folder is unavailable",
      }),
    ).toBeTruthy();

    cleanup();
    invokeMock.mockRejectedValueOnce("movies_scan_failed");
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
    invokeMock.mockImplementation(
      (_command: string, argumentsValue?: Record<string, unknown>) => {
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
