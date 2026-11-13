<script lang="ts">
import Bot from "@lucide/svelte/icons/bot";
import Check from "@lucide/svelte/icons/check";
import Database from "@lucide/svelte/icons/database";
import Eye from "@lucide/svelte/icons/eye";
import Lock from "@lucide/svelte/icons/lock";
import Save from "@lucide/svelte/icons/save";
import Search from "@lucide/svelte/icons/search";
import Server from "@lucide/svelte/icons/server";
import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
import { createQuery, useQueryClient } from "@tanstack/svelte-query";
import { agents as agentInfo } from "../lib/agents";
import {
  type Draft,
  destinations,
  differences,
  draftOf,
  problem,
  saveSettings,
  settingsOptions,
} from "../lib/settings";
import { statusOptions } from "../lib/status";
import type { Agent } from "../lib/types";
import AgentMark from "./AgentMark.svelte";
import QueryError from "./QueryError.svelte";
import Skeleton from "./Skeleton.svelte";

// Settings a person has a reason to change are controls; the rest of the
// configuration is shown as it is. Nothing is written until the reader
// reviews and saves, and history locations are never typed here: the machine
// finds them itself.
const queryClient = useQueryClient();
const status = createQuery(() => statusOptions);
const settings = createQuery(() => settingsOptions);
const config = $derived(status.data?.configuration);
const file = $derived(settings.data);
const names = Object.fromEntries(
  Object.entries(agentInfo).map(([agent, info]) => [agent, info.name]),
) as Record<Agent, string>;

let draft = $state<Draft | null>(null);
let based = $state("");
// A fresh copy of the file's settings whenever the file itself changes.
$effect(() => {
  if (file && file.revision !== based) {
    draft = draftOf(file);
    based = file.revision;
  }
});
const changed = $derived(file && draft ? differences(file, draft, names) : []);
const touched = $derived(new Set(changed.map((change) => change.key)));
const blocked = $derived(file && draft ? problem(file, draft) : "");
let reviewing = $state(false);
let saving = $state(false);
let failure = $state("");
let saved = $state(0);
let days = $state(90);

function discard(): void {
  if (file) draft = draftOf(file);
  reviewing = false;
  failure = "";
}
async function save(): Promise<void> {
  if (!file || !draft || saving || blocked || !changed.length) return;
  const count = changed.length;
  saving = true;
  failure = "";
  try {
    const next = await saveSettings(file, draft);
    queryClient.setQueryData(settingsOptions.queryKey, next);
    await queryClient.invalidateQueries({ queryKey: statusOptions.queryKey });
    saved = count;
    reviewing = false;
  } catch (error) {
    failure = error instanceof Error ? error.message : "The settings were not saved.";
  } finally {
    saving = false;
  }
}
</script>

{#snippet row(name: string, value: string)}
  <div class="kv" style:align-items="center" style:min-height="50px" style:border-bottom="1px solid var(--hair)" style:padding="0"><dt>{name}</dt><dd style:overflow-wrap="anywhere">{value}</dd></div>
{/snippet}
{#snippet toggle(label: string, on: boolean, flip: () => void)}
  <button class="sw" type="button" role="switch" aria-checked={on} aria-label={label} onclick={() => { saved = 0; flip(); }}><i></i></button>
{/snippet}

<div class="hd">
  <div><h1 class="h1">Settings</h1><p class="meta" style:font-size="13.5px" style:overflow-wrap="anywhere">{file?.path ?? config?.path ?? "Mash configuration"}</p></div>
  {#if status.data}
    <span class={status.data.ready ? "ok" : "warn"} style:display="inline-flex" style:align-items="center" style:gap="7px" style:font-size="13.5px">
      {#if status.data.ready}<Check size={16} aria-hidden="true" />Ready{:else}<TriangleAlert size={16} aria-hidden="true" />Needs attention{/if}
    </span>
  {/if}
</div>
{#if file && !file.editable}
  <p class="meta">Shown from this machine's configuration file. This dashboard can be reached from other machines and was not started with a services configuration, so it does not change settings. Open the dashboard on the machine itself, or edit the file.</p>
{:else if file}
  <p class="meta">These are this machine's settings. Nothing is written until you review and save.</p>
{/if}
{#if status.isPending || settings.isPending}<Skeleton rows={5} />{/if}
<QueryError title="Couldn’t load settings" query={status} />
{#if settings.error}<p class="meta warn" role="alert">Settings can't be changed right now: {settings.error.message}</p>{/if}
{#if status.data && config}
  {#if !config.loaded}<p class="meta warn">No configuration file was found at that path, so built-in defaults apply. Run <code class="i">mash init</code> to create one.</p>{/if}
  {#if status.data.nextStep === "start-qdrant"}<p class="meta warn">Suggested next step: start the Qdrant search service at <code class="i">{config.searchStore}</code>. Indexing and every search need it; <code class="i">mash status</code> shows how to start it.</p>
  {:else if status.data.nextStep}<p class="meta warn">Suggested next step: <code class="i">mash {status.data.nextStep}</code></p>{/if}

  <section class="set-s">
    <h2 class="h2"><Bot size={15} aria-hidden="true" />agent histories</h2>
    {#if file && draft}
      {#each file.agents as entry (entry.agent)}
        {@const on = draft.agents[entry.agent] ?? entry.indexed}
        <div class="set-a" class:off={!on} class:changed={touched.has(`agent:${entry.agent}`)}>
          <AgentMark agent={entry.agent} size={17} />
          <span class="set-n">{agentInfo[entry.agent].name}</span>
          <span class="meta set-p">{entry.roots.join(", ")}</span>
          {#if file.editable}{@render toggle(`Index ${agentInfo[entry.agent].name}`, on, () => { if (draft) draft.agents[entry.agent] = !on; })}{:else}<span class="meta">{on ? "indexed" : "not indexed"}</span>{/if}
        </div>
      {/each}
      <p class="meta" style:margin-top="8px">Agents with no history on this machine are not listed.</p>
    {:else}
      {#each config.providers as provider (provider.agent)}
        <div class="set-a"><AgentMark agent={provider.agent} size={17} /><span class="set-n">{agentInfo[provider.agent].name}</span><span class="meta set-p">{provider.roots.join(", ")}</span></div>
      {/each}
    {/if}
  </section>

  <section class="set-s">
    <h2 class="h2"><Eye size={15} aria-hidden="true" />indexing</h2>
    {#if file && draft && file.editable}
      <div class="set-r" class:changed={touched.has("range")}>
        <span class="set-l">history range</span>
        <span class="set-c">
          <span class="seg" role="group" aria-label="History range">
            <button type="button" aria-pressed={draft.sinceDays === null} onclick={() => { saved = 0; if (draft) draft.sinceDays = null; }}>all history</button>
            <button type="button" aria-pressed={draft.sinceDays !== null} onclick={() => { saved = 0; if (draft && draft.sinceDays === null) draft.sinceDays = file.sinceDays ?? days; }}>recent only</button>
          </span>
          {#if draft.sinceDays !== null}
            <span class="meta">the last <input class="field" type="number" min="1" max="36500" aria-label="Days of history" style:width="92px" style:margin="0 4px" value={draft.sinceDays} oninput={(event) => { saved = 0; days = event.currentTarget.valueAsNumber; if (draft) draft.sinceDays = Number.isNaN(days) ? 0 : days; }} /> days</span>
          {/if}
        </span>
      </div>
      <div class="set-r" class:changed={touched.has("destination")}>
        <span class="set-l">index into</span>
        <span class="set-c">
          {#if file.shared}
            <span class="seg" role="group" aria-label="Index into">
              <button type="button" aria-pressed={draft.destination === "both"} onclick={() => { saved = 0; if (draft) draft.destination = "both"; }}>this machine and the shared server</button>
              <button type="button" aria-pressed={draft.destination === "local"} onclick={() => { saved = 0; if (draft) draft.destination = "local"; }}>this machine only</button>
            </span>
            {#if draft.destination === "local" && file.destination === "both"}<span class="set-h">History already on the shared server stays there.</span>{/if}
          {:else}
            <span class="meta">this machine only · no shared server is configured</span>
          {/if}
        </span>
      </div>
      <div class="set-r" class:changed={touched.has("memory")}>
        <span class="set-l">memory capture<small>agents save decisions and preferences as you state them</small></span>
        <span class="set-c">{@render toggle("Memory capture", draft.memoryCapture, () => { if (draft) draft.memoryCapture = !draft.memoryCapture; })}</span>
      </div>
    {:else}
      <dl>
        {@render row("history range", config.sinceDays === null ? "all history" : `the last ${config.sinceDays} days`)}
        {@render row("index into", config.destination === "both" ? destinations.both : destinations.local)}
        {@render row("memory capture", config.recordsEnabled ? "on" : "off")}
      </dl>
    {/if}
  </section>

  <section class="set-s">
    <h2 class="h2"><Lock size={15} aria-hidden="true" />privacy</h2>
    {#if file && draft && file.editable}
      <div class="set-r" class:changed={touched.has("results")}>
        <span class="set-l">hide credentials in results</span>
        <span class="set-c">{@render toggle("Hide credentials in results", draft.hideInResults, () => { if (draft) draft.hideInResults = !draft.hideInResults; })}</span>
      </div>
      <div class="set-r" class:changed={touched.has("publishing")}>
        <span class="set-l">hide credentials when publishing</span>
        <span class="set-c">
          {@render toggle("Hide credentials when publishing", draft.hideWhenPublishing, () => { if (draft) draft.hideWhenPublishing = !draft.hideWhenPublishing; })}
          {#if draft.hideWhenPublishing !== file.hideWhenPublishing}<span class="set-h">Changing this publishes all of this machine's history again{draft.hideWhenPublishing ? "" : ", including credentials found in chats"}.</span>{/if}
        </span>
      </div>
    {:else}
      <dl>
        {@render row("hide credentials in results", config.redactOutput ? "on" : "off")}
        {@render row("hide credentials when publishing", config.redactPublication ? "on" : "off")}
      </dl>
    {/if}
    <p class="meta" style:margin-top="8px">Credentials are never shown here. The configuration names the environment variables that hold them.</p>
  </section>

  <section class="set-s">
    <h2 class="h2"><Search size={15} aria-hidden="true" />search</h2>
    <dl>
      {@render row("search mode", status.data.mode)}
      {@render row("embeddings", config.embeddings)}
      {@render row("query models", status.data.modelsInstalled ? "installed" : "not installed")}
      {@render row("vector search", config.vectorSearch.toUpperCase())}
      {@render row("ranking", config.ranking)}
    </dl>
  </section>
  <section class="set-s">
    <h2 class="h2"><Database size={15} aria-hidden="true" />storage</h2>
    <dl>
      {@render row("database", status.data.databasePath ?? "not created yet")}
      {@render row("chats", status.data.chats.toLocaleString())}
      {@render row("messages", `${status.data.messages.toLocaleString()} (${status.data.searchableMessages.toLocaleString()} searchable)`)}
      {@render row("search store", config.searchStore)}
    </dl>
  </section>
  <section class="set-s">
    <h2 class="h2"><Server size={15} aria-hidden="true" />shared index</h2>
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
  <p class="meta">How search works and where data is stored are shown, not offered as choices. Change those in the configuration file.</p>
{/if}

{#if file?.editable && (changed.length || saved || failure)}
  <div class="set-bar" role="region" aria-label="Unsaved settings">
    {#if changed.length}
      <div class="set-bt">
        <strong>{changed.length} unsaved change{changed.length === 1 ? "" : "s"}</strong>
        <button class="btn" type="button" disabled={saving} onclick={discard}>Discard</button>
        <button class="btn" type="button" aria-expanded={reviewing} onclick={() => (reviewing = !reviewing)}>{reviewing ? "Hide changes" : "Review changes"}</button>
        <button class="btn p" type="button" disabled={saving || !!blocked} onclick={save}><Save size={15} aria-hidden="true" />{saving ? "Saving…" : "Save to file"}</button>
      </div>
      {#if blocked}<p class="meta warn" role="alert">{blocked}</p>{/if}
      {#if reviewing}
        <ul class="set-d">
          {#each changed as change (change.key)}<li><span>{change.label}</span><span class="was">{change.from}</span><span aria-hidden="true">→</span><span class="now">{change.to}</span></li>{/each}
        </ul>
        <p class="meta">The file is rewritten; the previous one is kept beside it as <code class="i">config.toml.previous</code>. Settings the page does not show are kept.</p>
      {/if}
    {/if}
    {#if failure}<p class="fail" role="alert">{failure}</p>{/if}
    {#if saved && !changed.length}<p class="ok" role="status">Saved {saved} change{saved === 1 ? "" : "s"} to the file. A running watcher and connected agents keep their old settings until they restart. <a class="a" href="#/services">Restart the watcher from Services</a>.</p>{/if}
  </div>
{/if}
