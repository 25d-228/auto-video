/**
 * Seeders aggregator — TypeScript port of the sidecar's build_seeders()
 * (sidecar/av_proxy.py). Gathers real release rows for the Download dialog from
 * every relevant source, dedups by torrent infohash, and sorts by seeders desc.
 *
 * Source routing matches the Python exactly:
 *   - ad / vrc : sukebei + javdb (app-API magnets) + javbus (gated, user cookie)
 *   - mov      : apibay + yts
 *   - tv       : apibay
 *
 * Each source is best-effort: a network failure in one never sinks the others
 * (the Python wrapped javdb/javbus/yts in try/except and apibay/sukebei degrade
 * to []). The javdb step needs a code -> slug lookup the source module does not
 * expose, so it is implemented here against the same signed app API the javdb
 * source uses (javdbApi), then handed to javdbMagnets(slug).
 */
import { javdbApi, javdbMagnets } from "@/api/sources/javdb"
import { seedersSukebei } from "@/api/sources/sukebei"
import { seedersApibay } from "@/api/sources/tpb"
import { seedersYts } from "@/api/sources/yts"
import type { Cat, Release } from "@/api/types"
import { getKey, isDbAvailable } from "@/state/db"
import { httpText } from "@/net/http"
import { quality } from "@/lib/quality"

// ----------------------------------------------------------------- helpers

/**
 * Reproduce Python's urllib.parse.quote (default safe="/"): percent-encode every
 * byte except unreserved + "/". encodeURIComponent over-encodes "/", so we
 * un-escape just that one sequence back to a literal slash. (Used for the javbus
 * product-page path; every magnet here comes verbatim from its source.)
 */
function pyQuote(s: string): string {
  return encodeURIComponent(s ?? "").replace(/%2F/g, "/")
}

/** Minimal HTML-entity unescape for the few refs javbus emits in magnet links. */
function unescapeHtml(s: string): string {
  if (!s) return ""
  return s
    .replace(/&#(\d+);/g, (_m, d: string) => String.fromCodePoint(Number(d)))
    .replace(/&#x([0-9a-fA-F]+);/g, (_m, h: string) =>
      String.fromCodePoint(parseInt(h, 16))
    )
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&apos;/g, "'")
    .replace(/&nbsp;/g, " ")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&amp;/g, "&")
}

// ----------------------------------------------------------------- javdb

interface JdbSearch {
  movies?: { id?: string; number?: string }[]
}

/**
 * Resolve a JAV code to its javdb slug via the signed app-API search
 * (`/api/v2/search?q=<code>`), preferring an exact `number` match. Returns "" on
 * no hit / network error.
 *
 * NOTE: the sidecar's seeders_javdb relied on JDB_CODE2ID, a map filled lazily
 * from ranking feeds, so it only resolved codes already seen in a feed. The
 * search lookup here is strictly more capable while staying within the same API.
 */
async function javdbSlugForCode(code: string): Promise<string> {
  const data = await javdbApi<JdbSearch>(
    `/api/v2/search?q=${encodeURIComponent(code)}`
  )
  const movies = data?.movies ?? []
  if (movies.length === 0) return ""
  const want = code.toUpperCase()
  const exact = movies.find((m) => (m.number || "").toUpperCase() === want)
  const pick = exact ?? movies[0]
  return (pick?.id || "").trim()
}

/** Port of seeders_javdb(code): code -> slug -> app-API magnets. [] on miss. */
export async function seedersJavdb(code: string): Promise<Release[]> {
  if (!code) return []
  const slug = await javdbSlugForCode(code)
  if (!slug) return []
  return javdbMagnets(slug)
}

// ----------------------------------------------------------------- javbus

const JAVBUS_BASE = "https://www.javbus.com"

/**
 * Read one javbus URL with the user's verified cookie. Returns "" on failure.
 * Mirrors the sidecar's _jb_get (cookie + optional Referer).
 */
async function jbGet(url: string, cookie: string, ref?: string): Promise<string> {
  try {
    return await httpText(url, {
      cookie,
      ...(ref ? { referer: ref } : {}),
    })
  } catch {
    return ""
  }
}

/**
 * Port of seeders_javbus(code). javbus lists magnets (with sizes, no seeder
 * counts) via a gid/uc ajax endpoint behind the user's javbus cookie. The ajax
 * repeats each magnet across cells, so dedup by hash. Returns [] without a
 * cookie or when the product page has no gid.
 */
export async function seedersJavbus(
  code: string,
  cookie: string
): Promise<Release[]> {
  const ck = (cookie || "").trim()
  if (!code || !ck) return []
  const page = await jbGet(
    `${JAVBUS_BASE}/${pyQuote(code)}`,
    ck,
    `${JAVBUS_BASE}/`
  )
  const gidMatch = /gid\s*=\s*(\d+)/.exec(page)
  if (!gidMatch) return []
  const ucMatch = /\buc\s*=\s*(\d+)/.exec(page)
  const ajaxHtml = await jbGet(
    `${JAVBUS_BASE}/ajax/uncledatoolsbyajax.php?gid=${gidMatch[1]}&lang=en&img=&uc=${
      ucMatch ? ucMatch[1] : "0"
    }&floor=`,
    ck,
    `${JAVBUS_BASE}/${code}`
  )
  const out: Release[] = []
  const seen = new Set<string>()
  const re = /magnet:\?xt=urn:btih:([0-9a-fA-F]+)[^"'\s<]*/g
  let m: RegExpExecArray | null
  while ((m = re.exec(ajaxHtml)) !== null) {
    const infohash = m[1]!.toLowerCase()
    if (seen.has(infohash)) continue
    seen.add(infohash)
    const segment = ajaxHtml.slice(m.index, m.index + 500)
    const sizeMatch = /(\d+(?:\.\d+)?\s*[GM]B)/.exec(segment)
    out.push({
      name: code,
      source: "JavBus",
      seeders: 0,
      size: sizeMatch ? sizeMatch[1]! : "",
      magnet: unescapeHtml(m[0]),
      quality: quality(segment),
    })
  }
  return out
}

// ----------------------------------------------------------------- dedup / sort

/** Extract the lowercased btih hash from a magnet, or "" when absent. */
function btih(magnet: string): string {
  const m = /btih:([0-9a-fA-F]+)/.exec(magnet || "")
  return m ? m[1]!.toLowerCase() : ""
}

/**
 * Dedup by infohash (lowercased) — falling back to the release name when a row
 * has no hash — then sort by seeders desc. Pure (no network); exported for tests.
 * Port of the build_seeders tail (seen/uniq + sort).
 */
export function dedupeSort(rels: Release[]): Release[] {
  const seen = new Set<string>()
  const uniq: Release[] = []
  for (const r of rels) {
    const h = btih(r.magnet || "")
    const k = h || r.name
    if (seen.has(k)) continue
    seen.add(k)
    uniq.push(r)
  }
  uniq.sort((a, b) => (b.seeders || 0) - (a.seeders || 0))
  return uniq
}

// ----------------------------------------------------------------- read keys

/** Read the javbus cookie (provider key); "" when unset / DB unavailable. */
async function javbusCookie(): Promise<string> {
  if (!isDbAvailable()) return ""
  try {
    return (await getKey("javbus"))?.trim() ?? ""
  } catch {
    return ""
  }
}

// ----------------------------------------------------------------- public

/**
 * Aggregate real releases for one Discover item, for the Download dialog.
 *
 * @param cat   library category — picks the source set.
 * @param title display title (used as the apibay/sukebei query when no code).
 * @param code  JAV code (ad/vrc) — preferred query for the JAV sources.
 * @param year  release year (mov) — appended to the apibay query and filters YTS.
 * @returns deduped Release[] sorted by seeders desc.
 */
export async function seeders(
  cat: Cat,
  title: string,
  code: string,
  year?: string | number
): Promise<Release[]> {
  const rels: Release[] = []

  if (cat === "ad" || cat === "vrc") {
    const q = code || title
    // sukebei degrades to [] on failure internally.
    rels.push(...(await seedersSukebei(q)))
    // javdb + javbus are best-effort (the Python wrapped each in try/except).
    const cookie = await javbusCookie()
    const [jdb, jb] = await Promise.all([
      seedersJavdb(q).catch(() => [] as Release[]),
      seedersJavbus(q, cookie).catch(() => [] as Release[]),
    ])
    rels.push(...jdb, ...jb)
  } else {
    const q =
      cat === "mov" && year
        ? `${title} ${year}`.trim()
        : title || ""
    rels.push(...(await seedersApibay(q)))
    if (cat === "mov") {
      try {
        rels.push(...(await seedersYts(title, year)))
      } catch {
        // best-effort, matches the Python try/except around seeders_yts
      }
    }
  }

  return dedupeSort(rels)
}
