import * as React from "react";
import { ImageOff } from "lucide-react";

import { resolveManagedFileUrl } from "~/lib/files";
import type { MessagePartMetadata } from "~/types";

interface ImagePartProps {
  url: string;
  metadata?: MessagePartMetadata | null;
}

const LazyImagePreviewDialog = React.lazy(() => import("./image-preview-dialog"));

function positiveDimension(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) && value > 0 ? value : undefined;
}

export function ImagePart({ url, metadata }: ImagePartProps) {
  const [error, setError] = React.useState(false);
  const [loaded, setLoaded] = React.useState(false);
  const [previewOpen, setPreviewOpen] = React.useState(false);
  const imageUrl = resolveManagedFileUrl(url, metadata);
  const imageWidth = positiveDimension(metadata?.width);
  const imageHeight = positiveDimension(metadata?.height);
  const hasDimensions = imageWidth !== undefined && imageHeight !== undefined;

  React.useEffect(() => {
    setError(false);
    setLoaded(false);
    setPreviewOpen(false);
  }, [imageUrl]);

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
    <>
      <div className={`relative my-2 max-w-md ${loaded || hasDimensions ? "" : "min-h-48"}`}>
        {!loaded && (
          <div className="absolute inset-0 flex items-center justify-center rounded-md border border-muted bg-muted/30">
            <div className="text-sm text-muted-foreground">Loading image...</div>
          </div>
        )}
        <img
          src={imageUrl}
          alt="Message attachment"
          className={`block cursor-zoom-in rounded-md border border-muted object-contain transition-opacity ${loaded ? "opacity-100" : "opacity-0"}`}
          onClick={() => setPreviewOpen(true)}
          onLoad={() => setLoaded(true)}
          onError={() => setError(true)}
          draggable={false}
          loading="lazy"
          decoding="async"
          fetchPriority="auto"
          width={imageWidth}
          height={imageHeight}
          style={{
            maxHeight: "500px",
            width: "auto",
            aspectRatio: hasDimensions ? `${imageWidth} / ${imageHeight}` : undefined,
          }}
        />
      </div>

      {previewOpen ? (
        <React.Suspense fallback={null}>
          <LazyImagePreviewDialog
            open={previewOpen}
            imageUrl={imageUrl}
            onOpenChange={setPreviewOpen}
          />
        </React.Suspense>
      ) : null}
    </>
  );
}
