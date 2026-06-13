import { cn } from "@/lib/utils"
import type { ChipOption } from "./ChipRow"

export interface SegControlProps<T extends string | number> {
  options: readonly ChipOption<T>[]
  value: T
  onChange: (value: T) => void
  className?: string
}

/**
 * Segmented control per the prototype (.seg): muted track, the active
 * segment pops on a background card. Used for Trending/Newest, Asc/Desc,
 * Show 10/25/50/100, Light/Dark, …
 */
export function SegControl<T extends string | number>({
  options,
  value,
  onChange,
  className,
}: SegControlProps<T>) {
  return (
    <div className={cn("inline-flex rounded-lg bg-muted p-0.5", className)}>
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          onClick={() => onChange(o.value)}
          className={cn(
            "rounded-md px-2.5 py-1 text-xs font-medium transition-colors",
            o.value === value
              ? "bg-background text-foreground shadow-xs"
              : "text-muted-foreground hover:text-foreground"
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  )
}
