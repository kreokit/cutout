import { useCallback, useEffect, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { FolderOpen, ImageIcon, TriangleAlert, Trash2, Upload, Zap, Sparkles } from "lucide-react";
import { fileName } from "../lib/images";
import { RecentEntry, listRecent, relativeTime, removeRecent } from "../lib/recent";
import { useCutouts } from "../store/cutouts";
import { openPaths } from "../store/session";
import { Button, MenuEntry, Segmented, useContextMenu } from "../ui";
import { TopBar, chooseImages } from "./TopBar";
import "../styles/home.css";

/** Home: drop or choose images, or reopen a recent one. */
export function Home() {
  const [recent, setRecent] = useState<RecentEntry[] | null>(null);
  const [dragging, setDragging] = useState(false);
  const [busy, setBusy] = useState(false);
  const model = useCutouts((s) => s.model);
  const setModel = useCutouts((s) => s.setModel);

  const refresh = useCallback(() => {
    listRecent().then(setRecent, () => setRecent([]));
  }, []);
  useEffect(refresh, [refresh]);

  const open = useCallback(
    async (paths: string[]) => {
      setBusy(true);
      try {
        if (!(await openPaths(paths))) refresh();
      } finally {
        setBusy(false);
      }
    },
    [refresh],
  );

  // Dropping one or many images opens them.
  useEffect(() => {
    const unlisten = getCurrentWebview().onDragDropEvent((event) => {
      const { type } = event.payload;
      if (type === "enter" || type === "over") return setDragging(true);
      setDragging(false);
      if (type === "drop") void open(event.payload.paths);
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [open]);

  async function choose() {
    setBusy(true);
    try {
      await chooseImages();
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="home">
      <TopBar
        title={
          <span className="home-brand" data-tauri-drag-region>
            KreoKit Cutout
          </span>
        }
      />

      <main className="home-main">
        <section className={dragging ? "dropzone dragging" : "dropzone"} aria-label="Add images">
          <div className="dropzone-icon">
            <Upload size={22} strokeWidth={1.75} aria-hidden />
          </div>
          <h1 className="dropzone-title">{dragging ? "Drop to cut out" : "Drop images to remove the background"}</h1>
          <p className="dropzone-text">One image or a whole batch. Everything runs on this computer; your images never leave it.</p>
          <Button variant="primary" onClick={choose} disabled={busy}>
            Choose images
          </Button>
          <p className="dropzone-formats">PNG · JPG · WebP</p>
        </section>

        <section className="model-choice" aria-labelledby="model-title">
          <h2 id="model-title" className="section-title">
            Cutout quality
          </h2>
          <Segmented
            label="Cutout quality"
            value={model}
            onChange={setModel}
            options={[
              { value: "fast", label: "Fast", icon: Zap, hint: "about 1 s per image" },
              { value: "best", label: "Best", icon: Sparkles, hint: "coming soon", disabled: true },
            ]}
          />
        </section>

        <section className="recent" aria-labelledby="recent-title">
          <h2 id="recent-title" className="section-title">
            Recent images
          </h2>
          {recent === null ? null : recent.length === 0 ? (
            <p className="recent-empty">Images you open appear here.</p>
          ) : (
            <div className="recent-grid">
              {recent.map((entry) => (
                <RecentCard
                  key={entry.path}
                  entry={entry}
                  onOpen={() => void open([entry.path])}
                  onRemove={() => void removeRecent(entry.path).then(refresh)}
                />
              ))}
            </div>
          )}
        </section>
      </main>
    </div>
  );
}

function RecentCard({ entry, onOpen, onRemove }: { entry: RecentEntry; onOpen: () => void; onRemove: () => void }) {
  const { onContextMenu, menu } = useContextMenu(
    (): MenuEntry[] => [
      { label: "Open", icon: FolderOpen, disabled: !entry.exists, onSelect: onOpen },
      { label: "Remove from list", icon: Trash2, onSelect: onRemove },
    ],
    "Image actions",
  );
  return (
    <>
      <button
        type="button"
        className={entry.exists ? "recent-card" : "recent-card missing"}
        onClick={entry.exists ? onOpen : undefined}
        onContextMenu={onContextMenu}
        aria-disabled={!entry.exists}
        title={entry.path}
      >
        <span className="recent-thumb">
          {entry.thumbnail ? (
            <img src={convertFileSrc(entry.thumbnail)} alt="" loading="lazy" />
          ) : (
            <ImageIcon size={22} strokeWidth={1.75} aria-hidden />
          )}
        </span>
        <span className="recent-info">
          <span className="recent-name">{fileName(entry.path)}</span>
          {entry.exists ? (
            <span className="recent-time">{relativeTime(entry.opened_at, Date.now())}</span>
          ) : (
            <span className="recent-missing">
              <TriangleAlert size={12} strokeWidth={2} aria-hidden /> Moved or deleted
            </span>
          )}
        </span>
      </button>
      {menu}
    </>
  );
}
