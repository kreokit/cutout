import { Channel, invoke } from "@tauri-apps/api/core";
import { CutoutModel, ModelFile } from "./models";

export type Stage = "loading_model" | "cutting_out" | "saving";

/** Seconds per step (cutout.rs). */
export interface Timings {
  load_model: number;
  decode: number;
  preprocess: number;
  inference: number;
  postprocess: number;
  save: number;
  total: number;
}

export interface CutoutResult {
  /** Transparent PNG at the original size. */
  cutout: string;
  /** Greyscale mask PNG at the original size. */
  mask: string;
  model: CutoutModel;
  /** Came from the cache (no timings). */
  cached: boolean;
  timings: Timings;
}

export const STAGE_LABELS: Record<Stage, string> = {
  loading_model: "Loading the AI model…",
  cutting_out: "Removing the background…",
  saving: "Saving…",
};

/** What `cut_out` did (lib.rs): the result, or why it didn't run yet. */
export type CutOutcome =
  | { kind: "done"; result: CutoutResult }
  | { kind: "download"; file: ModelFile; size_bytes: number }
  | { kind: "low-memory"; available_bytes: number; needed_bytes: number };

/** Cuts out `path`. Best checks the free memory first; `force` ("Try anyway") skips that. */
export function cutOut(path: string, model: CutoutModel, onStage: (stage: Stage) => void, force = false) {
  const channel = new Channel<Stage>();
  channel.onmessage = onStage;
  return invoke<CutOutcome>("cut_out", { path, model, force, onProgress: channel });
}

/** "0.8 s", "1.2 s", "14 s". */
export const formatSeconds = (s: number) => (s < 10 ? `${s.toFixed(1)} s` : `${Math.round(s)} s`);
