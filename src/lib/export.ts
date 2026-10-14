import { invoke } from "@tauri-apps/api/core";
import { Background } from "./background";
import { CutoutModel } from "./models";
import { Edits, Render, parseRender } from "./refine";

export type Format = "png" | "webp" | "jpg";
export type Fit = "fit" | "fill";

export interface OutputSize {
  width: number;
  height: number;
}

/** The Export dialog's settings (export.rs `ExportSettings`). */
export interface ExportSettings {
  format: Format;
  /** 1–100, for WebP and JPG. */
  quality: number;
  /** null: the original's size. */
  size: OutputSize | null;
  fit: Fit;
}

export const DEFAULT_EXPORT: ExportSettings = { format: "png", quality: 90, size: null, fit: "fit" };
export const QUALITY_MIN = 40;
export const QUALITY_MAX = 100;

export const FORMATS: { value: Format; label: string }[] = [
  { value: "png", label: "PNG" },
  { value: "webp", label: "WebP" },
  { value: "jpg", label: "JPG" },
];

export type SizeId = "original" | "square" | "portrait" | "story" | "landscape";

export const SIZES: { id: SizeId; label: string; size: OutputSize | null }[] = [
  { id: "original", label: "Original", size: null },
  { id: "square", label: "Square", size: { width: 1080, height: 1080 } },
  { id: "portrait", label: "Portrait 4:5", size: { width: 1080, height: 1350 } },
  { id: "story", label: "Story 9:16", size: { width: 1080, height: 1920 } },
  { id: "landscape", label: "Landscape 16:9", size: { width: 1280, height: 720 } },
];

export const sizeId = (size: OutputSize | null): SizeId =>
  SIZES.find((s) => (s.size === null ? size === null : s.size.width === size?.width && s.size.height === size?.height))
    ?.id ?? "original";

/** Settings read back from storage, with anything unknown replaced by the defaults. */
export function sanitizeSettings(raw: unknown): ExportSettings {
  const r = (raw ?? {}) as Partial<ExportSettings>;
  const format = FORMATS.some((f) => f.value === r.format) ? r.format! : DEFAULT_EXPORT.format;
  const quality =
    typeof r.quality === "number" && Number.isFinite(r.quality)
      ? Math.round(Math.min(QUALITY_MAX, Math.max(QUALITY_MIN, r.quality)))
      : DEFAULT_EXPORT.quality;
  const preset = SIZES.find((s) => s.size && s.size.width === r.size?.width && s.size.height === r.size?.height);
  const fit = r.fit === "fill" ? "fill" : "fit";
  return { format, quality, size: preset?.size ?? null, fit };
}

/** "/photos/cat.jpg" → "/photos/cat-cutout.png" (the default name in the save dialog). */
export function exportPath(imagePath: string, format: Format): string {
  const cut = Math.max(imagePath.lastIndexOf("/"), imagePath.lastIndexOf("\\"));
  const dir = imagePath.slice(0, cut + 1);
  const name = imagePath.slice(cut + 1);
  const dot = name.lastIndexOf(".");
  const stem = dot > 0 ? name.slice(0, dot) : name;
  return `${dir}${stem}-cutout.${format}`;
}

/** Exports at full size to `dest`; returns the path written. */
export const exportImage = (
  path: string,
  model: CutoutModel,
  edits: Edits,
  background: Background,
  settings: ExportSettings,
  dest: string,
) => invoke<string>("export_image", { path, model, edits, background, settings, dest });

/** A small preview of exactly what will be exported. */
export async function exportPreview(
  path: string,
  model: CutoutModel,
  edits: Edits,
  background: Background,
  settings: ExportSettings,
): Promise<Render> {
  return parseRender(await invoke<ArrayBuffer>("export_preview", { path, model, edits, background, settings }));
}

export const revealExport = (path: string) => invoke<void>("reveal_export", { path });
