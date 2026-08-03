import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  canonicalizeProductCode,
  fetchExactJavdbVrItem,
  fetchVerifiedSukebeiReleases,
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
