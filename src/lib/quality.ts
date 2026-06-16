/**
 * Coarse quality label from a release name (Python _quality): the first of
 * 2160p/4K/8K/1080p/720p/480p found, uppercased; "" when none present. Shared by
 * the javdb / sukebei / tpb / javbus scrapers so the resolution ladder lives in
 * one place.
 */
const RESOLUTION_TOKENS = ["2160p", "4k", "8k", "1080p", "720p", "480p"] as const

export function quality(name: string): string {
  const lower = (name || "").toLowerCase()
  for (const token of RESOLUTION_TOKENS) {
    if (lower.includes(token)) return token.toUpperCase()
  }
  return ""
}
