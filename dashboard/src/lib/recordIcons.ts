import BadgeCheck from "@lucide/svelte/icons/badge-check";
import CircleHelp from "@lucide/svelte/icons/circle-help";
import Folder from "@lucide/svelte/icons/folder";
import FolderGit2 from "@lucide/svelte/icons/folder-git-2";
import Gavel from "@lucide/svelte/icons/gavel";
import MessageSquare from "@lucide/svelte/icons/message-square";
import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
import User from "@lucide/svelte/icons/user";

/** The icon for each kind of memory, the same wherever a kind is shown. */
export const kindIcons = {
  decision: Gavel,
  preference: SlidersHorizontal,
  fact: BadgeCheck,
  open_question: CircleHelp,
} as const;

/** The icon for what a memory applies to. */
export const scopeIcons = {
  personal: User,
  repository: FolderGit2,
  local_project: Folder,
  chat: MessageSquare,
} as const;
