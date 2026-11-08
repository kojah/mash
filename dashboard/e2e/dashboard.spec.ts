import { expect, test } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.route("**/api/records?**", (route) =>
    route.fulfill({ json: { records: [], next_offset: null } }),
  );
});

const chat = {
  chat_id: "same-id",
  provider: "codex",
  title: "Real fixture chat",
  project: "/work/project",
  updated_at: Date.now(),
};
test("direct chats load before the catalogue and tool output stays literal", async ({ page }) => {
  let release: (() => void) | undefined;
  const catalogReady = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route("**/api/chats**", async (route) => {
    await catalogReady;
    await route.fulfill({
      json: { local_node: "local", truncated: false, chats: [{ ...chat, node_id: "local" }] },
    });
  });
  await page.route("**/api/chat?**", (route) =>
    route.fulfill({
      json: {
        messages: [
          {
            id: "reply",
            ordinal: 0,
            role: "assistant",
            content:
              '## A readable heading\n\n- First\n- Second\n\n```rust\nfn main() {\n    println!("hello");\n}\n```',
          },
          {
            id: "tool",
            ordinal: 1,
            role: "user",
            content: "# README\n\n<div>literal HTML</div>",
            tools: { tool_results: [{ call_id: "call" }] },
          },
        ],
      },
    }),
  );
  await page.goto("/#/chat/same-id");
  try {
    await expect(page.getByRole("heading", { name: "A readable heading" })).toBeVisible();
    await expect(page.locator("main pre code")).toContainText('    println!("hello");');
    await expect(page.getByRole("heading", { name: "README" })).toHaveCount(0);
    await page.getByText("Tool output", { exact: true }).click();
    await expect(page.locator("details pre")).toContainText("<div>literal HTML</div>");
  } finally {
    release?.();
  }
  await expect(page.getByRole("heading", { name: chat.title })).toBeVisible();
});
test("direct search matches load without waiting for catalogue identity", async ({ page }) => {
  let release: (() => void) | undefined;
  const ready = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route("**/api/chats**", async (route) => {
    await ready;
    await route.fulfill({ json: { local_node: "workstation", truncated: false, chats: [] } });
  });
  await page.route("**/api/view?**", (route) => {
    const params = new URL(route.request().url()).searchParams;
    expect(params.get("node")).toBe("workstation");
    expect(params.get("id")).toBe("anchor");
    return route.fulfill({
      json: {
        messages: [{ id: "anchor", ordinal: 42, role: "assistant", content: "Selected message" }],
      },
    });
  });
  await page.goto("/#/chat/same-id?node=workstation&message=anchor");
  try {
    await expect(page.getByText("Selected message", { exact: true })).toBeVisible();
  } finally {
    release?.();
  }
});

test("real API shapes, explicit search, source identity, safe content and stable refresh", async ({
  page,
}) => {
  // Network HTTP previews do not expose this secure-context-only API.
  await page.addInitScript(() => {
    Object.defineProperty(crypto, "randomUUID", { value: undefined });
  });
  let title = chat.title;
  let content =
    "Initial **reply**\n\n<script>window.pwned = true</script><img src=https://evil.example/pixel onerror=alert(1)>[bad](javascript:alert(1))";
  let searchRequests = 0;
  const chatRequests: URL[] = [];
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.route("**/api/**", async (route) => {
    const url = new URL(route.request().url());
    if (url.pathname === "/api/chats") {
      await route.fulfill({
        json: {
          local_node: "local",
          truncated: false,
          shared_error: null,
          chats: [
            { ...chat, title, node_id: "local" },
            { ...chat, node_id: "remote" },
          ],
        },
      });
    } else if (url.pathname === "/api/search") {
      searchRequests++;
      await route.fulfill({
        json: {
          results: [
            {
              ...chat,
              node_id: "remote",
              matches: [{ message_id: "match-id", snippet: "matching snippet" }],
            },
          ],
          next_offset: null,
        },
      });
    } else if (url.pathname === "/api/records") {
      await route.fulfill({ json: { records: [], next_offset: null } });
    } else {
      chatRequests.push(url);
      await route.fulfill({
        json: {
          id: "same-id",
          messages: [
            { id: "match-id", ordinal: 0, role: "assistant", created_at: Date.now(), content },
          ],
        },
      });
    }
  });
  await page.clock.install();
  await page.goto("/");
  await expect(page.getByRole("link", { name: /Real fixture chat/ })).toHaveCount(2);
  title = "Refreshed catalogue";
  await page.clock.runFor(31_000);
  await expect(page.getByRole("link", { name: /Refreshed catalogue/ })).toHaveCount(0);
  await expect(page.getByRole("button", { name: /New or updated chats/ })).toHaveCount(0);
  await page.getByRole("button", { name: "Check for updates" }).click();
  await page.getByRole("button", { name: /New or updated chats/ }).click();
  await expect(page.getByRole("link", { name: /Refreshed catalogue/ })).toBeVisible();
  const homeInput = page.getByRole("searchbox");
  await homeInput.fill("needle");
  expect(searchRequests).toBe(0);
  await homeInput.press("Enter");
  const input = page.getByRole("searchbox");
  await expect(page.getByText("matching snippet")).toBeVisible();
  expect(searchRequests).toBe(1);
  await input.fill("unsubmitted draft");
  await page.clock.runFor(31_000);
  expect(searchRequests).toBe(1);
  await page
    .getByRole("main")
    .getByRole("link", { name: /Real fixture chat/ })
    .click();
  await expect(page.getByText("Initial", { exact: false }).last()).toBeVisible();
  expect(chatRequests.at(-1)?.searchParams.get("node")).toBe("remote");
  expect(
    await page.locator(".prose script, .prose img, .prose a[href^='javascript:']").count(),
  ).toBe(0);
  await expect(page.locator("main strong")).toHaveText("reply");
  content = "Updated reply";
  await page.getByRole("button", { name: "Check for updates" }).click();
  await expect(page.getByRole("button", { name: /This page has changed/ })).toBeVisible();
  await expect(page.getByText("Updated reply", { exact: true })).toHaveCount(0);
  await page.getByRole("button", { name: /This page has changed/ }).click();
  await expect(page.getByText("Updated reply", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Read from the beginning" }).click();
  await expect.poll(() => chatRequests.at(-1)?.pathname).toBe("/api/chat");
  expect(chatRequests.at(-1)?.searchParams.get("node")).toBe("remote");
  await page.getByRole("button", { name: "Next messages" }).click();
  await expect.poll(() => chatRequests.at(-1)?.searchParams.get("offset")).toBe("100");
  expect(errors).toEqual([]);
});

test("chat navigation retains the sidebar and refresh requires an explicit request", async ({
  page,
}) => {
  let requests = 0;
  let documents = 0;
  page.on("request", (request) => {
    if (request.url().includes("/api/")) requests++;
    if (request.resourceType() === "document") documents++;
  });
  let content = "Original answer";
  await page.route("**/api/chats**", (route) =>
    route.fulfill({
      json: {
        local_node: "local",
        truncated: false,
        chats: Array.from({ length: 80 }, (_, index) => ({
          ...chat,
          chat_id: `chat-${index}`,
          title: `Chat ${index}`,
          node_id: "local",
        })),
      },
    }),
  );
  await page.route("**/api/chat?**", (route) =>
    route.fulfill({
      json: { messages: [{ id: "answer", ordinal: 0, role: "assistant", content }] },
    }),
  );
  await page.clock.install();
  await page.goto("/#/chat/chat-0?node=local");
  await expect(page.getByText("Original answer", { exact: true })).toBeVisible();
  const sidebar = page.getByRole("complementary", { name: "Conversation outline" });
  await sidebar.locator("summary").click();
  await sidebar.evaluate((element) => {
    element.scrollTop = 500;
    element.setAttribute("data-retained", "yes");
  });
  await page.evaluate(() => {
    location.hash = "#/chat/chat-1?node=local";
  });
  await expect(page.getByRole("heading", { name: "Chat 1", exact: true })).toBeVisible();
  await expect(sidebar).toHaveAttribute("data-retained", "yes");
  expect(await sidebar.evaluate((element) => element.scrollTop)).toBe(500);
  content = "Background replacement";
  await page.waitForLoadState("networkidle");
  const beforeIdle = requests;
  await page.clock.runFor(91_000);
  expect(requests).toBe(beforeIdle);
  expect(documents).toBe(1);
  await expect(page.getByRole("button", { name: /This page has changed/ })).toHaveCount(0);
  await page.getByRole("button", { name: "Check for updates" }).click();
  await expect(page.getByRole("button", { name: /This page has changed/ })).toBeVisible();
  await expect(page.getByText("Original answer", { exact: true })).toBeVisible();
  await expect(page.getByText("Background replacement", { exact: true })).toHaveCount(0);
  expect(await sidebar.evaluate((element) => element.scrollTop)).toBe(500);
});

test("switching a scrolled chat keeps the header and sidebar stationary", async ({ page }) => {
  let documents = 0;
  page.on("request", (request) => {
    if (request.isNavigationRequest() && request.frame() === page.mainFrame()) documents++;
  });
  let release: (() => void) | undefined;
  const ready = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route("**/api/chats**", (route) =>
    route.fulfill({
      json: {
        local_node: "local",
        truncated: false,
        chats: Array.from({ length: 50 }, (_, i) => ({
          ...chat,
          chat_id: `chat-${i}`,
          title: `Chat ${i}`,
          node_id: "local",
        })),
      },
    }),
  );
  await page.route("**/api/chat?**", async (route) => {
    const id = new URL(route.request().url()).searchParams.get("id");
    if (id === "chat-8") await ready;
    await route.fulfill({
      json: {
        messages: [
          {
            id: `message-${id}`,
            ordinal: 0,
            role: "assistant",
            content: "Long paragraph.\n\n".repeat(120),
          },
        ],
      },
    });
  });
  await page.goto("/#/chat/chat-0?node=local");
  await page.locator("main ol li").first().waitFor();
  const header = page.getByRole("link", { name: "Search chats", exact: true });
  const initialHeader = await header.boundingBox();
  const sidebar = page.getByRole("complementary", { name: "Conversation outline" });
  await sidebar.locator("summary").click();
  await page.locator("main").evaluate((element) => {
    element.scrollTop = 600;
  });
  expect(await page.locator("main").evaluate((element) => element.scrollTop)).toBe(600);
  await sidebar.evaluate((element) => {
    element.scrollTop = 400;
  });
  const sidebarScroll = await sidebar.evaluate((element) => element.scrollTop);
  try {
    await sidebar.getByRole("link", { name: "Chat 8", exact: true }).click();
    await expect(page.getByText("Loading messages…", { exact: true })).toBeVisible();
    expect(await header.boundingBox()).toEqual(initialHeader);
    expect(await sidebar.evaluate((element) => element.scrollTop)).toBe(sidebarScroll);
    expect(await page.evaluate(() => window.scrollY)).toBe(0);
    expect(await page.locator("main").evaluate((element) => element.scrollTop)).toBe(0);
    expect(documents).toBe(1);
  } finally {
    release?.();
  }
  await page.locator("main ol li").first().waitFor();
  expect(await header.boundingBox()).toEqual(initialHeader);
});

test("Export downloads the entire chat as safe HTML and Markdown", async ({ page }) => {
  await page.route("**/api/chats**", (route) =>
    route.fulfill({
      json: { local_node: "local", truncated: false, chats: [{ ...chat, node_id: "local" }] },
    }),
  );
  await page.route("**/api/chat?**", (route) => {
    const offset = Number(new URL(route.request().url()).searchParams.get("offset"));
    return route.fulfill({
      json: {
        messages:
          offset >= 200
            ? []
            : [
                {
                  id: `message-${offset}`,
                  ordinal: offset,
                  role: "assistant",
                  content:
                    offset === 0
                      ? "First **message** <script>window.pwned=true</script><img src=https://evil.example/pixel>"
                      : "Last message on another page",
                },
              ],
      },
    });
  });
  await page.goto("/#/chat/same-id?node=local");
  await expect(page.getByRole("heading", { name: chat.title })).toBeVisible();
  await page
    .locator("summary")
    .filter({ hasText: /^Export$/ })
    .click();
  for (const format of ["Markdown", "HTML"]) {
    const downloaded = page.waitForEvent("download");
    await page.getByRole("button", { name: `Export to ${format}`, exact: true }).click();
    const download = await downloaded;
    const stream = await download.createReadStream();
    if (!stream) throw new Error("Missing export download");
    let content = "";
    for await (const chunk of stream) content += chunk.toString();
    expect(content).toContain("Last message on another page");
    if (format === "HTML") {
      expect(content).toContain("<strong>message</strong>");
      expect(content).not.toContain("<script>");
      expect(content).not.toContain("<img");
    }
  }
  await expect(page.getByRole("button", { name: "Export to PDF", exact: true })).toBeEnabled();
  await page.context().addInitScript(() => {
    window.print = () => {};
  });
  const opened = page.waitForEvent("popup");
  await page.getByRole("button", { name: "Export to PDF", exact: true }).click();
  const preview = await opened;
  await expect(preview.getByText("Last message on another page", { exact: true })).toBeVisible();
  await expect(preview.locator("script, img")).toHaveCount(0);
  const pdf = await preview.pdf();
  expect(pdf.subarray(0, 5).toString()).toBe("%PDF-");
  await preview.close();
});

test("network errors are visible and recoverable", async ({ page }) => {
  await page.route("**/api/chats**", (route) =>
    route.fulfill({ status: 503, body: "unavailable" }),
  );
  await page.goto("/");
  await expect(page.getByRole("alert")).toContainText("503");
  await page.route("**/api/chats**", (route) =>
    route.fulfill({
      json: { local_node: "local", truncated: false, shared_error: "remote offline", chats: [] },
    }),
  );
  await page.getByRole("button", { name: "Retry", exact: true }).click();
  await expect(page.getByText("No indexed chats yet.", { exact: false })).toBeVisible();
  await expect(page.getByRole("status")).toContainText("remote offline");
});
