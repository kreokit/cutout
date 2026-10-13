import { describe, expect, it } from "vitest";
import { fitContain } from "./fit";

describe("fitContain", () => {
  it("shrinks a large image to the box, keeping the aspect ratio", () => {
    expect(fitContain({ width: 4000, height: 3000 }, { width: 800, height: 800 })).toEqual({
      width: 800,
      height: 600,
      scale: 0.2,
    });
    expect(fitContain({ width: 1080, height: 1920 }, { width: 1000, height: 480 }).height).toBe(480);
  });

  it("leaves room for the padding", () => {
    expect(fitContain({ width: 2000, height: 1000 }, { width: 1040, height: 1000 }, 20).width).toBe(1000);
  });

  it("never enlarges a small image", () => {
    expect(fitContain({ width: 200, height: 100 }, { width: 800, height: 600 })).toEqual({
      width: 200,
      height: 100,
      scale: 1,
    });
  });

  it("copes with an empty box or image", () => {
    expect(fitContain({ width: 200, height: 100 }, { width: 0, height: 0 }, 24)).toEqual({
      width: 0,
      height: 0,
      scale: 0,
    });
    expect(fitContain({ width: 0, height: 0 }, { width: 800, height: 600 }).scale).toBe(0);
  });
});
