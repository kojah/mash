<script lang="ts">
import Bot from "@lucide/svelte/icons/bot";
import ChevronLeft from "@lucide/svelte/icons/chevron-left";
import Clock from "@lucide/svelte/icons/clock";
import Folder from "@lucide/svelte/icons/folder";
import Laptop from "@lucide/svelte/icons/laptop";
import MessagesSquare from "@lucide/svelte/icons/messages-square";
import { createQuery } from "@tanstack/svelte-query";
import type { Snippet } from "svelte";
import { agents as agentInfo } from "../lib/agents";
import { groupChats, projectLabel, whenLabel } from "../lib/browse";
import { plainTitle } from "../lib/markdown";
import { catalogOptions } from "../lib/queries";
import { agentHref, chatHref, machineHref, projectHref, searchHref } from "../lib/route.svelte";
import type { ChatSummary } from "../lib/types";
import AgentMark from "./AgentMark.svelte";
import ChatRow from "./ChatRow.svelte";
import QueryError from "./QueryError.svelte";
import Skeleton from "./Skeleton.svelte";

// One agent, machine or project: its chats and how they break down by the
// other two. `extra` adds page-specific sections above the recent chats.
let {
  field,
  value,
  title,
  icon,
  back,
  extra,
}: {
  field: "agent" | "machine" | "project";
  value: string;
  title: string;
  icon: Snippet;
  back?: { href: string; label: string };
  extra?: Snippet;
} = $props();
const catalogue = createQuery(() => catalogOptions);
const key = (chat: ChatSummary) =>
  field === "agent" ? chat.agent : field === "machine" ? chat.machine : chat.project;
const chats = $derived((catalogue.data?.chats ?? []).filter((chat) => key(chat) === value));
const group = $derived(groupChats(chats, key)[0]);
const named = $derived(group ? group.projects.filter(([name]) => name) : []);
const latest = $derived(
  chats.reduce<ChatSummary | undefined>(
    (best, chat) => (best && (best.updatedMs ?? 0) >= (chat.updatedMs ?? 0) ? best : chat),
    undefined,
  ),
);
const now = Date.now();
</script>

{#if back}<a class="back" href={back.href} aria-label="Back to {back.label}"><ChevronLeft size={15} aria-hidden="true" />{back.label}</a>{/if}
<div class="hd">
  <div style:display="flex" style:align-items="center" style:gap="14px">
    {@render icon()}
    <div style:min-width="0"><h1 class="h1">{title}</h1><p class="meta">{chats.length.toLocaleString()} chats</p></div>
  </div>
</div>
<QueryError title="Couldn’t load chats" query={catalogue} />
{#if catalogue.isPending}<Skeleton rows={5} />{/if}
{#if catalogue.data && !group}<div class="state-c"><p>No indexed chats here.</p></div>{/if}
{#if group}
  <nav class="stat" aria-label="Summary">
    <a href={searchHref({ [field]: value })}><span class="k"><MessagesSquare size={14} aria-hidden="true" />chats</span><span class="v">{chats.length.toLocaleString()}</span></a>
    {#if field !== "agent"}<a href={group.agents.length === 1 ? agentHref(group.agents[0]?.[0] ?? "") : "#/agents"}><span class="k"><Bot size={14} aria-hidden="true" />agents</span><span class="v">{group.agents.length}</span></a>{/if}
    {#if field !== "project"}<a href={named.length === 1 ? projectHref(named[0]?.[0] ?? "") : "#/projects"}><span class="k"><Folder size={14} aria-hidden="true" />projects</span><span class="v">{named.length}</span></a>{/if}
    {#if field !== "machine"}<a href={group.machines.length === 1 ? machineHref(group.machines[0]?.[0] ?? "") : "#/machines"}><span class="k"><Laptop size={14} aria-hidden="true" />machines</span><span class="v">{group.machines.length}</span></a>{/if}
    {#if latest}<a href={chatHref(latest.id, latest.machine)} title={plainTitle(latest.title)}><span class="k"><Clock size={14} aria-hidden="true" />last active</span><span class="v">{whenLabel(group.latestMs, now)}</span></a>{/if}
  </nav>
  <div class="cols">
    <div class="c-main stack">
      {#if extra}{@render extra()}{/if}
      <section>
        <div class="hd"><h2 class="h2"><Clock size={15} aria-hidden="true" />recent chats</h2><a class="meta" href={searchHref({ [field]: value })} style:display="inline-flex" style:align-items="center" style:min-height="44px">all {chats.length.toLocaleString()}</a></div>
        {#each chats.slice(0, 6) as chat (chat.machine + chat.id)}<ChatRow {chat} />{/each}
      </section>
    </div>
    <div class="c-side">
      {#if field !== "agent"}
        <section><h2 class="h2"><Bot size={15} aria-hidden="true" />agents</h2>
          {#each group.agents.slice(0, 6) as [agent, count] (agent)}<a class="li" href={agentHref(agent)}><AgentMark {agent} size={14} /><span class="g">{agentInfo[agent].name}</span><span class="n">{count.toLocaleString()}</span></a>{/each}
        </section>
      {/if}
      {#if field !== "machine"}
        <section><h2 class="h2"><Laptop size={15} aria-hidden="true" />machines</h2>
          {#each group.machines.slice(0, 6) as [machine, count] (machine)}<a class="li" href={machineHref(machine)}><Laptop size={16} aria-hidden="true" /><span class="g">{machine}</span><span class="n">{count.toLocaleString()}</span></a>{/each}
        </section>
      {/if}
      {#if field !== "project"}
        <section><h2 class="h2"><Folder size={15} aria-hidden="true" />projects</h2>
          {#each group.projects.filter(([name]) => name).slice(0, 8) as [project, count] (project)}<a class="li" href={projectHref(project)} title={project}><Folder size={16} aria-hidden="true" /><span class="g">{projectLabel(project)}</span><span class="n">{count.toLocaleString()}</span></a>{/each}
        </section>
      {/if}
    </div>
  </div>
{/if}
