import { expect, test } from "vitest";
import { agents } from "./agents";
import * as wire from "./settings";
import { differences, draftOf, problem } from "./settings";
import { trimmed } from "./trimmed";
import type { Agent } from "./types";

const names = Object.fromEntries(
  Object.entries(agents).map(([agent, info]) => [agent, info.name]),
) as Record<Agent, string>;
const parseSettings = (answer: unknown) => wire.parseSettings(trimmed(answer));
const body = {
  path: "/c/config.toml",
  revision: "abc",
  editable: true,
  agents: {
    codex: { indexed: true, roots: ["/h/codex"] },
    cursor: { indexed: false, roots: ["/h/cursor"] },
    "not-an-agent": { indexed: true, roots: [] },
  },
  since_days: null,
  destination: "both",
  shared: true,
  memory_capture: true,
  hide_credentials: { output: true, publication: true },
};

test("unknown agents are left out of the settings", () => {
  const settings = parseSettings(body);
  expect(settings.agents.map((entry) => entry.agent)).toEqual(["codex", "cursor"]);
  expect(settings.sinceDays).toBeNull();
});

test("an untouched draft changes nothing, and each change is named in the page's words", () => {
  const settings = parseSettings(body);
  const draft = draftOf(settings);
  expect(differences(settings, draft, names)).toEqual([]);
  draft.agents.cursor = true;
  draft.sinceDays = 30;
  draft.hideWhenPublishing = false;
  expect(differences(settings, draft, names)).toEqual([
    { key: "agent:cursor", label: "index Cursor", from: "off", to: "on" },
    { key: "range", label: "history range", from: "all history", to: "the last 30 days" },
    { key: "publishing", label: "hide credentials when publishing", from: "on", to: "off" },
  ]);
});

test("a draft that could not be saved says why", () => {
  const settings = parseSettings(body);
  const draft = draftOf(settings);
  expect(problem(settings, draft)).toBe("");
  draft.agents.codex = false;
  expect(problem(settings, draft)).toContain("at least one");
  draft.agents.codex = true;
  draft.sinceDays = 0;
  expect(problem(settings, draft)).toContain("between 1 and 36,500");
});
