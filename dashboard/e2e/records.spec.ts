import { expect, type Page, test } from "@playwright/test";
import { recordFixture } from "./record-fixtures";
import { followRail } from "./shell";

const kindNav = (page: Page) => page.getByRole("navigation", { name: "Memory kind" });
const records = (page: Page) => page.getByRole("main").getByRole("article");

test("memory categories work from the rail even when chat history is unavailable", async ({
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
  await expect(page.getByRole("alert")).toContainText("Couldn’t load chats");
  await expect(page.getByText(/Nothing saved yet/)).toBeVisible();
  for (const [label, kind] of [
    ["decisions", "decision"],
    ["preferences", "preference"],
    ["facts", "fact"],
  ] as const) {
    await followRail(page, label, "memory");
    await expect(page).toHaveURL(new RegExp(`kind=${kind}`));
    await expect(kindNav(page).getByRole("link", { name: label, exact: true })).toHaveAttribute(
      "aria-current",
      "page",
    );
    await expect(page.getByText(/No memory matches these filters/)).toBeVisible();
    await expect(page.getByRole("alert")).toHaveCount(0);
    await page.getByRole("link", { name: "mash", exact: true }).click();
    await expect(page.getByRole("heading", { name: "recent memories" })).toBeVisible();
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
    links: [{ source_id: fact.id, target_id: "decision-1", relation: "supports" }],
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
          ];
      return route.fulfill({ json: { records, next_offset: null } });
    }
    if (url.pathname === "/api/record") {
      if (url.searchParams.get("id") === "decision-1")
        return route.fulfill({ json: recordFixture("decision-1", "decision") });
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
  await followRail(page, "memory");
  await expect(page.getByRole("heading", { name: "Memory", exact: true })).toBeVisible();
  await expect(records(page)).toHaveCount(3);
  await kindNav(page).getByRole("link", { name: "facts", exact: true }).click();
  await expect(records(page)).toHaveCount(1);
  const apply = page.getByRole("button", { name: "Apply filters" });
  await expect(apply).toHaveCount(0);
  const beforeDraft = requests.length;
  await page.getByLabel("Search statements").fill("port");
  expect(requests).toHaveLength(beforeDraft);
  await apply.click();
  await expect(page).toHaveURL(/q=port/);
  await expect(records(page)).toHaveCount(1);
  expect(requests).toHaveLength(beforeDraft + 1);
  expect(requests.at(-1)?.searchParams.get("kind")).toBe("fact");
  expect(requests.at(-1)?.searchParams.get("q")).toBe("port");
  const factLink = page.getByRole("main").getByRole("link", { name: /fact fixture on port/ });
  await factLink.click();
  await expect(page.getByRole("heading", { name: statement, exact: true })).toBeVisible();
  expect(await page.evaluate(() => "recordPwned" in window)).toBe(false);
  for (const name of ["evidence", "revision history", "related records"])
    await expect(page.getByRole("heading", { name, exact: true })).toBeVisible();
  await expect(page.getByText(/^Revision 9 \(current\)/)).toBeVisible();
  await page.getByRole("button", { name: "Older revisions" }).click();
  await expect(page.getByText(/^Revision 8:/)).toBeVisible();
  await expect(page.getByRole("heading", { name: statement, exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Newest revisions" }).click();
  await expect(page.getByText(/^Revision 9 \(current\)/)).toBeVisible();
  const related = page.getByRole("link", { name: /this record supports.*decision-1/ });
  await expect(related).toBeVisible();
  await page.getByRole("button", { name: "More relationships" }).click();
  // The second page of relationships is empty; the way back stays offered.
  await expect(page.getByRole("button", { name: "First relationships" })).toBeVisible();
  await expect(related).toHaveCount(0);
  await page.getByRole("button", { name: "First relationships" }).click();
  await related.click();
  await expect(page.getByText("Keep the old port", { exact: true })).toBeVisible();
  await page.getByRole("link", { name: "Back to memory" }).click();
  await expect(kindNav(page).getByRole("link", { name: "facts", exact: true })).toHaveAttribute(
    "aria-current",
    "page",
  );
  await expect(page.getByLabel("Search statements")).toHaveValue("port");
  await factLink.click();
  await page.getByRole("link", { name: /open the message/ }).click();
  await expect(
    page.locator("#message-message-1").getByText("The service uses port 8080.", { exact: true }),
  ).toBeVisible();
  await page.getByRole("link", { name: "memory from this chat" }).click();
  await expect(page.getByText(/Memory citing chat/)).toContainText("chat-1");
  expect(documents).toBe(1);
  expect(errors).toEqual([]);
});

for (const sourceNode of ["remote/one", "local"]) {
  test(`shared records on ${sourceNode} continue empty pages, retain node identity, and never refresh unasked`, async ({
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
    const shared = records(page).getByRole("link", { name: "Shared fact" });
    await expect(shared).toBeVisible();
    expect(requests.at(-1)?.searchParams.get("shared")).toBe("true");
    expect(requests.at(-1)?.searchParams.get("cursor_node")).toBe(sourceNode);
    title = "Changed remotely";
    const before = calls;
    await page.clock.fastForward(31_000);
    expect(calls).toBe(before);
    await expect(shared).toBeVisible();
    await expect(page.getByText("Changed remotely")).toHaveCount(0);
    await shared.click();
    expect(new URLSearchParams(new URL(page.url()).hash.split("?")[1]).get("node")).toBe(
      sourceNode,
    );
    await page.getByRole("link", { name: /open the message/ }).click();
    await expect(
      page.locator("#message-message-1").getByText("Shared source reply", { exact: true }),
    ).toBeVisible();
    await page.getByRole("button", { name: "Export", exact: true }).click();
    const download = page.waitForEvent("download");
    await page.getByRole("button", { name: "Export to Markdown" }).click();
    expect((await download).suggestedFilename()).toContain(".md");
    await expect(page.getByRole("status").filter({ hasText: "Exported" })).toContainText(
      "Exported 1 messages",
    );
    await page.getByRole("link", { name: "memory from this chat" }).click();
    await expect(page.getByText(/No matching records on this page/)).toBeVisible();
    expect(requests.at(-1)?.searchParams.get("node")).toBe(sourceNode);
    expect(requests.at(-1)?.searchParams.get("shared")).toBe("true");
  });
}

test("unavailable or forgotten records show an error instead of a cached conclusion", async ({
  page,
}) => {
  let forgotten = false;
  await page.route("**/api/chats**", (route) =>
    route.fulfill({ json: { local_node: "local", truncated: false, chats: [] } }),
  );
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
  await page.getByRole("button", { name: "check for updates" }).click();
  await expect(page.getByRole("alert")).toContainText("forgotten");
  await expect(
    page.getByRole("heading", { name: "fact fixture on port 8080", exact: true }),
  ).toHaveCount(0);
});

// The earlier Memory page had a "Refresh memory" button. The list is now read
// once per address and cached for the life of the document (stableRead), and
// RecordsView.svelte offers no control that refetches it, so a reader who
// knows an agent has just saved something must reload the browser page to
// see it. The same cache also serves the overview's "recent memories".
test("the memory list can be refreshed on request without reloading", async ({ page }) => {
  let statement = "First statement";
  let documents = 0;
  page.on("request", (request) => {
    if (request.resourceType() === "document") documents++;
  });
  await page.route("**/api/chats**", (route) =>
    route.fulfill({ json: { local_node: "local", truncated: false, chats: [] } }),
  );
  await page.route("**/api/records?**", (route) => {
    const fixture = recordFixture();
    return route.fulfill({
      json: {
        records: [
          {
            ...fixture,
            revisions: [
              { ...fixture.revisions[0], body: { ...fixture.revisions[0]?.body, statement } },
            ],
          },
        ],
        next_offset: null,
      },
    });
  });
  await page.goto("/#/records?");
  await expect(records(page).getByRole("link", { name: "First statement" })).toBeVisible();
  statement = "Second statement";
  await page.getByRole("button", { name: /refresh|check for updates/i }).click();
  await expect(records(page).getByRole("link", { name: "Second statement" })).toBeVisible();
  expect(documents).toBe(1);
});
