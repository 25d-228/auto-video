import {
  parseTorrentInspection,
  type TorrentInspection,
} from "@/vr";

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

export type TvTorrentInspectionResult =
  | { status: "ready"; inspection: TorrentInspection }
  | { status: "source-unavailable" }
  | { status: "network-error" }
  | { status: "inspection-unavailable" }
  | { status: "timeout" }
  | { status: "no-peers" }
  | { status: "malformed-torrent" }
  | { status: "unsupported-torrent" }
  | { status: "infohash-mismatch" }
  | { status: "stale-context" }
  | { status: "inspection-error" };

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

function tvTorrentErrorStatus(
  error: unknown,
): Exclude<TvTorrentInspectionResult["status"], "ready"> {
  const errorCode =
    typeof error === "string"
      ? error
      : error instanceof Error
        ? error.message
        : "";
  switch (errorCode) {
    case "tv_torrent_source_unavailable":
      return "source-unavailable";
    case "tv_torrent_network_error":
      return "network-error";
    case "tv_torrent_inspection_unavailable":
      return "inspection-unavailable";
    case "tv_torrent_timeout":
      return "timeout";
    case "tv_torrent_no_peers":
      return "no-peers";
    case "tv_torrent_malformed":
      return "malformed-torrent";
    case "tv_torrent_unsupported":
      return "unsupported-torrent";
    case "tv_torrent_infohash_mismatch":
      return "infohash-mismatch";
    case "tv_torrent_context_invalid":
    case "tv_torrent_stale":
      return "stale-context";
    default:
      return "inspection-error";
  }
}

export async function inspectVerifiedApiBayTvTorrent(
  context: TvEpisodeReleaseContext,
  release: ApiBayTvRelease,
): Promise<TvTorrentInspectionResult> {
  try {
    const value = await window.__TAURI__.core.invoke<unknown>(
      "inspect_apibay_tv_torrent",
      {
        tmdbTvId: context.tmdbTvId,
        showName: context.showName,
        providerSeasonId: context.providerSeasonId,
        seasonNumber: context.seasonNumber,
        providerEpisodeId: context.providerEpisodeId,
        episodeNumber: context.episodeNumber,
        episodeName: context.episodeName,
        imdbId: context.imdbId,
        providerItemId: release.providerItemId,
        providerCategory: release.category,
        releaseName: release.name,
        expectedInfohash: release.infohash,
      },
    );
    const inspection = parseTorrentInspection(value);
    if (inspection === null) {
      return { status: "malformed-torrent" };
    }
    return inspection.infohash === release.infohash
      ? { status: "ready", inspection }
      : { status: "infohash-mismatch" };
  } catch (error: unknown) {
    return { status: tvTorrentErrorStatus(error) };
  }
}

export async function saveVerifiedTvTorrent(inspectionId: string) {
  if (inspectionId.trim() === "") {
    throw new Error("A current TV torrent inspection is required.");
  }
  const saved = await window.__TAURI__.core.invoke<unknown>(
    "save_verified_tv_torrent",
    { inspectionId },
  );
  if (typeof saved !== "boolean") {
    throw new Error("The native TV save response was invalid.");
  }
  return saved;
}

export async function startVerifiedTvDownload(
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
    throw new Error("A current TV inspection and valid file selection are required.");
  }
  const transferId = await window.__TAURI__.core.invoke<unknown>(
    "start_verified_tv_download",
    { inspectionId, selectedFileIds },
  );
  if (typeof transferId !== "string" || transferId === "") {
    throw new Error("The native TV download response was invalid.");
  }
  return transferId;
}

export function invalidateVerifiedTvTorrent() {
  return window.__TAURI__.core.invoke<void>("invalidate_verified_tv_torrent");
}

export function invalidateTvReleaseContext() {
  return window.__TAURI__.core.invoke<void>("invalidate_tv_release_context");
}
