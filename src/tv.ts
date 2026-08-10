export type TvFolderState =
  | { status: "unconfigured" }
  | { status: "ready"; path: string }
  | { status: "unavailable"; path: string };

export type TvLibraryFile = {
  path: string;
  relativePath: string;
  filename: string;
  sizeBytes: string;
  season: number | null;
  episode: number | null;
};

export type TvLibraryItem = {
  id: string;
  title: string;
  showTitle: string | null;
  files: TvLibraryFile[];
};

export type TvLibraryScan = {
  generation: string;
  items: TvLibraryItem[];
};

export type TvVolumeStorage = {
  totalBytes: bigint;
  freeBytes: bigint;
};

const unsignedU64Pattern = /^\d{1,20}$/;
const maximumU64 = 18_446_744_073_709_551_615n;

function parseTvFolderState(value: unknown): TvFolderState {
  if (!Array.isArray(value) || !value.every((entry) => typeof entry === "string")) {
    throw new Error("The native TV folder store returned invalid data.");
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
  throw new Error("The native TV folder store returned invalid data.");
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

function filenameStem(filename: string) {
  const extensionIndex = filename.lastIndexOf(".");
  return extensionIndex > 0 ? filename.slice(0, extensionIndex) : filename;
}

function parseTvLibrary(value: unknown): TvLibraryScan {
  if (
    !Array.isArray(value) ||
    value.length === 0 ||
    (value.length - 1) % 6 !== 0 ||
    !value.every((entry) => typeof entry === "string")
  ) {
    throw new Error("The native TV scanner returned invalid data.");
  }
  const generation = value[0] as string;
  if (
    !unsignedU64Pattern.test(generation) ||
    BigInt(generation) > maximumU64
  ) {
    throw new Error("The native TV scanner returned invalid data.");
  }

  const groupedShows = new Map<string, TvLibraryFile[]>();
  const unassociatedItems: TvLibraryItem[] = [];
  const paths = new Set<string>();
  const relativePaths = new Set<string>();
  for (let index = 1; index < value.length; index += 6) {
    const path = value[index] as string;
    const relativePath = value[index + 1] as string;
    const sizeBytes = value[index + 2] as string;
    const showTitle = value[index + 3] as string;
    const seasonText = value[index + 4] as string;
    const episodeText = value[index + 5] as string;
    const filename = exactFilename(relativePath);
    const extension = filename?.slice(filename.lastIndexOf(".") + 1) ?? "";
    const unassociated = showTitle === "" && seasonText === "" && episodeText === "";
    const season = Number(seasonText);
    const episode = Number(episodeText);
    if (
      path === "" ||
      filename === null ||
      paths.has(path) ||
      relativePaths.has(relativePath) ||
      !/^(?:mp4|mkv)$/i.test(extension) ||
      !unsignedU64Pattern.test(sizeBytes) ||
      BigInt(sizeBytes) > maximumU64 ||
      (!unassociated &&
        (showTitle === "" ||
          !/[\p{L}\p{N}]/u.test(showTitle) ||
          !/^[1-9]\d*$/.test(seasonText) ||
          !/^[1-9]\d*$/.test(episodeText) ||
          !Number.isSafeInteger(season) ||
          !Number.isSafeInteger(episode)))
    ) {
      throw new Error("The native TV scanner returned invalid data.");
    }
    paths.add(path);
    relativePaths.add(relativePath);

    const stem = filenameStem(filename);
    const file: TvLibraryFile = {
      path,
      relativePath,
      filename,
      sizeBytes,
      season: unassociated ? null : season,
      episode: unassociated ? null : episode,
    };
    if (unassociated) {
      unassociatedItems.push({
        id: `file:${path}`,
        title: stem,
        showTitle: null,
        files: [file],
      });
      continue;
    }
    const files = groupedShows.get(showTitle) ?? [];
    files.push(file);
    groupedShows.set(showTitle, files);
  }

  const showItems = [...groupedShows].map(([showTitle, files]) => ({
    id: `show:${showTitle}`,
    title: showTitle,
    showTitle,
    files: files.sort(
      (left, right) =>
        (left.season ?? 0) - (right.season ?? 0) ||
        (left.episode ?? 0) - (right.episode ?? 0) ||
        left.path.localeCompare(right.path),
    ),
  }));
  return { generation, items: [...showItems, ...unassociatedItems] };
}

export async function loadTvFolder() {
  return parseTvFolderState(
    await window.__TAURI__.core.invoke<unknown>("load_tv_folder"),
  );
}

export async function chooseTvFolder() {
  const value = await window.__TAURI__.core.invoke<unknown>("choose_tv_folder");
  if (value === null) {
    return null;
  }
  if (typeof value !== "string" || value === "") {
    throw new Error("The native TV folder picker returned invalid data.");
  }
  return value;
}

export async function clearTvFolder() {
  await window.__TAURI__.core.invoke("clear_tv_folder");
}

export async function scanTvLibrary() {
  return parseTvLibrary(
    await window.__TAURI__.core.invoke<unknown>("scan_tv_library"),
  );
}

export async function queryTvStorage(): Promise<TvVolumeStorage> {
  const value = await window.__TAURI__.core.invoke<unknown>("query_tv_storage");
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
    throw new Error("The native TV storage query returned invalid data.");
  }
  const totalBytes = BigInt(value[0]);
  const freeBytes = BigInt(value[1]);
  if (totalBytes === 0n || freeBytes > totalBytes) {
    throw new Error("The native TV storage query returned invalid data.");
  }
  return { totalBytes, freeBytes };
}

export async function openTvFile(path: string) {
  await window.__TAURI__.core.invoke("open_tv_file", { path });
}

export async function revealTvFile(path: string) {
  await window.__TAURI__.core.invoke("reveal_tv_file", { path });
}

export async function trashTvFile(path: string, scanGeneration: string) {
  await window.__TAURI__.core.invoke("trash_tv_file", {
    path,
    scanGeneration,
  });
}
