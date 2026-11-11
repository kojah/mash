import { expect, type Page, test } from "@playwright/test";
import { followRail, rail } from "./shell";

// Answers every read the rail's pages make with an empty but valid body.
async function mockEmptyDashboard(page: Page): Promise<void> {
  await page.route("**/api/**", (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/api/records") return route.fulfill({ json: { records: [], next_offset: null } });
    if (path === "/api/services") return route.fulfill({ json: { machines: [] } });
    if (path === "/api/services/operations") return route.fulfill({ json: [] });
    if (path === "/api/services/fleet")
      return route.fulfill({ json: { enabled: false, revision: null, actions: [], services: [] } });
    if (path === "/api/status") return route.fulfill({ json: statusFixture });
    return route.fulfill({ json: { local_node: "local", chats: [], truncated: false } });
  });
}

const statusFixture = {
  ready: true,
  chats: 0,
  messages: 0,
  searchable_messages: 0,
  models_installed: true,
  realized_mode: "hybrid",
  database_path: "/fixture/mash.db",
  recommended_action: null,
  configuration: {
    path: "/fixture/config.toml",
    loaded: true,
    providers: { codex: { roots: ["/fixture/codex"] } },
    index: { since_days: null, destination: "local" },
    embeddings: { backend: "local" },
    qdrant: { url: "http://127.0.0.1:6334", vector_search: { algorithm: "hnsw" } },
    views: { query: { ranking: "rrf" } },
    records: { enabled: true },
    redaction: { output: true, publication: true },
    shared: null,
  },
};

for (const width of [390, 1280]) {
  test(`rail routes update history without triggering fragment navigation at ${width}px`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 850 });
    let documents = 0;
    const errors: string[] = [];
    page.on("pageerror", (error) => errors.push(error.message));
    page.on("request", (request) => {
      if (request.resourceType() === "document") documents++;
    });
    await mockEmptyDashboard(page);
    await page.goto("/");
    await page.evaluate(() => {
      document.documentElement.dataset.fragmentNavigations = "0";
      for (const selector of ["header", "aside"])
        document.querySelector(selector)?.setAttribute("data-retained", "true");
      window.addEventListener("hashchange", () => {
        document.documentElement.dataset.fragmentNavigations = String(
          Number(document.documentElement.dataset.fragmentNavigations) + 1,
        );
      });
    });
    const burger = page.getByRole("button", { name: "Open navigation" });
    // Between links the drawer is shut on a narrow screen; a wide one has no burger.
    const hiddenAtRest = width < 800 ? rail(page) : burger;
    for (const [label, section, hash, heading] of [
      ["machines", "", "#/machines", "Machines"],
      ["agents", "", "#/agents", "Agents"],
      ["projects", "", "#/projects", "Projects"],
      ["chats", "", "#/search", "Chats"],
      ["viewing history", "chats", "#/history", "Viewing history"],
      ["recent", "chats", "#/search", "Chats"],
      ["memory", "", "#/records?", "Memory"],
      ["decisions", "memory", "#/records?kind=decision", "Memory"],
      ["manage fleet", "", "#/fleet", "Manage fleet"],
      ["mcp console", "", "#/console", "MCP console"],
      ["settings", "", "#/settings", "Settings"],
      ["services", "", "#/services", "Services"],
      ["overview", "", "#/", "Overview"],
    ] as const) {
      await expect(hiddenAtRest).toBeHidden();
      await followRail(page, label, section || undefined);
      expect(await page.evaluate(() => location.hash)).toBe(hash);
      await expect(
        page.getByRole("heading", { level: 1, name: heading, exact: true }),
      ).toBeVisible();
      await expect(hiddenAtRest).toBeHidden();
      await expect(page.getByRole("banner")).toHaveAttribute("data-retained", "true");
      await expect(page.locator("#rail")).toHaveAttribute("data-retained", "true");
    }
    expect(await page.evaluate(() => document.documentElement.dataset.fragmentNavigations)).toBe(
      "0",
    );
    expect(documents).toBe(1);
    await page.goBack();
    await expect(page.getByRole("heading", { name: "Services", exact: true })).toBeVisible();
    await page.goForward();
    await expect(page.getByRole("heading", { name: "recent chats", exact: true })).toBeVisible();
    expect(documents).toBe(1);
    expect(errors).toEqual([]);
  });
}

test("an unknown address shows a missing page instead of the overview", async ({ page }) => {
  await mockEmptyDashboard(page);
  await page.goto("/#/no-such-page");
  await expect(page.getByRole("heading", { name: "Page not found", exact: true })).toBeVisible();
  await expect(page.getByRole("heading", { name: "recent chats", exact: true })).toHaveCount(0);
  await page.getByRole("link", { name: "Go to overview" }).click();
  await expect(page.getByRole("heading", { name: "recent chats", exact: true })).toBeVisible();
});

test("the app boundary prevents document navigation from bare links and unhandled forms", async ({
  page,
}) => {
  let documents = 0;
  page.on("request", (request) => {
    if (request.resourceType() === "document") documents++;
  });
  await page.route("**/api/**", (route) =>
    route.fulfill({ json: { local_node: "local", chats: [], truncated: false } }),
  );
  await page.goto("/#/agents");
  await expect(page.getByRole("heading", { name: "Agents", exact: true })).toBeVisible();
  // A future component can omit preventDefault without reloading the app.
  await page.evaluate(() => {
    const form = document.createElement("form");
    form.id = "unhandled-form";
    const button = document.createElement("button");
    button.textContent = "Unhandled submit";
    form.append(button);
    document.body.append(form);
    const link = document.createElement("a");
    link.href = location.pathname;
    link.textContent = "Bare home link";
    document.body.append(link);
  });
  await page.getByRole("button", { name: "Unhandled submit" }).click();
  await expect(page.locator('meta[http-equiv="Content-Security-Policy"]')).toHaveAttribute(
    "content",
    "form-action 'none'",
  );
  await expect(page.locator("#unhandled-form")).toBeAttached();
  await page.getByRole("link", { name: "Bare home link" }).click();
  await expect(page).toHaveURL(/#\/$/);
  await page.goBack();
  await expect(page.getByRole("heading", { name: "Agents", exact: true })).toBeVisible();
  await page.goForward();
  await expect(page).toHaveURL(/#\/$/);
  await expect(page.locator("#unhandled-form")).toBeAttached();
  expect(documents).toBe(1);
  expect(
    await page.evaluate(() => getComputedStyle(document.documentElement).overscrollBehaviorY),
  ).toBe("none");
  // Native submit() bypasses events. Observe the browser's enforcement event
  // directly: Playwright locators can wait on the cancelled navigation.
  const blocked = await page.evaluate(
    () =>
      new Promise<string>((resolve, reject) => {
        const form = document.querySelector("#unhandled-form");
        if (!(form instanceof HTMLFormElement)) return reject(new Error("Missing fixture form"));
        document.addEventListener(
          "securitypolicyviolation",
          (event) => resolve(event.effectiveDirective),
          { once: true },
        );
        form.submit();
      }),
  );
  expect(blocked).toBe("form-action");
  expect(await page.evaluate(() => document.querySelector("#unhandled-form") !== null)).toBe(true);
  expect(documents).toBe(1);
});

test("the old index status address opens services, which reports the local index", async ({
  page,
}) => {
  await page.route("**/api/**", (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/api/services/access")
      return route.fulfill({ json: { authentication_required: false, local_node: "xenia" } });
    if (path === "/api/services/operations") return route.fulfill({ json: [] });
    if (path === "/api/services/installation")
      return route.fulfill({ json: { build: "0123456789abcdef", agents: [] } });
    if (path === "/api/services")
      return route.fulfill({
        json: {
          machines: [
            {
              id: "xenia",
              platform: "linux",
              checked_at: 1794140000,
              reachable: true,
              warnings: [],
              services: [
                {
                  id: "local-index",
                  name: "Local index",
                  kind: "sqlite",
                  state: "healthy",
                  summary: "Schema 65 · current",
                  checked_at: 1794140000,
                  pid: null,
                  actions: [],
                  control_note: "Read by this dashboard.",
                  diagnostics: [],
                  logs_available: false,
                  control_revision: null,
                  compatibility: null,
                },
              ],
            },
          ],
        },
      });
    if (path === "/api/records") return route.fulfill({ json: { records: [], next_offset: null } });
    return route.fulfill({
      json: { local_node: "xenia", truncated: false, chats: [], shared_error: "shared offline" },
    });
  });
  await page.goto("/#/status");
  await expect(page.getByRole("heading", { name: "Services", exact: true })).toBeVisible();
  // The local machine is selected without being named in the address.
  await expect(page.getByRole("heading", { name: "services on xenia", exact: true })).toBeVisible();
  const index = page.getByRole("link", { name: /Local index/ });
  await expect(index).toContainText("sqlite");
  await expect(index).toContainText("Schema 65 · current");
  await expect(page.getByRole("link", { name: "xenia: all services healthy" })).toHaveAttribute(
    "aria-current",
    "true",
  );
  await expect(page.getByRole("navigation", { name: "Main" })).toContainText("xenia / 0 chats");
  await expect(page.getByRole("link", { name: "Index status" })).toHaveCount(0);
});
