import { useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { FolderOpen } from "lucide-react";
import { Background } from "../lib/background";
import {
  BatchItem,
  ItemResult,
  batchConflicts,
  batchSummary,
  exportBatch,
  revealExport,
  stopBatch,
} from "../lib/export";
import { ImageInfo, fileName } from "../lib/images";
import { cutoutKey, isDownloaded, modelFor, useCutouts } from "../store/cutouts";
import { historyOf, useEdits } from "../store/edits";
import { useExportSettings } from "../store/exportSettings";
import { Button, Dialog, ProgressBar, toast } from "../ui";
import { ExportPreview, ExportSettingsFields } from "./ExportDialog";

type Status =
  | { kind: "idle" }
  | { kind: "conflicts"; folder: string; files: string[] }
  | { kind: "cutting"; done: number; total: number }
  | { kind: "exporting"; index: number; total: number }
  | { kind: "finished"; results: ItemResult[] };

/** Resolves once no image of `paths` is waiting for or running its cutout. */
function cutoutsSettled(paths: string[]): Promise<void> {
  const settled = () => {
    const s = useCutouts.getState();
    return paths.every((p) => {
      const model = modelFor(s, p);
      if (!isDownloaded(s.models, model)) return true;
      const state = s.byImage[cutoutKey(p, model)];
      return state !== undefined && state.status !== "running";
    });
  };
  if (settled()) return Promise.resolve();
  return new Promise((resolve) => {
    const unsubscribe = useCutouts.subscribe(() => {
      if (settled()) {
        unsubscribe();
        resolve();
      }
    });
  });
}

const doneCount = (paths: string[]) => {
  const s = useCutouts.getState();
  return paths.filter((p) => s.byImage[cutoutKey(p, modelFor(s, p))]?.status === "done").length;
};

/**
 * "Export all": the same settings for every image, into one folder. Existing files are
 * listed first (Replace / Keep both / Cancel); images not cut out yet are cut out first.
 */
export function BatchExportDialog({
  images,
  background,
  onClose,
}: {
  images: ImageInfo[];
  background: Background;
  onClose: () => void;
}) {
  const settings = useExportSettings((s) => s.settings);
  const [status, setStatus] = useState<Status>({ kind: "idle" });
  const [stopped, setStopped] = useState(false);
  // Read after awaits (the state value in this closure would be stale).
  const stopRequested = useRef(false);
  const paths = images.map((i) => i.path);
  // The preview shows the first image that is cut out.
  const preview = useCutouts((s) => images.find((i) => s.byImage[cutoutKey(i.path, modelFor(s, i.path))]?.status === "done"));
  const previewModel = useCutouts((s) => (preview ? modelFor(s, preview.path) : "fast"));
  const previewEdits = useEdits((s) => (preview ? historyOf(s.byImage, cutoutKey(preview.path, previewModel)).edits : null));

  const pickFolder = async () => {
    const first = images[0]?.path ?? "";
    const folder = await open({
      directory: true,
      multiple: false,
      defaultPath: first.slice(0, Math.max(first.lastIndexOf("/"), first.lastIndexOf("\\")) + 1) || undefined,
    });
    if (typeof folder !== "string") return;
    try {
      const files = await batchConflicts(folder, paths, settings.format);
      if (files.length) setStatus({ kind: "conflicts", folder, files });
      else await run(folder, false);
    } catch (e) {
      toast(String(e), "error");
    }
  };

  const run = async (folder: string, replace: boolean) => {
    setStopped(false);
    stopRequested.current = false;
    // Cut out what's missing first (cutouts already running are waited for).
    const { run: cut } = useCutouts.getState();
    for (const p of paths) void cut(p);
    setStatus({ kind: "cutting", done: doneCount(paths), total: paths.length });
    const unsubscribe = useCutouts.subscribe(() =>
      setStatus((s) => (s.kind === "cutting" ? { ...s, done: doneCount(paths) } : s)),
    );
    await cutoutsSettled(paths);
    unsubscribe();
    // Stopped while cutting out: the backend's stop flag is reset when the export starts.
    if (stopRequested.current) {
      setStatus({ kind: "finished", results: paths.map((path) => ({ status: "skipped", path })) });
      return;
    }

    const cutouts = useCutouts.getState();
    const edits = useEdits.getState();
    const items: BatchItem[] = paths.map((path) => {
      const model = modelFor(cutouts, path);
      return { path, model, edits: historyOf(edits.byImage, cutoutKey(path, model)).edits };
    });
    setStatus({ kind: "exporting", index: 0, total: items.length });
    try {
      const results = await exportBatch(items, background, settings, folder, replace, (p) => {
        if (p.kind === "started") setStatus({ kind: "exporting", index: p.index, total: p.total });
      });
      setStatus({ kind: "finished", results });
    } catch (e) {
      toast(String(e), "error");
      setStatus({ kind: "idle" });
    }
  };

  if (status.kind === "finished") {
    const saved = status.results.filter((r) => r.status === "saved");
    const failed = status.results.filter((r): r is Extract<ItemResult, { status: "failed" }> => r.status === "failed");
    return (
      <Dialog
        title="Export all"
        onClose={onClose}
        footer={
          <>
            {saved[0]?.status === "saved" && (
              <Button
                icon={FolderOpen}
                onClick={() => {
                  const first = saved[0];
                  if (first.status === "saved") void revealExport(first.dest).catch((e) => toast(String(e), "error"));
                }}
              >
                Show in folder
              </Button>
            )}
            <Button variant="primary" onClick={onClose}>
              Done
            </Button>
          </>
        }
      >
        <p>{batchSummary(status.results)}.</p>
        {failed.length > 0 && (
          <ul className="batch-export-failures">
            {failed.map((f) => (
              <li key={f.path}>
                <strong>{fileName(f.path)}</strong>: {f.error}
              </li>
            ))}
          </ul>
        )}
      </Dialog>
    );
  }

  if (status.kind === "conflicts") {
    const shown = status.files.slice(0, 5);
    return (
      <Dialog
        title="Some files already exist"
        onClose={() => setStatus({ kind: "idle" })}
        footer={
          <>
            <Button onClick={() => setStatus({ kind: "idle" })}>Cancel</Button>
            <Button onClick={() => void run(status.folder, false)}>Keep both</Button>
            <Button variant="primary" onClick={() => void run(status.folder, true)}>
              Replace
            </Button>
          </>
        }
      >
        <p>
          {status.files.length === 1 ? "This file is" : `These ${status.files.length} files are`} already in the
          folder:
        </p>
        <ul className="batch-export-conflicts">
          {shown.map((f) => (
            <li key={f}>{fileName(f)}</li>
          ))}
          {status.files.length > shown.length && <li>and {status.files.length - shown.length} more</li>}
        </ul>
        <p className="ui-field-hint">Keep both saves the new files with -2, -3… added to their names.</p>
      </Dialog>
    );
  }

  const busy = status.kind === "cutting" || status.kind === "exporting";
  return (
    <Dialog
      title={`Export all (${images.length} images)`}
      width={600}
      dismissible={!busy}
      onClose={onClose}
      footer={
        busy ? (
          <Button
            disabled={stopped}
            onClick={() => {
              setStopped(true);
              stopRequested.current = true;
              void stopBatch();
            }}
          >
            {stopped ? "Stopping…" : "Stop"}
          </Button>
        ) : (
          <>
            <Button onClick={onClose}>Cancel</Button>
            <Button variant="primary" icon={FolderOpen} onClick={() => void pickFolder()}>
              Choose folder…
            </Button>
          </>
        )
      }
    >
      <div className="export-body">
        {preview && previewEdits ? (
          <ExportPreview image={preview} model={previewModel} edits={previewEdits} background={background} settings={settings} />
        ) : (
          <div className="export-preview">
            <p className="ui-field-hint">The preview appears once an image is cut out.</p>
          </div>
        )}
        <div className="export-controls">
          <ExportSettingsFields originalSize="each image's own" background={background} disabled={busy} tooBigForWebp={false} />
          <p className="ui-field-hint">
            Each image is saved as name-cutout.{settings.format} with its own edits. Nothing is replaced without asking.
          </p>
          {status.kind === "cutting" && (
            <div className="batch-export-progress">
              <span>
                Cutting out: {status.done} of {status.total} done
              </span>
              <ProgressBar value={status.total ? Math.round((status.done / status.total) * 100) : null} label="Cutting out" />
            </div>
          )}
          {status.kind === "exporting" && (
            <div className="batch-export-progress">
              <span>
                Exporting {status.index + 1} of {status.total}
              </span>
              <ProgressBar value={Math.round((status.index / status.total) * 100)} label="Exporting" />
            </div>
          )}
        </div>
      </div>
    </Dialog>
  );
}
