import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  fetchFanzaCatalog,
  fetchFanzaCoverObjectUrl,
  fetchFanzaDetail,
  fetchFanzaPreview,
  fetchFanzaPreviewImageObjectUrl,
  invalidateFanzaCatalog,
  invalidateFanzaPreview,
  openFanzaSource,
  parseFanzaCatalogResponse,
  type FanzaCatalogItem,
} from "./fanza";

let invokeMock: ReturnType<typeof vi.fn>;

const item: FanzaCatalogItem = {
  category: "vr",
  contextGeneration: "7",
  requestGeneration: "11",
  providerItemId: "13dsvr01947",
  code: "3DSVR-1947",
  title: "Exact provider title",
  coverUrl: null,
  coverAuthorityId: "fanza-cover-11-1",
  sourceAspectRatio: 0.72,
  source: "FANZA",
};

beforeEach(() => {
  invokeMock = vi.fn();
  vi.stubGlobal("__TAURI__", { core: { invoke: invokeMock } });
  vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:fanza-image");
  vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => undefined);
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("FANZA structured catalog boundary", () => {
  it("accepts exact native identities in source order with optional presentation fields", () => {
    expect(
      parseFanzaCatalogResponse(
        [
          "11",
          "2",
          "vr",
          "13dsvr01947",
          "3DSVR-1947",
          "Exact provider title",
          "fanza-cover-11-1",
          "0.72",
          "vr",
          "ovvr616",
          "OVVR-616",
          "",
          "",
          "0.72",
        ],
        "vr",
        "7",
        10,
      ),
    ).toEqual({
      status: "ready",
      items: [
        item,
        {
          category: "vr",
          contextGeneration: "7",
          requestGeneration: "11",
          providerItemId: "ovvr616",
          code: "OVVR-616",
          title: null,
          coverUrl: null,
          coverAuthorityId: null,
          sourceAspectRatio: 0.72,
          source: "FANZA",
        },
      ],
    });
  });

  it("rejects malformed, duplicate, cross-category, and noncanonical native responses", () => {
    for (const response of [
      ["11", "1", "adult", "13dsvr01947", "3DSVR-1947", "", "", "0.72"],
      ["11", "1", "vr", "13DSVR01947", "3DSVR-1947", "", "", "0.72"],
      ["11", "1", "vr", "13dsvr01947", "3DSVR-01947", "", "", "0.72"],
      ["11", "1", "vr", "ab12", "AB1-2", "", "", "0.72"],
      ["11", "1", "vr", "13dsvr01947", "3DSVR-1947", "", "fanza-cover-10-1", "0.72"],
      ["11", "2", "vr", "ovvr616", "OVVR-616", "", "", "0.72", "vr", "ovvr616", "OVVR-616", "", "", "0.72"],
    ]) {
      expect(parseFanzaCatalogResponse(response, "vr", "7", 10)).toEqual({
        status: "malformed-provider",
      });
    }
  });

  it("rejects a native response that exceeds the exact requested count", () => {
    const fields = Array.from({ length: 11 }, (_, index) => [
      "vr",
      `ovvr${index + 1}`,
      `OVVR-${index + 1}`,
      "",
      "",
      "0.72",
    ]).flat();

    expect(
      parseFanzaCatalogResponse(["11", "11", ...fields], "vr", "7", 10),
    ).toEqual({ status: "malformed-provider" });
  });

  it("submits only category feed count and generation and maps exact errors", async () => {
    invokeMock.mockRejectedValueOnce("adult_fanza_conflicting_provider");
    await expect(fetchFanzaCatalog("adult", "monthly", 100, "9")).resolves.toEqual({
      status: "conflicting-provider",
    });
    expect(invokeMock).toHaveBeenCalledWith("fetch_fanza_catalog", {
      category: "adult",
      feed: "monthly",
      count: 100,
      contextGeneration: "9",
    });
  });
});

describe("FANZA retained item authority", () => {
  it("fetches cover bytes without exposing or submitting a provider URL", async () => {
    invokeMock.mockResolvedValue([0xff, 0xd8, 0xff, 0x01]);
    await expect(fetchFanzaCoverObjectUrl(item)).resolves.toBe("blob:fanza-image");
    expect(invokeMock).toHaveBeenCalledWith("fetch_fanza_image", {
      category: "vr",
      contextGeneration: "7",
      requestGeneration: "11",
      providerItemId: "13dsvr01947",
      code: "3DSVR-1947",
      previewGeneration: null,
      imageAuthorityId: "fanza-cover-11-1",
    });
    expect(JSON.stringify(invokeMock.mock.calls)).not.toContain("https://");
  });

  it("requires an exact native detail echo and opens only the retained source item", async () => {
    invokeMock
      .mockResolvedValueOnce([
        "19",
        "vr",
        "7",
        "11",
        "13dsvr01947",
        "3DSVR-1947",
        "Exact provider title",
        "fanza-cover-11-1",
      ])
      .mockResolvedValueOnce(undefined);
    await expect(fetchFanzaDetail(item)).resolves.toEqual({
      status: "ready",
      item,
      detailGeneration: "19",
    });
    await openFanzaSource(item, "19");
    expect(invokeMock).toHaveBeenLastCalledWith("open_fanza_source", {
      category: "vr",
      contextGeneration: "7",
      requestGeneration: "11",
      providerItemId: "13dsvr01947",
      code: "3DSVR-1947",
      detailGeneration: "19",
    });
  });

  it("retains at most exact opaque preview authorities and submits no image URL", async () => {
    invokeMock
      .mockResolvedValueOnce(["17", "2", "fanza-preview-17-1", "fanza-preview-17-2"])
      .mockResolvedValueOnce([0x89, 0x50, 0x4e, 0x47]);
    await expect(fetchFanzaPreview(item, "19")).resolves.toEqual({
      status: "ready",
      previewGeneration: "17",
      authorityIds: ["fanza-preview-17-1", "fanza-preview-17-2"],
    });
    expect(invokeMock).toHaveBeenNthCalledWith(1, "fetch_fanza_preview", {
      category: "vr",
      contextGeneration: "7",
      requestGeneration: "11",
      providerItemId: "13dsvr01947",
      code: "3DSVR-1947",
      detailGeneration: "19",
    });
    await expect(
      fetchFanzaPreviewImageObjectUrl(item, "17", "fanza-preview-17-1"),
    ).resolves.toBe("blob:fanza-image");
    expect(invokeMock).toHaveBeenLastCalledWith("fetch_fanza_image", {
      category: "vr",
      contextGeneration: "7",
      requestGeneration: "11",
      providerItemId: "13dsvr01947",
      code: "3DSVR-1947",
      previewGeneration: "17",
      imageAuthorityId: "fanza-preview-17-1",
    });
  });

  it("removes a returned preview generation when its renderer response is malformed", async () => {
    invokeMock
      .mockResolvedValueOnce(["17", "2", "fanza-preview-17-1"])
      .mockResolvedValueOnce(undefined);

    await expect(fetchFanzaPreview(item, "19")).resolves.toEqual({
      status: "malformed-provider",
    });
    expect(invokeMock).toHaveBeenLastCalledWith("invalidate_fanza_preview", {
      category: "vr",
      previewGeneration: "17",
    });
  });

  it("invalidates only explicit current catalog and preview generations", async () => {
    invokeMock.mockResolvedValue(undefined);
    await invalidateFanzaCatalog("vr", "12");
    await invalidateFanzaPreview("vr", "17");
    expect(invokeMock.mock.calls).toEqual([
      ["invalidate_fanza_catalog", { category: "vr", contextGeneration: "12" }],
      ["invalidate_fanza_preview", { category: "vr", previewGeneration: "17" }],
    ]);
  });
});
