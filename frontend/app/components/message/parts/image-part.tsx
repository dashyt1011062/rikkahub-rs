import * as React from "react";
import { ImageOff } from "lucide-react";
import { resolveManagedFileUrl } from "~/lib/files";
import type { MessagePartMetadata } from "~/types";

interface ImagePartProps {
  url: string;
  metadata?: MessagePartMetadata | null;
}

export function ImagePart({ url, metadata }: ImagePartProps) {
  const [error, setError] = React.useState(false);
  const [loaded, setLoaded] = React.useState(false);
  const imageUrl = resolveManagedFileUrl(url, metadata);

  if (!url) return null;

  if (error) {
    return (
      <div className="flex items-center gap-2 rounded-md border border-destructive/50 bg-destructive/10 px-3 py-2 text-sm text-destructive">
        <ImageOff className="h-4 w-4" />
        <span>Failed to load image: {imageUrl}</span>
      </div>
    );
  }

  return (
    <div className="relative my-2 max-w-md">
      {!loaded && (
        <div className="flex h-48 items-center justify-center rounded-md border border-muted bg-muted/30">
          <div className="text-sm text-muted-foreground">Loading image...</div>
        </div>
      )}
      <img
        src={imageUrl}
        alt="Message attachment"
        className={`rounded-md border border-muted object-contain ${loaded ? "block" : "hidden"}`}
        onLoad={() => setLoaded(true)}
        onError={() => setError(true)}
        style={{ maxHeight: "500px", width: "auto" }}
      />
    </div>
  );
}
