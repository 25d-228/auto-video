import { describe, expect, it } from "vitest";

import {
  parseTmdbCardCoverResponse,
  tmdbCoverMime,
  type TmdbCardCoverRequest,
} from "@/tmdb-cover";

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
    "ready",
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

describe("TMDB card-cover response authority", () => {
  it.each([movieDiscover, { ...movieDiscover, category: "tv" as const }, {
    ...tvLibrary,
    category: "movie" as const,
  }, tvLibrary])("accepts an exact current %s response", (request) => {
    expect(parseTmdbCardCoverResponse(response(request), request, "12")).toEqual({
      authorityId: `tmdb-cover-${"b".repeat(40)}`,
      status: "ready",
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
});
