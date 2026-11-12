import BadgeCheck from "@lucide/svelte/icons/badge-check";
import CircleHelp from "@lucide/svelte/icons/circle-help";
import Gavel from "@lucide/svelte/icons/gavel";
import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";

/** The icon for each kind of memory, the same wherever a kind is shown. */
export const kindIcons = {
  decision: Gavel,
  preference: SlidersHorizontal,
  fact: BadgeCheck,
  open_question: CircleHelp,
} as const;
