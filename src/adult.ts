import { hasSupportedLibraryExtension } from "@/library-media";
import {
  adultLibraryProductCodePrefixIsSupported,
  productCodeCandidates,
} from "@/vr";

export type AdultFolderState =
  | { status: "unconfigured" }
  | { status: "ready"; path: string }
  | { status: "unavailable"; path: string };

export type AdultLibraryFile = {
  path: string;
  relativePath: string;
  filename: string;
  title: string;
  sizeBytes: string;
  partLabel: string | null;
};

export type AdultLibraryItem = {
  id: string;
  title: string;
  code: string | null;
  files: AdultLibraryFile[];
};

export type AdultLibraryScan = {
  generation: string;
  items: AdultLibraryItem[];
};

export type AdultVolumeStorage = {
  totalBytes: bigint;
  freeBytes: bigint;
};

const unsignedU64Pattern = /^\d{1,20}$/;
const maximumU64 = 18_446_744_073_709_551_615n;
const multipartLabelPattern =
  /(^|[^A-Za-z0-9])((?:part|cd|disc|disk)[ _-]*0*([0-9]{1,4}))(?![ \t]*[-+][ \t]*[0-9])(?=$|[^A-Za-z0-9])/gi;
const multipartIdentityPrefixes = new Set(["PART", "CD", "DISC", "DISK"]);

function parseAdultFolderState(value: unknown): AdultFolderState {
  if (!Array.isArray(value) || !value.every((entry) => typeof entry === "string")) {
    throw new Error("The native Adult folder store returned invalid data.");
  }
  if (value.length === 1 && value[0] === "unconfigured") {
    return { status: "unconfigured" };
  }
  if (
    value.length === 2 &&
    (value[0] === "ready" || value[0] === "unavailable") &&
    value[1] !== ""
  ) {
    return { status: value[0], path: value[1] };
  }
  throw new Error("The native Adult folder store returned invalid data.");
}

function exactFilename(relativePath: string) {
  const components = relativePath.split(/[\\/]/);
  if (
    relativePath === "" ||
    /^[\\/]/.test(relativePath) ||
    components.some((component) => component === "" || component === "." || component === "..")
  ) {
    return null;
  }
  return components.at(-1) ?? null;
}

function filenameTitle(filename: string) {
  const extensionIndex = filename.lastIndexOf(".");
  return extensionIndex > 0 ? filename.slice(0, extensionIndex) : filename;
}

function exactMultipartLabel(title: string) {
  const matches = [...title.matchAll(multipartLabelPattern)];
  if (matches.length !== 1 || BigInt(matches[0][3]) === 0n) {
    return null;
  }
  return matches[0][2];
}

function adultLibraryProductCode(title: string) {
  const candidates = productCodeCandidates(title)
    .filter(
      (candidate) =>
        adultLibraryProductCodePrefixIsSupported(candidate.prefix) &&
        !multipartIdentityPrefixes.has(candidate.prefix),
    );
  const identities = new Set(candidates.map((candidate) => candidate.code));
  return identities.size === 1 ? candidates[0] : null;
}

function parseAdultLibrary(value: unknown): AdultLibraryScan {
  if (
    !Array.isArray(value) ||
    value.length === 0 ||
    (value.length - 1) % 3 !== 0 ||
    !value.every((entry) => typeof entry === "string")
  ) {
    throw new Error("The native Adult scanner returned invalid data.");
  }
  const generation = value[0] as string;
  if (
    !unsignedU64Pattern.test(generation) ||
    BigInt(generation) > maximumU64
  ) {
    throw new Error("The native Adult scanner returned invalid data.");
  }

  const groupedItems = new Map<string, AdultLibraryItem>();
  const unassociatedItems: AdultLibraryItem[] = [];
  const paths = new Set<string>();
  const relativePaths = new Set<string>();
  for (let index = 1; index < value.length; index += 3) {
    const path = value[index] as string;
    const relativePath = value[index + 1] as string;
    const sizeBytes = value[index + 2] as string;
    const filename = exactFilename(relativePath);
    if (
      path === "" ||
      filename === null ||
      paths.has(path) ||
      relativePaths.has(relativePath) ||
      !hasSupportedLibraryExtension(filename) ||
      !unsignedU64Pattern.test(sizeBytes) ||
      BigInt(sizeBytes) > maximumU64
    ) {
      throw new Error("The native Adult scanner returned invalid data.");
    }
    paths.add(path);
    relativePaths.add(relativePath);

    const title = filenameTitle(filename);
    const file: AdultLibraryFile = {
      path,
      relativePath,
      filename,
      title,
      sizeBytes,
      partLabel: exactMultipartLabel(title),
    };
    const productCode = adultLibraryProductCode(title);
    if (productCode === null) {
      unassociatedItems.push({
        id: `file:${path}`,
        title,
        code: null,
        files: [file],
      });
      continue;
    }
    const existingItem = groupedItems.get(productCode.code);
    if (existingItem === undefined) {
      groupedItems.set(productCode.code, {
        id: `code:${productCode.code}`,
        title: productCode.displayCode,
        code: productCode.displayCode,
        files: [file],
      });
    } else {
      existingItem.files.push(file);
    }
  }

  return {
    generation,
    items: [...groupedItems.values(), ...unassociatedItems],
  };
}

export async function loadAdultFolder() {
  return parseAdultFolderState(
    await window.__TAURI__.core.invoke<unknown>("load_adult_folder"),
  );
}

export async function chooseAdultFolder() {
  const value = await window.__TAURI__.core.invoke<unknown>("choose_adult_folder");
  if (value === null) {
    return null;
  }
  if (typeof value !== "string" || value === "") {
    throw new Error("The native Adult folder picker returned invalid data.");
  }
  return value;
}

export async function clearAdultFolder() {
  await window.__TAURI__.core.invoke("clear_adult_folder");
}

export async function scanAdultLibrary() {
  return parseAdultLibrary(
    await window.__TAURI__.core.invoke<unknown>("scan_adult_library"),
  );
}

export async function queryAdultStorage(): Promise<AdultVolumeStorage> {
  const value = await window.__TAURI__.core.invoke<unknown>("query_adult_storage");
  if (
    !Array.isArray(value) ||
    value.length !== 2 ||
    !value.every(
      (entry) =>
        typeof entry === "string" &&
        unsignedU64Pattern.test(entry) &&
        BigInt(entry) <= maximumU64,
    )
  ) {
    throw new Error("The native Adult storage query returned invalid data.");
  }
  const totalBytes = BigInt(value[0]);
  const freeBytes = BigInt(value[1]);
  if (totalBytes === 0n || freeBytes > totalBytes) {
    throw new Error("The native Adult storage query returned invalid data.");
  }
  return { totalBytes, freeBytes };
}

export async function openAdultFile(path: string) {
  if (path === "") {
    throw new Error("An Adult Library file path is required.");
  }
  await window.__TAURI__.core.invoke("open_adult_file", { path });
}

export async function revealAdultFile(path: string) {
  if (path === "") {
    throw new Error("An Adult Library file path is required.");
  }
  await window.__TAURI__.core.invoke("reveal_adult_file", { path });
}

export async function trashAdultFile(path: string, scanGeneration: string) {
  if (path === "") {
    throw new Error("An Adult Library file path is required.");
  }
  await window.__TAURI__.core.invoke("trash_adult_file", {
    path,
    scanGeneration,
  });
}
