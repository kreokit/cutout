import { useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { ChevronLeft, ChevronRight, Download, Redo2, Undo2 } from "lucide-react";
import { fileName, formatBytes } from "../lib/images";
import { LivePainter } from "../lib/paint";
import { Render, Stroke, closeRefine, workingOriginal } from "../lib/refine";
import { cutoutKey, isDownloaded, useCutouts } from "../store/cutouts";
import { useBrush } from "../store/brush";
import { canRedo, canUndo, historyOf, useEdits } from "../store/edits";
import { openPaths, useSession } from "../store/session";
import { Button, IconButton, SectionLabel, Segmented, Tooltip } from "../ui";
import { BackgroundSection } from "./BackgroundSection";
import { CutoutCanvas } from "./CutoutCanvas";
import { CutoutSection, StageStatus } from "./CutoutStatus";
import { EdgesSection } from "./EdgesSection";
import { RefineSection } from "./RefineSection";
import { Stage, StageBrush, ViewMode } from "./Stage";
import { StageBackdrop } from "./StageBackdrop";
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
  const background = useEdits((s) => s.liveBackground ?? s.background.value);
  const undoable = useEdits((s) => canUndo(s, editKey));
  const redoable = useEdits((s) => canRedo(s, editKey));
  const { undo, redo } = useEdits.getState();
  const done = state?.status === "done";

  // Brush: the live painter draws into the canvas; the finished stroke becomes an edit.
  const brush = useBrush();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const painting = useRef(false);
  const original = useRef<{ key: string; render: Render } | null>(null);
  const stroke = useRef<{ painter: LivePainter; k: number; data: Stroke } | null>(null);
  const brushing = brush.tool !== "hand" && done && mode !== "before";

  // The original pixels for restoring, fetched once per image when a brush is picked.
  useEffect(() => {
    if (!brushing || !image || original.current?.key === editKey) return;
    let cancelled = false;
    workingOriginal(image.path, model).then(
      (render) => {
        if (!cancelled) original.current = { key: editKey, render };
      },
      () => {},
    );
    return () => {
      cancelled = true;
    };
  }, [brushing, image, model, editKey]);

  const stageBrush: StageBrush | undefined =
    brushing && image
      ? {
          size: brush.size,
          softness: brush.softness,
          onStart: (p, scale) => {
            const canvas = canvasRef.current;
            const ctx = canvas?.getContext("2d");
            const orig = original.current;
            if (!canvas || !ctx || canvas.width === 0 || orig?.key !== editKey) return false;
            const k = canvas.width / image.width;
            const radius = brush.size / 2 / scale;
            const data: Stroke = { mode: brush.tool === "erase" ? "erase" : "restore", radius, softness: brush.softness, points: [] };
            const painter = new LivePainter(ctx, { mode: data.mode, radius: radius * k, softness: brush.softness }, orig.render.pixels, canvas.width, canvas.height);
            stroke.current = { painter, k, data };
            painting.current = true;
            data.points.push([p.x, p.y]);
            painter.add(p.x * k, p.y * k);
            return true;
          },
          onMove: (p) => {
            const s = stroke.current;
            if (!s) return;
            const [lx, ly] = s.data.points[s.data.points.length - 1];
            if (Math.hypot(p.x - lx, p.y - ly) < 0.5) return;
            s.data.points.push([p.x, p.y]);
            s.painter.add(p.x * s.k, p.y * s.k);
          },
          onEnd: () => {
            const s = stroke.current;
            stroke.current = null;
            painting.current = false;
            if (s) useEdits.getState().addStroke(editKey, s.data);
          },
        }
      : undefined;

  // H / E / R pick a tool; [ and ] change the size (with Shift, the softness).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey || !done) return;
      if ((e.target as HTMLElement | null)?.closest("input, textarea, [contenteditable]")) return;
      const tool = { h: "hand", e: "erase", r: "restore" }[e.key.toLowerCase()] as "hand" | "erase" | "restore" | undefined;
      if (tool && !e.shiftKey) useBrush.getState().setTool(tool);
      else if (e.code === "BracketLeft") useBrush.getState().step(-1, e.shiftKey);
      else if (e.code === "BracketRight") useBrush.getState().step(1, e.shiftKey);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [done]);

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
            disabled={!undoable}
            onClick={() => undo(editKey)}
          />
          <IconButton
            label="Redo"
            shortcut="⇧⌘Z"
            icon={Redo2}
            disabled={!redoable}
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
                <>
                  <StageBackdrop path={image.path} model={model} edits={history.edits} background={background} />
                  <CutoutCanvas
                    key={editKey}
                    path={image.path}
                    model={model}
                    cutoutFile={state.result.cutout}
                    edits={history.edits}
                    live={live}
                    canvasRef={canvasRef}
                    painting={painting}
                  />
                </>
              ) : null
            }
            mode={mode}
            size={image}
            brush={stageBrush}
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
          <BackgroundSection editKey={editKey} disabled={!done} />
          <RefineSection disabled={!done} beforeView={mode === "before"} />
          <EdgesSection editKey={editKey} disabled={!done} />
        </aside>
      </div>
    </div>
  );
}
