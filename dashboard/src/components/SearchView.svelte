<script lang="ts">
import { isAgent } from "../lib/api";
import type { SearchFilters } from "../lib/types";
import SearchPanel from "./SearchPanel.svelte";

let { params }: { params: string } = $props();
const values = $derived(new URLSearchParams(params));
const agent = $derived(values.get("agent"));
const initial = $derived<SearchFilters>({
  agents: agent && isAgent(agent) ? [agent] : [],
  machines: values.get("machine") ? [values.get("machine") ?? ""] : [],
  projects: [],
  days: null,
});
</script>
<main class="page-column search-page"><SearchPanel {initial} initialQuery={values.get("q") ?? ""} autoSubmit onClose={() => {}} /></main>
