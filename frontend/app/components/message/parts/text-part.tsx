import Markdown from "~/components/markdown/markdown";
import { useThrottledValue } from "~/hooks/use-throttled-value";
import { useChatInputStore } from "~/stores";

interface TextPartProps {
  text: string;
  loading?: boolean;
}

const STREAM_RENDER_INTERVAL_MS = 120;

export function TextPart({ text, loading = false }: TextPartProps) {
  const richTextRenderingEnabled = useChatInputStore(
    (state) => state.richTextRenderingEnabled,
  );
  const streamedText = useThrottledValue(text, STREAM_RENDER_INTERVAL_MS, loading);

  if (!text) return null;
  if (!richTextRenderingEnabled || loading) {
    return (
      <div className="whitespace-pre-wrap break-words leading-7 text-foreground">
        {loading ? streamedText : text}
      </div>
    );
  }

  return <Markdown content={text} />;
}
