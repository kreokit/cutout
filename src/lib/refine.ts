import { invoke } from "@tauri-apps/api/core";
import { CutoutModel } from "./models";

/** Settings of the Edges controls (refine.rs). Lengths in pixels of the original image. */
export interface EdgeSettings {
  feather: number;
  shift: number;
  decontaminate: boolean;
}

export type BrushMode = "erase" | "restore";

/** One brush stroke, in pixels of the original image. */
export interface Stroke {
  mode: BrushMode;
  radius: number;
  /** 0 = hard, 1 = soft from the centre. */
  softness: number;
  points: [number, number][];
}

/** Everything the user changed on one image. */
export interface Edits {
  edges: EdgeSettings;
  strokes: Stroke[];
}

export const DEFAULT_EDGES: EdgeSettings = { feather: 0, shift: 0, decontaminate: true };
export const NO_EDITS: Edits = { edges: DEFAULT_EDGES, strokes: [] };

export const FEATHER_MAX = 20;
export const SHIFT_MAX = 10;

/** Pixels from a render: a region of the working copy (session.rs). */
export interface Render {
  /** Size of the whole working copy (or of the preview). */
  width: number;
  height: number;
  x: number;
  y: number;
  regionWidth: number;
  regionHeight: number;
  pixels: Uint8ClampedArray<ArrayBuffer>;
}

/** Reads the 24-byte header (six little-endian u32) and the RGBA pixels after it. */
export function parseRender(buffer: ArrayBuffer): Render {
  const h = new DataView(buffer, 0, 24);
  const at = (i: number) => h.getUint32(i * 4, true);
  return {
    width: at(0),
    height: at(1),
    x: at(2),
    y: at(3),
    regionWidth: at(4),
    regionHeight: at(5),
    pixels: new Uint8ClampedArray(buffer, 24),
  };
}

export async function renderRefined(
  path: string,
  model: CutoutModel,
  edits: Edits,
  opts: { preview?: boolean; full?: boolean } = {},
): Promise<Render> {
  const buffer = await invoke<ArrayBuffer>("render_refined", {
    path,
    model,
    edits,
    preview: opts.preview ?? false,
    full: opts.full ?? false,
  });
  return parseRender(buffer);
}

export const closeRefine = () => invoke<void>("close_refine");
