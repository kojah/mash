<script lang="ts">
import LoadError from "./LoadError.svelte";

// A failed read's error, kept in place while its retry is in flight. The
// query clears its error the moment a retry starts; without this the alert
// would vanish and the reader could not tell a retry was running.
let {
  title,
  query,
  retryLabel,
}: {
  title: string;
  query: {
    readonly error: Error | null;
    readonly isFetching: boolean;
    refetch: () => unknown;
  };
  retryLabel?: string;
} = $props();
let retained = $state<Error | null>(null);
$effect(() => {
  if (query.error) retained = query.error;
  else if (!query.isFetching) retained = null;
});
</script>

{#if retained}<LoadError {title} message={retained.message} retry={() => query.refetch()} busy={query.isFetching} {...retryLabel ? { retryLabel } : {}} />{/if}
