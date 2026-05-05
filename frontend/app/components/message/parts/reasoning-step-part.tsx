import * as React from "react";
import { Sparkles } from "lucide-react";

import Markdown from "~/components/markdown/markdown";
import type { ReasoningPart as UIReasoningPart } from "~/types";
import Think from "~/assets/think.svg?react";

import { ControlledChainOfThoughtStep } from "../chain-of-thought";

interface ReasoningStepPartProps {
  reasoning: UIReasoningPart;
  isFirst?: boolean;
  isLast?: boolean;
}

function formatDuration(createdAt?: string, finishedAt?: string | null): string | null {
  if (!createdAt) return null;

  const start = Date.parse(createdAt);
  if (Number.isNaN(start)) return null;

  const end = finishedAt ? Date.parse(finishedAt) : Date.now();
  if (Number.isNaN(end)) return null;

  const seconds = Math.max((end - start) / 1000, 0);
  if (seconds <= 0) return null;

  return `${seconds.toFixed(1)}s`;
}

export function ReasoningStepPart({ reasoning, isFirst, isLast }: ReasoningStepPartProps) {
  const loading = reasoning.finishedAt == null;
  const [expanded, setExpanded] = React.useState(false);

  const onExpandedChange = (nextExpanded: boolean) => {
    setExpanded(nextExpanded);
  };

  const duration = formatDuration(reasoning.createdAt, reasoning.finishedAt);

  return (
    <ControlledChainOfThoughtStep
      expanded={expanded}
      onExpandedChange={onExpandedChange}
      isFirst={isFirst}
      isLast={isLast}
      icon={
        loading ? (
          <Sparkles className="h-4 w-4 animate-pulse text-primary" />
        ) : (
          <Think className="h-4 w-4 text-primary" />
        )
      }
      label={<span className="text-foreground text-xs font-medium">深度思考</span>}
      extra={
        duration ? <span className="text-muted-foreground text-xs">{duration}</span> : undefined
      }
      contentVisible={expanded}
    >
      <div>
        <Markdown content={reasoning.reasoning} className="text-xs" />
      </div>
    </ControlledChainOfThoughtStep>
  );
}
