import { canonicalizeProductCode } from "@/vr";

export type FanzaCategory = "adult" | "vr";
export type FanzaFeed =
  | "popular"
  | "newest"
  | "top-rated"
  | "trending"
  | "monthly";
export type FanzaCount = 10 | 25 | 50 | 100;

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

export type FanzaDetailResult =
  | { status: "ready"; item: FanzaCatalogItem; detailGeneration: string }
  | Exclude<FanzaCatalogResult, { status: "ready" }>;

export type FanzaPreviewResult =
  | { status: "ready"; previewGeneration: string; authorityIds: string[] }
  | Exclude<FanzaCatalogResult, { status: "ready" }>;

const unsignedU64Pattern = /^\d{1,20}$/;
const maximumU64 = 18_446_744_073_709_551_615n;
const providerItemPattern = /^[a-z0-9_]{1,64}$/;
const coverAuthorityPattern = /^fanza-cover-[1-9][0-9]*-[1-9][0-9]*$/;
const previewAuthorityPattern = /^fanza-preview-[1-9][0-9]*-[1-9][0-9]*$/;
const feeds = new Set<FanzaFeed>([
  "popular",
  "newest",
  "top-rated",
  "trending",
  "monthly",
]);
const counts = new Set<FanzaCount>([10, 25, 50, 100]);

function validGeneration(value: string) {
  return (
    unsignedU64Pattern.test(value) &&
    BigInt(value) > 0n &&
    BigInt(value) <= maximumU64
  );
}

function validItem(item: FanzaCatalogItem) {
  return (
    (item.category === "adult" || item.category === "vr") &&
    validGeneration(item.contextGeneration) &&
    validGeneration(item.requestGeneration) &&
    providerItemPattern.test(item.providerItemId) &&
    canonicalizeProductCode(item.code) === item.code
  );
}

function itemAuthority(item: FanzaCatalogItem) {
  return {
    category: item.category,
    contextGeneration: item.contextGeneration,
    requestGeneration: item.requestGeneration,
    providerItemId: item.providerItemId,
    code: item.code,
  };
}

export function fanzaErrorStatus(
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
  category: FanzaCategory,
  contextGeneration: string,
  requestedCount: FanzaCount,
): FanzaCatalogResult {
  if (
    !validGeneration(contextGeneration) ||
    !counts.has(requestedCount) ||
    !Array.isArray(value) ||
    value.length < 2 ||
    !value.every((entry) => typeof entry === "string")
  ) {
    return { status: "malformed-provider" };
  }
  const [requestGeneration, countText, ...fields] = value as string[];
  const count = Number(countText);
  if (
    !validGeneration(requestGeneration) ||
    !/^\d{1,3}$/.test(countText) ||
    !Number.isSafeInteger(count) ||
    count > requestedCount ||
    fields.length !== count * 6
  ) {
    return { status: "malformed-provider" };
  }
  const items: FanzaCatalogItem[] = [];
  const providerItems = new Set<string>();
  for (let index = 0; index < fields.length; index += 6) {
    const [itemCategory, providerItemId, code, title, coverAuthorityId, ratioText] =
      fields.slice(index, index + 6);
    const sourceAspectRatio = Number(ratioText);
    if (
      itemCategory !== category ||
      !providerItemPattern.test(providerItemId) ||
      canonicalizeProductCode(code) !== code ||
      providerItems.has(providerItemId) ||
      (coverAuthorityId !== "" &&
        (!coverAuthorityPattern.test(coverAuthorityId) ||
          !coverAuthorityId.startsWith(`fanza-cover-${requestGeneration}-`))) ||
      !Number.isFinite(sourceAspectRatio) ||
      sourceAspectRatio <= 0 ||
      sourceAspectRatio > 4
    ) {
      return { status: "malformed-provider" };
    }
    providerItems.add(providerItemId);
    items.push({
      category,
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
  category: FanzaCategory,
  feed: FanzaFeed,
  count: FanzaCount,
  contextGeneration: string,
): Promise<FanzaCatalogResult> {
  if (!feeds.has(feed) || !counts.has(count) || !validGeneration(contextGeneration)) {
    throw new Error("A valid FANZA catalog request is required.");
  }
  try {
    const value = await window.__TAURI__.core.invoke<unknown>(
      "fetch_fanza_catalog",
      { category, feed, count, contextGeneration },
    );
    return parseFanzaCatalogResponse(
      value,
      category,
      contextGeneration,
      count,
    );
  } catch (error: unknown) {
    return { status: fanzaErrorStatus(category, error) };
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

function imageMimeType(bytes: Uint8Array) {
  if (bytes.length >= 3 && bytes[0] === 0xff && bytes[1] === 0xd8 && bytes[2] === 0xff)
    return "image/jpeg";
  if (bytes.length >= 4 && bytes[0] === 0x89 && bytes[1] === 0x50 && bytes[2] === 0x4e && bytes[3] === 0x47)
    return "image/png";
  if (bytes.length >= 4 && bytes[0] === 0x47 && bytes[1] === 0x49 && bytes[2] === 0x46 && bytes[3] === 0x38)
    return "image/gif";
  if (bytes.length >= 12 && bytes[0] === 0x52 && bytes[1] === 0x49 && bytes[2] === 0x46 && bytes[3] === 0x46 && bytes[8] === 0x57 && bytes[9] === 0x45 && bytes[10] === 0x42 && bytes[11] === 0x50)
    return "image/webp";
  return null;
}

async function fetchImage(
  item: FanzaCatalogItem,
  imageAuthorityId: string,
  previewGeneration: string | null,
) {
  if (
    !validItem(item) ||
    (!coverAuthorityPattern.test(imageAuthorityId) &&
      !previewAuthorityPattern.test(imageAuthorityId)) ||
    (previewGeneration !== null && !validGeneration(previewGeneration))
  ) {
    throw new Error("A current FANZA image authority is required.");
  }
  const value = await window.__TAURI__.core.invoke<unknown>("fetch_fanza_image", {
    ...itemAuthority(item),
    previewGeneration,
    imageAuthorityId,
  });
  if (
    !Array.isArray(value) ||
    value.length === 0 ||
    value.length > 16 * 1024 * 1024 ||
    !value.every((byte) => Number.isInteger(byte) && byte >= 0 && byte <= 255)
  ) {
    throw new Error("The native FANZA image response was invalid.");
  }
  const bytes = Uint8Array.from(value as number[]);
  const mimeType = imageMimeType(bytes);
  if (mimeType === null) {
    throw new Error("The native FANZA image response was invalid.");
  }
  return URL.createObjectURL(new Blob([bytes], { type: mimeType }));
}

export function fetchFanzaCoverObjectUrl(item: FanzaCatalogItem) {
  if (item.coverAuthorityId === null) {
    throw new Error("A current FANZA cover authority is required.");
  }
  return fetchImage(item, item.coverAuthorityId, null);
}

export async function fetchFanzaDetail(
  item: FanzaCatalogItem,
): Promise<FanzaDetailResult> {
  if (!validItem(item)) {
    throw new Error("A current FANZA item is required.");
  }
  try {
    const value = await window.__TAURI__.core.invoke<unknown>(
      "fetch_fanza_detail",
      itemAuthority(item),
    );
    if (
      !Array.isArray(value) ||
      value.length !== 8 ||
      !value.every((field) => typeof field === "string") ||
      !validGeneration(value[0]) ||
      value[1] !== item.category ||
      value[2] !== item.contextGeneration ||
      value[3] !== item.requestGeneration ||
      value[4] !== item.providerItemId ||
      value[5] !== item.code ||
      value[6] !== (item.title ?? "") ||
      value[7] !== (item.coverAuthorityId ?? "")
    ) {
      if (Array.isArray(value) && typeof value[0] === "string" && validGeneration(value[0])) {
        await invalidateFanzaDetail(item.category, value[0]).catch(() => undefined);
      }
      return { status: "malformed-provider" };
    }
    return { status: "ready", item, detailGeneration: value[0] as string };
  } catch (error: unknown) {
    return { status: fanzaErrorStatus(item.category, error) };
  }
}

export async function fetchFanzaPreview(
  item: FanzaCatalogItem,
  detailGeneration: string,
): Promise<FanzaPreviewResult> {
  if (!validItem(item) || !validGeneration(detailGeneration)) {
    throw new Error("A current FANZA item is required.");
  }
  try {
    const value = await window.__TAURI__.core.invoke<unknown>(
      "fetch_fanza_preview",
      { ...itemAuthority(item), detailGeneration },
    );
    const returnedGeneration =
      Array.isArray(value) &&
      typeof value[0] === "string" &&
      validGeneration(value[0])
        ? value[0]
        : null;
    if (
      !Array.isArray(value) ||
      value.length < 2 ||
      !value.every((field) => typeof field === "string")
    ) {
      if (returnedGeneration !== null) {
        await invalidateFanzaPreview(item.category, returnedGeneration).catch(
          () => undefined,
        );
      }
      return { status: "malformed-provider" };
    }
    const [previewGeneration, countText, ...authorityIds] = value as string[];
    const count = Number(countText);
    if (
      !validGeneration(previewGeneration) ||
      !/^\d{1,2}$/.test(countText) ||
      !Number.isSafeInteger(count) ||
      count > 24 ||
      authorityIds.length !== count ||
      new Set(authorityIds).size !== count ||
      authorityIds.some(
        (id) =>
          !previewAuthorityPattern.test(id) ||
          !id.startsWith(`fanza-preview-${previewGeneration}-`),
      )
    ) {
      if (returnedGeneration !== null) {
        await invalidateFanzaPreview(item.category, returnedGeneration).catch(
          () => undefined,
        );
      }
      return { status: "malformed-provider" };
    }
    return { status: "ready", previewGeneration, authorityIds };
  } catch (error: unknown) {
    return { status: fanzaErrorStatus(item.category, error) };
  }
}

export function fetchFanzaPreviewImageObjectUrl(
  item: FanzaCatalogItem,
  previewGeneration: string,
  imageAuthorityId: string,
) {
  return fetchImage(item, imageAuthorityId, previewGeneration);
}

export async function invalidateFanzaPreview(
  category: FanzaCategory,
  previewGeneration: string,
) {
  if (!validGeneration(previewGeneration)) {
    throw new Error("A valid FANZA preview generation is required.");
  }
  await window.__TAURI__.core.invoke("invalidate_fanza_preview", {
    category,
    previewGeneration,
  });
}

export async function invalidateFanzaDetail(
  category: FanzaCategory,
  detailGeneration: string,
) {
  if (!validGeneration(detailGeneration)) {
    throw new Error("A valid FANZA detail generation is required.");
  }
  await window.__TAURI__.core.invoke("invalidate_fanza_detail", {
    category,
    detailGeneration,
  });
}

export async function openFanzaSource(
  item: FanzaCatalogItem,
  detailGeneration: string,
) {
  if (!validItem(item) || !validGeneration(detailGeneration)) {
    throw new Error("A current FANZA item is required.");
  }
  await window.__TAURI__.core.invoke("open_fanza_source", {
    ...itemAuthority(item),
    detailGeneration,
  });
}
