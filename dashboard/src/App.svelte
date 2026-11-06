<script lang="ts">
import { QueryClient, QueryClientProvider } from "@tanstack/svelte-query";
import Home from "./components/Home.svelte";
import TranscriptView from "./components/TranscriptView.svelte";
import { router } from "./lib/route.svelte";

const client = new QueryClient();
</script>

<QueryClientProvider client={client}>
{#if router.current.page === "chat"}
  {#key JSON.stringify([router.current.id, router.current.node, router.current.message])}
    <TranscriptView id={router.current.id} node={router.current.node} messageId={router.current.message} />
  {/key}
{:else}
  <Home />
{/if}
</QueryClientProvider>
