import { describe, expect, it } from "vitest";
import { recordFixture } from "../../e2e/record-fixtures";
import * as wire from "./records";
import { kinds, standing, timingLabel } from "./records";
import { trimmed } from "./trimmed";

const parseRecord = (row: unknown) => wire.parseRecord(trimmed(row));
const parseRecordPage = (page: unknown) => wire.parseRecordPage(trimmed(page));

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
  it("tells a choice the agent made from the person's and from a candidate", () => {
    const fixture = recordFixture("id", "decision");
    const theirs = parseRecord(fixture).revisions[0];
    if (!theirs) throw new Error("Missing fixture decision");
    expect(standing(theirs)).toBe("established");
    const revision = fixture.revisions[0];
    if (!revision) throw new Error("Missing fixture revision");
    const by = (decided_by: string) => ({
      ...revision,
      body: { ...revision.body, details: { ...revision.body.details, decided_by } },
    });
    const made = by("agent");
    const agents = parseRecord({ ...fixture, revisions: [made] }).revisions[0];
    if (!agents) throw new Error("Missing agent decision");
    expect(standing(agents)).toBe("agent decided");
    expect(standing({ ...agents, review: "candidate" })).toBe("unconfirmed");
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
});
