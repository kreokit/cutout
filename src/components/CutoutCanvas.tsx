import { useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { CutoutModel } from "../lib/models";
import { EdgeSettings, Edits, Render, renderRefined } from "../lib/refine";
import { toast } from "../ui";

interface Request {
  edits: Edits;
  preview: boolean;
}

/**
 * The refined cutout, rendered by the backend into a canvas the size of the working copy.
 * Edits are sent as they change ("latest wins": while one render runs, only the newest
 * waiting request is kept). While a slider moves, small previews are stretched over the
 * canvas; the full render follows when it's released. Until the first render arrives,
 * the plain cutout image is shown.
 */
export function CutoutCanvas({
  path,
  model,
  cutoutFile,
  edits,
  live,
  canvasRef,
}: {
  path: string;
  model: CutoutModel;
  /** The unrefined cutout PNG, shown until the canvas is ready. */
  cutoutFile: string;
  edits: Edits;
  /** Edge settings of a slider being dragged. */
  live: EdgeSettings | undefined;
  canvasRef?: React.RefObject<HTMLCanvasElement | null>;
}) {
  const own = useRef<HTMLCanvasElement>(null);
  const canvas = canvasRef ?? own;
  const [ready, setReady] = useState(false);
  const busy = useRef(false);
  const waiting = useRef<Request | null>(null);
  /** The canvas shows a stretched preview (or nothing yet): the next render must be full. */
  const needFull = useRef(true);
  const alive = useRef(true);
  useEffect(() => {
    // Set on every mount: StrictMode mounts, unmounts and mounts again in development.
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  const draw = (r: Render, preview: boolean) => {
    const c = canvas.current;
    const ctx = c?.getContext("2d");
    if (!c || !ctx) return;
    if (preview) {
      if (c.width === 0) return; // no full render yet to stretch over
      const small = document.createElement("canvas");
      small.width = r.width;
      small.height = r.height;
      small.getContext("2d")!.putImageData(new ImageData(r.pixels, r.width, r.height), 0, 0);
      ctx.clearRect(0, 0, c.width, c.height);
      ctx.imageSmoothingQuality = "high";
      ctx.drawImage(small, 0, 0, c.width, c.height);
      return;
    }
    if (c.width !== r.width || c.height !== r.height) {
      c.width = r.width;
      c.height = r.height;
    }
    if (r.regionWidth > 0 && r.regionHeight > 0) {
      ctx.putImageData(new ImageData(r.pixels, r.regionWidth, r.regionHeight), r.x, r.y);
    }
    setReady(true);
  };

  const pump = async () => {
    if (busy.current || !waiting.current) return;
    const req = waiting.current;
    waiting.current = null;
    busy.current = true;
    try {
      const full = !req.preview && needFull.current;
      const r = await renderRefined(path, model, req.edits, { preview: req.preview, full });
      if (!alive.current) return;
      draw(r, req.preview);
      needFull.current = req.preview || (needFull.current && !full);
    } catch (e) {
      if (alive.current) toast(String(e), "error");
    } finally {
      busy.current = false;
      if (alive.current) void pump();
    }
  };

  useEffect(() => {
    waiting.current = live ? { edits: { ...edits, edges: live }, preview: true } : { edits, preview: false };
    void pump();
    // pump reads refs only; re-run when the edits change.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [edits, live]);

  return (
    <>
      {!ready && <img src={convertFileSrc(cutoutFile)} alt="" draggable={false} />}
      <canvas ref={canvas} style={{ visibility: ready ? "visible" : "hidden" }} />
    </>
  );
}
