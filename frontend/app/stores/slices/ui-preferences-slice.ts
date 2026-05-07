import type { StateCreator } from "zustand";

import type { AppStoreState, UiPreferencesSlice } from "~/stores/slices/types";

const RICH_TEXT_RENDERING_STORAGE_KEY = "rikkahub.richTextRenderingEnabled";

function readRichTextRenderingPreference(): boolean {
  if (typeof window === "undefined") {
    return true;
  }

  try {
    const value = window.localStorage.getItem(RICH_TEXT_RENDERING_STORAGE_KEY);
    return value == null ? true : value === "true";
  } catch {
    return true;
  }
}

function writeRichTextRenderingPreference(enabled: boolean) {
  if (typeof window === "undefined") {
    return;
  }

  try {
    window.localStorage.setItem(RICH_TEXT_RENDERING_STORAGE_KEY, String(enabled));
  } catch {
    // Ignore storage failures; the in-memory state still updates for this session.
  }
}

export const createUiPreferencesSlice: StateCreator<AppStoreState, [], [], UiPreferencesSlice> = (
  set,
  get,
) => ({
  richTextRenderingEnabled: readRichTextRenderingPreference(),
  setRichTextRenderingEnabled: (enabled) => {
    writeRichTextRenderingPreference(enabled);
    set({ richTextRenderingEnabled: enabled });
  },
  toggleRichTextRendering: () => {
    const enabled = !get().richTextRenderingEnabled;
    writeRichTextRenderingPreference(enabled);
    set({ richTextRenderingEnabled: enabled });
  },
});
