import { createHash } from "node:crypto";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const iconDirectory = resolve("src-tauri/icons");
const lightSourcePath = resolve(iconDirectory, "auto-video-icon-light.svg");
const darkSourcePath = resolve(iconDirectory, "auto-video-icon-dark.svg");
const applicationIconPath = resolve(iconDirectory, "icon.png");
const macosIconPath = resolve(iconDirectory, "icon.icns");
const windowsIconPath = resolve(iconDirectory, "icon.ico");
const tauriConfiguration = JSON.parse(
  readFileSync(resolve("src-tauri/tauri.conf.json"), "utf8"),
) as { bundle?: { icon?: string[] } };

function sha256(path: string) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function readPngDimensions(buffer: Buffer, offset: number) {
  expect(buffer.subarray(offset, offset + 8)).toEqual(
    Buffer.from("89504e470d0a1a0a", "hex"),
  );
  expect(buffer.subarray(offset + 12, offset + 16).toString("ascii")).toBe(
    "IHDR",
  );
  return [buffer.readUInt32BE(offset + 16), buffer.readUInt32BE(offset + 20)];
}

describe("packaged application icon contract", () => {
  it("preserves the approved light and dark SVG sources exactly", () => {
    expect(sha256(lightSourcePath)).toBe(
      "96c07e5b673188259135f42f95d06e87e79a066c408e596ec2249c1333e6a5ca",
    );
    expect(sha256(darkSourcePath)).toBe(
      "aae5bd35efb76be0fba1c152e0104814eb4eb630379965584e995b35f586e187",
    );
  });

  it("configures one unambiguous light-derived desktop icon set", () => {
    expect(tauriConfiguration.bundle?.icon).toEqual([
      "icons/icon.png",
      "icons/icon.icns",
      "icons/icon.ico",
    ]);
    expect(JSON.stringify(tauriConfiguration)).not.toContain(
      "auto-video-icon-dark.svg",
    );
    expect(readdirSync(iconDirectory).sort()).toEqual([
      "auto-video-icon-dark.svg",
      "auto-video-icon-light.svg",
      "icon.icns",
      "icon.ico",
      "icon.png",
    ]);
    expect(existsSync(applicationIconPath)).toBe(true);
    expect(sha256(applicationIconPath)).toBe(
      "db9187c118c5656692f1575724c4218d7509e1aed08be0d230d33c442f794bb6",
    );
    expect(sha256(macosIconPath)).toBe(
      "43d688f6f7909bad07c733a7cd390a18873f7271e54fae37704cd9801f8f4a47",
    );
    expect(sha256(windowsIconPath)).toBe(
      "41c21df72a9487c1a78df1660a7aba771b84aa836fe3f033b7ce62ee005467b5",
    );
  });

  it("provides the generated 512 pixel application icon", () => {
    const icon = readFileSync(applicationIconPath);
    expect(readPngDimensions(icon, 0)).toEqual([512, 512]);
  });

  it("provides the complete generated Windows icon dimensions", () => {
    const icon = readFileSync(windowsIconPath);
    expect(icon.readUInt16LE(0)).toBe(0);
    expect(icon.readUInt16LE(2)).toBe(1);

    const entryCount = icon.readUInt16LE(4);
    expect(entryCount).toBe(6);
    const dimensions = Array.from({ length: entryCount }, (_, index) => {
      const entryOffset = 6 + index * 16;
      const width = icon[entryOffset] || 256;
      const height = icon[entryOffset + 1] || 256;
      const byteLength = icon.readUInt32LE(entryOffset + 8);
      const imageOffset = icon.readUInt32LE(entryOffset + 12);
      expect(imageOffset + byteLength).toBeLessThanOrEqual(icon.length);
      expect(readPngDimensions(icon, imageOffset)).toEqual([width, height]);
      return `${width}x${height}`;
    });

    expect(dimensions).toEqual([
      "32x32",
      "16x16",
      "24x24",
      "48x48",
      "64x64",
      "256x256",
    ]);
  });

  it("provides valid macOS icon chunks from 16 through 1024 pixels", () => {
    const icon = readFileSync(macosIconPath);
    expect(icon.subarray(0, 4).toString("ascii")).toBe("icns");
    expect(icon.readUInt32BE(4)).toBe(icon.length);

    const pngDimensions: string[] = [];
    const chunkTypes: string[] = [];
    let chunkOffset = 8;
    while (chunkOffset < icon.length) {
      const type = icon.subarray(chunkOffset, chunkOffset + 4).toString("ascii");
      const byteLength = icon.readUInt32BE(chunkOffset + 4);
      expect(byteLength).toBeGreaterThanOrEqual(8);
      expect(chunkOffset + byteLength).toBeLessThanOrEqual(icon.length);
      chunkTypes.push(type);
      if (icon.subarray(chunkOffset + 8, chunkOffset + 16).equals(
        Buffer.from("89504e470d0a1a0a", "hex"),
      )) {
        const [width, height] = readPngDimensions(icon, chunkOffset + 8);
        pngDimensions.push(`${width}x${height}`);
      }
      chunkOffset += byteLength;
    }

    expect(chunkOffset).toBe(icon.length);
    expect(chunkTypes).toEqual([
      "il32",
      "l8mk",
      "ic10",
      "ic11",
      "ic14",
      "ic13",
      "ic08",
      "is32",
      "s8mk",
      "ic12",
      "ic07",
      "ic09",
    ]);
    expect(new Set(pngDimensions)).toEqual(
      new Set([
        "32x32",
        "64x64",
        "128x128",
        "256x256",
        "512x512",
        "1024x1024",
      ]),
    );
  });
});
