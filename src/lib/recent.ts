import { invoke } from "@tauri-apps/api/core";

export interface RecentEntry {
  path: string;
  opened_at: number;
  /** The image still exists. */
  exists: boolean;
  thumbnail: string | null;
}

export const listRecent = () => invoke<RecentEntry[]>("list_recent");
export const removeRecent = (path: string) => invoke<void>("remove_recent", { path });

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** "just now", "5 min ago", "3 h ago", "yesterday", "Monday", "12 Oct", "12 Oct 2025". */
export function relativeTime(ms: number, now: number): string {
  const diff = now - ms;
  if (diff < MINUTE) return "just now";
  if (diff < HOUR) return `${Math.floor(diff / MINUTE)} min ago`;
  const then = new Date(ms);
  const today = new Date(now);
  const startOfToday = new Date(today.getFullYear(), today.getMonth(), today.getDate()).getTime();
  if (ms >= startOfToday) return `${Math.floor(diff / HOUR)} h ago`;
  if (ms >= startOfToday - DAY) return "yesterday";
  if (ms >= startOfToday - 6 * DAY) return then.toLocaleDateString("en-GB", { weekday: "long" });
  const sameYear = then.getFullYear() === today.getFullYear();
  return then.toLocaleDateString("en-GB", { day: "numeric", month: "short", ...(sameYear ? {} : { year: "numeric" }) });
}
