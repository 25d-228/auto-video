export type TmdbMovie = {
  id: number;
  title: string;
  posterPath: string | null;
  releaseDate: string | null;
};

export type TmdbTrendingResult =
  | { status: "ready"; movies: TmdbMovie[] }
  | { status: "empty" }
  | { status: "unauthorized" }
  | { status: "rate-limited" }
  | { status: "network-error" }
  | { status: "provider-error" };

const weeklyTrendingMoviesUrl =
  "https://api.themoviedb.org/3/trending/movie/week";
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

function parseTrendingResponse(value: unknown): TmdbTrendingResult {
  if (!isRecord(value) || !Array.isArray(value.results)) {
    return { status: "provider-error" };
  }

  const movies = value.results.flatMap((result) => {
    const movie = parseMovie(result);
    return movie === null ? [] : [movie];
  });

  return movies.length === 0
    ? { status: "empty" }
    : { status: "ready", movies };
}

export function tmdbPosterUrl(posterPath: string) {
  return `${posterBaseUrl}${posterPath}`;
}

export async function fetchWeeklyTrendingMovies(
  token: string,
  signal?: AbortSignal,
): Promise<TmdbTrendingResult> {
  let response: Response;

  try {
    response = await fetch(weeklyTrendingMoviesUrl, {
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
    return parseTrendingResponse((await response.json()) as unknown);
  } catch {
    return { status: "provider-error" };
  }
}
