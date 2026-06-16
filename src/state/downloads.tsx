/**
 * Download queue state over the Rust download commands (libtorrent):
 * start_download / pause_download / resume_download / cancel_download.
 * Progress arrives via the Tauri "download-progress" event; in a plain
 * browser (no window.__TAURI__) everything degrades to a toast + no-op.
 */
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react"
import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import { allDownloads, getKey, removeDownload, saveDownload } from "@/state/db"

/** True when running inside the Tauri shell (withGlobalTauri is on). */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI__" in window
}

export type DownloadState = "downloading" | "paused" | "done" | "error"

const DOWNLOAD_STATES: readonly string[] = [
  "downloading",
  "paused",
  "done",
  "error",
]

function asDownloadState(raw: string): DownloadState {
  return DOWNLOAD_STATES.includes(raw)
    ? (raw as DownloadState)
    : "downloading"
}

export interface DownloadEntry {
  id: string
  title: string
  /** 0..1 */
  progress: number
  speedMbps: number
  state: DownloadState
  /** Destination folder the torrent writes into. */
  dest: string
  /** Failure message — from the event payload, or local when start_download rejects. */
  error?: string
}

/** Base fields shared by an entry and its error variant. */
type DownloadEntryBase = Pick<DownloadEntry, "id" | "title" | "dest">

/** An "error" entry that keeps any progress already shown for this id. */
function errorEntry(
  prev: Record<string, DownloadEntry>,
  base: DownloadEntryBase,
  e: unknown
): DownloadEntry {
  return {
    ...base,
    progress: prev[base.id]?.progress ?? 0,
    speedMbps: 0,
    state: "error",
    error: String(e),
  }
}

export interface StartDownloadArgs {
  magnet: string
  /** Destination folder; the Rust side falls back to ~/Downloads/auto-video. */
  dest: string
  id: string
  title: string
  /** Torrent file indices to download (from the file-picker); omit/empty = all. */
  onlyFiles?: number[]
  /** Canonical renames to apply on completion (from planRename). */
  renames?: { from: string; to: string }[]
}

/** Payload emitted by src-tauri's DlProgress. */
interface DownloadProgressPayload {
  id: string
  title: string
  progress: number
  speed_mbps: number
  state: string
  dest: string
  /** Present only when state == "error" (Rust skips serializing None). */
  error?: string
}

export interface DownloadsApi {
  /** id -> entry, insertion-ordered. */
  downloads: Record<string, DownloadEntry>
  /** Entries in insertion order. */
  list: DownloadEntry[]
  /** Entries still in flight (downloading or paused). */
  active: DownloadEntry[]
  /** Combined speed of currently-downloading entries (MB/s). */
  totalSpeedMbps: number
  /** False in plain-browser dev — hide/disable Tauri-only actions. */
  tauri: boolean
  /** Resolves true if the download was handed to librqbit; false (with a toast) otherwise. */
  startDownload: (args: StartDownloadArgs) => Promise<boolean>
  /** Pause a downloading torrent. Resolves true on success; toasts the error otherwise. */
  pauseDownload: (id: string) => Promise<boolean>
  /** Resume a paused torrent. Resolves true on success; toasts the error otherwise. */
  resumeDownload: (id: string) => Promise<boolean>
  /**
   * Remove the torrent from the session (optionally deleting its files) and
   * drop the local entry. Resolves true on success; toasts the error otherwise.
   */
  cancelDownload: (id: string, deleteFiles: boolean) => Promise<boolean>
  /** Local-only: drop a finished/errored row from the list. */
  removeEntry: (id: string) => void
  /** Lightweight bottom-right toast (also used for the non-Tauri no-op). */
  notify: (message: string) => void
}

const DownloadsContext = createContext<DownloadsApi | null>(null)

export function DownloadsProvider({ children }: { children: ReactNode }) {
  const [downloads, setDownloads] = useState<Record<string, DownloadEntry>>({})
  const [toast, setToast] = useState<string | null>(null)
  const toastTimer = useRef<number | undefined>(undefined)
  const [tauri] = useState(isTauri)
  // ids cancelled locally — late in-flight progress events for them are ignored
  const cancelledIds = useRef<Set<string>>(new Set())
  // guard so the resume-on-launch pass runs at most once
  const resumedRef = useRef(false)

  const notify = useCallback((message: string) => {
    setToast(message)
    if (toastTimer.current !== undefined) window.clearTimeout(toastTimer.current)
    toastTimer.current = window.setTimeout(() => setToast(null), 2400)
  }, [])

  useEffect(() => {
    if (!isTauri()) return
    let cancelled = false
    let unlisten: (() => void) | undefined
    void listen<DownloadProgressPayload>("download-progress", (event) => {
      const p = event.payload
      if (cancelledIds.current.has(p.id)) return
      const state = asDownloadState(p.state)
      // A finished download no longer needs to be resumed next launch.
      if (state === "done") void removeDownload(p.id).catch(() => {})
      setDownloads((prev) => ({
        ...prev,
        [p.id]: {
          id: p.id,
          title: p.title,
          progress: Math.min(1, Math.max(0, p.progress)),
          speedMbps: p.speed_mbps,
          state,
          dest: p.dest,
          error: p.error ?? prev[p.id]?.error,
        },
      }))
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  // Apply the persisted download/upload speed limits to the libtorrent session
  // on launch — only when a limit is actually set, so an unlimited config does
  // not spin up the session early. Settings re-applies on change.
  useEffect(() => {
    if (!isTauri()) return
    void (async () => {
      try {
        const [dl, ul] = await Promise.all([
          getKey("dlLimitKib"),
          getKey("ulLimitKib"),
        ])
        const downloadKib = Number(dl) || 0
        const uploadKib = Number(ul) || 0
        if (downloadKib > 0 || uploadKib > 0) {
          await invoke("set_rate_limits", { downloadKib, uploadKib })
        }
      } catch {
        /* best-effort */
      }
    })()
  }, [])

  // Resume downloads that were in flight when the app last quit: re-add each
  // persisted magnet so libtorrent continues from the partial files on disk.
  useEffect(() => {
    if (!isTauri() || resumedRef.current) return
    resumedRef.current = true
    let cancelled = false
    const resumeRow = (r: Awaited<ReturnType<typeof allDownloads>>[number]) => {
      cancelledIds.current.delete(r.id)
      // optimistic entry so the sidebar reacts before the first progress event
      setDownloads((prev) =>
        prev[r.id]
          ? prev
          : {
              ...prev,
              [r.id]: {
                id: r.id,
                title: r.title,
                progress: 0,
                speedMbps: 0,
                state: "downloading",
                dest: r.dest,
              },
            }
      )
      void invoke("start_download", {
        magnet: r.magnet,
        dest: r.dest,
        id: r.id,
        title: r.title,
        onlyFiles: r.onlyFiles,
        renames: r.renames,
      }).catch((e) => {
        setDownloads((prev) => ({ ...prev, [r.id]: errorEntry(prev, r, e) }))
      })
    }
    void allDownloads()
      .then((rows) => {
        for (const r of rows) {
          if (cancelled) return
          resumeRow(r)
        }
      })
      .catch(() => {})
    return () => {
      cancelled = true
    }
  }, [])

  const startDownload = useCallback(
    async (args: StartDownloadArgs): Promise<boolean> => {
      const { magnet, dest, id, title, onlyFiles, renames } = args
      if (!isTauri()) {
        notify("Downloads need the desktop app — browser preview is read-only")
        return false
      }
      // a fresh start for an id clears any earlier local cancel of it
      cancelledIds.current.delete(id)
      // optimistic entry so the sidebar reacts before the first progress event
      setDownloads((prev) => ({
        ...prev,
        [id]: { id, title, progress: 0, speedMbps: 0, state: "downloading", dest },
      }))
      try {
        await invoke("start_download", { magnet, dest, id, title, onlyFiles, renames })
        // Persist so it resumes if the app quits mid-download (best-effort).
        void saveDownload({ id, magnet, dest, title, onlyFiles, renames }).catch(() => {})
        return true
      } catch (e) {
        setDownloads((prev) => ({
          ...prev,
          [id]: errorEntry(prev, { id, title, dest }, e),
        }))
        notify(`Download failed: ${String(e)}`)
        return false
      }
    },
    [notify]
  )

  const pauseDownload = useCallback(
    async (id: string): Promise<boolean> => {
      if (!isTauri()) {
        notify("Managing downloads needs the desktop app")
        return false
      }
      try {
        await invoke("pause_download", { id })
        return true
      } catch (e) {
        notify(`Pause failed: ${String(e)}`)
        return false
      }
    },
    [notify]
  )

  const resumeDownload = useCallback(
    async (id: string): Promise<boolean> => {
      if (!isTauri()) {
        notify("Managing downloads needs the desktop app")
        return false
      }
      try {
        await invoke("resume_download", { id })
        return true
      } catch (e) {
        notify(`Resume failed: ${String(e)}`)
        return false
      }
    },
    [notify]
  )

  const removeEntry = useCallback((id: string) => {
    setDownloads((prev) => {
      if (!(id in prev)) return prev
      const next = { ...prev }
      delete next[id]
      return next
    })
  }, [])

  const cancelDownload = useCallback(
    async (id: string, deleteFiles: boolean): Promise<boolean> => {
      if (!isTauri()) {
        notify("Managing downloads needs the desktop app")
        return false
      }
      try {
        // Tauri maps the camelCase arg onto Rust's delete_files param.
        await invoke("cancel_download", { id, deleteFiles })
        void removeDownload(id).catch(() => {}) // drop from the resume queue
        cancelledIds.current.add(id)
        removeEntry(id)
        return true
      } catch (e) {
        notify(`Cancel failed: ${String(e)}`)
        return false
      }
    },
    [notify, removeEntry]
  )

  const value = useMemo<DownloadsApi>(() => {
    const list = Object.values(downloads)
    const active = list.filter(
      (d) => d.state === "downloading" || d.state === "paused"
    )
    return {
      downloads,
      list,
      active,
      totalSpeedMbps: list
        .filter((d) => d.state === "downloading")
        .reduce((sum, d) => sum + d.speedMbps, 0),
      tauri,
      startDownload,
      pauseDownload,
      resumeDownload,
      cancelDownload,
      removeEntry,
      notify,
    }
  }, [
    downloads,
    tauri,
    startDownload,
    pauseDownload,
    resumeDownload,
    cancelDownload,
    removeEntry,
    notify,
  ])

  return (
    <DownloadsContext.Provider value={value}>
      {children}
      {toast !== null && (
        <div className="pointer-events-none fixed right-4 bottom-4 z-50 max-w-xs rounded-[10px] border bg-background px-3.5 py-2.5 text-xs font-medium shadow-lg">
          {toast}
        </div>
      )}
    </DownloadsContext.Provider>
  )
}

export function useDownloads(): DownloadsApi {
  const ctx = useContext(DownloadsContext)
  if (!ctx) {
    throw new Error("useDownloads must be used inside <DownloadsProvider>")
  }
  return ctx
}
