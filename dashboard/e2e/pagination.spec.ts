import { expect, test } from "@playwright/test";

test("recent conversations page through the catalogue and Back keeps the page", async ({
  page,
}) => {
  const now = Date.now();
  const chats = Array.from({ length: 120 }, (_, index) => ({
    chat_id: `chat-${index}`,
    node_id: "local",
    provider: "codex",
    title: `Conversation ${index}`,
    project: "/work/project",
    updated_at: now - index * 60_000,
  }));
  await page.route("**/api/records?**", (route) =>
    route.fulfill({ json: { records: [], next_offset: null } }),
  );
  await page.route("**/api/chats**", (route) =>
    route.fulfill({ json: { local_node: "local", truncated: false, chats } }),
  );
  await page.route("**/api/chat?**", (route) => route.fulfill({ json: { messages: [] } }));

  await page.goto("/#/recent");
  await expect(page.locator(".chat-row")).toHaveCount(50);
  await expect(page.getByText("1–50 of 120")).toBeVisible();
  await expect(page.getByRole("link", { name: "Previous" })).toHaveCount(0);

  await page.getByRole("link", { name: "Next" }).click();
  await expect(page.getByText("51–100 of 120")).toBeVisible();
  await expect(page.getByRole("link", { name: /Conversation 50\b/ })).toBeVisible();

  await page.getByRole("link", { name: /Conversation 60\b/ }).click();
  await expect(page).toHaveURL(/#\/chat\//);
  await page.goBack();
  await expect(page.getByText("51–100 of 120")).toBeVisible();

  await page.goto("/#/recent?page=99");
  await expect(page.getByText("101–120 of 120")).toBeVisible();
  await expect(page.getByRole("link", { name: "Next" })).toHaveCount(0);
});
