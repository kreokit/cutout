import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { CpuCheck } from "./components/CpuCheck";
import { Editor } from "./components/Editor";
import { Home } from "./components/Home";
import { chooseImages } from "./components/TopBar";
import { useCutouts } from "./store/cutouts";
import { openPaths, useSession } from "./store/session";
import { ToastRegion } from "./ui";

function App() {
  const hasImages = useSession((s) => s.images.length > 0);

  useEffect(() => {
    void useCutouts.getState().refreshModels();
  }, []);

  // ⌘O / Ctrl+O opens images from any screen.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && !e.shiftKey && e.key.toLowerCase() === "o") {
        e.preventDefault();
        void chooseImages();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // Development aid: KREOKIT_DEV_OPEN=/path/a.png:/path/b.jpg opens them at startup.
  useEffect(() => {
    if (!import.meta.env.DEV) return;
    invoke<string[]>("dev_startup_images").then((paths) => void openPaths(paths));
  }, []);

  return (
    <>
      {hasImages ? <Editor /> : <Home />}
      <ToastRegion />
      <CpuCheck />
    </>
  );
}

export default App;
