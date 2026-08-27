import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

import { buttonVariants } from "../src/components/ui/button";

const presetConfiguration = JSON.parse(
  readFileSync(resolve("components.json"), "utf8"),
) as {
  style: string;
  tailwind: { baseColor: string; cssVariables: boolean };
  iconLibrary: string;
  menuColor: string;
  menuAccent: string;
};
const applicationStyles = readFileSync(resolve("src/index.css"), "utf8");
const packageJson = JSON.parse(
  readFileSync(resolve("package.json"), "utf8"),
) as { dependencies: Record<string, string> };
const buttonSource = readFileSync(
  resolve("src/components/ui/button.tsx"),
  "utf8",
);
const inputSource = readFileSync(resolve("src/components/ui/input.tsx"), "utf8");

function ruleBody(selector: string) {
  const escapedSelector = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = applicationStyles.match(
    new RegExp(`${escapedSelector}\\s*\\{([^}]+)\\}`, "s"),
  );
  expect(match, `Expected a CSS rule for ${selector}`).not.toBeNull();
  return match?.[1] ?? "";
}

describe("default Nova preset contract", () => {
  it("records the pinned Base UI Nova configuration", () => {
    expect(presetConfiguration).toMatchObject({
      style: "base-nova",
      tailwind: {
        baseColor: "neutral",
        cssVariables: true,
      },
      iconLibrary: "lucide",
      menuColor: "default",
      menuAccent: "subtle",
    });
  });

  it("uses Geist, Lucide, neutral tokens, charts, and the default radius", () => {
    expect(applicationStyles).toContain('@import "@fontsource-variable/geist";');
    expect(applicationStyles).toContain('--font-sans: "Geist Variable", sans-serif;');
    expect(applicationStyles).toContain("--font-heading: var(--font-sans);");
    expect(packageJson.dependencies).toMatchObject({
      "@fontsource-variable/geist": "5.3.0",
      "lucide-react": "1.34.0",
      shadcn: "4.19.0",
    });
    expect(packageJson.dependencies).not.toHaveProperty("@phosphor-icons/react");
    expect(packageJson.dependencies).not.toHaveProperty("@fontsource-variable/noto-sans");
    expect(packageJson.dependencies).not.toHaveProperty("@fontsource-variable/playfair-display");
    expect(applicationStyles).toContain("--primary: oklch(0.205 0 0);");
    expect(applicationStyles).toContain("--chart-3: oklch(0.439 0 0);");
    expect(applicationStyles).toContain("--sidebar-primary: oklch(0.205 0 0);");
    expect(applicationStyles).toContain(":root[data-theme=\"dark\"]");
    expect(applicationStyles).toContain("--radius: 0.625rem;");
    expect(applicationStyles).not.toContain("--radius: 0;");
    expect(applicationStyles).not.toContain("Noto Sans");
    expect(applicationStyles).not.toContain("Playfair Display");

    const surfaceRadii = Array.from(
      applicationStyles.matchAll(/border-radius:\s*([^;]+);/g),
      (match) => match[1],
    );
    expect(surfaceRadii).not.toContain("0");
    expect(surfaceRadii).toEqual(
      expect.arrayContaining(["var(--radius-lg)", "var(--radius-xl)"]),
    );
  });

  it("uses the generated Nova Button and Input contracts", () => {
    const defaultButton = buttonVariants({ variant: "default" }).split(/\s+/);
    const outlineButton = buttonVariants({ variant: "outline" }).split(/\s+/);
    const destructiveButton = buttonVariants({
      variant: "destructive",
    }).split(/\s+/);

    expect(defaultButton).toEqual(
      expect.arrayContaining([
        "rounded-lg",
        "text-sm",
        "font-medium",
        "bg-primary",
        "text-primary-foreground",
        "hover:bg-primary/80",
        "disabled:pointer-events-none",
        "disabled:opacity-50",
        "focus-visible:border-ring",
        "focus-visible:ring-3",
        "focus-visible:ring-ring/50",
      ]),
    );
    expect(outlineButton).toEqual(
      expect.arrayContaining([
        "border-border",
        "bg-background",
        "hover:bg-muted",
        "hover:text-foreground",
        "dark:bg-input/30",
        "dark:hover:bg-input/50",
      ]),
    );
    expect(destructiveButton).toEqual(
      expect.arrayContaining([
        "bg-destructive/10",
        "text-destructive",
        "hover:bg-destructive/20",
        "focus-visible:border-destructive/40",
        "focus-visible:ring-destructive/20",
        "dark:bg-destructive/20",
        "dark:hover:bg-destructive/30",
        "dark:focus-visible:ring-destructive/40",
      ]),
    );
    expect(buttonSource).toContain("rounded-lg border border-transparent");
    expect(buttonSource).toContain("text-sm font-medium whitespace-nowrap");
    expect(buttonSource).toContain(
      '"border-border bg-background hover:bg-muted hover:text-foreground',
    );
    expect(buttonSource).toContain('default:\n          "h-8 gap-1.5 px-2.5');
    expect(inputSource).toContain('data-slot="input"');
    expect(inputSource).toContain("rounded-lg border border-input");
    expect(inputSource).toContain("focus-visible:ring-3");
  });

  it("keeps native-control inheritance below Nova utilities", () => {
    const baseLayerStart = applicationStyles.indexOf("@layer base {");
    const baseLayerEnd = applicationStyles.indexOf(
      "\n}\n\n* {",
      baseLayerStart,
    );
    expect(baseLayerStart).toBeGreaterThanOrEqual(0);
    expect(baseLayerEnd).toBeGreaterThan(baseLayerStart);

    const baseLayer = applicationStyles.slice(baseLayerStart, baseLayerEnd);
    const unlayeredStyles =
      applicationStyles.slice(0, baseLayerStart) +
      applicationStyles.slice(baseLayerEnd + 2);
    expect(baseLayer).toMatch(
      /button,\s*input,\s*select\s*{\s*font:\s*inherit;/s,
    );
    expect(baseLayer).toMatch(/button\s*{\s*color:\s*inherit;/s);
    expect(unlayeredStyles).not.toMatch(
      /(^|\n)\s*button,\s*input,\s*select\s*{\s*font:\s*inherit;/s,
    );
    expect(unlayeredStyles).not.toMatch(
      /(^|\n)\s*button\s*{\s*color:\s*inherit;/s,
    );
  });

  it("uses the complete workspace width while keeping descriptions readable", () => {
    const shell = ruleBody(".app-shell");
    const workspace = ruleBody(".workspace__content");
    expect(shell).toContain(
      "grid-template-columns: 13.5rem minmax(0, 1fr);",
    );
    expect(workspace).toContain("width: 100%;");
    expect(workspace).toContain("margin: 0;");
    expect(workspace).toContain(
      "padding: 1.5rem clamp(1.5rem, 4vw, 3rem);",
    );
    expect(workspace).not.toContain("max-width:");
    expect(workspace).not.toContain("62rem");
    expect(ruleBody(".page-header")).toContain("max-width: 42rem;");
    expect(applicationStyles).not.toContain("width: min(100%, 62rem);");
  });

  it("uses one viewport shell with independent sidebar and workspace scrolling", () => {
    const shell = ruleBody(".app-shell");
    const sidebar = ruleBody(".sidebar");
    const workspace = ruleBody(".workspace");
    const root = ruleBody("#root");

    expect(shell).toContain("height: 100vh;");
    expect(shell).toContain("min-height: 32.5rem;");
    expect(shell).toContain("overflow: hidden;");
    expect(sidebar).toContain("min-height: 0;");
    expect(sidebar).toContain("overflow-y: auto;");
    expect(workspace).toContain("min-height: 0;");
    expect(workspace).toContain("overflow-y: auto;");
    expect(applicationStyles).toMatch(
      /body\s*\{\s*min-width: 45rem;\s*margin: 0;\s*overflow: hidden;/,
    );
    expect(root).toContain("height: 100%;");
  });

  it("applies the Nova surface contract across pages, states, cards, and dialogs", () => {
    const sharedSurfaces = ruleBody(
      ".empty-state,\n.dashboard-library-summary,\n.settings-card,\n.library-toolbar,\n.movie-card,\n.discover-card,\n.vr-download-card,\n.tmdb-attribution",
    );
    const providerCard = ruleBody(".provider-browse-card");
    const dialogs = ruleBody(
      ".movie-details__popup,\n.vr-releases__popup,\n.vr-torrent__popup,\n.movie-metadata__popup,\n.trash-dialog__popup",
    );

    for (const surface of [sharedSurfaces, providerCard, dialogs]) {
      expect(surface).toContain("border: 1px solid var(--border);");
    }
    expect(sharedSurfaces).toContain("border-radius: var(--radius-xl);");
    expect(providerCard).toContain("border-radius: var(--radius-xl);");
    expect(dialogs).toContain("border-radius: var(--radius-xl);");
    expect(applicationStyles).not.toContain("border-radius: 0;");
  });

  it("uses responsive ordered toolbar rows without a fixed request block", () => {
    const controls = ruleBody(".provider-browse-controls");
    const request = ruleBody(".provider-browse-controls__request");
    expect(controls).toContain("display: flex;");
    expect(controls).toContain("flex-wrap: wrap;");
    expect(controls).toContain("min-width: 0;");
    expect(request).toContain("display: flex;");
    expect(request).toContain("flex-wrap: wrap;");
    expect(request).toContain("min-width: 0;");
    expect(ruleBody(".provider-select-label")).toContain(
      "flex: 0 1 9rem;",
    );
    expect(ruleBody(".provider-select-label select")).toContain("width: 100%;");
    expect(controls).not.toContain("grid-template-columns:");
    expect(request).not.toContain("grid-template-columns:");
  });

  it("keeps provider cards on a fixed cover height and natural-width wrapped gallery", () => {
    const viewport = ruleBody(
      ".media-gallery--provider-browse .media-gallery__viewport",
    );
    const grid = ruleBody(".provider-browse-grid");
    const cover = ruleBody(".provider-browse-card__cover");
    expect(viewport).toContain("min-height: 0;");
    expect(viewport).toContain("flex: 1;");
    expect(viewport).not.toContain("clamp(");
    expect(viewport).not.toContain("overflow:");
    expect(ruleBody(".media-gallery__viewport")).toContain("overflow: hidden;");
    expect(ruleBody(".workspace__content--bounded-gallery")).toContain(
      "overflow: hidden;",
    );
    expect(grid).toContain("display: flex;");
    expect(grid).toContain("flex-wrap: wrap;");
    expect(grid).toContain("align-content: flex-start;");
    expect(cover).toContain("height: 180px;");
    expect(applicationStyles).not.toContain(
      ".media-gallery--provider-browse .media-grid",
    );
  });

  it("keeps cover actions compact, vertical, transparent, and hidden until hover or focus", () => {
    const actions = ruleBody(".provider-browse-card__actions");
    const coverContrast = ruleBody(".provider-browse-card__cover::after");
    const actionButton = ruleBody(
      '.provider-browse-card__actions [data-slot="button"]',
    );
    const actionIcon = ruleBody(
      '.provider-browse-card__actions [data-slot="button"] svg',
    );
    const copyAction = ruleBody(
      ".provider-browse-card__actions .title-copy-button",
    );
    const revealedActions = ruleBody(
      ".provider-browse-card:hover .provider-browse-card__actions,\n.provider-browse-card:focus-within .provider-browse-card__actions,\n.provider-browse-card__actions:focus-within",
    );
    const revealedCopyAction = ruleBody(
      ".provider-browse-card:hover .provider-browse-card__actions .title-copy-button,\n.provider-browse-card:focus-within .provider-browse-card__actions .title-copy-button,\n.provider-browse-card__actions:focus-within .title-copy-button",
    );
    const revealedCoverContrast = ruleBody(
      ".provider-browse-card:hover .provider-browse-card__cover::after,\n.provider-browse-card:focus-within .provider-browse-card__cover::after",
    );
    expect(actions).toContain("flex-direction: column;");
    expect(actions).toContain("right: 0.375rem;");
    expect(actions).toContain("bottom: 0.375rem;");
    expect(actions).toContain("left: 0.375rem;");
    expect(actions).toContain("max-height: calc(100% - 0.75rem);");
    expect(actions).toContain("background: transparent;");
    expect(actions).toContain("border: 0;");
    expect(actions).toContain("opacity: 0;");
    expect(actions).toContain("pointer-events: none;");
    expect(actions).not.toContain("background: var(--card);");
    expect(actions).not.toContain("box-shadow:");
    expect(actionButton).toContain("width: fit-content;");
    expect(actionButton).toContain("max-width: 100%;");
    expect(actionButton).toContain("background: transparent;");
    expect(actionButton).toContain("color: white;");
    expect(actionButton).toContain("font-size: 0.75rem;");
    expect(actionButton).toContain("text-shadow:");
    expect(actionButton).toContain("text-transform: none;");
    expect(actionButton).toContain("letter-spacing: normal;");
    expect(actionButton).toContain("white-space: normal;");
    expect(actionIcon).toContain("filter: drop-shadow(");
    expect(coverContrast).toContain("height: 5.5rem;");
    expect(coverContrast).toContain("background: linear-gradient(");
    expect(coverContrast).toContain("opacity: 0;");
    expect(coverContrast).toContain("pointer-events: none;");
    expect(coverContrast).not.toContain("height: 100%;");
    expect(copyAction).not.toContain("pointer-events: auto;");
    expect(copyAction).toContain("pointer-events: none;");
    expect(applicationStyles).not.toContain(
      '.title-copy-button[data-copy-state="success"],\n.title-copy-button[data-copy-state="error"]',
    );
    expect(revealedActions).toContain("opacity: 1;");
    expect(revealedActions).toContain("transform: translateY(0);");
    expect(revealedActions).toContain("pointer-events: auto;");
    expect(revealedCopyAction).toContain("pointer-events: auto;");
    expect(revealedCoverContrast).toContain("opacity: 1;");
    expect(applicationStyles).toContain(
      '.provider-browse-card__actions [data-slot="button"]:focus-visible {\n  outline: 2px solid white;\n  outline-offset: 0;\n  box-shadow: 0 0 0 1px black;',
    );
    expect(applicationStyles).not.toContain(
      ".provider-browse-card__details-control",
    );
    expect(applicationStyles).not.toContain(
      '[data-actions-only="true"]',
    );
    expect(applicationStyles).not.toContain("data-narrow-cover");
    const hoverlessMedia = applicationStyles.slice(
      applicationStyles.indexOf("@media (hover: none)"),
      applicationStyles.indexOf("@media (max-height: 36rem)"),
    );
    expect(hoverlessMedia).not.toContain("provider-browse-card__actions");
  });

  it.each([80, 266])(
    "keeps disclosed action rectangles inside a %ipx cover",
    (coverWidth) => {
      const actions = ruleBody(".provider-browse-card__actions");
      const actionButton = ruleBody(
        '.provider-browse-card__actions [data-slot="button"]',
      );
      const inset = Number(
        actions.match(/left:\s*([\d.]+)rem;/)?.[1] ?? Number.NaN,
      );
      const pixelsPerRem = 16;
      const coverHeight = 180;
      const actionRectangle = {
        bottom: coverHeight - inset * pixelsPerRem,
        left: inset * pixelsPerRem,
        right: coverWidth - inset * pixelsPerRem,
        top: inset * pixelsPerRem,
      };

      expect(Number.isFinite(inset)).toBe(true);
      expect(actionRectangle.left).toBeGreaterThanOrEqual(0);
      expect(actionRectangle.top).toBeGreaterThanOrEqual(0);
      expect(actionRectangle.right).toBeLessThanOrEqual(coverWidth);
      expect(actionRectangle.bottom).toBeLessThanOrEqual(coverHeight);
      expect(actionRectangle.right).toBeGreaterThan(actionRectangle.left);
      expect(actionRectangle.bottom).toBeGreaterThan(actionRectangle.top);
      expect(actions).toContain("bottom: 0.375rem;");
      expect(actions).toContain("max-height: calc(100% - 0.75rem);");
      expect(actionButton).toContain("max-width: 100%;");
      expect(actionButton).toContain("overflow-wrap: anywhere;");
    },
  );

  it("uses stable selected surfaces instead of underlined navigation or segments", () => {
    const navigation = ruleBody('.navigation-item[aria-current="page"]');
    const option = ruleBody(".discover-category label span");
    const selectedOption = ruleBody(
      ".discover-category input:checked + span",
    );
    expect(navigation).toContain("background: var(--sidebar-accent);");
    expect(navigation).not.toContain("border-bottom-color:");
    expect(option).toContain("border: 1px solid var(--border);");
    expect(option).not.toContain("border-bottom:");
    expect(selectedOption).toContain("background: var(--primary);");
    expect(selectedOption).toContain("color: var(--primary-foreground);");
    expect(selectedOption).not.toContain("border-bottom-color:");
  });

  it("reserves a stable title action column and reveals it for hover and focus", () => {
    expect(applicationStyles).toContain(
      "grid-template-columns: minmax(0, 1fr) 4.25rem;",
    );
    expect(applicationStyles).toContain(
      ".media-title-row:hover .title-copy-button,",
    );
    expect(applicationStyles).toContain(
      ".media-title-row:focus-within .title-copy-button,",
    );
    expect(applicationStyles).toContain(
      '.title-copy-button[data-copy-state="success"],',
    );
    expect(applicationStyles).toContain("@media (hover: none)");
  });

  it("keeps verified torrent names and paths usable in the minimum window", () => {
    expect(applicationStyles).toContain(".vr-torrent__popup {");
    expect(applicationStyles).toContain("width: min(42rem, 100%);");
    expect(applicationStyles).toContain("max-height: 100%;");
    expect(applicationStyles).toContain(
      ".vr-torrent__file-selection li span:nth-child(2) {",
    );
    expect(applicationStyles).toContain("overflow-wrap: anywhere;");
    expect(applicationStyles).toContain("white-space: pre-wrap;");
  });
});
