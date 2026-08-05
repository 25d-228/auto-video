import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  applyVrOrganization,
  canonicalizeProductCode,
  dismissVrOrganization,
  fetchExactJavdbAdultItem,
  fetchExactJavdbVrItem,
  fetchVerifiedAdultSukebeiReleases,
  fetchVerifiedSukebeiReleases,
  inspectVerifiedAdultSukebeiTorrent,
  inspectVerifiedSukebeiTorrent,
  invalidateVerifiedAdultTorrent,
  loadVrDownloadLimit,
  loadVrDownloads,
  loadVrFolder,
  previewVrOrganization,
  scanVrLibrary,
  saveVrDownloadLimit,
  saveVerifiedAdultTorrent,
  startVerifiedAdultDownload,
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

describe("Adult exact-code provider boundaries", () => {
  it("accepts only the exact ADLT-123 JavDB identity and preserves provider metadata", async () => {
    invokeMock.mockResolvedValue(`
      <div class="movie-list">
        <div class="item"><div class="video-title"><strong>ADLT-124</strong> Neighbor</div></div>
        <div class="item"><div class="video-title"><strong>ADLT-1230</strong> Extension</div></div>
        <div class="item"><div class="video-title"><strong>XADLT-123</strong> Embedded</div></div>
        <div class="item"><div class="video-title"><strong>ADLT-123 + XYZ-7</strong> Mixed</div></div>
        <div class="item"><a class="box" href="/v/exact">
          <img data-src="https://images.example/adult.jpg">
          <div class="video-title"><strong>adlt_00123</strong> 作品  —  Exact  Title!</div>
        </a></div>
      </div>
    `);

    await expect(fetchExactJavdbAdultItem("ADLT-123")).resolves.toEqual({
      status: "ready",
      item: {
        code: "ADLT-123",
        title: "作品  —  Exact  Title!",
        coverUrl: "https://images.example/adult.jpg",
        source: "JavDB",
      },
    });
    expect(invokeMock).toHaveBeenCalledWith("fetch_javdb_adult_catalog", {
      code: "ADLT-123",
    });
  });

  it("rejects every required false-positive Adult catalog identity", async () => {
    invokeMock.mockResolvedValue(`
      <div class="movie-list">
        <div class="item"><div class="video-title"><strong>ADLT-124</strong> Neighbor</div></div>
        <div class="item"><div class="video-title"><strong>ADLT-1230</strong> Extension</div></div>
        <div class="item"><div class="video-title"><strong>XADLT-123</strong> Embedded</div></div>
        <div class="item"><div class="video-title"><strong>ADLT-123 + XYZ-7</strong> Mixed</div></div>
        <div class="item"><div class="video-title"><strong>ADLT-125</strong> Same prefix</div></div>
        <div class="item"><div class="video-title"><strong>Unknown</strong> No code</div></div>
      </div>
    `);

    await expect(fetchExactJavdbAdultItem("ADLT-123")).resolves.toEqual({
      status: "no-exact-match",
    });
  });

  it("keeps only unambiguous ADLT-123 releases and retains a complete same-item artifact", async () => {
    const exactName = "【作品】 adlt_00123  Director’s Cut\t—\n特別版!?";
    invokeMock.mockResolvedValue(
      releaseFeed(
        [
          `<item><title>${exactName}</title>
            <nyaa:size>7.5 GiB</nyaa:size><nyaa:seeders>12</nyaa:seeders>
            ${releaseArtifactElements("321")}
          </item>`,
          releaseItem("ADLT-124 neighbor"),
          releaseItem("ADLT-1230 extension"),
          releaseItem("XADLT-123 embedded"),
          releaseItem("ADLT-123 + XYZ-7 mixed"),
          releaseItem("ADLT-125 same prefix"),
          releaseItem("Candidate with no established code"),
        ].join(""),
      ),
    );

    await expect(
      fetchVerifiedAdultSukebeiReleases("ADLT-123"),
    ).resolves.toEqual({
      status: "ready",
      releases: [
        {
          artifact: {
            expectedInfohash: "0123456789abcdef0123456789abcdef01234567",
            providerItemId: "321",
            torrentUrl: "https://sukebei.nyaa.si/download/321.torrent",
          },
          name: exactName,
          source: "Sukebei",
          size: "7.5 GiB",
          seeders: 12,
        },
      ],
    });
    expect(invokeMock).toHaveBeenCalledWith("fetch_sukebei_adult_releases", {
      code: "ADLT-123",
    });
  });

  it("keeps incomplete exact Adult releases metadata-only", async () => {
    const validHash = "0123456789abcdef0123456789abcdef01234567";
    invokeMock.mockResolvedValue(
      releaseFeed(
        [
          `<item><title>ADLT-123 complete</title>${releaseArtifactElements("321", validHash)}</item>`,
          releaseItem("ADLT-123 missing artifact"),
          `<item><title>ADLT-123 mismatched item</title>${releaseArtifactElements("322", validHash, "https://sukebei.nyaa.si/download/323.torrent")}</item>`,
          `<item><title>ADLT-123 credentials</title>${releaseArtifactElements("324", validHash, "https://user@sukebei.nyaa.si/download/324.torrent")}</item>`,
          `<item><title>ADLT-123 query</title>${releaseArtifactElements("325", validHash, "https://sukebei.nyaa.si/download/325.torrent?alternate=1")}</item>`,
          `<item><title>ADLT-123 invalid hash</title>${releaseArtifactElements("326", "invalid")}</item>`,
        ].join(""),
      ),
    );

    const result = await fetchVerifiedAdultSukebeiReleases("ADLT-123");
    expect(result.status).toBe("ready");
    if (result.status !== "ready") return;
    expect(result.releases).toHaveLength(6);
    expect(result.releases[0].artifact).toEqual({
      expectedInfohash: validHash,
      providerItemId: "321",
      torrentUrl: "https://sukebei.nyaa.si/download/321.torrent",
    });
    for (const release of result.releases.slice(1)) {
      expect(release.artifact).toBeUndefined();
    }
  });

  it("returns a safe empty Adult result for unverified candidates", async () => {
    invokeMock.mockResolvedValue(
      releaseFeed(
        releaseItem("ADLT-1230 extension") +
          releaseItem("ADLT-123 + XYZ-7 mixed") +
          releaseItem("No product identity"),
      ),
    );

    await expect(
      fetchVerifiedAdultSukebeiReleases("ADLT-123"),
    ).resolves.toEqual({ status: "ready", releases: [] });
  });

  it("distinguishes Adult release provider failures and rejects noncanonical requests", async () => {
    invokeMock.mockResolvedValueOnce("<rss>");
    await expect(
      fetchVerifiedAdultSukebeiReleases("ADLT-123"),
    ).resolves.toEqual({ status: "malformed-provider" });

    for (const [error, status] of [
      ["adult_source_unavailable", "source-unavailable"],
      ["adult_network_error", "network-error"],
      ["adult_provider_error", "provider-error"],
    ]) {
      invokeMock.mockRejectedValueOnce(error);
      await expect(
        fetchVerifiedAdultSukebeiReleases("ADLT-123"),
      ).resolves.toEqual({ status });
    }

    await expect(
      fetchVerifiedAdultSukebeiReleases("adlt-123"),
    ).rejects.toThrow("A canonical Adult product code is required.");
  });
});

describe("verified Adult Sukebei torrent inspection", () => {
  const release = {
    artifact: {
      expectedInfohash: "0123456789abcdef0123456789abcdef01234567",
      providerItemId: "321",
      torrentUrl: "https://sukebei.nyaa.si/download/321.torrent",
    },
    name: "【Adult】 ADLT-123  Exact\t—\n特別版!?",
    seeders: 4,
    size: "8.0 GiB",
    source: "Sukebei" as const,
  };

  it("uses only the Adult inspection and save commands with exact identity", async () => {
    invokeMock.mockResolvedValueOnce([
      "adult-1-1-321",
      "作品  —  Exact",
      release.artifact.expectedInfohash,
      "12",
      "Folder/Part  1 — 映画.mkv",
      "5",
      "Folder/特別版  B.mp4",
      "7",
    ]);

    await expect(
      inspectVerifiedAdultSukebeiTorrent("ADLT-123", release),
    ).resolves.toMatchObject({
      status: "ready",
      inspection: {
        displayName: "作品  —  Exact",
        inspectionId: "adult-1-1-321",
        totalBytes: "12",
      },
    });
    expect(invokeMock).toHaveBeenCalledWith("inspect_sukebei_adult_torrent", {
      code: "ADLT-123",
      expectedInfohash: release.artifact.expectedInfohash,
      providerItemId: "321",
      releaseName: release.name,
      torrentUrl: release.artifact.torrentUrl,
    });

    invokeMock.mockResolvedValueOnce(false).mockResolvedValueOnce(true);
    await expect(saveVerifiedAdultTorrent("adult-1-1-321")).resolves.toBe(false);
    await expect(saveVerifiedAdultTorrent("adult-1-1-321")).resolves.toBe(true);
    expect(invokeMock).toHaveBeenLastCalledWith("save_verified_adult_torrent", {
      inspectionId: "adult-1-1-321",
    });
    invokeMock.mockResolvedValueOnce(undefined);
    await expect(invalidateVerifiedAdultTorrent()).resolves.toBeUndefined();
    expect(invokeMock).toHaveBeenLastCalledWith(
      "invalidate_verified_adult_torrent",
    );
    expect(
      invokeMock.mock.calls.some(([command]) =>
        [
          "inspect_sukebei_vr_torrent",
          "save_verified_vr_torrent",
          "start_verified_vr_download",
        ].includes(command),
      ),
    ).toBe(false);
  });

  it("distinguishes Adult failures and rejects metadata-only releases", async () => {
    for (const [error, status] of [
      ["adult_torrent_source_unavailable", "source-unavailable"],
      ["adult_torrent_network_error", "network-error"],
      ["adult_torrent_provider_error", "provider-error"],
      ["adult_torrent_malformed", "malformed-torrent"],
      ["adult_torrent_unsupported", "unsupported-torrent"],
      ["adult_torrent_infohash_mismatch", "infohash-mismatch"],
      ["adult_torrent_context_invalid", "stale-context"],
      ["adult_torrent_stale", "stale-context"],
      ["unexpected", "inspection-error"],
    ]) {
      invokeMock.mockRejectedValueOnce(error);
      await expect(
        inspectVerifiedAdultSukebeiTorrent("ADLT-123", release),
      ).resolves.toEqual({ status });
    }

    await expect(
      inspectVerifiedAdultSukebeiTorrent("ADLT-123", {
        name: "ADLT-123 metadata only",
        seeders: null,
        size: null,
        source: "Sukebei",
      }),
    ).resolves.toEqual({ status: "malformed-torrent" });
    expect(invokeMock).toHaveBeenCalledTimes(9);
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
  it("loads and saves explicit aggregate download-limit states", async () => {
    invokeMock
      .mockResolvedValueOnce(["unlimited"])
      .mockResolvedValueOnce(["limited", "8"])
      .mockResolvedValueOnce(["unlimited"]);

    await expect(loadVrDownloadLimit()).resolves.toEqual({
      mibPerSecond: null,
    });
    await expect(saveVrDownloadLimit("8")).resolves.toEqual({
      mibPerSecond: "8",
    });
    expect(invokeMock).toHaveBeenNthCalledWith(2, "save_vr_download_limit", {
      mibPerSecond: "8",
    });
    await expect(saveVrDownloadLimit(null)).resolves.toEqual({
      mibPerSecond: null,
    });
    expect(invokeMock).toHaveBeenNthCalledWith(3, "save_vr_download_limit", {
      mibPerSecond: null,
    });
  });

  it("rejects invalid aggregate limits and malformed native responses", async () => {
    for (const invalid of ["", "0", "01", "-1", "+1", "1.5", "4096"]) {
      await expect(saveVrDownloadLimit(invalid)).rejects.toThrow(
        "whole-number download limit",
      );
    }
    expect(invokeMock).not.toHaveBeenCalled();

    for (const malformed of [
      [],
      ["limited"],
      ["limited", "0"],
      ["limited", "4096"],
      ["unlimited", "1"],
    ]) {
      invokeMock.mockResolvedValueOnce(malformed);
      await expect(loadVrDownloadLimit()).rejects.toThrow("invalid data");
    }
  });

  it("parses exact persisted identities and selected-file progress", async () => {
    const exactReleaseName = "【VR】 MDVR-419  Exact\t—\n特別版";
    invokeMock.mockResolvedValue([
      "transfer-123",
      "vr",
      "MDVR-419",
      exactReleaseName,
      "2",
      "12",
      "7",
      "1024",
      "paused",
      "true",
      "none",
      "",
      "false",
      "adult-transfer-123",
      "adult",
      "ADLT-123",
      "【Adult】 ADLT-123  Exact  —  特別版",
      "1",
      "7",
      "7",
      "0",
      "completed",
      "true",
      "none",
      "",
      "false",
      "movie-transfer-419",
      "movie",
      "tt0123456",
      "Exact  Movie — 特別版",
      "1",
      "7",
      "3",
      "512",
      "downloading",
      "true",
      "none",
      "",
      "false",
    ]);

    await expect(loadVrDownloads()).resolves.toEqual([
      {
        transferId: "transfer-123",
        category: "vr",
        identity: "MDVR-419",
        releaseName: exactReleaseName,
        selectedFileCount: 2,
        totalBytes: "12",
        downloadedBytes: "7",
        speedBytesPerSecond: "1024",
        state: "paused",
        isCurrentFolder: true,
        organizationStatus: "none",
        organizationRelativeDirectory: null,
        canOrganize: false,
      },
      {
        transferId: "adult-transfer-123",
        category: "adult",
        identity: "ADLT-123",
        releaseName: "【Adult】 ADLT-123  Exact  —  特別版",
        selectedFileCount: 1,
        totalBytes: "7",
        downloadedBytes: "7",
        speedBytesPerSecond: "0",
        state: "completed",
        isCurrentFolder: true,
        organizationStatus: "none",
        organizationRelativeDirectory: null,
        canOrganize: false,
      },
      {
        transferId: "movie-transfer-419",
        category: "movie",
        identity: "tt0123456",
        releaseName: "Exact  Movie — 特別版",
        selectedFileCount: 1,
        totalBytes: "7",
        downloadedBytes: "3",
        speedBytesPerSecond: "512",
        state: "downloading",
        isCurrentFolder: true,
        organizationStatus: "none",
        organizationRelativeDirectory: null,
        canOrganize: false,
      },
    ]);
    expect(invokeMock).toHaveBeenCalledWith("load_vr_downloads");
  });

  it("rejects Movie rows with fabricated identity or organization state", async () => {
    const validMovieRow = [
      "movie-transfer-419",
      "movie",
      "tt0123456",
      "Exact Movie",
      "1",
      "7",
      "3",
      "512",
      "downloading",
      "true",
      "none",
      "",
      "false",
    ];
    for (const [fieldIndex, invalidValue] of [
      [2, "MDVR-419"],
      [10, "organized"],
      [11, "tt0123456/"],
      [12, "true"],
    ] as const) {
      const malformed = [...validMovieRow];
      malformed[fieldIndex] = invalidValue;
      invokeMock.mockResolvedValueOnce(malformed);
      await expect(loadVrDownloads()).rejects.toThrow("invalid data");
    }
  });

  it("accepts native-eligible and recoverable Adult organization rows", async () => {
    invokeMock.mockResolvedValue([
      "adult-transfer-123",
      "adult",
      "ADLT-123",
      "ADLT-123 exact",
      "1",
      "7",
      "7",
      "0",
      "completed",
      "true",
      "none",
      "",
      "true",
      "adult-recovery-123",
      "adult",
      "ADLT-123",
      "ADLT-123 recoverable",
      "2",
      "14",
      "14",
      "0",
      "completed",
      "true",
      "attention",
      "ADLT-123/",
      "true",
    ]);

    await expect(loadVrDownloads()).resolves.toEqual([
      expect.objectContaining({
        transferId: "adult-transfer-123",
        category: "adult",
        organizationStatus: "none",
        canOrganize: true,
      }),
      expect.objectContaining({
        transferId: "adult-recovery-123",
        category: "adult",
        organizationStatus: "attention",
        organizationRelativeDirectory: "ADLT-123/",
        canOrganize: true,
      }),
    ]);
  });

  it("accepts category-unknown persisted transfers only as inert offline rows", async () => {
    invokeMock.mockResolvedValue([
      "corrupt-1",
      "unknown",
      "ADLT-123",
      "【Adult】 ADLT-123 damaged V2 record",
      "0",
      "0",
      "0",
      "0",
      "offline",
      "false",
      "none",
      "",
      "false",
    ]);

    await expect(loadVrDownloads()).resolves.toEqual([
      {
        transferId: "corrupt-1",
        category: "unknown",
        identity: "ADLT-123",
        releaseName: "【Adult】 ADLT-123 damaged V2 record",
        selectedFileCount: 0,
        totalBytes: "0",
        downloadedBytes: "0",
        speedBytesPerSecond: "0",
        state: "offline",
        isCurrentFolder: false,
        organizationStatus: "none",
        organizationRelativeDirectory: null,
        canOrganize: false,
      },
    ]);

    for (const [fieldIndex, invalidValue] of [
      [8, "paused"],
      [10, "attention"],
      [12, "true"],
    ] as const) {
      const invalidRow = [
        "corrupt-1",
        "unknown",
        "ADLT-123",
        "【Adult】 ADLT-123 damaged V2 record",
        "0",
        "0",
        "0",
        "0",
        "offline",
        "false",
        "none",
        "",
        "false",
      ];
      invalidRow[fieldIndex] = invalidValue;
      invokeMock.mockResolvedValueOnce(invalidRow);
      await expect(loadVrDownloads()).rejects.toThrow("invalid data");
    }
  });

  it("parses exact native organization previews and applies only the plan identity", async () => {
    invokeMock
      .mockResolvedValueOnce([
        "plan-123",
        "transfer-123",
        "MDVR-419",
        "1",
        "2",
        "move",
        "Source/MDVR-419  —  映画.MKV",
        "MDVR-419/MDVR-419.MKV",
        "non-media-unchanged",
        "Source/notes  —  exact.txt",
        "",
      ])
      .mockResolvedValueOnce(undefined);

    await expect(previewVrOrganization("transfer-123")).resolves.toEqual({
      planId: "plan-123",
      transferId: "transfer-123",
      code: "MDVR-419",
      moveCount: 1,
      entries: [
        {
          kind: "move",
          sourceRelativePath: "Source/MDVR-419  —  映画.MKV",
          destinationRelativePath: "MDVR-419/MDVR-419.MKV",
        },
        {
          kind: "non-media-unchanged",
          sourceRelativePath: "Source/notes  —  exact.txt",
          destinationRelativePath: null,
        },
      ],
    });
    expect(invokeMock).toHaveBeenNthCalledWith(1, "preview_vr_organization", {
      transferId: "transfer-123",
    });
    await expect(applyVrOrganization("plan-123")).resolves.toBeUndefined();
    expect(invokeMock).toHaveBeenNthCalledWith(2, "apply_vr_organization", {
      planId: "plan-123",
    });
    invokeMock.mockResolvedValueOnce(undefined);
    await expect(dismissVrOrganization()).resolves.toBeUndefined();
    expect(invokeMock).toHaveBeenNthCalledWith(3, "dismiss_vr_organization");
  });

  it("preserves compact Adult multipart basenames in native previews", async () => {
    invokeMock.mockResolvedValueOnce([
      "adult-plan-123",
      "adult-transfer-123",
      "ADLT-123",
      "2",
      "2",
      "move",
      "Source/ADLT-123 Part 1-2.mp4",
      "ADLT-123/ADLT-123 Part 1-2.mp4",
      "move",
      "Source/ADLT-123 CD1+2.mkv",
      "ADLT-123/ADLT-123 CD1+2.mkv",
    ]);

    await expect(previewVrOrganization("adult-transfer-123")).resolves.toEqual({
      planId: "adult-plan-123",
      transferId: "adult-transfer-123",
      code: "ADLT-123",
      moveCount: 2,
      entries: [
        {
          kind: "move",
          sourceRelativePath: "Source/ADLT-123 Part 1-2.mp4",
          destinationRelativePath: "ADLT-123/ADLT-123 Part 1-2.mp4",
        },
        {
          kind: "move",
          sourceRelativePath: "Source/ADLT-123 CD1+2.mkv",
          destinationRelativePath: "ADLT-123/ADLT-123 CD1+2.mkv",
        },
      ],
    });
  });

  it("rejects malformed organization identities and paths at the interface boundary", async () => {
    await expect(previewVrOrganization("")).rejects.toThrow("transfer identity");
    expect(() => applyVrOrganization("")).toThrow("current organization plan");
    expect(invokeMock).not.toHaveBeenCalled();

    for (const malformed of [
      [],
      [
        "plan",
        "transfer",
        "MDVR-419",
        "1",
        "1",
        "move",
        "../source.mp4",
        "MDVR-419/file.mp4",
      ],
      [
        "plan",
        "transfer",
        "MDVR-419",
        "2",
        "1",
        "move",
        "source.mp4",
        "MDVR-419/file.mp4",
      ],
      [
        "plan",
        "transfer",
        "ABC-123",
        "0",
        "1",
        "non-media-unchanged",
        "notes.txt",
        "ABC-123/notes.txt",
      ],
      [
        "plan",
        "transfer",
        "MDVR-419",
        "0",
        "1",
        "media-unchanged",
        "source.mp4",
        "outside/file.mp4",
      ],
    ]) {
      invokeMock.mockResolvedValueOnce(malformed);
      await expect(previewVrOrganization("transfer")).rejects.toThrow(
        "invalid data",
      );
    }
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

    invokeMock.mockResolvedValue("adult-transfer-123");
    await expect(
      startVerifiedAdultDownload("adult-inspection-123", [1]),
    ).resolves.toBe("adult-transfer-123");
    expect(invokeMock).toHaveBeenCalledWith("start_verified_adult_download", {
      inspectionId: "adult-inspection-123",
      selectedFileIds: [1],
    });
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
