import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  parseTmdbCardCoverResponse,
  tmdbCoverMime,
  type TmdbCardCoverRequest,
  useTmdbCardCover,
} from "@/tmdb-cover";
import { scheduleLibraryPresentation } from "@/library-presentation";

const movieDiscover: TmdbCardCoverRequest = {
  category: "movie",
  contextGeneration: "7",
  posterPath: "/movie.jpg",
  surface: "discover",
  tmdbId: 101,
};

const tvLibrary: TmdbCardCoverRequest = {
  associationGeneration: "9",
  category: "tv",
  contextGeneration: "9",
  libraryItemId: "a".repeat(40),
  posterPath: "/show.webp",
  surface: "library",
  tmdbId: 202,
};

function response(request: TmdbCardCoverRequest) {
  return [
    "tmdb-card-cover-v1",
    "pending",
    request.category,
    request.surface,
    String(request.tmdbId),
    request.posterPath ?? "",
    request.contextGeneration,
    "12",
    request.libraryItemId ?? "",
    request.associationGeneration ?? "0",
    `tmdb-cover-${"b".repeat(40)}`,
    String(2 / 3),
    "TMDB",
  ];
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((next, fail) => {
    resolve = next;
    reject = fail;
  });
  return { promise, reject, resolve };
}

function coverBytes() {
  return [0xff, 0xd8, 0xff, ...Array.from({ length: 61 }, () => 0)];
}

describe("TMDB card-cover response authority", () => {
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });
  it.each([movieDiscover, { ...movieDiscover, category: "tv" as const }, {
    ...tvLibrary,
    category: "movie" as const,
  }, tvLibrary])("accepts an exact current %s response", (request) => {
    expect(parseTmdbCardCoverResponse(response(request), request, "12")).toEqual({
      authorityId: `tmdb-cover-${"b".repeat(40)}`,
      status: "pending",
    });
  });

  it("rejects crossed category, item, generation, poster, and Library authority", () => {
    for (const index of [2, 4, 5, 6, 7, 8, 9]) {
      const crossed = response(tvLibrary);
      crossed[index] = `${crossed[index]}-crossed`;
      expect(parseTmdbCardCoverResponse(crossed, tvLibrary, "12")).toBeNull();
    }
  });

  it("accepts only an exact missing response for an absent poster", () => {
    const request = { ...movieDiscover, posterPath: null };
    const missing = response(request);
    missing[1] = "missing";
    missing[10] = "";
    missing[11] = String(2 / 3);
    missing[12] = "";
    expect(parseTmdbCardCoverResponse(missing, request, "12")).toEqual({
      authorityId: null,
      status: "missing",
    });
    missing[5] = "/crossed.jpg";
    expect(parseTmdbCardCoverResponse(missing, request, "12")).toBeNull();
  });

  it("accepts supported raster magic and rejects provider text", () => {
    expect(tmdbCoverMime(Uint8Array.from([0xff, 0xd8, 0xff]))).toBe("image/jpeg");
    expect(tmdbCoverMime(Uint8Array.from([0x89, 0x50, 0x4e, 0x47]))).toBe("image/png");
    expect(tmdbCoverMime(new TextEncoder().encode("RIFF0000WEBP"))).toBe("image/webp");
    expect(tmdbCoverMime(new TextEncoder().encode("<html>"))).toBeNull();
  });

  it("discards a fifth queued cover when its visible card unmounts", async () => {
    const blockers = Array.from({ length: 4 }, () => deferred<void>());
    const occupied = blockers.map((blocker) =>
      scheduleLibraryPresentation("cover", () => blocker.promise),
    );
    const invoke = vi.fn(() => Promise.reject(new Error("must not dispatch")));
    const createObjectUrl = vi.spyOn(URL, "createObjectURL");
    vi.stubGlobal("__TAURI__", { core: { invoke } });

    const { unmount } = renderHook(() => useTmdbCardCover(movieDiscover));
    unmount();
    blockers.forEach((blocker) => blocker.resolve());
    await Promise.all(occupied);
    await Promise.resolve();

    expect(invoke).not.toHaveBeenCalledWith(
      "resolve_tmdb_card_cover",
      expect.anything(),
    );
    expect(invoke).not.toHaveBeenCalledWith(
      "fetch_tmdb_card_cover",
      expect.anything(),
    );
    expect(invoke).not.toHaveBeenCalledWith(
      "confirm_tmdb_card_cover",
      expect.anything(),
    );
    expect(createObjectUrl).not.toHaveBeenCalled();
  });

  it("keeps fetched bytes pending until decode confirms the reusable cover", async () => {
    const invoke = vi.fn((command: string, parameters?: Record<string, unknown>) => {
      if (command === "resolve_tmdb_card_cover") {
        return Promise.resolve(
          response(movieDiscover).map((field, index) =>
            index === 7 ? String(parameters?.requestGeneration) : field,
          ),
        );
      }
      if (command === "fetch_tmdb_card_cover") return Promise.resolve(coverBytes());
      if (command === "confirm_tmdb_card_cover") return Promise.resolve();
      if (command === "cancel_tmdb_card_cover") return Promise.resolve();
      return Promise.reject(new Error(`Unexpected command ${command}`));
    });
    vi.stubGlobal("__TAURI__", { core: { invoke } });
    vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:tmdb-cover");
    vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => undefined);

    const { result } = renderHook(() => useTmdbCardCover(movieDiscover));
    await waitFor(() => expect(result.current.objectUrl).toBe("blob:tmdb-cover"));
    expect(result.current.status).toBe("loading");
    expect(result.current.source).toBeNull();
    expect(
      invoke.mock.calls.filter(([command]) => command === "confirm_tmdb_card_cover"),
    ).toHaveLength(0);

    await act(async () => result.current.reportDecodeSuccess?.());
    await waitFor(() => expect(result.current.status).toBe("ready"));
    expect(result.current.source).toBe("TMDB");
    expect(
      invoke.mock.calls.filter(([command]) => command === "confirm_tmdb_card_cover"),
    ).toHaveLength(1);
  });

  it("waits for complete decode invalidation before enabling one fresh Retry", async () => {
    const invalidation = deferred<void>();
    let resolveCount = 0;
    const invoke = vi.fn((command: string, parameters?: Record<string, unknown>) => {
      if (command === "resolve_tmdb_card_cover") {
        resolveCount += 1;
        return Promise.resolve(
          response(movieDiscover).map((field, index) =>
            index === 7 ? String(parameters?.requestGeneration) : field,
          ),
        );
      }
      if (command === "fetch_tmdb_card_cover") return Promise.resolve(coverBytes());
      if (command === "invalidate_tmdb_card_cover") return invalidation.promise;
      if (command === "cancel_tmdb_card_cover") return Promise.resolve();
      return Promise.reject(new Error(`Unexpected command ${command}`));
    });
    vi.stubGlobal("__TAURI__", { core: { invoke } });
    vi.spyOn(URL, "createObjectURL")
      .mockReturnValueOnce("blob:undecodable")
      .mockReturnValueOnce("blob:replacement");
    vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => undefined);

    const { result } = renderHook(() => useTmdbCardCover(movieDiscover));
    await waitFor(() => expect(result.current.objectUrl).toBe("blob:undecodable"));
    act(() => result.current.reportDecodeFailure?.());
    expect(result.current.retry).toBeNull();
    expect(resolveCount).toBe(1);

    invalidation.resolve();
    await waitFor(() => expect(result.current.retry).not.toBeNull());
    act(() => result.current.retry?.());
    await waitFor(() => expect(result.current.objectUrl).toBe("blob:replacement"));
    expect(resolveCount).toBe(2);
  });

  it("reports failed decode cleanup without exposing an unsafe Retry", async () => {
    const invoke = vi.fn((command: string, parameters?: Record<string, unknown>) => {
      if (command === "resolve_tmdb_card_cover") {
        return Promise.resolve(
          response(movieDiscover).map((field, index) =>
            index === 7 ? String(parameters?.requestGeneration) : field,
          ),
        );
      }
      if (command === "fetch_tmdb_card_cover") return Promise.resolve(coverBytes());
      if (command === "invalidate_tmdb_card_cover") {
        return Promise.reject(new Error("cache deletion failed"));
      }
      if (command === "cancel_tmdb_card_cover") return Promise.resolve();
      return Promise.reject(new Error(`Unexpected command ${command}`));
    });
    vi.stubGlobal("__TAURI__", { core: { invoke } });
    vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:undecodable");
    vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => undefined);

    const { result } = renderHook(() => useTmdbCardCover(movieDiscover));
    await waitFor(() => expect(result.current.objectUrl).toBe("blob:undecodable"));
    act(() => result.current.reportDecodeFailure?.());
    await waitFor(() => expect(result.current.status).toBe("unavailable"));
    expect(result.current.retry).toBeNull();
    expect(
      invoke.mock.calls.filter(([command]) => command === "resolve_tmdb_card_cover"),
    ).toHaveLength(1);
  });
});
