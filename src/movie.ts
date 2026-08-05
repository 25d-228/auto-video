import {
  parseTorrentInspection,
  torrentInspectionErrorStatus,
  type TorrentInspectionResult,
} from "@/vr";

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

export function invalidateVerifiedMovieTorrent() {
  return window.__TAURI__.core.invoke<void>("invalidate_verified_movie_torrent");
}

export function invalidateMovieReleaseContext() {
  return window.__TAURI__.core.invoke<void>("invalidate_movie_release_context");
}
