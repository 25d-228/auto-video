export type TvEpisodeReleaseContext = {
  tmdbTvId: number;
  showName: string;
  providerSeasonId: number;
  seasonNumber: number;
  providerEpisodeId: number;
  episodeNumber: number;
  episodeName: string;
  imdbId: string;
};

export type ApiBayTvRelease = {
  providerItemId: string;
  name: string;
  category: "205" | "208";
  sizeBytes: string | null;
  seeders: string | null;
  leechers: string | null;
  uploader: string | null;
  providerStatus: string | null;
  added: string | null;
  infohash: string;
  source: "API Bay";
};

export type TvEpisodeReleasesResult =
  | {
      status: "ready";
      context: TvEpisodeReleaseContext;
      releases: ApiBayTvRelease[];
    }
  | { status: "tmdb-unauthorized" }
  | { status: "tmdb-rate-limited" }
  | { status: "tmdb-network-error" }
  | { status: "tmdb-malformed-provider" }
  | { status: "tmdb-provider-error" }
  | { status: "no-imdb-identity" }
  | { status: "apibay-source-unavailable" }
  | { status: "apibay-network-error" }
  | { status: "apibay-malformed-provider" }
  | { status: "apibay-conflicting-provider" }
  | { status: "apibay-provider-error" };

const headerLength = 9;
const rowLength = 11;
const positiveIntegerPattern = /^[1-9]\d{0,19}$/;
const unsignedIntegerPattern = /^\d{1,20}$/;
const imdbIdPattern = /^tt\d{7,10}$/;
const infohashPattern = /^[a-f0-9]{40}$/;

function positiveSafeInteger(value: string) {
  if (!positiveIntegerPattern.test(value)) {
    return null;
  }
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) ? parsed : null;
}

function nullableUnsigned(value: string) {
  return value === "" || unsignedIntegerPattern.test(value)
    ? value === ""
      ? null
      : value
    : undefined;
}

function nullableText(value: string) {
  return value === "" ? null : value;
}

function parseTvEpisodeReleases(value: unknown): TvEpisodeReleasesResult {
  if (
    !Array.isArray(value) ||
    value.length < headerLength ||
    !value.every((entry) => typeof entry === "string")
  ) {
    return { status: "apibay-malformed-provider" };
  }
  const values = value as string[];
  const [
    tmdbTvIdValue,
    showName,
    providerSeasonIdValue,
    seasonNumberValue,
    providerEpisodeIdValue,
    episodeNumberValue,
    episodeName,
    imdbId,
    releaseCountValue,
  ] = values;
  const tmdbTvId = positiveSafeInteger(tmdbTvIdValue);
  const providerSeasonId = positiveSafeInteger(providerSeasonIdValue);
  const seasonNumber = positiveSafeInteger(seasonNumberValue);
  const providerEpisodeId = positiveSafeInteger(providerEpisodeIdValue);
  const episodeNumber = positiveSafeInteger(episodeNumberValue);
  if (
    tmdbTvId === null ||
    providerSeasonId === null ||
    seasonNumber === null ||
    providerEpisodeId === null ||
    episodeNumber === null ||
    showName.trim() === "" ||
    episodeName.trim() === "" ||
    !imdbIdPattern.test(imdbId) ||
    !/^\d{1,3}$/.test(releaseCountValue)
  ) {
    return { status: "apibay-malformed-provider" };
  }
  const releaseCount = Number(releaseCountValue);
  if (values.length !== headerLength + releaseCount * rowLength) {
    return { status: "apibay-malformed-provider" };
  }

  const providerItemIds = new Set<string>();
  const infohashes = new Set<string>();
  const releases: ApiBayTvRelease[] = [];
  for (let index = headerLength; index < values.length; index += rowLength) {
    const [
      providerItemId,
      name,
      category,
      sizeBytesValue,
      seedersValue,
      leechersValue,
      uploader,
      providerStatus,
      addedValue,
      infohash,
      source,
    ] = values.slice(index, index + rowLength);
    const sizeBytes = nullableUnsigned(sizeBytesValue);
    const seeders = nullableUnsigned(seedersValue);
    const leechers = nullableUnsigned(leechersValue);
    const added = nullableUnsigned(addedValue);
    if (
      !positiveIntegerPattern.test(providerItemId) ||
      providerItemIds.has(providerItemId) ||
      name.trim() === "" ||
      (category !== "205" && category !== "208") ||
      sizeBytes === undefined ||
      seeders === undefined ||
      leechers === undefined ||
      added === undefined ||
      !infohashPattern.test(infohash) ||
      infohashes.has(infohash) ||
      source !== "API Bay"
    ) {
      return { status: "apibay-malformed-provider" };
    }
    providerItemIds.add(providerItemId);
    infohashes.add(infohash);
    releases.push({
      providerItemId,
      name,
      category,
      sizeBytes,
      seeders,
      leechers,
      uploader: nullableText(uploader),
      providerStatus: nullableText(providerStatus),
      added,
      infohash,
      source,
    });
  }

  return {
    status: "ready",
    context: {
      tmdbTvId,
      showName,
      providerSeasonId,
      seasonNumber,
      providerEpisodeId,
      episodeNumber,
      episodeName,
      imdbId,
    },
    releases,
  };
}

function tvReleaseErrorStatus(
  error: unknown,
): Exclude<TvEpisodeReleasesResult["status"], "ready"> {
  const errorCode =
    typeof error === "string"
      ? error
      : error instanceof Error
        ? error.message
        : "";
  switch (errorCode) {
    case "tv_release_tmdb_unauthorized":
      return "tmdb-unauthorized";
    case "tv_release_tmdb_rate_limited":
      return "tmdb-rate-limited";
    case "tv_release_tmdb_network_error":
      return "tmdb-network-error";
    case "tv_release_tmdb_malformed":
      return "tmdb-malformed-provider";
    case "tv_release_no_imdb_identity":
      return "no-imdb-identity";
    case "tv_release_apibay_source_unavailable":
      return "apibay-source-unavailable";
    case "tv_release_apibay_network_error":
      return "apibay-network-error";
    case "tv_release_apibay_malformed":
      return "apibay-malformed-provider";
    case "tv_release_apibay_conflicting":
      return "apibay-conflicting-provider";
    case "tv_release_apibay_provider_error":
      return "apibay-provider-error";
    default:
      return "tmdb-provider-error";
  }
}

export async function fetchVerifiedApiBayTvReleases(
  tmdbTvId: number,
  providerSeasonId: number,
  providerEpisodeId: number,
): Promise<TvEpisodeReleasesResult> {
  if (
    !Number.isSafeInteger(tmdbTvId) ||
    tmdbTvId <= 0 ||
    !Number.isSafeInteger(providerSeasonId) ||
    providerSeasonId <= 0 ||
    !Number.isSafeInteger(providerEpisodeId) ||
    providerEpisodeId <= 0
  ) {
    throw new Error("Positive provider TV, season, and episode IDs are required.");
  }
  try {
    const value = await window.__TAURI__.core.invoke<unknown>(
      "fetch_apibay_tv_releases",
      { tmdbTvId, providerSeasonId, providerEpisodeId },
    );
    return parseTvEpisodeReleases(value);
  } catch (error: unknown) {
    return { status: tvReleaseErrorStatus(error) };
  }
}
