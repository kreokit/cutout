import { create } from "zustand";
import { BRUSH_DEFAULT, BRUSH_MAX, BRUSH_MIN, stepBrushSize } from "../lib/brush";

export type Tool = "hand" | "erase" | "restore";

interface BrushStore {
  tool: Tool;
  /** Diameter in screen pixels, so zooming in gives a finer brush. */
  size: number;
  /** 0 = hard, 1 = soft. */
  softness: number;
  setTool: (tool: Tool) => void;
  setSize: (size: number) => void;
  setSoftness: (softness: number) => void;
  /** `[` / `]` change the size; with Shift, the softness. */
  step: (direction: 1 | -1, softness: boolean) => void;
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

export const useBrush = create<BrushStore>((set, get) => ({
  tool: "hand",
  size: BRUSH_DEFAULT,
  softness: 0.5,
  setTool: (tool) => set({ tool }),
  setSize: (size) => set({ size: clamp(Math.round(size), BRUSH_MIN, BRUSH_MAX) }),
  setSoftness: (softness) => set({ softness: clamp(softness, 0, 1) }),
  step: (direction, softness) => {
    const s = get();
    if (softness) set({ softness: clamp(Math.round((s.softness + direction * 0.1) * 10) / 10, 0, 1) });
    else set({ size: stepBrushSize(s.size, direction) });
  },
}));
