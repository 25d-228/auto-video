import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const applicationEntry = readFileSync(resolve("src/main.tsx"), "utf8");
const categoryStyles = readFileSync(
  resolve("src/discover-category.css"),
  "utf8",
);

function ruleBody(selector: string) {
  const escapedSelector = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = categoryStyles.match(
    new RegExp(`${escapedSelector}\\s*\\{([^}]+)\\}`, "s"),
  );
  expect(match, `Expected a CSS rule for ${selector}`).not.toBeNull();
  return match?.[1] ?? "";
}

describe("Discover category segmented control", () => {
  it("rounds only the outside corners of the four-button group", () => {
    expect(applicationEntry).toContain('import "./discover-category.css";');

    const segment = ruleBody(".discover-category > div > label span");
    expect(segment).toContain("border-top-left-radius: 0;");
    expect(segment).toContain("border-top-right-radius: 0;");
    expect(segment).toContain("border-bottom-right-radius: 0;");
    expect(segment).toContain("border-bottom-left-radius: 0;");

    const firstSegment = ruleBody(
      ".discover-category > div > label:first-child span",
    );
    expect(firstSegment).toContain(
      "border-top-left-radius: var(--radius-lg);",
    );
    expect(firstSegment).toContain(
      "border-bottom-left-radius: var(--radius-lg);",
    );
    expect(firstSegment).not.toContain("border-top-right-radius");
    expect(firstSegment).not.toContain("border-bottom-right-radius");

    const lastSegment = ruleBody(
      ".discover-category > div > label:last-child span",
    );
    expect(lastSegment).toContain(
      "border-top-right-radius: var(--radius-lg);",
    );
    expect(lastSegment).toContain(
      "border-bottom-right-radius: var(--radius-lg);",
    );
    expect(lastSegment).not.toContain("border-top-left-radius");
    expect(lastSegment).not.toContain("border-bottom-left-radius");
  });
});
