import type { StateCreator } from "zustand";

import type { AppStoreState, SettingsSlice } from "~/stores/slices/types";
import type { Settings } from "~/types";

function isPlainObject(value: unknown): value is Record<string, unknown> {
  if (value === null || typeof value !== "object") {
    return false;
  }

  const prototype = Object.getPrototypeOf(value);
  return prototype === Object.prototype || prototype === null;
}

function structuralShare<T>(previous: T, next: T): T {
  if (Object.is(previous, next)) {
    return previous;
  }

  if (Array.isArray(previous) && Array.isArray(next)) {
    let changed = previous.length !== next.length;
    const shared = next.map((item, index) => {
      const nextItem = index < previous.length ? structuralShare(previous[index], item) : item;
      if (!Object.is(nextItem, previous[index])) {
        changed = true;
      }
      return nextItem;
    });

    return (changed ? shared : previous) as T;
  }

  if (isPlainObject(previous) && isPlainObject(next)) {
    const previousKeys = Object.keys(previous);
    const nextKeys = Object.keys(next);
    let changed = previousKeys.length !== nextKeys.length;
    const shared: Record<string, unknown> = {};

    for (const key of nextKeys) {
      const nextValue = key in previous ? structuralShare(previous[key], next[key]) : next[key];
      shared[key] = nextValue;
      if (!(key in previous) || !Object.is(nextValue, previous[key])) {
        changed = true;
      }
    }

    return (changed ? shared : previous) as T;
  }

  return next;
}

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
