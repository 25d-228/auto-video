import { afterEach, describe, expect, it, vi } from "vitest";

import {
  fetchTmdbMoviesByTitle,
  fetchWeeklyTrendingMovies,
  tmdbPosterUrl,
} from "./tmdb";

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

describe("TMDB Movies title search request", () => {
  it("encodes the exact query as request data and keeps the token in the Bearer header", async () => {
    const query = "  映画 — Director's “Cut”! & CAPS  ";
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({ results: [] }));
    vi.stubGlobal("fetch", fetchMock);

    await fetchTmdbMoviesByTitle(testToken, query);

    const [requestUrl, requestOptions] = fetchMock.mock.calls[0];
    const parsedRequestUrl = new URL(requestUrl);
    expect(parsedRequestUrl.origin + parsedRequestUrl.pathname).toBe(
      "https://api.themoviedb.org/3/search/movie",
    );
    expect(parsedRequestUrl.searchParams.get("query")).toBe(query);
    expect(requestUrl).not.toContain(testToken);
    expect(requestOptions).toEqual({
      headers: {
        Accept: "application/json",
        Authorization: `Bearer ${testToken}`,
      },
      signal: undefined,
    });
  });

  it("rejects a whitespace-only query before dispatch", () => {
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);

    expect(() => fetchTmdbMoviesByTitle(testToken, " \t\n ")).toThrow(
      "A TMDB Movies search query is required.",
    );
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("preserves exact titles and card metadata from valid search results", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        jsonResponse({
          results: [
            {
              id: 51,
              title: "映画  —  Director's “Cut”!",
              poster_path: "/search-poster.jpg",
              release_date: "2026-08-02",
            },
            {
              id: 52,
              title: "Posterless Search Result",
              poster_path: null,
              release_date: "",
            },
          ],
        }),
      ),
    );

    await expect(
      fetchTmdbMoviesByTitle(testToken, "Director's Cut"),
    ).resolves.toEqual({
      status: "ready",
      movies: [
        {
          id: 51,
          title: "映画  —  Director's “Cut”!",
          posterPath: "/search-poster.jpg",
          releaseDate: "2026-08-02",
        },
        {
          id: 52,
          title: "Posterless Search Result",
          posterPath: null,
          releaseDate: null,
        },
      ],
    });
  });

  it.each([
    {
      caseName: "empty results",
      expectedStatus: "empty",
      response: jsonResponse({ results: [] }),
    },
    {
      caseName: "unauthorized credentials",
      expectedStatus: "unauthorized",
      response: jsonResponse({}, 401),
    },
    {
      caseName: "rate limiting",
      expectedStatus: "rate-limited",
      response: jsonResponse({}, 429),
    },
    {
      caseName: "general provider failure",
      expectedStatus: "provider-error",
      response: jsonResponse({}, 500),
    },
    {
      caseName: "malformed provider data",
      expectedStatus: "malformed-provider",
      response: jsonResponse({ page: 1 }),
    },
    {
      caseName: "malformed provider JSON",
      expectedStatus: "malformed-provider",
      response: new Response("not json", { status: 200 }),
    },
  ])(
    "reports $caseName as $expectedStatus",
    async ({ expectedStatus, response }) => {
      vi.stubGlobal("fetch", vi.fn().mockResolvedValue(response));

      await expect(
        fetchTmdbMoviesByTitle(testToken, "Fixture query"),
      ).resolves.toEqual({ status: expectedStatus });
    },
  );

  it("reports a network error when search cannot reach TMDB", async () => {
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new TypeError("offline")));

    await expect(
      fetchTmdbMoviesByTitle(testToken, "Fixture query"),
    ).resolves.toEqual({ status: "network-error" });
  });
});
