import { useEffect, useMemo, useRef, useState } from "react";

import { scheduleLibraryPresentation } from "@/library-presentation";

export type TmdbCardCoverRequest = {
  category: "movie" | "tv";
  surface: "discover" | "library";
  tmdbId: number;
  posterPath: string | null;
  contextGeneration: string;
  libraryItemId?: string;
  associationGeneration?: string;
  scanGeneration?: string;
};

export type TmdbCardCover = {
  status: "loading" | "ready" | "missing" | "unavailable";
  objectUrl: string | null;
  source: "TMDB" | null;
  retry: (() => void) | null;
  reportDecodeSuccess: (() => void) | null;
  reportDecodeFailure: (() => void) | null;
};

const maximumCoverBytes = 16 * 1024 * 1024;
const authorityPattern = /^tmdb-cover-[a-f0-9]{40}$/;
let nextRequestGeneration = 0;

function requestArguments(
  request: TmdbCardCoverRequest,
  requestGeneration: string,
) {
  return {
    associationGeneration: request.associationGeneration,
    category: request.category,
    contextGeneration: request.contextGeneration,
    libraryItemId: request.libraryItemId,
    posterPath: request.posterPath,
    requestGeneration,
    scanGeneration: request.scanGeneration,
    surface: request.surface,
    tmdbId: String(request.tmdbId),
  };
}

export function parseTmdbCardCoverResponse(
  value: unknown,
  request: TmdbCardCoverRequest,
  requestGeneration: string,
) {
  if (
    !Array.isArray(value) ||
    value.length !== 14 ||
    !value.every((field) => typeof field === "string")
  ) {
    return null;
  }
  const [
    version,
    status,
    category,
    surface,
    tmdbId,
    posterPath,
    contextGeneration,
    returnedRequestGeneration,
    libraryItemId,
    associationGeneration,
    scanGeneration,
    authorityId,
    ratio,
    source,
  ] = value;
  const commonIsExact =
    version === "tmdb-card-cover-v1" &&
    category === request.category &&
    surface === request.surface &&
    tmdbId === String(request.tmdbId) &&
    contextGeneration === request.contextGeneration &&
    returnedRequestGeneration === requestGeneration &&
    libraryItemId === (request.libraryItemId ?? "") &&
    associationGeneration === (request.associationGeneration ?? "0") &&
    scanGeneration === (request.scanGeneration ?? "0") &&
    Number(requestGeneration) > 0;
  if (!commonIsExact) return null;
  if (status === "missing") {
    return request.posterPath === null &&
      posterPath === "" &&
      authorityId === "" &&
      source === "" &&
      Number(ratio) === 2 / 3
      ? { status: "missing" as const, authorityId: null }
      : null;
  }
  return status === "pending" &&
    request.posterPath !== null &&
    posterPath === request.posterPath &&
    authorityPattern.test(authorityId) &&
    Number.isFinite(Number(ratio)) &&
    Number(ratio) > 0 &&
    Number(ratio) <= 4 &&
    source === "TMDB"
    ? { status: "pending" as const, authorityId }
    : null;
}

export function tmdbCoverMime(bytes: Uint8Array) {
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
    String.fromCharCode(...bytes.slice(0, 4)) === "RIFF" &&
    String.fromCharCode(...bytes.slice(8, 12)) === "WEBP"
  ) {
    return "image/webp";
  }
  return null;
}

export function useTmdbCardCover(request: TmdbCardCoverRequest): TmdbCardCover {
  const {
    associationGeneration,
    category,
    contextGeneration,
    libraryItemId,
    posterPath,
    scanGeneration,
    surface,
    tmdbId,
  } = request;
  const [retryGeneration, setRetryGeneration] = useState(0);
  const [state, setState] = useState<TmdbCardCover>({
    status: request.posterPath === null ? "missing" : "loading",
    objectUrl: null,
    source: null,
    retry: null,
    reportDecodeSuccess: null,
    reportDecodeFailure: null,
  });
  const stableRequest = useMemo(
    () => ({
      associationGeneration,
      category,
      contextGeneration,
      libraryItemId,
      posterPath,
      scanGeneration,
      surface,
      tmdbId,
    }),
    [
      associationGeneration,
      category,
      contextGeneration,
      libraryItemId,
      posterPath,
      scanGeneration,
      surface,
      tmdbId,
    ],
  );
  const currentKey = useRef("");

  useEffect(() => {
    const requestGeneration = String(++nextRequestGeneration);
    const key = `${JSON.stringify(stableRequest)}:${retryGeneration}:${requestGeneration}`;
    currentKey.current = key;
    let objectUrl: string | null = null;
    let active = true;
    let decodePending = false;
    if (stableRequest.posterPath === null) {
      setState({
        status: "missing",
        objectUrl: null,
        source: null,
        retry: null,
        reportDecodeSuccess: null,
        reportDecodeFailure: null,
      });
      return () => {
        active = false;
      };
    }
    setState({
      status: "loading",
      objectUrl: null,
      source: null,
      retry: null,
      reportDecodeSuccess: null,
      reportDecodeFailure: null,
    });
    const args = requestArguments(stableRequest, requestGeneration);
    void scheduleLibraryPresentation(
      "cover",
      async () => {
        const response = await window.__TAURI__.core.invoke<unknown>(
          "resolve_tmdb_card_cover",
          args,
        );
        const parsed = parseTmdbCardCoverResponse(
          response,
          stableRequest,
          requestGeneration,
        );
        if (parsed?.status !== "pending") {
          throw new Error(parsed?.status === "missing" ? "missing" : "invalid");
        }
        const value = await window.__TAURI__.core.invoke<unknown>(
          "fetch_tmdb_card_cover",
          { ...args, coverAuthorityId: parsed.authorityId },
        );
        if (
          !Array.isArray(value) ||
          value.length < 64 ||
          value.length > maximumCoverBytes ||
          !value.every(
            (byte) =>
              typeof byte === "number" &&
              Number.isInteger(byte) &&
              byte >= 0 &&
              byte <= 255,
          )
        ) {
          throw new Error("invalid");
        }
        const bytes = Uint8Array.from(value);
        const mime = tmdbCoverMime(bytes);
        if (mime === null) throw new Error("invalid");
        objectUrl = URL.createObjectURL(new Blob([bytes], { type: mime }));
        if (!active || currentKey.current !== key) {
          URL.revokeObjectURL(objectUrl);
          objectUrl = null;
          return;
        }
        decodePending = true;
        const invalidateAfterDecodeFailure = async () => {
          if (!active || currentKey.current !== key || !decodePending) return;
          decodePending = false;
          if (objectUrl !== null) URL.revokeObjectURL(objectUrl);
          objectUrl = null;
          setState({
            status: "loading",
            objectUrl: null,
            source: null,
            retry: null,
            reportDecodeSuccess: null,
            reportDecodeFailure: null,
          });
          try {
            await window.__TAURI__.core.invoke(
              "invalidate_tmdb_card_cover",
              args,
            );
            if (!active || currentKey.current !== key) return;
            setState({
              status: "unavailable",
              objectUrl: null,
              source: null,
              retry: () => setRetryGeneration((generation) => generation + 1),
              reportDecodeSuccess: null,
              reportDecodeFailure: null,
            });
          } catch {
            if (!active || currentKey.current !== key) return;
            setState({
              status: "unavailable",
              objectUrl: null,
              source: null,
              retry: null,
              reportDecodeSuccess: null,
              reportDecodeFailure: null,
            });
          }
        };
        const reportDecodeSuccess = async () => {
          if (!active || currentKey.current !== key || !decodePending) return;
          decodePending = false;
          try {
            await window.__TAURI__.core.invoke("confirm_tmdb_card_cover", {
              ...args,
              coverAuthorityId: parsed.authorityId,
            });
            if (!active || currentKey.current !== key || objectUrl === null)
              return;
            setState({
              status: "ready",
              objectUrl,
              source: "TMDB",
              retry: null,
              reportDecodeSuccess: null,
              reportDecodeFailure: null,
            });
          } catch {
            decodePending = true;
            await invalidateAfterDecodeFailure();
          }
        };
        setState({
          status: "loading",
          objectUrl,
          source: null,
          retry: null,
          reportDecodeSuccess,
          reportDecodeFailure: () => {
            void invalidateAfterDecodeFailure();
          },
        });
      },
      () => active && currentKey.current === key,
    ).catch((error: unknown) => {
      if (!active || currentKey.current !== key) return;
      setState({
        status:
          error instanceof Error && error.message === "missing"
            ? "missing"
            : "unavailable",
        objectUrl: null,
        source: null,
        retry:
          error instanceof Error && error.message === "missing"
            ? null
            : () => setRetryGeneration((generation) => generation + 1),
        reportDecodeSuccess: null,
        reportDecodeFailure: null,
      });
    });
    return () => {
      active = false;
      if (objectUrl !== null) URL.revokeObjectURL(objectUrl);
      void window.__TAURI__.core.invoke("cancel_tmdb_card_cover", args);
    };
  }, [retryGeneration, stableRequest]);

  return state;
}
