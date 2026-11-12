import { expect, test } from "@playwright/test";
import { fitsViewport } from "./shell";

for (const width of [390, 1440]) {
  test(`services open directly, inspect schemas and manage explicit targets at ${width}px`, async ({
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
        return route.fulfill({ json: { authentication_required: false, local_node: "xenia" } });
      if (url.pathname.startsWith("/api/services")) {
        expect(request.headers().authorization).toBeUndefined();
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
        if (url.pathname === "/api/services/installation") {
          expect(url.searchParams.get("machine")).toBe("xenia");
          const present = (component: string) => ({
            component,
            state: "present",
            message: `${component} found`,
            remediation: null,
          });
          const installed = ["mcp", "plugin", "skill", "hooks"].map(present);
          return route.fulfill({
            json: {
              build: "0123456789abcdef",
              agents: [
                {
                  agent: "codex",
                  expected: true,
                  connected: false,
                  steps: [
                    ...installed,
                    {
                      component: "hook_trust",
                      state: "missing",
                      message: "No user-level Codex trust record found for this hook.",
                      remediation: "Open /hooks in Codex and trust the current Mash hooks.",
                    },
                  ],
                },
                { agent: "claude-code", expected: true, connected: true, steps: installed },
              ],
            },
          });
        }
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
    await expect(page.getByRole("heading", { name: "Services", exact: true })).toBeVisible();
    await expect(page.getByLabel("Services access token")).toHaveCount(0);
    // Without a machine in the address every machine is listed and none is picked.
    await expect(page.getByRole("link", { name: "xenia: needs attention" })).not.toHaveAttribute(
      "aria-current",
    );
    await expect(page.getByRole("link", { name: "macbook: unreachable" })).toBeVisible();
    await expect(page.getByRole("heading", { name: "services on macbook" })).toBeVisible();
    await expect(page.getByRole("region", { name: "Needs attention on macbook" })).toBeVisible();
    await expect(page.getByRole("heading", { name: "services on xenia" })).toBeVisible();
    // Each fault is listed with what to do about it and, where one exists, its control.
    const attention = page.getByRole("region", { name: "Needs attention on xenia" });
    await expect(attention.getByText("Running · schema mismatch", { exact: true })).toBeVisible();
    await expect(
      attention.getByText("Reconnect the MCP. Do not downgrade the database."),
    ).toBeVisible();
    await attention.getByRole("button", { name: "Start", exact: true }).click();
    const inline = attention.getByRole("group", { name: "Confirm service action" });
    await expect(inline).toContainText("Start Index watcher on xenia?");
    await inline.getByRole("button", { name: "Cancel", exact: true }).click();
    expect(mutations).toBe(0);
    // A deployed plugin whose hooks await approval is visible from the list.
    const setup = page
      .locator("section")
      .filter({ has: page.getByRole("heading", { name: "agent setup", exact: true }) });
    await expect(setup.getByText("Hooks need approval")).toBeVisible();
    await expect(setup.getByText("Ready", { exact: true })).toBeVisible();
    await expect(
      setup.getByText("Open /hooks in Codex, trust the Mash hooks, then start a new session."),
    ).toBeVisible();
    await page.getByRole("link", { name: /Mash MCP · PID 10/ }).click();
    await expect(page.getByRole("heading", { name: "Schema compatibility" })).toBeVisible();
    await expect(page.getByText("database 65 / supported 64", { exact: true })).toBeVisible();
    await expect(
      page.getByText("Reconnect the MCP. Do not downgrade the database.", { exact: true }),
    ).toBeVisible();
    await expect(page.getByRole("button", { name: "Restart", exact: true })).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Start", exact: true })).toHaveCount(0);
    await page.getByRole("link", { name: "< services" }).click();
    await expect(page).toHaveURL(/#\/services\?machine=xenia$/);
    await expect(setup.getByText("Hooks need approval")).toBeVisible();
    // Choosing a machine in the table narrows the page to it; a link widens it again.
    await page.getByRole("link", { name: "macbook: unreachable" }).click();
    await expect(page.getByRole("region", { name: "Needs attention on macbook" })).toContainText(
      "Could not be reached",
    );
    await expect(page.getByRole("heading", { name: "services on xenia" })).toHaveCount(0);
    await page.getByRole("link", { name: "Show every machine" }).click();
    await expect(page.getByRole("heading", { name: "services on xenia" })).toBeVisible();
    await page.getByRole("link", { name: "xenia: needs attention" }).click();
    await page.getByRole("link", { name: /Index watcher/ }).click();
    await expect(page.getByText("Start it to bring it back.")).toBeVisible();
    await page.getByRole("button", { name: "Start", exact: true }).click();
    const confirm = page.getByRole("group", { name: "Confirm service action" });
    await expect(confirm).toContainText("Start Index watcher on xenia?");
    expect(mutations).toBe(0);
    await confirm.getByRole("button", { name: "Cancel", exact: true }).click();
    await expect(confirm).toHaveCount(0);
    expect(mutations).toBe(0);
    await page.getByRole("button", { name: "Start", exact: true }).click();
    await page.getByRole("button", { name: "Confirm start" }).click();
    await expect(page.getByRole("button", { name: "Restart", exact: true })).toBeVisible();
    expect(mutations).toBe(1);
    await page.getByRole("button", { name: "View log" }).click();
    await expect(page.locator(".service-log")).toContainText("<script>not executable</script>");
    await expect(page.locator(".service-log script")).toHaveCount(0);
    await page.getByRole("button", { name: "Hide log" }).click();
    await expect(page.locator(".service-log")).toHaveCount(0);
    await page.getByRole("button", { name: "Restart", exact: true }).click();
    await page.getByRole("button", { name: "Confirm restart" }).click();
    await expect(page.getByRole("alert")).toContainText("Outcome unknown");
    // An unconfirmed outcome blocks further actions until status is checked again.
    await expect(page.getByRole("button", { name: "Restart", exact: true })).toBeDisabled();
    await expect(page.getByRole("button", { name: "Stop", exact: true })).toBeDisabled();
    await page.waitForLoadState("networkidle");
    const reads = serviceReads;
    await page.clock.runFor(91_000);
    expect(serviceReads).toBe(reads);
    expect(mutations).toBe(2);
    expect(await fitsViewport(page)).toBe(true);
    expect(documents).toBe(1);
    expect(errors).toEqual([]);
  });
}
