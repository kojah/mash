<script lang="ts">
import ArrowUp from "@lucide/svelte/icons/arrow-up";
import RotateCw from "@lucide/svelte/icons/rotate-cw";
import Square from "@lucide/svelte/icons/square";
import Trash2 from "@lucide/svelte/icons/trash-2";
import X from "@lucide/svelte/icons/x";
import { createQuery } from "@tanstack/svelte-query";
import { tick } from "svelte";
import { agents } from "../lib/agents";
import { asset } from "../lib/asset";
import { answer, assistantOptions, linkify, type OpenChat } from "../lib/assistant";
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
let circle = $state<HTMLButtonElement>();
// Whether the reader is at the newest words. Scrolled up to read, they stay there.
let following = $state(true);
const empty = $derived(!mashy.turns.length && !mashy.busy && !mashy.note);

// Keep the newest words in view while they arrive, for a reader who is there.
$effect(() => {
  void mashy.answer;
  void mashy.turns.length;
  void mashy.phase;
  void mashy.note;
  if (log && following && !empty) log.scrollTop = log.scrollHeight;
});
function scrolled(): void {
  if (log) following = log.scrollHeight - log.scrollTop - log.clientHeight < 48;
}
// The box grows with what is typed, up to a few lines.
function resize(): void {
  if (!input) return;
  input.style.height = "auto";
  input.style.height = `${Math.min(input.scrollHeight + 2, 132)}px`;
}
const SHEET = "(max-width: 700px), (max-height: 500px)";
// On a phone the chat covers the page, so the Back button belongs to it:
// Back closes the sheet, and Back from a chat it linked to returns to it.
function sheetEntry(): boolean {
  return (history.state as { mashy?: boolean } | null)?.mashy === true;
}
async function show(): Promise<void> {
  mashy.toggle(true);
  following = true;
  await tick();
  input?.focus();
}
async function hide(): Promise<void> {
  mashy.toggle(false);
  await tick();
  circle?.focus();
}
async function open(): Promise<void> {
  if (window.matchMedia(SHEET).matches && !sheetEntry()) history.pushState({ mashy: true }, "");
  await show();
}
async function close(): Promise<void> {
  // Leaving the sheet's own history entry closes it, in `popstate` below.
  if (sheetEntry()) history.back();
  else await hide();
}
// The window can be moved by its header, away from what it covers. Where it
// was left is remembered for this browser; a double click puts it back.
const PLACE = "mashy.place";
let panel = $state<HTMLElement>();
let moved = $state(placed());
let grip: { x: number; y: number; left: number; top: number } | undefined;
function placed(): { x: number; y: number } {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(PLACE) ?? "null");
    if (saved && typeof saved === "object" && "x" in saved && "y" in saved) {
      const { x, y } = saved;
      if (typeof x === "number" && typeof y === "number") return { x, y };
    }
  } catch {
    // Without storage the window starts in its corner.
  }
  return { x: 0, y: 0 };
}
function place(x: number, y: number): void {
  moved = { x, y };
  try {
    localStorage.setItem(PLACE, JSON.stringify(moved));
  } catch {
    // Not remembered, and nothing else depends on it.
  }
}
// Keep the whole window on screen, whatever the offset or the window's size.
function keepIn(): void {
  if (!panel || window.matchMedia(SHEET).matches) return;
  const box = panel.getBoundingClientRect();
  const x = moved.x + Math.min(Math.max(8 - box.left, 0), window.innerWidth - 8 - box.right);
  const y = moved.y + Math.min(Math.max(8 - box.top, 0), window.innerHeight - 8 - box.bottom);
  if (x !== moved.x || y !== moved.y) moved = { x, y };
}
$effect(() => {
  if (panel) keepIn();
});
function grab(event: PointerEvent): void {
  if (event.button !== 0 || window.matchMedia(SHEET).matches) return;
  if (!(event.target instanceof Element) || event.target.closest("button")) return;
  if (!(event.currentTarget instanceof HTMLElement) || !panel) return;
  const box = panel.getBoundingClientRect();
  grip = {
    x: event.clientX - moved.x,
    y: event.clientY - moved.y,
    left: box.left - moved.x,
    top: box.top - moved.y,
  };
  event.currentTarget.setPointerCapture(event.pointerId);
  event.preventDefault();
}
function drag(event: PointerEvent): void {
  if (!grip || !panel) return;
  const low = (edge: number, span: number, limit: number, wanted: number): number =>
    Math.min(Math.max(wanted, 8 - edge), Math.max(8 - edge, limit - 8 - span - edge));
  place(
    low(grip.left, panel.offsetWidth, window.innerWidth, event.clientX - grip.x),
    low(grip.top, panel.offsetHeight, window.innerHeight, event.clientY - grip.y),
  );
}
function drop(): void {
  grip = undefined;
}
async function submit(text: string): Promise<void> {
  if (!text.trim() || mashy.busy) return;
  draft = "";
  following = true;
  void mashy.ask(text, openChat);
  await tick();
  resize();
  input?.focus();
}
// A link to a chat or a memory is shown as a chip: the agent's mark and a
// short name. The marks are drawn here, from the chat list, never from what
// the model wrote.
function chips(node: HTMLElement): { destroy(): void } {
  const draw = (): void => {
    for (const link of node.querySelectorAll<HTMLAnchorElement>(
      "a[href^='#/chat/'], a[href^='#/record/']",
    )) {
      if (link.dataset.chip) continue;
      link.dataset.chip = "1";
      link.classList.add("mashy-chip");
      const href = link.getAttribute("href") ?? "";
      if (!href.startsWith("#/chat/")) {
        link.classList.add("mem");
        continue;
      }
      const id = decodeURIComponent(href.slice(7).split("?", 1)[0] ?? "");
      const chat = catalogue.data?.chats.find((entry) => entry.id === id);
      if (!chat) continue;
      const mark = document.createElement("span");
      mark.className = "mark tint";
      mark.setAttribute("aria-hidden", "true");
      mark.style.backgroundColor = agents[chat.agent].tint;
      mark.style.maskImage = `url(${agents[chat.agent].logo})`;
      mark.style.setProperty("-webkit-mask-image", `url(${agents[chat.agent].logo})`);
      link.prepend(mark);
      link.title = `${plainTitle(chat.title)} · ${agents[chat.agent].name}`;
    }
  };
  draw();
  const observer = new MutationObserver(draw);
  observer.observe(node, { childList: true, subtree: true });
  return { destroy: () => observer.disconnect() };
}
// A link in an answer opens in the dashboard behind; on a phone the sheet
// covers it, so it steps aside.
function followed(event: MouseEvent): void {
  if (!(event.target instanceof Element) || !event.target.closest("a[href^='#/']")) return;
  // The sheet's history entry stays behind the page the link opens.
  if (window.matchMedia(SHEET).matches) mashy.toggle(false);
}
</script>

<svelte:window
  onresize={keepIn}
  onpopstate={() => {
    if (sheetEntry() && window.matchMedia(SHEET).matches) void show();
    else if (mashy.open && window.matchMedia(SHEET).matches) void hide();
  }}
  onkeydown={(event) => {
    // Escape closes Mashy when nothing else took it, such as an open find bar.
    if (event.key !== "Escape" || !mashy.open || event.defaultPrevented) return;
    const focused = document.activeElement;
    if (!focused || focused === document.body || focused.closest("#mashy, #mashy-fab")) void close();
  }}
/>
{#if status.data?.enabled}
  {#if mashy.open}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
    <section id="mashy" class:fresh={empty} aria-label="Mashy" bind:this={panel} style:translate="{moved.x}px {moved.y}px" onclick={followed}>
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <header title="Drag to move" onpointerdown={grab} onpointermove={drag} onpointerup={drop} onpointercancel={drop} ondblclick={(event) => { if (!(event.target instanceof Element) || !event.target.closest("button")) place(0, 0); }}>
        <img src={asset("mashy.png")} alt="" />
        <div><strong>Mashy</strong></div>
        {#if mashy.turns.length}<button type="button" aria-label="Start over" title="Start over" onclick={() => { mashy.clear(); input?.focus(); }}><Trash2 size={16} /></button>{/if}
        <button type="button" aria-label="Close Mashy" title="Close" onclick={() => void close()}><X size={18} /></button>
      </header>
      <div class="mashy-log" bind:this={log} onscroll={scrolled} aria-live="polite" aria-busy={mashy.busy}>
        {#if openChat}<span class="mashy-ctx"><i></i>About: {openChat.title ?? `chat ${openChat.id.slice(-8)}`}</span>{/if}
        {#if empty}
          <div class="mashy-hello">
            <img src={asset("mashy.png")} alt="" />
            <p>Hi, I’m Mashy. Ask me about anything in your past chats{openChat ? ", or about the chat you have open" : ""}.</p>
            <div class="mashy-sugg">
              {#each suggestions as suggestion (suggestion)}<button type="button" onclick={() => void submit(suggestion)}>{suggestion}</button>{/each}
            </div>
          </div>
        {/if}
        {#each mashy.turns as turn, index (index)}
          {#if turn.role === "user"}
            <div class="mashy-q">{turn.content}</div>
          {:else}
            {@const said = answer(turn.content)}
            <div class="mashy-a" use:chips><MessageBody text={linkify(said.body)} /></div>
            {#if said.next.length && index === mashy.turns.length - 1 && !mashy.busy}
              <div class="mashy-sugg next">
                {#each said.next as question (question)}<button type="button" onclick={() => void submit(question)}>{question}</button>{/each}
              </div>
            {/if}
          {/if}
        {/each}
        {#if mashy.answer}<div class="mashy-a" use:chips><MessageBody text={linkify(answer(mashy.answer).body)} /></div>{/if}
        {#if mashy.phase === "searching"}<p class="mashy-wait" role="status">Searching your history…</p>{/if}
        {#if mashy.note}
          <div class="mashy-note" class:bad={mashy.note.kind === "error"} role={mashy.note.kind === "error" ? "alert" : "status"}>
            <strong>{mashy.note.title}</strong>
            {#if mashy.note.text}<span>{mashy.note.text}</span>{/if}
            {#if mashy.note.retry}<button class="btn" type="button" onclick={() => mashy.retry()}><RotateCw size={14} aria-hidden="true" />{mashy.note.kind === "stopped" ? "Ask again" : "Retry"}</button>{/if}
          </div>
        {/if}
      </div>
      <form onsubmit={(event) => { event.preventDefault(); void submit(draft); }}>
        <label class="sr-only" for="mashy-question">Ask Mashy about your past chats</label>
        <textarea
          id="mashy-question"
          rows="1"
          enterkeyhint="send"
          placeholder={mashy.busy ? "Mashy is answering…" : "Ask Mashy about your past chats"}
          bind:this={input}
          bind:value={draft}
          oninput={resize}
          onkeydown={(event) => {
            if (event.key === "Enter" && !event.shiftKey && !event.isComposing) {
              event.preventDefault();
              void submit(draft);
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
  <button id="mashy-fab" bind:this={circle} type="button" aria-label={mashy.open ? "Close Mashy" : "Open Mashy"} aria-expanded={mashy.open} onclick={() => void (mashy.open ? close() : open())}>
    <img src={asset("mashy.png")} alt="" />
    {#if mashy.unread}<span class="dot"></span><span class="sr-only">New answer</span>{/if}
  </button>
{/if}
