import { beforeEach, describe, expect, it, vi } from "vitest";
import { CutoutResult } from "../lib/cutout";
import { ModelInfo } from "../lib/models";

type Call = { cmd: string; args: Record<string, unknown> };
const calls: Call[] = [];
const handlers: Record<string, (args: Record<string, unknown>) => unknown> = {};
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string, args: Record<string, unknown> = {}) => {
    calls.push({ cmd, args });
    return handlers[cmd]?.(args);
  }),
  Channel: class {
    onmessage: (m: unknown) => void = () => {};
  },
}));

const { useCutouts, cutoutKey, isDownloaded, modelFor } = await import("./cutouts");

const result = (path: string): CutoutResult => ({
  cutout: `${path}.cut.png`,
  mask: `${path}.mask.png`,
  model: "fast",
  cached: false,
  timings: { load_model: 0, decode: 0, preprocess: 0, inference: 0.8, postprocess: 0, save: 0, total: 1 },
});

const models = (downloaded: boolean): ModelInfo[] => [
  { id: "fast", size_bytes: 175_997_641, downloaded },
  { id: "best", size_bytes: 224_005_088, downloaded: false },
];

describe("cutouts store", () => {
  beforeEach(() => {
    calls.length = 0;
    for (const k of Object.keys(handlers)) delete handlers[k];
    useCutouts.setState({ model: "fast", modelOf: {}, models: null, download: null, downloadError: null, byImage: {} });
  });

  it("cuts out each image once and keeps the result", async () => {
    handlers.cut_out = (args) => result(args.path as string);
    const { run } = useCutouts.getState();
    await Promise.all([run("/a.png"), run("/a.png"), run("/b.png")]);
    expect(calls.filter((c) => c.cmd === "cut_out").map((c) => c.args.path)).toEqual(["/a.png", "/b.png"]);
    const state = useCutouts.getState().byImage[cutoutKey("/a.png", "fast")];
    expect(state).toEqual({ status: "done", result: result("/a.png") });
  });

  it("records errors and retries only when asked", async () => {
    handlers.cut_out = () => {
      throw "the AI model failed";
    };
    await useCutouts.getState().run("/a.png");
    expect(useCutouts.getState().byImage[cutoutKey("/a.png", "fast")]).toEqual({
      status: "error",
      error: "the AI model failed",
    });
    await useCutouts.getState().run("/a.png");
    expect(calls.filter((c) => c.cmd === "cut_out")).toHaveLength(1);
    handlers.cut_out = (args) => result(args.path as string);
    await useCutouts.getState().run("/a.png", { retry: true });
    expect(useCutouts.getState().byImage[cutoutKey("/a.png", "fast")].status).toBe("done");
  });

  it("downloads a model and refreshes the list", async () => {
    let listed = false;
    handlers.list_models = () => models(listed);
    handlers.download_model = () => {
      listed = true;
    };
    await useCutouts.getState().refreshModels();
    expect(isDownloaded(useCutouts.getState().models, "fast")).toBe(false);
    await useCutouts.getState().startDownload("fast");
    expect(isDownloaded(useCutouts.getState().models, "fast")).toBe(true);
    expect(useCutouts.getState().download).toBeNull();
    expect(useCutouts.getState().downloadError).toBeNull();
  });

  it("shows download errors but not a cancel", async () => {
    handlers.download_model = () => {
      throw "model download failed: offline";
    };
    await useCutouts.getState().startDownload("fast");
    expect(useCutouts.getState().downloadError).toBe("model download failed: offline");
    handlers.download_model = () => {
      throw "cancelled";
    };
    await useCutouts.getState().startDownload("fast");
    expect(useCutouts.getState().downloadError).toBeNull();
  });

  it("uses each image's own model", async () => {
    handlers.cut_out = (args) => result(args.path as string);
    const { run, setModelFor } = useCutouts.getState();
    setModelFor("/b.png", "best");
    expect(modelFor(useCutouts.getState(), "/a.png")).toBe("fast");
    expect(modelFor(useCutouts.getState(), "/b.png")).toBe("best");
    await run("/a.png");
    await run("/b.png");
    expect(calls.filter((c) => c.cmd === "cut_out").map((c) => c.args.model)).toEqual(["fast", "best"]);
    expect(useCutouts.getState().byImage[cutoutKey("/b.png", "best")]?.status).toBe("done");
  });
});
