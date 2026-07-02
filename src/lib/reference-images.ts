/** Přípony obrázků podporované jako reference pro generování. */
export const IMAGE_EXTENSIONS = ["png", "jpg", "jpeg", "webp", "gif", "bmp"];

export function isImagePath(path: string): boolean {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  return IMAGE_EXTENSIONS.includes(ext);
}

/**
 * Z cest (např. z drag & drop) vybere obrázky, které ještě nejsou přiložené.
 * Neobrázkové soubory tiše ignoruje.
 */
export function filterNewImagePaths(paths: string[], existing: string[]): string[] {
  const seen = new Set(existing);
  const result: string[] = [];
  for (const path of paths) {
    if (isImagePath(path) && !seen.has(path)) {
      seen.add(path);
      result.push(path);
    }
  }
  return result;
}
