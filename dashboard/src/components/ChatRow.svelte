<script lang="ts">
import { reading } from "../lib/reading.svelte";
import { chatHref } from "../lib/route.svelte";
import type { ChatSummary } from "../lib/types";
import AgentMark from "./AgentMark.svelte";
import MetaChips from "./MetaChips.svelte";

let { chat }: { chat: ChatSummary } = $props();
// A chat opened earlier in this browser session reopens where it was left.
const href = $derived.by(() => {
  const position = reading.positions.find(
    (position) => position.id === chat.id && position.node === chat.machine,
  );
  return chatHref(chat.id, chat.machine, position?.message ?? "", position?.shared ?? false);
});
</script>

<a class="row" {href}>
  <span class="row-t"><AgentMark agent={chat.agent} /><span>{chat.title}</span></span>
  <span class="elb" style:margin-top="2px"><MetaChips {chat} /></span>
</a>
