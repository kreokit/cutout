import { invoke } from "@tauri-apps/api/core";
import { CutoutModel } from "./models";
import { Edits, Render, parseRender } from "./refine";

/** The background behind the cutout (background.rs), shared by all open images. */
export type Background =
  | { kind: "transparent" }
  | { kind: "color"; color: string }
  | { kind: "blur"; strength: number }
  | { kind: "image"; path: string };

export type BackgroundKind = Background["kind"];

export const TRANSPARENT: Background = { kind: "transparent" };
export const BLUR_MIN = 5;
export const BLUR_MAX = 100;
export const DEFAULT_BLUR = 40;
export const DEFAULT_COLOR = "#FFFFFF";

/** Background swatches: white, off-white, black, the KreoKit dark and lime, studio colours. */
export const BACKGROUND_SWATCHES = [
  "#FFFFFF",
  "#F4F4F5",
  "#000000",
  "#0E0E10",
  "#C8F135",
  "#E9E2D6",
  "#D8E3EC",
  "#F6D3C8",
  "#FFD400",
  "#FF5A5F",
  "#3D8BFF",
  "#9B6BFF",
];

export const sameBackground = (a: Background, b: Background) => JSON.stringify(a) === JSON.stringify(b);

/** The blur or image layer under the stage's cutout (raw RGBA from the backend). */
export async function stageBackground(
  path: string,
  model: CutoutModel,
  edits: Edits,
  background: Background,
): Promise<Render> {
  return parseRender(await invoke<ArrayBuffer>("stage_background", { path, model, edits, background }));
}
