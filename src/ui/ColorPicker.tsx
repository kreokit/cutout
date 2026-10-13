import { useCallback, useEffect, useRef, useState } from "react";
import { normalizeHex } from "./logic";
import { Popover } from "./Popover";

/** Swatches offered in every colour picker: neutrals, the brand lime, and caption favourites. */
export const SWATCHES = [
  "#FFFFFF",
  "#A1A1AA",
  "#26262B",
  "#000000",
  "#C8F135",
  "#FFD400",
  "#FF8A1F",
  "#FF5A5F",
  "#FF5FD2",
  "#9B6BFF",
  "#3D8BFF",
  "#3DE0FF",
];

/** A colour swatch button that opens a flat picker: swatches plus a hex field. */
export function ColorPicker({
  value,
  onChange,
  label,
}: {
  value: string;
  onChange: (hex: string) => void;
  label: string;
}) {
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState(value);
  const anchor = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => {
    setOpen(false);
    anchor.current?.focus();
  }, []);
  useEffect(() => setDraft(value), [value, open]);

  const commitDraft = () => {
    const hex = normalizeHex(draft);
    if (hex) onChange(hex);
    else setDraft(value);
  };

  return (
    <>
      <button
        ref={anchor}
        type="button"
        className="ui-color"
        aria-label={`${label}: ${value}`}
        aria-haspopup="dialog"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        <span className="ui-color-chip" style={{ background: value }} />
        <span className="ui-color-hex tabular">{value}</span>
      </button>
      <Popover open={open} onClose={close} anchor={anchor}>
        <div role="dialog" aria-label={label} className="ui-color-panel">
          <div className="ui-swatches" role="group" aria-label="Swatches">
            {SWATCHES.map((hex) => (
              <button
                key={hex}
                type="button"
                className="ui-swatch"
                style={{ background: hex }}
                aria-label={hex}
                aria-pressed={hex === value}
                onClick={() => {
                  onChange(hex);
                  close();
                }}
              />
            ))}
          </div>
          <label className="ui-color-input">
            <span>Hex</span>
            <input
              className="ui-input tabular"
              value={draft}
              spellCheck={false}
              maxLength={7}
              aria-invalid={normalizeHex(draft) === null}
              onChange={(e) => setDraft(e.target.value)}
              onBlur={commitDraft}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  commitDraft();
                  close();
                }
              }}
            />
          </label>
        </div>
      </Popover>
    </>
  );
}
