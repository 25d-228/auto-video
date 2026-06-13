import type { ComponentProps, ReactNode } from "react"
import { cn } from "@/lib/utils"

export interface ChipOption<T extends string | number> {
  value: T
  label: ReactNode
}

export interface ChipRowProps<T extends string | number> {
  options: readonly ChipOption<T>[]
  value: T
  onChange: (value: T) => void
  className?: string
}

/** Category chip row per the prototype (.chips/.chip): pill buttons, active = primary. */
export function ChipRow<T extends string | number>({
  options,
  value,
  onChange,
  className,
}: ChipRowProps<T>) {
  return (
    <div className={cn("flex gap-1.5", className)}>
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          onClick={() => onChange(o.value)}
          className={cn(
            "rounded-full border px-3 py-1 text-xs font-medium transition-colors",
            o.value === value
              ? "border-primary bg-primary text-primary-foreground"
              : "bg-background text-muted-foreground hover:bg-muted hover:text-foreground"
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  )
}

/** Small muted pill for cast/genre chips (prototype .cchip / old .mk-chiplet). */
export function Chiplet({ className, ...props }: ComponentProps<"span">) {
  return (
    <span
      className={cn("rounded-full bg-muted px-2.5 py-1 text-[11px] text-foreground", className)}
      {...props}
    />
  )
}
