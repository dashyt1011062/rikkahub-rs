import * as React from "react";
import { useShallow } from "zustand/react/shallow";

import { resolveCurrentAssistant, useCurrentAssistant } from "~/hooks/use-current-assistant";
import { useSettingsStore } from "~/stores";
import type { ProviderModel, ProviderProfile } from "~/types";

export interface UseCurrentModelResult {
  currentModelId: string | null;
  currentModel: ProviderModel | null;
  currentProvider: ProviderProfile | null;
}

const EMPTY_PROVIDERS: ProviderProfile[] = [];

function resolveCurrentModelFromProviders(
  providers: ProviderProfile[],
  currentModelId: string | null,
): Pick<UseCurrentModelResult, "currentModel" | "currentProvider"> {
  if (!currentModelId) {
    return {
      currentModel: null,
      currentProvider: null,
    };
  }

  for (const provider of providers) {
    const model = provider.models.find((item) => item.id === currentModelId);
    if (model) {
      return {
        currentModel: model,
        currentProvider: provider,
      };
    }
  }

  return {
    currentModel: null,
    currentProvider: null,
  };
}

export function useCurrentModelSelection(): UseCurrentModelResult {
  const { providers, currentModelId } = useSettingsStore(
    useShallow((state) => {
      const settings = state.settings;
      const assistants = settings?.assistants ?? [];
      const currentAssistantId = settings?.assistantId ?? null;
      const currentAssistant = resolveCurrentAssistant(assistants, currentAssistantId);
      return {
        providers: settings?.providers ?? EMPTY_PROVIDERS,
        currentModelId: currentAssistant?.chatModelId ?? settings?.chatModelId ?? null,
      };
    }),
  );

  const { currentModel, currentProvider } = React.useMemo(
    () => resolveCurrentModelFromProviders(providers, currentModelId),
    [currentModelId, providers],
  );

  return {
    currentModelId,
    currentModel,
    currentProvider,
  };
}

export function useCurrentModel(): UseCurrentModelResult {
  const { settings, currentAssistant } = useCurrentAssistant();

  const currentModelId = currentAssistant?.chatModelId ?? settings?.chatModelId ?? null;

  const { currentModel, currentProvider } = React.useMemo(
    () => resolveCurrentModelFromProviders(settings?.providers ?? EMPTY_PROVIDERS, currentModelId),
    [currentModelId, settings?.providers],
  );

  return {
    currentModelId,
    currentModel,
    currentProvider,
  };
}
