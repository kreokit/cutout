import { Brush, Eraser, Hand } from "lucide-react";
import { BRUSH_MAX, BRUSH_MIN } from "../lib/brush";
import { Tool, useBrush } from "../store/brush";
import { Field, SectionLabel, Segmented, Slider } from "../ui";

const percent = (v: number) => `${Math.round(v * 100)}%`;

/** Inspector: the brush tools (Hand, Erase, Restore) with size and softness. */
export function RefineSection({ disabled, beforeView }: { disabled: boolean; beforeView: boolean }) {
  const { tool, size, softness, setTool, setSize, setSoftness } = useBrush();
  const brush = tool !== "hand";
  return (
    <section className="inspector-section">
      <SectionLabel>Refine</SectionLabel>
      <Segmented<Tool>
        label="Tool"
        value={tool}
        onChange={setTool}
        disabled={disabled}
        options={[
          { value: "hand", label: "Hand", icon: Hand, hint: "H" },
          { value: "erase", label: "Erase", icon: Eraser, hint: "E" },
          { value: "restore", label: "Restore", icon: Brush, hint: "R" },
        ]}
      />
      <Field label="Size" hint="[ and ] change the size. Zoom in for finer work.">
        <div className="slider-row">
          <Slider
            label="Brush size"
            value={size}
            min={BRUSH_MIN}
            max={BRUSH_MAX}
            format={(v) => `${v} px`}
            disabled={disabled || !brush}
            onChange={setSize}
          />
          <span className="slider-value">{size} px</span>
        </div>
      </Field>
      <Field label="Softness" hint="Shift + [ and ] change the softness.">
        <div className="slider-row">
          <Slider
            label="Brush softness"
            value={Math.round(softness * 100)}
            min={0}
            max={100}
            step={5}
            format={(v) => `${v}%`}
            disabled={disabled || !brush}
            onChange={(v) => setSoftness(v / 100)}
          />
          <span className="slider-value">{percent(softness)}</span>
        </div>
      </Field>
      {brush && !disabled && (
        <p className="inspector-note">
          {beforeView
            ? "Switch to After or Compare to paint."
            : tool === "erase"
              ? "Paint over what should go. Hold Space to pan."
              : "Paint over what should stay. Hold Space to pan."}
        </p>
      )}
    </section>
  );
}
