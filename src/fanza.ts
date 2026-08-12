import { canonicalizeProductCode } from "@/vr";

export type FanzaCategory = "adult" | "vr";
export type FanzaFeed =
  | "popular"
  | "newest"
  | "top-rated"
  | "trending"
  | "monthly";
export type FanzaResultCount = 10 | 25 | 50 | 100;

export type FanzaCatalogRequest = {
  category: FanzaCategory;
  feed: FanzaFeed;
  count: FanzaResultCount;
};

export type FanzaCatalogItem = {
  category: FanzaCategory;
  contextGeneration: string;
  requestGeneration: string;
  providerItemId: string;
  code: string;
  title: string | null;
  coverUrl: null;
  coverAuthorityId: string | null;
  sourceAspectRatio: number;
  source: "FANZA";
};

export type FanzaCatalogResult =
  | { status: "ready"; items: FanzaCatalogItem[] }
  | { status: "source-unavailable" }
  | { status: "network-error" }
  | { status: "malformed-provider" }
  | { status: "conflicting-provider" }
  | { status: "provider-error" }
  | { status: "stale" };

const generationPattern = /^[1-9][0-9]{0,19}$/;
const maximumU64 = 18_446_744_073_709_551_615n;
const providerItemPattern = /^[a-z0-9_]{1,64}$/;
const coverAuthorityPattern = /^fanza-cover-[1-9][0-9]*-[1-9][0-9]*$/;
const feeds = new Set<FanzaFeed>([
  "popular",
  "newest",
  "top-rated",
  "trending",
  "monthly",
]);
const resultCounts = new Set<FanzaResultCount>([10, 25, 50, 100]);

function validGeneration(value: string) {
  return generationPattern.test(value) && BigInt(value) <= maximumU64;
}

function validRequest(request: FanzaCatalogRequest) {
  return (
    (request.category === "adult" || request.category === "vr") &&
    feeds.has(request.feed) &&
    resultCounts.has(request.count)
  );
}

function errorStatus(
  category: FanzaCategory,
  error: unknown,
): Exclude<FanzaCatalogResult["status"], "ready"> {
  const code =
    typeof error === "string"
      ? error
      : error instanceof Error
        ? error.message
        : "";
  if (code === `${category}_source_unavailable`) return "source-unavailable";
  if (code === `${category}_network_error`) return "network-error";
  if (code === `${category}_fanza_malformed_provider`)
    return "malformed-provider";
  if (code === `${category}_fanza_conflicting_provider`)
    return "conflicting-provider";
  if (code === `${category}_fanza_stale`) return "stale";
  return "provider-error";
}

export function parseFanzaCatalogResponse(
  value: unknown,
  request: FanzaCatalogRequest,
  contextGeneration: string,
): FanzaCatalogResult {
  if (
    !validRequest(request) ||
    !validGeneration(contextGeneration) ||
    !Array.isArray(value) ||
    value.length < 2 ||
    !value.every((entry) => typeof entry === "string")
  ) {
    return { status: "malformed-provider" };
  }
  const [requestGeneration, itemCountText, ...fields] = value as string[];
  const itemCount = Number(itemCountText);
  if (
    !validGeneration(requestGeneration) ||
    !/^\d{1,3}$/.test(itemCountText) ||
    !Number.isSafeInteger(itemCount) ||
    itemCount > request.count ||
    fields.length !== itemCount * 6
  ) {
    return { status: "malformed-provider" };
  }

  const items: FanzaCatalogItem[] = [];
  const providerItemIds = new Set<string>();
  for (let index = 0; index < fields.length; index += 6) {
    const [
      category,
      providerItemId,
      code,
      title,
      coverAuthorityId,
      ratioText,
    ] = fields.slice(index, index + 6) as string[];
    const sourceAspectRatio = Number(ratioText);
    if (
      category !== request.category ||
      !providerItemPattern.test(providerItemId) ||
      providerItemIds.has(providerItemId) ||
      canonicalizeProductCode(code) !== code ||
      (coverAuthorityId !== "" &&
        (!coverAuthorityPattern.test(coverAuthorityId) ||
          !coverAuthorityId.startsWith(
            `fanza-cover-${requestGeneration}-`,
          ))) ||
      !Number.isFinite(sourceAspectRatio) ||
      sourceAspectRatio <= 0 ||
      sourceAspectRatio > 4
    ) {
      return { status: "malformed-provider" };
    }
    providerItemIds.add(providerItemId);
    items.push({
      category: request.category,
      contextGeneration,
      requestGeneration,
      providerItemId,
      code,
      title: title === "" ? null : title,
      coverUrl: null,
      coverAuthorityId: coverAuthorityId === "" ? null : coverAuthorityId,
      sourceAspectRatio,
      source: "FANZA",
    });
  }
  return { status: "ready", items };
}

export async function fetchFanzaCatalog(
  request: FanzaCatalogRequest,
  contextGeneration: string,
): Promise<FanzaCatalogResult> {
  if (!validRequest(request) || !validGeneration(contextGeneration)) {
    throw new Error("A valid FANZA catalog request is required.");
  }
  try {
    const response = await window.__TAURI__.core.invoke<unknown>(
      "fetch_fanza_catalog",
      { ...request, contextGeneration },
    );
    return parseFanzaCatalogResponse(response, request, contextGeneration);
  } catch (error: unknown) {
    return { status: errorStatus(request.category, error) };
  }
}

export async function invalidateFanzaCatalog(
  category: FanzaCategory,
  contextGeneration: string,
) {
  if (!validGeneration(contextGeneration)) {
    throw new Error("A valid FANZA catalog context is required.");
  }
  await window.__TAURI__.core.invoke("invalidate_fanza_catalog", {
    category,
    contextGeneration,
  });
}

function rasterMimeType(bytes: Uint8Array) {
  if (bytes.length < 12) return null;
  if (bytes[0] === 0xff && bytes[1] === 0xd8 && bytes[2] === 0xff)
    return "image/jpeg";
  if (
    bytes[0] === 0x89 &&
    bytes[1] === 0x50 &&
    bytes[2] === 0x4e &&
    bytes[3] === 0x47
  )
    return "image/png";
  if (
    bytes[0] === 0x47 &&
    bytes[1] === 0x49 &&
    bytes[2] === 0x46 &&
    bytes[3] === 0x38
  )
    return "image/gif";
  if (
    bytes[0] === 0x52 &&
    bytes[1] === 0x49 &&
    bytes[2] === 0x46 &&
    bytes[3] === 0x46 &&
    bytes[8] === 0x57 &&
    bytes[9] === 0x45 &&
    bytes[10] === 0x42 &&
    bytes[11] === 0x50
  )
    return "image/webp";
  return null;
}

export async function fetchFanzaCoverObjectUrl(item: FanzaCatalogItem) {
  if (
    item.coverAuthorityId === null ||
    !coverAuthorityPattern.test(item.coverAuthorityId) ||
    !providerItemPattern.test(item.providerItemId) ||
    !validGeneration(item.contextGeneration) ||
    !validGeneration(item.requestGeneration) ||
    canonicalizeProductCode(item.code) !== item.code
  ) {
    throw new Error("A current FANZA cover authority is required.");
  }
  const value = await window.__TAURI__.core.invoke<unknown>("fetch_fanza_cover", {
    category: item.category,
    contextGeneration: item.contextGeneration,
    requestGeneration: item.requestGeneration,
    providerItemId: item.providerItemId,
    code: item.code,
    coverAuthorityId: item.coverAuthorityId,
  });
  if (
    !Array.isArray(value) ||
    value.length === 0 ||
    value.length > 16 * 1024 * 1024 ||
    !value.every(
      (byte) => Number.isInteger(byte) && byte >= 0 && byte <= 255,
    )
  ) {
    throw new Error("The native FANZA cover response was invalid.");
  }
  const bytes = Uint8Array.from(value as number[]);
  const mimeType = rasterMimeType(bytes);
  if (mimeType === null) {
    throw new Error("The native FANZA cover response was invalid.");
  }
  return URL.createObjectURL(new Blob([bytes], { type: mimeType }));
}
