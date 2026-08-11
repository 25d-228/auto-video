import { describe, expect, it } from "vitest";

import { hasSupportedLibraryExtension } from "./library-media";

describe("Library media extensions", () => {
  it("accepts every supported extension case-insensitively and rejects unsupported names", () => {
    for (const extension of [
      "mKv",
      "MP4",
      "avi",
      "WmV",
      "m4v",
      "TS",
      "Mov",
      "flv",
      "ISO",
      "rmvb",
      "WEBM",
      "mpg",
      "MPEG",
    ]) {
      expect(hasSupportedLibraryExtension(`/Library/media.${extension}`)).toBe(
        true,
      );
    }
    for (const path of [
      "/Library/media.txt",
      "/Library/media",
      "/Library/.mp4",
      "/Library/media.mp4.txt",
    ]) {
      expect(hasSupportedLibraryExtension(path)).toBe(false);
    }
  });
});
