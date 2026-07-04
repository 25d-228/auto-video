import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

// Mock the Tauri http plugin BEFORE importing the module under test so the
// `import { fetch } from "@tauri-apps/plugin-http"` binding resolves to our spy.
const fetchMock = vi.fn()
vi.mock("@tauri-apps/plugin-http", () => ({
  fetch: (...args: unknown[]) => fetchMock(...args),
}))

import {
  buildHeaders,
  buildRequestInit,
  coverObjectUrl,
  DEFAULT_USER_AGENT,
  hasCachedCover,
  HttpError,
  httpBytes,
  httpJson,
  httpText,
  JAVDB_USER_AGENT,
  refererForImage,
  revokeAllCovers,
  revokeCover,
  decryptCmastd,
  isCmastdCover,
  looksLikeImage,
} from "@/net/http"

/** Build a minimal Response-like stub the helpers can consume. */
function makeResponse(opts: {
  ok?: boolean
  status?: number
  statusText?: string
  text?: string
  json?: unknown
  bytes?: Uint8Array
  contentType?: string
}): Response {
  const headers = new Map<string, string>()
  if (opts.contentType) headers.set("Content-Type", opts.contentType)
  return {
    ok: opts.ok ?? true,
    status: opts.status ?? 200,
    statusText: opts.statusText ?? "OK",
    headers: { get: (k: string) => headers.get(k) ?? null },
    text: async () => opts.text ?? "",
    json: async () => opts.json ?? {},
    arrayBuffer: async () =>
      (opts.bytes ?? new Uint8Array()).buffer as ArrayBuffer,
  } as unknown as Response
}

beforeEach(() => {
  fetchMock.mockReset()
})

describe("buildHeaders", () => {
  it("sets a desktop User-Agent and no explicit Accept-Encoding by default", () => {
    const h = buildHeaders()
    expect(h["User-Agent"]).toBe(DEFAULT_USER_AGENT)
    // An explicit Accept-Encoding would disable reqwest's transparent
    // gzip decompression now that unsafe-headers forwards it.
    expect(h["Accept-Encoding"]).toBeUndefined()
    expect(h["Referer"]).toBeUndefined()
    expect(h["Cookie"]).toBeUndefined()
  })

  it("allows overriding the User-Agent (javdb Dart UA)", () => {
    expect(buildHeaders({ userAgent: JAVDB_USER_AGENT })["User-Agent"]).toBe(
      JAVDB_USER_AGENT
    )
  })

  it("adds Referer and Cookie only when provided", () => {
    const h = buildHeaders({
      referer: "https://www.dmm.co.jp/",
      cookie: "age_check_done=1",
    })
    expect(h["Referer"]).toBe("https://www.dmm.co.jp/")
    expect(h["Cookie"]).toBe("age_check_done=1")
  })

  it("lets explicit headers win over derived ones", () => {
    const h = buildHeaders({
      userAgent: "from-ua-field",
      headers: { "User-Agent": "from-headers", "X-Custom": "1" },
    })
    expect(h["User-Agent"]).toBe("from-headers")
    expect(h["X-Custom"]).toBe("1")
  })
})

describe("buildRequestInit", () => {
  it("defaults to GET with a 15s connectTimeout and no body", () => {
    const init = buildRequestInit()
    expect(init.method).toBe("GET")
    expect(init.connectTimeout).toBe(15_000)
    expect("body" in init).toBe(false)
  })

  it("maps timeoutMs onto connectTimeout and forwards method/body", () => {
    const init = buildRequestInit({
      method: "POST",
      body: "payload",
      timeoutMs: 4_000,
    })
    expect(init.method).toBe("POST")
    expect(init.body).toBe("payload")
    expect(init.connectTimeout).toBe(4_000)
  })
})

describe("refererForImage", () => {
  it("maps hotlink-protected hosts to their expected Referer", () => {
    expect(refererForImage("https://pics.dmm.co.jp/x/y.jpg")).toBe(
      "https://www.dmm.co.jp/"
    )
    expect(refererForImage("https://image.mgstage.com/a.jpg")).toBe(
      "https://www.mgstage.com/"
    )
    expect(refererForImage("https://www.javbus.com/c.jpg")).toBe(
      "https://www.javbus.com/"
    )
    expect(refererForImage("https://tp.cmastd.com/d.jpg")).toBe(
      "https://javdb.com/"
    )
    expect(refererForImage("https://tp.spfcas.com/d.jpg")).toBe(
      "https://javdb.com/"
    )
    expect(refererForImage("https://www.javdatabase.com/e.jpg")).toBe(
      "https://www.javdatabase.com/"
    )
    expect(refererForImage("https://yts.mx/f.jpg")).toBe("https://yts.mx/")
  })

  it("returns undefined for hosts that need no special Referer", () => {
    expect(refererForImage("https://image.tmdb.org/t/p/w500/x.jpg")).toBeUndefined()
  })
})

describe("httpText / httpJson / httpBytes", () => {
  it("passes the assembled init to the plugin fetch and returns the text", async () => {
    fetchMock.mockResolvedValue(makeResponse({ text: "hello" }))
    const out = await httpText("https://apidd.spthgb.com/api/v1/x", {
      userAgent: JAVDB_USER_AGENT,
      headers: { jdsignature: "sig" },
    })
    expect(out).toBe("hello")
    expect(fetchMock).toHaveBeenCalledTimes(1)
    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit & { connectTimeout: number }]
    expect(url).toBe("https://apidd.spthgb.com/api/v1/x")
    const headers = init.headers as Record<string, string>
    expect(headers["User-Agent"]).toBe(JAVDB_USER_AGENT)
    expect(headers["jdsignature"]).toBe("sig")
  })

  it("parses JSON bodies as the requested type", async () => {
    fetchMock.mockResolvedValue(makeResponse({ json: { success: 1 } }))
    const out = await httpJson<{ success: number }>("https://h/api/v1/y")
    expect(out.success).toBe(1)
  })

  it("returns raw bytes as a Uint8Array", async () => {
    const bytes = new Uint8Array([1, 2, 3])
    fetchMock.mockResolvedValue(makeResponse({ bytes }))
    const out = await httpBytes("https://h/api/v1/z")
    expect(Array.from(out)).toEqual([1, 2, 3])
  })
})

describe("error handling", () => {
  it("throws a typed HttpError on non-2xx, capturing status/url/body", async () => {
    fetchMock.mockResolvedValue(
      makeResponse({ ok: false, status: 404, statusText: "Not Found", text: "nope" })
    )
    await expect(httpText("https://h/api/v1/missing")).rejects.toMatchObject({
      name: "HttpError",
      status: 404,
      url: "https://h/api/v1/missing",
      body: "nope",
    })
  })

  it("wraps transport failures as HttpError with status 0", async () => {
    fetchMock.mockRejectedValue(new Error("connection refused"))
    const err = await httpText("https://h/api/v1/down").catch((e) => e)
    expect(err).toBeInstanceOf(HttpError)
    expect((err as HttpError).status).toBe(0)
    expect((err as HttpError).message).toContain("connection refused")
  })
})

describe("coverObjectUrl", () => {
  const created: string[] = []
  const revoked: string[] = []

  beforeEach(() => {
    let counter = 0
    vi.stubGlobal("URL", {
      createObjectURL: vi.fn(() => {
        const u = `blob:mock-${counter++}`
        created.push(u)
        return u
      }),
      revokeObjectURL: vi.fn((u: string) => {
        revoked.push(u)
      }),
    })
    vi.stubGlobal(
      "Blob",
      class {
        constructor(
          public parts: unknown[],
          public opts: { type?: string }
        ) {}
      }
    )
    // Clear any covers cached by a previous test FIRST (so those revokes are
    // not recorded against this test's tracking arrays), then reset tracking.
    revokeAllCovers()
    created.length = 0
    revoked.length = 0
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it("derives a Referer for hotlink hosts and returns a blob URL", async () => {
    fetchMock.mockResolvedValue(
      makeResponse({ bytes: new Uint8Array([9]), contentType: "image/jpeg" })
    )
    const url = "https://pics.dmm.co.jp/x/cover.jpg"
    const objectUrl = await coverObjectUrl(url)
    expect(objectUrl).toBe("blob:mock-0")
    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    const headers = init.headers as Record<string, string>
    expect(headers["Referer"]).toBe("https://www.dmm.co.jp/")
  })

  it("caches by source url (single fetch) and revokeCover releases it", async () => {
    fetchMock.mockResolvedValue(
      makeResponse({ bytes: new Uint8Array([1]), contentType: "image/png" })
    )
    const src = "https://image.tmdb.org/t/p/w500/a.jpg"
    const a = await coverObjectUrl(src)
    const b = await coverObjectUrl(src)
    expect(a).toBe(b)
    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(hasCachedCover(src)).toBe(true)

    revokeCover(src)
    expect(revoked).toEqual([a])
    expect(hasCachedCover(src)).toBe(false)
  })

  it("revokeAllCovers releases every cached blob", async () => {
    fetchMock.mockResolvedValue(makeResponse({ bytes: new Uint8Array([1]) }))
    await coverObjectUrl("https://image.tmdb.org/1.jpg")
    await coverObjectUrl("https://image.tmdb.org/2.jpg")
    revokeAllCovers()
    expect(revoked.length).toBe(2)
    expect(hasCachedCover("https://image.tmdb.org/1.jpg")).toBe(false)
  })

  it("lets an explicit referer override the derived one", async () => {
    fetchMock.mockResolvedValue(makeResponse({ bytes: new Uint8Array([1]) }))
    await coverObjectUrl("https://pics.dmm.co.jp/x.jpg", {
      referer: "https://custom.example/",
    })
    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect((init.headers as Record<string, string>)["Referer"]).toBe(
      "https://custom.example/"
    )
  })
})

describe("looksLikeImage", () => {
  const pad = (magic: number[]) => Uint8Array.from([...magic, ...Array(12).fill(0)])

  it("accepts the common raster magics", () => {
    expect(looksLikeImage(pad([0xff, 0xd8, 0xff, 0xe0]))).toBe(true) // JPEG
    expect(looksLikeImage(pad([0x89, 0x50, 0x4e, 0x47]))).toBe(true) // PNG
    expect(looksLikeImage(pad([0x47, 0x49, 0x46, 0x38]))).toBe(true) // GIF
    const webp = pad([0x52, 0x49, 0x46, 0x46, 0, 0, 0, 0, 0x57, 0x45, 0x42, 0x50])
    expect(looksLikeImage(webp)).toBe(true)
  })

  it("rejects HTML, garbage, and too-short buffers", () => {
    expect(looksLikeImage(new TextEncoder().encode("<html><body>challenge</body></html>"))).toBe(false)
    expect(looksLikeImage(pad([0x00, 0x01, 0x02, 0x03]))).toBe(false)
    expect(looksLikeImage(Uint8Array.from([0xff, 0xd8, 0xff]))).toBe(false)
  })
})

describe("coverObjectUrl retry & cmastd validation", () => {
  const TP_URL = "https://tp.spfcas.com/rhe951l4q/covers/pk/pkkQWe.jpg"
  // A ≥12-byte JPEG header, XOR-"encrypted" the way the tp.* CDN serves it:
  // key byte first, every following byte is plain ^ key.
  const PLAIN_JPEG = Uint8Array.from([
    0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, 0x4a, 0x46, 0x49, 0x46, 0x00, 0x01,
  ])
  const KEY = 0x5a
  const ENC_JPEG = Uint8Array.from([KEY, ...Array.from(PLAIN_JPEG, (b) => b ^ KEY)])

  beforeEach(() => {
    vi.useFakeTimers()
    let counter = 0
    vi.stubGlobal("URL", {
      createObjectURL: vi.fn(() => `blob:mock-${counter++}`),
      revokeObjectURL: vi.fn(),
    })
    vi.stubGlobal(
      "Blob",
      class {
        constructor(
          public parts: unknown[],
          public opts: { type?: string }
        ) {}
      }
    )
    revokeAllCovers()
  })

  afterEach(() => {
    vi.useRealTimers()
    vi.unstubAllGlobals()
  })

  it("retries a dropped connection after the 400ms backoff and caches the success", async () => {
    fetchMock
      .mockRejectedValueOnce(new Error("connection reset"))
      .mockResolvedValueOnce(makeResponse({ bytes: ENC_JPEG }))
    const pending = coverObjectUrl(TP_URL)
    // Lower bound: no second attempt before the 400ms backoff has elapsed.
    await vi.advanceTimersByTimeAsync(399)
    expect(fetchMock).toHaveBeenCalledTimes(1)
    await vi.advanceTimersByTimeAsync(1)
    const out = await pending
    expect(out).toBe("blob:mock-0")
    expect(fetchMock).toHaveBeenCalledTimes(2)
    expect(hasCachedCover(TP_URL)).toBe(true)
    // Covers use the short 5s connect timeout, not the default 15s.
    const [, init] = fetchMock.mock.calls[0] as [string, { connectTimeout: number }]
    expect(init.connectTimeout).toBe(5_000)
  })

  it("backs off 400ms then 800ms across three attempts", async () => {
    fetchMock
      .mockRejectedValueOnce(new Error("reset 1"))
      .mockRejectedValueOnce(new Error("reset 2"))
      .mockResolvedValueOnce(makeResponse({ bytes: ENC_JPEG }))
    const pending = coverObjectUrl("https://tp.spfcas.com/twice.jpg")
    await vi.advanceTimersByTimeAsync(400)
    expect(fetchMock).toHaveBeenCalledTimes(2)
    // Second backoff is 800ms (400 * attempt), not another 400.
    await vi.advanceTimersByTimeAsync(799)
    expect(fetchMock).toHaveBeenCalledTimes(2)
    await vi.advanceTimersByTimeAsync(1)
    await expect(pending).resolves.toMatch(/^blob:/)
    expect(fetchMock).toHaveBeenCalledTimes(3)
  })

  it("retries a connection dropped mid-body (arrayBuffer rejects a plain Error)", async () => {
    const broken = makeResponse({}) as { arrayBuffer(): Promise<ArrayBuffer> }
    broken.arrayBuffer = async () => {
      throw new Error("stream reset")
    }
    fetchMock
      .mockResolvedValueOnce(broken)
      .mockResolvedValueOnce(makeResponse({ bytes: ENC_JPEG }))
    const pending = coverObjectUrl("https://tp.spfcas.com/midbody.jpg")
    await vi.advanceTimersByTimeAsync(400)
    await expect(pending).resolves.toMatch(/^blob:/)
    expect(fetchMock).toHaveBeenCalledTimes(2)
  })

  it("retries 429 and 5xx responses", async () => {
    fetchMock
      .mockResolvedValueOnce(makeResponse({ ok: false, status: 429, statusText: "Too Many" }))
      .mockResolvedValueOnce(makeResponse({ ok: false, status: 503, statusText: "Unavailable" }))
      .mockResolvedValueOnce(makeResponse({ bytes: ENC_JPEG }))
    const pending = coverObjectUrl("https://tp.spfcas.com/limited.jpg")
    await vi.advanceTimersByTimeAsync(400 + 800)
    await expect(pending).resolves.toMatch(/^blob:/)
    expect(fetchMock).toHaveBeenCalledTimes(3)
  })

  it("fails fast on genuine 4xx (403/404, no retry)", async () => {
    fetchMock.mockResolvedValue(
      makeResponse({ ok: false, status: 404, statusText: "Not Found" })
    )
    await expect(coverObjectUrl("https://tp.spfcas.com/gone.jpg")).rejects.toMatchObject(
      { name: "HttpError", status: 404 }
    )
    expect(fetchMock).toHaveBeenCalledTimes(1)

    fetchMock.mockReset()
    fetchMock.mockResolvedValue(
      makeResponse({ ok: false, status: 403, statusText: "Forbidden" })
    )
    await expect(coverObjectUrl("https://tp.spfcas.com/blocked.jpg")).rejects.toMatchObject(
      { name: "HttpError", status: 403 }
    )
    expect(fetchMock).toHaveBeenCalledTimes(1)
  })

  it("fails fast on a 200 challenge page (synthetic 415) and never caches the garbage", async () => {
    const html = new TextEncoder().encode(
      "<html><body>Checking your browser before accessing…</body></html>"
    )
    fetchMock.mockResolvedValue(makeResponse({ bytes: html }))
    const url = "https://tp.spfcas.com/challenged.jpg"
    await expect(coverObjectUrl(url)).rejects.toMatchObject({
      name: "HttpError",
      status: 415,
    })
    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(hasCachedCover(url)).toBe(false)

    // The next attempt (fresh fetch) recovers — nothing was poisoned.
    fetchMock.mockReset()
    fetchMock.mockResolvedValue(makeResponse({ bytes: ENC_JPEG }))
    await expect(coverObjectUrl(url)).resolves.toMatch(/^blob:/)
    expect(hasCachedCover(url)).toBe(true)
  })

  it("does not magic-check non-cmastd hosts (TMDB bytes pass through)", async () => {
    fetchMock.mockResolvedValue(
      makeResponse({ bytes: new Uint8Array([9]), contentType: "image/png" })
    )
    await expect(
      coverObjectUrl("https://image.tmdb.org/t/p/w500/x.jpg")
    ).resolves.toMatch(/^blob:/)
    expect(fetchMock).toHaveBeenCalledTimes(1)
  })
})

describe("cmastd cover decryption", () => {
  it("detects cmastd hosts", () => {
    expect(isCmastdCover("https://tp.cmastd.com/x/covers/aa/Ab.jpg")).toBe(true)
    // The CDN rotates (cmastd -> spfcas -> ...); match the stable `tp.` host prefix.
    expect(isCmastdCover("https://tp.spfcas.com/rhe951l4q/covers/2m/2mrnqX.jpg")).toBe(true)
    expect(isCmastdCover("https://pics.dmm.co.jp/x.jpg")).toBe(false)
    expect(isCmastdCover("https://image.tmdb.org/t/p/w500/x.jpg")).toBe(false)
  })

  it("decrypts the single-byte XOR (first byte is the key)", () => {
    // Encrypt a known JPEG header with key K, prepended: [K] + (plain ^ K).
    const plain = Uint8Array.from([0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10])
    const K = 0xad
    const enc = Uint8Array.from([K, ...Array.from(plain, (b) => b ^ K)])
    expect(Array.from(decryptCmastd(enc))).toEqual(Array.from(plain))
  })

  it("returns the input unchanged when too short to hold a key + payload", () => {
    const tiny = Uint8Array.from([0x12])
    expect(decryptCmastd(tiny)).toBe(tiny)
  })
})
