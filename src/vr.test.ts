import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  applyVrOrganization,
  canonicalizeProductCode,
  cleanupCancelledVrDownload,
  dismissVrOrganization,
  fetchExactJavdbAdultItem,
  fetchExactJavdbVrItem,
  fetchJavdbBrowse,
  fetchJavdbCoverObjectUrl,
  fetchJavdbDetail,
  fetchJavdbDetailImageObjectUrl,
  fetchVerifiedAdultSukebeiReleases,
  fetchVerifiedSukebeiReleases,
  inspectVerifiedAdultSukebeiTorrent,
  inspectVerifiedSukebeiTorrent,
  invalidateVerifiedAdultTorrent,
  invalidateJavdbDetail,
  loadVrDownloadLimit,
  loadVrDownloads,
  loadVrFolder,
  openJavdbDetailSource,
  parseJavdbBrowseResponse,
  parseJavdbDetailResponse,
  productCodeCandidates,
  previewVrOrganization,
  queryVrStorage,
  scanVrLibrary,
  saveVrDownloadLimit,
  saveVerifiedAdultTorrent,
  startVerifiedAdultDownload,
  startVerifiedVrDownload,
  trashVrFile,
} from "./vr";
import type {
  JavdbBrowseItem,
  JavdbBrowseRequest,
  JavdbDetail,
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

afterEach(() => {
  vi.unstubAllGlobals();
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
    expect(canonicalizeProductCode("13dsvr_01947")).toBe("13DSVR-1947");
    expect(canonicalizeProductCode("3DSVR-1947")).toBe("3DSVR-1947");
    expect(productCodeCandidates("Exact 3DSVR1947 release")).toEqual([
      { code: "3DSVR-1947", prefix: "3DSVR" },
    ]);
    expect(productCodeCandidates("MDVR419 1080p")).toEqual([
      { code: "MDVR-419", prefix: "MDVR" },
    ]);
    expect(productCodeCandidates("3DSVR1947 1080p")).toEqual([
      { code: "3DSVR-1947", prefix: "3DSVR" },
    ]);
    expect(productCodeCandidates("MDVR419 2024")).toEqual([
      { code: "MDVR-419", prefix: "MDVR" },
    ]);
    expect(productCodeCandidates("3DSVR1947 2025")).toEqual([
      { code: "3DSVR-1947", prefix: "3DSVR" },
    ]);
  });

  it("rejects missing, malformed, and zero product codes", () => {
    for (const value of [
      "",
      "   ",
      "MDVR",
      "419",
      "MDVR-0",
      "MDVR-41A",
      "AB1-2",
    ])
      expect(canonicalizeProductCode(value)).toBeNull();
    expect(productCodeCandidates("AB1-2 unsupported")).toEqual([]);
    expect(productCodeCandidates("MDVR419 420")).toEqual([]);
    expect(productCodeCandidates("MDVR419 + ABC123 pack")).toEqual([
      { code: "MDVR-419", prefix: "MDVR" },
      { code: "ABC-123", prefix: "ABC" },
    ]);
  });
});

describe("parsed VR Library identity", () => {
  it("groups only equivalent exact codes and preserves every exact file identity", async () => {
    const firstPath = "/VR/作品/MDVR-419  Disc 01 — 前編.mp4";
    const secondPath = "/VR/mdvr_00419_CD2  特別版.MKV";
    const mixedPath = "/VR/MDVR-419 + ABC-123  pack.mp4";
    invokeMock.mockResolvedValue([
      "7",
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
    const { generation, items } = await scanVrLibrary();

    expect(generation).toBe("7");
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
    invokeMock.mockResolvedValueOnce([]);
    await expect(scanVrLibrary()).rejects.toThrow("invalid data");
    invokeMock.mockResolvedValueOnce(["/VR/MDVR-419.mp4"]);
    await expect(scanVrLibrary()).rejects.toThrow("invalid data");
    invokeMock.mockResolvedValueOnce([
        "1",
        "/VR/MDVR-419.mp4",
        "1",
        "/VR/MDVR-419.mp4",
        "1",
      ]);
    await expect(scanVrLibrary()).rejects.toThrow("invalid data");
  });

  it("records only unambiguous multipart labels without inventing missing members", async () => {
    invokeMock.mockResolvedValue([
      "8",
      "/VR/Folder A/MDVR-777 Part 01.mp4",
      "1",
      "/VR/Folder C/MDVR-777 Part 03.mkv",
      "3",
      "/VR/Folder D/MDVR-777 Part 01 Disc 02.mp4",
      "4",
    ]);

    const { items } = await scanVrLibrary();

    expect(items).toHaveLength(1);
    expect(items[0].files.map((file) => [file.path, file.partLabel])).toEqual([
      ["/VR/Folder A/MDVR-777 Part 01.mp4", "Part 01"],
      ["/VR/Folder C/MDVR-777 Part 03.mkv", "Part 03"],
      ["/VR/Folder D/MDVR-777 Part 01 Disc 02.mp4", null],
    ]);
  });

  it("accepts a generation-only response and dispatches exact Trash authority", async () => {
    invokeMock.mockResolvedValueOnce(["9"]);

    await expect(scanVrLibrary()).resolves.toEqual({
      generation: "9",
      items: [],
    });
    invokeMock.mockResolvedValueOnce(undefined);
    await trashVrFile("/VR/MDVR-419 PT 02.mp4", "9");
    expect(invokeMock).toHaveBeenLastCalledWith("trash_vr_file", {
      path: "/VR/MDVR-419 PT 02.mp4",
      scanGeneration: "9",
    });
  });

  it("validates VR volume storage at the interface boundary", async () => {
    invokeMock.mockResolvedValueOnce(["100", "40"]);
    await expect(queryVrStorage()).resolves.toEqual({
      totalBytes: 100n,
      freeBytes: 40n,
    });
    invokeMock.mockResolvedValueOnce(["0", "0"]);
    await expect(queryVrStorage()).rejects.toThrow("inconsistent");
  });
});

describe("native-owned JavDB browse boundary", () => {
  const request: JavdbBrowseRequest = {
    category: "vr",
    mode: "category",
    period: "daily",
    year: null,
    month: null,
    sort: "newest",
    count: 25,
  };

  it("accepts only one strict structured category generation and item identity", () => {
    expect(
      parseJavdbBrowseResponse(
        [
          "7",
          "2",
          "vr",
          "VrA",
          "MDVR-419",
          "Exact title",
          "2026-08-12",
          "javdb-cover-7-1-0123abcd",
          "1.48",
          "vr",
          "VrB",
          "MDVR-422",
          "",
          "",
          "",
          "1.48",
        ],
        request,
        "3",
      ),
    ).toEqual({
      status: "ready",
      items: [
        {
          category: "vr",
          contextGeneration: "3",
          providerItemId: "VrA",
          requestGeneration: "7",
          code: "MDVR-419",
          title: "Exact title",
          releaseDate: "2026-08-12",
          coverAuthorityId: "javdb-cover-7-1-0123abcd",
          coverUrl: null,
          source: "JavDB",
          sourceAspectRatio: 1.48,
        },
        {
          category: "vr",
          contextGeneration: "3",
          providerItemId: "VrB",
          requestGeneration: "7",
          code: "MDVR-422",
          title: null,
          releaseDate: null,
          coverAuthorityId: null,
          coverUrl: null,
          source: "JavDB",
          sourceAspectRatio: 1.48,
        },
      ],
    });
    for (const response of [
      ["7", "1", "adult", "VrA", "MDVR-419", "", "", "", "1.48"],
      ["7", "1", "vr", "Bad Id", "MDVR-419", "", "", "", "1.48"],
      ["7", "1", "vr", "VrA", "MDVR-0419", "", "", "", "1.48"],
      ["7", "1", "vr", "VrA", "MDVR-419", "", "", "https://tp.evil.com/a.jpg", "1.48"],
    ]) {
      expect(parseJavdbBrowseResponse(response, request, "3")).toEqual({
        status: "malformed-provider",
      });
    }
  });

  it("submits the complete exact browse request and maps native local states", async () => {
    invokeMock.mockResolvedValueOnce(["8", "0"]);
    await expect(fetchJavdbBrowse(request, "1")).resolves.toEqual({
      status: "ready",
      items: [],
    });
    expect(invokeMock).toHaveBeenLastCalledWith("fetch_javdb_catalog", {
      ...request,
      contextGeneration: "1",
    });

    for (const [error, status] of [
      ["vr_source_unavailable", "source-unavailable"],
      ["vr_network_error", "network-error"],
      ["vr_javdb_malformed_provider", "malformed-provider"],
      ["vr_javdb_conflicting_provider", "conflicting-provider"],
      ["vr_provider_error", "provider-error"],
    ]) {
      invokeMock.mockRejectedValueOnce(error);
      await expect(fetchJavdbBrowse(request, "2")).resolves.toEqual({ status });
    }

    invokeMock.mockClear();
    for (const year of ["2000", String(new Date().getFullYear() + 1)]) {
      await expect(fetchJavdbBrowse({ ...request, year }, "3")).rejects.toThrow(
        "A valid JavDB browse request is required.",
      );
    }
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("requests cover bytes with opaque exact identity and no source URL", async () => {
    const createObjectURL = vi.fn().mockReturnValue("blob:exact-cover");
    vi.stubGlobal("URL", { createObjectURL });
    invokeMock.mockResolvedValue([
      0xff, 0xd8, 0xff, 0xe0, 0, 16, 0, 0, 0, 0, 0, 0,
    ]);
    const item: JavdbBrowseItem = {
      category: "vr",
      contextGeneration: "3",
      providerItemId: "VrA",
      requestGeneration: "7",
      code: "MDVR-419",
      title: null,
      releaseDate: null,
      coverAuthorityId: "javdb-cover-7-1-0123abcd",
      coverUrl: null,
      source: "JavDB",
      sourceAspectRatio: 1.48,
    };

    await expect(fetchJavdbCoverObjectUrl(item)).resolves.toBe(
      "blob:exact-cover",
    );
    expect(invokeMock).toHaveBeenCalledWith("fetch_javdb_cover", {
      category: "vr",
      requestGeneration: "7",
      providerItemId: "VrA",
      coverAuthorityId: "javdb-cover-7-1-0123abcd",
    });
    expect(invokeMock.mock.calls[0]?.[1]).not.toHaveProperty("sourceUrl");
    expect(createObjectURL).toHaveBeenCalledOnce();

    invokeMock.mockResolvedValueOnce([0xff, 0xd8, 0xff, 0xe0]);
    await expect(fetchJavdbCoverObjectUrl(item)).rejects.toThrow("invalid");
    expect(createObjectURL).toHaveBeenCalledOnce();
  });
});

describe("native-owned JavDB details and preview boundary", () => {
  const item: JavdbBrowseItem = {
    category: "vr",
    contextGeneration: "3",
    providerItemId: "VrA",
    requestGeneration: "7",
    code: "MDVR-419",
    title: "Listing title",
    releaseDate: null,
    coverAuthorityId: null,
    coverUrl: null,
    source: "JavDB",
    sourceAspectRatio: 1.48,
  };
  const response = [
    "11",
    "vr",
    "3",
    "7",
    "VrA",
    "MDVR-419",
    "Provider title",
    "Original title",
    "2026-08-12",
    "123",
    "Summary",
    "javdb-detail-cover-11-1-0123abcd",
    "2",
    "Actor A",
    "Actor B",
    "2",
    "VR",
    "Featured",
    "2",
    "javdb-preview-11-1-1111aaaa",
    "javdb-preview-11-2-2222bbbb",
  ];

  it("parses structured exact details while preserving optional presentation fields", () => {
    expect(parseJavdbDetailResponse(response, item)).toEqual({
      status: "ready",
      detail: {
        category: "vr",
        contextGeneration: "3",
        requestGeneration: "7",
        providerItemId: "VrA",
        code: "MDVR-419",
        detailGeneration: "11",
        title: "Provider title",
        originalTitle: "Original title",
        releaseDate: "2026-08-12",
        duration: "123",
        summary: "Summary",
        actors: ["Actor A", "Actor B"],
        tags: ["VR", "Featured"],
        coverAuthorityId: "javdb-detail-cover-11-1-0123abcd",
        previewAuthorityIds: [
          "javdb-preview-11-1-1111aaaa",
          "javdb-preview-11-2-2222bbbb",
        ],
      },
    });
    expect(
      parseJavdbDetailResponse(
        ["11", "vr", "3", "7", "VrA", "MDVR-419", "", "", "", "", "", "", "0", "0", "0"],
        item,
      ),
    ).toEqual({
      status: "ready",
      detail: expect.objectContaining({
        title: null,
        originalTitle: null,
        actors: [],
        tags: [],
        coverAuthorityId: null,
        previewAuthorityIds: [],
      }),
    });
    for (const invalid of [
      response.map((value, index) => (index === 1 ? "adult" : value)),
      response.map((value, index) => (index === 3 ? "8" : value)),
      response.map((value, index) => (index === 4 ? "VrB" : value)),
      response.map((value, index) =>
        index === 5 ? "MDVR-422" : value,
      ),
      response.map((value, index) =>
        index === 19 ? "https://tp.cmastd.com/raw.jpg" : value,
      ),
      [...response.slice(0, 18), "25", ...Array(25).fill("javdb-preview-11-1-1111aaaa")],
    ]) {
      expect(parseJavdbDetailResponse(invalid, item)).toEqual({
        status: "malformed-provider",
      });
    }
  });

  it("submits only the complete retained item authority and maps native detail errors", async () => {
    invokeMock.mockResolvedValueOnce(response);
    await expect(fetchJavdbDetail(item)).resolves.toEqual(
      parseJavdbDetailResponse(response, item),
    );
    expect(invokeMock).toHaveBeenLastCalledWith("fetch_javdb_detail", {
      category: "vr",
      contextGeneration: "3",
      requestGeneration: "7",
      providerItemId: "VrA",
      code: "MDVR-419",
    });
    expect(invokeMock.mock.calls[0]?.[1]).not.toHaveProperty("providerUrl");

    for (const [error, status] of [
      ["vr_source_unavailable", "source-unavailable"],
      ["vr_network_error", "network-error"],
      ["vr_javdb_malformed_provider", "malformed-provider"],
      ["vr_javdb_conflicting_provider", "conflicting-provider"],
      ["vr_javdb_stale", "stale"],
      ["vr_provider_error", "provider-error"],
    ]) {
      invokeMock.mockRejectedValueOnce(error);
      await expect(fetchJavdbDetail(item)).resolves.toEqual({ status });
    }
  });

  it("invalidates the exact native generation after renderer response validation fails", async () => {
    invokeMock
      .mockResolvedValueOnce(
        response.map((value, index) =>
          index === 4 ? "AnotherProviderItem" : value,
        ),
      )
      .mockResolvedValueOnce(undefined);

    await expect(fetchJavdbDetail(item)).resolves.toEqual({
      status: "malformed-provider",
    });
    expect(invokeMock).toHaveBeenNthCalledWith(1, "fetch_javdb_detail", {
      category: "vr",
      contextGeneration: "3",
      requestGeneration: "7",
      providerItemId: "VrA",
      code: "MDVR-419",
    });
    expect(invokeMock).toHaveBeenNthCalledWith(2, "invalidate_javdb_detail", {
      category: "vr",
      detailGeneration: "11",
    });
  });

  it("fetches and invalidates only opaque exact detail image authority", async () => {
    const parsed = parseJavdbDetailResponse(response, item);
    expect(parsed.status).toBe("ready");
    const detail = (parsed as { status: "ready"; detail: JavdbDetail }).detail;
    const createObjectURL = vi.fn().mockReturnValue("blob:preview");
    vi.stubGlobal("URL", { createObjectURL });
    invokeMock.mockResolvedValueOnce([
      0xff, 0xd8, 0xff, 0xe0, 0, 16, 0, 0, 0, 0, 0, 0,
    ]);
    await expect(
      fetchJavdbDetailImageObjectUrl(
        detail,
        "javdb-preview-11-1-1111aaaa",
      ),
    ).resolves.toBe("blob:preview");
    expect(invokeMock).toHaveBeenLastCalledWith(
      "fetch_javdb_detail_image",
      {
        category: "vr",
        contextGeneration: "3",
        requestGeneration: "7",
        providerItemId: "VrA",
        code: "MDVR-419",
        detailGeneration: "11",
        imageAuthorityId: "javdb-preview-11-1-1111aaaa",
      },
    );
    expect(invokeMock.mock.calls[0]?.[1]).not.toHaveProperty("imageUrl");

    invokeMock.mockResolvedValueOnce(undefined);
    await invalidateJavdbDetail(detail);
    expect(invokeMock).toHaveBeenLastCalledWith("invalidate_javdb_detail", {
      category: "vr",
      detailGeneration: "11",
    });
    invokeMock.mockResolvedValueOnce(undefined);
    await openJavdbDetailSource(detail);
    expect(invokeMock).toHaveBeenLastCalledWith(
      "open_javdb_detail_source",
      {
        category: "vr",
        contextGeneration: "3",
        requestGeneration: "7",
        providerItemId: "VrA",
        code: "MDVR-419",
        detailGeneration: "11",
      },
    );
    expect(invokeMock.mock.calls.at(-1)?.[1]).not.toHaveProperty("sourceUrl");

    await expect(
      fetchJavdbDetailImageObjectUrl(
        detail,
        "javdb-preview-11-3-3333cccc",
      ),
    ).rejects.toThrow("current");
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
  it("accepts a letter-ending alphanumeric prefix and rejects a digit-ending prefix before dispatch", async () => {
    invokeMock.mockResolvedValue(
      releaseFeed(releaseItem("Exact 3DSVR-1947 release")),
    );
    await expect(fetchVerifiedSukebeiReleases("3DSVR-1947")).resolves.toEqual({
      status: "ready",
      releases: [
        {
          name: "Exact 3DSVR-1947 release",
          source: "Sukebei",
          size: "12.5 GiB",
          seeders: 10,
        },
      ],
    });
    expect(invokeMock).toHaveBeenCalledWith("fetch_sukebei_vr_releases", {
      code: "3DSVR-1947",
    });

    invokeMock.mockClear();
    await expect(fetchVerifiedSukebeiReleases("AB1-2")).rejects.toThrow(
      "A canonical VR product code is required.",
    );
    expect(invokeMock).not.toHaveBeenCalled();
  });

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
      "false",
      "tv-transfer-701-2-3",
      "tv",
      "tt0123456 · S02E03",
      "Exact  Show — 特別版.S02E03+720p.第三話",
      "2",
      "12",
      "4",
      "256",
      "paused",
      "true",
      "none",
      "",
      "false",
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
      "false",
      "recovered-transfer-419",
      "vr",
      "MDVR-419",
      "【VR】 MDVR-419 recovered terminal",
      "1",
      "7",
      "7",
      "0",
      "failed",
      "true",
      "none",
      "",
      "false",
      "true",
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
        terminalRecovery: false,
      },
      {
        transferId: "tv-transfer-701-2-3",
        category: "tv",
        identity: "tt0123456 · S02E03",
        releaseName: "Exact  Show — 特別版.S02E03+720p.第三話",
        selectedFileCount: 2,
        totalBytes: "12",
        downloadedBytes: "4",
        speedBytesPerSecond: "256",
        state: "paused",
        isCurrentFolder: true,
        organizationStatus: "none",
        organizationRelativeDirectory: null,
        canOrganize: false,
        terminalRecovery: false,
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
        terminalRecovery: false,
      },
      {
        transferId: "recovered-transfer-419",
        category: "vr",
        identity: "MDVR-419",
        releaseName: "【VR】 MDVR-419 recovered terminal",
        selectedFileCount: 1,
        totalBytes: "7",
        downloadedBytes: "7",
        speedBytesPerSecond: "0",
        state: "failed",
        isCurrentFolder: true,
        organizationStatus: "none",
        organizationRelativeDirectory: null,
        canOrganize: false,
        terminalRecovery: true,
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
        terminalRecovery: false,
      },
    ]);
    expect(invokeMock).toHaveBeenCalledWith("load_vr_downloads");
  });

  it("accepts eligible, attention, and organized TV rows only in the canonical season directory", async () => {
    invokeMock.mockResolvedValue([
      "tv-ready-701-2-3",
      "tv",
      "tt0123456 · S02E03",
      "Exact Show S02E03",
      "1",
      "7",
      "7",
      "0",
      "completed",
      "true",
      "none",
      "",
      "true",
      "false",
      "tv-attention-701-2-3",
      "tv",
      "tt0123456 · S02E03",
      "Exact Show S02E03",
      "2",
      "14",
      "14",
      "0",
      "completed",
      "true",
      "attention",
      "Exact  Show — 特別版/Season 02/",
      "true",
      "false",
      "tv-organized-701-2-3",
      "tv",
      "tt0123456 · S02E03",
      "Exact Show S02E03",
      "1",
      "7",
      "7",
      "0",
      "completed",
      "true",
      "organized",
      "Exact  Show — 特別版/Season 02/",
      "false",
      "false",
    ]);

    await expect(loadVrDownloads()).resolves.toEqual([
      expect.objectContaining({
        transferId: "tv-ready-701-2-3",
        category: "tv",
        organizationStatus: "none",
        canOrganize: true,
      }),
      expect.objectContaining({
        transferId: "tv-attention-701-2-3",
        organizationStatus: "attention",
        organizationRelativeDirectory: "Exact  Show — 特別版/Season 02/",
        canOrganize: true,
      }),
      expect.objectContaining({
        transferId: "tv-organized-701-2-3",
        organizationStatus: "organized",
        organizationRelativeDirectory: "Exact  Show — 特別版/Season 02/",
        canOrganize: false,
      }),
    ]);
  });

  it("rejects TV rows with fabricated episode identity or organization state", async () => {
    const validTvRow = [
      "tv-transfer-701-2-3",
      "tv",
      "tt0123456 · S02E03",
      "Exact Show S02E03",
      "1",
      "7",
      "3",
      "512",
      "downloading",
      "true",
      "none",
      "",
      "false",
      "false",
    ];
    for (const [fieldIndex, invalidValue] of [
      [2, "tt0123456"],
      [2, "tt0123456 · S00E03"],
      [2, "tt0123456 · S02E00"],
      [10, "attention"],
      [11, "Exact Show/Season 03/"],
      [12, "invalid"],
    ] as const) {
      const malformed = [...validTvRow];
      malformed[fieldIndex] = invalidValue;
      invokeMock.mockResolvedValueOnce(malformed);
      await expect(loadVrDownloads()).rejects.toThrow("invalid data");
    }
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
      "false",
    ];
    for (const [fieldIndex, invalidValue] of [
      [2, "MDVR-419"],
      [10, "organized"],
      [11, "tt0123456/"],
      [12, "true"],
      [13, "true"],
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
      "false",
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
      "false",
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

  it("accepts only canonical eligible, attention, and organized Movie rows", async () => {
    invokeMock.mockResolvedValue([
      "movie-ready-419",
      "movie",
      "tt0123456",
      "Exact  Movie — 特別版",
      "1",
      "7",
      "7",
      "0",
      "completed",
      "true",
      "none",
      "",
      "true",
      "false",
      "movie-attention-419",
      "movie",
      "tt0123456",
      "Exact  Movie — 特別版",
      "2",
      "14",
      "14",
      "0",
      "completed",
      "true",
      "attention",
      "Exact  Movie — 特別版 (1999)/",
      "true",
      "false",
      "movie-organized-419",
      "movie",
      "tt0123456",
      "Exact  Movie — 特別版",
      "1",
      "7",
      "7",
      "0",
      "completed",
      "true",
      "organized",
      "Exact  Movie — 特別版 (1999)/",
      "false",
      "false",
    ]);

    await expect(loadVrDownloads()).resolves.toEqual([
      expect.objectContaining({
        transferId: "movie-ready-419",
        category: "movie",
        organizationStatus: "none",
        canOrganize: true,
      }),
      expect.objectContaining({
        transferId: "movie-attention-419",
        organizationStatus: "attention",
        organizationRelativeDirectory: "Exact  Movie — 特別版 (1999)/",
        canOrganize: true,
      }),
      expect.objectContaining({
        transferId: "movie-organized-419",
        organizationStatus: "organized",
        organizationRelativeDirectory: "Exact  Movie — 特別版 (1999)/",
        canOrganize: false,
      }),
    ]);

    for (const directory of [
      "YTS title (1999)/",
      "Exact  Movie — 特別版 (99)/",
      "Exact  Movie — 特別版 (0000)/",
      "../Exact  Movie — 特別版 (1999)/",
    ]) {
      invokeMock.mockResolvedValueOnce([
        "movie-attention-419",
        "movie",
        "tt0123456",
        "Exact  Movie — 特別版",
        "1",
        "7",
        "7",
        "0",
        "completed",
        "true",
        "attention",
        directory,
        "true",
        "false",
      ]);
      await expect(loadVrDownloads()).rejects.toThrow("invalid data");
    }
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
        terminalRecovery: false,
      },
    ]);

    for (const [fieldIndex, invalidValue] of [
      [8, "paused"],
      [10, "attention"],
      [12, "true"],
      [13, "true"],
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
        "false",
      ];
      invalidRow[fieldIndex] = invalidValue;
      invokeMock.mockResolvedValueOnce(invalidRow);
      await expect(loadVrDownloads()).rejects.toThrow("invalid data");
    }
  });

  it("parses exact Windows cleanup files only for cancelled or recovery rows", async () => {
    const exactReleaseName = "【VR】 MDVR-419  Exact — 特別版";
    const encode = (value: string) =>
      Array.from(new TextEncoder().encode(value), (byte) =>
        byte.toString(16).padStart(2, "0"),
      ).join("");
    const selectedFiles = [
      "Provider/Exact  Movie — 特別版.MKV",
      "Provider/notes  01.txt",
    ];
    invokeMock.mockResolvedValue([
      "cancelled-transfer-419",
      "movie",
      "tt0123456",
      "Exact  Movie — 特別版",
      "2",
      "12",
      "7",
      "0",
      "cancelled",
      "true",
      "none",
      "",
      "false",
      "false",
      selectedFiles.map(encode).join(","),
      "true",
      "cleanup-transfer-419",
      "vr",
      "MDVR-419",
      exactReleaseName,
      "1",
      "7",
      "7",
      "0",
      "cleanup",
      "true",
      "none",
      "",
      "false",
      "false",
      encode("Provider/MDVR-419.mp4"),
      "true",
    ]);

    await expect(loadVrDownloads()).resolves.toEqual([
      expect.objectContaining({
        transferId: "cancelled-transfer-419",
        state: "cancelled",
        selectedFiles,
        cleanupAvailable: true,
      }),
      expect.objectContaining({
        transferId: "cleanup-transfer-419",
        state: "cleanup",
        selectedFiles: ["Provider/MDVR-419.mp4"],
        cleanupAvailable: true,
      }),
    ]);

    const invalid = [
      "active-transfer-419",
      "vr",
      "MDVR-419",
      exactReleaseName,
      "1",
      "7",
      "3",
      "12",
      "downloading",
      "true",
      "none",
      "",
      "false",
      "false",
      encode("Provider/MDVR-419.mp4"),
      "true",
    ];
    invokeMock.mockResolvedValueOnce(invalid);
    await expect(loadVrDownloads()).rejects.toThrow("invalid data");
    invalid[8] = "cancelled";
    invalid[14] = "ff";
    invokeMock.mockResolvedValueOnce(invalid);
    await expect(loadVrDownloads()).rejects.toThrow("invalid data");
  });

  it("submits only the transfer identity to permanent cleanup", async () => {
    invokeMock.mockResolvedValue(["tv", "true"]);

    await expect(cleanupCancelledVrDownload("transfer-123")).resolves.toEqual({
      category: "tv",
      isCurrentFolder: true,
    });
    expect(invokeMock).toHaveBeenCalledWith(
      "cleanup_cancelled_vr_download",
      { transferId: "transfer-123" },
    );

    invokeMock.mockClear();
    await expect(cleanupCancelledVrDownload("")).rejects.toThrow(
      "transfer identity",
    );
    expect(invokeMock).not.toHaveBeenCalled();
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
      identity: "MDVR-419",
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

  it("accepts only a structurally consistent native TV organization plan", async () => {
    invokeMock.mockResolvedValueOnce([
      "tv-plan-701-2-3",
      "tv-transfer-701-2-3",
      "tt0123456 · S02E03",
      "1",
      "3",
      "move",
      "Provider/Exact  Show — 特別版.S02E03.Cut  A.mp4",
      "Exact  Show — 特別版/Season 02/Exact  Show — 特別版.S02E03.Cut  A.mp4",
      "media-unchanged",
      "Exact  Show — 特別版/Season 02/S02E03 — Cut  B.MKV",
      "Exact  Show — 特別版/Season 02/S02E03 — Cut  B.MKV",
      "non-media-unchanged",
      "Provider/notes.txt",
      "",
    ]);

    await expect(previewVrOrganization("tv-transfer-701-2-3")).resolves.toEqual({
      planId: "tv-plan-701-2-3",
      transferId: "tv-transfer-701-2-3",
      identity: "tt0123456 · S02E03",
      moveCount: 1,
      entries: [
        {
          kind: "move",
          sourceRelativePath:
            "Provider/Exact  Show — 特別版.S02E03.Cut  A.mp4",
          destinationRelativePath:
            "Exact  Show — 特別版/Season 02/Exact  Show — 特別版.S02E03.Cut  A.mp4",
        },
        {
          kind: "media-unchanged",
          sourceRelativePath:
            "Exact  Show — 特別版/Season 02/S02E03 — Cut  B.MKV",
          destinationRelativePath:
            "Exact  Show — 特別版/Season 02/S02E03 — Cut  B.MKV",
        },
        {
          kind: "non-media-unchanged",
          sourceRelativePath: "Provider/notes.txt",
          destinationRelativePath: null,
        },
      ],
    });

    for (const response of [
      [
        "tv-plan-701-2-3",
        "tv-transfer-701-2-3",
        "tt0123456 · S02E03",
        "1",
        "1",
        "move",
        "Provider/Episode.mp4",
        "Exact  Show — 特別版/Season 03/Episode.mp4",
      ],
      [
        "tv-plan-701-2-3",
        "tv-transfer-701-2-3",
        "tt0123456 · S02E03",
        "1",
        "1",
        "move",
        "Provider/Episode.mp4",
        "Exact  Show — 特別版/Episode.mp4",
      ],
      [
        "tv-plan-701-2-3",
        "tv-transfer-701-2-3",
        "tt0123456 · S02E03",
        "2",
        "2",
        "move",
        "Provider/Episode A.mp4",
        "Exact  Show — 特別版/Season 02/Episode A.mp4",
        "move",
        "Provider/Episode B.mp4",
        "Other Show/Season 02/Episode B.mp4",
      ],
    ]) {
      invokeMock.mockResolvedValueOnce(response);
      await expect(previewVrOrganization("tv-transfer-701-2-3")).rejects.toThrow(
        "invalid data",
      );
    }
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
      identity: "ADLT-123",
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

  it("parses exact Movie organization destinations without product-code coercion", async () => {
    invokeMock.mockResolvedValueOnce([
      "movie-plan-419",
      "movie-transfer-419",
      "tt0123456",
      "2",
      "3",
      "move",
      "Provider/Feature  Cut.mp4",
      "Exact  Movie — 特別版 (1999)/Feature  Cut.mp4",
      "move",
      "Provider/Second — 特別.MKV",
      "Exact  Movie — 特別版 (1999)/Second — 特別.MKV",
      "non-media-unchanged",
      "Provider/notes.txt",
      "",
    ]);

    await expect(previewVrOrganization("movie-transfer-419")).resolves.toEqual({
      planId: "movie-plan-419",
      transferId: "movie-transfer-419",
      identity: "tt0123456",
      moveCount: 2,
      entries: [
        {
          kind: "move",
          sourceRelativePath: "Provider/Feature  Cut.mp4",
          destinationRelativePath:
            "Exact  Movie — 特別版 (1999)/Feature  Cut.mp4",
        },
        {
          kind: "move",
          sourceRelativePath: "Provider/Second — 特別.MKV",
          destinationRelativePath:
            "Exact  Movie — 特別版 (1999)/Second — 特別.MKV",
        },
        {
          kind: "non-media-unchanged",
          sourceRelativePath: "Provider/notes.txt",
          destinationRelativePath: null,
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
      [
        "plan",
        "transfer",
        "tt0123456",
        "1",
        "1",
        "move",
        "source.mp4",
        "YTS title/file.mp4",
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
