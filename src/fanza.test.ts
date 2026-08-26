import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  fetchFanzaCatalog,
  fetchFanzaCoverObjectUrl,
  parseFanzaCatalogResponse,
  type FanzaCatalogItem,
  type FanzaCatalogRequest,
} from "./fanza";

const invoke = vi.fn();
const request: FanzaCatalogRequest = {
  category: "vr",
  feed: "popular",
  count: 10,
};

beforeEach(() => {
  invoke.mockReset();
  vi.stubGlobal("__TAURI__", { core: { invoke } });
  Object.defineProperty(URL, "createObjectURL", {
    configurable: true,
    value: vi.fn().mockReturnValue("blob:fanza-cover"),
  });
});

describe("FANZA catalog boundary", () => {
  it("accepts exact structured rows in provider order", () => {
    expect(
      parseFanzaCatalogResponse(
        [
          "8",
          "3",
          "vr",
          "13dsvr01947",
          "3DSVR-01947",
          "First title",
          "fanza-cover-8-1",
          "0.72",
          "vr",
          "vrkm01577",
          "VRKM-1577",
          "",
          "",
          "0.72",
          "vr",
          "ovvr616",
          "OVVR-616",
          "Third title",
          "fanza-cover-8-3",
          "0.72",
        ],
        request,
        "4",
      ),
    ).toEqual({
      status: "ready",
      items: [
        expect.objectContaining({
          contentId: "13dsvr01947",
          displayCode: "3DSVR-01947",
          title: "First title",
        }),
        expect.objectContaining({
          contentId: "vrkm01577",
          displayCode: "VRKM-1577",
          title: null,
          coverAuthorityId: null,
        }),
        expect.objectContaining({
          contentId: "ovvr616",
          displayCode: "OVVR-616",
        }),
      ],
    });
  });

  it("rejects over-return, invalid display identity, and duplicate content IDs", () => {
    expect(
      parseFanzaCatalogResponse(
        ["1", "11", ...Array.from({ length: 66 }, () => "")],
        request,
        "1",
      ),
    ).toEqual({ status: "malformed-provider" });
    expect(
      parseFanzaCatalogResponse(
        ["1", "1", "vr", "vrkm01577", "bad", "", "", "0.72"],
        request,
        "1",
      ),
    ).toEqual({ status: "malformed-provider" });
    expect(
      parseFanzaCatalogResponse(
        [
          "1", "2",
          "vr", "vrkm01577", "VRKM-1577", "", "", "0.72",
          "vr", "vrkm01577", "VRKM-1577", "", "", "0.72",
        ],
        request,
        "1",
      ),
    ).toEqual({ status: "malformed-provider" });
  });

  it("matches the native display-code prefix and number bounds", () => {
    for (const displayCode of ["AB-1", "123456789012345A-1234567890"]) {
      expect(
        parseFanzaCatalogResponse(
          ["1", "1", "vr", "exact1", displayCode, "", "", "0.72"],
          request,
          "1",
        ),
      ).toEqual({
        status: "ready",
        items: [expect.objectContaining({ displayCode })],
      });
    }

    for (const displayCode of [
      "A-12",
      "1234567890123456A-1",
      "AB1-2",
      "AB-0",
      "AB-12345678901",
    ]) {
      expect(
        parseFanzaCatalogResponse(
          ["1", "1", "vr", "exact1", displayCode, "", "", "0.72"],
          request,
          "1",
        ),
      ).toEqual({ status: "malformed-provider" });
    }
  });

  it("requires cover authority for the returned generation and row position", () => {
    for (const coverAuthorityId of [
      "fanza-cover-7-1",
      "fanza-cover-8-2",
      "fanza-cover-8-0",
      "fanza-cover-8-01",
      "fanza-cover-8-1-extra",
    ]) {
      expect(
        parseFanzaCatalogResponse(
          [
            "8",
            "1",
            "vr",
            "vrkm01577",
            "VRKM-1577",
            "",
            coverAuthorityId,
            "0.72",
          ],
          request,
          "4",
        ),
      ).toEqual({ status: "malformed-provider" });
    }

    expect(
      parseFanzaCatalogResponse(
        [
          "8", "2",
          "vr", "vrkm01577", "VRKM-1577", "", "fanza-cover-8-1", "0.72",
          "vr", "ovvr616", "OVVR-616", "", "fanza-cover-8-1", "0.72",
        ],
        request,
        "4",
      ),
    ).toEqual({ status: "malformed-provider" });
  });

  it("maps provider failures locally without changing the request", async () => {
    for (const [error, status] of [
      ["vr_source_unavailable", "source-unavailable"],
      ["vr_network_error", "network-error"],
      ["vr_fanza_malformed_provider", "malformed-provider"],
      ["vr_fanza_conflicting_provider", "conflicting-provider"],
      ["vr_fanza_stale", "stale"],
      ["vr_provider_error", "provider-error"],
    ] as const) {
      invoke.mockRejectedValueOnce(error);
      await expect(fetchFanzaCatalog(request, "9")).resolves.toEqual({
        status,
      });
    }
    expect(invoke).toHaveBeenCalledTimes(6);
    expect(invoke).toHaveBeenLastCalledWith("fetch_fanza_catalog", {
      ...request,
      contextGeneration: "9",
    });
  });

  it("submits opaque cover authority without a renderer URL", async () => {
    invoke.mockResolvedValue([
      0xff, 0xd8, 0xff, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ]);
    const item: FanzaCatalogItem = {
      category: "vr",
      contextGeneration: "4",
      requestGeneration: "8",
      contentId: "13dsvr01947",
      displayCode: "3DSVR-01947",
      title: null,
      coverAuthorityId: "fanza-cover-8-1",
      sourceAspectRatio: 0.72,
    };

    await expect(fetchFanzaCoverObjectUrl(item)).resolves.toBe(
      "blob:fanza-cover",
    );
    expect(invoke).toHaveBeenCalledWith("fetch_fanza_cover", {
      category: "vr",
      contextGeneration: "4",
      requestGeneration: "8",
      contentId: "13dsvr01947",
      displayCode: "3DSVR-01947",
      coverAuthorityId: "fanza-cover-8-1",
    });
    expect(invoke.mock.calls[0]?.[1]).not.toHaveProperty("url");
  });
});
