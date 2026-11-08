<script lang="ts">
import Activity from "@lucide/svelte/icons/activity";
import Bookmark from "@lucide/svelte/icons/bookmark";
import Bot from "@lucide/svelte/icons/bot";
import Clock from "@lucide/svelte/icons/clock";
import Home from "@lucide/svelte/icons/house";
import Laptop from "@lucide/svelte/icons/laptop";
import Layers from "@lucide/svelte/icons/layers";
import Menu from "@lucide/svelte/icons/menu";
import Search from "@lucide/svelte/icons/search";
import X from "@lucide/svelte/icons/x";
import { router } from "../lib/route.svelte";
import MemoryCategories from "./MemoryCategories.svelte";

let drawer: HTMLDialogElement;
const items = [
  { href: "#/", label: "Home", icon: Home },
  { href: "#/continue", label: "Continue reading", icon: Bookmark },
  { href: "#/recent", label: "Recent conversations", icon: Clock },
  { href: "#/machines", label: "Machines", icon: Laptop },
  { href: "#/agents", label: "Agents", icon: Bot },
] as const;
const active = $derived(
  router.current.page === "browse"
    ? `#/${router.current.section}`
    : router.current.page === "home"
      ? "#/"
      : "",
);
</script>

<header class="app-header">
  <button type="button" class="icon-button" aria-label="Open navigation" onclick={() => drawer.showModal()}><Menu size={18} /></button>
  <a class="wordmark" href="#/" aria-label="Mash home"><img src="/mash-logo.png" alt="" width="20" height="18" />mash</a>
  {#if router.current.page !== "home" && router.current.page !== "search"}<a class="header-search" href="#/search" aria-label="Search chats"><Search size={16} /> <span>Search chats</span></a>{/if}
  <a class="header-status" href="#/status">Index status</a>
</header>
<dialog bind:this={drawer} class="navigation-drawer" aria-label="Navigation" onclick={(event) => { if (event.target === drawer) drawer.close(); }} onkeydown={(event) => { if (event.key === "Escape") drawer.close(); }}>
  <div class="drawer-content">
    <div class="drawer-heading"><a class="wordmark" href="#/" onclick={() => drawer.close()}>mash</a><button type="button" class="icon-button" aria-label="Close navigation" onclick={() => drawer.close()}><X size={18} /></button></div>
    <nav aria-label="Main navigation">
      {#each items as item (item.href)}<a class="drawer-link" href={item.href} onclick={() => drawer.close()} aria-current={active === item.href ? "page" : undefined}><item.icon size={16} />{item.label}</a>{/each}
      <div class="drawer-memory"><a class="drawer-link" href="#/records" onclick={() => drawer.close()}><Layers size={16} />Memory</a><MemoryCategories vertical onNavigate={() => drawer.close()} /></div>
      <a class="drawer-link" href="#/status" onclick={() => drawer.close()}><Activity size={16} />Index status</a>
    </nav>
  </div>
</dialog>
