import { useEffect, useMemo, useRef, useState } from "react";

import {
  adultLibraryProductCodePrefixIsSupported,
  canonicalizeProductCode,
  productCodeDisplayForm,
  vrLibraryProductCodePrefixIsSupported,
} from "@/vr";

export type AutomaticLibraryCategory = "adult" | "vr";

export type LibraryPresentationRequest = {
  category: AutomaticLibraryCategory;
  itemId: string;
  scanGeneration: string;
};

export type LibraryCover = {
  state: "ready" | "missing" | "unavailable";
  source: string | null;
  providerId: string | null;
  sourceDisplayCode: string | null;
  authorityId: string | null;
  aspectRatio: number;
  verifiedIdentity: VerifiedDisplayIdentity | null;
};

export type VerifiedDisplayIdentity = {
  provider: "JavDB" | "FANZA";
  providerId: string;
  displayCode: string;
};

export type LibraryMetadata = {
  state: "automatic" | "local-only" | "unavailable";
  verifiedIdentity: VerifiedDisplayIdentity | null;
  identityConflict: boolean;
  source: string | null;
  providerId: string | null;
  displayCode: string | null;
  title: string | null;
  date: string | null;
  runtime: string | null;
  cast: string[];
};

export type LibraryPresentationState = {
  verifiedIdentity: VerifiedDisplayIdentity | null;
  cover: {
    status: "loading" | "ready" | "missing" | "unavailable";
    objectUrl: string | null;
    aspectRatio: number;
    source: string | null;
    retry: (() => void) | null;
    reportDecodeFailure: (() => void) | null;
  };
  metadata: {
    status: "waiting" | "loading" | "automatic" | "local-only" | "unavailable";
    value: LibraryMetadata | null;
    retry: (() => void) | null;
  };
};

const defaultAspectRatio = 0.72;
const maximumCoverBytes = 16 * 1024 * 1024;
const maximumConcurrentWork = 4;
const coverAuthorityPattern = /^library-cover-[a-f0-9]{40}$/;

class LibraryIdentityResponseError extends Error {}

type RequestProductIdentity = {
  canonicalCode: string;
  number: string;
  prefix: string;
};

function requestProductIdentity(
  request: LibraryPresentationRequest,
): RequestProductIdentity | null {
  const canonicalCode = canonicalizeProductCode(request.itemId);
  if (canonicalCode === null) return null;
  const match = canonicalCode.match(/^([A-Z0-9]{2,16})-([1-9][0-9]*)$/);
  if (match === null) return null;
  const [, prefix, number] = match;
  const prefixIsSupported =
    request.category === "adult"
      ? adultLibraryProductCodePrefixIsSupported(prefix)
      : vrLibraryProductCodePrefixIsSupported(prefix);
  return prefixIsSupported ? { canonicalCode, number, prefix } : null;
}

function displayCodeMatchesRequest(
  request: LibraryPresentationRequest,
  displayCode: string,
) {
  const identity = requestProductIdentity(request);
  return (
    identity !== null &&
    productCodeDisplayForm(displayCode) === displayCode &&
    canonicalizeProductCode(displayCode) === identity.canonicalCode
  );
}

function exactFanzaIdentityMatches(
  request: LibraryPresentationRequest,
  providerId: string,
  displayCode: string,
) {
  const identity = requestProductIdentity(request);
  if (identity === null) return false;
  if (request.category === "adult" && identity.prefix === "CAWB") {
    return (
      providerId === `cawb${identity.number.padStart(5, "0")}` &&
      displayCode === `CAWB-${identity.number.padStart(3, "0")}`
    );
  }
  if (request.category === "vr" && identity.prefix === "3DSVR") {
    return (
      providerId === `13dsvr${identity.number.padStart(5, "0")}` &&
      displayCode === `3DSVR-${identity.number.padStart(5, "0")}`
    );
  }
  return false;
}

function verifiedIdentityMatchesRequest(
  request: LibraryPresentationRequest,
  identity: VerifiedDisplayIdentity,
) {
  if (!displayCodeMatchesRequest(request, identity.displayCode)) return false;
  if (identity.provider === "FANZA") {
    return exactFanzaIdentityMatches(
      request,
      identity.providerId,
      identity.displayCode,
    );
  }
  return /^[A-Za-z0-9]{1,64}$/.test(identity.providerId);
}

function reconcileVerifiedIdentities(
  current: VerifiedDisplayIdentity | null,
  candidate: VerifiedDisplayIdentity | null,
): { identity: VerifiedDisplayIdentity | null; conflict: boolean } {
  if (current === null || candidate === null) {
    return { identity: current ?? candidate, conflict: false };
  }
  if (current.provider === candidate.provider) {
    return current.providerId === candidate.providerId &&
      current.displayCode === candidate.displayCode
      ? { identity: current, conflict: false }
      : { identity: null, conflict: true };
  }
  if (current.displayCode !== candidate.displayCode) {
    return { identity: null, conflict: true };
  }
  return {
    identity: current.provider === "JavDB" ? current : candidate,
    conflict: false,
  };
}

type Work = {
  priority: "cover" | "metadata";
  shouldStart: () => boolean;
  run: () => Promise<void>;
};

const queue: Work[] = [];
let activeWork = 0;
let nextCoverRequestGeneration = 0;

function runQueuedWork() {
  while (activeWork < maximumConcurrentWork) {
    const coverIndex = queue.findIndex((work) => work.priority === "cover");
    const work = queue.splice(coverIndex < 0 ? 0 : coverIndex, 1)[0];
    if (work === undefined) return;
    if (!work.shouldStart()) {
      void work.run();
      continue;
    }
    activeWork += 1;
    void work.run().finally(() => {
      activeWork -= 1;
      runQueuedWork();
    });
  }
}

export function scheduleLibraryPresentation<T>(
  priority: Work["priority"],
  work: () => Promise<T>,
  shouldStart: () => boolean = () => true,
) {
  return new Promise<T>((resolve, reject) => {
    queue.push({
      priority,
      shouldStart,
      run: async () => {
        if (!shouldStart()) {
          reject(new Error("The Library presentation request is no longer current."));
          return;
        }
        try {
          resolve(await work());
        } catch (error: unknown) {
          reject(error instanceof Error ? error : new Error(String(error)));
        }
      },
    });
    runQueuedWork();
  });
}

function optionalText(value: string) {
  return value === "" ? null : value;
}

export function parseLibraryCover(
  value: unknown,
  request: LibraryPresentationRequest,
): LibraryCover | null {
  if (
    !Array.isArray(value) ||
    value.length !== 11 ||
    !value.every((field) => typeof field === "string")
  ) {
    return null;
  }
  const [
    version,
    returnedCategory,
    state,
    source,
    providerId,
    displayCode,
    authorityId,
    ratioText,
    verificationProvider,
    verificationProviderId,
    verifiedDisplayCode,
  ] =
    value as string[];
  const ratio = Number(ratioText);
  if (
    version !== "library-cover-v3" ||
    requestProductIdentity(request) === null ||
    returnedCategory !== request.category ||
    !["ready", "missing", "unavailable"].includes(state) ||
    !Number.isFinite(ratio) ||
    ratio <= 0 ||
    ratio > 4 ||
    (state === "ready" &&
      (!(["JavDB", "FANZA", "r18.dev"] as string[]).includes(source) ||
        providerId === "" ||
        (source !== "r18.dev" && !/^[A-Za-z0-9]{1,64}$/.test(providerId)) ||
        (source === "r18.dev" && productCodeDisplayForm(providerId) === null) ||
        displayCode === "" ||
        productCodeDisplayForm(displayCode) !== displayCode ||
        !coverAuthorityPattern.test(authorityId))) ||
    (state !== "ready" &&
      (source !== "" ||
        providerId !== "" ||
        displayCode !== "" ||
        authorityId !== "")) ||
    ((verificationProvider === "" ||
      verificationProviderId === "" ||
      verifiedDisplayCode === "") &&
      (verificationProvider !== "" ||
        verificationProviderId !== "" ||
        verifiedDisplayCode !== "")) ||
    (verificationProvider !== "" &&
      (!(["JavDB", "FANZA"] as string[]).includes(verificationProvider) ||
        !verifiedIdentityMatchesRequest(request, {
          provider: verificationProvider as VerifiedDisplayIdentity["provider"],
          providerId: verificationProviderId,
          displayCode: verifiedDisplayCode,
        }))) ||
    (state === "ready" &&
      (source === "FANZA"
        ? !exactFanzaIdentityMatches(request, providerId, displayCode)
        : !displayCodeMatchesRequest(request, displayCode))) ||
    (state === "ready" &&
      ((source === "JavDB" &&
        (verificationProvider !== "JavDB" ||
          verificationProviderId !== providerId ||
          verifiedDisplayCode !== displayCode)) ||
        (source === "FANZA" &&
          (verificationProvider === "" ||
            verifiedDisplayCode !== displayCode ||
            (verificationProvider === "FANZA" &&
              verificationProviderId !== providerId))) ||
        (source === "r18.dev" &&
          verificationProvider !== "" &&
          verifiedDisplayCode !== displayCode)))
  ) {
    return null;
  }
  return {
    state: state as LibraryCover["state"],
    source: optionalText(source),
    providerId: optionalText(providerId),
    sourceDisplayCode: optionalText(displayCode),
    authorityId: optionalText(authorityId),
    aspectRatio: ratio,
    verifiedIdentity:
      verificationProvider === ""
        ? null
        : {
            provider: verificationProvider as VerifiedDisplayIdentity["provider"],
            providerId: verificationProviderId,
            displayCode: verifiedDisplayCode,
          },
  };
}

export function parseLibraryMetadata(
  value: unknown,
  request: LibraryPresentationRequest,
): LibraryMetadata | null {
  if (
    !Array.isArray(value) ||
    value.length < 14 ||
    !value.every((field) => typeof field === "string")
  ) {
    return null;
  }
  const fields = value as string[];
  const [
    version,
    returnedCategory,
    state,
    identityState,
    verificationProvider,
    verificationProviderId,
    verifiedDisplayCode,
    source,
    providerId,
    displayCode,
    title,
    date,
    runtime,
    castCountText,
  ] =
    fields;
  const castCount = Number(castCountText);
  const hasCompleteIdentity =
    verificationProvider !== "" &&
    verificationProviderId !== "" &&
    verifiedDisplayCode !== "";
  if (
    version !== "library-metadata-v3" ||
    requestProductIdentity(request) === null ||
    returnedCategory !== request.category ||
    !["automatic", "local-only", "unavailable"].includes(state) ||
    !["current", "conflict"].includes(identityState) ||
    !/^\d{1,2}$/.test(castCountText) ||
    !Number.isSafeInteger(castCount) ||
    fields.length !== 14 + castCount ||
    fields.slice(14).some((entry) => entry.trim() === "") ||
    ((verificationProvider === "" ||
      verificationProviderId === "" ||
      verifiedDisplayCode === "") &&
      (verificationProvider !== "" ||
        verificationProviderId !== "" ||
        verifiedDisplayCode !== "")) ||
    (hasCompleteIdentity &&
      (!(["JavDB", "FANZA"] as string[]).includes(verificationProvider) ||
        !verifiedIdentityMatchesRequest(request, {
          provider: verificationProvider as VerifiedDisplayIdentity["provider"],
          providerId: verificationProviderId,
          displayCode: verifiedDisplayCode,
        }))) ||
    (identityState === "conflict" &&
      (state !== "unavailable" || hasCompleteIdentity)) ||
    (state === "automatic" &&
      (source === "" ||
        providerId === "" ||
        displayCode === "" ||
        !displayCodeMatchesRequest(request, displayCode) ||
        (hasCompleteIdentity && displayCode !== verifiedDisplayCode) ||
        (source.startsWith("JavDB") &&
          (verificationProvider !== "JavDB" ||
            verificationProviderId !== providerId)))) ||
    (state !== "automatic" &&
      [
        source,
        providerId,
        displayCode,
        title,
        date,
        runtime,
        ...fields.slice(14),
      ].some(Boolean)) ||
    (date !== "" && !/^\d{4}-\d{2}-\d{2}$/.test(date))
  ) {
    return null;
  }
  return {
    state: state as LibraryMetadata["state"],
    verifiedIdentity: hasCompleteIdentity
      ? {
          provider: verificationProvider as VerifiedDisplayIdentity["provider"],
          providerId: verificationProviderId,
          displayCode: verifiedDisplayCode,
        }
      : null,
    identityConflict: identityState === "conflict",
    source: optionalText(source),
    providerId: optionalText(providerId),
    displayCode: optionalText(displayCode),
    title: optionalText(title),
    date: optionalText(date),
    runtime: optionalText(runtime),
    cast: fields.slice(14),
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

function requestArguments(request: LibraryPresentationRequest) {
  return {
    category: request.category,
    itemId: request.itemId,
    scanGeneration: request.scanGeneration,
  };
}

async function resolveCover(
  request: LibraryPresentationRequest,
  coverRequestGeneration: string,
) {
  const value = await window.__TAURI__.core.invoke<unknown>(
    "resolve_library_cover",
    { ...requestArguments(request), coverRequestGeneration },
  );
  const cover = parseLibraryCover(value, request);
  if (cover === null) {
    throw new LibraryIdentityResponseError(
      "The native Library cover response was invalid.",
    );
  }
  return cover;
}

async function fetchCover(
  request: LibraryPresentationRequest,
  authorityId: string,
) {
  const value = await window.__TAURI__.core.invoke<unknown>("fetch_library_cover", {
    ...requestArguments(request),
    coverAuthorityId: authorityId,
  });
  if (
    !Array.isArray(value) ||
    value.length === 0 ||
    value.length > maximumCoverBytes ||
    !value.every(
      (entry) => Number.isInteger(entry) && Number(entry) >= 0 && Number(entry) <= 255,
    )
  ) {
    throw new Error("The native Library cover bytes were invalid.");
  }
  const bytes = Uint8Array.from(value as number[]);
  const type = coverMimeType(bytes);
  if (type === null) throw new Error("The native Library cover bytes were invalid.");
  return URL.createObjectURL(new Blob([bytes], { type }));
}

async function cancelCoverRequest(
  request: LibraryPresentationRequest,
  coverRequestGeneration: string,
) {
  await window.__TAURI__.core.invoke("cancel_library_cover_request", {
    category: request.category,
    itemId: request.itemId,
    coverRequestGeneration,
  });
}

async function invalidateCover(
  request: LibraryPresentationRequest,
  coverRequestGeneration: string,
  authorityId: string,
) {
  await window.__TAURI__.core.invoke("invalidate_library_cover", {
    ...requestArguments(request),
    coverRequestGeneration,
    coverAuthorityId: authorityId,
  });
}

async function resolveMetadata(
  request: LibraryPresentationRequest,
  coverRequestGeneration: string,
) {
  const value = await window.__TAURI__.core.invoke<unknown>(
    "resolve_library_metadata",
    { ...requestArguments(request), coverRequestGeneration },
  );
  const metadata = parseLibraryMetadata(value, request);
  if (metadata === null) {
    throw new LibraryIdentityResponseError(
      "The native Library metadata response was invalid.",
    );
  }
  return metadata;
}

function initialState(): LibraryPresentationState {
  return {
    verifiedIdentity: null,
    cover: {
      status: "loading",
      objectUrl: null,
      aspectRatio: defaultAspectRatio,
      source: null,
      retry: null,
      reportDecodeFailure: null,
    },
    metadata: { status: "waiting", value: null, retry: null },
  };
}

export function useLibraryPresentation(
  request: LibraryPresentationRequest | null,
): LibraryPresentationState {
  const requestKey = request === null ? "" : JSON.stringify(request);
  const stableRequest = useMemo<LibraryPresentationRequest | null>(
    () => (requestKey === "" ? null : (JSON.parse(requestKey) as LibraryPresentationRequest)),
    [requestKey],
  );
  const [coverRetry, setCoverRetry] = useState(0);
  const [metadataRetry, setMetadataRetry] = useState(0);
  const [completedCover, setCompletedCover] = useState<{
    requestKey: string;
    requestGeneration: string;
  } | null>(null);
  const [state, setState] = useState<LibraryPresentationState>(initialState);
  const currentObjectUrl = useRef<string | null>(null);
  const verifiedIdentity = useRef<VerifiedDisplayIdentity | null>(null);
  const identityConflict = useRef(false);

  useEffect(() => {
    if (stableRequest === null) {
      verifiedIdentity.current = null;
      identityConflict.current = false;
      setState(initialState());
      setCompletedCover(null);
      return;
    }
    verifiedIdentity.current = null;
    identityConflict.current = false;
    setState(initialState());
    setCompletedCover(null);
  }, [stableRequest]);

  useEffect(() => {
    if (stableRequest === null) return;
    let current = true;
    nextCoverRequestGeneration += 1;
    const coverRequestGeneration = nextCoverRequestGeneration.toString();
    verifiedIdentity.current = null;
    identityConflict.current = false;
    setCompletedCover(null);
    let coverAuthorityId: string | null = null;
    let invalidationPending = false;
    const retryUnavailableCover = () => {
      if (!current || coverAuthorityId === null || invalidationPending) return;
      invalidationPending = true;
      void invalidateCover(
        stableRequest,
        coverRequestGeneration,
        coverAuthorityId,
      )
        .then(() => {
          if (current) setCoverRetry((generation) => generation + 1);
        })
        .catch(() => undefined)
        .finally(() => {
          invalidationPending = false;
        });
    };
    setState((value) => ({
      ...value,
      verifiedIdentity: null,
      cover: {
        status: "loading",
        objectUrl: null,
        aspectRatio: value.cover.aspectRatio,
        source: value.cover.source,
        retry: null,
        reportDecodeFailure: null,
      },
    }));
    const completeCover = () => {
      if (current) {
        setCompletedCover({
          requestKey,
          requestGeneration: coverRequestGeneration,
        });
      }
    };

    void scheduleLibraryPresentation(
      "cover",
      async () => {
        const cover = await resolveCover(stableRequest, coverRequestGeneration);
        const reconciledIdentity = reconcileVerifiedIdentities(
          verifiedIdentity.current,
          cover.verifiedIdentity,
        );
        if (reconciledIdentity.conflict) {
          return {
            cover,
            objectUrl: null,
            fetchFailed: false,
            identityConflict: true,
          };
        }
        verifiedIdentity.current = reconciledIdentity.identity;
        cover.verifiedIdentity = reconciledIdentity.identity;
        coverAuthorityId = cover.authorityId;
        if (!current) {
          return {
            cover,
            objectUrl: null,
            fetchFailed: false,
            identityConflict: false,
          };
        }
        if (cover.state !== "ready" || cover.authorityId === null) {
          return {
            cover,
            objectUrl: null,
            fetchFailed: false,
            identityConflict: false,
          };
        }
        setState((value) => ({
          ...value,
          cover: {
            status: "loading",
            objectUrl: null,
            aspectRatio: cover.aspectRatio,
            source: cover.source,
            retry: null,
            reportDecodeFailure: null,
          },
        }));
        try {
          const objectUrl = await fetchCover(stableRequest, cover.authorityId);
          return {
            cover,
            objectUrl,
            fetchFailed: false,
            identityConflict: false,
          };
        } catch {
          return {
            cover,
            objectUrl: null,
            fetchFailed: true,
            identityConflict: false,
          };
        }
      },
      () => current,
    )
      .then(({ cover, objectUrl, fetchFailed, identityConflict: hasConflict }) => {
        if (!current) {
          if (objectUrl !== null) URL.revokeObjectURL(objectUrl);
          return;
        }
        if (hasConflict) {
          identityConflict.current = true;
          verifiedIdentity.current = null;
          if (currentObjectUrl.current !== null) {
            URL.revokeObjectURL(currentObjectUrl.current);
            currentObjectUrl.current = null;
          }
          setState((value) => ({
            ...value,
            verifiedIdentity: null,
            cover: {
              ...value.cover,
              status: "unavailable",
              objectUrl: null,
              source: null,
              retry: () => setCoverRetry((generation) => generation + 1),
              reportDecodeFailure: null,
            },
            metadata: {
              status: "unavailable",
              value: null,
              retry: () => setMetadataRetry((generation) => generation + 1),
            },
          }));
          completeCover();
          return;
        }
        if (cover.state !== "ready" || cover.authorityId === null) {
          verifiedIdentity.current = cover.verifiedIdentity;
          setState((value) => ({
            ...value,
            verifiedIdentity: cover.verifiedIdentity,
            cover: {
              status: cover.state,
              objectUrl: null,
              aspectRatio: cover.aspectRatio,
              source: cover.source,
              retry:
                cover.state === "unavailable"
                  ? () => setCoverRetry((generation) => generation + 1)
                  : null,
              reportDecodeFailure: null,
            },
          }));
          completeCover();
          return;
        }
        if (!fetchFailed && objectUrl !== null) {
          currentObjectUrl.current = objectUrl;
          verifiedIdentity.current = cover.verifiedIdentity;
          setState((value) => ({
            ...value,
            verifiedIdentity: cover.verifiedIdentity,
            cover: {
              status: "ready",
              objectUrl,
              aspectRatio: cover.aspectRatio,
              source: cover.source,
              retry: null,
              reportDecodeFailure: () => {
                if (!current || currentObjectUrl.current === null) return;
                URL.revokeObjectURL(currentObjectUrl.current);
                currentObjectUrl.current = null;
                setState((currentState) => ({
                  ...currentState,
                  cover: {
                    ...currentState.cover,
                    status: "unavailable",
                    objectUrl: null,
                    retry: retryUnavailableCover,
                    reportDecodeFailure: null,
                  },
                }));
              },
            },
          }));
        } else {
          verifiedIdentity.current = cover.verifiedIdentity;
          setState((value) => ({
            ...value,
            verifiedIdentity: cover.verifiedIdentity,
            cover: {
              status: "unavailable",
              objectUrl: null,
              aspectRatio: cover.aspectRatio,
              source: cover.source,
              retry: retryUnavailableCover,
              reportDecodeFailure: null,
            },
          }));
        }
        completeCover();
      })
      .catch((error: unknown) => {
        if (!current) return;
        verifiedIdentity.current = null;
        identityConflict.current = error instanceof LibraryIdentityResponseError;
        if (currentObjectUrl.current !== null) {
          URL.revokeObjectURL(currentObjectUrl.current);
          currentObjectUrl.current = null;
        }
        setState((value) => ({
          ...value,
          verifiedIdentity: null,
          cover: {
            ...value.cover,
            status: "unavailable",
            source:
              error instanceof LibraryIdentityResponseError
                ? null
                : value.cover.source,
            retry: () => setCoverRetry((generation) => generation + 1),
          },
        }));
        completeCover();
      });

    return () => {
      current = false;
      void cancelCoverRequest(stableRequest, coverRequestGeneration).catch(
        () => undefined,
      );
      if (currentObjectUrl.current !== null) {
        URL.revokeObjectURL(currentObjectUrl.current);
        currentObjectUrl.current = null;
      }
    };
  }, [coverRetry, requestKey, stableRequest]);

  useEffect(() => {
    if (
      stableRequest === null ||
      completedCover?.requestKey !== requestKey
    ) {
      return;
    }
    let current = true;
    setState((value) => ({
      ...value,
      metadata: { status: "loading", value: null, retry: null },
    }));
    void scheduleLibraryPresentation(
      "metadata",
      () => resolveMetadata(stableRequest, completedCover.requestGeneration),
      () => current,
    )
      .then((metadata) => {
        if (!current) return;
        setState((value) => {
          const reconciledIdentity = reconcileVerifiedIdentities(
            verifiedIdentity.current,
            metadata.verifiedIdentity,
          );
          if (
            identityConflict.current ||
            metadata.identityConflict ||
            reconciledIdentity.conflict
          ) {
            identityConflict.current = true;
            verifiedIdentity.current = null;
            if (currentObjectUrl.current !== null) {
              URL.revokeObjectURL(currentObjectUrl.current);
              currentObjectUrl.current = null;
            }
            return {
              ...value,
              verifiedIdentity: null,
              cover: {
                ...value.cover,
                status: "unavailable",
                objectUrl: null,
                source: null,
                retry: () => setCoverRetry((generation) => generation + 1),
                reportDecodeFailure: null,
              },
              metadata: {
                status: "unavailable",
                value: null,
                retry: () => setMetadataRetry((generation) => generation + 1),
              },
            };
          }
          verifiedIdentity.current = reconciledIdentity.identity;
          return {
            ...value,
            verifiedIdentity: reconciledIdentity.identity,
            metadata: {
              status: metadata.state,
              value: metadata,
              retry:
                metadata.state === "unavailable"
                  ? () => setMetadataRetry((generation) => generation + 1)
                  : null,
            },
          };
        });
      })
      .catch((error: unknown) => {
        if (!current) return;
        if (error instanceof LibraryIdentityResponseError) {
          identityConflict.current = true;
          verifiedIdentity.current = null;
          if (currentObjectUrl.current !== null) {
            URL.revokeObjectURL(currentObjectUrl.current);
            currentObjectUrl.current = null;
          }
          setState((value) => ({
            ...value,
            verifiedIdentity: null,
            cover: {
              ...value.cover,
              status: "unavailable",
              objectUrl: null,
              source: null,
              retry: () => setCoverRetry((generation) => generation + 1),
              reportDecodeFailure: null,
            },
            metadata: {
              status: "unavailable",
              value: null,
              retry: () => setMetadataRetry((generation) => generation + 1),
            },
          }));
          return;
        }
        setState((value) => ({
          ...value,
          metadata: {
            status: "unavailable",
            value: null,
            retry: () => setMetadataRetry((generation) => generation + 1),
          },
        }));
      });

    return () => {
      current = false;
    };
  }, [completedCover, metadataRetry, requestKey, stableRequest]);

  return state;
}
