/**
 * LIVE audit: for each movie/TV item, is the TMDB match EXACT (title matches)
 * or only via the fuzzy year-fallback? Flags non-exact items (rename candidates
 * for "impeccable names"). Read-only (queries TMDB). Skipped unless RUN_LIVE=1.
 */
import { appendFileSync, readdirSync, statSync, writeFileSync } from "node:fs"
import { join } from "node:path"
import { beforeAll, describe, it, vi } from "vitest"
import type { Cat } from "@/api/types"

const OUT = process.env.LIVE_OUT ?? "/tmp/av_title_audit.jsonl"
vi.mock("@tauri-apps/api/core", () => ({ invoke: async () => { throw new Error("no invoke") } }))
vi.mock("@/state/db", () => ({
  isDbAvailable: () => true, getKey: async () => null, allPaths: async () => ({}),
  getCached: async () => null, setCached: async () => undefined,
  getCachedCover: async () => null, setCachedCover: async () => undefined,
}))
import { buildItems } from "@/api/library"

const KEY = process.env.TMDB_KEY ?? ""
const ROOTS: Partial<Record<Cat, string>> = { mov: "/Volumes/Be/films", tv: "/Volumes/Be/series" }
const VIDEO = new Set(["mkv","mp4","avi","wmv","m4v","ts","mov","flv","iso","rmvb","webm","mpg","mpeg"])

interface SF { name: string; path: string; size: number }
function walk(dir: string, out: SF[]): void {
  let ents; try { ents = readdirSync(dir, { withFileTypes: true }) } catch { return }
  for (const e of ents) {
    if (e.name.startsWith(".")) continue
    const p = join(dir, e.name)
    if (e.isDirectory()) walk(p, out)
    else if (e.isFile() && VIDEO.has(e.name.split(".").pop()?.toLowerCase() ?? "")) {
      let size = 0; try { size = statSync(p).size } catch { /**/ }
      out.push({ name: e.name, path: p, size })
    }
  }
}
const norm = (s: string) => (s||"").normalize("NFC").toLowerCase().replace(/[^\p{L}\p{N}]+/gu,"")
const tmatch = (a: string, b: string) => { const x=norm(a),y=norm(b); return !!x&&!!y&&(x===y||x.includes(y)||y.includes(x)) }

async function search(kind: string, q: string, year: string) {
  const yk = kind === "tv" ? "first_air_date_year" : "year"
  for (const params of [year ? `&${yk}=${year}` : "", ""]) {
    if (params === "" && year) {} // try year then bare
    const url = `https://api.themoviedb.org/3/search/${kind}?api_key=${KEY}&include_adult=false&query=${encodeURIComponent(q)}${params}`
    const r = await fetch(url, { signal: AbortSignal.timeout(30000) }).then(x=>x.json()).catch(()=>({}))
    const res = (r as any).results ?? []
    if (res.length) return res
  }
  return []
}

const RUN = Boolean(process.env.RUN_LIVE)
describe.skipIf(!RUN)("title audit", () => {
  beforeAll(() => writeFileSync(OUT, ""))
  it("flags non-exact mov/tv matches", async () => {
    for (const cat of ["mov","tv"] as Cat[]) {
      const root = ROOTS[cat]!; const files: SF[] = []; walk(root, files)
      const items = buildItems(cat, root, files)
      for (const it of items) {
        const kind = cat === "tv" ? "tv" : "movie"
        const res = await search(kind, it.title, String(it.year ?? ""))
        const names = (r: any) => [r.title, r.name, r.original_title, r.original_name].filter(Boolean)
        const exact = res.some((r: any) => names(r).some((n: string) => tmatch(it.title, n)))
        const top = res[0]
        appendFileSync(OUT, JSON.stringify({
          cat, parsed: `${it.title}${it.year?` (${it.year})`:""}`, hits: res.length, exact,
          chosen: top ? `${top.title||top.name} [${(top.release_date||top.first_air_date||"").slice(0,4)}] id=${top.id}` : null,
        }) + "\n")
        await new Promise(r=>setTimeout(r,120))
      }
    }
  }, 600_000)
})
