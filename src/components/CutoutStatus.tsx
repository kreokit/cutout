import { Download, RotateCcw, TriangleAlert } from "lucide-react";
import { STAGE_LABELS, formatSeconds } from "../lib/cutout";
import { formatBytes } from "../lib/images";
import { CutoutModel, MODEL_NAMES, percent } from "../lib/models";
import { CutoutState, isDownloaded, useCutouts } from "../store/cutouts";
import { Button, ProgressBar, SectionLabel } from "../ui";

/** On the stage: what's happening with the cutout (download, progress, error). */
export function StageStatus({
  model,
  state,
  onRetry,
}: {
  model: CutoutModel;
  state: CutoutState | undefined;
  onRetry: () => void;
}) {
  const models = useCutouts((s) => s.models);
  if (models === null) return null;
  if (!isDownloaded(models, model)) return <DownloadCard model={model} />;
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

/** Asks for the one-time model download, then shows its progress. */
export function DownloadCard({ model }: { model: CutoutModel }) {
  const info = useCutouts((s) => s.models?.find((m) => m.id === model));
  const download = useCutouts((s) => s.download);
  const error = useCutouts((s) => s.downloadError);
  const { startDownload, cancelDownload } = useCutouts.getState();
  const size = info ? formatBytes(info.size_bytes) : "";
  return (
    <div className="stage-card">
      <p className="stage-card-title">Download the AI model</p>
      <p className="stage-card-text">
        The {MODEL_NAMES[model]} model ({size}) is downloaded once. After that, everything runs on this computer, even
        offline.
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
        <Button variant="primary" icon={Download} onClick={() => void startDownload(model)}>
          Download ({size})
        </Button>
      )}
      {error && !download && <p className="stage-card-error">{error}</p>}
    </div>
  );
}

/** Inspector section: which model, and how long the cutout took. */
export function CutoutSection({ model, state }: { model: CutoutModel; state: CutoutState | undefined }) {
  const ready = useCutouts((s) => isDownloaded(s.models, model));
  let status = "Not downloaded yet";
  if (ready) {
    if (state?.status === "running") status = "Working…";
    else if (state?.status === "error") status = "Failed";
    else if (state?.status === "done")
      status = state.result.cached ? "Done (saved earlier)" : `Done in ${formatSeconds(state.result.timings.total)}`;
    else status = "Waiting";
  }
  return (
    <section className="inspector-section">
      <SectionLabel>Background removal</SectionLabel>
      <dl className="details">
        <dt>Model</dt>
        <dd>{MODEL_NAMES[model]}</dd>
        <dt>Status</dt>
        <dd>{status}</dd>
      </dl>
    </section>
  );
}
