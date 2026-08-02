import {
  type KeyboardEvent as ReactKeyboardEvent,
  useEffect,
  useRef,
  useState,
} from "react";

import "./index.css";

const destinations = [
  {
    id: "dashboard",
    label: "Dashboard",
    description: "A home for verified activity and storage information.",
    emptyHeading: "Dashboard data is not available yet",
    emptyMessage:
      "Metrics and storage details will appear here only after their data sources are implemented.",
  },
  {
    id: "discover",
    label: "Discover",
    description: "Find media after discovery providers are introduced.",
    emptyHeading: "Discovery is not configured",
    emptyMessage:
      "Provider feeds, search, and media results will be added in a later product slice.",
  },
  {
    id: "library",
    label: "Library",
    description: "Manage local media after library support is available.",
    emptyHeading: "No library folder is configured",
    emptyMessage:
      "Folder selection and media scanning are not available in this presentation-only shell.",
  },
  {
    id: "downloads",
    label: "Downloads",
    description: "Review transfers after download support is implemented.",
    emptyHeading: "Downloads are not available yet",
    emptyMessage:
      "Queue behavior, transfer controls, and torrent handling will be introduced separately.",
  },
  {
    id: "settings",
    label: "Settings",
    description: "Adjust the application options that are available today.",
    emptyHeading: "Other settings are not configured",
    emptyMessage:
      "Provider credentials and additional preferences will appear only with the features they control.",
  },
] as const;

const appearanceModes = [
  { id: "light", label: "Light" },
  { id: "dark", label: "Dark" },
  { id: "system", label: "System" },
] as const;

const iconPaths = {
  brand: ["m8 5 11 7-11 7V5Z"],
  dashboard: [
    "M3 3h7v7H3V3Z",
    "M14 3h7v7h-7V3Z",
    "M3 14h7v7H3v-7Z",
    "M14 14h7v7h-7v-7Z",
  ],
  discover: [
    "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18Z",
    "m15.5 8.5-2 5-5 2 2-5 5-2Z",
  ],
  library: [
    "M4 19.5V5a2 2 0 0 1 2-2h11v16H6a2 2 0 0 0-2 2Z",
    "M8 7h5",
    "M8 11h5",
  ],
  downloads: ["M12 3v12", "m7 10 5 5 5-5", "M5 21h14"],
  settings: [
    "M12 15.5a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7Z",
    "M19.4 15a1.7 1.7 0 0 0 .34 1.88l.06.06-2.12 2.12-.06-.06a1.7 1.7 0 0 0-1.88-.34 1.7 1.7 0 0 0-1 1.56V20h-3v-.08a1.7 1.7 0 0 0-1-1.56 1.7 1.7 0 0 0-1.88.34l-.06.06-2.12-2.12.06-.06A1.7 1.7 0 0 0 6.08 15 1.7 1.7 0 0 0 4.52 14H4v-3h.52a1.7 1.7 0 0 0 1.56-1 1.7 1.7 0 0 0-.34-1.88l-.06-.06 2.12-2.12.06.06a1.7 1.7 0 0 0 1.88.34 1.7 1.7 0 0 0 1-1.56V4h3v.78a1.7 1.7 0 0 0 1 1.56 1.7 1.7 0 0 0 1.88-.34l.06-.06 2.12 2.12-.06.06A1.7 1.7 0 0 0 19.4 10 1.7 1.7 0 0 0 20.96 11H21v3h-.04a1.7 1.7 0 0 0-1.56 1Z",
  ],
  light: [
    "M12 2v2",
    "M12 20v2",
    "m4.93-14.93 1.41-1.41",
    "m5.66 18.34 1.41-1.41",
    "M20 12h2",
    "M2 12h2",
    "m16.93 4.93 1.41 1.41",
    "m5.66 5.66 1.41 1.41",
    "M12 17a5 5 0 1 0 0-10 5 5 0 0 0 0 10Z",
  ],
  dark: ["M21 12.8A9 9 0 1 1 11.2 3 7 7 0 0 0 21 12.8Z"],
  system: ["M4 4h16v12H4V4Z", "M8 20h8", "M12 16v4"],
} as const;

type AppearanceMode = (typeof appearanceModes)[number]["id"];
type IconName = keyof typeof iconPaths;
type ResolvedTheme = Exclude<AppearanceMode, "system">;

const appearanceStorageKey = "auto-video-appearance";
const systemDarkModeQuery = "(prefers-color-scheme: dark)";

function AppIcon({ name }: { name: IconName }) {
  return (
    <svg
      aria-hidden="true"
      className="app-icon"
      fill="none"
      focusable="false"
      viewBox="0 0 24 24"
    >
      {iconPaths[name].map((path) => (
        <path d={path} key={path} />
      ))}
    </svg>
  );
}

function isAppearanceMode(value: string | null): value is AppearanceMode {
  return appearanceModes.some((mode) => mode.id === value);
}

export default function App() {
  const [activeDestination, setActiveDestination] = useState<
    (typeof destinations)[number]
  >(destinations[0]);
  const [appearance, setAppearance] = useState<AppearanceMode>(() => {
    const storedAppearance = window.localStorage.getItem(appearanceStorageKey);
    return isAppearanceMode(storedAppearance) ? storedAppearance : "system";
  });
  const [systemTheme, setSystemTheme] = useState<ResolvedTheme>(() =>
    window.matchMedia(systemDarkModeQuery).matches ? "dark" : "light",
  );
  const navigationItems = useRef<Array<HTMLButtonElement | null>>([]);

  useEffect(() => {
    const systemPreference = window.matchMedia(systemDarkModeQuery);
    const updateSystemTheme = (event: MediaQueryListEvent) => {
      setSystemTheme(event.matches ? "dark" : "light");
    };

    setSystemTheme(systemPreference.matches ? "dark" : "light");
    systemPreference.addEventListener("change", updateSystemTheme);
    return () => {
      systemPreference.removeEventListener("change", updateSystemTheme);
    };
  }, []);

  const resolvedTheme = appearance === "system" ? systemTheme : appearance;

  useEffect(() => {
    document.documentElement.dataset.appearance = appearance;
    document.documentElement.dataset.theme = resolvedTheme;
    window.localStorage.setItem(appearanceStorageKey, appearance);
  }, [appearance, resolvedTheme]);

  const moveNavigationFocus = (
    event: ReactKeyboardEvent<HTMLButtonElement>,
    currentIndex: number,
  ) => {
    let nextIndex: number;

    switch (event.key) {
      case "ArrowDown":
        nextIndex = (currentIndex + 1) % destinations.length;
        break;
      case "ArrowUp":
        nextIndex =
          (currentIndex - 1 + destinations.length) % destinations.length;
        break;
      case "Home":
        nextIndex = 0;
        break;
      case "End":
        nextIndex = destinations.length - 1;
        break;
      default:
        return;
    }

    event.preventDefault();
    navigationItems.current[nextIndex]?.focus();
  };

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <span className="brand__mark">
            <AppIcon name="brand" />
          </span>
          <span>
            <strong>Auto-Video</strong>
            <small>Desktop workspace</small>
          </span>
        </div>

        <nav aria-label="Primary navigation" className="primary-navigation">
          <p className="navigation-label">Workspace</p>
          <ul>
            {destinations.map((destination, index) => {
              const isActive = destination.id === activeDestination.id;

              return (
                <li key={destination.id}>
                  <button
                    aria-current={isActive ? "page" : undefined}
                    className="navigation-item"
                    onClick={() => setActiveDestination(destination)}
                    onKeyDown={(event) => moveNavigationFocus(event, index)}
                    ref={(element) => {
                      navigationItems.current[index] = element;
                    }}
                    type="button"
                  >
                    <AppIcon name={destination.id} />
                    <span>{destination.label}</span>
                  </button>
                </li>
              );
            })}
          </ul>
        </nav>

        <p className="sidebar__status">Presentation shell</p>
      </aside>

      <main className="workspace">
        <div className="workspace__content">
          <header className="page-header">
            <p className="page-eyebrow">Auto-Video workspace</p>
            <h1>{activeDestination.label}</h1>
            <p>{activeDestination.description}</p>
          </header>

          {activeDestination.id === "settings" ? (
            <div className="settings-content">
              <section
                aria-labelledby="appearance-heading"
                className="settings-card"
              >
                <div className="settings-card__heading">
                  <span className="empty-state__icon">
                    <AppIcon name="settings" />
                  </span>
                  <div>
                    <h2 id="appearance-heading">Appearance</h2>
                    <p>
                      Choose a palette for this application. System follows the
                      operating-system preference.
                    </p>
                  </div>
                </div>

                <fieldset className="appearance-options">
                  <legend>Appearance mode</legend>
                  <div>
                    {appearanceModes.map((mode) => (
                      <label className="appearance-option" key={mode.id}>
                        <input
                          checked={appearance === mode.id}
                          name="appearance"
                          onChange={() => setAppearance(mode.id)}
                          type="radio"
                          value={mode.id}
                        />
                        <span className="appearance-option__content">
                          <AppIcon name={mode.id} />
                          <span>{mode.label}</span>
                        </span>
                      </label>
                    ))}
                  </div>
                </fieldset>
              </section>

              <section
                aria-labelledby="settings-empty-heading"
                className="empty-state empty-state--compact"
              >
                <h2 id="settings-empty-heading">
                  {activeDestination.emptyHeading}
                </h2>
                <p>{activeDestination.emptyMessage}</p>
              </section>
            </div>
          ) : (
            <section
              aria-labelledby={`${activeDestination.id}-empty-heading`}
              className="empty-state"
            >
              <span className="empty-state__icon">
                <AppIcon name={activeDestination.id} />
              </span>
              <h2 id={`${activeDestination.id}-empty-heading`}>
                {activeDestination.emptyHeading}
              </h2>
              <p>{activeDestination.emptyMessage}</p>
            </section>
          )}
        </div>
      </main>
    </div>
  );
}
