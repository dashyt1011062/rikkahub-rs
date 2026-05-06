import * as React from "react";

import { cn } from "~/lib/utils";

const MarkdownRenderer = React.lazy(() => import("./markdown-renderer"));

type MarkdownProps = {
  content: string;
  className?: string;
  onClickCitation?: (id: string) => void;
  allowCodePreview?: boolean;
};

function MarkdownFallback({ content, className }: Pick<MarkdownProps, "content" | "className">) {
  return (
    <div className={cn("markdown whitespace-pre-wrap break-words", className)}>{content}</div>
  );
}

export default function Markdown(props: MarkdownProps) {
  return (
    <React.Suspense
      fallback={<MarkdownFallback content={props.content} className={props.className} />}
    >
      <MarkdownRenderer {...props} />
    </React.Suspense>
  );
}
