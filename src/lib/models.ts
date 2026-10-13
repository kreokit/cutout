import { Channel, invoke } from "@tauri-apps/api/core";

export type CutoutModel = "fast" | "best";

export interface ModelInfo {
  id: CutoutModel;
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

export const listModels = () => invoke<ModelInfo[]>("list_models");

export function downloadModel(model: CutoutModel, onProgress: (p: DownloadProgress) => void) {
  const channel = new Channel<DownloadProgress>();
  channel.onmessage = onProgress;
  return invoke<void>("download_model", { model, onProgress: channel });
}

export const cancelDownload = () => invoke<void>("cancel");

/** Download progress as a whole percentage, 0–100. */
export const percent = ({ downloaded_bytes, total_bytes }: DownloadProgress) =>
  total_bytes > 0 ? Math.min(100, Math.floor((downloaded_bytes / total_bytes) * 100)) : 0;
