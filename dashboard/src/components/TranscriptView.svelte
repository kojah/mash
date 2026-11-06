<script lang="ts">
import Search from "@lucide/svelte/icons/search";
import { createQuery } from "@tanstack/svelte-query";
import { tick } from "svelte";
import { agents } from "../lib/agents";
import { context, type MessagePage, PAGE_SIZE, transcript } from "../lib/api";
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
let anchor = $state("");
let searchOpen = $state(false);
let notice = $state("");
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
});
const key = $derived(JSON.stringify([id, machine, offset, anchor]));
const live = createQuery(() => ({
  queryKey: ["transcript", id, machine, offset, anchor],
  enabled: !anchor || catalogue.data !== undefined,
  queryFn: ({ signal }) =>
    anchor
      ? context(anchor, machine, catalogue.data?.localNode ?? "", signal)
      : transcript(id, machine, offset, signal),
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
  return (
    message.blocks
      .map((block) =>
        block.type === "text" ? block.markdown : block.type === "code" ? block.code : "",
      )
      .join("\n\n") + (message.activity ? `\n\n${message.activity}` : "")
  );
}
async function copy(message: Message): Promise<void> {
  try {
    await navigator.clipboard.writeText(plainText(message));
    notice = "Copied message.";
  } catch {
    notice = "Copy failed. Select the message text to copy it manually.";
  }
}
function exportPage(): void {
  if (!loaded) return;
  const text = `# ${chat?.title ?? id}\n\nMachine: ${machine}\n\nExport of the displayed page only.\n\n${loaded.messages.map((message) => `## ${message.role} · ${message.at}\n\n${plainText(message)}`).join("\n\n")}`;
  const url = URL.createObjectURL(new Blob([text], { type: "text/markdown;charset=utf-8" }));
  const link = document.createElement("a");
  link.href = url;
  link.download = "mash-chat-page.md";
  link.click();
  URL.revokeObjectURL(url);
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
    <aside aria-label="Recent sessions" class="sticky top-6 max-h-[90vh] overflow-auto max-[700px]:hidden">
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
          <button type="button" disabled={!loaded} onclick={exportPage}>Export this page as Markdown</button>
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
