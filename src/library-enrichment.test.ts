import { describe, expect, it, vi } from "vitest";

import {
  libraryEnrichmentConcurrency,
  parseLibraryPresentation,
  scheduleLibraryEnrichment,
} from "./library-enrichment";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}

function automaticPresentation(category: "movie" | "tv" | "adult" | "vr") {
  return [
    "library-enrichment-v1",
    category,
    "automatic",
    "TMDB",
    "419",
    "tt0123456",
    "Exact provider title",
    "Exact original title",
    "1999-04-19",
    "120 min",
    "Exact overview",
    "library-cover-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "ready",
    "0.6666666667",
    "2",
    "Drama",
    "Crime",
    "1",
    "Exact actor",
  ];
}

describe("Library enrichment response validation", () => {
  it("accepts one exact structured presentation and rejects cross-category or impossible cover authority", () => {
    expect(parseLibraryPresentation(automaticPresentation("movie"), "movie"))
      .toMatchObject({
        cast: ["Exact actor"],
        genres: ["Drama", "Crime"],
        providerId: "419",
        state: "automatic",
      });
    expect(parseLibraryPresentation(automaticPresentation("movie"), "tv"))
      .toBeNull();

    const impossibleCover = automaticPresentation("movie");
    impossibleCover[11] = "blob:stale-session-value";
    expect(parseLibraryPresentation(impossibleCover, "movie")).toBeNull();
  });

  it("rejects incomplete automatic identity and malformed variable-length fields", () => {
    const missingProvider = automaticPresentation("adult");
    missingProvider[4] = "";
    expect(parseLibraryPresentation(missingProvider, "adult")).toBeNull();

    const wrongGenreCount = automaticPresentation("vr");
    wrongGenreCount[14] = "3";
    expect(parseLibraryPresentation(wrongGenreCount, "vr")).toBeNull();
  });
});

describe("Library enrichment work scheduling", () => {
  it("shares one four-operation bound across simultaneous provider and cover work", async () => {
    const gates = Array.from({ length: 8 }, () => deferred<void>());
    let active = 0;
    let maximumActive = 0;
    const work = gates.map((gate) =>
      scheduleLibraryEnrichment(async () => {
        active += 1;
        maximumActive = Math.max(maximumActive, active);
        await gate.promise;
        active -= 1;
      }),
    );

    await vi.waitFor(() => expect(active).toBe(libraryEnrichmentConcurrency));
    expect(libraryEnrichmentConcurrency).toBe(4);
    expect(maximumActive).toBe(4);

    gates[0].resolve();
    await vi.waitFor(() => expect(active).toBe(4));
    expect(maximumActive).toBe(4);

    gates.slice(1).forEach((gate) => gate.resolve());
    await Promise.all(work);
    expect(maximumActive).toBe(4);
  });

  it("drops obsolete queued work before it can delay the latest current request", async () => {
    const activeGates = Array.from({ length: 4 }, () => deferred<void>());
    const active = activeGates.map((gate) =>
      scheduleLibraryEnrichment(() => gate.promise),
    );
    let staleCurrent = true;
    const staleDispatch = vi.fn(async () => undefined);
    const stale = scheduleLibraryEnrichment(
      staleDispatch,
      () => staleCurrent,
    );
    const latestDispatch = vi.fn(async () => undefined);
    const latest = scheduleLibraryEnrichment(latestDispatch);

    staleCurrent = false;
    activeGates[0].resolve();
    await expect(stale).rejects.toThrow("no longer current");
    await latest;
    expect(staleDispatch).not.toHaveBeenCalled();
    expect(latestDispatch).toHaveBeenCalledTimes(1);

    activeGates.slice(1).forEach((gate) => gate.resolve());
    await Promise.all(active);
  });
});
