import * as React from "react";
import { useStickToBottomContext } from "use-stick-to-bottom";

import { cn } from "~/lib/utils";

interface ViewportVirtualItemProps extends React.HTMLAttributes<HTMLDivElement> {
  estimatedHeight: number;
  alwaysMounted?: boolean;
  rootMargin?: string;
  onHeightChange?: (height: number) => void;
}

export const ViewportVirtualItem = React.memo(
  ({
    estimatedHeight,
    alwaysMounted = false,
    className,
    rootMargin = "1200px 0px",
    onHeightChange,
    children,
    style,
    ...props
  }: ViewportVirtualItemProps) => {
    const { scrollRef } = useStickToBottomContext();
    const containerRef = React.useRef<HTMLDivElement | null>(null);
    const observerRef = React.useRef<IntersectionObserver | null>(null);
    const animationFrameRef = React.useRef<number | null>(null);
    const [isNearViewport, setIsNearViewport] = React.useState(alwaysMounted);
    const [measuredHeight, setMeasuredHeight] = React.useState(() => Math.max(estimatedHeight, 1));

    React.useEffect(() => {
      if (alwaysMounted) {
        setIsNearViewport(true);
      }
    }, [alwaysMounted]);

    React.useEffect(() => {
      setMeasuredHeight((current) => {
        if (current > 1) {
          return current;
        }
        return Math.max(estimatedHeight, 1);
      });
    }, [estimatedHeight]);

    React.useEffect(() => {
      if (alwaysMounted) {
        setIsNearViewport(true);
        return;
      }

      let cancelled = false;

      const attach = () => {
        if (cancelled) return;

        const root = scrollRef.current;
        const node = containerRef.current;
        if (!root || !node) {
          animationFrameRef.current = window.requestAnimationFrame(attach);
          return;
        }

        observerRef.current?.disconnect();
        observerRef.current = new IntersectionObserver(
          (entries) => {
            const entry = entries[0];
            if (!entry) return;
            setIsNearViewport(entry.isIntersecting);
          },
          {
            root,
            rootMargin,
          },
        );
        observerRef.current.observe(node);
      };

      attach();

      return () => {
        cancelled = true;
        if (animationFrameRef.current != null) {
          window.cancelAnimationFrame(animationFrameRef.current);
          animationFrameRef.current = null;
        }
        observerRef.current?.disconnect();
        observerRef.current = null;
      };
    }, [alwaysMounted, rootMargin, scrollRef]);

    React.useEffect(() => {
      if (!isNearViewport && !alwaysMounted) {
        return;
      }

      const node = containerRef.current;
      if (!node) {
        return;
      }

      const updateHeight = () => {
        const nextHeight = Math.max(node.offsetHeight, 1);
        setMeasuredHeight((current) => {
          if (Math.abs(current - nextHeight) < 1) {
            return current;
          }
          return nextHeight;
        });
        onHeightChange?.(nextHeight);
      };

      updateHeight();

      const resizeObserver = new ResizeObserver(() => {
        updateHeight();
      });
      resizeObserver.observe(node);

      return () => {
        resizeObserver.disconnect();
      };
    }, [alwaysMounted, isNearViewport, onHeightChange]);

    const placeholderHeight = Math.max(measuredHeight, estimatedHeight, 1);
    const shouldRenderChildren = alwaysMounted || isNearViewport;

    return (
      <div
        ref={containerRef}
        className={cn(className)}
        style={{
          ...style,
          minHeight: shouldRenderChildren ? undefined : placeholderHeight,
          height: shouldRenderChildren ? undefined : placeholderHeight,
          contentVisibility: "auto",
          containIntrinsicSize: `${Math.round(placeholderHeight)}px`,
        }}
        {...props}
      >
        {shouldRenderChildren ? children : null}
      </div>
    );
  },
);

ViewportVirtualItem.displayName = "ViewportVirtualItem";
