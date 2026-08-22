import { useEffect, useMemo, useState } from "react";

export type LibraryEnrichmentCategory = "movie" | "tv" | "adult" | "vr";

export type LibraryEnrichmentRequest = {
  category: LibraryEnrichmentCategory;
  itemId: string;
  scanGeneration: string;
  code: string | null;
  credentialGeneration?: number;
};

export type LibraryPresentation = {
  state: "automatic" | "local-only";
  source: string | null;
  providerId: string | null;
  imdbId: string | null;
  title: string | null;
  originalTitle: string | null;
  date: string | null;
  runtime: string | null;
  genres: string[];
  cast: string[];
  overview: string | null;
  coverAuthorityId: string | null;
  coverState: "ready" | "missing" | "unavailable";
  aspectRatio: number;
};

export type LibraryPresentationState =
  | { status: "loading" }
  | {
      status: "ready";
      presentation: LibraryPresentation;
      coverUrl: string | null;
      retryCover: (() => void) | null;
    }
  | { status: "error"; retry: () => void };

const responseVersion = "library-enrichment-v1";
const authorityPattern = /^library-cover-[a-f0-9]{40}$/;
const maximumCoverBytes = 16 * 1024 * 1024;
const maximumConcurrentWork = 4;

function optionalText(value: string) {
  return value === "" ? null : value;
}

function hasControlCharacter(value: string) {
  return Array.from(value).some((character) => {
    const code = character.charCodeAt(0);
    return code <= 0x1f || code === 0x7f;
  });
}

export function parseLibraryPresentation(
  value: unknown,
  category: LibraryEnrichmentCategory,
): LibraryPresentation | null {
  if (
    !Array.isArray(value) ||
    value.length < 16 ||
    !value.every((entry) => typeof entry === "string")
  ) {
    return null;
  }
  const fields = value as string[];
  const [
    version,
    returnedCategory,
    presentationState,
    source,
    providerId,
    imdbId,
    title,
    originalTitle,
    date,
    runtime,
    overview,
    coverAuthorityId,
    coverState,
    aspectRatioText,
    genreCountText,
  ] = fields;
  const genreCount = Number(genreCountText);
  const aspectRatio = Number(aspectRatioText);
  if (
    version !== responseVersion ||
    returnedCategory !== category ||
    !["automatic", "local-only"].includes(presentationState) ||
    (presentationState === "automatic" && (source === "" || providerId === "")) ||
    (presentationState === "local-only" &&
      [source, providerId, imdbId, title, originalTitle, date, runtime, overview].some(Boolean)) ||
    (providerId !== "" &&
      (providerId !== providerId.trim() ||
        providerId.length > 128 ||
        hasControlCharacter(providerId))) ||
    (imdbId !== "" && !/^tt\d{7,10}$/.test(imdbId)) ||
    (date !== "" && !/^\d{4}-\d{2}-\d{2}$/.test(date)) ||
    (coverAuthorityId !== "" && !authorityPattern.test(coverAuthorityId)) ||
    !["ready", "missing", "unavailable"].includes(coverState) ||
    (coverState === "ready" && coverAuthorityId === "") ||
    (coverState !== "ready" && coverAuthorityId !== "") ||
    !Number.isFinite(aspectRatio) ||
    aspectRatio <= 0 ||
    aspectRatio > 4 ||
    !/^\d{1,3}$/.test(genreCountText) ||
    !Number.isSafeInteger(genreCount)
  ) {
    return null;
  }
  let cursor = 15;
  const genres = fields.slice(cursor, cursor + genreCount);
  cursor += genreCount;
  const castCountText = fields[cursor];
  const castCount = Number(castCountText);
  cursor += 1;
  if (
    castCountText === undefined ||
    !/^\d{1,3}$/.test(castCountText) ||
    !Number.isSafeInteger(castCount) ||
    fields.length !== cursor + castCount ||
    genres.some((entry) => entry.trim() === "") ||
    fields.slice(cursor).some((entry) => entry.trim() === "")
  ) {
    return null;
  }
  return {
    state: presentationState as LibraryPresentation["state"],
    source: optionalText(source),
    providerId: optionalText(providerId),
    imdbId: optionalText(imdbId),
    title: optionalText(title),
    originalTitle: optionalText(originalTitle),
    date: optionalText(date),
    runtime: optionalText(runtime),
    genres,
    cast: fields.slice(cursor),
    overview: optionalText(overview),
    coverAuthorityId: optionalText(coverAuthorityId),
    coverState: coverState as LibraryPresentation["coverState"],
    aspectRatio,
  };
}

function coverMimeType(bytes: Uint8Array) {
  if (bytes[0] === 0xff && bytes[1] === 0xd8) return "image/jpeg";
  if (
    bytes[0] === 0x89 &&
    bytes[1] === 0x50 &&
    bytes[2] === 0x4e &&
    bytes[3] === 0x47
  ) {
    return "image/png";
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

type QueuedWork = {
  run: () => Promise<void>;
  shouldStart: () => boolean;
};

const queue: QueuedWork[] = [];
let activeWork = 0;

function startQueuedWork() {
  while (activeWork < maximumConcurrentWork) {
    const work = queue.shift();
    if (work === undefined) return;
    if (!work.shouldStart()) {
      void work.run();
      continue;
    }
    activeWork += 1;
    void work.run().finally(() => {
      activeWork -= 1;
      startQueuedWork();
    });
  }
}

export function scheduleLibraryEnrichment<T>(
  work: () => Promise<T>,
  shouldStart: () => boolean = () => true,
) {
  return new Promise<T>((resolve, reject) => {
    queue.push({
      run: async () => {
        if (!shouldStart()) {
          reject(new Error("The Library enrichment request is no longer current."));
          return;
        }
        try {
          resolve(await work());
        } catch (error: unknown) {
          reject(error instanceof Error ? error : new Error(String(error)));
        }
      },
      shouldStart,
    });
    startQueuedWork();
  });
}

function requestKey(request: LibraryEnrichmentRequest) {
  return [
    request.category,
    request.itemId,
    request.scanGeneration,
    request.code ?? "",
    request.credentialGeneration ?? "",
  ].join("\0");
}

const presentationRequests = new Map<string, Promise<LibraryPresentation>>();
const coverRequests = new Map<string, Promise<{ bytes: Uint8Array; type: string }>>();
const requestConsumers = new Map<string, number>();
const requestVersions = new Map<string, number>();

async function fetchPresentation(request: LibraryEnrichmentRequest) {
  const value = await window.__TAURI__.core.invoke<unknown>(
    "fetch_library_presentation",
    {
      category: request.category,
      itemId: request.itemId,
      scanGeneration: request.scanGeneration,
      code: request.code,
    },
  );
  const presentation = parseLibraryPresentation(value, request.category);
  if (presentation === null) {
    throw new Error("The native Library presentation response was invalid.");
  }
  return presentation;
}

function loadPresentation(request: LibraryEnrichmentRequest) {
  const key = requestKey(request);
  const current = presentationRequests.get(key);
  if (current !== undefined) return current;
  const version = requestVersions.get(key) ?? 0;
  const next = scheduleLibraryEnrichment(
    () => fetchPresentation(request),
    () =>
      (requestConsumers.get(key) ?? 0) > 0 &&
      (requestVersions.get(key) ?? 0) === version,
  );
  presentationRequests.set(key, next);
  void next.catch(() => {
    if (presentationRequests.get(key) === next) presentationRequests.delete(key);
  });
  return next;
}

async function fetchCover(
  request: LibraryEnrichmentRequest,
  authorityId: string,
) {
  const value = await window.__TAURI__.core.invoke<unknown>(
    "fetch_library_cover",
    {
      category: request.category,
      itemId: request.itemId,
      scanGeneration: request.scanGeneration,
      code: request.code,
      coverAuthorityId: authorityId,
    },
  );
  if (
    !Array.isArray(value) ||
    value.length === 0 ||
    value.length > maximumCoverBytes ||
    !value.every(
      (entry) => Number.isInteger(entry) && entry >= 0 && entry <= 255,
    )
  ) {
    throw new Error("The native Library cover response was invalid.");
  }
  const bytes = Uint8Array.from(value as number[]);
  const type = coverMimeType(bytes);
  if (type === null) {
    throw new Error("The native Library cover response was invalid.");
  }
  return { bytes, type };
}

function loadCover(
  request: LibraryEnrichmentRequest,
  authorityId: string,
) {
  const key = `${requestKey(request)}\0${authorityId}`;
  const current = coverRequests.get(key);
  if (current !== undefined) return current;
  const requestIdentity = requestKey(request);
  const version = requestVersions.get(requestIdentity) ?? 0;
  const next = scheduleLibraryEnrichment(
    () => fetchCover(request, authorityId),
    () =>
      (requestConsumers.get(requestIdentity) ?? 0) > 0 &&
      (requestVersions.get(requestIdentity) ?? 0) === version,
  );
  coverRequests.set(key, next);
  void next.catch(() => {
    if (coverRequests.get(key) === next) coverRequests.delete(key);
  });
  return next;
}

export function invalidateLibraryEnrichment(
  category?: LibraryEnrichmentCategory,
) {
  for (const key of presentationRequests.keys()) {
    if (category === undefined || key.startsWith(`${category}\0`)) {
      presentationRequests.delete(key);
      requestVersions.set(key, (requestVersions.get(key) ?? 0) + 1);
    }
  }
  for (const key of coverRequests.keys()) {
    if (category === undefined || key.startsWith(`${category}\0`)) {
      coverRequests.delete(key);
    }
  }
}

export function useLibraryPresentation(
  request: LibraryEnrichmentRequest | null,
): LibraryPresentationState {
  const [retryGeneration, setRetryGeneration] = useState(0);
  const [state, setState] = useState<LibraryPresentationState>({
    status: "loading",
  });
  const key = request === null ? "" : requestKey(request);
  const stableRequest = useMemo<LibraryEnrichmentRequest | null>(() => {
    if (key === "") return null;
    const [category, itemId, scanGeneration, code, credentialGeneration] =
      key.split("\0");
    return {
      category: category as LibraryEnrichmentCategory,
      itemId,
      scanGeneration,
      code: code === "" ? null : code,
      credentialGeneration:
        credentialGeneration === ""
          ? undefined
          : Number(credentialGeneration),
    };
  }, [key]);

  useEffect(() => {
    if (stableRequest === null) return;
    let current = true;
    let objectUrl: string | null = null;
    const currentKey = requestKey(stableRequest);
    requestConsumers.set(
      currentKey,
      (requestConsumers.get(currentKey) ?? 0) + 1,
    );
    setState({ status: "loading" });
    void loadPresentation(stableRequest)
      .then(async (presentation) => {
        if (!current) return;
        if (presentation.coverAuthorityId === null) {
          setState({
            status: "ready",
            presentation,
            coverUrl: null,
            retryCover:
              presentation.state === "automatic" &&
              presentation.coverState === "unavailable"
                ? () => {
                    presentationRequests.delete(requestKey(stableRequest));
                    setRetryGeneration((generation) => generation + 1);
                  }
                : null,
          });
          return;
        }
        try {
          const cover = await loadCover(
            stableRequest,
            presentation.coverAuthorityId,
          );
          if (!current) return;
          objectUrl = URL.createObjectURL(
            new Blob([cover.bytes as BlobPart], { type: cover.type }),
          );
          setState({
            status: "ready",
            presentation,
            coverUrl: objectUrl,
            retryCover: null,
          });
        } catch {
          if (!current) return;
          setState({
            status: "ready",
            presentation,
            coverUrl: null,
            retryCover: () =>
              setRetryGeneration((generation) => generation + 1),
          });
        }
      })
      .catch(() => {
        if (!current) return;
        setState({
          status: "error",
          retry: () => {
            presentationRequests.delete(requestKey(stableRequest));
            setRetryGeneration((generation) => generation + 1);
          },
        });
      });
    return () => {
      current = false;
      const consumers = (requestConsumers.get(currentKey) ?? 1) - 1;
      if (consumers === 0) requestConsumers.delete(currentKey);
      else requestConsumers.set(currentKey, consumers);
      if (objectUrl !== null) URL.revokeObjectURL(objectUrl);
    };
  }, [stableRequest, retryGeneration]);

  return state;
}

export const libraryEnrichmentConcurrency = maximumConcurrentWork;
