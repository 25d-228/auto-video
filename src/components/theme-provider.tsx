import { createContext, useContext, useEffect, useState } from "react"

export type Theme = "dark" | "light" | "system"

interface ThemeProviderState {
  theme: Theme
  /** The theme actually applied to <html> after resolving "system". */
  resolvedTheme: "dark" | "light"
  setTheme: (theme: Theme) => void
}

const STORAGE_KEY = "auto-video-theme"

const ThemeProviderContext = createContext<ThemeProviderState | undefined>(
  undefined
)

function systemTheme(): "dark" | "light" {
  return window.matchMedia("(prefers-color-scheme: dark)").matches
    ? "dark"
    : "light"
}

function loadTheme(): Theme {
  const stored = localStorage.getItem(STORAGE_KEY)
  if (stored === "dark" || stored === "light" || stored === "system") {
    return stored
  }
  return "system"
}

export function ThemeProvider({ children }: { children: React.ReactNode }) {
  const [theme, setThemeState] = useState<Theme>(loadTheme)

  const resolve = (t: Theme): "dark" | "light" =>
    t === "system" ? systemTheme() : t

  const resolvedTheme = resolve(theme)

  useEffect(() => {
    const root = window.document.documentElement
    const apply = () => {
      root.classList.toggle("dark", resolve(theme) === "dark")
    }
    apply()
    // follow OS changes while in "system" mode
    const mq = window.matchMedia("(prefers-color-scheme: dark)")
    mq.addEventListener("change", apply)
    return () => mq.removeEventListener("change", apply)
  }, [theme])

  const setTheme = (next: Theme) => {
    localStorage.setItem(STORAGE_KEY, next)
    setThemeState(next)
  }

  return (
    <ThemeProviderContext.Provider value={{ theme, resolvedTheme, setTheme }}>
      {children}
    </ThemeProviderContext.Provider>
  )
}

export function useTheme(): ThemeProviderState {
  const ctx = useContext(ThemeProviderContext)
  if (!ctx) throw new Error("useTheme must be used within a ThemeProvider")
  return ctx
}
