import * as React from "react";
import { useTranslation } from "react-i18next";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import rehypeKatex from "rehype-katex";
import rehypeRaw from "rehype-raw";

import { cn } from "~/lib/utils";
import { getCodePreviewLanguage } from "~/components/workbench/code-preview-language";
import { useOptionalWorkbench } from "~/components/workbench/workbench-context";
import "katex/dist/katex.min.css";
import "./markdown.css";

const LazyCodeBlock = React.lazy(async () => {
  const module = await import("./code-block");
  return { default: module.CodeBlock };
});

const INLINE_LATEX_REGEX = /\\\((.+?)\\\)/g;
const BLOCK_LATEX_REGEX = /\\\[(.+?)\\\]/gs;
const THINKING_REGEX = /<think>([\s\S]*?)(?:<\/think>|$)/g;
const CODE_BLOCK_REGEX = /```[\s\S]*?```|`[^`\n]*`/g;

function preProcess(content: string): string {
  const codeBlocks: { start: number; end: number }[] = [];
  let match: RegExpExecArray | null;
  const codeBlockRegex = new RegExp(CODE_BLOCK_REGEX.source, "g");
  while ((match = codeBlockRegex.exec(content)) !== null) {
    codeBlocks.push({ start: match.index, end: match.index + match[0].length });
  }

  const isInCodeBlock = (position: number): boolean => {
    return codeBlocks.some((range) => position >= range.start && position < range.end);
  };

  let result = content.replace(
    new RegExp(INLINE_LATEX_REGEX.source, "g"),
    (fullMatch, group1, offset) => {
      if (isInCodeBlock(offset)) {
        return fullMatch;
      }
      return `$${group1}$`;
    },
  );

  result = result.replace(new RegExp(BLOCK_LATEX_REGEX.source, "gs"), (fullMatch, group1, offset) => {
    if (isInCodeBlock(offset)) {
      return fullMatch;
    }
    return `$$${group1}$$`;
  });

  result = result.replace(THINKING_REGEX, (_, thinkContent) => {
    return thinkContent
      .split("\n")
      .filter((line: string) => line.trim() !== "")
      .map((line: string) => `>${line}`)
      .join("\n");
  });

  return result;
}

type MarkdownProps = {
  content: string;
  className?: string;
  onClickCitation?: (id: string) => void;
  allowCodePreview?: boolean;
};

function CodeBlockFallback({
  code,
  className,
}: {
  code: string;
  className?: string;
}) {
  return (
    <pre className={cn("overflow-auto rounded-lg border border-border bg-muted/30 p-3 text-sm", className)}>
      <code className="font-mono whitespace-pre-wrap break-words">{code}</code>
    </pre>
  );
}

export default function MarkdownRenderer({
  content,
  className,
  onClickCitation,
  allowCodePreview = true,
}: MarkdownProps) {
  const { t } = useTranslation("markdown");
  const workbench = useOptionalWorkbench();
  const processedContent = React.useMemo(() => preProcess(content), [content]);
  const handlePreviewCode = React.useCallback(
    (language: string, code: string) => {
      if (!allowCodePreview || !workbench) return;

      const previewLanguage = getCodePreviewLanguage(language);
      if (!previewLanguage) return;

      workbench.openPanel({
        type: "code-preview",
        title: t("markdown.code_preview_title", {
          language: previewLanguage.toUpperCase(),
        }),
        payload: {
          language: previewLanguage,
          code,
        },
      });
    },
    [allowCodePreview, t, workbench],
  );

  return (
    <div className={cn("markdown", className)}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm, remarkMath]}
        rehypePlugins={[rehypeKatex, rehypeRaw]}
        components={{
          pre: ({ children }) => <>{children}</>,
          code: ({ className, children, ...props }) => {
            const match = /language-([A-Za-z0-9_-]+)/.exec(className || "");
            const code = String(children).replace(/\n$/, "");
            const isBlock = code.includes("\n");

            if (match || isBlock) {
              const language = match?.[1] || "";
              return (
                <React.Suspense fallback={<CodeBlockFallback code={code} className={className} />}>
                  <LazyCodeBlock
                    language={language}
                    code={code}
                    onPreview={
                      allowCodePreview && workbench
                        ? () => {
                            handlePreviewCode(language, code);
                          }
                        : undefined
                    }
                  />
                </React.Suspense>
              );
            }

            return (
              <code className="inline-code" {...props}>
                {children}
              </code>
            );
          },
          a: ({ href, children, ...props }) => {
            const childText = typeof children === "string" ? children : "";

            if (childText.startsWith("citation,")) {
              const domain = childText.substring("citation,".length);
              const id = href || "";

              if (id.length === 6) {
                return (
                  <span
                    className="citation-badge"
                    onClick={() => onClickCitation?.(id)}
                    title={domain}
                  >
                    {domain}
                  </span>
                );
              }
            }

            return (
              <a href={href} target="_blank" rel="noopener noreferrer" {...props}>
                {children}
              </a>
            );
          },
        }}
      >
        {processedContent}
      </ReactMarkdown>
    </div>
  );
}
