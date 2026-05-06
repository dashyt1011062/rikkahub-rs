import * as React from "react";
import { useShallow } from "zustand/react/shallow";

import { useSettingsStore } from "~/stores";
import type { AssistantProfile, Settings } from "~/types";

export interface UseCurrentAssistantResult {
  settings: Settings | null;
  assistants: AssistantProfile[];
  currentAssistantId: string | null;
  currentAssistant: AssistantProfile | null;
}

const EMPTY_ASSISTANTS: AssistantProfile[] = [];

export function resolveCurrentAssistant(
  assistants: AssistantProfile[],
  currentAssistantId: string | null,
): AssistantProfile | null {
  if (assistants.length === 0) {
    return null;
  }

  return assistants.find((assistant) => assistant.id === currentAssistantId) ?? assistants[0] ?? null;
}

export function useCurrentAssistantProfile(): AssistantProfile | null {
  const { assistants, currentAssistantId } = useSettingsStore(
    useShallow((state) => ({
      assistants: state.settings?.assistants ?? EMPTY_ASSISTANTS,
      currentAssistantId: state.settings?.assistantId ?? null,
    })),
  );

  return React.useMemo(
    () => resolveCurrentAssistant(assistants, currentAssistantId),
    [assistants, currentAssistantId],
  );
}

export function useCurrentAssistant(): UseCurrentAssistantResult {
  const settings = useSettingsStore((state) => state.settings);
  const { assistants, currentAssistantId } = useSettingsStore(
    useShallow((state) => ({
      assistants: state.settings?.assistants ?? EMPTY_ASSISTANTS,
      currentAssistantId: state.settings?.assistantId ?? null,
    })),
  );

  const currentAssistant = React.useMemo(
    () => resolveCurrentAssistant(assistants, currentAssistantId),
    [assistants, currentAssistantId],
  );

  return {
    settings,
    assistants,
    currentAssistantId,
    currentAssistant,
  };
}
