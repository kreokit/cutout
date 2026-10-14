import { open } from "@tauri-apps/plugin-dialog";
import { ImageIcon } from "lucide-react";
import { BACKGROUND_SWATCHES, BLUR_MAX, BLUR_MIN, Background, BackgroundKind } from "../lib/background";
import { IMAGE_EXTENSIONS, fileName } from "../lib/images";
import { useEdits } from "../store/edits";
import { Button, ColorPicker, Field, SectionLabel, Segmented, Slider } from "../ui";

/** Asks for a background image; null if the dialog was cancelled. */
async function pickImage(): Promise<string | null> {
  const picked = await open({
    multiple: false,
    directory: false,
    filters: [{ name: "Images", extensions: IMAGE_EXTENSIONS }],
  });
  return typeof picked === "string" ? picked : null;
}

/** Inspector: the background of all open images (transparent, colour, blur, image). */
export function BackgroundSection({ editKey, disabled }: { editKey: string; disabled: boolean }) {
  const committed = useEdits((s) => s.background.value);
  const live = useEdits((s) => s.liveBackground);
  const choices = useEdits((s) => s.choices);
  const { previewBackground, commitBackground } = useEdits.getState();
  const background = live ?? committed;
  const commit = (b: Background) => commitBackground(editKey, b);

  const chooseImage = async () => {
    const path = await pickImage();
    if (path) commit({ kind: "image", path });
  };
  const choose = (kind: BackgroundKind) => {
    if (kind === "transparent") commit({ kind });
    else if (kind === "color") commit({ kind, color: choices.color });
    else if (kind === "blur") commit({ kind, strength: choices.strength });
    else if (choices.path) commit({ kind, path: choices.path });
    else void chooseImage();
  };

  return (
    <section className="inspector-section">
      <SectionLabel>Background</SectionLabel>
      <Segmented<BackgroundKind>
        label="Background"
        value={background.kind}
        onChange={choose}
        disabled={disabled}
        options={[
          { value: "transparent", label: "None" },
          { value: "color", label: "Color" },
          { value: "blur", label: "Blur" },
          { value: "image", label: "Image" },
        ]}
      />
      {background.kind === "color" && (
        <Field label="Color" inline>
          <ColorPicker
            label="Background color"
            value={background.color}
            swatches={BACKGROUND_SWATCHES}
            disabled={disabled}
            onChange={(color) => previewBackground({ kind: "color", color })}
            onCommit={(color) => commit({ kind: "color", color })}
          />
        </Field>
      )}
      {background.kind === "blur" && (
        <Field label="Strength" hint="Blurs the original photo behind the cutout.">
          <div className="slider-row">
            <Slider
              label="Blur strength"
              value={background.strength}
              min={BLUR_MIN}
              max={BLUR_MAX}
              disabled={disabled}
              onChange={(strength) => previewBackground({ kind: "blur", strength })}
              onCommit={(strength) => commit({ kind: "blur", strength })}
            />
            <span className="slider-value">{background.strength}</span>
          </div>
        </Field>
      )}
      {background.kind === "image" && (
        <div className="background-file">
          <span className="background-file-name" title={background.path}>
            {fileName(background.path)}
          </span>
          <Button variant="secondary" size="sm" icon={ImageIcon} disabled={disabled} onClick={() => void chooseImage()}>
            Change…
          </Button>
        </div>
      )}
      <p className="ui-field-hint">Used for all open images.</p>
    </section>
  );
}
