import { describe, expect, it } from "vitest";
import { splitAt, splitForKey } from "./compare";

describe("splitAt", () => {
  it("maps the pointer onto the image and clamps outside it", () => {
    expect(splitAt(150, 100, 200)).toBe(0.25);
    expect(splitAt(50, 100, 200)).toBe(0);
    expect(splitAt(400, 100, 200)).toBe(1);
    expect(splitAt(10, 0, 0)).toBe(0.5);
  });
});

describe("splitForKey", () => {
  it("steps with the arrows and jumps with Home/End", () => {
    expect(splitForKey(0.5, "ArrowRight", false)).toBeCloseTo(0.55);
    expect(splitForKey(0.5, "ArrowLeft", true)).toBeCloseTo(0.25);
    expect(splitForKey(0.02, "ArrowLeft", false)).toBe(0);
    expect(splitForKey(0.4, "End", false)).toBe(1);
    expect(splitForKey(0.4, "Home", false)).toBe(0);
    expect(splitForKey(0.4, "a", false)).toBeNull();
  });
});
