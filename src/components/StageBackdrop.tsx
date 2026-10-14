import { useEffect, useRef, useState } from "react";
import { Background, stageBackground } from "../lib/background";
import { CutoutModel } from "../lib/models";
import { Edits } from "../lib/refine";
import { toast } from "../ui";

/**
 * The background under the cutout on the stage, stretched over the image: a colour, or
 * the blur / image rendered by the backend (the same functions as the export). Requests
 * are "latest wins"; the blur follows the committed edits (it leaves out the subject).
 */
export function StageBackdrop({
  path,
  model,
  edits,
  background,
}: {
  path: string;
  model: CutoutModel;
  edits: Edits;
  background: Background;
}) {
  if (background.kind === "transparent") return null;
  if (background.kind === "color") return <div className="stage-backdrop" style={{ background: background.color }} />;
  return <RenderedBackdrop path={path} model={model} edits={edits} background={background} />;
}

interface Request {
  edits: Edits;
  background: Background;
}

function RenderedBackdrop({ path, model, edits, background }: { path: string; model: CutoutModel; edits: Edits; background: Background }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const [ready, setReady] = useState(false);
  const busy = useRef(false);
  const waiting = useRef<Request | null>(null);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  const pump = async () => {
    if (busy.current || !waiting.current) return;
    const req = waiting.current;
    waiting.current = null;
    busy.current = true;
    try {
      const r = await stageBackground(path, model, req.edits, req.background);
      const c = canvas.current;
      const ctx = c?.getContext("2d");
      if (!alive.current || !c || !ctx) return;
      c.width = r.width;
      c.height = r.height;
      ctx.putImageData(new ImageData(r.pixels, r.width, r.height), 0, 0);
      setReady(true);
    } catch (e) {
      if (alive.current) toast(String(e), "error");
    } finally {
      busy.current = false;
      if (alive.current) void pump();
    }
  };

  // The image background doesn't depend on the edits; don't re-render it for each stroke.
  const editsKey = background.kind === "blur" ? edits : null;
  const backgroundKey = JSON.stringify(background);
  useEffect(() => {
    waiting.current = { edits, background };
    void pump();
    // pump reads refs only; re-run when what's shown changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [path, model, editsKey, backgroundKey]);

  return <canvas ref={canvas} className="stage-backdrop" style={{ visibility: ready ? "visible" : "hidden" }} />;
}
