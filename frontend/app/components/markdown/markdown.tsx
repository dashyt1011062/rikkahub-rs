import * as React from "react";

import { cn } from "~/lib/utils";

export type MarkdownProps = {
  content: string;
  className?: string;
  onClickCitation?: (id: string) => void;
  allowCodePreview?: boolean;
};

const loadRichMarkdown = () => import("./rich-markdown");
const RichMarkdown = React.lazy(loadRichMarkdown);

// Warm the renderer chunk once the page is idle, so the first reply after a page load doesn't
// flash as raw markdown (the Suspense fallback) while the chunk is still downloading.
if (typeof window !== "undefined") {
  const preload = () => {
    void loadRichMarkdown();
  };
  if ("requestIdleCallback" in window) {
    window.requestIdleCallback(preload, { timeout: 3000 });
  } else {
    setTimeout(preload, 1500);
  }
}

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
