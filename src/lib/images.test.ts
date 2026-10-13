import { describe, expect, it } from "vitest";
import { fileName, formatBytes, isImagePath } from "./images";

describe("isImagePath", () => {
  it("accepts PNG, JPG and WebP in any case", () => {
    expect(isImagePath("/a/photo.PNG")).toBe(true);
    expect(isImagePath("C:\\pics\\cat.Jpeg")).toBe(true);
    expect(isImagePath("/a/b.c/shot.webp")).toBe(true);
  });

  it("rejects other files and names without an extension", () => {
    expect(isImagePath("/a/clip.mp4")).toBe(false);
    expect(isImagePath("/a/anim.gif")).toBe(false);
    expect(isImagePath("/a.png/README")).toBe(false);
    expect(isImagePath("/a/png")).toBe(false);
  });
});

describe("fileName", () => {
  it("handles macOS and Windows paths", () => {
    expect(fileName("/Users/me/cat.png")).toBe("cat.png");
    expect(fileName("C:\\Users\\me\\dog.jpg")).toBe("dog.jpg");
  });
});

describe("formatBytes", () => {
  it("uses B, KB and MB", () => {
    expect(formatBytes(980)).toBe("980 B");
    expect(formatBytes(48_200)).toBe("48 KB");
    expect(formatBytes(3_420_000)).toBe("3.4 MB");
  });
});
