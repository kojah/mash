import { describe, expect, it } from "vitest";
import { parseStatus } from "./status";

const body = {
  ready: true,
  chats: 3,
  messages: 40,
  searchable_messages: 12,
  models_installed: true,
  realized_mode: "hybrid",
  database_path: "/data/mash.sqlite3",
  recommended_action: null,
  configuration: {
    path: "/config/mash/config.toml",
    loaded: true,
    providers: { codex: { roots: ["/h/.codex/sessions"] }, "future-agent": { roots: ["/x"] } },
    index: { since_days: 90, destination: "both" },
    embeddings: { backend: "remote", token_env: "MASH_EMBEDDING_TOKEN" },
    qdrant: { url: "http://127.0.0.1:6333", vector_search: { algorithm: "hnsw" } },
    views: { query: { ranking: "contextual-rerank" } },
    records: { enabled: true },
    redaction: { output: true, publication: false },
    shared: { group_id: "personal", node_id: "xenia", api_url: "http://api:8788", token_env: "T" },
  },
};

describe("status", () => {
  it("keeps what the settings page shows and drops credential details", () => {
    const status = parseStatus(body);
    expect(status.configuration.providers).toEqual([
      { agent: "codex", roots: ["/h/.codex/sessions"] },
    ]);
    expect(status.configuration.shared).toEqual({
      group: "personal",
      node: "xenia",
      url: "http://api:8788",
    });
    expect(JSON.stringify(status)).not.toContain("TOKEN");
    expect(status.configuration.sinceDays).toBe(90);
    expect(status.nextStep).toBeNull();
  });
  it("accepts a machine with no shared index and rejects a malformed report", () => {
    const local = parseStatus({ ...body, configuration: { ...body.configuration, shared: null } });
    expect(local.configuration.shared).toBeNull();
    expect(() => parseStatus({ ...body, chats: -1 })).toThrow();
    expect(() =>
      parseStatus({ ...body, configuration: { ...body.configuration, qdrant: 1 } }),
    ).toThrow();
  });
});
