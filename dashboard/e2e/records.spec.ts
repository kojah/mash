import { expect, test } from "@playwright/test";
import { recordFixture } from "./record-fixtures";

test("front page record categories work even when chat history is unavailable", async ({
  page,
}) => {
  await page.route("**/api/chats**", (route) => route.fulfill({ status: 503 }));
  await page.route("**/api/records?**", (route) =>
    route.fulfill({ json: { records: [], next_offset: null } }),
  );
  let documents = 0;
  page.on("request", (request) => {
    if (request.resourceType() === "document") documents++;
  });
  await page.goto("/");
  for (const [label, kind] of [
    ["Decisions", "decision"],
    ["Preferences", "preference"],
    ["Facts", "fact"],
    ["Questions", "open_question"],
  ] as const) {
    const section = page.getByRole("region", { name: "Records", exact: true });
    await section.getByRole("link", { name: label, exact: true }).click();
    await expect(page.getByRole("combobox", { name: "Kind", exact: true })).toHaveValue(kind);
    await expect(page.getByText(/No records match these filters/)).toBeVisible();
    await page.getByRole("link", { name: "Chats", exact: true }).click();
  }
  expect(documents).toBe(1);
});

test("records filter on submission, expose history and safe citations, and navigate without reloads", async ({
  page,
}) => {
  const errors: string[] = [],
    requests: URL[] = [];
  let documents = 0;
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("request", (request) => {
    if (request.resourceType() === "document") documents++;
  });
  const fact = recordFixture();
  const revision = fact.revisions[0];
  const statement = "Port 8080 <script>window.recordPwned=true</script>";
  const current = {
    ...fact,
    current_revision: 9,
    revisions: [{ ...revision, revision: 9, body: { ...revision?.body, statement } }],
    next_before_revision: 9,
    links: [{ source_id: fact.id, target_id: "question-1", relation: "resolves" }],
    next_link_offset: 128,
  };
  await page.route("**/api/**", (route) => {
    const url = new URL(route.request().url());
    if (url.pathname === "/api/chats")
      return route.fulfill({
        json: {
          local_node: "local",
          truncated: false,
          chats: [
            { chat_id: "chat-1", provider: "codex", title: "Source discussion", node_id: "local" },
          ],
        },
      });
    if (url.pathname === "/api/records") {
      requests.push(url);
      const records = url.searchParams.has("kind")
        ? [fact]
        : [
            fact,
            recordFixture("decision-1", "decision"),
            recordFixture("preference-1", "preference"),
            recordFixture("question-1", "open_question"),
          ];
      return route.fulfill({ json: { records, next_offset: null } });
    }
    if (url.pathname === "/api/record") {
      if (url.searchParams.get("id") === "question-1")
        return route.fulfill({ json: recordFixture("question-1", "open_question") });
      if (url.searchParams.has("before_revision"))
        return route.fulfill({
          json: {
            ...current,
            revisions: [{ ...revision, revision: 8 }],
            next_before_revision: null,
          },
        });
      if (url.searchParams.get("link_offset") === "128")
        return route.fulfill({ json: { ...current, links: [], next_link_offset: null } });
      return route.fulfill({ json: current });
    }
    return route.fulfill({
      json: {
        messages: [
          { id: "message-1", ordinal: 0, role: "user", content: "The service uses port 8080." },
        ],
      },
    });
  });
  await page.goto("/");
  await page.getByRole("link", { name: "Records", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Records", exact: true })).toBeVisible();
  await expect(page.locator("li h2")).toHaveCount(4);
  await page.getByRole("combobox", { name: "Kind", exact: true }).selectOption("fact");
  await page.getByLabel("Search statements").fill("port");
  expect(requests).toHaveLength(1);
  await page.getByRole("button", { name: "Apply filters" }).click();
  await expect(page.locator("li h2")).toHaveCount(1);
  expect(requests.at(-1)?.searchParams.get("kind")).toBe("fact");
  expect(requests.at(-1)?.searchParams.get("q")).toBe("port");
  await page.getByRole("link", { name: /fact fixture on port/ }).click();
  await expect(page.getByRole("heading", { name: statement, exact: true })).toBeVisible();
  expect(await page.evaluate(() => "recordPwned" in window)).toBe(false);
  await page.getByRole("button", { name: "Older revisions" }).click();
  await expect(page.getByRole("heading", { name: "Revision 8" })).toBeVisible();
  await expect(page.getByRole("heading", { name: statement, exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Newest revisions" }).click();
  await page.getByRole("button", { name: "More relationships" }).click();
  await expect(page.getByText("No relationships on this page.")).toBeVisible();
  await page.getByRole("button", { name: "First relationships" }).click();
  await page.getByRole("link", { name: "question-1", exact: true }).click();
  await expect(page.getByText("The platform team owns the review.", { exact: true })).toBeVisible();
  await page.getByRole("link", { name: "Back to records" }).click();
  await expect(page.getByRole("combobox", { name: "Kind", exact: true })).toHaveValue("fact");
  await page.getByRole("link", { name: /fact fixture on port/ }).click();
  await page.getByRole("link", { name: "Open source message" }).click();
  await expect(page.getByText("The service uses port 8080.", { exact: true })).toBeVisible();
  await page.getByRole("link", { name: "Chat records" }).click();
  await expect(page.getByText(/Records citing chat/)).toContainText("chat-1");
  expect(documents).toBe(1);
  expect(errors).toEqual([]);
});

for (const sourceNode of ["remote/one", "local"]) {
  test(`shared records on ${sourceNode} continue empty pages, retain node identity, and refresh only on request`, async ({
    page,
  }) => {
    let calls = 0,
      title = "Shared fact";
    const requests: URL[] = [];
    await page.route("**/api/chats**", (route) =>
      route.fulfill({ json: { local_node: "local", truncated: false, chats: [] } }),
    );
    await page.route("**/api/records?**", (route) => {
      calls++;
      const url = new URL(route.request().url());
      requests.push(url);
      if (!url.searchParams.has("cursor_node"))
        return route.fulfill({
          json: { records: [], next_offset: null, next_cursor: { node_id: sourceNode, offset: 0 } },
        });
      const fixture = recordFixture();
      return route.fulfill({
        json: {
          records: [
            {
              ...fixture,
              node_id: sourceNode,
              revisions: [
                {
                  ...fixture.revisions[0],
                  body: { ...fixture.revisions[0]?.body, statement: title },
                },
              ],
            },
          ],
          next_offset: null,
        },
      });
    });
    await page.route("**/api/record?**", (route) => {
      const params = new URL(route.request().url()).searchParams;
      expect(params.get("node")).toBe(sourceNode);
      return route.fulfill({ json: { ...recordFixture(), node_id: sourceNode } });
    });
    await page.route("**/api/view?**", (route) => {
      const params = new URL(route.request().url()).searchParams;
      expect(params.get("node")).toBe(sourceNode);
      expect(params.get("shared")).toBe("true");
      return route.fulfill({
        json: {
          messages: [{ id: "message-1", role: "user", content: "Shared source reply", ordinal: 0 }],
        },
      });
    });
    await page.route("**/api/chat?**", (route) => {
      const params = new URL(route.request().url()).searchParams;
      expect(params.get("node")).toBe(sourceNode);
      expect(params.get("shared")).toBe("true");
      return route.fulfill({
        json: {
          messages:
            params.get("offset") === "0"
              ? [{ id: "message-1", role: "user", content: "Shared source reply", ordinal: 0 }]
              : [],
        },
      });
    });
    await page.clock.install();
    await page.goto("/#/records?shared=true");
    await expect(page.getByText(/No matching records on this page/)).toBeVisible();
    await page.getByRole("button", { name: "Next page" }).click();
    await expect(page.getByRole("heading", { name: "Shared fact" })).toBeVisible();
    expect(requests.at(-1)?.searchParams.get("shared")).toBe("true");
    expect(requests.at(-1)?.searchParams.get("cursor_node")).toBe(sourceNode);
    title = "Changed remotely";
    const before = calls;
    await page.clock.fastForward(31_000);
    expect(calls).toBe(before);
    await expect(page.getByRole("heading", { name: "Shared fact" })).toBeVisible();
    await page.getByRole("button", { name: "Refresh records" }).click();
    await expect(page.getByRole("heading", { name: "Changed remotely" })).toBeVisible();
    await page.getByRole("link", { name: /Changed remotely/ }).click();
    expect(new URLSearchParams(new URL(page.url()).hash.split("?")[1]).get("node")).toBe(
      sourceNode,
    );
    await page.getByRole("link", { name: "Open source message" }).click();
    await expect(page.getByText("Shared source reply", { exact: true })).toBeVisible();
    await page.getByText("Export", { exact: true }).click();
    const download = page.waitForEvent("download");
    await page.getByRole("button", { name: "Export to Markdown" }).click();
    expect((await download).suggestedFilename()).toContain(".md");
    await expect(page.getByRole("status")).toContainText("Exported 1 messages");
    await page.getByRole("link", { name: "Chat records" }).click();
    await expect(page.getByText(/No matching records on this page/)).toBeVisible();
    expect(requests.at(-1)?.searchParams.get("node")).toBe(sourceNode);
    expect(requests.at(-1)?.searchParams.get("shared")).toBe("true");
  });
}

test("unavailable or forgotten records show an error instead of a cached conclusion", async ({
  page,
}) => {
  let forgotten = false;
  await page.route("**/api/record?**", (route) =>
    forgotten
      ? route.fulfill({
          status: 400,
          json: { error: { message: "Record does not exist or was forgotten" } },
        })
      : route.fulfill({ json: recordFixture() }),
  );
  await page.goto("/#/record/fact-1");
  await expect(
    page.getByRole("heading", { name: "fact fixture on port 8080", exact: true }),
  ).toBeVisible();
  forgotten = true;
  await page.getByRole("button", { name: "Refresh record" }).click();
  await expect(page.getByRole("alert")).toContainText("forgotten");
  await expect(
    page.getByRole("heading", { name: "fact fixture on port 8080", exact: true }),
  ).toHaveCount(0);
});
