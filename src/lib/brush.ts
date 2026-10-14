/**
 * Brush maths for the live preview, identical to src-tauri/src/refine.rs (both are
 * checked against fixtures/brush-cases.json).
 */

/**
 * How much a brush of `radius` and `softness` covers a pixel whose centre is `distance`
 * from the stroke: 1 inside the hard core, 0 outside, a smoothstep between. The soft band
 * is at least 1 px wide, so a hard brush is still anti-aliased.
 */
export function brushAlpha(distance: number, radius: number, softness: number): number {
  const band = Math.min(Math.max(radius * Math.min(Math.max(softness, 0), 1), 1), Math.max(radius, 1e-6));
  const inner = radius - band;
  if (distance <= inner) return 1;
  if (distance >= radius) return 0;
  const t = (radius - distance) / band;
  return t * t * (3 - 2 * t);
}

/** Distance from (px, py) to the segment (ax, ay)–(bx, by). */
export function segmentDistance(px: number, py: number, ax: number, ay: number, bx: number, by: number): number {
  const dx = bx - ax;
  const dy = by - ay;
  const len2 = dx * dx + dy * dy;
  const t = len2 > 0 ? Math.min(1, Math.max(0, ((px - ax) * dx + (py - ay) * dy) / len2)) : 0;
  const cx = ax + t * dx - px;
  const cy = ay + t * dy - py;
  return Math.sqrt(cx * cx + cy * cy);
}

/** Brush size limits (diameter in screen pixels) and the default. */
export const BRUSH_MIN = 4;
export const BRUSH_MAX = 300;
export const BRUSH_DEFAULT = 40;

/** The next brush size for `[` (−1) or `]` (+1): about 20% per step. */
export function stepBrushSize(size: number, direction: 1 | -1): number {
  const next = direction > 0 ? Math.max(size + 1, size * 1.25) : Math.min(size - 1, size / 1.25);
  return Math.round(Math.min(BRUSH_MAX, Math.max(BRUSH_MIN, next)));
}
