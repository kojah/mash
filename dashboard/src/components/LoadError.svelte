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

<div class="state-c" role="alert" aria-busy={busy} style:display="flex" style:flex-wrap="wrap" style:align-items="flex-start" style:gap="12px">
  <CircleAlert size={18} color="var(--warn)" aria-hidden="true" style="flex:none;margin-top:2px" />
  <div style:flex="1 1 200px" style:min-width="0" style:overflow-wrap="anywhere"><strong style:font-weight="500">{title}</strong><p class="meta" style:margin-top="4px" style:font-size="12.5px">{detail}</p></div>
  <button type="button" class="btn" onclick={retry} disabled={busy}>
    <RotateCw size={14} aria-hidden="true" />{busy ? "Trying again…" : retryLabel}
  </button>
</div>
