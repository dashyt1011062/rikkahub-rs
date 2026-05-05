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

interface ConversationQuickJumpProps {
  items: ConversationQuickJumpItem[];
}

interface ConversationScrollControlsProps {
  items: ConversationQuickJumpItem[];
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

function resolveScrollTargets(items: ConversationQuickJumpItem[], scrollElement: HTMLElement) {
  const viewportTop = scrollElement.scrollTop + SCROLL_ANCHOR_OFFSET;
  const userAnchors = items
    .filter(isUserMessage)
    .map((item) => ({ id: item.id, top: getAnchorOffset(scrollElement, item.id) }))
    .filter((item): item is { id: string; top: number } => item.top !== null);

  let previousId: string | null = null;
  let nextId: string | null = null;

  for (const item of userAnchors) {
    if (item.top < viewportTop - MESSAGE_TOLERANCE) {
      previousId = item.id;
      continue;
    }

    if (item.top > viewportTop + MESSAGE_TOLERANCE) {
      nextId = item.id;
      break;
    }
  }

  return { previousId, nextId };
}

export function ConversationQuickJump({ items }: ConversationQuickJumpProps) {
  const { t } = useTranslation();
  const { scrollRef } = useStickToBottomContext();
  const [activeMessageId, setActiveMessageId] = React.useState<string | null>(null);
  const canQuickJump = items.length > 1 && items.length <= 128;
  const activeIndex = React.useMemo(() => {
    if (!activeMessageId) return 0;
    const index = items.findIndex((item) => item.id === activeMessageId);
    return index >= 0 ? index + 1 : 0;
  }, [activeMessageId, items]);

  const resolveActiveMessageId = React.useCallback(() => {
    const scrollElement = scrollRef.current;
    if (!scrollElement || items.length === 0) {
      return items[items.length - 1]?.id ?? null;
    }

    const scrollTopLine = scrollElement.getBoundingClientRect().top + 24;
    let activeId = items[0]?.id ?? null;

    for (const item of items) {
      const anchor = document.getElementById(getConversationMessageAnchorId(item.id));
      if (!anchor) continue;

      if (anchor.getBoundingClientRect().top <= scrollTopLine) {
        activeId = item.id;
      } else {
        break;
      }
    }

    return activeId;
  }, [items, scrollRef]);

  React.useEffect(() => {
    if (!canQuickJump) {
      setActiveMessageId(null);
      return;
    }

    const scrollElement = scrollRef.current;
    if (!scrollElement) {
      setActiveMessageId(resolveActiveMessageId());
      return;
    }

    let frameId: number | null = null;
    const updateActive = () => {
      frameId = null;
      const nextActiveId = resolveActiveMessageId();
      setActiveMessageId((prev) => (prev === nextActiveId ? prev : nextActiveId));
    };
    const scheduleUpdate = () => {
      if (frameId !== null) return;
      frameId = window.requestAnimationFrame(updateActive);
    };

    updateActive();
    scrollElement.addEventListener("scroll", scheduleUpdate, { passive: true });
    window.addEventListener("resize", scheduleUpdate);

    return () => {
      scrollElement.removeEventListener("scroll", scheduleUpdate);
      window.removeEventListener("resize", scheduleUpdate);
      if (frameId !== null) {
        window.cancelAnimationFrame(frameId);
      }
    };
  }, [canQuickJump, resolveActiveMessageId, scrollRef]);

  const handleQuickJump = React.useCallback((messageId: string) => {
    const anchor = document.getElementById(getConversationMessageAnchorId(messageId));
    if (!anchor) return;

    setActiveMessageId(messageId);
    anchor.scrollIntoView({ behavior: "smooth", block: "start" });
  }, []);

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

export function ConversationScrollControls({ items }: ConversationScrollControlsProps) {
  const { t } = useTranslation();
  const { isAtBottom, scrollRef, scrollToBottom } = useStickToBottomContext();
  const [direction, setDirection] = React.useState<"older" | "newer" | null>(null);
  const [targets, setTargets] = React.useState<{ previousId: string | null; nextId: string | null }>({
    previousId: null,
    nextId: null,
  });
  const lastScrollTopRef = React.useRef(0);

  const resolveTargets = React.useCallback(() => {
    const scrollElement = scrollRef.current;
    return scrollElement ? resolveScrollTargets(items, scrollElement) : { previousId: null, nextId: null };
  }, [items, scrollRef]);

  React.useEffect(() => {
    const scrollElement = scrollRef.current;
    if (!scrollElement) {
      setTargets({ previousId: null, nextId: null });
      return;
    }

    let frameId: number | null = null;
    lastScrollTopRef.current = scrollElement.scrollTop;
    setTargets(resolveTargets());

    const update = () => {
      frameId = null;
      const currentScrollTop = scrollElement.scrollTop;
      const delta = currentScrollTop - lastScrollTopRef.current;
      if (Math.abs(delta) > 2) {
        setDirection(delta > 0 ? "newer" : "older");
      }
      lastScrollTopRef.current = currentScrollTop;
      setTargets(resolveTargets());
    };

    const scheduleUpdate = () => {
      if (frameId === null) {
        frameId = window.requestAnimationFrame(update);
      }
    };

    scrollElement.addEventListener("scroll", scheduleUpdate, { passive: true });
    window.addEventListener("resize", scheduleUpdate);

    return () => {
      scrollElement.removeEventListener("scroll", scheduleUpdate);
      window.removeEventListener("resize", scheduleUpdate);
      if (frameId !== null) {
        window.cancelAnimationFrame(frameId);
      }
    };
  }, [resolveTargets, scrollRef]);

  const scrollToMessage = React.useCallback((messageId: string, nextDirection: "older" | "newer") => {
    const anchor = document.getElementById(getConversationMessageAnchorId(messageId));
    if (!anchor) return;

    setDirection(nextDirection);
    anchor.scrollIntoView({ behavior: "smooth", block: "start" });
  }, []);

  const handleScrollToBottom = React.useCallback(() => {
    setDirection("newer");
    scrollToBottom();
  }, [scrollToBottom]);

  const showPreviousButton = direction === "older" && targets.previousId !== null;
  const showBottomButton = direction === "newer" && !isAtBottom;
  const showNextButton = showBottomButton && targets.nextId !== null;

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
            onClick={() => {
              if (targets.previousId) {
                scrollToMessage(targets.previousId, "older");
              }
            }}
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
            onClick={() => {
              if (targets.nextId) {
                scrollToMessage(targets.nextId, "newer");
              }
            }}
          >
            <ArrowDown className="size-4" />
          </Button>
        </div>
      ) : null}
    </>
  );
}
