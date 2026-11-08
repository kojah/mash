import { expect, test } from "@playwright/test";

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
