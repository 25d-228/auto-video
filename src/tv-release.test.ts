import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  fetchVerifiedApiBayTvReleases,
  inspectVerifiedApiBayTvTorrent,
  invalidateTvReleaseContext,
  invalidateVerifiedTvTorrent,
  saveVerifiedTvTorrent,
  selectVerifiedApiBayTvRelease,
  startVerifiedTvDownload,
} from "./tv-release";

const invokeMock = vi.fn<
  (command: string, parameters?: Record<string, unknown>) => Promise<unknown>
>();
const firstHash = "0123456789abcdef0123456789abcdef01234567";
const secondHash = "abcdef0123456789abcdef0123456789abcdef01";

beforeEach(() => {
  invokeMock.mockReset();
  vi.stubGlobal("__TAURI__", { core: { invoke: invokeMock } });
});

function releaseResponse() {
  return [
    "701",
    "Exact  Show — 特別版",
    "9001",
    "2",
    "9103",
    "3",
    "第三話  —  Exact Episode",
    "tt0123456",
    "2",
    "1001",
    "Exact  Show — 特別版.S02E03+720p.第三話",
    "205",
    "419000000",
    "12",
    "4",
    "Exact Uploader",
    "vip",
    "1710000000",
    firstHash,
    "API Bay",
    "1002",
    "Exact Show - 2x03+10bit - 2160p",
    "208",
    "",
    "",
    "0",
    "",
    "",
    "",
    secondHash,
    "API Bay",
  ];
}

describe("verified API Bay TV release boundary", () => {
  it("passes only application-owned provider IDs and preserves exact native metadata", async () => {
    invokeMock.mockResolvedValue(releaseResponse());

    await expect(fetchVerifiedApiBayTvReleases(701, 9001, 9103)).resolves.toEqual({
      status: "ready",
      context: {
        tmdbTvId: 701,
        showName: "Exact  Show — 特別版",
        providerSeasonId: 9001,
        seasonNumber: 2,
        providerEpisodeId: 9103,
        episodeNumber: 3,
        episodeName: "第三話  —  Exact Episode",
        imdbId: "tt0123456",
      },
      releases: [
        {
          providerItemId: "1001",
          name: "Exact  Show — 特別版.S02E03+720p.第三話",
          category: "205",
          sizeBytes: "419000000",
          seeders: "12",
          leechers: "4",
          uploader: "Exact Uploader",
          providerStatus: "vip",
          added: "1710000000",
          infohash: firstHash,
          source: "API Bay",
        },
        {
          providerItemId: "1002",
          name: "Exact Show - 2x03+10bit - 2160p",
          category: "208",
          sizeBytes: null,
          seeders: null,
          leechers: "0",
          uploader: null,
          providerStatus: null,
          added: null,
          infohash: secondHash,
          source: "API Bay",
        },
      ],
    });
    expect(invokeMock).toHaveBeenCalledWith("fetch_apibay_tv_releases", {
      tmdbTvId: 701,
      providerSeasonId: 9001,
      providerEpisodeId: 9103,
    });
  });

  it("inspects only native-selected TV identity and parses the complete exact file list", async () => {
    const context = {
      tmdbTvId: 701,
      showName: "Exact  Show — 特別版",
      providerSeasonId: 9001,
      seasonNumber: 2,
      providerEpisodeId: 9103,
      episodeNumber: 3,
      episodeName: "第三話  —  Exact Episode",
      imdbId: "tt0123456",
    };
    const release = {
      providerItemId: "1001",
      name: "Exact  Show — 特別版.S02E03+720p.第三話",
      category: "205" as const,
      sizeBytes: "419000000",
      seeders: "12",
      leechers: "4",
      uploader: "Exact Uploader",
      providerStatus: "vip",
      added: "1710000000",
      infohash: firstHash,
      source: "API Bay" as const,
    };
    invokeMock
      .mockResolvedValueOnce(undefined)
      .mockResolvedValueOnce([
        "tv-1-1-1001",
        "Exact  Torrent — 特別版",
        firstHash,
        "12",
        "Show/第三話  —  Exact Episode.mkv",
        "5",
        "Extras/予告  編.mp4",
        "7",
      ]);

    await selectVerifiedApiBayTvRelease(context, release);
    await expect(inspectVerifiedApiBayTvTorrent(context, release)).resolves.toEqual({
      status: "ready",
      inspection: {
        inspectionId: "tv-1-1-1001",
        displayName: "Exact  Torrent — 特別版",
        infohash: firstHash,
        totalBytes: "12",
        files: [
          { path: "Show/第三話  —  Exact Episode.mkv", sizeBytes: "5" },
          { path: "Extras/予告  編.mp4", sizeBytes: "7" },
        ],
      },
    });
    expect(invokeMock).toHaveBeenNthCalledWith(1, "select_apibay_tv_release", {
      tmdbTvId: 701,
      providerSeasonId: 9001,
      providerEpisodeId: 9103,
      providerItemId: "1001",
    });
    expect(invokeMock).toHaveBeenNthCalledWith(2, "inspect_apibay_tv_torrent", {
      tmdbTvId: 701,
      providerSeasonId: 9001,
      providerEpisodeId: 9103,
      providerItemId: "1001",
    });
  });

  it("keeps local readiness, metadata, identity, and Save failures distinct", async () => {
    const context = {
      tmdbTvId: 701,
      showName: "Exact Show",
      providerSeasonId: 9001,
      seasonNumber: 2,
      providerEpisodeId: 9103,
      episodeNumber: 3,
      episodeName: "Episode 3",
      imdbId: "tt0123456",
    };
    const release = {
      providerItemId: "1001",
      name: "Exact Show S02E03",
      category: "205" as const,
      sizeBytes: null,
      seeders: null,
      leechers: null,
      uploader: null,
      providerStatus: null,
      added: null,
      infohash: firstHash,
      source: "API Bay" as const,
    };
    for (const [error, status] of [
      ["tv_torrent_local_pending", "local-pending"],
      ["tv_torrent_local_unavailable", "local-unavailable"],
      ["tv_torrent_network_error", "network-error"],
      ["tv_torrent_timeout", "timeout"],
      ["tv_torrent_no_metadata_source", "no-metadata-source"],
      ["tv_torrent_malformed", "malformed-torrent"],
      ["tv_torrent_unsupported", "unsupported-torrent"],
      ["tv_torrent_infohash_mismatch", "infohash-mismatch"],
      ["tv_torrent_stale", "stale-context"],
      ["unexpected", "inspection-error"],
    ] as const) {
      invokeMock.mockRejectedValueOnce(error);
      await expect(inspectVerifiedApiBayTvTorrent(context, release)).resolves.toEqual({
        status,
      });
    }

    invokeMock
      .mockResolvedValueOnce(false)
      .mockResolvedValueOnce(true)
      .mockResolvedValueOnce(undefined)
      .mockResolvedValueOnce(undefined);
    await expect(saveVerifiedTvTorrent("tv-1-1-1001")).resolves.toBe(false);
    await expect(saveVerifiedTvTorrent("tv-1-1-1001")).resolves.toBe(true);
    await invalidateVerifiedTvTorrent();
    await invalidateTvReleaseContext();
    expect(invokeMock).toHaveBeenNthCalledWith(11, "save_verified_tv_torrent", {
      inspectionId: "tv-1-1-1001",
    });
    expect(invokeMock).toHaveBeenNthCalledWith(13, "invalidate_verified_tv_torrent");
    expect(invokeMock).toHaveBeenNthCalledWith(14, "invalidate_tv_release_context");
  });

  it("starts only an exact inspection with explicit unique selected file IDs", async () => {
    invokeMock.mockResolvedValue("tv-transfer-123");

    await expect(startVerifiedTvDownload("tv-1-1-1001", [1, 0])).resolves.toBe(
      "tv-transfer-123",
    );
    expect(invokeMock).toHaveBeenCalledWith("start_verified_tv_download", {
      inspectionId: "tv-1-1-1001",
      selectedFileIds: [1, 0],
    });

    for (const [inspectionId, selectedFileIds] of [
      ["", [0]],
      ["   ", [0]],
      ["tv-1", []],
      ["tv-1", [0, 0]],
      ["tv-1", [-1]],
      ["tv-1", [1.5]],
    ] as const) {
      await expect(
        startVerifiedTvDownload(inspectionId, [...selectedFileIds]),
      ).rejects.toThrow(
        "A current TV inspection and valid file selection are required.",
      );
    }
    expect(invokeMock).toHaveBeenCalledTimes(1);
  });

  it("preserves a native no-match without inventing release rows", async () => {
    const noMatchResponse = releaseResponse().slice(0, 9);
    noMatchResponse[8] = "0";
    invokeMock.mockResolvedValue(noMatchResponse);

    await expect(fetchVerifiedApiBayTvReleases(701, 9001, 9103)).resolves.toEqual({
      status: "ready",
      context: {
        tmdbTvId: 701,
        showName: "Exact  Show — 特別版",
        providerSeasonId: 9001,
        seasonNumber: 2,
        providerEpisodeId: 9103,
        episodeNumber: 3,
        episodeName: "第三話  —  Exact Episode",
        imdbId: "tt0123456",
      },
      releases: [],
    });
  });

  it("rejects malformed rows and maps each native failure locally", async () => {
    const malformed = releaseResponse();
    malformed[22] = "999";
    invokeMock.mockResolvedValueOnce(malformed);
    await expect(fetchVerifiedApiBayTvReleases(701, 9001, 9103)).resolves.toEqual({
      status: "apibay-malformed-provider",
    });

    for (const [error, status] of [
      ["tv_release_tmdb_unauthorized", "tmdb-unauthorized"],
      ["tv_release_tmdb_rate_limited", "tmdb-rate-limited"],
      ["tv_release_tmdb_network_error", "tmdb-network-error"],
      ["tv_release_tmdb_malformed", "tmdb-malformed-provider"],
      ["tv_release_no_imdb_identity", "no-imdb-identity"],
      ["tv_release_apibay_source_unavailable", "apibay-source-unavailable"],
      ["tv_release_apibay_network_error", "apibay-network-error"],
      ["tv_release_apibay_malformed", "apibay-malformed-provider"],
      ["tv_release_apibay_conflicting", "apibay-conflicting-provider"],
      ["tv_release_apibay_provider_error", "apibay-provider-error"],
      ["unexpected", "tmdb-provider-error"],
    ] as const) {
      invokeMock.mockRejectedValueOnce(error);
      await expect(fetchVerifiedApiBayTvReleases(701, 9001, 9103)).resolves.toEqual({
        status,
      });
    }
  });

  it("rejects invalid IDs before native dispatch", async () => {
    for (const [tmdbTvId, providerSeasonId, providerEpisodeId] of [
      [0, 9001, 9103],
      [701, -1, 9103],
      [701, 9001, 1.5],
    ]) {
      await expect(
        fetchVerifiedApiBayTvReleases(
          tmdbTvId,
          providerSeasonId,
          providerEpisodeId,
        ),
      ).rejects.toThrow(
        "Positive provider TV, season, and episode IDs",
      );
    }
    expect(invokeMock).not.toHaveBeenCalled();
  });
});
