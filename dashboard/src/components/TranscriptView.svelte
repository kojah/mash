<script lang="ts">
import Brain from "@lucide/svelte/icons/brain";
import ChevronDown from "@lucide/svelte/icons/chevron-down";
import ChevronLeft from "@lucide/svelte/icons/chevron-left";
import ChevronRight from "@lucide/svelte/icons/chevron-right";
import ChevronUp from "@lucide/svelte/icons/chevron-up";
import ChevronsDown from "@lucide/svelte/icons/chevrons-down";
import ChevronsLeft from "@lucide/svelte/icons/chevrons-left";
import ChevronsRight from "@lucide/svelte/icons/chevrons-right";
import Copy from "@lucide/svelte/icons/copy";
import Download from "@lucide/svelte/icons/download";
import FileCode from "@lucide/svelte/icons/file-code";
import FileText from "@lucide/svelte/icons/file-text";
import Folder from "@lucide/svelte/icons/folder";
import Lightbulb from "@lucide/svelte/icons/lightbulb";
import ListOrdered from "@lucide/svelte/icons/list-ordered";
import PanelLeft from "@lucide/svelte/icons/panel-left";
import Printer from "@lucide/svelte/icons/printer";
import RefreshCw from "@lucide/svelte/icons/refresh-cw";
import Search from "@lucide/svelte/icons/search";
import Terminal from "@lucide/svelte/icons/terminal";
import X from "@lucide/svelte/icons/x";
import { createQuery } from "@tanstack/svelte-query";
import { tick, untrack } from "svelte";
import { context, type MessagePage, PAGE_SIZE, transcript } from "../lib/api";
import { projectLabel, whenLabel } from "../lib/browse";
import {
  collectChat,
  downloadChat,
  type ExportFormat,
  htmlExport,
  markdownExport,
  messageText,
} from "../lib/export";
import { countLabel, type FindMatch, findMatches } from "../lib/find";
import { bringIntoView, clearMatches, paintMatches } from "../lib/findHighlight";
import { concerns, onChange, pageUpdate } from "../lib/live";
import { plainTitle } from "../lib/markdown";
import { catalogOptions, stableRead } from "../lib/queries";
import { reading } from "../lib/reading.svelte";
import { label, recordPage } from "../lib/records";
import { chatHref, projectHref, recordHref, recordsHref } from "../lib/route.svelte";
import { type ToolCall, toolCalls } from "../lib/toolcalls";
import type { Message } from "../lib/types";
import AgentMark from "./AgentMark.svelte";
import MessageBody from "./MessageBody.svelte";
import MetaChips from "./MetaChips.svelte";
import QueryError from "./QueryError.svelte";
import ToolStep from "./ToolStep.svelte";

let {
  id,
  node,
  messageId,
  shared = false,
}: { id: string; node: string; messageId: string; shared?: boolean } = $props();
const catalogue = createQuery(() => catalogOptions);
const machine = $derived(node || "");
const displayMachine = $derived(machine || catalogue.data?.localNode || "Local");
const sessions = $derived(catalogue.data?.chats ?? []);
const chat = $derived(sessions.find((chat) => chat.id === id && chat.machine === displayMachine));
let offset = $state(0);
let anchor = $state(untrack(() => messageId));
let showSessions = $state(true);
// On a phone the list takes the whole screen in place of the transcript when
// the reader opens it, and closing it returns to where they were reading.
const phoneQuery =
  typeof window === "undefined" ? undefined : window.matchMedia("(max-width: 700px)");
let phone = $state(phoneQuery?.matches ?? false);
let sessionsOpen = $state(false);
$effect(() => {
  if (!phoneQuery) return;
  const update = (): void => {
    phone = phoneQuery.matches;
  };
  phoneQuery.addEventListener("change", update);
  return () => phoneQuery.removeEventListener("change", update);
});
const sessionsShown = $derived(phone ? sessionsOpen : showSessions);
let readingAt = 0;
async function openSessions(): Promise<void> {
  readingAt = window.scrollY;
  sessionsOpen = true;
  await tick();
  window.scrollTo(0, 0);
}
/** Puts the transcript back; `restore` returns to where the reader was. */
async function closeSessions(restore: boolean): Promise<void> {
  if (!sessionsOpen) return;
  sessionsOpen = false;
  // The transcript has no height while the list replaces it.
  await tick();
  if (restore) window.scrollTo(0, readingAt);
}
function toggleSessions(): void {
  if (!phone) showSessions = !showSessions;
  else if (sessionsOpen) void closeSessions(true);
  else void openSessions();
}
let exportOpen = $state(false);
let currentMessage = $state("");
// Other chats from the same project, with this one kept in view.
const projectSessions = $derived.by(() => {
  if (!chat) return [];
  const same = sessions.filter((session) => session.project === chat.project).slice(0, 6);
  return same.includes(chat) ? same : [chat, ...same.slice(0, 5)];
});
const now = Date.now();
const memoryParams = $derived(
  new URLSearchParams({
    chat: id,
    ...(shared || (machine && machine !== catalogue.data?.localNode)
      ? { shared: "true", node: machine }
      : {}),
  }),
);
const memories = createQuery(() => ({
  ...stableRead,
  enabled: catalogue.data !== undefined,
  queryKey: ["chat-memory", id, memoryParams.toString()],
  queryFn: ({ signal }) => recordPage(memoryParams.toString(), signal),
}));
function remember(): void {
  const pane = transcriptPane;
  if (!pane || !loaded) return;
  // The page scrolls under a sticky header; the first message below it is
  // where the reader is.
  const elements = Array.from(pane.querySelectorAll<HTMLElement>("[data-message]"));
  const first = elements.find((element) => element.getBoundingClientRect().bottom > 90);
  if (first?.dataset.message) {
    currentMessage = first.dataset.message;
    reading.remember({ id, node: displayMachine, message: first.dataset.message, shared });
  }
}
function jump(message: string): void {
  currentMessage = message;
  document.getElementById(`message-${message}`)?.scrollIntoView({ block: "start" });
}

let transcriptPane: HTMLElement | undefined = $state();

// Find in chat reads the whole chat, every page, so matches on pages not
// shown yet still count; going to one loads its page and opens any
// collapsed step around it.
let findOpen = $state(false);
let findQuery = $state("");
let findIndex = $state(0);
let findActive = $state(false);
let findProgress = $state(0);
let findInput = $state<HTMLInputElement>();
let pendingFind: FindMatch | undefined;
const shortcut =
  typeof navigator !== "undefined" && /mac|iphone|ipad/i.test(navigator.platform) ? "⌘F" : "Ctrl F";
const whole = createQuery(() => ({
  queryKey: ["find-chat", id, machine, shared],
  enabled: findOpen,
  staleTime: 60_000,
  queryFn: ({ signal }: { signal: AbortSignal }) =>
    collectChat(
      id,
      machine,
      signal,
      (count) => {
        findProgress = count;
      },
      (id, node, offset, signal, limit) => transcript(id, node, offset, signal, shared, limit),
    ),
}));
const matches = $derived(
  findOpen && whole.data
    ? findMatches(
        whole.data.map((message) => ({ id: message.id, text: messageText(message) })),
        findQuery,
      )
    : [],
);
const findStatus = $derived.by(() => {
  if (whole.isPending) return `Reading the chat… ${findProgress.toLocaleString()} messages`;
  if (whole.error) return "Couldn’t read the whole chat";
  if (!findQuery.trim()) return "";
  if (!matches.length) return "No matches";
  return findActive
    ? countLabel(findIndex, matches.length)
    : `${countLabel(0, matches.length).replace(/^1 of /, "")} matches`;
});
$effect(() => {
  void findQuery;
  findActive = false;
  findIndex = 0;
});
function openFind(): void {
  findOpen = true;
  void tick().then(() => {
    findInput?.focus();
    findInput?.select();
  });
}
function closeFind(): void {
  findOpen = false;
  findActive = false;
  clearMatches();
}
function go(step: 1 | -1): void {
  const count = matches.length;
  if (!count) return;
  findIndex = findActive ? (findIndex + step + count) % count : step > 0 ? 0 : count - 1;
  findActive = true;
  const target = matches[findIndex];
  if (!target) return;
  const targetOffset = Math.floor(target.position / PAGE_SIZE) * PAGE_SIZE;
  if (anchor || offset !== targetOffset) {
    pendingFind = target;
    anchor = "";
    offset = targetOffset;
  } else void reveal(target);
}
async function reveal(target: FindMatch): Promise<void> {
  await tick();
  const pane = transcriptPane;
  if (!pane) return;
  const element = pane.querySelector(`[data-message="${CSS.escape(target.messageId)}"]`);
  if (!element) return;
  if (element instanceof HTMLDetailsElement) element.open = true;
  for (
    let fold = element.closest("details");
    fold;
    fold = fold.parentElement?.closest("details") ?? null
  )
    fold.open = true;
  await tick();
  const spot = paintMatches(pane, findQuery, { element, occurrence: target.occurrence });
  if (spot) bringIntoView(spot);
}
$effect(() => {
  const query = findQuery;
  const open = findOpen;
  void loaded;
  if (!open) return;
  void tick().then(() => {
    if (transcriptPane && !pendingFind) paintMatches(transcriptPane, query);
  });
});
$effect(() => () => clearMatches());
let notice = $state("");
let exporting = $state(false);
let exportController: AbortController | undefined;
let snapshot = $state<{ key: string; page: MessagePage } | null>(null);
// Keep the sidebar mounted across chat navigation. Only explicit navigation
// resets the reading page; catalogue updates never touch it.
$effect(() => {
  const route = [id, node, messageId, shared] as const;
  anchor = route[2];
  offset = 0;
  notice = "";
  exportOpen = false;
  sessionsOpen = false;
  window.scrollTo({ top: 0, behavior: "instant" });
  return () => exportController?.abort();
});
const key = $derived(JSON.stringify([id, machine, offset, anchor, shared]));
const live = createQuery(() => ({
  queryKey: ["transcript", id, machine, offset, anchor, shared],
  queryFn: ({ signal }) =>
    anchor
      ? context(anchor, machine, signal, shared)
      : transcript(id, machine, offset, signal, shared),
  ...stableRead,
}));
const loaded = $derived(snapshot?.key === key ? snapshot.page : undefined);
const total = $derived(loaded?.total);
/** Where the last page starts, when the chat's length is known. */
const lastOffset = $derived(total ? Math.floor((total - 1) / PAGE_SIZE) * PAGE_SIZE : undefined);
/** Set when the reader asked for the end, so the last page opens at its bottom. */
let toEnd = false;
let rememberedKey = "";
$effect(() => {
  const first = loaded?.messages[0];
  if (first && catalogue.data && rememberedKey !== key) {
    rememberedKey = key;
    currentMessage = anchor || first.id;
    const position = { id, node: displayMachine, message: first.id, shared };
    untrack(() => reading.remember(position));
  }
});
// When Mash indexes more of this chat the page is read again. Text already
// shown never changes by itself: new messages are added at the end only while
// the reader is at the bottom of the chat, and are otherwise offered.
$effect(() => {
  const chatId = id;
  const from = displayMachine;
  // The same chat ID may exist on another machine; follow the copy shown.
  const local = !shared && (!machine || machine === catalogue.data?.localNode);
  return onChange((change) => {
    if (concerns(change, chatId, from, local)) void live.refetch();
  });
});
const update = $derived(
  loaded && live.data ? pageUpdate(loaded, live.data) : ({ kind: "same" } as const),
);
const changed = $derived(update.kind === "rewritten");
const added = $derived(update.kind === "appended" ? update.added : 0);
function atBottom(): boolean {
  const page = document.documentElement;
  return window.innerHeight + window.scrollY >= page.scrollHeight - 120;
}
function toBottom(): void {
  void tick().then(() =>
    window.scrollTo({ top: document.documentElement.scrollHeight, behavior: "instant" }),
  );
}
$effect(() => {
  const next = live.data;
  if (update.kind !== "appended" || !update.onPage || !next) return;
  if (anchor || sessionsOpen || snapshot?.key !== key || !untrack(atBottom)) return;
  snapshot = { key, page: next };
  toBottom();
});
/** Shows the messages that arrived while the reader was elsewhere in the chat. */
function showNew(): void {
  const next = live.data;
  if (!next) return;
  const last = next.total ? Math.floor((next.total - 1) / PAGE_SIZE) * PAGE_SIZE : offset;
  if (!anchor && offset === last) {
    snapshot = { key, page: next };
    toBottom();
  } else {
    toEnd = true;
    page(last);
  }
}

$effect(() => {
  if (live.data && snapshot?.key !== key) {
    snapshot = { key, page: live.data };
    if (pendingFind) {
      const target = pendingFind;
      pendingFind = undefined;
      void reveal(target);
    } else if (toEnd) {
      toEnd = false;
      void tick().then(() =>
        window.scrollTo({ top: document.documentElement.scrollHeight, behavior: "instant" }),
      );
    } else if (anchor)
      tick()
        .then(() =>
          document.getElementById(`message-${anchor}`)?.scrollIntoView({ block: "center" }),
        )
        .catch(() => {
          notice = "Use Find to locate the selected message.";
        });
  }
});

function plainText(message: Message): string {
  return messageText(message);
}
async function copy(message: Message): Promise<void> {
  try {
    await navigator.clipboard.writeText(plainText(message));
    notice = "Copied message.";
  } catch {
    notice = "Copy failed. Select the message text to copy it manually.";
  }
}
async function exportChat(format: ExportFormat): Promise<void> {
  if (exporting) return;
  const popup = format === "pdf" ? window.open("", "_blank") : null;
  if (format === "pdf" && !popup) {
    notice = "Allow a new tab to prepare the PDF export.";
    return;
  }
  if (popup) {
    popup.opener = null;
    popup.document.body.textContent = "Preparing the entire chat for printing…";
  }
  const controller = new AbortController();
  exportController = controller;
  const title = chat?.title ?? id;
  const source = displayMachine;
  exporting = true;
  notice = "Preparing export…";
  try {
    const messages = await collectChat(
      id,
      machine,
      controller.signal,
      (count) => {
        notice = `Preparing export: ${count.toLocaleString()} messages…`;
      },
      (id, node, offset, signal, limit) => transcript(id, node, offset, signal, shared, limit),
    );
    controller.signal.throwIfAborted();
    if (format === "pdf" && popup) {
      if (popup.closed) throw new Error("The PDF preview tab was closed.");
      popup.document.open();
      popup.document.write(htmlExport(title, source, messages));
      popup.document.close();
      popup.focus();
      popup.print();
      notice = "Choose Save as PDF in the print dialog. The preview includes the entire chat.";
    } else {
      downloadChat(
        format === "markdown"
          ? markdownExport(title, source, messages)
          : htmlExport(title, source, messages),
        format === "markdown" ? "markdown" : "html",
      );
      notice = `Exported ${messages.length.toLocaleString()} messages.`;
    }
  } catch (error) {
    popup?.close();
    notice = controller.signal.aborted
      ? "Export cancelled."
      : error instanceof Error
        ? error.message
        : "Export failed.";
  } finally {
    exporting = false;
    exportController = undefined;
  }
}
function page(next: number): void {
  anchor = "";
  offset = next;
  window.scrollTo({ top: 0, behavior: "instant" });
}
function latest(): void {
  if (lastOffset === undefined) return;
  if (!anchor && offset === lastOffset) {
    window.scrollTo({ top: document.documentElement.scrollHeight, behavior: "instant" });
    return;
  }
  toEnd = true;
  page(lastOffset);
}
async function copySession(): Promise<void> {
  try {
    await navigator.clipboard.writeText(id);
    notice = "Session ID copied.";
  } catch {
    notice = `Copy failed. The session ID is ${id}.`;
  }
}
// Consecutive tool calls, tool output and reasoning read as one run of
// steps, folded so the conversation stays the main thing on the page.
type Item =
  | { readonly kind: "message"; readonly message: Message }
  | { readonly kind: "steps"; readonly messages: readonly Message[] };
const items = $derived.by(() => {
  const list: Item[] = [];
  let run: Message[] = [];
  const flush = () => {
    if (run.length) list.push({ kind: "steps", messages: run });
    run = [];
  };
  for (const message of loaded?.messages ?? []) {
    if (message.presentation && message.presentation !== "dialogue") run.push(message);
    else {
      flush();
      list.push({ kind: "message", message });
    }
  }
  flush();
  return list;
});
type StepPart =
  | { readonly kind: "call"; readonly call: ToolCall; readonly messages: readonly Message[] }
  | { readonly kind: "note"; readonly message: Message };
// A run of steps as what the reader sees: each tool call once, with its
// output, and the agent's thinking in between, in the order they happened.
function stepParts(messages: readonly Message[]): StepPart[] {
  const calls = toolCalls(messages);
  const parts: StepPart[] = [];
  for (const message of messages) {
    if (message.presentation === "reasoning") parts.push({ kind: "note", message });
    for (const call of calls) {
      if (call.id !== message.id && !call.id.startsWith(`${message.id}:`)) continue;
      parts.push({
        kind: "call",
        call,
        messages: messages.filter(
          (other) =>
            (other.id === message.id && call.id === message.id) || other.id === call.resultId,
        ),
      });
    }
  }
  return parts;
}
function stepLabel(message: Message): string {
  if (message.presentation === "reasoning") return "Thinking";
  return message.presentation === "tool-result" ? "Tool output" : message.toolLabel || "Tool call";
}
function stepsSummary(messages: readonly Message[]): string {
  const calls = toolCalls(messages);
  const names = [...new Set(calls.map((call) => call.name))];
  const count = calls.length || messages.length;
  const noun = calls.length ? "tool call" : "step";
  const failed = calls.filter((call) => call.status?.failed).length;
  return `${count} ${noun}${count === 1 ? "" : "s"}${names.length ? `: ${names.slice(0, 4).join(", ")}${names.length > 4 ? ", …" : ""}` : ""}${failed ? ` · ${failed} failed` : ""}`;
}
const prompts = $derived(
  (loaded?.messages ?? []).filter(
    (message) => message.role === "user" && message.presentation === "dialogue",
  ),
);
</script>

<svelte:window
  onscroll={remember}
  onkeydown={(event) => {
    if ((event.ctrlKey || event.metaKey) && !event.altKey && event.key.toLowerCase() === "f") {
      event.preventDefault();
      openFind();
    } else if (event.key === "Escape") {
      exportOpen = false;
      if (findOpen) closeFind();
    }
  }}
/>
<div class="chat" class:nosess={!showSessions} class:sessopen={sessionsOpen}>
  <nav class="sess" aria-label="Prompts and sessions">
    {#if phone && sessionsOpen}<button class="btn sessclose" type="button" onclick={() => closeSessions(true)}><X size={15} aria-hidden="true" />Back to chat</button>{/if}
    {#if prompts.length >= 2}
      <div class="outl">
        <h2 class="h2"><ListOrdered size={15} aria-hidden="true" />your prompts on this page</h2>
        <div class="outl-l">
          {#each prompts as message, index (message.id)}
            <button type="button" class:on={currentMessage === message.id} aria-current={currentMessage === message.id ? "location" : undefined} onclick={async () => { await closeSessions(false); jump(message.id); }}><small>{index + 1}</small><span>{plainText(message).replace(/\s+/g, " ").slice(0, 80)}</span></button>
          {/each}
        </div>
      </div>
    {/if}
    {#if chat?.project}<h2 class="h2"><a href={projectHref(chat.project)}><Folder size={15} aria-hidden="true" />{projectLabel(chat.project)} sessions</a></h2>{/if}
    {#each projectSessions as session (session.machine + session.id)}
      {@const current = session.id === id && session.machine === displayMachine}
      <a class="ses" class:on={current} href={chatHref(session.id, session.machine)} aria-current={current ? "page" : undefined}><AgentMark agent={session.agent} /><span><span class="clamp">{session.title}</span><small>{whenLabel(session.updatedMs, now)}</small></span></a>
    {/each}
    <h2 class="h2" style:margin-top="10px"><a href={recordsHref(memoryParams)}><Brain size={15} aria-hidden="true" />memory from this chat</a></h2>
    {#if memories.error}<p class="meta" role="alert">{memories.error.message}</p>{/if}
    {#each memories.data?.records ?? [] as record (record.id)}
      {@const revision = record.revisions.find((revision) => revision.revision === record.currentRevision)}
      {#if revision}<a class="ses" href={recordHref(record.id, record.node, recordsHref(memoryParams))}><span><span class="clamp">{revision.statement}</span><small>{label(revision.details.kind).toLowerCase()} / {label(revision.details.state).toLowerCase()}</small></span></a>{/if}
    {/each}
    {#if memories.data && !memories.data.records.length}<p class="meta" style:padding="0 10px">Nothing saved from this chat.</p>{/if}
  </nav>
  <article class="log" bind:this={transcriptPane}>
    <header class="hd" style:align-items="flex-start">
      <div style:min-width="0">
        <h1 class="h1">{chat ? plainTitle(chat.title) : `Chat ${id.slice(-8)}`}</h1>
        <p class="meta" style:margin-top="8px">
          {#if chat}<MetaChips {chat} />{:else}{displayMachine}{/if}
          <span class="mtx">{anchor ? "around the selected match" : total ? `messages ${(offset + 1).toLocaleString()}–${Math.min(offset + PAGE_SIZE, total).toLocaleString()} of ${total.toLocaleString()}` : `messages ${(offset + 1).toLocaleString()}–${(offset + PAGE_SIZE).toLocaleString()}`}</span>
        </p>
      </div>
      <span class="acts" style:position="relative">
        <button class="btn" type="button" aria-pressed={sessionsShown} aria-label="{sessionsShown ? 'Hide' : 'Show'} session list" title="{sessionsShown ? 'Hide' : 'Show'} session list" onclick={toggleSessions} style:padding="0 11px"><PanelLeft size={15} aria-hidden="true" />{#if phone}Sessions{/if}</button>
        <button class="btn" type="button" aria-expanded={findOpen} onclick={openFind}><Search size={15} aria-hidden="true" />Find in chat<kbd class="kbd">{shortcut}</kbd></button>
        <button class="btn" type="button" onclick={copySession}><Copy size={15} aria-hidden="true" />Copy session ID</button>
        <button class="btn" type="button" aria-expanded={exportOpen} onclick={() => (exportOpen = !exportOpen)}><Download size={15} aria-hidden="true" />Export</button>
        {#if exportOpen}
          <div class="menu">
            <button type="button" disabled={exporting || !loaded} onclick={() => { exportOpen = false; void exportChat("pdf"); }}><Printer size={15} aria-hidden="true" />Export to PDF</button>
            <button type="button" disabled={exporting || !loaded} onclick={() => { exportOpen = false; void exportChat("html"); }}><FileCode size={15} aria-hidden="true" />Export to HTML</button>
            <button type="button" disabled={exporting || !loaded} onclick={() => { exportOpen = false; void exportChat("markdown"); }}><FileText size={15} aria-hidden="true" />Export to Markdown</button>
          </div>
        {/if}
        <button class="btn" type="button" aria-label="Check for updates" title="Check for updates" disabled={live.isFetching} onclick={() => live.refetch()} style:padding="0 11px"><RefreshCw size={15} /></button>
      </span>
    </header>
    {#if findOpen}
      <div class="findbar" role="search">
        <Search size={16} aria-hidden="true" />
        <label class="sr-only" for="find-in-chat">Find in this chat</label>
        <input
          id="find-in-chat"
          type="text"
          enterkeyhint="search"
          placeholder="Find in this chat"
          autocomplete="off"
          bind:this={findInput}
          bind:value={findQuery}
          onkeydown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              go(event.shiftKey ? -1 : 1);
            }
          }}
        />
        <span class="findn" role="status">{findStatus}</span>
        <button class="btn" type="button" aria-label="Previous match" title="Previous match" disabled={!matches.length} onclick={() => go(-1)}><ChevronUp size={16} /></button>
        <button class="btn" type="button" aria-label="Next match" title="Next match" disabled={!matches.length} onclick={() => go(1)}><ChevronDown size={16} /></button>
        <button class="btn" type="button" aria-label="Close find" title="Close find" onclick={closeFind}><X size={16} /></button>
      </div>
    {/if}
    {#if exporting}<p class="meta"><button class="a" type="button" onclick={() => exportController?.abort()}>Cancel export</button></p>{/if}
    {#if anchor}<p class="meta">Showing the messages around a search match.</p>{/if}
    <QueryError title="Couldn’t load the chat list" query={catalogue} retryLabel="Retry catalogue" />
    {#each catalogue.data?.warnings ?? [] as warning (warning)}<p role="status" class="meta">{warning}</p>{/each}
    {#if live.isPending}
      <div class="state-c" role="status">
        <p style:display="flex" style:align-items="center" style:gap="10px"><Terminal size={18} color="var(--clay2)" aria-hidden="true" />Loading messages…</p>
        <div class="skel" style:width="82%"></div><div class="skel" style:width="64%"></div><div class="skel" style:width="74%"></div>
      </div>
    {/if}
    <QueryError title="Couldn’t load this conversation" query={live} />
    {#if changed}<button type="button" class="btn" onclick={() => { if (live.data) snapshot = { key, page: live.data }; }}><RefreshCw size={15} aria-hidden="true" />This page has changed. Show updated messages</button>{/if}
    {#if notice}<p role="status" class="meta">{notice}</p>{/if}
    <div class="newpill" aria-live="polite">{#if added && !sessionsOpen}<button type="button" class="btn p" onclick={showNew}><ChevronsDown size={15} aria-hidden="true" />{added.toLocaleString()} new message{added === 1 ? "" : "s"}</button>{/if}</div>
    {#if loaded}
      {#if !loaded.messages.length}<div class="state-c"><p>No messages at this position. The chat may have been removed, or this is the end.</p></div>{/if}
      {#snippet saved(message: Message)}
        {#each memories.data?.records ?? [] as record (record.id)}
          {@const revision = record.revisions.find((revision) => revision.revision === record.currentRevision)}
          {#if revision?.evidence.some((source) => source.messageId === message.id)}
            <a class="meta a" href={recordHref(record.id, record.node, recordsHref(memoryParams))} style:align-self={message.role === "user" ? "flex-end" : "flex-start"} style:margin-top="12px">Saved as {label(revision.details.kind).toLowerCase()}: {revision.statement}</a>
          {/if}
        {/each}
      {/snippet}
      {#each items as item (item.kind === "message" ? item.message.id : item.messages[0]?.id)}
        {#if item.kind === "steps"}
          <details class="fold" open={item.messages.some((message) => message.id === messageId)}>
            <summary><Terminal size={16} aria-hidden="true" />{stepsSummary(item.messages)}</summary>
            <div>
              {#each stepParts(item.messages) as part (part.kind === "call" ? part.call.id : part.message.id)}
                {#if part.kind === "call"}
                  <ToolStep call={part.call} selected={messageId} />
                  {#each part.messages as message (message.id)}{@render saved(message)}{/each}
                {:else}
                  {@const message = part.message}
                  <details class="fold" class:hit={message.id === messageId} id="message-{message.id}" data-message={message.id} open={message.id === messageId} style:scroll-margin-top="80px">
                    <summary><Lightbulb size={15} aria-hidden="true" />{stepLabel(message)}<span class="mtx">{message.at}</span></summary>
                    <div><pre class="pre" style:max-height="32rem" style:overflow="auto">{plainText(message)}</pre></div>
                  </details>
                  {@render saved(message)}
                {/if}
              {/each}
            </div>
          </details>
        {:else}
          {@const message = item.message}
          {@const selected = message.id === messageId}
          <div class="hitbox" class:hit={selected} id="message-{message.id}" data-message={message.id} style:scroll-margin-top="80px">
            {#if selected}<span class="hitlabel">Matched your search</span>{/if}
            {#if message.role === "user"}
              <div class="you"><MessageBody text={plainText(message)} own /><button class="cp" type="button" aria-label="Copy message" title="Copy message" onclick={() => copy(message)}><Copy size={14} /></button></div>
            {:else}
              <div class="msg">
                {#each message.blocks as block, i (i)}
                  {#if block.type === "text"}<MessageBody text={block.markdown} />{/if}
                {/each}
                <button class="cp" type="button" aria-label="Copy message" title="Copy message" onclick={() => copy(message)}><Copy size={14} /></button>
              </div>
              {#if message.activity}<details class="fold"><summary><Terminal size={16} aria-hidden="true" />Recorded tool activity</summary><div><pre class="pre" style:max-height="20rem" style:overflow="auto">{message.activity}</pre></div></details>{/if}
            {/if}
            {@render saved(message)}
          </div>
        {/if}
      {/each}
      <!-- Around a search match there are no pages to step through, only the two ends to jump to. -->
      <nav aria-label="Message pages" class="pager end">
        <button class="btn" type="button" disabled={!anchor && offset === 0} onclick={() => page(0)} aria-label="First message" title="First message"><ChevronsLeft size={15} aria-hidden="true" /></button>
        {#if !anchor}
          <button class="btn" type="button" disabled={offset === 0} onclick={() => page(Math.max(0, offset - PAGE_SIZE))}><ChevronLeft size={15} aria-hidden="true" />Previous messages</button>
          <button class="btn" type="button" disabled={loaded.messages.length < PAGE_SIZE} onclick={() => page(offset + PAGE_SIZE)}>Next messages<ChevronRight size={15} aria-hidden="true" /></button>
        {/if}
        {#if lastOffset !== undefined}<button class="btn" type="button" disabled={!anchor && offset === lastOffset} onclick={latest} aria-label="Latest message" title="Latest message"><ChevronsRight size={15} aria-hidden="true" /></button>{/if}
      </nav>
    {/if}
  </article>
</div>
