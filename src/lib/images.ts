import { invoke } from "@tauri-apps/api/core";

/** An opened image, as reported by the backend (images.rs). */
export interface ImageInfo {
  path: string;
  /** Displayed size, after the EXIF orientation. */
  width: number;
  height: number;
  format: "png" | "jpeg" | "webp";
  bytes: number;
}

export interface Opened {
  images: ImageInfo[];
  /** One message per file that couldn't be opened. */
  errors: string[];
}

/** Image files the app opens (the backend checks the contents too). */
export const IMAGE_EXTENSIONS = ["png", "jpg", "jpeg", "webp"];

export const isImagePath = (path: string) =>
  IMAGE_EXTENSIONS.includes(path.split(/[\\/]/).pop()?.split(".").slice(1).pop()?.toLowerCase() ?? "");

export const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;

/** "980 B", "48 KB", "3.4 MB". */
export function formatBytes(bytes: number): string {
  if (bytes < 1000) return `${bytes} B`;
  if (bytes < 1_000_000) return `${Math.round(bytes / 1000)} KB`;
  return `${(bytes / 1_000_000).toFixed(1)} MB`;
}

export const openImages = (paths: string[]) => invoke<Opened>("open_images", { paths });
