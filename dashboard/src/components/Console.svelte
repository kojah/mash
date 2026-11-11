<script lang="ts">
import ChevronDown from "@lucide/svelte/icons/chevron-down";
import Copy from "@lucide/svelte/icons/copy";
import Lock from "@lucide/svelte/icons/lock";
import Play from "@lucide/svelte/icons/play";
import SearchIcon from "@lucide/svelte/icons/search";
import { createQuery } from "@tanstack/svelte-query";
import { read } from "../lib/api";
import {
  type Answer,
  argumentsFor,
  groupOrder,
  headline,
  locked,
  parseAnswer,
  parseConsole,
  sentences,
  summarize,
  type Tool,
} from "../lib/console";
import { stableRead } from "../lib/queries";
import JsonView from "./JsonView.svelte";
import QueryError from "./QueryError.svelte";
import Skeleton from "./Skeleton.svelte";

// The tools an agent can call on Mash, listed from the server's own
// description of them, with a form to run the ones that only read. The server
// refuses the rest, whatever this page sends.
const listing = createQuery(() => ({
  ...stableRead,
  queryKey: ["console-tools"],
  queryFn: async ({ signal }) =>
    parseConsole(await read("/api/console/tools", new URLSearchParams(), signal)),
}));
let filter = $state("");
let open = $state("");
let allParams = $state(false);
let values = $state<Record<string, string>>({});
type Outcome =
  | { readonly state: "idle" }
  | { readonly state: "running" }
  | { readonly state: "done"; readonly answer: Answer; readonly ms: number; readonly bytes: number }
  | { readonly state: "unreachable"; readonly message: string };
let outcome = $state<Outcome>({ state: "idle" });
let controller: AbortController | undefined;
let notice = $state("");

const tools = $derived(listing.data?.tools ?? []);
const matching = $derived.by(() => {
  const words = filter.trim().toLowerCase();
  return words
    ? tools.filter((tool) => `${tool.name} ${tool.about}`.toLowerCase().includes(words))
    : tools;
});
const sections = $derived(
  groupOrder
    .map((name) => ({ name, tools: matching.filter((tool) => tool.group === name) }))
    .filter((section) => section.tools.length),
);
const runnable = $derived(tools.filter((tool) => tool.readOnly).length);
const tool = $derived(tools.find((entry) => entry.name === open));
const request = $derived(tool ? argumentsFor(tool, values) : null);
const summary = $derived(outcome.state === "done" ? summarize(outcome.answer) : null);
const FEW = 6;

function toggle(name: string): void {
  controller?.abort();
  open = open === name ? "" : name;
  allParams = false;
  outcome = { state: "idle" };
  notice = "";
}
function shownParams(entry: Tool) {
  if (allParams || entry.params.length <= FEW) return entry.params;
  return entry.params.filter(
    (param) => param.required || (values[`${entry.name}.${param.name}`] ?? "").trim(),
  );
}
async function run(event: SubmitEvent): Promise<void> {
  event.preventDefault();
  if (!tool || !request?.ok) return;
  controller?.abort();
  const current = new AbortController();
  controller = current;
  outcome = { state: "running" };
  notice = "";
  const started = performance.now();
  try {
    const body = await read(
      "/api/console/call",
      new URLSearchParams({ name: tool.name, arguments: JSON.stringify(request.arguments) }),
      current.signal,
    );
    const answer = parseAnswer(body);
    outcome = {
      state: "done",
      answer,
      ms: Math.round(performance.now() - started),
      bytes: new Blob([JSON.stringify(answer.reply)]).size,
    };
  } catch (error) {
    if (current.signal.aborted) return;
    outcome = {
      state: "unreachable",
      message: error instanceof Error ? error.message : "The request failed.",
    };
  }
}
async function copy(text: string, what: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
    notice = `${what} copied.`;
  } catch {
    notice = "Copy failed. Select the text to copy it.";
  }
}
function size(bytes: number): string {
  return bytes < 1024 ? `${bytes} B` : `${(bytes / 1024).toFixed(bytes < 10_240 ? 1 : 0)} KB`;
}
$effect(() => () => controller?.abort());
</script>

<div class="hd" style:align-items="flex-end">
  <div style:min-width="0">
    <h1 class="h1">MCP console</h1>
    <p class="meta" style:max-width="64ch">Every tool your agents can call on Mash, with what it takes and what it returns. Run the read-only ones here and see exactly what comes back.</p>
  </div>
  {#if listing.data}
    <span class="mts"><span class="mt">build <code>{listing.data.build.slice(0, 8)}</code></span><span class="mt">contract <code>{listing.data.contract.slice(0, 8)}</code></span></span>
  {/if}
</div>
<QueryError title="Couldn’t load the tool list" query={listing} />
{#if listing.isPending}<Skeleton rows={5} />{/if}
{#if listing.data}
  <div class="pctl">
    <label class="op-find"><SearchIcon size={16} aria-hidden="true" /><input type="search" aria-label="Filter tools" placeholder="Filter tools" autocomplete="off" bind:value={filter} /></label>
    <span class="meta" role="status">{tools.length} tools · {runnable} can run here · {tools.length - runnable} change data and are locked</span>
  </div>
  {#if !sections.length}<div class="state-c"><p>No tool matches “{filter}”.</p></div>{/if}
  {#each sections as section (section.name)}
    <section class="ops" aria-label={section.name}>
      <h2 class="op-g g-{section.name === locked ? 'locked' : section.name.toLowerCase()}">{section.name === locked ? "Changes data · locked" : section.name}</h2>
      {#each section.tools as entry (entry.name)}
        {@const expanded = open === entry.name}
        <div class="mop" class:open={expanded} class:locked={!entry.readOnly} id="tool-{entry.name}">
          <button class="op-h" type="button" aria-expanded={expanded} onclick={() => toggle(entry.name)}>
            <span class="badge {entry.readOnly ? 'read' : 'write'}">{entry.readOnly ? "READ" : "WRITE"}</span>
            <code class="op-n">{entry.name}</code>
            <span class="op-a">{headline(entry.about)}</span>
            {#if !entry.readOnly}<Lock size={14} aria-hidden="true" />{/if}
            <ChevronDown size={18} aria-hidden="true" class="op-c" />
          </button>
          {#if expanded}
            <form class="op-b" onsubmit={run}>
              {#if sentences(entry.about).length > 1}
                <ul class="op-d">{#each sentences(entry.about).slice(1) as point, index (index)}<li>{point}</li>{/each}</ul>
              {/if}
              {#if !entry.readOnly}<p class="meta">This tool changes data, so it can be run by an agent or from the command line, not from the dashboard. Its parameters are listed for reference.</p>{/if}
              <div>
                <h3 class="op-t">Parameters</h3>
                {#if !entry.params.length}<p class="meta">This tool takes no parameters.</p>{/if}
                <div class="prms">
                  {#each shownParams(entry) as param (param.name)}
                    {@const key = `${entry.name}.${param.name}`}
                    <label class="prm" class:bad={request && !request.ok && request.param === param.name && outcome.state !== "idle"}>
                      <span class="prm-n"><code>{param.name}</code><small>{param.type}</small>{#if param.required}<b>required</b>{/if}</span>
                      <span class="prm-a">{param.about}{#if param.fallback} <small>Default {param.fallback}.</small>{/if}</span>
                      {#if !entry.readOnly}<span></span>
                      {:else if param.kind === "enum" || param.kind === "boolean"}
                        <select class="prm-i" bind:value={values[key]}>
                          <option value="">{param.required ? "choose…" : "not set"}</option>
                          {#each param.kind === "boolean" ? ["true", "false"] : param.choices as choice (choice)}<option value={choice}>{choice}</option>{/each}
                        </select>
                      {:else}
                        <input class="prm-i" autocomplete="off" autocapitalize="off" spellcheck="false" inputmode={param.kind === "integer" ? "numeric" : undefined} placeholder={param.kind === "json" ? "JSON" : param.required ? "" : "not set"} bind:value={values[key]} />
                      {/if}
                    </label>
                  {/each}
                </div>
                {#if entry.params.length > FEW}
                  <button class="a" type="button" style:min-height="36px" onclick={() => (allParams = !allParams)}>{allParams ? "Show fewer parameters" : `Show all ${entry.params.length} parameters`}</button>
                {/if}
              </div>
              {#if entry.readOnly}
                <div class="op-r">
                  <button class="btn p" type="submit" disabled={outcome.state === "running" || !request?.ok}><Play size={15} aria-hidden="true" />{outcome.state === "running" ? "Running…" : "Run"}</button>
                  <button class="btn" type="button" onclick={() => { for (const param of entry.params) delete values[`${entry.name}.${param.name}`]; }}>Clear values</button>
                  {#if request && !request.ok}<span class="meta">{request.message}</span>{/if}
                </div>
                {#if request?.ok}
                  {@const call = JSON.stringify({ name: entry.name, arguments: request.arguments }, null, 2)}
                  <div>
                    <div class="op-th"><h3 class="op-t">Request</h3><button class="btn" type="button" onclick={() => copy(call, "Request")}><Copy size={14} aria-hidden="true" />Copy</button></div>
                    <div class="json"><JsonView value={{ name: entry.name, arguments: request.arguments }} /></div>
                  </div>
                {/if}
                {#if outcome.state === "unreachable"}<p class="fail" role="alert">{outcome.message}</p>{/if}
                {#if outcome.state === "done" && summary}
                  {@const reply = JSON.stringify(outcome.answer.reply, null, 2)}
                  <section aria-label="Response">
                    <div class="op-th">
                      <h3 class="op-t">Response</h3>
                      <span class="pill {summary.state}"><i class="dot {summary.state === 'ok' ? '' : summary.state === 'busy' ? 'warn' : 'bad'}"></i>{summary.state === "ok" ? "OK" : summary.state === "busy" ? "Busy" : "Failed"}</span>
                      <span class="meta">{[summary.items === null ? "" : `${summary.items.toLocaleString()} item${summary.items === 1 ? "" : "s"}`, `${outcome.ms.toLocaleString()} ms`, size(outcome.bytes)].filter(Boolean).join(" · ")}</span>
                      <button class="btn" type="button" style:margin-left="auto" onclick={() => copy(reply, "JSON")}><Copy size={14} aria-hidden="true" />Copy JSON</button>
                    </div>
                    {#if summary.heading}<p class="op-e" role="alert">{summary.heading}</p>{/if}
                    {#if outcome.answer.reply !== null}<div class="json"><JsonView value={outcome.answer.reply} /></div>
                    {:else if outcome.answer.text}<div class="json"><pre>{outcome.answer.text}</pre></div>{/if}
                    {#if summary.state === "busy"}<p class="meta">Another Mash operation is using the index. It usually clears within a minute; run it again.</p>{/if}
                  </section>
                {/if}
                {#if notice}<p class="meta" role="status">{notice}</p>{/if}
              {/if}
            </form>
          {/if}
        </div>
      {/each}
    </section>
  {/each}
{/if}
