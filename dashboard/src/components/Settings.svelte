<script lang="ts">
import Check from "@lucide/svelte/icons/check";
import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
import { createQuery } from "@tanstack/svelte-query";
import { agents as agentInfo } from "../lib/agents";
import { statusOptions } from "../lib/status";
import type { Agent } from "../lib/types";
import AgentMark from "./AgentMark.svelte";
import QueryError from "./QueryError.svelte";
import Skeleton from "./Skeleton.svelte";

// Settings are read from Mash's configuration file. The dashboard shows them
// and never writes them: the file stays the one place they are changed.
const status = createQuery(() => statusOptions);
const config = $derived(status.data?.configuration);
const known: readonly Agent[] = [
  "claude-code",
  "codex",
  "opencode",
  "cursor",
  "copilot",
  "antigravity",
];
const destinations: Record<string, string> = {
  local: "this machine only",
  shared: "shared server only",
  both: "this machine and the shared server",
};
</script>

{#snippet row(name: string, value: string)}
  <div class="kv" style:align-items="center" style:min-height="50px" style:border-bottom="1px solid var(--hair)" style:padding="0"><dt>{name}</dt><dd style:overflow-wrap="anywhere">{value}</dd></div>
{/snippet}

<div class="hd">
  <div><h1 class="h1">Settings</h1><p class="meta" style:font-size="12.5px" style:overflow-wrap="anywhere">{config?.path ?? "Mash configuration"}</p></div>
  {#if status.data}
    <span class={status.data.ready ? "ok" : "warn"} style:display="inline-flex" style:align-items="center" style:gap="7px" style:font-size="12.5px">
      {#if status.data.ready}<Check size={16} aria-hidden="true" />Ready{:else}<TriangleAlert size={16} aria-hidden="true" />Needs attention{/if}
    </span>
  {/if}
</div>
<p class="meta">Read from the configuration file. To change a setting, edit that file and restart Mash; the dashboard does not write it.</p>
{#if status.isPending}<Skeleton rows={5} />{/if}
<QueryError title="Couldn’t load settings" query={status} />
{#if status.data && config}
  {#if !config.loaded}<p class="meta warn">No configuration file was found at that path, so built-in defaults apply. Run <code class="i">mash init</code> to create one.</p>{/if}
  {#if status.data.nextStep}<p class="meta warn">Suggested next step: <code class="i">mash {status.data.nextStep}</code></p>{/if}
  <section style:max-width="860px">
    <h2 class="h2">agent histories</h2>
    {#each known as agent (agent)}
      {@const provider = config.providers.find((entry) => entry.agent === agent)}
      <div style:display="flex" style:flex-wrap="wrap" style:align-items="center" style:gap="2px 12px" style:min-height="50px" style:border-bottom="1px solid var(--hair)" style:opacity={provider ? 1 : 0.55}>
        <AgentMark {agent} size={17} />
        <span style:flex="0 0 150px">{agentInfo[agent].name}</span>
        <span class="meta" style:flex="1 1 180px" style:min-width="0" style:overflow-wrap="anywhere">{provider ? provider.roots.join(", ") : "not indexed"}</span>
      </div>
    {/each}
  </section>
  <section style:max-width="860px">
    <h2 class="h2">indexing</h2>
    <dl>
      {@render row("history range", config.sinceDays === null ? "all history" : `the last ${config.sinceDays} days`)}
      {@render row("index into", destinations[config.destination] ?? config.destination)}
      {@render row("memory capture", config.recordsEnabled ? "on" : "off")}
    </dl>
  </section>
  <section style:max-width="860px">
    <h2 class="h2">search</h2>
    <dl>
      {@render row("search mode", status.data.mode)}
      {@render row("embeddings", config.embeddings)}
      {@render row("query models", status.data.modelsInstalled ? "installed" : "not installed")}
      {@render row("vector search", config.vectorSearch.toUpperCase())}
      {@render row("ranking", config.ranking)}
    </dl>
  </section>
  <section style:max-width="860px">
    <h2 class="h2">storage</h2>
    <dl>
      {@render row("database", status.data.databasePath ?? "not created yet")}
      {@render row("chats", status.data.chats.toLocaleString())}
      {@render row("messages", `${status.data.messages.toLocaleString()} (${status.data.searchableMessages.toLocaleString()} searchable)`)}
      {@render row("search store", config.searchStore)}
      {@render row("hide credentials", [config.redactOutput ? "in results" : "", config.redactPublication ? "when publishing" : ""].filter(Boolean).join(" and ") || "off")}
    </dl>
  </section>
  <section style:max-width="860px">
    <h2 class="h2">shared index</h2>
    {#if config.shared}
      <dl>
        {@render row("this machine", config.shared.node)}
        {@render row("group", config.shared.group)}
        {@render row("server", config.shared.url)}
      </dl>
    {:else}
      <p class="meta">Not configured. This machine's history stays local.</p>
    {/if}
  </section>
  <p class="meta">Credentials are never shown here. The configuration names the environment variables that hold them.</p>
{/if}
