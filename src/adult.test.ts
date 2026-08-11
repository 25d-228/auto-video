import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  chooseAdultFolder,
  loadAdultFolder,
  openAdultFile,
  queryAdultStorage,
  revealAdultFile,
  scanAdultLibrary,
  trashAdultFile,
} from "./adult";

let invokeMock: ReturnType<typeof vi.fn>;

beforeEach(() => {
  invokeMock = vi.fn();
  vi.stubGlobal("__TAURI__", { core: { invoke: invokeMock } });
});

describe("Adult folder and storage boundaries", () => {
  it("loads every folder state while preserving its exact native path", async () => {
    invokeMock.mockResolvedValueOnce(["unconfigured"]);
    await expect(loadAdultFolder()).resolves.toEqual({ status: "unconfigured" });
    invokeMock.mockResolvedValueOnce(["ready", "/Adult/作品  Library"]);
    await expect(loadAdultFolder()).resolves.toEqual({
      status: "ready",
      path: "/Adult/作品  Library",
    });
    invokeMock.mockResolvedValueOnce(["unavailable", "/Adult/Missing"]);
    await expect(loadAdultFolder()).resolves.toEqual({
      status: "unavailable",
      path: "/Adult/Missing",
    });
  });

  it("preserves selection cancellation and rejects malformed native data", async () => {
    invokeMock.mockResolvedValueOnce("/Adult/Folder  A");
    await expect(chooseAdultFolder()).resolves.toBe("/Adult/Folder  A");
    invokeMock.mockResolvedValueOnce(null);
    await expect(chooseAdultFolder()).resolves.toBeNull();
    invokeMock.mockResolvedValueOnce(["ready", ""]);
    await expect(loadAdultFolder()).rejects.toThrow("invalid data");
    invokeMock.mockResolvedValueOnce(["100", "101"]);
    await expect(queryAdultStorage()).rejects.toThrow("invalid data");
  });
});

describe("conservative parsed Adult Library identity", () => {
  it("groups equivalent ADLT-123 variants without accepting nearby or mixed identities", async () => {
    const firstPath = "/Adult/作品/ADLT-123 Part 01 — 前編.mp4";
    const secondPath = "/Adult/adlt_00123_CD2  特別版.MKV";
    const mixedPath = "/Adult/ADLT-123 + XYZ-7  pack.mp4";
    const vrOnlyPartPath = "/Adult/ADLT-123 + PT-7.mp4";
    const conflictingPath = "/Adult/ADLT-123 ADLT-124.mp4";
    const noCodePath = "/Adult/作品  without code.mp4";
    invokeMock.mockResolvedValue([
      "7",
      firstPath,
      "作品/ADLT-123 Part 01 — 前編.mp4",
      "10",
      secondPath,
      "adlt_00123_CD2  特別版.MKV",
      "20",
      "/Adult/ADLT-124.mp4",
      "ADLT-124.mp4",
      "30",
      "/Adult/ADLT-1230.mp4",
      "ADLT-1230.mp4",
      "40",
      "/Adult/XADLT-123.mp4",
      "XADLT-123.mp4",
      "50",
      mixedPath,
      "ADLT-123 + XYZ-7  pack.mp4",
      "60",
      vrOnlyPartPath,
      "ADLT-123 + PT-7.mp4",
      "65",
      conflictingPath,
      "ADLT-123 ADLT-124.mp4",
      "70",
      noCodePath,
      "作品  without code.mp4",
      "80",
    ]);

    const { generation, items } = await scanAdultLibrary();

    expect(generation).toBe("7");
    expect(items.find((item) => item.code === "ADLT-123")).toEqual({
      id: "code:ADLT-123",
      title: "ADLT-123",
      code: "ADLT-123",
      files: [
        {
          path: firstPath,
          relativePath: "作品/ADLT-123 Part 01 — 前編.mp4",
          filename: "ADLT-123 Part 01 — 前編.mp4",
          title: "ADLT-123 Part 01 — 前編",
          sizeBytes: "10",
          partLabel: "Part 01",
        },
        {
          path: secondPath,
          relativePath: "adlt_00123_CD2  特別版.MKV",
          filename: "adlt_00123_CD2  特別版.MKV",
          title: "adlt_00123_CD2  特別版",
          sizeBytes: "20",
          partLabel: "CD2",
        },
      ],
    });
    expect(items.find((item) => item.code === "ADLT-124")?.files).toHaveLength(1);
    expect(items.find((item) => item.code === "ADLT-1230")?.files).toHaveLength(1);
    expect(items.find((item) => item.code === "XADLT-123")?.files).toHaveLength(1);
    for (const path of [
      mixedPath,
      vrOnlyPartPath,
      conflictingPath,
      noCodePath,
    ]) {
      expect(items.find((item) => item.id === `file:${path}`)?.code).toBeNull();
    }
  });

  it("shows only one exact unambiguous multipart label without inventing order", async () => {
    invokeMock.mockResolvedValue([
      "8",
      "/Adult/ADLT-777 Disk 03.mp4",
      "ADLT-777 Disk 03.mp4",
      "3",
      "/Adult/ADLT-777 Part 01 Disc 02.mkv",
      "ADLT-777 Part 01 Disc 02.mkv",
      "4",
      "/Adult/ADLT-777 finale.mp4",
      "ADLT-777 finale.mp4",
      "5",
      "/Adult/ADLT-777 Part 1-2.mp4",
      "ADLT-777 Part 1-2.mp4",
      "6",
      "/Adult/ADLT-777 CD1+2.mkv",
      "ADLT-777 CD1+2.mkv",
      "7",
    ]);

    const { items } = await scanAdultLibrary();

    expect(items).toHaveLength(1);
    expect(items[0].files.map((file) => [file.filename, file.partLabel])).toEqual([
      ["ADLT-777 Disk 03.mp4", "Disk 03"],
      ["ADLT-777 Part 01 Disc 02.mkv", null],
      ["ADLT-777 finale.mp4", null],
      ["ADLT-777 Part 1-2.mp4", null],
      ["ADLT-777 CD1+2.mkv", null],
    ]);
  });

  it("rejects malformed rows, duplicate paths, traversal, unsupported files, and oversized sizes", async () => {
    for (const response of [
      [],
      ["invalid"],
      ["/Adult/ADLT-123.mp4"],
      [
        "1",
        "/Adult/ADLT-123.mp4",
        "ADLT-123.mp4",
        "1",
        "/Adult/ADLT-123.mp4",
        "ADLT-123-copy.mp4",
        "1",
      ],
      ["1", "/Adult/ADLT-123.mp4", "../ADLT-123.mp4", "1"],
      ["1", "/Adult/ADLT-123.txt", "ADLT-123.txt", "1"],
      [
        "1",
        "/Adult/ADLT-123.mp4",
        "ADLT-123.mp4",
        "18446744073709551616",
      ],
    ]) {
      invokeMock.mockResolvedValueOnce(response);
      await expect(scanAdultLibrary()).rejects.toThrow("invalid data");
    }
  });

  it("accepts a generation-only response as an empty trusted scan", async () => {
    invokeMock.mockResolvedValue(["6"]);

    await expect(scanAdultLibrary()).resolves.toEqual({
      generation: "6",
      items: [],
    });
  });
});

describe("Adult file actions", () => {
  it("dispatches exact paths to isolated native commands", async () => {
    invokeMock.mockResolvedValue(undefined);

    await openAdultFile("/Adult/作品  ADLT-123.mp4");
    await revealAdultFile("/Adult/作品  ADLT-123.mp4");
    await trashAdultFile("/Adult/作品  ADLT-123.mp4", "9");

    expect(invokeMock).toHaveBeenNthCalledWith(1, "open_adult_file", {
      path: "/Adult/作品  ADLT-123.mp4",
    });
    expect(invokeMock).toHaveBeenNthCalledWith(2, "reveal_adult_file", {
      path: "/Adult/作品  ADLT-123.mp4",
    });
    expect(invokeMock).toHaveBeenNthCalledWith(3, "trash_adult_file", {
      path: "/Adult/作品  ADLT-123.mp4",
      scanGeneration: "9",
    });
  });
});
