/**
 * Library detail panel: cover, title, identified facts (date / runtime via r18
 * meta or the TMDB lookup), genre/cast chip sections, the on-disk location and
 * Play / Reveal / Delete.
 */
import type { LibraryItem } from "@/api/types"
import {
  cidOf,
  DetailPanel,
  type DetailFact,
  type DetailSection,
} from "@/components/media"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"
import { useMeta } from "@/state/queries"
import { isJavCat, itemDate, splitChips, useLibraryArt } from "./useLibraryArt"

function StatePill({
  pending,
  identified,
}: {
  pending: boolean
  identified?: boolean
}) {
  const label = pending
    ? "Looking up…"
    : identified
      ? "Identified"
      : "Needs review"
  return (
    <span
      className={cn(
        "rounded-full px-2 py-0.5 text-[10.5px] font-semibold",
        pending && "bg-muted text-muted-foreground",
        !pending && identified && "bg-green-600/15 text-green-600",
        !pending && !identified && "bg-amber-500/15 text-amber-600"
      )}
    >
      {label}
    </span>
  )
}

export interface LibraryDetailProps {
  /** Last opened item; kept while closed so the slide-out can play. */
  item: LibraryItem | null
  open: boolean
  onClose: () => void
  /** False in plain-browser dev; the OS action buttons are disabled. */
  tauri: boolean
  onPlay: (item: LibraryItem) => void
  onReveal: (item: LibraryItem) => void
  onDelete: (item: LibraryItem) => void
}

export function LibraryDetail({
  item,
  open,
  onClose,
  tauri,
  onPlay,
  onReveal,
  onDelete,
}: LibraryDetailProps) {
  const art = useLibraryArt(item)
  const jav = item != null && isJavCat(item.cat)
  // r18 content id parsed from the resolved cover
  const cid = jav && art.cover ? cidOf(art.cover) : ""
  const metaQ = useMeta(
    jav
      ? { cid: cid || undefined, code: item.code, cat: item.cat }
      : {}
  )

  if (!item) return null

  const javMeta = jav ? metaQ.data : undefined
  const date = (jav ? javMeta?.date : art.titleMeta?.date) || itemDate(item)
  const runtime = (jav ? javMeta?.runtime : art.titleMeta?.runtime) || ""

  const facts: DetailFact[] = []
  if (date) facts.push({ label: "Date", value: date })
  if (runtime) facts.push({ label: "Runtime", value: runtime })
  if (item.size) facts.push({ label: "Size", value: item.size })
  if (jav && item.code) facts.push({ label: "Code", value: item.code })

  const sections: DetailSection[] = []
  if (!jav && art.titleMeta?.genre) {
    sections.push({ label: "Genre", chips: splitChips(art.titleMeta.genre) })
  }
  if (!jav && art.titleMeta?.cast) {
    sections.push({ label: "Cast", chips: splitChips(art.titleMeta.cast) })
  }
  if (jav && (javMeta?.cast_ja || javMeta?.cast)) {
    sections.push({
      label: "出演 · Cast",
      chips: splitChips(javMeta?.cast_ja || javMeta?.cast),
    })
  }

  const note = jav
    ? metaQ.isLoading
      ? "fetching Japanese title + cast…"
      : metaQ.isError
        ? "metadata lookup failed"
        : ""
    : art.pending
      ? "fetching from TMDB…"
      : art.haskey === false
        ? "Add a TMDB key in Settings to identify movies and TV."
        : ""

  return (
    <DetailPanel
      open={open}
      onClose={onClose}
      title={item.title}
      sub={item.sub || undefined}
      cover={art.cover}
      coverAspect={item.ar}
      pill={<StatePill pending={art.pending} identified={art.identified} />}
      facts={facts}
      sections={sections}
      actions={
        <>
          <Button size="sm" disabled={!tauri} onClick={() => onPlay(item)}>
            Play
          </Button>
          <Button
            size="sm"
            variant="outline"
            title="Reveal in Finder"
            disabled={!tauri}
            onClick={() => onReveal(item)}
          >
            Reveal
          </Button>
          <Button
            size="sm"
            variant="destructive"
            title="Move to Trash"
            disabled={!tauri}
            onClick={() => onDelete(item)}
          >
            Delete
          </Button>
        </>
      }
    >
      {jav && javMeta?.jatitle && (
        <div className="mt-2 text-xs leading-snug text-muted-foreground">
          {javMeta.jatitle}
        </div>
      )}
      {note !== "" && (
        <div className="mt-3 text-[11px] text-muted-foreground">{note}</div>
      )}
      <div className="mt-3.5">
        <div className="mb-1 text-[10.5px] font-semibold tracking-wider text-muted-foreground uppercase">
          Location
        </div>
        <div className="rounded-lg border bg-muted/40 px-2.5 py-2 font-mono text-[10.5px] leading-relaxed break-all">
          {item.path}
        </div>
      </div>
      {!tauri && (
        <div className="mt-3 text-[11px] text-muted-foreground">
          Play, Reveal and Delete need the desktop app.
        </div>
      )}
    </DetailPanel>
  )
}
