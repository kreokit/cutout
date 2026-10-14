import { beforeEach, describe, expect, it } from "vitest";
import { DEFAULT_EDGES, Stroke } from "../lib/refine";
import { UNDO_LIMIT, historyOf, useEdits } from "./edits";

const K = "fast:/a.png";
const stroke = (x: number): Stroke => ({ mode: "erase", radius: 10, softness: 0.5, points: [[x, x]] });
const edits = () => historyOf(useEdits.getState().byImage, K).edits;

describe("edits store", () => {
  beforeEach(() => useEdits.setState({ byImage: {}, live: {} }));

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
});
