import { create } from "zustand";
import { EdgeSettings, Edits, NO_EDITS, Stroke } from "../lib/refine";

/** The edits of one image with their history. */
export interface History {
  edits: Edits;
  past: Edits[];
  future: Edits[];
}

const EMPTY: History = { edits: NO_EDITS, past: [], future: [] };

/** How many steps undo remembers per image. */
export const UNDO_LIMIT = 100;

interface EditsStore {
  /** History per `cutoutKey(path, model)`. Kept for the session only. */
  byImage: Record<string, History>;
  /** Edge settings while a slider is being dragged (not in the history yet). */
  live: Record<string, EdgeSettings | undefined>;
  /** Shows `edges` live while a slider moves. */
  previewEdges: (key: string, edges: EdgeSettings) => void;
  /** Makes `edges` part of the edits: one undo step. */
  commitEdges: (key: string, edges: EdgeSettings) => void;
  addStroke: (key: string, stroke: Stroke) => void;
  undo: (key: string) => void;
  redo: (key: string) => void;
}

export const historyOf = (byImage: Record<string, History>, key: string) => byImage[key] ?? EMPTY;

const sameEdges = (a: EdgeSettings, b: EdgeSettings) =>
  a.feather === b.feather && a.shift === b.shift && a.decontaminate === b.decontaminate;

export const useEdits = create<EditsStore>((set, get) => {
  /** Applies a change as a new step: the old edits go to the past, the future is cleared. */
  const step = (key: string, change: (e: Edits) => Edits) => {
    const h = historyOf(get().byImage, key);
    const next = change(h.edits);
    set((s) => ({
      byImage: { ...s.byImage, [key]: { edits: next, past: [...h.past, h.edits].slice(-UNDO_LIMIT), future: [] } },
      live: { ...s.live, [key]: undefined },
    }));
  };
  const setHistory = (key: string, h: History) => set((s) => ({ byImage: { ...s.byImage, [key]: h } }));

  return {
    byImage: {},
    live: {},

    previewEdges: (key, edges) => set((s) => ({ live: { ...s.live, [key]: edges } })),

    commitEdges: (key, edges) => {
      if (sameEdges(historyOf(get().byImage, key).edits.edges, edges)) {
        set((s) => ({ live: { ...s.live, [key]: undefined } }));
        return;
      }
      step(key, (e) => ({ ...e, edges }));
    },

    addStroke: (key, stroke) => step(key, (e) => ({ ...e, strokes: [...e.strokes, stroke] })),

    undo: (key) => {
      const h = historyOf(get().byImage, key);
      const previous = h.past[h.past.length - 1];
      if (!previous) return;
      setHistory(key, { edits: previous, past: h.past.slice(0, -1), future: [h.edits, ...h.future] });
    },

    redo: (key) => {
      const h = historyOf(get().byImage, key);
      const [next, ...rest] = h.future;
      if (!next) return;
      setHistory(key, { edits: next, past: [...h.past, h.edits], future: rest });
    },
  };
});
