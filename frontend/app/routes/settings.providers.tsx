import * as React from "react";

import { Link, useSearchParams } from "react-router";
import {
  Bot,
  ChevronLeft,
  ChevronRight,
  CircleCheck,
  Download,
  Eye,
  EyeOff,
  Home,
  LoaderCircle,
  Network,
  Package,
  Plus,
  RotateCcw,
  Save,
  Search,
  Settings2,
  Trash2,
  WalletCards,
  Wrench,
  X,
  XCircle,
} from "lucide-react";
import { toast } from "sonner";
import { v4 as uuidv4 } from "uuid";

import { useConfirm } from "~/components/confirm-dialog-provider";
import { AIIcon } from "~/components/ui/ai-icon";
import { Badge } from "~/components/ui/badge";
import { Button } from "~/components/ui/button";
import { Checkbox } from "~/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "~/components/ui/dialog";
import { Input } from "~/components/ui/input";
import { ScrollArea } from "~/components/ui/scroll-area";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "~/components/ui/select";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "~/components/ui/sheet";
import { Switch } from "~/components/ui/switch";
import { Textarea } from "~/components/ui/textarea";
import api from "~/services/api";
import { useSettingsStore } from "~/stores";
import type {
  FetchProviderModelsRequestDto,
  FetchProviderModelsResponseDto,
  ProviderModelFetchDto,
} from "~/types/dto";

export function meta() {
  return [{ title: "设置 - 供应商" }];
}

type AnyRecord = Record<string, unknown>;
type ProviderType = "openai" | "google" | "claude";
type ModelType = "CHAT" | "IMAGE" | "EMBEDDING";
type ModelModality = "TEXT" | "IMAGE";
type ModelAbility = "TOOL" | "REASONING";
type ProxyType = "none" | "http" | "socks5";
type ProviderDetailTab = "config" | "models";
type ModelEditorTab = "basic" | "advanced" | "tools";

interface ModelLibraryTemplate {
  abilities: string[];
  tools: AnyRecord[];
  inputModalities: string[];
  outputModalities: string[];
}

interface ProviderFetchState {
  loading: boolean;
  models: ProviderModelFetchDto[];
  selected: Record<string, boolean>;
  error: string | null;
}

interface ProviderProxyDraft {
  type: ProxyType;
  address: string;
  port: number;
  username: string;
  password: string;
}

interface ProviderBalanceDraft {
  enabled: boolean;
  apiPath: string;
  resultPath: string;
}

interface ProviderPreset {
  key: string;
  name: string;
  type: ProviderType;
  baseUrl: string;
  enabled?: boolean;
  description?: string;
  useResponseApi?: boolean;
  balanceOption?: Partial<ProviderBalanceDraft>;
}

interface ModelTestItem {
  status?: string;
  output?: string;
  error?: string;
}

interface ProviderModelTestResponseDto {
  providerId: string;
  modelId: string;
  nonStreaming?: ModelTestItem | null;
  streaming?: ModelTestItem | null;
  toolCall?: ModelTestItem | null;
}

interface TestDialogState {
  open: boolean;
  selectedModelId: string;
  testing: boolean;
  result: ProviderModelTestResponseDto | null;
}

const PROVIDER_TYPES: Array<{ value: ProviderType; label: string }> = [
  { value: "openai", label: "OpenAI 兼容" },
  { value: "google", label: "Google Gemini" },
  { value: "claude", label: "Claude / Anthropic" },
];

const MODEL_TYPES: Array<{ value: ModelType; label: string }> = [
  { value: "CHAT", label: "聊天" },
  { value: "IMAGE", label: "图像" },
  { value: "EMBEDDING", label: "嵌入" },
];

const PROXY_TYPES: Array<{ value: ProxyType; label: string }> = [
  { value: "none", label: "不使用代理" },
  { value: "http", label: "HTTP" },
  { value: "socks5", label: "SOCKS5" },
];

const PROVIDER_PRESETS: ProviderPreset[] = [
  {
    key: "openai",
    name: "OpenAI",
    type: "openai",
    baseUrl: "https://api.openai.com/v1",
    enabled: true,
  },
  {
    key: "gemini",
    name: "Gemini",
    type: "google",
    baseUrl: "https://generativelanguage.googleapis.com/v1beta",
    enabled: true,
  },
  {
    key: "aihubmix",
    name: "AiHubMix",
    type: "openai",
    baseUrl: "https://aihubmix.com/v1",
    enabled: true,
    description: "支持 GPT、Claude、Gemini 等 200+ 模型的 OpenAI 兼容服务。",
  },
  {
    key: "siliconflow",
    name: "硅基流动",
    type: "openai",
    baseUrl: "https://api.siliconflow.cn/v1",
    enabled: true,
    balanceOption: {
      enabled: true,
      apiPath: "/user/info",
      resultPath: "data.totalBalance",
    },
  },
  {
    key: "deepseek",
    name: "DeepSeek",
    type: "openai",
    baseUrl: "https://api.deepseek.com/v1",
    enabled: true,
    balanceOption: {
      enabled: true,
      apiPath: "/user/balance",
      resultPath: "balance_infos[0].total_balance",
    },
  },
  {
    key: "openrouter",
    name: "OpenRouter",
    type: "openai",
    baseUrl: "https://openrouter.ai/api/v1",
    enabled: true,
    balanceOption: {
      enabled: true,
      apiPath: "/credits",
      resultPath: "data.total_credits - data.total_usage",
    },
  },
  {
    key: "vercel-ai-gateway",
    name: "Vercel AI Gateway",
    type: "openai",
    baseUrl: "https://ai-gateway.vercel.sh/v1",
    balanceOption: {
      enabled: true,
      apiPath: "/credits",
      resultPath: "balance",
    },
  },
  {
    key: "tokenpony",
    name: "小马算力",
    type: "openai",
    baseUrl: "https://api.tokenpony.cn/v1",
    description: "国产模型 API 网关服务。",
  },
  {
    key: "dashscope",
    name: "阿里云百炼",
    type: "openai",
    baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
  },
  {
    key: "volcengine",
    name: "火山引擎",
    type: "openai",
    baseUrl: "https://ark.cn-beijing.volces.com/api/v3",
  },
  {
    key: "moonshot",
    name: "月之暗面",
    type: "openai",
    baseUrl: "https://api.moonshot.cn/v1",
    balanceOption: {
      enabled: true,
      apiPath: "/users/me/balance",
      resultPath: "data.available_balance",
    },
  },
  {
    key: "bigmodel",
    name: "智谱 AI 开放平台",
    type: "openai",
    baseUrl: "https://open.bigmodel.cn/api/paas/v4",
  },
  {
    key: "stepfun",
    name: "阶跃星辰",
    type: "openai",
    baseUrl: "https://api.stepfun.com/v1",
  },
  {
    key: "302ai",
    name: "302.AI",
    type: "openai",
    baseUrl: "https://api.302.ai/v1",
  },
  {
    key: "hunyuan",
    name: "腾讯 Hunyuan",
    type: "openai",
    baseUrl: "https://api.hunyuan.cloud.tencent.com/v1",
  },
  {
    key: "xai",
    name: "xAI",
    type: "openai",
    baseUrl: "https://api.x.ai/v1",
    useResponseApi: true,
  },
  {
    key: "ackai",
    name: "AckAI",
    type: "openai",
    baseUrl: "https://ackai.fun/v1",
  },
  {
    key: "unifyllm",
    name: "UnifyLLM",
    type: "openai",
    baseUrl: "https://apicn.unifyllm.top/v1",
  },
];

const DEFAULT_TITLE_PROMPT = [
  "I will give you some dialogue content in the <content> block.",
  "You need to summarize the conversation between user and assistant into a short title.",
  "1. The title language should be consistent with the user's primary language",
  "2. Do not use punctuation or other special symbols",
  "3. Reply directly with the title",
  "4. Summarize using {locale} language",
  "5. The title should not exceed 10 characters",
  "",
  "<content>",
  "{content}",
  "</content>",
].join("\n");

function deepClone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

function getString(value: unknown, fallback = ""): string {
  return typeof value === "string" ? value : fallback;
}

function getBoolean(value: unknown, fallback = false): boolean {
  return typeof value === "boolean" ? value : fallback;
}

function getNumber(value: unknown, fallback = 0): number {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function ensureArray<T>(value: unknown): T[] {
  return Array.isArray(value) ? (value as T[]) : [];
}

function ensureStringValues(value: unknown): string[] {
  return [
    ...new Set(
      ensureArray<unknown>(value)
        .map((item) => getString(item).trim())
        .filter(Boolean),
    ),
  ];
}

function normalizeToolType(value: unknown): string {
  return getString(value).trim().toLowerCase();
}

function normalizeToolList(value: unknown): AnyRecord[] {
  const normalized: Array<{ type: string }> = [];
  for (const item of ensureArray<unknown>(value)) {
    if (typeof item === "string") {
      const type = normalizeToolType(item);
      if (type.length > 0) {
        normalized.push({ type });
      }
      continue;
    }

    if (item && typeof item === "object") {
      const source = item as AnyRecord;
      const type = normalizeToolType(source.type);
      if (type.length > 0) {
        normalized.push({ type });
      }
    }
  }

  return normalized.filter((tool, index, items) => {
    return index === items.findIndex((item) => item.type === tool.type);
  }) as AnyRecord[];
}

function normalizeModalityType(value: unknown): string {
  const normalized = getString(value).trim().toUpperCase();
  if (normalized === "TEXT" || normalized === "IMAGE") {
    return normalized;
  }
  return "";
}

function normalizeModalityList(value: unknown): string[] {
  const normalized: string[] = [];
  for (const item of ensureArray<unknown>(value)) {
    const normalizedItem = normalizeModalityType(item);
    if (normalizedItem.length > 0 && !normalized.includes(normalizedItem)) {
      normalized.push(normalizedItem);
    }
  }
  return normalized;
}

function readModelLibrary(settings: AnyRecord | null): AnyRecord[] {
  return ensureArray<AnyRecord>(settings?.modelLibrary);
}

function normalizeModelRef(value: unknown): string {
  return getString(value).trim().toLowerCase();
}

function buildModelLibraryTemplate(
  settings: AnyRecord | null,
  modelId: string,
): ModelLibraryTemplate {
  const targetRef = normalizeModelRef(modelId);
  if (!targetRef) {
    return {
      abilities: ["TOOL"],
      tools: [],
      inputModalities: ["TEXT"],
      outputModalities: ["TEXT"],
    };
  }
  const entry = readModelLibrary(settings).find(
    (item) => normalizeModelRef(item.modelId) === targetRef,
  );
  if (!entry) {
    return {
      abilities: ["TOOL"],
      tools: [],
      inputModalities: ["TEXT"],
      outputModalities: ["TEXT"],
    };
  }
  return {
    abilities: ensureStringValues(entry.abilities),
    tools: normalizeToolList(entry.tools),
    inputModalities: normalizeModalityList(entry.inputModalities),
    outputModalities: normalizeModalityList(entry.outputModalities),
  };
}

function mergeModelLibraryTemplate(
  defaultTemplate: ModelLibraryTemplate,
  customTemplate: ModelLibraryTemplate,
): ModelLibraryTemplate {
  return {
    abilities:
      customTemplate.abilities.length > 0 ? customTemplate.abilities : defaultTemplate.abilities,
    tools: customTemplate.tools.length > 0 ? customTemplate.tools : defaultTemplate.tools,
    inputModalities:
      customTemplate.inputModalities.length > 0
        ? customTemplate.inputModalities
        : defaultTemplate.inputModalities,
    outputModalities:
      customTemplate.outputModalities.length > 0
        ? customTemplate.outputModalities
        : defaultTemplate.outputModalities,
  };
}

function modelTemplateFromLibrary(
  settings: AnyRecord | null,
  modelId: string,
  fallback: ModelLibraryTemplate,
): ModelLibraryTemplate {
  return mergeModelLibraryTemplate(fallback, buildModelLibraryTemplate(settings, modelId));
}

function findModelLibraryIndex(library: AnyRecord[], modelRef: string): number {
  return library.findIndex((item) => normalizeModelRef(item.modelId) === modelRef);
}

function syncModelAbilitiesToLibrary(
  library: AnyRecord[],
  modelRef: string,
  abilities: string[],
  tools: AnyRecord[],
  inputModalities: string[],
  outputModalities: string[],
): AnyRecord[] {
  if (!modelRef) {
    return library;
  }

  const index = findModelLibraryIndex(library, modelRef);
  if (index >= 0) {
    library[index] = {
      ...(library[index] as AnyRecord),
      modelId: getString(library[index].modelId, modelRef),
      abilities,
      tools,
      inputModalities,
      outputModalities,
    };
    return library;
  }

  library.push({
    modelId: modelRef,
    abilities,
    tools,
    inputModalities,
    outputModalities,
  });
  return library;
}

function syncModelRefAcrossProviders(
  settings: AnyRecord,
  modelRef: string,
  abilities: string[],
  tools: AnyRecord[],
  inputModalities: string[],
  outputModalities: string[],
): void {
  if (!modelRef) {
    return;
  }

  const nextProviders = ensureArray<AnyRecord>(settings.providers);
  for (const provider of nextProviders) {
    const nextModels = ensureArray<AnyRecord>(provider.models);
    provider.models = nextModels.map((item) => {
      if (normalizeModelRef(item.modelId) !== modelRef) {
        return item;
      }
      return {
        ...item,
        abilities,
        tools,
        inputModalities,
        outputModalities,
      };
    });
  }
  settings.providers = nextProviders;
}

function removeModelLibraryIfUnused(
  library: AnyRecord[],
  providers: AnyRecord[],
  modelRef: string,
): AnyRecord[] {
  if (!modelRef) {
    return library;
  }
  const used = ensureArray<AnyRecord>(providers).some((provider) =>
    ensureArray<AnyRecord>(provider.models).some(
      (model) => normalizeModelRef(model.modelId) === modelRef,
    ),
  );
  if (used) {
    return library;
  }
  return library.filter((item) => normalizeModelRef(item.modelId) !== modelRef);
}

function normalizeProviderType(value: unknown): ProviderType {
  const normalized = getString(value).trim().toLowerCase();
  if (normalized === "google") return "google";
  if (normalized === "claude") return "claude";
  return "openai";
}

function normalizeModelType(value: unknown): ModelType {
  const normalized = getString(value).trim().toUpperCase();
  if (normalized === "IMAGE") return "IMAGE";
  if (normalized === "EMBEDDING") return "EMBEDDING";
  return "CHAT";
}

function providerDefaults(type: ProviderType): AnyRecord {
  if (type === "google") {
    return {
      name: "Google",
      baseUrl: "https://generativelanguage.googleapis.com/v1beta",
      vertexAI: false,
    };
  }

  if (type === "claude") {
    return {
      name: "Claude",
      baseUrl: "https://api.anthropic.com/v1",
    };
  }

  return {
    name: "OpenAI",
    baseUrl: "https://api.openai.com/v1",
    chatCompletionsPath: "/chat/completions",
    useResponseApi: false,
  };
}

function buildDefaultProxy(): ProviderProxyDraft {
  return {
    type: "none",
    address: "",
    port: 7890,
    username: "",
    password: "",
  };
}

function readProxy(provider: AnyRecord): ProviderProxyDraft {
  const source = (provider.proxy as AnyRecord | undefined) ?? {};
  const normalizedType = getString(source.type).trim().toLowerCase();
  const type: ProxyType =
    normalizedType === "http" || normalizedType === "socks5" ? normalizedType : "none";
  return {
    type,
    address: getString(source.address),
    port: getNumber(source.port, 7890),
    username: getString(source.username),
    password: getString(source.password),
  };
}

function buildDefaultBalanceOption(): ProviderBalanceDraft {
  return {
    enabled: false,
    apiPath: "/credits",
    resultPath: "data.total_usage",
  };
}

function readBalanceOption(provider: AnyRecord): ProviderBalanceDraft {
  const source = (provider.balanceOption as AnyRecord | undefined) ?? {};
  return {
    enabled: getBoolean(source.enabled, false),
    apiPath: getString(source.apiPath, "/credits"),
    resultPath: getString(source.resultPath, "data.total_usage"),
  };
}

function buildDefaultModel(
  seed?: Partial<ProviderModelFetchDto>,
  template?: ModelLibraryTemplate,
): AnyRecord {
  const type = normalizeModelType(seed?.type);
  const imageModel = type === "IMAGE";
  const nextTemplate: ModelLibraryTemplate = mergeModelLibraryTemplate(
    {
      abilities: ["TOOL"],
      tools: [],
      inputModalities: imageModel ? ["TEXT", "IMAGE"] : ["TEXT"],
      outputModalities: imageModel ? ["TEXT", "IMAGE"] : ["TEXT"],
    },
    template ?? {
      abilities: ["TOOL"],
      tools: [],
      inputModalities: imageModel ? ["TEXT", "IMAGE"] : ["TEXT"],
      outputModalities: imageModel ? ["TEXT", "IMAGE"] : ["TEXT"],
    },
  );
  return {
    id: uuidv4(),
    modelId: getString(seed?.modelId, "gpt-4.1"),
    displayName: getString(seed?.displayName, getString(seed?.modelId, "GPT-4.1")),
    type,
    inputModalities: nextTemplate.inputModalities,
    outputModalities: nextTemplate.outputModalities,
    abilities: nextTemplate.abilities,
    tools: nextTemplate.tools,
    customHeaders: [],
    customBodies: [],
  };
}

function buildDefaultProvider(type: ProviderType = "openai"): AnyRecord {
  return {
    id: uuidv4(),
    enabled: true,
    apiKey: "",
    models: [],
    proxy: buildDefaultProxy(),
    balanceOption: buildDefaultBalanceOption(),
    ...providerDefaults(type),
    type,
  };
}

function buildPresetProvider(preset: ProviderPreset): AnyRecord {
  return {
    id: uuidv4(),
    enabled: preset.enabled ?? false,
    name: preset.name,
    type: preset.type,
    baseUrl: preset.baseUrl,
    apiKey: "",
    models: [],
    proxy: buildDefaultProxy(),
    balanceOption: preset.balanceOption ?? buildDefaultBalanceOption(),
    builtIn: true,
    presetKey: preset.key,
    presetDescription: preset.description ?? "",
    ...(preset.type === "openai"
      ? {
          chatCompletionsPath: "/chat/completions",
          useResponseApi: preset.useResponseApi ?? false,
        }
      : {}),
    ...(preset.type === "google"
      ? {
          vertexAI: false,
          useServiceAccount: false,
          privateKey: "",
          serviceAccountEmail: "",
          location: "us-central1",
          projectId: "",
        }
      : {}),
    ...(preset.type === "claude"
      ? {
          promptCaching: false,
        }
      : {}),
  };
}

function hasBuiltInTool(model: AnyRecord, toolType: string): boolean {
  return ensureArray<AnyRecord>(model.tools).some(
    (tool) => getString(tool.type).toLowerCase() === toolType.toLowerCase(),
  );
}

function toggleItem(values: string[], item: string, enabled: boolean): string[] {
  const normalized = values.filter((value) => value.trim().length > 0);
  if (enabled) {
    return normalized.includes(item) ? normalized : [...normalized, item];
  }
  return normalized.filter((value) => value !== item);
}

function upsertModelTool(model: AnyRecord, toolType: string, enabled: boolean): AnyRecord[] {
  const tools = ensureArray<AnyRecord>(model.tools).filter(
    (tool) => getString(tool.type) !== toolType,
  );
  if (enabled) {
    tools.push({ type: toolType });
  }
  return tools;
}

function modelAliasesFromValue(value: unknown): string[] {
  const normalized = normalizeModelRef(value);
  if (normalized.length === 0) return [];

  const aliases = new Set<string>([normalized]);
  const slashIndex = normalized.lastIndexOf("/");
  if (slashIndex >= 0 && slashIndex + 1 < normalized.length) {
    aliases.add(normalized.slice(slashIndex + 1));
  }
  return [...aliases];
}

function modelAliases(model: AnyRecord): string[] {
  const aliases = new Set<string>();
  modelAliasesFromValue(model.modelId).forEach((value) => aliases.add(value));
  modelAliasesFromValue(model.id).forEach((value) => aliases.add(value));
  return [...aliases];
}

function fetchedModelAliases(model: ProviderModelFetchDto): string[] {
  return modelAliasesFromValue(model.modelId);
}

function isChatSelectableModel(model: AnyRecord): boolean {
  const type = normalizeModelType(model.type);
  if (type === "CHAT") return true;
  if (type !== "IMAGE") return false;

  const inputModalities = ensureArray<unknown>(model.inputModalities)
    .map((item) => getString(item).trim().toUpperCase())
    .filter((item) => item.length > 0);

  return inputModalities.length === 0 || inputModalities.includes("TEXT");
}

function providerTypeLabel(type: ProviderType): string {
  return PROVIDER_TYPES.find((item) => item.value === type)?.label ?? type;
}

function modelTypeLabel(type: ModelType): string {
  return MODEL_TYPES.find((item) => item.value === type)?.label ?? type;
}

function modalityLabel(value: string): string {
  const normalized = value.trim().toUpperCase();
  if (normalized === "TEXT") return "文本";
  if (normalized === "IMAGE") return "图像";
  return value;
}

function abilityLabel(value: string): string {
  const normalized = value.trim().toUpperCase();
  if (normalized === "TOOL") return "工具";
  if (normalized === "REASONING") return "推理";
  return value;
}

function providerHost(baseUrl: unknown): string {
  const value = getString(baseUrl).trim();
  if (!value) return "未配置 Base URL";
  try {
    return new URL(value).host;
  } catch {
    return value;
  }
}

function providerKey(provider: AnyRecord, index: number): string {
  return getString(provider.id) || `provider-${index}`;
}

function buildNextSettings(
  current: AnyRecord | null,
  mutator: (next: AnyRecord) => void,
): AnyRecord {
  const next = current ? deepClone(current) : {};
  mutator(next);
  return next;
}

export default function SettingsProvidersPage() {
  const settings = useSettingsStore((state) => state.settings) as AnyRecord | null;
  const [searchParams] = useSearchParams();
  const [draft, setDraft] = React.useState<AnyRecord | null>(settings ? deepClone(settings) : null);
  const draftRef = React.useRef<AnyRecord | null>(settings ? deepClone(settings) : null);
  const [busy, setBusy] = React.useState(false);
  const [showSecrets, setShowSecrets] = React.useState(false);
  const [dirty, setDirty] = React.useState(false);
  const [addProviderType, setAddProviderType] = React.useState<ProviderType>("openai");
  const [addPresetKey, setAddPresetKey] = React.useState<string>(PROVIDER_PRESETS[0]?.key ?? "");
  const [fetchStates, setFetchStates] = React.useState<Record<string, ProviderFetchState>>({});
  const [defaultModelsCollapsed, setDefaultModelsCollapsed] = React.useState(true);
  const [titleSummaryCollapsed, setTitleSummaryCollapsed] = React.useState(true);
  const [providerSearch, setProviderSearch] = React.useState("");
  const [selectedProviderId, setSelectedProviderId] = React.useState<string | null>(null);
  const [providerTab, setProviderTab] = React.useState<ProviderDetailTab>("config");
  const [proxyCollapsed, setProxyCollapsed] = React.useState(true);
  const [balanceCollapsed, setBalanceCollapsed] = React.useState(true);
  const [modelLibrarySearch, setModelLibrarySearch] = React.useState("");
  const [modelLibraryOpen, setModelLibraryOpen] = React.useState(false);
  const [editingModelIndex, setEditingModelIndex] = React.useState<number | null>(null);
  const [modelEditorTab, setModelEditorTab] = React.useState<ModelEditorTab>("basic");
  const [draftModel, setDraftModel] = React.useState<AnyRecord | null>(null);
  const [testDialog, setTestDialog] = React.useState<TestDialogState>({
    open: false,
    selectedModelId: "",
    testing: false,
    result: null,
  });
  const confirm = useConfirm();

  const defaultsOnly = searchParams.get("section") === "defaults";

  React.useEffect(() => {
    draftRef.current = draft;
  }, [draft]);

  React.useEffect(() => {
    if (!settings || dirty) return;
    const next = deepClone(settings);
    draftRef.current = next;
    setDraft(next);
  }, [settings, dirty]);

  const providers = ensureArray<AnyRecord>(draft?.providers);

  const filteredProviders = React.useMemo(() => {
    const query = providerSearch.trim().toLowerCase();
    return providers
      .map((provider, index) => ({ provider, index }))
      .filter(({ provider }) => {
        if (!query) return true;
        return (
          getString(provider.name).toLowerCase().includes(query) ||
          getString(provider.baseUrl).toLowerCase().includes(query) ||
          getString(provider.presetDescription).toLowerCase().includes(query)
        );
      });
  }, [providers, providerSearch]);

  const currentProviderIndex = selectedProviderId
    ? providers.findIndex((provider, index) => providerKey(provider, index) === selectedProviderId)
    : -1;
  const currentProvider = currentProviderIndex >= 0 ? providers[currentProviderIndex] : null;

  React.useEffect(() => {
    if (!selectedProviderId) return;
    if (currentProviderIndex >= 0) return;

    setSelectedProviderId(null);
    setEditingModelIndex(null);
    setDraftModel(null);
  }, [selectedProviderId, currentProviderIndex]);

  React.useEffect(() => {
    setProxyCollapsed(true);
    setBalanceCollapsed(true);
    setModelLibrarySearch("");
    setModelLibraryOpen(false);
    setEditingModelIndex(null);
    setDraftModel(null);
    setModelEditorTab("basic");
    setTestDialog({
      open: false,
      selectedModelId: "",
      testing: false,
      result: null,
    });
  }, [selectedProviderId]);

  const currentModels = currentProvider ? ensureArray<AnyRecord>(currentProvider.models) : [];

  React.useEffect(() => {
    if (editingModelIndex === null || !currentProvider) return;
    if (editingModelIndex < currentModels.length) return;

    setEditingModelIndex(null);
    setDraftModel(null);
  }, [editingModelIndex, currentProvider, currentModels.length]);

  const titleModelOptions = React.useMemo(() => {
    const options: Array<{ id: string; label: string }> = [];
    const seen = new Set<string>();

    providers.forEach((provider, providerIndex) => {
      const providerName = getString(provider.name, `供应商 ${providerIndex + 1}`);
      ensureArray<AnyRecord>(provider.models).forEach((model) => {
        if (normalizeModelType(model.type) !== "CHAT") return;

        const id = getString(model.id).trim();
        if (!id || seen.has(id)) return;

        seen.add(id);
        const displayName = getString(model.displayName, id).trim() || id;
        const modelId = getString(model.modelId).trim();
        const suffix = modelId ? ` (${providerName} / ${modelId})` : ` (${providerName})`;
        options.push({ id, label: `${displayName}${suffix}` });
      });
    });

    if (!seen.has("auto")) {
      options.unshift({ id: "auto", label: "自动" });
    }

    return options;
  }, [providers]);

  const selectedTitleModelId = React.useMemo(() => {
    const explicit = getString(draft?.titleModelId).trim();
    if (explicit) return explicit;
    const fallback = getString(draft?.chatModelId).trim();
    return fallback || "auto";
  }, [draft?.titleModelId, draft?.chatModelId]);

  const titlePromptValue = getString(draft?.titlePrompt, DEFAULT_TITLE_PROMPT);
  const titleModelSelectValue = titleModelOptions.some((item) => item.id === selectedTitleModelId)
    ? selectedTitleModelId
    : "auto";

  const currentProviderId = currentProvider ? getString(currentProvider.id) : "";
  const currentProviderType = currentProvider
    ? normalizeProviderType(currentProvider.type)
    : "openai";
  const currentFetchState = currentProviderId ? fetchStates[currentProviderId] : undefined;
  const chatModels = React.useMemo(
    () => currentModels.filter((model) => normalizeModelType(model.type) === "CHAT"),
    [currentModels],
  );

  const buildLatestDraft = React.useCallback((mutator: (next: AnyRecord) => void): AnyRecord => {
    const next = buildNextSettings(draftRef.current, mutator);
    draftRef.current = next;
    setDraft(next);
    return next;
  }, []);

  const persistSettings = React.useCallback(
    async (
      next: AnyRecord,
      options?: {
        successMessage?: string | null;
        errorMessage?: string;
        clearDirty?: boolean;
      },
    ) => {
      setBusy(true);
      try {
        await api.post<{ status: string }>("settings/replace", next);
        if (options?.clearDirty !== false) {
          setDirty(false);
        }
        if (options?.successMessage) {
          toast.success(options.successMessage);
        }
      } catch (error) {
        console.error("settings/replace failed", error);
        toast.error(error instanceof Error ? error.message : (options?.errorMessage ?? "保存失败"));
      } finally {
        setBusy(false);
      }
    },
    [],
  );

  const autosaveSettings = React.useCallback(
    async (next: AnyRecord, errorMessage = "自动保存失败") => {
      setBusy(true);
      try {
        await api.post<{ status: string }>("settings/replace", next);
        setDirty(false);
      } catch (error) {
        console.error("settings/replace autosave failed", error);
        toast.error(error instanceof Error ? error.message : errorMessage);
      } finally {
        setBusy(false);
      }
    },
    [],
  );

  const saveCurrentDraft = React.useCallback(async () => {
    const next = draftRef.current;
    if (!next) return;
    await persistSettings(next, { successMessage: "设置已保存", clearDirty: true });
  }, [persistSettings]);

  const updateConfigOnly = React.useCallback(
    (mutator: (next: AnyRecord) => void) => {
      buildLatestDraft(mutator);
      setDirty(true);
    },
    [buildLatestDraft],
  );

  const updateTitleModel = React.useCallback(
    (modelId: string) => {
      const next = buildLatestDraft((settingsDraft) => {
        settingsDraft.titleModelId = modelId;
        if (getString(settingsDraft.titlePrompt).trim().length === 0) {
          settingsDraft.titlePrompt = DEFAULT_TITLE_PROMPT;
        }
      });
      void autosaveSettings(next);
    },
    [autosaveSettings, buildLatestDraft],
  );

  const updateTitlePrompt = React.useCallback(
    (prompt: string) => {
      const next = buildLatestDraft((settingsDraft) => {
        settingsDraft.titlePrompt = prompt;
      });
      void autosaveSettings(next);
    },
    [autosaveSettings, buildLatestDraft],
  );

  const resetTitlePrompt = React.useCallback(() => {
    const next = buildLatestDraft((settingsDraft) => {
      settingsDraft.titlePrompt = DEFAULT_TITLE_PROMPT;
    });
    void autosaveSettings(next);
  }, [autosaveSettings, buildLatestDraft]);

  const updateProvider = React.useCallback(
    (providerIndex: number, patch: Partial<AnyRecord>) => {
      updateConfigOnly((settingsDraft) => {
        const nextProviders = ensureArray<AnyRecord>(settingsDraft.providers);
        const provider = nextProviders[providerIndex];
        if (!provider) return;
        nextProviders[providerIndex] = { ...provider, ...patch };
        settingsDraft.providers = nextProviders;
      });
    },
    [updateConfigOnly],
  );

  const updateProviderType = React.useCallback(
    (providerIndex: number, nextType: ProviderType) => {
      updateConfigOnly((settingsDraft) => {
        const nextProviders = ensureArray<AnyRecord>(settingsDraft.providers);
        const provider = nextProviders[providerIndex];
        if (!provider) return;

        const previousType = normalizeProviderType(provider.type);
        const nextDefaults = providerDefaults(nextType);
        const previousDefaults = providerDefaults(previousType);
        const currentName = getString(provider.name);
        const currentBaseUrl = getString(provider.baseUrl);

        const mutated: AnyRecord = {
          ...provider,
          ...nextDefaults,
          type: nextType,
          name:
            currentName.length === 0 || currentName === getString(previousDefaults.name)
              ? getString(nextDefaults.name)
              : currentName,
          baseUrl:
            currentBaseUrl.length === 0 || currentBaseUrl === getString(previousDefaults.baseUrl)
              ? getString(nextDefaults.baseUrl)
              : currentBaseUrl,
        };

        if (nextType !== "openai") {
          delete mutated.chatCompletionsPath;
          delete mutated.useResponseApi;
        } else {
          mutated.chatCompletionsPath = getString(mutated.chatCompletionsPath, "/chat/completions");
          mutated.useResponseApi = getBoolean(mutated.useResponseApi, false);
        }

        if (nextType !== "google") {
          delete mutated.vertexAI;
          delete mutated.useServiceAccount;
          delete mutated.privateKey;
          delete mutated.serviceAccountEmail;
          delete mutated.location;
          delete mutated.projectId;
        } else {
          mutated.vertexAI = getBoolean(mutated.vertexAI, false);
          mutated.useServiceAccount = getBoolean(mutated.useServiceAccount, false);
          mutated.privateKey = getString(mutated.privateKey);
          mutated.serviceAccountEmail = getString(mutated.serviceAccountEmail);
          mutated.location = getString(mutated.location, "us-central1");
          mutated.projectId = getString(mutated.projectId);
        }

        if (nextType !== "claude") {
          delete mutated.promptCaching;
        } else {
          mutated.promptCaching = getBoolean(mutated.promptCaching, false);
        }

        nextProviders[providerIndex] = mutated;
        settingsDraft.providers = nextProviders;
      });
    },
    [updateConfigOnly],
  );

  const updateProviderProxy = React.useCallback(
    (providerIndex: number, patch: Partial<ProviderProxyDraft>) => {
      updateConfigOnly((settingsDraft) => {
        const nextProviders = ensureArray<AnyRecord>(settingsDraft.providers);
        const provider = nextProviders[providerIndex];
        if (!provider) return;

        nextProviders[providerIndex] = {
          ...provider,
          proxy: {
            ...readProxy(provider),
            ...patch,
          },
        };
        settingsDraft.providers = nextProviders;
      });
    },
    [updateConfigOnly],
  );

  const updateBalanceOption = React.useCallback(
    (providerIndex: number, patch: Partial<ProviderBalanceDraft>) => {
      updateConfigOnly((settingsDraft) => {
        const nextProviders = ensureArray<AnyRecord>(settingsDraft.providers);
        const provider = nextProviders[providerIndex];
        if (!provider) return;

        nextProviders[providerIndex] = {
          ...provider,
          balanceOption: {
            ...readBalanceOption(provider),
            ...patch,
          },
        };
        settingsDraft.providers = nextProviders;
      });
    },
    [updateConfigOnly],
  );

  const addPresetProvider = React.useCallback(() => {
    const preset = PROVIDER_PRESETS.find((item) => item.key === addPresetKey);
    if (!preset) return;

    const provider = buildPresetProvider(preset);
    const next = buildLatestDraft((settingsDraft) => {
      const nextProviders = ensureArray<AnyRecord>(settingsDraft.providers);
      nextProviders.push(provider);
      settingsDraft.providers = nextProviders;
    });

    setSelectedProviderId(getString(provider.id));
    setProviderTab("config");
    void autosaveSettings(next);
  }, [addPresetKey, autosaveSettings, buildLatestDraft]);

  const addCustomProvider = React.useCallback(() => {
    const provider = buildDefaultProvider(addProviderType);
    const next = buildLatestDraft((settingsDraft) => {
      const nextProviders = ensureArray<AnyRecord>(settingsDraft.providers);
      nextProviders.push(provider);
      settingsDraft.providers = nextProviders;
    });

    setSelectedProviderId(getString(provider.id));
    setProviderTab("config");
    void autosaveSettings(next);
  }, [addProviderType, autosaveSettings, buildLatestDraft]);

  const completePresetProviders = React.useCallback(() => {
    const next = buildLatestDraft((settingsDraft) => {
      const nextProviders = ensureArray<AnyRecord>(settingsDraft.providers);
      const existingPresetKeys = new Set(
        nextProviders.map((provider) => getString(provider.presetKey)).filter(Boolean),
      );
      const existingBaseUrls = new Set(
        nextProviders
          .map((provider) => getString(provider.baseUrl).trim().toLowerCase())
          .filter(Boolean),
      );

      PROVIDER_PRESETS.forEach((preset) => {
        if (
          existingPresetKeys.has(preset.key) ||
          existingBaseUrls.has(preset.baseUrl.toLowerCase())
        )
          return;
        nextProviders.push(buildPresetProvider(preset));
      });

      settingsDraft.providers = nextProviders;
    });

    void autosaveSettings(next);
  }, [autosaveSettings, buildLatestDraft]);

  const deleteProvider = React.useCallback(
    async (providerIndex: number) => {
      const confirmed = await confirm({
        title: "删除提供商？",
        description: "这会删除该提供商及其全部模型。",
        confirmText: "删除",
        cancelText: "取消",
        destructive: true,
      });
      if (!confirmed) return;

      const next = buildLatestDraft((settingsDraft) => {
        const nextProviders = ensureArray<AnyRecord>(settingsDraft.providers);
        nextProviders.splice(providerIndex, 1);
        settingsDraft.providers = nextProviders;
      });

      setSelectedProviderId(null);
      setEditingModelIndex(null);
      setDraftModel(null);
      void autosaveSettings(next);
    },
    [autosaveSettings, buildLatestDraft, confirm],
  );

  const resetProviderBaseUrl = React.useCallback(
    (providerIndex: number) => {
      const provider = providers[providerIndex];
      if (!provider) return;

      const presetKey = getString(provider.presetKey);
      const preset = PROVIDER_PRESETS.find((item) => item.key === presetKey);
      const defaults = providerDefaults(normalizeProviderType(provider.type));
      updateProvider(providerIndex, {
        baseUrl: preset?.baseUrl ?? getString(defaults.baseUrl),
      });
    },
    [providers, updateProvider],
  );

  const setFetchState = React.useCallback(
    (providerId: string, updater: (prev: ProviderFetchState) => ProviderFetchState) => {
      setFetchStates((prev) => {
        const current = prev[providerId] ?? {
          loading: false,
          models: [],
          selected: {},
          error: null,
        };
        return {
          ...prev,
          [providerId]: updater(current),
        };
      });
    },
    [],
  );

  const fetchProviderModels = React.useCallback(
    async (providerIndex: number) => {
      const provider = providers[providerIndex];
      if (!provider) return;

      const providerId = getString(provider.id);
      if (!providerId) {
        toast.error("供应商 ID 缺失");
        return;
      }
      if (dirty) {
        setFetchState(providerId, (state) => ({
          ...state,
          loading: false,
          error: "请先保存配置后再获取模型",
        }));
        toast.error("请先保存配置后再获取模型");
        return;
      }

      setFetchState(providerId, (state) => ({ ...state, loading: true, error: null }));
      try {
        const requestBody: FetchProviderModelsRequestDto = { providerId };
        const response = await api.post<FetchProviderModelsResponseDto>(
          "settings/provider/models/fetch",
          requestBody,
        );
        const existingIds = new Set(
          ensureArray<AnyRecord>(provider.models)
            .flatMap((model) => modelAliases(model))
            .filter((item) => item.length > 0),
        );

        const selected: Record<string, boolean> = {};
        response.models.forEach((model) => {
          selected[model.modelId] = fetchedModelAliases(model).some((value) =>
            existingIds.has(value),
          );
        });

        setFetchState(providerId, () => ({
          loading: false,
          models: response.models,
          selected,
          error: null,
        }));
        setModelLibraryOpen(true);
        toast.success(`已获取 ${response.models.length} 个模型`);
      } catch (error) {
        console.error("settings/provider/models/fetch failed", error);
        const message = error instanceof Error ? error.message : "获取模型失败";
        setFetchState(providerId, (state) => ({ ...state, loading: false, error: message }));
        toast.error(message);
      }
    },
    [dirty, providers, setFetchState],
  );

  const toggleFetchedSelection = React.useCallback(
    (providerId: string, modelId: string, checked: boolean) => {
      setFetchState(providerId, (state) => ({
        ...state,
        selected: {
          ...state.selected,
          [modelId]: checked,
        },
      }));
    },
    [setFetchState],
  );

  const currentFetchedModels = currentFetchState?.models ?? [];
  const filteredFetchedModels = React.useMemo(() => {
    const query = modelLibrarySearch.trim().toLowerCase();
    if (!query) return currentFetchedModels;

    return currentFetchedModels.filter((model) => {
      return (
        model.modelId.toLowerCase().includes(query) ||
        model.displayName.toLowerCase().includes(query) ||
        normalizeModelType(model.type).toLowerCase().includes(query)
      );
    });
  }, [currentFetchedModels, modelLibrarySearch]);

  const selectedFetchedCount = currentFetchState
    ? Object.values(currentFetchState.selected).filter(Boolean).length
    : 0;
  const visibleSelectedFetchedCount = React.useMemo(
    () =>
      filteredFetchedModels.filter((model) => Boolean(currentFetchState?.selected[model.modelId]))
        .length,
    [currentFetchState?.selected, filteredFetchedModels],
  );

  const toggleVisibleFetchedModels = React.useCallback(() => {
    if (!currentProviderId || !currentFetchState || filteredFetchedModels.length === 0) return;
    const selectAll = visibleSelectedFetchedCount < filteredFetchedModels.length;
    setFetchState(currentProviderId, (state) => {
      const selected = { ...state.selected };
      filteredFetchedModels.forEach((model) => {
        selected[model.modelId] = selectAll;
      });
      return { ...state, selected };
    });
  }, [
    currentFetchState,
    currentProviderId,
    filteredFetchedModels,
    setFetchState,
    visibleSelectedFetchedCount,
  ]);

  const importFetchedModels = React.useCallback(
    async (providerIndex: number) => {
      const provider = providers[providerIndex];
      if (!provider) return;

      const providerId = getString(provider.id);
      const state = fetchStates[providerId];
      if (!state || state.models.length === 0) {
        toast.error("没有可导入的已获取模型");
        return;
      }

      const selectedModels = state.models.filter((model) => state.selected[model.modelId]);
      if (selectedModels.length === 0) {
        toast.error("至少选择一个模型");
        return;
      }

      const existingIds = new Set(
        ensureArray<AnyRecord>(provider.models)
          .flatMap((model) => modelAliases(model))
          .filter((item) => item.length > 0),
      );
      const importable = selectedModels.filter(
        (model) => !fetchedModelAliases(model).some((item) => existingIds.has(item)),
      );
      if (importable.length === 0) {
        toast.message("选中的模型已存在");
        return;
      }

      const next = buildLatestDraft((settingsDraft) => {
        const nextProviders = ensureArray<AnyRecord>(settingsDraft.providers);
        const target = nextProviders[providerIndex];
        if (!target) return;

        const nextModels = ensureArray<AnyRecord>(target.models);
        importable.forEach((item) => {
          const template = modelTemplateFromLibrary(draftRef.current, item.modelId, {
            abilities: ["TOOL"],
            tools: [],
            inputModalities: ["TEXT"],
            outputModalities: ["TEXT"],
          });
          nextModels.push(
            buildDefaultModel(
              {
                modelId: item.modelId,
                displayName: item.displayName,
                type: item.type,
              },
              template,
            ),
          );
          settingsDraft.modelLibrary = syncModelAbilitiesToLibrary(
            readModelLibrary(settingsDraft),
            normalizeModelRef(item.modelId),
            template.abilities,
            template.tools,
            normalizeModalityList(template.inputModalities),
            normalizeModalityList(template.outputModalities),
          );
        });

        nextProviders[providerIndex] = { ...target, models: nextModels };
        settingsDraft.providers = nextProviders;
      });

      setFetchState(providerId, (prev) => {
        const selected = { ...prev.selected };
        importable.forEach((item) => {
          selected[item.modelId] = false;
        });
        return { ...prev, selected };
      });

      await autosaveSettings(next);
      toast.success(`已导入 ${importable.length} 个模型`);
    },
    [autosaveSettings, buildLatestDraft, fetchStates, providers, setFetchState],
  );

  const addModel = React.useCallback(
    (providerIndex: number) => {
      const provider = providers[providerIndex];
      const currentLength = ensureArray<AnyRecord>(provider?.models).length;
      const model = buildDefaultModel();
      const next = buildLatestDraft((settingsDraft) => {
        const nextProviders = ensureArray<AnyRecord>(settingsDraft.providers);
        const providerDraft = nextProviders[providerIndex];
        if (!providerDraft) return;

        const nextModels = ensureArray<AnyRecord>(providerDraft.models);
        nextModels.push(model);
        nextProviders[providerIndex] = { ...providerDraft, models: nextModels };
        settingsDraft.providers = nextProviders;
      });

      setEditingModelIndex(currentLength);
      setDraftModel(deepClone(model));
      setModelEditorTab("basic");
      setProviderTab("models");
      void autosaveSettings(next);
    },
    [autosaveSettings, buildLatestDraft, providers],
  );

  const deleteModel = React.useCallback(
    async (providerIndex: number, modelIndex: number) => {
      const confirmed = await confirm({
        title: "删除模型？",
        description: "这会从当前提供商移除该模型。",
        confirmText: "删除",
        cancelText: "取消",
        destructive: true,
      });
      if (!confirmed) return;

      const next = buildLatestDraft((settingsDraft) => {
        const nextProviders = ensureArray<AnyRecord>(settingsDraft.providers);
        const provider = nextProviders[providerIndex];
        if (!provider) return;

        const removedModel = ensureArray<AnyRecord>(provider.models)[modelIndex];
        const removedRef = normalizeModelRef(removedModel?.modelId);
        const nextModels = ensureArray<AnyRecord>(provider.models);
        nextModels.splice(modelIndex, 1);
        nextProviders[providerIndex] = { ...provider, models: nextModels };
        settingsDraft.providers = nextProviders;
        settingsDraft.modelLibrary = removeModelLibraryIfUnused(
          readModelLibrary(settingsDraft),
          nextProviders,
          removedRef,
        );
      });

      if (editingModelIndex === modelIndex) {
        setEditingModelIndex(null);
        setDraftModel(null);
      }
      void autosaveSettings(next);
    },
    [autosaveSettings, buildLatestDraft, confirm, editingModelIndex],
  );

  const openModelEditor = React.useCallback(
    (modelIndex: number) => {
      const model = currentModels[modelIndex];
      if (!model) return;

      setDraftModel(deepClone(model));
      setEditingModelIndex(modelIndex);
      setModelEditorTab("basic");
    },
    [currentModels],
  );

  const closeModelEditor = React.useCallback(() => {
    setEditingModelIndex(null);
    setDraftModel(null);
    setModelEditorTab("basic");
  }, []);

  const saveModelEditor = React.useCallback(async () => {
    if (currentProviderIndex < 0 || editingModelIndex === null) {
      closeModelEditor();
      return;
    }

    const sourceModel = currentModels[editingModelIndex] ?? null;
    const model = draftModel ?? sourceModel;
    if (!model) {
      closeModelEditor();
      return;
    }

    if (sourceModel && JSON.stringify(sourceModel) === JSON.stringify(model)) {
      closeModelEditor();
      return;
    }

    const nextModel = deepClone(model);
    const nextRef = normalizeModelRef(nextModel.modelId);
    const sourceRef = normalizeModelRef(sourceModel?.modelId);
    nextModel.abilities = ensureStringValues(nextModel.abilities);
    nextModel.inputModalities = normalizeModalityList(nextModel.inputModalities);
    nextModel.outputModalities = normalizeModalityList(nextModel.outputModalities);
    if (!getBoolean(nextModel.imageGenerationMode, false) || !nextModel.outputModalities.includes("IMAGE")) {
      nextModel.imageGenerationMode = false;
    }
    nextModel.tools = normalizeToolList(nextModel.tools);

    const next = buildLatestDraft((settingsDraft) => {
      const nextProviders = ensureArray<AnyRecord>(settingsDraft.providers);
      const provider = nextProviders[currentProviderIndex];
      if (!provider) return;

      const nextModels = ensureArray<AnyRecord>(provider.models);
      nextModels[editingModelIndex] = nextModel;
      nextProviders[currentProviderIndex] = { ...provider, models: nextModels };
      syncModelRefAcrossProviders(
        settingsDraft,
        nextRef,
        ensureStringValues(nextModel.abilities),
        normalizeToolList(nextModel.tools),
        normalizeModalityList(nextModel.inputModalities),
        normalizeModalityList(nextModel.outputModalities),
      );

      const library = syncModelAbilitiesToLibrary(
        readModelLibrary(settingsDraft),
        nextRef,
        ensureStringValues(nextModel.abilities),
        normalizeToolList(nextModel.tools),
        normalizeModalityList(nextModel.inputModalities),
        normalizeModalityList(nextModel.outputModalities),
      );
      settingsDraft.modelLibrary = removeModelLibraryIfUnused(library, nextProviders, sourceRef);
      settingsDraft.providers = nextProviders;
    });

    closeModelEditor();
    await autosaveSettings(next);
  }, [
    autosaveSettings,
    buildLatestDraft,
    closeModelEditor,
    currentModels,
    currentProviderIndex,
    draftModel,
    editingModelIndex,
  ]);

  const editingModel =
    editingModelIndex === null ? null : (draftModel ?? currentModels[editingModelIndex] ?? null);

  const patchDraftModel = React.useCallback((patch: Partial<AnyRecord>) => {
    setDraftModel((prev) => (prev ? { ...prev, ...patch } : prev));
  }, []);

  const updateDraftModelArrayField = React.useCallback(
    (
      field: "inputModalities" | "outputModalities" | "abilities",
      item: string,
      enabled: boolean,
    ) => {
      setDraftModel((prev) => {
        if (!prev) return prev;
        const currentValues = ensureArray<string>(prev[field]);
        const nextValues = toggleItem(currentValues, item, enabled);
        const nextDraft = { ...prev, [field]: nextValues };
        if (field === "outputModalities" && item === "IMAGE" && !enabled) {
          nextDraft.imageGenerationMode = false;
        }
        return nextDraft;
      });
    },
    [],
  );

  const updateDraftModelTool = React.useCallback(
    (toolType: "search" | "url_context", enabled: boolean) => {
      setDraftModel((prev) => {
        if (!prev) return prev;
        return {
          ...prev,
          tools: upsertModelTool(prev, toolType, enabled),
        };
      });
    },
    [],
  );

  const updateDraftModelEntry = React.useCallback(
    (field: "customHeaders" | "customBodies", index: number, patch: Partial<AnyRecord>) => {
      setDraftModel((prev) => {
        if (!prev) return prev;
        const items = ensureArray<AnyRecord>(prev[field]);
        const item = items[index];
        if (!item) return prev;
        items[index] = { ...item, ...patch };
        return { ...prev, [field]: items };
      });
    },
    [],
  );

  const addDraftModelEntry = React.useCallback((field: "customHeaders" | "customBodies") => {
    setDraftModel((prev) => {
      if (!prev) return prev;
      const items = ensureArray<AnyRecord>(prev[field]);
      items.push(field === "customHeaders" ? { name: "", value: "" } : { key: "", value: "" });
      return { ...prev, [field]: items };
    });
  }, []);

  const removeDraftModelEntry = React.useCallback(
    (field: "customHeaders" | "customBodies", index: number) => {
      setDraftModel((prev) => {
        if (!prev) return prev;
        const items = ensureArray<AnyRecord>(prev[field]);
        items.splice(index, 1);
        return { ...prev, [field]: items };
      });
    },
    [],
  );

  const openTestDialog = React.useCallback(() => {
    if (!currentProviderId || !currentProvider) return;

    const firstChatModel = chatModels[0];
    const selectedModelId = getString(firstChatModel?.id) || getString(firstChatModel?.modelId);
    if (!selectedModelId) {
      toast.error("当前供应商没有可测试的聊天模型");
      return;
    }

    setTestDialog({
      open: true,
      selectedModelId,
      testing: false,
      result: null,
    });
  }, [chatModels, currentProvider, currentProviderId]);

  const runProviderModelTest = React.useCallback(async () => {
    if (!currentProviderId || !draftRef.current || !testDialog.selectedModelId) return;
    if (dirty) {
      toast.error("请先保存配置后再测试");
      return;
    }

    setTestDialog((prev) => ({ ...prev, testing: true, result: null }));
    try {
      const response = await api.post<ProviderModelTestResponseDto>(
        "settings/provider/model/test",
        {
          providerId: currentProviderId,
          modelId: testDialog.selectedModelId,
        },
      );
      setTestDialog((prev) => ({ ...prev, testing: false, result: response }));
      toast.success("测试完成");
    } catch (error) {
      console.error("settings/provider/model/test failed", error);
      setTestDialog((prev) => ({ ...prev, testing: false }));
      toast.error(error instanceof Error ? error.message : "测试失败");
    }
  }, [currentProviderId, dirty, testDialog.selectedModelId]);

  return (
    <div className="flex h-svh flex-col bg-background">
      <div className="flex items-center gap-2 border-b px-4 py-3">
        <Button asChild variant="outline" size="icon-sm" title="返回设置" aria-label="返回设置">
          <Link to="/settings">
            <ChevronLeft className="size-4" />
          </Link>
        </Button>
        <Button asChild variant="outline" size="icon-sm" title="返回聊天" aria-label="返回聊天">
          <Link to="/">
            <Home className="size-4" />
          </Link>
        </Button>

        <div className="min-w-0 flex-1">
          <div className="truncate text-sm font-medium">
            {currentProvider ? getString(currentProvider.name, "供应商详情") : "供应商"}
          </div>
          <div className="truncate text-xs text-muted-foreground">
            {currentProvider
              ? `${providerTypeLabel(currentProviderType)} · ${providerHost(currentProvider.baseUrl)}`
              : "管理提供商卡片、配置项、模型与内置工具"}
          </div>
        </div>
      </div>

      <div className="min-h-0 flex-1">
        <ScrollArea className="h-full">
          <div className="mx-auto w-full max-w-6xl space-y-3 px-4 py-4">
            {draft ? (
              currentProvider ? (
                <>
                  <div className="flex flex-wrap items-center gap-3">
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      onClick={() => {
                        setSelectedProviderId(null);
                        setEditingModelIndex(null);
                      }}
                    >
                      <ChevronLeft className="size-4" />
                      提供商
                    </Button>

                    <AIIcon
                      name={getString(currentProvider.name, "Provider")}
                      size={30}
                      className="rounded-lg"
                    />

                    <div className="min-w-0 flex-1">
                      <div className="flex min-w-0 flex-wrap items-center gap-2">
                        <span className="truncate text-sm font-semibold">
                          {getString(currentProvider.name, "(未命名提供商)")}
                        </span>
                        <Badge variant="outline" className="hidden sm:inline-flex">
                          {providerTypeLabel(currentProviderType)}
                        </Badge>
                        <Badge
                          variant={currentProvider.enabled !== false ? "default" : "outline"}
                          className="hidden sm:inline-flex"
                        >
                          {currentProvider.enabled !== false ? "已启用" : "已停用"}
                        </Badge>
                        {dirty ? (
                          <span className="text-xs text-muted-foreground">
                            {busy ? "正在保存..." : "有未保存修改"}
                          </span>
                        ) : null}
                      </div>
                      <div className="truncate text-xs text-muted-foreground">
                        {providerHost(currentProvider.baseUrl)} · {currentModels.length} 个模型 ·{" "}
                        {currentModels.filter((model) => isChatSelectableModel(model)).length}{" "}
                        个可聊天
                      </div>
                    </div>
                  </div>

                  <div className="grid grid-cols-2 rounded-xl border bg-muted/30 p-1">
                    <button
                      type="button"
                      className={`flex h-10 items-center justify-center gap-2 rounded-lg text-sm transition ${
                        providerTab === "config"
                          ? "bg-background font-medium shadow-sm"
                          : "text-muted-foreground hover:text-foreground"
                      }`}
                      onClick={() => setProviderTab("config")}
                    >
                      <Wrench className="size-4" />
                      配置
                    </button>
                    <button
                      type="button"
                      className={`flex h-10 items-center justify-center gap-2 rounded-lg text-sm transition ${
                        providerTab === "models"
                          ? "bg-background font-medium shadow-sm"
                          : "text-muted-foreground hover:text-foreground"
                      }`}
                      onClick={() => setProviderTab("models")}
                    >
                      <Package className="size-4" />
                      模型
                    </button>
                  </div>

                  {providerTab === "config" ? (
                    <div className="space-y-3">
                      <section className="rounded-lg border bg-card p-3 shadow-sm">
                        <div className="mb-3 flex items-center gap-2 text-sm font-semibold">
                          <Settings2 className="size-4" />
                          基本设置
                        </div>

                        {getString(currentProvider.presetDescription) ? (
                          <div className="mb-3 rounded-md border bg-muted/20 px-3 py-2 text-xs text-muted-foreground">
                            {getString(currentProvider.presetDescription)}
                          </div>
                        ) : null}

                        <div className="grid gap-3 md:grid-cols-2 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_320px]">
                          <div>
                            <div className="mb-1 text-xs font-medium">名称</div>
                            <Input
                              value={getString(currentProvider.name)}
                              onChange={(event) =>
                                updateProvider(currentProviderIndex, { name: event.target.value })
                              }
                              disabled={busy}
                            />
                          </div>

                          <div>
                            <div className="mb-1 text-xs font-medium">类型</div>
                            {currentProvider.builtIn === true ? (
                              <div className="flex h-9 items-center rounded-md border bg-muted/40 px-3 text-sm">
                                {providerTypeLabel(currentProviderType)}
                              </div>
                            ) : (
                              <Select
                                value={currentProviderType}
                                onValueChange={(value) =>
                                  updateProviderType(currentProviderIndex, value as ProviderType)
                                }
                              >
                                <SelectTrigger className="w-full">
                                  <SelectValue />
                                </SelectTrigger>
                                <SelectContent>
                                  {PROVIDER_TYPES.map((item) => (
                                    <SelectItem key={item.value} value={item.value}>
                                      {item.label}
                                    </SelectItem>
                                  ))}
                                </SelectContent>
                              </Select>
                            )}
                          </div>

                          <div className="flex items-end">
                            <div className="flex h-9 w-full items-center justify-between gap-3 rounded-md border bg-muted/20 px-3">
                              <span className="text-xs font-medium">启用</span>
                              <Switch
                                checked={currentProvider.enabled !== false}
                                onCheckedChange={(checked) =>
                                  updateProvider(currentProviderIndex, { enabled: checked })
                                }
                                disabled={busy}
                              />
                            </div>
                          </div>

                          <div className="md:col-span-2 lg:col-span-2">
                            <div className="mb-1 text-xs font-medium">API Base URL</div>
                            <Input
                              value={getString(currentProvider.baseUrl)}
                              onChange={(event) =>
                                updateProvider(currentProviderIndex, {
                                  baseUrl: event.target.value,
                                })
                              }
                              disabled={busy}
                            />
                          </div>

                          <div>
                            <div className="mb-1 text-xs font-medium">API Key</div>
                            <div className="flex gap-2">
                              <Input
                                className="min-w-0 flex-1"
                                type={showSecrets ? "text" : "password"}
                                value={getString(currentProvider.apiKey)}
                                onChange={(event) =>
                                  updateProvider(currentProviderIndex, {
                                    apiKey: event.target.value,
                                  })
                                }
                                disabled={busy}
                                autoComplete="off"
                              />
                              <Button
                                type="button"
                                variant="outline"
                                size="icon"
                                onClick={() => setShowSecrets((value) => !value)}
                                disabled={busy}
                                title={showSecrets ? "隐藏密钥" : "显示密钥"}
                                aria-label={showSecrets ? "隐藏密钥" : "显示密钥"}
                              >
                                {showSecrets ? (
                                  <EyeOff className="size-4" />
                                ) : (
                                  <Eye className="size-4" />
                                )}
                              </Button>
                            </div>
                          </div>

                          {currentProviderType === "openai" ? (
                            <>
                              {getBoolean(currentProvider.useResponseApi) ? null : (
                                <div>
                                  <div className="mb-1 text-xs font-medium">API 路径</div>
                                  <Input
                                    value={getString(
                                      currentProvider.chatCompletionsPath,
                                      "/chat/completions",
                                    )}
                                    onChange={(event) =>
                                      updateProvider(currentProviderIndex, {
                                        chatCompletionsPath: event.target.value,
                                      })
                                    }
                                    disabled={busy || currentProvider.builtIn === true}
                                  />
                                </div>
                              )}

                              <div className="flex items-center justify-between gap-3 rounded-lg border bg-muted/20 px-3 py-2">
                                <div className="min-w-0">
                                  <div className="text-xs font-medium">Response API</div>
                                  <div className="mt-0.5 text-xs text-muted-foreground">
                                    OpenAI 新接口；多数中转服务不支持。
                                  </div>
                                </div>
                                <Switch
                                  checked={getBoolean(currentProvider.useResponseApi)}
                                  onCheckedChange={(checked) =>
                                    updateProvider(currentProviderIndex, {
                                      useResponseApi: checked,
                                    })
                                  }
                                  disabled={busy}
                                />
                              </div>
                            </>
                          ) : null}

                          {currentProviderType === "google" ? (
                            <>
                              <div className="flex items-center justify-between gap-3 rounded-lg border bg-muted/20 px-3 py-2">
                                <div>
                                  <div className="text-xs font-medium">Vertex AI</div>
                                  <div className="mt-0.5 text-xs text-muted-foreground">
                                    开启 Google Vertex AI 模式。
                                  </div>
                                </div>
                                <Switch
                                  checked={getBoolean(currentProvider.vertexAI)}
                                  onCheckedChange={(checked) =>
                                    updateProvider(currentProviderIndex, { vertexAI: checked })
                                  }
                                  disabled={busy}
                                />
                              </div>

                              {getBoolean(currentProvider.vertexAI) ? (
                                <>
                                  <div className="flex items-center justify-between gap-3 rounded-lg border bg-muted/20 px-3 py-2">
                                    <div>
                                      <div className="text-xs font-medium">服务账号认证</div>
                                      <div className="mt-0.5 text-xs text-muted-foreground">
                                        使用 service account 字段。
                                      </div>
                                    </div>
                                    <Switch
                                      checked={getBoolean(currentProvider.useServiceAccount)}
                                      onCheckedChange={(checked) =>
                                        updateProvider(currentProviderIndex, {
                                          useServiceAccount: checked,
                                        })
                                      }
                                      disabled={busy}
                                    />
                                  </div>

                                  <div>
                                    <div className="mb-1 text-xs font-medium">Project ID</div>
                                    <Input
                                      value={getString(currentProvider.projectId)}
                                      onChange={(event) =>
                                        updateProvider(currentProviderIndex, {
                                          projectId: event.target.value,
                                        })
                                      }
                                      disabled={busy}
                                    />
                                  </div>

                                  <div>
                                    <div className="mb-1 text-xs font-medium">Location</div>
                                    <Input
                                      value={getString(currentProvider.location, "us-central1")}
                                      onChange={(event) =>
                                        updateProvider(currentProviderIndex, {
                                          location: event.target.value,
                                        })
                                      }
                                      disabled={busy}
                                    />
                                  </div>

                                  <div className="md:col-span-2">
                                    <div className="mb-1 text-xs font-medium">
                                      Service Account Email
                                    </div>
                                    <Input
                                      value={getString(currentProvider.serviceAccountEmail)}
                                      onChange={(event) =>
                                        updateProvider(currentProviderIndex, {
                                          serviceAccountEmail: event.target.value,
                                        })
                                      }
                                      disabled={busy}
                                    />
                                  </div>

                                  <div className="md:col-span-2">
                                    <div className="mb-1 text-xs font-medium">Private Key</div>
                                    <Input
                                      type={showSecrets ? "text" : "password"}
                                      value={getString(currentProvider.privateKey)}
                                      onChange={(event) =>
                                        updateProvider(currentProviderIndex, {
                                          privateKey: event.target.value,
                                        })
                                      }
                                      disabled={busy}
                                      autoComplete="off"
                                    />
                                  </div>
                                </>
                              ) : null}
                            </>
                          ) : null}

                          {currentProviderType === "claude" ? (
                            <div className="flex items-center justify-between gap-3 rounded-lg border bg-muted/20 px-3 py-2">
                              <div>
                                <div className="text-xs font-medium">Prompt Caching</div>
                                <div className="mt-0.5 text-xs text-muted-foreground">
                                  Claude 提示词缓存。
                                </div>
                              </div>
                              <Switch
                                checked={getBoolean(currentProvider.promptCaching)}
                                onCheckedChange={(checked) =>
                                  updateProvider(currentProviderIndex, { promptCaching: checked })
                                }
                                disabled={busy}
                              />
                            </div>
                          ) : null}
                        </div>
                      </section>

                      <section className="overflow-hidden rounded-lg border bg-card shadow-sm">
                        {(() => {
                          const proxy = readProxy(currentProvider);
                          const enabled = proxy.type !== "none";
                          const proxyLabel =
                            PROXY_TYPES.find((item) => item.value === proxy.type)?.label ??
                            "不使用代理";
                          return (
                            <>
                              <div className="flex items-center justify-between gap-2 px-3 py-2">
                                <button
                                  type="button"
                                  className="flex min-w-0 flex-1 items-center gap-2 text-left"
                                  onClick={() => setProxyCollapsed((value) => !value)}
                                >
                                  <ChevronRight
                                    className={`size-4 shrink-0 transition-transform ${proxyCollapsed ? "" : "rotate-90"}`}
                                  />
                                  <Network className="size-4 shrink-0" />
                                  <span className="truncate text-sm font-semibold">供应商代理</span>
                                  <span className="truncate text-xs text-muted-foreground">
                                    {enabled ? proxyLabel : "直连"}
                                  </span>
                                </button>
                                <Switch
                                  checked={enabled}
                                  onCheckedChange={(checked) =>
                                    updateProviderProxy(currentProviderIndex, {
                                      type: checked ? "http" : "none",
                                    })
                                  }
                                  disabled={busy}
                                />
                              </div>

                              {proxyCollapsed ? null : (
                                <div className="grid gap-3 border-t p-3 md:grid-cols-2">
                                  <div>
                                    <div className="mb-1 text-xs font-medium">代理类型</div>
                                    <Select
                                      value={proxy.type}
                                      onValueChange={(value) =>
                                        updateProviderProxy(currentProviderIndex, {
                                          type: value as ProxyType,
                                        })
                                      }
                                      disabled={busy}
                                    >
                                      <SelectTrigger className="w-full">
                                        <SelectValue />
                                      </SelectTrigger>
                                      <SelectContent>
                                        {PROXY_TYPES.map((item) => (
                                          <SelectItem key={item.value} value={item.value}>
                                            {item.label}
                                          </SelectItem>
                                        ))}
                                      </SelectContent>
                                    </Select>
                                  </div>

                                  {enabled ? (
                                    <>
                                      <div>
                                        <div className="mb-1 text-xs font-medium">代理地址</div>
                                        <Input
                                          value={proxy.address}
                                          onChange={(event) =>
                                            updateProviderProxy(currentProviderIndex, {
                                              address: event.target.value,
                                            })
                                          }
                                          disabled={busy}
                                          placeholder="127.0.0.1"
                                        />
                                      </div>

                                      <div>
                                        <div className="mb-1 text-xs font-medium">代理端口</div>
                                        <Input
                                          value={String(proxy.port || "")}
                                          onChange={(event) => {
                                            const parsed = Number.parseInt(event.target.value, 10);
                                            updateProviderProxy(currentProviderIndex, {
                                              port: Number.isFinite(parsed) ? parsed : 0,
                                            });
                                          }}
                                          disabled={busy}
                                          placeholder="7890"
                                        />
                                      </div>

                                      <div>
                                        <div className="mb-1 text-xs font-medium">用户名</div>
                                        <Input
                                          value={proxy.username}
                                          onChange={(event) =>
                                            updateProviderProxy(currentProviderIndex, {
                                              username: event.target.value,
                                            })
                                          }
                                          disabled={busy}
                                        />
                                      </div>

                                      <div>
                                        <div className="mb-1 text-xs font-medium">密码</div>
                                        <Input
                                          type={showSecrets ? "text" : "password"}
                                          value={proxy.password}
                                          onChange={(event) =>
                                            updateProviderProxy(currentProviderIndex, {
                                              password: event.target.value,
                                            })
                                          }
                                          disabled={busy}
                                          autoComplete="off"
                                        />
                                      </div>
                                    </>
                                  ) : (
                                    <div className="flex items-end text-xs text-muted-foreground">
                                      当前直连，不使用代理。
                                    </div>
                                  )}
                                </div>
                              )}
                            </>
                          );
                        })()}
                      </section>

                      <section className="overflow-hidden rounded-lg border bg-card shadow-sm">
                        {(() => {
                          const balanceOption = readBalanceOption(currentProvider);
                          return (
                            <>
                              <div className="flex items-center justify-between gap-2 px-3 py-2">
                                <button
                                  type="button"
                                  className="flex min-w-0 flex-1 items-center gap-2 text-left"
                                  onClick={() => setBalanceCollapsed((value) => !value)}
                                >
                                  <ChevronRight
                                    className={`size-4 shrink-0 transition-transform ${balanceCollapsed ? "" : "rotate-90"}`}
                                  />
                                  <WalletCards className="size-4 shrink-0" />
                                  <span className="truncate text-sm font-semibold">余额 API</span>
                                  <span className="truncate text-xs text-muted-foreground">
                                    {balanceOption.enabled ? "已启用" : "未启用"}
                                  </span>
                                </button>
                                <Switch
                                  checked={balanceOption.enabled}
                                  onCheckedChange={(checked) =>
                                    updateBalanceOption(currentProviderIndex, { enabled: checked })
                                  }
                                  disabled={busy}
                                />
                              </div>

                              {balanceCollapsed ? null : (
                                <div className="grid gap-3 border-t p-3 md:grid-cols-2">
                                  {balanceOption.enabled ? (
                                    <>
                                      <div>
                                        <div className="mb-1 text-xs font-medium">
                                          余额 API 路径
                                        </div>
                                        <Input
                                          value={balanceOption.apiPath}
                                          onChange={(event) =>
                                            updateBalanceOption(currentProviderIndex, {
                                              apiPath: event.target.value,
                                            })
                                          }
                                          disabled={busy}
                                          placeholder="/credits"
                                        />
                                      </div>

                                      <div>
                                        <div className="mb-1 text-xs font-medium">
                                          结果 JSON 路径
                                        </div>
                                        <Input
                                          value={balanceOption.resultPath}
                                          onChange={(event) =>
                                            updateBalanceOption(currentProviderIndex, {
                                              resultPath: event.target.value,
                                            })
                                          }
                                          disabled={busy}
                                          placeholder="data.total_usage"
                                        />
                                      </div>
                                    </>
                                  ) : (
                                    <div className="text-xs text-muted-foreground">
                                      开启后可配置余额查询接口和结果字段路径。
                                    </div>
                                  )}
                                </div>
                              )}
                            </>
                          );
                        })()}
                      </section>

                      <div className="flex flex-wrap items-center gap-2 rounded-lg border bg-card px-3 py-2 shadow-sm">
                        <Button
                          type="button"
                          variant="outline"
                          size="sm"
                          onClick={openTestDialog}
                          disabled={busy || chatModels.length === 0}
                        >
                          <Network className="size-4" />
                          测试连接
                        </Button>

                        <Button
                          type="button"
                          variant="default"
                          size="sm"
                          onClick={() => void saveCurrentDraft()}
                          disabled={busy || !draft}
                        >
                          <Save className="size-4" />
                          保存
                        </Button>

                        <Button
                          type="button"
                          variant="outline"
                          size="sm"
                          onClick={() => resetProviderBaseUrl(currentProviderIndex)}
                          disabled={busy}
                        >
                          <RotateCcw className="size-4" />
                          重置 Base URL
                        </Button>

                        <Button
                          type="button"
                          variant="destructive"
                          size="sm"
                          onClick={() => void deleteProvider(currentProviderIndex)}
                          disabled={busy}
                        >
                          <Trash2 className="size-4" />
                          删除提供商
                        </Button>

                        <div className="min-w-4 flex-1" />
                      </div>
                    </div>
                  ) : (
                    <div className="min-w-0 space-y-3">
                      <div className="flex w-full min-w-0 flex-wrap items-center gap-2 overflow-hidden rounded-lg border bg-card px-3 py-2 shadow-sm">
                        <div className="flex min-w-0 flex-1 items-center gap-2">
                          <Package className="size-4 shrink-0" />
                          <span className="truncate text-sm font-semibold">模型</span>
                          <span className="truncate text-xs text-muted-foreground">
                            {currentModels.length} 个已添加
                          </span>
                        </div>

                        <Button
                          type="button"
                          variant="outline"
                          size="sm"
                          onClick={() => void fetchProviderModels(currentProviderIndex)}
                          disabled={busy || !currentProviderId || currentFetchState?.loading}
                        >
                          <Download className="size-4" />
                          {currentFetchState?.loading ? "获取中..." : "获取模型"}
                        </Button>

                        <Button
                          type="button"
                          variant="secondary"
                          size="sm"
                          onClick={() => setModelLibraryOpen(true)}
                          disabled={!currentFetchState || currentFetchState.models.length === 0}
                        >
                          模型库 ({currentFetchState?.models.length ?? 0})
                        </Button>

                        <Button
                          type="button"
                          variant="secondary"
                          size="sm"
                          onClick={() => void importFetchedModels(currentProviderIndex)}
                          disabled={busy || !currentFetchState || selectedFetchedCount === 0}
                        >
                          导入已选 ({selectedFetchedCount})
                        </Button>

                        <Button
                          type="button"
                          variant="default"
                          size="sm"
                          onClick={() => addModel(currentProviderIndex)}
                          disabled={busy}
                        >
                          <Plus className="size-4" />
                          添加新模型
                        </Button>
                      </div>

                      {currentFetchState?.error ? (
                        <div className="text-xs text-destructive">{currentFetchState.error}</div>
                      ) : null}

                      {currentModels.length === 0 ? (
                        <div className="rounded-xl border border-dashed p-10 text-center">
                          <Package className="mx-auto size-8 text-muted-foreground" />
                          <div className="mt-3 text-sm font-medium">暂无模型</div>
                          <div className="mt-1 text-xs text-muted-foreground">
                            点击上方按钮获取或添加模型。
                          </div>
                        </div>
                      ) : (
                        <div className="grid min-w-0 gap-3 md:grid-cols-2">
                          {currentModels.map((model, modelIndex) => {
                            const title = getString(
                              model.displayName,
                              getString(model.modelId, "(未命名模型)"),
                            );
                            const type = normalizeModelType(model.type);
                            const inputModalities = ensureArray<string>(model.inputModalities);
                            const outputModalities = ensureArray<string>(model.outputModalities);
                            const abilities = ensureArray<string>(model.abilities);
                            const searchEnabled = hasBuiltInTool(model, "search");
                            const urlContextEnabled = hasBuiltInTool(model, "url_context");
                            const badges = [
                              modelTypeLabel(type),
                              ...inputModalities.map((item) => `入 ${modalityLabel(item)}`),
                              ...outputModalities.map((item) => `出 ${modalityLabel(item)}`),
                              ...abilities.map((item) => abilityLabel(item)),
                              ...(searchEnabled ? ["搜索"] : []),
                              ...(urlContextEnabled ? ["URL Context"] : []),
                            ];

                            return (
                              <div
                                key={
                                  getString(model.id) || `${getString(model.modelId)}-${modelIndex}`
                                }
                                className="min-w-0 overflow-hidden rounded-lg border bg-card p-3 shadow-sm"
                              >
                                <div className="flex items-start gap-2.5">
                                  <AIIcon
                                    name={getString(model.modelId, title)}
                                    size={32}
                                    className="rounded-lg"
                                  />
                                  <div className="min-w-0 flex-1">
                                    <div className="flex min-w-0 items-center gap-2">
                                      <div className="truncate text-sm font-semibold">{title}</div>
                                      <div className="hidden truncate text-xs text-muted-foreground sm:block">
                                        {getString(model.modelId) || getString(model.id)}
                                      </div>
                                    </div>
                                    <div className="mt-1 flex max-h-12 flex-wrap gap-1 overflow-hidden">
                                      {badges.map((badge, badgeIndex) => (
                                        <Badge
                                          key={`${badge}-${badgeIndex}`}
                                          variant={badgeIndex === 0 ? "secondary" : "outline"}
                                          className="px-1.5 py-0 text-[11px]"
                                        >
                                          {badge}
                                        </Badge>
                                      ))}
                                    </div>
                                  </div>

                                  <div className="flex gap-1">
                                    <Button
                                      type="button"
                                      variant="ghost"
                                      size="icon-xs"
                                      onClick={() => openModelEditor(modelIndex)}
                                      title="编辑模型"
                                      aria-label="编辑模型"
                                    >
                                      <Wrench className="size-3.5" />
                                    </Button>
                                    <Button
                                      type="button"
                                      variant="ghost"
                                      size="icon-xs"
                                      onClick={() =>
                                        void deleteModel(currentProviderIndex, modelIndex)
                                      }
                                      disabled={busy}
                                      title="删除模型"
                                      aria-label="删除模型"
                                    >
                                      <Trash2 className="size-3.5" />
                                    </Button>
                                  </div>
                                </div>
                              </div>
                            );
                          })}
                        </div>
                      )}
                    </div>
                  )}

                  <ModelLibraryDialog
                    open={modelLibraryOpen}
                    onOpenChange={setModelLibraryOpen}
                    models={filteredFetchedModels}
                    totalCount={currentFetchState?.models.length ?? 0}
                    selectedCount={selectedFetchedCount}
                    visibleSelectedCount={visibleSelectedFetchedCount}
                    search={modelLibrarySearch}
                    onSearchChange={setModelLibrarySearch}
                    selected={currentFetchState?.selected ?? {}}
                    providerId={currentProviderId}
                    onToggleSelection={toggleFetchedSelection}
                    onToggleVisible={toggleVisibleFetchedModels}
                    onImport={() => void importFetchedModels(currentProviderIndex)}
                    importingDisabled={busy || !currentFetchState || selectedFetchedCount === 0}
                  />

                  <ConnectionTestDialog
                    open={testDialog.open}
                    onOpenChange={(open) => setTestDialog((prev) => ({ ...prev, open }))}
                    models={chatModels}
                    selectedModelId={testDialog.selectedModelId}
                    onModelChange={(selectedModelId) =>
                      setTestDialog((prev) => ({ ...prev, selectedModelId, result: null }))
                    }
                    testing={testDialog.testing}
                    result={testDialog.result}
                    onRun={() => void runProviderModelTest()}
                  />

                  {editingModel && editingModelIndex !== null ? (
                    <Sheet open onOpenChange={(open) => !open && closeModelEditor()}>
                      <SheetContent
                        side="right"
                        className="w-[min(100vw,760px)] gap-0 p-0 sm:max-w-[760px]"
                        showCloseButton={false}
                      >
                        <SheetHeader className="border-b px-4 py-3">
                          <div className="flex items-center justify-between gap-3">
                            <div className="flex min-w-0 items-center gap-3">
                              <AIIcon
                                name={getString(
                                  editingModel.modelId,
                                  getString(editingModel.displayName, "Model"),
                                )}
                                size={36}
                                className="rounded-lg"
                              />
                              <div className="min-w-0">
                                <SheetTitle className="truncate text-base">
                                  {getString(editingModel.displayName, "编辑模型")}
                                </SheetTitle>
                                <SheetDescription className="truncate">
                                  {getString(editingModel.modelId) || "未设置模型 ID"}
                                </SheetDescription>
                              </div>
                            </div>
                            <Button
                              type="button"
                              variant="ghost"
                              size="icon-sm"
                              onClick={closeModelEditor}
                              title="关闭"
                              aria-label="关闭"
                            >
                              <X className="size-4" />
                            </Button>
                          </div>
                        </SheetHeader>

                        <div className="grid grid-cols-3 gap-1 border-b bg-muted/30 p-2">
                          {[
                            { id: "basic", label: "基本设置" },
                            { id: "advanced", label: "高级设置" },
                            { id: "tools", label: "内置工具" },
                          ].map((item) => (
                            <button
                              key={item.id}
                              type="button"
                              className={`h-9 rounded-md text-sm transition ${
                                modelEditorTab === item.id
                                  ? "bg-background font-medium shadow-sm"
                                  : "text-muted-foreground hover:text-foreground"
                              }`}
                              onClick={() => setModelEditorTab(item.id as ModelEditorTab)}
                            >
                              {item.label}
                            </button>
                          ))}
                        </div>

                        <ScrollArea className="min-h-0 flex-1">
                          <div className="space-y-5 p-4">
                            {modelEditorTab === "basic" ? (
                              <>
                                <div className="grid gap-3 md:grid-cols-2">
                                  <div>
                                    <div className="mb-1 text-xs font-medium">模型 ID</div>
                                    <Input
                                      value={getString(editingModel.modelId)}
                                      onChange={(event) =>
                                        patchDraftModel({ modelId: event.target.value })
                                      }
                                      disabled={busy}
                                      placeholder="例如：gpt-4.1"
                                    />
                                  </div>

                                  <div>
                                    <div className="mb-1 text-xs font-medium">模型显示名称</div>
                                    <Input
                                      value={getString(editingModel.displayName)}
                                      onChange={(event) =>
                                        patchDraftModel({ displayName: event.target.value })
                                      }
                                      disabled={busy}
                                      placeholder="例如：GPT-4.1"
                                    />
                                  </div>
                                </div>

                                <div>
                                  <div className="mb-2 text-xs font-medium">模型类型</div>
                                  <div className="grid grid-cols-3 rounded-lg border bg-muted/30 p-1">
                                    {MODEL_TYPES.map((item) => (
                                      <button
                                        key={item.value}
                                        type="button"
                                        className={`h-9 rounded-md text-sm transition ${
                                          normalizeModelType(editingModel.type) === item.value
                                            ? "bg-background font-medium shadow-sm"
                                            : "text-muted-foreground hover:text-foreground"
                                        }`}
                                        onClick={() => patchDraftModel({ type: item.value })}
                                      >
                                        {item.label}
                                      </button>
                                    ))}
                                  </div>
                                </div>

                                <div className="grid gap-4 md:grid-cols-2">
                                  <div className="rounded-xl border p-3">
                                    <div className="text-xs font-medium">输入模态</div>
                                    <div className="mt-3 flex flex-wrap gap-3">
                                      {(["TEXT", "IMAGE"] as ModelModality[]).map((item) => (
                                        <label
                                          key={item}
                                          className="flex cursor-pointer items-center gap-2 text-sm"
                                        >
                                          <Checkbox
                                            checked={ensureArray<string>(
                                              editingModel.inputModalities,
                                            ).includes(item)}
                                            onCheckedChange={(checked) =>
                                              updateDraftModelArrayField(
                                                "inputModalities",
                                                item,
                                                Boolean(checked),
                                              )
                                            }
                                          />
                                          <span>{modalityLabel(item)}</span>
                                        </label>
                                      ))}
                                    </div>
                                  </div>

                                  <div className="rounded-xl border p-3">
                                    <div className="text-xs font-medium">输出模态</div>
                                    <div className="mt-3 flex flex-wrap gap-3">
                                      {(["TEXT", "IMAGE"] as ModelModality[]).map((item) => (
                                        <label
                                          key={item}
                                          className="flex cursor-pointer items-center gap-2 text-sm"
                                        >
                                          <Checkbox
                                            checked={ensureArray<string>(
                                              editingModel.outputModalities,
                                            ).includes(item)}
                                            onCheckedChange={(checked) =>
                                              updateDraftModelArrayField(
                                                "outputModalities",
                                                item,
                                                Boolean(checked),
                                              )
                                            }
                                          />
                                          <span>{modalityLabel(item)}</span>
                                        </label>
                                      ))}
                                    </div>

                                    {ensureArray<string>(editingModel.outputModalities).includes(
                                      "IMAGE",
                                    ) ? (
                                      <div className="mt-4 flex items-center justify-between gap-3 rounded-lg bg-muted/40 px-3 py-2">
                                        <div className="min-w-0">
                                          <div className="text-sm font-medium">生图模式</div>
                                          <div className="mt-0.5 text-xs text-muted-foreground">
                                            聊天输入栏显示新图 / 续图开关
                                          </div>
                                        </div>
                                        <Switch
                                          checked={getBoolean(editingModel.imageGenerationMode)}
                                          onCheckedChange={(checked) =>
                                            patchDraftModel({ imageGenerationMode: checked })
                                          }
                                          disabled={busy}
                                        />
                                      </div>
                                    ) : null}
                                  </div>
                                </div>

                                {normalizeModelType(editingModel.type) === "CHAT" ? (
                                  <div className="rounded-xl border p-3">
                                    <div className="text-xs font-medium">模型能力</div>
                                    <div className="mt-3 flex flex-wrap gap-3">
                                      {(["TOOL", "REASONING"] as ModelAbility[]).map((item) => (
                                        <label
                                          key={item}
                                          className="flex cursor-pointer items-center gap-2 text-sm"
                                        >
                                          <Checkbox
                                            checked={ensureArray<string>(
                                              editingModel.abilities,
                                            ).includes(item)}
                                            onCheckedChange={(checked) =>
                                              updateDraftModelArrayField(
                                                "abilities",
                                                item,
                                                Boolean(checked),
                                              )
                                            }
                                          />
                                          <span>{abilityLabel(item)}</span>
                                        </label>
                                      ))}
                                    </div>
                                  </div>
                                ) : null}
                              </>
                            ) : null}

                            {modelEditorTab === "advanced" ? (
                              <div className="grid gap-4">
                                <section className="rounded-xl border p-4">
                                  <div className="flex items-center justify-between gap-2">
                                    <div>
                                      <div className="text-sm font-semibold">自定义请求头</div>
                                      <div className="mt-1 text-xs text-muted-foreground">
                                        对应原版模型 customHeaders。
                                      </div>
                                    </div>
                                    <Button
                                      type="button"
                                      variant="outline"
                                      size="sm"
                                      onClick={() => addDraftModelEntry("customHeaders")}
                                      disabled={busy}
                                    >
                                      <Plus className="size-4" />
                                      添加
                                    </Button>
                                  </div>

                                  <div className="mt-4 space-y-2">
                                    {ensureArray<AnyRecord>(editingModel.customHeaders).length ===
                                    0 ? (
                                      <div className="rounded-lg border border-dashed p-4 text-center text-xs text-muted-foreground">
                                        未设置自定义请求头。
                                      </div>
                                    ) : null}

                                    {ensureArray<AnyRecord>(editingModel.customHeaders).map(
                                      (item, index) => (
                                        <div
                                          key={index}
                                          className="grid gap-2 rounded-lg border p-3"
                                        >
                                          <Input
                                            value={getString(item.name)}
                                            onChange={(event) =>
                                              updateDraftModelEntry("customHeaders", index, {
                                                name: event.target.value,
                                              })
                                            }
                                            disabled={busy}
                                            placeholder="Header 名称"
                                          />
                                          <div className="flex gap-2">
                                            <Input
                                              value={getString(item.value)}
                                              onChange={(event) =>
                                                updateDraftModelEntry("customHeaders", index, {
                                                  value: event.target.value,
                                                })
                                              }
                                              disabled={busy}
                                              placeholder="Header 值"
                                            />
                                            <Button
                                              type="button"
                                              variant="destructive"
                                              size="icon-sm"
                                              onClick={() =>
                                                removeDraftModelEntry("customHeaders", index)
                                              }
                                              disabled={busy}
                                              title="删除请求头"
                                              aria-label="删除请求头"
                                            >
                                              <Trash2 className="size-4" />
                                            </Button>
                                          </div>
                                        </div>
                                      ),
                                    )}
                                  </div>
                                </section>

                                <section className="rounded-xl border p-4">
                                  <div className="flex items-center justify-between gap-2">
                                    <div>
                                      <div className="text-sm font-semibold">自定义请求体</div>
                                      <div className="mt-1 text-xs text-muted-foreground">
                                        对应原版模型 customBodies。
                                      </div>
                                    </div>
                                    <Button
                                      type="button"
                                      variant="outline"
                                      size="sm"
                                      onClick={() => addDraftModelEntry("customBodies")}
                                      disabled={busy}
                                    >
                                      <Plus className="size-4" />
                                      添加
                                    </Button>
                                  </div>

                                  <div className="mt-4 space-y-2">
                                    {ensureArray<AnyRecord>(editingModel.customBodies).length ===
                                    0 ? (
                                      <div className="rounded-lg border border-dashed p-4 text-center text-xs text-muted-foreground">
                                        未设置自定义请求体字段。
                                      </div>
                                    ) : null}

                                    {ensureArray<AnyRecord>(editingModel.customBodies).map(
                                      (item, index) => (
                                        <div
                                          key={index}
                                          className="grid gap-2 rounded-lg border p-3"
                                        >
                                          <Input
                                            value={getString(item.key)}
                                            onChange={(event) =>
                                              updateDraftModelEntry("customBodies", index, {
                                                key: event.target.value,
                                              })
                                            }
                                            disabled={busy}
                                            placeholder="字段名"
                                          />
                                          <div className="flex gap-2">
                                            <Input
                                              value={getString(item.value)}
                                              onChange={(event) =>
                                                updateDraftModelEntry("customBodies", index, {
                                                  value: event.target.value,
                                                })
                                              }
                                              disabled={busy}
                                              placeholder="字段值"
                                            />
                                            <Button
                                              type="button"
                                              variant="destructive"
                                              size="icon-sm"
                                              onClick={() =>
                                                removeDraftModelEntry("customBodies", index)
                                              }
                                              disabled={busy}
                                              title="删除请求体字段"
                                              aria-label="删除请求体字段"
                                            >
                                              <Trash2 className="size-4" />
                                            </Button>
                                          </div>
                                        </div>
                                      ),
                                    )}
                                  </div>
                                </section>
                              </div>
                            ) : null}

                            {modelEditorTab === "tools" ? (
                              <div className="space-y-3">
                                {[
                                  {
                                    type: "search" as const,
                                    title: "搜索",
                                    description: "允许该模型使用内置联网搜索工具。",
                                  },
                                  {
                                    type: "url_context" as const,
                                    title: "URL Context",
                                    description: "允许该模型读取 URL 上下文工具。",
                                  },
                                ].map((item) => (
                                  <div
                                    key={item.type}
                                    className="flex items-center justify-between gap-4 rounded-xl border p-4"
                                  >
                                    <div className="min-w-0">
                                      <div className="text-sm font-semibold">{item.title}</div>
                                      <div className="mt-1 text-xs text-muted-foreground">
                                        {item.description}
                                      </div>
                                    </div>
                                    <Switch
                                      checked={hasBuiltInTool(editingModel, item.type)}
                                      onCheckedChange={(checked) =>
                                        updateDraftModelTool(item.type, checked)
                                      }
                                      disabled={busy}
                                    />
                                  </div>
                                ))}
                              </div>
                            ) : null}
                          </div>
                        </ScrollArea>

                        <SheetFooter className="border-t px-4 py-3">
                          <Button
                            type="button"
                            variant="outline"
                            onClick={() => void saveModelEditor()}
                          >
                            完成
                          </Button>
                        </SheetFooter>
                      </SheetContent>
                    </Sheet>
                  ) : null}
                </>
              ) : (
                <>
                  <section
                    className={defaultsOnly ? "rounded-xl border bg-card p-4 shadow-sm" : "hidden"}
                  >
                    <div className="flex items-center justify-between gap-2">
                      <div className="flex items-start gap-3">
                        <div className="rounded-lg border bg-muted/40 p-2">
                          <Bot className="size-4" />
                        </div>
                        <div>
                          <div className="text-sm font-semibold">默认模型与提示词</div>
                          <div className="mt-1 text-xs text-muted-foreground">
                            配置自动总结会话标题时使用的模型和提示词。
                          </div>
                        </div>
                      </div>
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon-sm"
                        onClick={() => setDefaultModelsCollapsed((value) => !value)}
                        title={defaultModelsCollapsed ? "展开默认模型" : "折叠默认模型"}
                        aria-label={defaultModelsCollapsed ? "展开默认模型" : "折叠默认模型"}
                      >
                        <ChevronRight
                          className={`size-4 transition-transform ${defaultModelsCollapsed ? "" : "rotate-90"}`}
                        />
                      </Button>
                    </div>

                    {defaultModelsCollapsed ? null : (
                      <div className="mt-4 rounded-xl border p-3">
                        <div className="flex items-center justify-between gap-2">
                          <div className="flex min-w-0 items-start gap-2">
                            <Button
                              type="button"
                              variant="ghost"
                              size="icon-sm"
                              className="mt-0.5 h-6 w-6"
                              onClick={() => setTitleSummaryCollapsed((value) => !value)}
                              title={
                                titleSummaryCollapsed ? "展开标题总结模型" : "折叠标题总结模型"
                              }
                              aria-label={
                                titleSummaryCollapsed ? "展开标题总结模型" : "折叠标题总结模型"
                              }
                            >
                              <ChevronRight
                                className={`size-4 transition-transform ${titleSummaryCollapsed ? "" : "rotate-90"}`}
                              />
                            </Button>
                            <div className="min-w-0">
                              <div className="text-sm font-medium">标题总结模型</div>
                              <div className="mt-1 text-xs text-muted-foreground">
                                用于首次回复后的自动标题，以及重新生成标题操作。
                              </div>
                            </div>
                          </div>
                        </div>

                        {titleSummaryCollapsed ? null : (
                          <>
                            <div className="mt-4 grid gap-3 md:grid-cols-2">
                              <div>
                                <div className="mb-1 text-xs font-medium">模型</div>
                                <Select
                                  value={titleModelSelectValue}
                                  onValueChange={updateTitleModel}
                                >
                                  <SelectTrigger className="w-full">
                                    <SelectValue placeholder="选择模型" />
                                  </SelectTrigger>
                                  <SelectContent>
                                    {titleModelOptions.map((item) => (
                                      <SelectItem key={item.id} value={item.id}>
                                        {item.label}
                                      </SelectItem>
                                    ))}
                                  </SelectContent>
                                </Select>
                              </div>
                            </div>

                            <div className="mt-4">
                              <div className="mb-1 flex items-center justify-between gap-2 text-xs font-medium">
                                <span>标题总结提示词</span>
                                <Button
                                  type="button"
                                  variant="outline"
                                  size="sm"
                                  onClick={resetTitlePrompt}
                                  disabled={busy}
                                >
                                  重置提示词
                                </Button>
                              </div>
                              <Textarea
                                value={titlePromptValue}
                                onChange={(event) => updateTitlePrompt(event.target.value)}
                                rows={8}
                                disabled={busy}
                                className="font-mono text-xs"
                              />
                              <div className="mt-2 text-xs text-muted-foreground">
                                支持占位符：{"{locale}"}、{"{content}"}
                              </div>
                            </div>
                          </>
                        )}
                      </div>
                    )}
                  </section>

                  <section
                    className={defaultsOnly ? "hidden" : "rounded-xl border bg-card p-4 shadow-sm"}
                  >
                    <div className="flex flex-wrap items-center justify-between gap-3">
                      <div>
                        <div className="text-sm font-semibold">提供商</div>
                        <div className="mt-1 text-xs text-muted-foreground">
                          点击卡片进入配置与模型页。
                        </div>
                      </div>

                      <div className="flex flex-wrap items-center gap-2">
                        <Select value={addPresetKey} onValueChange={setAddPresetKey}>
                          <SelectTrigger className="w-48" size="sm">
                            <SelectValue placeholder="选择原版预设" />
                          </SelectTrigger>
                          <SelectContent>
                            {PROVIDER_PRESETS.map((preset) => (
                              <SelectItem key={preset.key} value={preset.key}>
                                {preset.name}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>

                        <Button
                          type="button"
                          variant="secondary"
                          size="sm"
                          onClick={addPresetProvider}
                          disabled={busy || !addPresetKey}
                        >
                          <Plus className="size-4" />
                          添加预设
                        </Button>

                        <Button
                          type="button"
                          variant="outline"
                          size="sm"
                          onClick={completePresetProviders}
                          disabled={busy}
                        >
                          补全原版预设
                        </Button>

                        <Select
                          value={addProviderType}
                          onValueChange={(value) => setAddProviderType(value as ProviderType)}
                        >
                          <SelectTrigger className="w-40" size="sm">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            {PROVIDER_TYPES.map((item) => (
                              <SelectItem key={item.value} value={item.value}>
                                {item.label}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>

                        <Button
                          type="button"
                          variant="secondary"
                          size="sm"
                          onClick={addCustomProvider}
                          disabled={busy}
                        >
                          <Plus className="size-4" />
                          自定义
                        </Button>
                      </div>
                    </div>

                    <div className="mt-4 flex items-center gap-2 rounded-full border bg-background px-3">
                      <Search className="size-4 text-muted-foreground" />
                      <Input
                        value={providerSearch}
                        onChange={(event) => setProviderSearch(event.target.value)}
                        placeholder="搜索提供商"
                        className="h-10 border-0 px-0 shadow-none focus-visible:ring-0"
                      />
                      {providerSearch ? (
                        <Button
                          type="button"
                          variant="ghost"
                          size="icon-xs"
                          onClick={() => setProviderSearch("")}
                          title="清空搜索"
                          aria-label="清空搜索"
                        >
                          <X className="size-3" />
                        </Button>
                      ) : null}
                    </div>
                  </section>

                  {providers.length === 0 ? (
                    <div
                      className={
                        defaultsOnly
                          ? "hidden"
                          : "rounded-xl border border-dashed p-8 text-center text-sm text-muted-foreground"
                      }
                    >
                      还没有配置提供商。
                    </div>
                  ) : null}

                  <div className={defaultsOnly ? "hidden" : "grid gap-3 md:grid-cols-2"}>
                    {filteredProviders.map(({ provider, index }) => {
                      const key = providerKey(provider, index);
                      const type = normalizeProviderType(provider.type);
                      const models = ensureArray<AnyRecord>(provider.models);
                      const enabled = provider.enabled !== false;

                      return (
                        <button
                          key={key}
                          type="button"
                          className="group rounded-xl border bg-card p-4 text-left shadow-sm transition hover:border-primary/40 hover:bg-accent/40 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                          onClick={() => {
                            setSelectedProviderId(key);
                            setProviderTab("config");
                            setEditingModelIndex(null);
                          }}
                        >
                          <div className="flex items-start gap-3">
                            <AIIcon
                              name={getString(provider.name, "Provider")}
                              size={42}
                              className="rounded-xl"
                            />
                            <div className="min-w-0 flex-1">
                              <div className="flex items-center justify-between gap-3">
                                <div className="truncate text-sm font-semibold">
                                  {getString(provider.name, "(未命名提供商)")}
                                </div>
                                <ChevronRight className="size-4 shrink-0 text-muted-foreground transition group-hover:translate-x-0.5" />
                              </div>

                              <div className="mt-1 truncate text-xs text-muted-foreground">
                                {providerHost(provider.baseUrl)}
                              </div>

                              <div className="mt-3 flex flex-wrap gap-1.5">
                                <Badge variant={enabled ? "default" : "outline"}>
                                  {enabled ? "已启用" : "已停用"}
                                </Badge>
                                <Badge variant="secondary">{providerTypeLabel(type)}</Badge>
                                <Badge variant="outline">{models.length} 个模型</Badge>
                                {provider.builtIn ? <Badge variant="outline">内置</Badge> : null}
                              </div>
                            </div>
                          </div>

                          {getString(provider.presetDescription) ? (
                            <div className="mt-3 line-clamp-2 text-xs text-muted-foreground">
                              {getString(provider.presetDescription)}
                            </div>
                          ) : null}
                        </button>
                      );
                    })}
                  </div>

                  {filteredProviders.length === 0 && providers.length > 0 ? (
                    <div
                      className={
                        defaultsOnly
                          ? "hidden"
                          : "rounded-xl border border-dashed p-8 text-center text-sm text-muted-foreground"
                      }
                    >
                      没有匹配的提供商。
                    </div>
                  ) : null}

                  {dirty ? (
                    <div className={defaultsOnly ? "hidden" : "text-xs text-muted-foreground"}>
                      {busy ? "正在保存..." : "有未保存修改"}
                    </div>
                  ) : null}
                </>
              )
            ) : (
              <div className="rounded-lg border border-dashed p-8 text-center text-sm text-muted-foreground">
                设置加载中...
              </div>
            )}
          </div>
        </ScrollArea>
      </div>
    </div>
  );
}

function ModelLibraryDialog({
  open,
  onOpenChange,
  models,
  totalCount,
  selectedCount,
  visibleSelectedCount,
  search,
  onSearchChange,
  selected,
  providerId,
  onToggleSelection,
  onToggleVisible,
  onImport,
  importingDisabled,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  models: ProviderModelFetchDto[];
  totalCount: number;
  selectedCount: number;
  visibleSelectedCount: number;
  search: string;
  onSearchChange: (value: string) => void;
  selected: Record<string, boolean>;
  providerId: string;
  onToggleSelection: (providerId: string, modelId: string, checked: boolean) => void;
  onToggleVisible: () => void;
  onImport: () => void;
  importingDisabled: boolean;
}) {
  const selectAllVisible = models.length > 0 && visibleSelectedCount === models.length;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[min(760px,calc(100dvh-2rem))] min-w-0 flex-col gap-0 overflow-hidden p-0 sm:max-w-2xl">
        <DialogHeader className="border-b px-4 py-3">
          <div className="flex min-w-0 items-center justify-between gap-3">
            <div className="min-w-0">
              <DialogTitle>可用模型</DialogTitle>
              <DialogDescription className="truncate">
                共 {totalCount} 个模型，已选择 {selectedCount} 个
              </DialogDescription>
            </div>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={onToggleVisible}
              disabled={models.length === 0}
            >
              {selectAllVisible ? "取消当前" : `选择当前 (${models.length})`}
            </Button>
          </div>
        </DialogHeader>

        <div className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden p-3">
          {models.length === 0 ? (
            <div className="rounded-lg border border-dashed p-8 text-center text-sm text-muted-foreground">
              没有匹配的模型
            </div>
          ) : (
            <div className="space-y-2">
              {models.map((model) => (
                <label
                  key={model.modelId}
                  className="flex w-full min-w-0 cursor-pointer items-center gap-2 rounded-lg border px-2 py-2 text-sm hover:bg-muted/50"
                >
                  <Checkbox
                    checked={Boolean(selected[model.modelId])}
                    onCheckedChange={(checked) =>
                      onToggleSelection(providerId, model.modelId, Boolean(checked))
                    }
                    disabled={!providerId}
                  />
                  <AIIcon name={model.modelId} size={30} className="shrink-0 rounded-md" />
                  <div className="min-w-0 flex-1 overflow-hidden">
                    <div className="truncate font-medium">{model.displayName}</div>
                    <div className="truncate text-xs text-muted-foreground">{model.modelId}</div>
                  </div>
                  <Badge variant="outline" className="shrink-0">
                    {modelTypeLabel(normalizeModelType(model.type))}
                  </Badge>
                </label>
              ))}
            </div>
          )}
        </div>

        <div className="border-t p-3">
          <div className="relative">
            <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              value={search}
              onChange={(event) => onSearchChange(event.target.value)}
              placeholder="筛选模型名称、ID 或类型"
              className="pl-9"
            />
          </div>
        </div>

        <DialogFooter className="border-t px-4 py-3">
          <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
            关闭
          </Button>
          <Button type="button" onClick={onImport} disabled={importingDisabled}>
            导入已选 ({selectedCount})
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function ConnectionTestDialog({
  open,
  onOpenChange,
  models,
  selectedModelId,
  onModelChange,
  testing,
  result,
  onRun,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  models: AnyRecord[];
  selectedModelId: string;
  onModelChange: (value: string) => void;
  testing: boolean;
  result: ProviderModelTestResponseDto | null;
  onRun: () => void;
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[min(720px,calc(100dvh-2rem))] overflow-y-auto sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>测试连接</DialogTitle>
          <DialogDescription>
            选择当前供应商下的聊天模型，依次测试非流式、流式和工具调用。
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          <div>
            <div className="mb-1 text-xs font-medium">测试模型</div>
            <Select
              value={selectedModelId}
              onValueChange={onModelChange}
              disabled={testing || models.length === 0}
            >
              <SelectTrigger className="w-full">
                <SelectValue placeholder="选择模型" />
              </SelectTrigger>
              <SelectContent>
                {models.map((model) => {
                  const id = getString(model.id) || getString(model.modelId);
                  const displayName = getString(model.displayName, id);
                  const modelId = getString(model.modelId);
                  return (
                    <SelectItem key={id} value={id}>
                      {displayName}
                      {modelId && modelId !== displayName ? ` · ${modelId}` : ""}
                    </SelectItem>
                  );
                })}
              </SelectContent>
            </Select>
          </div>

          <div className="space-y-2 rounded-xl border bg-muted/20 p-3">
            <TestResultItem
              label="非流式"
              loading={testing && !result}
              item={result?.nonStreaming ?? null}
            />
            <TestResultItem
              label="流式"
              loading={testing && !result}
              item={result?.streaming ?? null}
            />
            <TestResultItem
              label="工具调用"
              loading={testing && !result}
              item={result?.toolCall ?? null}
            />
          </div>
        </div>

        <DialogFooter>
          <Button
            type="button"
            variant="outline"
            onClick={() => onOpenChange(false)}
            disabled={testing}
          >
            取消
          </Button>
          <Button type="button" onClick={onRun} disabled={testing || !selectedModelId}>
            {testing ? (
              <LoaderCircle className="size-4 animate-spin" />
            ) : (
              <Network className="size-4" />
            )}
            测试
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function TestResultItem({
  label,
  loading,
  item,
}: {
  label: string;
  loading: boolean;
  item: ModelTestItem | null;
}) {
  const success = item?.status === "success";
  const failure = item?.status === "error";
  const skipped = item?.status === "skipped";
  const text = success ? item?.output : failure ? item?.error : (item?.output ?? "");

  return (
    <div className="grid gap-2 rounded-lg bg-background px-3 py-2 text-sm sm:grid-cols-[72px_minmax(0,1fr)]">
      <div className="text-xs font-medium text-muted-foreground">{label}</div>
      <div className="min-w-0">
        {loading ? (
          <div className="flex items-center gap-2 text-xs text-muted-foreground">
            <LoaderCircle className="size-3.5 animate-spin" />
            测试中...
          </div>
        ) : success ? (
          <div className="space-y-1">
            <div className="flex items-center gap-1.5 text-xs text-emerald-600 dark:text-emerald-400">
              <CircleCheck className="size-3.5" />
              成功
            </div>
            {text ? (
              <div className="line-clamp-3 break-words text-xs text-muted-foreground">{text}</div>
            ) : null}
          </div>
        ) : failure ? (
          <div className="space-y-1">
            <div className="flex items-center gap-1.5 text-xs text-destructive">
              <XCircle className="size-3.5" />
              失败
            </div>
            {text ? (
              <div className="max-h-24 overflow-y-auto break-words text-xs text-destructive/90">
                {text}
              </div>
            ) : null}
          </div>
        ) : skipped ? (
          <div className="space-y-1">
            <div className="text-xs text-muted-foreground">已跳过</div>
            {text ? <div className="break-words text-xs text-muted-foreground">{text}</div> : null}
          </div>
        ) : (
          <div className="text-xs text-muted-foreground">—</div>
        )}
      </div>
    </div>
  );
}
