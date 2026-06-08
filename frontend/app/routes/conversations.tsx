import * as React from "react";

import { useNavigate, useParams } from "react-router";

import {
  ConversationScrollControls,
  ConversationQuickJump,
  getConversationMessageAnchorId,
} from "~/components/conversation-quick-jump";
import { ConversationSidebar } from "~/components/conversation-sidebar";
import { useConfirm } from "~/components/confirm-dialog-provider";
import {
  Conversation,
  ConversationContent,
  ConversationEmptyState,
} from "~/components/extended/conversation";
import { ChatInput, type ChatInputSendOptions } from "~/components/input/chat-input";
import { ChatMessage } from "~/components/message/chat-message";
import { Drawer, DrawerContent } from "~/components/ui/drawer";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "~/components/ui/resizable";
import { TypingIndicator } from "~/components/ui/typing-indicator";
import { SidebarInset, SidebarProvider, SidebarTrigger } from "~/components/ui/sidebar";
import { useIsMobile } from "~/hooks/use-mobile";
import { toConversationSummaryUpdate, useConversationList } from "~/hooks/use-conversation-list";
import { useCurrentAssistant } from "~/hooks/use-current-assistant";
import { getAssistantDisplayName } from "~/lib/display";
import { cn } from "~/lib/utils";
import api, { sse } from "~/services/api";
import { useChatInputStore } from "~/stores";
import { WorkbenchHost } from "~/components/workbench/workbench-host";
import {
  useWorkbench,
  useWorkbenchController,
  WorkbenchProvider,
} from "~/components/workbench/workbench-context";
import {
  type ConversationDto,
  type MessageNodeDto,
  type MessageDto,
  type ConversationNodeUpdateEventDto,
  type ConversationErrorEventDto,
  type ConversationSnapshotEventDto,
  type ConversationListDto,
  type PagedResult,
  type ProviderModel,
  type Settings,
  type TokenUsage,
  type UIMessagePart,
} from "~/types";
import { MessageSquare } from "lucide-react";
import type { PanelImperativeHandle } from "react-resizable-panels";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { v4 as uuidv4 } from "uuid";
import i18n from "~/i18n";

type ConversationStreamEvent =
  | ConversationSnapshotEventDto
  | ConversationNodeUpdateEventDto
  | ConversationErrorEventDto;

interface ConversationDetailRefreshOptions {
  showLoading?: boolean;
  clearOnError?: boolean;
  reportError?: boolean;
}

type ConversationDetailRefresher = (
  options?: ConversationDetailRefreshOptions,
) => Promise<ConversationDto | null>;

interface SelectedNodeMessage {
  node: MessageNodeDto;
  message: MessageNodeDto["messages"][number];
}

interface TimelineMessageItem {
  id: string;
  node: MessageNodeDto;
  message: MessageDto;
  deleteMessageIds?: string[];
  regenerateMessageId?: string;
  forkMessageId?: string;
  disableEdit?: boolean;
  disableBranchSwitch?: boolean;
}

interface TimelineMessageCacheEntry {
  item: TimelineMessageItem;
  source: SelectedNodeMessage[];
}

type ConversationSummaryUpdater = (update: ReturnType<typeof toConversationSummaryUpdate>) => void;

const EDIT_DRAFT_ATTACHMENT_MARK = "__from_message_attachment";
const EDIT_DRAFT_SOURCE_INDEX = "__from_message_source_index";

interface EditDraft {
  text: string;
  attachments: UIMessagePart[];
  sourceParts: UIMessagePart[];
  textPartIndex: number | null;
}

interface EditingSession {
  messageId: string;
  sourceParts: UIMessagePart[];
  textPartIndex: number | null;
}

function deepCloneSettings(settings: Settings): Settings {
  return JSON.parse(JSON.stringify(settings)) as Settings;
}

function createDefaultAssistant(index: number, fallbackModelId: string) {
  return {
    id: uuidv4(),
    name: `Assistant ${index + 1}`,
    chatModelId: fallbackModelId,
    tags: [],
    systemPrompt: "",
    messageTemplate: "{{ message }}",
    quickMessages: [],
    customHeaders: [],
    customBodies: [],
    mcpServers: [],
    modeInjectionIds: [],
    lorebookIds: [],
    localTools: ["time_info"],
    streamOutput: true,
  };
}

async function listAssistantConversations(assistantId: string): Promise<ConversationListDto[]> {
  await api.post<{ status: string }>("settings/assistant", { assistantId });
  const result: ConversationListDto[] = [];

  while (true) {
    const page = await api.get<PagedResult<ConversationListDto>>("conversations/paged", {
      searchParams: { offset: 0, limit: 100 },
    });
    if (page.items.length === 0) break;
    result.push(...page.items);

    for (const conversation of page.items) {
      await api.delete<Record<string, never>>(`conversations/${conversation.id}`, {
        parseJson: (raw) => (raw ? JSON.parse(raw) : {}),
      });
    }
  }

  return result;
}
function createHomeDraftId() {
  return `home-${uuidv4()}`;
}

function truncatePreviewText(value: string, maxLength = 48): string {
  if (value.length <= maxLength) {
    return value;
  }

  return `${value.slice(0, maxLength)}...`;
}

function getQuickJumpPreview(
  message: MessageDto,
  t: (key: string, options?: Record<string, unknown>) => string,
): string {
  const textPreview = message.parts
    .filter((part): part is Extract<UIMessagePart, { type: "text" }> => part.type === "text")
    .map((part) => part.text.trim())
    .find((text) => text.length > 0);

  if (textPreview) {
    return truncatePreviewText(textPreview.replace(/\s+/g, " "));
  }

  const fallbackPart = message.parts.find(Boolean);
  if (!fallbackPart) return t("conversations.preview.empty_message");

  switch (fallbackPart.type) {
    case "image":
      return t("conversations.preview.image");
    case "video":
      return t("conversations.preview.video");
    case "audio":
      return t("conversations.preview.audio");
    case "document":
      return fallbackPart.fileName.trim().length > 0
        ? t("conversations.preview.document_with_name", {
            name: truncatePreviewText(fallbackPart.fileName.trim(), 32),
          })
        : t("conversations.preview.document");
    case "reasoning":
      return fallbackPart.reasoning.trim().length > 0
        ? truncatePreviewText(fallbackPart.reasoning.trim().replace(/\s+/g, " "))
        : t("conversations.preview.thinking");
    case "tool":
      return fallbackPart.toolName.trim().length > 0
        ? t("conversations.preview.tool_with_name", {
            name: truncatePreviewText(fallbackPart.toolName.trim(), 32),
          })
        : t("conversations.preview.tool_call");
    case "text":
      return t("conversations.preview.empty_message");
  }
}

function isAttachmentPart(
  part: UIMessagePart,
): part is Extract<UIMessagePart, { type: "image" | "video" | "audio" | "document" }> {
  return (
    part.type === "image" ||
    part.type === "video" ||
    part.type === "audio" ||
    part.type === "document"
  );
}

function getLastTextPartIndex(parts: UIMessagePart[]): number | null {
  for (let index = parts.length - 1; index >= 0; index -= 1) {
    if (parts[index]?.type === "text") {
      return index;
    }
  }

  return null;
}

function getDraftSourceIndex(part: UIMessagePart): number | null {
  const value = part.metadata?.[EDIT_DRAFT_SOURCE_INDEX];
  return typeof value === "number" ? value : null;
}

function toEditDraft(message: MessageDto): EditDraft | null {
  const textPartIndex = getLastTextPartIndex(message.parts);
  const text =
    textPartIndex !== null && message.parts[textPartIndex]?.type === "text"
      ? message.parts[textPartIndex].text
      : "";

  const attachments = message.parts.flatMap((part, index) => {
    if (!isAttachmentPart(part)) return [];

    return [
      {
        ...part,
        metadata: {
          ...(part.metadata ?? {}),
          [EDIT_DRAFT_ATTACHMENT_MARK]: true,
          [EDIT_DRAFT_SOURCE_INDEX]: index,
        },
      },
    ];
  });

  if (text.trim().length === 0 && attachments.length === 0) {
    return null;
  }

  return {
    text,
    attachments,
    sourceParts: message.parts,
    textPartIndex,
  };
}

function shouldDeleteAttachmentFileOnRemove(part: UIMessagePart): boolean {
  if (!part.metadata) return true;

  return part.metadata[EDIT_DRAFT_ATTACHMENT_MARK] !== true;
}

function stripEditDraftMetadata(parts: UIMessagePart[]): UIMessagePart[] {
  return parts.map((part) => {
    if (!part.metadata) {
      return part;
    }

    const hasEditMark =
      EDIT_DRAFT_ATTACHMENT_MARK in part.metadata || EDIT_DRAFT_SOURCE_INDEX in part.metadata;
    if (!hasEditMark) {
      return part;
    }

    const nextMetadata = { ...part.metadata };
    delete nextMetadata[EDIT_DRAFT_ATTACHMENT_MARK];
    delete nextMetadata[EDIT_DRAFT_SOURCE_INDEX];

    return {
      ...part,
      metadata: Object.keys(nextMetadata).length > 0 ? nextMetadata : undefined,
    };
  });
}

function buildEditedParts(session: EditingSession, draftParts: UIMessagePart[]): UIMessagePart[] {
  const textPart = draftParts.find(
    (part): part is Extract<UIMessagePart, { type: "text" }> => part.type === "text",
  );
  const editedText = textPart?.text ?? "";

  const retainedAttachmentIndexes = new Set<number>();
  const appendedAttachments: UIMessagePart[] = [];

  draftParts.forEach((part) => {
    if (!isAttachmentPart(part)) return;

    if (part.metadata?.[EDIT_DRAFT_ATTACHMENT_MARK] === true) {
      const sourceIndex = getDraftSourceIndex(part);
      if (sourceIndex !== null) {
        retainedAttachmentIndexes.add(sourceIndex);
      }
      return;
    }

    appendedAttachments.push(part);
  });

  const preservedParts: UIMessagePart[] = [];

  session.sourceParts.forEach((part, index) => {
    if (session.textPartIndex !== null && index === session.textPartIndex && part.type === "text") {
      preservedParts.push({ ...part, text: editedText });
      return;
    }

    if (isAttachmentPart(part)) {
      if (retainedAttachmentIndexes.has(index)) {
        preservedParts.push(part);
      }
      return;
    }

    preservedParts.push(part);
  });

  if (session.textPartIndex === null && textPart && textPart.text.trim().length > 0) {
    return [textPart, ...preservedParts, ...appendedAttachments];
  }

  return [...preservedParts, ...appendedAttachments];
}

function getMessageRoleLabel(
  role: string,
  t: (key: string, options?: Record<string, unknown>) => string,
): string {
  switch (role.toUpperCase()) {
    case "USER":
      return t("conversations.quote.roles.user");
    case "ASSISTANT":
      return t("conversations.quote.roles.assistant");
    case "SYSTEM":
      return t("conversations.quote.roles.system");
    case "TOOL":
      return t("conversations.quote.roles.tool");
    default:
      return role;
  }
}

function getImageParts(parts: UIMessagePart[]): Array<Extract<UIMessagePart, { type: "image" }>> {
  return parts.filter((part): part is Extract<UIMessagePart, { type: "image" }> => part.type === "image");
}

function getImagePartIdentity(part: Extract<UIMessagePart, { type: "image" }>): string {
  const fileId = part.metadata?.fileId;
  if ((typeof fileId === "number" && Number.isFinite(fileId)) || typeof fileId === "string") {
    return `file:${String(fileId)}`;
  }

  return `url:${part.url.trim()}`;
}

function mergeQuotedAndCurrentParts(
  quotedMessage: MessageDto,
  draftParts: UIMessagePart[],
): UIMessagePart[] {
  const seen = new Set<string>();
  const mergedImages: UIMessagePart[] = [];
  const pushImage = (part: Extract<UIMessagePart, { type: "image" }>) => {
    const identity = getImagePartIdentity(part);
    if (seen.has(identity)) return;
    seen.add(identity);
    mergedImages.push(part);
  };

  getImageParts(quotedMessage.parts).forEach(pushImage);
  getImageParts(draftParts).forEach(pushImage);

  const textParts = draftParts.filter(
    (part): part is Extract<UIMessagePart, { type: "text" }> => part.type === "text",
  );
  return [...textParts, ...mergedImages];
}

function partToQuoteText(
  part: UIMessagePart,
  t: (key: string, options?: Record<string, unknown>) => string,
): string {
  switch (part.type) {
    case "text":
      return part.text.trim();
    case "image":
      return t("conversations.quote.parts.image");
    case "video":
      return t("conversations.quote.parts.video");
    case "audio":
      return t("conversations.quote.parts.audio");
    case "document":
      return part.fileName.trim().length > 0
        ? t("conversations.quote.parts.document_with_name", { name: part.fileName.trim() })
        : t("conversations.quote.parts.document");
    case "reasoning":
      return part.reasoning.trim().length > 0
        ? `${t("conversations.quote.parts.reasoning")}\n${part.reasoning.trim()}`
        : t("conversations.quote.parts.reasoning");
    case "tool":
      return part.toolName.trim().length > 0
        ? t("conversations.quote.parts.tool_with_name", { name: part.toolName.trim() })
        : t("conversations.quote.parts.tool");
  }
}

function buildQuoteContextText(
  quotedMessage: MessageDto,
  t: (key: string, options?: Record<string, unknown>) => string,
): string {
  const header = t("conversations.quote.context_header", {
    role: getMessageRoleLabel(quotedMessage.role, t),
  });
  const body = quotedMessage.parts
    .map((part) => partToQuoteText(part, t))
    .filter((value) => value.trim().length > 0)
    .join("\n");

  return `${header}\n${body || t("conversations.quote.parts.empty")}`;
}

function injectQuoteContextIntoDraftParts(
  quotedMessage: MessageDto,
  draftParts: UIMessagePart[],
  t: (key: string, options?: Record<string, unknown>) => string,
): UIMessagePart[] {
  const quoteText = buildQuoteContextText(quotedMessage, t);
  const textPartIndex = draftParts.findIndex((part) => part.type === "text");

  if (textPartIndex >= 0) {
    return draftParts.map((part, index) => {
      if (index !== textPartIndex || part.type !== "text") {
        return part;
      }

      const nextText = part.text.trim().length > 0 ? `${quoteText}\n\n${part.text}` : quoteText;
      return {
        ...part,
        text: nextText,
      };
    });
  }

  return [{ type: "text", text: quoteText }, ...draftParts];
}

function isImageGenerationModel(model: ProviderModel | null | undefined): boolean {
  if (!model || model.imageGenerationMode !== true) {
    return false;
  }

  return (model.outputModalities ?? []).some((modality) => modality.toUpperCase() === "IMAGE");
}

function sumUsage(usages: Array<TokenUsage | null | undefined>): TokenUsage | null {
  let hasUsage = false;
  let promptTokens = 0;
  let completionTokens = 0;
  let cachedTokens = 0;
  let totalTokens = 0;

  usages.forEach((usage) => {
    if (!usage) return;
    hasUsage = true;
    promptTokens += usage.promptTokens ?? 0;
    completionTokens += usage.completionTokens ?? 0;
    cachedTokens += usage.cachedTokens ?? 0;
    totalTokens += usage.totalTokens ?? 0;
  });

  if (!hasUsage) {
    return null;
  }

  return {
    promptTokens,
    completionTokens,
    cachedTokens,
    totalTokens,
  };
}

function buildGroupedTimelineItem(group: SelectedNodeMessage[]): TimelineMessageItem {
  const first = group[0];
  const last = group[group.length - 1];

  if (group.length === 1) {
    return {
      id: first.message.id,
      node: first.node,
      message: first.message,
    };
  }

  const mergedMessage: MessageDto = {
    ...first.message,
    parts: group.flatMap((item) => item.message.parts),
    annotations: group.flatMap((item) => item.message.annotations ?? []),
    finishedAt: last.message.finishedAt ?? first.message.finishedAt ?? null,
    modelId: last.message.modelId ?? first.message.modelId ?? null,
    usage: sumUsage(group.map((item) => item.message.usage)),
    translation: last.message.translation ?? first.message.translation ?? null,
  };

  return {
    id: first.message.id,
    node: last.node,
    message: mergedMessage,
    deleteMessageIds: group.map((item) => item.message.id),
    regenerateMessageId: first.message.id,
    forkMessageId: last.message.id,
    disableEdit: true,
    disableBranchSwitch: true,
  };
}

function sameSelectedNodeMessageGroup(
  previous: SelectedNodeMessage[],
  next: SelectedNodeMessage[],
): boolean {
  return (
    previous.length === next.length &&
    previous.every((item, index) => item === next[index])
  );
}

function groupTimelineMessages(selectedNodeMessages: SelectedNodeMessage[]): TimelineMessageItem[] {
  const grouped: TimelineMessageItem[] = [];
  let assistantGroup: SelectedNodeMessage[] = [];

  const flushAssistantGroup = () => {
    if (assistantGroup.length === 0) return;
    grouped.push(buildGroupedTimelineItem(assistantGroup));
    assistantGroup = [];
  };

  selectedNodeMessages.forEach((item) => {
    if (item.message.role.toUpperCase() === "ASSISTANT") {
      assistantGroup.push(item);
      return;
    }

    flushAssistantGroup();
    grouped.push(buildGroupedTimelineItem([item]));
  });

  flushAssistantGroup();
  return grouped;
}

function deriveTimelineMessageCache(
  selectedNodeMessages: SelectedNodeMessage[],
  previous: TimelineMessageCacheEntry[],
): TimelineMessageCacheEntry[] {
  const next: TimelineMessageCacheEntry[] = [];
  let assistantGroup: SelectedNodeMessage[] = [];

  const flushAssistantGroup = () => {
    if (assistantGroup.length === 0) return;
    const previousEntry = previous[next.length];
    if (previousEntry && sameSelectedNodeMessageGroup(previousEntry.source, assistantGroup)) {
      next.push(previousEntry);
    } else {
      next.push({
        item: buildGroupedTimelineItem(assistantGroup),
        source: assistantGroup,
      });
    }
    assistantGroup = [];
  };

  selectedNodeMessages.forEach((item) => {
    if (item.message.role.toUpperCase() === "ASSISTANT") {
      assistantGroup.push(item);
      return;
    }

    flushAssistantGroup();
    const previousEntry = previous[next.length];
    const nextSource = [item];
    if (previousEntry && sameSelectedNodeMessageGroup(previousEntry.source, nextSource)) {
      next.push(previousEntry);
    } else {
      next.push({
        item: buildGroupedTimelineItem(nextSource),
        source: nextSource,
      });
    }
  });

  flushAssistantGroup();
  return next;
}

function deriveSelectedNodeMessages(
  nodes: MessageNodeDto[],
  previous: SelectedNodeMessage[],
): SelectedNodeMessage[] {
  let changed = previous.length !== nodes.length;
  const next = nodes.map((node, index) => {
    const message = node.messages[node.selectIndex] ?? node.messages[0];
    const previousItem = previous[index];
    if (previousItem && previousItem.node === node && previousItem.message === message) {
      return previousItem;
    }
    changed = true;
    return {
      node,
      message,
    };
  });

  return changed ? next : previous;
}

function applyNodeUpdate(
  conversation: ConversationDto,
  event: ConversationNodeUpdateEventDto,
): ConversationDto {
  if (conversation.id !== event.conversationId) {
    return conversation;
  }

  const nextNodes = [...conversation.messages];
  const indexById = nextNodes.findIndex((node) => node.id === event.nodeId);
  const targetIndex = indexById >= 0 ? indexById : event.nodeIndex;

  if (targetIndex < 0) {
    return conversation;
  }

  if (targetIndex < nextNodes.length) {
    nextNodes[targetIndex] = event.node;
  } else if (targetIndex === nextNodes.length) {
    nextNodes.push(event.node);
  } else {
    return conversation;
  }

  return {
    ...conversation,
    messages: nextNodes,
    updateAt: event.updateAt,
    isGenerating: event.isGenerating,
  };
}

function applyOptimisticRegenerate(
  conversation: ConversationDto,
  messageId: string,
): ConversationDto {
  const targetIndex = conversation.messages.findIndex((node) =>
    node.messages.some((message) => message.id === messageId),
  );

  if (targetIndex < 0) {
    return conversation;
  }

  const targetMessage = conversation.messages[targetIndex].messages.find(
    (message) => message.id === messageId,
  );
  const keepCount =
    targetMessage?.role.toUpperCase() === "ASSISTANT" ? targetIndex : targetIndex + 1;

  if (keepCount >= conversation.messages.length && conversation.isGenerating) {
    return conversation;
  }

  return {
    ...conversation,
    messages: conversation.messages.slice(0, keepCount),
    isGenerating: true,
    updateAt: conversation.updateAt + 1,
  };
}

function applyOptimisticEditMessage(
  conversation: ConversationDto,
  messageId: string,
  parts: UIMessagePart[],
): ConversationDto {
  const targetIndex = conversation.messages.findIndex((node) =>
    node.messages.some((message) => message.id === messageId),
  );

  if (targetIndex < 0) {
    return conversation;
  }

  const targetNode = conversation.messages[targetIndex];
  const targetMessageIndex = targetNode.messages.findIndex((message) => message.id === messageId);
  const targetMessage = targetNode.messages[targetMessageIndex];

  if (!targetMessage) {
    return conversation;
  }

  const nextMessages = [...targetNode.messages];
  nextMessages[targetMessageIndex] = {
    ...targetMessage,
    parts,
  };

  const nextNode = {
    ...targetNode,
    messages: nextMessages,
    selectIndex: targetMessageIndex,
  };
  const nextNodes = conversation.messages.slice(0, targetIndex + 1);
  nextNodes[targetIndex] = nextNode;

  return {
    ...conversation,
    messages: nextNodes,
    isGenerating:
      targetMessage.role.toUpperCase() === "USER" ? true : conversation.isGenerating,
    updateAt: conversation.updateAt + 1,
  };
}

function useConversationDetail(activeId: string | null, updateSummary: ConversationSummaryUpdater) {
  const { t } = useTranslation("page");
  const [detail, setDetail] = React.useState<ConversationDto | null>(null);
  const [detailLoading, setDetailLoading] = React.useState(false);
  const [detailError, setDetailError] = React.useState<string | null>(null);
  const activeIdRef = React.useRef(activeId);
  const detailRef = React.useRef<ConversationDto | null>(null);
  const selectedNodeMessagesRef = React.useRef<SelectedNodeMessage[]>([]);
  const mountedRef = React.useRef(true);
  const requestVersionRef = React.useRef(0);
  const scheduledRefreshRef = React.useRef<number | null>(null);

  const clearScheduledRefresh = React.useCallback(() => {
    if (scheduledRefreshRef.current === null) return;
    clearTimeout(scheduledRefreshRef.current);
    scheduledRefreshRef.current = null;
  }, []);

  const applyDetail = React.useCallback(
    (nextDetail: ConversationDto) => {
      const currentDetail = detailRef.current;
      if (
        currentDetail &&
        currentDetail.id === nextDetail.id &&
        currentDetail.updateAt > nextDetail.updateAt
      ) {
        return;
      }

      detailRef.current = nextDetail;
      setDetail(nextDetail);
      updateSummary(toConversationSummaryUpdate(nextDetail));
    },
    [updateSummary],
  );

  const resetDetail = React.useCallback(() => {
    clearScheduledRefresh();
    detailRef.current = null;
    setDetail(null);
    setDetailError(null);
    setDetailLoading(false);
  }, [clearScheduledRefresh]);

  const refreshDetail = React.useCallback<ConversationDetailRefresher>(
    async ({ showLoading = false, clearOnError = false, reportError = true } = {}) => {
      const conversationId = activeIdRef.current;
      if (!conversationId) return null;

      const requestVersion = ++requestVersionRef.current;
      if (showLoading) {
        setDetailLoading(true);
        setDetailError(null);
      }

      try {
        const nextDetail = await api.get<ConversationDto>(`conversations/${conversationId}`);
        if (
          !mountedRef.current ||
          activeIdRef.current !== conversationId ||
          requestVersion !== requestVersionRef.current
        ) {
          return null;
        }

        applyDetail(nextDetail);
        setDetailError(null);
        return nextDetail;
      } catch (error) {
        if (
          reportError &&
          mountedRef.current &&
          activeIdRef.current === conversationId &&
          requestVersion === requestVersionRef.current
        ) {
          setDetailError(
            error instanceof Error && error.message
              ? error.message
              : t("conversations.errors.load_detail_failed"),
          );
          if (clearOnError) {
            detailRef.current = null;
            setDetail(null);
          }
        }
        return null;
      } finally {
        if (
          showLoading &&
          mountedRef.current &&
          activeIdRef.current === conversationId &&
          requestVersion <= requestVersionRef.current
        ) {
          setDetailLoading(false);
        }
      }
    },
    [applyDetail, t],
  );

  const scheduleRefreshDetail = React.useCallback(() => {
    if (scheduledRefreshRef.current !== null || typeof window === "undefined") return;
    scheduledRefreshRef.current = window.setTimeout(() => {
      scheduledRefreshRef.current = null;
      void refreshDetail({ reportError: false });
    }, 120);
  }, [refreshDetail]);

  const optimisticRegenerate = React.useCallback(
    (messageId: string) => {
      const currentDetail = detailRef.current;
      if (!currentDetail) return;

      const nextDetail = applyOptimisticRegenerate(currentDetail, messageId);
      if (nextDetail === currentDetail) return;

      detailRef.current = nextDetail;
      setDetail(nextDetail);
      updateSummary(toConversationSummaryUpdate(nextDetail));
    },
    [updateSummary],
  );

  const optimisticEditMessage = React.useCallback(
    (messageId: string, parts: UIMessagePart[]) => {
      const currentDetail = detailRef.current;
      if (!currentDetail) return;

      const nextDetail = applyOptimisticEditMessage(currentDetail, messageId, parts);
      if (nextDetail === currentDetail) return;

      detailRef.current = nextDetail;
      setDetail(nextDetail);
      updateSummary(toConversationSummaryUpdate(nextDetail));
    },
    [updateSummary],
  );

  React.useEffect(() => {
    activeIdRef.current = activeId;
  }, [activeId]);

  React.useEffect(
    () => () => {
      mountedRef.current = false;
      clearScheduledRefresh();
    },
    [clearScheduledRefresh],
  );

  React.useEffect(() => {
    if (!activeId) {
      resetDetail();
      return;
    }

    let disposed = false;
    let reconnectTimer: number | null = null;
    let streamController: AbortController | null = null;

    const clearReconnectTimer = () => {
      if (reconnectTimer === null) return;
      clearTimeout(reconnectTimer);
      reconnectTimer = null;
    };

    const scheduleReconnect = () => {
      if (disposed || reconnectTimer !== null || typeof window === "undefined") return;
      reconnectTimer = window.setTimeout(() => {
        reconnectTimer = null;
        connectStream();
      }, 1000);
    };

    const connectStream = () => {
      if (disposed) return;
      const nextController = new AbortController();
      streamController = nextController;

      void sse<ConversationStreamEvent>(
        `conversations/${activeId}/stream`,
        {
          onMessage: ({ event, data }) => {
            if (disposed) return;

            if (event === "error" && data.type === "error") {
              toast.error(data.message);
              return;
            }

            if (event === "snapshot" && data.type === "snapshot") {
              applyDetail(data.conversation);
              setDetailError(null);
              setDetailLoading(false);
              return;
            }

            if (event !== "node_update" || data.type !== "node_update") return;

            const currentDetail = detailRef.current;
            if (!currentDetail) {
              scheduleRefreshDetail();
              return;
            }

            const nextDetail = applyNodeUpdate(currentDetail, data);
            if (nextDetail === currentDetail) {
              scheduleRefreshDetail();
              return;
            }

            applyDetail(nextDetail);
            setDetailError(null);
            setDetailLoading(false);
          },
          onError: (streamError) => {
            if (disposed) return;
            console.error("Conversation detail SSE error:", streamError);
            scheduleRefreshDetail();
            scheduleReconnect();
          },
          onClose: () => {
            if (disposed || nextController.signal.aborted) return;
            scheduleReconnect();
          },
        },
        { signal: nextController.signal },
      );
    };

    void refreshDetail({ showLoading: true, clearOnError: true });
    connectStream();

    return () => {
      disposed = true;
      clearReconnectTimer();
      streamController?.abort();
    };
  }, [activeId, applyDetail, refreshDetail, resetDetail, scheduleRefreshDetail]);

  React.useEffect(() => {
    if (!activeId || typeof document === "undefined") return;

    const handleVisibilityChange = () => {
      if (document.visibilityState === "visible") {
        scheduleRefreshDetail();
      }
    };

    document.addEventListener("visibilitychange", handleVisibilityChange);
    return () => {
      document.removeEventListener("visibilitychange", handleVisibilityChange);
    };
  }, [activeId, scheduleRefreshDetail]);

  React.useEffect(() => {
    if (!detail?.isGenerating) return;

    const timer = window.setInterval(() => {
      void refreshDetail({ reportError: false });
    }, 2000);

    return () => {
      window.clearInterval(timer);
    };
  }, [detail?.id, detail?.isGenerating, refreshDetail]);

  const selectedNodeMessages = React.useMemo<SelectedNodeMessage[]>(() => {
    if (!detail) {
      selectedNodeMessagesRef.current = [];
      return [];
    }

    const next = deriveSelectedNodeMessages(detail.messages, selectedNodeMessagesRef.current);
    selectedNodeMessagesRef.current = next;
    return next;
  }, [detail?.messages]);

  return {
    detail,
    detailLoading,
    detailError,
    selectedNodeMessages,
    resetDetail,
    refreshDetail,
    optimisticRegenerate,
    optimisticEditMessage,
  };
}

function useDraftInputController({
  activeId,
  isHomeRoute,
  homeDraftId,
  setHomeDraftId,
  navigate,
  refreshList,
  refreshDetail,
}: {
  activeId: string | null;
  isHomeRoute: boolean;
  homeDraftId: string;
  setHomeDraftId: React.Dispatch<React.SetStateAction<string>>;
  navigate: ReturnType<typeof useNavigate>;
  refreshList: () => void;
  refreshDetail: ConversationDetailRefresher;
}) {
  const draftKey = activeId ?? (isHomeRoute ? homeDraftId : null);
  const draft = useChatInputStore(
    React.useCallback((state) => (draftKey ? state.drafts[draftKey] : undefined), [draftKey]),
  );

  const setDraftText = useChatInputStore((state) => state.setText);
  const addDraftParts = useChatInputStore((state) => state.addParts);
  const removeDraftPart = useChatInputStore((state) => state.removePartAt);
  const getSubmitParts = useChatInputStore((state) => state.getSubmitParts);
  const clearDraft = useChatInputStore((state) => state.clearDraft);

  const inputText = draft?.text ?? "";
  const inputAttachments = draft?.parts ?? [];

  const handleInputTextChange = React.useCallback(
    (text: string) => {
      if (!draftKey) return;
      setDraftText(draftKey, text);
    },
    [draftKey, setDraftText],
  );

  const handleAddInputParts = React.useCallback(
    (parts: UIMessagePart[]) => {
      if (!draftKey || parts.length === 0) return;
      addDraftParts(draftKey, parts);
    },
    [addDraftParts, draftKey],
  );

  const handleRemoveInputPart = React.useCallback(
    (index: number) => {
      if (!draftKey) return;
      removeDraftPart(draftKey, index);
    },
    [draftKey, removeDraftPart],
  );

  const handleSubmit = React.useCallback(
    async (options?: {
      partsOverride?: UIMessagePart[];
      imageGenerationMode?: ChatInputSendOptions["imageGenerationMode"];
    }) => {
      if (!draftKey) return;

      const parts = options?.partsOverride ?? getSubmitParts(draftKey);
      if (parts.length === 0) return;

      if (activeId) {
        await api.post<{ status: string }>(`conversations/${activeId}/messages`, {
          parts,
          imageGenerationMode: options?.imageGenerationMode,
        });
        clearDraft(draftKey);
        await refreshDetail({ reportError: false });
        return;
      }

      const conversationId = uuidv4();
      setHomeDraftId(createHomeDraftId());

      await api.post<{ status: string }>(`conversations/${conversationId}/messages`, {
        parts,
        imageGenerationMode: options?.imageGenerationMode,
      });
      clearDraft(draftKey);

      navigate(`/c/${conversationId}`);
      refreshList();
    },
    [activeId, clearDraft, draftKey, getSubmitParts, navigate, refreshDetail, refreshList, setHomeDraftId],
  );

  const handleQueueSubmit = React.useCallback(
    async (options?: {
      partsOverride?: UIMessagePart[];
      imageGenerationMode?: ChatInputSendOptions["imageGenerationMode"];
    }) => {
      if (!draftKey) return;

      const parts = options?.partsOverride ?? getSubmitParts(draftKey);
      if (parts.length === 0) return;

      if (!activeId) {
        await handleSubmit(options);
        return;
      }

      await api.post<{ status: string }>(`conversations/${activeId}/messages/queue`, {
        parts,
        imageGenerationMode: options?.imageGenerationMode,
      });
      clearDraft(draftKey);
    },
    [activeId, clearDraft, draftKey, getSubmitParts, handleSubmit],
  );

  const replaceDraft = React.useCallback(
    (text: string, parts: UIMessagePart[]) => {
      if (!draftKey) return;
      clearDraft(draftKey);
      setDraftText(draftKey, text);
      addDraftParts(draftKey, parts);
    },
    [addDraftParts, clearDraft, draftKey, setDraftText],
  );

  const clearCurrentDraft = React.useCallback(() => {
    if (!draftKey) return;
    clearDraft(draftKey);
  }, [clearDraft, draftKey]);

  const getCurrentSubmitParts = React.useCallback(() => {
    if (!draftKey) return [];
    return getSubmitParts(draftKey);
  }, [draftKey, getSubmitParts]);

  return {
    draftKey,
    inputText,
    inputAttachments,
    handleInputTextChange,
    handleAddInputParts,
    handleRemoveInputPart,
    handleSubmit,
    handleQueueSubmit,
    replaceDraft,
    clearCurrentDraft,
    getCurrentSubmitParts,
  };
}

const ConversationTimeline = React.memo(({
  activeId,
  isHomeRoute,
  detailLoading,
  detailError,
  selectedNodeMessages,
  isGenerating,
  settings,
  conversationAssistantId,
  contentClassName,
  onEdit,
  onDelete,
  onQuote,
  onFork,
  onRegenerate,
  onSelectBranch,
  onToolApproval,
}: {
  activeId: string | null;
  isHomeRoute: boolean;
  detailLoading: boolean;
  detailError: string | null;
  selectedNodeMessages: SelectedNodeMessage[];
  isGenerating: boolean;
  settings: Settings | null;
  conversationAssistantId: string | null;
  contentClassName?: string;
  onEdit: (message: MessageDto) => void | Promise<void>;
  onDelete: (messageIds: string | string[]) => Promise<void>;
  onQuote: (message: MessageDto) => void | Promise<void>;
  onFork: (messageId: string) => Promise<void>;
  onRegenerate: (messageId: string) => Promise<void>;
  onSelectBranch: (nodeId: string, selectIndex: number) => Promise<void>;
  onToolApproval: (toolCallId: string, approved: boolean, reason: string) => Promise<void>;
}) => {
  const { t } = useTranslation("page");
  const timelineCacheRef = React.useRef<TimelineMessageCacheEntry[]>([]);
  const timelineItems = React.useMemo(() => {
    const next = deriveTimelineMessageCache(selectedNodeMessages, timelineCacheRef.current);
    timelineCacheRef.current = next;
    return next.map((entry) => entry.item);
  }, [selectedNodeMessages]);
  const canQuickJump =
    Boolean(activeId) && !detailLoading && !detailError && timelineItems.length > 1;
  const quickJumpItems = React.useMemo(
    () =>
      timelineItems.map(({ message }) => ({
        id: message.id,
        role: message.role,
        preview: getQuickJumpPreview(message, t),
      })),
    [t, timelineItems],
  );
  const assistant = React.useMemo(() => {
    if (!settings || !conversationAssistantId) return null;
    return settings.assistants.find((item) => item.id === conversationAssistantId) ?? null;
  }, [conversationAssistantId, settings]);
  const modelById = React.useMemo(() => {
    const map = new Map<string, ProviderModel>();
    if (!settings) return map;

    for (const provider of settings.providers) {
      for (const model of provider.models) {
        if (!map.has(model.id)) {
          map.set(model.id, model);
        }
      }
    }

    return map;
  }, [settings]);

  return (
    <Conversation className="flex-1 min-h-0">
      <ConversationContent
        className={cn("w-full gap-4 px-4 py-6", canQuickJump && "lg:pr-16", contentClassName)}
      >
        {!activeId && !isHomeRoute && (
          <ConversationEmptyState
            icon={<MessageSquare className="size-10" />}
            title={t("conversations.empty_state.select_title")}
            description={t("conversations.empty_state.select_description")}
          />
        )}
        {activeId && detailLoading && (
          <ConversationEmptyState
            title={t("conversations.empty_state.loading_title")}
            description={t("conversations.empty_state.loading_description")}
          />
        )}
        {activeId && detailError && (
          <ConversationEmptyState
            title={t("conversations.empty_state.error_title")}
            description={detailError}
          />
        )}
        {!detailLoading && !detailError && activeId && selectedNodeMessages.length === 0 && (
          <ConversationEmptyState
            icon={<MessageSquare className="size-10" />}
            title={t("conversations.empty_state.no_message_title")}
            description={t("conversations.empty_state.no_message_description")}
          />
        )}
        {!detailLoading &&
          !detailError &&
          activeId &&
          timelineItems.map((item, index) => {
            const { node, message } = item;
            const model = message.modelId ? (modelById.get(message.modelId) ?? null) : null;

            return (
              <div
                key={item.id}
                id={getConversationMessageAnchorId(item.id)}
                className="scroll-mt-24"
              >
                <ChatMessage
                  node={node}
                  message={message}
                  previousRole={index > 0 ? timelineItems[index - 1]?.message.role : null}
                  loading={isGenerating && index === timelineItems.length - 1}
                  isLastMessage={index === timelineItems.length - 1}
                  assistant={assistant}
                  model={model}
                  deleteMessageIds={item.deleteMessageIds}
                  regenerateMessageId={item.regenerateMessageId}
                  forkMessageId={item.forkMessageId}
                  disableEdit={item.disableEdit}
                  disableBranchSwitch={item.disableBranchSwitch}
                  onEdit={onEdit}
                  onDelete={onDelete}
                  onQuote={onQuote}
                  onFork={onFork}
                  onRegenerate={onRegenerate}
                  onSelectBranch={onSelectBranch}
                  onToolApproval={onToolApproval}
                />
              </div>
            );
          })}
        {!detailLoading && !detailError && activeId && isGenerating && (
          <div className="flex items-start py-2">
            <TypingIndicator className="px-1 py-2" />
          </div>
        )}
      </ConversationContent>

      {canQuickJump ? (
        <ConversationQuickJump items={quickJumpItems} />
      ) : null}

      <ConversationScrollControls items={quickJumpItems} />
    </Conversation>
  );
});

export function meta() {
  return [
    { title: i18n.t("page:conversations.meta.title") },
    {
      name: "description",
      content: i18n.t("page:conversations.meta.description"),
    },
  ];
}

export default function ConversationsPage() {
  const workbench = useWorkbenchController();

  return (
    <WorkbenchProvider value={workbench}>
      <ConversationsPageInner />
    </WorkbenchProvider>
  );
}

function ConversationsPageInner() {
  const { t } = useTranslation("page");
  const confirm = useConfirm();
  const navigate = useNavigate();
  const { id: routeId } = useParams();
  const isHomeRoute = !routeId;
  const isMobile = useIsMobile();
  const { panel, closePanel } = useWorkbench();

  const { settings, assistants, currentAssistantId, currentAssistant } = useCurrentAssistant();
  const {
    conversations,
    activeId,
    setActiveId,
    loading,
    error,
    hasMore,
    loadMore,
    refreshList,
    updateConversationSummary,
  } = useConversationList({ currentAssistantId, routeId, autoSelectFirst: !isHomeRoute });

  const [homeDraftId, setHomeDraftId] = React.useState(() => createHomeDraftId());
  const [editingSession, setEditingSession] = React.useState<EditingSession | null>(null);
  const [quotedMessage, setQuotedMessage] = React.useState<MessageDto | null>(null);

  const {
    detail,
    detailLoading,
    detailError,
    selectedNodeMessages,
    resetDetail,
    refreshDetail,
    optimisticRegenerate,
    optimisticEditMessage,
  } = useConversationDetail(activeId, updateConversationSummary);

  const {
    draftKey,
    inputText,
    inputAttachments,
    handleInputTextChange,
    handleAddInputParts,
    handleRemoveInputPart,
    handleSubmit,
    handleQueueSubmit,
    replaceDraft,
    clearCurrentDraft,
    getCurrentSubmitParts,
  } = useDraftInputController({
    activeId,
    isHomeRoute,
    homeDraftId,
    setHomeDraftId,
    navigate,
    refreshList,
    refreshDetail,
  });

  const activeConversation = conversations.find((item) => item.id === activeId);
  const chatSuggestions = detail?.chatSuggestions ?? [];

  React.useEffect(() => {
    const base = t("conversations.meta.title");
    document.title = activeConversation?.title ? `${activeConversation.title} - ${base}` : base;
    return () => {
      document.title = base;
    };
  }, [activeConversation?.title, t]);
  const isNewChat = isHomeRoute && !activeId;
  const showSuggestions =
    Boolean(activeId) && !detailLoading && !detailError && chatSuggestions.length > 0;

  const handleSelect = React.useCallback(
    (id: string) => {
      setActiveId(id);
      if (routeId !== id) {
        navigate(`/c/${id}`);
      }
    },
    [navigate, routeId, setActiveId],
  );

  React.useEffect(() => {
    setEditingSession(null);
    setQuotedMessage(null);
  }, [activeId]);

  const handleAssistantChange = React.useCallback(
    async (assistantId: string) => {
      await api.post<{ status: string }>("settings/assistant", { assistantId });
      setActiveId(null);
      resetDetail();
      if (routeId) {
        navigate("/", { replace: true });
      }
      refreshList();
    },
    [navigate, refreshList, resetDetail, routeId, setActiveId],
  );

  const handleCreateAssistantAndOpenSettings = React.useCallback(async () => {
    if (!settings) {
      throw new Error("Settings are not ready");
    }

    const next = deepCloneSettings(settings);
    const nextAssistants = Array.isArray(next.assistants) ? [...next.assistants] : [];

    let fallbackModelId =
      typeof next.chatModelId === "string" && next.chatModelId.trim().length > 0
        ? next.chatModelId.trim()
        : "auto";
    for (const provider of next.providers ?? []) {
      if (provider.enabled === false) continue;
      const chatModel = provider.models.find(
        (model) => model.type === "CHAT" && typeof model.id === "string" && model.id.trim().length > 0,
      );
      if (!chatModel) continue;
      fallbackModelId = chatModel.id;
      break;
    }

    const createdAssistant = createDefaultAssistant(nextAssistants.length, fallbackModelId);
    nextAssistants.push(createdAssistant);
    next.assistants = nextAssistants;
    next.assistantId = createdAssistant.id;

    await api.post<{ status: string }>("settings/replace", next);

    setActiveId(null);
    resetDetail();
    setHomeDraftId(createHomeDraftId());
    if (routeId) {
      navigate("/", { replace: true });
    }
    refreshList();

    navigate(
      `/settings/assistants?assistantId=${encodeURIComponent(createdAssistant.id)}&mode=new`,
    );
  }, [navigate, refreshList, resetDetail, routeId, setActiveId, setHomeDraftId, settings]);

  const handleEditAssistantInSettings = React.useCallback(
    async (assistantId: string) => {
      navigate(`/settings/assistants?assistantId=${encodeURIComponent(assistantId)}`);
    },
    [navigate],
  );

  const handleDeleteAssistantInSidebar = React.useCallback(
    async (assistantId: string) => {
      if (!settings) {
        throw new Error("Settings are not ready");
      }

      if (assistants.length <= 1) {
        throw new Error("At least one assistant is required");
      }

      const targetAssistant = assistants.find((assistant) => assistant.id === assistantId);
      if (!targetAssistant) {
        throw new Error("Assistant not found");
      }

      const targetName = getAssistantDisplayName(targetAssistant.name);
      const confirmed = await confirm({
        title: "Delete assistant?",
        description: `Delete assistant "${targetName}" and all related conversations? This action cannot be undone.`,
        confirmText: "Delete",
        cancelText: "Cancel",
        destructive: true,
      });
      if (!confirmed) {
        return;
      }

      const fallbackAssistant = assistants.find((assistant) => assistant.id !== assistantId);
      if (!fallbackAssistant) {
        throw new Error("At least one assistant is required");
      }

      const previousAssistantId = currentAssistantId ?? settings.assistantId ?? fallbackAssistant.id;
      const nextAssistantId = previousAssistantId === assistantId ? fallbackAssistant.id : previousAssistantId;

      try {
        await listAssistantConversations(assistantId);

        const next = deepCloneSettings(settings);
        next.assistants = (next.assistants ?? []).filter((assistant) => assistant.id !== assistantId);
        next.assistantId = nextAssistantId;

        await api.post<{ status: string }>("settings/replace", next);
      } finally {
        if (nextAssistantId) {
          await api.post<{ status: string }>("settings/assistant", { assistantId: nextAssistantId });
        }
      }

      if (currentAssistantId === assistantId) {
        setActiveId(null);
        resetDetail();
        setHomeDraftId(createHomeDraftId());
        if (routeId) {
          navigate("/", { replace: true });
        }
      }

      refreshList();
      toast.success(`Assistant "${targetName}" deleted`);
    },
    [assistants, currentAssistantId, navigate, refreshList, resetDetail, routeId, setActiveId, setHomeDraftId, settings],
  );
  const handleToolApproval = React.useCallback(
    async (toolCallId: string, approved: boolean, reason: string) => {
      if (!activeId) return;
      await api.post<{ status: string }>(`conversations/${activeId}/tool-approval`, {
        toolCallId,
        approved,
        reason,
      });
      await refreshDetail({ reportError: false });
    },
    [activeId, refreshDetail],
  );

  const handleRegenerate = React.useCallback(
    async (messageId: string) => {
      if (!activeId) return;
      optimisticRegenerate(messageId);
      try {
        await api.post<{ status: string }>(`conversations/${activeId}/regenerate`, {
          messageId,
        });
      } finally {
        await refreshDetail({ reportError: false });
      }
    },
    [activeId, optimisticRegenerate, refreshDetail],
  );

  const handleSelectBranch = React.useCallback(
    async (nodeId: string, selectIndex: number) => {
      if (!activeId) return;
      await api.post<{ status: string }>(`conversations/${activeId}/nodes/${nodeId}/select`, {
        selectIndex,
      });
      await refreshDetail({ reportError: false });
    },
    [activeId, refreshDetail],
  );

  const handleDeleteMessage = React.useCallback(
    async (messageIds: string | string[]) => {
      if (!activeId) return;
      const ids = Array.isArray(messageIds) ? messageIds : [messageIds];
      for (const messageId of ids) {
        await api.delete<{ status: string }>(`conversations/${activeId}/messages/${messageId}`);
      }
      await refreshDetail({ reportError: false });
    },
    [activeId, refreshDetail],
  );

  const handleForkMessage = React.useCallback(
    async (messageId: string) => {
      if (!activeId) return;
      const response = await api.post<{ conversationId: string }>(
        `conversations/${activeId}/fork`,
        {
          messageId,
        },
      );
      setActiveId(response.conversationId);
      navigate(`/c/${response.conversationId}`);
      refreshList();
    },
    [activeId, navigate, refreshList, setActiveId],
  );

  const handleStartEdit = React.useCallback(
    (message: MessageDto) => {
      if (!activeId || (message.role !== "USER" && message.role !== "ASSISTANT")) return;

      const draft = toEditDraft(message);
      if (!draft) return;

      setEditingSession({
        messageId: message.id,
        sourceParts: draft.sourceParts,
        textPartIndex: draft.textPartIndex,
      });
      replaceDraft(draft.text, draft.attachments);
    },
    [activeId, replaceDraft],
  );

  const handleCancelEdit = React.useCallback(() => {
    setEditingSession(null);
    clearCurrentDraft();
  }, [clearCurrentDraft]);

  const handleQuoteMessage = React.useCallback(
    (message: MessageDto) => {
      if (editingSession) {
        handleCancelEdit();
      }
      setQuotedMessage(message);
    },
    [editingSession, handleCancelEdit],
  );

  const handleClickSuggestion = React.useCallback(
    (suggestion: string) => {
      if (editingSession) {
        setEditingSession(null);
      }
      handleInputTextChange(suggestion);
    },
    [editingSession, handleInputTextChange],
  );

  const handleSend = React.useCallback(async (options?: ChatInputSendOptions) => {
    if (!editingSession) {
      const draftParts = getCurrentSubmitParts();

      if (!quotedMessage) {
        await handleSubmit({ imageGenerationMode: options?.imageGenerationMode });
        return;
      }

      const currentModelId = currentAssistant?.chatModelId ?? settings?.chatModelId ?? null;
      const currentModel =
        settings?.providers
          .flatMap((provider) => provider.models)
          .find((model) => model.id === currentModelId) ?? null;

      if (isImageGenerationModel(currentModel)) {
        const mergedParts = mergeQuotedAndCurrentParts(quotedMessage, draftParts);
        const imageCount = getImageParts(mergedParts).length;
        if (imageCount === 0) {
          toast.error(t("conversations.quote.no_image"), { duration: 2000 });
          return;
        }

        await handleSubmit({
          partsOverride: mergedParts,
          imageGenerationMode: "new_image",
        });
      } else {
        await handleSubmit({
          partsOverride: injectQuoteContextIntoDraftParts(quotedMessage, draftParts, t),
        });
      }

      setQuotedMessage(null);
      return;
    }

    if (!activeId) return;

    const draftParts = getCurrentSubmitParts();
    if (draftParts.length === 0) return;

    const nextParts = buildEditedParts(editingSession, draftParts);
    const strippedParts = stripEditDraftMetadata(nextParts);

    optimisticEditMessage(editingSession.messageId, strippedParts);

    try {
      await api.post<{ status: string }>(
        `conversations/${activeId}/messages/${editingSession.messageId}/edit`,
        { parts: strippedParts },
      );

      setEditingSession(null);
      clearCurrentDraft();
    } finally {
      await refreshDetail({ reportError: false });
    }
  }, [
    activeId,
    clearCurrentDraft,
    currentAssistant?.chatModelId,
    editingSession,
    getCurrentSubmitParts,
    handleSubmit,
    optimisticEditMessage,
    quotedMessage,
    refreshDetail,
    settings,
    t,
  ]);

  const handleQueueSend = React.useCallback(async (options?: ChatInputSendOptions) => {
    if (!editingSession) {
      const draftParts = getCurrentSubmitParts();

      if (!quotedMessage) {
        await handleQueueSubmit({ imageGenerationMode: options?.imageGenerationMode });
        return;
      }

      const currentModelId = currentAssistant?.chatModelId ?? settings?.chatModelId ?? null;
      const currentModel =
        settings?.providers
          .flatMap((provider) => provider.models)
          .find((model) => model.id === currentModelId) ?? null;

      if (isImageGenerationModel(currentModel)) {
        const mergedParts = mergeQuotedAndCurrentParts(quotedMessage, draftParts);
        const imageCount = getImageParts(mergedParts).length;
        if (imageCount === 0) {
          toast.error(t("conversations.quote.no_image"), { duration: 2000 });
          return;
        }

        await handleQueueSubmit({
          partsOverride: mergedParts,
          imageGenerationMode: "new_image",
        });
      } else {
        await handleQueueSubmit({
          partsOverride: injectQuoteContextIntoDraftParts(quotedMessage, draftParts, t),
        });
      }

      setQuotedMessage(null);
    }
  }, [
    currentAssistant?.chatModelId,
    editingSession,
    getCurrentSubmitParts,
    handleQueueSubmit,
    quotedMessage,
    settings,
    t,
  ]);

  const handleTogglePinConversation = React.useCallback(
    async (conversationId: string) => {
      await api.post<{ status: string }>(`conversations/${conversationId}/pin`);
      refreshList();
    },
    [refreshList],
  );

  const handleRegenerateConversationTitle = React.useCallback(
    async (conversationId: string) => {
      await api.post<{ status: string }>(`conversations/${conversationId}/regenerate-title`);
      refreshList();
    },
    [refreshList],
  );

  const handleMoveConversation = React.useCallback(
    async (conversationId: string, assistantId: string) => {
      await api.post<{ status: string }>(`conversations/${conversationId}/move`, { assistantId });
      if (conversationId === activeId) {
        setActiveId(null);
        resetDetail();
        setHomeDraftId(createHomeDraftId());
        if (routeId === conversationId) {
          navigate("/", { replace: true });
        }
      }
      refreshList();
    },
    [activeId, navigate, refreshList, resetDetail, routeId, setActiveId],
  );

  const handleUpdateConversationTitle = React.useCallback(
    async (conversationId: string, title: string) => {
      await api.post<{ status: string }>(`conversations/${conversationId}/title`, { title });
      refreshList();
    },
    [refreshList],
  );

  const handleDeleteConversation = React.useCallback(
    async (conversationId: string) => {
      await api.delete<Record<string, never>>(`conversations/${conversationId}`, {
        parseJson: (raw) => (raw ? JSON.parse(raw) : {}),
      });
      if (conversationId === activeId) {
        setActiveId(null);
        resetDetail();
        setHomeDraftId(createHomeDraftId());
        if (routeId === conversationId) {
          navigate("/", { replace: true });
        }
      }
      refreshList();
    },
    [activeId, navigate, refreshList, resetDetail, routeId, setActiveId],
  );

  const handleCreateConversation = React.useCallback(() => {
    closePanel();
    setActiveId(null);
    resetDetail();
    setHomeDraftId(createHomeDraftId());

    if (routeId) {
      navigate("/");
    }
  }, [closePanel, navigate, resetDetail, routeId, setActiveId]);

  const handleStop = React.useCallback(async () => {
    if (!activeId) return;
    await api.post<{ status: string }>(`conversations/${activeId}/stop`);
    await refreshDetail({ reportError: false });
  }, [activeId, refreshDetail]);

  const hasWorkbenchPanel = Boolean(panel);
  const workbenchPanelRef = React.useRef<PanelImperativeHandle | null>(null);

  React.useEffect(() => {
    if (isMobile) return;

    const workbenchPanel = workbenchPanelRef.current;
    if (!workbenchPanel) return;

    if (hasWorkbenchPanel) {
      workbenchPanel.expand();
    } else {
      workbenchPanel.collapse();
    }
  }, [hasWorkbenchPanel, isMobile]);

  const chatContent = (
    <div
      className={cn("flex flex-1 min-h-0 flex-col overflow-hidden", isNewChat && "justify-end")}
    >
      {!isNewChat && (
        <div className="relative flex min-h-0 flex-1">
          <ConversationTimeline
            activeId={activeId}
            isHomeRoute={isHomeRoute}
            detailLoading={detailLoading}
            detailError={detailError}
            selectedNodeMessages={selectedNodeMessages}
            isGenerating={detail?.isGenerating ?? false}
            settings={settings}
            conversationAssistantId={detail?.assistantId ?? null}
            onEdit={handleStartEdit}
            onDelete={handleDeleteMessage}
            onQuote={handleQuoteMessage}
            onFork={handleForkMessage}
            onRegenerate={handleRegenerate}
            onSelectBranch={handleSelectBranch}
            onToolApproval={handleToolApproval}
          />
        </div>
      )}

      <div>
        {isNewChat ? (
          <div className="mb-4 text-center">
            <p className="text-lg text-muted-foreground">{t("conversations.welcome_prompt")}</p>
          </div>
        ) : null}
        <ChatInput
          value={inputText}
          attachments={inputAttachments}
          ready={draftKey !== null}
          isGenerating={detail?.isGenerating ?? false}
          disabled={detailLoading || Boolean(detailError)}
          onValueChange={handleInputTextChange}
          onAddParts={handleAddInputParts}
          suggestions={showSuggestions ? chatSuggestions : []}
          onSuggestionClick={handleClickSuggestion}
          quotedMessage={quotedMessage}
          onClearQuote={() => {
            setQuotedMessage(null);
          }}
          isEditing={Boolean(editingSession)}
          onCancelEdit={editingSession ? handleCancelEdit : undefined}
          shouldDeleteFileOnRemove={shouldDeleteAttachmentFileOnRemove}
          onRemovePart={(index) => {
            handleRemoveInputPart(index);
          }}
          onSend={handleSend}
          onQueueSend={activeId ? handleQueueSend : undefined}
          onStop={activeId ? handleStop : undefined}
        />
      </div>
    </div>
  );

  return (
    <SidebarProvider defaultOpen className="h-svh overflow-hidden">
      <ConversationSidebar
        conversations={conversations}
        activeId={activeId}
        loading={loading}
        error={error}
        hasMore={hasMore}
        loadMore={loadMore}
        userName={
          settings?.displaySetting.userNickname?.trim() || t("conversations.user.default_name")
        }
        userAvatar={settings?.displaySetting.userAvatar}
        assistants={assistants}
        assistantTags={settings?.assistantTags ?? []}
        currentAssistantId={currentAssistantId}
        onSelect={handleSelect}
        onAssistantChange={handleAssistantChange}
        onCreateAssistant={handleCreateAssistantAndOpenSettings}
        onEditAssistant={handleEditAssistantInSettings}
        onDeleteAssistant={handleDeleteAssistantInSidebar}
        onPin={handleTogglePinConversation}
        onRegenerateTitle={handleRegenerateConversationTitle}
        onMoveToAssistant={handleMoveConversation}
        onUpdateTitle={handleUpdateConversationTitle}
        onDelete={handleDeleteConversation}
        onCreateConversation={handleCreateConversation}
        webAuthEnabled={settings?.webServerJwtEnabled === true}
      />
      <SidebarInset className="relative flex min-h-svh flex-col overflow-hidden">
        <SidebarTrigger className="absolute top-3 left-3 z-30 size-8 rounded-full border bg-background/85 shadow-sm backdrop-blur hover:bg-background" />
        {!isMobile ? (
          <ResizablePanelGroup orientation="horizontal" className="min-h-0 flex-1">
            <ResizablePanel
              defaultSize={hasWorkbenchPanel ? 64 : 100}
              minSize={40}
              className="flex min-h-0 flex-col"
            >
              {chatContent}
            </ResizablePanel>
            <ResizableHandle
              withHandle
              className={cn(!hasWorkbenchPanel && "pointer-events-none opacity-0")}
            />
            <ResizablePanel
              defaultSize={hasWorkbenchPanel ? 36 : 0}
              minSize={24}
              collapsible
              collapsedSize={0}
              panelRef={workbenchPanelRef}
              className="flex min-h-0 flex-col"
            >
              {panel ? (
                <WorkbenchHost panel={panel} onClose={closePanel} className="border-l-0" />
              ) : null}
            </ResizablePanel>
          </ResizablePanelGroup>
        ) : (
          chatContent
        )}

        {isMobile && panel ? (
          <Drawer
            open={hasWorkbenchPanel}
            onOpenChange={(open) => {
              if (!open) {
                closePanel();
              }
            }}
            direction="bottom"
          >
            <DrawerContent className="h-[85vh] max-h-[85vh]">
              <WorkbenchHost panel={panel} onClose={closePanel} className="border-l-0" />
            </DrawerContent>
          </Drawer>
        ) : null}
      </SidebarInset>
    </SidebarProvider>
  );
}
