<script lang="ts">
import Search from "@lucide/svelte/icons/search";
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
import { catalogOptions } from "../lib/queries";
import { chatHref } from "../lib/route.svelte";
import type { Message } from "../lib/types";
import AgentMark from "./AgentMark.svelte";
import MessageBody from "./MessageBody.svelte";
import SearchPanel from "./SearchPanel.svelte";

let { id, node, messageId }: { id: string; node: string; messageId: string } = $props();
const catalogue = createQuery(() => catalogOptions);
const machine = $derived(node || "");
const displayMachine = $derived(machine || catalogue.data?.localNode || "Local");
const sessions = $derived(catalogue.data?.chats ?? []);
const chat = $derived(sessions.find((chat) => chat.id === id && chat.machine === displayMachine));
let offset = $state(0);
let anchor = $state(untrack(() => messageId));
let searchOpen = $state(false);
let notice = $state("");
let exporting = $state(false);
let exportController: AbortController | undefined;
let snapshot = $state<{ key: string; page: MessagePage } | null>(null);
// Keep the sidebar mounted across chat navigation. Only explicit navigation
// resets the reading page; catalogue polling never touches it.
$effect(() => {
  const route = [id, node, messageId];
  anchor = route[2] ?? "";
  offset = 0;
  notice = "";
  searchOpen = false;
  window.scrollTo({ top: 0, behavior: "instant" });
  return () => exportController?.abort();
});
const key = $derived(JSON.stringify([id, machine, offset, anchor]));
const live = createQuery(() => ({
  queryKey: ["transcript", id, machine, offset, anchor],
  queryFn: ({ signal }) =>
    anchor ? context(anchor, machine, signal) : transcript(id, machine, offset, signal),
  staleTime: 15_000,
  refetchInterval: 30_000,
  refetchIntervalInBackground: false,
  retry: false,
  refetchOnMount: false,
  refetchOnWindowFocus: false,
  refetchOnReconnect: false,
}));
const loaded = $derived(snapshot?.key === key ? snapshot.page : undefined);
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
    const messages = await collectChat(id, machine, controller.signal, (count) => {
      notice = `Preparing export: ${count.toLocaleString()} messages…`;
    });
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

<svelte:window onkeydown={(event) => event.key === "Escape" && (searchOpen = false)} />
<div class="min-h-screen px-5 pt-6 pb-16">
  <header class="mx-auto mb-7 flex max-w-[1120px] items-center gap-5">
    <a href="#/" aria-label="Mash home"><img src="/mash-logo.png" alt="" width="34" height="31" /></a>
    <button type="button" onclick={() => searchOpen = true} class="flex min-h-10 flex-1 items-center gap-2 rounded-[10px] bg-fill px-3 text-left text-sm text-muted"><Search size={15} />Search chats</button>
  </header>
  <div class="mx-auto grid max-w-[1120px] items-start gap-10 [grid-template-columns:260px_minmax(0,1fr)] max-[700px]:[grid-template-columns:minmax(0,1fr)]">
    <aside aria-label="Recent chats" class="sticky top-6 max-h-[90vh] overflow-auto max-[700px]:hidden">
      {#each sessions.slice(0, 100) as session (JSON.stringify([session.machine, session.id]))}
        {@const current = session.id === id && session.machine === displayMachine}
        <a href={chatHref(session.id, session.machine)} title={`${session.title}\n${session.project}\n${session.updatedAt}\n${session.id}`} aria-current={current ? "page" : undefined} class="flex min-h-[46px] items-start gap-2 rounded-[10px] p-2 no-underline {current ? 'bg-accent text-white' : 'text-ink hover:bg-fill'}">
          <AgentMark agent={session.agent} />
          <span class="min-w-0"><span class="line-clamp-2 text-sm font-medium">{session.title}</span><span class="block truncate text-xs">{session.project.split('/').filter(Boolean).at(-1) || 'No project'} · {session.machine}</span><span class="block text-xs opacity-75">{session.updatedAt} · {session.id.slice(-6)}</span></span>
        </a>
      {/each}
    </aside>
    <main class="flex min-w-0 flex-col gap-6">
      <header class="flex flex-col gap-2">
        <h1 class="m-0 text-[28px] font-semibold">{chat?.title ?? `Chat ${id.slice(-8)}`}</h1>
        <p class="m-0 text-sm text-muted">{chat ? agents[chat.agent].name : ""} · {displayMachine} · {chat?.project ?? ""}</p>
        <p class="m-0 text-xs text-muted">{anchor ? "Context around the selected search match" : `Message positions ${offset}–${offset + PAGE_SIZE - 1}`} · Browser Find searches this page. Branches are shown in recorded display order.</p>
        <div class="flex flex-wrap gap-4 text-sm text-accent">
          <details class="relative">
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
            <li id="message-{message.id}" class="flex min-w-0 flex-col gap-2 scroll-mt-8 {message.id === messageId ? 'rounded-lg ring-2 ring-accent p-3' : ''}">
              {#if message.presentation && message.presentation !== "dialogue"}
                <details class="rounded-lg border border-line bg-surface p-3">
                  <summary class="cursor-pointer text-sm"><strong>{message.presentation === 'tool-result' ? 'Tool output' : message.presentation === 'reasoning' ? 'Reasoning' : message.toolLabel || 'Tool call'}</strong><span class="ml-2 text-xs text-muted">{message.at}</span></summary>
                  <p class="text-xs text-muted">Recorded as {message.role} · message {message.ordinal}</p>
                  <pre class="max-h-[32rem] overflow-auto rounded bg-ground p-3 text-xs whitespace-pre-wrap break-words">{plainText(message)}</pre>
                </details>
              {:else}
              <span class="text-xs text-muted">{message.role === 'user' ? 'You' : message.role} · {message.at}</span>
              <div class="min-w-0 {message.role === 'user' ? 'ml-auto max-w-[85%] rounded-2xl bg-accent-soft p-3' : ''}">
                {#each message.blocks as block, i (i)}
                  {#if block.type === "text"}<MessageBody text={block.markdown} />{/if}
                {/each}
              </div>
              {#if message.activity}<details class="rounded-lg bg-fill p-3"><summary class="cursor-pointer text-sm">Recorded tool activity</summary><pre class="max-h-80 overflow-auto text-xs whitespace-pre-wrap">{message.activity}</pre></details>{/if}
              {/if}
              <button type="button" class="self-start text-xs text-muted" onclick={() => copy(message)}>Copy</button>
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
  </div>
</div>
{#if searchOpen}
  <button type="button" aria-label="Close search" class="fixed inset-0 z-10 bg-black/30" onclick={() => searchOpen = false}></button>
  <div role="dialog" aria-modal="true" aria-label="Search" class="fixed top-[12vh] left-1/2 z-20 max-h-[76vh] w-[min(720px,calc(100vw-32px))] -translate-x-1/2 overflow-auto rounded-2xl bg-ground p-6">
    <SearchPanel onClose={() => searchOpen = false} />
  </div>
{/if}
