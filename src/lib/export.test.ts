import { describe, expect, it } from "vitest";
import { DEFAULT_EXPORT, SIZES, batchSummary, exportPath, sanitizeSettings, sizeId } from "./export";

describe("export", () => {
  it("names the file after the image, next to it", () => {
    expect(exportPath("/Users/me/Photos/cat.jpg", "png")).toBe("/Users/me/Photos/cat-cutout.png");
    expect(exportPath("C:\\Pics\\my.dog.webp", "jpg")).toBe("C:\\Pics\\my.dog-cutout.jpg");
    expect(exportPath("/tmp/.hidden", "webp")).toBe("/tmp/.hidden-cutout.webp");
    expect(exportPath("noext", "png")).toBe("noext-cutout.png");
  });

  it("offers the original size and the four social presets", () => {
    expect(SIZES.map((s) => s.size && `${s.size.width}×${s.size.height}`)).toEqual([
      null,
      "1080×1080",
      "1080×1350",
      "1080×1920",
      "1280×720",
    ]);
    expect(sizeId(null)).toBe("original");
    expect(sizeId({ width: 1080, height: 1920 })).toBe("story");
  });

  it("keeps only valid stored settings", () => {
    expect(sanitizeSettings(undefined)).toEqual(DEFAULT_EXPORT);
    expect(sanitizeSettings({ format: "jpg", quality: 75, size: { width: 1280, height: 720 }, fit: "fill" })).toEqual({
      format: "jpg",
      quality: 75,
      size: { width: 1280, height: 720 },
      fit: "fill",
    });
    expect(sanitizeSettings({ format: "gif", quality: 500, size: { width: 7, height: 7 }, fit: "zoom" })).toEqual({
      ...DEFAULT_EXPORT,
      quality: 100,
    });
    expect(sanitizeSettings({ quality: Number.NaN }).quality).toBe(DEFAULT_EXPORT.quality);
  });

  it("summarises a batch export", () => {
    const saved = { status: "saved" as const, path: "/a.png", dest: "/out/a-cutout.png" };
    expect(batchSummary([saved, saved])).toBe("2 exported");
    expect(batchSummary([saved, { status: "failed", path: "/b.png", error: "x" }])).toBe("1 exported, 1 failed");
    expect(batchSummary([saved, { status: "skipped", path: "/c.png" }])).toBe("1 exported, 1 not exported (stopped)");
  });
});
