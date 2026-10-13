import { create } from "zustand";
import { ImageInfo, openImages } from "../lib/images";
import { toast } from "../ui";

interface Session {
  /** Images opened together (one, or a batch); empty on the Home screen. */
  images: ImageInfo[];
  /** Index of the image shown in the editor. */
  current: number;
  show: (index: number) => void;
  close: () => void;
}

export const useSession = create<Session>((set, get) => ({
  images: [],
  current: 0,
  show: (index) => set({ current: Math.max(0, Math.min(index, get().images.length - 1)) }),
  close: () => set({ images: [], current: 0 }),
}));

/**
 * Opens dropped or picked files: the good images replace the current session; each file
 * that can't be opened gets an error toast. Returns whether anything opened.
 */
export async function openPaths(paths: string[]): Promise<boolean> {
  if (paths.length === 0) return false;
  try {
    const { images, errors } = await openImages(paths);
    for (const error of errors.slice(0, 3)) toast(error, "error");
    if (errors.length > 3) toast(`${errors.length - 3} more files couldn't be opened.`, "error");
    if (images.length === 0) return false;
    useSession.setState({ images, current: 0 });
    return true;
  } catch (e) {
    toast(String(e), "error");
    return false;
  }
}
