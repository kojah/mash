<script lang="ts">
import Home from "./components/Home.svelte";
import TranscriptView from "./components/TranscriptView.svelte";
import { router } from "./lib/route.svelte";
</script>

{#if router.current.page === "chat"}
  <TranscriptView id={router.current.id} />
{:else}
  <Home />
{/if}
