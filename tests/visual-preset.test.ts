import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

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

describe("Mira preset contract", () => {
  it("records the Base UI Mira configuration with Neutral and Phosphor", () => {
    expect(presetConfiguration).toMatchObject({
      style: "base-mira",
      tailwind: {
        baseColor: "neutral",
        cssVariables: true,
      },
      iconLibrary: "phosphor",
      menuColor: "default",
      menuAccent: "subtle",
    });
  });

  it("defines the approved fonts, Red theme, Rose charts, and zero radius", () => {
    expect(applicationStyles).toContain(
      '@import "@fontsource-variable/inter";',
    );
    expect(applicationStyles).toContain(
      '@import "@fontsource-variable/geist-mono";',
    );
    expect(applicationStyles).toContain(
      '--font-sans: "Inter Variable", sans-serif;',
    );
    expect(applicationStyles).toContain(
      '--font-heading: "Geist Mono Variable", monospace;',
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
    expect(new Set(surfaceRadii)).toEqual(new Set(["var(--radius)"]));
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
