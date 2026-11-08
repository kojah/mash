import { expect, test } from "@playwright/test";

for (const width of [390, 1440]) {
  test(`services authenticate, inspect schemas and manage explicit targets at ${width}px`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 });
    let documents = 0,
      serviceReads = 0,
      mutations = 0,
      running = false;
    const errors: string[] = [];
    page.on("pageerror", (error) => errors.push(error.message));
    page.on("request", (request) => {
      if (request.resourceType() === "document") documents++;
    });
    const base = {
      control_revision: "fixture-target",
      checked_at: 1794140000,
      pid: null,
      diagnostics: [],
      logs_available: false,
      compatibility: null,
      control_note: "Managed explicitly in configuration.",
    };
    const machines = () => ({
      machines: [
        {
          id: "xenia",
          platform: "linux",
          checked_at: 1794140000,
          reachable: true,
          warnings: [],
          services: [
            {
              ...base,
              id: "managed-watch",
              name: "Index watcher",
              kind: "watcher",
              state: running ? "healthy" : "stopped",
              summary: running ? "Running" : "Stopped",
              actions: running ? ["stop", "restart"] : ["start"],
              logs_available: true,
            },
            {
              ...base,
              id: "runtime-10",
              name: "Mash MCP · PID 10",
              kind: "mcp",
              state: "failed",
              summary: "Running · schema mismatch",
              actions: [],
              control_note: "Reconnect Mash from the owning agent.",
              compatibility: {
                database_schema: 65,
                supported_schema: 64,
                running_build: "old-build",
                observer_build: "new-build",
                state: "failed",
                blocked: "Local reads and writes are blocked.",
                next_step: "Reconnect the MCP. Do not downgrade the database.",
              },
            },
          ],
        },
        {
          id: "macbook",
          platform: "unknown",
          checked_at: 1794140000,
          reachable: false,
          warnings: ["Machine could not be reached"],
          services: [],
        },
      ],
    });
    await page.route("**/api/**", async (route) => {
      const request = route.request(),
        url = new URL(request.url());
      if (url.pathname === "/api/services/access")
        return route.fulfill({ json: { authentication_required: true, local_node: "xenia" } });
      if (url.pathname.startsWith("/api/services")) {
        expect(request.headers().authorization).toBe("Bearer fixture-access-token");
        if (url.pathname === "/api/services") {
          serviceReads++;
          return route.fulfill({ json: machines() });
        }
        if (url.pathname === "/api/services/log")
          return route.fulfill({
            json: {
              text: "<script>not executable</script>\nWatcher started",
              checked_at: 1794140000,
            },
          });
        if (url.pathname === "/api/services/operations") return route.fulfill({ json: [] });
        if (url.pathname === "/api/services/action") {
          mutations++;
          const body = request.postDataJSON();
          expect(body.machine).toBe("xenia");
          expect(body.service).toBe("managed-watch");
          expect(body.request_id).toMatch(/^[\da-f-]{36}$/);
          if (body.action === "restart")
            return route.fulfill({
              status: 502,
              json: { error: { message: "Outcome unknown. Check status before retrying." } },
            });
          expect(body.action).toBe("start");
          running = true;
          return route.fulfill({
            json: {
              request: body,
              requested_at: 1794140000,
              completed_at: 1794140001,
              outcome: "succeeded",
              message: "Requested process state observed.",
            },
          });
        }
      }
      return route.fulfill({ json: { local_node: "xenia", chats: [], truncated: false } });
    });
    await page.clock.install();
    await page.goto("/#/services");
    await page.getByLabel("Services access token").fill("fixture-access-token");
    expect(serviceReads).toBe(0);
    await page.getByRole("button", { name: "Unlock services" }).click();
    await expect(page.getByText("Running · schema mismatch", { exact: true })).toBeVisible();
    await page.getByRole("link", { name: /Mash MCP · PID 10/ }).click();
    await expect(page.getByRole("heading", { name: "Schema compatibility" })).toBeVisible();
    await expect(page.getByText("65", { exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: "Restart", exact: true })).toHaveCount(0);
    await page.getByRole("link", { name: "xenia", exact: true }).click();
    await page.getByRole("link", { name: /Index watcher/ }).click();
    await page.getByRole("button", { name: "Start", exact: true }).click();
    await expect(page.getByRole("group", { name: "Confirm service action" })).toContainText(
      "xenia",
    );
    expect(mutations).toBe(0);
    await page.getByRole("button", { name: "Cancel", exact: true }).click();
    expect(mutations).toBe(0);
    await page.getByRole("button", { name: "Start", exact: true }).click();
    await page.getByRole("button", { name: "Confirm start" }).click();
    await expect(page.getByRole("button", { name: "Restart", exact: true })).toBeVisible();
    expect(mutations).toBe(1);
    await page.getByRole("button", { name: "View log" }).click();
    await expect(page.locator(".service-log")).toContainText("<script>not executable</script>");
    await expect(page.locator(".service-log script")).toHaveCount(0);
    await page.getByRole("button", { name: "Restart", exact: true }).click();
    await page.getByRole("button", { name: "Confirm restart" }).click();
    await expect(page.getByRole("alert")).toContainText("Outcome unknown");
    await page.waitForLoadState("networkidle");
    const reads = serviceReads;
    await page.clock.runFor(91_000);
    expect(serviceReads).toBe(reads);
    expect(mutations).toBe(2);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(
      true,
    );
    expect(
      await page.evaluate(() =>
        Object.values(localStorage).some((value) => value.includes("fixture-access-token")),
      ),
    ).toBe(false);
    await page.getByRole("button", { name: "Lock services" }).click();
    await expect(page.getByLabel("Services access token")).toBeVisible();
    await expect(page.getByText("old-build", { exact: true })).toHaveCount(0);
    expect(documents).toBe(1);
    expect(errors).toEqual([]);
  });
}
