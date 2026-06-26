/**
 * Settings: library folders, provider keys, appearance and about.
 *
 * Folder paths and provider keys persist in the local SQLite database
 * (src/state/db.ts, paths/provider_keys tables). The native folder picker
 * (dialog plugin) is desktop-only and disabled in a plain browser; typing a
 * path manually still works.
 */
import { useEffect, useRef, useState } from "react"
import { useQueryClient } from "@tanstack/react-query"
import { getVersion } from "@tauri-apps/api/app"
import { invoke } from "@tauri-apps/api/core"
import { open as openFolderDialog } from "@tauri-apps/plugin-dialog"
import { Check, FolderOpen } from "lucide-react"
import { saveKey, savePath } from "@/api/client"
import type { Cat } from "@/api/types"
import { SegControl } from "@/components/media"
import { useTheme, type Theme } from "@/components/theme-provider"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { cn } from "@/lib/utils"
import { useDownloads } from "@/state/downloads"
import { qk, useKeys, usePaths } from "@/state/queries"
import { Hint, NEEDS_DESKTOP } from "@/views/downloads/Hint"

const FOLDER_CATS: { cat: Cat; label: string }[] = [
  { cat: "mov", label: "Movies" },
  { cat: "tv", label: "TV" },
  { cat: "ad", label: "Adult" },
  { cat: "vrc", label: "VR" },
]

/** Provider ids. */
const KEY_FIELDS: { provider: string; label: string; placeholder: string }[] = [
  { provider: "tmdb", label: "TMDB API key", placeholder: "Not configured" },
  { provider: "dmmApi", label: "DMM API ID", placeholder: "Not configured" },
  {
    provider: "dmmAff",
    label: "DMM Affiliate ID",
    placeholder: "Not configured",
  },
]

/** Fallback when the Tauri shell (and getVersion) is unavailable. */
const APP_VERSION_FALLBACK = "0.1.0"

/** How long the "Saved"/"Save failed" status stays visible after saving keys. */
const SAVE_STATUS_FLASH_MS = 2500

const THEME_OPTIONS = [
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
  { value: "system", label: "System" },
] as const satisfies readonly { value: Theme; label: string }[]

// ----------------------------------------------------------- library folders

interface FolderRowProps {
  cat: Cat
  label: string
  /** Path currently stored by the helper ("" when unset). */
  saved: string
  tauri: boolean
  onSave: (cat: Cat, path: string) => Promise<boolean>
}

function FolderRow({ cat, label, saved, tauri, onSave }: FolderRowProps) {
  // null = untouched -> show the helper's stored value
  const [draft, setDraft] = useState<string | null>(null)
  const [isPicking, setIsPicking] = useState(false)

  const value = draft ?? saved
  const isDirty = draft !== null && draft.trim() !== saved

  const commit = async (path: string) => {
    const trimmed = path.trim()
    if (trimmed === saved) return
    if (await onSave(cat, trimmed)) setDraft(null)
  }

  const browse = async () => {
    if (!tauri || isPicking) return
    setIsPicking(true)
    try {
      const picked = await openFolderDialog({
        directory: true,
        multiple: false,
        title: `Choose ${label} folder`,
        ...(saved !== "" ? { defaultPath: saved } : {}),
      })
      if (typeof picked === "string" && picked !== "") {
        setDraft(picked)
        await commit(picked)
      }
    } finally {
      setIsPicking(false)
    }
  }

  return (
    <div>
      <div className="flex items-center gap-2">
        <span className="w-14 flex-none text-xs font-medium text-muted-foreground">
          {label}
        </span>
        {/* Primary affordance: the native folder picker. */}
        <Hint show={!tauri} title={NEEDS_DESKTOP}>
          <Button
            size="sm"
            className="h-8 flex-none gap-1.5"
            disabled={!tauri || isPicking}
            onClick={() => void browse()}
          >
            <FolderOpen className="size-3.5" />
            {isPicking ? "Choosing…" : "Browse…"}
          </Button>
        </Hint>
        {/* Show the current folder next to the button when it isn't being
            edited; the manual fallback below stays the source of truth. */}
        <span
          className={cn(
            "min-w-0 flex-1 truncate font-mono text-[11px]",
            value === "" && "font-sans text-muted-foreground/60"
          )}
          title={value || undefined}
        >
          {value === "" ? "No folder chosen" : value}
        </span>
      </div>
      <div className="mt-1.5 ml-16 flex items-center gap-2">
        <span className="flex-none text-[11px] text-muted-foreground">
          or type the full path
        </span>
        <Input
          value={value}
          spellCheck={false}
          placeholder={`Absolute path, e.g. /Volumes/Media/${label}`}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={() => {
            if (isDirty) void commit(value)
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter" && isDirty) void commit(value)
          }}
          className="h-7 flex-1 text-[11px]"
        />
      </div>
      <p className="mt-1 ml-16 text-[11px] text-muted-foreground">
        {isDirty ? (
          <span className="text-amber-600 dark:text-amber-500">
            Unsaved — press Enter or click away to save
          </span>
        ) : saved !== "" ? (
          <span className="inline-flex items-center gap-1">
            <span className="size-1.5 rounded-full bg-green-600" />
            Configured
          </span>
        ) : (
          <span className="inline-flex items-center gap-1">
            <span className="size-1.5 rounded-full bg-muted-foreground/40" />
            Not configured
          </span>
        )}
      </p>
    </div>
  )
}

function LibraryFoldersCard() {
  const queryClient = useQueryClient()
  const { tauri, notify } = useDownloads()
  const pathsQuery = usePaths()
  const paths = pathsQuery.data

  const savePathFor = async (cat: Cat, path: string): Promise<boolean> => {
    try {
      await savePath(cat, path)
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: qk.paths() }),
        queryClient.invalidateQueries({ queryKey: qk.library() }),
        queryClient.invalidateQueries({ queryKey: qk.stats() }),
      ])
      const label = FOLDER_CATS.find((c) => c.cat === cat)?.label ?? cat
      notify(
        path === "" ? `${label} folder cleared` : `${label} folder saved`
      )
      return true
    } catch {
      notify("Couldn't save the folder.")
      return false
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Library folders</CardTitle>
        <CardDescription>
          One absolute folder per category. Click Browse to pick it (or type
          the path in the browser preview). Downloads are moved into the
          matching folder and the Library scans them.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        {FOLDER_CATS.map(({ cat, label }) => (
          <FolderRow
            key={cat}
            cat={cat}
            label={label}
            saved={(paths?.[cat] ?? "").trim()}
            tauri={tauri}
            onSave={savePathFor}
          />
        ))}
        {pathsQuery.isError && (
          <p className="text-[11px] text-destructive">
            Couldn't load the stored folders.
          </p>
        )}
      </CardContent>
    </Card>
  )
}

// ------------------------------------------------------------- provider keys

function ProviderKeysCard() {
  const queryClient = useQueryClient()
  const { notify } = useDownloads()
  const keysQuery = useKeys()
  const stored = keysQuery.data

  // provider -> edited value; undefined = untouched -> show the stored value
  const [drafts, setDrafts] = useState<Record<string, string>>({})
  const [saving, setSaving] = useState(false)
  const [status, setStatus] = useState<"saved" | "error" | null>(null)
  const statusTimer = useRef<number | undefined>(undefined)

  useEffect(() => {
    return () => {
      if (statusTimer.current !== undefined) {
        window.clearTimeout(statusTimer.current)
      }
    }
  }, [])

  const valueFor = (provider: string) =>
    drafts[provider] ?? stored?.[provider] ?? ""

  const flash = (next: "saved" | "error") => {
    setStatus(next)
    if (statusTimer.current !== undefined) {
      window.clearTimeout(statusTimer.current)
    }
    statusTimer.current = window.setTimeout(() => setStatus(null), SAVE_STATUS_FLASH_MS)
  }

  const saveAll = async () => {
    setSaving(true)
    try {
      for (const { provider } of KEY_FIELDS) {
        await saveKey(provider, valueFor(provider).trim())
      }
      await queryClient.invalidateQueries({ queryKey: qk.keys() })
      setDrafts({})
      flash("saved")
    } catch {
      flash("error")
      notify("Couldn't save the keys.")
    } finally {
      setSaving(false)
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Provider keys</CardTitle>
        <CardDescription>
          TMDB powers movie/TV covers and metadata; the DMM ids fill Japanese
          titles and cast.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        {KEY_FIELDS.map(({ provider, label, placeholder }) => (
          <div key={provider} className="flex items-center gap-2">
            <span className="w-28 flex-none text-xs font-medium text-muted-foreground">
              {label}
            </span>
            <Input
              type="password"
              value={valueFor(provider)}
              placeholder={placeholder}
              autoComplete="off"
              spellCheck={false}
              onChange={(e) =>
                setDrafts((prev) => ({ ...prev, [provider]: e.target.value }))
              }
              className="h-8 flex-1 text-xs"
            />
          </div>
        ))}
        <div className="flex items-center gap-2.5">
          <Button
            size="sm"
            className="h-8"
            disabled={saving}
            onClick={() => void saveAll()}
          >
            {saving ? "Saving…" : "Save keys"}
          </Button>
          {status === "saved" && (
            <span className="inline-flex items-center gap-1 text-xs font-medium text-green-600">
              <Check className="size-3.5" /> Saved
            </span>
          )}
          {status === "error" && (
            <span className="text-xs font-medium text-destructive">
              Save failed
            </span>
          )}
        </div>
        <p className="text-[11px] text-muted-foreground">
          Keys are stored locally in the app database and never leave this
          machine.
        </p>
      </CardContent>
    </Card>
  )
}

// ------------------------------------------------------------- download speed

/** A numeric KiB/s field that treats blank / 0 / invalid as "unlimited". */
function parseKibLimit(raw: string): number {
  const parsed = Math.floor(Number(raw))
  return Number.isFinite(parsed) && parsed > 0 ? parsed : 0
}

function DownloadLimitsCard() {
  const queryClient = useQueryClient()
  const { tauri, notify } = useDownloads()
  const keysQuery = useKeys()
  const stored = keysQuery.data
  // undefined = untouched -> show the stored value ("0" renders as blank).
  const [drafts, setDrafts] = useState<{ download?: string; upload?: string }>(
    {}
  )
  const [saving, setSaving] = useState(false)

  const storedDisplayValue = (key: string) => {
    const v = stored?.[key] ?? ""
    return v === "0" ? "" : v
  }
  const downloadValue = drafts.download ?? storedDisplayValue("dlLimitKib")
  const uploadValue = drafts.upload ?? storedDisplayValue("ulLimitKib")
  const dirty = drafts.download !== undefined || drafts.upload !== undefined

  const save = async () => {
    setSaving(true)
    try {
      const downloadKib = parseKibLimit(downloadValue)
      const uploadKib = parseKibLimit(uploadValue)
      await saveKey("dlLimitKib", String(downloadKib))
      await saveKey("ulLimitKib", String(uploadKib))
      await queryClient.invalidateQueries({ queryKey: qk.keys() })
      if (tauri) {
        // Apply live; it also re-applies from storage on next launch.
        try {
          await invoke("set_rate_limits", { downloadKib, uploadKib })
        } catch {
          /* still persisted for next launch */
        }
      }
      setDrafts({})
      notify("Speed limits saved")
    } catch {
      notify("Couldn't save the speed limits.")
    } finally {
      setSaving(false)
    }
  }

  const field = (
    label: string,
    value: string,
    onChange: (v: string) => void
  ) => (
    <div className="flex items-center gap-2">
      <span className="w-28 flex-none text-xs font-medium text-muted-foreground">
        {label}
      </span>
      <Input
        type="number"
        min={0}
        inputMode="numeric"
        value={value}
        placeholder="Unlimited"
        onChange={(e) => onChange(e.target.value)}
        className="h-8 w-32 text-xs"
      />
      <span className="text-[11px] text-muted-foreground">KB/s</span>
    </div>
  )

  return (
    <Card>
      <CardHeader>
        <CardTitle>Download speed</CardTitle>
        <CardDescription>
          Caps apply to all torrents combined. Leave blank (or 0) for unlimited.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        {field("Max download", downloadValue, (v) =>
          setDrafts((p) => ({ ...p, download: v }))
        )}
        {field("Max upload", uploadValue, (v) =>
          setDrafts((p) => ({ ...p, upload: v }))
        )}
        <div>
          <Button
            size="sm"
            className="h-8"
            disabled={saving || !dirty}
            onClick={() => void save()}
          >
            {saving ? "Saving…" : "Save limits"}
          </Button>
        </div>
      </CardContent>
    </Card>
  )
}

// ---------------------------------------------------------------- appearance

function AppearanceCard() {
  const { theme, resolvedTheme, setTheme } = useTheme()
  return (
    <Card>
      <CardHeader>
        <CardTitle>Appearance</CardTitle>
        <CardDescription>
          System follows the OS preference (currently {resolvedTheme}).
        </CardDescription>
      </CardHeader>
      <CardContent>
        <div className="flex items-center gap-2">
          <span className="w-14 flex-none text-xs font-medium text-muted-foreground">
            Theme
          </span>
          <SegControl<Theme>
            options={THEME_OPTIONS}
            value={theme}
            onChange={setTheme}
          />
        </div>
      </CardContent>
    </Card>
  )
}

// --------------------------------------------------------------------- about

function AboutCard() {
  const { tauri } = useDownloads()
  const { data: paths } = usePaths()
  const [version, setVersion] = useState(APP_VERSION_FALLBACK)

  useEffect(() => {
    if (!tauri) return
    let cancelled = false
    getVersion()
      .then((v) => {
        if (!cancelled) setVersion(v)
      })
      .catch(() => {
        /* keep the fallback */
      })
    return () => {
      cancelled = true
    }
  }, [tauri])

  return (
    <Card>
      <CardHeader>
        <CardTitle>About</CardTitle>
        <CardDescription>
          auto-video v{version} ·{" "}
          {tauri ? "desktop app" : "browser preview (downloads disabled)"}
        </CardDescription>
      </CardHeader>
      <CardContent>
        <div className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 text-xs">
          {FOLDER_CATS.map(({ cat, label }) => {
            const p = (paths?.[cat] ?? "").trim()
            return (
              <div key={cat} className="contents">
                <span className="text-muted-foreground">{label}</span>
                <span
                  className={cn(
                    "truncate font-mono text-[11px]",
                    p === "" && "font-sans text-muted-foreground/60"
                  )}
                  title={p || undefined}
                >
                  {p === "" ? "Not set" : p}
                </span>
              </div>
            )
          })}
        </div>
      </CardContent>
    </Card>
  )
}

// ---------------------------------------------------------------------- view

export default function Settings() {
  return (
    <section className="flex h-full min-h-0 flex-col p-5">
      <div className="mb-4 flex flex-none items-baseline gap-2.5">
        <h1 className="text-lg font-semibold">Settings</h1>
        <span className="text-xs text-muted-foreground">
          Folders, providers and appearance
        </span>
      </div>
      {/* Full-width scroll container so the scrollbar sits at the window edge;
          the cards stay capped at max-w-xl and left-aligned inside it. */}
      <div className="min-h-0 flex-1 overflow-y-auto pb-5">
        <div className="flex max-w-xl flex-col gap-3">
          <LibraryFoldersCard />
          <ProviderKeysCard />
          <DownloadLimitsCard />
          <AppearanceCard />
          <AboutCard />
        </div>
      </div>
    </section>
  )
}
