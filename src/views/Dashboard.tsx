/**
 * Dashboard — port of the old engine's renderDash() (git show
 * HEAD:ui-src/engine.js): stat tiles, per-category disk usage and the active
 * download list, styled after docs/design/prototype-shadcn-approved.html.
 *
 * Stats come from the sidecar via useStatsLive() (qk.stats() + a 30 s
 * refetch interval while the view is visible); downloads come from the
 * shared DownloadsProvider. With no sidecar (plain-browser dev) every disk
 * row degrades to "offline" and the tiles show an em dash — nothing crashes.
 */
import type { ReactNode } from "react"
import { Card } from "@/components/ui/card"
import { fmtBytes } from "@/lib/format"
import { useDownloads, type DownloadEntry } from "@/state/downloads"
import type { Cat, DiskStats } from "@/api/types"
import { useStatsLive } from "@/state/queries"

/** Category order + fallback labels, as in the old renderDash(). */
const CATS: ReadonlyArray<{ cat: Cat; label: string }> = [
  { cat: "mov", label: "Movies" },
  { cat: "tv", label: "TV" },
  { cat: "ad", label: "Adult" },
  { cat: "vrc", label: "VR" },
]

const OFFLINE_DISK: DiskStats = {
  path: "",
  online: false,
  free: 0,
  total: 0,
  files: 0,
}

// ------------------------------------------------------------- stat tiles

function StatTile({
  label,
  value,
  sub,
}: {
  label: string
  value: ReactNode
  sub: ReactNode
}) {
  return (
    <Card className="gap-0 px-4 py-3.5">
      <div className="text-[11.5px] font-medium text-muted-foreground">
        {label}
      </div>
      <div className="mt-1 text-[22px] leading-tight font-semibold">{value}</div>
      <div className="mt-0.5 text-[11px] text-muted-foreground">{sub}</div>
    </Card>
  )
}

// -------------------------------------------------------------- disk rows

function DiskRow({ label, disk }: { label: string; disk: DiskStats }) {
  const used =
    disk.total > 0 ? Math.round((1 - disk.free / disk.total) * 100) : 0
  return (
    <div className="border-b py-2 last:border-b-0 last:pb-0.5">
      <div className="mb-1.5 flex items-baseline justify-between gap-3">
        <span className="truncate font-mono text-[11.5px] font-medium">
          {disk.path || label}
        </span>
        <span className="shrink-0 text-[11px] text-muted-foreground">
          {disk.online ? (
            `${used}% used`
          ) : (
            <span className="font-semibold text-amber-600">offline</span>
          )}
        </span>
      </div>
      <div className="h-1 overflow-hidden rounded-full bg-muted">
        <div
          className="h-full rounded-full bg-primary"
          style={{ width: `${disk.online ? used : 0}%` }}
        />
      </div>
      <div className="mt-1 text-[11px] text-muted-foreground">
        {disk.total > 0
          ? `${fmtBytes(disk.free)} free of ${fmtBytes(disk.total)} · ${disk.files} files`
          : disk.online
            ? `${disk.files} files`
            : "offline"}
      </div>
    </div>
  )
}

// --------------------------------------------------------- download rows

function DownloadRow({ entry }: { entry: DownloadEntry }) {
  const pct =
    entry.state === "done" ? 100 : Math.round(entry.progress * 100)
  const fill =
    entry.state === "done"
      ? "bg-green-600"
      : entry.state === "error"
        ? "bg-red-500"
        : "bg-primary"
  return (
    <div className="flex items-center gap-3 border-b py-2 last:border-b-0 last:pb-0.5">
      <div className="w-[46%] truncate text-xs font-medium" title={entry.title}>
        {entry.title}
      </div>
      <div className="h-1 flex-1 overflow-hidden rounded-full bg-muted">
        <div
          className={`h-full rounded-full ${fill}`}
          style={{ width: `${pct}%` }}
        />
      </div>
      <div className="w-24 shrink-0 text-right text-[11px] text-muted-foreground">
        {entry.state === "done" ? (
          "Done ✓"
        ) : entry.state === "error" ? (
          <span className="font-semibold text-red-500">failed</span>
        ) : (
          `${pct}% · ${entry.speedMbps.toFixed(1)} MB/s`
        )}
      </div>
    </div>
  )
}

// -------------------------------------------------------------------- view

export default function Dashboard() {
  const { list, active, totalSpeedMbps } = useDownloads()
  const statsQuery = useStatsLive()
  const disks = statsQuery.data?.disks

  const filesTotal = disks
    ? CATS.reduce((sum, c) => sum + (disks[c.cat]?.files ?? 0), 0)
    : undefined
  const offlineCats = disks
    ? CATS.filter((c) => !disks[c.cat]?.online)
    : undefined
  const onlineCount =
    offlineCats !== undefined ? CATS.length - offlineCats.length : undefined

  // tile subtitle while stats are unavailable
  const statsFallback = statsQuery.isError ? "stats unavailable" : "loading…"

  return (
    <section className="flex h-full min-h-0 flex-col p-5">
      <div className="mb-4 flex flex-none items-baseline gap-2.5">
        <h1 className="text-lg font-semibold">Dashboard</h1>
        <span className="text-xs text-muted-foreground">System overview</span>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
      <div className="mb-3.5 grid grid-cols-3 gap-3">
        <StatTile
          label="Active downloads"
          value={active.length}
          sub={
            active.length > 0
              ? `${totalSpeedMbps.toFixed(1)} MB/s combined`
              : list.length > 0
                ? "downloads complete"
                : "queue is idle"
          }
        />
        <StatTile
          label="Files in library"
          value={filesTotal ?? "—"}
          sub={disks ? "across 4 categories" : statsFallback}
        />
        <StatTile
          label="Folders online"
          value={onlineCount !== undefined ? `${onlineCount} / 4` : "—"}
          sub={
            offlineCats === undefined
              ? statsFallback
              : offlineCats.length === 0
                ? "all folders online"
                : `${offlineCats.map((c) => c.label).join(", ")} offline`
          }
        />
      </div>

      <div className="grid grid-cols-2 items-start gap-3">
        <Card className="gap-0 px-4 py-3.5">
          <div className="mb-1.5 text-[12.5px] font-semibold">Storage</div>
          {statsQuery.isError && (
            <div className="mb-1 text-[11px] text-muted-foreground">
              Sidecar not reachable — live disk stats unavailable.
            </div>
          )}
          {CATS.map((c) => (
            <DiskRow
              key={c.cat}
              label={c.label}
              disk={disks?.[c.cat] ?? OFFLINE_DISK}
            />
          ))}
        </Card>

        <Card className="gap-0 px-4 py-3.5">
          <div className="mb-1.5 text-[12.5px] font-semibold">
            Active downloads
          </div>
          {list.length > 0 ? (
            list.map((d) => <DownloadRow key={d.id} entry={d} />)
          ) : (
            <div className="py-1 text-[11px] text-muted-foreground">
              No downloads yet
            </div>
          )}
        </Card>
      </div>
      </div>
    </section>
  )
}
