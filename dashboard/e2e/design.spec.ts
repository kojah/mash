import { expect, type Page, test } from "@playwright/test";
import { recordFixture } from "./record-fixtures";
import { fitsViewport, followRail, rail, searchBox } from "./shell";

const chat = {
  chat_id: "chat-1",
  node_id: "local",
  title: "Investigate the watcher",
  provider: "codex",
  project: "/work/mash",
  updated_at: Date.now(),
};

async function mockHistory(page: Page, searched: () => void): Promise<void> {
  await page.route("**/api/**", (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/api/chats")
      return route.fulfill({ json: { local_node: "local", chats: [chat], truncated: false } });
    if (path === "/api/records")
      return route.fulfill({ json: { records: [recordFixture()], next_offset: null } });
    if (path === "/api/services/access")
      return route.fulfill({ json: { authentication_required: false, local_node: "local" } });
    if (path === "/api/services") return route.fulfill({ json: { machines: [] } });
    if (path === "/api/search") {
      searched();
      return route.fulfill({
        json: {
          results: [{ ...chat, matches: [{ message_id: "message-1", snippet: "watcher result" }] }],
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
            content: `Investigate the watcher\n\n**Agreed** <plan> first\nthen deploy\n\n${"More context.\n\n".repeat(80)}`,
          },
        ],
      },
    });
  });
}

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
    await mockHistory(page, () => searches++);
    await page.goto("/");
    const main = page.getByRole("main");
    const recentChat = main.getByRole("link", { name: /Investigate the watcher/ });
    await expect(recentChat).toBeVisible();
    for (const name of ["recent chats", "recent memories", "machines", "agents"])
      await expect(main.getByRole("heading", { name, exact: true })).toBeVisible();
    await expect(main.getByRole("link", { name: /fact fixture on port 8080/ })).toBeVisible();
    const section = (name: string) =>
      main.locator("section").filter({ has: page.getByRole("heading", { name, exact: true }) });
    await section("agents").getByRole("link", { name: /Codex/ }).click();
    await expect(page).toHaveURL(/#\/agent\/codex$/);
    await expect(page.getByRole("heading", { name: "Codex", exact: true })).toBeVisible();
    await page.goBack();
    await section("machines").getByRole("link", { name: /local/ }).click();
    await expect(page).toHaveURL(/#\/machine\/local$/);
    await expect(page.getByRole("heading", { name: "local", exact: true })).toBeVisible();
    await page.goBack();
    // The agent's mark is its logo used as a mask over the agent's colour:
    // the logo must load and the colour must not be transparent.
    const mark = await recentChat.locator(".mark").evaluate(async (element) => {
      const style = getComputedStyle(element);
      const source = /url\("?([^")]+)"?\)/.exec(style.maskImage || style.webkitMaskImage)?.[1];
      const logo = source ? await fetch(source) : null;
      return { loaded: logo?.ok === true, colour: style.backgroundColor };
    });
    expect(mark.loaded).toBe(true);
    expect(mark.colour).not.toBe("rgba(0, 0, 0, 0)");
    const open = page.getByRole("button", { name: "Open navigation" });
    if (width < 800) {
      // The rail is a drawer on a narrow screen: hidden until asked for.
      await expect(rail(page)).toBeHidden();
      await open.click();
      await expect(rail(page)).toBeVisible();
      await expect(open).toHaveAttribute("aria-expanded", "true");
      await page.getByRole("button", { name: "Close navigation" }).first().click();
      await expect(rail(page)).toBeHidden();
    } else {
      await expect(open).toBeHidden();
      await expect(rail(page)).toBeVisible();
    }
    await followRail(page, "machines");
    if (width < 800) await expect(rail(page)).toBeHidden();
    await expect(page.getByRole("heading", { name: "Machines", exact: true })).toBeVisible();
    await main.getByRole("link", { name: /local/ }).click();
    await main.getByRole("link", { name: "Search these chats" }).click();
    await expect(page).toHaveURL(/#\/search\?machine=local$/);
    await expect(page.getByRole("link", { name: "Remove filter local" })).toBeVisible();
    await expect(recentChat).toBeVisible();
    await searchBox(page).fill("watcher");
    await searchBox(page).press("Enter");
    await expect(page).toHaveURL(/machine=local/);
    await expect(main.getByText("watcher result")).toBeVisible();
    await main.getByRole("link", { name: /Investigate the watcher/ }).click();
    await expect(page.locator("#message-message-1")).toContainText("Investigate the watcher");
    // The reader's own message is formatted, keeps its line breaks, and shows
    // a tag they typed as text.
    await expect(page.locator("#message-message-1 strong")).toHaveText("Agreed");
    await expect(page.locator("#message-message-1")).toContainText("<plan> first");
    await expect(page.locator("#message-message-1 p br")).toHaveCount(1);
    const sessions = page.getByRole("navigation", { name: "Prompts and sessions" });
    await expect(sessions).toBeVisible();
    await page.getByRole("button", { name: "Hide session list" }).click();
    await expect(sessions).toBeHidden();
    await page.getByRole("button", { name: "Show session list" }).click();
    await expect(sessions).toBeVisible();
    await page.evaluate(() => window.scrollTo(0, 200));
    await page.goBack();
    await expect(searchBox(page)).toHaveValue("watcher");
    await expect(main.getByText("watcher result")).toBeVisible();
    expect(searches).toBe(1);
    await followRail(page, "viewing history", "chats");
    await expect(page.getByRole("heading", { name: "Viewing history", exact: true })).toBeVisible();
    await expect(recentChat).toBeVisible();
    await followRail(page, "facts", "memory");
    await expect(
      page
        .getByRole("navigation", { name: "Memory kind" })
        .getByRole("link", { name: "facts", exact: true }),
    ).toHaveAttribute("aria-current", "page");
    expect(await fitsViewport(page)).toBe(true);
    expect(documents).toBe(1);
    expect(errors).toEqual([]);
  });
}

// The earlier drawer was a dialog that Escape closed, handing focus back to
// the button that opened it. The rail drawer has no key handler (Rail.svelte,
// App.svelte), so Escape leaves it open over the page and a keyboard reader
// must tab to "Close navigation". Remove fixme once the source handles it.
test("the navigation drawer closes on Escape and returns focus to its button", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 900 });
  await mockHistory(page, () => {});
  await page.goto("/");
  const open = page.getByRole("button", { name: "Open navigation" });
  await open.click();
  await expect(rail(page)).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(rail(page)).toBeHidden();
  await expect(open).toBeFocused();
});
