import { expect, test } from "@playwright/test";

test("the light and dark switch sits in the top corner and keeps its choice", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.route("**/api/**", (route) =>
    route.fulfill({ json: { local_node: "xenia", chats: [], truncated: false } }),
  );
  await page.goto("/");
  const html = page.locator("html");
  const ground = () => page.evaluate(() => getComputedStyle(document.body).backgroundColor);
  // Dark until the reader chooses otherwise.
  await expect(html).not.toHaveAttribute("data-theme");
  const dark = await ground();
  const toLight = page.getByRole("button", { name: "Switch to light mode" });
  const box = await toLight.boundingBox();
  expect(box && box.x + box.width).toBeGreaterThan(1280 - 40);
  expect(box?.y).toBeLessThan(40);
  await toLight.click();
  await expect(html).toHaveAttribute("data-theme", "light");
  expect(await ground()).not.toBe(dark);
  // The logo follows: the light one has the dark dome.
  await expect
    .poll(() => page.locator("#rail .brand img").evaluate((img) => getComputedStyle(img).content))
    .toContain("mash-mark-light.svg");
  await page.reload();
  await expect(html).toHaveAttribute("data-theme", "light");
  await page.getByRole("button", { name: "Switch to dark mode" }).click();
  await expect(html).not.toHaveAttribute("data-theme");
  expect(await ground()).toBe(dark);
});
