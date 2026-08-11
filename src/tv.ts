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

export type TvShowMetadataAssociation = {
  tmdbTvId: number;
  imdbId: string;
  name: string;
  originalName: string | null;
  firstAirDate: string | null;
  posterPath: string | null;
  overview: string | null;
  generation: string;
};

export type TvShowMetadataCandidate = {
  tmdbTvId: number;
  name: string;
  originalName: string | null;
  firstAirDate: string | null;
  posterPath: string | null;
};

export type TvLibraryItem = {
  id: string;
  title: string;
  showTitle: string | null;
  files: TvLibraryFile[];
  groupId?: string;
  metadataState?: "ready" | "attention";
  association?: TvShowMetadataAssociation | null;
};

export type TvLibraryScan = {
  generation: string;
  items: TvLibraryItem[];
  metadataStatus?: "ready" | "attention" | "unavailable";
};

export type TvVolumeStorage = {
  totalBytes: bigint;
  freeBytes: bigint;
};

const unsignedU64Pattern = /^\d{1,20}$/;
const maximumU64 = 18_446_744_073_709_551_615n;
const nativeIdPattern = /^[0-9a-f]{40}$/;
const imdbSeriesIdPattern = /^tt\d+$/;
const metadataDatePattern = /^\d{4}-\d{2}-\d{2}$/;

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

function parseTvMetadataAssociation(fields: string[]) {
  if (fields.length !== 8) {
    return null;
  }
  const [tmdbTvIdText, imdbId, name, originalName, firstAirDate, posterPath, overview, generation] = fields;
  const tmdbTvId = Number(tmdbTvIdText);
  if (
    !/^[1-9]\d*$/.test(tmdbTvIdText) ||
    !Number.isSafeInteger(tmdbTvId) ||
    !imdbSeriesIdPattern.test(imdbId) ||
    name.trim() === "" ||
    (originalName !== "" && originalName.trim() === "") ||
    (firstAirDate !== "" && !metadataDatePattern.test(firstAirDate)) ||
    (posterPath !== "" && !posterPath.startsWith("/")) ||
    (overview !== "" && overview.trim() === "") ||
    !unsignedU64Pattern.test(generation) ||
    generation === "0" ||
    BigInt(generation) > maximumU64
  ) {
    return null;
  }
  return {
    tmdbTvId,
    imdbId,
    name,
    originalName: originalName === "" ? null : originalName,
    firstAirDate: firstAirDate === "" ? null : firstAirDate,
    posterPath: posterPath === "" ? null : posterPath,
    overview: overview === "" ? null : overview,
    generation,
  } satisfies TvShowMetadataAssociation;
}

function sameTvMetadataAssociation(
  left: TvShowMetadataAssociation | null,
  right: TvShowMetadataAssociation | null,
) {
  return JSON.stringify(left) === JSON.stringify(right);
}

function parseTvLibrary(value: unknown): TvLibraryScan {
  if (
    !Array.isArray(value) ||
    value.length === 0 ||
    !value.every((entry) => typeof entry === "string")
  ) {
    throw new Error("The native TV scanner returned invalid data.");
  }
  const metadataResponse = value[0] === "tv-library-metadata-v1";
  const metadataStatus = metadataResponse ? value[1] : undefined;
  const generation = (metadataResponse ? value[2] : value[0]) as string;
  const rowStart = metadataResponse ? 4 : 1;
  const rowSize = metadataResponse ? 16 : 6;
  if (
    (value.length - rowStart) % rowSize !== 0 ||
    (metadataResponse &&
      (metadataStatus !== "ready" &&
        metadataStatus !== "attention" &&
        metadataStatus !== "unavailable")) ||
    (metadataResponse &&
      value[3] !== String((value.length - rowStart) / rowSize)) ||
    !unsignedU64Pattern.test(generation) ||
    BigInt(generation) > maximumU64
  ) {
    throw new Error("The native TV scanner returned invalid data.");
  }

  const groupedShows = new Map<
    string,
    {
      association: TvShowMetadataAssociation | null;
      files: TvLibraryFile[];
      groupId?: string;
      metadataState?: "ready" | "attention";
    }
  >();
  const groupIds = new Set<string>();
  const unassociatedItems: TvLibraryItem[] = [];
  const paths = new Set<string>();
  const relativePaths = new Set<string>();
  for (let index = rowStart; index < value.length; index += rowSize) {
    const path = value[index] as string;
    const relativePath = value[index + 1] as string;
    const sizeBytes = value[index + 2] as string;
    const showTitle = value[index + 3] as string;
    const seasonText = value[index + 4] as string;
    const episodeText = value[index + 5] as string;
    const groupId = metadataResponse ? (value[index + 6] as string) : "";
    const metadataState = metadataResponse ? (value[index + 7] as string) : "";
    const associationFields = metadataResponse
      ? (value.slice(index + 8, index + 16) as string[])
      : Array.from({ length: 8 }, () => "");
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
          !Number.isSafeInteger(episode))) ||
      (metadataResponse &&
        (unassociated
          ? groupId !== "" || metadataState !== "" || associationFields.some(Boolean)
          : !nativeIdPattern.test(groupId) ||
            !["", "ready", "attention"].includes(metadataState)))
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
    const association =
      metadataState === "ready"
        ? parseTvMetadataAssociation(associationFields)
        : null;
    if (
      (metadataState === "ready" && association === null) ||
      (metadataState !== "ready" && associationFields.some(Boolean))
    ) {
      throw new Error("The native TV scanner returned invalid data.");
    }
    const existing = groupedShows.get(showTitle);
    if (existing === undefined) {
      if (groupId !== "" && !groupIds.add(groupId)) {
        throw new Error("The native TV scanner returned invalid data.");
      }
      groupedShows.set(showTitle, {
        association,
        files: [file],
        ...(groupId === "" ? {} : { groupId }),
        ...(metadataState === ""
          ? {}
          : { metadataState: metadataState as "ready" | "attention" }),
      });
    } else {
      if (
        existing.groupId !== (groupId === "" ? undefined : groupId) ||
        existing.metadataState !==
          (metadataState === ""
            ? undefined
            : (metadataState as "ready" | "attention")) ||
        !sameTvMetadataAssociation(existing.association, association)
      ) {
        throw new Error("The native TV scanner returned invalid data.");
      }
      existing.files.push(file);
    }
  }

  const showItems = [...groupedShows].map(([showTitle, group]) => ({
    id: group.groupId ?? `show:${showTitle}`,
    title: group.association?.name ?? showTitle,
    showTitle,
    files: group.files.sort(
      (left, right) =>
        (left.season ?? 0) - (right.season ?? 0) ||
        (left.episode ?? 0) - (right.episode ?? 0) ||
        left.path.localeCompare(right.path),
    ),
    ...(group.groupId === undefined ? {} : { groupId: group.groupId }),
    ...(group.metadataState === undefined
      ? {}
      : { metadataState: group.metadataState }),
    ...(group.groupId === undefined ? {} : { association: group.association }),
  }));
  return {
    generation,
    items: [...showItems, ...unassociatedItems],
    ...(metadataStatus === undefined
      ? {}
      : {
          metadataStatus: metadataStatus as
            | "ready"
            | "attention"
            | "unavailable",
        }),
  };
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

function validContextGeneration(value: number) {
  return Number.isSafeInteger(value) && value > 0;
}

function parseTvMetadataSearch(value: unknown) {
  if (
    !Array.isArray(value) ||
    value.length < 2 ||
    (value.length - 2) % 5 !== 0 ||
    !value.every((entry) => typeof entry === "string") ||
    !nativeIdPattern.test(value[0]) ||
    value[1] !== String((value.length - 2) / 5)
  ) {
    throw new Error("The native TV metadata search returned invalid data.");
  }
  const candidates: TvShowMetadataCandidate[] = [];
  const ids = new Set<number>();
  for (let index = 2; index < value.length; index += 5) {
    const tmdbTvId = Number(value[index]);
    const name = value[index + 1] as string;
    const originalName = value[index + 2] as string;
    const firstAirDate = value[index + 3] as string;
    const posterPath = value[index + 4] as string;
    if (
      !/^[1-9]\d*$/.test(value[index] as string) ||
      !Number.isSafeInteger(tmdbTvId) ||
      ids.has(tmdbTvId) ||
      name.trim() === "" ||
      (originalName !== "" && originalName.trim() === "") ||
      (firstAirDate !== "" && !metadataDatePattern.test(firstAirDate)) ||
      (posterPath !== "" && !posterPath.startsWith("/"))
    ) {
      throw new Error("The native TV metadata search returned invalid data.");
    }
    ids.add(tmdbTvId);
    candidates.push({
      tmdbTvId,
      name,
      originalName: originalName === "" ? null : originalName,
      firstAirDate: firstAirDate === "" ? null : firstAirDate,
      posterPath: posterPath === "" ? null : posterPath,
    });
  }
  return { matchingRequestId: value[0] as string, candidates };
}

function parseVerifiedTvMetadata(value: unknown) {
  if (
    !Array.isArray(value) ||
    value.length !== 9 ||
    !value.every((entry) => typeof entry === "string") ||
    !nativeIdPattern.test(value[0])
  ) {
    throw new Error("The native TV metadata verification returned invalid data.");
  }
  const association = parseTvMetadataAssociation(value.slice(1));
  if (association === null) {
    throw new Error("The native TV metadata verification returned invalid data.");
  }
  return { verificationId: value[0] as string, association };
}

export async function searchTvShowMetadata(
  groupId: string,
  query: string,
  contextGeneration: number,
) {
  if (
    !nativeIdPattern.test(groupId) ||
    query.trim() === "" ||
    !validContextGeneration(contextGeneration)
  ) {
    throw new Error("A current TV show group and metadata query are required.");
  }
  return parseTvMetadataSearch(
    await window.__TAURI__.core.invoke<unknown>("search_tv_show_metadata", {
      groupId,
      query,
      contextGeneration,
    }),
  );
}

export async function verifyTvShowMetadataCandidate(
  matchingRequestId: string,
  tmdbTvId: number,
  contextGeneration: number,
) {
  if (
    !nativeIdPattern.test(matchingRequestId) ||
    !Number.isSafeInteger(tmdbTvId) ||
    tmdbTvId <= 0 ||
    !validContextGeneration(contextGeneration)
  ) {
    throw new Error("A current TV metadata request and TMDB show are required.");
  }
  return parseVerifiedTvMetadata(
    await window.__TAURI__.core.invoke<unknown>(
      "verify_tv_show_metadata_candidate",
      { matchingRequestId, tmdbTvId, contextGeneration },
    ),
  );
}

export async function saveTvShowMetadataMatch(verificationId: string) {
  if (!nativeIdPattern.test(verificationId)) {
    throw new Error("A current verified TV metadata match is required.");
  }
  const value = await window.__TAURI__.core.invoke<unknown>(
    "save_tv_show_metadata_match",
    { verificationId },
  );
  if (!Array.isArray(value) || !value.every((entry) => typeof entry === "string")) {
    throw new Error("The native TV metadata save returned invalid data.");
  }
  const association = parseTvMetadataAssociation(value);
  if (association === null) {
    throw new Error("The native TV metadata save returned invalid data.");
  }
  return association;
}

export function clearTvShowMetadataMatch(groupId: string) {
  if (!nativeIdPattern.test(groupId)) {
    throw new Error("A current TV show group is required.");
  }
  return window.__TAURI__.core.invoke<void>("clear_tv_show_metadata_match", {
    groupId,
  });
}

export function invalidateTvShowMetadataContext(contextGeneration: number) {
  if (!validContextGeneration(contextGeneration)) {
    throw new Error("A current TV metadata context generation is required.");
  }
  return window.__TAURI__.core.invoke<void>(
    "invalidate_tv_show_metadata_context",
    { contextGeneration },
  );
}
