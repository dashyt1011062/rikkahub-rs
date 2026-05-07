import { create } from "zustand";

import { createChatInputSlice } from "~/stores/slices/chat-input-slice";
import { createSettingsSlice } from "~/stores/slices/settings-slice";
import { createUiPreferencesSlice } from "~/stores/slices/ui-preferences-slice";
import type { AppStoreState } from "~/stores/slices/types";

export const useAppStore = create<AppStoreState>()((...args) => ({
  ...createSettingsSlice(...args),
  ...createChatInputSlice(...args),
  ...createUiPreferencesSlice(...args),
}));

export const useSettingsStore = useAppStore;
export const useChatInputStore = useAppStore;

export type {
  AppStoreState,
  ChatInputSlice,
  Draft,
  SettingsSlice,
  UiPreferencesSlice,
} from "~/stores/slices/types";
