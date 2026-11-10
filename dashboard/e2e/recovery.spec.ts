import { expect, type Page, test } from "@playwright/test";
import { fitsViewport, followRail, rail } from "./shell";

async function mockServices(page: Page): Promise<void> {
  await page.route("**/api/services/access", (route) =>
    route.fulfill({ json: { authentication_required: false, local_node: "local" } }),
  );
  await page.route("**/api/services", (route) => route.fulfill({ json: { machines: [] } }));
  await page.route("**/api/services/operations**", (route) => route.fulfill({ json: [] }));
}
const emptyCatalogue = { local_node: "local", truncated: false, chats: [] };
const chatsAlert = (page: Page) =>
  page.getByRole("alert").filter({ hasText: "Couldn’t load chats" });

test("opening chats while offline settles and retries without a document reload", async ({
  page,
  context,
}) => {
  let documents = 0;
  page.on("request", (request) => {
    if (request.resourceType() === "document") documents++;
  });
  await mockServices(page);
  let online = true;
  await page.route("**/api/chats**", (route) =>
    online ? route.fulfill({ status: 503, body: "starting" }) : route.abort("internetdisconnected"),
  );
  await page.route("**/api/records?**", (route) =>
    online
      ? route.fulfill({ json: { records: [], next_offset: null } })
      : route.abort("internetdisconnected"),
  );
  await page.goto("/#/services");
  await expect(page.getByRole("heading", { name: "Services", exact: true })).toBeVisible();
  online = false;
  await context.setOffline(true);
  await followRail(page, "overview");
  await expect(chatsAlert(page)).toContainText("Could not reach Mash");
  await expect(page.getByRole("status", { name: "Loading" })).toHaveCount(0);
  await expect(page.getByText("Loading chats…", { exact: true })).toHaveCount(0);
  await context.setOffline(false);
  online = true;
  await page.route("**/api/chats**", (route) => route.fulfill({ json: emptyCatalogue }));
  await chatsAlert(page).getByRole("button", { name: "Retry", exact: true }).click();
  await expect(chatsAlert(page)).toHaveCount(0);
  await expect(page.getByText("No indexed chats yet.", { exact: false })).toBeVisible();
  expect(documents).toBe(1);
});

for (const width of [390, 1440]) {
  test(`dropped chat connection has clear recovery without losing the shell at ${width}px`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 });
    let documents = 0;
    page.on("request", (request) => {
      if (request.resourceType() === "document") documents++;
    });
    await mockServices(page);
    await page.route("**/api/records?**", (route) =>
      route.fulfill({ json: { records: [], next_offset: null } }),
    );
    await page.route("**/api/chats**", (route) => route.abort("connectionfailed"));
    await page.goto("/");
    const alert = page.getByRole("alert");
    await expect(alert).toContainText("Couldn’t load chats");
    await expect(alert).toContainText("Could not reach Mash");
    // Memory and the navigation stay usable while chat history is unreachable.
    await expect(page.getByRole("heading", { name: "recent memories", exact: true })).toBeVisible();
    await expect(page.getByText(/Nothing saved yet/)).toBeVisible();
    if (width < 800)
      await expect(page.getByRole("button", { name: "Open navigation" })).toBeVisible();
    else await expect(rail(page).getByRole("link", { name: "memory", exact: true })).toBeVisible();
    expect(await fitsViewport(page)).toBe(true);
    await page.screenshot({
      path: test.info().outputPath(`recovery-${width}.png`),
      fullPage: true,
    });
    let release: () => void = () => {};
    const ready = new Promise<void>((resolve) => {
      release = resolve;
    });
    await page.route("**/api/chats**", async (route) => {
      await ready;
      await route.fulfill({ json: emptyCatalogue });
    });
    await alert.getByRole("button", { name: "Retry", exact: true }).click();
    // The retry is in flight: nothing claims an empty history yet.
    await expect(page.getByRole("button", { name: "Retry", exact: true })).toHaveCount(0);
    await expect(page.getByText("No indexed chats yet.", { exact: false })).toHaveCount(0);
    release();
    await expect(alert).toHaveCount(0);
    await expect(page.getByText("No indexed chats yet.", { exact: false })).toBeVisible();
    expect(documents).toBe(1);
  });
}

// The earlier home page kept the error in place while a retry was in flight,
// with its button disabled and reading "Trying again…". Overview.svelte (and
// the other pages that pass busy={query.isFetching} to LoadError) render the
// error only while the query has one, and TanStack clears a first-load error
// the moment a refetch starts. The alert is therefore swapped for loading
// placeholders, LoadError's busy state is never shown, and a repeated failure
// announces a new alert each time.
test("a retry in flight keeps the error in place with a disabled button", async ({ page }) => {
  await mockServices(page);
  await page.route("**/api/records?**", (route) =>
    route.fulfill({ json: { records: [], next_offset: null } }),
  );
  await page.route("**/api/chats**", (route) => route.abort("connectionfailed"));
  await page.goto("/");
  const alert = chatsAlert(page);
  await expect(alert).toContainText("Could not reach Mash");
  let release: () => void = () => {};
  const ready = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route("**/api/chats**", async (route) => {
    await ready;
    await route.fulfill({ json: emptyCatalogue });
  });
  await alert.getByRole("button", { name: "Retry", exact: true }).click();
  await expect(alert.getByRole("button", { name: "Trying again…" })).toBeDisabled();
  release();
  await expect(alert).toHaveCount(0);
});
