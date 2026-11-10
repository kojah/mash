<script lang="ts">
import { createQuery } from "@tanstack/svelte-query";
import { catalogOptions } from "../lib/queries";
import { reading } from "../lib/reading.svelte";
import ChatRow from "./ChatRow.svelte";

// Chats opened in this browser session, most recent first. Positions live in
// the page, so they are gone after a reload.
const catalogue = createQuery(() => catalogOptions);
const chats = $derived(
  reading.positions.flatMap(
    (position) =>
      catalogue.data?.chats.find(
        (chat) => chat.id === position.id && chat.machine === position.node,
      ) ?? [],
  ),
);
</script>

<div><h1 class="h1">Viewing history</h1><p class="meta">Chats you opened in this browser session. Each reopens where you left it.</p></div>
<section>
  {#each chats as chat (chat.machine + chat.id)}<ChatRow {chat} />{/each}
  {#if !chats.length}<div class="state-c"><p>Nothing yet. Open a chat and it will be listed here until you reload the page.</p></div>{/if}
</section>
