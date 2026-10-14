import { describe, expect, it } from "vitest";
import { parseRender } from "./refine";

describe("parseRender", () => {
  it("reads the header and keeps the pixels without copying", () => {
    const buffer = new ArrayBuffer(24 + 8);
    const v = new DataView(buffer);
    [4032, 3024, 10, 20, 2, 1].forEach((n, i) => v.setUint32(i * 4, n, true));
    new Uint8Array(buffer, 24).set([1, 2, 3, 4, 5, 6, 7, 8]);
    const r = parseRender(buffer);
    expect([r.width, r.height, r.x, r.y, r.regionWidth, r.regionHeight]).toEqual([4032, 3024, 10, 20, 2, 1]);
    expect(Array.from(r.pixels)).toEqual([1, 2, 3, 4, 5, 6, 7, 8]);
    expect(r.pixels.buffer).toBe(buffer);
  });
});
