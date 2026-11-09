import { expect, test } from "@playwright/test";

test("hamburger routes update history without triggering fragment navigation", async ({ page }) => {
  let documents = 0;
  page.on("request", (request) => {
    if (request.resourceType() === "document") documents++;
  });
  await page.route("**/api/**", (route) =>
    route.fulfill({ json: { local_node: "local", chats: [], truncated: false } }),
  );
  await page.goto("/");
  await page.evaluate(() => {
    document.documentElement.dataset.fragmentNavigations = "0";
    document.querySelector("header")?.setAttribute("data-retained", "true");
    window.addEventListener("hashchange", () => {
      document.documentElement.dataset.fragmentNavigations = String(
        Number(document.documentElement.dataset.fragmentNavigations) + 1,
      );
    });
  });
  for (const [label, hash] of [
    ["Machines", "#/machines"],
    ["Agents", "#/agents"],
    ["Continue reading", "#/continue"],
    ["Recent conversations", "#/recent"],
    ["Memory", "#/records"],
    ["Decisions", "#/records?kind=decision"],
    ["Services", "#/services"],
    ["Home", "#/"],
  ] as const) {
    await page.getByRole("button", { name: "Open navigation" }).click();
    await page
      .getByRole("navigation", { name: "Main navigation" })
      .getByRole("link", { name: label, exact: true })
      .click();
    await expect(page.getByRole("dialog", { name: "Navigation" })).not.toBeVisible();
    expect(await page.evaluate(() => location.hash)).toBe(hash);
    await expect(page.locator(".app-header")).toHaveAttribute("data-retained", "true");
  }
  expect(await page.evaluate(() => document.documentElement.dataset.fragmentNavigations)).toBe("0");
  expect(documents).toBe(1);
  await page.goBack();
  await expect(page.getByRole("heading", { name: "Services", exact: true })).toBeVisible();
  await page.goForward();
  await expect(
    page.getByRole("heading", { name: "Recent conversations", exact: true }),
  ).toBeVisible();
  expect(documents).toBe(1);
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

test("index status lives on the services overview", async ({ page }) => {
  await page.route("**/api/records?**", (route) =>
    route.fulfill({ json: { records: [], next_offset: null } }),
  );
  await page.route("**/api/services/access", (route) =>
    route.fulfill({ json: { authentication_required: false, local_node: "xenia" } }),
  );
  await page.route("**/api/services", (route) => route.fulfill({ json: [] }));
  await page.route("**/api/chats**", (route) =>
    route.fulfill({
      json: { local_node: "xenia", truncated: false, chats: [], shared_error: "shared offline" },
    }),
  );
  await page.goto("/#/status");
  await expect(page.getByRole("heading", { name: "Services", exact: true })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Index", exact: true })).toBeVisible();
  await expect(page.getByText("Local node: xenia")).toBeVisible();
  await expect(page.getByRole("link", { name: "Index status" })).toHaveCount(0);
});
