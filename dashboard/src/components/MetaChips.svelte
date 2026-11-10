<script lang="ts">
import Bot from "@lucide/svelte/icons/bot";
import Folder from "@lucide/svelte/icons/folder";
import Laptop from "@lucide/svelte/icons/laptop";
import { agents } from "../lib/agents";
import { projectLabel, whenLabel } from "../lib/browse";
import type { ChatSummary } from "../lib/types";

// The agent, machine and project of a chat as quiet chips, then its time.
let { chat, extra = "" }: { chat: ChatSummary; extra?: string } = $props();
const now = Date.now();
</script>

<span class="mts">
  <span class="mt"><Bot size={12} aria-hidden="true" />{agents[chat.agent].name}</span>
  <span class="mt"><Laptop size={12} aria-hidden="true" />{chat.machine}</span>
  {#if chat.project}<span class="mt"><Folder size={12} aria-hidden="true" />{projectLabel(chat.project)}</span>{/if}
  <span class="mtx">{whenLabel(chat.updatedMs, now)}</span>
  {#if extra}<span class="mtx">{extra}</span>{/if}
</span>
