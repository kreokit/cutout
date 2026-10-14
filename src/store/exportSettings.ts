import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";
import { DEFAULT_EXPORT, ExportSettings, sanitizeSettings } from "../lib/export";

interface ExportSettingsStore {
  settings: ExportSettings;
  set: (change: Partial<ExportSettings>) => void;
}

/** The Export dialog's settings, remembered between launches (on this computer only). */
export const useExportSettings = create<ExportSettingsStore>()(
  persist(
    (set) => ({
      settings: DEFAULT_EXPORT,
      set: (change) => set((s) => ({ settings: { ...s.settings, ...change } })),
    }),
    {
      name: "kreokit-cutout-export",
      version: 1,
      storage: createJSONStorage(() => localStorage),
      partialize: (s) => ({ settings: s.settings }),
      // Stored settings may be from an older version or edited: keep only valid values.
      merge: (stored, current) => ({
        ...current,
        settings: sanitizeSettings((stored as { settings?: unknown } | undefined)?.settings),
      }),
    },
  ),
);
