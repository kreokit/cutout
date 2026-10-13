import { beforeEach, describe, expect, it, vi } from "vitest";
import { ImageInfo, Opened } from "../lib/images";

const opened: { value: Opened } = { value: { images: [], errors: [] } };
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => opened.value) }));
const toasts: string[] = [];
vi.mock("../ui", () => ({ toast: (message: string) => toasts.push(message) }));

const { openPaths, useSession } = await import("./session");

const image = (path: string): ImageInfo => ({ path, width: 10, height: 10, format: "png", bytes: 100 });

describe("openPaths", () => {
  beforeEach(() => {
    useSession.getState().close();
    toasts.length = 0;
  });

  it("opens the good images and reports the rest", async () => {
    opened.value = { images: [image("/a.png"), image("/b.png")], errors: ["Choose a PNG, JPG or WebP image."] };
    expect(await openPaths(["/a.png", "/x.gif", "/b.png"])).toBe(true);
    expect(useSession.getState().images.map((i) => i.path)).toEqual(["/a.png", "/b.png"]);
    expect(useSession.getState().current).toBe(0);
    expect(toasts).toEqual(["Choose a PNG, JPG or WebP image."]);
  });

  it("keeps the current session when nothing opens, and caps the toasts", async () => {
    useSession.setState({ images: [image("/keep.png")], current: 0 });
    opened.value = { images: [], errors: ["e1", "e2", "e3", "e4", "e5"] };
    expect(await openPaths(["/1", "/2", "/3", "/4", "/5"])).toBe(false);
    expect(useSession.getState().images).toHaveLength(1);
    expect(toasts).toEqual(["e1", "e2", "e3", "2 more files couldn't be opened."]);
  });

  it("clamps the shown image to the batch", () => {
    useSession.setState({ images: [image("/a.png"), image("/b.png")], current: 0 });
    useSession.getState().show(5);
    expect(useSession.getState().current).toBe(1);
    useSession.getState().show(-1);
    expect(useSession.getState().current).toBe(0);
  });
});
