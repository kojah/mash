import { readFile } from "node:fs/promises";
import { expect, type Page, test } from "@playwright/test";
import { followRail } from "./shell";

const now = Date.now();
const chats = Array.from({ length: 60 }, (_, index) => ({
  chat_id: `chat-${index}`,
  node_id: index % 2 ? "ares" : "local",
  provider: index % 3 ? "codex" : "claude-code",
  title: `Conversation ${index}`,
  project: index < 30 ? "/work/mash" : "/work/other",
  updated_at: now - index * 60_000,
}));

async function mockHistory(page: Page): Promise<string[]> {
  const read: string[] = [];
  await page.route("**/api/records?**", (route) =>
    route.fulfill({ json: { records: [], next_offset: null } }),
  );
  await page.route("**/api/chats**", (route) =>
    route.fulfill({ json: { local_node: "local", truncated: false, chats } }),
  );
  await page.route("**/api/chat?**", (route) => {
    const params = new URL(route.request().url()).searchParams;
    const id = params.get("id") ?? "";
    if (params.get("offset") !== "0") return route.fulfill({ json: { messages: [] } });
    read.push(id);
    return route.fulfill({
      json: {
        messages: [
          { id: `${id}-m`, ordinal: 0, role: "user", content: `said in ${id}`, created_at: now },
        ],
      },
    });
  });
  return read;
}

/** The file names in a stored ZIP, read from its central directory. */
function names(bytes: Buffer): string[] {
  const end = bytes.length - 22;
  let at = bytes.readUInt32LE(end + 16);
  return Array.from({ length: bytes.readUInt16LE(end + 10) }, () => {
    const length = bytes.readUInt16LE(at + 28);
    const name = bytes.subarray(at + 46, at + 46 + length).toString("utf8");
    at += 46 + length;
    return name;
  });
}

for (const width of [390, 1280]) {
  test(`export downloads the filtered chats minus those left out at ${width}px`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 850 });
    const read = await mockHistory(page);
    await page.goto("/#/");
    await followRail(page, "import / export", "chats");
    await expect(page).toHaveURL(/#\/export$/);
    const main = page.getByRole("main");
    const status = main.getByRole("status").first();
    await expect(status).toContainText("60 chats will be exported");
    await expect(main.getByRole("navigation", { name: "Pages" })).toContainText(
      "1–25 of 60 chats · 25 of the 25 on this page included",
    );

    // Filters apply at once and live in the address.
    await main.getByRole("button", { name: "project" }).click();
    await main.getByRole("menuitemcheckbox", { name: /mash/ }).click();
    await expect(page).toHaveURL(/#\/export\?project=/);
    await page.keyboard.press("Escape");
    await expect(status).toContainText("30 chats will be exported");

    // A chat left out by hand stays out on the next page and back.
    await main.getByRole("checkbox", { name: /Conversation 1\b/ }).uncheck();
    await expect(status).toContainText("29 chats will be exported");
    await expect(status).toContainText("1 chat left out by hand");
    await main.getByRole("link", { name: "Next" }).click();
    await expect(main.getByRole("navigation", { name: "Pages" })).toContainText("26–30 of 30");
    await main.getByRole("link", { name: "Previous" }).click();
    await expect(main.getByRole("checkbox", { name: /Conversation 1\b/ })).not.toBeChecked();
    await main.getByRole("button", { name: "show them" }).click();
    await expect(main.getByRole("checkbox")).toHaveCount(1);
    await main.getByRole("button", { name: "show all" }).click();

    await main.getByRole("radio", { name: /JSON/ }).check();
    const download = page.waitForEvent("download");
    await main.getByRole("button", { name: "Export 29 chats" }).click();
    const saved = await download;
    expect(saved.suggestedFilename()).toMatch(/^mash-chats-\d{4}-\d\d-\d\d\.zip$/);
    await expect(main.getByText(/Exported 29 chats and 29 messages/)).toBeVisible();
    const files = names(await readFile(await saved.path()));
    expect(files).toHaveLength(29);
    expect(files.every((name) => /^mash\/\d{4}-\d\d-\d\d Conversation \d+\.json$/.test(name))).toBe(
      true,
    );
    expect(read).not.toContain("chat-1");
    expect(read).toHaveLength(29);

    // A full backup is a command for the machine, not a download.
    await main.getByRole("radio", { name: /Mash archive/ }).check();
    await expect(main.getByText("mash export ~/mash-backup")).toBeVisible();
    await expect(main.getByRole("button", { name: /^Export/ })).toHaveCount(0);

    await main.getByRole("link", { name: "import", exact: true }).click();
    await expect(page).toHaveURL(/#\/import$/);
    await expect(main.getByRole("link", { name: "Set up sharing" })).toBeVisible();
    // Coming back keeps the filter and the chat left out.
    await main.getByRole("link", { name: "export", exact: true }).click();
    await expect(status).toContainText("29 chats will be exported");
  });
}

test("a chat that cannot be read fails the export and saves nothing", async ({ page }) => {
  await mockHistory(page);
  await page.route("**/api/chat?**", (route) =>
    route.fulfill({ status: 404, json: { error: { message: "chat not found" } } }),
  );
  await page.goto("/#/export?machine=ares");
  const main = page.getByRole("main");
  let downloads = 0;
  page.on("download", () => {
    downloads += 1;
  });
  await main.getByRole("button", { name: "Export 30 chats" }).click();
  await expect(main.getByRole("alert")).toContainText("Nothing was saved");
  expect(downloads).toBe(0);
});
