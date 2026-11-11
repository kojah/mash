<script lang="ts">
import Brain from "@lucide/svelte/icons/brain";
import Copy from "@lucide/svelte/icons/copy";
import Download from "@lucide/svelte/icons/download";
import Lightbulb from "@lucide/svelte/icons/lightbulb";
import PanelLeft from "@lucide/svelte/icons/panel-left";
import RefreshCw from "@lucide/svelte/icons/refresh-cw";
import Terminal from "@lucide/svelte/icons/terminal";
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
import { plainTitle } from "../lib/markdown";
import { catalogOptions, stableRead } from "../lib/queries";
import { reading } from "../lib/reading.svelte";
import { label, recordPage } from "../lib/records";
import { chatHref, projectHref, recordHref, recordsHref } from "../lib/route.svelte";
import type { Message } from "../lib/types";
import AgentMark from "./AgentMark.svelte";
import MessageBody from "./MessageBody.svelte";
import MetaChips from "./MetaChips.svelte";
import QueryError from "./QueryError.svelte";

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
const changed = $derived(
  loaded && live.data && JSON.stringify(loaded) !== JSON.stringify(live.data),
);

$effect(() => {
  if (live.data && snapshot?.key !== key) {
    snapshot = { key, page: live.data };
    if (anchor)
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
      (id, node, offset, signal) => transcript(id, node, offset, signal, shared),
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
function stepLabel(message: Message): string {
  if (message.presentation === "reasoning") return "Thinking";
  return message.presentation === "tool-result" ? "Tool output" : message.toolLabel || "Tool call";
}
function stepsSummary(messages: readonly Message[]): string {
  const calls = messages.filter((message) => message.presentation === "tool-call");
  const names = [...new Set(calls.map(stepLabel))];
  const count = calls.length || messages.length;
  const noun = calls.length ? "tool call" : "step";
  return `${count} ${noun}${count === 1 ? "" : "s"}${names.length ? `: ${names.slice(0, 4).join(", ")}${names.length > 4 ? ", …" : ""}` : ""}`;
}
const prompts = $derived(
  (loaded?.messages ?? []).filter(
    (message) => message.role === "user" && message.presentation === "dialogue",
  ),
);
</script>

<svelte:window onscroll={remember} onkeydown={(event) => { if (event.key === "Escape") exportOpen = false; }} />
<div class="chat" class:nosess={!showSessions}>
  <nav class="sess" aria-label="Prompts and sessions">
    {#if prompts.length >= 2}
      <div class="outl">
        <h2 class="h2">your prompts on this page</h2>
        <div class="outl-l">
          {#each prompts as message, index (message.id)}
            <button type="button" class:on={currentMessage === message.id} aria-current={currentMessage === message.id ? "location" : undefined} onclick={() => jump(message.id)}><small>{index + 1}</small><span>{plainText(message).replace(/\s+/g, " ").slice(0, 80)}</span></button>
          {/each}
        </div>
      </div>
    {/if}
    {#if chat?.project}<h2 class="h2"><a href={projectHref(chat.project)}>{projectLabel(chat.project)} sessions</a></h2>{/if}
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
          <span class="mtx">{anchor ? "around the selected match" : `messages ${offset.toLocaleString()}–${(offset + PAGE_SIZE - 1).toLocaleString()}`}</span>
        </p>
      </div>
      <span class="acts" style:position="relative">
        <button class="btn" type="button" aria-pressed={showSessions} aria-label="{showSessions ? 'Hide' : 'Show'} session list" title="{showSessions ? 'Hide' : 'Show'} session list" onclick={() => (showSessions = !showSessions)} style:padding="0 11px"><PanelLeft size={15} /></button>
        <button class="btn" type="button" onclick={copySession}><Copy size={15} aria-hidden="true" />Copy session ID</button>
        <button class="btn" type="button" aria-expanded={exportOpen} onclick={() => (exportOpen = !exportOpen)}><Download size={15} aria-hidden="true" />Export</button>
        {#if exportOpen}
          <div class="menu">
            <button type="button" disabled={exporting || !loaded} onclick={() => { exportOpen = false; void exportChat("pdf"); }}>Export to PDF</button>
            <button type="button" disabled={exporting || !loaded} onclick={() => { exportOpen = false; void exportChat("html"); }}>Export to HTML</button>
            <button type="button" disabled={exporting || !loaded} onclick={() => { exportOpen = false; void exportChat("markdown"); }}>Export to Markdown</button>
          </div>
        {/if}
        <button class="btn" type="button" aria-label="Check for updates" title="Check for updates" disabled={live.isFetching} onclick={() => live.refetch()} style:padding="0 11px"><RefreshCw size={15} /></button>
      </span>
    </header>
    {#if exporting}<p class="meta"><button class="a" type="button" onclick={() => exportController?.abort()}>Cancel export</button></p>{/if}
    {#if anchor}<p class="meta">Showing the messages around a search match. <button class="a" type="button" onclick={() => page(0)}>Read from the beginning</button></p>{/if}
    <QueryError title="Couldn’t load the chat list" query={catalogue} retryLabel="Retry catalogue" />
    {#each catalogue.data?.warnings ?? [] as warning (warning)}<p role="status" class="meta">{warning}</p>{/each}
    {#if live.isPending}
      <div class="state-c" role="status">
        <p style:display="flex" style:align-items="center" style:gap="10px"><Terminal size={18} color="var(--clay2)" aria-hidden="true" />Loading messages…</p>
        <div class="skel" style:width="82%"></div><div class="skel" style:width="64%"></div><div class="skel" style:width="74%"></div>
      </div>
    {/if}
    <QueryError title="Couldn’t load this conversation" query={live} />
    {#if changed}<button type="button" class="btn" onclick={() => { if (live.data) snapshot = { key, page: live.data }; }}>This page has changed. Show updated messages</button>{/if}
    {#if notice}<p role="status" class="meta">{notice}</p>{/if}
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
              {#each item.messages as message (message.id)}
                <details class="fold" class:hit={message.id === messageId} id="message-{message.id}" data-message={message.id} open={message.id === messageId} style:scroll-margin-top="80px">
                  <summary>{#if message.presentation === "reasoning"}<Lightbulb size={15} aria-hidden="true" />{:else}<Terminal size={15} aria-hidden="true" />{/if}{stepLabel(message)}<span class="mtx">{message.at}</span></summary>
                  <div><pre class="pre" style:max-height="32rem" style:overflow="auto">{plainText(message)}</pre></div>
                </details>
                {@render saved(message)}
              {/each}
            </div>
          </details>
        {:else}
          {@const message = item.message}
          {@const selected = message.id === messageId}
          <div class="hitbox" class:hit={selected} id="message-{message.id}" data-message={message.id} style:scroll-margin-top="80px" style:padding={selected ? "10px 12px" : "0"}>
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
      {#if !anchor}
        <nav aria-label="Message pages" class="pager end">
          <button class="btn" type="button" disabled={offset === 0} onclick={() => page(Math.max(0, offset - PAGE_SIZE))}>Previous messages</button>
          <button class="btn" type="button" disabled={loaded.messages.length < PAGE_SIZE} onclick={() => page(offset + PAGE_SIZE)}>Next messages</button>
        </nav>
      {/if}
    {/if}
  </article>
</div>
