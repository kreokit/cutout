import { Download, MemoryStick, RotateCcw, TriangleAlert } from "lucide-react";
import { STAGE_LABELS, formatSeconds } from "../lib/cutout";
import { formatBytes } from "../lib/images";
import { CutoutModel, MODEL_HINTS, MODEL_NAMES, ModelFile, percent } from "../lib/models";
import { CutoutState, isDownloaded, useCutouts } from "../store/cutouts";
import { Button, ProgressBar, SectionLabel, Segmented } from "../ui";

const gb = (bytes: number) => `${(bytes / 1e9).toFixed(1)} GB`;

/** On the stage: what's happening with the cutout (download, progress, error). */
export function StageStatus({
  path,
  model,
  state,
  onRetry,
}: {
  path: string;
  model: CutoutModel;
  state: CutoutState | undefined;
  onRetry: () => void;
}) {
  const models = useCutouts((s) => s.models);
  if (models === null) return null;
  if (!isDownloaded(models, model)) return <DownloadCard model={model} />;
  if (state?.status === "needs-download") {
    return <DownloadCard model={model} file={state.file} sizeBytes={state.sizeBytes} onDone={onRetry} />;
  }
  if (state?.status === "low-memory") return <LowMemoryCard path={path} state={state} />;
  if (state?.status === "running") {
    return (
      <div className="stage-status" role="status">
        <span>{state.stage ? STAGE_LABELS[state.stage] : "Waiting…"}</span>
        <ProgressBar value={null} label="Removing the background" />
      </div>
    );
  }
  if (state?.status === "error") {
    return (
      <div className="stage-card" role="alert">
        <TriangleAlert size={20} strokeWidth={1.75} className="stage-card-warning" aria-hidden />
        <p className="stage-card-title">The background couldn't be removed</p>
        <p className="stage-card-text">{state.error}</p>
        <Button icon={RotateCcw} onClick={onRetry}>
          Try again
        </Button>
      </div>
    );
  }
  return null;
}

/** Not enough free memory for Best right now: Use Fast or Try anyway. */
function LowMemoryCard({ path, state }: { path: string; state: Extract<CutoutState, { status: "low-memory" }> }) {
  const { setModelFor, run } = useCutouts.getState();
  return (
    <div className="stage-card" role="alert">
      <MemoryStick size={20} strokeWidth={1.75} className="stage-card-warning" aria-hidden />
      <p className="stage-card-title">Not enough free memory for Best</p>
      <p className="stage-card-text">
        Best needs about {gb(state.neededBytes)} of free memory; {gb(state.availableBytes)} is free right now. Close other
        apps and try again, use Fast, or try anyway (the computer may become slow).
      </p>
      <div className="stage-card-row">
        <Button variant="primary" onClick={() => setModelFor(path, "fast")}>
          Use Fast
        </Button>
        <Button onClick={() => void run(path, { retry: true, force: true })}>Try anyway</Button>
      </div>
    </div>
  );
}

/**
 * Asks for a model download, then shows its progress: the model's own file (once), or
 * `file`, Best's smaller version for when free memory is low. `onDone` runs after it.
 */
export function DownloadCard({
  model,
  file,
  sizeBytes,
  onDone,
}: {
  model: CutoutModel;
  file?: ModelFile;
  sizeBytes?: number;
  onDone?: () => void;
}) {
  const info = useCutouts((s) => s.models?.find((m) => m.id === model));
  const download = useCutouts((s) => s.download);
  const error = useCutouts((s) => s.downloadError);
  const { startDownload, cancelDownload } = useCutouts.getState();
  const target = file ?? info?.file;
  const size = formatBytes(sizeBytes ?? info?.size_bytes ?? 0);
  const fallback = file !== undefined;
  const start = async () => {
    if (!target) return;
    await startDownload(target);
    if (useCutouts.getState().downloadError === null) onDone?.();
  };
  return (
    <div className="stage-card">
      <p className="stage-card-title">{fallback ? "Download the smaller Best model" : "Download the AI model"}</p>
      <p className="stage-card-text">
        {fallback
          ? `There isn't enough free memory for the usual Best model right now. A smaller version (${size}) needs less and is downloaded once.`
          : `The ${MODEL_NAMES[model]} model (${size}) is downloaded once. After that, everything runs on this computer, even offline.`}
      </p>
      {download ? (
        <div className="stage-card-progress">
          <ProgressBar value={download.progress ? percent(download.progress) : null} label="Downloading the AI model" />
          <div className="stage-card-row">
            <span>
              {download.progress
                ? `${formatBytes(download.progress.downloaded_bytes)} of ${formatBytes(download.progress.total_bytes)}`
                : "Starting…"}
            </span>
            <Button variant="ghost" size="sm" onClick={cancelDownload}>
              Cancel
            </Button>
          </div>
        </div>
      ) : (
        <Button variant="primary" icon={Download} onClick={() => void start()}>
          Download ({size})
        </Button>
      )}
      {error && !download && <p className="stage-card-error">{error}</p>}
    </div>
  );
}

/** Inspector section: which model, and how long the cutout took. */
export function CutoutSection({
  path,
  model,
  state,
}: {
  path: string;
  model: CutoutModel;
  state: CutoutState | undefined;
}) {
  const ready = useCutouts((s) => isDownloaded(s.models, model));
  let status = "Not downloaded yet";
  if (ready) {
    if (state?.status === "running") status = "Working…";
    else if (state?.status === "error") status = "Failed";
    else if (state?.status === "low-memory") status = "Not enough memory";
    else if (state?.status === "needs-download") status = "Needs a download";
    else if (state?.status === "done")
      status = state.result.cached ? "Done (saved earlier)" : `Done in ${formatSeconds(state.result.timings.total)}`;
    else status = "Waiting";
  }
  return (
    <section className="inspector-section">
      <SectionLabel>Background removal</SectionLabel>
      <Segmented<CutoutModel>
        label="Model"
        value={model}
        onChange={(m) => useCutouts.getState().setModelFor(path, m)}
        disabled={state?.status === "running"}
        options={[
          { value: "fast", label: "Fast", hint: MODEL_HINTS.fast },
          { value: "best", label: "Best", hint: MODEL_HINTS.best },
        ]}
      />
      <dl className="details">
        <dt>Status</dt>
        <dd>{status}</dd>
      </dl>
      <p className="ui-field-hint">Each model keeps its own edits for this image.</p>
    </section>
  );
}
