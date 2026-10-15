import { create } from "zustand";
import { CutoutResult, Stage, cutOut } from "../lib/cutout";
import {
  CANCELLED,
  CutoutModel,
  DownloadProgress,
  ModelFile,
  ModelInfo,
  cancelDownload,
  downloadModel,
  listModels,
} from "../lib/models";

export type CutoutState =
  | { status: "running"; stage: Stage | null }
  | { status: "done"; result: CutoutResult }
  | { status: "error"; error: string }
  /** Best needs another file first (its smaller fallback, when free memory is low). */
  | { status: "needs-download"; file: ModelFile; sizeBytes: number }
  /** Not enough free memory for Best: Use Fast or Try anyway. */
  | { status: "low-memory"; availableBytes: number; neededBytes: number };

interface Cutouts {
  /** The model new cutouts use (picked on Home). */
  model: CutoutModel;
  /** The model chosen for one image (in the editor); others use `model`. */
  modelOf: Record<string, CutoutModel>;
  /** Model files and whether they're downloaded; null until loaded. */
  models: ModelInfo[] | null;
  download: { file: ModelFile; progress: DownloadProgress | null } | null;
  downloadError: string | null;
  /** Cutouts by `cutoutKey(path, model)`. */
  byImage: Record<string, CutoutState>;
  setModel: (model: CutoutModel) => void;
  /** Chooses the model for the image at `path` only. */
  setModelFor: (path: string, model: CutoutModel) => void;
  refreshModels: () => Promise<void>;
  startDownload: (file: ModelFile) => Promise<void>;
  cancelDownload: () => void;
  /**
   * Cuts out `path` with its model unless it's done or running (`retry` runs it again
   * after an error, a needed download or low memory; `force` runs Best even when memory
   * is short: "Try anyway").
   */
  run: (path: string, opts?: { retry?: boolean; force?: boolean }) => Promise<void>;
}

export const cutoutKey = (path: string, model: CutoutModel) => `${model}:${path}`;

/** The model the image at `path` uses. */
export const modelFor = (s: Pick<Cutouts, "model" | "modelOf">, path: string) => s.modelOf[path] ?? s.model;

export const isDownloaded = (models: ModelInfo[] | null, model: CutoutModel) =>
  models?.some((m) => m.id === model && m.downloaded) ?? false;

/** Cutouts run one after another, in the order they were asked for. */
let queue: Promise<unknown> = Promise.resolve();

export const useCutouts = create<Cutouts>((set, get) => {
  const setState = (key: string, state: CutoutState) =>
    set((s) => ({ byImage: { ...s.byImage, [key]: state } }));

  return {
    model: "fast",
    modelOf: {},
    models: null,
    download: null,
    downloadError: null,
    byImage: {},

    setModel: (model) => set({ model }),

    setModelFor: (path, model) => set((s) => ({ modelOf: { ...s.modelOf, [path]: model } })),

    refreshModels: async () => {
      set({ models: await listModels() });
    },

    startDownload: async (file) => {
      if (get().download) return;
      set({ download: { file, progress: null }, downloadError: null });
      try {
        await downloadModel(file, (progress) => set({ download: { file, progress } }));
        await get().refreshModels();
      } catch (e) {
        if (String(e) !== CANCELLED) set({ downloadError: String(e) });
      } finally {
        set({ download: null });
      }
    },

    cancelDownload: () => void cancelDownload(),

    run: (path, opts) => {
      const model = modelFor(get(), path);
      const key = cutoutKey(path, model);
      const current = get().byImage[key];
      const blocked = current?.status === "error" || current?.status === "needs-download" || current?.status === "low-memory";
      if (current && (!blocked || !opts?.retry)) return Promise.resolve();
      setState(key, { status: "running", stage: null });
      const job = queue.then(async () => {
        try {
          const outcome = await cutOut(path, model, (stage) => setState(key, { status: "running", stage }), opts?.force);
          if (outcome.kind === "done") setState(key, { status: "done", result: outcome.result });
          else if (outcome.kind === "download")
            setState(key, { status: "needs-download", file: outcome.file, sizeBytes: outcome.size_bytes });
          else
            setState(key, {
              status: "low-memory",
              availableBytes: outcome.available_bytes,
              neededBytes: outcome.needed_bytes,
            });
        } catch (e) {
          setState(key, { status: "error", error: String(e) });
        }
      });
      queue = job;
      return job;
    },
  };
});
