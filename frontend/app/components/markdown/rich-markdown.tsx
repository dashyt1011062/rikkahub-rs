import * as React from "react";
import { useTranslation } from "react-i18next";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeRaw from "rehype-raw";

import { getCodePreviewLanguage } from "~/components/workbench/code-preview-language";
import { useOptionalWorkbench } from "~/components/workbench/workbench-context";
import { cn } from "~/lib/utils";
import type { MarkdownProps } from "./markdown";

import "./markdown.css";

const LazyCodeBlock = React.lazy(() =>
  import("./code-block").then((module) => ({ default: module.CodeBlock })),
);

const INLINE_LATEX_REGEX = /\\\((.+?)\\\)/g;
const BLOCK_LATEX_REGEX = /\\\[(.+?)\\\]/gs;
const THINKING_REGEX = /<think>([\s\S]*?)(?:<\/think>|$)/g;
const CODE_BLOCK_REGEX = /```[\s\S]*?```|`[^`\n]*`/g;
const MATH_SYNTAX_REGEX = /\\\(|\\\[|(^|[^\\])\$\$?[\s\S]*?\$\$?/m;

type RemarkPlugins = NonNullable<React.ComponentProps<typeof ReactMarkdown>["remarkPlugins"]>;
type RehypePlugins = NonNullable<React.ComponentProps<typeof ReactMarkdown>["rehypePlugins"]>;

type MathPlugins = {
  remark: RemarkPlugins[number];
  rehype: RehypePlugins[number];
};

function preProcess(content: string): string {
  const codeBlocks: { start: number; end: number }[] = [];
  let match;
  const codeBlockRegex = new RegExp(CODE_BLOCK_REGEX.source, "g");
  while ((match = codeBlockRegex.exec(content)) !== null) {
    codeBlocks.push({ start: match.index, end: match.index + match[0].length });
  }

  const isInCodeBlock = (position: number): boolean =>
    codeBlocks.some((range) => position >= range.start && position < range.end);

  let result = content.replace(
    new RegExp(INLINE_LATEX_REGEX.source, "g"),
    (original, group1, offset) => (isInCodeBlock(offset) ? original : `${group1}$`),
  );

  result = result.replace(
    new RegExp(BLOCK_LATEX_REGEX.source, "gs"),
    (original, group1, offset) => (isInCodeBlock(offset) ? original : `$$`),
  );

  return result.replace(THINKING_REGEX, (_, thinkContent) =>
    String(thinkContent)
      .split("\n")
      .filter((line) => line.trim() !== "")
      .map((line) => `>`)
      .join("\n"),
  );
}

export default function RichMarkdown({
  content,
  className,
  onClickCitation,
  allowCodePreview = true,
}: MarkdownProps) {
  const { t } = useTranslation("markdown");
  const workbench = useOptionalWorkbench();
  const processedContent = React.useMemo(() => preProcess(content), [content]);
  const hasMath = React.useMemo(() => MATH_SYNTAX_REGEX.test(processedContent), [processedContent]);
  const [mathPlugins, setMathPlugins] = React.useState<MathPlugins | null>(null);

  React.useEffect(() => {
    if (!hasMath || mathPlugins) return;

    let active = true;
    void Promise.all([
      import("remark-math"),
      import("rehype-katex"),
      import("katex/dist/katex.min.css"),
    ]).then(([remarkModule, rehypeModule]) => {
      if (!active) return;
      setMathPlugins({
        remark: remarkModule.default as RemarkPlugins[number],
        rehype: rehypeModule.default as RehypePlugins[number],
      });
    });

    return () => {
      active = false;
    };
  }, [hasMath, mathPlugins]);

  const remarkPlugins = React.useMemo<RemarkPlugins>(
    () => (mathPlugins ? [remarkGfm, mathPlugins.remark] : [remarkGfm]),
    [mathPlugins],
  );
  const rehypePlugins = React.useMemo<RehypePlugins>(
    () => (mathPlugins ? [mathPlugins.rehype, rehypeRaw] : [rehypeRaw]),
    [mathPlugins],
  );

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
        remarkPlugins={remarkPlugins}
        rehypePlugins={rehypePlugins}
        components={{
          pre: ({ children }) => <>{children}</>,
          code: ({ className: codeClassName, children, ...props }) => {
            const match = /language-([A-Za-z0-9_-]+)/.exec(codeClassName || "");
            const code = String(children).replace(/\n$/, "");
            const isBlock = code.includes("\n");

            if (match || isBlock) {
              const language = match?.[1] || "";
              return (
                <React.Suspense
                  fallback={
                    <div className="my-2 overflow-x-auto rounded-md border bg-muted/30 p-3 font-mono text-xs whitespace-pre">
                      {code}
                    </div>
                  }
                >
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
