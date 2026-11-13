<script lang="ts">
import Folder from "@lucide/svelte/icons/folder";
import Laptop from "@lucide/svelte/icons/laptop";
import { QueryClient, QueryClientProvider } from "@tanstack/svelte-query";
import AgentMark from "./components/AgentMark.svelte";
import BrowseDetail from "./components/BrowseDetail.svelte";
import BrowseTiles from "./components/BrowseTiles.svelte";
import Console from "./components/Console.svelte";
import Fleet from "./components/Fleet.svelte";
import History from "./components/History.svelte";
import MachineExtras from "./components/MachineExtras.svelte";
import Missing from "./components/Missing.svelte";
import Overview from "./components/Overview.svelte";
import Projects from "./components/Projects.svelte";
import Rail from "./components/Rail.svelte";
import RecordsView from "./components/RecordsView.svelte";
import RecordView from "./components/RecordView.svelte";
import Search from "./components/Search.svelte";
import ServicesView from "./components/ServicesView.svelte";
import Settings from "./components/Settings.svelte";
import SignIn from "./components/SignIn.svelte";
import TopBar from "./components/TopBar.svelte";
import TranscriptView from "./components/TranscriptView.svelte";
import Transfer from "./components/Transfer.svelte";
import { agents } from "./lib/agents";
import { isAgent } from "./lib/api";
import { projectLabel } from "./lib/browse";
import { connect, onChange, throttle } from "./lib/live";
import { catalogOptions, stableRead } from "./lib/queries";
import { router } from "./lib/route.svelte";
import { check, gate, open } from "./lib/session.svelte";

const client = new QueryClient({ defaultOptions: { queries: stableRead } });
// Nothing is read until the dashboard has said whether it wants a sign-in.
void check();
function signedIn(): void {
  // Answers refused before the sign-in must not be shown as failures after it.
  client.clear();
  open();
}
// One stream per tab says when the index changed. Chat lists follow it,
// at most once every few seconds while indexing is busy.
$effect(() => {
  if (gate.value !== "open") return;
  const refresh = throttle(
    () => void client.invalidateQueries({ queryKey: catalogOptions.queryKey }),
    5000,
  );
  // Lists of memories follow too. A memory search keeps its results, like
  // any submitted search, and an open record is reread only on request.
  const memories = throttle(
    () =>
      void client.invalidateQueries({
        predicate: ({ queryKey: [kind, params] }) =>
          kind === "chat-memory" ||
          (kind === "records" &&
            typeof params === "string" &&
            !new URLSearchParams(params).has("q")),
      }),
    5000,
  );
  const off = onChange((change) => {
    refresh();
    if (change.all || change.records) memories();
  });
  // A stream refused because the session ended should bring up the sign-in.
  const stop = connect(() => void check());
  return () => {
    stop();
    off();
    refresh.cancel();
    memories.cancel();
  };
});
const route = $derived(router.current);
// Keep the submitted search mounted while a chat is open, so going back
// returns to the same results.
let searchParams = $state<string | null>(null);
$effect(() => {
  if (route.page === "search") searchParams = route.params;
});
let railOpen = $state(false);
$effect(() => {
  void route;
  railOpen = false;
});
</script>

<svelte:window
  onkeydown={(event) => {
    if (event.key === "Escape" && railOpen) {
      railOpen = false;
      document.getElementById("burger")?.focus();
    }
  }}
/>
{#if gate.value === "locked"}
  <SignIn done={signedIn} />
{:else if gate.value === "open"}
<QueryClientProvider {client}>
  <Rail open={railOpen} close={() => (railOpen = false)} />
  <div id="col">
    <TopBar {railOpen} toggleRail={() => (railOpen = !railOpen)} />
    <main id="main">
      {#if searchParams !== null}
        <div class="stack" hidden={route.page !== "search"}>
          {#key searchParams}<Search params={searchParams} />{/key}
        </div>
      {/if}
      {#if route.page === "overview"}
        <Overview />
      {:else if route.page === "search"}
        <!-- Rendered above so it survives opening a chat. -->
      {:else if route.page === "history"}
        <History />
      {:else if route.page === "export"}
        <Transfer params={route.params} />
      {:else if route.page === "projects"}
        <Projects />
      {:else if route.page === "project"}
        {#key route.name}
          <BrowseDetail field="project" value={route.name} title={projectLabel(route.name)} back={{ href: "#/projects", label: "projects" }}>
            {#snippet icon()}<Folder size={28} color="var(--warn)" aria-hidden="true" />{/snippet}
          </BrowseDetail>
        {/key}
      {:else if route.page === "agents"}
        <BrowseTiles field="agent" />
      {:else if route.page === "agent"}
        {@const agent = route.agent}
        {#if isAgent(agent)}
          {#key agent}
            <BrowseDetail field="agent" value={agent} title={agents[agent].name} back={{ href: "#/agents", label: "agents" }}>
              {#snippet icon()}<AgentMark {agent} size={28} />{/snippet}
            </BrowseDetail>
          {/key}
        {:else}
          <Missing />
        {/if}
      {:else if route.page === "machines"}
        <BrowseTiles field="machine" />
      {:else if route.page === "machine"}
        {@const id = route.id}
        {#key id}
          <BrowseDetail field="machine" value={id} title={id} back={{ href: "#/machines", label: "machines" }}>
            {#snippet icon()}<Laptop size={28} color="var(--blue)" aria-hidden="true" />{/snippet}
            {#snippet extra()}<MachineExtras machineId={id} />{/snippet}
          </BrowseDetail>
        {/key}
      {:else if route.page === "services"}
        <ServicesView params={route.params} />
      {:else if route.page === "fleet"}
        <Fleet action={route.action} />
      {:else if route.page === "console"}
        <Console />
      {:else if route.page === "settings"}
        <Settings />
      {:else if route.page === "records"}
        {#key route.params}<RecordsView params={route.params} />{/key}
      {:else if route.page === "record"}
        {#key JSON.stringify([route.id, route.node])}<RecordView id={route.id} node={route.node} back={route.back} />{/key}
      {:else if route.page === "chat"}
        <TranscriptView id={route.id} node={route.node} messageId={route.message} shared={route.shared} />
      {:else}
        <Missing />
      {/if}
    </main>
  </div>
  <!-- The drawer's own button and Escape close it for keyboards and screen readers. -->
  {#if railOpen}<div id="scrim" aria-hidden="true" onclick={() => (railOpen = false)}></div>{/if}
</QueryClientProvider>
{/if}
