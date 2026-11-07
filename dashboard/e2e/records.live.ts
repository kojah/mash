import process from "node:process";
import { expect, test } from "@playwright/test";

// Run against a disposable Mash database seeded with four Dashboard records
// through the real MCP record tool. No route interception or writes in-browser.
test("live records browse, filter, expose revisions and open the real source chat", async ({
  page,
}) => {
  test.skip(
    process.env.MASH_DASHBOARD_RECORD_FIXTURE !== "1",
    "Requires the disposable record fixture",
  );
  const errors: string[] = [];
  let documents = 0;
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("request", (request) => {
    if (request.resourceType() === "document") documents++;
  });
  await page.goto("/");
  await page.getByRole("link", { name: "Memory", exact: true }).click();
  await expect(page.locator("li h2")).toHaveCount(4);
  await page.screenshot({
    path: "../../mash-artifacts/dashboard-records-live/list.png",
    fullPage: true,
  });
  await page.getByRole("combobox", { name: "Kind", exact: true }).selectOption("fact");
  await page.getByRole("button", { name: "Apply filters" }).click();
  await expect(page.locator("li h2")).toHaveCount(1);
  await page.getByRole("link", { name: /Dashboard fact revised/ }).click();
  await expect(
    page.getByRole("heading", { name: "Revision 2 · Current", exact: true }),
  ).toBeVisible();
  await expect(page.getByRole("heading", { name: "Revision 1", exact: true })).toBeVisible();
  await expect(page.getByText("Expired", { exact: false }).first()).toBeVisible();
  await page.screenshot({
    path: "../../mash-artifacts/dashboard-records-live/detail.png",
    fullPage: true,
  });
  await page.getByRole("link", { name: "Open source message" }).first().click();
  await expect(
    page.getByText("Use port 8080. I prefer concise replies. The platform team owns the review.", {
      exact: true,
    }),
  ).toBeVisible();
  await page.getByRole("link", { name: "Chat memory" }).click();
  await expect(page.locator("li h2")).toHaveCount(4);
  await expect(page.getByRole("alert")).toHaveCount(0);
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(page.getByRole("heading", { name: "Memory", exact: true })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.screenshot({
    path: "../../mash-artifacts/dashboard-records-live/mobile-list.png",
    fullPage: true,
  });
  expect(documents).toBe(1);
  expect(errors).toEqual([]);
});
