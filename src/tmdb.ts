export type TmdbMovie = {
  id: number;
  title: string;
  posterPath: string | null;
  releaseDate: string | null;
};

export type TmdbMoviesResult =
  | { status: "ready"; movies: TmdbMovie[] }
  | { status: "empty" }
  | { status: "unauthorized" }
  | { status: "rate-limited" }
  | { status: "network-error" }
  | { status: "malformed-provider" }
  | { status: "provider-error" };

export type TmdbMovieDetails = {
  id: number;
  title: string;
  posterPath: string | null;
  releaseDate: string | null;
  runtimeMinutes: number | null;
  genres: string[];
  overview: string | null;
};

export type TmdbMovieDetailsResult =
  | { status: "ready"; details: TmdbMovieDetails }
  | { status: "unauthorized" }
  | { status: "rate-limited" }
  | { status: "network-error" }
  | { status: "malformed-provider" }
  | { status: "provider-error" };

type TmdbRequestError =
  | { status: "unauthorized" }
  | { status: "rate-limited" }
  | { status: "network-error" }
  | { status: "provider-error" };

const weeklyTrendingMoviesUrl =
  "https://api.themoviedb.org/3/trending/movie/week";
const movieSearchUrl = "https://api.themoviedb.org/3/search/movie";
const movieDetailsBaseUrl = "https://api.themoviedb.org/3/movie";
// The documented w500 size is sufficient for this fixed two-column surface without another API call.
const posterBaseUrl = "https://image.tmdb.org/t/p/w500";
const releaseDatePattern = /^\d{4}-\d{2}-\d{2}$/;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function parseMovie(value: unknown): TmdbMovie | null {
  if (!isRecord(value)) {
    return null;
  }

  const { id, poster_path: posterPath, release_date: releaseDate, title } =
    value;
  if (
    typeof id !== "number" ||
    !Number.isInteger(id) ||
    id <= 0 ||
    typeof title !== "string" ||
    title.trim() === ""
  ) {
    return null;
  }

  return {
    id,
    title,
    posterPath:
      typeof posterPath === "string" && posterPath.startsWith("/")
        ? posterPath
        : null,
    releaseDate:
      typeof releaseDate === "string" && releaseDatePattern.test(releaseDate)
        ? releaseDate
        : null,
  };
}

function parseMoviesResponse(
  value: unknown,
  malformedResponseStatus: "malformed-provider" | "provider-error",
): TmdbMoviesResult {
  if (!isRecord(value) || !Array.isArray(value.results)) {
    return { status: malformedResponseStatus };
  }

  const movies = value.results.flatMap((result) => {
    const movie = parseMovie(result);
    return movie === null ? [] : [movie];
  });

  return movies.length === 0
    ? { status: "empty" }
    : { status: "ready", movies };
}

function parseMovieDetailsResponse(
  value: unknown,
  requestedMovieId: number,
): TmdbMovieDetailsResult {
  if (
    !isRecord(value) ||
    typeof value.id !== "number" ||
    !Number.isInteger(value.id) ||
    value.id <= 0 ||
    value.id !== requestedMovieId ||
    typeof value.title !== "string" ||
    value.title.trim() === ""
  ) {
    return { status: "malformed-provider" };
  }

  const genres = Array.isArray(value.genres)
    ? value.genres.flatMap((genre) =>
        isRecord(genre) &&
        typeof genre.name === "string" &&
        genre.name.trim() !== ""
          ? [genre.name]
          : [],
      )
    : [];

  return {
    status: "ready",
    details: {
      id: value.id,
      title: value.title,
      posterPath:
        typeof value.poster_path === "string" &&
        value.poster_path.startsWith("/")
          ? value.poster_path
          : null,
      releaseDate:
        typeof value.release_date === "string" &&
        releaseDatePattern.test(value.release_date)
          ? value.release_date
          : null,
      runtimeMinutes:
        typeof value.runtime === "number" &&
        Number.isInteger(value.runtime) &&
        value.runtime > 0
          ? value.runtime
          : null,
      genres,
      overview:
        typeof value.overview === "string" && value.overview.trim() !== ""
          ? value.overview
          : null,
    },
  };
}

export function tmdbPosterUrl(posterPath: string) {
  return `${posterBaseUrl}${posterPath}`;
}

async function fetchTmdbResource<T>(
  url: string,
  token: string,
  parseResponse: (value: unknown) => T,
  malformedResponseStatus: "malformed-provider" | "provider-error",
  signal?: AbortSignal,
): Promise<T | TmdbRequestError | { status: "malformed-provider" }> {
  let response: Response;

  try {
    response = await fetch(url, {
      headers: {
        Accept: "application/json",
        Authorization: `Bearer ${token}`,
      },
      signal,
    });
  } catch {
    return { status: "network-error" };
  }

  if (response.status === 401 || response.status === 403) {
    return { status: "unauthorized" };
  }
  if (response.status === 429) {
    return { status: "rate-limited" };
  }
  if (!response.ok) {
    return { status: "provider-error" };
  }

  try {
    return parseResponse((await response.json()) as unknown);
  } catch {
    return { status: malformedResponseStatus };
  }
}

function fetchTmdbMovies(
  url: string,
  token: string,
  malformedResponseStatus: "malformed-provider" | "provider-error",
  signal?: AbortSignal,
): Promise<TmdbMoviesResult> {
  return fetchTmdbResource(
    url,
    token,
    (value) => parseMoviesResponse(value, malformedResponseStatus),
    malformedResponseStatus,
    signal,
  );
}

export function fetchWeeklyTrendingMovies(
  token: string,
  signal?: AbortSignal,
) {
  return fetchTmdbMovies(
    weeklyTrendingMoviesUrl,
    token,
    "provider-error",
    signal,
  );
}

export function fetchTmdbMoviesByTitle(
  token: string,
  query: string,
  signal?: AbortSignal,
) {
  if (query.trim() === "") {
    throw new Error("A TMDB Movies search query is required.");
  }

  const url = new URL(movieSearchUrl);
  url.searchParams.set("query", query);
  return fetchTmdbMovies(url.toString(), token, "malformed-provider", signal);
}

export function fetchTmdbMovieDetails(
  token: string,
  movieId: number,
  signal?: AbortSignal,
): Promise<TmdbMovieDetailsResult> {
  if (!Number.isInteger(movieId) || movieId <= 0) {
    throw new Error("A valid TMDB Movie ID is required.");
  }

  return fetchTmdbResource(
    `${movieDetailsBaseUrl}/${movieId}`,
    token,
    (value) => parseMovieDetailsResponse(value, movieId),
    "malformed-provider",
    signal,
  );
}
