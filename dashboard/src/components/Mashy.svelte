<script lang="ts">
import ArrowUp from "@lucide/svelte/icons/arrow-up";
import Brain from "@lucide/svelte/icons/brain";
import MessagesSquare from "@lucide/svelte/icons/messages-square";
import RotateCw from "@lucide/svelte/icons/rotate-cw";
import Square from "@lucide/svelte/icons/square";
import Trash2 from "@lucide/svelte/icons/trash-2";
import X from "@lucide/svelte/icons/x";
import { createQuery } from "@tanstack/svelte-query";
import { tick } from "svelte";
import { asset } from "../lib/asset";
import { assistantOptions, linkify, type OpenChat, sources } from "../lib/assistant";
import { plainTitle } from "../lib/markdown";
import { mashy } from "../lib/mashy.svelte";
import { catalogOptions } from "../lib/queries";
import { router } from "../lib/route.svelte";
import MessageBody from "./MessageBody.svelte";

// There is no Mashy unless this dashboard was given a model to ask.
const status = createQuery(() => assistantOptions);
const catalogue = createQuery(() => catalogOptions);
const route = $derived(router.current);
// On a chat's page, "this chat" means the one on screen.
const openChat = $derived.by((): OpenChat | undefined => {
  if (route.page !== "chat") return undefined;
  const machine = route.node || catalogue.data?.localNode || "";
  const title = catalogue.data?.chats.find(
    (chat) => chat.id === route.id && chat.machine === machine,
  )?.title;
  return {
    id: route.id,
    ...(route.node ? { node: route.node } : {}),
    ...(route.shared ? { shared: true } : {}),
    ...(title ? { title: plainTitle(title) } : {}),
  };
});
const suggestions = $derived(
  openChat
    ? ["Summarise this chat", "What was decided here?", "What was left unfinished?"]
    : [
        "What did I work on yesterday?",
        "What have I decided recently?",
        "Which chats were about the dashboard?",
      ],
);
let draft = $state("");
let log = $state<HTMLElement>();
let input = $state<HTMLTextAreaElement>();
const empty = $derived(!mashy.turns.length && !mashy.busy && !mashy.note);

// Keep the newest words in view while they arrive.
$effect(() => {
  void mashy.answer;
  void mashy.turns.length;
  void mashy.phase;
  void mashy.note;
  if (log) log.scrollTop = log.scrollHeight;
});
async function open(): Promise<void> {
  mashy.toggle(true);
  await tick();
  input?.focus();
}
function submit(text: string): void {
  if (!text.trim() || mashy.busy) return;
  draft = "";
  void mashy.ask(text, openChat);
}
// A link in an answer opens in the dashboard behind; on a phone the sheet
// covers it, so it steps aside.
function followed(event: MouseEvent): void {
  if (!(event.target instanceof Element) || !event.target.closest("a[href^='#/']")) return;
  if (window.matchMedia("(max-width: 700px)").matches) mashy.toggle(false);
}
</script>

<svelte:window
  onkeydown={(event) => {
    if (event.key === "Escape" && mashy.open) mashy.toggle(false);
  }}
/>
{#if status.data?.enabled}
  {#if mashy.open}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
    <section id="mashy" class:fresh={empty} aria-label="Mashy" onclick={followed}>
      <header>
        <img src={asset("mashy.png")} alt="" />
        <div><strong>Mashy</strong><small>reads your chats and memories · changes nothing</small></div>
        {#if mashy.turns.length}<button type="button" aria-label="Start over" title="Start over" onclick={() => mashy.clear()}><Trash2 size={16} /></button>{/if}
        <button type="button" aria-label="Close Mashy" title="Close" onclick={() => mashy.toggle(false)}><X size={18} /></button>
      </header>
      <div class="mashy-log" bind:this={log} aria-live="polite">
        {#if openChat}<span class="mashy-ctx"><i></i>About: {openChat.title ?? `chat ${openChat.id.slice(-8)}`}</span>{/if}
        {#if empty}
          <div class="mashy-hello">
            <img src={asset("mashy.png")} alt="" />
            <p>Hi, I’m Mashy. Ask me about anything in your past chats{openChat ? ", or about the chat you have open" : ""}.</p>
            <div class="mashy-sugg">
              {#each suggestions as suggestion (suggestion)}<button type="button" onclick={() => submit(suggestion)}>{suggestion}</button>{/each}
            </div>
          </div>
        {/if}
        {#each mashy.turns as turn, index (index)}
          {#if turn.role === "user"}
            <div class="mashy-q">{turn.content}</div>
          {:else}
            {@const cited = sources(turn.content)}
            <div class="mashy-a"><MessageBody text={linkify(turn.content)} /></div>
            {#if cited.length}
              <div class="mashy-src">
                {#each cited as source (source.href)}
                  <a href={source.href}>{#if source.kind === "chat"}<MessagesSquare size={14} aria-hidden="true" />{:else}<Brain size={14} aria-hidden="true" />{/if}<span>{source.title}</span></a>
                {/each}
              </div>
            {/if}
          {/if}
        {/each}
        {#if mashy.answer}<div class="mashy-a"><MessageBody text={linkify(mashy.answer)} /></div>{/if}
        {#if mashy.phase === "searching"}<p class="mashy-wait" role="status">Searching your history…</p>{/if}
        {#if mashy.note}
          <div class="mashy-note" class:bad={mashy.note.kind === "error"} role={mashy.note.kind === "error" ? "alert" : "status"}>
            <strong>{mashy.note.title}</strong>
            <span>{mashy.note.text}</span>
            {#if mashy.note.retry}<button class="btn" type="button" onclick={() => mashy.retry()}><RotateCw size={14} aria-hidden="true" />Retry</button>{/if}
          </div>
        {/if}
      </div>
      <form onsubmit={(event) => { event.preventDefault(); submit(draft); }}>
        <label class="sr-only" for="mashy-question">Ask Mashy about your past chats</label>
        <textarea
          id="mashy-question"
          rows="1"
          enterkeyhint="send"
          placeholder={mashy.busy ? "Mashy is answering…" : "Ask Mashy about your past chats"}
          bind:this={input}
          bind:value={draft}
          onkeydown={(event) => {
            if (event.key === "Enter" && !event.shiftKey && !event.isComposing) {
              event.preventDefault();
              submit(draft);
            }
          }}
        ></textarea>
        {#if mashy.busy}
          <button class="go" type="button" aria-label="Stop" title="Stop" onclick={() => mashy.stop()}><Square size={15} /></button>
        {:else}
          <button class="go" type="submit" aria-label="Send" title="Send" disabled={!draft.trim()}><ArrowUp size={17} /></button>
        {/if}
      </form>
    </section>
  {/if}
  <button id="mashy-fab" type="button" aria-label={mashy.open ? "Close Mashy" : "Open Mashy"} aria-expanded={mashy.open} onclick={() => (mashy.open ? mashy.toggle(false) : void open())}>
    <img src={asset("mashy.png")} alt="" />
    {#if mashy.unread}<span class="dot"></span>{/if}
  </button>
{/if}
