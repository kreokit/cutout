import { beforeEach, describe, expect, it } from "vitest";
import { DEFAULT_EDGES, Stroke } from "../lib/refine";
import { INITIAL_BACKGROUND, INITIAL_CHOICES, UNDO_LIMIT, canRedo, canUndo, historyOf, useEdits } from "./edits";

const K = "fast:/a.png";
const stroke = (x: number): Stroke => ({ mode: "erase", radius: 10, softness: 0.5, points: [[x, x]] });
const edits = () => historyOf(useEdits.getState().byImage, K).edits;

describe("edits store", () => {
  beforeEach(() =>
    useEdits.setState({
      byImage: {},
      live: {},
      background: INITIAL_BACKGROUND,
      liveBackground: undefined,
      choices: INITIAL_CHOICES,
      seq: 0,
    }),
  );

  it("starts with no edits and fringe removal on", () => {
    expect(edits()).toEqual({ edges: DEFAULT_EDGES, strokes: [] });
    expect(DEFAULT_EDGES.decontaminate).toBe(true);
  });

  it("makes one undo step per committed slider drag, not per tick", () => {
    const { previewEdges, commitEdges, undo } = useEdits.getState();
    for (const f of [1, 2, 3, 4]) previewEdges(K, { ...DEFAULT_EDGES, feather: f });
    expect(useEdits.getState().live[K]?.feather).toBe(4);
    expect(edits().edges.feather).toBe(0);
    commitEdges(K, { ...DEFAULT_EDGES, feather: 4 });
    expect(useEdits.getState().live[K]).toBeUndefined();
    expect(edits().edges.feather).toBe(4);
    undo(K);
    expect(edits().edges.feather).toBe(0);
    expect(historyOf(useEdits.getState().byImage, K).past).toHaveLength(0);
  });

  it("ignores a drag that ends where it started", () => {
    useEdits.getState().previewEdges(K, { ...DEFAULT_EDGES, shift: 2 });
    useEdits.getState().commitEdges(K, DEFAULT_EDGES);
    expect(historyOf(useEdits.getState().byImage, K).past).toHaveLength(0);
    expect(useEdits.getState().live[K]).toBeUndefined();
  });

  it("undoes and redoes strokes and edge changes in order", () => {
    const { addStroke, commitEdges, undo, redo } = useEdits.getState();
    addStroke(K, stroke(1));
    commitEdges(K, { ...DEFAULT_EDGES, decontaminate: false });
    addStroke(K, stroke(2));
    expect(edits().strokes).toHaveLength(2);
    undo(K);
    expect(edits().strokes).toHaveLength(1);
    expect(edits().edges.decontaminate).toBe(false);
    undo(K);
    expect(edits().edges.decontaminate).toBe(true);
    redo(K);
    redo(K);
    expect(edits().strokes).toHaveLength(2);
    redo(K); // nothing left
    expect(edits().strokes).toHaveLength(2);
    // A new edit after undo clears the redo steps.
    undo(K);
    addStroke(K, stroke(3));
    expect(historyOf(useEdits.getState().byImage, K).future).toHaveLength(0);
    expect(edits().strokes.map((s) => s.points[0][0])).toEqual([1, 3]);
  });

  it("keeps images apart and limits the history", () => {
    const { addStroke } = useEdits.getState();
    for (let i = 0; i < UNDO_LIMIT + 5; i++) addStroke(K, stroke(i));
    addStroke("fast:/b.png", stroke(0));
    expect(historyOf(useEdits.getState().byImage, K).past).toHaveLength(UNDO_LIMIT);
    expect(historyOf(useEdits.getState().byImage, "fast:/b.png").edits.strokes).toHaveLength(1);
  });

  it("shares one background between images, with undo in order", () => {
    const { addStroke, commitBackground, previewBackground, undo, redo } = useEdits.getState();
    const bg = () => useEdits.getState().background.value;
    const B = "fast:/b.png";
    addStroke(K, stroke(1)); // 1: image A
    commitBackground(K, { kind: "color", color: "#3D8BFF" }); // 2: background
    addStroke(K, stroke(2)); // 3: image A
    // One step per drag: previews don't count.
    for (const strength of [10, 20, 30]) previewBackground({ kind: "blur", strength });
    expect(useEdits.getState().liveBackground).toEqual({ kind: "blur", strength: 30 });
    commitBackground(B, { kind: "blur", strength: 30 }); // 4: background, while B is shown
    expect(useEdits.getState().liveBackground).toBeUndefined();

    // Image B sees the shared background; undo there goes back to the colour.
    expect(bg()).toEqual({ kind: "blur", strength: 30 });
    expect(canUndo(useEdits.getState(), B)).toBe(true);
    undo(B);
    expect(bg()).toEqual({ kind: "color", color: "#3D8BFF" });

    // In A: newest first — stroke 2, then the colour, then stroke 1.
    undo(K);
    expect(edits().strokes).toHaveLength(1);
    expect(bg().kind).toBe("color");
    undo(K);
    expect(bg()).toEqual({ kind: "transparent" });
    expect(edits().strokes).toHaveLength(1);
    undo(K);
    expect(edits().strokes).toHaveLength(0);
    expect(canUndo(useEdits.getState(), K)).toBe(false);

    // Redo replays them in the same order.
    redo(K);
    expect(edits().strokes).toHaveLength(1);
    redo(K);
    expect(bg().kind).toBe("color");
    redo(K);
    expect(edits().strokes).toHaveLength(2);
    redo(K);
    expect(bg()).toEqual({ kind: "blur", strength: 30 });
    expect(canRedo(useEdits.getState(), K)).toBe(false);
  });

  it("remembers the last colour, blur and image, and ignores no-op changes", () => {
    const { commitBackground, undo } = useEdits.getState();
    commitBackground(K, { kind: "color", color: "#FF5A5F" });
    commitBackground(K, { kind: "blur", strength: 70 });
    commitBackground(K, { kind: "image", path: "/bg.jpg" });
    commitBackground(K, { kind: "image", path: "/bg.jpg" }); // the same: no step
    expect(useEdits.getState().background.past).toHaveLength(3);
    expect(useEdits.getState().choices).toEqual({ color: "#FF5A5F", strength: 70, path: "/bg.jpg" });
    undo(K);
    expect(useEdits.getState().background.value).toEqual({ kind: "blur", strength: 70 });
  });

  it("a new edit clears the background's redo steps", () => {
    const { commitBackground, addStroke, undo } = useEdits.getState();
    commitBackground(K, { kind: "color", color: "#000000" });
    undo(K);
    expect(useEdits.getState().background.future).toHaveLength(1);
    addStroke(K, stroke(1));
    expect(useEdits.getState().background.future).toHaveLength(0);
  });
});
