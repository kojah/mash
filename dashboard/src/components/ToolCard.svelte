<script lang="ts">
import Bot from "@lucide/svelte/icons/bot";
import ChevronDown from "@lucide/svelte/icons/chevron-down";
import Copy from "@lucide/svelte/icons/copy";
import FileText from "@lucide/svelte/icons/file-text";
import Globe from "@lucide/svelte/icons/globe";
import Pencil from "@lucide/svelte/icons/pencil";
import Search from "@lucide/svelte/icons/search";
import Terminal from "@lucide/svelte/icons/terminal";
import type { ToolCall, ToolKind } from "../lib/types";

let { calls }: { calls: readonly ToolCall[] } = $props();

const icons = {
  read: FileText,
  search: Search,
  edit: Pencil,
  run: Terminal,
  web: Globe,
  agent: Bot,
} satisfies Record<ToolKind, unknown>;
const tones = { ok: "text-ok", bad: "text-bad", plain: "text-faint" } as const;

function diffClass(line: string): string {
  if (line.startsWith("+")) return "bg-[#e7f5ec] text-[#14532d]";
  if (line.startsWith("-")) return "bg-[#fdecec] text-[#8f1d14]";
  if (line.startsWith("@@")) return "text-faint";
  return "";
}
</script>

<section aria-label="Tool activity" class="overflow-hidden rounded-[14px] bg-surface text-sm">
  {#each calls as call, index (index)}
    {@const Icon = icons[call.kind]}
    <details class="group shadow-[inset_0_0.5px_0_var(--color-hairline)] first:shadow-none">
      <summary class="flex min-h-11 cursor-pointer list-none items-center gap-2.5 px-3.5 [&::-webkit-details-marker]:hidden">
        <Icon size={16} strokeWidth={1.75} class="shrink-0 text-muted" />
        <span class="min-w-0 flex-1 truncate">{call.verb} <span class="font-mono text-[13px]">{call.target}</span></span>
        <span class="shrink-0 text-xs font-medium tabular-nums {tones[call.tone]}">{call.status}</span>
        <ChevronDown size={13} strokeWidth={2.25} class="shrink-0 text-chevron transition-transform group-open:rotate-180" />
      </summary>
      <div class="flex flex-col gap-2 pr-3.5 pb-3.5 pl-[42px]">
        {#if call.cwd}<p class="m-0 font-mono text-xs text-muted">{call.cwd}</p>{/if}
        {#if call.diff}
          <pre class="m-0 overflow-x-auto rounded-[10px] border border-hairline py-2 font-mono text-[12.5px] leading-relaxed">{#each call.diff as line, i (i)}<span class="block px-3 {diffClass(line)}">{line}</span>{/each}</pre>
        {/if}
        {#if call.output}
          <div class="overflow-hidden rounded-[10px] bg-code">
            <pre class="m-0 overflow-x-auto px-3.5 py-2.5 font-mono text-xs leading-relaxed text-[#d1d5db]">{#if call.output.omitted > 0}<span class="block text-[#6b7280]">… {call.output.omitted.toLocaleString()} earlier lines</span>{/if}{#each call.output.lines as line, i (i)}<span class="block whitespace-pre-wrap">{line}</span>{/each}</pre>
            <div class="flex items-center gap-2.5 bg-[#1f2229] py-1.5 pr-1.5 pl-3.5 text-xs text-[#9ca3af]">
              <span class="flex-1">Showing the last {call.output.lines.length} of {(call.output.lines.length + call.output.omitted).toLocaleString()} lines</span>
              <button type="button" class="inline-flex min-h-[30px] items-center gap-1.5 rounded-md bg-white/10 px-2.5 text-xs text-[#e5e7eb]" onclick={() => navigator.clipboard?.writeText(call.output?.lines.join("\n") ?? "")}>
                <Copy size={13} strokeWidth={1.75} />Copy output
              </button>
            </div>
          </div>
        {/if}
        {#if call.missing}<p class="m-0 text-[13px] text-muted">{call.missing}</p>{/if}
      </div>
    </details>
  {/each}
</section>
