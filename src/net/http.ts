/**
 * HTTP wrapper for the TypeScript scrapers.
 *
 * This is the foundation for Phase 2 (replacing the Python sidecar). It wraps
 * `@tauri-apps/plugin-http`'s `fetch`, which runs the request through Rust:
 * that bypasses the webview's CORS wall and lets us set arbitrary request
 * headers (Referer / Cookie / a spoofed User-Agent), which is the whole reason
 * the Python sidecar existed.
 *
 * The plugin transparently handles gzip, so callers never deal with
 * Content-Encoding. Non-2xx responses are surfaced as a typed {@link HttpError}.
 *
 * `coverObjectUrl` replaces the sidecar's `/img` proxy: it fetches image bytes
 * (with a Referer for hotlink-protected hosts like pics.dmm.co.jp /
 * image.mgstage.com / javbus / cmastd) and hands back a `blob:` object URL an
 * `<img>` element can render directly.
 */
import { fetch } from "@tauri-apps/plugin-http"

/** Desktop Chrome UA used for every request unless overridden. */
export const DEFAULT_USER_AGENT =
  "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124 Safari/537.36"

/** UA the javdb mobile API expects (it is a Flutter/Dart client). */
export const JAVDB_USER_AGENT = "Dart/3.5 (dart:io)"

const DEFAULT_TIMEOUT_MS = 15_000

/** Options shared by every request helper. */
export interface HttpOptions {
  /** Sent as the `Referer` header (needed by hotlink-protected hosts). */
  referer?: string
  /** Sent as the `Cookie` header (e.g. a logged-in javbus / age-gate cookie). */
  cookie?: string
  /** Override the default desktop User-Agent (javdb needs {@link JAVDB_USER_AGENT}). */
  userAgent?: string
  /** Extra headers, merged last so they win over the derived ones. */
  headers?: Record<string, string>
  /** Request timeout in milliseconds. Defaults to 15000. */
  timeoutMs?: number
  /** HTTP method. Defaults to GET. */
  method?: string
  /** Request body (forwarded verbatim to the plugin fetch). */
  body?: BodyInit
}

/** Thrown for any non-2xx response or a network/transport failure. */
export class HttpError extends Error {
  /** HTTP status code, or 0 when the request never produced a response. */
  readonly status: number
  /** The URL that was requested. */
  readonly url: string
  /** Response body text, when one was available (truncated by the server). */
  readonly body?: string

  constructor(message: string, status: number, url: string, body?: string) {
    super(message)
    this.name = "HttpError"
    this.status = status
    this.url = url
    this.body = body
  }
}

/**
 * Build the header bag for a request from the high-level options. Exported for
 * unit testing the assembly logic without touching the network.
 *
 * Precedence (lowest → highest): default UA, then referer/cookie/userAgent,
 * then any explicit `headers` (so a caller can always override).
 */
export function buildHeaders(opts: HttpOptions = {}): Record<string, string> {
  const headers: Record<string, string> = {
    "User-Agent": opts.userAgent ?? DEFAULT_USER_AGENT,
    // No explicit Accept-Encoding: with the plugin's `unsafe-headers` feature
    // the header would actually reach reqwest, and a manually-set
    // Accept-Encoding DISABLES reqwest's automatic gzip decompression —
    // every body would come back as raw gzip bytes. Left unset, reqwest
    // negotiates compression and decodes transparently.
  }
  if (opts.referer) headers["Referer"] = opts.referer
  if (opts.cookie) headers["Cookie"] = opts.cookie
  if (opts.headers) {
    for (const [key, value] of Object.entries(opts.headers)) {
      headers[key] = value
    }
  }
  return headers
}

/**
 * Build the `init` object passed to the plugin `fetch`. Exported for testing.
 * Maps `timeoutMs` onto the plugin's `connectTimeout` ClientOption.
 */
export function buildRequestInit(
  opts: HttpOptions = {}
): RequestInit & { connectTimeout: number } {
  const init: RequestInit & { connectTimeout: number } = {
    method: opts.method ?? "GET",
    headers: buildHeaders(opts),
    connectTimeout: opts.timeoutMs ?? DEFAULT_TIMEOUT_MS,
  }
  if (opts.body !== undefined) init.body = opts.body
  return init
}

/** Run the request and throw {@link HttpError} on transport failure / non-2xx. */
async function request(url: string, opts: HttpOptions = {}): Promise<Response> {
  let res: Response
  try {
    res = await fetch(url, buildRequestInit(opts))
  } catch (cause) {
    const message = cause instanceof Error ? cause.message : String(cause)
    throw new HttpError(`network error for ${url}: ${message}`, 0, url)
  }
  if (!res.ok) {
    // Pull a bit of the body for diagnostics; ignore failures reading it.
    let body: string | undefined
    try {
      body = await res.text()
    } catch {
      body = undefined
    }
    throw new HttpError(
      `HTTP ${res.status} ${res.statusText} for ${url}`,
      res.status,
      url,
      body
    )
  }
  return res
}

/** GET (or `opts.method`) a URL and return the decoded text body. */
export async function httpText(
  url: string,
  opts: HttpOptions = {}
): Promise<string> {
  const res = await request(url, opts)
  return res.text()
}

/** GET (or `opts.method`) a URL and parse the JSON body as `T`. */
export async function httpJson<T>(
  url: string,
  opts: HttpOptions = {}
): Promise<T> {
  const res = await request(url, opts)
  return (await res.json()) as T
}

/** GET (or `opts.method`) a URL and return the raw bytes. */
export async function httpBytes(
  url: string,
  opts: HttpOptions = {}
): Promise<Uint8Array> {
  const res = await request(url, opts)
  const buf = await res.arrayBuffer()
  return new Uint8Array(buf)
}

// ---------------------------------------------------------------- cover proxy

/**
 * Derive the Referer a hotlink-protected image host expects, mirroring the
 * sidecar's `/img` host→referer map. Returns undefined when no special
 * Referer is required. Exported for testing.
 */
export function refererForImage(url: string): string | undefined {
  if (url.includes("dmm.co.jp")) return "https://www.dmm.co.jp/"
  if (url.includes("javdatabase")) return "https://www.javdatabase.com/"
  if (url.includes("javbus")) return "https://www.javbus.com/"
  if (url.includes("mgstage")) return "https://www.mgstage.com/"
  if (url.includes("cmastd") || url.includes("javdb")) return "https://javdb.com/"
  if (url.includes("yts")) return "https://yts.mx/"
  return undefined
}

/** In-memory cache of source URL → blob object URL. */
const coverCache = new Map<string, string>()

/**
 * javdb's cover CDN (tp.cmastd.com) serves images under a trivial single-byte
 * XOR: the FIRST byte of the payload is the key, and every subsequent byte is
 * `byte ^ key`. Decoding it yields the real JPEG/PNG/GIF. (Reverse-engineered
 * from the official app — see memory/javdb-cover-encryption-re.md.)
 */
export function isCmastdCover(url: string): boolean {
  return url.includes("cmastd.com")
}

/** Decrypt a cmastd payload: drop the key byte, XOR the rest with it. */
export function decryptCmastd(data: Uint8Array): Uint8Array {
  if (data.length < 2) return data
  const key = data[0]!
  const out = new Uint8Array(data.length - 1)
  for (let i = 1; i < data.length; i++) out[i - 1] = data[i]! ^ key
  return out
}

/**
 * Fetch image bytes (with a hotlink-bypassing Referer) and return a `blob:`
 * object URL that an `<img>` can render directly. Replaces the sidecar `/img`
 * passthrough. Results are cached by source URL; call {@link revokeCover} (or
 * {@link revokeAllCovers}) to release the underlying blob.
 *
 * If `opts.referer` is omitted, a host-appropriate Referer is derived via
 * {@link refererForImage}.
 */
export async function coverObjectUrl(
  url: string,
  opts: HttpOptions = {}
): Promise<string> {
  const cached = coverCache.get(url)
  if (cached) return cached

  const referer = opts.referer ?? refererForImage(url)
  const res = await request(url, { ...opts, referer })

  const raw = new Uint8Array(await res.arrayBuffer())
  // javdb covers are single-byte-XOR "encrypted"; decode to the real image.
  const bytes = isCmastdCover(url) ? decryptCmastd(raw) : raw

  // Some CDNs mislabel images as octet-stream; coerce to a sane image type.
  const contentType = res.headers.get("Content-Type") ?? ""
  const type =
    !isCmastdCover(url) && contentType.startsWith("image/")
      ? contentType
      : "image/jpeg"

  const blob = new Blob([bytes as BlobPart], { type })
  const objectUrl = URL.createObjectURL(blob)
  coverCache.set(url, objectUrl)
  return objectUrl
}

/** Revoke the cached blob URL for one source URL (no-op if not cached). */
export function revokeCover(url: string): void {
  const objectUrl = coverCache.get(url)
  if (objectUrl) {
    URL.revokeObjectURL(objectUrl)
    coverCache.delete(url)
  }
}

/** Revoke every cached cover blob URL and clear the cache. */
export function revokeAllCovers(): void {
  for (const objectUrl of coverCache.values()) {
    URL.revokeObjectURL(objectUrl)
  }
  coverCache.clear()
}

/** Whether a source URL currently has a cached cover blob. Exported for testing. */
export function hasCachedCover(url: string): boolean {
  return coverCache.has(url)
}
