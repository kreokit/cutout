import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button, Dialog } from "../ui";

interface CpuSupport {
  supported: boolean;
  missing: string[];
}

/**
 * Cutouts need a processor with SSE4.2 and POPCNT (src-tauri/src/cpu.rs). On an older
 * CPU, explain that once at startup instead of failing later.
 */
export function CpuCheck() {
  const [missing, setMissing] = useState<string[] | null>(null);

  useEffect(() => {
    invoke<CpuSupport>("cpu_support").then(
      (s) => !s.supported && setMissing(s.missing),
      () => {},
    );
  }, []);

  if (!missing) return null;
  return (
    <Dialog
      title="This CPU isn't supported"
      onClose={() => setMissing(null)}
      width={460}
      footer={
        <>
          <Button variant="secondary" onClick={() => invoke("quit_app")}>
            Quit
          </Button>
          <Button variant="primary" onClick={() => setMissing(null)}>
            Continue without cutouts
          </Button>
        </>
      }
    >
      <p className="cpu-check-text">
        Background removal needs a processor with {missing.join(", ")}, which almost all Intel and AMD processors
        from 2009 on have. This computer's processor doesn't, so KreoKit Cutout can't remove backgrounds here.
      </p>
      <p className="cpu-check-text">You can still open and view images.</p>
    </Dialog>
  );
}
