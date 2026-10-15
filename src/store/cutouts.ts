import { create } from "zustand";
import { CutoutResult, Stage, cutOut } from "../lib/cutout";
import {
  CANCELLED,
  CutoutModel,
  DownloadProgress,
  ModelInfo,
  cancelDownload,
  downloadModel,
  listModels,
} from "../lib/models";

export type CutoutState =
  | { status: "running"; stage: Stage | null }
  | { status: "done"; result: CutoutResult }
  | { status: "error"; error: string };

interface Cutouts {
  /** The model new cutouts use (picked on Home). */
  model: CutoutModel;
  /** The model chosen for one image (in the editor); others use `model`. */
  modelOf: Record<string, CutoutModel>;
  /** Model files and whether they're downloaded; null until loaded. */
  models: ModelInfo[] | null;
  download: { model: CutoutModel; progress: DownloadProgress | null } | null;
  downloadError: string | null;
  /** Cutouts by `cutoutKey(path, model)`. */
  byImage: Record<string, CutoutState>;
  setModel: (model: CutoutModel) => void;
  /** Chooses the model for the image at `path` only. */
  setModelFor: (path: string, model: CutoutModel) => void;
  refreshModels: () => Promise<void>;
  startDownload: (model: CutoutModel) => Promise<void>;
  cancelDownload: () => void;
  /** Cuts out `path` with its model unless it's done or running. */
  run: (path: string, opts?: { retry?: boolean }) => Promise<void>;
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

    startDownload: async (model) => {
      if (get().download) return;
      set({ download: { model, progress: null }, downloadError: null });
      try {
        await downloadModel(model, (progress) => set({ download: { model, progress } }));
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
      if (current && (current.status !== "error" || !opts?.retry)) return Promise.resolve();
      setState(key, { status: "running", stage: null });
      const job = queue.then(async () => {
        try {
          const result = await cutOut(path, model, (stage) => setState(key, { status: "running", stage }));
          setState(key, { status: "done", result });
        } catch (e) {
          setState(key, { status: "error", error: String(e) });
        }
      });
      queue = job;
      return job;
    },
  };
});
