import type { StateCreator } from "zustand";

import type { AppStoreState, SettingsSlice } from "~/stores/slices/types";
import { structuralShare } from "~/lib/structural-share";
import type { Settings } from "~/types";

function mergeSettings(previous: Settings | null, next: Settings): Settings {
  if (!previous) {
    return next;
  }
  return structuralShare(previous, next);
}

export const createSettingsSlice: StateCreator<AppStoreState, [], [], SettingsSlice> = (set) => ({
  settings: null,
  setSettings: (settings) =>
    set((state) => ({
      settings: mergeSettings(state.settings, settings),
    })),
});
