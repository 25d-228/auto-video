import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  parseLibraryCover,
  parseLibraryMetadata,
  scheduleLibraryPresentation,
  useLibraryPresentation,
} from "./library-presentation";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

describe("Library presentation boundary", () => {
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it("accepts exact cover and metadata responses and rejects mismatched authority", () => {
    expect(
      parseLibraryCover(
        [
          "library-cover-v1",
          "vr",
          "ready",
          "JavDB",
          "item",
          `library-cover-${"a".repeat(40)}`,
          "1.7777777777777777",
        ],
        "vr",
      ),
    ).toMatchObject({ state: "ready", source: "JavDB" });
    expect(
      parseLibraryCover(
        [
          "library-cover-v1",
          "adult",
          "ready",
          "JavDB",
          "item",
          `library-cover-${"a".repeat(40)}`,
          "1.77",
        ],
        "vr",
      ),
    ).toBeNull();
    expect(
      parseLibraryMetadata(
        [
          "library-metadata-v1",
          "adult",
          "automatic",
          "JavDB",
          "item",
          "Exact title",
          "2024-01-02",
          "90 min",
          "1",
          "Actor",
        ],
        "adult",
      ),
    ).toMatchObject({ state: "automatic", cast: ["Actor"] });
  });

  it("prioritizes visible cover work and discards obsolete queued work before a slot", async () => {
    const blockers = Array.from({ length: 4 }, () => deferred<void>());
    const running = blockers.map((blocker) =>
      scheduleLibraryPresentation("metadata", () => blocker.promise),
    );
    let obsoleteCurrent = true;
    const obsolete = vi.fn(async () => undefined);
    const obsoleteResult = scheduleLibraryPresentation(
      "metadata",
      obsolete,
      () => obsoleteCurrent,
    ).catch((error: unknown) => error);
    const cover = vi.fn(async () => "cover");
    const coverResult = scheduleLibraryPresentation("cover", cover);

    obsoleteCurrent = false;
    blockers[0].resolve();
    await expect(coverResult).resolves.toBe("cover");
    expect(cover).toHaveBeenCalledTimes(1);
    expect(obsolete).not.toHaveBeenCalled();

    blockers.slice(1).forEach((blocker) => blocker.resolve());
    await Promise.all(running);
    await expect(obsoleteResult).resolves.toBeInstanceOf(Error);
  });

  it("shows a validated cover before optional metadata finishes", async () => {
    const metadata = deferred<unknown>();
    const invoke = vi.fn((command: string) => {
      if (command === "resolve_library_cover") {
        return Promise.resolve([
          "library-cover-v1",
          "vr",
          "ready",
          "JavDB",
          "item",
          `library-cover-${"a".repeat(40)}`,
          "1.7777777777777777",
        ]);
      }
      if (command === "fetch_library_cover") {
        return Promise.resolve([0xff, 0xd8]);
      }
      if (command === "resolve_library_metadata") return metadata.promise;
      return Promise.reject(new Error(`Unexpected command ${command}`));
    });
    const createObjectURL = vi.fn(() => "blob:wide-cover");
    vi.stubGlobal("__TAURI__", { core: { invoke } });
    Object.defineProperty(URL, "createObjectURL", {
      configurable: true,
      value: createObjectURL,
    });
    Object.defineProperty(URL, "revokeObjectURL", {
      configurable: true,
      value: vi.fn(),
    });

    const { result } = renderHook(() =>
      useLibraryPresentation({
        category: "vr",
        itemId: "MDVR-419",
        scanGeneration: "7",
      }),
    );
    await waitFor(() => expect(result.current.cover.status).toBe("ready"));
    expect(result.current.cover.objectUrl).toBe("blob:wide-cover");
    expect(result.current.cover.aspectRatio).toBe(16 / 9);
    expect(result.current.metadata.status).toBe("loading");
    expect(invoke.mock.calls.map(([command]) => command)).toEqual([
      "resolve_library_cover",
      "fetch_library_cover",
      "resolve_library_metadata",
    ]);

    await act(async () => {
      metadata.resolve([
        "library-metadata-v1",
        "vr",
        "automatic",
        "JavDB",
        "item",
        "Provider title",
        "2024-01-02",
        "90 min",
        "0",
      ]);
      await metadata.promise;
    });
    expect(result.current.cover.objectUrl).toBe("blob:wide-cover");
    expect(result.current.metadata.status).toBe("automatic");
  });

  it("revokes a decode failure, retains geometry, and retries the exact cover", async () => {
    let generation = 0;
    const invoke = vi.fn((command: string) => {
      if (command === "resolve_library_cover") {
        generation += 1;
        return Promise.resolve([
          "library-cover-v1",
          "adult",
          "ready",
          "JavDB",
          "item",
          `library-cover-${String(generation).repeat(40)}`,
          "0.5",
        ]);
      }
      if (command === "fetch_library_cover") return Promise.resolve([0xff, 0xd8]);
      if (command === "resolve_library_metadata") {
        return Promise.resolve([
          "library-metadata-v1",
          "adult",
          "local-only",
          "",
          "",
          "",
          "",
          "",
          "0",
        ]);
      }
      return Promise.reject(new Error(`Unexpected command ${command}`));
    });
    const createObjectURL = vi
      .fn()
      .mockReturnValueOnce("blob:first")
      .mockReturnValueOnce("blob:replacement");
    const revokeObjectURL = vi.fn();
    vi.stubGlobal("__TAURI__", { core: { invoke } });
    Object.defineProperty(URL, "createObjectURL", {
      configurable: true,
      value: createObjectURL,
    });
    Object.defineProperty(URL, "revokeObjectURL", {
      configurable: true,
      value: revokeObjectURL,
    });

    const { result } = renderHook(() =>
      useLibraryPresentation({
        category: "adult",
        itemId: "ADLT-123",
        scanGeneration: "9",
      }),
    );
    await waitFor(() => expect(result.current.cover.objectUrl).toBe("blob:first"));
    act(() => result.current.cover.reportDecodeFailure?.());
    expect(revokeObjectURL).toHaveBeenCalledWith("blob:first");
    expect(result.current.cover.status).toBe("unavailable");
    expect(result.current.cover.aspectRatio).toBe(0.5);

    act(() => result.current.cover.retry?.());
    await waitFor(() =>
      expect(result.current.cover.objectUrl).toBe("blob:replacement"),
    );
    expect(result.current.cover.aspectRatio).toBe(0.5);
    expect(invoke.mock.calls.filter(([command]) => command === "resolve_library_cover"))
      .toHaveLength(2);
  });

  it("retries metadata without restarting accepted cover resolution", async () => {
    const invoke = vi.fn((command: string) => {
      if (command === "resolve_library_cover") {
        return Promise.resolve([
          "library-cover-v1",
          "adult",
          "missing",
          "",
          "",
          "",
          "0.72",
        ]);
      }
      if (command === "resolve_library_metadata") {
        const metadataAttempt = invoke.mock.calls.filter(
          ([calledCommand]) => calledCommand === "resolve_library_metadata",
        ).length;
        if (metadataAttempt === 1) return Promise.reject(new Error("offline"));
        return Promise.resolve([
          "library-metadata-v1",
          "adult",
          "local-only",
          "",
          "",
          "",
          "",
          "",
          "0",
        ]);
      }
      return Promise.reject(new Error(`Unexpected command ${command}`));
    });
    vi.stubGlobal("__TAURI__", { core: { invoke } });

    const { result } = renderHook(() =>
      useLibraryPresentation({
        category: "adult",
        itemId: "ADLT-123",
        scanGeneration: "11",
      }),
    );
    await waitFor(() => expect(result.current.metadata.status).toBe("unavailable"));
    act(() => result.current.metadata.retry?.());
    await waitFor(() => expect(result.current.metadata.status).toBe("local-only"));

    expect(
      invoke.mock.calls.filter(([command]) => command === "resolve_library_cover"),
    ).toHaveLength(1);
    expect(
      invoke.mock.calls.filter(([command]) => command === "resolve_library_metadata"),
    ).toHaveLength(2);
  });

  it("rejects a late cover result after the exact item authority changes", async () => {
    const oldCover = deferred<unknown>();
    const invoke = vi.fn(
      (command: string, parameters?: Record<string, unknown>) => {
        if (command === "resolve_library_cover") {
          if (parameters?.itemId === "MDVR-419") return oldCover.promise;
          return Promise.resolve([
            "library-cover-v1",
            "vr",
            "missing",
            "",
            "",
            "",
            "0.72",
          ]);
        }
        if (command === "resolve_library_metadata") {
          return Promise.resolve([
            "library-metadata-v1",
            "vr",
            "local-only",
            "",
            "",
            "",
            "",
            "",
            "0",
          ]);
        }
        if (command === "fetch_library_cover") {
          return Promise.resolve([0xff, 0xd8]);
        }
        return Promise.reject(new Error(`Unexpected command ${command}`));
      },
    );
    vi.stubGlobal("__TAURI__", { core: { invoke } });

    const { rerender, result } = renderHook(
      ({ itemId }) =>
        useLibraryPresentation({
          category: "vr",
          itemId,
          scanGeneration: "15",
        }),
      { initialProps: { itemId: "MDVR-419" } },
    );
    rerender({ itemId: "MDVR-420" });
    await waitFor(() => expect(result.current.metadata.status).toBe("local-only"));
    await act(async () => {
      oldCover.resolve([
        "library-cover-v1",
        "vr",
        "ready",
        "JavDB",
        "old-item",
        `library-cover-${"a".repeat(40)}`,
        "1.77",
      ]);
      await oldCover.promise;
    });

    expect(result.current.cover.status).toBe("missing");
    expect(
      invoke.mock.calls.filter(([command]) => command === "fetch_library_cover"),
    ).toHaveLength(0);
    expect(
      invoke.mock.calls.filter(
        ([command, parameters]) =>
          command === "resolve_library_metadata" &&
          parameters?.itemId === "MDVR-419",
      ),
    ).toHaveLength(0);
  });
});
