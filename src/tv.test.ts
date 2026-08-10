import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  chooseTvFolder,
  loadTvFolder,
  openTvFile,
  queryTvStorage,
  revealTvFile,
  scanTvLibrary,
  trashTvFile,
} from "./tv";

let invokeMock: ReturnType<typeof vi.fn>;

beforeEach(() => {
  invokeMock = vi.fn();
  vi.stubGlobal("__TAURI__", { core: { invoke: invokeMock } });
});

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
      "/TV/Series/星  Show.S01E02 — Pilot.mp4",
      "Series/星  Show.S01E02 — Pilot.mp4",
      "10",
      "/TV/Series/星  Show.s1e3.MKV",
      "Series/星  Show.s1e3.MKV",
      "20",
      "/TV/Folder  Show/1x04.mp4",
      "Folder  Show/1x04.mp4",
      "30",
      "/TV/Folder  Show/01x05.MKV",
      "Folder  Show/01x05.MKV",
      "40",
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
      ]),
    ]);

    const { items } = await scanTvLibrary();

    expect(items).toHaveLength(filenames.length);
    expect(items.every((item) => item.showTitle === null)).toBe(true);
    expect(items.map((item) => item.files[0].filename)).toEqual(filenames);
    expect(items.every((item) => item.files[0].season === null)).toBe(true);
  });

  it("does not merge prefix, substring, or neighboring show titles", async () => {
    invokeMock.mockResolvedValue([
      "9",
      "/TV/Show.S01E02.mp4",
      "Show.S01E02.mp4",
      "1",
      "/TV/Showtime.S01E02.mp4",
      "Showtime.S01E02.mp4",
      "2",
      "/TV/Show 2.S01E02.mp4",
      "Show 2.S01E02.mp4",
      "3",
    ]);

    const { items } = await scanTvLibrary();

    expect(items.map((item) => item.title)).toEqual(["Show", "Showtime", "Show 2"]);
    expect(items.every((item) => item.files.length === 1)).toBe(true);
  });

  it("uses only an exact canonical show and matching season parent for retained episode basenames", async () => {
    invokeMock.mockResolvedValue([
      "10",
      "/TV/Exact  Show — 特別版/Season 02/S02E03.Cut.mp4",
      "Exact  Show — 特別版/Season 02/S02E03.Cut.mp4",
      "3",
      "/TV/Exact  Show — 特別版/Season 02/S02E03 — Alternate.MKV",
      "Exact  Show — 特別版/Season 02/S02E03 — Alternate.MKV",
      "4",
      "/TV/Exact  Show — 特別版/Season 02/No episode marker.mp4",
      "Exact  Show — 特別版/Season 02/No episode marker.mp4",
      "5",
      "/TV/Wrong Parent/Season 03/S02E03.mp4",
      "Wrong Parent/Season 03/S02E03.mp4",
      "6",
      "/TV/Exact  Show — 特別版/Season 02/Exact  Show — 特別版 - S02E03 - 第三話  —  Exact Episode.MP4",
      "Exact  Show — 特別版/Season 02/Exact  Show — 特別版 - S02E03 - 第三話  —  Exact Episode.MP4",
      "7",
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

  it("rejects malformed rows, duplicate paths, traversal, unsupported files, and oversized sizes", async () => {
    for (const response of [
      [],
      ["invalid"],
      ["1", "/TV/Show.S01E02.mp4"],
      [
        "1",
        "/TV/Show.S01E02.mp4",
        "Show.S01E02.mp4",
        "1",
        "/TV/Show.S01E02.mp4",
        "Show.S01E02.mp4",
        "1",
      ],
      ["1", "/TV/Show.S01E02.mp4", "../Show.S01E02.mp4", "1"],
      ["1", "/TV/Show.S01E02.avi", "Show.S01E02.avi", "1"],
      [
        "1",
        "/TV/Show.S01E02.mp4",
        "Show.S01E02.mp4",
        "18446744073709551616",
      ],
    ]) {
      invokeMock.mockResolvedValueOnce(response);
      await expect(scanTvLibrary()).rejects.toThrow("invalid data");
    }
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
