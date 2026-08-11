const supportedLibraryExtensions = new Set([
  "mkv",
  "mp4",
  "avi",
  "wmv",
  "m4v",
  "ts",
  "mov",
  "flv",
  "iso",
  "rmvb",
  "webm",
  "mpg",
  "mpeg",
]);

export function hasSupportedLibraryExtension(path: string) {
  const filename = path.split(/[/\\]/).at(-1) ?? "";
  const extensionIndex = filename.lastIndexOf(".");
  return (
    extensionIndex > 0 &&
    supportedLibraryExtensions.has(filename.slice(extensionIndex + 1).toLowerCase())
  );
}
