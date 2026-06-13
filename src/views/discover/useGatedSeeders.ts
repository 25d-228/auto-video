/**
 * Lazy seeder-badge upgrades with a concurrency cap: every visible card
 * wants its real aggregated topSeed from /seeders, but only a few requests
 * may be in flight at once. Cards queue for a slot, fetch, then release the
 * slot when the query settles (success or error). Cached results resolve
 * instantly, so revisiting a page drains the queue immediately.
 */
import { useEffect, useRef, useState } from "react"
import { useSeeders, type SeederSubject } from "@/state/queries"

interface GateEntry {
  grant: () => void
  granted: boolean
  released: boolean
}

export interface GateSlot {
  /** Idempotent: queued entries are dequeued, granted ones free a slot. */
  release(): void
}

export class FetchGate {
  private active = 0
  private queue: GateEntry[] = []

  constructor(private readonly max: number) {}

  /** Calls `onGrant` now or once a slot frees up. */
  acquire(onGrant: () => void): GateSlot {
    const entry: GateEntry = { grant: onGrant, granted: false, released: false }
    if (this.active < this.max) {
      this.active++
      entry.granted = true
      onGrant()
    } else {
      this.queue.push(entry)
    }
    return {
      release: () => {
        if (entry.released) return
        entry.released = true
        if (entry.granted) {
          this.active--
          this.pump()
        } else {
          const i = this.queue.indexOf(entry)
          if (i >= 0) this.queue.splice(i, 1)
        }
      },
    }
  }

  private pump(): void {
    while (this.active < this.max && this.queue.length > 0) {
      const next = this.queue.shift()
      if (!next) return
      this.active++
      next.granted = true
      next.grant()
    }
  }
}

/** One shared gate for all Discover seeder lookups. */
const seedGate = new FetchGate(4)

/**
 * useSeeders, but held behind the shared gate. Pass the page item; the
 * query starts when a slot is granted and the slot is released as soon as
 * the lookup settles (or when the card unmounts / changes identity).
 */
export function useGatedSeeders(item: SeederSubject & { id: string }) {
  const [granted, setGranted] = useState(false)
  const slotRef = useRef<GateSlot | null>(null)

  useEffect(() => {
    setGranted(false)
    const slot = seedGate.acquire(() => setGranted(true))
    slotRef.current = slot
    return () => {
      slot.release()
      slotRef.current = null
    }
  }, [item.id])

  const query = useSeeders(granted ? item : null)

  // free the slot once the request settles — or immediately for items the
  // hook can never fetch (no title and no code)
  const fetchable = Boolean(item.title || item.code)
  const settled = query.isSuccess || query.isError
  useEffect(() => {
    if (granted && (settled || !fetchable)) slotRef.current?.release()
  }, [granted, settled, fetchable])

  return query
}
