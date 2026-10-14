import { RotateCcw } from "lucide-react";
import { DEFAULT_EDGES, EdgeSettings, FEATHER_MAX, SHIFT_MAX } from "../lib/refine";
import { historyOf, useEdits } from "../store/edits";
import { Button, Field, SectionLabel, Slider, Switch } from "../ui";

const px = (v: number) => `${v} px`;
const signedPx = (v: number) => `${v > 0 ? "+" : ""}${v} px`;

/** Inspector: Feather, Shift and Remove dark fringe for the cutout `editKey`. */
export function EdgesSection({ editKey, disabled }: { editKey: string; disabled: boolean }) {
  const committed = useEdits((s) => historyOf(s.byImage, editKey).edits.edges);
  const live = useEdits((s) => s.live[editKey]);
  const { previewEdges, commitEdges } = useEdits.getState();
  const edges = live ?? committed;
  const set = (change: Partial<EdgeSettings>) => previewEdges(editKey, { ...edges, ...change });
  const commit = (change: Partial<EdgeSettings>) => commitEdges(editKey, { ...edges, ...change });
  const isDefault =
    committed.feather === DEFAULT_EDGES.feather &&
    committed.shift === DEFAULT_EDGES.shift &&
    committed.decontaminate === DEFAULT_EDGES.decontaminate;

  return (
    <section className="inspector-section">
      <div className="section-head">
        <SectionLabel>Edges</SectionLabel>
        <Button
          variant="ghost"
          size="sm"
          icon={RotateCcw}
          disabled={disabled || isDefault}
          onClick={() => commitEdges(editKey, DEFAULT_EDGES)}
        >
          Reset
        </Button>
      </div>
      <Field label="Feather" hint="Softens the edge.">
        <div className="slider-row">
          <Slider
            label="Feather"
            value={edges.feather}
            min={0}
            max={FEATHER_MAX}
            step={0.5}
            format={px}
            disabled={disabled}
            onChange={(feather) => set({ feather })}
            onCommit={(feather) => commit({ feather })}
          />
          <span className="slider-value">{px(edges.feather)}</span>
        </div>
      </Field>
      <Field label="Shift" hint="Shrinks (−) or grows (+) the cutout.">
        <div className="slider-row">
          <Slider
            label="Shift"
            value={edges.shift}
            min={-SHIFT_MAX}
            max={SHIFT_MAX}
            origin={0}
            step={1}
            format={signedPx}
            disabled={disabled}
            onChange={(shift) => set({ shift })}
            onCommit={(shift) => commit({ shift })}
          />
          <span className="slider-value">{signedPx(edges.shift)}</span>
        </div>
      </Field>
      <Switch
        label="Remove dark fringe"
        checked={edges.decontaminate}
        disabled={disabled}
        onChange={(decontaminate) => commitEdges(editKey, { ...committed, decontaminate })}
      />
    </section>
  );
}
