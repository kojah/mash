import { expect, test } from "vitest";
import { agents } from "./agents";
import { differences, draftOf, parseSettings, problem } from "./settings";
import type { Agent } from "./types";

const names = Object.fromEntries(
  Object.entries(agents).map(([agent, info]) => [agent, info.name]),
) as Record<Agent, string>;
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

test("settings are read strictly and unknown agents are left out", () => {
  const settings = parseSettings(body);
  expect(settings.agents.map((entry) => entry.agent)).toEqual(["codex", "cursor"]);
  expect(settings.sinceDays).toBeNull();
  expect(() => parseSettings({ ...body, destination: "elsewhere" })).toThrow("destination");
  expect(() => parseSettings({ ...body, since_days: 0 })).toThrow("since_days");
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
