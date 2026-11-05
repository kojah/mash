import type { Agent } from "./types";

export interface AgentInfo {
  readonly name: string;
  /** A logo under public/agents, when one is available. */
  readonly logo?: string;
}

export const agents: Readonly<Record<Agent, AgentInfo>> = {
  "claude-code": { name: "Claude Code", logo: "/agents/claude.svg" },
  codex: { name: "Codex", logo: "/agents/openai.svg" },
  opencode: { name: "OpenCode" },
  copilot: { name: "GitHub Copilot", logo: "/agents/githubcopilot.svg" },
  cursor: { name: "Cursor" },
  antigravity: { name: "Antigravity" },
};
