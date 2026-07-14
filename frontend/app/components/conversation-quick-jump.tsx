import * as React from "react";
import { ArrowDown, ArrowDownToLine, ArrowUp } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Tooltip, TooltipContent, TooltipTrigger } from "~/components/ui/tooltip";
import { Button } from "~/components/ui/button";
import { cn } from "~/lib/utils";
import { useStickToBottomContext } from "use-stick-to-bottom";

export function getConversationMessageAnchorId(messageId: string): string {
  return `message-anchor-${messageId}`;
}

export interface ConversationQuickJumpItem {
  id: string;
  role: string;
  preview?: string;
}

interface ConversationNavigationProps {
  items: ConversationQuickJumpItem[];
  showQuickJump?: boolean;
}

interface ConversationAnchorOffset extends ConversationQuickJumpItem {
  top: number;
}

function getRoleLineClass(role: string): string {
  const normalizedRole = role.toUpperCase();
  if (normalizedRole === "USER") {
    return "bg-primary/35 hover:bg-primary/60";
  }

  if (normalizedRole === "ASSISTANT") {
    return "bg-foreground/25 hover:bg-foreground/50";
  }

  return "bg-muted hover:bg-foreground/40";
}

function getRoleDotClass(role: string): string {
  const normalizedRole = role.toUpperCase();
  if (normalizedRole === "USER") {
    return "bg-primary";
  }

  if (normalizedRole === "ASSISTANT") {
    return "bg-foreground";
  }

  return "bg-foreground/80";
}

function getRoleLabel(
  role: string,
  t: (key: string, options?: Record<string, unknown>) => string,
): string {
  const normalizedRole = role.toUpperCase();
  if (normalizedRole === "USER") return t("quick_jump.role_user");
  if (normalizedRole === "ASSISTANT") return t("quick_jump.role_assistant");
  return t("quick_jump.role_message");
}

const SCROLL_ANCHOR_OFFSET = 40;
const MESSAGE_TOLERANCE = 12;

function isUserMessage(item: ConversationQuickJumpItem): boolean {
  return item.role.toUpperCase() === "USER";
}

function getAnchorOffset(scrollElement: HTMLElement, messageId: string): number | null {
  const anchor = document.getElementById(getConversationMessageAnchorId(messageId));
  if (!anchor) return null;

  const containerRect = scrollElement.getBoundingClientRect();
  return anchor.getBoundingClientRect().top - containerRect.top + scrollElement.scrollTop;
}

function buildAnchorOffsets(
  items: ConversationQuickJumpItem[],
  scrollElement: HTMLElement,
): ConversationAnchorOffset[] {
  return items
    .map((item) => {
      const top = getAnchorOffset(scrollElement, item.id);
      if (top === null) return null;
      return {
        ...item,
        top,
      };
    })
    .filter((item): item is ConversationAnchorOffset => item !== null);
}

function areAnchorOffsetsEqual(
  previous: ConversationAnchorOffset[],
  next: ConversationAnchorOffset[],
): boolean {
  return (
    previous.length === next.length &&
    previous.every(
      (item, index) =>
        item.id === next[index]?.id &&
        item.role === next[index]?.role &&
        item.top === next[index]?.top &&
        item.preview === next[index]?.preview,
    )
  );
}

function resolveActiveMessageIdFromOffsets(
  items: ConversationQuickJumpItem[],
  anchorOffsets: ConversationAnchorOffset[],
  scrollElement: HTMLElement | null,
): string | null {
  if (!scrollElement || items.length === 0) {
    return items[items.length - 1]?.id ?? null;
  }
  if (anchorOffsets.length === 0) {
    return items[items.length - 1]?.id ?? null;
  }

  const viewportTop = scrollElement.scrollTop + 24;
  let activeId = anchorOffsets[0]?.id ?? items[items.length - 1]?.id ?? null;

  for (const item of anchorOffsets) {
    if (item.top <= viewportTop) {
      activeId = item.id;
    } else {
      break;
    }
  }

  return activeId;
}

function getAnchorScrollMarginTop(anchor: HTMLElement): number {
  const scrollMarginTop = Number.parseFloat(window.getComputedStyle(anchor).scrollMarginTop);
  return Number.isFinite(scrollMarginTop) && scrollMarginTop > 0
    ? scrollMarginTop
    : SCROLL_ANCHOR_OFFSET;
}

function resolveCurrentUserMessageId(
  items: ConversationQuickJumpItem[],
  scrollElement: HTMLElement,
): string | null {
  const userItems = items.filter(isUserMessage);
  if (userItems.length === 0) return null;

  const containerTop = scrollElement.getBoundingClientRect().top;
  let currentId = userItems[0]?.id ?? null;

  for (const item of userItems) {
    const anchor = document.getElementById(getConversationMessageAnchorId(item.id));
    if (!anchor) continue;

    const anchorTop = anchor.getBoundingClientRect().top - containerTop;
    if (anchorTop <= getAnchorScrollMarginTop(anchor) + MESSAGE_TOLERANCE) {
      currentId = item.id;
      continue;
    }
    break;
  }

  return currentId;
}

function useConversationAnchorOffsets(items: ConversationQuickJumpItem[]) {
  const { scrollRef, contentRef } = useStickToBottomContext();
  const anchorOffsetsRef = React.useRef<ConversationAnchorOffset[]>([]);
  const activeUserMessageIdRef = React.useRef<string | null>(null);
  const [activeUserMessageId, setActiveUserMessageId] = React.useState<string | null>(null);
  const jumpTargetIdRef = React.useRef<string | null>(null);
  const jumpResizeObserverRef = React.useRef<ResizeObserver | null>(null);
  const jumpFrameRef = React.useRef<number | null>(null);
  const jumpReadyTimerRef = React.useRef<number | null>(null);
  const jumpAlignmentReadyAtRef = React.useRef(0);

  const rebuildAnchorOffsets = React.useCallback(() => {
    const scrollElement = scrollRef.current;
    if (!scrollElement) {
      anchorOffsetsRef.current = [];
      return anchorOffsetsRef.current;
    }

    const next = buildAnchorOffsets(items, scrollElement);
    if (!areAnchorOffsetsEqual(anchorOffsetsRef.current, next)) {
      anchorOffsetsRef.current = next;
    }
    return anchorOffsetsRef.current;
  }, [items, scrollRef]);

  const syncActiveUserMessage = React.useCallback(() => {
    const lockedTargetId = jumpTargetIdRef.current;
    if (lockedTargetId && items.some((item) => item.id === lockedTargetId && isUserMessage(item))) {
      activeUserMessageIdRef.current = lockedTargetId;
      setActiveUserMessageId((previousId) =>
        previousId === lockedTargetId ? previousId : lockedTargetId,
      );
      return lockedTargetId;
    }

    const scrollElement = scrollRef.current;
    if (!scrollElement) {
      activeUserMessageIdRef.current = null;
      setActiveUserMessageId(null);
      return null;
    }

    const nextId = resolveCurrentUserMessageId(items, scrollElement);
    activeUserMessageIdRef.current = nextId;
    setActiveUserMessageId((previousId) => (previousId === nextId ? previousId : nextId));
    return nextId;
  }, [items, scrollRef]);

  const cancelJumpLock = React.useCallback(() => {
    jumpTargetIdRef.current = null;
    jumpResizeObserverRef.current?.disconnect();
    jumpResizeObserverRef.current = null;
    if (jumpFrameRef.current !== null) {
      window.cancelAnimationFrame(jumpFrameRef.current);
      jumpFrameRef.current = null;
    }
    if (jumpReadyTimerRef.current !== null) {
      window.clearTimeout(jumpReadyTimerRef.current);
      jumpReadyTimerRef.current = null;
    }
  }, []);

  const alignJumpTarget = React.useCallback(() => {
    jumpFrameRef.current = null;
    const targetId = jumpTargetIdRef.current;
    const scrollElement = scrollRef.current;
    if (!targetId || !scrollElement) return;

    const anchor = document.getElementById(getConversationMessageAnchorId(targetId));
    if (!anchor) {
      cancelJumpLock();
      return;
    }

    const containerTop = scrollElement.getBoundingClientRect().top;
    const anchorTop = anchor.getBoundingClientRect().top - containerTop;
    const alignmentError = anchorTop - getAnchorScrollMarginTop(anchor);
    if (Math.abs(alignmentError) > 1.5) {
      scrollElement.scrollTop += alignmentError;
    }
    syncActiveUserMessage();
  }, [cancelJumpLock, scrollRef, syncActiveUserMessage]);

  const scheduleJumpAlignment = React.useCallback(() => {
    if (!jumpTargetIdRef.current) return;

    const remainingDelay = jumpAlignmentReadyAtRef.current - performance.now();
    if (remainingDelay > 0) {
      if (jumpReadyTimerRef.current === null) {
        jumpReadyTimerRef.current = window.setTimeout(() => {
          jumpReadyTimerRef.current = null;
          scheduleJumpAlignment();
        }, remainingDelay);
      }
      return;
    }

    if (jumpFrameRef.current !== null) return;
    jumpFrameRef.current = window.requestAnimationFrame(alignJumpTarget);
  }, [alignJumpTarget]);

  const jumpToMessage = React.useCallback(
    (messageId: string, behavior: ScrollBehavior = "smooth") => {
      const anchor = document.getElementById(getConversationMessageAnchorId(messageId));
      const scrollElement = scrollRef.current;
      if (!anchor || !scrollElement) return false;

      cancelJumpLock();
      jumpTargetIdRef.current = messageId;
      jumpAlignmentReadyAtRef.current = performance.now() + (behavior === "smooth" ? 320 : 0);
      if (items.some((item) => item.id === messageId && isUserMessage(item))) {
        activeUserMessageIdRef.current = messageId;
        setActiveUserMessageId(messageId);
      }

      anchor.scrollIntoView({ behavior, block: "start" });

      const observer = new ResizeObserver(scheduleJumpAlignment);
      observer.observe(anchor);
      jumpResizeObserverRef.current = observer;
      scheduleJumpAlignment();
      return true;
    },
    [cancelJumpLock, items, scheduleJumpAlignment, scrollRef],
  );

  React.useEffect(() => {
    const scrollElement = scrollRef.current;
    const contentElement = contentRef.current;
    let frameId: number | null = null;

    const scheduleMeasurements = () => {
      if (frameId !== null) return;
      frameId = window.requestAnimationFrame(() => {
        frameId = null;
        rebuildAnchorOffsets();
        syncActiveUserMessage();
        scheduleJumpAlignment();
      });
    };

    scheduleMeasurements();

    const resizeObserver = contentElement ? new ResizeObserver(scheduleMeasurements) : null;
    if (contentElement && resizeObserver) {
      resizeObserver.observe(contentElement);
    }

    const intersectionObserver = scrollElement
      ? new IntersectionObserver(scheduleMeasurements, {
          root: scrollElement,
          rootMargin: "-1px 0px -65% 0px",
          threshold: [0, 1],
        })
      : null;
    if (intersectionObserver) {
      for (const item of items.filter(isUserMessage)) {
        const anchor = document.getElementById(getConversationMessageAnchorId(item.id));
        if (anchor) intersectionObserver.observe(anchor);
      }
    }

    scrollElement?.addEventListener("scroll", scheduleMeasurements, { passive: true });
    window.addEventListener("resize", scheduleMeasurements);

    return () => {
      resizeObserver?.disconnect();
      intersectionObserver?.disconnect();
      scrollElement?.removeEventListener("scroll", scheduleMeasurements);
      window.removeEventListener("resize", scheduleMeasurements);
      if (frameId !== null) {
        window.cancelAnimationFrame(frameId);
      }
    };
  }, [
    contentRef,
    items,
    rebuildAnchorOffsets,
    scheduleJumpAlignment,
    scrollRef,
    syncActiveUserMessage,
  ]);

  React.useEffect(() => {
    const scrollElement = scrollRef.current;
    if (!scrollElement) return;

    const cancelFromPointer = () => cancelJumpLock();
    const cancelFromKeyboard = (event: KeyboardEvent) => {
      if (
        event.key === "ArrowUp" ||
        event.key === "ArrowDown" ||
        event.key === "PageUp" ||
        event.key === "PageDown" ||
        event.key === "Home" ||
        event.key === "End" ||
        event.key === " "
      ) {
        cancelJumpLock();
      }
    };

    scrollElement.addEventListener("wheel", cancelFromPointer, { passive: true });
    scrollElement.addEventListener("touchstart", cancelFromPointer, { passive: true });
    scrollElement.addEventListener("pointerdown", cancelFromPointer, { passive: true });
    window.addEventListener("keydown", cancelFromKeyboard);

    return () => {
      scrollElement.removeEventListener("wheel", cancelFromPointer);
      scrollElement.removeEventListener("touchstart", cancelFromPointer);
      scrollElement.removeEventListener("pointerdown", cancelFromPointer);
      window.removeEventListener("keydown", cancelFromKeyboard);
    };
  }, [cancelJumpLock, scrollRef]);

  const getAdjacentUserMessageId = React.useCallback(
    (direction: "older" | "newer") => {
      const userItems = items.filter(isUserMessage);
      const currentId = activeUserMessageIdRef.current ?? syncActiveUserMessage();
      const currentIndex = userItems.findIndex((item) => item.id === currentId);
      if (currentIndex < 0) return null;
      return direction === "older"
        ? (userItems[currentIndex - 1]?.id ?? null)
        : (userItems[currentIndex + 1]?.id ?? null);
    },
    [items, syncActiveUserMessage],
  );

  React.useEffect(() => {
    const targetId = jumpTargetIdRef.current;
    if (targetId && !items.some((item) => item.id === targetId)) {
      cancelJumpLock();
    }
  }, [cancelJumpLock, items]);

  React.useEffect(() => cancelJumpLock, [cancelJumpLock]);

  return {
    scrollRef,
    anchorOffsetsRef,
    rebuildAnchorOffsets,
    activeUserMessageId,
    syncActiveUserMessage,
    getAdjacentUserMessageId,
    jumpToMessage,
    cancelJumpLock,
  };
}

type ConversationAnchorOffsetsController = ReturnType<typeof useConversationAnchorOffsets>;

interface ConversationQuickJumpProps {
  items: ConversationQuickJumpItem[];
  controller: ConversationAnchorOffsetsController;
}

interface ConversationScrollControlsProps {
  items: ConversationQuickJumpItem[];
  controller: ConversationAnchorOffsetsController;
}

function ConversationQuickJump({ items, controller }: ConversationQuickJumpProps) {
  const { t } = useTranslation();
  const { scrollRef, anchorOffsetsRef, rebuildAnchorOffsets, jumpToMessage } = controller;
  const [activeMessageId, setActiveMessageId] = React.useState<string | null>(null);
  const canQuickJump = items.length > 1 && items.length <= 128;
  const activeIndex = React.useMemo(() => {
    if (!activeMessageId) return 0;
    const index = items.findIndex((item) => item.id === activeMessageId);
    return index >= 0 ? index + 1 : 0;
  }, [activeMessageId, items]);

  const resolveActiveMessageId = React.useCallback(() => {
    const scrollElement = scrollRef.current;
    return resolveActiveMessageIdFromOffsets(items, anchorOffsetsRef.current, scrollElement);
  }, [anchorOffsetsRef, items, scrollRef]);

  React.useEffect(() => {
    if (!canQuickJump) {
      setActiveMessageId(null);
      return;
    }

    const scrollElement = scrollRef.current;
    if (!scrollElement) {
      rebuildAnchorOffsets();
      setActiveMessageId(resolveActiveMessageId());
      return;
    }

    let timeoutId: number | null = null;
    const updateActive = () => {
      const nextActiveId = resolveActiveMessageId();
      setActiveMessageId((prev) => (prev === nextActiveId ? prev : nextActiveId));
    };
    const scheduleUpdate = () => {
      if (timeoutId !== null) return;
      timeoutId = window.setTimeout(() => {
        timeoutId = null;
        updateActive();
      }, 48);
    };

    rebuildAnchorOffsets();
    updateActive();
    scrollElement.addEventListener("scroll", scheduleUpdate, { passive: true });

    return () => {
      scrollElement.removeEventListener("scroll", scheduleUpdate);
      if (timeoutId !== null) {
        window.clearTimeout(timeoutId);
      }
    };
  }, [canQuickJump, rebuildAnchorOffsets, resolveActiveMessageId, scrollRef]);

  const handleQuickJump = React.useCallback(
    (messageId: string) => {
      if (!jumpToMessage(messageId)) return;
      setActiveMessageId(messageId);
    },
    [jumpToMessage],
  );

  if (!canQuickJump) {
    return null;
  }

  return (
    <div className="pointer-events-none absolute inset-y-0 right-4 z-20 hidden items-center lg:flex">
      <div className="pointer-events-auto flex max-h-[calc(100%-2rem)] flex-col items-end gap-1 overflow-y-auto py-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
        {items.map((item, index) => {
          const isActive = activeMessageId === item.id;
          const roleLabel = getRoleLabel(item.role, t);

          return (
            <Tooltip key={`quick-jump-${item.id}`}>
              <TooltipTrigger asChild>
                <button
                  type="button"
                  className="flex w-8 items-center justify-end gap-1 transition-colors"
                  aria-label={t("quick_jump.jump_to_message", {
                    index: index + 1,
                    role: roleLabel,
                  })}
                  title={t("quick_jump.message_title", { index: index + 1, role: roleLabel })}
                  onClick={() => {
                    handleQuickJump(item.id);
                  }}
                >
                  <span
                    className={cn(
                      "h-1.5 w-5 rounded-full transition-colors",
                      getRoleLineClass(item.role),
                      isActive && "bg-foreground/80",
                    )}
                  />
                  <span
                    className={cn(
                      "size-1.5 rounded-full transition-opacity duration-200",
                      getRoleDotClass(item.role),
                      isActive ? "animate-pulse opacity-100" : "opacity-0",
                    )}
                  />
                </button>
              </TooltipTrigger>
              <TooltipContent side="left" sideOffset={8} className="max-w-64 text-left">
                <div className="space-y-0.5">
                  <div className="text-[11px] text-background/75">
                    {index + 1}/{items.length} · {roleLabel}
                  </div>
                  <div>{item.preview?.trim() || t("quick_jump.no_preview")}</div>
                </div>
              </TooltipContent>
            </Tooltip>
          );
        })}
        <div className="mt-1 w-8 text-center text-[10px] text-muted-foreground/80 tabular-nums">
          {activeIndex}/{items.length}
        </div>
      </div>
    </div>
  );
}

function ConversationScrollControls({ items, controller }: ConversationScrollControlsProps) {
  const { t } = useTranslation();
  const { isAtBottom, scrollToBottom } = useStickToBottomContext();
  const {
    scrollRef,
    activeUserMessageId,
    syncActiveUserMessage,
    getAdjacentUserMessageId,
    jumpToMessage,
    cancelJumpLock,
  } = controller;
  const [direction, setDirection] = React.useState<"older" | "newer" | null>(null);
  const lastScrollTopRef = React.useRef(0);
  const userItems = React.useMemo(() => items.filter(isUserMessage), [items]);
  const activeUserIndex = React.useMemo(
    () => userItems.findIndex((item) => item.id === activeUserMessageId),
    [activeUserMessageId, userItems],
  );
  const previousId = activeUserIndex > 0 ? (userItems[activeUserIndex - 1]?.id ?? null) : null;
  const nextId =
    activeUserIndex >= 0 ? (userItems[activeUserIndex + 1]?.id ?? null) : (userItems[0]?.id ?? null);

  React.useEffect(() => {
    const scrollElement = scrollRef.current;
    if (!scrollElement) return;

    let timeoutId: number | null = null;
    lastScrollTopRef.current = scrollElement.scrollTop;
    syncActiveUserMessage();

    const update = () => {
      const currentScrollTop = scrollElement.scrollTop;
      const delta = currentScrollTop - lastScrollTopRef.current;
      if (Math.abs(delta) > 2) {
        setDirection(delta > 0 ? "newer" : "older");
      }
      lastScrollTopRef.current = currentScrollTop;
      syncActiveUserMessage();
    };

    const scheduleUpdate = () => {
      if (timeoutId !== null) return;
      timeoutId = window.setTimeout(() => {
        timeoutId = null;
        update();
      }, 48);
    };

    scrollElement.addEventListener("scroll", scheduleUpdate, { passive: true });
    return () => {
      scrollElement.removeEventListener("scroll", scheduleUpdate);
      if (timeoutId !== null) {
        window.clearTimeout(timeoutId);
      }
    };
  }, [scrollRef, syncActiveUserMessage]);

  const scrollToAdjacentMessage = React.useCallback(
    (nextDirection: "older" | "newer") => {
      const targetId = getAdjacentUserMessageId(nextDirection);
      if (!targetId) return;
      setDirection(nextDirection);
      jumpToMessage(targetId, "auto");
    },
    [getAdjacentUserMessageId, jumpToMessage],
  );

  const handleScrollToBottom = React.useCallback(() => {
    cancelJumpLock();
    setDirection("newer");
    scrollToBottom();
  }, [cancelJumpLock, scrollToBottom]);

  const showPreviousButton = direction === "older" && previousId !== null;
  const showBottomButton = direction === "newer" && !isAtBottom;
  const showNextButton = showBottomButton && nextId !== null;

  if (!showPreviousButton && !showBottomButton) {
    return null;
  }

  return (
    <>
      {showPreviousButton ? (
        <div className="pointer-events-none absolute left-1/2 top-3 z-30 -translate-x-1/2">
          <Button
            type="button"
            variant="outline"
            size="icon"
            aria-label={t("quick_jump.previous_user_message")}
            title={t("quick_jump.previous_user_message")}
            className="pointer-events-auto size-9 rounded-full bg-background/90 shadow-md backdrop-blur dark:bg-background/90 dark:hover:bg-muted"
            onClick={() => scrollToAdjacentMessage("older")}
          >
            <ArrowUp className="size-4" />
          </Button>
        </div>
      ) : null}

      {showBottomButton ? (
        <div className="pointer-events-none absolute bottom-4 left-4 z-30">
          <Button
            type="button"
            variant="outline"
            size="icon"
            aria-label={t("quick_jump.scroll_to_bottom")}
            title={t("quick_jump.scroll_to_bottom")}
            className="pointer-events-auto size-9 rounded-full bg-background/90 text-foreground shadow-md backdrop-blur dark:bg-background/90 dark:hover:bg-muted"
            onClick={handleScrollToBottom}
          >
            <ArrowDownToLine className="size-4" />
          </Button>
        </div>
      ) : null}

      {showNextButton ? (
        <div className="pointer-events-none absolute bottom-4 right-4 z-30">
          <Button
            type="button"
            variant="outline"
            size="icon"
            aria-label={t("quick_jump.next_user_message")}
            title={t("quick_jump.next_user_message")}
            className="pointer-events-auto size-9 rounded-full bg-background/90 shadow-md backdrop-blur dark:bg-background/90 dark:hover:bg-muted"
            onClick={() => scrollToAdjacentMessage("newer")}
          >
            <ArrowDown className="size-4" />
          </Button>
        </div>
      ) : null}
    </>
  );
}

export function ConversationNavigation({
  items,
  showQuickJump = true,
}: ConversationNavigationProps) {
  const controller = useConversationAnchorOffsets(items);

  return (
    <>
      {showQuickJump ? <ConversationQuickJump items={items} controller={controller} /> : null}
      <ConversationScrollControls items={items} controller={controller} />
    </>
  );
}
