/** Vytáhne lokální cesty obrázků z markdownu zprávy (`![alt](cesta)`).
 *  Vzdálené URL (http/https) vynechává — ukládat/referencovat jde jen lokální. */
export function extractLocalImagePaths(content: string): string[] {
  const paths: string[] = [];
  const regex = /!\[[^\]]*\]\(([^)]+)\)/g;
  let match: RegExpExecArray | null;
  while ((match = regex.exec(content)) !== null) {
    const path = match[1];
    if (!/^https?:\/\//.test(path) && !paths.includes(path)) {
      paths.push(path);
    }
  }
  return paths;
}

/** Název souboru z cesty (Windows i Unix oddělovače). */
export function fileNameFromPath(path: string): string {
  return path.split(/[\\/]/).pop() ?? "obrazek.png";
}
