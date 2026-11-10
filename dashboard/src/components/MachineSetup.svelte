<script lang="ts">
import { createQuery } from "@tanstack/svelte-query";
import { agents } from "../lib/agents";
import { setupChain, setupHeadline, setupOptions } from "../lib/services.svelte";
import AgentMark from "./AgentMark.svelte";
import ServiceStatus from "./ServiceStatus.svelte";

// `compact` is the one-line summary on the machines list; the machine's own
// page shows each part of the integration and what to do about it.
let {
  machineId,
  enabled,
  compact = false,
}: { machineId: string; enabled: boolean; compact?: boolean } = $props();
const setup = createQuery(() => setupOptions(machineId, enabled));
const partWord = { ok: "ok", attention: "needs attention", unverified: "not verifiable" } as const;
const partDot = { ok: "healthy", attention: "degraded", unverified: "unknown" } as const;
</script>

{#if compact}
  {#if setup.data}
    <ul class="setup-summary" aria-label="Agent setup on {machineId}">
      {#each setup.data.agents as agent (agent.agent)}
        {@const headline = setupHeadline(agent)}
        <li><AgentMark agent={agent.agent} size={14}/><span>{agents[agent.agent].name}</span><ServiceStatus state={headline.state} label={headline.label}/></li>
      {/each}
    </ul>
  {:else if setup.error}
    <p class="service-checked">Agent setup unavailable: {setup.error.message}</p>
  {/if}
{:else}
  <section>
    <h2>Agent setup</h2>
    <p class="text-muted">Whether each agent on this machine is connected to Mash, with its plugin, skill and hooks in place. Mash inspects settings and files; it does not start the agents.</p>
    {#if setup.isPending && enabled}<p role="status">Checking agent setup…</p>{/if}
    {#if setup.error}<p role="alert">Agent setup is unavailable: {setup.error.message}</p><p class="text-muted">A machine running an older Mash cannot report its setup. Update it, then check again.</p>{/if}
    {#if setup.data}
      {#each setup.data.agents as agent (agent.agent)}
        {@const headline = setupHeadline(agent)}
        {@const chain = setupChain(agent)}
        <div class="setup-agent">
          <div class="setup-agent-heading"><h3><AgentMark agent={agent.agent}/>{agents[agent.agent].name}</h3><ServiceStatus state={headline.state} label={headline.label}/></div>
          {#if agent.expected}
            <ol class="setup-chain">
              {#each chain as part (part.label)}
                <li><span class="service-dot {partDot[part.state]}" aria-hidden="true"></span><span>{part.label}</span><small>{partWord[part.state]}</small></li>
              {/each}
            </ol>
            {#each chain.filter((part) => part.notes.length > 0) as part (part.label)}
              <div class="setup-notes"><h4>{part.label}</h4>
                {#each part.notes as note (note.message)}<p>{note.message}{#if note.remediation && part.state === "attention"} <span class="setup-fix">{note.remediation}</span>{/if}</p>{/each}
              </div>
            {/each}
          {/if}
        </div>
      {/each}
      <p class="service-checked">Inspected by Mash build {setup.data.build.slice(0, 8)}</p>
    {/if}
  </section>
{/if}
