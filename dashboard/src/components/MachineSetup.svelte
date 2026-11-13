<script lang="ts">
import Check from "@lucide/svelte/icons/check";
import CircleHelp from "@lucide/svelte/icons/circle-help";
import Copy from "@lucide/svelte/icons/copy";
import Laptop from "@lucide/svelte/icons/laptop";
import ListChecks from "@lucide/svelte/icons/list-checks";
import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
import { createQuery } from "@tanstack/svelte-query";
import { agents } from "../lib/agents";
import { fleetHref } from "../lib/route.svelte";
import { setupChain, setupFix, setupHeadline, setupOptions } from "../lib/services.svelte";
import AgentMark from "./AgentMark.svelte";

// One machine's agents as rows of the agent setup list: each agent's
// integration part by part, opening to what to do about a part that needs
// attention. The list itself, with its heading, belongs to the page.
let {
  machineId,
  enabled,
  showMachine = true,
}: { machineId: string; enabled: boolean; showMachine?: boolean } = $props();
const setup = createQuery(() => setupOptions(machineId, enabled));
let copied = $state("");
async function copy(agent: string, command: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(command);
    copied = agent;
  } catch {
    copied = "";
  }
}
const partWord = { ok: "ok", attention: "needs attention", unverified: "not verifiable" } as const;
</script>

{#if setup.isPending && enabled}<p class="meta rw-note" role="status">Checking agent setup on {machineId}…</p>{/if}
{#if setup.error}<p class="meta rw-note"><span class="warn" role="alert">Agent setup on {machineId} is unavailable: {setup.error.message}</span> A machine running an older Mash cannot report its setup. Update it, then check again.</p>{/if}
{#each setup.data?.agents ?? [] as agent (agent.agent)}
  {@const headline = setupHeadline(agent)}
  {@const chain = setupChain(agent)}
  {@const fix = setupFix(agent, agents[agent.agent].name)}
  {@const tone = headline.state === "healthy" ? "ok" : headline.state === "unknown" ? "none" : "warn"}
  {#if agent.expected}
    <details class="rw-d" open={!!fix}>
      <summary class="rw">
        <span class="rw-n"><AgentMark agent={agent.agent} size={17} /><strong>{agents[agent.agent].name}</strong></span>
        {#if showMachine}<span><span class="mt"><Laptop size={12} aria-hidden="true" />{machineId}</span></span>{/if}
        <span class="rw-parts">
          {#each chain as part (part.label)}
            <span class={part.state === "ok" ? "ok" : part.state === "attention" ? "warn" : "none"} title="{part.label}: {partWord[part.state]}">
              {#if part.state === "ok"}<Check size={15} aria-label="{part.label}: ok" />{:else if part.state === "attention"}<TriangleAlert size={15} aria-label="{part.label}: needs attention" />{:else}<CircleHelp size={15} aria-label="{part.label}: not verifiable" />{/if}
            </span>
          {/each}
        </span>
        <span class="svc-st {tone}">{headline.label}</span>
      </summary>
      <div class="rw-b">
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
        {#if fix}
          <div class="fix">
            <p class="why"><TriangleAlert size={16} aria-hidden="true" /><span>{fix.cause}</span></p>
            <ol>
              {#each fix.steps as step, index (step)}
                <li>
                  <span>{step}{#if fix.deploy && index === 0}{" "}<a class="a" href={fleetHref("deploy")}>Deploy it from Fleet manager</a>, or install it there yourself.{/if}</span>
                  {#if fix.command && index === fix.commandStep}
                    <div class="cmd"><code>{fix.command}</code><button type="button" aria-label={copied === agent.agent ? "Copied" : "Copy the command"} title={copied === agent.agent ? "Copied" : "Copy"} onclick={() => copy(agent.agent, fix.command ?? "")}>{#if copied === agent.agent}<Check size={16} aria-hidden="true" />{:else}<Copy size={16} aria-hidden="true" />{/if}</button></div>
                  {/if}
                </li>
              {/each}
            </ol>
          </div>
          <details class="fold" style:margin-top="8px">
            <summary><ListChecks size={15} aria-hidden="true" />{fix.checks === 1 ? "The check behind this" : `The ${fix.checks} checks behind this`}</summary>
            <div>
              {#each chain.filter((part) => part.notes.length > 0) as part (part.label)}
            {#each part.notes as note (note.message)}
              <p class="meta" style:margin-top="8px" style:overflow-wrap="anywhere"><span class={part.state === "attention" ? "warn" : ""}>{part.label}:</span> {note.message}{#if note.remediation && part.state === "attention"} <span style:display="block" style:color="var(--body)">{note.remediation}</span>{/if}</p>
            {/each}
          {/each}
            </div>
          </details>
        {:else}
          {#each chain.filter((part) => part.notes.length > 0) as part (part.label)}
            {#each part.notes as note (note.message)}
              <p class="meta" style:margin-top="8px" style:overflow-wrap="anywhere"><span class={part.state === "attention" ? "warn" : ""}>{part.label}:</span> {note.message}{#if note.remediation && part.state === "attention"} <span style:display="block" style:color="var(--body)">{note.remediation}</span>{/if}</p>
            {/each}
          {/each}
        {/if}
        <p class="meta" style:margin-top="8px">Inspected by build {setup.data?.build.slice(0, 8)}.</p>
      </div>
    </details>
  {:else}
    <div class="rw">
      <span class="rw-n"><AgentMark agent={agent.agent} size={17} /><strong>{agents[agent.agent].name}</strong></span>
      {#if showMachine}<span><span class="mt"><Laptop size={12} aria-hidden="true" />{machineId}</span></span>{/if}
      <span></span>
      <span class="svc-st {tone}">{headline.label}</span>
    </div>
  {/if}
{/each}
