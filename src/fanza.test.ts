import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  fetchFanzaCatalog,
  fetchFanzaCoverObjectUrl,
  invalidateFanzaCatalog,
  parseFanzaCatalogResponse,
  type FanzaCatalogItem,
  type FanzaCatalogRequest,
} from "./fanza";

const request: FanzaCatalogRequest = {
  category: "vr",
  feed: "popular",
  count: 10,
};

const item: FanzaCatalogItem = {
  category: "vr",
  contextGeneration: "7",
  requestGeneration: "11",
  providerItemId: "13dsvr01947",
  code: "3DSVR-1947",
  title: "Exact title",
  coverUrl: null,
  coverAuthorityId: "fanza-cover-11-1",
  sourceAspectRatio: 0.72,
  source: "FANZA",
};

describe("trusted FANZA catalog boundary", () => {
  beforeEach(() => {
    window.__TAURI__ = {
      core: { invoke: vi.fn() },
    };
    vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:fanza-cover");
  });

  it("accepts exact native-owned identities in provider order", () => {
    expect(
      parseFanzaCatalogResponse(
        [
          "11",
          "2",
          "vr",
          "13dsvr01947",
          "3DSVR-1947",
          "First",
          "fanza-cover-11-1",
          "0.72",
          "vr",
          "ovvr616",
          "OVVR-616",
          "",
          "",
          "0.72",
        ],
        request,
        "7",
      ),
    ).toEqual({
      status: "ready",
      items: [
        { ...item, title: "First" },
        {
          ...item,
          providerItemId: "ovvr616",
          code: "OVVR-616",
          title: null,
          coverAuthorityId: null,
        },
      ],
    });
  });

  it("rejects over-returned, cross-category, duplicate, and unsupported identities", () => {
    expect(
      parseFanzaCatalogResponse(["11", "11", ...Array(66).fill("")], request, "7"),
    ).toEqual({ status: "malformed-provider" });
    for (const fields of [
      ["adult", "13dsvr01947", "3DSVR-1947"],
      ["vr", "ab12", "AB1-2"],
    ]) {
      expect(
        parseFanzaCatalogResponse(
          ["11", "1", ...fields, "", "", "0.72"],
          request,
          "7",
        ),
      ).toEqual({ status: "malformed-provider" });
    }
  });

  it("submits only the bounded request and maps provider failures locally", async () => {
    vi.mocked(window.__TAURI__.core.invoke)
      .mockResolvedValueOnce(["11", "0"])
      .mockRejectedValueOnce("vr_source_unavailable")
      .mockRejectedValueOnce("vr_network_error")
      .mockRejectedValueOnce("vr_fanza_malformed_provider")
      .mockRejectedValueOnce("vr_fanza_conflicting_provider")
      .mockRejectedValueOnce("vr_provider_error")
      .mockRejectedValueOnce("vr_fanza_stale");
    await expect(fetchFanzaCatalog(request, "7")).resolves.toEqual({
      status: "ready",
      items: [],
    });
    expect(window.__TAURI__.core.invoke).toHaveBeenNthCalledWith(
      1,
      "fetch_fanza_catalog",
      { ...request, contextGeneration: "7" },
    );
    for (const status of [
      "source-unavailable",
      "network-error",
      "malformed-provider",
      "conflicting-provider",
      "provider-error",
      "stale",
    ]) {
      await expect(fetchFanzaCatalog(request, "8")).resolves.toEqual({ status });
    }
  });

  it("fetches a cover only through the exact opaque item authority", async () => {
    vi.mocked(window.__TAURI__.core.invoke).mockResolvedValue([
      0xff, 0xd8, 0xff, 0xe0, 0, 0, 0, 0, 0, 0, 0, 0,
    ]);
    await expect(fetchFanzaCoverObjectUrl(item)).resolves.toBe("blob:fanza-cover");
    expect(window.__TAURI__.core.invoke).toHaveBeenCalledWith(
      "fetch_fanza_cover",
      {
        category: "vr",
        contextGeneration: "7",
        requestGeneration: "11",
        providerItemId: "13dsvr01947",
        code: "3DSVR-1947",
        coverAuthorityId: "fanza-cover-11-1",
      },
    );
  });

  it("rejects forged cover authority before native dispatch", async () => {
    await expect(
      fetchFanzaCoverObjectUrl({
        ...item,
        coverAuthorityId: "https://awsimgsrc.dmm.co.jp/forged.jpg",
      }),
    ).rejects.toThrow("A current FANZA cover authority is required.");
    expect(window.__TAURI__.core.invoke).not.toHaveBeenCalled();
  });

  it("invalidates only a sequenced category context", async () => {
    vi.mocked(window.__TAURI__.core.invoke).mockResolvedValue(undefined);
    await invalidateFanzaCatalog("adult", "9");
    expect(window.__TAURI__.core.invoke).toHaveBeenCalledWith(
      "invalidate_fanza_catalog",
      { category: "adult", contextGeneration: "9" },
    );
    await expect(invalidateFanzaCatalog("adult", "0")).rejects.toThrow(
      "A valid FANZA catalog context is required.",
    );
  });
});
