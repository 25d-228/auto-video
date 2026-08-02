import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import App from "./App";

const systemDarkModeQuery = "(prefers-color-scheme: dark)";

let systemPrefersDark = false;
let mediaQueryListeners = new Set<(event: MediaQueryListEvent) => void>();

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
  window.localStorage.clear();
  vi.stubGlobal("matchMedia", vi.fn(createMediaQueryList));
});

afterEach(() => {
  cleanup();
  window.localStorage.clear();
  delete document.documentElement.dataset.appearance;
  delete document.documentElement.dataset.theme;
  vi.unstubAllGlobals();
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

    fireEvent.click(screen.getByRole("button", { name: "Library" }));
    expect(
      screen.getByRole("heading", {
        level: 2,
        name: "No library folder is configured",
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
