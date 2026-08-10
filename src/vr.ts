export type JavdbCatalogItem = {
  code: string;
  title: string | null;
  coverUrl: string | null;
  source: "JavDB";
};

export type JavdbCatalogResult =
  | { status: "ready"; item: JavdbCatalogItem }
  | { status: "no-exact-match" }
  | { status: "source-unavailable" }
  | { status: "network-error" }
  | { status: "malformed-provider" }
  | { status: "provider-error" };

export type VrCatalogItem = JavdbCatalogItem;
export type VrCatalogResult = JavdbCatalogResult;

export type SukebeiRelease = {
  artifact?: SukebeiReleaseArtifact;
  name: string;
  source: "Sukebei";
  size: string | null;
  seeders: number | null;
};

export type VrRelease = SukebeiRelease;

export type SukebeiReleaseArtifact = {
  expectedInfohash: string;
  providerItemId: string;
  torrentUrl: string;
};

export type TorrentFile = {
  path: string;
  sizeBytes: string;
};

export type TorrentInspection = {
  displayName: string;
  files: TorrentFile[];
  infohash: string;
  inspectionId: string;
  totalBytes: string;
};

export type TorrentInspectionResult =
  | { status: "ready"; inspection: TorrentInspection }
  | { status: "source-unavailable" }
  | { status: "network-error" }
  | { status: "provider-error" }
  | { status: "malformed-torrent" }
  | { status: "unsupported-torrent" }
  | { status: "infohash-mismatch" }
  | { status: "stale-context" }
  | { status: "inspection-error" };

export type VrFolderState =
  | { status: "unconfigured" }
  | { status: "ready"; path: string }
  | { status: "unavailable"; path: string };

export type VrDownloadState =
  | "queued"
  | "downloading"
  | "paused"
  | "completed"
  | "cancelled"
  | "offline"
  | "failed"
  | "cleanup";

export type VrDownload = {
  transferId: string;
  category: "adult" | "movie" | "tv" | "unknown" | "vr";
  identity: string;
  releaseName: string;
  selectedFileCount: number;
  totalBytes: string;
  downloadedBytes: string;
  speedBytesPerSecond: string;
  state: VrDownloadState;
  isCurrentFolder: boolean;
  organizationStatus: "none" | "organized" | "attention";
  organizationRelativeDirectory: string | null;
  canOrganize: boolean;
  terminalRecovery: boolean;
  selectedFiles?: string[];
  cleanupAvailable?: boolean;
};

export type VrOrganizationPreviewEntry = {
  kind: "move" | "media-unchanged" | "non-media-unchanged";
  sourceRelativePath: string;
  destinationRelativePath: string | null;
};

export type VrOrganizationPreview = {
  planId: string;
  transferId: string;
  identity: string;
  moveCount: number;
  entries: VrOrganizationPreviewEntry[];
};

export type VrDownloadLimit = {
  mibPerSecond: string | null;
};

export type VrLibraryFile = {
  path: string;
  filename: string;
  title: string;
  sizeBytes: string;
  partLabel: string | null;
};

export type VrLibraryItem = {
  id: string;
  title: string;
  code: string | null;
  files: VrLibraryFile[];
};

export type VrLibraryScan = {
  generation: string;
  items: VrLibraryItem[];
};

export type VrVolumeStorage = {
  totalBytes: bigint;
  freeBytes: bigint;
};

export type SukebeiReleasesResult<Release extends SukebeiRelease> =
  | { status: "ready"; releases: Release[] }
  | { status: "source-unavailable" }
  | { status: "network-error" }
  | { status: "malformed-provider" }
  | { status: "provider-error" };

export type VrReleasesResult = SukebeiReleasesResult<VrRelease>;

const productCodePattern = /^([A-Za-z]{2,16})[ _-]*([0-9]{1,10})$/;
const unsignedU64Pattern = /^\d{1,20}$/;
const maximumU64 = 18_446_744_073_709_551_615n;
const maximumSelectedVrFiles = 100_000;
const maximumDownloadLimitMibPerSecond = 4095n;
const vrLibraryPartPattern =
  /(^|[^A-Za-z0-9])((?:part|pt|cd|disc|disk)[ _-]*0*([0-9]{1,4}))(?=$|[^A-Za-z0-9])/gi;
const vrLibraryPartPrefixes = new Set(["PART", "PT", "CD", "DISC", "DISK"]);
const javdbBaseUrl = "https://javdb.com";

function invokeErrorStatus(error: unknown): Exclude<
  JavdbCatalogResult["status"],
  "ready" | "no-exact-match" | "malformed-provider"
> {
  const errorCode =
    typeof error === "string"
      ? error
      : error instanceof Error
        ? error.message
        : "";

  switch (errorCode) {
    case "adult_source_unavailable":
    case "vr_source_unavailable":
      return "source-unavailable";
    case "adult_network_error":
    case "vr_network_error":
      return "network-error";
    default:
      return "provider-error";
  }
}

function normalizedText(value: string | null) {
  if (value === null) {
    return null;
  }

  const normalized = value.replace(/\s+/g, " ").trim();
  return normalized === "" ? null : normalized;
}

function javdbCoverUrl(item: Element) {
  const image = item.querySelector("img");
  const value =
    image?.getAttribute("data-src") ??
    image?.getAttribute("data-original") ??
    image?.getAttribute("src");
  if (value === undefined || value === null || value.trim() === "") {
    return null;
  }

  try {
    const url = new URL(value, javdbBaseUrl);
    return url.protocol === "https:" ? url.toString() : null;
  } catch {
    return null;
  }
}

function javdbTitle(item: Element, codeElement: Element) {
  const titleElement = item.querySelector(".video-title");
  if (titleElement === null) {
    return null;
  }

  const titleWithoutCode = titleElement.cloneNode(true) as Element;
  const clonedCodeElement = titleWithoutCode.querySelector(
    codeElement.tagName.toLowerCase(),
  );
  clonedCodeElement?.remove();
  const title = titleWithoutCode.textContent?.trim() ?? "";
  return title === "" ? null : title;
}

function parseJavdbCatalog(
  documentText: string,
  requestedCode: string,
): JavdbCatalogResult {
  const document = new DOMParser().parseFromString(documentText, "text/html");
  const movieList = document.querySelector(".movie-list");
  if (movieList === null) {
    return { status: "malformed-provider" };
  }

  for (const item of movieList.querySelectorAll(".item")) {
    const codeElement = item.querySelector(".video-title strong, strong");
    const providerCode = canonicalizeProductCode(
      codeElement?.textContent ?? "",
    );
    if (codeElement === null || providerCode !== requestedCode) {
      continue;
    }

    return {
      status: "ready",
      item: {
        code: requestedCode,
        title: javdbTitle(item, codeElement),
        coverUrl: javdbCoverUrl(item),
        source: "JavDB",
      },
    };
  }

  return { status: "no-exact-match" };
}

function directChild(element: Element, localName: string) {
  return Array.from(element.children).find(
    (candidate) => candidate.localName === localName,
  );
}

function directChildText(element: Element, localName: string) {
  const child = directChild(element, localName);
  return child === undefined ? null : normalizedText(child.textContent);
}

function directChildTrimmedText(element: Element, localName: string) {
  const value = directChild(element, localName)?.textContent;
  if (value === undefined) {
    return null;
  }
  const trimmed = value.trim();
  return trimmed === "" ? null : trimmed;
}

function sukebeiItemId(value: string, artifact: boolean) {
  try {
    const url = new URL(value);
    if (
      url.protocol !== "https:" ||
      url.hostname !== "sukebei.nyaa.si" ||
      url.port !== "" ||
      url.username !== "" ||
      url.password !== "" ||
      url.search !== "" ||
      url.hash !== "" ||
      url.href !== value
    ) {
      return null;
    }
    const match = artifact
      ? /^\/download\/([1-9]\d{0,19})\.torrent$/.exec(url.pathname)
      : /^\/view\/([1-9]\d{0,19})$/.exec(url.pathname);
    return match?.[1] ?? null;
  } catch {
    return null;
  }
}

function releaseArtifact(item: Element): SukebeiReleaseArtifact | null {
  const providerIdentity = directChildTrimmedText(item, "guid");
  const torrentUrl = directChildTrimmedText(item, "link");
  const providerInfohash = directChildTrimmedText(item, "infoHash");
  if (
    providerIdentity === null ||
    torrentUrl === null ||
    providerInfohash === null ||
    !/^[A-Fa-f0-9]{40}$/.test(providerInfohash)
  ) {
    return null;
  }

  const providerItemId = sukebeiItemId(providerIdentity, false);
  if (
    providerItemId === null ||
    sukebeiItemId(torrentUrl, true) !== providerItemId
  ) {
    return null;
  }

  return {
    expectedInfohash: providerInfohash.toLowerCase(),
    providerItemId,
    torrentUrl,
  };
}

export function productCodeCandidates(value: string) {
  const identityPattern =
    /(^|[^A-Za-z0-9])([A-Za-z]{2,16})[ _-]*([0-9]{1,10})(?=$|[^A-Za-z0-9])/gi;
  const candidates: Array<{ code: string; prefix: string }> = [];
  for (const match of value.matchAll(identityPattern)) {
    const identity = canonicalizeProductCode(`${match[2]}-${match[3]}`);
    if (identity !== null) {
      candidates.push({ code: identity, prefix: match[2].toUpperCase() });
    }
  }
  return candidates;
}

function releaseMatchesProductCode(name: string, requestedCode: string) {
  const identities = new Set(
    productCodeCandidates(name).map((candidate) => candidate.code),
  );
  return identities.size === 1 && identities.has(requestedCode);
}

function parseSukebeiReleases(
  documentText: string,
  requestedCode: string,
): VrReleasesResult {
  const document = new DOMParser().parseFromString(
    documentText,
    "application/xml",
  );
  const channel = document.querySelector("rss > channel");
  if (document.querySelector("parsererror") !== null || channel === null) {
    return { status: "malformed-provider" };
  }

  const releases: VrRelease[] = [];
  for (const item of channel.querySelectorAll(":scope > item")) {
    const name = directChild(item, "title")?.textContent ?? null;
    if (name === null || name.trim() === "") {
      return { status: "malformed-provider" };
    }
    if (!releaseMatchesProductCode(name, requestedCode)) {
      continue;
    }

    const seedersText = directChildText(item, "seeders");
    const seeders =
      seedersText !== null && /^\d+$/.test(seedersText)
        ? Number(seedersText)
        : null;
    const artifact = releaseArtifact(item);
    releases.push({
      ...(artifact === null ? {} : { artifact }),
      name,
      source: "Sukebei",
      size: directChildText(item, "size"),
      seeders:
        seeders !== null && Number.isSafeInteger(seeders) ? seeders : null,
    });
  }

  return { status: "ready", releases };
}

export function canonicalizeProductCode(value: string) {
  const match = productCodePattern.exec(value.trim());
  if (match === null) {
    return null;
  }

  const number = BigInt(match[2]);
  if (number === 0n) {
    return null;
  }

  return `${match[1].toUpperCase()}-${number}`;
}

export async function fetchExactJavdbVrItem(
  code: string,
): Promise<JavdbCatalogResult> {
  const requestedCode = canonicalizeProductCode(code);
  if (requestedCode === null || requestedCode !== code) {
    throw new Error("A canonical VR product code is required.");
  }

  try {
    const documentText = await window.__TAURI__.core.invoke<string>(
      "fetch_javdb_vr_catalog",
      { code: requestedCode },
    );
    if (typeof documentText !== "string") {
      return { status: "malformed-provider" };
    }
    return parseJavdbCatalog(documentText, requestedCode);
  } catch (error: unknown) {
    return { status: invokeErrorStatus(error) };
  }
}

export async function fetchExactJavdbAdultItem(
  code: string,
): Promise<JavdbCatalogResult> {
  const requestedCode = canonicalizeProductCode(code);
  if (requestedCode === null || requestedCode !== code) {
    throw new Error("A canonical Adult product code is required.");
  }

  try {
    const documentText = await window.__TAURI__.core.invoke<string>(
      "fetch_javdb_adult_catalog",
      { code: requestedCode },
    );
    if (typeof documentText !== "string") {
      return { status: "malformed-provider" };
    }
    return parseJavdbCatalog(documentText, requestedCode);
  } catch (error: unknown) {
    return { status: invokeErrorStatus(error) };
  }
}

export async function fetchVerifiedSukebeiReleases(
  code: string,
): Promise<VrReleasesResult> {
  const requestedCode = canonicalizeProductCode(code);
  if (requestedCode === null || requestedCode !== code) {
    throw new Error("A canonical VR product code is required.");
  }

  try {
    const documentText = await window.__TAURI__.core.invoke<string>(
      "fetch_sukebei_vr_releases",
      { code: requestedCode },
    );
    if (typeof documentText !== "string") {
      return { status: "malformed-provider" };
    }
    return parseSukebeiReleases(documentText, requestedCode);
  } catch (error: unknown) {
    return { status: invokeErrorStatus(error) };
  }
}

export async function fetchVerifiedAdultSukebeiReleases(
  code: string,
): Promise<SukebeiReleasesResult<SukebeiRelease>> {
  const requestedCode = canonicalizeProductCode(code);
  if (requestedCode === null || requestedCode !== code) {
    throw new Error("A canonical Adult product code is required.");
  }

  try {
    const documentText = await window.__TAURI__.core.invoke<string>(
      "fetch_sukebei_adult_releases",
      { code: requestedCode },
    );
    if (typeof documentText !== "string") {
      return { status: "malformed-provider" };
    }
    return parseSukebeiReleases(documentText, requestedCode);
  } catch (error: unknown) {
    return { status: invokeErrorStatus(error) };
  }
}

export function torrentInspectionErrorStatus(
  error: unknown,
  category: "adult" | "movie" | "vr",
): Exclude<TorrentInspectionResult["status"], "ready"> {
  const errorCode =
    typeof error === "string"
      ? error
      : error instanceof Error
        ? error.message
        : "";

  switch (errorCode) {
    case `${category}_torrent_source_unavailable`:
      return "source-unavailable";
    case `${category}_torrent_network_error`:
      return "network-error";
    case `${category}_torrent_provider_error`:
      return "provider-error";
    case `${category}_torrent_malformed`:
      return "malformed-torrent";
    case `${category}_torrent_unsupported`:
      return "unsupported-torrent";
    case `${category}_torrent_infohash_mismatch`:
      return "infohash-mismatch";
    case `${category}_torrent_context_invalid`:
    case `${category}_torrent_stale`:
      return "stale-context";
    default:
      return "inspection-error";
  }
}

export function parseTorrentInspection(value: unknown): TorrentInspection | null {
  if (
    !Array.isArray(value) ||
    value.length < 6 ||
    value.length % 2 !== 0 ||
    !value.every((entry) => typeof entry === "string")
  ) {
    return null;
  }
  const [inspectionId, displayName, infohash, totalBytes, ...fileValues] =
    value as string[];
  if (
    inspectionId.trim() === "" ||
    displayName.trim() === "" ||
    !/^[a-f0-9]{40}$/.test(infohash) ||
    !/^\d{1,20}$/.test(totalBytes)
  ) {
    return null;
  }

  const files: TorrentFile[] = [];
  const paths = new Set<string>();
  let summedBytes = 0n;
  for (let index = 0; index < fileValues.length; index += 2) {
    const path = fileValues[index];
    const sizeBytes = fileValues[index + 1];
    if (
      path.trim() === "" ||
      paths.has(path) ||
      !/^\d{1,20}$/.test(sizeBytes)
    ) {
      return null;
    }
    paths.add(path);
    summedBytes += BigInt(sizeBytes);
    files.push({ path, sizeBytes });
  }
  if (files.length === 0 || summedBytes !== BigInt(totalBytes)) {
    return null;
  }

  return { displayName, files, infohash, inspectionId, totalBytes };
}

export async function inspectVerifiedSukebeiTorrent(
  code: string,
  release: VrRelease,
): Promise<TorrentInspectionResult> {
  const requestedCode = canonicalizeProductCode(code);
  if (requestedCode === null || requestedCode !== code) {
    throw new Error("A canonical VR product code is required.");
  }
  if (release.artifact === undefined) {
    return { status: "malformed-torrent" };
  }

  try {
    const value = await window.__TAURI__.core.invoke<unknown>(
      "inspect_sukebei_vr_torrent",
      {
        code,
        releaseName: release.name,
        providerItemId: release.artifact.providerItemId,
        torrentUrl: release.artifact.torrentUrl,
        expectedInfohash: release.artifact.expectedInfohash,
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
    return { status: torrentInspectionErrorStatus(error, "vr") };
  }
}

export async function inspectVerifiedAdultSukebeiTorrent(
  code: string,
  release: SukebeiRelease,
): Promise<TorrentInspectionResult> {
  const requestedCode = canonicalizeProductCode(code);
  if (requestedCode === null || requestedCode !== code) {
    throw new Error("A canonical Adult product code is required.");
  }
  if (release.artifact === undefined) {
    return { status: "malformed-torrent" };
  }

  try {
    const value = await window.__TAURI__.core.invoke<unknown>(
      "inspect_sukebei_adult_torrent",
      {
        code,
        releaseName: release.name,
        providerItemId: release.artifact.providerItemId,
        torrentUrl: release.artifact.torrentUrl,
        expectedInfohash: release.artifact.expectedInfohash,
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
    return { status: torrentInspectionErrorStatus(error, "adult") };
  }
}

export async function saveVerifiedVrTorrent(inspectionId: string) {
  if (inspectionId.trim() === "") {
    throw new Error("A current torrent inspection is required.");
  }
  const saved = await window.__TAURI__.core.invoke<unknown>(
    "save_verified_vr_torrent",
    { inspectionId },
  );
  if (typeof saved !== "boolean") {
    throw new Error("The native save response was invalid.");
  }
  return saved;
}

export async function saveVerifiedAdultTorrent(inspectionId: string) {
  if (inspectionId.trim() === "") {
    throw new Error("A current Adult torrent inspection is required.");
  }
  const saved = await window.__TAURI__.core.invoke<unknown>(
    "save_verified_adult_torrent",
    { inspectionId },
  );
  if (typeof saved !== "boolean") {
    throw new Error("The native Adult save response was invalid.");
  }
  return saved;
}

export function invalidateVerifiedVrTorrent() {
  return window.__TAURI__.core.invoke<void>("invalidate_verified_vr_torrent");
}

export function invalidateVerifiedAdultTorrent() {
  return window.__TAURI__.core.invoke<void>(
    "invalidate_verified_adult_torrent",
  );
}

function parseVrFolder(value: unknown): VrFolderState {
  if (
    !Array.isArray(value) ||
    !value.every((entry) => typeof entry === "string")
  ) {
    throw new Error("The native VR folder store returned invalid data.");
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
  throw new Error("The native VR folder store returned invalid data.");
}

export async function loadVrFolder() {
  return parseVrFolder(
    await window.__TAURI__.core.invoke<unknown>("load_vr_folder"),
  );
}

export async function chooseVrFolder() {
  const path = await window.__TAURI__.core.invoke<unknown>("choose_vr_folder");
  if (path !== null && (typeof path !== "string" || path === "")) {
    throw new Error("The native VR folder picker returned an invalid path.");
  }
  return path as string | null;
}

export function clearVrFolder() {
  return window.__TAURI__.core.invoke<void>("clear_vr_folder");
}

function vrFilename(path: string) {
  const separatorIndex = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return path.slice(separatorIndex + 1);
}

function vrTitle(filename: string) {
  const extensionIndex = filename.lastIndexOf(".");
  const title = extensionIndex > 0 ? filename.slice(0, extensionIndex) : filename;
  return title === "" ? filename : title;
}

function vrPartLabel(title: string) {
  const matches = Array.from(title.matchAll(vrLibraryPartPattern));
  if (matches.length === 0) {
    return null;
  }
  const partNumbers = new Set(matches.map((match) => BigInt(match[3]).toString()));
  return partNumbers.size === 1 && !partNumbers.has("0") ? matches[0][2] : null;
}

function canonicalVrLibraryProductCode(title: string) {
  const candidates = productCodeCandidates(title)
    .filter((candidate) => !vrLibraryPartPrefixes.has(candidate.prefix))
    .map((candidate) => candidate.code);
  const uniqueCandidates = new Set(candidates);
  return uniqueCandidates.size === 1 ? candidates[0] : null;
}

function parseVrLibrary(value: unknown): VrLibraryScan {
  if (
    !Array.isArray(value) ||
    value.length === 0 ||
    (value.length - 1) % 2 !== 0 ||
    !value.every((entry) => typeof entry === "string")
  ) {
    throw new Error("The native VR Library scanner returned invalid data.");
  }
  const generation = value[0] as string;
  if (
    !unsignedU64Pattern.test(generation) ||
    BigInt(generation) > maximumU64
  ) {
    throw new Error("The native VR Library scanner returned invalid data.");
  }

  const files: VrLibraryFile[] = [];
  const paths = new Set<string>();
  for (let index = 1; index < value.length; index += 2) {
    const [path, sizeBytes] = value.slice(index, index + 2) as string[];
    if (
      path === "" ||
      paths.has(path) ||
      !unsignedU64Pattern.test(sizeBytes) ||
      BigInt(sizeBytes) > maximumU64
    ) {
      throw new Error("The native VR Library scanner returned invalid data.");
    }
    paths.add(path);
    const filename = vrFilename(path);
    if (filename === "") {
      throw new Error("The native VR Library scanner returned invalid data.");
    }
    const title = vrTitle(filename);
    files.push({
      path,
      filename,
      title,
      sizeBytes,
      partLabel: vrPartLabel(title),
    });
  }

  const groupedItems = new Map<string, VrLibraryItem>();
  const unassociatedItems: VrLibraryItem[] = [];
  for (const file of files) {
    const code = canonicalVrLibraryProductCode(file.title);
    if (code === null) {
      unassociatedItems.push({
        id: `file:${file.path}`,
        title: file.title,
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

  return {
    generation,
    items: [...groupedItems.values(), ...unassociatedItems],
  };
}

export async function scanVrLibrary() {
  return parseVrLibrary(
    await window.__TAURI__.core.invoke<unknown>("scan_vr_library"),
  );
}

export async function queryVrStorage(): Promise<VrVolumeStorage> {
  const values = await window.__TAURI__.core.invoke<unknown>("query_vr_storage");
  if (
    !Array.isArray(values) ||
    values.length !== 2 ||
    values.some(
      (value) => typeof value !== "string" || !unsignedU64Pattern.test(value),
    )
  ) {
    throw new Error("The native VR storage query returned invalid data.");
  }
  const totalBytes = BigInt(values[0]);
  const freeBytes = BigInt(values[1]);
  if (
    totalBytes === 0n ||
    totalBytes > maximumU64 ||
    freeBytes > maximumU64 ||
    freeBytes > totalBytes
  ) {
    throw new Error("The native VR storage values were inconsistent.");
  }
  return { totalBytes, freeBytes };
}

export function openVrFile(path: string) {
  if (path === "") {
    throw new Error("A VR Library file path is required.");
  }
  return window.__TAURI__.core.invoke<void>("open_vr_file", { path });
}

export function revealVrFile(path: string) {
  if (path === "") {
    throw new Error("A VR Library file path is required.");
  }
  return window.__TAURI__.core.invoke<void>("reveal_vr_file", { path });
}

export function trashVrFile(path: string, scanGeneration: string) {
  if (path === "") {
    throw new Error("A VR Library file path is required.");
  }
  return window.__TAURI__.core.invoke<void>("trash_vr_file", {
    path,
    scanGeneration,
  });
}

const vrDownloadStates = new Set<VrDownloadState>([
  "queued",
  "downloading",
  "paused",
  "completed",
  "cancelled",
  "offline",
  "failed",
  "cleanup",
]);

function decodeDownloadPaths(value: string, expectedCount: number) {
  if (value === "" && expectedCount === 0) {
    return [];
  }
  const decoder = new TextDecoder("utf-8", { fatal: true });
  const paths = value.split(",").map((encoded) => {
    if (
      encoded.length === 0 ||
      encoded.length % 2 !== 0 ||
      !/^[0-9a-f]+$/.test(encoded)
    ) {
      throw new Error("The native download store returned invalid data.");
    }
    const bytes = new Uint8Array(encoded.length / 2);
    for (let index = 0; index < bytes.length; index += 1) {
      bytes[index] = Number.parseInt(encoded.slice(index * 2, index * 2 + 2), 16);
    }
    try {
      return decoder.decode(bytes);
    } catch {
      throw new Error("The native download store returned invalid data.");
    }
  });
  if (paths.length !== expectedCount || new Set(paths).size !== paths.length) {
    throw new Error("The native download store returned invalid data.");
  }
  return paths;
}

function parseVrDownloads(value: unknown): VrDownload[] {
  const hasCleanupFields =
    Array.isArray(value) &&
    value.length % 16 === 0 &&
    value.every(
      (entry, index) =>
        typeof entry === "string" &&
        (index % 16 !== 15 || entry === "true" || entry === "false"),
    );
  const fieldCount = hasCleanupFields ? 16 : 14;
  if (
    !Array.isArray(value) ||
    value.length % fieldCount !== 0 ||
    !value.every((entry) => typeof entry === "string")
  ) {
    throw new Error("The native download store returned invalid data.");
  }

  const downloads: VrDownload[] = [];
  const transferIds = new Set<string>();
  for (let index = 0; index < value.length; index += fieldCount) {
    const [
      transferId,
      category,
      identity,
      releaseName,
      selectedFileCount,
      totalBytes,
      downloadedBytes,
      speedBytesPerSecond,
      state,
      currentFolder,
      organizationStatus,
      organizationRelativeDirectory,
      canOrganize,
      terminalRecovery,
      encodedSelectedFiles,
      cleanupAvailable,
    ] = value.slice(index, index + fieldCount) as string[];
    const count = Number(selectedFileCount);
    const selectedFiles = hasCleanupFields
      ? decodeDownloadPaths(encodedSelectedFiles, count)
      : undefined;
    const canonicalCode = canonicalizeProductCode(identity);
    const movieOrganizationPath = organizationRelativeDirectory.endsWith("/")
      ? organizationRelativeDirectory.slice(0, -1)
      : null;
    const movieOrganizationDirectory =
      movieOrganizationPath !== null &&
      !movieOrganizationPath.includes("/") &&
      safeOrganizationRelativePath(movieOrganizationPath) &&
      organizationRelativeDirectory.startsWith(`${releaseName} (`)
        ? organizationRelativeDirectory.slice(releaseName.length + 2, -2)
        : null;
    const tvIdentity = identity.match(
      /^tt\d{7,10} · S(0[1-9]|[1-9]\d*)E(0[1-9]|[1-9]\d*)$/,
    );
    const tvOrganizationPath = organizationRelativeDirectory.endsWith("/")
      ? organizationRelativeDirectory.slice(0, -1)
      : null;
    const tvOrganizationParts = tvOrganizationPath?.split("/") ?? [];
    const tvOrganizationDirectory =
      tvIdentity !== null &&
      tvOrganizationPath !== null &&
      safeOrganizationRelativePath(tvOrganizationPath) &&
      tvOrganizationParts.length === 2 &&
      tvOrganizationParts[1] === `Season ${tvIdentity[1]}`
        ? tvOrganizationPath
        : null;
    if (
      transferId === "" ||
      transferIds.has(transferId) ||
      !["adult", "movie", "tv", "unknown", "vr"].includes(category) ||
      identity.trim() === "" ||
      ((category === "adult" || category === "vr") &&
        canonicalCode !== identity) ||
      (category === "movie" &&
        (!/^tt\d{7,10}$/.test(identity) ||
          (organizationStatus !== "none" &&
            (movieOrganizationDirectory === null ||
              !/^(?!0000)\d{4}$/.test(movieOrganizationDirectory))))) ||
      (category === "tv" &&
        (tvIdentity === null ||
          (organizationStatus !== "none" &&
            tvOrganizationDirectory === null))) ||
      (category === "unknown" &&
        (count !== 0 ||
          totalBytes !== "0" ||
          downloadedBytes !== "0" ||
          speedBytesPerSecond !== "0" ||
          state !== "offline" ||
          currentFolder !== "false" ||
          organizationStatus !== "none" ||
          organizationRelativeDirectory !== "" ||
          canOrganize !== "false" ||
          terminalRecovery !== "false" ||
          (hasCleanupFields && cleanupAvailable !== "false"))) ||
      releaseName.trim() === "" ||
      !Number.isSafeInteger(count) ||
      count < 0 ||
      count > maximumSelectedVrFiles ||
      !unsignedU64Pattern.test(totalBytes) ||
      !unsignedU64Pattern.test(downloadedBytes) ||
      !unsignedU64Pattern.test(speedBytesPerSecond) ||
      BigInt(totalBytes) > maximumU64 ||
      BigInt(downloadedBytes) > maximumU64 ||
      BigInt(speedBytesPerSecond) > maximumU64 ||
      BigInt(downloadedBytes) > BigInt(totalBytes) ||
      !vrDownloadStates.has(state as VrDownloadState) ||
      (currentFolder !== "true" && currentFolder !== "false") ||
      !["none", "organized", "attention"].includes(organizationStatus) ||
      (organizationStatus === "none") !==
        (organizationRelativeDirectory === "") ||
      (organizationStatus !== "none" &&
        (state !== "completed" ||
          (category === "movie"
            ? movieOrganizationDirectory === null
            : category === "tv"
              ? tvOrganizationDirectory === null
              : organizationRelativeDirectory !== `${identity}/`))) ||
      (canOrganize !== "true" && canOrganize !== "false") ||
      (canOrganize === "true" &&
        (state !== "completed" ||
          currentFolder !== "true" ||
          organizationStatus === "organized")) ||
      (terminalRecovery !== "true" && terminalRecovery !== "false") ||
      (terminalRecovery === "true" &&
        (state !== "failed" ||
          organizationStatus !== "none" ||
          canOrganize !== "false")) ||
      (hasCleanupFields &&
        cleanupAvailable !== "true" &&
        cleanupAvailable !== "false") ||
      (cleanupAvailable === "true" &&
        (!selectedFiles || !["cancelled", "cleanup"].includes(state)))
    ) {
      throw new Error("The native download store returned invalid data.");
    }
    transferIds.add(transferId);
    downloads.push({
      transferId,
      category: category as VrDownload["category"],
      identity,
      releaseName,
      selectedFileCount: count,
      totalBytes,
      downloadedBytes,
      speedBytesPerSecond,
      state: state as VrDownloadState,
      isCurrentFolder: currentFolder === "true",
      organizationStatus: organizationStatus as VrDownload["organizationStatus"],
      organizationRelativeDirectory:
        organizationRelativeDirectory === ""
          ? null
          : organizationRelativeDirectory,
      canOrganize: canOrganize === "true",
      terminalRecovery: terminalRecovery === "true",
      ...(selectedFiles === undefined ? {} : { selectedFiles }),
      ...(hasCleanupFields
        ? { cleanupAvailable: cleanupAvailable === "true" }
        : {}),
    });
  }
  return downloads;
}

function safeOrganizationRelativePath(value: string) {
  return (
    value !== "" &&
    !value.startsWith("/") &&
    !value.includes("\\") &&
    value.split("/").every((component) => component !== "" && component !== "." && component !== "..")
  );
}

function parseVrOrganizationPreview(value: unknown): VrOrganizationPreview {
  if (
    !Array.isArray(value) ||
    value.length < 5 ||
    !value.every((entry) => typeof entry === "string")
  ) {
    throw new Error("The native VR organization preview returned invalid data.");
  }
  const [planId, transferId, identity, moveCountValue, entryCountValue] = value as string[];
  const moveCount = Number(moveCountValue);
  const entryCount = Number(entryCountValue);
  const productIdentity = canonicalizeProductCode(identity) === identity;
  const movieIdentity = /^tt\d{7,10}$/.test(identity);
  const tvIdentity = identity.match(
    /^tt\d{7,10} · S(0[1-9]|[1-9]\d*)E(0[1-9]|[1-9]\d*)$/,
  );
  if (
    planId === "" ||
    transferId === "" ||
    (!productIdentity && !movieIdentity && tvIdentity === null) ||
    !Number.isSafeInteger(moveCount) ||
    moveCount < 0 ||
    !Number.isSafeInteger(entryCount) ||
    entryCount < 1 ||
    entryCount > maximumSelectedVrFiles ||
    value.length !== 5 + entryCount * 3
  ) {
    throw new Error("The native VR organization preview returned invalid data.");
  }
  const entries: VrOrganizationPreviewEntry[] = [];
  const sources = new Set<string>();
  let destinationDirectory: string | null = null;
  let observedMoveCount = 0;
  for (let index = 5; index < value.length; index += 3) {
    const [kind, sourceRelativePath, destinationRelativePath] = value.slice(
      index,
      index + 3,
    ) as string[];
    if (
      !["move", "media-unchanged", "non-media-unchanged"].includes(kind) ||
      !safeOrganizationRelativePath(sourceRelativePath) ||
      sources.has(sourceRelativePath) ||
      (kind === "non-media-unchanged"
        ? destinationRelativePath !== ""
        : !safeOrganizationRelativePath(destinationRelativePath))
    ) {
      throw new Error("The native VR organization preview returned invalid data.");
    }
    if (kind !== "non-media-unchanged") {
      const destinationParts = destinationRelativePath.split("/");
      const directory =
        tvIdentity === null
          ? destinationParts.length === 2
            ? destinationParts[0]
            : null
          : destinationParts.length === 3 &&
              destinationParts[1] === `Season ${tvIdentity[1]}`
            ? destinationParts.slice(0, 2).join("/")
            : null;
      if (
        directory === null ||
        (productIdentity && directory !== identity) ||
        (movieIdentity && !/^.+ \((?!0000)\d{4}\)$/.test(directory)) ||
        (destinationDirectory !== null && destinationDirectory !== directory)
      ) {
        throw new Error("The native VR organization preview returned invalid data.");
      }
      destinationDirectory = directory;
    }
    sources.add(sourceRelativePath);
    if (kind === "move") {
      observedMoveCount += 1;
    }
    entries.push({
      kind: kind as VrOrganizationPreviewEntry["kind"],
      sourceRelativePath,
      destinationRelativePath:
        destinationRelativePath === "" ? null : destinationRelativePath,
    });
  }
  if (observedMoveCount !== moveCount) {
    throw new Error("The native VR organization preview returned invalid data.");
  }
  if (destinationDirectory === null) {
    throw new Error("The native VR organization preview returned invalid data.");
  }
  return { planId, transferId, identity, moveCount, entries };
}

function parseVrDownloadLimit(value: unknown): VrDownloadLimit {
  if (
    Array.isArray(value) &&
    value.length === 1 &&
    value[0] === "unlimited"
  ) {
    return { mibPerSecond: null };
  }
  if (
    Array.isArray(value) &&
    value.length === 2 &&
    value[0] === "limited" &&
    typeof value[1] === "string" &&
    /^[1-9]\d*$/.test(value[1]) &&
    BigInt(value[1]) <= maximumDownloadLimitMibPerSecond
  ) {
    return { mibPerSecond: value[1] };
  }
  throw new Error("The native download limit store returned invalid data.");
}

export async function loadVrDownloadLimit() {
  return parseVrDownloadLimit(
    await window.__TAURI__.core.invoke<unknown>("load_vr_download_limit"),
  );
}

export async function saveVrDownloadLimit(mibPerSecond: string | null) {
  if (
    mibPerSecond !== null &&
    (!/^[1-9]\d*$/.test(mibPerSecond) ||
      BigInt(mibPerSecond) > maximumDownloadLimitMibPerSecond)
  ) {
    throw new Error("A whole-number download limit from 1 to 4095 MiB/s is required.");
  }
  return parseVrDownloadLimit(
    await window.__TAURI__.core.invoke<unknown>("save_vr_download_limit", {
      mibPerSecond,
    }),
  );
}

export async function loadVrDownloads() {
  return parseVrDownloads(
    await window.__TAURI__.core.invoke<unknown>("load_vr_downloads"),
  );
}

export async function listVrDownloads() {
  return parseVrDownloads(
    await window.__TAURI__.core.invoke<unknown>("list_vr_downloads"),
  );
}

export async function startVerifiedVrDownload(
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
    throw new Error("A current inspection and valid file selection are required.");
  }
  const transferId = await window.__TAURI__.core.invoke<unknown>(
    "start_verified_vr_download",
    { inspectionId, selectedFileIds },
  );
  if (typeof transferId !== "string" || transferId === "") {
    throw new Error("The native VR download response was invalid.");
  }
  return transferId;
}

export async function startVerifiedAdultDownload(
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
    throw new Error("A current Adult inspection and valid file selection are required.");
  }
  const transferId = await window.__TAURI__.core.invoke<unknown>(
    "start_verified_adult_download",
    { inspectionId, selectedFileIds },
  );
  if (typeof transferId !== "string" || transferId === "") {
    throw new Error("The native Adult download response was invalid.");
  }
  return transferId;
}

async function runVrDownloadCommand(command: string, transferId: string) {
  if (transferId === "") {
    throw new Error("A transfer identity is required.");
  }
  await window.__TAURI__.core.invoke<void>(command, { transferId });
}

export function pauseVrDownload(transferId: string) {
  return runVrDownloadCommand("pause_vr_download", transferId);
}

export function resumeVrDownload(transferId: string) {
  return runVrDownloadCommand("resume_vr_download", transferId);
}

export function cancelVrDownload(transferId: string) {
  return runVrDownloadCommand("cancel_vr_download", transferId);
}

export async function cleanupCancelledVrDownload(transferId: string) {
  if (transferId === "") {
    throw new Error("A transfer identity is required.");
  }
  const result = await window.__TAURI__.core.invoke<unknown>(
    "cleanup_cancelled_vr_download",
    { transferId },
  );
  if (
    !Array.isArray(result) ||
    result.length !== 2 ||
    !["adult", "movie", "tv", "vr"].includes(result[0] as string) ||
    (result[1] !== "true" && result[1] !== "false")
  ) {
    throw new Error("The native transfer cleanup response was invalid.");
  }
  return {
    category: result[0] as "adult" | "movie" | "tv" | "vr",
    isCurrentFolder: result[1] === "true",
  };
}

export function dismissVrDownload(transferId: string) {
  return runVrDownloadCommand("dismiss_vr_download", transferId);
}

export async function previewVrOrganization(transferId: string) {
  if (transferId === "") {
    throw new Error("A transfer identity is required.");
  }
  return parseVrOrganizationPreview(
    await window.__TAURI__.core.invoke<unknown>("preview_vr_organization", {
      transferId,
    }),
  );
}

export function applyVrOrganization(planId: string) {
  if (planId === "") {
    throw new Error("A current organization plan is required.");
  }
  return window.__TAURI__.core.invoke<void>("apply_vr_organization", {
    planId,
  });
}

export function dismissVrOrganization() {
  return window.__TAURI__.core.invoke<void>("dismiss_vr_organization");
}
