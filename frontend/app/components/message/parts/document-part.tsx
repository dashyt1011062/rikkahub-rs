import { File, FileText } from "lucide-react";

import { resolveManagedFileUrl } from "~/lib/files";
import type { MessagePartMetadata } from "~/types";

interface DocumentPartProps {
  url: string;
  fileName: string;
  mime: string;
  metadata?: MessagePartMetadata | null;
}

function getDocumentIcon(mime: string) {
  if (mime === "application/pdf") {
    return <FileText className="h-4 w-4" />;
  }
  if (mime === "application/vnd.openxmlformats-officedocument.wordprocessingml.document") {
    return <FileText className="h-4 w-4" />;
  }
  return <File className="h-4 w-4" />;
}

export function DocumentPart({ url, fileName, mime, metadata }: DocumentPartProps) {
  if (!url) return null;

  const documentUrl = resolveManagedFileUrl(url, metadata);

  return (
    <a
      className="my-2 inline-flex max-w-full items-center gap-2 rounded-full border border-muted bg-card px-3 py-1.5 text-sm hover:bg-muted/40"
      href={documentUrl}
      rel="noreferrer"
      target="_blank"
    >
      {getDocumentIcon(mime)}
      <span className="max-w-[320px] truncate">{fileName}</span>
    </a>
  );
}
