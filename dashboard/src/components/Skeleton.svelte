<script lang="ts">
// Placeholder rows shown while a page's first read is in flight.
let { rows = 4 }: { rows?: number } = $props();
</script>

<div role="status" aria-label="Loading">
  {#each { length: rows } as _, index (index)}
    <div class="skrow"><span class="sk" style:height="13px" style:width="{55 + ((index * 17) % 35)}%"></span><span class="sk" style:height="10px" style:width="{30 + ((index * 11) % 25)}%"></span></div>
  {/each}
</div>
