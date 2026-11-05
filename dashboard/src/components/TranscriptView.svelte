<script lang="ts">
import Check from "@lucide/svelte/icons/check";
import ChevronDown from "@lucide/svelte/icons/chevron-down";
import Code from "@lucide/svelte/icons/code";
import Copy from "@lucide/svelte/icons/copy";
import Download from "@lucide/svelte/icons/download";
import FileCode from "@lucide/svelte/icons/file-code";
import FileText from "@lucide/svelte/icons/file-text";
import Search from "@lucide/svelte/icons/search";
import { agents as agentInfo } from "../lib/agents";
import { chats, transcript } from "../lib/api";
import { chatHref } from "../lib/route.svelte";
import type { ChatSummary, Message, Transcript } from "../lib/types";
import AgentMark from "./AgentMark.svelte";
import CodeBlock from "./CodeBlock.svelte";
import SearchPanel from "./SearchPanel.svelte";
import ToolCard from "./ToolCard.svelte";

let { id }: { id: string } = $props();

let sessions = $state<readonly ChatSummary[]>([]);
let loaded = $state<Transcript | undefined>(undefined);
let searchOpen = $state(false);
let exportOpen = $state(false);
let copiedId = $state<string | null>(null);

$effect(() => {
  chats().then((all) => {
    sessions = all;
  });
});
$effect(() => {
  const wanted = id;
  transcript(wanted).then((found) => {
    if (wanted === id) loaded = found;
  });
});

const chat = $derived(loaded?.chat ?? sessions.find((candidate) => candidate.id === id));

function plainText(message: Message): string {
  return message.blocks
    .map((block) =>
      block.type === "text" ? block.markdown : block.type === "code" ? block.code : "",
    )
    .filter((part) => part !== "")
    .join("\n\n");
}

async function copyReply(message: Message): Promise<void> {
  await navigator.clipboard?.writeText(plainText(message));
  copiedId = message.id;
  setTimeout(() => {
    if (copiedId === message.id) copiedId = null;
  }, 1500);
}

const exports = [
  { label: "Export to PDF", icon: FileText },
  { label: "Export to HTML", icon: Code },
  { label: "Export to Markdown", icon: FileCode },
];
</script>

<svelte:window onkeydown={(event) => event.key === "Escape" && ((searchOpen = false), (exportOpen = false))} />

<div class="min-h-screen px-5 pt-6 pb-16">
  <header class="mx-auto mb-7 flex max-w-[1120px] items-center gap-5">
    <a href="#/" aria-label="Mash home" class="inline-flex size-10 shrink-0 items-center justify-center">
      <img src="/mash-logo.png" alt="" width="34" height="31" class="block" />
    </a>
    <div class="relative flex max-w-[560px] flex-1">
      <button type="button" onclick={() => (searchOpen = true)} class="flex min-h-10 flex-1 cursor-text items-center gap-2 rounded-[10px] border-0 bg-fill px-2.5 text-left text-sm text-faint">
        <Search size={15} strokeWidth={1.75} />Search chats
      </button>
      {#if searchOpen}
        <button type="button" aria-label="Close search" class="fixed inset-0 z-10 cursor-default border-0 bg-transparent" onclick={() => (searchOpen = false)}></button>
        <div role="dialog" aria-label="Search" class="absolute -top-1.5 -left-1.5 z-20 max-h-[70vh] w-[min(640px,calc(100vw-32px))] rounded-2xl bg-surface p-2 shadow-[0_16px_48px_rgba(0,0,0,0.14),0_0_0_0.5px_rgba(0,0,0,0.08)]">
          <SearchPanel compact onClose={() => (searchOpen = false)} />
        </div>
      {/if}
    </div>
  </header>

  <div class="mx-auto grid max-w-[1120px] items-start gap-10 [grid-template-columns:260px_minmax(0,1fr)] max-[700px]:[grid-template-columns:minmax(0,1fr)]">
    <aside aria-label="Sessions" class="sticky top-6 flex min-w-0 flex-col gap-0.5 max-[700px]:hidden">
      {#each sessions as session (session.id)}
        {@const current = session.id === id}
        <a href={chatHref(session.id)} aria-current={current ? "page" : undefined} class="flex min-h-[46px] items-center gap-2.5 rounded-[10px] px-2.5 py-1.5 no-underline {current ? 'bg-accent text-white' : 'text-ink hover:bg-fill/60'}">
          <span class="inline-flex size-7 shrink-0 items-center justify-center rounded-full {current ? 'bg-white/90' : 'bg-fill'}"><AgentMark agent={session.agent} size={15} /></span>
          <span class="flex min-w-0 flex-1 flex-col">
            <span class="flex items-baseline gap-2"><span class="min-w-0 flex-1 truncate text-sm font-medium">{session.title}</span><span class="shrink-0 text-xs {current ? 'text-white/85' : 'text-faint'}">{session.updatedAt}</span></span>
            <span class="truncate text-xs {current ? 'text-white/85' : 'text-faint'}">{session.machine} · {session.project}</span>
          </span>
        </a>
      {/each}
    </aside>

    <main class="flex min-w-0 max-w-[760px] flex-col gap-6">
      {#if chat}
        <header class="flex flex-wrap items-start gap-x-4 gap-y-3">
          <div class="flex min-w-[240px] flex-1 flex-col gap-2">
            <h1 class="m-0 text-[28px] font-semibold tracking-[-0.022em]">{chat.title}</h1>
            <p class="m-0 flex flex-wrap gap-x-1.5 gap-y-0.5 text-sm text-muted">
              <span class="whitespace-nowrap">{agentInfo[chat.agent].name}</span>·<span class="whitespace-nowrap">{chat.machine}</span>·<span class="whitespace-nowrap">{chat.project}</span>·<span class="whitespace-nowrap">{chat.messageCount} messages</span>
            </p>
            <p class="m-0 text-[13px] whitespace-nowrap text-faint"><kbd class="rounded-[5px] border border-b-2 border-line bg-surface px-1.5 py-px font-sans text-xs text-ink-2">⌘F</kbd> to find in this chat</p>
          </div>
          <div class="relative">
            <button type="button" aria-haspopup="menu" aria-expanded={exportOpen} onclick={() => (exportOpen = !exportOpen)} class="inline-flex min-h-11 items-center gap-2 rounded-[10px] border border-line bg-surface px-3.5 text-sm text-ink">
              <Download size={16} strokeWidth={1.75} />Export<ChevronDown size={14} strokeWidth={1.75} class="text-faint" />
            </button>
            {#if exportOpen}
              <ul role="menu" class="absolute top-[50px] right-0 z-10 m-0 min-w-[220px] list-none rounded-xl bg-surface p-1.5 shadow-[0_12px_32px_rgba(0,0,0,0.16),0_0_0_0.5px_rgba(0,0,0,0.06)]">
                {#each exports as item (item.label)}
                  <li role="none">
                    <button type="button" role="menuitem" onclick={() => (exportOpen = false)} class="flex min-h-11 w-full items-center gap-2.5 rounded-lg px-2.5 text-left text-sm text-ink hover:bg-ground">
                      <item.icon size={16} strokeWidth={1.75} class="text-faint" />{item.label}
                    </button>
                  </li>
                {/each}
              </ul>
            {/if}
          </div>
        </header>

        {#if loaded}
          <ol class="m-0 flex list-none flex-col gap-4 p-0">
            {#each loaded.messages as message (message.id)}
              {#if message.role === "user"}
                <li class="flex flex-col items-end gap-1">
                  <span class="text-xs text-faint select-none">{message.at}</span>
                  {#each message.blocks as block, i (i)}
                    {#if block.type === "text"}
                      <p class="m-0 max-w-[78%] rounded-[20px_20px_6px_20px] bg-accent px-3.5 py-2 text-[15px] leading-snug whitespace-pre-wrap text-white">{block.markdown}</p>
                    {/if}
                  {/each}
                </li>
              {:else}
                <li class="flex flex-col gap-3.5 py-1">
                  {#each message.blocks as block, i (i)}
                    {#if block.type === "text"}
                      <p class="m-0 text-[15px] leading-relaxed whitespace-pre-wrap">{block.markdown}</p>
                    {:else if block.type === "code"}
                      <CodeBlock code={block.code} language={block.language} file={block.file} />
                    {:else}
                      <ToolCard calls={block.calls} />
                    {/if}
                  {/each}
                  <div class="-ml-2.5 flex select-none">
                    <button type="button" onclick={() => copyReply(message)} class="inline-flex min-h-8 items-center gap-1.5 rounded-lg px-2.5 text-[13px] text-muted">
                      {#if copiedId === message.id}<Check size={14} strokeWidth={2.25} class="text-ok" /><span aria-live="polite">Copied</span>{:else}<Copy size={14} strokeWidth={1.75} /><span aria-live="polite">Copy</span>{/if}
                    </button>
                  </div>
                </li>
              {/if}
            {/each}
          </ol>
        {:else}
          <p class="m-0 text-sm text-faint">This chat's messages will load from <code class="font-mono">mash dash</code>; the sample data only includes “Flaky watcher test on macOS”.</p>
        {/if}
      {:else}
        <p class="m-0 text-sm text-faint">Chat not found.</p>
      {/if}
    </main>
  </div>
</div>
