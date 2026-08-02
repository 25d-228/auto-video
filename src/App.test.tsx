import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import App from "./App";

afterEach(cleanup);

describe("Auto-Video application entry surface", () => {
  it("renders the application identity and rebuild status", () => {
    render(<App />);

    expect(
      screen.getByRole("heading", { level: 1, name: "Auto-Video" }),
    ).toBeTruthy();
    expect(
      screen.getByText(
        "Application features will be added in separately scoped work.",
      ),
    ).toBeTruthy();
  });
});
