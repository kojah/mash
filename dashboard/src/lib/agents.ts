import { asset } from "./asset";
import type { Agent } from "./types";

export interface AgentInfo {
  readonly name: string;
  /** A vendored provider logo under public/agents. */
  readonly logo: string;
  /** The colour the single-colour mark is drawn in. */
  readonly tint: string;
}

export const agents: Readonly<Record<Agent, AgentInfo>> = {
  "claude-code": { name: "Claude Code", logo: asset("agents/claude.svg"), tint: "#d97757" },
  codex: { name: "Codex", logo: asset("agents/openai.svg"), tint: "#f0ece4" },
  opencode: { name: "OpenCode", logo: asset("agents/opencode.svg"), tint: "#bdb8ad" },
  copilot: { name: "GitHub Copilot", logo: asset("agents/githubcopilot.svg"), tint: "#b6a6f0" },
  cursor: { name: "Cursor", logo: asset("agents/cursor.svg"), tint: "#f0ece4" },
  antigravity: { name: "Antigravity", logo: asset("agents/antigravity.svg"), tint: "#7fb8ea" },
};
