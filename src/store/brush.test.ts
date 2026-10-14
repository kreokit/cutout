import { beforeEach, describe, expect, it } from "vitest";
import { useBrush } from "./brush";

describe("brush store", () => {
  beforeEach(() => useBrush.setState({ tool: "hand", size: 40, softness: 0.5 }));

  it("steps size and softness within limits", () => {
    const { step } = useBrush.getState();
    step(1, false);
    expect(useBrush.getState().size).toBe(50);
    step(1, true);
    step(1, true);
    expect(useBrush.getState().softness).toBe(0.7);
    for (let i = 0; i < 20; i++) step(-1, true);
    expect(useBrush.getState().softness).toBe(0);
    useBrush.getState().setSize(9999);
    expect(useBrush.getState().size).toBe(300);
  });
});
