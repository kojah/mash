// The dashboard's data source. Until `mash dash` serves the JSON API these
// functions return the mockup's sample data, with the same shapes.
import { sample } from "./sample";
import type {
  AgentSummary,
  ChatSummary,
  Machine,
  SearchFilters,
  SearchHit,
  Transcript,
} from "./types";

export interface Overview {
  readonly recent: readonly ChatSummary[];
  readonly machines: readonly Machine[];
  readonly agents: readonly AgentSummary[];
}

export async function overview(): Promise<Overview> {
  return {
    recent: sample.chats.slice(0, 5),
    machines: sample.machines,
    agents: sample.agents,
  };
}

/** An empty query lists the chats that match the filters, newest first. */
export async function search(query: string, filters: SearchFilters): Promise<readonly SearchHit[]> {
  const matches = (chat: ChatSummary): boolean =>
    (filters.agents.length === 0 || filters.agents.includes(chat.agent)) &&
    (filters.machines.length === 0 || filters.machines.includes(chat.machine)) &&
    (filters.projects.length === 0 || filters.projects.includes(chat.project));
  const hits = sample.hits.filter((hit) => matches(hit.chat));
  if (query.trim() !== "") return hits;
  return sample.chats
    .filter(matches)
    .map((chat) => ({ chat, messageId: "", before: "", match: "", after: "" }));
}

export async function transcript(chatId: string): Promise<Transcript | undefined> {
  return sample.transcripts[chatId];
}

export async function chats(): Promise<readonly ChatSummary[]> {
  return sample.chats;
}
