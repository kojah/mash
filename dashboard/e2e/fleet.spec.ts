import { expect, test } from "@playwright/test";
import type { FleetRequest } from "../src/lib/services.svelte";
import { fitsViewport } from "./shell";

for (const width of [390, 1280]) {
  test(`fleet confirmation, progress and uncertain outcomes at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 850 });
    let documents = 0;
    let submissions = 0;
    let submitted: FleetRequest | null = null;
    const action = width === 390 ? "status" : "watch-start";
    const label = width === 390 ? "Check machines" : "Set up and start watchers";
    const group = width === 390 ? "check" : "watchers";
    const readOnly = action === "status";
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
            // The shared server is a service of xenia, not a machine; only
            // some actions can target it.
            actions: [{ action, targets: [...(readOnly ? ["shared"] : []), "xenia", "macbook"] }],
            services: [{ id: "shared", name: "Shared server", machine: "xenia" }],
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
    await page.goto("/#/fleet");
    await expect(page.getByRole("heading", { name: "Manage fleet", exact: true })).toBeVisible();
    await expect(page.getByRole("heading", { name: group, exact: true })).toBeVisible();
    await page.getByRole("link", { name: label, exact: true }).click();
    await expect(page).toHaveURL(new RegExp(`#/fleet/${action}$`));
    await expect(page.getByRole("heading", { name: label, exact: true })).toBeVisible();
    const machine = (name: string) => page.getByRole("checkbox", { name, exact: true });
    const start = page.getByRole("button", { name: `${label} on 1 machine`, exact: true });
    const confirm = page.getByRole("group", { name: "Confirm", exact: true });
    await expect(
      page.getByRole("button", { name: readOnly ? "Choose targets" : "Choose machines" }),
    ).toBeDisabled();
    await page.getByRole("button", { name: "Select all" }).click();
    await expect(machine("macbook")).toBeChecked();
    await machine("xenia").uncheck();
    const server = page.getByRole("checkbox", { name: "Shared server", exact: true });
    if (readOnly) {
      await expect(page.getByRole("heading", { name: "services", exact: true })).toBeVisible();
      await expect(server).toContainText("on xenia");
      await expect(server).toBeChecked();
      await expect(
        page.getByRole("button", { name: `${label} on 2 targets`, exact: true }),
      ).toBeVisible();
      await server.uncheck();
    } else {
      await expect(server).toHaveCount(0);
    }
    await start.click();
    if (readOnly) {
      // A check that changes nothing starts without a second step.
      await expect(confirm).toHaveCount(0);
    } else {
      expect(submissions).toBe(0);
      await expect(confirm).toContainText(`${label} on macbook?`);
      await confirm.getByRole("button", { name: "Cancel", exact: true }).click();
      await expect(confirm).toHaveCount(0);
      expect(submissions).toBe(0);
      await start.click();
      await confirm.getByRole("button", { name: "Yes, start it" }).click();
    }
    const run = page.getByRole("article").filter({ hasText: label });
    await expect(run).toHaveCount(1);
    await expect(run).toContainText("macbook");
    await expect(run.locator("p").last()).toHaveText("succeeded");
    expect(submissions).toBe(1);
    expect(documents).toBe(1);
    await machine("macbook").check();
    await start.click();
    if (!readOnly) await confirm.getByRole("button", { name: "Yes, start it" }).click();
    await expect(page.getByRole("alert")).toContainText(
      /Check the activity below and the affected machines before starting another operation/,
    );
    // An uncertain outcome blocks another submission until the reader says it was checked.
    await expect(start).toBeDisabled();
    await expect(machine("macbook")).toBeDisabled();
    await expect(page.getByRole("button", { name: "Select all" })).toBeDisabled();
    expect(submissions).toBe(2);
    expect(documents).toBe(1);
    await page.getByRole("button", { name: "I’ve checked the outcome" }).click();
    await expect(page.getByRole("alert")).toHaveCount(0);
    await expect(start).toBeEnabled();
    expect(submissions).toBe(2);
    expect(await fitsViewport(page)).toBe(true);
  });
}
