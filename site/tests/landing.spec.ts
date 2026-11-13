import { expect, test } from "@playwright/test";

const agents = ["Claude Code", "Codex", "GitHub Copilot", "Cursor", "OpenCode", "Antigravity"];

test("the landing page says what Mash is and which agents it reads", async ({ page }) => {
  await page.goto("/");
  await expect(page.locator("h1")).toHaveText("Your agents forget. Mash doesn’t.");
  await expect(page).toHaveTitle(/Your agents forget\. Mash doesn’t\./);
  const marks = page.locator(".mash-agents li");
  await expect(marks).toHaveCount(agents.length);
  await expect(marks).toHaveText(agents);
  // Every mark is drawn from a file the site actually serves.
  for (const mark of await page.locator(".mash-agents .mark").all()) {
    const file = await mark.evaluate((node) => getComputedStyle(node).maskImage.slice(5, -2));
    expect((await page.request.get(file)).ok()).toBe(true);
  }
  await expect(page.locator(".card-grid .card")).toHaveCount(4);
  await expect(page.locator(".hero .actions a")).toHaveText([
    "Get started",
    "Try the demo",
    "Explore the docs",
  ]);
  await expect(page.getByRole("navigation", { name: "Footer" }).getByRole("link")).toHaveText([
    "Docs",
    "Demo",
    "Privacy",
    "GitHub",
  ]);
});

test("the landing page fits a phone", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});

test("the hero draws its surface in the theme's ink", async ({ page }) => {
  await page.goto("/");
  const surface = page.locator("canvas.mash-surface");
  // It spans the window without widening the page.
  await expect
    .poll(() => surface.evaluate((node) => node.getBoundingClientRect().width))
    .toBe(await page.evaluate(() => document.documentElement.clientWidth));
  const painted = () =>
    surface.evaluate((node) => {
      const canvas = node as HTMLCanvasElement;
      const pixels = canvas.getContext("2d")?.getImageData(0, 0, canvas.width, canvas.height).data;
      let count = 0;
      for (let i = 3; pixels && i < pixels.length; i += 4) if (pixels[i]) count += 1;
      return count;
    });
  await expect.poll(painted).toBeGreaterThan(1000);
  expect(await surface.evaluate((node) => getComputedStyle(node).color)).toBe(
    await page.evaluate(() => getComputedStyle(document.querySelector("h1") as Element).color),
  );
});
