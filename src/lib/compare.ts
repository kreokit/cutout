/** Where the compare divider is, 0 (left edge) to 1 (right edge). */
export type Split = number;

const clamp01 = (v: number) => Math.min(1, Math.max(0, v));

/** The split under a pointer at `clientX` over an image spanning `left`…`left + width`. */
export function splitAt(clientX: number, left: number, width: number): Split {
  return width > 0 ? clamp01((clientX - left) / width) : 0.5;
}

/** Arrow keys move the divider by 5% (Shift: 25%); Home/End jump to the edges. */
export function splitForKey(split: Split, key: string, shift: boolean): Split | null {
  const step = shift ? 0.25 : 0.05;
  switch (key) {
    case "ArrowLeft":
    case "ArrowDown":
      return clamp01(split - step);
    case "ArrowRight":
    case "ArrowUp":
      return clamp01(split + step);
    case "Home":
      return 0;
    case "End":
      return 1;
    default:
      return null;
  }
}
