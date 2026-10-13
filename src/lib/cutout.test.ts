import { describe, expect, it } from "vitest";
import { formatSeconds } from "./cutout";
import { percent } from "./models";

describe("formatSeconds", () => {
  it("shows tenths below 10 s", () => {
    expect(formatSeconds(0.84)).toBe("0.8 s");
    expect(formatSeconds(1.25)).toBe("1.3 s");
    expect(formatSeconds(14.4)).toBe("14 s");
  });
});

describe("percent", () => {
  it("rounds down and never passes 100", () => {
    expect(percent({ downloaded_bytes: 0, total_bytes: 176 })).toBe(0);
    expect(percent({ downloaded_bytes: 175, total_bytes: 176 })).toBe(99);
    expect(percent({ downloaded_bytes: 200, total_bytes: 176 })).toBe(100);
    expect(percent({ downloaded_bytes: 5, total_bytes: 0 })).toBe(0);
  });
});
