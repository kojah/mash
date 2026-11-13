import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  type Change,
  concerns,
  connect,
  onChange,
  pageUpdate,
  parseChange,
  retryDelay,
  throttle,
} from "./live";
import { type EventStream, installEvents } from "./transport";

class FakeStream implements EventStream {
  onopen: ((event: Event) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  closed = false;
  private readonly heard: ((event: MessageEvent<string>) => void)[] = [];
  addEventListener(_type: string, listener: (event: MessageEvent<string>) => void): void {
    this.heard.push(listener);
  }
  close(): void {
    this.closed = true;
  }
  open(): void {
    this.onopen?.(new Event("open"));
  }
  fail(): void {
    this.onerror?.(new Event("error"));
  }
  send(data: string): void {
    for (const listener of this.heard) listener({ data } as MessageEvent<string>);
  }
}

describe("parseChange", () => {
  it("reads what the server sends", () => {
    expect(
      parseChange('{"node":null,"revision":7,"chats":["a","b"],"all":false,"records":false}'),
    ).toEqual({
      node: null,
      chats: ["a", "b"],
      all: false,
      records: false,
    });
    expect(
      parseChange('{"node":"laptop","revision":7,"chats":[],"all":true,"records":true}'),
    ).toEqual({
      node: "laptop",
      chats: [],
      all: true,
      records: true,
    });
  });
  it("turns down anything else", () => {
    for (const data of [
      "",
      "{",
      "null",
      "[]",
      '{"chats":"a","all":false,"records":false}',
      '{"chats":[1],"all":false,"records":false}',
      '{"chats":[]}',
      '{"node":null,"chats":[],"all":false}',
      '{"node":7,"chats":[],"all":false,"records":false}',
    ])
      expect(parseChange(data)).toBeUndefined();
  });
});

describe("connect", () => {
  let streams: FakeStream[];
  let changes: Change[];
  let off: () => void;
  beforeEach(() => {
    vi.useFakeTimers();
    streams = [];
    changes = [];
    installEvents(() => {
      const stream = new FakeStream();
      streams.push(stream);
      return stream;
    });
    off = onChange((change) => changes.push(change));
  });
  afterEach(() => {
    off();
    vi.useRealTimers();
  });

  it("passes on valid changes and ignores malformed ones", () => {
    const stop = connect();
    streams[0]?.open();
    streams[0]?.send('{"node":null,"revision":1,"chats":["a"],"all":false,"records":false}');
    streams[0]?.send("<script>");
    expect(changes).toEqual([{ node: null, chats: ["a"], all: false, records: false }]);
    stop();
    expect(streams[0]?.closed).toBe(true);
  });

  it("reopens a dropped stream after a growing wait and then rereads everything", () => {
    const lost = vi.fn();
    const stop = connect(lost);
    streams[0]?.open();
    streams[0]?.fail();
    expect(streams[0]?.closed).toBe(true);
    expect(lost).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(999);
    expect(streams).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(streams).toHaveLength(2);
    // Still down: the second wait is twice the first, and `lost` is not repeated.
    streams[1]?.fail();
    vi.advanceTimersByTime(1999);
    expect(streams).toHaveLength(2);
    vi.advanceTimersByTime(1);
    expect(streams).toHaveLength(3);
    expect(lost).toHaveBeenCalledTimes(1);
    expect(changes).toEqual([]);
    streams[2]?.open();
    expect(changes).toEqual([{ node: null, chats: [], all: true, records: true }]);
    // Back up, so the next failure starts from the short wait again.
    streams[2]?.fail();
    vi.advanceTimersByTime(1000);
    expect(streams).toHaveLength(4);
    stop();
  });

  it("does not reopen once stopped", () => {
    const stop = connect();
    streams[0]?.fail();
    stop();
    vi.advanceTimersByTime(60_000);
    expect(streams).toHaveLength(1);
  });

  it("does nothing where there is no stream, as in the demo", () => {
    installEvents(undefined);
    const stop = connect();
    vi.advanceTimersByTime(60_000);
    stop();
    expect(changes).toEqual([]);
  });
});

it("waits 1s, doubling, at most 30s", () => {
  expect([1, 2, 3, 6, 20].map(retryDelay)).toEqual([1000, 2000, 4000, 30_000, 30_000]);
});

describe("concerns", () => {
  const local: Change = { node: null, chats: ["a"], all: false, records: false };
  const remote: Change = { node: "laptop", chats: ["a"], all: false, records: false };
  it("matches a chat with the index it is read from", () => {
    expect(concerns(local, "a", "desk", true)).toBe(true);
    expect(concerns(local, "b", "desk", true)).toBe(false);
    expect(concerns(remote, "a", "laptop", false)).toBe(true);
  });
  it("keeps the same chat ID on another machine apart", () => {
    expect(concerns(local, "a", "laptop", false)).toBe(false);
    expect(concerns(remote, "a", "desk", true)).toBe(false);
    expect(concerns(remote, "a", "server", false)).toBe(false);
  });
  it("takes everything as news", () => {
    expect(
      concerns({ node: null, chats: [], all: true, records: true }, "z", "laptop", false),
    ).toBe(true);
  });
});

describe("throttle", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());
  it("runs at once, then once more for a burst", () => {
    const run = vi.fn();
    const call = throttle(run, 5000);
    call();
    call();
    call();
    expect(run).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(5000);
    expect(run).toHaveBeenCalledTimes(2);
    vi.advanceTimersByTime(20_000);
    expect(run).toHaveBeenCalledTimes(2);
    call();
    expect(run).toHaveBeenCalledTimes(3);
    call();
    call.cancel();
    vi.advanceTimersByTime(20_000);
    expect(run).toHaveBeenCalledTimes(3);
  });
});

describe("pageUpdate", () => {
  const a = { id: "a", text: "one" };
  const b = { id: "b", text: "two" };
  const c = { id: "c", text: "three" };
  it("sees no change in the same page", () => {
    expect(pageUpdate({ messages: [a, b], total: 2 }, { messages: [a, b], total: 2 })).toEqual({
      kind: "same",
    });
  });
  it("counts messages added to this page", () => {
    expect(pageUpdate({ messages: [a], total: 1 }, { messages: [a, b, c], total: 3 })).toEqual({
      kind: "appended",
      added: 2,
      onPage: true,
    });
    expect(pageUpdate({ messages: [a] }, { messages: [a, b] })).toEqual({
      kind: "appended",
      added: 1,
      onPage: true,
    });
  });
  it("counts messages added beyond a full or earlier page", () => {
    expect(pageUpdate({ messages: [a, b], total: 200 }, { messages: [a, b], total: 203 })).toEqual({
      kind: "appended",
      added: 3,
      onPage: false,
    });
  });
  it("calls any change to shown text a rewrite", () => {
    expect(
      pageUpdate({ messages: [a, b] }, { messages: [a, { ...b, text: "changed" }, c] }),
    ).toEqual({ kind: "rewritten" });
    expect(pageUpdate({ messages: [a, b] }, { messages: [a] })).toEqual({ kind: "rewritten" });
    expect(pageUpdate({ messages: [a], total: 5 }, { messages: [a], total: 4 })).toEqual({
      kind: "rewritten",
    });
  });
});
