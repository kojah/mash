import type { Agent } from "./types";

export interface AgentInfo {
  readonly name: string;
  /** A vendored provider logo under public/agents. */
  readonly logo: string;
}

export const agents: Readonly<Record<Agent, AgentInfo>> = {
  "claude-code": { name: "Claude Code", logo: "/agents/claude-code.svg" },
  codex: { name: "Codex", logo: "/agents/codex.svg" },
  opencode: { name: "OpenCode", logo: "/agents/opencode.svg" },
  copilot: { name: "GitHub Copilot", logo: "/agents/githubcopilot.svg" },
  cursor: { name: "Cursor", logo: "/agents/cursor.svg" },
  antigravity: { name: "Antigravity", logo: "/agents/antigravity.svg" },
};
