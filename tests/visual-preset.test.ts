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

  it("uses responsive ordered toolbar tracks without a fixed request block", () => {
    const controls = ruleBody(".provider-browse-controls");
    const request = ruleBody(".provider-browse-controls__request");
    expect(controls).toContain("display: grid;");
    expect(controls).toContain("width: 100%;");
    expect(controls).toContain("min-width: 0;");
    expect(controls).toContain(
      "grid-template-columns: max-content minmax(0, 1fr);",
    );
    expect(request).toContain(
      "repeat(auto-fit, minmax(min(100%, 7.5rem), 1fr))",
    );
    expect(request).toContain("min-width: 0;");
    expect(ruleBody(".provider-select-label")).toContain("min-width: 0;");
    expect(ruleBody(".provider-select-label select")).toContain("width: 100%;");
    expect(applicationStyles).not.toContain("flex: 1 1 24rem;");
  });

  it("places cover actions on one opaque semantic surface for hover and focus", () => {
    const actions = ruleBody(".provider-browse-card__actions");
    expect(actions).toContain("background: var(--card);");
    expect(actions).toContain("color: var(--card-foreground);");
    expect(actions).toContain("border: 1px solid var(--border);");
    expect(actions).not.toContain("background: transparent;");
    expect(applicationStyles).toContain(
      ".provider-browse-card:hover .provider-browse-card__actions,\n.provider-browse-card:focus-within .provider-browse-card__actions",
    );
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
