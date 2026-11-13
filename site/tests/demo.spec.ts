import { expect, test } from "@playwright/test";

test("the landing page leads to a working demo of the dashboard", async ({ page }) => {
  const problems: string[] = [];
  page.on("pageerror", (error) => problems.push(error.message));
  page.on("response", (response) => {
    if (response.status() >= 400) problems.push(`${response.status()} ${response.url()}`);
  });
  await page.goto("/");
  await page.locator(".hero").getByRole("link", { name: "Try it", exact: true }).click();
  await expect(page).toHaveURL(/\/demo\/$/);
  await expect(page.getByRole("complementary", { name: "About this demo" })).toContainText(
    "This history is made up",
  );
  // The dashboard itself, answering from the made-up history.
  await page.getByRole("link", { name: "Move sign-in to short-lived tokens" }).first().click();
  await expect(page.locator("#message-sign-in-tokens-2")).toBeVisible();
  await page.getByRole("textbox", { name: "Search your history" }).fill("invoice exports queue");
  await page.keyboard.press("Enter");
  await expect(
    page.getByRole("link", { name: "Choose a queue for invoice exports" }).first(),
  ).toBeVisible();
  expect(problems).toEqual([]);
});
