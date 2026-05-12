import * as React from "react";
import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";

import {
  ArrowDown,
  ArrowUp,
  ChevronLeft,
  ChevronRight,
  Clock3,
  Copy,
  Ellipsis,
  GitFork,
  Pencil,
  Quote,
  RefreshCw,
  Trash2,
  Zap,
} from "lucide-react";

import { useSettingsStore } from "~/stores";
import type {
  AssistantProfile,
  MessageDto,
  MessageNodeDto,
  ProviderModel,
  TokenUsage,
  UIMessagePart,
} from "~/types";

import { cn } from "~/lib/utils";
import { copyTextToClipboard } from "~/lib/clipboard";
import { Button } from "~/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "~/components/ui/dropdown-menu";
import { ChatMessageAnnotationsRow } from "./chat-message-annotations";
import { ChatMessageAvatarRow } from "./chat-message-avatar-row";
import { MessageParts } from "./message-part";
import { useConfirm } from "~/components/confirm-dialog-provider";

interface ChatMessageProps {
  node: MessageNodeDto;
  message: MessageDto;
  previousRole?: string | null;
  loading?: boolean;
  isLastMessage?: boolean;
  assistant?: AssistantProfile | null;
  model?: ProviderModel | null;
  deleteMessageIds?: string[];
  regenerateMessageId?: string;
  forkMessageId?: string;
  disableEdit?: boolean;
  disableBranchSwitch?: boolean;
  onEdit?: (message: MessageDto) => void | Promise<void>;
  onRegenerate?: (messageId: string) => void | Promise<void>;
  onSelectBranch?: (nodeId: string, selectIndex: number) => void | Promise<void>;
  onDelete?: (messageIds: string | string[]) => void | Promise<void>;
  onQuote?: (message: MessageDto) => void | Promise<void>;
  onFork?: (messageId: string) => void | Promise<void>;
  onToolApproval?: (toolCallId: string, approved: boolean, reason: string) => void | Promise<void>;
}

function hasRenderablePart(part: UIMessagePart): boolean {
  switch (part.type) {
    case "text":
      return part.text.trim().length > 0;
    case "image":
    case "video":
    case "audio":
      return part.url.trim().length > 0;
    case "document":
      return part.url.trim().length > 0 || part.fileName.trim().length > 0;
    case "reasoning":
      return part.reasoning.trim().length > 0;
    case "tool":
      return true;
  }
}

function buildCopyText(parts: UIMessagePart[]): string {
  return parts
    .filter((part): part is Extract<UIMessagePart, { type: "text" }> => part.type === "text")
    .map((part) => part.text)
    .filter((value) => value.trim().length > 0)
    .join("\n\n")
    .trim();
}

function hasEditableContent(parts: UIMessagePart[]): boolean {
  return parts.some(
    (part) =>
      part.type === "text" ||
      part.type === "image" ||
      part.type === "video" ||
      part.type === "audio" ||
      part.type === "document",
  );
}

function formatNumber(value: number): string {
  return new Intl.NumberFormat().format(value);
}

function getDurationMs(createdAt: string, finishedAt?: string | null): number | null {
  const start = Date.parse(createdAt);
  if (Number.isNaN(start)) return null;

  const end = finishedAt ? Date.parse(finishedAt) : Date.now();
  if (Number.isNaN(end) || end <= start) return null;

  return end - start;
}

function getNerdStats(
  usage: TokenUsage,
  createdAt: string,
  finishedAt: string | null | undefined,
  t: TFunction,
) {
  const stats: Array<{ key: string; icon: React.ReactNode; label: string }> = [];

  stats.push({
    key: "prompt",
    icon: <ArrowUp className="size-3" />,
    label:
      usage.cachedTokens > 0
        ? t("chat_message.prompt_tokens_with_cache", {
            promptTokens: formatNumber(usage.promptTokens),
            cachedTokens: formatNumber(usage.cachedTokens),
          })
        : t("chat_message.prompt_tokens", {
            promptTokens: formatNumber(usage.promptTokens),
          }),
  });

  stats.push({
    key: "completion",
    icon: <ArrowDown className="size-3" />,
    label: t("chat_message.completion_tokens", {
      completionTokens: formatNumber(usage.completionTokens),
    }),
  });

  const durationMs = getDurationMs(createdAt, finishedAt);
  if (durationMs && usage.completionTokens > 0) {
    const durationSeconds = durationMs / 1000;
    const tps = usage.completionTokens / durationSeconds;

    stats.push({
      key: "speed",
      icon: <Zap className="size-3" />,
      label: t("chat_message.tokens_per_second", {
        value: tps.toFixed(1),
      }),
    });

    stats.push({
      key: "duration",
      icon: <Clock3 className="size-3" />,
      label: t("chat_message.duration_seconds", {
        value: durationSeconds.toFixed(1),
      }),
    });
  }

  return stats;
}

const ChatMessageActionsRow = React.memo(({
  node,
  message,
  loading,
  alignRight,
  deleteMessageIds,
  regenerateMessageId,
  forkMessageId,
  disableEdit,
  disableBranchSwitch,
  onEdit,
  onRegenerate,
  onSelectBranch,
  onDelete,
  onQuote,
  onFork,
}: {
  node: MessageNodeDto;
  message: MessageDto;
  loading: boolean;
  alignRight: boolean;
  deleteMessageIds?: string[];
  regenerateMessageId?: string;
  forkMessageId?: string;
  disableEdit?: boolean;
  disableBranchSwitch?: boolean;
  onEdit?: (message: MessageDto) => void | Promise<void>;
  onRegenerate?: (messageId: string) => void | Promise<void>;
  onSelectBranch?: (nodeId: string, selectIndex: number) => void | Promise<void>;
  onDelete?: (messageIds: string | string[]) => void | Promise<void>;
  onQuote?: (message: MessageDto) => void | Promise<void>;
  onFork?: (messageId: string) => void | Promise<void>;
}) => {
  const { t } = useTranslation("message");
  const confirm = useConfirm();
  const [regenerating, setRegenerating] = React.useState(false);
  const [switchingBranch, setSwitchingBranch] = React.useState(false);
  const [deleting, setDeleting] = React.useState(false);
  const [forking, setForking] = React.useState(false);

  const handleCopy = React.useCallback(async () => {
    const text = buildCopyText(message.parts);
    if (!text) return;
    await copyTextToClipboard(text);
  }, [message.parts]);

  const handleRegenerate = React.useCallback(async () => {
    if (!onRegenerate) return;

    const confirmed = await confirm({
      title: t("chat_message.regenerate"),
      description:
        message.role === "USER"
          ? t("chat_message.regenerate_from_user_confirm")
          : t("chat_message.regenerate_confirm"),
      confirmText: t("chat_message.regenerate"),
      cancelText: "Cancel",
    });
    if (!confirmed) return;

    setRegenerating(true);
    try {
      await onRegenerate(regenerateMessageId ?? message.id);
    } finally {
      setRegenerating(false);
    }
  }, [confirm, message.id, message.role, onRegenerate, regenerateMessageId, t]);

  const handleSwitchBranch = React.useCallback(
    async (selectIndex: number) => {
      if (!onSelectBranch) return;
      if (selectIndex < 0 || selectIndex > node.messages.length - 1) return;
      if (selectIndex === node.selectIndex) return;

      setSwitchingBranch(true);
      try {
        await onSelectBranch(node.id, selectIndex);
      } finally {
        setSwitchingBranch(false);
      }
    },
    [node.id, node.messages.length, node.selectIndex, onSelectBranch],
  );

  const handleDelete = React.useCallback(async () => {
    if (!onDelete) return;

    const confirmed = await confirm({
      title: t("chat_message.delete"),
      description: t("chat_message.delete_confirm"),
      confirmText: t("chat_message.delete"),
      cancelText: "Cancel",
      destructive: true,
    });
    if (!confirmed) return;

    setDeleting(true);
    try {
      await onDelete(deleteMessageIds && deleteMessageIds.length > 0 ? deleteMessageIds : message.id);
    } finally {
      setDeleting(false);
    }
  }, [confirm, deleteMessageIds, message.id, onDelete, t]);

  const handleFork = React.useCallback(async () => {
    if (!onFork) return;

    setForking(true);
    try {
      await onFork(forkMessageId ?? message.id);
    } finally {
      setForking(false);
    }
  }, [forkMessageId, message.id, onFork]);

  const canSwitchBranch =
    Boolean(onSelectBranch) && node.messages.length > 1 && disableBranchSwitch !== true;
  const canEdit =
    Boolean(onEdit) &&
    disableEdit !== true &&
    (message.role === "USER" || message.role === "ASSISTANT") &&
    hasEditableContent(message.parts);
  const actionDisabled = loading || switchingBranch || regenerating || deleting || forking;

  return (
    <div
      className={cn(
        "flex w-full items-center gap-1 px-1",
        alignRight ? "justify-end" : "justify-start",
      )}
    >
      <Button
        aria-label={t("chat_message.copy_message")}
        disabled={actionDisabled}
        onClick={() => {
          void handleCopy();
        }}
        size="icon-xs"
        title={t("chat_message.copy")}
        type="button"
        variant="ghost"
      >
        <Copy className="size-3.5" />
      </Button>

      {onQuote && (
        <Button
          aria-label={t("chat_message.quote_message")}
          disabled={actionDisabled}
          onClick={() => {
            void onQuote(message);
          }}
          size="icon-xs"
          title={t("chat_message.quote")}
          type="button"
          variant="ghost"
        >
          <Quote className="size-3.5" />
        </Button>
      )}

      {canEdit && (
        <Button
          aria-label={t("chat_message.edit_message")}
          disabled={actionDisabled}
          onClick={() => {
            void onEdit?.(message);
          }}
          size="icon-xs"
          title={t("chat_message.edit")}
          type="button"
          variant="ghost"
        >
          <Pencil className="size-3.5" />
        </Button>
      )}

      {onRegenerate && (
        <Button
          aria-label={t("chat_message.regenerate")}
          disabled={actionDisabled}
          onClick={() => {
            void handleRegenerate();
          }}
          size="icon-xs"
          title={t("chat_message.regenerate")}
          type="button"
          variant="ghost"
        >
          <RefreshCw className={cn("size-3.5", regenerating && "animate-spin")} />
        </Button>
      )}

      {canSwitchBranch && (
        <>
          <Button
            aria-label={t("chat_message.previous_branch")}
            disabled={actionDisabled || node.selectIndex <= 0}
            onClick={() => {
              void handleSwitchBranch(node.selectIndex - 1);
            }}
            size="icon-xs"
            title={t("chat_message.previous_branch")}
            type="button"
            variant="ghost"
          >
            <ChevronLeft className="size-3.5" />
          </Button>
          <span className="text-[11px] text-muted-foreground">
            {node.selectIndex + 1}/{node.messages.length}
          </span>
          <Button
            aria-label={t("chat_message.next_branch")}
            disabled={actionDisabled || node.selectIndex >= node.messages.length - 1}
            onClick={() => {
              void handleSwitchBranch(node.selectIndex + 1);
            }}
            size="icon-xs"
            title={t("chat_message.next_branch")}
            type="button"
            variant="ghost"
          >
            <ChevronRight className="size-3.5" />
          </Button>
        </>
      )}

      {onDelete && (
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              aria-label={t("chat_message.more_actions")}
              disabled={actionDisabled}
              size="icon-xs"
              title={t("chat_message.more_actions")}
              type="button"
              variant="ghost"
            >
              <Ellipsis className="size-3.5" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align={alignRight ? "end" : "start"}>
            {onFork && (
              <DropdownMenuItem
                disabled={actionDisabled}
                onSelect={() => {
                  void handleFork();
                }}
              >
                <GitFork className="size-3.5" />
                {t("chat_message.create_fork")}
              </DropdownMenuItem>
            )}
            <DropdownMenuItem
              variant="destructive"
              disabled={actionDisabled}
              onSelect={() => {
                void handleDelete();
              }}
            >
              <Trash2 className="size-3.5" />
              {t("chat_message.delete")}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      )}
    </div>
  );
});

const ChatMessageNerdLineRow = React.memo(({
  message,
  alignRight,
}: {
  message: MessageDto;
  alignRight: boolean;
}) => {
  const { t } = useTranslation("message");
  const displaySetting = useSettingsStore((state) => state.settings?.displaySetting);

  if (!displaySetting?.showTokenUsage || !message.usage) {
    return null;
  }

  const stats = getNerdStats(message.usage, message.createdAt, message.finishedAt, t);
  if (stats.length === 0) return null;

  return (
    <div
      className={cn(
        "flex w-full flex-wrap items-center gap-x-3 gap-y-1 px-1 text-[11px] text-muted-foreground/50",
        alignRight ? "justify-end" : "justify-start",
      )}
    >
      {stats.map((item) => (
        <div key={item.key} className="inline-flex items-center gap-1">
          {item.icon}
          <span>{item.label}</span>
        </div>
      ))}
    </div>
  );
});

export const ChatMessage = React.memo(({
  node,
  message,
  previousRole,
  loading = false,
  isLastMessage = false,
  assistant,
  model,
  deleteMessageIds,
  regenerateMessageId,
  forkMessageId,
  disableEdit,
  disableBranchSwitch,
  onEdit,
  onRegenerate,
  onSelectBranch,
  onDelete,
  onQuote,
  onFork,
  onToolApproval,
}: ChatMessageProps) => {
  const isUser = message.role === "USER";
  const hasMessageContent = message.parts.some(hasRenderablePart);
  const showActions = isLastMessage ? !loading : hasMessageContent;

  return (
    <div className={cn("flex flex-col gap-4", isUser ? "items-end" : "items-start")}>
      <div className="flex w-full flex-col gap-2">
        <ChatMessageAvatarRow
          message={message}
          previousRole={previousRole}
          hasMessageContent={hasMessageContent}
          loading={loading}
          assistant={assistant}
          model={model}
        />

        <div className={cn("flex w-full", isUser ? "justify-end" : "justify-start")}>
          <div
            className={cn(
              "flex flex-col gap-2 text-sm",
              isUser ? "max-w-[85%] rounded-lg bg-muted px-4 py-3" : "w-full",
            )}
          >
            <MessageParts parts={message.parts} loading={loading} onToolApproval={onToolApproval} />
          </div>
        </div>
      </div>

      <ChatMessageAnnotationsRow annotations={message.annotations} alignRight={isUser} />

      <ChatMessageNerdLineRow message={message} alignRight={isUser} />

      {showActions && (
        <ChatMessageActionsRow
          node={node}
          message={message}
          loading={loading}
          alignRight={isUser}
          deleteMessageIds={deleteMessageIds}
          regenerateMessageId={regenerateMessageId}
          forkMessageId={forkMessageId}
          disableEdit={disableEdit}
          disableBranchSwitch={disableBranchSwitch}
          onEdit={onEdit}
          onRegenerate={onRegenerate}
          onSelectBranch={onSelectBranch}
          onDelete={onDelete}
          onQuote={onQuote}
          onFork={onFork}
        />
      )}
    </div>
  );
});
