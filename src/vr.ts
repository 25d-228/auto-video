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
  name: string;
  source: "Sukebei";
  size: string | null;
  seeders: number | null;
};

export type VrReleasesResult =
  | { status: "ready"; releases: VrRelease[] }
  | { status: "source-unavailable" }
  | { status: "network-error" }
  | { status: "malformed-provider" }
  | { status: "provider-error" };

const productCodePattern = /^([A-Za-z]{2,16})[ _-]*([0-9]{1,10})$/;
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

function releaseMatchesProductCode(name: string, requestedCode: string) {
  const identityPattern =
    /(^|[^A-Za-z0-9])([A-Za-z]{2,16})[ _-]*([0-9]{1,10})(?=$|[^A-Za-z0-9])/gi;
  const identities = new Set<string>();
  for (const match of name.matchAll(identityPattern)) {
    const identity = canonicalizeProductCode(`${match[2]}-${match[3]}`);
    if (identity !== null) {
      identities.add(identity);
    }
  }

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
    releases.push({
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
