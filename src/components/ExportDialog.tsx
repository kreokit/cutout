import { useEffect, useRef, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { Download, FolderOpen } from "lucide-react";
import { Background } from "../lib/background";
import {
  ExportSettings,
  FORMATS,
  Fit,
  Format,
  QUALITY_MAX,
  QUALITY_MIN,
  SIZES,
  SizeId,
  exportImage,
  exportPath,
  exportPreview,
  revealExport,
  sizeId,
} from "../lib/export";
import { ImageInfo, fileName } from "../lib/images";
import { CutoutModel } from "../lib/models";
import { Edits } from "../lib/refine";
import { useExportSettings } from "../store/exportSettings";
import { Button, Dialog, Field, ProgressBar, Segmented, Select, Slider, toast } from "../ui";

/** libwebp's limit (encode.rs). */
const WEBP_MAX_SIDE = 16383;

type Status = { kind: "idle" } | { kind: "exporting" } | { kind: "saved"; path: string };

/**
 * Export: format, quality, size and Fit / Fill, with a preview of exactly what will be
 * saved (rendered by the backend). Export… asks where to save, then writes the file at
 * full size with every edit and the background.
 */
export function ExportDialog({
  image,
  model,
  edits,
  background,
  onClose,
}: {
  image: ImageInfo;
  model: CutoutModel;
  edits: Edits;
  background: Background;
  onClose: () => void;
}) {
  const settings = useExportSettings((s) => s.settings);
  const [status, setStatus] = useState<Status>({ kind: "idle" });
  const out = settings.size ?? { width: image.width, height: image.height };
  const tooBigForWebp = settings.format === "webp" && (out.width > WEBP_MAX_SIDE || out.height > WEBP_MAX_SIDE);

  const run = async () => {
    const dest = await save({
      defaultPath: exportPath(image.path, settings.format),
      filters: [
        {
          name: FORMATS.find((f) => f.value === settings.format)!.label,
          extensions: settings.format === "jpg" ? ["jpg", "jpeg"] : [settings.format],
        },
      ],
    });
    if (!dest) return;
    setStatus({ kind: "exporting" });
    try {
      const path = await exportImage(image.path, model, edits, background, settings, dest);
      setStatus({ kind: "saved", path });
    } catch (e) {
      setStatus({ kind: "idle" });
      toast(String(e), "error");
    }
  };

  if (status.kind === "saved") {
    return (
      <Dialog
        title="Exported"
        onClose={onClose}
        footer={
          <>
            <Button
              icon={FolderOpen}
              onClick={() => void revealExport(status.path).catch((e) => toast(String(e), "error"))}
            >
              Show in folder
            </Button>
            <Button variant="primary" onClick={onClose}>
              Done
            </Button>
          </>
        }
      >
        <p className="export-saved">
          Saved <strong title={status.path}>{fileName(status.path)}</strong> ({out.width} × {out.height} px).
        </p>
      </Dialog>
    );
  }

  const exporting = status.kind === "exporting";
  return (
    <Dialog
      title="Export"
      width={600}
      dismissible={!exporting}
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose} disabled={exporting}>
            Cancel
          </Button>
          <Button variant="primary" icon={Download} disabled={exporting || tooBigForWebp} onClick={() => void run()}>
            {exporting ? "Exporting…" : "Export…"}
          </Button>
        </>
      }
    >
      <div className="export-body">
        <ExportPreview image={image} model={model} edits={edits} background={background} settings={settings} />
        <div className="export-controls">
          <ExportSettingsFields
            originalSize={`${image.width} × ${image.height}`}
            background={background}
            disabled={exporting}
            tooBigForWebp={tooBigForWebp}
          />
          {exporting && <ProgressBar value={null} label="Exporting" />}
        </div>
      </div>
    </Dialog>
  );
}

/** The settings controls (format, quality, size, framing) with their hints. */
export function ExportSettingsFields({
  originalSize,
  background,
  disabled,
  tooBigForWebp,
}: {
  /** The hint for "Original": the image's size, or e.g. "each image's own". */
  originalSize: string;
  background: Background;
  disabled: boolean;
  tooBigForWebp: boolean;
}) {
  const settings = useExportSettings((s) => s.settings);
  const set = useExportSettings((s) => s.set);
  const lossy = settings.format !== "png";
  return (
    <>
      <Field label="Format">
        <Segmented<Format>
          label="Format"
          value={settings.format}
          onChange={(format) => set({ format })}
          disabled={disabled}
          options={FORMATS}
        />
      </Field>
      {lossy && (
        <Field label="Quality">
          <div className="slider-row">
            <Slider
              label="Quality"
              value={settings.quality}
              min={QUALITY_MIN}
              max={QUALITY_MAX}
              disabled={disabled}
              onChange={(quality) => set({ quality })}
            />
            <span className="slider-value">{settings.quality}</span>
          </div>
        </Field>
      )}
      <Field label="Size">
        <Select<SizeId>
          label="Size"
          value={sizeId(settings.size)}
          disabled={disabled}
          onChange={(id) => set({ size: SIZES.find((s) => s.id === id)!.size })}
          options={SIZES.map((s) => {
            const hint = s.size ? `${s.size.width} × ${s.size.height}` : originalSize;
            return { value: s.id, label: s.label, hint };
          })}
        />
      </Field>
      <Field
        label="Framing"
        hint={
          settings.size === null
            ? "The original size keeps the photo's framing."
            : settings.fit === "fit"
              ? "Shows the whole photo; the background fills the rest."
              : "Fills the frame around the subject, never cutting it off."
        }
      >
        <Segmented<Fit>
          label="Framing"
          value={settings.size === null ? "fit" : settings.fit}
          onChange={(fit) => set({ fit })}
          disabled={disabled || settings.size === null}
          options={[
            { value: "fit", label: "Fit" },
            { value: "fill", label: "Fill" },
          ]}
        />
      </Field>
      {settings.format === "jpg" && background.kind === "transparent" && (
        <p className="ui-field-hint">JPG has no transparency: the background will be white.</p>
      )}
      {settings.format === "png" && background.kind === "transparent" && (
        <p className="ui-field-hint">PNG keeps the transparency.</p>
      )}
      {tooBigForWebp && (
        <p className="ui-field-hint export-warning">
          WebP images can be at most {WEBP_MAX_SIDE} px on a side. Choose PNG or JPG, or a smaller size.
        </p>
      )}
    </>
  );
}

/** The preview, fitted into a square box on a checkerboard ("latest wins" requests). */
export function ExportPreview({
  image,
  model,
  edits,
  background,
  settings,
}: {
  image: ImageInfo;
  model: CutoutModel;
  edits: Edits;
  background: Background;
  settings: ExportSettings;
}) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const [shape, setShape] = useState<{ width: number; height: number } | null>(null);
  const busy = useRef(false);
  const waiting = useRef<{ settings: ExportSettings; edits: Edits; background: Background } | null>(null);
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
      const r = await exportPreview(image.path, model, req.edits, req.background, req.settings);
      const c = canvas.current;
      const ctx = c?.getContext("2d");
      if (!alive.current || !c || !ctx) return;
      c.width = r.width;
      c.height = r.height;
      ctx.putImageData(new ImageData(r.pixels, r.width, r.height), 0, 0);
      setShape({ width: r.width, height: r.height });
    } catch (e) {
      if (alive.current) toast(String(e), "error");
    } finally {
      busy.current = false;
      if (alive.current) void pump();
    }
  };

  // Quality doesn't change the preview.
  const key = JSON.stringify({ ...settings, quality: 0 });
  useEffect(() => {
    waiting.current = { settings, edits, background };
    void pump();
    // pump reads refs only; re-run when the picture changes (the edits can change by undo).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, edits, background]);

  const BOX = 240;
  const k = shape ? Math.min(BOX / shape.width, BOX / shape.height) : 0;
  return (
    <div className="export-preview" aria-label="Preview">
      <div
        className="checkerboard export-preview-frame"
        style={shape ? { width: shape.width * k, height: shape.height * k } : { visibility: "hidden" }}
      >
        <canvas ref={canvas} />
      </div>
    </div>
  );
}
