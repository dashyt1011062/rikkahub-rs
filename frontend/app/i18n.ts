import i18n from "i18next";
import { initReactI18next } from "react-i18next";

import enUSCommon from "./locales/en-US/common.json";
import enUSInput from "./locales/en-US/input.json";
import enUSMarkdown from "./locales/en-US/markdown.json";
import enUSMessage from "./locales/en-US/message.json";
import enUSPage from "./locales/en-US/page.json";
import enUSSettings from "./locales/en-US/settings.json";
import zhCNCommon from "./locales/zh-CN/common.json";
import zhCNInput from "./locales/zh-CN/input.json";
import zhCNMarkdown from "./locales/zh-CN/markdown.json";
import zhCNMessage from "./locales/zh-CN/message.json";
import zhCNPage from "./locales/zh-CN/page.json";
import zhCNSettings from "./locales/zh-CN/settings.json";

const SUPPORTED_LANGUAGES = ["zh-CN", "en-US"] as const;

function getInitialLanguage(): (typeof SUPPORTED_LANGUAGES)[number] {
  if (typeof window === "undefined") {
    return "zh-CN";
  }

  const fromStorage = window.localStorage.getItem("lang");
  if (fromStorage === "zh-CN" || fromStorage === "en-US") {
    return fromStorage;
  }

  const browserLanguage = window.navigator.language;
  return browserLanguage.startsWith("zh") ? "zh-CN" : "en-US";
}

void i18n.use(initReactI18next).init({
  resources: {
    "zh-CN": {
      common: zhCNCommon,
      input: zhCNInput,
      markdown: zhCNMarkdown,
      message: zhCNMessage,
      page: zhCNPage,
      settings: zhCNSettings,
    },
    "en-US": {
      common: enUSCommon,
      input: enUSInput,
      markdown: enUSMarkdown,
      message: enUSMessage,
      page: enUSPage,
      settings: enUSSettings,
    },
  },
  lng: getInitialLanguage(),
  fallbackLng: "zh-CN",
  supportedLngs: [...SUPPORTED_LANGUAGES],
  defaultNS: "common",
  ns: ["common", "input", "markdown", "message", "page", "settings"],
  interpolation: {
    escapeValue: false,
  },
});

type TranslateFn = (keys: unknown, options?: unknown, lastKey?: unknown) => unknown;

const TRANSLATION_CACHE_LIMIT = 5000;
const translationCache = new Map<string, string>();

function isPrimitiveOption(value: unknown): boolean {
  if (value == null) return true;
  const type = typeof value;
  if (type === "string" || type === "number" || type === "boolean") return true;
  return Array.isArray(value) && value.every((item) => typeof item === "string");
}

function isCacheableOptions(options: unknown): boolean {
  if (options == null || typeof options !== "object") return isPrimitiveOption(options);
  return Object.values(options).every(isPrimitiveOption);
}

/**
 * Memoizes i18next lookups. Every rendered message calls `t` dozens of times and each call
 * resolves the language hierarchy from scratch, which dominated render time when opening long
 * conversations. Resources are bundled and static, so results only depend on the language, keys
 * and (primitive) options; anything else bypasses the cache.
 */
function installTranslationCache() {
  const translator = (i18n as unknown as { translator?: { translate: TranslateFn } }).translator;
  if (!translator) return;
  const translate = translator.translate.bind(translator);
  translator.translate = (keys, options, lastKey) => {
    if (lastKey !== undefined || !isPrimitiveOption(keys) || !isCacheableOptions(options)) {
      return translate(keys, options, lastKey);
    }
    const cacheKey = JSON.stringify([i18n.language, keys, options ?? null]);
    const cached = translationCache.get(cacheKey);
    if (cached !== undefined) return cached;
    const result = translate(keys, options, lastKey);
    if (typeof result === "string") {
      if (translationCache.size >= TRANSLATION_CACHE_LIMIT) translationCache.clear();
      translationCache.set(cacheKey, result);
    }
    return result;
  };
}

installTranslationCache();
i18n.store.on("added", () => translationCache.clear());

function syncDocumentLanguage(language: string) {
  // The html lang drives CJK glyph selection (e.g. Chinese vs Japanese forms of shared characters).
  if (typeof document !== "undefined") {
    document.documentElement.lang = language;
  }
}

syncDocumentLanguage(i18n.language);

void i18n.on("languageChanged", (language) => {
  translationCache.clear();
  syncDocumentLanguage(language);
  if (typeof window !== "undefined") {
    window.localStorage.setItem("lang", language);
  }
});

export default i18n;
