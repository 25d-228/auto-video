export type VrCatalogItem = {
  code: string;
  title: string | null;
  coverUrl: string | null;
  source: "JavDB";
};

export type VrCatalogResult =
  | { status: "ready"; item: VrCatalogItem }
  | { status: "no-exact-match" }
  | { status: "source-unavailable" }
  | { status: "network-error" }
  | { status: "malformed-provider" }
  | { status: "provider-error" };

export type VrRelease = {
  artifact?: VrReleaseArtifact;
  name: string;
  source: "Sukebei";
  size: string | null;
  seeders: number | null;
};

export type VrReleaseArtifact = {
  expectedInfohash: string;
  providerItemId: string;
  torrentUrl: string;
};

export type VrTorrentFile = {
  path: string;
  sizeBytes: string;
};

export type VrTorrentInspection = {
  displayName: string;
  files: VrTorrentFile[];
  infohash: string;
  inspectionId: string;
  totalBytes: string;
};

export type VrTorrentInspectionResult =
  | { status: "ready"; inspection: VrTorrentInspection }
  | { status: "source-unavailable" }
  | { status: "network-error" }
  | { status: "provider-error" }
  | { status: "malformed-torrent" }
  | { status: "unsupported-torrent" }
  | { status: "infohash-mismatch" };

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
  | "failed";

export type VrDownload = {
  transferId: string;
  code: string;
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
};

export type VrOrganizationPreviewEntry = {
  kind: "move" | "media-unchanged" | "non-media-unchanged";
  sourceRelativePath: string;
  destinationRelativePath: string | null;
};

export type VrOrganizationPreview = {
  planId: string;
  transferId: string;
  code: string;
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

export type VrReleasesResult =
  | { status: "ready"; releases: VrRelease[] }
  | { status: "source-unavailable" }
  | { status: "network-error" }
  | { status: "malformed-provider" }
  | { status: "provider-error" };

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
  VrCatalogResult["status"],
  "ready" | "no-exact-match" | "malformed-provider"
> {
  const errorCode =
    typeof error === "string"
      ? error
      : error instanceof Error
        ? error.message
        : "";

  switch (errorCode) {
    case "vr_source_unavailable":
      return "source-unavailable";
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
  return normalizedText(titleWithoutCode.textContent);
}

function parseJavdbCatalog(
  documentText: string,
  requestedCode: string,
): VrCatalogResult {
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

function releaseArtifact(item: Element): VrReleaseArtifact | null {
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
): Promise<VrCatalogResult> {
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

function torrentInspectionErrorStatus(
  error: unknown,
): Exclude<VrTorrentInspectionResult["status"], "ready"> {
  const errorCode =
    typeof error === "string"
      ? error
      : error instanceof Error
        ? error.message
        : "";

  switch (errorCode) {
    case "vr_torrent_source_unavailable":
      return "source-unavailable";
    case "vr_torrent_network_error":
      return "network-error";
    case "vr_torrent_provider_error":
      return "provider-error";
    case "vr_torrent_unsupported":
      return "unsupported-torrent";
    case "vr_torrent_infohash_mismatch":
      return "infohash-mismatch";
    default:
      return "malformed-torrent";
  }
}

function parseTorrentInspection(value: unknown): VrTorrentInspection | null {
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

  const files: VrTorrentFile[] = [];
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
): Promise<VrTorrentInspectionResult> {
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
    return { status: torrentInspectionErrorStatus(error) };
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

export function invalidateVerifiedVrTorrent() {
  return window.__TAURI__.core.invoke<void>("invalidate_verified_vr_torrent");
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

function parseVrLibrary(value: unknown): VrLibraryItem[] {
  if (
    !Array.isArray(value) ||
    value.length % 2 !== 0 ||
    !value.every((entry) => typeof entry === "string")
  ) {
    throw new Error("The native VR Library scanner returned invalid data.");
  }

  const files: VrLibraryFile[] = [];
  const paths = new Set<string>();
  for (let index = 0; index < value.length; index += 2) {
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

  return [...groupedItems.values(), ...unassociatedItems];
}

export async function scanVrLibrary() {
  return parseVrLibrary(
    await window.__TAURI__.core.invoke<unknown>("scan_vr_library"),
  );
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

const vrDownloadStates = new Set<VrDownloadState>([
  "queued",
  "downloading",
  "paused",
  "completed",
  "cancelled",
  "offline",
  "failed",
]);

function parseVrDownloads(value: unknown): VrDownload[] {
  if (
    !Array.isArray(value) ||
    value.length % 12 !== 0 ||
    !value.every((entry) => typeof entry === "string")
  ) {
    throw new Error("The native VR download store returned invalid data.");
  }

  const downloads: VrDownload[] = [];
  const transferIds = new Set<string>();
  for (let index = 0; index < value.length; index += 12) {
    const [
      transferId,
      code,
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
    ] = value.slice(index, index + 12) as string[];
    const count = Number(selectedFileCount);
    const canonicalCode = canonicalizeProductCode(code);
    if (
      transferId === "" ||
      transferIds.has(transferId) ||
      code.trim() === "" ||
      ((canOrganize === "true" || organizationStatus !== "none") &&
        canonicalCode !== code) ||
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
          organizationRelativeDirectory !== `${code}/`)) ||
      (canOrganize !== "true" && canOrganize !== "false") ||
      (canOrganize === "true" &&
        (state !== "completed" ||
          currentFolder !== "true" ||
          organizationStatus === "organized"))
    ) {
      throw new Error("The native VR download store returned invalid data.");
    }
    transferIds.add(transferId);
    downloads.push({
      transferId,
      code,
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
  const [planId, transferId, code, moveCountValue, entryCountValue] = value as string[];
  const moveCount = Number(moveCountValue);
  const entryCount = Number(entryCountValue);
  if (
    planId === "" ||
    transferId === "" ||
    canonicalizeProductCode(code) !== code ||
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
        : !safeOrganizationRelativePath(destinationRelativePath) ||
          !destinationRelativePath.startsWith(`${code}/`))
    ) {
      throw new Error("The native VR organization preview returned invalid data.");
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
  return { planId, transferId, code, moveCount, entries };
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
  throw new Error("The native VR download limit store returned invalid data.");
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
    throw new Error("A whole-number VR download limit from 1 to 4095 MiB/s is required.");
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
