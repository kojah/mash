<script lang="ts">
import Check from "@lucide/svelte/icons/check";
import CircleHelp from "@lucide/svelte/icons/circle-help";
import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
import { createQuery } from "@tanstack/svelte-query";
import { agents } from "../lib/agents";
import { setupChain, setupHeadline, setupOptions } from "../lib/services.svelte";
import AgentMark from "./AgentMark.svelte";

// Each agent's integration on one machine, part by part, with what to do
// about any part that needs attention.
let { machineId, enabled }: { machineId: string; enabled: boolean } = $props();
const setup = createQuery(() => setupOptions(machineId, enabled));
const partWord = { ok: "ok", attention: "needs attention", unverified: "not verifiable" } as const;
</script>

  <section>
    <h2 class="h2">agent setup</h2>
    {#if setup.isPending && enabled}<p class="meta" role="status">Checking agent setup…</p>{/if}
    {#if setup.error}<p class="meta warn" role="alert">Agent setup is unavailable: {setup.error.message}</p><p class="meta">A machine running an older Mash cannot report its setup. Update it, then check again.</p>{/if}
    {#if setup.data}
      {#each setup.data.agents as agent (agent.agent)}
        {@const headline = setupHeadline(agent)}
        {@const chain = setupChain(agent)}
        <div style:margin-bottom="14px">
          <div class="hd" style:align-items="center"><h3 style:display="flex" style:align-items="center" style:gap="8px" style:font-size="15px" style:font-weight="500"><AgentMark agent={agent.agent} size={17} />{agents[agent.agent].name}</h3><span class={headline.state === "healthy" ? "ok" : headline.state === "unknown" ? "none" : "warn"} style:font-size="13.5px">{headline.label}</span></div>
          {#if agent.expected}
            <div class="set">
              {#each chain as part (part.label)}
                <div>
                  <span class={part.state === "ok" ? "ok" : part.state === "attention" ? "warn" : "none"} style:display="flex">
                    {#if part.state === "ok"}<Check size={16} aria-hidden="true" />{:else if part.state === "attention"}<TriangleAlert size={16} aria-hidden="true" />{:else}<CircleHelp size={16} aria-hidden="true" />{/if}
                  </span>
                  <span class="g">{part.label}</span>
                  <span class={part.state === "attention" ? "warn" : "n"} style:font-size="13px">{partWord[part.state]}</span>
                </div>
              {/each}
            </div>
            {#each chain.filter((part) => part.notes.length > 0) as part (part.label)}
              {#each part.notes as note (note.message)}
                <p class="meta" style:margin-top="8px" style:overflow-wrap="anywhere"><span class={part.state === "attention" ? "warn" : ""}>{part.label}:</span> {note.message}{#if note.remediation && part.state === "attention"} <span style:display="block" style:color="var(--body)">{note.remediation}</span>{/if}</p>
              {/each}
            {/each}
          {/if}
        </div>
      {/each}
      <p class="meta">Mash reads settings and files for this; it does not start the agents. Inspected by build {setup.data.build.slice(0, 8)}.</p>
    {/if}
  </section>
