<script lang="ts">
import Terminal from "@lucide/svelte/icons/terminal";
import { durationLabel, type ToolCall } from "../lib/toolcalls";
import JsonView from "./JsonView.svelte";

// One tool call with its output, laid out by the same rules for every tool:
// a header to scan, the arguments as labelled rows, then the output. All of
// it is archived text and is rendered as text.
let { call, selected = "" }: { call: ToolCall; selected?: string } = $props();
const LINES = 24;
const CHARACTERS = 4000;
const hit = $derived(
  !!selected && (selected === call.id || (!!call.resultId && selected === call.resultId)),
);
let all = $state(false);
let raw = $state(false);
const lines = $derived(call.output?.text.split("\n") ?? []);
const long = $derived(lines.length > LINES || (call.output?.text.length ?? 0) > CHARACTERS);
const shown = $derived(
  all || !long ? (call.output?.text ?? "") : lines.slice(0, LINES).join("\n").slice(0, CHARACTERS),
);
</script>

<details class="tc" class:hit class:bad={call.status?.failed} id="message-{call.id}" data-message={call.id} open={hit} style:scroll-margin-top="80px">
  <summary>
    <Terminal size={15} aria-hidden="true" />
    <span class="tc-n">{call.name}</span>
    <span class="tc-s">{call.summary}</span>
    {#if call.status}<span class="mt" class:ok={!call.status.failed} class:bad={call.status.failed}><i class="dot" class:bad={call.status.failed}></i>{call.status.label}</span>{/if}
    {#if call.durationMs !== undefined}<span class="mt">{durationLabel(call.durationMs)}</span>{/if}
    {#if call.server}<span class="mt">{call.server}</span>{/if}
    <span class="mtx">{call.at}</span>
  </summary>
  <div class="tc-b">
    {#each call.args as argument (argument.key)}
      <div class="tc-r">
        <span class="tc-k">{argument.key}</span>
        {#if argument.block}<pre class="tc-pre" class:wrap={!argument.text.includes("\n")}>{argument.text}</pre>{:else}<span class="tc-v">{argument.text}</span>{/if}
      </div>
    {/each}
    {#if call.output}
      <div class="tc-o" id={call.resultId ? `message-${call.resultId}` : undefined} data-message={call.resultId} style:scroll-margin-top="80px">
        <span class="tc-h">output</span>
        {#if call.output.json !== undefined}
          <div class="tc-pre"><JsonView value={call.output.json} /></div>
        {:else}
          <pre class="tc-pre" class:wrap={lines.length < 3}>{shown}</pre>
          {#if long}
            <p class="tc-f">
              <span>{all ? `${lines.length.toLocaleString()} lines` : `${Math.min(LINES, lines.length).toLocaleString()} of ${lines.length.toLocaleString()} lines`}</span>
              <button class="a" type="button" onclick={() => (all = !all)}>{all ? "Show less" : "Show all output"}</button>
            </p>
          {/if}
        {/if}
      </div>
    {:else if !call.args.length}
      <p class="meta">Nothing further was recorded for this call.</p>
    {/if}
    <p class="tc-f"><button class="a" type="button" aria-expanded={raw} onclick={() => (raw = !raw)}>{raw ? "Hide raw record" : "Raw record"}</button></p>
    {#if raw}<pre class="tc-pre wrap">{call.raw}</pre>{/if}
  </div>
</details>
