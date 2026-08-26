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

function ruleBody(selector: string) {
  const escapedSelector = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = applicationStyles.match(
    new RegExp(`${escapedSelector}\\s*\\{([^}]+)\\}`, "s"),
  );
  expect(match, `Expected a CSS rule for ${selector}`).not.toBeNull();
  return match?.[1] ?? "";
}

describe("Sera preset contract", () => {
  it("records the Base UI Sera configuration with Neutral and Phosphor", () => {
    expect(presetConfiguration).toMatchObject({
      style: "base-sera",
      tailwind: {
        baseColor: "neutral",
        cssVariables: true,
      },
      iconLibrary: "phosphor",
      menuColor: "default",
      menuAccent: "subtle",
    });
  });

  it("defines the approved Sera fonts, Red theme, Rose charts, and square surfaces", () => {
    expect(applicationStyles).toContain(
      '@import "@fontsource-variable/noto-sans";',
    );
    expect(applicationStyles).toContain(
      '@import "@fontsource-variable/playfair-display";',
    );
    expect(applicationStyles).toContain(
      '--font-sans: "Noto Sans Variable", sans-serif;',
    );
    expect(applicationStyles).toContain(
      '--font-heading: "Playfair Display Variable", serif;',
    );
    expect(packageJson.dependencies).toMatchObject({
      "@fontsource-variable/noto-sans": "5.3.0",
      "@fontsource-variable/playfair-display": "5.3.0",
    });
    expect(packageJson.dependencies).not.toHaveProperty(
      "@fontsource-variable/inter",
    );
    expect(packageJson.dependencies).not.toHaveProperty(
      "@fontsource-variable/geist-mono",
    );
    expect(applicationStyles).toContain(
      "--primary: oklch(0.505 0.213 27.518);",
    );
    expect(applicationStyles).toContain(
      "--chart-3: oklch(0.586 0.253 17.585);",
    );
    expect(applicationStyles).toContain(":root[data-theme=\"dark\"]");
    expect(applicationStyles).toContain("--radius: 0;");

    const surfaceRadii = Array.from(
      applicationStyles.matchAll(/border-radius:\s*([^;]+);/g),
      (match) => match[1],
    );
    expect(new Set(surfaceRadii)).toEqual(new Set(["0", "var(--radius)"]));
  });

  it("keeps the shared Button on the current Sera contract", () => {
    const defaultButton = buttonVariants({ variant: "default" }).split(/\s+/);
    const outlineButton = buttonVariants({ variant: "outline" }).split(/\s+/);
    const destructiveButton = buttonVariants({
      variant: "destructive",
    }).split(/\s+/);

    expect(defaultButton).toEqual(
      expect.arrayContaining([
        "text-xs",
        "font-semibold",
        "tracking-widest",
        "uppercase",
        "bg-primary",
        "text-primary-foreground",
        "hover:bg-primary/80",
        "disabled:pointer-events-none",
        "disabled:opacity-50",
        "focus-visible:border-ring",
        "focus-visible:ring-2",
        "focus-visible:ring-ring/30",
      ]),
    );
    expect(outlineButton).toEqual(
      expect.arrayContaining([
        "border-border",
        "bg-transparent",
        "hover:bg-muted",
        "hover:text-foreground",
        "dark:hover:bg-input/30",
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
    expect(buttonSource).toContain(
      "rounded-none border border-transparent bg-clip-padding text-xs font-semibold tracking-widest whitespace-nowrap uppercase",
    );
    expect(buttonSource).toContain(
      '"border-border bg-transparent hover:bg-muted hover:text-foreground',
    );
    expect(buttonSource).toContain(
      'default:\n          "h-10 gap-1.5 px-6',
    );
    expect(buttonSource).toContain('xs: "h-7 gap-1 px-3');
    expect(buttonSource).not.toContain("rounded-md");
    expect(buttonSource).not.toContain("text-xs/relaxed font-medium");
  });

  it("keeps native-control inheritance in the base layer below Sera utilities", () => {
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
