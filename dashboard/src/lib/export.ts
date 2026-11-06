import { PAGE_SIZE, transcript } from "./api";
import { markdown } from "./markdown";
import type { Message } from "./types";

export type ExportFormat = "markdown" | "html" | "pdf";

export function messageText(message: Message): string {
  return (
    message.blocks
      .map((block) =>
        block.type === "text" ? block.markdown : block.type === "code" ? block.code : "",
      )
      .join("\n\n") + (message.activity ? `\n\n${message.activity}` : "")
  );
}

/** Reads every page, failing visibly rather than downloading a truncated export. */
export async function collectChat(
  id: string,
  node: string,
  signal: AbortSignal,
  progress: (count: number) => void,
  readPage: typeof transcript = transcript,
): Promise<readonly Message[]> {
  const messages: Message[] = [];
  const seen = new Set<string>();
  let characters = 0;
  for (let offset = 0; offset < 100_000; offset += PAGE_SIZE) {
    signal.throwIfAborted();
    const page = await readPage(id, node, offset, signal);
    if (!page.messages.length) return messages;
    for (const message of page.messages) {
      if (seen.has(message.id))
        throw new Error("Chat changed during export. Try again after indexing finishes.");
      seen.add(message.id);
      characters += messageText(message).length;
      if (characters > 50_000_000)
        throw new Error(
          "This chat exceeds the browser export size limit. No partial file was saved.",
        );
      messages.push(message);
    }
    progress(messages.length);
  }
  throw new Error(
    "This chat exceeds the 100,000-position browser export limit. No partial file was saved.",
  );
}

function escapeHtml(text: string): string {
  return text.replace(
    /[&<>"']/g,
    (character) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[character] ??
      character,
  );
}

function label(message: Message): string {
  const kind = message.presentation;
  return kind === "tool-result"
    ? "Tool output"
    : kind === "tool-call"
      ? message.toolLabel || "Tool call"
      : kind === "reasoning"
        ? "Reasoning"
        : message.role;
}

export function markdownExport(title: string, node: string, messages: readonly Message[]): string {
  return `# ${title.replace(/\s+/g, " ")}\n\nMachine: ${node}\n\n${messages
    .map((message) => {
      const text = messageText(message);
      // A fence longer than any embedded run also preserves tool output verbatim.
      let fenceLength = 3;
      for (const match of text.matchAll(/`+/g))
        fenceLength = Math.max(fenceLength, match[0].length + 1);
      const fence = "`".repeat(fenceLength);
      const body =
        message.presentation && message.presentation !== "dialogue"
          ? `${fence}\n${text}\n${fence}`
          : text;
      return `## ${label(message)} · ${message.at}\n\n${body}`;
    })
    .join("\n\n---\n\n")}\n`;
}

export function htmlExport(title: string, node: string, messages: readonly Message[]): string {
  return `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>${escapeHtml(title)}</title><style>body{max-width:850px;margin:40px auto;padding:0 24px;font:16px/1.6 system-ui;color:#222}header{border-bottom:1px solid #ccc}article{margin:32px 0}h2{font-size:14px;color:#555}pre{white-space:pre-wrap;overflow-wrap:anywhere;background:#f5f5f5;padding:16px}table{border-collapse:collapse}td,th{border:1px solid #ccc;padding:6px}img{display:none}@media print{body{margin:0;max-width:none}pre{white-space:pre-wrap}a{color:inherit}}</style></head><body><header><h1>${escapeHtml(title)}</h1><p>Machine: ${escapeHtml(node)} · ${messages.length} messages</p></header>${messages.map((message) => `<article><h2>${escapeHtml(label(message))} · ${escapeHtml(message.at)}</h2>${message.presentation && message.presentation !== "dialogue" ? `<pre>${escapeHtml(messageText(message))}</pre>` : markdown(messageText(message))}</article>`).join("\n")}</body></html>`;
}

export function downloadChat(content: string, format: "markdown" | "html"): void {
  const url = URL.createObjectURL(
    new Blob([content], {
      type: format === "html" ? "text/html;charset=utf-8" : "text/markdown;charset=utf-8",
    }),
  );
  const link = document.createElement("a");
  link.href = url;
  link.download = `mash-chat.${format === "html" ? "html" : "md"}`;
  link.click();
  // Let the browser consume the download before releasing its backing data.
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
