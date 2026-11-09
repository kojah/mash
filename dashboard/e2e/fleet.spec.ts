import { expect, test } from "@playwright/test";
import type { FleetRequest } from "../src/lib/services.svelte";

for (const width of [390, 1280]) {
  test(`fleet confirmation, progress and uncertain outcomes at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 850 });
    let documents = 0;
    let submissions = 0;
    let submitted: FleetRequest | null = null;
    const action = width === 390 ? "status" : "watch-start";
    const label = width === 390 ? "Check machines" : "Set up and start watchers";
    page.on("request", (request) => {
      if (request.resourceType() === "document") documents++;
    });
    await page.route("**/api/**", async (route) => {
      const request = route.request();
      const path = new URL(request.url()).pathname;
      if (path === "/api/services/access")
        return route.fulfill({ json: { authentication_required: false, local_node: "xenia" } });
      if (path === "/api/services") return route.fulfill({ json: { machines: [] } });
      if (path === "/api/services/fleet")
        return route.fulfill({
          json: {
            enabled: true,
            revision: "revision-one",
            actions: [{ action, targets: ["xenia", "macbook"] }],
          },
        });
      if (path === "/api/services/fleet/jobs") {
        if (request.method() === "POST") {
          submissions++;
          if (submissions > 1) return route.abort("failed");
          submitted = request.postDataJSON();
          expect(submitted).toMatchObject({
            action,
            targets: ["macbook"],
            revision: "revision-one",
          });
          expect(submitted?.id).toMatch(/^[a-f0-9-]{36}$/);
          return route.fulfill({
            json: {
              request: submitted,
              requested_at: 1,
              completed_at: null,
              outcome: "running",
              targets: [{ id: "macbook", state: "waiting", progress: null }],
            },
          });
        }
        return route.fulfill({
          json: submitted
            ? [
                {
                  request: submitted,
                  requested_at: 1,
                  completed_at: 2,
                  outcome: "succeeded",
                  targets: [{ id: "macbook", state: "succeeded", progress: null }],
                },
              ]
            : [],
        });
      }
      return route.fulfill({ json: { local_node: "xenia", chats: [], truncated: false } });
    });
    await page.goto("/#/services");
    await page.getByRole("button", { name: "Select all eligible machines" }).click();
    await expect(page.getByLabel("macbook", { exact: true })).toBeChecked();
    await page.getByLabel("xenia", { exact: true }).uncheck();
    await page.getByRole("button", { name: "Review operation" }).click();
    expect(submissions).toBe(0);
    await expect(page.getByRole("group", { name: "Confirm fleet operation" })).toContainText(
      "Machines: macbook",
    );
    await page.getByRole("button", { name: "Confirm operation" }).click();
    await expect(page.getByText(`${label} · succeeded`, { exact: true })).toBeVisible();
    expect(submissions).toBe(1);
    expect(documents).toBe(1);
    await page.getByLabel("macbook", { exact: true }).check();
    await page.getByRole("button", { name: "Review operation" }).click();
    await page.getByRole("button", { name: "Confirm operation" }).click();
    await expect(page.getByText(/Check job history and the affected machines/)).toBeVisible();
    await expect(page.getByRole("button", { name: "Review operation" })).toBeDisabled();
    expect(submissions).toBe(2);
    expect(documents).toBe(1);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(
      true,
    );
  });
}
