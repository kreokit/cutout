import { describe, expect, it } from "vitest";
import cases from "../../fixtures/brush-cases.json";
import { BRUSH_MAX, BRUSH_MIN, brushAlpha, segmentDistance, stepBrushSize } from "./brush";

describe("brushAlpha", () => {
  it("matches the Rust brush (shared fixture)", () => {
    expect(cases.length).toBeGreaterThanOrEqual(10);
    for (const c of cases) {
      expect(brushAlpha(c.distance, c.radius, c.softness)).toBeCloseTo(c.alpha, 4);
    }
  });
});

describe("segmentDistance", () => {
  it("measures to the nearest point of the segment, ends included", () => {
    expect(segmentDistance(5, 3, 0, 0, 10, 0)).toBe(3);
    expect(segmentDistance(-4, 3, 0, 0, 10, 0)).toBe(5);
    expect(segmentDistance(1, 1, 1, 1, 1, 1)).toBe(0);
  });
});

describe("stepBrushSize", () => {
  it("steps by about a fifth, within the limits", () => {
    expect(stepBrushSize(40, 1)).toBe(50);
    expect(stepBrushSize(40, -1)).toBe(32);
    expect(stepBrushSize(4, 1)).toBe(5);
    expect(stepBrushSize(BRUSH_MAX, 1)).toBe(BRUSH_MAX);
    expect(stepBrushSize(BRUSH_MIN, -1)).toBe(BRUSH_MIN);
  });
});
