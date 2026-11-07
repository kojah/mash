import type { Kind } from "../src/lib/records";

export function recordFixture(id = "fact-1", kind: Kind = "fact") {
  const details = {
    decision: {
      kind: "decision",
      state: "accepted",
      alternatives: ["Keep the old port"],
      consequences: ["Update the clients"],
    },
    preference: { kind: "preference", state: "active", conditions: ["Incident reports"] },
    fact: { kind: "fact", state: "current", reported_by: "Fixture user", valid_until: null },
    open_question: {
      kind: "open_question",
      state: "resolved",
      answer: "The platform team owns the review.",
    },
  }[kind];
  return {
    id,
    owner: "fixture-owner",
    scope: { kind: "personal" },
    current_revision: 1,
    conflicted: false,
    revisions: [
      {
        revision: 1,
        recorded_at: 1_700_000_000_000,
        evidence_current: true,
        confirmations: 0,
        confirmation_history: [],
        body: {
          statement: `${kind} fixture on port 8080`,
          rationale: "Read the source reply",
          recorded_by: "Fixture agent",
          effective_at: null,
          review: "established",
          details,
        },
        evidence: [
          {
            chat_id: "chat-1",
            message_id: "message-1",
            fingerprint: "fixture-fingerprint",
            quote: "The service uses port 8080.",
            purpose: "report",
            source_role: "user",
          },
        ],
      },
    ],
    links: [],
    next_before_revision: null,
    next_link_offset: null,
  };
}
