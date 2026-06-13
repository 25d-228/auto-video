import { md5 } from "js-md5"
import { describe, expect, it } from "vitest"
import {
  JAVDB_API_HOST,
  JAVDB_UA,
  javdbMiddle,
  javdbSecret,
  signatureHeader,
} from "@/api/javdb/signature"

// The recon-confirmed literals (also documented in docs/javdb-api.md and the
// task brief). The signature module DERIVES these by blob-decode; the test
// asserts the decode reproduces them, so we can trust the derivation.
const EXPECTED_MIDDLE = "lpw6vgqzsp"
const EXPECTED_SECRET =
  "71cf27bb3c0bcdf207b64abecddc970098c7421ee7203b9cdae54478478a199e7d5a6e1a57691123c1a931c057842fb73ba3b3c83bcd69c17ccf174081e3d8aa"

describe("javdb signature constants", () => {
  it("exports the live host and Dart UA", () => {
    expect(JAVDB_API_HOST).toBe("apidd.spthgb.com")
    expect(JAVDB_UA).toBe("Dart/3.5 (dart:io)")
  })
})

describe("blob decode", () => {
  it("derives the recon-confirmed middle token", () => {
    expect(javdbMiddle()).toBe(EXPECTED_MIDDLE)
  })

  it("derives the recon-confirmed SECRET (128 hex chars)", () => {
    const secret = javdbSecret()
    expect(secret).toBe(EXPECTED_SECRET)
    expect(secret).toHaveLength(128)
    expect(secret).toMatch(/^[0-9a-f]{128}$/)
  })
})

describe("signatureHeader", () => {
  it("has the three-dot-part shape with the right middle and a 32-hex tail", () => {
    const sig = signatureHeader(1700000000)
    const parts = sig.split(".")
    expect(parts).toHaveLength(3)
    expect(parts[0]).toBe("1700000000")
    expect(parts[1]).toBe(EXPECTED_MIDDLE)
    expect(parts[2]).toMatch(/^[0-9a-f]{32}$/)
  })

  it("third part equals md5(ts + SECRET) computed independently", () => {
    const ts = 1700000000
    // Compute the expected digest independently from the known SECRET literal,
    // NOT by reusing the module's secret derivation.
    const expectedDigest = md5(`${ts}${EXPECTED_SECRET}`)
    const sig = signatureHeader(ts)
    expect(sig.split(".")[2]).toBe(expectedDigest)
    // Full cross-check value (matches the Python sidecar for ts=1700000000):
    expect(sig).toBe(`1700000000.lpw6vgqzsp.${expectedDigest}`)
  })

  it("uses Math.floor(Date.now()/1000) when no ts is given", () => {
    const before = Math.floor(Date.now() / 1000)
    const sig = signatureHeader()
    const after = Math.floor(Date.now() / 1000)
    const ts = Number(sig.split(".")[0])
    expect(Number.isInteger(ts)).toBe(true)
    expect(ts).toBeGreaterThanOrEqual(before)
    expect(ts).toBeLessThanOrEqual(after)
  })
})
