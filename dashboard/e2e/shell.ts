import type { Locator, Page } from "@playwright/test";

/** The rail's links. Below 800px the rail is a drawer and its links are hidden until opened. */
export function rail(page: Page): Locator {
  return page.getByRole("navigation", { name: "Main" });
}

export function searchBox(page: Page): Locator {
  return page.getByRole("textbox", { name: "Search your history" });
}

/** Opens the drawer on a narrow screen; the rail is always shown on a wide one. */
export async function openRail(page: Page): Promise<void> {
  const burger = page.getByRole("button", { name: "Open navigation" });
  if ((await burger.isVisible()) && (await burger.getAttribute("aria-expanded")) !== "true")
    await burger.click();
}

/**
 * Follows a rail link the way a reader would: opening the drawer when the
 * screen is narrow, and expanding `section` when the link is one of its children.
 */
export async function followRail(page: Page, label: string, section?: string): Promise<void> {
  await openRail(page);
  const link = rail(page).getByRole("link", { name: label, exact: true });
  if (section && !(await link.isVisible()))
    await rail(page)
      .getByRole("button", { name: `Expand ${section}`, exact: true })
      .click();
  await link.click();
}

export function fitsViewport(page: Page): Promise<boolean> {
  return page.evaluate(() => document.documentElement.scrollWidth <= innerWidth);
}
