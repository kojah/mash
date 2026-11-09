<script lang="ts">
import { createQuery } from "@tanstack/svelte-query";
import { catalogOptions } from "../lib/queries";

const catalogue = createQuery(() => catalogOptions);
</script>

<section aria-labelledby="index-status">
  <h2 id="index-status">Index</h2>
  <p class="text-muted">Mash reads indexed history. Indexing runs separately.</p>
  <p>Local node: {catalogue.data?.localNode ?? (catalogue.error ? "Unavailable" : "Loading…")}</p>
  {#each catalogue.data?.warnings ?? [] as warning (warning)}<p role="status">{warning}</p>{/each}
  <button type="button" class="plain-button" disabled={catalogue.isFetching} onclick={() => catalogue.refetch()}>{catalogue.isFetching ? "Checking…" : "Refresh index status"}</button>
</section>
