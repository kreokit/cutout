import { useRef } from "react";
import { clamp, valueAfterKey, valueAtRatio } from "./logic";

/** A flat slider: track with a lime fill and a square handle. Drag, click or use keys. */
export function Slider({
  value,
  min,
  max,
  step = 1,
  onChange,
  label,
  format = String,
  disabled,
}: {
  value: number;
  min: number;
  max: number;
  step?: number;
  onChange: (value: number) => void;
  label: string;
  format?: (value: number) => string;
  disabled?: boolean;
}) {
  const track = useRef<HTMLDivElement>(null);
  const ratio = max > min ? (clamp(value, min, max) - min) / (max - min) : 0;

  const fromPointer = (clientX: number) => {
    const r = track.current?.getBoundingClientRect();
    if (!r || r.width === 0) return;
    const next = valueAtRatio((clientX - r.left) / r.width, min, max, step);
    if (next !== value) onChange(next);
  };

  return (
    <div
      ref={track}
      role="slider"
      tabIndex={disabled ? -1 : 0}
      aria-label={label}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={value}
      aria-valuetext={format(value)}
      aria-disabled={disabled}
      className="ui-slider"
      onPointerDown={(e) => {
        if (disabled || e.button !== 0) return;
        e.currentTarget.setPointerCapture(e.pointerId);
        e.currentTarget.focus();
        fromPointer(e.clientX);
      }}
      onPointerMove={(e) => {
        if (e.currentTarget.hasPointerCapture(e.pointerId)) fromPointer(e.clientX);
      }}
      onKeyDown={(e) => {
        if (disabled) return;
        const next = valueAfterKey(e.key, e.shiftKey, value, min, max, step);
        if (next === null) return;
        e.preventDefault();
        if (next !== value) onChange(next);
      }}
    >
      <div className="ui-slider-track">
        <div className="ui-slider-fill" style={{ width: `${ratio * 100}%` }} />
      </div>
      <div className="ui-slider-thumb" style={{ left: `${ratio * 100}%` }} />
    </div>
  );
}
