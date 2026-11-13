import { expect, type Route, test } from "@playwright/test";

const body = (index: number): string =>
  `Message number ${index}. ${"A line of ordinary text that fills the page. ".repeat(12)}`;
const message = (index: number) => ({
  id: `m${index}`,
  ordinal: index,
  role: index % 2 ? "assistant" : "user",
  content: body(index),
});

test("an open chat follows new messages at the bottom and offers them elsewhere", async ({
  page,
}) => {
  let count = 30;
  let title = "Live fixture chat";
  await page.route("**/api/records?**", (route) =>
    route.fulfill({ json: { records: [], next_offset: null } }),
  );
  await page.route("**/api/chats**", (route) =>
    route.fulfill({
      json: {
        local_node: "local",
        truncated: false,
        chats: [
          {
            chat_id: "live",
            node_id: "local",
            provider: "codex",
            title,
            project: "/work/project",
            updated_at: Date.now(),
          },
        ],
      },
    }),
  );
  await page.route("**/api/chat?**", (route) =>
    route.fulfill({
      json: {
        message_count: count,
        messages: Array.from({ length: count }, (_, index) => message(index)),
      },
    }),
  );
  // Each stream is held open until the test ends it with what it should say.
  const streams: Route[] = [];
  await page.route("**/api/events", (route) => {
    streams.push(route);
  });
  const say = async (text: string): Promise<void> => {
    await expect.poll(() => streams.length, { timeout: 15_000 }).toBeGreaterThan(0);
    await streams.shift()?.fulfill({ contentType: "text/event-stream", body: text });
  };
  const change = (chats: readonly string[]): string =>
    `event: change\ndata: ${JSON.stringify({ node: null, revision: count, chats, all: false, records: false })}\n\n`;

  // The outline repeats each prompt, so look at the transcript itself.
  const shown = (index: number) => page.locator(`#message-m${index}`);

  await page.goto("/#/chat/live");
  await expect(shown(29)).toBeVisible();
  await expect(page.getByRole("heading", { name: "Live fixture chat" })).toBeVisible();

  // Reading earlier in the chat: the page stays as it is and the news is offered.
  await page.evaluate(() => window.scrollTo(0, 0));
  count = 31;
  await say(change(["someone-elses-chat", "live"]));
  const pill = page.getByRole("button", { name: "1 new message" });
  await expect(pill).toBeVisible();
  await expect(shown(30)).toHaveCount(0);
  expect(await page.evaluate(() => window.scrollY)).toBe(0);
  await pill.click();
  await expect(shown(30)).toBeInViewport();
  await expect(pill).toHaveCount(0);

  // That stream ended. The next one is a reconnection, which rereads
  // everything without being told what changed; at the bottom of the chat the
  // new message simply appears, and the chat list follows too.
  count = 32;
  title = "Renamed while open";
  await say(":\n\n");
  await expect(shown(31)).toBeInViewport();
  await expect(page.getByRole("button", { name: /new message/ })).toHaveCount(0);
  await expect(page.getByRole("heading", { name: "Renamed while open" })).toBeVisible();
});
