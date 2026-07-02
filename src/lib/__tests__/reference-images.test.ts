import { describe, it, expect } from "vitest";
import { filterNewImagePaths, isImagePath } from "$lib/reference-images";

describe("isImagePath", () => {
  it("pozná podporované přípony bez ohledu na velikost písmen", () => {
    expect(isImagePath("C:\\fotky\\selfie.PNG")).toBe(true);
    expect(isImagePath("/home/user/foto.jpeg")).toBe(true);
    expect(isImagePath("obrazek.webp")).toBe(true);
  });

  it("odmítne neobrázkové soubory", () => {
    expect(isImagePath("dokument.pdf")).toBe(false);
    expect(isImagePath("video.mp4")).toBe(false);
    expect(isImagePath("bez_pripony")).toBe(false);
  });
});

describe("filterNewImagePaths", () => {
  it("vybere jen nové obrázky, neobrázky ignoruje", () => {
    const dropped = ["a.png", "poznamky.txt", "b.jpg", "a.png"];
    expect(filterNewImagePaths(dropped, [])).toEqual(["a.png", "b.jpg"]);
  });

  it("přeskočí už přiložené cesty", () => {
    expect(filterNewImagePaths(["a.png", "b.png"], ["a.png"])).toEqual(["b.png"]);
  });
});
