import { afterEach, describe, expect, it, vi } from "vitest";

import { fetchWeeklyTrendingMovies, tmdbPosterUrl } from "./tmdb";

const testToken = "fixture-read-access-token";

function jsonResponse(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), {
    headers: { "Content-Type": "application/json" },
    status,
  });
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("TMDB weekly trending request", () => {
  it("sends the token only in the Bearer header", async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({ results: [] }));
    vi.stubGlobal("fetch", fetchMock);

    await fetchWeeklyTrendingMovies(testToken);

    expect(fetchMock).toHaveBeenCalledWith(
      "https://api.themoviedb.org/3/trending/movie/week",
      {
        headers: {
          Accept: "application/json",
          Authorization: `Bearer ${testToken}`,
        },
        signal: undefined,
      },
    );
    expect(fetchMock.mock.calls[0][0]).not.toContain(testToken);
  });

  it("preserves exact valid titles and ignores malformed movie results", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        jsonResponse({
          results: [
            {
              id: 41,
              title: "映画  —  Director's “Cut”!",
              poster_path: "/poster.jpg",
              release_date: "2026-07-31",
            },
            {
              id: 42,
              title: "Posterless Movie",
              poster_path: null,
              release_date: "",
            },
            { id: 43, title: "   ", poster_path: "/ignored.jpg" },
            { id: "44", title: "Invalid identifier" },
            null,
          ],
        }),
      ),
    );

    await expect(fetchWeeklyTrendingMovies(testToken)).resolves.toEqual({
      status: "ready",
      movies: [
        {
          id: 41,
          title: "映画  —  Director's “Cut”!",
          posterPath: "/poster.jpg",
          releaseDate: "2026-07-31",
        },
        {
          id: 42,
          title: "Posterless Movie",
          posterPath: null,
          releaseDate: null,
        },
      ],
    });
    expect(tmdbPosterUrl("/poster.jpg")).toBe(
      "https://image.tmdb.org/t/p/w500/poster.jpg",
    );
  });

  it.each([
    [401, "unauthorized"],
    [403, "unauthorized"],
    [429, "rate-limited"],
    [500, "provider-error"],
  ] as const)("maps HTTP %i to %s", async (status, expectedStatus) => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(jsonResponse({}, status)));

    await expect(fetchWeeklyTrendingMovies(testToken)).resolves.toEqual({
      status: expectedStatus,
    });
  });

  it("distinguishes an empty feed from a malformed response", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(jsonResponse({ results: [] }))
      .mockResolvedValueOnce(jsonResponse({ page: 1 }));
    vi.stubGlobal("fetch", fetchMock);

    await expect(fetchWeeklyTrendingMovies(testToken)).resolves.toEqual({
      status: "empty",
    });
    await expect(fetchWeeklyTrendingMovies(testToken)).resolves.toEqual({
      status: "provider-error",
    });
  });

  it("reports a network error when the provider cannot be reached", async () => {
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new TypeError("offline")));

    await expect(fetchWeeklyTrendingMovies(testToken)).resolves.toEqual({
      status: "network-error",
    });
  });
});
