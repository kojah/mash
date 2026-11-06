import { expect, test } from "@playwright/test";

test("internal links and browser history keep the current document alive", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto("/");
  const origin = await page.evaluate(() => performance.timeOrigin);
  await page.getByRole("link", { name: "Get started", exact: true }).click();
  await expect(page).toHaveURL(/\/quick-start\/$/);
  await expect(page.locator("h1")).toHaveText("Quick start");
  expect(await page.evaluate(() => performance.timeOrigin)).toBe(origin);
  await page.locator('nav.sidebar a[href="/installation/"]').click();
  await expect(page.locator("h1")).toHaveText("Install Mash");
  expect(await page.evaluate(() => performance.timeOrigin)).toBe(origin);
  await page.goBack();
  await expect(page.locator("h1")).toHaveText("Quick start");
  expect(await page.evaluate(() => performance.timeOrigin)).toBe(origin);
  await page.getByRole("button", { name: "Search", exact: true }).click();
  await page.getByPlaceholder("Search", { exact: true }).fill("Qdrant");
  await expect(page.locator("dialog a").first()).toBeVisible();
  await page.locator("dialog a").first().click();
  await expect(page.locator("dialog")).not.toBeVisible();
  expect(await page.evaluate(() => performance.timeOrigin)).toBe(origin);
  await page.keyboard.press("Control+k");
  await expect(page.locator("dialog")).toBeVisible();
  expect(errors).toEqual([]);
});

test("mobile menu and theme still work after navigation", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/quick-start/");
  await page.getByRole("button", { name: "Menu", exact: true }).click();
  await page.locator('nav.sidebar a[href="/installation/"]').click();
  await expect(page.locator("h1")).toHaveText("Install Mash");
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.getByRole("button", { name: "Menu", exact: true }).click();
  await page.locator("starlight-theme-select select:visible").selectOption("dark");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await page.locator('nav.sidebar a[href="/search/"]').click();
  await expect(page.locator("h1")).toHaveText("Find a conversation");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
});

test("direct links remain usable without JavaScript", async ({ browser }) => {
  const context = await browser.newContext({ javaScriptEnabled: false });
  const page = await context.newPage();
  await page.goto("http://127.0.0.1:8793/installation/");
  await expect(page.locator("h1")).toHaveText("Install Mash");
  await page.locator('nav.sidebar a[href="/search/"]').click();
  await expect(page.locator("h1")).toHaveText("Find a conversation");
  await context.close();
});
