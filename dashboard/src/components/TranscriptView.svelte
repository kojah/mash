<script lang="ts">
import Download from "@lucide/svelte/icons/download";
import Layers from "@lucide/svelte/icons/layers";
import List from "@lucide/svelte/icons/list";
import { createQuery } from "@tanstack/svelte-query";
import { tick, untrack } from "svelte";
import { agents } from "../lib/agents";
import { context, type MessagePage, PAGE_SIZE, transcript } from "../lib/api";
import {
  collectChat,
  downloadChat,
  type ExportFormat,
  htmlExport,
  markdownExport,
  messageText,
} from "../lib/export";
import { catalogOptions, stableRead } from "../lib/queries";
import { reading } from "../lib/reading.svelte";
import { label, recordPage } from "../lib/records";
import { chatHref, recordHref, recordsHref } from "../lib/route.svelte";
import type { Message } from "../lib/types";
import AgentMark from "./AgentMark.svelte";
import MessageBody from "./MessageBody.svelte";

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
let outlineOpen = $state(false);
let currentMessage = $state("");
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
  const top = pane.getBoundingClientRect().top;
  const elements = Array.from(pane.querySelectorAll<HTMLElement>("[data-message]"));
  const first = elements.find((element) => element.getBoundingClientRect().bottom > top + 30);
  if (first?.dataset.message) {
    currentMessage = first.dataset.message;
    reading.remember({ id, node: displayMachine, message: first.dataset.message, shared });
  }
}
function jump(message: string): void {
  currentMessage = message;
  document.getElementById(`message-${message}`)?.scrollIntoView({ block: "start" });
  outlineOpen = false;
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
  outlineOpen = false;
  transcriptPane?.scrollTo({ top: 0, behavior: "instant" });
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
}
</script>

<svelte:window onkeydown={(event) => { if (event.key === "Escape") outlineOpen = false; }} />
<div class="reader-layout">
    <aside class="reader-outline" class:mobile-open={outlineOpen} aria-label="Conversation outline">
      <h2>Outline <button type="button" class="mobile-only" onclick={() => outlineOpen = false}>Close</button></h2>
      <p class="text-xs text-muted">Messages on this page</p>
      {#each loaded?.messages.filter(message => message.role === "user") ?? [] as message (message.id)}
        <button type="button" class="outline-item" aria-current={currentMessage === message.id ? "location" : undefined} onclick={() => jump(message.id)}><span>{message.ordinal ?? ""}</span><span>{plainText(message).slice(0, 110)}</span></button>
      {/each}
      <details class="recent-sidebar"><summary>Recent chats</summary>
        {#each sessions.slice(0, 30) as session (JSON.stringify([session.machine, session.id]))}
          <a href={chatHref(session.id, session.machine)} aria-current={session.id === id && session.machine === displayMachine ? "page" : undefined}>{session.title}</a>
        {/each}
      </details>
    </aside>
    <main bind:this={transcriptPane} onscroll={remember} class="transcript-pane">
      <header class="flex flex-col gap-2">
        <h1 class="reader-title">{chat?.title ?? `Chat ${id.slice(-8)}`}</h1>
        <p class="reader-metadata">{#if chat}<AgentMark agent={chat.agent} size={13} />{agents[chat.agent].name} · {/if}{displayMachine} · {chat?.project ?? ""}</p>
        <p class="m-0 text-xs text-muted">{anchor ? "Context around the selected search match" : `Message positions ${offset}–${offset + PAGE_SIZE - 1}`} · Browser Find searches this page. Branches are shown in recorded display order.</p>
        <div class="reader-actions">
          <details class="relative" id="reader-export">
            <summary class="cursor-pointer">Export</summary>
            <div class="absolute left-0 top-7 z-10 flex w-56 flex-col gap-3 rounded-xl border border-line bg-surface p-4 shadow-lg">
              <button type="button" disabled={exporting || !loaded} onclick={() => exportChat('pdf')}>Export to PDF</button>
              <button type="button" disabled={exporting || !loaded} onclick={() => exportChat('html')}>Export to HTML</button>
              <button type="button" disabled={exporting || !loaded} onclick={() => exportChat('markdown')}>Export to Markdown</button>
              <span class="text-xs text-muted">Entire chat, including tool output. PDF uses your browser’s print dialog.</span>
            </div>
          </details>
          {#if exporting}<button type="button" onclick={() => exportController?.abort()}>Cancel export</button>{/if}
          <button type="button" onclick={() => live.refetch()}>Check for updates</button>
          {#if anchor}<button type="button" onclick={() => page(0)}>Read from the beginning</button>{/if}
        </div>
      </header>
      {#if catalogue.error}<p role="alert">{catalogue.error.message} <button type="button" onclick={() => catalogue.refetch()}>Retry catalogue</button></p>{/if}
      {#each catalogue.data?.warnings ?? [] as warning (warning)}<p role="status" class="text-sm text-muted">{warning}</p>{/each}
      {#if live.isPending}
        <div role="status" class="rounded-xl border border-line bg-surface p-5">
          <p class="m-0 text-sm text-muted">Loading messages…</p>
          <div aria-hidden="true" class="mt-5 flex flex-col gap-3"><div class="h-3 w-1/3 rounded bg-fill"></div><div class="h-3 w-full rounded bg-fill"></div><div class="h-3 w-4/5 rounded bg-fill"></div></div>
        </div>
      {/if}
      {#if live.error}<p role="alert">{live.error.message} <button type="button" onclick={() => live.refetch()}>Retry</button></p>{/if}
      {#if changed}<button type="button" class="rounded-lg bg-accent-soft p-3 text-accent-ink" onclick={() => { if (live.data) snapshot = { key, page: live.data }; }}>This page has changed. Show updated messages</button>{/if}
      {#if notice}<p role="status" class="text-sm text-muted">{notice}</p>{/if}
      {#if loaded}
        {#if !loaded.messages.length}<p>No messages at this position. The chat may have been removed or this is the end.</p>{/if}
        <ol class="m-0 flex list-none flex-col gap-6 p-0">
          {#each loaded.messages as message (message.id)}
            <li id="message-{message.id}" data-message={message.id} class="flex min-w-0 flex-col gap-2 scroll-mt-8 {message.id === messageId ? 'rounded-lg ring-2 ring-accent p-3' : ''}">
              {#if message.presentation && message.presentation !== "dialogue"}
                <details class="rounded-lg border border-line bg-surface p-3">
                  <summary class="cursor-pointer text-sm"><strong>{message.presentation === 'tool-result' ? 'Tool output' : message.presentation === 'reasoning' ? 'Reasoning' : message.toolLabel || 'Tool call'}</strong><span class="ml-2 text-xs text-muted">{message.at}</span></summary>
                  <p class="text-xs text-muted">Recorded as {message.role} · message {message.ordinal}</p>
                  <pre class="max-h-[32rem] overflow-auto rounded bg-ground p-3 text-xs whitespace-pre-wrap break-words">{plainText(message)}</pre>
                </details>
              {:else}
              <span class="text-xs text-muted">{message.role === 'user' ? 'You' : message.role} · {message.at}</span>
              <div class="min-w-0 {message.role === 'user' ? 'user-message' : ''}">
                {#each message.blocks as block, i (i)}
                  {#if block.type === "text"}<MessageBody text={block.markdown} />{/if}
                {/each}
              </div>
              {#if message.activity}<details class="rounded-lg bg-fill p-3"><summary class="cursor-pointer text-sm">Recorded tool activity</summary><pre class="max-h-80 overflow-auto text-xs whitespace-pre-wrap">{message.activity}</pre></details>{/if}
              {/if}
              <button type="button" class="self-start text-xs text-muted" onclick={() => copy(message)}>Copy</button>
              {#each memories.data?.records ?? [] as record (record.id)}
                {@const revision = record.revisions.find(revision => revision.revision === record.currentRevision)}
                {#if revision?.evidence.some(source => source.messageId === message.id)}
                  <a class="citation-note" href={recordHref(record.id, record.node, recordsHref(memoryParams))}>Cited in {label(revision.details.kind).toLowerCase()}: {revision.statement}</a>
                {/if}
              {/each}
            </li>
          {/each}
        </ol>
        {#if !anchor}
          <nav aria-label="Message pages" class="flex gap-5">
            <button type="button" disabled={offset === 0} onclick={() => page(Math.max(0, offset - PAGE_SIZE))}>Previous messages</button>
            <button type="button" disabled={!loaded.messages.length} onclick={() => page(offset + PAGE_SIZE)}>Next messages</button>
          </nav>
        {/if}
      {/if}
    </main>
    <aside class="reader-memory" aria-label="Chat memory">
      <h2><a href={recordsHref(memoryParams)}>Memory</a></h2>
      {#if memories.isPending}<p>Loading memory…</p>{/if}
      {#if memories.error}<p role="alert">{memories.error.message} <button type="button" onclick={() => memories.refetch()}>Retry</button></p>{/if}
      {#each memories.data?.records ?? [] as record (record.id)}
        {@const revision = record.revisions.find(revision => revision.revision === record.currentRevision)}
        {#if revision}<a class="memory-aside-link" href={recordHref(record.id, record.node, recordsHref(memoryParams))}><span>{revision.statement}</span><small>{label(revision.details.kind)} · {label(revision.details.state)}</small></a>{/if}
      {/each}
      {#if memories.data && !memories.data.records.length}<p>No memory cites this chat yet.</p>{/if}
      <a class="text-xs" href={recordsHref(memoryParams)}>Browse chat memory</a>
      <h2>Source</h2><p>{displayMachine}<br />{chat?.project ?? ""}</p>
    </aside>
</div>
<nav class="reader-dock" aria-label="Reading tools">
  <button type="button" onclick={() => outlineOpen = !outlineOpen}><List size={17} />Outline</button>
  <a href={recordsHref(memoryParams)}><Layers size={17} />Memory</a>
  <button type="button" onclick={() => { const element = document.getElementById("reader-export"); if (element instanceof HTMLDetailsElement) { element.open = !element.open; element.scrollIntoView({ block: "center" }); } }}><Download size={17} />Export</button>
</nav>
