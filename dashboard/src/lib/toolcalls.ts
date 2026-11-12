// Tool steps of a transcript, read into one shape for every agent and tool.
// A call and its output are matched by call ID. Nothing here knows a
// particular tool: names, arguments and output come from the structure of the
// stored record. Everything is archived, untrusted text and is only ever
// shown as text.
import { messageText } from "./export";
import type { Message } from "./types";

export interface ToolArgument {
  readonly key: string;
  readonly text: string;
  /** Long or multi-line values are shown in a block of their own. */
  readonly block: boolean;
}
export interface ToolOutput {
  readonly text: string;
  /** Parsed when the output is JSON, so it can be shown indented. */
  readonly json?: unknown;
}
export interface ToolCall {
  /** The message that holds the call, or the output when no call is on the page. */
  readonly id: string;
  /** The message that holds the output, when it is a separate one. */
  readonly resultId?: string;
  readonly name: string;
  /** One line saying what the call was for; empty when the record has none. */
  readonly summary: string;
  readonly at: string;
  readonly status?: { readonly label: string; readonly failed: boolean };
  readonly durationMs?: number;
  readonly server?: string;
  readonly args: readonly ToolArgument[];
  readonly output?: ToolOutput;
  /** The stored record, exactly as archived. */
  readonly raw: string;
}

type Fields = Readonly<Record<string, unknown>>;
function fields(value: unknown): Fields | undefined {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Fields)
    : undefined;
}
function parse(text: string): unknown {
  const trimmed = text.trim();
  if (!/^[[{]/.test(trimmed)) return undefined;
  try {
    return JSON.parse(trimmed);
  } catch {
    return undefined;
  }
}
function text(value: unknown): string | undefined {
  return typeof value === "string" && value ? value : undefined;
}
function oneLine(value: string, limit = 140): string {
  const line = value.replace(/\s+/g, " ").trim();
  return line.length > limit ? `${line.slice(0, limit - 1)}…` : line;
}

/** A record type such as `commandExecution` as words: "command execution". */
function spaced(value: string | undefined): string | undefined {
  return value?.replace(/([a-z])([A-Z])/g, "$1 $2").toLowerCase();
}
const SUMMARY_KEYS = ["description", "title", "summary"] as const;
function argumentsOf(input: unknown): { args: ToolArgument[]; summary: string } {
  const record = fields(input);
  if (!record) {
    const whole = input === undefined || input === null ? "" : stringify(input);
    return { args: whole ? [{ key: "input", text: whole, block: true }] : [], summary: "" };
  }
  const summaryKey = SUMMARY_KEYS.find((key) => text(record[key]));
  const args = Object.entries(record)
    .filter(([key, value]) => key !== summaryKey && value !== null && value !== undefined)
    .map(([key, value]) => {
      const shown = typeof value === "string" ? value : stringify(value);
      return { key, text: shown, block: shown.includes("\n") || shown.length > 72 };
    });
  // Without a description the call is summed up by its first short value,
  // such as a path or a query, not by a long body such as file content.
  const strings = Object.values(record).filter(
    (value): value is string => typeof value === "string" && value !== "",
  );
  const first = strings.find((value) => !value.includes("\n") && value.length <= 160) ?? strings[0];
  const summary = summaryKey ? text(record[summaryKey]) : first;
  // Short values read first; long ones follow in blocks of their own.
  args.sort((a, b) => Number(a.block) - Number(b.block));
  return { args, summary: oneLine(summary ?? "") };
}
function stringify(value: unknown): string {
  return typeof value === "string" ? value : JSON.stringify(value, null, 2);
}

/** A result envelope's text parts joined; anything else as it is. */
function unwrap(value: unknown): unknown {
  const record = fields(value);
  const content = record && Array.isArray(record.content) ? record.content : undefined;
  if (!content) return value;
  const parts = content.map((part) => text(fields(part)?.text));
  return parts.every((part) => part !== undefined) && parts.length ? parts.join("\n") : value;
}
export function toolOutput(value: unknown): ToolOutput | undefined {
  const inner = unwrap(value);
  if (inner === undefined || inner === null || inner === "") return undefined;
  if (typeof inner !== "string") return { text: JSON.stringify(inner, null, 2), json: inner };
  const json = parse(inner);
  return json === undefined ? { text: inner } : { text: inner, json };
}

/** A step whose whole text is one JSON record of the call, as Codex stores
 * them: the call, its arguments, its output and how it went, all together. */
function fromEnvelope(record: Fields): Partial<ToolCall> & { name: string } {
  const input =
    record.arguments ??
    record.input ??
    (text(record.command) ? { command: record.command, cwd: record.cwd } : undefined);
  const { args, summary } = argumentsOf(input);
  const status = text(record.status);
  const exit = typeof record.exitCode === "number" ? record.exitCode : undefined;
  const failed =
    (record.error !== null && record.error !== undefined) ||
    status === "failed" ||
    (exit !== undefined && exit !== 0);
  const output = toolOutput(record.result ?? record.aggregatedOutput ?? record.output);
  const server = text(record.server);
  return {
    name: text(record.tool) ?? text(record.name) ?? spaced(text(record.type)) ?? "Tool",
    summary,
    args,
    ...(status || exit !== undefined
      ? { status: { label: failed && exit ? `exit ${exit}` : (status ?? "finished"), failed } }
      : {}),
    ...(typeof record.durationMs === "number" ? { durationMs: record.durationMs } : {}),
    ...(server ? { server } : {}),
    ...(output ? { output } : {}),
  };
}

/** The tool calls in a run of step messages, in order, each with its output. */
export function toolCalls(messages: readonly Message[]): ToolCall[] {
  const calls: ToolCall[] = [];
  const open = new Map<string, number>();
  for (const message of messages) {
    if (message.presentation !== "tool-call" && message.presentation !== "tool-result") continue;
    const body = messageText({ ...message, activity: "" });
    const raw = messageText(message);
    if (message.presentation === "tool-result") {
      const result = message.tools?.results[0];
      const index = result?.callId === undefined ? undefined : open.get(result.callId);
      const output = toolOutput(body);
      const status = result?.isError === undefined ? undefined : statusOf(result.isError);
      const call = index === undefined ? undefined : calls[index];
      if (call && index !== undefined) {
        calls[index] = {
          ...call,
          resultId: message.id,
          ...(output ? { output } : {}),
          ...(status ? { status } : {}),
          raw: `${call.raw}\n\n${raw}`,
        };
      } else {
        calls.push({
          id: message.id,
          name: "Tool output",
          summary: "",
          at: message.at,
          args: [],
          ...(output ? { output } : {}),
          ...(status ? { status } : {}),
          raw,
        });
      }
      continue;
    }
    const stored = message.tools?.calls ?? [];
    const envelope = fields(parse(body));
    if (envelope && stored.length <= 1) {
      const call = {
        id: message.id,
        at: message.at,
        summary: "",
        args: [],
        raw,
        ...fromEnvelope(envelope),
      };
      calls.push(call);
      const callId = stored[0]?.callId;
      if (callId !== undefined && !call.output) open.set(callId, calls.length - 1);
      continue;
    }
    for (const [position, entry] of stored.entries()) {
      const { args, summary } = argumentsOf(entry.input);
      calls.push({
        // Only the first call of a message carries its anchor.
        id: position === 0 ? message.id : `${message.id}:${position}`,
        name: entry.name ?? "Tool",
        summary,
        at: message.at,
        args,
        raw,
      });
      if (entry.callId !== undefined) open.set(entry.callId, calls.length - 1);
    }
    if (!stored.length) {
      calls.push({
        id: message.id,
        name: message.toolLabel || "Tool",
        summary: "",
        at: message.at,
        args: body ? [{ key: "call", text: body, block: true }] : [],
        raw,
      });
    }
  }
  return calls;
}
function statusOf(isError: boolean): { label: string; failed: boolean } {
  return { label: isError ? "failed" : "finished", failed: isError };
}

/** How long a call took, in the unit a reader would say it in. */
export function durationLabel(ms: number): string {
  if (ms < 1000) return `${Math.round(ms)} ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(ms < 10_000 ? 1 : 0)} s`;
  return `${Math.floor(ms / 60_000)} min ${Math.round((ms % 60_000) / 1000)} s`;
}
