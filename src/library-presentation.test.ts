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

function presentationRequest(category: "adult" | "vr", itemId: string) {
  return { category, itemId, scanGeneration: "parser-test" };
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
          "library-cover-v3",
          "vr",
          "ready",
          "JavDB",
          "item",
          "MDVR-419",
          `library-cover-${"a".repeat(40)}`,
          "1.7777777777777777",
          "JavDB",
          "item",
          "MDVR-419",
        ],
        presentationRequest("vr", "MDVR-419"),
      ),
    ).toMatchObject({
      state: "ready",
      source: "JavDB",
      verifiedIdentity: {
        provider: "JavDB",
        providerId: "item",
        displayCode: "MDVR-419",
      },
    });
    expect(
      parseLibraryCover(
        [
          "library-cover-v3",
          "adult",
          "ready",
          "JavDB",
          "item",
          "ADLT-123",
          `library-cover-${"a".repeat(40)}`,
          "1.77",
          "JavDB",
          "item",
          "ADLT-123",
        ],
        presentationRequest("vr", "MDVR-419"),
      ),
    ).toBeNull();
    expect(
      parseLibraryCover(
        [
          "library-cover-v3",
          "adult",
          "missing",
          "",
          "",
          "",
          "",
          "0.72",
          "FANZA",
          "cawb00001",
          "CAWB-001",
        ],
        presentationRequest("adult", "CAWB-1"),
      ),
    ).toMatchObject({
      state: "missing",
      source: null,
      verifiedIdentity: {
        provider: "FANZA",
        providerId: "cawb00001",
        displayCode: "CAWB-001",
      },
    });
    expect(
      parseLibraryCover(
        [
          "library-cover-v3",
          "adult",
          "missing",
          "",
          "",
          "",
          "",
          "0.72",
          "FANZA",
          "",
          "CAWB-001",
        ],
        presentationRequest("adult", "CAWB-1"),
      ),
    ).toBeNull();
    expect(
      parseLibraryMetadata(
        [
          "library-metadata-v4",
          "adult",
          "automatic",
          "current",
          "JavDB",
          "item",
          "ADLT-123",
          "JavDB",
          "item",
          "ADLT-123",
          "Exact title",
          "2024-01-02",
          "90 min",
          "1",
          "Actor",
        ],
        presentationRequest("adult", "ADLT-123"),
      ),
    ).toMatchObject({
      state: "automatic",
      cast: ["Actor"],
      verifiedIdentity: {
        provider: "JavDB",
        providerId: "item",
        displayCode: "ADLT-123",
      },
    });
  });

  it("rejects exact-provider cover responses without the matching verified identity", () => {
    const authority = `library-cover-${"b".repeat(40)}`;
    const exactLegacyAuthority = `${authority}-cawb00001`;
    expect(
      parseLibraryCover(
        [
          "library-cover-v3",
          "adult",
          "ready",
          "JavDB",
          "coveritem",
          "CAWB-001",
          authority,
          "0.72",
          "",
          "",
          "",
        ],
        presentationRequest("adult", "CAWB-1"),
      ),
    ).toBeNull();
    expect(
      parseLibraryCover(
        [
          "library-cover-v3",
          "adult",
          "ready",
          "FANZA",
          "cawb00001",
          "CAWB-001",
          authority,
          "0.72",
          "",
          "",
          "",
        ],
        presentationRequest("adult", "CAWB-1"),
      ),
    ).toBeNull();
    expect(
      parseLibraryCover(
        [
          "library-cover-v3",
          "adult",
          "ready",
          "JavDB",
          "coveritem",
          "CAWB-001",
          authority,
          "0.72",
          "JavDB",
          "anotheritem",
          "CAWB-001",
        ],
        presentationRequest("adult", "CAWB-1"),
      ),
    ).toBeNull();
    expect(
      parseLibraryCover(
        [
          "library-cover-v3",
          "adult",
          "ready",
          "FANZA",
          "cawb00001",
          "CAWB-001",
          authority,
          "0.72",
          "",
          "",
          "",
        ],
        presentationRequest("adult", "CAWB-1"),
      ),
    ).toBeNull();
    expect(
      parseLibraryCover(
        [
          "library-cover-v3",
          "adult",
          "ready",
          "FANZA",
          "cawb00001",
          "CAWB-001",
          authority,
          "0.72",
          "JavDB",
          "coveritem",
          "CAWB-001",
        ],
        presentationRequest("adult", "CAWB-1"),
      ),
    ).toMatchObject({
      source: "FANZA",
      verifiedIdentity: {
        provider: "JavDB",
        providerId: "coveritem",
        displayCode: "CAWB-001",
      },
    });
    expect(
      parseLibraryCover(
        [
          "library-cover-v3",
          "adult",
          "ready",
          "r18.dev",
          "cawb00001",
          "CAWB-001",
          exactLegacyAuthority,
          "0.72",
          "r18.dev",
          "cawb00001",
          "CAWB-001",
        ],
        presentationRequest("adult", "CAWB-1"),
      ),
    ).toMatchObject({
      source: "r18.dev",
      verifiedIdentity: {
        provider: "r18.dev",
        providerId: "cawb00001",
        displayCode: "CAWB-001",
      },
    });
    expect(
      parseLibraryCover(
        [
          "library-cover-v3",
          "adult",
          "ready",
          "r18.dev",
          "cawb00001",
          "CAWB-001",
          `${authority}-cawb00002`,
          "0.72",
          "",
          "",
          "",
        ],
        presentationRequest("adult", "CAWB-1"),
      ),
    ).toBeNull();
    expect(
      parseLibraryCover(
        [
          "library-cover-v3",
          "adult",
          "ready",
          "r18.dev",
          "cawb00002",
          "CAWB-1",
          exactLegacyAuthority,
          "0.72",
          "",
          "",
          "",
        ],
        presentationRequest("adult", "CAWB-1"),
      ),
    ).toBeNull();
    expect(
      parseLibraryMetadata(
        [
          "library-metadata-v5",
          "adult",
          "automatic",
          "current",
          "r18.dev",
          "cawb00001",
          "CAWB-001",
          "r18.dev",
          "cawb00002",
          "CAWB-001",
          "Crossed title",
          "",
          "",
          "0",
          "cawb00002",
          "CAWB-2",
        ],
        presentationRequest("adult", "CAWB-1"),
      ),
    ).toBeNull();
    expect(
      parseLibraryMetadata(
        [
          "library-metadata-v5",
          "adult",
          "automatic",
          "current",
          "r18.dev",
          "cawb00001",
          "CAWB-001",
          "r18.dev",
          "cawb00001",
          "CAWB-001",
          "Exact legacy title",
          "",
          "",
          "0",
          "cawb00001",
          "CAWB-001",
        ],
        presentationRequest("adult", "CAWB-1"),
      ),
    ).toMatchObject({
      source: "r18.dev",
      providerId: "cawb00001",
      legacyProviderId: "cawb00001",
    });
    expect(
      parseLibraryMetadata(
        [
          "library-metadata-v5",
          "adult",
          "automatic",
          "current",
          "JavDB",
          "item",
          "CAWB-001",
          "JavDB + r18.dev",
          "item",
          "CAWB-001",
          "Combined title",
          "",
          "",
          "0",
          "cawb00002",
          "CAWB-2",
        ],
        presentationRequest("adult", "CAWB-1"),
      ),
    ).toBeNull();
  });

  it("binds legacy cover authority to the exact raw content ID without FANZA mapping", () => {
    for (const [itemId, contentId] of [
      ["3DSVR-01871", "13dsvr01871"],
      ["MDVR-419", "mdvr00419"],
    ] as const) {
      const authority = `library-cover-${"d".repeat(40)}-${contentId}`;
      expect(
        parseLibraryCover(
          [
            "library-cover-v3",
            "vr",
            "ready",
            "r18.dev",
            contentId,
            itemId,
            authority,
            "0.75",
            "r18.dev",
            contentId,
            itemId,
          ],
          presentationRequest("vr", itemId),
        ),
      ).toMatchObject({ source: "r18.dev", providerId: contentId });
      expect(
        parseLibraryCover(
          [
            "library-cover-v3",
            "vr",
            "ready",
            "r18.dev",
            contentId,
            itemId,
            `${authority}x`,
            "0.75",
            "",
            "",
            "",
          ],
          presentationRequest("vr", itemId),
        ),
      ).toBeNull();
    }
  });

  it("rejects responses for another local product or FANZA category authority", () => {
    const authority = `library-cover-${"f".repeat(40)}`;
    expect(
      parseLibraryCover(
        [
          "library-cover-v3",
          "adult",
          "ready",
          "JavDB",
          "item",
          "ADLT-124",
          authority,
          "0.72",
          "JavDB",
          "item",
          "ADLT-124",
        ],
        presentationRequest("adult", "ADLT-123"),
      ),
    ).toBeNull();
    expect(
      parseLibraryMetadata(
        [
          "library-metadata-v4",
          "adult",
          "automatic",
          "current",
          "JavDB",
          "item",
          "ADLT-124",
          "JavDB",
          "item",
          "ADLT-124",
          "Wrong item",
          "",
          "",
          "0",
        ],
        presentationRequest("adult", "ADLT-123"),
      ),
    ).toBeNull();
    expect(
      parseLibraryCover(
        [
          "library-cover-v3",
          "adult",
          "ready",
          "FANZA",
          "13dsvr01871",
          "3DSVR-01871",
          authority,
          "0.72",
          "FANZA",
          "13dsvr01871",
          "3DSVR-01871",
        ],
        presentationRequest("adult", "CAWB-1"),
      ),
    ).toBeNull();
    expect(
      parseLibraryCover(
        [
          "library-cover-v3",
          "vr",
          "ready",
          "FANZA",
          "cawb00001",
          "CAWB-001",
          authority,
          "0.72",
          "FANZA",
          "cawb00001",
          "CAWB-001",
        ],
        presentationRequest("vr", "3DSVR-01871"),
      ),
    ).toBeNull();
  });

  it("rejects crossed legacy image authority before native byte fetch", async () => {
    const invoke = vi.fn((command: string) => {
      if (command === "resolve_library_cover") {
        return Promise.resolve([
          "library-cover-v3",
          "adult",
          "ready",
          "r18.dev",
          "CAWB-1",
          "CAWB-1",
          `library-cover-${"e".repeat(40)}-cawb00002`,
          "0.72",
          "",
          "",
          "",
        ]);
      }
      if (command === "resolve_library_metadata") {
        return Promise.resolve([
          "library-metadata-v4",
          "adult",
          "unavailable",
          "conflict",
          "",
          "",
          "",
          "",
          "",
          "",
          "",
          "",
          "",
          "0",
        ]);
      }
      if (command === "cancel_library_cover_request") return Promise.resolve();
      return Promise.reject(new Error(`Unexpected command ${command}`));
    });
    vi.stubGlobal("__TAURI__", { core: { invoke } });

    const { result } = renderHook(() =>
      useLibraryPresentation({
        category: "adult",
        itemId: "CAWB-1",
        scanGeneration: "crossed-legacy",
      }),
    );

    await waitFor(() => expect(result.current.cover.status).toBe("unavailable"));
    expect(result.current.verifiedIdentity).toBeNull();
    expect(
      invoke.mock.calls.filter(
        ([command]) => command === "fetch_library_cover",
      ),
    ).toHaveLength(0);
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

  it("fetches all fourteen current cover authorities before the native bound can evict one", async () => {
    const pendingResolutions = new Map<
      string,
      ReturnType<typeof deferred<unknown>>
    >();
    const retainedAuthorityOrder: string[] = [];
    const retainedAuthorities = new Set<string>();
    let maximumRetainedAuthorities = 0;
    let staleFetches = 0;
    const invoke = vi.fn(
      (command: string, parameters?: Record<string, unknown>) => {
        if (command === "resolve_library_cover") {
          const itemId = String(parameters?.itemId);
          const resolution = deferred<unknown>();
          pendingResolutions.set(itemId, resolution);
          return resolution.promise;
        }
        if (command === "fetch_library_cover") {
          const authorityId = String(parameters?.coverAuthorityId);
          if (!retainedAuthorities.delete(authorityId)) {
            staleFetches += 1;
            return Promise.reject(new Error("cover authority was evicted"));
          }
          retainedAuthorityOrder.splice(
            retainedAuthorityOrder.indexOf(authorityId),
            1,
          );
          return Promise.resolve([0xff, 0xd8]);
        }
        if (command === "resolve_library_metadata") {
          return Promise.resolve([
            "library-metadata-v4",
            "adult",
            "local-only",
            "current",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "0",
          ]);
        }
        if (command === "cancel_library_cover_request") return Promise.resolve();
        return Promise.reject(new Error(`Unexpected command ${command}`));
      },
    );
    vi.stubGlobal("__TAURI__", { core: { invoke } });
    Object.defineProperty(URL, "createObjectURL", {
      configurable: true,
      value: vi.fn(() => `blob:cover-${retainedAuthorities.size}`),
    });
    Object.defineProperty(URL, "revokeObjectURL", {
      configurable: true,
      value: vi.fn(),
    });

    const hooks = Array.from({ length: 14 }, (_, index) =>
      renderHook(() =>
        useLibraryPresentation({
          category: "adult",
          itemId: `ADLT-${index + 1}`,
          scanGeneration: "14-card-page",
        }),
      ),
    );

    let resolvedCount = 0;
    while (resolvedCount < hooks.length) {
      const batchSize = Math.min(4, hooks.length - resolvedCount);
      await waitFor(() => expect(pendingResolutions.size).toBe(batchSize));
      const batch = Array.from(pendingResolutions.entries());
      pendingResolutions.clear();
      for (const [itemId, resolution] of batch) {
        const itemNumber = Number(itemId.slice("ADLT-".length));
        const authorityId = `library-cover-${itemNumber.toString(16).padStart(40, "0")}`;
        retainedAuthorityOrder.push(authorityId);
        retainedAuthorities.add(authorityId);
        if (retainedAuthorityOrder.length > 8) {
          const evictedAuthority = retainedAuthorityOrder.shift();
          if (evictedAuthority !== undefined) {
            retainedAuthorities.delete(evictedAuthority);
          }
        }
        maximumRetainedAuthorities = Math.max(
          maximumRetainedAuthorities,
          retainedAuthorities.size,
        );
        resolution.resolve([
          "library-cover-v3",
          "adult",
          "ready",
          "JavDB",
          `item${itemNumber}`,
          itemId,
          authorityId,
          "0.72",
          "JavDB",
          `item${itemNumber}`,
          itemId,
        ]);
      }
      resolvedCount += batchSize;
      await waitFor(() =>
        expect(
          invoke.mock.calls.filter(
            ([command]) => command === "fetch_library_cover",
          ),
        ).toHaveLength(resolvedCount),
      );
    }

    await waitFor(() =>
      expect(hooks.map(({ result }) => result.current.cover.status)).toEqual(
        Array.from({ length: 14 }, () => "ready"),
      ),
    );
    expect(maximumRetainedAuthorities).toBeLessThanOrEqual(4);
    expect(staleFetches).toBe(0);
    expect(retainedAuthorities.size).toBe(0);
    expect(
      invoke.mock.calls.filter(
        ([command]) => command === "resolve_library_cover",
      ),
    ).toHaveLength(14);
    expect(
      invoke.mock.calls.filter(
        ([command]) => command === "fetch_library_cover",
      ),
    ).toHaveLength(14);
  });

  it("shows a validated cover before optional metadata finishes", async () => {
    const metadata = deferred<unknown>();
    const invoke = vi.fn(
      (command: string, _parameters?: Record<string, unknown>) => {
        if (command === "resolve_library_cover") {
          return Promise.resolve([
            "library-cover-v3",
            "vr",
            "ready",
            "JavDB",
            "item",
            "MDVR-419",
            `library-cover-${"a".repeat(40)}`,
            "1.7777777777777777",
            "JavDB",
            "item",
            "MDVR-419",
          ]);
        }
        if (command === "fetch_library_cover") {
          return Promise.resolve([0xff, 0xd8]);
        }
        if (command === "resolve_library_metadata") return metadata.promise;
        return Promise.reject(new Error(`Unexpected command ${command}`));
      },
    );
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
    const coverArguments = invoke.mock.calls.find(
      ([command]) => command === "resolve_library_cover",
    )?.[1];
    const metadataArguments = invoke.mock.calls.find(
      ([command]) => command === "resolve_library_metadata",
    )?.[1];
    expect(metadataArguments).toMatchObject({
      category: "vr",
      itemId: "MDVR-419",
      scanGeneration: "7",
      coverRequestGeneration: coverArguments?.coverRequestGeneration,
    });

    await act(async () => {
      metadata.resolve([
        "library-metadata-v4",
        "vr",
        "automatic",
        "current",
        "JavDB",
        "item",
        "MDVR-419",
        "JavDB",
        "item",
        "MDVR-419",
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
          "library-cover-v3",
          "adult",
          "ready",
          "JavDB",
          "item",
          "ADLT-123",
          `library-cover-${String(generation).repeat(40)}`,
          "0.5",
          "JavDB",
          "item",
          "ADLT-123",
        ]);
      }
      if (command === "fetch_library_cover") return Promise.resolve([0xff, 0xd8]);
      if (command === "invalidate_library_cover") return Promise.resolve();
      if (command === "resolve_library_metadata") {
        return Promise.resolve([
          "library-metadata-v4",
          "adult",
          "local-only",
          "current",
          "",
          "",
          "",
          "",
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
    expect(result.current.verifiedIdentity?.displayCode).toBe("ADLT-123");
    act(() => result.current.cover.reportDecodeFailure?.());
    expect(revokeObjectURL).toHaveBeenCalledWith("blob:first");
    expect(result.current.cover.status).toBe("unavailable");
    expect(result.current.cover.aspectRatio).toBe(0.5);
    expect(result.current.verifiedIdentity?.displayCode).toBe("ADLT-123");

    act(() => result.current.cover.retry?.());
    await waitFor(() =>
      expect(result.current.cover.objectUrl).toBe("blob:replacement"),
    );
    expect(result.current.cover.aspectRatio).toBe(0.5);
    expect(result.current.verifiedIdentity?.displayCode).toBe("ADLT-123");
    expect(invoke.mock.calls.filter(([command]) => command === "resolve_library_cover"))
      .toHaveLength(2);
    expect(
      invoke.mock.calls.filter(
        ([command]) => command === "invalidate_library_cover",
      ),
    ).toHaveLength(1);
  });

  it("cancels an obsolete native authority before byte fetch after unmount", async () => {
    const resolution = deferred<unknown>();
    const invoke = vi.fn((command: string) => {
      if (command === "resolve_library_cover") return resolution.promise;
      if (command === "cancel_library_cover_request") return Promise.resolve();
      return Promise.reject(new Error(`Unexpected command ${command}`));
    });
    vi.stubGlobal("__TAURI__", { core: { invoke } });

    const { unmount } = renderHook(() =>
      useLibraryPresentation({
        category: "vr",
        itemId: "3DSVR-1871",
        scanGeneration: "12",
      }),
    );
    unmount();
    resolution.resolve([
      "library-cover-v3",
      "vr",
      "ready",
      "JavDB",
      "item",
      "MDVR-419",
      `library-cover-${"c".repeat(40)}`,
      "0.5",
      "JavDB",
      "item",
      "MDVR-419",
    ]);

    await waitFor(() =>
      expect(
        invoke.mock.calls.filter(
          ([command]) => command === "cancel_library_cover_request",
        ),
      ).toHaveLength(1),
    );
    expect(
      invoke.mock.calls.filter(([command]) => command === "fetch_library_cover"),
    ).toHaveLength(0);
  });

  it("invalidates a failed cached source and retries provider resolution without losing geometry", async () => {
    let resolution = 0;
    const invoke = vi.fn((command: string) => {
      if (command === "resolve_library_cover") {
        resolution += 1;
        return Promise.resolve([
          "library-cover-v3",
          "adult",
          "ready",
          resolution === 1 ? "JavDB" : "FANZA",
          resolution === 1 ? "old" : "cawb00001",
          "CAWB-001",
          `library-cover-${(resolution === 1 ? "d" : "e").repeat(40)}`,
          "0.5",
          resolution === 1 ? "JavDB" : "FANZA",
          resolution === 1 ? "old" : "cawb00001",
          "CAWB-001",
        ]);
      }
      if (command === "fetch_library_cover") {
        return resolution === 1
          ? Promise.reject(new Error("cached source failed"))
          : Promise.resolve([0xff, 0xd8]);
      }
      if (command === "invalidate_library_cover") return Promise.resolve();
      if (command === "resolve_library_metadata") {
        return Promise.resolve([
          "library-metadata-v4",
          "adult",
          "local-only",
          "current",
          "",
          "",
          "",
          "",
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
    Object.defineProperty(URL, "createObjectURL", {
      configurable: true,
      value: vi.fn(() => "blob:fresh"),
    });
    Object.defineProperty(URL, "revokeObjectURL", {
      configurable: true,
      value: vi.fn(),
    });

    const { result } = renderHook(() =>
      useLibraryPresentation({
        category: "adult",
        itemId: "CAWB-1",
        scanGeneration: "15",
      }),
    );
    await waitFor(() => expect(result.current.cover.status).toBe("unavailable"));
    expect(result.current.cover.aspectRatio).toBe(0.5);

    act(() => result.current.cover.retry?.());
    await waitFor(() => expect(result.current.cover.objectUrl).toBe("blob:fresh"));
    expect(result.current.cover.aspectRatio).toBe(0.5);
    expect(
      invoke.mock.calls.filter(
        ([command]) => command === "invalidate_library_cover",
      ),
    ).toHaveLength(1);
    expect(
      invoke.mock.calls.filter(([command]) => command === "resolve_library_cover"),
    ).toHaveLength(2);
  });

  it("retries metadata without restarting accepted cover resolution", async () => {
    const invoke = vi.fn((command: string) => {
      if (command === "resolve_library_cover") {
        return Promise.resolve([
          "library-cover-v3",
          "adult",
          "missing",
          "",
          "",
          "",
          "",
          "0.72",
          "",
          "",
          "",
        ]);
      }
      if (command === "resolve_library_metadata") {
        const metadataAttempt = invoke.mock.calls.filter(
          ([calledCommand]) => calledCommand === "resolve_library_metadata",
        ).length;
        if (metadataAttempt === 1) return Promise.reject(new Error("offline"));
        return Promise.resolve([
          "library-metadata-v4",
          "adult",
          "local-only",
          "current",
          "",
          "",
          "",
          "",
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

  it("lets metadata Retry establish first-class identity without a cover", async () => {
    const invoke = vi.fn((command: string) => {
      if (command === "resolve_library_cover") {
        return Promise.resolve([
          "library-cover-v3",
          "adult",
          "unavailable",
          "",
          "",
          "",
          "",
          "0.72",
          "",
          "",
          "",
        ]);
      }
      if (command === "resolve_library_metadata") {
        const attempt = invoke.mock.calls.filter(
          ([calledCommand]) => calledCommand === "resolve_library_metadata",
        ).length;
        return Promise.resolve(
          attempt === 1
            ? [
                "library-metadata-v4",
                "adult",
                "unavailable",
                "current",
                "",
                "",
                "",
                "",
                "",
                "",
                "",
                "",
                "",
                "0",
              ]
            : [
                "library-metadata-v4",
                "adult",
                "automatic",
                "current",
                "JavDB",
                "metadataitem",
                "CAWB-001",
                "JavDB",
                "metadataitem",
                "CAWB-001",
                "",
                "",
                "",
                "0",
              ],
        );
      }
      return Promise.reject(new Error(`Unexpected command ${command}`));
    });
    vi.stubGlobal("__TAURI__", { core: { invoke } });

    const { result } = renderHook(() =>
      useLibraryPresentation({
        category: "adult",
        itemId: "CAWB-1",
        scanGeneration: "metadata-identity",
      }),
    );
    await waitFor(() => expect(result.current.metadata.status).toBe("unavailable"));
    expect(result.current.verifiedIdentity).toBeNull();

    act(() => result.current.metadata.retry?.());
    await waitFor(() =>
      expect(result.current.verifiedIdentity).toEqual({
        provider: "JavDB",
        providerId: "metadataitem",
        displayCode: "CAWB-001",
      }),
    );
    expect(result.current.metadata.status).toBe("automatic");
    expect(
      invoke.mock.calls.filter(([command]) => command === "resolve_library_cover"),
    ).toHaveLength(1);
    expect(
      invoke.mock.calls.filter(([command]) => command === "resolve_library_metadata"),
    ).toHaveLength(2);
  });

  it("lets VR metadata establish the exact 3DSVR display identity without a cover", async () => {
    const invoke = vi.fn((command: string) => {
      if (command === "resolve_library_cover") {
        return Promise.resolve([
          "library-cover-v3",
          "vr",
          "unavailable",
          "",
          "",
          "",
          "",
          "0.72",
          "",
          "",
          "",
        ]);
      }
      if (command === "resolve_library_metadata") {
        return Promise.resolve([
          "library-metadata-v4",
          "vr",
          "automatic",
          "current",
          "JavDB",
          "vrmetadataitem",
          "3DSVR-01871",
          "JavDB",
          "vrmetadataitem",
          "3DSVR-01871",
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
        category: "vr",
        itemId: "3DSVR-01871",
        scanGeneration: "vr-metadata-identity",
      }),
    );
    await waitFor(() =>
      expect(result.current.verifiedIdentity).toEqual({
        provider: "JavDB",
        providerId: "vrmetadataitem",
        displayCode: "3DSVR-01871",
      }),
    );
    expect(result.current.cover.status).toBe("unavailable");
    expect(result.current.metadata.status).toBe("automatic");
  });

  it("clears a ready cover when metadata reports an exact identity conflict", async () => {
    const revokeObjectURL = vi.fn();
    const invoke = vi.fn((command: string) => {
      if (command === "resolve_library_cover") {
        return Promise.resolve([
          "library-cover-v3",
          "adult",
          "ready",
          "FANZA",
          "cawb00001",
          "CAWB-001",
          `library-cover-${"c".repeat(40)}`,
          "0.72",
          "FANZA",
          "cawb00001",
          "CAWB-001",
        ]);
      }
      if (command === "fetch_library_cover") return Promise.resolve([0xff, 0xd8]);
      if (command === "resolve_library_metadata") {
        return Promise.resolve([
          "library-metadata-v4",
          "adult",
          "unavailable",
          "conflict",
          "",
          "",
          "",
          "",
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
    Object.defineProperty(URL, "createObjectURL", {
      configurable: true,
      value: vi.fn(() => "blob:conflicted"),
    });
    Object.defineProperty(URL, "revokeObjectURL", {
      configurable: true,
      value: revokeObjectURL,
    });

    const { result } = renderHook(() =>
      useLibraryPresentation({
        category: "adult",
        itemId: "CAWB-1",
        scanGeneration: "metadata-conflict",
      }),
    );
    await waitFor(() => expect(result.current.metadata.status).toBe("unavailable"));
    expect(result.current.verifiedIdentity).toBeNull();
    expect(result.current.cover.status).toBe("unavailable");
    expect(result.current.cover.objectUrl).toBeNull();
    expect(result.current.cover.retry).not.toBeNull();
    expect(revokeObjectURL).toHaveBeenCalledWith("blob:conflicted");
  });

  it.each([
    {
      category: "adult" as const,
      itemId: "CAWB-1",
      fanzaId: "cawb00001",
      displayCode: "CAWB-001",
      conflictingDisplay: "CAWB-0001",
    },
    {
      category: "vr" as const,
      itemId: "3DSVR-01871",
      fanzaId: "13dsvr01871",
      displayCode: "3DSVR-01871",
      conflictingDisplay: "3DSVR-1871",
    },
  ])(
    "keeps a $category provider conflict authoritative until full exact-provider retry",
    async ({ category, itemId, fanzaId, displayCode, conflictingDisplay }) => {
      let coverAttempt = 0;
      let metadataAttempt = 0;
      const revokeObjectURL = vi.fn();
      const invoke = vi.fn((command: string) => {
        if (command === "resolve_library_cover") {
          coverAttempt += 1;
          return Promise.resolve([
            "library-cover-v3",
            category,
            "ready",
            "FANZA",
            fanzaId,
            displayCode,
            `library-cover-${(coverAttempt === 1 ? "1" : "2").repeat(40)}`,
            "0.72",
            "FANZA",
            fanzaId,
            displayCode,
          ]);
        }
        if (command === "fetch_library_cover") {
          return Promise.resolve([0xff, 0xd8]);
        }
        if (command === "resolve_library_metadata") {
          metadataAttempt += 1;
          const metadataDisplay =
            metadataAttempt === 1 ? conflictingDisplay : displayCode;
          return Promise.resolve([
            "library-metadata-v4",
            category,
            "automatic",
            "current",
            "JavDB",
            "javdbitem",
            metadataDisplay,
            "JavDB",
            "javdbitem",
            metadataDisplay,
            "Provider title",
            "",
            "",
            "0",
          ]);
        }
        if (command === "cancel_library_cover_request") return Promise.resolve();
        return Promise.reject(new Error(`Unexpected command ${command}`));
      });
      vi.stubGlobal("__TAURI__", { core: { invoke } });
      Object.defineProperty(URL, "createObjectURL", {
        configurable: true,
        value: vi
          .fn()
          .mockReturnValueOnce("blob:conflicting")
          .mockReturnValueOnce("blob:recovered"),
      });
      Object.defineProperty(URL, "revokeObjectURL", {
        configurable: true,
        value: revokeObjectURL,
      });

      const { result } = renderHook(() =>
        useLibraryPresentation({
          category,
          itemId,
          scanGeneration: "persistent-conflict",
        }),
      );
      await waitFor(() =>
        expect(result.current.metadata.status).toBe("unavailable"),
      );
      expect(result.current.verifiedIdentity).toBeNull();
      expect(result.current.cover.objectUrl).toBeNull();
      expect(revokeObjectURL).toHaveBeenCalledWith("blob:conflicting");

      act(() => result.current.metadata.retry?.());
      await waitFor(() =>
        expect(
          invoke.mock.calls.filter(
            ([calledCommand]) => calledCommand === "resolve_library_metadata",
          ),
        ).toHaveLength(2),
      );
      await waitFor(() =>
        expect(result.current.metadata.status).toBe("unavailable"),
      );
      expect(result.current.verifiedIdentity).toBeNull();
      expect(result.current.cover.objectUrl).toBeNull();
      expect(coverAttempt).toBe(1);

      act(() => result.current.cover.retry?.());
      await waitFor(() =>
        expect(result.current.cover.objectUrl).toBe("blob:recovered"),
      );
      await waitFor(() =>
        expect(result.current.metadata.status).toBe("automatic"),
      );
      expect(result.current.verifiedIdentity).toEqual({
        provider: "JavDB",
        providerId: "javdbitem",
        displayCode,
      });
      expect(coverAttempt).toBe(2);
      expect(metadataAttempt).toBe(3);
    },
  );

  it("rejects same-provider item disagreement between cover and metadata", async () => {
    const revokeObjectURL = vi.fn();
    const invoke = vi.fn((command: string) => {
      if (command === "resolve_library_cover") {
        return Promise.resolve([
          "library-cover-v3",
          "adult",
          "ready",
          "JavDB",
          "coveritem",
          "ADLT-123",
          `library-cover-${"d".repeat(40)}`,
          "0.72",
          "JavDB",
          "coveritem",
          "ADLT-123",
        ]);
      }
      if (command === "fetch_library_cover") return Promise.resolve([0xff, 0xd8]);
      if (command === "resolve_library_metadata") {
        return Promise.resolve([
          "library-metadata-v4",
          "adult",
          "automatic",
          "current",
          "JavDB",
          "metadataitem",
          "ADLT-123",
          "JavDB",
          "metadataitem",
          "ADLT-123",
          "Provider title",
          "",
          "",
          "0",
        ]);
      }
      return Promise.reject(new Error(`Unexpected command ${command}`));
    });
    vi.stubGlobal("__TAURI__", { core: { invoke } });
    Object.defineProperty(URL, "createObjectURL", {
      configurable: true,
      value: vi.fn(() => "blob:item-conflict"),
    });
    Object.defineProperty(URL, "revokeObjectURL", {
      configurable: true,
      value: revokeObjectURL,
    });

    const { result } = renderHook(() =>
      useLibraryPresentation({
        category: "adult",
        itemId: "ADLT-123",
        scanGeneration: "provider-item-conflict",
      }),
    );
    await waitFor(() =>
      expect(result.current.metadata.status).toBe("unavailable"),
    );
    expect(result.current.verifiedIdentity).toBeNull();
    expect(result.current.cover.objectUrl).toBeNull();
    expect(revokeObjectURL).toHaveBeenCalledWith("blob:item-conflict");
  });

  it("ignores obsolete metadata after a complete provider retry starts", async () => {
    const oldMetadata = deferred<unknown>();
    let coverAttempt = 0;
    let metadataAttempt = 0;
    const invoke = vi.fn((command: string) => {
      if (command === "resolve_library_cover") {
        coverAttempt += 1;
        return Promise.resolve([
          "library-cover-v3",
          "adult",
          coverAttempt === 1 ? "unavailable" : "missing",
          "",
          "",
          "",
          "",
          "0.72",
          ...(coverAttempt === 1
            ? ["", "", ""]
            : ["FANZA", "cawb00001", "CAWB-001"]),
        ]);
      }
      if (command === "resolve_library_metadata") {
        metadataAttempt += 1;
        if (metadataAttempt === 1) return oldMetadata.promise;
        return Promise.resolve([
          "library-metadata-v4",
          "adult",
          "automatic",
          "current",
          "JavDB",
          "currentitem",
          "CAWB-001",
          "JavDB",
          "currentitem",
          "CAWB-001",
          "Current metadata",
          "",
          "",
          "0",
        ]);
      }
      if (command === "cancel_library_cover_request") return Promise.resolve();
      return Promise.reject(new Error(`Unexpected command ${command}`));
    });
    vi.stubGlobal("__TAURI__", { core: { invoke } });

    const { result } = renderHook(() =>
      useLibraryPresentation({
        category: "adult",
        itemId: "CAWB-1",
        scanGeneration: "retry-generation",
      }),
    );
    await waitFor(() => expect(result.current.metadata.status).toBe("loading"));
    act(() => result.current.cover.retry?.());
    await waitFor(() =>
      expect(result.current.metadata.value?.title).toBe("Current metadata"),
    );

    await act(async () => {
      oldMetadata.resolve([
        "library-metadata-v4",
        "adult",
        "automatic",
        "current",
        "JavDB",
        "obsoleteitem",
        "CAWB-0001",
        "JavDB",
        "obsoleteitem",
        "CAWB-0001",
        "Obsolete metadata",
        "",
        "",
        "0",
      ]);
      await oldMetadata.promise;
    });
    expect(result.current.metadata.value?.title).toBe("Current metadata");
    expect(result.current.verifiedIdentity).toEqual({
      provider: "JavDB",
      providerId: "currentitem",
      displayCode: "CAWB-001",
    });
    expect(coverAttempt).toBe(2);
    expect(metadataAttempt).toBe(2);
  });

  it("rejects a late cover result after the exact item authority changes", async () => {
    const oldCover = deferred<unknown>();
    const invoke = vi.fn(
      (command: string, parameters?: Record<string, unknown>) => {
        if (command === "resolve_library_cover") {
          if (parameters?.itemId === "MDVR-419") return oldCover.promise;
          return Promise.resolve([
            "library-cover-v3",
            "vr",
            "missing",
            "",
            "",
            "",
            "",
            "0.72",
            "",
            "",
            "",
          ]);
        }
        if (command === "resolve_library_metadata") {
          return Promise.resolve([
            "library-metadata-v4",
            "vr",
            "local-only",
            "current",
            "",
            "",
            "",
            "",
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
        "library-cover-v3",
        "vr",
        "ready",
        "JavDB",
        "olditem",
        "MDVR-419",
        `library-cover-${"a".repeat(40)}`,
        "1.77",
        "JavDB",
        "olditem",
        "MDVR-419",
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
