import { describe, expect, it } from "vitest";
import { Tile, intersect, stampSegment } from "./paint";

/** A 4×1 tile of a half-transparent red cutout over an original of opaque blue. */
function setup() {
  const snap = new Uint8ClampedArray([200, 0, 0, 128, 200, 0, 0, 128, 200, 0, 0, 0, 200, 0, 0, 255]);
  const tile: Tile = { x: 0, y: 0, width: 4, height: 1, snap, coverage: new Float32Array(4), out: new Uint8ClampedArray(snap) };
  const original = new Uint8ClampedArray([0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255]);
  return { tile, original };
}
const all = { x: 0, y: 0, width: 4, height: 1 };

describe("stampSegment", () => {
  it("erases towards transparent and keeps the colour", () => {
    const { tile, original } = setup();
    // A hard brush of radius 10 covers every pixel fully.
    stampSegment(tile, all, [2, 0.5], [2, 0.5], { mode: "erase", radius: 10, softness: 0 }, original, 4);
    expect(Array.from(tile.out.slice(0, 4))).toEqual([200, 0, 0, 0]);
    expect(tile.out[15]).toBe(0);
  });

  it("restores the original, opaque", () => {
    const { tile, original } = setup();
    stampSegment(tile, all, [2, 0.5], [2, 0.5], { mode: "restore", radius: 10, softness: 0 }, original, 4);
    expect(Array.from(tile.out.slice(0, 4))).toEqual([0, 0, 255, 255]);
    expect(Array.from(tile.out.slice(8, 12))).toEqual([0, 0, 255, 255]); // was fully transparent
  });

  it("partial coverage blends, and overlapping segments don't build up", () => {
    const { tile, original } = setup();
    // Fully soft brush, radius 4: the pixel centre at x = 0.5 is 2 px away → coverage 0.5.
    const brush = { mode: "erase" as const, radius: 4, softness: 1 };
    stampSegment(tile, all, [2.5, 0.5], [2.5, 0.5], brush, original, 4);
    expect(tile.out[3]).toBe(64);
    stampSegment(tile, all, [2.5, 0.5], [2.5, 0.5], brush, original, 4);
    expect(tile.out[3]).toBe(64);
  });

  it("only touches the given region", () => {
    const { tile, original } = setup();
    stampSegment(tile, { x: 1, y: 0, width: 1, height: 1 }, [2, 0.5], [2, 0.5], { mode: "erase", radius: 10, softness: 0 }, original, 4);
    expect(tile.out[3]).toBe(128);
    expect(tile.out[7]).toBe(0);
  });
});

describe("intersect", () => {
  it("clips rectangles", () => {
    expect(intersect({ x: 0, y: 0, width: 10, height: 10 }, { x: 5, y: 8, width: 10, height: 10 })).toEqual({ x: 5, y: 8, width: 5, height: 2 });
    expect(intersect({ x: 0, y: 0, width: 2, height: 2 }, { x: 3, y: 3, width: 1, height: 1 })).toBeNull();
  });
});
