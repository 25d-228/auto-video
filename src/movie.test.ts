import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  clearMovieMetadataMatch,
  fetchVerifiedYtsMovieReleases,
  inspectVerifiedYtsMovieTorrent,
  invalidateMovieMetadataMatchContext,
  invalidateMovieReleaseContext,
  invalidateVerifiedMovieTorrent,
  parseMovieLibraryScan,
  saveMovieMetadataMatch,
  saveVerifiedMovieTorrent,
  searchMovieMetadata,
  startVerifiedMovieDownload,
  verifyMovieMetadataCandidate,
  type MovieReleaseContext,
  type YtsMovieRelease,
} from "./movie";

const invokeMock = vi.fn<(command: string, parameters?: Record<string, unknown>) => Promise<unknown>>();

beforeEach(() => {
  invokeMock.mockReset();
  vi.stubGlobal("__TAURI__", { core: { invoke: invokeMock } });
});

const infohash = "0123456789abcdef0123456789abcdef01234567";
const torrentUrl = `https://yts.mx/torrent/download/${infohash.toUpperCase()}`;

describe("trusted Movie Library metadata boundary", () => {
  const fileId = "1111111111111111111111111111111111111111";
  const requestId = "2222222222222222222222222222222222222222";
  const verificationId = "3333333333333333333333333333333333333333";

  it("parses exact trusted scan identity and accepted metadata without normalizing text", () => {
    expect(
      parseMovieLibraryScan([
        "movie-library-v1",
        "ready",
        "4",
        "1",
        fileId,
        "/Movies/映画  —  Local.File.MKV",
        "映画  —  Local.File.MKV",
        "987654321",
        "1",
        "419",
        "tt0123456",
        "Accepted  Title — 特別版",
        "Original  Title",
        "1999-04-19",
        "/poster.jpg",
        "Exact  overview.",
        "7",
      ]),
    ).toEqual({
      generation: "4",
      metadataStatus: "ready",
      movies: [
        {
          association: {
            generation: "7",
            imdbId: "tt0123456",
            originalTitle: "Original  Title",
            overview: "Exact  overview.",
            posterPath: "/poster.jpg",
            releaseDate: "1999-04-19",
            title: "Accepted  Title — 特別版",
            tmdbMovieId: 419,
          },
          fileId,
          path: "/Movies/映画  —  Local.File.MKV",
          relativePath: "映画  —  Local.File.MKV",
          sizeBytes: "987654321",
        },
      ],
    });
  });

  it("rejects malformed, duplicate, and conflicting trusted scan rows", () => {
    const validRow = [
      fileId,
      "/Movies/Exact.mp4",
      "Exact.mp4",
      "5",
      "0",
      "",
      "",
      "",
      "",
      "",
      "",
      "",
      "",
    ];
    for (const response of [
      ["movie-library-v2", "ready", "1", "0"],
      ["movie-library-v1", "invalid", "1", "0"],
      ["movie-library-v1", "ready", "0", "0"],
      ["movie-library-v1", "ready", "1", "1", ...validRow.slice(0, -1)],
      ["movie-library-v1", "ready", "1", "2", ...validRow, ...validRow],
      [
        "movie-library-v1",
        "ready",
        "1",
        "1",
        ...validRow.slice(0, 4),
        "1",
        "419",
        "tt0123456",
        "",
        "",
        "",
        "",
        "",
        "1",
      ],
    ]) {
      expect(parseMovieLibraryScan(response)).toBeNull();
    }
  });

  it("submits only explicit exact matching identities through search, verification, Save, clear, and invalidation", async () => {
    invokeMock
      .mockResolvedValueOnce([
        requestId,
        "1",
        "419",
        "Candidate  Title",
        "Original Title",
        "1999-04-19",
        "/poster.jpg",
      ])
      .mockResolvedValueOnce([
        verificationId,
        "419",
        "tt0123456",
        "Accepted  Title",
        "Original Title",
        "1999-04-19",
        "/poster.jpg",
        "Overview.",
        "1",
      ])
      .mockResolvedValueOnce([
        "419",
        "tt0123456",
        "Accepted  Title",
        "Original Title",
        "1999-04-19",
        "/poster.jpg",
        "Overview.",
        "4",
      ])
      .mockResolvedValue(undefined);

    await expect(searchMovieMetadata(fileId, "Exact  Query — 特別版", 1)).resolves.toMatchObject({
      matchingRequestId: requestId,
      candidates: [{ tmdbMovieId: 419, title: "Candidate  Title" }],
    });
    await expect(verifyMovieMetadataCandidate(requestId, 419, 2)).resolves.toMatchObject({
      verificationId,
      association: { imdbId: "tt0123456", tmdbMovieId: 419 },
    });
    await expect(saveMovieMetadataMatch(verificationId)).resolves.toMatchObject({
      generation: "4",
      imdbId: "tt0123456",
      tmdbMovieId: 419,
    });
    await clearMovieMetadataMatch(fileId);
    await invalidateMovieMetadataMatchContext(3);
    expect(invokeMock.mock.calls).toEqual([
      ["search_movie_metadata", { contextGeneration: 1, fileId, query: "Exact  Query — 特別版" }],
      ["verify_movie_metadata_candidate", { contextGeneration: 2, matchingRequestId: requestId, tmdbMovieId: 419 }],
      ["save_movie_metadata_match", { verificationId }],
      ["clear_movie_metadata_match", { fileId }],
      ["invalidate_movie_metadata_match_context", { contextGeneration: 3 }],
    ]);
  });
});

function movieReleaseResponse() {
  return [
    "419",
    "Exact  Movie — 特別版",
    "1999-04-19",
    "tt0123456",
    "700",
    "Exact  YTS Movie — 特別版",
    "1999",
    "2",
    "700:0",
    "1080p",
    "bluray",
    "x264",
    "1.5 GB",
    "1500000000",
    "42",
    "7",
    infohash,
    torrentUrl,
    "700:1",
    "2160p",
    "web",
    "x265",
    "4.0 GB",
    "4000000000",
    "2",
    "0",
    "",
    "",
  ];
}

describe("verified YTS Movie release boundary", () => {
  it("parses only the native-accepted exact IMDb rows and preserves exact metadata", async () => {
    invokeMock.mockResolvedValue(movieReleaseResponse());

    await expect(fetchVerifiedYtsMovieReleases(419)).resolves.toEqual({
      status: "ready",
      context: {
        tmdbMovieId: 419,
        tmdbTitle: "Exact  Movie — 特別版",
        releaseDate: "1999-04-19",
        imdbId: "tt0123456",
        providerMovieId: 700,
        providerTitle: "Exact  YTS Movie — 特別版",
        providerYear: "1999",
      },
      releases: [
        {
          artifact: { expectedInfohash: infohash, torrentUrl },
          rowId: "700:0",
          quality: "1080p",
          typeLabel: "bluray",
          videoCodec: "x264",
          size: "1.5 GB",
          sizeBytes: "1500000000",
          seeds: "42",
          peers: "7",
          source: "YTS",
        },
        {
          rowId: "700:1",
          quality: "2160p",
          typeLabel: "web",
          videoCodec: "x265",
          size: "4.0 GB",
          sizeBytes: "4000000000",
          seeds: "2",
          peers: "0",
          source: "YTS",
        },
      ],
    });
    expect(invokeMock).toHaveBeenCalledWith("fetch_yts_movie_releases", {
      tmdbMovieId: 419,
    });
  });

  it("rejects malformed native rows and maps every provider identity failure", async () => {
    invokeMock.mockResolvedValueOnce([...movieReleaseResponse(), "extra"]);
    await expect(fetchVerifiedYtsMovieReleases(419)).resolves.toEqual({
      status: "yts-malformed-provider",
    });

    for (const [error, status] of [
      ["movie_tmdb_unauthorized", "tmdb-unauthorized"],
      ["movie_tmdb_rate_limited", "tmdb-rate-limited"],
      ["movie_tmdb_network_error", "tmdb-network-error"],
      ["movie_tmdb_malformed", "tmdb-malformed-provider"],
      ["movie_no_imdb_identity", "no-imdb-identity"],
      ["movie_yts_source_unavailable", "yts-source-unavailable"],
      ["movie_yts_network_error", "yts-network-error"],
      ["movie_yts_malformed", "yts-malformed-provider"],
      ["movie_yts_conflicting_provider", "yts-conflicting-provider"],
      ["movie_yts_provider_error", "yts-provider-error"],
    ] as const) {
      invokeMock.mockRejectedValueOnce(error);
      await expect(fetchVerifiedYtsMovieReleases(419)).resolves.toEqual({ status });
    }
  });
});

describe("verified YTS Movie torrent inspection and Save", () => {
  const context: MovieReleaseContext = {
    tmdbMovieId: 419,
    tmdbTitle: "Exact  Movie — 特別版",
    releaseDate: "1999-04-19",
    imdbId: "tt0123456",
    providerMovieId: 700,
    providerTitle: "Exact  YTS Movie — 特別版",
    providerYear: "1999",
  };
  const release: YtsMovieRelease = {
    artifact: { expectedInfohash: infohash, torrentUrl },
    rowId: "700:0",
    quality: "1080p",
    typeLabel: "bluray",
    videoCodec: "x264",
    size: "1.5 GB",
    sizeBytes: "1500000000",
    seeds: "42",
    peers: "7",
    source: "YTS",
  };

  it("binds every exact Movie and torrent field to native inspection", async () => {
    invokeMock.mockResolvedValue([
      "movie-1-1-hash",
      "Exact  Torrent — 特別版",
      infohash,
      "12",
      "Folder/Part  1 — 映画.mkv",
      "5",
      "Folder/特別版  B.mp4",
      "7",
    ]);
    await expect(inspectVerifiedYtsMovieTorrent(context, release)).resolves.toEqual({
      status: "ready",
      inspection: {
        inspectionId: "movie-1-1-hash",
        displayName: "Exact  Torrent — 特別版",
        infohash,
        totalBytes: "12",
        files: [
          { path: "Folder/Part  1 — 映画.mkv", sizeBytes: "5" },
          { path: "Folder/特別版  B.mp4", sizeBytes: "7" },
        ],
      },
    });
    expect(invokeMock).toHaveBeenCalledWith("inspect_yts_movie_torrent", {
      tmdbMovieId: 419,
      tmdbTitle: context.tmdbTitle,
      releaseDate: context.releaseDate,
      imdbId: "tt0123456",
      providerMovieId: 700,
      providerTitle: context.providerTitle,
      providerYear: "1999",
      rowId: "700:0",
      quality: "1080p",
      typeLabel: "bluray",
      videoCodec: "x264",
      size: "1.5 GB",
      sizeBytes: "1500000000",
      seeds: "42",
      peers: "7",
      expectedInfohash: infohash,
      torrentUrl,
    });
  });

  it("maps every Movie inspection failure without exposing files or Save", async () => {
    for (const [error, status] of [
      ["movie_torrent_source_unavailable", "source-unavailable"],
      ["movie_torrent_network_error", "network-error"],
      ["movie_torrent_provider_error", "provider-error"],
      ["movie_torrent_malformed", "malformed-torrent"],
      ["movie_torrent_unsupported", "unsupported-torrent"],
      ["movie_torrent_infohash_mismatch", "infohash-mismatch"],
      ["movie_torrent_context_invalid", "stale-context"],
      ["movie_torrent_stale", "stale-context"],
      ["unexpected_error", "inspection-error"],
    ] as const) {
      invokeMock.mockRejectedValueOnce(error);
      await expect(
        inspectVerifiedYtsMovieTorrent(context, release),
      ).resolves.toEqual({ status });
    }
  });

  it("exposes no inspection for an incomplete row and keeps Save category-specific", async () => {
    await expect(
      inspectVerifiedYtsMovieTorrent(context, { ...release, artifact: undefined }),
    ).resolves.toEqual({ status: "malformed-torrent" });
    expect(invokeMock).not.toHaveBeenCalled();

    invokeMock.mockResolvedValueOnce(true).mockResolvedValueOnce(undefined).mockResolvedValueOnce(undefined);
    await expect(saveVerifiedMovieTorrent("movie-1-1-hash")).resolves.toBe(true);
    await invalidateVerifiedMovieTorrent();
    await invalidateMovieReleaseContext();
    expect(invokeMock.mock.calls).toEqual([
      ["save_verified_movie_torrent", { inspectionId: "movie-1-1-hash" }],
      ["invalidate_verified_movie_torrent"],
      ["invalidate_movie_release_context"],
    ]);
  });

  it("starts only an explicit unique Movie file selection from the current inspection", async () => {
    invokeMock.mockResolvedValue("movie-transfer-419");
    await expect(
      startVerifiedMovieDownload("movie-1-1-hash", [2, 0]),
    ).resolves.toBe("movie-transfer-419");
    expect(invokeMock).toHaveBeenCalledWith("start_verified_movie_download", {
      inspectionId: "movie-1-1-hash",
      selectedFileIds: [2, 0],
    });

    for (const selectedFileIds of [[], [0, 0], [-1], [1.5]]) {
      await expect(
        startVerifiedMovieDownload("movie-1-1-hash", selectedFileIds),
      ).rejects.toThrow("current Movie inspection");
    }
    await expect(startVerifiedMovieDownload(" ", [0])).rejects.toThrow(
      "current Movie inspection",
    );
    expect(invokeMock).toHaveBeenCalledOnce();
  });
});
