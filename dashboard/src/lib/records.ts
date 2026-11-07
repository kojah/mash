import { read } from "./api";

export const kinds = ["decision", "preference", "fact", "open_question"] as const;
export const states = [
  "proposed",
  "accepted",
  "active",
  "current",
  "open",
  "resolved",
  "dropped",
  "superseded",
  "withdrawn",
  "retracted",
] as const;
export type Kind = (typeof kinds)[number];
export type Lifecycle = (typeof states)[number];
export type Scope =
  | { kind: "personal" }
  | { kind: "repository"; identity: string }
  | { kind: "local_project"; path: string }
  | { kind: "chat"; id: string };
export interface Evidence {
  chatId: string;
  messageId: string;
  quote: string;
  purpose: "context" | "endorsement" | "report" | null;
  sourceRole: string | null;
}
export type Details =
  | {
      kind: "decision";
      state: "proposed" | "accepted" | "superseded" | "withdrawn";
      alternatives: readonly string[];
      consequences: readonly string[];
    }
  | {
      kind: "preference";
      state: "active" | "superseded" | "withdrawn";
      conditions: readonly string[];
    }
  | {
      kind: "fact";
      state: "current" | "superseded" | "retracted";
      reportedBy: string;
      validUntil: number | null;
    }
  | { kind: "open_question"; state: "open" | "resolved" | "dropped"; answer: string | null };
export interface Revision {
  revision: number;
  statement: string;
  rationale: string;
  review: "candidate" | "established";
  recordedBy: string;
  recordedAt: number;
  effectiveAt: number | null;
  details: Details;
  evidence: readonly Evidence[];
  evidenceCurrent: boolean;
  confirmations: number;
}
export interface NativeRecord {
  id: string;
  owner: string;
  node: string;
  scope: Scope;
  currentRevision: number;
  revisions: readonly Revision[];
  conflicted: boolean;
  links: readonly {
    sourceId: string;
    targetId: string;
    relation: "supersedes" | "resolves" | "supports" | "contradicts";
  }[];
  nextBeforeRevision: number | null;
  nextLinkOffset: number | null;
}
export function timingLabel(revision: Revision, now: number): string | null {
  if (revision.effectiveAt !== null && revision.effectiveAt > now) return "Not yet effective";
  if (
    revision.details.kind === "fact" &&
    revision.details.validUntil !== null &&
    revision.details.validUntil < now
  )
    return "Expired";
  return null;
}
export interface RecordPage {
  records: readonly NativeRecord[];
  nextOffset: number | null;
  nextCursor: { node: string; offset: number } | null;
}
function invalid(): never {
  throw new Error("Mash returned invalid record data. Reload after updating the dashboard.");
}
function object(value: unknown): Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : invalid();
}
function text(value: unknown): string {
  return typeof value === "string" ? value : invalid();
}
function integer(value: unknown): number {
  return typeof value === "number" && Number.isSafeInteger(value) ? value : invalid();
}
function count(value: unknown): number {
  const result = integer(value);
  return result >= 0 ? result : invalid();
}
function optionalNumber(value: unknown): number | null {
  return value === null ? null : count(value);
}
function instant(value: unknown): number | null {
  return value === null ? null : integer(value);
}
function boolean(value: unknown): boolean {
  return typeof value === "boolean" ? value : invalid();
}
function array(value: unknown): unknown[] {
  return Array.isArray(value) ? value : invalid();
}
function strings(value: unknown): string[] {
  return array(value).map(text);
}
function member<const T extends readonly string[]>(value: unknown, values: T): T[number] {
  const result = text(value);
  return values.includes(result) ? (result as T[number]) : invalid();
}
function scope(value: unknown): Scope {
  const row = object(value);
  switch (row.kind) {
    case "personal":
      return { kind: row.kind };
    case "repository":
      return { kind: row.kind, identity: text(row.identity) };
    case "local_project":
      return { kind: row.kind, path: text(row.path) };
    case "chat":
      return { kind: row.kind, id: text(row.id) };
    default:
      return invalid();
  }
}
function details(value: unknown): Details {
  const row = object(value);
  switch (row.kind) {
    case "decision":
      return {
        kind: row.kind,
        state: member(row.state, ["proposed", "accepted", "superseded", "withdrawn"]),
        alternatives: strings(row.alternatives),
        consequences: strings(row.consequences),
      };
    case "preference":
      return {
        kind: row.kind,
        state: member(row.state, ["active", "superseded", "withdrawn"]),
        conditions: strings(row.conditions),
      };
    case "fact":
      return {
        kind: row.kind,
        state: member(row.state, ["current", "superseded", "retracted"]),
        reportedBy: text(row.reported_by),
        validUntil: instant(row.valid_until),
      };
    case "open_question":
      return {
        kind: row.kind,
        state: member(row.state, ["open", "resolved", "dropped"]),
        answer: row.answer === null ? null : text(row.answer),
      };
    default:
      return invalid();
  }
}
function revision(value: unknown): Revision {
  const row = object(value),
    body = object(row.body);
  const number = count(row.revision);
  if (number === 0) return invalid();
  return {
    revision: number,
    statement: text(body.statement),
    rationale: text(body.rationale),
    review: member(body.review, ["candidate", "established"]),
    recordedBy: text(body.recorded_by),
    recordedAt: integer(row.recorded_at),
    effectiveAt: instant(body.effective_at),
    details: details(body.details),
    evidenceCurrent: boolean(row.evidence_current),
    confirmations: count(row.confirmations),
    evidence: array(row.evidence).map((value) => {
      const source = object(value);
      return {
        chatId: text(source.chat_id),
        messageId: text(source.message_id),
        quote: text(source.quote),
        purpose:
          source.purpose === null
            ? null
            : member(source.purpose, ["context", "endorsement", "report"]),
        sourceRole: source.source_role === null ? null : text(source.source_role),
      };
    }),
  };
}
export function parseRecord(value: unknown): NativeRecord {
  const row = object(value);
  const currentRevision = count(row.current_revision);
  if (currentRevision === 0) return invalid();
  return {
    id: text(row.id),
    owner: text(row.owner),
    node: row.node_id == null ? "" : text(row.node_id),
    scope: scope(row.scope),
    currentRevision,
    revisions: array(row.revisions).map(revision),
    conflicted: boolean(row.conflicted),
    nextBeforeRevision: optionalNumber(row.next_before_revision),
    nextLinkOffset: optionalNumber(row.next_link_offset),
    links: array(row.links).map((value) => {
      const link = object(value);
      return {
        sourceId: text(link.source_id),
        targetId: text(link.target_id),
        relation: member(link.relation, ["supersedes", "resolves", "supports", "contradicts"]),
      };
    }),
  };
}
export function parseRecordPage(value: unknown): RecordPage {
  const row = object(value),
    cursor = row.next_cursor == null ? null : object(row.next_cursor);
  const records = array(row.records).map(parseRecord);
  if (
    records.some(
      (record) =>
        !record.revisions.some((revision) => revision.revision === record.currentRevision),
    )
  )
    return invalid();
  return {
    records,
    nextOffset: optionalNumber(row.next_offset),
    nextCursor: cursor ? { node: text(cursor.node_id), offset: count(cursor.offset) } : null,
  };
}
export async function recordPage(params: string, signal: AbortSignal): Promise<RecordPage> {
  return parseRecordPage(await read("/api/records", new URLSearchParams(params), signal));
}
export async function getRecord(
  id: string,
  node: string,
  before: number | null,
  linkOffset: number,
  signal: AbortSignal,
): Promise<NativeRecord> {
  const params = new URLSearchParams({ id, link_offset: String(linkOffset) });
  if (node) params.set("node", node);
  if (before !== null) params.set("before_revision", String(before));
  const record = parseRecord(await read("/api/record", params, signal));
  if (
    before === null &&
    !record.revisions.some((revision) => revision.revision === record.currentRevision)
  )
    return invalid();
  return record;
}
export function label(value: string): string {
  return value.replaceAll("_", " ").replace(/^./, (letter) => letter.toUpperCase());
}
export function scopeLabel(value: Scope): string {
  switch (value.kind) {
    case "personal":
      return "Personal";
    case "repository":
      return value.identity;
    case "local_project":
      return value.path;
    case "chat":
      return `Chat ${value.id}`;
  }
}
