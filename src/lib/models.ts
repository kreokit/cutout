import { Channel, invoke } from "@tauri-apps/api/core";

export type CutoutModel = "fast" | "best";

/** A model file on the mirror (models.rs). Best is 768 on computers with more than 8 GB
 * of memory, 640 on others (and as the fallback when free memory is low). */
export type ModelFile = "u2net" | "birefnet-768" | "birefnet-640";

export interface ModelInfo {
  id: CutoutModel;
  /** The file this computer uses for the model. */
  file: ModelFile;
  size_bytes: number;
  downloaded: boolean;
}

export interface DownloadProgress {
  downloaded_bytes: number;
  total_bytes: number;
}

/** The backend's message when a download is cancelled (models.rs). */
export const CANCELLED = "cancelled";

export const MODEL_NAMES: Record<CutoutModel, string> = { fast: "Fast", best: "Best" };

/** Rough time per image on a slow laptop (docs/performance.md). */
export const MODEL_HINTS: Record<CutoutModel, string> = { fast: "about 1 s per image", best: "10–15 s per image" };

export const listModels = () => invoke<ModelInfo[]>("list_models");

export function downloadModel(file: ModelFile, onProgress: (p: DownloadProgress) => void) {
  const channel = new Channel<DownloadProgress>();
  channel.onmessage = onProgress;
  return invoke<void>("download_model", { file, onProgress: channel });
}

export const cancelDownload = () => invoke<void>("cancel");

/** Download progress as a whole percentage, 0–100. */
export const percent = ({ downloaded_bytes, total_bytes }: DownloadProgress) =>
  total_bytes > 0 ? Math.min(100, Math.floor((downloaded_bytes / total_bytes) * 100)) : 0;
