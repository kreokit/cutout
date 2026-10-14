import { BrushMode } from "./refine";
import { brushAlpha, segmentDistance } from "./brush";

/**
 * Live brush preview: paints a stroke straight into the cutout canvas while the pointer
 * moves, with the same maths as the backend (refine.rs), which then renders the final
 * pixels. Coverage within one stroke is the maximum over its segments, as in the backend.
 */

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** One tile touched by the stroke: its pixels when the stroke began, coverage, output. */
export interface Tile extends Rect {
  snap: Uint8ClampedArray;
  coverage: Float32Array;
  out: Uint8ClampedArray;
}

export interface Brush {
  mode: BrushMode;
  /** Radius in canvas pixels. */
  radius: number;
  softness: number;
}

/**
 * Stamps the segment a–b of `brush` into `tile` within `region` (canvas pixels).
 * `original` is the whole original image (RGBA, `originalWidth` wide), used to restore.
 */
export function stampSegment(
  tile: Tile,
  region: Rect,
  a: [number, number],
  b: [number, number],
  brush: Brush,
  original: Uint8ClampedArray,
  originalWidth: number,
): void {
  for (let y = region.y; y < region.y + region.height; y++) {
    for (let x = region.x; x < region.x + region.width; x++) {
      const d = segmentDistance(x + 0.5, y + 0.5, a[0], a[1], b[0], b[1]);
      const c = brushAlpha(d, brush.radius, brush.softness);
      const i = (y - tile.y) * tile.width + (x - tile.x);
      if (c <= tile.coverage[i]) continue;
      tile.coverage[i] = c;
      const p = i * 4;
      const sa = tile.snap[p + 3];
      if (brush.mode === "erase") {
        tile.out[p] = tile.snap[p];
        tile.out[p + 1] = tile.snap[p + 1];
        tile.out[p + 2] = tile.snap[p + 2];
        tile.out[p + 3] = sa * (1 - c);
      } else {
        const o = (y * originalWidth + x) * 4;
        for (let k = 0; k < 3; k++) {
          tile.out[p + k] = sa === 0 ? original[o + k] : tile.snap[p + k] + (original[o + k] - tile.snap[p + k]) * c;
        }
        tile.out[p + 3] = sa + (original[o + 3] - sa) * c;
      }
    }
  }
}

/** The part of `a` inside `b`, or null. */
export function intersect(a: Rect, b: Rect): Rect | null {
  const x = Math.max(a.x, b.x);
  const y = Math.max(a.y, b.y);
  const x1 = Math.min(a.x + a.width, b.x + b.width);
  const y1 = Math.min(a.y + a.height, b.y + b.height);
  return x1 > x && y1 > y ? { x, y, width: x1 - x, height: y1 - y } : null;
}

export const TILE = 256;

/** Paints one stroke into a canvas; create one per stroke. */
export class LivePainter {
  private tiles = new Map<number, Tile>();
  private last: [number, number] | null = null;

  constructor(
    private ctx: CanvasRenderingContext2D,
    private brush: Brush,
    private original: Uint8ClampedArray,
    private width: number,
    private height: number,
  ) {}

  /** Extends the stroke to (x, y), in canvas pixels. */
  add(x: number, y: number) {
    const a = this.last ?? [x, y];
    const b: [number, number] = [x, y];
    this.last = b;
    const r = this.brush.radius + 1;
    const box = intersect(
      {
        x: Math.floor(Math.min(a[0], b[0]) - r),
        y: Math.floor(Math.min(a[1], b[1]) - r),
        width: Math.ceil(Math.abs(a[0] - b[0]) + 2 * r) + 1,
        height: Math.ceil(Math.abs(a[1] - b[1]) + 2 * r) + 1,
      },
      { x: 0, y: 0, width: this.width, height: this.height },
    );
    if (!box) return;
    for (let ty = Math.floor(box.y / TILE); ty <= Math.floor((box.y + box.height - 1) / TILE); ty++) {
      for (let tx = Math.floor(box.x / TILE); tx <= Math.floor((box.x + box.width - 1) / TILE); tx++) {
        const tile = this.tile(tx, ty);
        const region = intersect(box, tile);
        if (!region) continue;
        stampSegment(tile, region, a, b, this.brush, this.original, this.width);
        const image = new ImageData(tile.out as Uint8ClampedArray<ArrayBuffer>, tile.width, tile.height);
        this.ctx.putImageData(image, tile.x, tile.y, region.x - tile.x, region.y - tile.y, region.width, region.height);
      }
    }
  }

  private tile(tx: number, ty: number): Tile {
    const key = ty * 100_000 + tx;
    let tile = this.tiles.get(key);
    if (!tile) {
      const x = tx * TILE;
      const y = ty * TILE;
      const width = Math.min(TILE, this.width - x);
      const height = Math.min(TILE, this.height - y);
      const snap = this.ctx.getImageData(x, y, width, height).data;
      tile = { x, y, width, height, snap, coverage: new Float32Array(width * height), out: new Uint8ClampedArray(snap) };
      this.tiles.set(key, tile);
    }
    return tile;
  }
}
