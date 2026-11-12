import { expect, test } from "vitest";
import { durationLabel, toolCalls, toolOutput } from "./toolcalls";
import type { Message } from "./types";

function step(
  id: string,
  presentation: "tool-call" | "tool-result",
  content: string,
  tools: Message["tools"],
): Message {
  return {
    id,
    role: presentation === "tool-call" ? "assistant" : "user",
    presentation,
    at: "10:22",
    blocks: [{ type: "text", markdown: content }],
    activity: JSON.stringify(tools),
    ...(tools ? { tools } : {}),
  };
}

test("a call and its output on separate messages become one call", () => {
  const calls = toolCalls([
    step("a", "tool-call", 'Tool Bash: {"command":"ls"}', {
      calls: [
        {
          name: "Bash",
          callId: "t1",
          input: { command: "ls -la\npwd", description: "List files" },
        },
      ],
      results: [],
    }),
    step("b", "tool-result", "README.md\nsrc", {
      calls: [],
      results: [{ callId: "t1", isError: false }],
    }),
  ]);
  expect(calls).toHaveLength(1);
  expect(calls[0]).toMatchObject({
    id: "a",
    resultId: "b",
    name: "Bash",
    summary: "List files",
    args: [{ key: "command", text: "ls -la\npwd", block: true }],
    output: { text: "README.md\nsrc" },
    status: { label: "finished", failed: false },
  });
});

test("a failed output marks its call, and an output with no call stands alone", () => {
  const calls = toolCalls([
    step("a", "tool-call", "Tool Edit: {}", {
      calls: [{ name: "Edit", callId: "t1", input: { file_path: "a.rs" } }],
      results: [],
    }),
    step("b", "tool-result", "File has not been read yet.", {
      calls: [],
      results: [{ callId: "t1", isError: true }],
    }),
    step("c", "tool-result", "orphan", {
      calls: [],
      results: [{ callId: "t9", isError: undefined }],
    }),
  ]);
  expect(calls[0]?.status).toEqual({ label: "failed", failed: true });
  expect(calls[0]?.summary).toBe("a.rs");
  expect(calls[1]).toMatchObject({ id: "c", name: "Tool output", output: { text: "orphan" } });
  expect(calls[1]?.status).toBeUndefined();
});

test("a record that holds the whole call is read by its structure", () => {
  const record = {
    type: "mcpToolCall",
    tool: "js",
    arguments: { code: "await tab.getAXState();", title: "Refresh browser state" },
    durationMs: 379,
    error: null,
    result: { content: [{ type: "text", text: "tab: 2\n\t1 area" }] },
    server: "cua_repl",
    status: "completed",
  };
  const [call] = toolCalls([
    step("a", "tool-call", JSON.stringify(record), {
      calls: [{ name: "js", callId: "c1", input: record.arguments }],
      results: [],
    }),
  ]);
  expect(call).toMatchObject({
    name: "js",
    summary: "Refresh browser state",
    args: [{ key: "code", text: "await tab.getAXState();", block: false }],
    output: { text: "tab: 2\n\t1 area" },
    status: { label: "completed", failed: false },
    durationMs: 379,
    server: "cua_repl",
  });
});

test("a command record reports its exit code and output", () => {
  const [call] = toolCalls([
    step(
      "a",
      "tool-call",
      JSON.stringify({
        type: "commandExecution",
        command: "/bin/bash -lc 'cat missing.md'",
        cwd: "/work",
        aggregatedOutput: "cat: missing.md: No such file\n",
        exitCode: 1,
        status: "failed",
        durationMs: 0,
      }),
      { calls: [{ name: undefined, callId: "e1", input: undefined }], results: [] },
    ),
  ]);
  expect(call).toMatchObject({
    name: "command execution",
    summary: "/bin/bash -lc 'cat missing.md'",
    status: { label: "exit 1", failed: true },
    output: { text: "cat: missing.md: No such file\n" },
  });
  expect(call?.args.map((argument) => argument.key)).toEqual(["command", "cwd"]);
});

test("output that is JSON is parsed for display, and other text is left alone", () => {
  expect(toolOutput('{"a": 1}')).toEqual({ text: '{"a": 1}', json: { a: 1 } });
  expect(toolOutput("{not json")).toEqual({ text: "{not json" });
  expect(toolOutput("")).toBeUndefined();
});

test("durations read in natural units", () => {
  expect(durationLabel(379)).toBe("379 ms");
  expect(durationLabel(2400)).toBe("2.4 s");
  expect(durationLabel(125_000)).toBe("2 min 5 s");
});
