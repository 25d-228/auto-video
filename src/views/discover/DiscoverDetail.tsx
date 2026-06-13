/**
 * Right-hand detail panel for a Discover item (old openDiscPreview /
 * discPanelHTML): poster, title (+ Japanese title for Adult/VR via /meta),
 * facts grid with a live seeder count, cast chips and a Download action.
 */
import { useState } from "react"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  cidOf,
  DetailPanel,
  type DetailFact,
  type DetailSection,
} from "@/components/media"
import { fseed } from "@/lib/format"
import { useMeta, useSeeders } from "@/state/queries"
import { isTauri, useDownloads } from "@/state/downloads"
import { itemState, providerLabel, stateLabel, type ScoredItem } from "./model"

/** Open a URL in the OS default browser, never in the app webview. */
async function openSourceLink(url: string) {
  if (isTauri()) {
    const { openUrl } = await import("@tauri-apps/plugin-opener")
    await openUrl(url)
  } else {
    window.open(url, "_blank", "noopener")
  }
}

export interface DiscoverDetailProps {
  /** null while closed; the last item keeps rendering for the slide-out. */
  item: ScoredItem | null
  owned: ReadonlySet<string>
  onClose: () => void
  onDownload: (item: ScoredItem) => void
}

export function DiscoverDetail({
  item,
  owned,
  onClose,
  onDownload,
}: DiscoverDetailProps) {
  // keep the last opened item mounted so the close transition can play
  const [last, setLast] = useState<ScoredItem | null>(item)
  if (item && item !== last) setLast(item)
  const it = item ?? last

  const { downloads } = useDownloads()

  const jav = it !== null && (it.cat === "ad" || it.cat === "vrc")
  const metaQ = useMeta(
    jav && it ? { cid: cidOf(it.cover), code: it.code, cat: it.cat } : {}
  )
  // fetch only while open; usually already cached by the card's badge
  const seedQ = useSeeders(item)

  if (!it) return null

  const cs = itemState(it, owned, downloads)
  const meta = jav ? metaQ.data : undefined

  const seed = seedQ.data
  const seedSources = seed ? Object.keys(seed.sources) : []
  const seedValue = seed
    ? `▲ ${fseed(seed.topSeed || 0)}${seedSources.length > 0 ? ` (${seedSources.join("/")})` : ""}`
    : `▲ ${fseed(it.seeders)}`

  const date = meta?.date || it.date || ""
  const runtime = meta?.runtime || (it.runtime ? `${it.runtime} min` : "")
  const facts: DetailFact[] = []
  if (date) facts.push({ label: "Date", value: date })
  if (runtime) facts.push({ label: "Runtime", value: runtime })
  facts.push({ label: "Seeders", value: seedValue })
  facts.push({ label: "Source", value: providerLabel(it.src) })
  facts.push({ label: "State", value: stateLabel(cs) })
  if (it.size) facts.push({ label: "Size", value: it.size })

  const castChips = (meta?.cast_ja || meta?.cast || "")
    .split(/[,/]/)
    .map((s) => s.trim())
    .filter((s) => s !== "")
  const sections: DetailSection[] =
    castChips.length > 0 ? [{ label: "出演 · Cast", chips: castChips }] : []

  return (
    <DetailPanel
      open={item !== null}
      onClose={onClose}
      title={it.title}
      cover={it.cover || undefined}
      coverAspect={it.ar}
      pill={<Badge variant="secondary">{stateLabel(cs)}</Badge>}
      sub={
        <>
          {meta?.jatitle && (
            <span className="mb-0.5 block text-xs leading-snug text-foreground">
              {meta.jatitle}
            </span>
          )}
          {it.sub} · {providerLabel(it.src)}
        </>
      }
      facts={facts}
      sections={sections}
      actions={
        cs.state === "new" ? (
          <Button onClick={() => onDownload(it)}>Download…</Button>
        ) : undefined
      }
    >
      {jav && metaQ.isFetching && (
        <div className="mt-3.5 text-[10.5px] font-semibold tracking-wider text-muted-foreground uppercase">
          fetching Japanese title + cast…
        </div>
      )}
      {jav && metaQ.isError && (
        <div className="mt-3.5 text-[10.5px] font-semibold tracking-wider text-muted-foreground uppercase">
          metadata lookup failed
        </div>
      )}
      {it.link && (
        <Button
          variant="outline"
          className="mt-3.5 w-full"
          onClick={() => void openSourceLink(it.link!)}
        >
          View on {providerLabel(it.src)} ↗
        </Button>
      )}
    </DetailPanel>
  )
}
