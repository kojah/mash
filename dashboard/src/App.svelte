<script lang="ts">
import { QueryClient, QueryClientProvider } from "@tanstack/svelte-query";
import AppHeader from "./components/AppHeader.svelte";
import Home from "./components/Home.svelte";
import RecordsView from "./components/RecordsView.svelte";
import RecordView from "./components/RecordView.svelte";
import SearchView from "./components/SearchView.svelte";
import ServicesView from "./components/ServicesView.svelte";
import TranscriptView from "./components/TranscriptView.svelte";
import { router } from "./lib/route.svelte";

const client = new QueryClient();
let searchParams = $state<string | null>(null);
$effect(() => {
  if (router.current.page === "search") searchParams = router.current.params;
});
</script>

<QueryClientProvider client={client}>
<AppHeader />
{#if searchParams !== null}
  <div hidden={router.current.page !== "search"}>
    {#key searchParams}<SearchView params={searchParams} />{/key}
  </div>
{/if}
{#if router.current.page === "services"}
  <ServicesView params={router.current.params}/>
{:else if router.current.page === "records"}
  {#key router.current.params}<RecordsView params={router.current.params} />{/key}
{:else if router.current.page === "record"}
  {#key JSON.stringify([router.current.id, router.current.node])}<RecordView id={router.current.id} node={router.current.node} back={router.current.back} />{/key}
{:else if router.current.page === "chat"}
    <TranscriptView id={router.current.id} node={router.current.node} messageId={router.current.message} shared={router.current.shared} />
{:else if router.current.page === "search"}
  <!-- Keep the submitted search mounted while a chat is open. -->
{:else if router.current.page === "browse"}
  <Home section={router.current.section} />
{:else}
  <Home />
{/if}
</QueryClientProvider>
