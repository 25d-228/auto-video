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
const episodeTokenPattern =
  /(^|[^A-Za-z0-9])(?:S([1-9][0-9]*|0[1-9])E([1-9][0-9]*|0[1-9])|([1-9][0-9]*|0[1-9])x([1-9][0-9]*|0[1-9]))(?=$|[^A-Za-z0-9])/gi;
const compactEpisodeContinuationPattern =
  /^[\s._+,&\p{Pd}]+(?:E[\s._+,&\p{Pd}]*)?[0-9]+(?=$|[^A-Za-z0-9])/iu;

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

function usableShowTitle(value: string) {
  return /[\p{L}\p{N}]/u.test(value) &&
    !/^season[\s._-]*\d+$/iu.test(value) &&
    !/^S\d{1,2}$/i.test(value) &&
    [...value.matchAll(episodeTokenPattern)].length === 0;
}

function parsedEpisodeIdentity(stem: string, relativePath: string) {
  const matches = [...stem.matchAll(episodeTokenPattern)];
  if (matches.length !== 1) {
    return null;
  }
  const match = matches[0];
  const tokenEnd = (match.index ?? 0) + match[0].length;
  if (compactEpisodeContinuationPattern.test(stem.slice(tokenEnd))) {
    return null;
  }
  const seasonText = match[2] ?? match[4];
  const episodeText = match[3] ?? match[5];
  const season = Number(seasonText);
  const episode = Number(episodeText);
  if (!Number.isSafeInteger(season) || !Number.isSafeInteger(episode)) {
    return null;
  }

  const tokenStart = (match.index ?? 0) + match[1].length;
  const filenameTitle = stem
    .slice(0, tokenStart)
    .replace(/^[\s._-]+|[\s._-]+$/gu, "");
  if (filenameTitle !== "" && usableShowTitle(filenameTitle)) {
    return { episode, season, showTitle: filenameTitle };
  }

  const components = relativePath.split(/[\\/]/);
  const parentTitle = components.length > 1 ? components.at(-2) ?? "" : "";
  if (usableShowTitle(parentTitle)) {
    return { episode, season, showTitle: parentTitle };
  }
  const canonicalSeasonDirectory = `Season ${String(season).padStart(2, "0")}`;
  const grandparentTitle = components.length > 2 ? components.at(-3) ?? "" : "";
  return parentTitle === canonicalSeasonDirectory && usableShowTitle(grandparentTitle)
    ? { episode, season, showTitle: grandparentTitle }
    : null;
}

function parseTvLibrary(value: unknown): TvLibraryScan {
  if (
    !Array.isArray(value) ||
    value.length === 0 ||
    (value.length - 1) % 3 !== 0 ||
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
  for (let index = 1; index < value.length; index += 3) {
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
      throw new Error("The native TV scanner returned invalid data.");
    }
    paths.add(path);
    relativePaths.add(relativePath);

    const stem = filenameStem(filename);
    const identity = parsedEpisodeIdentity(stem, relativePath);
    const file: TvLibraryFile = {
      path,
      relativePath,
      filename,
      sizeBytes,
      season: identity?.season ?? null,
      episode: identity?.episode ?? null,
    };
    if (identity === null) {
      unassociatedItems.push({
        id: `file:${path}`,
        title: stem,
        showTitle: null,
        files: [file],
      });
      continue;
    }
    const files = groupedShows.get(identity.showTitle) ?? [];
    files.push(file);
    groupedShows.set(identity.showTitle, files);
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
