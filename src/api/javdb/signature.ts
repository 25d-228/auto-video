// javdb mobile API request signature — TypeScript port of the Python sidecar's
// jdb_signature() / _jdb_dec() (see sidecar/av_proxy.py and docs/javdb-api.md).
//
// The javdb Android app authenticates every backend request with a `jdsignature`
// header of the shape:
//
//     <unix_ts>.lpw6vgqzsp.<md5(unix_ts + SECRET)>
//
// where the middle token ("lpw6vgqzsp") and the SECRET are NOT hardcoded in the
// app — they are obfuscated as base64 blobs that get XOR/subtract-decoded against
// md5("30820"). We port that exact blob-decode here (instead of hardcoding the
// literals) so the values are derived the same way the Python sidecar derives
// them, which is more robust if either blob is ever rotated. The recon-confirmed
// decoded values are middle == "lpw6vgqzsp" and SECRET == the 128-hex-char string
// below; the unit tests assert the decode reproduces them.

import { md5 } from "js-md5"

/** Live javdb backend host (rotates; treat as configurable). */
export const JAVDB_API_HOST = "apidd.spthgb.com"

/** User-Agent the javdb Flutter app sends; required to look like the real client. */
export const JAVDB_UA = "Dart/3.5 (dart:io)"

// --- obfuscated signature material (verbatim from sidecar/av_proxy.py) ---------
// JDB_CERT5 is the decode key; JDB_BLOB_A decodes to the SECRET; JDB_BLOB_B
// decodes to the middle token.
const JDB_CERT5 = "30820"
const JDB_BLOB_A =
  "WzE3OCwyMTksMTI3LDE2MSwxODksMTYyLDEyMywxMDMsMTM3LDIxMCwxMjMsMjE5LDE4OSwxNzksMTIzLDIwMiwxMzksMTUwLDEzMywxNjAsMTI2LDIwNywxNjYsMTUxLDE0NiwxNTksMTg4LDEwMCwxMzgsMTM2LDE3NiwxNjEsMTQyLDEwMywxMzUsMTYwLDE0MiwxNzUsMTYwLDEwNCwxMzAsMTIxLDExOCwxMDYsMTMyLDEyNCwxMzAsMTA0LDEzMSwxMjEsMTI2LDE3MywxNDMsMTQwLDEzOCwxMDQsMTMwLDE1OSwxMTgsMTc1LDE0MiwxNTksMTYxLDE1OSwxNDMsMTI0LDEyMywxNjEsMTMxLDEzNywxMzQsMTAxLDEzMSwxNzUsMTU2LDEwMSwxMzEsMTc1LDE1NywxNTcsMTMwLDEzNywxNjAsMTA2LDE0MywxMzcsMTUzLDE2MCwxMzEsMTQwLDEyMiwxMDMsMTQzLDEzNywxMjMsMTU3LDEzMSwxMzcsMTUyLDEwMywxMzIsMTM3LDEyMiwxNzMsMTMwLDE1OSwxMzEsMTU5LDEzMCwxNDAsMTIyLDEwNiwxMzAsMTc1LDEyMywxNTksMTMwLDEyMSwxMzgsMTA0LDEzMiwxMjEsMTM0LDE3NCwxNDMsMTYyLDEyNiwxMDQsMTMwLDEwMywxMjcsMTU3LDEzMCwxMDMsMTI2LDE3NSwxNDIsMTc1LDE1NiwxNzUsMTQyLDE2MiwxMzEsMTYwLDEzMSwxNTksMTYxLDE1OSwxMzAsMTM3LDE1MywxNTksMTQyLDEwMywxNDIsMTczLDEzMSwxNzUsMTM0LDE3MiwxMzIsMTIxLDEyMywxNjEsMTMwLDEwMywxMzQsMTA1LDE0MiwxNDAsMTIyLDExNF0="
const JDB_BLOB_B = "WzE5OCwxNjksMTIzLDEwNiwxNzcsMTY2LDE0MCwxNjIsMTQ3LDE4OSwxNjIsMjE5LDE5OSwxMjIsMTE4LDE1OF0="

// `atob` and `TextDecoder` are available both in the Tauri webview and in the
// vitest "node" environment (modern Node exposes them as globals), so no Node
// `Buffer` dependency is needed — which keeps this file clean under the
// DOM-only tsconfig.app.json.

/** Decode base64 to raw bytes. */
function b64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64)
  const bytes = new Uint8Array(bin.length)
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i)
  return bytes
}

/** Decode base64 to a UTF-8 string. */
function b64ToUtf8(b64: string): string {
  return new TextDecoder().decode(b64ToBytes(b64))
}

/** Encode raw bytes to a UTF-8 string. */
function bytesToUtf8(bytes: Uint8Array): string {
  return new TextDecoder().decode(bytes)
}

/**
 * Port of `_jdb_dec(key, blob)` from av_proxy.py:
 *   md = md5(key) (hex)
 *   lst = JSON.parse(base64_decode(blob))        // an int array
 *   raw[i] = (lst[i] - md[min(i, len-1)].charCodeAt) & 0xff
 *   return base64_decode(raw) as utf-8
 * The md5-hex string is shorter than the blob, so the last hex char is reused
 * for every index past the end (the `i <= last ? i : last` clamp in Python).
 */
function jdbDec(key: string, blob: string): string {
  const md = md5(key) // lowercase hex, 32 chars
  const last = md.length - 1
  const lst = JSON.parse(b64ToUtf8(blob)) as number[]
  const raw = new Uint8Array(lst.length)
  for (let i = 0; i < lst.length; i++) {
    const idx = i <= last ? i : last
    raw[i] = (lst[i] - md.charCodeAt(idx)) & 0xff
  }
  // `raw` is itself a base64 (ASCII) byte string; decode it once more.
  const innerB64 = bytesToUtf8(raw)
  return bytesToUtf8(b64ToBytes(innerB64))
}

/** Decoded SECRET used in the md5 of the signature (derived, not hardcoded). */
export function javdbSecret(): string {
  return jdbDec(JDB_CERT5, JDB_BLOB_A)
}

/** Decoded middle token (recon-confirmed "lpw6vgqzsp"). */
export function javdbMiddle(): string {
  return jdbDec(JDB_CERT5, JDB_BLOB_B)
}

/**
 * Build the `jdsignature` header value.
 * Port of `jdb_signature()`:  `${ts}.${MIDDLE}.${md5(`${ts}${SECRET}`)}`
 *
 * @param tsSeconds Unix time in **seconds**. Defaults to now.
 */
export function signatureHeader(tsSeconds?: number): string {
  const ts = tsSeconds ?? Math.floor(Date.now() / 1000)
  const middle = javdbMiddle()
  const secret = javdbSecret()
  const digest = md5(`${ts}${secret}`)
  return `${ts}.${middle}.${digest}`
}
