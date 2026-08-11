import {
  parseTorrentInspection,
  torrentInspectionErrorStatus,
  type TorrentInspectionResult,
} from "@/vr";

export type MovieMetadataAssociation = {
  tmdbMovieId: number;
  imdbId: string;
  title: string;
  originalTitle: string | null;
  releaseDate: string | null;
  posterPath: string | null;
  overview: string | null;
  generation: string;
};

export type MovieLibraryFile = {
  fileId: string;
  path: string;
  relativePath: string;
  sizeBytes: string;
  association: MovieMetadataAssociation | null;
};

export type MovieLibraryScan = {
  metadataStatus: "ready" | "attention" | "unavailable";
  movies: MovieLibraryFile[];
};

export type MovieMetadataCandidate = {
  tmdbMovieId: number;
  title: string;
  originalTitle: string | null;
  releaseDate: string | null;
  posterPath: string | null;
};

export type MovieMetadataSearchResult = {
  matchingRequestId: string;
  candidates: MovieMetadataCandidate[];
};

export type VerifiedMovieMetadata = {
  verificationId: string;
  association: MovieMetadataAssociation;
};

const movieLibraryHeaderLength = 3;
const movieLibraryRowLength = 13;
const movieMetadataAssociationLength = 8;
const movieMetadataCandidateLength = 5;
const positiveIntegerPattern = /^[1-9]\d{0,19}$/;
const nonnegativeIntegerPattern = /^\d{1,20}$/;
const movieFileIdPattern = /^[a-f0-9]{40}$/;
const movieImdbIdPattern = /^tt\d{7,10}$/;
const movieReleaseDatePattern = /^\d{4}-\d{2}-\d{2}$/;

function optionalMovieMetadataText(value: string) {
  return value === "" ? null : value;
}

function parseMovieMetadataAssociation(
  values: string[],
): MovieMetadataAssociation | null {
  if (values.length !== movieMetadataAssociationLength) {
    return null;
  }
  const [
    tmdbMovieIdValue,
    imdbId,
    title,
    originalTitle,
    releaseDate,
    posterPath,
    overview,
    generation,
  ] = values;
  if (
    !positiveIntegerPattern.test(tmdbMovieIdValue) ||
    !movieImdbIdPattern.test(imdbId) ||
    title.trim() === "" ||
    (originalTitle !== "" && originalTitle.trim() === "") ||
    (releaseDate !== "" && !movieReleaseDatePattern.test(releaseDate)) ||
    (posterPath !== "" && !posterPath.startsWith("/")) ||
    (overview !== "" && overview.trim() === "") ||
    !positiveIntegerPattern.test(generation)
  ) {
    return null;
  }
  const tmdbMovieId = Number(tmdbMovieIdValue);
  if (!Number.isSafeInteger(tmdbMovieId)) {
    return null;
  }
  return {
    tmdbMovieId,
    imdbId,
    title,
    originalTitle: optionalMovieMetadataText(originalTitle),
    releaseDate: optionalMovieMetadataText(releaseDate),
    posterPath: optionalMovieMetadataText(posterPath),
    overview: optionalMovieMetadataText(overview),
    generation,
  };
}

export function parseMovieLibraryScan(value: unknown): MovieLibraryScan | null {
  if (
    !Array.isArray(value) ||
    value.length < movieLibraryHeaderLength ||
    !value.every((entry) => typeof entry === "string")
  ) {
    return null;
  }
  const values = value as string[];
  const [version, metadataStatus, countValue] = values;
  if (
    version !== "movie-library-v1" ||
    !["ready", "attention", "unavailable"].includes(metadataStatus) ||
    !/^\d{1,6}$/.test(countValue)
  ) {
    return null;
  }
  const count = Number(countValue);
  if (values.length !== movieLibraryHeaderLength + count * movieLibraryRowLength) {
    return null;
  }
  const movies: MovieLibraryFile[] = [];
  const fileIds = new Set<string>();
  const paths = new Set<string>();
  for (
    let index = movieLibraryHeaderLength;
    index < values.length;
    index += movieLibraryRowLength
  ) {
    const [fileId, path, relativePath, sizeBytes, associated, ...associationValues] =
      values.slice(index, index + movieLibraryRowLength);
    if (
      !movieFileIdPattern.test(fileId) ||
      fileIds.has(fileId) ||
      path === "" ||
      paths.has(path) ||
      relativePath === "" ||
      !nonnegativeIntegerPattern.test(sizeBytes) ||
      !["0", "1"].includes(associated)
    ) {
      return null;
    }
    const association =
      associated === "1"
        ? parseMovieMetadataAssociation(associationValues)
        : associationValues.every((entry) => entry === "")
          ? null
          : undefined;
    if (association === undefined || (associated === "1" && association === null)) {
      return null;
    }
    fileIds.add(fileId);
    paths.add(path);
    movies.push({ fileId, path, relativePath, sizeBytes, association });
  }
  return {
    metadataStatus: metadataStatus as MovieLibraryScan["metadataStatus"],
    movies,
  };
}

function parseMovieMetadataSearch(value: unknown): MovieMetadataSearchResult | null {
  if (
    !Array.isArray(value) ||
    value.length < 2 ||
    !value.every((entry) => typeof entry === "string")
  ) {
    return null;
  }
  const values = value as string[];
  const [matchingRequestId, countValue] = values;
  if (!movieFileIdPattern.test(matchingRequestId) || !/^\d{1,3}$/.test(countValue)) {
    return null;
  }
  const count = Number(countValue);
  if (values.length !== 2 + count * movieMetadataCandidateLength) {
    return null;
  }
  const candidates: MovieMetadataCandidate[] = [];
  const ids = new Set<number>();
  for (let index = 2; index < values.length; index += movieMetadataCandidateLength) {
    const [idValue, title, originalTitle, releaseDate, posterPath] = values.slice(
      index,
      index + movieMetadataCandidateLength,
    );
    if (
      !positiveIntegerPattern.test(idValue) ||
      title.trim() === "" ||
      (originalTitle !== "" && originalTitle.trim() === "") ||
      (releaseDate !== "" && !movieReleaseDatePattern.test(releaseDate)) ||
      (posterPath !== "" && !posterPath.startsWith("/"))
    ) {
      return null;
    }
    const tmdbMovieId = Number(idValue);
    if (!Number.isSafeInteger(tmdbMovieId) || ids.has(tmdbMovieId)) {
      return null;
    }
    ids.add(tmdbMovieId);
    candidates.push({
      tmdbMovieId,
      title,
      originalTitle: optionalMovieMetadataText(originalTitle),
      releaseDate: optionalMovieMetadataText(releaseDate),
      posterPath: optionalMovieMetadataText(posterPath),
    });
  }
  return { matchingRequestId, candidates };
}

function parseVerifiedMovieMetadata(value: unknown): VerifiedMovieMetadata | null {
  if (
    !Array.isArray(value) ||
    value.length !== 1 + movieMetadataAssociationLength ||
    !value.every((entry) => typeof entry === "string")
  ) {
    return null;
  }
  const values = value as string[];
  const association = parseMovieMetadataAssociation(values.slice(1));
  return movieFileIdPattern.test(values[0]) && association !== null
    ? { verificationId: values[0], association }
    : null;
}

export async function searchMovieMetadata(fileId: string, query: string) {
  if (!movieFileIdPattern.test(fileId) || query.trim() === "") {
    throw new Error("A current Movie file and metadata query are required.");
  }
  const result = parseMovieMetadataSearch(
    await window.__TAURI__.core.invoke<unknown>("search_movie_metadata", {
      fileId,
      query,
    }),
  );
  if (result === null) {
    throw new Error("movie_metadata_malformed_provider");
  }
  return result;
}

export async function verifyMovieMetadataCandidate(
  matchingRequestId: string,
  tmdbMovieId: number,
) {
  if (
    !movieFileIdPattern.test(matchingRequestId) ||
    !Number.isSafeInteger(tmdbMovieId) ||
    tmdbMovieId <= 0
  ) {
    throw new Error("A current metadata request and TMDB Movie are required.");
  }
  const result = parseVerifiedMovieMetadata(
    await window.__TAURI__.core.invoke<unknown>(
      "verify_movie_metadata_candidate",
      { matchingRequestId, tmdbMovieId },
    ),
  );
  if (result === null) {
    throw new Error("movie_metadata_malformed_provider");
  }
  return result;
}

export async function saveMovieMetadataMatch(verificationId: string) {
  if (!movieFileIdPattern.test(verificationId)) {
    throw new Error("A verified Movie metadata match is required.");
  }
  const value = await window.__TAURI__.core.invoke<unknown>(
    "save_movie_metadata_match",
    { verificationId },
  );
  const association =
    Array.isArray(value) && value.every((entry) => typeof entry === "string")
      ? parseMovieMetadataAssociation(value as string[])
      : null;
  if (association === null) {
    throw new Error("movie_metadata_persistence_failed");
  }
  return association;
}

export function clearMovieMetadataMatch(fileId: string) {
  if (!movieFileIdPattern.test(fileId)) {
    throw new Error("A current Movie file is required.");
  }
  return window.__TAURI__.core.invoke<void>("clear_movie_metadata_match", {
    fileId,
  });
}

export function invalidateMovieMetadataMatchContext() {
  return window.__TAURI__.core.invoke<void>(
    "invalidate_movie_metadata_match_context",
  );
}

export type MovieReleaseContext = {
  tmdbMovieId: number;
  tmdbTitle: string;
  releaseDate: string | null;
  imdbId: string;
  providerMovieId: number | null;
  providerTitle: string | null;
  providerYear: string | null;
};

export type YtsTorrentArtifact = {
  expectedInfohash: string;
  torrentUrl: string;
};

export type YtsMovieRelease = {
  artifact?: YtsTorrentArtifact;
  rowId: string;
  quality: string | null;
  typeLabel: string | null;
  videoCodec: string | null;
  size: string | null;
  sizeBytes: string | null;
  seeds: string | null;
  peers: string | null;
  source: "YTS";
};

export type MovieReleasesResult =
  | {
      status: "ready";
      context: MovieReleaseContext;
      releases: YtsMovieRelease[];
    }
  | { status: "tmdb-unauthorized" }
  | { status: "tmdb-rate-limited" }
  | { status: "tmdb-network-error" }
  | { status: "tmdb-malformed-provider" }
  | { status: "tmdb-provider-error" }
  | { status: "no-imdb-identity" }
  | { status: "yts-source-unavailable" }
  | { status: "yts-network-error" }
  | { status: "yts-malformed-provider" }
  | { status: "yts-conflicting-provider" }
  | { status: "yts-provider-error" };

const releaseHeaderLength = 8;
const releaseRowLength = 10;
const unsignedU64Pattern = /^\d{1,20}$/;
const releaseDatePattern = /^\d{4}-\d{2}-\d{2}$/;
const imdbIdPattern = /^tt\d{7,10}$/;
const infohashPattern = /^[a-f0-9]{40}$/;
const ytsDownloadPrefix = "https://yts.mx/torrent/download/";

function nullableText(value: string) {
  return value === "" ? null : value;
}

function nullableUnsigned(value: string) {
  return value === "" || unsignedU64Pattern.test(value) ? nullableText(value) : undefined;
}

function parseMovieReleases(value: unknown): MovieReleasesResult {
  if (
    !Array.isArray(value) ||
    value.length < releaseHeaderLength ||
    !value.every((entry) => typeof entry === "string")
  ) {
    return { status: "yts-malformed-provider" };
  }
  const values = value as string[];
  const [
    tmdbMovieIdValue,
    tmdbTitle,
    releaseDateValue,
    imdbId,
    providerMovieIdValue,
    providerTitleValue,
    providerYearValue,
    releaseCountValue,
  ] = values;
  if (
    !unsignedU64Pattern.test(tmdbMovieIdValue) ||
    tmdbMovieIdValue === "0" ||
    tmdbTitle.trim() === "" ||
    (releaseDateValue !== "" && !releaseDatePattern.test(releaseDateValue)) ||
    !imdbIdPattern.test(imdbId) ||
    !unsignedU64Pattern.test(providerMovieIdValue) ||
    !/^\d{1,6}$/.test(releaseCountValue)
  ) {
    return { status: "yts-malformed-provider" };
  }
  const releaseCount = Number(releaseCountValue);
  const tmdbMovieId = Number(tmdbMovieIdValue);
  if (values.length !== releaseHeaderLength + releaseCount * releaseRowLength) {
    return { status: "yts-malformed-provider" };
  }
  const providerMovieId = Number(providerMovieIdValue);
  if (
    !Number.isSafeInteger(tmdbMovieId) ||
    !Number.isSafeInteger(providerMovieId) ||
    (providerMovieId === 0 &&
      (releaseCount !== 0 || providerTitleValue !== "" || providerYearValue !== "")) ||
    (providerMovieId > 0 && providerYearValue !== "" && !unsignedU64Pattern.test(providerYearValue))
  ) {
    return { status: "yts-malformed-provider" };
  }

  const releases: YtsMovieRelease[] = [];
  const rowIds = new Set<string>();
  for (let index = releaseHeaderLength; index < values.length; index += releaseRowLength) {
    const [
      rowId,
      quality,
      typeLabel,
      videoCodec,
      size,
      sizeBytesValue,
      seedsValue,
      peersValue,
      expectedInfohash,
      torrentUrl,
    ] = values.slice(index, index + releaseRowLength);
    const sizeBytes = nullableUnsigned(sizeBytesValue);
    const seeds = nullableUnsigned(seedsValue);
    const peers = nullableUnsigned(peersValue);
    const hasArtifact = expectedInfohash !== "" || torrentUrl !== "";
    if (
      rowId.trim() === "" ||
      rowIds.has(rowId) ||
      sizeBytes === undefined ||
      seeds === undefined ||
      peers === undefined ||
      (hasArtifact &&
        (!infohashPattern.test(expectedInfohash) ||
          torrentUrl.slice(ytsDownloadPrefix.length).toLowerCase() !== expectedInfohash ||
          !torrentUrl.startsWith(ytsDownloadPrefix)))
    ) {
      return { status: "yts-malformed-provider" };
    }
    rowIds.add(rowId);
    releases.push({
      ...(hasArtifact ? { artifact: { expectedInfohash, torrentUrl } } : {}),
      rowId,
      quality: nullableText(quality),
      typeLabel: nullableText(typeLabel),
      videoCodec: nullableText(videoCodec),
      size: nullableText(size),
      sizeBytes,
      seeds,
      peers,
      source: "YTS",
    });
  }

  return {
    status: "ready",
    context: {
      tmdbMovieId,
      tmdbTitle,
      releaseDate: nullableText(releaseDateValue),
      imdbId,
      providerMovieId: providerMovieId === 0 ? null : providerMovieId,
      providerTitle: nullableText(providerTitleValue),
      providerYear: nullableText(providerYearValue),
    },
    releases,
  };
}

function movieReleaseErrorStatus(error: unknown): Exclude<MovieReleasesResult["status"], "ready"> {
  const errorCode =
    typeof error === "string"
      ? error
      : error instanceof Error
        ? error.message
        : "";
  switch (errorCode) {
    case "movie_tmdb_unauthorized":
      return "tmdb-unauthorized";
    case "movie_tmdb_rate_limited":
      return "tmdb-rate-limited";
    case "movie_tmdb_network_error":
      return "tmdb-network-error";
    case "movie_tmdb_malformed":
      return "tmdb-malformed-provider";
    case "movie_no_imdb_identity":
      return "no-imdb-identity";
    case "movie_yts_source_unavailable":
      return "yts-source-unavailable";
    case "movie_yts_network_error":
      return "yts-network-error";
    case "movie_yts_malformed":
      return "yts-malformed-provider";
    case "movie_yts_conflicting_provider":
      return "yts-conflicting-provider";
    case "movie_yts_provider_error":
      return "yts-provider-error";
    default:
      return "tmdb-provider-error";
  }
}

export async function fetchVerifiedYtsMovieReleases(
  tmdbMovieId: number,
): Promise<MovieReleasesResult> {
  if (!Number.isSafeInteger(tmdbMovieId) || tmdbMovieId <= 0) {
    throw new Error("A positive TMDB Movie ID is required.");
  }
  try {
    const value = await window.__TAURI__.core.invoke<unknown>(
      "fetch_yts_movie_releases",
      { tmdbMovieId },
    );
    return parseMovieReleases(value);
  } catch (error: unknown) {
    return { status: movieReleaseErrorStatus(error) };
  }
}

export async function inspectVerifiedYtsMovieTorrent(
  context: MovieReleaseContext,
  release: YtsMovieRelease,
): Promise<TorrentInspectionResult> {
  if (context.providerMovieId === null || release.artifact === undefined) {
    return { status: "malformed-torrent" };
  }
  try {
    const value = await window.__TAURI__.core.invoke<unknown>(
      "inspect_yts_movie_torrent",
      {
        tmdbMovieId: context.tmdbMovieId,
        tmdbTitle: context.tmdbTitle,
        releaseDate: context.releaseDate,
        imdbId: context.imdbId,
        providerMovieId: context.providerMovieId,
        providerTitle: context.providerTitle,
        providerYear: context.providerYear,
        rowId: release.rowId,
        quality: release.quality,
        typeLabel: release.typeLabel,
        videoCodec: release.videoCodec,
        size: release.size,
        sizeBytes: release.sizeBytes,
        seeds: release.seeds,
        peers: release.peers,
        expectedInfohash: release.artifact.expectedInfohash,
        torrentUrl: release.artifact.torrentUrl,
      },
    );
    const inspection = parseTorrentInspection(value);
    if (inspection === null) {
      return { status: "malformed-torrent" };
    }
    return inspection.infohash === release.artifact.expectedInfohash
      ? { status: "ready", inspection }
      : { status: "infohash-mismatch" };
  } catch (error: unknown) {
    return { status: torrentInspectionErrorStatus(error, "movie") };
  }
}

export async function saveVerifiedMovieTorrent(inspectionId: string) {
  if (inspectionId.trim() === "") {
    throw new Error("A current Movie torrent inspection is required.");
  }
  const saved = await window.__TAURI__.core.invoke<unknown>(
    "save_verified_movie_torrent",
    { inspectionId },
  );
  if (typeof saved !== "boolean") {
    throw new Error("The native Movie save response was invalid.");
  }
  return saved;
}

export async function startVerifiedMovieDownload(
  inspectionId: string,
  selectedFileIds: number[],
) {
  const uniqueIds = new Set(selectedFileIds);
  if (
    inspectionId.trim() === "" ||
    selectedFileIds.length === 0 ||
    uniqueIds.size !== selectedFileIds.length ||
    selectedFileIds.some(
      (fileId) => !Number.isSafeInteger(fileId) || fileId < 0,
    )
  ) {
    throw new Error(
      "A current Movie inspection and valid file selection are required.",
    );
  }
  const transferId = await window.__TAURI__.core.invoke<unknown>(
    "start_verified_movie_download",
    { inspectionId, selectedFileIds },
  );
  if (typeof transferId !== "string" || transferId === "") {
    throw new Error("The native Movie download response was invalid.");
  }
  return transferId;
}

export function invalidateVerifiedMovieTorrent() {
  return window.__TAURI__.core.invoke<void>("invalidate_verified_movie_torrent");
}

export function invalidateMovieReleaseContext() {
  return window.__TAURI__.core.invoke<void>("invalidate_movie_release_context");
}
