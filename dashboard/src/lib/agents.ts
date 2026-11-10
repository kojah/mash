import type { Agent } from "./types";

export interface AgentInfo {
  readonly name: string;
  /** A vendored provider logo under public/agents. */
  readonly logo: string;
  /** The colour the single-colour mark is drawn in. */
  readonly tint: string;
}

export const agents: Readonly<Record<Agent, AgentInfo>> = {
  "claude-code": { name: "Claude Code", logo: "/agents/claude.svg", tint: "#d97757" },
  codex: { name: "Codex", logo: "/agents/openai.svg", tint: "#19c39c" },
  opencode: { name: "OpenCode", logo: "/agents/opencode.svg", tint: "#c9c4b8" },
  copilot: { name: "GitHub Copilot", logo: "/agents/githubcopilot.svg", tint: "#b6a6f0" },
  cursor: { name: "Cursor", logo: "/agents/cursor.svg", tint: "#f0ece4" },
  antigravity: { name: "Antigravity", logo: "/agents/antigravity.svg", tint: "#7fb8ea" },
};
