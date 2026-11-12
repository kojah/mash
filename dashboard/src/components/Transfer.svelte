<script lang="ts">
import Archive from "@lucide/svelte/icons/archive";
import Bot from "@lucide/svelte/icons/bot";
import Braces from "@lucide/svelte/icons/braces";
import Check from "@lucide/svelte/icons/check";
import Clock from "@lucide/svelte/icons/clock";
import Copy from "@lucide/svelte/icons/copy";
import FileText from "@lucide/svelte/icons/file-text";
import Folder from "@lucide/svelte/icons/folder";
import Laptop from "@lucide/svelte/icons/laptop";
import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
import Upload from "@lucide/svelte/icons/upload";
import { createQuery } from "@tanstack/svelte-query";
import { onDestroy } from "svelte";
import { agents as agentInfo } from "../lib/agents";
import { browse, isAgent } from "../lib/api";
import { groupChats, projectLabel } from "../lib/browse";
import { type BulkFormat, chatKey, exportChats, saveBlob } from "../lib/bulkExport";
import { plainTitle } from "../lib/markdown";
import { navigate } from "../lib/navigation";
import { catalogOptions } from "../lib/queries";
import { type Applied, appliedHref, dateRanges, filtersOf, parseApplied } from "../lib/search";
import AgentMark from "./AgentMark.svelte";
import FilterMenu from "./FilterMenu.svelte";
import MetaChips from "./MetaChips.svelte";
import QueryError from "./QueryError.svelte";
import Skeleton from "./Skeleton.svelte";

// Bulk export of the chats a set of filters matches. The export is built in the
// browser from the same read-only pages a single chat uses, so nothing here
// changes history or writes on the machine.
let { params }: { params: string } = $props();
const pageSize = 25;
const base = "#/export";
const now = Date.now();
const applied = $derived(parseApplied(params));
const catalogue = createQuery(() => catalogOptions);
const chats = $derived(catalogue.data?.chats ?? []);
const matching = $derived(
  catalogue.data ? browse(catalogue.data, filtersOf(applied), now).hits.map((hit) => hit.chat) : [],
);

// Everything the filters match is exported unless it was left out by hand.
let excluded = $state<ReadonlySet<string>>(new Set());
let onlyLeftOut = $state(false);
const chosen = $derived(matching.filter((chat) => !excluded.has(chatKey(chat))));
const leftOut = $derived(matching.length - chosen.length);
const listed = $derived(
  onlyLeftOut && leftOut > 0 ? matching.filter((chat) => excluded.has(chatKey(chat))) : matching,
);
// A link to a page past the end shows the last page instead of an empty one.
const first = $derived(
  Math.min(applied.offset, Math.max(0, Math.floor((listed.length - 1) / pageSize) * pageSize)),
);
const shown = $derived(listed.slice(first, first + pageSize));
const shownChosen = $derived(shown.filter((chat) => !excluded.has(chatKey(chat))).length);
const filtered = $derived(
  applied.agents.length + applied.machines.length + applied.projects.length > 0 ||
    applied.days !== null,
);

const options = $derived({
  agents: groupChats(chats, (chat) => chat.agent).map((group) => ({
    value: group.key,
    label: isAgent(group.key) ? agentInfo[group.key].name : group.key,
    count: group.chats.length,
  })),
  machines: groupChats(chats, (chat) => chat.machine).map((group) => ({
    value: group.key,
    label: group.key,
    count: group.chats.length,
  })),
  projects: groupChats(
    chats.filter((chat) => chat.project),
    (chat) => chat.project,
  ).map((group) => ({
    value: group.key,
    label: projectLabel(group.key),
    count: group.chats.length,
  })),
});

type Field = "agents" | "machines" | "projects";
function go(next: Applied): void {
  navigate(appliedHref({ ...next, query: "", offset: 0 }, base));
}
function toggleFilter(field: Field, value: string): void {
  const current: readonly string[] = applied[field];
  const next = current.includes(value)
    ? current.filter((item) => item !== value)
    : [...current, value];
  go(
    field === "agents"
      ? { ...applied, agents: next.filter(isAgent) }
      : { ...applied, [field]: next },
  );
}
function chipLabel(field: Field, noun: string): string {
  const picked = applied[field];
  if (picked.length === 0) return noun;
  if (picked.length > 1) return `${picked.length} ${noun}s`;
  const [only] = picked;
  if (only === undefined) return noun;
  if (field === "agents") return isAgent(only) ? agentInfo[only].name : only;
  return field === "projects" ? projectLabel(only) : only;
}
function setIncluded(key: string, included: boolean): void {
  const next = new Set(excluded);
  if (included) next.delete(key);
  else next.add(key);
  excluded = next;
}
const count = (value: number, noun: string) =>
  `${value.toLocaleString()} ${noun}${value === 1 ? "" : "s"}`;

type Format = BulkFormat | "archive";
let format = $state<Format>("markdown");
const formats = [
  {
    value: "markdown",
    name: "Markdown",
    icon: FileText,
    tint: "var(--sky)",
    about: "One readable file per chat, in folders by project. For sharing or keeping in notes.",
  },
  {
    value: "json",
    name: "JSON",
    icon: Braces,
    tint: "var(--teal)",
    about: "One file per chat with every message and its details. For your own scripts.",
  },
  {
    value: "archive",
    name: "Mash archive",
    icon: Archive,
    tint: "var(--clay2)",
    about: "A full backup of one machine, the only kind Mash can restore. Made on that machine.",
  },
] as const satisfies readonly { value: Format; [key: string]: unknown }[];

type Job =
  | { readonly state: "idle" }
  | {
      readonly state: "running";
      readonly chats: number;
      readonly total: number;
      readonly messages: number;
    }
  | {
      readonly state: "done";
      readonly chats: number;
      readonly messages: number;
      readonly file: string;
    }
  | { readonly state: "stopped" }
  | { readonly state: "failed"; readonly reason: string };
let job = $state<Job>({ state: "idle" });
let controller: AbortController | undefined;
onDestroy(() => controller?.abort());

async function start(kind: BulkFormat): Promise<void> {
  const selection = chosen;
  if (!selection.length || job.state === "running") return;
  controller = new AbortController();
  const { signal } = controller;
  job = { state: "running", chats: 0, total: selection.length, messages: 0 };
  let messages = 0;
  try {
    const blob = await exportChats(selection, kind, signal, (progress) => {
      messages = progress.messages;
      job = { state: "running", ...progress };
    });
    const file = `mash-chats-${new Date().toISOString().slice(0, 10)}.zip`;
    saveBlob(blob, file);
    job = { state: "done", chats: selection.length, messages, file };
  } catch (error) {
    job = signal.aborted
      ? { state: "stopped" }
      : {
          state: "failed",
          reason: error instanceof Error ? error.message : "The export could not be made.",
        };
  }
}

const backup = "mash export ~/mash-backup";
let copied = $state(false);
async function copy(): Promise<void> {
  try {
    await navigator.clipboard.writeText(backup);
    copied = true;
    setTimeout(() => (copied = false), 1500);
  } catch {
    // Without clipboard access the command stays selectable on the page.
  }
}
</script>

<div class="hd">
  <div><h1 class="h1">Export</h1><p class="meta">Copy many chats out of Mash at once.</p></div>
</div>

<QueryError title="Couldn’t load chats" query={catalogue} />
{#if catalogue.isPending}<Skeleton rows={5} />{/if}
{#if catalogue.data}
  <div class="chips" role="group" aria-label="Filters">
    <FilterMenu label={chipLabel("agents", "agent")} active={applied.agents.length > 0} multiple options={options.agents} selected={applied.agents} choose={(value) => toggleFilter("agents", value)}>
      {#snippet icon()}<Bot size={15} aria-hidden="true" />{/snippet}
    </FilterMenu>
    <FilterMenu label={chipLabel("machines", "machine")} active={applied.machines.length > 0} multiple options={options.machines} selected={applied.machines} choose={(value) => toggleFilter("machines", value)}>
      {#snippet icon()}<Laptop size={15} aria-hidden="true" />{/snippet}
    </FilterMenu>
    <FilterMenu label={chipLabel("projects", "project")} active={applied.projects.length > 0} multiple options={options.projects} selected={applied.projects} choose={(value) => toggleFilter("projects", value)}>
      {#snippet icon()}<Folder size={15} aria-hidden="true" />{/snippet}
    </FilterMenu>
    <FilterMenu
      label={dateRanges.find((range) => range.days === applied.days)?.label ?? "any time"}
      active={applied.days !== null}
      options={dateRanges.map((range) => ({ value: String(range.days ?? ""), label: range.label }))}
      selected={[String(applied.days ?? "")]}
      choose={(value) => go({ ...applied, days: value ? Number(value) : null })}
    >
      {#snippet icon()}<Clock size={15} aria-hidden="true" />{/snippet}
    </FilterMenu>
    {#if filtered}<a class="a" href={base} style:font-size="13.5px" style:padding="0 6px">Clear filters</a>{/if}
  </div>

  <p class="meta" role="status" style:font-size="13.5px">
    <b style:color="var(--fg)">{count(chosen.length, "chat")}</b> will be exported{#if !filtered}, which is every chat this dashboard lists{/if}.
    {#if leftOut > 0}
      {count(leftOut, "chat")} left out by hand:
      <button class="a" type="button" onclick={() => { onlyLeftOut = !onlyLeftOut; go(applied); }}>{onlyLeftOut ? "show all" : "show them"}</button>
      ·
      <button class="a" type="button" onclick={() => { excluded = new Set(); onlyLeftOut = false; }}>include all again</button>
    {/if}
  </p>

  {#if !matching.length}
    <div class="state-c"><p>No chat matches these filters.</p></div>
  {:else}
    <section aria-label="Chats to export">
      {#each shown as chat (chatKey(chat))}
        {@const key = chatKey(chat)}
        {@const included = !excluded.has(key)}
        <label class="xrow" class:off={!included}>
          <input type="checkbox" checked={included} onchange={(event) => setIncluded(key, event.currentTarget.checked)} />
          <span style:min-width="0">
            <span class="row-t"><AgentMark agent={chat.agent} /><span>{plainTitle(chat.title)}</span></span>
            <span class="elb" style:margin-top="2px"><MetaChips {chat} extra={included ? "" : "left out"} /></span>
          </span>
        </label>
      {/each}
      {#if listed.length > pageSize}
        <nav class="hd" aria-label="Pages" style:margin-top="14px">
          <span class="meta">{(first + 1).toLocaleString()}–{(first + shown.length).toLocaleString()} of {count(listed.length, "chat")} · {shownChosen.toLocaleString()} of the {shown.length} on this page included</span>
          <span class="acts">
            {#if first > 0}<a class="btn" rel="prev" href={appliedHref({ ...applied, offset: Math.max(0, first - pageSize) }, base)}>Previous</a>{:else}<button class="btn" type="button" disabled>Previous</button>{/if}
            {#if first + pageSize < listed.length}<a class="btn" rel="next" href={appliedHref({ ...applied, offset: first + pageSize }, base)}>Next</a>{:else}<button class="btn" type="button" disabled>Next</button>{/if}
          </span>
        </nav>
      {/if}
    </section>
  {/if}

  <div class="grid-tiles" role="radiogroup" aria-label="What to make">
    {#each formats as option (option.value)}
      <label class="tile pick" class:on={format === option.value}>
        <span class="tile-h"><option.icon size={22} color={option.tint} aria-hidden="true" /><b style:flex="1">{option.name}</b><input type="radio" name="format" value={option.value} bind:group={format} /></span>
        <span class="meta" style:font-size="13.5px">{option.about}</span>
      </label>
    {/each}
  </div>

  {#if format === "archive"}
    <div class="tile">
      <b>Make a full backup on the machine itself</b>
      <p class="meta" style:font-size="13.5px">A Mash archive always holds one machine’s whole history, so it ignores the filters above. Run this there, with a folder that does not exist yet:</p>
      <div class="cmdline"><code>{backup}</code><button type="button" aria-label={copied ? "Copied" : "Copy the command"} title={copied ? "Copied" : "Copy"} onclick={copy}>{#if copied}<Check size={16} aria-hidden="true" />{:else}<Copy size={16} aria-hidden="true" />{/if}</button></div>
      <p class="meta" style:font-size="13px">Restoring one is covered in <a class="a" href="https://getmash.io/upgrades/" rel="noreferrer">backups and upgrades</a>.</p>
    </div>
  {:else}
    <div class="tile">
      <p class="warn" style:display="flex" style:gap="8px" style:font-size="13.5px"><TriangleAlert size={16} aria-hidden="true" style="flex:none;margin-top:2px" /><span>The download holds these chats in full, including any passwords or private details said in them.</span></p>
      <div class="hd">
        <span class="acts">
          <button class="btn p" type="button" disabled={!chosen.length || job.state === "running"} onclick={() => { if (format !== "archive") void start(format); }}><Upload size={15} aria-hidden="true" />Export {count(chosen.length, "chat")}</button>
          {#if job.state === "running"}<button class="btn" type="button" onclick={() => controller?.abort()}>Stop</button>{/if}
        </span>
        <span class="meta" style:font-size="13px">Saved by your browser as one .zip file. Attachments are not included.</span>
      </div>
      {#if job.state === "running"}
        <div role="status">
          <p class="meta" style:font-size="13.5px">Reading chat {Math.min(job.chats + 1, job.total).toLocaleString()} of {job.total.toLocaleString()} · {count(job.messages, "message")} so far. Keep this page open.</p>
          <span class="bar" role="progressbar" aria-label="Export progress" aria-valuemin={0} aria-valuemax={job.total} aria-valuenow={job.chats}><i style:width="{Math.round((job.chats / job.total) * 100)}%"></i></span>
        </div>
      {:else if job.state === "done"}
        <p class="ok" role="status" style:font-size="13.5px">Exported {count(job.chats, "chat")} and {count(job.messages, "message")} to {job.file}.</p>
      {:else if job.state === "stopped"}
        <p class="meta" role="status" style:font-size="13.5px">Export stopped. Nothing was saved.</p>
      {:else if job.state === "failed"}
        <p class="warn" role="alert" style:font-size="13.5px">{job.reason}</p>
      {/if}
    </div>
  {/if}
{/if}
