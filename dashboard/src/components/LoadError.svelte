<script lang="ts">
import CircleAlert from "@lucide/svelte/icons/circle-alert";
import RotateCw from "@lucide/svelte/icons/rotate-cw";

let {
  title,
  message,
  retry,
  busy = false,
  retryLabel = "Retry",
}: {
  title: string;
  message: string;
  retry: () => unknown;
  busy?: boolean;
  retryLabel?: string;
} = $props();
const detail = $derived(
  ["Load failed", "Failed to fetch", "NetworkError when attempting to fetch resource."].includes(
    message,
  )
    ? "Could not reach Mash. Check your connection and that the dashboard server is running, then retry."
    : message,
);
</script>

<div class="load-error" role="alert" aria-busy={busy}>
  <CircleAlert size={19} aria-hidden="true" />
  <div class="load-error-copy"><strong>{title}</strong><p>{detail}</p></div>
  <button type="button" class="plain-button load-error-retry" onclick={retry} disabled={busy}>
    <RotateCw size={14} aria-hidden="true" />{busy ? "Trying again…" : retryLabel}
  </button>
</div>
