import { describe, expect, it } from "vitest";
import { centeredView, clampView, fitView, panBy, stepScale, toImage, wheelAction, zoomBy, zoomTo } from "./view";

const image = { width: 4000, height: 3000 };
const box = { width: 1000, height: 800 };

describe("view", () => {
  it("fits and centres, never enlarging", () => {
    const v = fitView(image, box, 20);
    expect(v.scale).toBeCloseTo(0.24);
    expect(v.x).toBeCloseTo(20);
    expect(v.y).toBeCloseTo((800 - 720) / 2);
    expect(fitView({ width: 100, height: 50 }, box, 20)).toEqual({ scale: 1, x: 450, y: 375 });
  });

  it("centres a given scale", () => {
    expect(centeredView(image, box, 1)).toEqual({ scale: 1, x: -1500, y: -1100 });
  });

  it("zooms around a point, keeping it in place", () => {
    const v = { scale: 0.5, x: 100, y: 50 };
    const at = { x: 300, y: 250 };
    const before = toImage(v, at);
    const z = zoomBy(v, 2, at);
    expect(z.scale).toBe(1);
    expect(toImage(z, at)).toEqual(before);
    expect(zoomTo(v, 1000, at).scale).toBe(32);
    expect(zoomTo(v, 0, at).scale).toBe(0.02);
  });

  it("steps through zoom levels", () => {
    expect(stepScale(0.24, 1)).toBe(0.25);
    expect(stepScale(0.25, 1)).toBe(0.33);
    expect(stepScale(1, -1)).toBe(0.67);
    expect(stepScale(32, 1)).toBe(32);
    expect(stepScale(0.05, -1)).toBe(0.02);
  });

  it("pans and keeps some of the image on screen", () => {
    const v = panBy({ scale: 1, x: 0, y: 0 }, 30, -10);
    expect(v).toEqual({ scale: 1, x: 30, y: -10 });
    const lost = clampView({ scale: 1, x: 5000, y: -9000 }, image, box);
    expect(lost.x).toBe(1000 - 64);
    expect(lost.y).toBe(64 - 3000);
  });
});

describe("wheelAction (Figma style)", () => {
  const wheel = (o: Partial<Parameters<typeof wheelAction>[0]>) =>
    wheelAction({ deltaX: 0, deltaY: 0, deltaMode: 0, ctrlKey: false, metaKey: false, ...o });

  it("pans on plain scroll, in lines or pixels", () => {
    expect(wheel({ deltaX: 5, deltaY: 12 })).toEqual({ kind: "pan", dx: -5, dy: -12 });
    expect(wheel({ deltaY: 3, deltaMode: 1 })).toEqual({ kind: "pan", dx: -0, dy: -48 });
  });

  it("zooms with ⌘ or Ctrl: scrolling up zooms in", () => {
    const up = wheel({ deltaY: -100, metaKey: true });
    expect(up.kind).toBe("zoom");
    expect(up.kind === "zoom" && up.factor).toBeCloseTo(Math.E);
    const down = wheel({ deltaY: 10, ctrlKey: true });
    expect(down.kind === "zoom" && down.factor).toBeLessThan(1);
  });
});
