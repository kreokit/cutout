import { describe, expect, it } from "vitest";
import { relativeTime } from "./recent";

// A fixed "now": Monday 12 October 2026, 15:00 local time.
const now = new Date(2026, 9, 12, 15, 0).getTime();
const at = (...args: [number, number, number, number?, number?]) => new Date(...args).getTime();

describe("relativeTime", () => {
  it("counts minutes and hours today", () => {
    expect(relativeTime(now - 20_000, now)).toBe("just now");
    expect(relativeTime(now - 5 * 60_000, now)).toBe("5 min ago");
    expect(relativeTime(at(2026, 9, 12, 9, 30), now)).toBe("5 h ago");
  });

  it("names yesterday and the last week's days", () => {
    expect(relativeTime(at(2026, 9, 11, 23, 0), now)).toBe("yesterday");
    expect(relativeTime(at(2026, 9, 8, 12, 0), now)).toBe("Thursday");
  });

  it("shows a date for older images, with the year when it differs", () => {
    expect(relativeTime(at(2026, 8, 1), now)).toBe("1 Sept");
    expect(relativeTime(at(2025, 11, 24), now)).toBe("24 Dec 2025");
  });
});
