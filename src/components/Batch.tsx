import { useEffect, useState } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { RotateCcw, TriangleAlert } from "lucide-react";
import { STAGE_LABELS } from "../lib/cutout";
import { ImageInfo, fileName } from "../lib/images";
import { CutoutModel, MODEL_NAMES } from "../lib/models";
import { DEFAULT_EDGES, Edits } from "../lib/refine";
import { CutoutState, cutoutKey, isDownloaded, modelFor, useCutouts } from "../store/cutouts";
import { historyOf, useEdits } from "../store/edits";
import { openPaths, useSession } from "../store/session";
import { Button, ProgressBar } from "../ui";
import { DownloadCard } from "./CutoutStatus";
import { TopBar } from "./TopBar";
import "../styles/batch.css";

const isEdited = (e: Edits) =>
  e.strokes.length > 0 ||
  e.edges.feather !== DEFAULT_EDGES.feather ||
  e.edges.shift !== DEFAULT_EDGES.shift ||
  e.edges.decontaminate !== DEFAULT_EDGES.decontaminate;

/** A cached thumbnail from the backend: the cutout once it's done, else the original. */
function useThumbnail(path: string, model: CutoutModel, done: boolean): string | null {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    let cancelled = false;
    const load = done
      ? invoke<string>("cutout_thumbnail", { path, model })
      : invoke<string>("image_thumbnail", { path });
    load.then(
      (file) => !cancelled && setSrc(convertFileSrc(file)),
      () => {},
    );
    return () => {
      cancelled = true;
    };
  }, [path, model, done]);
  return src;
}

function statusText(state: CutoutState | undefined, ready: boolean): string {
  if (!ready) return "Needs the model";
  if (!state) return "Waiting";
  if (state.status === "running") return state.stage ? STAGE_LABELS[state.stage] : "Waiting";
  if (state.status === "error") return "Failed";
  return "Done";
}

function Tile({ image, index }: { image: ImageInfo; index: number }) {
  const model = useCutouts((s) => modelFor(s, image.path));
  const ready = useCutouts((s) => isDownloaded(s.models, model));
  const key = cutoutKey(image.path, model);
  const state = useCutouts((s) => s.byImage[key]);
  const edited = useEdits((s) => isEdited(historyOf(s.byImage, key).edits));
  const done = state?.status === "done";
  const thumb = useThumbnail(image.path, model, done);
  const running = state?.status === "running";
  const failed = state?.status === "error";

  return (
    <li className="batch-tile" data-status={done ? "done" : failed ? "failed" : running ? "running" : "waiting"}>
      <button
        type="button"
        className="batch-tile-open"
        onClick={() => useSession.getState().edit(index)}
        aria-label={`Open ${fileName(image.path)}`}
      >
        <span className={done ? "batch-thumb checkerboard-flat" : "batch-thumb"}>
          {thumb && <img src={thumb} alt="" draggable={false} loading="lazy" />}
          {running && (
            <span className="batch-thumb-progress">
              <ProgressBar value={null} label={`Removing the background of ${fileName(image.path)}`} />
            </span>
          )}
          {failed && <TriangleAlert className="batch-thumb-warning" size={20} strokeWidth={1.75} aria-hidden />}
        </span>
      </button>
      <div className="batch-tile-info">
        <span className="batch-tile-name" title={image.path}>
          {fileName(image.path)}
        </span>
        <span className="batch-tile-meta">
          <span className="batch-tile-status">{statusText(state, ready)}</span>
          {model === "best" && <span className="batch-badge">{MODEL_NAMES.best}</span>}
          {edited && <span className="batch-badge">Edited</span>}
        </span>
        {failed && (
          <div className="batch-tile-error">
            <span title={state.error}>{state.error}</span>
            <Button
              variant="ghost"
              size="sm"
              icon={RotateCcw}
              onClick={() => void useCutouts.getState().run(image.path, { retry: true })}
            >
              Retry
            </Button>
          </div>
        )}
      </div>
    </li>
  );
}

/** Batch: every opened image with its cutout status; click one to edit it. */
export function Batch() {
  const images = useSession((s) => s.images);
  const close = useSession((s) => s.close);
  const model = useCutouts((s) => s.model);
  const models = useCutouts((s) => s.models);
  const modelOf = useCutouts((s) => s.modelOf);
  const byImage = useCutouts((s) => s.byImage);
  const run = useCutouts((s) => s.run);
  const done = images.filter((i) => byImage[cutoutKey(i.path, modelFor({ model, modelOf }, i.path))]?.status === "done").length;

  // Cut out every image whose model is downloaded, one after another.
  useEffect(() => {
    for (const image of images) {
      if (isDownloaded(models, modelFor({ model, modelOf }, image.path))) void run(image.path);
    }
  }, [images, models, model, modelOf, run]);

  // Dropping images here replaces the batch.
  useEffect(() => {
    const unlisten = getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === "drop") void openPaths(event.payload.paths);
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  return (
    <div className="batch">
      <TopBar
        onHome={close}
        title={
          <span className="topbar-name" data-tauri-drag-region>
            {images.length} images
            <span className="batch-count">
              {" "}
              · {done} of {images.length} cut out
            </span>
          </span>
        }
      >
      </TopBar>
      <main className="batch-main">
        {models !== null && !isDownloaded(models, model) && (
          <div className="batch-notice">
            <DownloadCard model={model} />
          </div>
        )}
        <ul className="batch-grid" aria-label="Images">
          {images.map((image, i) => (
            <Tile key={image.path} image={image} index={i} />
          ))}
        </ul>
      </main>
    </div>
  );
}
