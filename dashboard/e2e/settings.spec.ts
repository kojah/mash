import { expect, type Page, test } from "@playwright/test";
import { fitsViewport } from "./shell";

const status = {
  ready: true,
  chats: 12,
  messages: 340,
  searchable_messages: 300,
  models_installed: true,
  realized_mode: "hybrid",
  database_path: "/fixture/mash.db",
  recommended_action: null,
  configuration: {
    path: "/fixture/config.toml",
    loaded: true,
    providers: { codex: { roots: ["/fixture/codex"] } },
    index: { since_days: null, destination: "both" },
    embeddings: { backend: "local" },
    qdrant: { url: "http://127.0.0.1:6334", vector_search: { algorithm: "hnsw" } },
    views: { query: { ranking: "rrf" } },
    records: { enabled: true },
    redaction: { output: true, publication: true },
    shared: { group_id: "personal", node_id: "fixture", api_url: "http://shared.invalid" },
  },
};
const settings = {
  path: "/fixture/config.toml",
  revision: "r1",
  editable: true,
  agents: {
    codex: { indexed: true, roots: ["/fixture/codex"] },
    cursor: { indexed: false, roots: ["/fixture/cursor"] },
  },
  since_days: null,
  destination: "both",
  shared: true,
  memory_capture: true,
  hide_credentials: { output: true, publication: true },
};

async function mock(page: Page, served: unknown, posts: unknown[]): Promise<void> {
  await page.route("**/api/**", async (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    if (path === "/api/status") return route.fulfill({ json: status });
    if (path === "/api/services/settings") {
      if (request.method() === "POST") {
        const body = request.postDataJSON();
        posts.push(body);
        if (posts.length === 1)
          return route.fulfill({
            status: 400,
            json: {
              error: {
                message:
                  "the configuration file changed since these settings were loaded; reload them and try again",
              },
            },
          });
        return route.fulfill({
          json: {
            ...settings,
            revision: "r2",
            agents: { ...settings.agents, cursor: { indexed: true, roots: ["/fixture/cursor"] } },
            since_days: 30,
            hide_credentials: { output: true, publication: false },
          },
        });
      }
      return route.fulfill({ json: served });
    }
    if (path === "/api/services/access")
      return route.fulfill({ json: { authentication_required: false, local_node: "fixture" } });
    if (path === "/api/chats")
      return route.fulfill({ json: { local_node: "fixture", truncated: false, chats: [] } });
    return route.fulfill({ json: [] });
  });
}

for (const width of [390, 1280]) {
  test(`settings are changed, reviewed and saved at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    const posts: unknown[] = [];
    await mock(page, settings, posts);
    await page.goto("/#/settings");
    await expect(page.getByRole("heading", { name: "Settings", exact: true })).toBeVisible();
    // Nothing is offered to save until something differs from the file.
    await expect(page.getByRole("region", { name: "Unsaved settings" })).toHaveCount(0);
    // Search internals are shown, never offered as a choice.
    await expect(page.getByText("HNSW", { exact: true })).toBeVisible();
    await expect(page.getByRole("switch")).toHaveCount(5);

    await page.getByRole("switch", { name: "Index Cursor" }).click();
    await page.getByRole("button", { name: "recent only" }).click();
    await page.getByRole("spinbutton", { name: "Days of history" }).fill("30");
    await page.getByRole("switch", { name: "Hide credentials when publishing" }).click();
    await expect(
      page.getByText(/publishes all of this machine's history again, including credentials/),
    ).toBeVisible();
    const bar = page.getByRole("region", { name: "Unsaved settings" });
    await expect(bar).toContainText("3 unsaved changes");
    await bar.getByRole("button", { name: "Review changes" }).click();
    await expect(bar.getByRole("listitem")).toHaveText([
      /index Cursor\s*off\s*→\s*on/,
      /history range\s*all history\s*→\s*the last 30 days/,
      /hide credentials when publishing\s*on\s*→\s*off/,
    ]);
    expect(await fitsViewport(page)).toBe(true);

    // A file edited elsewhere in the meantime is not overwritten; the reason is shown.
    await bar.getByRole("button", { name: "Save to file" }).click();
    await expect(bar.getByRole("alert")).toContainText("changed since these settings were loaded");
    await expect(bar).toContainText("3 unsaved changes");
    await bar.getByRole("button", { name: "Save to file" }).click();
    await expect(bar.getByRole("status")).toContainText("Saved 3 changes to the file");
    await expect(
      bar.getByRole("link", { name: "Restart the watcher from Services" }),
    ).toBeVisible();
    expect(posts).toHaveLength(2);
    expect(posts[1]).toEqual({
      revision: "r1",
      agents: { codex: true, cursor: true },
      since_days: 30,
      destination: "both",
      memory_capture: true,
      hide_credentials: { output: true, publication: false },
    });
    await expect(page.getByRole("switch", { name: "Index Cursor" })).toHaveAttribute(
      "aria-checked",
      "true",
    );

    // Turning every agent off cannot be saved.
    await page.getByRole("switch", { name: "Index Cursor" }).click();
    await page.getByRole("switch", { name: "Index Codex" }).click();
    await expect(bar.getByRole("alert")).toContainText("at least one agent");
    await expect(bar.getByRole("button", { name: "Save to file" })).toBeDisabled();
    await bar.getByRole("button", { name: "Discard" }).click();
    await expect(page.getByRole("switch", { name: "Index Codex" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    expect(posts).toHaveLength(2);
  });
}

test("a dashboard that does not accept changes shows settings without controls", async ({
  page,
}) => {
  const posts: unknown[] = [];
  await mock(page, { ...settings, editable: false }, posts);
  await page.goto("/#/settings");
  await expect(page.getByText(/does not change settings/)).toBeVisible();
  await expect(page.getByRole("switch")).toHaveCount(0);
  await expect(page.getByText("not indexed", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Save to file" })).toHaveCount(0);
});
