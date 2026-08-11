import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  chooseTvFolder,
  clearTvShowMetadataMatch,
  invalidateTvShowMetadataContext,
  loadTvFolder,
  openTvFile,
  queryTvStorage,
  revealTvFile,
  saveTvShowMetadataMatch,
  scanTvLibrary,
  searchTvShowMetadata,
  trashTvFile,
  verifyTvShowMetadataCandidate,
} from "./tv";

let invokeMock: ReturnType<typeof vi.fn>;

beforeEach(() => {
  invokeMock = vi.fn();
  vi.stubGlobal("__TAURI__", { core: { invoke: invokeMock } });
});

function tvScanFile(
  path: string,
  relativePath: string,
  sizeBytes: string,
  identity?: { showTitle: string; season: number; episode: number },
) {
  return [
    path,
    relativePath,
    sizeBytes,
    identity?.showTitle ?? "",
    identity?.season.toString() ?? "",
    identity?.episode.toString() ?? "",
  ];
}

describe("TV folder and storage boundaries", () => {
  it("loads every native folder state without changing its exact path", async () => {
    invokeMock.mockResolvedValueOnce(["unconfigured"]);
    await expect(loadTvFolder()).resolves.toEqual({ status: "unconfigured" });
    invokeMock.mockResolvedValueOnce(["ready", "/TV/番組  Library"]);
    await expect(loadTvFolder()).resolves.toEqual({
      status: "ready",
      path: "/TV/番組  Library",
    });
    invokeMock.mockResolvedValueOnce(["unavailable", "/TV/Missing"]);
    await expect(loadTvFolder()).resolves.toEqual({
      status: "unavailable",
      path: "/TV/Missing",
    });
  });

  it("rejects malformed folder and storage responses", async () => {
    invokeMock.mockResolvedValueOnce(["ready", ""]);
    await expect(loadTvFolder()).rejects.toThrow("invalid data");
    invokeMock.mockResolvedValueOnce(["100", "101"]);
    await expect(queryTvStorage()).rejects.toThrow("invalid data");
  });

  it("preserves a selected path and validates cancelled selection", async () => {
    invokeMock.mockResolvedValueOnce("/TV/Show  Folder");
    await expect(chooseTvFolder()).resolves.toBe("/TV/Show  Folder");
    invokeMock.mockResolvedValueOnce(null);
    await expect(chooseTvFolder()).resolves.toBeNull();
  });
});

describe("conservative parsed TV Library identity", () => {
  it("preserves the trusted generation for an empty scan", async () => {
    invokeMock.mockResolvedValue(["6"]);

    await expect(scanTvLibrary()).resolves.toEqual({
      generation: "6",
      items: [],
    });
  });

  it("groups ordinary marker and padding variants while preserving exact file data", async () => {
    invokeMock.mockResolvedValue([
      "7",
      ...tvScanFile(
        "/TV/Series/星  Show.S01E02 — Pilot.mp4",
        "Series/星  Show.S01E02 — Pilot.mp4",
        "10",
        { showTitle: "星  Show", season: 1, episode: 2 },
      ),
      ...tvScanFile(
        "/TV/Series/星  Show.s1e3.MKV",
        "Series/星  Show.s1e3.MKV",
        "20",
        { showTitle: "星  Show", season: 1, episode: 3 },
      ),
      ...tvScanFile(
        "/TV/Folder  Show/1x04.mp4",
        "Folder  Show/1x04.mp4",
        "30",
        { showTitle: "Folder  Show", season: 1, episode: 4 },
      ),
      ...tvScanFile(
        "/TV/Folder  Show/01x05.MKV",
        "Folder  Show/01x05.MKV",
        "40",
        { showTitle: "Folder  Show", season: 1, episode: 5 },
      ),
    ]);

    const scan = await scanTvLibrary();

    expect(scan.generation).toBe("7");
    expect(scan.items).toEqual([
      {
        id: "show:星  Show",
        title: "星  Show",
        showTitle: "星  Show",
        files: [
          {
            path: "/TV/Series/星  Show.S01E02 — Pilot.mp4",
            relativePath: "Series/星  Show.S01E02 — Pilot.mp4",
            filename: "星  Show.S01E02 — Pilot.mp4",
            sizeBytes: "10",
            season: 1,
            episode: 2,
          },
          {
            path: "/TV/Series/星  Show.s1e3.MKV",
            relativePath: "Series/星  Show.s1e3.MKV",
            filename: "星  Show.s1e3.MKV",
            sizeBytes: "20",
            season: 1,
            episode: 3,
          },
        ],
      },
      {
        id: "show:Folder  Show",
        title: "Folder  Show",
        showTitle: "Folder  Show",
        files: [
          {
            path: "/TV/Folder  Show/1x04.mp4",
            relativePath: "Folder  Show/1x04.mp4",
            filename: "1x04.mp4",
            sizeBytes: "30",
            season: 1,
            episode: 4,
          },
          {
            path: "/TV/Folder  Show/01x05.MKV",
            relativePath: "Folder  Show/01x05.MKV",
            filename: "01x05.MKV",
            sizeBytes: "40",
            season: 1,
            episode: 5,
          },
        ],
      },
    ]);
  });

  it("keeps ambiguous, embedded, neighboring, conflicting, and titleless signals unassociated", async () => {
    const filenames = [
      "Show.S01E020.mp4",
      "ShowS01E02.mp4",
      "Show.S01E02extra.mkv",
      "Show.S01E02.S01E03.mp4",
      "Show.1x02.S01E02.mkv",
      "Show.S01E02-E03.mp4",
      "Show.S01E02-03.mkv",
      "Show.1x02-03.mp4",
      "Show.S123E456-E457.mp4",
      "Show.S123E456-E-457.mkv",
      "Show.S123E456-x457.mp4",
      "Show.S123E456 x 457.mkv",
      "Show.123x456-x457.mp4",
      "Show.123x456-(X)-457.mkv",
      "Show.123x456/x457.mp4",
      "Show.S0123E456.mp4",
      "Show.S123E0456.mkv",
      "Show.S9007199254740992E456.mp4",
      "S01E02.mp4",
      "Show.S00E02.mp4",
      "Show.S01E00.mp4",
    ];
    invokeMock.mockResolvedValue([
      "8",
      ...filenames.flatMap((filename, index) => [
        `/TV/${filename}`,
        filename,
        String(index + 1),
        "",
        "",
        "",
      ]),
    ]);

    const { items } = await scanTvLibrary();
    const basenames = filenames.map((filename) => filename.split("/").at(-1));

    expect(items).toHaveLength(filenames.length);
    expect(items.every((item) => item.showTitle === null)).toBe(true);
    expect(items.map((item) => item.files[0].filename)).toEqual(basenames);
    expect(items.every((item) => item.files[0].season === null)).toBe(true);
  });

  it("preserves valid quality and codec suffixes on one large episode identity", async () => {
    const filenames = [
      "Show.S123E456+720p.mp4",
      "Show.S123E456.x264.mkv",
      "Show.123x456+10bit.mp4",
      "Show.123x456.x265.mkv",
    ];
    invokeMock.mockResolvedValue([
      "9",
      ...filenames.flatMap((filename, index) => [
        `/TV/${filename}`,
        filename,
        String(index + 1),
        "Show",
        "123",
        "456",
      ]),
    ]);

    const { items } = await scanTvLibrary();

    expect(items).toHaveLength(1);
    expect(items[0].showTitle).toBe("Show");
    expect(items[0].files.map((file) => file.filename)).toEqual(
      expect.arrayContaining(filenames),
    );
    expect(
      items[0].files.every(
        (file) => file.season === 123 && file.episode === 456,
      ),
    ).toBe(true);
  });

  it("does not merge prefix, substring, or neighboring show titles", async () => {
    invokeMock.mockResolvedValue([
      "9",
      ...tvScanFile("/TV/Show.S01E02.mp4", "Show.S01E02.mp4", "1", {
        showTitle: "Show",
        season: 1,
        episode: 2,
      }),
      ...tvScanFile(
        "/TV/Showtime.S01E02.mp4",
        "Showtime.S01E02.mp4",
        "2",
        { showTitle: "Showtime", season: 1, episode: 2 },
      ),
      ...tvScanFile(
        "/TV/Show 2.S01E02.mp4",
        "Show 2.S01E02.mp4",
        "3",
        { showTitle: "Show 2", season: 1, episode: 2 },
      ),
    ]);

    const { items } = await scanTvLibrary();

    expect(items.map((item) => item.title)).toEqual(["Show", "Showtime", "Show 2"]);
    expect(items.every((item) => item.files.length === 1)).toBe(true);
  });

  it("uses only an exact canonical show and matching season parent for retained episode basenames", async () => {
    invokeMock.mockResolvedValue([
      "10",
      ...tvScanFile(
        "/TV/Exact  Show — 特別版/Season 02/S02E03.Cut.mp4",
        "Exact  Show — 特別版/Season 02/S02E03.Cut.mp4",
        "3",
        { showTitle: "Exact  Show — 特別版", season: 2, episode: 3 },
      ),
      ...tvScanFile(
        "/TV/Exact  Show — 特別版/Season 02/S02E03 — Alternate.MKV",
        "Exact  Show — 特別版/Season 02/S02E03 — Alternate.MKV",
        "4",
        { showTitle: "Exact  Show — 特別版", season: 2, episode: 3 },
      ),
      ...tvScanFile(
        "/TV/Exact  Show — 特別版/Season 02/No episode marker.mp4",
        "Exact  Show — 特別版/Season 02/No episode marker.mp4",
        "5",
      ),
      ...tvScanFile(
        "/TV/Wrong Parent/Season 03/S02E03.mp4",
        "Wrong Parent/Season 03/S02E03.mp4",
        "6",
      ),
      ...tvScanFile(
        "/TV/Exact  Show — 特別版/Season 02/Exact  Show — 特別版 - S02E03 - 第三話  —  Exact Episode.MP4",
        "Exact  Show — 特別版/Season 02/Exact  Show — 特別版 - S02E03 - 第三話  —  Exact Episode.MP4",
        "7",
        { showTitle: "Exact  Show — 特別版", season: 2, episode: 3 },
      ),
    ]);

    const { items } = await scanTvLibrary();
    const exactShow = items.find(
      (item) => item.showTitle === "Exact  Show — 特別版",
    );
    expect(exactShow?.files).toHaveLength(3);
    expect(exactShow?.files.map((file) => file.filename)).toEqual(
      expect.arrayContaining([
        "S02E03.Cut.mp4",
        "S02E03 — Alternate.MKV",
        "Exact  Show — 特別版 - S02E03 - 第三話  —  Exact Episode.MP4",
      ]),
    );
    expect(exactShow?.files.every((file) => file.season === 2 && file.episode === 3))
      .toBe(true);
    expect(
      items.find((item) => item.title === "No episode marker")?.showTitle,
    ).toBeNull();
    expect(items.find((item) => item.title === "S02E03")?.showTitle).toBeNull();
  });

  it("recognizes positive season and episode numbers above 99 only with an exact matching parent", async () => {
    invokeMock.mockResolvedValue([
      "11",
      ...tvScanFile(
        "/TV/Exact Big Show/Season 123/Exact Big Show - S123E456 - Exact Episode.MKV",
        "Exact Big Show/Season 123/Exact Big Show - S123E456 - Exact Episode.MKV",
        "3",
        { showTitle: "Exact Big Show", season: 123, episode: 456 },
      ),
      ...tvScanFile(
        "/TV/Exact Big Show/Season 123/S123E456.Cut.mp4",
        "Exact Big Show/Season 123/S123E456.Cut.mp4",
        "4",
        { showTitle: "Exact Big Show", season: 123, episode: 456 },
      ),
      ...tvScanFile(
        "/TV/Wrong Parent/Season 124/S123E456.mp4",
        "Wrong Parent/Season 124/S123E456.mp4",
        "5",
      ),
      ...tvScanFile("/TV/S123/S123E456.mkv", "S123/S123E456.mkv", "6"),
      ...tvScanFile(
        "/TV/Exact Big Show/Season 123/Exact S01E02 Show - S123E456 - Exact Episode.mkv",
        "Exact Big Show/Season 123/Exact S01E02 Show - S123E456 - Exact Episode.mkv",
        "7",
      ),
      ...tvScanFile(
        "/TV/Exact Big Show/Season 123/Exact Big Show - S123E456 - Flashback 1x02.mkv",
        "Exact Big Show/Season 123/Exact Big Show - S123E456 - Flashback 1x02.mkv",
        "8",
      ),
    ]);

    const { items } = await scanTvLibrary();
    const exactShow = items.find((item) => item.showTitle === "Exact Big Show");
    expect(exactShow?.files).toHaveLength(2);
    expect(exactShow?.files.map((file) => file.filename)).toEqual([
      "Exact Big Show - S123E456 - Exact Episode.MKV",
      "S123E456.Cut.mp4",
    ]);
    expect(
      exactShow?.files.every(
        (file) => file.season === 123 && file.episode === 456,
      ),
    ).toBe(true);
    const unassociated = items.filter((item) => item.showTitle === null);
    expect(unassociated).toHaveLength(4);
    expect(unassociated.map((item) => item.files[0].filename)).toEqual([
      "S123E456.mp4",
      "S123E456.mkv",
      "Exact S01E02 Show - S123E456 - Exact Episode.mkv",
      "Exact Big Show - S123E456 - Flashback 1x02.mkv",
    ]);
  });

  it("rejects malformed rows, duplicate paths, traversal, unsupported files, and oversized sizes", async () => {
    for (const response of [
      [],
      ["invalid"],
      ["1", "/TV/Show.S01E02.mp4"],
      [
        "1",
        ...tvScanFile("/TV/Show.S01E02.mp4", "Show.S01E02.mp4", "1", {
          showTitle: "Show",
          season: 1,
          episode: 2,
        }),
        ...tvScanFile("/TV/Show.S01E02.mp4", "Show.S01E02.mp4", "1", {
          showTitle: "Show",
          season: 1,
          episode: 2,
        }),
      ],
      [
        "1",
        ...tvScanFile(
          "/TV/Show.S01E02.mp4",
          "../Show.S01E02.mp4",
          "1",
          { showTitle: "Show", season: 1, episode: 2 },
        ),
      ],
      [
        "1",
        ...tvScanFile(
          "/TV/Show.S01E02.txt",
          "Show.S01E02.txt",
          "1",
          { showTitle: "Show", season: 1, episode: 2 },
        ),
      ],
      [
        "1",
        ...tvScanFile(
          "/TV/Show.S01E02.mp4",
          "Show.S01E02.mp4",
          "18446744073709551616",
          { showTitle: "Show", season: 1, episode: 2 },
        ),
      ],
      [
        "1",
        "/TV/Show.S01E02.mp4",
        "Show.S01E02.mp4",
        "1",
        "Show",
        "1",
        "",
      ],
      [
        "1",
        "/TV/Show.S01E02.mp4",
        "Show.S01E02.mp4",
        "1",
        "Show",
        "9007199254740992",
        "2",
      ],
      [
        "1",
        "/TV/Show.S01E02.mp4",
        "Show.S01E02.mp4",
        "1",
        "---",
        "1",
        "2",
      ],
    ]) {
      invokeMock.mockResolvedValueOnce(response);
      await expect(scanTvLibrary()).rejects.toThrow("invalid data");
    }
  });
});

describe("explicit TV show metadata boundary", () => {
  const groupId = "1".repeat(40);
  const matchingRequestId = "2".repeat(40);
  const verificationId = "3".repeat(40);
  const association = [
    "701",
    "tt1234567",
    "Canonical  番組",
    "Original  番組",
    "2020-04-03",
    "/poster.jpg",
    "Exact  overview.",
    "9",
  ];

  it("parses native group authority and exact accepted show metadata without enriching episodes", async () => {
    invokeMock.mockResolvedValue([
      "tv-library-metadata-v1",
      "ready",
      "14",
      "2",
      "/TV/Local  Show.S01E01.mp4",
      "Local  Show.S01E01.mp4",
      "10",
      "Local  Show",
      "1",
      "1",
      groupId,
      "ready",
      ...association,
      "/TV/Local  Show.S01E02.MKV",
      "Local  Show.S01E02.MKV",
      "20",
      "Local  Show",
      "1",
      "2",
      groupId,
      "ready",
      ...association,
    ]);

    await expect(scanTvLibrary()).resolves.toEqual({
      generation: "14",
      metadataStatus: "ready",
      items: [
        {
          id: groupId,
          groupId,
          metadataState: "ready",
          title: "Canonical  番組",
          showTitle: "Local  Show",
          association: {
            tmdbTvId: 701,
            imdbId: "tt1234567",
            name: "Canonical  番組",
            originalName: "Original  番組",
            firstAirDate: "2020-04-03",
            posterPath: "/poster.jpg",
            overview: "Exact  overview.",
            generation: "9",
          },
          files: [
            expect.objectContaining({ season: 1, episode: 1 }),
            expect.objectContaining({ season: 1, episode: 2 }),
          ],
        },
      ],
    });
  });

  it("fails closed on conflicting group metadata and leaves attention data unenriched", async () => {
    invokeMock.mockResolvedValueOnce([
      "tv-library-metadata-v1",
      "attention",
      "15",
      "1",
      "/TV/Local Show.S01E01.mp4",
      "Local Show.S01E01.mp4",
      "10",
      "Local Show",
      "1",
      "1",
      groupId,
      "attention",
      ...Array.from({ length: 8 }, () => ""),
    ]);
    await expect(scanTvLibrary()).resolves.toEqual({
      generation: "15",
      metadataStatus: "attention",
      items: [
        expect.objectContaining({
          id: groupId,
          title: "Local Show",
          association: null,
          metadataState: "attention",
        }),
      ],
    });

    for (const malformed of [
      [
        "tv-library-metadata-v1",
        "ready",
        "16",
        "1",
        "/TV/Show.S01E01.mp4",
        "Show.S01E01.mp4",
        "10",
        "Show",
        "1",
        "1",
        "fabricated",
        "",
        ...Array.from({ length: 8 }, () => ""),
      ],
      [
        "tv-library-metadata-v1",
        "ready",
        "16",
        "1",
        "/TV/Show.S01E01.mp4",
        "Show.S01E01.mp4",
        "10",
        "Show",
        "1",
        "1",
        groupId,
        "ready",
        ...association.map((field, index) => (index === 1 ? "tt0x" : field)),
      ],
    ]) {
      invokeMock.mockResolvedValueOnce(malformed);
      await expect(scanTvLibrary()).rejects.toThrow("invalid data");
    }
  });

  it("dispatches only exact bounded Search, verification, Save, clear, and invalidation values", async () => {
    invokeMock
      .mockResolvedValueOnce([
        matchingRequestId,
        "2",
        "701",
        "Same Name",
        "Original One",
        "2001-01-01",
        "/one.jpg",
        "702",
        "Same Name",
        "Original Two",
        "2021-01-01",
        "",
      ])
      .mockResolvedValueOnce([verificationId, ...association])
      .mockResolvedValueOnce(association)
      .mockResolvedValueOnce(undefined)
      .mockResolvedValueOnce(undefined);

    await expect(
      searchTvShowMetadata(groupId, "完全  Local  Show", 1),
    ).resolves.toEqual({
      matchingRequestId,
      candidates: [
        expect.objectContaining({ tmdbTvId: 701, firstAirDate: "2001-01-01" }),
        expect.objectContaining({ tmdbTvId: 702, firstAirDate: "2021-01-01" }),
      ],
    });
    await verifyTvShowMetadataCandidate(matchingRequestId, 702, 2);
    await saveTvShowMetadataMatch(verificationId);
    await clearTvShowMetadataMatch(groupId);
    await invalidateTvShowMetadataContext(3);

    expect(invokeMock).toHaveBeenNthCalledWith(1, "search_tv_show_metadata", {
      groupId,
      query: "完全  Local  Show",
      contextGeneration: 1,
    });
    expect(invokeMock).toHaveBeenNthCalledWith(
      2,
      "verify_tv_show_metadata_candidate",
      { matchingRequestId, tmdbTvId: 702, contextGeneration: 2 },
    );
    expect(invokeMock).toHaveBeenNthCalledWith(3, "save_tv_show_metadata_match", {
      verificationId,
    });
    expect(invokeMock).toHaveBeenNthCalledWith(4, "clear_tv_show_metadata_match", {
      groupId,
    });
    expect(invokeMock).toHaveBeenNthCalledWith(
      5,
      "invalidate_tv_show_metadata_context",
      { contextGeneration: 3 },
    );
  });

  it("rejects duplicate candidates and malformed exact verification responses", async () => {
    invokeMock.mockResolvedValueOnce([
      matchingRequestId,
      "2",
      "701",
      "Same",
      "",
      "",
      "",
      "701",
      "Conflict",
      "",
      "",
      "",
    ]);
    await expect(searchTvShowMetadata(groupId, "Show", 1)).rejects.toThrow(
      "invalid data",
    );
    invokeMock.mockResolvedValueOnce([
      verificationId,
      ...association.map((field, index) => (index === 1 ? "nm123" : field)),
    ]);
    await expect(
      verifyTvShowMetadataCandidate(matchingRequestId, 701, 2),
    ).rejects.toThrow("invalid data");
  });
});

describe("TV file actions", () => {
  it("dispatches exact paths to their isolated native commands", async () => {
    invokeMock.mockResolvedValue(undefined);

    await openTvFile("/TV/番組  Show.S01E02.mp4");
    await revealTvFile("/TV/番組  Show.S01E02.mp4");
    await trashTvFile("/TV/番組  Show.S01E02.mp4", "42");

    expect(invokeMock).toHaveBeenNthCalledWith(1, "open_tv_file", {
      path: "/TV/番組  Show.S01E02.mp4",
    });
    expect(invokeMock).toHaveBeenNthCalledWith(2, "reveal_tv_file", {
      path: "/TV/番組  Show.S01E02.mp4",
    });
    expect(invokeMock).toHaveBeenNthCalledWith(3, "trash_tv_file", {
      path: "/TV/番組  Show.S01E02.mp4",
      scanGeneration: "42",
    });
  });
});
