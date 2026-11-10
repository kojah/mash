<script lang="ts">
import type { ServiceState } from "../lib/services.svelte";
import { stateLabel } from "../lib/services.svelte";

let { state, label }: { state: ServiceState; label?: string } = $props();
const tone = {
  healthy: "",
  degraded: "warn",
  stopped: "warn",
  failed: "bad",
  unknown: "none",
} as const;
</script>

<span class="svc-st"><i class="dot {tone[state]}" aria-hidden="true"></i><span>{label ?? stateLabel[state]}</span></span>
