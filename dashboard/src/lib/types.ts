/** The agent that produced a chat; matches mash's provider names. */
export type Agent = "claude-code" | "codex" | "opencode" | "copilot" | "cursor" | "antigravity";

export interface ChatSummary {
  readonly id: string;
  readonly title: string;
  readonly agent: Agent;
  readonly machine: string;
  readonly project: string;
  readonly updatedAt: string;
  readonly messageCount?: number;
  readonly updatedMs?: number;
}

export interface Machine {
  readonly name: string;
  readonly chats: number;
  readonly lastIndexed: string;
  readonly needsAttention: boolean;
}

export interface AgentSummary {
  readonly agent: Agent;
  readonly chats: number;
  readonly lastChat: string;
  readonly quiet: boolean;
}

export type ToolKind = "read" | "search" | "edit" | "run" | "web" | "agent";

/** One tool call and what the provider kept of its result. */
export interface ToolCall {
  readonly kind: ToolKind;
  readonly verb: string;
  readonly target: string;
  readonly status: string;
  readonly tone: "ok" | "bad" | "plain";
  readonly cwd?: string;
  readonly output?: { readonly lines: readonly string[]; readonly omitted: number };
  readonly diff?: readonly string[];
  /** Why no output is shown, when the provider kept none. */
  readonly missing?: string;
}

export type Block =
  | { readonly type: "text"; readonly markdown: string }
  | {
      readonly type: "code";
      readonly language: string;
      readonly file?: string;
      readonly code: string;
    }
  | { readonly type: "tools"; readonly calls: readonly ToolCall[] };

export interface Message {
  readonly id: string;
  readonly role: string;
  readonly ordinal?: number;
  readonly activity?: string;
  readonly at: string;
  readonly blocks: readonly Block[];
}

export interface Transcript {
  readonly chat: ChatSummary;
  readonly messages: readonly Message[];
}

export interface SearchFilters {
  readonly agents: readonly Agent[];
  readonly machines: readonly string[];
  readonly projects: readonly string[];
  readonly days: number | null;
}

export interface SearchHit {
  readonly chat: ChatSummary;
  readonly messageId: string;
  readonly before: string;
  readonly match: string;
  readonly after: string;
}
