import process from "node:process";
import { expect, test } from "@playwright/test";

// These tests use the deployed page and real API, without route interception.
// Choose a query known to occur in the target's indexed history.
const query = process.env.MASH_DASHBOARD_QUERY || "database";

for (const submit of ["Enter", "button"] as const) {
  test(`live search via ${submit} opens a matching chat`, async ({ page }) => {
    test.setTimeout(120_000);
    const errors: string[] = [];
    page.on("pageerror", (error) => errors.push(error.message));
    let documents = 0;
    page.on("request", (request) => {
      if (request.resourceType() === "document") documents++;
    });
    await page.goto("/");
    await expect(page.locator('a[href*="/chat/"]').first()).toBeVisible({ timeout: 30_000 });
    await page.getByRole("searchbox").fill(query);
    const responseReady = page.waitForResponse(
      (response) => new URL(response.url()).pathname === "/api/search",
      { timeout: 90_000 },
    );
    if (submit === "Enter") await page.getByRole("searchbox").press("Enter");
    else await page.getByRole("button", { name: "Search", exact: true }).click();
    const response = await responseReady;
    expect(response.status()).toBe(200);
    const match = page.getByRole("region", { name: "Search results" }).getByRole("link").first();
    await expect(match).toBeVisible();
    await match.click();
    await expect(page.locator("main ol > li").first()).toBeVisible({ timeout: 30_000 });
    await expect(page.getByRole("alert")).toHaveCount(0);
    expect(documents).toBe(1);
    expect(errors).toEqual([]);
  });
}
