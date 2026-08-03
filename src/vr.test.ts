import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  canonicalizeProductCode,
  fetchExactJavdbVrItem,
  fetchVerifiedSukebeiReleases,
  inspectVerifiedSukebeiTorrent,
  loadVrDownloads,
  loadVrFolder,
  scanVrLibrary,
  startVerifiedVrDownload,
} from "./vr";

const catalogFixture = `
  <!doctype html>
  <html><body>
    <div class="movie-list">
      <div class="item">
        <a class="box" href="/v/wrong">
          <img src="https://images.example/neighbor.jpg">
          <div class="video-title"><strong>MDVR-422</strong> Neighbor</div>
        </a>
      </div>
      <div class="item">
        <a class="box" href="/v/exact">
          <img data-src="https://images.example/exact.jpg">
          <div class="video-title"><strong>mdvr_00419</strong> Provider title</div>
        </a>
      </div>
    </div>
  </body></html>
`;

function releaseItem(
  name: string,
  size = "12.5 GiB",
  seeders = "10",
) {
  return `<item>
    <title>${name}</title>
    <nyaa:size>${size}</nyaa:size>
    <nyaa:seeders>${seeders}</nyaa:seeders>
  </item>`;
}

function releaseFeed(items: string) {
  return `<rss xmlns:nyaa="https://sukebei.nyaa.si/xmlns/nyaa" version="2.0">
    <channel><title>Sukebei results</title>${items}</channel>
  </rss>`;
}

function releaseArtifactElements(
  itemId: string,
  infohash = "0123456789abcdef0123456789abcdef01234567",
  torrentUrl = `https://sukebei.nyaa.si/download/${itemId}.torrent`,
) {
  return `<guid>https://sukebei.nyaa.si/view/${itemId}</guid>
    <link>${torrentUrl}</link>
    <nyaa:infoHash>${infohash}</nyaa:infoHash>`;
}

let invokeMock: ReturnType<typeof vi.fn>;

beforeEach(() => {
  invokeMock = vi.fn();
  vi.stubGlobal("__TAURI__", { core: { invoke: invokeMock } });
});

describe("VR product-code identity", () => {
  it("canonicalizes ASCII case, supported separators, and harmless numeric padding", () => {
    for (const value of [
      "MDVR-419",
      "mdvr-419",
      "MdVr_00419",
      "MDVR 0419",
      "MDVR419",
    ]) {
      expect(canonicalizeProductCode(value)).toBe("MDVR-419");
    }
  });

  it("rejects missing, malformed, and zero product codes", () => {
    for (const value of ["", "   ", "MDVR", "419", "MDVR-0", "MDVR-41A"])
      expect(canonicalizeProductCode(value)).toBeNull();
  });
});

describe("parsed VR Library identity", () => {
  it("groups only equivalent exact codes and preserves every exact file identity", async () => {
    const firstPath = "/VR/作品/MDVR-419  Disc 01 — 前編.mp4";
    const secondPath = "/VR/mdvr_00419_CD2  特別版.MKV";
    const mixedPath = "/VR/MDVR-419 + ABC-123  pack.mp4";
    invokeMock.mockResolvedValue([
      firstPath,
      "10",
      secondPath,
      "20",
      "/VR/MDVR-422.mp4",
      "30",
      "/VR/MDVR-430.mp4",
      "40",
      "/VR/MDVR-433.mp4",
      "50",
      "/VR/MDVR-374.mp4",
      "60",
      "/VR/MDVR-4190.mp4",
      "70",
      "/VR/XMDVR-419.mp4",
      "80",
      mixedPath,
      "90",
    ]);
    const items = await scanVrLibrary();

    const mdvr419 = items.find((item) => item.code === "MDVR-419");
    expect(mdvr419).toEqual({
      id: "code:MDVR-419",
      title: "MDVR-419",
      code: "MDVR-419",
      files: [
        {
          path: firstPath,
          filename: "MDVR-419  Disc 01 — 前編.mp4",
          title: "MDVR-419  Disc 01 — 前編",
          sizeBytes: "10",
          partLabel: "Disc 01",
        },
        {
          path: secondPath,
          filename: "mdvr_00419_CD2  特別版.MKV",
          title: "mdvr_00419_CD2  特別版",
          sizeBytes: "20",
          partLabel: "CD2",
        },
      ],
    });
    expect(mdvr419?.files).toHaveLength(2);
    for (const protectedCode of [
      "MDVR-422",
      "MDVR-430",
      "MDVR-433",
      "MDVR-374",
      "MDVR-4190",
      "XMDVR-419",
    ]) {
      expect(items.find((item) => item.code === protectedCode)?.files).toHaveLength(1);
    }
    expect(items.find((item) => item.id === `file:${mixedPath}`)).toMatchObject({
      title: "MDVR-419 + ABC-123  pack",
      code: null,
    });
  });

  it("rejects malformed native rows and duplicate paths", async () => {
    invokeMock.mockResolvedValueOnce(["/VR/MDVR-419.mp4"]);
    await expect(scanVrLibrary()).rejects.toThrow("invalid data");
    invokeMock.mockResolvedValueOnce([
        "/VR/MDVR-419.mp4",
        "1",
        "/VR/MDVR-419.mp4",
        "1",
      ]);
    await expect(scanVrLibrary()).rejects.toThrow("invalid data");
  });

  it("records only unambiguous multipart labels without inventing missing members", async () => {
    invokeMock.mockResolvedValue([
      "/VR/Folder A/MDVR-777 Part 01.mp4",
      "1",
      "/VR/Folder C/MDVR-777 Part 03.mkv",
      "3",
      "/VR/Folder D/MDVR-777 Part 01 Disc 02.mp4",
      "4",
    ]);

    const items = await scanVrLibrary();

    expect(items).toHaveLength(1);
    expect(items[0].files.map((file) => [file.path, file.partLabel])).toEqual([
      ["/VR/Folder A/MDVR-777 Part 01.mp4", "Part 01"],
      ["/VR/Folder C/MDVR-777 Part 03.mkv", "Part 03"],
      ["/VR/Folder D/MDVR-777 Part 01 Disc 02.mp4", null],
    ]);
  });
});

describe("JavDB exact-code catalog request", () => {
  it("accepts only the exact provider identity and preserves provider metadata", async () => {
    invokeMock.mockResolvedValue(catalogFixture);

    await expect(fetchExactJavdbVrItem("MDVR-419")).resolves.toEqual({
      status: "ready",
      item: {
        code: "MDVR-419",
        title: "Provider title",
        coverUrl: "https://images.example/exact.jpg",
        source: "JavDB",
      },
    });
    expect(invokeMock).toHaveBeenCalledWith("fetch_javdb_vr_catalog", {
      code: "MDVR-419",
    });
  });

  it("returns no exact match for neighboring and same-prefix provider codes", async () => {
    invokeMock.mockResolvedValue(`
      <div class="movie-list">
        <div class="item"><div class="video-title"><strong>MDVR-4190</strong> Extension</div></div>
        <div class="item"><div class="video-title"><strong>MDVR-422</strong> Neighbor</div></div>
      </div>
    `);

    await expect(fetchExactJavdbVrItem("MDVR-419")).resolves.toEqual({
      status: "no-exact-match",
    });
  });

  it("distinguishes malformed and native provider failures", async () => {
    invokeMock.mockResolvedValueOnce("<html>blocked</html>");
    await expect(fetchExactJavdbVrItem("MDVR-419")).resolves.toEqual({
      status: "malformed-provider",
    });

    for (const [error, status] of [
      ["vr_source_unavailable", "source-unavailable"],
      ["vr_network_error", "network-error"],
      ["vr_provider_error", "provider-error"],
    ]) {
      invokeMock.mockRejectedValueOnce(error);
      await expect(fetchExactJavdbVrItem("MDVR-419")).resolves.toEqual({
        status,
      });
    }
  });

  it("rejects a non-canonical request before native dispatch", async () => {
    await expect(fetchExactJavdbVrItem("mdvr-419")).rejects.toThrow(
      "A canonical VR product code is required.",
    );
    expect(invokeMock).not.toHaveBeenCalled();
  });
});

describe("Sukebei identity-verified release request", () => {
  it("keeps only valid MDVR-419 representations and accepted comparison fields", async () => {
    invokeMock.mockResolvedValue(
      releaseFeed(
        [
          releaseItem("Exact MDVR-419 release"),
          releaseItem("Case mdvr_00419 release", "8.0 GiB", "4"),
          releaseItem("Compact MDVR419 release", "6.2 GiB", "0"),
          releaseItem("Neighbor MDVR-422 release"),
          releaseItem("Neighbor MDVR-430 release"),
          releaseItem("Neighbor MDVR-433 release"),
          releaseItem("Neighbor MDVR-374 release"),
          releaseItem("Extension MDVR-4190 release"),
          releaseItem("Embedded XMDVR-419 release"),
          releaseItem("Ambiguous MDVR-419 and MDVR-422 release"),
          releaseItem("Ambiguous MDVR-419 + ABC-123 pack"),
          releaseItem("Candidate with no established code"),
        ].join(""),
      ),
    );

    await expect(
      fetchVerifiedSukebeiReleases("MDVR-419"),
    ).resolves.toEqual({
      status: "ready",
      releases: [
        {
          name: "Exact MDVR-419 release",
          source: "Sukebei",
          size: "12.5 GiB",
          seeders: 10,
        },
        {
          name: "Case mdvr_00419 release",
          source: "Sukebei",
          size: "8.0 GiB",
          seeders: 4,
        },
        {
          name: "Compact MDVR419 release",
          source: "Sukebei",
          size: "6.2 GiB",
          seeders: 0,
        },
      ],
    });
    expect(invokeMock).toHaveBeenCalledWith(
      "fetch_sukebei_vr_releases",
      { code: "MDVR-419" },
    );
  });

  it("preserves an accepted Sukebei release name exactly", async () => {
    const exactReleaseName =
      "【VR】 MdVr_00419  Director’s Cut\t—\n特別版!?";
    invokeMock.mockResolvedValue(
      releaseFeed(releaseItem(exactReleaseName)),
    );

    await expect(
      fetchVerifiedSukebeiReleases("MDVR-419"),
    ).resolves.toEqual({
      status: "ready",
      releases: [
        {
          name: exactReleaseName,
          source: "Sukebei",
          size: "12.5 GiB",
          seeders: 10,
        },
      ],
    });
  });

  it("rejects a whitespace-only Sukebei release name", async () => {
    invokeMock.mockResolvedValue(
      releaseFeed("<item><title> \n\t </title></item>"),
    );

    await expect(
      fetchVerifiedSukebeiReleases("MDVR-419"),
    ).resolves.toEqual({ status: "malformed-provider" });
  });

  it("returns an accepted-only empty result without using raw candidates as matches", async () => {
    invokeMock.mockResolvedValue(
      releaseFeed(
        releaseItem("MDVR-4190 extension") +
          releaseItem("XMDVR-419 embedded") +
          releaseItem("No product identity"),
      ),
    );

    await expect(
      fetchVerifiedSukebeiReleases("MDVR-419"),
    ).resolves.toEqual({ status: "ready", releases: [] });
  });

  it("maps unavailable optional comparison values honestly", async () => {
    invokeMock.mockResolvedValue(
      releaseFeed("<item><title>MDVR-419 exact</title></item>"),
    );

    await expect(
      fetchVerifiedSukebeiReleases("MDVR-419"),
    ).resolves.toEqual({
      status: "ready",
      releases: [
        {
          name: "MDVR-419 exact",
          source: "Sukebei",
          size: null,
          seeders: null,
        },
      ],
    });
  });

  it("retains a complete same-item artifact identity only on its verified release", async () => {
    invokeMock.mockResolvedValue(
      releaseFeed(
        `<item><title>MDVR-419 complete artifact</title>
          ${releaseArtifactElements("123", "ABCDEF0123456789ABCDEF0123456789ABCDEF01")}
        </item>
        <item><title>MDVR-419 mismatched artifact</title>
          ${releaseArtifactElements("124", undefined, "https://sukebei.nyaa.si/download/125.torrent")}
        </item>
        <item><title>MDVR-419 unsafe artifact</title>
          ${releaseArtifactElements("126", undefined, "https://user@sukebei.nyaa.si/download/126.torrent")}
        </item>
        <item><title>MDVR-419 missing artifact</title></item>`,
      ),
    );

    await expect(
      fetchVerifiedSukebeiReleases("MDVR-419"),
    ).resolves.toEqual({
      status: "ready",
      releases: [
        {
          artifact: {
            expectedInfohash: "abcdef0123456789abcdef0123456789abcdef01",
            providerItemId: "123",
            torrentUrl: "https://sukebei.nyaa.si/download/123.torrent",
          },
          name: "MDVR-419 complete artifact",
          source: "Sukebei",
          size: null,
          seeders: null,
        },
        {
          name: "MDVR-419 mismatched artifact",
          source: "Sukebei",
          size: null,
          seeders: null,
        },
        {
          name: "MDVR-419 unsafe artifact",
          source: "Sukebei",
          size: null,
          seeders: null,
        },
        {
          name: "MDVR-419 missing artifact",
          source: "Sukebei",
          size: null,
          seeders: null,
        },
      ],
    });
  });

  it("distinguishes malformed and native provider failures", async () => {
    invokeMock.mockResolvedValueOnce("<rss>");
    await expect(
      fetchVerifiedSukebeiReleases("MDVR-419"),
    ).resolves.toEqual({ status: "malformed-provider" });

    for (const [error, status] of [
      ["vr_source_unavailable", "source-unavailable"],
      ["vr_network_error", "network-error"],
      ["vr_provider_error", "provider-error"],
    ]) {
      invokeMock.mockRejectedValueOnce(error);
      await expect(
        fetchVerifiedSukebeiReleases("MDVR-419"),
      ).resolves.toEqual({ status });
    }
  });
});

describe("verified Sukebei torrent inspection", () => {
  const release = {
    artifact: {
      expectedInfohash: "0123456789abcdef0123456789abcdef01234567",
      providerItemId: "123",
      torrentUrl: "https://sukebei.nyaa.si/download/123.torrent",
    },
    name: "【VR】 MDVR-419  Exact — 特別版",
    seeders: 4,
    size: "8.0 GiB",
    source: "Sukebei" as const,
  };

  it("accepts exact verified metadata and the complete file list", async () => {
    invokeMock.mockResolvedValue([
      "inspection-123",
      "VR  — 作品",
      release.artifact.expectedInfohash,
      "12",
      "Folder/Part  1 — 映画.mkv",
      "5",
      "Folder/特別版  B.mp4",
      "7",
    ]);

    await expect(
      inspectVerifiedSukebeiTorrent("MDVR-419", release),
    ).resolves.toEqual({
      status: "ready",
      inspection: {
        displayName: "VR  — 作品",
        files: [
          { path: "Folder/Part  1 — 映画.mkv", sizeBytes: "5" },
          { path: "Folder/特別版  B.mp4", sizeBytes: "7" },
        ],
        infohash: release.artifact.expectedInfohash,
        inspectionId: "inspection-123",
        totalBytes: "12",
      },
    });
    expect(invokeMock).toHaveBeenCalledWith("inspect_sukebei_vr_torrent", {
      code: "MDVR-419",
      expectedInfohash: release.artifact.expectedInfohash,
      providerItemId: "123",
      releaseName: release.name,
      torrentUrl: release.artifact.torrentUrl,
    });
  });

  it("distinguishes every native inspection failure", async () => {
    for (const [error, status] of [
      ["vr_torrent_source_unavailable", "source-unavailable"],
      ["vr_torrent_network_error", "network-error"],
      ["vr_torrent_provider_error", "provider-error"],
      ["vr_torrent_malformed", "malformed-torrent"],
      ["vr_torrent_unsupported", "unsupported-torrent"],
      ["vr_torrent_infohash_mismatch", "infohash-mismatch"],
    ]) {
      invokeMock.mockRejectedValueOnce(error);
      await expect(
        inspectVerifiedSukebeiTorrent("MDVR-419", release),
      ).resolves.toEqual({ status });
    }
  });

  it("rejects malformed native metadata and an unavailable release before use", async () => {
    invokeMock.mockResolvedValue([
      "inspection-123",
      "Torrent",
      release.artifact.expectedInfohash,
      "12",
      "Only file",
      "7",
    ]);
    await expect(
      inspectVerifiedSukebeiTorrent("MDVR-419", release),
    ).resolves.toEqual({ status: "malformed-torrent" });

    invokeMock.mockResolvedValue([
      "inspection-123",
      "Torrent",
      "0000000000000000000000000000000000000001",
      "7",
      "Only file",
      "7",
    ]);
    await expect(
      inspectVerifiedSukebeiTorrent("MDVR-419", release),
    ).resolves.toEqual({ status: "infohash-mismatch" });

    await expect(
      inspectVerifiedSukebeiTorrent("MDVR-419", {
        name: "MDVR-419 unavailable",
        seeders: null,
        size: null,
        source: "Sukebei",
      }),
    ).resolves.toEqual({ status: "malformed-torrent" });
    expect(invokeMock).toHaveBeenCalledTimes(2);
  });
});

describe("trusted VR download boundary", () => {
  it("parses exact persisted identities and selected-file progress", async () => {
    const exactReleaseName = "【VR】 MDVR-419  Exact\t—\n特別版";
    invokeMock.mockResolvedValue([
      "transfer-123",
      "MDVR-419",
      exactReleaseName,
      "2",
      "12",
      "7",
      "1024",
      "paused",
      "true",
    ]);

    await expect(loadVrDownloads()).resolves.toEqual([
      {
        transferId: "transfer-123",
        code: "MDVR-419",
        releaseName: exactReleaseName,
        selectedFileCount: 2,
        totalBytes: "12",
        downloadedBytes: "7",
        speedBytesPerSecond: "1024",
        state: "paused",
        isCurrentFolder: true,
      },
    ]);
    expect(invokeMock).toHaveBeenCalledWith("load_vr_downloads");
  });

  it("starts with only the current inspection and selected file IDs", async () => {
    invokeMock.mockResolvedValue("transfer-123");

    await expect(
      startVerifiedVrDownload("inspection-123", [0, 2]),
    ).resolves.toBe("transfer-123");
    expect(invokeMock).toHaveBeenCalledWith("start_verified_vr_download", {
      inspectionId: "inspection-123",
      selectedFileIds: [0, 2],
    });

    invokeMock.mockClear();
    await expect(
      startVerifiedVrDownload("inspection-123", [2, 2]),
    ).rejects.toThrow("valid file selection");
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("distinguishes configured, unavailable, and malformed VR folder state", async () => {
    invokeMock.mockResolvedValueOnce(["ready", "/Volumes/VR — 作品"]);
    await expect(loadVrFolder()).resolves.toEqual({
      status: "ready",
      path: "/Volumes/VR — 作品",
    });

    invokeMock.mockResolvedValueOnce(["unavailable", "/missing/VR"]);
    await expect(loadVrFolder()).resolves.toEqual({
      status: "unavailable",
      path: "/missing/VR",
    });

    invokeMock.mockResolvedValueOnce(["ready", ""]);
    await expect(loadVrFolder()).rejects.toThrow("invalid data");
  });
});
