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
  contentId: string;
  displayCode: string;
  title: string | null;
  coverAuthorityId: string | null;
  sourceAspectRatio: number;
};

export type FanzaCatalogResult =
  | { status: "ready"; items: FanzaCatalogItem[] }
  | {
      status:
        | "source-unavailable"
        | "network-error"
        | "malformed-provider"
        | "conflicting-provider"
        | "provider-error"
        | "stale";
    };

const feeds = new Set<FanzaFeed>([
  "popular",
  "newest",
  "top-rated",
  "trending",
  "monthly",
]);
const counts = new Set<FanzaResultCount>([10, 25, 50, 100]);
const u64Pattern = /^(?:0|[1-9][0-9]{0,19})$/;
const maximumU64 = 18_446_744_073_709_551_615n;
const contentIdPattern = /^[a-z0-9_]{1,64}$/;
const displayCodePattern = /^[A-Z0-9]{1,15}[A-Z]-[1-9][0-9]{0,9}$/;
const coverAuthorityPattern = /^fanza-cover-[1-9][0-9]{0,19}-[1-9][0-9]{0,2}$/;
const sourceAspectRatio = 0.72;

function validGeneration(value: string) {
  return (
    u64Pattern.test(value) &&
    BigInt(value) > 0n &&
    BigInt(value) <= maximumU64
  );
}

function validRequest(request: FanzaCatalogRequest) {
  return (
    (request.category === "adult" || request.category === "vr") &&
    feeds.has(request.feed) &&
    counts.has(request.count)
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
  if (code === `${category}_fanza_malformed_provider`) {
    return "malformed-provider";
  }
  if (code === `${category}_fanza_conflicting_provider`) {
    return "conflicting-provider";
  }
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

  const contentIds = new Set<string>();
  const coverAuthorityIds = new Set<string>();
  const items: FanzaCatalogItem[] = [];
  for (let index = 0; index < fields.length; index += 6) {
    const [category, contentId, displayCode, title, coverAuthorityId, ratio] =
      fields.slice(index, index + 6) as string[];
    const expectedCoverAuthorityId =
      `fanza-cover-${requestGeneration}-${index / 6 + 1}`;
    if (
      category !== request.category ||
      !contentIdPattern.test(contentId) ||
      contentIds.has(contentId) ||
      !displayCodePattern.test(displayCode) ||
      (coverAuthorityId !== "" &&
        (coverAuthorityId !== expectedCoverAuthorityId ||
          coverAuthorityIds.has(coverAuthorityId))) ||
      Number(ratio) !== sourceAspectRatio
    ) {
      return { status: "malformed-provider" };
    }
    contentIds.add(contentId);
    if (coverAuthorityId !== "") {
      coverAuthorityIds.add(coverAuthorityId);
    }
    items.push({
      category,
      contextGeneration,
      requestGeneration,
      contentId,
      displayCode,
      title: title === "" ? null : title,
      coverAuthorityId:
        coverAuthorityId === "" ? null : coverAuthorityId,
      sourceAspectRatio,
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
  } catch (error) {
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

function coverMimeType(bytes: Uint8Array) {
  if (bytes.length < 12) return null;
  if (bytes[0] === 0xff && bytes[1] === 0xd8 && bytes[2] === 0xff) {
    return "image/jpeg";
  }
  if (
    bytes[0] === 0x89 &&
    bytes[1] === 0x50 &&
    bytes[2] === 0x4e &&
    bytes[3] === 0x47
  ) {
    return "image/png";
  }
  if (
    bytes[0] === 0x47 &&
    bytes[1] === 0x49 &&
    bytes[2] === 0x46 &&
    bytes[3] === 0x38
  ) {
    return "image/gif";
  }
  if (
    bytes[0] === 0x52 &&
    bytes[1] === 0x49 &&
    bytes[2] === 0x46 &&
    bytes[3] === 0x46 &&
    bytes[8] === 0x57 &&
    bytes[9] === 0x45 &&
    bytes[10] === 0x42 &&
    bytes[11] === 0x50
  ) {
    return "image/webp";
  }
  return null;
}

export async function fetchFanzaCoverObjectUrl(item: FanzaCatalogItem) {
  if (
    item.coverAuthorityId === null ||
    !validGeneration(item.contextGeneration) ||
    !validGeneration(item.requestGeneration) ||
    !contentIdPattern.test(item.contentId) ||
    !displayCodePattern.test(item.displayCode) ||
    !coverAuthorityPattern.test(item.coverAuthorityId)
  ) {
    throw new Error("A current FANZA cover authority is required.");
  }
  const response = await window.__TAURI__.core.invoke<unknown>(
    "fetch_fanza_cover",
    {
      category: item.category,
      contextGeneration: item.contextGeneration,
      requestGeneration: item.requestGeneration,
      contentId: item.contentId,
      displayCode: item.displayCode,
      coverAuthorityId: item.coverAuthorityId,
    },
  );
  if (
    !Array.isArray(response) ||
    !response.every(
      (value) => Number.isInteger(value) && value >= 0 && value <= 255,
    )
  ) {
    throw new Error("FANZA returned an invalid cover.");
  }
  const bytes = Uint8Array.from(response as number[]);
  const mimeType = coverMimeType(bytes);
  if (mimeType === null) {
    throw new Error("FANZA returned an invalid cover.");
  }
  return URL.createObjectURL(new Blob([bytes], { type: mimeType }));
}
