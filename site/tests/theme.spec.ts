import { expect, test } from "@playwright/test";

test("the light and dark button holds its choice across pages and visits", async ({ browser }) => {
  const context = await browser.newContext({
    colorScheme: "dark",
    viewport: { width: 1200, height: 800 },
  });
  const page = await context.newPage();
  await page.goto("/quick-start/");
  const html = page.locator("html");
  const button = page.getByRole("button", { name: "Switch between light and dark mode" });
  await expect(html).toHaveAttribute("data-theme", "dark");
  await button.click();
  await expect(html).toHaveAttribute("data-theme", "light");
  // The light theme shows the light logo: the dark dome, not the cream one.
  await expect(page.locator(".site-title img:visible")).toHaveAttribute("src", /mash-mark-light/);
  // Moving to another page without a full load keeps the choice and the button.
  await page.locator('nav.sidebar a[href="/installation/"]').click();
  await expect(page).toHaveURL(/\/installation\/$/);
  await expect(html).toHaveAttribute("data-theme", "light");
  await button.click();
  await expect(html).toHaveAttribute("data-theme", "dark");
  await button.click();
  // A later visit starts in the chosen theme.
  await page.reload();
  await expect(html).toHaveAttribute("data-theme", "light");
  await context.close();
});
