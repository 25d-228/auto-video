import { beforeEach, describe, expect, it, vi } from "vitest";

import { fetchVerifiedApiBayTvReleases } from "./tv-release";

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
    "Exact  Show — 特別版.S02E03.第三話",
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
    "Exact Show - 2x03 - 2160p",
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
          name: "Exact  Show — 特別版.S02E03.第三話",
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
          name: "Exact Show - 2x03 - 2160p",
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
