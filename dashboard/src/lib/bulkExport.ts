import { projectLabel } from "./browse";
import { collectChat, markdownExport, type ReadPage } from "./export";
import { plainTitle } from "./markdown";
import type { ChatSummary, Message } from "./types";
import { ZipWriter } from "./zip";

export type BulkFormat = "markdown" | "json";

export interface BulkProgress {
  /** Chats read so far. */
  readonly chats: number;
  readonly total: number;
  /** Messages read so far, across those chats. */
  readonly messages: number;
}

/** Identifies a chat across machines; the same id can exist on two of them. */
export function chatKey(chat: ChatSummary): string {
  return JSON.stringify([chat.machine, chat.id]);
}

// Characters no common filesystem accepts in a name, and control characters.
// biome-ignore lint/suspicious/noControlCharactersInRegex: control characters are what this strips.
const UNSAFE = /[\\/:*?"<>|\u0000-\u001f]/g;
function safe(text: string, fallback: string): string {
  const cleaned = text
    .replace(UNSAFE, " ")
    .replace(/\s+/g, " ")
    .trim()
    .replace(/^\.+|\.+$/g, "")
    .trim();
  return cleaned || fallback;
}

/**
 * Where a chat goes in the archive: a folder per project, then the day it was
 * last active and its title. `taken` holds the paths already used, so two
 * chats with the same title on the same day both survive.
 */
export function chatPath(chat: ChatSummary, format: BulkFormat, taken: Set<string>): string {
  const folder = safe(chat.project ? projectLabel(chat.project) : "", "no project");
  const day =
    chat.updatedMs === undefined ? "undated" : new Date(chat.updatedMs).toISOString().slice(0, 10);
  const title = [...safe(plainTitle(chat.title), "chat")].slice(0, 80).join("").trim();
  const extension = format === "markdown" ? "md" : "json";
  const base = `${folder}/${day} ${title}`;
  let path = `${base}.${extension}`;
  for (let copy = 2; taken.has(path.toLowerCase()); copy += 1)
    path = `${base} (${copy}).${extension}`;
  taken.add(path.toLowerCase());
  return path;
}

export function jsonExport(chat: ChatSummary, messages: readonly Message[]): string {
  return `${JSON.stringify(
    {
      id: chat.id,
      title: plainTitle(chat.title),
      agent: chat.agent,
      machine: chat.machine,
      project: chat.project || null,
      updated: chat.updatedMs === undefined ? null : new Date(chat.updatedMs).toISOString(),
      messages: messages.map((message) => ({
        id: message.id,
        ordinal: message.ordinal ?? null,
        role: message.role,
        kind: message.presentation ?? "dialogue",
        at: message.at,
        text: message.blocks
          .map((block) =>
            block.type === "text" ? block.markdown : block.type === "code" ? block.code : "",
          )
          .join("\n\n"),
        // Opaque provider tool data, kept as the text Mash holds.
        tools: message.activity || null,
      })),
    },
    null,
    2,
  )}\n`;
}

const READERS = 4;

/**
 * Reads every chat in full and packs one file per chat into a ZIP archive.
 * Any chat that cannot be read fails the whole export, so a download is never
 * silently missing chats.
 */
export async function exportChats(
  chats: readonly ChatSummary[],
  format: BulkFormat,
  signal: AbortSignal,
  progress: (progress: BulkProgress) => void,
  readPage?: ReadPage,
  stamp = new Date(),
): Promise<Blob> {
  const files: (string | undefined)[] = chats.map(() => undefined);
  let next = 0;
  let done = 0;
  let messages = 0;
  const failure = new AbortController();
  const stop = AbortSignal.any([signal, failure.signal]);
  async function reader(): Promise<void> {
    while (next < chats.length) {
      const index = next;
      next += 1;
      const chat = chats[index];
      if (!chat) return;
      let counted = 0;
      try {
        const rows = await collectChat(
          chat.id,
          chat.machine,
          stop,
          (count) => {
            messages += count - counted;
            counted = count;
            progress({ chats: done, total: chats.length, messages });
          },
          readPage,
        );
        files[index] =
          format === "markdown"
            ? markdownExport(plainTitle(chat.title), chat.machine, rows)
            : jsonExport(chat, rows);
      } catch (error) {
        if (stop.aborted) throw error;
        failure.abort();
        const reason = error instanceof Error ? error.message : "it could not be read";
        throw new Error(
          `Couldn’t read “${plainTitle(chat.title)}” on ${chat.machine}: ${reason} Nothing was saved. Leave that chat out, or try again.`,
        );
      }
      done += 1;
      progress({ chats: done, total: chats.length, messages });
    }
  }
  const outcomes = await Promise.allSettled(
    Array.from({ length: Math.min(READERS, chats.length) }, reader),
  );
  signal.throwIfAborted();
  // The reader that failed first carries the reason; the rest were stopped by it.
  const failed =
    outcomes.find((outcome) => outcome.status === "rejected" && !isAbort(outcome.reason)) ??
    outcomes.find((outcome) => outcome.status === "rejected");
  if (failed?.status === "rejected") throw failed.reason;
  const zip = new ZipWriter(stamp);
  const taken = new Set<string>();
  const encoder = new TextEncoder();
  chats.forEach((chat, index) => {
    zip.add(chatPath(chat, format, taken), encoder.encode(files[index] ?? ""));
    // Release each transcript as soon as its bytes are in the archive.
    files[index] = undefined;
  });
  return zip.finish();
}

function isAbort(reason: unknown): boolean {
  return reason instanceof DOMException && reason.name === "AbortError";
}

export function saveBlob(blob: Blob, name: string): void {
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = name;
  link.click();
  // Let the browser consume the download before releasing its backing data.
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
