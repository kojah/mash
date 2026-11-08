import { expect, test } from "@playwright/test";
import { recordFixture } from "./record-fixtures";

for (const width of [1440, 390]) {
  test(`approved layout navigation, memory and reading at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    let documents = 0;
    let searches = 0;
    const errors: string[] = [];
    page.on("pageerror", (error) => errors.push(error.message));
    page.on("request", (request) => {
      if (request.resourceType() === "document") documents++;
    });
    const chat = {
      chat_id: "chat-1",
      node_id: "local",
      title: "Investigate the watcher",
      provider: "codex",
      project: "/work/mash",
      updated_at: Date.now(),
    };
    await page.route("**/api/**", (route) => {
      const path = new URL(route.request().url()).pathname;
      if (path === "/api/chats")
        return route.fulfill({ json: { local_node: "local", chats: [chat], truncated: false } });
      if (path === "/api/records")
        return route.fulfill({ json: { records: [recordFixture()], next_offset: null } });
      if (path === "/api/search") {
        searches++;
        return route.fulfill({
          json: {
            results: [
              { ...chat, matches: [{ message_id: "message-1", snippet: "watcher result" }] },
            ],
            next_offset: null,
          },
        });
      }
      return route.fulfill({
        json: {
          messages: [
            {
              id: "message-1",
              ordinal: 0,
              role: "user",
              content: `Investigate the watcher\n\n${"More context.\n\n".repeat(80)}`,
            },
          ],
        },
      });
    });
    await page.goto("/");
    await expect(page.locator(".chat-row")).toBeVisible();
    await expect(page.getByRole("heading", { name: "Machines" })).toHaveCount(0);
    expect(
      await page
        .locator(".chat-row img")
        .evaluate((img: HTMLImageElement) => img.complete && img.naturalWidth > 0),
    ).toBe(true);
    const open = page.getByRole("button", { name: "Open navigation" });
    await open.click();
    await expect(page.getByRole("dialog")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(open).toBeFocused();
    await open.click();
    await page.getByRole("dialog").getByRole("link", { name: "Machines", exact: true }).click();
    await expect(page.getByRole("dialog")).not.toBeVisible();
    await page.locator(".browse-list").getByRole("link", { name: "local" }).click();
    await expect(page.getByRole("region", { name: "Search results" })).toBeVisible();
    await page.getByRole("searchbox").fill("watcher");
    await page.getByRole("searchbox").press("Enter");
    await page.getByRole("region", { name: "Search results" }).getByRole("link").click();
    await expect(page.locator("main .prose")).toContainText("Investigate the watcher");
    if (width < 700) {
      await page.getByRole("button", { name: "Outline", exact: true }).click();
      await expect(page.getByRole("complementary", { name: "Conversation outline" })).toBeVisible();
      await page.getByRole("button", { name: "Close", exact: true }).click();
    }
    await page.locator(".transcript-pane").evaluate((element) => {
      element.scrollTop = 200;
    });
    await page.goBack();
    await expect(page.getByRole("searchbox")).toHaveValue("watcher");
    expect(searches).toBe(1);
    await open.click();
    await page.getByRole("dialog").getByRole("link", { name: "Continue reading" }).click();
    await expect(page.locator(".chat-row")).toBeVisible();
    await open.click();
    await page.getByRole("dialog").getByRole("link", { name: "Facts", exact: true }).click();
    await expect(page.locator(".memory-tabs [aria-current]")).toHaveText("Facts");
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(
      true,
    );
    expect(documents).toBe(1);
    expect(errors).toEqual([]);
  });
}
