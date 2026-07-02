import { describe, it, expect } from "vitest";
import { extractLocalImagePaths, fileNameFromPath } from "$lib/generated-images";

describe("extractLocalImagePaths", () => {
  it("vytáhne lokální cesty z markdownu, vzdálené URL vynechá", () => {
    const content =
      "Tady je obrázek: ![obrázek](C:/data/weave/gallery/weave_001.png) " +
      "a odkaz ![web](https://example.com/foto.png)";
    expect(extractLocalImagePaths(content)).toEqual(["C:/data/weave/gallery/weave_001.png"]);
  });

  it("deduplikuje a vrací prázdné pole bez obrázků", () => {
    expect(extractLocalImagePaths("žádný obrázek")).toEqual([]);
    const twice = "![a](/g/x.png) ![b](/g/x.png)";
    expect(extractLocalImagePaths(twice)).toEqual(["/g/x.png"]);
  });
});

describe("fileNameFromPath", () => {
  it("zvládne Windows i Unix cesty", () => {
    expect(fileNameFromPath("C:\\data\\gallery\\img.png")).toBe("img.png");
    expect(fileNameFromPath("/home/user/img.png")).toBe("img.png");
  });
});
