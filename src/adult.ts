import { canonicalLibraryProductCode } from "@/vr";

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

export type AdultVolumeStorage = {
  totalBytes: bigint;
  freeBytes: bigint;
};

const unsignedU64Pattern = /^\d{1,20}$/;
const maximumU64 = 18_446_744_073_709_551_615n;
const multipartLabelPattern =
  /(^|[^A-Za-z0-9])((?:part|cd|disc|disk)[ _-]*0*([0-9]{1,4}))(?=$|[^A-Za-z0-9])/gi;

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

function parseAdultLibrary(value: unknown): AdultLibraryItem[] {
  if (
    !Array.isArray(value) ||
    value.length % 3 !== 0 ||
    !value.every((entry) => typeof entry === "string")
  ) {
    throw new Error("The native Adult scanner returned invalid data.");
  }

  const groupedItems = new Map<string, AdultLibraryItem>();
  const unassociatedItems: AdultLibraryItem[] = [];
  const paths = new Set<string>();
  const relativePaths = new Set<string>();
  for (let index = 0; index < value.length; index += 3) {
    const path = value[index] as string;
    const relativePath = value[index + 1] as string;
    const sizeBytes = value[index + 2] as string;
    const filename = exactFilename(relativePath);
    const extension = filename?.slice(filename.lastIndexOf(".") + 1) ?? "";
    if (
      path === "" ||
      filename === null ||
      paths.has(path) ||
      relativePaths.has(relativePath) ||
      !/^(?:mp4|mkv)$/i.test(extension) ||
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
    const code = canonicalLibraryProductCode(title);
    if (code === null) {
      unassociatedItems.push({
        id: `file:${path}`,
        title,
        code: null,
        files: [file],
      });
      continue;
    }
    const existingItem = groupedItems.get(code);
    if (existingItem === undefined) {
      groupedItems.set(code, {
        id: `code:${code}`,
        title: code,
        code,
        files: [file],
      });
    } else {
      existingItem.files.push(file);
    }
  }

  return [...groupedItems.values(), ...unassociatedItems];
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
