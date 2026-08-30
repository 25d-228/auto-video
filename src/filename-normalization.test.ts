import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  applyLibraryFilenameNormalization,
  auditLibraryFilenames,
  loadLibraryFilenameNormalizationRecovery,
  parseFilenameNormalizationPlan,
  reconcileLibraryFilenameNormalization,
} from "@/filename-normalization";

const invokeMock = vi.fn();

beforeEach(() => {
  invokeMock.mockReset();
  window.__TAURI__ = { core: { invoke: invokeMock } };
});

const readyPlan = [
  "filename-normalization-v1",
  "a".repeat(40),
  "vr",
  "7",
  "1",
  "b".repeat(40),
  "ready",
  "DSVR-69",
  "FANZA",
  "dsvr00069",
  "DSVR-069",
  "Exact FANZA proof.",
  "1",
  "Incoming/DSVR-69.MKV",
  "Incoming/DSVR-069.MKV",
];

describe("filename normalization native boundary", () => {
  it("keeps FANZA transport identity separate from the verified maker code", async () => {
    invokeMock.mockResolvedValue(readyPlan);
    const plan = await auditLibraryFilenames("vr", "7");
    expect(plan.entries[0]).toMatchObject({
      provider: "FANZA",
      providerId: "dsvr00069",
      verifiedCode: "DSVR-069",
    });
    expect(invokeMock).toHaveBeenCalledWith("audit_library_filenames", {
      category: "vr",
      scanGeneration: "7",
    });
  });

  it("submits only the current plan and selected ready entry identities", async () => {
    const plan = parseFilenameNormalizationPlan(readyPlan, "vr", "7");
    invokeMock.mockResolvedValue(["8"]);
    await applyLibraryFilenameNormalization(plan, ["b".repeat(40)]);
    expect(invokeMock).toHaveBeenCalledWith(
      "apply_library_filename_normalization",
      {
        category: "vr",
        planId: "a".repeat(40),
        selectedEntryIds: ["b".repeat(40)],
      },
    );
    expect(JSON.stringify(invokeMock.mock.calls)).not.toContain("DSVR-069.MKV");
  });

  it("rejects stale, malformed, duplicate, and arbitrary native plans", () => {
    expect(() => parseFilenameNormalizationPlan(readyPlan, "adult", "7")).toThrow(
      "invalid data",
    );
    expect(() =>
      parseFilenameNormalizationPlan([...readyPlan, "extra"], "vr", "7"),
    ).toThrow("invalid data");
    expect(() =>
      parseFilenameNormalizationPlan(
        [...readyPlan.slice(0, 5), ...readyPlan.slice(5), ...readyPlan.slice(5)],
        "vr",
        "7",
      ),
    ).toThrow("invalid data");
  });

  it("reports every durable recovery path without exposing absolute paths", async () => {
    invokeMock.mockResolvedValue([
      "attention",
      "adult",
      "c".repeat(40),
      "2",
      "Incoming/CAWB-1.mp4",
      "Incoming/CAWB-001.mp4",
      "Incoming/CAWB-1 - Part 02.MKV",
      "Incoming/CAWB-001 - Part 02.MKV",
    ]);
    await expect(loadLibraryFilenameNormalizationRecovery()).resolves.toEqual({
      status: "attention",
      category: "adult",
      planId: "c".repeat(40),
      paths: [
        {
          current: "Incoming/CAWB-1.mp4",
          proposed: "Incoming/CAWB-001.mp4",
        },
        {
          current: "Incoming/CAWB-1 - Part 02.MKV",
          proposed: "Incoming/CAWB-001 - Part 02.MKV",
        },
      ],
    });
  });

  it("parses and resumes only an exact committed recovery identity", async () => {
    invokeMock.mockResolvedValueOnce([
      "committed",
      "vr",
      "d".repeat(40),
      "1",
      "Incoming/DSVR-069.MKV",
      "Incoming/DSVR-069.MKV",
      "1",
      "DSVR-69",
    ]);
    const recovery = await loadLibraryFilenameNormalizationRecovery();
    expect(recovery).toEqual({
      status: "committed",
      category: "vr",
      planId: "d".repeat(40),
      paths: [
        {
          current: "Incoming/DSVR-069.MKV",
          proposed: "Incoming/DSVR-069.MKV",
        },
      ],
      affectedCodes: ["DSVR-69"],
    });
    if (recovery.status !== "committed") throw new Error("fixture must be committed");
    invokeMock.mockResolvedValueOnce(["8", "/VR/DSVR-069.MKV", "1"]);
    await reconcileLibraryFilenameNormalization(recovery);
    expect(invokeMock).toHaveBeenLastCalledWith(
      "reconcile_library_filename_normalization",
      {
        category: "vr",
        planId: "d".repeat(40),
      },
    );
    expect(JSON.stringify(invokeMock.mock.calls.at(-1))).not.toContain("DSVR-069.MKV");
  });
});
