import { create } from "zustand";
import { Background, DEFAULT_BLUR, DEFAULT_COLOR, TRANSPARENT, sameBackground } from "../lib/background";
import { EdgeSettings, Edits, NO_EDITS, Stroke } from "../lib/refine";

/** A past or undone state, numbered so steps of different histories can be ordered. */
export interface Step<T> {
  value: T;
  seq: number;
}

/** The edits of one image with their history. */
export interface History {
  edits: Edits;
  past: Step<Edits>[];
  future: Step<Edits>[];
}

/** The shared background with its history. */
export interface BackgroundHistory {
  value: Background;
  past: Step<Background>[];
  future: Step<Background>[];
}

/** The last colour, blur and image picked, offered again when switching back. */
export interface BackgroundChoices {
  color: string;
  strength: number;
  path: string | null;
}

const EMPTY: History = { edits: NO_EDITS, past: [], future: [] };

/** How many steps undo remembers per image (and for the background). */
export const UNDO_LIMIT = 100;

interface EditsStore {
  /** History per `cutoutKey(path, model)`. Kept for the session only. */
  byImage: Record<string, History>;
  /** Edge settings while a slider is being dragged (not in the history yet). */
  live: Record<string, EdgeSettings | undefined>;
  /** The background of all open images. */
  background: BackgroundHistory;
  /** The background while a slider or colour is being dragged. */
  liveBackground: Background | undefined;
  choices: BackgroundChoices;
  /** Numbers the steps of all histories. */
  seq: number;
  /** Shows `edges` live while a slider moves. */
  previewEdges: (key: string, edges: EdgeSettings) => void;
  /** Makes `edges` part of the edits: one undo step. */
  commitEdges: (key: string, edges: EdgeSettings) => void;
  addStroke: (key: string, stroke: Stroke) => void;
  previewBackground: (background: Background) => void;
  /** Sets the background (one undo step), while image `key` is shown. */
  commitBackground: (key: string, background: Background) => void;
  /** Undoes the newest step of image `key` or of the background. */
  undo: (key: string) => void;
  redo: (key: string) => void;
}

export const historyOf = (byImage: Record<string, History>, key: string) => byImage[key] ?? EMPTY;

const sameEdges = (a: EdgeSettings, b: EdgeSettings) =>
  a.feather === b.feather && a.shift === b.shift && a.decontaminate === b.decontaminate;

const remember = (choices: BackgroundChoices, b: Background): BackgroundChoices => {
  switch (b.kind) {
    case "color":
      return { ...choices, color: b.color };
    case "blur":
      return { ...choices, strength: b.strength };
    case "image":
      return { ...choices, path: b.path };
    default:
      return choices;
  }
};

export const INITIAL_BACKGROUND: BackgroundHistory = { value: TRANSPARENT, past: [], future: [] };
export const INITIAL_CHOICES: BackgroundChoices = { color: DEFAULT_COLOR, strength: DEFAULT_BLUR, path: null };

/** Whether undo / redo has anything to do for image `key`. */
export const canUndo = (s: EditsStore, key: string) =>
  historyOf(s.byImage, key).past.length > 0 || s.background.past.length > 0;
export const canRedo = (s: EditsStore, key: string) =>
  historyOf(s.byImage, key).future.length > 0 || s.background.future.length > 0;

export const useEdits = create<EditsStore>((set, get) => {
  /**
   * Applies a change to image `key` as a new step: the old edits go to the past, and the
   * redo steps (of the image and the background) are cleared.
   */
  const step = (key: string, change: (e: Edits) => Edits) => {
    const s = get();
    const h = historyOf(s.byImage, key);
    const next = change(h.edits);
    set({
      byImage: {
        ...s.byImage,
        [key]: { edits: next, past: [...h.past, { value: h.edits, seq: s.seq }].slice(-UNDO_LIMIT), future: [] },
      },
      live: { ...s.live, [key]: undefined },
      background: { ...s.background, future: [] },
      seq: s.seq + 1,
    });
  };
  const top = <T,>(steps: Step<T>[]) => steps[steps.length - 1]?.seq ?? -1;

  return {
    byImage: {},
    live: {},
    background: INITIAL_BACKGROUND,
    liveBackground: undefined,
    choices: INITIAL_CHOICES,
    seq: 0,

    previewEdges: (key, edges) => set((s) => ({ live: { ...s.live, [key]: edges } })),

    commitEdges: (key, edges) => {
      if (sameEdges(historyOf(get().byImage, key).edits.edges, edges)) {
        set((s) => ({ live: { ...s.live, [key]: undefined } }));
        return;
      }
      step(key, (e) => ({ ...e, edges }));
    },

    addStroke: (key, stroke) => step(key, (e) => ({ ...e, strokes: [...e.strokes, stroke] })),

    previewBackground: (background) => set({ liveBackground: background }),

    commitBackground: (key, background) => {
      const s = get();
      const b = s.background;
      if (sameBackground(b.value, background)) {
        set({ liveBackground: undefined });
        return;
      }
      const h = historyOf(s.byImage, key);
      set({
        background: {
          value: background,
          past: [...b.past, { value: b.value, seq: s.seq }].slice(-UNDO_LIMIT),
          future: [],
        },
        liveBackground: undefined,
        choices: remember(s.choices, background),
        byImage: h.future.length ? { ...s.byImage, [key]: { ...h, future: [] } } : s.byImage,
        seq: s.seq + 1,
      });
    },

    undo: (key) => {
      const s = get();
      const h = historyOf(s.byImage, key);
      const b = s.background;
      if (top(h.past) < 0 && top(b.past) < 0) return;
      if (top(b.past) > top(h.past)) {
        const previous = b.past[b.past.length - 1];
        set({
          background: {
            value: previous.value,
            past: b.past.slice(0, -1),
            future: [{ value: b.value, seq: previous.seq }, ...b.future],
          },
          liveBackground: undefined,
          choices: remember(s.choices, previous.value),
        });
      } else {
        const previous = h.past[h.past.length - 1];
        set({
          byImage: {
            ...s.byImage,
            [key]: { edits: previous.value, past: h.past.slice(0, -1), future: [{ value: h.edits, seq: previous.seq }, ...h.future] },
          },
        });
      }
    },

    redo: (key) => {
      const s = get();
      const h = historyOf(s.byImage, key);
      const b = s.background;
      const first = <T,>(steps: Step<T>[]) => steps[0]?.seq ?? Infinity;
      if (!h.future.length && !b.future.length) return;
      // The last undone step has the lowest number.
      if (first(b.future) < first(h.future)) {
        const [next, ...rest] = b.future;
        set({
          background: { value: next.value, past: [...b.past, { value: b.value, seq: next.seq }], future: rest },
          choices: remember(s.choices, next.value),
        });
      } else {
        const [next, ...rest] = h.future;
        set({
          byImage: {
            ...s.byImage,
            [key]: { edits: next.value, past: [...h.past, { value: h.edits, seq: next.seq }], future: rest },
          },
        });
      }
    },
  };
});
