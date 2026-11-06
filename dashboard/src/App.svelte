<script lang="ts">
import { QueryClient, QueryClientProvider } from "@tanstack/svelte-query";
import Home from "./components/Home.svelte";
import TranscriptView from "./components/TranscriptView.svelte";
import { router } from "./lib/route.svelte";

const client = new QueryClient();
</script>

<QueryClientProvider client={client}>
{#if router.current.page === "chat"}
    <TranscriptView id={router.current.id} node={router.current.node} messageId={router.current.message} />
{:else}
  <Home />
{/if}
</QueryClientProvider>
