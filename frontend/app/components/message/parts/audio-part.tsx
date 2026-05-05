import * as React from "react";
import { AudioLines, VolumeX } from "lucide-react";
import { useTranslation } from "react-i18next";

import { resolveManagedFileUrl } from "~/lib/files";
import type { MessagePartMetadata } from "~/types";

interface AudioPartProps {
  url: string;
  metadata?: MessagePartMetadata | null;
}

export function AudioPart({ url, metadata }: AudioPartProps) {
  const { t } = useTranslation("message");
  const [error, setError] = React.useState(false);

  if (!url) return null;

  const audioUrl = resolveManagedFileUrl(url, metadata);

  if (error) {
    return (
      <div className="flex items-center gap-2 rounded-md border border-destructive/50 bg-destructive/10 px-3 py-2 text-sm text-destructive">
        <VolumeX className="h-4 w-4" />
        <span>{t("media_part.audio_load_failed", { url: audioUrl })}</span>
      </div>
    );
  }

  return (
    <div className="my-2 max-w-md space-y-2 rounded-xl border border-muted bg-card p-3">
      <audio
        className="w-full"
        controls
        onError={() => setError(true)}
        preload="metadata"
        src={audioUrl}
      />
      <a
        className="text-muted-foreground inline-flex items-center gap-1 text-xs hover:underline"
        href={audioUrl}
        rel="noreferrer"
        target="_blank"
      >
        <AudioLines className="h-3.5 w-3.5" />
        {t("media_part.open_audio_in_new_window")}
      </a>
    </div>
  );
}
