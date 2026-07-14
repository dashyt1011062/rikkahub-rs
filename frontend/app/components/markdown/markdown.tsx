import * as React from "react";

import { cn } from "~/lib/utils";

export type MarkdownProps = {
  content: string;
  className?: string;
  onClickCitation?: (id: string) => void;
  allowCodePreview?: boolean;
};

const RichMarkdown = React.lazy(() => import("./rich-markdown"));

export default function Markdown(props: MarkdownProps) {
  return (
    <React.Suspense
      fallback={
        <div className={cn("whitespace-pre-wrap break-words leading-7", props.className)}>
          {props.content}
        </div>
      }
    >
      <RichMarkdown {...props} />
    </React.Suspense>
  );
}
