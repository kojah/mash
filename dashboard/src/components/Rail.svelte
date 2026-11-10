<script lang="ts">
import BookOpen from "@lucide/svelte/icons/book-open";
import Bot from "@lucide/svelte/icons/bot";
import ChevronRight from "@lucide/svelte/icons/chevron-right";
import Clock from "@lucide/svelte/icons/clock";
import Folder from "@lucide/svelte/icons/folder";
import History from "@lucide/svelte/icons/history";
import House from "@lucide/svelte/icons/house";
import Laptop from "@lucide/svelte/icons/laptop";
import MessagesSquare from "@lucide/svelte/icons/messages-square";
import Server from "@lucide/svelte/icons/server";
import Settings from "@lucide/svelte/icons/settings";
import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
import Terminal from "@lucide/svelte/icons/terminal";
import { createQuery } from "@tanstack/svelte-query";
import { agents as agentInfo } from "../lib/agents";
import { groupChats, projectLabel } from "../lib/browse";
import { catalogOptions } from "../lib/queries";
import { label as kindLabel, kinds } from "../lib/records";
import { agentHref, machineHref, projectHref, recordsHref, router } from "../lib/route.svelte";
import AgentMark from "./AgentMark.svelte";

let { open, close }: { open: boolean; close: () => void } = $props();
const catalogue = createQuery(() => catalogOptions);
const chats = $derived(catalogue.data?.chats ?? []);
const projects = $derived(
  groupChats(
    chats.filter((chat) => chat.project),
    (chat) => chat.project,
  ).slice(0, 6),
);
const providers = $derived(groupChats(chats, (chat) => chat.agent));
const machines = $derived(groupChats(chats, (chat) => chat.machine));

const sections = [
  { id: "overview", label: "overview", href: "#/", icon: House, tint: "var(--clay2)" },
  { id: "chats", label: "chats", href: "#/search", icon: MessagesSquare, tint: "var(--sky)" },
  { id: "agents", label: "agents", href: "#/agents", icon: Bot, tint: "var(--lilac)" },
  { id: "machines", label: "machines", href: "#/machines", icon: Laptop, tint: "var(--blue)" },
  { id: "projects", label: "projects", href: "#/projects", icon: Folder, tint: "var(--warn)" },
  { id: "memory", label: "memory", href: "#/records?", icon: BookOpen, tint: "var(--pink)" },
  { id: "services", label: "services", href: "#/services", icon: Server, tint: "var(--teal)" },
  {
    id: "fleet",
    label: "manage fleet",
    href: "#/fleet",
    icon: SlidersHorizontal,
    tint: "var(--indigo)",
  },
  { id: "console", label: "mcp console", href: "#/console", icon: Terminal, tint: "var(--sky)" },
  { id: "settings", label: "settings", href: "#/settings", icon: Settings, tint: "var(--stone)" },
] as const;
type Section = (typeof sections)[number]["id"];
const expandable: readonly Section[] = ["chats", "agents", "machines", "projects", "memory"];

const route = $derived(router.current);
const area = $derived.by((): Section | "" => {
  switch (route.page) {
    case "overview":
      return "overview";
    case "search":
    case "history":
    case "chat":
      return "chats";
    case "agents":
    case "agent":
      return "agents";
    case "machines":
    case "machine":
      return "machines";
    case "projects":
    case "project":
      return "projects";
    case "records":
    case "record":
      return "memory";
    case "services":
      return "services";
    case "fleet":
      return "fleet";
    case "console":
      return "console";
    case "settings":
      return "settings";
    default:
      return "";
  }
});
// A row is the current page only while none of its children is.
const child = $derived.by((): string => {
  if (route.page === "search") return "recent";
  if (route.page === "history") return "history";
  if (route.page === "agent") return route.agent;
  if (route.page === "machine") return route.id;
  if (route.page === "project") return route.name;
  if (route.page === "records") return new URLSearchParams(route.params).get("kind") ?? "";
  return "";
});
let expanded = $state<readonly Section[]>([]);
$effect(() => {
  if (area && expandable.includes(area) && !expanded.includes(area)) expanded = [...expanded, area];
});
function toggle(id: Section): void {
  expanded = expanded.includes(id) ? expanded.filter((item) => item !== id) : [...expanded, id];
}
</script>

<aside id="rail" class:open>
  <a class="brand" href="#/" onclick={close}><img src="/mash-mark.png" alt="" /><span>mash<i aria-hidden="true">_</i></span></a>
  <button id="xdrawer" type="button" aria-label="Close navigation" onclick={close}>×</button>
  <nav class="nav" aria-label="Main">
    {#each sections as section (section.id)}
      {@const on = section.id === area && !child}
      {@const isOpen = expanded.includes(section.id)}
      <div class="nav-row" class:on>
        <a class="nav-link" href={section.href} aria-current={on ? "page" : undefined} onclick={close}><section.icon size={18} color={section.tint} aria-hidden="true" />{section.label}</a>
        {#if expandable.includes(section.id)}
          <button class="chev" type="button" aria-label="{isOpen ? 'Collapse' : 'Expand'} {section.label}" aria-expanded={isOpen} onclick={() => toggle(section.id)}><ChevronRight size={15} /></button>
        {/if}
      </div>
      {#if isOpen}
        {#if section.id === "chats"}
          <a class="sub" class:on={child === "recent"} href="#/search" aria-current={child === "recent" ? "page" : undefined} onclick={close}><Clock size={16} color="var(--sky)" aria-hidden="true" /><span>recent</span></a>
          <a class="sub" class:on={child === "history"} href="#/history" aria-current={child === "history" ? "page" : undefined} onclick={close}><History size={16} color="var(--sky)" aria-hidden="true" /><span>viewing history</span></a>
        {:else if section.id === "agents"}
          {#each providers as provider (provider.key)}
            {@const agent = provider.chats[0]?.agent}
            {#if agent}<a class="sub" class:on={child === agent} href={agentHref(agent)} onclick={close}><AgentMark {agent} size={14} /><span>{agentInfo[agent].name}</span></a>{/if}
          {/each}
        {:else if section.id === "machines"}
          {#each machines as machine (machine.key)}
            <a class="sub" class:on={child === machine.key} href={machineHref(machine.key)} onclick={close}><i class="dot"></i><span>{machine.key}</span></a>
          {/each}
        {:else if section.id === "projects"}
          {#each projects as project (project.key)}
            <a class="sub" class:on={child === project.key} href={projectHref(project.key)} onclick={close}><Folder size={15} color="var(--warn)" aria-hidden="true" /><span>{projectLabel(project.key)}</span></a>
          {/each}
        {:else if section.id === "memory"}
          {#each kinds as kind (kind)}
            <a class="sub" class:on={child === kind} href={recordsHref(new URLSearchParams({ kind }))} onclick={close}><BookOpen size={16} color="var(--pink)" aria-hidden="true" /><span>{kindLabel(kind).toLowerCase()}s</span></a>
          {/each}
        {/if}
      {/if}
    {/each}
    <div class="status" style:margin-top="auto" style:padding-top="14px">
      {#if catalogue.data}{catalogue.data.localNode} / {chats.length.toLocaleString()} chats{:else if catalogue.error}history unavailable{:else}loading…{/if}
      <button class="a" type="button" disabled={catalogue.isFetching} onclick={() => catalogue.refetch()} style:display="block" style:min-height="32px">{catalogue.isFetching ? "reloading…" : "reload chat list"}</button>
    </div>
  </nav>
</aside>
