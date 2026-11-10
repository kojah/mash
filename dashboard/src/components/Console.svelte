<script lang="ts">
import Copy from "@lucide/svelte/icons/copy";
import Play from "@lucide/svelte/icons/play";
import { read } from "../lib/api";

// Runs the read-only lookups behind Mash's agent tools, through this
// dashboard's own read routes, and shows exactly what came back. Nothing
// here can change history, memory or services.
interface Tool {
  readonly name: string;
  readonly about: string;
  readonly path: string;
  readonly fields: readonly { readonly name: string; readonly hint: string }[];
}
const tools: readonly Tool[] = [
  {
    name: "search",
    about: "Find chats by what was said in them.",
    path: "/api/search",
    fields: [
      { name: "q", hint: "words to look for" },
      { name: "provider", hint: "claude-code, codex, … (optional)" },
      { name: "machine", hint: "a machine name (optional)" },
      { name: "days", hint: "only the last N days (optional)" },
    ],
  },
  {
    name: "list_chats",
    about: "List indexed chats, newest first.",
    path: "/api/chats",
    fields: [],
  },
  {
    name: "view",
    about: "Read a message and the replies around it.",
    path: "/api/view",
    fields: [
      { name: "id", hint: "a message ID from a search result" },
      { name: "context", hint: "messages either side, e.g. 4" },
    ],
  },
  {
    name: "read_chat",
    about: "Read a page of one chat's messages in order.",
    path: "/api/chat",
    fields: [
      { name: "id", hint: "a chat ID" },
      { name: "offset", hint: "first message position, e.g. 0" },
      { name: "limit", hint: "how many, up to 100" },
    ],
  },
  {
    name: "list_records",
    about: "List saved memory: decisions, preferences, facts and open questions.",
    path: "/api/records",
    fields: [
      { name: "q", hint: "words in the statement (optional)" },
      { name: "kind", hint: "decision, preference, fact or open_question (optional)" },
    ],
  },
  {
    name: "get_record",
    about: "Read one memory record with its evidence and history.",
    path: "/api/record",
    fields: [{ name: "id", hint: "a record ID" }],
  },
  {
    name: "status",
    about: "Report index readiness and the resolved configuration.",
    path: "/api/status",
    fields: [],
  },
];
let selected = $state(tools[0]?.name ?? "search");
const tool = $derived(tools.find((entry) => entry.name === selected) ?? tools[0]);
let values = $state<Record<string, string>>({});
type Outcome =
  | { readonly state: "idle" }
  | { readonly state: "running" }
  | { readonly state: "done"; readonly call: string; readonly json: string; readonly ms: number }
  | { readonly state: "failed"; readonly call: string; readonly message: string };
let outcome = $state<Outcome>({ state: "idle" });
let controller: AbortController | undefined;
let notice = $state("");
const shownLimit = 200_000;

function choose(name: string): void {
  controller?.abort();
  selected = name;
  outcome = { state: "idle" };
  notice = "";
}
async function run(event: SubmitEvent): Promise<void> {
  event.preventDefault();
  if (!tool) return;
  controller?.abort();
  const current = new AbortController();
  controller = current;
  const params = new URLSearchParams();
  for (const field of tool.fields) {
    const value = (values[`${tool.name}.${field.name}`] ?? "").trim();
    if (value) params.set(field.name, value);
  }
  const call = `${tool.name}(${[...params].map(([name, value]) => `${name}: "${value}"`).join(", ")})`;
  outcome = { state: "running" };
  notice = "";
  const started = performance.now();
  try {
    const body = await read(tool.path, params, current.signal);
    outcome = {
      state: "done",
      call,
      json: JSON.stringify(body, null, 2),
      ms: Math.round(performance.now() - started),
    };
  } catch (error) {
    if (current.signal.aborted) return;
    outcome = {
      state: "failed",
      call,
      message: error instanceof Error ? error.message : "The request failed.",
    };
  }
}
async function copy(json: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(json);
    notice = "JSON copied.";
  } catch {
    notice = "Copy failed. Select the text to copy it.";
  }
}
$effect(() => () => controller?.abort());
</script>

<div><h1 class="h1">MCP console</h1><p class="meta" style:font-size="12.5px">Run the read-only lookups your agents use and see exactly what comes back. Nothing here changes history, memory or services.</p></div>
<div class="cols">
  <nav class="tl" aria-label="Tools">
    {#each tools as entry (entry.name)}<button class:on={entry.name === selected} type="button" aria-current={entry.name === selected ? "true" : undefined} onclick={() => choose(entry.name)}>{entry.name}</button>{/each}
  </nav>
  {#if tool}
    <form onsubmit={run} style:flex="999 1 420px" style:min-width="0" style:display="flex" style:flex-direction="column" style:gap="14px">
      <div><h2 style:font-size="16px">{tool.name}</h2><p class="meta" style:font-size="12.5px">{tool.about}</p></div>
      {#each tool.fields as field (field.name)}
        <label style:display="flex" style:flex-wrap="wrap" style:align-items="center" style:gap="4px 14px">
          <span class="meta" style:flex="0 0 90px" style:font-size="12.5px">{field.name}</span>
          <input class="field" style:flex="1 1 220px" placeholder={field.hint} autocomplete="off" bind:value={values[`${tool.name}.${field.name}`]} />
        </label>
      {/each}
      <div><button class="btn p" type="submit" disabled={outcome.state === "running"}><Play size={15} aria-hidden="true" />{outcome.state === "running" ? "Running…" : "Run"}</button></div>
      {#if outcome.state === "running"}<p class="meta" role="status">Running…</p>{/if}
      {#if outcome.state === "failed"}
        <section aria-label="Tool response" class="tool">
          <div class="hd2"><i class="dot bad"></i><span class="nm"><b>{outcome.call}</b></span></div>
          <p class="fail" role="alert" style:font-size="12.5px">{outcome.message}</p>
        </section>
      {/if}
      {#if outcome.state === "done"}
        <section aria-label="Tool response" class="tool">
          <div class="hd2"><i class="dot"></i><span class="nm"><b>{outcome.call}</b></span></div>
          <div class="elb" style:font-size="12.5px"><span>{outcome.ms.toLocaleString()} ms / {outcome.json.length.toLocaleString()} characters of JSON</span></div>
          <div class="blk indent">
            <div class="tabs"><span class="meta" style:padding="0 12px">JSON</span><button class="r" type="button" onclick={() => { if (outcome.state === "done") void copy(outcome.json); }}><Copy size={13} aria-hidden="true" /> Copy JSON</button></div>
            <div class="sc"><pre style:margin="0" style:white-space="pre">{outcome.json.slice(0, shownLimit)}</pre></div>
            {#if outcome.json.length > shownLimit}<p class="meta" style:padding="8px 12px">Showing the first {shownLimit.toLocaleString()} characters. Copy JSON has all of it.</p>{/if}
          </div>
          {#if notice}<p class="meta" role="status">{notice}</p>{/if}
        </section>
      {/if}
    </form>
  {/if}
</div>
