import { appendWebAuthQuery } from "~/services/api";
import type { MessagePartMetadata } from "~/types";

/**
 * Convert file URL to the correct API endpoint
 * - data: URLs are returned as-is (base64 encoded files)
 * - http/https URLs are returned as-is (external files)
 * - file:// URLs are extracted to relative paths and converted to /api/files/path/{path}
 * - Relative paths are converted to /api/files/path/{path}
 */
export function resolveFileUrl(url: string): string {
  if (url.startsWith("data:")) {
    return url;
  }
  if (url.startsWith("http://") || url.startsWith("https://")) {
    return url;
  }

  // Handle file:// protocol URLs from Android
  if (url.startsWith("file://")) {
    // Extract path after /files/
    // Format: file:///data/user/0/package.name/files/upload/xxx
    const match = url.match(/file:\/\/.*?\/files\/(.+)/);
    if (match && match[1]) {
      return appendWebAuthQuery(`/api/files/path/${match[1]}`);
    }
    // If we can't extract the path, return as-is (will fail to load with error)
    return url;
  }

  // Relative path - convert to API endpoint
  // Remove leading slash if present
  const path = url.startsWith("/") ? url.slice(1) : url;
  return appendWebAuthQuery(`/api/files/path/${path}`);
}

export function getMessagePartFileId(metadata?: MessagePartMetadata | null): number | null {
  const value = metadata?.fileId;
  if (typeof value === "number" && Number.isFinite(value) && value > 0) {
    return Math.trunc(value);
  }
  if (typeof value === "string" && value.trim().length > 0) {
    const parsed = Number.parseInt(value, 10);
    return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
  }
  return null;
}

export function resolveManagedFileUrl(
  url: string,
  metadata?: MessagePartMetadata | null,
): string {
  const fileId = getMessagePartFileId(metadata);
  if (fileId != null) {
    return appendWebAuthQuery(`/api/files/id/${fileId}?proxy=1`);
  }
  return resolveFileUrl(url);
}
