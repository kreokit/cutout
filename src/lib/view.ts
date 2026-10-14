import { Size, fitContain } from "./fit";

/** How the image sits on the stage: screen = image pixel × scale + (x, y). */
export interface View {
  scale: number;
  x: number;
  y: number;
}

export interface Point {
  x: number;
  y: number;
}

export const MIN_SCALE = 0.02;
export const MAX_SCALE = 32;
/** Zoom levels for the − / + buttons and ⌘− / ⌘+. */
export const ZOOM_STEPS = [0.05, 0.1, 0.25, 0.33, 0.5, 0.67, 1, 1.5, 2, 3, 4, 6, 8, 12, 16, 24, 32];

const clampScale = (s: number) => Math.min(MAX_SCALE, Math.max(MIN_SCALE, s));

/** The whole image centred in `box` with `padding`, never enlarged beyond 100%. */
export function fitView(image: Size, box: Size, padding: number): View {
  const { width, height, scale } = fitContain(image, box, padding);
  const s = scale > 0 ? scale : 1;
  return { scale: s, x: (box.width - width) / 2, y: (box.height - height) / 2 };
}

/** `scale` centred on `box` (e.g. 100%). */
export function centeredView(image: Size, box: Size, scale: number): View {
  const s = clampScale(scale);
  return { scale: s, x: (box.width - image.width * s) / 2, y: (box.height - image.height * s) / 2 };
}

/** Zooms to `scale` keeping the image point under `at` (stage coordinates) in place. */
export function zoomTo(view: View, scale: number, at: Point): View {
  const s = clampScale(scale);
  const k = s / view.scale;
  return { scale: s, x: at.x - (at.x - view.x) * k, y: at.y - (at.y - view.y) * k };
}

export const zoomBy = (view: View, factor: number, at: Point) => zoomTo(view, view.scale * factor, at);

/** The next zoom step above (direction 1) or below (−1) the current scale. */
export function stepScale(scale: number, direction: 1 | -1): number {
  const eps = 1e-6;
  if (direction > 0) return ZOOM_STEPS.find((s) => s > scale + eps) ?? MAX_SCALE;
  return [...ZOOM_STEPS].reverse().find((s) => s < scale - eps) ?? MIN_SCALE;
}

export const panBy = (view: View, dx: number, dy: number): View => ({ ...view, x: view.x + dx, y: view.y + dy });

/**
 * Keeps at least `keep` pixels of the image on the stage, so it can't be lost off-screen.
 */
export function clampView(view: View, image: Size, box: Size, keep = 64): View {
  const w = image.width * view.scale;
  const h = image.height * view.scale;
  const kx = Math.min(keep, w);
  const ky = Math.min(keep, h);
  return {
    ...view,
    x: Math.min(box.width - kx, Math.max(kx - w, view.x)),
    y: Math.min(box.height - ky, Math.max(ky - h, view.y)),
  };
}

export interface WheelInput {
  deltaX: number;
  deltaY: number;
  /** 0 = pixels, 1 = lines, 2 = pages. */
  deltaMode: number;
  ctrlKey: boolean;
  metaKey: boolean;
}

export type WheelAction = { kind: "pan"; dx: number; dy: number } | { kind: "zoom"; factor: number };

const LINE = 16;
const PAGE = 400;

/**
 * Figma style: scrolling pans; ⌘/Ctrl + scroll zooms (trackpad pinches arrive as Ctrl +
 * scroll on Windows and Linux; WebKit's pinch gesture is handled separately).
 */
export function wheelAction(e: WheelInput): WheelAction {
  const unit = e.deltaMode === 1 ? LINE : e.deltaMode === 2 ? PAGE : 1;
  const dx = e.deltaX * unit;
  const dy = e.deltaY * unit;
  if (e.ctrlKey || e.metaKey) return { kind: "zoom", factor: Math.exp(-dy * 0.01) };
  return { kind: "pan", dx: -dx, dy: -dy };
}

/** Converts a stage point to image pixels. */
export const toImage = (view: View, p: Point): Point => ({ x: (p.x - view.x) / view.scale, y: (p.y - view.y) / view.scale });
