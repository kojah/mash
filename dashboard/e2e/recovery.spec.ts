import { expect, test } from "@playwright/test";

test("opening chats while offline settles and retries without a document reload", async ({
  page,
  context,
}) => {
  let documents = 0;
  page.on("request", (request) => {
    if (request.resourceType() === "document") documents++;
  });
  await page.route("**/api/services/access", (route) =>
    route.fulfill({ json: { authentication_required: false, local_node: "local" } }),
  );
  await page.route("**/api/services", (route) => route.fulfill({ json: [] }));
  await page.goto("/#/services");
  await expect(page.getByRole("heading", { name: "Services", exact: true })).toBeVisible();
  await context.setOffline(true);
  await page.getByRole("link", { name: "Home", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("Could not reach Mash");
  await expect(page.getByText("Loading chats…", { exact: true })).toHaveCount(0);
  await context.setOffline(false);
  await page.route("**/api/chats**", (route) =>
    route.fulfill({ json: { local_node: "local", truncated: false, chats: [] } }),
  );
  await page.getByRole("button", { name: "Retry", exact: true }).click();
  await expect(page.getByRole("alert")).toHaveCount(0);
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
    await page.route("**/api/services/access", (route) =>
      route.fulfill({ json: { authentication_required: false, local_node: "local" } }),
    );
    await page.route("**/api/services", (route) => route.fulfill({ json: [] }));
    await page.route("**/api/chats**", (route) => route.abort("connectionfailed"));
    await page.goto("/");
    const alert = page.getByRole("alert");
    await expect(alert).toContainText("Couldn’t load chats");
    await expect(alert).toContainText("Could not reach Mash");
    await expect(page.getByRole("heading", { name: "Memory", exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: "Open navigation" })).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(
      true,
    );
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
      await route.fulfill({ json: { local_node: "local", truncated: false, chats: [] } });
    });
    await alert.getByRole("button", { name: "Retry", exact: true }).click();
    await expect(alert.getByRole("button", { name: "Trying again…" })).toBeDisabled();
    release();
    await expect(alert).toHaveCount(0);
    await expect(page.getByText("No indexed chats yet.", { exact: false })).toBeVisible();
    expect(documents).toBe(1);
  });
}
