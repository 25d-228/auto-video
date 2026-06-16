import { useState } from "react"
import {
  Compass,
  Download,
  LayoutDashboard,
  Library as LibraryIcon,
  Moon,
  Settings as SettingsIcon,
  Sun,
} from "lucide-react"
import { Button } from "@/components/ui/button"
import { useTheme } from "@/components/theme-provider"
import { cn } from "@/lib/utils"
import { DownloadsProvider, isTauri, useDownloads } from "@/state/downloads"
import Dashboard from "@/views/Dashboard"
import Discover from "@/views/Discover"
import Downloads from "@/views/Downloads"
import Library from "@/views/Library"
import Settings from "@/views/Settings"

export type ViewId =
  | "dashboard"
  | "discover"
  | "library"
  | "downloads"
  | "settings"

/**
 * macOS desktop runs with titleBarStyle "Overlay" + hiddenTitle (no native
 * title bar), so the traffic lights float over the top-left of the sidebar
 * and the brand row needs to clear them. Browser dev / Windows keep their
 * native chrome, so no headroom is reserved there.
 */
const MAC_OVERLAY_TITLEBAR =
  isTauri() && navigator.userAgent.includes("Macintosh")

const NAV: { id: ViewId; label: string; icon: typeof Compass }[] = [
  { id: "dashboard", label: "Dashboard", icon: LayoutDashboard },
  { id: "discover", label: "Discover", icon: Compass },
  { id: "library", label: "Library", icon: LibraryIcon },
  { id: "downloads", label: "Downloads", icon: Download },
  { id: "settings", label: "Settings", icon: SettingsIcon },
]

const FOOTER_RECENT_COUNT = 3

function ThemeToggle() {
  const { resolvedTheme, setTheme } = useTheme()
  const next = resolvedTheme === "dark" ? "light" : "dark"
  return (
    <Button
      variant="outline"
      size="sm"
      className="w-full justify-center gap-2"
      onClick={() => setTheme(next)}
    >
      {resolvedTheme === "dark" ? (
        <Sun className="size-4" />
      ) : (
        <Moon className="size-4" />
      )}
      {resolvedTheme === "dark" ? "Light mode" : "Dark mode"}
    </Button>
  )
}

function DownloadsFooter({ onOpen }: { onOpen: () => void }) {
  const { list, active, totalSpeedMbps } = useDownloads()
  const recent = list.slice(-FOOTER_RECENT_COUNT)

  return (
    <button
      type="button"
      onClick={onOpen}
      title="Open Downloads"
      className="-mx-0.5 w-[calc(100%+4px)] cursor-pointer rounded-lg px-0.5 py-1 text-left transition-colors hover:bg-accent/60 focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none"
    >
      <div className="px-2 pb-1.5 text-[10.5px] font-semibold uppercase tracking-wider text-muted-foreground">
        Downloads
      </div>
      {list.length === 0 ? (
        <p className="px-2 text-xs text-muted-foreground">
          No active downloads
        </p>
      ) : (
        <>
          {recent.map((d) => {
            const pct = d.state === "done" ? 100 : Math.round(d.progress * 100)
            return (
              <div
                key={d.id}
                className="mb-1.5 rounded-lg border bg-background px-2 py-1.5"
              >
                <div className="truncate text-[11px] font-medium">
                  {d.title}
                  {d.state === "done" ? " ✓" : ""}
                </div>
                <div className="my-1 h-1 overflow-hidden rounded-full bg-muted">
                  <div
                    className={cn(
                      "h-full rounded-full",
                      d.state === "error" ? "bg-destructive" : "bg-foreground"
                    )}
                    style={{ width: `${pct}%` }}
                  />
                </div>
                <div className="flex justify-between text-[10.5px] text-muted-foreground">
                  <span>{d.state === "error" ? "failed" : `${pct}%`}</span>
                  {d.state === "downloading" && (
                    <span>{d.speedMbps.toFixed(1)} MB/s</span>
                  )}
                </div>
              </div>
            )
          })}
          <p className="px-2 text-[10.5px] text-muted-foreground">
            {active.length > 0
              ? `${active.length} active · ↓ ${totalSpeedMbps.toFixed(1)} MB/s`
              : "downloads complete"}
          </p>
        </>
      )}
    </button>
  )
}

export default function App() {
  return (
    <DownloadsProvider>
      <AppShell />
    </DownloadsProvider>
  )
}

function AppShell() {
  const [view, setView] = useState<ViewId>("discover")
  const { active } = useDownloads()

  return (
    <div className="flex h-screen overflow-hidden bg-background text-foreground">
      <aside
        className={cn(
          "flex w-52 flex-none flex-col border-r bg-sidebar px-2.5 pb-3",
          MAC_OVERLAY_TITLEBAR ? "pt-0" : "pt-3.5"
        )}
      >
        {MAC_OVERLAY_TITLEBAR && (
          // headroom for the floating traffic lights (which sit lowered to the
          // content title row); doubles as the window-drag handle the hidden
          // title bar used to provide. Tall enough to leave a clear gap between
          // the lights and the brand below.
          <div data-tauri-drag-region className="h-14 flex-none" />
        )}
        <div
          data-tauri-drag-region
          className="flex items-center gap-2 px-2 pb-3.5 text-sm font-semibold"
        >
          <span className="pointer-events-none flex size-6 items-center justify-center rounded-md bg-gradient-to-br from-zinc-500 to-zinc-900 text-[11px] font-semibold text-zinc-50">
            av
          </span>
          <span className="pointer-events-none">auto-video</span>
        </div>
        <nav className="flex flex-col gap-0.5">
          {NAV.map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              type="button"
              onClick={() => setView(id)}
              className={cn(
                "flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-sm font-medium transition-colors",
                view === id
                  ? "bg-accent text-accent-foreground"
                  : "text-muted-foreground hover:bg-accent hover:text-accent-foreground"
              )}
            >
              <Icon className="size-4 flex-none" />
              {label}
              {id === "downloads" && active.length > 0 && (
                <span className="ml-auto flex h-4 min-w-4 flex-none items-center justify-center rounded-full bg-primary px-1 text-[10px] font-semibold tabular-nums text-primary-foreground">
                  {active.length}
                </span>
              )}
            </button>
          ))}
        </nav>
        <div className="mt-auto flex flex-col gap-2.5">
          <DownloadsFooter onOpen={() => setView("downloads")} />
          <ThemeToggle />
        </div>
      </aside>
      <main className="flex min-w-0 flex-1 flex-col overflow-hidden">
        {view === "dashboard" && <Dashboard />}
        {/* Discover stays mounted (just hidden when inactive) so its category,
            selectors and page are remembered across tab switches. */}
        <div
          className={cn(
            "flex min-h-0 flex-1 flex-col",
            view !== "discover" && "hidden"
          )}
        >
          <Discover active={view === "discover"} />
        </div>
        {view === "library" && <Library />}
        {view === "downloads" && (
          <Downloads onGoDiscover={() => setView("discover")} />
        )}
        {view === "settings" && <Settings />}
      </main>
    </div>
  )
}
