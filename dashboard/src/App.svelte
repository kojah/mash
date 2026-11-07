<script lang="ts">
import { QueryClient, QueryClientProvider } from "@tanstack/svelte-query";
import Home from "./components/Home.svelte";
import RecordsView from "./components/RecordsView.svelte";
import RecordView from "./components/RecordView.svelte";
import TranscriptView from "./components/TranscriptView.svelte";
import { router } from "./lib/route.svelte";

const client = new QueryClient();
</script>

<QueryClientProvider client={client}>
{#if router.current.page === "records"}
  {#key router.current.params}<RecordsView params={router.current.params} />{/key}
{:else if router.current.page === "record"}
  {#key JSON.stringify([router.current.id, router.current.node])}<RecordView id={router.current.id} node={router.current.node} back={router.current.back} />{/key}
{:else if router.current.page === "chat"}
    <TranscriptView id={router.current.id} node={router.current.node} messageId={router.current.message} shared={router.current.shared} />
{:else}
  <Home />
{/if}
</QueryClientProvider>
