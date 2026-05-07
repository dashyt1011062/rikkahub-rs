import Markdown from "~/components/markdown/markdown";
import { useChatInputStore } from "~/stores";

interface TextPartProps {
  text: string;
}

export function TextPart({ text }: TextPartProps) {
  const richTextRenderingEnabled = useChatInputStore(
    (state) => state.richTextRenderingEnabled,
  );

  if (!text) return null;
  if (!richTextRenderingEnabled) {
    return <div className="whitespace-pre-wrap break-words leading-7 text-foreground">{text}</div>;
  }

  return <Markdown content={text} />;
}
