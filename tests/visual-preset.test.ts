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
});
