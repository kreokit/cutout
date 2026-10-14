import { useEffect, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { ChevronLeft, ChevronRight, Download, Redo2, Undo2 } from "lucide-react";
import { fileName, formatBytes } from "../lib/images";
import { closeRefine } from "../lib/refine";
import { cutoutKey, isDownloaded, useCutouts } from "../store/cutouts";
import { historyOf, useEdits } from "../store/edits";
import { openPaths, useSession } from "../store/session";
import { Button, IconButton, SectionLabel, Segmented, Tooltip } from "../ui";
import { CutoutCanvas } from "./CutoutCanvas";
import { CutoutSection, StageStatus } from "./CutoutStatus";
import { EdgesSection } from "./EdgesSection";
import { Stage, ViewMode } from "./Stage";
import { TopBar } from "./TopBar";
import "../styles/editor.css";

/** Editor: the image on a checkerboard, with its details in the inspector. */
export function Editor() {
  const images = useSession((s) => s.images);
  const current = useSession((s) => s.current);
  const show = useSession((s) => s.show);
  const close = useSession((s) => s.close);
  const image = images[current];
  const model = useCutouts((s) => s.model);
  const ready = useCutouts((s) => isDownloaded(s.models, model));
  const state = useCutouts((s) => (image ? s.byImage[cutoutKey(image.path, model)] : undefined));
  const run = useCutouts((s) => s.run);
  const [mode, setMode] = useState<ViewMode>("after");
  const editKey = image ? cutoutKey(image.path, model) : "";
  const history = useEdits((s) => historyOf(s.byImage, editKey));
  const live = useEdits((s) => s.live[editKey]);
  const { undo, redo } = useEdits.getState();
  const done = state?.status === "done";

  // ⌘Z undo, ⇧⌘Z (or Ctrl+Y) redo.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.altKey) return;
      if ((e.target as HTMLElement | null)?.closest("input, textarea, [contenteditable]")) return;
      const key = e.key.toLowerCase();
      if (key === "z" && !e.shiftKey) {
        e.preventDefault();
        undo(editKey);
      } else if ((key === "z" && e.shiftKey) || (key === "y" && e.ctrlKey)) {
        e.preventDefault();
        redo(editKey);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [editKey, undo, redo]);

  // Cut out the shown image as soon as the model is there.
  useEffect(() => {
    if (image && ready) void run(image.path);
  }, [image, ready, run]);

  // ← / → step through a batch.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || (e.key !== "ArrowLeft" && e.key !== "ArrowRight")) return;
      if ((e.target as HTMLElement | null)?.closest("input, textarea, [role=slider]")) return;
      show(useSession.getState().current + (e.key === "ArrowLeft" ? -1 : 1));
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [show]);

  // Dropping images here replaces the current ones.
  useEffect(() => {
    const unlisten = getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === "drop") void openPaths(event.payload.paths);
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  if (!image) return null;
  const name = fileName(image.path);

  return (
    <div className="editor">
      <TopBar
        onHome={() => {
          close();
          void closeRefine();
        }}
        title={
          <>
            <span className="topbar-name" title={image.path} data-tauri-drag-region>
              {name}
            </span>
            {images.length > 1 && (
              <span className="batch-nav">
                <IconButton label="Previous image" icon={ChevronLeft} size="sm" disabled={current === 0} onClick={() => show(current - 1)} />
                <span className="batch-count">
                  {current + 1} of {images.length}
                </span>
                <IconButton
                  label="Next image"
                  icon={ChevronRight}
                  size="sm"
                  disabled={current === images.length - 1}
                  onClick={() => show(current + 1)}
                />
              </span>
            )}
          </>
        }
      >
        <span className="history-buttons">
          <IconButton
            label="Undo"
            shortcut="⌘Z"
            icon={Undo2}
            disabled={history.past.length === 0}
            onClick={() => undo(editKey)}
          />
          <IconButton
            label="Redo"
            shortcut="⇧⌘Z"
            icon={Redo2}
            disabled={history.future.length === 0}
            onClick={() => redo(editKey)}
          />
        </span>
        <Tooltip label="Export comes with the cutout in the next alpha">
          <span className="export-slot">
            <Button variant="primary" icon={Download} disabled>
              Export
            </Button>
          </span>
        </Tooltip>
      </TopBar>

      <div className="editor-main">
        <div className="stage-area">
          <div className="view-switch">
            <Segmented
              label="View"
              value={mode}
              onChange={setMode}
              disabled={!done}
              options={[
                { value: "before", label: "Before" },
                { value: "after", label: "After" },
                { value: "compare", label: "Compare" },
              ]}
            />
          </div>
          <Stage
            key={image.path}
            original={convertFileSrc(image.path)}
            cutout={
              state?.status === "done" ? (
                <CutoutCanvas
                  key={editKey}
                  path={image.path}
                  model={model}
                  cutoutFile={state.result.cutout}
                  edits={history.edits}
                  live={live}
                />
              ) : null
            }
            mode={mode}
            size={image}
            status={<StageStatus state={state} onRetry={() => void run(image.path, { retry: true })} />}
          />
        </div>
        <aside className="inspector" aria-label="Inspector">
          <section className="inspector-section">
            <SectionLabel>Image</SectionLabel>
            <dl className="details">
              <dt>Size</dt>
              <dd>
                {image.width} × {image.height} px
              </dd>
              <dt>Format</dt>
              <dd>{image.format === "jpeg" ? "JPG" : image.format.toUpperCase()}</dd>
              <dt>File</dt>
              <dd>{formatBytes(image.bytes)}</dd>
            </dl>
          </section>
          <CutoutSection state={state} />
          <EdgesSection editKey={editKey} disabled={!done} />
        </aside>
      </div>
    </div>
  );
}
