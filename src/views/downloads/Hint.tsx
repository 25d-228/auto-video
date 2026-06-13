import type { ReactNode } from "react"

/**
 * Wraps a (possibly disabled) control. Disabled buttons swallow pointer
 * events, so the native-tooltip `title` goes on this wrapper span instead —
 * used for the plain-browser "needs the desktop app" hints.
 */
export function Hint({
  show,
  title,
  children,
}: {
  show: boolean
  title: string
  children: ReactNode
}) {
  if (!show) return <>{children}</>
  return (
    <span title={title} className="inline-flex cursor-not-allowed">
      {children}
    </span>
  )
}

export const NEEDS_DESKTOP = "Needs the desktop app — browser preview is read-only"
