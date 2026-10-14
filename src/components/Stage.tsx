import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { ChevronsLeftRight, ZoomIn, ZoomOut } from "lucide-react";
import { Split, splitAt, splitForKey } from "../lib/compare";
import { Size } from "../lib/fit";
import {
  View,
  centeredView,
  clampView,
  fitView,
  panBy,
  stepScale,
  wheelAction,
  zoomBy,
  zoomTo,
} from "../lib/view";
import { Button, IconButton } from "../ui";

export type ViewMode = "before" | "after" | "compare";

/** Space kept around the image when it's fitted, in CSS pixels. */
const STAGE_PADDING = 32;
/** From this zoom on, pixels are drawn sharp (no smoothing) for precise edge work. */
const PIXELATED_FROM = 2;

/** WebKit's trackpad pinch (GestureEvent isn't in the DOM typings). */
interface PinchEvent extends UIEvent {
  scale: number;
  clientX: number;
  clientY: number;
}

const isTyping = (target: EventTarget | null) =>
  (target as HTMLElement | null)?.closest?.("input, textarea, [contenteditable], [role=slider]") != null;

/**
 * The image on a transparency checkerboard, zoomable and pannable: the original, the
 * cutout, or both split by a draggable divider. Scroll pans; ⌘/Ctrl + scroll and pinch
 * zoom; Space + drag, middle-drag or (with `panWithLeftButton`) any drag pans.
 */
export function Stage({
  original,
  cutout,
  mode,
  size,
  status,
  panWithLeftButton = true,
}: {
  original: string;
  /** The cutout layer (an image or canvas sized `size`), or null while there is none. */
  cutout: React.ReactNode | null;
  mode: ViewMode;
  size: Size;
  status: React.ReactNode;
  panWithLeftButton?: boolean;
}) {
  const stage = useRef<HTMLDivElement>(null);
  const [box, setBox] = useState<Size>({ width: 0, height: 0 });
  const [view, setView] = useState<View | null>(null);
  /** The view follows the stage size until the user zooms or pans. */
  const fitted = useRef(true);
  const [failed, setFailed] = useState(false);
  const [split, setSplit] = useState<Split>(0.5);
  const [space, setSpace] = useState(false);
  const [panning, setPanning] = useState(false);
  useEffect(() => setFailed(false), [original]);

  useLayoutEffect(() => {
    const el = stage.current;
    if (!el) return;
    const measure = () => setBox({ width: el.clientWidth, height: el.clientHeight });
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  useLayoutEffect(() => {
    if (box.width > 0 && fitted.current) setView(fitView(size, box, STAGE_PADDING));
  }, [box, size]);

  const change = useCallback(
    (update: (v: View) => View) => {
      fitted.current = false;
      setView((v) => (v ? clampView(update(v), size, box) : v));
    },
    [size, box],
  );
  const fit = useCallback(() => {
    fitted.current = true;
    setView(fitView(size, box, STAGE_PADDING));
  }, [size, box]);
  const actualSize = useCallback(() => change(() => centeredView(size, box, 1)), [change, size, box]);
  const centre = { x: box.width / 2, y: box.height / 2 };
  const step = (direction: 1 | -1) => change((v) => zoomTo(v, stepScale(v.scale, direction), centre));

  const stagePoint = (clientX: number, clientY: number) => {
    const r = stage.current!.getBoundingClientRect();
    return { x: clientX - r.left, y: clientY - r.top };
  };

  // Scroll pans; ⌘/Ctrl + scroll zooms. Not passive, so the page itself never scrolls.
  useEffect(() => {
    const el = stage.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const action = wheelAction(e);
      if (action.kind === "pan") change((v) => panBy(v, action.dx, action.dy));
      else {
        const at = stagePoint(e.clientX, e.clientY);
        change((v) => zoomBy(v, action.factor, at));
      }
    };
    // WebKit (macOS) reports trackpad pinches as gesture events.
    let startScale = 1;
    const onGestureStart = (e: Event) => {
      e.preventDefault();
      startScale = 1;
    };
    const onGestureChange = (e: Event) => {
      e.preventDefault();
      const g = e as PinchEvent;
      const at = stagePoint(g.clientX, g.clientY);
      const factor = g.scale / startScale;
      startScale = g.scale;
      change((v) => zoomBy(v, factor, at));
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    el.addEventListener("gesturestart", onGestureStart);
    el.addEventListener("gesturechange", onGestureChange);
    return () => {
      el.removeEventListener("wheel", onWheel);
      el.removeEventListener("gesturestart", onGestureStart);
      el.removeEventListener("gesturechange", onGestureChange);
    };
  }, [change]);

  // Space held = pan; ⌘0 fit, ⌘1 100%, ⌘+ / ⌘− zoom.
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (isTyping(e.target)) return;
      if (e.key === " " && !e.metaKey && !e.ctrlKey) {
        e.preventDefault();
        setSpace(true);
        return;
      }
      if (!(e.metaKey || e.ctrlKey) || e.altKey) return;
      const action = { "0": fit, "1": actualSize, "=": () => step(1), "+": () => step(1), "-": () => step(-1) }[e.key];
      if (action) {
        e.preventDefault();
        action();
      }
    };
    const onKeyUp = (e: KeyboardEvent) => {
      if (e.key === " ") setSpace(false);
    };
    const onBlur = () => setSpace(false);
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
      window.removeEventListener("blur", onBlur);
    };
  });

  // Dragging pans (middle button, Space + drag, or any drag when allowed).
  const last = useRef<{ x: number; y: number } | null>(null);
  const onPointerDown = (e: React.PointerEvent) => {
    const pan = e.button === 1 || (e.button === 0 && (space || panWithLeftButton));
    if (!pan || (e.target as HTMLElement).closest(".compare-divider, .zoom-bar, .stage-card")) return;
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    last.current = { x: e.clientX, y: e.clientY };
    setPanning(true);
  };
  const onPointerMove = (e: React.PointerEvent) => {
    if (!last.current) return;
    const dx = e.clientX - last.current.x;
    const dy = e.clientY - last.current.y;
    last.current = { x: e.clientX, y: e.clientY };
    change((v) => panBy(v, dx, dy));
  };
  const endPan = () => {
    last.current = null;
    setPanning(false);
  };

  const shown: ViewMode = cutout === null ? "before" : mode;
  const s = view?.scale ?? 1;
  const layer: React.CSSProperties | undefined = view
    ? {
        width: size.width,
        height: size.height,
        transform: `translate(${view.x}px, ${view.y}px) scale(${s})`,
        imageRendering: s >= PIXELATED_FROM ? "pixelated" : undefined,
      }
    : undefined;
  // The checkerboard is drawn in screen space, only where the image is visible.
  const rect = view && {
    left: Math.max(0, view.x),
    top: Math.max(0, view.y),
    right: Math.min(box.width, view.x + size.width * s),
    bottom: Math.min(box.height, view.y + size.height * s),
  };
  const dividerX = view ? view.x + split * size.width * s : 0;
  const cursor = panning ? "grabbing" : space || panWithLeftButton ? "grab" : undefined;

  return (
    <div
      className="stage"
      ref={stage}
      style={{ cursor }}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={endPan}
      onPointerCancel={endPan}
      onAuxClick={(e) => e.preventDefault()}
    >
      {failed ? (
        <p className="stage-error">This image can't be shown. It may be damaged or use an unusual format.</p>
      ) : (
        view &&
        rect && (
          <>
            {rect.right > rect.left && rect.bottom > rect.top && (
              <div
                className="checkerboard"
                style={{ left: rect.left, top: rect.top, width: rect.right - rect.left, height: rect.bottom - rect.top }}
              />
            )}
            {shown !== "before" && (
              <div className="stage-layer" style={layer}>
                {cutout}
              </div>
            )}
            {shown !== "after" && (
              <img
                className="stage-layer"
                src={original}
                alt=""
                draggable={false}
                onError={() => setFailed(true)}
                style={{ ...layer, clipPath: shown === "compare" ? `inset(0 ${(1 - split) * 100}% 0 0)` : undefined }}
              />
            )}
            {shown === "compare" && (
              <Divider
                x={Math.min(box.width, Math.max(0, dividerX))}
                top={rect.top}
                height={Math.max(0, rect.bottom - rect.top)}
                split={split}
                onDrag={(clientX) => {
                  const left = stage.current!.getBoundingClientRect().left + view.x;
                  setSplit(splitAt(clientX, left, size.width * s));
                }}
                onChange={setSplit}
              />
            )}
          </>
        )
      )}
      {view && (
        <div className="zoom-bar">
          <IconButton label="Zoom out" shortcut="⌘−" icon={ZoomOut} size="sm" onClick={() => step(-1)} />
          <span className="zoom-value" aria-live="polite">
            {Math.round(s * 100)}%
          </span>
          <IconButton label="Zoom in" shortcut="⌘+" icon={ZoomIn} size="sm" onClick={() => step(1)} />
          <Button variant="ghost" size="sm" onClick={fit} title="Fit (⌘0)">
            Fit
          </Button>
          <Button variant="ghost" size="sm" onClick={actualSize} title="100% (⌘1)">
            100%
          </Button>
        </div>
      )}
      {status}
    </div>
  );
}

/** The compare divider (screen space): drag it, or focus it and use the arrow keys. */
function Divider({
  x,
  top,
  height,
  split,
  onDrag,
  onChange,
}: {
  x: number;
  top: number;
  height: number;
  split: Split;
  onDrag: (clientX: number) => void;
  onChange: (split: Split) => void;
}) {
  return (
    <div
      className="compare-divider"
      style={{ left: x, top, height }}
      role="slider"
      tabIndex={0}
      aria-label="Compare before and after"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(split * 100)}
      aria-valuetext={`${Math.round(split * 100)}% original`}
      onPointerDown={(e) => {
        e.stopPropagation();
        e.currentTarget.setPointerCapture(e.pointerId);
        onDrag(e.clientX);
      }}
      onPointerMove={(e) => {
        if (e.currentTarget.hasPointerCapture(e.pointerId)) onDrag(e.clientX);
      }}
      onKeyDown={(e) => {
        const next = splitForKey(split, e.key, e.shiftKey);
        if (next === null) return;
        e.preventDefault();
        onChange(next);
      }}
    >
      <span className="compare-grip" aria-hidden>
        <ChevronsLeftRight size={14} strokeWidth={2} />
      </span>
    </div>
  );
}
