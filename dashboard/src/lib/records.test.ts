import { describe, expect, it } from "vitest";
import { recordFixture } from "../../e2e/record-fixtures";
import { kinds, parseRecord, parseRecordPage, timingLabel } from "./records";

describe("record API boundary", () => {
  it("marks future and expired conclusions at their time boundaries", () => {
    const revision = parseRecord(recordFixture()).revisions[0];
    if (revision?.details.kind !== "fact") throw new Error("Missing fixture fact");
    expect(timingLabel({ ...revision, effectiveAt: 101 }, 100)).toBe("Not yet effective");
    expect(
      timingLabel(
        { ...revision, effectiveAt: 100, details: { ...revision.details, validUntil: 100 } },
        100,
      ),
    ).toBeNull();
    expect(
      timingLabel(
        { ...revision, effectiveAt: null, details: { ...revision.details, validUntil: 99 } },
        100,
      ),
    ).toBe("Expired");
  });
  it("decodes all four kinds without interpreting archived markup", () => {
    for (const kind of kinds) {
      const fixture = recordFixture("id", kind);
      const record = parseRecord(fixture);
      expect(record.revisions[0]?.details.kind).toBe(kind);
      expect(record.revisions[0]?.evidence[0]?.messageId).toBe("message-1");
      expect(record.node).toBe("");
    }
    const fixture = recordFixture();
    const revision = fixture.revisions[0];
    const parsed = parseRecord({
      ...fixture,
      node_id: "remote/one",
      revisions: [
        { ...revision, body: { ...revision?.body, statement: "<script>untrusted</script>" } },
      ],
    });
    expect(parsed.node).toBe("remote/one");
    expect(parsed.revisions[0]?.statement).toBe("<script>untrusted</script>");
  });
  it("preserves empty shared pages with a continuation cursor", () => {
    expect(
      parseRecordPage({
        records: [],
        next_offset: null,
        next_cursor: { node_id: "remote", offset: 20 },
      }),
    ).toEqual({ records: [], nextOffset: null, nextCursor: { node: "remote", offset: 20 } });
  });
  it("rejects malformed records and cursors rather than silently hiding them", () => {
    const fixture = recordFixture();
    const revision = fixture.revisions[0];
    for (const value of [
      { ...fixture, revisions: [] },
      { ...fixture, conflicted: "false" },
      { ...fixture, current_revision: 0 },
      { ...fixture, next_link_offset: -1 },
      {
        ...fixture,
        revisions: [
          {
            ...revision,
            body: { ...revision?.body, details: { kind: "fact", state: "accepted" } },
          },
        ],
      },
      { ...fixture, revisions: [{ ...revision, evidence: [{ quote: "missing identity" }] }] },
      { ...fixture, links: [{ source_id: "a", target_id: "b", relation: "guessed" }] },
    ])
      expect(() => parseRecordPage({ records: [value], next_offset: null })).toThrow(
        "invalid record data",
      );
    expect(() =>
      parseRecordPage({ records: [], next_offset: null, next_cursor: { node_id: 123, offset: 0 } }),
    ).toThrow();
  });
});
