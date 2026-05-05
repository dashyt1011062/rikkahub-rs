import * as React from "react";
import { ImageOff, X } from "lucide-react";

import { Button } from "~/components/ui/button";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "~/components/ui/dialog";
import { resolveManagedFileUrl } from "~/lib/files";
import type { MessagePartMetadata } from "~/types";

interface ImagePartProps {
  url: string;
  metadata?: MessagePartMetadata | null;
}

type Point = { x: number; y: number };

const MIN_SCALE = 1;
const MAX_SCALE = 6;
const WHEEL_SCALE_STEP = 1.12;

function clampScale(value: number): number {
  return Math.min(MAX_SCALE, Math.max(MIN_SCALE, value));
}

function distanceBetween(a: Point, b: Point): number {
  return Math.hypot(a.x - b.x, a.y - b.y);
}

export function ImagePart({ url, metadata }: ImagePartProps) {
  const [error, setError] = React.useState(false);
  const [loaded, setLoaded] = React.useState(false);
  const [previewOpen, setPreviewOpen] = React.useState(false);
  const [scale, setScale] = React.useState(1);
  const [offset, setOffset] = React.useState<Point>({ x: 0, y: 0 });
  const [dragging, setDragging] = React.useState(false);
  const imageUrl = resolveManagedFileUrl(url, metadata);

  const scaleRef = React.useRef(scale);
  const offsetRef = React.useRef(offset);
  const activePointersRef = React.useRef<Map<number, Point>>(new Map());
  const dragStartRef = React.useRef<{
    pointerId: number;
    startX: number;
    startY: number;
    originX: number;
    originY: number;
  } | null>(null);
  const pinchStartRef = React.useRef<{
    distance: number;
    scale: number;
    offsetX: number;
    offsetY: number;
  } | null>(null);

  React.useEffect(() => {
    scaleRef.current = scale;
  }, [scale]);

  React.useEffect(() => {
    offsetRef.current = offset;
  }, [offset]);

  const resetPreviewState = React.useCallback(() => {
    activePointersRef.current.clear();
    dragStartRef.current = null;
    pinchStartRef.current = null;
    scaleRef.current = 1;
    offsetRef.current = { x: 0, y: 0 };
    setScale(1);
    setOffset({ x: 0, y: 0 });
    setDragging(false);
  }, []);

  const updateView = React.useCallback((nextScale: number, nextOffset: Point) => {
    scaleRef.current = nextScale;
    offsetRef.current = nextOffset;
    setScale(nextScale);
    setOffset(nextOffset);
  }, []);

  const applyCenteredScale = React.useCallback(
    (targetScale: number, baseScale = scaleRef.current, baseOffset = offsetRef.current) => {
      const nextScale = clampScale(targetScale);
      const ratio = nextScale / baseScale;
      if (!Number.isFinite(ratio) || Math.abs(nextScale - baseScale) < 0.001) {
        return;
      }

      updateView(
        nextScale,
        nextScale <= MIN_SCALE
          ? { x: 0, y: 0 }
          : {
              x: baseOffset.x * ratio,
              y: baseOffset.y * ratio,
            },
      );
    },
    [updateView],
  );

  const openPreview = React.useCallback(() => {
    resetPreviewState();
    setPreviewOpen(true);
  }, [resetPreviewState]);

  const handleOpenChange = React.useCallback(
    (open: boolean) => {
      setPreviewOpen(open);
      if (!open) {
        resetPreviewState();
      }
    },
    [resetPreviewState],
  );

  const beginDragFromPointer = React.useCallback((pointerId: number, point: Point) => {
    dragStartRef.current = {
      pointerId,
      startX: point.x,
      startY: point.y,
      originX: offsetRef.current.x,
      originY: offsetRef.current.y,
    };
  }, []);

  const handlePointerDown = React.useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      if (event.button !== 0 && event.pointerType === "mouse") {
        return;
      }

      const point = { x: event.clientX, y: event.clientY };
      activePointersRef.current.set(event.pointerId, point);
      event.currentTarget.setPointerCapture(event.pointerId);

      if (activePointersRef.current.size >= 2) {
        const points = Array.from(activePointersRef.current.values());
        pinchStartRef.current = {
          distance: distanceBetween(points[0], points[1]),
          scale: scaleRef.current,
          offsetX: offsetRef.current.x,
          offsetY: offsetRef.current.y,
        };
        dragStartRef.current = null;
        setDragging(false);
        return;
      }

      if (scaleRef.current > 1) {
        beginDragFromPointer(event.pointerId, point);
        setDragging(true);
      }
    },
    [beginDragFromPointer],
  );

  const handlePointerMove = React.useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      if (!activePointersRef.current.has(event.pointerId)) {
        return;
      }

      const point = { x: event.clientX, y: event.clientY };
      activePointersRef.current.set(event.pointerId, point);

      if (activePointersRef.current.size >= 2) {
        const points = Array.from(activePointersRef.current.values());
        const pinchStart = pinchStartRef.current;
        if (!pinchStart || pinchStart.distance <= 0) {
          return;
        }

        event.preventDefault();
        const currentDistance = distanceBetween(points[0], points[1]);
        applyCenteredScale(
          pinchStart.scale * (currentDistance / pinchStart.distance),
          pinchStart.scale,
          { x: pinchStart.offsetX, y: pinchStart.offsetY },
        );
        setDragging(false);
        return;
      }

      const dragState = dragStartRef.current;
      if (!dragState || dragState.pointerId !== event.pointerId) {
        return;
      }

      event.preventDefault();
      updateView(scaleRef.current, {
        x: dragState.originX + (point.x - dragState.startX),
        y: dragState.originY + (point.y - dragState.startY),
      });
      setDragging(scaleRef.current > 1);
    },
    [applyCenteredScale, updateView],
  );

  const finishPointer = React.useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      activePointersRef.current.delete(event.pointerId);

      if (activePointersRef.current.size >= 2) {
        const points = Array.from(activePointersRef.current.values());
        pinchStartRef.current = {
          distance: distanceBetween(points[0], points[1]),
          scale: scaleRef.current,
          offsetX: offsetRef.current.x,
          offsetY: offsetRef.current.y,
        };
        return;
      }

      pinchStartRef.current = null;

      if (activePointersRef.current.size === 1 && scaleRef.current > 1) {
        const [[pointerId, point]] = Array.from(activePointersRef.current.entries());
        beginDragFromPointer(pointerId, point);
        setDragging(false);
        return;
      }

      dragStartRef.current = null;
      setDragging(false);
    },
    [beginDragFromPointer],
  );

  const handleWheel = React.useCallback(
    (event: React.WheelEvent<HTMLDivElement>) => {
      event.preventDefault();
      const direction = event.deltaY < 0 ? WHEEL_SCALE_STEP : 1 / WHEEL_SCALE_STEP;
      applyCenteredScale(scaleRef.current * direction);
    },
    [applyCenteredScale],
  );

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
      <div className="relative my-2 max-w-md">
        {!loaded && (
          <div className="flex h-48 items-center justify-center rounded-md border border-muted bg-muted/30">
            <div className="text-sm text-muted-foreground">Loading image...</div>
          </div>
        )}
        <img
          src={imageUrl}
          alt="Message attachment"
          className={`cursor-zoom-in rounded-md border border-muted object-contain ${loaded ? "block" : "hidden"}`}
          onClick={openPreview}
          onLoad={() => setLoaded(true)}
          onError={() => setError(true)}
          draggable={false}
          style={{ maxHeight: "500px", width: "auto" }}
        />
      </div>

      <Dialog open={previewOpen} onOpenChange={handleOpenChange}>
        <DialogContent
          showCloseButton={false}
          className="top-0 left-0 h-screen w-screen max-w-none translate-x-0 translate-y-0 gap-0 rounded-none border-0 bg-black/92 p-0 shadow-none"
          onOpenAutoFocus={(event) => {
            event.preventDefault();
          }}
        >
          <DialogTitle className="sr-only">Image preview</DialogTitle>
          <DialogDescription className="sr-only">
            Use mouse wheel or two fingers to zoom the image.
          </DialogDescription>

          <DialogClose asChild>
            <Button
              type="button"
              variant="secondary"
              size="icon"
              className="absolute top-4 left-4 z-20 size-10 rounded-full bg-black/60 text-white hover:bg-black/75 hover:text-white"
            >
              <X className="size-5" />
            </Button>
          </DialogClose>

          <div
            className="flex h-full w-full items-center justify-center overflow-hidden select-none touch-none"
            onPointerDown={handlePointerDown}
            onPointerMove={handlePointerMove}
            onPointerUp={finishPointer}
            onPointerCancel={finishPointer}
            onPointerLeave={(event) => {
              if (event.pointerType === "mouse" && activePointersRef.current.has(event.pointerId)) {
                finishPointer(event);
              }
            }}
            onWheel={handleWheel}
          >
            <div
              className={dragging ? "cursor-grabbing" : scale > 1 ? "cursor-grab" : "cursor-default"}
              style={{
                transform: `translate3d(${offset.x}px, ${offset.y}px, 0)`,
                willChange: "transform",
              }}
            >
              <img
                src={imageUrl}
                alt="Preview"
                className="max-h-[calc(100vh-2rem)] max-w-[calc(100vw-2rem)] object-contain"
                draggable={false}
                style={{
                  transform: `scale(${scale})`,
                  transformOrigin: "center center",
                  willChange: "transform",
                }}
              />
            </div>
          </div>
        </DialogContent>
      </Dialog>
    </>
  );
}
