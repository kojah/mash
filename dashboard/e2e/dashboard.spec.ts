import { expect, test } from "@playwright/test";

const chat = {
  chat_id: "same-id",
  provider: "codex",
  title: "Real fixture chat",
  project: "/work/project",
  updated_at: Date.now(),
};
test("real API shapes, explicit search, source identity, safe content and stable refresh", async ({
  page,
}) => {
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
  await expect(page.getByRole("link", { name: /Refreshed catalogue/ })).toBeVisible();
  await page.getByPlaceholder("Search chats").click();
  const input = page.getByRole("dialog").getByRole("searchbox");
  await input.fill("needle");
  expect(searchRequests).toBe(0);
  await input.press("Enter");
  await expect(page.getByText("matching snippet")).toBeVisible();
  expect(searchRequests).toBe(1);
  await input.fill("unsubmitted draft");
  await page.clock.runFor(31_000);
  expect(searchRequests).toBe(1);
  await page
    .getByRole("dialog")
    .getByRole("link", { name: /Real fixture chat/ })
    .click();
  await expect(page.getByText("Initial", { exact: false }).last()).toBeVisible();
  expect(chatRequests.at(-1)?.searchParams.get("shared")).toBe("remote");
  expect(await page.locator("main script, main img, main a[href^='javascript:']").count()).toBe(0);
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
